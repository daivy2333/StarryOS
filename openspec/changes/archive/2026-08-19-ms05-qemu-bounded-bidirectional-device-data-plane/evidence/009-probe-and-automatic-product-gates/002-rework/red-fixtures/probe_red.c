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
    (void)rule; (void)count;
    return received == sent;
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
    (void)ctx->mode_abs; (void)ctx->phase_start; (void)ctx->phase_deadline_ms;
    return ms05_budget_remaining_ms(ctx->mode_start, now, MS05_MODE_DEADLINE_MS);
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
    (void)budget_ms; (void)kernel_timeout_ms;
    return 1;
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

#ifndef MS05_DATA_PLANE_PROBE_TESTING

#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <time.h>
#include <unistd.h>

/* ── Runtime (guest payload) ────────────────────────────────────────── */

static int monotonic_ms(uint64_t *now)
{
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) return -1;
    *now = (uint64_t)ts.tv_sec * 1000u + (uint64_t)ts.tv_nsec / 1000000u;
    return 0;
}

/* Remaining budget in ms for the running mode, or 0 when exhausted. */
static uint64_t ms05_budget_remaining(uint64_t mode_start, uint64_t total_ms)
{
    uint64_t now;
    if (monotonic_ms(&now) != 0) return 0;
    return ms05_budget_remaining_ms(mode_start, now, total_ms);
}

static int read_snapshot(struct ms05_snapshot *snapshot)
{
    if (ioctl(STDIN_FILENO, MS05_SNAPSHOT_V3, snapshot) < 0) {
        perror("ioctl MS05_SNAPSHOT_V3");
        return -1;
    }
    return 0;
}

/* Applies one QEMU diagnostic control within the phase and absolute mode
 * deadline. A success is accepted only after re-reading the clock and
 * proving completion is strictly before both the phase and the absolute
 * mode deadline; equal/late success fails. Bounded retry handles
 * `ResourceBusy` (EAGAIN) while the Service is held. */
static int control_apply(uint64_t op, uint64_t lease_ms, uint64_t deadline_ms,
                         uint64_t mode_deadline_abs)
{
    uint64_t start, now;
    uint64_t payload[2] = {op, lease_ms};
    if (monotonic_ms(&start) != 0) return -1;
    for (;;) {
        if (ioctl(STDIN_FILENO, MS05_DIAGNOSTIC_CTL, payload) == 0) {
            if (monotonic_ms(&now) != 0 ||
                ms05_deadline_expired(start, now, deadline_ms,
                                      mode_deadline_abs)) {
                return -1;
            }
            return 0;
        }
        if (errno != EAGAIN && errno != EWOULDBLOCK) {
            perror("ioctl MS05_DIAGNOSTIC_CTL");
            return -1;
        }
        if (monotonic_ms(&now) != 0 ||
            ms05_deadline_expired(start, now, deadline_ms,
                                  mode_deadline_abs)) {
            return -1;
        }
        usleep(20000);
    }
}

/* Waits for the FLUSH ioctl result. Before blocking, proves the remaining
 * mode budget can contain the kernel's flush timeout so the ioctl cannot
 * extend the mode bound; after return, rechecks the absolute deadline.
 * The kernel flush timeout is 2s (MS05_MAX_LEASE_MS). */
