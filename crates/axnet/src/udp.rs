use alloc::{sync::Arc, vec};
use core::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    task::Context,
};

use axerrno::{AxError, AxResult, ax_bail, ax_err_type};
use axio::prelude::*;
use axpoll::{IoEvents, Pollable};
use axsync::Mutex;
use smoltcp::{
    iface::SocketHandle,
    phy::PacketMeta,
    socket::udp::{self as smol, UdpMetadata},
    storage::PacketMetadata,
    wire::{IpAddress, IpEndpoint, IpListenEndpoint},
};
use spin::RwLock;

use crate::{
    RecvFlags, RecvOptions, SOCKET_SET, SendOptions, Shutdown, SocketAddrEx, SocketOps,
    consts::{UDP_RX_BUF_LEN, UDP_TX_BUF_LEN},
    general::GeneralOptions,
    get_service,
    options::{Configurable, GetSocketOption, SetSocketOption},
    readiness::{ReadinessBridge, TERMINAL_NONE, effective_terminal_code, terminal_ax_error},
};

pub(crate) fn new_udp_socket() -> smol::Socket<'static> {
    // TODO(mivik): buffer size
    smol::Socket::new(
        smol::PacketBuffer::new(vec![PacketMetadata::EMPTY; 256], vec![0; UDP_RX_BUF_LEN]),
        smol::PacketBuffer::new(vec![PacketMetadata::EMPTY; 256], vec![0; UDP_TX_BUF_LEN]),
    )
}

/// A UDP socket that provides POSIX-like APIs.
pub struct UdpSocket {
    handle: SocketHandle,
    readiness: Arc<ReadinessBridge>,
    local_addr: RwLock<Option<IpEndpoint>>,
    peer_addr: RwLock<Option<(IpEndpoint, IpAddress)>>,

    general: GeneralOptions,
    /// Task 5.1 (Iteration 006): test-only per-fixture registry pair.
    /// `None` (production and default tests) routes every access through the
    /// process-global `SOCKET_SET`.
    #[cfg(test)]
    test_ctx: Option<crate::wrapper::SocketTestContext>,
}

impl UdpSocket {
    /// Creates a new UDP socket.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let socket = new_udp_socket();
        let (handle, readiness) = SOCKET_SET.add_public(socket);

        Self {
            handle,
            readiness,
            local_addr: RwLock::new(None),
            peer_addr: RwLock::new(None),

            general: GeneralOptions::new(),
            #[cfg(test)]
            test_ctx: None,
        }
    }

    /// Task 5.1 (Iteration 006): test-only constructor binding the fixture's
    /// independent registry; the R57 global-churn race disappears because no
    /// test fixture ever touches the process-global registry.
    #[cfg(test)]
    pub(crate) fn new_with_context(ctx: crate::wrapper::SocketTestContext) -> Self {
        let (handle, readiness) = ctx.sockets.add_public(new_udp_socket());
        Self {
            handle,
            readiness,
            local_addr: RwLock::new(None),
            peer_addr: RwLock::new(None),
            general: GeneralOptions::new(),
            test_ctx: Some(ctx),
        }
    }

    /// The `SocketSetWrapper` this socket's raw smoltcp handle lives in: the
    /// test-injected fixture context when present, else the production
    /// singleton. Every access path routes through here so a socket never
    /// crosses from its fixture into the global (or a neighbor's) registry.
    fn sockets(&self) -> &'static crate::wrapper::SocketSetWrapper<'static> {
        #[cfg(test)]
        {
            if let Some(ctx) = self.test_ctx {
                return ctx.sockets;
            }
        }
        &*crate::SOCKET_SET
    }

    /// The Service that owns this socket's deferred removal: the fixture's
    /// paired local Service when a test context is present, else the
    /// production global. Task 5.1 Cycle 001 (rework): a fixture-local
    /// handle must be retired by the fixture's own Service - the global
    /// runner reaps with the global socket set and would misinterpret the
    /// handle (or collide with an equal numeric handle there). Production
    /// sockets keep the global route. The lock discipline is unchanged: the
    /// Drop caller holds the Service guard alone (never together with the
    /// socket-set guard).
    fn deferred_service(&self) -> Option<&'static Mutex<crate::service::Service>> {
        #[cfg(test)]
        {
            if let Some(ctx) = self.test_ctx {
                return Some(ctx.service);
            }
        }
        crate::SERVICE.get()
    }

    fn with_smol_socket<R>(&self, f: impl FnOnce(&mut smol::Socket) -> R) -> R {
        self.sockets()
            .with_socket_mut::<smol::Socket, _, _>(self.handle, f)
    }

    fn remote_endpoint(&self) -> AxResult<(IpEndpoint, IpAddress)> {
        match self.peer_addr.try_read() {
            Some(addr) => addr.ok_or(AxError::NotConnected),
            None => Err(AxError::NotConnected),
        }
    }

    /// Task 3.1: effective stable terminal code — the global data-plane
    /// fault takes precedence over the socket-local terminal.
    fn terminal_code(&self) -> u64 {
        effective_terminal_code(
            self.sockets().global_terminal_code(),
            self.readiness.terminal_code(),
        )
    }

    fn observe_terminal_error(&self) -> Option<AxError> {
        let code = self.terminal_code();
        if code == TERMINAL_NONE {
            return None;
        }
        let err = terminal_ax_error(code);
        self.general.record_socket_error(&err);
        Some(err)
    }
}

