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
#include <sys/uio.h>
#include <termios.h>
#include <errno.h>

#define DEVICE_PATH "/dev/console"
#define BUF_SIZE     1024
#define UART_LINE_RATE_KBPS 11.52

#define BENCH_VERSION "q19c-m0-20260703"
#ifndef BENCH_TARGET_MODE
#define BENCH_TARGET_MODE "unspecified"
#endif
#ifndef BENCH_STARTUP_CHAIN
#define BENCH_STARTUP_CHAIN "unspecified"
#endif
#ifndef BENCH_ROOT_PROVIDER
#define BENCH_ROOT_PROVIDER "unspecified"
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
#define BENCH_BACKEND "polling-console"

static const int TX_THROUGHPUT_SIZES[] = {64, 256, 1024};
static const int TX_BREAK_EVEN_SIZES[] = {64, 128, 256};
static const int FIFO_MATRIX_SIZES[] = {1, 15, 16, 17, 31, 32, 33, 48, 49};

typedef struct {
    int calls;
    int errors;
    int first_errno;
    int last_errno;
} drain_stats_t;

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

static void print_manifest(void) {
    printf("=== [S00] Benchmark Manifest ===\r\n");
    printf("  version=%s\r\n", BENCH_VERSION);
    printf("  backend=%s\r\n", BENCH_BACKEND);
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
    printf("  tx_transmit_policy=blocking\r\n");
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
    printf("\r\n");
}

/* ── TX throughput: 写 /dev/console + tcdrain ─────────────────────── */
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

/* ── S11 Blocking Transmit: write without per-iteration drain, final tcdrain outside timing ── */
static void test_tx_enqueue_no_drain(void) {
    printf("=== [S11] Blocking Transmit (write loop, final drain outside timing) ===\r\n");
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

/* ── TX writev: syscall-side aggregation witness ───────────────────── */
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

/* ── TX latency: 单字节 write + tcdrain ─────────────────────────── */
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

/* ── TX latency FIFO boundary matrix ────────────────────────────── */
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

/* ── non-blocking read test (FIONBIO) ───────────────────────────── */
static void test_nonblock_read(void) {
#ifdef BENCH_D1_DIAG
    printf("=== [S30] RX Empty Non-blocking Read (FIONBIO) ===\r\n");
    printf("  status=UNSUPPORTED reason=D1-UART-RX-not-implemented\r\n\r\n");
    return;
#endif
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

/* ── optional RX fixed-payload witness ───────────────────────────── */
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

/* ── TX counter proxy summary ─────────────────────────────────────── */
static void print_tx_counter_summary(int fd) {
    printf("=== [S40] TX Counter Proxy Summary ===\r\n");

    txdbg_snapshot_t s;
    int rc = txdbg_snapshot(fd, &s);
    if (rc < 0) {
        printf("  status=UNSUPPORTED reason=backend-polling-console-no-telemetry\r\n");
        printf("  proxy=not-available\r\n");
        printf("\r\n");
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

/* ── 主函数 ──────────────────────────────────────────────────────── */
int main(void) {
    printf("UART Async Benchmark\r\n");
    printf("====================\r\n\r\n");

    print_manifest();

    /* Startup ring — not applicable in polling-console mode (no async driver). */
    printf("=== [S05] Startup Ring ===\r\n");
    printf("  status=SKIPPED reason=no-async-driver\r\n\r\n");

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
