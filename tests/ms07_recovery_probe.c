/* MS07 QEMU recovery probe decision core.  The pure transition checks below
 * are shared by the guest payload and the host C harness; the actual QEMU
 * choreography remains manual in Iteration 007. */
#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <arpa/inet.h>
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

#ifndef MS07_REVISION_DEFAULT
#define MS07_REVISION_DEFAULT "unknown"
#endif

#define MS07_ENVIRONMENT_DEFAULT "qemu-virt-riscv64-single-hart-virtio-mmio-user-net"
#define MS07_SNAPSHOT_V4 0x4e494434u
#define MS07_RESET_REQUEST 0x4e495231u
#define MS07_PEER_PORT 15572u
#define MS07_PHASE_DEADLINE_MS 30000u

#define MS07_LINK_DOWN 0u
#define MS07_LINK_UP 1u

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
    uint64_t current_valid;
    uint64_t current_queue_epoch;
    uint64_t current_socket_epoch;
    uint64_t current_link_generation;
    uint64_t current_link_state;
    uint64_t owner_quarantined;
    uint64_t fault_valid;
    uint64_t fault_queue_epoch;
};

int ms07_deadline_expired(uint64_t start_ms, uint64_t now_ms, uint64_t budget_ms)
{
    return now_ms < start_ms || now_ms - start_ms >= budget_ms;
}

int ms07_reset_transition_valid(const struct ms07_v4_observation *before,
                                const struct ms07_v4_observation *after)
{
    if (before == NULL || after == NULL || !before->current_valid || !after->current_valid)
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

int ms07_probe_decision_core_self_test(void) {
    return sizeof(ms07_cases) / sizeof(ms07_cases[0]) == 6 &&
        strcmp(ms07_cases[0], "pre_reset_traffic") == 0 &&
        strcmp(ms07_cases[5], "hmp_link_up") == 0 &&
        ms07_terminal_errno_valid(MS07_TERMINAL_RESET, ECONNRESET) &&
        !ms07_terminal_errno_valid(MS07_TERMINAL_RESET, ENOTCONN);
}

#ifndef MS07_RECOVERY_PROBE_TESTING
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

_Static_assert(offsetof(struct ms07_snapshot_v4_wire, current_valid) == 72u * sizeof(uint64_t),
               "V4 must keep V3 as its byte-for-byte prefix");
_Static_assert(sizeof(struct ms07_snapshot_v4_wire) == 87u * sizeof(uint64_t),
               "C V4 tail must match the Rust append-only wire");

static uint64_t now_ms(void)
{
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) return 0;
    return (uint64_t)ts.tv_sec * 1000u + (uint64_t)ts.tv_nsec / 1000000u;
}

static int read_v4(struct ms07_snapshot_v4_wire *wire, struct ms07_v4_observation *out)
{
    memset(wire, 0, sizeof(*wire));
    if (ioctl(STDIN_FILENO, MS07_SNAPSHOT_V4, wire) < 0) return -1;
    out->current_valid = wire->current_valid;
    out->current_queue_epoch = wire->current_queue_epoch;
    out->current_socket_epoch = wire->current_socket_epoch;
    out->current_link_generation = wire->current_link_generation;
    out->current_link_state = wire->current_link_state;
    out->owner_quarantined = wire->current_owner_quarantined;
    out->fault_valid = wire->fault_valid;
    out->fault_queue_epoch = wire->fault_queue_epoch;
    return 0;
}

static void print_v4(const char *case_name, const struct ms07_snapshot_v4_wire *wire)
{
    printf("MS07_V4: case=%s current_valid=%llu q=%llu s=%llu l=%llu link=%s owned=%llu fault_valid=%llu\n",
           case_name, (unsigned long long)wire->current_valid,
           (unsigned long long)wire->current_queue_epoch,
           (unsigned long long)wire->current_socket_epoch,
           (unsigned long long)wire->current_link_generation,
           wire->current_link_state == MS07_LINK_UP ? "up" : "down",
           (unsigned long long)wire->current_owner_quarantined,
           (unsigned long long)wire->fault_valid);
}

