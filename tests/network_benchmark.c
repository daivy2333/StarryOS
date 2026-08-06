/* MS16 portable network benchmark. C11, single process, single poll loop. */
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <limits.h>
#include <netinet/in.h>
#include <netinet/tcp.h>
#include <poll.h>
#include <signal.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

#include "network_benchmark_platform.h"
#include "network_benchmark_protocol.h"

#define DEFAULT_PORT 5555
#define MAX_FLOWS 8
#define IO_DEADLINE_NS (10ULL * 1000000000ULL)
#define DRAIN_NS (250ULL * 1000000ULL)
#define RECV_BUFFER (NB_DATA_RECORD_MAX * 4)
#define UDP_REGISTER_MAGIC 0x4e425247u
#define NOMINAL_LINK_BPS 1000000000ULL

#define IO_ERROR (-1)
#define IO_EOF (-2)
#define IO_TIMEOUT (-3)
#define IO_CANCELLED (-4)

enum mode {
    MODE_SERVER, MODE_CLIENT, MODE_LOOPBACK, MODE_SELF_TEST, MODE_CALIBRATE
};
enum profile { PROFILE_SMOKE, PROFILE_QUICK, PROFILE_STANDARD };
enum side { SIDE_GUEST, SIDE_HOST };

struct args {
    enum mode mode;
    enum profile profile;
    enum side side;
    const char *addr;
    int port;
    int protocol;
    int direction;
    int flows;
    int payload;
    int duration;
    int warmup;
    uint32_t seed;
    int offered_load;
    int nagle;
    uint64_t run_id;
    uint32_t test_id;
    uint32_t round_id;
    int print_config;
};

struct tx_state {
    uint8_t wire[NB_DATA_RECORD_MAX];
    size_t length;
    size_t offset;
    uint32_t sequence;
    uint64_t next_send_ns;
};

struct rx_state {
    uint8_t bytes[RECV_BUFFER];
    size_t length;
    uint32_t next_sequence;
    int have_sequence;
};

struct flow {
    int fd;
    uint8_t id;
    struct tx_state tx;
    struct rx_state rx;
};

struct metrics {
    uint64_t tx_bytes;
    uint64_t tx_packets;
    uint64_t rx_bytes;
    uint64_t rx_packets;
    uint64_t wire_tx_bytes;
    uint64_t wire_rx_bytes;
    uint64_t udp_offered;
    uint64_t udp_accepted;
    uint64_t udp_loss;
    uint64_t udp_duplicate;
    uint64_t udp_reorder;
    uint64_t udp_corrupt;
    uint64_t udp_late;
    int invalid_reason;
};

struct endpoint {
    struct args args;
    struct nb_config config;
    struct flow flows[MAX_FLOWS];
    int control_fd;
    int listener_fd;
    int udp_fd;
    struct sockaddr_in udp_peer;
    socklen_t udp_peer_len;
    int udp_peer_known;
    int may_send;
    int may_recv;
    uint64_t data_end_ns;
    struct metrics metrics;
};

static volatile sig_atomic_t cancelled;

static uint64_t now_ns(void) { return nb_monotonic_ns(); }

static void on_signal(int signo)
{
    (void)signo;
    cancelled = 1;
}

static void json_line(const char *format, ...)
{
    va_list ap;
    va_start(ap, format);
    vprintf(format, ap);
    va_end(ap);
    putchar('\n');
    fflush(stdout);
}

static const char *side_name(enum side side)
{
    return side == SIDE_GUEST ? "guest" : "host";
}

static const char *protocol_name(int protocol)
{
    return protocol == NB_PROTO_TCP ? "TCP" : "UDP";
}

static const char *direction_name(int direction)
{
    if (direction == NB_DIR_TX) return "TX";
    if (direction == NB_DIR_RX) return "RX";
    return "BIDI";
}

static const char *profile_name(enum profile profile)
{
    if (profile == PROFILE_SMOKE) return "smoke";
    if (profile == PROFILE_QUICK) return "quick";
    return "standard";
}

static int parse_u64_range(const char *text, uint64_t minimum,
                           uint64_t maximum, uint64_t *value)
{
    char *end = NULL;
    unsigned long long parsed;
    if (!text || !*text || text[0] == '-') return -1;
    errno = 0;
    parsed = strtoull(text, &end, 0);
    if (errno || !end || *end || parsed < minimum || parsed > maximum) return -1;
    *value = (uint64_t)parsed;
    return 0;
}

static void apply_profile(struct args *args)
{
    args->seed = 12345;
    if (args->profile == PROFILE_SMOKE) {
        args->warmup = 0;
        args->duration = 2;
    } else if (args->profile == PROFILE_QUICK) {
        args->warmup = 1;
        args->duration = 5;
    } else {
        args->warmup = 2;
        args->duration = 10;
    }
}

static int value_after(int argc, char **argv, int *index, const char **value)
{
    if (*index + 1 >= argc) return -1;
    *value = argv[++*index];
    return 0;
}

