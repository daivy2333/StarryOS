use alloc::{boxed::Box, sync::Arc, vec, vec::Vec};
use core::sync::atomic::{AtomicU64, Ordering};

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
    readiness: Mutex<HashMap<SocketHandle, Arc<ReadinessBridge>>>,
    /// Task 3.1: first-wins stable code of the single data-plane fatal
    /// (`readiness::TERMINAL_NONE` = none). Committed before the registry
    /// snapshot so every public socket either appears in the publication
    /// snapshot or observes the committed code through the effective
    /// snapshot (global first) once installed.
    global_terminal: AtomicU64,
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
            global_terminal: AtomicU64::new(readiness::TERMINAL_NONE),
        }
    }

    /// Adds a smoltcp socket and atomically installs a fresh per-public-handle
    /// bridge for it. The handle is not observable until `Socket` construction
    /// returns, so registry lookup can never miss an installed bridge. A
    /// published data-plane fault is observed through the effective snapshot
    /// (global first); no fault code is copied into the fresh bridge.
    pub fn add_public<T: AnySocket<'a>>(&self, socket: T) -> (SocketHandle, Arc<ReadinessBridge>) {
        let handle = self.inner.lock().add(socket);
        debug!("socket {}: created", handle);
        let bridge = Arc::new(ReadinessBridge::new());
        self.readiness.lock().insert(handle, bridge.clone());
        (handle, bridge)
    }

    /// Installs a bridge for a handle that already lives in the smoltcp set:
    /// the accept-adoption path for a hidden listener socket. The adopted
    /// bridge keeps only socket-local state; global faults are observed
    /// through the effective snapshot.
    pub fn install_readiness(&self, handle: SocketHandle, bridge: Arc<ReadinessBridge>) {
        self.readiness.lock().insert(handle, bridge);
    }

    /// Returns the committed global data-plane terminal code, if any.
    pub fn global_terminal_code(&self) -> u64 {
        self.global_terminal.load(Ordering::Acquire)
    }

    /// Publishes one concrete queue-owner [`DevError`] as the registry-wide
    /// terminal fault. Idempotent: only the first publication commits.
    ///
    /// Order: commit the global code (the linearization point), snapshot the
    /// bridges under the registry lock, release the lock, then wake each
    /// bridge unconditionally — no guard is held across any wake, and no
    /// socket-local code is written or overwritten. Waiters recheck and
    /// observe the effective snapshot (global first).
    pub fn publish_global_fault(&self, err: &DevError) {
        self.publish_global_fault_code(readiness::dev_error_code(err));
    }

    /// Publishes an already-encoded terminal code; see
    /// [`Self::publish_global_fault`].
    pub fn publish_global_fault_code(&self, code: u64) {
        if code == readiness::TERMINAL_NONE {
            return;
        }
        if self
            .global_terminal
            .compare_exchange(
                readiness::TERMINAL_NONE,
                code,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_err()
        {
            return;
        }
        let bridges: Vec<Arc<ReadinessBridge>> = self.readiness.lock().values().cloned().collect();
        for bridge in bridges {
            bridge.wake_for_global_publication();
        }
    }

    /// Task 2.1 contract requires a registry lookup; currently test-witness only,
    /// alive when a future path wakes by handle. Do not delete as "dead".
    #[allow(dead_code)]
    pub fn lookup_readiness(&self, handle: SocketHandle) -> Option<Arc<ReadinessBridge>> {
        self.readiness.lock().get(&handle).cloned()
    }

    /// Removes the public-handle bridge from the registry and wakes its
    /// leftover waiters. The wake happens after the registry guard drops.
    fn take_readiness(&self, handle: SocketHandle) -> Option<Arc<ReadinessBridge>> {
        let bridge = self.readiness.lock().remove(&handle);
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
        self.inner.lock().remove(handle);
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

    use alloc::{boxed::Box, sync::Arc};
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
        // Source guard: the publication path only wakes bridges; it must not
        // write socket-local codes, and socket install paths must not read
        // or copy the global code.
        let src = include_str!("wrapper.rs");
        let start = src.find("pub fn publish_global_fault_code").unwrap();
        let end = src.find("pub fn lookup_readiness").unwrap();
        let publish_body = &src[start..end];
        assert!(!publish_body.contains("commit_terminal"));
        assert!(publish_body.contains("wake_for_global_publication"));
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
