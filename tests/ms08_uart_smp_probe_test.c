/* MS08 UART SMP probe host decision test.  The pure transition/payload checks
 * below are shared by the guest payload (tests/ms08_uart_smp_probe.c) and this
 * host harness; the QEMU serial choreography stays in
 * scripts/ms08-uart-serial.py. */
#define MS08_UART_PROBE_TESTING
#include "ms08_uart_smp_probe.c"

#include <assert.h>
#include <signal.h>

static struct ms08_uart_snapshot snap_fixture(void)
{
    struct ms08_uart_snapshot s;
    memset(&s, 0, sizeof(s));
    s.magic = MS08_UART_SNAPSHOT_IOCTL;
    s.configured_harts = 16;
    s.schedulable_mask = 0xffff;
    s.rx_affinity = 1;
    s.tx_affinity = 2;
    s.irq_last_hart = 0;
    s.irq_hart_mask = 0x1;
    s.rx_last_hart = 1;
    s.rx_hart_mask = 0x2;
    s.tx_last_hart = 2;
    s.tx_hart_mask = 0x4;
    s.rx_occupancy = 0;
    s.tx_vacancy = 4096;
    s.ring_empty = 1;
    s.copier_active = 0;
    s.staged_bytes = 0;
    s.transmitter_empty = 1;
    s.irq_events = 10;
    s.rx_polls = 20;
    s.tx_polls = 30;
    s.ipi_sent = 4;
    s.ipi_received = 4;
    s.affinity_rejects = 0;
    s.rx_migration_state = 0;
    s.tx_migration_state = 0;
    return s;
}

struct ms08_test_lines {
    char buf[4][MS08_UART_LINE_CAP + 1];
    size_t count;
};

static int ms08_test_collect_line(void *vctx, const char *line, size_t len)
{
    struct ms08_test_lines *t = vctx;
    if (t->count >= 4 || len > MS08_UART_LINE_CAP) return -1;
    memcpy(t->buf[t->count], line, len);
    t->buf[t->count][len] = '\0';
    t->count += 1;
    return 0;
}

