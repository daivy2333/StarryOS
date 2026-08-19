#define MS05_DATA_PLANE_PROBE_TESTING
#include "probe_red.c"

/* MS05 data-plane decision harness — mutates V3 inputs without a guest and
 * proves the decision core rejects: missing PRE, reordered phases, fake Full,
 * counter regression, non-closed ledgers, equal/after-deadline completion,
 * duplicate terminal markers and exit/marker inconsistency.
 */

#include <assert.h>

static struct ms05_snapshot active_snapshot(void)
{
    struct ms05_snapshot s = {0};
    s.rx_lifecycle = 2; /* Active */
    s.rx_owner = 1;     /* async-owned */
    s.tx_buffer_available = MS05_QS;
    s.tx_buffer_inflight = 0;
    s.tx_descriptor_available = MS05_QS;
    s.tx_descriptor_inflight = 0;
    return s;
}

static struct ms05_snapshot zero_delta(void)
{
    struct ms05_snapshot d = {0};
    return d;
}

static void test_counter_regression_is_rejected(void)
{
    struct ms05_snapshot pre = active_snapshot();
    struct ms05_snapshot post = active_snapshot();
    struct ms05_snapshot delta;
    pre.tx_submit = 2;
    post.tx_submit = 1;
    assert(ms05_snapshot_delta(&pre, &post, &delta) != 0);
}

static void test_gauges_are_not_subtracted(void)
{
    struct ms05_snapshot pre = active_snapshot();
    struct ms05_snapshot post = active_snapshot();
    struct ms05_snapshot delta;
    pre.tx_slot_occupancy = 40;
    post.tx_slot_occupancy = 20;
    pre.hold_mode = 1;
    post.hold_mode = 0;
    assert(ms05_snapshot_delta(&pre, &post, &delta) == 0);
    assert(delta.tx_slot_occupancy == 0);
    assert(delta.hold_mode == 0);
}

static void test_active_and_safety_validation(void)
{
    struct ms05_snapshot post = active_snapshot();
    struct ms05_snapshot delta = zero_delta();

    assert(ms05_active(&post));
    post.rx_lifecycle = 0;
    assert(!ms05_active(&post));
    post = active_snapshot();

    assert(ms05_common_valid(&post, &delta));
    post.fault = 1;
    assert(!ms05_common_valid(&post, &delta));
    post = active_snapshot();
    post.lifecycle_fault = 1;
    assert(!ms05_common_valid(&post, &delta));
    post = active_snapshot();
    post.ownership_invariant = 1;
    assert(!ms05_common_valid(&post, &delta));
    post = active_snapshot();
    post.restore_violation = 1;
    assert(!ms05_common_valid(&post, &delta));
    post = active_snapshot();
    post.irq_enabled_entry = 1;
    assert(!ms05_common_valid(&post, &delta));
}

static void test_ledger_conservation(void)
{
    struct ms05_snapshot pre = active_snapshot();
    struct ms05_snapshot post = active_snapshot();

    assert(ms05_tx_ledger_closed(&pre, &post));
    post.tx_buffer_available = MS05_QS - 1;
    post.tx_buffer_inflight = 0; /* lost one buffer */
    assert(!ms05_tx_ledger_closed(&pre, &post));
    post = active_snapshot();
    post.tx_descriptor_available = MS05_QS - 1;
    post.tx_descriptor_inflight = 0; /* lost one descriptor */
    assert(!ms05_tx_ledger_closed(&pre, &post));
}

static void test_slot_full_proof(void)
{
    struct ms05_snapshot held = active_snapshot();
    struct ms05_snapshot full = active_snapshot();
    held.tx_slot_occupancy = 40;
    full.tx_slot_occupancy = MS05_SLOT_CAPACITY;
    full.tx_slot_full = 1;
    full.tx_slot_high_water = MS05_SLOT_CAPACITY;

    assert(ms05_slot_full_proved(&held, &full));
    full.tx_slot_occupancy = MS05_SLOT_CAPACITY - 1; /* fake Full */
    assert(!ms05_slot_full_proved(&held, &full));
    full = active_snapshot();
    full.tx_slot_occupancy = MS05_SLOT_CAPACITY;
    full.tx_slot_high_water = MS05_SLOT_CAPACITY;
    /* full transition did not occur since held */
    assert(!ms05_slot_full_proved(&held, &full));
}

