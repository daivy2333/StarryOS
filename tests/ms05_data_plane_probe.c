#define _DEFAULT_SOURCE
#define _POSIX_C_SOURCE 200809L

/* MS05 data-plane probe — guest-side payload and host-testable decision core.
 *
 * Build (host syntax check):
 *   cc -std=c11 -Wall -Wextra -Werror -fsyntax-only tests/ms05_data_plane_probe.c
 *
 * Build (RISC-V static, user boundary):
 *   riscv64-linux-musl-gcc -std=c11 -Wall -Wextra -Werror -static -no-pie -Os \
 *     tests/ms05_data_plane_probe.c -o tests/ms05_data_plane_probe
 *
 * Host decision harness (mutations, no guest):
 *   cc -std=c11 -Wall -Wextra -Werror tests/ms05_data_plane_probe_test.c \
 *     -o /tmp/ms05-data-plane-probe-test && /tmp/ms05-data-plane-probe-test
 *
 * Usage (inside QEMU guest shell, Iteration 010):
 *   ./ms05_data_plane_probe snapshot
 *   ./ms05_data_plane_probe tx-only <count> <payload>
 *   ./ms05_data_plane_probe bidirectional <count> <payload>
 *   ./ms05_data_plane_probe slot-full
 *   ./ms05_data_plane_probe descriptor-full
 *   ./ms05_data_plane_probe flush
 *
 * Each mode records the applicable V3 phases (PRE/HELD/FULL/RELEASED/POST),
 * proves slot/descriptor Full from the exact ledger, and emits exactly one
 * terminal `MS05 PASS|FAIL mode=<mode>` whose exit status is consistent with
 * the marker.
 */

#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* ── MS05 wire ABI (fixed; see kernel virtio_net_irq_logic::IrqSnapshotV3) ── */

#define MS05_SNAPSHOT_V3     0x4e494433u
#define MS05_DIAGNOSTIC_CTL  0x4e494331u
#define MS05_FLUSH           0x4e494631u

/* Diagnostic control ops (crates/axnet/src/diag.rs). */
#define MS05_CTL_HOLD_SUBMIT  1u
#define MS05_CTL_HOLD_RECLAIM 2u
#define MS05_CTL_RELEASE      3u
#define MS05_MAX_LEASE_MS     2000u
#define MS05_HOLD_LEASE_MS    1500u

/* Fixed slot capacity and VirtIO queue size (QS). */
#define MS05_SLOT_CAPACITY    64u
#define MS05_QS               64u

/* Guest ↔ host UDP protocol (host stimulus binds 10.0.2.2:15557 in QEMU). */
#define MS05_HOST             "10.0.2.2"
#define MS05_PORT             15557u
#define MS05_MAGIC            0x4d533035u
#define MS05_DEFAULT_COUNT    96u
#define MS05_DEFAULT_PAYLOAD  64u

/* Fixed deadlines: equal-deadline completion is expired (FAIL). */
#define MS05_FULL_DEADLINE_MS 1200u
#define MS05_DRAIN_DEADLINE_MS 1500u
#define MS05_MODE_DEADLINE_MS 6000u
#define MS05_SOCKET_TIMEOUT_S 3
#define MS05_SEND_TIMEOUT_US 500000u
#define MS05_NOMINAL_SND_TIMEOUT_MS 500u
#define MS05_NOMINAL_RCV_TIMEOUT_MS 3000u
#define MS05_RETRY_SLEEP_MS 20u
#define MS05_POLL_SLEEP_MS 20u

/* ── V3 snapshot (72 x u64 = 576 bytes) ─────────────────────────────── */

struct ms05_snapshot {
    /* V2 prefix (28 fields, byte-for-byte IrqSnapshotV2). */
    uint64_t total;
    uint64_t used_ring;
    uint64_t config_change;
    uint64_t combined;
    uint64_t unknown;
    uint64_t spurious;
    uint64_t ack_count;
    uint64_t uart_irq_count;
    uint64_t restore_violation;
    uint64_t irq_enabled_entry;
    uint64_t rx_lifecycle;
    uint64_t rx_owner;
    uint64_t isr_publish;
    uint64_t isr_wake;
    uint64_t software_nudge;
    uint64_t task_poll;
    uint64_t reaped;
    uint64_t refilled;
    uint64_t delivered;
    uint64_t non_ip_consumed;
    uint64_t budget_exhausted;
    uint64_t self_yield;
    uint64_t router_full_wait;
    uint64_t space_wake;
    uint64_t empty_check;
    uint64_t fault;
    uint64_t last_error_stage;
    uint64_t last_error_code;
    /* RX slot ledger. */
    uint64_t rx_slot_occupancy;
    uint64_t rx_slot_high_water;
    uint64_t rx_slot_full;
    uint64_t rx_slot_enqueue;
    uint64_t rx_slot_dequeue;
    uint64_t rx_slot_space_event;
    /* TX slot ledger. */
    uint64_t tx_slot_occupancy;
    uint64_t tx_slot_high_water;
    uint64_t tx_slot_full;
    uint64_t tx_slot_enqueue;
    uint64_t tx_slot_dequeue;
    uint64_t tx_slot_space_event;
    /* TX driver ledger. */
    uint64_t tx_submit;
    uint64_t tx_again;
    uint64_t tx_completion;
    uint64_t tx_reclaim;
    uint64_t tx_buffer_available;
    uint64_t tx_buffer_inflight;
    uint64_t tx_descriptor_available;
    uint64_t tx_descriptor_inflight;
    /* Stage exhaustion. */
    uint64_t reclaim_exhausted;
    uint64_t rx_exhausted;
    uint64_t submit_exhausted;
    /* Queue event. */
    uint64_t queue_generation;
    uint64_t queue_wake;
    /* Ticket / flush. */
    uint64_t last_accepted;
    uint64_t live;
    uint64_t queued;
    uint64_t device_owned;
    uint64_t flush_target;
    uint64_t flush_success;
    uint64_t flush_error;
    uint64_t flush_busy;
    uint64_t flush_cancel;
    /* Diagnostic lease. */
    uint64_t hold_mode;
    uint64_t lease_expiry;
    uint64_t auto_release_failure;
    /* Fault / invariant. */
    uint64_t lifecycle_fault;
    uint64_t ownership_invariant;
    /* Stable drop reasons. */
    uint64_t drop_malformed_ip;
    uint64_t drop_no_route;
    uint64_t drop_route_source_mismatch;
    uint64_t drop_unsupported_address;
    uint64_t drop_frame_too_large;
};

_Static_assert(sizeof(struct ms05_snapshot) == 72 * sizeof(uint64_t),
               "MS05 V3 snapshot must remain 576 bytes");
_Static_assert(offsetof(struct ms05_snapshot, rx_slot_occupancy) ==
                   28 * sizeof(uint64_t),
               "V3 must preserve the V2 prefix at offset 28");
_Static_assert(offsetof(struct ms05_snapshot, drop_frame_too_large) ==
                   71 * sizeof(uint64_t),
               "V3 tail offset changed");

/* ── Phase model ────────────────────────────────────────────────────── */

enum ms05_phase {
    MS05_PHASE_PRE = 0,
    MS05_PHASE_HELD = 1,
    MS05_PHASE_FULL = 2,
    MS05_PHASE_RELEASED = 3,
    MS05_PHASE_POST = 4,
};

/* Per-mode required phase sequences (PRE..POST for plain modes, all five for
 * held modes). */
#define MS05_PHASES_PLAIN 2u
#define MS05_PHASES_HELD  5u

/* ── Decision core (host-testable, no I/O) ──────────────────────────── */

/* Wire datagram header (matches the guest/host UDP protocol). */
struct ms05_wire_header {
    uint32_t magic;
    uint32_t sequence;
    uint32_t count;
};

/* Byte-swaps a u32 between host and big-endian network order, matching the
 * Python host's `struct.pack/unpack("!III")` wire convention. */
static uint32_t ms05_be32(uint32_t value)
{
    return ((value & 0xffu) << 24) | ((value & 0xff00u) << 8) |
           ((value >> 8) & 0xff00u) | ((value >> 24) & 0xffu);
}

/* Remaining budget in ms from `now` within the absolute mode bound, or 0
 * when the budget is exhausted. Checked arithmetic: a regressed or wrapped
 * clock never yields a positive remaining budget. */
static uint64_t ms05_budget_remaining_ms(uint64_t mode_start, uint64_t now,
                                         uint64_t total_ms)
{
    uint64_t elapsed;
    if (now < mode_start) return 0;
    elapsed = now - mode_start;
    if (elapsed >= total_ms) return 0;
    return total_ms - elapsed;
}

/* Computes the absolute mode deadline from `mode_start`; returns -1 when the
 * addition would overflow, in which case no usable bound exists. */
static int ms05_mode_deadline_abs(uint64_t mode_start, uint64_t total_ms,
                                  uint64_t *abs_out)
{
    if (UINT64_MAX - mode_start < total_ms) return -1;
    *abs_out = mode_start + total_ms;
    return 0;
}

/* Deadline check. A completion at or after `deadline_ms` from `start` is
 * expired; so is any completion at or after the absolute `mode_deadline_abs`
 * (0 disables the mode bound); clock regression is also expired. */
