//! K3 GMAC `axdriver_net` adapter (MS12 Iteration 001, task T3).
//!
//! Binds the async [`Gmac`] (IOC notify surface, Iteration 000) to the
//! transport-neutral `NetDriverOps`/`NetQueueControl`/`NetTxQueue` contracts
//! so the existing axnet `Service` can own it as the single NIC. Ownership
//! shape (design §错误映射 and the trait contracts):
//!
//! - The adapter holds the only [`Gmac`] plus its [`GmacEnv`], a fixed
//!   16×2048 TX CPU-copy pool and a 1×2048 RX staging pool (never DMA
//!   backing), and a 16-entry per-descriptor owner table binding each live
//!   `TxCookie` to the CPU buffer of the actually published slot.
//! - `submit_tx` is `Gmac::try_submit`: pre-accept Full → `Again` (buffer
//!   back to the pool), length/address refusals → `InvalidParam`, other
//!   pre-publish refusals (fatal/cancel/phase deadline) → `Io` with the
//!   buffer released. The boundary is the Gmac's actual publish state: if
//!   OWN went out but the call failed, the buffer is quarantined (retained,
//!   never released) and the adapter latches a stable TX fault.
//! - `reclaim_tx` reclaims at most one: `Completed` returns the cookie
//!   exactly once; `LocalError` releases the terminal CPU buffer and
//!   reports `Io` (the caller faults the owning ticket/flush);
//!   `TerminalTimeout` preserves the descriptor and owner and reports `Io`
//!   — the platform `DevError` has no `TimedOut` variant (axdriver_base
//!   0.1.4-preview.3), so the design's deadline mapping lands on the
//!   generic I/O error; the behavioral content (owner kept, slot never
//!   reused, never a fabricated success) is unchanged. Duplicate/missing
//!   owner identity → `BadState`.
//! - `tx_resource_ledger` derives every count from the actual owner state
//!   (adapter owner table + `Gmac::tx_inflight`), never from the software
//!   slot view; `completions_seen` counts first synchronous observations of
//!   a legal OWN=0 head, independent of reclaim outcomes.
//! - Queue control maps directly onto the Iteration 000 primitives:
//!   `completion_pending`/`arm_notify_and_check` run `check_completions`
//!   (read-only recheck; TX is only reported under a live owner),
//!   `suppress_notify` is `mask_notify` and `arm_notify_and_check` is the
//!   `arm_notify` + recheck sequence. After a fault, submits are refused,
//!   the notify stays closed, legal software resources stay releaseable
//!   and all device-owned backing is retained.

use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

use axdriver_net::{
    BaseDriverOps, DevError, DevResult, DeviceType, EthernetAddress, NetBuf, NetBufBox,
    NetBufPtr, NetBufPool, NetDriverOps, NetQueueControl, NetQueueDirection, NetRecoveryControl,
    NetTxQueue, OwnerSummary, QueueEpoch, RecoveryProgress, RecoveryStage, TxCookie,
    TxResourceLedger,
};

use super::dma::{BUFFER_BYTES, NUM_SLOTS};
use super::{
    ArenaBacking, Gmac, GmacEnv, GmacState, RecoveryRefusal, RecoveryStep, RuntimeRecovery,
    TxPoll, TxSubmit, TxSubmitError, TX_FRAME_MAX, TX_FRAME_MIN,
};
// Same dual binding as the parent module: kernel builds see the kernel
// platform tree, host harnesses see the root-module sibling.
#[cfg(feature = "k3")]
use crate::platform::phy_probe;
#[cfg(not(feature = "k3"))]
use super::phy_probe;

