//! K3 GMAC polling driver core (MS11 Iteration 000 tasks 1.1–1.2, Iteration
//! 001 task 2.1).
//!
//! The board is the SpacemiT K3 CoM260 Kit GMAC (`mac_probe::MAC_BASE_PADDR`,
//! DWMAC-variant, RTL8211F PHY at MDIO address 1 — MS09/MS10 adjudications).
//! This module holds the exclusive-arena DMA foundation (task 1.1), the
//! bounded RX data plane (task 1.2) and the bounded TX data plane (task
//! 2.1). Design facts that shape the code:
//!
//! - Single foreground owner: the caller must have masked sources 60/133/277
//!   (stage 10 window closed, `WindowEnded`) before any MDIO/DMA access —
//!   the ordering invariant is enforced by the stage 11 gate (task 1.3).
//! - The arena is exclusive and cache-owned by this driver: CPU VAs drive
//!   Zicbom operations, PAs drive the device (`dma.rs`). Once the backing is
//!   published (after a confirmed reset, before the first ring-base write)
//!   it is retained until external reset — a drop never releases
//!   device-owned storage.
//! - Publish order per slot: fence → flush whole 2048-byte buffer → address/
//!   control fields → release fence → OWN last → fence → clean whole
//!   64-byte descriptor → `fence iorw, iorw` → tail doorbell. Reap order:
//!   invalidate descriptor → fence → volatile OWN read → (OWN=0) acquire
//!   fence → invalidate whole buffer → fence → copy. A software fence never
//!   substitutes for the cache operations.
//! - All waits are bounded on the platform timebase; deadlines are taken
//!   once per phase and never extended. Empty queues are idle, not timeouts;
//!   cancel/fatal never software-clear OWN to reclaim device-owned slots.

pub mod desc;
pub mod regs;

/// K3 smoltcp polling Device + bounded session (MS11 Iteration 002, task
/// 3.1). Compiled only where the kernel `k3-polling-net` feature (implied
/// by `k3`) or the host polling-net package is active — both bind smoltcp.
#[cfg(feature = "k3-polling-net")]
pub mod polling_net;

#[cfg(feature = "k3")]
use axplat_riscv64_k3::dma;
#[cfg(not(feature = "k3"))]
use crate::k3_dma as dma;

// Kernel builds bind the kernel platform tree; host harnesses bind the same
// source files as root-module siblings (same pattern as `k3_device_irq_probe`
// / `k3_aia`).
#[cfg(feature = "k3")]
use crate::platform::phy_probe as phy_probe;
#[cfg(not(feature = "k3"))]
use super::phy_probe as phy_probe;
#[cfg(feature = "k3")]
use crate::platform::mac_probe as mac_probe;
#[cfg(not(feature = "k3"))]
use super::mac_probe as mac_probe;

use dma::{
    addr_fits_width, addr_hi32, addr_lo32, check_cache_range, decode_addr_width, ArenaLayout,
    AddrWidth, DmaAddressError, BUFFER_BYTES, CACHE_BLOCK_BYTES, DESC_BYTES,
};

pub use desc::{
    decode_rx, RxDrop, RxCompletion, RDES3_BUF1V, RDES3_CTXT, RDES3_ES, RDES3_FD,
    RDES3_LD, RDES3_LEN_MASK, RDES3_OWN, TDES2_IOC, TDES2_LEN_MASK, TDES3_ES, TDES3_FD,
    TDES3_LD, TDES3_LEN_MASK, TDES3_OWN,
};
pub use regs::{
    DMA_CHAN_CTL, DMA_CHAN_IRQ_EN, DMA_CHAN_STATUS, DMA_RX_CTL, DMA_RX_CTL_PBL_8,
    DMA_RX_CTL_RBSZ_2048, DMA_RX_CTL_SR, DMA_RX_DESC_HL, DMA_RX_DESC_LL, DMA_RX_RINGLEN,
    DMA_STATUS_FBE, DMA_STATUS_RBU, DMA_STATUS_RI, DMA_STATUS_TBU, DMA_STATUS_TI,
    DMA_SYS_BUS_MODE, DMA_TX_CTL, DMA_TX_CTL_OSP, DMA_TX_CTL_PBL_8, DMA_TX_CTL_ST,
    DMA_TX_DESC_HL, DMA_TX_DESC_LL, DMA_TX_RINGLEN, DMA_TX_TAIL,
    MAC_ADDR0_HIGH, MAC_ADDR0_LOW, MAC_CONFIG, MAC_CONFIG_DM, MAC_CONFIG_FES, MAC_CONFIG_PS,
    MAC_CONFIG_RE, MAC_CONFIG_TE, MAC_HW_FEATURE1, MAC_PKT_FILTER, MAC_RXQ0_MAP, MAC_RXQ0_MAP_CH0,
    MAC_FILTER_PR, MTL_RXQ0_OP, MTL_TXQ0_OP,
};
pub use regs::DMA_RX_TAIL as RX_TAIL_OFF;

// ---------------------------------------------------------------------------
// Task 1.2: bounded controller init, RX poll and failure ownership
// ---------------------------------------------------------------------------

/// Reset wait bound (contract: ≤ 1 s on real monotonic time; a timeout never
/// writes the ring or starts anything).
pub const RESET_BOUND_NANOS: u64 = 1_000_000_000;
/// Link/autonegotiation wait bound (contract: ≤ 5 s).
pub const LINK_WAIT_NANOS: u64 = 5_000_000_000;
/// One synchronous refill/handoff/reclaim step bound (contract: ≤ 10 ms,
/// observed per step).
pub const REFILL_BOUND_NANOS: u64 = 10_000_000;
/// Poll wait step between link samples.
const LINK_POLL_STEP_NANOS: u64 = 10_000_000;

/// BMSR autonegotiation-complete (C22 bit5; link is [`phy_probe::BMSR_LINK`]).
const BMSR_ANEG_COMPLETE: u16 = 1 << 5;
/// C22 register addresses for the ability decode (BMSR/ANA live in phy_probe).
const LPA_REG: u32 = 5;
const CTRL1000_REG: u32 = 9;
const STAT1000_REG: u32 = 10;

/// Decoded common link ability (drives the MAC PS/FES/DM bits).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkSpeed {
    S1000,
    S100,
    S10,
}

/// The link result of the bounded MDIO negotiation read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LinkConfig {
    pub speed: LinkSpeed,
    pub full_duplex: bool,
}

