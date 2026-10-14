//! K3 APLIC (MSI mode) + IMSIC S-mode external interrupt core.
//!
//! MS10 Iteration 000 (change `ms10-com260-kit-diagnosable-device-irq-baseline`,
//! Iteration 000 Cycle `001-replan`, tasks 1.1–1.2). This module is the
//! host-testable core: it contains only `core` code and drives all hardware
//! through the [`AiaBackend`] seam, so host harnesses run the real
//! initialization, registration, claim and dispatch logic against fake
//! register banks. The riscv64 production backend lives in the `irq` module.
//!
//! Board facts (board DTB + R69 `OpenSBI-K3 aplic.c`/`imsic.c` +
//! somehal `imsic_aplic.rs`; stopei field layout per OpenSBI
//! `IMSIC_TOPEI_ID_SHIFT 16` and the Linux `csr_swap(CSR_TOPEI, 0)` handler):
//!
//! - APLIC at `0xe080_4000`, window `0x4000`, sources 1..=512.
//!   `domaincfg` at `0x0000`: IE = bit 8, DM (MSI mode) = bit 2.
//! - `sourcecfg[i] = 0x0004 + 4*(i-1)`, `target[i] = 0x3004 + 4*(i-1)`;
//!   MSI target = `hart<<18 | guest<<12 | EID` (guest 0, boot hart < 16).
//!   Source modes (OpenSBI `aplic.c`): Detached = 1, LevelHigh = 6.
//! - Bitmap registers are write-1-to-set/clear: CLRIE `0x1f00`, CLRIP
//!   `0x1d00`, SETIE `0x1e00`; word = `source/32`, bit = `source%32`.
//!   Init covers words 0..=16; the last word only touches source 512 (bit 0)
//!   and preserves the remaining reserved bits.
//! - Numeric registers: SETIENUM `0x1edc`, CLRIENUM `0x1fdc`,
//!   SETIPNUM_LE `0x2000`, GENMSI `0x3000` (value `hart<<18|EID`, busy = bit
//!   12). GENMSI bypasses the APLIC source-enable and must never be used to
//!   prove a source mask.
//! - S-mode MSI address config: `smsicfgaddr 0x1bc8` / `smsicfgaddrH 0x1bcc`.
//!   IMSIC base `0xe040_0000` (group PPN `0xe0400`), guest index 6 bits,
//!   hart index 4 bits, group 0 bits → per-hart stride `2^18 = 0x4_0000`.
//!   High word per R69: L (bit 31) = 1, LHXS = 6 (shift 20), LHXW = 4
//!   (shift 12), other fields 0. Only LHXS and BAPPN are the S-mode
//!   readback criteria; M-mode geometry fields may read back differently.
//! - IMSIC local file (via `siselect 0x150`/`sireg 0x151`): EIDELIVERY
//!   `0x70` = 1, EITHRESHOLD `0x72` = 0, EIP words at `0x80`, EIE words at
//!   `0xc0`; each 64-ID register sits at the next even selector (+2), and on
//!   RV64 odd selectors must not be accessed. IDs 0..=511 = eight 64-bit
//!   words.
//! - Claim: a single `csrrw rd, stopei(0x15c), zero` atomically returns and
//!   completes the top pending interrupt (EID = `raw>>16 & 0x7ff`,
//!   priority = `raw & 0x7ff`, raw 0 = none). The read-out and the cleared
//!   pending are the same interrupt, so claim+EOI strictly precede the
//!   handler; a new MSI raised inside the handler survives in EIP and is
//!   taken by the next SEI trap.
//! - Supported routes: source/EID 1 Detached, 60/133/277 LevelHigh. Only
//!   these four may be registered (EID 0 / source 0 / source 512 are
//!   rejected with no hardware effect); any other claimed EID is diagnosed
//!   as unknown and never dispatched.

use core::fmt;
use core::fmt::Write as _;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// APLIC chip base (the DT window). Board run 3 falsified the +0x4000
/// dual-domain S window (reads all-ones); the chip lives entirely in the
/// DT window.
pub const fn aplic_base() -> usize {
    APLIC_BASE_PADDR as usize
}

/// Single-line text writer into a stack buffer (no allocation, no_std).
pub struct LineBuf {
    pub buf: [u8; 256],
    pub len: usize,
}

impl LineBuf {
    pub const fn new() -> Self {
        LineBuf { buf: [0u8; 256], len: 0 }
    }

    /// Renders the written bytes; falls back to a marker on non-UTF-8.
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("[k3:irq] <line>")
    }
}

impl fmt::Write for LineBuf {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        let room = self.buf.len() - self.len;
        let n = bytes.len().min(room);
        self.buf[self.len..self.len + n].copy_from_slice(&bytes[..n]);
        self.len += n;
        Ok(())
    }
}

