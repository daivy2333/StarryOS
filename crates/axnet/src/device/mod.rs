use core::task::Waker;

use axdriver::prelude::{DevError, DevResult};
use axdriver_net::{NetQueueControl, NetRecoveryControl, QueueEpoch};
use smoltcp::{storage::PacketBuffer, time::Instant, wire::IpAddress};
mod ethernet;
pub(crate) mod fixed_queue;
mod loopback;
#[cfg(test)]
mod test_alloc;
#[cfg(test)]
mod tests;
#[cfg(feature = "vsock")]
mod vsock;

pub use axdriver_net::TxResourceLedger;
pub use ethernet::*;
pub(crate) use fixed_queue::{FlushState, TicketFaultStage, TicketOutcome};
pub use loopback::*;
#[cfg(feature = "vsock")]
pub use vsock::*;

/// Outcome of a single physical RX step.
pub enum RxStep {
    /// No RX completion was available.
    Empty,
    /// One completion was reaped and refilled without delivering an IP packet
    /// to the Router RX buffer.
    Consumed,
    /// One IP packet was delivered to the Router RX buffer.
    Delivered,
    /// A TX obligation owed by the current RX head was deferred because the
    /// device has no TX capacity. The exact head bytes are retained for a
    /// later retry, and the current RX loop must stop for this device so the
    /// same frame is not reprocessed in the same Service poll (Task 3.6).
    Blocked,
    /// A device or queue fault; the error carries the category.
    Fault(DevError),
}

/// Outcome of one raw→RX-slot copy by the queue task (Task 3.2).
#[derive(Debug)]
pub enum RxCopyStep {
    /// No raw completion was available.
    Empty,
    /// One frame was copied into the fixed RX slot.
    Copied,
    /// The RX slot storage is full; nothing was reaped.
    Full,
    /// A raw receive/recycle fault.
    Fault(DevError),
}

/// Outcome of one TX-slot→raw submit by the queue task (Task 3.2).
#[derive(Debug)]
pub enum TxSubmitStep {
    /// No TX slot frame was pending.
    Empty,
    /// One slot frame was submitted to the driver; its slot was popped and
    /// its ticket stays live until the completion is reclaimed.
    Submitted,
    /// The driver is full (`Again`); the slot frame is retained.
    Full,
    /// A raw fault; the slot frame is retained.
    Fault(DevError),
}

/// Outcome of one TX completion reclaim by the queue task (Task 3.2).
#[derive(Debug)]
pub enum TxReclaimStep {
    /// No completion was pending.
    Empty,
    /// One completion was reclaimed and its live ticket released.
    Reclaimed,
    /// A raw fault.
    Fault(DevError),
}

/// Stable reason a logical TX packet was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxDropReason {
    /// The packet is not a well-formed IP packet.
    MalformedIp,
    /// No route matches the destination.
    NoRoute,
    /// The route source does not match the packet source address.
    RouteSourceMismatch,
    /// The target address form is unsupported by the device.
    UnsupportedAddress,
    /// The packet exceeds the device's maximum frame size.
    FrameTooLarge,
}

impl TxDropReason {
    pub(crate) const COUNT: usize = 5;

    pub(crate) fn index(self) -> usize {
        match self {
            TxDropReason::MalformedIp => 0,
            TxDropReason::NoRoute => 1,
            TxDropReason::RouteSourceMismatch => 2,
            TxDropReason::UnsupportedAddress => 3,
            TxDropReason::FrameTooLarge => 4,
        }
    }
}

