/**
 * UART 异步串口性能测试程序
 *
 * 测试项目:
 * 1. TX 吞吐量 - 测量发送数据的最大速率
 * 2. RX 吞吐量 - 测量接收数据的最大速率
 * 3. 延迟 - 测量单字节 echo 的端到端延迟
 * 4. 数据完整性 - 验证数据传输的正确性
 *
 * 使用方法:
 *   cc -o benchmark benchmark.c -lrt
 *   ./benchmark
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <fcntl.h>
#include <unistd.h>
#include <time.h>
#include <sys/stat.h>

#define DEVICE_PATH "/dev/console"
#define BUF_SIZE 1024
#define THROUGHPUT_TEST_SIZE (1024 * 1024)  // 1MB
#define LATENCY_ITERATIONS 100

/**
 * 获取当前时间（纳秒）
 */
static long long get_time_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (long long)ts.tv_sec * 1000000000LL + ts.tv_nsec;
}

/**
 * 吞吐量测试
 *
 * 测量 TX（发送）的最大吞吐量
 * 方法：发送 1MB 数据，测量总时间
 */
static void test_throughput_tx(void) {
    printf("=== TX Throughput Test ===\n");

    int fd = open(DEVICE_PATH, O_WRONLY);
    if (fd < 0) {
        perror("open for write");
        return;
    }

    char *buf = malloc(BUF_SIZE);
    if (!buf) {
        perror("malloc");
        close(fd);
        return;
    }
    memset(buf, 'A', BUF_SIZE);

    long long start = get_time_ns();
    size_t total = 0;

    while (total < THROUGHPUT_TEST_SIZE) {
        ssize_t n = write(fd, buf, BUF_SIZE);
        if (n > 0) {
            total += n;
        } else if (n < 0) {
            perror("write");
            break;
        }
    }

    long long end = get_time_ns();
    double elapsed_s = (double)(end - start) / 1000000000.0;
    double kbps = (double)total / elapsed_s / 1024.0;
    double line_rate = kbps / 11.52 * 100.0;  // 115200 bps = 11.52 KB/s

    printf("  Sent: %zu bytes (%.2f KB)\n", total, (double)total / 1024.0);
    printf("  Time: %.3f s\n", elapsed_s);
    printf("  Throughput: %.2f KB/s\n", kbps);
    printf("  Line rate: %.1f%%\n", line_rate);
    printf("  Status: %s\n\n", kbps >= 10.0 ? "PASS" : "FAIL");

    free(buf);
    close(fd);
}

/**
 * 吞吐量测试（RX）
 *
 * 注意：RX 吞吐量测试需要外部发送数据
 * 在 QEMU 环境中，可以通过 TCP 串口连接发送数据
 */
static void test_throughput_rx(void) {
    printf("=== RX Throughput Test ===\n");
    printf("  Note: RX test requires external data injection\n");
    printf("  Use QEMU TCP serial connection to send data\n\n");
}

/**
 * 延迟测试
 *
 * 测量单字节 echo 的端到端延迟
 * 方法：发送单个字节，等待接收相同字节，记录往返时间
 */
