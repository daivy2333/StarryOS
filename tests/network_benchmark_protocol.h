/* MS16 network benchmark — bounded framed wire protocol (D11).
 *
 * No raw-struct ABI. Every integer is explicitly serialized/deserialized
 * to/from network byte order. Frame maximums are checked before any copy.
 * Decode failure preserves caller's frame buffer byte-for-byte.
 *
 * C11 + musl compatible. No external dependencies.
 */
#ifndef NETWORK_BENCHMARK_PROTOCOL_H
#define NETWORK_BENCHMARK_PROTOCOL_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* ── protocol constants ──────────────────────────────────────────────── */

#define NB_PROTO_MAGIC     0x4E423031u  /* "NB01" */
#define NB_PROTO_VERSION   1

/* Frame sizing (D11) */
#define NB_FRAME_MIN            8   /* magic(4) + version(1) + type(1) + body_len(2) */
#define NB_FRAME_BODY_MAX       16384
#define NB_FRAME_MAX            (NB_FRAME_MIN + NB_FRAME_BODY_MAX)

/* Control frame body sizes (all include 24 B common prefix) */
#define NB_COMMON_PREFIX_SIZE   24  /* run_id(8) + test_id(4) + round_id(4) + fingerprint(8) */
#define NB_HELLO_BODY_SIZE      48  /* prefix + role(1) + capability(8) + config(15) */
#define NB_READY_BODY_SIZE      24  /* prefix only */
#define NB_START_BODY_SIZE      24  /* prefix only */
#define NB_CANCEL_BODY_SIZE     24  /* prefix only */
#define NB_SUMMARY_BODY_SIZE    124 /* prefix(24) + metrics(100) */
#define NB_ERROR_BODY_SIZE      36  /* prefix(24) + reason(2) + reserved(2) + mismatch(8) */

/* Record sizing (D11) */
#define NB_RECORD_HDR_SIZE      28  /* record header */
#define NB_DATA_RECORD_FIXED    36  /* hdr(28) + payload_len(4) + crc(4) */
#define NB_DATA_RECORD_MAX      2048
#define NB_TCP_PAYLOAD_MAX      2012 /* NB_DATA_RECORD_MAX - NB_DATA_RECORD_FIXED */
#define NB_UDP_PAYLOAD_MAX      1436 /* 1472 - 36 */

/* Byte offsets within frame header */
#define NB_FRAME_BODY_LEN_OFF   6

/* Common prefix field offsets */
#define NB_CPREF_RUN_ID_OFF     0
#define NB_CPREF_TEST_ID_OFF    8
#define NB_CPREF_ROUND_ID_OFF   12
#define NB_CPREF_FINGERPRINT_OFF 16

/* ── frame types ─────────────────────────────────────────────────────── */

enum nb_frame_type {
    NB_FRAME_HELLO   = 0x01,
    NB_FRAME_READY   = 0x02,
    NB_FRAME_START   = 0x03,
    NB_FRAME_CANCEL  = 0x04,
    NB_FRAME_SUMMARY = 0x05,
    NB_FRAME_ERROR   = 0x06,
};

/* ── roles / protocols / directions / completion points ─────────────── */

enum nb_role {
    NB_ROLE_SENDER   = 0,
    NB_ROLE_RECEIVER = 1,
};

enum nb_protocol {
    NB_PROTO_TCP = 0,
    NB_PROTO_UDP = 1,
};

enum nb_direction {
    NB_DIR_TX    = 0,
    NB_DIR_RX    = 1,
    NB_DIR_BIDI  = 2,
};

enum nb_completion_point {
    NB_CP_C1 = 1,  /* syscall returned */
    NB_CP_C2 = 2,  /* stack accepted */
    NB_CP_C3 = 3,  /* descriptor submitted */
    NB_CP_C4 = 4,  /* descriptor completed */
    NB_CP_C5 = 5,  /* peer stack received */
    NB_CP_C6 = 6,  /* peer app validated */
};

enum nb_status {
    NB_STATUS_VALID   = 0,
    NB_STATUS_INVALID = 1,
};

/* Reason codes for invalid rounds */
enum nb_invalid_reason {
    NB_INVALID_NONE              = 0,
    NB_INVALID_CONFIG_MISMATCH   = 1,
    NB_INVALID_PEER_EOF          = 2,
    NB_INVALID_TIMEOUT           = 3,
    NB_INVALID_PARTIAL           = 4,
    NB_INVALID_CHECKSUM          = 5,
    NB_INVALID_CLOCK_MONOTONIC   = 6,
    NB_INVALID_CANCELLED         = 7,
};

/* ── configuration ───────────────────────────────────────────────────── */

struct nb_config {
    uint8_t  role;              /* enum nb_role */
    uint8_t  protocol;          /* enum nb_protocol */
    uint8_t  direction;         /* enum nb_direction */
    uint8_t  flow_count;        /* 1, 2, 4, or 8 */
    uint16_t payload_size;
    uint16_t duration_s;
    uint16_t warmup_s;
    uint32_t seed;
    uint8_t  offered_load_pct;  /* 0 = full speed, 1-100 = % */
    uint8_t  nagle;             /* 0 = TCP_NODELAY, 1 = Nagle on */
    uint64_t run_id;
    uint32_t test_id;
    uint32_t round_id;
    uint64_t capability_bitmap;
    uint64_t config_fingerprint; /* FNV-1a of canonical string */
};

/* ── decoded frame ───────────────────────────────────────────────────── */

