//! K3 GMAC polling driver pure-logic host test harness
//! (MS11 Iteration 000, tasks 1.1–1.3).
//!
//! Uses `#[path]` to include the real production modules: the K3 platform
//! DMA foundation (`crates/axplat-riscv64-k3/src/dma.rs`, root-module sibling
//! `k3_dma` for the crate-dep pattern used by `k3_gmac`), the probe modules
//! (for MDIO reuse through `crate::platform`) and the new driver
//! (`kernel/src/drivers/k3_gmac/`). `FakeEnv` is a deterministic board fake:
//! a GMAC register file with scriptable SWR behavior, a traced cache/fence
//! seam, a fake monotonic clock and a leaked 18-page arena so the production
//! code performs real volatile descriptor/buffer accesses.
//!
//! Timing conventions (same as the MS10 harnesses):
//! - `wait_nanos` advances the fake clock by exactly the requested nanos.
//! - Each register access advances the clock by `access_cost_ns` (default 0).

#![allow(dead_code, unused_imports)]

extern crate core;

#[path = "../crates/axplat-riscv64-k3/src/dma.rs"]
mod k3_dma;

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

#[path = "../kernel/src/platform/k3_network_probe.rs"]
mod k3_network_probe;

#[path = "../kernel/src/platform/k3_gmac/mod.rs"]
mod k3_gmac;

use k3_dma::{
    addr_fits_width, addr_hi32, addr_lo32, check_cache_range, decode_addr_width, ArenaLayout,
    AddrWidth, DmaAddressError, ARENA_BYTES, BUFFER_BYTES, CACHE_BLOCK_BYTES, DESC_BYTES,
    NUM_SLOTS, PAGE_SIZE_BYTES, RX_BUF_OFF, RX_RING_OFF, SAFE_RAM_END_PADDR, TX_BUF_OFF,
    TX_RING_OFF,
};
use k3_gmac::{
    ArenaBacking, ArenaGuard, GmacEnv, GmacError, GmacState, Gmac, Phase, RawRx, RxDrop, RxPoll,
    TxPoll, TxSubmit, TxSubmitError, TX_TERMINAL_NANOS,
    RX_TAIL_OFF, DMA_CHAN_IRQ_EN, DMA_CHAN_STATUS, DMA_RX_CTL, DMA_RX_CTL_SR, MAC_CONFIG,
    MAC_CONFIG_RE, RDES3_CTXT, RDES3_ES, RDES3_FD, RDES3_LD, RDES3_LEN_MASK, RDES3_OWN,
    DMA_STATUS_FBE, DMA_STATUS_RBU, DMA_STATUS_RI, TDES3_ES, TDES3_FD, TDES3_LD, TDES3_OWN,
    DMA_TX_CTL, DMA_TX_CTL_OSP, DMA_TX_CTL_PBL_8, DMA_TX_CTL_ST, DMA_TX_TAIL,
};
use phy_probe::ProbeEnv;

// ---------------------------------------------------------------------------
// Fake environment
// ---------------------------------------------------------------------------

/// Backing drop witness: `dropped` is set by `Drop` so tests can observe the
/// release/retention branch of [`ArenaGuard`].
struct TrackingBacking {
    arena_va: usize,
    arena_pa: u64,
    dropped: std::sync::Arc<std::sync::atomic::AtomicBool>,
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

struct FakeEnv {
    clock_ns: u64,
    access_cost_ns: u64,
    /// GMAC base-relative register file.
    regs: std::collections::BTreeMap<u32, u32>,
    reg_writes: Vec<(u32, u32)>,
    /// Absolute-paddr accesses outside the GMAC window (APMU, MDIO via probe).
    probe_writes: Vec<(u64, u32)>,
    /// Cache/fence operation trace, in issue order.
    ops: Vec<&'static str>,
    /// Every board access in issue order (masks, registers, MDIO, cache) —
    /// the stage 11 gate/order witness.
    access_log: Vec<&'static str>,
    arena_va: usize,
    arena_pa: u64,
    // --- MDIO engine (Clause 22 over MAC+0x200/0x204, MS10 harness model) ---
    mdio_page: u16,
    mdio_regs: [u16; 32],
    mdio_pending: Option<(u32, u32, u64)>,
    mdio_data: u32,
    mdio_latency_ns: u64,
    mdio_busy_stuck: bool,
    // --- SWR model: bit0 reads set until `swr_ready_at` ---
    swr_ready_at: u64,
    swr_latency_ns: u64,
    swr_stuck: bool,
    /// DMA_MODE read count (the T7 confirm-withhold witness: the injection
    /// reads the mode word exactly once, then withholds every later read).
    dma_mode_reads: u64,
    /// Cost advanced by each cache operation (models real CBO time so the
    /// 10 ms refill/reclaim deadlines are stallable in tests).
    cache_cost_ns: u64,
    /// Scripted device completion applied on the first idle wait inside the
    /// window (the device only completes after the ring is published).
    pending_arp: Option<([u8; 4], [u8; 4])>,
    /// Scripted ARP reply (oper=2) applied on the next wait (stage 12 RX).
    pending_arp_reply: Option<([u8; 4], [u8; 4])>,
    /// Scripted TX completion: on every wait, the oldest OWN-set TX
    /// descriptor is completed by the device (OWN cleared, FD/LD/len kept).
    pending_tx_complete: bool,
    /// When completing a TX descriptor, the device also flags ES (local error).
    pending_tx_es: bool,
    // --- APMU EMAC delay register (RMW readback) ---
    apmu_delay: u32,
}

/// MDIO absolute addresses (the probe seam routes these through ProbeEnv).
const MDIO_ADDR_PADDR: u64 = GMAC_BASE_PADDR + 0x200;
const MDIO_DATA_PADDR: u64 = GMAC_BASE_PADDR + 0x204;
/// C22 registers 5/9/10 used by the link decode (BMSR/ANA live in phy_probe).
const LPA_REG: u32 = 5;
const CTRL1000_REG: u32 = 9;
const STAT1000_REG: u32 = 10;
/// BMSR autonegotiation-complete (bit5).
const BMSR_ANEG_COMPLETE: u16 = 1 << 5;

const GMAC_BASE_PADDR: u64 = mac_probe::MAC_BASE_PADDR;
const GMAC_WINDOW: u64 = mac_probe::MAC_WINDOW_SIZE;
/// APMU EMAC delay register (`k3_gmac` board facts: base + 0x3f0).
const APMU_EBUS_DELAY_PADDR: u64 = 0xd428_2800 + 0x3f0;

/// 4 KiB-aligned arena backing (a plain Box only guarantees 1-byte
/// alignment, which the checked layout correctly refuses).
#[repr(align(4096))]
struct AlignedArena([u8; ARENA_BYTES]);

/// Leaks a fresh zeroed 18-page arena; returns (va, pa, static slice for
/// test-side direct memory assertions).
fn fresh_arena() -> (usize, u64, &'static mut [u8; ARENA_BYTES]) {
    let aligned = Box::leak(Box::new(AlignedArena([0u8; ARENA_BYTES])));
    let slice = &mut aligned.0;
    // A high-board-like PA inside the safe RAM window, 4 KiB aligned.
    (slice.as_ptr() as usize, 0x2_0000_0000, slice)
}

impl FakeEnv {
    fn new() -> (Self, &'static mut [u8; ARENA_BYTES]) {
        let (va, pa, slice) = fresh_arena();
        (
            FakeEnv {
                clock_ns: 0,
                access_cost_ns: 0,
                // Default controller capability: 48-bit addressing
                // (HW_FEATURE1[15:14] = 0b10), the board reality.
                regs: std::collections::BTreeMap::from([(0x120u32, 0b10 << 14)]),
                reg_writes: Vec::new(),
                probe_writes: Vec::new(),
                ops: Vec::new(),
                access_log: Vec::new(),
                arena_va: va,
                arena_pa: pa,
                mdio_page: 0,
                mdio_regs: [0; 32],
                mdio_pending: None,
                mdio_data: 0,
                mdio_latency_ns: 0,
                mdio_busy_stuck: false,
                swr_ready_at: 0,
                swr_latency_ns: 0,
                swr_stuck: false,
                dma_mode_reads: 0,
                cache_cost_ns: 0,
                pending_arp: None,
                pending_arp_reply: None,
                pending_tx_complete: false,
                pending_tx_es: false,
                apmu_delay: 0,
            },
            slice,
        )
    }

    /// Post-stage8 happy-path PHY state: link up, AN complete, both sides
    /// advertising 1000FD (contract decode: CTRL1000 bit9 vs STAT1000 bit11).
    fn set_phy_link_1000fd(&mut self) {
        self.mdio_regs[1] = 0x0004 | BMSR_ANEG_COMPLETE; // link + AN done
        self.mdio_regs[4] = 0x01e1; // 802.3 selector + 10/100 abilities
        self.mdio_regs[5] = 0x41e1; // peer 10/100 abilities
        self.mdio_regs[9] = 0x0300; // advertise 1000FD|1000HD
        self.mdio_regs[10] = 0x0800; // peer 1000FD capable (bit11)
    }

    /// Link up with only 100HD in common (ANA bit7 & LPA bit7).
    fn set_phy_link_100hd(&mut self) {
        self.mdio_regs[1] = 0x0004 | BMSR_ANEG_COMPLETE;
        self.mdio_regs[4] = 0x01e1;
        self.mdio_regs[5] = 0x4081; // peer: selector + 100HD only
        self.mdio_regs[9] = 0x0000; // no 1000 advertise
        self.mdio_regs[10] = 0x0000;
    }

    /// Counts how many register writes hit the RX data-plane window
    /// (ring bases / lengths / tail). `reset_timeout_no_start` (1.2) uses
    /// this to prove a failed reset never touches the ring.
    fn rx_ring_writes(&self) -> usize {
        // Data-plane-starting writes only: ring bases/lengths/tail, or an
        // RX_CTL write that sets SR (the bounded stop's SR-clear does not
        // count as a start).
        self.reg_writes
            .iter()
            .filter(|(off, v)| (0x1110..=0x1130).contains(off) || (*off == 0x1108 && v & 1 != 0))
            .count()
    }

    fn reg_write_offsets(&self) -> Vec<u32> {
        self.reg_writes.iter().map(|(off, _)| *off).collect()
    }

    /// DMA_MODE read count (the T7 confirm-withhold witness).
    fn dma_mode_reads(&self) -> u64 {
        self.dma_mode_reads
    }

    /// Advances the fake clock without any board access (the withheld confirm
    /// path performs no access, so only the test can move time forward).
    fn advance_clock(&mut self, nanos: u64) {
        self.clock_ns += nanos;
    }
}

impl k3_network_probe::Stage11Env for FakeEnv {
    fn mask_source(&mut self, source: usize) {
        match source {
            60 => self.access_log.push("mask:60"),
            133 => self.access_log.push("mask:133"),
            277 => self.access_log.push("mask:277"),
            other => panic!("unexpected mask source {other}"),
        }
    }
}

impl FakeEnv {
    /// Writes a padded ARP frame (oper 1 = request, 2 = reply) into RX slot 0
    /// and marks the descriptor completed — the device's mid-window view.
    fn fake_write_arp(&mut self, spa: [u8; 4], tpa: [u8; 4], oper: u8) {
        let mut frame = [0u8; 60];
        frame[12] = 0x08;
        frame[13] = 0x06;
        frame[14] = 0x00;
        frame[15] = 0x01; // htype ethernet
        frame[16] = 0x08;
        frame[17] = 0x00; // ptype IPv4
        frame[18] = 6;
        frame[19] = 4;
        frame[20] = 0;
        frame[21] = oper;
        frame[28..32].copy_from_slice(&spa);
        frame[38..42].copy_from_slice(&tpa);
        unsafe {
            let base = self.arena_va as *mut u8;
            core::ptr::copy_nonoverlapping(frame.as_ptr(), base.add(RX_BUF_OFF), frame.len());
            let w3 = base.add(RX_RING_OFF + 3 * 4) as *mut u32;
            w3.write_volatile(RDES3_FD | RDES3_LD | 64);
        }
    }

    fn serve_phy_read(&mut self, reg: u32) -> u16 {
        if reg == phy_probe::PHY_PAGE_SEL_REG {
            return self.mdio_page;
        }
        if self.mdio_page == 0 {
            return self.mdio_regs[reg as usize];
        }
        0
    }

    fn serve_phy_write(&mut self, reg: u32, value: u16) {
        if reg == phy_probe::PHY_PAGE_SEL_REG {
            self.mdio_page = value;
            return;
        }
        if self.mdio_page == 0 {
            self.mdio_regs[reg as usize] = value;
        }
    }
}

impl ProbeEnv for FakeEnv {
    fn mmio_read(&mut self, paddr: u64) -> u32 {
        self.clock_ns += self.access_cost_ns;
        self.access_log.push(if paddr == MDIO_ADDR_PADDR || paddr == MDIO_DATA_PADDR { "mdio" } else { "mmio" });
        match paddr {
            MDIO_ADDR_PADDR => {
                if self.mdio_busy_stuck {
                    return 1;
                }
                if let Some((reg, goc, ready)) = self.mdio_pending {
                    if self.clock_ns >= ready {
                        if goc == phy_probe::GOC_READ >> 2 {
                            self.mdio_data = self.serve_phy_read(reg) as u32;
                        } else {
                            self.serve_phy_write(reg, self.mdio_data as u16);
                        }
                        self.mdio_pending = None;
                    }
                }
                if self.mdio_pending.is_some() { 1 } else { 0 }
            }
            MDIO_DATA_PADDR => self.mdio_data,
            APMU_EBUS_DELAY_PADDR => self.apmu_delay,
            _ => 0,
        }
    }

    fn mmio_write(&mut self, paddr: u64, value: u32) {
        self.clock_ns += self.access_cost_ns;
        self.access_log.push(if paddr == MDIO_ADDR_PADDR || paddr == MDIO_DATA_PADDR { "mdio" } else { "mmio" });
        self.probe_writes.push((paddr, value));
        match paddr {
            MDIO_DATA_PADDR => self.mdio_data = value,
            MDIO_ADDR_PADDR => {
                if value & 1 == 0 {
                    return;
                }
                let goc = (value >> 2) & 3;
                let reg = (value >> 16) & 0x1f;
                let phy = (value >> 21) & 0x1f;
                assert_eq!(phy, phy_probe::PHY_ADDR, "driver must address PHY 1 only");
                self.mdio_pending = Some((reg, goc, self.clock_ns + self.mdio_latency_ns));
            }
            APMU_EBUS_DELAY_PADDR => self.apmu_delay = value,
            _ => {}
        }
    }

    fn now_nanos(&self) -> u64 {
        self.clock_ns
    }

    fn wait_nanos(&mut self, nanos: u64) {
        if let Some((spa, tpa)) = self.pending_arp.take() {
            self.fake_write_arp(spa, tpa, 1);
        }
        if let Some((spa, tpa)) = self.pending_arp_reply.take() {
            self.fake_write_arp(spa, tpa, 2);
        }
        if self.pending_tx_complete {
            // The device completes the oldest OWN-set TX descriptor: OWN
            // cleared, FD/LD/len kept, ES only when scripted.
            unsafe {
                let base = self.arena_va as *mut u8;
                for slot in 0..NUM_SLOTS {
                    let w3p = base.add(TX_RING_OFF + slot * DESC_BYTES + 3 * 4) as *mut u32;
                    let w3 = w3p.read_volatile();
                    if w3 & TDES3_OWN != 0 {
                        let mut done = w3 & !TDES3_OWN;
                        if self.pending_tx_es {
                            done |= TDES3_ES;
                        }
                        w3p.write_volatile(done);
                        break;
                    }
                }
            }
        }
        self.clock_ns += nanos;
    }
}

impl GmacEnv for FakeEnv {
    fn reg_read(&mut self, offset: u32) -> u32 {
        self.clock_ns += self.access_cost_ns;
        self.access_log.push("reg");
        assert!((offset as u64) < GMAC_WINDOW, "register {offset:#x} outside GMAC window");
        if offset == 0x1000 {
            // SWR model: bit0 stays set until `swr_ready_at`.
            self.dma_mode_reads += 1;
            return if self.clock_ns < self.swr_ready_at { 1 } else { 0 };
        }
        self.regs.get(&offset).copied().unwrap_or(0)
    }

