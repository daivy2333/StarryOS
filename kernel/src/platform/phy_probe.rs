//! K3 PHY/MDIO bounded probe and link observation (MS09 Iteration 004,
//! tasks 5.1–5.2).
//!
//! Board facts (current-board DTB + the same-board reference R69
//! `k3_gmac`/`pinctrl` sources + Linux v6.18 upstream; source levels in the
//! Iteration 004 Plan Context):
//!
//! - GPIO controller `0xd4019000/0x100`, bank stride 0x40; `GPSR=0x08`
//!   (write-1 set high), `GPCR=0x0c` (write-1 set low), `GSDR=0x1c`
//!   (write-1 set output). The GMAC PHY reset line is GPIO1_5, active-low;
//!   DT delays are pre=0/assert=20000/post=100000 µs and the same-board
//!   reference floors `pre` at 2 µs. Exactly one hardware reset sequence per
//!   boot; only bank-1 bit 5 is ever written.
//! - Pad mux: MFPR `0xd401e000/0x400`, register offset = pin*4, mux field =
//!   bits [2:0]. Board DTB `gmac1-cfg`: pins 21–35 mux 1, pin 37 mux 0. Only
//!   a differing mux field is RMW-ed; every write is read back and must keep
//!   all other bits. IO power domain `0xd401e800/0x34`: pins 21–41 sit at
//!   offset 0x0c, 1.8 V domain = bit 2. Per the same-board reference usage
//!   (`soc/k3/pinctrl`, R69) the domain is set by one APBC ASAR unlock
//!   followed by exactly one whole-word write and is never read back.
//! - MDIO (DWMAC4): `ADDR 0x200`, `DATA 0x204`. Busy bit0, C45E bit1 kept
//!   clear (Clause 22 only), GOC read=`0b11<<2` / write=`0b01<<2`, CR divider
//!   bits [11:8] = 5 (same-board U-Boot eqos value, CSR 250–300 MHz), PHY
//!   bits [25:21], register bits [20:16]. A read writes ADDR only and samples
//!   DATA[15:0] after completion; a write stores DATA first. Every wait is
//!   bounded by 10 ms on the env's native clock; the deadline is taken once
//!   per wait, never reset by re-reading the busy bit. Timeouts report
//!   phase/register/raw ADDR and stop; no fake 0/ffff success, no DATA read
//!   before completion.
//! - Clause 22: BMCR=0, BMSR=1, PHYID1=2, PHYID2=3; advertisement registers
//!   4/9 are read-only diagnostics in stage 8. BMSR link is latch-low: two
//!   reads per sample, the second read is the current level.
//! - PHY address 1; expected identity `0x001cc916` (RTL8211F exact match in
//!   the upstream realtek driver).
//!
//! All hardware access goes through [`ProbeEnv`] so host tests drive the real
//! probe logic against a fake board. The trait is a probe seam, not a general
//! MMIO interface: callers pass only addresses from the constants below, all
//! inside DT-declared windows identity-mapped via `devices.mmio-ranges`.

use super::mac_probe::{
    push_bytes, push_u16_hex4, push_u32_hex8, push_usize, MAC_BASE_PADDR,
};

// ---------------------------------------------------------------------------
// Replaceable board access (probe seam, host-test driver)
// ---------------------------------------------------------------------------

/// Board access + native time source for the probe stages. On the kernel side
/// only the K3 first-byte park path implements this over the identity-mapped
/// windows; host tests implement it over a fake board. No timer IRQ is
/// required: the clock is the platform timebase read directly.
pub trait ProbeEnv {
    /// Reads one u32 from an adjudicated MMIO address (probe constants only).
    fn mmio_read(&mut self, paddr: u64) -> u32;
    /// Writes one u32 to an adjudicated MMIO address (probe constants only).
    fn mmio_write(&mut self, paddr: u64, value: u32);
    /// Monotonic nanoseconds since boot on the platform timebase.
    fn now_nanos(&self) -> u64;
    /// Busy-waits the given nanoseconds on the same timebase.
    fn wait_nanos(&mut self, nanos: u64);
}

// ---------------------------------------------------------------------------
// GPIO / pad / IO-PD facts
// ---------------------------------------------------------------------------

/// GPIO controller base from the board DT (`gpio@d4019000`).
pub const GPIO_BASE_PADDR: u64 = 0xd401_9000;
/// GPIO controller `reg` window size from the same DT entry.
pub const GPIO_WINDOW_SIZE: u64 = 0x100;
/// K3 GPIO bank stride (`k3_gmac/regs.rs:200`).
pub const GPIO_BANK_STRIDE: u64 = 0x40;
/// Port set-high register (write 1 to the pin bit sets the line high).
pub const GPIO_GPSR_OFF: u64 = 0x08;
/// Port clear-low register (write 1 to the pin bit sets the line low).
pub const GPIO_GPCR_OFF: u64 = 0x0c;
/// Set-direction register (write 1 to the pin bit sets output).
pub const GPIO_GSDR_OFF: u64 = 0x1c;
/// The GMAC PHY reset line is GPIO1_5 (board DTB `snps,reset-gpios`).
pub const PHY_RESET_BANK: u64 = 1;
pub const PHY_RESET_PIN: u32 = 5;
pub const PHY_RESET_BIT: u32 = 1 << PHY_RESET_PIN;
/// Byte offset of bank 1 inside the GPIO controller.
pub const GPIO_RESET_BANK_OFFSET: u64 = PHY_RESET_BANK * GPIO_BANK_STRIDE;

/// GPIO reset timing: board DTB `snps,reset-delays-us = <0 20000 100000>`;
/// the same-board reference floors `pre` at 2 µs.
pub const GPIO_PRE_DELAY_NS: u64 = 2_000;
pub const GPIO_ASSERT_DELAY_NS: u64 = 20_000_000;
pub const GPIO_POST_DELAY_NS: u64 = 100_000_000;

/// Pad mux register file base from the board DT (`pinctrl` MFPR window).
pub const PAD_MFPR_BASE_PADDR: u64 = 0xd401_e000;
/// MFPR window size; registers sit at `base + pin*4`.
pub const PAD_MFPR_WINDOW_SIZE: u64 = 0x400;
/// Mux field mask, bits [2:0] (`soc/k3/pinctrl/regs.rs`).
pub const PAD_MUX_MASK: u32 = 0x7;
/// Pins 21–35 mux 1, pin 37 mux 0 (board DTB `gmac1-cfg`).
pub const PAD_PINS: [usize; 16] = [
    21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 37,
];

