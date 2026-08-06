/* MS16 network benchmark — bounded framed wire protocol implementation (D11).
 *
 * Every integer is explicitly serialized in network byte order.
 * Frame bounds are checked before any copy. Decode failure leaves
 * the caller's buffer unmodified.
 *
 * C11 + musl compatible.
 */
#include "network_benchmark_protocol.h"
#include <string.h>

/* ── byte order ──────────────────────────────────────────────────────── */

uint32_t nb_hton32(uint32_t host) {
    uint8_t b[4];
    b[0] = (uint8_t)(host >> 24);
    b[1] = (uint8_t)(host >> 16);
    b[2] = (uint8_t)(host >> 8);
    b[3] = (uint8_t)(host);
    uint32_t out;
    memcpy(&out, b, 4);
    return out;
}

uint32_t nb_ntoh32(uint32_t net) {
    uint8_t b[4];
    memcpy(b, &net, 4);
    return ((uint32_t)b[0] << 24) | ((uint32_t)b[1] << 16) |
           ((uint32_t)b[2] << 8)  | (uint32_t)b[3];
}

uint16_t nb_hton16(uint16_t host) {
    uint8_t b[2];
    b[0] = (uint8_t)(host >> 8);
    b[1] = (uint8_t)(host);
    uint16_t out;
    memcpy(&out, b, 2);
    return out;
}

uint16_t nb_ntoh16(uint16_t net) {
    uint8_t b[2];
    memcpy(b, &net, 2);
    return ((uint16_t)b[0] << 8) | (uint16_t)b[1];
}

uint64_t nb_hton64(uint64_t host) {
    uint8_t b[8];
    b[0] = (uint8_t)(host >> 56);
    b[1] = (uint8_t)(host >> 48);
    b[2] = (uint8_t)(host >> 40);
    b[3] = (uint8_t)(host >> 32);
    b[4] = (uint8_t)(host >> 24);
    b[5] = (uint8_t)(host >> 16);
    b[6] = (uint8_t)(host >> 8);
    b[7] = (uint8_t)(host);
    uint64_t out;
    memcpy(&out, b, 8);
    return out;
}

uint64_t nb_ntoh64(uint64_t net) {
    uint8_t b[8];
    memcpy(b, &net, 8);
    return ((uint64_t)b[0] << 56) | ((uint64_t)b[1] << 48) |
           ((uint64_t)b[2] << 40) | ((uint64_t)b[3] << 32) |
           ((uint64_t)b[4] << 24) | ((uint64_t)b[5] << 16) |
           ((uint64_t)b[6] << 8)  | (uint64_t)b[7];
}

/* ── CRC32 (IEEE 802.3 polynomial, software implementation) ──────────── */

uint32_t nb_crc32(const uint8_t *data, size_t len) {
    uint32_t crc = 0xFFFFFFFFu;
    for (size_t i = 0; i < len; i++) {
        crc ^= data[i];
        for (int b = 0; b < 8; b++) {
            if (crc & 1)
                crc = (crc >> 1) ^ 0xEDB88320u;
            else
                crc >>= 1;
        }
    }
    return crc ^ 0xFFFFFFFFu;
}

/* ── FNV-1a 64-bit config fingerprint (D11 canonical byte stream) ────── */

