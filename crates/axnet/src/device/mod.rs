use core::task::Waker;

use axdriver::prelude::DevError;
use axdriver_net::NetQueueControl;
use smoltcp::{storage::PacketBuffer, time::Instant, wire::IpAddress};

mod ethernet;
pub(crate) mod fixed_queue;
mod loopback;
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

    fn register_waker(&self, waker: &Waker);
}