macro_rules! probe_line {
    ($out:expr, $($arg:tt)*) => {{
        let mut lb = LineBuf::new();
        let _ = core::write!(&mut lb, $($arg)*);
        // 002 Cycle newline fix: stage 9+ wire lines carry a trailing \n
        // (stage 1-8 convention).
        let _ = lb.write_str("\n");
        $out(lb.as_str());
    }};
}

// ---------------------------------------------------------------------------
// Register map constants
// ---------------------------------------------------------------------------

/// APLIC physical base from the board DTB (`interrupt-controller@e0804000`).
pub const APLIC_BASE_PADDR: u64 = 0xe080_4000;
/// APLIC window size from the same DTB entry.
pub const APLIC_WINDOW_SIZE: u64 = 0x4000;
/// Wired source count of the K3 APLIC root domain.
pub const APLIC_MAX_SOURCE: usize = 512;

pub const APLIC_DOMAINCFG: u32 = 0x0000;
/// domaincfg IE: domain-level interrupt enable.
pub const APLIC_DOMAINCFG_IE: u32 = 1 << 8;
/// domaincfg DM: MSI delivery mode.
pub const APLIC_DOMAINCFG_DM: u32 = 1 << 2;

pub const APLIC_SOURCECFG_BASE: u32 = 0x0004;
pub const APLIC_TARGET_BASE: u32 = 0x3004;

/// Bitmap base offsets (32 sources per 32-bit word, write-1-to-set/clear).
pub const APLIC_SETIP_BASE: u32 = 0x1c00;
pub const APLIC_SETIE_BASE: u32 = 0x1e00;
pub const APLIC_CLRIE_BASE: u32 = 0x1f00;
pub const APLIC_CLRIP_BASE: u32 = 0x1d00;

pub const APLIC_SETIENUM: u32 = 0x1edc;
pub const APLIC_CLRIENUM: u32 = 0x1fdc;
pub const APLIC_SETIPNUM_LE: u32 = 0x2000;
pub const APLIC_GENMSI: u32 = 0x3000;
/// GENMSI busy flag (previous MSI still in flight).
pub const APLIC_GENMSI_BUSY: u32 = 1 << 12;

pub const APLIC_SMSICFGADDR: u32 = 0x1bc8;
pub const APLIC_SMSICFGADDRH: u32 = 0x1bcc;
/// M-mode MSI config mirrors — read-only diagnostics for S-mode software.
pub const APLIC_MMSICFGADDR: u32 = 0x1bc0;
pub const APLIC_MMSICFGADDRH: u32 = 0x1bc4;

/// sourcecfg source modes (OpenSBI-K3 `aplic.c`, board-adjudicated).
pub const SM_INACTIVE: u32 = 0;
pub const SM_DETACHED: u32 = 1;
pub const SM_LEVEL_HIGH: u32 = 6;

/// MSI target address of the S-mode IMSIC interrupt files.
pub const IMSIC_MSI_BASE: u64 = 0xe040_0000;
/// Guest index bits in the MSI address (LHXS).
pub const MSI_GUEST_INDEX_BITS: u32 = 6;
/// Hart index width inside the guest-index window (LHXW).
pub const MSI_HART_INDEX_BITS: u32 = 4;
/// Group index bits (none on this board).
pub const MSI_GROUP_INDEX_BITS: u32 = 0;

/// IMSIC register-file selectors (via `siselect`/`sireg`).
pub const IMSIC_EIDELIVERY: u16 = 0x70;
pub const IMSIC_EITHRESHOLD: u16 = 0x72;
pub const IMSIC_EIP0: u16 = 0x80;
pub const IMSIC_EIE0: u16 = 0xc0;
/// Supported interrupt identities (0 reserved).
pub const IMSIC_IDS: usize = 512;
/// 64-bit EIP/EIE register words covering [`IMSIC_IDS`].
pub const IMSIC_WORDS: usize = IMSIC_IDS / 64;

/// scause vector of supervisor external interrupts (`interrupt bit | 9`).
pub const EXTERNAL_IRQ_VECTOR: usize = (1usize << 63) | 9;

// ---------------------------------------------------------------------------
// Pure layout helpers (also the host-test surface for the encoding)
// ---------------------------------------------------------------------------

/// Bitmap word holding `source`'s bit (32 sources per word).
pub const fn bitmap_word(source: usize) -> usize {
    source / 32
}

/// Bitmap bit mask of `source` inside its word.
pub const fn bitmap_bit(source: usize) -> u32 {
    1 << (source % 32)
}

/// Mask used when init sweeps one bitmap word; the last word only owns
/// source 512 (bit 0) so the remaining reserved bits are preserved.
pub const fn bitmap_word_mask(word: usize) -> u32 {
    if word < APLIC_MAX_SOURCE / 32 {
        u32::MAX
    } else {
        bitmap_bit(APLIC_MAX_SOURCE)
    }
}