int main(void)
{
    char line[128];
    unsigned long seq;
    char payload[64];
    size_t len;

    /* Case registry is frozen, ordered and matches the schema contract. */
    assert(ms08_uart_case_count() == 8);
    assert(strcmp(ms08_uart_cases[0], "placement") == 0);
    assert(strcmp(ms08_uart_cases[1], "rx") == 0);
    assert(strcmp(ms08_uart_cases[2], "tx-full-recovery") == 0);
    assert(strcmp(ms08_uart_cases[3], "readiness") == 0);
    assert(strcmp(ms08_uart_cases[4], "tcdrain") == 0);
    assert(strcmp(ms08_uart_cases[5], "quiet") == 0);
    assert(strcmp(ms08_uart_cases[6], "rx-migration") == 0);
    assert(strcmp(ms08_uart_cases[7], "tx-migration") == 0);
    assert(ms08_uart_schema_count() == 8);
    assert(strcmp(ms08_uart_schema[6],
                  "rx-migration:MS08_UART_SNAP,MS08_UART_NEED,MS08_UART_RX,MS08UARTECHO,MS08_UART_SNAP,MS08_UART_SNAP") == 0);
    assert(ms08_uart_probe_self_test());

    /* Injected RX frames are numbered, length-prefixed ASCII; decode fails
     * closed on malformed text, length drift and out-of-range sequence. */
    assert(ms08_uart_frame_encode(3, "hello", 5, line, sizeof(line)) == 0);
    assert(ms08_uart_frame_decode(line, &seq, payload, sizeof(payload), &len) == 0);
    assert(seq == 3 && len == 5 && memcmp(payload, "hello", 5) == 0);
    assert(ms08_uart_frame_decode("MS08RX seq=1 len=5 hello", &seq, payload,
                                  sizeof(payload), &len) == 0);
    assert(ms08_uart_frame_decode("MS08RX seq=0 len=5 hello", &seq, payload,
                                  sizeof(payload), &len) != 0); /* seq starts at 1 */
    assert(ms08_uart_frame_decode("MS08RX seq=1 len=9 hello", &seq, payload,
                                  sizeof(payload), &len) != 0); /* length drift */
    assert(ms08_uart_frame_decode("MS08RX seq=1 len=5 hell", &seq, payload,
                                  sizeof(payload), &len) != 0); /* short bytes */
    assert(ms08_uart_frame_decode("MS08RX seq=1 len=5 hello extra", &seq, payload,
                                  sizeof(payload), &len) != 0); /* trailing junk */
    assert(ms08_uart_frame_decode("MS08RX seq=x len=5 hello", &seq, payload,
                                  sizeof(payload), &len) != 0);
    assert(ms08_uart_frame_decode("garbage", &seq, payload, sizeof(payload), &len) != 0);

    /* The RX sequence ledger accepts exactly the strictly-increasing stream. */
    {
        struct ms08_uart_seq_ledger ledger;
        ms08_uart_seq_ledger_reset(&ledger);
        assert(ms08_uart_seq_ledger_accept(&ledger, 1));
        assert(ms08_uart_seq_ledger_accept(&ledger, 2));
        assert(!ms08_uart_seq_ledger_accept(&ledger, 2)); /* duplicate */
        assert(!ms08_uart_seq_ledger_accept(&ledger, 4)); /* gap */
        assert(ledger.next == 3);
    }

    /* Placement: distinct schedulable affinities that agree with the actual
     * poll harts, with attributable progress and history masks that contain
     * the pin and never record an out-of-schedulable bit. */
    {
        struct ms08_uart_snapshot s = snap_fixture();
        assert(ms08_uart_placement_ok(&s));
        s.rx_affinity = 2; /* RX == TX hart: policy violation */
        assert(!ms08_uart_placement_ok(&s));
        s.rx_affinity = 17; /* outside the schedulable mask */
        assert(!ms08_uart_placement_ok(&s));
        s.rx_affinity = 1;
        s.schedulable_mask = 0x2; /* affinity not schedulable */
        assert(!ms08_uart_placement_ok(&s));
        s.schedulable_mask = 0xffff;
        s.configured_harts = 0; /* degenerate topology */
        assert(!ms08_uart_placement_ok(&s));
    }

    /* Placement failure on actual-hart drift and drift-history bits (RED:
     * the predicate could not accept affinity alone or an unknown mask bit as
     * fixed placement; Iteration 005, task 6.1). */
    {
        struct ms08_uart_snapshot s = snap_fixture();
        s.rx_last_hart = s.rx_affinity + 2; /* claimed but never polled here */
        assert(!ms08_uart_placement_ok(&s));
        s.rx_last_hart = (int32_t)s.rx_affinity;
        s.tx_last_hart = s.tx_affinity + 2;
        assert(!ms08_uart_placement_ok(&s));
        s.tx_last_hart = (int32_t)s.tx_affinity;
        s.rx_polls = 0; /* no attributable RX progress */
        assert(!ms08_uart_placement_ok(&s));
        s.rx_polls = 20;
        s.tx_polls = 0;
        assert(!ms08_uart_placement_ok(&s));
        s.tx_polls = 30;
        s.rx_hart_mask |= (uint64_t)1 << 40; /* unknown out-of-schedulable bit */
        assert(!ms08_uart_placement_ok(&s));
        s.rx_hart_mask = 0x2;
        s.tx_hart_mask |= (uint64_t)1 << 16;
        assert(!ms08_uart_placement_ok(&s));
        s.tx_hart_mask = 0x4;
        /* A pin missing from its own history mask is rejected. */
        s.rx_hart_mask &= ~((uint64_t)1 << s.rx_affinity);
        assert(!ms08_uart_placement_ok(&s));
        s.rx_hart_mask = 0x2;
        s.tx_hart_mask &= ~((uint64_t)1 << s.tx_affinity);
        assert(!ms08_uart_placement_ok(&s));
    }

    /* Post-boot migration history (Cycle 002, 6.1-R1): a history bit is legal
     * only when the current pin or this copier's own recorded migration
     * explains it; a recorded hart must still be schedulable to count. */
    {
        struct ms08_uart_snapshot s = snap_fixture();
        /* Extra schedulable bits without a migration record are rejected. */
        s.rx_hart_mask = 0x2 | 0x8;   /* unrecorded hart 3 */
        s.tx_hart_mask = 0x4 | 0x10;  /* unrecorded hart 4 */
        assert(!ms08_uart_placement_ok(&s));
        /* Recorded boot migrations (Restored) explain the same bits. */
        s.rx_migration_state = 2;
        s.rx_migration_from = 1;
        s.rx_migration_to = 3;
        s.tx_migration_state = 2;
        s.tx_migration_from = 2;
        s.tx_migration_to = 4;
        assert(ms08_uart_placement_ok(&s));
        /* A recorded hart outside the schedulable set explains nothing. */
        s.rx_migration_to = 17; /* outside the 0xffff fixture mask */
        s.rx_hart_mask = 0x2 | ((uint64_t)1 << 17);
        assert(!ms08_uart_placement_ok(&s));
    }

    /* Four-stage drain: all four completion stages must be settled. */
    {
        struct ms08_uart_snapshot s = snap_fixture();
        assert(ms08_uart_drain_ok(&s));
        s.copier_active = 1;
        assert(!ms08_uart_drain_ok(&s));
        s.copier_active = 0;
        s.staged_bytes = 1;
        assert(!ms08_uart_drain_ok(&s));
        s.staged_bytes = 0;
        s.transmitter_empty = 0;
        assert(!ms08_uart_drain_ok(&s));
    }

    /* Quiet: no attributable progress may occur inside a quiet window. */
    {
        struct ms08_uart_snapshot a = snap_fixture(), b = a;
        assert(ms08_uart_quiet_ok(&a, &b));
        b.irq_events = 11;
        assert(!ms08_uart_quiet_ok(&a, &b));
        b.irq_events = 10;
        b.rx_polls = 21;
        assert(!ms08_uart_quiet_ok(&a, &b));
        b.rx_polls = 20;
        b.ipi_sent = 5;
        assert(!ms08_uart_quiet_ok(&a, &b));
    }

    /* TX Full -> recovery: the ring must actually reach vacancy zero and the
     * post-drain snapshot must be fully settled with progress counters moved. */
    {
        struct ms08_uart_snapshot before = snap_fixture(), after = before;
        before.tx_vacancy = 0;
        before.copier_active = 1;
        before.ring_empty = 0;
        before.transmitter_empty = 0;
        before.tx_polls = 40;
        assert(ms08_uart_full_observed(&before));
        before.tx_vacancy = 1; /* never actually filled */
        assert(!ms08_uart_full_observed(&before));
        before.tx_vacancy = 0;
        after.tx_polls = 50; /* the drain made the copier progress */
        assert(ms08_uart_full_recovered(&before, &after));
        after.staged_bytes = 2; /* still unsettled after "recovery" */
        assert(!ms08_uart_full_recovered(&before, &after));
        after.staged_bytes = 0;
        after.tx_polls = before.tx_polls; /* no copier progress at all */
        assert(!ms08_uart_full_recovered(&before, &after));
    }

    /* Controlled RX/TX copier migration: Widened must move the observed copier
     * hart to the announced second hart with poll progress, and Restored must
     * land back on the original singleton with identity counters intact. */
    {
        struct ms08_uart_snapshot pre = snap_fixture(), mid = pre, post = pre;
        mid.rx_migration_state = 1; /* Widened */
        mid.rx_migration_from = 1;
        mid.rx_migration_to = 3;
        mid.rx_last_hart = 3;
        mid.rx_hart_mask = 0x2 | 0x8;
        mid.rx_polls = pre.rx_polls + 5;
        mid.rx_migration_requested_polls = pre.rx_polls;
        mid.rx_migration_observed_polls = mid.rx_polls;
        assert(ms08_uart_migration_widened(&pre, &mid, 0));
        mid.rx_last_hart = 1; /* copier never actually moved */
        assert(!ms08_uart_migration_widened(&pre, &mid, 0));
        mid.rx_last_hart = 3;
        mid.rx_polls = pre.rx_polls; /* no poll progress on the second hart */
        assert(!ms08_uart_migration_widened(&pre, &mid, 0));
        mid.rx_polls = pre.rx_polls + 5;

        post.rx_migration_state = 2; /* Restored */
        post.rx_migration_from = 1;
        post.rx_migration_to = 3;
        post.rx_last_hart = 1;
        post.rx_polls = mid.rx_polls + 2;
        assert(ms08_uart_migration_restored(&mid, &post, 0));
        post.rx_affinity = 3; /* singleton not restored */
        assert(!ms08_uart_migration_restored(&mid, &post, 0));
        post.rx_affinity = 1;
        post.rx_polls = mid.rx_polls; /* no further progress */
        assert(!ms08_uart_migration_restored(&mid, &post, 0));

        /* TX direction discriminates the fields under check. */
        struct ms08_uart_snapshot tpre = snap_fixture(), tmid = tpre;
        tmid.tx_migration_state = 1;
        tmid.tx_migration_from = 2;
        tmid.tx_migration_to = 5;
        tmid.tx_last_hart = 5;
        tmid.tx_hart_mask = 0x4 | 0x20;
        tmid.tx_polls = tpre.tx_polls + 7;
        tmid.tx_migration_requested_polls = tpre.tx_polls;
        tmid.tx_migration_observed_polls = tmid.tx_polls;
        assert(ms08_uart_migration_widened(&tpre, &tmid, 1));
        tmid.rx_last_hart = 5; /* drift on the non-migrated copier */
        assert(!ms08_uart_migration_widened(&tpre, &tmid, 1));
    }

    /* The 224-byte little-endian snapshot wire keeps its declared offsets. */
    {
        struct ms08_uart_snapshot s = snap_fixture();
        uint8_t wire[MS08_UART_WIRE_SIZE];
        ms08_uart_snapshot_to_wire(&s, wire);
        assert(wire[0] == 0x31 && wire[1] == 0x4d && wire[2] == 0x53 &&
               wire[3] == 0x55); /* magic LE == ioctl low bytes */
        assert(wire[36] == 0 && wire[37] == 0 && wire[38] == 0 && wire[39] == 0);
        struct ms08_uart_snapshot back;
        assert(ms08_uart_snapshot_from_wire(wire, &back) == 0);
        assert(memcmp(&s, &back, sizeof(s)) == 0);
        wire[36] = 1; /* reserved alignment byte must stay zero */
        assert(ms08_uart_snapshot_from_wire(wire, &back) != 0);
    }

    /* Concurrent migration record (5.1-R1): direction-neutral contract; the
     * per-direction progress evidence is computed by the caller (RX: drained
     * frame conservation; TX: written bytes + copier poll progress).  The
     * first-cut bug — passing the RX frame count (which stays zero in the TX
     * branch) to the record gate — is rejected by
     * ms08_uart_tx_migration_progress_ok below. */
    {
        assert(ms08_uart_migration_record_ok(1, 1, 1, 1, 1, 1));
        assert(!ms08_uart_migration_record_ok(1, 1, 0, 1, 1, 1)); /* sequential */
        assert(!ms08_uart_migration_record_ok(1, 0, 0, 1, 1, 1)); /* no mid */
        assert(!ms08_uart_migration_record_ok(1, 1, 1, 1, 0, 1)); /* control failed */
        assert(!ms08_uart_migration_record_ok(1, 1, 1, 1, 1, 0)); /* no progress */
        assert(!ms08_uart_migration_record_ok(0, 1, 1, 1, 1, 1)); /* not idle */
        assert(!ms08_uart_migration_record_ok(1, 1, 1, 0, 1, 1)); /* never restored */
    }

    /* TX-direction progress evidence (Review finding 1): real TX bytes were
     * written and the migrated copier's poll counter moved between the
     * pre and post views.  A zero frame count is never TX progress. */
    {
        struct ms08_uart_snapshot pre = snap_fixture(), post = pre;
        post.tx_polls = pre.tx_polls + 9;
        assert(ms08_uart_tx_migration_progress_ok(&pre, &post, 1, 1024));
        assert(!ms08_uart_tx_migration_progress_ok(&pre, &post, 1, 0));
        post.tx_polls = pre.tx_polls;
        assert(!ms08_uart_tx_migration_progress_ok(&pre, &post, 1, 1024));
        post.tx_polls = pre.tx_polls + 9;
        post.tx_last_hart = 5; /* discriminates the TX direction */
        assert(ms08_uart_tx_migration_progress_ok(&pre, &post, 1, 1024));
        {
            struct ms08_uart_snapshot rpre = snap_fixture(), rpost = rpre;
            rpost.rx_polls = rpre.rx_polls + 3;
            assert(ms08_uart_tx_migration_progress_ok(&rpre, &rpost, 0, 16));
        }
    }

    /* Bounded line reader (Iteration 005 task 6.1 + Cycle 002, 6.1-R1):
     * every complete '\n'-terminated frame survives arbitrary read
     * partitioning — coalesced (several frames in one chunk) and split (one
     * frame across many chunks) must both decode once in order; a valid
     * residual plus any allowed 128-byte read chunk must not overflow the
     * current-line buffer; an individual line at/over the capacity boundary
     * fails explicitly, never silently dropping bytes. */
    {
        char payload[MS08_UART_FRAME_MAX_PAYLOAD + 1];
        size_t len;

        /* Coalesced: two frames share one chunk, decoded in order. */
        {
            struct ms08_uart_line_reader r = { .len = 0 };
            struct ms08_test_lines got = { .count = 0 };
            unsigned long seq;
            const char *coalesced = "MS08RX seq=1 len=5 hello\n"
                                    "MS08RX seq=2 len=5 world\n";
            assert(line_reader_push(&r, coalesced, strlen(coalesced),
                                    ms08_test_collect_line, &got) == 0);
            assert(got.count == 2);
            assert(ms08_uart_frame_decode(got.buf[0], &seq, payload,
                                           sizeof(payload), &len) == 0);
            assert(seq == 1 && len == 5 && memcmp(payload, "hello", 5) == 0);
            assert(ms08_uart_frame_decode(got.buf[1], &seq, payload,
                                           sizeof(payload), &len) == 0);
            assert(seq == 2 && len == 5 && memcmp(payload, "world", 5) == 0);
        }
        /* Split: one frame arrives across many chunks, reassembled once. */
        {
            struct ms08_uart_line_reader r = { .len = 0 };
            struct ms08_test_lines got = { .count = 0 };
            unsigned long seq;
            const char *parts[] = { "MS08RX seq=3 ", "len=5 he", "llo\n"
                                    "MS08RX seq=4 len=5 world\n" };
            for (size_t i = 0; i < 3; ++i)
                assert(line_reader_push(&r, parts[i], strlen(parts[i]),
                                        ms08_test_collect_line, &got) == 0);
            assert(got.count == 2);
            assert(ms08_uart_frame_decode(got.buf[0], &seq, payload,
                                           sizeof(payload), &len) == 0);
            assert(seq == 3 && len == 5 && memcmp(payload, "hello", 5) == 0);
            assert(ms08_uart_frame_decode(got.buf[1], &seq, payload,
                                           sizeof(payload), &len) == 0);
            assert(seq == 4 && len == 5 && memcmp(payload, "world", 5) == 0);
        }
        /* An overlong unframed line overflows and fails instead of dropping. */
        {
            struct ms08_uart_line_reader r = { .len = 0 };
            struct ms08_test_lines got = { .count = 0 };
            char big[MS08_UART_LINE_CAP + 8];
            memset(big, 'X', sizeof(big));
            assert(line_reader_push(&r, big, sizeof(big),
                                    ms08_test_collect_line, &got) == -1);
            assert(got.count == 0);
        }
        /* Capacity boundary: a line of exactly MS08_UART_LINE_CAP content
         * bytes is delivered whole; one byte more fails explicitly. */
        {
            struct ms08_uart_line_reader r = { .len = 0 };
            struct ms08_test_lines got = { .count = 0 };
            char exact[MS08_UART_LINE_CAP + 1];
            memset(exact, 'y', MS08_UART_LINE_CAP);
            exact[MS08_UART_LINE_CAP] = '\n';
            assert(line_reader_push(&r, exact, sizeof(exact),
                                    ms08_test_collect_line, &got) == 0);
            assert(got.count == 1);
            assert(strlen(got.buf[0]) == MS08_UART_LINE_CAP);

            struct ms08_uart_line_reader r2 = { .len = 0 };
            struct ms08_test_lines got2 = { .count = 0 };
            char over[MS08_UART_LINE_CAP + 2];
            memset(over, 'y', MS08_UART_LINE_CAP + 1);
            over[MS08_UART_LINE_CAP + 1] = '\n';
            assert(line_reader_push(&r2, over, sizeof(over),
                                     ms08_test_collect_line, &got2) == -1);
            assert(got2.count == 0);
        }
        /* Cross-capacity partition (Cycle 002, 6.1-R1): a valid residual
         * plus the next 128-byte read chunk exceed the old 160-byte
         * aggregate buffer although every individual line is valid;
         * reassembly drains complete lines while consuming the chunk. */
        {
            struct ms08_uart_line_reader r = { .len = 0 };
            struct ms08_test_lines got = { .count = 0 };
            unsigned long seq;
            char l1[80], l2[80], chunk[128];
            size_t l1_len, l2_len, used;

            memset(payload, 'p', MS08_UART_FRAME_MAX_PAYLOAD);
            payload[MS08_UART_FRAME_MAX_PAYLOAD] = '\0';
            assert(ms08_uart_frame_encode(1, payload, MS08_UART_FRAME_MAX_PAYLOAD,
                                          l1, sizeof(l1)) == 0);
            assert(ms08_uart_frame_encode(2, payload, MS08_UART_FRAME_MAX_PAYLOAD,
                                          l2, sizeof(l2)) == 0);
            l1_len = strlen(l1); /* 68 bytes; the wire adds the newline */
            l2_len = strlen(l2);
            assert(l1_len == 68 && l2_len == 68);
            /* Residual: the whole first frame except its newline. */
            assert(line_reader_push(&r, l1, l1_len, ms08_test_collect_line,
                                    &got) == 0);
            assert(got.count == 0);
            /* One read chunk: newline + a second full valid frame + newline +
             * a valid partial tail: 68 + 128 > 160 while every individual
             * line is 69 bytes with its newline. */
            used = 0;
            chunk[used++] = '\n';
            memcpy(chunk + used, l2, l2_len);
            used += l2_len;
            chunk[used++] = '\n';
            while (used < sizeof(chunk))
                chunk[used++] = 'x';
            assert(line_reader_push(&r, chunk, sizeof(chunk),
                                    ms08_test_collect_line, &got) == 0);
            assert(got.count == 2);
            assert(ms08_uart_frame_decode(got.buf[0], &seq, payload,
                                           sizeof(payload), &len) == 0);
            assert(seq == 1 && len == MS08_UART_FRAME_MAX_PAYLOAD);
            assert(ms08_uart_frame_decode(got.buf[1], &seq, payload,
                                           sizeof(payload), &len) == 0);
            assert(seq == 2 && len == MS08_UART_FRAME_MAX_PAYLOAD);
        }
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
        /* Terminal cleanup is the helper's own contract (5.1-R1/5.4-R1): a
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

    return 0;
}
