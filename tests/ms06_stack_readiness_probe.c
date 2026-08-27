#define _GNU_SOURCE
#define _POSIX_C_SOURCE 200809L

/* MS06 application-visible readiness probe — guest payload + host seam core.
 *
 * Marker protocol (validated by scripts/ms06-qemu-validate.py):
 *   MS06_STACK_READINESS_START
 *   MS06_REVISION: <non-empty>
 *   MS06_ENVIRONMENT: <non-empty>
 *   PASS: <one line per case below, fixed order, exactly once>
 *   MS06_STACK_READINESS_END        (exit marker appended by the operator shell)
 *
 * Build (host syntax check):
 *   cc -std=c11 -Wall -Wextra -Werror -fsyntax-only tests/ms06_stack_readiness_probe.c
 *
 * Host decision harness (no guest, no sockets):
 *   cc -std=c11 -Wall -Wextra -Werror tests/ms06_stack_readiness_probe_test.c \
 *     -o /tmp/ms06-stack-readiness-probe-test && /tmp/ms06-stack-readiness-probe-test
 *
 * Build (RISC-V static, user boundary; run manually inside QEMU):
 *   make tests/ms06_stack_readiness_probe
 */

#define MS06_ENVIRONMENT_DEFAULT "qemu-virt-riscv64-single-hart"
#ifndef MS06_REVISION_DEFAULT
#define MS06_REVISION_DEFAULT "unknown"
#endif

#include <errno.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <poll.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/epoll.h>
#include <sys/select.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

/* ── Case registry ──────────────────────────────────────────────────── */

#define MS06_CASE_COUNT 12

enum {
    MS06_CASE_TCP_TIMER = 0,
    MS06_CASE_UDP_PROGRESS,
    MS06_CASE_LISTENER,
    MS06_CASE_NONBLOCK_CONNECT_ERROR,
    MS06_CASE_QUIET,
    MS06_CASE_CONTINUOUS_TRAFFIC,
    MS06_CASE_CLOSE_ERROR,
    MS06_CASE_POLL_MULTIWAITER,
    MS06_CASE_SELECT_MULTIWAITER,
    MS06_CASE_EPOLL_MULTIWAITER,
    MS06_CASE_WAITER_64,
    MS06_CASE_WAITER_65_REREGISTER
};

/* poll(2)-compatible bits reused across the seam so host tests need no
 * socket headers beyond <poll.h>. */
#define MS06_EV_IN      0x0001u /* POLLIN  */
#define MS06_EV_OUT     0x0004u /* POLLOUT */
#define MS06_EV_ERR     0x0008u /* POLLERR */
#define MS06_EV_HUP     0x0010u /* POLLHUP */
#define MS06_EV_RDHUP   0x2000u /* POLLRDHUP */

enum ms06_status {
    MS06_ST_COMPLETED = 0,
    MS06_ST_TIMEOUT,
    MS06_ST_EVENT_MISMATCH,
    MS06_ST_IO_ERROR,
    MS06_ST_CLEANUP_FAIL
};

struct ms06_case_result {
    int case_id;
    enum ms06_status status;
    uint32_t events;      /* observed event bits */
    int err;              /* errno captured at completion, 0 when none */
    int want_err;         /* errno the case contract expects at completion */
    int cleanup_ok;       /* children reaped, fds drained */
};

/* Fixed per-case deadlines (monotonic milliseconds). Generous: this is a
 * decision tool, not a benchmark; the deadline only bounds a wedged runner. */
#define MS06_TCP_TIMER_DEADLINE_MS      15000u
#define MS06_UDP_PROGRESS_DEADLINE_MS   15000u
#define MS06_LISTENER_DEADLINE_MS       20000u
#define MS06_CONNECT_ERR_DEADLINE_MS    15000u
#define MS06_QUIET_WINDOW_ITERS         24u
#define MS06_QUIET_SLICE_MS             25u
#define MS06_QUIET_DEADLINE_MS          20000u
#define MS06_TRAFFIC_MESSAGES           192u
#define MS06_TRAFFIC_MSG_SIZE           48u
#define MS06_TRAFFIC_DEADLINE_MS        30000u
#define MS06_CLOSE_DEADLINE_MS          15000u
#define MS06_MULTIWAITER_COUNT          4u
#define MS06_MULTIWAITER_DEADLINE_MS    30000u
#define MS06_WAITER64_DEADLINE_MS       45000u
#define MS06_WAITER65_DEADLINE_MS       60000u

/* ── Seam: host-testable decisions (pure, no syscalls) ──────────────── */

const char *ms06_case_name(int case_id)
{
    static const char *const names[MS06_CASE_COUNT] = {
        "tcp-timer",
        "udp-progress",
        "listener",
        "nonblock-connect-error",
        "quiet",
        "continuous-traffic",
        "close-error",
        "poll-multiwaiter",
        "select-multiwaiter",
        "epoll-multiwaiter",
        "waiter-64",
        "waiter-65-reregister"
    };
    if (case_id < 0 || case_id >= MS06_CASE_COUNT) return "?";
    return names[case_id];
}

/* Equal-to-deadline completion counts as expired: a case must finish strictly
 * inside its budget. Clock regression (now < start) is treated as expiry so a
 * broken monotonic source can never extend a case. */
int ms06_deadline_expired(uint64_t start_us, uint64_t now_us, uint64_t deadline_ms)
{
    uint64_t limit_us = deadline_ms * 1000u;
    if (now_us < start_us) return 1;
    return (now_us - start_us) >= limit_us;
}

/* Remaining poll/select/epoll timeout in ms, clamped to [0, deadline]. */
int64_t ms06_deadline_remaining_ms(uint64_t start_us, uint64_t now_us, uint64_t deadline_ms)
{
    uint64_t limit_us = deadline_ms * 1000u;
    if (now_us < start_us) return 0;
    if ((now_us - start_us) >= limit_us) return 0;
    return (int64_t)((limit_us - (now_us - start_us)) / 1000u);
}

/* Per-case event contract: required bits must all be present, forbidden bits
 * must all be absent. Cases that are not single-wait-event driven accept any
 * observation here and are judged by their own flow instead. */
int ms06_events_satisfy(int case_id, uint32_t events)
{
    uint32_t required = 0, forbidden = 0;
    switch (case_id) {
    case MS06_CASE_TCP_TIMER:
        required = MS06_EV_IN | MS06_EV_RDHUP;
        forbidden = MS06_EV_ERR;
        break;
    case MS06_CASE_UDP_PROGRESS:
        required = MS06_EV_IN;
        forbidden = MS06_EV_ERR;
        break;
    case MS06_CASE_NONBLOCK_CONNECT_ERROR:
        /* Iteration 004: connect recheck commits the local error and then
         * reports OUT|ERR together. */
        required = MS06_EV_OUT | MS06_EV_ERR;
        forbidden = 0;
        break;
    case MS06_CASE_QUIET:
        /* Only read/terminal/error direction counts as spurious progress; an
         * established socket that is writable is normal readiness, never
         * evidence of runner self-wake or fake work. */
        required = 0;
        forbidden = MS06_EV_IN | MS06_EV_ERR | MS06_EV_HUP | MS06_EV_RDHUP;
        break;
    case MS06_CASE_CLOSE_ERROR:
        /* Graceful peer close: EOF family, never a device fault. */
        required = MS06_EV_IN | MS06_EV_RDHUP;
        forbidden = MS06_EV_ERR;
        break;
    case MS06_CASE_POLL_MULTIWAITER:
    case MS06_CASE_SELECT_MULTIWAITER:
    case MS06_CASE_EPOLL_MULTIWAITER:
    case MS06_CASE_WAITER_64:
    case MS06_CASE_WAITER_65_REREGISTER:
        required = MS06_EV_IN;
        forbidden = MS06_EV_ERR;
        break;
    case MS06_CASE_LISTENER:
    case MS06_CASE_CONTINUOUS_TRAFFIC:
    default:
        return 1;
    }
    if ((events & required) != required) return 0;
    if ((events & forbidden) != 0) return 0;
    return 1;
}

int ms06_case_verdict(const struct ms06_case_result *r)
{
    if (r == NULL) return 0;
    if (r->case_id < 0 || r->case_id >= MS06_CASE_COUNT) return 0;
    if (r->status != MS06_ST_COMPLETED) return 0;
    if (!r->cleanup_ok) return 0;
    if (r->err != r->want_err) return 0;
    return ms06_events_satisfy(r->case_id, r->events);
}

/* A UDP bind spec must carry an explicit AF_INET loopback endpoint: the
 * zero-initialized struct the old probe submitted to bind(2) has family 0
 * and cannot form the queued-datagram witness. Port 0 requests an ephemeral
 * port and is a valid specification. */
