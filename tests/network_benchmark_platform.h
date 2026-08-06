/* MS16 network benchmark — platform measurement adapter.
 *
 * Provides monotonic clock, counter parsing, instret, IRQ snapshot
 * and capability queries. Host builds expose unavailable guest counters
 * as typed results, never as zero.
 *
 * C11 + musl + linux compatible. No external dependencies.
 */
#ifndef NETWORK_BENCHMARK_PLATFORM_H
#define NETWORK_BENCHMARK_PLATFORM_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

#define NB_UNAVAILABLE (-1)

/* ── monotonic clock ─────────────────────────────────────────────────── */

uint64_t nb_monotonic_ns(void);
void     nb_nanosleep(uint64_t ns);

/* ── strict u64 parser ───────────────────────────────────────────────── */

int nb_parse_u64(const char *s, uint64_t *out);

/* ── instret (guest-only on RISC-V; host returns unavailable) ─────────── */

struct nb_instret_result {
    int      available;   /* 1 = valid, 0 = unavailable */
    uint64_t begin;
    uint64_t end;
    uint64_t overhead;    /* cost of two consecutive reads */
};

int  nb_instret_read(struct nb_instret_result *r);
int  nb_instret_result_valid(const struct nb_instret_result *r);

/* ── IRQ snapshot (guest-only; host returns unavailable) ──────────────── */

struct nb_irq_snapshot {
    int      available;
    uint64_t total;
    uint64_t used_ring;
    uint64_t config_change;
    uint64_t combined;
    uint64_t unknown;
    uint64_t spurious;
    uint64_t ack_count;
    uint64_t uart_irq_count;
};

int nb_irq_snapshot_read(struct nb_irq_snapshot *s);

/* ── capability queries ──────────────────────────────────────────────── */

int nb_capability_monotonic(void);
int nb_capability_instret(void);
int nb_capability_irq_snapshot(void);

#ifdef __cplusplus
}
#endif

#endif /* NETWORK_BENCHMARK_PLATFORM_H */