/// Driver failure taxonomy: every variant names its phase and carries the
/// raw evidence; a cancelled/faulted driver never releases device-owned
/// storage and never software-clears OWN.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GmacError {
    /// Address/cache foundation refused an operation.
    Address(DmaAddressError),
    /// SWR did not self-clear within 1 s; `raw` is the last mode register.
    ResetTimeout { raw: u32 },
    /// MDIO transaction failure during the link phase.
    Mdio(phy_probe::MdioError),
    /// Link/AN did not come up within 5 s; `bmsr2` is the last level read.
    LinkTimeout { bmsr2: u16 },
    /// No common ability after the AN read; all four raw registers retained.
    NoCommonAbility { ana: u16, lpa: u16, ctrl1000: u16, stat1000: u16 },
    /// A bounded synchronous step exceeded its 10 ms phase deadline.
    PhaseTimeout { phase: Phase },
    /// Cancellation: `after_publish` tells whether device-owned slots exist
    /// (they are preserved either way; pre-publish handoffs were revoked).
    Cancelled { phase: Phase, after_publish: bool },
    /// Fatal bus error or unexpected DMA stop; the driver is faulted.
    Fault { raw_status: u32 },
}

/// Phase names for deadline/cancel reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Reset,
    Link,
    Refill,
    Reclaim,
    Poll,
    /// One bounded submit/doorbell step (10 ms, task 2.1).
    Submit,
}

/// Driver lifecycle state (observable for tests and reports).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GmacState {
    /// Arena mapped, nothing written to the controller.
    Uninit,
    /// RX running (refilled ring, RE/SR on).
    Running,
    /// Bounded stop ran (cancel or software-bounded failure); device-owned
    /// slots are preserved.
    Stopped { reason: &'static str },
    /// Fatal bus error observed; bounded stop ran.
    Faulted { raw_status: u32 },
}

/// One RX poll outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RxPoll {
    /// Head descriptor still device-owned: idle, never a timeout.
    Idle,
    /// Legal frame copied into the caller scratch (FCS stripped).
    Frame { len: usize, slot: usize, raw_w3: u32 },
    /// Classified non-deliverable descriptor (raw status retained); the slot
    /// was refilled.
    Dropped { slot: usize, reason: RxDrop, raw_w3: u32 },
}

// ---------------------------------------------------------------------------
// Task 2.1: bounded TX submit, terminal and single-shot reclaim
// ---------------------------------------------------------------------------

/// Per-submit terminal wait bound (contract: every submit gets an independent
/// 1 s window; a timeout never software-clears OWN).
pub const TX_TERMINAL_NANOS: u64 = 1_000_000_000;
/// TX submit frame bounds: complete Ethernet frames 14..=1514 bytes; the MAC
/// generates the FCS, so the descriptor length always excludes it. Frames
/// below 60 bytes are zero-padded to 60 inside the TX buffer.
pub const TX_FRAME_MIN: usize = 14;
pub const TX_FRAME_MAX: usize = 1514;
pub const TX_PAD_TO: usize = 60;

/// Status bits this driver consumes (W1C fault + summary set). The FBE clear
/// writes only consumed bits — never an unobserved whole-word write.
const STATUS_CONSUMED_MASK: u32 = regs::DMA_STATUS_FBE
    | regs::DMA_STATUS_RI
    | regs::DMA_STATUS_TI
    | regs::DMA_STATUS_RBU
    | regs::DMA_STATUS_TBU;

/// TX submit outcome (Ok branch).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TxSubmit {
    /// The descriptor was published (OWN written last); the independent 1 s
    /// terminal window for this submit has started.
    Submitted { slot: usize },
    /// No free slot: no slot, buffer or register was touched.
    Full,
}

/// TX submit refusal (Err branch): never truncates, never trial-writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TxSubmitError {
    /// Frame length outside `TX_FRAME_MIN..=TX_FRAME_MAX`; the original
    /// length is reported and nothing was written.
    LengthInvalid { len: usize },
    /// Driver-level failure (address/phase deadline/cancel/fault).
    Driver(GmacError),
}

/// One TX poll outcome (one descriptor reported per call, ≤ 16 scanned).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TxPoll {
    /// Head descriptor device-owned within its terminal window.
    Pending { slot: usize },
    /// Device completed (OWN cleared, ES=0); reclaimed exactly once.
    Completed { slot: usize, raw_w3: u32 },
    /// Device flagged ES=1: local error carrying the raw word; the slot is
    /// still reclaimed exactly once. A local terminal is never peer delivery.
    LocalError { slot: usize, raw_w3: u32 },
    /// The 1 s terminal window elapsed with OWN still set: the owner is
    /// retained (no software OWN clear) and the timeout is reported once per
    /// submit; the slot is never reused.
    TerminalTimeout { slot: usize, raw_w3: u32 },
    /// Nothing in flight.
    Idle,
}

/// Software owner state of one TX slot (kept outside the arena). `Prepared`
/// and `Completed` are transient within one call and folded here:
/// `try_submit` marks `Device` only after the OWN publish, and `poll_tx`
/// reclaims an observed completion in the same call.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TxSlotState {
    Free,
    Device,
    /// Terminal window expired and reported; the device still owns the
    /// storage and the slot is never reused.
    TimedOut,
}

/// The K3 GMAC polling driver over an exclusive arena. `E` (the seam) is
/// passed per call so host tests can script the fake board between calls.
pub struct Gmac<B: ArenaBacking> {
    arena: ArenaGuard<B>,
    layout: ArenaLayout,
    rx_head: usize,
    tx_head: usize,
    tx_tail: usize,
    tx_slots: [TxSlotState; dma::NUM_SLOTS],
    tx_deadlines: [u64; dma::NUM_SLOTS],
    tx_inflight: usize,
    state: GmacState,
    link: Option<LinkConfig>,
    cancel_requested: bool,
}

impl<B: ArenaBacking> Gmac<B> {
    /// Validates the arena placement before any controller access.
    pub fn new(arena: ArenaGuard<B>) -> Result<Self, DmaAddressError> {
        let layout = ArenaLayout::new(arena.base_pa(), arena.base_va())?;
        Ok(Gmac {
            arena,
            layout,
            rx_head: 0,
            tx_head: 0,
            tx_tail: 0,
            tx_slots: [TxSlotState::Free; dma::NUM_SLOTS],
            tx_deadlines: [0; dma::NUM_SLOTS],
            tx_inflight: 0,
            state: GmacState::Uninit,
            link: None,
            cancel_requested: false,
        })
    }

