//! K3 GMAC smoltcp polling Device and bounded session (MS11 Iteration 002,
//! task 3.1).
//!
//! This module adapts the accepted raw GMAC data plane (Iterations 000/001)
//! to a real protocol stack: [`K3PollDevice`] implements smoltcp 0.13's
//! token Device over [`Gmac`], and [`K3Session`] drives a static-storage
//! Interface with one UDP and one TCP echo socket under bounded waits.
//!
//! Adjudicated facts this code must keep (D5, 002 Plan Context):
//! - smoltcp's `consume` cannot return an error. Capacity is therefore
//!   reserved before a token exists: `transmit` only hands out a token when
//!   [`Gmac::tx_free_capacity`] reports a free slot, and `receive` withholds
//!   the whole RX/TX pair (retaining the received frame in a CPU stash)
//!   while no TX slot is free. An unconsumed token simply never reserves.
//! - RX frames are copied into a CPU-owned scratch before the token is
//!   created; the device-owned buffer was already refilled by the driver.
//! - Driver errors that `consume` cannot propagate are recorded as a device
//!   fault ([`SessionFault`], stage + owner) that the foreground reads right
//!   after the stack poll and ends the session on — never a fake success.
//! - Every protocol wait is bounded on the platform timebase (5 s per op
//!   phase); the UDP/TCP echo accumulation stops at 1024 bytes with
//!   offset-advanced partial send/recv and never drops a legal owner.
//!
//! The kernel compiles this module only under the `k3-polling-net` feature
//! (implied by `k3`); the `tests/k3-polling-net-host` package enables the
//! same feature so the identical module tree is host-tested.

use smoltcp::{
    iface::{Config, Interface, SocketHandle, SocketSet, SocketStorage},
    phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken},
    socket::{tcp, udp},
    time::Instant as StackInstant,
    wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr},
};

use super::{
    ArenaBacking, Gmac, GmacEnv, GmacError, GmacState, RxPoll, TxPoll, TxSubmitError,
    TX_FRAME_MAX, TX_FRAME_MIN,
};
use super::dma::NUM_SLOTS;

// ---------------------------------------------------------------------------
// Session constants (task 3.1 contract)
// ---------------------------------------------------------------------------

/// Ethernet MTU reported to the stack (IP MTU 1500 + 14 header bytes).
pub const SESSION_MTU: usize = 1514;
/// IP MTU ([`DeviceCapabilities::ip_mtu`] after the Ethernet header).
pub const SESSION_IP_MTU: usize = 1500;
/// One-direction socket buffer size for the UDP and TCP echo sockets.
pub const SESSION_SOCKET_BUF: usize = 4096;
/// Cumulative echo limit per protocol; accumulation stops there.
pub const SESSION_ECHO_LIMIT: usize = 1024;
/// Bounded wait per protocol operation phase (connect-echo/close).
pub const SESSION_OP_BOUND_NANOS: u64 = 5_000_000_000;
/// Board network identity: IPv4 address on the direct PC link.
pub const BOARD_IPV4: [u8; 4] = [192, 168, 137, 2];
/// Board network identity: software source MAC (matches `init_rx`).
pub const BOARD_MAC: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x26, 0x01];
/// UDP echo port (PC client sends `ms11-udp` here).
pub const UDP_ECHO_PORT: u16 = 26001;
/// TCP echo port (PC client sends `ms11-tcp` here).
pub const TCP_ECHO_PORT: u16 = 26002;

/// Slot count for the socket packet metadata rings (datagrams in flight).
const PACKET_META_SLOTS: usize = 4;

// ---------------------------------------------------------------------------
// Device fault recording (consume cannot fail — D5)
// ---------------------------------------------------------------------------

/// Where a recorded fault came from (the owner that must diagnose it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionFaultStage {
    /// Driver RX poll failed inside `receive`.
    DeviceRx,
    /// Driver TX reclaim failed inside a device entry.
    DeviceTxReclaim,
    /// The TX publish inside `TxToken::consume` was refused.
    DeviceTxSubmit,
}

