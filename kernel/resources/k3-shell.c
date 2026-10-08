/*
 * k3-shell.c — MS17 Iteration 003 (task T7): minimal interactive shell,
 * embedded in the K3 kernel image and spawned on the memory root with
 * stdio bound to the async tty (kernel/src/entry.rs, k3-shell shape).
 *
 * Command set (MS17 R7): help | echo <text> | sleep <ms 0..60000> |
 * exit [code 0..255]. Empty lines, unknown commands, malformed/overflowing
 * arguments and over-long lines report and return to the prompt. Reads and
 * writes retry EINTR and continue over short writes; a persistent error
 * reports (stderr) and exits. The command buffer is 256 B including the
 * NUL; a line that does not fit is discarded whole — the remaining bytes
 * up to its terminator are consumed and no truncated prefix is executed.
 * The tty's >1024 B whole-line discard arrives as EINVAL and is handled as
 * the same one-shot "line too long" feedback.
 *
 * Idle waits ride the blocking canonical read (no polling, no self-wake).
 * The shell prints its own exit code line; the kernel join value is not
 * the verdict (task.join() is observed to be 0 for exit_group).
 *
 * Test build (tests/k3_shell_test.c) defines K3_SHELL_TESTING and provides
 * the k3_boundary_* calls plus main(); the production build wires them to
 * read(2)/write(2)/nanosleep(2) on fd 0/1.
 */

#include <errno.h>
#include <limits.h>
#include <stddef.h>
#include <string.h>

#define K3_SHELL_CMD_CAP 256u   /* command buffer, including the NUL */

/* ── system boundary: the only surface the tests replace ─────────────── */

#ifdef K3_SHELL_TESTING

long k3_boundary_read(int fd, char *buf, unsigned long n);
long k3_boundary_write(int fd, const char *buf, unsigned long n);
int k3_boundary_nanosleep(long ms, long *rem_ms);

#else /* !K3_SHELL_TESTING */

#include <time.h>
#include <unistd.h>

static long k3_boundary_read(int fd, char *buf, unsigned long n)
{
    return read(fd, buf, n);
}

static long k3_boundary_write(int fd, const char *buf, unsigned long n)
{
    return write(fd, buf, n);
}

static int k3_boundary_nanosleep(long ms, long *rem_ms)
{
    struct timespec req;
    struct timespec rem;

    req.tv_sec = ms / 1000;
    req.tv_nsec = (ms % 1000) * 1000000L;
    rem.tv_sec = 0;
    rem.tv_nsec = 0;
    if (nanosleep(&req, &rem) != 0) {
        *rem_ms = rem.tv_sec * 1000 + rem.tv_nsec / 1000000;
        return -1;
    }
    *rem_ms = 0;
    return 0;
}

#endif /* K3_SHELL_TESTING */

/* ── results ─────────────────────────────────────────────────────────── */

enum k3_io_result {
    K3_IO_OK = 0,     /* one byte in *out */
    K3_IO_EOF,        /* end of input (0-read) */
    K3_IO_TOOLONG,    /* tty reported EINVAL: whole line discarded by the tty */
    K3_IO_ERROR       /* persistent error */
};

enum k3_line_result {
    K3_LINE_OK = 0,       /* complete line in buf (no '\n'), NUL-terminated */
    K3_LINE_EOF,          /* EOF with no byte of this line */
    K3_LINE_EOF_PARTIAL,  /* EOF ended a partial line; bytes discarded */
    K3_LINE_TOOLONG,      /* did not fit; consumed to terminator, never executed */
    K3_LINE_ERROR         /* persistent read error */
};

enum k3_parse_result {
    K3_PARSE_OK = 0,
    K3_PARSE_EMPTY,
    K3_PARSE_UNKNOWN,
    K3_PARSE_ARG_ERROR   /* wrong arg count, malformed or out-of-range value */
};

enum k3_cmd_kind {
    K3_CMD_NONE = 0,
    K3_CMD_HELP,
    K3_CMD_ECHO,
    K3_CMD_SLEEP,
    K3_CMD_EXIT
};

struct k3_command {
    enum k3_cmd_kind kind;
    const char *arg;        /* echo text / unknown word / malformed numeric */
    unsigned long arg_len;
    long number;            /* sleep ms or exit code when K3_PARSE_OK */
};

