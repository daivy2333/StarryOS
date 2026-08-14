//! Async RX queue-task decision layer.
//!
//! This module hosts the crate-private seam between the future RX queue task
//! and [`crate::service::Service`]: a single-waiter queue notification state,
//! pure lifecycle/event/budget decisions, the unique named queue task wiring,
//! and fixed ISR/software event publication entry points.

#[cfg(not(test))]
use alloc::borrow::ToOwned;
use core::{
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
    sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    task::{Context, Poll, Waker},
};

use axdriver::prelude::{DevError, DevResult};
use axdriver_net::NetQueueDirection;
use embassy_sync::waitqueue::AtomicWaker;

use crate::{
    device::{RxCopyStep, TxReclaimStep, TxSubmitStep},
    router::RxOwnerView,
    service::Service,
};

/// Dual-role queue notification state shared by the future queue task and
/// [`crate::service::Service`] (Task 3.1).
///
/// Two `AtomicWaker`s share one wrapping generation:
///
/// - The queue-owner role is the long-lived queue task. It registers without
///   taking the `SERVICE` lock, then publishes the waiting bit (Release)
///   inside the Service lock only after a serialized recheck confirms the
///   Router RX buffer is still full. `Service::poll` clears the bit (AcqRel)
///   and wakes the task exactly once.
/// - The stack-progress role is the socket/stack side. It is woken by slot
///   RX-ready, TX-slot space and fatal events so smoltcp re-evaluates
///   readiness. It is a hint, never an exact fd-readiness claim.
///
/// The two roles never overwrite each other's waker: they are distinct
/// `AtomicWaker` instances over one shared generation. `Acquire`/`Release`
/// order only the control state; counters are `Relaxed`.
pub(crate) struct QueueEvent {
    queue_waker: AtomicWaker,
    stack_waker: AtomicWaker,
    waiting: AtomicBool,
    generation: AtomicU64,
}

impl QueueEvent {
    pub(crate) const fn new() -> Self {
        Self {
            queue_waker: AtomicWaker::new(),
            stack_waker: AtomicWaker::new(),
            waiting: AtomicBool::new(false),
            generation: AtomicU64::new(0),
        }
    }

    #[cfg(test)]
    fn with_generation(generation: u64) -> Self {
        Self {
            queue_waker: AtomicWaker::new(),
            stack_waker: AtomicWaker::new(),
            waiting: AtomicBool::new(false),
            generation: AtomicU64::new(generation),
        }
    }

    /// Registers the queue-owner task waker. Callable without the Service
    /// lock.
    pub(crate) fn register_queue(&self, waker: &Waker) {
        self.queue_waker.register(waker);
    }

    /// Registers the stack-progress waker. Callable without the Service lock.
    pub(crate) fn register_stack(&self, waker: &Waker) {
        self.stack_waker.register(waker);
    }

    /// Publishes the waiting bit. Only called inside the Service guard after a
    /// serialized full-space recheck.
    pub(crate) fn publish_waiting(&self) {
        self.waiting.store(true, Ordering::Release);
    }

    /// Clears the waiting bit (AcqRel) and wakes the queue task exactly once
    /// when Router space is available. Never wakes the stack role.
    pub(crate) fn wake_if_space(&self, has_space: bool) -> bool {
        if has_space && self.waiting.swap(false, Ordering::AcqRel) {
            self.queue_waker.wake();
            true
        } else {
            false
        }
    }

    /// Publishes a queue event: wrapping Release increment of the shared
    /// generation, then wakes both roles. Called by the ISR path.
    pub(crate) fn publish_event(&self) {
        self.generation.fetch_add(1, Ordering::Release);
        self.queue_waker.wake();
        self.stack_waker.wake();
    }

    /// Publishes a stack-progress hint: bumps the shared generation so the
    /// queue wait protocol observes the change, and wakes only the stack role.
    /// Called after slot RX-ready, TX-slot space or a fatal event.
    pub(crate) fn publish_progress(&self) {
        self.generation.fetch_add(1, Ordering::Release);
        self.stack_waker.wake();
    }

    /// Acquire snapshot of the event generation.
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Empty-queue wait protocol: Acquire generation, register the queue
    /// waker, run the arm/recheck observation, then Acquire the generation
    /// again. A pending observation or a generation change yields `Retry`;
    /// only a quiescent arm with an unchanged generation yields `Sleep`. A
    /// failed arm is a queue-control fatal and yields `Fault` with the error.
    /// A stack-role publish between the two Acquire loads is observed as a
    /// generation change and forces a retry.
    pub(crate) fn wait_decision(
        &self,
        waker: &Waker,
        arm: impl FnOnce() -> DevResult<ArmObservation>,
    ) -> WaitDecision {
        let before = self.generation();
        self.register_queue(waker);
        let observation = arm();
        let after = self.generation();
        match observation {
            Err(err) => WaitDecision::Fault(err),
            Ok(ArmObservation::Pending) => WaitDecision::Retry,
            Ok(ArmObservation::Quiescent) => {
                if before != after {
                    WaitDecision::Retry
                } else {
                    WaitDecision::Sleep
                }
            }
        }
    }
}

/// Observation produced by the queue-control arm-and-recheck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ArmObservation {
    /// A completion is already pending; do not sleep.
    Pending,
    /// No completion is pending.
    Quiescent,
}

/// Outcome of the empty-queue wait protocol.
#[derive(Debug)]
pub(crate) enum WaitDecision {
    /// An event arrived or a completion is pending; self-wake/retry.
    Retry,
    /// No event window fired; the task may pend.
    Sleep,
    /// Queue-control arm failed; terminal for the wait, carries the category.
    Fault(DevError),
}

/// Maximum completions serviced per queue-task round.
pub(crate) const RX_BUDGET: usize = 32;

/// Maximum TX completions reclaimed per round (Task 3.2, D6).
pub(crate) const RECLAIM_BUDGET: usize = 32;

/// Maximum TX slot frames submitted per round (Task 3.2, D6).
pub(crate) const SUBMIT_BUDGET: usize = 32;

/// The one queue notification state. There is exactly one task waiter; Router
/// space wakes and future queue events share this waker.
pub(crate) static QUEUE_EVENT: QueueEvent = QueueEvent::new();

/// The one RX task lifecycle. Loaded by [`poll_interfaces`](crate::poll_interfaces)
/// to map the RX consumption right each round.
pub(crate) static RX_LIFECYCLE: RxLifecycle = RxLifecycle::new();

/// Monotonic RX queue-task telemetry.
///
/// Every counter is `Relaxed` and observation-only: none of them participate
/// in synchronization, ownership or wait correctness. Counters never reset and
/// success never clears the last-error fields, so a snapshot always reflects
/// the whole boot history of the async RX path.
pub(crate) static RX_TELEMETRY: RxTelemetry = RxTelemetry::new();

/// Stable diagnostic stage of the most recent RX queue-task error.
///
/// These values are part of the observable snapshot ABI: do not renumber them.
pub mod rx_error_stage {
    /// No error has been recorded yet.
    pub const NONE: u64 = 0;
    /// Activation preflight failed (missing Service/NIC/control).
    pub const PREFLIGHT: u64 = 1;
    /// RX notification suppression failed.
    pub const SUPPRESS: u64 = 2;
    /// RX completion visibility query failed.
    pub const COMPLETION_QUERY: u64 = 3;
    /// A receive/recycle (or Router handoff) aggregate failed.
    pub const RECEIVE_RECYCLE: u64 = 4;
    /// The register/arm/recheck wait protocol failed.
    pub const ARM: u64 = 5;
    /// A lifecycle transition was illegal.
    pub const LIFECYCLE: u64 = 6;
}

/// Stable diagnostic code for a [`DevError`].
///
/// The codes are explicit and never derived from the enum discriminant, so
/// they stay stable across dependency updates. Do not renumber them.
pub fn rx_error_code(err: &DevError) -> u64 {
    match err {
        DevError::AlreadyExists => 1,
        DevError::Again => 2,
        DevError::BadState => 3,
        DevError::InvalidParam => 4,
        DevError::Io => 5,
        DevError::NoMemory => 6,
        DevError::ResourceBusy => 7,
        DevError::Unsupported => 8,
    }
}

/// Monotonic relaxed-atomics telemetry of the async RX queue path.
#[derive(Debug)]
pub(crate) struct RxTelemetry {
    /// ISR event publishes (generation increments).
    pub isr_publish: AtomicU64,
    /// ISR wake calls on the sole queue-task waker.
    pub isr_wake: AtomicU64,
    /// Explicit software-only wake requests.
    pub software_nudge: AtomicU64,
    /// Queue-task `Future::poll` invocations.
    pub task_poll: AtomicU64,
    /// Completions reaped (Consumed + Delivered).
    pub reaped: AtomicU64,
    /// Descriptors refilled (one per reap).
    pub refilled: AtomicU64,
    /// IP packets delivered to the Router.
    pub delivered: AtomicU64,
    /// Non-IP / non-target / malformed completions consumed.
    pub non_ip_consumed: AtomicU64,
    /// Budget exhausted rounds with a backlog present.
    pub budget_exhausted: AtomicU64,
    /// Self-wakes issued for `block_on` yielding.
    pub self_yield: AtomicU64,
    /// Router-full waits published (Waiting).
    pub router_full_wait: AtomicU64,
    /// Space wakes delivered by `Service::poll`.
    pub space_wake: AtomicU64,
    /// Empty-queue register/arm/recheck protocols run.
    pub empty_check: AtomicU64,
    /// Terminal queue/device faults (Faulted transitions).
    pub fault: AtomicU64,
    /// Packed `(stage, code)` of the most recent error.
    ///
    /// A single atomic publication prevents snapshots from combining the
    /// stage from one fault with the code from another.
    pub last_error: AtomicU64,
    /// TX completions reclaimed by the queue task (Task 3.2).
    pub tx_reclaimed: AtomicU64,
    /// TX slot frames submitted to the driver by the queue task.
    pub tx_submitted: AtomicU64,
    /// TX submit rounds stopped on `Again` (slot frame retained).
    pub tx_again: AtomicU64,
    /// RX copy stages stopped because the fixed RX slot storage was full.
    pub rx_slot_full: AtomicU64,
}

