/* MS07 QEMU recovery probe decision core.  The pure transition checks below
 * are shared by the guest payload and the host C harness; the actual QEMU
 * choreography remains manual in Iteration 007. */
#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <arpa/inet.h>
#include <fcntl.h>
#include <limits.h>
#include <poll.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

#define MS07_ENVIRONMENT_DEFAULT "qemu-virt-riscv64-single-hart-virtio-mmio-user-net"
#define MS07_SNAPSHOT_V4 0x4e494434u
#define MS07_RESET_REQUEST 0x4e495231u
#define MS07_PEER_PORT 15572u
#define MS07_PHASE_DEADLINE_MS 30000u
#define MS07_OVERALL_DEADLINE_MS 180000u
#define MS07_OPERATOR_DEADLINE_MS 300000u
#define MS07_V3_RX_LIFECYCLE_INDEX 10u

#define MS07_LINK_DOWN 0u
#define MS07_LINK_UP 1u

/* Healthy VirtIO owner baseline (Iteration 007 / P1): the fixed single-hart
 * VirtIO-MMIO model fills `QS` resident RX owners and `QS` free TX buffers, so
 * the no-traffic idle tuple is `available==device_owned==QS` and `quarantine=0`.
 * `device_owned==0` is never a healthy state (it would mean RX owners are
 * absent); `available==device_owned` only holds when no packet is in flight. */
#define MS07_OWNER_SLOTS 64u

enum ms07_terminal_kind {
    MS07_TERMINAL_RESET = 0,
    MS07_TERMINAL_LINK_DOWN,
    MS07_TERMINAL_DEADLINE,
    MS07_TERMINAL_CANCEL
};

/* This is the V4 current tuple consumed by the guest decision layer.  The
 * wire additionally carries a separate historical-fault tuple; no zero value
 * is used as an invalid epoch sentinel. */
struct ms07_v4_observation {
    uint64_t lifecycle;
    uint64_t current_valid;
    uint64_t current_queue_epoch;
    uint64_t current_socket_epoch;
    uint64_t current_link_generation;
    uint64_t current_link_state;
    uint64_t owner_available;
    uint64_t owner_device_owned;
    uint64_t owner_quarantined;
    uint64_t fault_valid;
    uint64_t fault_queue_epoch;
};

int ms07_observation_stable(const struct ms07_v4_observation *first,
                            const struct ms07_v4_observation *second)
{
    return first != NULL && second != NULL &&
           memcmp(first, second, sizeof(*first)) == 0;
}

int ms07_stable_candidate_step(struct ms07_v4_observation *candidate,
                               const struct ms07_v4_observation *current,
                               int *have_candidate)
{
    if (candidate == NULL || current == NULL || have_candidate == NULL) return -1;
    if (!current->current_valid || current->lifecycle != 2) {
        *have_candidate = 0;
        return 0;
    }
    if (*have_candidate && ms07_observation_stable(candidate, current)) return 1;
    *candidate = *current;
    *have_candidate = 1;
    return 0;
}

int ms07_deadline_expired(uint64_t start_ms, uint64_t now_ms, uint64_t budget_ms)
{
    return now_ms < start_ms || now_ms - start_ms >= budget_ms;
}

int ms07_deadline_remaining(uint64_t now_ms, uint64_t deadline_ms, int *remaining_ms)
{
    uint64_t remaining;
    if (remaining_ms == NULL || now_ms >= deadline_ms) return -1;
    remaining = deadline_ms - now_ms;
    *remaining_ms = remaining > (uint64_t)INT_MAX ? INT_MAX : (int)remaining;
    return 0;
}

/* A3: post-wait deadline re-check.  A blocking wait that returns right at (or
 * after) the deadline must be treated as expired, never as a fresh success.
 * `now_ms` is injected so the host test can prove a "readable at deadline"
 * poll that has already exhausted its budget does not count as success. */
int ms07_wait_token_ok(uint64_t wake_ms, uint64_t deadline_ms)
{
    return wake_ms < deadline_ms;
}

/* A3: final I/O boundary decision, applied AFTER a wait has returned success
 * and immediately BEFORE the untrusted `send`/`recv(MSG_DONTWAIT)` syscall.
 * A wait that returned readable/writable at or after the absolute deadline
 * must not be followed by a syscall that consumes the budget, so the probe
 * refuses to issue I/O once the clock has crossed the deadline.  Pure so the
 * host test can drive a fake clock across the boundary. */