static int ms05_deadline_expired(uint64_t start, uint64_t now,
                                 uint64_t deadline_ms,
                                 uint64_t mode_deadline_abs)
{
    if (now < start) return 1;
    if (now - start >= deadline_ms) return 1;
    if (mode_deadline_abs != 0) {
        if (mode_deadline_abs < start) return 1;
        if (now >= mode_deadline_abs) return 1;
    }
    return 0;
}

/* ── Mode traffic decision (T5.1-R3) ─────────────────────────────────── */

enum ms05_traffic_rule {
    MS05_TRAFFIC_EXACT, /* normal modes: exact nonzero requested count */
    MS05_TRAFFIC_HELD,  /* held modes: nonzero short send bounded by count */
};

static int ms05_traffic_proved(enum ms05_traffic_rule rule, uint32_t count,
                               uint32_t sent, uint32_t received)
{
    if (rule == MS05_TRAFFIC_HELD) {
        return sent > 0 && sent <= count && received == sent;
    }
    return count > 0 && sent == count && received == count;
}

/* ── Deadline budget decisions (T5.1-R4) ─────────────────────────────── */

/* Deadline context carried through every phase of a mode. The absolute mode
 * bound (`mode_abs`, == `mode_start` + total) always applies; the phase
 * window is optional (`phase_deadline_ms == 0` disables it). */
struct ms05_deadline_ctx {
    uint64_t mode_start;
    uint64_t mode_abs;
    uint64_t phase_start;
    uint64_t phase_deadline_ms; /* 0 disables the phase bound */
};

/* Minimum positive remaining budget in ms across the phase and absolute mode
 * bounds, or 0 when any bound is exhausted, the clock regressed or the
 * arithmetic would overflow. A per-operation timeout never exceeds this. */
static uint64_t ms05_ctx_budget_ms(const struct ms05_deadline_ctx *ctx,
                                   uint64_t now)
{
    uint64_t phase_rem = UINT64_MAX;
    uint64_t mode_rem = ms05_budget_remaining_ms(ctx->mode_start, now,
                                                 MS05_MODE_DEADLINE_MS);
    if (mode_rem == 0) return 0;
    if (ctx->mode_abs != 0) {
        if (ctx->mode_abs < ctx->mode_start) return 0;
        if (now >= ctx->mode_abs) return 0;
        if (ctx->mode_abs - now < mode_rem) mode_rem = ctx->mode_abs - now;
    }
    if (ctx->phase_deadline_ms != 0) {
        if (now < ctx->phase_start) return 0;
        if (now - ctx->phase_start >= ctx->phase_deadline_ms) return 0;
        phase_rem = ctx->phase_deadline_ms - (now - ctx->phase_start);
    }
    return phase_rem < mode_rem ? phase_rem : mode_rem;
}

/* Clamps a nominal per-operation timeout to the minimum positive remaining
 * budget. Returns the clamped ms (0 when the budget or nominal is zero). */
static uint64_t ms05_clamp_timeout_ms(uint64_t remaining_ms,
                                      uint64_t nominal_ms)
{
    if (remaining_ms == 0 || nominal_ms == 0) return 0;
    return remaining_ms < nominal_ms ? remaining_ms : nominal_ms;
}

/* True when the remaining mode budget can contain the kernel's blocking
 * flush timeout so a completion still lands strictly before the absolute
 * mode deadline. Equal budget cannot guarantee a strictly-before result. */
static int ms05_flush_affordable(uint64_t budget_ms, uint64_t kernel_timeout_ms)
{
    return budget_ms > kernel_timeout_ms;
}

/* Counter fields of the V3 tuple: every one must be monotonic across a
 * phase boundary. Gauge fields (occupancy, availability, lifecycle, owner,
 * hold mode, lease expiry, ticket counters, flush target) are state and are
 * copied from `post` without subtraction. */
#define MS05_DELTA_FIELD(field)                                                \
    do {                                                                       \
        if (post->field < pre->field) return -1;                               \
        delta->field = post->field - pre->field;                               \
    } while (0)

/* Computes a monotonic delta over the V3 counter fields. Returns 0 on
 * success and -1 when any monotonic counter regressed. Gauge fields
 * (occupancy, availability, lifecycle, hold mode, lease, ticket counters)
 * are copied from `post` but never subtracted. */
static int ms05_snapshot_delta(const struct ms05_snapshot *pre,
                               const struct ms05_snapshot *post,
                               struct ms05_snapshot *delta)
{
    memset(delta, 0, sizeof(*delta));
    /* V2 prefix counters. */
    MS05_DELTA_FIELD(total);
    MS05_DELTA_FIELD(used_ring);
    MS05_DELTA_FIELD(config_change);
    MS05_DELTA_FIELD(combined);
    MS05_DELTA_FIELD(unknown);
    MS05_DELTA_FIELD(spurious);
    MS05_DELTA_FIELD(ack_count);
    MS05_DELTA_FIELD(uart_irq_count);
    MS05_DELTA_FIELD(restore_violation);
    MS05_DELTA_FIELD(irq_enabled_entry);
    MS05_DELTA_FIELD(isr_publish);
    MS05_DELTA_FIELD(isr_wake);
    MS05_DELTA_FIELD(software_nudge);
    MS05_DELTA_FIELD(task_poll);
    MS05_DELTA_FIELD(reaped);
    MS05_DELTA_FIELD(refilled);
    MS05_DELTA_FIELD(delivered);
    MS05_DELTA_FIELD(non_ip_consumed);
    MS05_DELTA_FIELD(budget_exhausted);
    MS05_DELTA_FIELD(self_yield);
    MS05_DELTA_FIELD(router_full_wait);
    MS05_DELTA_FIELD(space_wake);
    MS05_DELTA_FIELD(empty_check);
    MS05_DELTA_FIELD(fault);
    /* Slot counters. */
    MS05_DELTA_FIELD(rx_slot_full);
    MS05_DELTA_FIELD(rx_slot_enqueue);
    MS05_DELTA_FIELD(rx_slot_dequeue);
    MS05_DELTA_FIELD(rx_slot_space_event);
    MS05_DELTA_FIELD(tx_slot_full);
    MS05_DELTA_FIELD(tx_slot_enqueue);
    MS05_DELTA_FIELD(tx_slot_dequeue);
    MS05_DELTA_FIELD(tx_slot_space_event);
    /* TX driver counters. */
    MS05_DELTA_FIELD(tx_submit);
    MS05_DELTA_FIELD(tx_again);
    MS05_DELTA_FIELD(tx_completion);
    MS05_DELTA_FIELD(tx_reclaim);
    /* Stage exhaustion and event counters. */
    MS05_DELTA_FIELD(reclaim_exhausted);
    MS05_DELTA_FIELD(rx_exhausted);
    MS05_DELTA_FIELD(submit_exhausted);
    MS05_DELTA_FIELD(queue_generation);
    MS05_DELTA_FIELD(queue_wake);
    /* Flush counters. */
    MS05_DELTA_FIELD(flush_success);
    MS05_DELTA_FIELD(flush_error);
    MS05_DELTA_FIELD(flush_busy);
    MS05_DELTA_FIELD(flush_cancel);
    /* Diagnostic counter. */
    MS05_DELTA_FIELD(auto_release_failure);
    /* Fault and invariant counters. */
    MS05_DELTA_FIELD(lifecycle_fault);
    MS05_DELTA_FIELD(ownership_invariant);
    /* Drop counters. */
    MS05_DELTA_FIELD(drop_malformed_ip);
    MS05_DELTA_FIELD(drop_no_route);
    MS05_DELTA_FIELD(drop_route_source_mismatch);
    MS05_DELTA_FIELD(drop_unsupported_address);
    MS05_DELTA_FIELD(drop_frame_too_large);
    /* Gauges stay zero in the delta; the probe reads them from `post`. */
    return 0;
}

/* True when the V3 tuple reports an Active, async-owned data plane. */
static int ms05_active(const struct ms05_snapshot *s)
{
    return s->rx_lifecycle == 2 && s->rx_owner == 1;
}

/* True when the post snapshot and its delta carry no safety fault, no
 * lifecycle fault, no ownership-invariant violation, no IRQ restore
 * violation and no IRQ-enabled entry. */
static int ms05_common_valid(const struct ms05_snapshot *post,
                             const struct ms05_snapshot *delta)
{
    return ms05_active(post) &&
           post->fault == 0 && post->restore_violation == 0 &&
           post->irq_enabled_entry == 0 && post->lifecycle_fault == 0 &&
           post->ownership_invariant == 0 &&
           delta->fault == 0 && delta->restore_violation == 0 &&
           delta->irq_enabled_entry == 0 && delta->lifecycle_fault == 0 &&
           delta->ownership_invariant == 0;
}

/* True when TX buffer and descriptor conservation holds in both pre and
 * post: available + inflight == QS for each resource. */
static int ms05_tx_ledger_closed(const struct ms05_snapshot *pre,
                                 const struct ms05_snapshot *post)
{
    return pre->tx_buffer_available + pre->tx_buffer_inflight == MS05_QS &&
           post->tx_buffer_available + post->tx_buffer_inflight == MS05_QS &&
           pre->tx_descriptor_available + pre->tx_descriptor_inflight ==
               MS05_QS &&
           post->tx_descriptor_available + post->tx_descriptor_inflight ==
               MS05_QS;
}

