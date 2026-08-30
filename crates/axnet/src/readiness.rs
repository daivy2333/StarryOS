//! Per-public-handle socket readiness bridging.
//!
//! Task 2.1: every public TCP/UDP handle owns exactly one shared
//! [`ReadinessBridge`] (read / write / terminal `PollSet` groups). smoltcp's
//! one-shot single-slot recv/send wakers are pointed at the bridge, which
//! fans out to all registered application waiters. Hidden listener sockets
//! do not enter the public registry.

use alloc::sync::Arc;
use core::{
    sync::atomic::{AtomicU64, Ordering},
    task::Waker,
};

use axdriver::prelude::DevError;
use axerrno::AxError;
use axpoll::{IoEvents, PollSet};

/// No terminal state is committed.
pub(crate) const TERMINAL_NONE: u64 = 0;
/// Stable codes 1..=8 mirror the shared [`DevError`] encoding used by the
/// flush ledger and RX telemetry; 9 is the socket-local connection-refused
/// terminal committed by a failed nonblocking connect.
pub(crate) const TERMINAL_ALREADY_EXISTS: u64 = 1;
pub(crate) const TERMINAL_AGAIN: u64 = 2;
pub(crate) const TERMINAL_BAD_STATE: u64 = 3;
pub(crate) const TERMINAL_INVALID_PARAM: u64 = 4;
pub(crate) const TERMINAL_IO: u64 = 5;
pub(crate) const TERMINAL_NO_MEMORY: u64 = 6;
pub(crate) const TERMINAL_RESOURCE_BUSY: u64 = 7;
pub(crate) const TERMINAL_UNSUPPORTED: u64 = 8;
pub(crate) const TERMINAL_CONNECT_REFUSED: u64 = 9;
pub(crate) const TERMINAL_CONNECTION_RESET: u64 = 10;
pub(crate) const TERMINAL_NOT_CONNECTED: u64 = 11;
pub(crate) const TERMINAL_TIMED_OUT: u64 = 12;
pub(crate) const TERMINAL_INTERRUPTED: u64 = 13;
pub(crate) const TERMINAL_OWNERSHIP_FAULT: u64 = 14;
pub(crate) const TERMINAL_DEVICE_IO: u64 = 15;

/// Stable application-facing terminal identity for one SocketEpoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NetworkTerminal {
    ConnectionReset,
    LinkDown,
    Deadline,
    Cancelled,
    OwnershipFault,
    DeviceIo,
}

impl NetworkTerminal {
    pub(crate) const fn code(self) -> u64 {
        match self {
            Self::ConnectionReset => TERMINAL_CONNECTION_RESET,
            Self::LinkDown => TERMINAL_NOT_CONNECTED,
            Self::Deadline => TERMINAL_TIMED_OUT,
            Self::Cancelled => TERMINAL_INTERRUPTED,
            Self::OwnershipFault => TERMINAL_OWNERSHIP_FAULT,
            Self::DeviceIo => TERMINAL_DEVICE_IO,
        }
    }

    pub(crate) const fn ax_error(self) -> AxError {
        match self {
            Self::ConnectionReset => AxError::ConnectionReset,
            Self::LinkDown => AxError::NotConnected,
            Self::Deadline => AxError::TimedOut,
            Self::Cancelled => AxError::Interrupted,
            Self::OwnershipFault => AxError::BadState,
            Self::DeviceIo => AxError::Io,
        }
    }

    pub(crate) const fn from_code(code: u64) -> Option<Self> {
        match code {
            TERMINAL_CONNECTION_RESET => Some(Self::ConnectionReset),
            TERMINAL_NOT_CONNECTED => Some(Self::LinkDown),
            TERMINAL_TIMED_OUT => Some(Self::Deadline),
            TERMINAL_INTERRUPTED => Some(Self::Cancelled),
            TERMINAL_OWNERSHIP_FAULT => Some(Self::OwnershipFault),
            TERMINAL_DEVICE_IO => Some(Self::DeviceIo),
            _ => None,
        }
    }

