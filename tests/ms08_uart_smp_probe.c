/* MS08 UART SMP qualification guest probe (Iteration 004 protocol task 5.1).
 *
 * Pure decision core shared with the host harness (tests/ms08_uart_smp_probe_test.c):
 * the case registry, the numbered RX-frame codec, the sequence ledger and the
 * snapshot relationship checks are all plain C so the host test can drive them
 * without QEMU.  The QEMU serial choreography lives in
 * scripts/ms08-uart-serial.py: the guest prints MS08_UART_NEED markers, the
 * host injects numbered ASCII frames through the serial socket, and the guest
 * echoes the exact received bytes as MS08UARTECHO frames for the host to
 * compare byte-for-byte.  A console PASS marker alone never proves receipt.
 *
 * Transcript grammar (consumed by scripts/ms08-uart-validate.py):
 *   MS08_UART_START / MS08_UART_ENV: <text>           (env is diagnostic only)
 *   MS08_UART_READY                                   (harness may arm)
 *   MS08_UART_CASE_START: <case> ... PASS: <case>     (frozen case order)
 *   MS08_UART_NEED: case=<case> count=N len=L         (host inject trigger)
 *   MS08_UART_SNAP: case=<case> <snapshot fields>     (224B wire decoded)
 *   MS08_UART_RX: case=<case> seq=N len=L ok=0|1      (guest-side validation)
 *   MS08UARTECHO seq=N len=L <exact bytes>            (host-side comparison)
 *   MS08_UART_END / MS08_UART_HARNESS_EXIT: 0
 */
#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <poll.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define MS08_UART_ENVIRONMENT_DEFAULT "qemu-virt-riscv64-smp16-ns16550-serial-socket"
#define MS08_UART_SNAPSHOT_IOCTL 0x55534d31u
#define MS08_UART_MIGRATE_IOCTL 0x55534d32u
#define MS08_UART_TCDRAIN_IOCTL 0x5409u
#define MS08_UART_AUTO_HART 0xffffffffu
#define MS08_UART_OVERALL_DEADLINE_MS 180000u
#define MS08_UART_PHASE_DEADLINE_MS 30000u
#define MS08_UART_MIGRATION_DEADLINE_MS 60000u
#define MS08_UART_REAP_GRACE_MS 1000u /* SIGKILL-to-reap bound in cleanup */

/* Numbered injected frames: printable ASCII payload, no spaces. */
#define MS08_UART_FRAME_MAX_PAYLOAD 48u
#define MS08_UART_RX_FRAME_COUNT 4u
#define MS08_UART_RX_FRAME_LEN 32u
#define MS08_UART_READINESS_FRAME_COUNT 1u
#define MS08_UART_READINESS_FRAME_LEN 16u
#define MS08_UART_MIGRATION_FRAME_COUNT 4u
#define MS08_UART_MIGRATION_FRAME_LEN 32u
#define MS08_UART_TX_FILL_BYTES (256u * 1024u)
#define MS08_UART_QUIET_WINDOW_MS 200u

/* 224-byte little-endian UartSmpSnapshot wire (kernel uart_snapshot_types.rs).
 * Reserved alignment bytes stay zero; only the offsets below are consumed. */
#define MS08_UART_WIRE_SIZE 224u

struct ms08_uart_snapshot {
    uint32_t magic;
    uint32_t configured_harts;
    uint64_t schedulable_mask;
    uint64_t rx_affinity;
    uint64_t tx_affinity;
    int32_t irq_last_hart;
    uint64_t irq_hart_mask;
    int32_t rx_last_hart;
    uint64_t rx_hart_mask;
    int32_t tx_last_hart;
    uint64_t tx_hart_mask;
    uint32_t rx_occupancy;
    uint32_t tx_vacancy;
    uint8_t ring_empty;
    uint8_t copier_active;
    uint32_t staged_bytes;
    uint8_t transmitter_empty;
    uint64_t irq_events;
    uint64_t rx_polls;
    uint64_t tx_polls;
    uint64_t ipi_sent;
    uint64_t ipi_received;
    uint64_t affinity_rejects;
    uint8_t rx_migration_state;
    uint8_t tx_migration_state;
    uint32_t rx_migration_from;
    uint32_t rx_migration_to;
    uint32_t tx_migration_from;
    uint32_t tx_migration_to;
    uint64_t rx_migration_requested_polls;
    uint64_t rx_migration_observed_polls;
    uint64_t rx_migration_rejects;
    uint64_t tx_migration_requested_polls;
    uint64_t tx_migration_observed_polls;
    uint64_t tx_migration_rejects;
};

/* Wire field offsets (fixed by the kernel ABI). */
#define MS08_UART_OFF_MAGIC 0u
#define MS08_UART_OFF_CONFIGURED 4u
#define MS08_UART_OFF_SCHEDULABLE 8u
#define MS08_UART_OFF_RX_AFFINITY 16u
#define MS08_UART_OFF_TX_AFFINITY 24u
#define MS08_UART_OFF_IRQ_LAST 32u
#define MS08_UART_OFF_IRQ_MASK 40u
#define MS08_UART_OFF_RX_LAST 48u
#define MS08_UART_OFF_RX_MASK 56u
#define MS08_UART_OFF_TX_LAST 64u
#define MS08_UART_OFF_TX_MASK 72u
#define MS08_UART_OFF_RX_OCC 80u
#define MS08_UART_OFF_TX_VAC 84u
#define MS08_UART_OFF_RING_EMPTY 88u
#define MS08_UART_OFF_COPIER_ACTIVE 89u
#define MS08_UART_OFF_STAGED 92u
#define MS08_UART_OFF_TEMT 96u
#define MS08_UART_OFF_IRQ_EVENTS 104u
#define MS08_UART_OFF_RX_POLLS 112u
#define MS08_UART_OFF_TX_POLLS 120u
#define MS08_UART_OFF_IPI_SENT 128u
#define MS08_UART_OFF_IPI_RECEIVED 136u
#define MS08_UART_OFF_REJECTS 144u
#define MS08_UART_OFF_RX_MSTATE 152u
#define MS08_UART_OFF_TX_MSTATE 153u
#define MS08_UART_OFF_RX_MFROM 156u
#define MS08_UART_OFF_RX_MTO 160u
#define MS08_UART_OFF_TX_MFROM 164u
#define MS08_UART_OFF_TX_MTO 168u
#define MS08_UART_OFF_RX_REQ_POLLS 176u
#define MS08_UART_OFF_RX_OBS_POLLS 184u
#define MS08_UART_OFF_RX_MREJECTS 192u
#define MS08_UART_OFF_TX_REQ_POLLS 200u
#define MS08_UART_OFF_TX_OBS_POLLS 208u
#define MS08_UART_OFF_TX_MREJECTS 216u

static uint32_t ms08_get_u32(const uint8_t *w, size_t off)
{
    return (uint32_t)w[off] | ((uint32_t)w[off + 1] << 8) |
           ((uint32_t)w[off + 2] << 16) | ((uint32_t)w[off + 3] << 24);
}

static uint64_t ms08_get_u64(const uint8_t *w, size_t off)
{
    uint64_t v = 0;
    for (unsigned i = 0; i < 8; ++i) v |= (uint64_t)w[off + i] << (8 * i);
    return v;
}

static void ms08_put_u32(uint8_t *w, size_t off, uint32_t v)
{
    for (unsigned i = 0; i < 4; ++i) w[off + i] = (uint8_t)(v >> (8 * i));
}

