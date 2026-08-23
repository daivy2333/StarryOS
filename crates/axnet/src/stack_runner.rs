//! Resident smoltcp stack-runner notification and lifecycle.

#[cfg(not(test))]
use alloc::{borrow::ToOwned, boxed::Box};
use core::{
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    task::{Context, Poll, Waker},
};

#[cfg(not(test))]
use axhal::time::TimeValue;
#[cfg(not(test))]
use axtask::future::sleep_until;
use embassy_sync::waitqueue::AtomicWaker;
use smoltcp::time::{Duration, Instant};

#[cfg(not(test))]
use crate::async_rx::RX_LIFECYCLE;
use crate::{
    async_rx::{RxLifecycle, RxTaskLifecycle},
    router::RxOwnerView,
    service::StackRoundOutcome,
};

const POLLING_FALLBACK: Duration = Duration::from_millis(10);

/// Fixed name of the sole resident smoltcp stack runner.
pub(crate) const STACK_RUNNER_TASK_NAME: &str = "axnet-stack-runner";

/// Notification state owned exclusively by the resident stack runner.
///
/// Device progress and software mutations share this generation, but neither
/// publication changes the queue-owner generation in `async_rx`.
pub(crate) struct StackEvent {
    waker: AtomicWaker,
    generation: AtomicU64,
}

impl StackEvent {
    pub(crate) const fn new() -> Self {
        Self {
            waker: AtomicWaker::new(),
            generation: AtomicU64::new(0),
        }
    }

    #[cfg(test)]
    const fn with_generation(generation: u64) -> Self {
        Self {
            waker: AtomicWaker::new(),
            generation: AtomicU64::new(generation),
        }
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    pub(crate) fn register(&self, waker: &Waker) {
        self.waker.register(waker);
    }

    pub(crate) fn changed_since(&self, observed: u64) -> bool {
        self.generation() != observed
    }

    fn publish(&self) {
        self.generation.fetch_add(1, Ordering::Release);
        self.waker.wake();
    }

    pub(crate) fn publish_device(&self) {
        self.publish();
    }

    pub(crate) fn publish_software(&self) {
        self.publish();
    }
}

pub(crate) static STACK_EVENT: StackEvent = StackEvent::new();

/// One-way lifecycle for the unique stack runner.
pub(crate) struct StackRunnerLifecycle {
    started: AtomicBool,
}

impl StackRunnerLifecycle {
    pub(crate) const fn new() -> Self {
        Self {
            started: AtomicBool::new(false),
        }
    }

    pub(crate) fn is_started(&self) -> bool {
        self.started.load(Ordering::Acquire)
    }

