/*
 * MS01 Socket Baseline — characterization witness for t01-smoltcp-axnet-baseline
 *
 * Runs against the existing StarryOS kernel binary via loopback.
 * Each test prints exactly "PASS: <name>" or "FAIL: <name> <reason>".
 * Exit code is 0 when all markers pass.
 *
 * Build: riscv64-linux-musl-gcc -static -o ms01_socket_baseline ms01_socket_baseline.c
 */

#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <fcntl.h>
#include <errno.h>
#include <sys/socket.h>
#include <poll.h>
#include <sys/wait.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <signal.h>

#define TEST_PORT_BASE 18001
#define PASS(fmt, ...) fprintf(stdout, "PASS: " fmt "\n", ##__VA_ARGS__)
#define FAIL(fmt, ...) do { \
    fprintf(stdout, "FAIL: " fmt "\n", ##__VA_ARGS__); \
    _failed = 1; \
} while (0)

static volatile int _failed = 0;

static void set_nonblock(int fd) {
    int flags = fcntl(fd, F_GETFL, 0);
    if (flags < 0) { perror("fcntl F_GETFL"); return; }
    fcntl(fd, F_SETFL, flags | O_NONBLOCK);
}

/* ─── 1. TCP bind/listen/accept basic round-trip ─── */

static void test_tcp_accept_roundtrip(void) {
    int port = TEST_PORT_BASE + 1;
    pid_t pid = fork();
    if (pid < 0) { FAIL("tcp-accept: fork failed: %s", strerror(errno)); return; }

    if (pid == 0) {
        /* client */
        sleep(1);
        int fd = socket(AF_INET, SOCK_STREAM, 0);
        if (fd < 0) { fprintf(stdout, "FAIL: tcp-accept: client socket: %s\n", strerror(errno)); _exit(1); }
        struct sockaddr_in addr = { .sin_family = AF_INET, .sin_port = htons(port) };
        inet_pton(AF_INET, "127.0.0.1", &addr.sin_addr);
        if (connect(fd, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
            fprintf(stdout, "FAIL: tcp-accept: client connect: %s\n", strerror(errno)); _exit(1);
        }
        const char *msg = "tcp-ms01";
        if (send(fd, msg, strlen(msg), 0) < 0) {
            fprintf(stdout, "FAIL: tcp-accept: client send: %s\n", strerror(errno)); _exit(1);
        }
        close(fd);
        _exit(0);
    }

    /* server */
    int srv = socket(AF_INET, SOCK_STREAM, 0);
    if (srv < 0) { FAIL("tcp-accept: server socket: %s", strerror(errno)); waitpid(pid, NULL, 0); return; }

    int opt = 1;
    setsockopt(srv, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt));

    struct sockaddr_in addr = { .sin_family = AF_INET, .sin_port = htons(port), .sin_addr.s_addr = INADDR_ANY };
    if (bind(srv, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        FAIL("tcp-accept: bind: %s", strerror(errno));
        close(srv); waitpid(pid, NULL, 0); return;
    }
    if (listen(srv, 5) < 0) {
        FAIL("tcp-accept: listen: %s", strerror(errno));
        close(srv); waitpid(pid, NULL, 0); return;
    }

    struct sockaddr_in peer;
    socklen_t peer_len = sizeof(peer);
    int cli = accept(srv, (struct sockaddr *)&peer, &peer_len);
    if (cli < 0) {
        FAIL("tcp-accept: accept: %s", strerror(errno));
        close(srv); waitpid(pid, NULL, 0); return;
    }

    char buf[64] = {0};
    ssize_t n = recv(cli, buf, sizeof(buf) - 1, 0);
    if (n < 0) {
        FAIL("tcp-accept: recv: %s", strerror(errno));
    } else if (strcmp(buf, "tcp-ms01") != 0) {
        FAIL("tcp-accept: expected 'tcp-ms01', got '%s'", buf);
    } else {
        PASS("tcp-accept");
    }

    close(cli);
    close(srv);
    waitpid(pid, NULL, 0);
}

/* ─── 2. TCP two adjacent connections ─── */

static void test_tcp_adjacent(void) {
    int port = TEST_PORT_BASE + 2;
    int srv = socket(AF_INET, SOCK_STREAM, 0);
    if (srv < 0) { FAIL("tcp-adjacent: socket: %s", strerror(errno)); return; }

    int opt = 1;
    setsockopt(srv, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt));

    struct sockaddr_in addr = { .sin_family = AF_INET, .sin_port = htons(port), .sin_addr.s_addr = INADDR_ANY };
    if (bind(srv, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        FAIL("tcp-adjacent: bind: %s", strerror(errno)); close(srv); return;
    }
    if (listen(srv, 5) < 0) {
        FAIL("tcp-adjacent: listen: %s", strerror(errno)); close(srv); return;
    }

    /* spawn two clients */
    pid_t pids[2] = {-1, -1};
    int child_count = 0;
    for (int i = 0; i < 2; i++) {
        pid_t p = fork();
        if (p < 0) { FAIL("tcp-adjacent: fork: %s", strerror(errno)); goto cleanup; }
        if (p == 0) {
            sleep(1);
            int fd = socket(AF_INET, SOCK_STREAM, 0);
            if (fd < 0) _exit(1);
            struct sockaddr_in caddr = { .sin_family = AF_INET, .sin_port = htons(port) };
            inet_pton(AF_INET, "127.0.0.1", &caddr.sin_addr);
            if (connect(fd, (struct sockaddr *)&caddr, sizeof(caddr)) < 0) _exit(1);
            char tag = 'A' + i;
            send(fd, &tag, 1, 0);
            close(fd);
            _exit(0);
        }
        pids[i] = p;
        child_count++;
    }

    /* accept two connections */
    char results[2] = {0};
    int ok = 1;
    for (int i = 0; i < 2; i++) {
        int cli = accept(srv, NULL, NULL);
        if (cli < 0) { ok = 0; break; }
        char c;
        if (recv(cli, &c, 1, 0) != 1) { ok = 0; close(cli); break; }
        results[i] = c;
        close(cli);
    }

    if (!ok || results[0] == 0 || results[1] == 0 || results[0] == results[1]) {
        FAIL("tcp-adjacent: got '%c' '%c', expected two distinct connections", results[0] ? results[0] : '?', results[1] ? results[1] : '?');
    } else {
        PASS("tcp-adjacent");
    }

cleanup:
    close(srv);
    for (int i = 0; i < child_count; i++) waitpid(pids[i], NULL, 0);
}

/* ─── 3. TCP 512 capacity ─── */

static void test_tcp_512_capacity(void) {
    int port = TEST_PORT_BASE + 3;
    int srv = socket(AF_INET, SOCK_STREAM, 0);
    if (srv < 0) { FAIL("tcp-512cap: socket: %s", strerror(errno)); return; }

    int opt = 1;
    setsockopt(srv, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt));

    struct sockaddr_in addr = { .sin_family = AF_INET, .sin_port = htons(port), .sin_addr.s_addr = INADDR_ANY };
    if (bind(srv, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        FAIL("tcp-512cap: bind: %s", strerror(errno)); close(srv); return;
    }
    if (listen(srv, 512) < 0) {
        FAIL("tcp-512cap: listen: %s", strerror(errno)); close(srv); return;
    }

    /* since 512 client forks may be heavy, open connections sequentially */
    int clients[512];
    int n_connected = 0;
    for (int i = 0; i < 512; i++) {
        int fd = socket(AF_INET, SOCK_STREAM, 0);
        if (fd < 0) break;
        struct sockaddr_in caddr = { .sin_family = AF_INET, .sin_port = htons(port) };
        inet_pton(AF_INET, "127.0.0.1", &caddr.sin_addr);
        if (connect(fd, (struct sockaddr *)&caddr, sizeof(caddr)) < 0) {
            close(fd);
            break;
        }
        clients[i] = fd;
        n_connected = i + 1;
    }

    if (n_connected != 512) {
        FAIL("tcp-512cap: connected %d of 512", n_connected);
        for (int i = 0; i < n_connected; i++) close(clients[i]);
        close(srv);
        return;
    }

    /* The 513th attempt must not corrupt the full listener. */
    int overflow = socket(AF_INET, SOCK_STREAM | SOCK_NONBLOCK, 0);
    if (overflow >= 0) {
        struct sockaddr_in caddr = { .sin_family = AF_INET, .sin_port = htons(port) };
        inet_pton(AF_INET, "127.0.0.1", &caddr.sin_addr);
        (void)connect(overflow, (struct sockaddr *)&caddr, sizeof(caddr));
        close(overflow);
    }

    /* Release one slot, then connect again without any delay. */
    int first = accept(srv, NULL, NULL);
    if (first < 0) {
        FAIL("tcp-512cap: first accept: %s", strerror(errno));
        for (int i = 0; i < n_connected; i++) close(clients[i]);
        close(srv);
        return;
    }
    close(first);
    close(clients[0]);

    int recovery = socket(AF_INET, SOCK_STREAM, 0);
    if (recovery < 0) {
        FAIL("tcp-512-recovery: socket: %s", strerror(errno));
        for (int i = 1; i < n_connected; i++) close(clients[i]);
        close(srv);
        return;
    }
    struct sockaddr_in recovery_addr = { .sin_family = AF_INET, .sin_port = htons(port) };
    inet_pton(AF_INET, "127.0.0.1", &recovery_addr.sin_addr);
    if (connect(recovery, (struct sockaddr *)&recovery_addr, sizeof(recovery_addr)) < 0) {
        FAIL("tcp-512-recovery: connect: %s", strerror(errno));
        close(recovery);
        for (int i = 1; i < n_connected; i++) close(clients[i]);
        close(srv);
        return;
    }

    int accepted = 1;
    for (int i = 0; i < 512; i++) {
        int cli = accept(srv, NULL, NULL);
        if (cli < 0) break;
        accepted++;
        close(cli);
    }

    for (int i = 1; i < n_connected; i++) close(clients[i]);
    close(recovery);

    if (accepted == 513) {
        PASS("tcp-512cap: accepted 512 of 512 initial connections");
        PASS("tcp-512-recovery");
    } else {
        FAIL("tcp-512cap: accepted %d total, expected 513", accepted);
    }

    close(srv);
}

/* ─── 4. TCP close/relisten ─── */

static void test_tcp_close_relisten(void) {
    int port = TEST_PORT_BASE + 4;
    int srv = socket(AF_INET, SOCK_STREAM, 0);
    if (srv < 0) { FAIL("tcp-relisten: socket: %s", strerror(errno)); return; }

    int opt = 1;
    setsockopt(srv, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt));

    struct sockaddr_in addr = { .sin_family = AF_INET, .sin_port = htons(port), .sin_addr.s_addr = INADDR_ANY };
    if (bind(srv, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        FAIL("tcp-relisten: bind: %s", strerror(errno)); close(srv); return;
    }
    if (listen(srv, 5) < 0) {
        FAIL("tcp-relisten: listen: %s", strerror(errno)); close(srv); return;
    }

    /* spawn one client, accept it, then close server */
    pid_t p = fork();
    if (p == 0) {
        close(srv);
        sleep(1);
        int fd = socket(AF_INET, SOCK_STREAM, 0);
        if (fd < 0) _exit(1);
        struct sockaddr_in caddr = { .sin_family = AF_INET, .sin_port = htons(port) };
        inet_pton(AF_INET, "127.0.0.1", &caddr.sin_addr);
        connect(fd, (struct sockaddr *)&caddr, sizeof(caddr));
        close(fd);
        _exit(0);
    }

    int cli = accept(srv, NULL, NULL);
    if (cli < 0) { FAIL("tcp-relisten: accept1: %s", strerror(errno)); close(srv); waitpid(p, NULL, 0); return; }
    close(cli);
    close(srv);
    waitpid(p, NULL, 0);
    /* reopen and bind again */
    int srv2 = socket(AF_INET, SOCK_STREAM, 0);
    if (srv2 < 0) { FAIL("tcp-relisten: socket2: %s", strerror(errno)); return; }
    setsockopt(srv2, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt));
    if (bind(srv2, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        FAIL("tcp-relisten: rebind: %s", strerror(errno)); close(srv2); return;
    }
    if (listen(srv2, 5) < 0) {
        FAIL("tcp-relisten: relisten: %s", strerror(errno)); close(srv2); return;
    }

    /* verify new connection works */
    pid_t p2 = fork();
    if (p2 == 0) {
        close(srv2);
        sleep(1);
        int fd = socket(AF_INET, SOCK_STREAM, 0);
        if (fd < 0) _exit(1);
        struct sockaddr_in caddr = { .sin_family = AF_INET, .sin_port = htons(port) };
        inet_pton(AF_INET, "127.0.0.1", &caddr.sin_addr);
        connect(fd, (struct sockaddr *)&caddr, sizeof(caddr));
        close(fd);
        _exit(0);
    }

    int cli2 = accept(srv2, NULL, NULL);
    if (cli2 < 0) {
        FAIL("tcp-relisten: accept after relisten: %s", strerror(errno));
    } else {
        PASS("tcp-relisten");
        close(cli2);
    }

    close(srv2);
    waitpid(p2, NULL, 0);
}

/* ─── 5. UDP bidirectional payload ─── */

static void test_udp_bidirectional(void) {
    int port = TEST_PORT_BASE + 5;
    pid_t pid = fork();
    if (pid < 0) { FAIL("udp-bidi: fork: %s", strerror(errno)); return; }

    if (pid == 0) {
        /* responder */
        sleep(1);
        int fd = socket(AF_INET, SOCK_DGRAM, 0);
        if (fd < 0) _exit(1);
        struct sockaddr_in laddr = { .sin_family = AF_INET, .sin_port = htons(port), .sin_addr.s_addr = INADDR_ANY };
        if (bind(fd, (struct sockaddr *)&laddr, sizeof(laddr)) < 0) _exit(1);

        char buf[64];
        struct sockaddr_in peer;
        socklen_t peer_len = sizeof(peer);
        ssize_t n = recvfrom(fd, buf, sizeof(buf) - 1, 0, (struct sockaddr *)&peer, &peer_len);
        if (n <= 0) _exit(1);
        buf[n] = 0;
        /* echo back with prefix */
        char reply[128];
        snprintf(reply, sizeof(reply), "echo-%s", buf);
        if (sendto(fd, reply, strlen(reply), 0, (struct sockaddr *)&peer, peer_len) < 0) _exit(1);
        close(fd);
        _exit(0);
    }

    /* initiator */
    sleep(2); /* wait for child to sleep(1) + bind */
    int fd = socket(AF_INET, SOCK_DGRAM, 0);
    if (fd < 0) { FAIL("udp-bidi: socket: %s", strerror(errno)); waitpid(pid, NULL, 0); return; }
    struct sockaddr_in saddr = { .sin_family = AF_INET, .sin_port = htons(port) };
    inet_pton(AF_INET, "127.0.0.1", &saddr.sin_addr);

    const char *msg = "udp-ms01";
    if (sendto(fd, msg, strlen(msg), 0, (struct sockaddr *)&saddr, sizeof(saddr)) < 0) {
        FAIL("udp-bidi: sendto: %s", strerror(errno));
        close(fd); waitpid(pid, NULL, 0); return;
    }

    char reply[128] = {0};
    struct sockaddr_in from;
    socklen_t from_len = sizeof(from);
    ssize_t n = recvfrom(fd, reply, sizeof(reply) - 1, 0, (struct sockaddr *)&from, &from_len);
    if (n < 0) {
        FAIL("udp-bidi: recvfrom: %s", strerror(errno));
    } else if (strcmp(reply, "echo-udp-ms01") != 0) {
        FAIL("udp-bidi: expected 'echo-udp-ms01', got '%s'", reply);
    } else {
        PASS("udp-bidi");
    }

    close(fd);
    waitpid(pid, NULL, 0);
}

/* ─── 6. TCP nonblocking EAGAIN ─── */

static void test_tcp_nonblock_eagain(void) {
    int port = TEST_PORT_BASE + 6;
    int srv = socket(AF_INET, SOCK_STREAM, 0);
    if (srv < 0) { FAIL("tcp-nonblock: socket: %s", strerror(errno)); return; }
    int opt = 1;
    setsockopt(srv, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt));
    struct sockaddr_in addr = { .sin_family = AF_INET, .sin_port = htons(port), .sin_addr.s_addr = INADDR_ANY };
    bind(srv, (struct sockaddr *)&addr, sizeof(addr));
    listen(srv, 5);

    /* nonblocking accept with no pending connections */
    set_nonblock(srv);
    int cli = accept(srv, NULL, NULL);
    if (cli >= 0) {
        FAIL("tcp-nonblock: accept should return EAGAIN, got fd %d", cli);
        close(cli);
    } else if (errno == EAGAIN || errno == EWOULDBLOCK) {
        PASS("tcp-nonblock-accept");
    } else {
        FAIL("tcp-nonblock: accept errno=%d (%s), expected EAGAIN", errno, strerror(errno));
    }
    close(srv);
}

