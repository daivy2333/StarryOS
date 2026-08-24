use alloc::{boxed::Box, collections::VecDeque, sync::Arc, vec::Vec};

use axerrno::{AxError, AxResult};
use axpoll::IoEvents;
use axsync::Mutex;
use smoltcp::{
    iface::{SocketHandle, SocketSet},
    socket::tcp::{Socket, State},
    wire::IpListenEndpoint,
};

use crate::{
    SOCKET_SET, consts::LISTEN_QUEUE_SIZE, readiness::ReadinessBridge, tcp::new_tcp_socket,
};

const PORT_NUM: usize = 65536;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SlotState {
    Pending,
    Ready,
    Reset,
}

struct ListenSlot {
    handle: Option<SocketHandle>,
    state: SlotState,
}

struct ListenTableEntryInner {
    listen_endpoint: IpListenEndpoint,
    accept: Arc<ReadinessBridge>,
    idle: Option<SocketHandle>,
    queue: VecDeque<ListenSlot>,
}

impl ListenTableEntryInner {
    fn new(
        listen_endpoint: IpListenEndpoint,
        accept: Arc<ReadinessBridge>,
        sockets: &mut SocketSet<'_>,
    ) -> Self {
        let mut entry = Self {
            listen_endpoint,
            accept,
            idle: None,
            queue: VecDeque::with_capacity(LISTEN_QUEUE_SIZE),
        };
        entry.refill(sockets);
        entry
    }

    fn refill(&mut self, sockets: &mut SocketSet<'_>) {
        if self.idle.is_some() || self.queue.len() >= LISTEN_QUEUE_SIZE {
            if self.idle.is_none() && self.queue.len() >= LISTEN_QUEUE_SIZE {
                info!(
                    "listen {}: refill blocked (queue full, no idle)",
                    self.listen_endpoint.port
                );
            }
            return;
        }
        let mut socket = new_tcp_socket();
        socket
            .listen(self.listen_endpoint)
            .expect("validated nonzero TCP listen endpoint");
        let handle = sockets.add(socket);
        // Task 2.3: the hidden socket's one-shot recv slot carries the public
        // accept bridge, so any hidden state transition directly reaches the
        // accept waiters.
        sockets
            .get_mut::<Socket>(handle)
            .register_recv_waker(&self.accept.recv_waker());
        self.idle = Some(handle);
        info!(
            "listen {}: refilled idle hidden socket {}",
            self.listen_endpoint.port, handle
        );
    }

    /// Returns whether any hidden socket changed state this round; the caller
    /// stages an accept-bridge wake for the port after its network guards release.
    fn reconcile(&mut self, sockets: &mut SocketSet<'_>) -> bool {
        let mut changed = false;
        for slot in &mut self.queue {
            if slot.state != SlotState::Pending {
                continue;
            }
            let handle = slot.handle.expect("pending listener slot without handle");
            slot.state = match sockets.get::<Socket>(handle).state() {
                State::Listen | State::SynReceived => SlotState::Pending,
                State::Closed => {
                    sockets.remove(handle);
                    slot.handle = None;
                    changed = true;
                    info!(
                        "listen {}: slot {} aborted -> Reset",
                        self.listen_endpoint.port, handle
                    );
                    SlotState::Reset
                }
                _ => {
                    changed = true;
                    info!(
                        "listen {}: slot {} -> Ready",
                        self.listen_endpoint.port, handle
                    );
                    SlotState::Ready
                }
            };
        }

        if let Some(handle) = self.idle {
            let state = sockets.get::<Socket>(handle).state();
            if state != State::Listen {
                self.idle = None;
                changed = true;
                info!(
                    "listen {}: idle {} -> {:?}",
                    self.listen_endpoint.port, handle, state
                );
                let slot_state = match state {
                    State::Closed => {
                        sockets.remove(handle);
                        SlotState::Reset
                    }
                    State::Listen => unreachable!(),
                    State::SynReceived => SlotState::Pending,
                    _ => SlotState::Ready,
                };
                self.queue.push_back(ListenSlot {
                    handle: (slot_state != SlotState::Reset).then_some(handle),
                    state: slot_state,
                });
                self.refill(sockets);
            }
        }
        if self.idle.is_none() {
            self.refill(sockets);
        }
        // The one-shot recv slot of every live hidden socket clears on its
        // first transition; reconcile always reconnects it to the accept
        // bridge, even while the idle socket is still LISTEN.
        if let Some(handle) = self.idle {
            sockets
                .get_mut::<Socket>(handle)
                .register_recv_waker(&self.accept.recv_waker());
        }
        for slot in &self.queue {
            if let Some(handle) = slot.handle {
                sockets
                    .get_mut::<Socket>(handle)
                    .register_recv_waker(&self.accept.recv_waker());
            }
        }
        changed
    }