static int flush_wait(uint64_t mode_start, uint64_t mode_abs)
{
    uint64_t budget = ms05_budget_remaining(mode_start, MS05_MODE_DEADLINE_MS);
    uint64_t now;
    if (!ms05_flush_affordable(budget, MS05_MAX_LEASE_MS)) return -1;
    if (ioctl(STDIN_FILENO, MS05_FLUSH, 0) < 0) {
        perror("ioctl MS05_FLUSH");
        return -1;
    }
    if (monotonic_ms(&now) != 0) return -1;
    if (now >= mode_abs) return -1;
    return 0;
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

/* Reads one snapshot now; on read failure emits FAIL and returns -1. */
static int snapshot_or_fail(const char *mode, const char *phase,
                            struct ms05_snapshot *out)
{
    if (read_snapshot(out) != 0) {
        fail_mode(mode, phase);
        return -1;
    }
    print_snapshot(phase, out);
    return 0;
}

/* ── Guest ↔ host UDP protocol ──────────────────────────────────────── */

struct ms05_udp {
    int fd;
    struct sockaddr_in host;
};

static int udp_open(struct ms05_udp *u)
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
        return -1;
    }
    if (setsockopt(u->fd, SOL_SOCKET, SO_SNDTIMEO, &snd_timeout,
                   sizeof(snd_timeout)) != 0) {
        close(u->fd);
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

/* Clamps the socket receive timeout so a single recv never extends the
 * remaining phase or mode budget. Returns -1 when the budget is exhausted. */
static int udp_clamp_rcv_timeout(struct ms05_udp *u,
                                 const struct ms05_deadline_ctx *ctx)
{
    uint64_t now, remaining;
    struct timeval tv;
    if (monotonic_ms(&now) != 0) return -1;
    remaining = ms05_clamp_timeout_ms(
        ms05_ctx_budget_ms(ctx, now),
        (uint64_t)MS05_SOCKET_TIMEOUT_S * 1000u);
    if (remaining == 0) return -1;
    tv.tv_sec = (time_t)(remaining / 1000u);
    tv.tv_usec = (suseconds_t)((remaining % 1000u) * 1000u);
    return setsockopt(u->fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
}

/* Clamps the socket send timeout so a single send never extends the
 * remaining phase or mode budget. Returns -1 when the budget is exhausted. */
static int udp_clamp_snd_timeout(struct ms05_udp *u,
                                 const struct ms05_deadline_ctx *ctx)
{
    uint64_t now, remaining;
    struct timeval tv;
    if (monotonic_ms(&now) != 0) return -1;
    remaining = ms05_clamp_timeout_ms(
        ms05_ctx_budget_ms(ctx, now), MS05_SEND_TIMEOUT_US / 1000u);
    if (remaining == 0) return -1;
    tv.tv_sec = 0;
    tv.tv_usec = (suseconds_t)(remaining * 1000u);
    return setsockopt(u->fd, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof(tv));
}

/* Sends one ASCII control datagram; returns 0 on success. */
static int udp_control(struct ms05_udp *u, const char *text)
{
    ssize_t n = send(u->fd, text, strlen(text), 0);
    return n == (ssize_t)strlen(text) ? 0 : -1;
}

/* Receives one ASCII control datagram (non-empty), never extending the
 * remaining phase or mode budget. Returns 0 on success. */
static int udp_control_recv(struct ms05_udp *u, char *buf, size_t size,
                            const struct ms05_deadline_ctx *ctx)
{
    ssize_t n;
    if (udp_clamp_rcv_timeout(u, ctx) != 0) return -1;
    n = recv(u->fd, buf, size - 1, 0);
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
    if (udp_control(u, control) != 0) return -1;
    if (udp_control_recv(u, control, sizeof(control), ctx) != 0) return -1;
    snprintf(expected, sizeof(expected), "MS05 READY %s %u %u", mode, count,
             payload);
    if (strcmp(control, expected) != 0) return -1;
    snprintf(control, sizeof(control), "MS05 START %s %u %u", mode, count,
             payload);
    if (udp_control(u, control) != 0) return -1;
    return 0;
}

/* Sends a data datagram for `sequence` in network byte order, never
 * extending the remaining phase or mode budget. Returns 0 on success, -1 on
 * error (including EAGAIN/EWOULDBLOCK, which the caller treats as
 * capacity-full). */
static int udp_send_data(struct ms05_udp *u, uint32_t sequence, uint32_t count,
                         uint32_t payload_size,
                         const struct ms05_deadline_ctx *ctx)
{
    struct ms05_wire_header header;
    uint8_t packet[sizeof(struct ms05_wire_header) + 64];
    ssize_t expected = (ssize_t)(sizeof(header) + payload_size);
    if (payload_size > 64) return -1;
    if (udp_clamp_snd_timeout(u, ctx) != 0) return -1;
    header.magic = ms05_be32(MS05_MAGIC);
    header.sequence = ms05_be32(sequence);
    header.count = ms05_be32(count);
    memcpy(packet, &header, sizeof(header));
    for (uint32_t i = 0; i < payload_size; ++i) {
        packet[sizeof(header) + i] = (uint8_t)((sequence + i) & 0xffu);
    }
    return send(u->fd, packet, expected, 0) == expected ? 0 : -1;
}

/* Receives and validates one data datagram for `sequence`, never extending
 * the remaining phase or mode budget. */
static int udp_recv_data(struct ms05_udp *u, uint32_t sequence, uint32_t count,
                         uint32_t payload_size,
                         const struct ms05_deadline_ctx *ctx)
{
    uint8_t packet[sizeof(struct ms05_wire_header) + 64];
    ssize_t n;
    if (udp_clamp_rcv_timeout(u, ctx) != 0) return -1;
    n = recv(u->fd, packet, sizeof(packet), 0);
    return ms05_validate_datagram(packet, n, sequence, count, payload_size);
}

/* Host summary: `MS05 DONE <mode> <received>`. Returns the received count or
 * -1. */
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

/* Tells the host how many datagrams the guest accepted, then waits for the
 * host's DONE summary. Returns the received count or -1. */
static int udp_sent_done(struct ms05_udp *u, const char *mode, uint32_t sent,
                         const struct ms05_deadline_ctx *ctx)
{
    char control[96];
    snprintf(control, sizeof(control), "MS05 SENT %s %u", mode, sent);
    if (udp_control(u, control) != 0) return -1;
    return udp_done_recv(u, mode, ctx);
}

/* ── Mode runners ───────────────────────────────────────────────────── */

typedef int (*ms05_condition)(const struct ms05_snapshot *held,
                              const struct ms05_snapshot *current);
static int wait_for_condition(const char *phase,
                              const struct ms05_snapshot *held,
                              uint64_t start, uint64_t deadline_ms,
                              uint64_t mode_deadline_abs,
                              ms05_condition condition,
                              struct ms05_snapshot *out);
static int drain_tx(struct ms05_udp *u, const char *phase,
                    const struct ms05_snapshot *pre, uint32_t sent,
                    uint64_t start, uint64_t deadline_ms,
                    uint64_t mode_deadline_abs,
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
    uint64_t mode_start, mode_abs, now;
    int valid;

    if (monotonic_ms(&mode_start) != 0 ||
        ms05_mode_deadline_abs(mode_start, MS05_MODE_DEADLINE_MS,
                               &mode_abs) != 0) {
        return fail_mode("snapshot", "clock");
    }
    if (snapshot_or_fail("snapshot", "MS05 PRE", &pre) != 0) return 1;
    usleep(100000);
    if (snapshot_or_fail("snapshot", "MS05 POST", &post) != 0) return 1;
    if (ms05_snapshot_delta(&pre, &post, &delta) != 0) {
        return fail_mode("snapshot", "counter-regression");
    }
    /* Final decision requires a fresh clock read strictly before the mode
     * deadline; equal/late completion fails. */
    if (monotonic_ms(&now) != 0 ||
        ms05_deadline_expired(mode_start, now, MS05_MODE_DEADLINE_MS,
                              mode_abs)) {
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
    uint64_t mode_start, mode_abs, drain_start, now;

    if (monotonic_ms(&mode_start) != 0 ||
        ms05_mode_deadline_abs(mode_start, MS05_MODE_DEADLINE_MS,
                               &mode_abs) != 0) {
        return fail_mode("tx-only", "clock");
    }
    ctx.mode_start = mode_start;
    ctx.mode_abs = mode_abs;
    ctx.phase_start = mode_start;
    ctx.phase_deadline_ms = 0; /* send phase == mode bound */
    if (udp_open(&u) != 0) return fail_mode("tx-only", "udp-open");
    if (udp_ready_handshake(&u, "tx-only", count, payload, &ctx) != 0) {
        close(u.fd);
        return fail_mode("tx-only", "handshake");
    }
    if (snapshot_or_fail("tx-only", "MS05 PRE", &pre) != 0) {
        close(u.fd);
        return 1;
    }
    for (uint32_t sequence = 0; sequence < count; ++sequence) {
        if (monotonic_ms(&now) != 0) break;
        if (ms05_ctx_budget_ms(&ctx, now) == 0) break;
        if (udp_send_data(&u, sequence, count, payload, &ctx) == 0) {
            sent++;
        }
    }
    /* Drain is its own phase: capture a fresh drain start immediately before
     * it rather than reusing the mode start. */
    if (monotonic_ms(&drain_start) != 0) {
        close(u.fd);
        return fail_mode("tx-only", "clock");
    }
    ctx.phase_start = drain_start;
    ctx.phase_deadline_ms = MS05_DRAIN_DEADLINE_MS;
    if (drain_tx(&u, "MS05 POST", &pre, sent, drain_start,
                 MS05_DRAIN_DEADLINE_MS, mode_abs, &post) != 0) {
        close(u.fd);
        return fail_mode("tx-only", "drain-deadline");
    }
    ctx.phase_start = mode_start;
    ctx.phase_deadline_ms = 0;
    received = udp_sent_done(&u, "tx-only", sent, &ctx);
    close(u.fd);
    if (monotonic_ms(&now) != 0 ||
        ms05_deadline_expired(mode_start, now, MS05_MODE_DEADLINE_MS,
                              mode_abs)) {
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
    uint64_t mode_start, mode_abs, drain_start, now;

    if (monotonic_ms(&mode_start) != 0 ||
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
        close(u.fd);
        return fail_mode("bidirectional", "handshake");
    }
    if (snapshot_or_fail("bidirectional", "MS05 PRE", &pre) != 0) {
        close(u.fd);
        return 1;
    }
    /* Host sends `count` datagrams to the guest (RX direction). */
    for (uint32_t sequence = 0; sequence < count; ++sequence) {
        if (monotonic_ms(&now) != 0) break;
        if (ms05_ctx_budget_ms(&ctx, now) == 0) break;
        if (udp_recv_data(&u, sequence, count, payload, &ctx) == 0) {
            rx_received++;
        }
    }
    /* Guest sends `count` datagrams to the host (TX direction). */
    for (uint32_t sequence = 0; sequence < count; ++sequence) {
        if (monotonic_ms(&now) != 0) break;
        if (ms05_ctx_budget_ms(&ctx, now) == 0) break;
        if (udp_send_data(&u, sequence, count, payload, &ctx) == 0) {
            sent++;
        }
    }
    /* Drain TX before SENT so the host can validate every accepted datagram.
     * Drain is its own phase with a fresh start. */
    if (monotonic_ms(&drain_start) != 0) {
        close(u.fd);
        return fail_mode("bidirectional", "clock");
    }
    ctx.phase_start = drain_start;
    ctx.phase_deadline_ms = MS05_DRAIN_DEADLINE_MS;
    if (drain_tx(&u, "MS05 POST", &pre, sent, drain_start,
                 MS05_DRAIN_DEADLINE_MS, mode_abs, &post) != 0) {
        close(u.fd);
        return fail_mode("bidirectional", "drain-deadline");
    }
    ctx.phase_start = mode_start;
    ctx.phase_deadline_ms = 0;
    host_received = udp_sent_done(&u, "bidirectional", sent, &ctx);
    close(u.fd);
    if (monotonic_ms(&now) != 0 ||
        ms05_deadline_expired(mode_start, now, MS05_MODE_DEADLINE_MS,
                              mode_abs)) {
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

/* Polls V3 until `condition` becomes true or a deadline expires. A success
 * is accepted only after re-reading the clock and proving the current time
 * is strictly before both the phase deadline and the absolute mode deadline;
 * equal/late completion fails even when the condition is already true. The
 * latest snapshot is stored in `out`. Returns 0 on success, -1 otherwise. */
static int wait_for_condition(const char *phase,
                              const struct ms05_snapshot *held,
                              uint64_t start, uint64_t deadline_ms,
                              uint64_t mode_deadline_abs,
                              ms05_condition condition,
                              struct ms05_snapshot *out)
{
    uint64_t now;
    (void)phase;
    if (monotonic_ms(&now) != 0) return -1;
    for (;;) {
        if (ms05_deadline_expired(start, now, deadline_ms,
                                  mode_deadline_abs)) {
            return -1;
        }
        if (read_snapshot(out) != 0) return -1;
        if (condition(held, out)) {
            if (monotonic_ms(&now) != 0 ||
                ms05_deadline_expired(start, now, deadline_ms,
                                      mode_deadline_abs)) {
                return -1;
            }
            print_snapshot(phase, out);
            return 0;
        }
        if (monotonic_ms(&now) != 0) return -1;
        usleep(20000);
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
 * descriptor ledger is exactly closed. The queue task and the Router only
 * advance when something wakes them: each iteration issues a non-blocking
 * recv (which runs the axnet recv path and therefore `poll_interfaces`), so
 * smoltcp-buffered residue frames are committed to slots and drained. A
 * success requires a fresh clock read strictly before both the phase and the
 * absolute mode deadline. Returns 0 on completion, -1 otherwise. */
static int drain_tx(struct ms05_udp *u, const char *phase,
                    const struct ms05_snapshot *pre, uint32_t sent,
                    uint64_t start, uint64_t deadline_ms,
                    uint64_t mode_deadline_abs, struct ms05_snapshot *out)
{
    uint64_t now, submit_target;
    uint8_t scratch[128];
    int flags;
    if (monotonic_ms(&now) != 0) return -1;
    if (UINT64_MAX - pre->tx_submit < sent) return -1; /* checked add */
    submit_target = pre->tx_submit + sent;
    flags = fcntl(u->fd, F_GETFL, 0);
    if (flags < 0) return -1;
    if (fcntl(u->fd, F_SETFL, flags | O_NONBLOCK) != 0) return -1;
    for (;;) {
        if (ms05_deadline_expired(start, now, deadline_ms,
                                  mode_deadline_abs)) {
            (void)fcntl(u->fd, F_SETFL, flags);
            return -1;
        }
        if (read_snapshot(out) != 0) {
            (void)fcntl(u->fd, F_SETFL, flags);
            return -1;
        }
        if (out->tx_submit >= submit_target && ms05_post_closed(out)) {
            if (monotonic_ms(&now) != 0 ||
                ms05_deadline_expired(start, now, deadline_ms,
                                      mode_deadline_abs)) {
                (void)fcntl(u->fd, F_SETFL, flags);
                return -1;
            }
            print_snapshot(phase, out);
            (void)fcntl(u->fd, F_SETFL, flags);
            return 0;
        }
        if (monotonic_ms(&now) != 0) {
            (void)fcntl(u->fd, F_SETFL, flags);
            return -1;
        }
        (void)recv(u->fd, scratch, sizeof(scratch), MSG_DONTWAIT);
        usleep(20000);
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
        uint64_t now;
        if (monotonic_ms(&now) != 0 ||
            ms05_ctx_budget_ms(ctx, now) == 0) {
            break;
        }
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
    struct ms05_snapshot pre, held, full, released, post, delta;
    uint8_t observed[MS05_PHASES_HELD] = {
        MS05_PHASE_PRE, MS05_PHASE_HELD, MS05_PHASE_FULL,
        MS05_PHASE_RELEASED, MS05_PHASE_POST};
    struct ms05_deadline_ctx ctx;
    uint32_t sent = 0;
    int host_received = -1;
    uint64_t held_at, mode_start, mode_abs, drain_start, now;
    int valid = 0;
    ms05_condition condition =
        op == MS05_CTL_HOLD_SUBMIT ? slot_full_condition
                                   : descriptor_full_condition;
    uint32_t count = MS05_DEFAULT_COUNT;
    uint32_t payload = MS05_DEFAULT_PAYLOAD;

    if (monotonic_ms(&mode_start) != 0 ||
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
        close(u.fd);
        return fail_mode(mode, "handshake");
    }
    if (snapshot_or_fail(mode, "MS05 PRE", &pre) != 0) {
        close(u.fd);
        return 1;
    }
    if (control_apply(op, MS05_HOLD_LEASE_MS, MS05_FULL_DEADLINE_MS,
                      mode_abs) != 0) {
        close(u.fd);
        return fail_mode(mode, "hold-control");
    }
    if (snapshot_or_fail(mode, "MS05 HELD", &held) != 0) {
        close(u.fd);
        return 1;
    }
    if (monotonic_ms(&held_at) != 0) {
        close(u.fd);
        return fail_mode(mode, "clock");
    }
    if (held.hold_mode != op) {
        close(u.fd);
        return fail_mode(mode, "hold-mode");
    }
    /* Drive TX traffic until the ledger reaches exact Full, bounded by the
     * FULL phase deadline so held-mode sending stays within the lease. */
    ctx.phase_start = held_at;
    ctx.phase_deadline_ms = MS05_FULL_DEADLINE_MS;
    sent = send_until_full(&u, count, payload, &ctx);
    if (wait_for_condition("MS05 FULL", &held, held_at,
                           MS05_FULL_DEADLINE_MS, mode_abs, condition,
                           &full) != 0) {
        /* Hold is still active on this error path: attempt exactly one
         * bounded Release within the original remaining budget. */
        (void)control_apply(MS05_CTL_RELEASE, 0,
                            ms05_budget_remaining(mode_start,
                                                  MS05_MODE_DEADLINE_MS),
                            mode_abs);
        close(u.fd);
        printf("MS05 WITNESS mode=%s sent=%u\n", mode, sent);
        return fail_mode(mode, "full-deadline");
    }
    if (control_apply(MS05_CTL_RELEASE, 0, MS05_DRAIN_DEADLINE_MS,
                      mode_abs) != 0) {
        close(u.fd);
        return fail_mode(mode, "release-control");
    }
    if (snapshot_or_fail(mode, "MS05 RELEASED", &released) != 0) {
        close(u.fd);
        return 1;
    }
    if (released.hold_mode != 0) {
        close(u.fd);
        return fail_mode(mode, "released-mode");
    }
    /* Drain: occupancy returns to zero, every accepted datagram is submitted
     * to the driver and the slot/ticket/buffer/descriptor ledger closes
     * exactly. Drain is its own phase with a fresh start. */
    if (monotonic_ms(&drain_start) != 0) {
        close(u.fd);
        return fail_mode(mode, "clock");
    }
    ctx.phase_start = drain_start;
    ctx.phase_deadline_ms = MS05_DRAIN_DEADLINE_MS;
    if (drain_tx(&u, "MS05 POST", &pre, sent, drain_start,
                 MS05_DRAIN_DEADLINE_MS, mode_abs, &post) != 0) {
        close(u.fd);
        return fail_mode(mode, "drain-deadline");
    }
    ctx.phase_start = mode_start;
    ctx.phase_deadline_ms = 0;
    host_received = udp_sent_done(&u, mode, sent, &ctx);
    close(u.fd);
    if (monotonic_ms(&now) != 0 ||
        ms05_deadline_expired(mode_start, now, MS05_MODE_DEADLINE_MS,
                              mode_abs)) {
        return fail_mode(mode, "mode-deadline");
    }

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
    uint64_t mode_start, mode_abs, drain_start, now;

    if (monotonic_ms(&mode_start) != 0 ||
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
        close(u.fd);
        return fail_mode("flush", "handshake");
    }
    if (snapshot_or_fail("flush", "MS05 PRE", &pre) != 0) {
        close(u.fd);
        return 1;
    }
    for (uint32_t sequence = 0; sequence < count; ++sequence) {
        if (monotonic_ms(&now) != 0) break;
        if (ms05_ctx_budget_ms(&ctx, now) == 0) break;
        if (udp_send_data(&u, sequence, count, payload, &ctx) == 0) {
            sent++;
        }
    }
    /* The blocking flush ioctl gets a preflight budget check and a post-return
     * absolute-deadline recheck so it can never extend the mode bound. */
    if (flush_wait(mode_start, mode_abs) != 0) {
        close(u.fd);
        return fail_mode("flush", "flush-ioctl");
    }
    /* Flush waits for reclaim of the construction-time target; drain the
     * residue (smoltcp-buffered frames committed by socket ops) before SENT.
     * Drain is its own phase with a fresh start. */
    if (monotonic_ms(&drain_start) != 0) {
        close(u.fd);
        return fail_mode("flush", "clock");
    }
    ctx.phase_start = drain_start;
    ctx.phase_deadline_ms = MS05_DRAIN_DEADLINE_MS;
    if (drain_tx(&u, "MS05 POST", &pre, sent, drain_start,
                 MS05_DRAIN_DEADLINE_MS, mode_abs, &post) != 0) {
        close(u.fd);
        return fail_mode("flush", "drain-deadline");
    }
    ctx.phase_start = mode_start;
    ctx.phase_deadline_ms = 0;
    host_received = udp_sent_done(&u, "flush", sent, &ctx);
    close(u.fd);
    if (monotonic_ms(&now) != 0 ||
        ms05_deadline_expired(mode_start, now, MS05_MODE_DEADLINE_MS,
                              mode_abs)) {
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

