#define MS05_DATA_PLANE_PROBE_TESTING
#include "ms05_data_plane_probe.c"

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
    full.tx_again = 0; /* again never fired: not a FULL witness (repair 6.2-R4) */
    full.tx_buffer_available = 0;
    full.tx_buffer_inflight = MS05_QS;
    full.tx_descriptor_available = 0;
    full.tx_descriptor_inflight = MS05_QS;
    /* The conserved ledger proves real driver Full even when `tx_again` never
     * fired: slot capacity == driver capacity == MS05_QS and the 32-submit
     * budget divides 64 exactly, so the service drains to 64 in-flight at a
     * budget boundary with no pending slot to force the 65th submit. */
    assert(ms05_descriptor_full_proved(&held, &full));
    held.tx_again = 5;
    full.tx_again = 1; /* tx_again regression must not turn Full into FALSE */
    assert(ms05_descriptor_full_proved(&held, &full));
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

/* ── Production-runner operation seam tests (Task 5.3) ──────────────── */

static uint64_t g_fake_now;
static int g_fake_clock_fail_at_call; /* 1-based; 0 disables */
static int g_fake_clock_calls;
static int g_fake_ctl_script[16];
static size_t g_fake_ctl_n, g_fake_ctl_pos;
static uint64_t g_fake_ctl_ops[16];
static size_t g_fake_ctl_nops;
static int g_fake_sleep_ms[64];
static size_t g_fake_sleep_n;
static uint32_t g_fake_snd_timeout, g_fake_rcv_timeout;
static int g_fake_send_result;
static int g_fake_nonblock;
static int g_fake_flush_calls;
static int g_fake_flush_result;
static uint64_t g_fake_send_elapsed_ms, g_fake_recv_elapsed_ms;
static uint64_t g_fake_snd_timeout_elapsed_ms, g_fake_rcv_timeout_elapsed_ms;
static uint64_t g_fake_nonblock_elapsed_ms;
static int g_fake_send_calls, g_fake_recv_calls, g_fake_nonblock_calls;
static const uint8_t *g_fake_recv_data[64];
static size_t g_fake_recv_len[64];
static size_t g_fake_recv_n, g_fake_recv_pos;
static struct ms05_snapshot g_fake_snap[64];
static int g_fake_snap_result[64];
static size_t g_fake_snap_n, g_fake_snap_pos;

static int fake_clock_now(uint64_t *now)
{
    g_fake_clock_calls++;
    if (g_fake_clock_fail_at_call != 0 &&
        g_fake_clock_calls == g_fake_clock_fail_at_call) {
        return -1;
    }
    *now = g_fake_now;
    return 0;
}

static void fake_sleep_ms(uint32_t ms)
{
    if (g_fake_sleep_n < 64) g_fake_sleep_ms[g_fake_sleep_n++] = (int)ms;
    g_fake_now += ms;
}

static int fake_ioctl_ctl(uint64_t op, uint64_t lease_ms)
{
    (void)lease_ms;
    if (g_fake_ctl_nops < 16) g_fake_ctl_ops[g_fake_ctl_nops++] = op;
    if (g_fake_ctl_pos >= g_fake_ctl_n) return MS05_OP_ERROR;
    return g_fake_ctl_script[g_fake_ctl_pos++];
}

static int fake_ioctl_flush(void)
{
    g_fake_flush_calls++;
    return g_fake_flush_result;
}

static int fake_ioctl_snapshot(struct ms05_snapshot *out)
{
    if (g_fake_snap_pos >= g_fake_snap_n) return MS05_OP_ERROR;
    if (g_fake_snap_result[g_fake_snap_pos] == MS05_OP_OK) {
        *out = g_fake_snap[g_fake_snap_pos];
    }
    g_fake_snap_pos++;
    return g_fake_snap_result[g_fake_snap_pos - 1];
}

static int fake_sock_open(struct ms05_udp *u)
{
    u->fd = 7;
    return 0;
}

static void fake_sock_close(struct ms05_udp *u)
{
    u->fd = -1;
}

static int fake_sock_set_rcv_timeout(int fd, uint32_t ms)
{
    (void)fd;
    g_fake_rcv_timeout = ms;
    g_fake_now += g_fake_rcv_timeout_elapsed_ms;
    return 0;
}

