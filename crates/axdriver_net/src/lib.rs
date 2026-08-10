//! Common traits and types for network device (NIC) drivers.

#![no_std]
#![cfg_attr(doc, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "fxmac")]
/// fxmac driver for PhytiumPi
pub mod fxmac;
#[cfg(feature = "ixgbe")]
/// ixgbe NIC device driver.
pub mod ixgbe;

#[doc(no_inline)]
pub use axdriver_base::{BaseDriverOps, DevError, DevResult, DeviceType};

mod net_buf;
pub use self::net_buf::{NetBuf, NetBufBox, NetBufPool, NetBufPtr};

/// The ethernet address of the NIC (MAC address).
pub struct EthernetAddress(pub [u8; 6]);

/// Transport-neutral control of an NIC's RX queue notification and completion
/// visibility.
///
/// Data movement (reap/refill) is still performed through the existing
/// [`NetDriverOps::receive`] and [`NetDriverOps::recycle_rx_buffer`] methods;
/// this interface only controls whether used-buffer notifications are delivered
/// to the driver and whether RX completions are currently visible.
///
/// The notification-control methods must be atomic at the call level: an error
/// must never leave the queue in a half-suppressed / half-armed state.
pub trait NetQueueControl {
    /// Returns whether at least one RX completion is currently visible to the
    /// driver.
    fn has_rx_completion(&self) -> bool;

    /// Suppresses RX used-buffer notifications.
    fn suppress_rx_notify(&mut self) -> DevResult;

    /// Rearms RX notifications and reports whether a completion is still
    /// pending after the memory barrier required by the transport.
    fn arm_rx_notify_and_check(&mut self) -> DevResult<bool>;
}

/// Operations that require a network device (NIC) driver to implement.
pub trait NetDriverOps: BaseDriverOps {
    /// The ethernet address of the NIC.
    fn mac_address(&self) -> EthernetAddress;

    /// Whether can transmit packets.
    fn can_transmit(&self) -> bool;

    /// Whether can receive packets.
    fn can_receive(&self) -> bool;

    /// Size of the receive queue.
    fn rx_queue_size(&self) -> usize;

    /// Size of the transmit queue.
    fn tx_queue_size(&self) -> usize;

    /// Returns the RX queue-control interface of this NIC, if the underlying
    /// driver supports explicit notification control.
    fn queue_control(&mut self) -> Option<&mut dyn NetQueueControl> {
        None
    }

    /// Gives back the `rx_buf` to the receive queue for later receiving.
    ///
    /// `rx_buf` should be the same as the one returned by
    /// [`NetDriverOps::receive`].
    fn recycle_rx_buffer(&mut self, rx_buf: NetBufPtr) -> DevResult;

    /// Poll the transmit queue and gives back the buffers for previous transmiting.
    /// returns [`DevResult`].
    fn recycle_tx_buffers(&mut self) -> DevResult;

    /// Transmits a packet in the buffer to the network, without blocking,
    /// returns [`DevResult`].
    fn transmit(&mut self, tx_buf: NetBufPtr) -> DevResult;

    /// Receives a packet from the network and store it in the [`NetBuf`],
    /// returns the buffer.
    ///
    /// Before receiving, the driver should have already populated some buffers
    /// in the receive queue by [`NetDriverOps::recycle_rx_buffer`].
    ///
    /// If currently no incomming packets, returns an error with type
    /// [`DevError::Again`].
    fn receive(&mut self) -> DevResult<NetBufPtr>;

    /// Allocate a memory buffer of a specified size for network transmission,
    /// returns [`DevResult`]
    fn alloc_tx_buffer(&mut self, size: usize) -> DevResult<NetBufPtr>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyNet;

    impl BaseDriverOps for DummyNet {
        fn device_name(&self) -> &str {
            "dummy-net"
        }

        fn device_type(&self) -> DeviceType {
            DeviceType::Net
        }
    }

    impl NetDriverOps for DummyNet {
        fn mac_address(&self) -> EthernetAddress {
            EthernetAddress([0; 6])
        }

        fn can_transmit(&self) -> bool {
            false
        }

        fn can_receive(&self) -> bool {
            false
        }

