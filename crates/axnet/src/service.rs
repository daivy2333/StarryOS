use core::{sync::atomic::Ordering, task::Waker};

use axdriver::prelude::{DevError, DevResult};
use axdriver_net::NetQueueDirection;
use axhal::time::{NANOS_PER_MICROS, wall_time_nanos};
use smoltcp::{
    iface::{Interface, PollIngressSingleResult, PollResult, SocketHandle, SocketSet},
    socket::tcp::State,
    time::Instant,
    wire::{HardwareAddress, IpAddress},
};

use crate::{
    async_rx::{QUEUE_EVENT, RX_TELEMETRY, SpaceDecision},
    device::{RxCopyStep, TxDropReason, TxReclaimStep, TxSubmitStep},
    flush::{FlushRecheck, FlushTicket, FlushWaiter, error_code, error_from_code},
    router::{Router, RxOwnerView},
};

fn now() -> Instant {
    Instant::from_micros_const((wall_time_nanos() / NANOS_PER_MICROS) as i64)
}

pub(crate) const STACK_STAGE_BUDGET: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StageStep {
    Idle,
    Processed,
    SocketStateChanged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StageOutcome {
    processed: usize,
    budget_exhausted: bool,
    socket_changed: bool,
}

fn run_bounded_stage(mut step: impl FnMut() -> StageStep) -> StageOutcome {
    let mut processed = 0usize;
    let mut socket_changed = false;
    while processed < STACK_STAGE_BUDGET {
        match step() {
            StageStep::Idle => break,
            StageStep::Processed => processed += 1,
            StageStep::SocketStateChanged => {
                processed += 1;
                socket_changed = true;
            }
        }
    }
    StageOutcome {
        processed,
        budget_exhausted: processed == STACK_STAGE_BUDGET,
        socket_changed,
    }
}

/// Observable result of one bounded smoltcp stack round.
#[derive(Debug)]
pub(crate) struct StackRoundOutcome {
    pub(crate) work: usize,
    pub(crate) backlog: bool,
    pub(crate) self_yield: bool,
    pub(crate) socket_changed: bool,
    pub(crate) rx_ready: bool,
    pub(crate) rx_space_woken: bool,
    pub(crate) tx_enqueued: bool,
    /// Task 3.1: stable code of the concrete terminal fault observed by any
    /// stage of this round (`readiness::TERMINAL_NONE` = none). The error
    /// identity must reach the public fault publisher uncollapsed.
    pub(crate) fault_code: u64,
    pub(crate) protocol_deadline: Option<Instant>,
    pub(crate) requires_polling: bool,
    /// Task 2.6 replan: deferred-close entries examined this round (≤
    /// `STACK_STAGE_BUDGET`) and reclaimed exactly once.
    pub(crate) deferred_checked: usize,
    pub(crate) deferred_reclaimed: usize,
    /// True only while a bounded deferred sweep is still unfinished; the
    /// runner may self-wake once to finish it. After a complete sweep it is
    /// false regardless of the deferred list length.
    pub(crate) deferred_sweep_incomplete: bool,
    /// Task 2.6 replan: listener hidden-slot positions examined this round
    /// (≤ `STACK_STAGE_BUDGET`) and whether the bounded listener sweep still
    /// has more to examine.
    pub(crate) listener_checked: usize,
    pub(crate) listener_sweep_incomplete: bool,
    /// T2.8-R1: exact head micro-repairs executed after processed ingress
    /// packets this round (≤ processed ingress ≤ `STACK_STAGE_BUDGET`).
    pub(crate) listener_head_repairs: usize,
}

/// Which half of a deferred TCP close still needs peer acknowledgment
/// before the resident runner may reclaim the raw socket handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CloseKind {
    /// The local endpoint initiated the close; the FIN (and any queued TX)
    /// must be acknowledged before the handle is safe to remove.
    Active,
    /// The peer closed first and the local close entered `LastAck`; the
    /// FIN ACK must fully close the connection before removal.
    LastAck,
    /// A UDP socket was dropped while a datagram was still queued in its TX
    /// buffer (undispatched). The resident runner dispatches the datagram in
    /// its egress rounds and the reaper removes the raw handle once the TX
    /// buffer is empty — closing the socket (`close()` resets the TX buffer)
    /// would silently drop the queued datagram.
    UdpQueued,
}

impl CloseKind {
    /// True when a current smoltcp state proves the local close is fully
    /// acknowledged, so the runner may remove the raw handle.
    fn is_confirmed(self, state: State) -> bool {
        match self {
            Self::Active => matches!(state, State::FinWait2 | State::TimeWait | State::Closed),
            Self::LastAck => matches!(state, State::TimeWait | State::Closed),
            // UDP has no close protocol; dispatch progress is the reaper's
            // UDP-specific verdict (TX drained), never a TCP state.
            Self::UdpQueued => false,
        }
    }
}

/// A raw TCP handle whose close needs runner-owned protocol progress
/// before it can be removed from the smoltcp set.
#[derive(Debug, Clone, Copy)]
struct DeferredRemoval {
    handle: SocketHandle,
    kind: CloseKind,
}

/// Observable result of one bounded deferred-retirement stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct DeferredReapOutcome {
    /// Entries examined this round (at most `STACK_STAGE_BUDGET`).
    pub(crate) checked: usize,
    /// Handles reclaimed this round (confirmed or stale entries dropped).
    pub(crate) reclaimed: usize,
    /// True only while a bounded deferred sweep is still unfinished; the
    /// runner may self-wake once to finish it. After a complete sweep it is
    /// false regardless of the deferred list length.
    pub(crate) sweep_incomplete: bool,
}

/// Per-entry decision of the bounded deferred-removal stage (Task 2.6 replan).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeferredVerdict {
    /// Raw handle reached its close-confirmed state: remove it from the
    /// socket set and drop the deferred entry.
    Reap,
    /// Close is still unconfirmed: keep the deferred entry.
    Keep,
    /// Handle is gone or its slot was re-used by another type: drop the
    /// deferred entry without touching the socket set.
    Drop,
}

pub struct Service {
    pub iface: Interface,
    router: Router,
    target_dev: Option<usize>,
    flush_waiter: Option<FlushWaiter>,
    flush_next_identity: u64,
    /// RW-3: persisted terminal data-plane fault (error code). A submit/
    /// reclaim fault is recorded even when no flush waiter exists, and is
    /// never cleared by a waiter completing, so every flush constructed after
    /// the fault returns the same stable error instead of hanging on a live
    /// target whose owner has already stopped.
    flush_terminal_fault: Option<u64>,
    /// Flush successes (waiter completed with `Done`).
    flush_success: u64,
    /// Flush faults (waiter completed with a terminal error).
    flush_error: u64,
    /// Flush `ResourceBusy` rejections.
    flush_busy: u64,
    /// Flush cancellations (waiter dropped before completion).
    flush_cancel: u64,
    /// Service-owned QEMU diagnostic lease (D9): the committed hold mode.
    /// `HOLD_NONE` means no hold. Only the `qemu-diagnostics` feature build
    /// carries this state; ordinary axnet and D1 expose no control entry.
    #[cfg(feature = "qemu-diagnostics")]
    diag_hold_mode: u64,
    /// Service-owned lease expiry deadline in wall nanoseconds (0 = no hold).
    #[cfg(feature = "qemu-diagnostics")]
    diag_lease_expiry_nanos: u64,
    /// Service-owned count of lease-expiry auto-releases (saturating,
    /// monotonic telemetry; not a synchronization primitive).
    #[cfg(feature = "qemu-diagnostics")]
    diag_auto_release_failure: u64,
    /// Task 5.2 (Iteration 006): per-test fixture clock. `diag_hold_tick`
    /// reads this when attached; production never sets it (wall clock).
    #[cfg(all(test, feature = "qemu-diagnostics"))]
    diag_test_clock: Option<crate::diag::DiagTestClock>,
    /// Test-only local listener table so a full-chain witness can run
    /// `stack_round` against a caller-owned `ListenTable` whose hidden
    /// sockets live in the injected SocketSet instead of the production
    /// global. `new()` points this at the global table.
    #[cfg(test)]
    listen_table: &'static crate::listen_table::ListenTable,
    /// Raw TCP handles whose close commit still needs peer ACK; the runner
    /// reaps each exactly once when its smoltcp state proves confirmation.
    deferred_removals: alloc::vec::Vec<DeferredRemoval>,
    /// Task 2.6 replan: rotating position of the next deferred entry to
    /// examine. `swap_remove` and stale/reused drop keep it valid: it is
    /// re-clamped to the current length on every sweep step.
    deferred_cursor: usize,
    /// Task 2.6 replan: how many live entries the current bounded sweep has
    /// left to examine (0 = no sweep in progress). Counts down per round and
    /// restarts only when the sweep completed and new entries exist, so a
    /// >32-entry deferred list finishes through a bounded self-wake cascade
    /// instead of a busy loop.
    deferred_remaining: usize,
    /// Task 2.6 replan: a new deferred removal was enqueued since the sweep
    /// completed (or the Service was created), so the next round may start a
    /// fresh sweep even without protocol progress.
    deferred_dirty: bool,
}
impl Service {
    pub fn new(mut router: Router, target_dev: Option<usize>) -> Self {
        let config = smoltcp::iface::Config::new(HardwareAddress::Ip);
        let iface = Interface::new(config, &mut router, now());

        Self {
            iface,
            router,
            target_dev,
            flush_waiter: None,
            flush_next_identity: 0,
            flush_terminal_fault: None,
            flush_success: 0,
            flush_error: 0,
            flush_busy: 0,
            flush_cancel: 0,
            #[cfg(feature = "qemu-diagnostics")]
            diag_hold_mode: crate::diag::HOLD_NONE,
            #[cfg(feature = "qemu-diagnostics")]
            diag_lease_expiry_nanos: 0,
            #[cfg(feature = "qemu-diagnostics")]
            diag_auto_release_failure: 0,
            #[cfg(all(test, feature = "qemu-diagnostics"))]
            diag_test_clock: None,
            #[cfg(test)]
            listen_table: &*crate::LISTEN_TABLE,
            deferred_removals: alloc::vec::Vec::new(),
            deferred_cursor: 0,
            deferred_remaining: 0,
            deferred_dirty: false,
        }
    }

