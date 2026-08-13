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
use embassy_sync::waitqueue::AtomicWaker;

use crate::{
    router::{RxOutcome, RxOwnerView},
    service::Service,
};

/// Single-waiter queue notification state shared by the future RX queue task
/// and [`crate::service::Service`].
///
/// The queue side registers its waker without taking the `SERVICE` lock, then
/// publishes the waiting bit (Release) inside the Service lock only after a
/// serialized recheck confirms the Router RX buffer is still full.
/// `Service::poll` clears the bit (AcqRel) and wakes the task exactly once,
/// after ingress has freed Router buffer space. No `Relaxed` ordering is used
/// because the waiting bit participates in control flow.
pub(crate) struct RxNotify {
    waker: AtomicWaker,
    waiting: AtomicBool,
    generation: AtomicU64,
}

impl RxNotify {
    pub(crate) const fn new() -> Self {
        Self {
            waker: AtomicWaker::new(),
            waiting: AtomicBool::new(false),
            generation: AtomicU64::new(0),
        }
    }

    #[cfg(test)]
    fn with_generation(generation: u64) -> Self {
        Self {
            waker: AtomicWaker::new(),
            waiting: AtomicBool::new(false),
            generation: AtomicU64::new(generation),
        }
    }

    /// Registers the sole queue-task waker. Callable without the Service lock.
    pub(crate) fn register(&self, waker: &Waker) {
        self.waker.register(waker);
    }

    /// Publishes the waiting bit. Only called inside the Service guard after a
    /// serialized full-space recheck.
    pub(crate) fn publish_waiting(&self) {
        self.waiting.store(true, Ordering::Release);
    }

    /// Clears the waiting bit (AcqRel) and wakes the registered task exactly
    /// once when Router space is available.
    pub(crate) fn wake_if_space(&self, has_space: bool) -> bool {
        if has_space && self.waiting.swap(false, Ordering::AcqRel) {
            self.waker.wake();
            true
        } else {
            false
        }
    }

    /// Publishes a queue event: wrapping Release increment of the generation,
    /// then wakes the sole waiter. Called by the future ISR path.
    pub(crate) fn publish_event(&self) {
        self.generation.fetch_add(1, Ordering::Release);
        self.waker.wake();
    }