static void test_descriptor_full_proof(void)
{
    struct ms05_snapshot held = active_snapshot();
    struct ms05_snapshot full = active_snapshot();
    held.tx_again = 0;
    full.tx_again = 1;
    full.tx_buffer_available = 0;
    full.tx_buffer_inflight = MS05_QS;
    full.tx_descriptor_available = 0;
    full.tx_descriptor_inflight = MS05_QS;

    assert(ms05_descriptor_full_proved(&held, &full));
    full.tx_buffer_available = 1; /* not fully exhausted */
    assert(!ms05_descriptor_full_proved(&held, &full));
    full = active_snapshot();
    full.tx_again = 1;
    full.tx_buffer_available = 0;
    full.tx_buffer_inflight = MS05_QS;
    full.tx_descriptor_available = 32; /* descriptor headroom remains */
    full.tx_descriptor_inflight = 32;
    assert(!ms05_descriptor_full_proved(&held, &full));
    full = active_snapshot();
    full.tx_again = 1;
    full.tx_buffer_available = 0;
    full.tx_buffer_inflight = MS05_QS;
    full.tx_descriptor_available = 0;
    full.tx_descriptor_inflight = MS05_QS - 1; /* one descriptor inflight short */
    assert(!ms05_descriptor_full_proved(&held, &full));
    full = active_snapshot();
    full.tx_again = 0; /* again never fired */
    full.tx_buffer_available = 0;
    full.tx_buffer_inflight = MS05_QS;
    full.tx_descriptor_available = 0;
    full.tx_descriptor_inflight = MS05_QS;
    assert(!ms05_descriptor_full_proved(&held, &full));
}

static void test_flush_proof(void)
{
    struct ms05_snapshot pre = active_snapshot();
    struct ms05_snapshot post = active_snapshot();
    pre.flush_success = 0;
    post.flush_success = 1;
    post.live = 0;
    post.queued = 0;
    post.device_owned = 0;

    assert(ms05_flush_proved(&pre, &post));
    post.flush_success = 0; /* no success recorded */
    assert(!ms05_flush_proved(&pre, &post));
    post.flush_success = 1;
    post.live = 1; /* live ticket survives flush target */
    assert(!ms05_flush_proved(&pre, &post));

    /* a flush error/busy/cancel delta rejects closure */
    post = active_snapshot();
    post.flush_success = 1;
    post.flush_error = 1;
    assert(!ms05_flush_proved(&pre, &post));
    post = active_snapshot();
    post.flush_success = 1;
    post.flush_busy = 1;
    assert(!ms05_flush_proved(&pre, &post));
    post = active_snapshot();
    post.flush_success = 1;
    post.flush_cancel = 1;
    assert(!ms05_flush_proved(&pre, &post));

    /* wrap: a success counter at u64 max cannot claim another success */
    pre = active_snapshot();
    pre.flush_success = UINT64_MAX;
    post = active_snapshot();
    post.flush_success = 0;
    assert(!ms05_flush_proved(&pre, &post));
}

static void test_deadline_boundaries(void)
{
    /* phase deadline: strictly-before passes; equal/after/regression expire */
    assert(!ms05_deadline_expired(1000, 1099, 1200, 0));
    assert(ms05_deadline_expired(1000, 2200, 1200, 0));
    assert(ms05_deadline_expired(1000, 1000 + 1200, 1200, 0)); /* equal */
    assert(ms05_deadline_expired(1000, 999, 1200, 0));         /* regression */

    /* absolute mode deadline: equal/late expire even when the phase
     * deadline still has room */
    assert(!ms05_deadline_expired(1000, 1400, 1200, 1600));
    assert(ms05_deadline_expired(1000, 1400, 1200, 1400)); /* equal */
    assert(ms05_deadline_expired(1000, 1400, 1200, 1300)); /* late vs mode */
    assert(ms05_deadline_expired(1000, 1000, 1200, 1000)); /* now == abs */
    assert(ms05_deadline_expired(2000, 2100, 1200, 1500)); /* abs < start */
}