/// Peer-matching policy for one receive attempt (module-level so the
/// attempt is an extractable, testable path).
enum ExpectedRemote<'a> {
    Any(&'a mut SocketAddrEx),
    Expecting(IpEndpoint),
    Ignore,
}

impl UdpSocket {
    /// Single receive attempt shared verbatim by the blocking poll_io
    /// closure and model witnesses: dequeues at most one datagram into
    /// `dst`, matching `expected_remote` when a peer is pinned.
    fn try_recv_once(
        &self,
        dst: &mut impl Write,
        expected_remote: &mut ExpectedRemote<'_>,
        flags: RecvFlags,
    ) -> AxResult<usize> {
        // Task 3.2: every retry observes the effective terminal, so a fatal
        // landing between attempts returns its stable category instead of
        // another WouldBlock.
        if let Some(err) = self.observe_terminal_error() {
            return Err(err);
        }
        self.with_smol_socket(|socket| {
            if !socket.is_open() {
                // not bound
                Err(ax_err_type!(NotConnected))
            } else if !socket.can_recv() {
                info!("UDP socket {}: recv recheck WouldBlock", self.handle);
                Err(AxError::WouldBlock)
            } else {
                let result = if flags.contains(RecvFlags::PEEK) {
                    socket.peek().map(|(data, meta)| (data, *meta))
                } else {
                    socket.recv()
                };
                match result {
                    Ok((src, meta)) => {
                        match expected_remote {
                            ExpectedRemote::Any(remote_addr) => {
                                **remote_addr = SocketAddrEx::Ip(meta.endpoint.into());
                            }
                            ExpectedRemote::Expecting(expected) => {
                                if (!expected.addr.is_unspecified()
                                    && expected.addr != meta.endpoint.addr)
                                    || (expected.port != 0 && expected.port != meta.endpoint.port)
                                {
                                    return Err(AxError::WouldBlock);
                                }
                            }
                            ExpectedRemote::Ignore => {}
                        }

                        let read = dst.write(src)?;
                        if read < src.len() {
                            warn!("UDP message truncated: {} -> {} bytes", src.len(), read);
                        }
                        info!("UDP socket {}: recv {} bytes", self.handle, read);

                        Ok(if flags.contains(RecvFlags::TRUNCATE) {
                            src.len()
                        } else {
                            read
                        })
                    }
                    Err(smol::RecvError::Exhausted) => Err(AxError::WouldBlock),
                    Err(smol::RecvError::Truncated) => {
                        unreachable!("UDP socket recv never returns Err(Truncated)")
                    }
                }
            }
        })
    }

    /// Single send attempt shared verbatim by the blocking poll_io closure
    /// and model witnesses: reads the effective terminal first, then
    /// enqueues at most one datagram.
    fn try_send_once<S: Read + IoBuf>(
        &self,
        src: &mut S,
        remote_addr: IpEndpoint,
        source_addr: IpAddress,
    ) -> AxResult<usize> {
        if let Some(err) = self.observe_terminal_error() {
            return Err(err);
        }
        self.with_smol_socket(|socket| {
            if !socket.is_open() {
                // not connected
                Err(ax_err_type!(NotConnected))
            } else if !socket.can_send() {
                Err(AxError::WouldBlock)
            } else {
                let buf = socket
                    .send(
                        src.remaining(),
                        UdpMetadata {
                            endpoint: remote_addr,
                            local_address: Some(source_addr),
                            meta: PacketMeta::default(),
                        },
                    )
                    .map_err(|e| match e {
                        smol::SendError::BufferFull => AxError::WouldBlock,
                        smol::SendError::Unaddressable => {
                            ax_err_type!(ConnectionRefused, "unaddressable")
                        }
                    })?;
                let read = src.read(buf)?;
                assert_eq!(read, buf.len());
                Ok(read)
            }
        })
    }
}

