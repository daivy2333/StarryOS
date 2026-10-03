//! K3 stage 10 PHY device IRQ/status probe pure-logic host test harness
//! (MS10 Iteration 001, tasks 2.1–2.2).
//!
//! Uses `#[path]` to include the real `kernel/src/platform/{mac_probe,
//! phy_probe,aia_probe,k3_device_irq_probe}.rs` and the production AIA core.
//! `BoardModel` (+ the `BOARD` static) is a deterministic fake board: a
//! page-aware MDIO engine with a COR INSR, the pin-35 pad edge latch, the
//! four read-only `syscon-gpio-edge` words and a scripted device-event
//! schedule. The stage environment layers an APLIC/IMSIC fake (same shape as
//! `k3-aia-probe-host-harness.rs`) on top, so tests drive the actual handler
//! dispatch path — never a copy. Runs via `rustc --edition=2024 --test`.
//!
//! Fake timing rules (same convention as `phy-probe-host-harness.rs`):
//! - `wait_nanos` advances the fake clock by exactly the requested nanos.
//! - Each MMIO access advances the clock by `access_cost_ns` (default 0;
//!   MDIO-timeout tests raise it to 1 ms so the 10 ms deadline expires).
//! - `pump()` services at most one arrived interrupt (one trap) and models
//!   the LevelHigh pinctrl source: while the pad latch is set and source 60
//!   is enabled, the MSI pending is (re)raised.
#![allow(dead_code)]

extern crate core;

#[path = "../kernel/src/platform/mac_probe.rs"]
mod mac_probe;
#[path = "../kernel/src/platform/phy_probe.rs"]
mod phy_probe;
#[path = "../crates/axplat-riscv64-k3/src/aia.rs"]
mod k3_aia;
#[path = "../kernel/src/platform/aia_probe.rs"]
mod aia_probe;
#[path = "../kernel/src/platform/k3_device_irq_probe.rs"]
mod k3_device_irq_probe;

use phy_probe::{ProbeEnv, GOC_READ, GOC_WRITE, MDIO_ADDR_OFFSET, MDIO_CR, MDIO_DATA_OFFSET, PHY_ADDR};
use k3_device_irq_probe::HandlerAccess;
use k3_aia::{AiaBackend, AiaChip, HandlerStore, ImsicAccess, APLIC_DOMAINCFG, APLIC_DOMAINCFG_DM, APLIC_DOMAINCFG_IE};
use k3_aia as aia;
use std::sync::atomic::{AtomicU64, Ordering};

const MAC_MDIO_ADDR: u64 = mac_probe::MAC_BASE_PADDR + MDIO_ADDR_OFFSET;
const MAC_MDIO_DATA: u64 = mac_probe::MAC_BASE_PADDR + MDIO_DATA_OFFSET;
/// `syscon-gpio-edge` sub-window (board DT `reg = <0xd4019800 0x10>`).
const EDGE_BASE: u64 = phy_probe::GPIO_EDGE_BASE_PADDR;
/// End of the sub-window (pattern bound; const expressions only).
const EDGE_END: u64 = EDGE_BASE + 4 * phy_probe::GPIO_EDGE_WORDS as u64;
/// Pin 35 sits in edge word 1 (35/32), bit 3 (35%32).
const PIN35_WORD: usize = 1;
const PIN35_BIT: u32 = 1 << 3;

/// A scripted device event applied by `pump()` when the fake clock passes it.
#[derive(Clone, Copy, Debug)]
enum ScriptEvent {
    /// Peer link transition; with_insr=false models an edge without the PHY
    /// cause bit (misalignment witness).
    LinkChange { up: bool, with_insr: bool },
    /// Pad edge only (no INSR bit) — wrong-source/misalignment witness.
    EdgeOnly,
}

struct BoardModel {
    now_ns: u64,
    access_cost_ns: u64,
    // MDIO engine (DWMAC4 C22): one pending transaction at a time.
    mdio_pending: Option<(u32, u32, u64)>, // (reg, goc, ready_at)
    mdio_data: u32,
    mdio_busy_stuck: bool,
    /// Trigger indices that hang with the busy bit permanently set.
    hang_txns: std::collections::BTreeSet<usize>,
    txn: usize,
    // PHY (RTL8211F at address 1)
    page: u16,
    std_regs: [u16; 32],
    vendor: std::collections::BTreeMap<u16, [u16; 32]>,
    /// Current INSR value; a read clears it (clear-on-read).
    insr: u16,
    link: bool,
    // Pad 35 / edge latch
    mfpr35: u32,
    latch: bool,
    /// Falling-edge detect armed + GPIO mux (set by the pad35 setup write).
    fall_armed: bool,
    /// Persistent-level model: the pad re-latches immediately after every
    /// ack (a stuck line that never releases the LevelHigh source).
    persistent: bool,
    edge: [u32; 4],
    // Scripted device events, consumed in pump.
    script: Vec<(u64, ScriptEvent)>,
    // Full MMIO write log ((paddr, value)).
    writes: Vec<(u64, u32)>,
}

impl BoardModel {
    fn lock() -> std::sync::MutexGuard<'static, Self> {
        static BOARD: std::sync::Mutex<BoardModel> = std::sync::Mutex::new(BoardModel {
            now_ns: 0,
            access_cost_ns: 0,
            mdio_pending: None,
            mdio_data: 0,
            mdio_busy_stuck: false,
            hang_txns: std::collections::BTreeSet::new(),
            txn: 0,
            page: 0,
            std_regs: [0; 32],
            vendor: std::collections::BTreeMap::new(),
            insr: 0,
            link: true,
            mfpr35: 0,
            latch: false,
            fall_armed: false,
            persistent: false,
            edge: [0; 4],
            script: Vec::new(),
            writes: Vec::new(),
        });
        BOARD.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Post-stage6/7/8 state: PHY up, page 0, idle MDIO.
    fn reset(&mut self) {
        let cost = self.access_cost_ns;
        *self = BoardModel {
            access_cost_ns: cost,
            now_ns: 0,
            mdio_pending: None,
            mdio_data: 0,
            mdio_busy_stuck: false,
            hang_txns: std::collections::BTreeSet::new(),
            txn: 0,
            page: 0,
            std_regs: [0; 32],
            vendor: std::collections::BTreeMap::new(),
            insr: 0,
            link: true,
            mfpr35: 0,
            latch: false,
            fall_armed: false,
            persistent: false,
            edge: [0; 4],
            script: Vec::new(),
            writes: Vec::new(),
        };
        // BMSR: autoneg-capable + complete; link bit per current state.
        self.std_regs[1] = 0x0820 | phy_probe::BMSR_LINK;
    }

