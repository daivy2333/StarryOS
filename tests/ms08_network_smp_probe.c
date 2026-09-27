/* MS08 network SMP qualification guest probe (Iteration 004 task 5.2,
 * Iteration 005 final profile).
 *
 * Pure decision core shared with the host harness
 * (tests/ms08_network_smp_probe_test.c): placement, timer-disabled wake,
 * full-recovery and quiet checks are plain C.  The bounded TCP/UDP echo peer
 * lives in scripts/ms08-network-peer.py and is started by the operator before
 * the guest probe.  Migration/reset/link decision predicates remain in this
 * file for the host decision tests but the final runtime does not run those
 * cases (Iteration 005 replan).  All waits are poll-based with absolute
 * deadlines.
 *
 * Transcript grammar (consumed by scripts/ms08-network-validate.py):
 *   MS08_NET_START / MS08_NET_ENV: <text>            (env is diagnostic only)
 *   MS08_NET_CASE_START: <case> ... PASS: <case>     (frozen case order)
 *   MS08_NET_V5: case=<case> <V5 fields>             (118-u64 wire decoded)
 *   MS08_NET_WAKE: case=timer-disabled-wake <result>
 *   MS08_NET_DIAG: case=full-recovery op=<op> result=<r>
 *   MS08_NET_SOCK: case=<case> result=rw sent=N received=N
 *   MS08_NET_PEER: case=<case> result=ok
 *   MS08_NET_END / MS08_NET_HARNESS_EXIT: 0
 */
#define _GNU_SOURCE /* syscall(2) + SYS_sched_* + SO_ERROR for the baton. */
#include <errno.h>
#include <arpa/inet.h>
#include <fcntl.h>
#include <limits.h>
#include <poll.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define MS08_NET_ENVIRONMENT_DEFAULT "qemu-virt-riscv64-smp16-virtio-mmio-user-net"
#define MS08_NET_SNAPSHOT_V5 0x4e494435u
#define MS08_NET_WAKE_CONTROL 0x4e495731u
#define MS08_NET_DIAGNOSTIC_CONTROL 0x4e494331u
#define MS08_NET_FLUSH 0x4e494631u
#define MS08_NET_PEER_PORT 15578u
#define MS08_NET_PEER_ADDR 0x0a000202u /* QEMU user-net host */
#define MS08_NET_WAKE_START 1u
#define MS08_NET_WAKE_TRIGGER 2u
#define MS08_NET_WAKE_CANCEL 3u
#define MS08_NET_DIAG_HOLD_SUBMIT 1u
#define MS08_NET_DIAG_RELEASE 3u /* axnet::diag OP_RELEASE */
#define MS08_NET_DIAG_MAX_LEASE_MS 2000u /* axnet::diag MAX_LEASE_MS */
#define MS08_NET_OWNER_SLOTS 64u
#define MS08_NET_BIDIRECTIONAL_FRAMES 8u
#define MS08_NET_OVERALL_DEADLINE_MS 300000u
#define MS08_NET_PHASE_DEADLINE_MS 30000u
#define MS08_NET_REAP_GRACE_MS 1000u /* SIGKILL-to-reap bound in cleanup */

/* V5 wire: 72-u64 V3 prefix + 15-u64 V4 tuple + 31-u64 V5 tail. */
#define MS08_NET_V5_WIRE_U64 118u
#define MS08_NET_V3_LIFECYCLE 10u
#define MS08_NET_V3_TASK_POLL 15u
#define MS08_NET_V3_RX_SLOT_OCC 28u
#define MS08_NET_V3_TX_SLOT_OCC 34u
#define MS08_NET_V3_TX_SLOT_FULL 36u
#define MS08_NET_V3_TX_SLOT_ENQ 37u
#define MS08_NET_V3_TX_SLOT_DEQ 38u
#define MS08_NET_V3_TX_SUBMIT 40u
#define MS08_NET_V3_TX_AGAIN 41u
#define MS08_NET_V3_TX_COMPLETION 42u
#define MS08_NET_V3_TX_RECLAIM 43u
#define MS08_NET_V3_TX_BUF_AVAIL 44u
#define MS08_NET_V3_TX_BUF_INFLIGHT 45u
#define MS08_NET_V3_TX_DESC_AVAIL 46u
#define MS08_NET_V3_TX_DESC_INFLIGHT 47u
#define MS08_NET_V3_QUEUE_GENERATION 51u
#define MS08_NET_V3_LIVE 54u
#define MS08_NET_V3_QUEUED 55u

#define MS08_NET_LINK_DOWN 0u
#define MS08_NET_LINK_UP 1u

struct ms08_net_v5 {
    uint64_t v3[72];
    uint64_t current_valid;
    uint64_t current_queue_epoch;
    uint64_t current_socket_epoch;
    uint64_t current_link_generation;
    uint64_t current_link_state;
    uint64_t current_owner_available;
    uint64_t current_owner_device_owned;
    uint64_t current_owner_quarantined;
    uint64_t fault_valid;
    uint64_t fault_stage;
    uint64_t fault_cause;
    uint64_t fault_queue_epoch;
    uint64_t fault_owner_available;
    uint64_t fault_owner_device_owned;
    uint64_t fault_owner_quarantined;
    uint64_t configured_harts;
    uint64_t schedulable_mask;
    uint64_t owner_affinity;
    uint64_t runner_affinity;
    uint64_t irq_last_hart;
    uint64_t irq_hart_mask;
    uint64_t irq_events;
    uint64_t owner_last_hart;
    uint64_t owner_hart_mask;
    uint64_t owner_events;
    uint64_t runner_last_hart;
    uint64_t runner_hart_mask;
    uint64_t runner_events;
    uint64_t ipi_sent;
    uint64_t ipi_received;
    uint64_t affinity_rejects;
    uint64_t migration_owner_state;
    uint64_t migration_runner_state;
    uint64_t witness_phase;
    uint64_t witness_completed;
    uint64_t witness_failed;
    uint64_t witness_timer_restored;
    uint64_t witness_start_rejects;
    uint64_t witness_missing_restore;
    uint64_t witness_duplicate_terminal;
    uint64_t witness_trigger_rejects;
    uint64_t witness_illegal_transitions;
    uint64_t witness_last_target_hart;
    uint64_t witness_last_trigger_hart;
    uint64_t witness_target_ipi_before;
    uint64_t witness_target_ipi_after;
};

_Static_assert(sizeof(struct ms08_net_v5) == MS08_NET_V5_WIRE_U64 * sizeof(uint64_t),
               "V5 wire must be a flat 118-u64 frame (V4 is a byte prefix)");

void ms08_net_v5_to_wire(const struct ms08_net_v5 *s, uint64_t *wire)
{
    memcpy(wire, s, sizeof(*s));
}

int ms08_net_v5_from_wire(const uint64_t *wire, struct ms08_net_v5 *s)
{
    /* Fail-closed decode: a snapshot without a valid current tuple is a
     * transient reset sample, never an observation. */
    if (wire[72] != 1) return -1;
    memcpy(s, wire, sizeof(*s));
    return 0;
}

/* ── Case registry (frozen) ─────────────────────────────────────────── */

static const char *const ms08_net_cases[] = {
    "placement", "timer-disabled-wake", "tcp-bidirectional",
    "udp-bidirectional", "full-recovery", "readiness-quiet",
};

/* Per-case marker contract (schema guard compares this with the validator). */
static const char *const ms08_net_schema[] = {
    "placement:MS08_NET_V5",
    "timer-disabled-wake:MS08_NET_V5,MS08_NET_V5,MS08_NET_WAKE",
    "tcp-bidirectional:MS08_NET_V5,MS08_NET_SOCK,MS08_NET_PEER",
    "udp-bidirectional:MS08_NET_V5,MS08_NET_SOCK,MS08_NET_PEER",
    "full-recovery:MS08_NET_V5,MS08_NET_DIAG,MS08_NET_DIAG,MS08_NET_V5,MS08_NET_SOCK,MS08_NET_PEER",
    "readiness-quiet:MS08_NET_V5,MS08_NET_SOCK,MS08_NET_PEER",
};

/* Peer echo cases (strictly per-case increasing sequence, starting at 1). */
static const char *const ms08_net_peer_cases[] = {
    "tcp-bidirectional", "udp-bidirectional", "full-recovery",
    "readiness-quiet",
};

unsigned ms08_net_case_count(void)
{
    return sizeof(ms08_net_cases) / sizeof(ms08_net_cases[0]);
}

unsigned ms08_net_schema_count(void)
{
    return sizeof(ms08_net_schema) / sizeof(ms08_net_schema[0]);
}

static int ms08_net_peer_case_known(const char *case_name)
{
    for (unsigned i = 0; i < sizeof(ms08_net_peer_cases) /
                                sizeof(ms08_net_peer_cases[0]);
         ++i)
        if (strcmp(case_name, ms08_net_peer_cases[i]) == 0) return 1;
    return 0;
}