/* True when the FULL phase proves exact slot Full: occupancy == 64, a full
 * transition occurred since HELD, and the high-water mark reached capacity. */
static int ms05_slot_full_proved(const struct ms05_snapshot *held,
                                 const struct ms05_snapshot *full)
{
    return full->tx_slot_occupancy == MS05_SLOT_CAPACITY &&
           full->tx_slot_full > held->tx_slot_full &&
           full->tx_slot_high_water >= MS05_SLOT_CAPACITY;
}

/* True when the FULL phase proves descriptor Full from the driver ledger:
 * no TX buffer and no TX descriptor available, an Again transition since
 * HELD, and every buffer and descriptor is inflight. */
static int ms05_descriptor_full_proved(const struct ms05_snapshot *held,
                                       const struct ms05_snapshot *full)
{
    return full->tx_buffer_available == 0 &&
           full->tx_buffer_inflight == MS05_QS &&
           full->tx_descriptor_available == 0 &&
           full->tx_descriptor_inflight == MS05_QS &&
           full->tx_again > held->tx_again;
}

/* True when the POST snapshot proves exact closure: TX slot occupancy zero,
 * matched slot enqueue/dequeue, every buffer and descriptor returned
 * (availability == QS, inflight == 0) and no live/queued/device-owned
 * tickets remain. Conservation alone (available + inflight == QS with
 * inflight > 0) is NOT closure. */
static int ms05_post_closed(const struct ms05_snapshot *post)
{
    return post->tx_slot_occupancy == 0 &&
           post->tx_slot_enqueue == post->tx_slot_dequeue &&
           post->tx_buffer_available == MS05_QS &&
           post->tx_buffer_inflight == 0 &&
           post->tx_descriptor_available == MS05_QS &&
           post->tx_descriptor_inflight == 0 &&
           post->live == 0 && post->queued == 0 && post->device_owned == 0;
}

/* True when the flush phase proves C4 closure: exactly one flush success,
 * no error/busy/cancel delta, no live/queued/device-owned tickets, and the
 * TX ledger fully closed between pre and post. A success counter at u64 max
 * can never claim another success. */
static int ms05_flush_proved(const struct ms05_snapshot *pre,
                             const struct ms05_snapshot *post)
{
    return pre->flush_success != UINT64_MAX &&
           post->flush_success == pre->flush_success + 1 &&
           post->flush_error == pre->flush_error &&
           post->flush_busy == pre->flush_busy &&
           post->flush_cancel == pre->flush_cancel &&
           post->live == 0 && post->queued == 0 && post->device_owned == 0 &&
           ms05_tx_ledger_closed(pre, post) && ms05_post_closed(post);
}

/* True when the observed phase sequence exactly matches the required order:
 * every required phase appears exactly once and no phase is skipped,
 * duplicated or reordered. */
static int ms05_phase_order_valid(const uint8_t *phases, size_t n,
                                  const uint8_t *required, size_t required_n)
{
    if (n != required_n) return 0;
    for (size_t i = 0; i < n; ++i) {
        if (phases[i] != required[i]) return 0;
    }
    return 1;
}

#ifdef MS05_DATA_PLANE_PROBE_TESTING

/* Scans `line` for terminal markers. Returns the number of markers found and
 * records the first `MS05 PASS|FAIL mode=<m>` on success. */
static int ms05_scan_markers(const char *line, char *mode, size_t mode_len,
                             int *pass)
{
    const char *cursor = line;
    int found = 0;
    while ((cursor = strstr(cursor, "MS05 ")) != NULL) {
        int is_pass;
        if (strncmp(cursor + 5, "PASS", 4) == 0) {
            is_pass = 1;
        } else if (strncmp(cursor + 5, "FAIL", 4) == 0) {
            is_pass = 0;
        } else {
            cursor += 5;
            continue;
        }
        const char *after = cursor + 9;
        const char *eq = strstr(after, "mode=");
        const char *end;
        size_t len;
        if (eq == NULL) return -1;
        eq += 5;
        end = eq;
        while (*end != '\0' && *end != ' ' && *end != '\n' && *end != '\r') {
            end++;
        }
        len = (size_t)(end - eq);
        if (len == 0 || len >= mode_len) return -1;
        if (found == 0) {
            memcpy(mode, eq, len);
            mode[len] = '\0';
            *pass = is_pass;
        } else if (*pass != is_pass || strncmp(mode, eq, len) != 0) {
            return -1; /* conflicting markers */
        }
        found++;
        cursor = end;
    }
    return found;
}

/* Parses a terminal marker line. Returns 1 when the line contains exactly
 * one `MS05 PASS mode=<m>` or `MS05 FAIL mode=<m>`; 0 when no marker is
 * present; -1 when the line is malformed or contains multiple/conflicting
 * markers. On success stores the mode and pass flag. */
static int ms05_marker_parse(const char *line, char *mode, size_t mode_len,
                             int *pass)
{
    int found = ms05_scan_markers(line, mode, mode_len, pass);
    if (found < 0) return -1;
    if (found == 0) return 0;
    if (found > 1) return -1;
    return 1;
}

/* True when a PASS marker returns exit 0 and a FAIL marker returns nonzero. */
static int ms05_exit_consistent(int pass, int exit_code)
{
    return (pass != 0) == (exit_code == 0);
}

#endif /* MS05_DATA_PLANE_PROBE_TESTING */

/* True when a wire datagram header and payload are valid for the given
 * sequence/count/payload. Returns 0 on success, -1 on any mismatch. */
static int ms05_validate_datagram(const uint8_t *packet, ssize_t length,
                                  uint32_t sequence, uint32_t count,
                                  uint32_t payload_size)
{
    struct ms05_wire_header header;
    ssize_t expected = (ssize_t)(sizeof(header) + payload_size);
    if (length != expected) return -1;
    memcpy(&header, packet, sizeof(header));
    if (ms05_be32(header.magic) != MS05_MAGIC ||
        ms05_be32(header.sequence) != sequence ||
        ms05_be32(header.count) != count) {
        return -1;
    }
    for (uint32_t i = 0; i < payload_size; ++i) {
        if (packet[sizeof(header) + i] !=
            (uint8_t)((sequence + i) & 0xffu)) {
            return -1;
        }
    }
    return 0;
}

#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <time.h>
#include <unistd.h>

/* ── Operation seam (Task 5.3) ────────────────────────────────────────
 *
 * Every runtime side effect (monotonic clock, bounded sleep, diagnostic
 * control/flush/snapshot ioctl, socket timeout, send, receive, socket
 * open/close/nonblock) is reached through `g_ms05_ops`. The production
 * payload uses the `prod_*` implementations below; the host harness
 * replaces them with fakes so the exact mode runners run deterministically.
 */

enum ms05_op_result {
    MS05_OP_OK = 0,
    MS05_OP_BUSY = 1,   /* EAGAIN / EWOULDBLOCK (diagnostic control only) */
    MS05_OP_ERROR = -1,
};

struct ms05_udp {
    int fd;
    struct sockaddr_in host;
};

struct ms05_ops {
    int (*clock_now)(uint64_t *now);
    void (*sleep_ms)(uint32_t ms);
    int (*ioctl_ctl)(uint64_t op, uint64_t lease_ms);
    int (*ioctl_flush)(void);
    int (*ioctl_snapshot)(struct ms05_snapshot *out);
    int (*sock_open)(struct ms05_udp *u);
    void (*sock_close)(struct ms05_udp *u);
    int (*sock_set_rcv_timeout)(int fd, uint32_t ms);
    int (*sock_set_snd_timeout)(int fd, uint32_t ms);
    int (*sock_set_nonblock)(int fd, int enable);
    ssize_t (*sock_send)(int fd, const void *buf, size_t len);
    ssize_t (*sock_recv)(int fd, void *buf, size_t len);
};

static int prod_clock_now(uint64_t *now)
{
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) return -1;
    *now = (uint64_t)ts.tv_sec * 1000u + (uint64_t)ts.tv_nsec / 1000000u;
    return 0;
}

static void prod_sleep_ms(uint32_t ms)
{
    if (ms != 0) usleep((useconds_t)ms * 1000u);
}

static int prod_ioctl_ctl(uint64_t op, uint64_t lease_ms)
{
    uint64_t payload[2] = {op, lease_ms};
    if (ioctl(STDIN_FILENO, MS05_DIAGNOSTIC_CTL, payload) == 0) {
        return MS05_OP_OK;
    }
    if (errno == EAGAIN || errno == EWOULDBLOCK) return MS05_OP_BUSY;
    perror("ioctl MS05_DIAGNOSTIC_CTL");
    return MS05_OP_ERROR;
}

static int prod_ioctl_flush(void)
{
    if (ioctl(STDIN_FILENO, MS05_FLUSH, 0) == 0) return MS05_OP_OK;
    perror("ioctl MS05_FLUSH");
    return MS05_OP_ERROR;
}