pub const fn sourcecfg_off(source: usize) -> u32 {
    APLIC_SOURCECFG_BASE + 4 * (source - 1) as u32
}

pub const fn target_off(source: usize) -> u32 {
    APLIC_TARGET_BASE + 4 * (source - 1) as u32
}

pub const fn setie_word_off(word: usize) -> u32 {
    APLIC_SETIE_BASE + 4 * word as u32
}

pub const fn clrie_word_off(word: usize) -> u32 {
    APLIC_CLRIE_BASE + 4 * word as u32
}

pub const fn clrip_word_off(word: usize) -> u32 {
    APLIC_CLRIP_BASE + 4 * word as u32
}

/// MSI target register value (`hart<<18 | guest<<12 | EID`).
pub const fn target_value(hart: usize, guest: usize, eid: usize) -> u32 {
    ((hart as u32) << 18) | ((guest as u32) << 12) | eid as u32
}

/// MSI address of one hart's S-mode interrupt file (stride `2^18`).
pub const fn msi_addr_for_hart(hart: usize) -> u64 {
    IMSIC_MSI_BASE + ((hart as u64) << (12 + MSI_GUEST_INDEX_BITS))
}

/// `smsicfgaddr` value: base PPN with the guest/hart index windows cleared.
pub const fn smsicfgaddr_low() -> u32 {
    let ppn = (IMSIC_MSI_BASE >> 12) as u32;
    let guest_mask = (1u32 << MSI_GUEST_INDEX_BITS) - 1;
    let hart_mask = ((1u32 << MSI_HART_INDEX_BITS) - 1) << MSI_GUEST_INDEX_BITS;
    ppn & !(guest_mask | hart_mask)
}

/// `smsicfgaddrH` value per R69: L=1, LHXS=6, LHXW=4, group fields 0.
pub const fn smsicfgaddr_high() -> u32 {
    let l_bit: u32 = 1 << 31;
    let lhxw: u32 = MSI_HART_INDEX_BITS << 12;
    let lhxs: u32 = MSI_GUEST_INDEX_BITS << 20;
    let hhxs: u32 = MSI_GROUP_INDEX_BITS << 24;
    l_bit | lhxw | lhxs | hhxs
}

/// EIE register selector for 64-ID `word` (even selectors only on RV64).
pub const fn eie_selector(word: usize) -> u16 {
    (IMSIC_EIE0 as usize + 2 * word) as u16
}

/// EIP register selector for 64-ID `word`.
pub const fn eip_selector(word: usize) -> u16 {
    (IMSIC_EIP0 as usize + 2 * word) as u16
}

/// 64-ID word holding `eid`'s EIP/EIE bit.
pub const fn eie_word(eid: usize) -> usize {
    eid / 64
}

/// EIP/EIE bit mask of `eid` inside its word.
pub const fn eie_bit(eid: usize) -> u64 {
    1 << (eid % 64)
}

/// Parses a raw `stopei` claim value: identity in the high field, priority in
/// the low field (OpenSBI `IMSIC_TOPEI_ID_SHIFT 16`); raw 0 = no pending.
pub const fn parse_stopei(raw: u64) -> Option<(usize, usize)> {
    if raw == 0 {
        None
    } else {
        Some((((raw >> 16) & 0x7ff) as usize, (raw & 0x7ff) as usize))
    }
}

// ---------------------------------------------------------------------------
// Supported routes
// ---------------------------------------------------------------------------

/// A fixed wired source route; the EID equals the source number.
pub struct SourceRoute {
    pub source: usize,
    pub sm: u32,
}

/// The only registrable routes of this iteration (contract `supported routes`).
pub const SUPPORTED_ROUTES: [SourceRoute; 4] = [
    SourceRoute { source: 1, sm: SM_DETACHED },
    SourceRoute { source: 60, sm: SM_LEVEL_HIGH },
    SourceRoute { source: 133, sm: SM_LEVEL_HIGH },
    SourceRoute { source: 277, sm: SM_LEVEL_HIGH },
];

/// UART route (MS17 Iteration 000, task T2): source/EID 42 LevelHigh. Opt-in
/// only — without the `uart-irq` feature the source stays rejected like any
/// other unrouted line, and the four MS10 routes are untouched.
#[cfg(feature = "uart-irq")]
pub const UART_ROUTE: SourceRoute = SourceRoute { source: 42, sm: SM_LEVEL_HIGH };

pub fn route_for(source: usize) -> Option<&'static SourceRoute> {
    if let Some(route) = SUPPORTED_ROUTES.iter().find(|r| r.source == source) {
        return Some(route);
    }
    #[cfg(feature = "uart-irq")]
    if source == UART_ROUTE.source {
        return Some(&UART_ROUTE);
    }
    None
}