    /// Acquire snapshot of the event generation.
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Empty-queue wait protocol: Acquire generation, register the waker, run
    /// the arm/recheck observation, then Acquire the generation again. A
    /// pending observation or a generation change yields `Retry`; only a
    /// quiescent arm with an unchanged generation yields `Sleep`. A failed
    /// arm is a queue-control fatal and yields `Fault` with the error.
    pub(crate) fn wait_decision(
        &self,
        waker: &Waker,
        arm: impl FnOnce() -> DevResult<ArmObservation>,
    ) -> WaitDecision {
        let before = self.generation();
        self.register(waker);
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

/// Scheduling decision after one RX one-step outcome.
#[derive(Debug)]
pub(crate) enum RxDecision {
    /// Reap the next completion in this round.
    Continue,
    /// Empty queue or clean budget stop: run the register/arm/recheck wait
    /// protocol.
    RegisterRecheck,
    /// Router full: hand off to the Service-guard space recheck.
    WaitSpace,
    /// Budget exhausted with backlog present: self-wake and yield once.
    SelfWakeYield,
    /// Terminal for the decision layer; carries the queue/device error.
    Fault(DevError),
}

/// Pure per-step budget/scheduling decision.
///
/// `processed` counts completions already serviced this round including the
/// current one; `backlog` reports hardware completions still pending, never
/// probed by a 33rd reap. Exactly `RX_BUDGET` completions with backlog
/// self-wakes and yields; without backlog it stops cleanly. `Full` waits for
/// Router space instead of reaping; `Fault` is terminal.
pub(crate) fn decide_after_step(processed: usize, backlog: bool, outcome: RxOutcome) -> RxDecision {
    match outcome {
        RxOutcome::Consumed | RxOutcome::Delivered => {
            if processed >= RX_BUDGET {
                if backlog {
                    RxDecision::SelfWakeYield
                } else {
                    RxDecision::RegisterRecheck
                }
            } else {
                RxDecision::Continue
            }
        }
        RxOutcome::Empty => RxDecision::RegisterRecheck,
        RxOutcome::Full => RxDecision::WaitSpace,
        RxOutcome::Fault(err) => RxDecision::Fault(err),
    }
}

/// The one queue notification state. There is exactly one task waiter; Router
/// space wakes and future queue events share this waker.
pub(crate) static RX_NOTIFY: RxNotify = RxNotify::new();

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

/// ISR-safe fixed RX event publisher.
///
/// The kernel handler calls this *after* device ACK and telemetry. It updates
/// the publish/wake-call counters, then performs the existing Release
/// generation increment and the sole `AtomicWaker` wake. It never touches the
/// Service, queue-control, descriptors or smoltcp.
pub fn publish_rx_event() {
    RX_TELEMETRY.isr_publish.fetch_add(1, Ordering::Relaxed);
    RX_TELEMETRY.isr_wake.fetch_add(1, Ordering::Relaxed);
    RX_NOTIFY.publish_event();
}

fn software_nudge_impl(notify: &RxNotify, telemetry: &RxTelemetry) {
    telemetry.software_nudge.fetch_add(1, Ordering::Relaxed);
    notify.waker.wake();
}

/// Wake the unique RX task without publishing a hardware event.
pub fn software_nudge() {
    software_nudge_impl(&RX_NOTIFY, &RX_TELEMETRY);
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
    notify: &'static RxNotify,
    telemetry: &'static RxTelemetry,
}

/// Outcome of one RX servicing round before releasing the guard.
enum RoundOutcome {
    /// A self-wake plus Pending is required (budget backlog).
    SelfWakeYield,
    /// Run the empty-queue register/arm/recheck protocol.
    RegisterRecheck,
    /// Wait for Router space (full), possibly retrying.
    WaitSpace(SpaceDecision),
    /// Terminal queue/device fault.
    Fault(DevError),
}

impl RxRxFuture {
    /// Polls the Service under the lock until a scheduling point, then
    /// returns the next action. The guard never crosses a Pending/Ready.
    fn service_round(&self, service: &mut Service) -> RoundOutcome {
        if let Err(err) = service.rx_suppress_target() {
            self.telemetry.record_fault(rx_error_stage::SUPPRESS, &err);
            return RoundOutcome::Fault(err);
        }
        let mut processed = 0usize;
        loop {
            processed += 1;
            let outcome = service.rx_one_step_target();
            match &outcome {
                RxOutcome::Consumed => {
                    self.telemetry.reaped.fetch_add(1, Ordering::Relaxed);
                    self.telemetry.refilled.fetch_add(1, Ordering::Relaxed);
                    self.telemetry
                        .non_ip_consumed
                        .fetch_add(1, Ordering::Relaxed);
                }
                RxOutcome::Delivered => {
                    self.telemetry.reaped.fetch_add(1, Ordering::Relaxed);
                    self.telemetry.refilled.fetch_add(1, Ordering::Relaxed);
                    self.telemetry.delivered.fetch_add(1, Ordering::Relaxed);
                }
                RxOutcome::Fault(err) => {
                    self.telemetry
                        .record_fault(rx_error_stage::RECEIVE_RECYCLE, err);
                }
                _ => {}
            }
            let backlog = match outcome {
                RxOutcome::Consumed | RxOutcome::Delivered if processed >= RX_BUDGET => {
                    match service.rx_completion_visible_target() {
                        Ok(backlog) => backlog,
                        Err(err) => {
                            self.telemetry
                                .record_fault(rx_error_stage::COMPLETION_QUERY, &err);
                            return RoundOutcome::Fault(err);
                        }
                    }
                }
                _ => false,
            };
            match decide_after_step(processed, backlog, outcome) {
                RxDecision::Continue => continue,
                RxDecision::SelfWakeYield => {
                    self.telemetry
                        .budget_exhausted
                        .fetch_add(1, Ordering::Relaxed);
                    self.telemetry.self_yield.fetch_add(1, Ordering::Relaxed);
                    return RoundOutcome::SelfWakeYield;
                }
                RxDecision::RegisterRecheck => {
                    self.telemetry.empty_check.fetch_add(1, Ordering::Relaxed);
                    return RoundOutcome::RegisterRecheck;
                }
                RxDecision::WaitSpace => {
                    let decision = service.rx_space_recheck_or_wait();
                    if decision == SpaceDecision::Waiting {
                        self.telemetry
                            .router_full_wait
                            .fetch_add(1, Ordering::Relaxed);
                    }
                    return RoundOutcome::WaitSpace(decision);
                }
                // The Fault outcome was already recorded with the
                // RECEIVE_RECYCLE stage above; just hand the error up.
                RxDecision::Fault(err) => return RoundOutcome::Fault(err),
            }
        }
    }

    /// First poll: acquire the Service, preflight/suppress, publish
    /// Active (or Unavailable) under the guard, then hand off to the active
    /// servicing loop.
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
            self.notify.register(cx.waker());
            return Poll::Pending;
        };
        let preflight = service.rx_preflight_target();
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
        self.notify.register(cx.waker());
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

    /// Empty-queue wait: acquire generation, register, arm/recheck under the
    /// Service lock, then observe the generation again.
    fn poll_register_recheck(&self, cx: &mut Context<'_>) -> Poll<()> {
        let decision = self.notify.wait_decision(cx.waker(), || {
            let Some(mut service) = self.service.try_lock() else {
                return Err(DevError::BadState);
            };
            service.rx_arm_and_check_target().map(|pending| {
                if pending {
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
                notify: &RX_NOTIFY,
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

/// Serializes tests that touch the shared [`RX_NOTIFY`] static.
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
    use axdriver_net::NetQueueControl;
    use smoltcp::{storage::PacketBuffer, time::Instant, wire::IpAddress};

    use super::{
        ArmObservation, RX_BUDGET, RX_LIFECYCLE, RX_NOTIFY, RX_TELEMETRY, RxDecision, RxLifecycle,
        RxNotify, RxRxFuture, RxTaskLifecycle, RxTelemetry, SERIAL, ServiceAccess, SpaceDecision,
        StartError, TransitionError, WaitDecision, decide_after_step, rx_error_code,
        rx_error_stage, software_nudge_impl, start_with,
    };
    use crate::{
        device::{Device, LoopbackDevice, RxStep, TxOutcome, TxPreflight},
        router::{Router, RxOutcome, RxOwnerView},
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
            TxOutcome::Accepted { rx_became_ready: false }
        }

        fn register_waker(&self, _waker: &Waker) {}
    }

    #[test]
    fn notify_full_waiting_then_space_wakes_once() {
        let notify = RxNotify::new();
        let count = Arc::new(AtomicUsize::new(0));
        notify.register(&counting_waker(count.clone()));
        notify.publish_waiting();
        assert!(notify.wake_if_space(true));
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert!(!notify.wake_if_space(true));
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn notify_still_full_does_not_wake() {
        let notify = RxNotify::new();
        let count = Arc::new(AtomicUsize::new(0));
        notify.register(&counting_waker(count.clone()));
        notify.publish_waiting();
        assert!(!notify.wake_if_space(false));
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn notify_not_waiting_does_not_wake() {
        let notify = RxNotify::new();
        let count = Arc::new(AtomicUsize::new(0));
        notify.register(&counting_waker(count.clone()));
        assert!(!notify.wake_if_space(true));
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn notify_second_publish_after_clear_wakes_again() {
        let notify = RxNotify::new();
        let count = Arc::new(AtomicUsize::new(0));
        notify.register(&counting_waker(count.clone()));
        notify.publish_waiting();
        assert!(notify.wake_if_space(true));
        notify.publish_waiting();
        assert!(notify.wake_if_space(true));
        assert!(!notify.wake_if_space(true));
        assert_eq!(count.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn sibling_caller_reaches_target_one_step_and_register_seam() {
        // Touches the production `RX_NOTIFY` static: share the serial guard so
        // a sibling registration cannot overwrite another test's waker.
        let _serial = SERIAL.lock();
        let mut router = Router::new();
        router.add_device(Box::new(LoopbackDevice::new()));
        let mut service = Service::new(router, Some(0));

        let outcome = service.rx_one_step_target();
        assert!(matches!(outcome, RxOutcome::Empty));

        let count = Arc::new(AtomicUsize::new(0));
        RX_NOTIFY.register(&counting_waker(count.clone()));
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn one_step_missing_target_maps_to_bad_state() {
        let router = Router::new();
        let mut service = Service::new(router, None);

        let outcome = service.rx_one_step_target();
        assert!(matches!(outcome, RxOutcome::Fault(DevError::BadState)));
    }

    #[test]
    fn one_step_full_router_buffer_does_not_touch_device() {
        let recv_calls = Arc::new(AtomicUsize::new(0));
        let mut router = Router::new();
        router.add_device(Box::new(CountingDevice::new(recv_calls.clone())));
        router.fill_rx_buffer_for_test();
        let mut service = Service::new(router, Some(0));

        let outcome = service.rx_one_step_target();
        assert!(matches!(outcome, RxOutcome::Full));
        assert_eq!(recv_calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn space_freed_before_waiting_rechecks_to_retry_without_publish() {
        let _serial = SERIAL.lock();
        let count = Arc::new(AtomicUsize::new(0));
        RX_NOTIFY.register(&counting_waker(count.clone()));

        let router = Router::new();
        let service = Service::new(router, None);

        let decision = service.rx_space_recheck_or_wait();
        assert!(matches!(decision, SpaceDecision::Retry));
        // Retry must not have published waiting: a later space wake is a no-op.
        assert!(!RX_NOTIFY.wake_if_space(true));
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn still_full_publishes_waiting_then_space_wakes_once() {
        let _serial = SERIAL.lock();
        let count = Arc::new(AtomicUsize::new(0));
        RX_NOTIFY.register(&counting_waker(count.clone()));

        let mut router = Router::new();
        router.add_device(Box::new(LoopbackDevice::new()));
        router.fill_rx_buffer_for_test();
        let service = Service::new(router, Some(0));

        let decision = service.rx_space_recheck_or_wait();
        assert!(matches!(decision, SpaceDecision::Waiting));

        // Space freed after waiting: exactly one wake.
        assert!(RX_NOTIFY.wake_if_space(true));
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert!(!RX_NOTIFY.wake_if_space(true));
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
        let notify = RxNotify::new();
        let count = Arc::new(AtomicUsize::new(0));
        notify.register(&counting_waker(count.clone()));

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
        let notify = RxNotify::new();
        let telemetry = RxTelemetry::new();
        let count = Arc::new(AtomicUsize::new(0));
        notify.register(&counting_waker(count.clone()));
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
        let notify = RxNotify::with_generation(u64::MAX);
        notify.publish_event();
        assert_eq!(notify.generation(), 0);
    }

    #[test]
    fn event_before_register_is_caught_by_arm_recheck() {
        let notify = RxNotify::new();
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
        let notify = RxNotify::new();
        let count = Arc::new(AtomicUsize::new(0));

        let decision = notify.wait_decision(&counting_waker(count.clone()), || {
            notify.publish_event();
            Ok(ArmObservation::Quiescent)
        });
        assert!(matches!(decision, WaitDecision::Retry));
    }

    #[test]
    fn event_after_arm_wakes_sleep_decision() {
        let notify = RxNotify::new();
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
        let notify = RxNotify::new();
        let count = Arc::new(AtomicUsize::new(0));

        let decision = notify.wait_decision(&counting_waker(count.clone()), || {
            Ok(ArmObservation::Pending)
        });
        assert!(matches!(decision, WaitDecision::Retry));
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn quiescent_arm_without_event_sleeps() {
        let notify = RxNotify::new();
        let count = Arc::new(AtomicUsize::new(0));

        let decision = notify.wait_decision(&counting_waker(count.clone()), || {
            Ok(ArmObservation::Quiescent)
        });
        assert!(matches!(decision, WaitDecision::Sleep));
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn arm_error_maps_to_fault_with_error_category() {
        let notify = RxNotify::new();
        let count = Arc::new(AtomicUsize::new(0));

        let decision = notify.wait_decision(&counting_waker(count.clone()), || Err(DevError::Io));
        assert!(matches!(decision, WaitDecision::Fault(DevError::Io)));
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn budget_below_limit_continues_on_progress() {
        for (processed, outcome) in [
            (1, RxOutcome::Consumed),
            (1, RxOutcome::Delivered),
            (RX_BUDGET - 1, RxOutcome::Consumed),
            (RX_BUDGET - 1, RxOutcome::Delivered),
        ] {
            assert!(matches!(
                decide_after_step(processed, true, outcome),
                RxDecision::Continue
            ));
        }
    }

    #[test]
    fn budget_exact_with_backlog_self_wakes_and_yields() {
        // The literal 32 keeps the RX_BUDGET mutation witness sensitive.
        for outcome in [RxOutcome::Consumed, RxOutcome::Delivered] {
            assert!(matches!(
                decide_after_step(32, true, outcome),
                RxDecision::SelfWakeYield
            ));
        }
    }

    #[test]
    fn budget_exact_without_backlog_goes_register_recheck() {
        // The literal 32 keeps the RX_BUDGET mutation witness sensitive.
        for outcome in [RxOutcome::Consumed, RxOutcome::Delivered] {
            assert!(matches!(
                decide_after_step(32, false, outcome),
                RxDecision::RegisterRecheck
            ));
        }
    }

    #[test]
    fn empty_goes_register_recheck_at_any_count() {
        assert!(matches!(
            decide_after_step(1, false, RxOutcome::Empty),
            RxDecision::RegisterRecheck
        ));
        assert!(matches!(
            decide_after_step(RX_BUDGET, true, RxOutcome::Empty),
            RxDecision::RegisterRecheck
        ));
    }

    #[test]
    fn full_goes_wait_space_without_reaping() {
        assert!(matches!(
            decide_after_step(1, false, RxOutcome::Full),
            RxDecision::WaitSpace
        ));
        assert!(matches!(
            decide_after_step(RX_BUDGET, true, RxOutcome::Full),
            RxDecision::WaitSpace
        ));
    }

    #[test]
    fn fault_is_terminal_for_the_decision_layer() {
        assert!(matches!(
            decide_after_step(1, false, RxOutcome::Fault(DevError::BadState)),
            RxDecision::Fault(DevError::BadState)
        ));
        assert!(matches!(
            decide_after_step(RX_BUDGET, true, RxOutcome::Fault(DevError::Io)),
            RxDecision::Fault(DevError::Io)
        ));
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
    }

    /// A fake NIC whose `recv` replays scripted outcomes and whose optional
    /// queue control records calls and honors injected errors.
    struct ScriptedDevice {
        steps: spin::Mutex<VecDeque<RxStep>>,
        recv_calls: Arc<AtomicUsize>,
        control: Option<ScriptedControl>,
    }

    impl Device for ScriptedDevice {
        fn name(&self) -> &str {
            "scripted"
        }

        fn recv(&mut self, _buffer: &mut PacketBuffer<()>, _timestamp: Instant) -> RxStep {
            self.recv_calls.fetch_add(1, Ordering::Relaxed);
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
            TxOutcome::Accepted { rx_became_ready: false }
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
        let recv_calls = Arc::new(AtomicUsize::new(0));
        let stats = Arc::new(ScriptedControlStats::default());
        let control = with_control.then(|| ScriptedControl {
            stats: stats.clone(),
        });
        let device = ScriptedDevice {
            steps: spin::Mutex::new(steps.into()),
            recv_calls: recv_calls.clone(),
            control,
        };
        let mut router = Router::new();
        let idx = router.add_device(Box::new(device));
        let service = Service::new(router, Some(idx));
        let mutex: &'static spin::Mutex<Service> = Box::leak(Box::new(spin::Mutex::new(service)));
        (mutex, recv_calls, stats)
    }

    /// Builds an injected Future: local leaked lifecycle/notify/telemetry,
    /// spin service mutex, lifecycle already driven to `Spawned`.
    fn leaked_future(
        service_mutex: &'static spin::Mutex<Service>,
        notify: &'static RxNotify,
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
        let notify: &'static RxNotify = Box::leak(Box::new(RxNotify::new()));
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
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(RxNotify::new())));
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
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(RxNotify::new())));
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
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(RxNotify::new())));
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
        let (mutex, recv_calls, control) = leaked_service(vec![RxStep::Empty], true);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(RxNotify::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        assert_eq!(recv_calls.load(Ordering::Relaxed), 1);
        // Quiescent arm without event: sleep without self-wake.
        assert_eq!(count.load(Ordering::Relaxed), 0);
        assert_eq!(control.arm_calls.load(Ordering::Relaxed), 1);
        assert!(mutex.try_lock().is_some());
    }

    #[test]
    fn future_services_one_completion_then_registers() {
        let (mutex, recv_calls, _) = leaked_service(vec![RxStep::Consumed, RxStep::Empty], true);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(RxNotify::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        assert_eq!(recv_calls.load(Ordering::Relaxed), 2);
        assert!(mutex.try_lock().is_some());
    }

    #[test]
    fn future_31_completions_then_empty_registers_once() {
        // Exactly 31 progress steps then an Empty on the 32nd observation:
        // the future performs exactly RX_BUDGET (32) receive calls, arms once,
        // self-wakes zero times and releases the Service guard. The literal 31
        // keeps the RX_BUDGET boundary mutation witness sensitive.
        let steps: Vec<RxStep> = (0..31)
            .map(|_| RxStep::Consumed)
            .chain([RxStep::Empty])
            .collect();
        let (mutex, recv_calls, control) = leaked_service(steps, true);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(RxNotify::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        assert_eq!(recv_calls.load(Ordering::Relaxed), 32);
        assert_eq!(count.load(Ordering::Relaxed), 0);
        assert_eq!(control.arm_calls.load(Ordering::Relaxed), 1);
        assert!(mutex.try_lock().is_some());
    }

    #[test]
    fn future_budget_exhausted_with_backlog_self_wakes_and_yields() {
        let steps: Vec<RxStep> = (0..=RX_BUDGET).map(|_| RxStep::Consumed).collect();
        let (mutex, recv_calls, control) = leaked_service(steps, true);
        control.completion_visible.store(true, Ordering::Relaxed);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(RxNotify::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        // Exactly RX_BUDGET reaps; the 33rd is never probed by another reap.
        assert_eq!(recv_calls.load(Ordering::Relaxed), RX_BUDGET);
        // Self-wake for block_on yield, no extra spurious wake.
        assert_eq!(count.load(Ordering::Relaxed), 1);
        // SelfWakeYield keeps the queue suppressed: no rearm happened.
        assert_eq!(control.arm_calls.load(Ordering::Relaxed), 0);
        assert!(mutex.try_lock().is_some());
    }

    #[test]
    fn future_budget_exhausted_without_backlog_stops_cleanly() {
        let steps: Vec<RxStep> = (0..RX_BUDGET).map(|_| RxStep::Consumed).collect();
        let (mutex, recv_calls, _) = leaked_service(steps, true);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(RxNotify::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        assert_eq!(recv_calls.load(Ordering::Relaxed), RX_BUDGET);
        assert_eq!(count.load(Ordering::Relaxed), 0);
        assert!(mutex.try_lock().is_some());
    }

    #[test]
    fn future_arm_pending_retries_with_self_wake() {
        let (mutex, _, control) = leaked_service(vec![RxStep::Empty], true);
        control.completion_visible.store(true, Ordering::Relaxed);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(RxNotify::new())));
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
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(RxNotify::new())));
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
    fn future_router_full_waits_then_service_poll_wakes() {
        // The wait/space handoff shares the production `RX_NOTIFY` with
        // `Service::poll`: serialize against sibling tests.
        let _serial = SERIAL.lock();
        let (mutex, recv_calls, _) = leaked_service(vec![RxStep::Consumed], true);
        {
            let mut guard = mutex.lock();
            guard.fill_rx_buffer_for_test();
        }
        let (lifecycle, mut fut) = leaked_future(mutex, &RX_NOTIFY);
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        assert_eq!(recv_calls.load(Ordering::Relaxed), 0);
        assert_eq!(count.load(Ordering::Relaxed), 0);
        assert!(mutex.try_lock().is_some());

        // Ordinary Service::poll frees Router space and wakes the waiter once.
        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
        mutex.lock().poll(RxOwnerView::PollingOwned, &mut sockets);
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    // ---- T6.1b: monotonic telemetry deltas ----

    #[test]
    fn telemetry_empty_round_increments_empty_check_once() {
        let (mutex, ..) = leaked_service(vec![RxStep::Empty], true);
        let (_, mut fut) = leaked_future(mutex, Box::leak(Box::new(RxNotify::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(fut.telemetry.task_poll.load(Ordering::Relaxed), 1);
        assert_eq!(fut.telemetry.reaped.load(Ordering::Relaxed), 0);
        assert_eq!(fut.telemetry.empty_check.load(Ordering::Relaxed), 1);
        assert_eq!(fut.telemetry.fault.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn telemetry_consumed_increments_reap_refill_and_non_ip() {
        let (mutex, ..) = leaked_service(vec![RxStep::Consumed, RxStep::Empty], true);
        let (_, mut fut) = leaked_future(mutex, Box::leak(Box::new(RxNotify::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(fut.telemetry.reaped.load(Ordering::Relaxed), 1);
        assert_eq!(fut.telemetry.refilled.load(Ordering::Relaxed), 1);
        assert_eq!(fut.telemetry.delivered.load(Ordering::Relaxed), 0);
        assert_eq!(fut.telemetry.non_ip_consumed.load(Ordering::Relaxed), 1);
        assert_eq!(fut.telemetry.empty_check.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn telemetry_delivered_increments_delivered_counter() {
        let (mutex, ..) = leaked_service(vec![RxStep::Delivered, RxStep::Empty], true);
        let (_, mut fut) = leaked_future(mutex, Box::leak(Box::new(RxNotify::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(fut.telemetry.reaped.load(Ordering::Relaxed), 1);
        assert_eq!(fut.telemetry.refilled.load(Ordering::Relaxed), 1);
        assert_eq!(fut.telemetry.delivered.load(Ordering::Relaxed), 1);
        assert_eq!(fut.telemetry.non_ip_consumed.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn telemetry_budget_backlog_increments_exhausted_and_self_yield() {
        let steps: Vec<RxStep> = (0..=RX_BUDGET).map(|_| RxStep::Consumed).collect();
        let (mutex, _, control) = leaked_service(steps, true);
        control.completion_visible.store(true, Ordering::Relaxed);
        let (_, mut fut) = leaked_future(mutex, Box::leak(Box::new(RxNotify::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(fut.telemetry.budget_exhausted.load(Ordering::Relaxed), 1);
        assert_eq!(fut.telemetry.self_yield.load(Ordering::Relaxed), 1);
        assert_eq!(
            fut.telemetry.reaped.load(Ordering::Relaxed),
            RX_BUDGET as u64
        );
    }

    #[test]
    fn telemetry_router_full_increments_wait_and_space_wake() {
        // The wait/space handoff shares the production `RX_NOTIFY` with
        // `Service::poll` and the space-wake counter is recorded on the
        // production `RX_TELEMETRY` global: serialize against sibling tests.
        let _serial = SERIAL.lock();
        let (mutex, ..) = leaked_service(vec![RxStep::Consumed], true);
        {
            let mut guard = mutex.lock();
            guard.fill_rx_buffer_for_test();
        }
        let (_, mut fut) = leaked_future(mutex, &RX_NOTIFY);
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(fut.telemetry.router_full_wait.load(Ordering::Relaxed), 1);
        assert_eq!(fut.telemetry.space_wake.load(Ordering::Relaxed), 0);

        let space_wake_before = RX_TELEMETRY.space_wake.load(Ordering::Relaxed);
        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
        mutex.lock().poll(RxOwnerView::PollingOwned, &mut sockets);
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
        let (_, mut fut) = leaked_future(mutex, Box::leak(Box::new(RxNotify::new())));
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
        let (_, mut fut) = leaked_future(mutex, Box::leak(Box::new(RxNotify::new())));
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
    fn telemetry_active_suppress_fault_records_exactly_once() {
        let (mutex, _, control) = leaked_service(vec![], true);
        control.suppress_error.store(true, Ordering::Relaxed);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(RxNotify::new())));
        lifecycle.preflight(true).unwrap();
        let count = Arc::new(AtomicUsize::new(0));

        assert!(matches!(poll_once(&mut fut, count), Poll::Ready(())));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        assert_eq!(fut.telemetry.fault.load(Ordering::Relaxed), 1);
        assert_eq!(
            fut.telemetry.last_error(),
            (rx_error_stage::SUPPRESS, rx_error_code(&DevError::Io))
        );
    }

    #[test]
    fn telemetry_active_completion_query_fault_records_exactly_once() {
        let steps: Vec<RxStep> = (0..RX_BUDGET).map(|_| RxStep::Consumed).collect();
        let (mutex, _, control) = leaked_service(steps, true);
        control
            .missing_after_first_control_call
            .store(true, Ordering::Relaxed);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(RxNotify::new())));
        lifecycle.preflight(true).unwrap();
        let count = Arc::new(AtomicUsize::new(0));

        assert!(matches!(poll_once(&mut fut, count), Poll::Ready(())));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        assert_eq!(fut.telemetry.fault.load(Ordering::Relaxed), 1);
        assert_eq!(
            fut.telemetry.last_error(),
            (
                rx_error_stage::COMPLETION_QUERY,
                rx_error_code(&DevError::Unsupported),
            )
        );
    }

    #[test]
    fn telemetry_active_receive_fault_records_exactly_once() {
        let (mutex, ..) = leaked_service(vec![RxStep::Fault(DevError::Io)], true);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(RxNotify::new())));
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
            notify: Box::leak(Box::new(RxNotify::new())),
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
        let (lifecycle, fut) = leaked_future(mutex, Box::leak(Box::new(RxNotify::new())));
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
        let (lifecycle, fut) = leaked_future(mutex, Box::leak(Box::new(RxNotify::new())));
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
        let (_lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(RxNotify::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        let snap = super::rx_snapshot_impl(fut.lifecycle, fut.telemetry);
        assert_eq!(snap.lifecycle, RxTaskLifecycle::Active.code() as u64);
        assert_eq!(snap.owner, 1);
        assert_eq!(snap.reaped, 1);
        assert_eq!(snap.empty_check, 1);
    }
}