uint64_t nb_config_fingerprint(const struct nb_config *cfg) {
    /* Canonical fields (D11): version, test_id, protocol, direction,
     * flow_count, payload_size, duration_s, warmup_s, seed,
     * offered_load_pct, nagle.
     * Excludes: role, capability_bitmap, platform, treatment,
     * run_id, round_id, config_fingerprint itself. */
    uint64_t hash = 0xcbf29ce484222325ULL;
    uint8_t buf[64];
    int pos = 0;

    buf[pos++] = NB_PROTO_VERSION;
    buf[pos++] = (uint8_t)(cfg->test_id >> 24);
    buf[pos++] = (uint8_t)(cfg->test_id >> 16);
    buf[pos++] = (uint8_t)(cfg->test_id >> 8);
    buf[pos++] = (uint8_t)(cfg->test_id);
    buf[pos++] = cfg->protocol;
    buf[pos++] = cfg->direction;
    buf[pos++] = cfg->flow_count;
    buf[pos++] = (uint8_t)(cfg->payload_size >> 8);
    buf[pos++] = (uint8_t)(cfg->payload_size);
    buf[pos++] = (uint8_t)(cfg->duration_s >> 8);
    buf[pos++] = (uint8_t)(cfg->duration_s);
    buf[pos++] = (uint8_t)(cfg->warmup_s >> 8);
    buf[pos++] = (uint8_t)(cfg->warmup_s);
    buf[pos++] = (uint8_t)(cfg->seed >> 24);
    buf[pos++] = (uint8_t)(cfg->seed >> 16);
    buf[pos++] = (uint8_t)(cfg->seed >> 8);
    buf[pos++] = (uint8_t)(cfg->seed);
    buf[pos++] = cfg->offered_load_pct;
    buf[pos++] = cfg->nagle;

    for (int i = 0; i < pos; i++) {
        hash ^= buf[i];
        hash *= 0x100000001b3ULL;
    }
    return hash;
}

/* ── payload generator (LCG-based, endian-independent) ───────────────── */

void nb_generator_fill(uint8_t *buf, size_t len,
                       uint32_t seed, uint8_t flow, uint32_t seq,
                       size_t offset) {
    uint64_t state = ((uint64_t)seed) ^ (((uint64_t)flow) << 24) ^ ((uint64_t)seq);

    size_t block_index = offset / 8;
    size_t off_in_block = offset & 7;

    /* Advance state block_index times to skip full blocks */
    for (size_t i = 0; i < block_index; i++)
        state = state * 6364136223846793005ULL + 1442695040888963407ULL;

    if (off_in_block > 0 && len > 0) {
        /* Partial block at offset boundary — advance once for this block,
         * then extract bytes starting at off_in_block */
        state = state * 6364136223846793005ULL + 1442695040888963407ULL;
        uint64_t block = state;
        size_t take = 8 - off_in_block;
        if (take > len) take = len;
        for (size_t i = 0; i < take; i++)
            buf[i] = (uint8_t)(block >> ((off_in_block + i) * 8));
        buf += take;
        len -= take;
    }

    while (len >= 8) {
        state = state * 6364136223846793005ULL + 1442695040888963407ULL;
        uint64_t block = state;
        for (size_t i = 0; i < 8; i++)
            buf[i] = (uint8_t)(block >> (i * 8));
        buf += 8;
        len -= 8;
    }

    if (len > 0) {
        state = state * 6364136223846793005ULL + 1442695040888963407ULL;
        uint64_t block = state;
        for (size_t i = 0; i < len; i++)
            buf[i] = (uint8_t)(block >> (i * 8));
    }
}

/* ── common prefix (24 B) ────────────────────────────────────────────── */

void nb_common_prefix_write(uint8_t *out, uint64_t run_id, uint32_t test_id,
                            uint32_t round_id, uint64_t fingerprint) {
    uint64_t rid_be = nb_hton64(run_id);
    uint32_t tid_be = nb_hton32(test_id);
    uint32_t rnd_be = nb_hton32(round_id);
    uint64_t fp_be = nb_hton64(fingerprint);
    memcpy(out + NB_CPREF_RUN_ID_OFF, &rid_be, 8);
    memcpy(out + NB_CPREF_TEST_ID_OFF, &tid_be, 4);
    memcpy(out + NB_CPREF_ROUND_ID_OFF, &rnd_be, 4);
    memcpy(out + NB_CPREF_FINGERPRINT_OFF, &fp_be, 8);
}

int nb_common_prefix_read(const uint8_t *data, size_t len,
                          uint64_t *run_id, uint32_t *test_id,
                          uint32_t *round_id, uint64_t *fingerprint) {
    if (len < NB_COMMON_PREFIX_SIZE) return -1;
    uint64_t rid; memcpy(&rid, data + NB_CPREF_RUN_ID_OFF, 8);
    uint32_t tid; memcpy(&tid, data + NB_CPREF_TEST_ID_OFF, 4);
    uint32_t rnd; memcpy(&rnd, data + NB_CPREF_ROUND_ID_OFF, 4);
    uint64_t fp;  memcpy(&fp, data + NB_CPREF_FINGERPRINT_OFF, 8);
    if (run_id)     *run_id     = nb_ntoh64(rid);
    if (test_id)    *test_id    = nb_ntoh32(tid);
    if (round_id)   *round_id   = nb_ntoh32(rnd);
    if (fingerprint) *fingerprint = nb_ntoh64(fp);
    return 0;
}