int ms06_udp_bind_spec_valid(const struct sockaddr_in *sa)
{
    if (sa == NULL) return 0;
    if (sa->sin_family != AF_INET) return 0;
    if (sa->sin_addr.s_addr != htonl(INADDR_LOOPBACK)) return 0;
    return 1;
}

/* Quiet-window interest: only the read/terminal directions are armed.
 * POLLOUT is deliberately excluded: established writability is normal
 * readiness, never evidence of spurious runner progress. */
int ms06_quiet_interest(void)
{
    return POLLIN | POLLRDHUP;
}

/* ── Seam: waiter identity and exact-capacity aggregation (Task 4.3) ── */

#define MS06_PHASE_REGISTERED   0x01u
#define MS06_PHASE_WOKEN        0x02u
#define MS06_PHASE_RECHECK_NG   0x04u
#define MS06_PHASE_REREGISTERED 0x08u

enum ms06_wait_mode {
    MS06_WAIT_POLL = 0,
    MS06_WAIT_SELECT,
    MS06_WAIT_EPOLL
};

struct ms06_waiter_record {
    long pid;                 /* distinct waiter identity (guest pid) */
    uint32_t phases;          /* MS06_PHASE_* bitmask */
    uint32_t completions;     /* must reach exactly 1 for acceptance */
    uint32_t replacements;    /* pre-data 0-event wakes rechecked as not-ready */
};

struct ms06_waiter_set {
    uint32_t capacity;            /* exact waiter count: 64 or 65 */
};

int ms06_waiter_record_valid(const struct ms06_waiter_record *r)
{
    if (r == NULL) return 0;
    if (r->pid <= 0) return 0;
    if (r->completions > 1) return 0;
    if (r->completions == 1 && !(r->phases & MS06_PHASE_REGISTERED)) return 0;
    /* A replacement-class observation is only meaningful with its full
     * chain: woken without readiness, rechecked-not-ready, re-registered,
     * and only then completed. */
    if (r->replacements > 0 || (r->phases & MS06_PHASE_RECHECK_NG)) {
        if (!(r->phases & MS06_PHASE_WOKEN)) return 0;
        if (!(r->phases & MS06_PHASE_RECHECK_NG)) return 0;
        if (r->completions == 1 && !(r->phases & MS06_PHASE_REREGISTERED)) return 0;
    }
    return 1;
}

int ms06_waiter_set_accepts(const struct ms06_waiter_set *set,
                            const struct ms06_waiter_record *records,
                            uint32_t count)
{
    if (set == NULL || records == NULL) return 0;
    if (count != set->capacity) return 0; /* partial completion or over-run */
    for (uint32_t i = 0; i < count; ++i) {
        const struct ms06_waiter_record *w = &records[i];
        if (!ms06_waiter_record_valid(w)) return 0;
        if (w->completions != 1) return 0; /* every waiter completes exactly once */
        for (uint32_t j = 0; j < i; ++j) {
            if (records[j].pid == w->pid) return 0; /* identity collapse */
        }
    }
    return 1;
}

/* Exact 64/65 release decisions (Task 4.3 replan). The exact-waiter probe
 * must publish every arm through a synchronous per-worker epoll registration,
 * and the parent must release the trigger only after all N arms are in and
 * the consumable unit count equals the waiter count. Replacement/re-register
 * evidence is host/source-owned; the guest never waits for user-space empty
 * events and never claims to have observed one. */
int ms06_exact_mode_ok(int mode)
{
    return mode == MS06_WAIT_EPOLL;
}

int ms06_exact_arms_complete(uint32_t armed, uint32_t n_waiters)
{
    if (n_waiters == 0) return 0;
    return armed == n_waiters;
}

int ms06_trigger_units_valid(uint32_t units, uint32_t n_waiters)
{
    if (n_waiters == 0) return 0;
    return units == n_waiters;
}

/* Listener identity echo: the wire carries one byte, so identity and reply
 * are compared with unsigned-char semantics. Integer promotion of `~ident`
 * to 32 bits made every valid reply a runtime false negative. */
int ms06_listener_reply_matches(unsigned ident, unsigned char echo)
{
    return echo == (unsigned char)~ident;
}

/* Peer FIN closes only the receive half; the still-open local write half is
 * not required to reach EPIPE. A valid graceful-close observation is
 * IN|RDHUP readiness without device ERR plus two stable zero-length reads. */
int ms06_peer_fin_eof_valid(uint32_t events, int recv1, int recv2)
{
    if ((events & MS06_EV_ERR) != 0) return 0;
    if ((events & (MS06_EV_IN | MS06_EV_RDHUP)) != (MS06_EV_IN | MS06_EV_RDHUP)) return 0;
    return recv1 == 0 && recv2 == 0;
}

/* ── Guest payload (excluded from the host seam harness) ────────────── */

#ifndef MS06_STACK_READINESS_PROBE_TESTING

#define MS06_TRIGGER_BYTE 'X'
#define MS06_ARM_BYTE 'A'
#define MS06_MAX_WAITERS 65u

static uint64_t now_us(void)
{
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) return 0;
    return (uint64_t)ts.tv_sec * 1000000u + (uint64_t)ts.tv_nsec / 1000u;
}

static void ms06_report(int ok, int case_id, const char *detail)
{
    if (ok) {
        printf("PASS: %s\n", ms06_case_name(case_id));
    } else {
        printf("FAIL: %s %s\n", ms06_case_name(case_id), detail != NULL ? detail : "failed");
    }
    fflush(stdout);
}

/* ── bounded syscall helpers ────────────────────────────────────────── */

static int xpipe(int fds[2])
{
    return pipe(fds);
}

static int write_full(int fd, const void *buf, size_t len)
{
    const char *p = (const char *)buf;
    while (len > 0) {
        ssize_t n = write(fd, p, len);
        if (n < 0) {
            if (errno == EINTR) continue;
            return -1;
        }
        p += n;
        len -= (size_t)n;
    }
    return 0;
}

/* poll() one fd. Returns 1 when poll woke (*events holds revents, which may
 * be empty for a wake-without-readiness), 0 at deadline, -1 on fatal error.
 * The wake/timeout distinction is what lets waiter workers count replacement
 * wakes instead of guessing from elapsed time. */
static int poll_events_deadline(int fd, short req, uint64_t t0, uint64_t dl_ms,
                                uint32_t *events)
{
    for (;;) {
        int64_t rem = ms06_deadline_remaining_ms(t0, now_us(), dl_ms);
        if (rem <= 0) {
            *events = 0;
            return 0;
        }
        struct pollfd pfd;
        memset(&pfd, 0, sizeof(pfd));
        pfd.fd = fd;
        pfd.events = req;
        int rc = poll(&pfd, 1, (int)rem);
        if (rc < 0) {
            if (errno == EINTR) continue;
            return -1;
        }
        if (rc == 0) continue;
        *events = (uint32_t)pfd.revents & 0xFFFFu;
        return 1;
    }
}

static int read_byte_deadline(int fd, char *out, uint64_t t0, uint64_t dl_ms)
{
    for (;;) {
        uint32_t ev = 0;
        int rc = poll_events_deadline(fd, POLLIN, t0, dl_ms, &ev);
        if (rc <= 0) return -1;
        if (!(ev & MS06_EV_IN)) return -1;
        ssize_t n = read(fd, out, 1);
        if (n == 1) return 0;
        if (n < 0 && errno == EINTR) continue;
        if (n < 0 && (errno == EAGAIN || errno == EWOULDBLOCK)) continue;
        return -1;
    }
}

static int read_full_deadline(int fd, void *buf, size_t len,
                              uint64_t t0, uint64_t dl_ms)
{
    char *p = (char *)buf;
    size_t got = 0;
    while (got < len) {
        uint32_t ev = 0;
        int rc = poll_events_deadline(fd, POLLIN, t0, dl_ms, &ev);
        if (rc <= 0) return -1;
        if (!(ev & MS06_EV_IN)) return -1;
        ssize_t n = read(fd, p + got, len - got);
        if (n > 0) {
            got += (size_t)n;
            continue;
        }
        if (n == 0) return -1; /* peer closed early */
        if (errno == EINTR) continue;
        if (errno == EAGAIN || errno == EWOULDBLOCK) continue;
        return -1;
    }
    return 0;
}