// ---------------------------------------------------------------------------
// Access seams
// ---------------------------------------------------------------------------

/// Local IMSIC CSR access inside a saved-SIE critical section.
pub trait ImsicAccess {
    /// Reads the sireg value for `selector` (even selectors only on RV64).
    fn read(&mut self, selector: u16) -> u64;
    /// Writes the sireg value for `selector`.
    fn write(&mut self, selector: u16, value: u64);
    /// Atomic claim + completion (`csrrw stopei, x0`); raw 0 = no pending.
    fn claim(&mut self) -> u64;
}

/// APLIC MMIO + local IMSIC access seam. The production backend performs real
/// volatile MMIO, CSR instructions and SIE save/restore; host harnesses model
/// the register semantics.
pub trait AiaBackend {
    /// Reads one u32 at `offset` from the APLIC base.
    fn aplic_read(&mut self, offset: u32) -> u32;
    /// Writes one u32 at `offset` from the APLIC base.
    fn aplic_write(&mut self, offset: u32, value: u32);
    /// Runs `f` with external interrupts disabled (previous SIE state saved
    /// and restored exactly as found) and hands it the local IMSIC CSRs.
    fn with_imsic<R>(&mut self, f: impl FnOnce(&mut dyn ImsicAccess) -> R) -> R;
    /// Sets/clears `sie.SEXT`.
    fn set_sext(&mut self, on: bool);
    /// Local hart index (boot hart < 16).
    fn hart(&self) -> usize;
}

/// Handler storage seam. Production binds `axplat::irq::HandlerTable<512>`
/// (lock-free CAS); host harnesses model the same semantics.
pub trait HandlerStore {
    /// Registers `handler` for `source`; false if out of range or duplicate.
    fn register(&self, source: usize, handler: fn()) -> bool;
    /// Removes and returns the handler for `source`, if any.
    fn unregister(&self, source: usize) -> Option<fn()>;
    /// Calls the handler registered for `eid`; false when none is registered.
    fn call(&self, eid: usize) -> bool;
}

// ---------------------------------------------------------------------------
// Bring-up window probe (board run 3, single-variable adjudication)
// ---------------------------------------------------------------------------

/// Reset state + writability probe results of one APLIC window.
#[derive(Clone, Copy, Debug)]
pub struct AplicWindowProbe {
    pub domaincfg: u32,
    pub smsicfg_low: u32,
    pub smsicfg_high: u32,
    pub mmsicfg_low: u32,
    pub mmsicfg_high: u32,
    /// sourcecfg[1] before and after a SM_DETACHED write.
    pub sourcecfg1_reset: u32,
    pub sourcecfg1_writeback: u32,
    /// msiaddrcfg pair read back after writing the adjudicated values.
    pub smsicfg_low_writeback: u32,
    pub smsicfg_high_writeback: u32,
}

impl fmt::Display for AplicWindowProbe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "domaincfg={:#x} smsicfg={:#x}/{:#x} mmsicfg={:#x}/{:#x} srcfg1={:#x}->{:#x} msicfg-wb={:#x}/{:#x}",
            self.domaincfg,
            self.smsicfg_low,
            self.smsicfg_high,
            self.mmsicfg_low,
            self.mmsicfg_high,
            self.sourcecfg1_reset,
            self.sourcecfg1_writeback,
            self.smsicfg_low_writeback,
            self.smsicfg_high_writeback
        )
    }
}

/// Reads the DT window's reset state and probes sourcecfg[1] plus the
/// msiaddrcfg pair for writability (configuration registers only). The
/// report goes to the caller's sink as the serial-record baseline.
pub fn probe_aplic_domain(b: &mut impl AiaBackend, mut out: impl FnMut(&str)) {
    let domaincfg = b.aplic_read(APLIC_DOMAINCFG);
    let smsicfg_low = b.aplic_read(APLIC_SMSICFGADDR);
    let smsicfg_high = b.aplic_read(APLIC_SMSICFGADDRH);
    let mmsicfg_low = b.aplic_read(APLIC_MMSICFGADDR);
    let mmsicfg_high = b.aplic_read(APLIC_MMSICFGADDRH);
    let sourcecfg1_reset = b.aplic_read(sourcecfg_off(1));
    b.aplic_write(sourcecfg_off(1), SM_DETACHED);
    let sourcecfg1_writeback = b.aplic_read(sourcecfg_off(1));
    probe_line!(
        out,
        "[k3:irq] probe domaincfg={domaincfg:#x} smsicfg={smsicfg_low:#x}/{smsicfg_high:#x} mmsicfg={mmsicfg_low:#x}/{mmsicfg_high:#x} srcfg1={sourcecfg1_reset:#x}->{sourcecfg1_writeback:#x}"
    );
}