    fn serve_phy_read(&mut self, reg: u32) -> u16 {
        if reg == phy_probe::PHY_PAGE_SEL_REG {
            return self.page;
        }
        if self.page == 0 {
            if reg == phy_probe::BMSR_REG {
                // Latch-low is modeled flat: both reads return the level.
                return 0x0820 | if self.link { phy_probe::BMSR_LINK } else { 0 };
            }
            return self.std_regs[reg as usize];
        }
        if self.page == phy_probe::PHY_PAGE_INSR && reg == phy_probe::PHY_INSR_REG {
            let v = self.insr;
            self.insr = 0; // one clear-on-read
            return v;
        }
        self.vendor
            .get(&self.page)
            .map(|r| r[reg as usize])
            .unwrap_or(0)
    }

    fn serve_phy_write(&mut self, reg: u32, value: u16) {
        if reg == phy_probe::PHY_PAGE_SEL_REG {
            self.page = value;
            return;
        }
        if self.page == 0 {
            self.std_regs[reg as usize] = value;
        } else {
            self.vendor.entry(self.page).or_insert([0; 32])[reg as usize] = value;
        }
    }

    /// Applies one scripted event: link state, PHY cause, pad latch, edge bit.
    fn apply_event(&mut self, ev: ScriptEvent) {
        match ev {
            ScriptEvent::LinkChange { up, with_insr } => {
                self.link = up;
                if with_insr {
                    self.insr |= phy_probe::PHY_INSR_LINK_CHANGE;
                }
                // INTB routing: only a GPIO-muxed pad with the falling-edge
                // enable latches (board run 6 falsification witness).
                if self.fall_armed {
                    self.latch = true;
                    self.edge[PIN35_WORD] |= PIN35_BIT;
                }
            }
            ScriptEvent::EdgeOnly => {
                if self.fall_armed {
                    self.latch = true;
                    self.edge[PIN35_WORD] |= PIN35_BIT;
                }
            }
        }
    }
}

impl ProbeEnv for BoardModel {
    fn mmio_read(&mut self, paddr: u64) -> u32 {
        self.now_ns += self.access_cost_ns;
        match paddr {
            MAC_MDIO_ADDR => {
                if self.mdio_busy_stuck {
                    return 1;
                }
                if let Some((reg, goc, ready)) = self.mdio_pending {
                    if self.now_ns >= ready {
                        if goc == GOC_READ >> 2 {
                            self.mdio_data = self.serve_phy_read(reg) as u32;
                        } else {
                            self.serve_phy_write(reg, self.mdio_data as u16);
                        }
                        self.mdio_pending = None;
                    }
                }
                if self.mdio_pending.is_some() {
                    1
                } else {
                    0
                }
            }
            MAC_MDIO_DATA => self.mdio_data,
            EDGE_BASE..EDGE_END => {
                let w = ((paddr - EDGE_BASE) / 4) as usize;
                self.edge[w]
            }
            phy_probe::PAD35_MFPR_PADDR => self.mfpr35,
            _ => 0,
        }
    }

    fn mmio_write(&mut self, paddr: u64, value: u32) {
        self.now_ns += self.access_cost_ns;
        self.writes.push((paddr, value));
        match paddr {
            MAC_MDIO_DATA => self.mdio_data = value,
            MAC_MDIO_ADDR => {
                if value & 1 == 0 {
                    return; // not a trigger
                }
                let goc = (value >> 2) & 3;
                let reg = (value >> 16) & 0x1f;
                let phy = (value >> 21) & 0x1f;
                assert_eq!(phy, PHY_ADDR, "probe must address PHY 1 only");
                assert_eq!((value >> 8) & 0xf, MDIO_CR, "probe must use CR=5");
                assert_eq!(value & 0b10, 0, "C45E must stay clear");
                assert!(goc == GOC_READ >> 2 || goc == GOC_WRITE >> 2, "bad GOC");
                let txn = self.txn;
                self.txn += 1;
                if self.hang_txns.contains(&txn) {
                    self.mdio_busy_stuck = true;
                    self.mdio_pending = None;
                    return;
                }
                self.mdio_pending = Some((reg, goc, self.now_ns));
            }
            phy_probe::PAD35_MFPR_PADDR => {
                self.mfpr35 = value;
                // GPIO mux + falling-edge enable + CLEAR hold released = the
                // armed state (reset default asserts CLEAR=1; runs 1–2).
                self.fall_armed = value & phy_probe::PAD_MUX_MASK == phy_probe::PAD_MUX_GPIO
                    && value & phy_probe::PAD_EDGE_FALL != 0
                    && value & phy_probe::PAD_EDGE_CLEAR == 0;
                if value & phy_probe::PAD_EDGE_CLEAR != 0 {
                    // The pad CLEAR wipes the edge latch and the pin-35 edge
                    // word bit; a persistent line re-latches on the next pump.
                    self.latch = false;
                    self.edge[PIN35_WORD] &= !PIN35_BIT;
                }
            }
            _ => {}
        }
    }

    fn now_nanos(&self) -> u64 {
        self.now_ns
    }

    fn wait_nanos(&mut self, nanos: u64) {
        self.now_ns += nanos;
    }
}

// ---------------------------------------------------------------------------
// Handler access fns over the BOARD static (the armed production path)
// ---------------------------------------------------------------------------