    /// Current lifecycle state.
    pub fn state(&self) -> GmacState {
        self.state
    }

    /// Free TX slots (never-published capacity). The polling-net Device
    /// peeks this to guarantee a submit slot before handing out a TX token;
    /// a Full result never modified any slot, so the count is exact.
    pub fn tx_free_capacity(&self) -> usize {
        dma::NUM_SLOTS - self.tx_inflight
    }

    /// Requests cancellation: the next checkpoint (poll entry or the
    /// pre-publish point of a refill) executes the bounded semantics.
    pub fn request_cancel(&mut self) {
        self.cancel_requested = true;
    }

    /// Whether the backing is published (retained until external reset).
    pub fn is_published(&self) -> bool {
        self.arena.is_published()
    }

    /// Bounded init: capability check → APMU delay RMW → SWR (≤1 s, no ring
    /// write on timeout) → publish → link decode via the reused MDIO (≤5 s)
    /// → absolute controller config → ring clear/sync → ring bases → MAC
    /// address/filter/config (RE, TE off) → RX SR → 16-slot refill walk.
    pub fn init_rx<E: GmacEnv>(&mut self, env: &mut E) -> Result<LinkConfig, GmacError> {
        // 0. Address-width capability before any write: a 32-bit-only
        //    controller refuses this board's >4 GiB RAM outright.
        let hw_feature1 = env.reg_read(regs::MAC_HW_FEATURE1);
        let width: AddrWidth = decode_addr_width(hw_feature1).map_err(GmacError::Address)?;
        addr_fits_width(self.arena.base_pa(), &width).map_err(GmacError::Address)?;

        // 1. APMU TX/RX delay RMW (same window as the stage 4 clock enable;
        //    only the enable and phase fields change).
        let delay_pa = mac_probe::APMU_BASE_PADDR + 0x3f0;
        let old = env.mmio_read(delay_pa);
        let new = (old
            & !(regs::APMU_EBUS_DELAY_TX_EN
                | regs::APMU_EBUS_DELAY_TX_PHASE
                | regs::APMU_EBUS_DELAY_RX_EN
                | regs::APMU_EBUS_DELAY_RX_PHASE))
            | regs::APMU_EBUS_DELAY_TX_EN
            | regs::APMU_EBUS_DELAY_TX_PHASE
            | regs::APMU_EBUS_DELAY_RX_EN
            | regs::APMU_EBUS_DELAY_RX_PHASE;
        env.mmio_write(delay_pa, new);

        // 2. SWR bounded on the platform timebase; a timeout writes nothing.
        env.reg_write(regs::DMA_MODE, regs::DMA_MODE_SWR);
        let reset_deadline = env.now_nanos() + RESET_BOUND_NANOS;
        loop {
            if env.reg_read(regs::DMA_MODE) & regs::DMA_MODE_SWR == 0 {
                break;
            }
            if env.now_nanos() >= reset_deadline {
                let raw = env.reg_read(regs::DMA_MODE);
                return Err(GmacError::ResetTimeout { raw });
            }
            core::hint::spin_loop();
        }

        // 3. Publish point: the backing is retained until external reset
        //    from here on.
        self.arena.publish();

        // 4. Link via the reused bounded MDIO (no second reset/AN restart).
        let link = match self.decode_link(env) {
            Ok(link) => link,
            Err(e) => {
                self.bounded_stop(env);
                return Err(e);
            }
        };

        // 5. Absolute controller configuration (contract order).
        let eame = if width.bits() > 32 { regs::DMA_SYS_BUS_EAME } else { 0 };
        env.reg_write(
            regs::DMA_SYS_BUS_MODE,
            regs::DMA_SYS_BUS_RD_OSR_LMT_2 | regs::DMA_SYS_BUS_BLEN8 | regs::DMA_SYS_BUS_BLEN16 | eame,
        );
        env.reg_write(regs::DMA_CHAN_CTL, regs::DMA_CHAN_PBLX8 | regs::DMA_CHAN_DSL_6);
        env.reg_write(regs::DMA_TX_CTL, regs::DMA_TX_CTL_OSP | regs::DMA_TX_CTL_PBL_8); // ST=0
        env.reg_write(regs::DMA_RX_CTL, regs::DMA_RX_CTL_RBSZ_2048 | regs::DMA_RX_CTL_PBL_8); // SR=0
        env.reg_write(regs::DMA_CHAN_IRQ_EN, 0);
        env.reg_write(regs::MTL_TXQ0_OP, regs::MTL_TXQ_TSF | regs::MTL_TXQ_TXQEN | regs::MTL_TXQ_TQS_31);
        env.reg_write(
            regs::MTL_RXQ0_OP,
            regs::MTL_RXQ_RSF | regs::MTL_RXQ_DIS_TCP_EF | regs::MTL_RXQ_RQS_31,
        );

        // 6. Clear and sync both rings (empty, device-visible).
        self.clear_rings(env);

        // 7. Ring bases: high word first, then low.
        let tx_ring = self.layout.tx_ring_pa();
        let rx_ring = self.layout.rx_ring_pa();
        env.reg_write(regs::DMA_TX_DESC_HL, addr_hi32(tx_ring));
        env.reg_write(regs::DMA_TX_DESC_LL, addr_lo32(tx_ring));
        env.reg_write(regs::DMA_RX_DESC_HL, addr_hi32(rx_ring));
        env.reg_write(regs::DMA_RX_DESC_LL, addr_lo32(rx_ring));
        env.reg_write(regs::DMA_TX_RINGLEN, regs::DMA_RINGLEN_15);
        env.reg_write(regs::DMA_RX_RINGLEN, regs::DMA_RINGLEN_15);

        // 8. MAC address 02:00:00:00:26:01 (R69: LOW first, then HIGH+AE).
        env.reg_write(regs::MAC_ADDR0_LOW, 0x0000_0002);
        env.reg_write(regs::MAC_ADDR0_HIGH, regs::MAC_ADDR_AE | 0x0000_0126);

        // 9. RX queue mapping and promiscuous filter.
        env.reg_write(regs::MAC_RXQ0_MAP, regs::MAC_RXQ0_MAP_CH0);
        let pf = env.reg_read(regs::MAC_PKT_FILTER);
        env.reg_write(regs::MAC_PKT_FILTER, pf | regs::MAC_FILTER_PR);

        // 10. MAC config per the decoded ability; TE stays 0, RE on.
        let mut cfg = regs::MAC_CONFIG_RE;
        if link.full_duplex {
            cfg |= regs::MAC_CONFIG_DM;
        }
        match link.speed {
            LinkSpeed::S100 => cfg |= regs::MAC_CONFIG_FES | regs::MAC_CONFIG_PS,
            LinkSpeed::S10 => cfg |= regs::MAC_CONFIG_PS,
            LinkSpeed::S1000 => {}
        }
        env.reg_write(regs::MAC_CONFIG, cfg);
        self.link = Some(link);

        // 11. RX DMA start, then the R69 refill walk: per-slot buffer flush,
        //     descriptor publish, tail doorbell and SR rewrite.
        let sr = regs::DMA_RX_CTL_RBSZ_2048 | regs::DMA_RX_CTL_PBL_8 | regs::DMA_RX_CTL_SR;
        env.reg_write(regs::DMA_RX_CTL, sr);
        for slot in 0..dma::NUM_SLOTS {
            if let Err(e) = self.refill_slot(env, slot, sr) {
                self.bounded_stop(env);
                return Err(e);
            }
        }
        self.rx_head = 0;
        self.state = GmacState::Running;
        Ok(link)
    }