static int fake_sock_set_snd_timeout(int fd, uint32_t ms)
{
    (void)fd;
    g_fake_snd_timeout = ms;
    g_fake_now += g_fake_snd_timeout_elapsed_ms;
    return 0;
}

static int fake_sock_set_nonblock(int fd, int enable)
{
    (void)fd;
    g_fake_nonblock = enable;
    g_fake_nonblock_calls++;
    g_fake_now += g_fake_nonblock_elapsed_ms;
    return 0;
}

static ssize_t fake_sock_send(int fd, const void *buf, size_t len)
{
    (void)fd;
    (void)buf;
    g_fake_send_calls++;
    g_fake_now += g_fake_send_elapsed_ms;
    if (g_fake_send_result != 0) return -1;
    return (ssize_t)len;
}

static ssize_t fake_sock_recv(int fd, void *buf, size_t len)
{
    (void)fd;
    g_fake_recv_calls++;
    g_fake_now += g_fake_recv_elapsed_ms;
    if (g_fake_recv_pos >= g_fake_recv_n) return -1;
    size_t n = g_fake_recv_len[g_fake_recv_pos];
    if (n > len) n = len;
    memcpy(buf, g_fake_recv_data[g_fake_recv_pos], n);
    g_fake_recv_pos++;
    return (ssize_t)n;
}

static void fake_ops_reset(void)
{
    g_fake_now = 100000;
    g_fake_clock_fail_at_call = 0;
    g_fake_clock_calls = 0;
    g_fake_ctl_n = g_fake_ctl_pos = g_fake_ctl_nops = 0;
    g_fake_sleep_n = 0;
    g_fake_snd_timeout = g_fake_rcv_timeout = 0;
    g_fake_send_result = 0;
    g_fake_nonblock = 0;
    g_fake_flush_calls = 0;
    g_fake_flush_result = MS05_OP_OK;
    g_fake_send_elapsed_ms = g_fake_recv_elapsed_ms = 0;
    g_fake_snd_timeout_elapsed_ms = g_fake_rcv_timeout_elapsed_ms = 0;
    g_fake_nonblock_elapsed_ms = 0;
    g_fake_send_calls = g_fake_recv_calls = g_fake_nonblock_calls = 0;
    g_fake_recv_n = g_fake_recv_pos = 0;
    g_fake_snap_n = g_fake_snap_pos = 0;
    g_ms05_ops.clock_now = fake_clock_now;
    g_ms05_ops.sleep_ms = fake_sleep_ms;
    g_ms05_ops.ioctl_ctl = fake_ioctl_ctl;
    g_ms05_ops.ioctl_flush = fake_ioctl_flush;
    g_ms05_ops.ioctl_snapshot = fake_ioctl_snapshot;
    g_ms05_ops.sock_open = fake_sock_open;
    g_ms05_ops.sock_close = fake_sock_close;
    g_ms05_ops.sock_set_rcv_timeout = fake_sock_set_rcv_timeout;
    g_ms05_ops.sock_set_snd_timeout = fake_sock_set_snd_timeout;
    g_ms05_ops.sock_set_nonblock = fake_sock_set_nonblock;
    g_ms05_ops.sock_send = fake_sock_send;
    g_ms05_ops.sock_recv = fake_sock_recv;
}

static void fake_ctl_push(int rc)
{
    if (g_fake_ctl_n < 16) g_fake_ctl_script[g_fake_ctl_n++] = rc;
}

static void fake_recv_push(const char *text)
{
    if (g_fake_recv_n >= 64) return;
    g_fake_recv_data[g_fake_recv_n] = (const uint8_t *)text;
    g_fake_recv_len[g_fake_recv_n] = strlen(text);
    g_fake_recv_n++;
}

static void fake_snap_push(const struct ms05_snapshot *s, int rc)
{
    if (g_fake_snap_n >= 64) return;
    g_fake_snap[g_fake_snap_n] = *s;
    g_fake_snap_result[g_fake_snap_n] = rc;
    g_fake_snap_n++;
}

/* Default fake snapshot: Active, async-owned, empty closed ledger. */
static struct ms05_snapshot fake_active_snapshot(void)
{
    struct ms05_snapshot s = {0};
    s.rx_lifecycle = 2;
    s.rx_owner = 1;
    s.tx_buffer_available = MS05_QS;
    s.tx_descriptor_available = MS05_QS;
    return s;
}