int ms07_io_allowed(uint64_t now_ms, uint64_t deadline_ms)
{
    return ms07_wait_token_ok(now_ms, deadline_ms);
}

/* A3: single iteration of the bounded `wait_fd` decision, decoupled from the
 * real `poll`/clock so the host test can drive a late-readable and two-terminal
 * exhaustion sequence under a fake clock.  Returns 1 when the requested event
 * is present AND its wake time is still before the deadline (success), 0 when
 * the wait must fail because the budget is exhausted at the wake, and -1 on a
 * hard poll error.  When 0, the caller re-polls only if `again` is non-NULL and
 * set to 1 (the wake held no wanted event still within budget); a wake at/after
 * the deadline sets `again` to 0 so the wait fails without re-polling.
 * `revents`/`now` are injected; `want` selects the POLL* event bits. */
int ms07_wait_step(int poll_result, short revents, short want, uint64_t now_ms_arg,
                   uint64_t deadline_ms, int *again)
{
    if (poll_result < 0) return -1;
    if (poll_result == 0) { *again = 1; return 0; }
    if (revents & want) {
        /* A wanted event (e.g. POLLERR/POLLHUP as a terminal read) is success
         * only if its wake is still within the deadline; a wake at/after the
         * deadline is stale and must fail. */
        *again = ms07_wait_token_ok(now_ms_arg, deadline_ms) ? 1 : 0;
        return *again;
    }
    if (revents & (POLLERR | POLLHUP | POLLNVAL)) return -1;
    *again = 1;
    return 0;
}

int ms07_reset_transition_valid(const struct ms07_v4_observation *before,
                                const struct ms07_v4_observation *after)
{
    if (before == NULL || after == NULL || !before->current_valid || !after->current_valid)
        return 0;
    if (before->lifecycle != 2 || after->lifecycle != 2)
        return 0;
    if (before->current_link_state != MS07_LINK_UP || after->current_link_state != MS07_LINK_UP)
        return 0;
    if (before->owner_quarantined || after->owner_quarantined)
        return 0;
    return after->current_queue_epoch == before->current_queue_epoch + 1 &&
           after->current_socket_epoch == before->current_socket_epoch + 1 &&
           after->current_link_generation == before->current_link_generation;
}

int ms07_link_down_transition_valid(const struct ms07_v4_observation *before,
                                    const struct ms07_v4_observation *after)
{
    if (before == NULL || after == NULL || !before->current_valid || !after->current_valid)
        return 0;
    if (before->lifecycle != 2 || after->lifecycle != 2)
        return 0;
    /* A5: a link flap does not own or release packet slots, so `available`
     * AND `device_owned` must be conserved across the down transition. */
    if (before->owner_available != after->owner_available ||
        before->owner_device_owned != after->owner_device_owned)
        return 0;
    return before->current_link_state == MS07_LINK_UP &&
           after->current_link_state == MS07_LINK_DOWN &&
           after->current_queue_epoch == before->current_queue_epoch &&
           after->current_socket_epoch == before->current_socket_epoch &&
           after->current_link_generation == before->current_link_generation + 1;
}

int ms07_terminal_errno_valid(enum ms07_terminal_kind kind, int err)
{
    switch (kind) {
    case MS07_TERMINAL_RESET: return err == ECONNRESET;
    case MS07_TERMINAL_LINK_DOWN: return err == ENOTCONN;
    case MS07_TERMINAL_DEADLINE: return err == ETIMEDOUT;
    case MS07_TERMINAL_CANCEL: return err == EINTR;
    }
    return 0;
}

static const char *const ms07_cases[] = {
    "pre_reset_traffic", "reset_request", "old_socket_terminal",
    "new_epoch_traffic", "hmp_link_down", "hmp_link_up",
};

static const char *const ms07_schema[] = {
    "pre_reset_traffic:MS07_V4,MS07_PEER",
    "reset_request:MS07_RESET",
    "old_socket_terminal:MS07_V4,MS07_SOCKET",
    "new_epoch_traffic:MS07_V4,MS07_SOCKET,MS07_PEER",
    "hmp_link_down:MS07_HMP_READY,MS07_HMP_OBSERVED,MS07_V4,MS07_SOCKET",
    "hmp_link_up:MS07_HMP_READY,MS07_HMP_OBSERVED,MS07_V4,MS07_SOCKET,MS07_PEER",
};

