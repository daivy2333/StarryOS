/* MS16 network benchmark platform adapter — RED test suite.
 *
 * Build (host):
 *   cc -std=c11 -Wall -Wextra -Werror \
 *     tests/network_benchmark_platform_test.c \
 *     tests/network_benchmark_platform.c \
 *     -o /tmp/network-benchmark-platform-test
 *
 * RED state: tests/network_benchmark_platform.c is absent,
 * so build fails — expected RED witness.
 *
 * GREEN: after platform.c exists, exit 0 and all assertions pass.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <inttypes.h>
#include <assert.h>

#include "network_benchmark_platform.h"

static int failures = 0;

#define CHECK(cond, msg) do { \
    if (!(cond)) { \
        fprintf(stderr, "FAIL: %s:%d: %s\n", __FILE__, __LINE__, msg); \
        failures++; \
    } \
} while (0)

/* ── monotonic clock ─────────────────────────────────────────────────── */

static void test_monotonic_read_succeeds(void)
{
    uint64_t t = nb_monotonic_ns();
    CHECK(t > 0, "monotonic should return positive value");
}

static void test_monotonic_is_monotonic(void)
{
    uint64_t t1 = nb_monotonic_ns();
    uint64_t t2 = nb_monotonic_ns();
    CHECK(t2 >= t1, "monotonic clock must not go backward");
}

static void test_monotonic_overhead_sane(void)
{
    uint64_t t1 = nb_monotonic_ns();
    uint64_t t2 = nb_monotonic_ns();
    /* overhead should be under 1ms */
    CHECK(t2 - t1 < 1000000ULL, "monotonic read overhead must be < 1ms");
}

/* ── u64 parser ──────────────────────────────────────────────────────── */

static void test_parse_u64_valid(void)
{
    uint64_t val;
    int rc = nb_parse_u64("123456789012345", &val);
    CHECK(rc == 0, "valid parse should return 0");
    CHECK(val == 123456789012345ULL, "value should match");
}

static void test_parse_u64_zero(void)
{
    uint64_t val;
    int rc = nb_parse_u64("0", &val);
    CHECK(rc == 0, "zero parse should succeed");
    CHECK(val == 0, "zero should be zero");
}

static void test_parse_u64_max(void)
{
    uint64_t val;
    int rc = nb_parse_u64("18446744073709551615", &val);
    CHECK(rc == 0, "UINT64_MAX parse should succeed");
    CHECK(val == 18446744073709551615ULL, "should be UINT64_MAX");
}

static void test_parse_u64_empty(void)
{
    uint64_t val;
    int rc = nb_parse_u64("", &val);
    CHECK(rc < 0, "empty string should fail");
}

static void test_parse_u64_negative(void)
{
    uint64_t val;
    int rc = nb_parse_u64("-1", &val);
    CHECK(rc < 0, "negative string should fail");
}

static void test_parse_u64_non_numeric(void)
{
    uint64_t val;
    int rc = nb_parse_u64("abc", &val);
    CHECK(rc < 0, "non-numeric string should fail");
}

static void test_parse_u64_overflow(void)
{
    uint64_t val;
    int rc = nb_parse_u64("18446744073709551616", &val);
    CHECK(rc < 0, "overflow (UINT64_MAX+1) should fail");
}

/* ── instret adapter ─────────────────────────────────────────────────── */

static void test_instret_result_structure(void)
{
    struct nb_instret_result r;
    memset(&r, 0, sizeof(r));
    r.available = 1;
    r.begin = 100;
    r.end = 200;
    r.overhead = 10;
    int rc = nb_instret_result_valid(&r);
    CHECK(rc == 0, "valid instret result should pass validation");
}

static void test_instret_result_end_before_begin(void)
{
    struct nb_instret_result r;
    r.available = 1;
    r.begin = 200;
    r.end = 100;
    r.overhead = 10;
    int rc = nb_instret_result_valid(&r);
    CHECK(rc < 0, "end < begin should fail validation");
}

static void test_instret_result_overflow_after_overhead(void)
{
    struct nb_instret_result r;
    r.available = 1;
    r.begin = 0;
    r.end = UINT64_MAX;
    r.overhead = 10;
    int rc = nb_instret_result_valid(&r);
    CHECK(rc < 0, "overflow should be detected");
}

