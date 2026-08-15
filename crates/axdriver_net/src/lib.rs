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

/// A composable, transport-neutral set of network queue directions.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NetQueueDirection(u8);

impl NetQueueDirection {
    /// No queue direction.
    pub const NONE: Self = Self(0);
    /// Receive queue.
    pub const RX: Self = Self(1 << 0);
    /// Transmit queue.
    pub const TX: Self = Self(1 << 1);
    /// Receive and transmit queues.
    pub const BOTH: Self = Self(Self::RX.0 | Self::TX.0);

    /// Returns whether all directions in `other` are present.
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl core::ops::BitOr for NetQueueDirection {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl core::ops::BitOrAssign for NetQueueDirection {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

/// Opaque identity supplied by the queue owner and returned on TX completion.
///
/// This value identifies an owner-side slot; it is deliberately unrelated to
/// a transport descriptor, ring index, or device token.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TxCookie(u64);

impl TxCookie {
    /// Creates a cookie from an owner-side identity.
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the owner-side identity.
    pub const fn value(self) -> u64 {
        self.0
    }
}

/// Transport-neutral, read-only TX resource ledger (RW-2).
///
/// Reports how many TX buffers and descriptors the driver currently has
/// available and in flight. `available + inflight` equals the driver's fixed
/// capacity for each resource. A transport that cannot observe these counts
/// through the queue interface returns `None` from
/// [`NetTxQueue::tx_resource_ledger`]; callers must never synthesize the
/// ledger from slot or ticket capacities.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TxResourceLedger {
    /// Buffers the driver can hand out right now.
    pub buffer_available: u64,
    /// Buffers owned by the device/queue (submitted, not yet reclaimed).
    pub buffer_inflight: u64,
    /// Free descriptors in the TX queue.
    pub descriptor_available: u64,
    /// Descriptors in use by the TX queue.
    pub descriptor_inflight: u64,
    /// TX completions the transport has exposed (used-ring observations),
    /// independent of how many were later successfully reclaimed.
    pub completions_seen: u64,
}

/// Single-step, transport-neutral TX submission and completion reclaim.
pub trait NetTxQueue {
    /// Submits one prepared buffer with its owner cookie.
    ///
    /// On `Ok(())` the driver owns `tx_buf` and it stays device-owned until the
    /// matching cookie is returned by [`Self::reclaim_tx`].
    ///
    /// On a recoverable pre-accept error (including [`DevError::Again`]), the
    /// transport never borrowed the buffer: the driver has already returned it
    /// to its allocatable set. The caller keeps its logical slot but must not
    /// reuse the pointer.
    ///
    /// On a stable fatal error (a post-accept ownership invariant, e.g. an
    /// out-of-range or already-occupied token), the driver retains the buffer
    /// in a driver-owned fault state, stops all further TX operations and
    /// returns the same error afterwards. The caller must not reuse the
    /// pointer and must not treat the slot as recoverable.
    fn submit_tx(&mut self, tx_buf: NetBufPtr, cookie: TxCookie) -> DevResult;

    /// Reclaims at most one completed submission.
    fn reclaim_tx(&mut self) -> DevResult<Option<TxCookie>>;

    /// Returns the driver's real TX resource ledger, when the transport can
    /// observe it without leaking ring/token state. The default implementation
    /// reports no ledger; drivers that can must override it (RW-2).
    fn tx_resource_ledger(&self) -> Option<TxResourceLedger> {
        None
    }
}

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
    /// Reports which requested directions currently have visible completions.
    ///
    /// RX-only legacy implementations return [`DevError::Unsupported`] when TX
    /// is requested; bidirectional drivers should override this method.
    fn completion_pending(&self, directions: NetQueueDirection) -> DevResult<NetQueueDirection> {
        if directions.contains(NetQueueDirection::TX) {
            return Err(DevError::Unsupported);
        }
        Ok(
            if directions.contains(NetQueueDirection::RX) && self.has_rx_completion() {
                NetQueueDirection::RX
            } else {
                NetQueueDirection::NONE
            },
        )
    }

    /// Suppresses used-buffer notifications for the requested directions.
    fn suppress_notify(&mut self, directions: NetQueueDirection) -> DevResult {
        if directions.contains(NetQueueDirection::TX) {
            return Err(DevError::Unsupported);
        }
        if directions.contains(NetQueueDirection::RX) {
            self.suppress_rx_notify()?;
        }
        Ok(())
    }

    /// Arms the requested directions and returns directions still pending after
    /// the transport's required memory barrier.
    fn arm_notify_and_check(
        &mut self,
        directions: NetQueueDirection,
    ) -> DevResult<NetQueueDirection> {
        if directions.contains(NetQueueDirection::TX) {
            return Err(DevError::Unsupported);
        }
        Ok(
            if directions.contains(NetQueueDirection::RX) && self.arm_rx_notify_and_check()? {
                NetQueueDirection::RX
            } else {
                NetQueueDirection::NONE
            },
        )
    }

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

    /// Returns single-step TX queue operations when supported by the driver.
    fn tx_queue(&mut self) -> Option<&mut dyn NetTxQueue> {
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
    use alloc::vec::Vec;

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

    #[test]
    fn direction_mask_and_cookie_are_transport_neutral() {
        let both = NetQueueDirection::RX | NetQueueDirection::TX;
        assert!(both.contains(NetQueueDirection::RX));
        assert!(both.contains(NetQueueDirection::TX));
        assert_eq!(TxCookie::new(7).value(), 7);
    }

    #[derive(Default)]
    struct DwmacQueueModel {
        pending: NetQueueDirection,
        suppressed: NetQueueDirection,
        inflight: Vec<(TxCookie, NetBufBox)>,
        fail_submit: bool,
    }

    impl NetQueueControl for DwmacQueueModel {
        fn completion_pending(
            &self,
            directions: NetQueueDirection,
        ) -> DevResult<NetQueueDirection> {
            let mut pending = NetQueueDirection::NONE;
            if directions.contains(NetQueueDirection::RX)
                && self.pending.contains(NetQueueDirection::RX)
            {
                pending |= NetQueueDirection::RX;
            }
            if directions.contains(NetQueueDirection::TX)
                && self.pending.contains(NetQueueDirection::TX)
            {
                pending |= NetQueueDirection::TX;
            }
            Ok(pending)
        }

        fn suppress_notify(&mut self, directions: NetQueueDirection) -> DevResult {
            self.suppressed |= directions;
            Ok(())
        }

        fn arm_notify_and_check(
            &mut self,
            directions: NetQueueDirection,
        ) -> DevResult<NetQueueDirection> {
            if directions.contains(NetQueueDirection::RX) {
                self.suppressed = NetQueueDirection(self.suppressed.0 & !NetQueueDirection::RX.0);
            }
            if directions.contains(NetQueueDirection::TX) {
                self.suppressed = NetQueueDirection(self.suppressed.0 & !NetQueueDirection::TX.0);
            }
            self.completion_pending(directions)
        }

        fn has_rx_completion(&self) -> bool {
            self.pending.contains(NetQueueDirection::RX)
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

    impl NetTxQueue for DwmacQueueModel {
        fn submit_tx(&mut self, tx_buf: NetBufPtr, cookie: TxCookie) -> DevResult {
            // SAFETY: the test pointer was produced by `NetBuf::into_buf_ptr`
            // and this call assumes its ownership exactly once.
            let tx_buf = unsafe { NetBuf::from_buf_ptr(tx_buf) };
            if self.fail_submit {
                drop(tx_buf);
                return Err(DevError::Again);
            }
            self.inflight.push((cookie, tx_buf));
            Ok(())
        }

        fn reclaim_tx(&mut self) -> DevResult<Option<TxCookie>> {
            let Some((cookie, buffer)) = self.inflight.pop() else {
                return Ok(None);
            };
            drop(buffer);
            Ok(Some(cookie))
        }
    }

    #[test]
    fn dwmac_model_controls_rx_and_tx_without_transport_tokens() {
        let mut model = DwmacQueueModel {
            pending: NetQueueDirection::BOTH,
            ..Default::default()
        };
        model.suppress_notify(NetQueueDirection::BOTH).unwrap();
        assert_eq!(
            model.arm_notify_and_check(NetQueueDirection::BOTH).unwrap(),
            NetQueueDirection::BOTH
        );
    }

    #[test]
    fn dwmac_model_round_trips_cookie_and_recovers_submit_error_buffer() {
        let pool = NetBufPool::new(1, 1526).unwrap();
        let buffer = pool.alloc_boxed().unwrap().into_buf_ptr();
        let mut model = DwmacQueueModel::default();
        model.submit_tx(buffer, TxCookie::new(23)).unwrap();
        assert_eq!(model.reclaim_tx().unwrap(), Some(TxCookie::new(23)));
        assert!(pool.alloc_boxed().is_some());

        let buffer = pool.alloc_boxed().unwrap().into_buf_ptr();
        model.fail_submit = true;
        assert!(matches!(
            model.submit_tx(buffer, TxCookie::new(24)),
            Err(DevError::Again)
        ));
        assert!(pool.alloc_boxed().is_some());
    }
}
