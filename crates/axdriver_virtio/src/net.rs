use alloc::{sync::Arc, vec::Vec};

use axdriver_base::{BaseDriverOps, DevError, DevResult, DeviceType};
use axdriver_net::{
    EthernetAddress, NetBuf, NetBufBox, NetBufPool, NetBufPtr, NetDriverOps, NetQueueControl,
    NetQueueDirection, NetRecoveryControl, NetTxQueue, OwnerSummary, QueueEpoch, RecoveryProgress,
    RecoveryStage, TxCookie, TxResourceLedger,
};
use virtio_drivers::{Hal, device::net::VirtIONetRaw as InnerDev, transport::Transport};

use crate::as_dev_err;

const NET_BUF_LEN: usize = 1526;

/// One TX slot. It is either free, owned by the legacy synchronous path, or
/// owned by the queue path with an opaque cookie. A slot moves through the
/// states atomically: one buffer, one owner, one submission, one reclaim.
enum TxSlot {
    Free,
    Legacy(NetBufBox),
    Queue(NetBufBox, TxCookie),
}

/// The adapter's full-device recovery state machine for the exclusive-owner
/// path. `Recovered` and `Faulted` are terminal until a new `begin_recovery`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryState {
    Idle,
    Resetting,
    Reinitializing,
    Recovered,
    Faulted,
}

/// The VirtIO network device driver.
///
/// `QS` is the VirtIO queue size.
pub struct VirtIoNetDev<H: Hal, T: Transport, const QS: usize> {
    rx_buffers: [Option<NetBufBox>; QS],
    tx_slots: [TxSlot; QS],
    free_tx_bufs: Vec<NetBufBox>,
    /// Buffer retained by the driver after a post-accept ownership invariant
    /// broke; it must never return to the allocatable set.
    tx_fault_buf: Option<NetBufBox>,
    /// A buffer submitted for RX recycle while the data plane was inactive
    /// (reset/reinit/fault). Kept as a single driver owner until the device is
    /// dropped so the rejection never loses or duplicates the buffer; a further
    /// replay while one is already on hold returns to the pool.
    rx_recycle_hold: Option<NetBufBox>,
    /// Set once a TX ownership invariant breaks: all later TX operations fail
    /// with a stable [`DevError::BadState`] instead of panicking or reusing
    /// state.
    tx_fault: bool,
    /// RW-2: TX completions the transport has exposed in the used ring,
    /// whether or not the reclaim later succeeded. Lets V3 distinguish
    /// "completion observed" from "completion successfully reclaimed".
    tx_completions_seen: u64,
    /// The current device-reset epoch; advanced on each successful recovery.
    epoch: QueueEpoch,
    /// The epoch a recovery is moving toward, saved at `begin_recovery` so the
    /// progress report is explicit (no silent `unwrap_or` fallback on
    /// exhaustion). Cleared/reset to the current epoch after a recovery outcome.
    target_epoch: QueueEpoch,
    /// The adapter's full-device recovery state machine.
    recovery: RecoveryState,
    buf_pool: Arc<NetBufPool>,
    inner: InnerDev<H, T, QS>,
    irq: Option<usize>,
    /// Test-only seam: when set, the transport reports this token instead of
    /// the descriptor index it actually chose, to exercise post-accept
    /// ownership invariants through the real submit path.
    #[cfg(test)]
    forced_tx_token: Option<u16>,
    /// Test-only seam: when set, the next matching TX completion is reported
    /// as a device error so the stable-fault path is witnessed without
    /// corrupting the used ring. Production builds contain neither field.
    #[cfg(test)]
    forced_completion_failure: bool,
    /// Test-only seam: when set, `refill_all` fails at the Nth pooled-buffer
    /// allocation (0-based), so a partial RX/TX refill after a confirmed reset
    /// can be witnessed without a kernel interrupt source. Production builds do
    /// not contain this field.
    #[cfg(test)]
    refill_fail_at: Option<usize>,
}

unsafe impl<H: Hal, T: Transport, const QS: usize> Send for VirtIoNetDev<H, T, QS> {}
unsafe impl<H: Hal, T: Transport, const QS: usize> Sync for VirtIoNetDev<H, T, QS> {}

impl<H: Hal, T: Transport, const QS: usize> VirtIoNetDev<H, T, QS> {
    /// Creates a new driver instance and initializes the device, or returns
    /// an error if any step fails.
    pub fn try_new(transport: T, irq: Option<usize>) -> DevResult<Self> {
        // 0. Create a new driver instance.
        const NONE_BUF: Option<NetBufBox> = None;
        let inner = InnerDev::new(transport).map_err(as_dev_err)?;
        let rx_buffers = [NONE_BUF; QS];
        let tx_slots = core::array::from_fn(|_| TxSlot::Free);
        let buf_pool = NetBufPool::new(2 * QS, NET_BUF_LEN)?;
        let free_tx_bufs = Vec::with_capacity(QS);

        let mut dev = Self {
            rx_buffers,
            inner,
            tx_slots,
            free_tx_bufs,
            tx_fault_buf: None,
            tx_fault: false,
            rx_recycle_hold: None,
            tx_completions_seen: 0,
            epoch: QueueEpoch::MIN,
            target_epoch: QueueEpoch::MIN,
            recovery: RecoveryState::Idle,
            buf_pool,
            irq,
            #[cfg(test)]
            forced_tx_token: None,
            #[cfg(test)]
            forced_completion_failure: false,
            #[cfg(test)]
            refill_fail_at: None,
        };

        dev.refill_all()?;

        // 3. Return the driver instance.
        Ok(dev)
    }

    /// Refills every RX and TX slot against the current [`Self::inner`].
    ///
    /// Shared by [`Self::try_new`] and the post-reset recovery path so both
    /// drive the same initialization flow. Requires the underlying queues to
    /// already exist; `self.rx_buffers` and `self.free_tx_bufs` must be empty.
    fn refill_all(&mut self) -> DevResult<()> {
        #[cfg(test)]
        let mut alloc_count = 0usize;
        for (i, rx_buf_place) in self.rx_buffers.iter_mut().enumerate() {
            #[cfg(test)]
            if Some(alloc_count) == self.refill_fail_at {
                return Err(DevError::NoMemory);
            }
            #[cfg(test)]
            {
                alloc_count += 1;
            }
            let mut rx_buf = self.buf_pool.alloc_boxed().ok_or(DevError::NoMemory)?;
            // Safe because the buffer lives as long as the queue.
            let token = unsafe {
                self.inner
                    .receive_begin(rx_buf.raw_buf_mut())
                    .map_err(as_dev_err)?
            };
            assert_eq!(token, i as u16);
            *rx_buf_place = Some(rx_buf);
        }

        for _ in 0..QS {
            let mut tx_buf = self.buf_pool.alloc_boxed().ok_or(DevError::NoMemory)?;
            // Fill header
            let hdr_len = self
                .inner
                .fill_buffer_header(tx_buf.raw_buf_mut())
                .or(Err(DevError::InvalidParam))?;
            tx_buf.set_header_len(hdr_len);
            self.free_tx_bufs.push(tx_buf);
        }

        Ok(())
    }

    /// Rebuilds the device and all buffers after a confirmed device reset.
    ///
    /// Runs the transactional recovery: prepare the replacement queues (without
    /// `DRIVER_OK`), release obsolete owners, refill every RX/TX slot against the
    /// replacement queues, and only then commit `DRIVER_OK` and advance the epoch.
    /// The `DRIVER_OK` arm is deferred until the full refill succeeds, so a partial
    /// rebuild or partial refill never leaves an armed device DMAing into a
    /// partially populated queue. On any failure the caller keeps the adapter
    /// faulted with whatever backing it still owns.
    fn recover_after_reset(&mut self) -> DevResult<()> {
        // 1. Prepare replacement queues; DRIVER_OK stays unset.
        self.inner.reinit_prepare().map_err(as_dev_err)?;

        // 2. Release owners that are obsolete now the device has stopped. A
        //    late-RX recycle held during the reset is returned to the pool so
        //    the full `2*QS` refill below always has every buffer available; the
        //    caller that recycled it while the device was inactive has no right
        //    to keep ownership across the reset.
        for slot in &mut self.rx_buffers {
            *slot = None;
        }
        for slot in &mut self.tx_slots {
            *slot = TxSlot::Free;
        }
        self.rx_recycle_hold = None;
        self.free_tx_bufs.clear();
        self.tx_fault_buf = None;
        self.tx_fault = false;
        self.tx_completions_seen = 0;

        // 3. Fill all packet backing against the replacement queues.
        self.refill_all()?;

        // 4. Only now arm the device; a failure above never published DRIVER_OK.
        self.inner.commit_driver_ok().map_err(as_dev_err)?;

        let next = self.epoch.advance().ok_or(DevError::BadState)?;
        self.epoch = next;
        self.target_epoch = next;
        Ok(())
    }
}

/// Maps a TX submission error at the net TX boundary: `QueueFull` is
/// recoverable pressure and maps to `Again`; every other error keeps the shared
/// mapping used by the rest of the driver family.
fn map_tx_submit_error(e: virtio_drivers::Error) -> DevError {
    if matches!(e, virtio_drivers::Error::QueueFull) {
        DevError::Again
    } else {
        as_dev_err(e)
    }
}

/// Recovers a buffer after a pre-accept submit error: the transport never
/// borrowed the buffer, so it returns to the allocatable set unchanged.
fn recover_submit_error(
    free_tx_bufs: &mut Vec<NetBufBox>,
    tx_buf: NetBufBox,
    err: virtio_drivers::Error,
) -> DevError {
    free_tx_bufs.push(tx_buf);
    map_tx_submit_error(err)
}

impl<H: Hal, T: Transport, const QS: usize> VirtIoNetDev<H, T, QS> {
    /// Enters the stable TX fault state: every later TX operation returns
    /// [`DevError::BadState`]. A post-accept `retained` buffer is quarantined
    /// out of the free set, so it is conserved but never reused.
    fn enter_tx_fault(&mut self, retained: Option<NetBufBox>) -> DevError {
        self.tx_fault = true;
        if let Some(buf) = retained {
            debug_assert!(self.tx_fault_buf.is_none());
            self.tx_fault_buf = Some(buf);
        }
        DevError::BadState
    }

    /// Acquires a TX token from the transport. Test builds may forge the token
    /// to prove post-accept invariant handling; production always uses the real
    /// descriptor index chosen by the queue.
    fn begin_transmit(&mut self, tx_buf: &NetBufBox) -> virtio_drivers::Result<u16> {
        #[cfg(test)]
        if let Some(forced) = self.forced_tx_token.take() {
            // The transport really accepts the buffer (a descriptor is
            // consumed); it merely reports `forced` instead of its own index.
            unsafe { self.inner.transmit_begin(tx_buf.packet_with_header()) }?;
            return Ok(forced);
        }
        // SAFETY: `tx_buf` stays in the adapter ledger for as long as the queue.
        unsafe { self.inner.transmit_begin(tx_buf.packet_with_header()) }
    }

