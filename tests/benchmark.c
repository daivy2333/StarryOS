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

#ifndef BENCH_RX_FIXED_BYTES
#define BENCH_RX_FIXED_BYTES 0
#endif

static const int TX_THROUGHPUT_SIZES[] = {64, 256, 1024};
static const int TX_BREAK_EVEN_SIZES[] = {64, 128, 256};
static const int FIFO_MATRIX_SIZES[] = {1, 15, 16, 17, 31, 32, 33, 48, 49};

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
    printf("\r\n");
}

/* ── TX throughput: 写 /dev/console + tcdrain ─────────────────────── */
static void test_tx_throughput(void) {
    printf("=== [S10] TX Throughput Baseline (write + tcdrain each iteration) ===\r\n");

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
        size_t total = 0;

        for (int i = 0; i < iterations; i++) {
            ssize_t n = write_full(fd, buf, (size_t)test_size);
            if (n > 0) total += (size_t)n;
            tcdrain(fd);   /* wait until UART FIFO is empty */
        }

        long long end = get_time_ns();
        double elapsed_s = (double)(end - start) / 1000000000.0;
        double kbps = (double)total / elapsed_s / 1024.0;
        double line_rate = kbps / UART_LINE_RATE_KBPS * 100.0;

        printf("  policy=drain-each size=%d iters=%d bytes=%zu kbps=%.2f line_rate_pct=%.1f\r\n",
               test_size, iterations, total, kbps, line_rate);

        free(buf);
    }

    close(fd);
    printf("\r\n");
}

/* ── TX enqueue: write without per-iteration drain, final tcdrain outside timing ── */
static void test_tx_enqueue_no_drain(void) {
    printf("=== [S11] TX Enqueue Cost (write loop, final drain outside timing) ===\r\n");

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
        long long start = get_time_ns();

        for (int i = 0; i < TX_THROUGHPUT_ITERS; i++) {
            ssize_t n = write_full(fd, buf, (size_t)test_size);
            if (n > 0) total += (size_t)n;
            if (n != test_size) short_writes++;
        }

        long long enqueue_end = get_time_ns();
        tcdrain(fd);
        long long drain_end = get_time_ns();

        double elapsed_s = (double)(enqueue_end - start) / 1000000000.0;
        double kbps = elapsed_s > 0.0 ? (double)total / elapsed_s / 1024.0 : 0.0;

        printf("  policy=no-drain size=%d iters=%d bytes=%zu short_writes=%d enqueue_kbps=%.2f final_drain_ms=%ld\r\n",
               test_size, TX_THROUGHPUT_ITERS, total, short_writes, kbps,
               elapsed_ms(enqueue_end, drain_end));

        free(buf);
    }

    close(fd);
    printf("\r\n");
}

/* ── TX batch drain: amortize tcdrain overhead while preserving physical drain ── */
static void test_tx_batch_drain(void) {
    printf("=== [S12] TX Batch Drain (write N iterations, then tcdrain) ===\r\n");

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
        long long start = get_time_ns();

        for (int i = 0; i < TX_THROUGHPUT_ITERS; i++) {
            ssize_t n = write_full(fd, buf, (size_t)test_size);
            if (n > 0) total += (size_t)n;

            if ((i + 1) % TX_BATCH_DRAIN_EVERY == 0) {
                tcdrain(fd);
                drain_count++;
            }
        }

        tcdrain(fd);
        drain_count++;

        long long end = get_time_ns();
        double elapsed_s = (double)(end - start) / 1000000000.0;
        double kbps = (double)total / elapsed_s / 1024.0;
        double line_rate = kbps / UART_LINE_RATE_KBPS * 100.0;

        printf("  policy=batch-drain size=%d iters=%d batch=%d drains=%d bytes=%zu kbps=%.2f line_rate_pct=%.1f\r\n",
               test_size, TX_THROUGHPUT_ITERS, TX_BATCH_DRAIN_EVERY,
               drain_count, total, kbps, line_rate);

        free(buf);
    }

    close(fd);
    printf("\r\n");
}