/// Bounded INSR consume (MS13 rework, INTS/INTN ack): the RTL8211F INTN
/// output is a level, deasserted only by a clear-on-read of INSR; the ISR
/// stays MDIO-free (MS10), so the owner's link-read path consumes the
/// latched cause here. A latched link-change means events may postdate the
/// stepped BMSR sample, so the level is re-read (latch-low: the second read
/// is current) and the loop re-arms; three rounds bound a pathological flap
/// storm, and a cause arriving after the final INSR read lowers INTN again
/// and produces a fresh pad edge, so the bound itself never loses an event.
/// A paged failure reports `Io`: an unconsumed INSR would hold INTN low and
/// silence every later event.
fn consume_link_cause<E: phy_probe::ProbeEnv>(env: &mut E, mut up: bool) -> DevResult<bool> {
    for _ in 0..3 {
        let insr = phy_probe::paged_read(env, phy_probe::PHY_PAGE_INSR, phy_probe::PHY_INSR_REG)
            .map_err(|_| DevError::Io)?;
        if insr & phy_probe::PHY_INSR_LINK_CHANGE == 0 {
            return Ok(up);
        }
        let _first = phy_probe::mdio_read(env, phy_probe::PHY_ADDR, phy_probe::BMSR_REG)
            .map_err(|_| DevError::Io)?;
        let second = phy_probe::mdio_read(env, phy_probe::PHY_ADDR, phy_probe::BMSR_REG)
            .map_err(|_| DevError::Io)?;
        up = second & phy_probe::BMSR_LINK != 0;
    }
    Ok(up)
}

/// The adapter carries the board's single-hart contract into the trait
/// world: `BaseDriverOps` requires `Send + Sync`, so the board env/backing
/// pair must be transferable — both faces are.
pub trait AdapterEnv: GmacEnv + Send + Sync {}
impl<E: GmacEnv + Send + Sync> AdapterEnv for E {}
pub trait AdapterBacking: ArenaBacking + Send + Sync {}
impl<B: ArenaBacking + Send + Sync> AdapterBacking for B {}

/// The K3 GMAC device interrupt source (MS10 fixed route; the adapter
/// always reports it so `requires_polling()` stays false).
pub const GMAC_IRQ_SOURCE: usize = 133;

/// RX staging scratch: the largest legal FCS-stripped frame.
const RX_SCRATCH_LEN: usize = TX_FRAME_MAX;

/// One live TX submission: the caller's cookie and the CPU buffer bound to
/// the actually published descriptor slot.
struct TxOwner {
    cookie: TxCookie,
    buf: NetBufBox,
}

struct Core<B: ArenaBacking, E: GmacEnv> {
    drv: Gmac<B>,
    env: E,
    /// TX CPU-copy pool (16×2048): the native carrier behind
    /// `alloc_tx_buffer`, distinct from the DMA arena.
    tx_pool: Arc<NetBufPool>,
    /// RX staging pool (1×2048): frames are copied out of the DMA buffer
    /// into one of these before delivery. The production flow pairs every
    /// `receive` with one `recycle_rx_buffer` under the Service guard, so a
    /// free staging buffer is a caller invariant; `receive` fails loudly
    /// (`NoMemory`, before consuming any descriptor) if it is broken.
    rx_pool: Arc<NetBufPool>,
    /// Per-descriptor owner table: `owners[slot]` is live exactly while the
    /// Gmac slot is device-owned by that submission.
    owners: [Option<TxOwner>; NUM_SLOTS],
    /// Adapter-level stable TX fault (post-publish submission failure):
    /// every later `submit_tx`/`reclaim_tx` reports `Io`; the quarantined
    /// buffers stay retained.
    fault: Option<&'static str>,
    /// Buffers retained by the adapter fault state (never released to the
    /// pool while the fault stands).
    quarantine: Vec<(TxCookie, NetBufBox)>,
    /// First synchronous observations of a legal OWN=0 TX head, counted
    /// independently of what reclaim made of them (RW-2).
    completions_seen: u64,
    /// Reuse scratch for the DMA→CPU frame copy (never aliased).
    rx_scratch: Vec<u8>,
    /// The adapter-owned checked `QueueEpoch` (MS13 T2): advanced once per
    /// recovery at the begin boundary, before any device access. Cookies
    /// are only accepted under the current epoch.
    epoch: QueueEpoch,
    /// The running recovery's target epoch (`None` = Idle). The Gmac owns
    /// the hardware micro-stages; the adapter owns the epoch bookkeeping.
    recovery: Option<QueueEpoch>,
    /// MS13 A4 diagnostics (LOG=info): one-shot flag for the first notify
    /// arm, proving the owner's wait protocol reached the device.
    armed_reported: bool,
    /// MS13 A4 diagnostics (LOG=info): last reported runtime link snapshot,
    /// so a stable link prints once and a change prints once.
    link_reported: Option<bool>,
}