impl RxTelemetry {
    pub(crate) const fn new() -> Self {
        Self {
            isr_publish: AtomicU64::new(0),
            isr_wake: AtomicU64::new(0),
            software_nudge: AtomicU64::new(0),
            task_poll: AtomicU64::new(0),
            reaped: AtomicU64::new(0),
            refilled: AtomicU64::new(0),
            delivered: AtomicU64::new(0),
            non_ip_consumed: AtomicU64::new(0),
            budget_exhausted: AtomicU64::new(0),
            self_yield: AtomicU64::new(0),
            router_full_wait: AtomicU64::new(0),
            space_wake: AtomicU64::new(0),
            empty_check: AtomicU64::new(0),
            fault: AtomicU64::new(0),
            last_error: AtomicU64::new(pack_last_error(rx_error_stage::NONE, 0)),
            tx_reclaimed: AtomicU64::new(0),
            tx_submitted: AtomicU64::new(0),
            tx_again: AtomicU64::new(0),
            rx_slot_full: AtomicU64::new(0),
        }
    }

    /// Records a terminal fault and the most recent error category.
    fn record_fault(&self, stage: u64, err: &DevError) {
        self.fault.fetch_add(1, Ordering::Relaxed);
        self.record_last_error(stage, err);
    }

    /// Records the most recent error category without a fault counter.
    fn record_last_error(&self, stage: u64, err: &DevError) {
        self.record_last_error_code(stage, rx_error_code(err));
    }

    /// Records the most recent error stage with an explicit stable code.
    ///
    /// Used for categories that carry no [`DevError`], e.g. illegal lifecycle
    /// transitions where the observed state code is the payload.
    fn record_last_error_code(&self, stage: u64, code: u64) {
        self.last_error
            .store(pack_last_error(stage, code), Ordering::Relaxed);
    }

    fn last_error(&self) -> (u64, u64) {
        unpack_last_error(self.last_error.load(Ordering::Relaxed))
    }
}

const LAST_ERROR_HALF_BITS: u32 = u64::BITS / 2;
const LAST_ERROR_HALF_MASK: u64 = u32::MAX as u64;

const fn pack_last_error(stage: u64, code: u64) -> u64 {
    debug_assert!(stage <= LAST_ERROR_HALF_MASK);
    debug_assert!(code <= LAST_ERROR_HALF_MASK);
    (stage << LAST_ERROR_HALF_BITS) | code
}

fn unpack_last_error(value: u64) -> (u64, u64) {
    (value >> LAST_ERROR_HALF_BITS, value & LAST_ERROR_HALF_MASK)
}

/// Read-only bounded snapshot of the async RX queue path.
///
/// `repr(C)` and append-only: the kernel ioctl maps this into its own
/// `IrqSnapshot` without taking the Service lock. All fields are `u64` so the
/// Rust and C layouts stay trivially aligned.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RxSnapshot {
    /// Lifecycle code: 0 Polling, 1 Spawned, 2 Active, 3 Faulted, 4 Unavailable.
    pub lifecycle: u64,
    /// Owner view: 0 polling-owned, 1 async-owned.
    pub owner: u64,
    /// ISR event publishes.
    pub isr_publish: u64,
    /// ISR wake calls.
    pub isr_wake: u64,
    /// Explicit software-only wake requests.
    pub software_nudge: u64,
    /// Queue-task polls.
    pub task_poll: u64,
    /// Completions reaped.
    pub reaped: u64,
    /// Descriptors refilled.
    pub refilled: u64,
    /// IP packets delivered.
    pub delivered: u64,
    /// Non-IP completions consumed.
    pub non_ip_consumed: u64,
    /// Budget-exhausted rounds with backlog.
    pub budget_exhausted: u64,
    /// Self-yield wakes.
    pub self_yield: u64,
    /// Router-full waits.
    pub router_full_wait: u64,
    /// Space wakes.
    pub space_wake: u64,
    /// Empty-queue rechecks.
    pub empty_check: u64,
    /// Terminal faults.
    pub fault: u64,
    /// Last error stage code.
    pub last_error_stage: u64,
    /// Last error code.
    pub last_error_code: u64,
}

/// Pure snapshot mapping over a lifecycle and telemetry pair.
///
/// Exposed so host tests can build a snapshot from injected local state;
/// [`rx_snapshot`] binds the production globals and delegates here.
fn rx_snapshot_impl(lifecycle: &RxLifecycle, telemetry: &RxTelemetry) -> RxSnapshot {
    let lifecycle = lifecycle.load();
    let owner = match lifecycle.owner_view() {
        RxOwnerView::PollingOwned => 0,
        RxOwnerView::AsyncOwned => 1,
    };
    let t = telemetry;
    let (last_error_stage, last_error_code) = t.last_error();
    RxSnapshot {
        lifecycle: lifecycle.code() as u64,
        owner,
        isr_publish: t.isr_publish.load(Ordering::Relaxed),
        isr_wake: t.isr_wake.load(Ordering::Relaxed),
        software_nudge: t.software_nudge.load(Ordering::Relaxed),
        task_poll: t.task_poll.load(Ordering::Relaxed),
        reaped: t.reaped.load(Ordering::Relaxed),
        refilled: t.refilled.load(Ordering::Relaxed),
        delivered: t.delivered.load(Ordering::Relaxed),
        non_ip_consumed: t.non_ip_consumed.load(Ordering::Relaxed),
        budget_exhausted: t.budget_exhausted.load(Ordering::Relaxed),
        self_yield: t.self_yield.load(Ordering::Relaxed),
        router_full_wait: t.router_full_wait.load(Ordering::Relaxed),
        space_wake: t.space_wake.load(Ordering::Relaxed),
        empty_check: t.empty_check.load(Ordering::Relaxed),
        fault: t.fault.load(Ordering::Relaxed),
        last_error_stage,
        last_error_code,
    }
}

/// Read-only RX snapshot for the kernel ioctl. Never takes the Service lock.
pub fn rx_snapshot() -> RxSnapshot {
    rx_snapshot_impl(&RX_LIFECYCLE, &RX_TELEMETRY)
}

/// ISR-safe queue event publisher (Task 3.3).
///
/// The kernel handler calls this *after* device ACK and telemetry for any
/// used-ring cause. The used ring is direction-ambiguous: the ISR cannot tell
/// RX from TX completions, so this publishes one generic queue event that
/// wakes both the queue-owner role (the task queries both directions under
/// the Service) and the stack-progress role (socket waiters re-evaluate
/// readiness). It never touches the Service, queue-control, descriptors or
/// smoltcp, and a config-only / unknown-only / zero cause never publishes.
pub fn publish_queue_event() {
    RX_TELEMETRY.isr_publish.fetch_add(1, Ordering::Relaxed);
    RX_TELEMETRY.isr_wake.fetch_add(1, Ordering::Relaxed);
    QUEUE_EVENT.publish_event();
}

/// Backwards-compatible alias for the ISR event publisher.
pub fn publish_rx_event() {
    publish_queue_event();
}

fn software_nudge_impl(notify: &QueueEvent, telemetry: &RxTelemetry) {
    telemetry.software_nudge.fetch_add(1, Ordering::Relaxed);
    notify.queue_waker.wake();
}

/// Wake the unique RX task without publishing a hardware event.
pub fn software_nudge() {
    software_nudge_impl(&QUEUE_EVENT, &RX_TELEMETRY);
}

/// Fixed name of the single async RX queue task.
pub const RX_TASK_NAME: &str = "axnet-rx-queue";

/// Where the queue task acquires the [`Service`] from.
///
/// Host tests cannot lock the production `SERVICE` (an [`axsync::Mutex`] whose
/// lock needs an axtask scheduler), so the future is polled against an
/// injected spin mutex instead.
#[derive(Clone, Copy)]
pub(crate) enum ServiceAccess {
    /// The production global `SERVICE` singleton.
    Global,
    /// Host-test seam over a caller-owned mutex.
    #[cfg(test)]
    Injected(&'static spin::Mutex<Service>),
}

/// A locked [`Service`], derefing regardless of which access was used.
pub(crate) enum ServiceGuard<'a> {
    Global(axsync::MutexGuard<'a, Service>),
    #[cfg(test)]
    Injected(spin::MutexGuard<'a, Service>),
}

impl Deref for ServiceGuard<'_> {
    type Target = Service;

    fn deref(&self) -> &Service {
        match self {
            Self::Global(g) => g,
            #[cfg(test)]
            Self::Injected(g) => g,
        }
    }
}

impl DerefMut for ServiceGuard<'_> {
    fn deref_mut(&mut self) -> &mut Service {
        match self {
            Self::Global(g) => g,
            #[cfg(test)]
            Self::Injected(g) => g,
        }
    }
}

impl ServiceAccess {
    fn is_available(&self) -> bool {
        match self {
            Self::Global => crate::SERVICE.get().is_some(),
            #[cfg(test)]
            Self::Injected(_) => true,
        }
    }

    fn try_lock(&self) -> Option<ServiceGuard<'_>> {
        match self {
            Self::Global => crate::SERVICE.get().map(|m| ServiceGuard::Global(m.lock())),
            #[cfg(test)]
            Self::Injected(m) => Some(ServiceGuard::Injected(m.lock())),
        }
    }
}

/// The unique RX queue task future.
///
/// The task is spawned exactly once after a successful
/// [`start_rx_task`] CAS; its first poll runs activation (preflight +
/// suppression + Active/Unavailable publish) under the Service guard, then
/// every poll services at most [`RX_BUDGET`] completions and ends every
/// Pending/Ready path with the Service guard released.
pub(crate) struct RxRxFuture {
    service: ServiceAccess,
    lifecycle: &'static RxLifecycle,
    notify: &'static QueueEvent,
    telemetry: &'static RxTelemetry,
}

/// Outcome of one RX servicing round before releasing the guard.
enum RoundOutcome {
    /// A self-wake plus Pending is required (visible backlog remains).
    SelfWakeYield,
    /// Run the empty-queue register/arm/recheck protocol.
    RegisterRecheck,
    /// Wait for a resource release (slot space or Router space), possibly
    /// retrying.
    WaitSpace(SpaceDecision),
    /// Terminal queue/device fault.
    Fault(DevError),
}