/* ── Peer frame codec (shared grammar with scripts/ms08-network-peer.py) ── */

int ms08_net_frame_encode(const char *case_name, unsigned long seq, char *buf,
                          size_t cap)
{
    int n;
    if (case_name == NULL || buf == NULL || seq == 0 ||
        !ms08_net_peer_case_known(case_name))
        return -1;
    n = snprintf(buf, cap, "case=%s seq=%lu", case_name, seq);
    if (n < 0 || (size_t)n >= cap) return -1;
    return 0;
}

int ms08_net_frame_decode(const char *line, char *case_name, size_t case_cap,
                          unsigned long *seq)
{
    const char *p;
    const char *sp;
    char *end;
    unsigned long s;
    size_t case_len;
    if (line == NULL || case_name == NULL || seq == NULL) return -1;
    if (strncmp(line, "case=", 5) != 0) return -1;
    p = line + 5;
    sp = strchr(p, ' ');
    if (sp == NULL) return -1;
    case_len = (size_t)(sp - p);
    if (case_len == 0 || case_len + 1 > case_cap) return -1;
    memcpy(case_name, p, case_len);
    case_name[case_len] = '\0';
    if (strncmp(sp, " seq=", 5) != 0) return -1;
    p = sp + 5;
    errno = 0;
    s = strtoul(p, &end, 10);
    if (errno != 0 || end == p || s == 0 || *end != '\0') return -1;
    if (!ms08_net_peer_case_known(case_name)) return -1;
    *seq = s;
    return 0;
}

int ms08_net_probe_self_test(void)
{
    return ms08_net_case_count() == 6 && ms08_net_schema_count() == 6 &&
           strcmp(ms08_net_cases[0], "placement") == 0 &&
           strcmp(ms08_net_cases[5], "readiness-quiet") == 0 &&
           strcmp(ms08_net_schema[4],
                  "full-recovery:MS08_NET_V5,MS08_NET_DIAG,MS08_NET_DIAG,MS08_NET_V5,MS08_NET_SOCK,MS08_NET_PEER") == 0 &&
           MS08_NET_DIAG_RELEASE == 3u &&
           MS08_NET_DIAG_MAX_LEASE_MS == 2000u;
}

/* ── Diagnostic control payloads (5.2-R3) ────────────────────────────── */

int ms08_net_diag_hold_payload(uint64_t payload[2], uint64_t lease_ms)
{
    if (payload == NULL || lease_ms == 0 || lease_ms > MS08_NET_DIAG_MAX_LEASE_MS)
        return -1;
    payload[0] = MS08_NET_DIAG_HOLD_SUBMIT;
    payload[1] = lease_ms;
    return 0;
}

int ms08_net_diag_release_payload(uint64_t payload[2])
{
    if (payload == NULL) return -1;
    payload[0] = MS08_NET_DIAG_RELEASE;
    payload[1] = 0;
    return 0;
}

/* ── Timer-disabled wake baton trace (5.2-R1) ────────────────────────── */

#define MS08_NET_BATON_HOP_TARGET 1u
#define MS08_NET_BATON_HOP_CONTROLLER 2u
#define MS08_NET_BATON_TRIGGER 3u
#define MS08_NET_BATON_CANCEL 4u
#define MS08_NET_BATON_WAIT_ARMED 5u /* forbidden: polling the witness phase */
#define MS08_NET_BATON_RETRY 6u      /* forbidden: any retry loop */

/* The guest appends ops as it executes; only the exact single-thread
 * affinity baton is accepted — no Armed polling, no retry, one TRIGGER. */
int ms08_net_wake_trace_ok(const uint8_t *ops, size_t n)
{
    static const uint8_t baton[] = {
        MS08_NET_BATON_HOP_TARGET, MS08_NET_BATON_HOP_CONTROLLER,
        MS08_NET_BATON_TRIGGER, MS08_NET_BATON_HOP_TARGET,
    };
    return ops != NULL && n == sizeof(baton) && memcmp(ops, baton, n) == 0;
}

/* ── TCP stream reassembly (5.2-R2) ──────────────────────────────────── */

/* Nonblocking TCP connect verdict from the observable interface values: the
 * connection is established only on a synchronous success or on EINPROGRESS
 * followed by a fetched SO_ERROR of zero. */
int ms08_net_tcp_connect_done(int connect_rc, int connect_errno,
                              int so_error_known, int so_error)
{
    if (connect_rc == 0) return 1;
    if (connect_errno == EINPROGRESS && so_error_known && so_error == 0) return 1;
    return 0;
}

#define MS08_NET_STREAM_RX_CAP 128u

struct ms08_net_stream_rx {
    char buf[MS08_NET_STREAM_RX_CAP];
    size_t len;
};

void ms08_net_stream_rx_reset(struct ms08_net_stream_rx *rx)
{
    if (rx != NULL) rx->len = 0;
}

/* Append stream bytes and extract one newline-terminated frame (terminator
 * stripped).  Returns 1 when a frame was extracted, 0 when more bytes are
 * needed, -1 on overflow or malformed arguments.  Pass data == NULL with
 * n == 0 to only attempt an extraction. */
int ms08_net_stream_rx_feed(struct ms08_net_stream_rx *rx, const char *data,
                            size_t n, char *frame, size_t cap)
{
    size_t i;
    if (rx == NULL || frame == NULL || cap == 0) return -1;
    if (data != NULL && n > 0) {
        if (n > MS08_NET_STREAM_RX_CAP - rx->len) return -1;
        memcpy(rx->buf + rx->len, data, n);
        rx->len += n;
    }
    for (i = 0; i < rx->len; ++i)
        if (rx->buf[i] == '\n') break;
    if (i == rx->len) return 0;
    if (i + 1 > cap) return -1;
    memcpy(frame, rx->buf, i);
    frame[i] = '\0';
    memmove(rx->buf, rx->buf + i + 1, rx->len - i - 1);
    rx->len -= i + 1;
    return 1;
}

/* Classify one newline-stripped stream frame against the expected
 * case/seq: 0 = the expected frame, 1 = well-formed but unexpected
 * (duplicate/late/wrong case — rejected as data), -1 = malformed.  Every
 * received byte goes through this check; the runtime path never discards
 * readable socket data unexamined. */
int ms08_net_stream_frame_classify(const char *frame, const char *case_name,
                                   unsigned long seq)
{
    char decoded_case[32];
    unsigned long decoded_seq;
    if (frame == NULL || case_name == NULL) return -1;
    if (ms08_net_frame_decode(frame, decoded_case, sizeof(decoded_case),
                              &decoded_seq) != 0)
        return -1;
    return strcmp(decoded_case, case_name) == 0 && decoded_seq == seq ? 0 : 1;
}

/* Audit the reassembly buffer after the single expected echo frame was
 * accepted (5.2-R2): the peer grammar is exactly one echo frame per
 * request, so any additional complete frame — however well-formed — and any
 * partial tail are unaccounted data and fail closed before the exchange is
 * claimed, including the final exchange. */
int ms08_net_stream_rx_audit(struct ms08_net_stream_rx *rx)
{
    char frame[96];
    int rc;
    if (rx == NULL) return -1;
    for (;;) {
        rc = ms08_net_stream_rx_feed(rx, NULL, 0, frame, sizeof(frame));
        if (rc < 0) return -1;
        if (rc == 0) break;
        return -1; /* a second complete frame is always unexpected data */
    }
    return rx->len == 0 ? 0 : -1; /* a partial tail is unaccounted data */
}

/* ── HMP operator completion line (5.2-R5) ───────────────────────────── */

/* Parse the operator completion line that gates link-up observation.
 * Returns 1 and sets *link_on for a valid "MS08_NET_HMP_DONE link=on|off"
 * line, 0 when the line is not a completion line, -1 for a malformed one. */
int ms08_net_hmp_done_parse(const char *line, int *link_on)
{
    if (line == NULL || link_on == NULL) return -1;
    if (strncmp(line, "MS08_NET_HMP_DONE", 17) != 0) return 0;
    if (strcmp(line, "MS08_NET_HMP_DONE link=on") == 0) {
        *link_on = 1;
        return 1;
    }
    if (strcmp(line, "MS08_NET_HMP_DONE link=off") == 0) {
        *link_on = 0;
        return 1;
    }
    return -1;
}

/* ── Concurrent migration record (5.2-R4) ────────────────────────────── */

/* The migration cases run a separate bounded control caller that holds the
 * synchronous widen/observe/restore ioctl.  Acceptance requires the Widened
 * view to be sampled while that control call is still outstanding, a
 * successful control completion and a closed migration. */
int ms08_net_migration_record_ok(int pre_idle, int have_mid,
                                 int mid_while_outstanding, int have_post,
                                 int control_ok, int ledger_ok)
{
    return pre_idle && have_mid && mid_while_outstanding && have_post &&
           control_ok && ledger_ok;
}

/* ── Snapshot relationship decisions ────────────────────────────────── */