static int prod_ioctl_snapshot(struct ms05_snapshot *out)
{
    if (ioctl(STDIN_FILENO, MS05_SNAPSHOT_V3, out) == 0) return MS05_OP_OK;
    perror("ioctl MS05_SNAPSHOT_V3");
    return MS05_OP_ERROR;
}

static int prod_sock_open(struct ms05_udp *u)
{
    struct timeval rcv_timeout = {.tv_sec = MS05_SOCKET_TIMEOUT_S,
                                  .tv_usec = 0};
    struct timeval snd_timeout = {.tv_sec = 0,
                                  .tv_usec = (suseconds_t)MS05_SEND_TIMEOUT_US};
    u->fd = socket(AF_INET, SOCK_DGRAM, 0);
    if (u->fd < 0) return -1;
    if (setsockopt(u->fd, SOL_SOCKET, SO_RCVTIMEO, &rcv_timeout,
                   sizeof(rcv_timeout)) != 0) {
        close(u->fd);
        u->fd = -1;
        return -1;
    }
    if (setsockopt(u->fd, SOL_SOCKET, SO_SNDTIMEO, &snd_timeout,
                   sizeof(snd_timeout)) != 0) {
        close(u->fd);
        u->fd = -1;
        return -1;
    }
    memset(&u->host, 0, sizeof(u->host));
    u->host.sin_family = AF_INET;
    u->host.sin_port = htons(MS05_PORT);
    if (inet_pton(AF_INET, MS05_HOST, &u->host.sin_addr) != 1 ||
        connect(u->fd, (struct sockaddr *)&u->host, sizeof(u->host)) != 0) {
        close(u->fd);
        u->fd = -1;
        return -1;
    }
    return 0;
}

static void prod_sock_close(struct ms05_udp *u)
{
    if (u->fd >= 0) close(u->fd);
    u->fd = -1;
}

static int prod_sock_set_rcv_timeout(int fd, uint32_t ms)
{
    struct timeval tv = {.tv_sec = (time_t)(ms / 1000u),
                         .tv_usec = (suseconds_t)((ms % 1000u) * 1000u)};
    return setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
}

static int prod_sock_set_snd_timeout(int fd, uint32_t ms)
{
    struct timeval tv = {.tv_sec = (time_t)(ms / 1000u),
                         .tv_usec = (suseconds_t)((ms % 1000u) * 1000u)};
    return setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof(tv));
}

static int prod_sock_set_nonblock(int fd, int enable)
{
    int flags = fcntl(fd, F_GETFL, 0);
    if (flags < 0) return -1;
    if (enable) return fcntl(fd, F_SETFL, flags | O_NONBLOCK);
    return fcntl(fd, F_SETFL, flags & ~O_NONBLOCK);
}

static ssize_t prod_sock_send(int fd, const void *buf, size_t len)
{
    return send(fd, buf, len, 0);
}

static ssize_t prod_sock_recv(int fd, void *buf, size_t len)
{
    return recv(fd, buf, len, 0);
}

/* Initialized to the production implementations so the static payload uses
 * the real syscalls; the host harness overrides the fields it needs. */
static struct ms05_ops g_ms05_ops = {
    .clock_now = prod_clock_now,
    .sleep_ms = prod_sleep_ms,
    .ioctl_ctl = prod_ioctl_ctl,
    .ioctl_flush = prod_ioctl_flush,
    .ioctl_snapshot = prod_ioctl_snapshot,
    .sock_open = prod_sock_open,
    .sock_close = prod_sock_close,
    .sock_set_rcv_timeout = prod_sock_set_rcv_timeout,
    .sock_set_snd_timeout = prod_sock_set_snd_timeout,
    .sock_set_nonblock = prod_sock_set_nonblock,
    .sock_send = prod_sock_send,
    .sock_recv = prod_sock_recv,
};

/* ── Bounded side-effect helpers (Task 5.3) ────────────────────────────
 *
 * Every side effect obeys one absolute mode deadline (plus an optional phase
 * window): a fresh clock read must show a strictly positive remaining budget
 * before the operation starts, socket timeouts and retry sleeps clamp to the
 * minimum positive budget, and a fresh clock read must still be strictly
 * before the deadline after the operation returns. Equal/late completion,
 * clock regression and arithmetic overflow never start or continue an
 * operation. */

/* Precheck: remaining budget in ms strictly positive under `ctx` at a fresh
 * clock read, or 0 when exhausted/regressed/overflowed. */
static uint64_t ms05_precheck_budget(const struct ms05_deadline_ctx *ctx)
{
    uint64_t now;
    if (g_ms05_ops.clock_now(&now) != 0) return 0;
    return ms05_ctx_budget_ms(ctx, now);
}

/* Postcheck: a fresh clock read strictly before the phase and mode
 * deadlines. Returns 0 on success, -1 when equal/late/regressed. */
static int ms05_postcheck(const struct ms05_deadline_ctx *ctx)
{
    uint64_t now;
    if (g_ms05_ops.clock_now(&now) != 0) return -1;
    if (ms05_deadline_expired(ctx->mode_start, now, MS05_MODE_DEADLINE_MS,
                              ctx->mode_abs)) {
        return -1;
    }
    if (ctx->phase_deadline_ms != 0 &&
        ms05_deadline_expired(ctx->phase_start, now, ctx->phase_deadline_ms,
                              0)) {
        return -1;
    }
    return 0;
}

/* Bounded sleep: clamps `nominal_ms` to the minimum positive remaining
 * budget, sleeps, then postchecks. Returns 0 on success. */
static int ms05_bounded_sleep(const struct ms05_deadline_ctx *ctx,
                              uint64_t nominal_ms)
{
    uint64_t now, remaining, clamped;
    if (g_ms05_ops.clock_now(&now) != 0) return -1;
    remaining = ms05_ctx_budget_ms(ctx, now);
    clamped = ms05_clamp_timeout_ms(remaining, nominal_ms);
    if (clamped == 0) return -1;
    g_ms05_ops.sleep_ms((uint32_t)clamped);
    return ms05_postcheck(ctx);
}

/* Bounded snapshot read with pre/post deadline checks. */
static int ms05_bounded_snapshot(const struct ms05_deadline_ctx *ctx,
                                 struct ms05_snapshot *out)
{
    if (ms05_precheck_budget(ctx) == 0) return -1;
    if (g_ms05_ops.ioctl_snapshot(out) != MS05_OP_OK) return -1;
    return ms05_postcheck(ctx);
}

/* Bounded diagnostic control with `ResourceBusy` retry. The budget is
 * re-checked before the first and every retry ioctl; the retry sleep clamps
 * to the remaining budget, so an EAGAIN near expiry cannot sleep a fixed
 * interval and then invoke a late ioctl. */
static int ms05_bounded_control(const struct ms05_deadline_ctx *ctx,
                                uint64_t op, uint64_t lease_ms)
{
    for (;;) {
        if (ms05_precheck_budget(ctx) == 0) return -1;
        switch (g_ms05_ops.ioctl_ctl(op, lease_ms)) {
        case MS05_OP_OK:
            return ms05_postcheck(ctx);
        case MS05_OP_BUSY:
            if (ms05_bounded_sleep(ctx, MS05_RETRY_SLEEP_MS) != 0) return -1;
            break;
        default:
            return -1;
        }
    }
}

/* Bounded flush: the remaining budget must strictly contain the kernel
 * flush timeout, then the ioctl runs and the deadline is re-checked. */
static int ms05_bounded_flush(const struct ms05_deadline_ctx *ctx)
{
    uint64_t remaining = ms05_precheck_budget(ctx);
    if (remaining == 0 ||
        !ms05_flush_affordable(remaining, MS05_MAX_LEASE_MS)) {
        return -1;
    }
    if (g_ms05_ops.ioctl_flush() != MS05_OP_OK) return -1;
    return ms05_postcheck(ctx);
}

/* Bounded datagram send: precheck, clamp the send timeout to the minimum
 * positive budget, set the timeout, re-check the budget with a fresh clock
 * read (a setter that consumed the last budget prevents the send), send the
 * full buffer, then postcheck. */
static int ms05_bounded_send(const struct ms05_deadline_ctx *ctx,
                             struct ms05_udp *u, const void *buf, size_t len)
{
    uint64_t now, remaining, clamped;
    if (g_ms05_ops.clock_now(&now) != 0) return -1;
    remaining = ms05_ctx_budget_ms(ctx, now);
    clamped = ms05_clamp_timeout_ms(remaining, MS05_NOMINAL_SND_TIMEOUT_MS);
    if (clamped == 0) return -1;
    if (g_ms05_ops.sock_set_snd_timeout(u->fd, (uint32_t)clamped) != 0) {
        return -1;
    }
    if (ms05_precheck_budget(ctx) == 0) return -1;
    if (g_ms05_ops.sock_send(u->fd, buf, len) != (ssize_t)len) return -1;
    return ms05_postcheck(ctx);
}

/* Bounded datagram receive: precheck, clamp the receive timeout to the
 * minimum positive budget, set the timeout, re-check the budget with a fresh
 * clock read (a setter that consumed the last budget prevents the recv),
 * receive, then postcheck. Returns the number of bytes received or -1. */
