/*
 * k3_shell_test — MS17 Iteration 003 (task T7) native tests for the
 * minimal interactive shell. Includes the production kernel/resources/
 * k3-shell.c with K3_SHELL_TESTING defined and replaces only the three
 * system boundaries (read/write/nanosleep) with a scripted fake; every
 * parse/dispatch/I-O-discipline decision under test is the production code.
 *
 * Build & run (same pattern as Makefile host-test):
 *   cc -std=c11 -Wall -Wextra -Werror tests/k3_shell_test.c -o /tmp/ms17-k3-shell-test
 *   /tmp/ms17-k3-shell-test
 */

#define K3_SHELL_TESTING
#include "../kernel/resources/k3-shell.c"

#include <assert.h>
#include <stdio.h>
#include <string.h>

/* ── scripted stdin boundary ─────────────────────────────────────────── */

static const char *t_in;
static unsigned long t_in_len;
static int t_rd_eintr_left;   /* one-shot EINTR returns before real data */
static long t_rd_einval_after = -1;   /* deliver N bytes, then one EINVAL */
static long t_rd_bytes;
static int t_rd_fail;         /* persistent EIO */

/* Boundary fakes: external linkage to match the K3_SHELL_TESTING
 * declarations in the production source. */
long k3_boundary_read(int fd, char *buf, unsigned long n)
{
    (void)fd;
    assert(n == 1);
    if (t_rd_fail) {
        errno = EIO;
        return -1;
    }
    if (t_rd_eintr_left > 0) {
        t_rd_eintr_left--;
        errno = EINTR;
        return -1;
    }
    if (t_rd_einval_after >= 0 && t_rd_bytes >= t_rd_einval_after) {
        t_rd_einval_after = -1;
        errno = EINVAL;
        return -1;
    }
    if (t_in_len == 0) {
        errno = 0;
        return 0;
    }
    buf[0] = *t_in++;
    t_in_len--;
    t_rd_bytes++;
    return 1;
}

/* ── captured stdout/stderr boundary ─────────────────────────────────── */

static char t_out[16384];
static size_t t_out_len;
static char t_err[1024];
static size_t t_err_len;

/* write fault injection */
static int t_wr_fail_fd = -1;    /* persistent failure on this fd */
static int t_wr_eintr_fd = -1;   /* one EINTR, then proceed */
static int t_wr_eintr_pending;
static int t_wr_zero_fd = -1;    /* one 0-return (persistent per contract) */
static int t_wr_zero_pending;
static int t_wr_short_fd = -1;   /* every write to this fd emits only half */

long k3_boundary_write(int fd, const char *buf, unsigned long n)
{
    char *dst;
    size_t *dlen;
    size_t cap;

    if (fd == t_wr_fail_fd) {
        errno = EIO;
        return -1;
    }
    if (fd == t_wr_eintr_fd && t_wr_eintr_pending) {
        t_wr_eintr_pending = 0;
        errno = EINTR;
        return -1;
    }
    if (fd == t_wr_zero_fd && t_wr_zero_pending) {
        t_wr_zero_pending = 0;
        errno = 0;
        return 0;
    }
    if (fd == 1) {
        dst = t_out;
        dlen = &t_out_len;
        cap = sizeof t_out;
    } else if (fd == 2) {
        dst = t_err;
        dlen = &t_err_len;
        cap = sizeof t_err;
    } else {
        errno = EBADF;
        return -1;
    }
    if (fd == t_wr_short_fd && n >= 2) {
        unsigned long half = n / 2;

        memcpy(dst + *dlen, buf, half);
        dst[*dlen + half] = '\0';
        *dlen += half;
        return (long)half;
    }
    assert(*dlen + n < cap);
    memcpy(dst + *dlen, buf, n);
    *dlen += n;
    dst[*dlen] = '\0';
    return (long)n;
}

/* ── recorded sleep boundary ─────────────────────────────────────────── */

