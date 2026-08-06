/* MS16 network benchmark protocol — RED test suite.
 *
 * Build (host):
 *   cc -std=c11 -Wall -Wextra -Werror \
 *     tests/network_benchmark_protocol_test.c \
 *     tests/network_benchmark_protocol.c \
 *     -o /tmp/network-benchmark-protocol-test
 *
 * RED state: tests/network_benchmark_protocol.c is absent,
 * so the build command fails — this is the expected RED witness.
 *
 * GREEN: after protocol.c exists, /tmp/network-benchmark-protocol-test exits 0
 *   and all assertions pass.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>
#include <inttypes.h>
#include <assert.h>

#include "network_benchmark_protocol.h"

/* ── helpers ─────────────────────────────────────────────────────────── */

static int failures = 0;

#define CHECK(cond, msg) do { \
    if (!(cond)) { \
        fprintf(stderr, "FAIL: %s:%d: %s\n", __FILE__, __LINE__, msg); \
        failures++; \
    } \
} while (0)

#define STREQ(a, b, msg) CHECK(strcmp((a), (b)) == 0, msg)

/* ── magic / version / type ──────────────────────────────────────────── */

static void test_magic_mismatch(void)
{
    uint8_t buf[256];
    memset(buf, 0, sizeof(buf));
    /* write a magic that is NOT the expected one */
    uint32_t bad_magic = NB_PROTO_MAGIC ^ 0xDEADBEEF;
    uint32_t be = nb_hton32(bad_magic);
    memcpy(buf, &be, 4);

    struct nb_frame frame;
    int rc = nb_frame_decode(&frame, buf, 4);
    CHECK(rc < 0, "magic mismatch should fail decode");
}

static void test_version_mismatch(void)
{
    uint8_t buf[NB_FRAME_MAX];
    memset(buf, 0, sizeof(buf));
    uint32_t magic = nb_hton32(NB_PROTO_MAGIC);
    memcpy(buf, &magic, 4);
    buf[4] = NB_PROTO_VERSION + 1;  /* wrong version */

    struct nb_frame frame;
    int rc = nb_frame_decode(&frame, buf, 5);
    CHECK(rc < 0, "version mismatch should fail decode");
}

static void test_invalid_frame_type(void)
{
    uint8_t buf[NB_FRAME_MAX];
    memset(buf, 0, sizeof(buf));
    uint32_t magic = nb_hton32(NB_PROTO_MAGIC);
    memcpy(buf, &magic, 4);
    buf[4] = NB_PROTO_VERSION;
    buf[5] = 0xFF;  /* invalid type */

    struct nb_frame frame;
    int rc = nb_frame_decode(&frame, buf, NB_FRAME_MIN);
    CHECK(rc < 0, "invalid frame type should fail decode");
}

/* ── frame encoding round-trip ───────────────────────────────────────── */

static void test_hello_roundtrip(void)
{
    struct nb_config cfg;
    memset(&cfg, 0, sizeof(cfg));
    cfg.role = NB_ROLE_SENDER;
    cfg.test_id = 10;
    cfg.protocol = NB_PROTO_TCP;
    cfg.direction = NB_DIR_TX;
    cfg.flow_count = 1;
    cfg.payload_size = 1400;
    cfg.duration_s = 10;
    cfg.seed = 0x12345678;
    cfg.warmup_s = 2;
    cfg.run_id = 99;
    cfg.round_id = 7;
    cfg.capability_bitmap = 3;

    uint64_t fprint = nb_config_fingerprint(&cfg);
    cfg.config_fingerprint = fprint;
    CHECK(fprint != 0, "fingerprint should be non-zero");
    CHECK(fprint == nb_config_fingerprint(&cfg),
          "fingerprint should be deterministic");

    uint8_t buf[NB_FRAME_MAX];
    size_t len = NB_FRAME_MAX;
    int rc = nb_hello_encode(buf, &len, &cfg);
    CHECK(rc == 0, "hello encode should succeed");
    CHECK(len >= NB_FRAME_MIN, "encoded length should meet minimum");
    CHECK(len <= NB_FRAME_MAX, "encoded length should not exceed maximum");

    struct nb_frame frame;
    rc = nb_frame_decode(&frame, buf, len);
    CHECK(rc >= 0, "roundtrip decode should succeed");

    CHECK(frame.type == NB_FRAME_HELLO, "roundtrip type should be HELLO");
    CHECK(frame.version == NB_PROTO_VERSION, "roundtrip version should match");
    CHECK(frame.config.role == NB_ROLE_SENDER, "roundtrip role should match");
    CHECK(frame.config.test_id == 10, "roundtrip test_id should match");
    CHECK(frame.config.protocol == NB_PROTO_TCP, "roundtrip protocol should match");
    CHECK(frame.config.flow_count == 1, "roundtrip flow_count should match");
    CHECK(frame.config.payload_size == 1400, "roundtrip payload_size should match");
    CHECK(frame.config.duration_s == 10, "roundtrip duration_s should match");
    CHECK(frame.config.run_id == 99, "roundtrip run_id should match");
    CHECK(frame.config.round_id == 7, "roundtrip round_id should match");
    CHECK(frame.config.capability_bitmap == 3, "roundtrip capabilities should match");
}