/* ── frame header helpers ────────────────────────────────────────────── */

static size_t write_frame_hdr(uint8_t *out, uint8_t type, uint16_t body_len) {
    uint32_t magic_be = nb_hton32(NB_PROTO_MAGIC);
    memcpy(out, &magic_be, 4);
    out[4] = NB_PROTO_VERSION;
    out[5] = type;
    uint16_t bl_be = nb_hton16(body_len);
    memcpy(out + 6, &bl_be, 2);
    return NB_FRAME_MIN;
}

static int read_frame_hdr(const uint8_t *data, size_t len,
                          uint8_t *version, uint8_t *type, uint16_t *body_len) {
    if (len < NB_FRAME_MIN) return -1;
    uint32_t magic; memcpy(&magic, data, 4);
    if (nb_ntoh32(magic) != NB_PROTO_MAGIC) return -2;
    uint8_t ver = data[4];
    if (ver != NB_PROTO_VERSION) return -3;
    uint16_t bl; memcpy(&bl, data + NB_FRAME_BODY_LEN_OFF, 2);
    uint16_t bl_host = nb_ntoh16(bl);
    if (version)  *version = ver;
    if (type)     *type = data[5];
    if (body_len) *body_len = bl_host;
    return 0;
}

/* ── HELLO encode (48 B body) ────────────────────────────────────────── */

int nb_hello_encode(uint8_t *out, size_t *len, const struct nb_config *cfg) {
    size_t total = NB_FRAME_MIN + NB_HELLO_BODY_SIZE;
    if (*len < total) return -1;
    *len = total;

    size_t pos = write_frame_hdr(out, NB_FRAME_HELLO, NB_HELLO_BODY_SIZE);

    nb_common_prefix_write(out + pos, cfg->run_id, cfg->test_id,
                           cfg->round_id, cfg->config_fingerprint);
    pos += NB_COMMON_PREFIX_SIZE;

    out[pos++] = cfg->role;
    uint64_t cap_be = nb_hton64(cfg->capability_bitmap);
    memcpy(out + pos, &cap_be, 8); pos += 8;
    out[pos++] = cfg->protocol;
    out[pos++] = cfg->direction;
    out[pos++] = cfg->flow_count;
    uint16_t ps_be = nb_hton16(cfg->payload_size);
    memcpy(out + pos, &ps_be, 2); pos += 2;
    uint16_t ds_be = nb_hton16(cfg->duration_s);
    memcpy(out + pos, &ds_be, 2); pos += 2;
    uint16_t ws_be = nb_hton16(cfg->warmup_s);
    memcpy(out + pos, &ws_be, 2); pos += 2;
    uint32_t sd_be = nb_hton32(cfg->seed);
    memcpy(out + pos, &sd_be, 4); pos += 4;
    out[pos++] = cfg->offered_load_pct;
    out[pos++] = cfg->nagle;
    return 0;
}

/* ── prefix-only frames (READY/START/CANCEL, 24 B body) ──────────────── */

static int encode_prefix_only(uint8_t *out, size_t *len, uint8_t type,
                              uint16_t body_size, uint64_t run_id,
                              uint32_t test_id, uint32_t round_id,
                              uint64_t fingerprint) {
    size_t total = NB_FRAME_MIN + body_size;
    if (*len < total) return -1;
    *len = total;
    size_t pos = write_frame_hdr(out, type, body_size);
    nb_common_prefix_write(out + pos, run_id, test_id, round_id, fingerprint);
    return 0;
}

int nb_ready_encode(uint8_t *out, size_t *len,
                    uint64_t run_id, uint32_t test_id,
                    uint32_t round_id, uint64_t fingerprint) {
    return encode_prefix_only(out, len, NB_FRAME_READY, NB_READY_BODY_SIZE,
                              run_id, test_id, round_id, fingerprint);
}