static void ms08_put_u64(uint8_t *w, size_t off, uint64_t v)
{
    for (unsigned i = 0; i < 8; ++i) w[off + i] = (uint8_t)(v >> (8 * i));
}

/* Reserved alignment bytes must stay zero on the wire. */
static int ms08_uart_reserved_zeroed(const uint8_t *w)
{
    static const struct { size_t off, len; } gaps[] = {
        {36, 4}, {52, 4}, {68, 4}, {90, 2}, {97, 7}, {154, 2}, {172, 4},
    };
    for (size_t i = 0; i < sizeof(gaps) / sizeof(gaps[0]); ++i)
        for (size_t j = 0; j < gaps[i].len; ++j)
            if (w[gaps[i].off + j] != 0) return 0;
    return 1;
}

void ms08_uart_snapshot_to_wire(const struct ms08_uart_snapshot *s, uint8_t *w)
{
    memset(w, 0, MS08_UART_WIRE_SIZE);
    ms08_put_u32(w, MS08_UART_OFF_MAGIC, s->magic);
    ms08_put_u32(w, MS08_UART_OFF_CONFIGURED, s->configured_harts);
    ms08_put_u64(w, MS08_UART_OFF_SCHEDULABLE, s->schedulable_mask);
    ms08_put_u64(w, MS08_UART_OFF_RX_AFFINITY, s->rx_affinity);
    ms08_put_u64(w, MS08_UART_OFF_TX_AFFINITY, s->tx_affinity);
    ms08_put_u32(w, MS08_UART_OFF_IRQ_LAST, (uint32_t)s->irq_last_hart);
    ms08_put_u64(w, MS08_UART_OFF_IRQ_MASK, s->irq_hart_mask);
    ms08_put_u32(w, MS08_UART_OFF_RX_LAST, (uint32_t)s->rx_last_hart);
    ms08_put_u64(w, MS08_UART_OFF_RX_MASK, s->rx_hart_mask);
    ms08_put_u32(w, MS08_UART_OFF_TX_LAST, (uint32_t)s->tx_last_hart);
    ms08_put_u64(w, MS08_UART_OFF_TX_MASK, s->tx_hart_mask);
    ms08_put_u32(w, MS08_UART_OFF_RX_OCC, s->rx_occupancy);
    ms08_put_u32(w, MS08_UART_OFF_TX_VAC, s->tx_vacancy);
    w[MS08_UART_OFF_RING_EMPTY] = s->ring_empty;
    w[MS08_UART_OFF_COPIER_ACTIVE] = s->copier_active;
    ms08_put_u32(w, MS08_UART_OFF_STAGED, s->staged_bytes);
    w[MS08_UART_OFF_TEMT] = s->transmitter_empty;
    ms08_put_u64(w, MS08_UART_OFF_IRQ_EVENTS, s->irq_events);
    ms08_put_u64(w, MS08_UART_OFF_RX_POLLS, s->rx_polls);
    ms08_put_u64(w, MS08_UART_OFF_TX_POLLS, s->tx_polls);
    ms08_put_u64(w, MS08_UART_OFF_IPI_SENT, s->ipi_sent);
    ms08_put_u64(w, MS08_UART_OFF_IPI_RECEIVED, s->ipi_received);
    ms08_put_u64(w, MS08_UART_OFF_REJECTS, s->affinity_rejects);
    w[MS08_UART_OFF_RX_MSTATE] = s->rx_migration_state;
    w[MS08_UART_OFF_TX_MSTATE] = s->tx_migration_state;
    ms08_put_u32(w, MS08_UART_OFF_RX_MFROM, s->rx_migration_from);
    ms08_put_u32(w, MS08_UART_OFF_RX_MTO, s->rx_migration_to);
    ms08_put_u32(w, MS08_UART_OFF_TX_MFROM, s->tx_migration_from);
    ms08_put_u32(w, MS08_UART_OFF_TX_MTO, s->tx_migration_to);
    ms08_put_u64(w, MS08_UART_OFF_RX_REQ_POLLS, s->rx_migration_requested_polls);
    ms08_put_u64(w, MS08_UART_OFF_RX_OBS_POLLS, s->rx_migration_observed_polls);
    ms08_put_u64(w, MS08_UART_OFF_RX_MREJECTS, s->rx_migration_rejects);
    ms08_put_u64(w, MS08_UART_OFF_TX_REQ_POLLS, s->tx_migration_requested_polls);
    ms08_put_u64(w, MS08_UART_OFF_TX_OBS_POLLS, s->tx_migration_observed_polls);
    ms08_put_u64(w, MS08_UART_OFF_TX_MREJECTS, s->tx_migration_rejects);
}

int ms08_uart_snapshot_from_wire(const uint8_t *w, struct ms08_uart_snapshot *s)
{
    if (ms08_get_u32(w, MS08_UART_OFF_MAGIC) != MS08_UART_SNAPSHOT_IOCTL)
        return -1;
    if (!ms08_uart_reserved_zeroed(w)) return -1;
    memset(s, 0, sizeof(*s));
    s->magic = ms08_get_u32(w, MS08_UART_OFF_MAGIC);
    s->configured_harts = ms08_get_u32(w, MS08_UART_OFF_CONFIGURED);
    s->schedulable_mask = ms08_get_u64(w, MS08_UART_OFF_SCHEDULABLE);
    s->rx_affinity = ms08_get_u64(w, MS08_UART_OFF_RX_AFFINITY);
    s->tx_affinity = ms08_get_u64(w, MS08_UART_OFF_TX_AFFINITY);
    s->irq_last_hart = (int32_t)ms08_get_u32(w, MS08_UART_OFF_IRQ_LAST);
    s->irq_hart_mask = ms08_get_u64(w, MS08_UART_OFF_IRQ_MASK);
    s->rx_last_hart = (int32_t)ms08_get_u32(w, MS08_UART_OFF_RX_LAST);
    s->rx_hart_mask = ms08_get_u64(w, MS08_UART_OFF_RX_MASK);
    s->tx_last_hart = (int32_t)ms08_get_u32(w, MS08_UART_OFF_TX_LAST);
    s->tx_hart_mask = ms08_get_u64(w, MS08_UART_OFF_TX_MASK);
    s->rx_occupancy = ms08_get_u32(w, MS08_UART_OFF_RX_OCC);
    s->tx_vacancy = ms08_get_u32(w, MS08_UART_OFF_TX_VAC);
    s->ring_empty = w[MS08_UART_OFF_RING_EMPTY];
    s->copier_active = w[MS08_UART_OFF_COPIER_ACTIVE];
    s->staged_bytes = ms08_get_u32(w, MS08_UART_OFF_STAGED);
    s->transmitter_empty = w[MS08_UART_OFF_TEMT];
    s->irq_events = ms08_get_u64(w, MS08_UART_OFF_IRQ_EVENTS);
    s->rx_polls = ms08_get_u64(w, MS08_UART_OFF_RX_POLLS);
    s->tx_polls = ms08_get_u64(w, MS08_UART_OFF_TX_POLLS);
    s->ipi_sent = ms08_get_u64(w, MS08_UART_OFF_IPI_SENT);
    s->ipi_received = ms08_get_u64(w, MS08_UART_OFF_IPI_RECEIVED);
    s->affinity_rejects = ms08_get_u64(w, MS08_UART_OFF_REJECTS);
    s->rx_migration_state = w[MS08_UART_OFF_RX_MSTATE];
    s->tx_migration_state = w[MS08_UART_OFF_TX_MSTATE];
    s->rx_migration_from = ms08_get_u32(w, MS08_UART_OFF_RX_MFROM);
    s->rx_migration_to = ms08_get_u32(w, MS08_UART_OFF_RX_MTO);
    s->tx_migration_from = ms08_get_u32(w, MS08_UART_OFF_TX_MFROM);
    s->tx_migration_to = ms08_get_u32(w, MS08_UART_OFF_TX_MTO);
    s->rx_migration_requested_polls = ms08_get_u64(w, MS08_UART_OFF_RX_REQ_POLLS);
    s->rx_migration_observed_polls = ms08_get_u64(w, MS08_UART_OFF_RX_OBS_POLLS);
    s->rx_migration_rejects = ms08_get_u64(w, MS08_UART_OFF_RX_MREJECTS);
    s->tx_migration_requested_polls = ms08_get_u64(w, MS08_UART_OFF_TX_REQ_POLLS);
    s->tx_migration_observed_polls = ms08_get_u64(w, MS08_UART_OFF_TX_OBS_POLLS);
    s->tx_migration_rejects = ms08_get_u64(w, MS08_UART_OFF_TX_MREJECTS);
    return 0;
}