    /// Bounded link decode: a 10 ms re-read loop until BMSR reports both
    /// link and AN complete (single up-read is reliable under latch-low
    /// semantics; V4 confirmed the down case), ≤5 s wait, then one
    /// ANA/LPA/CTRL1000/STAT1000 read and the priority decode. No second
    /// reset, no AN restart, no static fallback.
    fn decode_link<E: GmacEnv>(&self, env: &mut E) -> Result<LinkConfig, GmacError> {
        let deadline = env.now_nanos() + LINK_WAIT_NANOS;
        loop {
            let b2 = phy_probe::mdio_read(env, phy_probe::PHY_ADDR, phy_probe::BMSR_REG)
                .map_err(GmacError::Mdio)?;
            if b2 & phy_probe::BMSR_LINK != 0 && b2 & BMSR_ANEG_COMPLETE != 0 {
                break;
            }
            if env.now_nanos() >= deadline {
                return Err(GmacError::LinkTimeout { bmsr2: b2 });
            }
            env.wait_nanos(LINK_POLL_STEP_NANOS);
        }
        let ana = phy_probe::mdio_read(env, phy_probe::PHY_ADDR, phy_probe::ANA_REG)
            .map_err(GmacError::Mdio)?;
        let lpa = phy_probe::mdio_read(env, phy_probe::PHY_ADDR, LPA_REG).map_err(GmacError::Mdio)?;
        let ctrl1000 = phy_probe::mdio_read(env, phy_probe::PHY_ADDR, CTRL1000_REG)
            .map_err(GmacError::Mdio)?;
        let stat1000 = phy_probe::mdio_read(env, phy_probe::PHY_ADDR, STAT1000_REG)
            .map_err(GmacError::Mdio)?;

        // Priority decode (contract): 1000FD/HD from CTRL1000 bits9/8 vs
        // peer STAT1000 bits11/10; 10/100 from ANA&LPA bits8/7/6/5.
        let common = ana & lpa;
        let link = if ctrl1000 & (1 << 9) != 0 && stat1000 & (1 << 11) != 0 {
            LinkConfig { speed: LinkSpeed::S1000, full_duplex: true }
        } else if ctrl1000 & (1 << 8) != 0 && stat1000 & (1 << 10) != 0 {
            LinkConfig { speed: LinkSpeed::S1000, full_duplex: false }
        } else if common & (1 << 8) != 0 {
            LinkConfig { speed: LinkSpeed::S100, full_duplex: true }
        } else if common & (1 << 7) != 0 {
            LinkConfig { speed: LinkSpeed::S100, full_duplex: false }
        } else if common & (1 << 6) != 0 {
            LinkConfig { speed: LinkSpeed::S10, full_duplex: true }
        } else if common & (1 << 5) != 0 {
            LinkConfig { speed: LinkSpeed::S10, full_duplex: false }
        } else {
            return Err(GmacError::NoCommonAbility { ana, lpa, ctrl1000, stat1000 });
        };
        Ok(link)
    }

    /// Zeros and cache-syncs both rings (empty, device-visible descriptors).
    fn clear_rings<E: GmacEnv>(&self, env: &mut E) {
        for ring in [self.layout.rx_ring_va(), self.layout.tx_ring_va()] {
            for slot in 0..dma::NUM_SLOTS {
                let va = ring + slot * DESC_BYTES;
                for word in 0..4 {
                    write_desc_word(va, word, 0);
                }
                env.fence_rw();
                check_cache_range(self.layout.base_va(), va, DESC_BYTES)
                    .expect("ring descriptor inside the arena");
                env.cache_clean(va, DESC_BYTES);
            }
        }
    }

    /// One checked refill: fence → flush whole buffer → address/control
    /// fields → [deadline/cancel checkpoint] → release fence → OWN last →
    /// fence → clean whole descriptor → full fence → tail doorbell → SR
    /// rewrite. A deadline or cancel before the checkpoint revokes the
    /// handoff; after the checkpoint the device owns the slot.
    fn refill_slot<E: GmacEnv>(
        &mut self,
        env: &mut E,
        slot: usize,
        sr_value: u32,
    ) -> Result<(), GmacError> {
        let deadline = env.now_nanos() + REFILL_BOUND_NANOS;
        let buf_pa = self.layout.rx_buf_pa(slot).map_err(GmacError::Address)?;
        let buf_va = self.layout.rx_buf_va(slot).map_err(GmacError::Address)?;
        let desc_va = self.layout.rx_desc_va(slot).map_err(GmacError::Address)?;

        env.fence_rw();
        check_cache_range(self.layout.base_va(), buf_va, BUFFER_BYTES)
            .map_err(GmacError::Address)?;
        env.cache_flush(buf_va, BUFFER_BYTES);
        write_desc_word(desc_va, 0, addr_lo32(buf_pa));
        write_desc_word(desc_va, 1, addr_hi32(buf_pa));
        write_desc_word(desc_va, 2, 0);
        write_desc_word(desc_va, 3, RDES3_BUF1V);

        // Checkpoint: bounded step + cancellation, both before the publish.
        if env.now_nanos() >= deadline {
            return Err(GmacError::PhaseTimeout { phase: Phase::Refill });
        }
        if self.cancel_requested {
            return Err(GmacError::Cancelled { phase: Phase::Refill, after_publish: false });
        }

        env.fence_rw();
        write_desc_word(desc_va, 3, RDES3_BUF1V | RDES3_OWN);
        env.fence_rw();
        check_cache_range(self.layout.base_va(), desc_va, DESC_BYTES).map_err(GmacError::Address)?;
        env.cache_clean(desc_va, DESC_BYTES);
        env.fence_iorw();
        env.reg_write(
            regs::DMA_RX_TAIL,
            addr_lo32(self.layout.rx_desc_pa(slot).map_err(GmacError::Address)?),
        );
        env.reg_write(regs::DMA_RX_CTL, sr_value);
        Ok(())
    }