    #[cfg(test)]
    fn listen_table(&self) -> &'static crate::listen_table::ListenTable {
        self.listen_table
    }

    #[cfg(not(test))]
    fn listen_table(&self) -> &'static crate::listen_table::ListenTable {
        &*crate::LISTEN_TABLE
    }

    /// Test-only constructor: `stack_round`'s listener reconcile uses a
    /// caller-owned table instead of the production global.
    #[cfg(test)]
    pub(crate) fn new_with_listen_table(
        router: Router,
        target_dev: Option<usize>,
        listen_table: &'static crate::listen_table::ListenTable,
    ) -> Self {
        let mut service = Self::new(router, target_dev);
        service.listen_table = listen_table;
        service
    }

    /// Task 5.2 (Iteration 006): attaches a per-test fixture clock so this
    /// Service's lease deadline reads the fixture's time instead of the
    /// process-global `diag::diag_now()`. Test-only; production never sets it.
    #[cfg(all(test, feature = "qemu-diagnostics"))]
    pub(crate) fn attach_test_clock(&mut self, clock: crate::diag::DiagTestClock) {
        self.diag_test_clock = Some(clock);
    }

    pub fn poll(&mut self, owner: RxOwnerView, sockets: &mut SocketSet) -> bool {
        let outcome = self.stack_round(now(), owner, sockets);
        outcome.socket_changed || outcome.rx_ready || outcome.self_yield
    }

    /// Enqueues a raw TCP handle whose close still needs runner-owned
    /// progress. Holds the Service guard only (never the SocketSet guard):
    /// the runner's `SERVICE -> SOCKET_SET` order. Duplicate requests are
    /// collapsed to one entry.
    pub(crate) fn queue_deferred_removal(&mut self, handle: SocketHandle, kind: CloseKind) {
        if !self.deferred_removals.iter().any(|d| d.handle == handle) {
            self.deferred_removals
                .push(DeferredRemoval { handle, kind });
            // A fresh entry is a reason to start a new sweep even if the
            // previous sweep completed without any protocol progress.
            self.deferred_dirty = true;
        }
    }

    /// Reclaims deferred raw TCP handles whose close protocol reached a
    /// confirmed state, bounded to `STACK_STAGE_BUDGET` examinations per
    /// round with a rotating cursor. Called by `stack_round` after
    /// egress/Router dispatch and before `poll_at` is recomputed, so a
    /// doomed deferred deadline can never park the runner. Runs under the
    /// Service + SocketSet guards (the runner's fixed order).
    ///
    /// The stage is fair across rounds: `deferred_cursor` keeps a rotating
    /// position so newly-appended entries do not starve older ones, and
    /// `swap_remove` (which moves the tail into the cursor slot) plus a per-
    /// sweep remaining count keep every live entry examined at most once per
    /// sweep. Stale handles (gone or re-typed) drop the entry without
    /// touching the set.
    /// Reclaims deferred raw TCP handles whose close protocol reached a
    /// confirmed state, bounded to `STACK_STAGE_BUDGET` examinations per
    /// round. Called by `stack_round` after egress/Router dispatch and
    /// before `poll_at` is recomputed, so a doomed deferred deadline can
    /// never park the runner. Runs under the Service + SocketSet guards.
    ///
    /// A "sweep" spans multiple rounds and covers the entries present when
    /// it starts: `deferred_remaining` counts how many are left to examine,
    /// and `deferred_cursor` keeps the rotating position across rounds, so
    /// a >32-entry list is finished by a bounded self-wake cascade before
    /// the runner parks for a protocol event or `poll_at` deadline — a
    /// non-empty list alone never sustains self-wakes. `swap_remove` and
    /// stale/re-typed drop keep the cursor valid.
    fn reap_deferred_removals(
        &mut self,
        sockets: &mut SocketSet,
        protocol_progressed: bool,
    ) -> DeferredReapOutcome {
        let mut checked = 0usize;
        let mut reclaimed = 0usize;
        // Start a sweep only when there is a reason to examine entries: an
        // unfinished sweep from a previous round, protocol progress this
        // round (an ACK can confirm a close), or a newly enqueued deferral.
        if self.deferred_remaining == 0 {
            if !self.deferred_dirty && !protocol_progressed {
                return DeferredReapOutcome::default();
            }
            self.deferred_remaining = self.deferred_removals.len();
            self.deferred_dirty = false;
        }
        while checked < STACK_STAGE_BUDGET
            && self.deferred_remaining > 0
            && !self.deferred_removals.is_empty()
        {
            let len = self.deferred_removals.len();
            if self.deferred_cursor >= len {
                self.deferred_cursor = 0;
            }
            let idx = self.deferred_cursor;
            let entry = self.deferred_removals[idx];
            // `iter().find` instead of `get`: a deferred handle may be stale
            // (reaped, cleanly removed elsewhere, or slot reused by another
            // type), and smoltcp's `get`/`remove` panic on invalid handles.
            let verdict = match sockets.iter().find(|(handle, _)| *handle == entry.handle) {
                // A UDPQueued entry whose slot is now a TCP socket is a
                // re-typed slot: drop the entry, keep the socket. Checked
                // before the generic TCP arms so `Keep` cannot swallow it.
                Some((_, smoltcp::socket::Socket::Tcp(_)))
                    if entry.kind == CloseKind::UdpQueued =>
                {
                    DeferredVerdict::Drop
                }
                Some((_, smoltcp::socket::Socket::Tcp(socket)))
                    if entry.kind.is_confirmed(socket.state()) =>
                {
                    DeferredVerdict::Reap
                }
                Some((_, smoltcp::socket::Socket::Tcp(_))) => DeferredVerdict::Keep,
                // T2.7: a dropped UDP socket with a queued (undispatched)
                // datagram is reclaimed only once its TX buffer drained
                // through the runner's egress rounds; `has_pending_tx()`
                // reads actual occupancy, unlike `can_send()` (capacity-not-
                // full) which would reap a full queue and keep an empty one.
                Some((_, smoltcp::socket::Socket::Udp(socket)))
                    if entry.kind == CloseKind::UdpQueued =>
                {
                    if socket.has_pending_tx() {
                        DeferredVerdict::Keep
                    } else {
                        DeferredVerdict::Reap
                    }
                }
                // Stale (handle gone) or re-typed entry: the entry cannot
                // own a close protocol anymore; drop it without touching the
                // socket set.
                Some(_) | None => DeferredVerdict::Drop,
            };
            match verdict {
                DeferredVerdict::Reap => {
                    sockets.remove(entry.handle);
                    self.deferred_removals.swap_remove(idx);
                    reclaimed += 1;
                    info!(
                        "deferred reap: socket {} ({:?}) reclaimed",
                        entry.handle, entry.kind
                    );
                }
                DeferredVerdict::Drop => {
                    self.deferred_removals.swap_remove(idx);
                    reclaimed += 1;
                }
                DeferredVerdict::Keep => {
                    self.deferred_cursor = (idx + 1) % len;
                }
            }
            checked += 1;
            self.deferred_remaining -= 1;
        }
        if self.deferred_removals.is_empty() {
            self.deferred_remaining = 0;
        }
        DeferredReapOutcome {
            checked,
            reclaimed,
            // A live sweep with remaining entries justifies one more
            // self-wake to finish it; after it completes (or the list is
            // empty) the runner must rely on a protocol event/deadline.
            sweep_incomplete: self.deferred_remaining > 0,
        }
    }

    /// Test-only observation of the deferred-removal backlog.
    #[cfg(test)]
    pub(crate) fn deferred_removals_len(&self) -> usize {
        self.deferred_removals.len()
    }

    /// Runs one fixed-order, bounded stack round.
    ///
    /// `timestamp` is the single Instant sampled once by the resident stack
    /// runner for this poll; the round, smoltcp ingress/egress/maintenance,
    /// `poll_at` deadline and the deferred retirement outcome all observe
    /// that same Instant (Task 2.6 replan). The wall-clock `now()` helper is
    /// only used by the compatibility `Service::poll` entry.
    pub(crate) fn stack_round(
        &mut self,
        timestamp: Instant,
        owner: RxOwnerView,
        sockets: &mut SocketSet,
    ) -> StackRoundOutcome {
        // Task 3.5 (Finding 2) + Iteration 011 A1: observe the target TX
        // pending state before ANY operation in this round can create a slot.
        // An ARP reply consumed by `router.poll` resolves a neighbor and flushes
        // the first dormant TX slot; sampling after that ingress hides the
        // empty->nonempty transition from the queue-owner event below.
        let tx_pending_before = self.tx_slot_pending_target();

        let router_rx =
            self.router
                .poll_bounded(owner, self.target_dev, timestamp, STACK_STAGE_BUDGET);
        // MS05 Task 3.2: frames are delivered/consumed by the stack RX path
        // (slot mode drains the fixed slots); the queue task only copies
        // raw→slot, so the delivered/non-IP counters come from here.
        RX_TELEMETRY
            .delivered
            .fetch_add(self.router.take_rx_delivered_delta(), Ordering::Relaxed);
        RX_TELEMETRY
            .non_ip_consumed
            .fetch_add(self.router.take_rx_consumed_delta(), Ordering::Relaxed);
        self.iface.poll_maintenance(timestamp);
        let mut listener_head_repairs = 0usize;
        let ingress = run_bounded_stage(|| {
            let step = match self
                .iface
                .poll_ingress_single(timestamp, &mut self.router, sockets)
            {
                PollIngressSingleResult::None => StageStep::Idle,
                PollIngressSingleResult::PacketProcessed => StageStep::Processed,
                PollIngressSingleResult::SocketStateChanged => StageStep::SocketStateChanged,
            };
            // T2.8-R1: after each processed ingress packet, repair at most one
            // exactly signaled listener head so the next same-batch packet
            // finds a Listen socket; bounded by the processed-packet count.
            if !matches!(step, StageStep::Idle) && self.listen_table().consume_head_signal(sockets)
            {
                listener_head_repairs += 1;
            }
            step
        });
        let egress = run_bounded_stage(|| {
            match self.iface.poll_egress(timestamp, &mut self.router, sockets) {
                PollResult::None => StageStep::Idle,
                PollResult::SocketStateChanged => StageStep::SocketStateChanged,
            }
        });
        // Task 2.6 replan: ONE bounded listener reconcile stage per round,
        // after ingress/egress so their socket transitions are visible to
        // it. A cross-round cursor limits the stage to `STACK_STAGE_BUDGET`
        // positions; a budget-exhausted stage requests a continuation.
        let listener = self
            .listen_table()
            .reconcile(sockets, ingress.socket_changed || egress.socket_changed);
        // Waking the queue task is a release of the resource it is blocked
        // on. The waiting bit is published only for a full RX slot (Task 3.2
        // slot-mode copy); Router-buffer space is drained by the stack itself
        // and must never clear it (Task 3.5 Finding 6).
        let space = self.rx_slot_has_space_target();
        let rx_space_woken = QUEUE_EVENT.wake_if_space(space);
        if rx_space_woken {
            RX_TELEMETRY
                .space_wake
                .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        }
        // Task 3.5 (Finding 2): a stack TX dispatch that fills an empty TX
        // slot must publish a queue-owner event. A sleeping queue task has no
        // hardware completion to wait on for the first frame, so without this
        // event the frame would sit in the slot forever. The before-sample is
        // taken at the top of the round so ingress-created slots (Iteration
        // 011 A1) also publish exactly once.
        let dispatch = self.router.dispatch_bounded(timestamp, STACK_STAGE_BUDGET);
        let tx_enqueued = !tx_pending_before && self.tx_slot_pending_target();
        if tx_enqueued {
            QUEUE_EVENT.publish_queue_work();
        }
        // T2.5-R2 + Task 2.6 replan: reclaim deferred close handles whose ACK
        // arrived during this round, before poll_at is recomputed: a
        // confirmed handle has no pending protocol timer, so it must not
        // extend the deadline. The reaper itself is bounded to one 32-entry
        // stage with a rotating cursor, and only examines when there is a
        // reason to do so (a socket state transition, a fresh enqueue, or an
        // unfinished sweep) — a quiet set of unconfirmed closes never keeps
        // the runner self-waking.
        let deferred =
            self.reap_deferred_removals(sockets, ingress.socket_changed || egress.socket_changed);
        let protocol_deadline = self.iface.poll_at(timestamp, sockets);
        let requires_polling = self
            .router
            .devices
            .iter()
            .any(|device| device.requires_polling());
        let socket_changed = ingress.socket_changed || egress.socket_changed;
        let self_yield = router_rx.budget_exhausted
            || ingress.budget_exhausted
            || egress.budget_exhausted
            || dispatch.budget_exhausted;
        let fault_code = match &router_rx.fault {
            Some(err) => crate::readiness::dev_error_code(err),
            None => dispatch.fault_code,
        };
        StackRoundOutcome {
            work: router_rx.processed + ingress.processed + egress.processed + dispatch.processed,
            backlog: router_rx.backlog
                || ingress.budget_exhausted
                || egress.budget_exhausted
                || dispatch.backlog,
            self_yield,
            socket_changed,
            rx_ready: dispatch.rx_ready,
            rx_space_woken,
            tx_enqueued,
            fault_code,
            protocol_deadline,
            requires_polling,
            deferred_checked: deferred.checked,
            deferred_reclaimed: deferred.reclaimed,
            deferred_sweep_incomplete: deferred.sweep_incomplete,
            listener_checked: listener.checked,
            listener_sweep_incomplete: listener.sweep_incomplete,
            listener_head_repairs,
        }
    }

    /// Whether the target's fixed RX slots have room for at least one frame.
    ///
    /// The stack consults this after draining RX slots so it can wake the
    /// queue task whose RX copy stage was blocked on a full slot. A missing
    /// target reports space (the queue task is not running then).
    pub(crate) fn rx_slot_has_space_target(&self) -> bool {
        match self.target_dev {
            Some(dev) => self.router.rx_slot_has_space(dev),
            None => true,
        }
    }

    /// Whether the target's fixed TX slots hold a pending frame.
    ///
    /// The stack consults this after TX dispatch so it can wake the queue
    /// task to submit. A missing target reports no pending frames.
    pub(crate) fn tx_slot_pending_target(&self) -> bool {
        match self.target_dev {
            Some(dev) => self.router.tx_slot_pending(dev),
            None => false,
        }
    }

    fn target_index(&self) -> DevResult<usize> {
        self.target_dev.ok_or(DevError::BadState)
    }

    /// All-or-nothing bidirectional activation of the stored target (Task
    /// 3.1).
    ///
    /// Under the single Service guard: validates the target, suppresses BOTH
    /// directions and switches the device to the slot data path. Any failure
    /// leaves the device in polling mode (both raw directions still polling
    /// owned); success means async owns both directions from here on. The
    /// caller publishes the `Active` lifecycle only after this returns `Ok`.
    pub(crate) fn activate_target(&mut self) -> DevResult {
        let dev = self.target_index()?;
        self.router.control_suppress_both(dev)?;
        self.router.activate_slot_mode(dev)
    }

    /// Rearms BOTH directions on the stored target and reports which
    /// directions still have a pending completion.
    pub(crate) fn arm_and_check_both_target(&mut self) -> DevResult<NetQueueDirection> {
        self.router.control_arm_and_check_both(self.target_index()?)
    }

    /// Returns which directions currently have visible completions on the
    /// stored target.
    pub(crate) fn completion_pending_both_target(&mut self) -> DevResult<NetQueueDirection> {
        self.router
            .control_completion_pending_both(self.target_index()?)
    }

    /// Advances the raw→RX-slot copy on the stored target by at most one
    /// frame (Task 3.2 queue service).
    pub(crate) fn rx_copy_one_target(&mut self) -> RxCopyStep {
        let Some(dev) = self.target_dev else {
            return RxCopyStep::Fault(DevError::BadState);
        };
        self.router.rx_copy_one(dev)
    }

    /// Advances the TX-slot→raw submit on the stored target by at most one
    /// frame (Task 3.2 queue service).
    pub(crate) fn tx_submit_one_target(&mut self) -> TxSubmitStep {
        let Some(dev) = self.target_dev else {
            return TxSubmitStep::Fault(DevError::BadState);
        };
        self.router.tx_submit_one(dev)
    }

    /// Advances the TX completion reclaim on the stored target by at most
    /// one completion (Task 3.2 queue service).
    pub(crate) fn tx_reclaim_one_target(&mut self) -> TxReclaimStep {
        let Some(dev) = self.target_dev else {
            return TxReclaimStep::Fault(DevError::BadState);
        };
        self.router.tx_reclaim_one(dev)
    }

    /// Target's current device-reset epoch (Task 2.1 recovery owner).
    pub(crate) fn queue_epoch_target(&self) -> axdriver_net::QueueEpoch {
        match self.target_dev {
            Some(dev) => self.router.queue_epoch(dev),
            None => axdriver_net::QueueEpoch::MIN,
        }
    }

    /// Cancels every current-epoch `Queued` ticket on the target, returning
    /// the count (Task 2.1). Called by the queue task under the Service guard
    /// at the recovery linearization point.
    pub(crate) fn tx_cancel_queued_target(&mut self) -> usize {
        match self.target_dev {
            Some(dev) => self.router.tx_cancel_queued(dev),
            None => 0,
        }
    }

    /// Drops every pre-submit packet waiting in the ARP/neighbor pending
    /// storage on the target, returning the count (Task 2.2, F3). Linearized
    /// with `tx_cancel_queued_target` under the Service guard so no pending
    /// pre-submit packet survives into a new epoch and is auto-sent after
    /// recovery.
    pub(crate) fn tx_cancel_pending_target(&mut self) -> usize {
        match self.target_dev {
            Some(dev) => self.router.tx_cancel_pending(dev),
            None => 0,
        }
    }

    /// Closes every remaining `DeviceOwned` ticket on the target as
    /// `ResetAborted` after a confirmed reset, returning the count (Task 2.1).
    pub(crate) fn tx_close_device_owned_target(&mut self) -> usize {
        match self.target_dev {
            Some(dev) => self.router.tx_close_device_owned(dev),
            None => 0,
        }
    }

    /// Resident-fault closure of every `DeviceOwned` ticket on the target
    /// device with the committed bounded stage identity (Task 2.1 / F4).
    /// Backing stays quarantined.
    pub(crate) fn tx_fault_device_owned_target(
        &mut self,
        stage: crate::device::TicketFaultStage,
    ) -> usize {
        match self.target_dev {
            Some(dev) => self.router.tx_fault_device_owned(dev, stage),
            None => 0,
        }
    }

    /// Advances the target's software ticket epoch after a confirmed reset
    /// (Task 2.1). Callers must hold the guard.
    pub(crate) fn tx_advance_epoch_target(&mut self, next: axdriver_net::QueueEpoch) {
        if let Some(dev) = self.target_dev {
            self.router.tx_advance_epoch(dev, next);
        }
    }

    /// Sets or clears the recovery I/O gate on the target (Task 2.2): while
    /// held, the device's TX enqueue path rejects new sends, so no new Queued
    /// ticket enters a data plane being reset. Callers must hold the guard.
    pub(crate) fn tx_set_recovery_hold_target(&mut self, held: bool) {
        if let Some(dev) = self.target_dev {
            self.router.tx_set_recovery_hold(dev, held);
        }
    }

    /// Number of DeviceOwned tickets still outstanding on the target (Task
    /// 2.2 quiesce drain). Callers must hold the guard.
    pub(crate) fn device_owned_len_target(&self) -> u64 {
        match self.target_dev {
            Some(dev) => self.router.tx_device_owned_len(dev),
            None => 0,
        }
    }

    /// Whether the target device exposes a transport-neutral recovery control
    /// that the queue owner can drive. Devices without one must fail closed:
    /// the owner cannot pretend to recover them.
    pub(crate) fn target_can_recover(&mut self) -> bool {
        let Some(dev) = self.target_dev else {
            return false;
        };
        self.router.recovery_control(dev).is_some()
    }

    /// Initiates the target's recovery: cancels every current-epoch `Queued`
    /// ticket, then starts the driver's bounded recovery flow. Callers must
    /// hold the guard. The returned epoch is the one recovery is moving to.
    ///
    /// A device without recovery support must fail-closed; the queue must not
    /// pretend to recover it.
    pub(crate) fn recovery_begin_target(&mut self) -> DevResult<axdriver_net::QueueEpoch> {
        let Some(dev) = self.target_dev else {
            return Err(DevError::BadState);
        };
        if !self.target_can_recover() {
            return Err(DevError::Unsupported);
        }
        // Linearize the pre-submit cancellation (queued tickets AND ARP-pending
        // pre-submit packets) with the driver begin under the same guard so no
        // ticket or packet is both cancelled and submitted.
        self.tx_cancel_queued_target();
        self.tx_cancel_pending_target();
        let Some(control) = self.router.recovery_control(dev) else {
            return Err(DevError::Unsupported);
        };
        control.begin_recovery().map(|p| p.epoch)
    }

    /// Advances the target's in-progress recovery by at most one bounded
    /// driver step. Callers must hold the guard. Returns the recovery progress;
    /// the caller decides whether to keep polling, publish a new epoch on
    /// `Recovered`, or fail-closed.
    pub(crate) fn recovery_step_target(&mut self) -> DevResult<axdriver_net::RecoveryProgress> {
        let Some(dev) = self.target_dev else {
            return Err(DevError::BadState);
        };
        let Some(control) = self.router.recovery_control(dev) else {
            return Err(DevError::Unsupported);
        };
        control.poll_recovery_step()
    }

    /// F2: reads the driver's current ownership summary (available /
    /// device-owned / quarantined resources) so a recovery fault freezes a
    /// structured ledger snapshot instead of a generic error. Callers hold the
    /// guard; a device without a recovery control reports the all-zero summary.
    pub(crate) fn recovery_owner_summary_target(&mut self) -> axdriver_net::OwnerSummary {
        let Some(dev) = self.target_dev else {
            return axdriver_net::OwnerSummary::default();
        };
        self.router
            .recovery_control(dev)
            .map(|control| control.owner_summary())
            .unwrap_or(axdriver_net::OwnerSummary::default())
    }

    /// RX-slot-space recheck, callable only while holding the Service guard.
    ///
    /// The queue task's RX copy stage stops without reaping when the fixed
    /// RX slots are full; the stack drains those slots, then this method
    /// decides whether the task may retry now (`Retry`) or must sleep on the
    /// waiting bit (`Waiting`).
    pub(crate) fn rx_slot_space_recheck_or_wait(&self) -> SpaceDecision {
        if self.rx_slot_has_space_target() {
            SpaceDecision::Retry
        } else {
            QUEUE_EVENT.publish_waiting();
            SpaceDecision::Waiting
        }
    }

    // ── Target-scoped C4 flush (D8) ─────────────────────────────────────

    /// Reserves the sole flush waiter, synchronously capturing the target as
    /// the current `last_accepted` ticket. The caller must hold the Service
    /// guard. A second concurrent flush is `ResourceBusy`; a persisted
    /// terminal fault (RW-3) or an exhausted waiter identity also fails the
    /// construction without consuming the waiter slot.
    pub(crate) fn flush_begin(&mut self) -> Result<FlushTicket, DevError> {
        // RW-3: a terminal data-plane fault is stable. A flush constructed
        // after the fault must return the same error immediately, never wait
        // on a live target whose owner has already stopped.
        if let Some(code) = self.flush_terminal_fault {
            self.flush_error += 1;
            return Err(error_from_code(code));
        }
        if self.flush_waiter.is_some() {
            self.flush_busy += 1;
            return Err(DevError::ResourceBusy);
        }
        // RW-3: checked identity allocation. `u64::MAX` is the invalid
        // sentinel (also used by V3 for empty optional tickets), so the
        // counter must never wrap back to 0 and reuse an identity (ABA).
        let identity = self.flush_next_identity;
        if identity == u64::MAX {
            self.flush_busy += 1;
            return Err(DevError::ResourceBusy);
        }
        self.flush_next_identity += 1;
        let target = match self.target_dev {
            Some(dev) => self.router.tx_last_accepted(dev),
            None => None,
        };
        let epoch = self.queue_epoch_target();
        self.flush_waiter = Some(FlushWaiter::new(identity, target, epoch));
        Ok(FlushTicket { identity, target })
    }

    /// Registers the sole waker for a matching waiter identity, then rechecks.
    /// Must be called under the Service guard (register-then-recheck closes
    /// the lost-wakeup window against reclaim/fault publications).
    pub(crate) fn flush_register(&mut self, identity: u64, waker: &Waker) {
        if let Some(waiter) = &mut self.flush_waiter {
            if waiter.identity() == identity {
                waiter.register(waker);
            }
        }
    }

    /// Rechecks flush completion under the guard. A `Stale` result means the
    /// waiter identity no longer owns the slot.
    pub(crate) fn flush_recheck(&mut self, identity: u64, target: Option<u64>) -> FlushRecheck {
        // Captured before the `&mut waiter` borrow so the epoch comparison and
        // the waiters' mutable methods do not conflict on `self`.
        let current_epoch = self.queue_epoch_target();
        let Some(waiter) = &mut self.flush_waiter else {
            return FlushRecheck::Stale;
        };
        if waiter.identity() != identity {
            return FlushRecheck::Stale;
        }
        if let Some(code) = waiter.take_fault_code() {
            let err = error_from_code(code);
            self.flush_error += 1;
            self.flush_waiter = None;
            return FlushRecheck::Faulted(err);
        }
        // Finding 1: a successful flush sealed by the recovery owner before the
        // epoch advanced survives the reset; the epoch advance must not turn it
        // into a false Lost, nor a pending old-epoch flush into a false success.
        if waiter.is_sealed_done() {
            self.flush_success += 1;
            self.flush_waiter = None;
            return FlushRecheck::Done;
        }
        // A flush still pending whose data-plane epoch advanced without being
        // sealed can never have fully reclaimed its target (the reset aborted
        // whatever was still in flight), so it must fail, never read a false
        // success from the new epoch's empty ledger.
        if waiter.epoch() != current_epoch {
            self.flush_error += 1;
            self.flush_waiter = None;
            return FlushRecheck::Faulted(DevError::BadState);
        }
        // Task 2.1: epoch-scoped outcome. `Lost` is a packet-loss terminal on a
        // ticket within scope; it fails this flush stably but does NOT set the
        // persistent device fault, so a later generation's flush can succeed.
        let state = match self.target_dev {
            Some(dev) => self.router.tx_flush_state(dev, target),
            None => crate::device::FlushState::Done,
        };
        match state {
            crate::device::FlushState::Done => {
                self.flush_success += 1;
                self.flush_waiter = None;
                FlushRecheck::Done
            }
            crate::device::FlushState::Pending => FlushRecheck::Pending,
            crate::device::FlushState::Lost(outcome) => {
                let err = Self::lost_outcome_error(outcome);
                self.flush_error += 1;
                self.flush_waiter = None;
                FlushRecheck::Faulted(err)
            }
        }
    }

    /// Clears the waiter slot only when `identity` still owns it. Called by
    /// the future's `Drop`; a waiter dropped before completion is a cancel.
    pub(crate) fn flush_clear(&mut self, identity: u64) {
        if self
            .flush_waiter
            .as_ref()
            .is_some_and(|waiter| waiter.identity() == identity)
        {
            self.flush_cancel += 1;
            self.flush_waiter = None;
        }
    }

    /// Publishes flush progress after a successful reclaim: wakes the sole
    /// waiter when its target is now satisfied or a packet-loss outcome makes
    /// it permanently unsatisfiable. Caller holds the guard.
    pub(crate) fn flush_progress(&mut self) {
        let Some(waiter) = &self.flush_waiter else {
            return;
        };
        let wake = match self.target_dev {
            Some(dev) => matches!(
                self.router.tx_flush_state(dev, waiter.target()),
                crate::device::FlushState::Done | crate::device::FlushState::Lost(_)
            ),
            None => true,
        };
        if wake {
            waiter.wake();
        }
    }

    /// F5: settles the sole flush waiter for the closing epoch, right before
    /// the recovery owner advances the device epoch. Commits the outcome
    /// WITHOUT waking: the caller must wake the waiter via
    /// [`Self::flush_wake_pending`] only after dropping the Service guard and
    /// committing the epoch/lifecycle, so a woken observer never sees a
    /// half-committed state.
    pub(crate) fn flush_recovery_close(&mut self) {
        if self.flush_waiter.is_none() {
            return;
        }
        if self
            .flush_waiter
            .as_ref()
            .is_some_and(|w| w.epoch() != self.queue_epoch_target())
        {
            return;
        }
        let state = match self.target_dev {
            Some(dev) => self
                .router
                .tx_flush_state(dev, self.flush_waiter.as_ref().unwrap().target()),
            None => crate::device::FlushState::Done,
        };
        if let Some(waiter) = &mut self.flush_waiter {
            match state {
                crate::device::FlushState::Done => waiter.commit_sealed_done(),
                crate::device::FlushState::Lost(outcome) => {
                    waiter.commit_fault(&Self::lost_outcome_error(outcome))
                }
                crate::device::FlushState::Pending => waiter.commit_fault(&DevError::BadState),
            }
        }
    }

    /// F5: fails the sole flush waiter with a stable error but does NOT wake.
    /// The recovery owner commits the fault inside the guard, then wakes via
    /// [`Self::flush_wake_pending`] after releasing the guard. A flush that is
    /// never woken after a commit would pend forever, so the recovery owner
    /// must pair this with that deferred wake.
    pub(crate) fn flush_recovery_abort_all(&mut self, err: &DevError) {
        if let Some(waiter) = &mut self.flush_waiter {
            waiter.commit_fault(err);
        }
    }

    /// F5: wakes the sole flush waiter if a commit is outstanding. MUST be
    /// called only after the Service guard is dropped and the caller has
    /// committed every ledger/epoch/lifecycle result, so the wake callback
    /// observes a fully-committed state.
    pub(crate) fn flush_wake_pending(&self) {
        if let Some(waiter) = &self.flush_waiter {
            waiter.wake();
        }
    }

    /// Stable error returned by a flush whose target was lost to a packet
    /// cancel/reset/fault outcome (Task 2.1). Distinct from the persistent
    /// device fault so a recovered generation's flush can still succeed.
    fn lost_outcome_error(outcome: crate::device::TicketOutcome) -> DevError {
        match outcome {
            crate::device::TicketOutcome::CancelledPreSubmit
            | crate::device::TicketOutcome::ResetAborted
            | crate::device::TicketOutcome::Fault(_) => DevError::BadState,
            crate::device::TicketOutcome::Reclaimed => DevError::BadState,
        }
    }

    /// Records a terminal submit/reclaim fault and wakes the sole waiter.
    ///
    /// RW-3: the error is persisted in the Service so a flush constructed
    /// after the fault (or after the current waiter consumes it) still
    /// returns the same stable error. A fault without a waiter is not lost.
    pub(crate) fn flush_fault(&mut self, err: &DevError) {
        let code = error_code(err);
        self.flush_terminal_fault = Some(code);
        if let Some(waiter) = &mut self.flush_waiter {
            waiter.set_fault(err);
        }
    }

    /// Target-device slot/ticket ledger for the V3 diagnostic snapshot.
    pub(crate) fn v3_slot_ledger(&self) -> crate::device::SlotLedger {
        match self.target_dev {
            Some(dev) => self.router.slot_ledger(dev),
            None => crate::device::SlotLedger::default(),
        }
    }

    /// Real driver TX resource ledger for the V3 diagnostic snapshot (RW-2).
    ///
    /// `None` when the target's driver cannot observe a transport-neutral
    /// ledger; the V3 snapshot then reports zeros instead of synthesizing a
    /// ledger from slot or ticket capacities.
    pub(crate) fn v3_tx_resource_ledger(&mut self) -> Option<axdriver_net::TxResourceLedger> {
        match self.target_dev {
            Some(dev) => self.router.tx_resource_ledger(dev),
            None => None,
        }
    }

    /// Target-device flush target for the V3 diagnostic snapshot
    /// (`u64::MAX` when no flush is in flight).
    pub(crate) fn v3_flush_target(&self) -> u64 {
        self.flush_waiter
            .as_ref()
            .and_then(|waiter| waiter.target())
            .unwrap_or(u64::MAX)
    }

    /// Flush lifecycle counters for the V3 diagnostic snapshot.
    pub(crate) fn v3_flush_counters(&self) -> [u64; 4] {
        [
            self.flush_success,
            self.flush_error,
            self.flush_busy,
            self.flush_cancel,
        ]
    }

    /// Per-reason drop counters for the V3 diagnostic snapshot.
    pub(crate) fn v3_drop_reasons(&self) -> [u64; 5] {
        [
            self.router.drop_count(TxDropReason::MalformedIp),
            self.router.drop_count(TxDropReason::NoRoute),
            self.router.drop_count(TxDropReason::RouteSourceMismatch),
            self.router.drop_count(TxDropReason::UnsupportedAddress),
            self.router.drop_count(TxDropReason::FrameTooLarge),
        ]
    }

    /// Advances the Service-owned QEMU diagnostic lease and returns the
    /// active hold mode. The queue task calls this once per round under the
    /// Service guard (D9). The clock is the injectable `diag::diag_now()` so
    /// host tests drive the lease deadline deterministically; a per-test
    /// fixture clock (Task 5.2) overrides it without sharing process-global
    /// state.
    ///
    /// An expired lease is cleared and `auto_release_failure` saturating-
    /// incremented exactly once; no lease generation exists, so no identity
    /// can exhaust and no reachable Hold is ever permanent. The timer is
    /// wake-only, so the queue task's own wake drives this round.
    #[cfg(feature = "qemu-diagnostics")]
    pub(crate) fn diag_hold_tick(&mut self) -> u64 {
        if self.diag_hold_mode != crate::diag::HOLD_NONE
            && self.diag_lease_expiry_nanos != 0
            && self.diag_now() >= self.diag_lease_expiry_nanos
        {
            self.diag_hold_mode = crate::diag::HOLD_NONE;
            self.diag_lease_expiry_nanos = 0;
            self.diag_auto_release_failure = self.diag_auto_release_failure.saturating_add(1);
        }
        self.diag_hold_mode
    }

    /// The clock used for lease-deadline decisions: the attached per-test
    /// fixture clock when present, else `diag::diag_now()`.
    #[cfg(feature = "qemu-diagnostics")]
    pub(crate) fn diag_now(&self) -> u64 {
        #[cfg(test)]
        {
            if let Some(clock) = self.diag_test_clock {
                return clock.load();
            }
        }
        crate::diag::diag_now()
    }

    /// Applies one QEMU diagnostic control under the Service guard (C2).
    ///
    /// Validates the command and the checked deadline before any mutation, so
    /// an overflowing `now + lease_ms * NS_PER_MS` fails closed with
    /// `InvalidParam` and leaves the committed lease untouched. The caller
    /// publishes queue work exactly once after dropping the guard; Busy and
    /// error paths publish none.
    #[cfg(feature = "qemu-diagnostics")]
    pub(crate) fn diag_control(&mut self, op: u64, lease_ms: u64, now_nanos: u64) -> DevResult {
        let (mode, expiry) = match op {
            crate::diag::OP_HOLD_TX_SUBMIT | crate::diag::OP_HOLD_TX_RECLAIM
                if (1..=crate::diag::MAX_LEASE_MS).contains(&lease_ms) =>
            {
                let mode = if op == crate::diag::OP_HOLD_TX_SUBMIT {
                    crate::diag::HOLD_SUBMIT
                } else {
                    crate::diag::HOLD_RECLAIM
                };
                let nanos = lease_ms
                    .checked_mul(crate::diag::NS_PER_MS)
                    .ok_or(DevError::InvalidParam)?;
                let expiry = now_nanos.checked_add(nanos).ok_or(DevError::InvalidParam)?;
                (mode, expiry)
            }
            crate::diag::OP_RELEASE if lease_ms == 0 => (crate::diag::HOLD_NONE, 0),
            _ => return Err(DevError::InvalidParam),
        };
        self.diag_hold_mode = mode;
        self.diag_lease_expiry_nanos = expiry;
        Ok(())
    }

    /// Committed diagnostic hold mode (C1/C5: one committed Service state).
    #[cfg(feature = "qemu-diagnostics")]
    pub(crate) fn diag_hold_mode(&self) -> u64 {
        self.diag_hold_mode
    }

    /// Committed lease expiry deadline in wall nanoseconds (0 = no hold).
    #[cfg(feature = "qemu-diagnostics")]
    pub(crate) fn diag_lease_expiry(&self) -> u64 {
        self.diag_lease_expiry_nanos
    }

    /// Committed auto-release failure counter (saturating, monotonic).
    #[cfg(feature = "qemu-diagnostics")]
    pub(crate) fn diag_auto_release_failure(&self) -> u64 {
        self.diag_auto_release_failure
    }

    #[cfg(test)]
    pub(crate) fn router_for_test(&mut self) -> &mut Router {
        &mut self.router
    }

    /// RW-3 test seam: force the waiter identity counter to a value so the
    /// exhaustion boundary can be exercised without allocating 2^64 flushes.
    #[cfg(test)]
    pub(crate) fn set_flush_next_identity_for_test(&mut self, value: u64) {
        self.flush_next_identity = value;
    }

    /// RW-3 test seam: observe the identity counter after exhaustion.
    #[cfg(test)]
    pub(crate) fn flush_next_identity_for_test(&self) -> u64 {
        self.flush_next_identity
    }

    pub fn get_source_address(&self, dst_addr: &IpAddress) -> IpAddress {
        let Some(rule) = self.router.table.lookup(dst_addr) else {
            panic!("no route to destination: {dst_addr}");
        };
        rule.src
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use alloc::{boxed::Box, sync::Arc, task::Wake, vec, vec::Vec};
    use core::{
        sync::atomic::{AtomicUsize, Ordering},
        task::Waker,
    };

    use axdriver::prelude::DevError;
    use smoltcp::{time::Instant, wire::IpAddress};

    use super::{CloseKind, STACK_STAGE_BUDGET, Service, StageStep, run_bounded_stage};
    use crate::{
        async_rx::{QUEUE_EVENT, SERIAL},
        device::{Device, LoopbackDevice, RxStep, TxOutcome, TxPreflight},
        router::{Router, Rule, RxOwnerView},
    };

    #[derive(Default)]
    struct CountWake(Arc<AtomicUsize>);

    impl Wake for CountWake {
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

    struct CountingRxDevice {
        remaining: usize,
    }

    impl Device for CountingRxDevice {
        fn name(&self) -> &str {
            "counting-rx"
        }

        fn recv(
            &mut self,
            _buffer: &mut smoltcp::storage::PacketBuffer<()>,
            _ts: Instant,
        ) -> RxStep {
            if self.remaining > 0 {
                self.remaining -= 1;
                RxStep::Consumed
            } else {
                RxStep::Empty
            }
        }

        fn preflight_send(
            &mut self,
            _next_hop: IpAddress,
            _packet: &[u8],
            _timestamp: Instant,
        ) -> TxPreflight {
            TxPreflight::Ready
        }

        fn send(&mut self, _next_hop: IpAddress, _packet: &[u8], _timestamp: Instant) -> TxOutcome {
            TxOutcome::Accepted {
                rx_became_ready: false,
            }
        }

        fn register_waker(&self, _waker: &Waker) {}
    }

    struct FaultingRxDevice;

    impl Device for FaultingRxDevice {
        fn name(&self) -> &str {
            "faulting-rx"
        }

        fn recv(
            &mut self,
            _buffer: &mut smoltcp::storage::PacketBuffer<()>,
            _ts: Instant,
        ) -> RxStep {
            RxStep::Fault(DevError::Io)
        }

        fn preflight_send(
            &mut self,
            _next_hop: IpAddress,
            _packet: &[u8],
            _timestamp: Instant,
        ) -> TxPreflight {
            TxPreflight::Ready
        }

        fn send(&mut self, _next_hop: IpAddress, _packet: &[u8], _timestamp: Instant) -> TxOutcome {
            TxOutcome::Accepted {
                rx_became_ready: false,
            }
        }

        fn register_waker(&self, _waker: &Waker) {}
    }

    struct FaultingTxDevice;

    impl Device for FaultingTxDevice {
        fn name(&self) -> &str {
            "faulting-tx"
        }

        fn recv(
            &mut self,
            _buffer: &mut smoltcp::storage::PacketBuffer<()>,
            _ts: Instant,
        ) -> RxStep {
            RxStep::Empty
        }

        fn preflight_send(
            &mut self,
            _next_hop: IpAddress,
            _packet: &[u8],
            _timestamp: Instant,
        ) -> TxPreflight {
            TxPreflight::Fault(DevError::Io)
        }

        fn send(&mut self, _next_hop: IpAddress, _packet: &[u8], _timestamp: Instant) -> TxOutcome {
            TxOutcome::Fault(DevError::Io)
        }

        fn register_waker(&self, _waker: &Waker) {}
    }

    // Builds a minimal valid IPv4 broadcast packet (src = 10.0.2.15).
    // Broadcast fans out without a route lookup, so this needs no Router rule.
    fn broadcast_ipv4_packet() -> Vec<u8> {
        vec![
            0x45, 0, 0, 20, 0, 0, 0, 0, 64, 17, 0, 0, 10, 0, 2, 15, 255, 255, 255, 255,
        ]
    }

    #[test]
    fn bounded_stage_reports_31_32_33_without_drain_to_empty() {
        for count in [31usize, 32, 33] {
            let mut remaining = count;
            let outcome = run_bounded_stage(|| {
                if remaining == 0 {
                    StageStep::Idle
                } else {
                    remaining -= 1;
                    StageStep::Processed
                }
            });
            assert_eq!(outcome.processed, count.min(STACK_STAGE_BUDGET));
            assert_eq!(outcome.budget_exhausted, count >= STACK_STAGE_BUDGET);
            assert_eq!(remaining, count.saturating_sub(STACK_STAGE_BUDGET));
        }
    }

    #[test]
    fn bounded_stage_preserves_socket_change() {
        let mut steps = [
            StageStep::Processed,
            StageStep::SocketStateChanged,
            StageStep::Idle,
        ]
        .into_iter();
        let outcome = run_bounded_stage(|| steps.next().unwrap_or(StageStep::Idle));
        assert_eq!(outcome.processed, 2);
        assert!(outcome.socket_changed);
        assert!(!outcome.budget_exhausted);
    }

    #[test]
    fn quiet_stack_round_has_no_backlog_or_fault() {
        let mut router = Router::new();
        router.add_device(Box::new(LoopbackDevice::new()));
        let mut service = Service::new(router, None);
        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);

        let outcome = service.stack_round(
            Instant::from_millis_const(0),
            RxOwnerView::PollingOwned,
            &mut sockets,
        );

        assert!(!outcome.backlog);
        assert_eq!(outcome.fault_code, crate::readiness::TERMINAL_NONE);
        assert!(!outcome.socket_changed);
        assert!(!outcome.tx_enqueued);
    }

    #[test]
    fn close_kind_confirmation_matrix() {
        // T2.5-R2: an active close is safe to reclaim only at FinWait2 /
        // TimeWait / Closed; a LastAck close only at a fully closed
        // connection. Every other state keeps the raw handle deferred.
        use smoltcp::socket::tcp::State;

        assert!(CloseKind::Active.is_confirmed(State::FinWait2));
        assert!(CloseKind::Active.is_confirmed(State::TimeWait));
        assert!(CloseKind::Active.is_confirmed(State::Closed));
        assert!(!CloseKind::Active.is_confirmed(State::FinWait1));
        assert!(!CloseKind::Active.is_confirmed(State::Closing));
        assert!(!CloseKind::Active.is_confirmed(State::LastAck));
        assert!(!CloseKind::Active.is_confirmed(State::SynReceived));
        assert!(!CloseKind::Active.is_confirmed(State::Established));

        assert!(CloseKind::LastAck.is_confirmed(State::Closed));
        assert!(CloseKind::LastAck.is_confirmed(State::TimeWait));
        assert!(!CloseKind::LastAck.is_confirmed(State::LastAck));
        assert!(!CloseKind::LastAck.is_confirmed(State::FinWait2));
        assert!(!CloseKind::LastAck.is_confirmed(State::FinWait1));
        assert!(!CloseKind::LastAck.is_confirmed(State::CloseWait));
    }

    #[test]
    fn deferred_close_reap_dedups_stale_and_confirmed_removal() {
        // T2.5-R2 reaper: entries de-duplicate, a confirmed handle is
        // removed exactly once, and a stale entry for a gone handle is
        // dropped without touching the set.
        let mut service = routed_service();
        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
        let handle = sockets.add(crate::tcp::new_tcp_socket());

        service.queue_deferred_removal(handle, CloseKind::Active);
        service.queue_deferred_removal(handle, CloseKind::Active);
        assert_eq!(service.deferred_removals_len(), 1);

        // A fresh TCP socket is Closed: the active close confirms right away.
        service.reap_deferred_removals(&mut sockets, true);
        assert_eq!(service.deferred_removals_len(), 0);
        assert!(!sockets.iter().any(|(h, _)| h == handle));

        // A stale entry whose handle is already gone is dropped inertly.
        service.queue_deferred_removal(handle, CloseKind::LastAck);
        service.reap_deferred_removals(&mut sockets, true);
        assert_eq!(service.deferred_removals_len(), 0);
    }

    #[test]
    fn stack_round_reaps_deferred_close_before_poll_at() {
        // T2.5-R2: `stack_round` reclaims confirmed deferred handles during
        // the round, before poll_at is recomputed, so a doomed deferred
        // deadline can never park the runner afterwards.
        let mut router = Router::new();
        router.add_device(Box::new(LoopbackDevice::new()));
        let mut service = Service::new(router, None);
        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
        let handle = sockets.add(crate::tcp::new_tcp_socket());
        service.queue_deferred_removal(handle, CloseKind::Active);

        let _ = service.stack_round(
            Instant::from_millis_const(0),
            RxOwnerView::PollingOwned,
            &mut sockets,
        );

        assert_eq!(service.deferred_removals_len(), 0);
        assert!(!sockets.iter().any(|(h, _)| h == handle));
    }

    // ── Task 2.6 replan: bounded, fair deferred retirement ───────────────

    /// Creates `count` raw TCP handles in a non-confirmed state (Listen) so a
    /// sweep keeps every entry live and the per-round budget is observable.
    fn unconfirmed_listener_handles(
        sockets: &mut smoltcp::iface::SocketSet<'static>,
        count: usize,
    ) -> alloc::vec::Vec<smoltcp::iface::SocketHandle> {
        use smoltcp::wire::IpListenEndpoint;

        let mut handles = alloc::vec::Vec::new();
        for i in 0..count {
            let mut socket = crate::tcp::new_tcp_socket();
            socket
                .listen(IpListenEndpoint {
                    addr: None,
                    port: 20000 + i as u16,
                })
                .expect("listen on a fresh socket");
            handles.push(sockets.add(socket));
        }
        handles
    }

    #[test]
    fn deferred_retirement_reaps_at_most_32_entries_per_round() {
        // Task 2.6 replan: 33 confirmed entries must NOT be drained by one
        // unbounded scan. Exactly one 32-entry stage step may run per round;
        // the 33rd entry waits for the next round (fair, bounded retirement).
        let mut router = Router::new();
        router.add_device(Box::new(LoopbackDevice::new()));
        let mut service = Service::new(router, None);
        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
        for _ in 0..33 {
            let handle = sockets.add(crate::tcp::new_tcp_socket());
            service.queue_deferred_removal(handle, CloseKind::Active);
        }
        assert_eq!(service.deferred_removals_len(), 33);

        let _ = service.stack_round(
            Instant::from_millis_const(0),
            RxOwnerView::PollingOwned,
            &mut sockets,
        );

        // Current code scans the whole Vec in one round (0 left); the
        // bounded stage must leave exactly 1 for the next round.
        assert_eq!(service.deferred_removals_len(), 1);
        let _ = service.stack_round(
            Instant::from_millis_const(0),
            RxOwnerView::PollingOwned,
            &mut sockets,
        );
        assert_eq!(service.deferred_removals_len(), 0);
    }

    #[test]
    fn deferred_retirement_512_confirmed_converges_in_16_bounded_rounds() {
        // Task 2.6 replan: 512 confirmed entries need 512/32 = 16 rounds;
        // each round may check at most STACK_STAGE_BUDGET entries, and the
        // other stages of the same round still do their own bounded work.
        let mut router = Router::new();
        router.add_device(Box::new(LoopbackDevice::new()));
        let mut service = Service::new(router, None);
        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
        for _ in 0..512 {
            let handle = sockets.add(crate::tcp::new_tcp_socket());
            service.queue_deferred_removal(handle, CloseKind::Active);
        }

        for round in 1..=16 {
            let _ = service.stack_round(
                Instant::from_millis_const(0),
                RxOwnerView::PollingOwned,
                &mut sockets,
            );
            // After round k at most 32*k entries are examined; after round 15
            // exactly 480 are gone, round 16 finishes the last 32.
            assert_eq!(service.deferred_removals_len(), 512 - 32 * round);
        }
        assert_eq!(service.deferred_removals_len(), 0);
        assert!(
            !sockets
                .iter()
                .any(|(_, s)| matches!(s, smoltcp::socket::Socket::Tcp(_)))
        );
    }

    #[test]
    fn deferred_retirement_unconfirmed_head_does_not_starve_confirmed_tail() {
        // Task 2.6 replan: a long unconfirmed head must not push the whole
        // scan while the tail is confirmed. One round may examine only the
        // first 32 head entries (all kept), leaving the confirmed tail for a
        // later round; every entry is eventually examined and reclaimed once.
        let mut router = Router::new();
        router.add_device(Box::new(LoopbackDevice::new()));
        let mut service = Service::new(router, None);
        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
        let unconfirmed = unconfirmed_listener_handles(&mut sockets, 40);
        for handle in &unconfirmed {
            service.queue_deferred_removal(*handle, CloseKind::Active);
        }
        let mut confirmed = alloc::vec::Vec::new();
        for _ in 0..40 {
            let handle = sockets.add(crate::tcp::new_tcp_socket());
            confirmed.push(handle);
            service.queue_deferred_removal(handle, CloseKind::Active);
        }
        assert_eq!(service.deferred_removals_len(), 80);

        // Round 1: only the 32-entry budget is consumed on the head; the
        // 40 confirmed tail entries are left untouched by the bounded sweep.
        let _ = service.stack_round(
            Instant::from_millis_const(0),
            RxOwnerView::PollingOwned,
            &mut sockets,
        );
        assert_eq!(service.deferred_removals_len(), 80);

        // After the full sweep every confirmed entry is reclaimed exactly
        // once and every unconfirmed entry is still kept.
        for _ in 0..4 {
            let _ = service.stack_round(
                Instant::from_millis_const(0),
                RxOwnerView::PollingOwned,
                &mut sockets,
            );
        }
        assert_eq!(service.deferred_removals_len(), 40);
        for handle in &confirmed {
            assert!(!sockets.iter().any(|(h, _)| h == *handle));
        }
        for handle in &unconfirmed {
            assert!(sockets.iter().any(|(h, _)| h == *handle));
        }
    }

    #[test]
    fn deferred_retirement_stale_and_retyped_handles_keep_cursor_valid() {
        // Task 2.6 replan + T2.6-R1: a stale entry for a handle already
        // gone, and an entry whose handle was re-typed by another socket
        // type (UDP), are dropped without touching the set or panicking at
        // any cursor position. Note the accurate naming: "retyped" (slot
        // taken over by a DIFFERENT socket type) is not the same as
        // same-type handle reuse — the latter is proven unreachable on legal
        // paths by `deferred_retirement_live_entry_keeps_raw_slot_occupied`
        // and the T2.6-R1 source/ownership witness.
        let mut router = Router::new();
        router.add_device(Box::new(LoopbackDevice::new()));
        let mut service = Service::new(router, None);
        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
        // One live unconfirmed head, then a stale handle (already removed),
        // then a confirmed tail: the cursor must survive the swap-remove
        // paths regardless of where they hit.
        let head = unconfirmed_listener_handles(&mut sockets, 1);
        service.queue_deferred_removal(head[0], CloseKind::Active);
        let stale = sockets.add(crate::tcp::new_tcp_socket());
        sockets.remove(stale);
        service.queue_deferred_removal(stale, CloseKind::Active);
        // Re-typed handle: a UDP socket now occupies the old TCP slot.
        let retyped = sockets.add(crate::tcp::new_tcp_socket());
        sockets.remove(retyped);
        let retyped_udp = sockets.add(crate::udp::new_udp_socket());
        service.queue_deferred_removal(retyped, CloseKind::Active);
        let tail = sockets.add(crate::tcp::new_tcp_socket());
        service.queue_deferred_removal(tail, CloseKind::Active);

        for _ in 0..2 {
            let _ = service.stack_round(
                Instant::from_millis_const(0),
                RxOwnerView::PollingOwned,
                &mut sockets,
            );
        }
        // Confirmed tail reclaimed, stale/retyped dropped, unconfirmed head
        // kept and the retyped UDP slot untouched.
        assert_eq!(service.deferred_removals_len(), 1);
        assert!(sockets.iter().any(|(h, _)| h == head[0]));
        assert!(sockets.iter().any(|(h, _)| h == retyped_udp));
        assert!(!sockets.iter().any(|(h, _)| h == tail));
    }

    #[test]
    fn deferred_retirement_live_entry_keeps_raw_slot_occupied_and_reap_commits_atomically() {
        // T2.6-R1 ownership runtime half: while a deferred entry lives its
        // raw smoltcp slot stays OCCUPIED in the set, so `SocketSet::add`
        // can never hand that slot to a new TCP — same-type handle reuse is
        // unreachable until the resident reaper commits raw removal + entry
        // removal together. After that atomic commit a later slot reuse is
        // safe because no stale entry may target the new socket. 100x on
        // both feature profiles witness no flakiness.
        for _ in 0..100 {
            let mut router = Router::new();
            router.add_device(Box::new(LoopbackDevice::new()));
            let mut service = Service::new(router, None);
            let mut sockets = smoltcp::iface::SocketSet::new(vec![]);

            // A live unconfirmed entry (Listen state is not Active-confirmed):
            // the sweep keeps both the entry and its occupied raw slot.
            let live = unconfirmed_listener_handles(&mut sockets, 1);
            service.queue_deferred_removal(live[0], CloseKind::Active);
            service.reap_deferred_removals(&mut sockets, false);
            assert_eq!(service.deferred_removals_len(), 1);
            assert!(sockets.iter().any(|(h, _)| h == live[0]));

            // No fresh TCP may take the live deferred slot.
            for _ in 0..64 {
                let fresh = sockets.add(crate::tcp::new_tcp_socket());
                assert_ne!(
                    fresh, live[0],
                    "a live deferred raw slot must never be handed to a new owner"
                );
            }

            // A confirmed entry (fresh Closed socket confirms an Active close
            // at once): the reaper removes the raw handle and the entry in
            // one guarded commit, and the live entry is untouched.
            let confirmed = sockets.add(crate::tcp::new_tcp_socket());
            service.queue_deferred_removal(confirmed, CloseKind::Active);
            service.reap_deferred_removals(&mut sockets, true);
            assert_eq!(service.deferred_removals_len(), 1);
            assert!(!sockets.iter().any(|(h, _)| h == confirmed));

            // Post-reap same-type reuse is safe: the new TCP in the freed
            // slot is never referenced by a leftover entry, so a later
            // sweep is inert for it.
            let reused = sockets.add(crate::tcp::new_tcp_socket());
            service.reap_deferred_removals(&mut sockets, true);
            assert_eq!(service.deferred_removals_len(), 1);
            assert!(sockets.iter().any(|(h, _)| h == reused));
            assert!(sockets.iter().any(|(h, _)| h == live[0]));
        }
    }

    #[test]
    fn deferred_retirement_budget_does_not_steal_other_stage_budget() {
        // Task 2.6 replan: a round with 33 deferred entries must still run the
        // full 32-entry RX stage (work counts Router RX), so the deferred
        // stage cannot starve the other stages of their own budgets.
        let mut router = Router::new();
        router.add_device(Box::new(CountingRxDevice { remaining: 33 }));
        let mut service = Service::new(router, None);
        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
        for _ in 0..33 {
            let handle = sockets.add(crate::tcp::new_tcp_socket());
            service.queue_deferred_removal(handle, CloseKind::Active);
        }

        let outcome = service.stack_round(
            Instant::from_millis_const(0),
            RxOwnerView::PollingOwned,
            &mut sockets,
        );

        // RX consumed its own 32 budget; the deferred stage got its own 32.
        assert_eq!(outcome.work, 32);
        assert!(outcome.backlog);
        assert_eq!(service.deferred_removals_len(), 1);
    }

    #[test]
    fn listener_stage_budget_does_not_steal_router_or_deferred_stage_budget() {
        // Task 2.6 replan (S5): a >32-pending listener backlog sharing one
        // round with 33 Router RX items and 33 deferred entries must not
        // starve any stage: every stage gets its own 32-entry budget in the
        // same round, and a budget-hit listener stage requests a continuation
        // instead of swallowing the round.
        use crate::service::STACK_STAGE_BUDGET;

        for _ in 0..100 {
            let mut router = Router::new();
            router.add_device(Box::new(CountingRxDevice { remaining: 33 }));
            let table: &'static crate::listen_table::ListenTable =
                Box::leak(Box::new(crate::listen_table::ListenTable::new()));
            let mut service = Service::new_with_listen_table(router, None, table);
            let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
            let accept = Arc::new(crate::readiness::ReadinessBridge::new());
            table
                .listen(
                    smoltcp::wire::IpListenEndpoint {
                        addr: None,
                        port: 18300,
                    },
                    accept,
                    &mut sockets,
                )
                .unwrap();
            // Seed 64 pending slots and start the sweep directly: the round's
            // listener stage must hit its 32-position budget while the Router RX
            // and deferred stages run their own budgets in the same round.
            table.test_seed_closed_slots(18300, &mut sockets, 64);
            assert!(table.reconcile(&mut sockets, true).sweep_incomplete);
            for _ in 0..33 {
                let handle = sockets.add(crate::tcp::new_tcp_socket());
                service.queue_deferred_removal(handle, CloseKind::Active);
            }

            let outcome = service.stack_round(
                Instant::from_millis_const(0),
                RxOwnerView::PollingOwned,
                &mut sockets,
            );

            assert_eq!(outcome.work, 32, "Router RX consumed its own 32 budget");
            assert!(outcome.backlog);
            assert!(outcome.listener_sweep_incomplete, "listener must continue");
            assert_eq!(
                outcome.listener_checked, STACK_STAGE_BUDGET,
                "the listener stage consumed exactly its own 32-position budget"
            );
            assert_eq!(service.deferred_removals_len(), 1);
        }
    }

    #[test]
    fn deferred_retirement_udp_queued_tx_wait_for_drain_before_reap() {
        // T2.7: a dropped UDP socket whose TX buffer still holds an
        // undispatched datagram is kept until the datagram is dispatched
        // (egress drains the TX), then reaped exactly once. Reaping while
        // the datagram is still queued would silently drop it — the guest
        // MS01 udp-bidirectional hang.
        let mut router = Router::new();
        router.add_device(Box::new(LoopbackDevice::new()));
        let mut service = Service::new(router, None);
        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);

        // A UDP socket with a queued datagram: bind, then enqueue a send.
        let handle = sockets.add(crate::udp::new_udp_socket());
        sockets
            .get_mut::<smoltcp::socket::udp::Socket>(handle)
            .bind(smoltcp::wire::IpListenEndpoint {
                addr: None,
                port: 22000,
            })
            .unwrap();
        sockets
            .get_mut::<smoltcp::socket::udp::Socket>(handle)
            .send_slice(
                b"queued",
                smoltcp::socket::udp::UdpMetadata {
                    endpoint: smoltcp::wire::IpEndpoint::new(
                        smoltcp::wire::Ipv4Address::new(10, 0, 0, 2).into(),
                        21234,
                    ),
                    local_address: Some(smoltcp::wire::Ipv4Address::new(10, 0, 0, 1).into()),
                    meta: Default::default(),
                },
            )
            .unwrap();
        service.queue_deferred_removal(handle, CloseKind::UdpQueued);

        // A directly-invoked sweep (no egress round) observes the datagram
        // still queued: the reaper must Keep (not drop it). A full round
        // would let egress dispatch it first, hiding the pending state.
        let _ = service.reap_deferred_removals(&mut sockets, true);
        assert_eq!(service.deferred_removals_len(), 1);
        assert!(sockets.iter().any(|(h, _)| h == handle));
        assert!(
            sockets
                .iter()
                .any(|(_, s)| matches!(s, smoltcp::socket::Socket::Udp(_)))
        );

        // Once the resident runner's egress dispatches the datagram (TX
        // drains), the same bounded reaper removes raw handle + deferred
        // entry in one guarded commit.
        let _ = service.stack_round(
            Instant::from_millis_const(0),
            RxOwnerView::PollingOwned,
            &mut sockets,
        );
        assert_eq!(service.deferred_removals_len(), 0);
        assert!(!sockets.iter().any(|(h, _)| h == handle));
    }

    #[test]
    fn deferred_retirement_udp_queued_entry_stale_or_retyped_drops() {
        // T2.7: a UDPQueued entry whose handle is already gone (stale) or
        // whose slot was re-typed by a DIFFERENT socket type (TCP) drops the
        // entry without touching the set. Same-type UDP->UDP replacement is
        // unreachable on legal paths: only the reaper removes the original
        // deferred UDP socket (removing its entry in the same commit), so a
        // fresh UDP can only take the slot after the entry is gone.
        let mut router = Router::new();
        router.add_device(Box::new(LoopbackDevice::new()));
        let mut service = Service::new(router, None);
        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
        let stale = sockets.add(crate::udp::new_udp_socket());
        sockets.remove(stale);
        service.queue_deferred_removal(stale, CloseKind::UdpQueued);
        let retyped_tcp = sockets.add(crate::tcp::new_tcp_socket());
        service.queue_deferred_removal(retyped_tcp, CloseKind::UdpQueued);

        let _ = service.stack_round(
            Instant::from_millis_const(0),
            RxOwnerView::PollingOwned,
            &mut sockets,
        );
        assert_eq!(service.deferred_removals_len(), 0);
        assert!(sockets.iter().any(|(h, _)| h == retyped_tcp));
    }

    #[test]
    fn drop_state_read_and_deferred_enqueue_compose_without_deadlock() {
        // T2.4-R2 close-retirement concurrency witness: the public Drop
        // reads the raw close state under the SocketSet guard alone, then
        // enqueues under the Service guard alone; the runner and connect
        // roles keep the fixed SERVICE -> SOCKET_SET order. All roles
        // interleave 100x without deadlock.
        use alloc::vec::Vec;

        use smoltcp::iface::SocketSet;
        use spin::Mutex;

        let service: &'static Mutex<Service> = Box::leak(Box::new(Mutex::new(routed_service())));
        let sockets: &'static Mutex<SocketSet<'static>> =
            Box::leak(Box::new(Mutex::new(SocketSet::new(vec![]))));
        let handle = sockets.lock().add(crate::tcp::new_tcp_socket());

        const ITERS: usize = 100;
        let runner_done = Arc::new(AtomicUsize::new(0));
        let connect_done = Arc::new(AtomicUsize::new(0));
        let drop_done = Arc::new(AtomicUsize::new(0));
        let mut threads = Vec::new();

        let done = runner_done.clone();
        threads.push(std::thread::spawn(move || {
            for _ in 0..ITERS {
                let mut guard = service.lock();
                let mut set = sockets.lock();
                let _ = guard.stack_round(
                    Instant::from_millis_const(0),
                    RxOwnerView::PollingOwned,
                    &mut set,
                );
                done.fetch_add(1, Ordering::Relaxed);
            }
        }));

        let done = connect_done.clone();
        threads.push(std::thread::spawn(move || {
            for _ in 0..ITERS {
                let guard = service.lock();
                let _src = guard.get_source_address(&IpAddress::v4(127, 0, 0, 1));
                let set = sockets.lock();
                drop(set);
                drop(guard);
                done.fetch_add(1, Ordering::Relaxed);
            }
        }));

        let done = drop_done.clone();
        threads.push(std::thread::spawn(move || {
            for _ in 0..ITERS {
                // Drop discipline: SocketSet-only state read ...
                let _state = {
                    let set = sockets.lock();
                    set.iter().count()
                };
                // ... then Service-only enqueue (dedup + the runner's reap
                // keep at most one live entry).
                service
                    .lock()
                    .queue_deferred_removal(handle, CloseKind::Active);
                done.fetch_add(1, Ordering::Relaxed);
            }
        }));

        for thread in threads {
            thread.join().unwrap();
        }

        assert_eq!(runner_done.load(Ordering::Relaxed), ITERS);
        assert_eq!(connect_done.load(Ordering::Relaxed), ITERS);
        assert_eq!(drop_done.load(Ordering::Relaxed), ITERS);
    }

    #[test]
    fn full_round_executes_dispatch_after_rx_budget_hit() {
        let _serial = SERIAL.lock();
        // 33 RX items exhaust the 32-item Router RX budget; 5 malformed TX
        // packets must still be dispatched by the same round so the stage
        // order never skips later stages after a budget hit.
        let mut router = Router::new();
        router.add_device(Box::new(CountingRxDevice { remaining: 33 }));
        let mut service = Service::new(router, None);
        for _ in 0..5 {
            assert!(service.router_for_test().enqueue_tx_for_test(&[0u8; 1]));
        }

        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
        let outcome = service.stack_round(
            Instant::from_millis_const(0),
            RxOwnerView::PollingOwned,
            &mut sockets,
        );

        assert_eq!(outcome.work, 32 + 5);
        assert!(outcome.backlog);
        assert!(outcome.self_yield);
        assert_eq!(outcome.fault_code, crate::readiness::TERMINAL_NONE);
        assert_eq!(
            service
                .router_for_test()
                .drop_count(crate::device::TxDropReason::MalformedIp),
            5
        );
    }

    #[test]
    fn full_round_rx_fault_is_not_hidden_as_idle() {
        let _serial = SERIAL.lock();
        let mut router = Router::new();
        router.add_device(Box::new(FaultingRxDevice));
        let mut service = Service::new(router, None);

        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
        let outcome = service.stack_round(
            Instant::from_millis_const(0),
            RxOwnerView::PollingOwned,
            &mut sockets,
        );

        assert_ne!(outcome.fault_code, crate::readiness::TERMINAL_NONE);
        assert_eq!(
            outcome.fault_code,
            crate::readiness::dev_error_code(&DevError::Io)
        );
        assert!(!outcome.self_yield);
    }

    #[test]
    fn full_round_dispatch_fault_surfaces_in_outcome() {
        let _serial = SERIAL.lock();
        let mut router = Router::new();
        router.add_device(Box::new(FaultingTxDevice));
        let mut service = Service::new(router, None);
        assert!(
            service
                .router_for_test()
                .enqueue_tx_for_test(&broadcast_ipv4_packet())
        );

        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
        let outcome = service.stack_round(
            Instant::from_millis_const(0),
            RxOwnerView::PollingOwned,
            &mut sockets,
        );

        assert_ne!(outcome.fault_code, crate::readiness::TERMINAL_NONE);
        assert_eq!(
            outcome.fault_code,
            crate::readiness::dev_error_code(&DevError::Io)
        );
        assert!(service.router_for_test().tx_faulted());
    }

    #[test]
    fn service_poll_wakes_waiting_rx_task_after_ingress_frees_space() {
        let _serial = SERIAL.lock();
        let mut router = Router::new();
        router.add_device(Box::new(LoopbackDevice::new()));
        router.fill_rx_buffer_for_test();
        let mut service = Service::new(router, None);

        let count = Arc::new(AtomicUsize::new(0));
        QUEUE_EVENT.register_queue(&counting_waker(count.clone()));
        QUEUE_EVENT.publish_waiting();

        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
        service.poll(RxOwnerView::PollingOwned, &mut sockets);

        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    // ── Task 2.4: runner/socket/connect/listener lock competition ───────

    fn routed_service() -> Service {
        let mut router = Router::new();
        let lo_dev = router.add_device(Box::new(LoopbackDevice::new()));
        let lo_ip = smoltcp::wire::Ipv4Cidr::new(smoltcp::wire::Ipv4Address::new(127, 0, 0, 1), 8);
        router.add_rule(Rule::new(
            lo_ip.into(),
            None,
            lo_dev,
            lo_ip.address().into(),
        ));
        Service::new(router, None)
    }

    #[test]
    fn runner_connect_listener_lock_orders_compose_without_deadlock() {
        use alloc::vec::Vec;

        use smoltcp::iface::SocketSet;
        use spin::Mutex;

        // Two independent `spin::Mutex`s (Service/SocketSet planes) plus a
        // third entry mutex, exactly mirroring the runner discipline.
        // `stack_round` holds Service then SocketSet; connect holds Service
        // then SocketSet; the listener role adds the entry lock last.
        let service: &'static Mutex<Service> = Box::leak(Box::new(Mutex::new(routed_service())));
        let sockets: &'static Mutex<SocketSet<'static>> =
            Box::leak(Box::new(Mutex::new(SocketSet::new(vec![]))));
        let entry: &'static Mutex<Vec<u64>> = Box::leak(Box::new(Mutex::new(Vec::new())));

        const ITERS: usize = 100;
        let runner_done = Arc::new(AtomicUsize::new(0));
        let connect_done = Arc::new(AtomicUsize::new(0));
        let listener_done = Arc::new(AtomicUsize::new(0));

        let mut threads = Vec::new();

        let done = runner_done.clone();
        threads.push(std::thread::spawn(move || {
            for _ in 0..ITERS {
                let mut guard = service.lock();
                let mut set = sockets.lock();
                let _ = guard.stack_round(
                    Instant::from_millis_const(0),
                    RxOwnerView::PollingOwned,
                    &mut set,
                );
                drop(set);
                drop(guard);
                done.fetch_add(1, Ordering::Relaxed);
            }
        }));

        let done = connect_done.clone();
        threads.push(std::thread::spawn(move || {
            for _ in 0..ITERS {
                // Fixed connect order: Service first (route), then SocketSet.
                let guard = service.lock();
                let _src = guard.get_source_address(&IpAddress::v4(127, 0, 0, 1));
                let set = sockets.lock();
                let _ = set.iter().count();
                drop(set);
                drop(guard);
                done.fetch_add(1, Ordering::Relaxed);
            }
        }));

        let done = listener_done.clone();
        threads.push(std::thread::spawn(move || {
            for _ in 0..ITERS {
                let guard = service.lock();
                {
                    let set = sockets.lock();
                    let _ = set.iter().count();
                }
                let mut e = entry.lock();
                e.push(1);
                e.clear();
                drop(e);
                drop(guard);
                done.fetch_add(1, Ordering::Relaxed);
            }
        }));

        for thread in threads {
            thread.join().unwrap();
        }

        assert_eq!(runner_done.load(Ordering::Relaxed), ITERS);
        assert_eq!(connect_done.load(Ordering::Relaxed), ITERS);
        assert_eq!(listener_done.load(Ordering::Relaxed), ITERS);
    }

    // ── Iteration 008: Service-owned QEMU diagnostic lease (C1-C6) ─────

    #[cfg(feature = "qemu-diagnostics")]
    mod diag {
        use axdriver::prelude::DevError;

        use super::*;
        use crate::diag::{
            HOLD_NONE, HOLD_RECLAIM, HOLD_SUBMIT, MAX_LEASE_MS, NS_PER_MS, OP_HOLD_TX_RECLAIM,
            OP_HOLD_TX_SUBMIT, OP_RELEASE,
        };

        fn now() -> u64 {
            1_000_000_000_000
        }

        /// Task 5.2 (Iteration 006): per-fixture clock + Service pair. Each
        /// test owns an independent fake clock, so advancing one fixture can
        /// never change a sibling's hold mode/expiry/auto-release counter
        /// (the R57 companion flake came from the process-global `TEST_NOW`).
        fn test_service() -> (crate::diag::DiagTestClock, Service) {
            let clock = crate::diag::DiagTestClock::new();
            clock.store(now());
            let mut router = Router::new();
            router.add_device(Box::new(LoopbackDevice::new()));
            let mut service = Service::new(router, None);
            service.attach_test_clock(clock);
            (clock, service)
        }

        #[test]
        fn two_fixture_clocks_hold_independent_leases() {
            // Fixture A holds a 100 ms submit lease, fixture B a 50 ms
            // reclaim lease, both starting at `now()`. Advancing ONLY A past
            // its deadline must release A exactly once and leave B's mode,
            // expiry and counter untouched.
            let (clock_a, mut a) = test_service();
            let (clock_b, mut b) = test_service();
            a.diag_control(OP_HOLD_TX_SUBMIT, 100, now()).unwrap();
            b.diag_control(OP_HOLD_TX_RECLAIM, 50, now()).unwrap();

            clock_a.store(now() + 100 * NS_PER_MS);
            assert_eq!(a.diag_hold_tick(), HOLD_NONE);
            assert_eq!(a.diag_auto_release_failure(), 1);
            assert_eq!(b.diag_hold_mode(), HOLD_RECLAIM, "B's mode must not change");
            assert_eq!(b.diag_lease_expiry(), now() + 50 * NS_PER_MS);
            assert_eq!(b.diag_auto_release_failure(), 0);

            // Advancing B's own clock releases B exactly once.
            clock_b.store(now() + 50 * NS_PER_MS);
            assert_eq!(b.diag_hold_tick(), HOLD_NONE);
            assert_eq!(b.diag_auto_release_failure(), 1);
            assert_eq!(a.diag_auto_release_failure(), 1);
        }

        #[test]
        fn concurrent_fixture_clocks_never_leak_expiry_across_services() {
            // Barrier-free concurrent churn: every thread drives its OWN
            // Service + clock through commit/advance/release cycles. The
            // pre-fix process-global `TEST_NOW` would let thread A's store
            // auto-release thread B's held lease, failing the per-thread
            // mode/counter assertions.
            std::thread::scope(|scope| {
                for _ in 0..4 {
                    scope.spawn(|| {
                        let (clock, mut s) = test_service();
                        let mut expected = 0u64;
                        for i in 0..50u64 {
                            s.diag_control(OP_HOLD_TX_SUBMIT, 100, now() + i).unwrap();
                            clock.store(now() + i + 100 * NS_PER_MS);
                            assert_eq!(s.diag_hold_tick(), HOLD_NONE);
                            expected += 1;
                            assert_eq!(s.diag_auto_release_failure(), expected);
                            assert_eq!(s.diag_hold_mode(), HOLD_NONE);
                        }
                    });
                }
            });
        }

        #[test]
        fn control_rejects_out_of_range_lease_and_bad_ops() {
            let (clock, mut s) = test_service();
            assert!(matches!(
                s.diag_control(OP_HOLD_TX_SUBMIT, 0, clock.load()),
                Err(DevError::InvalidParam)
            ));
            assert!(matches!(
                s.diag_control(OP_HOLD_TX_SUBMIT, MAX_LEASE_MS + 1, clock.load()),
                Err(DevError::InvalidParam)
            ));
            assert!(matches!(
                s.diag_control(OP_RELEASE, 1, clock.load()),
                Err(DevError::InvalidParam)
            ));
            assert!(matches!(
                s.diag_control(99, 10, clock.load()),
                Err(DevError::InvalidParam)
            ));
            assert_eq!(s.diag_hold_mode(), HOLD_NONE);
            assert_eq!(s.diag_lease_expiry(), 0);
        }

        #[test]
        fn hold_submit_and_reclaim_set_modes_and_expiry() {
            let (clock, mut s) = test_service();
            s.diag_control(OP_HOLD_TX_SUBMIT, 100, clock.load())
                .unwrap();
            assert_eq!(s.diag_hold_mode(), HOLD_SUBMIT);
            assert_eq!(s.diag_lease_expiry(), clock.load() + 100 * NS_PER_MS);
            assert_eq!(s.diag_hold_tick(), HOLD_SUBMIT);
            s.diag_control(OP_HOLD_TX_RECLAIM, 1, clock.load()).unwrap();
            assert_eq!(s.diag_hold_mode(), HOLD_RECLAIM);
            assert_eq!(s.diag_lease_expiry(), clock.load() + NS_PER_MS);
        }

        #[test]
        fn release_clears_hold_and_never_counts_failure() {
            let (clock, mut s) = test_service();
            s.diag_control(OP_HOLD_TX_SUBMIT, 2000, clock.load())
                .unwrap();
            s.diag_control(OP_RELEASE, 0, clock.load()).unwrap();
            assert_eq!(s.diag_hold_mode(), HOLD_NONE);
            assert_eq!(s.diag_lease_expiry(), 0);
            assert_eq!(s.diag_auto_release_failure(), 0);
            assert_eq!(s.diag_hold_tick(), HOLD_NONE);
        }

        #[test]
        fn expired_lease_auto_releases_and_counts_failure() {
            let (clock, mut s) = test_service();
            let t0 = clock.load();
            s.diag_control(OP_HOLD_TX_SUBMIT, 2, t0).unwrap();
            clock.store(t0 + 2 * NS_PER_MS - 1);
            assert_eq!(s.diag_hold_tick(), HOLD_SUBMIT);
            clock.store(t0 + 2 * NS_PER_MS);
            assert_eq!(s.diag_hold_tick(), HOLD_NONE);
            assert_eq!(s.diag_auto_release_failure(), 1);
            assert_eq!(s.diag_hold_mode(), HOLD_NONE);
            assert_eq!(s.diag_lease_expiry(), 0);
            clock.store(t0 + 2 * NS_PER_MS + 1);
            assert_eq!(s.diag_hold_tick(), HOLD_NONE);
            assert_eq!(s.diag_auto_release_failure(), 1);
        }

        #[test]
        fn second_hold_after_expiry_reuses_the_state() {
            let (clock, mut s) = test_service();
            let t0 = clock.load();
            s.diag_control(OP_HOLD_TX_RECLAIM, 1, t0).unwrap();
            clock.store(t0 + NS_PER_MS);
            assert_eq!(s.diag_hold_tick(), HOLD_NONE);
            s.diag_control(OP_HOLD_TX_RECLAIM, 1, t0 + NS_PER_MS)
                .unwrap();
            assert_eq!(s.diag_hold_mode(), HOLD_RECLAIM);
            assert_eq!(s.diag_auto_release_failure(), 1);
        }

        #[test]
        fn hold_does_not_mutate_owner_or_completion_state() {
            let (clock, mut s) = test_service();
            s.diag_control(OP_HOLD_TX_SUBMIT, 10, clock.load()).unwrap();
            assert_eq!(s.diag_hold_tick(), HOLD_SUBMIT);
            let _ = s.diag_hold_mode();
            let _ = s.diag_lease_expiry();
            let _ = s.diag_auto_release_failure();
        }

        #[test]
        fn control_deadline_overflow_fails_closed_atomically() {
            // C2: `now + lease_ms * NS_PER_MS` is a checked add; an
            // overflowing deadline is rejected before any mutation and the
            // committed no-hold state survives.
            let (clock, mut s) = test_service();
            let far_future = u64::MAX - 10;
            assert!(matches!(
                s.diag_control(OP_HOLD_TX_SUBMIT, MAX_LEASE_MS, far_future),
                Err(DevError::InvalidParam)
            ));
            assert_eq!(s.diag_hold_mode(), HOLD_NONE);
            assert_eq!(s.diag_lease_expiry(), 0);
            assert_eq!(s.diag_auto_release_failure(), 0);
            assert_eq!(clock.load(), now());
        }

        #[test]
        fn any_reachable_hold_is_releasable_or_expirable() {
            // D9/C1: the Service lease carries no generation, so no identity
            // can exhaust. Every reachable Hold is releasable explicitly or
            // by expiry, even after many commit/release/expiry cycles.
            let (clock, mut s) = test_service();
            for i in 0..200u64 {
                s.diag_control(OP_HOLD_TX_SUBMIT, 2, now() + i).unwrap();
                assert_eq!(s.diag_hold_mode(), HOLD_SUBMIT);
                clock.store(now() + i + 2 * NS_PER_MS);
                assert_eq!(s.diag_hold_tick(), HOLD_NONE);
                assert_eq!(s.diag_auto_release_failure(), i + 1);
                s.diag_control(OP_HOLD_TX_RECLAIM, 1, now() + i).unwrap();
                s.diag_control(OP_RELEASE, 0, now() + i).unwrap();
                assert_eq!(s.diag_hold_mode(), HOLD_NONE);
            }
            assert_eq!(s.diag_auto_release_failure(), 200);
        }
    }
}