fn fake_read_edge_words() -> [u32; phy_probe::GPIO_EDGE_WORDS] {
    let mut b = BoardModel::lock();
    phy_probe::read_edge_words(&mut *b)
}

fn fake_read_mfpr35() -> u32 {
    let mut b = BoardModel::lock();
    b.mmio_read(phy_probe::PAD35_MFPR_PADDR)
}

fn fake_write_mfpr35(value: u32) -> u32 {
    let mut b = BoardModel::lock();
    b.mmio_write(phy_probe::PAD35_MFPR_PADDR, value);
    b.mfpr35
}

fn fake_read_insr_snapshot() -> Result<u16, phy_probe::PagedError> {
    let mut b = BoardModel::lock();
    phy_probe::paged_read(&mut *b, phy_probe::PHY_PAGE_INSR, phy_probe::PHY_INSR_REG)
}

fn fake_read_bmsr_twice() -> Result<(u16, u16), phy_probe::MdioError> {
    let mut b = BoardModel::lock();
    let a = phy_probe::mdio_read(&mut *b, PHY_ADDR, phy_probe::BMSR_REG)?;
    let c = phy_probe::mdio_read(&mut *b, PHY_ADDR, phy_probe::BMSR_REG)?;
    Ok((a, c))
}

/// Handler-side mask request; consumed by the fake environment's pump (the
/// production board performs the APLIC CLRIE + local EIE clear directly).
fn fake_mask_source(source: usize) {
    MASK_REQUESTED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(source);
}

fn take_mask_requested(source: usize) -> bool {
    MASK_REQUESTED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&source)
}

static MASK_REQUESTED: std::sync::Mutex<std::collections::BTreeSet<usize>> =
    std::sync::Mutex::new(std::collections::BTreeSet::new());

static FAKE_ACCESS: HandlerAccess = HandlerAccess {
    read_edge_words: fake_read_edge_words,
    read_mfpr35: fake_read_mfpr35,
    write_mfpr35: fake_write_mfpr35,
    read_insr_snapshot: fake_read_insr_snapshot,
    read_bmsr_twice: fake_read_bmsr_twice,
    mask_source: fake_mask_source,
};

// ---------------------------------------------------------------------------
// Tests (2.1 witness list)
// ---------------------------------------------------------------------------

/// Serializes tests: they share the BOARD static and the fake statics.
fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn paged_access_restores_original_page() {
    let _guard = test_guard();
    {
        let mut b = BoardModel::lock();
        b.reset();
        b.page = 0x0b;
        b.vendor.insert(phy_probe::PHY_PAGE_INER, [0; 32]);
    }
    // Read through a vendor page, then confirm the selector came back.
    let v = {
        let mut b = BoardModel::lock();
        phy_probe::paged_read(&mut *b, phy_probe::PHY_PAGE_INER, phy_probe::PHY_INER_REG)
    };
    assert!(v.is_ok(), "paged read failed: {v:?}");
    {
        let mut b = BoardModel::lock();
        let sel = phy_probe::mdio_read(&mut *b, PHY_ADDR, phy_probe::PHY_PAGE_SEL_REG).unwrap();
        assert_eq!(sel, 0x0b, "original page must be restored, got {sel:#x}");
        // And a paged write restores too (INER is a plain RW vendor register;
        // INSR is COR and never used as a write vehicle).
        b.vendor.insert(phy_probe::PHY_PAGE_INER, [0; 32]);
        let w = phy_probe::paged_write(
            &mut *b,
            phy_probe::PHY_PAGE_INER,
            phy_probe::PHY_INER_REG,
            0x1234,
        );
        assert!(w.is_ok(), "paged write failed: {w:?}");
        let sel = phy_probe::mdio_read(&mut *b, PHY_ADDR, phy_probe::PHY_PAGE_SEL_REG).unwrap();
        assert_eq!(sel, 0x0b, "page must be restored after a paged write");
        // The write really landed in the vendor page, not page 0.
        b.page = phy_probe::PHY_PAGE_INER;
        assert_eq!(b.serve_phy_read(phy_probe::PHY_INER_REG), 0x1234);
    }
}

#[test]
fn insr_snapshot_survives_clear_on_read() {
    let _guard = test_guard();
    {
        let mut b = BoardModel::lock();
        b.reset();
        b.insr = phy_probe::PHY_INSR_LINK_CHANGE;
        b.latch = true;
        b.edge[PIN35_WORD] |= PIN35_BIT;
    }
    let first = k3_device_irq_probe::service_one_device_event(&FAKE_ACCESS);
    assert_eq!(first.insr, phy_probe::PHY_INSR_LINK_CHANGE);
    assert!(first.link_change_cause(), "snapshot must retain the cause");
    // The COR wiped the register: a second service sees no cause, but the
    // first snapshot object is untouched.
    let second = k3_device_irq_probe::service_one_device_event(&FAKE_ACCESS);
    assert_eq!(second.insr, 0, "second read must see the cleared register");
    assert!(!second.link_change_cause());
    assert!(first.link_change_cause(), "first snapshot survived the COR");
}

#[test]
fn all_raw_cause_bits_are_retained() {
    let _guard = test_guard();
    const EDGE: [u32; 4] = [0xdead_beef, 0x1234_5678, 0x9000_0000, 0x0000_000f];
    {
        let mut b = BoardModel::lock();
        b.reset();
        b.edge = EDGE;
        b.insr = 0x8fff; // link bit plus every other vendor cause bit
        b.latch = true;
    }
    let s = k3_device_irq_probe::service_one_device_event(&FAKE_ACCESS);
    assert_eq!(s.edge, EDGE, "all four edge words must be retained raw");
    assert_eq!(s.insr, 0x8fff, "all INSR cause bits must be retained");
}

