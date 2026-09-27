/* MS08 network SMP probe host decision test.  The pure placement/wake/
 * migration/conservation checks below are shared by the guest payload
 * (tests/ms08_network_smp_probe.c) and this host harness; the QEMU runtime
 * choreography stays in scripts/ms08-network-peer.py. */
#define MS08_NET_PROBE_TESTING
#include "ms08_network_smp_probe.c"

#include <assert.h>
#include <signal.h>

static struct ms08_net_v5 v5_fixture(void)
{
    struct ms08_net_v5 s;
    memset(&s, 0, sizeof(s));
    s.configured_harts = 16;
    s.schedulable_mask = 0xffff;
    s.owner_affinity = 2;
    s.runner_affinity = 3;
    s.irq_last_hart = 2;
    s.irq_hart_mask = 0x4;
    s.irq_events = 100;
    s.owner_last_hart = 2;
    s.owner_hart_mask = 0x4;
    s.owner_events = 500;
    s.runner_last_hart = 3;
    s.runner_hart_mask = 0x8;
    s.runner_events = 700;
    s.ipi_sent = 9;
    s.ipi_received = 9;
    s.affinity_rejects = 0;
    s.witness_phase = 0;
    s.current_valid = 1;
    s.current_queue_epoch = 7;
    s.current_socket_epoch = 11;
    s.current_link_generation = 13;
    s.current_link_state = 1;
    s.current_owner_available = 64;
    s.current_owner_device_owned = 64;
    s.current_owner_quarantined = 0;
    s.v3[MS08_NET_V3_RX_SLOT_OCC] = 0;
    s.v3[MS08_NET_V3_TX_SLOT_OCC] = 0;
    s.v3[MS08_NET_V3_TX_SLOT_ENQ] = 40;
    s.v3[MS08_NET_V3_TX_SLOT_DEQ] = 40;
    s.v3[MS08_NET_V3_TX_SUBMIT] = 40;
    s.v3[MS08_NET_V3_TX_AGAIN] = 0;
    s.v3[MS08_NET_V3_TX_COMPLETION] = 40;
    s.v3[MS08_NET_V3_TX_RECLAIM] = 40;
    s.v3[MS08_NET_V3_TX_BUF_AVAIL] = 64;
    s.v3[MS08_NET_V3_TX_BUF_INFLIGHT] = 0;
    s.v3[MS08_NET_V3_TX_DESC_AVAIL] = 64;
    s.v3[MS08_NET_V3_TX_DESC_INFLIGHT] = 0;
    s.v3[MS08_NET_V3_LIVE] = 0;
    s.v3[MS08_NET_V3_QUEUED] = 0;
    s.v3[MS08_NET_V3_TASK_POLL] = 300;
    s.v3[MS08_NET_V3_QUEUE_GENERATION] = 4;
    s.v3[MS08_NET_V3_LIFECYCLE] = 2;
    return s;
}

static uint64_t pack_view(uint8_t phase, uint32_t from, uint32_t to)
{
    return (uint64_t)phase | ((uint64_t)(from & 0xffffff) << 8) |
           ((uint64_t)(to & 0xffffff) << 32);
}