static size_t fake_release_count(void)
{
    size_t n = 0;
    for (size_t i = 0; i < g_fake_ctl_nops; ++i) {
        if (g_fake_ctl_ops[i] == MS05_CTL_RELEASE) n++;
    }
    return n;
}

/* A control send near the budget edge: the READY receive consumes almost all
 * of the mode budget, so the START control send is clamped to the remaining
 * budget. A send that then crosses the deadline must fail the handshake and
 * no later side effect (PRE snapshot) may run. */
static void test_seam_control_send_budget_edge(void)
{
    fake_ops_reset();
    fake_recv_push("MS05 READY tx-only 96 64");
    /* REGISTER send already advanced 7 ms; READY advances 5987 so the START
     * send begins with exactly 6 ms of mode budget remaining. */
    g_fake_recv_elapsed_ms = 5987;
    g_fake_send_elapsed_ms = 7;    /* START send crosses the deadline */
    int rc = run_tx_only(96, 64);
    assert(rc != 0);
    assert(g_fake_snap_pos == 0);      /* no PRE snapshot after late send */
    assert(g_fake_snd_timeout == 6);   /* clamped to remaining budget */
}

/* An EAGAIN control result near expiry: the retry sleep must clamp to the
 * remaining budget and the next ioctl must not start after expiry. */
static void test_seam_retry_sleep_clamped_no_late_ioctl(void)
{
    fake_ops_reset();
    fake_recv_push("MS05 READY slot-full 96 64");
    g_fake_recv_elapsed_ms = 5985; /* remaining mode budget becomes 15 ms */
    fake_ctl_push(MS05_OP_BUSY);   /* Hold returns EAGAIN once */
    struct ms05_snapshot s = fake_active_snapshot();
    fake_snap_push(&s, MS05_OP_OK); /* PRE */
    int rc = run_slot_full();
    assert(rc != 0);
    assert(g_fake_ctl_nops == 1);            /* no late retry ioctl */
    assert(g_fake_sleep_n == 1);
    assert(g_fake_sleep_ms[0] == 15);        /* clamped, not the 20 ms nominal */
    assert(fake_release_count() == 0);       /* Hold never committed */
}

/* A bounded retry: an EAGAIN Hold followed by a successful Hold within the
 * budget commits the hold (exactly one cleanup Release on the later error). */
static void test_seam_hold_commits_after_retry(void)
{
    fake_ops_reset();
    fake_recv_push("MS05 READY slot-full 96 64");
    g_fake_recv_elapsed_ms = 5970; /* remaining budget 30 ms */
    fake_ctl_push(MS05_OP_BUSY);
    fake_ctl_push(MS05_OP_OK);
    fake_ctl_push(MS05_OP_OK); /* cleanup Release succeeds */
    struct ms05_snapshot s = fake_active_snapshot();
    fake_snap_push(&s, MS05_OP_OK); /* PRE */
    fake_snap_push(&s, MS05_OP_ERROR); /* HELD read fails post-commit */
    int rc = run_slot_full();
    assert(rc != 0);
    assert(g_fake_sleep_n == 1);
    assert(g_fake_sleep_ms[0] == 20); /* clamped to nominal while affordable */
    assert(fake_release_count() == 1);
}

/* A post-Hold HELD snapshot failure must route through the single cleanup
 * owner and attempt exactly one Release. */
static void test_seam_held_snapshot_failure_cleanup(void)
{
    fake_ops_reset();
    fake_recv_push("MS05 READY slot-full 96 64");
    fake_ctl_push(MS05_OP_OK); /* Hold commits */
    fake_ctl_push(MS05_OP_OK); /* cleanup Release succeeds */
    struct ms05_snapshot s = fake_active_snapshot();
    fake_snap_push(&s, MS05_OP_OK);      /* PRE */
    fake_snap_push(&s, MS05_OP_ERROR);   /* HELD read fails */
    int rc = run_slot_full();
    assert(rc != 0);
    assert(fake_release_count() == 1);
    assert(g_fake_ctl_nops == 2);
    assert(g_fake_ctl_ops[0] == MS05_CTL_HOLD_SUBMIT);
    assert(g_fake_ctl_ops[1] == MS05_CTL_RELEASE);
}

