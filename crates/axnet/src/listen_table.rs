use alloc::{boxed::Box, collections::VecDeque, sync::Arc, vec::Vec};
use core::{
    sync::atomic::{AtomicU16, AtomicU64, AtomicUsize, Ordering},
    task::Waker,
};

use axerrno::{AxError, AxResult};
use axpoll::IoEvents;
use axsync::Mutex;
use smoltcp::{
    iface::{SocketHandle, SocketSet},
    socket::tcp::{Socket, State},
    wire::IpListenEndpoint,
};

use crate::{
    SOCKET_SET, consts::LISTEN_QUEUE_SIZE, readiness::ReadinessBridge, service::STACK_STAGE_BUDGET,
    tcp::new_tcp_socket,
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

/// Task 2.6 replan: per-position verdict of `examine_slot`.
enum SlotExamine {
    /// Slot stays in the queue (Ready/Reset committed, or still Pending).
    Advance { changed: bool },
    /// Slot was removed (Listen recovery): the slot that shifted into `k`
    /// must be examined instead of skipping ahead.
    Stay,
}

/// Task 2.6 replan: observable result of one bounded listener-reconcile
/// stage, mirroring the deferred-retirement budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct ListenerReconcileOutcome {
    /// Budget tokens consumed this round (at most `STACK_STAGE_BUDGET`): one
    /// active-port head visit or one queue-slot visit of any state. Every
    /// position — Pending, Ready or Reset — costs exactly one token.
    pub(crate) checked: usize,
    /// True only while a listener sweep is unfinished because the round hit
    /// its budget or a later pass is due; the runner may self-wake once
    /// to continue it. False after a quiet complete pass.
    pub(crate) sweep_incomplete: bool,
}

/// Task 2.6 replan: bounded pass state of the listener reconciliation sweep.
/// One budget token is one active-port head visit or one queue-slot visit,
/// regardless of Pending/Ready/Reset state. A topology generation
/// invalidates the running pass on every listen/unlisten so no live listener
/// is skipped, and any protocol progress observed while a pass is active is
/// latched into one further bounded pass before a quiet complete pass parks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct ReconcileCursor {
    /// Next active-port index to visit.
    port: usize,
    /// Next queue-slot index within that port's entry.
    slot: usize,
    /// True once the current port's head has been visited this pass.
    head_visited: bool,
    /// Topology generation observed when the pass began; a listen/unlisten
    /// mismatch restarts the pass from index 0.
    generation: u64,
    /// True while a pass is in progress.
    sweeping: bool,
    /// Protocol progress arrived while the pass was active: run one further
    /// bounded pass when the current pass completes.
    rescan: bool,
}

struct ListenTableEntryInner {
    listen_endpoint: IpListenEndpoint,
    accept: Arc<ReadinessBridge>,
    idle: Option<SocketHandle>,
    queue: VecDeque<ListenSlot>,
    /// T2.8-R1: exact head-signal target armed on this entry's idle hidden
    /// socket, so its transition identifies the entry without any scan.
    head_signal: Arc<HeadSignalWaker>,
}

/// T2.8-R1: pre-reserved exact-head-signal state shared by every listener
/// entry's hidden-socket waker (producer) and the runner-side consumer.
/// Lossless by construction: the per-port dedup bit keeps at most one queued
/// instance per port, so ring capacity `PORT_NUM` can never overflow.
struct HeadSignals {
    /// Dedup bitmap indexed by port: bit set while that port has an
    /// unconsumed signal in the ring.
    pending: Box<[AtomicU64]>,
    /// FIFO ring of signaled ports (`0` unused; listeners assert nonzero).
    slots: Box<[AtomicU16]>,
    head: AtomicUsize,
    tail: AtomicUsize,
}

impl HeadSignals {
    fn new() -> Self {
        Self {
            pending: (0..PORT_NUM / 64).map(|_| AtomicU64::new(0)).collect(),
            slots: (0..PORT_NUM).map(|_| AtomicU16::new(0)).collect(),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
        }
    }

    fn signal(&self, port: u16) {
        debug_assert_ne!(port, 0);
        let idx = port as usize;
        let mask = 1u64 << (idx % 64);
        if self.pending[idx / 64].fetch_or(mask, Ordering::AcqRel) & mask != 0 {
            return;
        }
        self.enqueue(port);
    }

    /// Single-producer enqueue. Producers are serialized: a hidden-socket
    /// waker only fires while its mutator holds the global SocketSet guard,
    /// so Lamport SPSC reserve-then-publish ordering suffices.
    fn enqueue(&self, port: u16) {
        let tail = self.tail.load(Ordering::Relaxed);
        debug_assert!(
            tail.wrapping_sub(self.head.load(Ordering::Relaxed)) < PORT_NUM,
            "deduplicated signals cannot exceed one slot per port"
        );
        self.slots[tail & (PORT_NUM - 1)].store(port, Ordering::Release);
        self.tail.store(tail.wrapping_add(1), Ordering::Release);
    }

    fn pop(&self) -> Option<u16> {
        let head = self.head.load(Ordering::Relaxed);
        if head == self.tail.load(Ordering::Acquire) {
            return None;
        }
        let port = self.slots[head & (PORT_NUM - 1)].load(Ordering::Acquire);
        self.head.store(head.wrapping_add(1), Ordering::Release);
        Some(port)
    }

    /// Clears the dedup bit; called by the consumer BEFORE repairing so a
    /// transition observed during or after the repair re-signals cleanly.
    fn clear_pending(&self, port: u16) {
        let idx = port as usize;
        self.pending[idx / 64].fetch_and(!(1u64 << (idx % 64)), Ordering::Release);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.tail
            .load(Ordering::Relaxed)
            .wrapping_sub(self.head.load(Ordering::Relaxed))
    }
}

/// T2.8-R1: one-shot recv target of exactly ONE listener's idle hidden
/// socket. Waking records a bounded deduplicated signal only: it never
/// allocates, never takes an entry/SocketSet/Service lock and never wakes
/// application accept waiters — the staged drain after the committed repair
/// does that.
struct HeadSignalWaker {
    port: u16,
    signals: Arc<HeadSignals>,
}

impl HeadSignalWaker {
    fn waker(self: &Arc<Self>) -> Waker {
        Waker::from(self.clone())
    }
}

impl alloc::task::Wake for HeadSignalWaker {
    fn wake(self: Arc<Self>) {
        self.signals.signal(self.port);
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.signals.signal(self.port);
    }
}