/// APBC system controller window from the board DT
/// (`system-controller@d4015000`, `spacemit,k3-syscon-apbc`).
pub const APBC_BASE_PADDR: u64 = 0xd401_5000;
/// APBC window size.
pub const APBC_WINDOW_SIZE: u64 = 0x1000;
/// ASAR sub-window offset inside APBC (board DTB `spacemit,apbc = <0x82 0x50>`).
pub const APBC_ASAR_OFFSET: u64 = 0x50;
/// Unlock-key registers relative to `APBC_BASE_PADDR + APBC_ASAR_OFFSET`
/// (`soc/k3/pinctrl/regs.rs`, traced to Linux `pinctrl-k1.c`).
pub const APBC_ASFAR_OFFSET: u64 = 0x0;
pub const APBC_ASSAR_OFFSET: u64 = 0x4;
/// ASAR unlock magic keys; one unlock covers exactly the next io_pd access.
pub const APBC_ASFAR_AKEY: u32 = 0xbaba;
pub const APBC_ASSAR_AKEY: u32 = 0xeb10;

/// IO power-domain window base (pads 21–41 supply control).
pub const IOPD_BASE_PADDR: u64 = 0xd401_e800;
/// IO PD window size.
pub const IOPD_WINDOW_SIZE: u64 = 0x34;
/// Word holding the domain bits of pins 21–41 (`soc/k3/pinctrl` layout);
/// written whole per the reference usage, never read.
pub const IOPD_GMAC_OFFSET: u64 = 0x0c;
/// 1.8 V domain bit (board DTB `gmac1-cfg power-source = 1800`).
pub const IOPD_1V8_BIT: u32 = 1 << 2;

/// Target mux for a gmac1 pad; `None` outside the adjudicated pin set.
pub const fn pad_mux_for_pin(pin: usize) -> Option<u32> {
    match pin {
        21..=35 => Some(1),
        37 => Some(0),
        _ => None,
    }
}

/// Bits-preserving mux RMW value.
pub const fn pad_mux_rmw(old: u32, mux: u32) -> u32 {
    (old & !PAD_MUX_MASK) | mux
}

/// True when the mux field differs from the target (only then is a write
/// allowed).
pub const fn pad_mux_needed(old: u32, mux: u32) -> bool {
    old & PAD_MUX_MASK != mux
}

/// Writeback check: the mux field took the written value and every other bit
/// kept its pre-write value.
pub const fn pad_writeback_ok(old: u32, written: u32, readback: u32) -> bool {
    readback & PAD_MUX_MASK == written & PAD_MUX_MASK
        && readback & !PAD_MUX_MASK == old & !PAD_MUX_MASK
}

fn gpio_reset_addr(offset: u64) -> u64 {
    GPIO_BASE_PADDR + GPIO_RESET_BANK_OFFSET + offset
}

// ---------------------------------------------------------------------------
// MDIO transaction core (DWMAC4 Clause 22)
// ---------------------------------------------------------------------------

/// MDIO address register offset inside the MAC window (upstream `dwmac4.h`).
pub const MDIO_ADDR_OFFSET: u64 = 0x200;
/// MDIO data register offset inside the MAC window.
pub const MDIO_DATA_OFFSET: u64 = 0x204;
/// MDIO clock divider field value: same-board U-Boot eqos value, CSR
/// 250–300 MHz (not a measured frequency; never changed on failure).
pub const MDIO_CR: u32 = 5;
/// Bound for every MDIO wait, on the env's native clock.
pub const MDIO_TIMEOUT_NANOS: u64 = 10_000_000;
/// Generation operation code: read = `0b11`, write = `0b01` (bits [3:2]).
pub const GOC_READ: u32 = 0b11 << 2;
pub const GOC_WRITE: u32 = 0b01 << 2;
/// Trigger bit (bit0 of the address register).
pub const MII_ADDR_BUSY: u32 = 1 << 0;
/// Clause 45 select — must stay clear for Clause 22 transactions.
pub const MII_ADDR_C45E: u32 = 1 << 1;
/// The confirmed PHY sits at MDIO address 1 (board DTB / 001 collection).
pub const PHY_ADDR: u32 = 1;

/// Builds the MII address register trigger value; `None` for an out-of-range
/// PHY or register (rejected before any MMIO access).
pub const fn mii_addr_value(phy: u32, reg: u32, goc: u32) -> Option<u32> {
    if phy > 31 || reg > 31 {
        return None;
    }
    Some(MII_ADDR_BUSY | goc | (MDIO_CR << 8) | (reg << 16) | (phy << 21))
}

/// MDIO failure taxonomy: the phase, register and raw trigger value are all
/// reportable; no fake data value is ever produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MdioError {
    /// Register/PHY out of Clause 22 range; no MMIO access happened.
    IllegalReg { reg: u32 },
    /// Busy bit did not clear before the transaction within the bound.
    IdleTimeout { reg: u32, raw_addr: u32 },
    /// Busy bit did not clear after the trigger within the bound.
    CompletionTimeout { reg: u32, raw_addr: u32 },
}

fn mdio_reg_addr(offset: u64) -> u64 {
    MAC_BASE_PADDR + offset
}

/// Waits until the MDIO engine is idle, bounded once per call: the deadline
/// is taken at entry and re-reading the busy bit never extends it.
fn wait_busy_clear(env: &mut impl ProbeEnv) -> bool {
    let deadline = env.now_nanos() + MDIO_TIMEOUT_NANOS;
    loop {
        if env.mmio_read(mdio_reg_addr(MDIO_ADDR_OFFSET)) & MII_ADDR_BUSY == 0 {
            return true;
        }
        if env.now_nanos() >= deadline {
            return false;
        }
        core::hint::spin_loop();
    }
}

/// Clause 22 read: idle wait → ADDR trigger (read opcode) → completion wait →
/// DATA[15:0]. DATA is read only after a confirmed completion.
pub fn mdio_read(env: &mut impl ProbeEnv, phy: u32, reg: u32) -> Result<u16, MdioError> {
    let Some(raw_addr) = mii_addr_value(phy, reg, GOC_READ) else {
        return Err(MdioError::IllegalReg { reg });
    };
    if !wait_busy_clear(env) {
        return Err(MdioError::IdleTimeout { reg, raw_addr });
    }
    env.mmio_write(mdio_reg_addr(MDIO_ADDR_OFFSET), raw_addr);
    if !wait_busy_clear(env) {
        return Err(MdioError::CompletionTimeout { reg, raw_addr });
    }
    Ok((env.mmio_read(mdio_reg_addr(MDIO_DATA_OFFSET)) & 0xffff) as u16)
}

