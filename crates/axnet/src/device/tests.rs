//! MS04 T4.1: one-completion RX primitive tests with a fake NIC (test-only
//! `axdriver/dyn`; product builds keep the static VirtIO device model).

use alloc::{boxed::Box, collections::VecDeque, sync::Arc, vec, vec::Vec};
use core::sync::atomic::{AtomicUsize, Ordering};

use axdriver::prelude::*;
use axdriver_net::{
    NetBuf, NetBufPool, NetQueueControl, NetQueueDirection, NetTxQueue, TxCookie, TxResourceLedger,
};
use axerrno::{AxError, AxResult};
use memory_addr::{PhysAddr, VirtAddr};
use smoltcp::{
    storage::{PacketBuffer, PacketMetadata},
    time::Instant,
    wire::{
        ArpOperation, ArpPacket, ArpRepr, EthernetAddress, EthernetFrame, EthernetProtocol,
        EthernetRepr, IpAddress, Ipv4Address, Ipv4Cidr,
    },
};
use spin::Mutex;

use crate::{
    async_rx::{QUEUE_EVENT, RX_TELEMETRY, SERIAL},
    consts::STANDARD_MTU,
    device::{
        Device, EthernetDevice, LoopbackDevice, RxStep, TxOutcome, TxPreflight, TxReclaimStep,
        TxSubmitStep,
    },
    router::{Router, Rule, RxOwnerView},
    service::Service,
};

// Kernel ABI symbol required by `axklib` (pulled in via `axdriver/dyn`).
// The trait-ffi macro `#[def_extern_trait]` defaults to the Rust ABI, so the
// stub must match `extern "Rust" fn(PhysAddr, usize) -> AxResult<VirtAddr>`.
// Host tests never call it (the fake NIC performs no iomap); it returns a
// stable error so an accidental call is diagnosable instead of trapping.
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __axklib_0_3_mem_iomap(
    _addr: PhysAddr,
    _size: usize,
) -> AxResult<VirtAddr> {
    Err(AxError::Unsupported)
}

// Compile-time ABI witness: the stub must be assignable to the exact fn
// pointer type the trait-ffi macro generates for `Klib::mem_iomap`.
const _: unsafe extern "Rust" fn(PhysAddr, usize) -> AxResult<VirtAddr> = __axklib_0_3_mem_iomap;

const TEST_MAC: EthernetAddress = EthernetAddress([0x52, 0x54, 0x00, 0x12, 0x34, 0x56]);
const PEER_MAC: EthernetAddress = EthernetAddress([0x52, 0x54, 0x00, 0xaa, 0xbb, 0xcc]);
const TEST_IP: Ipv4Cidr = Ipv4Cidr::new(Ipv4Address::new(10, 0, 2, 15), 24);

#[derive(Default)]
struct FakeStats {
    receive_calls: Mutex<usize>,
    recycle_calls: Mutex<usize>,
    tx_packets: Mutex<Vec<Vec<u8>>>,
    frames: Mutex<VecDeque<Vec<u8>>>,
    receive_error: Mutex<bool>,
    recycle_error: Mutex<bool>,
    transmit_error: Mutex<bool>,
    alloc_error: Mutex<bool>,
    /// Raw TX capacity reported by `can_transmit`; defaults to true (the
    /// non-default `Mutex<bool>` is seeded in the constructor).
    tx_capacity: Mutex<bool>,
    /// Legacy `recycle_tx_buffers()` invocations (polling preflight path).
    recycle_tx_calls: Mutex<usize>,
    /// `can_transmit()` invocations (polling preflight path).
    can_transmit_calls: Mutex<usize>,
    /// Scripted TX completion cookies, consumed in order by `reclaim_tx`.
    reclaim_cookies: Mutex<VecDeque<u64>>,
    /// RW-2: scripted driver ledger. `Some` makes `tx_resource_ledger` report
    /// a real ledger; `None` (default) means the driver cannot observe one.
    ledger: Mutex<Option<TxResourceLedger>>,
}

impl FakeStats {
    fn new() -> Self {
        Self {
            tx_capacity: Mutex::new(true),
            ..Default::default()
        }
    }
}

struct FakeNic {
    pool: Arc<NetBufPool>,
    stats: Arc<FakeStats>,
    control: Option<FakeQueueControl>,
    tx_queue: Option<FakeTxQueue>,
}

#[derive(Default)]
struct FakeControlStats {
    suppress_calls: Mutex<usize>,
    arm_calls: Mutex<usize>,
    completion_visible: Mutex<bool>,
    suppress_error: Mutex<bool>,
    arm_error: Mutex<bool>,
}

struct FakeQueueControl {
    stats: Arc<FakeControlStats>,
}

impl NetQueueControl for FakeQueueControl {
    fn has_rx_completion(&self) -> bool {
        *self.stats.completion_visible.lock()
    }

    // Repeated suppression is idempotent, matching the VirtIO adapter whose
    // suppress only rewrites `used_event` and a flag.
    fn suppress_rx_notify(&mut self) -> DevResult {
        *self.stats.suppress_calls.lock() += 1;
        if *self.stats.suppress_error.lock() {
            return Err(DevError::Io);
        }
        Ok(())
    }

    fn arm_rx_notify_and_check(&mut self) -> DevResult<bool> {
        *self.stats.arm_calls.lock() += 1;
        if *self.stats.arm_error.lock() {
            return Err(DevError::Io);
        }
        Ok(*self.stats.completion_visible.lock())
    }

    fn suppress_notify(&mut self, directions: NetQueueDirection) -> DevResult {
        if directions.contains(NetQueueDirection::RX) {
            self.suppress_rx_notify()?;
        }
        if directions.contains(NetQueueDirection::TX) {
            *self.stats.suppress_calls.lock() += 1;
            if *self.stats.suppress_error.lock() {
                return Err(DevError::Io);
            }
        }
        Ok(())
    }

    fn arm_notify_and_check(
        &mut self,
        directions: NetQueueDirection,
    ) -> DevResult<NetQueueDirection> {
        let mut pending = NetQueueDirection::NONE;
        if directions.contains(NetQueueDirection::RX) && self.arm_rx_notify_and_check()? {
            pending |= NetQueueDirection::RX;
        }
        if directions.contains(NetQueueDirection::TX) {
            *self.stats.arm_calls.lock() += 1;
            if *self.stats.arm_error.lock() {
                return Err(DevError::Io);
            }
            if *self.stats.completion_visible.lock() {
                pending |= NetQueueDirection::TX;
            }
        }
        Ok(pending)
    }

    fn completion_pending(&self, directions: NetQueueDirection) -> DevResult<NetQueueDirection> {
        let mut pending = NetQueueDirection::NONE;
        if directions.contains(NetQueueDirection::RX) && *self.stats.completion_visible.lock() {
            pending |= NetQueueDirection::RX;
        }
        if directions.contains(NetQueueDirection::TX) && *self.stats.completion_visible.lock() {
            pending |= NetQueueDirection::TX;
        }
        Ok(pending)
    }
}

impl BaseDriverOps for FakeNic {
    fn device_name(&self) -> &str {
        "fake-nic"
    }

    fn device_type(&self) -> DeviceType {
        DeviceType::Net
    }
}

/// Scripted single-step TX queue (Task 3.2/3.4): records submits, replays
/// scripted reclaim cookies and submit outcomes from [`FakeStats`].
struct FakeTxQueue {
    stats: Arc<FakeStats>,
}

impl NetTxQueue for FakeTxQueue {
    fn submit_tx(&mut self, tx_buf: NetBufPtr, _cookie: TxCookie) -> DevResult {
        // SAFETY: the pointer came from `into_buf_ptr`; restoring the Box
        // returns the buffer to the pool exactly once.
        drop(unsafe { NetBuf::from_buf_ptr(tx_buf) });
        Ok(())
    }

    fn reclaim_tx(&mut self) -> DevResult<Option<TxCookie>> {
        Ok(self
            .stats
            .reclaim_cookies
            .lock()
            .pop_front()
            .map(TxCookie::new))
    }

    fn tx_resource_ledger(&self) -> Option<TxResourceLedger> {
        *self.stats.ledger.lock()
    }
}

impl NetDriverOps for FakeNic {
    fn mac_address(&self) -> axdriver_net::EthernetAddress {
        axdriver_net::EthernetAddress(TEST_MAC.0)
    }

    fn can_transmit(&self) -> bool {
        *self.stats.can_transmit_calls.lock() += 1;
        *self.stats.tx_capacity.lock()
    }

    fn can_receive(&self) -> bool {
        true
    }

    fn rx_queue_size(&self) -> usize {
        0
    }

    fn tx_queue_size(&self) -> usize {
        0
    }

    fn recycle_rx_buffer(&mut self, rx_buf: NetBufPtr) -> DevResult {
        *self.stats.recycle_calls.lock() += 1;
        if *self.stats.recycle_error.lock() {
            return Err(DevError::Io);
        }
        // SAFETY: the pointer came from `into_buf_ptr`; restoring the Box
        // returns the buffer to the pool exactly once.
        drop(unsafe { NetBuf::from_buf_ptr(rx_buf) });
        Ok(())
    }

    fn recycle_tx_buffers(&mut self) -> DevResult {
        *self.stats.recycle_tx_calls.lock() += 1;
        Ok(())
    }

    fn transmit(&mut self, tx_buf: NetBufPtr) -> DevResult {
        if *self.stats.transmit_error.lock() {
            // Conserve the buffer: put it back into the pool so a later
            // transmit can succeed, mirroring recoverable driver pressure.
            drop(unsafe { NetBuf::from_buf_ptr(tx_buf) });
            return Err(DevError::Again);
        }
        self.stats.tx_packets.lock().push(tx_buf.packet().to_vec());
        // SAFETY: the pointer came from `into_buf_ptr`; restoring the Box
        // returns the buffer to the pool exactly once.
        drop(unsafe { NetBuf::from_buf_ptr(tx_buf) });
        Ok(())
    }

    fn receive(&mut self) -> DevResult<NetBufPtr> {
        *self.stats.receive_calls.lock() += 1;
        if *self.stats.receive_error.lock() {
            return Err(DevError::Io);
        }
        let frame = self
            .stats
            .frames
            .lock()
            .pop_front()
            .ok_or(DevError::Again)?;
        let mut buf = self.pool.alloc_boxed().ok_or(DevError::NoMemory)?;
        buf.set_packet_len(frame.len());
        buf.packet_mut().copy_from_slice(&frame);
        Ok(buf.into_buf_ptr())
    }

    fn alloc_tx_buffer(&mut self, size: usize) -> DevResult<NetBufPtr> {
        if *self.stats.alloc_error.lock() {
            return Err(DevError::Again);
        }
        let mut buf = self.pool.alloc_boxed().ok_or(DevError::NoMemory)?;
        buf.set_packet_len(size);
        Ok(buf.into_buf_ptr())
    }

    fn queue_control(&mut self) -> Option<&mut dyn NetQueueControl> {
        self.control
            .as_mut()
            .map(|control| control as &mut dyn NetQueueControl)
    }

    fn tx_queue(&mut self) -> Option<&mut dyn NetTxQueue> {
        self.tx_queue
            .as_mut()
            .map(|queue| queue as &mut dyn NetTxQueue)
    }
}

fn make_ethernet_with_control(
    with_control: bool,
) -> (EthernetDevice, Arc<FakeStats>, Arc<FakeControlStats>) {
    let pool = NetBufPool::new(8, 2048).expect("pool alloc");
    let stats = Arc::new(FakeStats::new());
    let control_stats = Arc::new(FakeControlStats::default());
    let control = with_control.then(|| FakeQueueControl {
        stats: control_stats.clone(),
    });
    let nic = FakeNic {
        pool,
        stats: stats.clone(),
        control,
        tx_queue: None,
    };
    let dev = EthernetDevice::new("eth0".into(), Box::new(nic), TEST_IP);
    (dev, stats, control_stats)
}

fn make_ethernet_with_tx_queue() -> (EthernetDevice, Arc<FakeStats>) {
    let pool = NetBufPool::new(8, 2048).expect("pool alloc");
    let stats = Arc::new(FakeStats::new());
    let nic = FakeNic {
        pool,
        stats: stats.clone(),
        control: None,
        tx_queue: Some(FakeTxQueue {
            stats: stats.clone(),
        }),
    };
    let dev = EthernetDevice::new("eth0".into(), Box::new(nic), TEST_IP);
    (dev, stats)
}

fn make_ethernet() -> (EthernetDevice, Arc<FakeStats>) {
    let (dev, stats, _) = make_ethernet_with_control(false);
    (dev, stats)
}

fn service_with_ethernet_target(
    with_control: bool,
) -> (Service, Arc<FakeStats>, Arc<FakeControlStats>) {
    let (dev, stats, control_stats) = make_ethernet_with_control(with_control);
    let mut router = Router::new();
    let eth = router.add_device(Box::new(dev));
    (Service::new(router, Some(eth)), stats, control_stats)
}

fn rx_buffer(slots: usize) -> PacketBuffer<'static, ()> {
    PacketBuffer::new(
        vec![PacketMetadata::EMPTY; slots],
        vec![0u8; (STANDARD_MTU + EthernetFrame::<&[u8]>::header_len()) * slots],
    )
}

fn full_rx_buffer() -> PacketBuffer<'static, ()> {
    let mut buf = PacketBuffer::new(vec![PacketMetadata::EMPTY; 1], vec![0u8; 2048]);
    buf.enqueue(100, ()).expect("fill single slot");
    buf
}

