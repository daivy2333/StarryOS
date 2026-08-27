#define MS06_STACK_READINESS_PROBE_TESTING
#include "ms06_stack_readiness_probe.c"

#include <assert.h>

/* ── deadline decisions ─────────────────────────────────────────────── */

static void test_deadline_equal_is_expired(void)
{
    assert(ms06_deadline_expired(1000000, 1000000 + 15000ull * 1000, 15000));
}

static void test_deadline_before_is_alive_and_after_expired(void)
{
    assert(!ms06_deadline_expired(1000000, 1000000 + 14999ull * 1000, 15000));
    assert(ms06_deadline_expired(1000000, 1000001 + 15000ull * 1000, 15000));
}

static void test_deadline_remaining_clamps_to_zero(void)
{
    assert(ms06_deadline_remaining_ms(1000000, 1000000 + 5000ull * 1000, 15000) == 10000);
    assert(ms06_deadline_remaining_ms(1000000, 1000000 + 15000ull * 1000, 15000) == 0);
    assert(ms06_deadline_remaining_ms(1000000, 1000000 + 99999ull * 1000, 15000) == 0);
    /* Clock regression must never extend a case: remaining is clamped to 0. */
    assert(ms06_deadline_remaining_ms(2000000, 1000000, 15000) == 0);
    assert(ms06_deadline_expired(2000000, 1000000, 15000));
}

/* ── event contracts ────────────────────────────────────────────────── */

static void test_event_rules_accept_required_bits_only(void)
{
    assert(ms06_events_satisfy(MS06_CASE_TCP_TIMER, MS06_EV_IN | MS06_EV_RDHUP));
    assert(!ms06_events_satisfy(MS06_CASE_TCP_TIMER, MS06_EV_IN));              /* missing RDHUP */
    assert(!ms06_events_satisfy(MS06_CASE_TCP_TIMER,
                                MS06_EV_IN | MS06_EV_RDHUP | MS06_EV_ERR));      /* ERR forbidden */
    assert(ms06_events_satisfy(MS06_CASE_UDP_PROGRESS, MS06_EV_IN));
    assert(!ms06_events_satisfy(MS06_CASE_UDP_PROGRESS, 0));
    assert(ms06_events_satisfy(MS06_CASE_NONBLOCK_CONNECT_ERROR,
                               MS06_EV_OUT | MS06_EV_ERR));
    assert(!ms06_events_satisfy(MS06_CASE_NONBLOCK_CONNECT_ERROR, MS06_EV_OUT)); /* missing ERR */
    assert(!ms06_events_satisfy(MS06_CASE_NONBLOCK_CONNECT_ERROR, MS06_EV_ERR)); /* missing OUT */
}

static void test_quiet_contract_ignores_writable_rejects_activity(void)
{
    /* Normal POLLOUT on an established socket is level-triggered writability,
     * not spurious runner progress: the quiet window must neither arm it nor
     * condemn it. Read/terminal/error readiness is the only condemned class. */
    assert(ms06_events_satisfy(MS06_CASE_QUIET, MS06_EV_OUT));
    assert(ms06_events_satisfy(MS06_CASE_QUIET, 0));
    const uint32_t activity[] = {
        MS06_EV_IN, MS06_EV_ERR, MS06_EV_HUP, MS06_EV_RDHUP
    };
    for (size_t i = 0; i < sizeof(activity) / sizeof(activity[0]); ++i) {
        assert(!ms06_events_satisfy(MS06_CASE_QUIET, activity[i]));
        assert(!ms06_events_satisfy(MS06_CASE_QUIET, activity[i] | MS06_EV_OUT));
    }
}

static void test_quiet_interest_excludes_writable(void)
{
    assert(ms06_quiet_interest() & POLLIN);
    assert(ms06_quiet_interest() & POLLRDHUP);
    assert((ms06_quiet_interest() & POLLOUT) == 0);
}

static void test_udp_bind_spec_rejects_zeroed_and_accepts_loopback(void)
{
    struct sockaddr_in sa;
    memset(&sa, 0, sizeof(sa));
    assert(!ms06_udp_bind_spec_valid(&sa)); /* family 0 never reaches bind() */
    sa.sin_family = AF_INET;
    sa.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    sa.sin_port = 0; /* ephemeral port is a valid bind spec */
    assert(ms06_udp_bind_spec_valid(&sa));
    sa.sin_port = htons(12345); /* fixed port remains a valid spec */
    assert(ms06_udp_bind_spec_valid(&sa));
    sa.sin_addr.s_addr = htonl(INADDR_ANY); /* contract pins loopback endpoints */
    assert(!ms06_udp_bind_spec_valid(&sa));
    assert(!ms06_udp_bind_spec_valid(NULL));
}

