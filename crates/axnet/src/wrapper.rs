#[cfg(test)]
use alloc::boxed::Box;
use alloc::{sync::Arc, vec, vec::Vec};

use axdriver::prelude::DevError;
use axerrno::{AxError, AxResult};
use axpoll::IoEvents;
use axsync::Mutex;
use hashbrown::HashMap;
use smoltcp::{
    iface::{SocketHandle, SocketSet},
    socket::{AnySocket, Socket},
    wire::{IpAddress, IpListenEndpoint},
};

use crate::readiness::{self, ReadinessBridge};

pub(crate) struct SocketSetWrapper<'a> {
    pub inner: Mutex<SocketSet<'a>>,
    tcp_bound: Mutex<HashMap<SocketHandle, IpListenEndpoint>>,
    readiness: Mutex<HashMap<SocketHandle, ReadinessRegistration>>,
    /// The current SocketEpoch state and its terminal are protected by the
    /// same lock used to install registry entries. This makes add/close a
    /// single linearization domain: a handle is either in the close snapshot
    /// or is installed into the already-closed epoch with its terminal.
    epoch_state: Mutex<SocketEpochState>,
}

#[derive(Clone)]
struct ReadinessRegistration {
    epoch: u64,
    bridge: Arc<ReadinessBridge>,
}

struct SocketEpochState {
    current: u64,
    open: bool,
    /// Legacy publisher code retained for existing observers.
    terminal: u64,
    /// NetworkTerminal code used to seed late registrations in this epoch.
    network_terminal: u64,
    /// One bounded fallback for an adoption race that crosses the current
    /// closure. Accepted owners register in `pending` before the listener
    /// critical section ends, so older identities live on the owner instead
    /// of accumulating in a boot-lifetime history.
    last_closed: Option<(u64, u64)>,
    pending: HashMap<SocketHandle, PendingEpochOwner>,
}

struct PendingEpochOwner {
    epoch: u64,
    terminal: u64,
}

#[derive(Clone, Copy)]
pub(crate) struct SocketEpochTerminalCommit {
    pub(crate) terminal: readiness::NetworkTerminal,
    pub(crate) committed: bool,
}

/// Task 5.1 (Iteration 006): test-only per-test socket/listener registry
/// pair. A fixture owns an independent [`SocketSetWrapper`] and
/// [`ListenTable`], so parallel host tests can reuse identical numeric
/// handles without touching the process-global `SOCKET_SET`/`LISTEN_TABLE`
/// (R57 stale-handle/SIGABRT race prerequisite).
///
/// Cycle 001 rework: the fixture also owns the [`Service`] paired with those
/// registries, so deferred removals enqueued by a fixture Drop are reaped
/// by a bounded local poll against this same `SocketSet` - the equal
/// numeric handle in the global (or a neighbor) set is never consulted.
#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) struct SocketTestContext {
    pub sockets: &'static SocketSetWrapper<'static>,
    pub listen_table: &'static crate::listen_table::ListenTable,
    pub service: &'static axsync::Mutex<crate::service::Service>,
}

#[cfg(test)]
impl SocketTestContext {
    /// Leaks a fresh independent registry pair plus the Service that owns
    /// this fixture's deferred removals (built against the same
    /// `ListenTable`, with a loopback-only Router so local egress can
    /// dispatch fixture datagrams). One fixture per call; the `PORT_NUM`
    /// port-indexed table makes each instance ~3 MiB, so callers reuse the
    /// context across churn iterations instead of per socket.
    pub(crate) fn leak_new() -> Self {
        let sockets: &'static SocketSetWrapper<'static> =
            Box::leak(Box::new(SocketSetWrapper::new()));
        let listen_table: &'static crate::listen_table::ListenTable =
            Box::leak(Box::new(crate::listen_table::ListenTable::new()));
        let mut router = crate::router::Router::new();
        router.add_device(Box::new(crate::device::LoopbackDevice::new()));
        let service: &'static axsync::Mutex<crate::service::Service> =
            Box::leak(Box::new(axsync::Mutex::new(
                crate::service::Service::new_with_listen_table(router, None, listen_table),
            )));
        service.lock().set_socket_registry(sockets);
        Self {
            sockets,
            listen_table,
            service,
        }
    }
}

