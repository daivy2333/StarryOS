use core::task::Waker;

use axdriver::prelude::{DevError, DevResult};
use axdriver_net::NetQueueControl;
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

pub use ethernet::*;
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

    fn register_waker(&self, waker: &Waker);
}