    /// Arms a one-shot failure for the next matching TX completion.
    #[cfg(test)]
    fn fail_next_tx_completion(&mut self) {
        self.forced_completion_failure = true;
    }

    /// Hands out the backing pool so a test can prove conservation after a
    /// fault by draining the pool when the device is dropped.
    #[cfg(test)]
    fn buf_pool(&self) -> Arc<NetBufPool> {
        Arc::clone(&self.buf_pool)
    }

    /// Runs the raw TX completion for `token` and reports whether the device
    /// rejected it. The slot and its buffer/cookie stay installed until the
    /// completion is accepted, so a completion error conserves ownership.
    /// Test builds may force one failure; production always executes the real
    /// device completion with no test branch.
    fn tx_completion_failed(&mut self, token: u16) -> bool {
        let buf = match &self.tx_slots[token as usize] {
            TxSlot::Queue(buf, _) => buf,
            TxSlot::Legacy(buf) => buf,
            _ => unreachable!(),
        };
        #[cfg(test)]
        {
            if core::mem::take(&mut self.forced_completion_failure) {
                return true;
            }
        }
        // SAFETY: `buf` is still owned by the installed `TxSlot`; the raw
        // completion consumes only the used entry, not the buffer.
        unsafe {
            self.inner
                .transmit_complete(token, buf.packet_with_header())
                .is_err()
        }
    }
}

impl<H: Hal, T: Transport, const QS: usize> BaseDriverOps for VirtIoNetDev<H, T, QS> {
    fn device_name(&self) -> &str {
        "virtio-net"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Net
    }

    fn irq_num(&self) -> Option<usize> {
        self.irq
    }
}

impl<H: Hal, T: Transport, const QS: usize> NetQueueControl for VirtIoNetDev<H, T, QS> {
    fn completion_pending(&self, directions: NetQueueDirection) -> DevResult<NetQueueDirection> {
        let mut pending = NetQueueDirection::NONE;
        if directions.contains(NetQueueDirection::RX) && self.inner.poll_rx_completion() {
            pending |= NetQueueDirection::RX;
        }
        if directions.contains(NetQueueDirection::TX) && self.inner.poll_tx_completion() {
            pending |= NetQueueDirection::TX;
        }
        Ok(pending)
    }

    fn suppress_notify(&mut self, directions: NetQueueDirection) -> DevResult {
        if directions.contains(NetQueueDirection::RX) {
            self.inner.suppress_rx_notify();
        }
        if directions.contains(NetQueueDirection::TX) {
            self.inner.suppress_tx_notify();
        }
        Ok(())
    }

    fn arm_notify_and_check(
        &mut self,
        directions: NetQueueDirection,
    ) -> DevResult<NetQueueDirection> {
        let mut pending = NetQueueDirection::NONE;
        if directions.contains(NetQueueDirection::RX) && self.inner.arm_rx_notify_and_check() {
            pending |= NetQueueDirection::RX;
        }
        if directions.contains(NetQueueDirection::TX) && self.inner.arm_tx_notify_and_check() {
            pending |= NetQueueDirection::TX;
        }
        Ok(pending)
    }

    fn has_rx_completion(&self) -> bool {
        self.inner.poll_rx_completion()
    }

    fn suppress_rx_notify(&mut self) -> DevResult {
        self.inner.suppress_rx_notify();
        Ok(())
    }

    fn arm_rx_notify_and_check(&mut self) -> DevResult<bool> {
        Ok(self.inner.arm_rx_notify_and_check())
    }
}

impl<H: Hal, T: Transport, const QS: usize> VirtIoNetDev<H, T, QS> {
    /// Whether the data plane is allowed to move packets right now.
    ///
    /// Only a healthy (idle) or freshly recovered device may; an in-flight
    /// reset/reinitialize or a terminal fault must reject new I/O while keeping
    /// its ownership ledger unchanged. `Recovered` counts as active because the
    /// device was rebuilt and is usable again.
    #[inline]
    fn data_plane_active(&self) -> bool {
        matches!(
            self.recovery,
            RecoveryState::Idle | RecoveryState::Recovered
        )
    }

    /// Returns how many TX slots and RX slots are currently installed as owner
    /// entries (device-addressable in an active/pending-reset device). Does not
    /// include the held late-RX buffer, which is always driver-held and handled
    /// separately in [`Self::owner_summary`].
    fn committed_owner_count(&self) -> u64 {
        self.tx_slots
            .iter()
            .filter(|slot| !matches!(slot, TxSlot::Free))
            .count() as u64
            + self.rx_buffers.iter().filter(|b| b.is_some()).count() as u64
    }
}

impl<H: Hal, T: Transport, const QS: usize> NetRecoveryControl for VirtIoNetDev<H, T, QS> {
    fn progress(&self) -> RecoveryProgress {
        let stage = match self.recovery {
            RecoveryState::Idle => RecoveryStage::Idle,
            RecoveryState::Resetting => RecoveryStage::Resetting,
            RecoveryState::Reinitializing => RecoveryStage::Reinitializing,
            RecoveryState::Recovered => RecoveryStage::Recovered,
            RecoveryState::Faulted => RecoveryStage::Faulted,
        };
        // `RecoveryProgress::epoch` is the target epoch the recovery operates
        // on; while resetting or reinitializing, report the saved target rather
        // than silently falling back on the not-yet-advanced current one.
        let epoch = match self.recovery {
            RecoveryState::Resetting | RecoveryState::Reinitializing => self.target_epoch,
            _ => self.epoch,
        };
        RecoveryProgress { stage, epoch }
    }

    fn begin_recovery(&mut self) -> DevResult<RecoveryProgress> {
        // A checked advance before any device touch: if the epoch counter is
        // exhausted, fail closed without a status write, queue mutation, DMA
        // allocation/deallocation or ledger change (fail-before-touch).
        let target = self.epoch.advance().ok_or(DevError::BadState)?;
        // Only a healthy device can start a fresh recovery. A Faulted adapter
        // is a stable quarantined owner: it keeps its backing and the current
        // contract defines no retry policy, so it refuses recovery here (the
        // higher layer must tear it down instead).
        match self.recovery {
            RecoveryState::Idle | RecoveryState::Recovered => {
                self.target_epoch = target;
                self.inner.begin_reset();
                self.recovery = RecoveryState::Resetting;
                Ok(self.progress())
            }
            _ => Err(DevError::BadState),
        }
    }

    fn poll_recovery_step(&mut self) -> DevResult<RecoveryProgress> {
        match self.recovery {
            RecoveryState::Resetting => {
                // Step 1 (bounded): confirm the device stopped. If not yet
                // confirmed, stay pending; once confirmed, mark the rebuild
                // stage so a later poll performs it.
                if !self.inner.reset_confirmed() {
                    return Ok(self.progress());
                }
                self.recovery = RecoveryState::Reinitializing;
                Ok(self.progress())
            }
            RecoveryState::Reinitializing => {
                // Step 2 (bounded): rebuild queues and buffers, or fault.
                match self.recover_after_reset() {
                    Ok(()) => {
                        self.recovery = RecoveryState::Recovered;
                        Ok(self.progress())
                    }
                    Err(e) => {
                        // Preserve the exact bounded fault category so the
                        // higher layer can distinguish why recovery failed.
                        self.recovery = RecoveryState::Faulted;
                        Err(e)
                    }
                }
            }
            _ => Err(DevError::BadState),
        }
    }

    fn owner_summary(&self) -> OwnerSummary {
        // Owner classification follows the *actual* device-access boundary:
        // only a healthy device, or a recovery still waiting for the reset
        // status to read back zero, has the device path able to reach its old
        // owners. Once a reset is confirmed (or the adapter has faulted), the
        // device path stopped and every committed owner is driver-quarantined.
        // The held late-RX buffer is driver-held in *every* phase (it was handed
        // back out of the queue to the caller before recycling), so it is always
        // a quarantined driver owner.
        let committed = self.committed_owner_count();
        let driver_held =
            u64::from(self.tx_fault_buf.is_some()) + u64::from(self.rx_recycle_hold.is_some());
        let reset_pending =
            matches!(self.recovery, RecoveryState::Resetting) && !self.inner.reset_confirmed();
        if self.data_plane_active() || reset_pending {
            OwnerSummary {
                available: self.free_tx_bufs.len() as u64,
                device_owned: committed,
                quarantined: driver_held,
            }
        } else {
            OwnerSummary {
                available: self.free_tx_bufs.len() as u64,
                device_owned: 0,
                quarantined: committed + driver_held,
            }
        }
    }

    fn read_link_status(&mut self) -> DevResult<bool> {
        self.inner.read_link_status().map_err(as_dev_err)
    }
}

impl<H: Hal, T: Transport, const QS: usize> NetDriverOps for VirtIoNetDev<H, T, QS> {
    #[inline]
    fn mac_address(&self) -> EthernetAddress {
        EthernetAddress(self.inner.mac_address())
    }

    #[inline]
    fn can_transmit(&self) -> bool {
        !self.tx_fault
            && self.data_plane_active()
            && !self.free_tx_bufs.is_empty()
            && self.inner.can_send()
    }

    #[inline]
    fn can_receive(&self) -> bool {
        self.data_plane_active() && self.inner.poll_receive().is_some()
    }

    #[inline]
    fn rx_queue_size(&self) -> usize {
        QS
    }

    #[inline]
    fn tx_queue_size(&self) -> usize {
        QS
    }

    fn queue_control(&mut self) -> Option<&mut dyn NetQueueControl> {
        Some(self)
    }

    fn tx_queue(&mut self) -> Option<&mut dyn NetTxQueue> {
        Some(self)
    }

    fn recovery_control(&mut self) -> Option<&mut dyn NetRecoveryControl> {
        Some(self)
    }

    fn recycle_rx_buffer(&mut self, rx_buf: NetBufPtr) -> DevResult {
        let mut rx_buf = unsafe { NetBuf::from_buf_ptr(rx_buf) };
        // During a reset/reinitialize/fault the queue may be being torn down or
        // rebuilt; submitting to it here would mutate a queue whose backing is
        // not ready. Reject without touching the queue and preserve the buffer
        // as a single driver-owned owner, matching the contract that a rejection
        // must not mutate the queue nor lose/double the buffer. If a replay
        // buffer is already on hold, return the new one to the pool so it keeps a
        // single owner.
        if !self.data_plane_active() {
            if self.rx_recycle_hold.is_none() {
                self.rx_recycle_hold = Some(rx_buf);
            } else {
                drop(rx_buf);
            }
            return Err(DevError::BadState);
        }
        // Safe because we take the ownership of `rx_buf` back to `rx_buffers`,
        // it lives as long as the queue.
        let new_token = unsafe {
            self.inner
                .receive_begin(rx_buf.raw_buf_mut())
                .map_err(as_dev_err)?
        };
        // `rx_buffers[new_token]` is expected to be `None` since it was taken
        // away at `Self::receive()` and has not been added back.
        if self.rx_buffers[new_token as usize].is_some() {
            drop(rx_buf);
            return Err(DevError::BadState);
        }
        self.rx_buffers[new_token as usize] = Some(rx_buf);
        Ok(())
    }