/// Clause 22 write: idle wait → DATA first → ADDR trigger (write opcode) →
/// completion wait. Illegal registers are rejected without any MMIO access.
pub fn mdio_write(env: &mut impl ProbeEnv, phy: u32, reg: u32, value: u16) -> Result<(), MdioError> {
    let Some(raw_addr) = mii_addr_value(phy, reg, GOC_WRITE) else {
        return Err(MdioError::IllegalReg { reg });
    };
    if !wait_busy_clear(env) {
        return Err(MdioError::IdleTimeout { reg, raw_addr });
    }
    env.mmio_write(mdio_reg_addr(MDIO_DATA_OFFSET), value as u32);
    env.mmio_write(mdio_reg_addr(MDIO_ADDR_OFFSET), raw_addr);
    if !wait_busy_clear(env) {
        return Err(MdioError::CompletionTimeout { reg, raw_addr });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Paged vendor access + device cause/ack facts (MS10 Iteration 001, task 2.1)
// ---------------------------------------------------------------------------
//
// D5 fixed device contract (board DTB + R69 same-position sources, adjudicated
// in the Iteration 001 Plan Context):
// - RTL8211F vendor pages are selected by writing reg `0x1f`; the selector
//   reads back the current page. Every paged access saves the original page
//   first and restores it last — a failed access still attempts the restore,
//   and a failed restore is reported separately from the primary failure.
// - INER: page `0xa42`, reg `0x12`, bit4 = link-change interrupt enable.
// - INSR: page `0xa43`, reg `0x1d` — clear-on-read. It is read exactly once
//   per serviced event and the value is retained as the cause snapshot.
// - Edge status: the `syscon-gpio-edge` sub-window `0xd4019800/0x10` holds
//   four read-only words. They are snapshotted whole (undecoded pin-bank bits
//   included) and never written.
// - MFPR35 (`0xd401e000 + 35*4 = 0xd401e08c`) is the PHY INTN pad. Its edge
//   field is bits [6:4] (RISE|FALL|CLEAR, R69 `soc/k3/pinctrl/regs.rs`). The
//   ack is: set CLEAR → read back → clear CLEAR (re-arm), with every bit
//   outside CLEAR preserved (mux and electrical bits untouched).

/// RTL8211F page selector register (present in every page; reads back the
/// currently selected page).
pub const PHY_PAGE_SEL_REG: u32 = 0x1f;
/// Vendor page holding INER (interrupt enable).
pub const PHY_PAGE_INER: u16 = 0xa42;
/// INER register offset inside page `a42`.
pub const PHY_INER_REG: u32 = 0x12;
/// INER bit4: link-change interrupt enable.
pub const PHY_INER_LINK_CHANGE: u16 = 1 << 4;
/// Vendor page holding INSR (interrupt status).
pub const PHY_PAGE_INSR: u16 = 0xa43;
/// INSR register offset inside page `a43`.
pub const PHY_INSR_REG: u32 = 0x1d;
/// INSR bit4: link-change cause (same position as the INER enable bit).
pub const PHY_INSR_LINK_CHANGE: u16 = 1 << 4;

/// Paged-access failure taxonomy: the primary operation and the page-restore
/// outcome are reported independently; no fake data value is ever produced.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PagedError {
    /// Reading the page selector failed; nothing was modified.
    SaveFailed(MdioError),
    /// The main operation failed; `restore` is the page-restore outcome.
    Primary {
        primary: MdioError,
        restore: Option<MdioError>,
    },
    /// The main operation succeeded but restoring the original page failed.
    RestoreOnly(MdioError),
}

/// Reads the currently selected page.
pub fn mdio_page(env: &mut impl ProbeEnv) -> Result<u16, MdioError> {
    mdio_read(env, PHY_ADDR, PHY_PAGE_SEL_REG)
}

/// Selects `page` (no restore); the caller pairs this with [`mdio_page`].
fn mdio_select_page(env: &mut impl ProbeEnv, page: u16) -> Result<(), MdioError> {
    mdio_write(env, PHY_ADDR, PHY_PAGE_SEL_REG, page)
}

/// Runs `f` with the vendor page selected, restoring the saved page after —
/// including on failure. Returns the restore outcome alongside `f`'s.
fn with_page<E: ProbeEnv, T>(
    env: &mut E,
    page: u16,
    f: impl FnOnce(&mut E) -> Result<T, MdioError>,
) -> Result<T, PagedError> {
    let saved = match mdio_page(env) {
        Ok(v) => v,
        Err(e) => return Err(PagedError::SaveFailed(e)),
    };
    if let Err(e) = mdio_select_page(env, page) {
        // The select write failed; restore anyway (idempotent) and report
        // both outcomes.
        let restore = mdio_select_page(env, saved).err();
        return Err(PagedError::Primary { primary: e, restore });
    }
    let core = f(env);
    let restore = mdio_select_page(env, saved).err();
    match (core, restore) {
        (Ok(v), None) => Ok(v),
        (Ok(_), Some(re)) => Err(PagedError::RestoreOnly(re)),
        (Err(pe), re) => Err(PagedError::Primary { primary: pe, restore: re }),
    }
}

/// Paged Clause 22 read: save page → select → read → restore.
pub fn paged_read(
    env: &mut impl ProbeEnv,
    page: u16,
    reg: u32,
) -> Result<u16, PagedError> {
    with_page(env, page, |env| mdio_read(env, PHY_ADDR, reg))
}

/// Paged Clause 22 write: save page → select → write → restore.
pub fn paged_write(
    env: &mut impl ProbeEnv,
    page: u16,
    reg: u32,
    value: u16,
) -> Result<(), PagedError> {
    with_page(env, page, |env| mdio_write(env, PHY_ADDR, reg, value))
}

/// Written and read-back INER values after the enable attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InerEnableEvidence {
    pub written: u16,
    pub readback: u16,
}

impl InerEnableEvidence {
    /// The link-change enable bit stuck in the register.
    pub const fn stuck(&self) -> bool {
        self.readback & PHY_INER_LINK_CHANGE != 0
    }
}

/// Enables the PHY link-change interrupt: a bits-preserving paged RMW of INER
/// a42:12 bit4, verified by a paged read-back (board runs falsify nothing
/// silently: an unstuck enable means INTB can never fire).
pub fn iner_enable_link_change(env: &mut impl ProbeEnv) -> Result<InerEnableEvidence, PagedError> {
    let before = paged_read(env, PHY_PAGE_INER, PHY_INER_REG)?;
    let after = before | PHY_INER_LINK_CHANGE;
    if after != before {
        paged_write(env, PHY_PAGE_INER, PHY_INER_REG, after)?;
    }
    let readback = paged_read(env, PHY_PAGE_INER, PHY_INER_REG)?;
    Ok(InerEnableEvidence {
        written: after,
        readback,
    })
}

/// `syscon-gpio-edge` sub-window base (board DT `reg = <0xd4019800 0x10>`).
pub const GPIO_EDGE_BASE_PADDR: u64 = 0xd401_9800;
/// Edge status words in the sub-window (four u32, read-only).
pub const GPIO_EDGE_WORDS: usize = 4;

/// Reads the four raw edge-status words. The window is never written and the
/// values are kept whole — undecoded pin-bank bits are retained, not guessed.
pub fn read_edge_words(env: &mut impl ProbeEnv) -> [u32; GPIO_EDGE_WORDS] {
    let mut words = [0u32; GPIO_EDGE_WORDS];
    for (i, w) in words.iter_mut().enumerate() {
        *w = env.mmio_read(GPIO_EDGE_BASE_PADDR + (i as u64) * 4);
    }
    words
}