    fn reg_write(&mut self, offset: u32, value: u32) {
        self.clock_ns += self.access_cost_ns;
        self.access_log.push("reg");
        assert!((offset as u64) < GMAC_WINDOW, "register {offset:#x} outside GMAC window");
        self.reg_writes.push((offset, value));
        if offset == 0x1000 && value & 1 != 0 {
            // SWR trigger: model the self-clear (or a stuck reset).
            self.swr_ready_at = if self.swr_stuck {
                u64::MAX
            } else {
                self.clock_ns + self.swr_latency_ns
            };
            return;
        }
        self.regs.insert(offset, value);
    }

    fn cache_flush(&mut self, _va: usize, len: usize) {
        self.clock_ns += self.cache_cost_ns;
        self.access_log.push("cache");
        assert!(len > 0 && len % CACHE_BLOCK_BYTES == 0, "cache range not block-granular");
        self.ops.push("flush");
    }

    fn cache_clean(&mut self, _va: usize, len: usize) {
        self.clock_ns += self.cache_cost_ns;
        self.access_log.push("cache");
        assert!(len > 0 && len % CACHE_BLOCK_BYTES == 0, "cache range not block-granular");
        self.ops.push("clean");
    }

    fn cache_invalidate(&mut self, _va: usize, len: usize) {
        self.clock_ns += self.cache_cost_ns;
        self.access_log.push("cache");
        assert!(len > 0 && len % CACHE_BLOCK_BYTES == 0, "cache range not block-granular");
        self.ops.push("inval");
    }

    fn fence_rw(&mut self) {
        self.ops.push("fence_rw");
    }