    fn recycle_tx_buffers(&mut self) -> DevResult {
        if self.tx_fault || !self.data_plane_active() {
            return Err(DevError::BadState);
        }
        while let Some(token) = self.inner.poll_transmit() {
            // A used-ring completion was exposed regardless of reclaim outcome
            // (RW-2 completion vs reclaim observation).
            self.tx_completions_seen += 1;
            let slot = token as usize;
            if slot >= QS {
                return Err(self.enter_tx_fault(None));
            }
            if !matches!(self.tx_slots[slot], TxSlot::Legacy(_)) {
                return Err(self.enter_tx_fault(None));
            }
            // Keep the ledger intact until the transport has accepted the
            // completion; a completion error therefore conserves both the
            // buffer and its slot instead of dropping either.
            if self.tx_completion_failed(token) {
                return Err(self.enter_tx_fault(None));
            }
            let tx_buf = match core::mem::replace(&mut self.tx_slots[slot], TxSlot::Free) {
                TxSlot::Legacy(buf) => buf,
                _ => unreachable!(),
            };
            self.free_tx_bufs.push(tx_buf);
        }
        Ok(())
    }

    fn transmit(&mut self, tx_buf: NetBufPtr) -> DevResult {
        // 0. prepare tx buffer.
        let tx_buf = unsafe { NetBuf::from_buf_ptr(tx_buf) };
        if self.tx_fault || !self.data_plane_active() {
            self.free_tx_bufs.push(tx_buf);
            return Err(DevError::BadState);
        }
        // 1. transmit packet.
        let token = match self.begin_transmit(&tx_buf) {
            Ok(token) => token,
            Err(err) => return Err(recover_submit_error(&mut self.free_tx_bufs, tx_buf, err)),
        };
        // 2. A post-accept invariant (out-of-range or occupied token) is a
        // stable fatal: the new buffer is retained by the driver and never
        // returned to the free set.
        let slot = token as usize;
        if slot >= QS {
            return Err(self.enter_tx_fault(Some(tx_buf)));
        }
        if !matches!(self.tx_slots[slot], TxSlot::Free) {
            return Err(self.enter_tx_fault(Some(tx_buf)));
        }
        self.tx_slots[slot] = TxSlot::Legacy(tx_buf);
        Ok(())
    }

    fn receive(&mut self) -> DevResult<NetBufPtr> {
        if !self.data_plane_active() {
            return Err(DevError::BadState);
        }
        self.inner.ack_interrupt();
        if let Some(token) = self.inner.poll_receive() {
            let mut rx_buf = self.rx_buffers[token as usize]
                .take()
                .ok_or(DevError::BadState)?;
            // Safe because the buffer lives as long as the queue.
            let (hdr_len, pkt_len) = unsafe {
                self.inner
                    .receive_complete(token, rx_buf.raw_buf_mut())
                    .map_err(as_dev_err)?
            };
            rx_buf.set_header_len(hdr_len);
            rx_buf.set_packet_len(pkt_len);

            Ok(rx_buf.into_buf_ptr())
        } else {
            Err(DevError::Again)
        }
    }

    fn alloc_tx_buffer(&mut self, size: usize) -> DevResult<NetBufPtr> {
        if self.tx_fault || !self.data_plane_active() {
            return Err(DevError::BadState);
        }
        // 0. Allocate a buffer from the queue. Runtime exhaustion is
        // recoverable pressure, so it maps to `Again`; only the initial
        // allocation in `try_new` reports `NoMemory`.
        let mut net_buf = self.free_tx_bufs.pop().ok_or(DevError::Again)?;
        let pkt_len = size;

        // 1. Check if the buffer is large enough.
        let hdr_len = net_buf.header_len();
        if hdr_len + pkt_len > net_buf.capacity() {
            self.free_tx_bufs.push(net_buf);
            return Err(DevError::InvalidParam);
        }
        net_buf.set_packet_len(pkt_len);

        // 2. Return the buffer.
        Ok(net_buf.into_buf_ptr())
    }
}

impl<H: Hal, T: Transport, const QS: usize> NetTxQueue for VirtIoNetDev<H, T, QS> {
    fn submit_tx(&mut self, tx_buf: NetBufPtr, cookie: TxCookie) -> DevResult {
        let tx_buf = unsafe { NetBuf::from_buf_ptr(tx_buf) };
        if cookie.epoch() != self.epoch {
            // Stale-epoch submission is a pre-accept rejection: the buffer is
            // returned to the free set and the device is never touched.
            self.free_tx_bufs.push(tx_buf);
            return Err(DevError::BadState);
        }
        if self.tx_fault || !self.data_plane_active() {
            self.free_tx_bufs.push(tx_buf);
            return Err(DevError::BadState);
        }
        let token = match self.begin_transmit(&tx_buf) {
            Ok(token) => token,
            Err(err) => return Err(recover_submit_error(&mut self.free_tx_bufs, tx_buf, err)),
        };
        let slot = token as usize;
        if slot >= QS {
            return Err(self.enter_tx_fault(Some(tx_buf)));
        }
        if !matches!(self.tx_slots[slot], TxSlot::Free) {
            return Err(self.enter_tx_fault(Some(tx_buf)));
        }
        self.tx_slots[slot] = TxSlot::Queue(tx_buf, cookie);
        Ok(())
    }

    fn reclaim_tx(&mut self) -> DevResult<Option<TxCookie>> {
        if self.tx_fault || !self.data_plane_active() {
            return Err(DevError::BadState);
        }
        let Some(token) = self.inner.poll_transmit() else {
            return Ok(None);
        };
        // The used ring exposed a completion: observe it before any reclaim
        // outcome, so V3 can distinguish completion from successful reclaim
        // (RW-2).
        self.tx_completions_seen += 1;
        let slot = token as usize;
        if slot >= QS {
            return Err(self.enter_tx_fault(None));
        }
        if !matches!(self.tx_slots[slot], TxSlot::Queue(_, _)) {
            return Err(self.enter_tx_fault(None));
        }
        let cookie_epoch = match &self.tx_slots[slot] {
            TxSlot::Queue(_, cookie) => cookie.epoch(),
            _ => unreachable!(),
        };
        if cookie_epoch != self.epoch {
            // The device returned a completion for a slot whose cookie belongs
            // to an older generation: a device/owner drift that must not be
            // counted as a valid reclaim.
            return Err(self.enter_tx_fault(None));
        }
        // Keep the ledger intact until the transport has accepted the
        // completion. A fatal completion error therefore still has one clear
        // owner for both the buffer and cookie.
        if self.tx_completion_failed(token) {
            return Err(self.enter_tx_fault(None));
        }
        let (tx_buf, cookie) = match core::mem::replace(&mut self.tx_slots[slot], TxSlot::Free) {
            TxSlot::Queue(buf, cookie) => (buf, cookie),
            _ => unreachable!(),
        };
        self.free_tx_bufs.push(tx_buf);
        Ok(Some(cookie))
    }

    fn tx_resource_ledger(&self) -> Option<TxResourceLedger> {
        let buffer_available = self.free_tx_bufs.len() as u64;
        let descriptor_available = self.inner.send_available_desc() as u64;
        Some(TxResourceLedger {
            buffer_available,
            // RW-6: buffer inflight counts the actual declared owners — every
            // occupied slot plus any quarantined fault buffer — never the
            // capacity complement. A lost, duplicated or externally-held
            // buffer therefore surfaces as conservation drift instead of
            // being normalized into a passing sum.
            buffer_inflight: self
                .tx_slots
                .iter()
                .filter(|slot| !matches!(slot, TxSlot::Free))
                .count() as u64
                + u64::from(self.tx_fault_buf.is_some()),
            descriptor_available,
            descriptor_inflight: QS as u64 - descriptor_available,
            completions_seen: self.tx_completions_seen,
        })
    }
}

#[cfg(test)]
mod tests {
    use alloc::alloc::{alloc_zeroed, dealloc};
    use core::{
        alloc::Layout,
        cell::Cell,
        ptr::NonNull,
        sync::atomic::{AtomicU16, Ordering},
    };
    use std::{sync::Mutex, thread_local, vec::Vec};

    use virtio_drivers::{
        BufferDirection, Hal, PhysAddr, Result,
        transport::{DeviceStatus, DeviceType, Transport},
    };

    use super::*;

    const QS: usize = 4;

    // Per-test-thread DMA accounting for `TestHal`, so a fail-before-touch
    // witness can prove no DMA allocation/deallocation occurred (thread-local
    // gives each test isolated counts and keeps the default parallel run
    // deterministic).
    thread_local! {
        static HAL_DMA_ALLOCS: Cell<usize> = const { Cell::new(0) };
        static HAL_DMA_DEALLOCS: Cell<usize> = const { Cell::new(0) };
    }

    // Identity-mapped host memory: the driver and the fake device see the same
    // addresses, so the test can write used-ring completions directly.
    #[derive(Debug)]
    struct TestHal;

    unsafe impl Hal for TestHal {
        fn dma_alloc(pages: usize, _direction: BufferDirection) -> (PhysAddr, NonNull<u8>) {
            HAL_DMA_ALLOCS.set(HAL_DMA_ALLOCS.get() + 1);
            assert_ne!(pages, 0);
            let layout = Layout::from_size_align(
                pages * virtio_drivers::PAGE_SIZE,
                virtio_drivers::PAGE_SIZE,
            )
            .unwrap();
            let ptr = unsafe { alloc_zeroed(layout) };
            let ptr = NonNull::new(ptr).unwrap();
            (ptr.as_ptr() as PhysAddr, ptr)
        }

        unsafe fn dma_dealloc(_paddr: PhysAddr, vaddr: NonNull<u8>, pages: usize) -> i32 {
            HAL_DMA_DEALLOCS.set(HAL_DMA_DEALLOCS.get() + 1);
            let layout = Layout::from_size_align(
                pages * virtio_drivers::PAGE_SIZE,
                virtio_drivers::PAGE_SIZE,
            )
            .unwrap();
            unsafe { dealloc(vaddr.as_ptr(), layout) }
            0
        }

        unsafe fn mmio_phys_to_virt(paddr: PhysAddr, _size: usize) -> NonNull<u8> {
            NonNull::new(paddr as *mut u8).unwrap()
        }

        unsafe fn share(buffer: NonNull<[u8]>, _direction: BufferDirection) -> PhysAddr {
            buffer.as_ptr() as *const u8 as PhysAddr
        }

        unsafe fn unshare(_paddr: PhysAddr, _buffer: NonNull<[u8]>, _direction: BufferDirection) {}
    }

    // The virtio net config space the driver reads during init; the layout
    // matches the driver's own private `Config`.
    #[repr(C)]
    struct TestNetConfig {
        mac: [u8; 6],
        status: u16,
        max_virtqueue_pairs: u16,
        mtu: u16,
    }

