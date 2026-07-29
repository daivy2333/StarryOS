/*
 * MS02 guest TCP/UDP service for manual QEMU network verification.
 *
 * One poll() loop owns the TCP listener, the active TCP connection, and the
 * UDP socket. The service exits successfully after two TCP round trips and
 * one UDP datagram round trip.
 *
 * Build:
 *   riscv64-linux-musl-gcc -static -O2 \
 *     -o tests/ms02_guest_service tests/ms02_guest_service.c
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

#define MS02_PORT 5555
#define MS02_TCP_REQUEST "MS02_TCP_REQUEST"
#define MS02_TCP_RESPONSE "MS02_TCP_RESPONSE\n"
#define MS02_UDP_REQUEST "MS02_UDP_REQUEST"
#define MS02_UDP_RESPONSE "MS02_UDP_RESPONSE\n"
#define MS02_TCP_ROUND_TRIPS 2
#define MS02_BUFFER_SIZE 256

static int fail(const char *stage)
{
    printf("MS02_FAIL stage=%s errno=%d message=%s\n",
           stage, errno, strerror(errno));
    fflush(stdout);
    return -1;
}

static int set_reuseaddr(int fd)
{
    int enabled = 1;
    return setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &enabled, sizeof(enabled));
}

static int bind_port(int fd)
{
    struct sockaddr_in address = {
        .sin_family = AF_INET,
        .sin_port = htons(MS02_PORT),
        .sin_addr.s_addr = htonl(INADDR_ANY),
    };

    return bind(fd, (struct sockaddr *)&address, sizeof(address));
}

static int create_tcp_listener(void)
{
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    if (fd < 0)
        return fail("tcp-socket");
    if (set_reuseaddr(fd) < 0) {
        fail("tcp-reuseaddr");
        close(fd);
        return -1;
    }
    if (bind_port(fd) < 0) {
        fail("tcp-bind");
        close(fd);
        return -1;
    }
    if (listen(fd, 4) < 0) {
        fail("tcp-listen");
        close(fd);
        return -1;
    }
    return fd;
}

static int create_udp_socket(void)
{
    int fd = socket(AF_INET, SOCK_DGRAM, 0);
    if (fd < 0)
        return fail("udp-socket");
    if (set_reuseaddr(fd) < 0) {
        fail("udp-reuseaddr");
        close(fd);
        return -1;
    }
    if (bind_port(fd) < 0) {
        fail("udp-bind");
        close(fd);
        return -1;
    }
    return fd;
}

static size_t trim_line(char *buffer, size_t length)
{
    while (length > 0 &&
           (buffer[length - 1] == '\n' || buffer[length - 1] == '\r'))
        length--;
    buffer[length] = '\0';
    return length;
}

static int send_all(int fd, const char *buffer, size_t length)
{
    size_t sent = 0;

    while (sent < length) {
        ssize_t result = send(fd, buffer + sent, length - sent, 0);
        if (result < 0) {
            if (errno == EINTR)
                continue;
            return fail("tcp-send");
        }
        if (result == 0) {
            errno = EPIPE;
            return fail("tcp-send-zero");
        }
        sent += (size_t)result;
    }
    return 0;
}

int main(void)
{
    int tcp_listener = -1;
    int udp_socket = -1;
    int tcp_client = -1;
    int tcp_passes = 0;
    int udp_passed = 0;
    char tcp_buffer[MS02_BUFFER_SIZE];
    size_t tcp_length = 0;

    signal(SIGPIPE, SIG_IGN);

    tcp_listener = create_tcp_listener();
    if (tcp_listener < 0)
        return EXIT_FAILURE;
    udp_socket = create_udp_socket();
    if (udp_socket < 0) {
        close(tcp_listener);
        return EXIT_FAILURE;
    }

    printf("MS02_READY tcp=%d udp=%d\n", MS02_PORT, MS02_PORT);
    fflush(stdout);

    while (tcp_passes < MS02_TCP_ROUND_TRIPS || !udp_passed) {
        struct pollfd fds[3] = {
            {
                .fd = tcp_client < 0 ? tcp_listener : -1,
                .events = POLLIN,
            },
            {
                .fd = udp_socket,
                .events = POLLIN,
            },
            {
                .fd = tcp_client,
                .events = POLLIN,
            },
        };
        int ready = poll(fds, 3, -1);

        if (ready < 0) {
            if (errno == EINTR)
                continue;
            fail("poll");
            goto failure;
        }

        if (fds[0].revents & (POLLERR | POLLHUP | POLLNVAL)) {
            errno = EIO;
            fail("tcp-listener-poll");
            goto failure;
        }
        if (fds[0].revents & POLLIN) {
            tcp_client = accept(tcp_listener, NULL, NULL);
            if (tcp_client < 0) {
                fail("tcp-accept");
                goto failure;
            }
            tcp_length = 0;
            printf("MS02_TCP_ACCEPTED connection=%d\n", tcp_passes + 1);
            fflush(stdout);
        }

        if (fds[1].revents & (POLLERR | POLLHUP | POLLNVAL)) {
            errno = EIO;
            fail("udp-poll");
            goto failure;
        }
        if (fds[1].revents & POLLIN) {
            struct sockaddr_in peer;
            socklen_t peer_length = sizeof(peer);
            char buffer[MS02_BUFFER_SIZE];
            ssize_t received = recvfrom(
                udp_socket, buffer, sizeof(buffer) - 1, 0,
                (struct sockaddr *)&peer, &peer_length);

            if (received < 0) {
                fail("udp-recvfrom");
                goto failure;
            }
            trim_line(buffer, (size_t)received);
            if (strcmp(buffer, MS02_UDP_REQUEST) != 0) {
                errno = EPROTO;
                fail("udp-payload");
                goto failure;
            }
            if (sendto(udp_socket, MS02_UDP_RESPONSE,
                       strlen(MS02_UDP_RESPONSE), 0,
                       (struct sockaddr *)&peer, peer_length) < 0) {
                fail("udp-sendto");
                goto failure;
            }
            udp_passed = 1;
            printf("MS02_UDP_PASS datagrams=1\n");
            fflush(stdout);
        }

        if (tcp_client >= 0 && fds[2].revents & POLLIN) {
            ssize_t received = recv(tcp_client, tcp_buffer + tcp_length,
                                    sizeof(tcp_buffer) - tcp_length - 1, 0);

            if (received < 0) {
                if (errno == EINTR)
                    continue;
                fail("tcp-recv");
                goto failure;
            }
            if (received == 0) {
                errno = ECONNRESET;
                fail("tcp-close-before-payload");
                goto failure;
            }
            tcp_length += (size_t)received;
            tcp_buffer[tcp_length] = '\0';

            if (strchr(tcp_buffer, '\n') != NULL) {
                trim_line(tcp_buffer, tcp_length);
                if (strcmp(tcp_buffer, MS02_TCP_REQUEST) != 0) {
                    errno = EPROTO;
                    fail("tcp-payload");
                    goto failure;
                }
                if (send_all(tcp_client, MS02_TCP_RESPONSE,
                             strlen(MS02_TCP_RESPONSE)) < 0)
                    goto failure;

                close(tcp_client);
                tcp_client = -1;
                tcp_length = 0;
                tcp_passes++;
                printf("MS02_TCP_PASS connection=%d\n", tcp_passes);
                fflush(stdout);
            } else if (tcp_length == sizeof(tcp_buffer) - 1) {
                errno = EMSGSIZE;
                fail("tcp-payload-too-large");
                goto failure;
            }
        }

        if (tcp_client >= 0 &&
            fds[2].revents & (POLLERR | POLLHUP | POLLNVAL)) {
            errno = EIO;
            fail("tcp-client-poll");
            goto failure;
        }
    }

    printf("MS02_COMPLETE tcp=%d udp=1\n", tcp_passes);
    fflush(stdout);
    close(udp_socket);
    close(tcp_listener);
    return EXIT_SUCCESS;

failure:
    if (tcp_client >= 0)
        close(tcp_client);
    close(udp_socket);
    close(tcp_listener);
    return EXIT_FAILURE;
}