int ms07_probe_decision_core_self_test(void) {
    return sizeof(ms07_cases) / sizeof(ms07_cases[0]) == 6 &&
        sizeof(ms07_schema) / sizeof(ms07_schema[0]) == 6 &&
        strcmp(ms07_cases[0], "pre_reset_traffic") == 0 &&
        strcmp(ms07_cases[5], "hmp_link_up") == 0 &&
        strcmp(ms07_schema[0], "pre_reset_traffic:MS07_V4,MS07_PEER") == 0 &&
        ms07_terminal_errno_valid(MS07_TERMINAL_RESET, ECONNRESET) &&
        !ms07_terminal_errno_valid(MS07_TERMINAL_RESET, ENOTCONN);
}

/* Pure V4 wire observer shared by the guest decision layer and the host C
 * harness: proves the owner is Active at the expected queue/socket epoch with
 * the healthy VirtIO owner baseline (available==device_owned==expected, no
 * quarantine).  `expected` is the QS fixed capacity observed pre-reset, so
 * `device_owned==0` is never accepted as a healthy observation. */
int ms07_drained_epoch_ok(const struct ms07_v4_observation *obs, uint64_t q, uint64_t s,
                          uint64_t expected_available)
{
    return obs != NULL && obs->current_valid && obs->lifecycle == 2 &&
           obs->current_queue_epoch == q && obs->current_socket_epoch == s &&
           obs->owner_available == expected_available &&
           obs->owner_device_owned == expected_available &&
           obs->owner_quarantined == 0;
}

/* V3 is a frozen 72-u64 prefix.  V4 only appends these two independently
 * valid tuples, matching `IrqSnapshotV4` without repurposing a V3 byte. */
struct ms07_snapshot_v4_wire {
    uint64_t v3[72];
    uint64_t current_valid, current_queue_epoch, current_socket_epoch;
    uint64_t current_link_generation, current_link_state;
    uint64_t current_owner_available, current_owner_device_owned, current_owner_quarantined;
    uint64_t fault_valid, fault_stage, fault_cause, fault_queue_epoch;
    uint64_t fault_owner_available, fault_owner_device_owned, fault_owner_quarantined;
};

/* Compiled in both the probe and the host test so the wire layout is mutation-
 * checked wherever the decision core is. */
_Static_assert(offsetof(struct ms07_snapshot_v4_wire, current_valid) == 72u * sizeof(uint64_t),
               "V4 must keep V3 as its byte-for-byte prefix");
_Static_assert(offsetof(struct ms07_snapshot_v4_wire, current_queue_epoch) == 73u * sizeof(uint64_t), "V4 current_queue_epoch offset");
_Static_assert(offsetof(struct ms07_snapshot_v4_wire, current_socket_epoch) == 74u * sizeof(uint64_t), "V4 current_socket_epoch offset");
_Static_assert(offsetof(struct ms07_snapshot_v4_wire, current_link_generation) == 75u * sizeof(uint64_t), "V4 current_link_generation offset");
_Static_assert(offsetof(struct ms07_snapshot_v4_wire, current_link_state) == 76u * sizeof(uint64_t), "V4 current_link_state offset");
_Static_assert(offsetof(struct ms07_snapshot_v4_wire, current_owner_available) == 77u * sizeof(uint64_t), "V4 current_owner_available offset");
_Static_assert(offsetof(struct ms07_snapshot_v4_wire, current_owner_device_owned) == 78u * sizeof(uint64_t), "V4 current_owner_device_owned offset");
_Static_assert(offsetof(struct ms07_snapshot_v4_wire, current_owner_quarantined) == 79u * sizeof(uint64_t), "V4 current_owner_quarantined offset");
_Static_assert(offsetof(struct ms07_snapshot_v4_wire, fault_valid) == 80u * sizeof(uint64_t),
               "V4 fault tuple must begin at wire field 80");