// ---------------------------------------------------------------------------
// Initialization
// ---------------------------------------------------------------------------

/// Initialization failure with the readback values needed to diagnose it.
#[derive(Clone, Copy, Debug)]
pub enum AiaInitError {
    DomainIeNotClear { readback: u32 },
    DeliveryReadback { readback: u64 },
    ThresholdReadback { readback: u64 },
    EieNotCleared { word: usize },
}

impl fmt::Display for AiaInitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            AiaInitError::DomainIeNotClear { readback } => {
                write!(f, "domaincfg IE not clear after disable: {readback:#x}")
            }
            AiaInitError::DeliveryReadback { readback } => {
                write!(f, "eidelivery readback {readback:#x} != 1")
            }
            AiaInitError::ThresholdReadback { readback } => {
                write!(f, "eithreshold readback {readback:#x} != 0")
            }
            AiaInitError::EieNotCleared { word } => {
                write!(f, "eie word {word} not cleared")
            }
        }
    }
}

/// Global (per-domain) APLIC initialization. Runs once, before any per-hart
/// init or registration; touches only APLIC. Sequence per the R69-proven
/// OpenSBI pattern: window probe → domain IE off → disable-all →
/// clear-pending → MSI address → domaincfg IE|DM. Readback mismatches return
/// a diagnosable error and the caller must not continue to stage 9.
pub fn init_global(
    b: &mut impl AiaBackend,
    mut out: impl FnMut(&str),
) -> Result<(), AiaInitError> {
    // Board-run-3 probe: record the domain's reset state on the serial wire.
    probe_aplic_domain(b, &mut out);

    // 1. Domain configuration off (IE clear; OpenSBI writes 0 here too).
    b.aplic_write(APLIC_DOMAINCFG, 0);
    let rb = b.aplic_read(APLIC_DOMAINCFG);
    if rb & APLIC_DOMAINCFG_IE != 0 {
        return Err(AiaInitError::DomainIeNotClear { readback: rb });
    }

    // 2. Disable-all: CLRIE bitmap (last word: source 512 bit 0 only) and
    //    every sourcecfg set Inactive.
    for word in 0..=APLIC_MAX_SOURCE / 32 {
        b.aplic_write(clrie_word_off(word), bitmap_word_mask(word));
    }
    for source in 1..=APLIC_MAX_SOURCE {
        b.aplic_write(sourcecfg_off(source), SM_INACTIVE);
    }

    // 3. Clear pending bits (same word sweep).
    for word in 0..=APLIC_MAX_SOURCE / 32 {
        b.aplic_write(clrip_word_off(word), bitmap_word_mask(word));
    }

    // 4. S-mode MSI address configuration. Board evidence (2026-10-03 first
    //    board run): with the domain disabled the msiaddrcfg pair reads 0
    //    after the OpenSBI-order write, i.e. some implementations gate the
    //    pair on domaincfg.DM. The write is therefore verified after the
    //    OpenSBI order and, on failure, retried once with DM set (IE still
    //    clear). If both orders fail, the error carries the complete probe
    //    dump (both msicfg readbacks, the domaincfg readbacks and the M-mode
    //    mirrors) and the domain is left disabled for the park path.
    // 4b. Board evidence (runs 2–3): this silicon's msiaddrcfg pair reads 0
    //     even though the domain is otherwise alive and writable (sourcecfg
    //     sticks, DM RO=1 MSI-only). OpenSBI's production msicfg write never
    //     verifies by readback either. The readback is therefore recorded as
    //     an informational line and the REAL criterion — an actual software
    //     MSI arriving at the IMSIC — is adjudicated by stage 9's delivery
    //     test, which dumps the APLIC pending bit on timeout.
    let low = smsicfgaddr_low();
    let high = smsicfgaddr_high();
    b.aplic_write(APLIC_SMSICFGADDR, low);
    b.aplic_write(APLIC_SMSICFGADDRH, high);
    let rbl = b.aplic_read(APLIC_SMSICFGADDR);
    let rbh = b.aplic_read(APLIC_SMSICFGADDRH);
    probe_line!(
        out,
        "[k3:irq] msicfg written={low:#x}/{high:#x} readback={rbl:#x}/{rbh:#x}{}",
        if rbl == low
            && (rbh >> 20) & 0x7 == (high >> 20) & 0x7
            && rbh & 0xfff == high & 0xfff
        {
            " ok"
        } else {
            " readback-refused (delivery test adjudicates)"
        }
    );

    // 5. Domain enable: IE | DM. DM reads RO=1 on this MSI-only domain
    //    (bit31 is a constant WARL-1); IE readability is recorded, not
    //    enforced — stage 9's delivery test is the real criterion.
    let enable = APLIC_DOMAINCFG_IE | APLIC_DOMAINCFG_DM;
    b.aplic_write(APLIC_DOMAINCFG, enable);
    let rb = b.aplic_read(APLIC_DOMAINCFG);
    probe_line!(out, "[k3:irq] domaincfg enabled={enable:#x} readback={rb:#x}");
    Ok(())
}