/// MFPR of pin 35 (`PAD_MFPR_BASE_PADDR + 35*4`): the PHY INTN pad.
pub const PAD35_MFPR_PADDR: u64 = PAD_MFPR_BASE_PADDR + 35 * 4;
/// MFPR edge-detect rising-enable bit (R69 `pinctrl/regs.rs`).
pub const PAD_EDGE_RISE: u32 = 1 << 4;
/// MFPR edge-detect falling-enable bit.
pub const PAD_EDGE_FALL: u32 = 1 << 5;
/// MFPR edge-status clear bit (write-1 to clear the latched pad edge).
pub const PAD_EDGE_CLEAR: u32 = 1 << 6;
/// MFPR edge field mask, bits [6:4].
pub const PAD_EDGE_FIELD_MASK: u32 = PAD_EDGE_RISE | PAD_EDGE_FALL | PAD_EDGE_CLEAR;
/// GPIO mux function value (pin 35 handoff fact: the stock system leaves the
/// PHY INTN pad in GPIO function; the board DTB `ethernet-phy@1` declares it
/// as a pinctrl GPIO interrupt with falling-edge trigger).
pub const PAD_MUX_GPIO: u32 = 0;

/// Raw evidence of the pin 35 interrupt-path setup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PadEdgeSetupEvidence {
    pub before: u32,
    pub written: u32,
    pub readback: u32,
}

impl PadEdgeSetupEvidence {
    /// The written value took effect and changed only the mux field plus the
    /// EDGE field's CLEAR-release and RISE|FALL enables (every electrical
    /// bit preserved).
    pub const fn ok(&self) -> bool {
        let non_mux_written = self.written & !PAD_MUX_MASK;
        let non_mux_expected = (self.before & !PAD_MUX_MASK & !PAD_EDGE_CLEAR)
            | PAD_EDGE_RISE
            | PAD_EDGE_FALL;
        self.readback == self.written
            && non_mux_written == non_mux_expected
            && self.written & PAD_MUX_MASK == PAD_MUX_GPIO
    }
}

/// Arms the PHY INTN pad for the pinctrl GPIO interrupt path: mux → GPIO,
/// both edge enables set (falling = unplug, rising = plug-in — MS13 T7-R3;
/// the board DTB `ethernet-phy@1` declares only `<35 2>` falling, but the
/// plug-in rising edge is the only trigger that re-reads link after a
/// recovery), the CLEAR hold released (every pad resets with CLEAR=1 —
/// board runs 1–2 falsified latching with the reset default left in place),
/// every other bit preserved, then read back. Called once from the stage 10
/// setup before the window opens; the ack sequence later pulses CLEAR and
/// re-arms back to exactly this released state.
pub fn pad35_edge_setup(env: &mut impl ProbeEnv) -> PadEdgeSetupEvidence {
    let before = env.mmio_read(PAD35_MFPR_PADDR);
    let written =
        (before & !PAD_MUX_MASK & !PAD_EDGE_CLEAR) | PAD_EDGE_RISE | PAD_EDGE_FALL;
    env.mmio_write(PAD35_MFPR_PADDR, written);
    let readback = env.mmio_read(PAD35_MFPR_PADDR);
    PadEdgeSetupEvidence {
        before,
        written,
        readback,
    }
}

/// Ack write 1: set CLEAR, preserving every other bit (edge config, mux and
/// electrical bits).
pub const fn pad_edge_ack_set(old: u32) -> u32 {
    old | PAD_EDGE_CLEAR
}

/// Ack write 2 (re-arm): clear CLEAR, restoring every other bit.
pub const fn pad_edge_rearm(old: u32) -> u32 {
    old & !PAD_EDGE_CLEAR
}

/// Raw evidence of one pad-edge ack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PadEdgeAckEvidence {
    pub before: u32,
    pub set_readback: u32,
    pub rearm_readback: u32,
}

/// The D5 pad ack: set CLEAR → read back → clear CLEAR (re-arm). Only the
/// CLEAR bit ever differs from `before`; the caller snapshots every
/// destructive cause before calling this.
pub fn pad35_edge_ack(env: &mut impl ProbeEnv) -> PadEdgeAckEvidence {
    let before = env.mmio_read(PAD35_MFPR_PADDR);
    env.mmio_write(PAD35_MFPR_PADDR, pad_edge_ack_set(before));
    let set_readback = env.mmio_read(PAD35_MFPR_PADDR);
    env.mmio_write(PAD35_MFPR_PADDR, pad_edge_rearm(before));
    let rearm_readback = env.mmio_read(PAD35_MFPR_PADDR);
    PadEdgeAckEvidence {
        before,
        set_readback,
        rearm_readback,
    }
}

// ---------------------------------------------------------------------------
// Clause 22 registers and status semantics
// ---------------------------------------------------------------------------

pub const BMCR_REG: u32 = 0;
pub const BMSR_REG: u32 = 1;
pub const PHYID1_REG: u32 = 2;
pub const PHYID2_REG: u32 = 3;
/// Autonegotiation advertisement — read-only diagnostic (stage 8).
pub const ANA_REG: u32 = 4;
/// 1000BASE-T control — read-only diagnostic (stage 8).
pub const GBCR_REG: u32 = 9;

pub const BMCR_RESET: u16 = 0x8000;
pub const BMCR_LOOPBACK: u16 = 0x4000;
pub const BMCR_AN_ENABLE: u16 = 0x1000;
pub const BMCR_POWERDOWN: u16 = 0x0800;
pub const BMCR_ISOLATE: u16 = 0x0400;
pub const BMCR_AN_RESTART: u16 = 0x0200;
/// AN enable + restart, applied once by a bits-preserving RMW (stage 8).
pub const BMCR_AN_RESTART_VALUE: u16 = BMCR_AN_ENABLE | BMCR_AN_RESTART;

pub const BMSR_LINK: u16 = 0x0004;
pub const BMSR_AN_CAPABLE: u16 = 0x0008;
pub const BMSR_REMOTE_FAULT: u16 = 0x0010;
pub const BMSR_AN_COMPLETE: u16 = 0x0020;

/// Control bits that mean the PHY is not in a usable operating state.
pub const BMCR_ANOMALY_BITS: u16 = BMCR_RESET | BMCR_LOOPBACK | BMCR_POWERDOWN | BMCR_ISOLATE;

/// Expected PHY identity (upstream realtek exact match: RTL8211F).
pub const EXPECTED_PHY_ID: u32 = 0x001c_c916;

/// PHY identity classification. All-bits-unset/one and any `0xffff` word are
/// "no PHY observable"; only the exact expected identity continues.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhyIdClass {
    Rtl8211f,
    Unexpected,
    Unavailable,
}