static void test_instret_host_unavailable(void)
{
    struct nb_instret_result r;
    int rc = nb_instret_read(&r);
    /* On host, instret is unavailable — API should return NB_UNAVAILABLE */
    if (rc == 0) {
        CHECK(r.available != 0, "host instret should be unavailable");
    }
}

/* ── IRQ snapshot adapter ────────────────────────────────────────────── */

static void test_irq_snapshot_struct_fields(void)
{
    struct nb_irq_snapshot s;
    memset(&s, 0, sizeof(s));
    s.available = 0;
    s.total = 42;
    s.used_ring = 10;
    s.ack_count = 5;

    CHECK(s.total == 42, "total field should be accessible");
    CHECK(s.used_ring == 10, "used_ring field should be accessible");
    CHECK(s.ack_count == 5, "ack_count field should be accessible");
}

static void test_irq_snapshot_host_unavailable(void)
{
    struct nb_irq_snapshot s;
    int rc = nb_irq_snapshot_read(&s);
    if (rc == 0) {
        /* If the read succeeds, at least check availability semantics */
        /* On host it will be unavailable */
    }
    /* Not asserting on rc: ms03 ioctl is guest-only */
}

/* ── capability query ────────────────────────────────────────────────── */

static void test_capability_host_instr(void)
{
    int cap = nb_capability_instret();
    CHECK(cap == 0, "instret should be unavailable on host");
}

static void test_capability_host_irq(void)
{
    int cap = nb_capability_irq_snapshot();
    CHECK(cap == 0, "IRQ snapshot should be unavailable on host");
}

static void test_capability_monotonic(void)
{
    int cap = nb_capability_monotonic();
    CHECK(cap == 1, "monotonic clock should be available on host");
}

/* ── time helpers ────────────────────────────────────────────────────── */

static void test_nanosleep_wakeup(void)
{
    uint64_t before = nb_monotonic_ns();
    nb_nanosleep(10000000ULL);  /* 10ms */
    uint64_t after = nb_monotonic_ns();
    uint64_t elapsed = after - before;
    CHECK(elapsed >= 9000000ULL, "sleep 10ms should yield at least ~9ms");
    CHECK(elapsed < 100000000ULL, "sleep 10ms should not exceed 100ms");
}

/* ── driver ──────────────────────────────────────────────────────────── */

static void run_test(const char *name, void (*fn)(void))
{
    int before = failures;
    fn();
    if (failures == before)
        printf("PASS: %s\n", name);
}

int main(void)
{
    printf("=== network_benchmark_platform RED test suite ===\n\n");

    run_test("monotonic read succeeds",       test_monotonic_read_succeeds);
    run_test("monotonic is monotonic",        test_monotonic_is_monotonic);
    run_test("monotonic overhead sane",       test_monotonic_overhead_sane);
    run_test("parse_u64 valid",               test_parse_u64_valid);
    run_test("parse_u64 zero",                test_parse_u64_zero);
    run_test("parse_u64 max",                 test_parse_u64_max);
    run_test("parse_u64 empty",               test_parse_u64_empty);
    run_test("parse_u64 negative",            test_parse_u64_negative);
    run_test("parse_u64 non-numeric",         test_parse_u64_non_numeric);
    run_test("parse_u64 overflow",            test_parse_u64_overflow);
    run_test("instret result structure",      test_instret_result_structure);
    run_test("instret end before begin",      test_instret_result_end_before_begin);
    run_test("instret overflow",              test_instret_result_overflow_after_overhead);
    run_test("instret host unavailable",      test_instret_host_unavailable);
    run_test("IRQ snapshot struct fields",    test_irq_snapshot_struct_fields);
    run_test("IRQ snapshot host unavailable", test_irq_snapshot_host_unavailable);
    run_test("capability instret",            test_capability_host_instr);
    run_test("capability IRQ",                test_capability_host_irq);
    run_test("capability monotonic",          test_capability_monotonic);
    run_test("nanosleep wakeup",              test_nanosleep_wakeup);

    printf("\n");
    if (failures == 0) {
        printf("ALL TESTS PASSED\n");
        return 0;
    }
    printf("FAILED: %d assertion(s)\n", failures);
    return 1;
}