static ssize_t recv_some_deadline(int fd, void *buf, size_t len,
                                  uint64_t t0, uint64_t dl_ms)
{
    for (;;) {
        uint32_t ev = 0;
        int rc = poll_events_deadline(fd, POLLIN, t0, dl_ms, &ev);
        if (rc <= 0) return -1;
        if (!(ev & MS06_EV_IN)) return -1;
        ssize_t n = recv(fd, buf, len, 0);
        if (n >= 0) return n;
        if (errno == EINTR) continue;
        if (errno == EAGAIN || errno == EWOULDBLOCK) continue;
        return -1;
    }
}

static int recv_exact_deadline(int fd, void *buf, size_t len,
                               uint64_t t0, uint64_t dl_ms)
{
    char *p = (char *)buf;
    size_t got = 0;
    while (got < len) {
        ssize_t n = recv_some_deadline(fd, p + got, len - got, t0, dl_ms);
        if (n <= 0) return -1;
        got += (size_t)n;
    }
    return 0;
}

static int send_all_deadline(int fd, const void *buf, size_t len,
                             uint64_t t0, uint64_t dl_ms)
{
    const char *p = (const char *)buf;
    size_t sent = 0;
    while (sent < len) {
        int64_t rem = ms06_deadline_remaining_ms(t0, now_us(), dl_ms);
        if (rem <= 0) return -1;
        uint32_t ev = 0;
        int rc = poll_events_deadline(fd, POLLOUT, t0, dl_ms, &ev);
        if (rc <= 0) return -1;
        if (!(ev & MS06_EV_OUT)) return -1;
        ssize_t n = send(fd, p + sent, len - sent, 0);
        if (n >= 0) {
            sent += (size_t)n;
            continue;
        }
        if (errno == EINTR) continue;
        if (errno == EAGAIN || errno == EWOULDBLOCK) continue;
        return -1;
    }
    return 0;
}

static int make_listener(struct sockaddr_in *addr_out)
{
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0) return -1;
    memset(addr_out, 0, sizeof(*addr_out));
    addr_out->sin_family = AF_INET;
    addr_out->sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    addr_out->sin_port = 0;
    if (bind(fd, (struct sockaddr *)addr_out, sizeof(*addr_out)) < 0 ||
        listen(fd, 16) < 0) {
        close(fd);
        return -1;
    }
    socklen_t len = sizeof(*addr_out);
    if (getsockname(fd, (struct sockaddr *)addr_out, &len) < 0) {
        close(fd);
        return -1;
    }
    return fd;
}

static int set_nonblock(int fd)
{
    int flags = fcntl(fd, F_GETFL, 0);
    if (flags < 0) return -1;
    return fcntl(fd, F_SETFL, flags | O_NONBLOCK);
}

/* Reap every pid; on failure paths force-terminate stragglers first. */
static void reap_children(pid_t *pids, unsigned count)
{
    for (unsigned i = 0; i < count; ++i) {
        if (pids[i] <= 0) continue;
        kill(pids[i], SIGKILL);
    }
    for (unsigned i = 0; i < count; ++i) {
        if (pids[i] <= 0) continue;
        int st;
        while (waitpid(pids[i], &st, 0) < 0 && errno == EINTR) {}
    }
}

static int child_exited_ok(int status)
{
    return WIFEXITED(status) && WEXITSTATUS(status) == 0;
}

/* ── Case 1: tcp-timer ──────────────────────────────────────────────── */

static int run_tcp_timer(void)
{
    const uint64_t dl = MS06_TCP_TIMER_DEADLINE_MS;
    const uint64_t t0 = now_us();
    struct ms06_case_result r;
    struct sockaddr_in sa;
    int srv = -1, cfd = -1, up[2] = {-1, -1}, down[2] = {-1, -1};
    pid_t kid = -1;
    int reaped_ok = -1;

    memset(&r, 0, sizeof(r));
    r.case_id = MS06_CASE_TCP_TIMER;
    r.want_err = 0;

    do {
        if (srv < 0) srv = make_listener(&sa);
        if (srv < 0 || xpipe(up) < 0 || xpipe(down) < 0) { r.status = MS06_ST_IO_ERROR; break; }

        kid = fork();
        if (kid < 0) { r.status = MS06_ST_IO_ERROR; break; }
        if (kid == 0) {
            int c = socket(AF_INET, SOCK_STREAM, 0);
            char b;
            if (c < 0) _exit(2);
            if (connect(c, (struct sockaddr *)&sa, sizeof(sa)) < 0) _exit(2);
            close(srv); close(up[0]); close(down[1]);
            if (write_full(up[1], "K", 1) < 0) _exit(2);
            if (read_byte_deadline(down[0], &b, t0, dl) < 0 || b != 'G') _exit(2);
            close(c);
            _exit(0);
        }
        close(up[1]); up[1] = -1;
        close(down[0]); down[0] = -1;

        {
            uint32_t ev = 0;
            if (poll_events_deadline(srv, POLLIN, t0, dl, &ev) != 1 ||
                !(ev & MS06_EV_IN)) {
                r.status = MS06_ST_TIMEOUT;
                break;
            }
            cfd = accept(srv, NULL, NULL);
            if (cfd < 0) { r.status = MS06_ST_IO_ERROR; r.err = errno; break; }
        }
        {
            char b;
            if (read_byte_deadline(up[0], &b, t0, dl) < 0 || b != 'K') {
                r.status = MS06_ST_IO_ERROR;
                break;
            }
        }
        /* The connection is now accepted and idle. Delivering the peer's FIN
         * is left entirely to the resident runner: the probe performs no
         * driving I/O past this point, so any progress seen below is
         * timer-driven protocol work. */
        if (write_full(down[1], "G", 1) < 0) { r.status = MS06_ST_IO_ERROR; break; }

        {
            uint32_t ev = 0;
            int rc = poll_events_deadline(cfd, POLLIN | POLLRDHUP, t0, dl, &ev);
            char b;
            if (rc != 1) {
                r.status = MS06_ST_TIMEOUT;
                break;
            }
            r.events = ev;
            if (recv(cfd, &b, 1, 0) != 0) {
                r.status = MS06_ST_IO_ERROR;
                r.err = errno;
                break;
            }
            if (recv(cfd, &b, 1, 0) != 0) {
                r.status = MS06_ST_EVENT_MISMATCH; /* EOF not stable on retry */
                r.err = errno;
                break;
            }
            r.status = MS06_ST_COMPLETED;
        }
    } while (0);

    if (kid > 0) {
        if (r.status == MS06_ST_COMPLETED) {
            int st;
            while (waitpid(kid, &st, 0) < 0 && errno == EINTR) {}
            reaped_ok = child_exited_ok(st);
        } else {
            reap_children(&kid, 1);
            reaped_ok = 0;
        }
    }
    if (reaped_ok <= 0 && r.status == MS06_ST_COMPLETED) {
        r.status = MS06_ST_CLEANUP_FAIL;
    }
    r.cleanup_ok = reaped_ok > 0;

    if (up[0] >= 0) close(up[0]);
    if (up[1] >= 0) close(up[1]);
    if (down[0] >= 0) close(down[0]);
    if (down[1] >= 0) close(down[1]);
    if (cfd >= 0) close(cfd);
    if (srv >= 0) close(srv);

    {
        int ok = ms06_case_verdict(&r);
        ms06_report(ok, MS06_CASE_TCP_TIMER,
                    ok ? NULL : "FIN not delivered to an idle connection inside deadline");
        return ok;
    }
}

/* ── Case 2: udp-progress ───────────────────────────────────────────── */