    /// Best-effort bounded stop: clears RE/SR and masks the DMA IRQ. This is
    /// damage control, never a stop proof: device-owned descriptors are not
    /// reclaimed and the backing is not released.
    fn bounded_stop<E: GmacEnv>(&mut self, env: &mut E) {
        let mac = env.reg_read(regs::MAC_CONFIG);
        env.reg_write(regs::MAC_CONFIG, mac & !regs::MAC_CONFIG_RE);
        let rx = env.reg_read(regs::DMA_RX_CTL);
        env.reg_write(regs::DMA_RX_CTL, rx & !regs::DMA_RX_CTL_SR);
        env.reg_write(regs::DMA_CHAN_IRQ_EN, 0);
    }

    /// Revocation landing: consume status, run the bounded stop and close
    /// the ring. The half-published slot stays CPU-held and is never
    /// re-scanned; device-owned slots are untouched.
    fn revoke<E: GmacEnv>(&mut self, env: &mut E, status: u32) {
        self.w1c_status(env, status);
        self.bounded_stop(env);
        self.state = GmacState::Stopped { reason: "rx-handoff-revoked" };
    }

    /// Consumes the non-fatal status causes W1C-style: only the bits actually
    /// observed are written back, never a whole-word RMW.
    fn w1c_status<E: GmacEnv>(&self, env: &mut E, status: u32) {
        let consumed = status & (regs::DMA_STATUS_RI | regs::DMA_STATUS_TI | regs::DMA_STATUS_RBU | regs::DMA_STATUS_TBU);
        if consumed != 0 {
            env.reg_write(regs::DMA_CHAN_STATUS, consumed);
        }
    }

    /// One bounded RX poll: consume the status word (FBE → fault), reap the
    /// head descriptor, refill it, and report. The CPU reclaim/refill stays
    /// within its 10 ms phase; an empty queue is idle, not a timeout.
    pub fn poll_rx<E: GmacEnv>(
        &mut self,
        env: &mut E,
        scratch: &mut [u8],
    ) -> Result<RxPoll, GmacError> {
        match self.state {
            GmacState::Running => {}
            GmacState::Faulted { raw_status } => return Err(GmacError::Fault { raw_status }),
            GmacState::Stopped { .. } => {
                return Err(GmacError::Cancelled { phase: Phase::Poll, after_publish: true })
            }
            GmacState::Uninit => {
                return Err(GmacError::Fault { raw_status: 0 });
            }
        }
        let status = env.reg_read(regs::DMA_CHAN_STATUS);
        if status & regs::DMA_STATUS_FBE != 0 {
            self.bounded_stop(env);
            self.stop_tx(env);
            self.state = GmacState::Faulted { raw_status: status };
            env.reg_write(regs::DMA_CHAN_STATUS, status & STATUS_CONSUMED_MASK);
            return Err(GmacError::Fault { raw_status: status });
        }
        let reclaim_deadline = env.now_nanos() + REFILL_BOUND_NANOS;
        let slot = self.rx_head;
        let outcome = raw_poll_rx_desc(env, &self.layout, slot, scratch).map_err(GmacError::Address)?;
        // A revoked handoff (cancel or deadline) leaves the slot CPU-held;
        // [`Self::revoke`] closes the ring so the half-published descriptor
        // is never re-scanned as a device completion.
        let result = match outcome {
            RawRx::NotOwned => {
                if self.cancel_requested {
                    self.revoke(env, status);
                    return Err(GmacError::Cancelled { phase: Phase::Poll, after_publish: true });
                }
                self.w1c_status(env, status);
                RxPoll::Idle
            }
            RawRx::Frame { len, raw_w3 } => {
                // Reclaim bound: observed after the copy, before any new
                // handoff (revocable).
                if env.now_nanos() >= reclaim_deadline {
                    self.revoke(env, status);
                    return Err(GmacError::PhaseTimeout { phase: Phase::Reclaim });
                }
                let sr = regs::DMA_RX_CTL_RBSZ_2048 | regs::DMA_RX_CTL_PBL_8 | regs::DMA_RX_CTL_SR;
                if let Err(e) = self.refill_slot(env, slot, sr) {
                    self.revoke(env, status);
                    return Err(e);
                }
                RxPoll::Frame { len, slot, raw_w3 }
            }
            RawRx::Rejected { reason, raw_w3 } => {
                if env.now_nanos() >= reclaim_deadline {
                    self.revoke(env, status);
                    return Err(GmacError::PhaseTimeout { phase: Phase::Reclaim });
                }
                let sr = regs::DMA_RX_CTL_RBSZ_2048 | regs::DMA_RX_CTL_PBL_8 | regs::DMA_RX_CTL_SR;
                if let Err(e) = self.refill_slot(env, slot, sr) {
                    self.revoke(env, status);
                    return Err(e);
                }
                RxPoll::Dropped { slot, reason, raw_w3 }
            }
        };
        if !matches!(result, RxPoll::Idle) {
            self.rx_head = (slot + 1) % dma::NUM_SLOTS;
            self.w1c_status(env, status);
        }
        Ok(result)
    }

    // -----------------------------------------------------------------------
    // Task 2.1: TX data plane
    // -----------------------------------------------------------------------

    /// Damage-control stop for the TX direction: clears TE and ST (RMW,
    /// every other bit preserved) and masks the DMA IRQ. Never a stop proof:
    /// device-owned descriptors are not reclaimed.
    fn stop_tx<E: GmacEnv>(&mut self, env: &mut E) {
        let mac = env.reg_read(regs::MAC_CONFIG);
        env.reg_write(regs::MAC_CONFIG, mac & !regs::MAC_CONFIG_TE);
        let tx = env.reg_read(regs::DMA_TX_CTL);
        env.reg_write(regs::DMA_TX_CTL, tx & !regs::DMA_TX_CTL_ST);
        env.reg_write(regs::DMA_CHAN_IRQ_EN, 0);
    }