#[test]
fn phy_cause_precedes_pad_clear_and_rearm() {
    let _guard = test_guard();
    {
        let mut b = BoardModel::lock();
        b.reset();
        b.insr = phy_probe::PHY_INSR_LINK_CHANGE;
        b.latch = true;
    }
    static OP_LOG: std::sync::Mutex<Vec<&'static str>> = std::sync::Mutex::new(Vec::new());
    fn log(name: &'static str) {
        OP_LOG.lock().unwrap_or_else(|e| e.into_inner()).push(name);
    }
    let recording = HandlerAccess {
        read_edge_words: || {
            log("edge");
            fake_read_edge_words()
        },
        read_mfpr35: || {
            log("mfpr-read");
            fake_read_mfpr35()
        },
        write_mfpr35: |v| {
            log("mfpr-write");
            fake_write_mfpr35(v)
        },
        read_insr_snapshot: || {
            log("insr");
            fake_read_insr_snapshot()
        },
        read_bmsr_twice: || {
            log("bmsr");
            fake_read_bmsr_twice()
        },
        mask_source: fake_mask_source,
    };
    k3_device_irq_probe::service_one_device_event(&recording);
    let ops = OP_LOG.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let pos = |name: &str| ops.iter().position(|o| *o == name).unwrap();
    assert_eq!(ops.first(), Some(&"edge"), "edge snapshot comes first: {ops:?}");
    assert!(
        pos("insr") < pos("mfpr-write"),
        "PHY cause must be snapshotted before the pad ack: {ops:?}"
    );
    assert!(
        pos("bmsr") < pos("mfpr-write"),
        "link reads precede the pad ack: {ops:?}"
    );
    // Exactly one set write and one re-arm write.
    assert_eq!(ops.iter().filter(|o| **o == "mfpr-write").count(), 2, "{ops:?}");
}

#[test]
fn pad_ack_preserves_mux_and_electrical_bits() {
    let _guard = test_guard();
    // mux field 0b101, edge detect armed (RISE|FALL), electrical bits above
    // the field — CLEAR (bit6) deliberately unset before the ack.
    const BEFORE: u32 = 0x0000_8335;
    {
        let mut b = BoardModel::lock();
        b.reset();
        b.mfpr35 = BEFORE;
        b.latch = true;
    }
    let ev = {
        let mut b = BoardModel::lock();
        phy_probe::pad35_edge_ack(&mut *b)
    };
    assert_eq!(
        ev.set_readback,
        BEFORE | phy_probe::PAD_EDGE_CLEAR,
        "the set write may only add CLEAR"
    );
    assert_eq!(ev.rearm_readback, BEFORE, "re-arm restores the exact value");
    {
        let b = BoardModel::lock();
        assert_eq!(b.mfpr35, BEFORE, "final register equals the pre-ack value");
        let mfpr_writes: Vec<u32> = b
            .writes
            .iter()
            .filter(|(p, _)| *p == phy_probe::PAD35_MFPR_PADDR)
            .map(|(_, v)| *v)
            .collect();
        assert_eq!(
            mfpr_writes,
            vec![BEFORE | phy_probe::PAD_EDGE_CLEAR, BEFORE & !phy_probe::PAD_EDGE_CLEAR],
            "ack writes are before|CLEAR then before&!CLEAR"
        );
    }
}

#[test]
fn mdio_error_reports_primary_and_restore_failure() {
    let _guard = test_guard();
    {
        let mut b = BoardModel::lock();
        b.reset();
        b.access_cost_ns = 1_000_000; // so the 10 ms deadline expires in ~10 polls
                                      // Txns: 0 selector read, 1 page write, 2 core read (hangs),
                                      // then the restore write can never go idle.
        b.hang_txns.insert(2);
    }
    let err = {
        let mut b = BoardModel::lock();
        phy_probe::paged_read(&mut *b, phy_probe::PHY_PAGE_INER, phy_probe::PHY_INER_REG)
    }
    .unwrap_err();
    match err {
        phy_probe::PagedError::Primary { primary, restore } => {
            assert!(
                matches!(primary, phy_probe::MdioError::CompletionTimeout { reg, .. } if reg == phy_probe::PHY_INER_REG),
                "primary must be the core read failure: {primary:?}"
            );
            assert!(
                matches!(restore, Some(phy_probe::MdioError::IdleTimeout { reg, .. }) if reg == phy_probe::PHY_PAGE_SEL_REG),
                "restore failure must be reported separately: {restore:?}"
            );
        }
        other => panic!("expected Primary with restore, got {other:?}"),
    }
}

#[test]
fn edge_window_is_read_only() {
    let _guard = test_guard();
    {
        let mut b = BoardModel::lock();
        b.reset();
        b.edge = [0x1, 0x2, 0x3, 0x4];
        b.insr = phy_probe::PHY_INSR_LINK_CHANGE;
        b.latch = true;
    }
    {
        let mut b = BoardModel::lock();
        let words = phy_probe::read_edge_words(&mut *b);
        assert_eq!(words, [0x1, 0x2, 0x3, 0x4]);
        assert!(
            b.writes.is_empty(),
            "a pure edge read must not write anything"
        );
    }
    // Full service path: the only writes belong to the MDIO engine and the
    // pad ack, never the edge sub-window.
    k3_device_irq_probe::service_one_device_event(&FAKE_ACCESS);
    let b = BoardModel::lock();
    let edge_writes = b
        .writes
        .iter()
        .filter(|(p, _)| (EDGE_BASE..EDGE_BASE + 0x10).contains(p))
        .count();
    assert_eq!(edge_writes, 0, "the edge syscon window must never be written");
}

// ---------------------------------------------------------------------------
// Fake AIA environment + stage 10 tests (2.2 witness list)
// ---------------------------------------------------------------------------

const IMSIC_WORDS: usize = 8;

static EIP: [AtomicU64; IMSIC_WORDS] = [const { AtomicU64::new(0) }; IMSIC_WORDS];
static EIE: [AtomicU64; IMSIC_WORDS] = [const { AtomicU64::new(0) }; IMSIC_WORDS];
static EIDELIVERY: AtomicU64 = AtomicU64::new(1);
static EITHRESHOLD: AtomicU64 = AtomicU64::new(0);

