use log::{debug, info, warn};
use zerocopy::AsBytes;

use super::{
    Config, EthernetAddress, Features, MIN_BUFFER_LEN, NET_HDR_SIZE, QUEUE_RECEIVE, QUEUE_TRANSMIT,
    SUPPORTED_FEATURES, Status, VirtioNetHdr,
};
use crate::{
    Error, Result,
    hal::Hal,
    queue::VirtQueue,
    transport::{DeviceStatus, Transport},
    volatile::volread,
};

/// Raw driver for a VirtIO network device.
///
/// This is a raw version of the VirtIONet driver. It provides non-blocking
/// methods for transmitting and receiving raw slices, without the buffer
/// management. For more higher-level functions such as receive buffer backing,
/// see [`VirtIONet`].
///
/// [`VirtIONet`]: super::VirtIONet
pub struct VirtIONetRaw<H: Hal, T: Transport, const QUEUE_SIZE: usize> {
    transport: T,
    mac: EthernetAddress,
    recv_queue: VirtQueue<H, QUEUE_SIZE>,
    send_queue: VirtQueue<H, QUEUE_SIZE>,
    /// A partially-built replacement queue retained after a failed recovery
    /// prepare. Its DMA address has already been handed to the transport by
    /// `queue_set`, so it must stay alive (never dropped as a local) until the
    /// device has confirmed it stopped or the raw device is dropped; otherwise
    /// the transport would reference freed memory. When the peer queue also
    /// builds, this is cleared and the pair is committed.
    pending_send: Option<VirtQueue<H, QUEUE_SIZE>>,
}

impl<H: Hal, T: Transport, const QUEUE_SIZE: usize> VirtIONetRaw<H, T, QUEUE_SIZE> {
    /// Create a new VirtIO-Net driver.
    pub fn new(mut transport: T) -> Result<Self> {
        let negotiated_features = transport.begin_init(SUPPORTED_FEATURES);
        info!("negotiated_features {:?}", negotiated_features);
        // read configuration space
        let config = transport.config_space::<Config>()?;
        let mac;
        // Safe because config points to a valid MMIO region for the config space.
        unsafe {
            mac = volread!(config, mac);
            debug!(
                "Got MAC={:02x?}, status={:?}",
                mac,
                volread!(config, status)
            );
        }
        let send_queue = VirtQueue::new(
            &mut transport,
            QUEUE_TRANSMIT,
            negotiated_features.contains(Features::RING_INDIRECT_DESC),
            negotiated_features.contains(Features::RING_EVENT_IDX),
        )?;
        let recv_queue = VirtQueue::new(
            &mut transport,
            QUEUE_RECEIVE,
            negotiated_features.contains(Features::RING_INDIRECT_DESC),
            negotiated_features.contains(Features::RING_EVENT_IDX),
        )?;

        transport.finish_init();

        Ok(VirtIONetRaw {
            transport,
            mac,
            recv_queue,
            send_queue,
            pending_send: None,
        })
    }

    /// Acknowledge interrupt.
    pub fn ack_interrupt(&mut self) -> bool {
        self.transport.ack_interrupt()
    }

    /// Initiates a full device reset by writing the empty device status.
    pub fn begin_reset(&mut self) {
        self.transport.begin_reset();
    }

    /// Reports whether the device has confirmed it stopped accessing its
    /// queues after a [`Self::begin_reset`].
    pub fn reset_confirmed(&self) -> bool {
        self.transport.reset_confirmed()
    }

    /// Reads back the device status register.
    ///
    /// Exposed so a control path (e.g. an adapter test or a recovery owner) can
    /// observe whether `DRIVER_OK` has been published, without reaching into the
    /// transport's private status type.
    pub fn device_status(&self) -> DeviceStatus {
        self.transport.get_status()
    }

