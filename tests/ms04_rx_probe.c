#define _DEFAULT_SOURCE
#define _POSIX_C_SOURCE 200809L

#include <arpa/inet.h>
#include <errno.h>
#include <stddef.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

#define MS04_SNAPSHOT_V2 0x4e494432
#define MS04_SOFTWARE_NUDGE 0x4e494e31
#define MS04_HOST "10.0.2.2"
#define MS04_PORT 15556
#define MS04_BURST_COUNT 96u
#define MS04_PAYLOAD_SIZE 64u
#define MS04_WIRE_MAGIC 0x4d533034u
#define MS04_IDLE_MS 250u
#define MS04_STABLE_TIMEOUT_MS 1000u

struct ms04_snapshot {
    uint64_t total;
    uint64_t used_ring;
    uint64_t config_change;
    uint64_t combined;
    uint64_t unknown;
    uint64_t spurious;
    uint64_t ack_count;
    uint64_t uart_irq_count;
    uint64_t restore_violation;
    uint64_t irq_enabled_entry;
    uint64_t rx_lifecycle;
    uint64_t rx_owner;
    uint64_t isr_publish;
    uint64_t isr_wake;
    uint64_t software_nudge;
    uint64_t task_poll;
    uint64_t reaped;
    uint64_t refilled;
    uint64_t delivered;
    uint64_t non_ip_consumed;
    uint64_t budget_exhausted;
    uint64_t self_yield;
    uint64_t router_full_wait;
    uint64_t space_wake;
    uint64_t empty_check;
    uint64_t fault;
    uint64_t last_error_stage;
    uint64_t last_error_code;
};

_Static_assert(sizeof(struct ms04_snapshot) == 28 * sizeof(uint64_t),
               "MS04 V2 snapshot must remain 224 bytes");
_Static_assert(offsetof(struct ms04_snapshot, total) == 0,
               "V2 must preserve the V1 prefix");
_Static_assert(offsetof(struct ms04_snapshot, uart_irq_count) == 7 * sizeof(uint64_t),
               "V2 must preserve the V1 prefix");
_Static_assert(offsetof(struct ms04_snapshot, restore_violation) == 8 * sizeof(uint64_t),
               "V2 restore offset changed");
_Static_assert(offsetof(struct ms04_snapshot, software_nudge) == 14 * sizeof(uint64_t),
               "V2 nudge offset changed");
_Static_assert(offsetof(struct ms04_snapshot, last_error_code) == 27 * sizeof(uint64_t),
               "V2 tail offset changed");

struct ms04_wire_header {
    uint32_t magic;
    uint32_t sequence;
    uint32_t count;
};

#define DELTA_FIELD(field)                                                        \
    do {                                                                          \
        if (post->field < pre->field) return -1;                                  \
        delta->field = post->field - pre->field;                                  \
    } while (0)

static int snapshot_delta(const struct ms04_snapshot *pre,
                          const struct ms04_snapshot *post,
                          struct ms04_snapshot *delta)
{
    memset(delta, 0, sizeof(*delta));
    DELTA_FIELD(total);
    DELTA_FIELD(used_ring);
    DELTA_FIELD(config_change);
    DELTA_FIELD(combined);
    DELTA_FIELD(unknown);
    DELTA_FIELD(spurious);
    DELTA_FIELD(ack_count);
    DELTA_FIELD(uart_irq_count);
    DELTA_FIELD(restore_violation);
    DELTA_FIELD(irq_enabled_entry);
    DELTA_FIELD(isr_publish);
    DELTA_FIELD(isr_wake);
    DELTA_FIELD(software_nudge);
    DELTA_FIELD(task_poll);
    DELTA_FIELD(reaped);
    DELTA_FIELD(refilled);
    DELTA_FIELD(delivered);
    DELTA_FIELD(non_ip_consumed);
    DELTA_FIELD(budget_exhausted);
    DELTA_FIELD(self_yield);
    DELTA_FIELD(router_full_wait);
    DELTA_FIELD(space_wake);
    DELTA_FIELD(empty_check);
    DELTA_FIELD(fault);
    return 0;
}