/// The recorded fault content. `Driver` carries the raw [`GmacError`];
/// the submit-stage variants name the refused frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionFaultDetail {
    Driver(GmacError),
    TxLengthInvalid { len: usize },
    TxFull,
}

/// A recorded device fault: the foreground reads this right after a stack
/// poll and ends the session bounded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionFault {
    pub stage: SessionFaultStage,
    pub detail: SessionFaultDetail,
}

/// Factual device counters (observation only — the kernel never judges).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeviceCounters {
    /// RX frames delivered to the stack.
    pub rx_frames: usize,
    /// Classified non-deliverable RX descriptors.
    pub rx_drops: usize,
    /// TX completions reclaimed exactly once.
    pub tx_completed: usize,
    /// Device-reported ES=1 local TX errors.
    pub tx_errors: usize,
    /// 1 s terminal windows that expired with OWN still set.
    pub tx_timeouts: usize,
    /// ICMP echo requests seen at RX (the stack auto-replies one per
    /// request — the board-side reply count).
    pub icmp_echo_replies: usize,
}

/// The polling smoltcp Device over an initialized, TX-enabled [`Gmac`].
///
/// The device owns the environment so the stack can drive it through the
/// `Device` trait; the foreground reads time and faults through the
/// accessors.
pub struct K3PollDevice<B: ArenaBacking, E: GmacEnv> {
    gmac: Gmac<B>,
    env: E,
    fault: Option<SessionFault>,
    /// One retained RX frame awaiting a TX slot for its reply path.
    rx_stash: Option<(usize, [u8; SESSION_MTU])>,
    /// CPU-owned copy target for driver RX polls (never device memory).
    rx_scratch: [u8; SESSION_MTU],
    counters: DeviceCounters,
}

impl<B: ArenaBacking, E: GmacEnv> K3PollDevice<B, E> {
    /// Wraps an initialized driver (after `init_rx` + `enable_tx`).
    pub fn new(gmac: Gmac<B>, env: E) -> Self {
        K3PollDevice {
            gmac,
            env,
            fault: None,
            rx_stash: None,
            rx_scratch: [0; SESSION_MTU],
            counters: DeviceCounters::default(),
        }
    }

    /// Device counters (facts only).
    pub fn counters(&self) -> &DeviceCounters {
        &self.counters
    }

    /// Takes the recorded fault, if any (read right after a stack poll).
    pub fn take_fault(&mut self) -> Option<SessionFault> {
        self.fault.take()
    }

    /// Whether a fault is recorded or the driver itself is faulted (both
    /// make the device refuse new work until the session ends).
    pub fn is_faulted(&self) -> bool {
        self.fault.is_some() || matches!(self.gmac_state(), GmacState::Faulted { .. })
    }

    /// Whether the published backing is still retained (never released).
    pub fn backing_retained(&self) -> bool {
        self.gmac.is_published()
    }

    /// Driver lifecycle state.
    pub fn gmac_state(&self) -> GmacState {
        self.gmac.state()
    }

    /// Free TX slots (the reservation peek behind the token contract).
    pub fn tx_free_capacity(&self) -> usize {
        self.gmac.tx_free_capacity()
    }

    /// Platform time (the session's bounded waits run on this clock).
    pub fn now_nanos(&self) -> u64 {
        self.env.now_nanos()
    }

    /// Bounded idle wait on the platform time source.
    pub fn wait_nanos(&mut self, nanos: u64) {
        self.env.wait_nanos(nanos);
    }

    /// Propagates the foreground cancel request to the driver (the next
    /// checkpoint runs the bounded stop; device-owned slots are preserved).
    pub fn request_cancel(&mut self) {
        self.gmac.request_cancel();
    }

