/* Host classification contract tests for benchmark boundary inputs.
   No UART/Console dependency — pure logic verification.
   Compile: gcc -std=c99 -Wall -Wextra -o classify_test benchmark_classify_test.c
   Run: ./classify_test */

#include <stdio.h>
#include <stdlib.h>
#include "benchmark_classify.h"

static int failures = 0;
static int tests = 0;

#define TEST(name, expr) do { \
    tests++; \
    if (!(expr)) { \
        failures++; \
        printf("RED: %s FAILED\n", name); \
    } else { \
        printf("GREEN: %s PASSED\n", name); \
    } \
} while(0)

/* ── S11/S41/S42 completion validity ── */
static void test_write_completion(void) {
    printf("\n--- classify_write_completion ---\n");

    /* Happy path: all correct */
    TEST("happy-all-ok",
         classify_write_completion(6400, 6400, 0, 0, 0) == 1);

    /* byte mismatch */
    TEST("byte-mismatch",
         classify_write_completion(6300, 6400, 0, 0, 0) == 0);

    /* incomplete logical writes */
    TEST("incomplete-logical",
         classify_write_completion(6400, 6400, 1, 0, 0) == 0);

    /* drain failure */
    TEST("drain-rc-negative",
         classify_write_completion(6400, 6400, 0, -1, 0) == 0);

    /* drain errors present */
    TEST("drain-errors",
         classify_write_completion(6400, 6400, 0, 0, 2) == 0);

    /* zero bytes */
    TEST("zero-bytes-valid",
         classify_write_completion(0, 0, 0, 0, 0) == 1);

    /* multiple failures */
    TEST("multiple-failures",
         classify_write_completion(100, 200, 1, -1, 1) == 0);
}

/* ── S42 overlap applicability ── */
static void test_overlap_applicable(void) {
    printf("\n--- classify_overlap_applicable ---\n");

    /* Write faster than line time → overlap possible */
    TEST("fast-write-applicable",
         classify_overlap_applicable(10000000LL, 347222222LL) == 1);

    /* Write equals line time → no overlap (synchronous) */
    TEST("equal-time-no-overlap",
         classify_overlap_applicable(347222222LL, 347222222LL) == 0);

    /* Write slower than line time → no overlap (synchronous Console) */
    TEST("slow-write-no-overlap",
         classify_overlap_applicable(500000000LL, 347222222LL) == 0);

    /* Near-zero write (virtual UART) */
    TEST("near-zero-write",
         classify_overlap_applicable(1LL, 347222222LL) == 1);

    /* Zero theoretical time → handles gracefully */
    TEST("zero-theoretical",
         classify_overlap_applicable(100LL, 0LL) == 0);
}

/* ── S43 loaded applicability ── */
static void test_s43_loaded_applicable(void) {
    printf("\n--- classify_s43_loaded_applicable ---\n");

    /* Write fast → overlap window exists */
    TEST("fast-burst-applicable",
         classify_s43_loaded_applicable(10000000LL, 347222222LL) == 1);

    /* Write blocks full line time → not applicable */
    TEST("blocking-not-applicable",
         classify_s43_loaded_applicable(347222222LL, 347222222LL) == 0);

    /* Write slower than line → not applicable */
    TEST("slow-burst-not-applicable",
         classify_s43_loaded_applicable(500000000LL, 347222222LL) == 0);

    /* Negative write duration (timer error) */
    TEST("negative-duration-not-applicable",
         classify_s43_loaded_applicable(-1LL, 347222222LL) == 0);

    /* Zero theoretical time */
    TEST("zero-line-time-not-applicable",
         classify_s43_loaded_applicable(100LL, 0LL) == 0);
}

/* ── overlap efficiency computation ── */
static void test_overlap_efficiency(void) {
    printf("\n--- classify_overlap_efficiency ---\n");

    /* Normal case */
    {
        double eff = classify_overlap_efficiency(300000, 310000);
        TEST("normal-efficiency", eff > 0.9 && eff < 1.0);
    }

    /* Zero overlap (synchronous Console) */
    {
        double eff = classify_overlap_efficiency(0, 310000);
        TEST("zero-overlap", eff == 0.0);
    }

    /* Idle iters zero → prevents division by zero */
    {
        double eff = classify_overlap_efficiency(100, 0);
        TEST("idle-zero-no-divzero", eff == 0.0);
    }

    /* Both zero → no crash */
    {
        double eff = classify_overlap_efficiency(0, 0);
        TEST("both-zero-no-crash", eff == 0.0);
    }

    /* Exact equality */
    {
        double eff = classify_overlap_efficiency(310000, 310000);
        TEST("exact-equality", eff == 1.0);
    }
}

/* ── counter capability state ── */
static void test_counter_state(void) {
    printf("\n--- classify_counter_state ---\n");

    /* ioctl failed → not-available */
    TEST("ioctl-failed",
         classify_counter_state(-1, 6400) == 0);

    /* ioctl ok, bytes present → available */
    TEST("ioctl-ok-with-bytes",
         classify_counter_state(0, 6400) == 1);

    /* ioctl ok, zero bytes → zero-bytes */
    TEST("ioctl-ok-zero-bytes",
         classify_counter_state(0, 0) == 2);

    /* Any negative ioctl → not-available regardless of bytes */
    TEST("ioctl-negative-ignores-bytes",
         classify_counter_state(-5, 100000) == 0);
}

int main(void) {
    test_write_completion();
    test_overlap_applicable();
    test_s43_loaded_applicable();
    test_overlap_efficiency();
    test_counter_state();

    printf("\n=== SUMMARY ===\n");
    printf("%d tests run, %d failures\n", tests, failures);
    if (failures == 0) {
        printf("ALL GREEN.\n");
    } else {
        printf("RED: %d test(s) failed.\n", failures);
    }
    return failures ? 1 : 0;
}