fn reset_fake_imsic() {
    for w in 0..IMSIC_WORDS {
        EIP[w].store(0, Ordering::SeqCst);
        EIE[w].store(0, Ordering::SeqCst);
    }
    EIDELIVERY.store(1, Ordering::SeqCst);
    EITHRESHOLD.store(0, Ordering::SeqCst);
}

fn pending_set(eid: usize) -> bool {
    EIP[aia::eie_word(eid)].load(Ordering::SeqCst) & aia::eie_bit(eid) != 0
}

fn set_pending(eid: usize) {
    EIP[aia::eie_word(eid)].fetch_or(aia::eie_bit(eid), Ordering::SeqCst);
}

fn enabled_bit(eid: usize) -> bool {
    EIE[aia::eie_word(eid)].load(Ordering::SeqCst) & aia::eie_bit(eid) != 0
}

fn any_pending_enabled() -> bool {
    (1..512).any(|eid| pending_set(eid) && enabled_bit(eid))
}

/// Models the production lock-free handler table (same as the stage 9 fake).
struct FakeStore {
    entries: [std::sync::atomic::AtomicUsize; 512],
}

impl FakeStore {
    const fn new() -> Self {
        FakeStore {
            entries: [const { std::sync::atomic::AtomicUsize::new(0) }; 512],
        }
    }
}

static STORE: FakeStore = FakeStore::new();

impl HandlerStore for FakeStore {
    fn register(&self, source: usize, handler: fn()) -> bool {
        if source == 0 || source >= 512 {
            return false;
        }
        self.entries[source]
            .compare_exchange(0, handler as usize, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    }

    fn unregister(&self, source: usize) -> Option<fn()> {
        if source == 0 || source >= 512 {
            return None;
        }
        let h = self.entries[source].swap(0, Ordering::AcqRel);
        if h != 0 {
            Some(unsafe { core::mem::transmute::<usize, fn()>(h) })
        } else {
            None
        }
    }

    fn call(&self, eid: usize) -> bool {
        if eid == 0 || eid >= 512 {
            return false;
        }
        let h = self.entries[eid].load(Ordering::Acquire);
        if h != 0 {
            let f: fn() = unsafe { core::mem::transmute(h) };
            f();
            true
        } else {
            false
        }
    }
}

static CHIP: AiaChip<FakeStore> = AiaChip::new(&STORE);

struct ImsicFile;

impl ImsicAccess for ImsicFile {
    fn read(&mut self, selector: u16) -> u64 {
        match selector {
            0x70 => EIDELIVERY.load(Ordering::SeqCst),
            0x72 => EITHRESHOLD.load(Ordering::SeqCst),
            s if (0xc0..0xc0 + 2 * IMSIC_WORDS as u16).contains(&s) && s % 2 == 0 => {
                EIE[((s - 0xc0) / 2) as usize].load(Ordering::SeqCst)
            }
            s if (0x80..0x80 + 2 * IMSIC_WORDS as u16).contains(&s) && s % 2 == 0 => {
                EIP[((s - 0x80) / 2) as usize].load(Ordering::SeqCst)
            }
            _ => 0,
        }
    }

    fn write(&mut self, selector: u16, value: u64) {
        match selector {
            0x70 => EIDELIVERY.store(value, Ordering::SeqCst),
            0x72 => EITHRESHOLD.store(value, Ordering::SeqCst),
            s if (0xc0..0xc0 + 2 * IMSIC_WORDS as u16).contains(&s) && s % 2 == 0 => {
                EIE[((s - 0xc0) / 2) as usize].store(value, Ordering::SeqCst)
            }
            _ => {}
        }
    }

    fn claim(&mut self) -> u64 {
        for eid in 1..512 {
            if pending_set(eid) && enabled_bit(eid) {
                EIP[aia::eie_word(eid)].fetch_and(!aia::eie_bit(eid), Ordering::SeqCst);
                return ((eid as u64) << 16) | eid as u64;
            }
        }
        0
    }
}

/// Stage 10 environment: APLIC bank + trap pump over the BOARD model.
struct FakeStage10 {
    aplic: std::collections::BTreeMap<u32, u32>,
    /// Full APLIC write log ((offset, value)) for read-only witnesses.
    aplic_writes: Vec<(u32, u32)>,
    sext: bool,
    sie: bool,
    in_critical: bool,
}

impl FakeStage10 {
    fn new() -> Self {
        reset_fake_imsic();
        for entry in STORE.entries.iter() {
            entry.store(0, Ordering::SeqCst);
        }
        MASK_REQUESTED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        let mut env = FakeStage10 {
            aplic: std::collections::BTreeMap::new(),
            aplic_writes: Vec::new(),
            sext: true,
            sie: true,
            in_critical: false,
        };
        // Post-init domain state: MSI mode, domain IE on.
        env.aplic.insert(APLIC_DOMAINCFG, APLIC_DOMAINCFG_IE | APLIC_DOMAINCFG_DM);
        env
    }

    fn domain_ie(&self) -> bool {
        self.aplic.get(&APLIC_DOMAINCFG).copied().unwrap_or(0) & APLIC_DOMAINCFG_IE != 0
    }

    fn source_enabled(&self, source: usize) -> bool {
        let word = self
            .aplic
            .get(&aia::setie_word_off(source / 32))
            .copied()
            .unwrap_or(0);
        word & (1 << (source % 32)) != 0
    }
}

impl AiaBackend for FakeStage10 {
    fn aplic_read(&mut self, offset: u32) -> u32 {
        self.aplic.get(&offset).copied().unwrap_or(0)
    }