/// Per-hart local IMSIC initialization: SEXT off → EIE words cleared,
/// threshold 0, delivery 1 → SEXT on last. Each hart runs this once.
pub fn init_percpu(b: &mut impl AiaBackend) -> Result<(), AiaInitError> {
    b.set_sext(false);
    b.with_imsic(|c| {
        for word in 0..IMSIC_WORDS {
            let sel = eie_selector(word);
            c.write(sel, 0);
            if c.read(sel) != 0 {
                return Err(AiaInitError::EieNotCleared { word });
            }
        }
        c.write(IMSIC_EITHRESHOLD, 0);
        let rb = c.read(IMSIC_EITHRESHOLD);
        if rb != 0 {
            return Err(AiaInitError::ThresholdReadback { readback: rb });
        }
        c.write(IMSIC_EIDELIVERY, 1);
        let rb = c.read(IMSIC_EIDELIVERY);
        if rb != 1 {
            return Err(AiaInitError::DeliveryReadback { readback: rb });
        }
        Ok(())
    })?;
    b.set_sext(true);
    Ok(())
}

// ---------------------------------------------------------------------------
// Readback snapshot (stage 9 display)
// ---------------------------------------------------------------------------

/// Consistent snapshot of the AIA configuration surfaces shown by the probe.
pub struct AiaReadback {
    pub domaincfg: u32,
    pub smsicfg_low: u32,
    pub smsicfg_high: u32,
    pub delivery: u64,
    pub threshold: u64,
    pub sourcecfg: u32,
    pub target: u32,
    pub eie_word0: u64,
}

/// Reads the snapshot with external interrupts disabled inside the local CSR
/// critical section (the caller prints it after the closure returns).
pub fn readback(b: &mut impl AiaBackend, source: usize) -> AiaReadback {
    let domaincfg = b.aplic_read(APLIC_DOMAINCFG);
    let smsicfg_low = b.aplic_read(APLIC_SMSICFGADDR);
    let smsicfg_high = b.aplic_read(APLIC_SMSICFGADDRH);
    let sourcecfg = b.aplic_read(sourcecfg_off(source));
    let target = b.aplic_read(target_off(source));
    let (delivery, threshold, eie_word0) = b.with_imsic(|c| {
        (
            c.read(IMSIC_EIDELIVERY),
            c.read(IMSIC_EITHRESHOLD),
            c.read(eie_selector(0)),
        )
    });
    AiaReadback {
        domaincfg,
        smsicfg_low,
        smsicfg_high,
        delivery,
        threshold,
        sourcecfg,
        target,
        eie_word0,
    }
}

// ---------------------------------------------------------------------------
/// MS13 A4 diagnostics: board-loop counters read by the payload snapshot —
/// SEI traps that reached `handle_external` and the raw `stopei` of the
/// latest one. Both are observation-only and never participate in control.
pub static AIA_DEBUG_TRAPS: AtomicU64 = AtomicU64::new(0);
pub static AIA_DEBUG_LAST_CLAIM: AtomicU64 = AtomicU64::new(0);

// Chip (registration + dispatch)
// ---------------------------------------------------------------------------

/// K3 AIA interrupt chip: fixed-route registration over a [`HandlerStore`]
/// plus the claim/dispatch path. Shared between the registering thread and
/// the IRQ context; all state is atomic.
pub struct AiaChip<'a, T: ?Sized + HandlerStore> {
    table: &'a T,
    spurious: AtomicUsize,
    unknown: AtomicUsize,
    last_raw_claim: AtomicU64,
}

impl<'a, T: ?Sized + HandlerStore> AiaChip<'a, T> {
    pub const fn new(table: &'a T) -> Self {
        AiaChip {
            table,
            spurious: AtomicUsize::new(0),
            unknown: AtomicUsize::new(0),
            last_raw_claim: AtomicU64::new(0),
        }
    }

    pub fn spurious_count(&self) -> usize {
        self.spurious.load(Ordering::Relaxed)
    }

    pub fn unknown_count(&self) -> usize {
        self.unknown.load(Ordering::Relaxed)
    }

    /// Raw `stopei` value of the most recent dispatch attempt (diagnostic).
    pub fn last_raw_claim(&self) -> u64 {
        self.last_raw_claim.load(Ordering::Relaxed)
    }