static long t_sleep_ms[16];
static int t_sleep_count;
static int t_sleep_eintr_idx = -1;
static long t_sleep_eintr_rem;
static int t_sleep_fail_idx = -1;   /* non-EINTR failure (EINVAL) */

int k3_boundary_nanosleep(long ms, long *rem_ms)
{
    int idx = t_sleep_count;

    assert(idx < 16);
    t_sleep_ms[idx] = ms;
    t_sleep_count++;
    if (idx == t_sleep_eintr_idx) {
        errno = EINTR;
        *rem_ms = t_sleep_eintr_rem;
        return -1;
    }
    if (idx == t_sleep_fail_idx) {
        errno = EINVAL;
        *rem_ms = ms;
        return -1;
    }
    *rem_ms = 0;
    return 0;
}

/* ── transcript helpers ──────────────────────────────────────────────── */

static void t_reset(const char *input)
{
    t_in = input;
    t_in_len = input ? strlen(input) : 0;
    t_rd_eintr_left = 0;
    t_rd_einval_after = -1;
    t_rd_bytes = 0;
    t_rd_fail = 0;
    t_out_len = 0;
    t_err_len = 0;
    t_out[0] = '\0';
    t_err[0] = '\0';
    t_wr_fail_fd = -1;
    t_wr_eintr_fd = -1;
    t_wr_eintr_pending = 0;
    t_wr_zero_fd = -1;
    t_wr_zero_pending = 0;
    t_wr_short_fd = -1;
    t_sleep_count = 0;
    t_sleep_eintr_idx = -1;
    t_sleep_eintr_rem = 0;
    t_sleep_fail_idx = -1;
}

static int t_contains(const char *s)
{
    return strstr(t_out, s) != NULL;
}

static int t_err_contains(const char *s)
{
    return strstr(t_err, s) != NULL;
}

static int t_count(const char *s)
{
    int n = 0;
    const char *p = t_out;
    size_t sl = strlen(s);

    while ((p = strstr(p, s)) != NULL) {
        n++;
        p += sl;
    }
    return n;
}

/* "echo " + n times 'x' + "\n" into inbuf; text line is 5+n chars long. */
static void t_build_echo_line(char *inbuf, size_t cap, unsigned int n)
{
    unsigned int i;

    assert(cap > 5 + n + 1 + strlen("exit 0\n") + 1);
    memcpy(inbuf, "echo ", 5);
    for (i = 0; i < n; i++)
        inbuf[5 + i] = 'x';
    inbuf[5 + n] = '\n';
    memcpy(inbuf + 5 + n + 1, "exit 0\n", strlen("exit 0\n") + 1);
}

/* needle of n 'x' + "\n" (the exact echo result of the fitting line) */
static void t_build_x_line(char *buf, size_t cap, unsigned int n)
{
    unsigned int i;

    assert(cap > n + 2);
    for (i = 0; i < n; i++)
        buf[i] = 'x';
    buf[n] = '\n';
    buf[n + 1] = '\0';
}

/* ── tests ───────────────────────────────────────────────────────────── */

static void test_help_and_echo_repeat(void)
{
    t_reset("help\necho one two\nhelp\necho three\nexit 0\n");
    assert(k3_shell_main() == 0);
    assert(t_contains(
        "[shell] commands: help | echo <text> | sleep <ms 0..60000> | exit [code 0..255]\n"));
    assert(t_contains("sh> one two\n"));
    assert(t_contains("sh> three\n"));
    assert(t_count("[shell] commands:") == 2);
    assert(t_contains("[shell] exit code=0\n"));
    assert(t_count("sh> ") == 5);
}

static void test_echo_preserves_internal_spaces(void)
{
    t_reset("echo a  b   c\nexit\n");
    assert(k3_shell_main() == 0);
    /* separator spaces after the word are skipped, internal spaces kept */
    assert(t_contains("sh> a  b   c\n"));
}