    fn aplic_write(&mut self, offset: u32, value: u32) {
        self.aplic_writes.push((offset, value));
        // APLIC bitmap semantics: write-1-to-set / write-1-to-clear, with
        // the set and clear counterparts of the same word paired.
        const SETIP: u32 = aia::APLIC_SETIP_BASE;
        const SETIE: u32 = aia::APLIC_SETIE_BASE;
        const CLRIP: u32 = aia::APLIC_CLRIP_BASE;
        const CLRIE: u32 = aia::APLIC_CLRIE_BASE;
        const SPAN: u32 = 4 * 17; // words 0..=16 (source 512)
        // A clear-register write acts on its set-register counterpart word.
        if (CLRIE..CLRIE + SPAN).contains(&offset) {
            let peer = SETIE + (offset - CLRIE);
            let cur = self.aplic.get(&peer).copied().unwrap_or(0);
            self.aplic.insert(peer, cur & !value);
            return;
        }
        if (CLRIP..CLRIP + SPAN).contains(&offset) {
            let peer = SETIP + (offset - CLRIP);
            let cur = self.aplic.get(&peer).copied().unwrap_or(0);
            self.aplic.insert(peer, cur & !value);
            return;
        }
        if (SETIE..SETIE + SPAN).contains(&offset) {
            let cur = self.aplic.get(&offset).copied().unwrap_or(0);
            self.aplic.insert(offset, cur | value);
            return;
        }
        if (SETIP..SETIP + SPAN).contains(&offset) {
            let cur = self.aplic.get(&offset).copied().unwrap_or(0);
            self.aplic.insert(offset, cur | value);
            return;
        }
        self.aplic.insert(offset, value);
    }

    fn with_imsic<R>(&mut self, f: impl FnOnce(&mut dyn ImsicAccess) -> R) -> R {
        assert!(!self.in_critical, "nested IMSIC critical section");
        self.in_critical = true;
        let saved = self.sie;
        self.sie = false;
        let r = f(&mut ImsicFile);
        if saved {
            self.sie = true;
        }
        self.in_critical = false;
        r
    }

    fn set_sext(&mut self, on: bool) {
        self.sext = on;
    }

    fn hart(&self) -> usize {
        3
    }
}

impl ProbeEnv for FakeStage10 {
    fn mmio_read(&mut self, paddr: u64) -> u32 {
        let mut b = BoardModel::lock();
        ProbeEnv::mmio_read(&mut *b, paddr)
    }

    fn mmio_write(&mut self, paddr: u64, value: u32) {
        let mut b = BoardModel::lock();
        ProbeEnv::mmio_write(&mut *b, paddr, value)
    }

    fn now_nanos(&self) -> u64 {
        BoardModel::lock().now_ns
    }

    fn wait_nanos(&mut self, nanos: u64) {
        BoardModel::lock().now_ns += nanos;
    }
}

impl k3_device_irq_probe::Stage10Env for FakeStage10 {
    fn pump(&mut self) {
        // 1. Apply due scripted device events / persistent re-latch.
        {
            let mut b = BoardModel::lock();
            let now = b.now_ns;
            while let Some(&(at, ev)) = b.script.first() {
                if at > now {
                    break;
                }
                b.script.remove(0);
                b.apply_event(ev);
            }
            if b.persistent && !b.latch {
                b.apply_event(ScriptEvent::LinkChange { up: false, with_insr: true });
            }
        }
        // 2. Consume handler-side mask requests (armed fn defers to the env).
        for source in [60usize, 133, 277] {
            if take_mask_requested(source) {
                CHIP.set_enable(self, source, false);
            }
        }
        // 3. LevelHigh pinctrl source model: while the pad latch is set and
        //    the route is open, the MSI pending is (re)raised.
        let latch = BoardModel::lock().latch;
        if latch
            && self.source_enabled(k3_device_irq_probe::STAGE10_SOURCE)
            && self.domain_ie()
            && enabled_bit(k3_device_irq_probe::STAGE10_SOURCE)
        {
            set_pending(k3_device_irq_probe::STAGE10_SOURCE);
            // The APLIC latches the input in its SETIP word (fake-hardware
            // side effect; direct map insert so it never counts as a
            // software write).
            let setip_off =
                aia::APLIC_SETIP_BASE + 4 * (k3_device_irq_probe::STAGE10_SOURCE / 32) as u32;
            let bit = 1u32 << (k3_device_irq_probe::STAGE10_SOURCE % 32);
            let w = self.aplic.get(&setip_off).copied().unwrap_or(0) | bit;
            self.aplic.insert(setip_off, w);
        }
        // 4. Service at most one trap.
        if self.sext && any_pending_enabled() {
            CHIP.handle_external(self);
        }
    }

    fn register_handler(&mut self, source: usize, handler: fn()) -> bool {
        CHIP.register(self, source, handler)
    }

    fn set_route_enabled(&mut self, source: usize, enabled: bool) {
        CHIP.set_enable(self, source, enabled)
    }

    fn observed(&self) -> aia_probe::Observed {
        aia_probe::Observed {
            last_raw_claim: CHIP.last_raw_claim(),
            spurious: CHIP.spurious_count(),
            unknown: CHIP.unknown_count(),
        }
    }

    fn handler_access(&self) -> &'static HandlerAccess {
        &FAKE_ACCESS
    }
}

/// Resets the shared fake board and returns a fresh stage environment.
fn fresh_env() -> FakeStage10 {
    BoardModel::lock().reset();
    FakeStage10::new()
}

fn run_stage(env: &mut FakeStage10) -> (k3_device_irq_probe::Stage10Outcome, Vec<String>) {
    let mut lines = Vec::new();
    let outcome = k3_device_irq_probe::run_stage10(env, |line| lines.push(line.to_string()));
    (outcome, lines)
}

fn assert_window_ended(
    outcome: k3_device_irq_probe::Stage10Outcome,
) -> k3_device_irq_probe::Stage10Summary {
    match outcome {
        k3_device_irq_probe::Stage10Outcome::WindowEnded(s) => s,
        other => panic!("expected WindowEnded, got {other:?}"),
    }
}