struct nb_summary {
    uint64_t run_id;
    uint32_t test_id;
    uint32_t round_id;
    uint64_t config_fingerprint;
    uint8_t  completion_point;
    uint8_t  status;
    uint8_t  invalid_reason;
    uint8_t  _pad;
    uint64_t rx_bytes;
    uint64_t rx_packets;
    uint64_t tx_bytes;
    uint64_t tx_packets;
    uint32_t rtt_min_us;
    uint32_t rtt_p50_us;
    uint32_t rtt_p95_us;
    uint32_t rtt_p99_us;
    uint32_t rtt_max_us;
    uint32_t udp_loss;
    uint32_t udp_duplicate;
    uint32_t udp_reorder;
    uint32_t udp_corrupt;
    uint32_t udp_late;
    uint64_t instret_begin;
    uint64_t instret_end;
    uint64_t instret_overhead;
};

struct nb_error_info {
    uint16_t error_code;
    uint64_t mismatch_bitmap;
    char     reason_text[64];
};

struct nb_frame {
    uint8_t  version;
    uint8_t  type;           /* enum nb_frame_type */
    uint16_t body_length;
    uint8_t  body[NB_FRAME_BODY_MAX];

    /* typed access — valid only when decode succeeds */
    struct nb_config      config;
    struct nb_summary     summary;
    struct nb_error_info  error;
};

/* ── record header (per-packet/per-record metadata, 28 B D11) ────────── */

struct nb_record_header {
    uint32_t sequence;
    uint8_t  completion_point;
    uint8_t  protocol;
    uint8_t  direction;
    uint8_t  flow_id;
    uint32_t round_id;
    uint64_t byte_count;
    uint64_t timestamp_ns;
};

/* ── data record ─────────────────────────────────────────────────────── */

struct nb_data_record {
    struct nb_record_header hdr;
    uint32_t payload_length;
    uint32_t crc;
    uint8_t  payload[NB_DATA_RECORD_MAX - NB_DATA_RECORD_FIXED];
};

/* ── byte order ──────────────────────────────────────────────────────── */

uint32_t nb_hton32(uint32_t host);
uint32_t nb_ntoh32(uint32_t net);
uint16_t nb_hton16(uint16_t host);
uint16_t nb_ntoh16(uint16_t net);
uint64_t nb_hton64(uint64_t host);
uint64_t nb_ntoh64(uint64_t net);

/* ── CRC32 (IEEE 802.3 polynomial) ───────────────────────────────────── */

uint32_t nb_crc32(const uint8_t *data, size_t len);

/* ── FNV-1a 64-bit config fingerprint ───────────────────────────────── */

uint64_t nb_config_fingerprint(const struct nb_config *cfg);

/* ── payload generator (deterministic, seed+flow+seq+offset, endian-safe) ─ */

void nb_generator_fill(uint8_t *buf, size_t len,
                       uint32_t seed, uint8_t flow, uint32_t seq,
                       size_t offset);

/* ── common prefix encode/decode ─────────────────────────────────────── */

void nb_common_prefix_write(uint8_t *out, uint64_t run_id, uint32_t test_id,
                            uint32_t round_id, uint64_t fingerprint);
int  nb_common_prefix_read(const uint8_t *data, size_t len,
                           uint64_t *run_id, uint32_t *test_id,
                           uint32_t *round_id, uint64_t *fingerprint);

/* ── frame encoding/decoding ─────────────────────────────────────────── */

/* Write a HELLO frame. *len receives actual bytes written. */
int nb_hello_encode(uint8_t *out, size_t *len, const struct nb_config *cfg);

/* Write a READY frame. */
int nb_ready_encode(uint8_t *out, size_t *len,
                    uint64_t run_id, uint32_t test_id,
                    uint32_t round_id, uint64_t fingerprint);

/* Write a START frame. */
int nb_start_encode(uint8_t *out, size_t *len,
                    uint64_t run_id, uint32_t test_id,
                    uint32_t round_id, uint64_t fingerprint);

/* Write a CANCEL frame. */
int nb_cancel_encode(uint8_t *out, size_t *len,
                     uint64_t run_id, uint32_t test_id,
                     uint32_t round_id, uint64_t fingerprint);

/* Write a SUMMARY frame. */
int nb_summary_encode(uint8_t *out, size_t *len, const struct nb_summary *sum);

/* Write an ERROR frame with reason code and mismatch bitmap. */
int nb_error_encode(uint8_t *out, size_t *len,
                    uint64_t run_id, uint32_t test_id,
                    uint32_t round_id, uint64_t fingerprint,
                    uint16_t reason, uint64_t mismatch_bitmap);

/* Decode any frame type into struct nb_frame.
 * Returns >= 0 on success (bytes consumed), < 0 on failure.
 * On failure, frame->body is unmodified from caller's state. */
int nb_frame_decode(struct nb_frame *frame, const uint8_t *data, size_t len);

/* ── record encoding/decoding ────────────────────────────────────────── */

int nb_record_header_encode(uint8_t *out, size_t *len,
                            const struct nb_record_header *hdr);

int nb_record_header_decode(struct nb_record_header *hdr,
                            const uint8_t *data, size_t len);

/* ── data record ─────────────────────────────────────────────────────── */

int nb_data_record_encode(uint8_t *out, size_t *len,
                          const uint8_t *payload, size_t payload_len,
                          uint8_t protocol, uint8_t direction,
                          uint32_t sequence, uint32_t flow_id,
                          uint32_t round_id, uint8_t cp);

int nb_data_record_decode(struct nb_data_record *rec,
                          const uint8_t *data, size_t len);

#ifdef __cplusplus
}
#endif

#endif /* NETWORK_BENCHMARK_PROTOCOL_H */