static int run_udp_progress(void)
{
    const uint64_t dl = MS06_UDP_PROGRESS_DEADLINE_MS;
    const uint64_t t0 = now_us();
    struct ms06_case_result r;
    int a = -1, b = -1;

    memset(&r, 0, sizeof(r));
    r.case_id = MS06_CASE_UDP_PROGRESS;
    r.want_err = 0;

    do {
        a = socket(AF_INET, SOCK_DGRAM, 0);
        b = socket(AF_INET, SOCK_DGRAM, 0);
        if (a < 0 || b < 0) { r.status = MS06_ST_IO_ERROR; break; }

        struct sockaddr_in aa, ba;
        socklen_t len;
        memset(&aa, 0, sizeof(aa));
        memset(&ba, 0, sizeof(ba));
        aa.sin_family = AF_INET;
        aa.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        aa.sin_port = 0;
        ba.sin_family = AF_INET;
        ba.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
        ba.sin_port = 0;
        if (!ms06_udp_bind_spec_valid(&aa) || !ms06_udp_bind_spec_valid(&ba) ||
            bind(a, (struct sockaddr *)&aa, sizeof(aa)) < 0 ||
            bind(b, (struct sockaddr *)&ba, sizeof(ba)) < 0) {
            r.status = MS06_ST_IO_ERROR;
            break;
        }
        len = sizeof(aa);
        if (getsockname(a, (struct sockaddr *)&aa, &len) < 0) { r.status = MS06_ST_IO_ERROR; break; }
        len = sizeof(ba);
        if (getsockname(b, (struct sockaddr *)&ba, &len) < 0) { r.status = MS06_ST_IO_ERROR; break; }

        char payload[64];
        for (size_t i = 0; i < sizeof(payload); ++i) payload[i] = (char)('U' + (i % 26));
        if (sendto(a, payload, sizeof(payload), 0,
                   (struct sockaddr *)&ba, sizeof(ba)) != (ssize_t)sizeof(payload)) {
            r.status = MS06_ST_IO_ERROR;
            r.err = errno;
            break;
        }

        /* The datagram is queued: only the runner can move it to B. */
        uint32_t ev = 0;
        int rc = poll_events_deadline(b, POLLIN, t0, dl, &ev);
        if (rc != 1) { r.status = MS06_ST_TIMEOUT; break; }
        r.events = ev;

        char buf[64];
        struct sockaddr_in src;
        socklen_t slen = sizeof(src);
        ssize_t n = recvfrom(b, buf, sizeof(buf), 0, (struct sockaddr *)&src, &slen);
        if (n != (ssize_t)sizeof(payload)) {
            r.status = MS06_ST_IO_ERROR;
            r.err = errno;
            break;
        }
        if (memcmp(buf, payload, sizeof(payload)) != 0 ||
            src.sin_port != aa.sin_port ||
            src.sin_addr.s_addr != aa.sin_addr.s_addr) {
            r.status = MS06_ST_EVENT_MISMATCH;
            break;
        }
        r.status = MS06_ST_COMPLETED;
    } while (0);

    if (a >= 0) close(a);
    if (b >= 0) close(b);
    r.cleanup_ok = 1;

    {
        int ok = ms06_case_verdict(&r);
        ms06_report(ok, MS06_CASE_UDP_PROGRESS,
                    ok ? NULL : "queued datagram not delivered through the runner");
        return ok;
    }
}

/* ── Case 3: listener ───────────────────────────────────────────────── */

#define MS06_LISTENER_CONNECTIONS 4u

static int run_listener(void)
{
    const uint64_t dl = MS06_LISTENER_DEADLINE_MS;
    const uint64_t t0 = now_us();
    struct ms06_case_result r;
    struct sockaddr_in sa;
    int srv = -1, up[2] = {-1, -1};
    pid_t kids[MS06_LISTENER_CONNECTIONS] = {0};
    int accepted[MS06_LISTENER_CONNECTIONS] = {-1, -1, -1, -1};

    memset(&r, 0, sizeof(r));
    r.case_id = MS06_CASE_LISTENER;

    do {
        srv = make_listener(&sa);
        if (srv < 0 || xpipe(up) < 0) { r.status = MS06_ST_IO_ERROR; break; }

        for (unsigned i = 0; i < MS06_LISTENER_CONNECTIONS; ++i) {
            kids[i] = fork();
            if (kids[i] < 0) { r.status = MS06_ST_IO_ERROR; break; }
            if (kids[i] == 0) {
                int c = socket(AF_INET, SOCK_STREAM, 0);
                char echo;
                unsigned ident = i + 1u; /* non-zero identity byte */
                if (c < 0) _exit(2);
                if (connect(c, (struct sockaddr *)&sa, sizeof(sa)) < 0) _exit(2);
                close(srv); close(up[0]);
                if (send(c, &ident, 1, 0) != 1) _exit(2);
                if (write_full(up[1], "C", 1) < 0) _exit(2);
                if (recv(c, &echo, 1, 0) != 1 ||
                    !ms06_listener_reply_matches(ident, (unsigned char)echo)) {
                    _exit(3);
                }
                _exit(0);
            }
        }
        if (r.status != MS06_ST_COMPLETED && r.status != 0) break;
        close(up[1]); up[1] = -1;

        /* Wait until every connector is established and has sent its identity
         * BEFORE accepting anything: all four then sit in the backlog while
         * the listener stays idle. */
        for (unsigned i = 0; i < MS06_LISTENER_CONNECTIONS; ++i) {
            char b;
            if (read_byte_deadline(up[0], &b, t0, dl) < 0 || b != 'C') {
                r.status = MS06_ST_TIMEOUT;
                break;
            }
        }
        if (r.status == MS06_ST_TIMEOUT) break;

        uint32_t seen_identities = 0;
        int accept_failed = 0;
        for (unsigned i = 0; i < MS06_LISTENER_CONNECTIONS; ++i) {
            uint32_t ev = 0;
            if (poll_events_deadline(srv, POLLIN, t0, dl, &ev) != 1 ||
                !(ev & MS06_EV_IN)) {
                r.status = MS06_ST_TIMEOUT;
                accept_failed = 1;
                break;
            }
            accepted[i] = accept(srv, NULL, NULL);
            if (accepted[i] < 0) {
                r.status = MS06_ST_IO_ERROR;
                r.err = errno;
                accept_failed = 1;
                break;
            }
        }
        if (accept_failed) break;

        /* Each accepted connection must carry a distinct identity and answer
         * its echo: unique-accept plus working data path on hidden sockets. */
        for (unsigned i = 0; i < MS06_LISTENER_CONNECTIONS; ++i) {
            unsigned char ident = 0;
            if (recv_exact_deadline(accepted[i], &ident, 1, t0, dl) < 0) {
                r.status = MS06_ST_IO_ERROR;
                break;
            }
            if (ident < 1 || ident > MS06_LISTENER_CONNECTIONS ||
                (seen_identities & (1u << ident)) != 0) {
                r.status = MS06_ST_EVENT_MISMATCH; /* duplicate or bogus accept */
                break;
            }
            seen_identities |= 1u << ident;
            unsigned char reply = (unsigned char)~(unsigned)ident;
            if (send_all_deadline(accepted[i], &reply, 1, t0, dl) < 0) {
                r.status = MS06_ST_IO_ERROR;
                break;
            }
        }
        if (r.status != 0 && r.status != MS06_ST_COMPLETED) break;

        r.cleanup_ok = 1;
        for (unsigned i = 0; i < MS06_LISTENER_CONNECTIONS; ++i) {
            int st;
            while (waitpid(kids[i], &st, 0) < 0 && errno == EINTR) {}
            if (!child_exited_ok(st)) r.cleanup_ok = 0;
        }
        if (!r.cleanup_ok) { r.status = MS06_ST_CLEANUP_FAIL; break; }
        r.status = MS06_ST_COMPLETED;
    } while (0);

    if (r.status != MS06_ST_COMPLETED) reap_children(kids, MS06_LISTENER_CONNECTIONS);
    if (up[0] >= 0) close(up[0]);
    if (up[1] >= 0) close(up[1]);
    for (unsigned i = 0; i < MS06_LISTENER_CONNECTIONS; ++i) {
        if (accepted[i] >= 0) close(accepted[i]);
    }
    if (srv >= 0) close(srv);

    {
        int ok = ms06_case_verdict(&r);
        ms06_report(ok, MS06_CASE_LISTENER,
                    ok ? NULL : "backlog connections not accepted uniquely inside deadline");
        return ok;
    }
}

/* ── Case 4: nonblock-connect-error ─────────────────────────────────── */