/// Slot/ticket ledger observed for the V3 diagnostic snapshot (Task 4.2).
///
/// Occupancy, high-water, full/enqueue/dequeue/space counters for both fixed
/// slot rings, plus the live ticket ledger. All fields are read-only
/// observations; devices without slot storage report zeros.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SlotLedger {
    /// RX slot occupancy (live frames).
    pub rx_occupancy: u64,
    /// RX slot high-water mark.
    pub rx_high_water: u64,
    /// RX slot full transitions.
    pub rx_full: u64,
    /// RX slot successful enqueues.
    pub rx_enqueue: u64,
    /// RX slot successful dequeues.
    pub rx_dequeue: u64,
    /// RX slot full→space events.
    pub rx_space_event: u64,
    /// TX slot occupancy (live frames).
    pub tx_occupancy: u64,
    /// TX slot high-water mark.
    pub tx_high_water: u64,
    /// TX slot full transitions.
    pub tx_full: u64,
    /// TX slot successful enqueues.
    pub tx_enqueue: u64,
    /// TX slot successful dequeues.
    pub tx_dequeue: u64,
    /// TX slot full→space events.
    pub tx_space_event: u64,
    /// Live ticket count.
    pub live: u64,
    /// Queued ticket count.
    pub queued: u64,
    /// DeviceOwned ticket count.
    pub device_owned: u64,
    /// Most recently accepted ticket (`u64::MAX` when none).
    pub last_accepted: u64,
}

/// Result of a side-effect-free TX capacity preflight.
#[derive(Debug)]
pub enum TxPreflight {
    /// The device can accept the packet under the current lock scope.
    Ready,
    /// The device has no capacity right now; the Router must keep the head.
    Full,
    /// The packet is deterministically rejected with a stable reason.
    Drop(TxDropReason),
    /// A device or queue fault.
    Fault(DevError),
}

/// Result of committing a preflighted packet to a device.
#[derive(Debug)]
pub enum TxOutcome {
    /// The packet was accepted. `rx_became_ready` reports whether this send
    /// made the receive side ready (true for loopback).
    Accepted { rx_became_ready: bool },
    /// The device had no capacity; the packet was not consumed.
    Full,
    /// The packet was dropped with a stable reason.
    ///
    /// The reason field is consumed by later Iterations (V3 telemetry); the
    /// product dispatch currently maps every non-Accepted commit to a fault.
    #[allow(dead_code)]
    Dropped(TxDropReason),
    /// A device or queue fault.
    Fault(DevError),
}

pub trait Device: Send + Sync {
    fn name(&self) -> &str;

    /// Advances RX by at most one physical completion.
    fn recv(&mut self, buffer: &mut PacketBuffer<()>, timestamp: Instant) -> RxStep;

    /// Side-effect-free TX capacity preflight for `packet` to `next_hop`.
    ///
    /// Must not send, occupy a slot or pending entry, update neighbors, count
    /// a drop or dequeue the packet. Synchronous drivers may recycle already
    /// completed TX buffers to establish [`TxPreflight::Ready`]. A `Ready`
    /// result promises that a following [`Device::send`] under the same lock
    /// scope returns [`TxOutcome::Accepted`]; any other commit result is an
    /// invariant violation and becomes a stable Router fault.
    fn preflight_send(
        &mut self,
        next_hop: IpAddress,
        packet: &[u8],
        timestamp: Instant,
    ) -> TxPreflight;

    /// Commits a preflighted packet to the next hop.
    fn send(&mut self, next_hop: IpAddress, packet: &[u8], timestamp: Instant) -> TxOutcome;

    /// Returns whether this device needs periodic polling to make progress.
    fn requires_polling(&self) -> bool {
        false
    }

    /// Returns the transport-neutral RX queue-control interface, if the
    /// underlying driver supports explicit notification control.
    fn queue_control(&mut self) -> Option<&mut dyn NetQueueControl> {
        None
    }

    /// Switches both raw directions to the fixed slot data path.
    ///
    /// Only the queue task calls this during activation, under the Service
    /// guard, and only after BOTH-direction notification suppression
    /// succeeded. After a successful call the device accepts stack TX into
    /// TX slots and serves stack RX from RX slots; raw driver access becomes
    /// the queue task's alone. Devices without slot storage (e.g. loopback)
    /// return `Ok(())` as a no-op.
    fn activate_slot_mode(&mut self) -> DevResult {
        Ok(())
    }

    /// Advances the raw→RX-slot copy by at most one frame (queue task only).
    ///
    /// Reaps at most one raw completion, copies it into the fixed RX slot and
    /// refills the driver buffer. Returns [`RxCopyStep::Full`] without
    /// reaping when the RX slot storage is full, so a full slot never drops a
    /// completion. Devices without slot storage return `Full` as a no-op.
    fn rx_copy_one(&mut self) -> RxCopyStep {
        RxCopyStep::Full
    }