static void test_empty_and_blank_lines_return_to_prompt(void)
{
    t_reset("\n\n   \nexit 7\n");
    assert(k3_shell_main() == 7);
    assert(t_contains("[shell] exit code=7\n"));
    assert(t_count("sh> ") == 4);
    assert(!t_contains("unknown"));
    assert(!t_contains("usage"));
}

static void test_unknown_command_reports_and_continues(void)
{
    t_reset("frobnicate now\nexit 0\n");
    assert(k3_shell_main() == 0);
    assert(t_contains("[shell] unknown command: frobnicate\n"));
    assert(t_contains("[shell] exit code=0\n"));
}

static void test_help_rejects_extra_args(void)
{
    t_reset("help me\nexit\n");
    assert(k3_shell_main() == 0);
    assert(t_contains("[shell] usage: help\n"));
}

static void test_sleep_arg_validation(void)
{
    t_reset("sleep\nsleep abc\nsleep -5\nsleep 60001\n"
            "sleep 99999999999999999999999\nsleep 100 extra\nexit 3\n");
    assert(k3_shell_main() == 3);
    assert(t_count("[shell] usage: sleep <ms 0..60000>\n") == 6);
    assert(t_sleep_count == 0);

    t_reset("sleep 250\nsleep 0\nexit 0\n");
    assert(k3_shell_main() == 0);
    assert(t_sleep_count == 1);   /* sleep 0 returns without a syscall */
    assert(t_sleep_ms[0] == 250);
}

static void test_sleep_eintr_resumes_with_remaining(void)
{
    t_reset("sleep 1000\nexit 0\n");
    t_sleep_eintr_idx = 0;
    t_sleep_eintr_rem = 400;
    assert(k3_shell_main() == 0);
    assert(t_sleep_count == 2);
    assert(t_sleep_ms[0] == 1000);
    assert(t_sleep_ms[1] == 400);
}

static void test_sleep_persistent_failure_reports_and_continues(void)
{
    t_reset("sleep 50\nexit 0\n");
    t_sleep_fail_idx = 0;
    assert(k3_shell_main() == 0);
    assert(t_contains("[shell] sleep failed\n"));
    assert(t_sleep_count == 1);
}

static void test_exit_forms(void)
{
    t_reset("exit\n");
    assert(k3_shell_main() == 0);
    t_reset("exit 42\n");
    assert(k3_shell_main() == 42);
    t_reset("exit 256\nexit 9\n");
    assert(k3_shell_main() == 9);
    assert(t_count("[shell] usage: exit [code 0..255]\n") == 1);
    t_reset("exit -1\nexit 2 3\nexit\n");
    assert(k3_shell_main() == 0);
    assert(t_count("[shell] usage: exit [code 0..255]\n") == 2);
    assert(t_contains("[shell] exit code=0\n"));
}

static void test_line_of_255_chars_executes_whole(void)
{
    char in[300];
    char needle[300];

    t_build_echo_line(in, sizeof in, 250);   /* line is exactly 255 chars */
    t_build_x_line(needle, sizeof needle, 250);
    t_reset(in);
    assert(k3_shell_main() == 0);
    assert(!t_contains("line too long"));
    assert(t_contains(needle));
}

static void test_line_over_buffer_discarded_without_execution(void)
{
    char in[320];
    char needle[300];

    t_build_echo_line(in, sizeof in, 251);   /* line is 256 chars: does not fit */
    t_build_x_line(needle, sizeof needle, 250);
    t_reset(in);
    assert(k3_shell_main() == 0);
    assert(t_contains("[shell] line too long, discarded\n"));
    /* the truncated prefix must NOT have been executed */
    assert(!t_contains(needle));
    /* the terminator was consumed; the next command still runs */
    assert(t_contains("[shell] exit code=0\n"));
}