int nb_start_encode(uint8_t *out, size_t *len,
                    uint64_t run_id, uint32_t test_id,
                    uint32_t round_id, uint64_t fingerprint) {
    return encode_prefix_only(out, len, NB_FRAME_START, NB_START_BODY_SIZE,
                              run_id, test_id, round_id, fingerprint);
}

int nb_cancel_encode(uint8_t *out, size_t *len,
                     uint64_t run_id, uint32_t test_id,
                     uint32_t round_id, uint64_t fingerprint) {
    return encode_prefix_only(out, len, NB_FRAME_CANCEL, NB_CANCEL_BODY_SIZE,
                              run_id, test_id, round_id, fingerprint);
}

/* ── SUMMARY encode (124 B body) ─────────────────────────────────────── */

int nb_summary_encode(uint8_t *out, size_t *len, const struct nb_summary *sum) {
    size_t total = NB_FRAME_MIN + NB_SUMMARY_BODY_SIZE;
    if (*len < total) return -1;
    *len = total;

    size_t pos = write_frame_hdr(out, NB_FRAME_SUMMARY, NB_SUMMARY_BODY_SIZE);
    nb_common_prefix_write(out + pos, sum->run_id, sum->test_id,
                           sum->round_id, sum->config_fingerprint);
    pos += NB_COMMON_PREFIX_SIZE;

    out[pos++] = sum->completion_point;
    out[pos++] = sum->status;
    out[pos++] = sum->invalid_reason;
    out[pos++] = 0;

    uint64_t v;
    v = nb_hton64(sum->rx_bytes);     memcpy(out + pos, &v, 8); pos += 8;
    v = nb_hton64(sum->rx_packets);   memcpy(out + pos, &v, 8); pos += 8;
    v = nb_hton64(sum->tx_bytes);     memcpy(out + pos, &v, 8); pos += 8;
    v = nb_hton64(sum->tx_packets);   memcpy(out + pos, &v, 8); pos += 8;

    uint32_t v32;
    v32 = nb_hton32(sum->rtt_min_us); memcpy(out + pos, &v32, 4); pos += 4;
    v32 = nb_hton32(sum->rtt_p50_us); memcpy(out + pos, &v32, 4); pos += 4;
    v32 = nb_hton32(sum->rtt_p95_us); memcpy(out + pos, &v32, 4); pos += 4;
    v32 = nb_hton32(sum->rtt_p99_us); memcpy(out + pos, &v32, 4); pos += 4;
    v32 = nb_hton32(sum->rtt_max_us); memcpy(out + pos, &v32, 4); pos += 4;
    v32 = nb_hton32(sum->udp_loss);      memcpy(out + pos, &v32, 4); pos += 4;
    v32 = nb_hton32(sum->udp_duplicate); memcpy(out + pos, &v32, 4); pos += 4;
    v32 = nb_hton32(sum->udp_reorder);   memcpy(out + pos, &v32, 4); pos += 4;
    v32 = nb_hton32(sum->udp_corrupt);   memcpy(out + pos, &v32, 4); pos += 4;
    v32 = nb_hton32(sum->udp_late);      memcpy(out + pos, &v32, 4); pos += 4;

    v = nb_hton64(sum->instret_begin);  memcpy(out + pos, &v, 8); pos += 8;
    v = nb_hton64(sum->instret_end);    memcpy(out + pos, &v, 8); pos += 8;
    v = nb_hton64(sum->instret_overhead); memcpy(out + pos, &v, 8); pos += 8;

    return 0;
}

/* ── ERROR encode (36 B body) ────────────────────────────────────────── */

int nb_error_encode(uint8_t *out, size_t *len,
                    uint64_t run_id, uint32_t test_id,
                    uint32_t round_id, uint64_t fingerprint,
                    uint16_t reason, uint64_t mismatch_bitmap) {
    size_t total = NB_FRAME_MIN + NB_ERROR_BODY_SIZE;
    if (*len < total) return -1;
    *len = total;

    size_t pos = write_frame_hdr(out, NB_FRAME_ERROR, NB_ERROR_BODY_SIZE);
    nb_common_prefix_write(out + pos, run_id, test_id, round_id, fingerprint);
    pos += NB_COMMON_PREFIX_SIZE;

    uint16_t r_be = nb_hton16(reason);
    memcpy(out + pos, &r_be, 2); pos += 2;
    out[pos++] = 0; out[pos++] = 0;
    uint64_t mb_be = nb_hton64(mismatch_bitmap);
    memcpy(out + pos, &mb_be, 8); pos += 8;

    return 0;
}