    fn fence_iorw(&mut self) {
        self.ops.push("fence_iorw");
    }
}

/// Reads a descriptor word from the arena (test-side volatile view).
fn desc_word(arena: &[u8; ARENA_BYTES], env: &FakeEnv, slot: usize, word: usize) -> u32 {
    let va = env.arena_va + RX_RING_OFF + slot * DESC_BYTES + word * 4;
    let off = va - env.arena_va;
    u32::from_le_bytes(arena[off..off + 4].try_into().unwrap())
}

/// Writes a descriptor word directly (simulates hardware-side completion).
fn set_desc_word(arena: &mut [u8; ARENA_BYTES], slot: usize, word: usize, v: u32) {
    let off = RX_RING_OFF + slot * DESC_BYTES + word * 4;
    arena[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

/// Writes payload bytes into an RX buffer (simulates hardware DMA write).
fn set_buf_bytes(arena: &mut [u8; ARENA_BYTES], slot: usize, bytes: &[u8]) {
    let off = RX_BUF_OFF + slot * BUFFER_BYTES;
    arena[off..off + bytes.len()].copy_from_slice(bytes);
}

/// Reads copied-back bytes from the caller scratch.
fn scratch_bytes(scratch: &[u8], n: usize) -> Vec<u8> {
    scratch[..n].to_vec()
}

/// Reads a TX descriptor word from the arena (test-side view).
fn tx_desc_word(arena: &[u8; ARENA_BYTES], slot: usize, word: usize) -> u32 {
    let off = TX_RING_OFF + slot * DESC_BYTES + word * 4;
    u32::from_le_bytes(arena[off..off + 4].try_into().unwrap())
}

/// Writes a TX descriptor word directly (simulates hardware-side completion).
fn set_tx_desc_word(arena: &mut [u8; ARENA_BYTES], slot: usize, word: usize, v: u32) {
    let off = TX_RING_OFF + slot * DESC_BYTES + word * 4;
    arena[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

// ---------------------------------------------------------------------------
// Tests — task 1.1 witness list
// ---------------------------------------------------------------------------

/// Layout facts: constants, partition isolation, slot ranges, the checked
/// constructor (alignment / overflow / safe RAM / 4 GiB ring crossing),
/// address-width decode and rejection (32-bit capability must refuse this
/// board's >4 GiB RAM without truncation), cache-range checks, and the
/// cache/fence order of the RX publish and reap sequences.
#[test]
fn dma_span_and_cache_order() {
    // -- constants and partition ------------------------------------------
    assert_eq!(ARENA_BYTES, 18 * PAGE_SIZE_BYTES, "18 pages");
    assert_eq!(ARENA_BYTES, 73_728, "73,728 bytes total");
    assert_eq!(RX_RING_OFF, 0);
    assert_eq!(TX_RING_OFF, PAGE_SIZE_BYTES);
    assert_eq!(RX_BUF_OFF, 2 * PAGE_SIZE_BYTES);
    assert_eq!(TX_BUF_OFF, RX_BUF_OFF + 8 * PAGE_SIZE_BYTES);
    assert_eq!(DESC_BYTES, 64);
    assert_eq!(BUFFER_BYTES, 2048);
    assert_eq!(NUM_SLOTS, 16);
    assert_eq!(CACHE_BLOCK_BYTES, 64);
    // Partitions must not overlap and slots must stay inside their own
    // partition (checked for every slot).
    let l = ArenaLayout::new(0x2_0000_0000, 0xffff_ffc2_0000_0000).expect("high safe PA layout");
    for s in 0..NUM_SLOTS {
        let d = l.rx_desc_pa(s).unwrap();
        let b = l.rx_buf_pa(s).unwrap();
        assert!(d >= 0x2_0000_0000 && d < 0x2_0000_0000 + PAGE_SIZE_BYTES as u64, "desc in RX ring page");
        assert!(
            b >= 0x2_0000_0000 + RX_BUF_OFF as u64
                && b < 0x2_0000_0000 + TX_BUF_OFF as u64,
            "rx buffer in RX buffer partition"
        );
        let td = l.tx_desc_pa(s).unwrap();
        let tb = l.tx_buf_pa(s).unwrap();
        assert!(
            td >= 0x2_0000_0000 + TX_RING_OFF as u64
                && td < 0x2_0000_0000 + RX_BUF_OFF as u64,
            "tx desc in TX ring page"
        );
        assert!(tb >= 0x2_0000_0000 + TX_BUF_OFF as u64, "tx buffer in TX partition");
    }
    assert_eq!(l.rx_desc_pa(0).unwrap(), 0x2_0000_0000);
    assert_eq!(l.rx_desc_pa(NUM_SLOTS - 1).unwrap(), 0x2_0000_0000 + 15 * 64);
    assert_eq!(l.rx_next_tail_pa().unwrap(), 0x2_0000_0000 + NUM_SLOTS as u64 * DESC_BYTES as u64);
    // High/low split keeps the high bits.
    assert_eq!(addr_lo32(0x2_0000_0104), 0x0000_0104);
    assert_eq!(addr_hi32(0x2_0000_0104), 2);
    assert_eq!(addr_lo32(0x1_2345_6789), 0x2345_6789);
    assert_eq!(addr_hi32(0x1_2345_6789), 1);

    // -- constructor rejections -------------------------------------------
    // Not 4 KiB aligned (PA).
    assert!(matches!(
        ArenaLayout::new(0x2_0000_0800, 0xffff_ffc2_0000_0000),
        Err(DmaAddressError::Unaligned)
    ));
    // Not 4 KiB aligned (VA).
    assert!(matches!(
        ArenaLayout::new(0x2_0000_0000, 0xffff_ffc2_0000_0800),
        Err(DmaAddressError::Unaligned)
    ));
    // Span addition overflow (4 KiB-aligned base near the address-space end).
    assert!(matches!(
        ArenaLayout::new(0xFFFF_FFFF_FFFF_F000, 0xffff_ffc2_0000_0000),
        Err(DmaAddressError::Overflow)
    ));
    // Below the safe RAM window.
    assert!(matches!(
        ArenaLayout::new(0x1000_0000, 0xffff_ffc0_1000_0000),
        Err(DmaAddressError::OutsideSafeRam)
    ));
    // Span crosses the safe RAM window end.
    assert!(matches!(
        ArenaLayout::new(SAFE_RAM_END_PADDR - 0x1000, 0xffff_ffc2_efff_0000),
        Err(DmaAddressError::OutsideSafeRam)
    ));
    // Slot out of range.
    assert!(matches!(l.rx_desc_pa(16), Err(DmaAddressError::SlotOutOfRange)));
    assert!(matches!(l.rx_buf_pa(16), Err(DmaAddressError::SlotOutOfRange)));
    assert!(matches!(l.tx_desc_pa(usize::MAX), Err(DmaAddressError::SlotOutOfRange)));
    // RX ring + next-tail must not cross a 4 GiB boundary (32-bit tail
    // register would be ambiguous). The standalone check is exercised
    // directly: an aligned arena layout keeps every ring 4 KiB-aligned so
    // the constructor case is unreachable, the guarantee is not.
    assert!(k3_dma::ring_crosses_4g(0x1_FFFF_FC00), "ring+tail crossing 0x2_0000_0000");
    assert!(k3_dma::ring_crosses_4g(0x1_FFFF_FFFF), "ring straddling the boundary");
    assert!(!k3_dma::ring_crosses_4g(0x2_0000_0000), "aligned ring below the boundary");
    assert!(!k3_dma::ring_crosses_4g(0x2_0000_0000 - 4096), "ring ending at the boundary");

    // -- address width decode ---------------------------------------------
    assert!(matches!(decode_addr_width(0 << 14), Ok(AddrWidth::W32)));
    assert!(matches!(decode_addr_width(1 << 14), Ok(AddrWidth::W40)));
    assert!(matches!(decode_addr_width(2 << 14), Ok(AddrWidth::W48)));
    assert!(matches!(decode_addr_width(3 << 14), Err(DmaAddressError::WidthReserved)));
    // This board's RAM is above 4 GiB: a 32-bit capability must be rejected
    // outright, never truncated.
    assert!(matches!(
        addr_fits_width(0x2_0000_0000, &AddrWidth::W32),
        Err(DmaAddressError::AddressTooWide { width_bits: 32, .. })
    ));
    assert!(addr_fits_width(0x2_0000_0000, &AddrWidth::W40).is_ok());
    assert!(matches!(
        addr_fits_width(0x100_0000_0000, &AddrWidth::W40),
        Err(DmaAddressError::AddressTooWide { width_bits: 40, .. })
    ));
    assert!(addr_fits_width(0x100_0000_0000, &AddrWidth::W48).is_ok());

    // -- cache range checks -------------------------------------------------
    let arena_va = 0xffff_ffc2_0000_0000usize;
    assert!(check_cache_range(arena_va, arena_va, DESC_BYTES).is_ok());
    assert!(check_cache_range(arena_va, arena_va + RX_BUF_OFF, BUFFER_BYTES).is_ok());
    assert!(check_cache_range(arena_va, arena_va - 64, 64).is_err(), "before arena");
    assert!(check_cache_range(arena_va, arena_va + ARENA_BYTES, 64).is_err(), "past end");
    assert!(check_cache_range(arena_va, arena_va, 0).is_err(), "empty range");
    assert!(check_cache_range(arena_va, arena_va, ARENA_BYTES + 1).is_err());

    // -- RX publish cache/fence order ---------------------------------------
    let (mut env, mut arena) = FakeEnv::new();
    let l = ArenaLayout::new(env.arena_pa, env.arena_va).unwrap();
    k3_gmac::raw_refill_rx_desc(&mut env, &l, 3).unwrap();
    assert_eq!(
        env.ops,
        vec![
            "fence_rw", // before the RX buffer flush
            "flush",    // whole 2048-byte RX buffer
            "fence_rw", // release fence before the final OWN word
            "fence_rw", // before the descriptor clean
            "clean",    // whole 64-byte descriptor
            "fence_iorw", // full fence before the doorbell
        ],
        "publish order: flush buffer → release fence → OWN → clean desc → full fence"
    );
    assert_eq!(
        env.reg_writes.last(),
        Some(&(RX_TAIL_OFF, addr_lo32(l.rx_desc_pa(3).unwrap()))),
        "the doorbell writes the refilled slot address to the RX tail"
    );
    // Descriptor memory: address split low/high, control 0, OWN|BUF1V last.
    assert_eq!(desc_word(&arena, &env, 3, 0), addr_lo32(l.rx_buf_pa(3).unwrap()));
    assert_eq!(desc_word(&arena, &env, 3, 1), addr_hi32(l.rx_buf_pa(3).unwrap()));
    assert_eq!(desc_word(&arena, &env, 3, 2), 0, "no interrupt-on-completion");
    assert_eq!(
        desc_word(&arena, &env, 3, 3),
        k3_gmac::RDES3_OWN | k3_gmac::RDES3_BUF1V,
        "OWN is the final descriptor word"
    );

    // -- RX reap cache/fence order (owned desc: nothing else touched) -------
    env.ops.clear();
    env.reg_writes.clear();
    let mut scratch = [0u8; 1514];
    match k3_gmac::raw_poll_rx_desc(&mut env, &l, 3, &mut scratch).unwrap() {
        RawRx::NotOwned => {}
        other => panic!("device-owned descriptor must report NotOwned, got {other:?}"),
    }
    assert_eq!(
        env.ops,
        vec!["inval", "fence_rw"],
        "reap: invalidate descriptor → fence → volatile OWN read; OWN=1 must not touch the buffer"
    );

    // -- RX reap of a completed frame ---------------------------------------
    // The fake hardware completed the frame: OWN clear, FD|LD set, ES clear,
    // raw length 70 (66 payload after FCS). Payload bytes are visible.
    set_desc_word(&mut arena, 3, 3, k3_gmac::RDES3_FD | k3_gmac::RDES3_LD | 70);
    for b in arena[RX_BUF_OFF + 3 * BUFFER_BYTES..RX_BUF_OFF + 3 * BUFFER_BYTES + 66].iter_mut() {
        *b = 0xA5;
    }
    env.ops.clear();
    let out = k3_gmac::raw_poll_rx_desc(&mut env, &l, 3, &mut scratch).unwrap();
    match out {
        RawRx::Frame { len, raw_w3 } => {
            assert_eq!(len, 66, "FCS stripped");
            assert_eq!(raw_w3, k3_gmac::RDES3_FD | k3_gmac::RDES3_LD | 70, "raw status retained");
            assert_eq!(
                scratch_bytes(&scratch, 66),
                vec![0xA5u8; 66],
                "CPU copy matches the DMA-written buffer"
            );
        }
        other => panic!("expected Frame, got {other:?}"),
    }
    assert_eq!(
        env.ops,
        vec![
            "inval",    // descriptor invalidate before the OWN read
            "fence_rw", // fence after descriptor invalidate
            "fence_rw", // acquire fence after OWN=0
            "inval",    // buffer invalidate
            "fence_rw", // fence after buffer invalidate
        ],
        "reap order: desc inval → fence → OWN=0 → acquire fence → buffer inval → fence → copy"
    );

    // -- publish of the refilled slot restores the device-owned state -------
    env.ops.clear();
    env.reg_writes.clear();
    k3_gmac::raw_refill_rx_desc(&mut env, &l, 3).unwrap();
    assert_eq!(desc_word(&arena, &env, 3, 3), k3_gmac::RDES3_OWN | k3_gmac::RDES3_BUF1V);
}

/// Backing lifetime: before the arena is published a drop releases the
/// pages; after the publish point the backing is retained until external
/// reset — software reset/status must never deallocate device-owned storage.
#[test]
fn fatal_keeps_backing() {
    // -- pre-publish: drop releases -----------------------------------------
    let (va, pa, _arena) = fresh_arena();
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let guard = ArenaGuard::new(TrackingBacking {
            arena_va: va,
            arena_pa: pa,
            dropped: flag.clone(),
        });
        assert!(!guard.is_published());
        assert_eq!(guard.base_va(), va);
        assert_eq!(guard.base_pa(), pa);
    }
    assert!(
        flag.load(std::sync::atomic::Ordering::SeqCst),
        "pre-publish drop must release the backing"
    );

    // -- post-publish: drop retains ------------------------------------------
    let (va, pa, _arena) = fresh_arena();
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut guard = ArenaGuard::new(TrackingBacking {
        arena_va: va,
        arena_pa: pa,
        dropped: flag.clone(),
    });
    guard.publish();
    assert!(guard.is_published());
    drop(guard);
    assert!(
        !flag.load(std::sync::atomic::Ordering::SeqCst),
        "post-publish drop must retain the backing until external reset"
    );
}

// ---------------------------------------------------------------------------
// Tests — task 1.2 witness list
// ---------------------------------------------------------------------------

/// Plain (non-witnessed) backing for tests that never drop the driver.
struct TestBacking {
    arena_va: usize,
    arena_pa: u64,
}

impl ArenaBacking for TestBacking {
    fn base_va(&self) -> usize {
        self.arena_va
    }
    fn base_pa(&self) -> u64 {
        self.arena_pa
    }
}

/// Simulates one hardware-side frame completion: OWN cleared, FD|LD set, ES
/// clear, raw length, payload bytes visible in the buffer.
fn hw_complete(arena: &mut [u8; ARENA_BYTES], slot: usize, raw_len: u32, payload: &[u8]) {
    set_desc_word(arena, slot, 3, RDES3_FD | RDES3_LD | raw_len);
    set_buf_bytes(arena, slot, payload);
}

/// Happy-path init over a fresh env: SWR self-clears, PHY link/AN up,
/// 1000FD common ability, W48 address capability (the FakeEnv default).
fn init_ok() -> (Gmac<TestBacking>, FakeEnv, &'static mut [u8; ARENA_BYTES]) {
    let (mut env, slice) = FakeEnv::new();
    env.set_phy_link_1000fd();
    let guard = ArenaGuard::new(TestBacking { arena_va: env.arena_va, arena_pa: env.arena_pa });
    let mut g = Gmac::new(guard).expect("fresh arena layout must pass");
    g.init_rx(&mut env).expect("happy-path init");
    (g, env, slice)
}

/// The controller reset failed: no ring/start write, nothing enabled, and a
/// drop at this point releases the backing (pre-publish).
#[test]
fn reset_timeout_no_start() {
    let (mut env, _arena) = FakeEnv::new();
    env.set_phy_link_1000fd();
    env.swr_stuck = true;
    env.access_cost_ns = 100_000_000; // 100 ms per access: 1 s in ~10 polls
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let guard = ArenaGuard::new(TrackingBacking {
        arena_va: env.arena_va,
        arena_pa: env.arena_pa,
        dropped: flag.clone(),
    });
    let mut g = Gmac::new(guard).expect("layout");
    match g.init_rx(&mut env) {
        Err(GmacError::ResetTimeout { raw }) => assert_eq!(raw, 1, "raw SWR bit reported"),
        other => panic!("expected ResetTimeout, got {other:?}"),
    }
    assert_eq!(
        env.reg_write_offsets(),
        vec![0x1000],
        "only the SWR trigger was written; no ring/start/enable write"
    );
    assert_eq!(env.rx_ring_writes(), 0);
    drop(g);
    assert!(
        flag.load(std::sync::atomic::Ordering::SeqCst),
        "pre-publish drop must release the backing"
    );
}

/// A 32-bit-only controller capability refuses this board's >4 GiB arena
/// before any register write at all (no truncation).
#[test]
fn init_rejects_32bit_before_any_write() {
    let (mut env, _arena) = FakeEnv::new();
    env.set_phy_link_1000fd();
    env.regs.insert(0x120, 0 << 14); // W32 capability
    let guard = ArenaGuard::new(TestBacking { arena_va: env.arena_va, arena_pa: env.arena_pa });
    let mut g = Gmac::new(guard).expect("layout");
    match g.init_rx(&mut env) {
        Err(GmacError::Address(DmaAddressError::AddressTooWide { width_bits: 32, .. })) => {}
        other => panic!("expected AddressTooWide(W32), got {other:?}"),
    }
    assert!(env.reg_writes.is_empty(), "no register write before the capability check");
    assert!(env.probe_writes.is_empty(), "no APMU write before the capability check");
}

/// Link/peer phase: bounded 5 s wait, no-common-ability stop with the raw
/// registers, and the ability→MAC-config mapping (1000 priority).
#[test]
fn link_and_peer_deadlines() {
    // -- link never comes up: bounded 5 s wait, then stop ------------------
    let (mut env, _arena) = FakeEnv::new();
    env.mdio_regs[1] = 0x0008 | BMSR_ANEG_COMPLETE; // AN done but link down
    env.access_cost_ns = 100_000_000;
    let guard = ArenaGuard::new(TestBacking { arena_va: env.arena_va, arena_pa: env.arena_pa });
    let mut g = Gmac::new(guard).unwrap();
    match g.init_rx(&mut env) {
        Err(GmacError::LinkTimeout { bmsr2 }) => {
            assert_eq!(bmsr2 & 0x0004, 0, "the last level read had link down")
        }
        other => panic!("expected LinkTimeout, got {other:?}"),
    }
    assert!(env.clock_ns >= 5_000_000_000, "the full 5 s window elapsed");
    assert_eq!(env.rx_ring_writes(), 0, "no ring write without a link");

    // -- link up but no common ability: stop with the raw registers --------
    let (mut env, _arena) = FakeEnv::new();
    env.mdio_regs[1] = 0x0004 | BMSR_ANEG_COMPLETE;
    env.mdio_regs[4] = 0x01e1;
    env.mdio_regs[5] = 0x0001; // peer advertises only the selector
    env.mdio_regs[9] = 0x0300;
    env.mdio_regs[10] = 0x0000;
    let guard = ArenaGuard::new(TestBacking { arena_va: env.arena_va, arena_pa: env.arena_pa });
    let mut g = Gmac::new(guard).unwrap();
    match g.init_rx(&mut env) {
        Err(GmacError::NoCommonAbility { ana, lpa, ctrl1000, stat1000 }) => {
            assert_eq!((ana, lpa, ctrl1000, stat1000), (0x01e1, 0x0001, 0x0300, 0x0000));
        }
        other => panic!("expected NoCommonAbility, got {other:?}"),
    }
    assert_eq!(env.rx_ring_writes(), 0);

    // -- 1000FD priority: MAC config RE|DM, PS/FES clear --------------------
    let (mut env, _arena) = FakeEnv::new();
    env.set_phy_link_1000fd();
    let guard = ArenaGuard::new(TestBacking { arena_va: env.arena_va, arena_pa: env.arena_pa });
    let mut g = Gmac::new(guard).unwrap();
    let link = g.init_rx(&mut env).expect("1000FD init");
    assert_eq!(format!("{link:?}"), "LinkConfig { speed: S1000, full_duplex: true }");
    let cfg = env
        .reg_writes
        .iter()
        .find(|(off, _)| *off == MAC_CONFIG)
        .map(|(_, v)| *v)
        .expect("MAC config written");
    assert_eq!(cfg, MAC_CONFIG_RE | k3_gmac::MAC_CONFIG_DM, "1000FD: RE|DM only");

    // -- 100HD common: MAC config RE|FES|PS, DM clear -----------------------
    let (mut env, _arena) = FakeEnv::new();
    env.set_phy_link_100hd();
    let guard = ArenaGuard::new(TestBacking { arena_va: env.arena_va, arena_pa: env.arena_pa });
    let mut g = Gmac::new(guard).unwrap();
    let link = g.init_rx(&mut env).expect("100HD init");
    assert_eq!(format!("{link:?}"), "LinkConfig { speed: S100, full_duplex: false }");
    let cfg = env
        .reg_writes
        .iter()
        .find(|(off, _)| *off == MAC_CONFIG)
        .map(|(_, v)| *v)
        .expect("MAC config written");
    assert_eq!(
        cfg,
        MAC_CONFIG_RE | k3_gmac::MAC_CONFIG_FES | k3_gmac::MAC_CONFIG_PS,
        "100HD: RE|FES|PS"
    );
}

/// The full init publish: APMU delay RMW, the exact controller config
/// sequence, the cleared-and-synced rings, ring bases (high word first),
/// MAC address, filter, and the 16-slot refill walk (tail + SR per slot).
#[test]
fn rx_publish_and_reap() {
    let (mut g, mut env, mut arena) = init_ok();

    // APMU delay RMW: only the TX/RX enable+phase fields changed.
    assert_eq!(
        env.apmu_delay,
        (0x47 << 24) | (1 << 16) | (0x53 << 8) | 1,
        "delay RMW result: TX 0x47 enabled, RX 0x53 enabled"
    );
    assert_eq!(
        env.probe_writes.iter().filter(|(p, _)| *p == APMU_EBUS_DELAY_PADDR).count(),
        1,
        "exactly one APMU delay write"
    );

    // Register config order between the SWR trigger and the refill walk.
    let offsets: Vec<u32> = env.reg_write_offsets();
    let expected_prefix: Vec<u32> = vec![
        0x1000, // SWR
        k3_gmac::DMA_SYS_BUS_MODE,
        k3_gmac::DMA_CHAN_CTL,
        k3_gmac::DMA_TX_CTL,
        DMA_RX_CTL,
        DMA_CHAN_IRQ_EN,
        k3_gmac::MTL_TXQ0_OP,
        k3_gmac::MTL_RXQ0_OP,
        // ring clear happens in memory (no writes)
        k3_gmac::DMA_TX_DESC_HL,
        k3_gmac::DMA_TX_DESC_LL,
        k3_gmac::DMA_RX_DESC_HL,
        k3_gmac::DMA_RX_DESC_LL,
        k3_gmac::DMA_TX_RINGLEN,
        k3_gmac::DMA_RX_RINGLEN,
        k3_gmac::MAC_ADDR0_LOW,
        k3_gmac::MAC_ADDR0_HIGH,
        k3_gmac::MAC_RXQ0_MAP,
        k3_gmac::MAC_PKT_FILTER,
        MAC_CONFIG,
        DMA_RX_CTL, // RX SR start
    ];
    assert_eq!(
        &offsets[..expected_prefix.len()],
        &expected_prefix[..],
        "config sequence: {offsets:?}"
    );
    // Ring bases: high word first, both carry the full PA split.
    assert_eq!(env.regs[&k3_gmac::DMA_RX_DESC_HL], addr_hi32(0x2_0000_0000));
    assert_eq!(env.regs[&k3_gmac::DMA_RX_DESC_LL], addr_lo32(0x2_0000_0000));
    assert_eq!(env.regs[&k3_gmac::DMA_TX_DESC_HL], addr_hi32(0x2_0000_0000 + TX_RING_OFF as u64));
    // MAC address 02:00:00:00:26:01, LOW before HIGH with AE.
    assert_eq!(env.regs[&k3_gmac::MAC_ADDR0_LOW], 0x0000_0002);
    assert_eq!(env.regs[&k3_gmac::MAC_ADDR0_HIGH], 0x8000_0126);
    assert_eq!(env.regs[&k3_gmac::MAC_RXQ0_MAP], k3_gmac::MAC_RXQ0_MAP_CH0);
    assert_eq!(env.regs[&k3_gmac::MAC_PKT_FILTER], k3_gmac::MAC_FILTER_PR, "PR set");
    assert_eq!(env.regs[&DMA_CHAN_IRQ_EN], 0, "DMA IRQ stays masked");

    // Refill walk: 16 tails + 16 SR rewrites; every slot device-owned.
    let tails: Vec<(u32, u32)> = env
        .reg_writes
        .iter()
        .filter(|(off, _)| *off == RX_TAIL_OFF)
        .copied()
        .collect();
    assert_eq!(tails.len(), 16, "one tail write per slot");
    for (slot, (_, v)) in tails.iter().enumerate() {
        let desc_pa = 0x2_0000_0000 + (slot * DESC_BYTES) as u64;
        assert_eq!(*v, addr_lo32(desc_pa), "tail {slot} points at its own descriptor");
    }
    assert_eq!(
        env.reg_writes.iter().filter(|(off, _)| *off == DMA_RX_CTL).count(),
        18,
        "config RX_CTL + RX start + 16 refill SR rewrites"
    );
    for slot in 0..NUM_SLOTS {
        assert_eq!(desc_word(&arena, &env, slot, 3), RDES3_OWN | k3_gmac::RDES3_BUF1V);
        assert_eq!(
            desc_word(&arena, &env, slot, 0),
            addr_lo32(0x2_0000_0000 + RX_BUF_OFF as u64 + (slot * BUFFER_BYTES) as u64)
        );
    }

    // Reap: the fake hardware completed the head slot 0 (in-order ring).
    hw_complete(&mut arena, 0, 70, &[0xA5u8; 66]);
    env.ops.clear();
    env.reg_writes.clear();
    let mut scratch = [0u8; 1514];
    match g.poll_rx(&mut env, &mut scratch).expect("poll") {
        RxPoll::Frame { len, slot, raw_w3 } => {
            assert_eq!((len, slot), (66, 0));
            assert_eq!(raw_w3, RDES3_FD | RDES3_LD | 70);
            assert_eq!(scratch_bytes(&scratch, 66), vec![0xA5u8; 66]);
        }
        other => panic!("expected Frame, got {other:?}"),
    }
    assert_eq!(
        env.ops,
        vec![
            "inval", "fence_rw", // descriptor before OWN read
            "fence_rw",          // acquire
            "inval", "fence_rw", // buffer before copy
            // refill of slot 3:
            "fence_rw", "flush", "fence_rw", "fence_rw", "clean", "fence_iorw",
        ],
        "poll: reap sequence then the publish sequence"
    );
    assert_eq!(
        env.reg_write_offsets(),
        vec![RX_TAIL_OFF, DMA_RX_CTL],
        "refill doorbell then the SR rewrite"
    );
    assert_eq!(desc_word(&arena, &env, 0, 3), RDES3_OWN | k3_gmac::RDES3_BUF1V, "slot re-owned");
}

/// After the copy the slot is flushed, re-owned and immediately reusable by
/// the device; the buffer PA is unchanged across the refill cycle.
#[test]
fn rx_refill_after_copy() {
    let (mut g, mut env, mut arena) = init_ok();
    let buf0_before = desc_word(&arena, &env, 0, 0);
    hw_complete(&mut arena, 0, 70, &[0x11u8; 66]);
    let mut scratch = [0u8; 1514];
    assert!(matches!(
        g.poll_rx(&mut env, &mut scratch).expect("first poll"),
        RxPoll::Frame { len: 66, slot: 0, .. }
    ));
    assert_eq!(desc_word(&arena, &env, 0, 0), buf0_before, "same buffer PA");
    assert_eq!(desc_word(&arena, &env, 0, 3), RDES3_OWN | k3_gmac::RDES3_BUF1V);
    // The device fills slots in ring order: slot 1 completes next, and a
    // much later completion wraps back into the reused slot 0.
    hw_complete(&mut arena, 1, 74, &[0x22u8; 70]);
    assert!(matches!(
        g.poll_rx(&mut env, &mut scratch).expect("second poll"),
        RxPoll::Frame { len: 70, slot: 1, .. }
    ));
    assert_eq!(scratch_bytes(&scratch, 70), vec![0x22u8; 70], "fresh content copied");
}

/// Every malformed completion shape is classified and never delivered; the
/// slot is still refilled after each drop.
#[test]
fn rx_status_and_length_edges() {
    let (mut g, mut env, mut arena) = init_ok();
    let mut scratch = [0u8; 1514];

    let cases: &[(u32, RxDrop)] = &[
        (RDES3_ES | RDES3_FD | RDES3_LD | 70, RxDrop::ErrorSummary),
        (RDES3_CTXT | RDES3_FD | RDES3_LD | 70, RxDrop::ContextDescriptor),
        (RDES3_FD | 70, RxDrop::NotComplete { fd: true, ld: false }),
        (RDES3_LD | 70, RxDrop::NotComplete { fd: false, ld: true }),
        (RDES3_FD | RDES3_LD | 63, RxDrop::LengthOutOfBounds { len: 63 }),
        (RDES3_FD | RDES3_LD | 1519, RxDrop::LengthOutOfBounds { len: 1519 }),
    ];
    for (i, (w3, reason)) in cases.iter().enumerate() {
        let slot = i % NUM_SLOTS;
        set_desc_word(&mut arena, slot, 3, *w3);
        match g.poll_rx(&mut env, &mut scratch).expect("poll") {
            RxPoll::Dropped { slot: s, reason: r, raw_w3 } => {
                assert_eq!(s, slot);
                assert_eq!(&r, reason, "case {i}");
                assert_eq!(raw_w3, *w3, "raw status retained");
            }
            other => panic!("case {i}: expected Dropped, got {other:?}"),
        }
        assert_eq!(
            desc_word(&arena, &env, slot, 3),
            RDES3_OWN | k3_gmac::RDES3_BUF1V,
            "case {i}: slot refilled"
        );
    }

    // Legal boundaries: raw 64 → 60 bytes, raw 1518 → 1514 bytes.
    hw_complete(&mut arena, 6, 64, &[0x33u8; 60]);
    assert!(matches!(
        g.poll_rx(&mut env, &mut scratch).expect("min frame"),
        RxPoll::Frame { len: 60, slot: 6, .. }
    ));
    hw_complete(&mut arena, 7, 1518, &[0x44u8; 1514]);
    assert!(matches!(
        g.poll_rx(&mut env, &mut scratch).expect("max frame"),
        RxPoll::Frame { len: 1514, slot: 7, .. }
    ));
}

/// In-order consumption across the 16-slot ring and wrap-around; the RBU
/// capacity event is consumed W1C-style and the refill re-doors the tail.
#[test]
fn rx_full_wrap_resume() {
    let (mut g, mut env, mut arena) = init_ok();
    let mut scratch = [0u8; 1514];

    // 16 in-order completions: slots 0..=15, head wraps at 16.
    for i in 0..NUM_SLOTS {
        hw_complete(&mut arena, i, 70, &[i as u8; 66]);
        match g.poll_rx(&mut env, &mut scratch).expect("poll") {
            RxPoll::Frame { slot, .. } => assert_eq!(slot, i),
            other => panic!("poll {i}: expected Frame, got {other:?}"),
        }
    }
    // Wrap: the next completions land on slots 0..=3 again (20 frames total).
    for i in 0..4 {
        hw_complete(&mut arena, i, 70, &[(i + 16) as u8; 66]);
        match g.poll_rx(&mut env, &mut scratch).expect("wrapped poll") {
            RxPoll::Frame { slot, len, .. } => {
                assert_eq!(slot, i);
                assert_eq!(scratch_bytes(&scratch, len)[0], (i + 16) as u8, "no stale content");
            }
            other => panic!("wrapped poll {i}: expected Frame, got {other:?}"),
        }
    }

    // RBU capacity event: W1C only the consumed bits; refill re-doors.
    env.regs.insert(DMA_CHAN_STATUS, DMA_STATUS_RBU | DMA_STATUS_RI);
    hw_complete(&mut arena, 4, 70, &[0x55u8; 66]);
    env.reg_writes.clear();
    assert!(matches!(
        g.poll_rx(&mut env, &mut scratch).expect("RBU poll"),
        RxPoll::Frame { slot: 4, .. }
    ));
    assert_eq!(
        env.reg_writes.iter().find(|(off, _)| *off == DMA_CHAN_STATUS),
        Some(&(DMA_CHAN_STATUS, DMA_STATUS_RBU | DMA_STATUS_RI)),
        "W1C writes exactly the consumed cause bits"
    );
    assert!(
        env.reg_writes.iter().any(|(off, _)| *off == RX_TAIL_OFF),
        "refill re-doores the tail after the capacity event"
    );

    // An empty ring is idle, never a timeout.
    env.regs.remove(&DMA_CHAN_STATUS);
    assert!(matches!(g.poll_rx(&mut env, &mut scratch).expect("idle"), RxPoll::Idle));
}

/// Phase deadlines are observed separately (SWR, link, refill handoff,
/// reclaim); a deadline hit before the ownership publish revokes the
/// handoff and preserves the owner.
#[test]
fn phase_deadlines_preserve_owner() {
    // -- refill handoff deadline during init: nothing published -------------
    let (mut env, _arena) = FakeEnv::new();
    env.set_phy_link_1000fd();
    env.cache_cost_ns = 11_000_000; // each cache op alone exceeds the 10 ms phase
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let guard = ArenaGuard::new(TrackingBacking {
        arena_va: env.arena_va,
        arena_pa: env.arena_pa,
        dropped: flag.clone(),
    });
    let mut g = Gmac::new(guard).unwrap();
    match g.init_rx(&mut env) {
        Err(GmacError::PhaseTimeout { phase: Phase::Refill }) => {}
        other => panic!("expected PhaseTimeout(Refill), got {other:?}"),
    }
    // The deadline hit before the first OWN publish: no tail write, the
    // first slot stayed CPU-owned, and the backing is now retained.
    assert!(!env.reg_writes.iter().any(|(off, _)| *off == RX_TAIL_OFF), "no doorbell");
    drop(g);
    assert!(
        !flag.load(std::sync::atomic::Ordering::SeqCst),
        "post-publish-point backing is retained even though init failed"
    );

    // -- CPU reclaim deadline during poll: copy kept, no new handoff --------
    let (mut g, mut env, mut arena) = init_ok();
    hw_complete(&mut arena, 0, 70, &[0x66u8; 66]);
    env.reg_writes.clear(); // observe only this poll's writes
    env.cache_cost_ns = 11_000_000; // the reap alone exceeds the 10 ms reclaim bound
    let mut scratch = [0u8; 1514];
    match g.poll_rx(&mut env, &mut scratch) {
        Err(GmacError::PhaseTimeout { phase: Phase::Reclaim }) => {}
        other => panic!("expected PhaseTimeout(Refill), got {other:?}"),
    }
    assert_eq!(scratch_bytes(&scratch, 66), vec![0x66u8; 66], "the frame was already copied");
    assert_eq!(desc_word(&arena, &env, 0, 3) & RDES3_OWN, 0, "no new handoff: slot not re-owned");
    assert!(
        !env.reg_writes.iter().any(|(off, _)| *off == RX_TAIL_OFF),
        "no doorbell for the revoked handoff"
    );
}

/// Cancel semantics: before the ownership publish the handoff is revoked;
/// after it the device keeps the slot and only a bounded stop runs — never
/// a software OWN clear, never a backing release.
#[test]
fn cancel_before_after_publish() {
    // -- pre-publish revoke ---------------------------------------------------
    let (mut g, mut env, mut arena) = init_ok();
    hw_complete(&mut arena, 0, 70, &[0x77u8; 66]);
    g.request_cancel();
    env.reg_writes.clear(); // observe only this poll's writes
    let mut scratch = [0u8; 1514];
    match g.poll_rx(&mut env, &mut scratch) {
        Err(GmacError::Cancelled { phase: Phase::Refill, after_publish: false }) => {}
        other => panic!("expected Cancelled(Refill, pre-publish), got {other:?}"),
    }
    assert_eq!(scratch_bytes(&scratch, 66), vec![0x77u8; 66], "CPU-owned frame delivered");
    assert_eq!(desc_word(&arena, &env, 0, 3) & RDES3_OWN, 0, "handoff revoked before OWN");
    assert!(
        !env.reg_writes.iter().any(|(off, _)| *off == RX_TAIL_OFF),
        "no doorbell for the revoked handoff"
    );

    // -- post-publish preserve ------------------------------------------------
    let (mut g, mut env, arena) = init_ok();
    assert!(matches!(g.poll_rx(&mut env, &mut scratch2()).expect("idle"), RxPoll::Idle));
    g.request_cancel();
    match g.poll_rx(&mut env, &mut scratch2()) {
        Err(GmacError::Cancelled { phase: Phase::Poll, after_publish: true }) => {}
        other => panic!("expected Cancelled(Poll, post-publish), got {other:?}"),
    }
    // Device-owned slots are untouched: OWN stays set, no software clear.
    for slot in 0..NUM_SLOTS {
        assert_eq!(desc_word(&arena, &env, slot, 3) & RDES3_OWN, RDES3_OWN);
    }
    // Bounded stop: RE cleared, SR cleared, DMA IRQ masked.
    let stop: Vec<(u32, u32)> = env.reg_writes[
        env.reg_writes.len() - 3..
    ]
    .to_vec();
    assert_eq!(
        stop,
        vec![
            (MAC_CONFIG, k3_gmac::MAC_CONFIG_DM), // RE cleared, DM kept
            (DMA_RX_CTL, k3_gmac::DMA_RX_CTL_RBSZ_2048 | k3_gmac::DMA_RX_CTL_PBL_8), // SR cleared
            (DMA_CHAN_IRQ_EN, 0),
        ],
        "bounded stop writes: {stop:?}"
    );
}

fn scratch2() -> [u8; 1514] {
    [0u8; 1514]
}

// ---------------------------------------------------------------------------
// Tests — task 1.3 witness list (stage 11 foreground)
// ---------------------------------------------------------------------------

use k3_device_irq_probe::Stage10Outcome;

fn st10_window_ended() -> Stage10Outcome {
    Stage10Outcome::WindowEnded(k3_device_irq_probe::Stage10Summary {
        claims: 2,
        aligned_down: 1,
        aligned_up: 1,
        unaligned: 0,
        mdio_errors: 0,
        printed: 2,
        quiet_clean: true,
        quiet_claims: 0,
        mac_events: [0, 0],
        window_elapsed_nanos: 0,
    })
}

/// Builds a minimal padded ARP request (who-has 192.168.137.2 → 60-byte
/// payload, 64 raw with FCS) with `spa`/`tpa` and writes it into slot 0.
fn hw_complete_arp(arena: &mut [u8; ARENA_BYTES], spa: [u8; 4], tpa: [u8; 4]) {
    let mut frame = [0u8; 60];
    frame[12] = 0x08;
    frame[13] = 0x06;
    frame[14] = 0x00;
    frame[15] = 0x01; // htype ethernet
    frame[16] = 0x08;
    frame[17] = 0x00; // ptype IPv4
    frame[18] = 6;
    frame[19] = 4;
    frame[20] = 0;
    frame[21] = 1; // operation: request
    frame[28..32].copy_from_slice(&spa);
    frame[38..42].copy_from_slice(&tpa);
    set_buf_bytes(arena, 0, &frame);
    set_desc_word(arena, 0, 3, RDES3_FD | RDES3_LD | 64);
}

/// Non-`WindowEnded` stage 10 outcomes never open stage 11: no mask, no
/// register, no MDIO, no cache access happens at all.
#[test]
fn stage11_gate_and_masks_before_mdio() {
    for st10 in [
        Stage10Outcome::Stopped { reason: "snapshot-capacity-exceeded" },
        Stage10Outcome::SetupFailed { phase: "mdio" },
    ] {
        let (mut env, _slice) = FakeEnv::new();
        env.set_phy_link_1000fd();
        let guard = ArenaGuard::new(TestBacking { arena_va: env.arena_va, arena_pa: env.arena_pa });
        let mut lines = Vec::new();
        let outcome = k3_network_probe::run_stage11(&mut env, st10, guard, &mut |l| lines.push(l.to_string()));
        assert!(
            matches!(outcome, k3_network_probe::Stage11Outcome::SetupFailed { phase: "gate" }),
            "{outcome:?}"
        );
        assert!(env.access_log.is_empty(), "no board access behind the gate");
        assert!(lines.iter().any(|l| l.contains("err=gate")), "{lines:?}");
    }

    // A normal window end opens the stage: the three routes are masked as
    // the very first board accesses, before any register/MDIO/cache touch.
    let (mut env, _slice) = FakeEnv::new();
    env.set_phy_link_1000fd();
    let guard = ArenaGuard::new(TestBacking { arena_va: env.arena_va, arena_pa: env.arena_pa });
    let mut lines = Vec::new();
    let outcome = k3_network_probe::run_stage11(&mut env, st10_window_ended(), guard, &mut |l| lines.push(l.to_string()));
    assert!(matches!(outcome, k3_network_probe::Stage11Outcome::WindowEnded(_)), "{outcome:?}");
    let masks: Vec<&&str> = env.access_log.iter().take(3).collect();
    assert!(
        masks.contains(&&"mask:60") && masks.contains(&&"mask:133") && masks.contains(&&"mask:277"),
        "first three accesses are the route masks: {masks:?}"
    );
    assert!(
        !masks.contains(&&"reg") && !masks.contains(&&"mdio") && !masks.contains(&&"cache"),
        "no foreground owner access precedes the masks: {masks:?}"
    );
    assert!(lines.iter().any(|l| l.contains("stage=11 init")), "{lines:?}");
}

/// The bounded 30 s window: idle polling stays bounded, a real ARP request
/// is printed once as a raw sample, the window ends with a summary, and an
/// FBE fault ends the stage bounded without touching device-owned state.
#[test]
fn stage11_bounded_rx_observation() {
    // -- idle window runs its fixed length and reports ----------------------
    let (mut env, _slice) = FakeEnv::new();
    env.set_phy_link_1000fd();
    let guard = ArenaGuard::new(TestBacking { arena_va: env.arena_va, arena_pa: env.arena_pa });
    let mut lines = Vec::new();
    let outcome = k3_network_probe::run_stage11(&mut env, st10_window_ended(), guard, &mut |l| lines.push(l.to_string()));
    match outcome {
        k3_network_probe::Stage11Outcome::WindowEnded(s) => {
            assert_eq!(s.frames, 0);
            assert_eq!(s.drops, 0);
            assert!(s.window_elapsed_nanos >= k3_network_probe::STAGE11_WINDOW_NANOS);
        }
        other => panic!("expected WindowEnded, got {other:?}"),
    }
    assert!(lines.iter().any(|l| l.contains("result frames=0")), "{lines:?}");

    // -- one real ARP request: sampled raw once, counted in the summary ------
    let (mut env, _arena) = FakeEnv::new();
    env.set_phy_link_1000fd();
    // The device completes the head frame during the window (after the ring
    // is published — a pre-init write would be wiped by the ring clear).
    env.pending_arp = Some(([192, 168, 137, 1], [192, 168, 137, 2]));
    let guard = ArenaGuard::new(TestBacking { arena_va: env.arena_va, arena_pa: env.arena_pa });
    let mut lines = Vec::new();
    let outcome = k3_network_probe::run_stage11(&mut env, st10_window_ended(), guard, &mut |l| lines.push(l.to_string()));
    match outcome {
        k3_network_probe::Stage11Outcome::WindowEnded(s) => {
            assert_eq!(s.frames, 1, "{lines:?}");
            assert!(s.arp_sample_printed, "{lines:?}");
        }
        other => panic!("expected WindowEnded, got {other:?}"),
    }
    let sample = lines
        .iter()
        .find(|l| l.contains("arp-req slot=0 len=60"))
        .expect("raw ARP sample line");
    assert!(sample.contains("raw="), "descriptor status retained: {sample}");
    assert!(
        sample.contains("0806") && sample.contains("c0a88901") && sample.contains("c0a88902"),
        "ethertype plus SPA 192.168.137.1 and TPA 192.168.137.2 in the raw bytes: {sample}"
    );
    let result_line = lines.iter().find(|l| l.contains("stage=11 result")).expect("summary line");
    assert!(result_line.contains("frames=1"), "{result_line}");
    assert!(result_line.contains("arp-sample=1"), "{result_line}");

    // -- drop sample and FBE fault end the stage bounded ----------------------
    let (mut env, mut arena) = FakeEnv::new();
    env.set_phy_link_1000fd();
    set_desc_word(&mut arena, 0, 3, RDES3_ES | RDES3_FD | RDES3_LD | 70);
    env.regs.insert(DMA_CHAN_STATUS, k3_gmac::DMA_STATUS_FBE);
    let guard = ArenaGuard::new(TestBacking { arena_va: env.arena_va, arena_pa: env.arena_pa });
    let mut lines = Vec::new();
    // The FBE is observed on the first poll: bounded fault, no hang.
    let outcome = k3_network_probe::run_stage11(&mut env, st10_window_ended(), guard, &mut |l| lines.push(l.to_string()));
    assert!(
        matches!(outcome, k3_network_probe::Stage11Outcome::Fault { .. }),
        "{outcome:?}"
    );
    assert!(lines.iter().any(|l| l.contains("err=fbe")), "{lines:?}");
}

/// Init-failure branches of the backing lifetime: a failure after the
/// publish point (link wait) retains the backing; a forced fault retains it
/// through the stop; only pre-publish failures release.
#[test]
fn fatal_keeps_backing_on_failure_paths() {
    // -- link failure after publish: retained ---------------------------------
    let (mut env, _arena) = FakeEnv::new();
    env.mdio_regs[1] = 0x0008 | BMSR_ANEG_COMPLETE; // never link
    env.access_cost_ns = 100_000_000;
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let guard = ArenaGuard::new(TrackingBacking {
        arena_va: env.arena_va,
        arena_pa: env.arena_pa,
        dropped: flag.clone(),
    });
    let mut g = Gmac::new(guard).unwrap();
    assert!(matches!(g.init_rx(&mut env), Err(GmacError::LinkTimeout { .. })));
    drop(g);
    assert!(
        !flag.load(std::sync::atomic::Ordering::SeqCst),
        "post-publish failure must retain the backing"
    );

    // -- FBE fault during poll: retained, state faulted ------------------------
    let (mut env, _slice) = FakeEnv::new();
    env.set_phy_link_1000fd();
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let guard = ArenaGuard::new(TrackingBacking {
        arena_va: env.arena_va,
        arena_pa: env.arena_pa,
        dropped: flag.clone(),
    });
    let mut g = Gmac::new(guard).unwrap();
    g.init_rx(&mut env).expect("init");
    env.regs.insert(DMA_CHAN_STATUS, k3_gmac::DMA_STATUS_FBE);
    match g.poll_rx(&mut env, &mut scratch2()) {
        Err(GmacError::Fault { raw_status, .. }) => {
            assert_eq!(raw_status & k3_gmac::DMA_STATUS_FBE, k3_gmac::DMA_STATUS_FBE);
        }
        other => panic!("expected Fault, got {other:?}"),
    }
    assert!(matches!(g.state(), GmacState::Faulted { .. }));
    drop(g);
    assert!(
        !flag.load(std::sync::atomic::Ordering::SeqCst),
        "faulted stop must retain the backing"
    );
}

// ---------------------------------------------------------------------------
// Tests — task 2.1 witness list (TX submit, terminal, reclaim)
// ---------------------------------------------------------------------------

/// Happy-path init + TX enable: TE joins the MAC config, ST joins the TX
/// control (RX bits preserved).
fn init_tx_ok() -> (Gmac<TestBacking>, FakeEnv, &'static mut [u8; ARENA_BYTES]) {
    let (mut g, mut env, slice) = init_ok();
    g.enable_tx(&mut env);
    (g, env, slice)
}

/// Length contract: 14..=1514 accepted, below/above refused with the
/// original length and no write at all (no truncation, no trial publish);
/// 14..=59 zero-padded to 60 in the buffer with the padded length in the
/// descriptor (FCS is hardware-generated and excluded).
#[test]
fn tx_length_no_truncate() {
    let (mut g, mut env, arena) = init_tx_ok();
    // enable_tx RMW kept the RX bits and added TE/ST.
    assert_eq!(
        env.regs[&MAC_CONFIG],
        MAC_CONFIG_RE | k3_gmac::MAC_CONFIG_DM | k3_gmac::MAC_CONFIG_TE
    );
    assert_eq!(
        env.regs[&DMA_TX_CTL],
        DMA_TX_CTL_OSP | DMA_TX_CTL_PBL_8 | DMA_TX_CTL_ST
    );

    // Refusals report the original length and never touch the device.
    env.reg_writes.clear();
    match g.try_submit(&mut env, &[0u8; 13]) {
        Err(TxSubmitError::LengthInvalid { len: 13 }) => {}
        other => panic!("expected LengthInvalid(13), got {other:?}"),
    }
    match g.try_submit(&mut env, &[0u8; 1515]) {
        Err(TxSubmitError::LengthInvalid { len: 1515 }) => {}
        other => panic!("expected LengthInvalid(1515), got {other:?}"),
    }
    assert!(env.reg_writes.is_empty(), "rejected lengths write nothing");
    assert_eq!(tx_desc_word(&arena, 0, 3) & TDES3_OWN, 0, "no trial publish");

    // Minimum frame: padded to 60 in the buffer, descriptor len 60.
    let mut short = [0u8; 14];
    for (i, b) in short.iter_mut().enumerate() {
        *b = i as u8;
    }
    match g.try_submit(&mut env, &short) {
        Ok(TxSubmit::Submitted { slot: 0 }) => {}
        other => panic!("expected Submitted(0), got {other:?}"),
    }
    assert_eq!(tx_desc_word(&arena, 0, 0), addr_lo32(0x2_0000_0000 + TX_BUF_OFF as u64));
    assert_eq!(tx_desc_word(&arena, 0, 2), 60 & 0x3fff, "des2 padded len, IOC clear");
    assert_eq!(tx_desc_word(&arena, 0, 3), TDES3_OWN | TDES3_FD | TDES3_LD | 60);
    let off = TX_BUF_OFF;
    assert_eq!(&arena[off..off + 14], &short[..], "frame copied verbatim");
    assert!(arena[off + 14..off + 60].iter().all(|&b| b == 0), "zero-padded to 60");

    // Maximum frame: exactly 1514, no truncation.
    let long = [0x5Au8; 1514];
    match g.try_submit(&mut env, &long) {
        Ok(TxSubmit::Submitted { slot: 1 }) => {}
        other => panic!("expected Submitted(1), got {other:?}"),
    }
    assert_eq!(tx_desc_word(&arena, 1, 3), TDES3_OWN | TDES3_FD | TDES3_LD | 1514);
    let off1 = TX_BUF_OFF + BUFFER_BYTES;
    assert_eq!(&arena[off1..off1 + 1514], &long[..], "full frame in the TX buffer");
}

/// Capacity and ring walk: 16 submits fill the ring, the 17th is Full and
/// touches nothing; freed slots are reused across the wrap and every
/// completion is reclaimed exactly once.
#[test]
fn tx_full_wrap_no_overwrite() {
    let (mut g, mut env, mut arena) = init_tx_ok();
    let frame = [0x11u8; 60];
    for i in 0..NUM_SLOTS {
        match g.try_submit(&mut env, &frame) {
            Ok(TxSubmit::Submitted { slot }) => assert_eq!(slot, i, "FIFO slot allocation"),
            other => panic!("submit {i}: expected Submitted, got {other:?}"),
        }
    }
    // Full: no slot, no descriptor word, no register modified.
    let before: Vec<[u32; 4]> = (0..NUM_SLOTS)
        .map(|s| {
            let mut w = [0u32; 4];
            for (i, x) in w.iter_mut().enumerate() {
                *x = tx_desc_word(&arena, s, i);
            }
            w
        })
        .collect();
    env.reg_writes.clear();
    assert!(matches!(g.try_submit(&mut env, &frame), Ok(TxSubmit::Full)));
    assert!(env.reg_writes.is_empty(), "Full never writes registers");
    for s in 0..NUM_SLOTS {
        for w in 0..4 {
            assert_eq!(
                tx_desc_word(&arena, s, w),
                before[s][w],
                "Full must not touch slot {s} word {w}"
            );
        }
    }
    // Wrap wave: complete the four oldest fill submits and resubmit across
    // the wrap (20 submits total; slots 0..3 reused once each, every
    // completion reclaimed exactly once).
    for i in 0..4 {
        set_tx_desc_word(&mut arena, i, 3, TDES3_FD | TDES3_LD | 60);
        match g.poll_tx(&mut env) {
            Ok(TxPoll::Completed { slot, raw_w3 }) => {
                assert_eq!(slot, i);
                assert_eq!(raw_w3, TDES3_FD | TDES3_LD | 60, "raw status retained");
            }
            other => panic!("reap {i}: expected Completed, got {other:?}"),
        }
        assert!(
            !matches!(g.poll_tx(&mut env), Ok(TxPoll::Completed { slot, .. }) if slot == i),
            "completion {i} is reclaimed exactly once"
        );
        match g.try_submit(&mut env, &frame) {
            Ok(TxSubmit::Submitted { slot }) => assert_eq!(slot, i, "wrapped reuse of slot {i}"),
            other => panic!("wrap submit {i}: expected Submitted, got {other:?}"),
        }
    }
}

/// In-order completion with single-shot reclaim: each completed descriptor
/// is reported once with its raw word, and freed capacity is immediately
/// reusable.
#[test]
fn tx_complete_once() {
    let (mut g, mut env, mut arena) = init_tx_ok();
    let f1 = [0xA1u8; 100];
    let f2 = [0xB2u8; 1514];
    assert!(matches!(g.try_submit(&mut env, &f1), Ok(TxSubmit::Submitted { slot: 0 })));
    assert!(matches!(g.try_submit(&mut env, &f2), Ok(TxSubmit::Submitted { slot: 1 })));
    set_tx_desc_word(&mut arena, 0, 3, TDES3_FD | TDES3_LD | 100);
    set_tx_desc_word(&mut arena, 1, 3, TDES3_FD | TDES3_LD | 1514);
    match g.poll_tx(&mut env) {
        Ok(TxPoll::Completed { slot: 0, raw_w3 }) => {
            assert_eq!(raw_w3, TDES3_FD | TDES3_LD | 100)
        }
        other => panic!("expected Completed(0), got {other:?}"),
    }
    match g.poll_tx(&mut env) {
        Ok(TxPoll::Completed { slot: 1, raw_w3 }) => {
            assert_eq!(raw_w3, TDES3_FD | TDES3_LD | 1514)
        }
        other => panic!("expected Completed(1), got {other:?}"),
    }
    assert!(matches!(g.poll_tx(&mut env), Ok(TxPoll::Idle)), "ring drained");
    // A freed slot is never re-reclaimed even if its device word stays OWN=0.
    set_tx_desc_word(&mut arena, 0, 3, TDES3_FD | TDES3_LD | 100);
    assert!(matches!(g.poll_tx(&mut env), Ok(TxPoll::Idle)), "no re-reclaim");
    // Capacity restored: the ring walk continues at the wrapped tail.
    assert!(matches!(g.try_submit(&mut env, &f1), Ok(TxSubmit::Submitted { slot: 2 })));
    assert!(matches!(g.try_submit(&mut env, &f2), Ok(TxSubmit::Submitted { slot: 3 })));
}

/// TX deadlines and cancel: the 10 ms submit phase revokes before OWN, a
/// pre-publish cancel revokes at the checkpoint, the 1 s terminal window
/// times out with the owner retained, a post-publish cancel stops new
/// submits without touching device-owned slots, an ES completion is a local
/// error reclaimed once, FBE faults the driver, and the reclaim phase has
/// its own 10 ms bound.
#[test]
fn tx_deadlines_and_cancel() {
    // -- submit deadline: the phase trips before the OWN publish -------------
    let (mut g, mut env, arena) = init_tx_ok();
    env.cache_cost_ns = 11_000_000; // one cache op alone exceeds 10 ms
    env.reg_writes.clear();
    match g.try_submit(&mut env, &[0x44u8; 60]) {
        Err(TxSubmitError::Driver(GmacError::PhaseTimeout { phase: Phase::Submit })) => {}
        other => panic!("expected PhaseTimeout(Submit), got {other:?}"),
    }
    assert_eq!(tx_desc_word(&arena, 0, 3) & TDES3_OWN, 0, "no OWN at the deadline");
    assert!(!env.reg_writes.iter().any(|(off, _)| *off == DMA_TX_TAIL), "no doorbell");

    // -- pre-publish cancel: revoked at the checkpoint, slot stays Free ------
    let (mut g, mut env, arena) = init_tx_ok();
    g.request_cancel();
    env.reg_writes.clear();
    match g.try_submit(&mut env, &[0x44u8; 60]) {
        Err(TxSubmitError::Driver(GmacError::Cancelled {
            phase: Phase::Submit,
            after_publish: false,
        })) => {}
        other => panic!("expected Cancelled(Submit, pre-publish), got {other:?}"),
    }
    assert_eq!(tx_desc_word(&arena, 0, 3) & TDES3_OWN, 0);
    assert!(!env.reg_writes.iter().any(|(off, _)| *off == DMA_TX_TAIL));

    // -- terminal timeout: 1 s window expires with OWN still set -------------
    let (mut g, mut env, arena) = init_tx_ok();
    assert!(matches!(g.try_submit(&mut env, &[0x44u8; 60]), Ok(TxSubmit::Submitted { slot: 0 })));
    env.wait_nanos(TX_TERMINAL_NANOS + 1_000_000);
    match g.poll_tx(&mut env) {
        Ok(TxPoll::TerminalTimeout { slot: 0, raw_w3 }) => {
            assert_eq!(raw_w3 & TDES3_OWN, TDES3_OWN, "raw word is the owned descriptor");
        }
        other => panic!("expected TerminalTimeout(0), got {other:?}"),
    }
    assert_eq!(
        tx_desc_word(&arena, 0, 3) & TDES3_OWN,
        TDES3_OWN,
        "owner retained, no software OWN clear"
    );
    assert!(matches!(g.poll_tx(&mut env), Ok(TxPoll::Idle)), "reported once per submit");
    assert!(
        matches!(g.try_submit(&mut env, &[0x44u8; 60]), Ok(TxSubmit::Submitted { slot: 1 })),
        "fresh slots still accept submits after a timeout"
    );

    // -- post-publish cancel: wait ends, owner kept, new submits refused -----
    let (mut g, mut env, arena) = init_tx_ok();
    assert!(matches!(g.try_submit(&mut env, &[0x44u8; 60]), Ok(TxSubmit::Submitted { slot: 0 })));
    g.request_cancel();
    match g.poll_tx(&mut env) {
        Err(GmacError::Cancelled { phase: Phase::Poll, after_publish: true }) => {}
        other => panic!("expected Cancelled(Poll, post-publish), got {other:?}"),
    }
    assert_eq!(
        tx_desc_word(&arena, 0, 3) & TDES3_OWN,
        TDES3_OWN,
        "device-owned slot untouched by the cancel"
    );
    let stop: Vec<(u32, u32)> = env.reg_writes[env.reg_writes.len() - 3..].to_vec();
    assert_eq!(
        stop,
        vec![
            (MAC_CONFIG, MAC_CONFIG_RE | k3_gmac::MAC_CONFIG_DM), // TE cleared
            (DMA_TX_CTL, DMA_TX_CTL_OSP | DMA_TX_CTL_PBL_8),      // ST cleared
            (DMA_CHAN_IRQ_EN, 0),
        ],
        "TX bounded stop writes: {stop:?}"
    );
    match g.try_submit(&mut env, &[0x44u8; 60]) {
        Err(TxSubmitError::Driver(GmacError::Cancelled {
            phase: Phase::Submit,
            after_publish: true,
        })) => {}
        other => panic!("new submits refused after the stop, got {other:?}"),
    }

    // -- ES device error: local error with raw, slot reclaimed once ----------
    let (mut g, mut env, _arena) = init_tx_ok();
    env.pending_tx_complete = true;
    env.pending_tx_es = true;
    assert!(matches!(g.try_submit(&mut env, &[0x44u8; 60]), Ok(TxSubmit::Submitted { slot: 0 })));
    env.wait_nanos(1_000_000); // the fake completes with ES
    match g.poll_tx(&mut env) {
        Ok(TxPoll::LocalError { slot: 0, raw_w3 }) => {
            assert_eq!(raw_w3 & TDES3_ES, TDES3_ES, "raw ES retained");
            assert_eq!(raw_w3 & TDES3_OWN, 0, "device completed before the error report");
        }
        other => panic!("expected LocalError(0), got {other:?}"),
    }
    assert!(matches!(g.poll_tx(&mut env), Ok(TxPoll::Idle)), "single reclaim");
    assert!(
        matches!(g.try_submit(&mut env, &[0x44u8; 60]), Ok(TxSubmit::Submitted { slot: 1 })),
        "capacity restored after a local error"
    );

    // -- FBE: fatal bus error faults the driver, refuses submit and poll -----
    let (mut env, _slice) = FakeEnv::new();
    env.set_phy_link_1000fd();
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let guard = ArenaGuard::new(TrackingBacking {
        arena_va: env.arena_va,
        arena_pa: env.arena_pa,
        dropped: flag.clone(),
    });
    let mut g = Gmac::new(guard).unwrap();
    g.init_rx(&mut env).expect("init");
    g.enable_tx(&mut env);
    env.regs.insert(DMA_CHAN_STATUS, DMA_STATUS_FBE);
    match g.try_submit(&mut env, &[0x44u8; 60]) {
        Err(TxSubmitError::Driver(GmacError::Fault { raw_status })) => {
            assert_eq!(raw_status & DMA_STATUS_FBE, DMA_STATUS_FBE);
        }
        other => panic!("expected Fault on submit, got {other:?}"),
    }
    assert!(matches!(g.poll_tx(&mut env), Err(GmacError::Fault { .. })));
    assert!(matches!(g.state(), GmacState::Faulted { .. }));
    drop(g);
    assert!(
        !flag.load(std::sync::atomic::Ordering::SeqCst),
        "faulted stop must retain the backing"
    );

    // -- reclaim phase: independent 10 ms bound, retry reclaims once ---------
    let (mut g, mut env, _arena) = init_tx_ok();
    env.pending_tx_complete = true;
    assert!(matches!(g.try_submit(&mut env, &[0x44u8; 60]), Ok(TxSubmit::Submitted { slot: 0 })));
    env.wait_nanos(1_000_000); // device completes
    env.cache_cost_ns = 11_000_000;
    match g.poll_tx(&mut env) {
        Err(GmacError::PhaseTimeout { phase: Phase::Reclaim }) => {}
        other => panic!("expected PhaseTimeout(Reclaim), got {other:?}"),
    }
    env.cache_cost_ns = 0;
    assert!(
        matches!(g.poll_tx(&mut env), Ok(TxPoll::Completed { slot: 0, .. })),
        "retry reclaims the observed completion"
    );
}

/// The TX tail doorbell names the next free descriptor: the tail register is
/// an exclusive upper bound (the DMA fetches strictly below it), so each
/// submit writes addr((i+1) mod 16) — submitting slot 15 doors addr(0).
#[test]
fn tx_tail_doorbell_names_next_slot() {
    let (mut g, mut env, _arena) = init_tx_ok();
    let frame = [0x11u8; 60];
    for i in 0..NUM_SLOTS {
        env.reg_writes.clear();
        assert!(matches!(
            g.try_submit(&mut env, &frame),
            Ok(TxSubmit::Submitted { slot }) if slot == i
        ));
        let tails: Vec<u32> = env
            .reg_writes
            .iter()
            .filter(|(off, _)| *off == DMA_TX_TAIL)
            .map(|(_, v)| *v)
            .collect();
        let expected = addr_lo32(
            0x2_0000_0000 + TX_RING_OFF as u64 + (((i + 1) % NUM_SLOTS) * DESC_BYTES) as u64,
        );
        assert_eq!(
            tails,
            vec![expected],
            "submit {i}: exactly one tail write naming the next free descriptor"
        );
    }
}

/// A reused slot's pad region is truly zeroed: stale bytes from a previous
/// frame must not leak into the padding of a short frame.
#[test]
fn tx_pad_zeroed_on_reuse() {
    let (mut g, mut env, arena) = init_tx_ok();
    let off = TX_BUF_OFF;
    // A previous frame left stale bytes across the whole pad region.
    for b in arena[off..off + k3_gmac::TX_PAD_TO].iter_mut() {
        *b = 0xA5;
    }
    let short = [0x5Eu8; 14];
    assert!(matches!(
        g.try_submit(&mut env, &short),
        Ok(TxSubmit::Submitted { slot: 0 })
    ));
    assert_eq!(&arena[off..off + 14], &short[..], "frame copied verbatim");
    assert!(
        arena[off + 14..off + k3_gmac::TX_PAD_TO].iter().all(|&b| b == 0),
        "stale pad bytes zeroed on the submit path"
    );
}

// ---------------------------------------------------------------------------
// Tests — task 2.2 witness list (stage 12 raw bidirectional frames)
// ---------------------------------------------------------------------------

/// Stage 12 gate and bounded sequence: a non-`WindowEnded` stage 11 never
/// touches the board; the happy path sends 20 submits 100 ms apart with
/// per-submit terminals, covers ≥16 slots across the wrap, runs the bounded
/// 10 s RX window, reports the peer reply separately from local terminals
/// and retains the published backing.
#[test]
fn raw_tx_sequence_requires_rx_and_bounds_wait() {
    use k3_network_probe::{Stage11Outcome, Stage11Summary, Stage12Outcome};

    // -- gate: stage 11 must have ended its window normally -------------------
    for st11 in [
        Stage11Outcome::SetupFailed { phase: "init" },
        Stage11Outcome::Stopped { reason: "deadline" },
    ] {
        let (mut env, _slice) = FakeEnv::new();
        env.set_phy_link_1000fd();
        let guard = ArenaGuard::new(TestBacking { arena_va: env.arena_va, arena_pa: env.arena_pa });
        let mut lines = Vec::new();
        let outcome =
            k3_network_probe::run_stage12(&mut env, st11, guard, &mut |l| lines.push(l.to_string()));
        assert!(
            matches!(outcome, Stage12Outcome::SetupFailed { phase: "gate" }),
            "{outcome:?}"
        );
        assert!(env.access_log.is_empty(), "stage12 behind a failed gate never touches the board");
        assert!(lines.iter().any(|l| l.contains("stage=12 err=gate")), "{lines:?}");
    }

    // -- happy path: 20 submits, wrap coverage, bounded windows ---------------
    let (mut env, _slice) = FakeEnv::new();
    env.set_phy_link_1000fd();
    env.pending_tx_complete = true;
    // The PC answers with a real is-at reply (oper=2) for 192.168.137.2.
    env.pending_arp_reply = Some(([192, 168, 137, 2], [192, 168, 137, 1]));
    let flag = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let guard = ArenaGuard::new(TrackingBacking {
        arena_va: env.arena_va,
        arena_pa: env.arena_pa,
        dropped: flag.clone(),
    });
    let mut lines = Vec::new();
    let st11 = Stage11Outcome::WindowEnded(Stage11Summary::default());
    let outcome =
        k3_network_probe::run_stage12(&mut env, st11, guard, &mut |l| lines.push(l.to_string()));
    match outcome {
        Stage12Outcome::WindowEnded(s) => {
            assert_eq!(s.sent, 20, "every submit published: {lines:?}");
            assert_eq!(s.terminals, 20, "every submit reached its local terminal");
            assert_eq!(s.tx_errors, 0);
            assert_eq!(s.tx_timeouts, 0);
            assert_eq!(s.not_sent, 0);
            assert!(s.arp_reply_printed, "the peer reply was sampled");
            assert!(s.rx_frames >= 1);
            assert!(
                s.window_elapsed_nanos >= k3_network_probe::STAGE12_RX_WINDOW_NANOS,
                "the 10 s RX window ran to its bound"
            );
        }
        other => panic!("expected WindowEnded, got {other:?}"),
    }
    // Slot coverage: 20 tx lines, ≥16 distinct slots, four reused once.
    let slots: Vec<usize> = lines
        .iter()
        .filter(|l| l.contains("stage=12 tx i="))
        .filter_map(|l| {
            let p = l.find("slot=")?;
            l[p + 5..].split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
        })
        .collect();
    assert_eq!(slots.len(), 20, "one submit line per packet: {lines:?}");
    let mut distinct = slots.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert!(distinct.len() >= 16, "wrap coverage across the ring: {distinct:?}");
    let mut counts = [0usize; NUM_SLOTS];
    for s in &slots {
        counts[*s] += 1;
    }
    assert_eq!(
        counts.iter().filter(|&&c| c >= 2).count(),
        4,
        "four slots reused across the wrap: {counts:?}"
    );
    // Bounded pacing: ≥20 × 100 ms of interval waits plus the 10 s window;
    // per-submit terminals are bounded by the driver's 1 s deadline.
    assert!(
        env.clock_ns >= 20 * 100_000_000 + k3_network_probe::STAGE12_RX_WINDOW_NANOS,
        "interval pacing observed: {}",
        env.clock_ns
    );
    // Summary line separates local terminals from the peer reply evidence.
    let result = lines.iter().find(|l| l.contains("stage=12 result")).expect("summary line");
    assert!(result.contains("sent=20") && result.contains("terminals=20"), "{result}");
    assert!(result.contains("arp-reply=1"), "{result}");
    // The published backing is retained after the stage ends.
    drop(lines);
    assert!(
        !flag.load(std::sync::atomic::Ordering::SeqCst),
        "stage12 retains the published backing until external reset"
    );
}

// ---------------------------------------------------------------------------
// Tests — MS12 Iteration 000 (001-replan) task T2: async IOC publish, the
// source-133 ISR core and the deferred-owner arm/recheck protocol.
//
// The IRQ board fake is a process-global static because the production ISR
// is a bare `fn()`. The tests that drive it serialize on IRQ_TEST_LOCK, so
// the harness stays correct under the default `make host-test` parallelism
// as well as the single-threaded V2 invocation.
// ---------------------------------------------------------------------------

#[path = "../kernel/src/platform/k3_async_net.rs"]
mod k3_async_net;

use std::cell::UnsafeCell;

use k3_async_net::IrqAccess;
use k3_gmac::regs::{
    DMA_IRQ_EN_DEFAULT, DMA_STATUS_AIS, DMA_STATUS_BIT10, DMA_STATUS_NIS, DMA_STATUS_TI,
    DMA_STATUS_TBU,
};
use k3_gmac::{RDES3_BUF1V, RDES3_IOC, TDES2_IOC};

/// Serializes every test that drives the shared IRQ board fake (poisoning
/// is ignored: a failed test must not fail its siblings).
static IRQ_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn irq_test_guard() -> std::sync::MutexGuard<'static, ()> {
    IRQ_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

struct FakeIrqBoard {
    /// GMAC base-relative register file (IRQ-context bare fns need a static).
    regs: UnsafeCell<std::collections::BTreeMap<u32, u32>>,
    writes: UnsafeCell<Vec<(u32, u32)>>,
    publishes: std::sync::atomic::AtomicU64,
}

// SAFETY: single-threaded harness (--test-threads=1); every access goes
// through the bare fns below.
unsafe impl Sync for FakeIrqBoard {}

impl FakeIrqBoard {
    const fn new() -> Self {
        FakeIrqBoard {
            regs: UnsafeCell::new(std::collections::BTreeMap::new()),
            writes: UnsafeCell::new(Vec::new()),
            publishes: std::sync::atomic::AtomicU64::new(0),
        }
    }

    fn reset(&self) {
        // SAFETY: single-threaded harness.
        unsafe {
            (*self.regs.get()).clear();
            (*self.writes.get()).clear();
        }
        self.publishes.store(0, std::sync::atomic::Ordering::SeqCst);
    }

    fn read(&self, off: u32) -> u32 {
        // SAFETY: single-threaded harness.
        unsafe { (*self.regs.get()).get(&off).copied().unwrap_or(0) }
    }

    fn write(&self, off: u32, v: u32) {
        // SAFETY: single-threaded harness.
        unsafe {
            (*self.regs.get()).insert(off, v);
            (*self.writes.get()).push((off, v));
        }
    }

    /// Sets a register WITHOUT recording it: fixture pre-state, not a
    /// handler-produced write.
    fn set_quiet(&self, off: u32, v: u32) {
        // SAFETY: single-threaded harness.
        unsafe {
            (*self.regs.get()).insert(off, v);
        }
    }

    fn writes_snapshot(&self) -> Vec<(u32, u32)> {
        // SAFETY: single-threaded harness.
        unsafe { (*self.writes.get()).clone() }
    }
}

static IRQ_BOARD: FakeIrqBoard = FakeIrqBoard::new();

fn irq_reg_read(off: u32) -> u32 {
    IRQ_BOARD.read(off)
}

fn irq_reg_write(off: u32, v: u32) {
    IRQ_BOARD.write(off, v)
}

fn irq_publish_work() {
    IRQ_BOARD.publishes.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

static HOST_IRQ_ACCESS: IrqAccess = IrqAccess {
    reg_read: irq_reg_read,
    reg_write: irq_reg_write,
    publish_work: irq_publish_work,
};

/// Resets the IRQ board + handler state and arms the fake access with one
/// raw STATUS value (the device's pre-ack view).
fn arm_irq_board(status: u32) {
    IRQ_BOARD.reset();
    IRQ_BOARD.set_quiet(DMA_CHAN_STATUS, status);
    k3_async_net::reset_irq_state();
    k3_async_net::arm_irq_access(&HOST_IRQ_ACCESS);
}

fn publishes() -> u64 {
    IRQ_BOARD.publishes.load(std::sync::atomic::Ordering::SeqCst)
}

/// Async-mode happy init over a fresh env (IOC mode, notify closed).
fn init_async() -> (
    Gmac<TestBacking>,
    FakeEnv,
    &'static mut [u8; ARENA_BYTES],
) {
    let (mut env, slice) = FakeEnv::new();
    env.set_phy_link_1000fd();
    let guard = ArenaGuard::new(TestBacking { arena_va: env.arena_va, arena_pa: env.arena_pa });
    let mut g = Gmac::new_async(guard).expect("fresh arena layout must pass");
    g.init_rx(&mut env).expect("async init");
    g.enable_tx(&mut env);
    (g, env, slice)
}

/// The async publish keeps IOC set and OWN last; init leaves the device
/// notification closed (the TX IOC witness lives in the next test).
#[test]
fn async_ioc_bits_published_own_last() {
    let (_g, env, arena) = init_async();
    assert_eq!(
        env.regs.get(&DMA_CHAN_IRQ_EN),
        Some(&0),
        "init establishes IOC mode with the notification closed"
    );
    assert_eq!(
        desc_word(&arena, &env, 0, 3),
        RDES3_IOC | RDES3_OWN | RDES3_BUF1V,
        "async refill publishes the RX IOC bit with OWN as the final word"
    );
}

#[test]
fn async_ioc_tx_word2_and_polling_keeps_clear() {
    let (mut g, mut env, arena) = init_async();
    let frame = [0x11u8; 60];
    match g.try_submit(&mut env, &frame) {
        Ok(TxSubmit::Submitted { slot }) => assert_eq!(slot, 0),
        other => panic!("expected Submitted, got {other:?}"),
    }
    assert_eq!(
        tx_desc_word(&arena, 0, 2),
        60 | TDES2_IOC,
        "async TX submit raises the completion interrupt (des2 IOC)"
    );

    // The polling constructor keeps both IOC bits clear (MS11 behavior).
    let (mut penv, parena) = FakeEnv::new();
    penv.set_phy_link_1000fd();
    let guard =
        ArenaGuard::new(TestBacking { arena_va: penv.arena_va, arena_pa: penv.arena_pa });
    let mut p = Gmac::new(guard).expect("layout");
    p.init_rx(&mut penv).expect("polling init");
    p.enable_tx(&mut penv);
    assert_eq!(
        desc_word(&parena, &penv, 0, 3),
        RDES3_OWN | RDES3_BUF1V,
        "polling RX publish keeps IOC clear"
    );
    match p.try_submit(&mut penv, &frame) {
        Ok(TxSubmit::Submitted { .. }) => {}
        other => panic!("expected Submitted, got {other:?}"),
    }
    assert_eq!(tx_desc_word(&parena, 0, 2), 60, "polling TX submit keeps IOC clear");
}

/// Combined cause: the handler saves the full pre-ack raw, latches the fatal
/// before the ack, suppresses the device, W1Cs exactly the observed known
/// bits/summaries and publishes exactly one work hint.
#[test]
fn irq_pre_ack_raw_fatal_w1c_and_work_hint() {
    let _irq = irq_test_guard();
    let raw = DMA_STATUS_FBE | DMA_STATUS_RI | DMA_STATUS_TI | DMA_STATUS_NIS | DMA_STATUS_AIS;
    arm_irq_board(raw);
    k3_async_net::gmac133_handler();
    assert_eq!(k3_async_net::irq_last_raw(), raw, "pre-ack raw kept whole");
    assert!(k3_async_net::irq_fatal(), "fatal latched before the ack");
    assert!(k3_async_net::irq_faulted());
    assert_eq!(
        k3_async_net::irq_counts(),
        (1, 1, 1, 0),
        "RI/TI/abnormal counted once each from the combined raw; no spurious"
    );
    let known = raw & k3_async_net::KNOWN_STATUS_MASK;
    assert_eq!(
        IRQ_BOARD.writes_snapshot(),
        vec![(DMA_CHAN_IRQ_EN, 0), (DMA_CHAN_STATUS, known)],
        "fatal suppresses the device notify, then W1Cs exactly the observed known bits"
    );
    assert_eq!(publishes(), 1, "the fatal publishes one work hint for the waiter");
}

/// Zero status: counted as spurious only — no W1C, no mask, no work hint.
#[test]
fn irq_zero_status_counts_spurious_without_work() {
    let _irq = irq_test_guard();
    arm_irq_board(0);
    k3_async_net::gmac133_handler();
    assert_eq!(k3_async_net::irq_counts(), (0, 0, 0, 1));
    assert!(
        IRQ_BOARD.writes_snapshot().is_empty(),
        "zero cause never writes the device"
    );
    assert_eq!(publishes(), 0, "zero fabricates no work");
    assert!(!k3_async_net::irq_faulted());
}

/// An unknown effective channel cause is a stable fault: mask the device,
/// W1C only the known bits and publish once so the waiter sees the error.
#[test]
fn irq_unknown_cause_masks_and_faults() {
    let _irq = irq_test_guard();
    const UNKNOWN: u32 = 1 << 21;
    let raw = DMA_STATUS_RI | DMA_STATUS_NIS | UNKNOWN;
    arm_irq_board(raw);
    k3_async_net::gmac133_handler();
    assert!(k3_async_net::irq_faulted(), "unknown effective cause is a stable fault");
    assert!(!k3_async_net::irq_fatal());
    assert_eq!(k3_async_net::irq_last_raw(), raw);
    let writes = IRQ_BOARD.writes_snapshot();
    assert_eq!(
        writes[0],
        (DMA_CHAN_IRQ_EN, 0),
        "unknown cause masks the device notify (storm stop)"
    );
    assert_eq!(
        writes[1],
        (DMA_CHAN_STATUS, raw & k3_async_net::KNOWN_STATUS_MASK),
        "only the observed known bits are W1C'd; unknown bits stay observable"
    );
    assert_eq!(publishes(), 1, "the unknown fault publishes one work hint");
}

/// The board-adjudicated routine TX-completion shape (V4 run 2 raw `0x8405`):
/// TI + TBU + NIS + bit10 — classified as known, no fault, and bit10 rides
/// the W1C exactly like the named causes.
#[test]
fn irq_bit10_companion_is_benign() {
    let raw = DMA_STATUS_TI | DMA_STATUS_TBU | DMA_STATUS_NIS | DMA_STATUS_BIT10;
    arm_irq_board(raw);
    k3_async_net::gmac133_handler();
    assert_eq!(k3_async_net::irq_last_raw(), raw, "pre-ack raw kept whole");
    assert!(!k3_async_net::irq_faulted(), "bit10 is a known benign companion");
    assert_eq!(k3_async_net::irq_counts(), (0, 1, 1, 0), "TI counted, TBU counts abnormal");
    assert_eq!(
        IRQ_BOARD.writes_snapshot(),
        vec![(DMA_CHAN_STATUS, raw)],
        "no suppress; the W1C covers the observed known bits including bit10"
    );
    assert_eq!(publishes(), 1);
}

/// The IRQ context touches only the GMAC register window: the arena stays
/// byte-identical across a handler run and every write targets a known
/// register (no descriptor, cache, lock or console surface exists on the
/// IRQ access table).
#[test]
fn irq_handler_touches_only_gmac_registers() {
    let _irq = irq_test_guard();
    arm_irq_board(DMA_STATUS_TI | DMA_STATUS_NIS);
    let (_env, arena) = FakeEnv::new();
    k3_async_net::gmac133_handler();
    assert!(
        arena.iter().all(|&b| b == 0),
        "the handler never touches descriptor or buffer memory"
    );
    let offs: Vec<u32> = IRQ_BOARD.writes_snapshot().iter().map(|(o, _)| *o).collect();
    assert!(
        offs.iter().all(|o| *o == DMA_CHAN_IRQ_EN || *o == DMA_CHAN_STATUS),
        "handler writes target only IRQ_EN/STATUS: {offs:?}"
    );
    assert_eq!(publishes(), 1);
}

/// The pre-ack fatal latch folds into the driver: after the ack cleared the
/// STATUS word, every later poll/query still reports the fault, no new
/// submit is accepted, the bounded stop masks the notify and the published
/// backing stays retained.
#[test]
fn fatal_after_ack_queries_error_and_submit_refused() {
    let _irq = irq_test_guard();
    arm_irq_board(DMA_STATUS_FBE | DMA_STATUS_NIS | DMA_STATUS_AIS);
    k3_async_net::gmac133_handler();
    let (mut g, mut env, _arena) = init_async();
    g.arm_notify(&mut env, true);
    assert_eq!(
        env.regs.get(&DMA_CHAN_IRQ_EN),
        Some(&DMA_IRQ_EN_DEFAULT),
        "the both-direction arm word is 0xD041"
    );
    let frame = [0x22u8; 60];
    match g.try_submit(&mut env, &frame) {
        Ok(TxSubmit::Submitted { .. }) => {}
        other => panic!("expected Submitted, got {other:?}"),
    }
    // Fold the pre-ack fatal latch into the driver (the session's step).
    g.latch_fault(&mut env, k3_async_net::irq_last_raw());
    let mut scratch = [0u8; 1514];
    assert!(matches!(
        g.poll_rx(&mut env, &mut scratch),
        Err(GmacError::Fault { .. })
    ));
    assert!(matches!(g.poll_tx(&mut env), Err(GmacError::Fault { .. })));
    assert!(matches!(g.check_completions(&mut env), Err(GmacError::Fault { .. })));
    assert!(matches!(
        g.try_submit(&mut env, &frame),
        Err(TxSubmitError::Driver(GmacError::Fault { .. }))
    ));
    assert_eq!(
        env.regs.get(&DMA_CHAN_IRQ_EN),
        Some(&0),
        "the folded fault runs the bounded stop (notify masked)"
    );
    assert!(g.is_published(), "the faulted owner retains the published backing");
}

/// The read-only arm-recheck: descriptor work before, during and after the
/// arm is caught without reclaiming or advancing a head; an empty TX
/// direction (unused OWN=0 slots) never reports work.
#[test]
fn async_check_completions_windows_no_false_work() {
    let (mut g, mut env, mut arena) = init_async();

    // Quiescent after init: nothing ready.
    let hint = g.check_completions(&mut env).expect("quiescent check");
    assert!(!hint.rx_ready && !hint.tx_ready);

    // Before-arm window: the completion already landed on RX head slot 0.
    hw_complete(&mut arena, 0, 70, &[0x5Au8; 66]);
    let hint = g.check_completions(&mut env).expect("rx-ready check");
    assert!(hint.rx_ready && !hint.tx_ready, "before-arm completion is work, not sleep");

    // Reclaim it through the owner path; the head advances (read-only check
    // never advances anything itself).
    let mut scratch = [0u8; 1514];
    match g.poll_rx(&mut env, &mut scratch).expect("reclaim") {
        RxPoll::Frame { len, .. } => assert_eq!(len, 66),
        other => panic!("expected Frame, got {other:?}"),
    }
    let hint = g.check_completions(&mut env).expect("quiescent again");
    assert!(!hint.rx_ready && !hint.tx_ready);

    // Empty TX: every unused slot reads OWN=0, but with no live owner that
    // is not a completion.
    let hint = g.check_completions(&mut env).expect("empty-tx check");
    assert!(!hint.tx_ready, "no live TX owner: unused OWN=0 is not work");

    // TX submit → pending (OWN set, live owner) → device completes it →
    // during/after-arm window reports tx work.
    let frame = [0x33u8; 60];
    match g.try_submit(&mut env, &frame) {
        Ok(TxSubmit::Submitted { slot }) => assert_eq!(slot, 0),
        other => panic!("expected Submitted, got {other:?}"),
    }
    let hint = g.check_completions(&mut env).expect("tx pending check");
    assert!(!hint.tx_ready, "an in-flight descriptor is pending, not work");
    set_tx_desc_word(&mut arena, 0, 3, k3_gmac::TDES3_FD | k3_gmac::TDES3_LD | 60);
    let hint = g.check_completions(&mut env).expect("tx-ready check");
    assert!(hint.tx_ready, "the completed TX head with a live owner is work");
    // Ring wrap: the RX head wrapped past slot 15 keeps reporting only real
    // completions (slot 1 is still device-owned here).
    let hint = g.check_completions(&mut env).expect("wrap check");
    assert!(!hint.rx_ready, "device-owned head after wrap is idle, not work");
}

// ---------------------------------------------------------------------------
// Tests — MS13 Iteration 000 T1: bounded runtime stop / SWR / same-arena
// rebuild micro-steps
// ---------------------------------------------------------------------------

/// Steps the machine from begin to the confirmed Stopped boundary
/// (`RuntimeRecovery::Rebuilding`); returns the steps consumed.
fn steps_to_confirmed<B: ArenaBacking>(
    g: &mut Gmac<B>,
    env: &mut FakeEnv,
) -> usize {
    let mut n = 0usize;
    loop {
        match g.runtime_poll_step(env).expect("in-window step") {
            k3_gmac::RecoveryStep::Pending
            | k3_gmac::RecoveryStep::Stepped { stage: k3_gmac::RuntimeRecovery::Resetting } => {}
            k3_gmac::RecoveryStep::Stepped { stage: k3_gmac::RuntimeRecovery::Rebuilding } => {
                return n + 1;
            }
            other => panic!("unexpected step before confirmation: {other:?}"),
        }
        n += 1;
        assert!(n < 200, "the stop machine never reached the confirmed boundary");
    }
}

/// Counts the SWR trigger writes (register 0x1000 with bit0 set).
fn swr_trigger_writes(env: &FakeEnv) -> usize {
    env.reg_writes
        .iter()
        .filter(|(o, v)| *o == 0x1000 && v & 1 != 0)
        .count()
}

/// Counts the issued cache operations (flush/clean/invalidate; fences are
/// barrier markers, not cache work).
fn cache_ops(env: &FakeEnv) -> usize {
    env.ops
        .iter()
        .filter(|op| **op == "flush" || **op == "clean" || **op == "inval")
        .count()
}

/// The machine is a fixed, finite sequence of bounded micro-steps: begin
/// writes nothing, each step performs at most one plain MMIO write or one
/// fixed-slot clear/refill unit (≤2 register writes, ≤2 cache ops), the SWR
/// request is written exactly once, and the rebuild lands on Running with
/// the fixed capacity restored. Repeated begin is a stable Busy.
#[test]
fn runtime_reset_is_step_bounded() {
    let (mut g, mut env, _arena) = init_async();
    let frame = [0x44u8; 60];
    match g.try_submit(&mut env, &frame) {
        Ok(TxSubmit::Submitted { slot }) => assert_eq!(slot, 0),
        other => panic!("expected Submitted, got {other:?}"),
    }
    let writes_at_begin = env.reg_writes.len();
    assert_eq!(
        g.runtime_begin_reset(),
        Ok(k3_gmac::RuntimeRecovery::Resetting),
        "begin arms the machine from the healthy Running state"
    );
    assert_eq!(
        env.reg_writes.len(),
        writes_at_begin,
        "begin is a pure arm: no board access"
    );
    assert_eq!(g.runtime_recovery(), Some(k3_gmac::RuntimeRecovery::Resetting));
    // Mid-recovery begin: stable Busy, never a second SWR write.
    assert_eq!(g.runtime_begin_reset(), Err(k3_gmac::RecoveryRefusal::Busy));

    let mut steps = 0usize;
    loop {
        let (prev_writes, prev_cache) = (env.reg_writes.len(), cache_ops(&env));
        let out = g.runtime_poll_step(&mut env).expect("in-window step");
        steps += 1;
        assert!(
            env.reg_writes.len() - prev_writes <= 2,
            "step {steps} wrote more than one unit (doorbell+SR rewrite bound)"
        );
        assert!(
            cache_ops(&env) - prev_cache <= 2,
            "step {steps} issued more cache work than one unit"
        );
        match out {
            k3_gmac::RecoveryStep::Pending
            | k3_gmac::RecoveryStep::Stepped {
                stage: k3_gmac::RuntimeRecovery::Resetting
            }
            | k3_gmac::RecoveryStep::Stepped {
                stage: k3_gmac::RuntimeRecovery::Rebuilding
            } => {}
            k3_gmac::RecoveryStep::Stepped {
                stage: k3_gmac::RuntimeRecovery::Failed
            } => panic!("a failed stage never reports Stepped"),
            k3_gmac::RecoveryStep::Recovered => break,
        }
        assert!(steps < 200, "the machine is not step-bounded");
    }
    assert!(
        steps <= 100,
        "the fixed stop+rebuild sequence exceeded the step bound: {steps}"
    );
    assert_eq!(
        swr_trigger_writes(&env),
        2,
        "init + exactly one recovery SWR request; begin/step never re-writes it"
    );
    assert_eq!(g.state(), GmacState::Running, "rebuild publishes Running");
    assert_eq!(g.runtime_recovery(), None, "the machine is consumed on Recovered");
    assert_eq!(g.tx_free_capacity(), 16, "fixed TX capacity restored");
}

/// Between begin and the confirmed SWR boundary no ring register is written
/// (bases/lengths/doorbells, no SR start) and the arena memory is
/// byte-identical; the old owner summary stays observable at the boundary.
#[test]
fn reset_does_not_rewrite_ring_before_confirmation() {
    let (mut g, mut env, arena) = init_async();
    let frame = [0x55u8; 60];
    match g.try_submit(&mut env, &frame) {
        Ok(TxSubmit::Submitted { .. }) => {}
        other => panic!("expected Submitted, got {other:?}"),
    }
    let before = arena.to_vec();
    let ring_writes_at_begin = env.rx_ring_writes();
    assert_eq!(g.runtime_begin_reset(), Ok(k3_gmac::RuntimeRecovery::Resetting));
    loop {
        match g.runtime_poll_step(&mut env).expect("in-window step") {
            k3_gmac::RecoveryStep::Pending
            | k3_gmac::RecoveryStep::Stepped { stage: k3_gmac::RuntimeRecovery::Resetting } => {}
            k3_gmac::RecoveryStep::Stepped { stage: k3_gmac::RuntimeRecovery::Rebuilding } => break,
            other => panic!("unexpected step outcome: {other:?}"),
        }
        assert_eq!(
            env.rx_ring_writes(),
            ring_writes_at_begin,
            "no ring-base/length/tail/start write before the SWR confirmation"
        );
        assert!(
            arena[..] == before[..],
            "no DMA-reachable memory is rewritten before confirmation"
        );
    }
    // The fixed Stopped boundary: observable state, retained owner summary.
    assert!(matches!(g.state(), GmacState::Stopped { .. }));
    assert_eq!(g.runtime_recovery(), Some(k3_gmac::RuntimeRecovery::Rebuilding));
    assert_eq!(g.tx_inflight(), 1, "the old owner summary is still observable");
    assert_eq!(env.rx_ring_writes(), ring_writes_at_begin);
    assert!(arena[..] == before[..], "the boundary itself rewrote nothing");
}

/// The SWR confirmation deadline is absolute: taken once at the request
/// write, never renewed by the polling steps. A self-clear past the window
/// (2 s latency, 100 ms per access) must report ResetTimeout — with a
/// renewed deadline the machine would have confirmed at 2 s instead. The
/// failed machine re-reports the same stable error with no board access.
#[test]
fn reset_deadline_is_absolute() {
    let (mut g, mut env, _arena) = init_async();
    env.access_cost_ns = 100_000_000; // every board access costs 100 ms
    env.swr_latency_ns = 2_000_000_000; // self-clears only after 2 s
    assert_eq!(g.runtime_begin_reset(), Ok(k3_gmac::RuntimeRecovery::Resetting));
    let mut pending = 0usize;
    loop {
        match g.runtime_poll_step(&mut env) {
            Ok(k3_gmac::RecoveryStep::Pending) => pending += 1,
            Ok(k3_gmac::RecoveryStep::Stepped { stage: k3_gmac::RuntimeRecovery::Resetting }) => {}
            Err(GmacError::ResetTimeout { raw }) => {
                assert_eq!(raw, 1, "the raw SWR bit is reported");
                break;
            }
            other => panic!("expected the absolute deadline to fire, got {other:?}"),
        }
        assert!(
            pending <= ((k3_gmac::RESET_BOUND_NANOS / env.access_cost_ns) + 2) as usize,
            "the SWR deadline was renewed instead of staying absolute"
        );
    }
    assert!(pending >= 1, "the window held Pending before the deadline");
    assert_eq!(g.runtime_recovery(), Some(k3_gmac::RuntimeRecovery::Failed));
    assert!(matches!(g.state(), GmacState::Stopped { .. }));
    let writes_before = env.reg_writes.len();
    assert!(
        matches!(
            g.runtime_poll_step(&mut env),
            Err(GmacError::ResetTimeout { .. })
        ),
        "the failed machine re-reports the same stable error"
    );
    assert_eq!(
        env.reg_writes.len(),
        writes_before,
        "the stable error re-report touches no board access"
    );
    assert_eq!(
        g.runtime_begin_reset(),
        Err(k3_gmac::RecoveryRefusal::BadState),
        "a failed machine refuses begin"
    );
}

/// MS13 T7: the honest fail-stop injection. The SWR write is really issued;
/// the FIRST confirm read executes with its real value, and every later
/// confirm read is withheld (no board access at all), so the machine rides
/// the absolute deadline into `ResetTimeout` carrying the last honest raw
/// value. During the withheld window: no read, no write, submit refuses; the
/// failed machine re-reports the stable error with no board access.
#[test]
fn confirm_withhold_injection_reports_honest_raw_timeout() {
    let (mut g, mut env, _arena) = init_async();
    // SWR self-clears only after 10 s of fake time; the 1 s absolute deadline
    // hits first, so the injection has an honest SWR-set value to report.
    env.swr_latency_ns = 10_000_000_000;
    k3_gmac::diagnostic_arm_reset_confirm_withhold(true);

    let frame = [0x77u8; 60];
    match g.try_submit(&mut env, &frame) {
        Ok(TxSubmit::Submitted { .. }) => {}
        other => panic!("expected Submitted, got {other:?}"),
    }
    assert_eq!(g.runtime_begin_reset(), Ok(k3_gmac::RuntimeRecovery::Resetting));

    // Step to the first confirm. DMA_MODE is read twice before the machine
    // holds Pending: once by the stop teardown's RMW (step 7) and once by the
    // FIRST confirm read — the injection's single honest read. Everything
    // else in the window reads nothing.
    let reads_at_begin = env.dma_mode_reads();
    let mut saw_pending = false;
    for _ in 0..40 {
        match g.runtime_poll_step(&mut env) {
            Ok(k3_gmac::RecoveryStep::Pending) => {
                saw_pending = true;
                assert_eq!(
                    env.dma_mode_reads(),
                    reads_at_begin + 2,
                    "exactly the teardown RMW read plus the ONE honest confirm read"
                );
                break;
            }
            Ok(k3_gmac::RecoveryStep::Stepped {
                stage: k3_gmac::RuntimeRecovery::Resetting,
            }) => {
                assert!(env.dma_mode_reads() <= reads_at_begin + 2);
            }
            other => panic!("unexpected step before the confirm: {other:?}"),
        }
    }
    assert!(saw_pending, "the injected machine reached the confirm step");

    // Withheld window: every later confirm step touches NO board access —
    // no read, no write, no submit — and the machine stays Pending.
    let reads_at_withhold = env.dma_mode_reads();
    let writes_at_withhold = env.reg_writes.len();
    for _ in 0..8 {
        match g.runtime_poll_step(&mut env) {
            Ok(k3_gmac::RecoveryStep::Pending) => {}
            other => panic!("the withheld window must hold Pending, got {other:?}"),
        }
        assert_eq!(env.dma_mode_reads(), reads_at_withhold, "confirm reads are withheld");
        assert_eq!(env.reg_writes.len(), writes_at_withhold, "the window writes nothing");
        let mut stalled_frame = [0x88u8; 60];
        assert!(
            g.try_submit(&mut env, &stalled_frame).is_err(),
            "a machine mid-stop refuses new submits"
        );
        assert_eq!(env.reg_writes.len(), writes_at_withhold, "the refusal writes nothing");
    }

    // Only the test can move time forward (the withheld path performs no
    // access); past the absolute deadline the machine fails with the LAST
    // HONEST raw value.
    env.advance_clock(k3_gmac::RESET_BOUND_NANOS + 1);
    match g.runtime_poll_step(&mut env) {
        Err(GmacError::ResetTimeout { raw }) => {
            assert_eq!(raw, 1, "raw is the last honest SWR-set read, never fabricated");
        }
        other => panic!("expected the honest ResetTimeout, got {other:?}"),
    }
    assert_eq!(env.dma_mode_reads(), reads_at_withhold, "the failure itself reads nothing");
    assert_eq!(g.runtime_recovery(), Some(k3_gmac::RuntimeRecovery::Failed));
    assert!(matches!(g.state(), GmacState::Stopped { .. }));
    let writes_at_fail = env.reg_writes.len();
    assert!(
        matches!(g.runtime_poll_step(&mut env), Err(GmacError::ResetTimeout { raw: 1 })),
        "the failed machine re-reports the same honest error"
    );
    assert_eq!(env.reg_writes.len(), writes_at_fail, "the re-report touches nothing");
    assert_eq!(
        g.runtime_begin_reset(),
        Err(k3_gmac::RecoveryRefusal::BadState),
        "a failed machine refuses begin"
    );
    k3_gmac::diagnostic_arm_reset_confirm_withhold(false);
}

/// The rebuild reuses the same arena: the rebuilt ring-base registers carry
/// the original PA bases, all 16 RX slots are republished (async IOC, OWN
/// last), the fixed capacity returns, the notify stays closed for the owner
/// to arm, and the rebuilt plane serves a full TX terminal.
#[test]
fn runtime_reset_rebuilds_same_arena() {
    let (mut g, mut env, mut arena) = init_async();
    let frame = [0x66u8; 60];
    match g.try_submit(&mut env, &frame) {
        Ok(TxSubmit::Submitted { .. }) => {}
        other => panic!("expected Submitted, got {other:?}"),
    }
    let rx_ll0 = env.regs.get(&0x111c).copied().expect("init wrote the RX base");
    let tx_ll0 = env.regs.get(&0x1114).copied().expect("init wrote the TX base");
    assert_eq!(g.runtime_begin_reset(), Ok(k3_gmac::RuntimeRecovery::Resetting));
    let mut steps = 0usize;
    loop {
        match g.runtime_poll_step(&mut env).expect("in-window step") {
            k3_gmac::RecoveryStep::Pending
            | k3_gmac::RecoveryStep::Stepped { .. } => {}
            k3_gmac::RecoveryStep::Recovered => break,
        }
        steps += 1;
        assert!(steps < 200, "the machine never recovered");
    }
    assert_eq!(g.state(), GmacState::Running);
    assert_eq!(g.runtime_recovery(), None);
    // Same arena: the rebuilt ring bases name the original PA addresses.
    assert_eq!(env.regs.get(&0x111c), Some(&rx_ll0), "same RX ring PA");
    assert_eq!(env.regs.get(&0x1114), Some(&tx_ll0), "same TX ring PA");
    assert!(g.is_published(), "the backing is retained across the rebuild");
    for slot in 0..NUM_SLOTS {
        assert_eq!(
            desc_word(&arena, &env, slot, 3),
            RDES3_IOC | RDES3_OWN | RDES3_BUF1V,
            "RX slot {slot} republished with the async IOC and OWN last"
        );
    }
    assert_eq!(g.tx_inflight(), 0, "the confirmed stop closed the old carriers");
    assert_eq!(g.tx_free_capacity(), 16, "fixed TX capacity restored");
    assert_eq!(
        env.regs.get(&DMA_CHAN_IRQ_EN),
        Some(&0),
        "the notify stays closed after the rebuild (owner arms it)"
    );
    // The rebuilt plane is live: one full submit → device completion →
    // single-shot terminal on the same arena.
    let frame2 = [0x77u8; 60];
    match g.try_submit(&mut env, &frame2) {
        Ok(TxSubmit::Submitted { slot }) => assert_eq!(slot, 0, "the wrap starts at slot 0"),
        other => panic!("expected Submitted after rebuild, got {other:?}"),
    }
    set_tx_desc_word(&mut arena, 0, 3, TDES3_FD | TDES3_LD | 60);
    match g.poll_tx(&mut env).expect("terminal after rebuild") {
        TxPoll::Completed { slot, .. } => assert_eq!(slot, 0),
        other => panic!("expected Completed, got {other:?}"),
    }
}

/// A stuck SWR holds Pending without rewriting the arena or losing owners,
/// crosses the absolute deadline into a stable ResetTimeout, refuses every
/// data-plane entry, and the published backing survives the drop.
#[test]
fn stuck_reset_retains_arena_and_owners() {
    let (mut env, arena) = FakeEnv::new();
    env.set_phy_link_1000fd();
    let dropped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let guard = ArenaGuard::new(TrackingBacking {
        arena_va: env.arena_va,
        arena_pa: env.arena_pa,
        dropped: dropped.clone(),
    });
    let mut g = Gmac::new_async(guard).expect("layout");
    g.init_rx(&mut env).expect("init");
    g.enable_tx(&mut env);
    let frame = [0x88u8; 60];
    match g.try_submit(&mut env, &frame) {
        Ok(TxSubmit::Submitted { .. }) => {}
        other => panic!("expected Submitted, got {other:?}"),
    }
    let before = arena.to_vec();
    env.swr_stuck = true;
    env.access_cost_ns = 100_000_000;
    assert_eq!(g.runtime_begin_reset(), Ok(k3_gmac::RuntimeRecovery::Resetting));
    // Inside the window: Pending only, nothing retained changes.
    let mut in_window = 0usize;
    loop {
        match g.runtime_poll_step(&mut env) {
            Ok(k3_gmac::RecoveryStep::Pending) => in_window += 1,
            Ok(k3_gmac::RecoveryStep::Stepped { stage: k3_gmac::RuntimeRecovery::Resetting }) => {}
            Err(_) => break,
            other => panic!("stuck SWR must hold the window, got {other:?}"),
        }
        assert_eq!(g.tx_inflight(), 1, "the owner is preserved while pending");
        assert!(g.is_published(), "the backing is retained while pending");
        assert!(
            arena[..] == before[..],
            "a pending stop never rewrites DMA-reachable memory"
        );
        assert!(in_window < 30, "the window never crossed into the deadline");
    }
    // The deadline fired (the loop above broke on Err): stable failure state.
    assert!(matches!(g.state(), GmacState::Stopped { .. }));
    assert_eq!(g.runtime_recovery(), Some(k3_gmac::RuntimeRecovery::Failed));
    assert_eq!(g.tx_inflight(), 1, "the unconfirmed stop keeps the owner");
    assert!(g.is_published(), "the backing is retained after the failure");
    let mut scratch = [0u8; 1514];
    assert!(
        matches!(
            g.try_submit(&mut env, &frame),
            Err(TxSubmitError::Driver(_))
        ),
        "the failed machine refuses submits"
    );
    assert!(g.poll_rx(&mut env, &mut scratch).is_err(), "refuses reap");
    assert!(g.poll_tx(&mut env).is_err(), "refuses reclaim");
    assert!(
        arena[..] == before[..],
        "the failure path rewrote no DMA-reachable memory"
    );
    drop(g);
    assert!(
        !dropped.load(std::sync::atomic::Ordering::SeqCst),
        "the published backing survives the drop (retained until external reset)"
    );
}

/// The pure wait decision: fault first, then descriptor work, then the
/// event generation observed since the caller's snapshot (the window that
/// closes event-before/during/after-arm and the pre-Pending publish).
#[test]
fn raw_wait_decision_prefers_fault_then_descriptors_then_events() {
    use k3_async_net::{raw_wait_decision, RawWaitDecision};
    let _irq = irq_test_guard();

    assert_eq!(raw_wait_decision(true, 0, false, false), RawWaitDecision::Fault);
    assert_eq!(raw_wait_decision(false, 0, true, false), RawWaitDecision::Work);
    assert_eq!(raw_wait_decision(false, 0, false, true), RawWaitDecision::Work);
    // Sleep requires a FRESH generation snapshot: the decision reads the
    // global counter, so the quiescent case compares against it directly.
    let baseline = k3_async_net::irq_work_gen();
    assert_eq!(
        raw_wait_decision(false, baseline, false, false),
        RawWaitDecision::Sleep,
        "nothing new since a fresh snapshot: the waiter may pend"
    );
    assert_eq!(
        raw_wait_decision(false, baseline.wrapping_sub(1), false, false),
        RawWaitDecision::Work,
        "a stale snapshot with a moved generation reports work"
    );

    // Event published while the descriptor surface stayed quiet: a stale
    // generation snapshot reports work, a fresh one sleeps.
    arm_irq_board(DMA_STATUS_TI | DMA_STATUS_NIS);
    k3_async_net::gmac133_handler();
    let work_gen = k3_async_net::irq_work_gen();
    assert_eq!(work_gen, 1);
    assert_eq!(
        raw_wait_decision(false, work_gen - 1, false, false),
        RawWaitDecision::Work,
        "event-before-register window closes through the generation recheck"
    );
    assert_eq!(
        raw_wait_decision(false, work_gen, false, false),
        RawWaitDecision::Sleep,
        "nothing new since the snapshot: the waiter may pend"
    );
}