/* ─── 7. UDP nonblocking EAGAIN ─── */

static void test_udp_nonblock_eagain(void) {
    int fd = socket(AF_INET, SOCK_DGRAM, 0);
    if (fd < 0) { FAIL("udp-nonblock: socket: %s", strerror(errno)); return; }
    struct sockaddr_in addr = { .sin_family = AF_INET, .sin_port = htons(TEST_PORT_BASE + 7), .sin_addr.s_addr = INADDR_ANY };
    bind(fd, (struct sockaddr *)&addr, sizeof(addr));

    set_nonblock(fd);
    char buf[64];
    ssize_t n = recvfrom(fd, buf, sizeof(buf), 0, NULL, NULL);
    if (n >= 0) {
        FAIL("udp-nonblock: recvfrom should return EAGAIN, got %zd bytes", n);
    } else if (errno == EAGAIN || errno == EWOULDBLOCK) {
        PASS("udp-nonblock");
    } else {
        FAIL("udp-nonblock: recvfrom errno=%d (%s), expected EAGAIN", errno, strerror(errno));
    }
    close(fd);
}

/* ─── 8. Poll readiness ─── */

static void test_poll_readiness(void) {
    int port = TEST_PORT_BASE + 8;
    int srv = socket(AF_INET, SOCK_STREAM, 0);
    if (srv < 0) { FAIL("poll: socket: %s", strerror(errno)); return; }
    int opt = 1;
    setsockopt(srv, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt));
    struct sockaddr_in addr = { .sin_family = AF_INET, .sin_port = htons(port), .sin_addr.s_addr = INADDR_ANY };
    bind(srv, (struct sockaddr *)&addr, sizeof(addr));
    listen(srv, 5);

    /* poll before connection: should timeout */
    struct pollfd pfd = { .fd = srv, .events = POLLIN };
    int ret = poll(&pfd, 1, 500); /* 500ms timeout */
    if (ret < 0) {
        FAIL("poll: poll error: %s", strerror(errno));
        close(srv); return;
    }
    if (ret > 0 && (pfd.revents & POLLIN)) {
        FAIL("poll: unexpected POLLIN before connection");
        close(srv); return;
    }

    /* connect a client */
    pid_t p = fork();
    if (p == 0) {
        sleep(1);
        int fd = socket(AF_INET, SOCK_STREAM, 0);
        if (fd < 0) _exit(1);
        struct sockaddr_in caddr = { .sin_family = AF_INET, .sin_port = htons(port) };
        inet_pton(AF_INET, "127.0.0.1", &caddr.sin_addr);
        connect(fd, (struct sockaddr *)&caddr, sizeof(caddr));
        close(fd);
        _exit(0);
    }

    /* poll after connection: should be readable */
    struct pollfd pfd2 = { .fd = srv, .events = POLLIN };
    ret = poll(&pfd2, 1, 3000);
    if (ret <= 0) {
        FAIL("poll: poll after connect returned %d, expected POLLIN", ret);
    } else if (!(pfd2.revents & POLLIN)) {
        FAIL("poll: revents=0x%x, expected POLLIN", pfd2.revents);
    } else {
        /* consume the pending connection */
        int cli = accept(srv, NULL, NULL);
        if (cli < 0) {
            FAIL("poll: accept after POLLIN: %s", strerror(errno));
        } else {
            PASS("poll-readiness");
            close(cli);
        }
    }

    close(srv);
    waitpid(p, NULL, 0);
}

