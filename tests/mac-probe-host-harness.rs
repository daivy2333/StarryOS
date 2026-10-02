//! MAC-probe pure-logic host test harness (MS09 Iteration 003, task 4.1;
//! extended by Iteration 004 for the PHY gate facts and the env-based APMU
//! sequence with native time-interface waits).
//!
//! Uses `#[path]` to reference the real `kernel/src/platform/{mac_probe,phy_probe}.rs`
//! without compiling the full kernel. Runs via `rustc --test` → `/tmp` binary
//! (same convention as `early-console-host-harness.rs`).
#![allow(dead_code)]

extern crate core;

#[path = "../kernel/src/platform/mac_probe.rs"]
mod mac_probe;
#[path = "../kernel/src/platform/phy_probe.rs"]
mod phy_probe;

use mac_probe::{
    APMU_BASE_PADDR, APMU_CTRL_OFFSET, EMAC_BUS_CLK_EN, EMAC_BUS_RST_DEASSERT, MAC_REG_DEBUG,
    MAC_REG_VERSION, MAC_WINDOW_SIZE, MacReadClass, PHY_INTF_MODE_MASK, PHY_INTF_RGMII, apmu_step1,
    apmu_step2, class_label, classify_mac_read, format_apmu_line, format_debug_line,
    format_gate_line, format_read_line,
};

#[test]
fn register_offsets_match_traced_source() {
    // hwif.h v6.18.3:691 `GMAC4_VERSION 0x110` ("GMAC4+ CORE Version")
    assert_eq!(MAC_REG_VERSION, 0x110);
    // dwmac4.h v6.18.3:33 `GMAC_DEBUG 0x114`
    assert_eq!(MAC_REG_DEBUG, 0x114);
    // Board DT `reg = <0x0 0xcac82000 0x0 0x2000>`: both registers inside.
    assert!(MAC_REG_VERSION + 4 <= MAC_WINDOW_SIZE);
    assert!(MAC_REG_DEBUG + 4 <= MAC_WINDOW_SIZE);
}

#[test]
fn classify_covers_zero_one_and_normal() {
    assert_eq!(classify_mac_read(0x0), MacReadClass::AllZero);
    assert_eq!(classify_mac_read(0xffff_ffff), MacReadClass::AllOne);
    // Board-expected identity shape (User ID 0x10, Synopsys ID 0x54 → 0x1054)
    // classifies as ok; the kernel itself never judges expected values.
    assert_eq!(classify_mac_read(0x1054), MacReadClass::Ok);
}

#[test]
fn classify_boundary_inputs() {
    assert_eq!(classify_mac_read(0x1), MacReadClass::Ok);
    assert_eq!(classify_mac_read(0x8000_0000), MacReadClass::Ok);
    assert_eq!(classify_mac_read(0xffff_fffe), MacReadClass::Ok);
}

#[test]
fn class_labels_are_stable() {
    assert_eq!(class_label(MacReadClass::Ok), "ok");
    assert_eq!(class_label(MacReadClass::AllZero), "all-zero");
    assert_eq!(class_label(MacReadClass::AllOne), "all-one");
}

#[test]
fn line_format_for_normal_read() {
    let mut buf = [0u8; mac_probe::READ_LINE_BUF_LEN];
    let line = format_read_line(&mut buf, "version", 1, 0x1054);
    assert_eq!(
        line,
        "[starry-k3] stage=5 mac version read1=0x00001054 class=ok\n"
    );
}

#[test]
fn line_format_for_zero_and_one_reads() {
    // MS09 Iteration 004: the debug register line carries a register-semantic
    // label — a legitimate idle zero is not reported as a fault conclusion.
    let mut zero_buf = [0u8; mac_probe::READ_LINE_BUF_LEN];
    assert_eq!(
        format_debug_line(&mut zero_buf, 2, 0x0),
        "[starry-k3] stage=5 mac debug read2=0x00000000 class=all-zero sem=idle\n"
    );
    let mut one_buf = [0u8; mac_probe::READ_LINE_BUF_LEN];
    assert_eq!(
        format_debug_line(&mut one_buf, 2, 0xffff_ffff),
        "[starry-k3] stage=5 mac debug read2=0xffffffff class=all-one sem=active\n"
    );
    // Version lines keep their Iteration 003 shape (no sem suffix).
    let mut ver_buf = [0u8; mac_probe::READ_LINE_BUF_LEN];
    assert_eq!(
        format_read_line(&mut ver_buf, "version", 1, 0x0),
        "[starry-k3] stage=5 mac version read1=0x00000000 class=all-zero\n"
    );
}

