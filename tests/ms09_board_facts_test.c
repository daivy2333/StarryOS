/* MS09 board facts host tests — pure property decoding / error classification.
 *
 * Includes the probe implementation directly (project convention, see
 * tests/ms04_rx_probe_test.c).  These tests cover only decision-free
 * decoding helpers; live board output is exercised in Iteration 001.
 */
#define MS09_BOARD_FACTS_TESTING
#include "ms09_board_facts.c"

#include <assert.h>

static void test_be32_decodes_big_endian(void)
{
    const uint8_t raw[4] = {0xd4, 0x01, 0x70, 0x00};
    assert(fact_be32(raw) == 0xd4017000u);
}

static void test_string_list_basic(void)
{
    const uint8_t buf[] = "spacemit,k1-uart\0intel,xscale-uart\0";
    const char *out[4] = {0};
    size_t n = fact_decode_string_list(buf, sizeof(buf) - 1, out, 4);
    assert(n == 2);
    assert(strcmp(out[0], "spacemit,k1-uart") == 0);
    assert(strcmp(out[1], "intel,xscale-uart") == 0);
}

static void test_string_list_empty(void)
{
    const uint8_t one = 0;
    const char *out[2] = {0};
    assert(fact_decode_string_list(&one, 0, out, 2) == 0);
}

static void test_string_list_unterminated_tail_is_bounded(void)
{
    const uint8_t buf[] = {'a', 'b', 'c'};
    const char *out[2] = {0};
    size_t n = fact_decode_string_list(buf, sizeof(buf), out, 2);
    assert(n == 1);
    assert(memcmp(out[0], "abc", 3) == 0);
}

static void test_string_list_respects_max_out(void)
{
    const uint8_t buf[] = "one\0two\0three\0";
    const char *out[2] = {0};
    size_t n = fact_decode_string_list(buf, sizeof(buf) - 1, out, 2);
    assert(n == 2);
}

static void test_reg_decode_two_cell_parent(void)
{
    /* K3 style: parent #address-cells=2 #size-cells=2 */
    const uint8_t buf[] = {
        0x00, 0x00, 0x00, 0x00, 0xd4, 0x01, 0x70, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00,
    };
    uint64_t addr[2] = {0}, size[2] = {0};
    int n = fact_decode_reg(buf, sizeof(buf), 2, 2, addr, size, 2);
    assert(n == 1);
    assert(addr[0] == 0xd4017000ull);
    assert(size[0] == 0x100ull);
}

static void test_reg_decode_one_cell_parent(void)
{
    const uint8_t buf[] = {
        0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x10, 0x00,
    };
    uint64_t addr[2] = {0}, size[2] = {0};
    int n = fact_decode_reg(buf, sizeof(buf), 1, 1, addr, size, 2);
    assert(n == 1);
    assert(addr[0] == 1ull);
    assert(size[0] == 0x1000ull);
}

static void test_reg_decode_above_4g_address(void)
{
    /* memory@102000000: <0x1 0x02000000 0x1 0xfe000000> */
    const uint8_t buf[] = {
        0x00, 0x00, 0x00, 0x01, 0x02, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x01, 0xfe, 0x00, 0x00, 0x00,
    };
    uint64_t addr[1] = {0}, size[1] = {0};
    int n = fact_decode_reg(buf, sizeof(buf), 2, 2, addr, size, 1);
    assert(n == 1);
    assert(addr[0] == 0x102000000ull);
    assert(size[0] == 0x1fe000000ull);
}

static void test_reg_decode_malformed_length(void)
{
    const uint8_t buf[6] = {0};
    uint64_t addr[1] = {0}, size[1] = {0};
    assert(fact_decode_reg(buf, sizeof(buf), 2, 2, addr, size, 1) == -1);
}

static void test_reg_decode_too_few_cells(void)
{
    const uint8_t buf[8] = {0};
    uint64_t addr[1] = {0}, size[1] = {0};
    assert(fact_decode_reg(buf, sizeof(buf), 2, 2, addr, size, 1) == -1);
}

static void test_reg_decode_max_pairs_truncates(void)
{
    const uint8_t buf[] = {
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x10, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x20, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x00,
    };
    uint64_t addr[1] = {0}, size[1] = {0};
    assert(fact_decode_reg(buf, sizeof(buf), 2, 2, addr, size, 1) == 1);
    assert(addr[0] == 0x1000ull);
}

static void test_errno_classification(void)
{
    assert(fact_classify_errno(0) == FACT_OK);
    assert(fact_classify_errno(ENOENT) == FACT_MISSING);
    assert(fact_classify_errno(EACCES) == FACT_PERM);
    assert(fact_classify_errno(EPERM) == FACT_PERM);
    assert(fact_classify_errno(EIO) == FACT_IOERR);
    assert(fact_classify_errno(ENOTDIR) == FACT_IOERR);
}

static void test_phandle_lookup(void)
{
    const uint32_t handles[] = {0x82, 0x83, 0xe0};
    const char *const paths[] = {
        "/soc/clock-controller@d401e000",
        "/soc/interrupt-controller@e0804000",
        "/soc/mdio/phy@1",
    };
    assert(strcmp(fact_phandle_lookup(handles, paths, 3, 0x83),
                  "/soc/interrupt-controller@e0804000") == 0);
    assert(fact_phandle_lookup(handles, paths, 3, 0x99) == NULL);
    assert(fact_phandle_lookup(handles, paths, 0, 0x82) == NULL);
}