    pub(crate) const fn from_legacy_code(code: u64) -> Self {
        match code {
            TERMINAL_BAD_STATE => Self::OwnershipFault,
            _ => Self::DeviceIo,
        }
    }
}

/// Encodes a concrete [`DevError`] into its stable terminal code. The code is
/// the identity carried across publication; it survives where a `DevError`
/// value cannot (move-only type, atomic storage).
pub(crate) fn dev_error_code(err: &DevError) -> u64 {
    match err {
        DevError::AlreadyExists => TERMINAL_ALREADY_EXISTS,
        DevError::Again => TERMINAL_AGAIN,
        DevError::BadState => TERMINAL_BAD_STATE,
        DevError::InvalidParam => TERMINAL_INVALID_PARAM,
        DevError::Io => TERMINAL_IO,
        DevError::NoMemory => TERMINAL_NO_MEMORY,
        DevError::ResourceBusy => TERMINAL_RESOURCE_BUSY,
        DevError::Unsupported => TERMINAL_UNSUPPORTED,
    }
}

/// Reconstructs a [`DevError`] from [`dev_error_code`].
pub(crate) fn dev_error_from_code(code: u64) -> DevError {
    match code {
        TERMINAL_ALREADY_EXISTS => DevError::AlreadyExists,
        TERMINAL_AGAIN => DevError::Again,
        TERMINAL_BAD_STATE => DevError::BadState,
        TERMINAL_INVALID_PARAM => DevError::InvalidParam,
        TERMINAL_IO => DevError::Io,
        TERMINAL_NO_MEMORY => DevError::NoMemory,
        TERMINAL_RESOURCE_BUSY => DevError::ResourceBusy,
        _ => DevError::Unsupported,
    }
}

/// Maps one stable terminal code to the public error category. A committed
/// fatal `Again` maps to `Io`, never to `WouldBlock`: committed terminal
/// state is not retryable backpressure.
pub(crate) fn terminal_ax_error(code: u64) -> AxError {
    match code {
        TERMINAL_ALREADY_EXISTS => AxError::AlreadyExists,
        TERMINAL_AGAIN => AxError::Io,
        TERMINAL_BAD_STATE => AxError::BadState,
        TERMINAL_INVALID_PARAM => AxError::InvalidInput,
        TERMINAL_IO => AxError::Io,
        TERMINAL_NO_MEMORY => AxError::NoMemory,
        TERMINAL_RESOURCE_BUSY => AxError::ResourceBusy,
        TERMINAL_UNSUPPORTED => AxError::Unsupported,
        TERMINAL_CONNECT_REFUSED => AxError::ConnectionRefused,
        TERMINAL_CONNECTION_RESET => AxError::ConnectionReset,
        TERMINAL_NOT_CONNECTED => AxError::NotConnected,
        TERMINAL_TIMED_OUT => AxError::TimedOut,
        TERMINAL_INTERRUPTED => AxError::Interrupted,
        TERMINAL_OWNERSHIP_FAULT => AxError::BadState,
        TERMINAL_DEVICE_IO => AxError::Io,
        _ => AxError::Unsupported,
    }
}

/// Resolves the effective terminal category: the global data-plane fault
/// takes precedence over any socket-local terminal error.
pub(crate) fn effective_terminal_code(global: u64, local: u64) -> u64 {
    if global != TERMINAL_NONE {
        global
    } else {
        local
    }
}

/// A smoltcp socket that can rearm its one-shot recv/send slots for the
/// bridge fan-out (Task 2.2).
pub(crate) trait OneShotSocket {
    /// Rearms the recv slot; returns whether reads are currently ready.
    fn rearm_read(&mut self, waker: &Waker) -> bool;
    /// Rearms the send slot; returns whether writes are currently ready.
    fn rearm_write(&mut self, waker: &Waker) -> bool;
}

/// smoltcp transitions that can affect read readiness or terminal state ride
/// the single recv slot; write-side only rides the send slot.
const SLOT_READ_INTEREST: IoEvents = IoEvents::IN
    .union(IoEvents::RDHUP)
    .union(IoEvents::HUP)
    .union(IoEvents::ERR);

