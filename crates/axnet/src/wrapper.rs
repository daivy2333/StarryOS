use alloc::{sync::Arc, vec};

use axerrno::{AxError, AxResult};
use axpoll::IoEvents;
use axsync::Mutex;
use hashbrown::HashMap;
use smoltcp::{
    iface::{SocketHandle, SocketSet},
    socket::{AnySocket, Socket},
    wire::{IpAddress, IpListenEndpoint},
};

use crate::readiness::ReadinessBridge;

pub(crate) struct SocketSetWrapper<'a> {
    pub inner: Mutex<SocketSet<'a>>,
    tcp_bound: Mutex<HashMap<SocketHandle, IpListenEndpoint>>,
    readiness: Mutex<HashMap<SocketHandle, Arc<ReadinessBridge>>>,
}

impl<'a> SocketSetWrapper<'a> {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(SocketSet::new(vec![])),
            tcp_bound: Mutex::new(HashMap::new()),
            readiness: Mutex::new(HashMap::new()),
        }
    }

    /// Adds a smoltcp socket and atomically installs a fresh per-public-handle
    /// bridge for it. The handle is not observable until `Socket` construction
    /// returns, so registry lookup can never miss an installed bridge.
    pub fn add_public<T: AnySocket<'a>>(&self, socket: T) -> (SocketHandle, Arc<ReadinessBridge>) {
        let handle = self.inner.lock().add(socket);
        debug!("socket {}: created", handle);
        let bridge = Arc::new(ReadinessBridge::new());
        self.readiness.lock().insert(handle, bridge.clone());
        (handle, bridge)
    }

    /// Installs a bridge for a handle that already lives in the smoltcp set:
    /// the accept-adoption path for a hidden listener socket.
    pub fn install_readiness(&self, handle: SocketHandle, bridge: Arc<ReadinessBridge>) {
        self.readiness.lock().insert(handle, bridge);
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

    use alloc::sync::Arc;
    use core::{
        sync::atomic::{AtomicUsize, Ordering},
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
}