_Static_assert(offsetof(struct ms07_snapshot_v4_wire, fault_stage) == 81u * sizeof(uint64_t), "V4 fault_stage offset");
_Static_assert(offsetof(struct ms07_snapshot_v4_wire, fault_cause) == 82u * sizeof(uint64_t), "V4 fault_cause offset");
_Static_assert(offsetof(struct ms07_snapshot_v4_wire, fault_queue_epoch) == 83u * sizeof(uint64_t), "V4 fault_queue_epoch offset");
_Static_assert(offsetof(struct ms07_snapshot_v4_wire, fault_owner_available) == 84u * sizeof(uint64_t), "V4 fault_owner_available offset");
_Static_assert(offsetof(struct ms07_snapshot_v4_wire, fault_owner_device_owned) == 85u * sizeof(uint64_t), "V4 fault_owner_device_owned offset");
_Static_assert(offsetof(struct ms07_snapshot_v4_wire, fault_owner_quarantined) == 86u * sizeof(uint64_t), "V4 fault_owner_quarantined offset");
_Static_assert(sizeof(struct ms07_snapshot_v4_wire) == 87u * sizeof(uint64_t),
               "C V4 tail must match the Rust append-only wire");

#ifndef MS07_RECOVERY_PROBE_TESTING
static int now_ms(uint64_t *out)
{
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) return -1;
    *out = (uint64_t)ts.tv_sec * 1000u + (uint64_t)ts.tv_nsec / 1000000u;
    return 0;
}

static int read_v4(struct ms07_snapshot_v4_wire *wire, struct ms07_v4_observation *out)
{
    memset(wire, 0, sizeof(*wire));
    if (ioctl(STDIN_FILENO, MS07_SNAPSHOT_V4, wire) < 0) return -1;
    out->lifecycle = wire->v3[MS07_V3_RX_LIFECYCLE_INDEX];
    out->current_valid = wire->current_valid;
    out->current_queue_epoch = wire->current_queue_epoch;
    out->current_socket_epoch = wire->current_socket_epoch;
    out->current_link_generation = wire->current_link_generation;
    out->current_link_state = wire->current_link_state;
    out->owner_available = wire->current_owner_available;
    out->owner_device_owned = wire->current_owner_device_owned;
    out->owner_quarantined = wire->current_owner_quarantined;
    out->fault_valid = wire->fault_valid;
    out->fault_queue_epoch = wire->fault_queue_epoch;
    return 0;
}

static void print_v4(const char *case_name, const struct ms07_snapshot_v4_wire *wire)
{
    printf("MS07_V4: case=%s lifecycle=%llu current_valid=%llu q=%llu s=%llu l=%llu link=%s available=%llu device_owned=%llu quarantined=%llu fault_valid=%llu fault_stage=%llu fault_cause=%llu fault_q=%llu fault_available=%llu fault_device_owned=%llu fault_quarantined=%llu\n",
           case_name,
           (unsigned long long)wire->v3[MS07_V3_RX_LIFECYCLE_INDEX],
           (unsigned long long)wire->current_valid,
           (unsigned long long)wire->current_queue_epoch,
           (unsigned long long)wire->current_socket_epoch,
           (unsigned long long)wire->current_link_generation,
           wire->current_link_state == MS07_LINK_UP ? "up" : "down",
           (unsigned long long)wire->current_owner_available,
           (unsigned long long)wire->current_owner_device_owned,
           (unsigned long long)wire->current_owner_quarantined,
           (unsigned long long)wire->fault_valid,
           (unsigned long long)wire->fault_stage,
           (unsigned long long)wire->fault_cause,
           (unsigned long long)wire->fault_queue_epoch,
           (unsigned long long)wire->fault_owner_available,
(unsigned long long)wire->fault_owner_device_owned,
            (unsigned long long)wire->fault_owner_quarantined);
}

static int next_stable_observation(struct ms07_snapshot_v4_wire *wire,
                                   struct ms07_v4_observation *previous,
                                   struct ms07_v4_observation *current,
                                   int *have_previous)
{
    if (read_v4(wire, current) != 0 || current->lifecycle == 3) return -1;
    return ms07_stable_candidate_step(previous, current, have_previous);
}

static int make_deadline(uint64_t overall_deadline, uint64_t budget_ms, uint64_t *deadline)
{
    uint64_t now, candidate;
    if (deadline == NULL || now_ms(&now) != 0 || now >= overall_deadline) return -1;
    candidate = budget_ms > UINT64_MAX - now ? UINT64_MAX : now + budget_ms;
    *deadline = candidate < overall_deadline ? candidate : overall_deadline;
    return *deadline > now ? 0 : -1;
}