impl<'a> SocketSetWrapper<'a> {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(SocketSet::new(vec![])),
            tcp_bound: Mutex::new(HashMap::new()),
            readiness: Mutex::new(HashMap::new()),
            epoch_state: Mutex::new(SocketEpochState {
                current: 0,
                open: true,
                terminal: readiness::TERMINAL_NONE,
                network_terminal: readiness::TERMINAL_NONE,
                last_closed: None,
                pending: HashMap::new(),
            }),
        }
    }

    /// Adds a smoltcp socket and atomically installs a fresh per-public-handle
    /// bridge for it. The handle is not observable until `Socket` construction
    /// returns, so registry lookup can never miss an installed bridge. A new
    /// handle in an open epoch starts fresh; a handle created while its epoch
    /// is already closed receives that epoch's terminal immediately.
    pub fn add_public<T: AnySocket<'a>>(&self, socket: T) -> (SocketHandle, Arc<ReadinessBridge>) {
        let handle = {
            let mut sockets = self.inner.lock();
            let handle = sockets.add(socket);
            drop(sockets);
            handle
        };
        debug!("socket {}: created", handle);
        let bridge = Arc::new(ReadinessBridge::new());
        let mut state = self.epoch_state.lock();
        state.pending.remove(&handle);
        let epoch = state.current;
        let terminal = (!state.open).then_some(state.network_terminal);
        self.readiness.lock().insert(
            handle,
            ReadinessRegistration {
                epoch,
                bridge: bridge.clone(),
            },
        );
        if let Some(code) = terminal {
            if let Some(terminal) = readiness::NetworkTerminal::from_code(code) {
                bridge.commit_network_terminal(terminal);
            }
        }
        (handle, bridge)
    }

    /// Installs a bridge for a handle that already lives in the smoltcp set:
    /// the accept-adoption path for a hidden listener socket. The adopted
    /// bridge keeps only socket-local state; global faults are observed
    /// through the effective snapshot.
    pub fn install_readiness(&self, handle: SocketHandle, bridge: Arc<ReadinessBridge>) {
        let mut state = self.epoch_state.lock();
        let pending = state.pending.remove(&handle);
        let epoch = self.readiness.lock().get(&handle).map_or_else(
            || pending.as_ref().map_or(state.current, |owner| owner.epoch),
            |registration| registration.epoch,
        );
        let terminal = pending
            .filter(|owner| owner.epoch == epoch)
            .map(|owner| owner.terminal)
            .or_else(|| {
                state
                    .last_closed
                    .filter(|(closed_epoch, _)| *closed_epoch == epoch)
                    .map(|(_, terminal)| terminal)
            });
        self.readiness.lock().insert(
            handle,
            ReadinessRegistration {
                epoch,
                bridge: bridge.clone(),
            },
        );
        if let Some(code) = terminal {
            if let Some(terminal) = readiness::NetworkTerminal::from_code(code) {
                bridge.commit_network_terminal(terminal);
            }
        }
    }

    /// Installs an adopted hidden-listener handle into its recorded epoch.
    /// The accepted connection may have crossed a link-down/up boundary
    /// before the application adopts it, so this never substitutes the
    /// current epoch.
    pub(crate) fn install_readiness_for_epoch(
        &self,
        handle: SocketHandle,
        bridge: Arc<ReadinessBridge>,
        epoch: u64,
    ) {
        let mut state = self.epoch_state.lock();
        let pending = state.pending.remove(&handle);
        let terminal = pending
            .filter(|owner| owner.epoch == epoch)
            .map(|owner| owner.terminal)
            .or_else(|| {
                state
                    .last_closed
                    .filter(|(closed_epoch, _)| *closed_epoch == epoch)
                    .map(|(_, terminal)| terminal)
            });
        self.readiness.lock().insert(
            handle,
            ReadinessRegistration {
                epoch,
                bridge: bridge.clone(),
            },
        );
        if let Some(code) = terminal.and_then(readiness::NetworkTerminal::from_code) {
            bridge.commit_network_terminal(code);
        }
    }

    /// Returns whether the current SocketEpoch accepts new sessions.
    /// Returns the epoch recorded with a public or adopted handle.
    pub(crate) fn socket_epoch(&self, handle: SocketHandle) -> Option<u64> {
        self.readiness
            .lock()
            .get(&handle)
            .map(|registration| registration.epoch)
    }

    /// Returns the current epoch identity for a service-owned transition.
    pub(crate) fn current_socket_epoch(&self) -> u64 {
        self.epoch_state.lock().current
    }

    /// Records a hidden accepted raw owner before the listener critical
    /// section ends. A concurrent closure writes its terminal into this owner
    /// record, allowing adoption after any later epoch opens without an
    /// unbounded terminal history.
    pub(crate) fn register_pending_epoch_owner(&self, handle: SocketHandle, epoch: u64) {
        let mut state = self.epoch_state.lock();
        let terminal = state
            .last_closed
            .filter(|(closed_epoch, _)| *closed_epoch == epoch)
            .map_or(readiness::TERMINAL_NONE, |(_, terminal)| terminal);
        state
            .pending
            .insert(handle, PendingEpochOwner { epoch, terminal });
    }

    #[cfg(test)]
    pub(crate) fn terminal_history_len(&self) -> usize {
        usize::from(self.epoch_state.lock().last_closed.is_some())
    }

    #[cfg(test)]
    pub(crate) fn pending_epoch_owner_count(&self) -> usize {
        self.epoch_state.lock().pending.len()
    }

    /// Closes one epoch with a first-wins terminal, snapshots only its
    /// registrations, and wakes after all registry locks are released.
    pub(crate) fn close_socket_epoch(
        &self,
        epoch: u64,
        terminal: readiness::NetworkTerminal,
    ) -> bool {
        self.close_socket_epoch_with_codes(epoch, terminal, terminal.code(), true)
    }

    fn close_socket_epoch_with_codes(
        &self,
        epoch: u64,
        terminal: readiness::NetworkTerminal,
        reported_code: u64,
        wake: bool,
    ) -> bool {
        let Some(outcome) = self.commit_socket_epoch_with_codes(epoch, terminal, reported_code)
        else {
            return false;
        };
        if wake && outcome.committed {
            self.wake_socket_epoch(epoch);
        }
        outcome.committed
    }

    fn commit_socket_epoch_with_codes(
        &self,
        epoch: u64,
        terminal: readiness::NetworkTerminal,
        reported_code: u64,
    ) -> Option<SocketEpochTerminalCommit> {
        let mut state = self.epoch_state.lock();
        if state.current != epoch {
            return None;
        }
        if !state.open {
            let terminal = readiness::NetworkTerminal::from_code(state.network_terminal)?;
            return Some(SocketEpochTerminalCommit {
                terminal,
                committed: false,
            });
        }
        state.open = false;
        state.terminal = reported_code;
        state.network_terminal = terminal.code();
        state.last_closed = Some((epoch, terminal.code()));
        for owner in state.pending.values_mut() {
            if owner.epoch == epoch {
                owner.terminal = terminal.code();
            }
        }

        let readiness = self.readiness.lock();
        for registration in readiness.values() {
            if registration.epoch == epoch {
                registration.bridge.commit_network_terminal(terminal);
            }
        }
        Some(SocketEpochTerminalCommit {
            terminal,
            committed: true,
        })
    }

    /// Wakes all bridges registered to one already-committed epoch. No
    /// Service/SocketSet/registry guard is held while invoking a waker.
    pub(crate) fn wake_socket_epoch(&self, epoch: u64) {
        let bridges = self
            .readiness
            .lock()
            .values()
            .filter(|registration| registration.epoch == epoch)
            .map(|registration| registration.bridge.clone())
            .collect::<Vec<_>>();
        for bridge in bridges {
            bridge.wake_for_global_publication();
        }
    }

    /// Opens the next checked SocketEpoch. Old bridge terminal state remains
    /// attached to its old registrations and is never cleared.
    pub(crate) fn open_next_socket_epoch(&self) -> Result<u64, AxError> {
        let mut state = self.epoch_state.lock();
        if state.open || state.current == u64::MAX {
            return Err(AxError::BadState);
        }
        state.current += 1;
        state.open = true;
        state.terminal = readiness::TERMINAL_NONE;
        state.network_terminal = readiness::TERMINAL_NONE;
        Ok(state.current)
    }

    /// Returns the committed terminal code for the current SocketEpoch, if
    /// that epoch is closed. This name remains for compatibility with the
    /// existing diagnostics/tests; the value is no longer boot-global.
    pub fn global_terminal_code(&self) -> u64 {
        self.epoch_state.lock().terminal
    }

    /// Publishes one concrete queue-owner [`DevError`] as the terminal of the
    /// current SocketEpoch. Idempotent while that epoch is open: only the
    /// first publication closes and commits it.
    ///
    /// Order: commit the epoch code (the linearization point), snapshot only
    /// matching bridges under the registry lock, release the lock, then wake
    /// each bridge unconditionally — no guard is held across any wake.
    pub fn publish_global_fault(&self, err: &DevError) {
        self.publish_global_fault_code(readiness::dev_error_code(err));
    }

    /// Publishes an already-encoded terminal code; see
    /// [`Self::publish_global_fault`].
    pub fn publish_global_fault_code(&self, code: u64) {
        if code == readiness::TERMINAL_NONE {
            return;
        }
        let epoch = self.epoch_state.lock().current;
        let _ = self.publish_socket_epoch_fault_code(epoch, code);
    }

    /// Publishes a fault to one explicitly captured SocketEpoch. A late
    /// publisher for a closed epoch is ignored, so it cannot retarget a later
    /// open epoch. The method commits, snapshots and wakes using the same
    /// target identity.
    pub(crate) fn publish_socket_epoch_fault_code(&self, epoch: u64, code: u64) -> bool {
        let Some(outcome) = self.commit_socket_epoch_fault_code(epoch, code) else {
            return false;
        };
        if outcome.committed {
            self.wake_socket_epoch(epoch);
        }
        outcome.committed
    }

    pub(crate) fn commit_socket_epoch_fault_code(
        &self,
        epoch: u64,
        code: u64,
    ) -> Option<SocketEpochTerminalCommit> {
        if code == readiness::TERMINAL_NONE {
            return None;
        }
        let terminal = readiness::NetworkTerminal::from_code(code)
            .unwrap_or_else(|| readiness::NetworkTerminal::from_legacy_code(code));
        self.commit_socket_epoch_with_codes(epoch, terminal, code)
    }

    /// Task 2.1 contract requires a registry lookup; currently test-witness
    /// only, alive when a future path wakes by handle. Do not delete as "dead".
    #[allow(dead_code)]
    pub fn lookup_readiness(&self, handle: SocketHandle) -> Option<Arc<ReadinessBridge>> {
        self.readiness
            .lock()
            .get(&handle)
            .map(|registration| registration.bridge.clone())
    }

    /// Removes the public-handle bridge from the registry and wakes its
    /// leftover waiters. The wake happens after the registry guard drops.
    fn take_readiness(&self, handle: SocketHandle) -> Option<Arc<ReadinessBridge>> {
        let bridge = self
            .readiness
            .lock()
            .remove(&handle)
            .map(|registration| registration.bridge);
        if let Some(bridge) = &bridge {
            bridge.wake(
                IoEvents::IN | IoEvents::OUT | IoEvents::RDHUP | IoEvents::HUP | IoEvents::ERR,
            );
        }
        bridge
    }

    pub fn with_socket_mut<T: AnySocket<'a>, R, F>(&self, handle: SocketHandle, f: F) -> R
    where
        F: FnOnce(&mut T) -> R,
    {
        let mut set = self.inner.lock();
        let socket = set.get_mut(handle);
        f(socket)
    }

    pub fn bind_check(&self, addr: IpAddress, port: u16) -> AxResult {
        if port == 0 {
            return Ok(());
        }

        // TODO(mivik): optimize
        let mut sockets = self.inner.lock();
        if self
            .tcp_bound
            .lock()
            .values()
            .any(|endpoint| endpoint.port == port)
        {
            return Err(AxError::AddrInUse);
        }
        for (_, socket) in sockets.iter_mut() {
            match socket {
                Socket::Tcp(_) => {}
                Socket::Udp(s) => {
                    if s.endpoint().addr == Some(addr) && s.endpoint().port == port {
                        return Err(AxError::AddrInUse);
                    }
                }
                _ => continue,
            };
        }
        Ok(())
    }

    pub fn tcp_bound_endpoint(&self, handle: SocketHandle) -> Option<IpListenEndpoint> {
        self.tcp_bound.lock().get(&handle).copied()
    }

    pub fn set_tcp_bound_endpoint(&self, handle: SocketHandle, endpoint: IpListenEndpoint) {
        self.tcp_bound.lock().insert(handle, endpoint);
    }

    /// T2.5-R2: detaches public metadata (readiness bridge + bound endpoint)
    /// of a closing TCP handle and wakes leftover waiters once; the raw
    /// smoltcp handle stays in the set for the resident runner to reclaim
    /// once the close is protocol-confirmed. Never touches the raw set.
    pub fn retire_public(&self, handle: SocketHandle) {
        self.take_readiness(handle);
        self.tcp_bound.lock().remove(&handle);
    }

    /// T2.5-R2: removes only the raw smoltcp handle. Reserved for the
    /// resident runner's confirmed-close reaper and for immediate-removal
    /// paths that already retired public metadata; never wakes waiters.
    pub fn remove_raw(&self, handle: SocketHandle) {
        let mut sockets = self.inner.lock();
        sockets.remove(handle);
        let mut state = self.epoch_state.lock();
        state.pending.remove(&handle);
        drop(state);
        drop(sockets);
        debug!("socket {}: destroyed", handle);
    }

    pub fn remove(&self, handle: SocketHandle) {
        // Combined immediate teardown for paths without an outstanding
        // close protocol (UDP, idle/confirmed TCP): retire public metadata,
        // then remove the raw handle.
        self.retire_public(handle);
        self.remove_raw(handle);
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use alloc::{boxed::Box, format, sync::Arc};
    use core::{
        sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        task::Waker,
    };

    use axpoll::IoEvents;

    use super::SocketSetWrapper;
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

    #[test]
    fn new_public_handle_installs_identical_bridge() {
        let wrapper = SocketSetWrapper::new();
        let (handle, bridge) = wrapper.add_public(new_tcp_socket());

        assert!(Arc::ptr_eq(
            &bridge,
            &wrapper.lookup_readiness(handle).unwrap()
        ));
    }

    #[test]
    fn adoption_installs_bridge_for_existing_handle() {
        let wrapper = SocketSetWrapper::new();
        let (handle, _original) = wrapper.add_public(new_tcp_socket());
        let adopted = Arc::new(ReadinessBridge::new());
        wrapper.install_readiness(handle, adopted.clone());

        assert!(Arc::ptr_eq(
            &adopted,
            &wrapper.lookup_readiness(handle).unwrap()
        ));
    }

    #[test]
    fn remove_takes_bridge_and_wakes_leftover_waiters() {
        let wrapper = SocketSetWrapper::new();
        let (handle, bridge) = wrapper.add_public(new_tcp_socket());
        let count = Arc::new(AtomicUsize::new(0));
        bridge.register(IoEvents::IN, &counting_waker(count.clone()));

        wrapper.remove(handle);

        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert!(wrapper.lookup_readiness(handle).is_none());
    }

    #[test]
    fn handle_reuse_does_not_inherit_old_bridge() {
        let wrapper = SocketSetWrapper::new();
        let (handle, first) = wrapper.add_public(new_tcp_socket());
        wrapper.remove(handle);
        let (_, second) = wrapper.add_public(new_tcp_socket());

        assert!(!Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn socket_epoch_closure_is_scoped_and_new_epoch_is_fresh() {
        use crate::readiness::NetworkTerminal;

        let wrapper = SocketSetWrapper::new();
        let (old_handle, old_bridge) = wrapper.add_public(new_tcp_socket());
        let old_epoch = wrapper.socket_epoch(old_handle).unwrap();
        let wakes = Arc::new(AtomicUsize::new(0));
        old_bridge.register(IoEvents::ERR, &counting_waker(wakes.clone()));

        assert!(wrapper.close_socket_epoch(old_epoch, NetworkTerminal::LinkDown));
        assert_eq!(
            old_bridge.network_terminal_code(),
            NetworkTerminal::LinkDown.code()
        );
        assert_eq!(wakes.load(Ordering::Relaxed), 1);

        // A late handle while the epoch is closed must remain in that
        // terminal; opening the next epoch must not clear the old bridge.
        let (_late_handle, late_bridge) = wrapper.add_public(new_tcp_socket());
        assert_eq!(
            late_bridge.network_terminal_code(),
            NetworkTerminal::LinkDown.code()
        );

        let new_epoch = wrapper.open_next_socket_epoch().unwrap();
        assert_eq!(new_epoch, old_epoch + 1);
        let (new_handle, new_bridge) = wrapper.add_public(new_tcp_socket());
        assert_eq!(wrapper.socket_epoch(new_handle), Some(new_epoch));
        assert_eq!(new_bridge.terminal_code(), readiness::TERMINAL_NONE);
        assert_eq!(
            old_bridge.network_terminal_code(),
            NetworkTerminal::LinkDown.code()
        );
    }

    #[test]
    fn delayed_listener_adoption_keeps_the_closed_epoch_terminal() {
        use crate::readiness::NetworkTerminal;

        let wrapper = SocketSetWrapper::new();
        let (listener_handle, _listener_bridge) = wrapper.add_public(new_tcp_socket());
        let old_epoch = wrapper.socket_epoch(listener_handle).unwrap();
        let hidden_handle = wrapper.inner.lock().add(new_tcp_socket());
        wrapper.register_pending_epoch_owner(hidden_handle, old_epoch);
        assert_eq!(wrapper.pending_epoch_owner_count(), 1);

        assert!(wrapper.close_socket_epoch(old_epoch, NetworkTerminal::ConnectionReset));
        assert_eq!(wrapper.open_next_socket_epoch(), Ok(old_epoch + 1));

        let adopted = Arc::new(ReadinessBridge::new());
        wrapper.install_readiness_for_epoch(hidden_handle, adopted.clone(), old_epoch);
        assert_eq!(
            adopted.network_terminal_code(),
            NetworkTerminal::ConnectionReset.code()
        );
        assert_eq!(wrapper.socket_epoch(hidden_handle), Some(old_epoch));
        assert_eq!(wrapper.pending_epoch_owner_count(), 0);
    }

    #[test]
    fn pending_epoch_owner_is_removed_with_raw_handle() {
        let wrapper = SocketSetWrapper::new();
        let handle = wrapper.inner.lock().add(new_tcp_socket());
        wrapper.register_pending_epoch_owner(handle, wrapper.current_socket_epoch());
        assert_eq!(wrapper.pending_epoch_owner_count(), 1);

        wrapper.remove_raw(handle);
        assert_eq!(wrapper.pending_epoch_owner_count(), 0);
    }

    #[test]
    fn raw_removal_keeps_slot_and_pending_owner_in_one_critical_section() {
        let src = include_str!("wrapper.rs");
        let remove =
            &src[src.find("pub fn remove_raw").unwrap()..src.find("pub fn remove(&self").unwrap()];
        let pending_remove = remove.find("state.pending.remove(&handle)").unwrap();
        let release_inner = remove.find("drop(sockets)").unwrap();

        assert!(
            pending_remove < release_inner,
            "the raw slot must not become reusable before its pending owner is removed"
        );
    }

    #[test]
    fn raw_removal_cannot_delete_pending_owner_for_reused_handle() {
        let wrapper = SocketSetWrapper::new();
        let handle = wrapper.inner.lock().add(new_tcp_socket());
        wrapper.register_pending_epoch_owner(handle, wrapper.current_socket_epoch());

        wrapper.remove_raw(handle);

        let reused = wrapper.inner.lock().add(new_tcp_socket());
        assert_eq!(reused, handle, "the released numeric slot must be reused");
        wrapper.register_pending_epoch_owner(reused, wrapper.current_socket_epoch());
        assert_eq!(wrapper.pending_epoch_owner_count(), 1);
    }

    #[test]
    fn socket_registry_lock_order_is_acyclic() {
        let wrapper = include_str!("wrapper.rs");
        let add = &wrapper[wrapper.find("pub fn add_public").unwrap()
            ..wrapper.find("/// Installs a bridge for a handle").unwrap()];
        let add_state = add.find("let mut state = self.epoch_state.lock()").unwrap();
        let add_inner = add.find("self.inner.lock()").unwrap();
        let add_readiness = add.find("self.readiness.lock().insert").unwrap();
        assert!(add_inner < add_state && add_state < add_readiness);

        let remove = &wrapper[wrapper.find("pub fn remove_raw").unwrap()
            ..wrapper.find("pub fn remove(&self").unwrap()];
        assert!(
            remove.find("self.inner.lock()").unwrap()
                < remove.find("self.epoch_state.lock()").unwrap()
        );

        let tcp = include_str!("tcp.rs");
        let accept = &tcp[tcp.find("fn accept(&self)").unwrap()..tcp.find("fn send(").unwrap()];
        assert!(
            accept.find("register_pending_epoch_owner").unwrap()
                < accept.find("drop(sockets)").unwrap()
        );
    }

    #[test]
    fn explicit_epoch_fault_cannot_retarget_after_reopen() {
        use crate::readiness::NetworkTerminal;

        let wrapper = SocketSetWrapper::new();
        let (old_handle, old_bridge) = wrapper.add_public(new_tcp_socket());
        let old_epoch = wrapper.socket_epoch(old_handle).unwrap();
        assert!(wrapper.close_socket_epoch(old_epoch, NetworkTerminal::LinkDown));
        assert_eq!(wrapper.open_next_socket_epoch(), Ok(old_epoch + 1));
        let (_fresh_handle, fresh_bridge) = wrapper.add_public(new_tcp_socket());

        assert!(!wrapper.publish_socket_epoch_fault_code(
            old_epoch,
            crate::readiness::dev_error_code(&DevError::Io),
        ));
        assert_eq!(
            old_bridge.network_terminal_code(),
            NetworkTerminal::LinkDown.code()
        );
        assert_eq!(
            fresh_bridge.network_terminal_code(),
            readiness::TERMINAL_NONE
        );
        assert_eq!(wrapper.current_socket_epoch(), old_epoch + 1);
    }

    #[test]
    fn fault_publishers_carry_captured_epoch_to_registry_commit() {
        let stack = include_str!("stack_runner.rs");
        assert!(stack.contains("commit_socket_epoch_terminal_for(registry, epoch, code)"));

        let rx = include_str!("async_rx.rs");
        for name in [
            "publish_fatal",
            "publish_recovery_fault",
            "enter_drift_quarantine",
        ] {
            let start = rx.find(&format!("fn {name}")).unwrap();
            // 2200-char window had grown too tight; scan to the next top-level fn.
            let body_end = rx[start + 5..]
                .find("\n    fn ")
                .map(|i| start + 5 + i)
                .unwrap_or(rx.len());
            let body = &rx[start..body_end];
            assert!(
                body.contains("publish_fault_epoch_terminal(epoch"),
                "{name} must publish using its captured epoch"
            );
        }
        let helper = &rx[rx.find("fn publish_fault_epoch_terminal").unwrap()..];
        assert!(helper.contains("commit_socket_epoch_terminal_for(registry, epoch, code)"));
        assert!(helper.contains("commit_socket_epoch_fault_code(epoch, code)"));
    }

    #[test]
    fn terminal_publishers_use_registry_winner_before_listener_marker() {
        let stack = include_str!("stack_runner.rs");
        let start = stack.find("fn publish_terminal(&self").unwrap();
        let end = stack[start..].find("\n    }\n}").unwrap() + start;
        let body = &stack[start..end];

        assert!(
            body.contains("commit_socket_epoch_terminal_for"),
            "stack terminal publication must commit the registry before marking listeners"
        );
        assert!(
            !body.contains("mark_socket_epoch_closed_for"),
            "publishers must not pre-mark listeners with their losing terminal"
        );
    }

    #[test]
    fn stack_fault_epoch_is_captured_by_round_outcome() {
        let service = include_str!("service.rs");
        let outcome_start = service.find("pub(crate) struct StackRoundOutcome").unwrap();
        let outcome_end = service[outcome_start..].find("\n}\n").unwrap() + outcome_start;
        assert!(service[outcome_start..outcome_end].contains("fault_epoch: Option<u64>"));

        let stack = include_str!("stack_runner.rs");
        let publish_start = stack.find("fn publish_terminal(&self").unwrap();
        let publish_end = stack[publish_start..].find("\n    }\n}").unwrap() + publish_start;
        let publish = &stack[publish_start..publish_end];
        assert!(publish.contains("epoch: u64"));
        assert!(!publish.contains("current_socket_epoch"));
        assert!(stack.contains("publish_terminal(fault_epoch, outcome.fault_code)"));
    }

    #[test]
    fn registry_winner_drives_listener_marker_in_both_terminal_orders() {
        use axerrno::AxError;
        use smoltcp::wire::IpListenEndpoint;

        use crate::{readiness::NetworkTerminal, wrapper::SocketTestContext};

        fn run(first: NetworkTerminal, second: NetworkTerminal, expected: AxError, port: u16) {
            let ctx = SocketTestContext::leak_new();
            let (_handle, bridge) = ctx.sockets.add_public(new_tcp_socket());
            let epoch = ctx.sockets.current_socket_epoch();

            let first_commit = ctx.service.lock().commit_socket_epoch_terminal_for(
                ctx.sockets,
                epoch,
                first.code(),
            );
            let second_commit = ctx.service.lock().commit_socket_epoch_terminal_for(
                ctx.sockets,
                epoch,
                second.code(),
            );
            assert_eq!(first_commit, Some(true));
            assert_eq!(second_commit, Some(false));
            ctx.sockets.wake_socket_epoch(epoch);

            assert_eq!(bridge.network_terminal_code(), first.code());
            let mut sockets = ctx.sockets.inner.lock();
            let err = ctx
                .listen_table
                .listen_with_epoch(
                    IpListenEndpoint { addr: None, port },
                    Arc::new(ReadinessBridge::new()),
                    epoch,
                    &mut sockets,
                )
                .unwrap_err();
            assert_eq!(err, expected);
        }

        run(
            NetworkTerminal::DeviceIo,
            NetworkTerminal::LinkDown,
            AxError::Io,
            18101,
        );
        run(
            NetworkTerminal::LinkDown,
            NetworkTerminal::DeviceIo,
            AxError::NotConnected,
            18102,
        );
    }

    #[test]
    fn terminal_history_stays_bounded_across_repeated_epoch_flaps() {
        use crate::readiness::NetworkTerminal;

        let wrapper = SocketSetWrapper::new();
        for _ in 0..32 {
            let epoch = wrapper.current_socket_epoch();
            assert!(wrapper.close_socket_epoch(epoch, NetworkTerminal::DeviceIo));
            assert!(wrapper.terminal_history_len() <= 1);
            wrapper.open_next_socket_epoch().unwrap();
        }
        assert_eq!(wrapper.terminal_history_len(), 1);
    }

    #[test]
    fn retire_public_keeps_raw_handle_and_remove_raw_removes_it() {
        // T2.5-R2: public-metadata retirement (bridge + bound, waiter wake)
        // and the runner-only raw removal are separate steps; the raw
        // smoltcp handle survives retirement and `remove_raw` never wakes.
        let wrapper = SocketSetWrapper::new();
        let (handle, bridge) = wrapper.add_public(new_tcp_socket());
        let count = Arc::new(AtomicUsize::new(0));
        bridge.register(IoEvents::IN, &counting_waker(count.clone()));

        wrapper.retire_public(handle);

        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert!(wrapper.lookup_readiness(handle).is_none());
        assert!(wrapper.inner.lock().iter().any(|(h, _)| h == handle));

        wrapper.remove_raw(handle);

        assert!(!wrapper.inner.lock().iter().any(|(h, _)| h == handle));
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    // ── Task 3.1: global data-plane fault publication ───────────────────

    use axdriver::prelude::DevError;

    use crate::readiness;

    #[test]
    fn published_fault_wakes_waiters_that_observe_committed_global_code() {
        let wrapper: &'static SocketSetWrapper<'static> =
            Box::leak(Box::new(SocketSetWrapper::new()));
        let (_handle, bridge) = wrapper.add_public(new_tcp_socket());
        let observed = Arc::new(AtomicU64::new(0));
        struct GlobalObservingWake {
            wrapper: &'static SocketSetWrapper<'static>,
            observed: Arc<AtomicU64>,
        }
        impl alloc::task::Wake for GlobalObservingWake {
            fn wake(self: Arc<Self>) {
                self.observed
                    .store(self.wrapper.global_terminal_code(), Ordering::SeqCst);
            }
            fn wake_by_ref(self: &Arc<Self>) {
                self.observed
                    .store(self.wrapper.global_terminal_code(), Ordering::SeqCst);
            }
        }
        bridge.register(
            IoEvents::ERR,
            &Waker::from(Arc::new(GlobalObservingWake {
                wrapper,
                observed: observed.clone(),
            })),
        );

        wrapper.publish_global_fault(&DevError::Io);

        assert_eq!(
            observed.load(Ordering::SeqCst),
            readiness::dev_error_code(&DevError::Io)
        );
        assert_eq!(
            wrapper.global_terminal_code(),
            readiness::dev_error_code(&DevError::Io)
        );
        // The bridge keeps only socket-local state; the fault is observed
        // through the effective snapshot, never copied on publication.
        assert_eq!(bridge.terminal_code(), readiness::TERMINAL_NONE);
    }

    #[test]
    fn duplicate_publish_is_idempotent_and_wakes_once() {
        let wrapper = SocketSetWrapper::new();
        let (_handle, bridge) = wrapper.add_public(new_tcp_socket());
        let count = Arc::new(AtomicUsize::new(0));
        bridge.register(IoEvents::IN, &counting_waker(count.clone()));

        wrapper.publish_global_fault(&DevError::BadState);
        wrapper.publish_global_fault(&DevError::Io);
        wrapper.publish_global_fault(&DevError::BadState);

        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert_eq!(
            wrapper.global_terminal_code(),
            readiness::dev_error_code(&DevError::BadState)
        );
    }

    #[test]
    fn late_add_public_reports_global_fault_via_effective_snapshot() {
        // Late sockets observe a published fault through the effective
        // snapshot (global first) without any copy into their local code.
        let wrapper = SocketSetWrapper::new();
        wrapper.publish_global_fault(&DevError::NoMemory);

        let (_handle, late) = wrapper.add_public(new_tcp_socket());

        assert_eq!(late.terminal_code(), readiness::TERMINAL_NONE);
        assert_eq!(
            readiness::effective_terminal_code(
                wrapper.global_terminal_code(),
                late.terminal_code()
            ),
            readiness::dev_error_code(&DevError::NoMemory)
        );
    }

    #[test]
    fn install_readiness_reports_global_fault_via_effective_snapshot() {
        let wrapper = SocketSetWrapper::new();
        let (handle, _original) = wrapper.add_public(new_tcp_socket());
        wrapper.publish_global_fault(&DevError::Unsupported);

        let adopted = Arc::new(ReadinessBridge::new());
        wrapper.install_readiness(handle, adopted.clone());

        assert_eq!(adopted.terminal_code(), readiness::TERMINAL_NONE);
        assert_eq!(
            readiness::effective_terminal_code(
                wrapper.global_terminal_code(),
                adopted.terminal_code()
            ),
            readiness::dev_error_code(&DevError::Unsupported)
        );
    }

    #[test]
    fn global_publish_wakes_bridge_with_preexisting_local_code_and_effective_is_global() {
        // Task 3.1 RED witness: a bridge that already committed a
        // socket-local terminal (e.g. failed nonblocking connect) must still
        // be woken by the first global publication, the wake callback must
        // observe the already-committed global code, and the effective
        // snapshot must report the global category without overwriting the
        // local code.
        let wrapper: &'static SocketSetWrapper<'static> =
            Box::leak(Box::new(SocketSetWrapper::new()));
        let (_handle, bridge) = wrapper.add_public(new_tcp_socket());

        // Socket-local terminal commits before any global publication.
        bridge.commit_terminal(readiness::TERMINAL_CONNECT_REFUSED);

        struct GlobalObservingWake {
            wrapper: &'static SocketSetWrapper<'static>,
            observed: Arc<AtomicU64>,
        }
        impl alloc::task::Wake for GlobalObservingWake {
            fn wake(self: Arc<Self>) {
                self.observed
                    .store(self.wrapper.global_terminal_code(), Ordering::SeqCst);
            }
            fn wake_by_ref(self: &Arc<Self>) {
                self.observed
                    .store(self.wrapper.global_terminal_code(), Ordering::SeqCst);
            }
        }
        let count = Arc::new(AtomicUsize::new(0));
        let observed_global = Arc::new(AtomicU64::new(0));
        bridge.register(IoEvents::ERR, &counting_waker(count.clone()));
        bridge.register(
            IoEvents::ERR,
            &Waker::from(Arc::new(GlobalObservingWake {
                wrapper,
                observed: observed_global.clone(),
            })),
        );

        wrapper.publish_global_fault(&DevError::Io);

        assert_eq!(
            count.load(Ordering::Relaxed),
            1,
            "first global publication must wake a bridge holding a socket-local code"
        );
        assert_eq!(
            observed_global.load(Ordering::SeqCst),
            readiness::dev_error_code(&DevError::Io),
            "wake callback must observe the committed global code"
        );
        assert_eq!(
            bridge.terminal_code(),
            readiness::TERMINAL_CONNECT_REFUSED,
            "global publication must not overwrite the socket-local code"
        );
        assert_eq!(
            readiness::effective_terminal_code(
                wrapper.global_terminal_code(),
                bridge.terminal_code()
            ),
            readiness::dev_error_code(&DevError::Io),
            "effective snapshot must report the global category"
        );
    }

    /// Distinct wakers so a replaced PollSet slot is identifiable by its
    /// witness counter (mirrors the readiness.rs capacity pattern).
    fn distinct_wakers(n: usize) -> alloc::vec::Vec<(Arc<AtomicUsize>, Waker)> {
        (0..n)
            .map(|_| {
                let count = Arc::new(AtomicUsize::new(0));
                let waker = counting_waker(count.clone());
                (count, waker)
            })
            .collect()
    }

    #[test]
    fn global_publication_fans_out_to_every_waiter_count_despite_local_codes() {
        // Acceptance: 0/1/2/64/65 registered waiters all get exactly one
        // recheck opportunity from the first global publication even though
        // the bridge already holds a socket-local code; 65 follows axpoll's
        // wake-on-replacement for the displaced slot holder.
        let expected_total = |count: usize| -> usize { if count == 65 { 1 + 64 } else { count } };
        for count in [0usize, 1, 2, 64, 65] {
            let wrapper = SocketSetWrapper::new();
            let (_handle, bridge) = wrapper.add_public(new_tcp_socket());
            bridge.commit_terminal(readiness::TERMINAL_CONNECT_REFUSED);

            let waiters = distinct_wakers(count);
            for (_, waker) in &waiters {
                bridge.register(IoEvents::ERR, waker);
            }

            wrapper.publish_global_fault(&DevError::Io);

            let total: usize = waiters
                .iter()
                .map(|(counter, _)| counter.load(Ordering::Relaxed))
                .sum();
            assert_eq!(
                total,
                expected_total(count),
                "waiter count={count}: every waiter needs one recheck opportunity"
            );
            if count >= 1 && count <= 64 {
                for (counter, _) in &waiters {
                    assert_eq!(counter.load(Ordering::Relaxed), 1, "count={count}");
                }
            }
        }
    }

    #[test]
    fn source_global_publication_never_touches_socket_local_commit() {
        // Source guard: follow the publication call chain through the epoch
        // commit and wake helpers. It may commit the epoch-scoped network
        // terminal, but must never write a socket-local terminal, and every
        // wake remains in the helper that runs after registry guards drop.
        let src = include_str!("wrapper.rs");
        let start = src.find("pub fn publish_global_fault_code").unwrap();
        let end = src.find("pub fn lookup_readiness").unwrap();
        let publish_body = &src[start..end];
        assert!(!publish_body.contains(".commit_terminal("));
        let commit_call = publish_body
            .find("commit_socket_epoch_fault_code(epoch, code)")
            .unwrap();
        let wake_call = publish_body.find("wake_socket_epoch(epoch)").unwrap();
        assert!(commit_call < wake_call);

        let commit_start = src.find("fn commit_socket_epoch_with_codes").unwrap();
        let commit_end = src[commit_start..].find("/// Wakes all bridges").unwrap() + commit_start;
        let commit_helper = &src[commit_start..commit_end];
        assert!(commit_helper.contains("commit_network_terminal"));
        assert!(!commit_helper.contains(".commit_terminal("));

        let wake_start = src.find("pub(crate) fn wake_socket_epoch").unwrap();
        let wake_end = src[wake_start..]
            .find("/// Opens the next checked SocketEpoch")
            .unwrap()
            + wake_start;
        assert!(src[wake_start..wake_end].contains("wake_for_global_publication"));
        // add_public + install_readiness install bridges without reading or
        // copying the global code (the region ends at the next method).
        let install_start = src.find("pub fn add_public").unwrap();
        let install_end = src.find("pub fn global_terminal_code").unwrap();
        let install_body = &src[install_start..install_end];
        assert!(!install_body.contains("commit_terminal"));
        assert!(!install_body.contains("global_terminal"));
    }

    #[test]
    fn publication_ordering_holds_across_100_deterministic_cycles() {
        for _ in 0..100u32 {
            let wrapper = SocketSetWrapper::new();
            let (_handle, bridge) = wrapper.add_public(new_tcp_socket());
            let count = Arc::new(AtomicUsize::new(0));
            bridge.register(IoEvents::ERR, &counting_waker(count.clone()));

            wrapper.publish_global_fault(&DevError::Io);

            assert_eq!(count.load(Ordering::Relaxed), 1);
            assert_eq!(
                readiness::effective_terminal_code(
                    wrapper.global_terminal_code(),
                    bridge.terminal_code()
                ),
                readiness::dev_error_code(&DevError::Io)
            );
            assert_eq!(
                wrapper.global_terminal_code(),
                readiness::dev_error_code(&DevError::Io)
            );
        }
    }

    #[test]
    fn every_bridge_ends_committed_regardless_of_add_publish_interleaving() {
        // Linearization pin (Cycle risk note): the first-wins CAS precedes
        // the registry snapshot; sockets added after the snapshot observe
        // the committed code through the effective snapshot. No interleaving
        // can leave a public socket reporting anything but the global code.
        for _ in 0..10u32 {
            let wrapper: &'static SocketSetWrapper<'static> =
                Box::leak(Box::new(SocketSetWrapper::new()));
            let bridges: &'static spin::Mutex<std::vec::Vec<Arc<ReadinessBridge>>> =
                Box::leak(Box::new(spin::Mutex::new(std::vec::Vec::new())));
            let stop: &'static AtomicBool = Box::leak(Box::new(AtomicBool::new(false)));

            std::thread::scope(|scope| {
                scope.spawn(|| {
                    while !stop.load(Ordering::Relaxed) {
                        let (_h, bridge) = wrapper.add_public(new_tcp_socket());
                        bridges.lock().push(bridge);
                    }
                });
                scope.spawn(|| {
                    wrapper.publish_global_fault(&DevError::Io);
                });
                while wrapper.global_terminal_code() == 0 {
                    core::hint::spin_loop();
                }
                stop.store(true, Ordering::SeqCst);
            });

            let expected = readiness::dev_error_code(&DevError::Io);
            for bridge in bridges.lock().iter() {
                assert_eq!(
                    readiness::effective_terminal_code(
                        wrapper.global_terminal_code(),
                        bridge.terminal_code()
                    ),
                    expected
                );
            }
            assert_eq!(wrapper.global_terminal_code(), expected);
        }
    }

    // ── Task 5.1 Cycle 001: fixture-paired deferred removal owner ───────

    use super::SocketTestContext;

    #[test]
    fn fixture_context_pairs_service_with_its_own_registries() {
        // The three ownership units (sockets, listener table, deferred
        // Service) are one context: independently leaked per fixture and
        // never aliasing the production singletons.
        let a = SocketTestContext::leak_new();
        let b = SocketTestContext::leak_new();

        assert!(!core::ptr::eq(a.sockets, b.sockets));
        assert!(!core::ptr::eq(a.listen_table, b.listen_table));
        assert!(!core::ptr::eq(a.service, b.service));

        assert!(!core::ptr::eq(a.sockets, &*crate::SOCKET_SET));
        assert!(!core::ptr::eq(a.listen_table, &*crate::LISTEN_TABLE));
    }

    #[test]
    fn fixture_service_constructor_routes_the_local_registries_in_source() {
        // Source guard: the fixture Service is built against this context's
        // own listen table; the constructor never touches the production
        // global Service or socket set.
        let src = include_str!("wrapper.rs");
        let start = src.find("pub(crate) fn leak_new").unwrap();
        let end = src.find("impl<'a> SocketSetWrapper<'a>").unwrap();
        let ctor = &src[start..end];
        assert!(ctor.contains("new_with_listen_table"));
        assert!(ctor.contains("listen_table"));
        assert!(!ctor.contains("crate::SERVICE"));
        assert!(!ctor.contains("SOCKET_SET"));
    }
}