/* A post-Hold clock failure at the held_at read must still release once. */
static void test_seam_held_clock_failure_cleanup(void)
{
    fake_ops_reset();
    fake_recv_push("MS05 READY slot-full 96 64");
    fake_ctl_push(MS05_OP_OK); /* Hold commits */
    fake_ctl_push(MS05_OP_OK); /* cleanup Release succeeds */
    struct ms05_snapshot s = fake_active_snapshot();
    fake_snap_push(&s, MS05_OP_OK); /* PRE */
    fake_snap_push(&s, MS05_OP_OK); /* HELD reads fine */
    s.hold_mode = MS05_CTL_HOLD_SUBMIT;
    /* clock calls: mode_start, REGISTER pre/fresh/post, READY pre/fresh/post,
     * START pre/fresh/post, PRE pre/post, Hold pre/post, HELD pre/post ->
     * held_at is the 17th clock read. */
    g_fake_clock_fail_at_call = 17;
    int rc = run_slot_full();
    assert(rc != 0);
    assert(fake_release_count() == 1);
}

/* A post-Hold hold-mode mismatch must release once. */
static void test_seam_hold_mode_mismatch_cleanup(void)
{
    fake_ops_reset();
    fake_recv_push("MS05 READY slot-full 96 64");
    fake_ctl_push(MS05_OP_OK); /* Hold commits */
    fake_ctl_push(MS05_OP_OK); /* cleanup Release succeeds */
    struct ms05_snapshot s = fake_active_snapshot();
    fake_snap_push(&s, MS05_OP_OK); /* PRE */
    fake_snap_push(&s, MS05_OP_OK); /* HELD reports hold_mode != op */
    int rc = run_slot_full();
    assert(rc != 0);
    assert(fake_release_count() == 1);
}

/* A post-Hold Full-wait deadline failure must release once under the original
 * mode deadline (phase-disabled cleanup context). */
static void test_seam_full_wait_cleanup(void)
{
    fake_ops_reset();
    fake_recv_push("MS05 READY slot-full 96 64");
    fake_ctl_push(MS05_OP_OK); /* Hold commits */
    fake_ctl_push(MS05_OP_OK); /* cleanup Release succeeds */
    struct ms05_snapshot s = fake_active_snapshot();
    s.hold_mode = MS05_CTL_HOLD_SUBMIT;
    fake_snap_push(&s, MS05_OP_OK); /* PRE */
    fake_snap_push(&s, MS05_OP_OK); /* HELD matches op */
    /* The FULL phase never satisfies the condition; 62 OK snapshots let the
     * 1200 ms FULL budget expire through clamped 20 ms poll sleeps (60). */
    for (int i = 0; i < 60; ++i) {
        fake_snap_push(&s, MS05_OP_OK);
    }
    int rc = run_slot_full();
    assert(rc != 0);
    assert(fake_release_count() == 1);
    assert(g_fake_sleep_n > 1);
}

/* A pre-Hold failure (Hold control error) must not invoke any Release. */
static void test_seam_no_cleanup_before_hold(void)
{
    fake_ops_reset();
    fake_recv_push("MS05 READY slot-full 96 64");
    fake_ctl_push(MS05_OP_ERROR); /* Hold fails outright */
    struct ms05_snapshot s = fake_active_snapshot();
    fake_snap_push(&s, MS05_OP_OK); /* PRE (never consumed) */
    int rc = run_slot_full();
    assert(rc != 0);
    assert(fake_release_count() == 0);
}

/* A failing cleanup Release is reported but creates no retry entry: exactly
 * one Release is attempted and the mode still fails. */
static void test_seam_release_cleanup_failure_reports(void)
{
    fake_ops_reset();
    fake_recv_push("MS05 READY slot-full 96 64");
    fake_ctl_push(MS05_OP_OK);      /* Hold commits */
    fake_ctl_push(MS05_OP_ERROR);   /* cleanup Release fails once */
    struct ms05_snapshot s = fake_active_snapshot();
    fake_snap_push(&s, MS05_OP_OK);    /* PRE */
    fake_snap_push(&s, MS05_OP_ERROR); /* HELD read fails post-commit */
    int rc = run_slot_full();
    assert(rc != 0);
    assert(fake_release_count() == 1);
    assert(g_fake_ctl_nops == 2);
}

/* Flush with a budget that cannot contain the 2 s kernel timeout must not
 * invoke the flush ioctl at all. */