static int parse_args(int argc, char **argv, struct args *args)
{
    uint64_t number;
    int seen_profile = 0;
    memset(args, 0, sizeof(*args));
    args->port = DEFAULT_PORT;
    args->profile = PROFILE_SMOKE;
    args->side = SIDE_GUEST;
    args->protocol = NB_PROTO_TCP;
    args->direction = NB_DIR_TX;
    args->flows = 1;
    args->payload = 1400;
    args->run_id = 1;
    args->test_id = 1;
    args->round_id = 1;

    if (argc < 2) return -1;
    if (!strcmp(argv[1], "server")) args->mode = MODE_SERVER;
    else if (!strcmp(argv[1], "client")) args->mode = MODE_CLIENT;
    else if (!strcmp(argv[1], "loopback")) args->mode = MODE_LOOPBACK;
    else if (!strcmp(argv[1], "--self-test")) args->mode = MODE_SELF_TEST;
    else if (!strcmp(argv[1], "--calibrate")) args->mode = MODE_CALIBRATE;
    else if (!strcmp(argv[1], "--print-config")) args->print_config = 1;
    else return -1;

    for (int i = 2; i < argc; i++) {
        if (!strcmp(argv[i], "--profile")) {
            const char *value;
            if (seen_profile || value_after(argc, argv, &i, &value) < 0) return -1;
            seen_profile = 1;
            if (!strcmp(value, "smoke")) args->profile = PROFILE_SMOKE;
            else if (!strcmp(value, "quick")) args->profile = PROFILE_QUICK;
            else if (!strcmp(value, "standard")) args->profile = PROFILE_STANDARD;
            else return -1;
        }
    }
    apply_profile(args);

    unsigned seen = 0;
    for (int i = 2; i < argc; i++) {
        const char *key = argv[i];
        const char *value;
        unsigned bit = 0;
        if (!strcmp(key, "--profile")) { i++; continue; }
        if (!strcmp(key, "--print-config")) { args->print_config = 1; continue; }
        if (value_after(argc, argv, &i, &value) < 0) return -1;
        if (!strcmp(key, "--addr")) { bit = 1u << 0; args->addr = value; }
        else if (!strcmp(key, "--port")) {
            bit = 1u << 1;
            if (parse_u64_range(value, 1, 65535, &number)) return -1;
            args->port = (int)number;
        } else if (!strcmp(key, "--protocol")) {
            bit = 1u << 2;
            if (!strcmp(value, "tcp")) args->protocol = NB_PROTO_TCP;
            else if (!strcmp(value, "udp")) args->protocol = NB_PROTO_UDP;
            else return -1;
        } else if (!strcmp(key, "--direction")) {
            bit = 1u << 3;
            if (!strcmp(value, "tx")) args->direction = NB_DIR_TX;
            else if (!strcmp(value, "rx")) args->direction = NB_DIR_RX;
            else if (!strcmp(value, "bidi")) args->direction = NB_DIR_BIDI;
            else return -1;
        } else if (!strcmp(key, "--flows")) {
            bit = 1u << 4;
            if (parse_u64_range(value, 1, 8, &number)) return -1;
            if (number != 1 && number != 2 && number != 4 && number != 8) return -1;
            args->flows = (int)number;
        } else if (!strcmp(key, "--payload")) {
            bit = 1u << 5;
            if (parse_u64_range(value, 1, NB_TCP_PAYLOAD_MAX, &number)) return -1;
            args->payload = (int)number;
        } else if (!strcmp(key, "--duration")) {
            bit = 1u << 6;
            if (parse_u64_range(value, 1, UINT16_MAX, &number)) return -1;
            args->duration = (int)number;
        } else if (!strcmp(key, "--warmup")) {
            bit = 1u << 7;
            if (parse_u64_range(value, 0, UINT16_MAX, &number)) return -1;
            args->warmup = (int)number;
        } else if (!strcmp(key, "--seed")) {
            bit = 1u << 8;
            if (parse_u64_range(value, 0, UINT32_MAX, &number)) return -1;
            args->seed = (uint32_t)number;
        } else if (!strcmp(key, "--offered-load")) {
            bit = 1u << 9;
            if (parse_u64_range(value, 0, 100, &number)) return -1;
            args->offered_load = (int)number;
        } else if (!strcmp(key, "--side")) {
            bit = 1u << 10;
            if (!strcmp(value, "guest")) args->side = SIDE_GUEST;
            else if (!strcmp(value, "host")) args->side = SIDE_HOST;
            else return -1;
        } else if (!strcmp(key, "--run-id")) {
            bit = 1u << 11;
            if (parse_u64_range(value, 1, UINT64_MAX, &number)) return -1;
            args->run_id = number;
        } else if (!strcmp(key, "--test-id")) {
            bit = 1u << 12;
            if (parse_u64_range(value, 1, UINT32_MAX, &number)) return -1;
            args->test_id = (uint32_t)number;
        } else if (!strcmp(key, "--round-id")) {
            bit = 1u << 13;
            if (parse_u64_range(value, 1, UINT32_MAX, &number)) return -1;
            args->round_id = (uint32_t)number;
        } else return -1;
        if (seen & bit) return -1;
        seen |= bit;
    }

    if (args->protocol == NB_PROTO_UDP && args->payload > NB_UDP_PAYLOAD_MAX) return -1;
    if (args->mode == MODE_CLIENT && !args->addr) return -1;
    return 0;
}

static void usage(const char *program)
{
    fprintf(stderr,
        "Usage: %s server|client|loopback [options]\n"
        "       %s --self-test | --calibrate\n"
        "Options: --side guest|host --addr IPv4:PORT --port PORT\n"
        "  --protocol tcp|udp --direction tx|rx|bidi --flows 1|2|4|8\n"
        "  --payload BYTES --profile smoke|quick|standard --duration S\n"
        "  --warmup S --seed N --offered-load 0..100 --run-id N\n"
        "  --test-id N --round-id N --print-config\n", program, program);
}

static int set_nonblocking(int fd)
{
    int flags = fcntl(fd, F_GETFL, 0);
    if (flags < 0) return -1;
    return fcntl(fd, F_SETFL, flags | O_NONBLOCK);
}

static int poll_until(int fd, short events, uint64_t deadline)
{
    while (!cancelled) {
        uint64_t now = now_ns();
        if (now >= deadline) return IO_TIMEOUT;
        uint64_t remaining = deadline - now;
        int timeout = (int)((remaining + 999999ULL) / 1000000ULL);
        struct pollfd pfd = {fd, events, 0};
        int rc = poll(&pfd, 1, timeout);
        if (rc > 0) {
            if (pfd.revents & (POLLERR | POLLNVAL)) return IO_ERROR;
            if (pfd.revents & events) return 0;
            if (pfd.revents & POLLHUP) return IO_EOF;
        } else if (rc < 0 && errno != EINTR) return IO_ERROR;
    }
    return IO_CANCELLED;
}

static int send_all(int fd, const uint8_t *data, size_t length, uint64_t deadline)
{
    size_t offset = 0;
    while (offset < length) {
        ssize_t sent = send(fd, data + offset, length - offset, MSG_NOSIGNAL);
        if (sent > 0) { offset += (size_t)sent; continue; }
        if (sent == 0) return IO_EOF;
        if (errno != EAGAIN && errno != EWOULDBLOCK && errno != EINTR) return IO_ERROR;
        int wait_rc = poll_until(fd, POLLOUT, deadline);
        if (wait_rc) return wait_rc;
    }
    return 0;
}

static int recv_exact(int fd, uint8_t *data, size_t length, uint64_t deadline)
{
    size_t offset = 0;
    while (offset < length) {
        ssize_t received = recv(fd, data + offset, length - offset, 0);
        if (received > 0) { offset += (size_t)received; continue; }
        if (received == 0) return IO_EOF;
        if (errno != EAGAIN && errno != EWOULDBLOCK && errno != EINTR) return IO_ERROR;
        int wait_rc = poll_until(fd, POLLIN, deadline);
        if (wait_rc) return wait_rc;
    }
    return 0;
}

static int send_control(int fd, int type, const struct endpoint *endpoint,
                        const struct nb_summary *summary)
{
    uint8_t wire[NB_FRAME_MAX];
    size_t length = sizeof(wire);
    int rc;
    const struct nb_config *cfg = &endpoint->config;
    if (type == NB_FRAME_HELLO) rc = nb_hello_encode(wire, &length, cfg);
    else if (type == NB_FRAME_READY) rc = nb_ready_encode(
        wire, &length, cfg->run_id, cfg->test_id, cfg->round_id,
        cfg->config_fingerprint);
    else if (type == NB_FRAME_START) rc = nb_start_encode(
        wire, &length, cfg->run_id, cfg->test_id, cfg->round_id,
        cfg->config_fingerprint);
    else if (type == NB_FRAME_SUMMARY) rc = nb_summary_encode(wire, &length, summary);
    else return -1;
    if (rc) return -1;
    return send_all(fd, wire, length, now_ns() + IO_DEADLINE_NS);
}