    /// Enables the TX data plane after a successful init: MAC_CONFIG RMW
    /// sets TE (RE/DM/PS/FES preserved), TX_CTL sets ST (OSP|PBL8
    /// preserved). Called once by the foreground sequence.
    pub fn enable_tx<E: GmacEnv>(&mut self, env: &mut E) {
        let mac = env.reg_read(regs::MAC_CONFIG);
        env.reg_write(regs::MAC_CONFIG, mac | regs::MAC_CONFIG_TE);
        let tx = env.reg_read(regs::DMA_TX_CTL);
        env.reg_write(regs::DMA_TX_CTL, tx | regs::DMA_TX_CTL_ST);
    }

    /// One bounded TX submit: validates the length (no truncation, no trial
    /// write), copies the frame (zero-padded to 60 when short), publishes
    /// the descriptor (buffer clean → fields → [deadline/cancel checkpoint]
    /// → release fence → OWN last → fence → descriptor clean → full fence →
    /// tail doorbell → ST rewrite) and starts this submit's independent 1 s
    /// terminal window. Full never modifies anything.
    pub fn try_submit<E: GmacEnv>(
        &mut self,
        env: &mut E,
        frame: &[u8],
    ) -> Result<TxSubmit, TxSubmitError> {
        match self.state {
            GmacState::Running => {}
            GmacState::Faulted { raw_status } => {
                return Err(TxSubmitError::Driver(GmacError::Fault { raw_status }))
            }
            GmacState::Stopped { .. } => {
                return Err(TxSubmitError::Driver(GmacError::Cancelled {
                    phase: Phase::Submit,
                    after_publish: true,
                }))
            }
            GmacState::Uninit => return Err(TxSubmitError::Driver(GmacError::Fault { raw_status: 0 })),
        }
        let status = env.reg_read(regs::DMA_CHAN_STATUS);
        if status & regs::DMA_STATUS_FBE != 0 {
            self.bounded_stop(env);
            self.stop_tx(env);
            self.state = GmacState::Faulted { raw_status: status };
            env.reg_write(regs::DMA_CHAN_STATUS, status & STATUS_CONSUMED_MASK);
            return Err(TxSubmitError::Driver(GmacError::Fault { raw_status: status }));
        }
        self.w1c_status(env, status);
        // Capacity first: a Full return must not modify any slot.
        if self.tx_inflight >= dma::NUM_SLOTS {
            return Ok(TxSubmit::Full);
        }
        // Length contract: reject with the original length, no write.
        if frame.len() < TX_FRAME_MIN || frame.len() > TX_FRAME_MAX {
            return Err(TxSubmitError::LengthInvalid { len: frame.len() });
        }

        let slot = self.tx_tail;
        let submit_deadline = env.now_nanos() + REFILL_BOUND_NANOS;
        let buf_pa = self
            .layout
            .tx_buf_pa(slot)
            .map_err(|e| TxSubmitError::Driver(GmacError::Address(e)))?;
        let buf_va = self
            .layout
            .tx_buf_va(slot)
            .map_err(|e| TxSubmitError::Driver(GmacError::Address(e)))?;
        let desc_va = self
            .layout
            .tx_desc_va(slot)
            .map_err(|e| TxSubmitError::Driver(GmacError::Address(e)))?;
        let pub_len = if frame.len() < TX_PAD_TO { TX_PAD_TO } else { frame.len() };

        // Copy the frame (zero-padded to 60), fence, clean the written
        // region — the device reads what the descriptor length names.
        copy_padded_to_buf(buf_va, frame);
        env.fence_rw();
        check_cache_range(self.layout.base_va(), buf_va, block_round_up(pub_len))
            .map_err(|e| TxSubmitError::Driver(GmacError::Address(e)))?;
        env.cache_clean(buf_va, block_round_up(pub_len));

        // Descriptor fields without OWN (IOC stays clear, no CIC).
        write_desc_word(desc_va, 0, addr_lo32(buf_pa));
        write_desc_word(desc_va, 1, addr_hi32(buf_pa));
        write_desc_word(desc_va, 2, (pub_len as u32) & TDES2_LEN_MASK);
        write_desc_word(desc_va, 3, TDES3_FD | TDES3_LD | ((pub_len as u32) & TDES3_LEN_MASK));

        // Checkpoint: bounded step + cancellation, both before the publish.
        if env.now_nanos() >= submit_deadline {
            return Err(TxSubmitError::Driver(GmacError::PhaseTimeout { phase: Phase::Submit }));
        }
        if self.cancel_requested {
            return Err(TxSubmitError::Driver(GmacError::Cancelled {
                phase: Phase::Submit,
                after_publish: false,
            }));
        }

        // Release fence, then the ownership word last.
        env.fence_rw();
        write_desc_word(
            desc_va,
            3,
            TDES3_OWN | TDES3_FD | TDES3_LD | ((pub_len as u32) & TDES3_LEN_MASK),
        );
        self.tx_slots[slot] = TxSlotState::Device;
        self.tx_deadlines[slot] = env.now_nanos() + TX_TERMINAL_NANOS;
        self.tx_inflight += 1;

        // Clean the whole 64-byte descriptor, then the full fence, the tail
        // doorbell and the ST rewrite (extra doorbell after TBU, R69).
        env.fence_rw();
        check_cache_range(self.layout.base_va(), desc_va, DESC_BYTES)
            .map_err(|e| TxSubmitError::Driver(GmacError::Address(e)))?;
        env.cache_clean(desc_va, DESC_BYTES);
        env.fence_iorw();
        // The tail register is an exclusive upper bound: the DMA fetches
        // strictly below it, so the doorbell names the next free descriptor,
        // never the submitted one.
        let next = (slot + 1) % dma::NUM_SLOTS;
        let next_pa = self
            .layout
            .tx_desc_pa(next)
            .map_err(|e| TxSubmitError::Driver(GmacError::Address(e)))?;
        env.reg_write(regs::DMA_TX_TAIL, addr_lo32(next_pa));
        env.reg_write(
            regs::DMA_TX_CTL,
            regs::DMA_TX_CTL_OSP | regs::DMA_TX_CTL_PBL_8 | regs::DMA_TX_CTL_ST,
        );
        self.tx_tail = next;
        Ok(TxSubmit::Submitted { slot })
    }

