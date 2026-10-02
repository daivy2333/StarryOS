//! K3 target-MAC read-only identity/status probe (MS09 Iteration 003, task 4.1).
//!
//! Register facts are quoted from the upstream `stmmac` driver sources matching
//! the board kernel (Linux 6.18.3-generic, `dwmac-spacemit-ethqos` bound to
//! `ethernet@cac82000`, `snps,dwmac-5.10a`; board kernel log identity
//! `User ID: 0x10, Synopsys ID: 0x54` / `DWMAC4/5`, see change evidence
//! `001-board-ram-boundary/000-initial/uboot-readonly.txt:564-571`):
//!
//! - MAC version (identity): `GMAC4_VERSION = 0x110` ("GMAC4+ CORE Version"),
//!   `drivers/net/ethernet/stmicro/stmmac/hwif.h:691` @ v6.18.3; read by
//!   `stmmac_get_id(priv, GMAC4_VERSION)` on the gmac4/xgmac path
//!   (`hwif.c:323`); `stmmac_get_id` splits User ID = bits [15:8] and
//!   Synopsys ID = bits [7:0] (`hwif.c:15-28`).
//! - MAC debug (status): `GMAC_DEBUG = 0x114`,
//!   `drivers/net/ethernet/stmicro/stmmac/dwmac4.h:33` @ v6.18.3.
//!
//! The MAC itself is strictly read-only: it reports raw values plus a coarse
//! classification (`ok` / `all-zero` / `all-one`) and never writes any MAC
//! register. Expected-value matching against the board log identity belongs
//! to the change Act Response, not the kernel.
//!
//! Stage 4 is the APMU GMAC bus clock-enable sequence that must run before
//! the MAC window responds (Iteration 003 rework 4.1-R1: the board located
//! the handoff bus hang at the gated GMAC clock). The two-step write order
//! is traced from the same-board reference implementation (R69,
//! `k3/others/Rt-Async-AMP/` `tgoskits .../k3_gmac/syscon.rs:242-263`, bit
//! constants `regs.rs:167-181`; upstream labels ccu-k3.c `emacx_bus_clk`
//! BIT(0), reset-spacemit.c deassert BIT(1), user manual 14.3.4.1) and
//! cross-checked against the board DTB (`spacemit,ctrl-offset = <1004>` =
//! 0x3ec, `phy-mode = "rgmii"`, no `spacemit,wake-irq-enable`). The DLINE
//! offset (0x3f0) is never written and no "clock off first" variant is used
//! (the traced three-step pulse leaves the DMA SFT_RESET stuck).
//!
//! Faults are not caught; the serial wire shapes separate the layers:
//! `stage=4 apmu begin` with nothing after it — APMU access fault (mapping
//! layer); readbacks whose bit0/bit1 are not set after step 2 — APMU write
//! not effective; `stage=4 ... done` followed by `stage=5 mac probe begin`
//! and silence — the clock-gating assumption is falsified; all-zero/all-one
//! MAC reads — deeper clock/reset layer.

/// Target MAC base from the board DT: `ethernet@cac82000`,
/// `reg = <0x0 0xcac82000 0x0 0x2000>` (shipped `tools/k3_com260_ifx.dtb`).
pub const MAC_BASE_PADDR: u64 = 0xcac8_2000;
/// MAC `reg` window size from the same DT entry.
pub const MAC_WINDOW_SIZE: u64 = 0x2000;

/// `GMAC4_VERSION` — MAC identity register (`hwif.h:691` @ v6.18.3).
pub const MAC_REG_VERSION: u64 = 0x110;
/// `GMAC_DEBUG` — MAC status register (`dwmac4.h:33` @ v6.18.3).
pub const MAC_REG_DEBUG: u64 = 0x114;

/// APMU system-controller base from the board DT:
/// `/soc/system-controller@d4282800` (`spacemit,k3-syscon-apmu`,
/// `reg = <0x0 0xd4282800 0x0 0x400>`).
pub const APMU_BASE_PADDR: u64 = 0xd428_2800;
/// EMAC bus control register offset inside the APMU: board DTB
/// `spacemit,ctrl-offset = <1004>` = 0x3ec (`k3_gmac/regs.rs:167-181`).
pub const APMU_CTRL_OFFSET: u64 = 0x3ec;
/// EMAC bus clock enable — ccu-k3.c `emacx_bus_clk` BIT(0)
/// (`k3_gmac/regs.rs:167-181`).
pub const EMAC_BUS_CLK_EN: u32 = 1 << 0;
/// EMAC bus reset deassert — reset-spacemit.c deassert BIT(1).
pub const EMAC_BUS_RST_DEASSERT: u32 = 1 << 1;
/// PHY interface mode select field mask, bits [4:3].
pub const PHY_INTF_MODE_MASK: u32 = 0b11 << 3;
/// PHY interface mode RGMII (`phy-mode = "rgmii"`, board DTB).
pub const PHY_INTF_RGMII: u32 = 0b01 << 3;