static int wait_for_reset(const struct ms07_v4_observation *before,
                          struct ms07_snapshot_v4_wire *wire,
                          struct ms07_v4_observation *after)
{
    uint64_t start = now_ms();
    while (!ms07_deadline_expired(start, now_ms(), MS07_PHASE_DEADLINE_MS)) {
        if (read_v4(wire, after) == 0 && ms07_reset_transition_valid(before, after)) return 0;
        poll(NULL, 0, 20);
    }
    return -1;
}

static int wait_for_link_down(const struct ms07_v4_observation *before,
                              struct ms07_snapshot_v4_wire *wire,
                              struct ms07_v4_observation *after)
{
    uint64_t start = now_ms();
    while (!ms07_deadline_expired(start, now_ms(), MS07_PHASE_DEADLINE_MS)) {
        if (read_v4(wire, after) == 0 && ms07_link_down_transition_valid(before, after)) return 0;
        poll(NULL, 0, 20);
    }
    return -1;
}

static int wait_for_link_up(const struct ms07_v4_observation *before,
                            const struct ms07_v4_observation *down,
                            struct ms07_snapshot_v4_wire *wire,
                            struct ms07_v4_observation *after)
{
    uint64_t start = now_ms();
    while (!ms07_deadline_expired(start, now_ms(), MS07_PHASE_DEADLINE_MS)) {
        if (read_v4(wire, after) == 0 && after->current_valid &&
            after->current_queue_epoch == before->current_queue_epoch &&
            after->current_socket_epoch == before->current_socket_epoch + 1 &&
            after->current_link_generation == down->current_link_generation + 1 &&
            after->current_link_state == MS07_LINK_UP && !after->owner_quarantined) return 0;
        poll(NULL, 0, 20);
    }
    return -1;
}

static int peer_exchange(int fd, const char *run, const char *phase)
{
    char payload[192];
    int n = snprintf(payload, sizeof(payload), "run=%s phase=%s seq=0", run, phase);
    if (n < 0 || (size_t)n >= sizeof(payload) || send(fd, payload, (size_t)n, 0) != n) return -1;
    struct pollfd pfd = { .fd = fd, .events = POLLIN };
    if (poll(&pfd, 1, (int)MS07_PHASE_DEADLINE_MS) != 1 || !(pfd.revents & POLLIN)) return -1;
    char reply[sizeof(payload)];
    ssize_t got = recv(fd, reply, sizeof(reply), 0);
    return got == n && memcmp(payload, reply, (size_t)n) == 0 ? 0 : -1;
}

static int open_peer_socket(void)
{
    int fd = socket(AF_INET, SOCK_DGRAM, 0);
    struct sockaddr_in peer;
    if (fd < 0) return -1;
    memset(&peer, 0, sizeof(peer));
    peer.sin_family = AF_INET;
    peer.sin_port = htons(MS07_PEER_PORT);
    peer.sin_addr.s_addr = htonl(0x0a000202u); /* QEMU user-net host */
    if (connect(fd, (const struct sockaddr *)&peer, sizeof(peer)) == 0) return fd;
    close(fd);
    return -1;
}

static int expect_terminal(int fd, int expected)
{
    struct pollfd pfd = { .fd = fd, .events = POLLIN | POLLERR | POLLHUP };
    if (poll(&pfd, 1, (int)MS07_PHASE_DEADLINE_MS) != 1) return -1;
    char byte;
    if (recv(fd, &byte, sizeof(byte), MSG_DONTWAIT) >= 0) return -1;
    return errno == expected ? 0 : -1;
}

static int fail_case(const char *case_name, const char *reason)
{
    printf("FAIL: %s reason=%s\n", case_name, reason);
    return 1;
}

