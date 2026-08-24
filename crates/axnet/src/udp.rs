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
    readiness::ReadinessBridge,
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
        }
    }

    fn with_smol_socket<R>(&self, f: impl FnOnce(&mut smol::Socket) -> R) -> R {
        SOCKET_SET.with_socket_mut::<smol::Socket, _, _>(self.handle, f)
    }

    fn remote_endpoint(&self) -> AxResult<(IpEndpoint, IpAddress)> {
        match self.peer_addr.try_read() {
            Some(addr) => addr.ok_or(AxError::NotConnected),
            None => Err(AxError::NotConnected),
        }
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
            SOCKET_SET.bind_check(local_endpoint.addr, local_endpoint.port)?;
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
            let result = self.with_smol_socket(|socket| {
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
            });
            if result.is_ok() {
                crate::stack_runner::publish_software_work();
            }
            result
        })
    }

    fn recv(&self, mut dst: impl Write, options: RecvOptions) -> AxResult<usize> {
        if self.local_addr.read().is_none() {
            ax_bail!(NotConnected);
        }

        enum ExpectedRemote<'a> {
            Any(&'a mut SocketAddrEx),
            Expecting(IpEndpoint),
            Ignore,
        }
        let mut expected_remote = match options.from {
            Some(addr) => ExpectedRemote::Any(addr),
            None => match *self.peer_addr.read() {
                Some((endpoint, _)) => ExpectedRemote::Expecting(endpoint),
                None => ExpectedRemote::Ignore,
            },
        };

        self.general.recv_poller(self, || {
            let result = self.with_smol_socket(|socket| {
                if !socket.is_open() {
                    // not bound
                    Err(ax_err_type!(NotConnected))
                } else if !socket.can_recv() {
                    info!("UDP socket {}: recv recheck WouldBlock", self.handle);
                    Err(AxError::WouldBlock)
                } else {
                    let result = if options.flags.contains(RecvFlags::PEEK) {
                        socket.peek().map(|(data, meta)| (data, *meta))
                    } else {
                        socket.recv()
                    };
                    match result {
                        Ok((src, meta)) => {
                            match &mut expected_remote {
                                ExpectedRemote::Any(remote_addr) => {
                                    **remote_addr = SocketAddrEx::Ip(meta.endpoint.into());
                                }
                                ExpectedRemote::Expecting(expected) => {
                                    if (!expected.addr.is_unspecified()
                                        && expected.addr != meta.endpoint.addr)
                                        || (expected.port != 0
                                            && expected.port != meta.endpoint.port)
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

                            Ok(if options.flags.contains(RecvFlags::TRUNCATE) {
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
            });
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
        self.with_smol_socket(|socket| udp_readiness(socket, bound))
    }

    fn register(&self, context: &mut Context<'_>, events: IoEvents) {
        self.readiness.register(events, context.waker());
        let bound = self.local_addr.read().is_some();
        let ready = self.with_smol_socket(|socket| {
            let slot_ready = self.readiness.rearm(socket, events);
            let full = udp_readiness(socket, bound);
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
        let has_queued_tx = {
            let sockets = crate::SOCKET_SET.inner.lock();
            sockets.get::<smol::Socket>(self.handle).can_send()
        };
        if has_queued_tx {
            if let Some(service) = crate::SERVICE.get() {
                crate::SOCKET_SET.retire_public(self.handle);
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
        SOCKET_SET.remove(self.handle);
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

    #[test]
    fn closed_socket_reports_hup_and_no_io() {
        let mut socket = new_udp_socket();
        socket
            .bind(IpListenEndpoint {
                addr: None,
                port: 9001,
            })
            .unwrap();
        socket.close();
        let events = udp_readiness(&socket, true);

        assert!(events.contains(IoEvents::HUP));
        assert!(!events.contains(IoEvents::IN));
        assert!(!events.contains(IoEvents::OUT));
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
}