    fn start(&self) -> Result<(), StartError> {
        self.started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| StartError::AlreadyStarted)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StartError {
    AlreadyStarted,
}

pub(crate) static STACK_RUNNER_LIFECYCLE: StackRunnerLifecycle = StackRunnerLifecycle::new();

fn start_with(lifecycle: &StackRunnerLifecycle, spawn: impl FnOnce()) -> Result<(), StartError> {
    lifecycle.start()?;
    spawn();
    Ok(())
}

#[derive(Clone, Copy)]
pub(crate) enum StackAccess {
    Global,
    #[cfg(test)]
    Injected {
        service: &'static spin::Mutex<crate::service::Service>,
        sockets: &'static spin::Mutex<smoltcp::iface::SocketSet<'static>>,
    },
}

impl StackAccess {
    fn round(&self, owner: RxOwnerView) -> Option<StackRoundOutcome> {
        match self {
            Self::Global => {
                let mut service = crate::SERVICE.get()?.lock();
                let mut sockets = crate::SOCKET_SET.inner.lock();
                Some(service.stack_round(owner, &mut sockets))
            }
            #[cfg(test)]
            Self::Injected { service, sockets } => {
                let mut service = service.lock();
                let mut sockets = sockets.lock();
                Some(service.stack_round(owner, &mut sockets))
            }
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum StackClock {
    #[cfg_attr(test, allow(dead_code))]
    System,
    #[cfg(test)]
    Injected(&'static core::sync::atomic::AtomicI64),
}

impl StackClock {
    fn now(&self) -> Instant {
        match self {
            Self::System => Instant::from_micros_const(
                (axhal::time::wall_time_nanos() / axhal::time::NANOS_PER_MICROS) as i64,
            ),
            #[cfg(test)]
            Self::Injected(now) => Instant::from_micros_const(now.load(Ordering::Relaxed)),
        }
    }
}

pub(crate) struct StackTelemetry {
    task_poll: AtomicU64,
    rounds: AtomicU64,
    work: AtomicU64,
    self_yield: AtomicU64,
    timer_wake: AtomicU64,
    fallback_poll: AtomicU64,
    event_retry: AtomicU64,
    backlog_round: AtomicU64,
    fault: AtomicU64,
    rx_space_wake: AtomicU64,
    tx_enqueue: AtomicU64,
}

impl StackTelemetry {
    pub(crate) const fn new() -> Self {
        Self {
            task_poll: AtomicU64::new(0),
            rounds: AtomicU64::new(0),
            work: AtomicU64::new(0),
            self_yield: AtomicU64::new(0),
            timer_wake: AtomicU64::new(0),
            fallback_poll: AtomicU64::new(0),
            event_retry: AtomicU64::new(0),
            backlog_round: AtomicU64::new(0),
            fault: AtomicU64::new(0),
            rx_space_wake: AtomicU64::new(0),
            tx_enqueue: AtomicU64::new(0),
        }
    }
}

pub(crate) static STACK_TELEMETRY: StackTelemetry = StackTelemetry::new();

/// Observation-only T09 runner counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StackSnapshot {
    pub started: u64,
    pub generation: u64,
    pub task_poll: u64,
    pub rounds: u64,
    pub work: u64,
    pub self_yield: u64,
    pub timer_wake: u64,
    pub fallback_poll: u64,
    pub event_retry: u64,
    pub backlog_round: u64,
    pub fault: u64,
    pub rx_space_wake: u64,
    pub tx_enqueue: u64,
}

fn stack_snapshot_impl(
    lifecycle: &StackRunnerLifecycle,
    event: &StackEvent,
    telemetry: &StackTelemetry,
) -> StackSnapshot {
    StackSnapshot {
        started: lifecycle.is_started() as u64,
        generation: event.generation(),
        task_poll: telemetry.task_poll.load(Ordering::Relaxed),
        rounds: telemetry.rounds.load(Ordering::Relaxed),
        work: telemetry.work.load(Ordering::Relaxed),
        self_yield: telemetry.self_yield.load(Ordering::Relaxed),
        timer_wake: telemetry.timer_wake.load(Ordering::Relaxed),
        fallback_poll: telemetry.fallback_poll.load(Ordering::Relaxed),
        event_retry: telemetry.event_retry.load(Ordering::Relaxed),
        backlog_round: telemetry.backlog_round.load(Ordering::Relaxed),
        fault: telemetry.fault.load(Ordering::Relaxed),
        rx_space_wake: telemetry.rx_space_wake.load(Ordering::Relaxed),
        tx_enqueue: telemetry.tx_enqueue.load(Ordering::Relaxed),
    }
}

/// Returns a non-synchronizing snapshot of the resident stack runner.
pub fn stack_snapshot() -> StackSnapshot {
    stack_snapshot_impl(&STACK_RUNNER_LIFECYCLE, &STACK_EVENT, &STACK_TELEMETRY)
}

/// Publishes stack work committed by a software socket operation.
#[allow(dead_code)]
pub(crate) fn publish_software_work() {
    STACK_EVENT.publish_software();
}

fn select_runner_deadline(
    protocol_deadline: Option<Instant>,
    fallback_deadline: Option<Instant>,
) -> Option<Instant> {
    match (protocol_deadline, fallback_deadline) {
        (Some(protocol), Some(fallback)) => Some(protocol.min(fallback)),
        (Some(protocol), None) => Some(protocol),
        (None, Some(fallback)) => Some(fallback),
        (None, None) => None,
    }
}

pub(crate) struct StackRunnerFuture {
    access: StackAccess,
    rx_lifecycle: &'static RxLifecycle,
    event: &'static StackEvent,
    clock: StackClock,
    telemetry: &'static StackTelemetry,
    timer_deadline: Option<Instant>,
    #[cfg(not(test))]
    timer: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
}

impl StackRunnerFuture {
    fn new(
        access: StackAccess,
        rx_lifecycle: &'static RxLifecycle,
        event: &'static StackEvent,
        clock: StackClock,
        telemetry: &'static StackTelemetry,
    ) -> Self {
        Self {
            access,
            rx_lifecycle,
            event,
            clock,
            telemetry,
            timer_deadline: None,
            #[cfg(not(test))]
            timer: None,
        }
    }

    #[cfg(test)]
    fn timer_deadline(&self) -> Option<Instant> {
        self.timer_deadline
    }

    fn cancel_timer(&mut self) {
        self.timer_deadline = None;
        #[cfg(not(test))]
        {
            self.timer = None;
        }
    }

    fn poll_timer(&mut self, _cx: &mut Context<'_>, _now: Instant) {
        let Some(_deadline) = self.timer_deadline else {
            return;
        };
        #[cfg(not(test))]
        let elapsed = self
            .timer
            .as_mut()
            .is_some_and(|timer| timer.as_mut().poll(_cx).is_ready());
        #[cfg(test)]
        let elapsed = _now >= _deadline;
        if elapsed {
            self.cancel_timer();
            self.telemetry.timer_wake.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn arm_timer(&mut self, cx: &mut Context<'_>, now: Instant, deadline: Option<Instant>) {
        if self.timer_deadline == deadline {
            return;
        }
        self.cancel_timer();
        let Some(deadline) = deadline else {
            return;
        };
        if deadline <= now {
            self.telemetry.timer_wake.fetch_add(1, Ordering::Relaxed);
            cx.waker().wake_by_ref();
            return;
        }
        self.timer_deadline = Some(deadline);
        #[cfg(not(test))]
        {
            let mut timer = Box::pin(sleep_until(TimeValue::from_micros(
                deadline.total_micros() as u64
            )));
            if timer.as_mut().poll(cx).is_ready() {
                self.timer_deadline = None;
                self.telemetry.timer_wake.fetch_add(1, Ordering::Relaxed);
                cx.waker().wake_by_ref();
            } else {
                self.timer = Some(timer);
            }
        }
    }
}

impl Future for StackRunnerFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        this.telemetry.task_poll.fetch_add(1, Ordering::Relaxed);
        let now = this.clock.now();
        this.poll_timer(cx, now);

        let observed = this.event.generation();
        this.event.register(cx.waker());
        let lifecycle = this.rx_lifecycle.load();
        let Some(outcome) = this.access.round(lifecycle.owner_view()) else {
            return Poll::Pending;
        };
        this.telemetry.rounds.fetch_add(1, Ordering::Relaxed);
        this.telemetry
            .work
            .fetch_add(outcome.work as u64, Ordering::Relaxed);
        if outcome.backlog {
            this.telemetry.backlog_round.fetch_add(1, Ordering::Relaxed);
        }
        if outcome.faulted {
            this.telemetry.fault.fetch_add(1, Ordering::Relaxed);
        }
        if outcome.rx_space_woken {
            this.telemetry.rx_space_wake.fetch_add(1, Ordering::Relaxed);
        }
        if outcome.tx_enqueued {
            this.telemetry.tx_enqueue.fetch_add(1, Ordering::Relaxed);
        }

        if outcome.self_yield {
            this.cancel_timer();
            this.telemetry.self_yield.fetch_add(1, Ordering::Relaxed);
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        if this.event.changed_since(observed) {
            this.cancel_timer();
            this.telemetry.event_retry.fetch_add(1, Ordering::Relaxed);
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }

        let fallback_deadline = match lifecycle {
            RxTaskLifecycle::Faulted => None,
            RxTaskLifecycle::Active if !outcome.requires_polling => None,
            _ => {
                this.telemetry.fallback_poll.fetch_add(1, Ordering::Relaxed);
                Some(now + POLLING_FALLBACK)
            }
        };
        let deadline = select_runner_deadline(outcome.protocol_deadline, fallback_deadline);
        this.arm_timer(cx, now, deadline);
        Poll::Pending
    }
}

#[cfg(not(test))]
fn spawn_stack_runner() {
    axtask::spawn_with_name(
        || {
            axtask::future::block_on(StackRunnerFuture::new(
                StackAccess::Global,
                &RX_LIFECYCLE,
                &STACK_EVENT,
                StackClock::System,
                &STACK_TELEMETRY,
            ))
        },
        STACK_RUNNER_TASK_NAME.to_owned(),
    );
}

#[cfg(test)]
fn spawn_stack_runner() {}

pub(crate) fn start_stack_runner() -> Result<(), StartError> {
    start_with(&STACK_RUNNER_LIFECYCLE, spawn_stack_runner)
}

#[cfg(test)]
mod tests {
    extern crate std;

    use alloc::{boxed::Box, sync::Arc};
    use core::{
        future::Future,
        pin::Pin,
        sync::atomic::{AtomicI64, AtomicUsize, Ordering},
        task::{Context, Poll, Waker},
    };

    use smoltcp::{iface::SocketSet, storage::PacketBuffer, time::Instant, wire::IpAddress};

    use super::{
        STACK_EVENT, STACK_RUNNER_LIFECYCLE, STACK_RUNNER_TASK_NAME, StackAccess, StackClock,
        StackEvent, StackRunnerFuture, StackRunnerLifecycle, StackTelemetry, StartError,
        select_runner_deadline, stack_snapshot_impl, start_with,
    };
    use crate::{
        async_rx::{QueueEvent, RxLifecycle, RxTaskLifecycle},
        device::{Device, RxStep, TxOutcome, TxPreflight},
        router::Router,
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

    struct BurstDevice {
        remaining: Arc<AtomicUsize>,
        requires_polling: bool,
    }

    impl Device for BurstDevice {
        fn name(&self) -> &str {
            "stack-runner-test"
        }

        fn recv(&mut self, _buffer: &mut PacketBuffer<()>, _timestamp: Instant) -> RxStep {
            self.remaining
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                    if remaining > 0 {
                        Some(remaining - 1)
                    } else {
                        None
                    }
                })
                .map_or(RxStep::Empty, |_| RxStep::Consumed)
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

        fn requires_polling(&self) -> bool {
            self.requires_polling
        }

        fn register_waker(&self, _waker: &Waker) {}
    }

    fn lifecycle(state: RxTaskLifecycle) -> &'static RxLifecycle {
        let lifecycle = Box::leak(Box::new(RxLifecycle::new()));
        match state {
            RxTaskLifecycle::Polling => {}
            RxTaskLifecycle::Spawned => lifecycle.start().unwrap(),
            RxTaskLifecycle::Active => {
                lifecycle.start().unwrap();
                lifecycle.preflight(true).unwrap();
            }
            RxTaskLifecycle::Unavailable => {
                lifecycle.start().unwrap();
                lifecycle.preflight(false).unwrap();
            }
            RxTaskLifecycle::Faulted => {
                lifecycle.start().unwrap();
                lifecycle.preflight(true).unwrap();
                lifecycle.fatal().unwrap();
            }
        }
        lifecycle
    }

    /// Test device that publishes the future's `StackEvent` exactly once,
    /// from inside `recv`, so the publication lands after the future has
    /// registered its waker and before it rechecks the generation.
    struct PublishOnceDevice {
        event: &'static StackEvent,
        remaining: Arc<AtomicUsize>,
    }

    impl Device for PublishOnceDevice {
        fn name(&self) -> &str {
            "publish-once"
        }

        fn recv(&mut self, _buffer: &mut PacketBuffer<()>, _timestamp: Instant) -> RxStep {
            self.remaining
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                    if remaining > 0 {
                        Some(remaining - 1)
                    } else {
                        None
                    }
                })
                .map_or(RxStep::Empty, |_| {
                    self.event.publish_device();
                    RxStep::Consumed
                })
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

    fn runner(
        state: RxTaskLifecycle,
        work: usize,
        requires_polling: bool,
    ) -> (
        StackRunnerFuture,
        &'static AtomicI64,
        &'static StackTelemetry,
        &'static spin::Mutex<Service>,
    ) {
        let mut router = Router::new();
        if work != 0 || requires_polling {
            router.add_device(Box::new(BurstDevice {
                remaining: Arc::new(AtomicUsize::new(work)),
                requires_polling,
            }));
        }
        let service = Box::leak(Box::new(spin::Mutex::new(Service::new(router, None))));
        let sockets = Box::leak(Box::new(spin::Mutex::new(SocketSet::new(alloc::vec![]))));
        let event = Box::leak(Box::new(StackEvent::new()));
        let now = Box::leak(Box::new(AtomicI64::new(0)));
        let telemetry = Box::leak(Box::new(StackTelemetry::new()));
        (
            StackRunnerFuture::new(
                StackAccess::Injected { service, sockets },
                lifecycle(state),
                event,
                StackClock::Injected(now),
                telemetry,
            ),
            now,
            telemetry,
            service,
        )
    }

    fn poll_once(future: &mut StackRunnerFuture, waker: &Waker) -> Poll<()> {
        let mut cx = Context::from_waker(waker);
        Pin::new(future).poll(&mut cx)
    }

    #[test]
    fn event_before_register_is_seen_by_generation_recheck() {
        let event = StackEvent::new();
        let observed = event.generation();
        event.publish_software();

        let wakes = Arc::new(AtomicUsize::new(0));
        event.register(&counting_waker(wakes.clone()));

        assert!(event.changed_since(observed));
        assert_eq!(wakes.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn event_during_register_window_wakes_and_changes_generation() {
        let event = StackEvent::new();
        let observed = event.generation();
        let wakes = Arc::new(AtomicUsize::new(0));
        event.register(&counting_waker(wakes.clone()));
        event.publish_device();

        assert!(event.changed_since(observed));
        assert_eq!(wakes.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn device_and_software_publication_do_not_touch_queue_generation() {
        let stack = StackEvent::new();
        let queue = QueueEvent::new();
        let queue_generation = queue.generation();

        stack.publish_device();
        stack.publish_software();

        assert_eq!(stack.generation(), 2);
        assert_eq!(queue.generation(), queue_generation);
    }

    #[test]
    fn generation_wraps() {
        let event = StackEvent::with_generation(u64::MAX);
        event.publish_device();
        assert_eq!(event.generation(), 0);
    }

    #[test]
    fn start_seam_spawns_once_without_touching_production_global() {
        let lifecycle = StackRunnerLifecycle::new();
        let spawns = Arc::new(AtomicUsize::new(0));

        let first = spawns.clone();
        assert_eq!(
            start_with(&lifecycle, || {
                first.fetch_add(1, Ordering::Relaxed);
            }),
            Ok(())
        );
        let second = spawns.clone();
        assert_eq!(
            start_with(&lifecycle, || {
                second.fetch_add(1, Ordering::Relaxed);
            }),
            Err(StartError::AlreadyStarted)
        );

        assert_eq!(spawns.load(Ordering::Relaxed), 1);
        assert!(!STACK_RUNNER_LIFECYCLE.is_started());
        assert_eq!(STACK_EVENT.generation(), 0);
        assert_eq!(STACK_RUNNER_TASK_NAME, "axnet-stack-runner");
    }

    #[test]
    fn concurrent_start_has_one_winner() {
        let lifecycle: &'static StackRunnerLifecycle =
            Box::leak(Box::new(StackRunnerLifecycle::new()));
        let spawns = Arc::new(AtomicUsize::new(0));
        let mut threads = std::vec::Vec::new();

        for _ in 0..8 {
            let spawns = spawns.clone();
            threads.push(std::thread::spawn(move || {
                start_with(lifecycle, || {
                    spawns.fetch_add(1, Ordering::Relaxed);
                })
            }));
        }

        let winners = threads
            .into_iter()
            .map(|thread| thread.join().unwrap().is_ok())
            .filter(|won| *won)
            .count();
        assert_eq!(winners, 1);
        assert_eq!(spawns.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn active_quiet_has_no_periodic_fallback_or_self_wake() {
        let (mut future, _, telemetry, _) = runner(RxTaskLifecycle::Active, 0, false);
        let wakes = Arc::new(AtomicUsize::new(0));
        assert_eq!(
            poll_once(&mut future, &counting_waker(wakes.clone())),
            Poll::Pending
        );
        assert_eq!(future.timer_deadline(), None);
        assert_eq!(wakes.load(Ordering::Relaxed), 0);
        assert_eq!(telemetry.rounds.load(Ordering::Relaxed), 1);
        assert_eq!(telemetry.fallback_poll.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn lifecycle_matrix_arms_only_allowed_fallbacks() {
        for state in [
            RxTaskLifecycle::Polling,
            RxTaskLifecycle::Spawned,
            RxTaskLifecycle::Unavailable,
        ] {
            let (mut future, ..) = runner(state, 0, false);
            assert_eq!(
                poll_once(&mut future, &counting_waker(Arc::new(AtomicUsize::new(0)))),
                Poll::Pending
            );
            assert_eq!(
                future.timer_deadline(),
                Some(Instant::from_millis_const(10))
            );
        }

        let (mut active_polling, ..) = runner(RxTaskLifecycle::Active, 0, true);
        assert_eq!(
            poll_once(
                &mut active_polling,
                &counting_waker(Arc::new(AtomicUsize::new(0)))
            ),
            Poll::Pending
        );
        assert_eq!(
            active_polling.timer_deadline(),
            Some(Instant::from_millis_const(10))
        );

        let (mut faulted, ..) = runner(RxTaskLifecycle::Faulted, 0, false);
        assert_eq!(
            poll_once(&mut faulted, &counting_waker(Arc::new(AtomicUsize::new(0)))),
            Poll::Pending
        );
        assert_eq!(faulted.timer_deadline(), None);
    }

    #[test]
    fn elapsed_fallback_runs_one_more_bounded_round_and_rearms() {
        let (mut future, now, telemetry, _) = runner(RxTaskLifecycle::Polling, 0, false);
        let waker = counting_waker(Arc::new(AtomicUsize::new(0)));
        assert_eq!(poll_once(&mut future, &waker), Poll::Pending);
        now.store(10_000, Ordering::Relaxed);
        assert_eq!(poll_once(&mut future, &waker), Poll::Pending);
        assert_eq!(telemetry.timer_wake.load(Ordering::Relaxed), 1);
        assert_eq!(telemetry.rounds.load(Ordering::Relaxed), 2);
        assert_eq!(
            future.timer_deadline(),
            Some(Instant::from_millis_const(20))
        );
    }

    #[test]
    fn deadline_selection_replaces_earlier_and_later_deadlines() {
        let early = Instant::from_millis_const(5);
        let late = Instant::from_millis_const(10);
        assert_eq!(select_runner_deadline(Some(late), Some(early)), Some(early));
        assert_eq!(select_runner_deadline(Some(early), Some(late)), Some(early));
        assert_eq!(select_runner_deadline(None, Some(late)), Some(late));
        assert_eq!(select_runner_deadline(Some(early), None), Some(early));
    }

    struct UnlockWake {
        service: &'static spin::Mutex<Service>,
        unlocked: Arc<AtomicUsize>,
    }

    impl alloc::task::Wake for UnlockWake {
        fn wake(self: Arc<Self>) {
            if self.service.try_lock().is_some() {
                self.unlocked.fetch_add(1, Ordering::Relaxed);
            }
        }

        fn wake_by_ref(self: &Arc<Self>) {
            if self.service.try_lock().is_some() {
                self.unlocked.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    #[test]
    fn budget_self_yield_wakes_after_service_guard_is_released() {
        let (mut future, _, telemetry, service) = runner(RxTaskLifecycle::Active, 33, false);
        let unlocked = Arc::new(AtomicUsize::new(0));
        let waker = Waker::from(Arc::new(UnlockWake {
            service,
            unlocked: unlocked.clone(),
        }));

        assert_eq!(poll_once(&mut future, &waker), Poll::Pending);
        assert_eq!(telemetry.self_yield.load(Ordering::Relaxed), 1);
        assert_eq!(unlocked.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn event_published_inside_round_retries_after_guard_release() {
        // An event published from inside `recv` (register done, recheck not
        // yet run) must be observed by the generation recheck; the retry
        // self-wake must happen only after the Service guard is released.
        let event: &'static StackEvent = Box::leak(Box::new(StackEvent::new()));
        let mut router = Router::new();
        router.add_device(Box::new(PublishOnceDevice {
            event,
            remaining: Arc::new(AtomicUsize::new(1)),
        }));
        let service = Box::leak(Box::new(spin::Mutex::new(Service::new(router, None))));
        let sockets = Box::leak(Box::new(spin::Mutex::new(SocketSet::new(alloc::vec![]))));
        let lifecycle = lifecycle(RxTaskLifecycle::Active);
        let now = Box::leak(Box::new(AtomicI64::new(0)));
        let telemetry = Box::leak(Box::new(StackTelemetry::new()));
        let mut future = StackRunnerFuture::new(
            StackAccess::Injected { service, sockets },
            lifecycle,
            event,
            StackClock::Injected(now),
            telemetry,
        );

        let unlocked = Arc::new(AtomicUsize::new(0));
        let waker = Waker::from(Arc::new(UnlockWake {
            service,
            unlocked: unlocked.clone(),
        }));

        assert_eq!(poll_once(&mut future, &waker), Poll::Pending);
        // One event publication from the round, one retry self-wake.
        assert_eq!(telemetry.event_retry.load(Ordering::Relaxed), 1);
        assert_eq!(telemetry.rounds.load(Ordering::Relaxed), 1);
        assert!(telemetry.task_poll.load(Ordering::Relaxed) >= 1);
        // Retry wake is after the Service guard was released.
        assert_eq!(unlocked.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn timer_replacement_ignores_stale_and_expires_exactly_once() {
        let (mut future, now, telemetry, _) = runner(RxTaskLifecycle::Polling, 0, false);
        let wakes = Arc::new(AtomicUsize::new(0));
        let waker = counting_waker(wakes.clone());
        let mut cx = Context::from_waker(&waker);
        now.store(0, Ordering::Relaxed);
        let t0 = Instant::from_millis_const(0);
        let t5 = Instant::from_millis_const(5);
        let t10 = Instant::from_millis_const(10);
        let t20 = Instant::from_millis_const(20);

        // Earlier deadline then a later replacement: the later one wins.
        future.arm_timer(&mut cx, t0, Some(t5));
        assert_eq!(future.timer_deadline(), Some(t5));
        future.arm_timer(&mut cx, t0, Some(t10));
        assert_eq!(future.timer_deadline(), Some(t10));

        // Stale deadline arrival (old t5) must not fire anymore.
        now.store(5_000, Ordering::Relaxed);
        future.poll_timer(&mut cx, Instant::from_millis_const(5));
        assert_eq!(future.timer_deadline(), Some(t10));
        assert_eq!(telemetry.timer_wake.load(Ordering::Relaxed), 0);

        // Current deadline arrival fires exactly once and cancels.
        now.store(10_000, Ordering::Relaxed);
        future.poll_timer(&mut cx, Instant::from_millis_const(10));
        assert_eq!(future.timer_deadline(), None);
        assert_eq!(telemetry.timer_wake.load(Ordering::Relaxed), 1);

        // A second poll at the same instant does not double-fire.
        future.poll_timer(&mut cx, Instant::from_millis_const(10));
        assert_eq!(telemetry.timer_wake.load(Ordering::Relaxed), 1);

        // Re-arming with a fresh future deadline works after expiry and
        // does not double-count the previous expiry.
        future.arm_timer(&mut cx, t10, Some(t20));
        assert_eq!(future.timer_deadline(), Some(t20));
        assert_eq!(telemetry.timer_wake.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn pre_service_global_access_is_safe_and_does_not_start_runner() {
        assert!(
            StackAccess::Global
                .round(RxTaskLifecycle::Polling.owner_view())
                .is_none()
        );
        assert!(!STACK_RUNNER_LIFECYCLE.is_started());
    }

    #[test]
    fn init_installs_service_before_starting_exactly_one_runner() {
        let source = include_str!("lib.rs");
        let install = source.find("SERVICE.call_once").unwrap();
        let start = source.find("start_stack_runner()").unwrap();
        assert!(install < start);
        assert_eq!(source.matches("start_stack_runner()").count(), 1);
        assert!(source.contains("while get_service().poll"));
        assert!(include_str!("service.rs").contains("pub fn register_waker"));
    }

    #[test]
    fn snapshot_is_observation_only_and_mirrors_local_state() {
        let lifecycle = StackRunnerLifecycle::new();
        lifecycle.start().unwrap();
        let event = StackEvent::with_generation(7);
        let telemetry = StackTelemetry::new();
        telemetry.task_poll.store(11, Ordering::Relaxed);
        telemetry.rounds.store(9, Ordering::Relaxed);
        telemetry.work.store(33, Ordering::Relaxed);
        telemetry.self_yield.store(2, Ordering::Relaxed);
        telemetry.fallback_poll.store(4, Ordering::Relaxed);

        let snapshot = stack_snapshot_impl(&lifecycle, &event, &telemetry);
        assert_eq!(snapshot.started, 1);
        assert_eq!(snapshot.generation, 7);
        assert_eq!(snapshot.task_poll, 11);
        assert_eq!(snapshot.rounds, 9);
        assert_eq!(snapshot.work, 33);
        assert_eq!(snapshot.self_yield, 2);
        assert_eq!(snapshot.fallback_poll, 4);
    }
}