static ssize_t ms05_bounded_recv(const struct ms05_deadline_ctx *ctx,
                                 struct ms05_udp *u, void *buf, size_t len)
{
    uint64_t now, remaining, clamped;
    ssize_t n;
    if (g_ms05_ops.clock_now(&now) != 0) return -1;
    remaining = ms05_ctx_budget_ms(ctx, now);
    clamped = ms05_clamp_timeout_ms(remaining, MS05_NOMINAL_RCV_TIMEOUT_MS);
    if (clamped == 0) return -1;
    if (g_ms05_ops.sock_set_rcv_timeout(u->fd, (uint32_t)clamped) != 0) {
        return -1;
    }
    if (ms05_precheck_budget(ctx) == 0) return -1;
    n = g_ms05_ops.sock_recv(u->fd, buf, len);
    if (n <= 0) return -1;
    if (ms05_postcheck(ctx) != 0) return -1;
    return n;
}

static void print_snapshot(const char *label, const struct ms05_snapshot *s)
{
    printf("%s lifecycle=%lu owner=%lu fault=%lu lc_fault=%lu owner_inv=%lu "
           "hold=%lu auto_rel=%lu rx_occ=%lu rx_full=%lu tx_occ=%lu "
           "tx_full=%lu tx_enq=%lu tx_deq=%lu tx_submit=%lu tx_again=%lu "
           "tx_comp=%lu tx_reclaim=%lu buf_avail=%lu buf_inflight=%lu "
           "desc_avail=%lu desc_inflight=%lu live=%lu queued=%lu "
           "dev_owned=%lu flush_ok=%lu flush_err=%lu flush_busy=%lu "
           "flush_cancel=%lu last_accepted=%lu\n",
           label, (unsigned long)s->rx_lifecycle,
           (unsigned long)s->rx_owner, (unsigned long)s->fault,
           (unsigned long)s->lifecycle_fault,
           (unsigned long)s->ownership_invariant,
           (unsigned long)s->hold_mode,
           (unsigned long)s->auto_release_failure,
           (unsigned long)s->rx_slot_occupancy,
           (unsigned long)s->rx_slot_full,
           (unsigned long)s->tx_slot_occupancy,
           (unsigned long)s->tx_slot_full,
           (unsigned long)s->tx_slot_enqueue,
           (unsigned long)s->tx_slot_dequeue,
           (unsigned long)s->tx_submit, (unsigned long)s->tx_again,
           (unsigned long)s->tx_completion, (unsigned long)s->tx_reclaim,
           (unsigned long)s->tx_buffer_available,
           (unsigned long)s->tx_buffer_inflight,
           (unsigned long)s->tx_descriptor_available,
           (unsigned long)s->tx_descriptor_inflight,
           (unsigned long)s->live, (unsigned long)s->queued,
           (unsigned long)s->device_owned, (unsigned long)s->flush_success,
           (unsigned long)s->flush_error, (unsigned long)s->flush_busy,
           (unsigned long)s->flush_cancel,
           (unsigned long)s->last_accepted);
}

/* Terminal marker. `ok` selects PASS (exit 0) or FAIL (exit 1). */
static int finish_mode(const char *mode, int ok)
{
    printf("MS05 %s mode=%s\n", ok ? "PASS" : "FAIL", mode);
    return ok ? 0 : 1;
}

static int fail_mode(const char *mode, const char *reason)
{
    printf("MS05 FAIL mode=%s reason=%s\n", mode, reason);
    return 1;
}

/* Reads one snapshot through the bounded seam; on read failure emits FAIL
 * and returns -1. */
static int snapshot_or_fail(const char *mode, const char *phase,
                            const struct ms05_deadline_ctx *ctx,
                            struct ms05_snapshot *out)
{
    if (ms05_bounded_snapshot(ctx, out) != 0) {
        fail_mode(mode, phase);
        return -1;
    }
    print_snapshot(phase, out);
    return 0;
}

/* ── Guest ↔ host UDP protocol (through the operation seam) ─────────── */

static int udp_open(struct ms05_udp *u)
{
    return g_ms05_ops.sock_open(u);
}

static int udp_control(struct ms05_udp *u, const char *text,
                       const struct ms05_deadline_ctx *ctx)
{
    return ms05_bounded_send(ctx, u, text, strlen(text));
}

static int udp_control_recv(struct ms05_udp *u, char *buf, size_t size,
                            const struct ms05_deadline_ctx *ctx)
{
    ssize_t n = ms05_bounded_recv(ctx, u, buf, size - 1);
    if (n <= 0) return -1;
    buf[n] = '\0';
    return 0;
}

static int udp_ready_handshake(struct ms05_udp *u, const char *mode,
                               uint32_t count, uint32_t payload,
                               const struct ms05_deadline_ctx *ctx)
{
    char control[96];
    char expected[96];
    snprintf(control, sizeof(control), "MS05 REGISTER %s %u %u", mode, count,
             payload);
    if (udp_control(u, control, ctx) != 0) return -1;
    if (udp_control_recv(u, control, sizeof(control), ctx) != 0) return -1;
    snprintf(expected, sizeof(expected), "MS05 READY %s %u %u", mode, count,
             payload);
    if (strcmp(control, expected) != 0) return -1;
    snprintf(control, sizeof(control), "MS05 START %s %u %u", mode, count,
             payload);
    if (udp_control(u, control, ctx) != 0) return -1;
    return 0;
}

static int udp_send_data(struct ms05_udp *u, uint32_t sequence, uint32_t count,
                         uint32_t payload_size,
                         const struct ms05_deadline_ctx *ctx)
{
    struct ms05_wire_header header;
    uint8_t packet[sizeof(struct ms05_wire_header) + 64];
    ssize_t expected = (ssize_t)(sizeof(header) + payload_size);
    if (payload_size > 64) return -1;
    header.magic = ms05_be32(MS05_MAGIC);
    header.sequence = ms05_be32(sequence);
    header.count = ms05_be32(count);
    memcpy(packet, &header, sizeof(header));
    for (uint32_t i = 0; i < payload_size; ++i) {
        packet[sizeof(header) + i] = (uint8_t)((sequence + i) & 0xffu);
    }
    return ms05_bounded_send(ctx, u, packet, (size_t)expected);
}

static int udp_recv_data(struct ms05_udp *u, uint32_t sequence, uint32_t count,
                         uint32_t payload_size,
                         const struct ms05_deadline_ctx *ctx)
{
    uint8_t packet[sizeof(struct ms05_wire_header) + 64];
    ssize_t n = ms05_bounded_recv(ctx, u, packet, sizeof(packet));
    return ms05_validate_datagram(packet, n, sequence, count, payload_size);
}

static int udp_done_recv(struct ms05_udp *u, const char *mode,
                         const struct ms05_deadline_ctx *ctx)
{
    char control[96];
    char expected[16];
    uint32_t received = 0;
    int matched;
    if (udp_control_recv(u, control, sizeof(control), ctx) != 0) return -1;
    snprintf(expected, sizeof(expected), "MS05 DONE %s ", mode);
    matched = strncmp(control, expected, strlen(expected)) == 0;
    if (!matched) return -1;
    received = (uint32_t)strtoul(control + strlen(expected), NULL, 10);
    return (int)received;
}

static int udp_sent_done(struct ms05_udp *u, const char *mode, uint32_t sent,
                         const struct ms05_deadline_ctx *ctx)
{
    char control[96];
    snprintf(control, sizeof(control), "MS05 SENT %s %u", mode, sent);
    if (udp_control(u, control, ctx) != 0) return -1;
    return udp_done_recv(u, mode, ctx);
}

/* ── Mode runners ───────────────────────────────────────────────────── */

typedef int (*ms05_condition)(const struct ms05_snapshot *held,
                              const struct ms05_snapshot *current);
static int wait_for_condition(const char *phase,
                              const struct ms05_snapshot *held,
                              const struct ms05_deadline_ctx *ctx,
                              ms05_condition condition,
                              struct ms05_snapshot *out);
static int drain_tx(struct ms05_udp *u, const char *phase,
                    const struct ms05_snapshot *pre, uint32_t sent,
                    const struct ms05_deadline_ctx *ctx,
                    struct ms05_snapshot *out);

/* Shared: record phases, verify monotonic delta, safety and ledger. */
static int finalize_mode(const char *mode, const struct ms05_snapshot *pre,
                         const struct ms05_snapshot *post,
                         const uint8_t *required, size_t required_n,
                         const uint8_t *observed, size_t observed_n,
                         int valid)
{
    struct ms05_snapshot delta;
    int monotonic = ms05_snapshot_delta(pre, post, &delta) == 0;
    int order = ms05_phase_order_valid(observed, observed_n, required,
                                       required_n);
    printf("MS05 DELTA tx_submit=%lu tx_again=%lu tx_comp=%lu tx_reclaim=%lu "
           "tx_enq=%lu tx_deq=%lu buf_avail=%lu buf_inflight=%lu "
           "desc_avail=%lu desc_inflight=%lu live=%lu flush_ok=%lu\n",
           (unsigned long)delta.tx_submit, (unsigned long)delta.tx_again,
           (unsigned long)delta.tx_completion,
           (unsigned long)delta.tx_reclaim,
           (unsigned long)delta.tx_slot_enqueue,
           (unsigned long)delta.tx_slot_dequeue,
           (unsigned long)post->tx_buffer_available,
           (unsigned long)post->tx_buffer_inflight,
           (unsigned long)post->tx_descriptor_available,
           (unsigned long)post->tx_descriptor_inflight,
           (unsigned long)post->live, (unsigned long)delta.flush_success);
    if (!monotonic) {
        printf("MS05 FAIL mode=%s reason=counter-regression\n", mode);
        return 1;
    }
    if (!order) {
        printf("MS05 FAIL mode=%s reason=phase-order\n", mode);
        return 1;
    }
    return finish_mode(mode, valid);
}

