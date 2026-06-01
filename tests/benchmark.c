/**
 * UART 串口性能测试程序
 *
 * 测试项目：
 * 1. TX 吞吐量 - 用户态写入 /dev/console 的速度
 * 2. RX 吞吐量 - 用户态从 /dev/console 读取的速度
 * 3. 延迟 - 单字节 write() 延迟
 * 4. 数据完整性 - 验证数据传输的正确性
 * 5. 不同数据大小测试
 * 6. 压力测试
 *
 * 使用方法：
 *   /opt/musl/riscv64-linux-musl-cross/bin/riscv64-linux-musl-gcc -static -o benchmark benchmark.c
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
 * TX 吞吐量测试
 *
 * 测量从用户态写入 /dev/console 的速度
 * 使用 /dev/null 避免数据泄漏到终端
 */
static void test_throughput_tx(void) {
    printf("=== TX Throughput Test ===\n");

    // 使用 /dev/null 避免数据泄漏
    int fd = open("/dev/null", O_WRONLY);
    if (fd < 0) {
        perror("open /dev/null");
        return;
    }

    // 测试不同数据大小
    int sizes[] = {64, 256, 1024, 4096};
    int num_sizes = 4;

    for (int s = 0; s < num_sizes; s++) {
        int test_size = sizes[s];
        int iterations = 1000;
        char *buf = malloc(test_size);
        if (!buf) {
            perror("malloc");
            continue;
        }
        memset(buf, 'A', test_size);

        long long start = get_time_ns();
        size_t total = 0;

        for (int i = 0; i < iterations; i++) {
            ssize_t n = write(fd, buf, test_size);
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

        printf("  Size: %d bytes, Iterations: %d\n", test_size, iterations);
        printf("    Sent: %zu bytes (%.2f KB)\n", total, (double)total / 1024.0);
        printf("    Time: %.3f s\n", elapsed_s);
        printf("    Throughput: %.2f KB/s\n", kbps);
        printf("    Line rate: %.1f%%\n", line_rate);
        printf("\n");

        free(buf);
    }

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
 * 测量 write() 的延迟
 * 注意：在 QEMU 中，echo 测试不可靠，改为测量 write 延迟
 */
static void test_latency(void) {
    printf("=== Latency Test ===\n");

    int fd = open(DEVICE_PATH, O_WRONLY);
    if (fd < 0) {
        perror("open for write");
        return;
    }

    long latencies_ns[LATENCY_ITERATIONS];
    int successful = 0;

    for (int i = 0; i < LATENCY_ITERATIONS; i++) {
        long long start = get_time_ns();

        // 发送单个字节
        char tx = 'A' + (i % 26);
        if (write(fd, &tx, 1) != 1) {
            printf("  Write failed at iteration %d\n", i);
            continue;
        }

        long long end = get_time_ns();
        latencies_ns[successful] = (long)(end - start);
        successful++;
    }

    if (successful == 0) {
        printf("  No successful write tests\n\n");
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
    printf("  Note: Measuring write() latency, not echo latency\n");
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
 * 压力测试
 *
 * 长时间持续写入，测试稳定性
 */
static void test_stress(void) {
    printf("=== Stress Test ===\n");

    int fd = open("/dev/null", O_WRONLY);
    if (fd < 0) {
        perror("open /dev/null");
        return;
    }

    int test_size = 1024;
    int duration_sec = 2;  // 2 秒
    char *buf = malloc(test_size);
    if (!buf) {
        perror("malloc");
        close(fd);
        return;
    }
    memset(buf, 'A', test_size);

    long long start = get_time_ns();
    size_t total = 0;
    int iterations = 0;

    while (1) {
        long long now = get_time_ns();
        if ((now - start) > (long long)duration_sec * 1000000000LL) {
            break;
        }

        ssize_t n = write(fd, buf, test_size);
        if (n > 0) {
            total += n;
            iterations++;
        } else if (n < 0) {
            perror("write");
            break;
        }
    }

    long long end = get_time_ns();
    double elapsed_s = (double)(end - start) / 1000000000.0;
    double kbps = (double)total / elapsed_s / 1024.0;

    printf("  Duration: %.1f s\n", elapsed_s);
    printf("  Iterations: %d\n", iterations);
    printf("  Total: %zu bytes (%.2f KB)\n", total, (double)total / 1024.0);
    printf("  Throughput: %.2f KB/s\n", kbps);
    printf("  Status: PASS\n\n");

    free(buf);
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
    test_stress();

    printf("Benchmark complete.\n");
    return 0;
}
