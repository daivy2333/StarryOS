/*
 * ms01_loopback_diagnostic — layered single-hart QEMU witness for the
 * MS06 Iteration 001 loopback stall (Cycle 001-rework, T2.5-R1).
 *
 * The original MS01 first case printed only a global START and stopped
 * inside some blocking socket call, so it could not say whether the
 * failure was client connect, client send, parent accept or parent recv.
 * This payload splits that path into two deadline-bounded modes and flushes
 * a unique phase marker before every wait:
 *
 *   single  — one process: listener, nonblocking connect + poll, accept,
 *             payload round-trip. Proves loopback + resident runner +
 *             socket readiness without any fork interaction.
 *   fork    — parent listener, forked blocking child connect/send, parent
 *             accept/recv. Preserves the original MS01 first-case shape.
 *
 * Every mode prints:
 *   MS01_LOOPBACK_DIAGNOSTIC_START <mode>
 *   PHASE: <name>            (flushed before every wait)
 *   PASS: <case>             or   FAIL: <case> <reason>
 *   MS01_LOOPBACK_DIAGNOSTIC_END <mode>
 * and exits 0 only when the case PASSes inside the fixed total deadline.
 * The payload never calls any axnet-internal poll; socket waits use poll(2)
 * or SO_SNDTIMEO/SO_RCVTIMEO.
 *
 * Build: riscv64-linux-musl-gcc -static -O2 -o tests/ms01_loopback_diagnostic \
 *            tests/ms01_loopback_diagnostic.c
 */

#define _GNU_SOURCE
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <poll.h>
#include <signal.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define DIAG_PORT 18601
#define DIAG_TOTAL_DEADLINE_US 15000000u /* 15 s, fixed total bound */
#define DIAG_STEP_TIMEOUT_MS 3000
#define DIAG_PAYLOAD "tcp-ms01"
#define DIAG_PAYLOAD_LEN 8

static volatile int _failed = 0;

static void marker(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    vfprintf(stdout, fmt, ap);
    va_end(ap);
    fflush(stdout);
}

#define PHASE(name) marker("PHASE: %s\n", name)

static uint64_t now_us(void) {
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) return 0;
    return (uint64_t)ts.tv_sec * 1000000u + (uint64_t)ts.tv_nsec / 1000u;
}

static int deadline_remaining_ms(uint64_t start_us) {
    uint64_t now = now_us();
    if (now < start_us) return (int)(DIAG_TOTAL_DEADLINE_US / 1000u);
    if (now - start_us >= DIAG_TOTAL_DEADLINE_US) return 0;
    return (int)((DIAG_TOTAL_DEADLINE_US - (now - start_us)) / 1000u);
}

static void set_timeval_timeout(int fd, int opt, int ms) {
    struct timeval tv;
    tv.tv_sec = ms / 1000;
    tv.tv_usec = (ms % 1000) * 1000;
    setsockopt(fd, SOL_SOCKET, opt, &tv, sizeof(tv));
}

static void set_nonblock(int fd) {
    int flags = fcntl(fd, F_GETFL, 0);
    if (flags >= 0) fcntl(fd, F_SETFL, flags | O_NONBLOCK);
}

static int listen_socket(uint16_t port) {
    int srv = socket(AF_INET, SOCK_STREAM, 0);
    if (srv < 0) return -1;
    int opt = 1;
    setsockopt(srv, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt));
    struct sockaddr_in addr = {
        .sin_family = AF_INET,
        .sin_port = htons(port),
        .sin_addr.s_addr = INADDR_ANY,
    };
    if (bind(srv, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        close(srv);
        return -1;
    }
    if (listen(srv, 5) < 0) {
        close(srv);
        return -1;
    }
    return srv;
}

/* ─── single mode: nonblocking connect + poll, accept, payload ─── */