static int run_nonblock_connect_error(void)
{
    const uint64_t dl = MS06_CONNECT_ERR_DEADLINE_MS;
    const uint64_t t0 = now_us();
    struct ms06_case_result r;
    int probe_listener = -1, c = -1;
    struct sockaddr_in sa;

    memset(&r, 0, sizeof(r));
    r.case_id = MS06_CASE_NONBLOCK_CONNECT_ERROR;
    r.want_err = ECONNREFUSED;

    do {
        probe_listener = make_listener(&sa);
        if (probe_listener < 0) { r.status = MS06_ST_IO_ERROR; break; }
        close(probe_listener); /* nothing listens on this port anymore */
        probe_listener = -1;

        c = socket(AF_INET, SOCK_STREAM, 0);
        if (c < 0 || set_nonblock(c) < 0) { r.status = MS06_ST_IO_ERROR; break; }

        errno = 0;
        int rc = connect(c, (struct sockaddr *)&sa, sizeof(sa));
        if (rc == 0) {
            r.status = MS06_ST_EVENT_MISMATCH; /* refusal expected, got success */
            break;
        }
        if (errno != EINPROGRESS && errno != ECONNREFUSED) {
            r.status = MS06_ST_IO_ERROR;
            r.err = errno;
            break;
        }

        uint32_t ev = 0;
        if (poll_events_deadline(c, POLLOUT, t0, dl, &ev) != 1) {
            r.status = MS06_ST_TIMEOUT;
            break;
        }
        r.events = ev;

        /* Stability: repeated observations must return the same category
         * (Iteration 004 terminal-first semantics), never WouldBlock drift. */
        int e1 = 0, e2 = 0;
        socklen_t elen = sizeof(e1);
        if (getsockopt(c, SOL_SOCKET, SO_ERROR, &e1, &elen) < 0) {
            r.status = MS06_ST_IO_ERROR;
            r.err = errno;
            break;
        }
        elen = sizeof(e2);
        if (getsockopt(c, SOL_SOCKET, SO_ERROR, &e2, &elen) < 0 || e1 == 0 || e1 != e2) {
            r.status = MS06_ST_EVENT_MISMATCH; /* error category drifted */
            break;
        }
        errno = 0;
        (void)connect(c, (struct sockaddr *)&sa, sizeof(sa));
        if (errno != e1) {
            r.status = MS06_ST_EVENT_MISMATCH; /* reconnect attempt saw another category */
            break;
        }
        r.err = e1;
        r.status = MS06_ST_COMPLETED;
    } while (0);

    if (probe_listener >= 0) close(probe_listener);
    if (c >= 0) close(c);
    r.cleanup_ok = 1;

    {
        int ok = ms06_case_verdict(&r);
        ms06_report(ok, MS06_CASE_NONBLOCK_CONNECT_ERROR,
                    ok ? NULL : "connect refusal not reported as stable OUT|ERR + ECONNREFUSED");
        return ok;
    }
}

/* ── Case 5: quiet ──────────────────────────────────────────────────── */

static int run_quiet(void)
{
    const uint64_t dl = MS06_QUIET_DEADLINE_MS;
    const uint64_t t0 = now_us();
    struct ms06_case_result r;
    struct sockaddr_in sa;
    int srv = -1, cfd = -1, up[2] = {-1, -1};
    pid_t kid = -1;

    memset(&r, 0, sizeof(r));
    r.case_id = MS06_CASE_QUIET;
    r.want_err = 0;

    do {
        srv = make_listener(&sa);
        if (srv < 0 || xpipe(up) < 0) { r.status = MS06_ST_IO_ERROR; break; }

        kid = fork();
        if (kid < 0) { r.status = MS06_ST_IO_ERROR; break; }
        if (kid == 0) {
            int c = socket(AF_INET, SOCK_STREAM, 0);
            char b;
            if (c < 0) _exit(2);
            if (connect(c, (struct sockaddr *)&sa, sizeof(sa)) < 0) _exit(2);
            close(srv); close(up[0]);
            if (write_full(up[1], "R", 1) < 0) _exit(2);
            /* Echo server: stays silent unless pinged, exits on EOF. */
            for (;;) {
                ssize_t n = recv(c, &b, 1, 0);
                if (n == 0) _exit(0);
                if (n < 0) _exit(errno == EINTR ? 0 : 2);
                if (send(c, &b, 1, 0) != 1) _exit(2);
            }
        }
        close(up[1]); up[1] = -1;

        {
            uint32_t ev = 0;
            if (poll_events_deadline(srv, POLLIN, t0, dl, &ev) != 1 ||
                !(ev & MS06_EV_IN)) {
                r.status = MS06_ST_TIMEOUT;
                break;
            }
            cfd = accept(srv, NULL, NULL);
            if (cfd < 0) { r.status = MS06_ST_IO_ERROR; r.err = errno; break; }
        }
        {
            char b;
            if (read_byte_deadline(up[0], &b, t0, dl) < 0 || b != 'R') {
                r.status = MS06_ST_IO_ERROR;
                break;
            }
        }

        /* Quiet window: an idle connection must produce zero readiness events
         * on the read/terminal/error directions. Normal writability is not
         * armed (established sockets are expected to stay writable), so any
         * observed event here is spurious bridge progress (Iteration 001
         * Active quiet invariant observed from the public ABI). */
        for (unsigned i = 0; i < MS06_QUIET_WINDOW_ITERS; ++i) {
            int64_t rem = ms06_deadline_remaining_ms(t0, now_us(), dl);
            if (rem <= 0) { r.status = MS06_ST_TIMEOUT; break; }
            struct pollfd pfd;
            memset(&pfd, 0, sizeof(pfd));
            pfd.fd = cfd;
            pfd.events = ms06_quiet_interest();
            int slice = rem < (int64_t)MS06_QUIET_SLICE_MS ? (int)rem : (int)MS06_QUIET_SLICE_MS;
            int rc = poll(&pfd, 1, slice);
            if (rc < 0) {
                if (errno == EINTR) continue;
                r.status = MS06_ST_IO_ERROR;
                r.err = errno;
                break;
            }
            if (rc > 0 && pfd.revents != 0) {
                r.events = (uint32_t)pfd.revents & 0xFFFFu;
                if (!ms06_events_satisfy(MS06_CASE_QUIET, r.events)) {
                    r.status = MS06_ST_EVENT_MISMATCH; /* spurious read/terminal while idle */
                    break;
                }
            }
        }
        if (r.status != 0 && r.status != MS06_ST_COMPLETED) break;
        r.events = 0;

        /* Liveness: after silence the same connection still works end-to-end,
         * proving the window observed genuine quiescence, not a wedged stack. */
        {
            char q = 'Q', got = 0;
            if (send_all_deadline(cfd, &q, 1, t0, dl) < 0 ||
                recv_exact_deadline(cfd, &got, 1, t0, dl) < 0 ||
                got != 'Q') {
                r.status = MS06_ST_TIMEOUT;
                break;
            }
        }
        r.status = MS06_ST_COMPLETED;
    } while (0);

    if (cfd >= 0) close(cfd); /* holder observes EOF and exits */
    if (kid > 0) {
        if (r.status == MS06_ST_COMPLETED) {
            int st;
            while (waitpid(kid, &st, 0) < 0 && errno == EINTR) {}
            r.cleanup_ok = child_exited_ok(st);
        } else {
            reap_children(&kid, 1);
            r.cleanup_ok = 0;
        }
    }
    if (!r.cleanup_ok && r.status == MS06_ST_COMPLETED) r.status = MS06_ST_CLEANUP_FAIL;
    if (up[0] >= 0) close(up[0]);
    if (up[1] >= 0) close(up[1]);
    if (srv >= 0) close(srv);

    {
        int ok = ms06_case_verdict(&r);
        ms06_report(ok, MS06_CASE_QUIET,
                    ok ? NULL : "idle connection showed spurious readiness or lost liveness");
        return ok;
    }
}

/* ── Case 6: continuous-traffic ─────────────────────────────────────── */

static void ms06_fill_message(unsigned char *msg, unsigned seq)
{
    for (unsigned j = 0; j < MS06_TRAFFIC_MSG_SIZE; ++j) {
        msg[j] = (unsigned char)((seq + j) & 0xFFu);
    }
}