    fn cleanup(self, sockets: &mut SocketSet<'_>) {
        if let Some(handle) = self.idle {
            sockets.remove(handle);
        }
        for slot in self.queue {
            if let Some(handle) = slot.handle {
                sockets.remove(handle);
            }
        }
    }
}

type ListenTableEntry = Arc<Mutex<Option<Box<ListenTableEntryInner>>>>;

pub struct ListenTable {
    tcp: Box<[ListenTableEntry]>,
    active_ports: Mutex<Vec<u16>>,
    /// Ports whose entry committed a hidden-socket transition during a stack
    /// round; drained by the runner after all network guards release.
    pending_accept_wakes: Mutex<Vec<u16>>,
}

impl ListenTable {
    pub fn new() -> Self {
        let tcp = unsafe {
            let mut buf = Box::new_uninit_slice(PORT_NUM);
            for i in 0..PORT_NUM {
                buf[i].write(Arc::default());
            }
            buf.assume_init()
        };
        Self {
            tcp,
            active_ports: Mutex::new(Vec::new()),
            pending_accept_wakes: Mutex::new(Vec::new()),
        }
    }

    pub fn can_listen(&self, port: u16) -> bool {
        self.tcp[port as usize].lock().is_none()
    }

    fn listen_to(
        &self,
        listen_endpoint: IpListenEndpoint,
        accept: Arc<ReadinessBridge>,
        sockets: &mut SocketSet<'_>,
    ) -> AxResult {
        let port = listen_endpoint.port;
        assert_ne!(port, 0);

        let mut entry = self.tcp[port as usize].lock();
        if entry.is_some() {
            warn!("socket already listening on port {port}");
            return Err(AxError::AddrInUse);
        }
        *entry = Some(Box::new(ListenTableEntryInner::new(
            listen_endpoint,
            accept,
            sockets,
        )));
        drop(entry);
        self.active_ports.lock().push(port);
        Ok(())
    }

    pub fn listen(
        &self,
        listen_endpoint: IpListenEndpoint,
        accept: Arc<ReadinessBridge>,
    ) -> AxResult {
        let mut sockets = SOCKET_SET.inner.lock();
        self.listen_to(listen_endpoint, accept, &mut sockets)
    }

    /// Test-only seam: registers a listener whose hidden sockets live in the
    /// caller-owned `SocketSet` instead of the production global, so a
    /// full-chain witness can drive `reconcile` over the injected set.
    #[cfg(test)]
    pub(crate) fn listen_with(
        &self,
        listen_endpoint: IpListenEndpoint,
        accept: Arc<ReadinessBridge>,
        sockets: &mut SocketSet<'_>,
    ) -> AxResult {
        self.listen_to(listen_endpoint, accept, sockets)
    }

    pub fn unlisten(&self, port: u16) {
        debug!("TCP socket unlisten on {}", port);
        let mut sockets = SOCKET_SET.inner.lock();
        if let Some(entry) = self.tcp[port as usize].lock().take() {
            entry.cleanup(&mut sockets);
        }
        drop(sockets);
        self.active_ports.lock().retain(|&active| active != port);
    }

    fn listen_entry(&self, port: u16) -> Arc<Mutex<Option<Box<ListenTableEntryInner>>>> {
        self.tcp[port as usize].clone()
    }

    /// Commits hidden transitions under Service + SocketSet guards; staged
    /// wakes drain only after those guards release.
    pub fn reconcile(&self, sockets: &mut SocketSet<'_>) {
        let active_ports = self.active_ports.lock().clone();
        let mut staged = Vec::new();
        for port in active_ports {
            if let Some(entry) = self.listen_entry(port).lock().as_mut() {
                if entry.reconcile(sockets) {
                    staged.push(port);
                }
            }
        }
        if !staged.is_empty() {
            self.pending_accept_wakes.lock().extend(staged);
        }
    }

    /// Wakes the accept bridge of every port whose transition was committed
    /// since the last drain. Only called after Service / SocketSet / entry
    /// guards release.
    pub fn drain_accept_wakes(&self) {
        let pending = core::mem::take(&mut *self.pending_accept_wakes.lock());
        let mut bridges = Vec::new();
        for port in pending {
            if let Some(entry) = self.listen_entry(port).lock().as_ref() {
                bridges.push(entry.accept.clone());
            }
        }
        for bridge in bridges {
            bridge.wake(IoEvents::IN);
        }
    }