/// Wakes a direction's waiters plus every terminal waiter (Task 2.2): one
/// smoltcp one-shot slot reaches both its own direction and the terminal
/// set, so a terminal-only waiter can never be left asleep by a peer
/// transition.
struct DirectionNotify {
    direction: Arc<PollSet>,
    terminal: Arc<PollSet>,
}

impl alloc::task::Wake for DirectionNotify {
    fn wake(self: Arc<Self>) {
        self.direction.as_ref().wake();
        self.terminal.as_ref().wake();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.direction.as_ref().wake();
        self.terminal.as_ref().wake();
    }
}

// SAFETY: both members are internally synchronized `PollSet`s; the wake
// fan-out touches only those synchronous sets.
unsafe impl Send for DirectionNotify {}
unsafe impl Sync for DirectionNotify {}

/// Fan-out target of the smoltcp one-shot waker slots. The terminals live in
/// a separate set so a terminal waiter never steals a read/write slot.
pub(crate) struct ReadinessBridge {
    read: Arc<PollSet>,
    write: Arc<PollSet>,
    terminal: Arc<PollSet>,
    recv_notify: Arc<DirectionNotify>,
    send_notify: Arc<DirectionNotify>,
    /// Task 3.1: first-wins stable terminal code (`TERMINAL_NONE` = none).
    /// Committed strictly before any wake that publishes it.
    terminal_code: AtomicU64,
    /// Task 3.2: first-wins terminal owned by this bridge's SocketEpoch.
    network_terminal_code: AtomicU64,
}

// SAFETY: every member is an internally synchronized `PollSet`; the Arc
// wrapper preserves the same analysis the existing `TcpSocket` applies to a
// plain `PollSet` field (`unsafe impl Sync`).
unsafe impl Sync for ReadinessBridge {}

impl ReadinessBridge {
    pub(crate) fn new() -> Self {
        let read = Arc::new(PollSet::new());
        let write = Arc::new(PollSet::new());
        let terminal = Arc::new(PollSet::new());
        Self {
            recv_notify: Arc::new(DirectionNotify {
                direction: read.clone(),
                terminal: terminal.clone(),
            }),
            send_notify: Arc::new(DirectionNotify {
                direction: write.clone(),
                terminal: terminal.clone(),
            }),
            read,
            write,
            terminal,
            terminal_code: AtomicU64::new(TERMINAL_NONE),
            network_terminal_code: AtomicU64::new(TERMINAL_NONE),
        }
    }

    /// Returns the committed stable terminal code, if any.
    pub(crate) fn terminal_code(&self) -> u64 {
        self.terminal_code.load(Ordering::Acquire)
    }

    pub(crate) fn network_terminal_code(&self) -> u64 {
        self.network_terminal_code.load(Ordering::Acquire)
    }

    /// SocketEpoch terminal takes precedence over a socket-local connect
    /// failure and cannot be cleared by a later epoch opening.
    pub(crate) fn effective_terminal_code(&self) -> u64 {
        let network = self.network_terminal_code();
        if network != TERMINAL_NONE {
            network
        } else {
            self.terminal_code()
        }
    }

