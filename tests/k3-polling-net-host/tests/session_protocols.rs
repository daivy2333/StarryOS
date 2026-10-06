//! Task 3.1 witness: the bounded session end-to-end over a real smoltcp
//! memory peer — ARP resolution, ICMP echo, UDP echo with the 1024-byte
//! limit, TCP echo + close, the 5 s idle bound and cancellation.

use smoltcp::{
    socket::{tcp, udp},
    time::Instant as SmolInstant,
    wire::{IpAddress, IpEndpoint},
};

use k3_polling_net_host::fake::{self, FakeEnv, PeerHost};
use k3_polling_net_host::k3_gmac::{
    polling_net::{
        K3PollDevice, K3Session, SessionError, SessionFaultDetail, SessionStorage,
        TcpCloseState, BOARD_IPV4, UDP_ECHO_PORT,
    },
    GmacError,
};
use k3_polling_net_host::fake::LeakBacking;

struct World {
    board: K3Session<'static, LeakBacking, FakeEnv>,
    wire: fake::FakeWire,
    peer: PeerHost,
    ms: u64,
}

fn world() -> World {
    let mut b = FakeEnv::board();
    let device = K3PollDevice::new(b.ready_driver(), b.env);
    let wire = b.wire;
    let storage = Box::leak(Box::new(SessionStorage::new()));
    World {
        board: K3Session::new(device, storage),
        wire,
        peer: PeerHost::new(SmolInstant::from_millis(0)),
        ms: 0,
    }
}

/// One wire tick: board stack poll/echo services, board TX → peer, peer
/// stack poll, peer TX → board RX ring, board poll again, 1 ms of time.
fn tick(w: &mut World) {
    w.ms += 1;
    w.board.poll_stack();
    w.board.service_udp_echo();
    let _ = w.board.service_tcp_echo_and_close();
    for frame in w.wire.drain_board_tx() {
        w.peer.push_rx(frame);
    }
    w.peer.poll(SmolInstant::from_millis(w.ms as i64));
    for frame in w.peer.take_tx() {
        w.wire.inject_rx_frame(&frame);
    }
    w.board.poll_stack();
    w.board.service_udp_echo();
    let _ = w.board.service_tcp_echo_and_close();
    w.board.wait_nanos(1_000_000);
}

fn board_endpoint(port: u16) -> IpEndpoint {
    IpEndpoint::new(IpAddress::v4(BOARD_IPV4[0], BOARD_IPV4[1], BOARD_IPV4[2], BOARD_IPV4[3]), port)
}

fn peer_udp(w: &mut World) -> &mut udp::Socket<'static> {
    w.peer.sockets.get_mut::<udp::Socket>(w.peer.udp)
}

fn peer_tcp(w: &mut World) -> &mut tcp::Socket<'static> {
    w.peer.sockets.get_mut::<tcp::Socket>(w.peer.tcp)
}

fn peer_connect_tcp(w: &mut World) {
    let PeerHost { iface, sockets, tcp: handle, .. } = &mut w.peer;
    let cx = iface.context();
    sockets
        .get_mut::<tcp::Socket>(*handle)
        .connect(cx, board_endpoint(26002), 61002)
        .expect("peer connect");
}

#[test]
fn arp_and_udp_echo_roundtrip_through_the_device() {
    let mut w = world();
    peer_udp(&mut w)
        .send_slice(b"ms11-udp", board_endpoint(UDP_ECHO_PORT))
        .expect("peer udp send");
    let mut echoed = false;
    for _ in 0..200 {
        tick(&mut w);
        if let Ok((payload, _meta)) = peer_udp(&mut w).recv() {
            assert_eq!(payload, b"ms11-udp", "echo content matches the datagram");
            echoed = true;
            break;
        }
    }
    assert!(echoed, "the datagram echoed within the tick budget");
    let counters = w.board.counters();
    assert!(counters.tx_completed >= 1, "ARP reply and/or datagram terminals");
    assert!(counters.rx_frames >= 1);
    assert_eq!(counters.udp_echoed, 8);
}

#[test]
fn icmp_echo_request_is_answered_by_the_stack() {
    let mut w = world();
    // ICMP echo request as a raw ICMP message (type 8, checksum precomputed:
    // the peer dispatches the buffered bytes as-is).
    let mut msg = vec![8, 0, 0, 0];
    msg.extend_from_slice(&0x1234u16.to_be_bytes());
    msg.extend_from_slice(&1u16.to_be_bytes());
    msg.extend_from_slice(b"ms11-icmp");
    let csum = crate_checksum(&msg);
    msg[2..4].copy_from_slice(&csum.to_be_bytes());
    let peer_icmp_ep = IpAddress::v4(fake::BOARD_IP[0], fake::BOARD_IP[1], fake::BOARD_IP[2], fake::BOARD_IP[3]);
    let PeerHost { sockets, icmp, .. } = &mut w.peer;
    sockets
        .get_mut::<smoltcp::socket::icmp::Socket>(*icmp)
        .send_slice(&msg, peer_icmp_ep)
        .expect("peer icmp send");

    let mut replied = false;
    for _ in 0..200 {
        tick(&mut w);
        let PeerHost { sockets, icmp, .. } = &mut w.peer;
        if let Ok((reply, src)) = sockets.get_mut::<smoltcp::socket::icmp::Socket>(*icmp).recv() {
            assert_eq!(src, peer_icmp_ep);
            assert_eq!(reply[0], 0, "echo reply type");
            assert_eq!(&reply[4..6], &0x1234u16.to_be_bytes(), "ident preserved");
            assert_eq!(&reply[8..], b"ms11-icmp", "payload preserved");
            replied = true;
            break;
        }
    }
    assert!(replied, "the echo reply came back within the tick budget");
    assert_eq!(w.board.counters().icmp_echo_replies, 1);
}