/* ── body-length validation ──────────────────────────────────────────── */

static uint16_t expected_body_len(uint8_t type) {
    switch (type) {
    case NB_FRAME_HELLO:   return NB_HELLO_BODY_SIZE;
    case NB_FRAME_READY:   return NB_READY_BODY_SIZE;
    case NB_FRAME_START:   return NB_START_BODY_SIZE;
    case NB_FRAME_CANCEL:  return NB_CANCEL_BODY_SIZE;
    case NB_FRAME_SUMMARY: return NB_SUMMARY_BODY_SIZE;
    case NB_FRAME_ERROR:   return NB_ERROR_BODY_SIZE;
    default:               return 0;
    }
}

/* ── frame decode (atomic on failure) ────────────────────────────────── */

static int decode_hello_body(struct nb_frame *frame, const uint8_t *body, size_t len) {
    if (len < NB_HELLO_BODY_SIZE) return -10;
    size_t pos = 0;

    if (nb_common_prefix_read(body + pos, len - pos,
                              &frame->config.run_id, &frame->config.test_id,
                              &frame->config.round_id,
                              &frame->config.config_fingerprint) < 0)
        return -11;
    pos += NB_COMMON_PREFIX_SIZE;

    frame->config.role = body[pos++];
    uint64_t cap; memcpy(&cap, body + pos, 8); pos += 8;
    frame->config.capability_bitmap = nb_ntoh64(cap);
    frame->config.protocol      = body[pos++];
    frame->config.direction     = body[pos++];
    frame->config.flow_count    = body[pos++];
    uint16_t ps; memcpy(&ps, body + pos, 2); pos += 2;
    frame->config.payload_size  = nb_ntoh16(ps);
    uint16_t ds; memcpy(&ds, body + pos, 2); pos += 2;
    frame->config.duration_s    = nb_ntoh16(ds);
    uint16_t ws; memcpy(&ws, body + pos, 2); pos += 2;
    frame->config.warmup_s      = nb_ntoh16(ws);
    uint32_t sd; memcpy(&sd, body + pos, 4); pos += 4;
    frame->config.seed          = nb_ntoh32(sd);
    frame->config.offered_load_pct = body[pos++];
    frame->config.nagle         = body[pos++];
    return 0;
}

static int decode_summary_body(struct nb_frame *frame, const uint8_t *body, size_t len) {
    if (len < NB_SUMMARY_BODY_SIZE) return -10;
    size_t pos = 0;

    if (nb_common_prefix_read(body + pos, len - pos,
                              &frame->summary.run_id, &frame->summary.test_id,
                              &frame->summary.round_id,
                              &frame->summary.config_fingerprint) < 0)
        return -11;
    pos += NB_COMMON_PREFIX_SIZE;

    frame->summary.completion_point = body[pos++];
    frame->summary.status           = body[pos++];
    frame->summary.invalid_reason   = body[pos++];
    pos++;

    uint64_t v;
    memcpy(&v, body + pos, 8); pos += 8; frame->summary.rx_bytes   = nb_ntoh64(v);
    memcpy(&v, body + pos, 8); pos += 8; frame->summary.rx_packets = nb_ntoh64(v);
    memcpy(&v, body + pos, 8); pos += 8; frame->summary.tx_bytes   = nb_ntoh64(v);
    memcpy(&v, body + pos, 8); pos += 8; frame->summary.tx_packets = nb_ntoh64(v);

    uint32_t v32;
    memcpy(&v32, body + pos, 4); pos += 4; frame->summary.rtt_min_us = nb_ntoh32(v32);
    memcpy(&v32, body + pos, 4); pos += 4; frame->summary.rtt_p50_us = nb_ntoh32(v32);
    memcpy(&v32, body + pos, 4); pos += 4; frame->summary.rtt_p95_us = nb_ntoh32(v32);
    memcpy(&v32, body + pos, 4); pos += 4; frame->summary.rtt_p99_us = nb_ntoh32(v32);
    memcpy(&v32, body + pos, 4); pos += 4; frame->summary.rtt_max_us = nb_ntoh32(v32);
    memcpy(&v32, body + pos, 4); pos += 4; frame->summary.udp_loss      = nb_ntoh32(v32);
    memcpy(&v32, body + pos, 4); pos += 4; frame->summary.udp_duplicate = nb_ntoh32(v32);
    memcpy(&v32, body + pos, 4); pos += 4; frame->summary.udp_reorder   = nb_ntoh32(v32);
    memcpy(&v32, body + pos, 4); pos += 4; frame->summary.udp_corrupt   = nb_ntoh32(v32);
    memcpy(&v32, body + pos, 4); pos += 4; frame->summary.udp_late      = nb_ntoh32(v32);

    memcpy(&v, body + pos, 8); pos += 8; frame->summary.instret_begin  = nb_ntoh64(v);
    memcpy(&v, body + pos, 8); pos += 8; frame->summary.instret_end    = nb_ntoh64(v);
    memcpy(&v, body + pos, 8); pos += 8; frame->summary.instret_overhead = nb_ntoh64(v);

    return 0;
}