static void test_mode_deadline_abs(void)
{
    uint64_t abs = 0;
    assert(ms05_mode_deadline_abs(1000, 6000, &abs) == 0);
    assert(abs == 7000);
    /* checked arithmetic: a wrap cannot produce a usable bound */
    assert(ms05_mode_deadline_abs(UINT64_MAX - 10, 20, &abs) != 0);
    assert(ms05_mode_deadline_abs(UINT64_MAX - 20, 20, &abs) == 0);
    assert(abs == UINT64_MAX);
}

static void test_budget_remaining(void)
{
    assert(ms05_budget_remaining_ms(1000, 1000, 6000) == 6000);
    assert(ms05_budget_remaining_ms(1000, 6999, 6000) == 1);
    assert(ms05_budget_remaining_ms(1000, 7000, 6000) == 0); /* equal */
    assert(ms05_budget_remaining_ms(1000, 8000, 6000) == 0); /* late */
    assert(ms05_budget_remaining_ms(1000, 999, 6000) == 0);  /* regression */
}

static void test_post_closure(void)
{
    struct ms05_snapshot post = active_snapshot();
    assert(ms05_post_closed(&post));

    /* all buffers/descriptors inflight satisfies conservation but is not
     * closure */
    post.tx_buffer_available = 0;
    post.tx_buffer_inflight = MS05_QS;
    post.tx_descriptor_available = 0;
    post.tx_descriptor_inflight = MS05_QS;
    assert(!ms05_post_closed(&post));

    post = active_snapshot();
    post.live = 1;
    assert(!ms05_post_closed(&post));
    post = active_snapshot();
    post.queued = 1;
    assert(!ms05_post_closed(&post));
    post = active_snapshot();
    post.device_owned = 1;
    assert(!ms05_post_closed(&post));
    post = active_snapshot();
    post.tx_slot_occupancy = 1;
    assert(!ms05_post_closed(&post));
    post = active_snapshot();
    post.tx_slot_enqueue = 1;
    assert(!ms05_post_closed(&post));
    post = active_snapshot();
    post.tx_buffer_inflight = 1;
    assert(!ms05_post_closed(&post));
    post = active_snapshot();
    post.tx_descriptor_inflight = 1;
    assert(!ms05_post_closed(&post));
}

static void test_conservation_is_not_closure(void)
{
    struct ms05_snapshot pre = active_snapshot();
    struct ms05_snapshot post = active_snapshot();
    post.tx_buffer_available = 0;
    post.tx_buffer_inflight = MS05_QS;
    post.tx_descriptor_available = 0;
    post.tx_descriptor_inflight = MS05_QS;
    assert(ms05_tx_ledger_closed(&pre, &post)); /* sums hold */
    assert(!ms05_post_closed(&post));           /* but nothing returned */
}

static void test_phase_order_valid(void)
{
    static const uint8_t required_plain[MS05_PHASES_PLAIN] = {
        MS05_PHASE_PRE, MS05_PHASE_POST,
    };
    static const uint8_t required_held[MS05_PHASES_HELD] = {
        MS05_PHASE_PRE, MS05_PHASE_HELD, MS05_PHASE_FULL,
        MS05_PHASE_RELEASED, MS05_PHASE_POST,
    };

    const uint8_t ok_plain[MS05_PHASES_PLAIN] = {
        MS05_PHASE_PRE, MS05_PHASE_POST,
    };
    const uint8_t ok_held[MS05_PHASES_HELD] = {
        MS05_PHASE_PRE, MS05_PHASE_HELD, MS05_PHASE_FULL,
        MS05_PHASE_RELEASED, MS05_PHASE_POST,
    };
    const uint8_t missing_pre[MS05_PHASES_PLAIN] = {
        MS05_PHASE_POST, MS05_PHASE_POST,
    };
    const uint8_t reordered[MS05_PHASES_HELD] = {
        MS05_PHASE_PRE, MS05_PHASE_FULL, MS05_PHASE_HELD,
        MS05_PHASE_RELEASED, MS05_PHASE_POST,
    };
    const uint8_t duplicated[MS05_PHASES_HELD] = {
        MS05_PHASE_PRE, MS05_PHASE_HELD, MS05_PHASE_FULL,
        MS05_PHASE_FULL, MS05_PHASE_POST,
    };

    assert(ms05_phase_order_valid(ok_plain, MS05_PHASES_PLAIN,
                                  required_plain, MS05_PHASES_PLAIN));
    assert(ms05_phase_order_valid(ok_held, MS05_PHASES_HELD,
                                  required_held, MS05_PHASES_HELD));
    assert(!ms05_phase_order_valid(missing_pre, MS05_PHASES_PLAIN,
                                   required_plain, MS05_PHASES_PLAIN));
    assert(!ms05_phase_order_valid(reordered, MS05_PHASES_HELD,
                                   required_held, MS05_PHASES_HELD));
    assert(!ms05_phase_order_valid(duplicated, MS05_PHASES_HELD,
                                   required_held, MS05_PHASES_HELD));
}

