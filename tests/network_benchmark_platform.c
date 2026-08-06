#define _GNU_SOURCE
#define _POSIX_C_SOURCE 199309L

/* MS16 network benchmark — platform measurement adapter implementation.
 *
 * Host builds use standard POSIX/Linux APIs. Guest builds provide
 * RISC-V specific implementations when compiled with the musl toolchain.
 */
#include "network_benchmark_platform.h"
#include <time.h>
#include <string.h>
#include <stdio.h>
#include <stdlib.h>
#include <errno.h>
#include <limits.h>
#include <unistd.h>
#include <fcntl.h>
#include <sys/ioctl.h>

/* ── monotonic clock ─────────────────────────────────────────────────── */

uint64_t nb_monotonic_ns(void)
{
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) < 0) {
        return 0;
    }
    return (uint64_t)ts.tv_sec * 1000000000ULL + (uint64_t)ts.tv_nsec;
}

void nb_nanosleep(uint64_t ns)
{
    struct timespec ts;
    ts.tv_sec  = (time_t)(ns / 1000000000ULL);
    ts.tv_nsec = (long)(ns % 1000000000ULL);
    clock_nanosleep(CLOCK_MONOTONIC, 0, &ts, NULL);
}

/* ── strict u64 parser ───────────────────────────────────────────────── */

int nb_parse_u64(const char *s, uint64_t *out)
{
    if (!s || !out) return -1;
    if (s[0] == '\0') return -2;
    if (s[0] == '-') return -3;  /* no negative */

    char *end = NULL;
    errno = 0;
    unsigned long long val = strtoull(s, &end, 10);

    if (errno == ERANGE) return -4;
    if (end == s) return -5;          /* no digits converted */
    if (*end != '\0' && *end != '\n') return -6;  /* trailing non-whitespace */

    *out = (uint64_t)val;
    return 0;
}

/* ── instret (host: /proc/instret does not exist) ────────────────────── */

int nb_instret_read(struct nb_instret_result *r)
{
    memset(r, 0, sizeof(*r));
    r->available = 0;

#if defined(__riscv) && !defined(NB_HOST_BUILD)
    char buf[32];
    uint64_t sample[3];
    for (size_t i = 0; i < 3; i++) {
        int fd = open("/proc/instret", O_RDONLY);
        if (fd < 0) return -1;
        ssize_t n = read(fd, buf, sizeof(buf) - 1);
        close(fd);
        if (n <= 0) return -1;
        buf[n] = '\0';
        if (nb_parse_u64(buf, &sample[i]) < 0) return -1;
    }
    if (sample[1] < sample[0] || sample[2] < sample[1]) return -1;
    r->begin = sample[0];
    r->end = sample[1];
    r->overhead = sample[2] - sample[1];
    r->available = 1;
    return 0;
#else
    /* Host: instret is not available */
    (void)r;
    return NB_UNAVAILABLE;
#endif
}

int nb_instret_result_valid(const struct nb_instret_result *r)
{
    if (!r || !r->available) return -1;
    if (r->end < r->begin) return -2;
    if (r->begin > UINT64_MAX - r->overhead) return -3;  /* overflow check */
    if (r->end > UINT64_MAX - r->overhead) return -3;
    return 0;
}

/* ── IRQ snapshot ────────────────────────────────────────────────────── */

#if defined(__riscv) && !defined(NB_HOST_BUILD)
static const unsigned long NB_IOCTL_SNAPSHOT = 0x4e494431;
#endif

int nb_irq_snapshot_read(struct nb_irq_snapshot *s)
{
    memset(s, 0, sizeof(*s));
    s->available = 0;

#if defined(__riscv) && !defined(NB_HOST_BUILD)
    int fd = 0;  /* stdin — ioctl is dispatched by sys_ioctl */
    struct {
        uint64_t total;
        uint64_t used_ring;
        uint64_t config_change;
        uint64_t combined;
        uint64_t unknown;
        uint64_t spurious;
        uint64_t ack_count;
        uint64_t uart_irq_count;
    } raw;

    if (ioctl(fd, NB_IOCTL_SNAPSHOT, &raw) < 0) {
        s->available = 0;
        return -1;
    }

    s->total          = raw.total;
    s->used_ring      = raw.used_ring;
    s->config_change  = raw.config_change;
    s->combined       = raw.combined;
    s->unknown        = raw.unknown;
    s->spurious       = raw.spurious;
    s->ack_count      = raw.ack_count;
    s->uart_irq_count = raw.uart_irq_count;
    s->available      = 1;
    return 0;
#else
    (void)s;
    return NB_UNAVAILABLE;
#endif
}

/* ── capability queries ──────────────────────────────────────────────── */

int nb_capability_monotonic(void)
{
    struct timespec ts;
    return clock_gettime(CLOCK_MONOTONIC, &ts) == 0 ? 1 : 0;
}

int nb_capability_instret(void)
{
#if defined(__riscv) && !defined(NB_HOST_BUILD)
    return access("/proc/instret", R_OK) == 0 ? 1 : 0;
#else
    return 0;
#endif
}

int nb_capability_irq_snapshot(void)
{
#if defined(__riscv) && !defined(NB_HOST_BUILD)
    /* Only available when MS03 IRQ snapshot ioctl is wired */
    int fd = 0;
    uint64_t dummy[8];
    return ioctl(fd, NB_IOCTL_SNAPSHOT, dummy) == 0 ? 1 : 0;
#else
    return 0;
#endif
}