static int ms08_net_hart_schedulable(const struct ms08_net_v5 *s, uint64_t hart)
{
    return hart < 64 && (s->schedulable_mask & ((uint64_t)1 << hart)) != 0;
}

static uint64_t ms08_net_mig_state(const struct ms08_net_v5 *s, int runner)
{
    return runner ? s->migration_runner_state : s->migration_owner_state;
}

static uint64_t ms08_net_role_last(const struct ms08_net_v5 *s, int runner)
{
    return runner ? s->runner_last_hart : s->owner_last_hart;
}

static uint64_t ms08_net_role_aff(const struct ms08_net_v5 *s, int runner)
{
    return runner ? s->runner_affinity : s->owner_affinity;
}

static uint64_t ms08_net_role_events(const struct ms08_net_v5 *s, int runner)
{
    return runner ? s->runner_events : s->owner_events;
}

static uint64_t ms08_net_role_mask(const struct ms08_net_v5 *s, int runner)
{
    return runner ? s->runner_hart_mask : s->owner_hart_mask;
}

static uint8_t ms08_net_mig_phase(uint64_t packed)
{
    return (uint8_t)(packed & 0xff);
}

static uint32_t ms08_net_mig_from(uint64_t packed)
{
    return (uint32_t)((packed >> 8) & 0xffffff);
}

static uint32_t ms08_net_mig_to(uint64_t packed)
{
    return (uint32_t)((packed >> 32) & 0xffffff);
}

/* A cumulative history bit is legal only when the current pin or this
 * role's own migration record explains it: the recorded from/to harts count
 * only while they are members of the published schedulable set.  An in-set
 * but unrecorded hart is rejected (Cycle 002, 6.2-R1). */
static uint64_t ms08_net_explained_hart_bits(const struct ms08_net_v5 *s,
                                             int runner)
{
    uint64_t packed = ms08_net_mig_state(s, runner);
    uint64_t allowed = (uint64_t)1 << ms08_net_role_aff(s, runner);
    uint8_t phase = ms08_net_mig_phase(packed);
    if (phase == 1 || phase == 2) { /* Widened or Restored carries a record */
        uint32_t from = ms08_net_mig_from(packed), to = ms08_net_mig_to(packed);
        if (ms08_net_hart_schedulable(s, from))
            allowed |= (uint64_t)1 << from;
        if (ms08_net_hart_schedulable(s, to))
            allowed |= (uint64_t)1 << to;
    }
    return allowed;
}

int ms08_net_placement_ok(const struct ms08_net_v5 *s)
{
    if (s == NULL || s->configured_harts == 0 || s->schedulable_mask == 0)
        return 0;
    if (s->owner_affinity == s->runner_affinity) return 0;
    if (!ms08_net_hart_schedulable(s, s->owner_affinity) ||
        !ms08_net_hart_schedulable(s, s->runner_affinity))
        return 0;
    /* Fixed placement: the actual observed harts must equal the pins and each
     * role must show attributable progress. */
    if (s->owner_last_hart != s->owner_affinity ||
        s->runner_last_hart != s->runner_affinity)
        return 0;
    if (s->owner_events == 0 || s->runner_events == 0)
        return 0;
    /* The owner/runner masks are cumulative lifetime telemetry, not the
     * current singleton: they must contain the pinned hart, and every
     * recorded bit must be explained by the pin or this role's own recorded
     * migration (from/to, still schedulable). */
    if ((s->owner_hart_mask & ((uint64_t)1 << s->owner_affinity)) == 0 ||
        (s->runner_hart_mask & ((uint64_t)1 << s->runner_affinity)) == 0)
        return 0;
    if ((s->owner_hart_mask & ~ms08_net_explained_hart_bits(s, 0)) != 0 ||
        (s->runner_hart_mask & ~ms08_net_explained_hart_bits(s, 1)) != 0)
        return 0;
    return 1;
}

int ms08_net_wake_ok(const struct ms08_net_v5 *before,
                     const struct ms08_net_v5 *after, uint64_t target)
{
    if (before == NULL || after == NULL) return 0;
    if (after->witness_completed != before->witness_completed + 1) return 0;
    if (after->witness_timer_restored != before->witness_timer_restored + 1)
        return 0;
    if (after->witness_missing_restore != 0) return 0;
    if (after->witness_last_target_hart != target) return 0;
    if (after->witness_last_trigger_hart == target) return 0;
    if (after->witness_last_trigger_hart >= 64) return 0;
    if (!ms08_net_hart_schedulable(after, after->witness_last_trigger_hart))
        return 0;
    /* IPI causality: the accepted remote trigger delivered the target hart's
     * reschedule IPI between arm and resume. */
    return after->witness_target_ipi_after > after->witness_target_ipi_before;
}

int ms08_net_full_observed(const struct ms08_net_v5 *s)
{
    /* Capacity pressure must be proven by counters, not by a marker. */
    return s != NULL &&
           (s->v3[MS08_NET_V3_TX_AGAIN] > 0 || s->v3[MS08_NET_V3_TX_SLOT_FULL] > 0);
}

int ms08_net_full_recovered(const struct ms08_net_v5 *full,
                            const struct ms08_net_v5 *after)
{
    if (full == NULL || after == NULL || !ms08_net_full_observed(full)) return 0;
    /* Nothing left inflight and the slot ledger closes exactly. */
    if (after->v3[MS08_NET_V3_TX_BUF_INFLIGHT] != 0 ||
        after->v3[MS08_NET_V3_TX_DESC_INFLIGHT] != 0 ||
        after->v3[MS08_NET_V3_LIVE] != 0)
        return 0;
    if (after->v3[MS08_NET_V3_TX_SUBMIT] != after->v3[MS08_NET_V3_TX_COMPLETION] ||
        after->v3[MS08_NET_V3_TX_COMPLETION] != after->v3[MS08_NET_V3_TX_RECLAIM])
        return 0;
    if (after->v3[MS08_NET_V3_TX_SLOT_ENQ] -
            after->v3[MS08_NET_V3_TX_SLOT_DEQ] !=
        after->v3[MS08_NET_V3_TX_SLOT_OCC])
        return 0;
    /* The submit path actually made progress past the hold. */
    return after->v3[MS08_NET_V3_TX_SUBMIT] > full->v3[MS08_NET_V3_TX_SUBMIT];
}

int ms08_net_quiet_ok(const struct ms08_net_v5 *a, const struct ms08_net_v5 *b)
{
    return a != NULL && b != NULL &&
           a->v3[MS08_NET_V3_TASK_POLL] == b->v3[MS08_NET_V3_TASK_POLL] &&
           a->irq_events == b->irq_events &&
           a->owner_events == b->owner_events &&
           a->runner_events == b->runner_events &&
           a->v3[MS08_NET_V3_RX_SLOT_OCC] == b->v3[MS08_NET_V3_RX_SLOT_OCC] &&
           a->v3[MS08_NET_V3_TX_SLOT_OCC] == b->v3[MS08_NET_V3_TX_SLOT_OCC];
}

int ms08_net_reset_transition_valid(const struct ms08_net_v5 *before,
                                    const struct ms08_net_v5 *after)
{
    if (before == NULL || after == NULL) return 0;
    if (before->v3[MS08_NET_V3_LIFECYCLE] != 2 ||
        after->v3[MS08_NET_V3_LIFECYCLE] != 2)
        return 0;
    if (before->current_link_state != MS08_NET_LINK_UP ||
        after->current_link_state != MS08_NET_LINK_UP)
        return 0;
    if (before->current_owner_quarantined || after->current_owner_quarantined)
        return 0;
    if (after->current_owner_available != before->current_owner_available)
        return 0;
    return after->current_queue_epoch == before->current_queue_epoch + 1 &&
           after->current_socket_epoch == before->current_socket_epoch + 1 &&
           after->current_link_generation == before->current_link_generation;
}

int ms08_net_link_down_valid(const struct ms08_net_v5 *before,
                             const struct ms08_net_v5 *down)
{
    if (before == NULL || down == NULL) return 0;
    if (before->v3[MS08_NET_V3_LIFECYCLE] != 2 ||
        down->v3[MS08_NET_V3_LIFECYCLE] != 2)
        return 0;
    /* A link flap does not own or release packet slots: both owner channels
     * are conserved across the down transition. */
    if (before->current_owner_available != down->current_owner_available ||
        before->current_owner_device_owned != down->current_owner_device_owned)
        return 0;
    return before->current_link_state == MS08_NET_LINK_UP &&
           down->current_link_state == MS08_NET_LINK_DOWN &&
           down->current_queue_epoch == before->current_queue_epoch &&
           down->current_socket_epoch == before->current_socket_epoch &&
           down->current_link_generation == before->current_link_generation + 1;
}