impl Configurable for UdpSocket {
    fn get_option_inner(&self, option: &mut GetSocketOption) -> AxResult<bool> {
        use GetSocketOption as O;

        if self.general.get_option_inner(option)? {
            return Ok(true);
        }
        match option {
            O::Ttl(ttl) => {
                self.with_smol_socket(|socket| {
                    **ttl = socket.hop_limit().unwrap_or(64);
                });
            }
            O::SendBuffer(size) => {
                **size = UDP_TX_BUF_LEN;
            }
            O::ReceiveBuffer(size) => {
                **size = UDP_RX_BUF_LEN;
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
            O::Ttl(ttl) => {
                self.with_smol_socket(|socket| {
                    socket.set_hop_limit(Some(*ttl));
                });
            }
            _ => return Ok(false),
        }
        Ok(true)
    }
}
impl SocketOps for UdpSocket {
    fn bind(&self, local_addr: SocketAddrEx) -> AxResult {
        let mut local_addr = local_addr.into_ip()?;
        let mut guard = self.local_addr.write();

        if local_addr.port() == 0 {
            local_addr.set_port(get_ephemeral_port()?);
        }
        if guard.is_some() {
            ax_bail!(InvalidInput, "already bound");
        }

        let local_endpoint = IpEndpoint::from(local_addr);
        let endpoint = IpListenEndpoint {
            addr: (!local_endpoint.addr.is_unspecified()).then_some(local_endpoint.addr),
            port: local_endpoint.port,
        };

        if !self.general.reuse_address() {
            // Check if the address is already in use
            self.sockets()
                .bind_check(local_endpoint.addr, local_endpoint.port)?;
        }

        self.with_smol_socket(|socket| {
            socket.bind(endpoint).map_err(|e| match e {
                smol::BindError::InvalidState => ax_err_type!(InvalidInput, "already bound"),
                smol::BindError::Unaddressable => ax_err_type!(ConnectionRefused, "unaddressable"),
            })
        })?;

        *guard = Some(local_endpoint);
        info!("UDP socket {}: bound on {}", self.handle, endpoint);
        crate::stack_runner::publish_software_work();
        Ok(())
    }

    fn connect(&self, remote_addr: SocketAddrEx) -> AxResult {
        // Task 3.2: a preexisting effective fatal precedes address parsing,
        // implicit bind and the peer-endpoint commit.
        if let Some(err) = self.observe_terminal_error() {
            return Err(err);
        }
        let remote_addr = remote_addr.into_ip()?;
        let mut guard = self.peer_addr.write();
        if self.local_addr.read().is_none() {
            self.bind(SocketAddrEx::Ip(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                0,
            )))?;
        }

        let remote_addr = IpEndpoint::from(remote_addr);
        let src = get_service().get_source_address(&remote_addr.addr);
        *guard = Some((remote_addr, src));
        debug!("UDP socket {}: connected to {}", self.handle, remote_addr);
        crate::stack_runner::publish_software_work();
        Ok(())
    }