/* ─── 10. bind getsockname witness ─── */

static void test_bind_getsockname(void) {
    int port = TEST_PORT_BASE + 11;
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) { FAIL("bind-getsockname: socket: %s", strerror(errno)); return; }

    struct sockaddr_in addr = { .sin_family = AF_INET, .sin_port = htons(port), .sin_addr.s_addr = INADDR_ANY };
    if (bind(fd, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        FAIL("bind-getsockname: bind: %s", strerror(errno));
        close(fd); return;
    }

    struct sockaddr_in got;
    socklen_t got_len = sizeof(got);
    if (getsockname(fd, (struct sockaddr *)&got, &got_len) < 0) {
        FAIL("bind-getsockname: getsockname: %s", strerror(errno));
        close(fd); return;
    }

    if (got.sin_port == htons(port)) {
        PASS("bind-getsockname: port %d", port);
    } else {
        FAIL("bind-getsockname: expected port %d, got %d", port, ntohs(got.sin_port));
    }
    close(fd);
}

/* ─── 11. bind ephemeral connect witness ─── */

static void test_bind_ephemeral_connect(void) {
    int port = TEST_PORT_BASE + 12;
    int srv = socket(AF_INET, SOCK_STREAM, 0);
    if (srv < 0) { FAIL("bind-ephemeral: server socket: %s", strerror(errno)); return; }

    int opt = 1;
    setsockopt(srv, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt));

    struct sockaddr_in addr = { .sin_family = AF_INET, .sin_port = htons(port), .sin_addr.s_addr = INADDR_ANY };
    if (bind(srv, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        FAIL("bind-ephemeral: server bind: %s", strerror(errno));
        close(srv); return;
    }
    if (listen(srv, 1) < 0) {
        FAIL("bind-ephemeral: server listen: %s", strerror(errno));
        close(srv); return;
    }

    pid_t p = fork();
    if (p == 0) {
        close(srv);
        int fd = socket(AF_INET, SOCK_STREAM, 0);
        if (fd < 0) _exit(1);
        struct sockaddr_in caddr = { .sin_family = AF_INET, .sin_port = htons(port) };
        inet_pton(AF_INET, "127.0.0.1", &caddr.sin_addr);
        if (connect(fd, (struct sockaddr *)&caddr, sizeof(caddr)) < 0) _exit(1);
        struct sockaddr_in got;
        socklen_t got_len = sizeof(got);
        if (getsockname(fd, (struct sockaddr *)&got, &got_len) == 0 && ntohs(got.sin_port) != 0) {
            fprintf(stdout, "PASS: bind-ephemeral: port %d\n", ntohs(got.sin_port));
        } else {
            fprintf(stdout, "FAIL: bind-ephemeral: expected non-zero port\n");
            _failed = 1;
        }
        close(fd);
        _exit(0);
    }

    int cli = accept(srv, NULL, NULL);
    if (cli < 0) {
        FAIL("bind-ephemeral: server accept: %s", strerror(errno));
    } else {
        close(cli);
    }
    close(srv);
    waitpid(p, NULL, 0);
}