int ms08_net_link_up_valid(const struct ms08_net_v5 *before,
                           const struct ms08_net_v5 *down,
                           const struct ms08_net_v5 *up)
{
    if (before == NULL || down == NULL || up == NULL) return 0;
    if (up->v3[MS08_NET_V3_LIFECYCLE] != 2) return 0;
    if (up->current_queue_epoch != before->current_queue_epoch ||
        up->current_socket_epoch != before->current_socket_epoch + 1 ||
        up->current_link_generation != down->current_link_generation + 1 ||
        up->current_link_state != MS08_NET_LINK_UP ||
        up->current_owner_quarantined != 0)
        return 0;
    return up->current_owner_available == down->current_owner_available &&
           up->current_owner_device_owned == down->current_owner_device_owned;
}

/* The migration slot is idle (ready for a new controlled migration) when it
 * is None (0, never migrated) or Restored (2, a boot-smoke migration finished
 * and returned to the singleton pin); only Widened (1) marks an in-flight
 * migration that must not be re-entered (Iteration 005, 6.2). */
int ms08_net_mig_idle(uint64_t packed)
{
    uint8_t phase = ms08_net_mig_phase(packed);
    return phase == 0 || phase == 2;
}

static int ms08_net_ticket_ledger_ok(const struct ms08_net_v5 *s)
{
    return s->v3[MS08_NET_V3_LIVE] == 0 &&
           s->v3[MS08_NET_V3_TX_SUBMIT] == s->v3[MS08_NET_V3_TX_COMPLETION] &&
           s->v3[MS08_NET_V3_TX_COMPLETION] == s->v3[MS08_NET_V3_TX_RECLAIM];
}

int ms08_net_migration_widened(const struct ms08_net_v5 *pre,
                               const struct ms08_net_v5 *mid, int runner)
{
    uint64_t packed;
    uint32_t from, to;
    if (pre == NULL || mid == NULL) return 0;
    packed = ms08_net_mig_state(mid, runner);
    if (!ms08_net_mig_idle(ms08_net_mig_state(pre, runner)) ||
        ms08_net_mig_phase(packed) != 1)
        return 0;
    from = ms08_net_mig_from(packed);
    to = ms08_net_mig_to(packed);
    if (from != ms08_net_role_aff(pre, runner) || from == to) return 0;
    if (!ms08_net_hart_schedulable(mid, to)) return 0;
    /* The migrated role must actually be observed polling on the second hart
     * with progress, the other role must stay pinned, and no ticket may leak
     * mid-migration. */
    if (ms08_net_role_last(mid, runner) != to) return 0;
    if (ms08_net_role_last(mid, !runner) != ms08_net_role_last(pre, !runner))
        return 0;
    if (ms08_net_role_events(mid, runner) <= ms08_net_role_events(pre, runner))
        return 0;
    return ms08_net_ticket_ledger_ok(mid);
}

int ms08_net_migration_restored(const struct ms08_net_v5 *mid,
                                const struct ms08_net_v5 *post, int runner)
{
    uint64_t packed_mid, packed_post;
    if (mid == NULL || post == NULL) return 0;
    packed_mid = ms08_net_mig_state(mid, runner);
    packed_post = ms08_net_mig_state(post, runner);
    if (ms08_net_mig_phase(packed_mid) != 1 ||
        ms08_net_mig_phase(packed_post) != 2)
        return 0;
    if (ms08_net_mig_from(packed_mid) != ms08_net_mig_from(packed_post) ||
        ms08_net_mig_to(packed_mid) != ms08_net_mig_to(packed_post))
        return 0;
    if (ms08_net_role_aff(post, runner) != ms08_net_mig_from(packed_mid))
        return 0;
    if (ms08_net_role_last(post, runner) != ms08_net_mig_from(packed_mid))
        return 0;
    if (ms08_net_role_events(post, runner) <= ms08_net_role_events(mid, runner))
        return 0;
    return ms08_net_ticket_ledger_ok(post);
}

/* Post-completion validity without a sampled Widened mid-state: the control
 * caller completed (the caller gates on its success before this relation),
 * the slot is Restored, the announced migration identifies a distinct
 * schedulable target recorded in the role's cumulative history, the role is
 * back on its pinned hart with attributable progress, the other role holds
 * its pin, and the ticket ledger stays closed.  When the probe happened to
 * sample the Widened phase it still runs the full widened/restored
 * relations; this predicate covers the fast-migration window where a
 * run-time migration completes between the probe's snapshot reads
 * (Iteration 005, 6.2; Cycle 002, 6.2-R1). */
int ms08_net_migration_completed(const struct ms08_net_v5 *pre,
                                 const struct ms08_net_v5 *post, int runner)
{
    uint64_t packed_post;
    uint32_t from, to;
    if (pre == NULL || post == NULL) return 0;
    packed_post = ms08_net_mig_state(post, runner);
    if (ms08_net_mig_phase(packed_post) != 2) return 0; /* must be Restored */
    from = (uint32_t)ms08_net_role_aff(pre, runner);
    if (ms08_net_mig_from(packed_post) != from) return 0;
    /* The reported target must be a distinct, schedulable hart recorded in
     * the role's cumulative history — aggregate growth alone is not a
     * migration. */
    to = ms08_net_mig_to(packed_post);
    if (to == from || !ms08_net_hart_schedulable(post, to)) return 0;
    if ((ms08_net_role_mask(post, runner) & ((uint64_t)1 << to)) == 0)
        return 0;
    if (ms08_net_role_last(post, runner) != from) return 0;
    if (ms08_net_role_events(post, runner) <= ms08_net_role_events(pre, runner))
        return 0;
    if (ms08_net_role_last(post, !runner) != ms08_net_role_last(pre, !runner) ||
        ms08_net_role_aff(post, !runner) != ms08_net_role_aff(pre, !runner))
        return 0;
    if (!ms08_net_ticket_ledger_ok(post)) return 0;
    return 1;
}

/* Time/deadline and bounded-fd-wait helpers, plus the control-caller
 * cleanup, live outside the testing guard so the host harness can witness
 * them against real pipes and processes. */

static int now_ms(uint64_t *out)
{
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) return -1;
    *out = (uint64_t)ts.tv_sec * 1000u + (uint64_t)ts.tv_nsec / 1000000u;
    return 0;
}

/* Bounded fd wait (poll only; no sleep calls anywhere in this probe). */
static int wait_fd(int fd, short events, uint64_t deadline)
{
    struct pollfd pfd = { .fd = fd, .events = events };
    uint64_t now;
    for (;;) {
        int remaining;
        if (now_ms(&now) != 0 || now >= deadline) return -1;
        remaining = (int)((deadline - now) > 1000u ? 1000u : deadline - now);
        pfd.revents = 0;
        if (poll(&pfd, 1, remaining) < 0 && errno != EINTR) return -1;
        if (now_ms(&now) != 0) return -1;
        if (pfd.revents & events) return now < deadline ? 0 : -1;
        if (pfd.revents & (POLLERR | POLLNVAL)) return -1;
    }
}

#ifdef MS08_NET_PROBE_TESTING
/* Terminate a control caller that is still alive when the case has failed
 * or the deadline passed: a child parked inside the migration ioctl must
 * not outlive the case holding the inherited TTY and pipe write end
 * (5.2-R4 terminal cleanup).  SIGKILL plus a bounded reap inside a small
 * grace window; the parent never blocks unboundedly. */
static int terminate_control_caller(pid_t child)
{
    uint64_t grace, now;
    if (kill(child, SIGKILL) != 0 && errno != ESRCH) return -1;
    if (now_ms(&grace) != 0) return -1;
    grace += MS08_NET_REAP_GRACE_MS;
    for (;;) {
        pid_t rc = waitpid(child, NULL, WNOHANG);
        if (rc == child) return 0;
        if (rc < 0 && errno == ECHILD) return 0; /* already reaped */
        if (now_ms(&now) != 0 || now >= grace) return -1;
    }
}

/* Bounded control-caller cleanup: drain the completion pipe to EOF (the
 * child's write end closes only at exit), then reap.  Returns 0 when the
 * child was reaped; -1 when the deadline passed first or a helper failed —
 * in both cases a still-alive control caller is terminated and reaped
 * within the grace window, and the original failure reason stands. */
static int reap_control_caller(pid_t child, int pipe_rd, uint64_t deadline)
{
    char buf[16];
    /* The completion pipe is created blocking and the control caller may
     * still hold its write end inside the migration ioctl: make our end
     * nonblocking so no read can bypass the deadline (5.2-R4 bounded
     * cleanup). */
    int flags = fcntl(pipe_rd, F_GETFL);
    if (flags < 0 || fcntl(pipe_rd, F_SETFL, flags | O_NONBLOCK) != 0) {
        close(pipe_rd);
        (void)terminate_control_caller(child);
        return -1;
    }
    for (;;) {
        ssize_t n = read(pipe_rd, buf, sizeof(buf));
        if (n == 0) break; /* EOF: the child exited */
        if (n > 0) continue;
        if (errno == EAGAIN || errno == EWOULDBLOCK || errno == EINTR) {
            uint64_t now;
            if (now_ms(&now) != 0 || now >= deadline) {
                close(pipe_rd);
                (void)terminate_control_caller(child);
                return -1;
            }
            /* POLLHUP is the EOF event for a pipe whose writer exited: it
             * must wake the drain too, not spin until the deadline. */
            if (wait_fd(pipe_rd, POLLIN | POLLHUP, deadline) != 0) {
                close(pipe_rd);
                (void)terminate_control_caller(child);
                return -1;
            }
            continue;
        }
        close(pipe_rd);
        (void)terminate_control_caller(child);
        return -1;
    }
    close(pipe_rd);
    for (;;) {
        uint64_t now;
        if (waitpid(child, NULL, WNOHANG) == child) return 0;
        if (now_ms(&now) != 0 || now >= deadline) return -1;
    }
}
#endif /* MS08_NET_PROBE_TESTING (host harness cleanup helpers) */