    /// Advances the TX-slot→raw submit by at most one frame (queue task
    /// only). A `Full` or `Fault` result retains the slot frame.
    fn tx_submit_one(&mut self) -> TxSubmitStep {
        TxSubmitStep::Full
    }

    /// Advances the TX completion reclaim by at most one completion (queue
    /// task only); a reclaimed completion releases its live ticket.
    fn tx_reclaim_one(&mut self) -> TxReclaimStep {
        TxReclaimStep::Fault(DevError::Unsupported)
    }

    /// Whether the fixed RX slot storage currently has room for at least one
    /// complete frame.
    ///
    /// Consulted by the stack after draining RX slots to decide whether the
    /// waiting queue task can resume its RX copy stage. Devices without slot
    /// storage always report space.
    fn rx_slot_has_space(&self) -> bool {
        true
    }

    /// Whether the fixed TX slot storage currently holds at least one frame
    /// waiting to be submitted.
    ///
    /// Consulted by the stack after TX dispatch to decide whether the queue
    /// task should be woken to submit. Devices without slot storage always
    /// report no pending frames.
    fn tx_slot_pending(&self) -> bool {
        false
    }

    /// Most recently accepted TX ticket, used as the D8 flush target source.
    ///
    /// Devices without ticket tracking report `None` (empty data plane).
    fn tx_last_accepted(&self) -> Option<u64> {
        None
    }

    /// Epoch-scoped flush state (Task 2.1): distinguishes a fully-reclaimed
    /// target from a still-pending one and a packet-loss outcome. Devices
    /// without ticket tracking always report `Done`.
    fn tx_flush_state(&self, target: Option<u64>) -> FlushState {
        let _ = target;
        FlushState::Done
    }

    /// The device-reset epoch every live ticket belongs to (Task 2.1). Devices
    /// not participating in recovery report the minimum epoch.
    fn queue_epoch(&self) -> QueueEpoch {
        QueueEpoch::MIN
    }

    /// Cancels every `Queued` ticket of the current epoch as
    /// [`CancelledPreSubmit`](fixed_queue::TicketOutcome::CancelledPreSubmit),
    /// returning the count. Called by the recovery owner under the Service
    /// guard (Task 2.1). Devices without ticket tracking cancel nothing.
    fn tx_cancel_queued(&mut self) -> usize {
        0
    }

    /// Drops every pre-submit packet waiting in the ARP/neighbor pending
    /// storage of the current epoch, returning the count (Task 2.2, F3).
    ///
    /// A recovery linearizes this with `tx_cancel_queued` under the Service
    /// guard so no pending pre-submit packet survives into a new epoch and is
    /// auto-sent after recovery. Devices without neighbor pending storage
    /// cancel nothing.
    fn tx_cancel_pending(&mut self) -> usize {
        0
    }

    /// Number of pre-submit packets still waiting in the ARP/neighbor pending
    /// storage (Task 2.2, F3). Host-test observer proving the recovery gate
    /// rejects new pending and the recovery cancel drained the old ones.
    #[cfg(test)]
    fn tx_pending_len_for_test(&self) -> usize {
        0
    }

    /// Closes every remaining `DeviceOwned` ticket as
    /// [`ResetAborted`](fixed_queue::TicketOutcome::ResetAborted) after a
    /// confirmed reset, returning the count (Task 2.1).
    fn tx_close_device_owned(&mut self) -> usize {
        0
    }

    /// Terminates every `DeviceOwned` ticket as
    /// [`Fault`](fixed_queue::TicketOutcome::Fault) with the committed
    /// bounded `stage` identity on a resident fault without a confirmed
    /// reset, returning the count (Task 2.1 / F4). The driver backing is NOT
    /// released — the recovery holder keeps it quarantined. Devices without
    /// ticket tracking terminate nothing.
    fn tx_fault_device_owned(&mut self, stage: fixed_queue::TicketFaultStage) -> usize {
        let _ = stage;
        0
    }