#[test]
fn apmu_constants_match_traced_sources() {
    // Board DTB `/soc/system-controller@d4282800` (`spacemit,k3-syscon-apmu`,
    // reg = <0x0 0xd4282800 0x0 0x400>); `spacemit,ctrl-offset = <1004>` = 0x3ec.
    assert_eq!(APMU_BASE_PADDR, 0xd428_2800);
    assert_eq!(APMU_CTRL_OFFSET, 0x3ec);
    // ccu-k3.c `emacx_bus_clk` BIT(0); reset-spacemit.c deassert BIT(1)
    // (traced in R69 `k3_gmac/regs.rs:167-181`).
    assert_eq!(EMAC_BUS_CLK_EN, 1);
    assert_eq!(EMAC_BUS_RST_DEASSERT, 2);
    // Interface field bits [4:3]; RGMII = 0b01 (`phy-mode = "rgmii"`, board DTB).
    assert_eq!(PHY_INTF_RGMII, 0b01 << 3);
    assert_eq!(PHY_INTF_MODE_MASK, 0b11 << 3);
}

#[test]
fn apmu_step1_enables_clock_and_selects_rgmii_keeps_other_bits() {
    // bit0 set, interface field forced to RGMII.
    assert_eq!(apmu_step1(0x0000_0000), EMAC_BUS_CLK_EN | PHY_INTF_RGMII);
    // Bits outside the mask (bit1 reset-deassert, WOL bit 12, bit 31) preserved.
    let old = 0x8000_1002;
    assert_eq!(apmu_step1(old), old | EMAC_BUS_CLK_EN | PHY_INTF_RGMII);
    // A pre-set non-RGMII interface mode is replaced, not OR-ed in.
    assert_eq!(apmu_step1(0b10 << 3), EMAC_BUS_CLK_EN | PHY_INTF_RGMII);
}

#[test]
fn apmu_step2_deasserts_reset_keeps_other_bits() {
    assert_eq!(apmu_step2(0x0000_0000), EMAC_BUS_RST_DEASSERT);
    // step1 output + reset deassert, all other bits preserved.
    let after_step1 = apmu_step1(0);
    assert_eq!(apmu_step2(after_step1), after_step1 | EMAC_BUS_RST_DEASSERT);
    // bit1 already set → value unchanged; WOL/bit31 outside mask preserved.
    assert_eq!(apmu_step2(0x8000_1002), 0x8000_1002);
    assert_eq!(apmu_step2(1 << 12), (1 << 12) | EMAC_BUS_RST_DEASSERT);
}

#[test]
fn apmu_sequence_from_zero_matches_traced_write_order() {
    // The chain the board sequence applies when CTRL starts at zero:
    // step1 (clock + RGMII) then step2 (reset deassert). The WOL bit is
    // never touched (board DTB lacks `spacemit,wake-irq-enable`).
    let step1 = apmu_step1(0);
    let step2 = apmu_step2(step1);
    assert_eq!(step1, EMAC_BUS_CLK_EN | PHY_INTF_RGMII);
    assert_eq!(
        step2,
        EMAC_BUS_CLK_EN | PHY_INTF_RGMII | EMAC_BUS_RST_DEASSERT
    );
    assert_eq!(step2 & (1 << 12), 0);
}

#[test]
fn apmu_line_format_is_raw_value_only() {
    // APMU lines carry raw u32 values; bit judgement stays in the Act Response.
    let mut buf = [0u8; mac_probe::READ_LINE_BUF_LEN];
    assert_eq!(
        format_apmu_line(&mut buf, "before", 0x0),
        "[starry-k3] stage=4 apmu ctrl before=0x00000000\n"
    );
    let mut done_buf = [0u8; mac_probe::READ_LINE_BUF_LEN];
    assert_eq!(
        format_apmu_line(&mut done_buf, "after2", 0x0000_001b),
        "[starry-k3] stage=4 apmu ctrl after2=0x0000001b\n"
    );
}

// ---------------------------------------------------------------------------
// MS09 Iteration 004: gate facts, env-based APMU sequence
// ---------------------------------------------------------------------------