impl RxRxFuture {
    /// Polls the Service under the lock until a scheduling point, then
    /// returns the next action. The guard never crosses a Pending/Ready.
    ///
    /// Task 3.2: one round runs three independent, fixed-order stages with
    /// their own budgets — TX reclaim ≤32, RX copy/refill ≤32, TX submit
    /// ≤32. Exhausting one stage never skips a later stage. After the
    /// stages, a visible backlog self-wakes/yields once; no work sleeps via
    /// the register/arm/recheck protocol.
    fn service_round(&self, service: &mut Service) -> RoundOutcome {
        // Stage 1: TX completion reclaim (≤32). Releasing a completion
        // frees a driver buffer and its live ticket.
        let mut reclaimed = 0usize;
        loop {
            match service.tx_reclaim_one_target() {
                TxReclaimStep::Reclaimed => {
                    reclaimed += 1;
                    self.telemetry.tx_reclaimed.fetch_add(1, Ordering::Relaxed);
                    if reclaimed >= RECLAIM_BUDGET {
                        self.telemetry
                            .budget_exhausted
                            .fetch_add(1, Ordering::Relaxed);
                        break;
                    }
                }
                TxReclaimStep::Empty => break,
                TxReclaimStep::Fault(err) => {
                    self.telemetry
                        .record_fault(rx_error_stage::RECEIVE_RECYCLE, &err);
                    return RoundOutcome::Fault(err);
                }
            }
        }

        // Stage 2: RX copy/refill (≤32). A full slot never reaps a used
        // descriptor, so no frame is dropped; the stage stops and the round
        // continues with TX submit.
        let mut copied = 0usize;
        let mut rx_full = false;
        loop {
            match service.rx_copy_one_target() {
                RxCopyStep::Copied => {
                    copied += 1;
                    self.telemetry.reaped.fetch_add(1, Ordering::Relaxed);
                    self.telemetry.refilled.fetch_add(1, Ordering::Relaxed);
                    // A new frame in the RX slot is stack-progress: wake the
                    // socket role so smoltcp re-evaluates readiness (T3.3).
                    self.notify.publish_progress();
                    if copied >= RX_BUDGET {
                        self.telemetry
                            .budget_exhausted
                            .fetch_add(1, Ordering::Relaxed);
                        break;
                    }
                }
                RxCopyStep::Empty => break,
                RxCopyStep::Full => {
                    rx_full = true;
                    self.telemetry.rx_slot_full.fetch_add(1, Ordering::Relaxed);
                    break;
                }
                RxCopyStep::Fault(err) => {
                    self.telemetry
                        .record_fault(rx_error_stage::RECEIVE_RECYCLE, &err);
                    return RoundOutcome::Fault(err);
                }
            }
        }

        // Stage 3: TX slot submit (≤32). A successful submit pops the slot
        // and keeps its ticket live; `Again` retains the slot frame and
        // stops this stage.
        let mut submitted = 0usize;
        let mut submit_full = false;
        loop {
            match service.tx_submit_one_target() {
                TxSubmitStep::Submitted => {
                    submitted += 1;
                    self.telemetry.tx_submitted.fetch_add(1, Ordering::Relaxed);
                    // A freed TX slot is stack-progress: wake the socket
                    // role so blocked senders re-check write readiness (T3.3).
                    self.notify.publish_progress();
                    if submitted >= SUBMIT_BUDGET {
                        self.telemetry
                            .budget_exhausted
                            .fetch_add(1, Ordering::Relaxed);
                        break;
                    }
                }
                TxSubmitStep::Empty => break,
                TxSubmitStep::Full => {
                    submit_full = true;
                    self.telemetry.tx_again.fetch_add(1, Ordering::Relaxed);
                    break;
                }
                TxSubmitStep::Fault(err) => {
                    self.telemetry
                        .record_fault(rx_error_stage::RECEIVE_RECYCLE, &err);
                    return RoundOutcome::Fault(err);
                }
            }
        }

        // Round-end scheduling decision. Any visible completion or pending
        // slot work self-wakes/yields once so the round re-runs promptly;
        // a full RX slot waits for the stack to drain it; nothing visible
        // runs the register/arm/recheck protocol and sleeps.
        let pending = match service.completion_pending_both_target() {
            Ok(pending) => pending,
            Err(err) => {
                self.telemetry
                    .record_fault(rx_error_stage::COMPLETION_QUERY, &err);
                return RoundOutcome::Fault(err);
            }
        };
        let tx_pending = service.tx_slot_pending_target();
        if rx_full {
            let decision = service.rx_slot_space_recheck_or_wait();
            if decision == SpaceDecision::Waiting {
                self.telemetry
                    .router_full_wait
                    .fetch_add(1, Ordering::Relaxed);
            }
            RoundOutcome::WaitSpace(decision)
        } else if pending.contains(NetQueueDirection::RX)
            || pending.contains(NetQueueDirection::TX)
            || tx_pending
            || submit_full
        {
            self.telemetry
                .budget_exhausted
                .fetch_add(1, Ordering::Relaxed);
            self.telemetry.self_yield.fetch_add(1, Ordering::Relaxed);
            RoundOutcome::SelfWakeYield
        } else {
            self.telemetry.empty_check.fetch_add(1, Ordering::Relaxed);
            RoundOutcome::RegisterRecheck
        }
    }

    /// First poll: acquire the Service, run the all-or-nothing bidirectional
    /// activation (suppress BOTH + slot-mode switch), publish Active (or
    /// Unavailable) under the guard, then hand off to the active servicing
    /// loop.
    fn poll_first(&self, cx: &mut Context<'_>) -> Poll<()> {
        if !self.service.is_available() {
            // Missing Service cannot be preflighted: Unavailable keeps the
            // polling owner (D4), never panics and never pends forever.
            self.telemetry
                .record_last_error(rx_error_stage::PREFLIGHT, &DevError::BadState);
            self.transition_preflight(false);
            return Poll::Ready(());
        }
        let Some(mut service) = self.service.try_lock() else {
            self.notify.register_queue(cx.waker());
            return Poll::Pending;
        };
        let preflight = service.activate_target();
        if let Err(err) = &preflight {
            self.telemetry
                .record_last_error(rx_error_stage::PREFLIGHT, err);
        }
        let preflight_ok = preflight.is_ok();
        self.transition_preflight(preflight_ok);
        drop(service);
        if preflight_ok {
            self.poll_active(cx)
        } else {
            Poll::Ready(())
        }
    }

    /// Records the illegal-lifecycle transition as a LIFECYCLE-stage error.
    ///
    /// The payload is the observed lifecycle state code, which is stable and
    /// never derived from the enum discriminant position alone.
    fn transition_preflight(&self, ok: bool) {
        if let Err(TransitionError::Illegal(state)) = self.lifecycle.preflight(ok) {
            self.telemetry
                .record_last_error_code(rx_error_stage::LIFECYCLE, state.code() as u64);
        }
    }

    /// Active poll: register the sole waker outside the Service lock, then
    /// service at most RX_BUDGET completions under the guard.
    fn poll_active(&self, cx: &mut Context<'_>) -> Poll<()> {
        self.notify.register_queue(cx.waker());
        let Some(mut service) = self.service.try_lock() else {
            return Poll::Pending;
        };
        match self.service_round(&mut service) {
            RoundOutcome::SelfWakeYield => {
                drop(service);
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            RoundOutcome::WaitSpace(SpaceDecision::Retry) => {
                drop(service);
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            RoundOutcome::WaitSpace(SpaceDecision::Waiting) => {
                drop(service);
                Poll::Pending
            }
            RoundOutcome::RegisterRecheck => {
                drop(service);
                self.poll_register_recheck(cx)
            }
            RoundOutcome::Fault(_err) => {
                self.transition_fatal();
                drop(service);
                Poll::Ready(())
            }
        }
    }

    /// Records an illegal `Active -> Faulted` transition as LIFECYCLE-stage.
    fn transition_fatal(&self) {
        if let Err(TransitionError::Illegal(state)) = self.lifecycle.fatal() {
            self.telemetry
                .record_last_error_code(rx_error_stage::LIFECYCLE, state.code() as u64);
        }
    }

    /// Empty-queue wait: acquire generation, register, arm/recheck BOTH
    /// directions under the Service lock, then observe the generation again.
    fn poll_register_recheck(&self, cx: &mut Context<'_>) -> Poll<()> {
        let decision = self.notify.wait_decision(cx.waker(), || {
            let Some(mut service) = self.service.try_lock() else {
                return Err(DevError::BadState);
            };
            service.arm_and_check_both_target().map(|pending| {
                if pending != NetQueueDirection::NONE {
                    ArmObservation::Pending
                } else {
                    ArmObservation::Quiescent
                }
            })
        });
        match decision {
            WaitDecision::Retry => {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            WaitDecision::Sleep => Poll::Pending,
            WaitDecision::Fault(err) => {
                self.telemetry.record_fault(rx_error_stage::ARM, &err);
                self.transition_fatal();
                Poll::Ready(())
            }
        }
    }
}

impl Future for RxRxFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        self.telemetry.task_poll.fetch_add(1, Ordering::Relaxed);
        match self.lifecycle.load() {
            RxTaskLifecycle::Spawned => self.poll_first(cx),
            RxTaskLifecycle::Active => self.poll_active(cx),
            // Terminal/unavailable states: the task exits; polling keeps the
            // owner for Spawned/Unavailable.
            _ => Poll::Ready(()),
        }
    }
}

/// Spawn seam. Host tests inject their own counting closure instead of
/// running the axtask scheduler or touching any production global.
#[cfg(not(test))]
fn spawn_rx_task() {
    axtask::spawn_with_name(
        || {
            axtask::future::block_on(RxRxFuture {
                service: ServiceAccess::Global,
                lifecycle: &RX_LIFECYCLE,
                notify: &QUEUE_EVENT,
                telemetry: &RX_TELEMETRY,
            })
        },
        RX_TASK_NAME.to_owned(),
    );
}

/// Test-mode binding so the production [`start_rx_task`] wrapper still
/// compiles. Tests never call it: they exercise [`start_with`] with a local
/// lifecycle and counting closure, so the global is never advanced.
#[cfg(test)]
fn spawn_rx_task() {}

/// Core start decision: CAS the given lifecycle `Polling -> Spawned`, then
/// run the spawn action exactly once.
///
/// Production binds the global lifecycle and the fixed-name spawn via
/// [`start_rx_task`]; host tests inject a local lifecycle and a counting
/// closure so the production `RX_LIFECYCLE` is never advanced by a test.
fn start_with(lifecycle: &RxLifecycle, spawn: impl FnOnce()) -> Result<(), StartError> {
    lifecycle.start()?;
    spawn();
    Ok(())
}

/// Activates the async RX path. The CAS winner alone requests one fixed-name
/// spawn; a repeated call returns `AlreadyStarted` without a second task.
///
/// The kernel calls this only after the VirtIO-net IRQ handler has been
/// registered, so no task can suppress notifications without a wake source.
pub fn start_rx_task() -> Result<(), StartError> {
    start_with(&RX_LIFECYCLE, spawn_rx_task)
}

/// Outcome of the Service-guard full-space recheck.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SpaceDecision {
    /// Space is already available; the caller must retry instead of sleeping.
    Retry,
    /// Still full; the waiting bit was published and the caller may pend.
    Waiting,
}

