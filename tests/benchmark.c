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

static const int TX_THROUGHPUT_SIZES[] = {64, 256, 1024, 4096};
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

static void print_manifest(void) {
    printf("=== Benchmark Manifest ===\r\n");
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
    printf("  tx_throughput_iters=%d\r\n", TX_THROUGHPUT_ITERS);
    printf("  tx_throughput_drain=tcdrain-after-each-write\r\n");
    printf("  tx_latency_size=1\r\n");
    printf("  tx_latency_iters=%d\r\n", TX_LATENCY_ITERS);
    printf("  fifo_matrix_sizes=");
    print_int_list(FIFO_MATRIX_SIZES, sizeof(FIFO_MATRIX_SIZES) / sizeof(FIFO_MATRIX_SIZES[0]));
    printf("\r\n");
    printf("  fifo_matrix_iters=%d\r\n", FIFO_MATRIX_ITERS);
    printf("  rx_mode=empty-nonblocking-eagain\r\n");
    printf("\r\n");
}

/* ── TX throughput: 写 /dev/console + tcdrain ─────────────────────── */
static void test_tx_throughput(void) {
    printf("=== TX Throughput (to /dev/console + tcdrain) ===\r\n");

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
            /* loop on short writes — M3 contract returns actual accepted count */
            size_t remaining = test_size;
            while (remaining > 0) {
                ssize_t n = write(fd, buf + (test_size - remaining), remaining);
                if (n > 0) {
                    total += n;
                    remaining -= n;
                } else {
                    break;
                }
            }
            tcdrain(fd);   /* wait until UART FIFO is empty */
        }

        long long end = get_time_ns();
        double elapsed_s = (double)(end - start) / 1000000000.0;
        double kbps = (double)total / elapsed_s / 1024.0;
        double line_rate = kbps / UART_LINE_RATE_KBPS * 100.0;

        printf("  size=%d  iters=%d | %.2f KB/s | %.1f%% line rate\r\n",
               test_size, iterations, kbps, line_rate);

        free(buf);
    }

    close(fd);
    printf("\r\n");
}

/* ── TX latency: 单字节 write + tcdrain ─────────────────────────── */
static void test_tx_latency(void) {
    printf("=== TX Latency (single byte + tcdrain) ===\r\n");

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

    if (ok == 0) { printf("  no data\r\n\r\n"); close(fd); return; }

    /* sort for percentiles */
    for (int i = 0; i < ok - 1; i++)
        for (int j = 0; j < ok - i - 1; j++)
            if (latencies[j] > latencies[j + 1]) {
                long t = latencies[j];
                latencies[j] = latencies[j + 1];
                latencies[j + 1] = t;
            }

    long sum = 0;
    for (int i = 0; i < ok; i++) sum += latencies[i];
    printf("  n=%d  avg=%.3f ms  P50=%.3f ms  P95=%.3f ms  P99=%.3f ms\r\n\r\n",
           ok,
           (double)sum / ok / 1000000.0,
           (double)latencies[ok * 50 / 100] / 1000000.0,
           (double)latencies[ok * 95 / 100] / 1000000.0,
           (double)latencies[ok * 99 / 100] / 1000000.0);

    close(fd);
}

/* ── TX latency FIFO boundary matrix ────────────────────────────── */
static void test_tx_latency_matrix(void) {
    printf("=== TX Latency FIFO Boundary Matrix ===\r\n");

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

        if (ok == 0) { printf("  size=%d  no data\r\n\r\n", sz); continue; }

        /* sort for percentiles (bubble — same as test_tx_latency) */
        for (int i = 0; i < ok - 1; i++)
            for (int j = 0; j < ok - i - 1; j++)
                if (latencies[j] > latencies[j + 1]) {
                    long t = latencies[j];
                    latencies[j] = latencies[j + 1];
                    latencies[j + 1] = t;
                }

        long sum = 0;
        for (int i = 0; i < ok; i++) sum += latencies[i];
        printf("  size=%d  n=%d  avg=%.3f ms  P50=%.3f ms  P95=%.3f ms\r\n\r\n",
               sz, ok,
               (double)sum / ok / 1000000.0,
               (double)latencies[ok * 50 / 100] / 1000000.0,
               (double)latencies[ok * 95 / 100] / 1000000.0);
    }

    close(fd);
}

/* ── non-blocking read test (FIONBIO) ───────────────────────────── */
static void test_nonblock_read(void) {
    printf("=== Non-blocking Read (FIONBIO) ===\r\n");

    int fd = open(DEVICE_PATH, O_RDWR | O_NONBLOCK);
    if (fd < 0) { perror("open"); return; }

    char buf[16];
    ssize_t n = read(fd, buf, sizeof(buf));

    if (n == -1 && (errno == EAGAIN || errno == EWOULDBLOCK)) {
        printf("  PASS: O_NONBLOCK read → EAGAIN (no data)\r\n");
    } else if (n >= 0) {
        printf("  INFO: read %zd bytes (data already in buffer)\r\n", n);
    } else {
        printf("  FAIL: errno=%d (%s)\r\n", errno, strerror(errno));
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
        printf("  PASS: ioctl FIONBIO read → EAGAIN (no data)\r\n");
    } else if (n >= 0) {
        printf("  INFO: read %zd bytes (data already in buffer)\r\n", n);
    } else {
        printf("  FAIL: errno=%d (%s)\r\n", errno, strerror(errno));
    }

    close(fd);
    printf("\r\n");
}

/* ── 主函数 ──────────────────────────────────────────────────────── */
int main(void) {
    printf("UART Async Benchmark\r\n");
    printf("====================\r\n\r\n");

    print_manifest();
    test_tx_throughput();
    test_tx_latency();
    test_tx_latency_matrix();
    test_nonblock_read();

    printf("Done.\r\n");
    fflush(stdout);
    tcdrain(STDOUT_FILENO);
    return 0;
}