static void test_seam_flush_budget_preflight(void)
{
    fake_ops_reset();
    fake_recv_push("MS05 READY flush 96 64");
    g_fake_recv_elapsed_ms = 4500; /* remaining budget 1500 ms < 2000 ms */
    struct ms05_snapshot s = fake_active_snapshot();
    fake_snap_push(&s, MS05_OP_OK); /* PRE */
    int rc = run_flush();
    assert(rc != 0);
    assert(g_fake_flush_calls == 0);
}

/* Flush with an affordable budget invokes the flush ioctl exactly once. */
static void test_seam_flush_success_invokes_ioctl(void)
{
    fake_ops_reset();
    fake_recv_push("MS05 READY flush 96 64");
    g_fake_recv_elapsed_ms = 0; /* full 6 s mode budget */
    fake_ctl_push(MS05_OP_OK);
    fake_ctl_push(MS05_OP_OK);
    fake_ctl_push(MS05_OP_OK);
    fake_ctl_push(MS05_OP_OK);
    fake_ctl_push(MS05_OP_OK);
    fake_ctl_push(MS05_OP_OK);
    fake_ctl_push(MS05_OP_OK);
    fake_ctl_push(MS05_OP_OK);
    struct ms05_snapshot s = fake_active_snapshot();
    fake_snap_push(&s, MS05_OP_OK); /* PRE */
    fake_snap_push(&s, MS05_OP_OK); /* POST via drain */
    int rc = run_flush();
    assert(rc != 0);
    assert(g_fake_flush_calls == 1);
}

/* Bidirectional: the RX receive timeout clamps to the remaining budget, so
 * a receive cannot extend the mode deadline. */
static void test_seam_bidirectional_clamped_recv(void)
{
    fake_ops_reset();
    fake_recv_push("MS05 READY bidirectional 96 64");
    g_fake_recv_elapsed_ms = 5994; /* remaining budget 6 ms at RX loop */
    struct ms05_snapshot s = fake_active_snapshot();
    fake_snap_push(&s, MS05_OP_OK); /* PRE */
    int rc = run_bidirectional(96, 64);
    assert(rc != 0);
    assert(g_fake_rcv_timeout == 6);
}

/* Descriptor-full held mode: a post-Hold HELD snapshot failure releases
 * exactly once under the descriptor-hold op. */
static void test_seam_descriptor_full_cleanup(void)
{
    fake_ops_reset();
    fake_recv_push("MS05 READY descriptor-full 96 64");
    fake_ctl_push(MS05_OP_OK); /* Hold (reclaim) commits */
    fake_ctl_push(MS05_OP_OK); /* cleanup Release succeeds */
    struct ms05_snapshot s = fake_active_snapshot();
    fake_snap_push(&s, MS05_OP_OK);    /* PRE */
    fake_snap_push(&s, MS05_OP_ERROR); /* HELD read fails post-commit */
    int rc = run_descriptor_full();
    assert(rc != 0);
    assert(fake_release_count() == 1);
    assert(g_fake_ctl_nops == 2);
    assert(g_fake_ctl_ops[0] == MS05_CTL_HOLD_RECLAIM);
    assert(g_fake_ctl_ops[1] == MS05_CTL_RELEASE);
}

/* Snapshot mode: two bounded snapshot reads and a clamped 100 ms sleep stay
 * within the mode budget and close PASS. */
static void test_seam_snapshot_mode(void)
{
    fake_ops_reset();
    struct ms05_snapshot s = fake_active_snapshot();
    fake_snap_push(&s, MS05_OP_OK); /* PRE */
    fake_snap_push(&s, MS05_OP_OK); /* POST */
    int rc = run_snapshot();
    assert(rc == 0);
    assert(g_fake_sleep_n == 1);
    assert(g_fake_sleep_ms[0] == 100);
}

/* RED: the snd-timeout setter consumes the entire mode budget; the send that
 * follows must NOT start (the I/O needs a fresh precheck after the setter).
 * The current runner starts the send at the deadline and only its postcheck
 * fails. */
static void test_seam_snd_timeout_setter_consumes_budget_blocks_send(void)
{
    fake_ops_reset();
    fake_recv_push("MS05 READY tx-only 96 64");
    g_fake_snd_timeout_elapsed_ms = 6000; /* setter reaches mode_abs */
    int rc = run_tx_only(96, 64);
    assert(rc != 0);
    assert(g_fake_send_calls == 0); /* no late send may start */
}