        fn rx_queue_size(&self) -> usize {
            0
        }

        fn tx_queue_size(&self) -> usize {
            0
        }

        fn recycle_rx_buffer(&mut self, _rx_buf: NetBufPtr) -> DevResult {
            Ok(())
        }

        fn recycle_tx_buffers(&mut self) -> DevResult {
            Ok(())
        }

        fn transmit(&mut self, _tx_buf: NetBufPtr) -> DevResult {
            Ok(())
        }

        fn receive(&mut self) -> DevResult<NetBufPtr> {
            Err(DevError::Again)
        }

        fn alloc_tx_buffer(&mut self, _size: usize) -> DevResult<NetBufPtr> {
            Err(DevError::NoMemory)
        }
    }

    #[derive(Debug, Default)]
    struct FakeQueueControl {
        completion_visible: bool,
        suppressed: bool,
        armed: bool,
    }

    impl NetQueueControl for FakeQueueControl {
        fn has_rx_completion(&self) -> bool {
            self.completion_visible
        }

        fn suppress_rx_notify(&mut self) -> DevResult {
            if self.suppressed {
                return Err(DevError::BadState);
            }
            self.suppressed = true;
            self.armed = false;
            Ok(())
        }

        fn arm_rx_notify_and_check(&mut self) -> DevResult<bool> {
            if !self.suppressed {
                return Err(DevError::BadState);
            }
            self.suppressed = false;
            self.armed = true;
            Ok(self.completion_visible)
        }
    }

    struct ControllingNet {
        control: FakeQueueControl,
    }

    impl BaseDriverOps for ControllingNet {
        fn device_name(&self) -> &str {
            "controlling-net"
        }

        fn device_type(&self) -> DeviceType {
            DeviceType::Net
        }
    }

    impl NetDriverOps for ControllingNet {
        fn mac_address(&self) -> EthernetAddress {
            EthernetAddress([0; 6])
        }

        fn can_transmit(&self) -> bool {
            false
        }

        fn can_receive(&self) -> bool {
            self.control.completion_visible
        }

        fn rx_queue_size(&self) -> usize {
            0
        }

        fn tx_queue_size(&self) -> usize {
            0
        }

        fn recycle_rx_buffer(&mut self, _rx_buf: NetBufPtr) -> DevResult {
            Ok(())
        }

        fn recycle_tx_buffers(&mut self) -> DevResult {
            Ok(())
        }

        fn transmit(&mut self, _tx_buf: NetBufPtr) -> DevResult {
            Ok(())
        }

        fn receive(&mut self) -> DevResult<NetBufPtr> {
            Err(DevError::Again)
        }

        fn alloc_tx_buffer(&mut self, _size: usize) -> DevResult<NetBufPtr> {
            Err(DevError::NoMemory)
        }

        fn queue_control(&mut self) -> Option<&mut dyn NetQueueControl> {
            Some(&mut self.control)
        }
    }

    #[test]
    fn default_accessor_is_none() {
        let mut dev = DummyNet;
        assert!(dev.queue_control().is_none());
    }

    #[test]
    fn accessor_exposes_queue_control() {
        let mut dev = ControllingNet {
            control: FakeQueueControl::default(),
        };
        dev.control.completion_visible = true;
        let control = dev.queue_control().expect("queue control missing");
        assert!(control.has_rx_completion());
        control.suppress_rx_notify().unwrap();
        assert!(control.has_rx_completion());
        assert!(control.arm_rx_notify_and_check().unwrap());
    }

    #[test]
    fn arm_reports_pending_completion() {
        let mut dev = ControllingNet {
            control: FakeQueueControl::default(),
        };
        dev.control.completion_visible = true;
        let control = dev.queue_control().unwrap();
        control.suppress_rx_notify().unwrap();
        let pending = control.arm_rx_notify_and_check().unwrap();
        assert!(pending);
    }

    #[test]
    fn suppress_is_atomic_on_repeat() {
        let mut dev = ControllingNet {
            control: FakeQueueControl::default(),
        };
        let control = dev.queue_control().unwrap();
        control.suppress_rx_notify().unwrap();
        assert!(control.suppress_rx_notify().is_err());
    }
}
