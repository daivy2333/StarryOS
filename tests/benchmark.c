/**
 * UART Async Benchmark — 真实串口性能测试
 *
 * 修复项 (O44):
 * - TX throughput 改为写 /dev/console（而非 /dev/null，绕过 UART）
 * - TX 延迟加 tcdrain() 等待硬件发送完成
 * - 新增非阻塞模式测试 (FIONBIO)
 *
 * 编译:
 *   riscv64-linux-musl-gcc -static -o tests/benchmark tests/benchmark.c
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

static long long get_time_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (long long)ts.tv_sec * 1000000000LL + ts.tv_nsec;
}

/* ── TX throughput: 写 /dev/console + tcdrain ─────────────────────── */
static void test_tx_throughput(void) {
    printf("=== TX Throughput (to /dev/console + tcdrain) ===\n");

    int fd = open(DEVICE_PATH, O_WRONLY);
    if (fd < 0) { perror("open"); return; }

    int sizes[] = {64, 256, 1024, 4096};
    int num_sizes = 4;

    for (int s = 0; s < num_sizes; s++) {
        int test_size = sizes[s];
        int iterations = 100;
        char *buf = malloc(test_size);
        if (!buf) { perror("malloc"); continue; }
        memset(buf, 'A', test_size);

        long long start = get_time_ns();
        size_t total = 0;

        for (int i = 0; i < iterations; i++) {
            ssize_t n = write(fd, buf, test_size);
            if (n > 0) {
                total += n;
                tcdrain(fd);   /* wait until UART FIFO is empty */
            } else {
                break;
            }
        }

        long long end = get_time_ns();
        double elapsed_s = (double)(end - start) / 1000000000.0;
        double kbps = (double)total / elapsed_s / 1024.0;
        double line_rate = kbps / 11.52 * 100.0;  /* 115200 bps = 11.52 KB/s */

        printf("  size=%d  iters=%d | %.2f KB/s | %.1f%% line rate\n",
               test_size, iterations, kbps, line_rate);

        free(buf);
    }

    close(fd);
    printf("\n");
}

/* ── TX latency: 单字节 write + tcdrain ─────────────────────────── */
static void test_tx_latency(void) {
    printf("=== TX Latency (single byte + tcdrain) ===\n");

    int fd = open(DEVICE_PATH, O_WRONLY);
    if (fd < 0) { perror("open"); return; }

    #define LAT_N 100
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
    printf("  n=%d  avg=%.3f ms  P50=%.3f ms  P95=%.3f ms  P99=%.3f ms\n\n",
           ok,
           (double)sum / ok / 1000000.0,
           (double)latencies[ok * 50 / 100] / 1000000.0,
           (double)latencies[ok * 95 / 100] / 1000000.0,
           (double)latencies[ok * 99 / 100] / 1000000.0);

    close(fd);
}

/* ── non-blocking read test (FIONBIO) ───────────────────────────── */
static void test_nonblock_read(void) {
    printf("=== Non-blocking Read (FIONBIO) ===\n");

    int fd = open(DEVICE_PATH, O_RDWR | O_NONBLOCK);
    if (fd < 0) { perror("open"); return; }

    char buf[16];
    ssize_t n = read(fd, buf, sizeof(buf));

    if (n == -1 && (errno == EAGAIN || errno == EWOULDBLOCK)) {
        printf("  PASS: O_NONBLOCK read → EAGAIN (no data)\n");
    } else if (n >= 0) {
        printf("  INFO: read %zd bytes (data already in buffer)\n", n);
    } else {
        printf("  FAIL: errno=%d (%s)\n", errno, strerror(errno));
    }

    close(fd);

    /* test via ioctl */
    fd = open(DEVICE_PATH, O_RDWR);
    if (fd < 0) { perror("open"); return; }

    int on = 1;
    if (ioctl(fd, FIONBIO, &on) < 0) {
        printf("  FAIL: ioctl FIONBIO: %s\n", strerror(errno));
        close(fd);
        return;
    }

    n = read(fd, buf, sizeof(buf));
    if (n == -1 && (errno == EAGAIN || errno == EWOULDBLOCK)) {
        printf("  PASS: ioctl FIONBIO read → EAGAIN (no data)\n");
    } else if (n >= 0) {
        printf("  INFO: read %zd bytes (data already in buffer)\n", n);
    } else {
        printf("  FAIL: errno=%d (%s)\n", errno, strerror(errno));
    }

    close(fd);
    printf("\n");
}

/* ── 主函数 ──────────────────────────────────────────────────────── */
int main(void) {
    printf("UART Async Benchmark (QEMU @ 115200 bps)\n");
    printf("=========================================\n\n");

    test_tx_throughput();
    test_tx_latency();
    test_nonblock_read();

    printf("Done.\n");
    return 0;
}