    fn record_fault(&mut self, stage: SessionFaultStage, detail: SessionFaultDetail) {
        if self.fault.is_none() {
            self.fault = Some(SessionFault { stage, detail });
        }
    }

    /// One bounded TX reclaim drain (the poll-entry contract: reclaim
    /// first). Outcomes are counted; errors become device faults.
    fn reclaim_tx(&mut self) {
        for _ in 0..NUM_SLOTS {
            match self.gmac.poll_tx(&mut self.env) {
                Ok(TxPoll::Idle) | Ok(TxPoll::Pending { .. }) => return,
                Ok(TxPoll::Completed { .. }) => self.counters.tx_completed += 1,
                Ok(TxPoll::LocalError { .. }) => self.counters.tx_errors += 1,
                Ok(TxPoll::TerminalTimeout { .. }) => self.counters.tx_timeouts += 1,
                Err(e) => {
                    self.record_fault(SessionFaultStage::DeviceTxReclaim, SessionFaultDetail::Driver(e));
                    return;
                }
            }
        }
    }

    /// Pumps the RX ring into the CPU stash. `Ok(true)` = a frame is
    /// stashed; drops are counted and skipped within one bounded sweep.
    /// Delivery counters move to the token handout (a stashed frame has
    /// not reached the stack yet).
    fn poll_rx_into_stash(&mut self) -> Result<bool, ()> {
        for _ in 0..NUM_SLOTS {
            match self.gmac.poll_rx(&mut self.env, &mut self.rx_scratch) {
                Ok(RxPoll::Idle) => return Ok(false),
                Ok(RxPoll::Frame { len, .. }) => {
                    let mut frame = [0u8; SESSION_MTU];
                    frame[..len].copy_from_slice(&self.rx_scratch[..len]);
                    self.rx_stash = Some((len, frame));
                    return Ok(true);
                }
                Ok(RxPoll::Dropped { .. }) => self.counters.rx_drops += 1,
                Err(e) => {
                    self.record_fault(SessionFaultStage::DeviceRx, SessionFaultDetail::Driver(e));
                    return Err(());
                }
            }
        }
        Ok(false)
    }

    /// ICMP echo request classification for reply counting only (observation,
    /// not a verdict): IPv4 + protocol 1 + ICMP type 8.
    fn is_icmp_echo_request(frame: &[u8]) -> bool {
        if frame.len() < 14 + 20 + 4 {
            return false;
        }
        if frame[12] != 0x08 || frame[13] != 0x00 {
            return false;
        }
        let ihl = (frame[14] & 0x0f) as usize * 4;
        if ihl < 20 || 14 + ihl + 4 > frame.len() {
            return false;
        }
        frame[14 + 9] == 1 && frame[14 + ihl] == 8
    }
}

impl<B: ArenaBacking, E: GmacEnv> Device for K3PollDevice<B, E> {
    type RxToken<'a>
        = K3RxToken
    where
        Self: 'a;
    type TxToken<'a>
        = K3TxToken<'a, B, E>
    where
        Self: 'a;

    fn receive(&mut self, _timestamp: StackInstant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        if self.fault.is_some() {
            return None;
        }
        self.reclaim_tx();
        if self.fault.is_some() {
            return None;
        }
        // Retained frame first (bounded backpressure kept it here).
        if self.rx_stash.is_none() && matches!(self.poll_rx_into_stash(), Ok(false) | Err(())) {
            return None;
        }
        // The pair carries a TX token: one free slot is reserved by the
        // peek below; without it the frame stays stashed (never dropped).
        if self.gmac.tx_free_capacity() == 0 {
            return None;
        }
        let (len, frame) = self.rx_stash.take().expect("stash present");
        self.counters.rx_frames += 1;
        if Self::is_icmp_echo_request(&frame[..len]) {
            self.counters.icmp_echo_replies += 1;
        }
        Some((K3RxToken { frame, len }, K3TxToken { device: self }))
    }