static int wait_until_sample(uint64_t deadline)
{
    uint64_t now;
    int remaining;
    if (now_ms(&now) != 0 || ms07_deadline_remaining(now, deadline, &remaining) != 0) return -1;
    if (remaining > 20) remaining = 20;
    return poll(NULL, 0, remaining) < 0 && errno != EINTR ? -1 : 0;
}

static int wait_fd(int fd, short events, uint64_t deadline)
{
    struct pollfd pfd = { .fd = fd, .events = events };
    uint64_t now;
    int remaining, result;
    int again;
    for (;;) {
        if (now_ms(&now) != 0 || ms07_deadline_remaining(now, deadline, &remaining) != 0)
            return -1;
        pfd.revents = 0;
        result = poll(&pfd, 1, remaining);
        if (result < 0 && errno == EINTR) continue;
        /* A3: re-sample the clock AFTER poll returns.  A wake observed at or
         * after the absolute deadline is stale and must fail, even if the idle
         * wait returned a readable fd right at the deadline.  The decision is
         * shared with the host-tested `ms07_wait_step` so the timeout rule is
         * the same under the injected fake clock and the real poll. */
        if (now_ms(&now) != 0) return -1;
        {
            int step = ms07_wait_step(result, pfd.revents, events, now, deadline, &again);
            if (step < 0) return -1;
            if (step == 1) return 0;
            if (again) continue;
            return -1;
        }
    }
}

static int wait_for_pre_reset(struct ms07_snapshot_v4_wire *wire,
                              struct ms07_v4_observation *pre, uint64_t deadline)
{
    struct ms07_v4_observation previous;
    int have_previous = 0;
    for (;;) {
        int stable = next_stable_observation(wire, &previous, pre, &have_previous);
        if (stable < 0) return -1;
        if (stable > 0 && pre->current_link_state == MS07_LINK_UP &&
            pre->owner_available == MS07_OWNER_SLOTS &&
            pre->owner_device_owned == MS07_OWNER_SLOTS &&
            pre->owner_quarantined == 0) return 0;
        if (wait_until_sample(deadline) != 0) return -1;
    }
}

static int wait_for_reset(const struct ms07_v4_observation *before,
                          struct ms07_snapshot_v4_wire *wire,
                          struct ms07_v4_observation *after, uint64_t deadline)
{
    struct ms07_v4_observation previous;
    int have_previous = 0;
    uint64_t now;
    while (now_ms(&now) == 0 && now < deadline) {
        int stable = next_stable_observation(wire, &previous, after, &have_previous);
        if (stable < 0) return -1;
        if (stable > 0 && ms07_reset_transition_valid(before, after)) return 0;
        if (wait_until_sample(deadline) != 0) return -1;
    }
    return -1;
}

static int wait_for_link_down(const struct ms07_v4_observation *before,
                              struct ms07_snapshot_v4_wire *wire,
                              struct ms07_v4_observation *after, uint64_t deadline)
{
    struct ms07_v4_observation previous;
    int have_previous = 0;
    uint64_t now;
    while (now_ms(&now) == 0 && now < deadline) {
        int stable = next_stable_observation(wire, &previous, after, &have_previous);
        if (stable < 0) return -1;
        if (stable > 0 && ms07_link_down_transition_valid(before, after)) return 0;
        if (wait_until_sample(deadline) != 0) return -1;
    }
    return -1;
}

static int wait_for_link_up(const struct ms07_v4_observation *before,
                            const struct ms07_v4_observation *down,
                            struct ms07_snapshot_v4_wire *wire,
                            struct ms07_v4_observation *after, uint64_t deadline)
{
    struct ms07_v4_observation previous;
    int have_previous = 0;
    uint64_t now;
    while (now_ms(&now) == 0 && now < deadline) {
        int stable = next_stable_observation(wire, &previous, after, &have_previous);
        if (stable < 0) return -1;
        if (stable > 0 && after->current_valid && after->lifecycle == 2 &&
            after->current_queue_epoch == before->current_queue_epoch &&
            after->current_socket_epoch == before->current_socket_epoch + 1 &&
            after->current_link_generation == down->current_link_generation + 1 &&
            after->current_link_state == MS07_LINK_UP && !after->owner_quarantined &&
            /* A5: a link flap does not own or release packet slots. */
            after->owner_available == down->owner_available &&
            after->owner_device_owned == down->owner_device_owned) return 0;
        if (wait_until_sample(deadline) != 0) return -1;
    }
    return -1;
}

