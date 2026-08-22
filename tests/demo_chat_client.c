#define _DEFAULT_SOURCE
#define _POSIX_C_SOURCE 200809L

/* Demo chat client — guest-side payload and host-testable decision core.
 *
 * Build (host syntax check):
 *   cc -std=c11 -Wall -Wextra -Werror -fsyntax-only tests/demo_chat_client.c
 *
 * Build (RISC-V static, user boundary):
 *   riscv64-linux-musl-gcc -std=c11 -Wall -Wextra -Werror -static -no-pie -Os \
 *     tests/demo_chat_client.c -o tests/demo_chat_client
 *
 * Host decision harness (no guest):
 *   cc -std=c11 -Wall -Wextra -Werror tests/demo_chat_client_test.c \
 *     -o /tmp/demo-chat-client-test && /tmp/demo-chat-client-test
 *
 * Usage (inside QEMU guest shell, after `wget http://10.0.2.2:18765/
 * demo_chat_client`):
 *   ./demo_chat_client [host] [port]
 *
 * Connects to the host chat server, then relays lines between the serial
 * stdin (fd 0) and the socket. Empty lines are dropped; "/quit" closes the
 * connection and exits 0; peer EOF / POLLERR / POLLHUP / POLLNVAL prints
 * DEMO_CHAT_CLIENT_PEER_CLOSED and exits 1.
 */

#include <arpa/inet.h>
#include <errno.h>
#include <netinet/in.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <unistd.h>

#define DEMO_CHAT_DEFAULT_HOST "10.0.2.2"
#define DEMO_CHAT_DEFAULT_PORT 15560
#define DEMO_CHAT_BUFFER_SIZE 1024

/* ── Decision core (host-testable; kept outside the TESTING guard) ────── */

typedef enum {
    CHAT_LINE_EMPTY,
    CHAT_LINE_QUIT,
    CHAT_LINE_TEXT,
} chat_line_kind;

typedef enum {
    CHAT_ACTION_SKIP,
    CHAT_ACTION_SEND,
    CHAT_ACTION_QUIT,
    CHAT_ACTION_PEER_CLOSED,
} chat_action;

static const char *chat_strip(const char *s)
{
    while (*s == ' ' || *s == '\t')
        s++;
    return s;
}

static chat_line_kind chat_classify_line(const char *line)
{
    const char *start;
    const char *end;
    size_t len;

    if (line == NULL)
        return CHAT_LINE_EMPTY;
    start = chat_strip(line);
    if (*start == '\0')
        return CHAT_LINE_EMPTY;
    end = start + strlen(start);
    while (end > start && (end[-1] == ' ' || end[-1] == '\t'))
        end--;
    len = (size_t)(end - start);
    if (len == 5 && strncmp(start, "/quit", 5) == 0)
        return CHAT_LINE_QUIT;
    return CHAT_LINE_TEXT;
}

static chat_action chat_local_input_action(chat_line_kind kind)
{
    switch (kind) {
    case CHAT_LINE_EMPTY:
        return CHAT_ACTION_SKIP;
    case CHAT_LINE_QUIT:
        return CHAT_ACTION_QUIT;
    default:
        return CHAT_ACTION_SEND;
    }
}

static chat_action chat_remote_eof_action(void)
{
    return CHAT_ACTION_PEER_CLOSED;
}

static int chat_poll_trouble(short revents)
{
    return (revents & (POLLERR | POLLHUP | POLLNVAL)) != 0;
}

static int chat_exit_code(chat_action action)
{
    switch (action) {
    case CHAT_ACTION_QUIT:
        return 0;
    case CHAT_ACTION_PEER_CLOSED:
        return 1;
    default:
        return 0;
    }
}

#ifndef DEMO_CHAT_CLIENT_TESTING

/* ── Guest payload (I/O loop; compiled only into the guest binary) ────── */

static int connect_server(const char *host, int port)
{
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    struct sockaddr_in address;

    if (fd < 0)
        return -1;
    memset(&address, 0, sizeof(address));
    address.sin_family = AF_INET;
    address.sin_port = htons((uint16_t)port);
    if (inet_pton(AF_INET, host, &address.sin_addr) <= 0) {
        close(fd);
        return -1;
    }
    if (connect(fd, (struct sockaddr *)&address, sizeof(address)) < 0) {
        close(fd);
        return -1;
    }
    return fd;
}

static int send_all(int fd, const char *buffer, size_t length)
{
    size_t sent = 0;

    while (sent < length) {
        ssize_t result = send(fd, buffer + sent, length - sent, 0);
        if (result < 0) {
            if (errno == EINTR)
                continue;
            return -1;
        }
        if (result == 0) {
            errno = EPIPE;
            return -1;
        }
        sent += (size_t)result;
    }
    return 0;
}

static void trim_crlf(char *line)
{
    size_t len = strlen(line);

    while (len > 0 && (line[len - 1] == '\n' || line[len - 1] == '\r'))
        line[--len] = '\0';
}