static int snapshot_progress_equal(const struct ms04_snapshot *a,
                                   const struct ms04_snapshot *b)
{
    return a->isr_publish == b->isr_publish &&
           a->isr_wake == b->isr_wake &&
           a->software_nudge == b->software_nudge &&
           a->task_poll == b->task_poll &&
           a->reaped == b->reaped &&
           a->refilled == b->refilled &&
           a->delivered == b->delivered &&
           a->non_ip_consumed == b->non_ip_consumed &&
           a->budget_exhausted == b->budget_exhausted &&
           a->self_yield == b->self_yield &&
           a->router_full_wait == b->router_full_wait &&
           a->space_wake == b->space_wake &&
           a->empty_check == b->empty_check &&
           a->fault == b->fault;
}

static int stable_deadline_expired(uint64_t start, uint64_t now)
{
    return now < start || now - start >= MS04_STABLE_TIMEOUT_MS;
}

static int snapshot_active(const struct ms04_snapshot *snapshot)
{
    return snapshot->rx_lifecycle == 2 && snapshot->rx_owner == 1;
}

static int common_delta_valid(const struct ms04_snapshot *post,
                              const struct ms04_snapshot *delta)
{
    return snapshot_active(post) &&
           delta->fault == 0 &&
           delta->restore_violation == 0 &&
           delta->irq_enabled_entry == 0;
}

static int validate_idle(const struct ms04_snapshot *post,
                         const struct ms04_snapshot *delta)
{
    return common_delta_valid(post, delta) &&
           delta->reaped == 0 && delta->refilled == 0 &&
           delta->delivered == 0 && delta->non_ip_consumed == 0 &&
           delta->budget_exhausted == 0 && delta->self_yield == 0 &&
           delta->router_full_wait == 0 && delta->space_wake == 0 &&
           delta->task_poll <= 1 && delta->empty_check <= 1;
}

static int validate_nudge(const struct ms04_snapshot *post,
                          const struct ms04_snapshot *delta)
{
    return common_delta_valid(post, delta) &&
           delta->software_nudge == 1 &&
           delta->isr_publish == 0 && delta->isr_wake == 0 &&
           delta->task_poll == 1 && delta->empty_check == 1 &&
           delta->reaped == 0 && delta->refilled == 0 &&
           delta->budget_exhausted == 0 && delta->self_yield == 0;
}

static int validate_burst(const struct ms04_snapshot *post,
                          const struct ms04_snapshot *delta,
                          uint32_t received,
                          uint32_t expected)
{
    return common_delta_valid(post, delta) && received == expected &&
           delta->isr_publish > 0 && delta->isr_wake > 0 &&
           delta->task_poll > 0 && delta->reaped > 0 &&
           delta->reaped == delta->refilled &&
           delta->budget_exhausted > 0 && delta->self_yield > 0;
}

#ifndef MS04_RX_PROBE_TESTING

static int monotonic_ms(uint64_t *now)
{
    struct timespec ts;
    if (clock_gettime(CLOCK_MONOTONIC, &ts) != 0) return -1;
    *now = (uint64_t)ts.tv_sec * 1000u + (uint64_t)ts.tv_nsec / 1000000u;
    return 0;
}

static int read_snapshot(struct ms04_snapshot *snapshot)
{
    if (ioctl(STDIN_FILENO, MS04_SNAPSHOT_V2, snapshot) < 0) {
        perror("ioctl MS04_SNAPSHOT_V2");
        return -1;
    }
    return 0;
}

static int read_stable_snapshot(struct ms04_snapshot *snapshot)
{
    struct ms04_snapshot previous;
    struct ms04_snapshot current;
    uint64_t start;
    uint64_t now;

    if (monotonic_ms(&start) != 0 || read_snapshot(&previous) != 0) return -1;
    for (;;) {
        usleep(20000);
        if (read_snapshot(&current) != 0) return -1;
        if (snapshot_progress_equal(&previous, &current)) {
            *snapshot = current;
            return 0;
        }
        previous = current;
        if (monotonic_ms(&now) != 0 || stable_deadline_expired(start, now)) break;
    }
    return -1;
}

