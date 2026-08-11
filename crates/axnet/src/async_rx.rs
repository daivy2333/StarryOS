//! Async RX queue-task decision layer.
//!
//! This module hosts the crate-private seam between the future RX queue task
//! and [`crate::service::Service`]: a single-waiter queue notification state,
//! pure lifecycle/event/budget decisions, and the unique named queue task
//! wiring. No ISR publish or kernel caller lives here yet (T6.1).

use alloc::borrow::ToOwned;
#[cfg(test)]
use core::sync::atomic::AtomicUsize;
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
            return RoundOutcome::Fault(err);
        }
        let mut processed = 0usize;
        loop {
            processed += 1;
            let outcome = service.rx_one_step_target();
            let backlog = match outcome {
                RxOutcome::Consumed | RxOutcome::Delivered if processed >= RX_BUDGET => {
                    match service.rx_completion_visible_target() {
                        Ok(backlog) => backlog,
                        Err(err) => return RoundOutcome::Fault(err),
                    }
                }
                _ => false,
            };
            match decide_after_step(processed, backlog, outcome) {
                RxDecision::Continue => continue,
                RxDecision::SelfWakeYield => return RoundOutcome::SelfWakeYield,
                RxDecision::RegisterRecheck => return RoundOutcome::RegisterRecheck,
                RxDecision::WaitSpace => {
                    return RoundOutcome::WaitSpace(service.rx_space_recheck_or_wait());
                }
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
            let _ = self.lifecycle.preflight(false);
            return Poll::Ready(());
        }
        let Some(mut service) = self.service.try_lock() else {
            self.notify.register(cx.waker());
            return Poll::Pending;
        };
        let preflight_ok = service.rx_preflight_target().is_ok();
        let _ = self.lifecycle.preflight(preflight_ok);
        drop(service);
        if preflight_ok {
            self.poll_active(cx)
        } else {
            Poll::Ready(())
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
            RoundOutcome::Fault(_) => {
                let _ = self.lifecycle.fatal();
                drop(service);
                Poll::Ready(())
            }
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
            WaitDecision::Fault(_) => {
                let _ = self.lifecycle.fatal();
                Poll::Ready(())
            }
        }
    }
}

impl Future for RxRxFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        match self.lifecycle.load() {
            RxTaskLifecycle::Spawned => self.poll_first(cx),
            RxTaskLifecycle::Active => self.poll_active(cx),
            // Terminal/unavailable states: the task exits; polling keeps the
            // owner for Spawned/Unavailable.
            _ => Poll::Ready(()),
        }
    }
}

/// Spawn seam. Host tests witness the single-spawn decision with a counter
/// instead of running the axtask scheduler.
#[cfg(test)]
static SPAWN_COUNT: AtomicUsize = AtomicUsize::new(0);

#[cfg(not(test))]
fn spawn_rx_task() {
    axtask::spawn_with_name(
        || {
            axtask::future::block_on(RxRxFuture {
                service: ServiceAccess::Global,
                lifecycle: &RX_LIFECYCLE,
                notify: &RX_NOTIFY,
            })
        },
        RX_TASK_NAME.to_owned(),
    );
}

#[cfg(test)]
fn spawn_rx_task() {
    SPAWN_COUNT.fetch_add(1, Ordering::Relaxed);
}

/// Activates the async RX path. The CAS winner alone requests one fixed-name
/// spawn; a repeated call returns `AlreadyStarted` without a second task.
///
/// Dormant in this iteration: no kernel caller exists until T6.1 wires the
/// ISR publish/wake, so the polling owner remains active at runtime.
pub fn start_rx_task() -> Result<(), StartError> {
    RX_LIFECYCLE.start()?;
    spawn_rx_task();
    Ok(())
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
        ArmObservation, RX_BUDGET, RX_NOTIFY, RxDecision, RxLifecycle, RxNotify, RxRxFuture,
        RxTaskLifecycle, SERIAL, SPAWN_COUNT, ServiceAccess, SpaceDecision, StartError,
        TransitionError, WaitDecision, decide_after_step, start_rx_task,
    };
    use crate::{
        device::{Device, LoopbackDevice, RxStep},
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

        fn send(&mut self, _next_hop: IpAddress, _packet: &[u8], _timestamp: Instant) -> bool {
            false
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
        completion_visible: AtomicBool,
        suppress_error: AtomicBool,
        arm_error: AtomicBool,
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

        fn send(&mut self, _next_hop: IpAddress, _packet: &[u8], _timestamp: Instant) -> bool {
            false
        }

        fn queue_control(&mut self) -> Option<&mut dyn NetQueueControl> {
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

    /// Builds an injected Future: local leaked lifecycle/notify, spin service
    /// mutex, lifecycle already driven to `Spawned`.
    fn leaked_future(
        service_mutex: &'static spin::Mutex<Service>,
        notify: &'static RxNotify,
    ) -> (&'static RxLifecycle, RxRxFuture) {
        let lifecycle: &'static RxLifecycle = Box::leak(Box::new(RxLifecycle::new()));
        lifecycle.start().unwrap();
        let fut = RxRxFuture {
            service: ServiceAccess::Injected(service_mutex),
            lifecycle,
            notify,
        };
        (lifecycle, fut)
    }

    fn poll_once(fut: &mut RxRxFuture, count: Arc<AtomicUsize>) -> Poll<()> {
        let waker = counting_waker(count.clone());
        let mut cx = Context::from_waker(&waker);
        Pin::new(fut).poll(&mut cx)
    }

    #[test]
    fn start_rx_task_spawns_once_and_rejects_duplicate() {
        // Touches the global lifecycle/spawn counter: serialize.
        let _serial = SERIAL.lock();
        SPAWN_COUNT.store(0, Ordering::Relaxed);
        assert!(start_rx_task().is_ok());
        assert_eq!(SPAWN_COUNT.load(Ordering::Relaxed), 1);
        assert_eq!(
            start_rx_task(),
            Err(StartError::AlreadyStarted(RxTaskLifecycle::Spawned))
        );
        assert_eq!(SPAWN_COUNT.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn future_missing_service_publishes_unavailable() {
        // `ServiceAccess::Global` resolves the never-initialized `SERVICE`
        // once in host tests: the first poll must not panic and must exit
        // with Unavailable, keeping the polling owner.
        let lifecycle: &'static RxLifecycle = Box::leak(Box::new(RxLifecycle::new()));
        lifecycle.start().unwrap();
        let notify: &'static RxNotify = Box::leak(Box::new(RxNotify::new()));
        let mut fut = RxRxFuture {
            service: ServiceAccess::Global,
            lifecycle,
            notify,
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
}