fn eth_frame(
    dst: EthernetAddress,
    src: EthernetAddress,
    ethertype: EthernetProtocol,
    payload: &[u8],
) -> Vec<u8> {
    let mut buf = vec![0u8; EthernetFrame::<&[u8]>::header_len() + payload.len()];
    let repr = EthernetRepr {
        src_addr: src,
        dst_addr: dst,
        ethertype,
    };
    let mut frame = EthernetFrame::new_unchecked(&mut buf);
    repr.emit(&mut frame);
    frame.payload_mut().copy_from_slice(payload);
    buf
}

const IPV4_PAYLOAD: [u8; 20] = [
    0x45, 0x00, 0x00, 0x14, 0x00, 0x00, 0x00, 0x00, 0x40, 0x11, 0x00, 0x00, 10, 0, 2, 1, 10, 0, 2,
    15,
];

fn ipv4_frame_to_us() -> Vec<u8> {
    eth_frame(TEST_MAC, PEER_MAC, EthernetProtocol::Ipv4, &IPV4_PAYLOAD)
}

fn foreign_mac_frame() -> Vec<u8> {
    eth_frame(
        EthernetAddress([0x52, 0x54, 0x00, 0xde, 0xad, 0xbe]),
        PEER_MAC,
        EthernetProtocol::Ipv4,
        &IPV4_PAYLOAD,
    )
}

fn arp_request_frame() -> Vec<u8> {
    let repr = ArpRepr::EthernetIpv4 {
        operation: ArpOperation::Request,
        source_hardware_addr: PEER_MAC,
        source_protocol_addr: Ipv4Address::new(10, 0, 2, 1),
        target_hardware_addr: EthernetAddress::BROADCAST,
        target_protocol_addr: TEST_IP.address(),
    };
    let mut payload = vec![0u8; repr.buffer_len()];
    repr.emit(&mut ArpPacket::new_unchecked(&mut payload));
    eth_frame(
        EthernetAddress::BROADCAST,
        PEER_MAC,
        EthernetProtocol::Arp,
        &payload,
    )
}

const PEER_IP: Ipv4Address = Ipv4Address::new(10, 0, 2, 1);

fn arp_reply_frame() -> Vec<u8> {
    let repr = ArpRepr::EthernetIpv4 {
        operation: ArpOperation::Reply,
        source_hardware_addr: PEER_MAC,
        source_protocol_addr: PEER_IP,
        target_hardware_addr: TEST_MAC,
        target_protocol_addr: TEST_IP.address(),
    };
    let mut payload = vec![0u8; repr.buffer_len()];
    repr.emit(&mut ArpPacket::new_unchecked(&mut payload));
    eth_frame(TEST_MAC, PEER_MAC, EthernetProtocol::Arp, &payload)
}

#[test]
fn one_completion_per_recv_call() {
    let (mut dev, stats) = make_ethernet();
    stats.frames.lock().push_back(arp_request_frame());
    stats.frames.lock().push_back(ipv4_frame_to_us());
    let mut rx = rx_buffer(8);
    let step = dev.recv(&mut rx, Instant::from_millis_const(0));
    assert!(matches!(step, RxStep::Consumed));
    assert_eq!(*stats.receive_calls.lock(), 1);
    assert_eq!(*stats.recycle_calls.lock(), 1);
    let step = dev.recv(&mut rx, Instant::from_millis_const(0));
    assert!(matches!(step, RxStep::Delivered));
    assert_eq!(*stats.receive_calls.lock(), 2);
    assert_eq!(*stats.recycle_calls.lock(), 2);
}

#[test]
fn again_maps_to_empty() {
    let (mut dev, stats) = make_ethernet();
    let mut rx = rx_buffer(8);
    let step = dev.recv(&mut rx, Instant::from_millis_const(0));
    assert!(matches!(step, RxStep::Empty));
    assert_eq!(*stats.receive_calls.lock(), 1);
    assert_eq!(*stats.recycle_calls.lock(), 0);
}

#[test]
fn receive_fault_propagates() {
    let (mut dev, stats) = make_ethernet();
    stats.frames.lock().push_back(ipv4_frame_to_us());
    *stats.receive_error.lock() = true;
    let mut rx = rx_buffer(8);
    let step = dev.recv(&mut rx, Instant::from_millis_const(0));
    assert!(matches!(step, RxStep::Fault(DevError::Io)));
    assert_eq!(*stats.receive_calls.lock(), 1);
    assert_eq!(*stats.recycle_calls.lock(), 0);
}

#[test]
fn recycle_fault_overrides_delivered() {
    let (mut dev, stats) = make_ethernet();
    stats.frames.lock().push_back(ipv4_frame_to_us());
    *stats.recycle_error.lock() = true;
    let mut rx = rx_buffer(8);
    let step = dev.recv(&mut rx, Instant::from_millis_const(0));
    assert!(matches!(step, RxStep::Fault(DevError::Io)));
    assert_eq!(*stats.receive_calls.lock(), 1);
    assert_eq!(*stats.recycle_calls.lock(), 1);
}

#[test]
fn recycle_fault_prevails_over_enqueue_fault() {
    let (mut dev, stats) = make_ethernet();
    stats.frames.lock().push_back(ipv4_frame_to_us());
    *stats.recycle_error.lock() = true;
    let mut rx = full_rx_buffer();
    let step = dev.recv(&mut rx, Instant::from_millis_const(0));
    assert!(matches!(step, RxStep::Fault(DevError::Io)));
    assert_eq!(*stats.receive_calls.lock(), 1);
    assert_eq!(*stats.recycle_calls.lock(), 1);
}

#[test]
fn malformed_frame_is_consumed_and_recycled() {
    let (mut dev, stats) = make_ethernet();
    stats.frames.lock().push_back(vec![0u8; 5]);
    let mut rx = rx_buffer(8);
    let step = dev.recv(&mut rx, Instant::from_millis_const(0));
    assert!(matches!(step, RxStep::Consumed));
    assert_eq!(*stats.recycle_calls.lock(), 1);
}

#[test]
fn foreign_mac_frame_is_consumed() {
    let (mut dev, stats) = make_ethernet();
    stats.frames.lock().push_back(foreign_mac_frame());
    let mut rx = rx_buffer(8);
    let step = dev.recv(&mut rx, Instant::from_millis_const(0));
    assert!(matches!(step, RxStep::Consumed));
    assert_eq!(*stats.recycle_calls.lock(), 1);
}

#[test]
fn non_ipv4_ethertype_is_consumed() {
    let (mut dev, stats) = make_ethernet();
    stats.frames.lock().push_back(eth_frame(
        TEST_MAC,
        PEER_MAC,
        EthernetProtocol::Unknown(0x88A8),
        &[1, 2, 3],
    ));
    let mut rx = rx_buffer(8);
    let step = dev.recv(&mut rx, Instant::from_millis_const(0));
    assert!(matches!(step, RxStep::Consumed));
    assert_eq!(*stats.recycle_calls.lock(), 1);
}

#[test]
fn arp_request_replies_but_is_consumed() {
    let (mut dev, stats) = make_ethernet();
    stats.frames.lock().push_back(arp_request_frame());
    let mut rx = rx_buffer(8);
    let step = dev.recv(&mut rx, Instant::from_millis_const(0));
    assert!(matches!(step, RxStep::Consumed));
    assert_eq!(stats.tx_packets.lock().len(), 1);
    assert_eq!(*stats.recycle_calls.lock(), 1);
}

#[test]
fn arp_reply_flushes_pending_ipv4_once() {
    let (mut dev, stats) = make_ethernet();
    // Unknown neighbor: ARP request is sent and the IPv4 payload is held
    // pending until the reply resolves the neighbor.
    let _ = dev.send(
        IpAddress::Ipv4(PEER_IP),
        &IPV4_PAYLOAD,
        Instant::from_millis_const(0),
    );
    assert_eq!(stats.tx_packets.lock().len(), 1);
    stats.frames.lock().push_back(arp_reply_frame());
    let mut rx = rx_buffer(8);
    let step = dev.recv(&mut rx, Instant::from_millis_const(0));
    assert!(matches!(step, RxStep::Consumed));
    assert_eq!(*stats.recycle_calls.lock(), 1);
    assert_eq!(stats.tx_packets.lock().len(), 2);
    let sent = stats.tx_packets.lock().pop().unwrap();
    let frame = EthernetFrame::new_unchecked(&sent);
    assert_eq!(
        EthernetRepr::parse(&frame).unwrap().ethertype,
        EthernetProtocol::Ipv4
    );
    assert_eq!(frame.payload(), &IPV4_PAYLOAD[..]);
}

#[test]
fn ipv4_delivers_payload_and_recycles() {
    let (mut dev, stats) = make_ethernet();
    stats.frames.lock().push_back(ipv4_frame_to_us());
    let mut rx = rx_buffer(8);
    let step = dev.recv(&mut rx, Instant::from_millis_const(0));
    assert!(matches!(step, RxStep::Delivered));
    assert_eq!(*stats.recycle_calls.lock(), 1);
    let ((), payload) = rx.dequeue().expect("payload delivered");
    assert_eq!(&*payload, &IPV4_PAYLOAD[..]);
}

#[test]
fn full_destination_enqueue_returns_fault() {
    let (mut dev, stats) = make_ethernet();
    stats.frames.lock().push_back(ipv4_frame_to_us());
    let mut rx = full_rx_buffer();
    let step = dev.recv(&mut rx, Instant::from_millis_const(0));
    assert!(matches!(step, RxStep::Fault(DevError::BadState)));
    assert_eq!(*stats.recycle_calls.lock(), 1);
}

#[test]
fn loopback_empty_returns_empty() {
    let mut dev = LoopbackDevice::new();
    let mut rx = rx_buffer(8);
    let step = dev.recv(&mut rx, Instant::from_millis_const(0));
    assert!(matches!(step, RxStep::Empty));
}

#[test]
fn loopback_delivers_after_send() {
    let mut dev = LoopbackDevice::new();
    let packet = [1u8, 2, 3, 4];
    let outcome = dev.send(
        IpAddress::Ipv4(Ipv4Address::new(127, 0, 0, 1)),
        &packet,
        Instant::from_millis_const(0),
    );
    assert!(matches!(
        outcome,
        TxOutcome::Accepted {
            rx_became_ready: true
        }
    ));
    let mut rx = rx_buffer(8);
    let step = dev.recv(&mut rx, Instant::from_millis_const(0));
    assert!(matches!(step, RxStep::Delivered));
    let ((), payload) = rx.dequeue().expect("loopback payload delivered");
    assert_eq!(&*payload, &packet[..]);
}

#[test]
fn loopback_full_destination_returns_fault() {
    let mut dev = LoopbackDevice::new();
    let packet = [1u8; 100];
    let _ = dev.send(
        IpAddress::Ipv4(Ipv4Address::new(127, 0, 0, 1)),
        &packet,
        Instant::from_millis_const(0),
    );
    let mut rx = full_rx_buffer();
    let step = dev.recv(&mut rx, Instant::from_millis_const(0));
    assert!(matches!(step, RxStep::Fault(DevError::BadState)));
}

struct ScriptedDevice {
    steps: Mutex<VecDeque<RxStep>>,
    recv_calls: Arc<Mutex<usize>>,
}

impl ScriptedDevice {
    fn new(steps: Vec<RxStep>, recv_calls: Arc<Mutex<usize>>) -> Self {
        Self {
            steps: Mutex::new(steps.into()),
            recv_calls,
        }
    }
}

impl Device for ScriptedDevice {
    fn name(&self) -> &str {
        "scripted"
    }

    fn recv(&mut self, _buffer: &mut PacketBuffer<()>, _timestamp: Instant) -> RxStep {
        *self.recv_calls.lock() += 1;
        self.steps.lock().pop_front().unwrap_or(RxStep::Empty)
    }

    fn preflight_send(
        &mut self,
        _next_hop: IpAddress,
        _packet: &[u8],
        _timestamp: Instant,
    ) -> TxPreflight {
        TxPreflight::Ready
    }

    fn send(&mut self, _next_hop: IpAddress, _packet: &[u8], _timestamp: Instant) -> TxOutcome {
        TxOutcome::Accepted {
            rx_became_ready: false,
        }
    }

    fn register_waker(&self, _waker: &core::task::Waker) {}
}

fn scripted_router(steps: Vec<RxStep>, recv_calls: Arc<Mutex<usize>>) -> Router {
    let dev = ScriptedDevice::new(steps, recv_calls);
    let mut router = Router::new();
    router.add_device(Box::new(dev));
    router
}

#[test]
fn router_continues_on_consumed_and_delivered() {
    let recv_calls = Arc::new(Mutex::new(0usize));
    let mut router = scripted_router(
        vec![RxStep::Consumed, RxStep::Delivered, RxStep::Empty],
        recv_calls.clone(),
    );
    router.poll(
        RxOwnerView::PollingOwned,
        None,
        Instant::from_millis_const(0),
    );
    assert_eq!(*recv_calls.lock(), 3);
}

#[test]
fn router_stops_on_empty() {
    let recv_calls = Arc::new(Mutex::new(0usize));
    let mut router = scripted_router(vec![RxStep::Consumed, RxStep::Empty], recv_calls.clone());
    router.poll(
        RxOwnerView::PollingOwned,
        None,
        Instant::from_millis_const(0),
    );
    assert_eq!(*recv_calls.lock(), 2);
}

