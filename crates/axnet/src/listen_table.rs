use alloc::{boxed::Box, collections::VecDeque, sync::Arc, vec::Vec};

use axerrno::{AxError, AxResult};
use axsync::Mutex;
use smoltcp::{
    iface::{SocketHandle, SocketSet},
    socket::tcp::{Socket, State},
    wire::IpListenEndpoint,
};

use crate::{SOCKET_SET, consts::LISTEN_QUEUE_SIZE, tcp::new_tcp_socket};

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
    idle: Option<SocketHandle>,
    queue: VecDeque<ListenSlot>,
}

impl ListenTableEntryInner {
    fn new(listen_endpoint: IpListenEndpoint, sockets: &mut SocketSet<'_>) -> Self {
        let mut entry = Self {
            listen_endpoint,
            idle: None,
            queue: VecDeque::with_capacity(LISTEN_QUEUE_SIZE),
        };
        entry.refill(sockets);
        entry
    }

    fn refill(&mut self, sockets: &mut SocketSet<'_>) {
        if self.idle.is_some() || self.queue.len() >= LISTEN_QUEUE_SIZE {
            return;
        }
        let mut socket = new_tcp_socket();
        socket
            .listen(self.listen_endpoint)
            .expect("validated nonzero TCP listen endpoint");
        self.idle = Some(sockets.add(socket));
    }

    fn reconcile(&mut self, sockets: &mut SocketSet<'_>) {
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
                    SlotState::Reset
                }
                _ => SlotState::Ready,
            };
        }

        let Some(handle) = self.idle else {
            self.refill(sockets);
            return;
        };
        if sockets.get::<Socket>(handle).state() == State::Listen {
            return;
        }

        self.idle = None;
        let state = match sockets.get::<Socket>(handle).state() {
            State::Closed => {
                sockets.remove(handle);
                SlotState::Reset
            }
            State::Listen => unreachable!(),
            State::SynReceived => SlotState::Pending,
            _ => SlotState::Ready,
        };
        self.queue.push_back(ListenSlot {
            handle: (state != SlotState::Reset).then_some(handle),
            state,
        });
        self.refill(sockets);
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
        }
    }

    pub fn can_listen(&self, port: u16) -> bool {
        self.tcp[port as usize].lock().is_none()
    }

    pub fn listen(&self, listen_endpoint: IpListenEndpoint) -> AxResult {
        let port = listen_endpoint.port;
        assert_ne!(port, 0);

        let mut sockets = SOCKET_SET.inner.lock();
        let mut entry = self.tcp[port as usize].lock();
        if entry.is_some() {
            warn!("socket already listening on port {port}");
            return Err(AxError::AddrInUse);
        }
        *entry = Some(Box::new(ListenTableEntryInner::new(
            listen_endpoint,
            &mut sockets,
        )));
        drop(entry);
        drop(sockets);
        self.active_ports.lock().push(port);
        Ok(())
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

    pub fn reconcile(&self, sockets: &mut SocketSet<'_>) {
        let active_ports = self.active_ports.lock().clone();
        for port in active_ports {
            if let Some(entry) = self.listen_entry(port).lock().as_mut() {
                entry.reconcile(sockets);
            }
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

    pub fn accept(&self, port: u16) -> AxResult<SocketHandle> {
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
        match slot.state {
            SlotState::Ready => Ok(slot.handle.expect("ready listener slot without handle")),
            SlotState::Reset => {
                warn!("accept failed: connection reset");
                Err(AxError::ConnectionReset)
            }
            SlotState::Pending => unreachable!(),
        }
    }
}