#[test]
fn apmu_bits_effective_cases() {
    // Board-observed shape (003/001-rework): before=0, after1=9, after2=0xb.
    let (clk, rst) = mac_probe::apmu_bits_effective(0x9, 0xb);
    assert!(clk && rst);
    // Step-1 readback without the clock bit: clock not effective.
    assert_eq!(mac_probe::apmu_bits_effective(0x8, 0xb), (false, true));
    // Clock enabled but reset bit missing in the step-2 readback.
    assert_eq!(mac_probe::apmu_bits_effective(0x9, 0x9), (true, false));
    // Clock bit dropped between steps also fails the reset-side check.
    assert_eq!(mac_probe::apmu_bits_effective(0x9, 0x2), (true, false));
}

#[test]
fn mac_probe_summary_version_identity_ok() {
    use mac_probe::{MacProbeSummary, READS_PER_REG};
    let ok = MacProbeSummary {
        version_classes: [MacReadClass::Ok; READS_PER_REG],
        debug_classes: [MacReadClass::AllZero; READS_PER_REG],
    };
    assert!(ok.version_identity_ok(), "debug=0 (idle) is not an identity failure");
    let bad = MacProbeSummary {
        version_classes: [MacReadClass::Ok, MacReadClass::AllZero],
        debug_classes: [MacReadClass::Ok; READS_PER_REG],
    };
    assert!(!bad.version_identity_ok());
}

#[test]
fn gate_line_format_proceed_and_stop() {
    let mut buf = [0u8; mac_probe::READ_LINE_BUF_LEN];
    assert_eq!(
        format_gate_line(&mut buf, true, true, "ok", true),
        "[starry-k3] stage=6 gate apmu-clk=1 apmu-rst=1 mac-version=ok proceed\n"
    );
    let mut stop_buf = [0u8; mac_probe::READ_LINE_BUF_LEN];
    assert_eq!(
        format_gate_line(&mut stop_buf, false, true, "all-zero", false),
        "[starry-k3] stage=6 gate apmu-clk=0 apmu-rst=1 mac-version=all-zero stop\n"
    );
}

/// Minimal env for the APMU-only window: a single CTRL register plus waits.
struct FakeApmu {
    ctrl: u32,
    writes: Vec<(u64, u32)>,
    waits_ns: Vec<u64>,
}

impl FakeApmu {
    fn new() -> Self {
        FakeApmu { ctrl: 0, writes: Vec::new(), waits_ns: Vec::new() }
    }
}

impl phy_probe::ProbeEnv for FakeApmu {
    fn mmio_read(&mut self, _paddr: u64) -> u32 {
        self.ctrl
    }

    fn mmio_write(&mut self, paddr: u64, value: u32) {
        assert_eq!(
            paddr,
            APMU_BASE_PADDR + APMU_CTRL_OFFSET,
            "the APMU sequence must write the CTRL register only"
        );
        self.writes.push((paddr, value));
        self.ctrl = value;
    }

    fn now_nanos(&self) -> u64 {
        0
    }

    fn wait_nanos(&mut self, nanos: u64) {
        self.waits_ns.push(nanos);
    }
}

#[test]
fn apmu_sequence_via_env_matches_traced_order_and_board_shape() {
    let mut env = FakeApmu::new();
    let mut lines = Vec::new();
    let outcome = mac_probe::apmu_clock_enable(&mut env, |line| lines.push(line.to_string()));
    // Board-observed raw shape (003/001-rework): before=0, after1=9, after2=b.
    assert_eq!((outcome.before, outcome.after1, outcome.after2), (0, 0x9, 0xb));
    // Two-step write order on the CTRL register only.
    assert_eq!(
        env.writes,
        vec![
            (APMU_BASE_PADDR + APMU_CTRL_OFFSET, apmu_step1(0)),
            (APMU_BASE_PADDR + APMU_CTRL_OFFSET, apmu_step2(0x9)),
        ]
    );
    // The two inter-step waits use the native time interface with the real
    // 100 µs parameter (never a spin-calibration constant).
    assert_eq!(env.waits_ns, vec![100_000, 100_000]);
    assert!(lines.contains(&"[starry-k3] stage=4 apmu begin\n".to_string()));
    assert!(lines.contains(&"[starry-k3] stage=4 apmu ctrl before=0x00000000\n".to_string()));
    assert!(lines.contains(&"[starry-k3] stage=4 apmu ctrl after1=0x00000009\n".to_string()));
    assert!(lines.contains(&"[starry-k3] stage=4 apmu ctrl after2=0x0000000b\n".to_string()));
    assert!(lines.contains(&"[starry-k3] stage=4 apmu clock enable done\n".to_string()));
}
