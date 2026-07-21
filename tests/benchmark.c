/**
 * UART Async Benchmark — 真实串口性能测试
 *
 * 修复项 (O44):
 * - TX throughput 改为写 /dev/console（而非 /dev/null，绕过 UART）
 * - TX 延迟加 tcdrain() 等待硬件发送完成
 * - 新增非阻塞模式测试 (FIONBIO)
 *
 * 编译:
 *   riscv64-linux-musl-gcc -static -no-pie -fno-pie -Os -s \
 *     -DBENCH_TARGET_MODE='"qemu-rootfs"' \
 *     -DBENCH_STARTUP_CHAIN='"/bin/sh -c init.sh -> /bin/benchmark"' \
 *     -DBENCH_ROOT_PROVIDER='"qemu-virtio-ext4-rootfs"' \
 *     -o tests/benchmark tests/benchmark.c
 *
 * 运行:
 *   ./benchmark
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <time.h>
#include <stdint.h>
#include <sys/ioctl.h>
#include <sys/stat.h>
#include <sys/uio.h>
#include <sys/syscall.h>
#include <termios.h>
#include <errno.h>
#include <sys/sysmacros.h>
#ifndef major
#define major(dev) (((unsigned int)(dev)) >> 8)
#define minor(dev) (((unsigned int)(dev)) & 0xff)
#endif

#define DEVICE_PATH "/dev/console"
#define BUF_SIZE     1024
#define UART_LINE_RATE_KBPS 11.52

#define BENCH_VERSION "q31-cpu-efficiency-20260721"
#ifndef BENCH_TARGET_MODE
#define BENCH_TARGET_MODE "unspecified"
#endif
#ifndef BENCH_STARTUP_CHAIN
#define BENCH_STARTUP_CHAIN "unspecified"
#endif
#ifndef BENCH_ROOT_PROVIDER
#define BENCH_ROOT_PROVIDER "unspecified"
#endif
#ifndef BENCH_HART_COUNT
#define BENCH_HART_COUNT "not-available"
#endif
#ifndef BENCH_SOURCE_REVISION
#define BENCH_SOURCE_REVISION "not-available"
#endif
#ifndef BENCH_SOURCE_DIRTY
#define BENCH_SOURCE_DIRTY "not-available"
#endif

#define TX_THROUGHPUT_ITERS 100
#define TX_LATENCY_ITERS 100
#define FIFO_MATRIX_ITERS 100
#define TX_BATCH_DRAIN_EVERY 8
#define TX_WRITEV_FRAGMENTS 4
#define TX_WRITEV_FRAGMENT_SIZE 64
#define RX_FIXED_TIMEOUT_MS 5000
#define TX_SLOW_ITER_NS 10000000LL

#ifndef BENCH_RX_FIXED_BYTES
#define BENCH_RX_FIXED_BYTES 0
#endif

#define S41_ROUNDS          5
#define S42_PAYLOAD_BYTES  64
#define S42_PAYLOAD_ITERS  100
#define S42_WARMUP_ROUNDS  1
#define S42_SAMPLE_ROUNDS  5

#define S43_GROUPS          5
#define S43_SAMPLES         50
#define S43_INTERVAL_US     5000
#define S43_TX_BURST_BYTES  4096

#ifndef SYS_clock_nanosleep
#define SYS_clock_nanosleep 115
#endif

static const int TX_THROUGHPUT_SIZES[] = {64, 256, 1024};
static const int TX_BREAK_EVEN_SIZES[] = {64, 128, 256};
static const int FIFO_MATRIX_SIZES[] = {1, 15, 16, 17, 31, 32, 33, 48, 49};

typedef struct {
    int calls;
    int errors;
    int first_errno;
    int last_errno;
} drain_stats_t;

typedef struct {
    size_t completed_bytes;
    int logical_writes;
    int syscall_calls;
    int partial_syscalls;
    int zero_progress_retries;
    int incomplete_logical_writes;
    int first_errno;
    int last_errno;
    int timeout;
} counted_write_stats_t;

#define UART_TXDBG_SNAPSHOT 0x54584431U
#define UART_TXDBG_RESET    0x54584432U

typedef struct {
    uint64_t user_push_calls;
    uint64_t user_push_requested_bytes;
    uint64_t user_push_accepted_bytes;
    uint64_t ring_pop_calls;
    uint64_t ring_pop_bytes;
    uint64_t hw_send_calls;
    uint64_t hw_send_bytes;
    uint64_t hw_send_zero;
    uint64_t hw_send_max_chunk;
    uint64_t no_progress_budget_exhausted;
    uint64_t slow_poll_exhausted;
    uint64_t yield_retries_exhausted;
    uint64_t ring_empty;
    uint64_t copier_active;
    uint64_t staged_bytes;
    uint64_t transmitter_empty;
} txdbg_snapshot_t;

static long long get_time_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (long long)ts.tv_sec * 1000000000LL + ts.tv_nsec;
}

static void print_int_list(const int *values, int count) {
    for (int i = 0; i < count; i++) {
        if (i > 0) printf(",");
        printf("%d", values[i]);
    }
}

static long elapsed_ms(long long start, long long end) {
    return (long)((end - start) / 1000000LL);
}

static double line_time_ms(size_t bytes) {
    return (double)bytes / (UART_LINE_RATE_KBPS * 1024.0) * 1000.0;
}

static int checked_tcdrain(int fd, drain_stats_t *stats) {
    errno = 0;
    int rc = tcdrain(fd);
    if (stats) {
        stats->calls++;
        if (rc < 0) {
            stats->errors++;
            if (stats->first_errno == 0) stats->first_errno = errno;
            stats->last_errno = errno;
        }
    }
    return rc;
}

static int txdbg_reset(int fd) {
    return ioctl(fd, UART_TXDBG_RESET, 0);
}

static int txdbg_snapshot(int fd, txdbg_snapshot_t *snapshot) {
    memset(snapshot, 0, sizeof(*snapshot));
    return ioctl(fd, UART_TXDBG_SNAPSHOT, snapshot);
}

static void print_txdbg_snapshot(const char *phase, int size, const txdbg_snapshot_t *s, int rc) {
    printf("  diag=s11-txdbg phase=%s size=%d ioctl_rc=%d user_calls=%llu user_req=%llu user_acc=%llu ring_pop_calls=%llu ring_pop_bytes=%llu hw_send_calls=%llu hw_send_bytes=%llu hw_send_zero=%llu hw_send_max_chunk=%llu no_progress_budget=%llu slow_poll_exh=%llu yield_exh=%llu ring_empty=%llu copier_active=%llu staged_bytes=%llu transmitter_empty=%llu\r\n",
           phase, size, rc,
           (unsigned long long)s->user_push_calls,
           (unsigned long long)s->user_push_requested_bytes,
           (unsigned long long)s->user_push_accepted_bytes,
           (unsigned long long)s->ring_pop_calls,
           (unsigned long long)s->ring_pop_bytes,
           (unsigned long long)s->hw_send_calls,
           (unsigned long long)s->hw_send_bytes,
           (unsigned long long)s->hw_send_zero,
           (unsigned long long)s->hw_send_max_chunk,
           (unsigned long long)s->no_progress_budget_exhausted,
           (unsigned long long)s->slow_poll_exhausted,
           (unsigned long long)s->yield_retries_exhausted,
           (unsigned long long)s->ring_empty,
           (unsigned long long)s->copier_active,
           (unsigned long long)s->staged_bytes,
           (unsigned long long)s->transmitter_empty);
}

static void sort_longs(long *values, int count) {
    for (int i = 0; i < count - 1; i++) {
        for (int j = 0; j < count - i - 1; j++) {
            if (values[j] > values[j + 1]) {
                long t = values[j];
                values[j] = values[j + 1];
                values[j + 1] = t;
            }
        }
    }
}

static void sort_doubles(double *values, int count) {
    for (int i = 0; i < count - 1; i++) {
        for (int j = 0; j < count - i - 1; j++) {
            if (values[j] > values[j + 1]) {
                double t = values[j];
                values[j] = values[j + 1];
                values[j + 1] = t;
            }
        }
    }
}

static void print_latency_summary(const char *prefix, long *latencies, int count) {
    if (count == 0) {
        printf("  %s status=no-data\r\n\r\n", prefix);
        return;
    }

    sort_longs(latencies, count);

    long sum = 0;
    for (int i = 0; i < count; i++) sum += latencies[i];

    printf("  %s n=%d avg_ms=%.3f p50_ms=%.3f p95_ms=%.3f p99_ms=%.3f\r\n",
           prefix, count,
           (double)sum / count / 1000000.0,
           (double)latencies[count * 50 / 100] / 1000000.0,
           (double)latencies[count * 95 / 100] / 1000000.0,
           (double)latencies[count * 99 / 100] / 1000000.0);
}

static void print_tx_latency_diag(const char *prefix, long *latencies, int count, int payload_size) {
    if (count == 0) {
        printf("  diag=%s latency_status=no-data\r\n", prefix);
        return;
    }

    sort_longs(latencies, count);

    long sum = 0;
    int slow = 0;
    int slow_over_line_plus10ms = 0;
    long long line_ns = (long long)(line_time_ms((size_t)payload_size) * 1000000.0);
    long long line_plus_10ms_ns = line_ns + TX_SLOW_ITER_NS;
    for (int i = 0; i < count; i++) {
        sum += latencies[i];
        if (latencies[i] > TX_SLOW_ITER_NS) slow++;
        if ((long long)latencies[i] > line_plus_10ms_ns) slow_over_line_plus10ms++;
    }
    double max_line_ratio = line_ns > 0 ? (double)latencies[count - 1] / (double)line_ns : 0.0;
    long p50_ns = latencies[count * 50 / 100];
    double p99_p50_ratio = p50_ns > 0 ? (double)latencies[count * 99 / 100] / (double)p50_ns : 0.0;
    double max_p50_ratio = p50_ns > 0 ? (double)latencies[count - 1] / (double)p50_ns : 0.0;

    printf("  diag=%s n=%d avg_ms=%.3f p50_ms=%.3f p95_ms=%.3f p99_ms=%.3f max_ms=%.3f slow_gt10ms=%d slow_over_line_plus10ms=%d max_line_ratio=%.2f p99_p50_ratio=%.2f max_p50_ratio=%.2f\r\n",
           prefix, count,
           (double)sum / count / 1000000.0,
           (double)p50_ns / 1000000.0,
           (double)latencies[count * 95 / 100] / 1000000.0,
           (double)latencies[count * 99 / 100] / 1000000.0,
           (double)latencies[count - 1] / 1000000.0,
           slow, slow_over_line_plus10ms, max_line_ratio,
           p99_p50_ratio, max_p50_ratio);
}

static void prepare_section(const char *section) {
    long long start = get_time_ns();
    drain_stats_t stats = {0};
    fflush(stdout);
    checked_tcdrain(STDOUT_FILENO, &stats);
    long long end = get_time_ns();
    printf("  diag=%s pre_section_stdout_drain_ms=%ld drain_errors=%d last_errno=%d\r\n",
           section, elapsed_ms(start, end), stats.errors, stats.last_errno);
    fflush(stdout);
    checked_tcdrain(STDOUT_FILENO, &stats);
}

static ssize_t write_full(int fd, const char *buf, size_t len) {
    size_t written = 0;
    while (written < len) {
        ssize_t n = write(fd, buf + written, len - written);
        if (n <= 0) {
            return written > 0 ? (ssize_t)written : n;
        }
        written += (size_t)n;
    }
    return (ssize_t)written;
}

static size_t run_write_drain_iters(int fd, const char *buf, int size, int iters,
                                    long *latencies, int *ok, int *short_writes,
                                    drain_stats_t *drain_stats) {
    size_t total = 0;
    *ok = 0;
    *short_writes = 0;

    for (int i = 0; i < iters; i++) {
        long long iter_start = get_time_ns();
        ssize_t n = write_full(fd, buf, (size_t)size);
        if (n > 0) total += (size_t)n;
        if (n != size) (*short_writes)++;
        checked_tcdrain(fd, drain_stats);
        long long iter_end = get_time_ns();
        latencies[(*ok)++] = (long)(iter_end - iter_start);
    }

    return total;
}

/* ── strict instret reader with reason codes ────────────────────────────────
 * Returns status code:
 *   3 = OK
 *   2 = parse_overflow (UINT64_MAX)
 *   1 = counter_regression
 *   0 = open/read/parse error
 *  -1 = not_attempted
 */