static int run_single(uint64_t t0) {
    PHASE("single-listen");
    int srv = listen_socket(DIAG_PORT);
    if (srv < 0) {
        marker("FAIL: single-loopback listen: %s\n", strerror(errno));
        return 1;
    }

    PHASE("single-connect");
    int cli = socket(AF_INET, SOCK_STREAM, 0);
    if (cli < 0) {
        marker("FAIL: single-loopback socket: %s\n", strerror(errno));
        close(srv);
        return 1;
    }
    set_nonblock(cli);
    struct sockaddr_in addr = { .sin_family = AF_INET, .sin_port = htons(DIAG_PORT) };
    inet_pton(AF_INET, "127.0.0.1", &addr.sin_addr);
    int rc = connect(cli, (struct sockaddr *)&addr, sizeof(addr));
    if (rc < 0 && errno != EINPROGRESS) {
        marker("FAIL: single-loopback connect: %s\n", strerror(errno));
        close(cli);
        close(srv);
        return 1;
    }

    PHASE("single-connect-poll");
    struct pollfd pfd = { .fd = cli, .events = POLLOUT };
    int pr = poll(&pfd, 1, deadline_remaining_ms(t0));
    if (pr <= 0 || !(pfd.revents & POLLOUT)) {
        marker("FAIL: single-loopback connect-poll pr=%d revents=0x%x\n",
               pr, pfd.revents);
        close(cli);
        close(srv);
        return 1;
    }

    PHASE("single-accept-poll");
    struct pollfd spfd = { .fd = srv, .events = POLLIN };
    pr = poll(&spfd, 1, deadline_remaining_ms(t0));
    if (pr <= 0 || !(spfd.revents & POLLIN)) {
        marker("FAIL: single-loopback accept-poll pr=%d revents=0x%x\n",
               pr, spfd.revents);
        close(cli);
        close(srv);
        return 1;
    }
    PHASE("single-accept");
    int acc = accept(srv, NULL, NULL);
    if (acc < 0) {
        marker("FAIL: single-loopback accept: %s\n", strerror(errno));
        close(cli);
        close(srv);
        return 1;
    }
    set_timeval_timeout(acc, SO_RCVTIMEO, DIAG_STEP_TIMEOUT_MS);
    set_timeval_timeout(cli, SO_SNDTIMEO, DIAG_STEP_TIMEOUT_MS);

    PHASE("single-send");
    ssize_t ns = send(cli, DIAG_PAYLOAD, DIAG_PAYLOAD_LEN, 0);
    if (ns != DIAG_PAYLOAD_LEN) {
        marker("FAIL: single-loopback send ns=%zd: %s\n", ns, strerror(errno));
        close(acc);
        close(cli);
        close(srv);
        return 1;
    }

    PHASE("single-recv");
    struct pollfd apfd = { .fd = acc, .events = POLLIN };
    pr = poll(&apfd, 1, deadline_remaining_ms(t0));
    if (pr <= 0 || !(apfd.revents & POLLIN)) {
        marker("FAIL: single-loopback recv-poll pr=%d revents=0x%x\n",
               pr, apfd.revents);
        close(acc);
        close(cli);
        close(srv);
        return 1;
    }
    char buf[DIAG_PAYLOAD_LEN + 1] = {0};
    ssize_t nr = recv(acc, buf, DIAG_PAYLOAD_LEN, 0);
    if (nr != DIAG_PAYLOAD_LEN ||
        memcmp(buf, DIAG_PAYLOAD, DIAG_PAYLOAD_LEN) != 0) {
        marker("FAIL: single-loopback recv nr=%zd buf='%s'\n", nr, buf);
        close(acc);
        close(cli);
        close(srv);
        return 1;
    }

    marker("PASS: single-loopback\n");
    close(acc);
    close(cli);
    close(srv);
    return 0;
}

/* ─── fork mode: parent listener, blocking child connect/send ─── */

