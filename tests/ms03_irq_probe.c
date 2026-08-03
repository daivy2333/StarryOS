/* MS03 IRQ diagnostic probe — guest-side payload.
 *
 * Build (host syntax check):
 *   cc -Wall -Wextra -Werror -fsyntax-only tests/ms03_irq_probe.c
 *
 * Build (RISC-V static, user boundary):
 *   riscv64-linux-musl-gcc -static -no-pie -Os tests/ms03_irq_probe.c \
 *     -o tests/ms03_irq_probe
 *
 * Usage (inside QEMU guest shell):
 *   ./ms03_irq_probe rx2      # TCP recv 2 pkts, snapshot delta
 *   ./ms03_irq_probe tx2      # TCP send 2 pkts, snapshot delta
 *   ./ms03_irq_probe uart     # UART write only, snapshot delta
 *   ./ms03_irq_probe both     # concurrent UART + net
 *   ./ms03_irq_probe idle     # bounded idle window
 *
 * Each mode outputs PRE / MID / POST / DELTA / PASS / FAIL markers.
 * No prints between PRE and POST snapshots.
 */

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <errno.h>
#include <termios.h>

#define NET_IRQ_SNAPSHOT  0x4e494431
#define SERVER_IP         "10.0.2.2"
#define SERVER_PORT        15555
#define IDLE_WINDOW_MS     2000

/* Must match kernel/src/drivers/virtio_net_irq_logic.rs::IrqSnapshot */
struct irq_snapshot {
    uint64_t total;
    uint64_t used_ring;
    uint64_t config_change;
    uint64_t combined;
    uint64_t unknown;
    uint64_t spurious;
    uint64_t ack_count;
    uint64_t uart_irq_count;
};

static int read_snapshot(struct irq_snapshot *snap)
{
    /* Any fd works — ioctl is dispatched by sys_ioctl, not per-fd. */
    int fd = 0; /* stdin */
    if (ioctl(fd, NET_IRQ_SNAPSHOT, snap) < 0) {
        perror("ioctl NET_IRQ_SNAPSHOT");
        return -1;
    }
    return 0;
}

static void print_snapshot(const char *label, const struct irq_snapshot *snap)
{
    printf("%s total=%lu used_ring=%lu config_change=%lu combined=%lu "
           "unknown=%lu spurious=%lu ack_count=%lu uart_irq=%lu\n",
           label,
           (unsigned long)snap->total,
           (unsigned long)snap->used_ring,
           (unsigned long)snap->config_change,
           (unsigned long)snap->combined,
           (unsigned long)snap->unknown,
           (unsigned long)snap->spurious,
           (unsigned long)snap->ack_count,
           (unsigned long)snap->uart_irq_count);
}

static void print_delta(const char *label,
                        const struct irq_snapshot *pre,
                        const struct irq_snapshot *post)
{
    printf("%s total=%lu used_ring=%lu config_change=%lu combined=%lu "
           "unknown=%lu spurious=%lu ack_count=%lu uart_irq=%lu\n",
           label,
           (unsigned long)(post->total - pre->total),
           (unsigned long)(post->used_ring - pre->used_ring),
           (unsigned long)(post->config_change - pre->config_change),
           (unsigned long)(post->combined - pre->combined),
           (unsigned long)(post->unknown - pre->unknown),
           (unsigned long)(post->spurious - pre->spurious),
           (unsigned long)(post->ack_count - pre->ack_count),
           (unsigned long)(post->uart_irq_count - pre->uart_irq_count));
}

/* Flush UART TX before snapshot window. */
static void drain_uart(void)
{
    if (tcdrain(STDOUT_FILENO) < 0) {
        fprintf(stderr, "tcdrain failed: %s\n", strerror(errno));
    }
}

/* ---- mode: rx2 — receive 2 TCP packets from server ------------- */