fn crate_checksum(data: &[u8]) -> u16 {
    let mut sum = 0u32;
    for chunk in data.chunks(2) {
        let word = if chunk.len() == 2 {
            u16::from_be_bytes([chunk[0], chunk[1]]) as u32
        } else {
            (chunk[0] as u32) << 8
        };
        sum += word;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

#[test]
fn udp_echo_accumulates_bounded_at_1024_bytes() {
    let mut w = world();
    let big = [0x41u8; 600];
    peer_udp(&mut w)
        .send_slice(&big, board_endpoint(UDP_ECHO_PORT))
        .unwrap();
    for _ in 0..200 {
        tick(&mut w);
        if peer_udp(&mut w).recv().is_ok() {
            break;
        }
    }
    peer_udp(&mut w)
        .send_slice(&big, board_endpoint(UDP_ECHO_PORT))
        .unwrap();
    for _ in 0..200 {
        tick(&mut w);
        if peer_udp(&mut w).recv().is_ok() {
            break;
        }
    }
    // A third datagram has no budget left: counted, never echoed.
    peer_udp(&mut w)
        .send_slice(&big, board_endpoint(UDP_ECHO_PORT))
        .unwrap();
    for _ in 0..200 {
        tick(&mut w);
        if peer_udp(&mut w).recv().is_ok() {
            break;
        }
    }
    let counters = w.board.counters();
    assert_eq!(counters.udp_echoed, 1024, "echo accumulation stops at the limit");
    assert_eq!(counters.udp_over_limit, 1800 - 1024);
}

#[test]
fn tcp_echo_roundtrip_then_board_closes() {
    let mut w = world();
    peer_connect_tcp(&mut w);
    let mut established = false;
    for _ in 0..300 {
        tick(&mut w);
        if peer_tcp(&mut w).may_send() {
            established = true;
            break;
        }
    }
    assert!(established, "the connection established through the device");
    peer_tcp(&mut w).send_slice(b"ms11-tcp").unwrap();
    let mut echoed = Vec::new();
    for _ in 0..400 {
        tick(&mut w);
        let mut buf = [0u8; 64];
        loop {
            match peer_tcp(&mut w).recv_slice(&mut buf) {
                Ok(0) => break,
                Ok(n) => echoed.extend_from_slice(&buf[..n]),
                Err(tcp::RecvError::Finished) => break,
                Err(e) => panic!("unexpected recv error: {e}"),
            }
        }
        if matches!(w.board.counters().tcp_state, TcpCloseState::Closed) {
            break;
        }
    }
    assert_eq!(echoed, b"ms11-tcp", "echo content matches the stream");
    // The board closed (its FIN put the peer in CloseWait, recv reports
    // Finished). The client half-closes too — the standard PC flow after
    // reading the echo — completing the lifecycle in both directions.
    let mut buf = [0u8; 16];
    assert!(matches!(
        peer_tcp(&mut w).recv_slice(&mut buf),
        Err(tcp::RecvError::Finished)
    ));
    peer_tcp(&mut w).close();
    for _ in 0..400 {
        tick(&mut w);
        if matches!(w.board.counters().tcp_state, TcpCloseState::Closed) {
            break;
        }
    }
    let counters = w.board.counters();
    assert_eq!(counters.tcp_echoed, 8);
    assert_eq!(counters.tcp_state, TcpCloseState::Closed);
}

#[test]
fn connected_silence_times_out_at_five_seconds() {
    let mut w = world();
    peer_connect_tcp(&mut w);
    let mut established = false;
    for _ in 0..300 {
        tick(&mut w);
        if peer_tcp(&mut w).may_send() {
            established = true;
            break;
        }
    }
    assert!(established);
    // Silent peer: only the board clock advances; no wire, no echo progress.
    let mut outcome = Ok(());
    for _ in 0..6000 {
        w.board.wait_nanos(1_000_000);
        w.board.poll_stack();
        outcome = w.board.service_tcp_echo_and_close();
        if outcome.is_err() {
            break;
        }
    }
    assert!(
        matches!(outcome, Err(SessionError::TcpTimeout { .. })),
        "the 5 s idle bound ends the session: {outcome:?}"
    );
}

#[test]
fn cancellation_keeps_the_backing_and_reports_the_fault() {
    let mut w = world();
    w.board.request_cancel();
    w.board.poll_stack();
    let fault = w.board.take_fault().expect("cancel surfaces as a session fault");
    assert!(matches!(fault.detail, SessionFaultDetail::Driver(GmacError::Cancelled { .. })));
    assert!(w.board.backing_retained(), "published backing is never released");
}