    /// One bounded TX poll: consumes the status word (FBE → fault), then
    /// reports one outcome for the oldest in-flight slot — completion, local
    /// error, terminal timeout (once per submit) or pending. The reclaim
    /// stays within its independent 10 ms phase; a timed-out slot keeps the
    /// device owner and is skipped by later polls.
    pub fn poll_tx<E: GmacEnv>(&mut self, env: &mut E) -> Result<TxPoll, GmacError> {
        match self.state {
            GmacState::Running => {}
            GmacState::Faulted { raw_status } => return Err(GmacError::Fault { raw_status }),
            GmacState::Stopped { .. } => {
                return Err(GmacError::Cancelled { phase: Phase::Poll, after_publish: true })
            }
            GmacState::Uninit => return Err(GmacError::Fault { raw_status: 0 }),
        }
        let status = env.reg_read(regs::DMA_CHAN_STATUS);
        if status & regs::DMA_STATUS_FBE != 0 {
            self.bounded_stop(env);
            self.stop_tx(env);
            self.state = GmacState::Faulted { raw_status: status };
            env.reg_write(regs::DMA_CHAN_STATUS, status & STATUS_CONSUMED_MASK);
            return Err(GmacError::Fault { raw_status: status });
        }
        self.w1c_status(env, status);
        let reclaim_deadline = env.now_nanos() + REFILL_BOUND_NANOS;
        for _ in 0..dma::NUM_SLOTS {
            let slot = self.tx_head;
            match self.tx_slots[slot] {
                TxSlotState::Free => {
                    // Nothing in flight. A pending cancel still stops new
                    // submits (bounded stop, isomorphic to the RX direction).
                    if self.cancel_requested {
                        self.stop_tx(env);
                        self.state = GmacState::Stopped { reason: "tx-cancelled" };
                        return Err(GmacError::Cancelled { phase: Phase::Poll, after_publish: false });
                    }
                    return Ok(TxPoll::Idle);
                }
                TxSlotState::TimedOut => {
                    // Already reported; the device keeps the storage.
                    self.tx_head = (slot + 1) % dma::NUM_SLOTS;
                }
                TxSlotState::Device => {
                    let desc_va = self.layout.tx_desc_va(slot).map_err(GmacError::Address)?;
                    env.cache_invalidate(desc_va, DESC_BYTES);
                    env.fence_rw();
                    let w3 = read_desc_word(desc_va, 3);
                    if w3 & TDES3_OWN != 0 {
                        if self.cancel_requested {
                            // Post-publish cancel: end the wait, keep the
                            // owner, refuse new submits.
                            self.stop_tx(env);
                            self.state = GmacState::Stopped { reason: "tx-cancelled" };
                            return Err(GmacError::Cancelled { phase: Phase::Poll, after_publish: true });
                        }
                        if env.now_nanos() >= self.tx_deadlines[slot] {
                            self.tx_slots[slot] = TxSlotState::TimedOut;
                            self.tx_head = (slot + 1) % dma::NUM_SLOTS;
                            return Ok(TxPoll::TerminalTimeout { slot, raw_w3: w3 });
                        }
                        return Ok(TxPoll::Pending { slot });
                    }
                    // Own reclaim phase bound (observed after the descriptor
                    // read, before the state transition).
                    if env.now_nanos() >= reclaim_deadline {
                        return Err(GmacError::PhaseTimeout { phase: Phase::Reclaim });
                    }
                    self.tx_slots[slot] = TxSlotState::Free;
                    self.tx_inflight -= 1;
                    self.tx_head = (slot + 1) % dma::NUM_SLOTS;
                    return Ok(if w3 & TDES3_ES != 0 {
                        TxPoll::LocalError { slot, raw_w3: w3 }
                    } else {
                        TxPoll::Completed { slot, raw_w3: w3 }
                    });
                }
            }
        }
        Ok(TxPoll::Idle)
    }
}
// ---------------------------------------------------------------------------
// Seams (probe-style: the real board binds K3Env + CBO, host tests trace)
// ---------------------------------------------------------------------------

/// Board seam for the GMAC driver: GMAC register access (base-relative,
/// bounded by the DT-declared window), the exclusive cache/fence seam and
/// the arena base VA. Extends [`phy_probe::ProbeEnv`] so the driver reuses
/// `phy_probe::mdio_read` unchanged for the link phase.
pub trait GmacEnv: phy_probe::ProbeEnv {
    /// Reads one GMAC register at `offset` from `MAC_BASE_PADDR`.
    fn reg_read(&mut self, offset: u32) -> u32;
    /// Writes one GMAC register at `offset` from `MAC_BASE_PADDR`.
    fn reg_write(&mut self, offset: u32, value: u32);
    /// Flushes (clean+invalidate) a block-granular arena range.
    fn cache_flush(&mut self, va: usize, len: usize);
    /// Cleans (write back) a block-granular arena range.
    fn cache_clean(&mut self, va: usize, len: usize);
    /// Invalidates a block-granular arena range.
    fn cache_invalidate(&mut self, va: usize, len: usize);
    /// Inner cache barrier around cache operations (before flush/clean,
    /// after invalidate).
    fn fence_rw(&mut self);
    /// Full `fence iorw, iorw` before a DMA-publishing doorbell.
    fn fence_iorw(&mut self);
}

/// Backing storage for one 18-page arena. Kernel builds bind a `GlobalPage`
/// wrapper (Drop deallocates); host tests bind a leak-backed witness.
pub trait ArenaBacking {
    /// Base CPU VA of the backing.
    fn base_va(&self) -> usize;
    /// Base PA of the backing (device view, checked by [`ArenaLayout`]).
    fn base_pa(&self) -> u64;
}

/// Arena lifetime guard: before the publish point a drop releases the
/// backing; after it, the backing is retained until external reset — no
/// software path (normal end, fault, cancel, software status clear)
/// deallocates device-owned storage.
pub struct ArenaGuard<B: ArenaBacking> {
    backing: core::mem::ManuallyDrop<B>,
    published: bool,
}

impl<B: ArenaBacking> ArenaGuard<B> {
    /// Wraps freshly allocated (zeroed) backing before any ring register is
    /// written.
    pub fn new(backing: B) -> Self {
        ArenaGuard { backing: core::mem::ManuallyDrop::new(backing), published: false }
    }

    /// Publish point: the caller invokes this after a confirmed reset and
    /// before writing the first ring-base register.
    pub fn publish(&mut self) {
        self.published = true;
    }

    /// Whether the backing is (to be) retained until external reset.
    pub fn is_published(&self) -> bool {
        self.published
    }

    /// Base CPU VA.
    pub fn base_va(&self) -> usize {
        self.backing.base_va()
    }