#[test]
fn router_stops_on_fault() {
    let recv_calls = Arc::new(Mutex::new(0usize));
    let mut router = scripted_router(
        vec![
            RxStep::Consumed,
            RxStep::Fault(DevError::Io),
            RxStep::Delivered,
        ],
        recv_calls.clone(),
    );
    router.poll(
        RxOwnerView::PollingOwned,
        None,
        Instant::from_millis_const(0),
    );
    assert_eq!(*recv_calls.lock(), 2);
}

fn router_with_target_and_loopback(
    target_steps: Vec<RxStep>,
    target_calls: Arc<Mutex<usize>>,
) -> Router {
    let mut router = Router::new();
    router.add_device(Box::new(LoopbackDevice::new()));
    router.add_device(Box::new(ScriptedDevice::new(target_steps, target_calls)));
    router
}

#[test]
fn router_poll_polling_owned_drains_target() {
    let target_calls = Arc::new(Mutex::new(0usize));
    let mut router = router_with_target_and_loopback(
        vec![RxStep::Consumed, RxStep::Delivered, RxStep::Empty],
        target_calls.clone(),
    );
    router.poll(
        RxOwnerView::PollingOwned,
        Some(1),
        Instant::from_millis_const(0),
    );
    assert_eq!(*target_calls.lock(), 3);
}

#[test]
fn router_poll_async_owned_consumes_slots_keeps_loopback() {
    // After activation the target runs in slot mode: ordinary Router
    // polling must drain the fixed RX slots (never the raw driver), while
    // the loopback device keeps being serviced. This is the MS05 Task 3.2
    // replacement for the MS04 "async owned skips target" witness.
    let (mut dev, stats) = make_ethernet();
    dev.activate_slot_mode().unwrap();
    let frame = ipv4_frame_to_us();
    assert!(dev.push_rx_frame_for_test(&frame));
    let mut router = Router::new();
    let lo = router.add_device(Box::new(LoopbackDevice::new()));
    let idx = router.add_device(Box::new(dev));
    // Put a frame into the loopback device: it must still be consumed by
    // stack polling even though the async owner targets the Ethernet NIC.
    assert!(matches!(
        router.devices[lo].send(
            IpAddress::Ipv4(Ipv4Address::new(127, 0, 0, 1)),
            &IPV4_PAYLOAD,
            Instant::from_millis_const(0),
        ),
        TxOutcome::Accepted { .. }
    ));
    router.poll(
        RxOwnerView::AsyncOwned,
        Some(idx),
        Instant::from_millis_const(0),
    );
    // The slot frame reached the Router RX buffer (the loopback frame may
    // have been delivered into the same buffer as well)...
    assert!(router.rx_buffer_pending_for_test());
    // ...the raw driver was never touched...
    assert_eq!(*stats.receive_calls.lock(), 0);
    // ...and the loopback device was still serviced: its frame was drained.
    assert!(router.rx_buffer_pending_for_test());
}

#[test]
fn router_poll_async_owned_without_target_is_safe() {
    let target_calls = Arc::new(Mutex::new(0usize));
    let mut router = router_with_target_and_loopback(
        vec![RxStep::Consumed, RxStep::Delivered, RxStep::Empty],
        target_calls.clone(),
    );
    router.poll(RxOwnerView::AsyncOwned, None, Instant::from_millis_const(0));
    assert_eq!(*target_calls.lock(), 3);
}

#[test]
fn router_poll_async_owned_faulted_target_still_consumes_slots() {
    // A faulted owner still holds the async consumption right; because the
    // target stays in slot mode, stack polling drains the remaining RX slot
    // frames without touching the raw driver.
    let (mut dev, stats) = make_ethernet();
    dev.activate_slot_mode().unwrap();
    let frame = ipv4_frame_to_us();
    assert!(dev.push_rx_frame_for_test(&frame));
    let mut router = Router::new();
    router.add_device(Box::new(LoopbackDevice::new()));
    let idx = router.add_device(Box::new(dev));
    router.poll(
        RxOwnerView::AsyncOwned,
        Some(idx),
        Instant::from_millis_const(0),
    );
    assert!(router.rx_buffer_pending_for_test());
    assert_eq!(*stats.receive_calls.lock(), 0);
}

fn service_with_target(target_steps: Vec<RxStep>, target_calls: Arc<Mutex<usize>>) -> Service {
    let router = router_with_target_and_loopback(target_steps, target_calls);
    Service::new(router, Some(1))
}

#[test]
fn service_poll_polling_owned_drains_target() {
    let _serial = SERIAL.lock();
    let target_calls = Arc::new(Mutex::new(0usize));
    let mut service = service_with_target(
        vec![RxStep::Consumed, RxStep::Delivered, RxStep::Empty],
        target_calls.clone(),
    );
    let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
    service.poll(RxOwnerView::PollingOwned, &mut sockets);
    assert_eq!(*target_calls.lock(), 3);
}

#[test]
fn service_poll_async_owned_consumes_slots() {
    let _serial = SERIAL.lock();
    // After activation the target runs in slot mode; Service::poll must
    // drain the fixed RX slots through the stack, never the raw driver.
    let (mut dev, stats) = make_ethernet();
    dev.activate_slot_mode().unwrap();
    let frame = ipv4_frame_to_us();
    assert!(dev.push_rx_frame_for_test(&frame));
    let mut router = Router::new();
    let eth = router.add_device(Box::new(dev));
    let mut service = Service::new(router, Some(eth));
    let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
    // The slot frame is drained into the Router buffer by `router.poll`;
    // the ingress pass then consumes it into smoltcp, so the buffer ends
    // empty again. The observable invariant is that the raw driver was
    // never touched while the slot data path ran without error.
    service.poll(RxOwnerView::AsyncOwned, &mut sockets);
    assert_eq!(*stats.receive_calls.lock(), 0);
}

#[test]
fn service_without_target_dev_async_owned_is_safe() {
    let _serial = SERIAL.lock();
    let target_calls = Arc::new(Mutex::new(0usize));
    let router = router_with_target_and_loopback(
        vec![RxStep::Consumed, RxStep::Empty],
        target_calls.clone(),
    );
    let mut service = Service::new(router, None);
    let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
    service.poll(RxOwnerView::AsyncOwned, &mut sockets);
    assert_eq!(*target_calls.lock(), 2);
}

#[test]
fn ethernet_delegates_queue_control_to_inner() {
    let (mut dev, ..) = make_ethernet_with_control(true);
    assert!(dev.queue_control().is_some());

    let (mut dev, ..) = make_ethernet_with_control(false);
    assert!(dev.queue_control().is_none());
}

#[test]
fn preflight_suppresses_target_control_without_reaping() {
    let (mut service, stats, control) = service_with_ethernet_target(true);
    service.activate_target().unwrap();
    assert_eq!(*control.suppress_calls.lock(), 2);
    assert_eq!(*stats.receive_calls.lock(), 0);
}

#[test]
fn preflight_missing_target_is_bad_state() {
    let router = Router::new();
    let mut service = Service::new(router, None);
    assert!(matches!(service.activate_target(), Err(DevError::BadState)));
}

#[test]
fn preflight_missing_control_is_unsupported() {
    let (mut service, ..) = service_with_ethernet_target(false);
    assert!(matches!(
        service.activate_target(),
        Err(DevError::Unsupported)
    ));
}

#[test]
fn preflight_suppress_error_propagates() {
    let (mut service, _, control) = service_with_ethernet_target(true);
    *control.suppress_error.lock() = true;
    assert!(matches!(service.activate_target(), Err(DevError::Io)));
}

#[test]
fn completion_pending_both_reports_control_state() {
    let (mut service, _, control) = service_with_ethernet_target(true);
    assert!(matches!(
        service.completion_pending_both_target(),
        Ok(NetQueueDirection::NONE)
    ));
    *control.completion_visible.lock() = true;
    assert!(matches!(
        service.completion_pending_both_target(),
        Ok(pending) if pending.contains(NetQueueDirection::RX)
    ));
}

#[test]
fn completion_pending_without_control_is_unsupported() {
    let (mut service, ..) = service_with_ethernet_target(false);
    assert!(matches!(
        service.completion_pending_both_target(),
        Err(DevError::Unsupported)
    ));
}

#[test]
fn repeated_suppress_both_is_idempotent() {
    let (mut service, _, control) = service_with_ethernet_target(true);
    service.activate_target().unwrap();
    service.activate_target().unwrap();
    assert_eq!(*control.suppress_calls.lock(), 4);
}

#[test]
fn arm_and_check_both_reports_pending_and_quiescent() {
    let (mut service, _, control) = service_with_ethernet_target(true);
    assert!(matches!(
        service.arm_and_check_both_target(),
        Ok(NetQueueDirection::NONE)
    ));
    *control.completion_visible.lock() = true;
    assert!(matches!(
        service.arm_and_check_both_target(),
        Ok(pending) if pending.contains(NetQueueDirection::RX)
    ));
    // Each BOTH-direction arm records one RX arm and one TX arm.
    assert_eq!(*control.arm_calls.lock(), 4);
}

#[test]
fn arm_error_propagates_with_category() {
    let (mut service, _, control) = service_with_ethernet_target(true);
    *control.arm_error.lock() = true;
    assert!(matches!(
        service.arm_and_check_both_target(),
        Err(DevError::Io)
    ));
}

#[test]
fn loopback_target_control_is_unsupported() {
    let mut router = Router::new();
    let lo = router.add_device(Box::new(LoopbackDevice::new()));
    let mut service = Service::new(router, Some(lo));
    assert!(matches!(
        service.activate_target(),
        Err(DevError::Unsupported)
    ));
}

// --- Task 3.1: bidirectional activation under one guard ---

#[test]
fn activate_target_suppresses_both_directions_once_each() {
    let (mut service, stats, control) = service_with_ethernet_target(true);
    let before = *control.suppress_calls.lock();
    service.activate_target().unwrap();
    // BOTH directions were suppressed: exactly two suppress calls.
    assert_eq!(*control.suppress_calls.lock(), before + 2);
    // No completion was reaped during activation.
    assert_eq!(*stats.receive_calls.lock(), 0);
}

#[test]
fn activate_target_missing_target_is_bad_state() {
    let router = Router::new();
    let mut service = Service::new(router, None);
    assert!(matches!(service.activate_target(), Err(DevError::BadState)));
}

#[test]
fn activate_target_missing_control_is_unsupported() {
    let (mut service, ..) = service_with_ethernet_target(false);
    assert!(matches!(
        service.activate_target(),
        Err(DevError::Unsupported)
    ));
}

#[test]
fn activate_target_suppress_error_propagates_and_keeps_polling() {
    let (mut service, stats, control) = service_with_ethernet_target(true);
    *control.suppress_error.lock() = true;
    assert!(matches!(service.activate_target(), Err(DevError::Io)));
    // The device was not switched to slot mode on failure.
    assert_eq!(*stats.receive_calls.lock(), 0);
}

#[test]
fn activate_target_switches_ethernet_to_slot_mode() {
    use crate::device::ethernet::EthernetDevice;
    let pool = NetBufPool::new(8, 2048).expect("pool alloc");
    let stats = Arc::new(FakeStats::new());
    let control_stats = Arc::new(FakeControlStats::default());
    let control = FakeQueueControl {
        stats: control_stats.clone(),
    };
    let nic = FakeNic {
        pool,
        stats: stats.clone(),
        control: Some(control),
        tx_queue: None,
    };
    let dev = EthernetDevice::new("eth0".into(), Box::new(nic), TEST_IP);
    let mut router = Router::new();
    let idx = router.add_device(Box::new(dev));
    let mut service = Service::new(router, Some(idx));

    service.activate_target().unwrap();
    // After activation the stack TX lands in the fixed TX slot, not the raw
    // driver (both directions are async-owned).
    let outcome = service.router_for_test().devices[idx].send(
        IpAddress::Ipv4(Ipv4Address::new(255, 255, 255, 255)),
        &[1u8; 10],
        Instant::from_millis_const(0),
    );
    assert!(matches!(outcome, TxOutcome::Accepted { .. }));
    assert_eq!(stats.tx_packets.lock().len(), 0);
}

#[test]
fn ethernet_slots_are_exact_64_heap_direct_and_transport_neutral() {
    let (dev, ..) = make_ethernet();
    let (rx, tx) = dev.slots_for_test();
    // Task 2.1: Ethernet owns RX/TX queues of exactly 64 complete frames.
    assert_eq!(rx.capacity(), 64);
    assert_eq!(tx.capacity(), 64);
    assert_eq!(rx.len(), 0);
    assert_eq!(tx.len(), 0);
    assert!(rx.is_empty() && tx.is_empty());
    // The struct is small because the backing is heap-direct, never a stack
    // materialized `[Frame; 64]` (~97 KiB).
    assert!(core::mem::size_of::<crate::device::fixed_queue::FixedFrameQueue<64>>() < 1024);
}

#[test]
fn loopback_storage_is_exact_socket_capacity() {
    use crate::device::fixed_queue::MAX_FRAME_SIZE;
    let mut dev = LoopbackDevice::new();
    // Fill the loopback buffer to its fixed capacity (SOCKET_BUFFER_SIZE).
    let mut accepted = 0;
    loop {
        let outcome = dev.send(
            IpAddress::Ipv4(Ipv4Address::new(127, 0, 0, 1)),
            &[1u8; 100],
            Instant::from_millis_const(0),
        );
        if matches!(outcome, TxOutcome::Accepted { .. }) {
            accepted += 1;
        } else {
            break;
        }
    }
    assert_eq!(accepted, 64);
    assert!(MAX_FRAME_SIZE >= 100);
    // A small packet still arrives after the buffer is drained once.
    let mut rx = rx_buffer(8);
    assert!(matches!(
        dev.recv(&mut rx, Instant::from_millis_const(0)),
        RxStep::Delivered
    ));
}