    pub fn can_accept(&self, port: u16) -> AxResult<bool> {
        if let Some(entry) = self.listen_entry(port).lock().as_ref() {
            Ok(entry
                .queue
                .iter()
                .any(|slot| matches!(slot.state, SlotState::Ready | SlotState::Reset)))
        } else {
            warn!("accept before listen");
            Err(AxError::InvalidInput)
        }
    }

    /// Consumes one Ready/Reset slot and restores an idle hidden listener in
    /// a single `SOCKET_SET -> entry` critical section (Task 2.7 replan).
    ///
    /// The refill only creates/registers a hidden smoltcp socket; it never
    /// calls `Interface::poll`, never acquires the Service, and never wakes
    /// inside the guard. Waking the accept bridge and publishing software
    /// work happen after this returns, when the caller's guards are dropped.
    pub fn accept_with(&self, port: u16, sockets: &mut SocketSet<'_>) -> AxResult<SocketHandle> {
        let entry = self.listen_entry(port);
        let mut table = entry.lock();
        let Some(entry) = table.as_mut() else {
            warn!("accept before listen");
            return Err(AxError::InvalidInput);
        };

        let idx = entry
            .queue
            .iter()
            .position(|slot| slot.state != SlotState::Pending)
            .ok_or(AxError::WouldBlock)?;
        if idx > 0 {
            warn!(
                "slow listen queue enumeration: index = {}, len = {}!",
                idx,
                entry.queue.len()
            );
        }
        let slot = entry.queue.swap_remove_front(idx).unwrap();
        // Consuming one slot frees headroom: restore an idle hidden listener
        // before returning so an immediate reconnect finds a LISTEN socket
        // without waiting for the runner's next reconcile.
        entry.refill(sockets);
        info!(
            "listen {}: accept consumed {:?} (queue {}, idle {})",
            port,
            slot.state,
            entry.queue.len(),
            entry.idle.is_some()
        );
        match slot.state {
            SlotState::Ready => Ok(slot.handle.expect("ready listener slot without handle")),
            SlotState::Reset => {
                debug!("accept failed: connection reset");
                Err(AxError::ConnectionReset)
            }
            SlotState::Pending => unreachable!(),
        }
    }

    /// T2.7-R1 test seam: seeds the port's hidden queue to exactly
    /// `LISTEN_QUEUE_SIZE` real hidden sockets — one Ready, the rest
    /// Pending — with no idle listener, so the next `accept_with` must
    /// restore one. Tears down the entry's previous hidden sockets first so
    /// a 100x scale loop reuses one injected SocketSet. Returns the seeded
    /// Ready handle. Production paths never call this.
    #[cfg(test)]
    pub(crate) fn test_seed_full_queue(
        &self,
        port: u16,
        sockets: &mut SocketSet<'_>,
    ) -> SocketHandle {
        let entry = self.listen_entry(port);
        let mut guard = entry.lock();
        let entry = guard.as_mut().expect("test listener registered");
        if let Some(idle) = entry.idle.take() {
            sockets.remove(idle);
        }
        for slot in entry.queue.drain(..) {
            if let Some(handle) = slot.handle {
                sockets.remove(handle);
            }
        }
        let mut ready_handle = None;
        while entry.queue.len() < LISTEN_QUEUE_SIZE {
            let mut socket = new_tcp_socket();
            socket
                .listen(entry.listen_endpoint)
                .expect("seeded hidden listen socket");
            let handle = sockets.add(socket);
            sockets
                .get_mut::<Socket>(handle)
                .register_recv_waker(&entry.accept.recv_waker());
            let state = if entry.queue.len() == LISTEN_QUEUE_SIZE - 1 {
                ready_handle = Some(handle);
                SlotState::Ready
            } else {
                SlotState::Pending
            };
            entry.queue.push_back(ListenSlot {
                handle: Some(handle),
                state,
            });
        }
        drop(guard);
        ready_handle.expect("one Ready slot staged")
    }

    /// T2.7-R1 test seam: number of hidden slots currently queued for the
    /// port (inspection only).
    #[cfg(test)]
    pub(crate) fn test_queue_len(&self, port: u16) -> usize {
        self.listen_entry(port)
            .lock()
            .as_ref()
            .map_or(0, |entry| entry.queue.len())
    }