    pub(crate) fn commit_network_terminal(&self, terminal: NetworkTerminal) -> bool {
        self.network_terminal_code
            .compare_exchange(
                TERMINAL_NONE,
                terminal.code(),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    /// Commits the stable terminal code first-wins; returns whether this
    /// call performed the commit. The identity is immutable once observable.
    /// Callers publish the commit through a later wake while holding no
    /// Service / SocketSet / registry / listener guard.
    pub(crate) fn commit_terminal(&self, code: u64) -> bool {
        debug_assert_ne!(code, TERMINAL_NONE);
        self.terminal_code
            .compare_exchange(TERMINAL_NONE, code, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    /// Wakes every direction set for a fault already committed in the
    /// wrapper's global terminal. The socket-local code is left untouched;
    /// waiters recheck and observe the effective snapshot (global first).
    /// Callers must hold no Service / SocketSet / registry / listener guard.
    pub(crate) fn wake_for_global_publication(&self) {
        self.wake(IoEvents::IN | IoEvents::OUT | IoEvents::RDHUP | IoEvents::HUP | IoEvents::ERR);
    }

    pub(crate) fn register(&self, interest: IoEvents, waker: &Waker) {
        if interest.intersects(IoEvents::IN) {
            self.read.register(waker);
        }
        if interest.intersects(IoEvents::OUT) {
            self.write.register(waker);
        }
        if interest.intersects(IoEvents::RDHUP | IoEvents::HUP | IoEvents::ERR) {
            self.terminal.register(waker);
        }
    }

    pub(crate) fn recv_waker(&self) -> Waker {
        Waker::from(self.recv_notify.clone())
    }

    pub(crate) fn send_waker(&self) -> Waker {
        Waker::from(self.send_notify.clone())
    }

    /// Rearms smoltcp's one-shot slots for `interest` and rechecks current
    /// readiness under the caller's SocketSet guard. Caller wakes the
    /// returned overlay after the guard drops.
    pub(crate) fn rearm<S: OneShotSocket>(&self, socket: &mut S, interest: IoEvents) -> IoEvents {
        let mut ready = IoEvents::empty();
        if interest.intersects(SLOT_READ_INTEREST) && socket.rearm_read(&self.recv_waker()) {
            ready.insert(IoEvents::IN);
        }
        if interest.intersects(IoEvents::OUT) && socket.rearm_write(&self.send_waker()) {
            ready.insert(IoEvents::OUT);
        }
        ready
    }

    /// Must only be called after all Service / SocketSet / ListenTable /
    /// registry guards are released.
    pub(crate) fn wake(&self, events: IoEvents) {
        if events.intersects(IoEvents::IN) {
            self.read.wake();
        }
        if events.intersects(IoEvents::OUT) {
            self.write.wake();
        }
        if events.intersects(IoEvents::RDHUP | IoEvents::HUP | IoEvents::ERR) {
            self.terminal.wake();
        }
    }
}

impl Default for ReadinessBridge {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use alloc::{sync::Arc, vec::Vec};
    use core::{
        sync::atomic::{AtomicU64, AtomicUsize, Ordering},
        task::Waker,
    };

    use axpoll::{IoEvents, PollSet};

    use super::{ReadinessBridge, SLOT_READ_INTEREST};
    use crate::{tcp::new_tcp_socket, udp::new_udp_socket};

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

    fn counting_waker(count: Arc<AtomicUsize>) -> Waker {
        Waker::from(Arc::new(CountWake(count)))
    }

    /// Distinct wakers for capacity tests: each has its own counter so the
    /// replaced `PollSet` slot can be told apart by witness identity.
    fn distinct_wakers(n: usize) -> Vec<(Arc<AtomicUsize>, Waker)> {
        (0..n)
            .map(|_| {
                let count = Arc::new(AtomicUsize::new(0));
                let waker = counting_waker(count.clone());
                (count, waker)
            })
            .collect()
    }

    #[test]
    fn wake_fires_only_matching_direction_sets() {
        let bridge = ReadinessBridge::new();
        let read_count = Arc::new(AtomicUsize::new(0));
        let write_count = Arc::new(AtomicUsize::new(0));
        let terminal_count = Arc::new(AtomicUsize::new(0));
        bridge.register(IoEvents::IN, &counting_waker(read_count.clone()));
        bridge.register(IoEvents::OUT, &counting_waker(write_count.clone()));
        bridge.register(IoEvents::RDHUP, &counting_waker(terminal_count.clone()));

        bridge.wake(IoEvents::OUT);

        assert_eq!(read_count.load(Ordering::Relaxed), 0);
        assert_eq!(write_count.load(Ordering::Relaxed), 1);
        assert_eq!(terminal_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn recv_and_send_wakers_fan_out_to_their_sets() {
        let bridge = ReadinessBridge::new();
        let read_count = Arc::new(AtomicUsize::new(0));
        let write_count = Arc::new(AtomicUsize::new(0));
        bridge.register(IoEvents::IN, &counting_waker(read_count.clone()));
        bridge.register(IoEvents::OUT, &counting_waker(write_count.clone()));

        bridge.recv_waker().wake();
        bridge.send_waker().wake();

        assert_eq!(read_count.load(Ordering::Relaxed), 1);
        assert_eq!(write_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn one_direction_registration_does_not_disturb_other_direction() {
        let bridge = ReadinessBridge::new();
        let write_count = Arc::new(AtomicUsize::new(0));
        bridge.register(IoEvents::OUT, &counting_waker(write_count.clone()));
        let read_count = Arc::new(AtomicUsize::new(0));
        bridge.register(IoEvents::IN, &counting_waker(read_count.clone()));

        bridge.read.wake();

        assert_eq!(read_count.load(Ordering::Relaxed), 1);
        assert_eq!(write_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn registry_dropped_the_unconsumed_socket_creation_event() {
        let listener = concat!("event_", "listener");
        let new_socket = concat!("new_", "socket");
        assert!(!include_str!("wrapper.rs").contains(listener));
        assert!(!include_str!("wrapper.rs").contains(new_socket));
    }

    #[test]
    fn pollset_capacity_replaces_and_wakes_on_the_65th_distinct_waker() {
        let set = PollSet::new();
        let waiters = distinct_wakers(65);

        // First 64 register cleanly.
        for (_, waker) in waiters.iter().take(64) {
            set.register(waker);
        }

        // The 65th distinct waker replaces slot 0 and wakes the old holder.
        let (replaced_count, _) = waiters[0].clone();
        let (last_count, last_waker) = waiters[64].clone();
        set.register(&last_waker);
        assert_eq!(replaced_count.load(Ordering::Relaxed), 1);

        // A wake now hits the 64 currently resident (65th + the other 63).
        assert_eq!(set.wake(), 64);
        assert_eq!(last_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn repeated_register_adds_another_recheck_opportunity() {
        let bridge = ReadinessBridge::new();
        let count = Arc::new(AtomicUsize::new(0));
        let waker = counting_waker(count.clone());
        bridge.register(IoEvents::IN, &waker);
        bridge.register(IoEvents::IN, &waker);

        bridge.read.wake();
        // Each registration is an independent one-shot recheck hint; axpoll
        // never drops the earlier slot on a same-waker re-register.
        assert_eq!(count.load(Ordering::Relaxed), 2);
    }

    // ── Task 2.2: smoltcp one-shot slot bridging ────────────────────────

    #[test]
    fn read_transition_fans_out_to_all_registered_read_waiters() {
        for count in [1usize, 2, 64, 65] {
            let bridge = ReadinessBridge::new();
            let waiters = distinct_wakers(count);
            for (_, waker) in &waiters {
                bridge.register(IoEvents::IN, waker);
            }
            let mut socket = new_tcp_socket();
            assert!(
                !bridge
                    .rearm(&mut socket, IoEvents::IN)
                    .contains(IoEvents::IN)
            );

            socket.abort();

            for (counter, _) in &waiters {
                assert_eq!(counter.load(Ordering::Relaxed), 1, "count={count}");
            }
        }
    }

    #[test]
    fn ready_before_register_wakes_direction_immediately() {
        // A fresh UDP socket is already writable (`can_send` = buffer not
        // full), so the OUT rearm reports ready and wakes at once.
        let bridge = ReadinessBridge::new();
        let count = Arc::new(AtomicUsize::new(0));
        bridge.register(IoEvents::OUT, &counting_waker(count.clone()));
        let mut socket = new_udp_socket();

        let ready = bridge.rearm(&mut socket, IoEvents::OUT);
        bridge.wake(ready);

        assert!(ready.contains(IoEvents::OUT));
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn read_register_when_not_ready_sleeps_until_transition() {
        let bridge = ReadinessBridge::new();
        let count = Arc::new(AtomicUsize::new(0));
        bridge.register(IoEvents::IN, &counting_waker(count.clone()));
        let mut socket = new_tcp_socket();

        let ready = bridge.rearm(&mut socket, IoEvents::IN);
        bridge.wake(ready);

        assert!(!ready.contains(IoEvents::IN));
        assert_eq!(count.load(Ordering::Relaxed), 0);

        socket.abort();
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn terminal_waiter_rides_recv_slot_and_receives_transition_wake() {
        // HUP rides the recv slot (SLOT_READ_INTEREST) and wakes the terminal
        // set through DirectionNotify, so a HUP-only waiter never starves.
        let bridge = ReadinessBridge::new();
        let count = Arc::new(AtomicUsize::new(0));
        bridge.register(IoEvents::HUP, &counting_waker(count.clone()));
        let mut socket = new_tcp_socket();

        assert!(SLOT_READ_INTEREST.contains(IoEvents::HUP));
        let ready = bridge.rearm(&mut socket, IoEvents::HUP);
        bridge.wake(ready);
        assert!(ready.is_empty());
        assert_eq!(count.load(Ordering::Relaxed), 0);

        socket.abort();
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn oneshot_slot_clears_after_wake_and_rearm_restores() {
        let bridge = ReadinessBridge::new();
        let first = Arc::new(AtomicUsize::new(0));
        let second = Arc::new(AtomicUsize::new(0));
        let mut socket = new_tcp_socket();

        bridge.register(IoEvents::IN, &counting_waker(first.clone()));
        bridge.rearm(&mut socket, IoEvents::IN);
        socket.abort();
        assert_eq!(first.load(Ordering::Relaxed), 1);

        bridge.register(IoEvents::IN, &counting_waker(second.clone()));
        socket.abort();
        assert_eq!(second.load(Ordering::Relaxed), 0);

        bridge.rearm(&mut socket, IoEvents::IN);
        socket.abort();
        assert_eq!(second.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn terminal_waiter_registration_never_occupies_read_slot() {
        let bridge = ReadinessBridge::new();
        let read_waiters = distinct_wakers(64);
        for (_, waker) in &read_waiters {
            bridge.register(IoEvents::IN, waker);
        }
        let terminal = Arc::new(AtomicUsize::new(0));
        bridge.register(IoEvents::RDHUP, &counting_waker(terminal.clone()));
        let mut socket = new_tcp_socket();

        bridge.rearm(&mut socket, IoEvents::IN | IoEvents::RDHUP);
        socket.abort();

        for (counter, _) in &read_waiters {
            assert_eq!(counter.load(Ordering::Relaxed), 1);
        }
        assert_eq!(terminal.load(Ordering::Relaxed), 1);
    }

    // ── Task 3.1: stable terminal encoding, mapping and commit ordering ──

    use axdriver::prelude::DevError;
    use axerrno::AxError;

    #[test]
    fn dev_error_encoding_roundtrips_every_variant() {
        // One stable encoding shared by flush and the queue owner: every
        // DevError variant has a distinct nonzero code that reconstructs
        // exactly the same variant.
        let variants = [
            DevError::AlreadyExists,
            DevError::Again,
            DevError::BadState,
            DevError::InvalidParam,
            DevError::Io,
            DevError::NoMemory,
            DevError::ResourceBusy,
            DevError::Unsupported,
        ];
        let mut seen = alloc::collections::BTreeSet::new();
        for err in variants {
            let code = super::dev_error_code(&err);
            assert_ne!(code, super::TERMINAL_NONE, "{err:?} must encode nonzero");
            assert!(seen.insert(code), "duplicate code {code} for {err:?}");
            match (super::dev_error_from_code(code), err) {
                (a, b) if core::mem::discriminant(&a) == core::mem::discriminant(&b) => {}
                (a, b) => panic!("code {code} reconstructed {a:?}, expected {b:?}"),
            }
        }
    }

    #[test]
    fn terminal_ax_error_maps_every_stable_code() {
        // D3 table: fatal `Again` maps to Io (never WouldBlock backpressure),
        // InvalidParam maps to InvalidInput; the connection-refused
        // socket-local terminal keeps its own category.
        let cases: &[(u64, AxError)] = &[
            (super::TERMINAL_ALREADY_EXISTS, AxError::AlreadyExists),
            (super::TERMINAL_AGAIN, AxError::Io),
            (super::TERMINAL_BAD_STATE, AxError::BadState),
            (super::TERMINAL_INVALID_PARAM, AxError::InvalidInput),
            (super::TERMINAL_IO, AxError::Io),
            (super::TERMINAL_NO_MEMORY, AxError::NoMemory),
            (super::TERMINAL_RESOURCE_BUSY, AxError::ResourceBusy),
            (super::TERMINAL_UNSUPPORTED, AxError::Unsupported),
            (super::TERMINAL_CONNECT_REFUSED, AxError::ConnectionRefused),
        ];
        for (code, expected) in cases {
            let mapped = super::terminal_ax_error(*code);
            assert!(
                mapped == *expected,
                "code {code} mapped to {mapped:?}, expected {expected:?}"
            );
        }
    }

    #[test]
    fn terminal_commit_is_first_wins() {
        let bridge = ReadinessBridge::new();
        assert_eq!(bridge.terminal_code(), super::TERMINAL_NONE);

        assert!(bridge.commit_terminal(super::TERMINAL_IO));
        assert_eq!(bridge.terminal_code(), super::TERMINAL_IO);
        assert!(!bridge.commit_terminal(super::TERMINAL_BAD_STATE));
        assert_eq!(bridge.terminal_code(), super::TERMINAL_IO);
    }

    #[test]
    fn wake_callback_observes_committed_code() {
        static OBSERVED: AtomicU64 = AtomicU64::new(0);
        struct AssertingWake;
        impl alloc::task::Wake for AssertingWake {
            fn wake(self: Arc<Self>) {
                OBSERVED.store(BRIDGE.terminal_code(), Ordering::SeqCst);
            }
            fn wake_by_ref(self: &Arc<Self>) {
                OBSERVED.store(BRIDGE.terminal_code(), Ordering::SeqCst);
            }
        }
        static BRIDGE: spin::Lazy<ReadinessBridge> = spin::Lazy::new(ReadinessBridge::new);

        BRIDGE.register(IoEvents::ERR, &Waker::from(Arc::new(AssertingWake)));

        assert!(BRIDGE.commit_terminal(super::TERMINAL_BAD_STATE));
        BRIDGE.wake_for_global_publication();

        assert_eq!(OBSERVED.load(Ordering::SeqCst), super::TERMINAL_BAD_STATE);
        assert_eq!(BRIDGE.terminal_code(), super::TERMINAL_BAD_STATE);
    }

    #[test]
    fn global_fault_takes_precedence_over_socket_local_terminal() {
        assert_eq!(
            super::effective_terminal_code(super::TERMINAL_IO, super::TERMINAL_CONNECT_REFUSED),
            super::TERMINAL_IO
        );
        assert_eq!(
            super::effective_terminal_code(super::TERMINAL_NONE, super::TERMINAL_CONNECT_REFUSED),
            super::TERMINAL_CONNECT_REFUSED
        );
        assert_eq!(
            super::effective_terminal_code(super::TERMINAL_NONE, super::TERMINAL_NONE),
            super::TERMINAL_NONE
        );
    }

    #[test]
    fn network_terminal_maps_each_recovery_category() {
        use super::NetworkTerminal;

        let cases = [
            (NetworkTerminal::ConnectionReset, AxError::ConnectionReset),
            (NetworkTerminal::LinkDown, AxError::NotConnected),
            (NetworkTerminal::Deadline, AxError::TimedOut),
            (NetworkTerminal::Cancelled, AxError::Interrupted),
            (NetworkTerminal::OwnershipFault, AxError::BadState),
            (NetworkTerminal::DeviceIo, AxError::Io),
        ];
        for (terminal, expected) in cases {
            assert_eq!(terminal.ax_error(), expected);
            assert_eq!(NetworkTerminal::from_code(terminal.code()), Some(terminal));
        }
    }
}