    /// Rebuilds the device queues after a *confirmed* device reset, without yet
    /// publishing `DRIVER_OK`.
    ///
    /// Re-runs the VirtIO initialization sequence (feature negotiation and queue
    /// creation) against the same transport and replaces the old queues, which
    /// frees their DMA backing. Re-run only after [`Transport::reset_confirmed`]
    /// (the confirmation boundary is enforced inside this primitive).
    ///
    /// This is the *prepare* phase of a transactional recovery: it installs the
    /// replacement queues so RX/TX packet backing can be filled against them, but
    /// does **not** set `DEVICE_OK`/`DRIVER_OK`. [`Self::commit_driver_ok`] must
    /// be called only after the caller has fully refilled every slot, so a partial
    /// rebuild or partial refill never leaves an armed device DMAing into a
    /// partially populated queue.
    ///
    /// If the transmit queue builds but the receive queue fails, the built
    /// transmit queue is retained in [`Self::pending_send`] (never dropped as a
    /// local) until it can be committed or the device confirms it stopped again,
    /// so the transport never references freed DMA.
    pub fn reinit_prepare(&mut self) -> Result<()> {
        if !self.transport.reset_confirmed() {
            return Err(Error::NotReady);
        }
        let negotiated = self.transport.begin_init(SUPPORTED_FEATURES);
        // Build the transmit queue first and retain it on a partial failure:
        // its DMA address is already registered with the transport.
        let new_send = VirtQueue::new(
            &mut self.transport,
            QUEUE_TRANSMIT,
            negotiated.contains(Features::RING_INDIRECT_DESC),
            negotiated.contains(Features::RING_EVENT_IDX),
        );
        let new_send = match new_send {
            Ok(q) => q,
            Err(e) => return Err(e),
        };
        let new_recv = match VirtQueue::new(
            &mut self.transport,
            QUEUE_RECEIVE,
            negotiated.contains(Features::RING_INDIRECT_DESC),
            negotiated.contains(Features::RING_EVENT_IDX),
        ) {
            Ok(q) => q,
            Err(e) => {
                // Retain the built transmit queue so its registered DMA address
                // stays alive (no free-while-registered). It is released when the
                // device confirms it stopped or when this raw is dropped.
                self.pending_send = Some(new_send);
                return Err(e);
            }
        };
        // Both queues built: replace the old (confirmed-stopped) queues. The old
        // backing is freed now that the device is confirmed not to access them.
        self.send_queue = new_send;
        self.recv_queue = new_recv;
        Ok(())
    }

    /// Commits a prepared [`Self::reinit_prepare`] by publishing `DRIVER_OK`.
    ///
    /// Call only after every RX/TX slot has been refilled against the replacement
    /// queues; never before the replacement queue is armed to be trampling-free.
    pub fn commit_driver_ok(&mut self) -> Result<()> {
        self.transport.finish_init();
        Ok(())
    }

    /// Reads the device's net configuration `status` (link state) under a
    /// config-generation guard, returning a consistent snapshot or
    /// [`Error::Retry`] when a device config update raced the read.
    pub fn read_link_status(&mut self) -> Result<bool> {
        self.transport.read_config_snapshot(|transport| {
            let config = transport.config_space::<Config>()?;
            // SAFETY: `config` points to the valid VirtIO MMIO config space for
            // the duration of the read; the volatile read observes device state.
            Ok(unsafe { volread!(config, status).contains(Status::LINK_UP) })
        })
    }

    /// Disable interrupts.
    pub fn disable_interrupts(&mut self) {
        self.send_queue.set_dev_notify(false);
        self.recv_queue.set_dev_notify(false);
    }

    /// Enable interrupts.
    pub fn enable_interrupts(&mut self) {
        self.send_queue.set_dev_notify(true);
        self.recv_queue.set_dev_notify(true);
    }

    /// Whether the receive queue currently has a completed buffer.
    pub fn poll_rx_completion(&self) -> bool {
        self.recv_queue.can_pop()
    }

    /// Suppresses receive-queue notifications, leaving the transmit queue
    /// unchanged.
    pub fn suppress_rx_notify(&mut self) {
        self.recv_queue.suppress_dev_notify();
    }