static void test_close_error_requires_eof_without_fault(void)
{
    assert(ms06_events_satisfy(MS06_CASE_CLOSE_ERROR, MS06_EV_IN | MS06_EV_RDHUP));
    assert(ms06_events_satisfy(MS06_CASE_CLOSE_ERROR,
                               MS06_EV_IN | MS06_EV_RDHUP | MS06_EV_HUP));
    assert(!ms06_events_satisfy(MS06_CASE_CLOSE_ERROR,
                                MS06_EV_IN | MS06_EV_RDHUP | MS06_EV_ERR));
}

/* ── case verdicts ──────────────────────────────────────────────────── */

static struct ms06_case_result completed_case(int case_id)
{
    struct ms06_case_result r;
    memset(&r, 0, sizeof(r));
    r.case_id = case_id;
    r.status = MS06_ST_COMPLETED;
    r.cleanup_ok = 1;
    r.events = MS06_EV_IN | MS06_EV_RDHUP;
    return r;
}

static void test_verdict_accepts_complete_clean_case(void)
{
    struct ms06_case_result r = completed_case(MS06_CASE_TCP_TIMER);
    r.want_err = 0;
    r.err = 0;
    assert(ms06_case_verdict(&r));
}

static void test_verdict_rejects_every_failure_status(void)
{
    const enum ms06_status statuses[] = {
        MS06_ST_TIMEOUT, MS06_ST_EVENT_MISMATCH, MS06_ST_IO_ERROR, MS06_ST_CLEANUP_FAIL
    };
    for (size_t i = 0; i < sizeof(statuses) / sizeof(statuses[0]); ++i) {
        struct ms06_case_result r = completed_case(MS06_CASE_TCP_TIMER);
        r.status = statuses[i];
        assert(!ms06_case_verdict(&r));
    }
}

static void test_partial_terminal_state_is_rejected(void)
{
    struct ms06_case_result r = completed_case(MS06_CASE_TCP_TIMER);
    r.events = MS06_EV_IN; /* RDHUP missing: terminal state only partially observed */
    assert(!ms06_case_verdict(&r));
}

static void test_wrong_error_category_is_rejected(void)
{
    struct ms06_case_result r = completed_case(MS06_CASE_NONBLOCK_CONNECT_ERROR);
    r.events = MS06_EV_OUT | MS06_EV_ERR;
    r.want_err = ECONNREFUSED;
    r.err = ECONNREFUSED;
    assert(ms06_case_verdict(&r));
    r.err = EAGAIN; /* category drift between observations */
    assert(!ms06_case_verdict(&r));
}

static void test_cleanup_failure_blocks_pass_even_when_completed(void)
{
    struct ms06_case_result r = completed_case(MS06_CASE_UDP_PROGRESS);
    r.events = MS06_EV_IN;
    r.cleanup_ok = 0;
    assert(!ms06_case_verdict(&r));
}

static void test_normal_close_misclassified_as_fault_is_rejected(void)
{
    struct ms06_case_result r = completed_case(MS06_CASE_CLOSE_ERROR);
    r.events = MS06_EV_ERR; /* graceful close surfaced as device fault */
    r.err = EIO;
    r.want_err = 0;
    assert(!ms06_case_verdict(&r));
}

/* ── waiter records (Task 4.3) ──────────────────────────────────────── */

static struct ms06_waiter_record finished_waiter(long pid)
{
    struct ms06_waiter_record w;
    memset(&w, 0, sizeof(w));
    w.pid = pid;
    w.phases = MS06_PHASE_REGISTERED;
    w.completions = 1;
    return w;
}

static struct ms06_waiter_record replaced_waiter(long pid)
{
    struct ms06_waiter_record w = finished_waiter(pid);
    w.phases |= MS06_PHASE_WOKEN | MS06_PHASE_RECHECK_NG | MS06_PHASE_REREGISTERED;
    w.replacements = 1;
    return w;
}

static void test_record_rejects_zero_or_double_completion(void)
{
    struct ms06_waiter_record w = finished_waiter(100);
    assert(ms06_waiter_record_valid(&w));
    w.completions = 0;
    assert(ms06_waiter_record_valid(&w)); /* incomplete is record-valid; set rejects */
    w.completions = 2;
    assert(!ms06_waiter_record_valid(&w)); /* exactly-once violated */
}

