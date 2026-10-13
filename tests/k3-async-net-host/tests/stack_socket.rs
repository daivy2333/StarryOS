//! MS12 Iteration 002, task T6 (3.1): the T6 payload contract over the
//! production K3 adapter.
//!
//! Host mirror of the board `run_k3_async_net_service` shape (design D4):
//! after the queue owner reaches `Active`, the payload starts exactly one
//! stack runner and serves one UDP and one TCP echo session through the real
//! `SocketOps` + `poll_io`/`timeout` surface, never `poll_interfaces`. The
//! test thread plays the device half (FakeWire) and the ISR work-hint half,
//! exactly like `service_owner.rs`. Witnessed here: the device and software
//! wake sources, full-byte UDP/TCP echo with an actively closed TCP
//! connection (the peer reads the data and then EOF), the quiet window
//! (runner counters frozen, no polling fallback) and stable fault
//! propagation to blocked socket and flush waiters.
//!
//! The third wake source (protocol timer) is witnessed by the board V3 run
//! (2026-10-12 user direction): the host axhal build has no platform backend
//! (`wall_time()` is the 0ns stub), so the axtask timer wheel — and with it
//! smoltcp retransmission deadlines — can structurally never fire here; on
//! the board the runtime timer IRQ drives the same wheel.

#![allow(dead_code, unused_imports)]

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant};

use axdriver::AxDeviceContainer;
use axdriver_net::NetDriverOps;
use axnet::{
    RecvOptions, SendOptions, Shutdown, SocketAddrEx, SocketOps, flush,
    init_network, publish_queue_event, rx_snapshot, runner_software_nudge, stack_snapshot,
    start_rx_task_affinity,
};
use axnet::udp::UdpSocket;
use axpoll::IoEvents;
use axtask::future::{block_on, poll_io};
use axtask::{init_scheduler, schedulable_cpu_mask, yield_now};
use memory_addr::{PhysAddr, VirtAddr};

use k3_async_net_host::fake::Board;
use k3_async_net_host::k3_gmac::adapter::K3NetAdapter;
use k3_async_net_host::payload;

// Kernel ABI symbol required by `axklib` (pulled in via `axdriver/dyn`).
// Same stub as `service_owner.rs`: never called on this path.
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __axklib_0_3_mem_iomap(
    _addr: PhysAddr,
    _size: usize,
) -> axerrno::AxResult<VirtAddr> {
    Err(axerrno::AxError::Unsupported)
}

const BOARD_MAC: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x26, 0x01];
const PEER_MAC: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x26, 0x02];
/// The PC-side UDP source port (the echo's return address).
const PEER_UDP_PORT: u16 = 40000;

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