static int run_continuous_traffic(void)
{
    const uint64_t dl = MS06_TRAFFIC_DEADLINE_MS;
    const uint64_t t0 = now_us();
    struct ms06_case_result r;
    struct sockaddr_in sa;
    int srv = -1, cfd = -1, up[2] = {-1, -1}, down[2] = {-1, -1};
    pid_t kid = -1;

    memset(&r, 0, sizeof(r));
    r.case_id = MS06_CASE_CONTINUOUS_TRAFFIC;
    r.want_err = 0;

    do {
        srv = make_listener(&sa);
        if (srv < 0 || xpipe(up) < 0 || xpipe(down) < 0) {
            r.status = MS06_ST_IO_ERROR;
            break;
        }

        kid = fork();
        if (kid < 0) { r.status = MS06_ST_IO_ERROR; break; }
        if (kid == 0) {
            int c = socket(AF_INET, SOCK_STREAM, 0);
            unsigned char msg[MS06_TRAFFIC_MSG_SIZE], seen[MS06_TRAFFIC_MSG_SIZE];
            char b;
            if (c < 0) _exit(2);
            if (connect(c, (struct sockaddr *)&sa, sizeof(sa)) < 0) _exit(2);
            close(srv); close(up[0]); close(down[1]);
            if (write_full(up[1], "C", 1) < 0) _exit(2);
            if (read_byte_deadline(down[0], &b, t0, dl) < 0 || b != 'G') _exit(2);
            /* Pipeline all sends first, then verify every echo in order:
             * both directions are in flight simultaneously. */
            for (unsigned m = 0; m < MS06_TRAFFIC_MESSAGES; ++m) {
                ms06_fill_message(msg, m);
                if (send_all_deadline(c, msg, sizeof(msg), t0, dl) < 0) _exit(3);
            }
            for (unsigned m = 0; m < MS06_TRAFFIC_MESSAGES; ++m) {
                ms06_fill_message(msg, m);
                if (recv_exact_deadline(c, seen, sizeof(seen), t0, dl) < 0) _exit(4);
                if (memcmp(msg, seen, sizeof(msg)) != 0) _exit(5);
            }
            _exit(0);
        }
        close(up[1]); up[1] = -1;
        close(down[0]); down[0] = -1;

        {
            uint32_t ev = 0;
            if (poll_events_deadline(srv, POLLIN, t0, dl, &ev) != 1 ||
                !(ev & MS06_EV_IN)) {
                r.status = MS06_ST_TIMEOUT;
                break;
            }
            cfd = accept(srv, NULL, NULL);
            if (cfd < 0) { r.status = MS06_ST_IO_ERROR; r.err = errno; break; }
        }
        {
            char b;
            if (read_byte_deadline(up[0], &b, t0, dl) < 0 || b != 'C' ||
                write_full(down[1], "G", 1) < 0) {
                r.status = MS06_ST_IO_ERROR;
                break;
            }
        }

        /* Echo every message in arrival order: sustained traffic must not
         * starve the runner and must not reorder within the stream. */
        unsigned expected = 0;
        int echo_failed = 0;
        while (expected < MS06_TRAFFIC_MESSAGES) {
            unsigned char got[MS06_TRAFFIC_MSG_SIZE], want[MS06_TRAFFIC_MSG_SIZE];
            if (recv_exact_deadline(cfd, got, sizeof(got), t0, dl) < 0) {
                r.status = MS06_ST_TIMEOUT;
                echo_failed = 1;
                break;
            }
            ms06_fill_message(want, expected);
            if (memcmp(got, want, sizeof(got)) != 0) {
                r.status = MS06_ST_EVENT_MISMATCH; /* stream reordered or lost data */
                echo_failed = 1;
                break;
            }
            if (send_all_deadline(cfd, got, sizeof(got), t0, dl) < 0) {
                r.status = MS06_ST_IO_ERROR;
                r.err = errno;
                echo_failed = 1;
                break;
            }
            ++expected;
        }
        if (echo_failed) break;

        int st;
        while (waitpid(kid, &st, 0) < 0 && errno == EINTR) {}
        r.cleanup_ok = child_exited_ok(st);
        if (!r.cleanup_ok) { r.status = MS06_ST_CLEANUP_FAIL; break; }
        r.status = MS06_ST_COMPLETED;
    } while (0);

    if (r.status != MS06_ST_COMPLETED) reap_children(&kid, 1);
    if (up[0] >= 0) close(up[0]);
    if (up[1] >= 0) close(up[1]);
    if (down[0] >= 0) close(down[0]);
    if (down[1] >= 0) close(down[1]);
    if (cfd >= 0) close(cfd);
    if (srv >= 0) close(srv);

    {
        int ok = ms06_case_verdict(&r);
        ms06_report(ok, MS06_CASE_CONTINUOUS_TRAFFIC,
                    ok ? NULL : "sustained bidirectional traffic lost, reordered or starved");
        return ok;
    }
}

/* ── Case 7: close-error ────────────────────────────────────────────── */

static int run_close_error(void)
{
    const uint64_t dl = MS06_CLOSE_DEADLINE_MS;
    const uint64_t t0 = now_us();
    struct ms06_case_result r;
    struct sockaddr_in sa;
    int srv = -1, cfd = -1, up[2] = {-1, -1}, down[2] = {-1, -1};
    pid_t kid = -1;

    memset(&r, 0, sizeof(r));
    r.case_id = MS06_CASE_CLOSE_ERROR;
    r.want_err = 0;

    do {
        srv = make_listener(&sa);
        if (srv < 0 || xpipe(up) < 0 || xpipe(down) < 0) {
            r.status = MS06_ST_IO_ERROR;
            break;
        }

        kid = fork();
        if (kid < 0) { r.status = MS06_ST_IO_ERROR; break; }
        if (kid == 0) {
            int c = socket(AF_INET, SOCK_STREAM, 0);
            char b;
            if (c < 0) _exit(2);
            if (connect(c, (struct sockaddr *)&sa, sizeof(sa)) < 0) _exit(2);
            close(srv); close(up[0]); close(down[1]);
            if (write_full(up[1], "R", 1) < 0) _exit(2);
            if (read_byte_deadline(down[0], &b, t0, dl) < 0 || b != 'G') _exit(2);
            close(c); /* graceful full close: FIN then FIN/ACK exchange */
            _exit(0);
        }
        close(up[1]); up[1] = -1;
        close(down[0]); down[0] = -1;

        {
            uint32_t ev = 0;
            if (poll_events_deadline(srv, POLLIN, t0, dl, &ev) != 1 ||
                !(ev & MS06_EV_IN)) {
                r.status = MS06_ST_TIMEOUT;
                break;
            }
            cfd = accept(srv, NULL, NULL);
            if (cfd < 0) { r.status = MS06_ST_IO_ERROR; r.err = errno; break; }
        }
        {
            char b;
            if (read_byte_deadline(up[0], &b, t0, dl) < 0 || b != 'R' ||
                write_full(down[1], "G", 1) < 0) {
                r.status = MS06_ST_IO_ERROR;
                break;
            }
        }

        uint32_t ev = 0;
        if (poll_events_deadline(cfd, POLLIN | POLLRDHUP, t0, dl, &ev) != 1) {
            r.status = MS06_ST_TIMEOUT;
            break;
        }
        r.events = ev;

        /* A graceful peer close is EOF-family readiness with stable zero
         * reads; the still-open local write half is not required to reach
         * EPIPE (peer FIN closes only the receive half). POLLERR here would
         * mean a normal close was misclassified as a device fault. */
        char b;
        int r1 = recv(cfd, &b, 1, 0);
        int r2 = recv(cfd, &b, 1, 0);
        if (!ms06_peer_fin_eof_valid(ev, r1, r2)) {
            r.status = (r1 != 0 || r2 != 0) ? MS06_ST_IO_ERROR : MS06_ST_EVENT_MISMATCH;
            r.err = errno;
            break;
        }
        if (recv(cfd, &b, 1, MSG_PEEK | MSG_DONTWAIT) != 0 &&
            !(errno == EAGAIN || errno == EWOULDBLOCK)) {
            r.status = MS06_ST_EVENT_MISMATCH; /* data appeared after clean EOF */
            break;
        }
        r.status = MS06_ST_COMPLETED;
    } while (0);

    if (kid > 0) {
        if (r.status == MS06_ST_COMPLETED) {
            int st;
            while (waitpid(kid, &st, 0) < 0 && errno == EINTR) {}
            r.cleanup_ok = child_exited_ok(st);
        } else {
            reap_children(&kid, 1);
            r.cleanup_ok = 0;
        }
    }
    if (!r.cleanup_ok && r.status == MS06_ST_COMPLETED) r.status = MS06_ST_CLEANUP_FAIL;
    if (up[0] >= 0) close(up[0]);
    if (up[1] >= 0) close(up[1]);
    if (down[0] >= 0) close(down[0]);
    if (down[1] >= 0) close(down[1]);
    if (cfd >= 0) close(cfd);
    if (srv >= 0) close(srv);

    {
        int ok = ms06_case_verdict(&r);
        ms06_report(ok, MS06_CASE_CLOSE_ERROR,
                    ok ? NULL : "graceful close misclassified or unstable after EOF");
        return ok;
    }
}

/* ── Multiwaiter orchestration (cases 8-12) ─────────────────────────── */

struct mw_cfg {
    int mode;
    int sfd;
    int arm_fd;  /* exact arm channel: worker -> parent ('A' after synchronous EPOLL_CTL_ADD) */
    int res_fd;
    uint64_t t0;
    uint64_t dl;
    int exact;   /* exact 64/65: private epoll pre-registration + arm flow */
};

/* One registration attempt: block until a wake or the overall deadline.
 * Returns 1 on wake (*ev holds revents, possibly empty for a replacement
 * wake), 0 at deadline, -1 fatal. Each call registers afresh, so returning
 * without completion is itself the re-register step of the state machine. */
static int mw_epoll_wait_loop(int ep, uint64_t t0, uint64_t dl, uint32_t *ev)
{
    for (;;) {
        int64_t rem = ms06_deadline_remaining_ms(t0, now_us(), dl);
        if (rem <= 0) {
            *ev = 0;
            return 0;
        }
        struct epoll_event hit;
        memset(&hit, 0, sizeof(hit));
        int rc = epoll_wait(ep, &hit, 1, (int)rem);
        if (rc < 0) {
            if (errno == EINTR) continue;
            return -1;
        }
        if (rc == 0) continue;
        *ev = (uint32_t)(hit.events & 0xFFFFu);
        return 1;
    }
}