    fn transmit(&mut self, _timestamp: StackInstant) -> Option<Self::TxToken<'_>> {
        if self.fault.is_some() {
            return None;
        }
        self.reclaim_tx();
        if self.fault.is_some() {
            return None;
        }
        if self.gmac.tx_free_capacity() == 0 {
            return None;
        }
        Some(K3TxToken { device: self })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = SESSION_MTU;
        // Checksums stay at the software default (no device offload).
        caps
    }
}

/// RX token owning a CPU copy of the delivered frame (never device memory).
pub struct K3RxToken {
    frame: [u8; SESSION_MTU],
    len: usize,
}

impl RxToken for K3RxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(&self.frame[..self.len])
    }
}

/// TX token over the reserved capacity: `consume` writes the complete frame
/// and publishes it through the accepted submit sequence. An unconsumed
/// token reserved nothing. Driver errors land in the device fault record.
pub struct K3TxToken<'a, B: ArenaBacking, E: GmacEnv> {
    device: &'a mut K3PollDevice<B, E>,
}

impl<B: ArenaBacking, E: GmacEnv> TxToken for K3TxToken<'_, B, E> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut frame = [0u8; SESSION_MTU];
        let write_len = len.min(SESSION_MTU);
        let result = f(&mut frame[..write_len]);
        let stage = SessionFaultStage::DeviceTxSubmit;
        if write_len < TX_FRAME_MIN || write_len > TX_FRAME_MAX {
            self.device.record_fault(stage, SessionFaultDetail::TxLengthInvalid { len: write_len });
            return result;
        }
        match self.device.gmac.try_submit(&mut self.device.env, &frame[..write_len]) {
            Ok(_) => {}
            Err(TxSubmitError::LengthInvalid { len }) => {
                self.device.record_fault(stage, SessionFaultDetail::TxLengthInvalid { len });
            }
            Err(TxSubmitError::Driver(e)) => {
                self.device.record_fault(stage, SessionFaultDetail::Driver(e));
            }
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Bounded session: static sockets, echo limits, bounded waits
// ---------------------------------------------------------------------------

/// TCP echo close lifecycle (factual session observation).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TcpCloseState {
    /// No active connection.
    #[default]
    NotConnected,
    /// Connection active; echo may progress.
    Connected,
    /// Close initiated (either direction); completion bounded.
    Closing,
    /// Close completed (FIN observed in both directions or TimeWait).
    Closed,
}

/// Session-level counters: device facts plus the bounded echo accumulation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SessionCounters {
    pub rx_frames: usize,
    pub rx_drops: usize,
    pub tx_completed: usize,
    pub tx_errors: usize,
    pub tx_timeouts: usize,
    pub icmp_echo_replies: usize,
    /// UDP bytes echoed (cumulative, bounded at [`SESSION_ECHO_LIMIT`]).
    pub udp_echoed: usize,
    /// UDP bytes beyond the limit (counted, never echoed).
    pub udp_over_limit: usize,
    /// TCP bytes echoed (cumulative, bounded at [`SESSION_ECHO_LIMIT`]).
    pub tcp_echoed: usize,
    pub tcp_state: TcpCloseState,
}

/// Static socket storage for one session (no dynamic allocation). `'s` is
/// the storage lifetime the sockets borrow (callers typically use `'static`
/// or a stack local).
pub struct SessionStorage<'s> {
    pub sockets: [SocketStorage<'s>; 2],
    pub udp_rx_meta: [udp::PacketMetadata; PACKET_META_SLOTS],
    pub udp_rx: [u8; SESSION_SOCKET_BUF],
    pub udp_tx_meta: [udp::PacketMetadata; PACKET_META_SLOTS],
    pub udp_tx: [u8; SESSION_SOCKET_BUF],
    pub tcp_rx: [u8; SESSION_SOCKET_BUF],
    pub tcp_tx: [u8; SESSION_SOCKET_BUF],
}

