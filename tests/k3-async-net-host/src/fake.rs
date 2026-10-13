//! Fake K3 board, DMA wire and backing witnesses for the async-net host
//! tests (MS12 Iteration 001).
//!
//! `FakeEnv` is the deterministic register/clock/MDIO board fake (same model
//! as the rustc-direct `k3-gmac-host-harness.rs`): the production driver and
//! adapter perform real volatile descriptor/buffer accesses against a leaked
//! 18-page arena. All board state lives behind one shared mutex so a test
//! keeps a scripting handle while the adapter owns a clone of the env.
//! `FakeWire` is the test-side view of the same arena — the device half of
//! the DMA: it completes device-owned TX descriptors and writes completed
//! frames into device-owned RX slots. `TrackingBacking` observes the
//! pre-publish release / post-publish retention branch of [`ArenaGuard`].

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::k3_dma::{ARENA_BYTES, DESC_BYTES, NUM_SLOTS, RX_BUF_OFF, RX_RING_OFF, TX_BUF_OFF, TX_RING_OFF};
use crate::k3_gmac::{
    ArenaBacking, ArenaGuard, Gmac, GmacEnv, RDES3_FD, RDES3_LD, RDES3_OWN, TDES3_LEN_MASK,
    TDES3_OWN,
};
use crate::mac_probe;
use crate::phy_probe::ProbeEnv;

/// Board addresses on the test wire (matches the kernel session constants).
pub const BOARD_MAC: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x26, 0x01];

/// The fake arena's device address (8 GiB, above the 4 GiB ring check).
pub const TEST_ARENA_PA: u64 = 0x2_0000_0000;

const GMAC_BASE_PADDR: u64 = mac_probe::MAC_BASE_PADDR;
const MDIO_ADDR_PADDR: u64 = GMAC_BASE_PADDR + 0x200;
const MDIO_DATA_PADDR: u64 = GMAC_BASE_PADDR + 0x204;
const APMU_EBUS_DELAY_PADDR: u64 = 0xd428_2800 + 0x3f0;
const MAC_HW_FEATURE1: u32 = 0x120;
const DMA_MODE: u32 = 0x1000;
/// BMSR autonegotiation-complete (bit5).
const BMSR_ANEG_COMPLETE: u16 = 1 << 5;

// ---------------------------------------------------------------------------
// Fake board environment
// ---------------------------------------------------------------------------

struct BoardState {
    clock_ns: u64,
    regs: BTreeMap<u32, u32>,
    arena_va: usize,
    arena_pa: u64,
    mdio_page: u16,
    mdio_regs: [u16; 32],
    mdio_pending: Option<(u32, u32, u64)>,
    mdio_data: u32,
    swr_ready_at: u64,
    apmu_delay: u32,
}

/// Scriptable fake board. `Clone` shares the state, so a test keeps a handle
/// and may script registers/clock between (or during) adapter calls.
#[derive(Clone)]
pub struct FakeEnv {
    board: Arc<Mutex<BoardState>>,
}

impl FakeEnv {
    pub fn new() -> (Self, &'static mut [u8; ARENA_BYTES]) {
        let aligned = Box::leak(Box::new(AlignedArena([0u8; ARENA_BYTES])));
        let slice = &mut aligned.0;
        let va = slice.as_ptr() as usize;
        let pa = 0x2_0000_0000;
        let env = FakeEnv {
            board: Arc::new(Mutex::new(BoardState {
                clock_ns: 0,
                regs: BTreeMap::from([(MAC_HW_FEATURE1, 0b10 << 14)]),
                arena_va: va,
                arena_pa: pa,
                mdio_page: 0,
                mdio_regs: [0; 32],
                mdio_pending: None,
                mdio_data: 0,
                swr_ready_at: 0,
                apmu_delay: 0,
            })),
        };
        (env, slice)
    }

    /// Scripting handle alias (the env itself is clonable and shared).
    pub fn handle(&self) -> Self {
        self.clone()
    }

    /// Post-stage8 happy-path PHY state: link up, AN complete, 1000FD.
    pub fn set_phy_link_1000fd(&self) {
        let mut b = self.board.lock().unwrap();
        b.mdio_regs[1] = 0x0004 | BMSR_ANEG_COMPLETE;
        b.mdio_regs[4] = 0x01e1;
        b.mdio_regs[5] = 0x41e1;
        b.mdio_regs[9] = 0x0300;
        b.mdio_regs[10] = 0x0800;
    }

    /// Sets a GMAC register value the fake controller reports back.
    pub fn set_reg(&self, offset: u32, value: u32) {
        self.board.lock().unwrap().regs.insert(offset, value);
    }

    /// Reads back a GMAC register the driver wrote (e.g. IRQ_EN).
    pub fn get_reg(&self, offset: u32) -> u32 {
        self.board.lock().unwrap().regs.get(&offset).copied().unwrap_or(0)
    }

    /// Advances the fake monotonic clock.
    pub fn advance_clock(&self, nanos: u64) {
        self.board.lock().unwrap().clock_ns += nanos;
    }