int main(void)
{
    /* Case registry is frozen, ordered and matches the schema contract
     * (Iteration 005 final profile: six network cases). */
    assert(ms08_net_case_count() == 6);
    assert(strcmp(ms08_net_cases[0], "placement") == 0);
    assert(strcmp(ms08_net_cases[5], "readiness-quiet") == 0);
    assert(ms08_net_schema_count() == 6);
    assert(strcmp(ms08_net_schema[1],
                  "timer-disabled-wake:MS08_NET_V5,MS08_NET_V5,MS08_NET_WAKE") == 0);
    assert(ms08_net_probe_self_test());

    /* Placement: distinct schedulable affinities naming the actual current poll
     * harts, with attributable progress and history masks that contain the pin
     * and never record an out-of-schedulable bit.  Extra schedulable harts
     * (boot-smoke migration history) are legal. */
    {
        struct ms08_net_v5 s = v5_fixture();
        assert(ms08_net_placement_ok(&s));
        s.owner_affinity = 3; /* owner == runner hart */
        assert(!ms08_net_placement_ok(&s));
        s.owner_affinity = 17; /* outside schedulable mask */
        assert(!ms08_net_placement_ok(&s));
        s.owner_affinity = 2;
        s.owner_last_hart = 5; /* observed owner drifted from the pin */
        assert(!ms08_net_placement_ok(&s));
        s.owner_last_hart = 2;
        s.runner_events = 0; /* no attributable runner progress */
        assert(!ms08_net_placement_ok(&s));
        s.runner_events = 700;
        s.owner_hart_mask = 0x4 | 0x10000; /* out-of-schedulable history bit */
        assert(!ms08_net_placement_ok(&s));
        s.owner_hart_mask = 0x4;
        s.runner_hart_mask = 0x8 | 0x20000;
        assert(!ms08_net_placement_ok(&s));
        s.runner_hart_mask = 0x8;
        s.owner_hart_mask = 0x1; /* current pin absent from history mask */
        assert(!ms08_net_placement_ok(&s));
        s.owner_hart_mask = 0x4;
        s.runner_hart_mask = 0x2;
        assert(!ms08_net_placement_ok(&s));
        /* Cycle 002 (6.2-R1): in-set but unrecorded history bits are
         * rejected; a role's own recorded (Restored) migration explains its
         * extra bits, and the recorded hart must still be schedulable. */
        s.owner_hart_mask = 0x4 | 0x40;  /* unrecorded hart 6 */
        s.runner_hart_mask = 0x8 | 0x80; /* unrecorded hart 7 */
        assert(!ms08_net_placement_ok(&s));
        s.migration_owner_state = pack_view(2, 2, 6);
        s.migration_runner_state = pack_view(2, 3, 7);
        assert(ms08_net_placement_ok(&s));
        s.migration_owner_state = pack_view(2, 2, 17); /* outside 0xffff */
        s.owner_hart_mask = 0x4 | ((uint64_t)1 << 17);
        assert(!ms08_net_placement_ok(&s));
    }

    /* Timer-disabled remote wake: exactly one completed run, timer restored,
     * IPI causality after > before, trigger hart != target hart, no terminal
     * without restore. */
    {
        struct ms08_net_v5 before = v5_fixture(), after = before;
        after.witness_completed = before.witness_completed + 1;
        after.witness_timer_restored = before.witness_timer_restored + 1;
        after.witness_target_ipi_before = 3;
        after.witness_target_ipi_after = 4;
        after.witness_last_target_hart = 3;
        after.witness_last_trigger_hart = 9;
        assert(ms08_net_wake_ok(&before, &after, 3));
        after.witness_target_ipi_after = 3; /* no IPI delivered */
        assert(!ms08_net_wake_ok(&before, &after, 3));
        after.witness_target_ipi_after = 4;
        after.witness_last_trigger_hart = 3; /* local trigger accepted */
        assert(!ms08_net_wake_ok(&before, &after, 3));
        after.witness_last_trigger_hart = 9;
        after.witness_missing_restore = 1;
        assert(!ms08_net_wake_ok(&before, &after, 3));
        after.witness_missing_restore = 0;
        after.witness_completed = before.witness_completed; /* no run */
        assert(!ms08_net_wake_ok(&before, &after, 3));
    }

    /* Full -> recovery: capacity pressure is proven by tx_again/slot-full,
     * release must restore send progress and conserve the slot ledger. */
    {
        struct ms08_net_v5 full = v5_fixture(), after = v5_fixture();
        full.v3[MS08_NET_V3_TX_AGAIN] = 2;
        full.v3[MS08_NET_V3_TX_SLOT_FULL] = 1;
        assert(ms08_net_full_observed(&full));
        struct ms08_net_v5 soft = v5_fixture();
        assert(!ms08_net_full_observed(&soft)); /* no pressure seen */
        after.v3[MS08_NET_V3_TX_SUBMIT] = 48;
        after.v3[MS08_NET_V3_TX_COMPLETION] = 48;
        after.v3[MS08_NET_V3_TX_RECLAIM] = 48;
        after.v3[MS08_NET_V3_TX_SLOT_ENQ] = 48;
        after.v3[MS08_NET_V3_TX_SLOT_DEQ] = 48;
        assert(ms08_net_full_recovered(&full, &after));
        struct ms08_net_v5 leaky = after;
        leaky.v3[MS08_NET_V3_TX_RECLAIM] = 47; /* a buffer leaked across recovery */
        assert(!ms08_net_full_recovered(&full, &leaky));
        leaky.v3[MS08_NET_V3_TX_RECLAIM] = 48;
        leaky.v3[MS08_NET_V3_TX_BUF_INFLIGHT] = 1; /* recovery left an inflight buffer */
        assert(!ms08_net_full_recovered(&full, &leaky));
    }

    /* Quiet: no stack progress and no traffic movement inside the window. */
    {
        struct ms08_net_v5 a = v5_fixture(), b = a;
        assert(ms08_net_quiet_ok(&a, &b));
        b.v3[MS08_NET_V3_TASK_POLL] = 301;
        assert(!ms08_net_quiet_ok(&a, &b));
        b.v3[MS08_NET_V3_TASK_POLL] = 300;
        b.irq_events = 101;
        assert(!ms08_net_quiet_ok(&a, &b));
        b.irq_events = 100;
        b.owner_events = 501;
        assert(!ms08_net_quiet_ok(&a, &b));
    }

    /* Reset transition: exactly QueueEpoch and SocketEpoch advance by one. */
    {
        struct ms08_net_v5 before = v5_fixture(), after = before;
        after.current_queue_epoch = 8;
        after.current_socket_epoch = 12;
        assert(ms08_net_reset_transition_valid(&before, &after));
        after.current_link_generation = 14;
        assert(!ms08_net_reset_transition_valid(&before, &after));
        after.current_link_generation = 13;
        after.current_owner_available = 63;
        assert(!ms08_net_reset_transition_valid(&before, &after));
    }

    /* Link down/up: a flap conserves the owner ledger and bumps LinkGeneration
     * by exactly one in each direction without touching the epochs. */
    {
        struct ms08_net_v5 before = v5_fixture(), down = before, up = down;
        down.current_link_generation = 14;
        down.current_link_state = 0;
        assert(ms08_net_link_down_valid(&before, &down));
        up.current_link_generation = 15;
        up.current_link_state = 1;
        up.current_socket_epoch = 12;
        assert(ms08_net_link_up_valid(&before, &down, &up));
        struct ms08_net_v5 leaky = down;
        leaky.current_owner_available = 63;
        assert(!ms08_net_link_down_valid(&before, &leaky));
        up.current_socket_epoch = 11; /* flap must not rewind the socket epoch */
        assert(!ms08_net_link_up_valid(&before, &down, &up));
    }

    /* Controlled owner/runner migration: Widened moves the observed hart to
     * the announced second hart with event progress; Restored lands back on
     * the original singleton; the other role and the ticket ledger hold. */
    {
        struct ms08_net_v5 pre = v5_fixture(), mid = pre, post = pre;
        mid.migration_owner_state = pack_view(1, 2, 6);
        mid.owner_last_hart = 6;
        mid.owner_hart_mask = 0x4 | 0x40;
        mid.owner_events = pre.owner_events + 5;
        assert(ms08_net_migration_widened(&pre, &mid, 0));
        mid.owner_last_hart = 2; /* owner never actually moved */
        assert(!ms08_net_migration_widened(&pre, &mid, 0));
        mid.owner_last_hart = 6;
        mid.runner_last_hart = 2; /* the runner drifted: role confusion */
        assert(!ms08_net_migration_widened(&pre, &mid, 0));
        mid.runner_last_hart = 3;
        mid.owner_events = pre.owner_events; /* no poll progress on second */
        assert(!ms08_net_migration_widened(&pre, &mid, 0));
        mid.owner_events = pre.owner_events + 5;
        mid.v3[MS08_NET_V3_LIVE] = 1; /* an un-reclaimed ticket mid-migration */
        assert(!ms08_net_migration_widened(&pre, &mid, 0));
        mid.v3[MS08_NET_V3_LIVE] = 0;

        post.migration_owner_state = pack_view(2, 2, 6);
        post.owner_last_hart = 2;
        post.owner_affinity = 2;
        post.owner_events = mid.owner_events + 2;
        assert(ms08_net_migration_restored(&mid, &post, 0));
        post.owner_affinity = 6; /* singleton pin not restored */
        assert(!ms08_net_migration_restored(&mid, &post, 0));
        post.owner_affinity = 2;
        post.owner_events = mid.owner_events; /* no further progress */
        assert(!ms08_net_migration_restored(&mid, &post, 0));
        post.owner_events = mid.owner_events + 2;
        post.v3[MS08_NET_V3_TX_RECLAIM] = 39; /* ticket ledger drifted during migration */
        assert(!ms08_net_migration_restored(&mid, &post, 0));

        /* Runner direction discriminates the fields under check. */
        struct ms08_net_v5 rpre = v5_fixture(), rmid = rpre;
        rmid.migration_runner_state = pack_view(1, 3, 7);
        rmid.runner_last_hart = 7;
        rmid.runner_hart_mask = 0x8 | 0x80;
        rmid.runner_events = rpre.runner_events + 4;
        assert(ms08_net_migration_widened(&rpre, &rmid, 1));
        rmid.owner_last_hart = 7; /* owner follows the runner: rejected */
        assert(!ms08_net_migration_widened(&rpre, &rmid, 1));
    }

    /* Migration pre-state 'idle' semantics (Iteration 005, 6.2): the slot is
     * ready for a new controlled migration when None (0, never migrated) or
     * Restored (2, an earlier boot-smoke migration already returned to the
     * singleton pin); only an in-flight Widened (1) blocks re-entry. */
    {
        assert(ms08_net_mig_idle(pack_view(0, 2, 6)));
        assert(ms08_net_mig_idle(pack_view(2, 2, 6)));
        assert(!ms08_net_mig_idle(pack_view(1, 2, 6)));

        /* A Restored pre-state is accepted as idle by the Widened relation. */
        struct ms08_net_v5 pre = v5_fixture(), mid = pre;
        pre.migration_owner_state = pack_view(2, 2, 6);
        mid.migration_owner_state = pack_view(1, 2, 6);
        mid.owner_last_hart = 6;
        mid.owner_hart_mask = 0x4 | 0x40;
        mid.owner_events = pre.owner_events + 5;
        assert(ms08_net_migration_widened(&pre, &mid, 0));
        /* A Widened pre-state is not idle and is rejected. */
        struct ms08_net_v5 busy = pre, bmid = pre;
        busy.migration_owner_state = pack_view(1, 2, 6);
        bmid.migration_owner_state = pack_view(1, 2, 8);
        assert(!ms08_net_migration_widened(&busy, &bmid, 0));
    }

    /* Completed migration without a sampled Widened mid-state (Iteration 005,
     * 6.2): a fast runtime migration may finish between snapshot reads; the
     * Restored post view plus progress and a closed ledger prove the cycle.
     * Cycle 002 (6.2-R1): the relation must also identify the reported
     * distinct schedulable target and require its bit in the role's
     * cumulative history, with the other role holding its pin. */
    {
        struct ms08_net_v5 pre = v5_fixture(), post = pre;
        pre.migration_owner_state = pack_view(2, 2, 6);
        post.migration_owner_state = pack_view(2, 2, 6);
        post.owner_events = pre.owner_events + 5;
        post.owner_hart_mask = 0x4 | 0x40; /* the visited hart is recorded */
        assert(ms08_net_migration_completed(&pre, &post, 0));
        post.migration_owner_state = pack_view(1, 2, 6); /* not Restored */
        assert(!ms08_net_migration_completed(&pre, &post, 0));
        post.migration_owner_state = pack_view(2, 2, 6);
        post.owner_last_hart = 6; /* did not return to the pin */
        assert(!ms08_net_migration_completed(&pre, &post, 0));
        post.owner_last_hart = 2;
        post.owner_events = pre.owner_events; /* no progress */
        assert(!ms08_net_migration_completed(&pre, &post, 0));
        post.owner_events = pre.owner_events + 5;
        post.v3[MS08_NET_V3_LIVE] = 1; /* leaked ticket */
        assert(!ms08_net_migration_completed(&pre, &post, 0));
        post.v3[MS08_NET_V3_LIVE] = 0;
        post.migration_owner_state = pack_view(2, 2, 2); /* to == from */
        assert(!ms08_net_migration_completed(&pre, &post, 0));
        post.migration_owner_state = pack_view(2, 2, 40); /* unschedulable */
        assert(!ms08_net_migration_completed(&pre, &post, 0));
        post.migration_owner_state = pack_view(2, 2, 6);
        post.owner_hart_mask = 0x4; /* visited hart not in the history */
        assert(!ms08_net_migration_completed(&pre, &post, 0));
        post.owner_hart_mask = 0x4 | 0x40;
        post.runner_last_hart = 7; /* the other role drifted */
        assert(!ms08_net_migration_completed(&pre, &post, 0));
    }

    /* The 118-u64 V5 wire keeps V4 as its byte-for-byte prefix. */
    {
        struct ms08_net_v5 s = v5_fixture();
        uint64_t wire[MS08_NET_V5_WIRE_U64];
        ms08_net_v5_to_wire(&s, wire);
        assert(wire[87] == 16); /* configured_harts */
        assert(wire[103] == 0 && wire[104] == 0); /* migration views idle */
        struct ms08_net_v5 back;
        assert(ms08_net_v5_from_wire(wire, &back) == 0);
        assert(memcmp(&s, &back, sizeof(s)) == 0);
        wire[72] = 0; /* current_valid corrupted */
        assert(ms08_net_v5_from_wire(wire, &back) != 0);
    }

    /* Peer frame codec: strict case/seq grammar shared with the peer script. */
    {
        char buf[96];
        unsigned long seq;
        char casename[32];
        assert(ms08_net_frame_encode("tcp-bidirectional", 3, buf, sizeof(buf)) == 0);
        assert(ms08_net_frame_decode(buf, casename, sizeof(casename), &seq) == 0);
        assert(seq == 3 && strcmp(casename, "tcp-bidirectional") == 0);
        assert(ms08_net_frame_decode("case=tcp-bidirectional seq=0", casename,
                                     sizeof(casename), &seq) != 0);
        assert(ms08_net_frame_decode("case=unknown seq=1", casename,
                                     sizeof(casename), &seq) != 0);
        assert(ms08_net_frame_decode("case=tcp-bidirectional seq=1 extra=2",
                                     casename, sizeof(casename), &seq) != 0);
    }

    /* Diagnostic contract (5.2-R3): hold lease must be within 1..=2000 ms and
     * the release op is the kernel's OP_RELEASE (3), never op 0.  The Cycle
     * 000 values (30000 ms lease, release op 0) are rejected here. */
    {
        uint64_t payload[2];
        assert(MS08_NET_DIAG_RELEASE == 3u);
        assert(MS08_NET_DIAG_MAX_LEASE_MS == 2000u);
        assert(ms08_net_diag_hold_payload(payload, 30000u) != 0);
        assert(ms08_net_diag_hold_payload(payload, 0u) != 0);
        assert(ms08_net_diag_hold_payload(payload, 2001u) != 0);
        assert(ms08_net_diag_hold_payload(payload, 2000u) == 0);
        assert(payload[0] == MS08_NET_DIAG_HOLD_SUBMIT && payload[1] == 2000u);
        assert(ms08_net_diag_hold_payload(payload, 1u) == 0);
        assert(ms08_net_diag_release_payload(payload) == 0);
        assert(payload[0] == 3u && payload[1] == 0u);
    }

    /* Timer-disabled wake baton (5.2-R1): the guest must reach TRIGGER from a
     * non-target hart through the queue-order baton, with no Armed polling and
     * no retry.  The Cycle 000 trace (wait-Armed, forked retry trigger) is
     * rejected; only the exact single-thread baton is accepted. */
    {
        static const uint8_t old_trace[] = {
            MS08_NET_BATON_WAIT_ARMED, MS08_NET_BATON_RETRY,
            MS08_NET_BATON_TRIGGER,
        };
        static const uint8_t no_hop_trace[] = {
            MS08_NET_BATON_TRIGGER, MS08_NET_BATON_HOP_TARGET,
        };
        static const uint8_t double_trigger[] = {
            MS08_NET_BATON_HOP_TARGET, MS08_NET_BATON_HOP_CONTROLLER,
            MS08_NET_BATON_TRIGGER, MS08_NET_BATON_TRIGGER,
            MS08_NET_BATON_HOP_TARGET,
        };
        static const uint8_t baton[] = {
            MS08_NET_BATON_HOP_TARGET, MS08_NET_BATON_HOP_CONTROLLER,
            MS08_NET_BATON_TRIGGER, MS08_NET_BATON_HOP_TARGET,
        };
        assert(!ms08_net_wake_trace_ok(old_trace, sizeof(old_trace)));
        assert(!ms08_net_wake_trace_ok(no_hop_trace, sizeof(no_hop_trace)));
        assert(!ms08_net_wake_trace_ok(double_trigger, sizeof(double_trigger)));
        assert(!ms08_net_wake_trace_ok(baton, sizeof(baton) - 1));
        assert(ms08_net_wake_trace_ok(baton, sizeof(baton)));
        assert(!ms08_net_wake_trace_ok(NULL, 0));
    }

    /* Nonblocking TCP connect verdict (5.2-R2): EINPROGRESS is only a
     * success after writable readiness plus a fetched SO_ERROR of zero. */
    {
        assert(ms08_net_tcp_connect_done(0, 0, 0, 0));
        assert(!ms08_net_tcp_connect_done(-1, EINPROGRESS, 0, 0));
        assert(ms08_net_tcp_connect_done(-1, EINPROGRESS, 1, 0));
        assert(!ms08_net_tcp_connect_done(-1, EINPROGRESS, 1, ECONNREFUSED));
        assert(!ms08_net_tcp_connect_done(-1, ECONNREFUSED, 0, 0));
    }

    /* TCP stream reassembly (5.2-R2): frames are newline-terminated inside a
     * byte stream, so split and coalesced segments must both reassemble. */
    {
        struct ms08_net_stream_rx rx;
        char frame[96];
        ms08_net_stream_rx_reset(&rx);
        assert(ms08_net_stream_rx_feed(&rx, "case=tcp-bidirectional seq=1",
                                       strlen("case=tcp-bidirectional seq=1"),
                                       frame, sizeof(frame)) == 0);
        assert(ms08_net_stream_rx_feed(&rx, "\ncase=tcp-bidirectional",
                                       strlen("\ncase=tcp-bidirectional"),
                                       frame, sizeof(frame)) == 1);
        assert(strcmp(frame, "case=tcp-bidirectional seq=1") == 0);
        assert(ms08_net_stream_rx_feed(&rx, " seq=2\n", strlen(" seq=2\n"),
                                       frame, sizeof(frame)) == 1);
        assert(strcmp(frame, "case=tcp-bidirectional seq=2") == 0);
        assert(ms08_net_stream_rx_feed(&rx, "", 0, frame, sizeof(frame)) == 0);
        /* Overflow is a hard protocol failure, not a truncation. */
        ms08_net_stream_rx_reset(&rx);
        char junk[sizeof(rx.buf) + 8];
        memset(junk, 'x', sizeof(junk));
        assert(ms08_net_stream_rx_feed(&rx, junk, sizeof(junk),
                                       frame, sizeof(frame)) != 0);
    }

    /* HMP operator completion line (5.2-R5): link-up advances only after the
     * explicit operator line, accepted in exactly one grammar. */
    {
        int link_on = -1;
        assert(ms08_net_hmp_done_parse("MS08_NET_HMP_DONE link=on", &link_on) == 1);
        assert(link_on == 1);
        assert(ms08_net_hmp_done_parse("MS08_NET_HMP_DONE link=off", &link_on) == 1);
        assert(link_on == 0);
        assert(ms08_net_hmp_done_parse("kernel noise", &link_on) == 0);
        assert(ms08_net_hmp_done_parse("MS08_NET_HMP_DONE link=up", &link_on) != 0);
        assert(ms08_net_hmp_done_parse("MS08_NET_HMP_DONE link=on extra=1",
                                       &link_on) != 0);
        assert(ms08_net_hmp_done_parse("MS08_NET_HMP_DONE", &link_on) != 0);
    }

    /* Concurrent migration record (5.2-R4): the Widened view must be sampled
     * while the control ioctl is still outstanding, the control caller must
     * complete successfully, and the peer/owner ledger must stay continuous.
     * The Cycle 000 sequential caller (mid sampled after the ioctl returned)
     * is rejected. */
    {
        assert(ms08_net_migration_record_ok(1, 1, 1, 1, 1, 1));
        assert(!ms08_net_migration_record_ok(1, 1, 0, 1, 1, 1)); /* sequential */
        assert(!ms08_net_migration_record_ok(1, 0, 0, 1, 1, 1)); /* no mid */
        assert(!ms08_net_migration_record_ok(1, 1, 1, 1, 0, 1)); /* control failed */
        assert(!ms08_net_migration_record_ok(0, 1, 1, 1, 1, 1)); /* not idle pre */
        assert(!ms08_net_migration_record_ok(1, 1, 1, 0, 1, 1)); /* never restored */
    }

    /* Stream frame classification (Review finding 2): every received byte
     * must be validated — the expected frame is accepted, a well-formed but
     * unexpected frame (duplicate/late/wrong case) is rejected as data, and
     * malformed text fails closed.  The runtime path must never discard
     * readable bytes unexamined. */
    {
        assert(ms08_net_stream_frame_classify("case=tcp-bidirectional seq=3",
                                              "tcp-bidirectional", 3) == 0);
        assert(ms08_net_stream_frame_classify("case=tcp-bidirectional seq=2",
                                              "tcp-bidirectional", 3) == 1);
        assert(ms08_net_stream_frame_classify("case=udp-bidirectional seq=3",
                                              "tcp-bidirectional", 3) == 1);
        assert(ms08_net_stream_frame_classify("garbage seq=3",
                                              "tcp-bidirectional", 3) != 0);
    }

    /* Post-exchange reassembly audit (Review finding 4): the peer grammar is
     * exactly one echo frame per request, so once the expected frame is
     * accepted the shared buffer must hold nothing else — a coalesced
     * duplicate after the final frame and any partial tail are unaccounted
     * data and fail closed before the exchange is claimed. */
    {
        struct ms08_net_stream_rx rx;
        char frame[96];
        const char *exact = "case=tcp-bidirectional seq=1\n";

        ms08_net_stream_rx_reset(&rx);
        assert(ms08_net_stream_rx_feed(&rx, exact, strlen(exact), frame,
                                       sizeof(frame)) == 1);
        assert(strcmp(frame, "case=tcp-bidirectional seq=1") == 0);
        assert(ms08_net_stream_rx_audit(&rx) == 0); /* exact exchange */

        ms08_net_stream_rx_reset(&rx);
        assert(ms08_net_stream_rx_feed(&rx, "case=tcp-bidirectional seq=1\n"
                                           "case=tcp-bidirectional seq=1\n",
                                       strlen(exact) + strlen(exact), frame,
                                       sizeof(frame)) == 1);
        assert(ms08_net_stream_rx_audit(&rx) != 0); /* coalesced duplicate */

        ms08_net_stream_rx_reset(&rx);
        assert(ms08_net_stream_rx_feed(&rx, "case=tcp-bidirectional seq=1\n"
                                           "case=",
                                       strlen(exact) + strlen("case="), frame,
                                       sizeof(frame)) == 1);
        assert(ms08_net_stream_rx_audit(&rx) != 0); /* partial tail */

        ms08_net_stream_rx_reset(&rx);
        assert(ms08_net_stream_rx_audit(&rx) == 0); /* empty buffer */
        assert(ms08_net_stream_rx_audit(NULL) != 0);
    }

    /* Bounded control-caller cleanup (Review finding 3): a child that keeps
     * the completion pipe write end open — e.g. still inside the migration
     * ioctl — must not make reap block in read past the deadline.  On the
     * pre-repair blocking-pipe path this witness hangs (run it under
     * timeout) instead of finishing. */
    {
        int fds[2];
        uint64_t start = 0, end = 0;
        pid_t pid;

        assert(pipe(fds) == 0);
        pid = fork();
        assert(pid >= 0);
        if (pid == 0) {
            close(fds[0]);
            for (;;)
                pause(); /* write end stays open, child never exits */
        }
        close(fds[1]);
        assert(now_ms(&start) == 0);
        assert(reap_control_caller(pid, fds[0], start + 200) == -1);
        assert(now_ms(&end) == 0);
        assert(end - start < 2000); /* bounded: no indefinite read block */
        /* Terminal cleanup is the helper's own contract (5.2-R4/5.4-R1): a
         * child still alive at the deadline must be terminated and reaped
         * by reap_control_caller itself — no fixture-side kill may mask a
         * missing production cleanup. */
        errno = 0;
        assert(kill(pid, 0) == -1 && errno == ESRCH);

        /* A control caller that completed and closed the write end is
         * drained to EOF and reaped normally. */
        assert(pipe(fds) == 0);
        pid = fork();
        assert(pid >= 0);
        if (pid == 0) {
            char byte = 'K';
            close(fds[0]);
            if (write(fds[1], &byte, 1) != 1) _exit(1);
            _exit(0);
        }
        close(fds[1]);
        assert(now_ms(&start) == 0);
        assert(reap_control_caller(pid, fds[0], start + 5000) == 0);
        errno = 0;
        assert(kill(pid, 0) == -1 && errno == ESRCH);
    }

    /* Affinity baton syscall-return predicate (Iteration 005, 6.2): a
     * successful sched_getaffinity returns the mask size (> 0), never 0, and
     * sched_setaffinity returns 0.  Only a negative return is a failure; the
     * prior `!= 0` test misread a successful get as `affinity-read` FAIL. */
    {
        assert(ms08_net_syscall_return_ok(0));   /* set: success (0) */
        assert(ms08_net_syscall_return_ok(2));   /* get: mask size in bytes */
        assert(ms08_net_syscall_return_ok(8));
        assert(!ms08_net_syscall_return_ok(-1)); /* EPERM etc. */
        assert(!ms08_net_syscall_return_ok(-22));
        assert(ms08_net_syscall_return_ok(16));
    }

    return 0;
}