    /// Rearms receive-queue notifications and reports whether a completion is
    /// still pending after the memory barrier, leaving the transmit queue
    /// unchanged.
    pub fn arm_rx_notify_and_check(&mut self) -> bool {
        self.recv_queue.arm_dev_notify_and_check()
    }

    /// Whether the transmit queue currently has a completed buffer.
    pub fn poll_tx_completion(&self) -> bool {
        self.send_queue.can_pop()
    }

    /// Suppresses transmit-queue used-buffer notifications.
    pub fn suppress_tx_notify(&mut self) {
        self.send_queue.suppress_dev_notify();
    }

    /// Rearms transmit-queue notifications and checks for a raced completion.
    pub fn arm_tx_notify_and_check(&mut self) -> bool {
        self.send_queue.arm_dev_notify_and_check()
    }

    /// Get MAC address.
    pub fn mac_address(&self) -> EthernetAddress {
        self.mac
    }

    /// Whether can send packet.
    pub fn can_send(&self) -> bool {
        self.send_queue.available_desc() >= 1
    }

    /// Number of free descriptors in the send queue (RW-2 real ledger).
    pub fn send_available_desc(&self) -> usize {
        self.send_queue.available_desc()
    }

    /// Whether the length of the receive buffer is valid.
    fn check_rx_buf_len(rx_buf: &[u8]) -> Result<()> {
        if rx_buf.len() < MIN_BUFFER_LEN {
            warn!("Receive buffer len {} is too small", rx_buf.len());
            Err(Error::InvalidParam)
        } else {
            Ok(())
        }
    }

    /// Whether the length of the transmit buffer is valid.
    fn check_tx_buf_len(tx_buf: &[u8]) -> Result<()> {
        if tx_buf.len() < NET_HDR_SIZE {
            warn!("Transmit buffer len {} is too small", tx_buf.len());
            Err(Error::InvalidParam)
        } else {
            Ok(())
        }
    }

    /// Fill the header of the `buffer` with [`VirtioNetHdr`].
    ///
    /// If the `buffer` is not large enough, it returns [`Error::InvalidParam`].
    pub fn fill_buffer_header(&self, buffer: &mut [u8]) -> Result<usize> {
        if buffer.len() < NET_HDR_SIZE {
            return Err(Error::InvalidParam);
        }
        let header = VirtioNetHdr::default();
        buffer[..NET_HDR_SIZE].copy_from_slice(header.as_bytes());
        Ok(NET_HDR_SIZE)
    }

    /// Submits a request to transmit a buffer immediately without waiting for
    /// the transmission to complete.
    ///
    /// It will submit request to the VirtIO net device and return a token
    /// identifying the position of the first descriptor in the chain. If there
    /// are not enough descriptors to allocate, then it returns
    /// [`Error::QueueFull`].
    ///
    /// The caller needs to fill the `tx_buf` with a header by calling
    /// [`fill_buffer_header`] before transmission. Then it calls [`poll_transmit`]
    /// with the returned token to check whether the device has finished handling
    /// the request. Once it has, the caller must call [`transmit_complete`] with
    /// the same buffer before reading the result (transmitted length).
    ///
    /// # Safety
    ///
    /// `tx_buf` is still borrowed by the underlying VirtIO net device even after
    /// this method returns. Thus, it is the caller's responsibility to guarantee
    /// that they are not accessed before the request is completed in order to
    /// avoid data races.
    ///
    /// [`fill_buffer_header`]: Self::fill_buffer_header
    /// [`poll_transmit`]: Self::poll_transmit
    /// [`transmit_complete`]: Self::transmit_complete
    pub unsafe fn transmit_begin(&mut self, tx_buf: &[u8]) -> Result<u16> {
        Self::check_tx_buf_len(tx_buf)?;
        let token = self.send_queue.add(&[tx_buf], &mut [])?;
        if self.send_queue.should_notify() {
            self.transport.notify(QUEUE_TRANSMIT);
        }
        Ok(token)
    }

    /// Fetches the token of the next completed transmission request from the
    /// used ring and returns it, without removing it from the used ring. If
    /// there are no pending completed requests it returns [`None`].
    pub fn poll_transmit(&mut self) -> Option<u16> {
        self.send_queue.peek_used()
    }