impl<'s> SessionStorage<'s> {
    pub const fn new() -> Self {
        SessionStorage {
            sockets: [SocketStorage::EMPTY; 2],
            udp_rx_meta: [udp::PacketMetadata::EMPTY; PACKET_META_SLOTS],
            udp_rx: [0; SESSION_SOCKET_BUF],
            udp_tx_meta: [udp::PacketMetadata::EMPTY; PACKET_META_SLOTS],
            udp_tx: [0; SESSION_SOCKET_BUF],
            tcp_rx: [0; SESSION_SOCKET_BUF],
            tcp_tx: [0; SESSION_SOCKET_BUF],
        }
    }
}

/// A bounded session error: either a recorded device fault or an expired
/// 5 s protocol wait. Both end the session; the owner is always named.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionError {
    Fault(SessionFault),
    TcpTimeout { stage: &'static str },
}

/// One bounded polling session: the smoltcp Interface plus one UDP and one
/// TCP echo socket over static storage. All waits run on the driver clock
/// and stay within [`SESSION_OP_BOUND_NANOS`]; the echo accumulation stops
/// at [`SESSION_ECHO_LIMIT`] with offset-advanced partial transfers.
pub struct K3Session<'a, B: ArenaBacking, E: GmacEnv> {
    device: K3PollDevice<B, E>,
    iface: Interface,
    sockets: SocketSet<'a>,
    udp: SocketHandle,
    tcp: SocketHandle,
    udp_echoed: usize,
    udp_over_limit: usize,
    tcp_echoed: usize,
    /// Offset-advanced TCP echo staging (CPU-owned, bounded).
    tcp_pending: [u8; SESSION_ECHO_LIMIT],
    tcp_pending_off: usize,
    tcp_pending_len: usize,
    tcp_state: TcpCloseState,
    tcp_last_progress: Option<u64>,
    tcp_close_started: Option<u64>,
    fault: Option<SessionFault>,
}

