//! MS13 Iteration 002, task T6 (3.1): the old/new socket combination
//! contract over the production K3 adapter + Service + real `SocketOps`.
//!
//! Host mirror of the board recovery matrix shape: the same stack_socket.rs
//! device-half/ISR-half driving, plus the production recovery entries —
//! `axnet::recovery_reset_request()` (the controlled reset the resident owner
//! serves) and the real link-policy step (a scripted BMSR transition taken
//! through a published CONFIG cause). Witnessed here, all through the public
//! `SocketOps` + `poll_io` surface:
//!
//! - `old_socket_terminal_precedes_wake_new_socket_is_clean`: old TCP/UDP/
//!   listener waiters observe their stable epoch terminal (a wake implies the
//!   terminal was committed first — recv is terminal-first), a new-epoch
//!   socket completes real full-byte I/O, and old queued data never reaches
//!   new objects.
//! - `old_flush_is_aborted_not_c4`: a Pending flush anchored in the old
//!   data-plane epoch completes with an error, never a forged C4 success.
//!
//! The timer wake source is structurally unreachable on the host (the axhal
//! stub keeps `wall_time()` at 0), so every wait here is event/clock-step
//! driven by the test thread — exactly like stack_socket.rs; real timer/DMA
//! conclusions stay with the T7 board matrix.

#![allow(dead_code, unused_imports)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

use axdriver::AxDeviceContainer;
use axdriver_net::NetDriverOps;
use axnet::{
    RecvOptions, SendOptions, Shutdown, Socket, SocketAddrEx, SocketOps, flush,
    init_network, publish_queue_event, rx_snapshot, rx_snapshot_v3,
    stack_snapshot, start_rx_task_affinity,
};
use axnet::tcp::TcpSocket;
use axnet::udp::UdpSocket;
use axpoll::{IoEvents, PollSet, Pollable};
use axtask::future::{block_on, poll_io};
use axtask::{init_scheduler, schedulable_cpu_mask, yield_now};
use memory_addr::{PhysAddr, VirtAddr};

use k3_async_net_host::fake::Board;
use k3_async_net_host::k3_gmac::adapter::K3NetAdapter;
use k3_async_net_host::payload;

// Kernel ABI symbol required by `axklib` (pulled in via `axdriver/dyn`).
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __axklib_0_3_mem_iomap(
    _addr: PhysAddr,
    _size: usize,
) -> axerrno::AxResult<VirtAddr> {
    Err(axerrno::AxError::Unsupported)
}

const BOARD_MAC: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x26, 0x01];
const PEER_MAC: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x26, 0x02];
const PEER_UDP_PORT: u16 = 40000;
/// The recovery-matrix ports (distinct from the MS12 payload ports).
const UDP_PORT: u16 = 26011;
const TCP_PORT: u16 = 26012;
const UDP_WAITER_PORT: u16 = 26013;

fn board_ip() -> [u8; 4] {
    match option_env!("AX_IP") {
        Some("192.168.137.2") => [192, 168, 137, 2],
        _ => [10, 0, 2, 15],
    }
}

fn peer_ip() -> [u8; 4] {
    match option_env!("AX_GW") {
        Some("192.168.137.1") => [192, 168, 137, 1],
        _ => [10, 0, 2, 2],
    }
}

static SCHED: Once = Once::new();

fn init_scheduler_once() {
    SCHED.call_once(init_scheduler);
}

// ---------------------------------------------------------------------------
// Wire frames (IPv4/UDP/TCP/ARP with real checksums)
// ---------------------------------------------------------------------------

fn checksum16(sum: u32) -> u16 {
    let mut sum = (sum & 0xffff) + (sum >> 16);
    sum = (sum & 0xffff) + (sum >> 16);
    !(sum as u16)
}

fn ipv4_checksum(header: &[u8]) -> u16 {
    let mut sum = 0u32;
    for pair in header.chunks_exact(2) {
        sum += u16::from_be_bytes([pair[0], pair[1]]) as u32;
    }
    checksum16(sum)
}

fn udp_checksum(src_ip: [u8; 4], dst_ip: [u8; 4], udp_header_and_payload: &[u8]) -> u16 {
    let mut sum = 0u32;
    for pair in src_ip.chunks_exact(2) {
        sum += u16::from_be_bytes([pair[0], pair[1]]) as u32;
    }
    for pair in dst_ip.chunks_exact(2) {
        sum += u16::from_be_bytes([pair[0], pair[1]]) as u32;
    }
    sum += 17u32;
    sum += udp_header_and_payload.len() as u32;
    let mut chunks = udp_header_and_payload.chunks_exact(2);
    for pair in &mut chunks {
        sum += u16::from_be_bytes([pair[0], pair[1]]) as u32;
    }
    if let [last] = chunks.remainder() {
        sum += ((*last as u16) << 8) as u32;
    }
    checksum16(sum)
}