pub const fn classify_phy_id(id1: u16, id2: u16) -> PhyIdClass {
    if id1 == 0xffff || id2 == 0xffff {
        return PhyIdClass::Unavailable;
    }
    match ((id1 as u32) << 16) | id2 as u32 {
        0 => PhyIdClass::Unavailable,
        EXPECTED_PHY_ID => PhyIdClass::Rtl8211f,
        _ => PhyIdClass::Unexpected,
    }
}

pub const fn phy_id_label(class: PhyIdClass) -> &'static str {
    match class {
        PhyIdClass::Rtl8211f => "rtl8211f",
        PhyIdClass::Unexpected => "unexpected",
        PhyIdClass::Unavailable => "unavailable",
    }
}

/// Ordered anomaly labels for the set control bits (at most 4; `count` is the
/// used prefix length).
pub const fn bmcr_anomaly_labels(bmcr: u16) -> ([&'static str; 4], usize) {
    let mut labels = ["", "", "", ""];
    let mut count = 0usize;
    if bmcr & BMCR_RESET != 0 {
        labels[count] = "reset";
        count += 1;
    }
    if bmcr & BMCR_LOOPBACK != 0 {
        labels[count] = "loopback";
        count += 1;
    }
    if bmcr & BMCR_POWERDOWN != 0 {
        labels[count] = "powerdown";
        count += 1;
    }
    if bmcr & BMCR_ISOLATE != 0 {
        labels[count] = "isolate";
        count += 1;
    }
    (labels, count)
}

/// BMSR-level anomalies: an all-zero/all-one bus signature or a PHY that
/// cannot autonegotiate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BmsrAnomaly {
    AllZero,
    AllOne,
    NoAnCapable,
}

pub const fn classify_bmsr_anomaly(bmsr: u16) -> Option<BmsrAnomaly> {
    match bmsr {
        0 => Some(BmsrAnomaly::AllZero),
        0xffff => Some(BmsrAnomaly::AllOne),
        _ if bmsr & BMSR_AN_CAPABLE == 0 => Some(BmsrAnomaly::NoAnCapable),
        _ => None,
    }
}

pub const fn bmsr_anomaly_label(anomaly: BmsrAnomaly) -> &'static str {
    match anomaly {
        BmsrAnomaly::AllZero => "all-zero",
        BmsrAnomaly::AllOne => "all-one",
        BmsrAnomaly::NoAnCapable => "no-an-capable",
    }
}

// ---------------------------------------------------------------------------
// Link observation (stage 8, task 5.2)
// ---------------------------------------------------------------------------

/// Observation window and sample interval (5.2 contract item 2; window
/// shortened from 120 s to 30 s per the 2026-10-03 operator instruction).
pub const LINK_WINDOW_NANOS: u64 = 30_000_000_000;
pub const LINK_SAMPLE_INTERVAL_NANOS: u64 = 100_000_000;

/// Physical link state for one sample. Negotiating covers the restart bit
/// still set or AN incomplete — never reported as up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkState {
    Up,
    Down,
    Negotiating,
    RemoteFault,
}

pub const fn link_state_label(state: LinkState) -> &'static str {
    match state {
        LinkState::Up => "up",
        LinkState::Down => "down",
        LinkState::Negotiating => "negotiating",
        LinkState::RemoteFault => "remote-fault",
    }
}

/// One observation sample: BMCR plus two BMSR reads (latch-low handling).
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub bmcr: u16,
    pub bmsr1: u16,
    pub bmsr2: u16,
}

impl Sample {
    /// BMSR link is latch-low: the second read is the current level.
    pub const fn current_link(&self) -> bool {
        self.bmsr2 & BMSR_LINK != 0
    }

    /// First read down, second read up: a latched drop was just cleared.
    pub const fn latched_down_cleared(&self) -> bool {
        self.bmsr1 & BMSR_LINK == 0 && self.bmsr2 & BMSR_LINK != 0
    }

    /// Sample decision: negotiating → remote fault → current link level.
    /// A remote fault is a definite non-up observation, not an access error.
    pub const fn state(&self) -> LinkState {
        if self.bmcr & BMCR_AN_RESTART != 0 || self.bmsr2 & BMSR_AN_COMPLETE == 0 {
            LinkState::Negotiating
        } else if self.bmsr2 & BMSR_REMOTE_FAULT != 0 {
            LinkState::RemoteFault
        } else if self.current_link() {
            LinkState::Up
        } else {
            LinkState::Down
        }
    }
}

// ---------------------------------------------------------------------------
// Line formatting (no alloc: fixed buffers, shared push helpers)
// ---------------------------------------------------------------------------

/// Buffer for one probe line built by the helpers below.
pub const PHY_LINE_BUF_LEN: usize = 128;

/// Builds one probe line: `fill` appends the bytes, the result must be ASCII.
fn line<'a>(buf: &'a mut [u8], fill: impl FnOnce(&mut [u8], &mut usize)) -> &'a str {
    let mut n = 0usize;
    fill(buf, &mut n);
    core::str::from_utf8(&buf[..n]).expect("probe line is ASCII")
}

fn push_str(buf: &mut [u8], n: &mut usize, s: &str) {
    push_bytes(buf, n, s.as_bytes());
}

fn push_bool(buf: &mut [u8], n: &mut usize, value: bool) {
    push_str(buf, n, if value { "1" } else { "0" });
}

fn push_labels(buf: &mut [u8], n: &mut usize, labels: &[&'static str], count: usize) {
    let mut i = 0usize;
    while i < count {
        if i > 0 {
            push_str(buf, n, ",");
        }
        push_str(buf, n, labels[i]);
        i += 1;
    }
}

fn push_u16(buf: &mut [u8], n: &mut usize, value: u16) {
    push_u16_hex4(buf, n, value);
}

fn mdio_timeout_line<'a>(buf: &'a mut [u8], stage: usize, err: &MdioError) -> &'a str {
    let (phase, reg, raw_addr) = match *err {
        MdioError::IdleTimeout { reg, raw_addr } => ("idle", reg, raw_addr),
        MdioError::CompletionTimeout { reg, raw_addr } => ("completion", reg, raw_addr),
        MdioError::IllegalReg { reg } => ("illegal", reg, 0),
    };
    line(buf, |b, n| {
        push_str(b, n, "[starry-k3] stage=");
        push_usize(b, n, stage);
        push_str(b, n, " mdio timeout phase=");
        push_str(b, n, phase);
        push_str(b, n, " reg=");
        push_usize(b, n, reg as usize);
        push_str(b, n, " addr=0x");
        push_u32_hex8(b, n, raw_addr);
        push_str(b, n, " stop\n");
    })
}

// ---------------------------------------------------------------------------
// stage 6: IO-PD voltage check → pad mux → one GPIO1_5 hardware reset
// ---------------------------------------------------------------------------