static void test_summary_roundtrip(void)
{
    struct nb_summary sum;
    memset(&sum, 0, sizeof(sum));
    sum.completion_point = NB_CP_C6;
    sum.status = NB_STATUS_VALID;
    sum.rx_bytes = 14000000;
    sum.rx_packets = 10000;
    sum.rtt_min_us = 50;
    sum.rtt_p50_us = 80;
    sum.rtt_p99_us = 500;
    sum.rtt_max_us = 2000;
    sum.run_id = 99;
    sum.test_id = 10;
    sum.round_id = 7;
    sum.config_fingerprint = 123;

    uint8_t buf[NB_FRAME_MAX];
    size_t len = NB_FRAME_MAX;
    int rc = nb_summary_encode(buf, &len, &sum);
    CHECK(rc == 0, "summary encode should succeed");

    struct nb_frame frame;
    rc = nb_frame_decode(&frame, buf, len);
    CHECK(rc >= 0, "summary decode should succeed");
    CHECK(frame.type == NB_FRAME_SUMMARY, "should decode as SUMMARY");
    CHECK(frame.summary.completion_point == NB_CP_C6, "C6 should survive roundtrip");
    CHECK(frame.summary.rx_bytes == 14000000, "rx_bytes should survive roundtrip");
    CHECK(frame.summary.config_fingerprint == 123, "summary fingerprint should survive");
}

/* ── bounds checking ─────────────────────────────────────────────────── */

static void test_frame_too_small(void)
{
    uint8_t buf[2] = {0, 0};
    struct nb_frame frame;
    int rc = nb_frame_decode(&frame, buf, 2);
    CHECK(rc < 0, "too-small frame should fail");
}

static void test_body_length_exceeds_maximum(void)
{
    uint8_t buf[NB_FRAME_MAX];
    memset(buf, 0, sizeof(buf));
    uint32_t magic = nb_hton32(NB_PROTO_MAGIC);
    memcpy(buf, &magic, 4);
    buf[4] = NB_PROTO_VERSION;
    buf[5] = NB_FRAME_HELLO;

    /* set body_length past the protocol limit */
    uint16_t huge_len = nb_hton16(NB_FRAME_BODY_MAX + 1);
    memcpy(buf + NB_FRAME_BODY_LEN_OFF, &huge_len, 2);

    struct nb_frame frame;
    int rc = nb_frame_decode(&frame, buf, NB_FRAME_MIN);
    CHECK(rc < 0, "body_length > NB_FRAME_BODY_MAX should fail");
}

static void test_truncated_frame(void)
{
    /* only give the header, not the body */
    uint8_t buf[NB_FRAME_MIN + 10];
    memset(buf, 0, sizeof(buf));
    uint32_t magic = nb_hton32(NB_PROTO_MAGIC);
    memcpy(buf, &magic, 4);
    buf[4] = NB_PROTO_VERSION;
    buf[5] = NB_FRAME_HELLO;
    uint16_t body_len = nb_hton16(20);
    memcpy(buf + NB_FRAME_BODY_LEN_OFF, &body_len, 2);

    struct nb_frame frame;
    int rc = nb_frame_decode(&frame, buf, NB_FRAME_MIN);
    CHECK(rc < 0, "truncated frame (header only, body missing) should fail");
}