static int read_instret_strict(uint64_t *value, const char **reason) {
    *value = 0;
    *reason = "not_attempted";
    int fd = open("/proc/instret", O_RDONLY);
    if (fd < 0) { *reason = "open_failed"; return 0; }
    char buf[32] = {0};
    ssize_t n = read(fd, buf, sizeof(buf) - 1);
    close(fd);
    if (n <= 0) { *reason = "read_failed"; return 0; }
    if (n >= (ssize_t)sizeof(buf) - 1) { *reason = "buffer_overflow"; return 0; }
    buf[n] = '\0';
    /* strip trailing newline / spaces */
    while (n > 0 && (buf[n-1] == '\n' || buf[n-1] == '\r' || buf[n-1] == ' ')) {
        buf[--n] = '\0';
    }
    if (n == 0) { *reason = "parse_empty"; return 0; }
    /* check all chars are digits */
    for (int i = 0; i < (int)n; i++) {
        if (buf[i] < '0' || buf[i] > '9') { *reason = "parse_non_digit"; *value = 0; return 0; }
    }
    char *end;
    errno = 0;
    unsigned long long val = strtoull(buf, &end, 10);
    if (end != buf + n) { *reason = "parse_trailing"; return 0; }
    if (errno == ERANGE) { *reason = "parse_overflow"; *value = UINT64_MAX; return 2; }
    *value = val;
    *reason = "ok";
    return 3;
}

/* ── instret overhead reporter (uses strict reader) ───────────────────────── */
static void report_instret_overhead(void) {
    const char *r1, *r2;
    uint64_t a, b;
    int s1 = read_instret_strict(&a, &r1);
    int s2 = read_instret_strict(&b, &r2);
    if (s1 == 3 && s2 == 3 && b >= a) {
        printf("  instret_read_overhead=%llu\r\n", (unsigned long long)(b - a));
    } else {
        printf("  instret_read_overhead=not-available reason1=%s status1=%d reason2=%s status2=%d\r\n",
               r1, s1, r2, s2);
    }
}

/* ── fixed compute kernel ─────────────────────────────────────────────────── */
static unsigned long long fixed_compute(long long deadline_ns) {
    volatile unsigned long long sink = 0;
    unsigned long long iters = 0;
    while (get_time_ns() < deadline_ns) {
        sink ^= (unsigned long long)(iters * 0x9e3779b97f4a7c15ULL);
        iters++;
    }
    (void)sink;
    return iters;
}

/* ── absolute sleep sample collector ──────────────────────────────────────── */
static int collect_abs_sleep_samples(long *overshoot_ns, int count,
                                      long long base_ns, long long interval_ns) {
    int collected = 0;
    for (int i = 0; i < count; i++) {
        long long deadline = base_ns + (long long)(i + 1) * interval_ns;
        struct timespec req = { .tv_sec = deadline / 1000000000LL,
                                 .tv_nsec = deadline % 1000000000LL };
        int rc = syscall(SYS_clock_nanosleep, CLOCK_MONOTONIC, TIMER_ABSTIME, &req, NULL);
        long long after = get_time_ns();
        if (rc < 0) {
            overshoot_ns[collected] = -(long)errno;
        } else {
            overshoot_ns[collected] = (long)(after - deadline);
        }
        collected++;
    }
    return collected;
}

/* ── timer stats printer ──────────────────────────────────────────────────── */
static void print_timer_stats(const char *label, long *samples, int count) {
    int errors = 0;
    int valid_count = 0;
    for (int i = 0; i < count; i++) {
        if (samples[i] < 0) errors++;
        else samples[valid_count++] = samples[i];
    }
    if (valid_count == 0) {
        printf("  %s status=no-valid-samples errors=%d\r\n", label, errors);
        return;
    }
    sort_longs(samples, valid_count);
    int p50_idx = valid_count * 50 / 100;
    int p95_idx = valid_count * 95 / 100;
    int p99_idx = valid_count * 99 / 100;
    printf("  %s n=%d errors=%d p50_ns=%ld p95_ns=%ld p99_ns=%ld max_ns=%ld\r\n",
           label, valid_count, errors,
           samples[p50_idx], samples[p95_idx], samples[p99_idx],
           samples[valid_count - 1]);
}

/* ── counted full-write helper with deadline ──────────────────────────────── */
static size_t counted_write_full(int fd, const char *buf, size_t size,
                                  counted_write_stats_t *stats,
                                  long long deadline_ns) {
    memset(stats, 0, sizeof(*stats));
    size_t total = 0;
    stats->logical_writes = 1;

    while (total < size) {
        if (deadline_ns > 0 && get_time_ns() >= deadline_ns) {
            stats->timeout = 1;
            break;
        }
        ssize_t n = write(fd, buf + total, size - total);
        stats->syscall_calls++;
        if (n > 0) {
            if ((size_t)n < (size - total)) stats->partial_syscalls++;
            total += (size_t)n;
        } else if (n == 0) {
            stats->zero_progress_retries++;
        } else {
            if (errno == EAGAIN || errno == EWOULDBLOCK) {
                stats->zero_progress_retries++;
            } else {
                if (stats->first_errno == 0) stats->first_errno = errno;
                stats->last_errno = errno;
                break;
            }
        }
    }

    if (total < size) stats->incomplete_logical_writes++;
    stats->completed_bytes = total;
    return total;
}

