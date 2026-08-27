use alloc::{sync::Arc, vec};
use core::{
    net::{Ipv4Addr, SocketAddr},
    sync::atomic::{AtomicBool, Ordering},
    task::Context,
};

use axerrno::{AxError, AxResult, ax_bail, ax_err_type};
use axio::prelude::*;
use axpoll::{IoEvents, Pollable};
use axsync::Mutex;
use smoltcp::{
    iface::SocketHandle,
    socket::tcp as smol,
    time::Duration,
    wire::{IpEndpoint, IpListenEndpoint},
};

use crate::{
    LISTEN_TABLE, RecvFlags, RecvOptions, SOCKET_SET, SendOptions, Shutdown, Socket, SocketAddrEx,
    SocketOps,
    consts::{TCP_RX_BUF_LEN, TCP_TX_BUF_LEN},
    general::GeneralOptions,
    get_service,
    options::{Configurable, GetSocketOption, SetSocketOption},
    readiness::{
        self, ReadinessBridge, TERMINAL_CONNECT_REFUSED, TERMINAL_NONE, effective_terminal_code,
        terminal_ax_error,
    },
    state::*,
};

pub(crate) fn new_tcp_socket() -> smol::Socket<'static> {
    smol::Socket::new(
        smol::SocketBuffer::new(vec![0; TCP_RX_BUF_LEN]),
        smol::SocketBuffer::new(vec![0; TCP_TX_BUF_LEN]),
    )
}

/// A TCP socket that provides POSIX-like APIs.
pub struct TcpSocket {
    state: StateLock,
    handle: SocketHandle,
    readiness: Arc<ReadinessBridge>,
    general: GeneralOptions,
    rx_closed: AtomicBool,
}

unsafe impl Sync for TcpSocket {}

impl TcpSocket {
    /// Creates a new TCP socket.
    pub fn new() -> Self {
        let (handle, readiness) = SOCKET_SET.add_public(new_tcp_socket());
        Self {
            state: StateLock::new(State::Idle),
            handle,
            readiness,
            general: GeneralOptions::new(),
            rx_closed: AtomicBool::new(false),
        }
    }

    /// Creates a new TCP socket that is already connected.
    fn new_connected(handle: SocketHandle) -> Self {
        let readiness = Arc::new(ReadinessBridge::new());
        SOCKET_SET.install_readiness(handle, readiness.clone());
        Self {
            state: StateLock::new(State::Connected),
            handle,
            readiness,
            general: GeneralOptions::new(),
            rx_closed: AtomicBool::new(false),
        }
    }
}

impl Default for TcpSocket {
    fn default() -> Self {
        Self::new()
    }
}

/// Task 2.5: the single TCP readiness predicate shared by `poll` and the
/// register recheck, so readiness always agrees with the immediately
/// following `send`/`recv`. `local_rx_closed` is the axnet read-shutdown
/// flag; `axnet_state == Idle` means the socket was never used, so it stays
/// quiet until bound / listening / connected. `Connecting` readiness is
/// exclusively `poll_connect` (OUT on completion), never stream EOF bits.
fn tcp_readiness(socket: &smol::Socket, local_rx_closed: bool, axnet_state: State) -> IoEvents {
    let mut events = IoEvents::empty();
    if axnet_state == State::Idle || axnet_state == State::Connecting {
        return events;
    }
    if socket.can_recv() {
        events.insert(IoEvents::IN);
    }
    if !socket.may_recv() {
        // Peer closed and the recv buffer drained: `recv` returns Ok(0) (EOF),
        // so keep IN so a blocked recv wakes and observes the EOF.
        events.insert(IoEvents::IN);
        events.insert(IoEvents::RDHUP);
    }
    if socket.can_send() {
        events.insert(IoEvents::OUT);
    }
    if !socket.may_recv() && !socket.may_send() {
        events.insert(IoEvents::HUP);
    }
    if local_rx_closed {
        events.insert(IoEvents::RDHUP);
    }
    events
}

/// Private methods
impl TcpSocket {
    fn state(&self) -> State {
        self.state.get()
    }

    #[inline]
    fn is_listening(&self) -> bool {
        self.state() == State::Listening
    }