static void test_latency(void) {
    printf("=== Latency Test ===\n");

    int fd = open(DEVICE_PATH, O_RDWR);
    if (fd < 0) {
        perror("open for read/write");
        return;
    }

    long latencies_ns[LATENCY_ITERATIONS];
    int successful = 0;

    for (int i = 0; i < LATENCY_ITERATIONS; i++) {
        long long start = get_time_ns();

        char tx = 'A' + (i % 26);
        char rx = 0;

        // 发送单个字节
        if (write(fd, &tx, 1) != 1) {
            printf("  Write failed at iteration %d\n", i);
            continue;
        }

        // 等待接收（带超时）
        int attempts = 0;
        while (attempts < 1000) {
            if (read(fd, &rx, 1) == 1) {
                break;
            }
            attempts++;
        }

        long long end = get_time_ns();

        if (rx == tx) {
            latencies_ns[successful] = (long)(end - start);
            successful++;
        } else {
            printf("  Mismatch at iteration %d: sent %c, got %c\n", i, tx, rx);
        }
    }

    if (successful == 0) {
        printf("  No successful echo tests\n\n");
        close(fd);
        return;
    }

    // 计算统计值
    long sum = 0, min = latencies_ns[0], max = latencies_ns[0];
    for (int i = 0; i < successful; i++) {
        sum += latencies_ns[i];
        if (latencies_ns[i] < min) min = latencies_ns[i];
        if (latencies_ns[i] > max) max = latencies_ns[i];
    }

    // 排序计算百分位（简单冒泡排序）
    for (int i = 0; i < successful - 1; i++) {
        for (int j = 0; j < successful - i - 1; j++) {
            if (latencies_ns[j] > latencies_ns[j + 1]) {
                long temp = latencies_ns[j];
                latencies_ns[j] = latencies_ns[j + 1];
                latencies_ns[j + 1] = temp;
            }
        }
    }

    long p50 = latencies_ns[successful * 50 / 100];
    long p95 = latencies_ns[successful * 95 / 100];
    long p99 = latencies_ns[successful * 99 / 100];

    printf("  Iterations: %d (successful: %d)\n", LATENCY_ITERATIONS, successful);
    printf("  Min: %ld ns (%.3f ms)\n", min, (double)min / 1000000.0);
    printf("  Max: %ld ns (%.3f ms)\n", max, (double)max / 1000000.0);
    printf("  Avg: %ld ns (%.3f ms)\n", sum / successful, (double)(sum / successful) / 1000000.0);
    printf("  P50: %ld ns (%.3f ms)\n", p50, (double)p50 / 1000000.0);
    printf("  P95: %ld ns (%.3f ms)\n", p95, (double)p95 / 1000000.0);
    printf("  P99: %ld ns (%.3f ms)\n", p99, (double)p99 / 1000000.0);
    printf("  Status: %s\n\n", p99 < 2000000 ? "PASS" : "FAIL");

    close(fd);
}

/**
 * 数据完整性测试
 *
 * 验证数据传输的正确性
 * 方法：发送已知模式的数据，验证接收的数据是否一致
 */
static void test_data_integrity(void) {
    printf("=== Data Integrity Test ===\n");

    int fd = open(DEVICE_PATH, O_RDWR);
    if (fd < 0) {
        perror("open for read/write");
        return;
    }

    // 生成测试数据
    char tx_buf[256];
    for (int i = 0; i < 256; i++) {
        tx_buf[i] = (char)(i & 0xFF);
    }

    // 发送数据
    ssize_t written = write(fd, tx_buf, 256);
    if (written != 256) {
        printf("  Write failed: %zd bytes written\n", written);
        close(fd);
        return;
    }

    // 等待接收（带超时）
    char rx_buf[256];
    int total_read = 0;
    int attempts = 0;

    while (total_read < 256 && attempts < 10000) {
        ssize_t n = read(fd, rx_buf + total_read, 256 - total_read);
        if (n > 0) {
            total_read += n;
        }
        attempts++;
    }

    // 验证数据
    int errors = 0;
    for (int i = 0; i < total_read; i++) {
        if (rx_buf[i] != tx_buf[i]) {
            errors++;
            if (errors <= 5) {
                printf("  Mismatch at byte %d: sent %02x, got %02x\n",
                       i, tx_buf[i] & 0xFF, rx_buf[i] & 0xFF);
            }
        }
    }

    printf("  Sent: 256 bytes\n");
    printf("  Received: %d bytes\n", total_read);
    printf("  Errors: %d\n", errors);
    printf("  Status: %s\n\n", errors == 0 ? "PASS" : "FAIL");

    close(fd);
}

/**
 * 主函数
 */
int main(void) {
    printf("UART Async Benchmark\n");
    printf("====================\n\n");

    test_throughput_tx();
    test_throughput_rx();
    test_latency();
    test_data_integrity();

    printf("Benchmark complete.\n");
    return 0;
}