static void test_record_requires_registration_before_completion(void)
{
    struct ms06_waiter_record w = finished_waiter(101);
    w.phases = 0; /* claims completion without ever registering */
    assert(!ms06_waiter_record_valid(&w));
}

static void test_record_rejects_pidless_identity(void)
{
    struct ms06_waiter_record w = finished_waiter(0);
    assert(!ms06_waiter_record_valid(&w));
    w.pid = -5;
    assert(!ms06_waiter_record_valid(&w));
}

static void test_record_replacement_requires_full_wake_recheck_reregister_chain(void)
{
    /* Replacement observed but no re-register before completing: impossible
     * path under wake-on-replacement semantics. */
    struct ms06_waiter_record w = replaced_waiter(102);
    w.phases &= ~(uint32_t)MS06_PHASE_REREGISTERED;
    assert(!ms06_waiter_record_valid(&w));

    /* Wake + recheck-not-ready recorded without any replacement counter. */
    struct ms06_waiter_record v = finished_waiter(103);
    v.phases |= MS06_PHASE_WOKEN | MS06_PHASE_RECHECK_NG;
    assert(!ms06_waiter_record_valid(&v));

    /* Replacement counter without the matching recheck bookkeeping. */
    struct ms06_waiter_record u = finished_waiter(104);
    u.replacements = 1;
    assert(!ms06_waiter_record_valid(&u));

    /* Full chain is valid. */
    struct ms06_waiter_record full_chain = replaced_waiter(105);
    assert(ms06_waiter_record_valid(&full_chain));
}

static void fill_set(struct ms06_waiter_record *records, uint32_t n, int replaced)
{
    for (uint32_t i = 0; i < n; ++i) {
        records[i] = replaced ? replaced_waiter((long)(1000 + i))
                              : finished_waiter((long)(1000 + i));
    }
}

static void test_set_accepts_exact_64_all_complete(void)
{
    struct ms06_waiter_record records[64];
    struct ms06_waiter_set set = { .capacity = 64 };
    fill_set(records, 64, 0);
    assert(ms06_waiter_set_accepts(&set, records, 64));
}

static void test_set_accepts_exact_65_all_complete(void)
{
    /* Task 4.3 replan: guest only proves 65 distinct exactly-once
     * completions; replacement/re-register evidence is host/source-owned and
     * no longer required from the guest record set. */
    struct ms06_waiter_record records[65];
    struct ms06_waiter_set set = { .capacity = 65 };
    fill_set(records, 65, 0);
    assert(ms06_waiter_set_accepts(&set, records, 65));
}

static void test_set_rejects_partial_completion_63_of_64(void)
{
    struct ms06_waiter_record records[64];
    struct ms06_waiter_set set = { .capacity = 64 };
    fill_set(records, 64, 0);
    assert(!ms06_waiter_set_accepts(&set, records, 63)); /* one waiter missing */

    records[63] = finished_waiter(1063);
    records[63].completions = 0; /* present but never completed */
    assert(!ms06_waiter_set_accepts(&set, records, 64));
}

static void test_set_rejects_partial_completion_64_of_65(void)
{
    struct ms06_waiter_record records[64]; /* only 64 of the required 65 exist */
    struct ms06_waiter_set set = { .capacity = 65 };
    fill_set(records, 64, 0);
    assert(!ms06_waiter_set_accepts(&set, records, 64));
}

static void test_set_rejects_duplicate_identity(void)
{
    struct ms06_waiter_record records[65];
    struct ms06_waiter_set set = { .capacity = 65 };
    fill_set(records, 65, 0);
    records[7].pid = records[3].pid; /* two records collapse into one identity */
    assert(!ms06_waiter_set_accepts(&set, records, 65));
}

/* ── exact 64/65 release decisions (Task 4.3 replan) ────────────────── */

static void test_exact_mode_requires_epoll(void)
{
    /* Exact waiter cases must publish their arms through a synchronous
     * epoll registration; a poll/select release gate is a contract error. */
    assert(ms06_exact_mode_ok(MS06_WAIT_EPOLL));
    assert(!ms06_exact_mode_ok(MS06_WAIT_POLL));
    assert(!ms06_exact_mode_ok(MS06_WAIT_SELECT));
}