static void test_frame_trailing_bytes_rejected(void)
{
    struct nb_config cfg;
    memset(&cfg, 0, sizeof(cfg));
    uint8_t buf[NB_FRAME_MAX];
    size_t len = sizeof(buf);
    CHECK(nb_hello_encode(buf, &len, &cfg) == 0, "hello for trailing test should encode");
    buf[len++] = 0xA5;
    struct nb_frame frame;
    CHECK(nb_frame_decode(&frame, buf, len) < 0, "trailing bytes should fail exact decode");
}

static void test_generator_non_aligned_offset(void)
{
    /* Verify generator is deterministic and offset-consistent.
     * LCG-based generator: nb_generator_fill(buf, len, seed, flow, seq, offset).
     * Generate full buffer then verify at each offset the partial matches. */
    uint8_t full[64], part[23];
    nb_generator_fill(full, sizeof(full), 0x3333, 1, 0, 0);
    nb_generator_fill(part, sizeof(part), 0x3333, 1, 0, 7);
    CHECK(memcmp(full + 7, part, sizeof(part)) == 0,
          "non-aligned offset must match contiguous fill");
}


static void test_fingerprint_excludes_role(void)
{
    struct nb_config sender, receiver;
    memset(&sender, 0, sizeof(sender));
    receiver = sender;
    sender.role = NB_ROLE_SENDER;
    receiver.role = NB_ROLE_RECEIVER;
    CHECK(nb_config_fingerprint(&sender) == nb_config_fingerprint(&receiver),
          "role must not change workload fingerprint");
}

/* ── CRC32 ───────────────────────────────────────────────────────────── */

static void test_crc32_deterministic(void)
{
    const char *data = "hello world";
    uint32_t c1 = nb_crc32((const uint8_t *)data, strlen(data));
    uint32_t c2 = nb_crc32((const uint8_t *)data, strlen(data));
    CHECK(c1 == c2, "CRC32 should be deterministic");
    CHECK(c1 != 0, "CRC32 should be non-zero for non-empty input");
}

static void test_crc32_changes_with_data(void)
{
    uint32_t c1 = nb_crc32((const uint8_t *)"abc", 3);
    uint32_t c2 = nb_crc32((const uint8_t *)"abd", 3);
    CHECK(c1 != c2, "CRC32 should change when data changes");
}

static void test_crc32_empty(void)
{
    uint32_t c = nb_crc32(NULL, 0);
    CHECK(c == 0, "CRC32 of empty must be 0 (reflected final xor = 0xFFFFFFFF ^ 0xFFFFFFFF)");
}

/* ── payload generator ───────────────────────────────────────────────── */

static void test_generator_deterministic(void)
{
    uint8_t buf1[256], buf2[256];
    nb_generator_fill(buf1, 256, 0x1234, 1, 0, 0);
    nb_generator_fill(buf2, 256, 0x1234, 1, 0, 0);
    CHECK(memcmp(buf1, buf2, 256) == 0, "generator should be deterministic");
    CHECK(buf1[0] != 0 || buf1[1] != 0, "generator output should not be all-zero");
}

static void test_generator_different_seed_differs(void)
{
    uint8_t buf1[128], buf2[128];
    nb_generator_fill(buf1, 128, 0xAAAA, 1, 0, 0);
    nb_generator_fill(buf2, 128, 0xBBBB, 1, 0, 0);
    CHECK(memcmp(buf1, buf2, 128) != 0,
          "different seeds should produce different output");
}

static void test_generator_different_sequence_differs(void)
{
    uint8_t buf1[128], buf2[128];
    nb_generator_fill(buf1, 128, 0x5555, 1, 0, 0);
    nb_generator_fill(buf2, 128, 0x5555, 1, 1, 0);   /* different sequence */
    CHECK(memcmp(buf1, buf2, 128) != 0,
          "different sequence numbers should produce different output");
}