int main(int argc, char **argv)
{
    const char *host = DEMO_CHAT_DEFAULT_HOST;
    int port = DEMO_CHAT_DEFAULT_PORT;
    int fd;
    char in_buf[DEMO_CHAT_BUFFER_SIZE];
    size_t in_len = 0;
    char out_buf[DEMO_CHAT_BUFFER_SIZE];
    size_t out_len = 0;

    if (argc >= 2)
        host = argv[1];
    if (argc >= 3)
        port = atoi(argv[2]);

    signal(SIGPIPE, SIG_IGN);

    fd = connect_server(host, port);
    if (fd < 0) {
        printf("DEMO_CHAT_CLIENT_CONNECT_FAILED %s errno=%d\n",
               strerror(errno), errno);
        fflush(stdout);
        return EXIT_FAILURE;
    }
    printf("DEMO_CHAT_CLIENT_CONNECTED server=%s:%d\n", host, port);
    fflush(stdout);

    for (;;) {
        struct pollfd fds[2];
        int ready;

        fds[0].fd = 0; /* serial stdin */
        fds[0].events = POLLIN;
        fds[0].revents = 0;
        fds[1].fd = fd;
        fds[1].events = POLLIN;
        fds[1].revents = 0;

        ready = poll(fds, 2, -1);
        if (ready < 0) {
            if (errno == EINTR)
                continue;
            printf("DEMO_CHAT_CLIENT_CONNECT_FAILED poll errno=%d\n", errno);
            fflush(stdout);
            close(fd);
            return EXIT_FAILURE;
        }

        if (chat_poll_trouble(fds[1].revents)) {
            printf("DEMO_CHAT_CLIENT_PEER_CLOSED\n");
            fflush(stdout);
            close(fd);
            return 1;
        }

        if (fds[1].revents & POLLIN) {
            ssize_t received = recv(fd, out_buf + out_len,
                                    sizeof(out_buf) - out_len - 1, 0);
            if (received < 0) {
                if (errno == EINTR)
                    continue;
                printf("DEMO_CHAT_CLIENT_PEER_CLOSED\n");
                fflush(stdout);
                close(fd);
                return 1;
            }
            if (received == 0) {
                chat_action action = chat_remote_eof_action();
                printf("DEMO_CHAT_CLIENT_PEER_CLOSED\n");
                fflush(stdout);
                close(fd);
                return chat_exit_code(action);
            }
            out_len += (size_t)received;
            out_buf[out_len] = '\0';
            for (;;) {
                char *nl = strchr(out_buf, '\n');
                if (nl == NULL)
                    break;
                *nl = '\0';
                trim_crlf(out_buf);
                if (chat_classify_line(out_buf) != CHAT_LINE_EMPTY) {
                    printf("%s\n", out_buf);
                    fflush(stdout);
                }
                {
                    size_t rest = out_len - (size_t)(nl + 1 - out_buf);
                    memmove(out_buf, nl + 1, rest);
                    out_len = rest;
                }
                out_buf[out_len] = '\0';
            }
            if (out_len == sizeof(out_buf) - 1) {
                out_len = 0;
                out_buf[0] = '\0';
            }
        }

        if (fds[0].revents & POLLIN) {
            ssize_t received = read(0, in_buf + in_len,
                                    sizeof(in_buf) - in_len - 1);
            if (received < 0) {
                if (errno == EINTR)
                    continue;
                close(fd);
                return EXIT_FAILURE;
            }
            if (received == 0) {
                close(fd);
                return EXIT_FAILURE;
            }
            in_len += (size_t)received;
            in_buf[in_len] = '\0';
            for (;;) {
                char *nl = strchr(in_buf, '\n');
                if (nl == NULL)
                    break;
                *nl = '\0';
                trim_crlf(in_buf);
                if (in_buf[0] != '\0') {
                    chat_action action =
                        chat_local_input_action(chat_classify_line(in_buf));
                    if (action == CHAT_ACTION_QUIT) {
                        printf("DEMO_CHAT_CLIENT_QUIT\n");
                        fflush(stdout);
                        close(fd);
                        return chat_exit_code(action);
                    }
                    if (action == CHAT_ACTION_SEND) {
                        if (send_all(fd, in_buf, strlen(in_buf)) < 0 ||
                            send_all(fd, "\n", 1) < 0) {
                            printf("DEMO_CHAT_CLIENT_PEER_CLOSED\n");
                            fflush(stdout);
                            close(fd);
                            return 1;
                        }
                    }
                }
                {
                    size_t rest = in_len - (size_t)(nl + 1 - in_buf);
                    memmove(in_buf, nl + 1, rest);
                    in_len = rest;
                }
                in_buf[in_len] = '\0';
            }
            if (in_len == sizeof(in_buf) - 1) {
                in_len = 0;
                in_buf[0] = '\0';
            }
        }
    }
}

#endif /* !DEMO_CHAT_CLIENT_TESTING */