static int run_snapshot(void)
{
    static const uint8_t required[MS05_PHASES_PLAIN] = {MS05_PHASE_PRE,
                                                        MS05_PHASE_POST};
    struct ms05_snapshot pre, post;
    uint8_t observed[MS05_PHASES_PLAIN] = {MS05_PHASE_PRE, MS05_PHASE_POST};
    struct ms05_snapshot delta;
    struct ms05_deadline_ctx ctx;
    uint64_t mode_start, mode_abs;
    int valid;

    if (g_ms05_ops.clock_now(&mode_start) != 0 ||
        ms05_mode_deadline_abs(mode_start, MS05_MODE_DEADLINE_MS,
                               &mode_abs) != 0) {
        return fail_mode("snapshot", "clock");
    }
    ctx.mode_start = mode_start;
    ctx.mode_abs = mode_abs;
    ctx.phase_start = mode_start;
    ctx.phase_deadline_ms = 0;
    if (snapshot_or_fail("snapshot", "MS05 PRE", &ctx, &pre) != 0) return 1;
    if (ms05_bounded_sleep(&ctx, 100) != 0) {
        return fail_mode("snapshot", "sleep");
    }
    if (snapshot_or_fail("snapshot", "MS05 POST", &ctx, &post) != 0) return 1;
    if (ms05_snapshot_delta(&pre, &post, &delta) != 0) {
        return fail_mode("snapshot", "counter-regression");
    }
    if (ms05_postcheck(&ctx) != 0) {
        return fail_mode("snapshot", "mode-deadline");
    }
    valid = ms05_common_valid(&post, &delta) && pre.hold_mode == 0 &&
            post.hold_mode == 0;
    return finalize_mode("snapshot", &pre, &post, required,
                         MS05_PHASES_PLAIN, observed, MS05_PHASES_PLAIN, valid);
}

static int run_tx_only(uint32_t count, uint32_t payload)
{
    static const uint8_t required[MS05_PHASES_PLAIN] = {MS05_PHASE_PRE,
                                                        MS05_PHASE_POST};
    struct ms05_udp u;
    struct ms05_snapshot pre, post, delta;
    uint8_t observed[MS05_PHASES_PLAIN] = {MS05_PHASE_PRE, MS05_PHASE_POST};
    struct ms05_deadline_ctx ctx;
    uint32_t sent = 0;
    int received;
    int valid;
    uint64_t mode_start, mode_abs, drain_start;

    if (g_ms05_ops.clock_now(&mode_start) != 0 ||
        ms05_mode_deadline_abs(mode_start, MS05_MODE_DEADLINE_MS,
                               &mode_abs) != 0) {
        return fail_mode("tx-only", "clock");
    }
    ctx.mode_start = mode_start;
    ctx.mode_abs = mode_abs;
    ctx.phase_start = mode_start;
    ctx.phase_deadline_ms = 0;
    if (udp_open(&u) != 0) return fail_mode("tx-only", "udp-open");
    if (udp_ready_handshake(&u, "tx-only", count, payload, &ctx) != 0) {
        g_ms05_ops.sock_close(&u);
        return fail_mode("tx-only", "handshake");
    }
    if (snapshot_or_fail("tx-only", "MS05 PRE", &ctx, &pre) != 0) {
        g_ms05_ops.sock_close(&u);
        return 1;
    }
    for (uint32_t sequence = 0; sequence < count; ++sequence) {
        if (ms05_precheck_budget(&ctx) == 0) break;
        if (udp_send_data(&u, sequence, count, payload, &ctx) == 0) {
            sent++;
        }
    }
    /* Drain is its own phase: capture a fresh drain start immediately before
     * it rather than reusing the mode start. */
    if (g_ms05_ops.clock_now(&drain_start) != 0) {
        g_ms05_ops.sock_close(&u);
        return fail_mode("tx-only", "clock");
    }
    ctx.phase_start = drain_start;
    ctx.phase_deadline_ms = MS05_DRAIN_DEADLINE_MS;
    if (drain_tx(&u, "MS05 POST", &pre, sent, &ctx, &post) != 0) {
        g_ms05_ops.sock_close(&u);
        return fail_mode("tx-only", "drain-deadline");
    }
    ctx.phase_start = mode_start;
    ctx.phase_deadline_ms = 0;
    received = udp_sent_done(&u, "tx-only", sent, &ctx);
    g_ms05_ops.sock_close(&u);
    if (ms05_postcheck(&ctx) != 0) {
        return fail_mode("tx-only", "mode-deadline");
    }
    if (ms05_snapshot_delta(&pre, &post, &delta) != 0) {
        return fail_mode("tx-only", "counter-regression");
    }
    valid = ms05_common_valid(&post, &delta) &&
            ms05_tx_ledger_closed(&pre, &post) && ms05_post_closed(&post) &&
            ms05_traffic_proved(MS05_TRAFFIC_EXACT, count, sent,
                                (uint32_t)received) &&
            delta.tx_submit >= sent;
    printf("MS05 WITNESS mode=tx-only sent=%u received=%d\n", sent, received);
    return finalize_mode("tx-only", &pre, &post, required, MS05_PHASES_PLAIN,
                         observed, MS05_PHASES_PLAIN, valid);
}

static int run_bidirectional(uint32_t count, uint32_t payload)
{
    static const uint8_t required[MS05_PHASES_PLAIN] = {MS05_PHASE_PRE,
                                                        MS05_PHASE_POST};
    struct ms05_udp u;
    struct ms05_snapshot pre, post, delta;
    uint8_t observed[MS05_PHASES_PLAIN] = {MS05_PHASE_PRE, MS05_PHASE_POST};
    struct ms05_deadline_ctx ctx;
    uint32_t sent = 0;
    uint32_t rx_received = 0;
    int host_received;
    int valid;
    uint64_t mode_start, mode_abs, drain_start;

    if (g_ms05_ops.clock_now(&mode_start) != 0 ||
        ms05_mode_deadline_abs(mode_start, MS05_MODE_DEADLINE_MS,
                               &mode_abs) != 0) {
        return fail_mode("bidirectional", "clock");
    }
    ctx.mode_start = mode_start;
    ctx.mode_abs = mode_abs;
    ctx.phase_start = mode_start;
    ctx.phase_deadline_ms = 0;
    if (udp_open(&u) != 0) return fail_mode("bidirectional", "udp-open");
    if (udp_ready_handshake(&u, "bidirectional", count, payload, &ctx) != 0) {
        g_ms05_ops.sock_close(&u);
        return fail_mode("bidirectional", "handshake");
    }
    if (snapshot_or_fail("bidirectional", "MS05 PRE", &ctx, &pre) != 0) {
        g_ms05_ops.sock_close(&u);
        return 1;
    }
    /* Host sends `count` datagrams to the guest (RX direction). */
    for (uint32_t sequence = 0; sequence < count; ++sequence) {
        if (ms05_precheck_budget(&ctx) == 0) break;
        if (udp_recv_data(&u, sequence, count, payload, &ctx) == 0) {
            rx_received++;
        }
    }
    /* Guest sends `count` datagrams to the host (TX direction). */
    for (uint32_t sequence = 0; sequence < count; ++sequence) {
        if (ms05_precheck_budget(&ctx) == 0) break;
        if (udp_send_data(&u, sequence, count, payload, &ctx) == 0) {
            sent++;
        }
    }
    /* Drain TX before SENT so the host can validate every accepted datagram.
     * Drain is its own phase with a fresh start. */
    if (g_ms05_ops.clock_now(&drain_start) != 0) {
        g_ms05_ops.sock_close(&u);
        return fail_mode("bidirectional", "clock");
    }
    ctx.phase_start = drain_start;
    ctx.phase_deadline_ms = MS05_DRAIN_DEADLINE_MS;
    if (drain_tx(&u, "MS05 POST", &pre, sent, &ctx, &post) != 0) {
        g_ms05_ops.sock_close(&u);
        return fail_mode("bidirectional", "drain-deadline");
    }
    ctx.phase_start = mode_start;
    ctx.phase_deadline_ms = 0;
    host_received = udp_sent_done(&u, "bidirectional", sent, &ctx);
    g_ms05_ops.sock_close(&u);
    if (ms05_postcheck(&ctx) != 0) {
        return fail_mode("bidirectional", "mode-deadline");
    }
    if (ms05_snapshot_delta(&pre, &post, &delta) != 0) {
        return fail_mode("bidirectional", "counter-regression");
    }
    valid = ms05_common_valid(&post, &delta) &&
            ms05_tx_ledger_closed(&pre, &post) && ms05_post_closed(&post) &&
            rx_received == count &&
            ms05_traffic_proved(MS05_TRAFFIC_EXACT, count, sent,
                                (uint32_t)host_received) &&
            delta.tx_submit >= sent && delta.reaped >= count &&
            delta.refilled >= count;
    printf("MS05 WITNESS mode=bidirectional tx_sent=%u rx_received=%u "
           "host_received=%d\n",
           sent, rx_received, host_received);
    return finalize_mode("bidirectional", &pre, &post, required,
                         MS05_PHASES_PLAIN, observed, MS05_PHASES_PLAIN, valid);
}