/* ── workload-local TX counter helper ─────────────────────────────────────── */
static void print_workload_tx_counters(int fd, const char *section,
                                        int completed_bytes,
                                        int reset_rc) {
    if (fd < 0) {
        printf("  %s counters=not-available reason=no-fd\r\n", section);
        return;
    }
    txdbg_snapshot_t s;
    int snapshot_rc = txdbg_snapshot(fd, &s);
    if (snapshot_rc < 0) {
        printf("  %s counters=not-available reason=ioctl-failed errno=%d snapshot_rc=%d reset_rc=%d\r\n",
               section, errno, snapshot_rc, reset_rc);
        return;
    }
    double kb = completed_bytes > 0 ? (double)completed_bytes / 1024.0 : 0.0;
    if (kb <= 0.0) {
        printf("  %s counters=not-available reason=zero-bytes reset_rc=%d snapshot_rc=%d\r\n",
               section, reset_rc, snapshot_rc);
        return;
    }
    double ring_pop_calls_per_kb = (double)s.ring_pop_calls / kb;
    double zero_per_kb = (double)s.hw_send_zero / kb;
    double ring_pop_bytes_per_kb = (double)s.ring_pop_bytes / kb;
    double no_progress_per_kb = (double)s.no_progress_budget_exhausted / kb;
    double bytes_per_hw_send = s.hw_send_calls > 0 ? (double)s.hw_send_bytes / (double)s.hw_send_calls : 0.0;
    double bytes_per_ring_pop = s.ring_pop_calls > 0 ? (double)s.ring_pop_bytes / (double)s.ring_pop_calls : 0.0;
    printf("  %s counters=ok reset_rc=%d snapshot_rc=%d ring_pop_calls_per_kb=%.1f hw_send_zero_per_kb=%.1f ring_pop_bytes_per_kb=%.1f no_progress_per_kb=%.1f bytes_per_hw_send=%.3f bytes_per_ring_pop=%.1f hw_send_calls=%llu hw_send_bytes=%llu hw_send_zero=%llu ring_pop_calls=%llu ring_pop_bytes=%llu user_push_calls=%llu user_push_acc=%llu\r\n",
           section, reset_rc, snapshot_rc,
           ring_pop_calls_per_kb, zero_per_kb, ring_pop_bytes_per_kb, no_progress_per_kb,
           bytes_per_hw_send, bytes_per_ring_pop,
           (unsigned long long)s.hw_send_calls,
           (unsigned long long)s.hw_send_bytes,
           (unsigned long long)s.hw_send_zero,
           (unsigned long long)s.ring_pop_calls,
           (unsigned long long)s.ring_pop_bytes,
           (unsigned long long)s.user_push_calls,
           (unsigned long long)s.user_push_accepted_bytes);
}

static void print_manifest(void) {
    printf("=== [S00] Benchmark Manifest ===\r\n");
    printf("  version=%s\r\n", BENCH_VERSION);
    printf("  target_mode=%s\r\n", BENCH_TARGET_MODE);
    printf("  startup_chain=%s\r\n", BENCH_STARTUP_CHAIN);
    printf("  root_provider=%s\r\n", BENCH_ROOT_PROVIDER);
    printf("  device=%s\r\n", DEVICE_PATH);
    printf("  timer_source=CLOCK_MONOTONIC\r\n");
    printf("  uart_line_rate=%.2f KB/s\r\n", UART_LINE_RATE_KBPS);
    printf("  tx_throughput_sizes=");
    print_int_list(TX_THROUGHPUT_SIZES, sizeof(TX_THROUGHPUT_SIZES) / sizeof(TX_THROUGHPUT_SIZES[0]));
    printf("\r\n");
    printf("  tx_break_even_sizes=");
    print_int_list(TX_BREAK_EVEN_SIZES, sizeof(TX_BREAK_EVEN_SIZES) / sizeof(TX_BREAK_EVEN_SIZES[0]));
    printf("\r\n");
    printf("  tx_throughput_iters=%d\r\n", TX_THROUGHPUT_ITERS);
    printf("  tx_baseline_drain_policy=tcdrain-after-each-write\r\n");
    printf("  tx_enqueue_policy=no-drain-during-measure-final-tcdrain-after\r\n");
    printf("  tx_batch_drain_every=%d\r\n", TX_BATCH_DRAIN_EVERY);
    printf("  tx_writev_fragments=%d\r\n", TX_WRITEV_FRAGMENTS);
    printf("  tx_writev_fragment_size=%d\r\n", TX_WRITEV_FRAGMENT_SIZE);
    printf("  tx_latency_size=1\r\n");
    printf("  tx_latency_iters=%d\r\n", TX_LATENCY_ITERS);
    printf("  fifo_matrix_sizes=");
    print_int_list(FIFO_MATRIX_SIZES, sizeof(FIFO_MATRIX_SIZES) / sizeof(FIFO_MATRIX_SIZES[0]));
    printf("\r\n");
    printf("  fifo_matrix_iters=%d\r\n", FIFO_MATRIX_ITERS);
    printf("  rx_mode=empty-nonblocking-eagain\r\n");
    printf("  rx_fixed_bytes=%d\r\n", BENCH_RX_FIXED_BYTES);
    printf("  rx_fixed_timeout_ms=%d\r\n", RX_FIXED_TIMEOUT_MS);
    printf("  hart_count=%s\r\n", BENCH_HART_COUNT);
    {
        int cons_fd = open(DEVICE_PATH, O_RDONLY);
        if (cons_fd >= 0) {
            struct stat st;
            if (fstat(cons_fd, &st) == 0) {
                printf("  fstat_dev=major=%lu minor=%lu\r\n",
                       (unsigned long)major(st.st_rdev),
                       (unsigned long)minor(st.st_rdev));
            } else {
                printf("  fstat_dev=not-available reason=fstat-failed errno=%d\r\n", errno);
            }
            close(cons_fd);
        } else {
            printf("  fstat_dev=not-available reason=open-failed errno=%d\r\n", errno);
        }
    }
    printf("  source_revision=%s\r\n", BENCH_SOURCE_REVISION);
    printf("  source_dirty=%s\r\n", BENCH_SOURCE_DIRTY);
    printf("  timer_source_detail=CLOCK_MONOTONIC\r\n");
    printf("  clock_nanosleep_available=yes\r\n");
    printf("  instret_source=/proc/instret\r\n");
    printf("  bench_version_extra=q31-cpu-efficiency\r\n");
    printf("\r\n");
}

/* ── TX throughput: 写 /dev/console + tcdrain ─────────────────────────────── */
static void test_tx_throughput(void) {
    printf("=== [S10] TX Throughput Baseline (write + tcdrain each iteration) ===\r\n");
    prepare_section("S10");

    int fd = open(DEVICE_PATH, O_WRONLY);
    if (fd < 0) { perror("open"); return; }

    int num_sizes = sizeof(TX_THROUGHPUT_SIZES) / sizeof(TX_THROUGHPUT_SIZES[0]);

    for (int s = 0; s < num_sizes; s++) {
        int test_size = TX_THROUGHPUT_SIZES[s];
        int iterations = TX_THROUGHPUT_ITERS;
        char *buf = malloc(test_size);
        if (!buf) { perror("malloc"); continue; }
        memset(buf, 0, test_size);

        long long start = get_time_ns();
        long latencies[TX_THROUGHPUT_ITERS];
        int ok = 0;
        int short_writes = 0;
        drain_stats_t drain_stats = {0};
        size_t total = run_write_drain_iters(
            fd, buf, test_size, iterations, latencies, &ok, &short_writes,
            &drain_stats);

        long long end = get_time_ns();
        double elapsed_s = (double)(end - start) / 1000000000.0;
        double kbps = (double)total / elapsed_s / 1024.0;
        double line_rate = kbps / UART_LINE_RATE_KBPS * 100.0;

        printf("  policy=drain-each size=%d iters=%d bytes=%zu short_writes=%d drain_calls=%d drain_errors=%d last_drain_errno=%d elapsed_ms=%ld line_time_ms=%.1f kbps=%.2f line_rate_pct=%.1f\r\n",
               test_size, iterations, total, short_writes, drain_stats.calls,
               drain_stats.errors, drain_stats.last_errno, elapsed_ms(start, end),
               line_time_ms(total), kbps, line_rate);
        char diag_prefix[64];
        snprintf(diag_prefix, sizeof(diag_prefix), "drain-each-size-%d", test_size);
        print_tx_latency_diag(diag_prefix, latencies, ok, test_size);

        free(buf);
    }

    close(fd);
    printf("\r\n");
}

