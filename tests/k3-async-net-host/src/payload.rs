//! Host mirror of the MS12 Iteration 002 T6 payload: the stack runner start
//! seam and the one-shot UDP/TCP echo sessions, written exactly the way the
//! board payload in `kernel/src/platform/k3_async_net.rs` runs. Everything
//! goes through the public axnet surface (`SocketOps` + `poll_io`/`timeout`,
//! per-operation bound); nothing ever calls `poll_interfaces` and there is
//! no polling timer — deadlines are real timer wakes (board: the runtime
//! timer IRQ; host: the environment has no platform clock, so the bound is
//! inert and the test thread bounds progress instead).

use core::net::{IpAddr, Ipv4Addr, SocketAddr};
use core::time::Duration;

use axerrno::AxError;
use axnet::{
    RecvOptions, SendOptions, Shutdown, Socket, SocketAddrEx, SocketOps, options::Configurable,
    start_stack_runner_affinity,
};
use axnet::tcp::TcpSocket;
use axnet::udp::UdpSocket;
use axpoll::IoEvents;
use axtask::future::{poll_io, timeout};

/// The fixed echo ports (T6 contract).
pub const UDP_ECHO_PORT: u16 = 26001;
pub const TCP_ECHO_PORT: u16 = 26002;
/// Per-operation bound (T6 contract): every payload wait is 5 s bounded.
pub const OP_BOUND_NANOS: u64 = 5_000_000_000;

fn op_bound() -> Option<Duration> {
    Some(Duration::from_nanos(OP_BOUND_NANOS))
}

/// Starts the ONE resident stack runner (T6). Failure stops: the caller must
/// never retry — the lifecycle is once-only by design.
pub fn start_stack_runner_once(mask: axtask::AxCpuMask) -> Result<(), ()> {
    start_stack_runner_affinity(mask).map(|_| ()).map_err(|_| ())
}

/// One UDP echo session: bind `UDP_ECHO_PORT`, receive one datagram, echo
/// every byte back to the sender.
pub async fn udp_echo_once() -> Result<(usize, SocketAddr), AxError> {
    let sock = UdpSocket::new();
    sock.bind(SocketAddrEx::Ip(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        UDP_ECHO_PORT,
    )))?;
    let mut buf = [0u8; 128];
    // `RecvOptions::from` needs an EXISTING address slot to write into.
    let mut from = SocketAddrEx::Ip(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0));
    let n = timeout(
        op_bound(),
        poll_io(&sock, IoEvents::IN, false, || {
            let opts = RecvOptions {
                from: Some(&mut from),
                ..Default::default()
            };
            sock.recv(&mut buf[..], opts)
        }),
    )
    .await
    .map_err(|_| AxError::TimedOut)??;
    let peer = match from {
        SocketAddrEx::Ip(addr) => addr,
        _ => return Err(AxError::InvalidInput),
    };
    let sent = timeout(
        op_bound(),
        poll_io(&sock, IoEvents::OUT, false, || {
            sock.send(
                &buf[..n],
                SendOptions {
                    to: Some(SocketAddrEx::Ip(peer)),
                    ..Default::default()
                },
            )
        }),
    )
    .await
    .map_err(|_| AxError::TimedOut)??;
    Ok((sent, peer))
}

/// One TCP echo session: listen on `TCP_ECHO_PORT`, accept one connection,
/// echo every byte of the peer's send, then actively close (normal FIN).
pub async fn tcp_echo_once_close() -> Result<usize, AxError> {
    let listener = TcpSocket::new();
    listener.bind(SocketAddrEx::Ip(SocketAddr::new(
        IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        TCP_ECHO_PORT,
    )))?;
    listener.listen()?;
    let socket = timeout(
        op_bound(),
        poll_io(&listener, IoEvents::IN, false, || listener.accept()),
    )
    .await
    .map_err(|_| AxError::TimedOut)??;
    let Socket::Tcp(conn) = socket else {
        return Err(AxError::InvalidInput);
    };
    let mut buf = [0u8; 64];
    let n = timeout(
        op_bound(),
        poll_io(&conn, IoEvents::IN, false, || {
            conn.recv(&mut buf[..], RecvOptions::default())
        }),
    )
    .await
    .map_err(|_| AxError::TimedOut)??;
    let sent = timeout(
        op_bound(),
        poll_io(&conn, IoEvents::OUT, false, || {
            conn.send(&buf[..n], SendOptions::default())
        }),
    )
    .await
    .map_err(|_| AxError::TimedOut)??;
    conn.shutdown(Shutdown::Both)?;
    Ok(sent)
}
