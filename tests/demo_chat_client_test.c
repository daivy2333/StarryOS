/*
 * Host test harness for the demo chat client decision core.
 *
 * Compiled on the host with `#include "demo_chat_client.c"` behind the
 * DEMO_CHAT_CLIENT_TESTING guard, so the decision functions run natively
 * without a guest. Covers line classification (EMPTY/QUIT/TEXT), local-input
 * actions (SKIP/SEND/QUIT), remote EOF/poll-trouble -> peer-closed and the
 * QUIT/PEER_CLOSED exit codes.
 *
 * Build & run:
 *   cc -std=c11 -Wall -Wextra -Werror tests/demo_chat_client_test.c \
 *     -o /tmp/demo-chat-client-test && /tmp/demo-chat-client-test
 */

#define DEMO_CHAT_CLIENT_TESTING
#include "demo_chat_client.c"

#include <assert.h>

static void test_classify_lines(void)
{
    assert(chat_classify_line("") == CHAT_LINE_EMPTY);
    assert(chat_classify_line("   ") == CHAT_LINE_EMPTY);
    assert(chat_classify_line("  \t ") == CHAT_LINE_EMPTY);
    assert(chat_classify_line("/quit") == CHAT_LINE_QUIT);
    assert(chat_classify_line(" /quit ") == CHAT_LINE_QUIT);
    assert(chat_classify_line("hello") == CHAT_LINE_TEXT);
    assert(chat_classify_line("a b c") == CHAT_LINE_TEXT);
    assert(chat_classify_line("/foo") == CHAT_LINE_TEXT); /* only /quit */
}

static void test_local_actions(void)
{
    assert(chat_local_input_action(CHAT_LINE_EMPTY) == CHAT_ACTION_SKIP);
    assert(chat_local_input_action(CHAT_LINE_QUIT) == CHAT_ACTION_QUIT);
    assert(chat_local_input_action(CHAT_LINE_TEXT) == CHAT_ACTION_SEND);
}

static void test_remote_eof_action(void)
{
    assert(chat_remote_eof_action() == CHAT_ACTION_PEER_CLOSED);
    assert(chat_poll_trouble(0) == 0);
    assert(chat_poll_trouble(POLLERR) != 0);
    assert(chat_poll_trouble(POLLHUP) != 0);
    assert(chat_poll_trouble(POLLNVAL) != 0);
    assert(chat_poll_trouble(POLLIN) == 0);
    assert(chat_poll_trouble(POLLERR | POLLIN) != 0);
}

static void test_exit_codes(void)
{
    assert(chat_exit_code(CHAT_ACTION_QUIT) == 0);
    assert(chat_exit_code(CHAT_ACTION_PEER_CLOSED) == 1);
}

int main(void)
{
    test_classify_lines();
    test_local_actions();
    test_remote_eof_action();
    test_exit_codes();
    printf("demo_chat_client_test: all PASS\n");
    return 0;
}