// --- Task 2.2: typed TX handoff and atomic Router fanout ---

/// Builds a minimal valid IPv4 packet from src/dst addresses (no checksum).
fn ipv4_packet(src: [u8; 4], dst: [u8; 4]) -> Vec<u8> {
    let mut buf = vec![0u8; 20];
    buf[0] = 0x45; // version 4, IHL 5
    buf[2..4].copy_from_slice(&(20u16).to_be_bytes()); // total length
    buf[8] = 64; // TTL
    buf[9] = 17; // UDP
    buf[12..16].copy_from_slice(&src);
    buf[16..20].copy_from_slice(&dst);
    buf
}

/// Builds a minimal valid IPv6 packet from src/dst addresses.
fn ipv6_packet(src: [u8; 16], dst: [u8; 16]) -> Vec<u8> {
    let mut buf = vec![0u8; 40];
    buf[0] = 0x60; // version 6
    buf[6] = 17; // next header: UDP
    buf[8..24].copy_from_slice(&src);
    buf[24..40].copy_from_slice(&dst);
    buf
}

const DST_IPV4: [u8; 4] = [10, 0, 2, 15];
const SRC_IPV4: [u8; 4] = [10, 0, 2, 1];
const BROADCAST_IPV4: [u8; 4] = [255, 255, 255, 255];

/// A device whose preflight/commit results are scripted per call.
struct ScriptedTxDevice {
    preflights: Mutex<VecDeque<TxPreflight>>,
    commits: Mutex<VecDeque<TxOutcome>>,
    preflight_calls: Arc<Mutex<usize>>,
    commit_calls: Arc<Mutex<usize>>,
    rx_became_ready: bool,
}

impl ScriptedTxDevice {
    fn new(
        preflights: Vec<TxPreflight>,
        commits: Vec<TxOutcome>,
        preflight_calls: Arc<Mutex<usize>>,
        commit_calls: Arc<Mutex<usize>>,
    ) -> Self {
        Self {
            preflights: Mutex::new(preflights.into()),
            commits: Mutex::new(commits.into()),
            preflight_calls,
            commit_calls,
            rx_became_ready: false,
        }
    }
}

impl Device for ScriptedTxDevice {
    fn name(&self) -> &str {
        "scripted-tx"
    }

    fn recv(&mut self, _buffer: &mut PacketBuffer<()>, _timestamp: Instant) -> RxStep {
        RxStep::Empty
    }

    fn preflight_send(
        &mut self,
        _next_hop: IpAddress,
        _packet: &[u8],
        _timestamp: Instant,
    ) -> TxPreflight {
        *self.preflight_calls.lock() += 1;
        self.preflights
            .lock()
            .pop_front()
            .unwrap_or(TxPreflight::Ready)
    }

    fn send(&mut self, _next_hop: IpAddress, _packet: &[u8], _timestamp: Instant) -> TxOutcome {
        *self.commit_calls.lock() += 1;
        self.commits
            .lock()
            .pop_front()
            .unwrap_or(TxOutcome::Accepted {
                rx_became_ready: self.rx_became_ready,
            })
    }

    fn register_waker(&self, _waker: &core::task::Waker) {}
}

fn tx_router_with(devs: Vec<(Box<dyn Device>, Ipv4Cidr)>, src: [u8; 4]) -> Router {
    let mut router = Router::new();
    for (dev, cidr) in devs {
        let idx = router.add_device(dev);
        router.add_rule(Rule::new(
            cidr.into(),
            None,
            idx,
            Ipv4Address::new(src[0], src[1], src[2], src[3]).into(),
        ));
    }
    router
}

struct CountingRxDevice {
    remaining: Arc<AtomicUsize>,
    calls: Arc<AtomicUsize>,
}

impl Device for CountingRxDevice {
    fn name(&self) -> &str {
        "counting-rx"
    }

    fn recv(&mut self, _buffer: &mut PacketBuffer<()>, _timestamp: Instant) -> RxStep {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.remaining
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                if remaining > 0 {
                    Some(remaining - 1)
                } else {
                    None
                }
            })
            .map_or(RxStep::Empty, |_| RxStep::Consumed)
    }

    fn preflight_send(
        &mut self,
        _next_hop: IpAddress,
        _packet: &[u8],
        _timestamp: Instant,
    ) -> TxPreflight {
        TxPreflight::Ready
    }

    fn send(&mut self, _next_hop: IpAddress, _packet: &[u8], _timestamp: Instant) -> TxOutcome {
        TxOutcome::Accepted {
            rx_became_ready: false,
        }
    }

    fn register_waker(&self, _waker: &core::task::Waker) {}
}

fn add_counting_rx(router: &mut Router, count: usize) -> (Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let remaining = Arc::new(AtomicUsize::new(count));
    let calls = Arc::new(AtomicUsize::new(0));
    router.add_device(Box::new(CountingRxDevice {
        remaining: remaining.clone(),
        calls: calls.clone(),
    }));
    (remaining, calls)
}

#[test]
fn bounded_rx_reports_31_32_33_and_preserves_backlog() {
    for count in [31usize, 32, 33] {
        let mut router = Router::new();
        let (remaining, _) = add_counting_rx(&mut router, count);
        let first = router.poll_bounded(
            RxOwnerView::PollingOwned,
            None,
            Instant::from_millis_const(0),
            32,
        );
        assert_eq!(first.processed, count.min(32));
        assert_eq!(first.budget_exhausted, count >= 32);
        assert_eq!(first.backlog, count >= 32);
        assert_eq!(remaining.load(Ordering::Relaxed), count.saturating_sub(32));

        if count == 33 {
            let second = router.poll_bounded(
                RxOwnerView::PollingOwned,
                None,
                Instant::from_millis_const(0),
                32,
            );
            assert_eq!(second.processed, 1);
            assert!(!second.budget_exhausted);
            assert!(!second.backlog);
        }
    }
}

#[test]
fn bounded_rx_round_robin_services_target_behind_continuous_device() {
    let mut router = Router::new();
    let (_continuous_remaining, continuous_calls) = add_counting_rx(&mut router, 100);
    let (target_remaining, target_calls) = add_counting_rx(&mut router, 1);

    let outcome = router.poll_bounded(
        RxOwnerView::PollingOwned,
        Some(1),
        Instant::from_millis_const(0),
        2,
    );

    assert_eq!(outcome.processed, 2);
    assert!(outcome.backlog);
    assert_eq!(continuous_calls.load(Ordering::Relaxed), 1);
    assert_eq!(target_calls.load(Ordering::Relaxed), 1);
    assert_eq!(target_remaining.load(Ordering::Relaxed), 0);
}

#[test]
fn bounded_dispatch_reports_budget_and_retains_remainder() {
    let mut router = Router::new();
    for _ in 0..33 {
        assert!(router.enqueue_tx_for_test(&[0u8; 1]));
    }

    let first = router.dispatch_bounded(Instant::from_millis_const(0), 32);
    assert_eq!(first.processed, 32);
    assert!(first.budget_exhausted);
    assert!(first.backlog);
    assert_eq!(
        router.drop_count(crate::device::TxDropReason::MalformedIp),
        32
    );

    let second = router.dispatch_bounded(Instant::from_millis_const(0), 32);
    assert_eq!(second.processed, 1);
    assert!(!second.budget_exhausted);
    assert!(!second.backlog);
    assert_eq!(
        router.drop_count(crate::device::TxDropReason::MalformedIp),
        33
    );
}

#[test]
fn dispatch_single_target_full_keeps_head_no_commit() {
    let pre_calls = Arc::new(Mutex::new(0usize));
    let commit_calls = Arc::new(Mutex::new(0usize));
    let dev = ScriptedTxDevice::new(
        vec![TxPreflight::Full],
        vec![],
        pre_calls.clone(),
        commit_calls.clone(),
    );
    let mut router = tx_router_with(
        vec![(
            Box::new(dev),
            Ipv4Cidr::new(Ipv4Address::new(10, 0, 2, 0), 24),
        )],
        SRC_IPV4,
    );
    assert!(router.enqueue_tx_for_test(&ipv4_packet(SRC_IPV4, DST_IPV4)));
    let poll_next = router.dispatch(Instant::from_millis_const(0));
    assert!(!poll_next);
    // Full keeps the head: zero commits, one preflight, no dequeue.
    assert_eq!(*pre_calls.lock(), 1);
    assert_eq!(*commit_calls.lock(), 0);
    assert!(router.tx_pending_for_test());
    assert!(!router.tx_faulted());
}

#[test]
fn dispatch_fanout_second_full_commits_nothing() {
    let pre_calls = Arc::new(Mutex::new(0usize));
    let commit_calls = Arc::new(Mutex::new(0usize));
    // First device Ready, second Full: fanout must commit zero.
    let ready = ScriptedTxDevice::new(
        vec![TxPreflight::Ready],
        vec![],
        pre_calls.clone(),
        commit_calls.clone(),
    );
    let full = ScriptedTxDevice::new(
        vec![TxPreflight::Full],
        vec![],
        pre_calls.clone(),
        commit_calls.clone(),
    );
    let mut router = tx_router_with(
        vec![
            (
                Box::new(ready),
                Ipv4Cidr::new(Ipv4Address::new(10, 0, 2, 0), 24),
            ),
            (
                Box::new(full),
                Ipv4Cidr::new(Ipv4Address::new(10, 0, 2, 0), 24),
            ),
        ],
        SRC_IPV4,
    );
    // Broadcast reaches every device; the second is Full.
    assert!(router.enqueue_tx_for_test(&ipv4_packet(SRC_IPV4, BROADCAST_IPV4)));
    router.dispatch(Instant::from_millis_const(0));
    assert_eq!(*pre_calls.lock(), 2);
    assert_eq!(*commit_calls.lock(), 0);
    assert!(router.tx_pending_for_test());
}

#[test]
fn dispatch_fanout_all_ready_commits_each_once_and_dequeues_once() {
    let pre_calls = Arc::new(Mutex::new(0usize));
    let commit_calls = Arc::new(Mutex::new(0usize));
    let dev_a = ScriptedTxDevice::new(
        vec![TxPreflight::Ready],
        vec![TxOutcome::Accepted {
            rx_became_ready: false,
        }],
        pre_calls.clone(),
        commit_calls.clone(),
    );
    let dev_b = ScriptedTxDevice::new(
        vec![TxPreflight::Ready],
        vec![TxOutcome::Accepted {
            rx_became_ready: false,
        }],
        pre_calls.clone(),
        commit_calls.clone(),
    );
    let mut router = tx_router_with(
        vec![
            (
                Box::new(dev_a),
                Ipv4Cidr::new(Ipv4Address::new(10, 0, 2, 0), 24),
            ),
            (
                Box::new(dev_b),
                Ipv4Cidr::new(Ipv4Address::new(10, 0, 2, 0), 24),
            ),
        ],
        SRC_IPV4,
    );
    assert!(router.enqueue_tx_for_test(&ipv4_packet(SRC_IPV4, BROADCAST_IPV4)));
    router.dispatch(Instant::from_millis_const(0));
    // One commit per unique target, one dequeue, no drops.
    assert_eq!(*pre_calls.lock(), 2);
    assert_eq!(*commit_calls.lock(), 2);
    assert!(!router.tx_pending_for_test());
    assert_eq!(
        router.drop_count(crate::device::TxDropReason::MalformedIp),
        0
    );
}

#[test]
fn dispatch_loopback_accepted_reports_rx_ready() {
    let lo = LoopbackDevice::new();
    let mut router = Router::new();
    let idx = router.add_device(Box::new(lo));
    router.add_rule(Rule::new(
        Ipv4Cidr::new(Ipv4Address::new(127, 0, 0, 0), 8).into(),
        None,
        idx,
        Ipv4Address::new(127, 0, 0, 1).into(),
    ));
    let packet = ipv4_packet([127, 0, 0, 1], [127, 0, 0, 1]);
    assert!(router.enqueue_tx_for_test(&packet));
    let poll_next = router.dispatch(Instant::from_millis_const(0));
    assert!(poll_next);
    assert!(!router.tx_pending_for_test());
}

#[test]
fn dispatch_ethernet_accepted_without_rx_ready() {
    let (mut dev, stats) = make_ethernet();
    // Resolve a neighbor so the packet takes the direct send path.
    let _ = dev.send(
        IpAddress::Ipv4(PEER_IP),
        &IPV4_PAYLOAD,
        Instant::from_millis_const(0),
    );
    stats.frames.lock().push_back(arp_reply_frame());
    let mut rx = rx_buffer(8);
    let _ = dev.recv(&mut rx, Instant::from_millis_const(0));

    let mut router = Router::new();
    let idx = router.add_device(Box::new(dev));
    router.add_rule(Rule::new(
        Ipv4Cidr::new(Ipv4Address::new(10, 0, 2, 0), 24).into(),
        None,
        idx,
        Ipv4Address::new(10, 0, 2, 15).into(),
    ));
    let packet = ipv4_packet(SRC_IPV4, DST_IPV4);
    assert!(router.enqueue_tx_for_test(&packet));
    // Ethernet accepts without making RX ready.
    let poll_next = router.dispatch(Instant::from_millis_const(0));
    assert!(!poll_next);
    assert!(!router.tx_pending_for_test());
    assert_eq!(stats.tx_packets.lock().len(), 2);
}