static void test_marker_parse(void)
{
    char mode[32];
    int pass = 0;

    assert(ms05_marker_parse("MS05 PASS mode=slot-full", mode, sizeof(mode),
                             &pass) == 1);
    assert(pass == 1);
    assert(strcmp(mode, "slot-full") == 0);

    assert(ms05_marker_parse("MS05 FAIL mode=flush reason=timeout", mode,
                             sizeof(mode), &pass) == 1);
    assert(pass == 0);
    assert(strcmp(mode, "flush") == 0);

    /* no marker */
    assert(ms05_marker_parse("MS05 PRE total=0", mode, sizeof(mode), &pass) == 0);

    /* duplicate / conflicting markers in one line */
    assert(ms05_marker_parse(
               "MS05 PASS mode=a MS05 PASS mode=a", mode, sizeof(mode), &pass) == -1);
    assert(ms05_marker_parse(
               "MS05 PASS mode=a MS05 FAIL mode=b", mode, sizeof(mode), &pass) == -1);

    /* malformed marker */
    assert(ms05_marker_parse("MS05 PASS mode=", mode, sizeof(mode), &pass) == -1);
    assert(ms05_marker_parse("MS05 MAybe mode=a", mode, sizeof(mode), &pass) == 0);
}

static void test_exit_consistency(void)
{
    assert(ms05_exit_consistent(1, 0));
    assert(ms05_exit_consistent(0, 1));
    assert(!ms05_exit_consistent(1, 1));
    assert(!ms05_exit_consistent(0, 0));
}

static void test_datagram_validation(void)
{
    struct ms05_wire_header {
        uint32_t magic;
        uint32_t sequence;
        uint32_t count;
    };
    uint8_t packet[sizeof(struct ms05_wire_header) + 64];
    memset(packet, 0x5a, sizeof(packet));
    struct ms05_wire_header *header = (struct ms05_wire_header *)packet;
    header->magic = ms05_be32(MS05_MAGIC);
    header->sequence = ms05_be32(3);
    header->count = ms05_be32(96);
    for (size_t i = 0; i < 64; ++i) {
        packet[sizeof(struct ms05_wire_header) + i] =
            (uint8_t)((3 + i) & 0xffu);
    }

    assert(ms05_validate_datagram(packet, (ssize_t)sizeof(packet), 3, 96, 64) == 0);
    assert(ms05_validate_datagram(packet, (ssize_t)sizeof(packet), 4, 96, 64) != 0);
    assert(ms05_validate_datagram(packet, (ssize_t)sizeof(packet) - 1, 3, 96, 64) != 0);
    header->magic = ms05_be32(0xdeadbeefu);
    assert(ms05_validate_datagram(packet, (ssize_t)sizeof(packet), 3, 96, 64) != 0);
}

static void test_traffic_exact_rules(void)
{
    /* EXACT modes require the exact nonzero requested count in both
     * directions. */
    assert(ms05_traffic_proved(MS05_TRAFFIC_EXACT, 96, 96, 96));
    /* zero and partial traffic are vacuous and must fail. */
    assert(!ms05_traffic_proved(MS05_TRAFFIC_EXACT, 96, 0, 0));
    assert(!ms05_traffic_proved(MS05_TRAFFIC_EXACT, 96, 95, 95));
    assert(!ms05_traffic_proved(MS05_TRAFFIC_EXACT, 96, 96, 95));
    /* a registered count of zero is rejected at the protocol boundary. */
    assert(!ms05_traffic_proved(MS05_TRAFFIC_EXACT, 0, 0, 0));
}