/// Spin between the two APMU steps and before the MAC probe, matching the
/// traced `delay_us(100)` (`k3_gmac/syscon.rs:242-263`).
const APMU_STEP_DELAY_US: usize = 100;

/// Buffer size for one probe line built by [`format_read_line`].
pub const READ_LINE_BUF_LEN: usize = 96;

/// Reads printed per register on the serial wire (same-boot repetition,
/// task 4.1 contract: every read is emitted, none is filtered).
const READS_PER_REG: usize = 2;

/// Coarse read classification. The fault layer is not representable here:
/// a trapping MMIO read never returns, which the serial wire shows as the
/// `probe begin` line with no register lines after it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MacReadClass {
    /// Neither all-zero nor all-one.
    Ok,
    /// Every bit zero — clock/reset handoff layer signature.
    AllZero,
    /// Every bit one — clock/reset handoff layer signature.
    AllOne,
}

/// Classifies a raw MMIO read. Pure and host-tested.
pub const fn classify_mac_read(value: u32) -> MacReadClass {
    match value {
        0 => MacReadClass::AllZero,
        0xffff_ffff => MacReadClass::AllOne,
        _ => MacReadClass::Ok,
    }
}

/// Stable serial label for a classification.
pub const fn class_label(class: MacReadClass) -> &'static str {
    match class {
        MacReadClass::Ok => "ok",
        MacReadClass::AllZero => "all-zero",
        MacReadClass::AllOne => "all-one",
    }
}

/// APMU step 1 RMW: enable the EMAC bus clock and select RGMII, keeping every
/// bit outside `EMAC_BUS_CLK_EN | PHY_INTF_MODE_MASK` unchanged (`update_bits`
/// semantics, `k3_gmac/syscon.rs:307-310`). Pure and host-tested.
pub const fn apmu_step1(old: u32) -> u32 {
    (old & !(EMAC_BUS_CLK_EN | PHY_INTF_MODE_MASK)) | (EMAC_BUS_CLK_EN | PHY_INTF_RGMII)
}

/// APMU step 2 RMW: deassert the EMAC bus reset, keeping every other bit
/// unchanged. The mask is `EMAC_BUS_RST_DEASSERT` only: the board DTB lacks
/// `spacemit,wake-irq-enable`, so the WOL bit keeps its handoff value (more
/// conservative than the traced reference, which ORs in the WOL enable).
/// Pure and host-tested.
pub const fn apmu_step2(old: u32) -> u32 {
    (old & !EMAC_BUS_RST_DEASSERT) | EMAC_BUS_RST_DEASSERT
}

fn push_bytes(buf: &mut [u8], n: &mut usize, src: &[u8]) {
    let end = *n + src.len();
    assert!(end <= buf.len(), "probe line buffer overflow");
    buf[*n..end].copy_from_slice(src);
    *n = end;
}

fn push_u32_hex8(buf: &mut [u8], n: &mut usize, value: u32) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut digits = [0u8; 8];
    for (i, d) in digits.iter_mut().enumerate() {
        *d = HEX[((value >> (28 - i * 4)) & 0xf) as usize];
    }
    push_bytes(buf, n, &digits);
}

fn push_usize(buf: &mut [u8], n: &mut usize, value: usize) {
    let mut digits = [0u8; 20];
    let mut len = 0;
    let mut rest = value;
    loop {
        digits[len] = b'0' + (rest % 10) as u8;
        len += 1;
        rest /= 10;
        if rest == 0 {
            break;
        }
    }
    for i in (0..len).rev() {
        push_bytes(buf, n, &digits[i..i + 1]);
    }
}

/// Formats one read line into `buf` and returns it as `&str`.
///
/// Shape: `[starry-k3] stage=5 mac <reg> read<N>=0xXXXXXXXX class=<label>\n`
pub fn format_read_line<'a>(
    buf: &'a mut [u8],
    reg_name: &str,
    read_idx: usize,
    value: u32,
) -> &'a str {
    let class = classify_mac_read(value);
    let mut n = 0usize;
    push_bytes(buf, &mut n, b"[starry-k3] stage=5 mac ");
    push_bytes(buf, &mut n, reg_name.as_bytes());
    push_bytes(buf, &mut n, b" read");
    push_usize(buf, &mut n, read_idx);
    push_bytes(buf, &mut n, b"=0x");
    push_u32_hex8(buf, &mut n, value);
    push_bytes(buf, &mut n, b" class=");
    push_bytes(buf, &mut n, class_label(class).as_bytes());
    push_bytes(buf, &mut n, b"\n");
    core::str::from_utf8(&buf[..n]).expect("probe line is ASCII")
}

/// Formats one APMU sequence line into `buf` and returns it as `&str`.
///
/// Shape: `[starry-k3] stage=4 apmu ctrl <label>=0xXXXXXXXX\n` — raw u32
/// values only; readback bit judgement belongs to the change Act Response.
pub fn format_apmu_line<'a>(buf: &'a mut [u8], label: &str, value: u32) -> &'a str {
    let mut n = 0usize;
    push_bytes(buf, &mut n, b"[starry-k3] stage=4 apmu ctrl ");
    push_bytes(buf, &mut n, label.as_bytes());
    push_bytes(buf, &mut n, b"=0x");
    push_u32_hex8(buf, &mut n, value);
    push_bytes(buf, &mut n, b"\n");
    core::str::from_utf8(&buf[..n]).expect("probe line is ASCII")
}