enum k3_dispatch_result {
    K3_DISPATCH_CONTINUE = 0,
    K3_DISPATCH_EXIT = 1    /* leave the shell; exit code in cmd->number */
};

/* ── output discipline ───────────────────────────────────────────────── */

static int k3_out_broken;

/* Write the whole buffer; retry EINTR, continue over short writes. A
 * persistent error (including a 0-return write) reports -1 once and the
 * caller exits; no unbounded retry. */
static long k3_write_all(int fd, const char *buf, unsigned long n)
{
    unsigned long off = 0;

    while (off < n) {
        long w;

        errno = 0;
        w = k3_boundary_write(fd, buf + off, n - off);
        if (w < 0) {
            if (errno == EINTR)
                continue;
            return -1;
        }
        if (w == 0)
            return -1;
        off += (unsigned long)w;
    }
    return (long)n;
}

/* Emit to stdout; a persistent write error latches k3_out_broken. */
static void k3_put(const char *s, unsigned long n)
{
    if (k3_write_all(1, s, n) < 0)
        k3_out_broken = 1;
}

static void k3_put_ulong(unsigned long u)
{
    char buf[24];
    unsigned long i = sizeof buf;

    if (u == 0) {
        k3_put("0", 1);
        return;
    }
    while (u > 0 && i > 0) {
        buf[--i] = (char)('0' + (u % 10));
        u /= 10;
    }
    k3_put(buf + i, (sizeof buf) - i);
}

static int k3_exit_write_fault(void)
{
    static const char msg[] = "[shell] stdout write failed, exiting\n";

    (void)k3_write_all(2, msg, sizeof msg - 1);
    return 1;
}

/* ── input discipline ────────────────────────────────────────────────── */

/* One raw byte from stdin; retry EINTR; map the tty's EINVAL (its
 * whole-line-discard delivery) to K3_IO_TOOLONG. */
static enum k3_io_result k3_read_byte(char *out)
{
    for (;;) {
        long n;

        errno = 0;
        n = k3_boundary_read(0, out, 1);
        if (n == 1)
            return K3_IO_OK;
        if (n == 0)
            return K3_IO_EOF;
        if (n < 0) {
            if (errno == EINTR)
                continue;
            if (errno == EINVAL)
                return K3_IO_TOOLONG;
            return K3_IO_ERROR;
        }
        return K3_IO_ERROR;   /* read(fd, buf, 1) cannot over-report */
    }
}

/* Read one line into buf (cap includes the NUL). A line that does not fit
 * is discarded whole: bytes up to the terminator are consumed and nothing
 * is handed to execution. */
static enum k3_line_result k3_read_line(char *buf, unsigned long cap,
                                        unsigned long *len_out)
{
    unsigned long len = 0;
    int over = 0;

    *len_out = 0;
    for (;;) {
        char ch;

        switch (k3_read_byte(&ch)) {
        case K3_IO_OK:
            if (ch == '\n') {
                if (over)
                    return K3_LINE_TOOLONG;
                buf[len] = '\0';
                *len_out = len;
                return K3_LINE_OK;
            }
            if (!over) {
                if (len + 1 >= cap)
                    over = 1;
                else
                    buf[len++] = ch;
            }
            break;
        case K3_IO_EOF:
            if (over)
                return K3_LINE_TOOLONG;
            if (len == 0)
                return K3_LINE_EOF;
            return K3_LINE_EOF_PARTIAL;
        case K3_IO_TOOLONG:
            return K3_LINE_TOOLONG;
        case K3_IO_ERROR:
        default:
            return K3_LINE_ERROR;
        }
    }
}

/* nanosleep in whole milliseconds; EINTR resumes with the remaining time. */
static int k3_sleep_ms(long ms)
{
    long rem = ms;

    while (rem > 0) {
        errno = 0;
        if (k3_boundary_nanosleep(rem, &rem) == 0)
            return 0;
        if (errno != EINTR)
            return -1;
    }
    return 0;
}

/* ── parse ───────────────────────────────────────────────────────────── */

static int k3_word_is(const char *w, unsigned long wl, const char *word)
{
    return wl == strlen(word) && memcmp(w, word, wl) == 0;
}