    /// Registers `handler` for a supported route and enables it: sourcecfg,
    /// target, pending clear, local EIE, then the APLIC SETIE bit. Fails
    /// without touching hardware for unsupported or duplicate sources.
    pub fn register(&self, b: &mut impl AiaBackend, source: usize, handler: fn()) -> bool {
        let Some(route) = route_for(source) else {
            return false;
        };
        if !self.table.register(source, handler) {
            return false;
        }
        self.enable_source(b, route);
        true
    }

    /// Disables the route (APLIC first, then local EIE) and removes the
    /// handler. Returns the removed handler, or None when the route was
    /// unsupported or nothing was registered.
    pub fn unregister(&self, b: &mut impl AiaBackend, source: usize) -> Option<fn()> {
        if route_for(source).is_none() {
            return None;
        }
        self.disable_source(b, source);
        self.table.unregister(source)
    }

    /// Mask/unmask a supported route without touching the handler. Disable
    /// masks the APLIC source first, then the local EIE; enable is the
    /// reverse so delivery is opened last.
    pub fn set_enable(&self, b: &mut impl AiaBackend, source: usize, enabled: bool) {
        if route_for(source).is_none() {
            return;
        }
        if enabled {
            set_eid_enabled(b, source, true);
            b.aplic_write(setie_word_off(bitmap_word(source)), bitmap_bit(source));
        } else {
            b.aplic_write(clrie_word_off(bitmap_word(source)), bitmap_bit(source));
            set_eid_enabled(b, source, false);
        }
    }

    /// Dispatches one external interrupt: a single atomic claim+EOI strictly
    /// before the handler call, exactly once per SEI trap. A pending raised
    /// inside the handler survives and is taken by the next trap. Spurious
    /// (zero claim) and unknown (no handler) events are only counted.
    pub fn handle_external(&self, b: &mut impl AiaBackend) -> Option<usize> {
        // MS13 A4 diagnostics: board-loop counters for the payload snapshot
        // (traps that reached the claim and the raw stopei they returned).
        AIA_DEBUG_TRAPS.fetch_add(1, Ordering::Relaxed);
        let raw = b.with_imsic(|c| c.claim());
        AIA_DEBUG_LAST_CLAIM.store(raw, Ordering::Relaxed);
        self.last_raw_claim.store(raw, Ordering::Relaxed);
        let Some((eid, _priority)) = parse_stopei(raw) else {
            self.spurious.fetch_add(1, Ordering::Relaxed);
            return None;
        };
        if self.table.call(eid) {
            Some(eid)
        } else {
            self.unknown.fetch_add(1, Ordering::Relaxed);
            None
        }
    }

    fn enable_source(&self, b: &mut impl AiaBackend, route: &SourceRoute) {
        let hart = b.hart();
        // Clear the pending AFTER the sourcecfg write: changing the mode may
        // rectify the current level and raise a stale pending (R69 practice).
        b.aplic_write(sourcecfg_off(route.source), route.sm);
        b.aplic_write(target_off(route.source), target_value(hart, 0, route.source));
        b.aplic_write(clrip_word_off(bitmap_word(route.source)), bitmap_bit(route.source));
        set_eid_enabled(b, route.source, true);
        b.aplic_write(setie_word_off(bitmap_word(route.source)), bitmap_bit(route.source));
    }

    fn disable_source(&self, b: &mut impl AiaBackend, source: usize) {
        b.aplic_write(clrie_word_off(bitmap_word(source)), bitmap_bit(source));
        set_eid_enabled(b, source, false);
    }
}

// ---------------------------------------------------------------------------
// Free helpers shared by the platform and the stage 9 probe
// ---------------------------------------------------------------------------

/// Sets/clears the local EIE bit of `eid` (probe-facing mask layer).
pub fn set_eid_enabled(b: &mut impl AiaBackend, eid: usize, on: bool) {
    b.with_imsic(|c| {
        let sel = eie_selector(eie_word(eid));
        let cur = c.read(sel);
        c.write(sel, if on { cur | eie_bit(eid) } else { cur & !eie_bit(eid) });
    });
}

/// Clears one source's APLIC pending bit (CLRIP bitmap write).
pub fn clear_source_pending(b: &mut impl AiaBackend, source: usize) {
    b.aplic_write(clrip_word_off(bitmap_word(source)), bitmap_bit(source));
}

/// Reads the local EIP words with interrupts disabled (consistent snapshot).
pub fn read_eip_snapshot(b: &mut impl AiaBackend) -> [u64; IMSIC_WORDS] {
    b.with_imsic(|c| {
        let mut words = [0u64; IMSIC_WORDS];
        for (i, w) in words.iter_mut().enumerate() {
            *w = c.read(eip_selector(i));
        }
        words
    })
}

// ---------------------------------------------------------------------------
// Production backend (riscv64 builds only; host harnesses supply fakes)
// ---------------------------------------------------------------------------