static void test_traffic_held_rules(void)
{
    /* HELD modes accept a nonzero short send only within the requested
     * count; the exact Full/Again proof explains the short send. */
    assert(ms05_traffic_proved(MS05_TRAFFIC_HELD, 96, 40, 40));
    assert(ms05_traffic_proved(MS05_TRAFFIC_HELD, 96, 96, 96));
    /* zero held send is never valid traffic. */
    assert(!ms05_traffic_proved(MS05_TRAFFIC_HELD, 96, 0, 0));
    /* sending more than the requested count is never valid. */
    assert(!ms05_traffic_proved(MS05_TRAFFIC_HELD, 96, 97, 97));
    /* received must equal the reported sent count. */
    assert(!ms05_traffic_proved(MS05_TRAFFIC_HELD, 96, 40, 39));
}

static void test_clamped_budget(void)
{
    struct ms05_deadline_ctx ctx;
    memset(&ctx, 0, sizeof(ctx));
    ctx.mode_start = 1000;
    ctx.mode_abs = 1000 + MS05_MODE_DEADLINE_MS;
    ctx.phase_start = 1000;
    ctx.phase_deadline_ms = 0; /* phase bound disabled */

    assert(ms05_ctx_budget_ms(&ctx, 2000) == MS05_MODE_DEADLINE_MS - 1000);
    assert(ms05_ctx_budget_ms(&ctx, 1000 + MS05_MODE_DEADLINE_MS) == 0);
    assert(ms05_ctx_budget_ms(&ctx, 999) == 0); /* regressed clock */

    /* a tighter phase bound wins. */
    ctx.phase_start = 2000;
    ctx.phase_deadline_ms = 1200;
    assert(ms05_ctx_budget_ms(&ctx, 2500) == 700);
    assert(ms05_ctx_budget_ms(&ctx, 3200) == 0); /* phase exhausted */
    assert(ms05_ctx_budget_ms(&ctx, 1500) == 0); /* before phase start */

    /* absolute mode deadline caps the relative budget. */
    ctx.phase_deadline_ms = 0;
    ctx.mode_abs = 4000;
    assert(ms05_ctx_budget_ms(&ctx, 3500) == 500);
    assert(ms05_ctx_budget_ms(&ctx, 4000) == 0);
    ctx.mode_abs = 500; /* mode deadline before mode start: unusable */
    assert(ms05_ctx_budget_ms(&ctx, 2000) == 0);
}

static void test_clamp_timeout(void)
{
    assert(ms05_clamp_timeout_ms(5000, 3000) == 3000); /* nominal wins */
    assert(ms05_clamp_timeout_ms(500, 3000) == 500);   /* budget wins */
    assert(ms05_clamp_timeout_ms(0, 3000) == 0);       /* exhausted */
    assert(ms05_clamp_timeout_ms(500, 0) == 0);        /* no nominal */
}

static void test_flush_affordable(void)
{
    assert(ms05_flush_affordable(3000, 2000));  /* budget contains timeout */
    assert(!ms05_flush_affordable(2000, 2000)); /* equal cannot guarantee */
    assert(!ms05_flush_affordable(1500, 2000)); /* budget too small */
}

static void test_wire_network_order(void)
{
    /* Known fixed network-order bytes: magic MS05, sequence 3, count 96. */
    static const uint8_t known[12] = {
        0x4d, 0x53, 0x30, 0x35, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x60,
    };
    uint8_t packet[sizeof(struct ms05_wire_header) + 64];
    uint8_t native[sizeof(packet)];
    memset(packet, 0, sizeof(packet));
    memcpy(packet, known, sizeof(known));
    for (size_t i = 0; i < 64; ++i) {
        packet[sizeof(known) + i] = (uint8_t)((3 + i) & 0xffu);
    }
    assert(ms05_validate_datagram(packet, (ssize_t)sizeof(packet), 3, 96, 64) == 0);

    /* The same header laid out in host native (little-endian) order must be
     * rejected: the production decoder only accepts network order. */
    memcpy(native, packet, sizeof(packet));
    native[0] = 0x35; native[1] = 0x30; native[2] = 0x53; native[3] = 0x4d;
    native[4] = 0x03; native[5] = 0x00; native[6] = 0x00; native[7] = 0x00;
    native[8] = 0x60; native[9] = 0x00; native[10] = 0x00; native[11] = 0x00;
    assert(ms05_validate_datagram(native, (ssize_t)sizeof(native), 3, 96, 64) != 0);
}

int main(void)
{
    test_clamped_budget();
    return 0;
}