static int mw_wait_once(int mode, int fd, uint64_t t0, uint64_t dl, uint32_t *ev)
{
    if (mode == MS06_WAIT_POLL) {
        return poll_events_deadline(fd, POLLIN, t0, dl, ev);
    }
    if (mode == MS06_WAIT_SELECT) {
        for (;;) {
            int64_t rem = ms06_deadline_remaining_ms(t0, now_us(), dl);
            if (rem <= 0) {
                *ev = 0;
                return 0;
            }
            fd_set rs, es;
            FD_ZERO(&rs);
            FD_SET(fd, &rs);
            FD_ZERO(&es);
            FD_SET(fd, &es);
            struct timeval tv;
            tv.tv_sec = (time_t)(rem / 1000);
            tv.tv_usec = (suseconds_t)((rem % 1000) * 1000);
            int rc = select(fd + 1, &rs, NULL, &es, &tv);
            if (rc < 0) {
                if (errno == EINTR) continue;
                return -1;
            }
            if (rc == 0) continue;
            *ev = (uint32_t)((FD_ISSET(fd, &rs) ? MS06_EV_IN : 0u) |
                             (FD_ISSET(fd, &es) ? MS06_EV_ERR : 0u));
            return 1;
        }
    }
    /* epoll bits are numerically identical to poll bits on Linux. */
    int ep = epoll_create1(0);
    if (ep < 0) return -1;
    struct epoll_event req;
    memset(&req, 0, sizeof(req));
    req.events = EPOLLIN | EPOLLRDHUP;
    req.data.fd = fd;
    if (epoll_ctl(ep, EPOLL_CTL_ADD, fd, &req) < 0) {
        close(ep);
        return -1;
    }
    int out = mw_epoll_wait_loop(ep, t0, dl, ev);
    close(ep);
    return out;
}

static void mw_emit(const struct mw_cfg *cfg, const struct ms06_waiter_record *rec)
{
    (void)write_full(cfg->res_fd, rec, sizeof(*rec));
}

static int mw_worker_body(const struct mw_cfg *cfg)
{
    struct ms06_waiter_record rec;
    char b;
    memset(&rec, 0, sizeof(rec));
    rec.pid = (long)getpid();

    /* Exact 64/65: the worker's own epoll instance registers the shared
     * socket synchronously (EPOLL_CTL_ADD), then publishes one arm byte
     * BEFORE any trigger data can exist. The parent counts exactly n arms
     * and only then releases the N units. The registration stays live for
     * the whole wait: replacement/no-event recheck is kernel-internal and
     * the guest never fabricates a user-space empty-event observation. */
    int exact_ep = -1;
    if (cfg->exact) {
        exact_ep = epoll_create1(0);
        if (exact_ep < 0) { mw_emit(cfg, &rec); return 3; }
        struct epoll_event req;
        memset(&req, 0, sizeof(req));
        req.events = EPOLLIN | EPOLLRDHUP;
        req.data.fd = cfg->sfd;
        if (epoll_ctl(exact_ep, EPOLL_CTL_ADD, cfg->sfd, &req) < 0) {
            close(exact_ep);
            mw_emit(cfg, &rec);
            return 3;
        }
        rec.phases |= MS06_PHASE_REGISTERED;
        const char arm = MS06_ARM_BYTE;
        if (write_full(cfg->arm_fd, &arm, 1) < 0) {
            close(exact_ep);
            mw_emit(cfg, &rec);
            return 3;
        }
    }

    for (;;) {
        if (ms06_deadline_expired(cfg->t0, now_us(), cfg->dl)) {
            if (exact_ep >= 0) close(exact_ep);
            mw_emit(cfg, &rec);
            return 3;
        }

        uint32_t ev = 0;
        int rc;
        if (cfg->exact) {
            rc = mw_epoll_wait_loop(exact_ep, cfg->t0, cfg->dl, &ev);
        } else {
            /* Re-entering the wait after a not-ready recheck IS the
             * re-register step; mark it before arming again. */
            if (rec.phases & MS06_PHASE_RECHECK_NG) {
                rec.phases |= MS06_PHASE_REREGISTERED;
            }
            rc = mw_wait_once(cfg->mode, cfg->sfd, cfg->t0, cfg->dl, &ev);
            if (rc > 0) rec.phases |= MS06_PHASE_REGISTERED;
        }
        if (rc <= 0) {
            if (exact_ep >= 0) close(exact_ep);
            mw_emit(cfg, &rec);
            return 3;
        }

        if (ev & MS06_EV_ERR) {
            if (exact_ep >= 0) close(exact_ep);
            mw_emit(cfg, &rec);
            return 3;
        }
        if (ev & MS06_EV_IN) {
            /* Consume exactly one trigger unit, nonblockingly: a readiness
             * race must never push a consuming recheck past the deadline. */
            if (recv(cfg->sfd, &b, 1, MSG_DONTWAIT) == 1) {
                if (b == MS06_TRIGGER_BYTE) {
                    rec.completions = 1;
                    if (exact_ep >= 0) close(exact_ep);
                    mw_emit(cfg, &rec);
                    return 0;
                }
                if (exact_ep >= 0) close(exact_ep);
                mw_emit(cfg, &rec);
                return 3;
            }
            if (errno == EAGAIN || errno == EWOULDBLOCK) continue;
            if (exact_ep >= 0) close(exact_ep);
            mw_emit(cfg, &rec);
            return 3;
        }

        /* Exact mode: a wake without data leaves the registration live and
         * simply loops back (kernel rechecks/re-registers internally); the
         * guest claims no user-space empty event. Non-exact mode keeps the
         * generic replacement-class bookkeeping. */
        if (!cfg->exact) {
            ssize_t pn = recv(cfg->sfd, &b, 1, MSG_PEEK | MSG_DONTWAIT);
            if (pn == 1) {
                if (recv(cfg->sfd, &b, 1, MSG_DONTWAIT) == 1 && b == MS06_TRIGGER_BYTE) {
                    rec.completions = 1; /* data raced ahead of the wake */
                    mw_emit(cfg, &rec);
                    return 0;
                }
                mw_emit(cfg, &rec);
                return 3;
            }
            if (pn == 0 || !(errno == EAGAIN || errno == EWOULDBLOCK)) {
                mw_emit(cfg, &rec);
                return 3;
            }
            rec.phases |= MS06_PHASE_WOKEN | MS06_PHASE_RECHECK_NG;
            rec.replacements += 1;
        }
    }
}