    fn with_smol_socket<R>(&self, f: impl FnOnce(&mut smol::Socket) -> R) -> R {
        SOCKET_SET.with_socket_mut::<smol::Socket, _, _>(self.handle, f)
    }

    /// Task 3.1: effective stable terminal code — the global data-plane
    /// fault takes precedence over the socket-local terminal.
    fn terminal_code(&self) -> u64 {
        effective_terminal_code(
            SOCKET_SET.global_terminal_code(),
            self.readiness.terminal_code(),
        )
    }

    /// Returns the stable terminal category and records it for the
    /// non-consuming `SO_ERROR` view, or `None` without terminal state.
    fn observe_terminal_error(&self) -> Option<AxError> {
        let code = self.terminal_code();
        if code == TERMINAL_NONE {
            return None;
        }
        let err = terminal_ax_error(code);
        self.general.record_socket_error(&err);
        Some(err)
    }

    /// Adds the `ERR` readiness bit once a stable terminal state exists.
    fn terminal_overlay(&self, mut events: IoEvents) -> IoEvents {
        if self.terminal_code() != TERMINAL_NONE {
            events.insert(IoEvents::ERR);
        }
        events
    }

    fn bound_endpoint(&self) -> AxResult<IpListenEndpoint> {
        let endpoint = SOCKET_SET
            .tcp_bound_endpoint(self.handle)
            .or_else(|| {
                self.with_smol_socket(|socket| socket.local_endpoint().map(IpListenEndpoint::from))
            })
            .unwrap_or_default();
        if endpoint.port == 0 {
            ax_bail!(InvalidInput, "not bound");
        }
        Ok(endpoint)
    }

    fn poll_connect(&self) -> IoEvents {
        let mut events = IoEvents::empty();
        let writable = self.with_smol_socket(|socket| match socket.state() {
            smol::State::SynSent => false, // wait for connection
            smol::State::Established => {
                self.state.set(State::Connected); // connected
                debug!(
                    "TCP socket {}: connected to {}",
                    self.handle,
                    socket.remote_endpoint().unwrap(),
                );
                true
            }
            _ => {
                self.state.set(State::Closed); // connection failed
                // Task 3.1: commit the stable completion error before the
                // completion event becomes observable (`OUT` then `ERR`).
                self.readiness.commit_terminal(TERMINAL_CONNECT_REFUSED);
                let err = terminal_ax_error(TERMINAL_CONNECT_REFUSED);
                self.general.record_socket_error(&err);
                true
            }
        });
        events.set(IoEvents::OUT, writable);
        if writable && self.terminal_code() != TERMINAL_NONE {
            events.insert(IoEvents::ERR);
        }
        events
    }

    fn poll_stream(&self) -> IoEvents {
        let rx_closed = self.rx_closed.load(Ordering::Acquire);
        let axnet_state = self.state();
        let events = self.with_smol_socket(|socket| tcp_readiness(socket, rx_closed, axnet_state));
        self.terminal_overlay(events)
    }

    fn poll_listener(&self) -> IoEvents {
        let mut events = IoEvents::empty();
        // Task 3.1 (D4): readiness inspects the first consumable accept
        // outcome; a queued Reset reports once as `IN|ERR` and never poisons
        // the listener.
        let port = self.bound_endpoint().unwrap().port;
        match LISTEN_TABLE.accept_head_is_reset(port) {
            Some(true) => {
                events.insert(IoEvents::IN);
                events.insert(IoEvents::ERR);
            }
            Some(false) => {
                events.insert(IoEvents::IN);
            }
            None => {}
        }
        self.terminal_overlay(events)
    }
}