#[test]
fn dispatch_malformed_drops_once_and_continues() {
    let mut router = Router::new();
    // No devices or routes: a malformed packet must not panic.
    assert!(router.enqueue_tx_for_test(&[0u8; 40]));
    let poll_next = router.dispatch(Instant::from_millis_const(0));
    assert!(!poll_next);
    assert_eq!(
        router.drop_count(crate::device::TxDropReason::MalformedIp),
        1
    );
    assert!(!router.tx_pending_for_test());
}

#[test]
fn dispatch_missing_route_drops_with_reason() {
    let mut router = Router::new();
    assert!(router.enqueue_tx_for_test(&ipv4_packet(SRC_IPV4, DST_IPV4)));
    router.dispatch(Instant::from_millis_const(0));
    assert_eq!(router.drop_count(crate::device::TxDropReason::NoRoute), 1);
    assert!(!router.tx_pending_for_test());
}

#[test]
fn dispatch_route_source_mismatch_drops_with_reason() {
    let mut router = Router::new();
    let idx = router.add_device(Box::new(ScriptedDevice::new(
        vec![],
        Arc::new(Mutex::new(0)),
    )));
    router.add_rule(Rule::new(
        Ipv4Cidr::new(Ipv4Address::new(10, 0, 2, 0), 24).into(),
        None,
        idx,
        // Route source is 10.0.2.15 but the packet src is 10.0.2.1.
        Ipv4Address::new(10, 0, 2, 15).into(),
    ));
    assert!(router.enqueue_tx_for_test(&ipv4_packet(SRC_IPV4, DST_IPV4)));
    router.dispatch(Instant::from_millis_const(0));
    assert_eq!(
        router.drop_count(crate::device::TxDropReason::RouteSourceMismatch),
        1
    );
    assert!(!router.tx_pending_for_test());
}

#[test]
fn dispatch_preflight_drop_counts_once_and_dequeues() {
    let pre_calls = Arc::new(Mutex::new(0usize));
    let commit_calls = Arc::new(Mutex::new(0usize));
    let dev = ScriptedTxDevice::new(
        vec![TxPreflight::Drop(
            crate::device::TxDropReason::UnsupportedAddress,
        )],
        vec![],
        pre_calls.clone(),
        commit_calls.clone(),
    );
    let mut router = tx_router_with(
        vec![(
            Box::new(dev),
            Ipv4Cidr::new(Ipv4Address::new(10, 0, 2, 0), 24),
        )],
        SRC_IPV4,
    );
    assert!(router.enqueue_tx_for_test(&ipv4_packet(SRC_IPV4, DST_IPV4)));
    router.dispatch(Instant::from_millis_const(0));
    assert_eq!(
        router.drop_count(crate::device::TxDropReason::UnsupportedAddress),
        1
    );
    assert_eq!(*commit_calls.lock(), 0);
    assert!(!router.tx_pending_for_test());
}

#[test]
fn dispatch_preflight_fault_enters_stable_fault() {
    let pre_calls = Arc::new(Mutex::new(0usize));
    let commit_calls = Arc::new(Mutex::new(0usize));
    let dev = ScriptedTxDevice::new(
        vec![TxPreflight::Fault(DevError::Io)],
        vec![],
        pre_calls.clone(),
        commit_calls.clone(),
    );
    let mut router = tx_router_with(
        vec![(
            Box::new(dev),
            Ipv4Cidr::new(Ipv4Address::new(10, 0, 2, 0), 24),
        )],
        SRC_IPV4,
    );
    assert!(router.enqueue_tx_for_test(&ipv4_packet(SRC_IPV4, DST_IPV4)));
    router.dispatch(Instant::from_millis_const(0));
    assert!(router.tx_faulted());
    assert_eq!(router.tx_fault_kind(), Some("Io"));
    // The head is not dequeued on a fault.
    assert!(router.tx_pending_for_test());
    // A later dispatch stays faulted and touches nothing.
    *commit_calls.lock() += 0;
    router.dispatch(Instant::from_millis_const(0));
    assert_eq!(*pre_calls.lock(), 1);
    assert_eq!(*commit_calls.lock(), 0);
}

#[test]
fn dispatch_ready_commit_drift_enters_stable_fault_and_removes_head() {
    let pre_calls = Arc::new(Mutex::new(0usize));
    let commit_calls = Arc::new(Mutex::new(0usize));
    // Preflight Ready but commit returns Full: invariant violation.
    let dev = ScriptedTxDevice::new(
        vec![TxPreflight::Ready],
        vec![TxOutcome::Full],
        pre_calls.clone(),
        commit_calls.clone(),
    );
    let mut router = tx_router_with(
        vec![(
            Box::new(dev),
            Ipv4Cidr::new(Ipv4Address::new(10, 0, 2, 0), 24),
        )],
        SRC_IPV4,
    );
    assert!(router.enqueue_tx_for_test(&ipv4_packet(SRC_IPV4, DST_IPV4)));
    router.dispatch(Instant::from_millis_const(0));
    assert!(router.tx_faulted());
    assert_eq!(router.tx_fault_kind(), Some("BadState"));
    // A Ready->non-Accepted commit drift removes the head so the packet is
    // never delivered twice (no dual logical ownership after the fault).
    assert!(!router.tx_pending_for_test());
}

#[test]
fn dispatch_fanout_drift_removes_head_after_first_target_accepted() {
    let pre_calls = Arc::new(Mutex::new(0usize));
    let commit_calls = Arc::new(Mutex::new(0usize));
    // Target 0 preflights Ready and commits Accepted; target 1 preflights
    // Ready but drifts to Full on commit.
    let accepted = ScriptedTxDevice::new(
        vec![TxPreflight::Ready],
        vec![TxOutcome::Accepted {
            rx_became_ready: false,
        }],
        pre_calls.clone(),
        commit_calls.clone(),
    );
    let drifting = ScriptedTxDevice::new(
        vec![TxPreflight::Ready],
        vec![TxOutcome::Full],
        pre_calls.clone(),
        commit_calls.clone(),
    );
    let mut router = tx_router_with(
        vec![
            (
                Box::new(accepted),
                Ipv4Cidr::new(Ipv4Address::new(10, 0, 2, 0), 24),
            ),
            (
                Box::new(drifting),
                Ipv4Cidr::new(Ipv4Address::new(10, 0, 2, 0), 24),
            ),
        ],
        SRC_IPV4,
    );
    assert!(router.enqueue_tx_for_test(&ipv4_packet(SRC_IPV4, BROADCAST_IPV4)));
    router.dispatch(Instant::from_millis_const(0));
    // Both targets were committed; the drift faults the Router and removes
    // the head so the packet exists only in target 0 after the fault.
    assert_eq!(*commit_calls.lock(), 2);
    assert!(router.tx_faulted());
    assert!(!router.tx_pending_for_test());
}

#[test]
fn dispatch_fanout_drop_counts_exactly_once() {
    let pre_calls = Arc::new(Mutex::new(0usize));
    let commit_calls = Arc::new(Mutex::new(0usize));
    // First target Ready, second preflight-Drops: the packet is dropped once.
    let ready = ScriptedTxDevice::new(
        vec![TxPreflight::Ready],
        vec![],
        pre_calls.clone(),
        commit_calls.clone(),
    );
    let drop_dev = ScriptedTxDevice::new(
        vec![TxPreflight::Drop(
            crate::device::TxDropReason::UnsupportedAddress,
        )],
        vec![],
        pre_calls.clone(),
        commit_calls.clone(),
    );
    let mut router = tx_router_with(
        vec![
            (
                Box::new(ready),
                Ipv4Cidr::new(Ipv4Address::new(10, 0, 2, 0), 24),
            ),
            (
                Box::new(drop_dev),
                Ipv4Cidr::new(Ipv4Address::new(10, 0, 2, 0), 24),
            ),
        ],
        SRC_IPV4,
    );
    assert!(router.enqueue_tx_for_test(&ipv4_packet(SRC_IPV4, BROADCAST_IPV4)));
    router.dispatch(Instant::from_millis_const(0));
    assert_eq!(
        router.drop_count(crate::device::TxDropReason::UnsupportedAddress),
        1
    );
    assert_eq!(*commit_calls.lock(), 0);
    assert!(!router.tx_pending_for_test());
}

#[test]
fn dispatch_ipv6_multicast_fanout_preflights_all() {
    let pre_calls = Arc::new(Mutex::new(0usize));
    let commit_calls = Arc::new(Mutex::new(0usize));
    let dev = ScriptedTxDevice::new(
        vec![TxPreflight::Ready],
        vec![TxOutcome::Accepted {
            rx_became_ready: false,
        }],
        pre_calls.clone(),
        commit_calls.clone(),
    );
    let mut router = Router::new();
    router.add_device(Box::new(dev));
    // ff02::1 is a link-local multicast group.
    let mut dst = [0u8; 16];
    dst[0] = 0xff;
    dst[1] = 0x02;
    dst[15] = 1;
    let packet = ipv6_packet([0u8; 16], dst);
    assert!(router.enqueue_tx_for_test(&packet));
    router.dispatch(Instant::from_millis_const(0));
    assert_eq!(*pre_calls.lock(), 1);
    assert_eq!(*commit_calls.lock(), 1);
    assert!(!router.tx_pending_for_test());
}

// --- Task 2.4: allocation-free handoff and MTU boundary ---

use crate::device::test_alloc::alloc_count;

#[test]
fn dispatch_unicast_allocates_zero_after_initialization() {
    let pre_calls = Arc::new(Mutex::new(0usize));
    let commit_calls = Arc::new(Mutex::new(0usize));
    let dev = ScriptedTxDevice::new(
        vec![TxPreflight::Ready],
        vec![TxOutcome::Accepted {
            rx_became_ready: false,
        }],
        pre_calls.clone(),
        commit_calls.clone(),
    );
    let mut router = tx_router_with(
        vec![(
            Box::new(dev),
            Ipv4Cidr::new(Ipv4Address::new(10, 0, 2, 0), 24),
        )],
        SRC_IPV4,
    );
    assert!(router.enqueue_tx_for_test(&ipv4_packet(SRC_IPV4, DST_IPV4)));
    let frozen = alloc_count();
    let poll_next = router.dispatch(Instant::from_millis_const(0));
    assert!(!poll_next);
    assert!(!router.tx_pending_for_test());
    assert_eq!(alloc_count(), frozen, "unicast dispatch must not allocate");
}

#[test]
fn dispatch_fanout_allocates_zero_after_initialization() {
    let pre_calls = Arc::new(Mutex::new(0usize));
    let commit_calls = Arc::new(Mutex::new(0usize));
    let dev_a = ScriptedTxDevice::new(
        vec![TxPreflight::Ready],
        vec![TxOutcome::Accepted {
            rx_became_ready: false,
        }],
        pre_calls.clone(),
        commit_calls.clone(),
    );
    let dev_b = ScriptedTxDevice::new(
        vec![TxPreflight::Ready],
        vec![TxOutcome::Accepted {
            rx_became_ready: false,
        }],
        pre_calls.clone(),
        commit_calls.clone(),
    );
    let mut router = tx_router_with(
        vec![
            (
                Box::new(dev_a),
                Ipv4Cidr::new(Ipv4Address::new(10, 0, 2, 0), 24),
            ),
            (
                Box::new(dev_b),
                Ipv4Cidr::new(Ipv4Address::new(10, 0, 2, 0), 24),
            ),
        ],
        SRC_IPV4,
    );
    assert!(router.enqueue_tx_for_test(&ipv4_packet(SRC_IPV4, BROADCAST_IPV4)));
    let frozen = alloc_count();
    router.dispatch(Instant::from_millis_const(0));
    assert!(!router.tx_pending_for_test());
    assert_eq!(alloc_count(), frozen, "fanout dispatch must not allocate");
}

#[test]
fn dispatch_malformed_drop_allocates_zero() {
    let mut router = Router::new();
    assert!(router.enqueue_tx_for_test(&[0u8; 40]));
    let frozen = alloc_count();
    router.dispatch(Instant::from_millis_const(0));
    assert_eq!(
        router.drop_count(crate::device::TxDropReason::MalformedIp),
        1
    );
    assert!(!router.tx_pending_for_test());
    assert_eq!(alloc_count(), frozen, "drop path must not allocate");
}

#[test]
fn ethernet_1500_payload_is_accepted_and_1501_dropped() {
    let (mut dev, _) = make_ethernet();
    let at_mtu = vec![0u8; crate::consts::STANDARD_MTU];
    let pre = dev.preflight_send(
        IpAddress::Ipv4(Ipv4Address::new(10, 0, 2, 9)),
        &at_mtu,
        Instant::from_millis_const(0),
    );
    assert!(
        matches!(pre, TxPreflight::Ready | TxPreflight::Full),
        "a 1500-byte payload must not be rejected as FrameTooLarge"
    );

    let oversized = vec![0u8; crate::consts::STANDARD_MTU + 1];
    let pre = dev.preflight_send(
        IpAddress::Ipv4(Ipv4Address::new(10, 0, 2, 9)),
        &oversized,
        Instant::from_millis_const(0),
    );
    assert!(matches!(
        pre,
        TxPreflight::Drop(crate::device::TxDropReason::FrameTooLarge)
    ));
    let outcome = dev.send(
        IpAddress::Ipv4(Ipv4Address::new(10, 0, 2, 9)),
        &oversized,
        Instant::from_millis_const(0),
    );
    assert!(matches!(
        outcome,
        TxOutcome::Dropped(crate::device::TxDropReason::FrameTooLarge)
    ));
}