#[test]
fn link_change_matches_source60() {
    let _guard = test_guard();
    let mut env = fresh_env();
    {
        let mut b = BoardModel::lock();
        b.script = vec![
            (5_000_000_000, ScriptEvent::LinkChange { up: false, with_insr: true }),
            (30_000_000_000, ScriptEvent::LinkChange { up: true, with_insr: true }),
        ];
    }
    let (outcome, lines) = run_stage(&mut env);
    let s = assert_window_ended(outcome);
    assert_eq!(s.claims, 2, "one claim per link transition: {lines:?}");
    assert_eq!(s.aligned_down, 1, "unplug claim: {lines:?}");
    assert_eq!(s.aligned_up, 1, "replug claim: {lines:?}");
    assert_eq!(s.unaligned, 0);
    // The snapshots came through the real dispatch path and carry the raw
    // evidence: INSR bit4, the pre-ack pad state and the pin-35 edge word.
    let first = k3_device_irq_probe::probe_snapshot(0).expect("snapshot 0");
    assert!(first.link_change_cause());
    assert!(!first.current_link_up(), "unplug snapshot must show link down");
    assert_ne!(first.edge[PIN35_WORD] & PIN35_BIT, 0, "edge word retained");
    let second = k3_device_irq_probe::probe_snapshot(1).expect("snapshot 1");
    assert!(second.link_change_cause());
    assert!(second.current_link_up(), "replug snapshot must show link up");
    // Both claim lines name source 60 and the aligned judgment.
    let claim_lines: Vec<&String> = lines.iter().filter(|l| l.contains("claim idx=")).collect();
    assert_eq!(claim_lines.len(), 2, "{lines:?}");
    for l in &claim_lines {
        assert!(l.contains("source=60"), "{l}");
        assert!(l.contains("aligned=1"), "{l}");
    }
}

#[test]
fn wrong_source_or_missing_insr_link_bit_fails() {
    let _guard = test_guard();
    let mut env = fresh_env();
    {
        let mut b = BoardModel::lock();
        // A pad edge without the PHY cause bit: claim must be reported
        // unaligned, never counted as link-change evidence.
        b.script = vec![(5_000_000_000, ScriptEvent::EdgeOnly)];
    }
    let (outcome, lines) = run_stage(&mut env);
    let s = assert_window_ended(outcome);
    assert_eq!(s.claims, 1, "{lines:?}");
    assert_eq!(s.unaligned, 1, "edge-only claim must be unaligned: {lines:?}");
    assert_eq!(s.aligned_down + s.aligned_up, 0);
    assert!(
        lines.iter().any(|l| l.contains("aligned=0")),
        "the unaligned claim must be printed as such: {lines:?}"
    );
}

#[test]
fn cause_snapshot_is_not_overwritten() {
    let _guard = test_guard();
    let mut env = fresh_env();
    {
        let mut b = BoardModel::lock();
        // 33 distinct events at 20 ms spacing: cap is 32, so the 33rd claim
        // must overflow (mask + stop) without touching any stored slot.
        let mut script = Vec::new();
        for i in 0..33u64 {
            script.push((
                1_000_000_000 + i * 20_000_000,
                ScriptEvent::LinkChange { up: i % 2 == 0, with_insr: true },
            ));
        }
        b.script = script;
    }
    let (outcome, lines) = run_stage(&mut env);
    match outcome {
        k3_device_irq_probe::Stage10Outcome::Stopped { reason } => {
            assert_eq!(reason, "snapshot-capacity-exceeded", "{lines:?}");
        }
        other => panic!("expected Stopped, got {other:?}"),
    }
    assert_eq!(
        k3_device_irq_probe::probe_snapshot_count(),
        32,
        "exactly the cap is stored: {lines:?}"
    );
    // Every stored snapshot keeps its own raw insr value (1<<bit per event).
    for i in 0..32usize {
        let snap = k3_device_irq_probe::probe_snapshot(i).unwrap();
        assert!(
            snap.link_change_cause(),
            "slot {i} lost its cause: insr={:#x}",
            snap.insr
        );
    }
    assert!(lines.iter().any(|l| l.contains("err=snapshot-capacity-exceeded")), "{lines:?}");
}

#[test]
fn quiet_window_has_no_new_claim() {
    let _guard = test_guard();
    // Clean quiet: events early, then nothing through the window end and the
    // 1 s quiet window.
    let mut env = fresh_env();
    {
        let mut b = BoardModel::lock();
        b.script = vec![
            (2_000_000_000, ScriptEvent::LinkChange { up: false, with_insr: true }),
            (4_000_000_000, ScriptEvent::LinkChange { up: true, with_insr: true }),
        ];
    }
    let (outcome, _) = run_stage(&mut env);
    let s = assert_window_ended(outcome);
    assert!(s.quiet_clean, "stable tail must be quiet");
    assert_eq!(s.quiet_claims, 0);
    // Dirty quiet: a late claim inside the quiet window is recorded as such.
    let mut env = fresh_env();
    {
        let mut b = BoardModel::lock();
        b.script = vec![(
            k3_device_irq_probe::STAGE10_WINDOW_NANOS + 500_000_000,
            ScriptEvent::LinkChange { up: false, with_insr: true },
        )];
    }
    let (outcome, lines) = run_stage(&mut env);
    let s = assert_window_ended(outcome);
    assert!(!s.quiet_clean, "a claim in the quiet window must be recorded: {lines:?}");
    assert_eq!(s.quiet_claims, 1);
}

#[test]
fn persistent_level_masks_and_reports() {
    let _guard = test_guard();
    let mut env = fresh_env();
    BoardModel::lock().persistent = true;
    let (outcome, lines) = run_stage(&mut env);
    match outcome {
        k3_device_irq_probe::Stage10Outcome::Stopped { reason } => {
            assert_eq!(reason, "snapshot-capacity-exceeded", "{lines:?}");
        }
        other => panic!("expected Stopped, got {other:?}"),
    }
    // The route ended masked at both layers (APLIC SETIE and local EIE) and
    // exactly the cap of snapshots was retained.
    let setie60 = env.aplic_read(aia::setie_word_off(60 / 32));
    assert_eq!(setie60 & (1 << (60 % 32)), 0, "APLIC source 60 must be masked");
    assert!(!enabled_bit(60), "local EIE 60 must be masked");
    assert_eq!(k3_device_irq_probe::probe_snapshot_count(), 32, "{lines:?}");
    assert!(
        lines.iter().any(|l| l.contains("err=snapshot-capacity-exceeded") && l.contains("mask60=1")),
        "{lines:?}"
    );
}