static int recv_control(int fd, int expected, struct nb_frame *frame)
{
    uint8_t wire[NB_FRAME_MAX];
    uint16_t body_be;
    uint16_t body_length;
    uint64_t deadline = now_ns() + IO_DEADLINE_NS;
    int rc = recv_exact(fd, wire, NB_FRAME_MIN, deadline);
    if (rc) return rc;
    memcpy(&body_be, wire + NB_FRAME_BODY_LEN_OFF, sizeof(body_be));
    body_length = nb_ntoh16(body_be);
    if (body_length > NB_FRAME_BODY_MAX) return -1;
    rc = recv_exact(fd, wire + NB_FRAME_MIN, body_length, deadline);
    if (rc) return rc;
    if (nb_frame_decode(frame, wire, NB_FRAME_MIN + body_length) < 0) return -1;
    return frame->type == expected ? 0 : -1;
}

static int make_listener(int port, int type)
{
    int fd = socket(AF_INET, type, 0);
    int one = 1;
    struct sockaddr_in address;
    if (fd < 0) return -1;
    if (setsockopt(fd, SOL_SOCKET, SO_REUSEADDR, &one, sizeof(one))) goto fail;
    memset(&address, 0, sizeof(address));
    address.sin_family = AF_INET;
    address.sin_addr.s_addr = htonl(INADDR_ANY);
    address.sin_port = htons((uint16_t)port);
    if (bind(fd, (struct sockaddr *)&address, sizeof(address))) goto fail;
    if (type == SOCK_STREAM && listen(fd, 16)) goto fail;
    if (set_nonblocking(fd)) goto fail;
    return fd;
fail:
    close(fd);
    return -1;
}

static int parse_address(const char *text, int fallback, struct sockaddr_in *address)
{
    char host[64];
    const char *colon = strrchr(text, ':');
    size_t length = colon ? (size_t)(colon - text) : strlen(text);
    uint64_t port = (uint64_t)fallback;
    if (!length || length >= sizeof(host)) return -1;
    memcpy(host, text, length);
    host[length] = 0;
    if (colon && parse_u64_range(colon + 1, 1, 65535, &port)) return -1;
    memset(address, 0, sizeof(*address));
    address->sin_family = AF_INET;
    address->sin_port = htons((uint16_t)port);
    return inet_pton(AF_INET, host, &address->sin_addr) == 1 ? 0 : -1;
}

static int connect_socket(const char *text, int fallback, int type)
{
    struct sockaddr_in address;
    int fd;
    if (parse_address(text, fallback, &address)) return -1;
    fd = socket(AF_INET, type, 0);
    if (fd < 0) return -1;
    if (set_nonblocking(fd)) { close(fd); return -1; }
    if (connect(fd, (struct sockaddr *)&address, sizeof(address))) {
        if (errno != EINPROGRESS || poll_until(fd, POLLOUT, now_ns() + IO_DEADLINE_NS)) {
            close(fd);
            return -1;
        }
        int error = 0;
        socklen_t length = sizeof(error);
        if (getsockopt(fd, SOL_SOCKET, SO_ERROR, &error, &length) || error) {
            close(fd);
            return -1;
        }
    }
    return fd;
}

static int accept_socket(int listener)
{
    uint64_t deadline = now_ns() + IO_DEADLINE_NS;
    for (;;) {
        int fd = accept(listener, NULL, NULL);
        if (fd >= 0) {
            if (set_nonblocking(fd)) { close(fd); return -1; }
            return fd;
        }
        if (errno != EAGAIN && errno != EWOULDBLOCK && errno != EINTR) return -1;
        if (poll_until(listener, POLLIN, deadline)) return -1;
    }
}

static void build_config(struct endpoint *endpoint)
{
    struct nb_config *cfg = &endpoint->config;
    const struct args *args = &endpoint->args;
    memset(cfg, 0, sizeof(*cfg));
    cfg->protocol = (uint8_t)args->protocol;
    cfg->direction = (uint8_t)args->direction;
    cfg->flow_count = (uint8_t)args->flows;
    cfg->payload_size = (uint16_t)args->payload;
    cfg->duration_s = (uint16_t)args->duration;
    cfg->warmup_s = (uint16_t)args->warmup;
    cfg->seed = args->seed;
    cfg->offered_load_pct = (uint8_t)args->offered_load;
    cfg->nagle = (uint8_t)args->nagle;
    cfg->run_id = args->run_id;
    cfg->test_id = args->test_id;
    cfg->round_id = args->round_id;
    cfg->capability_bitmap = (uint64_t)nb_capability_monotonic();
    cfg->config_fingerprint = nb_config_fingerprint(cfg);
    endpoint->may_send = args->direction == NB_DIR_BIDI ||
        (args->direction == NB_DIR_TX && args->side == SIDE_GUEST) ||
        (args->direction == NB_DIR_RX && args->side == SIDE_HOST);
    endpoint->may_recv = args->direction == NB_DIR_BIDI || !endpoint->may_send;
}

static void emit_manifest(const struct endpoint *endpoint)
{
    const struct args *args = &endpoint->args;
    const struct nb_config *cfg = &endpoint->config;
    json_line("{\"schema_version\":1,\"type\":\"manifest\","
        "\"side\":\"%s\",\"platform\":\"%s\","
        "\"driver_mode\":\"polling\",\"profile\":\"%s\","
        "\"protocol\":\"%s\",\"direction\":\"%s\","
        "\"flow_count\":%u,\"payload_size\":%u,"
        "\"duration_s\":%u,\"warmup_s\":%u,\"seed\":%u,"
        "\"offered_load_pct\":%u,\"run_id\":%" PRIu64 ","
        "\"test_id\":%u,\"round_id\":%u,"
        "\"config_fingerprint\":\"%016" PRIx64 "\"}",
        side_name(args->side), args->mode == MODE_LOOPBACK ? "local" : "qemu",
        profile_name(args->profile), protocol_name(cfg->protocol),
        direction_name(cfg->direction), cfg->flow_count, cfg->payload_size,
        cfg->duration_s, cfg->warmup_s, cfg->seed, cfg->offered_load_pct,
        cfg->run_id, cfg->test_id, cfg->round_id, cfg->config_fingerprint);
}

static int validate_payload(const struct endpoint *endpoint,
                            const struct nb_data_record *record)
{
    uint8_t expected[NB_TCP_PAYLOAD_MAX];
    if (record->payload_length != endpoint->config.payload_size) return -1;
    nb_generator_fill(expected, record->payload_length, endpoint->config.seed,
        record->hdr.flow_id, record->hdr.sequence, 0);
    return memcmp(expected, record->payload, record->payload_length) ? -1 : 0;
}