    /// T2.7-R1 test seam: whether an idle hidden listener is present.
    #[cfg(test)]
    pub(crate) fn test_idle_is_some(&self, port: u16) -> bool {
        self.listen_entry(port)
            .lock()
            .as_ref()
            .is_some_and(|entry| entry.idle.is_some())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use alloc::{boxed::Box, sync::Arc, vec, vec::Vec};
    use core::{
        sync::atomic::{AtomicUsize, Ordering},
        task::Waker,
    };

    use axpoll::IoEvents;
    use smoltcp::{iface::SocketSet, wire::IpListenEndpoint};

    use super::{LISTEN_QUEUE_SIZE, ListenSlot, ListenTable, ListenTableEntryInner, SlotState};
    use crate::{readiness::ReadinessBridge, tcp::new_tcp_socket};

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

    fn endpoint(port: u16) -> IpListenEndpoint {
        IpListenEndpoint { addr: None, port }
    }

    /// A standalone session: a `ListenTable` with one registered entry whose
    /// hidden sockets live in the caller-owned `SocketSet` (no globals).
    fn session(port: u16) -> (ListenTable, SocketSet<'static>, Arc<ReadinessBridge>) {
        let table = ListenTable::new();
        let bridge = Arc::new(ReadinessBridge::new());
        let mut sockets = SocketSet::new(vec![]);
        *table.tcp[port as usize].lock() = Some(Box::new(ListenTableEntryInner::new(
            endpoint(port),
            bridge.clone(),
            &mut sockets,
        )));
        table.active_ports.lock().push(port);
        (table, sockets, bridge)
    }

    /// Closes the idle hidden socket and returns false when there is none.
    fn close_idle(table: &ListenTable, port: u16, sockets: &mut SocketSet<'_>) -> bool {
        let idle = {
            let entry = table.tcp[port as usize].lock();
            let Some(entry) = entry.as_ref() else {
                return false;
            };
            let Some(idle) = entry.idle else {
                return false;
            };
            idle
        };
        use smoltcp::socket::tcp::Socket;
        sockets.get_mut::<Socket>(idle).close();
        true
    }

    #[test]
    fn hidden_socket_creation_arms_accept_bridge_recv_slot() {
        let (table, mut sockets, bridge) = session(18080);
        let count = Arc::new(AtomicUsize::new(0));
        bridge.register(IoEvents::IN, &counting_waker(count.clone()));

        close_idle(&table, 18080, &mut sockets);

        // The closing hidden socket wakes its armed recv slot → accept bridge.
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn reconcile_stages_transition_and_drain_wakes_after_commit() {
        let (table, mut sockets, bridge) = session(18081);
        // The closing hidden socket already consumed its direct slot wake
        // with no waiter registered; the staged drain is now the only path
        // that can wake a waiter registered after the transition.
        assert!(close_idle(&table, 18081, &mut sockets));
        table.reconcile(&mut sockets);

        let count = Arc::new(AtomicUsize::new(0));
        bridge.register(IoEvents::IN, &counting_waker(count.clone()));
        assert_eq!(count.load(Ordering::Relaxed), 0);

        table.drain_accept_wakes();
        assert_eq!(count.load(Ordering::Relaxed), 1);
        table.drain_accept_wakes();
        assert_eq!(count.load(Ordering::Relaxed), 1);

        let second = Arc::new(AtomicUsize::new(0));
        bridge.register(IoEvents::IN, &counting_waker(second.clone()));
        table.reconcile(&mut sockets);
        table.drain_accept_wakes();
        assert_eq!(second.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn reset_transition_removes_hidden_handle_and_reconcile_rearms_idle() {
        let (table, mut sockets, bridge) = session(18082);
        assert!(close_idle(&table, 18082, &mut sockets));
        table.reconcile(&mut sockets);
        table.drain_accept_wakes();

        let count = Arc::new(AtomicUsize::new(0));
        bridge.register(IoEvents::IN, &counting_waker(count.clone()));

        // Reconcile rearms the fresh idle hidden socket; closing it wakes the
        // accept bridge again (one-shot slot restored).
        assert!(close_idle(&table, 18082, &mut sockets));
        table.reconcile(&mut sockets);
        table.drain_accept_wakes();
        assert_eq!(count.load(Ordering::Relaxed), 1);

        let entry = table.tcp[18082].lock();
        let entry = entry.as_ref().unwrap();
        assert!(entry.idle.is_some());
        let mut reset_found = false;
        for slot in &entry.queue {
            if matches!(slot.state, SlotState::Reset) {
                reset_found = true;
                assert!(slot.handle.is_none());
            }
        }
        assert!(reset_found);
    }

    #[test]
    fn accept_consumes_ready_and_reset_slots_exactly_once() {
        let (table, mut sockets, _) = session(18083);
        let ready_handle = {
            let mut socket = new_tcp_socket();
            socket.listen(endpoint(18083)).unwrap();
            sockets.add(socket)
        };
        {
            let mut entry = table.tcp[18083].lock();
            let entry = entry.as_mut().unwrap();
            entry.queue.push_back(ListenSlot {
                handle: Some(ready_handle),
                state: SlotState::Ready,
            });
        }
        assert_eq!(
            table.accept_with(18083, &mut sockets).unwrap(),
            ready_handle
        );
        assert!(matches!(
            table.accept_with(18083, &mut sockets),
            Err(axerrno::AxError::WouldBlock)
        ));

        // A reset slot reports ConnectionReset and is consumed once.
        let mut entry = table.tcp[18083].lock();
        entry.as_mut().unwrap().queue.push_back(ListenSlot {
            handle: None,
            state: SlotState::Reset,
        });
        drop(entry);
        assert!(matches!(
            table.accept_with(18083, &mut sockets),
            Err(axerrno::AxError::ConnectionReset)
        ));
        assert!(matches!(
            table.accept_with(18083, &mut sockets),
            Err(axerrno::AxError::WouldBlock)
        ));
    }

    #[test]
    fn full_queue_accept_frees_headroom_and_refills_idle_atomically() {
        // Task 2.7 replan: consuming one backlog slot must restore an idle
        // hidden listener inside the same `SOCKET_SET -> entry` critical
        // section, so an immediate reconnect never depends on the runner's
        // next reconcile (fresh QEMU MS01 tcp-512-recovery got
        // ConnectionRefused without it). The old witness required an
        // explicit `table.reconcile` call after accept; that call must no
        // longer be needed.
        let (table, mut sockets, _) = session(18084);
        let mut entry_guard = table.tcp[18084].lock();
        let entry = entry_guard.as_mut().unwrap();
        // Leave no idle and fill the backlog: a consumed slot must then
        // trigger an atomic refill, not wait for a later reconcile.
        entry.idle = None;
        assert!(entry.queue.len() < LISTEN_QUEUE_SIZE);
        let mut ready_handle = None;
        while entry.queue.len() < LISTEN_QUEUE_SIZE {
            let socket = new_tcp_socket();
            let handle = sockets.add(socket);
            let state = if entry.queue.len() == LISTEN_QUEUE_SIZE - 1 {
                ready_handle = Some(handle);
                SlotState::Ready
            } else {
                SlotState::Pending
            };
            entry.queue.push_back(ListenSlot {
                handle: Some(handle),
                state,
            });
        }
        let ready_handle = ready_handle.expect("one Ready slot staged");
        drop(entry_guard);

        // Consume the Ready slot; the idle hidden listener must already be
        // refilled when accept returns — no reconcile call in between.
        assert_eq!(
            table.accept_with(18084, &mut sockets).unwrap(),
            ready_handle
        );
        let entry = table.tcp[18084].lock();
        assert!(
            entry.as_ref().unwrap().idle.is_some(),
            "accept must restore an idle hidden listener before returning"
        );
        let queue_len = entry.as_ref().unwrap().queue.len();
        assert_eq!(queue_len, LISTEN_QUEUE_SIZE - 1);
    }

    #[test]
    fn cleanup_removes_all_hidden_handles_without_leak() {
        let (table, mut sockets, _) = session(18085);
        let entry = table.tcp[18085].lock().take();
        entry.unwrap().cleanup(&mut sockets);
        assert_eq!(sockets.iter().count(), 0);
    }

    #[test]
    fn drain_fans_out_to_all_registered_accept_waiters() {
        let (table, mut sockets, bridge) = session(18086);
        let waiters: Vec<_> = (0..64)
            .map(|_| {
                let count = Arc::new(AtomicUsize::new(0));
                bridge.register(IoEvents::IN, &counting_waker(count.clone()));
                count
            })
            .collect();

        close_idle(&table, 18086, &mut sockets);
        table.reconcile(&mut sockets);
        table.drain_accept_wakes();

        for counter in &waiters {
            assert_eq!(counter.load(Ordering::Relaxed), 1);
        }
    }
}