/* Polls V3 through the bounded seam until `condition` becomes true or the
 * phase/mode budget expires. A success is accepted only after re-reading the
 * clock and proving the current time is strictly before both the phase
 * deadline and the absolute mode deadline; equal/late completion fails even
 * when the condition is already true. The latest snapshot is stored in
 * `out`. Returns 0 on success, -1 otherwise. */
static int wait_for_condition(const char *phase,
                              const struct ms05_snapshot *held,
                              const struct ms05_deadline_ctx *ctx,
                              ms05_condition condition,
                              struct ms05_snapshot *out)
{
    uint64_t now;
    (void)phase;
    if (g_ms05_ops.clock_now(&now) != 0) return -1;
    for (;;) {
        if (ms05_ctx_budget_ms(ctx, now) == 0) return -1;
        if (ms05_bounded_snapshot(ctx, out) != 0) return -1;
        if (condition(held, out)) {
            if (ms05_postcheck(ctx) != 0) return -1;
            print_snapshot(phase, out);
            return 0;
        }
        if (ms05_bounded_sleep(ctx, MS05_POLL_SLEEP_MS) != 0) return -1;
        if (g_ms05_ops.clock_now(&now) != 0) return -1;
    }
}

static int slot_full_condition(const struct ms05_snapshot *held,
                               const struct ms05_snapshot *current)
{
    return ms05_slot_full_proved(held, current);
}

static int descriptor_full_condition(const struct ms05_snapshot *held,
                                     const struct ms05_snapshot *current)
{
    return ms05_descriptor_full_proved(held, current);
}

/* Waits until every datagram accepted by the stack (count `sent` relative to
 * `pre`) has been submitted to the driver and the TX slot/ticket/buffer/
 * descriptor ledger is exactly closed. Each iteration issues a non-blocking
 * recv (which runs the axnet recv path and therefore `poll_interfaces`), so
 * smoltcp-buffered residue frames are committed to slots and drained. A
 * success requires a fresh clock read strictly before both the phase and the
 * absolute mode deadline. The nonblocking transition and the nudge recv
 * follow the same pre/post deadline rule; restoring socket flags afterward is
 * explicit best-effort cleanup that cannot advance payload or change a PASS
 * decision. Returns 0 on completion, -1 otherwise. */
static int drain_tx(struct ms05_udp *u, const char *phase,
                    const struct ms05_snapshot *pre, uint32_t sent,
                    const struct ms05_deadline_ctx *ctx,
                    struct ms05_snapshot *out)
{
    uint64_t now, submit_target;
    uint8_t scratch[128];
    if (g_ms05_ops.clock_now(&now) != 0) return -1;
    if (UINT64_MAX - pre->tx_submit < sent) return -1; /* checked add */
    submit_target = pre->tx_submit + sent;
    /* The nonblocking transition never starts when the budget is exhausted
     * or regressed. */
    if (ms05_ctx_budget_ms(ctx, now) == 0) return -1;
    if (g_ms05_ops.sock_set_nonblock(u->fd, 1) != 0) return -1;
    for (;;) {
        if (ms05_ctx_budget_ms(ctx, now) == 0) {
            (void)g_ms05_ops.sock_set_nonblock(u->fd, 0);
            return -1;
        }
        if (ms05_bounded_snapshot(ctx, out) != 0) {
            (void)g_ms05_ops.sock_set_nonblock(u->fd, 0);
            return -1;
        }
        if (out->tx_submit >= submit_target && ms05_post_closed(out)) {
            if (ms05_postcheck(ctx) != 0) {
                (void)g_ms05_ops.sock_set_nonblock(u->fd, 0);
                return -1;
            }
            print_snapshot(phase, out);
            (void)g_ms05_ops.sock_set_nonblock(u->fd, 0);
            return 0;
        }
        if (g_ms05_ops.clock_now(&now) != 0) {
            (void)g_ms05_ops.sock_set_nonblock(u->fd, 0);
            return -1;
        }
        /* Nonblocking nudge recv: fresh precheck before, postcheck after its
         * return; a late/equal return cannot continue to the poll sleep. */
        if (ms05_ctx_budget_ms(ctx, now) == 0) {
            (void)g_ms05_ops.sock_set_nonblock(u->fd, 0);
            return -1;
        }
        (void)g_ms05_ops.sock_recv(u->fd, scratch, sizeof(scratch));
        if (ms05_postcheck(ctx) != 0) {
            (void)g_ms05_ops.sock_set_nonblock(u->fd, 0);
            return -1;
        }
        if (ms05_bounded_sleep(ctx, MS05_POLL_SLEEP_MS) != 0) {
            (void)g_ms05_ops.sock_set_nonblock(u->fd, 0);
            return -1;
        }
    }
}

/* Sends data datagrams until the FULL condition should hold, up to `count`
 * attempts, bounded by the hold-lease phase and the absolute mode deadline
 * so that held-mode sending can never spend `count * SO_SNDTIMEO` beyond
 * the lease. Each send timeout is clamped to the minimum positive remaining
 * phase and mode budget; a send near Full expiry cannot consume a fresh
 * operation timeout beyond the hold lease. Returns the number of datagrams
 * accepted by the stack (a capacity-full send returns -1/EAGAIN and stops
 * the loop). */
static uint32_t send_until_full(struct ms05_udp *u, uint32_t count,
                                uint32_t payload,
                                const struct ms05_deadline_ctx *ctx)
{
    uint32_t sent = 0;
    for (uint32_t sequence = 0; sequence < count; ++sequence) {
        if (ms05_precheck_budget(ctx) == 0) break;
        if (udp_send_data(u, sequence, count, payload, ctx) != 0) {
            break;
        }
        sent++;
    }
    return sent;
}