static void test_tty_inval_discards_whole_line(void)
{
    /* The tty delivers 3 bytes of a >1024B line, then discards the whole
     * line with a single EINVAL: the partial prefix must not execute and
     * the shell stays usable for the following lines. */
    t_reset("abcecho hi\nexit 0\n");
    t_rd_einval_after = 3;
    assert(k3_shell_main() == 0);
    assert(t_contains("[shell] line too long, discarded\n"));
    assert(!t_contains("sh> abc\n"));
    assert(t_contains("sh> hi\n"));
    assert(t_contains("[shell] exit code=0\n"));

    /* EINVAL before any byte: same one-shot feedback, then continue. */
    t_reset("echo hi\nexit 0\n");
    t_rd_einval_after = 0;
    assert(k3_shell_main() == 0);
    assert(t_contains("[shell] line too long, discarded\n"));
    assert(t_contains("sh> hi\n"));
    assert(t_contains("[shell] exit code=0\n"));
}

static void test_eof_immediate(void)
{
    t_reset("");
    assert(k3_shell_main() == 0);
    assert(t_contains("[shell] eof\n"));
}

static void test_eof_after_partial_line_discards_it(void)
{
    t_reset("xyz");
    assert(k3_shell_main() == 0);
    assert(t_contains("[shell] eof, partial line discarded\n"));
    assert(!t_contains("unknown command"));
}

static void test_read_eintr_retries(void)
{
    t_reset("exit 0\n");
    t_rd_eintr_left = 1;
    assert(k3_shell_main() == 0);
    assert(t_contains("[shell] exit code=0\n"));
}

static void test_read_persistent_error_exits(void)
{
    t_reset("exit\n");
    t_rd_fail = 1;
    assert(k3_shell_main() == 1);
    assert(t_err_contains("[shell] stdin read error, exiting\n"));
}

static void test_short_writes_still_complete_output(void)
{
    t_reset("echo hello world\nexit 0\n");
    t_wr_short_fd = 1;   /* every stdout write emits only half */
    assert(k3_shell_main() == 0);
    assert(t_contains(
        "[shell] k3 minimal interactive shell, 'help' lists commands\n"));
    assert(t_contains("sh> hello world\n"));
    assert(t_contains("[shell] exit code=0\n"));
}

static void test_write_eintr_retries(void)
{
    t_reset("echo x\nexit 0\n");
    t_wr_eintr_fd = 1;
    t_wr_eintr_pending = 1;
    assert(k3_shell_main() == 0);
    assert(t_contains("sh> x\n"));
    assert(t_contains("[shell] exit code=0\n"));
}

static void test_write_zero_return_is_persistent(void)
{
    t_reset("echo x\nexit\n");
    t_wr_zero_fd = 1;
    t_wr_zero_pending = 1;
    assert(k3_shell_main() == 1);
    assert(t_err_contains("[shell] stdout write failed, exiting\n"));
}

static void test_write_persistent_error_exits(void)
{
    t_reset("echo x\nexit\n");
    t_wr_fail_fd = 1;
    assert(k3_shell_main() == 1);
    assert(t_err_contains("[shell] stdout write failed, exiting\n"));
}

int main(void)
{
    test_help_and_echo_repeat();
    test_echo_preserves_internal_spaces();
    test_empty_and_blank_lines_return_to_prompt();
    test_unknown_command_reports_and_continues();
    test_help_rejects_extra_args();
    test_sleep_arg_validation();
    test_sleep_eintr_resumes_with_remaining();
    test_sleep_persistent_failure_reports_and_continues();
    test_exit_forms();
    test_line_of_255_chars_executes_whole();
    test_line_over_buffer_discarded_without_execution();
    test_tty_inval_discards_whole_line();
    test_eof_immediate();
    test_eof_after_partial_line_discards_it();
    test_read_eintr_retries();
    test_read_persistent_error_exits();
    test_short_writes_still_complete_output();
    test_write_eintr_retries();
    test_write_zero_return_is_persistent();
    test_write_persistent_error_exits();
    puts("k3 shell tests: 20 passed");
    return 0;
}