static void print_snapshot(const char *label, const struct ms04_snapshot *s)
{
    printf("%s total=%lu used=%lu config=%lu combined=%lu unknown=%lu "
           "spurious=%lu ack=%lu uart=%lu restore=%lu irq_entry=%lu "
           "lifecycle=%lu owner=%lu isr_publish=%lu isr_wake=%lu nudge=%lu "
           "task=%lu reaped=%lu refilled=%lu delivered=%lu non_ip=%lu "
           "budget=%lu yield=%lu router_full=%lu space_wake=%lu empty=%lu "
           "fault=%lu err_stage=%lu err_code=%lu\n",
           label, (unsigned long)s->total, (unsigned long)s->used_ring,
           (unsigned long)s->config_change, (unsigned long)s->combined,
           (unsigned long)s->unknown, (unsigned long)s->spurious,
           (unsigned long)s->ack_count, (unsigned long)s->uart_irq_count,
           (unsigned long)s->restore_violation,
           (unsigned long)s->irq_enabled_entry,
           (unsigned long)s->rx_lifecycle, (unsigned long)s->rx_owner,
           (unsigned long)s->isr_publish, (unsigned long)s->isr_wake,
           (unsigned long)s->software_nudge, (unsigned long)s->task_poll,
           (unsigned long)s->reaped, (unsigned long)s->refilled,
           (unsigned long)s->delivered, (unsigned long)s->non_ip_consumed,
           (unsigned long)s->budget_exhausted, (unsigned long)s->self_yield,
           (unsigned long)s->router_full_wait, (unsigned long)s->space_wake,
           (unsigned long)s->empty_check, (unsigned long)s->fault,
           (unsigned long)s->last_error_stage,
           (unsigned long)s->last_error_code);
}

static void print_delta(const struct ms04_snapshot *d)
{
    printf("MS04 DELTA isr_publish=%lu isr_wake=%lu nudge=%lu task=%lu "
           "reaped=%lu refilled=%lu delivered=%lu non_ip=%lu budget=%lu "
           "yield=%lu router_full=%lu space_wake=%lu empty=%lu fault=%lu "
           "restore=%lu irq_entry=%lu\n",
           (unsigned long)d->isr_publish, (unsigned long)d->isr_wake,
           (unsigned long)d->software_nudge, (unsigned long)d->task_poll,
           (unsigned long)d->reaped, (unsigned long)d->refilled,
           (unsigned long)d->delivered, (unsigned long)d->non_ip_consumed,
           (unsigned long)d->budget_exhausted, (unsigned long)d->self_yield,
           (unsigned long)d->router_full_wait, (unsigned long)d->space_wake,
           (unsigned long)d->empty_check, (unsigned long)d->fault,
           (unsigned long)d->restore_violation,
           (unsigned long)d->irq_enabled_entry);
}

static int finish_mode(const char *mode,
                       const struct ms04_snapshot *pre,
                       const struct ms04_snapshot *post,
                       int valid)
{
    struct ms04_snapshot delta;
    int monotonic = snapshot_delta(pre, post, &delta) == 0;
    print_snapshot("MS04 PRE", pre);
    print_snapshot("MS04 POST", post);
    if (monotonic) print_delta(&delta);
    printf("MS04 %s mode=%s\n", monotonic && valid ? "PASS" : "FAIL", mode);
    return monotonic && valid ? 0 : 1;
}

static int fail_mode(const char *mode, const char *reason)
{
    printf("MS04 FAIL mode=%s reason=%s\n", mode, reason);
    return 1;
}

static int run_snapshot(void)
{
    struct ms04_snapshot pre, post, delta;
    if (read_snapshot(&pre) != 0 || read_stable_snapshot(&post) != 0)
        return fail_mode("snapshot", "snapshot-read");
    if (snapshot_delta(&pre, &post, &delta) != 0) return finish_mode("snapshot", &pre, &post, 0);
    return finish_mode("snapshot", &pre, &post, common_delta_valid(&post, &delta));
}

static int run_idle(void)
{
    struct ms04_snapshot pre, post, delta;
    if (read_stable_snapshot(&pre) != 0) return fail_mode("idle", "pre-snapshot");
    usleep(MS04_IDLE_MS * 1000u);
    if (read_stable_snapshot(&post) != 0) return fail_mode("idle", "post-snapshot");
    if (snapshot_delta(&pre, &post, &delta) != 0) return finish_mode("idle", &pre, &post, 0);
    return finish_mode("idle", &pre, &post, validate_idle(&post, &delta));
}