static int run_held_mode(const char *mode, uint64_t op)
{
    static const uint8_t required[MS05_PHASES_HELD] = {
        MS05_PHASE_PRE, MS05_PHASE_HELD, MS05_PHASE_FULL,
        MS05_PHASE_RELEASED, MS05_PHASE_POST};
    struct ms05_udp u;
    struct ms05_snapshot pre, held, full, rel_snap, post, delta;
    uint8_t observed[MS05_PHASES_HELD] = {
        MS05_PHASE_PRE, MS05_PHASE_HELD, MS05_PHASE_FULL,
        MS05_PHASE_RELEASED, MS05_PHASE_POST};
    struct ms05_deadline_ctx ctx;
    uint32_t sent = 0;
    int host_received = -1;
    uint64_t held_at, mode_start, mode_abs, drain_start;
    int valid = 0;
    int hold_active = 0;
    int released = 0;
    const char *reason = NULL;
    ms05_condition condition =
        op == MS05_CTL_HOLD_SUBMIT ? slot_full_condition
                                   : descriptor_full_condition;
    uint32_t count = MS05_DEFAULT_COUNT;
    uint32_t payload = MS05_DEFAULT_PAYLOAD;

    if (g_ms05_ops.clock_now(&mode_start) != 0 ||
        ms05_mode_deadline_abs(mode_start, MS05_MODE_DEADLINE_MS,
                               &mode_abs) != 0) {
        return fail_mode(mode, "clock");
    }
    ctx.mode_start = mode_start;
    ctx.mode_abs = mode_abs;
    ctx.phase_start = mode_start;
    ctx.phase_deadline_ms = 0;
    if (udp_open(&u) != 0) return fail_mode(mode, "udp-open");
    if (udp_ready_handshake(&u, mode, count, payload, &ctx) != 0) {
        g_ms05_ops.sock_close(&u);
        return fail_mode(mode, "handshake");
    }
    if (snapshot_or_fail(mode, "MS05 PRE", &ctx, &pre) != 0) {
        g_ms05_ops.sock_close(&u);
        return 1;
    }
    /* Hold commit: from here every exit runs the single cleanup block. */
    if (ms05_bounded_control(&ctx, op, MS05_HOLD_LEASE_MS) != 0) {
        g_ms05_ops.sock_close(&u);
        return fail_mode(mode, "hold-control");
    }
    hold_active = 1;
    if (ms05_bounded_snapshot(&ctx, &held) != 0) {
        reason = "MS05 HELD";
        goto out;
    }
    print_snapshot("MS05 HELD", &held);
    if (g_ms05_ops.clock_now(&held_at) != 0) {
        reason = "clock";
        goto out;
    }
    if (held.hold_mode != op) {
        reason = "hold-mode";
        goto out;
    }
    /* Drive TX traffic until the ledger reaches exact Full, bounded by the
     * FULL phase deadline so held-mode sending stays within the lease. */
    ctx.phase_start = held_at;
    ctx.phase_deadline_ms = MS05_FULL_DEADLINE_MS;
    sent = send_until_full(&u, count, payload, &ctx);
    if (wait_for_condition("MS05 FULL", &held, &ctx, condition, &full) != 0) {
        reason = "full-deadline";
        goto out;
    }
    if (ms05_bounded_control(&ctx, MS05_CTL_RELEASE, 0) != 0) {
        reason = "release-control";
        goto out;
    }
    released = 1;
    if (ms05_bounded_snapshot(&ctx, &rel_snap) != 0) {
        reason = "MS05 RELEASED";
        goto out;
    }
    print_snapshot("MS05 RELEASED", &rel_snap);
    if (rel_snap.hold_mode != 0) {
        reason = "released-mode";
        goto out;
    }
    /* Drain: occupancy returns to zero, every accepted datagram is submitted
     * to the driver and the slot/ticket/buffer/descriptor ledger closes
     * exactly. Drain is its own phase with a fresh start. */
    if (g_ms05_ops.clock_now(&drain_start) != 0) {
        reason = "clock";
        goto out;
    }
    ctx.phase_start = drain_start;
    ctx.phase_deadline_ms = MS05_DRAIN_DEADLINE_MS;
    if (drain_tx(&u, "MS05 POST", &pre, sent, &ctx, &post) != 0) {
        reason = "drain-deadline";
        goto out;
    }
    ctx.phase_start = mode_start;
    ctx.phase_deadline_ms = 0;
    host_received = udp_sent_done(&u, mode, sent, &ctx);
    if (ms05_postcheck(&ctx) != 0) {
        reason = "mode-deadline";
        goto out;
    }

out:
    /* Single cleanup owner: at most one bounded Release under the original
     * absolute mode deadline when Hold is still active and unreleased. A
     * cleanup Release failure is reported but creates no retry entry. */
    if (hold_active && !released) {
        struct ms05_deadline_ctx cleanup_ctx = ctx;
        released = 1;
        cleanup_ctx.phase_start = 0;
        cleanup_ctx.phase_deadline_ms = 0;
        if (ms05_bounded_control(&cleanup_ctx, MS05_CTL_RELEASE, 0) != 0) {
            printf("MS05 WARN mode=%s release-in-cleanup-failed\n", mode);
        }
    }
    g_ms05_ops.sock_close(&u);
    if (reason != NULL) return fail_mode(mode, reason);

    if (ms05_snapshot_delta(&pre, &post, &delta) != 0) {
        return fail_mode(mode, "counter-regression");
    }
    if (op == MS05_CTL_HOLD_SUBMIT) {
        valid = ms05_slot_full_proved(&held, &full) &&
                ms05_common_valid(&post, &delta) &&
                ms05_tx_ledger_closed(&pre, &post) &&
                ms05_post_closed(&post) &&
                ms05_traffic_proved(MS05_TRAFFIC_HELD, count, sent,
                                    (uint32_t)host_received) &&
                delta.auto_release_failure == 0;
    } else {
        valid = ms05_descriptor_full_proved(&held, &full) &&
                ms05_common_valid(&post, &delta) &&
                ms05_tx_ledger_closed(&pre, &post) &&
                ms05_post_closed(&post) &&
                ms05_traffic_proved(MS05_TRAFFIC_HELD, count, sent,
                                    (uint32_t)host_received) &&
                delta.auto_release_failure == 0;
    }
    printf("MS05 WITNESS mode=%s sent=%u host_received=%d\n", mode, sent,
           host_received);
    return finalize_mode(mode, &pre, &post, required, MS05_PHASES_HELD,
                         observed, MS05_PHASES_HELD, valid);
}

static int run_slot_full(void)
{
    return run_held_mode("slot-full", MS05_CTL_HOLD_SUBMIT);
}

static int run_descriptor_full(void)
{
    return run_held_mode("descriptor-full", MS05_CTL_HOLD_RECLAIM);
}

static int run_flush(void)
{
    static const uint8_t required[MS05_PHASES_PLAIN] = {MS05_PHASE_PRE,
                                                        MS05_PHASE_POST};
    struct ms05_udp u;
    struct ms05_snapshot pre, post, delta;
    uint8_t observed[MS05_PHASES_PLAIN] = {MS05_PHASE_PRE, MS05_PHASE_POST};
    struct ms05_deadline_ctx ctx;
    uint32_t count = MS05_DEFAULT_COUNT;
    uint32_t payload = MS05_DEFAULT_PAYLOAD;
    uint32_t sent = 0;
    int host_received;
    int valid;
    uint64_t mode_start, mode_abs, drain_start;

    if (g_ms05_ops.clock_now(&mode_start) != 0 ||
        ms05_mode_deadline_abs(mode_start, MS05_MODE_DEADLINE_MS,
                               &mode_abs) != 0) {
        return fail_mode("flush", "clock");
    }
    ctx.mode_start = mode_start;
    ctx.mode_abs = mode_abs;
    ctx.phase_start = mode_start;
    ctx.phase_deadline_ms = 0;
    if (udp_open(&u) != 0) return fail_mode("flush", "udp-open");
    if (udp_ready_handshake(&u, "flush", count, payload, &ctx) != 0) {
        g_ms05_ops.sock_close(&u);
        return fail_mode("flush", "handshake");
    }
    if (snapshot_or_fail("flush", "MS05 PRE", &ctx, &pre) != 0) {
        g_ms05_ops.sock_close(&u);
        return 1;
    }
    for (uint32_t sequence = 0; sequence < count; ++sequence) {
        if (ms05_precheck_budget(&ctx) == 0) break;
        if (udp_send_data(&u, sequence, count, payload, &ctx) == 0) {
            sent++;
        }
    }
    /* The blocking flush ioctl gets a preflight budget check and a post-return
     * absolute-deadline recheck so it can never extend the mode bound. */
    if (ms05_bounded_flush(&ctx) != 0) {
        g_ms05_ops.sock_close(&u);
        return fail_mode("flush", "flush-ioctl");
    }
    /* Flush waits for reclaim of the construction-time target; drain the
     * residue (smoltcp-buffered frames committed by socket ops) before SENT.
     * Drain is its own phase with a fresh start. */
    if (g_ms05_ops.clock_now(&drain_start) != 0) {
        g_ms05_ops.sock_close(&u);
        return fail_mode("flush", "clock");
    }
    ctx.phase_start = drain_start;
    ctx.phase_deadline_ms = MS05_DRAIN_DEADLINE_MS;
    if (drain_tx(&u, "MS05 POST", &pre, sent, &ctx, &post) != 0) {
        g_ms05_ops.sock_close(&u);
        return fail_mode("flush", "drain-deadline");
    }
    ctx.phase_start = mode_start;
    ctx.phase_deadline_ms = 0;
    host_received = udp_sent_done(&u, "flush", sent, &ctx);
    g_ms05_ops.sock_close(&u);
    if (ms05_postcheck(&ctx) != 0) {
        return fail_mode("flush", "mode-deadline");
    }
    if (ms05_snapshot_delta(&pre, &post, &delta) != 0) {
        return fail_mode("flush", "counter-regression");
    }
    valid = ms05_common_valid(&post, &delta) &&
            ms05_flush_proved(&pre, &post) &&
            ms05_traffic_proved(MS05_TRAFFIC_EXACT, count, sent,
                                (uint32_t)host_received) &&
            delta.tx_submit >= sent;
    printf("MS05 WITNESS mode=flush sent=%u host_received=%d\n", sent,
           host_received);
    return finalize_mode("flush", &pre, &post, required, MS05_PHASES_PLAIN,
                         observed, MS05_PHASES_PLAIN, valid);
}

#ifndef MS05_DATA_PLANE_PROBE_TESTING

int main(int argc, char **argv)
{
    uint32_t count = MS05_DEFAULT_COUNT;
    uint32_t payload = MS05_DEFAULT_PAYLOAD;

    if (argc < 2) {
        fprintf(stderr, "usage: %s snapshot|tx-only|bidirectional|"
                        "slot-full|descriptor-full|flush [count] [payload]\n",
                argv[0]);
        return 2;
    }
    if (argc >= 3) {
        count = (uint32_t)strtoul(argv[2], NULL, 10);
    }
    if (argc >= 4) {
        payload = (uint32_t)strtoul(argv[3], NULL, 10);
    }
    if (count == 0 || count > 4096 || payload == 0 || payload > 64) {
        fprintf(stderr, "invalid count/payload bounds\n");
        return 2;
    }
    if (strcmp(argv[1], "snapshot") == 0) return run_snapshot();
    if (strcmp(argv[1], "tx-only") == 0) {
        return run_tx_only(count, payload);
    }
    if (strcmp(argv[1], "bidirectional") == 0) {
        return run_bidirectional(count, payload);
    }
    if (strcmp(argv[1], "slot-full") == 0) return run_slot_full();
    if (strcmp(argv[1], "descriptor-full") == 0) return run_descriptor_full();
    if (strcmp(argv[1], "flush") == 0) return run_flush();
    fprintf(stderr, "unknown mode: %s\n", argv[1]);
    return 2;
}

#endif /* MS05_DATA_PLANE_PROBE_TESTING */
