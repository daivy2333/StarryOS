use core::task::Waker;

use axdriver::prelude::DevError;
use axdriver_net::NetQueueControl;
use smoltcp::{storage::PacketBuffer, time::Instant, wire::IpAddress};

mod ethernet;
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

pub trait Device: Send + Sync {
    fn name(&self) -> &str;

    /// Advances RX by at most one physical completion.
    fn recv(&mut self, buffer: &mut PacketBuffer<()>, timestamp: Instant) -> RxStep;
    /// Sends a packet to the next hop.
    ///
    /// Returns `true` if this operation resulted in the readiness of receive
    /// operation. This is true for loopback devices and can be used to speed
    /// up packet processing.
    fn send(&mut self, next_hop: IpAddress, packet: &[u8], timestamp: Instant) -> bool;

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