static int run_probe(const char *revision)
{
    struct ms07_snapshot_v4_wire wire;
    struct ms07_v4_observation pre, reset, down, up;
    int old_socket = -1, new_socket = -1, newest_socket = -1;
    printf("MS07_RECOVERY_START\nMS07_REVISION: %s\nMS07_ENVIRONMENT: %s\n",
           revision, MS07_ENVIRONMENT_DEFAULT);
    printf("MS07_CASE_START: pre_reset_traffic\n");
    old_socket = open_peer_socket();
    if (old_socket < 0 || read_v4(&wire, &pre) < 0 || !pre.current_valid ||
        pre.current_link_state != MS07_LINK_UP || pre.owner_quarantined ||
        peer_exchange(old_socket, revision, "pre_reset_traffic") != 0) return fail_case("pre_reset_traffic", "precondition");
    print_v4("pre_reset_traffic", &wire);
    printf("MS07_PEER: case=pre_reset_traffic result=ok\nPASS: pre_reset_traffic\n");
    printf("MS07_CASE_START: reset_request\n");
    if (ioctl(STDIN_FILENO, MS07_RESET_REQUEST, 0) < 0) return fail_case("reset_request", "ioctl");
    if (ioctl(STDIN_FILENO, MS07_RESET_REQUEST, 0) == 0 || errno != EAGAIN) return fail_case("reset_request", "duplicate");
    printf("MS07_RESET: accepted=1 duplicate=EAGAIN\nPASS: reset_request\n");
    printf("MS07_CASE_START: old_socket_terminal\n");
    if (wait_for_reset(&pre, &wire, &reset) != 0 || expect_terminal(old_socket, ECONNRESET) != 0) return fail_case("old_socket_terminal", "reset-terminal");
    print_v4("old_socket_terminal", &wire);
    printf("MS07_SOCKET: case=old_socket_terminal terminal=ECONNRESET\nPASS: old_socket_terminal\n");
    printf("MS07_CASE_START: new_epoch_traffic\n");
    new_socket = open_peer_socket();
    if (new_socket < 0 || peer_exchange(new_socket, revision, "new_epoch_traffic") != 0) return fail_case("new_epoch_traffic", "peer");
    print_v4("new_epoch_traffic", &wire);
    printf("MS07_PEER: case=new_epoch_traffic result=ok\nPASS: new_epoch_traffic\n");
    printf("MS07_CASE_START: hmp_link_down\nMS07_HMP_READY: link=off\n");
    if (wait_for_link_down(&reset, &wire, &down) != 0 || expect_terminal(new_socket, ENOTCONN) != 0) return fail_case("hmp_link_down", "terminal");
    printf("MS07_HMP_OBSERVED: link=off\n"); print_v4("hmp_link_down", &wire);
    printf("MS07_SOCKET: case=hmp_link_down terminal=ENOTCONN\nPASS: hmp_link_down\n");
    printf("MS07_CASE_START: hmp_link_up\nMS07_HMP_READY: link=on\n");
    if (wait_for_link_up(&reset, &down, &wire, &up) != 0) return fail_case("hmp_link_up", "snapshot");
    newest_socket = open_peer_socket();
    if (newest_socket < 0 || peer_exchange(newest_socket, revision, "hmp_link_up") != 0) return fail_case("hmp_link_up", "peer");
    printf("MS07_HMP_OBSERVED: link=on\n"); print_v4("hmp_link_up", &wire);
    printf("MS07_PEER: case=hmp_link_up result=ok\nPASS: hmp_link_up\nMS07_RECOVERY_END\n");
    close(old_socket); close(new_socket); close(newest_socket);
    return 0;
}

int main(int argc, char **argv) {
    if (argc == 2 && strcmp(argv[1], "--print-cases") == 0) {
        for (unsigned i = 0; i < sizeof(ms07_cases) / sizeof(ms07_cases[0]); ++i)
            puts(ms07_cases[i]);
        return 0;
    }
    if (argc == 2 && strcmp(argv[1], "--self-test") == 0)
        return ms07_probe_decision_core_self_test() ? 0 : 1;
    if (argc == 3 && strcmp(argv[1], "--run") == 0 && argv[2][0] != '\0')
        return run_probe(argv[2]);
    fprintf(stderr, "usage: %s --print-cases | --self-test | --run <revision>\n", argv[0]);
    return 2;
}
#endif