/* ── TX enqueue: write without per-iteration drain, final tcdrain outside timing ── */
static void test_tx_enqueue_no_drain(void) {
    printf("=== [S11] TX Enqueue Cost (write loop, final drain outside timing) ===\r\n");
    prepare_section("S11");

    int fd = open(DEVICE_PATH, O_WRONLY);
    if (fd < 0) { perror("open"); return; }

    int num_sizes = sizeof(TX_THROUGHPUT_SIZES) / sizeof(TX_THROUGHPUT_SIZES[0]);

    for (int s = 0; s < num_sizes; s++) {
        int test_size = TX_THROUGHPUT_SIZES[s];
        char *buf = malloc(test_size);
        if (!buf) { perror("malloc"); continue; }
        memset(buf, 0, test_size);

        size_t total = 0;
        int short_writes = 0;
        drain_stats_t drain_stats = {0};
        txdbg_snapshot_t txdbg_enqueue;
        txdbg_snapshot_t txdbg_final;
        fflush(stdout);
        checked_tcdrain(STDOUT_FILENO, NULL);
        int txdbg_reset_rc = txdbg_reset(fd);
        long long start = get_time_ns();

        for (int i = 0; i < TX_THROUGHPUT_ITERS; i++) {
            ssize_t n = write_full(fd, buf, (size_t)test_size);
            if (n > 0) total += (size_t)n;
            if (n != test_size) short_writes++;
        }

        long long enqueue_end = get_time_ns();
        int txdbg_enqueue_rc = txdbg_snapshot(fd, &txdbg_enqueue);
        int final_drain_rc = checked_tcdrain(fd, &drain_stats);
        int final_drain_errno = final_drain_rc < 0 ? errno : 0;
        long long drain_end = get_time_ns();
        int txdbg_final_rc = txdbg_snapshot(fd, &txdbg_final);

        double elapsed_s = (double)(enqueue_end - start) / 1000000000.0;
        double kbps = elapsed_s > 0.0 ? (double)total / elapsed_s / 1024.0 : 0.0;

        printf("  policy=no-drain size=%d iters=%d bytes=%zu short_writes=%d enqueue_ms=%ld final_drain_ms=%ld final_drain_rc=%d final_drain_errno=%d drain_calls=%d drain_errors=%d last_drain_errno=%d line_time_ms=%.1f enqueue_kbps=%.2f\r\n",
               test_size, TX_THROUGHPUT_ITERS, total, short_writes,
               elapsed_ms(start, enqueue_end), elapsed_ms(enqueue_end, drain_end),
               final_drain_rc, final_drain_errno,
               drain_stats.calls, drain_stats.errors, drain_stats.last_errno,
               line_time_ms(total), kbps);
        printf("  diag=s11-txdbg-reset size=%d ioctl_rc=%d\r\n", test_size, txdbg_reset_rc);
        print_txdbg_snapshot("enqueue", test_size, &txdbg_enqueue, txdbg_enqueue_rc);
        print_txdbg_snapshot("final-drain", test_size, &txdbg_final, txdbg_final_rc);

        long long total_time_ns = drain_end - start;
        long long enqueue_time_ns = enqueue_end - start;
        if (total_time_ns <= 0) {
            printf("  diag=s11-derived size=%d producer_available=not-available reason=zero-total-time\r\n", test_size);
        } else {
            double submit_fraction = (double)enqueue_time_ns / (double)total_time_ns;
            double producer_available = 1.0 - submit_fraction;
            printf("  diag=s11-derived size=%d submit_fraction=%.4f producer_available=%.4f total_time_ms=%ld enqueue_time_ms=%ld\r\n",
                   test_size, submit_fraction, producer_available,
                   (long)(total_time_ns / 1000000LL),
                   (long)(enqueue_time_ns / 1000000LL));
        }

        free(buf);
    }

    close(fd);
    printf("\r\n");
}

/* ── TX batch drain: amortize tcdrain overhead while preserving physical drain ── */
static void test_tx_batch_drain(void) {
    printf("=== [S12] TX Batch Drain (write N iterations, then tcdrain) ===\r\n");
    prepare_section("S12");

    int fd = open(DEVICE_PATH, O_WRONLY);
    if (fd < 0) { perror("open"); return; }

    int num_sizes = sizeof(TX_THROUGHPUT_SIZES) / sizeof(TX_THROUGHPUT_SIZES[0]);

    for (int s = 0; s < num_sizes; s++) {
        int test_size = TX_THROUGHPUT_SIZES[s];
        char *buf = malloc(test_size);
        if (!buf) { perror("malloc"); continue; }
        memset(buf, 0, test_size);

        size_t total = 0;
        int drain_count = 0;
        drain_stats_t drain_stats = {0};
        long long start = get_time_ns();

        for (int i = 0; i < TX_THROUGHPUT_ITERS; i++) {
            ssize_t n = write_full(fd, buf, (size_t)test_size);
            if (n > 0) total += (size_t)n;

            if ((i + 1) % TX_BATCH_DRAIN_EVERY == 0) {
                checked_tcdrain(fd, &drain_stats);
                drain_count++;
            }
        }

        checked_tcdrain(fd, &drain_stats);
        drain_count++;

        long long end = get_time_ns();
        double elapsed_s = (double)(end - start) / 1000000000.0;
        double kbps = (double)total / elapsed_s / 1024.0;
        double line_rate = kbps / UART_LINE_RATE_KBPS * 100.0;

        printf("  policy=batch-drain size=%d iters=%d batch=%d drains=%d bytes=%zu drain_calls=%d drain_errors=%d last_drain_errno=%d elapsed_ms=%ld line_time_ms=%.1f kbps=%.2f line_rate_pct=%.1f\r\n",
               test_size, TX_THROUGHPUT_ITERS, TX_BATCH_DRAIN_EVERY,
               drain_count, total, drain_stats.calls, drain_stats.errors,
               drain_stats.last_errno, elapsed_ms(start, end),
               line_time_ms(total), kbps, line_rate);

        free(buf);
    }

    close(fd);
    printf("\r\n");
}

/* ── TX writev: syscall-side aggregation witness ───────────────────────────── */
static void test_tx_writev_fragments(void) {
    printf("=== [S13] TX writev Fragments (fragment aggregation witness) ===\r\n");
    prepare_section("S13");

    int fd = open(DEVICE_PATH, O_WRONLY);
    if (fd < 0) { perror("open"); return; }

    char *buf = malloc(TX_WRITEV_FRAGMENT_SIZE * TX_WRITEV_FRAGMENTS);
    if (!buf) {
        perror("malloc");
        close(fd);
        return;
    }
    memset(buf, 0, TX_WRITEV_FRAGMENT_SIZE * TX_WRITEV_FRAGMENTS);

    struct iovec iov[TX_WRITEV_FRAGMENTS];
    for (int i = 0; i < TX_WRITEV_FRAGMENTS; i++) {
        iov[i].iov_base = buf + i * TX_WRITEV_FRAGMENT_SIZE;
        iov[i].iov_len = TX_WRITEV_FRAGMENT_SIZE;
    }

    size_t total = 0;
    int short_writes = 0;
    drain_stats_t drain_stats = {0};
    long long start = get_time_ns();

    for (int i = 0; i < TX_THROUGHPUT_ITERS; i++) {
        ssize_t n = writev(fd, iov, TX_WRITEV_FRAGMENTS);
        if (n > 0) total += (size_t)n;
        if (n != TX_WRITEV_FRAGMENT_SIZE * TX_WRITEV_FRAGMENTS) short_writes++;
        checked_tcdrain(fd, &drain_stats);
    }

    long long end = get_time_ns();
    double elapsed_s = (double)(end - start) / 1000000000.0;
    double kbps = (double)total / elapsed_s / 1024.0;
    double line_rate = kbps / UART_LINE_RATE_KBPS * 100.0;

    printf("  policy=writev-drain-each fragments=%d fragment_size=%d total_size=%d iters=%d bytes=%zu short_writes=%d drain_calls=%d drain_errors=%d last_drain_errno=%d elapsed_ms=%ld line_time_ms=%.1f kbps=%.2f line_rate_pct=%.1f\r\n",
           TX_WRITEV_FRAGMENTS, TX_WRITEV_FRAGMENT_SIZE,
           TX_WRITEV_FRAGMENTS * TX_WRITEV_FRAGMENT_SIZE,
           TX_THROUGHPUT_ITERS, total, short_writes, drain_stats.calls,
           drain_stats.errors, drain_stats.last_errno, elapsed_ms(start, end),
           line_time_ms(total), kbps, line_rate);

    free(buf);
    close(fd);
    printf("\r\n");
}