    /// Advance the software ticket epoch to `next` after a confirmed reset
    /// (Task 2.1). Devices without ticket tracking are a no-op.
    fn tx_advance_epoch(&mut self, next: QueueEpoch) {
        let _ = next;
    }

    /// Access to the device's transport-neutral recovery control (Task 2.2).
    /// Devices whose driver does not support bounded recovery return `None`,
    /// and the recovery owner must then fail closed instead of pretending the
    /// device can recover.
    fn recovery_control(&mut self) -> Option<&mut dyn NetRecoveryControl> {
        None
    }

    /// Sets or clears the recovery I/O gate (Task 2.2). While the device is
    /// gated the send/enqueue path must reject new TX (return `Full`) so no
    /// new Queued ticket is allocated into a data plane being reset. The
    /// recovery owner holds the gate from the moment a recoverable fault is
    /// detected until recovery commits (or permanently on quarantine).
    fn tx_set_recovery_hold(&mut self, held: bool) {
        let _ = held;
    }

    /// Reads a consistent link snapshot from the underlying driver (Task 3.1 /
    /// R6). The driver returns `Again` on a config-generation race; the owner
    /// retains the cause and retries once per later poll. Devices whose driver
    /// exposes no recovery control (and thus no link status) report
    /// `Unsupported`.
    fn read_link_status(&mut self) -> DevResult<bool> {
        match self.recovery_control() {
            Some(control) => control.read_link_status(),
            None => Err(DevError::Unsupported),
        }
    }

    /// Sets or clears the link gate (Task 3.1 / D6). While the link is down
    /// the device rejects new enqueue/submit (returns `Full`); DeviceOwned
    /// completions are still reclaimed. This is an independent gate from the
    /// recovery gate: clearing the link gate never clears a still-active
    /// recovery/fault hold, and the send path rejects while either holds.
    fn tx_set_link_hold(&mut self, held: bool) {
        let _ = held;
    }

    /// Number of DeviceOwned tickets still outstanding on the device (Task
    /// 2.2). The recovery owner consults this during the quiesce drain to
    /// decide when the ledger is stable (all reclaimed) versus still waiting.
    /// Devices without ticket tracking report zero.
    fn tx_device_owned_len(&self) -> u64 {
        0
    }

    /// Slot ledger for the V3 diagnostic snapshot (Task 4.2).
    ///
    /// Devices without fixed slot storage report all zeros.
    fn slot_ledger(&self) -> SlotLedger {
        SlotLedger::default()
    }

    /// Real driver TX resource ledger for the V3 diagnostic snapshot (RW-2).
    ///
    /// Devices whose driver cannot observe buffer/descriptor counts through
    /// the transport-neutral queue interface report `None`; the V3 snapshot
    /// must never synthesize a ledger from slot or ticket capacities.
    fn tx_resource_ledger(&mut self) -> Option<TxResourceLedger> {
        None
    }

    fn register_waker(&self, waker: &Waker);

    /// Host-test observer: number of `recv` attempts through the dormant slot
    /// path (Task 3.6 retry-count witness). Devices without slot storage
    /// report zero.
    #[cfg(test)]
    fn recv_dormant_calls_for_test(&self) -> usize {
        0
    }

    /// Host-test observer: occupied length of the fixed RX slot storage.
    #[cfg(test)]
    fn rx_slot_len_for_test(&self) -> usize {
        0
    }

    /// Host-test observer: bytes at the fixed RX slot head, if any.
    #[cfg(test)]
    fn rx_slot_peek_for_test(&self) -> Option<&[u8]> {
        None
    }

    /// Host-test observer: pops one fixed TX slot to free capacity.
    #[cfg(test)]
    fn pop_tx_slot_for_test(&mut self) -> bool {
        false
    }

    /// Host-test observer: occupied length of the fixed TX slot storage.
    #[cfg(test)]
    fn tx_slot_len_for_test(&self) -> usize {
        0
    }

    /// Host-test observer: number of `tx_submit_one` calls (Task 4.3 holds).
    #[cfg(test)]
    fn tx_submit_calls_for_test(&self) -> usize {
        0
    }
}