    fn send(&self, mut src: impl Read + IoBuf, options: SendOptions) -> AxResult<usize> {
        // Task 3.2: a preexisting effective fatal precedes remote-address
        // resolution and any implicit bind.
        if let Some(err) = self.observe_terminal_error() {
            return Err(err);
        }
        let (remote_addr, source_addr) = match options.to {
            Some(addr) => {
                let addr = IpEndpoint::from(addr.into_ip()?);
                let src = get_service().get_source_address(&addr.addr);
                (addr, src)
            }
            None => self.remote_endpoint()?,
        };
        if remote_addr.port == 0 || remote_addr.addr.is_unspecified() {
            ax_bail!(InvalidInput, "invalid address");
        }

        if self.local_addr.read().is_none() {
            self.bind(SocketAddrEx::Ip(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                0,
            )))?;
        }
        self.general.send_poller(self, || {
            let result = self.try_send_once(&mut src, remote_addr, source_addr);
            if result.is_ok() {
                crate::stack_runner::publish_software_work();
            }
            result
        })
    }

    fn recv(&self, mut dst: impl Write, options: RecvOptions) -> AxResult<usize> {
        // Task 3.1: terminal-first, before the bound pre-check, so a
        // committed fault surfaces as its stable category on any affected
        // socket.
        if let Some(err) = self.observe_terminal_error() {
            return Err(err);
        }
        if self.local_addr.read().is_none() {
            ax_bail!(NotConnected);
        }

        let mut expected_remote = match options.from {
            Some(addr) => ExpectedRemote::Any(addr),
            None => match *self.peer_addr.read() {
                Some((endpoint, _)) => ExpectedRemote::Expecting(endpoint),
                None => ExpectedRemote::Ignore,
            },
        };

        self.general.recv_poller(self, || {
            let result = self.try_recv_once(&mut dst, &mut expected_remote, options.flags);
            if result.is_ok() && !options.flags.contains(RecvFlags::PEEK) {
                crate::stack_runner::publish_software_work();
            }
            result
        })
    }

    fn local_addr(&self) -> AxResult<SocketAddrEx> {
        match self.local_addr.try_read() {
            Some(addr) => addr
                .map(Into::into)
                .map(SocketAddrEx::Ip)
                .ok_or(AxError::NotConnected),
            None => Err(AxError::NotConnected),
        }
    }

    fn peer_addr(&self) -> AxResult<SocketAddrEx> {
        self.remote_endpoint()
            .map(|it| it.0.into())
            .map(SocketAddrEx::Ip)
    }

    fn shutdown(&self, _how: Shutdown) -> AxResult {
        // TODO(mivik): shutdown
        self.with_smol_socket(|socket| {
            debug!("UDP socket {}: shutting down", self.handle);
            socket.close();
        });
        crate::stack_runner::publish_software_work();
        Ok(())
    }
}

impl Pollable for UdpSocket {
    fn poll(&self) -> IoEvents {
        let bound = self.local_addr.read().is_some();
        let mut events = self.with_smol_socket(|socket| udp_readiness(socket, bound));
        if self.terminal_code() != TERMINAL_NONE {
            events.insert(IoEvents::ERR);
        }
        events
    }