static int run_multiwaiter(int case_id, int mode, uint32_t n_waiters, uint64_t dl)
{
    const uint64_t t0 = now_us();
    struct ms06_case_result r;
    struct sockaddr_in sa;
    struct ms06_waiter_record records[MS06_MAX_WAITERS];
    int srv = -1, sfd = -1;
    int up[2] = {-1, -1}, down[2] = {-1, -1};   /* connector handshake */
    int arm[2] = {-1, -1}, res[2] = {-1, -1};   /* exact arms + final records */
    pid_t ckid = -1;
    pid_t kids[MS06_MAX_WAITERS] = {0};
    const int exact = (case_id == MS06_CASE_WAITER_64 ||
                       case_id == MS06_CASE_WAITER_65_REREGISTER);

    memset(&r, 0, sizeof(r));
    r.case_id = case_id;
    r.want_err = 0;

    do {
        if (n_waiters > MS06_MAX_WAITERS) { r.status = MS06_ST_IO_ERROR; break; }
        srv = make_listener(&sa);
        if (srv < 0 || xpipe(up) < 0 || xpipe(down) < 0 ||
            xpipe(arm) < 0 || xpipe(res) < 0) {
            r.status = MS06_ST_IO_ERROR;
            break;
        }

        ckid = fork();
        if (ckid < 0) { r.status = MS06_ST_IO_ERROR; break; }
        if (ckid == 0) {
            int c = socket(AF_INET, SOCK_STREAM, 0);
            if (c < 0) _exit(2);
            if (connect(c, (struct sockaddr *)&sa, sizeof(sa)) < 0) _exit(2);
            close(srv); close(arm[0]); close(arm[1]);
            close(res[0]); close(res[1]); close(up[0]); close(down[1]);
            if (write_full(up[1], "L", 1) < 0) _exit(2);
            char g;
            if (read_byte_deadline(down[0], &g, t0, dl) < 0 || g != 'G') _exit(2);
            /* The peer provides exactly n_waiters consumable trigger units:
             * one per distinct waiter identity, so every waiter can complete
             * its own I/O instead of racing for a single byte. */
            char units[MS06_MAX_WAITERS];
            if (read_full_deadline(down[0], units, n_waiters, t0, dl) < 0) _exit(2);
            for (uint32_t i = 0; i < n_waiters; ++i) {
                if (send(c, &units[i], 1, 0) != 1) _exit(2);
            }
            _exit(0);
        }
        close(up[1]); up[1] = -1;
        close(down[0]); down[0] = -1;

        {
            uint32_t ev = 0;
            if (poll_events_deadline(srv, POLLIN, t0, dl, &ev) != 1 ||
                !(ev & MS06_EV_IN)) {
                r.status = MS06_ST_TIMEOUT;
                break;
            }
            sfd = accept(srv, NULL, NULL);
            if (sfd < 0) { r.status = MS06_ST_IO_ERROR; r.err = errno; break; }
        }

        for (uint32_t i = 0; i < n_waiters; ++i) {
            kids[i] = fork();
            if (kids[i] < 0) { r.status = MS06_ST_IO_ERROR; break; }
            if (kids[i] == 0) {
                struct mw_cfg cfg;
                cfg.mode = mode;
                cfg.sfd = sfd;
                cfg.arm_fd = arm[1];
                cfg.res_fd = res[1];
                cfg.t0 = t0;
                cfg.dl = dl;
                cfg.exact = exact;
                close(srv); close(up[0]); close(up[1]);
                close(down[0]); close(down[1]);
                close(arm[0]); close(res[0]);
                _exit(mw_worker_body(&cfg));
            }
        }
        if (r.status == MS06_ST_IO_ERROR) break;

        close(arm[1]); arm[1] = -1;
        close(res[1]); res[1] = -1;

        {
            char l;
            if (read_byte_deadline(up[0], &l, t0, dl) < 0 || l != 'L') {
                r.status = MS06_ST_IO_ERROR;
                break;
            }
        }

        /* Release the trigger only under the exact choreography contract:
         *  - exact 64/65: every distinct worker synchronously registers its
         *    own epoll interest (EPOLL_CTL_ADD) and publishes one arm byte
         *    BEFORE any data exists; the parent counts exactly n_waiters arms
         *    and the unit count equals the waiter count. Replacement/no-event
         *    recheck and re-register are kernel-internal and never fabricated
         *    as guest observations.
         *  - 4-waiter: no arm barrier; check-register-recheck converges and
         *    the unit count still equals the waiter count. */
        uint32_t armed = 0;
        if (exact) {
            char arm_bytes[MS06_MAX_WAITERS];
            if (read_full_deadline(arm[0], arm_bytes, n_waiters, t0, dl) < 0) {
                r.status = MS06_ST_TIMEOUT; /* never saw all N arms */
                break;
            }
            for (uint32_t i = 0; i < n_waiters; ++i) {
                if (arm_bytes[i] == MS06_ARM_BYTE) ++armed;
            }
            if (!ms06_exact_arms_complete(armed, n_waiters)) {
                r.status = MS06_ST_EVENT_MISMATCH; /* arm barrier violated */
                break;
            }
        }
        if (exact && !ms06_exact_mode_ok(mode)) {
            r.status = MS06_ST_EVENT_MISMATCH; /* exact cases must arm via epoll */
            break;
        }
        if (!ms06_trigger_units_valid(n_waiters, n_waiters)) {
            r.status = MS06_ST_EVENT_MISMATCH; /* unit-count contract violated */
            break;
        }
        {
            char units[MS06_MAX_WAITERS];
            memset(units, MS06_TRIGGER_BYTE, sizeof(units));
            if (write_full(down[1], "G", 1) < 0 ||
                write_full(down[1], units, (size_t)n_waiters) < 0) {
                r.status = MS06_ST_IO_ERROR;
                break;
            }
        }

        int connector_ok = 0;
        {
            int st;
            while (waitpid(ckid, &st, 0) < 0 && errno == EINTR) {}
            connector_ok = child_exited_ok(st);
        }

        if (!connector_ok ||
            read_full_deadline(res[0], records,
                               (size_t)n_waiters * sizeof(records[0]), t0, dl) < 0) {
            r.status = MS06_ST_TIMEOUT;
            break;
        }

        int workers_ok = 1;
        for (uint32_t i = 0; i < n_waiters; ++i) {
            int st;
            while (waitpid(kids[i], &st, 0) < 0 && errno == EINTR) {}
            if (!child_exited_ok(st)) workers_ok = 0;
        }
        if (!workers_ok) { r.status = MS06_ST_CLEANUP_FAIL; break; }

        struct ms06_waiter_set set;
        set.capacity = n_waiters;

        r.cleanup_ok = connector_ok && workers_ok;
        if (ms06_waiter_set_accepts(&set, records, n_waiters)) {
            r.status = MS06_ST_COMPLETED;
            r.events = MS06_EV_IN;
        } else {
            r.status = MS06_ST_EVENT_MISMATCH; /* aggregate identity/completion contract violated */
        }
    } while (0);

    if (r.status != MS06_ST_COMPLETED) {
        reap_children(kids, MS06_MAX_WAITERS);
        if (ckid > 0) reap_children(&ckid, 1);
    }
    if (up[0] >= 0) close(up[0]);
    if (up[1] >= 0) close(up[1]);
    if (down[0] >= 0) close(down[0]);
    if (down[1] >= 0) close(down[1]);
    if (arm[0] >= 0) close(arm[0]);
    if (arm[1] >= 0) close(arm[1]);
    if (res[0] >= 0) close(res[0]);
    if (res[1] >= 0) close(res[1]);
    if (sfd >= 0) close(sfd);
    if (srv >= 0) close(srv);

    {
        int verdict = ms06_case_verdict(&r);
        ms06_report(verdict, case_id,
                    verdict ? NULL : "multiwaiter identity/completion contract violated");
        return verdict;
    }
}

static int run_poll_multiwaiter(void)
{
    return run_multiwaiter(MS06_CASE_POLL_MULTIWAITER, MS06_WAIT_POLL,
                           MS06_MULTIWAITER_COUNT, MS06_MULTIWAITER_DEADLINE_MS);
}

static int run_select_multiwaiter(void)
{
    return run_multiwaiter(MS06_CASE_SELECT_MULTIWAITER, MS06_WAIT_SELECT,
                           MS06_MULTIWAITER_COUNT, MS06_MULTIWAITER_DEADLINE_MS);
}

static int run_epoll_multiwaiter(void)
{
    return run_multiwaiter(MS06_CASE_EPOLL_MULTIWAITER, MS06_WAIT_EPOLL,
                           MS06_MULTIWAITER_COUNT, MS06_MULTIWAITER_DEADLINE_MS);
}

static int run_waiter_64(void)
{
    return run_multiwaiter(MS06_CASE_WAITER_64, MS06_WAIT_EPOLL,
                           64u, MS06_WAITER64_DEADLINE_MS);
}

static int run_waiter_65_reregister(void)
{
    return run_multiwaiter(MS06_CASE_WAITER_65_REREGISTER, MS06_WAIT_EPOLL,
                           65u, MS06_WAITER65_DEADLINE_MS);
}

/* ── Entry point ────────────────────────────────────────────────────── */

typedef int (*ms06_case_fn)(void);

static const ms06_case_fn ms06_runners[MS06_CASE_COUNT] = {
    run_tcp_timer,
    run_udp_progress,
    run_listener,
    run_nonblock_connect_error,
    run_quiet,
    run_continuous_traffic,
    run_close_error,
    run_poll_multiwaiter,
    run_select_multiwaiter,
    run_epoll_multiwaiter,
    run_waiter_64,
    run_waiter_65_reregister
};

int main(int argc, char **argv)
{
    signal(SIGPIPE, SIG_IGN);

    if (argc == 2 && strcmp(argv[1], "--print-cases") == 0) {
        for (int i = 0; i < MS06_CASE_COUNT; ++i) {
            printf("%s\n", ms06_case_name(i));
        }
        return 0;
    }

    printf("MS06_STACK_READINESS_START\n");
    printf("MS06_REVISION: %s\n", MS06_REVISION_DEFAULT);
    printf("MS06_ENVIRONMENT: %s\n", MS06_ENVIRONMENT_DEFAULT);
    fflush(stdout);

    int failed = 0;
    for (int i = 0; i < MS06_CASE_COUNT; ++i) {
        if (!ms06_runners[i]()) ++failed;
    }

    printf("MS06_STACK_READINESS_END\n");
    fflush(stdout);
    return failed ? 1 : 0;
}

#endif /* !MS06_STACK_READINESS_PROBE_TESTING */