    fn serve_phy_read(b: &mut BoardState, reg: u32) -> u16 {
        if reg == crate::phy_probe::PHY_PAGE_SEL_REG {
            return b.mdio_page;
        }
        if b.mdio_page == 0 {
            return b.mdio_regs[reg as usize];
        }
        0
    }
}

#[repr(align(4096))]
struct AlignedArena([u8; ARENA_BYTES]);

impl ProbeEnv for FakeEnv {
    fn mmio_read(&mut self, paddr: u64) -> u32 {
        let mut b = self.board.lock().unwrap();
        match paddr {
            MDIO_ADDR_PADDR => {
                if let Some((reg, goc, ready)) = b.mdio_pending {
                    if b.clock_ns >= ready {
                        if goc == crate::phy_probe::GOC_READ >> 2 {
                            b.mdio_data = Self::serve_phy_read(&mut b, reg) as u32;
                        }
                        b.mdio_pending = None;
                    }
                }
                if b.mdio_pending.is_some() { 1 } else { 0 }
            }
            MDIO_DATA_PADDR => b.mdio_data,
            APMU_EBUS_DELAY_PADDR => b.apmu_delay,
            _ => 0,
        }
    }

    fn mmio_write(&mut self, paddr: u64, value: u32) {
        let mut b = self.board.lock().unwrap();
        match paddr {
            MDIO_DATA_PADDR => b.mdio_data = value,
            MDIO_ADDR_PADDR => {
                if value & 1 == 0 {
                    return;
                }
                let goc = (value >> 2) & 3;
                let reg = (value >> 16) & 0x1f;
                b.mdio_pending = Some((reg, goc, b.clock_ns));
            }
            APMU_EBUS_DELAY_PADDR => b.apmu_delay = value,
            _ => {}
        }
    }

    fn now_nanos(&self) -> u64 {
        self.board.lock().unwrap().clock_ns
    }

    fn wait_nanos(&mut self, nanos: u64) {
        self.board.lock().unwrap().clock_ns += nanos;
    }
}

impl GmacEnv for FakeEnv {
    fn reg_read(&mut self, offset: u32) -> u32 {
        let b = self.board.lock().unwrap();
        if offset == DMA_MODE {
            return if b.clock_ns >= b.swr_ready_at { 0 } else { 1 };
        }
        b.regs.get(&offset).copied().unwrap_or(0)
    }

    fn reg_write(&mut self, offset: u32, value: u32) {
        let mut b = self.board.lock().unwrap();
        b.regs.insert(offset, value);
        if offset == DMA_MODE && value & 1 != 0 {
            b.swr_ready_at = b.clock_ns;
        }
    }

    fn cache_flush(&mut self, _va: usize, _len: usize) {}
    fn cache_clean(&mut self, _va: usize, _len: usize) {}
    fn cache_invalidate(&mut self, _va: usize, _len: usize) {}
    fn fence_rw(&mut self) {}
    fn fence_iorw(&mut self) {}
}

// ---------------------------------------------------------------------------
// Arena backings
// ---------------------------------------------------------------------------

/// Leak-backed arena backing (host tests never reclaim the pages).
pub struct LeakBacking {
    va: usize,
    pa: u64,
}

impl ArenaBacking for LeakBacking {
    fn base_va(&self) -> usize {
        self.va
    }
    fn base_pa(&self) -> u64 {
        self.pa
    }
}

/// Backing drop witness: `dropped` is set by `Drop` so tests can observe the
/// release/retention branch of [`ArenaGuard`] through the adapter.
pub struct TrackingBacking {
    arena_va: usize,
    arena_pa: u64,
    pub dropped: Arc<std::sync::atomic::AtomicBool>,
}

impl TrackingBacking {
    pub fn new(va: usize, pa: u64, dropped: Arc<std::sync::atomic::AtomicBool>) -> Self {
        TrackingBacking { arena_va: va, arena_pa: pa, dropped }
    }
}

impl ArenaBacking for TrackingBacking {
    fn base_va(&self) -> usize {
        self.arena_va
    }
    fn base_pa(&self) -> u64 {
        self.arena_pa
    }
}