static int decode_error_body(struct nb_frame *frame, const uint8_t *body, size_t len) {
    if (len < NB_ERROR_BODY_SIZE) return -10;
    size_t pos = NB_COMMON_PREFIX_SIZE;

    uint16_t r; memcpy(&r, body + pos, 2); pos += 2;
    frame->error.error_code = nb_ntoh16(r);
    pos += 2;
    uint64_t mb; memcpy(&mb, body + pos, 8);
    frame->error.mismatch_bitmap = nb_ntoh64(mb);
    frame->error.reason_text[0] = '\0';
    return 0;
}

int nb_frame_decode(struct nb_frame *frame, const uint8_t *data, size_t len) {
    struct nb_frame decoded;
    int decode_rc;
    if (!frame || !data) return -1;

    uint8_t version = 0, type_val = 0;
    uint16_t body_len = 0;
    int rc = read_frame_hdr(data, len, &version, &type_val, &body_len);
    if (rc < 0) return rc;
    if (version != NB_PROTO_VERSION) return -3;

    uint16_t expected = expected_body_len(type_val);
    if (expected == 0) return -4;
    if (body_len != expected) return -5;

    size_t total = NB_FRAME_MIN + body_len;
    if (len < total) return -6;
    /* trailing bytes — exact decoder rejects extra data */
    /* (D11: stream frame reassembly uses probe before exact decode) */
    if (len > total) return -7;

    memset(&decoded, 0, sizeof(decoded));
    decoded.version = version;
    decoded.type = type_val;
    decoded.body_length = body_len;

    /* Copy body for atomic decode */
    memcpy(decoded.body, data + NB_FRAME_MIN, body_len);

    switch (type_val) {
    case NB_FRAME_HELLO:
        decode_rc = decode_hello_body(&decoded, decoded.body, body_len);
        break;
    case NB_FRAME_READY:
    case NB_FRAME_START:
    case NB_FRAME_CANCEL:
        decode_rc = nb_common_prefix_read(decoded.body, body_len,
                                          &decoded.config.run_id,
                                          &decoded.config.test_id,
                                          &decoded.config.round_id,
                                          &decoded.config.config_fingerprint);
        break;
    case NB_FRAME_SUMMARY:
        decode_rc = decode_summary_body(&decoded, decoded.body, body_len);
        break;
    case NB_FRAME_ERROR:
        decode_rc = decode_error_body(&decoded, decoded.body, body_len);
        break;
    default:
        return -4;
    }
    if (decode_rc < 0) return decode_rc;
    *frame = decoded;
    return decode_rc;
}

/* ── record header encode/decode (28 B) ──────────────────────────────── */