/* ── TX small packet break-even: 64/128/256 with the baseline drain policy ── */
static void test_tx_small_packet_break_even(void) {
    printf("=== [S14] TX Small Packet Break-even (64/128/256 drain-each) ===\r\n");
    prepare_section("S14");

    int fd = open(DEVICE_PATH, O_WRONLY);
    if (fd < 0) { perror("open"); return; }

    int num_sizes = sizeof(TX_BREAK_EVEN_SIZES) / sizeof(TX_BREAK_EVEN_SIZES[0]);

    for (int s = 0; s < num_sizes; s++) {
        int test_size = TX_BREAK_EVEN_SIZES[s];
        char *buf = malloc(test_size);
        if (!buf) { perror("malloc"); continue; }
        memset(buf, 0, test_size);

        long long start = get_time_ns();
        long latencies[TX_THROUGHPUT_ITERS];
        int ok = 0;
        int short_writes = 0;
        drain_stats_t drain_stats = {0};
        size_t total = run_write_drain_iters(
            fd, buf, test_size, TX_THROUGHPUT_ITERS, latencies, &ok, &short_writes,
            &drain_stats);

        long long end = get_time_ns();
        double elapsed_s = (double)(end - start) / 1000000000.0;
        double kbps = (double)total / elapsed_s / 1024.0;
        double line_rate = kbps / UART_LINE_RATE_KBPS * 100.0;

        printf("  policy=drain-each size=%d iters=%d bytes=%zu short_writes=%d drain_calls=%d drain_errors=%d last_drain_errno=%d elapsed_ms=%ld line_time_ms=%.1f kbps=%.2f line_rate_pct=%.1f\r\n",
               test_size, TX_THROUGHPUT_ITERS, total, short_writes,
               drain_stats.calls, drain_stats.errors, drain_stats.last_errno,
               elapsed_ms(start, end), line_time_ms(total), kbps, line_rate);
        char diag_prefix[64];
        snprintf(diag_prefix, sizeof(diag_prefix), "break-even-size-%d", test_size);
        print_tx_latency_diag(diag_prefix, latencies, ok, test_size);

        free(buf);
    }

    close(fd);
    printf("\r\n");
}

/* ── TX latency: 单字节 write + tcdrain ─────────────────────────────────── */
static void test_tx_latency(void) {
    printf("=== [S20] TX Latency (single byte + tcdrain) ===\r\n");
    prepare_section("S20");

    int fd = open(DEVICE_PATH, O_WRONLY);
    if (fd < 0) { perror("open"); return; }

    long latencies[TX_LATENCY_ITERS];
    int ok = 0;
    drain_stats_t drain_stats = {0};

    for (int i = 0; i < TX_LATENCY_ITERS; i++) {
        char tx = 0;
        long long start = get_time_ns();
        if (write(fd, &tx, 1) != 1) continue;
        checked_tcdrain(fd, &drain_stats);
        long long end = get_time_ns();
        latencies[ok++] = (long)(end - start);
    }

    print_tx_latency_diag("s20-single-byte", latencies, ok, 1);
    printf("  diag=S20 drain_calls=%d drain_errors=%d last_drain_errno=%d\r\n",
           drain_stats.calls, drain_stats.errors, drain_stats.last_errno);
    printf("\r\n");

    close(fd);
}

/* ── TX latency FIFO boundary matrix ────────────────────────────────────── */
static void test_tx_latency_matrix(void) {
    printf("=== [S21] TX Latency FIFO Boundary Matrix ===\r\n");
    prepare_section("S21");

    int fd = open(DEVICE_PATH, O_WRONLY);
    if (fd < 0) { perror("open"); return; }

    int num_sizes = sizeof(FIFO_MATRIX_SIZES) / sizeof(FIFO_MATRIX_SIZES[0]);

    for (int s = 0; s < num_sizes; s++) {
        int sz = FIFO_MATRIX_SIZES[s];
        char *buf = malloc(sz);
        if (!buf) { perror("malloc"); continue; }
        memset(buf, 0, sz);

        long latencies[FIFO_MATRIX_ITERS];
        int ok = 0;
        drain_stats_t drain_stats = {0};

        for (int i = 0; i < FIFO_MATRIX_ITERS; i++) {
            long long start = get_time_ns();
            ssize_t n = write(fd, buf, sz);
            if (n != sz) continue;
            checked_tcdrain(fd, &drain_stats);
            long long end = get_time_ns();
            latencies[ok++] = (long)(end - start);
        }

        free(buf);

        char prefix[64];
        snprintf(prefix, sizeof(prefix), "s21-fifo-size-%d", sz);
        print_tx_latency_diag(prefix, latencies, ok, sz);
        printf("  diag=fifo-size-%d drain_calls=%d drain_errors=%d last_drain_errno=%d\r\n",
               sz, drain_stats.calls, drain_stats.errors, drain_stats.last_errno);
    }

    close(fd);
    printf("\r\n");
}

/* ── non-blocking read test (FIONBIO) ───────────────────────────────────── */
static void test_nonblock_read(void) {
    printf("=== [S30] RX Empty Non-blocking Read (FIONBIO) ===\r\n");
    prepare_section("S30");

    int fd = open(DEVICE_PATH, O_RDWR | O_NONBLOCK);
    if (fd < 0) { perror("open"); return; }

    char buf[16];
    ssize_t n = read(fd, buf, sizeof(buf));

    if (n == -1 && (errno == EAGAIN || errno == EWOULDBLOCK)) {
        printf("  method=open-o-nonblock status=PASS result=EAGAIN\r\n");
    } else if (n >= 0) {
        printf("  method=open-o-nonblock status=INFO bytes=%zd result=data-already-buffered\r\n", n);
    } else {
        printf("  method=open-o-nonblock status=FAIL errno=%d error=%s\r\n", errno, strerror(errno));
    }

    close(fd);

    /* test via ioctl */
    fd = open(DEVICE_PATH, O_RDWR);
    if (fd < 0) { perror("open"); return; }

    int on = 1;
    if (ioctl(fd, FIONBIO, &on) < 0) {
        printf("  FAIL: ioctl FIONBIO: %s\r\n", strerror(errno));
        close(fd);
        return;
    }

    n = read(fd, buf, sizeof(buf));
    if (n == -1 && (errno == EAGAIN || errno == EWOULDBLOCK)) {
        printf("  method=ioctl-fionbio status=PASS result=EAGAIN\r\n");
    } else if (n >= 0) {
        printf("  method=ioctl-fionbio status=INFO bytes=%zd result=data-already-buffered\r\n", n);
    } else {
        printf("  method=ioctl-fionbio status=FAIL errno=%d error=%s\r\n", errno, strerror(errno));
    }

    close(fd);
    printf("\r\n");
}

/* ── optional RX fixed-payload witness ───────────────────────────────────── */
static void test_rx_fixed_payload(void) {
    printf("=== [S31] RX Fixed Payload Witness ===\r\n");
    prepare_section("S31");

    if (BENCH_RX_FIXED_BYTES <= 0) {
        printf("  status=SKIPPED reason=BENCH_RX_FIXED_BYTES=0\r\n\r\n");
        return;
    }

    int fd = open(DEVICE_PATH, O_RDONLY | O_NONBLOCK);
    if (fd < 0) { perror("open"); return; }

    char buf[64];
    int received = 0;
    int reads = 0;
    long long start = get_time_ns();
    long long deadline = start + (long long)RX_FIXED_TIMEOUT_MS * 1000000LL;

    while (received < BENCH_RX_FIXED_BYTES && get_time_ns() < deadline) {
        int want = BENCH_RX_FIXED_BYTES - received;
        if (want > (int)sizeof(buf)) want = (int)sizeof(buf);

        ssize_t n = read(fd, buf, (size_t)want);
        if (n > 0) {
            received += (int)n;
            reads++;
        } else if (n < 0 && errno != EAGAIN && errno != EWOULDBLOCK) {
            printf("  status=FAIL errno=%d error=%s received=%d\r\n\r\n",
                   errno, strerror(errno), received);
            close(fd);
            return;
        }
    }

    long long end = get_time_ns();
    double elapsed_s = (double)(end - start) / 1000000000.0;
    double kbps = elapsed_s > 0.0 ? (double)received / elapsed_s / 1024.0 : 0.0;
    const char *status = received >= BENCH_RX_FIXED_BYTES ? "PASS" : "SKIPPED";

    printf("  status=%s target_bytes=%d received=%d reads=%d elapsed_ms=%ld rx_kbps=%.2f\r\n\r\n",
           status, BENCH_RX_FIXED_BYTES, received, reads, elapsed_ms(start, end), kbps);

    close(fd);
}