/// Lifecycle of the async RX queue task.
///
/// Monotonic: `Polling -> Spawned -> Active -> Faulted`, or `Spawned ->
/// Unavailable` when preflight fails. No transition ever rolls the owner back
/// to an earlier state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RxTaskLifecycle {
    Polling,
    Spawned,
    Active,
    Faulted,
    Unavailable,
}

impl RxTaskLifecycle {
    const fn code(self) -> u8 {
        match self {
            Self::Polling => 0,
            Self::Spawned => 1,
            Self::Active => 2,
            Self::Faulted => 3,
            Self::Unavailable => 4,
        }
    }

    fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Polling,
            1 => Self::Spawned,
            2 => Self::Active,
            3 => Self::Faulted,
            4 => Self::Unavailable,
            _ => unreachable!("lifecycle code out of range"),
        }
    }

    /// Consumption-right view: the async task owns RX only once `Active`, and
    /// keeps it after a fatal fault so polling never silently resumes.
    pub(crate) fn owner_view(self) -> RxOwnerView {
        match self {
            Self::Active | Self::Faulted => RxOwnerView::AsyncOwned,
            Self::Polling | Self::Spawned | Self::Unavailable => RxOwnerView::PollingOwned,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartError {
    AlreadyStarted(RxTaskLifecycle),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TransitionError {
    Illegal(RxTaskLifecycle),
}

/// Atomic lifecycle state. Loads are Acquire; successful transitions are
/// AcqRel CAS and failure observations are Acquire, so the owner view never
/// observes a torn state.
pub(crate) struct RxLifecycle {
    state: AtomicU8,
}

impl RxLifecycle {
    pub(crate) const fn new() -> Self {
        Self {
            state: AtomicU8::new(RxTaskLifecycle::Polling.code()),
        }
    }

    pub(crate) fn load(&self) -> RxTaskLifecycle {
        RxTaskLifecycle::from_code(self.state.load(Ordering::Acquire))
    }

    /// `Polling -> Spawned`. A second start reports the current state instead
    /// of making a spawn decision.
    pub(crate) fn start(&self) -> Result<(), StartError> {
        self.transition(RxTaskLifecycle::Polling, RxTaskLifecycle::Spawned)
            .map_err(|TransitionError::Illegal(current)| StartError::AlreadyStarted(current))
    }

    /// Preflight outcome: `Spawned -> Active` on success, `Spawned ->
    /// Unavailable` on failure. Polling remains the owner in the latter case.
    pub(crate) fn preflight(&self, ok: bool) -> Result<(), TransitionError> {
        let next = if ok {
            RxTaskLifecycle::Active
        } else {
            RxTaskLifecycle::Unavailable
        };
        self.transition(RxTaskLifecycle::Spawned, next)
    }

    /// `Active -> Faulted`. Never restores the polling owner.
    pub(crate) fn fatal(&self) -> Result<(), TransitionError> {
        self.transition(RxTaskLifecycle::Active, RxTaskLifecycle::Faulted)
    }

    fn transition(
        &self,
        from: RxTaskLifecycle,
        to: RxTaskLifecycle,
    ) -> Result<(), TransitionError> {
        self.state
            .compare_exchange(from.code(), to.code(), Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|current| TransitionError::Illegal(RxTaskLifecycle::from_code(current)))
    }

    pub(crate) fn owner_view(&self) -> RxOwnerView {
        self.load().owner_view()
    }
}

/// Serializes tests that touch the shared [`QUEUE_EVENT`] static.
#[cfg(test)]
pub(crate) static SERIAL: spin::Mutex<()> = spin::Mutex::new(());

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, collections::VecDeque, sync::Arc, vec, vec::Vec};
    use core::{
        pin::Pin,
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
        task::{Context, Poll, Waker},
    };

    use axdriver::prelude::{DevError, DevResult};
    use axdriver_net::{NetQueueControl, NetQueueDirection};
    use smoltcp::{storage::PacketBuffer, time::Instant, wire::IpAddress};

    use super::{
        ArmObservation, QUEUE_EVENT, QueueEvent, RECLAIM_BUDGET, RX_BUDGET, RX_LIFECYCLE,
        RX_TELEMETRY, RxLifecycle, RxRxFuture, RxTaskLifecycle, RxTelemetry, SERIAL, SUBMIT_BUDGET,
        ServiceAccess, SpaceDecision, StartError, TransitionError, WaitDecision, rx_error_code,
        rx_error_stage, software_nudge_impl, start_with,
    };
    use crate::{
        device::{
            Device, LoopbackDevice, RxCopyStep, RxStep, TxOutcome, TxPreflight, TxReclaimStep,
            TxSubmitStep,
        },
        router::{Router, RxOwnerView},
        service::Service,
    };

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

    /// Counts `recv` calls so tests can prove the device was not touched.
    struct CountingDevice {
        recv_calls: Arc<AtomicUsize>,
    }

    impl CountingDevice {
        fn new(recv_calls: Arc<AtomicUsize>) -> Self {
            Self { recv_calls }
        }
    }

    impl Device for CountingDevice {
        fn name(&self) -> &str {
            "counting"
        }

        fn recv(&mut self, _buffer: &mut PacketBuffer<()>, _timestamp: Instant) -> RxStep {
            self.recv_calls.fetch_add(1, Ordering::Relaxed);
            RxStep::Empty
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

    #[test]
    fn notify_full_waiting_then_space_wakes_once() {
        let notify = QueueEvent::new();
        let count = Arc::new(AtomicUsize::new(0));
        notify.register_queue(&counting_waker(count.clone()));
        notify.publish_waiting();
        assert!(notify.wake_if_space(true));
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert!(!notify.wake_if_space(true));
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn notify_still_full_does_not_wake() {
        let notify = QueueEvent::new();
        let count = Arc::new(AtomicUsize::new(0));
        notify.register_queue(&counting_waker(count.clone()));
        notify.publish_waiting();
        assert!(!notify.wake_if_space(false));
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn notify_not_waiting_does_not_wake() {
        let notify = QueueEvent::new();
        let count = Arc::new(AtomicUsize::new(0));
        notify.register_queue(&counting_waker(count.clone()));
        assert!(!notify.wake_if_space(true));
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn notify_second_publish_after_clear_wakes_again() {
        let notify = QueueEvent::new();
        let count = Arc::new(AtomicUsize::new(0));
        notify.register_queue(&counting_waker(count.clone()));
        notify.publish_waiting();
        assert!(notify.wake_if_space(true));
        notify.publish_waiting();
        assert!(notify.wake_if_space(true));
        assert!(!notify.wake_if_space(true));
        assert_eq!(count.load(Ordering::Relaxed), 2);
    }

    // ---- T3.1: dual-role QueueEvent and bidirectional activation ----

    #[test]
    fn event_queue_and_stack_wakers_are_independent() {
        let event = super::QueueEvent::new();
        let queue_count = Arc::new(AtomicUsize::new(0));
        let stack_count = Arc::new(AtomicUsize::new(0));
        event.register_queue(&counting_waker(queue_count.clone()));
        event.register_stack(&counting_waker(stack_count.clone()));

        // A queue event wakes both roles.
        event.publish_event();
        assert_eq!(queue_count.load(Ordering::Relaxed), 1);
        assert_eq!(stack_count.load(Ordering::Relaxed), 1);

        // A stack-progress hint wakes only the stack role.
        event.publish_progress();
        assert_eq!(queue_count.load(Ordering::Relaxed), 1);
        assert_eq!(stack_count.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn event_queue_register_does_not_overwrite_stack_waker() {
        let event = super::QueueEvent::new();
        let queue_count = Arc::new(AtomicUsize::new(0));
        let stack_count = Arc::new(AtomicUsize::new(0));
        event.register_stack(&counting_waker(stack_count.clone()));
        event.register_queue(&counting_waker(queue_count.clone()));
        event.publish_event();
        assert_eq!(queue_count.load(Ordering::Relaxed), 1);
        assert_eq!(stack_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn event_stack_register_does_not_overwrite_queue_waker() {
        let event = super::QueueEvent::new();
        let queue_count = Arc::new(AtomicUsize::new(0));
        let stack_count = Arc::new(AtomicUsize::new(0));
        event.register_queue(&counting_waker(queue_count.clone()));
        event.register_stack(&counting_waker(stack_count.clone()));
        event.publish_progress();
        assert_eq!(queue_count.load(Ordering::Relaxed), 0);
        assert_eq!(stack_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn event_generation_wraps_and_wakes_both() {
        let event = super::QueueEvent::with_generation(u64::MAX);
        let queue_count = Arc::new(AtomicUsize::new(0));
        let stack_count = Arc::new(AtomicUsize::new(0));
        event.register_queue(&counting_waker(queue_count.clone()));
        event.register_stack(&counting_waker(stack_count.clone()));
        event.publish_event();
        assert_eq!(event.generation(), 0);
        assert_eq!(queue_count.load(Ordering::Relaxed), 1);
        assert_eq!(stack_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn event_space_wait_wakes_only_queue_role() {
        let event = super::QueueEvent::new();
        let queue_count = Arc::new(AtomicUsize::new(0));
        let stack_count = Arc::new(AtomicUsize::new(0));
        event.register_queue(&counting_waker(queue_count.clone()));
        event.register_stack(&counting_waker(stack_count.clone()));
        event.publish_waiting();
        assert!(event.wake_if_space(true));
        assert_eq!(queue_count.load(Ordering::Relaxed), 1);
        assert_eq!(stack_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn event_dual_role_wait_decision_retries_on_any_generation_change() {
        let event = super::QueueEvent::new();
        let queue_count = Arc::new(AtomicUsize::new(0));
        let stack_count = Arc::new(AtomicUsize::new(0));
        let before = event.generation();
        let decision = event.wait_decision(&counting_waker(queue_count.clone()), || {
            event.register_stack(&counting_waker(stack_count.clone()));
            event.publish_progress();
            Ok(ArmObservation::Quiescent)
        });
        // The queue wait observes the stack-role generation change and retries.
        assert!(matches!(decision, WaitDecision::Retry));
        assert_eq!(before, event.generation() - 1);
    }

    #[test]
    fn rx_copy_missing_target_maps_to_bad_state() {
        let router = Router::new();
        let mut service = Service::new(router, None);

        let step = service.rx_copy_one_target();
        assert!(matches!(step, RxCopyStep::Fault(DevError::BadState)));
    }

    #[test]
    fn tx_submit_missing_target_maps_to_bad_state() {
        let router = Router::new();
        let mut service = Service::new(router, None);

        let step = service.tx_submit_one_target();
        assert!(matches!(step, TxSubmitStep::Fault(DevError::BadState)));
    }

    #[test]
    fn tx_reclaim_missing_target_maps_to_bad_state() {
        let router = Router::new();
        let mut service = Service::new(router, None);

        let step = service.tx_reclaim_one_target();
        assert!(matches!(step, TxReclaimStep::Fault(DevError::BadState)));
    }

    #[test]
    fn space_freed_before_waiting_rechecks_to_retry_without_publish() {
        let _serial = SERIAL.lock();
        let count = Arc::new(AtomicUsize::new(0));
        QUEUE_EVENT.register_queue(&counting_waker(count.clone()));

        let router = Router::new();
        let service = Service::new(router, None);

        let decision = service.rx_slot_space_recheck_or_wait();
        assert!(matches!(decision, SpaceDecision::Retry));
        // Retry must not have published waiting: a later space wake is a no-op.
        assert!(!QUEUE_EVENT.wake_if_space(true));
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn still_full_publishes_waiting_then_space_wakes_once() {
        let _serial = SERIAL.lock();
        let count = Arc::new(AtomicUsize::new(0));
        QUEUE_EVENT.register_queue(&counting_waker(count.clone()));

        let (mutex, _, control) = leaked_service(vec![RxStep::Consumed], true);
        // The fixed RX slots are full: the slot-space recheck must publish
        // the waiting bit.
        control.rx_slot_full.store(true, Ordering::Relaxed);
        let service = mutex.lock();

        let decision = service.rx_slot_space_recheck_or_wait();
        assert!(matches!(decision, SpaceDecision::Waiting));

        // Space freed after waiting: exactly one wake.
        assert!(QUEUE_EVENT.wake_if_space(true));
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert!(!QUEUE_EVENT.wake_if_space(true));
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    fn drive_to(state: RxTaskLifecycle) -> RxLifecycle {
        let lifecycle = RxLifecycle::new();
        match state {
            RxTaskLifecycle::Polling => {}
            RxTaskLifecycle::Spawned => {
                lifecycle.start().unwrap();
            }
            RxTaskLifecycle::Active => {
                lifecycle.start().unwrap();
                lifecycle.preflight(true).unwrap();
            }
            RxTaskLifecycle::Faulted => {
                lifecycle.start().unwrap();
                lifecycle.preflight(true).unwrap();
                lifecycle.fatal().unwrap();
            }
            RxTaskLifecycle::Unavailable => {
                lifecycle.start().unwrap();
                lifecycle.preflight(false).unwrap();
            }
        }
        assert_eq!(lifecycle.load(), state);
        lifecycle
    }

    #[test]
    fn lifecycle_start_moves_polling_to_spawned() {
        let lifecycle = RxLifecycle::new();
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Polling);
        lifecycle.start().unwrap();
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Spawned);
    }

    #[test]
    fn lifecycle_duplicate_start_is_rejected_without_state_change() {
        for state in [
            RxTaskLifecycle::Spawned,
            RxTaskLifecycle::Active,
            RxTaskLifecycle::Faulted,
            RxTaskLifecycle::Unavailable,
        ] {
            let lifecycle = drive_to(state);
            assert_eq!(lifecycle.start(), Err(StartError::AlreadyStarted(state)));
            assert_eq!(lifecycle.load(), state);
        }
    }

    #[test]
    fn lifecycle_preflight_only_from_spawned() {
        for state in [
            RxTaskLifecycle::Polling,
            RxTaskLifecycle::Active,
            RxTaskLifecycle::Faulted,
            RxTaskLifecycle::Unavailable,
        ] {
            for ok in [true, false] {
                let lifecycle = drive_to(state);
                assert_eq!(
                    lifecycle.preflight(ok),
                    Err(TransitionError::Illegal(state))
                );
                assert_eq!(lifecycle.load(), state);
            }
        }
    }

    #[test]
    fn lifecycle_preflight_outcomes_from_spawned() {
        let lifecycle = drive_to(RxTaskLifecycle::Spawned);
        lifecycle.preflight(true).unwrap();
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);

        let lifecycle = drive_to(RxTaskLifecycle::Spawned);
        lifecycle.preflight(false).unwrap();
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Unavailable);
        assert_eq!(lifecycle.owner_view(), RxOwnerView::PollingOwned);
    }

    #[test]
    fn lifecycle_fatal_only_from_active() {
        for state in [
            RxTaskLifecycle::Polling,
            RxTaskLifecycle::Spawned,
            RxTaskLifecycle::Faulted,
            RxTaskLifecycle::Unavailable,
        ] {
            let lifecycle = drive_to(state);
            assert_eq!(lifecycle.fatal(), Err(TransitionError::Illegal(state)));
            assert_eq!(lifecycle.load(), state);
        }

        let lifecycle = drive_to(RxTaskLifecycle::Active);
        lifecycle.fatal().unwrap();
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
    }

    #[test]
    fn lifecycle_terminal_states_accept_no_transition() {
        for state in [RxTaskLifecycle::Faulted, RxTaskLifecycle::Unavailable] {
            let lifecycle = drive_to(state);
            assert!(lifecycle.start().is_err());
            assert!(lifecycle.preflight(true).is_err());
            assert!(lifecycle.preflight(false).is_err());
            assert!(lifecycle.fatal().is_err());
            assert_eq!(lifecycle.load(), state);
        }
    }

    #[test]
    fn lifecycle_owner_view_mapping() {
        for (state, expected) in [
            (RxTaskLifecycle::Polling, RxOwnerView::PollingOwned),
            (RxTaskLifecycle::Spawned, RxOwnerView::PollingOwned),
            (RxTaskLifecycle::Active, RxOwnerView::AsyncOwned),
            (RxTaskLifecycle::Faulted, RxOwnerView::AsyncOwned),
            (RxTaskLifecycle::Unavailable, RxOwnerView::PollingOwned),
        ] {
            assert_eq!(state.owner_view(), expected);
            assert_eq!(drive_to(state).owner_view(), expected);
        }
    }

    #[test]
    fn publish_event_increments_generation_and_wakes() {
        let notify = QueueEvent::new();
        let count = Arc::new(AtomicUsize::new(0));
        notify.register_queue(&counting_waker(count.clone()));

        let before = notify.generation();
        notify.publish_event();
        assert_eq!(notify.generation(), before + 1);
        assert_eq!(count.load(Ordering::Relaxed), 1);

        notify.publish_event();
        assert_eq!(notify.generation(), before + 2);
        assert_eq!(count.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn software_nudge_wakes_without_publishing_hardware_event() {
        let notify = QueueEvent::new();
        let telemetry = RxTelemetry::new();
        let count = Arc::new(AtomicUsize::new(0));
        notify.register_queue(&counting_waker(count.clone()));
        let generation_before = notify.generation();

        software_nudge_impl(&notify, &telemetry);

        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert_eq!(notify.generation(), generation_before);
        assert_eq!(telemetry.isr_publish.load(Ordering::Relaxed), 0);
        assert_eq!(telemetry.isr_wake.load(Ordering::Relaxed), 0);
        assert_eq!(telemetry.software_nudge.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn publish_event_generation_wraps() {
        let notify = QueueEvent::with_generation(u64::MAX);
        notify.publish_event();
        assert_eq!(notify.generation(), 0);
    }

    #[test]
    fn event_before_register_is_caught_by_arm_recheck() {
        let notify = QueueEvent::new();
        let count = Arc::new(AtomicUsize::new(0));
        notify.publish_event();

        let decision = notify.wait_decision(&counting_waker(count.clone()), || {
            Ok(ArmObservation::Pending)
        });
        assert!(matches!(decision, WaitDecision::Retry));
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn event_during_register_window_retries() {
        let notify = QueueEvent::new();
        let count = Arc::new(AtomicUsize::new(0));

        let decision = notify.wait_decision(&counting_waker(count.clone()), || {
            notify.publish_event();
            Ok(ArmObservation::Quiescent)
        });
        assert!(matches!(decision, WaitDecision::Retry));
    }

    #[test]
    fn event_after_arm_wakes_sleep_decision() {
        let notify = QueueEvent::new();
        let count = Arc::new(AtomicUsize::new(0));

        let decision = notify.wait_decision(&counting_waker(count.clone()), || {
            Ok(ArmObservation::Quiescent)
        });
        assert!(matches!(decision, WaitDecision::Sleep));

        notify.publish_event();
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn pending_found_by_arm_recheck_retries_without_event() {
        let notify = QueueEvent::new();
        let count = Arc::new(AtomicUsize::new(0));

        let decision = notify.wait_decision(&counting_waker(count.clone()), || {
            Ok(ArmObservation::Pending)
        });
        assert!(matches!(decision, WaitDecision::Retry));
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn quiescent_arm_without_event_sleeps() {
        let notify = QueueEvent::new();
        let count = Arc::new(AtomicUsize::new(0));

        let decision = notify.wait_decision(&counting_waker(count.clone()), || {
            Ok(ArmObservation::Quiescent)
        });
        assert!(matches!(decision, WaitDecision::Sleep));
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn arm_error_maps_to_fault_with_error_category() {
        let notify = QueueEvent::new();
        let count = Arc::new(AtomicUsize::new(0));

        let decision = notify.wait_decision(&counting_waker(count.clone()), || Err(DevError::Io));
        assert!(matches!(decision, WaitDecision::Fault(DevError::Io)));
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    // ---- T5.2b: unique named task, owner handoff and budget wiring ----

    /// Scripted queue-control backend shared by a fake NIC and the assertions.
    #[derive(Default)]
    struct ScriptedControlStats {
        suppress_calls: AtomicUsize,
        arm_calls: AtomicUsize,
        control_calls: AtomicUsize,
        completion_visible: AtomicBool,
        suppress_error: AtomicBool,
        arm_error: AtomicBool,
        missing_after_first_control_call: AtomicBool,
        rx_slot_full: AtomicBool,
        tx_slot_pending: AtomicBool,
    }

    struct ScriptedControl {
        stats: Arc<ScriptedControlStats>,
    }

    impl NetQueueControl for ScriptedControl {
        fn has_rx_completion(&self) -> bool {
            self.stats.completion_visible.load(Ordering::Relaxed)
        }

        fn suppress_rx_notify(&mut self) -> DevResult {
            self.stats.suppress_calls.fetch_add(1, Ordering::Relaxed);
            if self.stats.suppress_error.load(Ordering::Relaxed) {
                return Err(DevError::Io);
            }
            Ok(())
        }

        fn arm_rx_notify_and_check(&mut self) -> DevResult<bool> {
            self.stats.arm_calls.fetch_add(1, Ordering::Relaxed);
            if self.stats.arm_error.load(Ordering::Relaxed) {
                return Err(DevError::Io);
            }
            Ok(self.stats.completion_visible.load(Ordering::Relaxed))
        }

        fn suppress_notify(&mut self, directions: NetQueueDirection) -> DevResult {
            if directions.contains(NetQueueDirection::RX) {
                self.suppress_rx_notify()?;
            }
            if directions.contains(NetQueueDirection::TX) {
                self.stats.suppress_calls.fetch_add(1, Ordering::Relaxed);
                if self.stats.suppress_error.load(Ordering::Relaxed) {
                    return Err(DevError::Io);
                }
            }
            Ok(())
        }

        fn arm_notify_and_check(
            &mut self,
            directions: NetQueueDirection,
        ) -> DevResult<NetQueueDirection> {
            let mut pending = NetQueueDirection::NONE;
            if directions.contains(NetQueueDirection::RX) && self.arm_rx_notify_and_check()? {
                pending |= NetQueueDirection::RX;
            }
            if directions.contains(NetQueueDirection::TX) {
                self.stats.arm_calls.fetch_add(1, Ordering::Relaxed);
                if self.stats.arm_error.load(Ordering::Relaxed) {
                    return Err(DevError::Io);
                }
                if self.stats.completion_visible.load(Ordering::Relaxed) {
                    pending |= NetQueueDirection::TX;
                }
            }
            Ok(pending)
        }

        fn completion_pending(
            &self,
            directions: NetQueueDirection,
        ) -> DevResult<NetQueueDirection> {
            let mut pending = NetQueueDirection::NONE;
            if directions.contains(NetQueueDirection::RX)
                && self.stats.completion_visible.load(Ordering::Relaxed)
            {
                pending |= NetQueueDirection::RX;
            }
            if directions.contains(NetQueueDirection::TX)
                && self.stats.completion_visible.load(Ordering::Relaxed)
            {
                pending |= NetQueueDirection::TX;
            }
            Ok(pending)
        }
    }

    /// A fake NIC whose three queue-service stages replay scripted outcomes
    /// and whose optional queue control records calls and honors injected
    /// errors (Task 3.2 fake driver/slot matrix).
    struct ScriptedDevice {
        steps: spin::Mutex<VecDeque<RxStep>>,
        tx_submit_steps: spin::Mutex<VecDeque<TxSubmitStep>>,
        tx_reclaim_steps: spin::Mutex<VecDeque<TxReclaimStep>>,
        copy_calls: Arc<AtomicUsize>,
        submit_calls: Arc<AtomicUsize>,
        stats: Arc<ScriptedControlStats>,
        control: Option<ScriptedControl>,
    }

    impl Device for ScriptedDevice {
        fn name(&self) -> &str {
            "scripted"
        }

        fn recv(&mut self, _buffer: &mut PacketBuffer<()>, _timestamp: Instant) -> RxStep {
            self.copy_calls.fetch_add(1, Ordering::Relaxed);
            self.steps.lock().pop_front().unwrap_or(RxStep::Empty)
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

        fn rx_copy_one(&mut self) -> RxCopyStep {
            if self.stats.rx_slot_full.load(Ordering::Relaxed) {
                return RxCopyStep::Full;
            }
            self.copy_calls.fetch_add(1, Ordering::Relaxed);
            match self.steps.lock().pop_front().unwrap_or(RxStep::Empty) {
                RxStep::Consumed | RxStep::Delivered => RxCopyStep::Copied,
                RxStep::Empty => RxCopyStep::Empty,
                RxStep::Fault(err) => RxCopyStep::Fault(err),
            }
        }

        fn tx_submit_one(&mut self) -> TxSubmitStep {
            self.submit_calls.fetch_add(1, Ordering::Relaxed);
            self.tx_submit_steps
                .lock()
                .pop_front()
                .unwrap_or(TxSubmitStep::Empty)
        }

        fn tx_reclaim_one(&mut self) -> TxReclaimStep {
            self.tx_reclaim_steps
                .lock()
                .pop_front()
                .unwrap_or(TxReclaimStep::Empty)
        }

        fn rx_slot_has_space(&self) -> bool {
            !self.stats.rx_slot_full.load(Ordering::Relaxed)
        }

        fn tx_slot_pending(&self) -> bool {
            self.stats.tx_slot_pending.load(Ordering::Relaxed)
        }

        fn queue_control(&mut self) -> Option<&mut dyn NetQueueControl> {
            let call = self
                .control
                .as_ref()
                .map(|control| control.stats.control_calls.fetch_add(1, Ordering::Relaxed))
                .unwrap_or(0);
            if call > 0
                && self.control.as_ref().is_some_and(|control| {
                    control
                        .stats
                        .missing_after_first_control_call
                        .load(Ordering::Relaxed)
                })
            {
                return None;
            }
            self.control.as_mut().map(|c| c as &mut dyn NetQueueControl)
        }

        fn register_waker(&self, _waker: &Waker) {}
    }

    fn leaked_service(
        steps: Vec<RxStep>,
        with_control: bool,
    ) -> (
        &'static spin::Mutex<Service>,
        Arc<AtomicUsize>,
        Arc<ScriptedControlStats>,
    ) {
        leaked_service_tx(steps, vec![], vec![], with_control)
    }

    fn leaked_service_tx(
        steps: Vec<RxStep>,
        tx_submit_steps: Vec<TxSubmitStep>,
        tx_reclaim_steps: Vec<TxReclaimStep>,
        with_control: bool,
    ) -> (
        &'static spin::Mutex<Service>,
        Arc<AtomicUsize>,
        Arc<ScriptedControlStats>,
    ) {
        let copy_calls = Arc::new(AtomicUsize::new(0));
        let stats = Arc::new(ScriptedControlStats::default());
        let control = with_control.then(|| ScriptedControl {
            stats: stats.clone(),
        });
        let device = ScriptedDevice {
            steps: spin::Mutex::new(steps.into()),
            tx_submit_steps: spin::Mutex::new(tx_submit_steps.into()),
            tx_reclaim_steps: spin::Mutex::new(tx_reclaim_steps.into()),
            copy_calls: copy_calls.clone(),
            submit_calls: Arc::new(AtomicUsize::new(0)),
            stats: stats.clone(),
            control,
        };
        let mut router = Router::new();
        let idx = router.add_device(Box::new(device));
        let service = Service::new(router, Some(idx));
        let mutex: &'static spin::Mutex<Service> = Box::leak(Box::new(spin::Mutex::new(service)));
        (mutex, copy_calls, stats)
    }

    /// Builds an injected Future: local leaked lifecycle/notify/telemetry,
    /// spin service mutex, lifecycle already driven to `Spawned`.
    fn leaked_future(
        service_mutex: &'static spin::Mutex<Service>,
        notify: &'static QueueEvent,
    ) -> (&'static RxLifecycle, RxRxFuture) {
        let lifecycle: &'static RxLifecycle = Box::leak(Box::new(RxLifecycle::new()));
        lifecycle.start().unwrap();
        let telemetry: &'static RxTelemetry = Box::leak(Box::new(RxTelemetry::new()));
        let fut = RxRxFuture {
            service: ServiceAccess::Injected(service_mutex),
            lifecycle,
            notify,
            telemetry,
        };
        (lifecycle, fut)
    }

    fn poll_once(fut: &mut RxRxFuture, count: Arc<AtomicUsize>) -> Poll<()> {
        let waker = counting_waker(count.clone());
        let mut cx = Context::from_waker(&waker);
        Pin::new(fut).poll(&mut cx)
    }

    #[test]
    fn start_seam_spawns_once_and_rejects_duplicate_with_local_state() {
        // Local lifecycle + counting closure: never touches the production
        // globals, so any test order leaves RX_LIFECYCLE at its initial state.
        let lifecycle = RxLifecycle::new();
        let spawns = Arc::new(AtomicUsize::new(0));
        let spawn_count = spawns.clone();

        assert!(
            start_with(&lifecycle, || {
                spawn_count.fetch_add(1, Ordering::Relaxed);
            })
            .is_ok()
        );
        assert_eq!(spawns.load(Ordering::Relaxed), 1);
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Spawned);

        assert_eq!(
            start_with(&lifecycle, || {
                spawn_count.fetch_add(1, Ordering::Relaxed);
            }),
            Err(StartError::AlreadyStarted(RxTaskLifecycle::Spawned))
        );
        assert_eq!(spawns.load(Ordering::Relaxed), 1);

        // The fixed task name is bound by the production spawn path.
        assert_eq!(super::RX_TASK_NAME, "axnet-rx-queue");

        // The global lifecycle was never advanced by the seam test.
        assert_eq!(RX_LIFECYCLE.load(), RxTaskLifecycle::Polling);
    }

    #[test]
    fn future_missing_service_publishes_unavailable() {
        // `ServiceAccess::Global` resolves the never-initialized `SERVICE`
        // once in host tests: the first poll must not panic and must exit
        // with Unavailable, keeping the polling owner.
        let lifecycle: &'static RxLifecycle = Box::leak(Box::new(RxLifecycle::new()));
        lifecycle.start().unwrap();
        let notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));
        let telemetry: &'static RxTelemetry = Box::leak(Box::new(RxTelemetry::new()));
        let mut fut = RxRxFuture {
            service: ServiceAccess::Global,
            lifecycle,
            notify,
            telemetry,
        };
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(
            poll_once(&mut fut, count.clone()),
            Poll::Ready(())
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Unavailable);
        assert_eq!(lifecycle.owner_view(), RxOwnerView::PollingOwned);
    }

    #[test]
    fn future_missing_target_publishes_unavailable() {
        let service = Service::new(Router::new(), None);
        let mutex: &'static spin::Mutex<Service> = Box::leak(Box::new(spin::Mutex::new(service)));
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(
            poll_once(&mut fut, count.clone()),
            Poll::Ready(())
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Unavailable);
        assert!(mutex.try_lock().is_some());
    }

    #[test]
    fn future_missing_control_publishes_unavailable() {
        let (mutex, recv_calls, _) = leaked_service(vec![], false);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(
            poll_once(&mut fut, count.clone()),
            Poll::Ready(())
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Unavailable);
        assert_eq!(recv_calls.load(Ordering::Relaxed), 0);
        assert!(mutex.try_lock().is_some());
    }

    #[test]
    fn future_preflight_suppress_failure_publishes_unavailable() {
        let (mutex, recv_calls, control) = leaked_service(vec![], true);
        control.suppress_error.store(true, Ordering::Relaxed);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(
            poll_once(&mut fut, count.clone()),
            Poll::Ready(())
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Unavailable);
        assert_eq!(recv_calls.load(Ordering::Relaxed), 0);
        assert!(mutex.try_lock().is_some());
    }

    #[test]
    fn future_first_poll_activates_and_waits_on_empty() {
        let (mutex, copy_calls, control) = leaked_service(vec![RxStep::Empty], true);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        // The RX copy stage probes once and stops on Empty.
        assert_eq!(copy_calls.load(Ordering::Relaxed), 1);
        // Quiescent BOTH-direction arm without event: sleep without self-wake.
        assert_eq!(count.load(Ordering::Relaxed), 0);
        assert_eq!(control.arm_calls.load(Ordering::Relaxed), 2);
        assert!(mutex.try_lock().is_some());
    }

    #[test]
    fn future_services_one_completion_then_registers() {
        let (mutex, copy_calls, control) =
            leaked_service(vec![RxStep::Consumed, RxStep::Empty], true);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        // One RX copy then an Empty probe, then BOTH-direction register.
        assert_eq!(copy_calls.load(Ordering::Relaxed), 2);
        assert_eq!(control.arm_calls.load(Ordering::Relaxed), 2);
        assert!(mutex.try_lock().is_some());
    }

    #[test]
    fn future_rx_copy_publishes_stack_progress() {
        // Task 3.3: a successful RX copy fills the fixed RX slot, which is
        // stack-progress — the socket role must be woken so smoltcp
        // re-evaluates readiness. The queue-owner waker is untouched.
        let notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));
        let (mutex, ..) = leaked_service(vec![RxStep::Consumed, RxStep::Empty], true);
        let (_, mut fut) = leaked_future(mutex, notify);
        let queue_count = Arc::new(AtomicUsize::new(0));
        let stack_count = Arc::new(AtomicUsize::new(0));
        notify.register_queue(&counting_waker(queue_count.clone()));
        notify.register_stack(&counting_waker(stack_count.clone()));
        let _ = poll_once(&mut fut, Arc::new(AtomicUsize::new(0)));
        // The RX copy published a stack-progress hint; the queue role was
        // not woken by it (the task itself drives the next round).
        assert_eq!(stack_count.load(Ordering::Relaxed), 1);
        assert_eq!(queue_count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn future_31_completions_then_empty_registers_once() {
        // Exactly 31 RX copies then an Empty on the 32nd observation: the
        // future performs exactly RX_BUDGET (32) copy probes, arms BOTH
        // directions once, self-wakes zero times and releases the Service
        // guard. The literal 31 keeps the RX_BUDGET boundary witness
        // sensitive.
        let steps: Vec<RxStep> = (0..31)
            .map(|_| RxStep::Consumed)
            .chain([RxStep::Empty])
            .collect();
        let (mutex, copy_calls, control) = leaked_service(steps, true);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        assert_eq!(copy_calls.load(Ordering::Relaxed), 32);
        assert_eq!(count.load(Ordering::Relaxed), 0);
        assert_eq!(control.arm_calls.load(Ordering::Relaxed), 2);
        assert!(mutex.try_lock().is_some());
    }

    #[test]
    fn future_budget_exhausted_with_backlog_self_wakes_and_yields() {
        let steps: Vec<RxStep> = (0..=RX_BUDGET).map(|_| RxStep::Consumed).collect();
        let (mutex, copy_calls, control) = leaked_service(steps, true);
        control.completion_visible.store(true, Ordering::Relaxed);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        // Exactly RX_BUDGET copies; the 33rd is never probed by another copy.
        assert_eq!(copy_calls.load(Ordering::Relaxed), RX_BUDGET);
        // Visible completion: self-wake for block_on yield, no spurious wake.
        assert_eq!(count.load(Ordering::Relaxed), 1);
        // SelfWakeYield keeps the queue suppressed: no rearm happened.
        assert_eq!(control.arm_calls.load(Ordering::Relaxed), 0);
        assert!(mutex.try_lock().is_some());
    }

    #[test]
    fn future_budget_exhausted_without_backlog_stops_cleanly() {
        let steps: Vec<RxStep> = (0..RX_BUDGET).map(|_| RxStep::Consumed).collect();
        let (mutex, copy_calls, control) = leaked_service(steps, true);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        assert_eq!(copy_calls.load(Ordering::Relaxed), RX_BUDGET);
        assert_eq!(count.load(Ordering::Relaxed), 0);
        // Clean budget stop without backlog: BOTH-direction register/arm.
        assert_eq!(control.arm_calls.load(Ordering::Relaxed), 2);
        assert!(mutex.try_lock().is_some());
    }

    #[test]
    fn future_arm_pending_retries_with_self_wake() {
        let (mutex, _, control) = leaked_service(vec![RxStep::Empty], true);
        control.completion_visible.store(true, Ordering::Relaxed);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert!(mutex.try_lock().is_some());
    }

    #[test]
    fn future_arm_error_faults_and_keeps_async_owner() {
        let (mutex, _, control) = leaked_service(vec![RxStep::Empty], true);
        control.arm_error.store(true, Ordering::Relaxed);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(
            poll_once(&mut fut, count.clone()),
            Poll::Ready(())
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        // Fatal never restores the polling owner.
        assert_eq!(lifecycle.owner_view(), RxOwnerView::AsyncOwned);
        assert!(mutex.try_lock().is_some());
    }

    #[test]
    fn future_rx_slot_full_waits_then_service_poll_wakes() {
        // The wait/space handoff shares the production `QUEUE_EVENT` with
        // `Service::poll`: serialize against sibling tests.
        let _serial = SERIAL.lock();
        let (mutex, copy_calls, control) = leaked_service(vec![RxStep::Consumed], true);
        // The fixed RX slots are full: the copy stage stops without reaping.
        control.rx_slot_full.store(true, Ordering::Relaxed);
        let (lifecycle, mut fut) = leaked_future(mutex, &QUEUE_EVENT);
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        assert_eq!(copy_calls.load(Ordering::Relaxed), 0);
        assert_eq!(count.load(Ordering::Relaxed), 0);
        assert!(mutex.try_lock().is_some());

        // Stack polling drains the RX slots and wakes the waiter once.
        control.rx_slot_full.store(false, Ordering::Relaxed);
        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
        mutex.lock().poll(RxOwnerView::AsyncOwned, &mut sockets);
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn round_reclaim_exhausted_still_runs_rx_and_submit() {
        // Task 3.2: exhausting one stage never skips a later stage. Here the
        // TX reclaim stage is busy for its whole budget while RX copy and TX
        // submit each make progress; the round still visits both later
        // stages.
        let reclaim_steps: Vec<_> = (0..RECLAIM_BUDGET)
            .map(|_| TxReclaimStep::Reclaimed)
            .collect();
        let rx_steps: Vec<_> = (0..RX_BUDGET - 1).map(|_| RxStep::Consumed).collect();
        let submit_steps: Vec<_> = (0..SUBMIT_BUDGET - 1)
            .map(|_| TxSubmitStep::Submitted)
            .collect();
        let (mutex, copy_calls, control) =
            leaked_service_tx(rx_steps, submit_steps, reclaim_steps, true);
        control.completion_visible.store(true, Ordering::Relaxed);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        // Even though the reclaim stage consumed its full budget, RX copy and
        // TX submit both advanced (independent budgets). The RX stage ran 31
        // copies plus the Empty probe that ends the stage (32 calls).
        assert_eq!(copy_calls.load(Ordering::Relaxed), RX_BUDGET);
        // Visible TX completion keeps the round self-waking once.
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert!(mutex.try_lock().is_some());
    }

    #[test]
    fn round_tx_again_retains_slot_and_self_wakes() {
        // Task 3.2: a `Full` (Again) TX submit retains the slot frame and
        // stops only the submit stage; the visible TX backlog self-wakes the
        // round once instead of sleeping.
        let (mutex, copy_calls, control) =
            leaked_service_tx(vec![RxStep::Empty], vec![TxSubmitStep::Full], vec![], true);
        control.tx_slot_pending.store(true, Ordering::Relaxed);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        // RX copy ran (Empty probe) and the submit stage hit `Full`.
        assert_eq!(copy_calls.load(Ordering::Relaxed), 1);
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert!(mutex.try_lock().is_some());
    }

    // ---- T6.1b: monotonic telemetry deltas ----

    #[test]
    fn telemetry_empty_round_increments_empty_check_once() {
        let (mutex, ..) = leaked_service(vec![RxStep::Empty], true);
        let (_, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(fut.telemetry.task_poll.load(Ordering::Relaxed), 1);
        assert_eq!(fut.telemetry.reaped.load(Ordering::Relaxed), 0);
        assert_eq!(fut.telemetry.empty_check.load(Ordering::Relaxed), 1);
        assert_eq!(fut.telemetry.fault.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn telemetry_consumed_increments_reap_and_refill() {
        let (mutex, ..) = leaked_service(vec![RxStep::Consumed, RxStep::Empty], true);
        let (_, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(fut.telemetry.reaped.load(Ordering::Relaxed), 1);
        assert_eq!(fut.telemetry.refilled.load(Ordering::Relaxed), 1);
        // MS05 Task 3.2: the queue task only copies raw→slot; delivered and
        // non-IP counters are produced by the stack RX path, not the task.
        assert_eq!(fut.telemetry.delivered.load(Ordering::Relaxed), 0);
        assert_eq!(fut.telemetry.non_ip_consumed.load(Ordering::Relaxed), 0);
        assert_eq!(fut.telemetry.empty_check.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn telemetry_rx_copy_increments_reap_and_refill() {
        let (mutex, ..) = leaked_service(vec![RxStep::Delivered, RxStep::Empty], true);
        let (_, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(fut.telemetry.reaped.load(Ordering::Relaxed), 1);
        assert_eq!(fut.telemetry.refilled.load(Ordering::Relaxed), 1);
        // Delivered is a stack-path counter in MS05 (the task does not parse
        // the frame).
        assert_eq!(fut.telemetry.delivered.load(Ordering::Relaxed), 0);
        assert_eq!(fut.telemetry.non_ip_consumed.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn telemetry_budget_backlog_increments_exhausted_and_self_yield() {
        let steps: Vec<RxStep> = (0..=RX_BUDGET).map(|_| RxStep::Consumed).collect();
        let (mutex, _, control) = leaked_service(steps, true);
        control.completion_visible.store(true, Ordering::Relaxed);
        let (_, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        // The RX stage exhausts its budget once and the round-end backlog
        // decision records the second exhaustion (Task 3.2 stage budgets).
        assert_eq!(fut.telemetry.budget_exhausted.load(Ordering::Relaxed), 2);
        assert_eq!(fut.telemetry.self_yield.load(Ordering::Relaxed), 1);
        assert_eq!(
            fut.telemetry.reaped.load(Ordering::Relaxed),
            RX_BUDGET as u64
        );
    }

    #[test]
    fn telemetry_rx_slot_full_waits_then_service_poll_wakes() {
        // The wait/space handoff shares the production `QUEUE_EVENT` with
        // `Service::poll` and the space-wake counter is recorded on the
        // production `RX_TELEMETRY` global: serialize against sibling tests.
        let _serial = SERIAL.lock();
        let (mutex, .., control) = leaked_service(vec![RxStep::Consumed], true);
        // Fill the target's fixed RX slots so the copy stage stops on `Full`
        // instead of reaping.
        control.rx_slot_full.store(true, Ordering::Relaxed);
        let (_, mut fut) = leaked_future(mutex, &QUEUE_EVENT);
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(fut.telemetry.rx_slot_full.load(Ordering::Relaxed), 1);
        assert_eq!(fut.telemetry.router_full_wait.load(Ordering::Relaxed), 1);
        assert_eq!(fut.telemetry.space_wake.load(Ordering::Relaxed), 0);

        let space_wake_before = RX_TELEMETRY.space_wake.load(Ordering::Relaxed);
        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
        // Stack polling drains the RX slots; the released slot space wakes
        // the waiting queue task.
        control.rx_slot_full.store(false, Ordering::Relaxed);
        mutex.lock().poll(RxOwnerView::AsyncOwned, &mut sockets);
        assert_eq!(
            RX_TELEMETRY.space_wake.load(Ordering::Relaxed) - space_wake_before,
            1
        );
        assert_eq!(fut.telemetry.router_full_wait.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn telemetry_preflight_failure_records_last_error_without_fault() {
        let (mutex, _, control) = leaked_service(vec![], true);
        control.suppress_error.store(true, Ordering::Relaxed);
        let (_, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(
            poll_once(&mut fut, count.clone()),
            Poll::Ready(())
        ));
        assert_eq!(fut.telemetry.fault.load(Ordering::Relaxed), 0);
        assert_eq!(
            fut.telemetry.last_error(),
            (rx_error_stage::PREFLIGHT, rx_error_code(&DevError::Io))
        );
    }

    #[test]
    fn telemetry_active_arm_fault_records_fault_and_stage() {
        let (mutex, _, control) = leaked_service(vec![RxStep::Empty], true);
        control.arm_error.store(true, Ordering::Relaxed);
        let (_, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(
            poll_once(&mut fut, count.clone()),
            Poll::Ready(())
        ));
        assert_eq!(fut.telemetry.fault.load(Ordering::Relaxed), 1);
        assert_eq!(
            fut.telemetry.last_error(),
            (rx_error_stage::ARM, rx_error_code(&DevError::Io))
        );
    }

    #[test]
    fn telemetry_active_arm_fault_records_exactly_once() {
        let steps: Vec<RxStep> = (0..RX_BUDGET).map(|_| RxStep::Consumed).collect();
        let (mutex, _, control) = leaked_service(steps, true);
        control
            .missing_after_first_control_call
            .store(true, Ordering::Relaxed);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        lifecycle.preflight(true).unwrap();
        let count = Arc::new(AtomicUsize::new(0));

        // The control survives the first round-end pending query, then
        // disappears before the register/arm recheck: the BOTH-direction arm
        // faults with the ARM stage.
        assert!(matches!(poll_once(&mut fut, count), Poll::Ready(())));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        assert_eq!(fut.telemetry.fault.load(Ordering::Relaxed), 1);
        assert_eq!(
            fut.telemetry.last_error(),
            (rx_error_stage::ARM, rx_error_code(&DevError::Unsupported))
        );
    }

    #[test]
    fn telemetry_active_receive_fault_records_exactly_once() {
        let (mutex, ..) = leaked_service(vec![RxStep::Fault(DevError::Io)], true);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        lifecycle.preflight(true).unwrap();
        let count = Arc::new(AtomicUsize::new(0));

        assert!(matches!(poll_once(&mut fut, count), Poll::Ready(())));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        assert_eq!(fut.telemetry.fault.load(Ordering::Relaxed), 1);
        assert_eq!(
            fut.telemetry.last_error(),
            (
                rx_error_stage::RECEIVE_RECYCLE,
                rx_error_code(&DevError::Io),
            )
        );
    }

    #[test]
    fn telemetry_missing_service_records_preflight_bad_state() {
        let lifecycle: &'static RxLifecycle = Box::leak(Box::new(RxLifecycle::new()));
        lifecycle.start().unwrap();
        let telemetry: &'static RxTelemetry = Box::leak(Box::new(RxTelemetry::new()));
        let mut fut = RxRxFuture {
            service: ServiceAccess::Global,
            lifecycle,
            notify: Box::leak(Box::new(QueueEvent::new())),
            telemetry,
        };
        let count = Arc::new(AtomicUsize::new(0));

        assert!(matches!(poll_once(&mut fut, count), Poll::Ready(())));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Unavailable);
        assert_eq!(telemetry.fault.load(Ordering::Relaxed), 0);
        assert_eq!(
            telemetry.last_error(),
            (
                rx_error_stage::PREFLIGHT,
                rx_error_code(&DevError::BadState),
            )
        );
    }

    #[test]
    fn snapshot_source_uses_one_lifecycle_observation() {
        let source = include_str!("async_rx.rs");
        let start = source.find("fn rx_snapshot_impl").unwrap();
        let end = source[start..]
            .find("/// Read-only RX snapshot for the kernel ioctl")
            .map(|offset| start + offset)
            .unwrap();
        let body = &source[start..end];

        assert_eq!(
            body.matches("lifecycle.load()").count(),
            1,
            "lifecycle and owner must derive from one acquired state"
        );
    }

    #[test]
    fn last_error_pair_uses_one_atomic_publication() {
        let source = include_str!("async_rx.rs");
        let start = source.find("pub(crate) struct RxTelemetry").unwrap();
        let end = source[start..]
            .find("impl RxTelemetry")
            .map(|offset| start + offset)
            .unwrap();
        let fields = &source[start..end];

        assert!(fields.contains("last_error: AtomicU64"));
        assert!(!fields.contains("last_error_stage: AtomicU64"));
        assert!(!fields.contains("last_error_code: AtomicU64"));
    }

    #[test]
    fn last_error_pair_round_trips_as_one_value() {
        let telemetry = RxTelemetry::new();
        assert_eq!(telemetry.last_error(), (rx_error_stage::NONE, 0));

        telemetry.record_last_error_code(rx_error_stage::SUPPRESS, 7);
        assert_eq!(telemetry.last_error(), (rx_error_stage::SUPPRESS, 7));

        telemetry.record_last_error_code(rx_error_stage::ARM, u32::MAX as u64);
        assert_eq!(
            telemetry.last_error(),
            (rx_error_stage::ARM, u32::MAX as u64)
        );
    }

    #[test]
    fn telemetry_illegal_preflight_records_lifecycle_stage() {
        // Drive the lifecycle past Spawned so the Spawned-only preflight
        // transition must fail; the failure is recorded as LIFECYCLE-stage
        // with the observed state code, and never increments the fault counter.
        let (mutex, ..) = leaked_service(vec![], true);
        let (lifecycle, fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        lifecycle.preflight(true).unwrap();
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);

        fut.transition_preflight(true);
        assert_eq!(fut.telemetry.fault.load(Ordering::Relaxed), 0);
        assert_eq!(
            fut.telemetry.last_error(),
            (
                rx_error_stage::LIFECYCLE,
                RxTaskLifecycle::Active.code() as u64,
            )
        );
    }

    #[test]
    fn telemetry_illegal_fatal_records_lifecycle_stage() {
        // The fatal transition requires Active; from Spawned it must fail and
        // be recorded as LIFECYCLE-stage without changing the state.
        let (mutex, ..) = leaked_service(vec![], true);
        let (lifecycle, fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Spawned);

        fut.transition_fatal();
        assert_eq!(fut.telemetry.fault.load(Ordering::Relaxed), 0);
        assert_eq!(
            fut.telemetry.last_error(),
            (
                rx_error_stage::LIFECYCLE,
                RxTaskLifecycle::Spawned.code() as u64,
            )
        );
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Spawned);
    }

    #[test]
    fn telemetry_snapshot_mirrors_lifecycle_and_counters() {
        let (mutex, _, control) = leaked_service(vec![RxStep::Consumed, RxStep::Empty], true);
        control.completion_visible.store(true, Ordering::Relaxed);
        let (_lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        let snap = super::rx_snapshot_impl(fut.lifecycle, fut.telemetry);
        assert_eq!(snap.lifecycle, RxTaskLifecycle::Active.code() as u64);
        assert_eq!(snap.owner, 1);
        assert_eq!(snap.reaped, 1);
        // A visible completion yields (self-wake), not an empty recheck.
        assert_eq!(snap.empty_check, 0);
        assert_eq!(snap.self_yield, 1);
    }
}