/* ── Case registry (frozen) ─────────────────────────────────────────── */

static const char *const ms08_uart_cases[] = {
    "placement", "rx", "tx-full-recovery", "readiness",
    "tcdrain", "quiet", "rx-migration", "tx-migration",
};

/* Per-case marker contract (schema guard compares this with the validator). */
static const char *const ms08_uart_schema[] = {
    "placement:MS08_UART_SNAP",
    "rx:MS08_UART_NEED,MS08_UART_RX,MS08UARTECHO,MS08_UART_SNAP",
    "tx-full-recovery:MS08_UART_SNAP,MS08_UART_SNAP",
    "readiness:MS08_UART_NEED,MS08_UART_RX,MS08UARTECHO,MS08_UART_SNAP",
    "tcdrain:MS08_UART_SNAP",
    "quiet:MS08_UART_SNAP,MS08_UART_SNAP",
    "rx-migration:MS08_UART_SNAP,MS08_UART_NEED,MS08_UART_RX,MS08UARTECHO,MS08_UART_SNAP,MS08_UART_SNAP",
    "tx-migration:MS08_UART_SNAP,MS08_UART_SNAP,MS08_UART_SNAP",
};

unsigned ms08_uart_case_count(void)
{
    return sizeof(ms08_uart_cases) / sizeof(ms08_uart_cases[0]);
}

unsigned ms08_uart_schema_count(void)
{
    return sizeof(ms08_uart_schema) / sizeof(ms08_uart_schema[0]);
}

/* ── Numbered RX frame codec + sequence ledger ──────────────────────── */

int ms08_uart_frame_encode(unsigned long seq, const char *payload, size_t len,
                           char *buf, size_t cap)
{
    int n;
    if (buf == NULL || payload == NULL || len == 0 ||
        len > MS08_UART_FRAME_MAX_PAYLOAD)
        return -1;
    for (size_t i = 0; i < len; ++i)
        if (payload[i] <= 0x20 || payload[i] > 0x7e) return -1;
    n = snprintf(buf, cap, "MS08RX seq=%lu len=%zu %s", seq, len, payload);
    if (n < 0 || (size_t)n >= cap) return -1;
    return 0;
}

int ms08_uart_frame_decode(const char *line, unsigned long *seq, char *payload,
                           size_t payload_cap, size_t *len)
{
    unsigned long s;
    unsigned long l;
    const char *p;
    char *end;
    if (line == NULL || seq == NULL || payload == NULL || len == NULL)
        return -1;
    if (strncmp(line, "MS08RX seq=", 11) != 0) return -1;
    errno = 0;
    s = strtoul(line + 11, &end, 10);
    if (errno != 0 || end == line + 11 || s == 0 || s > 0xfffffffful)
        return -1;
    if (strncmp(end, " len=", 5) != 0) return -1;
    p = end + 5;
    errno = 0;
    l = strtoul(p, &end, 10);
    if (errno != 0 || end == p || l == 0 || l > MS08_UART_FRAME_MAX_PAYLOAD)
        return -1;
    if (*end != ' ' || end[1] == '\0') return -1;
    p = end + 1;
    /* Exactly one payload token: no spaces, correct length, then end of line. */
    if (strlen(p) != l) return -1;
    for (size_t i = 0; i < l; ++i)
        if (p[i] <= 0x20 || p[i] > 0x7e) return -1;
    if (l + 1 > payload_cap) return -1;
    memcpy(payload, p, l);
    payload[l] = '\0';
    *seq = s;
    *len = (size_t)l;
    return 0;
}

struct ms08_uart_seq_ledger {
    unsigned long next;
};

void ms08_uart_seq_ledger_reset(struct ms08_uart_seq_ledger *ledger)
{
    if (ledger != NULL) ledger->next = 1;
}

int ms08_uart_seq_ledger_accept(struct ms08_uart_seq_ledger *ledger, unsigned long seq)
{
    if (ledger == NULL || seq != ledger->next) return 0;
    ledger->next += 1;
    return 1;
}

/* ── Snapshot relationship decisions ────────────────────────────────── */

static int ms08_uart_hart_schedulable(const struct ms08_uart_snapshot *s, uint64_t hart)
{
    return hart < 64 && (s->schedulable_mask & ((uint64_t)1 << hart)) != 0;
}

static uint8_t ms08_uart_mstate(const struct ms08_uart_snapshot *s, int tx)
{
    return tx ? s->tx_migration_state : s->rx_migration_state;
}

static uint32_t ms08_uart_mfrom(const struct ms08_uart_snapshot *s, int tx)
{
    return tx ? s->tx_migration_from : s->rx_migration_from;
}

static uint32_t ms08_uart_mto(const struct ms08_uart_snapshot *s, int tx)
{
    return tx ? s->tx_migration_to : s->rx_migration_to;
}

static int32_t ms08_uart_last_hart(const struct ms08_uart_snapshot *s, int tx)
{
    return tx ? s->tx_last_hart : s->rx_last_hart;
}

static uint64_t ms08_uart_polls(const struct ms08_uart_snapshot *s, int tx)
{
    return tx ? s->tx_polls : s->rx_polls;
}

static uint64_t ms08_uart_affinity(const struct ms08_uart_snapshot *s, int tx)
{
    return tx ? s->tx_affinity : s->rx_affinity;
}

/* A cumulative history bit is legal only when the current pin or this
 * copier's own migration record explains it: the recorded from/to harts
 * count only while they are members of the published schedulable set.
 * An in-set but unrecorded hart is rejected (Cycle 002, 6.1-R1). */
static uint64_t ms08_uart_explained_hart_bits(const struct ms08_uart_snapshot *s,
                                              int tx)
{
    uint64_t allowed = (uint64_t)1 << ms08_uart_affinity(s, tx);
    uint8_t st = ms08_uart_mstate(s, tx);
    if (st == 1 || st == 2) { /* Widened or Restored carries a record */
        uint32_t from = ms08_uart_mfrom(s, tx), to = ms08_uart_mto(s, tx);
        if (ms08_uart_hart_schedulable(s, from))
            allowed |= (uint64_t)1 << from;
        if (ms08_uart_hart_schedulable(s, to))
            allowed |= (uint64_t)1 << to;
    }
    return allowed;
}