#[test]
fn device_deadline_stops_without_success() {
    let _guard = test_guard();
    let mut env = fresh_env();
    // No scripted events at all: the fixed 120 s window elapses with zero
    // claims and the stage returns (it never blocks forever, never fakes an
    // observation).
    let (outcome, lines) = run_stage(&mut env);
    let s = assert_window_ended(outcome);
    assert_eq!(s.claims, 0, "{lines:?}");
    assert_eq!(s.aligned_down + s.aligned_up + s.unaligned, 0);
    assert!(s.quiet_clean);
    assert!(
        lines.iter().any(|l| l.contains("result=no-device-event")),
        "the empty window must be reported as no-device-event: {lines:?}"
    );
    let elapsed = BoardModel::lock().now_ns;
    assert!(
        elapsed >= k3_device_irq_probe::STAGE10_WINDOW_NANOS,
        "the window must run its full fixed length, elapsed={elapsed}"
    );
}

#[test]
fn stage10_setup_arms_pin35_falling_edge_gpio_mux() {
    let _guard = test_guard();
    let mut env = fresh_env();
    // Board run 6 handoff/stage-6 outcome: mux=1 (GMAC), no edge armed.
    BoardModel::lock().mfpr35 = 0x0000_a441;
    {
        let mut b = BoardModel::lock();
        b.script = vec![(2_000_000_000, ScriptEvent::LinkChange { up: false, with_insr: true })];
    }
    let (outcome, lines) = run_stage(&mut env);
    assert_window_ended(outcome);
    // Setup write: mux field cleared to GPIO, FALL set, CLEAR hold released,
    // every other bit kept (0xa441 → 0xa420).
    assert!(
        lines.iter().any(|l| {
            l.contains("pad35 setup before=0x0000a441 written=0x0000a420 readback=0x0000a420 ok=1")
        }),
        "{lines:?}"
    );
    {
        let b = BoardModel::lock();
        let pad_writes: Vec<u32> = b
            .writes
            .iter()
            .filter(|(p, _)| *p == phy_probe::PAD35_MFPR_PADDR)
            .map(|(_, v)| *v)
            .collect();
        assert_eq!(
            pad_writes.first(),
            Some(&0x0000_a420),
            "setup writes GPIO mux + FALL with CLEAR released: {pad_writes:?}"
        );
    }
    // With the pad armed, the scripted unplug delivers exactly one claim.
    assert_eq!(k3_device_irq_probe::probe_snapshot_count(), 1, "{lines:?}");
    assert!(
        lines.iter().any(|l| l.contains("claim idx=0") && l.contains("aligned=1")),
        "{lines:?}"
    );
}

#[test]
fn insr_drain_releases_stale_cause_before_window() {
    let _guard = test_guard();
    let mut env = fresh_env();
    // A link-change cause latched before the stage (INTB holds low until the
    // COR read); arming the falling edge must not see it as a window claim.
    BoardModel::lock().insr = phy_probe::PHY_INSR_LINK_CHANGE;
    let (outcome, lines) = run_stage(&mut env);
    assert_window_ended(outcome);
    assert!(
        lines.iter().any(|l| l.contains("insr-drain=0x0010")),
        "{lines:?}"
    );
    assert_eq!(BoardModel::lock().insr, 0, "the drain COR read cleared INSR");
    assert_eq!(
        k3_device_irq_probe::probe_snapshot_count(),
        0,
        "the drained stale cause must not surface as a window claim: {lines:?}"
    );
}

#[test]
fn window_close_dumps_raw_state_without_claims() {
    let _guard = test_guard();
    let mut env = fresh_env();
    // An edge latched without a handler run (the "pad fired but source 60
    // never reached the IMSIC" discriminator) shows in the close dump.
    {
        let mut b = BoardModel::lock();
        b.edge[PIN35_WORD] |= PIN35_BIT;
        b.mfpr35 = 0x0000_a420;
    }
    let (outcome, lines) = run_stage(&mut env);
    assert_window_ended(outcome);
    let close = lines
        .iter()
        .find(|l| l.contains("window-close"))
        .expect("window-close diagnostic line");
    assert!(
        close.contains("edge=0x00000000/0x00000008/0x00000000/0x00000000"),
        "{close}"
    );
    assert!(close.contains("mfpr35=0x0000a420"), "{close}");
    assert!(close.contains("setip60=0"), "{close}");
}

#[test]
fn mac_routes_are_observed_without_software_trigger() {
    let _guard = test_guard();
    let mut env = fresh_env();
    {
        let mut b = BoardModel::lock();
        b.script = vec![(2_000_000_000, ScriptEvent::LinkChange { up: false, with_insr: true })];
    }
    let (outcome, lines) = run_stage(&mut env);
    assert_window_ended(outcome);
    // Route display lines exist for all three sources with raw APLIC state.
    for source in [60usize, 133, 277] {
        assert!(
            lines
                .iter()
                .any(|l| l.contains("route") && l.contains(&format!("source={source}")) && l.contains("target=")),
            "missing route display for {source}: {lines:?}"
        );
    }
    // GMAC 0x1160 and the APMU wake-enable bit are shown read-only.
    assert!(lines.iter().any(|l| l.contains("gmac1160=0x")), "{lines:?}");
    assert!(lines.iter().any(|l| l.contains("apmu-wake-bit12=")), "{lines:?}");
    // No software trigger ever fired a MAC or device interrupt.
    let triggers = env
        .aplic_writes
        .iter()
        .filter(|(off, _)| {
            *off == aia::APLIC_SETIPNUM_LE
                || *off == aia::APLIC_GENMSI
                || *off == aia::APLIC_SETIENUM
                || (0x1c00..0x1d00).contains(off)
        })
        .count();
    assert_eq!(triggers, 0, "observation must not software-trigger sources");
}