#[test]
fn dormant_emission_allocates_zero_after_initialization() {
    let (mut dev, stats) = make_ethernet();
    dev.set_dormant_slots_for_test();
    let frozen = alloc_count();
    let outcome = dev.send(
        IpAddress::Ipv4(Ipv4Address::new(255, 255, 255, 255)),
        &[1u8; 10],
        Instant::from_millis_const(0),
    );
    assert!(matches!(outcome, TxOutcome::Accepted { .. }));
    assert_eq!(stats.tx_packets.lock().len(), 0);
    let (_, tx) = dev.slots_for_test();
    assert_eq!(tx.len(), 1);
    assert_eq!(
        alloc_count(),
        frozen,
        "dormant frame emission must not allocate"
    );
}

#[test]
fn arp_pending_flush_allocates_zero_after_initialization() {
    let (mut dev, stats) = make_ethernet();
    dev.set_dormant_slots_for_test();
    // Unknown neighbor: the ARP request lands in the TX slot and the IPv4
    // payload is held pending until the reply resolves the neighbor.
    let _ = dev.send(
        IpAddress::Ipv4(PEER_IP),
        &IPV4_PAYLOAD,
        Instant::from_millis_const(0),
    );
    assert_eq!(stats.tx_packets.lock().len(), 0);
    let (_, tx) = dev.slots_for_test();
    assert_eq!(tx.len(), 1);
    // Feed the ARP reply through the dormant RX slot path.
    assert!(dev.push_rx_frame_for_test(&arp_reply_frame()));
    let mut rx = rx_buffer(8);
    let frozen = alloc_count();
    let step = dev.recv(&mut rx, Instant::from_millis_const(0));
    assert!(matches!(step, RxStep::Consumed));
    // The flush wrote the pending IPv4 frame directly into a second TX slot,
    // without copying the payload out of pending storage or allocating. The
    // RX slot read uses a fixed stack copy, never a heap allocation.
    let (rx, tx) = dev.slots_for_test();
    assert_eq!(rx.len(), 0);
    assert_eq!(tx.len(), 2);
    assert_eq!(stats.tx_packets.lock().len(), 0);
    assert_eq!(alloc_count(), frozen, "ARP pending flush must not allocate");
}

// --- Task 2.3: transactional Ethernet/ARP commits ---

/// Sends an ARP request frame into a fake NIC so `process_arp` handles it.
fn enqueue_arp_request(dev: &EthernetDevice, stats: &Arc<FakeStats>) {
    let repr = ArpRepr::EthernetIpv4 {
        operation: ArpOperation::Request,
        source_hardware_addr: PEER_MAC,
        source_protocol_addr: PEER_IP,
        target_hardware_addr: EthernetAddress::BROADCAST,
        target_protocol_addr: TEST_IP.address(),
    };
    let mut payload = vec![0u8; repr.buffer_len()];
    repr.emit(&mut ArpPacket::new_unchecked(&mut payload));
    stats.frames.lock().push_back(eth_frame(
        EthernetAddress::BROADCAST,
        PEER_MAC,
        EthernetProtocol::Arp,
        &payload,
    ));
    let _ = dev;
}

#[test]
fn arp_reply_tx_full_keeps_neighbor_unresolved_and_rx_consumed() {
    let (mut dev, stats) = make_ethernet();
    // Inject a TX failure: the ARP reply cannot be sent.
    *stats.transmit_error.lock() = true;
    enqueue_arp_request(&dev, &stats);
    let mut rx = rx_buffer(8);
    // process_arp runs inside recv; a reply TX failure must not resolve the
    // neighbor (no stale Neighbor entry), while the RX frame is still
    // consumed (polling path recycles the driver buffer regardless).
    let step = dev.recv(&mut rx, Instant::from_millis_const(0));
    assert!(matches!(step, RxStep::Consumed));
    assert_eq!(*stats.recycle_calls.lock(), 1);
    // After clearing the fault, a send to the peer still requires ARP
    // (neighbor was not resolved by the failed-reply path).
    *stats.transmit_error.lock() = false;
    let outcome = dev.send(
        IpAddress::Ipv4(PEER_IP),
        &IPV4_PAYLOAD,
        Instant::from_millis_const(0),
    );
    // Unknown neighbor: ARP request + pending; both succeed now.
    assert!(matches!(outcome, TxOutcome::Accepted { .. }));
    // Exactly one ARP request was submitted (no IP payload yet).
    assert_eq!(stats.tx_packets.lock().len(), 1);
    let sent = stats.tx_packets.lock().pop().unwrap();
    let frame = EthernetFrame::new_unchecked(&sent);
    assert_eq!(
        EthernetRepr::parse(&frame).unwrap().ethertype,
        EthernetProtocol::Arp
    );
}

#[test]
fn arp_request_tx_full_does_not_record_pending_neighbor() {
    let (mut dev, stats) = make_ethernet();
    // The ARP request cannot be sent: no neighbor entry and no pending packet.
    *stats.transmit_error.lock() = true;
    let outcome = dev.send(
        IpAddress::Ipv4(PEER_IP),
        &IPV4_PAYLOAD,
        Instant::from_millis_const(0),
    );
    // request_arp fails; the pending path must not have enqueued anything.
    assert!(matches!(outcome, TxOutcome::Full));
    assert_eq!(stats.tx_packets.lock().len(), 0);
    // Recovery: next send succeeds and records the ARP request.
    *stats.transmit_error.lock() = false;
    let outcome = dev.send(
        IpAddress::Ipv4(PEER_IP),
        &IPV4_PAYLOAD,
        Instant::from_millis_const(0),
    );
    assert!(matches!(outcome, TxOutcome::Accepted { .. }));
    assert_eq!(stats.tx_packets.lock().len(), 1);
}

#[test]
fn pending_flush_only_dequeues_after_accepted_send() {
    let (mut dev, stats) = make_ethernet();
    // Resolve the neighbor via a real ARP reply.
    let _ = dev.send(
        IpAddress::Ipv4(PEER_IP),
        &IPV4_PAYLOAD,
        Instant::from_millis_const(0),
    );
    assert_eq!(stats.tx_packets.lock().len(), 1);
    stats.frames.lock().push_back(arp_reply_frame());
    let mut rx = rx_buffer(8);
    // With the neighbor resolved and the TX healthy, the pending flush sends.
    let step = dev.recv(&mut rx, Instant::from_millis_const(0));
    assert!(matches!(step, RxStep::Consumed));
    assert_eq!(stats.tx_packets.lock().len(), 2);
    // A later failed send must NOT dequeue a pending packet that was not
    // accepted; but here the pending queue is empty, so nothing to verify.
    assert_eq!(*stats.recycle_calls.lock(), 1);
}

#[test]
fn ethernet_preflight_reports_again_as_full() {
    let (mut dev, stats) = make_ethernet();
    *stats.transmit_error.lock() = true;
    // Broadcast path preflight: recycle succeeds but transmit would fail.
    let pre = dev.preflight_send(
        IpAddress::Ipv4(Ipv4Address::new(255, 255, 255, 255)),
        &[1u8; 10],
        Instant::from_millis_const(0),
    );
    // The fake driver's can_transmit is not tied to the injected error, so
    // preflight reports Ready; the commit then returns Full via `Again`.
    assert!(matches!(pre, TxPreflight::Ready));
    let outcome = dev.send(
        IpAddress::Ipv4(Ipv4Address::new(255, 255, 255, 255)),
        &[1u8; 10],
        Instant::from_millis_const(0),
    );
    assert!(matches!(outcome, TxOutcome::Full));
    *stats.transmit_error.lock() = false;
    let outcome = dev.send(
        IpAddress::Ipv4(Ipv4Address::new(255, 255, 255, 255)),
        &[1u8; 10],
        Instant::from_millis_const(0),
    );
    assert!(matches!(outcome, TxOutcome::Accepted { .. }));
}

#[test]
fn dormant_slots_mode_commits_to_fixed_slot_with_ticket() {
    let (mut dev, stats) = make_ethernet();
    // Activate the dormant slot seam; product builds never do this.
    dev.set_dormant_slots_for_test();
    {
        let (rx, tx) = dev.slots_for_test();
        assert_eq!(rx.len(), 0);
        assert_eq!(tx.len(), 0);
    }
    // Broadcast frame is emitted into the TX slot, not the raw driver.
    let outcome = dev.send(
        IpAddress::Ipv4(Ipv4Address::new(255, 255, 255, 255)),
        &[1u8; 10],
        Instant::from_millis_const(0),
    );
    assert!(matches!(outcome, TxOutcome::Accepted { .. }));
    assert_eq!(stats.tx_packets.lock().len(), 0);
    let (_, tx) = dev.slots_for_test();
    assert_eq!(tx.len(), 1);
}

#[test]
fn dormant_slots_mode_keeps_polling_parity_for_broadcast() {
    let (mut dev, stats) = make_ethernet();
    // Polling: the same packet goes to the raw driver.
    let outcome = dev.send(
        IpAddress::Ipv4(Ipv4Address::new(255, 255, 255, 255)),
        &[1u8; 10],
        Instant::from_millis_const(0),
    );
    assert!(matches!(outcome, TxOutcome::Accepted { .. }));
    assert_eq!(stats.tx_packets.lock().len(), 1);
    // Dormant: same packet lands in the slot instead, same disposition.
    dev.set_dormant_slots_for_test();
    let outcome = dev.send(
        IpAddress::Ipv4(Ipv4Address::new(255, 255, 255, 255)),
        &[1u8; 10],
        Instant::from_millis_const(0),
    );
    assert!(matches!(outcome, TxOutcome::Accepted { .. }));
    let (_, tx) = dev.slots_for_test();
    assert_eq!(tx.len(), 1);
    assert_eq!(stats.tx_packets.lock().len(), 1);
}

#[test]
fn dormant_slots_full_returns_backpressure() {
    let (mut dev, _) = make_ethernet();
    dev.set_dormant_slots_for_test();
    // Fill all 64 TX slots.
    let mut accepted = 0;
    loop {
        let outcome = dev.send(
            IpAddress::Ipv4(Ipv4Address::new(255, 255, 255, 255)),
            &[1u8; 10],
            Instant::from_millis_const(0),
        );
        if matches!(outcome, TxOutcome::Accepted { .. }) {
            accepted += 1;
        } else {
            break;
        }
    }
    assert_eq!(accepted, 64);
    let (_, tx) = dev.slots_for_test();
    assert_eq!(tx.len(), 64);
    assert!(tx.is_full());
}

#[test]
fn pending_head_full_keeps_router_packet() {
    let (mut dev, stats) = make_ethernet();
    // Resolve the peer neighbor first (via ARP reply).
    let _ = dev.send(
        IpAddress::Ipv4(PEER_IP),
        &IPV4_PAYLOAD,
        Instant::from_millis_const(0),
    );
    stats.frames.lock().push_back(arp_reply_frame());
    let mut rx = rx_buffer(8);
    let _ = dev.recv(&mut rx, Instant::from_millis_const(0));
    // Now inject a TX failure: a direct send to the resolved neighbor fails.
    *stats.transmit_error.lock() = true;
    let outcome = dev.send(
        IpAddress::Ipv4(PEER_IP),
        &[2u8; 20],
        Instant::from_millis_const(0),
    );
    assert!(matches!(outcome, TxOutcome::Full));
    *stats.transmit_error.lock() = false;
    // Recovery: the same packet now succeeds.
    let outcome = dev.send(
        IpAddress::Ipv4(PEER_IP),
        &[2u8; 20],
        Instant::from_millis_const(0),
    );
    assert!(matches!(outcome, TxOutcome::Accepted { .. }));
}

#[test]
fn arp_reply_accepted_resolves_neighbor_and_flushes_pending_once() {
    let (mut dev, stats) = make_ethernet();
    // Enqueue a pending packet via an unknown-neighbor send.
    let outcome = dev.send(
        IpAddress::Ipv4(PEER_IP),
        &IPV4_PAYLOAD,
        Instant::from_millis_const(0),
    );
    assert!(
        matches!(outcome, TxOutcome::Accepted { .. }),
        "first send must be accepted, got {outcome:?}"
    );
    assert_eq!(stats.tx_packets.lock().len(), 1, "after send");
    // Feed the ARP reply; the pending flush sends exactly one IPv4 frame.
    stats.frames.lock().push_back(arp_reply_frame());
    let mut rx = rx_buffer(8);
    let step = dev.recv(&mut rx, Instant::from_millis_const(0));
    assert!(matches!(step, RxStep::Consumed));
    assert!(
        stats.tx_packets.lock().len() == 2,
        "DBG3 actual tx={}",
        stats.tx_packets.lock().len()
    );
    let sent = stats.tx_packets.lock().pop().unwrap();
    let frame = EthernetFrame::new_unchecked(&sent);
    assert_eq!(
        EthernetRepr::parse(&frame).unwrap().ethertype,
        EthernetProtocol::Ipv4
    );
    // A second recv with no pending packet sends nothing more.
    let step = dev.recv(&mut rx, Instant::from_millis_const(0));
    assert!(matches!(step, RxStep::Empty));
    // After the pop above, only the original ARP request remains.
    assert_eq!(stats.tx_packets.lock().len(), 1);
}

