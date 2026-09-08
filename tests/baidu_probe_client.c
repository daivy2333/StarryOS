#define _DEFAULT_SOURCE
#define _POSIX_C_SOURCE 200809L

/* Baidu probe client — guest-side payload for the host ping server.
 *
 * Connects to host(10.0.2.2):15561 — the baidu_ping_server.py host proxy — and
 * prints whatever reachability line the server returns. The host server does
 * the real ICMP ping + HTTP check to baidu on the guest's behalf and reports
 * back over TCP, because SLIRP does not forward guest ICMP to the internet.
 *
 * Build (RISC-V static, user boundary):
 *   riscv64-linux-musl-gcc -std=c11 -Wall -Wextra -Werror -static -no-pie -Os \
 *     tests/baidu_probe_client.c -o tests/baidu_probe_client
 *
 * Host decision harness (no guest):
 *   cc -std=c11 -Wall -Wextra -Werror tests/baidu_probe_client_test.c \
 *     -o /tmp/baidu-probe-client-test && /tmp/baidu-probe-client-test
 *
 * Usage (inside QEMU guest shell, after `wget http://10.0.2.2:18765/
 * baidu_probe_client`):
 *   ./baidu_probe_client [host] [port]
 *
 * Exits 0 when the server reports a reachable/pinged result, 1 otherwise.
 */

#include <arpa/inet.h>
#include <errno.h>
#include <netinet/in.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <unistd.h>

#define BAIDU_PROBE_DEFAULT_HOST "10.0.2.2"
#define BAIDU_PROBE_DEFAULT_PORT 15561
#define BAIDU_PROBE_BUFFER_SIZE 1024

/* ── Decision core (host-testable; kept outside the TESTING guard) ────── */

typedef enum {
    PROBE_RESULT_UNKNOWN,
    PROBE_RESULT_OK,
    PROBE_RESULT_UNREACHABLE,
} probe_result;

static probe_result probe_classify(const char *report)
{
    if (report == NULL)
        return PROBE_RESULT_UNKNOWN;
    while (*report == ' ' || *report == '\t')
        report++;
    if (strncmp(report, "BAIDU_PROBE_OK", 14) == 0)
        return PROBE_RESULT_OK;
    if (strncmp(report, "BAIDU_PROBE_UNREACHABLE", 23) == 0)
        return PROBE_RESULT_UNREACHABLE;
    /* ICMP_FAIL and HTTP_FAIL are partial reachability: count them as OK
     * because the host did reach baidu on at least one path. */
    if (strncmp(report, "BAIDU_PROBE_ICMP_FAIL", 21) == 0)
        return PROBE_RESULT_OK;
    if (strncmp(report, "BAIDU_PROBE_HTTP_FAIL", 21) == 0)
        return PROBE_RESULT_OK;
    return PROBE_RESULT_UNKNOWN;
}

#ifndef BAIDU_PROBE_CLIENT_TESTING

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

int main(int argc, char **argv)
{
    const char *host = BAIDU_PROBE_DEFAULT_HOST;
    int port = BAIDU_PROBE_DEFAULT_PORT;
    int fd;
    char buffer[BAIDU_PROBE_BUFFER_SIZE];
    ssize_t received;
    probe_result result;

    if (argc >= 2)
        host = argv[1];
    if (argc >= 3)
        port = atoi(argv[2]);

    fd = connect_server(host, port);
    if (fd < 0) {
        printf("BAIDU_PROBE_CLIENT_CONNECT_FAILED %s errno=%d\n",
               strerror(errno), errno);
        fflush(stdout);
        return EXIT_FAILURE;
    }
    printf("BAIDU_PROBE_CLIENT_CONNECTED server=%s:%d\n", host, port);
    fflush(stdout);

    /* Ask the host proxy to run the probe. The server sends one line back. */
    if (send(fd, "\n", 1, 0) < 0) {
        printf("BAIDU_PROBE_CLIENT_CONNECT_FAILED send errno=%d\n", errno);
        fflush(stdout);
        close(fd);
        return EXIT_FAILURE;
    }

    received = recv(fd, buffer, sizeof(buffer) - 1, 0);
    close(fd);
    if (received < 0) {
        printf("BAIDU_PROBE_CLIENT_PEER_CLOSED recv errno=%d\n", errno);
        fflush(stdout);
        return EXIT_FAILURE;
    }
    buffer[received] = '\0';
    /* Trim trailing newline for a clean serial line. */
    {
        size_t len = strlen(buffer);
        while (len > 0 && (buffer[len - 1] == '\n' || buffer[len - 1] == '\r'))
            buffer[--len] = '\0';
    }
    printf("%s\n", buffer);
    fflush(stdout);

    result = probe_classify(buffer);
    return result == PROBE_RESULT_OK ? EXIT_SUCCESS : EXIT_FAILURE;
}

#endif /* !BAIDU_PROBE_CLIENT_TESTING */