    // Shared device-side completion state: the transport records each queue's
    // used ring (device_area) during `queue_set`; the retained controller
    // writes used elements into the send queue's used ring, simulating a real
    // device completion without reaching into the adapter.
    struct FakeDeviceState {
        used_rings: [Option<NonNull<u8>>; 2],
        used_idx: [u16; 2],
        observed_status: DeviceStatus,
        defer_reset: bool,
        config_generation: u8,
        fail_reinit: bool,
        fail_recv_reinit: bool,
        bump_generation_on_config_read: bool,
        /// Number of `set_status` calls: counts any device status write, even
        /// one that restores the same value, so a fail-before-touch witness can
        /// prove no status write occurred.
        status_writes: usize,
        /// Number of `queue_set` calls: counts any queue registration mutation
        /// so a fail-before-touch witness can prove no queue change occurred.
        queue_sets: usize,
    }

    // A controller the test keeps after the transport moves into the device.
    // It shares only fake-device state: ring addresses and the write index.
    struct FakeDevice {
        shared: Arc<Mutex<FakeDeviceState>>,
    }

    impl FakeDevice {
        fn new() -> Self {
            FakeDevice {
                shared: Arc::new(Mutex::new(FakeDeviceState {
                    used_rings: [None; 2],
                    used_idx: [0; 2],
                    observed_status: DeviceStatus::empty(),
                    defer_reset: false,
                    config_generation: 0,
                    fail_reinit: false,
                    fail_recv_reinit: false,
                    bump_generation_on_config_read: false,
                    status_writes: 0,
                    queue_sets: 0,
                })),
            }
        }

        fn complete_tx(&self, token: u16, len: u32) {
            self.complete_used(1, token, len);
        }

        fn complete_rx(&self, token: u16, len: u32) {
            self.complete_used(0, token, len);
        }

        /// Publishes a used-ring completion on the given queue index (0 == RX,
        /// 1 == TX) as a real device would, so a test can drive the
        /// queue → adapter → caller ownership transition through the normal
        /// `poll_*`/`receive` path rather than manufacturing caller-owned
        /// buffers directly.
        fn complete_used(&self, queue: usize, token: u16, len: u32) {
            let mut state = self.shared.lock().unwrap();
            let used = state.used_rings[queue].expect("queue not configured");
            let used_idx = state.used_idx[queue];
            // SAFETY: `used` points at the queue's used ring, whose layout is
            // flags(u16) + idx(u16) + used_elems[QS] + used_event(u16); each
            // used elem is {id: u32, len: u32} at offset 4 + 8 * slot.
            unsafe {
                let entry = used.as_ptr().add(4 + 8 * (used_idx as usize % QS)) as *mut u32;
                entry.write_volatile(u32::from(token));
                entry.add(1).write_volatile(len);
                let idx = used.as_ptr().add(2) as *mut AtomicU16;
                (*idx).store(used_idx.wrapping_add(1), Ordering::Release);
            }
            state.used_idx[queue] = used_idx.wrapping_add(1);
        }
    }

    // A minimal in-memory transport. `queue_set` records each queue's used
    // ring (device_area) in the fake device state shared with the controller.
    struct FakeTransport {
        config: TestNetConfig,
        shared: Arc<Mutex<FakeDeviceState>>,
    }

    impl Transport for FakeTransport {
        fn device_type(&self) -> DeviceType {
            DeviceType::Network
        }

        fn read_device_features(&mut self) -> u64 {
            0
        }

        fn write_driver_features(&mut self, _driver_features: u64) {}

        fn max_queue_size(&mut self, _queue: u16) -> u32 {
            QS as u32
        }

        fn notify(&mut self, _queue: u16) {}

        fn get_status(&self) -> DeviceStatus {
            self.shared.lock().unwrap().observed_status
        }

        fn set_status(&mut self, status: DeviceStatus) {
            let mut state = self.shared.lock().unwrap();
            state.status_writes += 1;
            if status.is_empty() {
                if state.defer_reset {
                    return;
                }
                // A confirmed reset releases every queue on the device, so the
                // next `VirtQueue::new` in a reinit sees them unused.
                state.observed_status = DeviceStatus::empty();
                state.used_rings = [None; 2];
                return;
            }
            state.observed_status = status;
        }

        fn set_guest_page_size(&mut self, _guest_page_size: u32) {}

        fn requires_legacy_layout(&self) -> bool {
            false
        }

        fn queue_set(
            &mut self,
            queue: u16,
            _size: u32,
            _descriptors: PhysAddr,
            _driver_area: PhysAddr,
            device_area: PhysAddr,
        ) {
            self.shared.lock().unwrap().used_rings[queue as usize] =
                Some(NonNull::new(device_area as *mut u8).unwrap());
            self.shared.lock().unwrap().queue_sets += 1;
        }

        fn queue_unset(&mut self, queue: u16) {
            self.shared.lock().unwrap().used_rings[queue as usize] = None;
        }

        fn queue_used(&mut self, queue: u16) -> bool {
            let state = self.shared.lock().unwrap();
            // Queue 0 == RECEIVE, queue 1 == TRANSMIT (VirtIO-net layout). A
            // reinit rebuilds transmit first, then receive; `fail_recv_reinit`
            // fails only the receive rebuild so a partial queue rebuild (one
            // new queue allocated, then a failure) can be witnessed.
            state.fail_reinit
                || (state.fail_recv_reinit && queue == 0)
                || state.used_rings[queue as usize].is_some()
        }

        fn ack_interrupt(&mut self) -> bool {
            false
        }

        fn config_generation(&self) -> Option<u8> {
            Some(self.shared.lock().unwrap().config_generation)
        }