/// The K3 GMAC as an `axdriver_net` NIC. All queue state sits behind one
/// mutex: every caller holds the Service guard (the ISR never touches the
/// driver — Iteration 000 invariant), so the lock never contends and never
/// nests into the Service.
pub struct K3NetAdapter<B: ArenaBacking, E: GmacEnv> {
    core: Mutex<Core<B, E>>,
}

impl<B: ArenaBacking, E: GmacEnv> K3NetAdapter<B, E> {
    /// Wraps an initialized async driver (the board startup runs
    /// `init_rx`/`enable_tx` before `init_network`). Fails closed when the
    /// fixed CPU pools cannot be built.
    pub fn new(drv: Gmac<B>, env: E) -> DevResult<Self> {
        let tx_pool = NetBufPool::new(NUM_SLOTS, BUFFER_BYTES)?;
        let rx_pool = NetBufPool::new(1, BUFFER_BYTES)?;
        Ok(K3NetAdapter {
            core: Mutex::new(Core {
                drv,
                env,
                tx_pool,
                rx_pool,
                owners: core::array::from_fn(|_| None),
                fault: None,
                quarantine: Vec::new(),
                completions_seen: 0,
                rx_scratch: alloc::vec![0u8; RX_SCRATCH_LEN],
                epoch: QueueEpoch::MIN,
                recovery: None,
                armed_reported: false,
                link_reported: None,
            }),
        })
    }

    /// One read-only completion recheck through the shared core.
    fn check_hint(&self) -> DevResult<super::CompletionHint> {
        let mut core = self.core.lock();
        let Core { drv, env, .. } = &mut *core;
        drv.check_completions(env).map_err(map_gmac_err)
    }
}

fn map_gmac_err(e: super::GmacError) -> DevError {
    match e {
        // Address refusals are parameter-shaped (a checked layout only
        // fails on an out-of-contract request).
        super::GmacError::Address(_) => DevError::InvalidParam,
        // Fatal, cancel, phase deadline and terminal timeout all report the
        // generic I/O error; the deadline case keeps its descriptor and
        // owner so the distinction survives in the driver ledger.
        _ => DevError::Io,
    }
}

fn map_hint(directions: NetQueueDirection, hint: super::CompletionHint) -> NetQueueDirection {
    let mut pending = NetQueueDirection::NONE;
    if directions.contains(NetQueueDirection::RX) && hint.rx_ready {
        pending |= NetQueueDirection::RX;
    }
    if directions.contains(NetQueueDirection::TX) && hint.tx_ready {
        pending |= NetQueueDirection::TX;
    }
    pending
}

impl<B: AdapterBacking, E: AdapterEnv> BaseDriverOps for K3NetAdapter<B, E> {
    fn device_name(&self) -> &str {
        "k3-gmac-async"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Net
    }

    fn irq_num(&self) -> Option<usize> {
        Some(GMAC_IRQ_SOURCE)
    }
}

impl<B: AdapterBacking, E: AdapterEnv> NetDriverOps for K3NetAdapter<B, E> {
    fn mac_address(&self) -> EthernetAddress {
        // The address init_rx programmed (02:00:00:00:26:01, R69 order).
        EthernetAddress([0x02, 0x00, 0x00, 0x00, 0x26, 0x01])
    }

    fn can_transmit(&self) -> bool {
        self.core.lock().drv.state() == GmacState::Running
    }

    fn can_receive(&self) -> bool {
        self.core.lock().drv.state() == GmacState::Running
    }

    fn rx_queue_size(&self) -> usize {
        NUM_SLOTS
    }

    fn tx_queue_size(&self) -> usize {
        NUM_SLOTS
    }

    fn queue_control(&mut self) -> Option<&mut dyn NetQueueControl> {
        Some(self)
    }

    fn recovery_control(&mut self) -> Option<&mut dyn NetRecoveryControl> {
        Some(self)
    }

    fn tx_queue(&mut self) -> Option<&mut dyn NetTxQueue> {
        Some(self)
    }