impl Drop for TrackingBacking {
    fn drop(&mut self) {
        self.dropped
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------
// Device half of the DMA (test-side view of the same arena)
// ---------------------------------------------------------------------------

/// Test-side device half: injects RX completions and drains TX into wire
/// frames by touching the same arena memory the driver accesses.
pub struct FakeWire {
    arena_va: usize,
    arena_pa: u64,
    arena: &'static mut [u8; ARENA_BYTES],
    /// The device's RX fetch position: completions are DMA-written in ring
    /// order, so injection targets this slot and then advances.
    rx_fetch: usize,
}

impl FakeWire {
    pub fn new(arena: &'static mut [u8; ARENA_BYTES], arena_pa: u64) -> Self {
        FakeWire {
            arena_va: arena.as_ptr() as usize,
            arena_pa,
            arena,
            rx_fetch: 0,
        }
    }

    /// A ready-to-script board: fake env plus the device-half wire.
    pub fn new_board() -> Board {
        let (env, arena) = FakeEnv::new();
        Board { env, wire: FakeWire::new(arena, TEST_ARENA_PA) }
    }

    pub fn arena_pa(&self) -> u64 {
        self.arena_pa
    }

    fn rx_desc_word(&self, slot: usize, word: usize) -> u32 {
        let off = RX_RING_OFF + slot * DESC_BYTES + word * 4;
        u32::from_le_bytes(self.arena[off..off + 4].try_into().unwrap())
    }

    fn set_rx_desc_word(&mut self, slot: usize, word: usize, v: u32) {
        let off = RX_RING_OFF + slot * DESC_BYTES + word * 4;
        self.arena[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn tx_desc_word(&self, slot: usize, word: usize) -> u32 {
        let off = TX_RING_OFF + slot * DESC_BYTES + word * 4;
        u32::from_le_bytes(self.arena[off..off + 4].try_into().unwrap())
    }

    fn set_tx_desc_word(&mut self, slot: usize, word: usize, v: u32) {
        let off = TX_RING_OFF + slot * DESC_BYTES + word * 4;
        self.arena[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }

    /// Device-side RX completion: the frame is DMA-written into the fetch
    /// slot in ring order (zero-padded to the 60-byte minimum; the raw
    /// descriptor length includes the 4-byte FCS the MAC appends).
    pub fn inject_rx_frame(&mut self, frame: &[u8]) {
        assert!(frame.len() <= 1514, "frame longer than the driver MTU");
        let slot = self.rx_fetch;
        assert!(
            self.rx_desc_word(slot, 3) & RDES3_OWN != 0,
            "RX fetch slot {slot} is not device-owned (driver fell behind)"
        );
        let wire_len = frame.len().max(60);
        let off = RX_BUF_OFF + slot * crate::k3_dma::BUFFER_BYTES;
        self.arena[off..off + frame.len()].copy_from_slice(frame);
        let raw_len = (wire_len + 4) as u32;
        self.set_rx_desc_word(slot, 3, RDES3_FD | RDES3_LD | raw_len);
        self.rx_fetch = (slot + 1) % NUM_SLOTS;
    }

    /// Device-side RX completion of a raw length without FD/LD context bits:
    /// the adapter classifies and drops it (bounded continue, never fatal).
    pub fn inject_rx_reject(&mut self) {
        let slot = self.rx_fetch;
        assert!(self.rx_desc_word(slot, 3) & RDES3_OWN != 0);
        // FD missing: NotComplete classification, length legal on its own.
        self.set_rx_desc_word(slot, 3, RDES3_LD | (68u32));
        self.rx_fetch = (slot + 1) % NUM_SLOTS;
    }

    /// Device-side TX completion: every OWN-set TX descriptor is "sent" and
    /// cleared; the wire frames (length-field bytes, FCS never included) are
    /// returned in slot order.
    pub fn drain_board_tx(&mut self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for slot in 0..NUM_SLOTS {
            let w3 = self.tx_desc_word(slot, 3);
            if w3 & TDES3_OWN != 0 {
                let len = (w3 & TDES3_LEN_MASK) as usize;
                let off = TX_BUF_OFF + slot * crate::k3_dma::BUFFER_BYTES;
                out.push(self.arena[off..off + len].to_vec());
                self.set_tx_desc_word(slot, 3, w3 & !TDES3_OWN);
            }
        }
        out
    }

    /// A fresh backing over this wire's arena.
    pub fn backing(&self) -> LeakBacking {
        LeakBacking { va: self.arena_va, pa: self.arena_pa }
    }

    /// Tracking backing over this wire's arena (drop-witness tests).
    pub fn tracking_backing(
        &self,
        dropped: Arc<std::sync::atomic::AtomicBool>,
    ) -> ArenaGuard<TrackingBacking> {
        ArenaGuard::new(TrackingBacking::new(self.arena_va, self.arena_pa, dropped))
    }

    /// ArenaGuard over this wire's arena.
    pub fn arena_guard(&self) -> ArenaGuard<LeakBacking> {
        ArenaGuard::new(self.backing())
    }
}

// ---------------------------------------------------------------------------
// Board bundle
// ---------------------------------------------------------------------------

/// Board bundle: the fake env plus its device-half wire over one arena.
pub struct Board {
    pub env: FakeEnv,
    pub wire: FakeWire,
}

impl Board {
    pub fn new() -> Board {
        FakeWire::new_board()
    }

    /// A driver initialized to Running in IOC async mode with TX enabled —
    /// the exact state the board Service path hands to the adapter.
    pub fn ready_async_driver(&mut self) -> Gmac<LeakBacking> {
        self.env.set_phy_link_1000fd();
        let mut driver = Gmac::new_async(self.wire.arena_guard()).expect("arena placement");
        driver.init_rx(&mut self.env).expect("init_rx");
        driver.enable_tx(&mut self.env);
        driver
    }
}