/* A host-testable success predicate for the affinity baton helpers: any
 * non-negative return is success (sched_setaffinity -> 0, sched_getaffinity
 * -> mask size in bytes), only a negative return is a failure.  This was the
 * cause of the `affinity-read` FAIL: a successful get (returns > 0) was being
 * classified as an error (Iteration 005, 6.2). */
int ms08_net_syscall_return_ok(long rc)
{
    return rc >= 0;
}

#ifndef MS08_NET_PROBE_TESTING

static int fail_case(const char *case_name, const char *reason)
{
    printf("FAIL: %s reason=%s\n", case_name, reason);
    fflush(stdout);
    return 1;
}

/* Forward declaration: defined below after the V5 wire reader. */
static int read_v5(struct ms08_net_v5 *out);

/* Read one V5 snapshot after an observable event; a transiently invalid
 * current tuple fails closed instead of busy retrying.  Returns 0 on a
 * valid sample, -1 otherwise. */
static int read_v5_once(struct ms08_net_v5 *out)
{
    return read_v5(out) == 0 ? 0 : -1;
}

/* One bounded wait that must end in silence: any readiness event is
 * spontaneous progress and fails the case.  Returns 0 on timed-out silence. */
static int expect_fd_silence(int fd, short events, uint64_t window_ms)
{
    struct pollfd pfd = { .fd = fd, .events = events };
    uint64_t now, end;
    int remaining;
    if (now_ms(&now) != 0) return -1;
    end = now + window_ms;
    remaining = (int)(end - now);
    pfd.revents = 0;
    if (poll(&pfd, 1, remaining) < 0 && errno != EINTR) return -1;
    if (pfd.revents & (events | POLLERR | POLLHUP | POLLNVAL)) return -1;
    return 0;
}

/* ── Affinity baton primitives (5.2-R1) ─────────────────────────────── */

static int get_affinity_mask(uint64_t *mask)
{
    unsigned char bytes[8] = { 0 };
    uint64_t value = 0;
    long rc;
    if (mask == NULL) return -1;
    rc = syscall(SYS_sched_getaffinity, 0, sizeof(bytes), bytes);
    if (!ms08_net_syscall_return_ok(rc)) return -1;
    for (unsigned i = 0; i < 8; ++i) value |= (uint64_t)bytes[i] << (8 * i);
    *mask = value;
    return 0;
}

static int set_affinity_mask(uint64_t mask)
{
    unsigned char bytes[8];
    long rc;
    for (unsigned i = 0; i < 8; ++i)
        bytes[i] = (unsigned char)(mask >> (8 * i));
    rc = syscall(SYS_sched_setaffinity, 0, sizeof(bytes), bytes);
    return ms08_net_syscall_return_ok(rc) ? 0 : -1;
}

static int pick_two_harts(const struct ms08_net_v5 *s, uint64_t *first,
                          uint64_t *second)
{
    uint64_t a = 64, b = 64, h;
    for (h = 0; h < 64; ++h)
        if (ms08_net_hart_schedulable(s, h)) {
            a = h;
            break;
        }
    for (h = a + 1; h < 64; ++h)
        if (ms08_net_hart_schedulable(s, h)) {
            b = h;
            break;
        }
    if (a >= 64 || b >= 64) return -1;
    *first = a;
    *second = b;
    return 0;
}

static int make_deadline(uint64_t overall_deadline, uint64_t budget_ms,
                         uint64_t *deadline)
{
    uint64_t now, candidate;
    if (deadline == NULL || now_ms(&now) != 0 || now >= overall_deadline)
        return -1;
    candidate = budget_ms > UINT64_MAX - now ? UINT64_MAX : now + budget_ms;
    *deadline = candidate < overall_deadline ? candidate : overall_deadline;
    return *deadline > now ? 0 : -1;
}

/* Read a V5 snapshot; returns 0 on a valid (current tuple present) sample,
 * 1 when the sample is transiently invalid (e.g. mid-reset), -1 on ioctl
 * failure. */
static int read_v5(struct ms08_net_v5 *out)
{
    uint64_t wire[MS08_NET_V5_WIRE_U64];
    memset(wire, 0, sizeof(wire));
    if (ioctl(STDIN_FILENO, MS08_NET_SNAPSHOT_V5, wire) < 0) return -1;
    if (ms08_net_v5_from_wire(wire, out) != 0) return 1;
    return 0;
}

static void print_v5(const char *case_name, const struct ms08_net_v5 *s)
{
    printf("MS08_NET_V5: case=%s configured=%llu schedulable=0x%llx "
           "owner_aff=%llu runner_aff=%llu irq_last=%llu irq_mask=0x%llx "
           "irq_events=%llu owner_last=%llu owner_mask=0x%llx owner_events=%llu "
           "runner_last=%llu runner_mask=0x%llx runner_events=%llu "
           "ipi_sent=%llu ipi_received=%llu rejects=%llu own_mig=0x%llx "
           "run_mig=0x%llx wphase=%llu wcompleted=%llu wfailed=%llu "
           "wtimer_restored=%llu wstart_rejects=%llu wmissing_restore=%llu "
           "wtrigger_rejects=%llu willegal=%llu wlast_target=%llu "
           "wlast_trigger=%llu wipi_before=%llu wipi_after=%llu lifecycle=%llu "
           "q=%llu s=%llu l=%llu link=%s available=%llu device_owned=%llu "
           "quarantined=%llu fault_valid=%llu fault_stage=%llu rx_occ=%llu "
           "tx_occ=%llu tx_enq=%llu tx_deq=%llu tx_submit=%llu tx_again=%llu "
           "tx_completion=%llu tx_reclaim=%llu tx_buf_avail=%llu "
           "tx_buf_inflight=%llu tx_desc_avail=%llu tx_desc_inflight=%llu "
           "live=%llu task_poll=%llu queue_generation=%llu\n",
           case_name,
           (unsigned long long)s->configured_harts,
           (unsigned long long)s->schedulable_mask,
           (unsigned long long)s->owner_affinity,
           (unsigned long long)s->runner_affinity,
           (unsigned long long)s->irq_last_hart,
           (unsigned long long)s->irq_hart_mask,
           (unsigned long long)s->irq_events,
           (unsigned long long)s->owner_last_hart,
           (unsigned long long)s->owner_hart_mask,
           (unsigned long long)s->owner_events,
           (unsigned long long)s->runner_last_hart,
           (unsigned long long)s->runner_hart_mask,
           (unsigned long long)s->runner_events,
           (unsigned long long)s->ipi_sent,
           (unsigned long long)s->ipi_received,
           (unsigned long long)s->affinity_rejects,
           (unsigned long long)s->migration_owner_state,
           (unsigned long long)s->migration_runner_state,
           (unsigned long long)s->witness_phase,
           (unsigned long long)s->witness_completed,
           (unsigned long long)s->witness_failed,
           (unsigned long long)s->witness_timer_restored,
           (unsigned long long)s->witness_start_rejects,
           (unsigned long long)s->witness_missing_restore,
           (unsigned long long)s->witness_trigger_rejects,
           (unsigned long long)s->witness_illegal_transitions,
           (unsigned long long)s->witness_last_target_hart,
           (unsigned long long)s->witness_last_trigger_hart,
           (unsigned long long)s->witness_target_ipi_before,
           (unsigned long long)s->witness_target_ipi_after,
           (unsigned long long)s->v3[MS08_NET_V3_LIFECYCLE],
           (unsigned long long)s->current_queue_epoch,
           (unsigned long long)s->current_socket_epoch,
           (unsigned long long)s->current_link_generation,
           s->current_link_state == MS08_NET_LINK_UP ? "up" : "down",
           (unsigned long long)s->current_owner_available,
           (unsigned long long)s->current_owner_device_owned,
           (unsigned long long)s->current_owner_quarantined,
           (unsigned long long)s->fault_valid,
           (unsigned long long)s->fault_stage,
           (unsigned long long)s->v3[MS08_NET_V3_RX_SLOT_OCC],
           (unsigned long long)s->v3[MS08_NET_V3_TX_SLOT_OCC],
           (unsigned long long)s->v3[MS08_NET_V3_TX_SLOT_ENQ],
           (unsigned long long)s->v3[MS08_NET_V3_TX_SLOT_DEQ],
           (unsigned long long)s->v3[MS08_NET_V3_TX_SUBMIT],
           (unsigned long long)s->v3[MS08_NET_V3_TX_AGAIN],
           (unsigned long long)s->v3[MS08_NET_V3_TX_COMPLETION],
           (unsigned long long)s->v3[MS08_NET_V3_TX_RECLAIM],
           (unsigned long long)s->v3[MS08_NET_V3_TX_BUF_AVAIL],
           (unsigned long long)s->v3[MS08_NET_V3_TX_BUF_INFLIGHT],
           (unsigned long long)s->v3[MS08_NET_V3_TX_DESC_AVAIL],
           (unsigned long long)s->v3[MS08_NET_V3_TX_DESC_INFLIGHT],
           (unsigned long long)s->v3[MS08_NET_V3_LIVE],
           (unsigned long long)s->v3[MS08_NET_V3_TASK_POLL],
           (unsigned long long)s->v3[MS08_NET_V3_QUEUE_GENERATION]);
    fflush(stdout);
}