    /// Completes a transmission operation which was started by [`transmit_begin`].
    /// Returns number of bytes transmitted.
    ///
    /// # Safety
    ///
    /// The same buffer must be passed in again as was passed to
    /// [`transmit_begin`] when it returned the token.
    ///
    /// [`transmit_begin`]: Self::transmit_begin
    pub unsafe fn transmit_complete(&mut self, token: u16, tx_buf: &[u8]) -> Result<usize> {
        let len = self.send_queue.pop_used(token, &[tx_buf], &mut [])?;
        Ok(len as usize)
    }

    /// Submits a request to receive a buffer immediately without waiting for
    /// the reception to complete.
    ///
    /// It will submit request to the VirtIO net device and return a token
    /// identifying the position of the first descriptor in the chain. If there
    /// are not enough descriptors to allocate, then it returns
    /// [`Error::QueueFull`].
    ///
    /// The caller can then call [`poll_receive`] with the returned token to
    /// check whether the device has finished handling the request. Once it has,
    /// the caller must call [`receive_complete`] with the same buffer before
    /// reading the response.
    ///
    /// # Safety
    ///
    /// `rx_buf` is still borrowed by the underlying VirtIO net device even after
    /// this method returns. Thus, it is the caller's responsibility to guarantee
    /// that they are not accessed before the request is completed in order to
    /// avoid data races.
    ///
    /// [`poll_receive`]: Self::poll_receive
    /// [`receive_complete`]: Self::receive_complete
    pub unsafe fn receive_begin(&mut self, rx_buf: &mut [u8]) -> Result<u16> {
        Self::check_rx_buf_len(rx_buf)?;
        let token = self.recv_queue.add(&[], &mut [rx_buf])?;
        if self.recv_queue.should_notify() {
            self.transport.notify(QUEUE_RECEIVE);
        }
        Ok(token)
    }

    /// Fetches the token of the next completed reception request from the
    /// used ring and returns it, without removing it from the used ring. If
    /// there are no pending completed requests it returns [`None`].
    pub fn poll_receive(&self) -> Option<u16> {
        self.recv_queue.peek_used()
    }

    /// Completes a transmission operation which was started by [`receive_begin`].
    ///
    /// After completion, the `rx_buf` will contain a header followed by the
    /// received packet. It returns the length of the header and the length of
    /// the packet.
    ///
    /// # Safety
    ///
    /// The same buffer must be passed in again as was passed to
    /// [`receive_begin`] when it returned the token.
    ///
    /// [`receive_begin`]: Self::receive_begin
    pub unsafe fn receive_complete(
        &mut self,
        token: u16,
        rx_buf: &mut [u8],
    ) -> Result<(usize, usize)> {
        let len = self.recv_queue.pop_used(token, &[], &mut [rx_buf])? as usize;
        let packet_len = len.checked_sub(NET_HDR_SIZE).ok_or(Error::IoError)?;
        Ok((NET_HDR_SIZE, packet_len))
    }

    /// Sends a packet to the network, and blocks until the request completed.
    pub fn send(&mut self, tx_buf: &[u8]) -> Result {
        let header = VirtioNetHdr::default();
        if tx_buf.is_empty() {
            // Special case sending an empty packet, to avoid adding an empty buffer to the
            // virtqueue.
            self.send_queue.add_notify_wait_pop(
                &[header.as_bytes()],
                &mut [],
                &mut self.transport,
            )?;
        } else {
            self.send_queue.add_notify_wait_pop(
                &[header.as_bytes(), tx_buf],
                &mut [],
                &mut self.transport,
            )?;
        }
        Ok(())
    }

    /// Blocks and waits for a packet to be received.
    ///
    /// After completion, the `rx_buf` will contain a header followed by the
    /// received packet. It returns the length of the header and the length of
    /// the packet.
    pub fn receive_wait(&mut self, rx_buf: &mut [u8]) -> Result<(usize, usize)> {
        let token = unsafe { self.receive_begin(rx_buf)? };
        while self.poll_receive().is_none() {
            core::hint::spin_loop();
        }
        unsafe { self.receive_complete(token, rx_buf) }
    }
}