static int prepare_record(struct endpoint *endpoint, struct flow *flow)
{
    uint8_t payload[NB_TCP_PAYLOAD_MAX];
    size_t length = sizeof(flow->tx.wire);
    nb_generator_fill(payload, endpoint->config.payload_size, endpoint->config.seed,
        flow->id, flow->tx.sequence, 0);
    if (nb_data_record_encode(flow->tx.wire, &length, payload,
            endpoint->config.payload_size, endpoint->config.protocol,
            endpoint->config.direction, flow->tx.sequence, flow->id,
            endpoint->config.round_id, NB_CP_C1)) return -1;
    flow->tx.length = length;
    flow->tx.offset = 0;
    return 0;
}

static int accept_record(struct endpoint *endpoint, struct flow *flow,
                         const uint8_t *wire, size_t length, int udp)
{
    struct nb_data_record record;
    int rc = nb_data_record_decode(&record, wire, length);
    if (rc || record.hdr.protocol != endpoint->config.protocol ||
        record.hdr.direction != endpoint->config.direction ||
        record.hdr.flow_id != flow->id ||
        record.hdr.round_id != endpoint->config.round_id ||
        validate_payload(endpoint, &record)) {
        endpoint->metrics.udp_corrupt += udp ? 1U : 0U;
        endpoint->metrics.invalid_reason = NB_INVALID_CHECKSUM;
        return -1;
    }
    if (udp && endpoint->data_end_ns && now_ns() > endpoint->data_end_ns) {
        endpoint->metrics.udp_late++;
        return 0;
    }
    if (!flow->rx.have_sequence) {
        flow->rx.next_sequence = record.hdr.sequence;
        flow->rx.have_sequence = 1;
    }
    if (record.hdr.sequence < flow->rx.next_sequence) {
        if (udp) endpoint->metrics.udp_duplicate++;
        else endpoint->metrics.invalid_reason = NB_INVALID_PARTIAL;
        return udp ? 0 : -1;
    }
    if (record.hdr.sequence > flow->rx.next_sequence) {
        if (udp) {
            endpoint->metrics.udp_loss += record.hdr.sequence - flow->rx.next_sequence;
            endpoint->metrics.udp_reorder++;
        } else {
            endpoint->metrics.invalid_reason = NB_INVALID_PARTIAL;
            return -1;
        }
    }
    flow->rx.next_sequence = record.hdr.sequence + 1;
    endpoint->metrics.rx_bytes += record.payload_length;
    endpoint->metrics.rx_packets++;
    endpoint->metrics.wire_rx_bytes += length;
    return 0;
}

static int consume_tcp(struct endpoint *endpoint, struct flow *flow)
{
    for (;;) {
        if (flow->rx.length < NB_DATA_RECORD_FIXED) return 0;
        uint32_t payload_be;
        uint32_t payload_length;
        size_t total;
        memcpy(&payload_be, flow->rx.bytes + NB_RECORD_HDR_SIZE, 4);
        payload_length = nb_ntoh32(payload_be);
        if (payload_length > NB_TCP_PAYLOAD_MAX) return -1;
        total = NB_DATA_RECORD_FIXED + payload_length;
        if (flow->rx.length < total) return 0;
        if (accept_record(endpoint, flow, flow->rx.bytes, total, 0)) return -1;
        memmove(flow->rx.bytes, flow->rx.bytes + total, flow->rx.length - total);
        flow->rx.length -= total;
    }
}

static int send_tcp(struct endpoint *endpoint, struct flow *flow)
{
    if (!flow->tx.length && prepare_record(endpoint, flow)) return -1;
    ssize_t sent = send(flow->fd, flow->tx.wire + flow->tx.offset,
                        flow->tx.length - flow->tx.offset, MSG_NOSIGNAL);
    if (sent < 0) {
        if (errno == EAGAIN || errno == EWOULDBLOCK || errno == EINTR) return 0;
        return -1;
    }
    if (!sent) return -1;
    flow->tx.offset += (size_t)sent;
    endpoint->metrics.wire_tx_bytes += (uint64_t)sent;
    if (flow->tx.offset == flow->tx.length) {
        endpoint->metrics.tx_bytes += endpoint->config.payload_size;
        endpoint->metrics.tx_packets++;
        flow->tx.sequence++;
        flow->tx.length = 0;
        flow->tx.offset = 0;
    }
    return 0;
}

static int recv_tcp(struct endpoint *endpoint, struct flow *flow)
{
    ssize_t received;
    size_t room = sizeof(flow->rx.bytes) - flow->rx.length;
    if (!room) return -1;
    received = recv(flow->fd, flow->rx.bytes + flow->rx.length, room, 0);
    if (received > 0) {
        flow->rx.length += (size_t)received;
        return consume_tcp(endpoint, flow);
    }
    if (!received) return 1;
    if (errno == EAGAIN || errno == EWOULDBLOCK || errno == EINTR) return 0;
    return -1;
}

static uint64_t udp_interval_ns(const struct endpoint *endpoint)
{
    if (!endpoint->args.offered_load) return 0;
    uint64_t bits = (uint64_t)(endpoint->config.payload_size + NB_DATA_RECORD_FIXED) * 8ULL;
    uint64_t bps = NOMINAL_LINK_BPS * (uint64_t)endpoint->args.offered_load / 100ULL;
    return bps ? (bits * 1000000000ULL) / bps : 0;
}

static int send_udp(struct endpoint *endpoint, struct flow *flow, uint64_t now)
{
    ssize_t sent;
    uint64_t interval = udp_interval_ns(endpoint);
    if (interval && now < flow->tx.next_send_ns) return 0;
    if (flow->tx.length == 0 && prepare_record(endpoint, flow)) return -1;
    endpoint->metrics.udp_offered++;
    if (endpoint->udp_peer_known) {
        sent = sendto(endpoint->udp_fd, flow->tx.wire, flow->tx.length,
            MSG_NOSIGNAL, (struct sockaddr *)&endpoint->udp_peer,
            endpoint->udp_peer_len);
    } else {
        sent = send(endpoint->udp_fd, flow->tx.wire, flow->tx.length, MSG_NOSIGNAL);
    }
    if (sent < 0) {
        if (errno == EAGAIN || errno == EWOULDBLOCK || errno == EINTR) return 0;
        return -1;
    }
    if ((size_t)sent != flow->tx.length) return -1;
    endpoint->metrics.udp_accepted++;
    endpoint->metrics.tx_bytes += endpoint->config.payload_size;
    endpoint->metrics.tx_packets++;
    endpoint->metrics.wire_tx_bytes += (uint64_t)sent;
    flow->tx.sequence++;
    flow->tx.length = 0;
    if (interval) {
        uint64_t next = flow->tx.next_send_ns ? flow->tx.next_send_ns + interval : now + interval;
        flow->tx.next_send_ns = next < now ? now + interval : next;
    }
    return 0;
}