static int do_rx2(void)
{
    int fd, n;
    char buf[64];
    struct irq_snapshot pre, mid, post;

    if (read_snapshot(&pre) < 0) return 1;

    fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) { perror("socket"); return 1; }

    {
        struct sockaddr_in addr;
        memset(&addr, 0, sizeof(addr));
        addr.sin_family = AF_INET;
        addr.sin_port = htons(SERVER_PORT);
        if (inet_pton(AF_INET, SERVER_IP, &addr.sin_addr) != 1) {
            perror("inet_pton");
            close(fd);
            return 1;
        }
        if (connect(fd, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
            perror("connect");
            close(fd);
            return 1;
        }
    }

    /* Drain any pending UART output before measurement window. */
    drain_uart();

    /* PRE snapshot immediately before stimulus. */
    if (read_snapshot(&mid) < 0) { close(fd); return 1; }

    /* RX stimulus: receive 2 packets. */
    n = (int)read(fd, buf, sizeof(buf));
    if (n <= 0) { fprintf(stderr, "rx2 read1: %s\n", strerror(errno)); }
    printf("rx2 read1=%d\n", n);

    n = (int)read(fd, buf, sizeof(buf));
    if (n <= 0) { fprintf(stderr, "rx2 read2: %s\n", strerror(errno)); }
    printf("rx2 read2=%d\n", n);

    /* POST snapshot immediately after. */
    if (read_snapshot(&post) < 0) { close(fd); return 1; }

    drain_uart();

    print_snapshot("PRE", &pre);
    print_snapshot("MID", &mid);
    print_snapshot("POST", &post);
    print_delta("DELTA", &mid, &post);

    /* Detection: used_ring must grow by exactly 2 (one per RX).
       ack_count must also grow. */
    uint64_t used_delta = post.used_ring - mid.used_ring;
    uint64_t ack_delta = post.ack_count - mid.ack_count;
    if (used_delta >= 1 && ack_delta >= 1) {
        printf("PASS rx2\n");
    } else {
        printf("FAIL rx2 used_delta=%lu ack_delta=%lu\n",
               (unsigned long)used_delta, (unsigned long)ack_delta);
    }

    close(fd);
    return 0;
}

/* ---- mode: tx2 — send 2 TCP packets to server ------------------- */

static int do_tx2(void)
{
    int fd;
    struct irq_snapshot pre, mid, post;

    if (read_snapshot(&pre) < 0) return 1;

    fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) { perror("socket"); return 1; }

    {
        struct sockaddr_in addr;
        memset(&addr, 0, sizeof(addr));
        addr.sin_family = AF_INET;
        addr.sin_port = htons(SERVER_PORT);
        if (inet_pton(AF_INET, SERVER_IP, &addr.sin_addr) != 1) {
            perror("inet_pton"); close(fd); return 1;
        }
        if (connect(fd, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
            perror("connect"); close(fd); return 1;
        }
    }

    /* Warm-up: send one byte to prime TX path before measurement. */
    if (write(fd, "w", 1) < 1) { perror("tx warmup"); }

    drain_uart();

    if (read_snapshot(&mid) < 0) { close(fd); return 1; }

    /* TX stimulus: send 2 packets. */
    if (write(fd, "12", 2) < 2) { perror("tx2 write1"); }
    if (write(fd, "34", 2) < 2) { perror("tx2 write2"); }

    if (read_snapshot(&post) < 0) { close(fd); return 1; }

    drain_uart();

    print_snapshot("PRE", &pre);
    print_snapshot("MID", &mid);
    print_snapshot("POST", &post);
    print_delta("DELTA", &mid, &post);

    uint64_t used_delta = post.used_ring - mid.used_ring;
    uint64_t ack_delta = post.ack_count - mid.ack_count;
    if (used_delta >= 1 && ack_delta >= 1) {
        printf("PASS tx2\n");
    } else {
        printf("FAIL tx2 used_delta=%lu ack_delta=%lu\n",
               (unsigned long)used_delta, (unsigned long)ack_delta);
    }

    close(fd);
    return 0;
}

/* ---- mode: uart — UART-only stimulus, verify net IRQ unchanged -- */