/// One complete peer→board IPv4/UDP frame (Ethernet + IPv4 + UDP).
fn udp_frame(sport: u16, dport: u16, payload_bytes: &[u8]) -> Vec<u8> {
    let udp_len = 8 + payload_bytes.len();
    let mut udp = vec![0u8; udp_len];
    udp[0..2].copy_from_slice(&sport.to_be_bytes());
    udp[2..4].copy_from_slice(&dport.to_be_bytes());
    udp[4..6].copy_from_slice(&(udp_len as u16).to_be_bytes());
    udp[6..8].copy_from_slice(&[0, 0]);
    udp[8..].copy_from_slice(payload_bytes);
    let ck = udp_checksum(peer_ip(), board_ip(), &udp);
    udp[6..8].copy_from_slice(&ck.to_be_bytes());

    let ip_total = 20 + udp_len;
    let mut ip = vec![0u8; 20];
    ip[0] = 0x45;
    ip[1] = 0;
    ip[2..4].copy_from_slice(&(ip_total as u16).to_be_bytes());
    ip[4..6].copy_from_slice(&[0, 1]);
    ip[6..7].copy_from_slice(&[0x40]);
    ip[8] = 64;
    ip[9] = 17;
    ip[12..16].copy_from_slice(&peer_ip());
    ip[16..20].copy_from_slice(&board_ip());
    let ck = ipv4_checksum(&ip);
    ip[10..12].copy_from_slice(&ck.to_be_bytes());

    let mut frame = vec![0u8; 14 + ip_total];
    frame[0..6].copy_from_slice(&BOARD_MAC);
    frame[6..12].copy_from_slice(&PEER_MAC);
    frame[12] = 0x08;
    frame[13] = 0x00;
    frame[14..34].copy_from_slice(&ip);
    frame[34..].copy_from_slice(&udp);
    frame
}

// ── TCP (the hand-crafted PC peer) ─────────────────────────────────────────

const FLAG_FIN: u8 = 0x01;
const FLAG_SYN: u8 = 0x02;
const FLAG_RST: u8 = 0x04;
const FLAG_PSH: u8 = 0x08;
const FLAG_ACK: u8 = 0x10;

fn tcp_checksum(src_ip: [u8; 4], dst_ip: [u8; 4], segment: &[u8]) -> u16 {
    let mut sum = 0u32;
    for pair in src_ip.chunks_exact(2) {
        sum += u16::from_be_bytes([pair[0], pair[1]]) as u32;
    }
    for pair in dst_ip.chunks_exact(2) {
        sum += u16::from_be_bytes([pair[0], pair[1]]) as u32;
    }
    sum += 6u32;
    sum += segment.len() as u32;
    let mut chunks = segment.chunks_exact(2);
    for pair in &mut chunks {
        sum += u16::from_be_bytes([pair[0], pair[1]]) as u32;
    }
    if let [last] = chunks.remainder() {
        sum += ((*last as u16) << 8) as u32;
    }
    checksum16(sum)
}

/// One complete peer→board IPv4/TCP frame to `dport`.
fn tcp_frame(dport: u16, seq: u32, ack: u32, flags: u8, payload_bytes: &[u8]) -> Vec<u8> {
    let mut tcp = vec![0u8; 20 + payload_bytes.len()];
    tcp[0..2].copy_from_slice(&PEER_UDP_PORT.to_be_bytes());
    tcp[2..4].copy_from_slice(&dport.to_be_bytes());
    tcp[4..8].copy_from_slice(&seq.to_be_bytes());
    tcp[8..12].copy_from_slice(&ack.to_be_bytes());
    tcp[12] = 5 << 4; // data offset 20 bytes
    tcp[13] = flags;
    tcp[14..16].copy_from_slice(&0xffffu16.to_be_bytes());
    tcp[20..].copy_from_slice(payload_bytes);
    let ck = tcp_checksum(peer_ip(), board_ip(), &tcp);
    tcp[16..18].copy_from_slice(&ck.to_be_bytes());

    let ip_total = 20 + tcp.len();
    let mut ip = vec![0u8; 20];
    ip[0] = 0x45;
    ip[1] = 0;
    ip[2..4].copy_from_slice(&(ip_total as u16).to_be_bytes());
    ip[4..6].copy_from_slice(&[0, 2]);
    ip[6..7].copy_from_slice(&[0x40]);
    ip[8] = 64;
    ip[9] = 6;
    ip[12..16].copy_from_slice(&peer_ip());
    ip[16..20].copy_from_slice(&board_ip());
    let ck = ipv4_checksum(&ip);
    ip[10..12].copy_from_slice(&ck.to_be_bytes());

    let mut frame = vec![0u8; 14 + ip_total];
    frame[0..6].copy_from_slice(&BOARD_MAC);
    frame[6..12].copy_from_slice(&PEER_MAC);
    frame[12] = 0x08;
    frame[13] = 0x00;
    frame[14..34].copy_from_slice(&ip);
    frame[34..].copy_from_slice(&tcp);
    frame
}