static void test_generator_offset(void)
{
    uint8_t base[512], tail[256];
    nb_generator_fill(base, 512, 0x3333, 1, 0, 0);
    nb_generator_fill(tail, 256, 0x3333, 1, 0, 256);  /* start at offset 256 */
    CHECK(memcmp(base + 256, tail, 256) == 0,
          "offset generation should match contiguous fill");
}

/* ── config fingerprint ──────────────────────────────────────────────── */

static void test_fingerprint_deterministic(void)
{
    struct nb_config cfg;
    memset(&cfg, 0, sizeof(cfg));
    cfg.role = NB_ROLE_RECEIVER;
    cfg.test_id = 42;
    cfg.flow_count = 4;

    uint64_t f1 = nb_config_fingerprint(&cfg);
    uint64_t f2 = nb_config_fingerprint(&cfg);
    CHECK(f1 == f2, "fingerprint should be deterministic");
    CHECK(f1 != 0, "fingerprint should be non-zero for non-empty config");
}

static void test_fingerprint_different_configs_differ(void)
{
    struct nb_config a, b;
    memset(&a, 0, sizeof(a));
    memset(&b, 0, sizeof(b));

    a.test_id = 1;
    a.payload_size = 1400;
    b.test_id = 2;
    b.payload_size = 1400;

    CHECK(nb_config_fingerprint(&a) != nb_config_fingerprint(&b),
          "different test_id should give different fingerprint");
}

/* ── network byte order ──────────────────────────────────────────────── */

static void test_hton32_roundtrip(void)
{
    uint32_t val = 0x12345678;
    uint32_t net = nb_hton32(val);
    uint32_t host = nb_ntoh32(net);
    CHECK(host == val, "hton32/ntoh32 roundtrip should preserve value");
}

static void test_hton16_roundtrip(void)
{
    uint16_t val = 0xABCD;
    uint16_t net = nb_hton16(val);
    uint16_t host = nb_ntoh16(net);
    CHECK(host == val, "hton16/ntoh16 roundtrip should preserve value");
}

/* ── record header ───────────────────────────────────────────────────── */

static void test_record_header_roundtrip(void)
{
    uint8_t buf[64];
    struct nb_record_header hdr_in = {0};
    hdr_in.sequence = 1;
    hdr_in.completion_point = NB_CP_C6;
    hdr_in.protocol = NB_PROTO_TCP;
    hdr_in.direction = NB_DIR_RX;
    hdr_in.flow_id = 99;
    hdr_in.round_id = 42;
    hdr_in.byte_count = 12345678ULL;
    hdr_in.timestamp_ns = 0;

    size_t hdr_len = sizeof(buf);
    int rc = nb_record_header_encode(buf, &hdr_len, &hdr_in);
    CHECK(rc == 0, "record header encode should succeed");

    struct nb_record_header hdr;
    rc = nb_record_header_decode(&hdr, buf, hdr_len);
    CHECK(rc == 0, "record header decode should succeed");
    CHECK(hdr.sequence == 1, "sequence should survive");
    CHECK(hdr.completion_point == NB_CP_C6, "completion_point should survive");
    CHECK(hdr.round_id == 42, "round_id should survive");
    CHECK(hdr.byte_count == 12345678ULL, "byte_count should survive");
    CHECK(hdr.protocol == NB_PROTO_TCP, "protocol should survive");
}

/* ── data record ─────────────────────────────────────────────────────── */

static void test_data_record_crc(void)
{
    uint8_t payload[256];
    nb_generator_fill(payload, 256, 0xDEAD, 2, 0, 0);

    size_t enc_len = 32;

    uint8_t buf[NB_DATA_RECORD_MAX];
    size_t len = NB_DATA_RECORD_MAX;
    int rc = nb_data_record_encode(buf, &len, payload, enc_len,
                                    NB_PROTO_TCP, NB_DIR_TX, 0, 0, 1, NB_CP_C1);
    CHECK(rc == 0, "data record encode should succeed");

    /* CRC covers header(28) + payload_length(4) + payload */
    uint32_t expected_crc = nb_crc32(buf, NB_RECORD_HDR_SIZE + 4 + enc_len);

    struct nb_data_record rec;
    rc = nb_data_record_decode(&rec, buf, len);
    CHECK(rc == 0, "data record decode should succeed");
    CHECK(rec.payload_length == enc_len, "payload length should survive");
    CHECK(rec.crc == expected_crc, "CRC should match payload CRC");
}