/* ── Peer exchange helpers ──────────────────────────────────────────── */

static int open_peer_socket(int socktype, const char *case_name, uint64_t deadline)
{
    int fd = socket(AF_INET, socktype | SOCK_NONBLOCK, 0);
    struct sockaddr_in peer;
    if (fd < 0) return -1;
    memset(&peer, 0, sizeof(peer));
    peer.sin_family = AF_INET;
    peer.sin_port = htons(MS08_NET_PEER_PORT);
    peer.sin_addr.s_addr = htonl(MS08_NET_PEER_ADDR);
    if (connect(fd, (const struct sockaddr *)&peer, sizeof(peer)) != 0) {
        if (socktype == SOCK_STREAM && errno == EINPROGRESS) {
            /* A normal nonblocking TCP connect: wait for writable readiness
             * and let SO_ERROR decide (5.2-R2). */
            int so_error = 0;
            socklen_t opt_len = sizeof(so_error);
            int so_error_known =
                wait_fd(fd, POLLOUT, deadline) == 0 &&
                getsockopt(fd, SOL_SOCKET, SO_ERROR, &so_error, &opt_len) == 0;
            if (!ms08_net_tcp_connect_done(-1, EINPROGRESS, so_error_known,
                                           so_error)) {
                close(fd);
                return -1;
            }
        } else {
            close(fd);
            return -1;
        }
    }
    (void)case_name;
    return fd;
}

/* Send one frame and read its exact echo back (bounded, nonblocking).
 * ``rx`` carries the externally-owned TCP reassembly buffer; pass NULL for
 * UDP, where each datagram is one complete frame. */
static int peer_roundtrip(int fd, int socktype, const char *case_name,
                          unsigned long seq, uint64_t deadline,
                          struct ms08_net_stream_rx *rx)
{
    char frame[96];
    char wire[100];
    size_t frame_len = 0;
    struct ms08_net_stream_rx local_rx;
    if (ms08_net_frame_encode(case_name, seq, frame, sizeof(frame)) != 0)
        return -1;
    if (rx == NULL) {
        ms08_net_stream_rx_reset(&local_rx);
        rx = &local_rx; /* unused for UDP, keeps the send path uniform */
    }
    /* TCP frames are newline-delimited in the byte stream; UDP datagrams
     * keep the bare grammar. */
    if (socktype == SOCK_STREAM) {
        if (snprintf(wire, sizeof(wire), "%s\n", frame) < 0) return -1;
    } else {
        if (snprintf(wire, sizeof(wire), "%s", frame) < 0) return -1;
    }
    frame_len = strlen(wire);
    for (size_t off = 0; off < frame_len;) {
        ssize_t n;
        uint64_t now;
        if (wait_fd(fd, POLLOUT, deadline) != 0) return -1;
        if (now_ms(&now) != 0 || now >= deadline) return -1;
        n = send(fd, wire + off, frame_len - off, MSG_DONTWAIT);
        if (n > 0) {
            off += (size_t)n;
            continue;
        }
        if (n < 0 && (errno == EAGAIN || errno == EINTR || errno == EWOULDBLOCK))
            continue;
        return -1;
    }
    if (socktype == SOCK_DGRAM) {
        ssize_t n;
        uint64_t now;
        char reply[96];
        if (wait_fd(fd, POLLIN, deadline) != 0) return -1;
        if (now_ms(&now) != 0 || now >= deadline) return -1;
        n = recv(fd, reply, sizeof(reply) - 1, MSG_DONTWAIT);
        if (n <= 0) return -1;
        reply[n] = '\0';
        if (strchr(reply, '\n') != NULL) *strchr(reply, '\n') = '\0';
        /* Any readable datagram must be the expected frame: duplicates,
         * late frames and malformed text all fail closed. */
        if (ms08_net_stream_frame_classify(reply, case_name, seq) != 0)
            return -1;
        return 0;
    }
    /* TCP: the echo is a newline-terminated frame inside a byte stream; it
     * may be split across segments or coalesced with the next frame. */
    for (;;) {
        char chunk[96];
        char stream_frame[96];
        ssize_t n;
        uint64_t now;
        int feed_rc;
        if (wait_fd(fd, POLLIN, deadline) != 0) return -1;
        if (now_ms(&now) != 0 || now >= deadline) return -1;
        n = recv(fd, chunk, sizeof(chunk) - 1, MSG_DONTWAIT);
        if (n < 0 && (errno == EAGAIN || errno == EINTR || errno == EWOULDBLOCK))
            continue;
        if (n <= 0) return -1;
        chunk[n] = '\0';
        feed_rc = ms08_net_stream_rx_feed(rx, chunk, (size_t)n, stream_frame,
                                          sizeof(stream_frame));
        if (feed_rc < 0) return -1;
        while (feed_rc == 1) {
            /* Every extracted frame is classified: only the exact expected
             * frame is accepted; duplicates/late frames and malformed text
             * fail closed instead of being discarded. */
            if (ms08_net_stream_frame_classify(stream_frame, case_name,
                                               seq) != 0)
                return -1;
            /* One echo per request: audit every remaining complete frame
             * and any partial tail before claiming this exchange. */
            if (ms08_net_stream_rx_audit(rx) != 0) return -1;
            return 0;
        }
    }
}

/* ── Case implementations ───────────────────────────────────────────── */

static int run_placement(uint64_t deadline)
{
    struct ms08_net_v5 s;
    int rc;
    (void)deadline;
    rc = read_v5_once(&s);
    if (rc != 0) return fail_case("placement", "snapshot");
    print_v5("placement", &s);
    if (!ms08_net_placement_ok(&s)) return fail_case("placement", "policy");
    printf("PASS: placement\n");
    fflush(stdout);
    return 0;
}