static int run_fork(uint64_t t0) {
    PHASE("fork-listen");
    int srv = listen_socket(DIAG_PORT);
    if (srv < 0) {
        marker("FAIL: fork-loopback listen: %s\n", strerror(errno));
        return 1;
    }

    PHASE("fork-child-spawn");
    pid_t pid = fork();
    if (pid < 0) {
        marker("FAIL: fork-loopback fork: %s\n", strerror(errno));
        close(srv);
        return 1;
    }

    if (pid == 0) {
        PHASE("fork-child-connect");
        int fd = socket(AF_INET, SOCK_STREAM, 0);
        if (fd < 0) {
            marker("FAIL: fork-loopback child socket: %s\n", strerror(errno));
            _exit(1);
        }
        set_timeval_timeout(fd, SO_SNDTIMEO, DIAG_STEP_TIMEOUT_MS);
        struct sockaddr_in addr = { .sin_family = AF_INET,
                                    .sin_port = htons(DIAG_PORT) };
        inet_pton(AF_INET, "127.0.0.1", &addr.sin_addr);
        if (connect(fd, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
            marker("FAIL: fork-loopback child connect: %s\n", strerror(errno));
            _exit(1);
        }
        PHASE("fork-child-send");
        ssize_t ns = send(fd, DIAG_PAYLOAD, DIAG_PAYLOAD_LEN, 0);
        if (ns != DIAG_PAYLOAD_LEN) {
            marker("FAIL: fork-loopback child send ns=%zd: %s\n", ns,
                   strerror(errno));
            _exit(1);
        }
        PHASE("fork-child-done");
        close(fd);
        _exit(0);
    }

    PHASE("fork-parent-accept-poll");
    struct pollfd spfd = { .fd = srv, .events = POLLIN };
    int pr = poll(&spfd, 1, deadline_remaining_ms(t0));
    if (pr <= 0 || !(spfd.revents & POLLIN)) {
        marker("FAIL: fork-loopback accept-poll pr=%d revents=0x%x\n",
               pr, spfd.revents);
        kill(pid, SIGKILL);
        waitpid(pid, NULL, 0);
        close(srv);
        return 1;
    }
    PHASE("fork-parent-accept");
    int acc = accept(srv, NULL, NULL);
    if (acc < 0) {
        marker("FAIL: fork-loopback accept: %s\n", strerror(errno));
        kill(pid, SIGKILL);
        waitpid(pid, NULL, 0);
        close(srv);
        return 1;
    }
    set_timeval_timeout(acc, SO_RCVTIMEO, DIAG_STEP_TIMEOUT_MS);

    PHASE("fork-parent-recv");
    char buf[DIAG_PAYLOAD_LEN + 1] = {0};
    ssize_t nr = recv(acc, buf, DIAG_PAYLOAD_LEN, 0);
    if (nr != DIAG_PAYLOAD_LEN ||
        memcmp(buf, DIAG_PAYLOAD, DIAG_PAYLOAD_LEN) != 0) {
        marker("FAIL: fork-loopback recv nr=%zd buf='%s'\n", nr, buf);
        kill(pid, SIGKILL);
        waitpid(pid, NULL, 0);
        close(acc);
        close(srv);
        return 1;
    }

    int status = 0;
    waitpid(pid, &status, 0);
    if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
        marker("FAIL: fork-loopback child status=0x%x\n", status);
        close(acc);
        close(srv);
        return 1;
    }

    marker("PASS: fork-loopback\n");
    close(acc);
    close(srv);
    return 0;
}

int main(int argc, char **argv) {
    if (argc != 2 || (strcmp(argv[1], "single") != 0 &&
                      strcmp(argv[1], "fork") != 0)) {
        fprintf(stderr, "usage: %s single|fork\n", argv[0]);
        return 2;
    }
    const char *mode = argv[1];

    marker("MS01_LOOPBACK_DIAGNOSTIC_START %s\n", mode);
    uint64_t t0 = now_us();
    int rc = 0;
    if (strcmp(mode, "single") == 0) {
        rc = run_single(t0);
    } else {
        rc = run_fork(t0);
    }
    if (rc != 0) _failed = 1;
    marker("MS01_LOOPBACK_DIAGNOSTIC_END %s\n", mode);
    return _failed ? 1 : 0;
}