/* ── S41: TX CPU Work (5 rounds, strict completion) ──────────────────────── */
static void test_tx_cpu_work(void) {
    printf("=== [S41] TX CPU Work (instret: write start → final TEMT drain, %d rounds) ===\r\n", S41_ROUNDS);
    prepare_section("S41");

    int fd = open(DEVICE_PATH, O_WRONLY);
    if (fd < 0) { perror("open"); return; }

    /* instret overhead measured once before all rounds */
    report_instret_overhead();

    int num_sizes = sizeof(TX_THROUGHPUT_SIZES) / sizeof(TX_THROUGHPUT_SIZES[0]);

    for (int s = 0; s < num_sizes; s++) {
        int test_size = TX_THROUGHPUT_SIZES[s];
        char *buf = malloc(test_size);
        if (!buf) { perror("malloc"); continue; }
        memset(buf, 0, test_size);

        size_t expected_bytes = (size_t)test_size * TX_THROUGHPUT_ITERS;
        double inst_per_byte_arr[S41_ROUNDS];
        double inst_per_write_arr[S41_ROUNDS];
        int valid_rounds = 0;
        int last_reset_rc = 0;

        printf("  s41-size=%d expected_bytes=%zu iters=%d\r\n", test_size, expected_bytes, TX_THROUGHPUT_ITERS);

        for (int round = 0; round < S41_ROUNDS; round++) {
            fflush(stdout);
            checked_tcdrain(STDOUT_FILENO, NULL);
            int reset_rc = txdbg_reset(fd);
            last_reset_rc = reset_rc;

            /* ── instret begin (strict) ── */
            const char *begin_reason;
            uint64_t instret_begin;
            int begin_status = read_instret_strict(&instret_begin, &begin_reason);

            /* ── measured writes ── */
            long long t_start = get_time_ns();
            size_t total_completed = 0;
            int total_syscall_calls = 0;
            int incomplete_logical = 0;

            long long round_line_ns = (long long)((double)(test_size * TX_THROUGHPUT_ITERS) / (UART_LINE_RATE_KBPS * 1024.0) * 1e9);
            long long round_deadline = t_start + round_line_ns * 100;

            for (int i = 0; i < TX_THROUGHPUT_ITERS; i++) {
                counted_write_stats_t cws;
                size_t n = counted_write_full(fd, buf, (size_t)test_size, &cws, round_deadline);
                total_completed += n;
                total_syscall_calls += cws.syscall_calls;
                if (cws.incomplete_logical_writes > 0) incomplete_logical++;
            }

            /* ── final drain ── */
            drain_stats_t drain_stats = {0};
            int final_drain_rc = checked_tcdrain(fd, &drain_stats);
            long long t_end = get_time_ns();

            /* ── instret end (strict) ── */
            const char *end_reason;
            uint64_t instret_end;
            int end_status = read_instret_strict(&instret_end, &end_reason);

            /* ── completion checks ── */
            int byte_ok = (total_completed == expected_bytes) ? 1 : 0;
            int drain_ok = (final_drain_rc >= 0 && drain_stats.errors == 0) ? 1 : 0;
            int instret_ok = (begin_status == 3 && end_status == 3
                              && instret_end >= instret_begin) ? 1 : 0;

            if (byte_ok && incomplete_logical == 0 && drain_ok && instret_ok) {
                long long duration_ns = t_end - t_start;
                uint64_t instret_delta = instret_end - instret_begin;
                double ipb = total_completed > 0
                    ? (double)instret_delta / (double)total_completed : 0.0;
                double ipw = total_syscall_calls > 0
                    ? (double)instret_delta / (double)total_syscall_calls : 0.0;

                inst_per_byte_arr[valid_rounds] = ipb;
                inst_per_write_arr[valid_rounds] = ipw;
                valid_rounds++;

                printf("  diag=s41-valid size=%d round=%d completed=%zu expected=%zu reset_rc=%d instret_begin=%llu instret_end=%llu instret_delta=%llu instructions_per_byte=%.2f instructions_per_write=%.0f begin_reason=%s end_reason=%s drain_rc=%d drain_errors=%d logical_writes=%d syscall_writes=%d duration_ms=%ld\r\n",
                       test_size, round + 1, total_completed, expected_bytes,
                       reset_rc,
                       (unsigned long long)instret_begin,
                       (unsigned long long)instret_end,
                       (unsigned long long)instret_delta,
                       ipb, ipw,
                       begin_reason, end_reason,
                       final_drain_rc, drain_stats.errors,
                       TX_THROUGHPUT_ITERS, total_syscall_calls,
                       (long)(duration_ns / 1000000LL));
            } else {
                printf("  diag=s41-invalid size=%d round=%d completed=%zu expected=%zu byte_ok=%d incomplete_logical=%d drain_ok=%d drain_rc=%d drain_errors=%d instret_begin_status=%d instret_end_status=%d instret_ok=%d begin_reason=%s end_reason=%s instret_begin=%llu instret_end=%llu\r\n",
                       test_size, round + 1, total_completed, expected_bytes,
                       byte_ok, incomplete_logical,
                       drain_ok, final_drain_rc, drain_stats.errors,
                       begin_status, end_status, instret_ok,
                       begin_reason, end_reason,
                       (unsigned long long)instret_begin,
                       (unsigned long long)instret_end);
            }
        }

        /* ── summary ── */
        if (valid_rounds > 0) {
            sort_doubles(inst_per_byte_arr, valid_rounds);
            sort_doubles(inst_per_write_arr, valid_rounds);
            int mid = valid_rounds / 2;
            double median_ipb = inst_per_byte_arr[mid];
            double median_ipw = inst_per_write_arr[mid];
            printf("  diag=s41-summary size=%d valid_rounds=%d median_instructions_per_byte=%.2f median_instructions_per_write=%.0f\r\n",
                   test_size, valid_rounds, median_ipb, median_ipw);
        } else {
            printf("  diag=s41-summary size=%d valid_rounds=0 status=no-valid-rounds\r\n",
                   test_size);
        }

        print_workload_tx_counters(fd, "s41-local-counters", (int)expected_bytes, last_reset_rc);

        free(buf);
    }

    close(fd);
    printf("\r\n");
}