static int run_timer_disabled_wake(uint64_t deadline)
{
    const char *case_name = "timer-disabled-wake";
    struct ms08_net_v5 before, after;
    uint64_t payload[2];
    uint64_t orig_mask = 0, controller, target;
    uint8_t trace[8];
    size_t trace_len = 0;
    int rc, started = 0, status = 1;
    const char *reason = "snapshot";

    (void)deadline;
    rc = read_v5_once(&before);
    if (rc != 0) return fail_case(case_name, rc < 0 ? "snapshot" : "deadline");
    print_v5(case_name, &before);
    if (pick_two_harts(&before, &controller, &target) != 0)
        return fail_case(case_name, "no-hart");
    if (get_affinity_mask(&orig_mask) != 0)
        return fail_case(case_name, "affinity-read");

    /* Single-thread affinity baton (5.2-R1): pin the caller to a controller
     * hart, START the witness on a different target hart, hop onto the
     * target run queue (the caller resumes only after the target reached its
     * timer-disabled Blocked park, because it was enqueued ahead and parks
     * without yielding), hop back so the one TRIGGER is issued from a
     * non-target hart, then hop onto the target again so the terminal V5
     * read happens after the woken target completed.  No Armed polling, no
     * retry loop, no child process. */
    if (set_affinity_mask((uint64_t)1 << controller) != 0) {
        reason = "pin-controller";
        goto out;
    }
    payload[0] = MS08_NET_WAKE_START;
    payload[1] = target;
    if (ioctl(STDIN_FILENO, MS08_NET_WAKE_CONTROL, payload) < 0) {
        reason = "start";
        goto out;
    }
    started = 1;
    trace[trace_len++] = MS08_NET_BATON_HOP_TARGET;
    if (set_affinity_mask((uint64_t)1 << target) != 0) {
        reason = "hop-target";
        goto out;
    }
    trace[trace_len++] = MS08_NET_BATON_HOP_CONTROLLER;
    if (set_affinity_mask((uint64_t)1 << controller) != 0) {
        reason = "hop-controller";
        goto out;
    }
    trace[trace_len++] = MS08_NET_BATON_TRIGGER;
    payload[0] = MS08_NET_WAKE_TRIGGER;
    payload[1] = 0;
    if (ioctl(STDIN_FILENO, MS08_NET_WAKE_CONTROL, payload) < 0) {
        reason = "trigger";
        goto out;
    }
    trace[trace_len++] = MS08_NET_BATON_HOP_TARGET;
    if (set_affinity_mask((uint64_t)1 << target) != 0) {
        reason = "hop-terminal";
        goto out;
    }
    if (!ms08_net_wake_trace_ok(trace, trace_len)) {
        reason = "baton";
        goto out;
    }
    rc = read_v5_once(&after);
    if (rc != 0) {
        reason = "snapshot";
        goto out;
    }
    print_v5(case_name, &after);
    if (!ms08_net_wake_ok(&before, &after, target)) {
        reason = "relation";
        goto out;
    }
    status = 0;
out:
    if (status && started) {
        /* Supervised cancel of the unfinished run, then the same target-hop
         * ordering gives the cancelled target its turn so timer restoration
         * is observed before anything else runs. */
        payload[0] = MS08_NET_WAKE_CANCEL;
        payload[1] = 0;
        if (ioctl(STDIN_FILENO, MS08_NET_WAKE_CONTROL, payload) == 0) {
            struct ms08_net_v5 cancelled;
            (void)set_affinity_mask((uint64_t)1 << target);
            if (read_v5_once(&cancelled) == 0 &&
                cancelled.witness_timer_restored <= before.witness_timer_restored)
                reason = "timer-not-restored";
        }
    }
    (void)set_affinity_mask(orig_mask);
    if (status) return fail_case(case_name, reason);
    printf("MS08_NET_WAKE: case=timer-disabled-wake target=%llu trigger=%llu "
           "ipi_before=%llu ipi_after=%llu completed=%llu timer_restored=%llu "
           "missing_restore=%llu\n",
           (unsigned long long)after.witness_last_target_hart,
           (unsigned long long)after.witness_last_trigger_hart,
           (unsigned long long)after.witness_target_ipi_before,
           (unsigned long long)after.witness_target_ipi_after,
           (unsigned long long)after.witness_completed,
           (unsigned long long)after.witness_timer_restored,
           (unsigned long long)after.witness_missing_restore);
    printf("PASS: timer-disabled-wake\n");
    fflush(stdout);
    return 0;
}

static int run_bidirectional(const char *case_name, int socktype, uint64_t deadline)
{
    struct ms08_net_v5 before, after;
    struct ms08_net_stream_rx rx;
    int fd, rc;
    unsigned long seq;
    ms08_net_stream_rx_reset(&rx);
    rc = read_v5_once(&before);
    if (rc != 0) return fail_case(case_name, "snapshot-before");
    fd = open_peer_socket(socktype, case_name, deadline);
    if (fd < 0) return fail_case(case_name, "socket");
    for (seq = 1; seq <= MS08_NET_BIDIRECTIONAL_FRAMES; ++seq)
        if (peer_roundtrip(fd, socktype, case_name, seq, deadline,
                           socktype == SOCK_STREAM ? &rx : NULL) != 0) {
            close(fd);
            return fail_case(case_name, "roundtrip");
        }
    close(fd);
    rc = read_v5_once(&after);
    if (rc != 0) return fail_case(case_name, "snapshot-after");
    print_v5(case_name, &after);
    if (after.v3[MS08_NET_V3_TX_SUBMIT] <= before.v3[MS08_NET_V3_TX_SUBMIT])
        return fail_case(case_name, "no-tx-progress");
    printf("MS08_NET_SOCK: case=%s result=rw sent=%lu received=%lu\n", case_name,
           (unsigned long)MS08_NET_BIDIRECTIONAL_FRAMES,
           (unsigned long)MS08_NET_BIDIRECTIONAL_FRAMES);
    printf("MS08_NET_PEER: case=%s result=ok\n", case_name);
    printf("PASS: %s\n", case_name);
    fflush(stdout);
    return 0;
}

static int run_full_recovery(uint64_t overall_deadline)
{
    const char *case_name = "full-recovery";
    uint64_t payload[2];
    uint64_t hold_deadline, drain_deadline;
    struct ms08_net_v5 full, after;
    int fd, rc, full_seen = 0;
    unsigned long seq = 1;
    unsigned long echo_next = 1; /* guest-side echo conservation ledger */
    unsigned long received = 0;
    const char *reason = NULL;

    if (make_deadline(overall_deadline, MS08_NET_PHASE_DEADLINE_MS,
                      &hold_deadline) != 0)
        return fail_case(case_name, "deadline");
    fd = open_peer_socket(SOCK_DGRAM, case_name, hold_deadline);
    if (fd < 0) return fail_case(case_name, "socket");
    if (ms08_net_diag_hold_payload(payload, MS08_NET_DIAG_MAX_LEASE_MS) != 0) {
        close(fd);
        return fail_case(case_name, "hold-args");
    }
    if (ioctl(STDIN_FILENO, MS08_NET_DIAGNOSTIC_CONTROL, payload) < 0) {
        close(fd);
        return fail_case(case_name, "hold");
    }
    /* Burst under the bounded lease until the driver reports real capacity
     * pressure.  Send pacing and inbound echo draining are both driven by
     * socket readiness — no sleep polling. */
    while (!full_seen) {
        char frame[96];
        ssize_t n;
        struct pollfd pfd = { .fd = fd, .events = POLLIN | POLLOUT };
        uint64_t now;
        int poll_rc;
        if (now_ms(&now) != 0 || now >= hold_deadline) {
            reason = "hold-deadline";
            goto release_fail;
        }
        pfd.revents = 0;
        poll_rc = poll(&pfd, 1,
                       (int)((hold_deadline - now) > 1000u ? 1000u
                                                           : hold_deadline - now));
        if (poll_rc < 0 && errno != EINTR) {
            reason = "poll";
            goto release_fail;
        }
        if (pfd.revents & POLLIN) {
            for (;;) {
                char echo[96];
                char echo_case[32];
                unsigned long echo_seq;
                ssize_t en = recv(fd, echo, sizeof(echo) - 1, MSG_DONTWAIT);
                if (en < 0 && (errno == EAGAIN || errno == EWOULDBLOCK ||
                               errno == EINTR))
                    break;
                if (en <= 0) {
                    reason = "echo-recv";
                    goto release_fail;
                }
                echo[en] = '\0';
                if (strchr(echo, '\n') != NULL) *strchr(echo, '\n') = '\0';
                if (ms08_net_frame_decode(echo, echo_case, sizeof(echo_case),
                                          &echo_seq) != 0 ||
                    strcmp(echo_case, case_name) != 0 || echo_seq != echo_next) {
                    reason = "echo-ledger";
                    goto release_fail;
                }
                ++echo_next;
                ++received;
            }
        }
        if (pfd.revents & POLLOUT) {
            if (ms08_net_frame_encode(case_name, seq, frame, sizeof(frame)) != 0) {
                reason = "frame";
                goto release_fail;
            }
            n = send(fd, frame, strlen(frame), MSG_DONTWAIT);
            if (n == (ssize_t)strlen(frame)) ++seq;
            else if (n < 0 && errno != EAGAIN && errno != EINTR &&
                     errno != EWOULDBLOCK) {
                reason = "send";
                goto release_fail;
            }
        }
        rc = read_v5(&full);
        if (rc < 0) {
            reason = "snapshot-poll";
            goto release_fail;
        }
        if (rc == 0 && ms08_net_full_observed(&full)) full_seen = 1;
    }
    print_v5(case_name, &full);
    if (ms08_net_diag_release_payload(payload) != 0 ||
        ioctl(STDIN_FILENO, MS08_NET_DIAGNOSTIC_CONTROL, payload) < 0) {
        close(fd);
        return fail_case(case_name, "release");
    }
    printf("MS08_NET_DIAG: case=full-recovery op=hold-submit result=ok\n");
    printf("MS08_NET_DIAG: case=full-recovery op=release result=ok\n");
    if (make_deadline(overall_deadline, MS08_NET_PHASE_DEADLINE_MS,
                      &drain_deadline) != 0) {
        close(fd);
        return fail_case(case_name, "deadline");
    }
    /* Every burst echo must still arrive exactly once and in order. */
    for (;;) {
        char echo[96];
        char echo_case[32];
        unsigned long echo_seq;
        ssize_t en;
        struct pollfd pfd = { .fd = fd, .events = POLLIN };
        uint64_t now;
        if (now_ms(&now) != 0 || now >= drain_deadline) {
            close(fd);
            return fail_case(case_name, "echo-drain-deadline");
        }
        pfd.revents = 0;
        if (poll(&pfd, 1, (int)((drain_deadline - now) > 1000u ? 1000u
                                                              : drain_deadline - now)) <= 0)
            continue;
        en = recv(fd, echo, sizeof(echo) - 1, MSG_DONTWAIT);
        if (en < 0 && (errno == EAGAIN || errno == EWOULDBLOCK || errno == EINTR))
            continue;
        if (en <= 0) {
            close(fd);
            return fail_case(case_name, "echo-recv");
        }
        echo[en] = '\0';
        if (strchr(echo, '\n') != NULL) *strchr(echo, '\n') = '\0';
        if (ms08_net_frame_decode(echo, echo_case, sizeof(echo_case),
                                  &echo_seq) != 0 ||
            strcmp(echo_case, case_name) != 0 || echo_seq != echo_next) {
            close(fd);
            return fail_case(case_name, "echo-ledger");
        }
        ++echo_next;
        ++received;
        if (echo_next == seq) break; /* all burst echoes accounted for */
    }
    /* Drain: finish the burst with the peer and flush the C4 queue. */
    while (seq <= MS08_NET_BIDIRECTIONAL_FRAMES * 4u) {
        if (peer_roundtrip(fd, SOCK_DGRAM, case_name, seq++, drain_deadline,
                           NULL) != 0) {
            close(fd);
            return fail_case(case_name, "drain");
        }
        ++received;
    }
    close(fd);
    if (ioctl(STDIN_FILENO, MS08_NET_FLUSH, 0) < 0)
        return fail_case(case_name, "flush");
    rc = read_v5_once(&after);
    if (rc != 0) return fail_case(case_name, "snapshot-after");
    print_v5(case_name, &after);
    if (!ms08_net_full_recovered(&full, &after))
        return fail_case(case_name, "not-recovered");
    if (received != seq - 1)
        return fail_case(case_name, "conservation");
    printf("MS08_NET_SOCK: case=%s result=rw sent=%lu received=%lu\n", case_name,
           seq - 1, received);
    printf("MS08_NET_PEER: case=%s result=ok\n", case_name);
    printf("PASS: %s\n", case_name);
    fflush(stdout);
    return 0;

release_fail:
    /* Bounded cleanup on every exit path: release the hold with the valid
     * op 3 / lease 0 contract. */
    {
        uint64_t release[2];
        if (ms08_net_diag_release_payload(release) == 0)
            (void)ioctl(STDIN_FILENO, MS08_NET_DIAGNOSTIC_CONTROL, release);
    }
    close(fd);
    return fail_case(case_name, reason);
}