static int recv_udp(struct endpoint *endpoint)
{
    uint8_t wire[NB_DATA_RECORD_MAX];
    ssize_t received = recv(endpoint->udp_fd, wire, sizeof(wire), 0);
    if (received < 0) {
        if (errno == EAGAIN || errno == EWOULDBLOCK || errno == EINTR) return 0;
        return -1;
    }
    if ((size_t)received < NB_DATA_RECORD_FIXED) return -1;
    uint8_t flow_id = wire[7];
    if (flow_id >= endpoint->args.flows) return -1;
    return accept_record(endpoint, &endpoint->flows[flow_id], wire,
                         (size_t)received, 1);
}

static int run_data(struct endpoint *endpoint)
{
    uint64_t warmup_end = now_ns() + (uint64_t)endpoint->args.warmup * 1000000000ULL;
    while (now_ns() < warmup_end && !cancelled) nb_nanosleep(1000000ULL);
    uint64_t end = now_ns() + (uint64_t)endpoint->args.duration * 1000000000ULL;
    endpoint->data_end_ns = end;
    while (now_ns() < end && !cancelled) {
        struct pollfd pfds[MAX_FLOWS];
        int count = endpoint->args.protocol == NB_PROTO_TCP ? endpoint->args.flows : 1;
        for (int i = 0; i < count; i++) {
            pfds[i].fd = endpoint->args.protocol == NB_PROTO_TCP ?
                endpoint->flows[i].fd : endpoint->udp_fd;
            pfds[i].events = 0;
            if (endpoint->may_recv) pfds[i].events |= POLLIN;
            if (endpoint->may_send) pfds[i].events |= POLLOUT;
            pfds[i].revents = 0;
        }
        int rc = poll(pfds, (nfds_t)count, 20);
        if (rc < 0 && errno != EINTR) return -1;
        uint64_t now = now_ns();
        for (int i = 0; i < count; i++) {
            if (pfds[i].revents & (POLLERR | POLLNVAL)) return -1;
            if (endpoint->args.protocol == NB_PROTO_TCP) {
                if ((pfds[i].revents & POLLIN) && endpoint->may_recv) {
                    int recv_rc = recv_tcp(endpoint, &endpoint->flows[i]);
                    if (recv_rc < 0) return -1;
                    if (recv_rc == 1) {
                        endpoint->metrics.invalid_reason = NB_INVALID_PEER_EOF;
                        return -1;
                    }
                }
                if ((pfds[i].revents & POLLOUT) && endpoint->may_send &&
                    send_tcp(endpoint, &endpoint->flows[i])) return -1;
            } else {
                if ((pfds[i].revents & POLLIN) && endpoint->may_recv && recv_udp(endpoint)) return -1;
                if ((pfds[i].revents & POLLOUT) && endpoint->may_send) {
                    for (int flow = 0; flow < endpoint->args.flows; flow++)
                        if (send_udp(endpoint, &endpoint->flows[flow], now)) return -1;
                }
            }
        }
    }
    if (cancelled) { endpoint->metrics.invalid_reason = NB_INVALID_CANCELLED; return -1; }
    if (endpoint->args.protocol == NB_PROTO_TCP && endpoint->may_send) {
        uint64_t flush_deadline = now_ns() + DRAIN_NS;
        for (;;) {
            int pending = 0;
            for (int i = 0; i < endpoint->args.flows; i++) {
                if (endpoint->flows[i].tx.length) {
                    pending = 1;
                    if (send_tcp(endpoint, &endpoint->flows[i])) return -1;
                }
            }
            if (!pending) break;
            if (now_ns() >= flush_deadline) {
                endpoint->metrics.invalid_reason = NB_INVALID_TIMEOUT;
                return -1;
            }
            nb_nanosleep(1000000ULL);
        }
    }
    uint64_t drain_end = now_ns() + DRAIN_NS;
    while (endpoint->may_recv && now_ns() < drain_end) {
        if (endpoint->args.protocol == NB_PROTO_TCP) {
            for (int i = 0; i < endpoint->args.flows; i++) {
                int rc = recv_tcp(endpoint, &endpoint->flows[i]);
                if (rc < 0) return -1;
            }
        } else if (recv_udp(endpoint)) return -1;
        nb_nanosleep(1000000ULL);
    }
    for (int i = 0; i < endpoint->args.flows; i++) {
        if (endpoint->flows[i].tx.length || endpoint->flows[i].rx.length) {
            endpoint->metrics.invalid_reason = NB_INVALID_PARTIAL;
            return -1;
        }
    }
    return 0;
}

static void fill_summary(const struct endpoint *endpoint, struct nb_summary *summary)
{
    memset(summary, 0, sizeof(*summary));
    summary->run_id = endpoint->config.run_id;
    summary->test_id = endpoint->config.test_id;
    summary->round_id = endpoint->config.round_id;
    summary->config_fingerprint = endpoint->config.config_fingerprint;
    summary->completion_point = NB_CP_C6;
    summary->status = endpoint->metrics.invalid_reason ? NB_STATUS_INVALID : NB_STATUS_VALID;
    summary->invalid_reason = (uint8_t)endpoint->metrics.invalid_reason;
    summary->rx_bytes = endpoint->metrics.rx_bytes;
    summary->rx_packets = endpoint->metrics.rx_packets;
    summary->tx_bytes = endpoint->metrics.tx_bytes;
    summary->tx_packets = endpoint->metrics.tx_packets;
    summary->udp_loss = (uint32_t)endpoint->metrics.udp_loss;
    summary->udp_duplicate = (uint32_t)endpoint->metrics.udp_duplicate;
    summary->udp_reorder = (uint32_t)endpoint->metrics.udp_reorder;
    summary->udp_corrupt = (uint32_t)endpoint->metrics.udp_corrupt;
    summary->udp_late = (uint32_t)endpoint->metrics.udp_late;
}

static int summaries_close(const struct endpoint *endpoint,
                           const struct nb_summary *local,
                           const struct nb_summary *peer)
{
    if (peer->run_id != local->run_id || peer->test_id != local->test_id ||
        peer->round_id != local->round_id ||
        peer->config_fingerprint != local->config_fingerprint ||
        peer->status != NB_STATUS_VALID) return -1;
    if (local->tx_bytes != peer->rx_bytes || peer->tx_bytes != local->rx_bytes) return -1;
    if (endpoint->may_send && !local->tx_bytes) return -1;
    if (endpoint->may_recv && !local->rx_bytes) return -1;
    return 0;
}