/// A parsed board→peer TCP segment (payload from the IPv4 total length).
#[derive(Debug, Clone)]
struct TcpSeg {
    seq: u32,
    ack: u32,
    flags: u8,
    payload: Vec<u8>,
}

fn parse_tcp(frame: &[u8]) -> Option<TcpSeg> {
    if frame.len() < 34 || frame[12] != 0x08 || frame[13] != 0x00 {
        return None;
    }
    if frame[23] != 6 {
        return None;
    }
    let ihl = (frame[14] & 0x0f) as usize * 4;
    let ip_total = u16::from_be_bytes([frame[16], frame[17]]) as usize;
    let tcp_start = 14 + ihl;
    if frame.len() < tcp_start + 20 || ip_total < ihl + 20 {
        return None;
    }
    let tcp = &frame[tcp_start..14 + ip_total];
    let data_off = (tcp[12] >> 4) as usize * 4;
    Some(TcpSeg {
        seq: u32::from_be_bytes([tcp[4], tcp[5], tcp[6], tcp[7]]),
        ack: u32::from_be_bytes([tcp[8], tcp[9], tcp[10], tcp[11]]),
        flags: tcp[13],
        payload: tcp[data_off.min(tcp.len())..].to_vec(),
    })
}

/// Unsolicited ARP announcement: peer_ip is at PEER_MAC.
fn arp_announcement() -> Vec<u8> {
    let mut arp = vec![0u8; 28];
    arp[0..2].copy_from_slice(&1u16.to_be_bytes());
    arp[2..4].copy_from_slice(&0x0800u16.to_be_bytes());
    arp[4] = 6;
    arp[5] = 4;
    arp[6..8].copy_from_slice(&2u16.to_be_bytes()); // reply
    arp[8..14].copy_from_slice(&PEER_MAC);
    arp[14..18].copy_from_slice(&peer_ip());
    arp[18..24].copy_from_slice(&BOARD_MAC);
    arp[24..28].copy_from_slice(&board_ip());
    let mut frame = vec![0u8; 14 + 28];
    frame[0..6].copy_from_slice(&BOARD_MAC);
    frame[6..12].copy_from_slice(&PEER_MAC);
    frame[12] = 0x08;
    frame[13] = 0x06;
    frame[14..].copy_from_slice(&arp);
    frame
}

fn is_arp_request(frame: &[u8]) -> bool {
    frame.len() >= 14 + 28
        && frame[12] == 0x08
        && frame[13] == 0x06
        && frame[14 + 6..14 + 8] == [0, 1]
}

/// The board→peer UDP echo payload from `sport`, if this frame is one.
fn udp_payload_from(frame: &[u8], sport: u16) -> Option<Vec<u8>> {
    if frame.len() < 34 || frame[12] != 0x08 || frame[13] != 0x00 || frame[23] != 17 {
        return None;
    }
    let ihl = (frame[14] & 0x0f) as usize * 4;
    let ip_total = u16::from_be_bytes([frame[16], frame[17]]) as usize;
    let udp_start = 14 + ihl;
    if frame.len() < udp_start + 8 || ip_total < ihl + 8 {
        return None;
    }
    let udp = &frame[udp_start..14 + ip_total];
    let found_sport = u16::from_be_bytes([udp[0], udp[1]]);
    let found_dport = u16::from_be_bytes([udp[2], udp[3]]);
    if found_sport != sport || found_dport != PEER_UDP_PORT {
        return None;
    }
    Some(udp[8..].to_vec())
}

// ---------------------------------------------------------------------------
// The hand-crafted PC peer state machine
// ---------------------------------------------------------------------------

struct TcpPeer {
    isn: u32,
    next: u32,
    board_isn: u32,
    /// Next board seq we expect (reassembly watermark).
    board_next: u32,
    data: Vec<u8>,
    fin: Option<u32>,
    saw_rst: bool,
    dport: u16,
}

impl TcpPeer {
    fn new(dport: u16) -> Self {
        let isn = 0x51d0_0101;
        TcpPeer {
            isn,
            next: isn + 1,
            board_isn: 0,
            board_next: 0,
            data: Vec::new(),
            fin: None,
            saw_rst: false,
            dport,
        }
    }

    fn syn(&self) -> Vec<u8> {
        tcp_frame(self.dport, self.isn, 0, FLAG_SYN, &[])
    }

