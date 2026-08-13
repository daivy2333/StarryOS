use alloc::{sync::Arc, vec::Vec};

use axdriver_base::{BaseDriverOps, DevError, DevResult, DeviceType};
use axdriver_net::{
    EthernetAddress, NetBuf, NetBufBox, NetBufPool, NetBufPtr, NetDriverOps, NetQueueControl,
    NetQueueDirection, NetTxQueue, TxCookie,
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
    /// Set once a TX ownership invariant breaks: all later TX operations fail
    /// with a stable [`DevError::BadState`] instead of panicking or reusing
    /// state.
    tx_fault: bool,
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
            buf_pool,
            irq,
            #[cfg(test)]
            forced_tx_token: None,
            #[cfg(test)]
            forced_completion_failure: false,
        };

        // 1. Fill all rx buffers.
        for (i, rx_buf_place) in dev.rx_buffers.iter_mut().enumerate() {
            let mut rx_buf = dev.buf_pool.alloc_boxed().ok_or(DevError::NoMemory)?;
            // Safe because the buffer lives as long as the queue.
            let token = unsafe {
                dev.inner
                    .receive_begin(rx_buf.raw_buf_mut())
                    .map_err(as_dev_err)?
            };
            assert_eq!(token, i as u16);
            *rx_buf_place = Some(rx_buf);
        }

        // 2. Allocate all tx buffers.
        for _ in 0..QS {
            let mut tx_buf = dev.buf_pool.alloc_boxed().ok_or(DevError::NoMemory)?;
            // Fill header
            let hdr_len = dev
                .inner
                .fill_buffer_header(tx_buf.raw_buf_mut())
                .or(Err(DevError::InvalidParam))?;
            tx_buf.set_header_len(hdr_len);
            dev.free_tx_bufs.push(tx_buf);
        }

        // 3. Return the driver instance.
        Ok(dev)
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

impl<H: Hal, T: Transport, const QS: usize> NetDriverOps for VirtIoNetDev<H, T, QS> {
    #[inline]
    fn mac_address(&self) -> EthernetAddress {
        EthernetAddress(self.inner.mac_address())
    }

    #[inline]
    fn can_transmit(&self) -> bool {
        !self.tx_fault && !self.free_tx_bufs.is_empty() && self.inner.can_send()
    }

    #[inline]
    fn can_receive(&self) -> bool {
        self.inner.poll_receive().is_some()
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

    fn recycle_rx_buffer(&mut self, rx_buf: NetBufPtr) -> DevResult {
        let mut rx_buf = unsafe { NetBuf::from_buf_ptr(rx_buf) };
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
            return Err(DevError::BadState);
        }
        self.rx_buffers[new_token as usize] = Some(rx_buf);
        Ok(())
    }

    fn recycle_tx_buffers(&mut self) -> DevResult {
        if self.tx_fault {
            return Err(DevError::BadState);
        }
        while let Some(token) = self.inner.poll_transmit() {
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
        if self.tx_fault {
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
        if self.tx_fault {
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
        if self.tx_fault {
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
        if self.tx_fault {
            return Err(DevError::BadState);
        }
        let Some(token) = self.inner.poll_transmit() else {
            return Ok(None);
        };
        let slot = token as usize;
        if slot >= QS {
            return Err(self.enter_tx_fault(None));
        }
        if !matches!(self.tx_slots[slot], TxSlot::Queue(_, _)) {
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
}

#[cfg(test)]
mod tests {
    use alloc::alloc::{alloc_zeroed, dealloc};
    use core::{
        alloc::Layout,
        ptr::NonNull,
        sync::atomic::{AtomicU16, Ordering},
    };
    use std::sync::Mutex;

    use virtio_drivers::{
        BufferDirection, Hal, PhysAddr, Result,
        transport::{DeviceStatus, DeviceType, Transport},
    };

    use super::*;

    const QS: usize = 4;

    // Identity-mapped host memory: the driver and the fake device see the same
    // addresses, so the test can write used-ring completions directly.
    #[derive(Debug)]
    struct TestHal;

    unsafe impl Hal for TestHal {
        fn dma_alloc(pages: usize, _direction: BufferDirection) -> (PhysAddr, NonNull<u8>) {
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
                })),
            }
        }

        fn complete_tx(&self, token: u16, len: u32) {
            let mut state = self.shared.lock().unwrap();
            let used = state.used_rings[1].expect("send queue not configured");
            let used_idx = state.used_idx[1];
            // SAFETY: `used` points at the send queue's used ring, whose layout
            // is flags(u16) + idx(u16) + used_elems[QS] + used_event(u16); each
            // used elem is {id: u32, len: u32} at offset 4 + 8 * slot.
            unsafe {
                let entry = used.as_ptr().add(4 + 8 * (used_idx as usize % QS)) as *mut u32;
                entry.write_volatile(u32::from(token));
                entry.add(1).write_volatile(len);
                let idx = used.as_ptr().add(2) as *mut AtomicU16;
                (*idx).store(used_idx.wrapping_add(1), Ordering::Release);
            }
            state.used_idx[1] = used_idx.wrapping_add(1);
        }
    }

    // A minimal in-memory transport. `queue_set` records each queue's used
    // ring (device_area) in the fake device state shared with the controller.
    struct FakeTransport {
        config: TestNetConfig,
        shared: Arc<Mutex<FakeDeviceState>>,
        status: DeviceStatus,
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
            self.status
        }

        fn set_status(&mut self, status: DeviceStatus) {
            self.status = status;
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
        }

        fn queue_unset(&mut self, queue: u16) {
            self.shared.lock().unwrap().used_rings[queue as usize] = None;
        }

        fn queue_used(&mut self, _queue: u16) -> bool {
            false
        }

        fn ack_interrupt(&mut self) -> bool {
            false
        }

        fn config_space<T: 'static>(&self) -> Result<NonNull<T>> {
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
            status: DeviceStatus::empty(),
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
}