/* A3 rework: proves the resident owner is Active at the expected queue/socket
 * epoch with the DeviceOwned ledger drained (zero in-flight, no quarantine). */
static int wait_for_drained_active(struct ms07_snapshot_v4_wire *wire,
                                   const struct ms07_v4_observation *target,
                                   uint64_t expected_available, uint64_t deadline,
                                   struct ms07_v4_observation *out)
{
    struct ms07_v4_observation previous;
    int have_previous = 0;
    for (;;) {
        int stable = next_stable_observation(wire, &previous, out, &have_previous);
        if (stable < 0) return -1;
        if (stable > 0 &&
            ms07_drained_epoch_ok(out, target->current_queue_epoch,
                                  target->current_socket_epoch, expected_available))
            return 0;
        if (wait_until_sample(deadline) != 0) return -1;
    }
}

static int peer_exchange(int fd, const char *phase, uint64_t deadline)
{
    char payload[192];
    int n = snprintf(payload, sizeof(payload), "phase=%s seq=0", phase);
    ssize_t sent;
    uint64_t now;
    if (n < 0 || (size_t)n >= sizeof(payload)) return -1;
    if (wait_fd(fd, POLLOUT, deadline) != 0) {
        printf("DBG: peer_socket stage=pollout phase=%s errno=%d\n", phase, errno);
        return -1;
    }
    if (now_ms(&now) != 0 || !ms07_io_allowed(now, deadline)) return -1;
    do sent = send(fd, payload, (size_t)n, MSG_DONTWAIT); while (sent < 0 && errno == EINTR);
    if (sent != n) {
        printf("DBG: peer_socket stage=send phase=%s errno=%d sent=%zd want=%d\n",
               phase, errno, sent, n);
        return -1;
    }
    if (wait_fd(fd, POLLIN, deadline) != 0) {
        printf("DBG: peer_socket stage=recv-wait phase=%s errno=%d\n", phase, errno);
        return -1;
    }
    if (now_ms(&now) != 0 || !ms07_io_allowed(now, deadline)) return -1;
    char reply[sizeof(payload)];
    ssize_t got;
    do got = recv(fd, reply, sizeof(reply), MSG_DONTWAIT); while (got < 0 && errno == EINTR);
    if (got != n) {
        printf("DBG: peer_socket stage=recv phase=%s errno=%d got=%zd want=%d\n",
               phase, errno, got, n);
        return -1;
    }
    return memcmp(payload, reply, (size_t)n) == 0 ? 0 : -1;
}

static int open_peer_socket(void)
{
    /* P3 / R8: create the socket non-blocking directly (the kernel supports
     * `O_NONBLOCK` in the socket type), then handle the socket() and connect()
     * failures separately so raw serial can name the exact failing stage. */
    int fd = socket(AF_INET, SOCK_DGRAM | O_NONBLOCK, 0);
    struct sockaddr_in peer;
    if (fd < 0) {
        printf("DBG: peer_socket stage=socket errno=%d\n", errno);
        return -1;
    }
    memset(&peer, 0, sizeof(peer));
    peer.sin_family = AF_INET;
    peer.sin_port = htons(MS07_PEER_PORT);
    peer.sin_addr.s_addr = htonl(0x0a000202u); /* QEMU user-net host */
    if (connect(fd, (const struct sockaddr *)&peer, sizeof(peer)) != 0) {
        printf("DBG: peer_socket stage=connect errno=%d\n", errno);
        close(fd);
        return -1;
    }
    return fd;
}

static int expect_terminal(int fd, int expected, uint64_t deadline)
{
    uint64_t now;
    if (wait_fd(fd, POLLIN | POLLERR | POLLHUP, deadline) != 0) return -1;
    if (now_ms(&now) != 0 || !ms07_io_allowed(now, deadline)) return -1;
    char byte;
    if (recv(fd, &byte, sizeof(byte), MSG_DONTWAIT) >= 0) return -1;
    return errno == expected ? 0 : -1;
}