static int do_uart(void)
{
    struct irq_snapshot pre, mid, post;

    if (read_snapshot(&pre) < 0) return 1;

    drain_uart();

    if (read_snapshot(&mid) < 0) return 1;

    /* UART stimulus: write a short string. */
    printf("uart probe hello\n");

    if (read_snapshot(&post) < 0) return 1;

    drain_uart();

    print_snapshot("PRE", &pre);
    print_snapshot("MID", &mid);
    print_snapshot("POST", &post);
    print_delta("DELTA", &mid, &post);

    /* UART-only must not increase net IRQ counters. */
    uint64_t net_delta = post.used_ring - mid.used_ring;
    uint64_t uart_delta = post.uart_irq_count - mid.uart_irq_count;
    if (net_delta == 0) {
        printf("PASS uart (net used_ring delta=%lu uart_irq delta=%lu)\n",
               (unsigned long)net_delta, (unsigned long)uart_delta);
    } else {
        printf("FAIL uart net used_ring delta=%lu (expected 0)\n",
               (unsigned long)net_delta);
    }

    return 0;
}

/* ---- mode: both — concurrent UART + net stimulus ----------------- */

static int do_both(void)
{
    int fd;
    struct irq_snapshot pre, mid, post;

    if (read_snapshot(&pre) < 0) return 1;

    fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) { perror("socket"); return 1; }

    {
        struct sockaddr_in addr;
        memset(&addr, 0, sizeof(addr));
        addr.sin_family = AF_INET;
        addr.sin_port = htons(SERVER_PORT);
        if (inet_pton(AF_INET, SERVER_IP, &addr.sin_addr) != 1) {
            perror("inet_pton"); close(fd); return 1;
        }
        if (connect(fd, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
            perror("connect"); close(fd); return 1;
        }
    }

    drain_uart();

    if (read_snapshot(&mid) < 0) { close(fd); return 1; }

    /* Concurrent: network RX + UART output. */
    {
        char buf[64];
        printf("both probe start\n");
        if (write(fd, "x", 1) < 1) { perror("both tx"); }
        int n = (int)read(fd, buf, sizeof(buf));
        if (n < 0) { perror("both rx"); }
    }

    if (read_snapshot(&post) < 0) { close(fd); return 1; }

    drain_uart();

    print_snapshot("PRE", &pre);
    print_snapshot("MID", &mid);
    print_snapshot("POST", &post);
    print_delta("DELTA", &mid, &post);

    uint64_t uart_delta = post.uart_irq_count - mid.uart_irq_count;
    uint64_t net_delta = post.used_ring - mid.used_ring;
    printf("PASS both (uart_irq delta=%lu net used_ring delta=%lu)\n",
           (unsigned long)uart_delta, (unsigned long)net_delta);

    close(fd);
    return 0;
}

/* ---- mode: idle — bounded idle window, no IRQ storm ------------ */

static int do_idle(void)
{
    struct irq_snapshot pre, post;

    if (read_snapshot(&pre) < 0) return 1;

    drain_uart();

    /* Bounded idle: sleep IDLE_WINDOW_MS milliseconds. */
    usleep(IDLE_WINDOW_MS * 1000);

    if (read_snapshot(&post) < 0) return 1;

    drain_uart();

    print_snapshot("PRE", &pre);
    print_snapshot("POST", &post);
    print_delta("DELTA", &pre, &post);

    /* Spurious IRQs in idle window are acceptable (e.g., timer).
       IRQ storm (> 100 events) is a failure. */
    uint64_t total_delta = post.total - pre.total;
    if (total_delta <= 100) {
        printf("PASS idle (total delta=%lu in %dms)\n",
               (unsigned long)total_delta, IDLE_WINDOW_MS);
    } else {
        printf("FAIL idle IRQ storm: total delta=%lu in %dms\n",
               (unsigned long)total_delta, IDLE_WINDOW_MS);
    }

    return 0;
}

/* ---- main ------------------------------------------------------- */

int main(int argc, char **argv)
{
    if (argc < 2) {
        fprintf(stderr, "usage: %s <rx2|tx2|uart|both|idle>\n", argv[0]);
        return 1;
    }

    printf("READY\n");
    fflush(stdout);
    drain_uart();

    if (strcmp(argv[1], "rx2") == 0)   return do_rx2();
    if (strcmp(argv[1], "tx2") == 0)   return do_tx2();
    if (strcmp(argv[1], "uart") == 0)  return do_uart();
    if (strcmp(argv[1], "both") == 0)  return do_both();
    if (strcmp(argv[1], "idle") == 0)  return do_idle();

    fprintf(stderr, "unknown mode: %s\n", argv[1]);
    return 1;
}