    fn recycle_rx_buffer(&mut self, rx_buf: NetBufPtr) -> DevResult {
        // SAFETY: the pointer came from `receive` and is returned to the
        // staging pool exactly once, matching the receive/recycle pairing.
        drop(unsafe { NetBuf::from_buf_ptr(rx_buf) });
        Ok(())
    }

    fn recycle_tx_buffers(&mut self) -> DevResult {
        // The async data plane has no batch recycle: the queue owner
        // reclaims single-step through `reclaim_tx`. Kept side-effect-free
        // so a legacy polling caller cannot double-reclaim.
        Ok(())
    }

    fn transmit(&mut self, _tx_buf: NetBufPtr) -> DevResult {
        // Fail loud: the K3 data plane submits through `tx_queue` with
        // checked cookies; a raw transmit call means the forbidden polling
        // path reached this adapter.
        Err(DevError::Unsupported)
    }

    fn receive(&mut self) -> DevResult<NetBufPtr> {
        let mut core = self.core.lock();
        let Core { drv, env, rx_pool, rx_scratch, fault, .. } = &mut *core;
        if fault.is_some() {
            return Err(DevError::Io);
        }
        // Reserve the staging buffer BEFORE consuming a descriptor: a legal
        // frame is never reaped with nowhere to land.
        let mut buf = rx_pool.alloc_boxed().ok_or(DevError::NoMemory)?;
        for _ in 0..NUM_SLOTS {
            match drv.poll_rx(env, rx_scratch) {
                Ok(super::RxPoll::Idle) => {
                    drop(buf);
                    return Err(DevError::Again);
                }
                Ok(super::RxPoll::Frame { len, .. }) => {
                    buf.set_packet_len(len);
                    buf.packet_mut().copy_from_slice(&rx_scratch[..len]);
                    return Ok(buf.into_buf_ptr());
                }
                // Classified drop: the slot was already refilled; bounded
                // continue, never fatal.
                Ok(super::RxPoll::Dropped { .. }) => {}
                Err(e) => {
                    drop(buf);
                    return Err(map_gmac_err(e));
                }
            }
        }
        drop(buf);
        Err(DevError::Again)
    }

    fn alloc_tx_buffer(&mut self, size: usize) -> DevResult<NetBufPtr> {
        let core = self.core.lock();
        if !(TX_FRAME_MIN..=TX_FRAME_MAX).contains(&size) {
            return Err(DevError::InvalidParam);
        }
        let mut buf = core.tx_pool.alloc_boxed().ok_or(DevError::Again)?;
        buf.set_packet_len(size);
        Ok(buf.into_buf_ptr())
    }
}

impl<B: AdapterBacking, E: AdapterEnv> NetTxQueue for K3NetAdapter<B, E> {
    fn submit_tx(&mut self, tx_buf: NetBufPtr, cookie: TxCookie) -> DevResult {
        // SAFETY: the pointer was produced by `into_buf_ptr` and ownership
        // moves into this call exactly once.
        let buf = unsafe { NetBuf::from_buf_ptr(tx_buf) };
        let mut core = self.core.lock();
        let Core { drv, env, owners, fault, quarantine, epoch, recovery, .. } = &mut *core;
        // Faulted driver or a running recovery: the refused buffer is a
        // legal software resource and goes back to the pool; the same error
        // repeats.
        if fault.is_some() || recovery.is_some() {
            drop(buf);
            return Err(DevError::Io);
        }
        // Epoch binding: only a cookie minted under the CURRENT epoch is
        // accepted; a stale or future cookie is refused before any device
        // access.
        if cookie.epoch() != *epoch {
            drop(buf);
            return Err(DevError::BadState);
        }
        // Duplicate owner identity: pre-accept invariant breach, nothing
        // touched on the device.
        if owners
            .iter()
            .any(|o| matches!(o, Some(owner) if owner.cookie == cookie))
        {
            drop(buf);
            return Err(DevError::BadState);
        }
        let frame = buf.packet();
        let inflight_before = drv.tx_inflight();
        match drv.try_submit(env, frame) {
            Ok(TxSubmit::Submitted { slot }) => {
                owners[slot] = Some(TxOwner { cookie, buf });
                Ok(())
            }
            // Pre-accept pressure: the pool absorbed the buffer back.
            Ok(TxSubmit::Full) => {
                drop(buf);
                Err(DevError::Again)
            }
            Err(TxSubmitError::LengthInvalid { .. })
            | Err(TxSubmitError::Driver(super::GmacError::Address(_))) => {
                drop(buf);
                Err(DevError::InvalidParam)
            }
            Err(_) => {
                // Pre-publish refusal (fatal/cancel/phase deadline) — unless
                // the Gmac's actual publish state says OWN went out: then
                // this is a post-accept stable fault and the buffer is
                // retained, never released while the fault stands.
                if drv.tx_inflight() > inflight_before {
                    *fault = Some("tx-postpublish");
                    quarantine.push((cookie, buf));
                } else {
                    drop(buf);
                }
                Err(DevError::Io)
            }
        }
    }

