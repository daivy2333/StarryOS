/*
 * k3_stdio_probe — MS17 Iteration 002 (task T6) board qualification payload.
 *
 * A static freestanding-user program spawned on the K3 memory root with
 * stdio bound to the async tty. Each stage prints its own marker line; the
 * operator adjudicates from the serial wire (the kernel never judges PASS):
 *
 *   stage=argv       argc/argv as passed by the kernel
 *   stage=eagain     FIONBIO=1 makes read return EAGAIN
 *   stage=line       one full blocking line read, echoed back
 *   stage=ppoll      ppoll wait for the second line, then echo
 *   stage=sleep      nanosleep 300 ms (timer alive under the tty)
 *   stage=eof        read returns 0 on VEOF (Ctrl-D), then exit(42)
 *
 * Build (see Makefile k3-stdio-elf):
 *   riscv64-linux-musl-gcc -static -no-pie -fno-pie -Os -s
 */

/* _GNU_SOURCE exposes ppoll on musl. */
#define _GNU_SOURCE

#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <string.h>
#include <sys/ioctl.h>
#include <time.h>
#include <unistd.h>

#define LINE_BUF 512

static void put(const char *s) {
    size_t len = strlen(s);
    ssize_t w = write(1, s, len);
    (void)w;
}

static void put_line(const char *s) {
    put(s);
    put("\n");
}

static void put_u32(const char *label, unsigned long v) {
    char buf[64];
    size_t n = 0;
    const char *hex = "0123456789abcdef";
    if (v == 0) {
        buf[n++] = '0';
    } else {
        char tmp[16];
        size_t t = 0;
        while (v > 0) {
            tmp[t++] = hex[v & 0xf];
            v >>= 4;
        }
        while (t > 0) {
            buf[n++] = tmp[--t];
        }
    }
    buf[n] = 0;
    put(label);
    put(buf);
    put("\n");
}

/* Blocking full-line read: completes on '\n' or when the buffer is full. */
static long read_line(char *buf, long cap) {
    long total = 0;
    while (total < cap) {
        long n = read(0, buf + total, 1);
        if (n < 0) {
            return -1;
        }
        if (n == 0) {
            return total;
        }
        total += n;
        if (buf[total - 1] == '\n') {
            break;
        }
    }
    return total;
}

int main(int argc, char **argv) {
    int on = 1;
    int off = 0;
    char buf[LINE_BUF];
    long n;

    put_line("[probe] stage=argv begin");
    put_u32("[probe] argc=", (unsigned long)argc);
    for (int i = 0; i < argc; i++) {
        put("[probe] argv=");
        put_line(argv[i]);
    }

    /* FIONBIO: non-blocking read must fail with EAGAIN, not stall. */
    if (ioctl(0, FIONBIO, &on) != 0) {
        put_line("[probe] fault stage=eagain ioctl-set");
        return 1;
    }
    errno = 0;
    n = read(0, buf, sizeof(buf));
    if (!(n < 0 && errno == EAGAIN)) {
        put_line("[probe] fault stage=eagain expected-EAGAIN");
        return 1;
    }
    put_line("[probe] stage=eagain result=EAGAIN");
    if (ioctl(0, FIONBIO, &off) != 0) {
        put_line("[probe] fault stage=eagain ioctl-clear");
        return 1;
    }

    /* One full line, echoed back. */
    put_line("[probe] stage=line send-one-line");
    n = read_line(buf, sizeof(buf));
    if (n <= 0) {
        put_line("[probe] fault stage=line read");
        return 1;
    }
    put("[probe] stage=line echo=");
    ssize_t w = write(1, buf, (size_t)n);
    (void)w;

    /* ppoll: wait until the second line is readable, then read it. */
    put_line("[probe] stage=ppoll send-another-line (30s window)");
    struct pollfd pfd;
    pfd.fd = 0;
    pfd.events = POLLIN;
    pfd.revents = 0;
    struct timespec to = { .tv_sec = 30, .tv_nsec = 0 };
    int ready = ppoll(&pfd, 1, &to, NULL);
    if (ready != 1 || !(pfd.revents & POLLIN)) {
        put_line("[probe] fault stage=ppoll not-readable");
        return 1;
    }
    n = read_line(buf, sizeof(buf));
    if (n <= 0) {
        put_line("[probe] fault stage=ppoll read");
        return 1;
    }
    put("[probe] stage=ppoll echo=");
    w = write(1, buf, (size_t)n);
    (void)w;

    /* nanosleep: the timer runtime keeps working under the tty. */
    put_line("[probe] stage=sleep begin=300ms");
    struct timespec req = { .tv_sec = 0, .tv_nsec = 300 * 1000 * 1000 };
    struct timespec rem;
    while (nanosleep(&req, &rem) != 0 && errno == EINTR) {
        req = rem;
    }
    put_line("[probe] stage=sleep done");

    /* VEOF: a bare Ctrl-D makes the canonical read return 0 once.
     * Queued lines precede the EOF event by contract (data first), so any
     * positive read here is leftover input (e.g. an Enter after Ctrl-D):
     * report it and retry; the FIRST 0-read is the verdict. */
    put_line("[probe] stage=eof send-ctrl-d (press Ctrl-D alone, no Enter)");
    for (int tries = 0; tries < 8; tries++) {
        errno = 0;
        n = read(0, buf, sizeof(buf));
        if (n == 0) {
            put_line("[probe] stage=eof result=read-0");
            break;
        }
        if (n < 0) {
            put_u32("[probe] fault stage=eof errno=", (unsigned long)errno);
            put_line("[probe] fault stage=eof read-error");
            return 1;
        }
        put_u32("[probe] stage=eof leftover bytes=", (unsigned long)n);
        ssize_t wl = write(1, buf, (size_t)n);
        (void)wl;
    }
    if (n != 0) {
        put_line("[probe] fault stage=eof expected-0-read (still no EOF after retries)");
        return 1;
    }
    put_line("[probe] stage=eof result=read-0");

    put_line("[probe] stage=exit code=42");
    return 42;
}