/* ── S42: TX Compute Overlap (completion contract, enriched output) ──────── */
static void test_tx_compute_overlap(void) {
    printf("=== [S42] TX Compute Overlap (64B x %d, fixed window, %d sample rounds) ===\r\n",
           S42_PAYLOAD_ITERS, S42_SAMPLE_ROUNDS);
    prepare_section("S42");

    int fd = open(DEVICE_PATH, O_WRONLY);
    if (fd < 0) { perror("open"); return; }

    size_t total_payload = (size_t)S42_PAYLOAD_BYTES * S42_PAYLOAD_ITERS;
    double line_s = (double)total_payload / (UART_LINE_RATE_KBPS * 1024.0);
    long long window_ns = (long long)(line_s * 1e9);
    double theoretical_line_time_ms = line_s * 1000.0;

    /* idle calibration */
    fflush(stdout); checked_tcdrain(STDOUT_FILENO, NULL);
    long long idle_start = get_time_ns();
    long long idle_deadline = idle_start + window_ns;
    unsigned long long idle_iters = fixed_compute(idle_deadline);
    long long idle_end = get_time_ns();
    long long idle_duration_ns = idle_end - idle_start;
    double idle_rate = idle_duration_ns > 0 ? (double)idle_iters / ((double)idle_duration_ns / 1e9) : 0.0;

    printf("  idle window_ms=%.3f window_ns=%lld iters=%llu duration_ms=%.3f iters_per_sec=%.0f\r\n",
           (double)window_ns / 1e6, window_ns, idle_iters,
           (double)idle_duration_ns / 1e6, idle_rate);

    char *buf = malloc(S42_PAYLOAD_BYTES);
    if (!buf) { perror("malloc"); close(fd); return; }
    memset(buf, 0, S42_PAYLOAD_BYTES);

    printf("  overlap payload=%d iters=%d warmup=%d sample_rounds=%d theoretical_line_time_ms=%.3f\r\n",
           S42_PAYLOAD_BYTES, S42_PAYLOAD_ITERS, S42_WARMUP_ROUNDS, S42_SAMPLE_ROUNDS,
           theoretical_line_time_ms);

    /* per-round data for valid rounds */
    long valid_iters_arr[S42_SAMPLE_ROUNDS];
    long valid_total_dur_arr[S42_SAMPLE_ROUNDS];
    double valid_efficiency_arr[S42_SAMPLE_ROUNDS];
    int valid_round_count = 0;
    int last_reset_rc = 0;

    for (int round = 0; round < S42_WARMUP_ROUNDS + S42_SAMPLE_ROUNDS; round++) {
        fflush(stdout); checked_tcdrain(STDOUT_FILENO, NULL);
        int reset_rc = txdbg_reset(fd);
        last_reset_rc = reset_rc;

        long long t0 = get_time_ns();
        long long deadline = t0 + window_ns;

        /* burst write */
        size_t written = 0;
        long long write_start = get_time_ns();
        for (int i = 0; i < S42_PAYLOAD_ITERS; i++) {
            ssize_t n = write_full(fd, buf, S42_PAYLOAD_BYTES);
            if (n > 0) written += (size_t)n;
        }
        long long write_end = get_time_ns();
        long long write_duration = write_end - write_start;

        /* compute overlap */
        unsigned long long uart_iters = fixed_compute(deadline);
        long long compute_end = get_time_ns();

        /* final drain */
        drain_stats_t ds = {0};
        checked_tcdrain(fd, &ds);
        long long drain_end = get_time_ns();

        /* timing */
        long long total_dur = drain_end - t0;
        long long leftover_ns = compute_end - deadline;
        if (leftover_ns < 0) leftover_ns = 0;

        size_t expected_bytes = (size_t)S42_PAYLOAD_BYTES * S42_PAYLOAD_ITERS;
        int byte_ok = (written == expected_bytes) ? 1 : 0;
        int drain_ok = (ds.errors == 0) ? 1 : 0;
        int completion_ok = (byte_ok && drain_ok) ? 1 : 0;

        double useful_work_per_ms = total_dur > 0 ? (double)uart_iters / ((double)total_dur / 1e6) : 0.0;
        double total_over_line_ratio = theoretical_line_time_ms > 0.0
            ? (double)total_dur / 1e6 / theoretical_line_time_ms : 0.0;
        double overlap_efficiency = idle_iters > 0 ? (double)uart_iters / (double)idle_iters : 0.0;

        if (round >= S42_WARMUP_ROUNDS) {
            int idx = round - S42_WARMUP_ROUNDS;
            printf("  diag=s42-sample round=%d completion=%s byte_ok=%d drain_ok=%d completed=%zu expected=%zu write_return_us=%.1f useful_iters=%llu useful_work_per_ms=%.0f final_drain_ms=%.3f total_duration_ms=%.3f total_over_line_ratio=%.3f overlap_efficiency=%.4f reset_rc=%d drain_errors=%d leftover_ns=%lld\r\n",
                   idx + 1,
                   completion_ok ? "PASS" : "FAIL",
                   byte_ok, drain_ok,
                   written, expected_bytes,
                   (double)write_duration / 1000.0,
                   (unsigned long long)uart_iters,
                   useful_work_per_ms,
                   (double)(drain_end - compute_end) / 1e6,
                   (double)total_dur / 1e6,
                   total_over_line_ratio,
                   overlap_efficiency,
                   reset_rc, ds.errors, leftover_ns);

            if (completion_ok) {
                valid_iters_arr[valid_round_count] = (long)uart_iters;
                valid_total_dur_arr[valid_round_count] = (long)total_dur;
                valid_efficiency_arr[valid_round_count] = overlap_efficiency;
                valid_round_count++;
            }
        }
    }

    /* ── summary ── */
    if (valid_round_count > 0) {
        sort_longs(valid_iters_arr, valid_round_count);
        sort_longs(valid_total_dur_arr, valid_round_count);
        sort_doubles(valid_efficiency_arr, valid_round_count);
        int mid = valid_round_count / 2;
        printf("  diag=s42-summary valid_rounds=%d median_useful_iters=%ld median_total_duration_ms=%.3f median_overlap_efficiency=%.4f\r\n",
               valid_round_count,
               valid_iters_arr[mid],
               (double)valid_total_dur_arr[mid] / 1e6,
               valid_efficiency_arr[mid]);
    } else {
        printf("  diag=s42-summary valid_rounds=0 status=no-valid-rounds\r\n");
    }

    print_workload_tx_counters(fd, "s42-local-counters", S42_PAYLOAD_BYTES * S42_PAYLOAD_ITERS, last_reset_rc);

    free(buf);
    close(fd);
    printf("\r\n");
}

/* ── S43: Timer Wakeup Overshoot (5 idle + 5 loaded groups) ──────────────── */
static void test_timer_wakeup_overshoot(void) {
    printf("=== [S43] Timer Wakeup Overshoot (%d idle groups + %d loaded groups) ===\r\n",
           S43_GROUPS, S43_GROUPS);
    prepare_section("S43");

    int fd = open(DEVICE_PATH, O_WRONLY);
    if (fd < 0) {
        printf("  diag=S43 console-open-failed errno=%d\r\n", errno);
    }

    long long interval_ns = (long long)S43_INTERVAL_US * 1000LL;
    double theoretical_line_time_ns = (double)S43_TX_BURST_BYTES
        / (UART_LINE_RATE_KBPS * 1024.0) * 1e9;

    long samples[S43_SAMPLES];
    long all_valid[S43_GROUPS * S43_SAMPLES];
    int all_valid_count = 0;

    /* ── idle groups ── */
    printf("  s43-phase=idle groups=%d samples=%d interval_us=%d\r\n",
           S43_GROUPS, S43_SAMPLES, S43_INTERVAL_US);

    int idle_valid_groups = 0;
    for (int g = 0; g < S43_GROUPS; g++) {
        long long base = get_time_ns();
        int collected = collect_abs_sleep_samples(samples, S43_SAMPLES, base, interval_ns);
        long long after = get_time_ns();

        int errors = 0;
        int valid_count = 0;
        for (int i = 0; i < collected; i++) {
            if (samples[i] < 0) errors++;
            else valid_count++;
        }

        const char *status = (errors == 0) ? "PASS" : "FAIL";
        if (errors == 0 && valid_count > 0) {
            idle_valid_groups++;
            for (int i = 0; i < collected; i++) {
                if (samples[i] >= 0) all_valid[all_valid_count++] = samples[i];
            }
        }

        /* per-group raw */
        printf("  diag=s43-idle-group group=%d status=%s collected=%d errors=%d valid=%d duration_ms=%ld",
               g + 1, status, collected, errors, valid_count,
               (long)((after - base) / 1000000LL));
        for (int i = 0; i < collected && i < 3; i++) {
            printf(" sample[%d]=%ld", i, samples[i]);
        }
        printf("\r\n");

        /* per-group summary */
        if (valid_count > 0) {
            sort_longs(samples, collected);
            print_timer_stats("s43-idle-group-summary", samples, collected);
        }
    }

    /* ── loaded groups ── */
    if (fd >= 0) {
        char *load_buf = malloc(S43_TX_BURST_BYTES);
        if (load_buf) {
            memset(load_buf, 0, S43_TX_BURST_BYTES);

            printf("  s43-phase=loaded groups=%d burst_bytes=%d theoretical_line_time_ns=%.0f\r\n",
                   S43_GROUPS, S43_TX_BURST_BYTES, theoretical_line_time_ns);

            int loaded_valid_groups = 0;
            long loaded_all_samples[S43_GROUPS * S43_SAMPLES];
            int loaded_all_count = 0;

            for (int g = 0; g < S43_GROUPS; g++) {
                fflush(stdout); checked_tcdrain(STDOUT_FILENO, NULL);
                int reset_rc = txdbg_reset(fd);

                long long load_base = get_time_ns();
                counted_write_stats_t cws;
                long long burst_line_ns = (long long)((double)S43_TX_BURST_BYTES / (UART_LINE_RATE_KBPS * 1024.0) * 1e9);
                long long burst_deadline = get_time_ns() + burst_line_ns * 5;
                size_t burst_written = counted_write_full(fd, load_buf,
                    S43_TX_BURST_BYTES, &cws, burst_deadline);
                long long after_write = get_time_ns();

                /* check burst completion */
                if (burst_written != (size_t)S43_TX_BURST_BYTES) {
                    printf("  diag=s43-loaded-group group=%d status=FAIL reason=burst-incomplete written=%zu expected=%d incomplete_logical=%d syscall_calls=%d first_errno=%d\r\n",
                           g + 1, burst_written, S43_TX_BURST_BYTES,
                           cws.incomplete_logical_writes,
                           cws.syscall_calls, cws.first_errno);
                    /* still try to collect samples */
                }

                /* overlap window check */
                long long write_dur_ns = after_write - load_base;
                int overlap_ok = (write_dur_ns < (long long)theoretical_line_time_ns) ? 1 : 0;
                if (!overlap_ok) {
                    printf("  diag=s43-loaded-group group=%d status=not-applicable reason=no-overlap-window write_dur_ns=%lld theoretical_line_time_ns=%.0f\r\n",
                           g + 1, write_dur_ns, theoretical_line_time_ns);
                }

                /* collect samples */
                int collected = collect_abs_sleep_samples(samples, S43_SAMPLES,
                    load_base, interval_ns);
                long long after_samples = get_time_ns();

                /* final drain */
                drain_stats_t ds = {0};
                checked_tcdrain(fd, &ds);
                long long after_drain = get_time_ns();

                int errors = 0;
                int valid_count = 0;
                for (int i = 0; i < collected; i++) {
                    if (samples[i] < 0) errors++;
                    else valid_count++;
                }

                const char *status;
                if (burst_written != (size_t)S43_TX_BURST_BYTES) {
                    status = "FAIL";
                } else if (!overlap_ok) {
                    status = "not-applicable";
                } else if (errors > 0) {
                    status = "FAIL";
                } else {
                    status = "PASS";
                    loaded_valid_groups++;
                    for (int i = 0; i < collected; i++) {
                        if (samples[i] >= 0)
                            loaded_all_samples[loaded_all_count++] = samples[i];
                    }
                }

                printf("  diag=s43-loaded-group group=%d status=%s reset_rc=%d burst_written=%zu expected=%d write_dur_ns=%lld incomplete_logical=%d syscall_calls=%d collected=%d errors=%d valid=%d sample_duration_ms=%ld drain_ms=%ld drain_errors=%d",
                       g + 1, status, reset_rc,
                       burst_written, S43_TX_BURST_BYTES,
                       write_dur_ns, cws.incomplete_logical_writes,
                       cws.syscall_calls,
                       collected, errors, valid_count,
                       (long)((after_samples - after_write) / 1000000LL),
                       (long)((after_drain - after_samples) / 1000000LL),
                       ds.errors);
                for (int i = 0; i < collected && i < 3; i++) {
                    printf(" sample[%d]=%ld", i, samples[i]);
                }
                printf("\r\n");

                if (valid_count > 0) {
                    sort_longs(samples, collected);
                    print_timer_stats("s43-loaded-group-summary", samples, collected);
                }

                print_workload_tx_counters(fd, "s43-loaded-local-counters",
                    (int)burst_written, reset_rc);
            }

            /* ── loaded aggregate summary ── */
            if (loaded_all_count > 0) {
                sort_longs(loaded_all_samples, loaded_all_count);
                int p50_idx = loaded_all_count * 50 / 100;
                int p95_idx = loaded_all_count * 95 / 100;
                int p99_idx = loaded_all_count * 99 / 100;
                printf("  diag=s43-loaded-aggregate n=%d valid_groups=%d p50_ns=%ld p95_ns=%ld p99_ns=%ld max_ns=%ld\r\n",
                       loaded_all_count, loaded_valid_groups,
                       loaded_all_samples[p50_idx],
                       loaded_all_samples[p95_idx],
                       loaded_all_samples[p99_idx],
                       loaded_all_samples[loaded_all_count - 1]);
            }

            free(load_buf);
        }
    } else {
        printf("  s43-loaded status=not-applicable reason=no-console-fd\r\n");
    }

    /* ── idle aggregate summary ── */
    if (all_valid_count > 0) {
        sort_longs(all_valid, all_valid_count);
        int p50_idx = all_valid_count * 50 / 100;
        int p95_idx = all_valid_count * 95 / 100;
        int p99_idx = all_valid_count * 99 / 100;
        printf("  diag=s43-idle-aggregate n=%d valid_groups=%d p50_ns=%ld p95_ns=%ld p99_ns=%ld max_ns=%ld\r\n",
               all_valid_count, idle_valid_groups,
               all_valid[p50_idx],
               all_valid[p95_idx],
               all_valid[p99_idx],
               all_valid[all_valid_count - 1]);
    }

    if (fd >= 0) close(fd);
    printf("\r\n");
}