        fn config_space<T: 'static>(&self) -> Result<NonNull<T>> {
            let mut state = self.shared.lock().unwrap();
            if state.bump_generation_on_config_read {
                // A real config update may land between the two generation
                // reads of a snapshot; bump once during the read so a mid-read
                // race is observable end-to-end through the adapter.
                state.config_generation = state.config_generation.wrapping_add(1);
            }
            Ok(NonNull::from(&self.config).cast())
        }
    }

    fn test_dev() -> (VirtIoNetDev<TestHal, FakeTransport, QS>, FakeDevice) {
        let device = FakeDevice::new();
        let transport = FakeTransport {
            config: TestNetConfig {
                mac: [0x52, 0x54, 0x00, 0x12, 0x34, 0x56],
                status: 0x1,
                max_virtqueue_pairs: 0,
                mtu: 1500,
            },
            shared: device.shared.clone(),
        };
        let dev = VirtIoNetDev::try_new(transport, None).expect("driver init");
        (dev, device)
    }

    #[test]
    fn queue_full_recovers_buffer_and_maps_to_again() {
        let pool = NetBufPool::new(1, NET_BUF_LEN).unwrap();
        let buffer = pool.alloc_boxed().unwrap();
        let mut free = Vec::new();
        let err = recover_submit_error(&mut free, buffer, virtio_drivers::Error::QueueFull);
        assert!(matches!(err, DevError::Again));
        assert_eq!(free.len(), 1);
    }

    #[test]
    fn real_adapter_round_trips_submit_and_reclaim() {
        let (mut dev, device) = test_dev();
        assert_eq!(dev.free_tx_bufs.len(), QS);

        let buf = dev.alloc_tx_buffer(100).unwrap();
        assert_eq!(dev.free_tx_bufs.len(), QS - 1);
        assert!(dev.can_transmit());

        dev.submit_tx(buf, TxCookie::new(7)).unwrap();
        assert_eq!(dev.free_tx_bufs.len(), QS - 1);
        assert!(matches!(&dev.tx_slots[0], TxSlot::Queue(_, c) if *c == TxCookie::new(7)));

        device.complete_tx(0, 100);
        assert!(dev.can_transmit());

        assert_eq!(dev.reclaim_tx().unwrap(), Some(TxCookie::new(7)));
        assert_eq!(dev.reclaim_tx().unwrap(), None);
        assert_eq!(dev.free_tx_bufs.len(), QS);
        assert!(matches!(dev.tx_slots[0], TxSlot::Free));
    }

    #[test]
    fn repeated_oversize_errors_do_not_shrink_capacity() {
        let (mut dev, device) = test_dev();
        let capacity = dev.free_tx_bufs.len();
        for _ in 0..(2 * QS) {
            assert!(matches!(
                dev.alloc_tx_buffer(2000),
                Err(DevError::InvalidParam)
            ));
            assert_eq!(dev.free_tx_bufs.len(), capacity);
        }
        let buf = dev.alloc_tx_buffer(100).unwrap();
        dev.submit_tx(buf, TxCookie::new(1)).unwrap();
        device.complete_tx(0, 100);
        assert_eq!(dev.reclaim_tx().unwrap(), Some(TxCookie::new(1)));
        assert_eq!(dev.free_tx_bufs.len(), capacity);
    }

    #[test]
    fn runtime_exhaustion_is_again_and_recovers() {
        let (mut dev, device) = test_dev();
        for i in 0..QS {
            let buf = dev.alloc_tx_buffer(100).unwrap();
            dev.submit_tx(buf, TxCookie::new(i as u64)).unwrap();
        }
        assert!(!dev.can_transmit());
        assert!(matches!(dev.alloc_tx_buffer(100), Err(DevError::Again)));

        device.complete_tx(0, 100);
        assert_eq!(dev.reclaim_tx().unwrap(), Some(TxCookie::new(0)));
        assert!(dev.can_transmit());

        let buf = dev.alloc_tx_buffer(100).unwrap();
        dev.submit_tx(buf, TxCookie::new(QS as u64)).unwrap();
        assert_eq!(dev.free_tx_bufs.len(), 0);
    }

    #[test]
    fn legacy_recycle_rejects_queue_owned_token() {
        let (mut dev, device) = test_dev();
        let buf = dev.alloc_tx_buffer(100).unwrap();
        dev.submit_tx(buf, TxCookie::new(3)).unwrap();
        assert!(matches!(&dev.tx_slots[0], TxSlot::Queue(_, c) if *c == TxCookie::new(3)));

        device.complete_tx(0, 100);
        assert!(matches!(dev.recycle_tx_buffers(), Err(DevError::BadState)));
        assert!(!dev.can_transmit());
        assert!(matches!(dev.alloc_tx_buffer(100), Err(DevError::BadState)));
        assert!(matches!(dev.recycle_tx_buffers(), Err(DevError::BadState)));
    }

    #[test]
    fn tx_resource_ledger_reports_real_buffer_and_descriptor_counts() {
        // RW-2: the ledger must come from the real driver state, not a
        // synthesis from ticket/slot capacities. `available + inflight` is
        // exactly the fixed queue size at every step.
        let (mut dev, device) = test_dev();
        let qs = QS as u64;

        // Fresh driver: all buffers and descriptors are available.
        let l0 = dev.tx_resource_ledger().unwrap();
        assert_eq!(l0.buffer_available, qs);
        assert_eq!(l0.buffer_inflight, 0);
        assert_eq!(l0.descriptor_available, qs);
        assert_eq!(l0.descriptor_inflight, 0);
        assert_eq!(l0.buffer_available + l0.buffer_inflight, qs);
        assert_eq!(l0.descriptor_available + l0.descriptor_inflight, qs);

        // Submit one: one buffer and one descriptor move in-flight.
        let buf = dev.alloc_tx_buffer(100).unwrap();
        dev.submit_tx(buf, TxCookie::new(1)).unwrap();
        let l1 = dev.tx_resource_ledger().unwrap();
        assert_eq!(l1.buffer_available, qs - 1);
        assert_eq!(l1.buffer_inflight, 1);
        assert_eq!(l1.descriptor_available, qs - 1);
        assert_eq!(l1.descriptor_inflight, 1);
        assert_eq!(l1.buffer_available + l1.buffer_inflight, qs);

        // Completion observed: the reclaim call observes the used ring before
        // returning the cookie, so completions_seen grows on the same call.
        device.complete_tx(0, 100);
        assert_eq!(dev.reclaim_tx().unwrap(), Some(TxCookie::new(1)));
        let l2 = dev.tx_resource_ledger().unwrap();
        assert_eq!(l2.completions_seen, 1);
        // Reclaim: buffer/descriptor return to available, completion count
        // stays observed.
        let l3 = dev.tx_resource_ledger().unwrap();
        assert_eq!(l3.buffer_available, qs);
        assert_eq!(l3.buffer_inflight, 0);
        assert_eq!(l3.descriptor_available, qs);
        assert_eq!(l3.descriptor_inflight, 0);
        assert_eq!(l3.completions_seen, 1);
    }

    #[test]
    fn tx_resource_ledger_counts_completion_even_when_reclaim_faults() {
        // RW-2: a completion is observed in the used ring even when the
        // reclaim later enters a stable fault; completion and reclaim are
        // independent counters.
        let (mut dev, device) = test_dev();
        let buf = dev.alloc_tx_buffer(100).unwrap();
        dev.submit_tx(buf, TxCookie::new(7)).unwrap();

        device.complete_tx(0, 100);
        dev.fail_next_tx_completion();
        assert!(matches!(dev.reclaim_tx(), Err(DevError::BadState)));
        let ledger = dev.tx_resource_ledger().unwrap();
        assert_eq!(
            ledger.completions_seen, 1,
            "completion observed despite fault"
        );
        assert_eq!(ledger.buffer_inflight, 1, "faulted buffer stays owned");
        assert_eq!(ledger.buffer_available + ledger.buffer_inflight, QS as u64);
    }

    // ── RW-6: buffer owners are counted independently, drift is evidence ──

    #[test]
    fn tx_resource_ledger_exposes_oversized_free_list_drift() {
        // RW-6: a free list holding more than the fixed capacity (a buffer
        // duplicated into the owner set) must be visible as conservation
        // drift, not normalized away by a complement. The complement would
        // underflow here.
        let (mut dev, _device) = test_dev();
        // Push an extra buffer from an unrelated pool into the free list:
        // QS + 1 free, 0 slot owners, no fault owner.
        let spare_pool = NetBufPool::new(1, NET_BUF_LEN).unwrap();
        let extra = spare_pool.alloc_boxed().expect("spare pool has a buffer");
        dev.free_tx_bufs.push(extra);
        let ledger = dev.tx_resource_ledger().unwrap();
        assert_eq!(ledger.buffer_available, QS as u64 + 1);
        assert_eq!(ledger.buffer_inflight, 0);
        assert_ne!(
            ledger.buffer_available + ledger.buffer_inflight,
            QS as u64,
            "oversized free list must surface as drift"
        );
    }

    #[test]
    fn tx_resource_ledger_exposes_lost_owner_drift() {
        // RW-6: a slot owner dropped without returning to the free list is a
        // lost buffer. The complement masks it (inflight = QS - available,
        // sum always QS); independent owner counting must report the gap.
        let (mut dev, _device) = test_dev();
        let buf = dev.alloc_tx_buffer(100).unwrap();
        dev.submit_tx(buf, TxCookie::new(1)).unwrap();
        // Lose the slot owner: replace it with Free and drop the buffer
        // without pushing it back into the free list.
        let lost = core::mem::replace(&mut dev.tx_slots[0], TxSlot::Free);
        drop(lost);
        let ledger = dev.tx_resource_ledger().unwrap();
        assert_eq!(ledger.buffer_available, QS as u64 - 1);
        assert_eq!(ledger.buffer_inflight, 0, "no slot or fault owner remains");
        assert_ne!(
            ledger.buffer_available + ledger.buffer_inflight,
            QS as u64,
            "lost buffer must surface as drift"
        );
    }

    #[test]
    fn tx_resource_ledger_counts_quarantined_fault_owner() {
        // RW-6: a buffer quarantined to tx_fault_buf is a real owner. It must
        // appear in inflight alongside the surviving slot owner, so the two
        // buffers never collide into a synthetic sum.
        let (mut dev, _device) = test_dev();
        let first = dev.alloc_tx_buffer(100).unwrap();
        dev.submit_tx(first, TxCookie::new(1)).unwrap();
        let second = dev.alloc_tx_buffer(100).unwrap();
        dev.forced_tx_token = Some(0);
        assert!(matches!(
            dev.submit_tx(second, TxCookie::new(2)),
            Err(DevError::BadState)
        ));
        assert!(dev.tx_fault_buf.is_some());
        let ledger = dev.tx_resource_ledger().unwrap();
        assert_eq!(ledger.buffer_available, QS as u64 - 2);
        assert_eq!(
            ledger.buffer_inflight, 2,
            "one slot owner + one fault owner"
        );
    }

    #[test]
    fn queue_reclaim_rejects_legacy_owned_token() {
        let (mut dev, device) = test_dev();
        let buf = dev.alloc_tx_buffer(100).unwrap();
        dev.transmit(buf).unwrap();
        assert!(matches!(&dev.tx_slots[0], TxSlot::Legacy(_)));

        device.complete_tx(0, 100);
        assert!(matches!(dev.reclaim_tx(), Err(DevError::BadState)));
        assert!(!dev.can_transmit());
        assert!(matches!(dev.alloc_tx_buffer(100), Err(DevError::BadState)));
    }

    #[test]
    fn duplicate_completion_after_reclaim_is_stable_fatal() {
        let (mut dev, device) = test_dev();
        let buf = dev.alloc_tx_buffer(100).unwrap();
        dev.submit_tx(buf, TxCookie::new(9)).unwrap();
        device.complete_tx(0, 100);
        assert_eq!(dev.reclaim_tx().unwrap(), Some(TxCookie::new(9)));
        assert_eq!(dev.free_tx_bufs.len(), QS);

        device.complete_tx(0, 100);
        assert!(matches!(dev.reclaim_tx(), Err(DevError::BadState)));
        assert_eq!(dev.free_tx_bufs.len(), QS);
        assert!(!dev.can_transmit());
    }

    #[test]
    fn post_accept_occupied_token_is_stable_fatal_not_panic() {
        let (mut dev, _device) = test_dev();
        let first = dev.alloc_tx_buffer(100).unwrap();
        dev.submit_tx(first, TxCookie::new(1)).unwrap();
        assert!(matches!(&dev.tx_slots[0], TxSlot::Queue(_, c) if *c == TxCookie::new(1)));

        let second = dev.alloc_tx_buffer(100).unwrap();
        dev.forced_tx_token = Some(0);
        let err = dev.submit_tx(second, TxCookie::new(2)).unwrap_err();
        assert!(matches!(err, DevError::BadState));
        // The new buffer is quarantined by the driver, not returned to the
        // free set: first is in slot 0, second is the fault owner.
        assert!(dev.tx_fault_buf.is_some());
        assert_eq!(dev.free_tx_bufs.len(), QS - 2);
        assert!(!dev.can_transmit());
    }

    #[test]
    fn post_accept_out_of_range_token_is_stable_fatal_not_panic() {
        let (mut dev, _device) = test_dev();
        let buf = dev.alloc_tx_buffer(100).unwrap();
        dev.forced_tx_token = Some(QS as u16 + 5);
        let err = dev.submit_tx(buf, TxCookie::new(4)).unwrap_err();
        assert!(matches!(err, DevError::BadState));
        assert_eq!(dev.free_tx_bufs.len(), QS - 1);
        assert!(!dev.can_transmit());
    }

    #[test]
    fn queue_completion_error_retains_owner_and_enters_fault() {
        let (mut dev, device) = test_dev();
        let buf = dev.alloc_tx_buffer(100).unwrap();
        dev.submit_tx(buf, TxCookie::new(7)).unwrap();
        assert_eq!(dev.free_tx_bufs.len(), QS - 1);
        assert!(matches!(&dev.tx_slots[0], TxSlot::Queue(_, c) if *c == TxCookie::new(7)));

        device.complete_tx(0, 100);
        dev.fail_next_tx_completion();
        assert!(matches!(dev.reclaim_tx(), Err(DevError::BadState)));
        // The slot still owns the buffer and cookie; the free set is untouched.
        assert!(matches!(&dev.tx_slots[0], TxSlot::Queue(_, c) if *c == TxCookie::new(7)));
        assert_eq!(dev.free_tx_bufs.len(), QS - 1);
        assert!(!dev.can_transmit());
        // Later TX operations observe the stable fault.
        assert!(matches!(dev.alloc_tx_buffer(100), Err(DevError::BadState)));
        assert!(matches!(dev.reclaim_tx(), Err(DevError::BadState)));
    }

    #[test]
    fn legacy_completion_error_retains_owner_and_enters_fault() {
        let (mut dev, device) = test_dev();
        let buf = dev.alloc_tx_buffer(100).unwrap();
        dev.transmit(buf).unwrap();
        assert!(matches!(&dev.tx_slots[0], TxSlot::Legacy(_)));

        device.complete_tx(0, 100);
        dev.fail_next_tx_completion();
        assert!(matches!(dev.recycle_tx_buffers(), Err(DevError::BadState)));
        // The slot still owns the buffer; the free set is untouched.
        assert!(matches!(&dev.tx_slots[0], TxSlot::Legacy(_)));
        assert_eq!(dev.free_tx_bufs.len(), QS - 1);
        assert!(!dev.can_transmit());
        // Later TX operations observe the stable fault.
        assert!(matches!(dev.alloc_tx_buffer(100), Err(DevError::BadState)));
        assert!(matches!(dev.recycle_tx_buffers(), Err(DevError::BadState)));
    }

    // ── Recovery contract (R2/R5/R6): reset deferral, reinit failure, epoch ──

    #[test]
    fn recovery_completes_on_confirmed_reset_and_advances_epoch() {
        let (mut dev, _device) = test_dev();
        let buf = dev.alloc_tx_buffer(100).unwrap();
        dev.submit_tx(buf, TxCookie::with_epoch(dev.epoch, 1))
            .unwrap();
        let epoch_before = dev.epoch;

        // With an immediately-confirmed reset, begin + two bounded polls reach
        // Recovered and advance the epoch: confirm, then rebuild.
        assert_eq!(
            dev.recovery_control()
                .unwrap()
                .begin_recovery()
                .unwrap()
                .stage,
            RecoveryStage::Resetting
        );
        assert_eq!(
            dev.recovery_control()
                .unwrap()
                .poll_recovery_step()
                .unwrap()
                .stage,
            RecoveryStage::Reinitializing
        );
        assert_eq!(
            dev.recovery_control()
                .unwrap()
                .poll_recovery_step()
                .unwrap()
                .stage,
            RecoveryStage::Recovered
        );
        assert_eq!(dev.epoch.current(), epoch_before.current() + 1);
        assert_eq!(dev.free_tx_bufs.len(), QS, "capacity rebuilt");
    }

    #[test]
    fn deferred_reset_keeps_backing_until_confirmed() {
        let (mut dev, device) = test_dev();
        device.shared.lock().unwrap().defer_reset = true;

        let buf = dev.alloc_tx_buffer(100).unwrap();
        dev.submit_tx(buf, TxCookie::with_epoch(dev.epoch, 1))
            .unwrap();
        let epoch_before = dev.epoch;
        let free_before = dev.free_tx_bufs.len();
        let inflight_before = dev.tx_resource_ledger().unwrap().buffer_inflight;

        dev.recovery_control().unwrap().begin_recovery().unwrap();
        for _ in 0..3 {
            assert_eq!(
                dev.recovery_control()
                    .unwrap()
                    .poll_recovery_step()
                    .unwrap()
                    .stage,
                RecoveryStage::Resetting,
                "never-confirmed reset stays pending"
            );
        }
        assert_eq!(dev.epoch, epoch_before, "epoch not advanced while pending");
        assert_eq!(dev.free_tx_bufs.len(), free_before);
        assert_eq!(
            dev.tx_resource_ledger().unwrap().buffer_inflight,
            inflight_before,
            "backing not released while reset pending"
        );

        // The device confirms it stopped; recovery completes in one more step.
        device.shared.lock().unwrap().defer_reset = false;
        device.shared.lock().unwrap().observed_status = DeviceStatus::empty();
        assert_eq!(
            dev.recovery_control()
                .unwrap()
                .poll_recovery_step()
                .unwrap()
                .stage,
            RecoveryStage::Reinitializing
        );
        let done = dev
            .recovery_control()
            .unwrap()
            .poll_recovery_step()
            .unwrap();
        assert_eq!(done.stage, RecoveryStage::Recovered);
        assert_eq!(dev.epoch.current(), epoch_before.current() + 1);
        assert_eq!(dev.free_tx_bufs.len(), QS, "full capacity rebuilt");
    }

    #[test]
    fn reinit_failure_keeps_faulted_owner_and_backing() {
        let (mut dev, device) = test_dev();
        let buf = dev.alloc_tx_buffer(100).unwrap();
        dev.submit_tx(buf, TxCookie::with_epoch(dev.epoch, 1))
            .unwrap();
        let epoch_before = dev.epoch;
        let free_before = dev.free_tx_bufs.len();
        let inflight_before = dev.tx_resource_ledger().unwrap().buffer_inflight;

        device.shared.lock().unwrap().fail_reinit = true;
        dev.recovery_control().unwrap().begin_recovery().unwrap();
        assert_eq!(
            dev.recovery_control()
                .unwrap()
                .poll_recovery_step()
                .unwrap()
                .stage,
            RecoveryStage::Reinitializing,
            "confirmed reset enters the rebuild stage"
        );
        // Reinit fails (the fake transport reports the queue already used), so
        // the exact fault category is preserved in the Err and the adapter
        // settles into Faulted.
        let err = dev
            .recovery_control()
            .unwrap()
            .poll_recovery_step()
            .unwrap_err();
        assert!(matches!(err, DevError::AlreadyExists));
        assert_eq!(
            dev.recovery_control().unwrap().progress().stage,
            RecoveryStage::Faulted
        );
        assert_eq!(dev.epoch, epoch_before, "epoch not advanced on failure");

        let ledger = dev.tx_resource_ledger().unwrap();
        assert_eq!(ledger.buffer_available, free_before as u64);
        assert_eq!(
            ledger.buffer_inflight, inflight_before,
            "inflight preserved"
        );
        assert_eq!(
            ledger.buffer_available + ledger.buffer_inflight,
            QS as u64,
            "no backing leaked or duplicated in the faulted state"
        );
        // A terminal fault refuses further progression and every data-plane
        // entry point rejects new I/O while keeping ownership unchanged.
        assert!(matches!(
            dev.recovery_control().unwrap().poll_recovery_step(),
            Err(DevError::BadState)
        ));
        assert!(matches!(
            dev.recovery_control().unwrap().begin_recovery(),
            Err(DevError::BadState)
        ));
        assert!(!dev.can_transmit());
        assert!(matches!(dev.alloc_tx_buffer(100), Err(DevError::BadState)));
        assert!(matches!(dev.recycle_tx_buffers(), Err(DevError::BadState)));
        assert!(matches!(dev.receive(), Err(DevError::BadState)));
    }

    #[test]
    fn progress_reports_target_epoch_during_reset_reinit() {
        // Finding 2: RecoveryProgress::epoch is the target epoch the recovery
        // moves toward; while resetting or reinitializing it must report the
        // next epoch, not the not-yet-advanced current one.
        let (mut dev, device) = test_dev();
        let epoch_before = dev.epoch;
        device.shared.lock().unwrap().defer_reset = true;
        {
            let control = dev.recovery_control().unwrap();
            control.begin_recovery().unwrap();
            assert_eq!(control.progress().stage, RecoveryStage::Resetting);
            assert_eq!(
                control.progress().epoch.current(),
                epoch_before.current() + 1,
                "during reset the progress reports the target (next) epoch"
            );
        }
        // Confirm the reset, then inspect the reinitializing stage too.
        device.shared.lock().unwrap().defer_reset = false;
        device.shared.lock().unwrap().observed_status = DeviceStatus::empty();
        {
            let control = dev.recovery_control().unwrap();
            assert_eq!(
                control.poll_recovery_step().unwrap().stage,
                RecoveryStage::Reinitializing
            );
            assert_eq!(
                control.progress().epoch.current(),
                epoch_before.current() + 1
            );
        }
        // Once recovered the reported epoch is the advanced current epoch.
        let done = dev
            .recovery_control()
            .unwrap()
            .poll_recovery_step()
            .unwrap();
        assert_eq!(done.stage, RecoveryStage::Recovered);
        assert_eq!(done.epoch.current(), epoch_before.current() + 1);
        assert_eq!(dev.epoch.current(), epoch_before.current() + 1);
    }

    #[test]
    fn partial_rebuild_failure_conserves_backing_and_quarantines() {
        // Finding 5: reinit rebuilds transmit, then fails on receive, so a
        // partial queue rebuild is witnessed. The fault preserves the category,
        // quarantine reclassifies every committed owner, and the pool is
        // conserved exactly when the device is dropped.
        let (mut dev, device) = test_dev();
        let epoch_before = dev.epoch;
        let buf = dev.alloc_tx_buffer(100).unwrap();
        dev.submit_tx(buf, TxCookie::with_epoch(dev.epoch, 1))
            .unwrap();
        let pool = dev.buf_pool();

        device.shared.lock().unwrap().fail_recv_reinit = true;
        dev.recovery_control().unwrap().begin_recovery().unwrap();
        assert_eq!(
            dev.recovery_control()
                .unwrap()
                .poll_recovery_step()
                .unwrap()
                .stage,
            RecoveryStage::Reinitializing
        );
        let err = dev
            .recovery_control()
            .unwrap()
            .poll_recovery_step()
            .unwrap_err();
        assert!(matches!(err, DevError::AlreadyExists));
        assert_eq!(
            dev.recovery_control().unwrap().progress().stage,
            RecoveryStage::Faulted
        );
        assert_eq!(dev.epoch, epoch_before, "epoch not advanced on failure");

        // No owner is device-owned; every committed slot/RX is quarantined.
        let os = dev.recovery_control().unwrap().owner_summary();
        assert_eq!(os.device_owned, 0);
        assert_eq!(os.available, (QS - 1) as u64);
        assert_eq!(os.quarantined, (1 + QS) as u64);
        assert!(!dev.can_transmit());
        assert!(matches!(dev.alloc_tx_buffer(100), Err(DevError::BadState)));

        drop(dev);
        // Hold the drained buffers so they cannot return to the pool mid-loop;
        // exactly `2 * QS` must be recovered: a smaller count is a leak, a
        // larger one a duplicate.
        let mut drained: Vec<_> = Vec::new();
        while let Some(b) = pool.alloc_boxed() {
            drained.push(b);
        }
        assert_eq!(drained.len(), 2 * QS, "partial-rebuild fault conserves");
    }

    #[test]
    fn partial_refill_failure_faults_and_conserves_pool() {
        // Finding 5: after a confirmed reset and successful reinit, the refill
        // fails partway through the RX phase. The adapter faults, reclassifies
        // the partially refilled owner as quarantined, blocks the data plane and
        // conserves every pooled buffer on drop.
        let (mut dev, _device) = test_dev();
        let epoch_before = dev.epoch;
        let buf = dev.alloc_tx_buffer(100).unwrap();
        dev.submit_tx(buf, TxCookie::with_epoch(dev.epoch, 1))
            .unwrap();
        let pool = dev.buf_pool();
        dev.refill_fail_at = Some(1);

        dev.recovery_control().unwrap().begin_recovery().unwrap();
        assert_eq!(
            dev.recovery_control()
                .unwrap()
                .poll_recovery_step()
                .unwrap()
                .stage,
            RecoveryStage::Reinitializing,
            "confirmed reset enters the rebuild stage"
        );
        let err = dev
            .recovery_control()
            .unwrap()
            .poll_recovery_step()
            .unwrap_err();
        assert!(matches!(err, DevError::NoMemory));
        assert_eq!(
            dev.recovery_control().unwrap().progress().stage,
            RecoveryStage::Faulted
        );
        assert_eq!(dev.epoch, epoch_before, "epoch not advanced on failure");

        // Only the one partially refilled RX buffer is held and quarantined.
        let os = dev.recovery_control().unwrap().owner_summary();
        assert_eq!(os.available, 0);
        assert_eq!(os.device_owned, 0);
        assert_eq!(os.quarantined, 1);
        assert!(!dev.can_transmit());
        assert!(matches!(dev.alloc_tx_buffer(100), Err(DevError::BadState)));
        assert!(matches!(dev.receive(), Err(DevError::BadState)));

        drop(dev);
        // Hold the drained buffers so they cannot return to the pool mid-loop;
        // exactly `2 * QS` must be recovered: a smaller count is a leak, a
        // larger one a duplicate.
        let mut drained: Vec<_> = Vec::new();
        while let Some(b) = pool.alloc_boxed() {
            drained.push(b);
        }
        assert_eq!(drained.len(), 2 * QS, "partial-refill fault conserves");
    }

    #[test]
    fn submit_rejects_stale_epoch_cookie_without_poisoning_device() {
        let (mut dev, _device) = test_dev();
        let future = dev.epoch.advance().expect("epoch advances");
        let buf = dev.alloc_tx_buffer(100).unwrap();
        assert!(matches!(
            dev.submit_tx(buf, TxCookie::with_epoch(future, 1)),
            Err(DevError::BadState)
        ));
        assert_eq!(dev.free_tx_bufs.len(), QS, "buffer returned on rejection");
        assert!(dev.can_transmit(), "device not poisoned by stale cookie");
    }

    #[test]
    fn full_recovery_cycle_conserves_resources_and_rebuilds_capacity() {
        let (mut dev, device) = test_dev();
        let epoch_before = dev.epoch;
        for i in 0..QS {
            let buf = dev.alloc_tx_buffer(100).unwrap();
            dev.submit_tx(buf, TxCookie::with_epoch(dev.epoch, i as u64))
                .unwrap();
        }
        assert_eq!(dev.free_tx_bufs.len(), 0, "all tx buffers in flight");

        dev.recovery_control().unwrap().begin_recovery().unwrap();
        assert_eq!(
            dev.recovery_control()
                .unwrap()
                .poll_recovery_step()
                .unwrap()
                .stage,
            RecoveryStage::Reinitializing
        );
        let done = dev
            .recovery_control()
            .unwrap()
            .poll_recovery_step()
            .unwrap();
        assert_eq!(done.stage, RecoveryStage::Recovered);
        assert_eq!(dev.epoch.current(), epoch_before.current() + 1);

        assert_eq!(dev.free_tx_bufs.len(), QS);
        let ledger = dev.tx_resource_ledger().unwrap();
        assert_eq!(ledger.buffer_available + ledger.buffer_inflight, QS as u64);
        assert!(dev.can_transmit());
        let buf = dev.alloc_tx_buffer(100).unwrap();
        dev.submit_tx(buf, TxCookie::with_epoch(dev.epoch, 11))
            .unwrap();
        device.complete_tx(0, 100);
        assert_eq!(
            dev.reclaim_tx().unwrap(),
            Some(TxCookie::with_epoch(dev.epoch, 11))
        );
    }

    #[test]
    fn link_status_reads_through_trait_object() {
        // A generic control path holding only `dyn NetRecoveryControl` can
        // request the link snapshot without reaching into the concrete adapter
        // (Finding 3).
        let (mut dev, device) = test_dev();
        let control = dev.recovery_control().unwrap();
        // TestNetConfig.status bit0 == LINK_UP.
        assert!(control.read_link_status().unwrap());
        // A stable read with an exposed generation still returns the link state.
        device.shared.lock().unwrap().config_generation = 1;
        assert!(control.read_link_status().is_ok());
    }

    #[test]
    fn link_status_mid_read_generation_bump_maps_to_again() {
        // Finding 5: the generation must change *between* the two guaranteed
        // reads of a snapshot, so the mid-read race is proven through the
        // adapter rather than only observed with a pre-raced generation.
        let (mut dev, device) = test_dev();
        device.shared.lock().unwrap().bump_generation_on_config_read = true;
        let control = dev.recovery_control().unwrap();
        assert!(matches!(control.read_link_status(), Err(DevError::Again)),);
        // With the race disabled the same read settles on the stable link state.
        device.shared.lock().unwrap().bump_generation_on_config_read = false;
        assert!(control.read_link_status().unwrap());
    }

    // ── Cycle 001 rework: transactional prepare/refill/commit (1.3-R1) ──

    /// A partial refill failure must not arm the device: `DRIVER_OK` must stay
    /// absent even though the reset was confirmed and the queues rebuilt, so the
    /// device never DMA's into a partially populated replacement queue.
    #[test]
    fn partial_refill_failure_does_not_publish_driver_ok() {
        let (mut dev, _device) = test_dev();
        dev.refill_fail_at = Some(1);
        dev.recovery_control().unwrap().begin_recovery().unwrap();
        assert_eq!(
            dev.recovery_control()
                .unwrap()
                .poll_recovery_step()
                .unwrap()
                .stage,
            RecoveryStage::Reinitializing
        );
        let err = dev
            .recovery_control()
            .unwrap()
            .poll_recovery_step()
            .unwrap_err();
        assert!(matches!(err, DevError::NoMemory));
        assert_eq!(
            dev.recovery_control().unwrap().progress().stage,
            RecoveryStage::Faulted
        );
        // DRIVER_OK must never be published: a partial refill leaves the device
        // unarmed so it cannot DMA into the partially populated queues.
        assert!(
            !dev.inner
                .device_status()
                .contains(virtio_drivers::transport::DeviceStatus::DRIVER_OK),
            "a partial refill must not publish DRIVER_OK"
        );
    }

    /// A successful recovery publishes DRIVER_OK only after the full refill,
    /// m reaching the operative (Recovered) state with full capacity.
    #[test]
    fn successful_recovery_commits_driver_ok_and_advances_epoch() {
        let (mut dev, _device) = test_dev();
        let epoch_before = dev.epoch;
        dev.recovery_control().unwrap().begin_recovery().unwrap();
        assert_eq!(
            dev.recovery_control()
                .unwrap()
                .poll_recovery_step()
                .unwrap()
                .stage,
            RecoveryStage::Reinitializing,
            "confirmed reset enters rebuild"
        );
        // Before the final rebuild+commit step, DRIVER_OK is not yet published.
        assert!(
            !dev.inner
                .device_status()
                .contains(virtio_drivers::transport::DeviceStatus::DRIVER_OK),
            "DRIVER_OK must not be armed before the full refill commits"
        );
        let done = dev
            .recovery_control()
            .unwrap()
            .poll_recovery_step()
            .unwrap();
        assert_eq!(done.stage, RecoveryStage::Recovered);
        assert!(
            dev.inner
                .device_status()
                .contains(virtio_drivers::transport::DeviceStatus::DRIVER_OK),
            "after a full refill the device is armed"
        );
        assert_eq!(dev.epoch.current(), epoch_before.current() + 1);
        assert_eq!(dev.free_tx_bufs.len(), QS, "full capacity rebuilt");
    }

    // ── Cycle 001 rework: phase-aware ownership (1.3-R2) ──

    /// While a reset is pending (status not yet read back zero), the old owners
    /// may still be device-accessible and must be conservatively reported as
    /// `device_owned`, not as driver-only quarantine.
    #[test]
    fn owner_summary_keeps_device_owned_during_unconfirmed_reset() {
        let (mut dev, device) = test_dev();
        let buf = dev.alloc_tx_buffer(100).unwrap();
        dev.submit_tx(buf, TxCookie::with_epoch(dev.epoch, 1))
            .unwrap();
        device.shared.lock().unwrap().defer_reset = true;

        dev.recovery_control().unwrap().begin_recovery().unwrap();
        assert_eq!(
            dev.recovery_control().unwrap().progress().stage,
            RecoveryStage::Resetting
        );
        assert!(
            !dev.inner.reset_confirmed(),
            "reset still pending in the deferred phase"
        );
        // Old committed owners may still be reached by the device: report them
        // as device_owned, not as driver-only quarantine.
        let os = dev.recovery_control().unwrap().owner_summary();
        assert_eq!(
            os.device_owned,
            (1 + QS) as u64,
            "old owners stay device-owned unpconfirmed"
        );
        assert_eq!(os.quarantined, 0);
    }

    /// After the reset is confirmed and the device path stopped, the committed
    /// owners become driver-quarantined (not device-owned) until they are
    /// replaced by the rebuild/refill.
    #[test]
    fn owner_summary_quarantines_after_confirmed_reset() {
        let (mut dev, device) = test_dev();
        let buf = dev.alloc_tx_buffer(100).unwrap();
        dev.submit_tx(buf, TxCookie::with_epoch(dev.epoch, 1))
            .unwrap();
        // Force a fault by failing the receive-queue rebuild, so the adapter
        // settles in Faulted with reset confirmed and owners quarantined.
        device.shared.lock().unwrap().fail_reinit = true;
        dev.recovery_control().unwrap().begin_recovery().unwrap();
        assert_eq!(
            dev.recovery_control()
                .unwrap()
                .poll_recovery_step()
                .unwrap()
                .stage,
            RecoveryStage::Reinitializing
        );
        assert!(
            dev.recovery_control()
                .unwrap()
                .poll_recovery_step()
                .is_err()
        );
        assert_eq!(
            dev.recovery_control().unwrap().progress().stage,
            RecoveryStage::Faulted
        );
        let os = dev.recovery_control().unwrap().owner_summary();
        assert_eq!(
            os.device_owned, 0,
            "no owner is device-owned after confirmed reset"
        );
        assert_eq!(
            os.quarantined,
            (1 + QS) as u64,
            "all committed owners are quarantined"
        );
    }

    /// A late RX recycle carried across a *deferred* reset (caller receives a
    /// buffer, reset begins, then the caller recycles while the device is still
    /// Resetting) must not break recovery: the held buffer is released before
    /// the refill, so the recovery converges to a full-capacity `Recovered`
    /// rather than wrongly faulting on a transient `NoMemory`. The caller-owned
    /// buffer comes from a real used-ring completion + `receive()`, so the
    /// queue → adapter → caller ownership transition is the one production
    /// creates.
    #[test]
    fn recycle_during_resetting_converges_to_recovered_full_capacity() {
        let (mut dev, device) = test_dev();
        let pool = dev.buf_pool();
        device.shared.lock().unwrap().defer_reset = true;

        // A device RX completion hands a buffer to the caller through the normal
        // poll path before the reset begins.
        device.complete_rx(0, NET_BUF_LEN as u32);
        let received = dev.receive().expect("receive returns the completed frame");

        dev.recovery_control().unwrap().begin_recovery().unwrap();
        assert_eq!(
            dev.recovery_control().unwrap().progress().stage,
            RecoveryStage::Resetting
        );
        // The caller recycles the received buffer into an inactive data plane
        // mid-reset; ownership is held until recovery releases it.
        assert!(matches!(
            dev.recycle_rx_buffer(received),
            Err(DevError::BadState)
        ));
        assert!(
            dev.rx_recycle_hold.is_some(),
            "late recycle held during reset"
        );

        // Now the device confirms it stopped; recovery proceeds and must converge.
        device.shared.lock().unwrap().defer_reset = false;
        device.shared.lock().unwrap().observed_status = DeviceStatus::empty();
        assert_eq!(
            dev.recovery_control()
                .unwrap()
                .poll_recovery_step()
                .unwrap()
                .stage,
            RecoveryStage::Reinitializing
        );
        let done = dev
            .recovery_control()
            .unwrap()
            .poll_recovery_step()
            .unwrap();
        assert_eq!(
            done.stage,
            RecoveryStage::Recovered,
            "recovery must converge to Recovered despite the mid-reset recycle"
        );
        assert!(
            dev.rx_recycle_hold.is_none(),
            "held buffer released on recovery"
        );
        assert_eq!(dev.free_tx_bufs.len(), QS, "full TX capacity rebuilt");
        assert_eq!(dev.rx_buffers.iter().filter(|b| b.is_some()).count(), QS);

        drop(dev);
        let mut drained: Vec<_> = Vec::new();
        while let Some(b) = pool.alloc_boxed() {
            drained.push(b);
        }
        assert_eq!(drained.len(), 2 * QS, "recovery conserves the full pool");
    }

    /// The same interleave when the recycle lands on the *Reinitializing* phase
    /// (reset confirmed, rebuild in progress) must also converge to `Recovered`
    /// with the held buffer released and the pool conserved. The caller-owned
    /// buffer again crosses the real queue → adapter → caller boundary.
    #[test]
    fn recycle_during_reinitializing_converges_to_recovered_full_capacity() {
        let (mut dev, device) = test_dev();
        let pool = dev.buf_pool();
        device.complete_rx(0, NET_BUF_LEN as u32);
        let received = dev.receive().expect("receive returns the completed frame");

        dev.recovery_control().unwrap().begin_recovery().unwrap();
        assert_eq!(
            dev.recovery_control()
                .unwrap()
                .poll_recovery_step()
                .unwrap()
                .stage,
            RecoveryStage::Reinitializing
        );

        assert!(matches!(
            dev.recycle_rx_buffer(received),
            Err(DevError::BadState)
        ));
        assert!(
            dev.rx_recycle_hold.is_some(),
            "late recycle held during reinitializing"
        );

        let done = dev
            .recovery_control()
            .unwrap()
            .poll_recovery_step()
            .unwrap();
        assert_eq!(done.stage, RecoveryStage::Recovered);
        assert!(
            dev.rx_recycle_hold.is_none(),
            "held buffer released on recovery"
        );
        assert_eq!(dev.free_tx_bufs.len(), QS);

        drop(dev);
        let mut drained: Vec<_> = Vec::new();
        while let Some(b) = pool.alloc_boxed() {
            drained.push(b);
        }
        assert_eq!(drained.len(), 2 * QS, "recovery conserves the full pool");
    }

    /// A late RX recycle in a *Faulted* adapter must reject without touching the
    /// queue, preserve the buffer as a single driver owner, and conserve the full
    /// pool exactly once when the device is dropped. The caller-owned buffer is
    /// obtained through a real used-ring completion + `receive()` before the
    /// adapter faults.
    #[test]
    fn recycle_during_faulted_conserves_pool_on_drop() {
        let (mut dev, device) = test_dev();
        // A receive hands a buffer to the caller through the normal poll path.
        device.complete_rx(0, NET_BUF_LEN as u32);
        let received = dev.receive().expect("receive returns the completed frame");

        let pool = dev.buf_pool();
        dev.refill_fail_at = Some(1);
        dev.recovery_control().unwrap().begin_recovery().unwrap();
        assert_eq!(
            dev.recovery_control()
                .unwrap()
                .poll_recovery_step()
                .unwrap()
                .stage,
            RecoveryStage::Reinitializing
        );
        assert!(
            dev.recovery_control()
                .unwrap()
                .poll_recovery_step()
                .is_err()
        );
        assert_eq!(
            dev.recovery_control().unwrap().progress().stage,
            RecoveryStage::Faulted
        );

        // The caller now recycles its received buffer into the inactive data
        // plane.
        let queued_before = dev.rx_buffers.iter().filter(|b| b.is_some()).count();
        assert!(matches!(
            dev.recycle_rx_buffer(received),
            Err(DevError::BadState)
        ));
        let queued_after = dev.rx_buffers.iter().filter(|b| b.is_some()).count();
        assert_eq!(
            queued_after, queued_before,
            "the rejected recycle must not touch the RX queue"
        );
        assert!(
            dev.rx_recycle_hold.is_some(),
            "late-recycled buffer preserved as one driver owner"
        );
        // The held buffer is visible in the owner summary as driver-quarantined.
        let os = dev.recovery_control().unwrap().owner_summary();
        assert_eq!(
            os.quarantined,
            queued_before as u64 + 1,
            "held recycle buffer counted in the quarantined owner set"
        );

        drop(dev);
        let mut drained: Vec<_> = Vec::new();
        while let Some(b) = pool.alloc_boxed() {
            drained.push(b);
        }
        assert_eq!(
            drained.len(),
            2 * QS,
            "faulted adapter conserves every pooled buffer exactly once"
        );
    }

    /// In the active `Recovered` state, a normal recycle must succeed: it calls
    /// `receive_begin` and restores the RX slot so the device is back to full
    /// receive capacity.
    #[test]
    fn recycle_during_recovered_restores_rx_slot() {
        let (mut dev, device) = test_dev();
        // Complete a recovery to reach an active Recovered device.
        dev.recovery_control().unwrap().begin_recovery().unwrap();
        assert_eq!(
            dev.recovery_control()
                .unwrap()
                .poll_recovery_step()
                .unwrap()
                .stage,
            RecoveryStage::Reinitializing
        );
        assert_eq!(
            dev.recovery_control()
                .unwrap()
                .poll_recovery_step()
                .unwrap()
                .stage,
            RecoveryStage::Recovered
        );
        assert!(dev.data_plane_active(), "Recovered device is active");

        // A normal receive + recycle round trips through the active data plane.
        device.complete_rx(0, NET_BUF_LEN as u32);
        let received = dev.receive().expect("receive returns a frame");
        let queued_before = dev.rx_buffers.iter().filter(|b| b.is_some()).count();
        assert!(dev.recycle_rx_buffer(received).is_ok());
        let queued_after = dev.rx_buffers.iter().filter(|b| b.is_some()).count();
        assert_eq!(
            queued_after,
            queued_before + 1,
            "an active recycle re-arms the RX slot"
        );
        assert!(
            dev.rx_recycle_hold.is_none(),
            "active recycle does not hold the buffer"
        );
    }

    // ── Cycle 001 rework: epoch exhaustion fail-before-touch (1.2/1.3-R3) ──

    /// `begin_recovery` at `QueueEpoch::MAX` must fail before any device touch:
    /// no status write, queue mutation, DMA alloc/dealloc or ledger change. The
    /// fake device counts status writes and queue registrations, and the test
    /// snapshots DMA allocation, the owner summary and the observable TX/RX
    /// resource ledger, so a write that restores the same value or any resource
    /// change would still be detected.
    #[test]
    fn begin_recovery_at_max_epoch_fails_before_device_touch() {
        let (mut dev, device) = test_dev();
        dev.epoch = QueueEpoch::MAX;
        dev.target_epoch = QueueEpoch::MAX;
        let status_before = dev.inner.device_status();
        let stage_before = dev.recovery;
        // Snapshot the two DMA counters independently, not just their net live
        // difference: one allocation plus one deallocation would leave the
        // alloc/dealloc difference unchanged, so only comparing the net value
        // could hide a device/ledger side effect.
        let dma_allocs_before = HAL_DMA_ALLOCS.get();
        let dma_deallocs_before = HAL_DMA_DEALLOCS.get();
        let owner_before = dev.recovery_control().unwrap().owner_summary();
        let ledger_before = dev.tx_resource_ledger();
        let rx_queued_before = dev.rx_buffers.iter().filter(|b| b.is_some()).count();
        let tx_occupied_before = dev
            .tx_slots
            .iter()
            .filter(|s| !matches!(s, TxSlot::Free))
            .count();

        // Side-effect counters: any of these changing would mean recovery began
        // to touch the device/ledger before the exhaustion rejection landed.
        let (status_writes_before, queue_sets_before) = {
            let s = device.shared.lock().unwrap();
            (s.status_writes, s.queue_sets)
        };

        let control = dev.recovery_control().unwrap();
        assert!(matches!(control.begin_recovery(), Err(DevError::BadState)));

        // No state transition, no status write, no queue registration, no DMA
        // allocation/deallocation, no owner/resource-ledger change.
        assert_eq!(
            dev.recovery, stage_before,
            "state unchanged by fail-before-touch"
        );
        assert_eq!(
            dev.inner.device_status(),
            status_before,
            "no status write before exhaustion rejection"
        );
        assert_eq!(dev.epoch, QueueEpoch::MAX, "epoch unchanged");
        assert_eq!(dev.target_epoch, QueueEpoch::MAX, "target unchanged");
        assert_eq!(
            HAL_DMA_ALLOCS.get(),
            dma_allocs_before,
            "no DMA allocation before exhaustion rejection"
        );
        assert_eq!(
            HAL_DMA_DEALLOCS.get(),
            dma_deallocs_before,
            "no DMA deallocation before exhaustion rejection"
        );
        assert_eq!(
            dev.recovery_control().unwrap().owner_summary(),
            owner_before,
            "owner summary unchanged"
        );
        assert_eq!(
            dev.tx_resource_ledger(),
            ledger_before,
            "observable TX/RX resource ledger unchanged"
        );
        assert_eq!(
            dev.rx_buffers.iter().filter(|b| b.is_some()).count(),
            rx_queued_before,
            "RX slots unchanged"
        );
        assert_eq!(
            dev.tx_slots
                .iter()
                .filter(|s| !matches!(s, TxSlot::Free))
                .count(),
            tx_occupied_before,
            "TX slots unchanged"
        );

        let (status_writes_after, queue_sets_after) = {
            let s = device.shared.lock().unwrap();
            (s.status_writes, s.queue_sets)
        };
        assert_eq!(
            status_writes_after, status_writes_before,
            "no device status write before exhaustion rejection"
        );
        assert_eq!(
            queue_sets_after, queue_sets_before,
            "no queue registration before exhaustion rejection"
        );
    }

    /// A normal (non-exhausted) recovery still advances the epoch exactly once
    /// and reports an explicit target epoch during reset/reinit.
    #[test]
    fn non_exhausted_recovery_advances_epoch_exactly_once() {
        let (mut dev, _device) = test_dev();
        let epoch_before = dev.epoch;
        let control = dev.recovery_control().unwrap();
        let started = control.begin_recovery().unwrap();
        assert_eq!(started.epoch.current(), epoch_before.current() + 1);
        assert_eq!(
            control.progress().epoch.current(),
            epoch_before.current() + 1,
            "explicit target epoch reported during reset"
        );
        assert_eq!(
            control.poll_recovery_step().unwrap().stage,
            RecoveryStage::Reinitializing
        );
        let done = control.poll_recovery_step().unwrap();
        assert_eq!(done.stage, RecoveryStage::Recovered);
        assert_eq!(
            done.epoch.current(),
            epoch_before.current() + 1,
            "recovered epoch advanced exactly once"
        );
    }
}