impl<H: Hal, T: Transport, const QUEUE_SIZE: usize> Drop for VirtIONetRaw<H, T, QUEUE_SIZE> {
    fn drop(&mut self) {
        // Clear any pointers pointing to DMA regions, so the device doesn't try to access them
        // after they have been freed.
        self.transport.queue_unset(QUEUE_RECEIVE);
        self.transport.queue_unset(QUEUE_TRANSMIT);
    }
}

#[cfg(test)]
mod tests {
    use alloc::alloc::{alloc_zeroed, dealloc};
    use core::{
        alloc::Layout,
        any::TypeId,
        cell::{Cell, RefCell},
        ptr::NonNull,
    };

    use super::*;
    use crate::{
        BufferDirection, Hal, PAGE_SIZE, PhysAddr,
        transport::{DeviceStatus, DeviceType, Transport},
        volatile::ReadOnly,
    };

    // Per-test-thread DMA accounting. A Rust test runs on its own thread, so
    // `thread_local!` gives each test isolated allocation/deallocation counts
    // instead of the shared global statics that previously made parallel runs
    // nondeterministic. The set `LIVE_ADDRS` records which allocations are still
    // alive so a witness can check that a transport-registered queue address
    // still points at a live allocation (address-aware, not just a count).
    thread_local! {
        static DMA_ALLOCS: Cell<usize> = const { Cell::new(0) };
        static DMA_DEALLOCS: Cell<usize> = const { Cell::new(0) };
        static LIVE_ADDRS: RefCell<alloc::collections::BTreeSet<PhysAddr>> =
            const { RefCell::new(alloc::collections::BTreeSet::new()) };
    }

    /// Bumps and returns the allocating Hal's live DMA count for the current
    /// test thread.
    fn dma_alive_count() -> usize {
        DMA_ALLOCS.get() - DMA_DEALLOCS.get()
    }

    /// A Hal that counts DMA allocations and deallocations so tests can prove
    /// queue backing is conserved across a reinit.
    #[derive(Debug)]
    struct CountingHal;

    unsafe impl Hal for CountingHal {
        fn dma_alloc(pages: usize, _direction: BufferDirection) -> (PhysAddr, NonNull<u8>) {
            DMA_ALLOCS.set(DMA_ALLOCS.get() + 1);
            let layout = Layout::from_size_align(pages * PAGE_SIZE, PAGE_SIZE).unwrap();
            let ptr = unsafe { alloc_zeroed(layout) };
            let paddr = ptr as PhysAddr;
            LIVE_ADDRS.with(|s| {
                s.borrow_mut().insert(paddr);
            });
            (paddr, NonNull::new(ptr).unwrap())
        }