impl ListenTableEntryInner {
    fn new(
        listen_endpoint: IpListenEndpoint,
        accept: Arc<ReadinessBridge>,
        signals: &Arc<HeadSignals>,
        sockets: &mut SocketSet<'_>,
    ) -> Self {
        let mut entry = Self {
            listen_endpoint,
            accept,
            idle: None,
            queue: VecDeque::with_capacity(LISTEN_QUEUE_SIZE),
            head_signal: Arc::new(HeadSignalWaker {
                port: listen_endpoint.port,
                signals: signals.clone(),
            }),
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
        // Task 2.3 + T2.8-R1: the idle hidden socket's one-shot recv slot
        // records an exact head signal instead of waking the accept bridge,
        // so application wake stays staged until the transition commits.
        sockets
            .get_mut::<Socket>(handle)
            .register_recv_waker(&self.head_signal.waker());
        self.idle = Some(handle);
        info!(
            "listen {}: refilled idle hidden socket {}",
            self.listen_endpoint.port, handle
        );
    }

    /// Task 2.6 replan: O(1) head position of one listener — transitions the
    /// idle hidden socket (if it left `Listen`), refills a missing idle, and
    /// re-arms the accept bridge on the idle socket. Mirrors the old
    /// unconditional idle/refill block of `reconcile`.
    fn reconcile_head(&mut self, sockets: &mut SocketSet<'_>) -> bool {
        let mut changed = false;
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
        if let Some(handle) = self.idle {
            sockets
                .get_mut::<Socket>(handle)
                .register_recv_waker(&self.head_signal.waker());
        }
        changed
    }

    /// Task 2.6 replan: O(1) examination of one queue slot at `k`. Pending
    /// slots read the live socket state and commit Ready/Reset, or recover a
    /// `SynReceived -> Listen` revert (restore as idle / remove the
    /// redundant raw socket). Committed slots are skipped. Every visited
    /// live socket is re-armed to the accept bridge (its one-shot recv slot
    /// cleared on the last transition).
    fn examine_slot(&mut self, sockets: &mut SocketSet<'_>, k: usize) -> SlotExamine {
        let slot = &mut self.queue[k];
        if slot.state != SlotState::Pending {
            return SlotExamine::Advance { changed: false };
        }
        let handle = slot.handle.expect("pending listener slot without handle");
        match sockets.get::<Socket>(handle).state() {
            State::Listen => {
                // A SynReceived socket reset back to Listen no longer owns a
                // backlog slot: restore it as the idle listener, or remove
                // the redundant raw socket when an idle already exists.
                let redundant = self.idle.is_some();
                if redundant {
                    sockets.remove(handle);
                    debug!(
                        "listen {}: redundant {} removed",
                        self.listen_endpoint.port, handle
                    );
                } else {
                    self.idle = Some(handle);
                    debug!(
                        "listen {}: {} restored as idle",
                        self.listen_endpoint.port, handle
                    );
                }
                self.queue.remove(k);
                SlotExamine::Stay
            }
            State::SynReceived => {
                sockets
                    .get_mut::<Socket>(handle)
                    .register_recv_waker(&self.accept.recv_waker());
                SlotExamine::Advance { changed: false }
            }
            State::Closed => {
                sockets.remove(handle);
                slot.handle = None;
                slot.state = SlotState::Reset;
                debug!(
                    "listen {}: slot {} aborted -> Reset",
                    self.listen_endpoint.port, handle
                );
                SlotExamine::Advance { changed: true }
            }
            _ => {
                slot.state = SlotState::Ready;
                debug!(
                    "listen {}: slot {} -> Ready",
                    self.listen_endpoint.port, handle
                );
                SlotExamine::Advance { changed: true }
            }
        }
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
    /// Task 2.6 replan: cross-round rotating cursor of the bounded listener
    /// reconciliation sweep (port index, queue-slot index, sweep flag).
    reconcile_cursor: Mutex<ReconcileCursor>,
    /// Listener structure generation bumped by listen/unlisten and external
    /// queue removals, so a running sweep restarts from a safe position
    /// instead of trusting stale port or slot indices.
    structure_generation: AtomicU64,
    /// T2.8-R1: pre-reserved exact-head-signal ring shared by every entry's
    /// hidden-socket waker; consumed by the runner after each ingress packet.
    head_signals: Arc<HeadSignals>,
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
            reconcile_cursor: Mutex::new(ReconcileCursor::default()),
            structure_generation: AtomicU64::new(0),
            head_signals: Arc::new(HeadSignals::new()),
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
            &self.head_signals,
            sockets,
        )));
        drop(entry);
        self.active_ports.lock().push(port);
        self.structure_generation.fetch_add(1, Ordering::Release);
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
        self.structure_generation.fetch_add(1, Ordering::Release);
    }

    /// Test-only seam: unregisters a listener whose hidden sockets live in the
    /// caller-owned `SocketSet` instead of the production global, mirroring
    /// [`Self::listen_with`].
    #[cfg(test)]
    pub(crate) fn unlisten_with(&self, port: u16, sockets: &mut SocketSet<'_>) {
        if let Some(entry) = self.tcp[port as usize].lock().take() {
            entry.cleanup(sockets);
        }
        self.active_ports.lock().retain(|&active| active != port);
        self.structure_generation.fetch_add(1, Ordering::Release);
    }

    fn listen_entry(&self, port: u16) -> Arc<Mutex<Option<Box<ListenTableEntryInner>>>> {
        self.tcp[port as usize].clone()
    }

    /// Commits hidden transitions under Service + SocketSet guards; staged
    /// wakes drain only after those guards release.
    ///
    /// Task 2.6 replan: one bounded reconciliation stage per runner round. A
    /// cross-round pass cursor walks at most `STACK_STAGE_BUDGET` budget
    /// tokens per round, where one token is an active-port head visit or one
    /// queue-slot visit of any state (Pending, Ready or Reset). There is no
    /// full active-port count/clone/lock pre-pass: the pass starts and
    /// resumes from cursor state alone. A listen/unlisten bumps
    /// `structure_generation`, restarting the running pass from a safe
    /// position after listener topology or external queue mutation; protocol
    /// progress seen while a pass is active — primary or later — requests one
    /// further bounded pass so a transition observed outside the pass's
    /// snapshot is reconciled before the runner parks.
    pub(crate) fn reconcile(
        &self,
        sockets: &mut SocketSet<'_>,
        protocol_progressed: bool,
    ) -> ListenerReconcileOutcome {
        let mut staged = Vec::new();
        let mut checked = 0usize;
        {
            let mut cursor = self.reconcile_cursor.lock();
            if cursor.sweeping && protocol_progressed {
                // Progress seen while a pass is unfinished is latched instead
                // of dropped: it may mark a transition outside this pass's
                // snapshot.
                if !cursor.rescan {
                    cursor.rescan = true;
                }
                // A new observation is also a reason to re-serve the current
                // port's head on this same round (idle transition + refill)
                // instead of only trailing the slot walk: under a connect
                // storm the inbound SYN rate outruns a multi-round
                // committed-slot pass, and a stale busy idle would leave no
                // `Listen` socket to accept the next SYN. Quiet continuations
                // never reach here, so the 513-token quiet-pass accounting is
                // unchanged.
                cursor.head_visited = false;
            }
            let ports = self.active_ports.lock();
            if ports.is_empty() {
                cursor.sweeping = false;
                return ListenerReconcileOutcome::default();
            }
            // A listen/unlisten or external queue removal since the pass began
            // invalidates its port/slot indices: restart from a safe position
            // (bounded duplicate visits are fine, skipping a live slot is not).
            let generation = self.structure_generation.load(Ordering::Acquire);
            if cursor.generation != generation {
                cursor.generation = generation;
                cursor.port = 0;
                cursor.slot = 0;
                cursor.head_visited = false;
            }
            // Start a fresh pass only when there is a reason: a latched
            // rescan or protocol progress. A quiet idle table parks.
            if !cursor.sweeping {
                if !cursor.rescan && !protocol_progressed {
                    return ListenerReconcileOutcome::default();
                }
                cursor.sweeping = true;
                cursor.rescan = false;
                cursor.port = 0;
                cursor.slot = 0;
                cursor.head_visited = false;
                cursor.generation = generation;
            }
            while checked < STACK_STAGE_BUDGET {
                if cursor.port >= ports.len() {
                    break;
                }
                let port = ports[cursor.port];
                match self.tcp[port as usize].lock().as_mut() {
                    None => {
                        // Port unlistened between rounds: advance without a
                        // token; the next call's generation check restarts.
                        cursor.port += 1;
                        cursor.slot = 0;
                        cursor.head_visited = false;
                    }
                    Some(entry) => {
                        if !cursor.head_visited {
                            // Token: the port's head (idle transition + refill
                            // + accept rearm), once per pass.
                            if entry.reconcile_head(sockets) {
                                staged.push(port);
                            }
                            cursor.head_visited = true;
                            checked += 1;
                        } else {
                            // A queue that shrunk below the cursor (accept or
                            // recovery removal) must not skip a shifted
                            // pending slot: re-scan the live region.
                            if cursor.slot > entry.queue.len() {
                                cursor.slot = 0;
                            }
                            if cursor.slot >= entry.queue.len() {
                                // The port's live positions are covered:
                                // advance without a token.
                                cursor.port += 1;
                                cursor.slot = 0;
                                cursor.head_visited = false;
                            } else {
                                // One queue-slot visit costs one token for
                                // every state: committed Ready/Reset slots
                                // yield a static `Advance` verdict instead of
                                // an unbudgeted inline skip.
                                let changed = match entry.examine_slot(sockets, cursor.slot) {
                                    // Recovery removed the slot; the shifted
                                    // slot stays at the same index.
                                    SlotExamine::Stay => true,
                                    SlotExamine::Advance { changed } => {
                                        cursor.slot += 1;
                                        changed
                                    }
                                };
                                if changed {
                                    staged.push(port);
                                }
                                checked += 1;
                            }
                        }
                    }
                }
            }
            // A pass parks only after it completes without newer progress or
            // a topology change. Progress observed during any active pass —
            // primary or later — requests one further bounded pass, so a
            // transition is never dropped based on which pass is running.
            if cursor.port >= ports.len() {
                if cursor.rescan {
                    cursor.rescan = false;
                    cursor.port = 0;
                    cursor.slot = 0;
                    cursor.head_visited = false;
                    cursor.generation = self.structure_generation.load(Ordering::Acquire);
                } else {
                    cursor.sweeping = false;
                }
            }
            if !staged.is_empty() {
                self.pending_accept_wakes.lock().extend(staged);
            }
            let sweep_incomplete = cursor.sweeping;
            ListenerReconcileOutcome {
                checked,
                sweep_incomplete,
            }
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

    /// T2.8-R1: consumes at most one exact head signal and runs only that
    /// entry's O(1) `reconcile_head` under the caller's Service + SocketSet
    /// guards (the runner's fixed order). A stale identifier — the port was
    /// unlistened between signal and consume — is safely discarded, and a
    /// repaired transition stages its accept wake like any other committed
    /// change. Returns whether a repair ran.
    pub(crate) fn consume_head_signal(&self, sockets: &mut SocketSet<'_>) -> bool {
        let Some(port) = self.head_signals.pop() else {
            return false;
        };
        // Clear the dedup bit before repairing: a transition observed during
        // or after this repair must be able to enqueue a fresh signal.
        self.head_signals.clear_pending(port);
        let changed = match self.listen_entry(port).lock().as_mut() {
            Some(entry) => entry.reconcile_head(sockets),
            None => false,
        };
        if changed {
            self.pending_accept_wakes.lock().push(port);
        }
        changed
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

    /// Task 3.1 (D4): inspects the first consumable accept outcome for
    /// readiness. `Some(true)` when the head commits a Reset, `Some(false)`
    /// when it commits a Ready connection, `None` when nothing is consumable.
    /// Pending slots are transparent.
    pub(crate) fn accept_head_is_reset(&self, port: u16) -> Option<bool> {
        let entry = self.listen_entry(port);
        let guard = entry.lock();
        let entry = guard.as_ref()?;
        entry
            .queue
            .iter()
            .find(|slot| slot.state != SlotState::Pending)
            .map(|slot| slot.state == SlotState::Reset)
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
        // The active reconcile cursor indexes this queue. Publish the shape
        // change without taking the cursor lock; the next bounded stage uses
        // the generation mismatch to restart from a safe position.
        self.structure_generation.fetch_add(1, Ordering::Release);
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

    /// Task 3.1 test seam: queues one consumable Reset slot for `port`
    /// (readiness witness injection). Production paths never call this.
    #[cfg(test)]
    pub(crate) fn test_push_reset_slot(&self, port: u16) {
        if let Some(entry) = self.listen_entry(port).lock().as_mut() {
            entry.queue.push_back(ListenSlot {
                handle: None,
                state: SlotState::Reset,
            });
        }
    }

    /// T2.8-R1 test seam: records a head signal for `port` exactly like the
    /// hidden-socket waker would (unit-level signal injection).
    #[cfg(test)]
    pub(crate) fn test_signal_head(&self, port: u16) {
        self.head_signals.signal(port);
    }

    /// T2.8-R1 test seam: number of signals currently queued (inspection
    /// only; proves dedup coalescing and losslessness).
    #[cfg(test)]
    pub(crate) fn test_pending_head_signals(&self) -> usize {
        self.head_signals.len()
    }

    /// T2.8-R1 test seam: closes the entry's idle hidden socket under the
    /// caller's SocketSet guard (a real head transition), mirroring the
    /// module-internal unit helper.
    #[cfg(test)]
    pub(crate) fn test_close_idle(&self, port: u16, sockets: &mut SocketSet<'_>) -> bool {
        let idle = match self.listen_entry(port).lock().as_ref() {
            Some(entry) => entry.idle,
            None => return false,
        };
        let Some(idle) = idle else {
            return false;
        };
        sockets.get_mut::<Socket>(idle).close();
        true
    }

    /// T2.7-R1 test seam: whether an idle hidden listener is present.
    #[cfg(test)]
    pub(crate) fn test_idle_is_some(&self, port: u16) -> bool {
        self.listen_entry(port)
            .lock()
            .as_ref()
            .is_some_and(|entry| entry.idle.is_some())
    }

    /// Task 2.6 replan test seam: appends `count` real hidden sockets (fresh
    /// `Closed`-state TCP sockets, instantly committable Reset commits) as
    /// Pending-marked queue slots, so the bounded reconcile budget is
    /// observable without a real handshake.
    #[cfg(test)]
    pub(crate) fn test_seed_closed_slots(
        &self,
        port: u16,
        sockets: &mut SocketSet<'_>,
        count: usize,
    ) {
        let entry_lock = self.listen_entry(port);
        let mut guard = entry_lock.lock();
        let entry = guard.as_mut().expect("test listener registered");
        for _ in 0..count {
            let handle = sockets.add(new_tcp_socket());
            entry.queue.push_back(ListenSlot {
                handle: Some(handle),
                state: SlotState::Pending,
            });
        }
    }

    /// Task 2.6 replan test seam: moves the entry's current idle hidden
    /// socket into the queue as a Pending-marked slot (its live state stays
    /// `Listen`), so `reconcile` must exercise the SynReceived->Listen
    /// recovery ownership paths. Returns the moved handle.
    #[cfg(test)]
    pub(crate) fn test_park_idle_as_pending_slot(&self, port: u16) -> SocketHandle {
        let entry_lock = self.listen_entry(port);
        let mut guard = entry_lock.lock();
        let entry = guard.as_mut().expect("test listener registered");
        let idle = entry.idle.take().expect("test listener needs an idle");
        entry.queue.push_back(ListenSlot {
            handle: Some(idle),
            state: SlotState::Pending,
        });
        idle
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
            &table.head_signals,
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
    fn closing_idle_records_head_signal_and_stages_accept_wake() {
        // T2.8-R1: the idle hidden socket's one-shot recv slot carries the
        // exact head-signal waker instead of the accept bridge — a transition
        // must never wake application accept waiters before its repair
        // commits. The staged drain after commit remains the only wake path.
        let (table, mut sockets, bridge) = session(18080);
        let count = Arc::new(AtomicUsize::new(0));
        bridge.register(IoEvents::IN, &counting_waker(count.clone()));

        assert!(close_idle(&table, 18080, &mut sockets));
        assert_eq!(
            count.load(Ordering::Relaxed),
            0,
            "no direct accept wake from an uncommitted transition"
        );
        assert_eq!(table.test_pending_head_signals(), 1);

        assert!(table.consume_head_signal(&mut sockets));
        assert_eq!(table.test_pending_head_signals(), 0);
        table.drain_accept_wakes();
        assert_eq!(count.load(Ordering::Relaxed), 1, "staged accept wake");
    }

    #[test]
    fn duplicate_head_signals_coalesce_into_one_repair() {
        // T2.8-R1 (S3): repeated wakes of one listener dedup to a single
        // queued signal; consuming it once empties the ring and a further
        // consume is a quiet no-op. The natural close-time wake plus two
        // injected duplicates still produce exactly one real repair.
        let (table, mut sockets, _) = session(18302);
        assert!(close_idle(&table, 18302, &mut sockets));
        table.test_signal_head(18302);
        table.test_signal_head(18302);
        assert_eq!(table.test_pending_head_signals(), 1);

        let sockets_before = sockets.iter().count();
        assert!(table.consume_head_signal(&mut sockets));
        assert_eq!(table.test_pending_head_signals(), 0);
        assert!(!table.consume_head_signal(&mut sockets));
        // One committed transition: closed idle removed, Reset slot queued,
        // fresh idle refilled — the set size is unchanged.
        assert_eq!(table.test_queue_len(18302), 1);
        assert!(table.test_idle_is_some(18302));
        assert_eq!(sockets.iter().count(), sockets_before);
    }

    #[test]
    fn head_signal_repairs_only_the_signaled_listener() {
        // T2.8-R1 (S2): a signal identifies its exact entry. Consuming it
        // must repair that entry and leave every other listener's state and
        // cursors untouched (no active-port scan, no wrong-entry repair).
        for _ in 0..100 {
            let table = ListenTable::new();
            let mut sockets = SocketSet::new(vec![]);
            *table.tcp[18300usize].lock() = Some(Box::new(ListenTableEntryInner::new(
                endpoint(18300),
                Arc::new(ReadinessBridge::new()),
                &table.head_signals,
                &mut sockets,
            )));
            *table.tcp[18301usize].lock() = Some(Box::new(ListenTableEntryInner::new(
                endpoint(18301),
                Arc::new(ReadinessBridge::new()),
                &table.head_signals,
                &mut sockets,
            )));
            table.active_ports.lock().extend([18300u16, 18301]);

            let untouched_idle = {
                let entry = table.tcp[18301].lock();
                entry.as_ref().unwrap().idle
            };
            assert!(close_idle(&table, 18300, &mut sockets));
            assert_eq!(table.test_pending_head_signals(), 1);

            assert!(table.consume_head_signal(&mut sockets));
            assert_eq!(table.test_queue_len(18300), 1);
            assert!(table.test_idle_is_some(18300));

            let entry = table.tcp[18301].lock();
            assert_eq!(
                entry.as_ref().unwrap().idle,
                untouched_idle,
                "the unsignaled listener must not be touched"
            );
            drop(entry);
            assert_eq!(table.test_queue_len(18301), 0);
            assert_eq!(table.test_pending_head_signals(), 0);
        }
    }

    #[test]
    fn stale_head_signal_after_unlisten_is_discarded_safely() {
        // T2.8-R1 (S3): an unlisten race leaves a stale identifier; consuming
        // it must be a harmless discard with no panic and no state change,
        // and re-listening on the same port keeps working.
        for _ in 0..100 {
            let (table, mut sockets, _) = session(18303);
            assert!(close_idle(&table, 18303, &mut sockets));
            assert_eq!(table.test_pending_head_signals(), 1);

            table.unlisten_with(18303, &mut sockets);
            assert!(!table.consume_head_signal(&mut sockets));
            assert_eq!(table.test_pending_head_signals(), 0);
            assert_eq!(sockets.iter().count(), 0, "cleanup removed all handles");

            let accept = Arc::new(ReadinessBridge::new());
            table
                .listen_with(endpoint(18303), accept, &mut sockets)
                .expect("re-listen after stale signal");
            assert!(table.test_idle_is_some(18303));
        }
    }

    #[test]
    fn quiet_listener_table_consumes_nothing_and_repairs_nothing() {
        // T2.8-R1 (S3): without signals the micro-step is a quiet no-op —
        // no repair, no allocation of work, no change to the entry.
        for _ in 0..100 {
            let (table, mut sockets, _) = session(18304);
            let idle_before = {
                let entry = table.tcp[18304].lock();
                entry.as_ref().unwrap().idle
            };
            for _ in 0..32 {
                assert!(!table.consume_head_signal(&mut sockets));
            }
            let entry = table.tcp[18304].lock();
            assert_eq!(entry.as_ref().unwrap().idle, idle_before);
            assert_eq!(entry.as_ref().unwrap().queue.len(), 0);
        }
    }

    #[test]
    fn reconcile_stages_transition_and_drain_wakes_after_commit() {
        let (table, mut sockets, bridge) = session(18081);
        // The closing hidden socket already consumed its direct slot wake
        // with no waiter registered; the staged drain is now the only path
        // that can wake a waiter registered after the transition.
        assert!(close_idle(&table, 18081, &mut sockets));
        table.reconcile(&mut sockets, true);

        let count = Arc::new(AtomicUsize::new(0));
        bridge.register(IoEvents::IN, &counting_waker(count.clone()));
        assert_eq!(count.load(Ordering::Relaxed), 0);

        table.drain_accept_wakes();
        assert_eq!(count.load(Ordering::Relaxed), 1);
        table.drain_accept_wakes();
        assert_eq!(count.load(Ordering::Relaxed), 1);

        let second = Arc::new(AtomicUsize::new(0));
        bridge.register(IoEvents::IN, &counting_waker(second.clone()));
        table.reconcile(&mut sockets, true);
        table.drain_accept_wakes();
        assert_eq!(second.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn reset_transition_removes_hidden_handle_and_reconcile_rearms_idle() {
        let (table, mut sockets, bridge) = session(18082);
        assert!(close_idle(&table, 18082, &mut sockets));
        table.reconcile(&mut sockets, true);
        table.drain_accept_wakes();

        let count = Arc::new(AtomicUsize::new(0));
        bridge.register(IoEvents::IN, &counting_waker(count.clone()));

        // Reconcile rearms the fresh idle hidden socket; closing it wakes the
        // accept bridge again (one-shot slot restored).
        assert!(close_idle(&table, 18082, &mut sockets));
        table.reconcile(&mut sockets, true);
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
    fn accept_head_readiness_inspects_first_consumable_slot() {
        use crate::readiness;

        let (table, mut sockets, bridge) = session(18085);
        assert_eq!(table.accept_head_is_reset(18085), None);

        // Pending slots are transparent to readiness.
        let parked = table.test_park_idle_as_pending_slot(18085);
        assert!(table.tcp[18085].lock().as_ref().unwrap().idle.is_none());
        let _ = parked;
        assert_eq!(table.accept_head_is_reset(18085), None);

        // The first committed Ready slot decides.
        let ready_handle = {
            let mut socket = new_tcp_socket();
            socket.listen(endpoint(18085)).unwrap();
            sockets.add(socket)
        };
        table.tcp[18085]
            .lock()
            .as_mut()
            .unwrap()
            .queue
            .push_back(ListenSlot {
                handle: Some(ready_handle),
                state: SlotState::Ready,
            });
        assert_eq!(table.accept_head_is_reset(18085), Some(false));

        // Reset behind it becomes the head only after the Ready item leaves.
        table.tcp[18085]
            .lock()
            .as_mut()
            .unwrap()
            .queue
            .push_back(ListenSlot {
                handle: None,
                state: SlotState::Reset,
            });
        assert_eq!(
            table.accept_with(18085, &mut sockets).unwrap(),
            ready_handle
        );
        assert_eq!(table.accept_head_is_reset(18085), Some(true));

        let count = Arc::new(AtomicUsize::new(0));
        bridge.register(IoEvents::ERR, &counting_waker(count.clone()));
        assert!(matches!(
            table.accept_with(18085, &mut sockets),
            Err(axerrno::AxError::ConnectionReset)
        ));
        assert_eq!(table.accept_head_is_reset(18085), None);

        // Readiness never turns a transient reset into a permanent terminal.
        assert_eq!(bridge.terminal_code(), readiness::TERMINAL_NONE);
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
        table.reconcile(&mut sockets, true);
        table.drain_accept_wakes();

        for counter in &waiters {
            assert_eq!(counter.load(Ordering::Relaxed), 1);
        }
    }

    // ── Task 2.6 replan: bounded listener reconciliation ───────────────

    #[test]
    fn reconcile_checks_at_most_32_positions_per_round_and_converges() {
        // Task 2.6 replan (S1): 31/32/33/512 pending-slot workloads must
        // examine at most STACK_STAGE_BUDGET positions per round and
        // converge over later rounds; the last round parks the sweep.
        use crate::service::STACK_STAGE_BUDGET;

        for _ in 0..100 {
            for count in [31usize, 32, 33, 512] {
                let (table, mut sockets, _) = session(18200);
                table.test_seed_closed_slots(18200, &mut sockets, count);
                let mut total_checked = 0usize;
                let mut rounds = 0usize;
                loop {
                    rounds += 1;
                    assert!(
                        rounds <= 20,
                        "count {count}: no convergence after {rounds} rounds"
                    );
                    // T2.6-R4: `true` models the observed progress that starts
                    // the pass; later rounds are quiet self-wake continuations.
                    let outcome = table.reconcile(&mut sockets, rounds == 1);
                    assert!(
                        outcome.checked <= STACK_STAGE_BUDGET,
                        "count {count} round {rounds}: checked {} > {STACK_STAGE_BUDGET}",
                        outcome.checked
                    );
                    total_checked += outcome.checked;
                    if !outcome.sweep_incomplete {
                        break;
                    }
                }
                // Every seeded closed socket was examined (committed to Reset)
                // at least once; the head position of the port was also counted.
                assert!(
                    total_checked >= count,
                    "count {count}: only {total_checked} positions examined"
                );
                let entry = table.tcp[18200].lock();
                let entry = entry.as_ref().unwrap();
                assert_eq!(entry.queue.len(), count, "count {count}: Reset markers");
                assert!(
                    entry
                        .queue
                        .iter()
                        .all(|slot| slot.state == SlotState::Reset),
                    "count {count}: every seeded slot must have been committed"
                );
                assert!(
                    sockets.iter().count() <= 1,
                    "count {count}: only the idle stays"
                );
            }
        }
    }

    #[test]
    fn reconcile_rotates_across_multiple_listeners_fairly() {
        // Task 2.6 replan (S2): two listeners with pending backlogs are
        // served by the cross-round rotating cursor without port starvation;
        // both queues converge to fully-committed slots over the same sweep.
        for _ in 0..100 {
            let table = ListenTable::new();
            let mut sockets = SocketSet::new(vec![]);
            let b1 = Arc::new(ReadinessBridge::new());
            let b2 = Arc::new(ReadinessBridge::new());
            *table.tcp[18204usize].lock() = Some(Box::new(ListenTableEntryInner::new(
                endpoint(18204),
                b1,
                &table.head_signals,
                &mut sockets,
            )));
            *table.tcp[18205usize].lock() = Some(Box::new(ListenTableEntryInner::new(
                endpoint(18205),
                b2,
                &table.head_signals,
                &mut sockets,
            )));
            table.active_ports.lock().extend([18204u16, 18205]);
            table.test_seed_closed_slots(18204, &mut sockets, 48);
            table.test_seed_closed_slots(18205, &mut sockets, 48);

            let mut rounds = 0usize;
            loop {
                rounds += 1;
                assert!(rounds <= 6, "multi-listener sweep did not converge");
                let outcome = table.reconcile(&mut sockets, rounds == 1);
                assert!(outcome.checked <= 32);
                if !outcome.sweep_incomplete {
                    break;
                }
            }
            for port in [18204u16, 18205] {
                let entry = table.tcp[port as usize].lock();
                let entry = entry.as_ref().unwrap();
                assert_eq!(entry.queue.len(), 48, "port {port}: Reset markers");
                assert!(
                    entry
                        .queue
                        .iter()
                        .all(|slot| slot.state == SlotState::Reset),
                    "port {port}: every seeded slot committed"
                );
                assert!(entry.idle.is_some(), "port {port}: idle survives");
            }
        }
    }

    #[test]
    fn reconcile_cursor_survives_accept_removal_between_rounds() {
        // Task 2.6 replan (S3): removing slots between rounds (accept
        // consuming committed slots) must keep the rotating cursor clamped;
        // the sweep still converges and never re-examines a removed slot.
        for _ in 0..100 {
            let (table, mut sockets, _) = session(18201);
            table.test_seed_closed_slots(18201, &mut sockets, 33);

            let first = table.reconcile(&mut sockets, true);
            assert_eq!(first.checked, 32);
            assert!(first.sweep_incomplete);

            // Consume committed Reset/Ready slots like accept does (ConnectionReset).
            for _ in 0..4 {
                assert!(matches!(
                    table.accept_with(18201, &mut sockets),
                    Err(axerrno::AxError::ConnectionReset)
                ));
            }
            let mut total_checked = first.checked;
            for _round in 0..8 {
                let outcome = table.reconcile(&mut sockets, false);
                total_checked += outcome.checked;
                if !outcome.sweep_incomplete {
                    break;
                }
            }
            assert!(
                total_checked >= 33,
                "the remaining seeded slots must all be examined"
            );
        }
    }

    #[test]
    fn reconcile_cursor_survives_small_accept_removal_with_large_queue() {
        // Cycle 009 S1/S5: a small front removal must invalidate an active
        // pass even while the remaining queue is still longer than the
        // numeric cursor. The following quiet rounds model the software-only
        // runner wake after accept; they provide no fabricated protocol
        // transition that could hide a skipped slot.
        for _ in 0..100 {
            let (table, mut sockets, _) = session(18217);
            table.test_seed_closed_slots(18217, &mut sockets, 64);

            let first = table.reconcile(&mut sockets, true);
            assert_eq!(first.checked, crate::service::STACK_STAGE_BUDGET);
            assert!(first.sweep_incomplete);

            assert!(matches!(
                table.accept_with(18217, &mut sockets),
                Err(axerrno::AxError::ConnectionReset)
            ));
            assert_eq!(table.test_queue_len(18217), 63);

            let mut rounds = 0usize;
            loop {
                rounds += 1;
                assert!(rounds <= 4, "mutated sweep did not converge");
                let outcome = table.reconcile(&mut sockets, false);
                assert!(outcome.checked <= crate::service::STACK_STAGE_BUDGET);
                if !outcome.sweep_incomplete {
                    break;
                }
            }

            let entry = table.tcp[18217].lock();
            let entry = entry.as_ref().unwrap();
            assert_eq!(entry.queue.len(), 63);
            assert!(
                entry
                    .queue
                    .iter()
                    .all(|slot| slot.state == SlotState::Reset),
                "accept shifted an unvisited live slot behind the cursor"
            );
        }
    }

    #[test]
    fn reconcile_recovers_listen_slot_as_idle_when_none_exists() {
        // Task 2.6 replan (S4, unit): a Pending slot whose live socket
        // reverted from SynReceived to Listen — and no idle listener exists —
        // must be restored as the idle hidden socket and removed from the
        // backlog, so pending counts do not leak and accept keeps working.
        for _ in 0..100 {
            let (table, mut sockets, _) = session(18202);
            let parked = table.test_park_idle_as_pending_slot(18202);
            // Full queue: the head's refill is blocked, so reconciliation must
            // restore the parked Listen socket as the idle instead of creating
            // a fresh one (the promote path).
            table.test_seed_closed_slots(18202, &mut sockets, LISTEN_QUEUE_SIZE - 1);
            assert_eq!(table.test_queue_len(18202), LISTEN_QUEUE_SIZE);
            assert!(!table.test_idle_is_some(18202));

            let outcome = table.reconcile(&mut sockets, true);

            assert_eq!(table.test_queue_len(18202), LISTEN_QUEUE_SIZE - 1);
            assert!(table.test_idle_is_some(18202));
            let entry = table.tcp[18202].lock();
            assert_eq!(entry.as_ref().unwrap().idle, Some(parked));
            drop(entry);
            assert!(outcome.checked >= 1);
        }
    }

    #[test]
    fn reconcile_recovers_listen_slot_by_removing_redundant_socket() {
        // Task 2.6 replan (S4, unit): with an idle listener present, a
        // Pending slot whose socket reverted to Listen is redundant: the raw
        // socket is removed from the set together with its slot, and the
        // existing idle survives.
        for _ in 0..100 {
            let (table, mut sockets, _) = session(18203);
            let redundant = {
                let socket = {
                    let mut socket = new_tcp_socket();
                    socket.listen(endpoint(18203)).unwrap();
                    socket
                };
                sockets.add(socket)
            };
            {
                let mut guard = table.tcp[18203].lock();
                guard.as_mut().unwrap().queue.push_back(ListenSlot {
                    handle: Some(redundant),
                    state: SlotState::Pending,
                });
            }
            assert!(
                table.test_idle_is_some(18203),
                "session starts with an idle"
            );

            let _ = table.reconcile(&mut sockets, true);

            assert!(
                !sockets.iter().any(|(h, _)| h == redundant),
                "the redundant revert socket must be removed from the set"
            );
            assert_eq!(table.test_queue_len(18203), 0);
            assert!(table.test_idle_is_some(18203));
        }
    }

    #[test]
    fn reconcile_latches_progress_during_sweep_into_follow_up_pass() {
        // T2.6-R1 (S7): protocol progress arriving while a listener sweep is
        // unfinished is latched into a bounded follow-up pass instead of being
        // discarded: the current pass still finishes, the follow-up re-covers
        // the table, and only a quiet complete pass parks. The Cycle 006
        // cursor dropped in-flight progress — it parked as soon as its
        // snapshot count hit zero, so a transition observed outside the
        // snapshot could miss reconciliation until the next protocol event.
        // Cycle 008 makes the latch pass-independent (T2.6-R4) and charges
        // every slot state one token (T2.6-R3), so the quiet continuation of
        // the follow-up traversal itself is bounded and parks only after a
        // clean pass.
        for _ in 0..100 {
            let (table, mut sockets, _) = session(18210);
            table.test_seed_closed_slots(18210, &mut sockets, 33);
            // Round 1: the head + 31 slots (32 positions); the sweep is
            // unfinished.
            let first = table.reconcile(&mut sockets, true);
            assert_eq!(first.checked, 32);
            assert!(first.sweep_incomplete);

            // Round 2: protocol progress while the sweep is active must arm a
            // follow-up pass (the Cycle 006 cursor ignored it and parked).
            let second = table.reconcile(&mut sockets, true);
            assert!(
                second.sweep_incomplete,
                "progress during an unfinished sweep must arm a bounded follow-up pass"
            );

            // Quiet continuation: the follow-up traversal re-covers each
            // committed slot for one token and parks only after a clean pass.
            let mut rounds = 0usize;
            loop {
                rounds += 1;
                assert!(rounds <= 6, "no convergence after {rounds} rounds");
                let outcome = table.reconcile(&mut sockets, false);
                assert!(
                    outcome.checked <= crate::service::STACK_STAGE_BUDGET,
                    "quiet continuation must stay bounded"
                );
                if !outcome.sweep_incomplete {
                    break;
                }
            }
            let entry = table.tcp[18210].lock();
            let entry = entry.as_ref().unwrap();
            assert_eq!(entry.queue.len(), 33, "all seeded slots stay committed");
            assert!(
                entry
                    .queue
                    .iter()
                    .all(|slot| slot.state == SlotState::Reset),
                "every seeded slot must be committed"
            );
        }
    }

    #[test]
    fn reconcile_charges_one_token_per_committed_slot_and_head() {
        // T2.6-R3 (S1): every queue position — Pending, Ready or Reset — costs
        // exactly one budget token. A listener with 512 committed Reset slots
        // plus its head completes one quiet pass in exactly 513 tokens across
        // 17 bounded rounds (32 per round), with no unbudgeted inline skip.
        // The Cycle 007 cursor read all 512 committed slots inside one free
        // `while` loop, so this witness fails there (1 token / 1 round).
        use crate::service::STACK_STAGE_BUDGET;

        for _ in 0..100 {
            let (table, mut sockets, _) = session(18215);
            {
                let mut entry = table.tcp[18215].lock();
                let entry = entry.as_mut().unwrap();
                for _ in 0..LISTEN_QUEUE_SIZE {
                    entry.queue.push_back(ListenSlot {
                        handle: None,
                        state: SlotState::Reset,
                    });
                }
            }
            let mut rounds = 0usize;
            let mut total = 0usize;
            loop {
                rounds += 1;
                assert!(rounds <= 20, "no convergence after {rounds} rounds");
                // The first call starts the pass on the observed progress;
                // continuations are quiet self-wakes of the in-flight sweep.
                let outcome = table.reconcile(&mut sockets, rounds == 1);
                assert!(
                    outcome.checked <= STACK_STAGE_BUDGET,
                    "round {rounds}: checked {} > {STACK_STAGE_BUDGET}",
                    outcome.checked
                );
                total += outcome.checked;
                if !outcome.sweep_incomplete {
                    break;
                }
            }
            assert_eq!(
                total,
                LISTEN_QUEUE_SIZE + 1,
                "head + 512 committed positions must cost exactly 513 tokens"
            );
            assert_eq!(
                rounds, 17,
                "513 tokens at 32 per round span exactly 17 rounds"
            );
            // Committed state is final: the pass must not mutate the queue.
            let entry = table.tcp[18215].lock();
            let entry = entry.as_ref().unwrap();
            assert_eq!(entry.queue.len(), LISTEN_QUEUE_SIZE);
            assert!(
                entry
                    .queue
                    .iter()
                    .all(|slot| slot.state == SlotState::Reset),
                "the sweep must never touch committed slots"
            );
        }
    }

    #[test]
    fn reconcile_retains_progress_observed_during_later_traversal_pass() {
        // T2.6-R4 (S2): a new progress observation during a SECOND/LATER
        // traversal pass — not just the primary pass — must also arm a
        // subsequent bounded pass. Cycle 007's `!follow_up` immunity dropped
        // in-flight progress during the follow-up pass, letting it park with
        // a possibly-unreconciled transition.
        for _ in 0..100 {
            let (table, mut sockets, _) = session(18216);
            table.test_seed_closed_slots(18216, &mut sockets, 33);

            // Round 1: start the primary pass (head + 31 slots = 32 positions).
            let first = table.reconcile(&mut sockets, true);
            assert_eq!(first.checked, 32);
            assert!(first.sweep_incomplete);

            // Round 2: progress while the primary pass is active arms the
            // follow-up traversal.
            let second = table.reconcile(&mut sockets, true);
            assert!(second.sweep_incomplete);

            // Round 3: the follow-up traversal is now running. New progress
            // observed during IT must arm yet another pass. RED witness:
            // Cycle 007 parks here (`!follow_up` suppressed the latch and the
            // committed-slot skip finished the pass in one round).
            let third = table.reconcile(&mut sockets, true);
            assert!(
                third.sweep_incomplete,
                "a progress observation during a later traversal pass must arm a subsequent pass"
            );

            // New-code witness: quiet continuation keeps every pass bounded
            // and eventually parks after a clean pass.
            let mut rounds = 0usize;
            loop {
                rounds += 1;
                assert!(rounds <= 8, "no convergence after {rounds} rounds");
                let outcome = table.reconcile(&mut sockets, false);
                assert!(
                    outcome.checked <= crate::service::STACK_STAGE_BUDGET,
                    "quiet continuation must stay bounded"
                );
                if !outcome.sweep_incomplete {
                    break;
                }
            }
            let entry = table.tcp[18216].lock();
            let entry = entry.as_ref().unwrap();
            assert_eq!(entry.queue.len(), 33);
            assert!(
                entry
                    .queue
                    .iter()
                    .all(|slot| slot.state == SlotState::Reset),
                "every seeded slot must be committed"
            );
        }
    }

    #[test]
    fn reconcile_visits_listener_added_mid_sweep() {
        // T2.6-R1 (S8): a listener registered while a sweep is running must
        // not be permanently skipped. The Cycle 006 sweep computed its
        // position snapshot once; a port added afterwards was never visited
        // before the sweep parked, so its hidden transitions stayed
        // unreconciled. A topology generation invalidates the running pass
        // and restarts it from a safe position.
        for _ in 0..100 {
            let (table, mut sockets, _) = session(18211);
            table.test_seed_closed_slots(18211, &mut sockets, 33);
            let first = table.reconcile(&mut sockets, true);
            assert_eq!(first.checked, 32);
            assert!(first.sweep_incomplete);

            // Mid-sweep: register a second listener, then close its idle
            // hidden socket. The transition must be reconciled by the running
            // sweep before it parks.
            let accept = Arc::new(ReadinessBridge::new());
            table
                .listen_with(endpoint(18212), accept, &mut sockets)
                .unwrap();
            assert!(close_idle(&table, 18212, &mut sockets));

            let _ = table.reconcile(&mut sockets, true);
            let _ = table.reconcile(&mut sockets, false);
            assert_eq!(
                table.test_queue_len(18212),
                1,
                "a listener added mid-sweep must be reconciled before parking"
            );
        }
    }

    #[test]
    fn reconcile_bounded_33_active_listeners_without_pre_pass() {
        // T2.6-R1 (S9): a round is bounded by position count even with more
        // active listeners than the budget: 33 listeners cost exactly 33 head
        // positions, split 32 + 1 across two rounds, with no full active-port
        // count/clone/lock pre-pass outside the budget. Each round reports at
        // most `STACK_STAGE_BUDGET` checked positions and the sweep parks only
        // after every live listener was visited.
        for _ in 0..100 {
            let table = ListenTable::new();
            let mut sockets = SocketSet::new(vec![]);
            for i in 0..33u16 {
                let port = 18230 + i;
                *table.tcp[port as usize].lock() = Some(Box::new(ListenTableEntryInner::new(
                    endpoint(port),
                    Arc::new(ReadinessBridge::new()),
                    &table.head_signals,
                    &mut sockets,
                )));
                table.active_ports.lock().push(port);
            }

            let mut total_checked = 0usize;
            let mut rounds = 0usize;
            loop {
                rounds += 1;
                assert!(rounds <= 4, "no convergence after {rounds} rounds");
                let outcome = table.reconcile(&mut sockets, rounds == 1);
                assert!(
                    outcome.checked <= crate::service::STACK_STAGE_BUDGET,
                    "round {rounds}: checked {} > budget",
                    outcome.checked
                );
                total_checked += outcome.checked;
                if !outcome.sweep_incomplete {
                    break;
                }
            }
            assert!(
                total_checked >= 33,
                "every active listener's head position is visited at least once"
            );
            assert_eq!(
                total_checked % 33,
                0,
                "each covered pass pays exactly one head token per listener"
            );
            assert!(rounds >= 2, "a 33-port pass must span a round boundary");
        }
    }

    #[test]
    fn reconcile_cursor_survives_listener_removed_mid_sweep() {
        // T2.6-R1 (S10): removing a listener mid-sweep must not leave a stale
        // handle in cursor state or starve a live listener; the sweep
        // converges over the survivors and commits every remaining slot.
        for _ in 0..100 {
            let table = ListenTable::new();
            let mut sockets = SocketSet::new(vec![]);
            *table.tcp[18213usize].lock() = Some(Box::new(ListenTableEntryInner::new(
                endpoint(18213),
                Arc::new(ReadinessBridge::new()),
                &table.head_signals,
                &mut sockets,
            )));
            *table.tcp[18214usize].lock() = Some(Box::new(ListenTableEntryInner::new(
                endpoint(18214),
                Arc::new(ReadinessBridge::new()),
                &table.head_signals,
                &mut sockets,
            )));
            table.active_ports.lock().extend([18213u16, 18214]);
            table.test_seed_closed_slots(18213, &mut sockets, 33);

            let first = table.reconcile(&mut sockets, true);
            assert_eq!(first.checked, 32);
            assert!(first.sweep_incomplete);

            // Remove the second listener mid-sweep.
            table.unlisten_with(18214, &mut sockets);

            let mut rounds = 0usize;
            loop {
                rounds += 1;
                assert!(rounds <= 8, "sweep did not converge after removal");
                let outcome = table.reconcile(&mut sockets, false);
                if !outcome.sweep_incomplete {
                    break;
                }
            }
            {
                let entry = table.tcp[18213].lock();
                let entry = entry.as_ref().unwrap();
                assert!(
                    entry
                        .queue
                        .iter()
                        .all(|slot| slot.state == SlotState::Reset),
                    "every surviving listener slot was committed"
                );
            }
            assert_eq!(table.active_ports.lock().len(), 1);
        }
    }
}