impl<'a, B: ArenaBacking, E: GmacEnv> K3Session<'a, B, E> {
    /// Builds the Interface (board MAC/IP on the /24 PC link) and the two
    /// echo sockets over the caller's static storage.
    pub fn new<'s>(
        mut device: K3PollDevice<B, E>,
        storage: &'s mut SessionStorage<'s>,
    ) -> K3Session<'s, B, E> {
        let mut config = Config::new(HardwareAddress::Ethernet(
            EthernetAddress::from_bytes(&BOARD_MAC),
        ));
        config.random_seed = 0;
        let now = StackInstant::from_millis((device.now_nanos() / 1_000_000) as i64);
        let mut iface = Interface::new(config, &mut device, now);
        iface.update_ip_addrs(|addrs| {
            addrs
                .push(IpCidr::new(
                    IpAddress::v4(BOARD_IPV4[0], BOARD_IPV4[1], BOARD_IPV4[2], BOARD_IPV4[3]),
                    24,
                ))
                .expect("one interface address");
        });

        // Moving the field references out of `storage` ties every buffer to
        // 'a (the caller's static/stack storage outlives the session).
        let SessionStorage { sockets: socket_area, udp_rx_meta, udp_rx, udp_tx_meta, udp_tx, tcp_rx, tcp_tx } =
            storage;
        let mut sockets = SocketSet::new(&mut socket_area[..]);
        let mut udp_sock = udp::Socket::new(
            udp::PacketBuffer::new(&mut udp_rx_meta[..], &mut udp_rx[..]),
            udp::PacketBuffer::new(&mut udp_tx_meta[..], &mut udp_tx[..]),
        );
        udp_sock.bind(UDP_ECHO_PORT).expect("udp echo bind");
        let udp = sockets.add(udp_sock);
        let mut tcp_sock = tcp::Socket::new(
            tcp::SocketBuffer::new(&mut tcp_rx[..]),
            tcp::SocketBuffer::new(&mut tcp_tx[..]),
        );
        tcp_sock.listen(TCP_ECHO_PORT).expect("tcp echo listen");
        let tcp = sockets.add(tcp_sock);

        K3Session {
            device,
            iface,
            sockets,
            udp,
            tcp,
            udp_echoed: 0,
            udp_over_limit: 0,
            tcp_echoed: 0,
            tcp_pending: [0; SESSION_ECHO_LIMIT],
            tcp_pending_off: 0,
            tcp_pending_len: 0,
            tcp_state: TcpCloseState::NotConnected,
            tcp_last_progress: None,
            tcp_close_started: None,
            fault: None,
        }
    }

    /// One stack poll: ingress, egress and TX reclaim run through the
    /// device; a recorded device fault is harvested immediately.
    pub fn poll_stack(&mut self) {
        let ts = StackInstant::from_millis((self.device.now_nanos() / 1_000_000) as i64);
        let Self { iface, device, sockets, .. } = self;
        iface.poll(ts, device, sockets);
        if self.fault.is_none() {
            self.fault = device.take_fault();
        }
    }

    /// Drains the UDP echo socket within the cumulative limit. A datagram
    /// beyond the budget echoes its fitting prefix; the remainder is counted,
    /// never faked as delivered.
    pub fn service_udp_echo(&mut self) {
        if self.fault.is_some() {
            return;
        }
        let udp_handle = self.udp;
        let Self { sockets, udp_echoed, udp_over_limit, .. } = self;
        let sock = sockets.get_mut::<udp::Socket>(udp_handle);
        loop {
            let (payload, meta) = match sock.recv() {
                Ok(v) => v,
                Err(_) => break,
            };
            let remote = meta.endpoint;
            let plen = payload.len();
            let budget = SESSION_ECHO_LIMIT.saturating_sub(*udp_echoed);
            let n = plen.min(budget);
            // Copy first so the socket can be used mutably for the echo.
            let mut staging = [0u8; SESSION_ECHO_LIMIT];
            staging[..n].copy_from_slice(&payload[..n]);
            if n > 0 {
                match sock.send(n, remote) {
                    Ok(buf) => {
                        buf.copy_from_slice(&staging[..n]);
                        *udp_echoed += n;
                    }
                    // TX buffer full: bounded backpressure, the remainder
                    // stays serviceable on the next call.
                    Err(_) => break,
                }
            }
            if n < plen {
                *udp_over_limit += plen - n;
            }
        }
    }

    /// Services the TCP echo and the close lifecycle with the 5 s bounds.
    /// Echo progress advances through CPU-owned offsets; the board closes
    /// once everything echoed is acknowledged (or the peer half-closed),
    /// and the close completes within its own bound.
    pub fn service_tcp_echo_and_close(&mut self) -> Result<(), SessionError> {
        if let Some(fault) = self.fault {
            return Err(SessionError::Fault(fault));
        }
        let tcp_handle = self.tcp;
        let Self {
            device,
            sockets,
            tcp_echoed,
            tcp_pending,
            tcp_pending_off,
            tcp_pending_len,
            tcp_state,
            tcp_last_progress,
            tcp_close_started,
            ..
        } = self;
        let sock = sockets.get_mut::<tcp::Socket>(tcp_handle);
        let now = device.now_nanos();

        // Connection lifecycle: first activity starts the bounded phase.
        if matches!(*tcp_state, TcpCloseState::NotConnected) && sock.is_active() {
            *tcp_state = TcpCloseState::Connected;
            *tcp_last_progress = Some(now);
        }

        // Receive into the bounded staging (offset accumulation).
        if sock.may_recv() {
            let budget = SESSION_ECHO_LIMIT.saturating_sub(*tcp_echoed);
            let room = budget.min(SESSION_ECHO_LIMIT - *tcp_pending_len);
            let mut advanced = 0;
            while advanced < room {
                let n = match sock.recv_slice(&mut tcp_pending[*tcp_pending_len + advanced..*tcp_pending_len + room]) {
                    Ok(n) => n,
                    Err(tcp::RecvError::Finished) | Err(tcp::RecvError::InvalidState) => break,
                };
                if n == 0 {
                    break;
                }
                advanced += n;
            }
            if advanced > 0 {
                *tcp_pending_len += advanced;
                *tcp_last_progress = Some(now);
            }
        }

        // Send the staged echo (partial sends advance the offset).
        if *tcp_pending_len > *tcp_pending_off && sock.can_send() {
            let sent = sock
                .send_slice(&tcp_pending[*tcp_pending_off..*tcp_pending_len])
                .unwrap_or(0);
            if sent > 0 {
                *tcp_pending_off += sent;
                *tcp_echoed += sent;
                *tcp_last_progress = Some(now);
                if *tcp_pending_off == *tcp_pending_len {
                    *tcp_pending_off = 0;
                    *tcp_pending_len = 0;
                }
            }
        }

        // Close initiation: everything echoed is acknowledged, or the peer
        // half-closed with nothing pending. The close direction is not
        // substantive; completion is what the acceptance observes.
        let pending_drained = *tcp_pending_len == *tcp_pending_off;
        if matches!(*tcp_state, TcpCloseState::Connected)
            && pending_drained
            && (*tcp_echoed > 0 && sock.send_queue() == 0 || sock.state() == tcp::State::CloseWait)
        {
            sock.close();
            *tcp_state = TcpCloseState::Closing;
            *tcp_close_started = Some(now);
        }

        // Close completion within its own 5 s bound.
        if matches!(*tcp_state, TcpCloseState::Closing) {
            if !sock.is_open() {
                *tcp_state = TcpCloseState::Closed;
                *tcp_close_started = None;
                *tcp_last_progress = Some(now);
            } else if now.saturating_sub(tcp_close_started.expect("closing started"))
                > SESSION_OP_BOUND_NANOS
            {
                return Err(SessionError::TcpTimeout { stage: "close" });
            }
        }

        // Bounded echo phase: an active connection with no progress for
        // 5 s ends the session (the operator flow connects and sends).
        if matches!(*tcp_state, TcpCloseState::Connected) {
            if let Some(t0) = *tcp_last_progress {
                if now.saturating_sub(t0) > SESSION_OP_BOUND_NANOS {
                    return Err(SessionError::TcpTimeout { stage: "echo" });
                }
            }
        }
        Ok(())
    }

    /// Aggregated session counters (device facts + echo accumulation).
    pub fn counters(&self) -> SessionCounters {
        let d = self.device.counters();
        SessionCounters {
            rx_frames: d.rx_frames,
            rx_drops: d.rx_drops,
            tx_completed: d.tx_completed,
            tx_errors: d.tx_errors,
            tx_timeouts: d.tx_timeouts,
            icmp_echo_replies: d.icmp_echo_replies,
            udp_echoed: self.udp_echoed,
            udp_over_limit: self.udp_over_limit,
            tcp_echoed: self.tcp_echoed,
            tcp_state: self.tcp_state,
        }
    }

    /// Takes the session fault (device faults end the session bounded).
    pub fn take_fault(&mut self) -> Option<SessionFault> {
        self.fault.take()
    }

    /// Requests cancellation: the next driver checkpoint runs the bounded
    /// stop; the recorded fault keeps the owner named and the backing held.
    pub fn request_cancel(&mut self) {
        self.device.request_cancel();
    }

    /// Whether the published backing is retained (never released).
    pub fn backing_retained(&self) -> bool {
        self.device.backing_retained()
    }

    /// Bounded wait on the driver clock.
    pub fn wait_nanos(&mut self, nanos: u64) {
        self.device.wait_nanos(nanos);
    }

    /// Platform time (the session window bound runs on this clock).
    pub fn now_nanos(&self) -> u64 {
        self.device.now_nanos()
    }
}