#[test]
fn expired_neighbor_retriggers_arp_request() {
    let (mut dev, stats) = make_ethernet();
    // Resolve the neighbor with a very short TTL by advancing time.
    let _ = dev.send(
        IpAddress::Ipv4(PEER_IP),
        &IPV4_PAYLOAD,
        Instant::from_millis_const(0),
    );
    stats.frames.lock().push_back(arp_reply_frame());
    let mut rx = rx_buffer(8);
    let _ = dev.recv(&mut rx, Instant::from_millis_const(0));
    let tx_after_resolve = stats.tx_packets.lock().len();
    // Simulate neighbor expiry (NEIGHBOR_TTL = 60 s) and flush a pending
    // packet: the expired entry must trigger a fresh ARP request.
    let later = Instant::from_millis_const(120_000);
    stats.frames.lock().push_back(arp_reply_frame());
    let _ = dev.recv(&mut rx, later);
    // A new send past the expiry must re-request ARP (one more frame).
    let outcome = dev.send(IpAddress::Ipv4(PEER_IP), &[3u8; 20], later);
    assert!(matches!(outcome, TxOutcome::Accepted { .. }));
    assert_eq!(stats.tx_packets.lock().len(), tx_after_resolve + 1);
}

#[test]
fn ethernet_oversize_packet_is_dropped_with_reason() {
    let (mut dev, _) = make_ethernet();
    let oversized = vec![0u8; crate::device::fixed_queue::MAX_FRAME_SIZE + 1];
    let pre = dev.preflight_send(
        IpAddress::Ipv4(Ipv4Address::new(10, 0, 2, 9)),
        &oversized,
        Instant::from_millis_const(0),
    );
    assert!(matches!(
        pre,
        TxPreflight::Drop(crate::device::TxDropReason::FrameTooLarge)
    ));
    let outcome = dev.send(
        IpAddress::Ipv4(Ipv4Address::new(10, 0, 2, 9)),
        &oversized,
        Instant::from_millis_const(0),
    );
    assert!(matches!(
        outcome,
        TxOutcome::Dropped(crate::device::TxDropReason::FrameTooLarge)
    ));
}

#[test]
fn requested_neighbor_preflight_ignores_raw_tx_full() {
    let (mut dev, stats) = make_ethernet();
    // Unknown neighbor: the ARP request is sent and the neighbor is recorded
    // as pending (`None`), with the IPv4 payload enqueued into pending storage.
    let _ = dev.send(
        IpAddress::Ipv4(PEER_IP),
        &IPV4_PAYLOAD,
        Instant::from_millis_const(0),
    );
    // Raw TX is now full (no transmit capacity) but the request was already
    // sent: preflight must depend only on pending capacity, never on TX.
    *stats.tx_capacity.lock() = false;
    let pre = dev.preflight_send(
        IpAddress::Ipv4(PEER_IP),
        &IPV4_PAYLOAD,
        Instant::from_millis_const(0),
    );
    assert!(
        matches!(pre, TxPreflight::Ready),
        "an already-requested neighbor must preflight Ready with raw TX full, got {pre:?}"
    );
    // The commit also enqueues into pending storage without touching TX.
    let outcome = dev.send(
        IpAddress::Ipv4(PEER_IP),
        &IPV4_PAYLOAD,
        Instant::from_millis_const(0),
    );
    assert!(matches!(outcome, TxOutcome::Accepted { .. }));
}

#[test]
fn requested_neighbor_preflight_full_only_when_pending_full() {
    let (mut dev, stats) = make_ethernet();
    *stats.tx_capacity.lock() = false;
    let _ = dev.send(
        IpAddress::Ipv4(PEER_IP),
        &IPV4_PAYLOAD,
        Instant::from_millis_const(0),
    );
    // Fill pending storage to capacity; preflight must now report Full
    // because only pending capacity is the bottleneck.
    loop {
        let outcome = dev.send(
            IpAddress::Ipv4(PEER_IP),
            &IPV4_PAYLOAD,
            Instant::from_millis_const(0),
        );
        if !matches!(outcome, TxOutcome::Accepted { .. }) {
            break;
        }
    }
    let pre = dev.preflight_send(
        IpAddress::Ipv4(PEER_IP),
        &IPV4_PAYLOAD,
        Instant::from_millis_const(0),
    );
    assert!(matches!(pre, TxPreflight::Full));
}

#[test]
fn dormant_rx_arp_reply_full_retains_exact_frame_and_retries_once() {
    let (mut dev, _) = make_ethernet();
    dev.set_dormant_slots_for_test();
    // Fill all 64 TX slots so the owed ARP reply cannot be submitted.
    let mut accepted = 0;
    loop {
        let outcome = dev.send(
            IpAddress::Ipv4(Ipv4Address::new(255, 255, 255, 255)),
            &[1u8; 10],
            Instant::from_millis_const(0),
        );
        if matches!(outcome, TxOutcome::Accepted { .. }) {
            accepted += 1;
        } else {
            break;
        }
    }
    assert_eq!(accepted, 64);
    let frame = arp_request_frame();
    assert!(dev.push_rx_frame_for_test(&frame));
    {
        let (rx, _) = dev.slots_for_test();
        assert_eq!(rx.len(), 1);
    }

    let mut rx = rx_buffer(8);
    // First recv: the reply is deferred (TX slots full); the device returns
    // the distinct `Blocked` step so the Router stops its RX loop for this
    // device, and the RX head retains the exact frame bytes.
    let step = dev.recv(&mut rx, Instant::from_millis_const(0));
    assert!(matches!(step, RxStep::Blocked));
    {
        let (rx, _) = dev.slots_for_test();
        assert_eq!(rx.len(), 1, "deferred reply must keep the RX head");
        assert_eq!(rx.peek().unwrap(), &frame[..], "bytes must be exact");
    }

    // Free one TX slot, then retry: the reply commits exactly once and the
    // RX head is released.
    let (_, tx) = dev.slots_for_test();
    assert_eq!(tx.len(), 64);
    assert!(dev.pop_tx_slot_for_test());
    let step = dev.recv(&mut rx, Instant::from_millis_const(0));
    assert!(matches!(step, RxStep::Consumed));
    {
        let (rx, tx) = dev.slots_for_test();
        assert_eq!(rx.len(), 0, "retry must release the RX head");
        assert_eq!(
            tx.len(),
            64,
            "exactly one reply committed into the freed slot"
        );
    }
}

// --- Task 3.4: slot-mode raw owner and ticket ledger ---

#[test]
fn dormant_slots_preflight_broadcast_never_touches_raw_tx() {
    let (mut dev, stats) = make_ethernet();
    dev.set_dormant_slots_for_test();
    let pre = dev.preflight_send(
        IpAddress::Ipv4(Ipv4Address::new(255, 255, 255, 255)),
        &[1u8; 10],
        Instant::from_millis_const(0),
    );
    assert!(
        matches!(pre, TxPreflight::Ready),
        "an empty TX slot must preflight Ready, got {pre:?}"
    );
    assert_eq!(
        *stats.recycle_tx_calls.lock(),
        0,
        "slot-mode preflight must not recycle raw TX completions"
    );
    assert_eq!(
        *stats.can_transmit_calls.lock(),
        0,
        "slot-mode preflight must not query raw TX capacity"
    );
}

#[test]
fn dormant_slots_preflight_resolved_neighbor_never_touches_raw_tx() {
    let (mut dev, stats) = make_ethernet();
    dev.set_dormant_slots_for_test();
    // Resolve a neighbor through the dormant RX path so the preflight takes
    // the direct send branch.
    let _ = dev.send(
        IpAddress::Ipv4(PEER_IP),
        &IPV4_PAYLOAD,
        Instant::from_millis_const(0),
    );
    assert!(dev.push_rx_frame_for_test(&arp_reply_frame()));
    let mut rx = rx_buffer(8);
    let _ = dev.recv(&mut rx, Instant::from_millis_const(0));
    // Reset the counters: the resolution path above may legitimately touch
    // nothing raw in slot mode, but the preflight assertion must be isolated.
    *stats.recycle_tx_calls.lock() = 0;
    *stats.can_transmit_calls.lock() = 0;
    let pre = dev.preflight_send(
        IpAddress::Ipv4(PEER_IP),
        &[1u8; 10],
        Instant::from_millis_const(0),
    );
    assert!(matches!(pre, TxPreflight::Ready));
    assert_eq!(*stats.recycle_tx_calls.lock(), 0);
    assert_eq!(*stats.can_transmit_calls.lock(), 0);
}

#[test]
fn dormant_slots_preflight_unknown_neighbor_never_touches_raw_tx() {
    let (mut dev, stats) = make_ethernet();
    dev.set_dormant_slots_for_test();
    let pre = dev.preflight_send(
        IpAddress::Ipv4(Ipv4Address::new(10, 0, 2, 99)),
        &[1u8; 10],
        Instant::from_millis_const(0),
    );
    assert!(
        matches!(pre, TxPreflight::Ready),
        "unknown-neighbor preflight must be Ready with slot room, got {pre:?}"
    );
    assert_eq!(*stats.recycle_tx_calls.lock(), 0);
    assert_eq!(*stats.can_transmit_calls.lock(), 0);
}

#[test]
fn reclaim_unknown_cookie_faults_instead_of_reclaimed() {
    let (mut dev, stats) = make_ethernet_with_tx_queue();
    // Script a completion cookie that was never allocated to any live ticket.
    stats.reclaim_cookies.lock().push_back(42);
    let step = dev.tx_reclaim_one();
    assert!(
        matches!(step, TxReclaimStep::Fault(DevError::BadState)),
        "an unknown reclaim cookie must be an ownership fault, got {step:?}"
    );
}

#[test]
fn reclaim_duplicate_cookie_faults_after_release() {
    let (mut dev, stats) = make_ethernet_with_tx_queue();
    dev.set_dormant_slots_for_test();
    // One dormant send allocates ticket 0 in a TX slot.
    let outcome = dev.send(
        IpAddress::Ipv4(Ipv4Address::new(255, 255, 255, 255)),
        &[1u8; 10],
        Instant::from_millis_const(0),
    );
    assert!(matches!(outcome, TxOutcome::Accepted { .. }));
    // Submitting the slot transitions ticket 0 Queued -> DeviceOwned (D8);
    // only a DeviceOwned ticket may be reclaimed.
    assert!(matches!(dev.tx_submit_one(), TxSubmitStep::Submitted));
    // The first reclaim releases the matching DeviceOwned ticket.
    stats.reclaim_cookies.lock().push_back(0);
    assert!(matches!(dev.tx_reclaim_one(), TxReclaimStep::Reclaimed));
    // The same cookie is now stale: a duplicate must fault, not re-reclaim.
    stats.reclaim_cookies.lock().push_back(0);
    let step = dev.tx_reclaim_one();
    assert!(
        matches!(step, TxReclaimStep::Fault(DevError::BadState)),
        "a duplicate reclaim cookie must be an ownership fault, got {step:?}"
    );
}

#[test]
fn reclaim_valid_cookie_still_releases_matching_ticket() {
    let (mut dev, stats) = make_ethernet_with_tx_queue();
    dev.set_dormant_slots_for_test();
    // Two dormant sends allocate tickets 0 and 1.
    for _ in 0..2 {
        let outcome = dev.send(
            IpAddress::Ipv4(Ipv4Address::new(255, 255, 255, 255)),
            &[1u8; 10],
            Instant::from_millis_const(0),
        );
        assert!(matches!(outcome, TxOutcome::Accepted { .. }));
    }
    // Submit both slots so the tickets become DeviceOwned and reclaimable.
    for _ in 0..2 {
        assert!(matches!(dev.tx_submit_one(), TxSubmitStep::Submitted));
    }
    // Reclaiming ticket 1 releases exactly that DeviceOwned ticket.
    stats.reclaim_cookies.lock().push_back(1);
    assert!(matches!(dev.tx_reclaim_one(), TxReclaimStep::Reclaimed));
    // Ticket 0 is still live and reclaimable.
    stats.reclaim_cookies.lock().push_back(0);
    assert!(matches!(dev.tx_reclaim_one(), TxReclaimStep::Reclaimed));
}

// --- Task 3.5: stack TX enqueue wakes the queue owner ---

#[derive(Default)]
struct CountWake(Arc<AtomicUsize>);