/// Busy-wait `us` microseconds, calibrated at the 50 spin iterations per µs
/// used by the traced reference (`k3_gmac/syscon.rs:312-319`). Timer-free on
/// purpose: the minimal platform stage has no timer initialized.
fn delay_us(us: usize) {
    for _ in 0..us * 50 {
        core::hint::spin_loop();
    }
}

/// APMU GMAC bus clock-enable sequence (stage=4), in the board-verified
/// two-step write order traced in R69 (`k3_gmac/syscon.rs:242-263`): step 1
/// applies [`apmu_step1`] (clock + RGMII), step 2 applies [`apmu_step2`]
/// (reset deassert), each followed by an [`APMU_STEP_DELAY_US`] spin. Every
/// readback is emitted as a raw u32 through `emit`; the kernel never judges
/// expected values.
///
/// # Safety
///
/// The APMU window `[APMU_BASE_PADDR, APMU_BASE_PADDR + 0x400)` must be
/// identity-mapped (K3 `devices.mmio-ranges`). The only write target is the
/// CTRL register at `APMU_CTRL_OFFSET` (0x3ec); no other APMU address (in
/// particular DLINE 0x3f0) is read or written.
pub unsafe fn apmu_clock_enable(mut emit: impl FnMut(&str)) {
    emit("[starry-k3] stage=4 apmu begin\n");
    let ctrl = (APMU_BASE_PADDR + APMU_CTRL_OFFSET) as usize;
    // SAFETY: identity-mapped APMU CTRL read; see the safety contract above.
    let before = unsafe { core::ptr::read_volatile(ctrl as *const u32) };
    let mut buf = [0u8; READ_LINE_BUF_LEN];
    emit(format_apmu_line(&mut buf, "before", before));
    let step1 = apmu_step1(before);
    let mut buf = [0u8; READ_LINE_BUF_LEN];
    emit(format_apmu_line(&mut buf, "step1", step1));
    // SAFETY: CTRL 0x3ec is the only APMU write target of this sequence.
    unsafe { core::ptr::write_volatile(ctrl as *mut u32, step1) };
    // SAFETY: identity-mapped APMU CTRL read.
    let after1 = unsafe { core::ptr::read_volatile(ctrl as *const u32) };
    let mut buf = [0u8; READ_LINE_BUF_LEN];
    emit(format_apmu_line(&mut buf, "after1", after1));
    delay_us(APMU_STEP_DELAY_US);
    let step2 = apmu_step2(after1);
    let mut buf = [0u8; READ_LINE_BUF_LEN];
    emit(format_apmu_line(&mut buf, "step2", step2));
    // SAFETY: CTRL 0x3ec is the only APMU write target of this sequence.
    unsafe { core::ptr::write_volatile(ctrl as *mut u32, step2) };
    // SAFETY: identity-mapped APMU CTRL read.
    let after2 = unsafe { core::ptr::read_volatile(ctrl as *const u32) };
    let mut buf = [0u8; READ_LINE_BUF_LEN];
    emit(format_apmu_line(&mut buf, "after2", after2));
    delay_us(APMU_STEP_DELAY_US);
    emit("[starry-k3] stage=4 apmu clock enable done\n");
}

/// Read-only MAC probe (stage=5): prints a begin marker, then
/// [`READS_PER_REG`] lines per register (raw value + classification), then a
/// done marker, through `emit`. Runs after [`apmu_clock_enable`].
///
/// # Safety
///
/// The MAC MMIO window `[MAC_BASE_PADDR, MAC_BASE_PADDR + MAC_WINDOW_SIZE)`
/// must be identity-mapped (K3 `devices.mmio-ranges`) with the GMAC bus clock
/// already enabled by [`apmu_clock_enable`]. The probe performs volatile
/// reads inside the DT-declared window only and never writes any register.
pub unsafe fn probe_and_report(mut emit: impl FnMut(&str)) {
    emit("[starry-k3] stage=5 mac probe begin base=0xcac82000 (read-only)\n");
    for (name, offset) in [("version", MAC_REG_VERSION), ("debug", MAC_REG_DEBUG)] {
        for read_idx in 1..=READS_PER_REG {
            let addr = (MAC_BASE_PADDR + offset) as usize;
            // SAFETY: identity-mapped MMIO read inside the DT-declared window;
            // see the function safety contract above.
            let value = unsafe { core::ptr::read_volatile(addr as *const u32) };
            let mut buf = [0u8; READ_LINE_BUF_LEN];
            emit(format_read_line(&mut buf, name, read_idx, value));
        }
    }
    emit("[starry-k3] stage=5 mac probe done (parking next)\n");
}
