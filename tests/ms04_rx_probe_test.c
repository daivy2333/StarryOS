#define MS04_RX_PROBE_TESTING
#include "ms04_rx_probe.c"

#include <assert.h>

static struct ms04_snapshot active_snapshot(void)
{
    struct ms04_snapshot snapshot = {0};
    snapshot.rx_lifecycle = 2;
    snapshot.rx_owner = 1;
    return snapshot;
}

static void test_counter_regression_is_rejected(void)
{
    struct ms04_snapshot pre = active_snapshot();
    struct ms04_snapshot post = active_snapshot();
    struct ms04_snapshot delta;
    pre.reaped = 2;
    post.reaped = 1;
    assert(snapshot_delta(&pre, &post, &delta) != 0);
}

static void test_gauges_are_not_subtracted(void)
{
    struct ms04_snapshot pre = active_snapshot();
    struct ms04_snapshot post = active_snapshot();
    struct ms04_snapshot delta;
    pre.rx_lifecycle = 4;
    pre.rx_owner = 0;
    post.last_error_stage = 5;
    post.last_error_code = 7;
    assert(snapshot_delta(&pre, &post, &delta) == 0);
    assert(delta.rx_lifecycle == 0);
    assert(delta.rx_owner == 0);
    assert(delta.last_error_stage == 0);
    assert(delta.last_error_code == 0);
}

static void test_idle_pass_and_busy_failure(void)
{
    struct ms04_snapshot post = active_snapshot();
    struct ms04_snapshot delta = {0};
    delta.task_poll = 1;
    delta.empty_check = 1;
    assert(validate_idle(&post, &delta));
    delta.task_poll = 2;
    assert(!validate_idle(&post, &delta));
}

static void test_nudge_exact_deltas(void)
{
    struct ms04_snapshot post = active_snapshot();
    struct ms04_snapshot delta = {0};
    delta.software_nudge = 1;
    delta.task_poll = 1;
    delta.empty_check = 1;
    assert(validate_nudge(&post, &delta));
    delta.isr_publish = 1;
    assert(!validate_nudge(&post, &delta));
}

static void test_burst_rejects_partial_telemetry_and_receive(void)
{
    struct ms04_snapshot post = active_snapshot();
    struct ms04_snapshot delta = {0};
    delta.isr_publish = 1;
    delta.isr_wake = 1;
    delta.task_poll = 2;
    delta.reaped = 64;
    delta.refilled = 64;
    delta.budget_exhausted = 1;
    delta.self_yield = 1;
    assert(validate_burst(&post, &delta, 96, 96));
    delta.refilled = 63;
    assert(!validate_burst(&post, &delta, 96, 96));
    delta.refilled = 64;
    assert(!validate_burst(&post, &delta, 95, 96));
}

static void test_stable_snapshot_is_bounded_and_requires_equal_progress(void)
{
    struct ms04_snapshot first = active_snapshot();
    struct ms04_snapshot second = active_snapshot();
    assert(snapshot_progress_equal(&first, &second));
    second.task_poll = 1;
    assert(!snapshot_progress_equal(&first, &second));
    assert(!stable_deadline_expired(100, 1099));
    assert(stable_deadline_expired(100, 1100));
    assert(stable_deadline_expired(100, 99));
}

int main(void)
{
    test_counter_regression_is_rejected();
    test_gauges_are_not_subtracted();
    test_idle_pass_and_busy_failure();
    test_nudge_exact_deltas();
    test_burst_rejects_partial_telemetry_and_receive();
    test_stable_snapshot_is_bounded_and_requires_equal_progress();
    puts("ms04 probe decision tests: 6 passed");
    return 0;
}