static void emit_round(const struct endpoint *endpoint,
                       const struct nb_summary *summary)
{
    const struct metrics *m = &endpoint->metrics;
    json_line("{\"schema_version\":1,\"type\":\"round\","
        "\"run_id\":%" PRIu64 ",\"test_id\":%u,\"round_id\":%u,"
        "\"side\":\"%s\",\"status\":\"%s\",\"invalid_reason\":%u,"
        "\"protocol\":\"%s\",\"direction\":\"%s\","
        "\"completion_point\":6,\"config_fingerprint\":\"%016" PRIx64 "\","
        "\"duration_s\":%u,\"tx_bytes\":%" PRIu64 ","
        "\"tx_packets\":%" PRIu64 ",\"rx_bytes\":%" PRIu64 ","
        "\"rx_packets\":%" PRIu64 ",\"wire_tx_bytes\":%" PRIu64 ","
        "\"wire_rx_bytes\":%" PRIu64 ",\"udp_offered\":%" PRIu64 ","
        "\"udp_accepted\":%" PRIu64 ",\"udp_loss\":%" PRIu64 ","
        "\"udp_duplicate\":%" PRIu64 ",\"udp_reorder\":%" PRIu64 ","
        "\"udp_corrupt\":%" PRIu64 ",\"udp_late\":%" PRIu64 ","
        "\"flow_count\":%u,\"instret_status\":\"unavailable\"}",
        summary->run_id, summary->test_id, summary->round_id,
        side_name(endpoint->args.side),
        summary->status == NB_STATUS_VALID ? "valid" : "invalid",
        summary->invalid_reason, protocol_name(endpoint->config.protocol),
        direction_name(endpoint->config.direction), endpoint->config.config_fingerprint,
        endpoint->config.duration_s, summary->tx_bytes, summary->tx_packets,
        summary->rx_bytes, summary->rx_packets, m->wire_tx_bytes, m->wire_rx_bytes,
        m->udp_offered, m->udp_accepted, m->udp_loss, m->udp_duplicate,
        m->udp_reorder, m->udp_corrupt, m->udp_late, endpoint->config.flow_count);
}

static int invalid_reason_from_io(int rc)
{
    if (rc == IO_EOF) return NB_INVALID_PEER_EOF;
    if (rc == IO_TIMEOUT) return NB_INVALID_TIMEOUT;
    if (rc == IO_CANCELLED) return NB_INVALID_CANCELLED;
    return NB_INVALID_PARTIAL;
}

static int fail_network(struct endpoint *endpoint, int reason,
                        int manifest_emitted)
{
    struct nb_summary summary;
    endpoint->metrics.invalid_reason = reason;
    if (!manifest_emitted) emit_manifest(endpoint);
    fill_summary(endpoint, &summary);
    emit_round(endpoint, &summary);
    return 2;
}

static int setup_tcp_flows(struct endpoint *endpoint)
{
    for (int i = 0; i < endpoint->args.flows; i++) {
        int fd = endpoint->args.mode == MODE_SERVER ?
            accept_socket(endpoint->listener_fd) :
            connect_socket(endpoint->args.addr, endpoint->args.port, SOCK_STREAM);
        if (fd < 0) return -1;
        if (!endpoint->args.nagle) {
            int one = 1;
            if (setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &one, sizeof(one))) {
                close(fd);
                return -1;
            }
        }
        endpoint->flows[i].fd = fd;
        endpoint->flows[i].id = (uint8_t)i;
    }
    return 0;
}

static int setup_udp(struct endpoint *endpoint)
{
    uint32_t registrations = 0;
    if (endpoint->args.mode == MODE_SERVER) {
        endpoint->udp_fd = make_listener(endpoint->args.port, SOCK_DGRAM);
        if (endpoint->udp_fd < 0) return -1;
        uint64_t deadline = now_ns() + IO_DEADLINE_NS;
        while (registrations != (uint32_t)((1U << endpoint->args.flows) - 1U)) {
            uint8_t registration[8];
            struct sockaddr_in peer;
            socklen_t peer_length = sizeof(peer);
            ssize_t received = recvfrom(endpoint->udp_fd, registration,
                sizeof(registration), 0, (struct sockaddr *)&peer, &peer_length);
            if (received < 0) {
                if ((errno == EAGAIN || errno == EWOULDBLOCK || errno == EINTR) &&
                    !poll_until(endpoint->udp_fd, POLLIN, deadline)) continue;
                return -1;
            }
            uint32_t magic;
            if (received != 8) return -1;
            memcpy(&magic, registration, 4);
            magic = ntohl(magic);
            if (magic != UDP_REGISTER_MAGIC || registration[4] >= endpoint->args.flows) return -1;
            registrations |= 1U << registration[4];
            endpoint->udp_peer = peer;
            endpoint->udp_peer_len = peer_length;
            endpoint->udp_peer_known = 1;
        }
    } else {
        endpoint->udp_fd = connect_socket(endpoint->args.addr, endpoint->args.port, SOCK_DGRAM);
        if (endpoint->udp_fd < 0) return -1;
        for (int i = 0; i < endpoint->args.flows; i++) {
            uint8_t registration[8] = {0};
            uint32_t magic = htonl(UDP_REGISTER_MAGIC);
            memcpy(registration, &magic, 4);
            registration[4] = (uint8_t)i;
            if (send_all(endpoint->udp_fd, registration, sizeof(registration),
                         now_ns() + IO_DEADLINE_NS)) return -1;
        }
    }
    for (int i = 0; i < endpoint->args.flows; i++) endpoint->flows[i].id = (uint8_t)i;
    return 0;
}