    fn reclaim_tx(&mut self) -> DevResult<Option<TxCookie>> {
        let mut core = self.core.lock();
        let Core { drv, env, owners, completions_seen, fault, recovery, .. } = &mut *core;
        if fault.is_some() || recovery.is_some() {
            return Err(DevError::Io);
        }
        match drv.poll_tx(env) {
            Ok(TxPoll::Idle | TxPoll::Pending { .. }) => Ok(None),
            Ok(TxPoll::Completed { slot, .. }) => {
                *completions_seen += 1;
                match owners[slot].take() {
                    // The cookie comes back exactly once; the CPU buffer
                    // returns to the pool with it.
                    Some(owner) => {
                        drop(owner.buf);
                        Ok(Some(owner.cookie))
                    }
                    // A completion with no live owner is ledger drift.
                    None => Err(DevError::BadState),
                }
            }
            Ok(TxPoll::LocalError { slot, .. }) => {
                *completions_seen += 1;
                // The local terminal frees the CPU buffer, but the outcome
                // is never laundered into a success: `Io` faults the owning
                // ticket and wakes the flush waiter upstream.
                if let Some(owner) = owners[slot].take() {
                    drop(owner.buf);
                }
                Err(DevError::Io)
            }
            // Terminal timeout: the descriptor and owner stay preserved (the
            // slot is retired and never reused); see the module docs for the
            // `TimedOut` → `Io` platform mapping.
            Ok(TxPoll::TerminalTimeout { .. }) => Err(DevError::Io),
            Err(e) => Err(map_gmac_err(e)),
        }
    }

    fn tx_resource_ledger(&self) -> Option<TxResourceLedger> {
        let core = self.core.lock();
        let Core { drv, owners, quarantine, completions_seen, .. } = &*core;
        let buffer_inflight = (owners.iter().filter(|o| o.is_some()).count()
            + quarantine.len()) as u64;
        let descriptor_inflight = drv.tx_inflight() as u64;
        Some(TxResourceLedger {
            buffer_available: NUM_SLOTS as u64 - buffer_inflight,
            buffer_inflight,
            descriptor_available: NUM_SLOTS as u64 - descriptor_inflight,
            descriptor_inflight,
            completions_seen: *completions_seen,
        })
    }
}

impl<B: AdapterBacking, E: AdapterEnv> NetQueueControl for K3NetAdapter<B, E> {
    fn completion_pending(&self, directions: NetQueueDirection) -> DevResult<NetQueueDirection> {
        Ok(map_hint(directions, self.check_hint()?))
    }

    fn suppress_notify(&mut self, _directions: NetQueueDirection) -> DevResult {
        // One IRQ_EN word covers both directions: suppressing either closes
        // the device notification (the owner re-arms through
        // `arm_notify_and_check`).
        let mut core = self.core.lock();
        let Core { drv, env, .. } = &mut *core;
        drv.mask_notify(env);
        Ok(())
    }