/* RED: the rcv-timeout setter consumes the entire mode budget; the receive
 * that follows must NOT start (fresh precheck after the setter). */
static void test_seam_rcv_timeout_setter_consumes_budget_blocks_recv(void)
{
    fake_ops_reset();
    fake_recv_push("MS05 READY tx-only 96 64");
    g_fake_rcv_timeout_elapsed_ms = 6000; /* setter reaches mode_abs */
    int rc = run_tx_only(96, 64);
    assert(rc != 0);
    assert(g_fake_recv_calls == 0); /* no late recv may start */
}

/* RED: the drain's nonblocking transition must never start when the mode
 * budget is already exhausted. The current runner calls sock_set_nonblock(1)
 * unconditionally at drain entry even at/after the deadline. */
static void test_seam_drain_nonblock_never_starts_late(void)
{
    fake_ops_reset();
    fake_recv_push("MS05 READY tx-only 96 64");
    /* 96 data sends at 62 ms each plus the handshake send consume 6076 ms,
     * so the mode budget is exhausted when the drain begins. */
    g_fake_send_elapsed_ms = 62;
    struct ms05_snapshot s = fake_active_snapshot();
    fake_snap_push(&s, MS05_OP_OK); /* PRE */
    int rc = run_tx_only(96, 64);
    assert(rc != 0);
    assert(g_fake_nonblock_calls == 0); /* no nonblock transition at 0 budget */
}

/* Lock: the drain's recv nudge advances the clock to exactly the mode
 * deadline; the drain must fail at the nudge boundary and never start a poll
 * sleep after a late return. */
static void test_seam_drain_recv_lands_at_deadline_no_sleep(void)
{
    fake_ops_reset();
    fake_recv_push("MS05 READY tx-only 96 64");
    g_fake_recv_elapsed_ms = 3000; /* READY recv + drain recv advance 6000 */
    struct ms05_snapshot s = fake_active_snapshot();
    fake_snap_push(&s, MS05_OP_OK); /* PRE */
    fake_snap_push(&s, MS05_OP_OK); /* drain snapshot (condition false) */
    int rc = run_tx_only(96, 64);
    assert(rc != 0);
    assert(g_fake_sleep_n == 0); /* no poll sleep after the late nudge */
}

/* RED (repair 6.2-R6): drives `udp_done_recv` through the op seam with one
 * configured received datagram and a bounded mode window. Returns the count
 * the production parser returned, or -1. `recv_elapsed_ms` advances the fake
 * clock across the receive so an equal/late completion is detectable. */
static int done_recv_via_seam(const char *mode, const char *text,
                              uint64_t recv_elapsed_ms)
{
    struct ms05_udp u;
    struct ms05_deadline_ctx ctx;
    uint64_t mode_start, mode_abs;
    fake_ops_reset();
    fake_recv_push(text);
    g_fake_recv_elapsed_ms = recv_elapsed_ms;
    mode_start = g_fake_now;
    assert(ms05_mode_deadline_abs(mode_start, MS05_MODE_DEADLINE_MS,
                                  &mode_abs) == 0);
    ctx.mode_start = mode_start;
    ctx.mode_abs = mode_abs;
    ctx.phase_start = mode_start;
    ctx.phase_deadline_ms = 0;
    assert(fake_sock_open(&u) == 0);
    return udp_done_recv(&u, mode, &ctx);
}

/* RED (repair 6.2-R6): a DONE carrying trailing text after the numeric count
 * is permissively accepted today because `udp_done_recv` uses `strtoul`,
 * which parses a leading numeric prefix and ignores the remainder. It must be
 * rejected before ACK: a host DONE of `tx-only 96x` is not an exact DONE. */
static void test_udp_done_rejects_trailing(void)
{
    assert(done_recv_via_seam("tx-only", "MS05 DONE tx-only 96x", 0) != 96);
}

/* RED (repair 6.2-R6): a DONE whose numeric count overflows u64 must be
 * rejected, not silently truncated by `strtoul` to ULONG_MAX then to u32. */
static void test_udp_done_rejects_overflow(void)
{
    assert(done_recv_via_seam(
               "tx-only", "MS05 DONE tx-only 18446744073709551616", 0) != 0);
}