static int run_nudge(void)
{
    struct ms04_snapshot pre, post, delta;
    if (read_stable_snapshot(&pre) != 0) return fail_mode("nudge", "pre-snapshot");
    if (!snapshot_active(&pre)) return fail_mode("nudge", "inactive");
    if (ioctl(STDIN_FILENO, MS04_SOFTWARE_NUDGE, 0) < 0) {
        perror("ioctl MS04_SOFTWARE_NUDGE");
        return fail_mode("nudge", "ioctl");
    }
    if (read_stable_snapshot(&post) != 0) return fail_mode("nudge", "post-snapshot");
    if (snapshot_delta(&pre, &post, &delta) != 0) return finish_mode("nudge", &pre, &post, 0);
    return finish_mode("nudge", &pre, &post, validate_nudge(&post, &delta));
}

static int validate_datagram(const uint8_t *packet, ssize_t length,
                             uint32_t sequence, uint32_t count,
                             uint32_t payload_size)
{
    struct ms04_wire_header header;
    if (length != (ssize_t)(sizeof(header) + payload_size)) return -1;
    memcpy(&header, packet, sizeof(header));
    if (ntohl(header.magic) != MS04_WIRE_MAGIC ||
        ntohl(header.sequence) != sequence || ntohl(header.count) != count) return -1;
    for (uint32_t i = 0; i < payload_size; ++i) {
        if (packet[sizeof(header) + i] != (uint8_t)((sequence + i) & 0xffu)) return -1;
    }
    return 0;
}

static int run_burst(void)
{
    int fd = -1;
    int result = 1;
    int reported = 0;
    struct sockaddr_in host = {0};
    struct timeval timeout = {.tv_sec = 3, .tv_usec = 0};
    char control[96];
    uint8_t packet[sizeof(struct ms04_wire_header) + MS04_PAYLOAD_SIZE];
    struct ms04_snapshot pre, post, delta;
    uint32_t received = 0;

    fd = socket(AF_INET, SOCK_DGRAM, 0);
    if (fd < 0) {
        perror("socket");
        return fail_mode("burst", "socket");
    }
    if (setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &timeout, sizeof(timeout)) != 0)
        goto out;
    host.sin_family = AF_INET;
    host.sin_port = htons(MS04_PORT);
    if (inet_pton(AF_INET, MS04_HOST, &host.sin_addr) != 1 ||
        connect(fd, (struct sockaddr *)&host, sizeof(host)) != 0) goto out;

    snprintf(control, sizeof(control), "MS04 REGISTER %u %u", MS04_BURST_COUNT,
             MS04_PAYLOAD_SIZE);
    if (send(fd, control, strlen(control), 0) < 0) goto out;
    ssize_t n = recv(fd, control, sizeof(control) - 1, 0);
    if (n <= 0) goto out;
    control[n] = '\0';
    if (strcmp(control, "MS04 READY 96 64") != 0) goto out;

    if (read_stable_snapshot(&pre) != 0 || !snapshot_active(&pre)) goto out;
    snprintf(control, sizeof(control), "MS04 START %u %u", MS04_BURST_COUNT,
             MS04_PAYLOAD_SIZE);
    if (send(fd, control, strlen(control), 0) < 0) goto out;

    for (uint32_t sequence = 0; sequence < MS04_BURST_COUNT; ++sequence) {
        n = recv(fd, packet, sizeof(packet), 0);
        if (validate_datagram(packet, n, sequence, MS04_BURST_COUNT,
                              MS04_PAYLOAD_SIZE) != 0) goto out;
        received++;
    }
    if (read_stable_snapshot(&post) != 0 || snapshot_delta(&pre, &post, &delta) != 0) goto out;
    result = finish_mode("burst", &pre, &post,
                         validate_burst(&post, &delta, received, MS04_BURST_COUNT));
    reported = 1;
out:
    close(fd);
    if (result != 0 && !reported)
        printf("MS04 FAIL mode=burst reason=protocol received=%u\n", received);
    return result;
}

int main(int argc, char **argv)
{
    if (argc != 2) {
        fprintf(stderr, "usage: %s snapshot|idle|nudge|burst\n", argv[0]);
        return 2;
    }
    if (strcmp(argv[1], "snapshot") == 0) return run_snapshot();
    if (strcmp(argv[1], "idle") == 0) return run_idle();
    if (strcmp(argv[1], "nudge") == 0) return run_nudge();
    if (strcmp(argv[1], "burst") == 0) return run_burst();
    fprintf(stderr, "unknown mode: %s\n", argv[1]);
    return 2;
}

#endif
