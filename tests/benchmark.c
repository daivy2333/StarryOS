/**
 * UART Async End-to-End Benchmark
 *
 * 测量 write() + tcdrain() 的完整端到端延迟和吞吐量，并区分：
 *   - 理论硬件时间（bytes / baud_rate）
 *   - 软件开销（实测 - 理论）
 *
 * QEMU 上：硬件时间为 0（UART 瞬时），测出的是纯软件路径开销。
 * 真板上：硬件时间主导（~86.8 µs/byte），软件开销可忽略。
 *
 * 编译:
 *   riscv64-linux-musl-gcc -static -o tests/benchmark tests/benchmark.c -lm
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
#define BAUD_BPS     115200.0
#define BYTE_TIME_NS (10.0 / BAUD_BPS * 1e9)  /* 86.8 us/byte @ 115200 */
#define LAT_N        200
#define LAT_WARMUP   5
#define TP_ITERS     100

static long long get_time_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (long long)ts.tv_sec * 1000000000LL + ts.tv_nsec;
}

static int cmp_long(const void *a, const void *b) {
    long va = *(const long *)a, vb = *(const long *)b;
    return (va > vb) - (va < vb);
}

static double percentile(const long *sorted, int n, double p) {
    if (n <= 0) return 0.0;
    if (n == 1) return sorted[0];
    double pos = (n - 1) * p / 100.0;
    int lo = (int)pos, hi = lo + 1;
    if (hi >= n) return sorted[n - 1];
    return sorted[lo] * (1.0 - (pos - lo)) + sorted[hi] * (pos - lo);
}

static double stddev(const long *data, int n, double avg) {
    if (n <= 1) return 0.0;
    double sum_sq = 0.0;
    for (int i = 0; i < n; i++) { double d = data[i] - avg; sum_sq += d * d; }
    return sqrt(sum_sq / (n - 1));
}

/* ── end-to-end throughput ───────────────────────────────────────── */
static void test_e2e_throughput(void) {
    printf("=== End-to-End TX Throughput (write + tcdrain) ===\n");
    printf("  %6s  %6s  %10s  %10s\n",
           "size", "iters", "measured/iter", "hw-theory/iter");
    printf("  %6s  %6s  %10s  %10s\n",
           "-----", "-----", "----------", "-----------");

    int fd = open(DEVICE_PATH, O_WRONLY);
    if (fd < 0) { perror("open"); return; }

    char wb[64] = {0};
    for (int i = 0; i < 5; i++) { write(fd, wb, sizeof(wb)); tcdrain(fd); }

    int sizes[] = {64, 256, 1024, 4096};
    for (int s = 0; s < 4; s++) {
        int sz = sizes[s];
        char *buf = calloc(1, sz);
        if (!buf) continue;

        long long t0 = get_time_ns();
        size_t total = 0;
        for (int i = 0; i < TP_ITERS; i++) {
            ssize_t n = write(fd, buf, sz);
            if (n > 0) { total += n; tcdrain(fd); }
            else break;
        }
        long long t1 = get_time_ns();

        double per_us = (t1 - t0) / 1000.0 / TP_ITERS;
        double hw_us = sz * BYTE_TIME_NS / 1000.0;

        printf("  %6d  %6d  %7.1f us  %7.1f us\n",
               sz, TP_ITERS, per_us, hw_us);
        free(buf);
    }
    close(fd);
    printf("  hw-theory = bytes * 10 / baud (86.8 us/byte @ 115200)\n");
    printf("  On QEMU: measured ≈ software overhead (HW is instant)\n");
    printf("  On real HW: end-to-end = hw-theory + software overhead\n");
    printf("\n");
}

/* ── end-to-end latency ──────────────────────────────────────────── */
static void test_e2e_latency(void) {
    printf("=== End-to-End TX Latency (1-byte write + tcdrain, n=%d) ===\n", LAT_N);

    int fd = open(DEVICE_PATH, O_WRONLY);
    if (fd < 0) { perror("open"); return; }

    /* warmup */
    char wb = 0;
    for (int i = 0; i < LAT_WARMUP; i++) { write(fd, &wb, 1); tcdrain(fd); }

    long latencies[LAT_N];
    int ok = 0;
    for (int i = 0; i < LAT_N; i++) {
        char tx = 0;  /* non-printable — doesn't clutter terminal */
        long long t0 = get_time_ns();
        if (write(fd, &tx, 1) != 1) continue;
        tcdrain(fd);
        latencies[ok++] = (long)(get_time_ns() - t0);
    }
    if (ok == 0) { printf("  no data\n\n"); close(fd); return; }

    qsort(latencies, ok, sizeof(long), cmp_long);

    double hw_us = BYTE_TIME_NS / 1000.0;
    long sum = 0, min = latencies[0], max = latencies[ok - 1];
    for (int i = 0; i < ok; i++) sum += latencies[i];
    double avg_us = (double)sum / ok / 1000.0;
    double sd_us  = stddev(latencies, ok, (double)sum / ok) / 1000.0;

    printf("  1-byte hardware time: %.1f us\n", hw_us);
    printf("  %6s  %8s  %8s  %8s  %8s  %8s  %8s  %8s\n",
           "n", "min", "max", "avg", "stddev", "P50", "P95", "P99");
    printf("  %6d  %5.0f us  %5.0f us  %5.1f us  %5.1f us  %5.1f us  %5.1f us  %5.1f us\n",
           ok, min / 1000.0, max / 1000.0, avg_us, sd_us,
           percentile(latencies, ok, 50) / 1000.0,
           percentile(latencies, ok, 95) / 1000.0,
           percentile(latencies, ok, 99) / 1000.0);
    printf("  overhead = %.1f - %.1f = %.1f us\n\n",
           avg_us, hw_us, avg_us > hw_us ? avg_us - hw_us : 0.0);
    close(fd);
}

/* ── non-blocking read ───────────────────────────────────────────── */
static void test_nonblock_read(void) {
    printf("=== Non-blocking Read (FIONBIO) ===\n");

    int fd = open(DEVICE_PATH, O_RDWR | O_NONBLOCK);
    if (fd < 0) { perror("open"); return; }
    char buf[16];
    ssize_t n = read(fd, buf, sizeof(buf));
    printf("  O_NONBLOCK open: %s\n",
           (n == -1 && (errno == EAGAIN || errno == EWOULDBLOCK))
               ? "PASS (EAGAIN)" : (n >= 0 ? "INFO (data buffered)" : "FAIL"));
    close(fd);

    fd = open(DEVICE_PATH, O_RDWR);
    if (fd < 0) { perror("open"); return; }
    int on = 1;
    ioctl(fd, FIONBIO, &on);
    n = read(fd, buf, sizeof(buf));
    printf("  ioctl FIONBIO:   %s\n",
           (n == -1 && (errno == EAGAIN || errno == EWOULDBLOCK))
               ? "PASS (EAGAIN)" : (n >= 0 ? "INFO (data buffered)" : "FAIL"));
    close(fd);
    printf("\n");
}

int main(void) {
    printf("UART Async E2E Benchmark  @ %.0f bps  (%.0f us/byte hardware)\n",
           BAUD_BPS, BYTE_TIME_NS / 1000.0);
    printf("===============================================================\n\n");
    test_e2e_throughput();
    test_e2e_latency();
    test_nonblock_read();
    printf("Done.\n");
    return 0;
}