static int run_readiness_quiet(uint64_t overall_deadline)
{
    const char *case_name = "readiness-quiet";
    struct ms08_net_v5 b;
    uint64_t deadline;
    int fd, rc;
    char frame[96];

    if (make_deadline(overall_deadline, MS08_NET_PHASE_DEADLINE_MS,
                      &deadline) != 0)
        return fail_case(case_name, "deadline");
    fd = open_peer_socket(SOCK_DGRAM, case_name, deadline);
    if (fd < 0) return fail_case(case_name, "socket");
    /* Empty readiness first: nothing pending, poll must report no event. */
    {
        struct pollfd pfd = { .fd = fd, .events = POLLIN };
        pfd.revents = 0;
        if (poll(&pfd, 1, 0) != 0) {
            close(fd);
            return fail_case(case_name, "spurious-readable");
        }
    }
    /* Arm POLLIN by sending one frame, then consume the reply exactly once
     * after the readable event — no helper may consume it first (5.2-R3). */
    if (ms08_net_frame_encode(case_name, 1, frame, sizeof(frame)) != 0) {
        close(fd);
        return fail_case(case_name, "frame");
    }
    for (size_t off = 0; off < strlen(frame);) {
        ssize_t n;
        uint64_t now;
        if (wait_fd(fd, POLLOUT, deadline) != 0 || now_ms(&now) != 0 ||
            now >= deadline) {
            close(fd);
            return fail_case(case_name, "send");
        }
        n = send(fd, frame + off, strlen(frame) - off, MSG_DONTWAIT);
        if (n > 0) {
            off += (size_t)n;
            continue;
        }
        if (n < 0 && (errno == EAGAIN || errno == EINTR || errno == EWOULDBLOCK))
            continue;
        close(fd);
        return fail_case(case_name, "send");
    }
    if (wait_fd(fd, POLLIN, deadline) != 0) {
        close(fd);
        return fail_case(case_name, "not-readable");
    }
    {
        char reply[96];
        char echo_case[32];
        unsigned long echo_seq;
        ssize_t n = recv(fd, reply, sizeof(reply) - 1, MSG_DONTWAIT);
        if (n <= 0) {
            close(fd);
            return fail_case(case_name, "read");
        }
        reply[n] = '\0';
        if (strchr(reply, '\n') != NULL) *strchr(reply, '\n') = '\0';
        if (ms08_net_frame_decode(reply, echo_case, sizeof(echo_case),
                                  &echo_seq) != 0 ||
            strcmp(echo_case, case_name) != 0 || echo_seq != 1) {
            close(fd);
            return fail_case(case_name, "echo-mismatch");
        }
    }
    /* Ready: the arm frame was consumed, so the data path progressed.  Then a
     * bounded silent wait on the observed fd — any new readable byte in the
     * window is spontaneous I/O progress and fails the case.  The protocol
     * contract (`readiness-quiet: V5, SOCK, PEER`) carries a single terminal
     * V5 and the validator gates it on stack progress only; the silent window
     * (not a strict multi-snapshot counter equality) is the quiet criterion, so
     * the periodic stack runner maintenance poll must not be misread as I/O
     * progress (Iteration 005, 6.2). */
    rc = read_v5_once(&b);
    if (rc != 0) return fail_case(case_name, "snapshot");
    if (expect_fd_silence(fd, POLLIN, 200u) != 0) {
        close(fd);
        return fail_case(case_name, "quiet-progress");
    }
    close(fd);
    print_v5(case_name, &b);
    printf("MS08_NET_SOCK: case=%s result=rw sent=1 received=1\n", case_name);
    printf("MS08_NET_PEER: case=%s result=ok\n", case_name);
    printf("PASS: %s\n", case_name);
    fflush(stdout);
    return 0;
}


static int run_probe(void)
{
    uint64_t now, overall_deadline;
    printf("MS08_NET_START\nMS08_NET_ENV: %s\n", MS08_NET_ENVIRONMENT_DEFAULT);
    fflush(stdout);
    setvbuf(stdout, NULL, _IONBF, 0);
    if (now_ms(&now) != 0 || MS08_NET_OVERALL_DEADLINE_MS > UINT64_MAX - now)
        return fail_case("setup", "clock");
    overall_deadline = now + MS08_NET_OVERALL_DEADLINE_MS;

#define MS08_NET_PHASE(OUT) \
    do { \
        uint64_t _budget = MS08_NET_PHASE_DEADLINE_MS; \
        if (make_deadline(overall_deadline, _budget, &(OUT))) \
            return fail_case("setup", "deadline"); \
    } while (0)

    {
        uint64_t deadline;
        printf("MS08_NET_CASE_START: placement\n");
        MS08_NET_PHASE(deadline);
        if (run_placement(deadline) != 0) return 1;
    }
    {
        uint64_t deadline;
        printf("MS08_NET_CASE_START: timer-disabled-wake\n");
        MS08_NET_PHASE(deadline);
        if (run_timer_disabled_wake(deadline) != 0) return 1;
    }
    {
        uint64_t deadline;
        printf("MS08_NET_CASE_START: tcp-bidirectional\n");
        MS08_NET_PHASE(deadline);
        if (run_bidirectional("tcp-bidirectional", SOCK_STREAM, deadline) != 0)
            return 1;
    }
    {
        uint64_t deadline;
        printf("MS08_NET_CASE_START: udp-bidirectional\n");
        MS08_NET_PHASE(deadline);
        if (run_bidirectional("udp-bidirectional", SOCK_DGRAM, deadline) != 0)
            return 1;
    }
    printf("MS08_NET_CASE_START: full-recovery\n");
    if (run_full_recovery(overall_deadline) != 0) return 1;
    printf("MS08_NET_CASE_START: readiness-quiet\n");
    if (run_readiness_quiet(overall_deadline) != 0) return 1;

#undef MS08_NET_PHASE

    printf("MS08_NET_END\n");
    fflush(stdout);
    return 0;
}

int main(int argc, char **argv)
{
    if (argc == 2 && strcmp(argv[1], "--print-cases") == 0) {
        for (unsigned i = 0; i < ms08_net_case_count(); ++i)
            puts(ms08_net_cases[i]);
        return 0;
    }
    if (argc == 2 && strcmp(argv[1], "--print-schema") == 0) {
        for (unsigned i = 0; i < ms08_net_schema_count(); ++i)
            puts(ms08_net_schema[i]);
        return 0;
    }
    if (argc == 2 && strcmp(argv[1], "--self-test") == 0)
        return ms08_net_probe_self_test() ? 0 : 1;
    if (argc == 2 && strcmp(argv[1], "--run") == 0)
        return run_probe();
    fprintf(stderr, "usage: %s --print-cases | --print-schema | --self-test | --run\n",
            argv[0]);
    return 2;
}

#endif /* MS08_NET_PROBE_TESTING */