impl Configurable for TcpSocket {
    fn get_option_inner(&self, option: &mut GetSocketOption) -> AxResult<bool> {
        use GetSocketOption as O;

        if self.general.get_option_inner(option)? {
            return Ok(true);
        }

        match option {
            O::NoDelay(no_delay) => {
                **no_delay = self.with_smol_socket(|socket| !socket.nagle_enabled());
            }
            O::KeepAlive(keep_alive) => {
                **keep_alive = self.with_smol_socket(|socket| socket.keep_alive().is_some());
            }
            O::MaxSegment(max_segment) => {
                // TODO(mivik): get actual MSS
                **max_segment = 1460;
            }
            O::SendBuffer(size) => {
                **size = TCP_TX_BUF_LEN;
            }
            O::ReceiveBuffer(size) => {
                **size = TCP_RX_BUF_LEN;
            }
            O::TcpInfo(_) => {
                // TODO(mivik): implement TCP_INFO
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn set_option_inner(&self, option: SetSocketOption) -> AxResult<bool> {
        use SetSocketOption as O;

        if self.general.set_option_inner(option)? {
            return Ok(true);
        }

        match option {
            O::NoDelay(no_delay) => {
                self.with_smol_socket(|socket| {
                    socket.set_nagle_enabled(!no_delay);
                });
            }
            O::KeepAlive(keep_alive) => {
                self.with_smol_socket(|socket| {
                    socket.set_keep_alive(keep_alive.then(|| Duration::from_secs(75)));
                });
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}
impl SocketOps for TcpSocket {
    fn bind(&self, local_addr: SocketAddrEx) -> AxResult {
        let mut local_addr = local_addr.into_ip()?;
        self.state
            .lock(State::Idle)
            .map_err(|_| ax_err_type!(InvalidInput, "already bound"))?
            .transit(State::Idle, || {
                // TODO: check addr is available
                if local_addr.port() == 0 {
                    local_addr.set_port(get_ephemeral_port()?);
                }
                if !self.general.reuse_address() {
                    SOCKET_SET.bind_check(local_addr.ip().into(), local_addr.port())?;
                }

                if SOCKET_SET.tcp_bound_endpoint(self.handle).is_some() {
                    return Err(AxError::InvalidInput);
                }
                let endpoint = IpListenEndpoint {
                    addr: if local_addr.ip().is_unspecified() {
                        None
                    } else {
                        Some(local_addr.ip().into())
                    },
                    port: local_addr.port(),
                };
                SOCKET_SET.set_tcp_bound_endpoint(self.handle, endpoint);
                debug!("TCP socket {}: binding to {}", self.handle, local_addr);
                Ok(())
            })?;
        crate::stack_runner::publish_software_work();
        Ok(())
    }

    fn connect(&self, remote_addr: SocketAddrEx) -> AxResult {
        // Task 3.2: a preexisting effective fatal precedes state transit,
        // route resolution and the smoltcp connect commit.
        if let Some(err) = self.observe_terminal_error() {
            return Err(err);
        }
        let remote_addr = remote_addr.into_ip()?;
        self.state
            .lock(State::Idle)
            .map_err(|state| {
                if state == State::Connecting {
                    AxError::InProgress
                } else {
                    // TODO(mivik): error code
                    ax_err_type!(AlreadyConnected)
                }
            })?
            .transit(State::Connecting, || {
                // TODO: check remote addr unreachable
                // let (bound_endpoint, remote_endpoint) = self.get_endpoint_pair(remote_addr)?;
                let remote_endpoint = IpEndpoint::from(remote_addr);
                let mut bound_endpoint = SOCKET_SET
                    .tcp_bound_endpoint(self.handle)
                    .unwrap_or_default();
                // Ordered `SERVICE -> SOCKET_SET` like the runner: resolve the
                // route and context under the Service guard, then commit to the
                // smoltcp socket under the SocketSet guard.
                let mut service = get_service();
                if bound_endpoint.addr.is_none() {
                    bound_endpoint.addr = Some(service.get_source_address(&remote_endpoint.addr));
                }
                if bound_endpoint.port == 0 {
                    bound_endpoint.port = get_ephemeral_port()?;
                }
                info!(
                    "TCP connection from {} to {}",
                    bound_endpoint, remote_endpoint
                );
                SOCKET_SET.set_tcp_bound_endpoint(self.handle, bound_endpoint);
                let context = service.iface.context();
                self.with_smol_socket(|socket| {
                    socket
                        .connect(context, remote_endpoint, bound_endpoint)
                        .map_err(|e| match e {
                            smol::ConnectError::InvalidState => {
                                ax_err_type!(AlreadyConnected)
                            }
                            smol::ConnectError::Unaddressable => {
                                ax_err_type!(ConnectionRefused, "unaddressable")
                            }
                        })?;
                    Ok(())
                })
            })?;

        crate::stack_runner::publish_software_work();

        // Hack: let the server listen
        axtask::yield_now();

        // Here our state must be `CONNECTING`, and only one thread can run here.
        self.general.send_poller(self, || {
            // Task 3.1: terminal-first — a committed global fault or a prior
            // failed completion returns its stable category instead of
            // blocking or re-deriving an error.
            if let Some(err) = self.observe_terminal_error() {
                return Err(err);
            }
            let events = self.poll_connect();
            if !events.contains(IoEvents::OUT) {
                Err(AxError::WouldBlock)
            } else if self.state() == State::Connected {
                Ok(())
            } else {
                Err(ax_err_type!(ConnectionRefused, "connection refused"))
            }
        })
    }

    fn listen(&self) -> AxResult {
        if let Ok(guard) = self.state.lock(State::Idle) {
            guard.transit(State::Listening, || {
                let bound_endpoint = self.bound_endpoint()?;
                LISTEN_TABLE.listen(bound_endpoint, self.readiness.clone())?;
                debug!("listening on {}", bound_endpoint);
                Ok(())
            })?;
        } else {
            // ignore simultaneous `listen`s.
        }
        crate::stack_runner::publish_software_work();
        Ok(())
    }

    fn accept(&self) -> AxResult<Socket> {
        // Task 3.2: a preexisting effective fatal precedes the listening
        // state check and never consumes queued Ready/Reset slots.
        if let Some(err) = self.observe_terminal_error() {
            return Err(err);
        }
        if !self.is_listening() {
            ax_bail!(InvalidInput, "not listening");
        }

        let bound_port = self.bound_endpoint()?.port;
        self.general.recv_poller(self, || {
            if let Some(err) = self.observe_terminal_error() {
                return Err(err);
            }
            // Task 2.7 replan: accept consumes the Ready/Reset slot and
            // refills an idle hidden listener inside one `SOCKET_SET ->
            // ListenTable entry` critical section, so an immediate
            // reconnect after a full backlog never waits for the runner's
            // next reconcile. Wakes publish only after both guards drop.
            let mut sockets = crate::SOCKET_SET.inner.lock();
            let result = LISTEN_TABLE.accept_with(bound_port, &mut sockets);
            drop(sockets);
            match result {
                Err(err @ AxError::WouldBlock) => Err(err),
                Ok(handle) => {
                    // Other accept waiters recheck after the entry lock drops;
                    // the refilled idle listener is already armed by the
                    // atomic accept+refill helper.
                    self.readiness.wake(IoEvents::IN);
                    crate::stack_runner::publish_software_work();
                    let socket = TcpSocket::new_connected(handle);
                    debug!(
                        "accepted connection from {}, {}",
                        handle,
                        socket.with_smol_socket(|socket| socket.remote_endpoint().unwrap())
                    );
                    Ok(Socket::Tcp(socket))
                }
                Err(err) => {
                    // A reset slot was consumed (backlog headroom freed); the
                    // atomic refill already restored an idle listener, and
                    // remaining waiters still recheck.
                    self.readiness.wake(IoEvents::IN);
                    crate::stack_runner::publish_software_work();
                    Err(err)
                }
            }
        })
    }

    fn send(&self, mut src: impl Read, _options: SendOptions) -> AxResult<usize> {
        // SAFETY: `self.handle` should be initialized in a connected socket.
        self.general.send_poller(self, || {
            if let Some(err) = self.observe_terminal_error() {
                return Err(err);
            }
            let result = self.with_smol_socket(|socket| {
                if !socket.is_active() {
                    Err(AxError::NotConnected)
                } else if !socket.can_send() {
                    Err(AxError::WouldBlock)
                } else {
                    // connected, and the tx buffer is not full
                    let len = socket
                        .send(|buffer| {
                            let result = src.read(buffer);
                            let len = result.unwrap_or(0);
                            (len, result)
                        })
                        .map_err(|_| ax_err_type!(NotConnected, "not connected?"))??;
                    Ok(len)
                }
            });
            if result.is_ok() {
                crate::stack_runner::publish_software_work();
            }
            result
        })
    }

    fn recv(&self, mut dst: impl Write + IoBufMut, options: RecvOptions<'_>) -> AxResult<usize> {
        // Task 3.2: a preexisting effective fatal precedes the rx-closed
        // branch and every protocol-state read.
        if let Some(err) = self.observe_terminal_error() {
            return Err(err);
        }
        if self.rx_closed.load(Ordering::Acquire) {
            return Err(AxError::NotConnected);
        }
        self.general.recv_poller(self, || {
            if let Some(err) = self.observe_terminal_error() {
                return Err(err);
            }
            let result = self.with_smol_socket(|socket| {
                if !socket.is_active() {
                    Err(AxError::NotConnected)
                } else if !socket.may_recv() {
                    Ok(0)
                } else if socket.recv_queue() == 0 {
                    Err(AxError::WouldBlock)
                } else if options.flags.contains(RecvFlags::PEEK) {
                    dst.write(
                        socket
                            .peek(dst.remaining_mut())
                            .map_err(|_| ax_err_type!(NotConnected, "not connected?"))?,
                    )
                } else {
                    socket
                        .recv(|buf| {
                            let result = dst.write(buf);
                            let len = result.unwrap_or(0);
                            (len, result)
                        })
                        .map_err(|_| ax_err_type!(NotConnected, "not connected?"))?
                }
            });
            if result.is_ok() && !options.flags.contains(RecvFlags::PEEK) {
                // A real dequeue releases protocol buffer/window state that the
                // unique runner must advance; PEEK is protocol-quiet.
                crate::stack_runner::publish_software_work();
            }
            result
        })
    }

    fn local_addr(&self) -> AxResult<SocketAddrEx> {
        let endpoint = self.bound_endpoint()?;
        Ok(SocketAddrEx::Ip(SocketAddr::new(
            endpoint
                .addr
                .map_or_else(|| Ipv4Addr::UNSPECIFIED.into(), Into::into),
            endpoint.port,
        )))
    }

    fn peer_addr(&self) -> AxResult<SocketAddrEx> {
        self.with_smol_socket(|socket| {
            Ok(SocketAddrEx::Ip(
                socket
                    .remote_endpoint()
                    .ok_or(AxError::NotConnected)?
                    .into(),
            ))
        })
    }

    fn shutdown(&self, how: Shutdown) -> AxResult {
        // TODO(mivik): shutdown
        if how.has_read() {
            self.rx_closed.store(true, Ordering::Release);
            self.readiness.wake(IoEvents::RDHUP);
        }

        // stream
        if let Ok(guard) = self.state.lock(State::Connected) {
            guard.transit(State::Closed, || {
                if how.has_write() {
                    self.with_smol_socket(|socket| {
                        debug!("TCP socket {}: shutting down", self.handle);
                        socket.close();
                    });
                }
                crate::stack_runner::publish_software_work();
                Ok(())
            })?;
        }

        // listener
        if let Ok(guard) = self.state.lock(State::Listening) {
            guard.transit(State::Closed, || {
                LISTEN_TABLE.unlisten(self.bound_endpoint()?.port);
                // Leftover accept waiters recheck after the entry cleanup.
                self.readiness.wake(IoEvents::IN);
                crate::stack_runner::publish_software_work();
                Ok(())
            })?;
        }

        // ignore for other states
        Ok(())
    }
}

impl Pollable for TcpSocket {
    fn poll(&self) -> IoEvents {
        let events = match self.state() {
            State::Connecting => self.poll_connect(),
            State::Connected | State::Idle | State::Closed => self.poll_stream(),
            State::Listening => self.poll_listener(),
            State::Busy => IoEvents::empty(),
        };
        self.terminal_overlay(events)
    }

    fn register(&self, context: &mut Context<'_>, events: IoEvents) {
        self.readiness.register(events, context.waker());
        if self.state() == State::Listening {
            // Accept waiters wake only from ListenTable transitions; the
            // listener socket itself never receives data, so recheck the
            // hidden-socket queue instead of a smoltcp slot.
            let ready = self.terminal_overlay(self.poll_listener()) & events;
            self.readiness.wake(ready);
        } else {
            let ready = self.with_smol_socket(|socket| {
                let slot_ready = self.readiness.rearm(socket, events);
                let mut full =
                    tcp_readiness(socket, self.rx_closed.load(Ordering::Acquire), self.state());
                if self.terminal_code() != TERMINAL_NONE {
                    full.insert(IoEvents::ERR);
                }
                slot_ready | (full & events)
            });
            self.readiness.wake(ready);
        }
    }
}

impl<'a> super::readiness::OneShotSocket for smol::Socket<'a> {
    fn rearm_read(&mut self, waker: &core::task::Waker) -> bool {
        self.register_recv_waker(waker);
        self.can_recv()
    }

    fn rearm_write(&mut self, waker: &core::task::Waker) -> bool {
        self.register_send_waker(waker);
        self.can_send()
    }
}

impl Drop for TcpSocket {
    fn drop(&mut self) {
        if let Err(err) = self.shutdown(Shutdown::Both) {
            warn!("TCP socket {}: shutdown failed: {}", self.handle, err);
        }
        // Detach public metadata (bridge + bound endpoint) and wake leftover
        // waiters once; the raw smoltcp handle stays for the resident runner.
        crate::SOCKET_SET.retire_public(self.handle);
        // Decide defer-vs-immediate from the raw close state. The state read
        // holds the SocketSet guard alone; the Service enqueue below holds
        // the Service guard alone — Drop never holds both at the same time,
        // so it can never form the reverse `SocketSet -> Service` edge the
        // removed synchronous flush introduced.
        let defer = {
            let sockets = crate::SOCKET_SET.inner.lock();
            let socket = sockets.get::<smol::Socket>(self.handle);
            close_kind(socket.state())
        };
        match defer {
            Some(kind) => {
                if let Some(service) = crate::SERVICE.get() {
                    // The runner owns the FIN/ACK progress and the final
                    // raw-handle removal; this caller runs zero rounds.
                    service.lock().queue_deferred_removal(self.handle, kind);
                } else {
                    // No Service installed -> no resident runner can reap;
                    // fall back to the safe immediate removal.
                    crate::SOCKET_SET.remove_raw(self.handle);
                }
            }
            // No outstanding close protocol (idle/closed/confirmed state):
            // the raw handle is removed immediately.
            None => crate::SOCKET_SET.remove_raw(self.handle),
        }
        // Kick the unique runner so the committed FIN and any deferred
        // retirement are progressed without this caller.
        crate::stack_runner::publish_software_work();
    }
}

/// Decides whether a closing smoltcp TCP state still needs runner-owned
/// protocol progress before its raw handle may be removed.
///
/// `Some(kind)` means the close is not yet acknowledged and the resident
/// runner must drive it; `None` means no deferred protocol is outstanding
/// (idle/closed/confirmed states) and the handle may be removed now.
pub(crate) fn close_kind(state: smol::State) -> Option<crate::service::CloseKind> {
    match state {
        // The transmit half just closed: the FIN (and possibly queued TX)
        // is still pending the peer's ACK.
        smol::State::FinWait1 | smol::State::Closing => Some(crate::service::CloseKind::Active),
        // The peer closed first and the local close entered LAST-ACK: the
        // FIN ACK must fully close the connection.
        smol::State::LastAck => Some(crate::service::CloseKind::LastAck),
        // FIN acknowledged or no close protocol committed: immediate.
        _ => None,
    }
}

fn get_ephemeral_port() -> AxResult<u16> {
    const PORT_START: u16 = 0xc000;
    const PORT_END: u16 = 0xffff;
    static CURR: Mutex<u16> = Mutex::new(PORT_START);

    let mut curr = CURR.lock();
    let mut tries = 0;
    // TODO: more robust
    while tries <= PORT_END - PORT_START {
        let port = *curr;
        if *curr == PORT_END {
            *curr = PORT_START;
        } else {
            *curr += 1;
        }
        if LISTEN_TABLE.can_listen(port) {
            return Ok(port);
        }
        tries += 1;
    }
    ax_bail!(AddrInUse, "no available ports");
}

#[cfg(test)]
mod tests {
    extern crate std;

    use axpoll::IoEvents;

    use super::{close_kind, new_tcp_socket, tcp_readiness};
    use crate::state::State;

    fn idle_readiness(socket: &smoltcp::socket::tcp::Socket) -> IoEvents {
        tcp_readiness(socket, false, State::Idle)
    }

    #[test]
    fn idle_socket_reports_no_stream_readiness() {
        // axnet-Idle (never used) must be quiet: no IN/OUT/HUP, and the old
        // `!may_send` pseudo-OUT is gone.
        let socket = new_tcp_socket();
        let events = idle_readiness(&socket);
        assert!(events.is_empty());
        assert!(!events.contains(IoEvents::OUT));
    }

    #[test]
    fn closed_socket_reports_eof_with_hup_and_no_out() {
        let mut socket = new_tcp_socket();
        socket.abort();
        let events = tcp_readiness(&socket, false, State::Closed);

        // poll reports IN|RDHUP|HUP so a blocked recv wakes to the error;
        // OUT is correctly absent (the old `!may_send` pseudo-OUT is gone).
        assert!(events.contains(IoEvents::IN));
        assert!(events.contains(IoEvents::RDHUP));
        assert!(events.contains(IoEvents::HUP));
        assert!(!events.contains(IoEvents::OUT));
        assert!(!socket.is_active());
    }

    #[test]
    fn connecting_socket_readiness_is_connect_only() {
        // A connecting socket must never surface stream EOF bits; readiness
        // comes exclusively from `poll_connect` (OUT on establishment).
        let socket = new_tcp_socket();
        assert!(tcp_readiness(&socket, false, State::Connecting).is_empty());
        let mut socket = new_tcp_socket();
        socket.abort();
        assert!(tcp_readiness(&socket, false, State::Connecting).is_empty());
    }

    #[test]
    fn local_read_shutdown_turns_on_rdhup_for_closed_socket() {
        // The local-rx-closed flag must surface as RDHUP once the socket
        // leaves the axnet-Idle state.
        let mut socket = new_tcp_socket();
        socket.abort();
        assert!(tcp_readiness(&socket, true, State::Connected).contains(IoEvents::RDHUP));
    }

    #[test]
    fn close_kind_decides_defer_vs_immediate_removal() {
        // T2.5-R2: a close is deferred only while its FIN is un-acknowledged
        // (FinWait1/Closing active; LastAck for a peer-first close); every
        // other state allows immediate raw-handle removal.
        use smoltcp::socket::tcp::State;

        use crate::service::CloseKind;

        assert_eq!(close_kind(State::FinWait1), Some(CloseKind::Active));
        assert_eq!(close_kind(State::Closing), Some(CloseKind::Active));
        assert_eq!(close_kind(State::LastAck), Some(CloseKind::LastAck));
        assert_eq!(close_kind(State::FinWait2), None);
        assert_eq!(close_kind(State::TimeWait), None);
        assert_eq!(close_kind(State::Closed), None);
        assert_eq!(close_kind(State::Established), None);
        assert_eq!(close_kind(State::SynReceived), None);
        assert_eq!(close_kind(State::SynSent), None);
        assert_eq!(close_kind(State::Listen), None);
        assert_eq!(close_kind(State::CloseWait), None);
        assert_eq!(close_kind(State::Closed), None);
    }

    // ── Task 3.1: terminal readiness and stable errors before wake ──────

    use core::net::{IpAddr, Ipv4Addr, SocketAddr};

    use axerrno::AxError;
    use axpoll::Pollable;

    use super::TcpSocket;
    use crate::{LISTEN_TABLE, SOCKET_SET, SocketAddrEx, SocketOps, readiness};

    #[test]
    fn normal_close_reports_eof_hup_without_device_err() {
        let mut raw = new_tcp_socket();
        raw.abort();
        let events = tcp_readiness(&raw, false, State::Closed);
        assert!(!events.contains(IoEvents::ERR));
    }

    #[test]
    fn connect_failure_commits_socket_local_terminal_before_reporting_out_err() {
        let socket = TcpSocket::new();
        socket.state.set(State::Connecting);
        socket.with_smol_socket(|s| s.abort());

        let events = socket.poll_connect();

        assert!(events.contains(IoEvents::OUT));
        assert!(events.contains(IoEvents::ERR));
        assert_eq!(
            socket.readiness.terminal_code(),
            readiness::TERMINAL_CONNECT_REFUSED
        );
        assert_eq!(socket.state(), State::Closed);
    }

    #[test]
    fn terminal_guard_maps_committed_codes_for_io() {
        let socket = TcpSocket::new();
        assert_eq!(socket.observe_terminal_error(), None);

        socket
            .readiness
            .commit_terminal(readiness::TERMINAL_RESOURCE_BUSY);
        assert_eq!(socket.observe_terminal_error(), Some(AxError::ResourceBusy));

        // Precedence (pure table): the global fault wins over socket-local.
        assert_eq!(
            readiness::effective_terminal_code(
                readiness::TERMINAL_NO_MEMORY,
                readiness::TERMINAL_RESOURCE_BUSY
            ),
            readiness::TERMINAL_NO_MEMORY
        );
    }

    #[test]
    fn listener_reset_head_polls_in_err_until_accept_consumes_it_once() {
        const PORT: u16 = 18500;
        let socket = TcpSocket::new();
        SocketOps::bind(
            &socket,
            SocketAddrEx::Ip(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                PORT,
            )),
        )
        .unwrap();
        SocketOps::listen(&socket).unwrap();

        assert!(!Pollable::poll(&socket).contains(IoEvents::IN));

        LISTEN_TABLE.test_push_reset_slot(PORT);

        let events = Pollable::poll(&socket);
        assert!(events.contains(IoEvents::IN));
        assert!(events.contains(IoEvents::ERR));

        // Consuming the queued reset clears the one-shot `IN|ERR` report
        // without committing any listener or device terminal state.
        let mut sockets = SOCKET_SET.inner.lock();
        assert!(matches!(
            LISTEN_TABLE.accept_with(PORT, &mut sockets),
            Err(AxError::ConnectionReset)
        ));
        drop(sockets);

        let events = Pollable::poll(&socket);
        assert!(!events.contains(IoEvents::ERR));
        assert!(!events.contains(IoEvents::IN));
        assert_eq!(socket.readiness.terminal_code(), readiness::TERMINAL_NONE);
    }

    // ── Task 3.2: entry ordering under preexisting terminal state ───────

    use core::sync::atomic::Ordering;

    use crate::RecvOptions;

    #[test]
    fn tcp_connect_entry_reports_preexisting_terminal_without_protocol_submit() {
        let socket = TcpSocket::new();
        socket
            .readiness
            .commit_terminal(readiness::TERMINAL_BAD_STATE);

        let err = SocketOps::connect(
            &socket,
            SocketAddrEx::Ip(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                9100,
            )),
        )
        .unwrap_err();
        assert_eq!(err, AxError::BadState);
        assert_eq!(
            socket.state(),
            State::Idle,
            "no Connecting transit or smoltcp submit may happen"
        );
    }

    #[test]
    fn tcp_accept_entry_reports_preexisting_terminal_before_state_checks() {
        let socket = TcpSocket::new(); // not listening
        socket.readiness.commit_terminal(readiness::TERMINAL_IO);

        let result = SocketOps::accept(&socket);
        assert!(
            matches!(result, Err(AxError::Io)),
            "the terminal precedes the not-listening InvalidInput"
        );
    }

    #[test]
    fn tcp_recv_entry_reports_preexisting_terminal_before_rxclosed() {
        let socket = TcpSocket::new();
        socket.rx_closed.store(true, Ordering::Release);
        socket.readiness.commit_terminal(readiness::TERMINAL_IO);

        let mut sink = std::vec::Vec::<u8>::new();
        let err = SocketOps::recv(&socket, &mut sink, RecvOptions::default()).unwrap_err();
        assert_eq!(
            err,
            AxError::Io,
            "the terminal precedes the rx-closed NotConnected"
        );
    }

    #[test]
    fn preexisting_terminal_leaves_queued_listener_reset_intact() {
        const PORT: u16 = 18501;
        let socket = TcpSocket::new();
        SocketOps::bind(
            &socket,
            SocketAddrEx::Ip(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                PORT,
            )),
        )
        .unwrap();
        SocketOps::listen(&socket).unwrap();
        LISTEN_TABLE.test_push_reset_slot(PORT);
        // A committed terminal (the same effective read a global fault
        // takes) must fail accept fast WITHOUT consuming the queued Reset.
        socket
            .readiness
            .commit_terminal(readiness::TERMINAL_NO_MEMORY);

        let err = SocketOps::accept(&socket);
        assert!(
            matches!(err, Err(AxError::NoMemory)),
            "accept must fail fast with the terminal category"
        );

        let mut sockets = SOCKET_SET.inner.lock();
        assert!(
            matches!(
                LISTEN_TABLE.accept_with(PORT, &mut sockets),
                Err(AxError::ConnectionReset)
            ),
            "the Reset slot survives the terminal-failed accept"
        );
        drop(sockets);
    }
}