    fn arm_notify_and_check(
        &mut self,
        directions: NetQueueDirection,
    ) -> DevResult<NetQueueDirection> {
        {
            let core = self.core.lock();
            // A running recovery owns the device: arming an unsafe state is
            // refused, the owner re-arms after the machine finishes.
            if core.recovery.is_some() {
                return Err(DevError::Io);
            }
        }
        let mut core = self.core.lock();
        let Core { drv, env, armed_reported, .. } = &mut *core;
        drv.arm_notify(env, directions.contains(NetQueueDirection::TX));
        // MS13 A4 diagnostics (LOG=info): the first arm proves the owner's
        // wait protocol reached the device notify word.
        if !*armed_reported {
            *armed_reported = true;
            axlog::info!(
                "k3-gmac: owner armed the device notify irq_en={:#x}",
                env.reg_read(super::DMA_CHAN_IRQ_EN),
            );
        }
        let hint = drv.check_completions(env).map_err(map_gmac_err)?;
        Ok(map_hint(directions, hint))
    }

    fn has_rx_completion(&self) -> bool {
        self.check_hint().map(|hint| hint.rx_ready).unwrap_or(false)
    }

    fn suppress_rx_notify(&mut self) -> DevResult {
        self.suppress_notify(NetQueueDirection::RX)
    }

    fn arm_rx_notify_and_check(&mut self) -> DevResult<bool> {
        Ok(self
            .arm_notify_and_check(NetQueueDirection::RX)?
            .contains(NetQueueDirection::RX))
    }
}

impl<B: AdapterBacking, E: AdapterEnv> NetRecoveryControl for K3NetAdapter<B, E> {
    /// MS13 T5: a consistent BMSR link snapshot through the Gmac's stepped
    /// MDIO machine. A running recovery owns the device and reports `Again`
    /// (the caller retries after it commits); a stable fault reports `Io`;
    /// a busy MDIO phase reports `Again` so the bounded cadence retries.
    /// Link transitions never touch the adapter epoch bookkeeping.
    fn read_link_status(&mut self) -> DevResult<bool> {
        let mut core = self.core.lock();
        if core.recovery.is_some() {
            return Err(DevError::Again);
        }
        if core.fault.is_some() {
            return Err(DevError::Io);
        }
        let Core { drv, env, link_reported, .. } = &mut *core;
        match drv.read_link_step(env) {
            Ok(super::LinkReadStep::Pending) => Err(DevError::Again),
            Ok(super::LinkReadStep::Done(up)) => {
                // MS13 rework: consume the PHY's latched cause so INTN
                // returns high (see `consume_link_cause`).
                let up = consume_link_cause(env, up)?;
                // MS13 A4 diagnostics (LOG=info): the runtime snapshot value,
                // printed once for a stable link and once per change.
                if *link_reported != Some(up) {
                    *link_reported = Some(up);
                    axlog::info!("k3-gmac: runtime link snapshot up={}", up);
                }
                Ok(up)
            }
            Err(e) => Err(map_gmac_err(e)),
        }
    }

    /// Observable progress without device access: the adapter's epoch
    /// bookkeeping plus the Gmac's hardware stage.
    fn progress(&self) -> RecoveryProgress {
        let core = self.core.lock();
        let stage = match core.recovery {
            None => RecoveryStage::Idle,
            Some(_) => match core.drv.runtime_recovery() {
                Some(RuntimeRecovery::Resetting) => RecoveryStage::Resetting,
                Some(RuntimeRecovery::Rebuilding) => RecoveryStage::Reinitializing,
                Some(RuntimeRecovery::Failed) => RecoveryStage::Faulted,
                // The Gmac machine finished between adapter steps; the next
                // step lands on Recovered.
                None => RecoveryStage::Recovered,
            },
        };
        RecoveryProgress {
            stage,
            epoch: core.recovery.unwrap_or(core.epoch),
        }
    }

    /// Initiates a recovery from a healthy driver: the checked epoch
    /// advance happens BEFORE any device access (exhaustion fails closed),
    /// and a refused begin leaves nothing touched.
    fn begin_recovery(&mut self) -> DevResult<RecoveryProgress> {
        let mut core = self.core.lock();
        if core.recovery.is_some() {
            return Err(DevError::ResourceBusy);
        }
        if core.fault.is_some() || !matches!(core.drv.state(), GmacState::Running) {
            return Err(DevError::BadState);
        }
        let target = core.epoch.advance().ok_or(DevError::BadState)?;
        match core.drv.runtime_begin_reset() {
            Ok(_) => {
                core.epoch = target;
                core.recovery = Some(target);
                Ok(RecoveryProgress { stage: RecoveryStage::Resetting, epoch: target })
            }
            Err(RecoveryRefusal::Busy) => Err(DevError::ResourceBusy),
            Err(RecoveryRefusal::BadState) => Err(DevError::BadState),
        }
    }