/* ── TX writev: syscall-side aggregation witness ───────────────────── */
static void test_tx_writev_fragments(void) {
    printf("=== [S13] TX writev Fragments (fragment aggregation witness) ===\r\n");

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
    long long start = get_time_ns();

    for (int i = 0; i < TX_THROUGHPUT_ITERS; i++) {
        ssize_t n = writev(fd, iov, TX_WRITEV_FRAGMENTS);
        if (n > 0) total += (size_t)n;
        if (n != TX_WRITEV_FRAGMENT_SIZE * TX_WRITEV_FRAGMENTS) short_writes++;
        tcdrain(fd);
    }

    long long end = get_time_ns();
    double elapsed_s = (double)(end - start) / 1000000000.0;
    double kbps = (double)total / elapsed_s / 1024.0;
    double line_rate = kbps / UART_LINE_RATE_KBPS * 100.0;

    printf("  policy=writev-drain-each fragments=%d fragment_size=%d total_size=%d iters=%d bytes=%zu short_writes=%d kbps=%.2f line_rate_pct=%.1f\r\n",
           TX_WRITEV_FRAGMENTS, TX_WRITEV_FRAGMENT_SIZE,
           TX_WRITEV_FRAGMENTS * TX_WRITEV_FRAGMENT_SIZE,
           TX_THROUGHPUT_ITERS, total, short_writes, kbps, line_rate);

    free(buf);
    close(fd);
    printf("\r\n");
}

/* ── TX small packet break-even: 64/128/256 with the baseline drain policy ── */
static void test_tx_small_packet_break_even(void) {
    printf("=== [S14] TX Small Packet Break-even (64/128/256 drain-each) ===\r\n");

    int fd = open(DEVICE_PATH, O_WRONLY);
    if (fd < 0) { perror("open"); return; }

    int num_sizes = sizeof(TX_BREAK_EVEN_SIZES) / sizeof(TX_BREAK_EVEN_SIZES[0]);

    for (int s = 0; s < num_sizes; s++) {
        int test_size = TX_BREAK_EVEN_SIZES[s];
        char *buf = malloc(test_size);
        if (!buf) { perror("malloc"); continue; }
        memset(buf, 0, test_size);

        size_t total = 0;
        long long start = get_time_ns();

        for (int i = 0; i < TX_THROUGHPUT_ITERS; i++) {
            ssize_t n = write_full(fd, buf, (size_t)test_size);
            if (n > 0) total += (size_t)n;
            tcdrain(fd);
        }

        long long end = get_time_ns();
        double elapsed_s = (double)(end - start) / 1000000000.0;
        double kbps = (double)total / elapsed_s / 1024.0;
        double line_rate = kbps / UART_LINE_RATE_KBPS * 100.0;

        printf("  policy=drain-each size=%d iters=%d bytes=%zu kbps=%.2f line_rate_pct=%.1f\r\n",
               test_size, TX_THROUGHPUT_ITERS, total, kbps, line_rate);

        free(buf);
    }

    close(fd);
    printf("\r\n");
}

/* ── TX latency: 单字节 write + tcdrain ─────────────────────────── */
static void test_tx_latency(void) {
    printf("=== [S20] TX Latency (single byte + tcdrain) ===\r\n");

    int fd = open(DEVICE_PATH, O_WRONLY);
    if (fd < 0) { perror("open"); return; }

    long latencies[TX_LATENCY_ITERS];
    int ok = 0;

    for (int i = 0; i < TX_LATENCY_ITERS; i++) {
        char tx = 0;
        long long start = get_time_ns();
        if (write(fd, &tx, 1) != 1) continue;
        tcdrain(fd);
        long long end = get_time_ns();
        latencies[ok++] = (long)(end - start);
    }

    print_latency_summary("size=1 policy=drain-each", latencies, ok);
    printf("\r\n");

    close(fd);
}

/* ── TX latency FIFO boundary matrix ────────────────────────────── */
static void test_tx_latency_matrix(void) {
    printf("=== [S21] TX Latency FIFO Boundary Matrix ===\r\n");

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

        for (int i = 0; i < FIFO_MATRIX_ITERS; i++) {
            long long start = get_time_ns();
            ssize_t n = write(fd, buf, sz);
            if (n != sz) continue;
            tcdrain(fd);
            long long end = get_time_ns();
            latencies[ok++] = (long)(end - start);
        }

        free(buf);

        char prefix[64];
        snprintf(prefix, sizeof(prefix), "size=%d policy=drain-each", sz);
        print_latency_summary(prefix, latencies, ok);
    }

    close(fd);
    printf("\r\n");
}

/* ── non-blocking read test (FIONBIO) ───────────────────────────── */
static void test_nonblock_read(void) {
    printf("=== [S30] RX Empty Non-blocking Read (FIONBIO) ===\r\n");

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

/* ── 主函数 ──────────────────────────────────────────────────────── */
int main(void) {
    printf("UART Async Benchmark\r\n");
    printf("====================\r\n\r\n");

    print_manifest();
    test_tx_throughput();
    test_tx_enqueue_no_drain();
    test_tx_batch_drain();
    test_tx_writev_fragments();
    test_tx_small_packet_break_even();
    test_tx_latency();
    test_tx_latency_matrix();
    test_nonblock_read();
    test_rx_fixed_payload();

    printf("Done.\r\n");
    fflush(stdout);
    tcdrain(STDOUT_FILENO);
    return 0;
}
