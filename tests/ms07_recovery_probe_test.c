#define MS07_RECOVERY_PROBE_TESTING
#include "ms07_recovery_probe.c"

#include <assert.h>

int main(void) {
    struct ms07_v4_observation before = {
        .current_valid = 1, .current_queue_epoch = 7,
        .current_socket_epoch = 11, .current_link_generation = 13,
        .current_link_state = MS07_LINK_UP, .owner_quarantined = 0,
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
    return ms07_probe_decision_core_self_test() ? 0 : 1;
}
