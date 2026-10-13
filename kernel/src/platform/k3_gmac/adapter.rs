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
    NetBufPool, NetBufPtr, NetDriverOps, NetQueueControl, NetQueueDirection, NetTxQueue,
    TxCookie, TxResourceLedger,
};

use super::dma::{BUFFER_BYTES, NUM_SLOTS};
use super::{
    ArenaBacking, Gmac, GmacEnv, GmacState, TxPoll, TxSubmit, TxSubmitError, TX_FRAME_MAX,
    TX_FRAME_MIN,
};

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
        let Core { drv, env, owners, fault, quarantine, .. } = &mut *core;
        // Faulted driver: the refused buffer is a legal software resource
        // and goes back to the pool; the same error repeats.
        if fault.is_some() {
            drop(buf);
            return Err(DevError::Io);
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
        let Core { drv, env, owners, completions_seen, fault, .. } = &mut *core;
        if fault.is_some() {
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
        let mut core = self.core.lock();
        let Core { drv, env, .. } = &mut *core;
        drv.arm_notify(env, directions.contains(NetQueueDirection::TX));
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