static int run_network(struct endpoint *endpoint)
{
    struct nb_frame frame;
    struct nb_summary local;
    struct nb_summary peer;
    int data_rc;
    int manifest_emitted = 0;
    int io_rc;
    if (endpoint->args.mode == MODE_SERVER) {
        endpoint->listener_fd = make_listener(endpoint->args.port, SOCK_STREAM);
        if (endpoint->listener_fd < 0)
            return fail_network(endpoint, NB_INVALID_PARTIAL, manifest_emitted);
        endpoint->control_fd = accept_socket(endpoint->listener_fd);
        if (endpoint->control_fd < 0)
            return fail_network(endpoint, NB_INVALID_PARTIAL, manifest_emitted);
        io_rc = recv_control(endpoint->control_fd, NB_FRAME_HELLO, &frame);
        if (io_rc)
            return fail_network(endpoint, invalid_reason_from_io(io_rc), manifest_emitted);
        if (frame.config.config_fingerprint != endpoint->config.config_fingerprint)
            return fail_network(endpoint, NB_INVALID_CONFIG_MISMATCH, manifest_emitted);
        io_rc = send_control(endpoint->control_fd, NB_FRAME_READY, endpoint, NULL);
        if (io_rc)
            return fail_network(endpoint, invalid_reason_from_io(io_rc), manifest_emitted);
    } else {
        endpoint->control_fd = connect_socket(endpoint->args.addr, endpoint->args.port, SOCK_STREAM);
        if (endpoint->control_fd < 0)
            return fail_network(endpoint, NB_INVALID_PARTIAL, manifest_emitted);
        io_rc = send_control(endpoint->control_fd, NB_FRAME_HELLO, endpoint, NULL);
        if (io_rc)
            return fail_network(endpoint, invalid_reason_from_io(io_rc), manifest_emitted);
        io_rc = recv_control(endpoint->control_fd, NB_FRAME_READY, &frame);
        if (io_rc)
            return fail_network(endpoint, invalid_reason_from_io(io_rc), manifest_emitted);
    }
    if (endpoint->args.protocol == NB_PROTO_TCP) {
        if (setup_tcp_flows(endpoint))
            return fail_network(endpoint, NB_INVALID_PARTIAL, manifest_emitted);
    } else if (setup_udp(endpoint))
        return fail_network(endpoint, NB_INVALID_PARTIAL, manifest_emitted);
    if (endpoint->args.mode == MODE_SERVER) {
        io_rc = send_control(endpoint->control_fd, NB_FRAME_START, endpoint, NULL);
    } else {
        io_rc = recv_control(endpoint->control_fd, NB_FRAME_START, &frame);
    }
    if (io_rc)
        return fail_network(endpoint, invalid_reason_from_io(io_rc), manifest_emitted);

    emit_manifest(endpoint);
    manifest_emitted = 1;
    data_rc = run_data(endpoint);
    if (data_rc && !endpoint->metrics.invalid_reason)
        endpoint->metrics.invalid_reason = NB_INVALID_PARTIAL;
    fill_summary(endpoint, &local);
    if (endpoint->args.mode == MODE_CLIENT) {
        io_rc = send_control(endpoint->control_fd, NB_FRAME_SUMMARY, endpoint, &local);
        if (io_rc)
            return fail_network(endpoint, invalid_reason_from_io(io_rc), manifest_emitted);
        io_rc = recv_control(endpoint->control_fd, NB_FRAME_SUMMARY, &frame);
        if (io_rc)
            return fail_network(endpoint, invalid_reason_from_io(io_rc), manifest_emitted);
        peer = frame.summary;
    } else {
        io_rc = recv_control(endpoint->control_fd, NB_FRAME_SUMMARY, &frame);
        if (io_rc)
            return fail_network(endpoint, invalid_reason_from_io(io_rc), manifest_emitted);
        peer = frame.summary;
        if (summaries_close(endpoint, &local, &peer)) {
            local.status = NB_STATUS_INVALID;
            local.invalid_reason = NB_INVALID_PARTIAL;
        }
        io_rc = send_control(endpoint->control_fd, NB_FRAME_SUMMARY, endpoint, &local);
        if (io_rc)
            return fail_network(endpoint, invalid_reason_from_io(io_rc), manifest_emitted);
    }
    if (summaries_close(endpoint, &local, &peer)) {
        local.status = NB_STATUS_INVALID;
        local.invalid_reason = NB_INVALID_PARTIAL;
    }
    emit_round(endpoint, &local);
    return local.status == NB_STATUS_VALID ? 0 : 2;
}

static int simulate_direction(struct endpoint *guest, struct endpoint *host,
                              int sender_is_guest)
{
    struct endpoint *sender = sender_is_guest ? guest : host;
    struct endpoint *receiver = sender_is_guest ? host : guest;
    for (int flow_id = 0; flow_id < sender->args.flows; flow_id++) {
        struct flow *flow = &sender->flows[flow_id];
        struct flow *peer_flow = &receiver->flows[flow_id];
        flow->id = (uint8_t)flow_id;
        peer_flow->id = (uint8_t)flow_id;
        for (int record = 0; record < 128; record++) {
            if (prepare_record(sender, flow)) return -1;
            if (sender->config.protocol == NB_PROTO_UDP) {
                sender->metrics.udp_offered++;
                sender->metrics.udp_accepted++;
            }
            sender->metrics.tx_bytes += sender->config.payload_size;
            sender->metrics.tx_packets++;
            sender->metrics.wire_tx_bytes += flow->tx.length;
            if (accept_record(receiver, peer_flow, flow->tx.wire, flow->tx.length,
                              sender->config.protocol == NB_PROTO_UDP)) return -1;
            flow->tx.sequence++;
            flow->tx.length = 0;
        }
    }
    return 0;
}

static int run_loopback(struct endpoint *prototype)
{
    struct endpoint guest = *prototype;
    struct endpoint host = *prototype;
    struct nb_summary guest_summary;
    struct nb_summary host_summary;
    guest.args.side = SIDE_GUEST;
    host.args.side = SIDE_HOST;
    build_config(&guest);
    build_config(&host);
    emit_manifest(&guest);
    emit_manifest(&host);
    if ((prototype->args.direction == NB_DIR_TX || prototype->args.direction == NB_DIR_BIDI) &&
        simulate_direction(&guest, &host, 1)) return 1;
    if ((prototype->args.direction == NB_DIR_RX || prototype->args.direction == NB_DIR_BIDI) &&
        simulate_direction(&guest, &host, 0)) return 1;
    fill_summary(&guest, &guest_summary);
    fill_summary(&host, &host_summary);
    if (summaries_close(&guest, &guest_summary, &host_summary) ||
        summaries_close(&host, &host_summary, &guest_summary)) return 1;
    emit_round(&guest, &guest_summary);
    emit_round(&host, &host_summary);
    return 0;
}