    /// Records the board's SYN-ACK (ISN + the echoed client ISN).
    fn observe_synack(&mut self, seg: &TcpSeg) {
        assert!(seg.flags & FLAG_SYN != 0 && seg.flags & FLAG_ACK != 0);
        assert_eq!(seg.ack, self.isn + 1, "SYN-ACK must ack our ISN+1");
        self.board_isn = seg.seq;
        self.board_next = seg.seq.wrapping_add(1);
    }

    fn ack_handshake(&self) -> Vec<u8> {
        tcp_frame(self.dport, self.next, self.board_next, FLAG_ACK, &[])
    }

    fn send_data(&mut self, bytes: &[u8]) -> Vec<u8> {
        let frame = tcp_frame(self.dport, self.next, self.board_next, FLAG_PSH | FLAG_ACK, bytes);
        self.next = self.next.wrapping_add(bytes.len() as u32);
        frame
    }

    /// Accepts one board segment; returns its payload when in-order data.
    fn observe_board(&mut self, seg: &TcpSeg) -> Option<Vec<u8>> {
        if seg.flags & FLAG_RST != 0 {
            self.saw_rst = true;
            return None;
        }
        let mut out = None;
        if !seg.payload.is_empty() && seg.seq == self.board_next {
            self.data.extend_from_slice(&seg.payload);
            self.board_next = self.board_next.wrapping_add(seg.payload.len() as u32);
            out = Some(seg.payload.clone());
        }
        if seg.flags & FLAG_FIN != 0 && self.fin.is_none() {
            self.fin = Some(seg.seq);
            self.board_next = self.board_next.wrapping_add(1);
        }
        out
    }

    fn ack_board_data(&self) -> Vec<u8> {
        tcp_frame(self.dport, self.next, self.board_next, FLAG_ACK, &[])
    }

    /// ACKs the board's FIN and closes our side (FIN).
    fn ack_fin_and_close(&mut self) -> Vec<u8> {
        let frame = tcp_frame(self.dport, self.next, self.board_next, FLAG_FIN | FLAG_ACK, &[]);
        self.next = self.next.wrapping_add(1);
        frame
    }
}

// ---------------------------------------------------------------------------
// Test harness: pump the host scheduler, drive the device half
// ---------------------------------------------------------------------------

struct StackOwner {
    board: Board,
}

impl StackOwner {
    fn pump(&mut self, rounds: usize) {
        for _ in 0..rounds {
            yield_now();
        }
    }

    fn pump_until(&mut self, deadline: Instant, mut cond: impl FnMut() -> bool, what: &str) {
        loop {
            if cond() {
                return;
            }
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            yield_now();
        }
    }

    /// Drains board TX; answers ARP requests inline (the neighbor warm-up)
    /// and returns the data frames. Every drain is followed by an IRQ-style
    /// kick so the queue owner reclaims the completed descriptors.
    fn exchange(&mut self) -> Vec<Vec<u8>> {
        let mut data = Vec::new();
        for frame in self.board.wire.drain_board_tx() {
            if is_arp_request(&frame) {
                self.board.wire.inject_rx_frame(&arp_announcement());
                publish_queue_event();
                self.pump(16);
            } else {
                data.push(frame);
            }
        }
        publish_queue_event();
        self.pump(16);
        data
    }

    /// Exchanges until `f` finds its target in the drained data frames.
    fn drain_until<T>(
        &mut self,
        deadline: Instant,
        mut f: impl FnMut(&[Vec<u8>]) -> Option<T>,
    ) -> T {
        loop {
            let frames = self.exchange();
            if let Some(found) = f(&frames) {
                return found;
            }
            assert!(Instant::now() < deadline, "timed out draining the wire");
            self.pump(8);
        }
    }
}

/// Spawns the payload session as a scheduler task; the test thread stays
/// free as the device half. `None` until the session completes.
fn spawn_session<F, T>(fut: F) -> Arc<Mutex<Option<T>>>
where
    F: core::future::Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    let slot: Arc<Mutex<Option<T>>> = Arc::new(Mutex::new(None));
    let slot2 = slot.clone();
    axtask::spawn(move || {
        let out = block_on(fut);
        *slot2.lock().unwrap() = Some(out);
    });
    slot
}

fn session_done<T>(slot: &Arc<Mutex<Option<T>>>, deadline: Instant, what: &str) -> T {
    loop {
        if let Some(out) = slot.lock().unwrap().take() {
            return out;
        }
        assert!(Instant::now() < deadline, "session never completed: {what}");
        yield_now();
    }
}