    /// Base PA.
    pub fn base_pa(&self) -> u64 {
        self.backing.base_pa()
    }
}

impl<B: ArenaBacking> Drop for ArenaGuard<B> {
    fn drop(&mut self) {
        if !self.published {
            // SAFETY: the pre-publish branch runs exactly once; the backing
            // is not device-owned yet, so releasing is correct.
            unsafe { core::mem::ManuallyDrop::drop(&mut self.backing) };
        }
        // Published: retain until external reset (deliberate leak).
    }
}

// ---------------------------------------------------------------------------
// Descriptor memory access (raw, volatile; the arena is exclusive)
// ---------------------------------------------------------------------------

/// SAFETY (all helpers below): `va` comes from a checked [`ArenaLayout`];
/// the arena is exclusive to this driver and accessed through raw volatile
/// word stores/loads only — no Rust references alias the storage.
fn write_desc_word(va: usize, word: usize, value: u32) {
    // SAFETY: checked arena VA, 4-byte aligned word inside the descriptor.
    unsafe { ((va + word * 4) as *mut u32).write_volatile(value) }
}

fn read_desc_word(va: usize, word: usize) -> u32 {
    // SAFETY: checked arena VA, 4-byte aligned word inside the descriptor.
    unsafe { ((va + word * 4) as *const u32).read_volatile() }
}

fn copy_from_buf(buf_va: usize, dst: &mut [u8]) {
    for (i, b) in dst.iter_mut().enumerate() {
        // SAFETY: checked arena VA inside one 2048-byte buffer slot.
        *b = unsafe { ((buf_va + i) as *const u8).read_volatile() };
    }
}

fn copy_padded_to_buf(buf_va: usize, src: &[u8]) {
    for (i, b) in src.iter().enumerate() {
        // SAFETY: checked arena VA inside one 2048-byte buffer slot.
        unsafe { ((buf_va + i) as *mut u8).write_volatile(*b) }
    }
    // Slots are reused: the pad region must be truly zeroed, or a previous
    // frame's bytes leak into the padding of a short frame.
    for i in src.len()..TX_PAD_TO {
        // SAFETY: checked arena VA inside one 2048-byte buffer slot.
        unsafe { ((buf_va + i) as *mut u8).write_volatile(0) }
    }
}

/// Rounds a byte length up to the cache block granularity (the cache seam
/// requires block-granular ranges).
fn block_round_up(len: usize) -> usize {
    (len + CACHE_BLOCK_BYTES - 1) & !(CACHE_BLOCK_BYTES - 1)
}

// ---------------------------------------------------------------------------
// Task 1.1: checked publish/reap sequences (used by init refill and poll)
// ---------------------------------------------------------------------------

/// Publishes RX slot `slot` to the device: fence → flush whole buffer →
/// address/control fields → release fence → OWN last → fence → clean whole
/// descriptor → full fence → tail doorbell with this slot's address.
pub fn raw_refill_rx_desc<E: GmacEnv>(
    env: &mut E,
    l: &ArenaLayout,
    slot: usize,
) -> Result<(), DmaAddressError> {
    let buf_pa = l.rx_buf_pa(slot)?;
    let buf_va = l.rx_buf_va(slot)?;
    let desc_va = l.rx_desc_va(slot)?;

    // Fence before the flush; the whole 2048-byte buffer is flushed so no
    // stale dirty line can overwrite the device's DMA write.
    env.fence_rw();
    check_cache_range(l.base_va(), buf_va, BUFFER_BYTES)?;
    env.cache_flush(buf_va, BUFFER_BYTES);

    // Address/control fields (interrupt-on-completion stays clear).
    write_desc_word(desc_va, 0, addr_lo32(buf_pa));
    write_desc_word(desc_va, 1, addr_hi32(buf_pa));
    write_desc_word(desc_va, 2, 0);
    write_desc_word(desc_va, 3, RDES3_BUF1V);

    // Release fence, then the ownership word last.
    env.fence_rw();
    write_desc_word(desc_va, 3, RDES3_BUF1V | RDES3_OWN);

    // Clean the whole 64-byte descriptor, then the full fence/doorbell.
    env.fence_rw();
    check_cache_range(l.base_va(), desc_va, DESC_BYTES)?;
    env.cache_clean(desc_va, DESC_BYTES);
    env.fence_iorw();
    env.reg_write(regs::DMA_RX_TAIL, addr_lo32(l.rx_desc_pa(slot)?));
    Ok(())
}

/// One raw RX poll step over descriptor `slot`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawRx {
    /// Descriptor still device-owned; nothing else was touched.
    NotOwned,
    /// Legal frame: `len` is the FCS-stripped copy length (60..=1514) and
    /// `raw_w3` is the unmodified descriptor status word.
    Frame { len: usize, raw_w3: u32 },
    /// Classified non-deliverable descriptor; the slot must be refilled.
    Rejected { reason: RxDrop, raw_w3: u32 },
}

/// Reaps RX descriptor `slot`: invalidate descriptor → fence → volatile OWN
/// read (OWN=1 ends here); a legal OWN=0 completion acquires, invalidates
/// the whole buffer, fences and copies the FCS-stripped frame into
/// `scratch` (bounded by the scratch length).
pub fn raw_poll_rx_desc<E: GmacEnv>(
    env: &mut E,
    l: &ArenaLayout,
    slot: usize,
    scratch: &mut [u8],
) -> Result<RawRx, DmaAddressError> {
    let desc_va = l.rx_desc_va(slot)?;
    env.cache_invalidate(desc_va, DESC_BYTES);
    env.fence_rw();
    let w3 = read_desc_word(desc_va, 3);
    if w3 & RDES3_OWN != 0 {
        return Ok(RawRx::NotOwned);
    }
    // Acquire: the device's buffer writes become visible after this fence.
    env.fence_rw();
    match decode_rx(w3) {
        RxCompletion::Frame { raw_len, w3 } => {
            let buf_va = l.rx_buf_va(slot)?;
            env.cache_invalidate(buf_va, BUFFER_BYTES);
            env.fence_rw();
            let copy_len = raw_len as usize - desc::FCS_LEN;
            let n = copy_len.min(scratch.len());
            copy_from_buf(buf_va, &mut scratch[..n]);
            Ok(RawRx::Frame { len: copy_len, raw_w3: w3 })
        }
        RxCompletion::Drop { reason, w3 } => Ok(RawRx::Rejected { reason, raw_w3: w3 }),
    }
}