/* ─── 12. bind conflict witness ─── */

static void test_bind_conflict(void) {
    int port = TEST_PORT_BASE + 13;
    int fd1 = socket(AF_INET, SOCK_STREAM, 0);
    if (fd1 < 0) { FAIL("bind-conflict: socket1: %s", strerror(errno)); return; }

    struct sockaddr_in addr = { .sin_family = AF_INET, .sin_port = htons(port), .sin_addr.s_addr = INADDR_ANY };
    if (bind(fd1, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        FAIL("bind-conflict: bind1: %s", strerror(errno));
        close(fd1); return;
    }

    int fd2 = socket(AF_INET, SOCK_STREAM, 0);
    if (fd2 < 0) { FAIL("bind-conflict: socket2: %s", strerror(errno)); close(fd1); return; }
    if (bind(fd2, (struct sockaddr *)&addr, sizeof(addr)) == 0) {
        FAIL("bind-conflict: second bind should fail with EADDRINUSE, but succeeded");
        close(fd2);
    } else if (errno == EADDRINUSE) {
        PASS("bind-conflict: EADDRINUSE");
    } else {
        FAIL("bind-conflict: expected EADDRINUSE, got errno %d (%s)", errno, strerror(errno));
    }
    close(fd2);
    close(fd1);
}

/* ─── 13. bind close cleanup witness ─── */

static void test_bind_close_cleanup(void) {
    int port = TEST_PORT_BASE + 14;
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) { FAIL("bind-close-cleanup: socket1: %s", strerror(errno)); return; }

    int opt = 1;
    setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt));

    struct sockaddr_in addr = { .sin_family = AF_INET, .sin_port = htons(port), .sin_addr.s_addr = INADDR_ANY };
    if (bind(fd, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        FAIL("bind-close-cleanup: bind1: %s", strerror(errno));
        close(fd); return;
    }
    if (listen(fd, 1) < 0) {
        FAIL("bind-close-cleanup: listen: %s", strerror(errno));
        close(fd); return;
    }
    close(fd);

    int fd2 = socket(AF_INET, SOCK_STREAM, 0);
    if (fd2 < 0) { FAIL("bind-close-cleanup: socket2: %s", strerror(errno)); return; }
    setsockopt(fd2, SOL_SOCKET, SO_REUSEADDR, &opt, sizeof(opt));
    if (bind(fd2, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        FAIL("bind-close-cleanup: rebind after close: errno %d (%s)", errno, strerror(errno));
    } else {
        PASS("bind-close-cleanup");
    }
    close(fd2);
}

/* ─── 9. UDP datagram boundary (source address) ─── */

static void test_udp_source_address(void) {
    int port = TEST_PORT_BASE + 9;
    int fd = socket(AF_INET, SOCK_DGRAM, 0);
    if (fd < 0) { FAIL("udp-source: socket: %s", strerror(errno)); return; }
    struct sockaddr_in addr = { .sin_family = AF_INET, .sin_port = htons(port), .sin_addr.s_addr = INADDR_ANY };
    if (bind(fd, (struct sockaddr *)&addr, sizeof(addr)) < 0) {
        FAIL("udp-source: bind: %s", strerror(errno)); close(fd); return;
    }

    pid_t p = fork();
    if (p == 0) {
        sleep(1);
        int cfd = socket(AF_INET, SOCK_DGRAM, 0);
        if (cfd < 0) _exit(1);
        struct sockaddr_in saddr = { .sin_family = AF_INET, .sin_port = htons(port) };
        inet_pton(AF_INET, "127.0.0.1", &saddr.sin_addr);
        const char *msg = "src-test";
        sendto(cfd, msg, strlen(msg), 0, (struct sockaddr *)&saddr, sizeof(saddr));
        close(cfd);
        _exit(0);
    }

    char buf[64];
    struct sockaddr_in from;
    socklen_t from_len = sizeof(from);
    ssize_t n = recvfrom(fd, buf, sizeof(buf) - 1, 0, (struct sockaddr *)&from, &from_len);
    if (n < 0) {
        FAIL("udp-source: recvfrom: %s", strerror(errno));
    } else {
        char ip[INET_ADDRSTRLEN];
        inet_ntop(AF_INET, &from.sin_addr, ip, sizeof(ip));
        if (strcmp(ip, "127.0.0.1") == 0 && ntohs(from.sin_port) != 0) {
            PASS("udp-source: %s:%d", ip, ntohs(from.sin_port));
        } else {
            FAIL("udp-source: unexpected source %s:%d", ip, ntohs(from.sin_port));
        }
    }

    close(fd);
    waitpid(p, NULL, 0);
}

/* ─── main ─── */

int main(void) {
    /* avoid lingering children */
    signal(SIGCHLD, SIG_IGN);

    fprintf(stdout, "MS01_SOCKET_BASELINE_START\n");

    test_tcp_accept_roundtrip();
    test_tcp_adjacent();
    test_tcp_512_capacity();
    test_tcp_close_relisten();
    test_udp_bidirectional();
    test_tcp_nonblock_eagain();
    test_udp_nonblock_eagain();
    test_poll_readiness();
    test_udp_source_address();
    test_bind_getsockname();
    test_bind_ephemeral_connect();
    test_bind_conflict();
    test_bind_close_cleanup();

    fprintf(stdout, "MS01_SOCKET_BASELINE_END\n");

    return _failed ? 1 : 0;
}