    fn register(&self, context: &mut Context<'_>, events: IoEvents) {
        self.readiness.register(events, context.waker());
        let bound = self.local_addr.read().is_some();
        let ready = self.with_smol_socket(|socket| {
            let slot_ready = self.readiness.rearm(socket, events);
            let mut full = udp_readiness(socket, bound);
            if self.terminal_code() != TERMINAL_NONE {
                full.insert(IoEvents::ERR);
            }
            slot_ready | (full & events)
        });
        self.readiness.wake(ready);
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

impl Drop for UdpSocket {
    fn drop(&mut self) {
        // T2.7: dropping a socket whose TX buffer still holds an
        // undispatched datagram must NOT reset/remove it (smoltcp `close()`
        // resets the TX buffer; removing drops the queued packet). The
        // resident runner dispatches the queued datagram in its egress
        // rounds and the reaper reclaims the raw handle once the TX drained
        // (guest MS01 udp-bidirectional lost the fork child's echo otherwise).
        // `has_pending_tx()` observes actual occupancy; `can_send()`
        // (capacity-not-full) would misclassify an empty buffer as queued.
        let has_queued_tx = {
            let sockets = self.sockets().inner.lock();
            sockets.get::<smol::Socket>(self.handle).has_pending_tx()
        };
        if has_queued_tx {
            // The Service owning this handle's queued-TX retirement: the
            // socket's own context (test fixture) or the production global.
            if let Some(service) = self.deferred_service() {
                self.sockets().retire_public(self.handle);
                service
                    .lock()
                    .queue_deferred_removal(self.handle, crate::service::CloseKind::UdpQueued);
                crate::stack_runner::publish_software_work();
                return;
            }
            // No resident runner installed: fall through to the safe
            // immediate teardown (the queued datagram is lost, matching the
            // pre-fix close semantics when there is no runner to dispatch).
        }
        self.shutdown(Shutdown::Both).ok();
        self.sockets().remove(self.handle);
    }
}

fn get_ephemeral_port() -> AxResult<u16> {
    const PORT_START: u16 = 0xc000;
    const PORT_END: u16 = 0xffff;
    static CURR: Mutex<u16> = Mutex::new(PORT_START);
    let mut curr = CURR.lock();

    let port = *curr;
    if *curr == PORT_END {
        *curr = PORT_START;
    } else {
        *curr += 1;
    }
    Ok(port)
}

/// Task 2.5: the single UDP readiness predicate shared by `poll` and the
/// register recheck, so readiness agrees with the next `recv`/`send`.
/// `bound` mirrors the axnet `local_addr` state; shut-down sockets report HUP.
fn udp_readiness(socket: &smol::Socket, bound: bool) -> IoEvents {
    if !bound {
        return IoEvents::empty();
    }
    let mut events = IoEvents::empty();
    if socket.is_open() {
        if socket.can_recv() {
            events.insert(IoEvents::IN);
        }
        if socket.can_send() {
            events.insert(IoEvents::OUT);
        }
    } else {
        events.insert(IoEvents::HUP);
    }
    events
}

#[cfg(test)]
mod tests {
    extern crate std;

    use axpoll::IoEvents;
    use smoltcp::wire::IpListenEndpoint;

    use super::{new_udp_socket, udp_readiness};

    #[test]
    fn unbound_socket_reports_no_readiness() {
        let socket = new_udp_socket();
        assert!(udp_readiness(&socket, false).is_empty());
    }

    #[test]
    fn bound_socket_with_send_room_reports_out() {
        let mut socket = new_udp_socket();
        assert!(
            socket
                .bind(IpListenEndpoint {
                    addr: None,
                    port: 9000
                })
                .is_ok()
        );
        let events = udp_readiness(&socket, true);

        assert!(events.contains(IoEvents::OUT));
        assert!(!events.contains(IoEvents::IN));
        assert!(!events.contains(IoEvents::HUP));
    }

    // ── Task 3.1: terminal readiness overlay and terminal-first I/O ─────

    use axerrno::AxError;
    use axpoll::Pollable;

    use super::UdpSocket;
    use crate::{readiness, wrapper::SocketTestContext};

    /// Task 5.1 (Iteration 006): leaked per-test fixture; removes the R57
    /// global `SOCKET_SET` churn prerequisite.
    fn test_ctx() -> SocketTestContext {
        SocketTestContext::leak_new()
    }

    #[test]
    fn normal_udp_states_stay_free_of_device_err() {
        let mut raw = new_udp_socket();
        raw.bind(IpListenEndpoint {
            addr: None,
            port: 9050,
        })
        .unwrap();
        assert!(!udp_readiness(&raw, true).contains(IoEvents::ERR));
        raw.close();
        assert!(!udp_readiness(&raw, true).contains(IoEvents::ERR));
    }

    #[test]
    fn terminal_commit_surfaces_err_on_poll() {
        let socket = UdpSocket::new_with_context(test_ctx());
        assert!(Pollable::poll(&socket).is_empty());

        socket
            .readiness
            .commit_terminal(readiness::TERMINAL_BAD_STATE);

        let events = Pollable::poll(&socket);
        assert!(events.contains(IoEvents::ERR));
    }

    #[test]
    fn terminal_guard_maps_committed_codes_for_udp_io() {
        let socket = UdpSocket::new_with_context(test_ctx());
        assert_eq!(socket.observe_terminal_error(), None);

        socket
            .readiness
            .commit_terminal(readiness::TERMINAL_CONNECT_REFUSED);
        assert_eq!(
            socket.observe_terminal_error(),
            Some(AxError::ConnectionRefused)
        );
    }

    #[test]
    fn rebound_socket_after_close_reports_io_again() {
        let mut socket = new_udp_socket();
        socket
            .bind(IpListenEndpoint {
                addr: None,
                port: 9002,
            })
            .unwrap();
        socket.close();
        assert!(
            socket
                .bind(IpListenEndpoint {
                    addr: None,
                    port: 9003
                })
                .is_ok()
        );
        let events = udp_readiness(&socket, true);
        assert!(events.contains(IoEvents::OUT));
        assert!(!events.contains(IoEvents::HUP));
    }

    // ── Task 3.2: per-attempt effective terminal and entry ordering ─────

    use core::net::{IpAddr, Ipv4Addr, SocketAddr};

    use crate::{RecvFlags, SendOptions, SocketAddrEx, SocketOps};

    #[test]
    fn fatal_between_attempts_makes_second_attempt_return_stable_error() {
        let socket = UdpSocket::new_with_context(test_ctx());
        SocketOps::bind(
            &socket,
            SocketAddrEx::Ip(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 9061)),
        )
        .unwrap();

        let mut remote = super::ExpectedRemote::Ignore;
        let mut sink = std::vec::Vec::<u8>::new();

        let first = socket.try_recv_once(&mut sink, &mut remote, RecvFlags::empty());
        assert_eq!(first.unwrap_err(), AxError::WouldBlock);

        // A data-plane fault commits between attempt one and attempt two;
        // the same effective-terminal read serves a global publication.
        socket.readiness.commit_terminal(readiness::TERMINAL_IO);

        let second = socket.try_recv_once(&mut sink, &mut remote, RecvFlags::empty());
        assert_eq!(
            second.unwrap_err(),
            AxError::Io,
            "the retry must observe the effective terminal instead of re-Pending on WouldBlock"
        );
    }

    #[test]
    fn send_entry_reports_preexisting_terminal_before_address_work() {
        let socket = UdpSocket::new_with_context(test_ctx()); // unbound
        socket.readiness.commit_terminal(readiness::TERMINAL_IO);

        let err = SocketOps::send(&socket, &b"ab"[..], SendOptions::default()).unwrap_err();
        assert_eq!(err, AxError::Io);
        assert!(
            socket.local_addr.read().is_none(),
            "no implicit bind may happen under a preexisting fatal"
        );
    }

    #[test]
    fn connect_entry_reports_preexisting_terminal_before_peer_commit() {
        let socket = UdpSocket::new_with_context(test_ctx());
        socket
            .readiness
            .commit_terminal(readiness::TERMINAL_CONNECT_REFUSED);

        let err = SocketOps::connect(
            &socket,
            SocketAddrEx::Ip(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)),
                9000,
            )),
        )
        .unwrap_err();
        assert_eq!(err, AxError::ConnectionRefused);
        assert!(
            socket.peer_addr.write().is_none(),
            "peer endpoint must stay unset under a preexisting fatal"
        );
    }

    // ── Task 5.1 (Iteration 006): per-test socket isolation ─────────────

    #[test]
    fn udp_fixtures_bind_the_same_ephemeral_port_and_drop_independently() {
        for _ in 0..10u32 {
            let ctx_a = test_ctx();
            let ctx_b = test_ctx();
            let a = UdpSocket::new_with_context(ctx_a);
            let b = UdpSocket::new_with_context(ctx_b);
            assert_eq!(a.handle, b.handle, "fresh fixtures share the start handle");

            // The same numeric port binds in both independent registries.
            SocketOps::bind(
                &a,
                SocketAddrEx::Ip(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 24242)),
            )
            .unwrap();
            SocketOps::bind(
                &b,
                SocketAddrEx::Ip(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 24242)),
            )
            .unwrap();

            // A's full public drop must not remove B's identical-numeric
            // raw socket.
            drop(a);
            assert!(
                ctx_b
                    .sockets
                    .inner
                    .lock()
                    .iter()
                    .any(|(h, _)| h == b.handle),
                "A dropping handle {} must not remove B's identical handle",
                b.handle,
            );
            drop(b);
            assert!(ctx_a.sockets.inner.lock().iter().next().is_none());
            assert!(ctx_b.sockets.inner.lock().iter().next().is_none());
        }
    }

    // ── Task 5.1 Cycle 001 (rework): fixture-local queued-TX removal ───

    #[test]
    fn udp_queued_tx_drop_enqueues_into_fixture_service() {
        // S2: dropping a fixture UDP socket whose TX buffer still holds one
        // undispatched datagram must enqueue into the fixture's paired
        // Service exactly once and keep the raw handle for the local
        // reaper; the queued datagram is not destroyed. Pre-fix RED: the
        // Drop consults the process-global Service, tears the socket down
        // immediately and the queued datagram is lost.
        let ctx = test_ctx();
        let neighbor = test_ctx();
        let socket = UdpSocket::new_with_context(ctx);
        let neighbor_socket = UdpSocket::new_with_context(neighbor);
        assert_eq!(socket.handle, neighbor_socket.handle);
        let handle = socket.handle;
        socket.with_smol_socket(|s| {
            s.bind(IpListenEndpoint {
                addr: None,
                port: 22300,
            })
            .unwrap();
            s.send_slice(
                b"queued",
                smoltcp::socket::udp::UdpMetadata {
                    endpoint: smoltcp::wire::IpEndpoint::new(
                        smoltcp::wire::Ipv4Address::new(10, 0, 0, 2).into(),
                        21235,
                    ),
                    local_address: Some(smoltcp::wire::Ipv4Address::new(10, 0, 0, 1).into()),
                    meta: Default::default(),
                },
            )
            .unwrap();
        });

        drop(socket);

        assert_eq!(
            ctx.service.lock().deferred_removals_len(),
            1,
            "the queued-TX retirement must land in the fixture's own Service"
        );
        assert!(ctx.sockets.inner.lock().iter().any(|(h, _)| h == handle));
        // The neighbor's equal-numeric socket and queue are untouched.
        assert!(
            neighbor
                .sockets
                .inner
                .lock()
                .iter()
                .any(|(h, _)| h == neighbor_socket.handle)
        );
        assert_eq!(neighbor.service.lock().deferred_removals_len(), 0);
    }

    #[test]
    fn udp_queued_tx_local_drain_reaps_only_the_owning_fixture() {
        // S2 + local drain: the fixture's own round dispatches the queued
        // datagram through the fixture Router (egress to the loopback
        // device), the reaper reclaims the raw handle exactly once, and the
        // neighbor's equal numeric handle is untouched.
        let ctx = test_ctx();
        let neighbor = test_ctx();
        let socket = UdpSocket::new_with_context(ctx);
        let neighbor_socket = UdpSocket::new_with_context(neighbor);
        assert_eq!(socket.handle, neighbor_socket.handle);
        let handle = socket.handle;
        socket.with_smol_socket(|s| {
            s.bind(IpListenEndpoint {
                addr: None,
                port: 22400,
            })
            .unwrap();
            s.send_slice(
                b"queued",
                smoltcp::socket::udp::UdpMetadata {
                    endpoint: smoltcp::wire::IpEndpoint::new(
                        smoltcp::wire::Ipv4Address::new(10, 0, 0, 2).into(),
                        21236,
                    ),
                    local_address: Some(smoltcp::wire::Ipv4Address::new(10, 0, 0, 1).into()),
                    meta: Default::default(),
                },
            )
            .unwrap();
        });

        drop(socket);
        assert_eq!(ctx.service.lock().deferred_removals_len(), 1);

        // Runner lock order: the Service guard first, then the socket set.
        let mut service = ctx.service.lock();
        let mut set = ctx.sockets.inner.lock();
        let _ = service.poll(crate::router::RxOwnerView::PollingOwned, &mut set);
        drop(set);
        drop(service);

        assert_eq!(
            ctx.service.lock().deferred_removals_len(),
            0,
            "the drained datagram retires the deferred entry exactly once"
        );
        assert!(!ctx.sockets.inner.lock().iter().any(|(h, _)| h == handle));
        assert!(
            neighbor
                .sockets
                .inner
                .lock()
                .iter()
                .any(|(h, _)| h == neighbor_socket.handle),
            "the neighbor's identical-numeric raw socket must survive the local drain"
        );
        assert_eq!(neighbor.service.lock().deferred_removals_len(), 0);
    }

    #[test]
    fn udp_queued_tx_drop_routes_service_through_the_socket_context_in_source() {
        // Source guard: the Drop body must never touch the global Service
        // directly - the socket's context resolves the fixture-paired local
        // Service first, and production sockets keep the global fallback
        // inside `deferred_service`.
        let src = include_str!("udp.rs");
        let drop_start = src.find("impl Drop for UdpSocket").unwrap();
        let drop_end = src.find("fn get_ephemeral_port").unwrap();
        let drop_body = &src[drop_start..drop_end];
        assert!(drop_body.contains("self.deferred_service()"));
        assert!(
            !drop_body.contains("crate::SERVICE"),
            "the queued-TX Drop must resolve the Service through the socket's context"
        );

        let helper_start = src.find("fn deferred_service(&self)").unwrap();
        let helper_end = src.find("fn with_smol_socket").unwrap();
        let helper = &src[helper_start..helper_end];
        assert!(
            helper.find("ctx.service").unwrap() < helper.find("crate::SERVICE").unwrap(),
            "the fixture branch must precede the global fallback"
        );
    }
}