/* Advance past spaces and take the next non-space token. */
static int k3_next_arg(const char *line, unsigned long len, unsigned long *i,
                       const char **arg, unsigned long *al)
{
    unsigned long j = *i;
    unsigned long start;

    while (j < len && line[j] == ' ')
        j++;
    if (j == len) {
        *arg = NULL;
        *al = 0;
        return -1;
    }
    start = j;
    while (j < len && line[j] != ' ')
        j++;
    *i = j;
    *arg = line + start;
    *al = j - start;
    return 0;
}

static int k3_rest_blank(const char *line, unsigned long len, unsigned long i)
{
    while (i < len) {
        if (line[i] != ' ')
            return 0;
        i++;
    }
    return 1;
}

/* Decimal digits only (a sign is malformed, so negatives are rejected).
 * Returns 0 ok, -1 malformed, -2 overflow/out of range. */
static int k3_parse_num(const char *s, unsigned long len, long max, long *out)
{
    unsigned long v = 0;
    unsigned long i;

    if (len == 0)
        return -1;
    for (i = 0; i < len; i++) {
        unsigned long d;

        if (s[i] < '0' || s[i] > '9')
            return -1;
        d = (unsigned long)(s[i] - '0');
        if (v > (ULONG_MAX - d) / 10)
            return -2;
        v = v * 10 + d;
    }
    if (v > (unsigned long)max)
        return -2;
    *out = (long)v;
    return 0;
}

/* Split one complete line into a command. Leading spaces are skipped; the
 * echo text keeps its internal spaces verbatim; the numeric commands take
 * exactly one argument token. */
static enum k3_parse_result k3_parse(const char *line, unsigned long len,
                                     struct k3_command *cmd)
{
    unsigned long i = 0;
    const char *w;
    unsigned long wl;

    cmd->kind = K3_CMD_NONE;
    cmd->arg = NULL;
    cmd->arg_len = 0;
    cmd->number = 0;

    while (i < len && line[i] == ' ')
        i++;
    if (i == len)
        return K3_PARSE_EMPTY;
    w = line + i;
    wl = 0;
    while (i < len && line[i] != ' ') {
        i++;
        wl++;
    }

    if (k3_word_is(w, wl, "help")) {
        cmd->kind = K3_CMD_HELP;
        return k3_rest_blank(line, len, i) ? K3_PARSE_OK : K3_PARSE_ARG_ERROR;
    }
    if (k3_word_is(w, wl, "echo")) {
        while (i < len && line[i] == ' ')
            i++;
        cmd->kind = K3_CMD_ECHO;
        cmd->arg = line + i;
        cmd->arg_len = len - i;
        return K3_PARSE_OK;
    }
    if (k3_word_is(w, wl, "sleep")) {
        const char *a;
        unsigned long al;

        cmd->kind = K3_CMD_SLEEP;
        if (k3_next_arg(line, len, &i, &a, &al) != 0 ||
            !k3_rest_blank(line, len, i))
            return K3_PARSE_ARG_ERROR;
        if (k3_parse_num(a, al, 60000, &cmd->number) != 0)
            return K3_PARSE_ARG_ERROR;
        return K3_PARSE_OK;
    }
    if (k3_word_is(w, wl, "exit")) {
        const char *a;
        unsigned long al;

        cmd->kind = K3_CMD_EXIT;
        if (k3_next_arg(line, len, &i, &a, &al) != 0) {
            cmd->number = 0;   /* bare exit is exit 0 */
            return K3_PARSE_OK;
        }
        if (!k3_rest_blank(line, len, i))
            return K3_PARSE_ARG_ERROR;
        if (k3_parse_num(a, al, 255, &cmd->number) != 0)
            return K3_PARSE_ARG_ERROR;
        return K3_PARSE_OK;
    }

    cmd->arg = w;
    cmd->arg_len = wl;
    return K3_PARSE_UNKNOWN;
}

/* ── dispatch ────────────────────────────────────────────────────────── */

static const char k3_msg_banner[] =
    "[shell] k3 minimal interactive shell, 'help' lists commands\n";
static const char k3_msg_prompt[] = "sh> ";
static const char k3_msg_help[] =
    "[shell] commands: help | echo <text> | sleep <ms 0..60000> | exit [code 0..255]\n";
static const char k3_msg_usage_help[] = "[shell] usage: help\n";
static const char k3_msg_usage_sleep[] = "[shell] usage: sleep <ms 0..60000>\n";
static const char k3_msg_usage_exit[] = "[shell] usage: exit [code 0..255]\n";
static const char k3_msg_unknown[] = "[shell] unknown command: ";
static const char k3_msg_exit[] = "[shell] exit code=";
static const char k3_msg_sleep_failed[] = "[shell] sleep failed\n";