int ms08_uart_placement_ok(const struct ms08_uart_snapshot *s)
{
    if (s == NULL || s->configured_harts == 0 || s->schedulable_mask == 0)
        return 0;
    if (s->rx_affinity == s->tx_affinity) return 0;
    if (!ms08_uart_hart_schedulable(s, s->rx_affinity) ||
        !ms08_uart_hart_schedulable(s, s->tx_affinity))
        return 0;
    /* Fixed placement: the copier must actually have polled on its pinned
     * hart (not merely claim the affinity) and shown attributable progress. */
    if (s->rx_last_hart != (int32_t)s->rx_affinity ||
        s->tx_last_hart != (int32_t)s->tx_affinity)
        return 0;
    if (s->rx_polls == 0 || s->tx_polls == 0) return 0;
    /* The cumulative history masks are lifetime telemetry, not the current
     * singleton: they must contain the pinned hart, and every recorded bit
     * must be explained by the pin or this copier's own recorded migration
     * (from/to, still schedulable). */
    if ((s->rx_hart_mask & ((uint64_t)1 << s->rx_affinity)) == 0 ||
        (s->tx_hart_mask & ((uint64_t)1 << s->tx_affinity)) == 0)
        return 0;
    if ((s->rx_hart_mask & ~ms08_uart_explained_hart_bits(s, 0)) != 0 ||
        (s->tx_hart_mask & ~ms08_uart_explained_hart_bits(s, 1)) != 0)
        return 0;
    return 1;
}

int ms08_uart_drain_ok(const struct ms08_uart_snapshot *s)
{
    return s != NULL && s->ring_empty == 1 && s->copier_active == 0 &&
           s->staged_bytes == 0 && s->transmitter_empty == 1;
}

int ms08_uart_quiet_ok(const struct ms08_uart_snapshot *a,
                       const struct ms08_uart_snapshot *b)
{
    return a != NULL && b != NULL &&
           a->irq_events == b->irq_events && a->rx_polls == b->rx_polls &&
           a->tx_polls == b->tx_polls && a->ipi_sent == b->ipi_sent &&
           a->ipi_received == b->ipi_received;
}

int ms08_uart_full_observed(const struct ms08_uart_snapshot *s)
{
    /* The TX ring reached vacancy zero: real capacity pressure, not a marker. */
    return s != NULL && s->tx_vacancy == 0;
}

int ms08_uart_full_recovered(const struct ms08_uart_snapshot *full,
                             const struct ms08_uart_snapshot *after)
{
    return full != NULL && after != NULL &&
           ms08_uart_full_observed(full) && ms08_uart_drain_ok(after) &&
           after->tx_vacancy != 0 && after->tx_polls > full->tx_polls;
}

int ms08_uart_migration_widened(const struct ms08_uart_snapshot *pre,
                                const struct ms08_uart_snapshot *mid, int tx)
{
    uint32_t from, to;
    if (pre == NULL || mid == NULL || pre->magic != MS08_UART_SNAPSHOT_IOCTL ||
        mid->magic != MS08_UART_SNAPSHOT_IOCTL)
        return 0;
    if (ms08_uart_mstate(pre, tx) != 0 || ms08_uart_mstate(mid, tx) != 1)
        return 0;
    from = ms08_uart_mfrom(mid, tx);
    to = ms08_uart_mto(mid, tx);
    if (from != ms08_uart_affinity(pre, tx) || from == to) return 0;
    if (!ms08_uart_hart_schedulable(mid, to)) return 0;
    /* The migrated copier must actually be observed polling on the second
     * hart with attributable progress; the other copier must not drift. */
    if (ms08_uart_last_hart(mid, tx) != (int32_t)to) return 0;
    if (ms08_uart_last_hart(mid, !tx) != ms08_uart_last_hart(pre, !tx)) return 0;
    return ms08_uart_polls(mid, tx) > ms08_uart_polls(pre, tx);
}

int ms08_uart_migration_restored(const struct ms08_uart_snapshot *mid,
                                 const struct ms08_uart_snapshot *post, int tx)
{
    if (mid == NULL || post == NULL || ms08_uart_mstate(mid, tx) != 1 ||
        ms08_uart_mstate(post, tx) != 2)
        return 0;
    if (ms08_uart_mfrom(mid, tx) != ms08_uart_mfrom(post, tx) ||
        ms08_uart_mto(mid, tx) != ms08_uart_mto(post, tx))
        return 0;
    if (ms08_uart_affinity(post, tx) != ms08_uart_mfrom(mid, tx)) return 0;
    if (ms08_uart_last_hart(post, tx) != (int32_t)ms08_uart_mfrom(mid, tx))
        return 0;
    return ms08_uart_polls(post, tx) > ms08_uart_polls(mid, tx);
}

/* Concurrency contract for the copier migration cases (5.1-R1): the
 * synchronous widen/observe/restore ioctl runs in a separate bounded control
 * caller, so the Widened view must be sampled while that control call is
 * still outstanding, the control caller must complete successfully, and the
 * migration must close.  ``progress_ok`` is direction-specific evidence
 * computed by the caller: RX frame conservation, or TX bytes written plus
 * copier poll progress (see ms08_uart_tx_migration_progress_ok). */
int ms08_uart_migration_record_ok(int pre_idle, int have_mid,
                                  int mid_while_outstanding, int have_post,
                                  int control_ok, int progress_ok)
{
    return pre_idle && have_mid && mid_while_outstanding && have_post &&
           control_ok && progress_ok;
}

/* TX-direction progress evidence: real bytes were written and the migrated
 * copier's poll counter moved between the pre and post views.  A zero frame
 * count (the RX-direction evidence) is never TX progress. */
int ms08_uart_tx_migration_progress_ok(const struct ms08_uart_snapshot *pre,
                                       const struct ms08_uart_snapshot *post,
                                       int tx, uint64_t written_bytes)
{
    return pre != NULL && post != NULL && written_bytes > 0 &&
           ms08_uart_polls(post, tx) > ms08_uart_polls(pre, tx);
}