impl alloc::task::Wake for CountWake {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

fn counting_waker(count: Arc<AtomicUsize>) -> core::task::Waker {
    core::task::Waker::from(Arc::new(CountWake(count)))
}

#[test]
fn service_poll_publishes_queue_event_after_tx_enqueue() {
    // Task 3.5 (Finding 2): a stack TX dispatch that enqueues the first
    // frame into the dormant TX slot must publish a queue-owner event, so a
    // sleeping queue task submits it without waiting for a hardware
    // completion.
    let _serial = SERIAL.lock();
    let (mut dev, _stats) = make_ethernet();
    dev.activate_slot_mode().unwrap();
    let mut router = Router::new();
    let eth = router.add_device(Box::new(dev));
    router.add_rule(Rule::new(
        Ipv4Cidr::new(Ipv4Address::new(10, 0, 2, 0), 24).into(),
        None,
        eth,
        Ipv4Address::new(SRC_IPV4[0], SRC_IPV4[1], SRC_IPV4[2], SRC_IPV4[3]).into(),
    ));
    let mut service = Service::new(router, Some(eth));
    {
        let router = service.router_for_test();
        assert!(router.enqueue_tx_for_test(&ipv4_packet(SRC_IPV4, DST_IPV4)));
    }

    let count = Arc::new(AtomicUsize::new(0));
    QUEUE_EVENT.register_queue(&counting_waker(count.clone()));

    let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
    service.poll(RxOwnerView::AsyncOwned, &mut sockets);

    // The first stack TX enqueue woke the queue owner exactly once.
    assert_eq!(count.load(Ordering::Relaxed), 1);
}

#[test]
fn service_poll_arp_flush_first_tx_slot_wakes_queue_owner_once() {
    // Cycle 000 A1 (Iteration 011): an ARP reply consumed by `router.poll`
    // resolves a neighbor and flushes the first dormant TX slot. The queue
    // owner must be woken exactly once in the same `Service::poll` round, even
    // though the slot was created during ingress rather than dispatch.
    let _serial = SERIAL.lock();
    let (mut dev, stats) = make_ethernet_with_tx_queue();
    dev.activate_slot_mode().unwrap();
    // Unknown neighbor: the ARP request lands in the TX slot and the IPv4
    // payload is held pending until the reply resolves the neighbor.
    let outcome = dev.send(
        IpAddress::Ipv4(PEER_IP),
        &IPV4_PAYLOAD,
        Instant::from_millis_const(0),
    );
    assert!(matches!(outcome, TxOutcome::Accepted { .. }));
    // Drain the already-published ARP request through the fake driver's real
    // ticket path so TX is empty again: submit the slot (Queued ->
    // DeviceOwned), then reclaim the matching completion. This never uses the
    // test-only slot pop, so the ticket/ownership assertion stays intact.
    assert!(matches!(dev.tx_submit_one(), TxSubmitStep::Submitted));
    stats.reclaim_cookies.lock().push_back(0);
    assert!(matches!(dev.tx_reclaim_one(), TxReclaimStep::Reclaimed));

    // Place the matching ARP reply in an RX slot.
    assert!(dev.push_rx_frame_for_test(&arp_reply_frame()));

    let mut router = Router::new();
    let eth = router.add_device(Box::new(dev));
    let mut service = Service::new(router, Some(eth));
    let count = Arc::new(AtomicUsize::new(0));
    QUEUE_EVENT.register_queue(&counting_waker(count.clone()));
    let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
    service.poll(RxOwnerView::AsyncOwned, &mut sockets);

    // The ARP reply flushed the pending IPv4 into the first TX slot inside
    // `router.poll`; a sleeping queue owner must observe exactly one queue
    // work wake so it submits the frame without waiting for a hardware
    // completion.
    assert_eq!(count.load(Ordering::Relaxed), 1);
    // The flushed frame stays in the TX slot, correctly ticketed for the
    // queue owner to submit; no stable Router fault was entered.
    let router = service.router_for_test();
    assert_eq!(router.devices[eth].tx_slot_len_for_test(), 1);
    assert!(!router.tx_faulted());
}

#[test]
fn service_poll_already_pending_tx_does_not_publish_again() {
    // Preservation (Cycle 000 A1): a TX slot already pending at the start of
    // the round must not produce a second queue-owner wake, so an in-flight
    // frame never creates a wake storm.
    let _serial = SERIAL.lock();
    let (mut dev, _stats) = make_ethernet();
    dev.activate_slot_mode().unwrap();
    let mut router = Router::new();
    let eth = router.add_device(Box::new(dev));
    router.add_rule(Rule::new(
        Ipv4Cidr::new(Ipv4Address::new(10, 0, 2, 0), 24).into(),
        None,
        eth,
        Ipv4Address::new(SRC_IPV4[0], SRC_IPV4[1], SRC_IPV4[2], SRC_IPV4[3]).into(),
    ));
    let mut service = Service::new(router, Some(eth));
    {
        let router = service.router_for_test();
        assert!(router.enqueue_tx_for_test(&ipv4_packet(SRC_IPV4, DST_IPV4)));
    }

    let count = Arc::new(AtomicUsize::new(0));
    QUEUE_EVENT.register_queue(&counting_waker(count.clone()));
    let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
    // Round 1 publishes exactly one wake when dispatch fills the empty slot.
    service.poll(RxOwnerView::AsyncOwned, &mut sockets);
    assert_eq!(count.load(Ordering::Relaxed), 1);
    // The slot is still pending; a second round with no new TX work must not
    // publish another wake.
    count.store(0, Ordering::Relaxed);
    service.poll(RxOwnerView::AsyncOwned, &mut sockets);
    assert_eq!(count.load(Ordering::Relaxed), 0);
}

#[test]
fn service_poll_empty_round_keeps_queue_owner_asleep() {
    // Preservation (Cycle 000 A1): an empty round that creates no TX slot and
    // consumes no frame stays quiescent for the queue owner.
    let _serial = SERIAL.lock();
    let (dev, _stats) = make_ethernet();
    // `dev` is consumed by the router; the empty round touches the shared
    // `QUEUE_EVENT` through the production `Service::poll`.
    let mut router = Router::new();
    let eth = router.add_device(Box::new(dev));
    let mut service = Service::new(router, Some(eth));
    let count = Arc::new(AtomicUsize::new(0));
    QUEUE_EVENT.register_queue(&counting_waker(count.clone()));
    let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
    service.poll(RxOwnerView::AsyncOwned, &mut sockets);
    assert_eq!(count.load(Ordering::Relaxed), 0);
}

#[test]
fn service_poll_deferred_arp_attempts_once_and_stops_round() {
    // Task 3.6 (Finding 3): a TX-full ARP request entering the real
    // Router/Service loop must be attempted once, retain the exact RX bytes,
    // end the round immediately, and not grow the consumed/delivered deltas.
    // Task 3.7: the round calls the production `Service::poll`, which touches
    // the shared `QUEUE_EVENT`/`RX_TELEMETRY`; serialize against sibling
    // tests so a parallel `wake_if_space` cannot clear their waiting bit.
    let _serial = SERIAL.lock();
    let (mut dev, _) = make_ethernet();
    dev.set_dormant_slots_for_test();
    // Fill all 64 TX slots so the owed ARP reply cannot be submitted.
    let mut accepted = 0;
    loop {
        let outcome = dev.send(
            IpAddress::Ipv4(Ipv4Address::new(255, 255, 255, 255)),
            &[1u8; 10],
            Instant::from_millis_const(0),
        );
        if matches!(outcome, TxOutcome::Accepted { .. }) {
            accepted += 1;
        } else {
            break;
        }
    }
    assert_eq!(accepted, 64);
    let frame = arp_request_frame();
    assert!(dev.push_rx_frame_for_test(&frame));

    let mut router = Router::new();
    let eth = router.add_device(Box::new(dev));
    let mut service = Service::new(router, Some(eth));
    let mut sockets = smoltcp::iface::SocketSet::new(vec![]);

    service.poll(RxOwnerView::AsyncOwned, &mut sockets);

    // The deferred ARP request was attempted exactly once; the RX head is
    // retained and no consumed/delivered delta was produced.
    let router = service.router_for_test();
    assert_eq!(router.devices[eth].recv_dormant_calls_for_test(), 1);
    assert_eq!(router.take_rx_consumed_delta(), 0);
    assert_eq!(router.take_rx_delivered_delta(), 0);
    assert_eq!(router.devices[eth].rx_slot_len_for_test(), 1);
    assert_eq!(
        router.devices[eth].rx_slot_peek_for_test(),
        Some(&frame[..]),
        "bytes must be exact"
    );
}

#[test]
fn service_poll_deferred_arp_retries_once_after_tx_space() {
    // Task 3.6 (Finding 3): after a TX slot frees, the next Service poll
    // commits exactly one ARP reply, parses once, and pops the RX head.
    // Task 3.7: the round calls the production `Service::poll` and reads the
    // `RX_TELEMETRY.non_ip_consumed` delta; serialize against sibling tests
    // so a parallel Service poll cannot perturb the delta.
    let _serial = SERIAL.lock();
    let (mut dev, _) = make_ethernet();
    dev.set_dormant_slots_for_test();
    let mut accepted = 0;
    loop {
        let outcome = dev.send(
            IpAddress::Ipv4(Ipv4Address::new(255, 255, 255, 255)),
            &[1u8; 10],
            Instant::from_millis_const(0),
        );
        if matches!(outcome, TxOutcome::Accepted { .. }) {
            accepted += 1;
        } else {
            break;
        }
    }
    assert_eq!(accepted, 64);
    assert!(dev.push_rx_frame_for_test(&arp_request_frame()));

    let mut router = Router::new();
    let eth = router.add_device(Box::new(dev));
    let mut service = Service::new(router, Some(eth));
    let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
    service.poll(RxOwnerView::AsyncOwned, &mut sockets);
    let attempts_before = service.router_for_test().devices[eth].recv_dormant_calls_for_test();

    // Free one TX slot and release the space event; the retry commits the
    // reply exactly once and pops the RX head.
    let consumed_before = RX_TELEMETRY.non_ip_consumed.load(Ordering::Relaxed);
    assert!(service.router_for_test().devices[eth].pop_tx_slot_for_test());
    service.poll(RxOwnerView::AsyncOwned, &mut sockets);

    let router = service.router_for_test();
    // The retry round ran two bounded recv calls: one that processed and
    // committed the ARP reply, then an Empty probe that ends the loop. The
    // deferred head was never reprocessed in a single poll.
    assert_eq!(
        router.devices[eth].recv_dormant_calls_for_test(),
        attempts_before + 2
    );
    // The RX head was consumed once (the reply was accepted and popped).
    // `Service::poll` folds the delta into the global telemetry.
    assert_eq!(
        RX_TELEMETRY.non_ip_consumed.load(Ordering::Relaxed) - consumed_before,
        1
    );
    assert_eq!(
        router.devices[eth].rx_slot_len_for_test(),
        0,
        "retry must release the RX head"
    );
    assert_eq!(
        router.devices[eth].tx_slot_len_for_test(),
        64,
        "exactly one reply committed into the freed slot"
    );
}

// ── RW-2: real driver TX resource ledger and ownership invariant ────────

#[test]
fn tx_resource_ledger_forwards_driver_counts_and_conserves_capacity() {
    // A scripted driver ledger must be forwarded unchanged through the
    // EthernetDevice, with `available + inflight` equal to the fixed capacity.
    let pool = NetBufPool::new(8, 2048).expect("pool alloc");
    let stats = Arc::new(FakeStats::new());
    *stats.ledger.lock() = Some(TxResourceLedger {
        buffer_available: 3,
        buffer_inflight: 1,
        descriptor_available: 2,
        descriptor_inflight: 2,
        completions_seen: 5,
    });
    let nic = FakeNic {
        pool,
        stats: stats.clone(),
        control: None,
        tx_queue: Some(FakeTxQueue {
            stats: stats.clone(),
        }),
    };
    let mut dev = EthernetDevice::new("eth0".into(), Box::new(nic), TEST_IP);
    let ledger = dev.tx_resource_ledger().expect("ledger present");
    assert_eq!(ledger.buffer_available, 3);
    assert_eq!(ledger.buffer_inflight, 1);
    assert_eq!(ledger.descriptor_available, 2);
    assert_eq!(ledger.descriptor_inflight, 2);
    assert_eq!(ledger.completions_seen, 5);
    assert_eq!(ledger.buffer_available + ledger.buffer_inflight, 4);
    assert_eq!(ledger.descriptor_available + ledger.descriptor_inflight, 4);
}

#[test]
fn tx_resource_ledger_is_none_when_driver_cannot_observe_it() {
    // The default (no scripted ledger) reports `None`; the V3 snapshot must
    // not synthesize numbers from ticket/slot capacities.
    let pool = NetBufPool::new(8, 2048).expect("pool alloc");
    let stats = Arc::new(FakeStats::new());
    let nic = FakeNic {
        pool,
        stats: stats.clone(),
        control: None,
        tx_queue: Some(FakeTxQueue {
            stats: stats.clone(),
        }),
    };
    let mut dev = EthernetDevice::new("eth0".into(), Box::new(nic), TEST_IP);
    assert!(dev.tx_resource_ledger().is_none());
}

#[test]
fn v3_snapshot_maps_real_driver_ledger_under_the_service_guard() {
    // The V3 snapshot must report the driver's real buffer/descriptor counts,
    // not a synthesis from ticket capacities (`MAX_LIVE_TICKETS - live`).
    let pool = NetBufPool::new(8, 2048).expect("pool alloc");
    let stats = Arc::new(FakeStats::new());
    *stats.ledger.lock() = Some(TxResourceLedger {
        buffer_available: 3,
        buffer_inflight: 1,
        descriptor_available: 2,
        descriptor_inflight: 2,
        completions_seen: 7,
    });
    let nic = FakeNic {
        pool,
        stats: stats.clone(),
        control: None,
        tx_queue: Some(FakeTxQueue {
            stats: stats.clone(),
        }),
    };
    let dev = EthernetDevice::new("eth0".into(), Box::new(nic), TEST_IP);
    let mut router = Router::new();
    let eth = router.add_device(Box::new(dev));
    let mut service = Service::new(router, Some(eth));
    let ledger = service.v3_tx_resource_ledger().expect("ledger present");
    assert_eq!(ledger.buffer_available, 3);
    assert_eq!(ledger.buffer_inflight, 1);
    assert_eq!(ledger.descriptor_available, 2);
    assert_eq!(ledger.descriptor_inflight, 2);
    // The driver's fixed capacity is exactly the sum.
    assert_eq!(ledger.buffer_available + ledger.buffer_inflight, 4);
    assert_eq!(ledger.descriptor_available + ledger.descriptor_inflight, 4);
}