static int expect_terminal_twice(int fd, int expected, uint64_t deadline)
{
    return expect_terminal(fd, expected, deadline) == 0 &&
           expect_terminal(fd, expected, deadline) == 0 ? 0 : -1;
}

static int fail_case(const char *case_name, const char *reason)
{
    printf("FAIL: %s reason=%s\n", case_name, reason);
    return 1;
}

static int run_probe(void)
{
    struct ms07_snapshot_v4_wire wire;
    struct ms07_v4_observation pre, reset, down, up;
    int old_socket = -1, new_socket = -1, newest_socket = -1;
    uint64_t now, overall_deadline, operator_deadline, deadline;
    printf("MS07_RECOVERY_START\nMS07_ENVIRONMENT: %s\n", MS07_ENVIRONMENT_DEFAULT);
    if (now_ms(&now) != 0 || MS07_OVERALL_DEADLINE_MS > UINT64_MAX - now)
        return fail_case("setup", "clock");
    overall_deadline = now + MS07_OVERALL_DEADLINE_MS;
    printf("MS07_CASE_START: pre_reset_traffic\n");
    /* Diagnostic (next-cycle root-cause): dump the raw V4 snapshot on the first
     * read so an instant "precondition" failure can be attributed to the ioctl
     * (errno), the owner lifecycle, or the link/drain contract. `DBG:` is serial
     * noise to the validator, which only consumes MS07_/PASS/FAIL markers. */
    {
        struct ms07_snapshot_v4_wire d_wire;
        struct ms07_v4_observation d_obs = {0};
        errno = 0;
        int dr = read_v4(&d_wire, &d_obs);
        printf("DBG: read_v4=%d errno=%d lifecycle=%llu current_valid=%llu q=%llu s=%llu l=%llu link=%llu avail=%llu dev=%llu quar=%llu\n",
               dr, errno,
               (unsigned long long)d_obs.lifecycle,
               (unsigned long long)d_obs.current_valid,
               (unsigned long long)d_obs.current_queue_epoch,
               (unsigned long long)d_obs.current_socket_epoch,
               (unsigned long long)d_obs.current_link_generation,
               (unsigned long long)d_obs.current_link_state,
               (unsigned long long)d_obs.owner_available,
               (unsigned long long)d_obs.owner_device_owned,
               (unsigned long long)d_obs.owner_quarantined);
    }
    if (make_deadline(overall_deadline, MS07_PHASE_DEADLINE_MS, &deadline) != 0)
        return fail_case("pre_reset_traffic", "deadline");
    old_socket = open_peer_socket();
    if (old_socket < 0 || wait_for_pre_reset(&wire, &pre, deadline) != 0 ||
        peer_exchange(old_socket, "pre_reset_traffic", deadline) != 0)
        return fail_case("pre_reset_traffic", "precondition");
    print_v4("pre_reset_traffic", &wire);
    printf("MS07_PEER: case=pre_reset_traffic result=ok\nPASS: pre_reset_traffic\n");
    printf("MS07_CASE_START: reset_request\n");
    if (ioctl(STDIN_FILENO, MS07_RESET_REQUEST, 0) < 0) return fail_case("reset_request", "ioctl");
    if (ioctl(STDIN_FILENO, MS07_RESET_REQUEST, 0) == 0 || errno != EAGAIN) return fail_case("reset_request", "duplicate");
    printf("MS07_RESET: accepted=1 duplicate=EAGAIN\nPASS: reset_request\n");
    if (make_deadline(overall_deadline, MS07_PHASE_DEADLINE_MS, &deadline) != 0)
        return fail_case("old_socket_terminal", "deadline");
    printf("MS07_CASE_START: old_socket_terminal\n");
    if (wait_for_reset(&pre, &wire, &reset, deadline) != 0 ||
        expect_terminal_twice(old_socket, ECONNRESET, deadline) != 0)
        return fail_case("old_socket_terminal", "reset-terminal");
    print_v4("old_socket_terminal", &wire);
    printf("MS07_SOCKET: case=old_socket_terminal terminal=ECONNRESET\nPASS: old_socket_terminal\n");
    if (make_deadline(overall_deadline, MS07_PHASE_DEADLINE_MS, &deadline) != 0)
        return fail_case("new_epoch_traffic", "deadline");
    printf("MS07_CASE_START: new_epoch_traffic\n");
    new_socket = open_peer_socket();
    if (new_socket < 0 ||
        wait_for_drained_active(&wire, &reset, pre.owner_available, deadline, &up) != 0)
        return fail_case("new_epoch_traffic", "pre-drain");
    if (peer_exchange(new_socket, "new_epoch_traffic", deadline) != 0)
        return fail_case("new_epoch_traffic", "peer");
    /* A3 rework: after the new-epoch exchange the owner must be Active at
     * (Q1, S1) with the DeviceOwned ledger drained and available conserved, and
     * the old S0 socket must still be permanently ECONNRESET. */
    if (wait_for_drained_active(&wire, &reset, pre.owner_available, deadline, &up) != 0 ||
        expect_terminal_twice(old_socket, ECONNRESET, deadline) != 0)
        return fail_case("new_epoch_traffic", "drain");
    print_v4("new_epoch_traffic", &wire);
    printf("MS07_SOCKET: case=new_epoch_traffic terminal=ECONNRESET\nMS07_PEER: case=new_epoch_traffic result=ok\nPASS: new_epoch_traffic\n");
    if (make_deadline(overall_deadline, MS07_OPERATOR_DEADLINE_MS, &operator_deadline) != 0)
        return fail_case("hmp_link_down", "deadline");
    printf("MS07_CASE_START: hmp_link_down\nMS07_HMP_READY: link=off\n");
    /* A5: baseline is the fresh new-epoch drained observation (available==pre),
     * not the reset snapshot which still holds the transient in-flight slot. */
    if (wait_for_link_down(&up, &wire, &down, operator_deadline) != 0 ||
        expect_terminal_twice(new_socket, ENOTCONN, operator_deadline) != 0)
        return fail_case("hmp_link_down", "terminal");
    printf("MS07_HMP_OBSERVED: link=off\n"); print_v4("hmp_link_down", &wire);
    printf("MS07_SOCKET: case=hmp_link_down terminal=ENOTCONN\nPASS: hmp_link_down\n");
    if (make_deadline(overall_deadline, MS07_OPERATOR_DEADLINE_MS, &operator_deadline) != 0)
        return fail_case("hmp_link_up", "deadline");
    printf("MS07_CASE_START: hmp_link_up\nMS07_HMP_READY: link=on\n");
    if (wait_for_link_up(&reset, &down, &wire, &up, operator_deadline) != 0) return fail_case("hmp_link_up", "snapshot");
    newest_socket = open_peer_socket();
    if (newest_socket < 0 ||
        peer_exchange(newest_socket, "hmp_link_up", operator_deadline) != 0)
        return fail_case("hmp_link_up", "peer");
    /* The S1 socket must remain permanently ENOTCONN even after link is up and
     * a brand-new S2 socket exchanges successfully. */
    if (expect_terminal_twice(new_socket, ENOTCONN, operator_deadline) != 0)
        return fail_case("hmp_link_up", "s1-still-closed");
    printf("MS07_HMP_OBSERVED: link=on\n");
    print_v4("hmp_link_up", &wire);
    printf("MS07_SOCKET: case=hmp_link_up terminal=ENOTCONN\nMS07_PEER: case=hmp_link_up result=ok\nPASS: hmp_link_up\nMS07_RECOVERY_END\n");
    close(old_socket); close(new_socket); close(newest_socket);
    return 0;
}

int main(int argc, char **argv) {
    if (argc == 2 && strcmp(argv[1], "--print-cases") == 0) {
        for (unsigned i = 0; i < sizeof(ms07_cases) / sizeof(ms07_cases[0]); ++i)
            puts(ms07_cases[i]);
        return 0;
    }
    if (argc == 2 && strcmp(argv[1], "--print-schema") == 0) {
        for (unsigned i = 0; i < sizeof(ms07_schema) / sizeof(ms07_schema[0]); ++i)
            puts(ms07_schema[i]);
        return 0;
    }
    if (argc == 2 && strcmp(argv[1], "--self-test") == 0)
        return ms07_probe_decision_core_self_test() ? 0 : 1;
    if (argc == 2 && strcmp(argv[1], "--run") == 0)
        return run_probe();
    fprintf(stderr, "usage: %s --print-cases | --print-schema | --self-test | --run\n", argv[0]);
    return 2;
}
#endif