/// Stage-6 stop taxonomy; every stop happens before any dependent write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadGpioStop {
    /// Pad writeback broke the mux field or another register bit.
    MuxWriteback {
        pin: usize,
        before: u32,
        written: u32,
        readback: u32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PadGpioOutcome {
    Done,
    Stopped(PadGpioStop),
}

/// Stage 6 (task 5.1): set the IO-PD voltage domain per the R69 usage (one
/// ASAR unlock + one whole-word 1.8 V write, never read back), minimal
/// pad-mux RMW for the adjudicated gmac1 pins, then exactly one GPIO1_5 PHY
/// hardware reset. The GMAC bus clock must already be enabled (stage 4 runs
/// first). No soft reset, no repeated reset, no other electrical config.
pub fn run_pad_gpio<E: ProbeEnv>(env: &mut E, mut emit: impl FnMut(&str)) -> PadGpioOutcome {
    emit("[starry-k3] stage=6 pad/gpio begin\n");
    let mut buf = [0u8; PHY_LINE_BUF_LEN];

    // IO-PD voltage domain, R69 `set_io_pwr_domain` usage: ASFAR key → ASSAR
    // key → one whole-word io_pd write (1.8 V from the board DT). The three
    // writes stay adjacent — the unlock covers exactly the next io_pd access
    // and the register is never read — so there is no preflight and no
    // retry; a fault here shows as silence after the begin line.
    env.mmio_write(
        APBC_BASE_PADDR + APBC_ASAR_OFFSET + APBC_ASFAR_OFFSET,
        APBC_ASFAR_AKEY,
    );
    env.mmio_write(
        APBC_BASE_PADDR + APBC_ASAR_OFFSET + APBC_ASSAR_OFFSET,
        APBC_ASSAR_AKEY,
    );
    env.mmio_write(IOPD_BASE_PADDR + IOPD_GMAC_OFFSET, IOPD_1V8_BIT);
    emit(line(&mut buf, |b, n| {
        push_str(b, n, "[starry-k3] stage=6 iopd write=0x");
        push_u32_hex8(b, n, IOPD_1V8_BIT);
        push_str(b, n, " ok\n");
    }));

    for pin in PAD_PINS {
        let Some(mux) = pad_mux_for_pin(pin) else { continue };
        let addr = PAD_MFPR_BASE_PADDR + (pin as u64) * 4;
        let before = env.mmio_read(addr);
        if !pad_mux_needed(before, mux) {
            emit(line(&mut buf, |b, n| {
                push_str(b, n, "[starry-k3] stage=6 pin=");
                push_usize(b, n, pin);
                push_str(b, n, " mux=");
                push_usize(b, n, mux as usize);
                push_str(b, n, " before=0x");
                push_u32_hex8(b, n, before);
                push_str(b, n, " action=keep after=0x");
                push_u32_hex8(b, n, before);
                push_str(b, n, " ok\n");
            }));
            continue;
        }
        let written = pad_mux_rmw(before, mux);
        env.mmio_write(addr, written);
        let readback = env.mmio_read(addr);
        if !pad_writeback_ok(before, written, readback) {
            emit(line(&mut buf, |b, n| {
                push_str(b, n, "[starry-k3] stage=6 pin=");
                push_usize(b, n, pin);
                push_str(b, n, " mux=");
                push_usize(b, n, mux as usize);
                push_str(b, n, " before=0x");
                push_u32_hex8(b, n, before);
                push_str(b, n, " written=0x");
                push_u32_hex8(b, n, written);
                push_str(b, n, " readback=0x");
                push_u32_hex8(b, n, readback);
                push_str(b, n, " err=mux-writeback stop\n");
            }));
            return PadGpioOutcome::Stopped(PadGpioStop::MuxWriteback {
                pin,
                before,
                written,
                readback,
            });
        }
        emit(line(&mut buf, |b, n| {
            push_str(b, n, "[starry-k3] stage=6 pin=");
            push_usize(b, n, pin);
            push_str(b, n, " mux=");
            push_usize(b, n, mux as usize);
            push_str(b, n, " before=0x");
            push_u32_hex8(b, n, before);
            push_str(b, n, " action=write after=0x");
            push_u32_hex8(b, n, readback);
            push_str(b, n, " ok\n");
        }));
    }

    // Exactly one hardware reset sequence: direction → deassert → pre →
    // assert → assert delay → deassert → post. Only bank-1 bit 5 is written;
    // the GPSR/GPCR/GSDR registers act on written-1 bits.
    emit("[starry-k3] stage=6 gpio reset begin\n");
    env.mmio_write(gpio_reset_addr(GPIO_GSDR_OFF), PHY_RESET_BIT);
    env.mmio_write(gpio_reset_addr(GPIO_GPSR_OFF), PHY_RESET_BIT);
    env.wait_nanos(GPIO_PRE_DELAY_NS);
    env.mmio_write(gpio_reset_addr(GPIO_GPCR_OFF), PHY_RESET_BIT);
    env.wait_nanos(GPIO_ASSERT_DELAY_NS);
    env.mmio_write(gpio_reset_addr(GPIO_GPSR_OFF), PHY_RESET_BIT);
    env.wait_nanos(GPIO_POST_DELAY_NS);
    emit("[starry-k3] stage=6 gpio reset done\n");
    emit("[starry-k3] stage=6 pad/gpio done\n");
    PadGpioOutcome::Done
}

// ---------------------------------------------------------------------------
// stage 7: bounded MDIO → PHY identity and Clause 22 status
// ---------------------------------------------------------------------------

/// Stage-7 stop taxonomy; identity failures stop before any status access.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhyStop {
    MdioTimeout(MdioError),
    PhyUnavailable { id1: u16, id2: u16 },
    PhyUnexpected { id1: u16, id2: u16 },
    BmcrAnomaly { bmcr: u16 },
    BmsrAnomaly { bmsr: u16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhyStatus {
    Ok,
    Stopped(PhyStop),
}

fn mdio_stop(emit: &mut impl FnMut(&str), stage: usize, err: MdioError) -> PhyStatus {
    let mut buf = [0u8; PHY_LINE_BUF_LEN];
    emit(mdio_timeout_line(&mut buf, stage, &err));
    PhyStatus::Stopped(PhyStop::MdioTimeout(err))
}

/// Stage 7 (task 5.1): read PHYID1/2 at address 1, report raw values and the
/// classification, then (only for the confirmed identity) read BMCR/BMSR and
/// report the status with the anomaly checks. No vendor configuration, no
/// address scan, no writes to the PHY.
pub fn run_phy_identity<E: ProbeEnv>(env: &mut E, mut emit: impl FnMut(&str)) -> PhyStatus {
    emit("[starry-k3] stage=7 mdio/phy begin\n");
    let id1 = match mdio_read(env, PHY_ADDR, PHYID1_REG) {
        Ok(v) => v,
        Err(e) => return mdio_stop(&mut emit, 7, e),
    };
    let id2 = match mdio_read(env, PHY_ADDR, PHYID2_REG) {
        Ok(v) => v,
        Err(e) => return mdio_stop(&mut emit, 7, e),
    };
    let class = classify_phy_id(id1, id2);
    let mut buf = [0u8; PHY_LINE_BUF_LEN];
    emit(line(&mut buf, |b, n| {
        push_str(b, n, "[starry-k3] stage=7 phy id1=0x");
        push_u16(b, n, id1);
        push_str(b, n, " id2=0x");
        push_u16(b, n, id2);
        push_str(b, n, " phyid=0x");
        push_u32_hex8(b, n, ((id1 as u32) << 16) | id2 as u32);
        push_str(b, n, " result=");
        push_str(b, n, phy_id_label(class));
        push_str(b, n, "\n");
    }));
    match class {
        PhyIdClass::Unavailable => PhyStatus::Stopped(PhyStop::PhyUnavailable { id1, id2 }),
        PhyIdClass::Unexpected => PhyStatus::Stopped(PhyStop::PhyUnexpected { id1, id2 }),
        PhyIdClass::Rtl8211f => {
            let bmcr = match mdio_read(env, PHY_ADDR, BMCR_REG) {
                Ok(v) => v,
                Err(e) => return mdio_stop(&mut emit, 7, e),
            };
            let bmsr = match mdio_read(env, PHY_ADDR, BMSR_REG) {
                Ok(v) => v,
                Err(e) => return mdio_stop(&mut emit, 7, e),
            };
            let (labels, count) = bmcr_anomaly_labels(bmcr);
            if count > 0 {
                emit(line(&mut buf, |b, n| {
                    push_str(b, n, "[starry-k3] stage=7 phy bmcr=0x");
                    push_u16(b, n, bmcr);
                    push_str(b, n, " err=bmcr-anomaly(");
                    push_labels(b, n, &labels, count);
                    push_str(b, n, ") stop\n");
                }));
                return PhyStatus::Stopped(PhyStop::BmcrAnomaly { bmcr });
            }
            if let Some(anomaly) = classify_bmsr_anomaly(bmsr) {
                emit(line(&mut buf, |b, n| {
                    push_str(b, n, "[starry-k3] stage=7 phy bmsr=0x");
                    push_u16(b, n, bmsr);
                    push_str(b, n, " err=");
                    push_str(b, n, bmsr_anomaly_label(anomaly));
                    push_str(b, n, " stop\n");
                }));
                return PhyStatus::Stopped(PhyStop::BmsrAnomaly { bmsr });
            }
            emit(line(&mut buf, |b, n| {
                push_str(b, n, "[starry-k3] stage=7 phy bmcr=0x");
                push_u16(b, n, bmcr);
                push_str(b, n, " bmsr=0x");
                push_u16(b, n, bmsr);
                push_str(b, n, " link=");
                push_str(b, n, if bmsr & BMSR_LINK != 0 { "up" } else { "down" });
                push_str(b, n, " an-capable=1 an-complete=");
                push_bool(b, n, bmsr & BMSR_AN_COMPLETE != 0);
                push_str(b, n, "\n");
            }));
            emit("[starry-k3] stage=7 mdio/phy done\n");
            PhyStatus::Ok
        }
    }
}

// ---------------------------------------------------------------------------
// stage 8: one AN restart + bounded observation window
// ---------------------------------------------------------------------------

/// Mid-observation failures; each exits the window immediately.
#[derive(Clone, Copy, Debug)]
enum ObsStop {
    MdioTimeout(MdioError),
    InvalidBmsr { bmsr: u16, anomaly: BmsrAnomaly },
    BmcrAnomaly { bmcr: u16 },
}

/// Stage 8 (task 5.2): apply one bits-preserving AN restart, dump the
/// advertisement registers read-only, then sample BMCR + BMSR×2 every 100 ms
/// inside a fixed window (30 s since the 2026-10-03 operator instruction,
/// originally 120 s). State changes (and the initial state) are emitted; the
/// window and per-transaction deadlines are never extended. When the window
/// ends the last state is reported — the kernel never judges PASS and never
/// resets or reboots by itself.
pub fn run_link_observation<E: ProbeEnv>(env: &mut E, mut emit: impl FnMut(&str)) {
    let mut buf = [0u8; PHY_LINE_BUF_LEN];
    emit(line(&mut buf, |b, n| {
        push_str(b, n, "[starry-k3] stage=8 link observation begin window-ms=");
        push_usize(b, n, (LINK_WINDOW_NANOS / 1_000_000) as usize);
        push_str(b, n, " interval-ms=");
        push_usize(b, n, (LINK_SAMPLE_INTERVAL_NANOS / 1_000_000) as usize);
        push_str(b, n, "\n");
    }));
    let mut buf = [0u8; PHY_LINE_BUF_LEN];

    let bmcr_before = match mdio_read(env, PHY_ADDR, BMCR_REG) {
        Ok(v) => v,
        Err(e) => {
            emit(mdio_timeout_line(&mut buf, 8, &e));
            emit_observation_ended(&mut emit, None, false, Some(ObsStop::MdioTimeout(e)));
            return;
        }
    };
    let bmcr_written = bmcr_before | BMCR_AN_RESTART_VALUE;
    if let Err(e) = mdio_write(env, PHY_ADDR, BMCR_REG, bmcr_written) {
        emit(mdio_timeout_line(&mut buf, 8, &e));
        emit_observation_ended(&mut emit, None, false, Some(ObsStop::MdioTimeout(e)));
        return;
    }
    emit(line(&mut buf, |b, n| {
        push_str(b, n, "[starry-k3] stage=8 an restart bmcr-before=0x");
        push_u16(b, n, bmcr_before);
        push_str(b, n, " bmcr-written=0x");
        push_u16(b, n, bmcr_written);
        push_str(b, n, "\n");
    }));

    // Read-only advertisement diagnostics; never rewritten.
    let ana = match mdio_read(env, PHY_ADDR, ANA_REG) {
        Ok(v) => v,
        Err(e) => {
            emit(mdio_timeout_line(&mut buf, 8, &e));
            emit_observation_ended(&mut emit, None, false, Some(ObsStop::MdioTimeout(e)));
            return;
        }
    };
    let gbcr = match mdio_read(env, PHY_ADDR, GBCR_REG) {
        Ok(v) => v,
        Err(e) => {
            emit(mdio_timeout_line(&mut buf, 8, &e));
            emit_observation_ended(&mut emit, None, false, Some(ObsStop::MdioTimeout(e)));
            return;
        }
    };
    emit(line(&mut buf, |b, n| {
        push_str(b, n, "[starry-k3] stage=8 an advert reg4=0x");
        push_u16(b, n, ana);
        push_str(b, n, " reg9=0x");
        push_u16(b, n, gbcr);
        push_str(b, n, "\n");
    }));

    let (last, up_observed, stop) = observe(env, &mut emit);
    emit_observation_ended(&mut emit, last, up_observed, stop);
}

/// Emits the terminal observation line: clean window end (with an explicit
/// `no-link-observed` when no link was ever seen) or the error taxonomy.
fn emit_observation_ended(
    emit: &mut impl FnMut(&str),
    last: Option<LinkState>,
    up_observed: bool,
    stop: Option<ObsStop>,
) {
    let last_label = last.map(link_state_label).unwrap_or("none");
    let mut buf = [0u8; PHY_LINE_BUF_LEN];
    emit(line(&mut buf, |b, n| {
        push_str(b, n, "[starry-k3] stage=8 observation-ended ");
        match stop {
            None => {
                if up_observed {
                    push_str(b, n, "last=");
                    push_str(b, n, last_label);
                    push_str(b, n, " up-observed=yes\n");
                } else {
                    push_str(b, n, "last=");
                    push_str(b, n, last_label);
                    push_str(b, n, " up-observed=no no-link-observed\n");
                }
            }
            Some(ObsStop::MdioTimeout(_)) => {
                push_str(b, n, "error=mdio-timeout last=");
                push_str(b, n, last_label);
                push_str(b, n, " up-observed=");
                push_str(b, n, if up_observed { "yes" } else { "no" });
                push_str(b, n, "\n");
            }
            Some(ObsStop::InvalidBmsr { .. }) => {
                push_str(b, n, "error=invalid-bmsr last=");
                push_str(b, n, last_label);
                push_str(b, n, " up-observed=");
                push_str(b, n, if up_observed { "yes" } else { "no" });
                push_str(b, n, "\n");
            }
            Some(ObsStop::BmcrAnomaly { .. }) => {
                push_str(b, n, "error=bmcr-anomaly last=");
                push_str(b, n, last_label);
                push_str(b, n, " up-observed=");
                push_str(b, n, if up_observed { "yes" } else { "no" });
                push_str(b, n, "\n");
            }
        }
    }));
}

/// The sampling loop. Returns the last emitted state, whether up was ever
/// observed, and the stop reason (`None` = window expired).
fn observe<E: ProbeEnv>(
    env: &mut E,
    emit: &mut impl FnMut(&str),
) -> (Option<LinkState>, bool, Option<ObsStop>) {
    let t0 = env.now_nanos();
    // The window is fixed once from t0: plug cycles never extend it.
    let deadline = t0 + LINK_WINDOW_NANOS;
    let mut next_sample = t0;
    let mut last: Option<LinkState> = None;
    let mut up_observed = false;
    loop {
        if env.now_nanos() >= deadline {
            return (last, up_observed, None);
        }
        let bmcr = match mdio_read(env, PHY_ADDR, BMCR_REG) {
            Ok(v) => v,
            Err(e) => {
                emit_mdio_stop(emit, e);
                return (last, up_observed, Some(ObsStop::MdioTimeout(e)));
            }
        };
        let (labels, count) = bmcr_anomaly_labels(bmcr);
        if count > 0 {
            let mut buf = [0u8; PHY_LINE_BUF_LEN];
            emit(line(&mut buf, |b, n| {
                push_str(b, n, "[starry-k3] stage=8 phy bmcr=0x");
                push_u16(b, n, bmcr);
                push_str(b, n, " err=bmcr-anomaly(");
                push_labels(b, n, &labels, count);
                push_str(b, n, ") stop\n");
            }));
            return (last, up_observed, Some(ObsStop::BmcrAnomaly { bmcr }));
        }
        let bmsr1 = match mdio_read(env, PHY_ADDR, BMSR_REG) {
            Ok(v) => v,
            Err(e) => {
                emit_mdio_stop(emit, e);
                return (last, up_observed, Some(ObsStop::MdioTimeout(e)));
            }
        };
        let bmsr2 = match mdio_read(env, PHY_ADDR, BMSR_REG) {
            Ok(v) => v,
            Err(e) => {
                emit_mdio_stop(emit, e);
                return (last, up_observed, Some(ObsStop::MdioTimeout(e)));
            }
        };
        for bmsr in [bmsr1, bmsr2] {
            if let Some(anomaly) = classify_bmsr_anomaly(bmsr) {
                let mut buf = [0u8; PHY_LINE_BUF_LEN];
                emit(line(&mut buf, |b, n| {
                    push_str(b, n, "[starry-k3] stage=8 phy bmsr=0x");
                    push_u16(b, n, bmsr);
                    push_str(b, n, " err=");
                    push_str(b, n, bmsr_anomaly_label(anomaly));
                    push_str(b, n, " stop\n");
                }));
                return (
                    last,
                    up_observed,
                    Some(ObsStop::InvalidBmsr { bmsr, anomaly }),
                );
            }
        }
        let sample = Sample { bmcr, bmsr1, bmsr2 };
        let state = sample.state();
        if state == LinkState::Up {
            up_observed = true;
        }
        let changed = match last {
            None => true,
            Some(prev) => prev != state,
        };
        if changed {
            emit_sample(emit, &sample, state);
            last = Some(state);
        }
        // Pace to the next absolute sample slot; a slow sample delays but
        // never shifts the schedule or extends the window. Past the last
        // slot the remainder is slept out: the window ends exactly at the
        // fixed deadline instead of over-sampling the tail.
        next_sample += LINK_SAMPLE_INTERVAL_NANOS;
        let now = env.now_nanos();
        if now >= deadline {
            return (last, up_observed, None);
        }
        let wait_until = if next_sample < deadline { next_sample } else { deadline };
        if wait_until > now {
            env.wait_nanos(wait_until - now);
        }
    }
}

fn emit_mdio_stop(emit: &mut impl FnMut(&str), err: MdioError) {
    let mut buf = [0u8; PHY_LINE_BUF_LEN];
    emit(mdio_timeout_line(&mut buf, 8, &err));
}

fn emit_sample(emit: &mut impl FnMut(&str), sample: &Sample, state: LinkState) {
    let mut buf = [0u8; PHY_LINE_BUF_LEN];
    let latch = sample.latched_down_cleared();
    emit(line(&mut buf, |b, n| {
        push_str(b, n, "[starry-k3] stage=8 sample bmcr=0x");
        push_u16(b, n, sample.bmcr);
        push_str(b, n, " bmsr1=0x");
        push_u16(b, n, sample.bmsr1);
        push_str(b, n, " bmsr2=0x");
        push_u16(b, n, sample.bmsr2);
        push_str(b, n, " state=");
        push_str(b, n, link_state_label(state));
        if latch {
            push_str(b, n, " latch-cleared");
        }
        push_str(b, n, "\n");
    }));
}