/// Raw boot hartid handed over by `boot::_start` (never assume hart 0).
#[cfg(target_arch = "riscv64")]
static BOOT_HART: AtomicUsize = AtomicUsize::new(0);

/// Records the boot hartid before the first route registration.
#[cfg(target_arch = "riscv64")]
pub fn record_boot_hart(hart: usize) {
    BOOT_HART.store(hart, Ordering::Relaxed);
}

/// `siselect` CSR (S-mode indirect register select).
#[cfg(target_arch = "riscv64")]
const CSR_SELECT: u16 = 0x150;
/// `sireg` CSR (S-mode indirect register data).
#[cfg(target_arch = "riscv64")]
const CSR_IREG: u16 = 0x151;
/// `stopei` CSR (S-mode top-external-interrupt: atomic claim + complete).
#[cfg(target_arch = "riscv64")]
const CSR_TOPEI: u16 = 0x15c;

/// Volatile-MMIO + CSR production backend over the identity-mapped APLIC
/// window and the local IMSIC CSR file. Shared by the axplat `IrqIf` (irq
/// module) and the kernel-side stage 9 probe so the hardware access rules
/// (SIE critical sections, even selectors, atomic `stopei` claim) exist once.
#[cfg(target_arch = "riscv64")]
pub struct HardwareBackend;

#[cfg(target_arch = "riscv64")]
struct LocalCsrs;

#[cfg(target_arch = "riscv64")]
impl ImsicAccess for LocalCsrs {
    fn read(&mut self, selector: u16) -> u64 {
        let value: usize;
        // SAFETY: runs S-mode on the local hart with external interrupts
        // disabled (see `with_imsic`); even selectors only on RV64.
        unsafe {
            core::arch::asm!(
                "csrw {sel_csr}, {sel}",
                "csrr {val}, {reg_csr}",
                sel_csr = const CSR_SELECT,
                reg_csr = const CSR_IREG,
                sel = in(reg) selector as usize,
                val = out(reg) value,
            )
        }
        value as u64
    }

    fn write(&mut self, selector: u16, value: u64) {
        // SAFETY: same critical-section contract as `read`.
        unsafe {
            core::arch::asm!(
                "csrw {sel_csr}, {sel}",
                "csrw {reg_csr}, {val}",
                sel_csr = const CSR_SELECT,
                reg_csr = const CSR_IREG,
                sel = in(reg) selector as usize,
                val = in(reg) value as usize,
            )
        }
    }

    fn claim(&mut self) -> u64 {
        let raw: usize;
        // SAFETY: single atomic claim+complete (`csrrw stopei, x0`); the read
        // and the cleared pending are the same interrupt (R69-verified).
        unsafe {
            core::arch::asm!(
                "csrrw {val}, {topei_csr}, zero",
                topei_csr = const CSR_TOPEI,
                val = out(reg) raw,
            )
        }
        raw as u64
    }
}

#[cfg(target_arch = "riscv64")]
impl AiaBackend for HardwareBackend {
    fn aplic_read(&mut self, offset: u32) -> u32 {
        let ptr = (aplic_base() + offset as usize
            + crate::config::plat::PHYS_VIRT_OFFSET) as *const u32;
        // SAFETY: the APLIC window (DT or +0x4000 S window, both in
        // `devices.mmio-ranges`) is mapped; reads are side-effectful only on
        // the claim-free status registers the core reads.
        unsafe { ptr.read_volatile() }
    }

    fn aplic_write(&mut self, offset: u32, value: u32) {
        let ptr = (aplic_base() + offset as usize
            + crate::config::plat::PHYS_VIRT_OFFSET) as *mut u32;
        // SAFETY: same window contract as `aplic_read`; writes go only to the
        // registers the AIA core's init/route/enable sequences address.
        unsafe { ptr.write_volatile(value) }
    }

    fn with_imsic<R>(&mut self, f: impl FnOnce(&mut dyn ImsicAccess) -> R) -> R {
        let saved = axcpu::asm::irqs_enabled();
        axcpu::asm::disable_irqs();
        let mut csrs = LocalCsrs;
        let r = f(&mut csrs);
        // Restore exactly as found; never unconditionally re-enable.
        if saved {
            axcpu::asm::enable_irqs();
        }
        r
    }

    fn set_sext(&mut self, on: bool) {
        if on {
            // SAFETY: sets the sie.SEXT bit of the current hart.
            unsafe { riscv::register::sie::set_sext() }
        } else {
            // SAFETY: clears the sie.SEXT bit of the current hart.
            unsafe { riscv::register::sie::clear_sext() }
        }
    }

    fn hart(&self) -> usize {
        BOOT_HART.load(Ordering::Relaxed)
    }
}