/// One complete peer→board IPv4/TCP frame.
fn tcp_frame(seq: u32, ack: u32, flags: u8, payload_bytes: &[u8]) -> Vec<u8> {
    let mut tcp = vec![0u8; 20 + payload_bytes.len()];
    tcp[0..2].copy_from_slice(&PEER_UDP_PORT.to_be_bytes());
    tcp[2..4].copy_from_slice(&26002u16.to_be_bytes());
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

/// A parsed board→peer TCP segment (payload from the IPv4 total length, so
/// Ethernet padding is ignored).
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

/// The board→peer UDP echo payload, if this frame is one.
fn udp_echo_payload(frame: &[u8]) -> Option<Vec<u8>> {
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
    let sport = u16::from_be_bytes([udp[0], udp[1]]);
    let dport = u16::from_be_bytes([udp[2], udp[3]]);
    if sport != 26001 || dport != PEER_UDP_PORT {
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
}

impl TcpPeer {
    fn new() -> Self {
        let isn = 0x51d0_0001;
        TcpPeer {
            isn,
            next: isn + 1,
            board_isn: 0,
            board_next: 0,
            data: Vec::new(),
            fin: None,
            saw_rst: false,
        }
    }

    fn syn(&self) -> Vec<u8> {
        tcp_frame(self.isn, 0, FLAG_SYN, &[])
    }

    /// Records the board's SYN-ACK (ISN + the echoed client ISN).
    fn observe_synack(&mut self, seg: &TcpSeg) {
        assert!(seg.flags & FLAG_SYN != 0 && seg.flags & FLAG_ACK != 0);
        assert_eq!(seg.ack, self.isn + 1, "SYN-ACK must ack our ISN+1");
        self.board_isn = seg.seq;
        self.board_next = seg.seq.wrapping_add(1);
    }

    fn ack_handshake(&self) -> Vec<u8> {
        tcp_frame(self.next, self.board_next, FLAG_ACK, &[])
    }

    fn send_data(&mut self, bytes: &[u8]) -> Vec<u8> {
        let frame = tcp_frame(self.next, self.board_next, FLAG_PSH | FLAG_ACK, bytes);
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
            // FIN consumes one sequence number after any payload carried in
            // the same segment; the watermark already advanced for payload.
            self.board_next = self.board_next.wrapping_add(1);
        }
        out
    }

    fn ack_board_data(&self) -> Vec<u8> {
        tcp_frame(self.next, self.board_next, FLAG_ACK, &[])
    }

    /// ACKs the board's FIN and closes our side (FIN).
    fn ack_fin_and_close(&mut self) -> Vec<u8> {
        let frame = tcp_frame(self.next, self.board_next, FLAG_FIN | FLAG_ACK, &[]);
        // Our own FIN consumes one sequence number; the board's final ACK
        // acknowledges past it.
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

// ---------------------------------------------------------------------------
// The T6 contract test
// ---------------------------------------------------------------------------

#[test]
fn stack_socket_three_source_close_fault_quiet_contract() {
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

    eprintln!("PHASE setup");
    // Neighbor warm-up (the board test's "PC 先一包"): announce the peer.
    owner.board.wire.inject_rx_frame(&arp_announcement());
    publish_queue_event();
    owner.pump(32);

    // ── The T6 startup seam: exactly one stack runner, duplicate rejected.
    payload::start_stack_runner_once(schedulable_cpu_mask())
        .expect("the one stack runner start");
    assert!(
        payload::start_stack_runner_once(schedulable_cpu_mask()).is_err(),
        "a second stack runner start must be rejected"
    );
    let stack = stack_snapshot();
    assert_eq!(stack.started, 1, "exactly one stack runner");
    assert_eq!(stack.fallback_poll, 0, "no polling fallback");

    // ── Wake source 1 (device): the injected datagram makes the queue owner
    //    reap and publish progress; the stack runner must wake on it.
    let udp_slot = spawn_session(payload::udp_echo_once());
    owner.pump(32); // bind happens before the first await
    let gen0 = stack_snapshot();
    owner
        .board
        .wire
        .inject_rx_frame(&udp_frame(PEER_UDP_PORT, 26001, b"ms12-udp"));
    publish_queue_event();
    owner.pump(64);
    let gen1 = stack_snapshot();
    assert!(
        gen1.generation > gen0.generation && gen1.task_poll > gen0.task_poll,
        "device wake: injected frame must reach the stack runner \
         (gen {}→{}, task_poll {}→{})",
        gen0.generation,
        gen1.generation,
        gen0.task_poll,
        gen1.task_poll
    );

    eprintln!("PHASE udp-echo");
    // Full-byte UDP echo on the wire.
    let echoed: Vec<u8> = owner.drain_until(deadline, |frames| {
        frames
            .iter()
            .find_map(|f| udp_echo_payload(f))
            .filter(|p| p.as_slice() == b"ms12-udp")
    });
    assert_eq!(echoed, b"ms12-udp", "echo must carry every byte");
    let (len, from) = {
        let res = session_done(&udp_slot, deadline, "udp echo");
        res.expect("udp echo must complete within the op bound")
    };
    assert_eq!(len, 8);
    assert_eq!(
        from.ip(),
        IpAddr::V4(Ipv4Addr::from(peer_ip())),
        "echo returns to the sender's address"
    );
    assert_eq!(from.port(), PEER_UDP_PORT, "echo returns to the sender's port");

    eprintln!("PHASE software-wake");
    // ── Wake source 2 (software): the nudge publishes a stack event.
    let gen2 = stack_snapshot();
    runner_software_nudge();
    owner.pump(32);
    let gen3 = stack_snapshot();
    assert!(
        gen3.generation > gen2.generation && gen3.task_poll > gen2.task_poll,
        "software nudge must wake the stack runner \
         (gen {}→{}, task_poll {}→{})",
        gen2.generation,
        gen3.generation,
        gen2.task_poll,
        gen3.task_poll
    );

    eprintln!("PHASE tcp-handshake");
    // ── TCP echo with an actively closed connection.
    let tcp_slot = spawn_session(payload::tcp_echo_once_close());
    owner.pump(32); // bind + listen before the SYN
    let mut peer = TcpPeer::new();
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
        .inject_rx_frame(&peer.send_data(b"ms12-tcp"));
    publish_queue_event();
    owner.pump(64);

    eprintln!("PHASE tcp-echo");
    // The board echoes the full 8 bytes.
    let _echo: Vec<u8> = owner.drain_until(deadline, |frames| {
        frames
            .iter()
            .find_map(|f| parse_tcp(f))
            .and_then(|seg| peer.observe_board(&seg))
            .filter(|p| p.as_slice() == b"ms12-tcp")
    });
    assert_eq!(peer.data, b"ms12-tcp", "TCP echo must carry every byte");
    let tcp_len = session_done(&tcp_slot, deadline, "tcp echo")
        .expect("tcp echo must complete within the op bound");
    assert_eq!(tcp_len, 8, "the payload echoed one 8-byte send");

    eprintln!("PHASE tcp-close");
    // The board actively closes (the FIN may ride with the echo segment):
    // collect frames until the FIN is recorded, never a RST.
    let fin_deadline = Instant::now() + Duration::from_secs(30);
    while peer.fin.is_none() {
        assert!(
            Instant::now() < fin_deadline,
            "the active close FIN never arrived"
        );
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
    // The board's final ACK of our FIN; afterwards the socket goes quiet.
    let final_deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let mut acked = false;
        for f in owner.exchange() {
            if let Some(seg) = parse_tcp(&f) {
                if seg.flags & FLAG_RST != 0 {
                    peer.saw_rst = true;
                }
                if seg.flags & FLAG_ACK != 0 && seg.ack == peer.next {
                    acked = true;
                }
            }
        }
        if acked {
            break;
        }
        assert!(
            Instant::now() < final_deadline,
            "the final close ACK never arrived"
        );
        owner.pump(8);
    }
    assert!(!peer.saw_rst, "the full close sequence must stay a normal close");

    eprintln!("PHASE quiet");
    // ── Quiet window: with no injection, no nudge and no due timer, the
    //    runner stays parked and the fallback counter never moves.
    let quiet_deadline = Instant::now() + Duration::from_millis(300);
    while Instant::now() < quiet_deadline {
        owner.pump(16);
        std::thread::sleep(Duration::from_millis(5));
    }
    let q0 = stack_snapshot();
    let quiet_end = Instant::now() + Duration::from_millis(300);
    while Instant::now() < quiet_end {
        owner.pump(16);
        std::thread::sleep(Duration::from_millis(5));
    }
    let q1 = stack_snapshot();
    assert_eq!(q1.task_poll, q0.task_poll, "quiet: runner stays parked");
    assert_eq!(q1.rounds, q0.rounds, "quiet: no rounds");
    assert_eq!(q1.work, q0.work, "quiet: no work");
    assert_eq!(q1.self_yield, q0.self_yield, "quiet: no self-yield");
    assert_eq!(q1.fallback_poll, 0, "quiet: no polling fallback");

    eprintln!("PHASE fault");
    // ── Stable fault propagates to blocked waiters.
    // A fresh UDP recv pends forever on the quiet data plane.
    let recv_slot = spawn_session(async {
        let sock = UdpSocket::new();
        sock.bind(SocketAddrEx::Ip(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::UNSPECIFIED),
            0,
        )))?;
        let mut buf = [0u8; 64];
        poll_io(&sock, IoEvents::IN, false, || {
            sock.recv(&mut buf[..], RecvOptions::default())
        })
        .await
    });
    owner.pump(32);
    // A held ticket anchors a Pending flush (the reclaim is held).
    axnet::diagnostic_control(2, 200).expect("hold tx reclaim");
    let rep = axnet::diagnostic_enqueue_frame([0xff; 6], 0x0800, b"pre-fault")
        .expect("enqueue under hold");
    assert!(rep.accepted, "the held ticket must be accepted");
    owner.pump(32);
    let mut flush_fut = core::pin::pin!(flush().expect("flush reservation"));
    {
        use core::task::{Context, Poll};
        let mut cx = Context::from_waker(core::task::Waker::noop());
        assert!(
            matches!(flush_fut.as_mut().poll(&mut cx), Poll::Pending),
            "the held ticket must keep the flush Pending"
        );
    }
    // The fatal: FBE on the wire, one real publish, the owner folds it.
    owner.board.env.set_reg(
        k3_async_net_host::k3_gmac::DMA_CHAN_STATUS,
        k3_async_net_host::k3_gmac::DMA_STATUS_FBE,
    );
    publish_queue_event();
    owner.pump_until(deadline, || rx_snapshot().lifecycle == 3, "owner Faulted");
    // The blocked socket waiter gets the terminal error, never silence.
    let recv_out = session_done(&recv_slot, deadline, "blocked recv fault propagation");
    assert!(
        recv_out.is_err(),
        "the fault must surface as an error, got {recv_out:?}"
    );
    // The flush waiter must never read a false success from a faulted plane.
    let flush_deadline = Instant::now() + Duration::from_secs(2);
    let mut flush_out = "still-pending";
    while Instant::now() < flush_deadline {
        use core::task::{Context, Poll};
        let mut cx = Context::from_waker(core::task::Waker::noop());
        if let Poll::Ready(res) = flush_fut.as_mut().poll(&mut cx) {
            flush_out = if res.is_ok() { "ok" } else { "err" };
            break;
        }
        owner.pump(32);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        flush_out != "ok",
        "a faulted plane must never complete a flush with Ok"
    );
}