static void test_path_join_normal(void)
{
    char out[64];
    assert(fact_path_join(out, sizeof(out), "/soc", "serial@d4017000") == 20);
    assert(strcmp(out, "/soc/serial@d4017000") == 0);
    assert(fact_path_join(out, sizeof(out), "/", "chosen") == 7);
    assert(strcmp(out, "/chosen") == 0);
}

static void test_path_join_boundary_rejects_overflow(void)
{
    char out[8];
    assert(fact_path_join(out, sizeof(out), "/soc", "serial@d4017000") == -1);
    assert(fact_path_join(out, 0, "/soc", "serial") == -1);
}

/* --- reference property cell formatting --------------------------------- */

static const char *test_ref_lookup(uint32_t handle)
{
    /* Kit V02 phandle shapes verified in Task 1.1. */
    const uint32_t handles[] = {0x82, 0x87};
    const char *const paths[] = {
        "/soc/clock-controller@d401e000",
        "/soc/pinctrl@d401e000",
    };
    return fact_phandle_lookup(handles, paths, 2, handle);
}

static void test_ref_cells_single_phandle_kit_shape(void)
{
    /* serial@d4017000 { pinctrl-0 = <0x87>; } — one bare phandle cell. */
    const uint8_t buf[] = {0x00, 0x00, 0x00, 0x87};
    char out[64];
    assert(fact_format_ref_cells(buf, sizeof(buf), test_ref_lookup,
                                 out, sizeof(out)) == 0);
    assert(strcmp(out, "/soc/pinctrl@d401e000") == 0);
}

static void test_ref_cells_single_phandle_unresolved(void)
{
    const uint8_t buf[] = {0x12, 0x34, 0x56, 0x78};
    char out[64];
    assert(fact_format_ref_cells(buf, sizeof(buf), test_ref_lookup,
                                 out, sizeof(out)) == 0);
    assert(strcmp(out, "phandle=12345678") == 0);
}

static void test_ref_cells_clocks_six_cell_pairs(void)
{
    /* uart clocks = <0x82 0x00 0x82 0x0a 0x82 0x19> — three pairs. */
    const uint8_t buf[] = {
        0x00, 0x00, 0x00, 0x82, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x82, 0x00, 0x00, 0x00, 0x0a,
        0x00, 0x00, 0x00, 0x82, 0x00, 0x00, 0x00, 0x19,
    };
    char out[128];
    assert(fact_format_ref_cells(buf, sizeof(buf), test_ref_lookup,
                                 out, sizeof(out)) == 0);
    assert(strcmp(out,
                  "/soc/clock-controller@d401e000#0,"
                  "/soc/clock-controller@d401e000#10,"
                  "/soc/clock-controller@d401e000#25") == 0);
}

static void test_ref_cells_pair_unresolved(void)
{
    const uint8_t buf[] = {0x00, 0x00, 0x00, 0x99, 0x00, 0x00, 0x00, 0x07};
    char out[64];
    assert(fact_format_ref_cells(buf, sizeof(buf), test_ref_lookup,
                                 out, sizeof(out)) == 0);
    assert(strcmp(out, "phandle=99#7") == 0);
}

static void test_ref_cells_odd_trailing_bare_phandle(void)
{
    /* <0x82 0x00 0x87> — one pair then one bare phandle cell. */
    const uint8_t buf[] = {
        0x00, 0x00, 0x00, 0x82, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x87,
    };
    char out[128];
    assert(fact_format_ref_cells(buf, sizeof(buf), test_ref_lookup,
                                 out, sizeof(out)) == 0);
    assert(strcmp(out,
                  "/soc/clock-controller@d401e000#0,/soc/pinctrl@d401e000") == 0);
}

static void test_ref_cells_zero_cell_is_empty(void)
{
    char out[64];
    assert(fact_format_ref_cells(NULL, 0, test_ref_lookup, out, sizeof(out)) == 0);
    assert(strcmp(out, "<empty>") == 0);
}

static void test_ref_cells_truncated_tail_is_malformed(void)
{
    const uint8_t buf[] = {0x00, 0x00}; /* not a whole cell */
    char out[64];
    assert(fact_format_ref_cells(buf, sizeof(buf), test_ref_lookup,
                                 out, sizeof(out)) == 0);
    assert(strcmp(out, "<malformed>") == 0);
}

static void test_ref_cells_overflow_rejected(void)
{
    static uint8_t buf[4 * 512]; /* 512 pair cells overflow a 64-byte out */
    char out[64];
    memset(buf, 0, sizeof(buf));
    assert(fact_format_ref_cells(buf, sizeof(buf), test_ref_lookup,
                                 out, sizeof(out)) == -1);
}

int main(void)
{
    test_be32_decodes_big_endian();
    test_string_list_basic();
    test_string_list_empty();
    test_string_list_unterminated_tail_is_bounded();
    test_string_list_respects_max_out();
    test_reg_decode_two_cell_parent();
    test_reg_decode_one_cell_parent();
    test_reg_decode_above_4g_address();
    test_reg_decode_malformed_length();
    test_reg_decode_too_few_cells();
    test_reg_decode_max_pairs_truncates();
    test_errno_classification();
    test_phandle_lookup();
    test_path_join_normal();
    test_path_join_boundary_rejects_overflow();
    test_ref_cells_single_phandle_kit_shape();
    test_ref_cells_single_phandle_unresolved();
    test_ref_cells_clocks_six_cell_pairs();
    test_ref_cells_pair_unresolved();
    test_ref_cells_odd_trailing_bare_phandle();
    test_ref_cells_zero_cell_is_empty();
    test_ref_cells_truncated_tail_is_malformed();
    test_ref_cells_overflow_rejected();
    printf("ms09-board-facts: all host tests passed\n");
    return 0;
}