/* RED (repair 6.2-R6): a DONE for the wrong mode must not yield a count. */
static void test_udp_done_rejects_wrong_mode(void)
{
    assert(done_recv_via_seam(
               "tx-only", "MS05 DONE bidirectional 96", 0) != 96);
}

/* RED (repair 6.2-R6): a DONE with a missing numeric count must fail. */
static void test_udp_done_rejects_missing_count(void)
{
    assert(done_recv_via_seam("tx-only", "MS05 DONE tx-only", 0) != 96);
}

/* An exact valid DONE returns the shared count under the seam. */
static void test_udp_done_accepts_exact(void)
{
    assert(done_recv_via_seam("tx-only", "MS05 DONE tx-only 96", 0) == 96);
}

/* RED (repair 6.2-R8): a DONE count outside the command-line `1..4096` bound
 * must be rejected before the narrowing conversion. `4294967392` (= 2^32 + 96)
 * fits `unsigned long` but wraps to `96` when narrowed to `int`, so today it
 * masquerades as the normal 96-packet completion instead of failing. */
static void test_udp_done_rejects_wrap_into_valid(void)
{
    assert(done_recv_via_seam(
               "tx-only", "MS05 DONE tx-only 4294967392", 0) != 96);
    assert(done_recv_via_seam(
               "tx-only", "MS05 DONE tx-only 4294967392", 0) != 0);
}

/* RED (repair 6.2-R8): zero must fail; the probe protocol bound is 1..4096. */
static void test_udp_done_rejects_zero(void)
{
    assert(done_recv_via_seam("tx-only", "MS05 DONE tx-only 0", 0) == -1);
}

/* RED (repair 6.2-R8): a count above the upper protocol bound must fail. */
static void test_udp_done_rejects_above_max(void)
{
    assert(done_recv_via_seam("tx-only", "MS05 DONE tx-only 4097", 0) == -1);
}

/* The upper protocol boundary 4096 is a valid exact DONE. */
static void test_udp_done_accepts_max_boundary(void)
{
    assert(done_recv_via_seam("tx-only", "MS05 DONE tx-only 4096", 0) == 4096);
}

int main(void)
{
    test_counter_regression_is_rejected();
    test_gauges_are_not_subtracted();
    test_active_and_safety_validation();
    test_ledger_conservation();
    test_slot_full_proof();
    test_descriptor_full_proof();
    test_flush_proof();
    test_deadline_boundaries();
    test_mode_deadline_abs();
    test_budget_remaining();
    test_phase_order_valid();
    test_marker_parse();
    test_exit_consistency();
    test_datagram_validation();
    test_wire_network_order();
    test_post_closure();
    test_conservation_is_not_closure();
    test_traffic_exact_rules();
    test_traffic_held_rules();
    test_clamped_budget();
    test_clamp_timeout();
    test_flush_affordable();
    puts("ms05 probe decision tests: 22 passed");
    test_seam_control_send_budget_edge();
    test_seam_retry_sleep_clamped_no_late_ioctl();
    test_seam_hold_commits_after_retry();
    test_seam_held_snapshot_failure_cleanup();
    test_seam_held_clock_failure_cleanup();
    test_seam_hold_mode_mismatch_cleanup();
    test_seam_full_wait_cleanup();
    test_seam_no_cleanup_before_hold();
    test_seam_release_cleanup_failure_reports();
    test_seam_flush_budget_preflight();
    test_seam_flush_success_invokes_ioctl();
    test_seam_bidirectional_clamped_recv();
    test_seam_descriptor_full_cleanup();
    test_seam_snapshot_mode();
    test_seam_snd_timeout_setter_consumes_budget_blocks_send();
    test_seam_rcv_timeout_setter_consumes_budget_blocks_recv();
    test_seam_drain_nonblock_never_starts_late();
    test_seam_drain_recv_lands_at_deadline_no_sleep();
    puts("ms05 probe seam tests: 18 passed");
    test_udp_done_rejects_trailing();
    test_udp_done_rejects_overflow();
    test_udp_done_rejects_wrong_mode();
    test_udp_done_rejects_missing_count();
    test_udp_done_accepts_exact();
    test_udp_done_rejects_wrap_into_valid();
    test_udp_done_rejects_zero();
    test_udp_done_rejects_above_max();
    test_udp_done_accepts_max_boundary();
    puts("ms05 probe seam tests: 9 done-parsing passed");
    return 0;
}
