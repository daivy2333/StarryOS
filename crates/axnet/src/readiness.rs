//! Per-public-handle socket readiness bridging.
//!
//! Task 2.1: every public TCP/UDP handle owns exactly one shared
//! [`ReadinessBridge`] (read / write / terminal `PollSet` groups). smoltcp's
//! one-shot single-slot recv/send wakers are pointed at the bridge, which
//! fans out to all registered application waiters. Hidden listener sockets
//! do not enter the public registry.

use alloc::sync::Arc;
use core::task::Waker;

use axpoll::{IoEvents, PollSet};

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
        }
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
        sync::atomic::{AtomicUsize, Ordering},
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
}