        unsafe fn dma_dealloc(paddr: PhysAddr, vaddr: NonNull<u8>, pages: usize) -> i32 {
            DMA_DEALLOCS.set(DMA_DEALLOCS.get() + 1);
            LIVE_ADDRS.with(|s| {
                s.borrow_mut().remove(&paddr);
            });
            let layout = Layout::from_size_align(pages * PAGE_SIZE, PAGE_SIZE).unwrap();
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

    /// A transport that models a real MMIO device: writing empty status resets
    /// the device, which clears its queue registers (so a later `VirtQueue::new`
    /// sees the queue as unused); `defer_reset` holds the reset pending.
    #[derive(Debug)]
    struct RecoveryTransport {
        config: Config,
        status: DeviceStatus,
        queue_addr: [Option<(PhysAddr, PhysAddr, PhysAddr)>; 2],
        gen: u8,
        defer_reset: bool,
        fail_recv_reinit: bool,
    }

    impl Transport for RecoveryTransport {
        fn device_type(&self) -> DeviceType {
            DeviceType::Network
        }
        fn read_device_features(&mut self) -> u64 {
            0
        }
        fn write_driver_features(&mut self, _driver_features: u64) {}
        fn max_queue_size(&mut self, _queue: u16) -> u32 {
            4
        }
        fn notify(&mut self, _queue: u16) {}
        fn get_status(&self) -> DeviceStatus {
            self.status
        }
        fn set_status(&mut self, status: DeviceStatus) {
            if status.is_empty() {
                if self.defer_reset {
                    return;
                }
                self.status = status;
                self.queue_addr = [None, None];
                return;
            }
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
            descriptors: PhysAddr,
            driver_area: PhysAddr,
            device_area: PhysAddr,
        ) {
            self.queue_addr[queue as usize] = Some((descriptors, driver_area, device_area));
        }
        fn queue_unset(&mut self, queue: u16) {
            self.queue_addr[queue as usize] = None;
        }
        fn queue_used(&mut self, queue: u16) -> bool {
            self.queue_addr[queue as usize].is_some() || (self.fail_recv_reinit && queue == 0)
        }
        fn ack_interrupt(&mut self) -> bool {
            false
        }
        fn config_space<T: 'static>(&self) -> Result<NonNull<T>> {
            if TypeId::of::<T>() == TypeId::of::<Config>() {
                Ok(NonNull::from(&self.config).cast())
            } else {
                Err(Error::ConfigSpaceMissing)
            }
        }
        fn config_generation(&self) -> Option<u8> {
            Some(self.gen)
        }
    }

    fn recovery_transport(status: Status) -> RecoveryTransport {
        RecoveryTransport {
            config: Config {
                mac: ReadOnly::new([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]),
                status: ReadOnly::new(status),
                max_virtqueue_pairs: ReadOnly::new(0),
                mtu: ReadOnly::new(1500),
            },
            status: DeviceStatus::empty(),
            queue_addr: [None, None],
            gen: 0,
            defer_reset: false,
            fail_recv_reinit: false,
        }
    }

    #[test]
    fn reinit_rebuilds_queues_without_leaking_dma_backing() {
        DMA_ALLOCS.set(0);
        DMA_DEALLOCS.set(0);
        LIVE_ADDRS.with(|s| s.borrow_mut().clear());

        let mut dev: VirtIONetRaw<CountingHal, RecoveryTransport, 4> =
            VirtIONetRaw::new(recovery_transport(Status::empty())).unwrap();
        let alive_after_new = dma_alive_count();
        assert!(alive_after_new > 0, "queues allocate DMA backing");

        // Driver initiates reset; device confirms it stopped (clearing queues).
        dev.begin_reset();
        assert!(dev.reset_confirmed(), "reset acknowledged");

        dev.reinit_prepare().unwrap();
        dev.commit_driver_ok().unwrap();

        let alive_after_reinit = dma_alive_count();
        assert_eq!(
            alive_after_reinit, alive_after_new,
            "no DMA backing leaked or duplicated across reinit"
        );
        assert!(DMA_DEALLOCS.get() > 0, "old queues were freed");
        assert!(dev.can_send(), "rebuilt send queue is usable");
    }

    #[test]
    fn deferred_unconfirmed_reset_forbids_reinit() {
        // Contract: reinit (which frees old backing) must not run while the
        // device has not confirmed it stopped. A transport whose reset is held
        // pending by `defer_reset` reports a non-empty status even after a
        // reset write, so `reset_confirmed` stays false and `reinit` must
        // refuse without touching DMA backing.
        let mut transport = recovery_transport(Status::empty());
        transport.defer_reset = true;
        DMA_ALLOCS.set(0);
        DMA_DEALLOCS.set(0);
        LIVE_ADDRS.with(|s| s.borrow_mut().clear());
        let mut dev: VirtIONetRaw<CountingHal, RecoveryTransport, 4> =
            VirtIONetRaw::new(transport).unwrap();
        let alive_after_new = dma_alive_count();
        dev.begin_reset();
        assert!(
            !dev.reset_confirmed(),
            "a pending reset must not allow freeing old backing"
        );
        assert!(
            dev.reinit_prepare().is_err(),
            "reinit must refuse while the reset is unconfirmed"
        );
        assert_eq!(
            dma_alive_count(),
            alive_after_new,
            "a refused reinit must not allocate or free any DMA backing"
        );
    }