/* ── protocol maximum ────────────────────────────────────────────────── */

static void test_protocol_maximum_is_sane(void)
{
    CHECK(NB_FRAME_BODY_MAX >= 4096, "body max should accommodate config records");
    CHECK(NB_FRAME_BODY_MAX <= 65536, "body max should be bounded");
    CHECK(NB_FRAME_MAX >= 128, "frame max should be reasonable");
    CHECK(NB_DATA_RECORD_MAX >= 2048, "data record max should accommodate MTU payload");
}

/* ── driver ──────────────────────────────────────────────────────────── */

static void test_decode_failure_is_atomic(void)
{
    struct nb_config cfg;
    struct nb_frame before;
    struct nb_frame after;
    uint8_t wire[NB_FRAME_MAX];
    size_t len = sizeof(wire);

    memset(&cfg, 0, sizeof(cfg));
    cfg.flow_count = 1;
    cfg.payload_size = 1400;
    cfg.duration_s = 1;
    cfg.config_fingerprint = nb_config_fingerprint(&cfg);
    memset(&before, 0xa5, sizeof(before));
    after = before;
    CHECK(nb_hello_encode(wire, &len, &cfg) == 0,
          "failure-atomic setup encode");
    CHECK(nb_frame_decode(&after, wire, len - 1) < 0,
          "truncated typed frame rejected");
    CHECK(memcmp(&after, &before, sizeof(before)) == 0,
          "failed decode preserves complete caller output");
}

static void run_test(const char *name, void (*fn)(void))
{
    int before = failures;
    fn();
    if (failures == before)
        printf("PASS: %s\n", name);
}

int main(void)
{
    printf("=== network_benchmark_protocol RED test suite ===\n\n");

    run_test("magic mismatch",       test_magic_mismatch);
    run_test("version mismatch",     test_version_mismatch);
    run_test("invalid frame type",   test_invalid_frame_type);
    run_test("hello roundtrip",      test_hello_roundtrip);
    run_test("summary roundtrip",    test_summary_roundtrip);
    run_test("frame too small",      test_frame_too_small);
    run_test("body length > max",    test_body_length_exceeds_maximum);
    run_test("truncated frame",      test_truncated_frame);
    run_test("trailing frame bytes", test_frame_trailing_bytes_rejected);
    run_test("CRC32 deterministic",  test_crc32_deterministic);
    run_test("CRC32 data change",    test_crc32_changes_with_data);
    run_test("CRC32 empty",          test_crc32_empty);
    run_test("generator deterministic",        test_generator_deterministic);
    run_test("generator different seed",       test_generator_different_seed_differs);
    run_test("generator different sequence",   test_generator_different_sequence_differs);
    run_test("generator offset",               test_generator_offset);
    run_test("generator non-aligned offset",    test_generator_non_aligned_offset);
    run_test("fingerprint deterministic",      test_fingerprint_deterministic);
    run_test("fingerprint different configs",  test_fingerprint_different_configs_differ);
    run_test("fingerprint excludes role",      test_fingerprint_excludes_role);
    run_test("hton32 roundtrip",     test_hton32_roundtrip);
    run_test("hton16 roundtrip",     test_hton16_roundtrip);
    run_test("record header roundtrip", test_record_header_roundtrip);
    run_test("data record CRC",      test_data_record_crc);
    run_test("frame constants sane", test_protocol_maximum_is_sane);
    run_test("decode failure atomic", test_decode_failure_is_atomic);

    printf("\n");
    if (failures == 0) {
        printf("ALL TESTS PASSED (%lu test functions)\n",
               (unsigned long)(26UL));
        return 0;
    }
    printf("FAILED: %d assertion(s)\n", failures);
    return 1;
}
