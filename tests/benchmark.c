/**
 * UART Async Benchmark — 真实串口性能测试
 *
 * 修复项 (O44 + statistics):
 * - TX throughput 写 /dev/console（非 /dev/null） + tcdrain
 * - TX 延迟 写 + tcdrain，预热 5 次，测量 200 次
 * - 非阻塞模式测试 (FIONBIO)
 * - 分位值：线性插值（P = (N-1)*p/100，线性插值相邻元素）
 *
 * 编译:
 *   riscv64-linux-musl-gcc -static -o tests/benchmark tests/benchmark.c -lm
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
#include <math.h>
#include <sys/ioctl.h>
#include <termios.h>
#include <errno.h>

#define DEVICE_PATH "/dev/console"
#define BUF_SIZE     1024
#define LAT_N        200      /* latency measurement iterations */
#define LAT_WARMUP   5        /* warmup iterations (discarded) */

static long long get_time_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (long long)ts.tv_sec * 1000000000LL + ts.tv_nsec;
}

static int cmp_long(const void *a, const void *b) {
    long va = *(const long *)a, vb = *(const long *)b;
    return (va > vb) - (va < vb);
}

/* linear interpolation percentile: P = (N-1)*p/100, interp between adjacent */
static double percentile(const long *sorted, int n, double p) {
    if (n <= 0) return 0.0;
    if (n == 1) return sorted[0];
    double pos = (n - 1) * p / 100.0;
    int lo = (int)pos;
    int hi = lo + 1;
    if (hi >= n) return sorted[n - 1];
    double frac = pos - lo;
    return sorted[lo] * (1.0 - frac) + sorted[hi] * frac;
}

static double stddev(const long *data, int n, double avg) {
    if (n <= 1) return 0.0;
    double sum_sq = 0.0;
    for (int i = 0; i < n; i++) {
        double d = data[i] - avg;
        sum_sq += d * d;
    }
    return sqrt(sum_sq / (n - 1));
}

static void print_methodology(void) {
    printf("Methodology:\n");
    printf("  Warmup:    %d iterations (discarded)\n", LAT_WARMUP);
    printf("  Latency:   %d iterations (measured)\n", LAT_N);
    printf("  Throughput: 100 iterations per size\n");
    printf("  Percentiles: linear interpolation on sorted array\n");
    printf("  Timer:      clock_gettime(CLOCK_MONOTONIC), ns resolution\n");
    printf("\n");
}

/* ── TX throughput: 写 /dev/console + tcdrain ─────────────────────── */
static void test_tx_throughput(void) {
    printf("=== TX Throughput (to /dev/console + tcdrain) ===\n");

    int fd = open(DEVICE_PATH, O_WRONLY);
    if (fd < 0) { perror("open"); return; }

    /* warmup */
    char wb[64] = {0};
    for (int i = 0; i < 5; i++) { write(fd, wb, sizeof(wb)); tcdrain(fd); }

    int sizes[] = {64, 256, 1024, 4096};
    int num_sizes = 4;

    for (int s = 0; s < num_sizes; s++) {
        int test_size = sizes[s];
        int iterations = 100;
        char *buf = calloc(1, test_size);
        if (!buf) { perror("calloc"); continue; }

        long long start = get_time_ns();
        size_t total = 0;

        for (int i = 0; i < iterations; i++) {
            ssize_t n = write(fd, buf, test_size);
            if (n > 0) { total += n; tcdrain(fd); }
            else break;
        }

        long long end = get_time_ns();
        double elapsed_s = (double)(end - start) / 1000000000.0;
        double kbps = (double)total / elapsed_s / 1024.0;
        double line_rate = kbps / 11.52 * 100.0;

        printf("  size=%d  iters=%d | %.2f KB/s | %.1f%% line rate\n",
               test_size, iterations, kbps, line_rate);
        free(buf);
    }
    close(fd);
    printf("\n");
}

/* ── TX latency: 单字节 write + tcdrain，linear interpolation ─────── */
static void test_tx_latency(void) {
    printf("=== TX Latency (single byte + tcdrain, warmup=%d, n=%d) ===\n",
           LAT_WARMUP, LAT_N);

    int fd = open(DEVICE_PATH, O_WRONLY);
    if (fd < 0) { perror("open"); return; }

    /* warmup */
    char wb = 0;
    for (int i = 0; i < LAT_WARMUP; i++) { write(fd, &wb, 1); tcdrain(fd); }

    long latencies[LAT_N];
    int ok = 0;
    for (int i = 0; i < LAT_N; i++) {
        char tx = 'A' + (i % 26);
        long long start = get_time_ns();
        if (write(fd, &tx, 1) != 1) continue;
        tcdrain(fd);
        long long end = get_time_ns();
        latencies[ok++] = (long)(end - start);
    }

    if (ok == 0) { printf("  no data\n\n"); close(fd); return; }

    qsort(latencies, ok, sizeof(long), cmp_long);

    long sum = 0, min = latencies[0], max = latencies[ok - 1];
    for (int i = 0; i < ok; i++) sum += latencies[i];
    double avg = (double)sum / ok;
    double sd  = stddev(latencies, ok, avg);

    printf("  n=%-4d  min=%.3f ms  max=%.3f ms  avg=%.3f ms  stddev=%.3f ms\n",
           ok, min / 1e6, max / 1e6, avg / 1e6, sd / 1e6);
    printf("  P50=%.3f ms  P95=%.3f ms  P99=%.3f ms  P999=%.3f ms\n",
           percentile(latencies, ok, 50) / 1e6,
           percentile(latencies, ok, 95) / 1e6,
           percentile(latencies, ok, 99) / 1e6,
           percentile(latencies, ok, 99.9) / 1e6);
    printf("\n");
    close(fd);
}

/* ── non-blocking read test (FIONBIO) ───────────────────────────── */
static void test_nonblock_read(void) {
    printf("=== Non-blocking Read (FIONBIO) ===\n");

    int fd = open(DEVICE_PATH, O_RDWR | O_NONBLOCK);
    if (fd < 0) { perror("open"); return; }

    char buf[16];
    ssize_t n = read(fd, buf, sizeof(buf));
    if (n == -1 && (errno == EAGAIN || errno == EWOULDBLOCK))
        printf("  PASS: O_NONBLOCK read → EAGAIN (no data)\n");
    else if (n >= 0)
        printf("  INFO: read %zd bytes (data already in buffer)\n", n);
    else
        printf("  FAIL: errno=%d (%s)\n", errno, strerror(errno));
    close(fd);

    /* test via ioctl */
    fd = open(DEVICE_PATH, O_RDWR);
    if (fd < 0) { perror("open"); return; }
    int on = 1;
    if (ioctl(fd, FIONBIO, &on) < 0) {
        printf("  FAIL: ioctl FIONBIO: %s\n", strerror(errno));
        close(fd); return;
    }
    n = read(fd, buf, sizeof(buf));
    if (n == -1 && (errno == EAGAIN || errno == EWOULDBLOCK))
        printf("  PASS: ioctl FIONBIO read → EAGAIN (no data)\n");
    else if (n >= 0)
        printf("  INFO: read %zd bytes (data already in buffer)\n", n);
    else
        printf("  FAIL: errno=%d (%s)\n", errno, strerror(errno));
    close(fd);
    printf("\n");
}

int main(void) {
    printf("UART Async Benchmark (QEMU @ 115200 bps)\n");
    printf("=========================================\n\n");
    print_methodology();
    test_tx_throughput();
    test_tx_latency();
    test_nonblock_read();
    printf("Done.\n");
    return 0;
}