static int run_self_test(void)
{
    struct endpoint endpoint;
    struct endpoint udp_endpoint;
    struct flow flow;
    struct flow udp_flow;
    struct nb_summary local;
    struct nb_summary peer;
    size_t split;
    memset(&endpoint, 0, sizeof(endpoint));
    memset(&flow, 0, sizeof(flow));
    endpoint.args.protocol = NB_PROTO_TCP;
    endpoint.args.direction = NB_DIR_TX;
    endpoint.args.flows = 1;
    endpoint.args.payload = 64;
    endpoint.args.duration = 1;
    endpoint.args.seed = 7;
    endpoint.args.run_id = 1;
    endpoint.args.test_id = 1;
    endpoint.args.round_id = 1;
    build_config(&endpoint);
    flow.id = 0;
    if (prepare_record(&endpoint, &flow)) return 1;
    split = flow.tx.length / 3;
    memcpy(flow.rx.bytes, flow.tx.wire, split);
    flow.rx.length = split;
    if (consume_tcp(&endpoint, &flow) || endpoint.metrics.rx_packets) return 1;
    memcpy(flow.rx.bytes + flow.rx.length, flow.tx.wire + split,
           flow.tx.length - split);
    flow.rx.length = flow.tx.length;
    if (consume_tcp(&endpoint, &flow) || endpoint.metrics.rx_packets != 1 ||
        endpoint.metrics.rx_bytes != 64 || flow.rx.length != 0) return 1;
    flow.tx.offset = split;
    if (flow.tx.offset >= flow.tx.length) return 1;

    fill_summary(&endpoint, &local);
    peer = local;
    peer.config_fingerprint++;
    if (!summaries_close(&endpoint, &local, &peer)) return 1;
    endpoint.metrics.invalid_reason = NB_INVALID_PEER_EOF;
    fill_summary(&endpoint, &local);
    if (local.invalid_reason != NB_INVALID_PEER_EOF) return 1;
    endpoint.metrics.invalid_reason = NB_INVALID_TIMEOUT;
    fill_summary(&endpoint, &local);
    if (local.invalid_reason != NB_INVALID_TIMEOUT) return 1;
    endpoint.metrics.invalid_reason = NB_INVALID_CANCELLED;
    fill_summary(&endpoint, &local);
    if (local.invalid_reason != NB_INVALID_CANCELLED) return 1;
    if (invalid_reason_from_io(IO_EOF) != NB_INVALID_PEER_EOF ||
        invalid_reason_from_io(IO_TIMEOUT) != NB_INVALID_TIMEOUT ||
        invalid_reason_from_io(IO_CANCELLED) != NB_INVALID_CANCELLED)
        return 1;

    memset(&udp_endpoint, 0, sizeof(udp_endpoint));
    memset(&udp_flow, 0, sizeof(udp_flow));
    udp_endpoint.args.protocol = NB_PROTO_UDP;
    udp_endpoint.args.direction = NB_DIR_TX;
    udp_endpoint.args.flows = 1;
    udp_endpoint.args.payload = 64;
    udp_endpoint.args.duration = 1;
    udp_endpoint.args.seed = 7;
    udp_endpoint.args.run_id = 1;
    udp_endpoint.args.test_id = 2;
    udp_endpoint.args.round_id = 1;
    build_config(&udp_endpoint);
    udp_flow.id = 0;
    if (prepare_record(&udp_endpoint, &udp_flow)) return 1;
    if (accept_record(&udp_endpoint, &udp_flow, udp_flow.tx.wire,
                      udp_flow.tx.length, 1)) return 1;
    if (accept_record(&udp_endpoint, &udp_flow, udp_flow.tx.wire,
                      udp_flow.tx.length, 1)) return 1;
    udp_flow.tx.sequence = 2;
    udp_flow.tx.length = 0;
    if (prepare_record(&udp_endpoint, &udp_flow) ||
        accept_record(&udp_endpoint, &udp_flow, udp_flow.tx.wire,
                      udp_flow.tx.length, 1)) return 1;
    udp_endpoint.data_end_ns = 1;
    udp_flow.tx.sequence = 3;
    udp_flow.tx.length = 0;
    if (prepare_record(&udp_endpoint, &udp_flow) ||
        accept_record(&udp_endpoint, &udp_flow, udp_flow.tx.wire,
                      udp_flow.tx.length, 1)) return 1;
    udp_endpoint.data_end_ns = 0;
    udp_flow.tx.sequence = 4;
    udp_flow.tx.length = 0;
    if (prepare_record(&udp_endpoint, &udp_flow)) return 1;
    udp_flow.tx.wire[udp_flow.tx.length - 1] ^= 1U;
    if (!accept_record(&udp_endpoint, &udp_flow, udp_flow.tx.wire,
                       udp_flow.tx.length, 1)) return 1;
    if (udp_endpoint.metrics.udp_duplicate != 1 ||
        udp_endpoint.metrics.udp_loss != 1 ||
        udp_endpoint.metrics.udp_reorder != 1 ||
        udp_endpoint.metrics.udp_late != 1 ||
        udp_endpoint.metrics.udp_corrupt != 1) return 1;
    puts("SELF-TEST PASS partial-record state codec deadline-policy "
         "faults=config-mismatch,peer-eof,timeout,cancel,udp-anomalies");
    return 0;
}

static int run_calibration(void)
{
    uint64_t minimum = UINT64_MAX;
    uint64_t maximum = 0;
    uint64_t total = 0;
    struct nb_instret_result instret;
    for (int i = 0; i < 1000; i++) {
        uint64_t begin = now_ns();
        uint64_t end = now_ns();
        if (end < begin) return 1;
        uint64_t delta = end - begin;
        if (delta < minimum) minimum = delta;
        if (delta > maximum) maximum = delta;
        total += delta;
    }
    int instret_rc = nb_instret_read(&instret);
    if (instret_rc == 0 && instret.available) {
        json_line("{\"schema_version\":1,\"type\":\"calibration\"," 
            "\"monotonic_samples\":1000,\"monotonic_min_ns\":%" PRIu64 ","
            "\"monotonic_mean_ns\":%" PRIu64 ",\"monotonic_max_ns\":%" PRIu64 ","
            "\"instret_status\":\"available\",\"instret_begin\":%" PRIu64 ","
            "\"instret_end\":%" PRIu64 ",\"instret_overhead\":%" PRIu64 "}",
            minimum, total / 1000, maximum,
            instret.begin, instret.end, instret.overhead);
    } else {
        json_line("{\"schema_version\":1,\"type\":\"calibration\"," 
            "\"monotonic_samples\":1000,\"monotonic_min_ns\":%" PRIu64 ","
            "\"monotonic_mean_ns\":%" PRIu64 ",\"monotonic_max_ns\":%" PRIu64 ","
            "\"instret_status\":\"unavailable\",\"instret_begin\":null,"
            "\"instret_end\":null,\"instret_overhead\":null}",
            minimum, total / 1000, maximum);
    }
    return 0;
}

static void close_endpoint(struct endpoint *endpoint)
{
    for (int i = 0; i < endpoint->args.flows; i++)
        if (endpoint->flows[i].fd >= 0) close(endpoint->flows[i].fd);
    if (endpoint->control_fd >= 0) close(endpoint->control_fd);
    if (endpoint->listener_fd >= 0) close(endpoint->listener_fd);
    if (endpoint->udp_fd >= 0) close(endpoint->udp_fd);
}

int main(int argc, char **argv)
{
    struct endpoint endpoint;
    int rc;
    memset(&endpoint, 0, sizeof(endpoint));
    endpoint.control_fd = endpoint.listener_fd = endpoint.udp_fd = -1;
    for (int i = 0; i < MAX_FLOWS; i++) endpoint.flows[i].fd = -1;
    signal(SIGINT, on_signal);
    signal(SIGTERM, on_signal);
    if (parse_args(argc, argv, &endpoint.args)) {
        usage(argv[0]);
        return 1;
    }
    if (endpoint.args.mode == MODE_SELF_TEST) return run_self_test();
    if (endpoint.args.mode == MODE_CALIBRATE) return run_calibration();
    build_config(&endpoint);
    if (endpoint.args.print_config) {
        emit_manifest(&endpoint);
        return 0;
    }
    if (endpoint.args.mode == MODE_LOOPBACK) return run_loopback(&endpoint);
    rc = run_network(&endpoint);
    close_endpoint(&endpoint);
    return rc ? 1 : 0;
}