    /// One bounded recovery step. At the confirmed SWR boundary the old TX
    /// carriers die here: their cookies can never return (the owner table
    /// is settled into the quarantine) and their buffers stay retained.
    fn poll_recovery_step(&mut self) -> DevResult<RecoveryProgress> {
        let mut core = self.core.lock();
        let Some(target) = core.recovery else {
            return Err(DevError::BadState);
        };
        let outcome = {
            let Core { drv, env, owners, quarantine, .. } = &mut *core;
            match drv.runtime_poll_step(env) {
                Ok(RecoveryStep::Pending)
                | Ok(RecoveryStep::Stepped { stage: RuntimeRecovery::Resetting }) => Ok(
                    RecoveryProgress { stage: RecoveryStage::Resetting, epoch: target },
                ),
                Ok(RecoveryStep::Stepped { stage: RuntimeRecovery::Rebuilding }) => {
                    for slot in owners.iter_mut() {
                        if let Some(owner) = slot.take() {
                            quarantine.push((owner.cookie, owner.buf));
                        }
                    }
                    Ok(RecoveryProgress {
                        stage: RecoveryStage::Reinitializing,
                        epoch: target,
                    })
                }
                Ok(RecoveryStep::Recovered) => Ok(RecoveryProgress {
                    stage: RecoveryStage::Recovered,
                    epoch: target,
                }),
                // A failed machine re-reports its stable error: the stage
                // stays observable as Faulted, the classification keeps the
                // unconfirmed device-owned carriers.
                Ok(RecoveryStep::Stepped { stage: RuntimeRecovery::Failed }) => Err(DevError::Io),
                Err(_) => Err(DevError::Io),
            }
        };
        if matches!(outcome, Ok(RecoveryProgress { stage: RecoveryStage::Recovered, .. })) {
            // The machine is consumed; the next begin starts from the new
            // epoch.
            core.recovery = None;
        }
        outcome
    }

    /// The fixed 32-carrier conservation view: 16 TX CPU buffers + 16 RX
    /// descriptors. `available` counts free TX buffers, `device_owned` the
    /// 16 RX descriptors plus the live TX carriers, `quarantined` the
    /// buffers retained by a confirmed stop or fault (never released, never
    /// reused).
    fn owner_summary(&self) -> OwnerSummary {
        let core = self.core.lock();
        let live = core.owners.iter().filter(|o| o.is_some()).count() as u64;
        let quarantined = core.quarantine.len() as u64;
        OwnerSummary {
            available: NUM_SLOTS as u64 - live - quarantined,
            device_owned: NUM_SLOTS as u64 + live,
            quarantined,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::Board;

    /// Epoch exhaustion fails closed BEFORE any device access: the checked
    /// advance is the first thing a healthy begin does.
    #[test]
    fn recovery_epoch_exhaustion_is_fail_before_touch() {
        let mut board = Board::new();
        board.env.set_phy_link_1000fd();
        let drv = board.ready_async_driver();
        let script = board.env.handle();
        let mut adapter = K3NetAdapter::new(drv, board.env.handle()).expect("adapter");
        // Private-state boundary: set the epoch directly at MAX (no
        // production seed API exists or is added).
        {
            let mut core = adapter.core.lock();
            core.epoch = QueueEpoch::MAX;
        }
        let writes_at = script.reg_writes().len();
        let control = adapter.recovery_control().unwrap();
        assert!(
            matches!(control.begin_recovery(), Err(DevError::BadState)),
            "exhaustion refuses begin with BadState"
        );
        assert_eq!(
            script.reg_writes().len(),
            writes_at,
            "the exhausted begin touched no device register"
        );
    }
}
