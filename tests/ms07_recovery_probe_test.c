#define MS07_RECOVERY_PROBE_TESTING
#include "ms07_recovery_probe.c"

#include <assert.h>

static struct ms07_v4_observation drain_fixture(uint64_t q, uint64_t s)
{
    struct ms07_v4_observation obs = {
        .lifecycle = 2, .current_valid = 1,
        .current_queue_epoch = q, .current_socket_epoch = s,
        .current_link_generation = 13, .current_link_state = MS07_LINK_UP,
        .owner_available = MS07_OWNER_SLOTS,
        .owner_device_owned = MS07_OWNER_SLOTS, .owner_quarantined = 0,
    };
    return obs;
}

int main(void) {
    struct ms07_v4_observation before = {
        .lifecycle = 2, .current_valid = 1,
        .current_queue_epoch = 7, .current_socket_epoch = 11,
        .current_link_generation = 13, .current_link_state = MS07_LINK_UP,
        .owner_quarantined = 0,
    };
    struct ms07_v4_observation after = before;
    after.current_queue_epoch = 8;
    after.current_socket_epoch = 12;
    assert(ms07_reset_transition_valid(&before, &after));
    after.current_queue_epoch = 9;
    assert(!ms07_reset_transition_valid(&before, &after));
    after = before;
    after.current_link_generation = 14;
    after.current_link_state = MS07_LINK_DOWN;
    assert(ms07_link_down_transition_valid(&before, &after));
    assert(ms07_terminal_errno_valid(MS07_TERMINAL_RESET, ECONNRESET));
    assert(ms07_terminal_errno_valid(MS07_TERMINAL_LINK_DOWN, ENOTCONN));
    assert(ms07_deadline_expired(10, 20, 10));
    assert(!ms07_deadline_expired(10, 19, 10));
    assert(ms07_deadline_expired(20, 19, 10)); /* now before start: fail-closed, never spins */

    /* A3: a non-Active sample breaks adjacency. Active A, Resetting, Active A
     * therefore needs one more Active A before it can become stable. */
    {
        struct ms07_v4_observation candidate = {0};
        struct ms07_v4_observation active = drain_fixture(8, 12);
        struct ms07_v4_observation resetting = active;
        int have_candidate = 0;
        resetting.lifecycle = 6;
        assert(ms07_stable_candidate_step(&candidate, &active, &have_candidate) == 0);
        assert(have_candidate);
        assert(ms07_stable_candidate_step(&candidate, &resetting, &have_candidate) == 0);
        assert(!have_candidate);
        assert(ms07_stable_candidate_step(&candidate, &active, &have_candidate) == 0);
        assert(ms07_stable_candidate_step(&candidate, &active, &have_candidate) == 1);
    }

    /* Every I/O operation consumes one shared absolute deadline. */
    {
        int remaining = -1;
        assert(ms07_deadline_remaining(100, 120, &remaining) == 0 && remaining == 20);
        assert(ms07_deadline_remaining(120, 120, &remaining) == -1);
        assert(ms07_deadline_remaining(121, 120, &remaining) == -1);
    }

    /* A3: a wait token observed at or after the deadline is stale, and
     * two terminal waits sharing one deadline must fail once the budget is
     * exhausted between them.  `ms07_wait_step` is the same decision `wait_fd`
     * uses, so these drive the production timeout rule under a fake clock. */
    {
        uint64_t deadline = 100;
        int again, st;
        /* A wake exactly at the deadline (>=) is not accepted (late-readable
         * must not succeed). */
        again = 1; st = ms07_wait_step(1, POLLIN, POLLIN, 101, deadline, &again);
        assert(st == 0 && again == 0);
        /* Two-terminal exhaustion: first terminal consumes most of the budget;
         * the second terminal's wake at/after deadline must be rejected. */
        again = 1; st = ms07_wait_step(1, POLLHUP, POLLIN | POLLERR | POLLHUP, 80, deadline, &again);
        assert(st == 1 && again == 1);
        again = 1; st = ms07_wait_step(1, POLLHUP, POLLIN | POLLERR | POLLHUP, deadline, deadline, &again);
        assert(st == 0 && again == 0);
        /* A wanted-but-not-yet-readable wake re-polls within budget. */
        again = 1; st = ms07_wait_step(1, POLLIN, POLLHUP, 80, deadline, &again);
        assert(st == 0 && again == 1);
    }

    /* A3: the post-wait I/O boundary.  A wait that returns success at or after
     * the absolute deadline must not be followed by a `send`/`recv` that
     * consumes the budget; `ms07_io_allowed` is the pure decision the probe
     * applies between wait-return and the untrusted syscall. */
    {
        uint64_t deadline = 100;
        assert(ms07_io_allowed(99, deadline));  /* within budget: I/O permitted */
        assert(!ms07_io_allowed(100, deadline)); /* at the deadline: refused */
        assert(!ms07_io_allowed(101, deadline)); /* crossed: refused */
        assert(!ms07_io_allowed(deadline, deadline));
    }

    /* P6 / R8: nonblocking UDP send readiness.  A post-POLLOUT send may
     * legitimately return EAGAIN/EWOULDBLOCK; that must re-enter the wait
     * within the SAME absolute deadline instead of failing the phase.  EINTR
     * retries, any other errno and a short write stop; a full datagram is
     * success.  `ms07_send_step` is the same decision `peer_exchange` uses,
     * so these drive the production nonblocking rule under a fake clock. */
    {
        uint64_t deadline = 100;
        int again = 0;
        /* Full datagram sent: success. */
        assert(ms07_send_step((ssize_t)4, 0, 4, 90, deadline, &again) == 1);
        /* EAGAIN within budget: retry allowed (re-enter wait, same deadline). */
        again = 1;
        assert(ms07_send_step((ssize_t)-1, EAGAIN, 4, 90, deadline, &again) == 0);
        assert(again == 1);
        /* EAGAIN at/after deadline: must not arm a retry that would consume
         * budget and never pass the deadline. */
        again = 1;
        assert(ms07_send_step((ssize_t)-1, EAGAIN, 4, 101, deadline, &again) == 0);
        assert(again == 0);
        /* EWOULDBLOCK treated identically to EAGAIN. */
        again = 1;
        assert(ms07_send_step((ssize_t)-1, EWOULDBLOCK, 4, 95, deadline, &again) == 0);
        assert(again == 1);
        /* EINTR: retry within budget. */
        again = 1;
        assert(ms07_send_step((ssize_t)-1, EINTR, 4, 98, deadline, &again) == 0);
        assert(again == 1);
        /* Other errno (e.g. ENOTCONN) is terminal, stage/errno kept by caller. */
        assert(ms07_send_step((ssize_t)-1, ENOTCONN, 4, 90, deadline, &again) == -1);
        /* Short write is terminal; never auto-resend a partial datagram. */
        assert(ms07_send_step((ssize_t)2, 0, 4, 90, deadline, &again) == -1);
    }

    /* A5: the link-down conservation baseline.  A link flap does not own or
     * release packet slots, so `available` must be conserved ACROSS the down
     * transition.  The reset snapshot still holds the transient in-flight slot
     * (available=63), so it must NOT be the baseline; the fresh new-epoch
     * drained observation (available=64) is.  This pins
     * `reset.available != fresh.available == down.available`. */
    {
        struct ms07_v4_observation reset = {
            .lifecycle = 2, .current_valid = 1, .current_queue_epoch = 8,
            .current_socket_epoch = 12, .current_link_generation = 13,
            .current_link_state = MS07_LINK_UP, .owner_available = 63,
            .owner_device_owned = MS07_OWNER_SLOTS,
        };
        struct ms07_v4_observation fresh = reset;
        fresh.owner_available = MS07_OWNER_SLOTS;
        struct ms07_v4_observation down = fresh;
        down.current_link_generation = 14;
        down.current_link_state = MS07_LINK_DOWN;
        assert(ms07_link_down_transition_valid(&fresh, &down));
        /* A link flap conserves BOTH owner channels: an available or
         * device_owned drift across the down transition is rejected. */
        struct ms07_v4_observation bad_avail = down;
        bad_avail.owner_available = MS07_OWNER_SLOTS - 1;
        struct ms07_v4_observation bad_dev = down;
        bad_dev.owner_device_owned = MS07_OWNER_SLOTS - 1;
        assert(!ms07_link_down_transition_valid(&fresh, &bad_avail));
        assert(!ms07_link_down_transition_valid(&fresh, &bad_dev));
        /* The reset snapshot (available=63, in-flight slot) must NOT be the
         * down baseline; the fresh new-epoch idle observation is. */
        assert(!ms07_link_down_transition_valid(&reset, &down));
    }

    /* A2/A3 rework: the drained-epoch observer must accept exactly an Active
     * observation at the target queue/socket epoch at the healthy VirtIO owner
     * baseline (available==device_owned==expected, no quarantine).  `expected`
     * is the QS fixed capacity, so a healthy observation with `device_owned==0`
     * (absent RX owners) and a 63/64 or 64/63 imbalance are all rejected.
     * Every single-field mutation must be rejected, which pins the C
     * wire/observation contract the validator audits. */
    {
        struct ms07_v4_observation drained = drain_fixture(9, 12);
        assert(ms07_drained_epoch_ok(&drained, 9, 12, MS07_OWNER_SLOTS));
    }
    {
        struct ms07_v4_observation o = drain_fixture(9, 12);
        o.lifecycle = 0; assert(!ms07_drained_epoch_ok(&o, 9, 12, MS07_OWNER_SLOTS));
        o.lifecycle = 2; o.current_valid = 0; assert(!ms07_drained_epoch_ok(&o, 9, 12, MS07_OWNER_SLOTS));
        o.current_valid = 1; o.current_queue_epoch = 10; assert(!ms07_drained_epoch_ok(&o, 9, 12, MS07_OWNER_SLOTS));
        o.current_queue_epoch = 9; o.current_socket_epoch = 13; assert(!ms07_drained_epoch_ok(&o, 9, 12, MS07_OWNER_SLOTS));
        o.current_socket_epoch = 12; o.owner_available = 0; assert(!ms07_drained_epoch_ok(&o, 9, 12, MS07_OWNER_SLOTS));
        o.owner_available = MS07_OWNER_SLOTS; o.owner_device_owned = 0; assert(!ms07_drained_epoch_ok(&o, 9, 12, MS07_OWNER_SLOTS));
        o.owner_device_owned = MS07_OWNER_SLOTS - 1; assert(!ms07_drained_epoch_ok(&o, 9, 12, MS07_OWNER_SLOTS));
        o.owner_device_owned = MS07_OWNER_SLOTS; o.owner_available = MS07_OWNER_SLOTS - 1; assert(!ms07_drained_epoch_ok(&o, 9, 12, MS07_OWNER_SLOTS));
        o.owner_available = MS07_OWNER_SLOTS; o.owner_quarantined = 2; assert(!ms07_drained_epoch_ok(&o, 9, 12, MS07_OWNER_SLOTS));
    }
    return ms07_probe_decision_core_self_test() ? 0 : 1;
}