static enum k3_dispatch_result k3_dispatch(const struct k3_command *cmd,
                                           enum k3_parse_result pr)
{
    switch (pr) {
    case K3_PARSE_EMPTY:
        return K3_DISPATCH_CONTINUE;
    case K3_PARSE_UNKNOWN:
        k3_put(k3_msg_unknown, sizeof k3_msg_unknown - 1);
        k3_put(cmd->arg, cmd->arg_len);
        k3_put("\n", 1);
        return K3_DISPATCH_CONTINUE;
    case K3_PARSE_ARG_ERROR:
        switch (cmd->kind) {
        case K3_CMD_SLEEP:
            k3_put(k3_msg_usage_sleep, sizeof k3_msg_usage_sleep - 1);
            break;
        case K3_CMD_EXIT:
            k3_put(k3_msg_usage_exit, sizeof k3_msg_usage_exit - 1);
            break;
        case K3_CMD_HELP:
            k3_put(k3_msg_usage_help, sizeof k3_msg_usage_help - 1);
            break;
        default:
            break;
        }
        return K3_DISPATCH_CONTINUE;
    case K3_PARSE_OK:
        break;
    }

    switch (cmd->kind) {
    case K3_CMD_HELP:
        k3_put(k3_msg_help, sizeof k3_msg_help - 1);
        return K3_DISPATCH_CONTINUE;
    case K3_CMD_ECHO:
        k3_put(cmd->arg, cmd->arg_len);
        k3_put("\n", 1);
        return K3_DISPATCH_CONTINUE;
    case K3_CMD_SLEEP:
        if (k3_sleep_ms(cmd->number) != 0)
            k3_put(k3_msg_sleep_failed, sizeof k3_msg_sleep_failed - 1);
        return K3_DISPATCH_CONTINUE;
    case K3_CMD_EXIT:
        k3_put(k3_msg_exit, sizeof k3_msg_exit - 1);
        k3_put_ulong((unsigned long)cmd->number);
        k3_put("\n", 1);
        return K3_DISPATCH_EXIT;
    case K3_CMD_NONE:
    default:
        return K3_DISPATCH_CONTINUE;
    }
}

/* ── shell loop ──────────────────────────────────────────────────────── */

int k3_shell_main(void)
{
    static const char too_long[] = "[shell] line too long, discarded\n";
    static const char eof_msg[] = "[shell] eof\n";
    static const char eof_partial[] = "[shell] eof, partial line discarded\n";
    static const char read_err[] = "[shell] stdin read error, exiting\n";
    char buf[K3_SHELL_CMD_CAP];

    k3_put(k3_msg_banner, sizeof k3_msg_banner - 1);
    if (k3_out_broken)
        return k3_exit_write_fault();
    for (;;) {
        unsigned long len = 0;
        struct k3_command cmd;
        enum k3_line_result lr;

        k3_put(k3_msg_prompt, sizeof k3_msg_prompt - 1);
        if (k3_out_broken)
            return k3_exit_write_fault();
        lr = k3_read_line(buf, sizeof buf, &len);
        switch (lr) {
        case K3_LINE_OK: {
            enum k3_dispatch_result dr =
                k3_dispatch(&cmd, k3_parse(buf, len, &cmd));

            if (k3_out_broken)
                return k3_exit_write_fault();
            if (dr == K3_DISPATCH_EXIT)
                return (int)cmd.number;
            break;
        }
        case K3_LINE_TOOLONG:
            k3_put(too_long, sizeof too_long - 1);
            break;
        case K3_LINE_EOF:
            k3_put(eof_msg, sizeof eof_msg - 1);
            if (k3_out_broken)
                return k3_exit_write_fault();
            return 0;
        case K3_LINE_EOF_PARTIAL:
            k3_put(eof_partial, sizeof eof_partial - 1);
            if (k3_out_broken)
                return k3_exit_write_fault();
            return 0;
        case K3_LINE_ERROR:
        default:
            (void)k3_write_all(2, read_err, sizeof read_err - 1);
            return 1;
        }
        if (k3_out_broken)
            return k3_exit_write_fault();
    }
}

#ifndef K3_SHELL_TESTING
int main(void)
{
    return k3_shell_main();
}
#endif