int ms08_uart_probe_self_test(void)
{
    return ms08_uart_case_count() == 8 && ms08_uart_schema_count() == 8 &&
           strcmp(ms08_uart_cases[0], "placement") == 0 &&
           strcmp(ms08_uart_cases[7], "tx-migration") == 0 &&
           strcmp(ms08_uart_schema[2],
                  "tx-full-recovery:MS08_UART_SNAP,MS08_UART_SNAP") == 0;
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

/* Terminate a control caller that is still alive when the case has failed
 * or the deadline passed: a child parked inside the migration ioctl must
 * not outlive the case holding the inherited TTY and pipe write end
 * (5.1-R1 terminal cleanup).  SIGKILL plus a bounded reap inside a small
 * grace window; the parent never blocks unboundedly. */
static int terminate_control_caller(pid_t child)
{
    uint64_t grace, now;
    if (kill(child, SIGKILL) != 0 && errno != ESRCH) return -1;
    if (now_ms(&grace) != 0) return -1;
    grace += MS08_UART_REAP_GRACE_MS;
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
     * nonblocking so no read can bypass the deadline (5.1-R1 bounded
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

/* ── Serial line framing: the console byte stream is chunked arbitrarily, so
 * injected frames are reassembled from complete '\n'-terminated lines.  The
 * reader is a pure shared decision core: it is compiled into both the guest
 * payload and the host decision test, so read partitioning, overflow and
 * frame ordering are witnessed without QEMU (Iteration 005, task 6.1). ── */

#define MS08_UART_LINE_CAP 160u

struct ms08_uart_line_reader {
    char buf[MS08_UART_LINE_CAP];
    size_t len;
};

/* Consume one received byte chunk, delivering every complete '\n'-terminated
 * line through `sink` in arrival order (a sink that rejects a line aborts the
 * push with -1).  The buffer holds only the bytes of the current partial
 * line: its capacity bounds the largest valid line however the console
 * stream is partitioned, so a valid residual plus any allowed 128-byte read
 * chunk can never overflow an aggregate buffer (Cycle 002, 6.1-R1).
 * Returns 0 on success, -1 when an individual line exceeds
 * MS08_UART_LINE_CAP before a newline (an explicit corruption failure, never
 * a silent drop). */
typedef int (*ms08_uart_line_sink)(void *ctx, const char *line, size_t len);

static int line_reader_push(struct ms08_uart_line_reader *r, const char *chunk,
                            size_t n, ms08_uart_line_sink sink, void *ctx)
{
    for (size_t i = 0; i < n; ++i) {
        char c = chunk[i];
        if (c == '\n') {
            size_t out = r->len;
            while (out > 0 && r->buf[out - 1] == '\r') out -= 1;
            if (sink(ctx, r->buf, out) != 0) return -1;
            r->len = 0;
            continue;
        }
        if (r->len >= sizeof(r->buf)) return -1; /* overlong unframed line */
        r->buf[r->len++] = c;
    }
    return 0;
}

/* Per-line numbered-frame handling shared by the RX receiver and the
 * RX-migration loop: decode fail-closed, optional strict length check,
 * strict sequence ledger, receipt marker plus exact-byte echo.  The handler
 * records its failure tag in `ctx->err` for the caller's reason mapping.
 * Production call sites only; the host decision test exercises the reader
 * with its own sink. */
#ifndef MS08_UART_PROBE_TESTING
enum {
    MS08_UART_LINE_OK = 0,
    MS08_UART_LINE_DECODE,   /* malformed frame text */
    MS08_UART_LINE_LENGTH,   /* valid frame, unexpected payload length */
    MS08_UART_LINE_SEQUENCE, /* duplicate / out-of-order sequence */
};

struct ms08_uart_frame_line_ctx {
    const char *case_name;
    struct ms08_uart_seq_ledger *ledger;
    long expect_len; /* negative: accept any valid frame length */
    unsigned *received;
    char *payload; /* MS08_UART_FRAME_MAX_PAYLOAD + 1 bytes */
    size_t payload_cap;
    int err;
};

static int ms08_uart_frame_line(void *vctx, const char *line, size_t len)
{
    struct ms08_uart_frame_line_ctx *ctx = vctx;
    unsigned long seq;
    size_t plen;
    char text[MS08_UART_LINE_CAP + 1];

    if (len > MS08_UART_LINE_CAP) {
        ctx->err = MS08_UART_LINE_DECODE;
        return -1;
    }
    memcpy(text, line, len);
    text[len] = '\0';
    if (ms08_uart_frame_decode(text, &seq, ctx->payload, ctx->payload_cap,
                               &plen) != 0) {
        ctx->err = MS08_UART_LINE_DECODE;
        return -1;
    }
    if (ctx->expect_len >= 0 && (long)plen != ctx->expect_len) {
        ctx->err = MS08_UART_LINE_LENGTH;
        return -1;
    }
    if (!ms08_uart_seq_ledger_accept(ctx->ledger, seq)) {
        ctx->err = MS08_UART_LINE_SEQUENCE;
        return -1;
    }
    printf("MS08_UART_RX: case=%s seq=%lu len=%zu ok=1\n", ctx->case_name,
           seq, plen);
    /* Exact-byte echo: the host harness compares these bytes against what it
     * injected; a marker alone never proves receipt. */
    printf("MS08UARTECHO seq=%lu len=%zu %s\n", seq, plen, ctx->payload);
    fflush(stdout);
    *ctx->received += 1;
    return 0;
}

/* Map a failed frame-line push to the case failure reason. */
static const char *ms08_uart_line_fail_reason(int err)
{
    switch (err) {
    case MS08_UART_LINE_DECODE: return "rx-corrupt-frame";
    case MS08_UART_LINE_LENGTH: return "rx-length";
    case MS08_UART_LINE_SEQUENCE: return "rx-sequence";
    default: return "rx-overflow";
    }
}
#endif

#ifndef MS08_UART_PROBE_TESTING

static int read_snapshot(struct ms08_uart_snapshot *out)
{
    uint8_t wire[MS08_UART_WIRE_SIZE];
    memset(wire, 0, sizeof(wire));
    if (ioctl(STDIN_FILENO, MS08_UART_SNAPSHOT_IOCTL, wire) < 0) return -1;
    return ms08_uart_snapshot_from_wire(wire, out);
}

static void print_snap(const char *case_name, const struct ms08_uart_snapshot *s)
{
    printf("MS08_UART_SNAP: case=%s configured=%u schedulable=0x%llx "
           "rx_affinity=%llu tx_affinity=%llu irq_last=%d irq_mask=0x%llx "
           "rx_last=%d rx_mask=0x%llx tx_last=%d tx_mask=0x%llx "
           "rx_occ=%u tx_vac=%u ring_empty=%u copier_active=%u staged=%u temt=%u "
           "irq_events=%llu rx_polls=%llu tx_polls=%llu ipi_sent=%llu "
           "ipi_received=%llu rejects=%llu rx_mstate=%u rx_mfrom=%u rx_mto=%u "
           "tx_mstate=%u tx_mfrom=%u tx_mto=%u\n",
           case_name, s->configured_harts,
           (unsigned long long)s->schedulable_mask,
           (unsigned long long)s->rx_affinity,
           (unsigned long long)s->tx_affinity, s->irq_last_hart,
           (unsigned long long)s->irq_hart_mask, s->rx_last_hart,
           (unsigned long long)s->rx_hart_mask, s->tx_last_hart,
           (unsigned long long)s->tx_hart_mask, s->rx_occupancy, s->tx_vacancy,
           s->ring_empty, s->copier_active, s->staged_bytes, s->transmitter_empty,
           (unsigned long long)s->irq_events, (unsigned long long)s->rx_polls,
           (unsigned long long)s->tx_polls, (unsigned long long)s->ipi_sent,
           (unsigned long long)s->ipi_received,
           (unsigned long long)s->affinity_rejects, s->rx_migration_state,
           s->rx_migration_from, s->rx_migration_to, s->tx_migration_state,
           s->tx_migration_from, s->tx_migration_to);
    fflush(stdout);
}

/* Start a marker on a fresh protocol line even when the preceding bulk raw
 * TX burst (T/D/M bytes) left the stream mid-line: an appended marker would
 * otherwise be unreadable to the serial harness and the pure validator
 * (Iteration 005, task 6.1).  A leading newline is harmless to an already
 * fresh line but guarantees the marker is never merged into a payload. */
static void fresh_line(void)
{
    write(STDOUT_FILENO, "\n", 1);
}

static int fail_case(const char *case_name, const char *reason)
{
    printf("FAIL: %s reason=%s\n", case_name, reason);
    fflush(stdout);
    return 1;
}

/* One bounded wait that must end in silence: any input event is spontaneous
 * progress and fails the case.  Returns 0 on timed-out silence. */
static int expect_silence(int fd, short events, uint64_t window_ms)
{
    struct pollfd pfd = { .fd = fd, .events = events };
    uint64_t now, end;
    int remaining;
    if (now_ms(&now) != 0) return -1;
    end = now + window_ms;
    remaining = (int)(end - now);
    pfd.revents = 0;
    if (poll(&pfd, 1, remaining) < 0 && errno != EINTR) return -1;
    if (pfd.revents & (events | POLLERR | POLLNVAL)) return -1;
    return 0;
}

/* ── Read numbered frames from the console under a deadline: reassemble lines,
 * decode each frame fail-closed, enforce the strict sequence ledger, and echo
 * the exact received bytes for the host to compare. */
static int receive_frames(const char *case_name, unsigned count, size_t expect_len,
                          uint64_t deadline)
{
    struct ms08_uart_line_reader reader = { .len = 0 };
    struct ms08_uart_seq_ledger ledger;
    struct ms08_uart_frame_line_ctx ctx;
    char chunk[128];
    char payload[MS08_UART_FRAME_MAX_PAYLOAD + 1];
    unsigned received = 0;
    ms08_uart_seq_ledger_reset(&ledger);
    ctx.case_name = case_name;
    ctx.ledger = &ledger;
    ctx.expect_len = (long)expect_len;
    ctx.received = &received;
    ctx.payload = payload;
    ctx.payload_cap = sizeof(payload);
    ctx.err = MS08_UART_LINE_OK;
    while (received < count) {
        ssize_t n;
        if (wait_fd(STDIN_FILENO, POLLIN, deadline) != 0)
            return fail_case(case_name, "rx-wait");
        n = read(STDIN_FILENO, chunk, sizeof(chunk));
        if (n <= 0) return fail_case(case_name, "rx-read");
        if (line_reader_push(&reader, chunk, (size_t)n, ms08_uart_frame_line,
                             &ctx) != 0)
            return fail_case(case_name, ms08_uart_line_fail_reason(ctx.err));
    }
    return 0;
}

static int request_inject(const char *case_name, unsigned count, size_t len)
{
    printf("MS08_UART_NEED: case=%s count=%u len=%zu\n", case_name, count, len);
    fflush(stdout);
    return 0;
}

static int run_placement(uint64_t deadline)
{
    struct ms08_uart_snapshot s;
    if (read_snapshot(&s) != 0) return fail_case("placement", "snapshot");
    print_snap("placement", &s);
    if (!ms08_uart_placement_ok(&s)) return fail_case("placement", "policy");
    (void)deadline;
    printf("PASS: placement\n");
    fflush(stdout);
    return 0;
}

static int run_rx(uint64_t deadline)
{
    struct ms08_uart_snapshot before, after;
    if (read_snapshot(&before) != 0) return fail_case("rx", "snapshot-before");
    request_inject("rx", MS08_UART_RX_FRAME_COUNT, MS08_UART_RX_FRAME_LEN);
    if (receive_frames("rx", MS08_UART_RX_FRAME_COUNT, MS08_UART_RX_FRAME_LEN,
                       deadline) != 0)
        return 1;
    if (read_snapshot(&after) != 0) return fail_case("rx", "snapshot-after");
    print_snap("rx", &after);
    if (after.rx_polls <= before.rx_polls || after.irq_events <= before.irq_events)
        return fail_case("rx", "no-attributable-progress");
    printf("PASS: rx\n");
    fflush(stdout);
    return 0;
}

static int run_tx_full_recovery(uint64_t overall_deadline, uint64_t *phase_deadline_out)
{
    static char block[4096];
    struct ms08_uart_snapshot full, after;
    uint64_t filled_total = 0;
    int full_seen = 0;
    uint64_t deadline;
    int nb = 1;
    memset(block, 'T', sizeof(block));
    if (ioctl(STDIN_FILENO, FIONBIO, &nb) < 0)
        return fail_case("tx-full-recovery", "nonblock");
    if (now_ms(&deadline) != 0) return fail_case("tx-full-recovery", "clock");
    deadline += MS08_UART_PHASE_DEADLINE_MS;
    if (deadline > overall_deadline) deadline = overall_deadline;
    while (!full_seen) {
        uint64_t now;
        ssize_t w = write(STDOUT_FILENO, block, sizeof(block));
        if (w > 0) filled_total += (uint64_t)w;
        if (w < 0 && errno != EAGAIN && errno != EINTR && errno != EWOULDBLOCK)
            return fail_case("tx-full-recovery", "write");
        if ((w < 0 && (errno == EAGAIN || errno == EWOULDBLOCK)) ||
            filled_total >= MS08_UART_TX_FILL_BYTES) {
            if (read_snapshot(&full) != 0)
                return fail_case("tx-full-recovery", "snapshot-full");
            if (ms08_uart_full_observed(&full)) { full_seen = 1; break; }
        }
        if (now_ms(&now) != 0 || now >= deadline)
            return fail_case("tx-full-recovery", "deadline");
        if (wait_fd(STDOUT_FILENO, POLLOUT, deadline) != 0 && errno != EAGAIN)
            return fail_case("tx-full-recovery", "pollout");
    }
    if (!full_seen) return fail_case("tx-full-recovery", "never-full");
    fresh_line();   /* the T-bulk loop left the stream mid-line */
    print_snap("tx-full-recovery", &full);
    if (ioctl(STDIN_FILENO, MS08_UART_TCDRAIN_IOCTL, 0) < 0)
        return fail_case("tx-full-recovery", "tcdrain");
    if (read_snapshot(&after) != 0)
        return fail_case("tx-full-recovery", "snapshot-after");
    print_snap("tx-full-recovery", &after);
    if (!ms08_uart_full_recovered(&full, &after))
        return fail_case("tx-full-recovery", "not-recovered");
    *phase_deadline_out = deadline;
    printf("PASS: tx-full-recovery\n");
    fflush(stdout);
    return 0;
}

static int run_readiness(uint64_t deadline)
{
    struct pollfd pfd = { .fd = STDIN_FILENO, .events = POLLIN };
    struct ms08_uart_snapshot before, after;
    if (read_snapshot(&before) != 0) return fail_case("readiness", "snapshot-before");
    request_inject("readiness", MS08_UART_READINESS_FRAME_COUNT,
                   MS08_UART_READINESS_FRAME_LEN);
    /* Empty-readiness first: no data pending, poll must time out cleanly. */
    pfd.revents = 0;
    if (poll(&pfd, 1, 0) != 0) return fail_case("readiness", "spurious-readable");
    if (wait_fd(STDIN_FILENO, POLLIN, deadline) != 0)
        return fail_case("readiness", "not-readable");
    if (receive_frames("readiness", MS08_UART_READINESS_FRAME_COUNT,
                       MS08_UART_READINESS_FRAME_LEN, deadline) != 0)
        return 1;
    if (read_snapshot(&after) != 0) return fail_case("readiness", "snapshot-after");
    print_snap("readiness", &after);
    if (after.rx_polls <= before.rx_polls)
        return fail_case("readiness", "no-progress");
    printf("PASS: readiness\n");
    fflush(stdout);
    return 0;
}

static int run_tcdrain(uint64_t deadline)
{
    static char payload[512];
    struct ms08_uart_snapshot after;
    memset(payload, 'D', sizeof(payload));
    if (write(STDOUT_FILENO, payload, sizeof(payload)) < 0)
        return fail_case("tcdrain", "write");
    if (ioctl(STDIN_FILENO, MS08_UART_TCDRAIN_IOCTL, 0) < 0)
        return fail_case("tcdrain", "ioctl");
    fresh_line();   /* the 512-byte D payload left the stream mid-line */
    if (read_snapshot(&after) != 0) return fail_case("tcdrain", "snapshot");
    print_snap("tcdrain", &after);
    if (!ms08_uart_drain_ok(&after)) return fail_case("tcdrain", "not-drained");
    (void)deadline;
    printf("PASS: tcdrain\n");
    fflush(stdout);
    return 0;
}

static int run_quiet(uint64_t overall_deadline)
{
    struct ms08_uart_snapshot a, b;
    (void)overall_deadline;
    if (read_snapshot(&a) != 0) return fail_case("quiet", "snapshot-before");
    print_snap("quiet", &a);
    /* The quiet window is one bounded silent wait on the observed fd: any
     * input event is spontaneous progress. */
    if (expect_silence(STDIN_FILENO, POLLIN, MS08_UART_QUIET_WINDOW_MS) != 0)
        return fail_case("quiet", "spurious-input");
    if (read_snapshot(&b) != 0) return fail_case("quiet", "snapshot-after");
    print_snap("quiet", &b);
    if (!ms08_uart_quiet_ok(&a, &b)) return fail_case("quiet", "progress-drift");
    printf("PASS: quiet\n");
    fflush(stdout);
    return 0;
}

static int run_copier_migration(const char *case_name, int tx, uint64_t deadline)
{
    struct ms08_uart_snapshot pre, snap, mid, post;
    int have_mid = 0, have_post = 0, mid_while_outstanding = 0;
    int child_done = 0, child_ok = 0, reaped = 0;
    unsigned received = 0;
    uint64_t written = 0;
    uint64_t arg = tx ? 1u : 0u;
    int pipefd[2];
    pid_t child;
    const char *reason = NULL;
    int progress_ok = 0;

    arg |= (uint64_t)MS08_UART_AUTO_HART << 32;
    if (read_snapshot(&pre) != 0) return fail_case(case_name, "snapshot-pre");
    print_snap(case_name, &pre);
    if (ms08_uart_mstate(&pre, tx) != 0)
        return fail_case(case_name, "migration-not-idle");
    /* The migration ioctl is synchronous (widen/observe/restore), so a
     * separate bounded control caller holds it while this thread drives
     * stimulus and samples the migration view (5.1-R1). */
    if (pipe(pipefd) != 0) return fail_case(case_name, "pipe");
    child = fork();
    if (child < 0) {
        close(pipefd[0]);
        close(pipefd[1]);
        return fail_case(case_name, "fork");
    }
    if (child == 0) {
        char byte = 'K';
        close(pipefd[0]);
        if (ioctl(STDIN_FILENO, MS08_UART_MIGRATE_IOCTL, arg) < 0) byte = 'E';
        (void)write(pipefd[1], &byte, 1);
        _exit(0);
    }
    close(pipefd[1]);

#define MS08_UART_MIGRATION_OBSERVE()                                          \
    do {                                                                       \
        if (read_snapshot(&snap) != 0) {                                       \
            reason = "snapshot-poll";                                          \
            goto fail_cleanup;                                                 \
        }                                                                      \
        if (!have_mid && ms08_uart_mstate(&snap, tx) == 1 &&                   \
            ms08_uart_last_hart(&snap, tx) ==                                  \
                (int32_t)ms08_uart_mto(&snap, tx)) {                           \
            mid = snap;                                                        \
            have_mid = 1;                                                      \
            if (!child_done) mid_while_outstanding = 1;                        \
        }                                                                      \
        if (ms08_uart_mstate(&snap, tx) == 2) {                                \
            post = snap;                                                       \
            have_post = 1;                                                     \
        }                                                                      \
    } while (0)

    if (!tx) {
        /* The RX copier only polls when RX traffic arrives: the host injects
         * numbered frames while we consume them and watch the migration view.
         * The completion pipe is the control-caller event. */
        struct ms08_uart_line_reader reader = { .len = 0 };
        struct ms08_uart_seq_ledger ledger;
        struct ms08_uart_frame_line_ctx ctx;
        char chunk[128];
        char payload[MS08_UART_FRAME_MAX_PAYLOAD + 1];
        ms08_uart_seq_ledger_reset(&ledger);
        ctx.case_name = case_name;
        ctx.ledger = &ledger;
        ctx.expect_len = -1; /* any valid migration frame length */
        ctx.received = &received;
        ctx.payload = payload;
        ctx.payload_cap = sizeof(payload);
        ctx.err = MS08_UART_LINE_OK;
        request_inject(case_name, MS08_UART_MIGRATION_FRAME_COUNT,
                       MS08_UART_MIGRATION_FRAME_LEN);
        while (received < MS08_UART_MIGRATION_FRAME_COUNT) {
            struct pollfd pfds[2];
            uint64_t now;
            int prc;
            if (now_ms(&now) != 0 || now >= deadline) {
                reason = "deadline";
                goto fail_cleanup;
            }
            pfds[0].fd = STDIN_FILENO;
            pfds[0].events = POLLIN;
            pfds[0].revents = 0;
            pfds[1].fd = pipefd[0];
            pfds[1].events = POLLIN;
            pfds[1].revents = 0;
            prc = poll(pfds, 2,
                       (int)((deadline - now) > 1000u ? 1000u
                                                      : deadline - now));
            if (prc < 0 && errno != EINTR) {
                reason = "poll";
                goto fail_cleanup;
            }
            if (pfds[1].revents & POLLIN) {
                char byte;
                if (read(pipefd[0], &byte, 1) == 1) {
                    child_done = 1;
                    child_ok = byte == 'K';
                }
            }
            if (pfds[0].revents & POLLIN) {
                ssize_t n = read(STDIN_FILENO, chunk, sizeof(chunk));
                if (n <= 0) {
                    reason = "rx-read";
                    goto fail_cleanup;
                }
                if (line_reader_push(&reader, chunk, (size_t)n,
                                     ms08_uart_frame_line, &ctx) != 0) {
                    reason = ms08_uart_line_fail_reason(ctx.err);
                    goto fail_cleanup;
                }
            }
            /* Consume-and-observe after every event batch. */
            MS08_UART_MIGRATION_OBSERVE();
        }
        /* RX-direction progress evidence: every injected frame drained,
         * echoed and strictly sequenced (the ledger above enforces order). */
        progress_ok = received == MS08_UART_MIGRATION_FRAME_COUNT;
    } else {
        /* The TX copier polls when TX traffic flows: writes are paced by
         * console backpressure (POLLOUT) and the completion pipe. */
        static char block[1024];
        memset(block, 'M', sizeof(block));
        while (!have_post || !child_done || !reaped) {
            struct pollfd pfds[2];
            uint64_t now;
            ssize_t w;
            int prc;
            if (now_ms(&now) != 0 || now >= deadline) {
                reason = "deadline";
                goto fail_cleanup;
            }
            w = write(STDOUT_FILENO, block, sizeof(block));
            if (w < 0 && errno != EAGAIN && errno != EINTR && errno != EWOULDBLOCK) {
                reason = "write";
                goto fail_cleanup;
            }
            if (w > 0) written += (uint64_t)w;
            MS08_UART_MIGRATION_OBSERVE();
            pfds[0].fd = STDIN_FILENO;
            pfds[0].events = POLLIN;
            pfds[0].revents = 0;
            pfds[1].fd = pipefd[0];
            pfds[1].events = POLLIN;
            pfds[1].revents = 0;
            prc = poll(pfds, 2,
                       (int)((deadline - now) > 1000u ? 1000u
                                                      : deadline - now));
            if (prc < 0 && errno != EINTR) {
                reason = "poll";
                goto fail_cleanup;
            }
            if (pfds[1].revents & POLLIN) {
                char byte;
                if (read(pipefd[0], &byte, 1) == 1) {
                    child_done = 1;
                    child_ok = byte == 'K';
                }
            }
            if (pfds[0].revents & POLLIN) {
                char scratch[128];
                ssize_t n;
                while ((n = read(STDIN_FILENO, scratch, sizeof(scratch))) > 0)
                    continue;
                (void)n;
            }
            if (child_done && !reaped && waitpid(child, NULL, WNOHANG) == child)
                reaped = 1;
        }
        /* TX-direction progress evidence: real bytes written and the
         * migrated copier's poll counter moved (never the RX frame count). */
        progress_ok = ms08_uart_tx_migration_progress_ok(&pre, &post, tx,
                                                         written);
    }
    /* Both directions: the control caller must complete and the Restored
     * view must be observed; events are the completion pipe and any
     * remaining console bytes. */
    while (!child_done || !have_post || !reaped) {
        struct pollfd pfds[2];
        uint64_t now;
        int prc;
        if (now_ms(&now) != 0 || now >= deadline) {
            reason = "deadline";
            goto fail_cleanup;
        }
        pfds[0].fd = pipefd[0];
        pfds[0].events = POLLIN;
        pfds[0].revents = 0;
        pfds[1].fd = STDIN_FILENO;
        pfds[1].events = POLLIN;
        pfds[1].revents = 0;
        prc = poll(pfds, 2,
                   (int)((deadline - now) > 1000u ? 1000u : deadline - now));
        if (prc < 0 && errno != EINTR) {
            reason = "poll";
            goto fail_cleanup;
        }
        if (pfds[0].revents & POLLIN) {
            char byte;
            if (read(pipefd[0], &byte, 1) == 1) {
                child_done = 1;
                child_ok = byte == 'K';
            }
        }
        if (pfds[1].revents & POLLIN) {
            char scratch[128];
            ssize_t n;
            while ((n = read(STDIN_FILENO, scratch, sizeof(scratch))) > 0)
                continue;
            (void)n;
        }
        if (!have_post) {
            if (read_snapshot(&snap) != 0) {
                reason = "snapshot-poll";
                goto fail_cleanup;
            }
            if (ms08_uart_mstate(&snap, tx) == 2) {
                post = snap;
                have_post = 1;
            }
        }
        if (child_done && !reaped && waitpid(child, NULL, WNOHANG) == child)
            reaped = 1;
    }
#undef MS08_UART_MIGRATION_OBSERVE
    close(pipefd[0]);
    if (!child_ok) return fail_case(case_name, "control-caller");
    if (!have_mid) return fail_case(case_name, "never-widened-observed");
    if (!have_post) return fail_case(case_name, "never-restored");
    if (!progress_ok) return fail_case(case_name, "no-direction-progress");
    /* The transcript grammar is frozen: the mid view (observed while the
     * control call was outstanding) prints after all injected frames. */
    if (tx) fresh_line();   /* the M-bulk loop left the stream mid-line */
    print_snap(case_name, &mid);
    print_snap(case_name, &post);
    if (!ms08_uart_migration_widened(&pre, &mid, tx))
        return fail_case(case_name, "widened-relation");
    if (!ms08_uart_migration_restored(&mid, &post, tx))
        return fail_case(case_name, "restored-relation");
    if (!ms08_uart_migration_record_ok(1, have_mid, mid_while_outstanding,
                                       have_post, child_ok, progress_ok))
        return fail_case(case_name, "migration-record");
    printf("PASS: %s\n", case_name);
    fflush(stdout);
    return 0;

fail_cleanup:
    /* Every exit path after fork closes the pipe and reaps the control
     * caller within the remaining deadline; the original reason stands. */
    (void)reap_control_caller(child, pipefd[0], deadline);
    return fail_case(case_name, reason);
}

static int run_probe(void)
{
    uint64_t now, overall_deadline, deadline;
    printf("MS08_UART_START\nMS08_UART_ENV: %s\nMS08_UART_READY\n",
           MS08_UART_ENVIRONMENT_DEFAULT);
    fflush(stdout);
    setvbuf(stdout, NULL, _IONBF, 0);
    if (now_ms(&now) != 0 || MS08_UART_OVERALL_DEADLINE_MS > UINT64_MAX - now)
        return fail_case("setup", "clock");
    overall_deadline = now + MS08_UART_OVERALL_DEADLINE_MS;

#define MS08_PHASE() \
    do { \
        if (now_ms(&now) != 0) return fail_case("setup", "clock"); \
        deadline = now + MS08_UART_PHASE_DEADLINE_MS; \
        if (deadline > overall_deadline) deadline = overall_deadline; \
    } while (0)

    printf("MS08_UART_CASE_START: placement\n");
    MS08_PHASE();
    if (run_placement(deadline) != 0) return 1;

    printf("MS08_UART_CASE_START: rx\n");
    MS08_PHASE();
    if (run_rx(deadline) != 0) return 1;

    printf("MS08_UART_CASE_START: tx-full-recovery\n");
    MS08_PHASE();
    if (run_tx_full_recovery(overall_deadline, &deadline) != 0) return 1;

    printf("MS08_UART_CASE_START: readiness\n");
    MS08_PHASE();
    if (run_readiness(deadline) != 0) return 1;

    printf("MS08_UART_CASE_START: tcdrain\n");
    MS08_PHASE();
    if (run_tcdrain(deadline) != 0) return 1;

    printf("MS08_UART_CASE_START: quiet\n");
    MS08_PHASE();
    if (run_quiet(overall_deadline) != 0) return 1;

    printf("MS08_UART_CASE_START: rx-migration\n");
    MS08_PHASE();
    if (run_copier_migration("rx-migration", 0, deadline) != 0) return 1;

    printf("MS08_UART_CASE_START: tx-migration\n");
    MS08_PHASE();
    if (run_copier_migration("tx-migration", 1, deadline) != 0) return 1;

#undef MS08_PHASE

    printf("MS08_UART_END\n");
    fflush(stdout);
    return 0;
}

int main(int argc, char **argv)
{
    if (argc == 2 && strcmp(argv[1], "--print-cases") == 0) {
        for (unsigned i = 0; i < ms08_uart_case_count(); ++i)
            puts(ms08_uart_cases[i]);
        return 0;
    }
    if (argc == 2 && strcmp(argv[1], "--print-schema") == 0) {
        for (unsigned i = 0; i < ms08_uart_schema_count(); ++i)
            puts(ms08_uart_schema[i]);
        return 0;
    }
    if (argc == 2 && strcmp(argv[1], "--self-test") == 0)
        return ms08_uart_probe_self_test() ? 0 : 1;
    if (argc == 2 && strcmp(argv[1], "--run") == 0)
        return run_probe();
    fprintf(stderr, "usage: %s --print-cases | --print-schema | --self-test | --run\n",
            argv[0]);
    return 2;
}

#endif /* MS08_UART_PROBE_TESTING */