static void test_exact_arms_complete_matrix(void)
{
    /* Release only after every distinct waiter's synchronous arm is in:
     * 63/64 and 64/65 are partial, 64/64 and 65/65 are exact. */
    assert(!ms06_exact_arms_complete(63, 64));
    assert(ms06_exact_arms_complete(64, 64));
    assert(!ms06_exact_arms_complete(64, 65));
    assert(ms06_exact_arms_complete(65, 65));
    assert(!ms06_exact_arms_complete(0, 64));
    assert(!ms06_exact_arms_complete(66, 65)); /* over-arming */
}

static void test_exact_trigger_units_matrix(void)
{
    /* The stimulus must equal the waiter count: N−1 cannot witness all
     * distinct completions, N+1 exceeds the published units. */
    assert(!ms06_trigger_units_valid(63, 64));
    assert(ms06_trigger_units_valid(64, 64));
    assert(!ms06_trigger_units_valid(65, 64));
    assert(ms06_trigger_units_valid(65, 65));
    assert(!ms06_trigger_units_valid(0, 64));
}

/* ── Task 7.3 witness repairs ───────────────────────────────────────── */

static void test_listener_reply_matches_byte_width(void)
{
    /* The wire carries one byte: a valid reply must match under byte
     * semantics, never the 32-bit integer promotion of `~ident` that made
     * every correct reply a runtime false negative. */
    for (unsigned ident = 1; ident <= 4; ++ident) {
        unsigned char reply = (unsigned char)~ident;
        assert(ms06_listener_reply_matches(ident, reply));
        assert(!ms06_listener_reply_matches(ident, (unsigned char)(reply ^ 0x5a)));
    }
    /* A neighbouring identity's reply must not be accepted. */
    assert(!ms06_listener_reply_matches(1u, (unsigned char)~2u));
}

static void test_peer_fin_eof_valid_contract(void)
{
    /* Graceful peer FIN: IN|RDHUP readiness without a device fault and two
     * stable zero-length reads. */
    assert(ms06_peer_fin_eof_valid(MS06_EV_IN | MS06_EV_RDHUP, 0, 0));
    /* A normal close surfaced as a device fault is never a valid witness. */
    assert(!ms06_peer_fin_eof_valid(MS06_EV_IN | MS06_EV_RDHUP | MS06_EV_ERR, 0, 0));
    /* Readiness without the EOF family is not a peer-FIN observation. */
    assert(!ms06_peer_fin_eof_valid(MS06_EV_IN, 0, 0));
    /* Unstable EOF (nonzero or failed read) is not a graceful close. */
    assert(!ms06_peer_fin_eof_valid(MS06_EV_IN | MS06_EV_RDHUP, 0, 1));
    assert(!ms06_peer_fin_eof_valid(MS06_EV_IN | MS06_EV_RDHUP, -1, 0));
}

int main(int argc, char **argv)
{
    if (argc == 2 && strcmp(argv[1], "--print-cases") == 0) {
        for (int i = 0; i < MS06_CASE_COUNT; ++i) {
            printf("%s\n", ms06_case_name(i));
        }
        return 0;
    }
    test_deadline_equal_is_expired();
    test_deadline_before_is_alive_and_after_expired();
    test_deadline_remaining_clamps_to_zero();
    test_event_rules_accept_required_bits_only();
    test_quiet_contract_ignores_writable_rejects_activity();
    test_quiet_interest_excludes_writable();
    test_udp_bind_spec_rejects_zeroed_and_accepts_loopback();
    test_close_error_requires_eof_without_fault();
    test_verdict_accepts_complete_clean_case();
    test_verdict_rejects_every_failure_status();
    test_partial_terminal_state_is_rejected();
    test_wrong_error_category_is_rejected();
    test_cleanup_failure_blocks_pass_even_when_completed();
    test_normal_close_misclassified_as_fault_is_rejected();
    test_record_rejects_zero_or_double_completion();
    test_record_requires_registration_before_completion();
    test_record_rejects_pidless_identity();
    test_record_replacement_requires_full_wake_recheck_reregister_chain();
    test_set_accepts_exact_64_all_complete();
    test_set_accepts_exact_65_all_complete();
    test_set_rejects_partial_completion_63_of_64();
    test_set_rejects_partial_completion_64_of_65();
    test_set_rejects_duplicate_identity();
    test_exact_mode_requires_epoll();
    test_exact_arms_complete_matrix();
    test_exact_trigger_units_matrix();
    test_listener_reply_matches_byte_width();
    test_peer_fin_eof_valid_contract();
    puts("ms06 probe decision tests: 28 passed");
    return 0;
}