    #[test]
    fn link_status_reads_consistent_net_config_status() {
        let mut link_up: VirtIONetRaw<CountingHal, RecoveryTransport, 4> =
            VirtIONetRaw::new(recovery_transport(Status::LINK_UP)).unwrap();
        assert!(link_up.read_link_status().unwrap(), "link up is reported");

        let mut link_down: VirtIONetRaw<CountingHal, RecoveryTransport, 4> =
            VirtIONetRaw::new(recovery_transport(Status::empty())).unwrap();
        assert!(
            !link_down.read_link_status().unwrap(),
            "link down is not reported"
        );
    }

    /// `reinit_prepare` must not publish `DRIVER_OK`: the adapter refills every
    /// slot against the replacement queues only after prepare, and commit is the
    /// single point that arms the device.
    #[test]
    fn reinit_prepare_does_not_publish_driver_ok_until_commit() {
        let mut dev: VirtIONetRaw<CountingHal, RecoveryTransport, 4> =
            VirtIONetRaw::new(recovery_transport(Status::empty())).unwrap();
        dev.begin_reset();
        assert!(dev.reset_confirmed(), "reset acknowledged");

        dev.reinit_prepare().unwrap();
        let after_prepare = dev.transport.get_status();
        assert!(
            !after_prepare.contains(DeviceStatus::DRIVER_OK),
            "prepare must not arm the device (status {:?})",
            after_prepare
        );

        dev.commit_driver_ok().unwrap();
        let after_commit = dev.transport.get_status();
        assert!(
            after_commit.contains(DeviceStatus::DRIVER_OK),
            "commit publishes DRIVER_OK (status {:?})",
            after_commit
        );
    }

    /// A partial queue construction (send builds, receive fails) must retain the
    /// built send queue's DMA backing under a live owner: the registered address
    /// must never be freed while the transport still references it. The witness
    /// is address-aware: it checks the queue address the transport registered
    /// against the set of allocations that are still alive, rather than only
    /// comparing an aggregate count (which a concurrent test could perturb).
    #[test]
    fn partial_reinit_retains_send_backing_and_does_not_driver_ok() {
        DMA_ALLOCS.set(0);
        DMA_DEALLOCS.set(0);
        LIVE_ADDRS.with(|s| s.borrow_mut().clear());
        let transport = recovery_transport(Status::empty());
        let mut dev: VirtIONetRaw<CountingHal, RecoveryTransport, 4> =
            VirtIONetRaw::new(transport).unwrap();
        // Enable the receive-rebuild failure only after a successful initial
        // bring-up, so it exercises the recovery prepare path, not `new`.
        dev.transport.fail_recv_reinit = true;

        dev.begin_reset();
        assert!(dev.reset_confirmed(), "reset acknowledged");

        // The receive rebuild fails, but the successfully-built send queue must
        // be retained (not dropped as a local) so its DMA survives.
        assert!(dev.reinit_prepare().is_err());
        assert!(
            dev.pending_send.is_some(),
            "partial send queue must be retained while the transport references it"
        );
        assert!(
            !dev.transport.get_status().contains(DeviceStatus::DRIVER_OK),
            "no DRIVER_OK after a failed partial rebuild"
        );
        // Address-aware: the send-queue descriptor address the transport
        // registered during the partial prepare must still point at a live
        // allocation (never freed while the transport references it).
        let registered_send_addr = dev.transport.queue_addr[1]
            .map(|(descriptors, ..)| descriptors)
            .expect("send queue registered during the partial prepare");
        assert!(
            LIVE_ADDRS.with(|s| s.borrow().contains(&registered_send_addr)),
            "the transport-registered send address must still be live (address {:x})",
            registered_send_addr
        );

        drop(dev);
        // Dropping the raw frees every allocation, including the retained partial
        // queue; nothing leaks.
        assert_eq!(
            dma_alive_count(),
            0,
            "no DMA backing leaked after the raw is dropped"
        );
    }
}