/* ── TX counter proxy summary ─────────────────────────────────────────────── */
static void print_tx_counter_summary(int fd) {
    printf("=== [S40] TX Counter Proxy Summary ===\r\n");

    txdbg_snapshot_t s;
    int rc = txdbg_snapshot(fd, &s);
    if (rc < 0) {
        printf("  status=FAIL ioctl_error=%d errno=%d error=%s\r\n\r\n",
               rc, errno, strerror(errno));
        return;
    }

    /* Determine availability: if user_push_calls is 0, telemetry counters are unavailable */
    int telemetry_available = (s.user_push_calls > 0);

    printf("  telemetry_available=%d ioctl_rc=%d\r\n", telemetry_available, rc);

    /* Raw counter fields */
    printf("  counter=user-push user_calls=%llu user_req=%llu user_acc=%llu\r\n",
           (unsigned long long)s.user_push_calls,
           (unsigned long long)s.user_push_requested_bytes,
           (unsigned long long)s.user_push_accepted_bytes);
    printf("  counter=ring-pop ring_pop_calls=%llu ring_pop_bytes=%llu\r\n",
           (unsigned long long)s.ring_pop_calls,
           (unsigned long long)s.ring_pop_bytes);
    printf("  counter=hw-send hw_send_calls=%llu hw_send_bytes=%llu hw_send_zero=%llu hw_send_max_chunk=%llu\r\n",
           (unsigned long long)s.hw_send_calls,
           (unsigned long long)s.hw_send_bytes,
           (unsigned long long)s.hw_send_zero,
           (unsigned long long)s.hw_send_max_chunk);
    printf("  counter=no-progress no_progress_budget=%llu slow_poll_exh=%llu yield_exh=%llu\r\n",
           (unsigned long long)s.no_progress_budget_exhausted,
           (unsigned long long)s.slow_poll_exhausted,
           (unsigned long long)s.yield_retries_exhausted);
    printf("  counter=drain-state ring_empty=%llu copier_active=%llu staged_bytes=%llu transmitter_empty=%llu\r\n",
           (unsigned long long)s.ring_empty,
           (unsigned long long)s.copier_active,
           (unsigned long long)s.staged_bytes,
           (unsigned long long)s.transmitter_empty);

    /* Derived proxy fields */
    if (telemetry_available) {
        double bytes_per_user_call = s.user_push_calls > 0
            ? (double)s.user_push_accepted_bytes / (double)s.user_push_calls : 0.0;
        double bytes_per_ring_pop = s.ring_pop_calls > 0
            ? (double)s.ring_pop_bytes / (double)s.ring_pop_calls : 0.0;
        double bytes_per_hw_send = s.hw_send_calls > 0
            ? (double)s.hw_send_bytes / (double)s.hw_send_calls : 0.0;
        double hw_kb = (double)s.hw_send_bytes / 1024.0;
        double zero_per_kb = hw_kb > 0.0
            ? (double)s.hw_send_zero / hw_kb : 0.0;
        double no_progress_per_kb = hw_kb > 0.0
            ? (double)s.no_progress_budget_exhausted / hw_kb : 0.0;

        printf("  proxy=derived bytes_per_user_call=%.1f bytes_per_ring_pop=%.1f bytes_per_hw_send=%.3f zero_per_kb=%.1f no_progress_per_kb=%.1f\r\n",
               bytes_per_user_call, bytes_per_ring_pop, bytes_per_hw_send,
               zero_per_kb, no_progress_per_kb);
    } else {
        printf("  proxy=derived status=not-available reason=telemetry-counters-are-zero\r\n");
    }

    printf("\r\n");
}

/* ── 主函数 ──────────────────────────────────────────────────────────────── */
int main(void) {
    printf("UART Async Benchmark\r\n");
    printf("====================\r\n\r\n");

    print_manifest();
    report_instret_overhead();

    /* Reset TX debug counters before benchmark run for clean delta */
    int fd_counter = open(DEVICE_PATH, O_WRONLY);
    if (fd_counter >= 0) {
        txdbg_reset(fd_counter);
    }

    test_tx_throughput();
    test_tx_enqueue_no_drain();
    test_tx_batch_drain();
    test_tx_writev_fragments();
    test_tx_small_packet_break_even();
    test_tx_latency();
    test_tx_latency_matrix();
    test_nonblock_read();
    test_rx_fixed_payload();
    test_tx_cpu_work();
    test_tx_compute_overlap();
    test_timer_wakeup_overshoot();

    /* TX counter proxy summary after all benchmarks */
    if (fd_counter >= 0) {
        print_tx_counter_summary(fd_counter);
        close(fd_counter);
    }

    printf("Done.\r\n");
    fflush(stdout);
    checked_tcdrain(STDOUT_FILENO, NULL);
    return 0;
}