int nb_record_header_encode(uint8_t *out, size_t *len,
                            const struct nb_record_header *hdr) {
    if (*len < NB_RECORD_HDR_SIZE) return -1;
    *len = NB_RECORD_HDR_SIZE;

    uint32_t s_be = nb_hton32(hdr->sequence);
    memcpy(out, &s_be, 4);
    out[4] = hdr->completion_point;
    out[5] = hdr->protocol;
    out[6] = hdr->direction;
    out[7] = hdr->flow_id;
    uint32_t r_be = nb_hton32(hdr->round_id);
    memcpy(out + 8, &r_be, 4);
    uint64_t b_be = nb_hton64(hdr->byte_count);
    memcpy(out + 12, &b_be, 8);
    uint64_t t_be = nb_hton64(hdr->timestamp_ns);
    memcpy(out + 20, &t_be, 8);
    return 0;
}

int nb_record_header_decode(struct nb_record_header *hdr,
                            const uint8_t *data, size_t len) {
    if (len < NB_RECORD_HDR_SIZE) return -1;
    uint32_t s; memcpy(&s, data, 4);
    hdr->sequence = nb_ntoh32(s);
    hdr->completion_point = data[4];
    hdr->protocol    = data[5];
    hdr->direction   = data[6];
    hdr->flow_id     = data[7];
    uint32_t r; memcpy(&r, data + 8, 4);
    hdr->round_id = nb_ntoh32(r);
    uint64_t b; memcpy(&b, data + 12, 8);
    hdr->byte_count = nb_ntoh64(b);
    uint64_t t; memcpy(&t, data + 20, 8);
    hdr->timestamp_ns = nb_ntoh64(t);
    return 0;
}

/* ── data record encode/decode ───────────────────────────────────────── */

int nb_data_record_encode(uint8_t *out, size_t *len,
                          const uint8_t *payload, size_t payload_len,
                          uint8_t protocol, uint8_t direction,
                          uint32_t sequence, uint32_t flow_id,
                          uint32_t round_id, uint8_t cp) {
    size_t record_size = NB_DATA_RECORD_FIXED + payload_len;
    if (*len < record_size) return -1;
    if (payload_len > (size_t)(NB_DATA_RECORD_MAX - NB_DATA_RECORD_FIXED))
        return -2;
    *len = record_size;

    struct nb_record_header hdr;
    hdr.sequence         = sequence;
    hdr.completion_point = cp;
    hdr.protocol         = protocol;
    hdr.direction        = direction;
    hdr.flow_id          = (uint8_t)flow_id;
    hdr.round_id         = round_id;
    hdr.byte_count       = (uint64_t)payload_len;
    hdr.timestamp_ns     = 0;

    size_t hdr_len = NB_RECORD_HDR_SIZE;
    nb_record_header_encode(out, &hdr_len, &hdr);

    uint32_t pl_be = nb_hton32((uint32_t)payload_len);
    memcpy(out + NB_RECORD_HDR_SIZE, &pl_be, 4);

    if (payload_len > 0)
        memcpy(out + NB_RECORD_HDR_SIZE + 4, payload, payload_len);

    size_t crc_covered = NB_RECORD_HDR_SIZE + 4 + payload_len;
    uint32_t crc = nb_crc32(out, crc_covered);
    uint32_t crc_be = nb_hton32(crc);
    memcpy(out + crc_covered, &crc_be, 4);

    return 0;
}

int nb_data_record_decode(struct nb_data_record *rec,
                          const uint8_t *data, size_t len) {
    if (len < NB_DATA_RECORD_FIXED) return -1;
    if (len > NB_DATA_RECORD_MAX) return -3;

    if (nb_record_header_decode(&rec->hdr, data, NB_RECORD_HDR_SIZE) < 0)
        return -2;

    uint32_t pl; memcpy(&pl, data + NB_RECORD_HDR_SIZE, 4);
    rec->payload_length = nb_ntoh32(pl);

    if (NB_DATA_RECORD_FIXED + rec->payload_length != len)
        return -4;

    if (rec->payload_length > 0)
        memcpy(rec->payload, data + NB_RECORD_HDR_SIZE + 4, rec->payload_length);

    size_t crc_covered = NB_RECORD_HDR_SIZE + 4 + rec->payload_length;
    if (crc_covered + 4 != len) return -5;
    uint32_t expected_crc = nb_crc32(data, crc_covered);
    uint32_t wire_crc; memcpy(&wire_crc, data + crc_covered, 4);
    wire_crc = nb_ntoh32(wire_crc);

    rec->crc = wire_crc;
    if (expected_crc != wire_crc) return -6;
    return 0;
}