/// A raw second waiter parked on one socket's bridge through the public
/// `Pollable::register` — woken iff the epoch terminal wake fans out to the
/// object, without occupying a task.
fn raw_bridge_waiter(sock: &UdpSocket) -> Arc<std::sync::atomic::AtomicUsize> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Wake, Waker};
    struct CountWake(Arc<AtomicUsize>);
    impl Wake for CountWake {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let count = Arc::new(AtomicUsize::new(0));
    let waker = Waker::from(Arc::new(CountWake(count.clone())));
    sock.register(&mut Context::from_waker(&waker), IoEvents::IN | IoEvents::RDHUP);
    count
}

// ---------------------------------------------------------------------------
// The T6 contract test
// ---------------------------------------------------------------------------

#[test]
fn old_socket_terminal_precedes_wake_new_socket_is_clean_old_flush_is_aborted_not_c4() {
    let deadline = Instant::now() + Duration::from_secs(120);
    init_scheduler_once();

    // ── Setup: the production adapter as the only NIC, the queue owner
    //    Active — the exact Iteration 001 host sequence.
    let mut board = Board::new();
    board.env.set_phy_link_1000fd();
    let drv = board.ready_async_driver();
    let adapter = K3NetAdapter::new(drv, board.env.handle()).expect("adapter construction");
    let nic: Box<dyn NetDriverOps> = Box::new(adapter);
    init_network(AxDeviceContainer::from_one(nic));
    let _owner_handle = start_rx_task_affinity(schedulable_cpu_mask()).expect("queue owner");
    let mut owner = StackOwner { board };
    owner.pump_until(deadline, || rx_snapshot().lifecycle == 2, "owner Active");
    assert_eq!(rx_snapshot().owner, 1, "async owner, no polling fallback");

    payload::start_stack_runner_once(schedulable_cpu_mask()).expect("the one stack runner start");

    // Neighbor warm-up (the peer exists before any exchange).
    owner.board.wire.inject_rx_frame(&arp_announcement());
    publish_queue_event();
    owner.pump(32);

    let fault0 = rx_snapshot().fault;

    eprintln!("PHASE old-baseline");
    // ── The old epoch's data plane works: one full-byte UDP exchange on a
    //    live echo session (the session owns the port and drops it when done).
    let echo_slot = spawn_session(async {
        let sock = UdpSocket::new();
        sock.bind(SocketAddrEx::Ip(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            UDP_PORT,
        )))?;
        let mut buf = [0u8; 128];
        let mut from = SocketAddrEx::Ip(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0));
        let n = poll_io(&sock, IoEvents::IN, false, || {
            let opts = RecvOptions {
                from: Some(&mut from),
                ..Default::default()
            };
            sock.recv(&mut buf[..], opts)
        })
        .await?;
        let peer = match from {
            SocketAddrEx::Ip(addr) => addr,
            _ => return Err(axerrno::AxError::InvalidInput),
        };
        poll_io(&sock, IoEvents::OUT, false, || {
            sock.send(
                &buf[..n],
                SendOptions {
                    to: Some(SocketAddrEx::Ip(peer)),
                    ..Default::default()
                },
            )
        })
        .await
    });
    owner.pump(32);
    owner
        .board
        .wire
        .inject_rx_frame(&udp_frame(PEER_UDP_PORT, UDP_PORT, b"pre-recov"));
    publish_queue_event();
    owner.pump(64);
    let echoed: Vec<u8> = owner.drain_until(deadline, |frames| {
        frames
            .iter()
            .find_map(|f| udp_payload_from(f, UDP_PORT))
            .filter(|p| p.as_slice() == b"pre-recov")
    });
    assert_eq!(echoed, b"pre-recov", "old epoch carries every byte");
    let _ = session_done(&echo_slot, deadline, "old epoch echo");

    // The old object that carries the recovery witness: same port, same
    // epoch, nobody waiting on it.
    let old_sock = UdpSocket::new();
    old_sock
        .bind(SocketAddrEx::Ip(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), UDP_PORT)))
        .expect("old bind");
    // A raw second waiter on the old object (the fan-out witness).
    let raw_count = raw_bridge_waiter(&old_sock);

    // Old queued data: a datagram for the old socket with nobody waiting —
    // it parks in the old epoch's software path (socket queue or device
    // slots) and must never surface on a new-epoch object.
    owner
        .board
        .wire
        .inject_rx_frame(&udp_frame(PEER_UDP_PORT, UDP_PORT, b"stale-datagram"));
    publish_queue_event();
    owner.pump(64);

    eprintln!("PHASE blocked-waiters");
    // ── Old-epoch waiters parked with no data coming: a UDP recv and a TCP
    //    listener accept. They pend until the epoch terminal commits.
    let udp_waiter_slot = spawn_session({
        let sock = UdpSocket::new();
        async move {
            sock.bind(SocketAddrEx::Ip(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                UDP_WAITER_PORT,
            )))?;
            let mut buf = [0u8; 64];
            poll_io(&sock, IoEvents::IN, false, || {
                sock.recv(&mut buf[..], RecvOptions::default())
            })
            .await
        }
    });
    owner.pump(32);
    let listener_waiter_slot = spawn_session(async {
        let listener = TcpSocket::new();
        listener.bind(SocketAddrEx::Ip(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            TCP_PORT,
        )))?;
        listener.listen()?;
        poll_io(&listener, IoEvents::IN, false, || listener.accept()).await
    });
    owner.pump(32);

    eprintln!("PHASE flush-pending");
    // ── A Pending flush anchored on a ticket that is still QUEUED at the
    //    recovery entry: every hardware descriptor holds an incomplete
    //    (device-unsent) frame, so the next accepted frame parks in the slot
    //    storage. The quiesce will cancel it (a packet-loss terminal for the
    //    flush), never a C4 success.
    for i in 0..16usize {
        let rep = axnet::diagnostic_enqueue_frame([0xff; 6], 0x0800, &[b'f', i as u8])
            .expect("descriptor-fill enqueue");
        assert!(rep.accepted, "fill frame {i} must be accepted");
    }
    let mut left = false;
    owner.pump_until(
        deadline,
        || {
            publish_queue_event();
            let inflight = rx_snapshot_v3().tx_descriptor_inflight;
            if inflight > 0 {
                left = true;
            }
            left && inflight == 16
        },
        "16 descriptors in flight",
    );
    let rep = axnet::diagnostic_enqueue_frame([0xff; 6], 0x0800, b"pre-reset")
        .expect("queued-frame enqueue");
    assert!(rep.accepted, "the 17th frame takes a software slot");
    owner.pump(32);
    let mut flush_fut = core::pin::pin!(flush().expect("flush reservation"));
    {
        use core::task::{Context, Poll};
        let mut cx = Context::from_waker(core::task::Waker::noop());
        assert!(
            matches!(flush_fut.as_mut().poll(&mut cx), Poll::Pending),
            "the incomplete plane must keep the flush Pending"
        );
    }

    eprintln!("PHASE controlled-reset");
    // ── The controlled reset through the production entry: the resident
    //    owner stops, rebuilds the same arena, aborts the old-epoch flush and
    //    opens the next socket epoch. (The physical link-flap trigger is the
    //    T7 board matrix: the owner's link snapshot rides a 1 ms one-shot
    //    retry timer that is structurally unreachable on this host build.)
    axnet::recovery_reset_request().expect("the controlled reset request");
    // The recovery stages advance on owner re-polls; the test thread plays
    // the ISR half (publishes) AND the device half (completing the in-flight
    // descriptors so the quiesce drain converges). The wait requires the
    // lifecycle to LEAVE Active first — the pre-recovery Active snapshot
    // must not satisfy the condition.
    let mut left_active = false;
    loop {
        publish_queue_event();
        for _ in owner.board.wire.drain_board_tx() {}
        owner.pump(4);
        let lc = rx_snapshot().lifecycle;
        if lc != 2 {
            left_active = true;
        }
        if left_active && lc == 2 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "recovery never committed (back to Active after leaving it)"
        );
    }
    assert_eq!(rx_snapshot().owner, 1, "the recovery kept one async owner");
    assert_eq!(
        rx_snapshot().fault,
        fault0,
        "a successful controlled recovery commits no owner fault"
    );

    eprintln!("PHASE old_socket_terminal_precedes_wake");
    // Every old-epoch waiter observes the committed terminal — a wake that
    // surfaces an error implies the terminal was committed first (recv is
    // terminal-first), never a silent pend and never old data.
    let w1 = session_done(&udp_waiter_slot, deadline, "old udp waiter reset");
    assert!(
        matches!(w1, Err(axerrno::AxError::ConnectionReset)),
        "the controlled reset must surface ConnectionReset, got {w1:?}"
    );
    let w2 = session_done(&listener_waiter_slot, deadline, "old listener reset");
    let w2_err = w2.err();
    assert!(
        matches!(w2_err, Some(axerrno::AxError::ConnectionReset)),
        "the controlled reset must surface ConnectionReset, got {w2_err:?}"
    );
    assert!(
        raw_count.load(std::sync::atomic::Ordering::SeqCst) >= 1,
        "the terminal wake must fan out to a second registered waiter"
    );
    // The old object's terminal is stable and terminal-first: even with the
    // stale datagram already queued for it, recv reports the terminal.
    let mut stale_buf = [0u8; 64];
    let old_after = old_sock.recv(&mut stale_buf[..], RecvOptions::default());
    assert!(
        matches!(old_after, Err(axerrno::AxError::ConnectionReset)),
        "the closed epoch keeps erroring, got {old_after:?}"
    );

    eprintln!("PHASE old_flush_is_aborted_not_c4");
    // The flush observes the aborted outcome: the quiesce cancelled its
    // anchored ticket (a packet-loss terminal), never a C4 success.
    owner.pump(64);
    {
        use core::task::{Context, Poll};
        let mut cx = Context::from_waker(core::task::Waker::noop());
        let flush_deadline = Instant::now() + Duration::from_secs(10);
        let outcome = loop {
            if let Poll::Ready(res) = flush_fut.as_mut().poll(&mut cx) {
                break res;
            }
            assert!(Instant::now() < flush_deadline, "the aborted flush never settled");
            yield_now();
        };
        assert!(outcome.is_err(), "old-epoch flush must abort, got {outcome:?}");
    }

    // The old object keeps erroring after the recovery too (first-wins
    // identity, no resurrection), and the stale datagram never surfaced.
    let old_after_reset = old_sock.recv(&mut stale_buf[..], RecvOptions::default());
    assert!(
        old_after_reset.is_err(),
        "the old object stays terminal, got {old_after_reset:?}"
    );
    drop(old_sock);

    eprintln!("PHASE new-epoch-io");
    // ── New objects on the same ports complete real I/O under the new epoch.
    //    The first datagram must be the fresh one: old-epoch queued data is
    //    isolated (dropped with the old epoch, never re-delivered).
    owner.board.wire.reset_rx_fetch();
    let new_sock = UdpSocket::new();
    new_sock
        .bind(SocketAddrEx::Ip(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), UDP_PORT)))
        .expect("new bind on the old port");
    let udp_new_slot = spawn_session({
        let sock = new_sock;
        async move {
            let mut buf = [0u8; 128];
            let mut from = SocketAddrEx::Ip(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0));
            let n = poll_io(&sock, IoEvents::IN, false, || {
                let opts = RecvOptions {
                    from: Some(&mut from),
                    ..Default::default()
                };
                sock.recv(&mut buf[..], opts)
            })
            .await?;
            let peer = match from {
                SocketAddrEx::Ip(addr) => addr,
                _ => return Err(axerrno::AxError::InvalidInput),
            };
            poll_io(&sock, IoEvents::OUT, false, || {
                sock.send(
                    &buf[..n],
                    SendOptions {
                        to: Some(SocketAddrEx::Ip(peer)),
                        ..Default::default()
                    },
                )
            })
            .await
        }
    });
    owner.pump(32);
    owner
        .board
        .wire
        .inject_rx_frame(&udp_frame(PEER_UDP_PORT, UDP_PORT, b"post-recov"));
    publish_queue_event();
    let echoed2: Vec<u8> = owner.drain_until(deadline, |frames| {
        frames
            .iter()
            .find_map(|f| udp_payload_from(f, UDP_PORT))
            .filter(|p| p.as_slice() == b"post-recov")
    });
    assert_eq!(echoed2, b"post-recov", "new epoch carries every byte");
    let _ = session_done(&udp_new_slot, deadline, "new udp echo");

    // A fresh waiter object on a port that had an old object also serves.
    let fresh_sock = UdpSocket::new();
    fresh_sock
        .bind(SocketAddrEx::Ip(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            UDP_WAITER_PORT,
        )))
        .expect("new bind on the waiter port");
    owner
        .board
        .wire
        .inject_rx_frame(&udp_frame(PEER_UDP_PORT, UDP_WAITER_PORT, b"fresh-13"));
    publish_queue_event();
    let mut fresh_buf = [0u8; 64];
    let fresh_n = loop {
        match fresh_sock.recv(&mut fresh_buf[..], RecvOptions::default()) {
            Ok(n) => break n,
            Err(axerrno::AxError::WouldBlock) => {
                assert!(Instant::now() < deadline, "fresh recv never completed");
                owner.pump(8);
            }
            Err(e) => panic!("fresh recv failed: {e:?}"),
        }
    };
    assert_eq!(&fresh_buf[..fresh_n], b"fresh-13", "new object receives new data only");
    drop(fresh_sock);

    eprintln!("PHASE new-tcp-listener");
    // ── The new listener accepts, echoes every byte and closes actively.
    let listener_slot = spawn_session(async {
        let listener = TcpSocket::new();
        listener.bind(SocketAddrEx::Ip(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            TCP_PORT,
        )))?;
        listener.listen()?;
        let socket = poll_io(&listener, IoEvents::IN, false, || listener.accept()).await?;
        let Socket::Tcp(conn) = socket else {
            return Err(axerrno::AxError::InvalidInput);
        };
        let mut buf = [0u8; 64];
        let n = poll_io(&conn, IoEvents::IN, false, || {
            conn.recv(&mut buf[..], RecvOptions::default())
        })
        .await?;
        let sent = poll_io(&conn, IoEvents::OUT, false, || {
            conn.send(&buf[..n], SendOptions::default())
        })
        .await?;
        conn.shutdown(Shutdown::Both)?;
        Ok::<usize, axerrno::AxError>(sent)
    });
    owner.pump(32);
    let mut peer = TcpPeer::new(TCP_PORT);
    owner.board.wire.inject_rx_frame(&peer.syn());
    publish_queue_event();
    owner.pump(64);
    let synack_seg: TcpSeg = owner.drain_until(deadline, |frames| {
        frames
            .iter()
            .find_map(|f| parse_tcp(f))
            .filter(|s| s.flags & FLAG_SYN != 0)
    });
    peer.observe_synack(&synack_seg);
    owner.board.wire.inject_rx_frame(&peer.ack_handshake());
    publish_queue_event();
    owner.pump(64);
    owner
        .board
        .wire
        .inject_rx_frame(&peer.send_data(b"ms13-tcp"));
    publish_queue_event();
    owner.pump(64);
    let _echo: Vec<u8> = owner.drain_until(deadline, |frames| {
        frames
            .iter()
            .find_map(|f| parse_tcp(f))
            .and_then(|seg| peer.observe_board(&seg))
            .filter(|p| p.as_slice() == b"ms13-tcp")
    });
    assert_eq!(peer.data, b"ms13-tcp", "TCP echo must carry every byte");
    let tcp_len = session_done(&listener_slot, deadline, "new tcp echo")
        .expect("new tcp echo must complete");
    assert_eq!(tcp_len, 8, "the session echoed one 8-byte send");

    // The active close stays a normal FIN exchange (the new plane's close
    // semantics, not a reset-scarred RST).
    let fin_deadline = Instant::now() + Duration::from_secs(30);
    while peer.fin.is_none() {
        assert!(Instant::now() < fin_deadline, "the active close FIN never arrived");
        for f in owner.exchange() {
            if let Some(seg) = parse_tcp(&f) {
                peer.observe_board(&seg);
            }
        }
        owner.pump(8);
    }
    assert!(!peer.saw_rst, "active close must be a FIN, not a RST");
    owner.board.wire.inject_rx_frame(&peer.ack_fin_and_close());
    publish_queue_event();
    owner.pump(64);

    eprintln!("PHASE post-recovery-c4");
    // The recovery windows legitimately arm the stack runner's bounded
    // polling-fallback safety net (idle rounds while the lifecycle is
    // neither Active nor Faulted — stack_runner's catch-all arm). The
    // accepted invariant is about the ACTIVE steady state: from this point
    // on (every recovery committed, lifecycle Active) the runner must stay
    // purely event-driven, so the tail is asserted against this baseline.
    let active_tail = stack_snapshot();
    // ── The flush path is alive under the new epoch: the closed session's
    //    last frame is reclaimed by the queue owner first, then a fresh
    //    flush settles as a real C4. Bounded: a timeout fails with the
    //    scheduler counters instead of hanging the harness.
    let c4_deadline = Instant::now() + Duration::from_secs(20);
    let c4 = {
        let mut c4_fut = core::pin::pin!(flush().expect("post-recovery flush"));
        loop {
            use core::task::{Context, Poll};
            let mut cx = Context::from_waker(core::task::Waker::noop());
            if let Poll::Ready(res) = c4_fut.as_mut().poll(&mut cx) {
                break res;
            }
            if Instant::now() >= c4_deadline {
                let s0 = stack_snapshot();
                let v = rx_snapshot_v3();
                panic!(
                    "post-recovery flush never settled: lifecycle={} flush-target={} flush-ok={} flush-err={} stack-rounds={} stack-work={} task-poll={}",
                    v.lifecycle, v.flush_target, v.flush_success, v.flush_error,
                    s0.rounds, s0.work, s0.task_poll,
                );
            }
            // Device half of the same exchange() paradigm: complete what is
            // still in flight (the closed session's tail frames need no
            // peer) and kick the queue owner to reclaim it.
            let _drained = owner.board.wire.drain_board_tx();
            publish_queue_event();
            owner.pump(8);
        }
    };
    assert!(c4.is_ok(), "the new epoch's flush completes, got {c4:?}");

    let end = rx_snapshot();
    assert_eq!(end.lifecycle, 2, "the whole matrix keeps the owner Active");
    assert_eq!(end.owner, 1, "one async owner throughout");
    assert_eq!(end.fault, fault0, "no owner fault across the whole matrix");
    assert_eq!(
        stack_snapshot().fallback_poll,
        active_tail.fallback_poll,
        "the Active tail never falls back to polling (recovery-window safety-net \
         rounds are the runner's designed behavior and live in the baseline)"
    );
}

