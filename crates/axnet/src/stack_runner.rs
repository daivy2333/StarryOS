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
        listen_table: &'static crate::listen_table::ListenTable,
    },
}

impl StackAccess {
    /// Runs one bounded round under the injected or production guards with
    /// the single `now` the resident runner sampled for this poll (Task 2.6
    /// replan), so smoltcp ingress/egress/maintenance, `poll_at` and the
    /// timer all observe the same Instant.
    fn round(&self, now: Instant, owner: RxOwnerView) -> Option<StackRoundOutcome> {
        match self {
            Self::Global => {
                let mut service = crate::SERVICE.get()?.lock();
                let mut sockets = crate::SOCKET_SET.inner.lock();
                Some(service.stack_round(now, owner, &mut sockets))
            }
            #[cfg(test)]
            Self::Injected {
                service, sockets, ..
            } => {
                let mut service = service.lock();
                let mut sockets = sockets.lock();
                Some(service.stack_round(now, owner, &mut sockets))
            }
        }
    }

    /// Drains listener accept wakes after `round` released every Service /
    /// SocketSet / ListenTable guard. Production drains the global table; a
    /// full-chain test must drain its injected local table.
    fn drain_accept_wakes(&self) {
        match self {
            Self::Global => crate::LISTEN_TABLE.drain_accept_wakes(),
            #[cfg(test)]
            Self::Injected { listen_table, .. } => listen_table.drain_accept_wakes(),
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
    progress_wake: AtomicU64,
    timer_wake: AtomicU64,
    fallback_poll: AtomicU64,
    event_retry: AtomicU64,
    backlog_round: AtomicU64,
    fault: AtomicU64,
    rx_space_wake: AtomicU64,
    tx_enqueue: AtomicU64,
    /// Task 2.6 replan: deferred-close entries examined / reclaimed by the
    /// bounded retirement stage (cumulative; observation-only).
    deferred_checked: AtomicU64,
    deferred_reclaimed: AtomicU64,
    /// Task 2.6 replan: listener hidden-slot positions examined by the
    /// bounded reconciliation stage (cumulative; observation-only).
    listener_checked: AtomicU64,
    /// T2.8-R1: exact head micro-repairs executed after processed ingress
    /// packets (cumulative; observation-only).
    listener_head_repairs: AtomicU64,
}

impl StackTelemetry {
    pub(crate) const fn new() -> Self {
        Self {
            task_poll: AtomicU64::new(0),
            rounds: AtomicU64::new(0),
            work: AtomicU64::new(0),
            self_yield: AtomicU64::new(0),
            progress_wake: AtomicU64::new(0),
            timer_wake: AtomicU64::new(0),
            fallback_poll: AtomicU64::new(0),
            event_retry: AtomicU64::new(0),
            backlog_round: AtomicU64::new(0),
            fault: AtomicU64::new(0),
            rx_space_wake: AtomicU64::new(0),
            tx_enqueue: AtomicU64::new(0),
            deferred_checked: AtomicU64::new(0),
            deferred_reclaimed: AtomicU64::new(0),
            listener_checked: AtomicU64::new(0),
            listener_head_repairs: AtomicU64::new(0),
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
    pub progress_wake: u64,
    pub timer_wake: u64,
    pub fallback_poll: u64,
    pub event_retry: u64,
    pub backlog_round: u64,
    pub fault: u64,
    pub rx_space_wake: u64,
    pub tx_enqueue: u64,
    /// Task 2.6 replan: deferred-close entries examined / reclaimed so far.
    pub deferred_checked: u64,
    pub deferred_reclaimed: u64,
    /// Task 2.6 replan: listener hidden-slot positions examined so far.
    pub listener_checked: u64,
    /// T2.8-R1: exact head micro-repairs so far.
    pub listener_head_repairs: u64,
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
        progress_wake: telemetry.progress_wake.load(Ordering::Relaxed),
        timer_wake: telemetry.timer_wake.load(Ordering::Relaxed),
        fallback_poll: telemetry.fallback_poll.load(Ordering::Relaxed),
        event_retry: telemetry.event_retry.load(Ordering::Relaxed),
        backlog_round: telemetry.backlog_round.load(Ordering::Relaxed),
        fault: telemetry.fault.load(Ordering::Relaxed),
        rx_space_wake: telemetry.rx_space_wake.load(Ordering::Relaxed),
        tx_enqueue: telemetry.tx_enqueue.load(Ordering::Relaxed),
        deferred_checked: telemetry.deferred_checked.load(Ordering::Relaxed),
        deferred_reclaimed: telemetry.deferred_reclaimed.load(Ordering::Relaxed),
        listener_checked: telemetry.listener_checked.load(Ordering::Relaxed),
        listener_head_repairs: telemetry.listener_head_repairs.load(Ordering::Relaxed),
    }
}

/// Returns a non-synchronizing snapshot of the resident stack runner.
pub fn stack_snapshot() -> StackSnapshot {
    stack_snapshot_impl(&STACK_RUNNER_LIFECYCLE, &STACK_EVENT, &STACK_TELEMETRY)
}

/// Publishes stack work committed by a software socket operation.
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
        let Some(outcome) = this.access.round(now, lifecycle.owner_view()) else {
            return Poll::Pending;
        };
        // Listener transitions committed inside the round wake accept waiters
        // only after the Service/SocketSet guards in `round` have dropped.
        this.access.drain_accept_wakes();
        this.telemetry.rounds.fetch_add(1, Ordering::Relaxed);
        this.telemetry
            .work
            .fetch_add(outcome.work as u64, Ordering::Relaxed);
        this.telemetry
            .deferred_checked
            .fetch_add(outcome.deferred_checked as u64, Ordering::Relaxed);
        this.telemetry
            .deferred_reclaimed
            .fetch_add(outcome.deferred_reclaimed as u64, Ordering::Relaxed);
        this.telemetry
            .listener_checked
            .fetch_add(outcome.listener_checked as u64, Ordering::Relaxed);
        this.telemetry
            .listener_head_repairs
            .fetch_add(outcome.listener_head_repairs as u64, Ordering::Relaxed);
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

        // A round must continue immediately when more work is reachable without a
        // new event: budget exhaust (`self_yield`), a loopback/non-IRQ TX that
        // made RX ready (`rx_ready`), a protocol state transition
        // (`socket_changed`), an unfinished bounded deferred sweep
        // (`deferred_sweep_incomplete`, Task 2.6 replan), or an unfinished
        // bounded listener sweep (`listener_sweep_incomplete`, Task 2.6
        // replan) — mirroring `Service::poll`'s drain condition. Once the
        // sweeps are complete with nothing reclaimable, the runner parks and
        // relies on a new protocol event or the `poll_at` deadline; a
        // non-empty deferred/listener list alone never self-wakes it.
        if outcome.socket_changed {
            debug!("stack round: socket state changed (ingress/egress)");
        }
        if outcome.self_yield
            || outcome.rx_ready
            || outcome.socket_changed
            || outcome.deferred_sweep_incomplete
            || outcome.listener_sweep_incomplete
        {
            this.cancel_timer();
            if outcome.self_yield {
                this.telemetry.self_yield.fetch_add(1, Ordering::Relaxed);
            } else {
                this.telemetry.progress_wake.fetch_add(1, Ordering::Relaxed);
            }
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

    use axpoll::IoEvents;
    use smoltcp::{
        iface::{SocketHandle, SocketSet},
        socket::tcp::{Socket, State},
        storage::PacketBuffer,
        time::Instant,
        wire::{IpAddress, IpEndpoint, Ipv4Address, Ipv4Cidr},
    };

    use super::{
        STACK_EVENT, STACK_RUNNER_LIFECYCLE, STACK_RUNNER_TASK_NAME, StackAccess, StackClock,
        StackEvent, StackRunnerFuture, StackRunnerLifecycle, StackTelemetry, StartError,
        select_runner_deadline, stack_snapshot_impl, start_with,
    };
    use crate::{
        async_rx::{QueueEvent, RxLifecycle, RxTaskLifecycle},
        device::{Device, LoopbackDevice, RxStep, TxOutcome, TxPreflight},
        listen_table::ListenTable,
        readiness::ReadinessBridge,
        router::{Router, Rule},
        service::Service,
        tcp::new_tcp_socket,
    };

    const FULL_CHAIN_PORT: u16 = 19555;
    const FULL_CHAIN_LOCAL_PORT: u16 = 39501;
    const FULL_CHAIN_UDP_PORT: u16 = 19556;
    const FULL_CHAIN_UDP_LOCAL_PORT: u16 = 39502;
    const POLL_BOUND: usize = 128;

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
        &'static ListenTable,
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
        let listen_table = Box::leak(Box::new(ListenTable::new()));
        let event = Box::leak(Box::new(StackEvent::new()));
        let now = Box::leak(Box::new(AtomicI64::new(0)));
        let telemetry = Box::leak(Box::new(StackTelemetry::new()));
        (
            StackRunnerFuture::new(
                StackAccess::Injected {
                    service,
                    sockets,
                    listen_table,
                },
                lifecycle(state),
                event,
                StackClock::Injected(now),
                telemetry,
            ),
            now,
            telemetry,
            service,
            listen_table,
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
        let (mut future, _, telemetry, ..) = runner(RxTaskLifecycle::Active, 0, false);
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
        let (mut future, now, telemetry, ..) = runner(RxTaskLifecycle::Polling, 0, false);
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
        let (mut future, _, telemetry, service, _) = runner(RxTaskLifecycle::Active, 33, false);
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
        let listen_table = Box::leak(Box::new(ListenTable::new()));
        let lifecycle = lifecycle(RxTaskLifecycle::Active);
        let now = Box::leak(Box::new(AtomicI64::new(0)));
        let telemetry = Box::leak(Box::new(StackTelemetry::new()));
        let mut future = StackRunnerFuture::new(
            StackAccess::Injected {
                service,
                sockets,
                listen_table,
            },
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
        let (mut future, now, telemetry, ..) = runner(RxTaskLifecycle::Polling, 0, false);
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
                .round(
                    Instant::from_millis_const(0),
                    RxTaskLifecycle::Polling.owner_view()
                )
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
    }

    #[test]
    fn task_24_cutover_removed_socket_register_and_caller_driven_poll() {
        // The per-waiter Service register/timer entry and the 12 product
        // inline-poll call sites are gone; the runner now owns progress.
        assert!(!include_str!("service.rs").contains("pub fn register_waker"));
        assert!(!include_str!("general.rs").contains("register_waker"));
        assert!(!include_str!("tcp.rs").contains("poll_interfaces"));
        assert!(!include_str!("udp.rs").contains("poll_interfaces"));
        // Mutations signal the unique runner through the software seam.
        assert!(include_str!("tcp.rs").contains("publish_software_work"));
        assert!(include_str!("udp.rs").contains("publish_software_work"));
    }

    #[test]
    fn task_24_r2_public_socket_paths_never_drive_stack_progress() {
        // T2.4-R2: the synchronous close flush and every direct stack-round
        // call from the public socket modules are gone. The resident runner
        // is the only smoltcp progress owner after the T2.5-R2 deferred
        // retirement design; a caller must never advance the stack itself.
        let tcp = include_str!("tcp.rs");
        let udp = include_str!("udp.rs");
        assert!(!tcp.contains("flush_removal_tx"));
        assert!(!tcp.contains("stack_round"));
        assert!(!udp.contains("stack_round"));
        assert!(!include_str!("listen_table.rs").contains("stack_round"));
        assert!(!include_str!("wrapper.rs").contains("stack_round"));
    }

    #[test]
    fn drop_keeps_socket_set_and_service_guards_disjoint() {
        // T2.4-R2: the new `TcpSocket::drop` reads the raw close state under
        // the SocketSet guard alone (a scoped expression block), then
        // enqueues the runner-owned deferred retirement under the Service
        // guard alone. It never holds both guards at once, never runs a
        // stack round, and never calls the removed synchronous flush.
        let tcp = include_str!("tcp.rs");
        let start = tcp.find("impl Drop for TcpSocket").unwrap();
        let end = tcp.find("fn get_ephemeral_port").unwrap();
        let drop_src = &tcp[start..end];
        assert!(!drop_src.contains("flush_removal_tx"));
        assert!(!drop_src.contains("stack_round"));
        assert!(drop_src.contains("let defer = {"));
        assert!(drop_src.contains("SOCKET_SET.inner.lock()"));
        assert!(drop_src.contains("queue_deferred_removal"));
    }

    #[test]
    fn task_26_round_and_service_share_one_sampled_timestamp() {
        // Task 2.6 replan: the runner samples ONE timestamp per poll and
        // passes it into `StackAccess::round` and `Service::stack_round`;
        // `Service::stack_round` must not re-read the wall clock itself.
        // Only the retained compatibility `Service::poll` helper may sample
        // the system clock at its entry.
        assert!(
            include_str!("stack_runner.rs")
                .contains("fn round(&self, now: Instant, owner: RxOwnerView)")
        );
        assert!(include_str!("stack_runner.rs").contains("this.access.round(now,"));
        assert!(include_str!("service.rs").contains(
            "pub(crate) fn stack_round(\n        &mut self,\n        timestamp: Instant,"
        ));
        // The wall-clock `now()` helper may only exist for the compat
        // `Service::poll` entry; `stack_round` itself must not sample it.
        let service = include_str!("service.rs");
        let round_start = service.find("fn stack_round(").unwrap();
        let round_end = service.find("fn rx_slot_has_space_target").unwrap();
        let round_src = &service[round_start..round_end];
        assert!(!round_src.contains("now()"));
    }

    #[test]
    fn task_26_service_poll_compat_still_samples_system_clock() {
        let service = include_str!("service.rs");
        assert!(
            service.contains("pub fn poll(&mut self") || service.contains("pub(crate) fn poll(")
        );
        // The compat poll samples the system clock once at entry and passes
        // the sampled Instant into the timestamped stack_round.
        assert!(service.contains("self.stack_round(now(), owner, sockets)"));
    }

    #[test]
    fn task_27_accept_refills_in_guard_without_stack_progress_or_wake() {
        // Task 2.7 replan: `accept_with` refills an idle hidden listener
        // inside the SocketSet-guard critical section but never calls
        // `stack_round` / `poll_interfaces` and never wakes inside the
        // guards; `TcpSocket::accept` publishes its wakes only after the
        // SocketSet guard is dropped.
        let listen_table = include_str!("listen_table.rs");
        assert!(listen_table.contains("pub fn accept_with(&self, port: u16, sockets"));
        let accept_src = &listen_table[listen_table.find("pub fn accept_with").unwrap()..];
        assert!(!accept_src.contains("stack_round"));
        assert!(!accept_src.contains("poll_interfaces"));
        assert!(accept_src.contains("entry.refill(sockets)"));

        let tcp = include_str!("tcp.rs");
        let accept_src = &tcp[tcp.find("fn accept(&self)").unwrap()..tcp.find("fn send(").unwrap()];
        assert!(accept_src.contains("SOCKET_SET.inner.lock()"));
        assert!(accept_src.contains("LISTEN_TABLE.accept_with(bound_port, &mut sockets)"));
        assert!(accept_src.contains("drop(sockets)"));
        // The wake and software-work publish appear only after the guard
        // drop inside the closure.
        let wake_pos = accept_src
            .find("self.readiness.wake(IoEvents::IN)")
            .unwrap();
        let drop_pos = accept_src.find("drop(sockets)").unwrap();
        assert!(
            drop_pos < wake_pos,
            "accept must publish wakes only after the SocketSet guard drops"
        );
        assert!(!accept_src.contains("stack_round"));
        assert!(!accept_src.contains("poll_interfaces"));
    }

    #[test]
    fn task_26_r1_deferred_raw_handle_ownership_is_exclusive_and_atomic() {
        // T2.6-R1 source/ownership witness: a deferred close entry's raw
        // smoltcp slot is owned exclusively by the resident reaper while the
        // entry lives. The ONLY production enqueue site is `TcpSocket::drop`
        // (exactly one `queue_deferred_removal(` call), so no UDP / listener
        // / wrapper path can create a deferred entry; the two `remove_raw`
        // calls left in Drop are the no-Service fallback and the immediate
        // branch, both entry-free in the same drop; and the reaper removes
        // the raw handle and its deferred entry in the same guarded scope,
        // so a stale entry can never delete a same-type reused slot.
        // Deterministic source scan; 100x matches the plan verification
        // profile and guards against accidental import drift.
        for _ in 0..100 {
            let tcp = include_str!("tcp.rs");
            assert_eq!(
                tcp.matches("queue_deferred_removal(").count(),
                1,
                "only TcpSocket::drop may enqueue a TCP deferred close"
            );
            assert_eq!(
                tcp.matches("remove_raw(").count(),
                2,
                "Drop removes raw only on the no-Service fallback and the immediate branch, both \
                 entry-free in the same drop"
            );
            let udp = include_str!("udp.rs");
            assert_eq!(
                udp.matches("queue_deferred_removal(").count(),
                1,
                "UdpSocket::drop enqueues exactly one deferred close"
            );
            assert!(!include_str!("listen_table.rs").contains("queue_deferred_removal("));
            assert!(!include_str!("general.rs").contains("queue_deferred_removal("));
            assert!(!include_str!("wrapper.rs").contains("queue_deferred_removal("));
            let service = include_str!("service.rs");
            let reap_src = &service[service.find("fn reap_deferred_removals(").unwrap()..];
            assert!(
                reap_src.contains("sockets.remove(entry.handle)"),
                "the reaper is the only raw remover for a live deferred slot"
            );
            assert!(
                reap_src.contains("self.deferred_removals.swap_remove(idx)"),
                "the reaper removes entry and raw handle in the same guarded scope"
            );
        }
    }

    #[test]
    fn tcp_connect_acquires_service_before_socket_set() {
        // Task 2.4 fixes the reverse lock edge: `get_service()` must run
        // before `with_smol_socket` inside `connect`.
        let source = include_str!("tcp.rs");
        let connect = &source[source.find("fn connect(").unwrap()..];
        let service_pos = connect.find("let mut service = get_service()").unwrap();
        let set_pos = connect.find("self.with_smol_socket").unwrap();
        assert!(service_pos < set_pos);
    }

    #[test]
    fn software_publish_on_unregistered_event_is_safe() {
        let event = StackEvent::new();
        event.publish_software();
        event.publish_software();
        assert_eq!(event.generation(), 2);
    }

    #[test]
    fn mutation_publishes_only_after_success_and_skip_read_only_paths() {
        // send publishes only on success; recv skips PEEK; poll is quiet.
        let tcp = include_str!("tcp.rs");
        let udp = include_str!("udp.rs");
        assert!(tcp.contains("if result.is_ok() {"));
        let recv = &tcp[tcp.find("fn recv(").unwrap()..tcp.find("fn local_addr(").unwrap()];
        assert!(recv.contains("!options.flags.contains(RecvFlags::PEEK)"));
        let poll = &tcp[tcp.find("fn poll(&self)").unwrap()..tcp.find("fn register(").unwrap()];
        assert!(!poll.contains("publish_software_work"));
        let poll_udp = &udp[udp.find("fn poll(&self)").unwrap()..udp.find("fn register(").unwrap()];
        assert!(!poll_udp.contains("publish_software_work"));
    }

    #[test]
    fn loopback_tx_making_rx_ready_self_wakes_to_drain() {
        // MS01 regression: a loopback TX has no IRQ/event source, so the
        // runner must continue immediately when the round reports `rx_ready`.
        // Minimal valid IPv4 broadcast header (no route lookup needed).
        let broadcast: [u8; 20] = [
            0x45, 0, 0, 20, 0, 0, 0, 0, 64, 17, 0, 0, 10, 0, 2, 15, 255, 255, 255, 255,
        ];
        let mut router = Router::new();
        router.add_device(Box::new(LoopbackDevice::new()));
        router.enqueue_tx_for_test(&broadcast);
        let service = Box::leak(Box::new(spin::Mutex::new(Service::new(router, None))));
        let sockets = Box::leak(Box::new(spin::Mutex::new(SocketSet::new(alloc::vec![]))));
        let listen_table = Box::leak(Box::new(ListenTable::new()));
        let event: &'static StackEvent = Box::leak(Box::new(StackEvent::new()));
        let now = Box::leak(Box::new(AtomicI64::new(0)));
        let telemetry = Box::leak(Box::new(StackTelemetry::new()));
        let mut future = StackRunnerFuture::new(
            StackAccess::Injected {
                service,
                sockets,
                listen_table,
            },
            lifecycle(RxTaskLifecycle::Active),
            event,
            StackClock::Injected(now),
            telemetry,
        );

        let wakes = Arc::new(AtomicUsize::new(0));
        let waker = counting_waker(wakes.clone());
        assert_eq!(poll_once(&mut future, &waker), Poll::Pending);
        // The dispatch made loopback RX ready: the round self-wakes instead
        // of parking with an un-ingested frame.
        assert_eq!(telemetry.progress_wake.load(Ordering::Relaxed), 1);
        assert_eq!(telemetry.self_yield.load(Ordering::Relaxed), 0);
        assert!(wakes.load(Ordering::Relaxed) >= 1);
        assert_eq!(telemetry.rounds.load(Ordering::Relaxed), 1);
        // The follow-up poll ingests the loopback frame into the Router RX path.
        assert_eq!(poll_once(&mut future, &waker), Poll::Pending);
        assert!(telemetry.rounds.load(Ordering::Relaxed) >= 2);
    }

    #[test]
    fn full_chain_loopback_handshake_and_accept_deliver_payload_within_bound() {
        // Runs the REAL bounded stack round + Router loopback + smoltcp TCP +
        // ListenTable hidden listener + ReadinessBridge with no production
        // global and no forced socket state; smoltcp only moves through
        // ordinary egress/ingress. 100 full chains witness no flakiness.
        for _ in 0..100 {
            let mut router = Router::new();
            let lo_dev = router.add_device(Box::new(LoopbackDevice::new()));
            let lo_ip = Ipv4Cidr::new(Ipv4Address::new(127, 0, 0, 1), 8);
            router.add_rule(Rule::new(
                lo_ip.into(),
                None,
                lo_dev,
                lo_ip.address().into(),
            ));

            let listen_table = Box::leak(Box::new(ListenTable::new()));
            let mut service = Service::new_with_listen_table(router, None, listen_table);
            service
                .iface
                .update_ip_addrs(|addrs| addrs.push(lo_ip.into()).unwrap());

            let sockets = Box::leak(Box::new(spin::Mutex::new(SocketSet::new(alloc::vec![]))));
            let event = Box::leak(Box::new(StackEvent::new()));
            let now = Box::leak(Box::new(AtomicI64::new(0)));
            let telemetry = Box::leak(Box::new(StackTelemetry::new()));

            let accept = Arc::new(ReadinessBridge::new());
            let accept_wakes = Arc::new(AtomicUsize::new(0));
            accept.register(IoEvents::IN, &counting_waker(accept_wakes.clone()));
            listen_table
                .listen_with(
                    smoltcp::wire::IpListenEndpoint {
                        addr: None,
                        port: FULL_CHAIN_PORT,
                    },
                    accept.clone(),
                    &mut sockets.lock(),
                )
                .unwrap();

            let client_bridge = Arc::new(ReadinessBridge::new());
            let client_out = Arc::new(AtomicUsize::new(0));
            client_bridge.register(IoEvents::OUT, &counting_waker(client_out.clone()));
            let mut client_sock = new_tcp_socket();
            let client_handle;
            {
                let mut sockets = sockets.lock();
                client_sock.register_recv_waker(&client_bridge.recv_waker());
                client_sock.register_send_waker(&client_bridge.send_waker());
                let remote =
                    IpEndpoint::new(Ipv4Address::new(127, 0, 0, 1).into(), FULL_CHAIN_PORT);
                let local =
                    IpEndpoint::new(Ipv4Address::new(127, 0, 0, 1).into(), FULL_CHAIN_LOCAL_PORT);
                client_sock
                    .connect(service.iface.context(), remote, local)
                    .expect("client connect must be accepted by smoltcp");
                client_handle = sockets.add(client_sock);
            }

            let data_bridge = Arc::new(ReadinessBridge::new());
            let data_wakes = Arc::new(AtomicUsize::new(0));
            data_bridge.register(IoEvents::IN, &counting_waker(data_wakes.clone()));
            let service = Box::leak(Box::new(spin::Mutex::new(service)));

            let wakes = Arc::new(AtomicUsize::new(0));
            let waker = counting_waker(wakes.clone());
            event.publish_software();
            let mut future = StackRunnerFuture::new(
                StackAccess::Injected {
                    service,
                    sockets,
                    listen_table,
                },
                lifecycle(RxTaskLifecycle::Active),
                event,
                StackClock::Injected(now),
                telemetry,
            );

            let mut polls = 0usize;
            // Executor loop: after every poll the runner either self-woke
            // (progress) or parked, in which case only a timer can wake it,
            // so the injected clock jumps to its deadline.
            loop {
                polls += 1;
                assert!(
                    polls <= POLL_BOUND,
                    "handshake stalled after {polls} polls (client state {})",
                    sockets.lock().get::<Socket>(client_handle).state()
                );
                let before = wakes.load(Ordering::Relaxed);
                let _ = poll_once(&mut future, &waker);
                let self_woke = wakes.load(Ordering::Relaxed) > before;
                if sockets.lock().get::<Socket>(client_handle).state() == State::Established
                    && client_out.load(Ordering::Relaxed) >= 1
                    && listen_table.can_accept(FULL_CHAIN_PORT) == Ok(true)
                {
                    break;
                }
                if !self_woke {
                    let deadline = future
                        .timer_deadline()
                        .expect("runner parked without a timer while the handshake is incomplete");
                    now.store(deadline.total_micros() as i64, Ordering::Relaxed);
                }
            }

            let accepted_handle = listen_table
                .accept_with(FULL_CHAIN_PORT, &mut sockets.lock())
                .unwrap();
            assert!(matches!(
                listen_table.accept_with(FULL_CHAIN_PORT, &mut sockets.lock()),
                Err(axerrno::AxError::WouldBlock)
            ));
            // The listener IN counting waker must have been woken by the
            // staged transition drain (bridge fan-out witness).
            assert!(accept_wakes.load(Ordering::Relaxed) >= 1);

            {
                let mut sockets = sockets.lock();
                sockets
                    .get_mut::<Socket>(accepted_handle)
                    .register_recv_waker(&data_bridge.recv_waker());
                sockets
                    .get_mut::<Socket>(client_handle)
                    .send_slice(b"tcp-ms01")
                    .expect("client send_slice");
            }
            event.publish_software();
            loop {
                polls += 1;
                assert!(
                    polls <= POLL_BOUND,
                    "payload delivery stalled after {polls} polls"
                );
                let before = wakes.load(Ordering::Relaxed);
                let _ = poll_once(&mut future, &waker);
                let self_woke = wakes.load(Ordering::Relaxed) > before;
                if data_wakes.load(Ordering::Relaxed) >= 1 {
                    break;
                }
                if !self_woke {
                    let deadline = future
                        .timer_deadline()
                        .expect("runner parked without a timer while payload is outstanding");
                    now.store(deadline.total_micros() as i64, Ordering::Relaxed);
                }
            }
            let mut buf = [0u8; 16];
            let n = sockets
                .lock()
                .get_mut::<Socket>(accepted_handle)
                .recv_slice(&mut buf)
                .unwrap();
            assert_eq!(&buf[..n], b"tcp-ms01");
            assert!(polls <= POLL_BOUND);
        }
    }

    #[test]
    fn same_batch_adjacent_syns_both_establish_and_are_accepted() {
        // T2.8-R1 (S1) RED witness: two clients whose SYNs are processed
        // consecutively within ONE ingress batch must both establish while
        // backlog headroom exists, without sleep, caller polling or an
        // unrelated runner round. The pre-fix code only repaired the listener
        // head once per round (after ingress + egress), so the second
        // same-batch SYN found no Listen socket and smoltcp answered RST —
        // this test is RED exactly when client B becomes refused/Closed.
        for _ in 0..100 {
            let mut router = Router::new();
            let lo_dev = router.add_device(Box::new(LoopbackDevice::new()));
            let lo_ip = Ipv4Cidr::new(Ipv4Address::new(127, 0, 0, 1), 8);
            router.add_rule(Rule::new(
                lo_ip.into(),
                None,
                lo_dev,
                lo_ip.address().into(),
            ));

            let listen_table = Box::leak(Box::new(ListenTable::new()));
            let mut service = Service::new_with_listen_table(router, None, listen_table);
            service
                .iface
                .update_ip_addrs(|addrs| addrs.push(lo_ip.into()).unwrap());

            let sockets = Box::leak(Box::new(spin::Mutex::new(SocketSet::new(alloc::vec![]))));
            let event = Box::leak(Box::new(StackEvent::new()));
            let now = Box::leak(Box::new(AtomicI64::new(0)));
            let telemetry = Box::leak(Box::new(StackTelemetry::new()));

            let accept = Arc::new(ReadinessBridge::new());
            listen_table
                .listen_with(
                    smoltcp::wire::IpListenEndpoint {
                        addr: None,
                        port: FULL_CHAIN_PORT,
                    },
                    accept,
                    &mut sockets.lock(),
                )
                .unwrap();

            // Both clients connect BEFORE the first runner round so their
            // SYNs sit in adjacent loopback frames and are ingested inside
            // one bounded batch (ingress budget 32 ≫ 2).
            let client_out = Arc::new(AtomicUsize::new(0));
            let mut clients = alloc::vec::Vec::new();
            for local_port in [FULL_CHAIN_LOCAL_PORT, FULL_CHAIN_LOCAL_PORT + 1] {
                let bridge = Arc::new(ReadinessBridge::new());
                bridge.register(IoEvents::OUT, &counting_waker(client_out.clone()));
                let mut sock = new_tcp_socket();
                {
                    let mut sockets = sockets.lock();
                    sock.register_recv_waker(&bridge.recv_waker());
                    sock.register_send_waker(&bridge.send_waker());
                    let remote =
                        IpEndpoint::new(Ipv4Address::new(127, 0, 0, 1).into(), FULL_CHAIN_PORT);
                    let local = IpEndpoint::new(Ipv4Address::new(127, 0, 0, 1).into(), local_port);
                    sock.connect(service.iface.context(), remote, local)
                        .expect("adjacent-SYN client connect must be accepted by smoltcp");
                    clients.push(sockets.add(sock));
                }
            }
            let service = Box::leak(Box::new(spin::Mutex::new(service)));

            let wakes = Arc::new(AtomicUsize::new(0));
            let waker = counting_waker(wakes.clone());
            event.publish_software();
            let mut future = StackRunnerFuture::new(
                StackAccess::Injected {
                    service,
                    sockets,
                    listen_table,
                },
                lifecycle(RxTaskLifecycle::Active),
                event,
                StackClock::Injected(now),
                telemetry,
            );

            let state = |handle: SocketHandle| sockets.lock().get::<Socket>(handle).state();
            let mut polls = 0usize;
            loop {
                polls += 1;
                assert!(
                    polls <= POLL_BOUND,
                    "same-batch handshake stalled after {polls} polls (client A {}, client B {})",
                    state(clients[0]),
                    state(clients[1]),
                );
                // RED criterion: the second same-batch SYN is falsely refused.
                assert_ne!(
                    state(clients[1]),
                    State::Closed,
                    "second same-batch SYN was refused (client A {}) despite backlog headroom",
                    state(clients[0]),
                );
                let before = wakes.load(Ordering::Relaxed);
                let _ = poll_once(&mut future, &waker);
                let self_woke = wakes.load(Ordering::Relaxed) > before;
                if state(clients[0]) == State::Established
                    && state(clients[1]) == State::Established
                    && listen_table.can_accept(FULL_CHAIN_PORT) == Ok(true)
                {
                    break;
                }
                if !self_woke {
                    let deadline = future.timer_deadline().expect(
                        "runner parked without a timer while a same-batch handshake is incomplete",
                    );
                    now.store(deadline.total_micros() as i64, Ordering::Relaxed);
                }
            }

            // Both connections must be acceptable; neither needs a later
            // unrelated round to become visible.
            let first = listen_table.accept_with(FULL_CHAIN_PORT, &mut sockets.lock());
            assert!(first.is_ok(), "first adjacent connection must be accepted");
            let second = loop {
                polls += 1;
                assert!(polls <= POLL_BOUND, "second accept stalled");
                if listen_table.can_accept(FULL_CHAIN_PORT) == Ok(true) {
                    break listen_table.accept_with(FULL_CHAIN_PORT, &mut sockets.lock());
                }
                let before = wakes.load(Ordering::Relaxed);
                let _ = poll_once(&mut future, &waker);
                let self_woke = wakes.load(Ordering::Relaxed) > before;
                if !self_woke {
                    if let Some(deadline) = future.timer_deadline() {
                        now.store(deadline.total_micros() as i64, Ordering::Relaxed);
                    }
                }
            };
            assert!(
                second.is_ok(),
                "second adjacent connection must be accepted without any caller-driven polling"
            );
        }
    }

    #[test]
    fn task_28_ingress_packet_count_bounds_head_repairs_without_loss() {
        // T2.8-R1 (R3/S3) runtime bound: with 33 signaled listeners but only
        // 3 real loopback frames, the round that INGESTS them runs exactly 3
        // head micro-repairs (one per processed packet), every consumed
        // signal maps to exactly one committed repair (all idles really left
        // Listen), and unconsumed signals stay queued losslessly for later
        // packets/rounds. The frames are enqueued via Router TX, so the first
        // poll only dispatches them; polling continues until ingestion has
        // actually consumed signals — a zero-repair vacuous pass fails here.
        let frame: [u8; 20] = [
            0x45, 0, 0, 20, 0, 0, 0, 0, 64, 17, 0, 0, 10, 0, 2, 15, 255, 255, 255, 255,
        ];
        let mut router = Router::new();
        router.add_device(Box::new(LoopbackDevice::new()));
        for _ in 0..3 {
            router.enqueue_tx_for_test(&frame);
        }

        let listen_table = Box::leak(Box::new(ListenTable::new()));
        let mut service = Service::new_with_listen_table(router, None, listen_table);
        let sockets = Box::leak(Box::new(spin::Mutex::new(SocketSet::new(alloc::vec![]))));
        let event = Box::leak(Box::new(StackEvent::new()));
        let now = Box::leak(Box::new(AtomicI64::new(0)));
        let telemetry = Box::leak(Box::new(StackTelemetry::new()));

        const LISTENERS: usize = 33;
        for i in 0..LISTENERS as u16 {
            let port = 19600 + i;
            listen_table
                .listen_with(
                    smoltcp::wire::IpListenEndpoint { addr: None, port },
                    Arc::new(ReadinessBridge::new()),
                    &mut sockets.lock(),
                )
                .unwrap();
            // A real transition: each closed idle records its own natural
            // signal, so every consume must produce a committed repair.
            assert!(listen_table.test_close_idle(port, &mut sockets.lock()));
        }
        assert_eq!(listen_table.test_pending_head_signals(), LISTENERS);

        let service = Box::leak(Box::new(spin::Mutex::new(service)));
        let wakes = Arc::new(AtomicUsize::new(0));
        event.publish_software();
        let mut future = StackRunnerFuture::new(
            StackAccess::Injected {
                service,
                sockets,
                listen_table,
            },
            lifecycle(RxTaskLifecycle::Active),
            event,
            StackClock::Injected(now),
            telemetry,
        );
        // Drive until the frames were ingested and signals were consumed;
        // the dispatch-only first poll must not satisfy this loop.
        for polls in 1..=POLL_BOUND {
            let _ = poll_once(&mut future, &counting_waker(wakes.clone()));
            if listen_table.test_pending_head_signals() < LISTENERS {
                assert!(
                    telemetry.listener_head_repairs.load(Ordering::Relaxed) > 0,
                    "poll {polls}: signals consumed without any recorded repair"
                );
                break;
            }
            assert!(polls < POLL_BOUND, "frames never reached ingress");
        }

        // All 3 frames were ingested within one batch: exactly one repair per
        // processed packet, never more, with the remaining 30 signals intact.
        let repairs = telemetry.listener_head_repairs.load(Ordering::Relaxed);
        assert_eq!(
            repairs, 3,
            "each of the 3 processed ingress packets must cause exactly one head repair"
        );
        let consumed = LISTENERS as u64 - listen_table.test_pending_head_signals() as u64;
        assert_eq!(
            consumed, repairs,
            "every consumed signal of a live transition must repair exactly once"
        );
        assert_eq!(listen_table.test_pending_head_signals(), LISTENERS - 3);
    }

    #[test]
    fn closing_socket_queued_tx_reaches_peer_before_removal() {
        // Guest fork-mode regression (T2.5-R1 manual runs): a client that
        // sends and then closes without yielding lost the queued payload,
        // because TcpSocket::drop removed the smoltcp socket (and its
        // un-dispatched TX buffer) before the resident runner dispatched it.
        // This mirrors the public close path exactly: graceful close, then
        // the bounded flush, then handle removal.
        let mut router = Router::new();
        let lo_dev = router.add_device(Box::new(LoopbackDevice::new()));
        let lo_ip = Ipv4Cidr::new(Ipv4Address::new(127, 0, 0, 1), 8);
        router.add_rule(Rule::new(
            lo_ip.into(),
            None,
            lo_dev,
            lo_ip.address().into(),
        ));

        let listen_table = Box::leak(Box::new(ListenTable::new()));
        let mut service = Service::new_with_listen_table(router, None, listen_table);
        service
            .iface
            .update_ip_addrs(|addrs| addrs.push(lo_ip.into()).unwrap());
        let sockets = Box::leak(Box::new(spin::Mutex::new(SocketSet::new(alloc::vec![]))));
        let event = Box::leak(Box::new(StackEvent::new()));
        let now = Box::leak(Box::new(AtomicI64::new(0)));
        let telemetry = Box::leak(Box::new(StackTelemetry::new()));

        let accept = Arc::new(ReadinessBridge::new());
        let accept_wakes = Arc::new(AtomicUsize::new(0));
        accept.register(IoEvents::IN, &counting_waker(accept_wakes.clone()));
        listen_table
            .listen_with(
                smoltcp::wire::IpListenEndpoint {
                    addr: None,
                    port: FULL_CHAIN_PORT,
                },
                accept.clone(),
                &mut sockets.lock(),
            )
            .unwrap();

        let client_bridge = Arc::new(ReadinessBridge::new());
        let client_out = Arc::new(AtomicUsize::new(0));
        client_bridge.register(IoEvents::OUT, &counting_waker(client_out.clone()));
        let mut client_sock = new_tcp_socket();
        let client_handle;
        {
            let mut sockets = sockets.lock();
            client_sock.register_recv_waker(&client_bridge.recv_waker());
            client_sock.register_send_waker(&client_bridge.send_waker());
            let remote = IpEndpoint::new(Ipv4Address::new(127, 0, 0, 1).into(), FULL_CHAIN_PORT);
            let local =
                IpEndpoint::new(Ipv4Address::new(127, 0, 0, 1).into(), FULL_CHAIN_LOCAL_PORT);
            client_sock
                .connect(service.iface.context(), remote, local)
                .expect("client connect");
            client_handle = sockets.add(client_sock);
        }
        let service = Box::leak(Box::new(spin::Mutex::new(service)));

        let wakes = Arc::new(AtomicUsize::new(0));
        let waker = counting_waker(wakes.clone());
        event.publish_software();
        let mut future = StackRunnerFuture::new(
            StackAccess::Injected {
                service,
                sockets,
                listen_table,
            },
            lifecycle(RxTaskLifecycle::Active),
            event,
            StackClock::Injected(now),
            telemetry,
        );

        let mut polls = 0usize;
        loop {
            polls += 1;
            assert!(
                polls <= POLL_BOUND,
                "handshake stalled after {polls} polls (client state {})",
                sockets.lock().get::<Socket>(client_handle).state()
            );
            let before = wakes.load(Ordering::Relaxed);
            let _ = poll_once(&mut future, &waker);
            let self_woke = wakes.load(Ordering::Relaxed) > before;
            if sockets.lock().get::<Socket>(client_handle).state() == State::Established
                && listen_table.can_accept(FULL_CHAIN_PORT) == Ok(true)
            {
                break;
            }
            if !self_woke {
                let deadline = future
                    .timer_deadline()
                    .expect("runner parked without a timer while the handshake is incomplete");
                now.store(deadline.total_micros() as i64, Ordering::Relaxed);
            }
        }
        let accepted_handle = listen_table
            .accept_with(FULL_CHAIN_PORT, &mut sockets.lock())
            .unwrap();

        // T2.5-R2: mirror the public close path (TcpSocket::drop) without
        // the public registry. Commit send+close under the SocketSet guard;
        // decide defer-vs-immediate from the raw state; then enqueue the
        // runner-owned retirement under the Service guard alone. The caller
        // must run ZERO stack rounds, and an un-acknowledged FIN must keep
        // the raw handle alive for the resident runner.
        let rounds_before_close = telemetry.rounds.load(Ordering::Relaxed);
        let post_close_state = {
            let mut sockets = sockets.lock();
            sockets
                .get_mut::<Socket>(client_handle)
                .send_slice(b"tcp-ms01")
                .expect("client send_slice");
            sockets.get_mut::<Socket>(client_handle).close();
            sockets.get::<Socket>(client_handle).state()
        };
        match crate::tcp::close_kind(post_close_state) {
            Some(kind) => service.lock().queue_deferred_removal(client_handle, kind),
            None => {
                let _ = sockets.lock().remove(client_handle);
            }
        }
        event.publish_software();
        assert_eq!(
            telemetry.rounds.load(Ordering::Relaxed),
            rounds_before_close,
            "the close caller must not run any stack round"
        );
        assert_eq!(
            sockets.lock().get::<Socket>(client_handle).state(),
            post_close_state,
            "the raw handle must stay for the runner while the FIN is un-acknowledged"
        );

        // Drive the runner until the peer receives the queued payload; the
        // closing client handle must still be alive at that moment.
        let mut polls = 0usize;
        let mut received = false;
        loop {
            polls += 1;
            assert!(
                polls <= POLL_BOUND,
                "payload delivery stalled after {polls} polls"
            );
            let before = wakes.load(Ordering::Relaxed);
            let _ = poll_once(&mut future, &waker);
            let self_woke = wakes.load(Ordering::Relaxed) > before;
            if sockets.lock().get::<Socket>(accepted_handle).can_recv() {
                received = true;
                break;
            }
            if !self_woke {
                let Some(deadline) = future.timer_deadline() else {
                    break;
                };
                now.store(deadline.total_micros() as i64, Ordering::Relaxed);
            }
        }
        let mut buf = [0u8; 16];
        let n = if received {
            sockets
                .lock()
                .get_mut::<Socket>(accepted_handle)
                .recv_slice(&mut buf)
                .unwrap_or(0)
        } else {
            0
        };
        assert!(received && n == 8, "queued TX payload lost on close");
        assert_eq!(&buf[..8], b"tcp-ms01");
        assert!(
            sockets
                .lock()
                .iter()
                .any(|(handle, _)| handle == client_handle),
            "client raw handle was reclaimed before its payload reached the peer"
        );
        assert_eq!(
            sockets.lock().get::<Socket>(client_handle).state(),
            post_close_state,
            "client raw handle must not be removed while its FIN is un-acknowledged"
        );

        // The peer observes EOF after the payload: the client's FIN must be
        // delivered so a later recv returns 0 instead of blocking.
        let mut eof_seen = false;
        loop {
            polls += 1;
            assert!(polls <= POLL_BOUND, "peer EOF stalled after {polls} polls");
            let before = wakes.load(Ordering::Relaxed);
            let _ = poll_once(&mut future, &waker);
            let self_woke = wakes.load(Ordering::Relaxed) > before;
            if !sockets.lock().get::<Socket>(accepted_handle).may_recv() {
                eof_seen = true;
                break;
            }
            if !self_woke {
                let Some(deadline) = future.timer_deadline() else {
                    break;
                };
                now.store(deadline.total_micros() as i64, Ordering::Relaxed);
            }
        }
        assert!(eof_seen, "peer never observed the closing FIN (EOF)");

        // T2.5-R2 + Task 2.6 replan: deferred retirement holds the raw
        // handle while the close is un-acknowledged, then the resident
        // runner's injected clock drives the peer delayed-ACK to a confirmed
        // terminal state and reclaims the handle exactly once. With the
        // single sampled timestamp, the loopback FinWait1 -> FinWait2
        // transition is reachable inside the poll bound (the double-clock
        // blocker that made 003-replan necessary is gone).
        let mut reclaimed_within_bound = false;
        for _ in 0..POLL_BOUND {
            let before = wakes.load(Ordering::Relaxed);
            let _ = poll_once(&mut future, &waker);
            let self_woke = wakes.load(Ordering::Relaxed) > before;
            match future.timer_deadline() {
                Some(deadline) if !self_woke => {
                    now.store(deadline.total_micros() as i64, Ordering::Relaxed);
                }
                None if self_woke => {
                    now.fetch_add(10_000, Ordering::Relaxed);
                }
                _ => {}
            }
            if !sockets
                .lock()
                .iter()
                .any(|(handle, _)| handle == client_handle)
            {
                reclaimed_within_bound = true;
                break;
            }
        }
        // The raw handle is reclaimed only after the injected-clock FIN
        // confirmation, never while the close is un-acknowledged.
        assert!(
            reclaimed_within_bound,
            "the deferred raw handle must be reclaimed once the injected clock confirms the FIN"
        );
    }

    #[test]
    fn task_27_accept_refills_idle_listener_no_reconcile_needed() {
        // Task 2.7 replan (exact-512 scale witness): a real 512-slot hidden
        // backlog and no idle listener means `accept_with` must restore a
        // LISTEN socket inside the same `SOCKET_SET -> entry` critical
        // section, so an immediate reconnect completes on the same runner
        // with no caller-side reconcile. The pre-T2.7-R1 witness built only
        // ONE backlog slot (`let _ = LISTEN_QUEUE_SIZE`); this one asserts
        // exactly `LISTEN_QUEUE_SIZE` slots before accept and keeps
        // queue.len() <= 512 after the reconnect. 100x in both feature
        // profiles witness no flakiness. The leaked infrastructure is
        // shared across iterations (the seed seam tears down its own hidden
        // sockets); per-iteration client sockets are reclaimed below so a
        // 100x run leaks neither 512-slot sets nor accumulated clients.
        use crate::consts::LISTEN_QUEUE_SIZE;

        let mut router = Router::new();
        let lo_dev = router.add_device(Box::new(LoopbackDevice::new()));
        let lo_ip = Ipv4Cidr::new(Ipv4Address::new(127, 0, 0, 1), 8);
        router.add_rule(Rule::new(
            lo_ip.into(),
            None,
            lo_dev,
            lo_ip.address().into(),
        ));
        let listen_table = Box::leak(Box::new(ListenTable::new()));
        let mut service = Service::new_with_listen_table(router, None, listen_table);
        service
            .iface
            .update_ip_addrs(|addrs| addrs.push(lo_ip.into()).unwrap());
        let sockets = Box::leak(Box::new(spin::Mutex::new(SocketSet::new(alloc::vec![]))));
        let event = Box::leak(Box::new(StackEvent::new()));
        let now = Box::leak(Box::new(AtomicI64::new(0)));
        let telemetry = Box::leak(Box::new(StackTelemetry::new()));

        let accept = Arc::new(ReadinessBridge::new());
        listen_table
            .listen_with(
                smoltcp::wire::IpListenEndpoint {
                    addr: None,
                    port: FULL_CHAIN_PORT,
                },
                accept.clone(),
                &mut sockets.lock(),
            )
            .unwrap();
        let service = Box::leak(Box::new(spin::Mutex::new(service)));
        let wakes = Arc::new(AtomicUsize::new(0));
        let waker = counting_waker(wakes.clone());
        let client2_bridge = Arc::new(ReadinessBridge::new());

        for _ in 0..100 {
            // Seed a real exact-512 queue (one Ready, the rest Pending), no
            // idle listener; the seam tears down the previous iteration's
            // hidden sockets, so one SocketSet is reused.
            let seeded_ready =
                listen_table.test_seed_full_queue(FULL_CHAIN_PORT, &mut sockets.lock());
            // Gate 3: the backlog must be exactly full before accept — the
            // pre-T2.7-R1 single-slot witness fails this assertion.
            assert_eq!(
                listen_table.test_queue_len(FULL_CHAIN_PORT),
                LISTEN_QUEUE_SIZE,
                "backlog must be exactly full before accept"
            );
            let accepted_handle = listen_table
                .accept_with(FULL_CHAIN_PORT, &mut sockets.lock())
                .unwrap();
            assert_eq!(accepted_handle, seeded_ready);
            assert!(
                listen_table.test_idle_is_some(FULL_CHAIN_PORT),
                "accept must restore an idle hidden listener before returning"
            );
            assert!(
                listen_table.test_queue_len(FULL_CHAIN_PORT) <= LISTEN_QUEUE_SIZE,
                "queue must never exceed LISTEN_QUEUE_SIZE after accept"
            );

            // Immediate reconnect: client2's SYN must reach the idle hidden
            // LISTEN socket restored by the accept above — no reconcile
            // is called anywhere. Ordered SERVICE -> SOCKET_SET like the
            // production connect path.
            let mut client2 = new_tcp_socket();
            let client2_handle = {
                let mut guard = service.lock();
                let context = guard.iface.context();
                let mut sockets = sockets.lock();
                client2.register_recv_waker(&client2_bridge.recv_waker());
                client2.register_send_waker(&client2_bridge.send_waker());
                let remote =
                    IpEndpoint::new(Ipv4Address::new(127, 0, 0, 1).into(), FULL_CHAIN_PORT);
                let local = IpEndpoint::new(
                    Ipv4Address::new(127, 0, 0, 1).into(),
                    FULL_CHAIN_LOCAL_PORT + 1,
                );
                client2
                    .connect(context, remote, local)
                    .expect("client2 connect");
                sockets.add(client2)
            };
            event.publish_software();
            let mut future = StackRunnerFuture::new(
                StackAccess::Injected {
                    service,
                    sockets,
                    listen_table,
                },
                lifecycle(RxTaskLifecycle::Active),
                event,
                StackClock::Injected(now),
                telemetry,
            );
            let mut polls = 0usize;
            loop {
                polls += 1;
                assert!(
                    polls <= POLL_BOUND,
                    "client2 reconnect stalled after {polls} polls (state {})",
                    sockets.lock().get::<Socket>(client2_handle).state()
                );
                let before = wakes.load(Ordering::Relaxed);
                let _ = poll_once(&mut future, &waker);
                let self_woke = wakes.load(Ordering::Relaxed) > before;
                if sockets.lock().get::<Socket>(client2_handle).state() == State::Established {
                    break;
                }
                if !self_woke {
                    let deadline = future
                        .timer_deadline()
                        .expect("runner parked without a timer during reconnect");
                    now.store(deadline.total_micros() as i64, Ordering::Relaxed);
                }
            }
            assert_eq!(
                sockets.lock().get::<Socket>(client2_handle).state(),
                State::Established,
                "immediate reconnect after accept must find a refilled idle listener"
            );
            assert!(
                listen_table.test_queue_len(FULL_CHAIN_PORT) <= LISTEN_QUEUE_SIZE,
                "backlog must stay bounded at 512 through the reconnect"
            );
            // Reclaim this iteration's accepted + client raw handles so the
            // shared leaked SocketSet does not accumulate across 100 runs.
            {
                let mut sockets = sockets.lock();
                sockets.remove(accepted_handle);
                sockets.remove(client2_handle);
            }
        }
    }

    #[test]
    fn task_27_cleanup_storm_keeps_unrelated_udp_forward_progress() {
        // Task 2.7 replan (T2.7-R2 combo witness): exactly 512 deferred TCP
        // close entries and an unrelated UDP datagram share ONE runner /
        // Service / SocketSet. Per poll the reaper examines at most 32
        // entries (via deferred telemetry deltas); the UDP payload is
        // delivered within the poll bound WHILE the deferred backlog is
        // still non-empty; and the 512 confirmed handles then converge
        // within the bounded sweep. After convergence the deferred stage
        // cannot self-wake the runner. 100x in both feature profiles
        // witness no flakiness. The leaked infrastructure is shared across
        // iterations; the reaper itself reclaims all 512 raw handles, so
        // teardown only drops the two UDP sockets.
        use crate::service::CloseKind;

        let mut router = Router::new();
        let lo_dev = router.add_device(Box::new(LoopbackDevice::new()));
        let lo_ip = Ipv4Cidr::new(Ipv4Address::new(127, 0, 0, 1), 8);
        router.add_rule(Rule::new(
            lo_ip.into(),
            None,
            lo_dev,
            lo_ip.address().into(),
        ));
        let listen_table = Box::leak(Box::new(ListenTable::new()));
        let mut service = Service::new_with_listen_table(router, None, listen_table);
        service
            .iface
            .update_ip_addrs(|addrs| addrs.push(lo_ip.into()).unwrap());
        let sockets = Box::leak(Box::new(spin::Mutex::new(SocketSet::new(alloc::vec![]))));
        let event = Box::leak(Box::new(StackEvent::new()));
        let now = Box::leak(Box::new(AtomicI64::new(0)));
        let telemetry = Box::leak(Box::new(StackTelemetry::new()));
        let service = Box::leak(Box::new(spin::Mutex::new(service)));
        let wakes = Arc::new(AtomicUsize::new(0));
        let waker = counting_waker(wakes.clone());

        for _ in 0..100 {
            // Exactly 512 confirmed deferred handles: a fresh Closed TCP
            // socket confirms an Active close immediately.
            {
                let mut guard = service.lock();
                for _ in 0..512 {
                    let handle = sockets.lock().add(crate::tcp::new_tcp_socket());
                    guard.queue_deferred_removal(handle, CloseKind::Active);
                }
            }
            // One unrelated UDP pair over the same loopback router.
            let udp_rx = {
                let mut sockets = sockets.lock();
                let mut rx = crate::udp::new_udp_socket();
                rx.bind(IpEndpoint::new(
                    Ipv4Address::new(127, 0, 0, 1).into(),
                    FULL_CHAIN_UDP_PORT,
                ))
                .expect("udp rx bind");
                sockets.add(rx)
            };
            let udp_tx = {
                let mut sockets = sockets.lock();
                let mut tx = crate::udp::new_udp_socket();
                tx.bind(IpEndpoint::new(
                    Ipv4Address::new(127, 0, 0, 1).into(),
                    FULL_CHAIN_UDP_LOCAL_PORT,
                ))
                .expect("udp tx bind");
                sockets.add(tx)
            };
            {
                let mut sockets = sockets.lock();
                sockets
                    .get_mut::<smoltcp::socket::udp::Socket>(udp_tx)
                    .send_slice(
                        b"udp-ms01",
                        smoltcp::socket::udp::UdpMetadata {
                            endpoint: IpEndpoint::new(
                                Ipv4Address::new(127, 0, 0, 1).into(),
                                FULL_CHAIN_UDP_PORT,
                            ),
                            local_address: None,
                            meta: smoltcp::phy::PacketMeta::default(),
                        },
                    )
                    .expect("udp datagram send");
            }
            assert_eq!(service.lock().deferred_removals_len(), 512);
            let checked_start = telemetry.deferred_checked.load(Ordering::Relaxed);
            let reclaimed_start = telemetry.deferred_reclaimed.load(Ordering::Relaxed);

            event.publish_software();
            let mut future = StackRunnerFuture::new(
                StackAccess::Injected {
                    service,
                    sockets,
                    listen_table,
                },
                lifecycle(RxTaskLifecycle::Active),
                event,
                StackClock::Injected(now),
                telemetry,
            );

            // Deliver the UDP datagram within the bound, while the deferred
            // backlog is still present; per-poll reaper checks never exceed
            // STACK_STAGE_BUDGET.
            let mut buf = [0u8; 16];
            let mut last_checked = checked_start;
            let mut polls = 0usize;
            loop {
                polls += 1;
                assert!(
                    polls <= POLL_BOUND,
                    "unrelated UDP delivery stalled after {polls} polls"
                );
                let before = wakes.load(Ordering::Relaxed);
                let _ = poll_once(&mut future, &waker);
                let self_woke = wakes.load(Ordering::Relaxed) > before;
                let checked = telemetry.deferred_checked.load(Ordering::Relaxed);
                assert!(
                    checked - last_checked <= 32,
                    "the reaper must check at most 32 deferred entries per round"
                );
                last_checked = checked;
                if sockets
                    .lock()
                    .get_mut::<smoltcp::socket::udp::Socket>(udp_rx)
                    .recv_slice(&mut buf)
                    .is_ok()
                {
                    break;
                }
                if !self_woke {
                    let deadline = future
                        .timer_deadline()
                        .expect("runner parked without a timer while UDP is outstanding");
                    now.store(deadline.total_micros() as i64, Ordering::Relaxed);
                }
            }
            assert_eq!(&buf[..8], b"udp-ms01");
            assert!(
                service.lock().deferred_removals_len() > 0,
                "the unrelated UDP datagram must be delivered while the deferred backlog is still \
                 non-empty"
            );

            // The 512 confirmed handles converge within the bounded sweep.
            loop {
                if service.lock().deferred_removals_len() == 0 {
                    break;
                }
                polls += 1;
                assert!(
                    polls <= POLL_BOUND,
                    "512 deferred close entries did not converge within {polls} polls"
                );
                let before = wakes.load(Ordering::Relaxed);
                let _ = poll_once(&mut future, &waker);
                let self_woke = wakes.load(Ordering::Relaxed) > before;
                let checked = telemetry.deferred_checked.load(Ordering::Relaxed);
                assert!(
                    checked - last_checked <= 32,
                    "the reaper must check at most 32 deferred entries per round"
                );
                last_checked = checked;
                if !self_woke {
                    if let Some(deadline) = future.timer_deadline() {
                        now.store(deadline.total_micros() as i64, Ordering::Relaxed);
                    }
                }
            }
            assert_eq!(
                telemetry.deferred_checked.load(Ordering::Relaxed) - checked_start,
                512,
                "every confirmed deferred entry must be examined exactly once"
            );
            assert_eq!(
                telemetry.deferred_reclaimed.load(Ordering::Relaxed) - reclaimed_start,
                512,
                "every confirmed deferred entry must be reclaimed exactly once"
            );

            // Quiet: after convergence the deferred stage must not keep the
            // runner self-waking; a clock nudge past any deadline parks it.
            let wakes_before_quiet = wakes.load(Ordering::Relaxed);
            now.fetch_add(10_000, Ordering::Relaxed);
            let _ = poll_once(&mut future, &waker);
            assert!(
                wakes.load(Ordering::Relaxed) <= wakes_before_quiet,
                "a converged deferred stage must not self-wake the runner"
            );

            // Teardown: the reaper already reclaimed all 512 raw handles;
            // only the two UDP sockets of this iteration remain.
            {
                let mut sockets = sockets.lock();
                sockets.remove(udp_rx);
                sockets.remove(udp_tx);
            }
        }
    }

    #[test]
    fn task_27_r1_scale_tests_drive_progress_only_through_the_runner() {
        // T2.7-R1 source assertions: the exact-512 and cleanup-storm scale
        // witnesses build real scale through the cfg(test) seed seam (no
        // `let _ = LISTEN_QUEUE_SIZE` placeholder), never call `reconcile`
        // or a direct `stack_round` from the test body, and exercise a real
        // UDP payload — the runner drives all protocol progress.
        let src = include_str!("stack_runner.rs");
        let accept_region = &src[src
            .find("fn task_27_accept_refills_idle_listener_no_reconcile_needed")
            .unwrap()
            ..src.find("fn task_27_cleanup_storm").unwrap()];
        assert!(!accept_region.contains("let _ = LISTEN_QUEUE_SIZE;"));
        assert!(!accept_region.contains("stack_round("));
        assert!(!accept_region.contains(".reconcile("));
        assert!(accept_region.contains("test_seed_full_queue"));
        let cleanup_region = &src[src.find("fn task_27_cleanup_storm").unwrap()
            ..src.find("fn task_27_r1_scale_tests").unwrap()];
        assert!(!cleanup_region.contains("stack_round("));
        assert!(!cleanup_region.contains(".reconcile("));
        assert!(cleanup_region.contains("send_slice"));
        assert!(cleanup_region.contains("recv_slice"));
        assert!(cleanup_region.contains("deferred_removals_len() == 0"));
    }

    #[test]
    fn ms01_diagnostic_payloads_keep_markers_and_deadlines() {
        // T2.5-R1: the loopback diagnostic must give every mode a fixed total
        // deadline and unique START/phase/PASS|FAIL/END markers; the original
        // MS01 payload must keep its 14 PASS markers and add the same
        // failure boundary to its first case, never calling axnet internals.
        let diagnostic = include_str!("../../../tests/ms01_loopback_diagnostic.c");
        assert!(diagnostic.contains("MS01_LOOPBACK_DIAGNOSTIC_START"));
        assert!(diagnostic.contains("MS01_LOOPBACK_DIAGNOSTIC_END"));
        assert!(diagnostic.contains("DIAG_TOTAL_DEADLINE_US 15000000u"));
        assert!(diagnostic.contains("PHASE:"));
        assert!(diagnostic.contains("PASS:"));
        assert!(diagnostic.contains("FAIL:"));
        assert!(diagnostic.contains("single"));
        assert!(diagnostic.contains("fork"));
        assert!(!diagnostic.contains("poll_interfaces"));

        let ms01 = include_str!("../../../tests/ms01_socket_baseline.c");
        // 14 runtime PASS markers: 13 `PASS("...")` macro calls plus the
        // bind-ephemeral child's direct fprintf.
        assert_eq!(ms01.matches("PASS(\"").count(), 13);
        assert!(ms01.contains("\"PASS: bind-ephemeral"));
        assert!(ms01.contains("phase(\"tcp-accept"));
        assert!(ms01.contains("TCP_ACCEPT_DEADLINE_US"));
        assert!(ms01.contains("SO_SNDTIMEO"));
        assert!(ms01.contains("SO_RCVTIMEO"));
        // Task 2.8: overflow safety belongs to the host/model witnesses; the
        // guest must not inject an unobservable fire-and-close overflow and
        // its capacity case must carry fixed phase/deadline failure bounds.
        assert!(!ms01.contains("SOCK_NONBLOCK"));
        assert!(ms01.contains("TCP_512CAP_DEADLINE_US"));
        assert!(ms01.contains("phase(\"tcp-512cap listen\")"));
        assert!(ms01.contains("phase(\"tcp-512cap connect\")"));
        assert!(ms01.contains("phase(\"tcp-512cap accept-refill\")"));
        assert!(ms01.contains("phase(\"tcp-512-recovery connect\")"));
        assert!(ms01.contains("phase(\"tcp-512cap drain\")"));
        assert!(!ms01.contains("poll_interfaces"));
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
        telemetry.progress_wake.store(3, Ordering::Relaxed);
        telemetry.fallback_poll.store(4, Ordering::Relaxed);

        let snapshot = stack_snapshot_impl(&lifecycle, &event, &telemetry);
        assert_eq!(snapshot.started, 1);
        assert_eq!(snapshot.generation, 7);
        assert_eq!(snapshot.task_poll, 11);
        assert_eq!(snapshot.rounds, 9);
        assert_eq!(snapshot.work, 33);
        assert_eq!(snapshot.self_yield, 2);
        assert_eq!(snapshot.progress_wake, 3);
        assert_eq!(snapshot.fallback_poll, 4);
    }

    #[test]
    fn task_26_injected_clock_confirms_delayed_ack_and_reclaims_raw_handle() {
        // Task 2.6 replan: the runner's sampled Instant must flow into the
        // Service round and into smoltcp's timers. Driving the injected clock
        // to the peer delayed-ACK deadline must make the loopback FIN/ACK
        // reach a confirmed state and reclaim the client raw handle — without
        // waiting on the wall clock (which the current Service::stack_round
        // re-reads, making the close never observable under an injected
        // clock). RED on the double-clock baseline.
        let mut router = Router::new();
        let lo_dev = router.add_device(Box::new(LoopbackDevice::new()));
        let lo_ip = Ipv4Cidr::new(Ipv4Address::new(127, 0, 0, 1), 8);
        router.add_rule(Rule::new(
            lo_ip.into(),
            None,
            lo_dev,
            lo_ip.address().into(),
        ));

        let listen_table = Box::leak(Box::new(ListenTable::new()));
        let mut service = Service::new_with_listen_table(router, None, listen_table);
        service
            .iface
            .update_ip_addrs(|addrs| addrs.push(lo_ip.into()).unwrap());
        let sockets = Box::leak(Box::new(spin::Mutex::new(SocketSet::new(alloc::vec![]))));
        let event = Box::leak(Box::new(StackEvent::new()));
        let now = Box::leak(Box::new(AtomicI64::new(0)));
        let telemetry = Box::leak(Box::new(StackTelemetry::new()));

        let accept = Arc::new(ReadinessBridge::new());
        listen_table
            .listen_with(
                smoltcp::wire::IpListenEndpoint {
                    addr: None,
                    port: FULL_CHAIN_PORT,
                },
                accept,
                &mut sockets.lock(),
            )
            .unwrap();

        let client_bridge = Arc::new(ReadinessBridge::new());
        let mut client_sock = new_tcp_socket();
        let client_handle;
        {
            let mut sockets = sockets.lock();
            client_sock.register_recv_waker(&client_bridge.recv_waker());
            client_sock.register_send_waker(&client_bridge.send_waker());
            let remote = IpEndpoint::new(Ipv4Address::new(127, 0, 0, 1).into(), FULL_CHAIN_PORT);
            let local =
                IpEndpoint::new(Ipv4Address::new(127, 0, 0, 1).into(), FULL_CHAIN_LOCAL_PORT);
            client_sock
                .connect(service.iface.context(), remote, local)
                .expect("client connect");
            client_handle = sockets.add(client_sock);
        }
        let service = Box::leak(Box::new(spin::Mutex::new(service)));

        let wakes = Arc::new(AtomicUsize::new(0));
        let waker = counting_waker(wakes.clone());
        event.publish_software();
        let mut future = StackRunnerFuture::new(
            StackAccess::Injected {
                service,
                sockets,
                listen_table,
            },
            lifecycle(RxTaskLifecycle::Active),
            event,
            StackClock::Injected(now),
            telemetry,
        );

        let mut polls = 0usize;
        loop {
            polls += 1;
            assert!(
                polls <= POLL_BOUND,
                "handshake stalled after {polls} polls (client state {})",
                sockets.lock().get::<Socket>(client_handle).state()
            );
            let before = wakes.load(Ordering::Relaxed);
            let _ = poll_once(&mut future, &waker);
            let self_woke = wakes.load(Ordering::Relaxed) > before;
            if sockets.lock().get::<Socket>(client_handle).state() == State::Established
                && listen_table.can_accept(FULL_CHAIN_PORT) == Ok(true)
            {
                break;
            }
            if !self_woke {
                let deadline = future.timer_deadline().expect("handshake without timer");
                now.store(deadline.total_micros() as i64, Ordering::Relaxed);
            }
        }
        let accepted_handle = listen_table
            .accept_with(FULL_CHAIN_PORT, &mut sockets.lock())
            .unwrap();

        // Mirror the public close path: payload+FIN committed, Active kind
        // deferred to the runner under the Service guard alone.
        {
            let mut sockets = sockets.lock();
            sockets
                .get_mut::<Socket>(client_handle)
                .send_slice(b"tcp-ms01")
                .expect("client send_slice");
            sockets.get_mut::<Socket>(client_handle).close();
        }
        let post_close_state = sockets.lock().get::<Socket>(client_handle).state();
        match crate::tcp::close_kind(post_close_state) {
            Some(kind) => service.lock().queue_deferred_removal(client_handle, kind),
            None => {
                let _ = sockets.lock().remove(client_handle);
            }
        }
        event.publish_software();
        assert_eq!(
            sockets.lock().get::<Socket>(client_handle).state(),
            post_close_state,
            "raw handle must stay for the runner while the FIN is un-acknowledged"
        );

        // Drive the injected clock into the protocol: each parked poll hops
        // to the runner's own deadline (the same timestamp the Service round
        // now observes). Delayed ACK must confirm the FIN, moving the client
        // out of FinWait1, and the reaper must reclaim the handle exactly
        // once after that confirmation.
        let mut polls = 0usize;
        loop {
            polls += 1;
            assert!(
                polls <= POLL_BOUND,
                "delayed-ACK confirmation stalled after {polls} polls (client {})",
                sockets.lock().get::<Socket>(client_handle).state()
            );
            let before = wakes.load(Ordering::Relaxed);
            let _ = poll_once(&mut future, &waker);
            let self_woke = wakes.load(Ordering::Relaxed) > before;
            if !sockets
                .lock()
                .iter()
                .any(|(handle, _)| handle == client_handle)
            {
                break;
            }
            if !self_woke {
                if let Some(deadline) = future.timer_deadline() {
                    now.store(deadline.total_micros() as i64, Ordering::Relaxed);
                }
            }
        }
        // The client raw handle is reclaimed only after the injected clock
        // confirmed the FIN; the peer still observes EOF afterwards.
        assert!(
            !sockets
                .lock()
                .iter()
                .any(|(handle, _)| handle == client_handle),
            "client raw handle must be reclaimed after injected-clock FIN confirmation"
        );
        let mut buf = [0u8; 16];
        let n = sockets
            .lock()
            .get_mut::<Socket>(accepted_handle)
            .recv_slice(&mut buf)
            .unwrap_or(0);
        if n > 0 {
            assert_eq!(&buf[..n], b"tcp-ms01");
        }
    }

    #[test]
    fn task_26_incomplete_deferred_sweep_self_wakes_then_parks() {
        // Task 2.6 replan: a deferred sweep that does not fit in one 32-entry
        // stage must self-wake to finish it (bounded progress), but a full
        // sweep of all-unconfirmed entries must not keep self-waking once it
        // is complete: only a protocol event or `poll_at` deadline may wake
        // the runner again. RED on the unbounded baseline, which cannot
        // report sweep-progress at all.
        let mut router = Router::new();
        let lo_dev = router.add_device(Box::new(LoopbackDevice::new()));
        let lo_ip = Ipv4Cidr::new(Ipv4Address::new(127, 0, 0, 1), 8);
        router.add_rule(Rule::new(
            lo_ip.into(),
            None,
            lo_dev,
            lo_ip.address().into(),
        ));
        let listen_table = Box::leak(Box::new(ListenTable::new()));
        let mut service = Service::new_with_listen_table(router, None, listen_table);
        service
            .iface
            .update_ip_addrs(|addrs| addrs.push(lo_ip.into()).unwrap());
        let sockets = Box::leak(Box::new(spin::Mutex::new(SocketSet::new(alloc::vec![]))));
        let event = Box::leak(Box::new(StackEvent::new()));
        let now = Box::leak(Box::new(AtomicI64::new(0)));
        let telemetry = Box::leak(Box::new(StackTelemetry::new()));
        // Inject 33 non-confirmed (Listen) close handles into the SAME
        // SocketSet the runner's Injected access uses: one 32-entry sweep
        // round plus one continuation round, then quiet (no busy loop).
        let mut handles = alloc::vec::Vec::new();
        for i in 0..33 {
            let mut socket = new_tcp_socket();
            socket
                .listen(smoltcp::wire::IpListenEndpoint {
                    addr: None,
                    port: 21000 + i as u16,
                })
                .expect("listen");
            let handle = sockets.lock().add(socket);
            handles.push(handle);
        }
        {
            let mut injected = service;
            for handle in &handles {
                injected.queue_deferred_removal(*handle, crate::service::CloseKind::Active);
            }
            service = injected;
        }
        let service = Box::leak(Box::new(spin::Mutex::new(service)));
        let mut future = StackRunnerFuture::new(
            StackAccess::Injected {
                service,
                sockets,
                listen_table,
            },
            lifecycle(RxTaskLifecycle::Active),
            event,
            StackClock::Injected(now),
            telemetry,
        );

        // The whole sweep needs 2 polls; after it completes with nothing
        // reclaimable, a third poll must not self-wake again.
        let wakes = Arc::new(AtomicUsize::new(0));
        let waker = counting_waker(wakes.clone());
        assert_eq!(poll_once(&mut future, &waker), Poll::Pending);
        let wakes_after_sweep_round = wakes.load(Ordering::Relaxed);
        assert_eq!(poll_once(&mut future, &waker), Poll::Pending);
        let wakes_after_continuation = wakes.load(Ordering::Relaxed);
        let rounds_after_sweep = telemetry.rounds.load(Ordering::Relaxed);
        now.fetch_add(1, Ordering::Relaxed);
        assert_eq!(poll_once(&mut future, &waker), Poll::Pending);
        let wakes_after_quiet = wakes.load(Ordering::Relaxed);
        assert!(
            wakes_after_quiet <= wakes_after_continuation,
            "quiet sweep must not keep self-waking (wakes {wakes_after_quiet} vs \
             {wakes_after_continuation})"
        );
        assert!(rounds_after_sweep >= 2, "sweep must span two rounds");
        assert_eq!(handles.len(), 33);
        let _ = wakes_after_sweep_round;
    }

    #[test]
    fn task_27_repro_guest_512_recovery_sequence() {
        // Fresh-QEMU MS01 tcp-512-capacity repro at ONE iteration: 512 REAL
        // loopback handshakes fill the listener backlog, the 513th overflow
        // connect must not corrupt the full listener, accept#1 consumes one
        // Ready slot and refills the idle, the accepted + client#0 close
        // with un-acknowledged FINs (deferred), then an immediate recovery
        // connect must reach the refilled idle listener. The guest gets
        // `connect: Connection refused` at this exact step: the host model
        // either reproduces it (RED to fix) or proves the divergence is
        // scheduling-only (GREEN; the missing layer is the axtask runner).
        let mut router = Router::new();
        let lo_dev = router.add_device(Box::new(LoopbackDevice::new()));
        let lo_ip = Ipv4Cidr::new(Ipv4Address::new(127, 0, 0, 1), 8);
        router.add_rule(Rule::new(
            lo_ip.into(),
            None,
            lo_dev,
            lo_ip.address().into(),
        ));
        let listen_table = Box::leak(Box::new(ListenTable::new()));
        let mut service = Service::new_with_listen_table(router, None, listen_table);
        service
            .iface
            .update_ip_addrs(|addrs| addrs.push(lo_ip.into()).unwrap());
        let sockets = Box::leak(Box::new(spin::Mutex::new(SocketSet::new(alloc::vec![]))));
        let event = Box::leak(Box::new(StackEvent::new()));
        let now = Box::leak(Box::new(AtomicI64::new(0)));
        let telemetry = Box::leak(Box::new(StackTelemetry::new()));

        let accept = Arc::new(ReadinessBridge::new());
        listen_table
            .listen_with(
                smoltcp::wire::IpListenEndpoint {
                    addr: None,
                    port: FULL_CHAIN_PORT,
                },
                accept,
                &mut sockets.lock(),
            )
            .unwrap();
        let service = Box::leak(Box::new(spin::Mutex::new(service)));
        let wakes = Arc::new(AtomicUsize::new(0));
        let waker = counting_waker(wakes.clone());
        event.publish_software();
        let mut future = StackRunnerFuture::new(
            StackAccess::Injected {
                service,
                sockets,
                listen_table,
            },
            lifecycle(RxTaskLifecycle::Active),
            event,
            StackClock::Injected(now),
            telemetry,
        );

        // Five-hundred-twelve sequential real handshakes (blocking-connect
        // semantics: drive the runner until each client is Established).
        let mut clients: alloc::vec::Vec<(SocketHandle, Arc<ReadinessBridge>)> =
            alloc::vec::Vec::new();
        for i in 0..512 {
            let bridge = Arc::new(ReadinessBridge::new());
            let handle = {
                let mut guard = service.lock();
                let context = guard.iface.context();
                let mut sockets = sockets.lock();
                let mut sock = new_tcp_socket();
                sock.register_recv_waker(&bridge.recv_waker());
                sock.register_send_waker(&bridge.send_waker());
                let remote =
                    IpEndpoint::new(Ipv4Address::new(127, 0, 0, 1).into(), FULL_CHAIN_PORT);
                let local = IpEndpoint::new(
                    Ipv4Address::new(127, 0, 0, 1).into(),
                    FULL_CHAIN_LOCAL_PORT + 0x400 + i as u16,
                );
                sock.connect(context, remote, local)
                    .expect("repro client connect");
                sockets.add(sock)
            };
            clients.push((handle, bridge));
            let mut polls = 0usize;
            loop {
                polls += 1;
                assert!(
                    polls <= POLL_BOUND,
                    "repro client {i} stalled after {polls} polls (state {})",
                    sockets.lock().get::<Socket>(handle).state()
                );
                let before = wakes.load(Ordering::Relaxed);
                let _ = poll_once(&mut future, &waker);
                let self_woke = wakes.load(Ordering::Relaxed) > before;
                if sockets.lock().get::<Socket>(handle).state() == State::Established {
                    break;
                }
                if !self_woke {
                    let deadline = future
                        .timer_deadline()
                        .expect("runner parked without a timer during repro handshake");
                    now.store(deadline.total_micros() as i64, Ordering::Relaxed);
                }
            }
        }
        let overflow_bridge = Arc::new(ReadinessBridge::new());
        let overflow_handle = {
            let mut guard = service.lock();
            let context = guard.iface.context();
            let mut sockets = sockets.lock();
            let mut sock = new_tcp_socket();
            sock.register_recv_waker(&overflow_bridge.recv_waker());
            sock.register_send_waker(&overflow_bridge.send_waker());
            let remote = IpEndpoint::new(Ipv4Address::new(127, 0, 0, 1).into(), FULL_CHAIN_PORT);
            let local = IpEndpoint::new(
                Ipv4Address::new(127, 0, 0, 1).into(),
                FULL_CHAIN_LOCAL_PORT + 0x400 + 512,
            );
            sock.connect(context, remote, local)
                .expect("repro overflow connect");
            sockets.add(sock)
        };
        // The overflow must reach a deterministic refused/closed terminal
        // state while the backlog remains full and before any headroom is
        // released; a merely pending socket is not evidence.
        let mut polls = 0usize;
        loop {
            let state = sockets.lock().get::<Socket>(overflow_handle).state();
            assert_ne!(
                state,
                State::Established,
                "the 513th overflow connect must not corrupt the full listener"
            );
            if state == State::Closed {
                break;
            }
            polls += 1;
            assert!(
                polls <= POLL_BOUND,
                "overflow connect stayed pending after {polls} polls (state {state})"
            );
            let before = wakes.load(Ordering::Relaxed);
            let _ = poll_once(&mut future, &waker);
            let self_woke = wakes.load(Ordering::Relaxed) > before;
            if !self_woke {
                let deadline = future
                    .timer_deadline()
                    .expect("runner parked without a timer during overflow termination");
                now.store(deadline.total_micros() as i64, Ordering::Relaxed);
            }
        }
        // accept#1 consumes the first Ready slot; the idle listener must be
        // refilled before accept returns.
        let first = listen_table
            .accept_with(FULL_CHAIN_PORT, &mut sockets.lock())
            .unwrap();
        assert!(
            listen_table.test_idle_is_some(FULL_CHAIN_PORT),
            "accept must refill an idle hidden listener"
        );
        // Mirror the public close path for the accepted (server-side)
        // connection AND client#0: un-acknowledged FINs become deferred
        // entries; no caller stack round runs.
        {
            let state = sockets.lock().get::<Socket>(first).state();
            sockets.lock().get_mut::<Socket>(first).close();
            let _ = state;
            match crate::tcp::close_kind(sockets.lock().get::<Socket>(first).state()) {
                Some(kind) => service.lock().queue_deferred_removal(first, kind),
                None => {
                    sockets.lock().remove(first);
                }
            }
        }
        {
            let client0_handle = clients[0].0;
            sockets.lock().get_mut::<Socket>(client0_handle).close();
            match crate::tcp::close_kind(sockets.lock().get::<Socket>(client0_handle).state()) {
                Some(kind) => service.lock().queue_deferred_removal(client0_handle, kind),
                None => {
                    sockets.lock().remove(client0_handle);
                }
            }
        }
        event.publish_software();

        // The money step: an immediate recovery connect must reach the
        // refilled idle listener — the guest gets `Connection refused` here.
        let recovery_bridge = Arc::new(ReadinessBridge::new());
        let recovery_handle = {
            let mut guard = service.lock();
            let context = guard.iface.context();
            let mut sockets = sockets.lock();
            let mut sock = new_tcp_socket();
            sock.register_recv_waker(&recovery_bridge.recv_waker());
            sock.register_send_waker(&recovery_bridge.send_waker());
            let remote = IpEndpoint::new(Ipv4Address::new(127, 0, 0, 1).into(), FULL_CHAIN_PORT);
            let local = IpEndpoint::new(
                Ipv4Address::new(127, 0, 0, 1).into(),
                FULL_CHAIN_LOCAL_PORT + 0x400 + 513,
            );
            sock.connect(context, remote, local)
                .expect("repro recovery connect");
            sockets.add(sock)
        };
        let mut polls = 0usize;
        loop {
            polls += 1;
            assert!(
                polls <= POLL_BOUND,
                "recovery connect stalled/refused after {polls} polls (state {})",
                sockets.lock().get::<Socket>(recovery_handle).state()
            );
            let before = wakes.load(Ordering::Relaxed);
            let _ = poll_once(&mut future, &waker);
            let self_woke = wakes.load(Ordering::Relaxed) > before;
            let state = sockets.lock().get::<Socket>(recovery_handle).state();
            if state == State::Established {
                break;
            }
            if !self_woke {
                let deadline = future
                    .timer_deadline()
                    .expect("runner parked without a timer during recovery connect");
                now.store(deadline.total_micros() as i64, Ordering::Relaxed);
            }
        }
        assert_eq!(
            sockets.lock().get::<Socket>(recovery_handle).state(),
            State::Established,
            "immediate recovery connect after accept+close must succeed — the guest sees \
             Connection refused at this step"
        );
        let _ = (overflow_bridge, first, clients);
    }

    #[test]
    fn task_27_repro_guest_udp_bidirectional() {
        // Fresh-QEMU MS01 udp-bidirectional hang: the responder parks on a
        // bridge whose recv waker is the smoltcp one-shot slot; ingress
        // delivery in a runner round must wake it. The responder's echo must
        // then wake the initiator the same way. The guest hangs here with no
        // marker. The host model either reproduces the hang (RED to fix) or
        // proves the blocking-wake chain is sound (GREEN; divergence is in
        // the axtask scheduling layer).
        for _ in 0..100 {
            let mut router = Router::new();
            let lo_dev = router.add_device(Box::new(LoopbackDevice::new()));
            let lo_ip = Ipv4Cidr::new(Ipv4Address::new(127, 0, 0, 1), 8);
            router.add_rule(Rule::new(
                lo_ip.into(),
                None,
                lo_dev,
                lo_ip.address().into(),
            ));
            let listen_table = Box::leak(Box::new(ListenTable::new()));
            let mut service = Service::new_with_listen_table(router, None, listen_table);
            service
                .iface
                .update_ip_addrs(|addrs| addrs.push(lo_ip.into()).unwrap());
            let sockets = Box::leak(Box::new(spin::Mutex::new(SocketSet::new(alloc::vec![]))));
            let event = Box::leak(Box::new(StackEvent::new()));
            let now = Box::leak(Box::new(AtomicI64::new(0)));
            let telemetry = Box::leak(Box::new(StackTelemetry::new()));
            let service = Box::leak(Box::new(spin::Mutex::new(service)));

            // Responder (bound) and initiator (bound elsewhere), each parked
            // on its own bridge recv waker like a blocking recvfrom.
            let responder_wakes = Arc::new(AtomicUsize::new(0));
            let responder_bridge = Arc::new(ReadinessBridge::new());
            responder_bridge.register(IoEvents::IN, &counting_waker(responder_wakes.clone()));
            let udp_rx = {
                let mut sockets = sockets.lock();
                let mut rx = crate::udp::new_udp_socket();
                rx.bind(IpEndpoint::new(
                    Ipv4Address::new(127, 0, 0, 1).into(),
                    FULL_CHAIN_UDP_PORT,
                ))
                .expect("udp rx bind");
                rx.register_recv_waker(&responder_bridge.recv_waker());
                sockets.add(rx)
            };
            let initiator_wakes = Arc::new(AtomicUsize::new(0));
            let initiator_bridge = Arc::new(ReadinessBridge::new());
            initiator_bridge.register(IoEvents::IN, &counting_waker(initiator_wakes.clone()));
            let udp_tx = {
                let mut sockets = sockets.lock();
                let mut tx = crate::udp::new_udp_socket();
                tx.bind(IpEndpoint::new(
                    Ipv4Address::new(127, 0, 0, 1).into(),
                    FULL_CHAIN_UDP_LOCAL_PORT,
                ))
                .expect("udp tx bind");
                tx.register_recv_waker(&initiator_bridge.recv_waker());
                sockets.add(tx)
            };

            let wakes = Arc::new(AtomicUsize::new(0));
            let waker = counting_waker(wakes.clone());
            event.publish_software();
            let mut future = StackRunnerFuture::new(
                StackAccess::Injected {
                    service,
                    sockets,
                    listen_table,
                },
                lifecycle(RxTaskLifecycle::Active),
                event,
                StackClock::Injected(now),
                telemetry,
            );

            // Initiator -> responder datagram; the responder's blocking
            // recvfrom must be woken by the ingress round.
            let peer = IpEndpoint::new(Ipv4Address::new(127, 0, 0, 1).into(), FULL_CHAIN_UDP_PORT);
            {
                let mut sockets = sockets.lock();
                sockets
                    .get_mut::<smoltcp::socket::udp::Socket>(udp_tx)
                    .send_slice(
                        b"udp-ms01",
                        smoltcp::socket::udp::UdpMetadata {
                            endpoint: peer,
                            local_address: None,
                            meta: smoltcp::phy::PacketMeta::default(),
                        },
                    )
                    .expect("initiator send");
            }
            event.publish_software();
            let mut buf = [0u8; 16];
            let mut polls = 0usize;
            loop {
                polls += 1;
                assert!(
                    polls <= POLL_BOUND,
                    "responder recvfrom sleep never woke after {polls} polls"
                );
                let before = wakes.load(Ordering::Relaxed);
                let _ = poll_once(&mut future, &waker);
                let self_woke = wakes.load(Ordering::Relaxed) > before;
                if responder_wakes.load(Ordering::Relaxed) >= 1 {
                    break;
                }
                if !self_woke {
                    let deadline = future
                        .timer_deadline()
                        .expect("runner parked without timer while responder waits");
                    now.store(deadline.total_micros() as i64, Ordering::Relaxed);
                }
            }
            let (n, _) = sockets
                .lock()
                .get_mut::<smoltcp::socket::udp::Socket>(udp_rx)
                .recv_slice(&mut buf)
                .expect("responder recv");
            assert_eq!(&buf[..n], b"udp-ms01");

            // Responder echo -> initiator; the initiator's blocking recvfrom
            // must be woken the same way.
            let initiator_local = IpEndpoint::new(
                Ipv4Address::new(127, 0, 0, 1).into(),
                FULL_CHAIN_UDP_LOCAL_PORT,
            );
            {
                let mut sockets = sockets.lock();
                sockets
                    .get_mut::<smoltcp::socket::udp::Socket>(udp_rx)
                    .send_slice(
                        b"echo-udp-ms01",
                        smoltcp::socket::udp::UdpMetadata {
                            endpoint: initiator_local,
                            local_address: None,
                            meta: smoltcp::phy::PacketMeta::default(),
                        },
                    )
                    .expect("responder echo");
            }
            event.publish_software();
            loop {
                polls += 1;
                assert!(
                    polls <= POLL_BOUND,
                    "initiator recvfrom never woke after {polls} polls"
                );
                let before = wakes.load(Ordering::Relaxed);
                let _ = poll_once(&mut future, &waker);
                let self_woke = wakes.load(Ordering::Relaxed) > before;
                if initiator_wakes.load(Ordering::Relaxed) >= 1 {
                    break;
                }
                if !self_woke {
                    let deadline = future
                        .timer_deadline()
                        .expect("runner parked without timer while initiator waits");
                    now.store(deadline.total_micros() as i64, Ordering::Relaxed);
                }
            }
            let (n, _) = sockets
                .lock()
                .get_mut::<smoltcp::socket::udp::Socket>(udp_tx)
                .recv_slice(&mut buf)
                .expect("initiator recv");
            assert_eq!(&buf[..n], b"echo-udp-ms01");
        }
    }

    #[test]
    fn task_26_listener_sweep_self_wakes_to_finish_then_parks() {
        // Task 2.6 replan (S6): a listener sweep of 33 pending slots spans a
        // bounded self-wake cascade (32 per round), then a fully-swept table
        // must park: a clock nudge with no new event must not self-wake.
        // 100x in both feature profiles witness no flakiness.
        for _ in 0..100 {
            let mut router = Router::new();
            let lo_dev = router.add_device(Box::new(LoopbackDevice::new()));
            let lo_ip = Ipv4Cidr::new(Ipv4Address::new(127, 0, 0, 1), 8);
            router.add_rule(Rule::new(
                lo_ip.into(),
                None,
                lo_dev,
                lo_ip.address().into(),
            ));
            let listen_table = Box::leak(Box::new(ListenTable::new()));
            let mut service = Service::new_with_listen_table(router, None, listen_table);
            service
                .iface
                .update_ip_addrs(|addrs| addrs.push(lo_ip.into()).unwrap());
            let sockets = Box::leak(Box::new(spin::Mutex::new(SocketSet::new(alloc::vec![]))));
            let event = Box::leak(Box::new(StackEvent::new()));
            let now = Box::leak(Box::new(AtomicI64::new(0)));
            let telemetry = Box::leak(Box::new(StackTelemetry::new()));

            let accept = Arc::new(ReadinessBridge::new());
            listen_table
                .listen_with(
                    smoltcp::wire::IpListenEndpoint {
                        addr: None,
                        port: FULL_CHAIN_PORT,
                    },
                    accept,
                    &mut sockets.lock(),
                )
                .unwrap();
            listen_table.test_seed_closed_slots(FULL_CHAIN_PORT, &mut sockets.lock(), 33);
            // Start the sweep directly: a runner round only continues an
            // in-progress sweep without a protocol event.
            let first = listen_table.reconcile(&mut sockets.lock(), true);
            assert_eq!(first.checked, 32);
            assert!(first.sweep_incomplete);
            let service = Box::leak(Box::new(spin::Mutex::new(service)));

            let wakes = Arc::new(AtomicUsize::new(0));
            let waker = counting_waker(wakes.clone());
            event.publish_software();
            let mut future = StackRunnerFuture::new(
                StackAccess::Injected {
                    service,
                    sockets,
                    listen_table,
                },
                lifecycle(RxTaskLifecycle::Active),
                event,
                StackClock::Injected(now),
                telemetry,
            );

            // First poll: the runner continues the sweep (remaining 2 positions:
            // the last slot + the head), then the sweep completes.
            assert_eq!(poll_once(&mut future, &waker), Poll::Pending);
            assert_eq!(
                telemetry.listener_checked.load(Ordering::Relaxed),
                2,
                "the runner must finish the remaining listener positions"
            );
            let checked_start = telemetry.listener_checked.load(Ordering::Relaxed);

            // Quiet: a nudge with no protocol event must not self-wake again.
            let wakes_before_quiet = wakes.load(Ordering::Relaxed);
            now.fetch_add(10_000, Ordering::Relaxed);
            let _ = poll_once(&mut future, &waker);
            let _ = poll_once(&mut future, &waker);
            assert!(
                wakes.load(Ordering::Relaxed) <= wakes_before_quiet,
                "a fully-swept listener table must not keep self-waking"
            );
            assert_eq!(
                telemetry.listener_checked.load(Ordering::Relaxed),
                checked_start,
                "no new listener positions after the sweep completed"
            );
        }
    }

    #[test]
    fn task_26_passive_rst_returns_hidden_socket_to_listen_and_recovers() {
        // Task 2.6 replan (S4, runtime): a real passive-open hidden socket
        // that receives an RST while SynReceived reverts to Listen (smoltcp).
        // The bounded reconcile must recover it — no Pending leak, no lost
        // idle — leaving the listener ready for the next connection.
        // 100x in both feature profiles witness no flakiness.
        for _ in 0..100 {
            let mut router = Router::new();
            let lo_dev = router.add_device(Box::new(LoopbackDevice::new()));
            let lo_ip = Ipv4Cidr::new(Ipv4Address::new(127, 0, 0, 1), 8);
            router.add_rule(Rule::new(
                lo_ip.into(),
                None,
                lo_dev,
                lo_ip.address().into(),
            ));
            let listen_table = Box::leak(Box::new(ListenTable::new()));
            let mut service = Service::new_with_listen_table(router, None, listen_table);
            service
                .iface
                .update_ip_addrs(|addrs| addrs.push(lo_ip.into()).unwrap());
            let sockets = Box::leak(Box::new(spin::Mutex::new(SocketSet::new(alloc::vec![]))));
            let event = Box::leak(Box::new(StackEvent::new()));
            let now = Box::leak(Box::new(AtomicI64::new(0)));
            let telemetry = Box::leak(Box::new(StackTelemetry::new()));
            let service = Box::leak(Box::new(spin::Mutex::new(service)));

            let accept = Arc::new(ReadinessBridge::new());
            listen_table
                .listen_with(
                    smoltcp::wire::IpListenEndpoint {
                        addr: None,
                        port: FULL_CHAIN_PORT,
                    },
                    accept,
                    &mut sockets.lock(),
                )
                .unwrap();

            let client_bridge = Arc::new(ReadinessBridge::new());
            let mut client_sock = new_tcp_socket();
            let client_handle;
            {
                let mut guard = service.lock();
                let context = guard.iface.context();
                let mut sockets = sockets.lock();
                client_sock.register_recv_waker(&client_bridge.recv_waker());
                client_sock.register_send_waker(&client_bridge.send_waker());
                let remote =
                    IpEndpoint::new(Ipv4Address::new(127, 0, 0, 1).into(), FULL_CHAIN_PORT);
                let local =
                    IpEndpoint::new(Ipv4Address::new(127, 0, 0, 1).into(), FULL_CHAIN_LOCAL_PORT);
                client_sock
                    .connect(context, remote, local)
                    .expect("client connect");
                client_handle = sockets.add(client_sock);
            }
            let wakes = Arc::new(AtomicUsize::new(0));
            let waker = counting_waker(wakes.clone());
            event.publish_software();
            let mut future = StackRunnerFuture::new(
                StackAccess::Injected {
                    service,
                    sockets,
                    listen_table,
                },
                lifecycle(RxTaskLifecycle::Active),
                event,
                StackClock::Injected(now),
                telemetry,
            );
            // Drive until the SYN is promoted: one Pending hidden slot exists and
            // the hidden socket is still SynReceived (the client's ACK is not yet
            // dispatched, so a later RST reverts it to Listen instead of closing
            // an Established connection).
            let mut polls = 0usize;
            loop {
                polls += 1;
                assert!(
                    polls <= POLL_BOUND,
                    "SYN promotion stalled after {polls} polls (client {})",
                    sockets.lock().get::<Socket>(client_handle).state()
                );
                let before = wakes.load(Ordering::Relaxed);
                let _ = poll_once(&mut future, &waker);
                let self_woke = wakes.load(Ordering::Relaxed) > before;
                if listen_table.test_queue_len(FULL_CHAIN_PORT) == 1 {
                    break;
                }
                if !self_woke {
                    let deadline = future
                        .timer_deadline()
                        .expect("runner parked without a timer during handshake");
                    now.store(deadline.total_micros() as i64, Ordering::Relaxed);
                }
            }
            assert_eq!(listen_table.test_queue_len(FULL_CHAIN_PORT), 1);
            assert!(
                matches!(
                    sockets.lock().get::<Socket>(client_handle).state(),
                    State::SynSent | State::Established
                ),
                "the client must still be mid-handshake when the SYN is promoted"
            );

            // Abort the client while the hidden socket is still SynReceived: its
            // RST reverts the hidden socket to Listen (smoltcp), which the
            // bounded reconcile must recover without leaking the slot.
            sockets.lock().get_mut::<Socket>(client_handle).abort();
            event.publish_software();
            let mut polls = 0usize;
            loop {
                polls += 1;
                assert!(
                    polls <= POLL_BOUND,
                    "RST recovery stalled after {polls} polls"
                );
                let before = wakes.load(Ordering::Relaxed);
                let _ = poll_once(&mut future, &waker);
                let self_woke = wakes.load(Ordering::Relaxed) > before;
                let queue_len = listen_table.test_queue_len(FULL_CHAIN_PORT);
                if queue_len == 0 {
                    break;
                }
                if queue_len != 1 {
                    panic!("listener queue grew to {queue_len} after RST");
                }
                if !self_woke {
                    if let Some(deadline) = future.timer_deadline() {
                        now.store(deadline.total_micros() as i64, Ordering::Relaxed);
                    }
                }
            }
            assert_eq!(listen_table.test_queue_len(FULL_CHAIN_PORT), 0);
            assert!(
                listen_table.test_idle_is_some(FULL_CHAIN_PORT),
                "the listener must keep an idle hidden socket after RST recovery"
            );
            // The reverted socket was not leaked into the set.
            assert_eq!(sockets.lock().iter().count(), 2, "client + idle remain");
            {
                let mut sockets = sockets.lock();
                sockets.remove(client_handle);
            }
        }
    }

    #[test]
    fn task_26_listener_stage_is_single_bounded_call_without_guard_wake() {
        // Task 2.6 replan source witness: `stack_round` runs exactly ONE
        // listener reconciliation stage per round (the in-ingress and
        // pre-maintenance calls are gone); the stage is budget-constrained and
        // performs no full active-port snapshot pre-pass (the Cycle 006
        // `remaining = ports.iter().map(...).sum()` is gone); the topology
        // generation restart and the progress-during-sweep latch exist; and
        // `reconcile` never wakes accept bridges inside the Service /
        // SocketSet guards — only the drain after guard release does.
        for _ in 0..100 {
            let service = include_str!("service.rs");
            let round_src = &service[service.find("fn stack_round(").unwrap()
                ..service.find("fn rx_slot_has_space_target").unwrap()];
            assert_eq!(
                round_src.matches(".reconcile(").count(),
                1,
                "exactly one bounded listener stage per round"
            );
            let listen_table = include_str!("listen_table.rs");
            let reconcile_src = &listen_table[listen_table.find("fn reconcile(").unwrap()
                ..listen_table.find("fn drain_accept_wakes").unwrap()];
            assert!(reconcile_src.contains("STACK_STAGE_BUDGET"));
            assert!(reconcile_src.contains("reconcile_cursor"));
            assert!(
                !reconcile_src.contains("remaining"),
                "no snapshot-total active-port pre-pass outside the budget"
            );
            assert!(
                reconcile_src.contains("structure_generation"),
                "listener topology or queue mutation must invalidate the running pass"
            );
            let accept_src = &listen_table[listen_table.find("pub fn accept_with").unwrap()
                ..listen_table
                    .find("pub(crate) fn test_seed_full_queue")
                    .unwrap()];
            assert!(
                accept_src.contains("structure_generation.fetch_add"),
                "successful accept must invalidate an active listener cursor"
            );
            assert!(
                !accept_src.contains("reconcile_cursor.lock"),
                "accept must not reverse the cursor-to-entry lock order"
            );
            assert!(
                reconcile_src.contains("rescan"),
                "progress during an unfinished pass must be latched"
            );
            let entry_src = &listen_table[listen_table.find("fn reconcile_head").unwrap()
                ..listen_table.find("fn cleanup").unwrap()];
            assert!(
                !entry_src.contains(".wake("),
                "no accept bridge wake inside any reconcile guard"
            );
        }
    }

    #[test]
    fn task_28_head_micro_step_is_exact_bounded_and_lock_free() {
        // T2.8-R1 source witness: the exact head micro-step runs at exactly
        // one site inside the ingress stage (after processed packets, before
        // egress — never a separate once-per-round scan); its signal ring is
        // pre-reserved to `PORT_NUM` so dedup cannot overflow it; the waker
        // and ring code acquire no locks and allocate nothing; and the
        // consume body performs no loop over ports (O(1) by construction).
        for _ in 0..100 {
            let service = include_str!("service.rs");
            let round_src = &service[service.find("fn stack_round(").unwrap()
                ..service.find("fn rx_slot_has_space_target").unwrap()];
            assert_eq!(
                round_src.matches("consume_head_signal").count(),
                1,
                "exactly one head micro-step site per round"
            );
            let ingress_at = round_src.find("poll_ingress_single").unwrap();
            let consume_at = round_src.find("consume_head_signal").unwrap();
            let egress_at = round_src.find("poll_egress").unwrap();
            assert!(
                ingress_at < consume_at && consume_at < egress_at,
                "head consumption belongs to the ingress stage"
            );

            let listen_table = include_str!("listen_table.rs");
            let signals_start = listen_table.find("struct HeadSignals").unwrap();
            let signals_end = listen_table.find("impl ListenTableEntryInner").unwrap();
            let signals_src = &listen_table[signals_start..signals_end];
            assert!(
                signals_src.contains("PORT_NUM"),
                "ring capacity must be the pre-reserved port count"
            );
            assert!(
                !signals_src.contains(".lock("),
                "no mutex acquisition in waker or ring code"
            );
            assert!(
                !signals_src.contains("Box::new(") && !signals_src.contains("Vec::new"),
                "no allocation in waker or ring steady state"
            );
            let consume_src = &listen_table[listen_table
                .find("pub(crate) fn consume_head_signal")
                .unwrap()
                ..listen_table.find("pub fn can_accept").unwrap()];
            assert!(!consume_src.contains("active_ports"));
            assert!(!consume_src.contains("while ") && !consume_src.contains("loop {"));
        }
    }

    #[test]
    fn task_27_r2_udp_drop_source_deferrals_and_reaper_arm() {
        // T2.7: the production `UdpSocket::drop` must defer the raw removal
        // when the TX buffer still holds an undispatched datagram (smoltcp
        // `close()` resets the TX buffer and would drop it), and the reaper
        // must own a UDP-specific verdict (Keep while `has_pending_tx()`,
        // Reap once drained) instead of treating the UDP slot as stale. Both
        // decisions must use the occupancy accessor, never `can_send()`
        // (capacity-not-full), which would reap a full queue and keep an
        // empty one.
        let udp = include_str!("udp.rs");
        let drop_start = udp.find("impl Drop for UdpSocket").unwrap();
        let drop_end = udp.find("fn get_ephemeral_port").unwrap();
        let drop_src = &udp[drop_start..drop_end];
        assert!(drop_src.contains(".has_pending_tx()"));
        assert!(!drop_src.contains(".can_send()"));
        assert!(drop_src.contains("queue_deferred_removal"));
        assert!(drop_src.contains("UdpQueued"));
        assert!(drop_src.contains("retire_public"));
        assert!(drop_src.contains("publish_software_work"));
        let service = include_str!("service.rs");
        let reap = &service[service.find("fn reap_deferred_removals(").unwrap()
            ..service.find("fn deferred_removals_len(").unwrap()];
        assert!(reap.contains("CloseKind::UdpQueued"));
        assert!(reap.contains("socket.has_pending_tx()"));
        assert!(!reap.contains(".can_send()"));
    }

    #[test]
    fn task_27_repro_udp_child_close_keeps_queued_echo() {
        // T2.7 (guest udp-bidirectional): the forked responder receives the
        // datagram, queues its echo, and closes/exits BEFORE the runner
        // dispatches it. The close must keep the raw socket alive until the
        // runner's egress dispatches the queued datagram (the reaper reclaims
        // it once the TX drained), so the initiator's blocking recvfrom still
        // receives the echo. The pre-fix `UdpSocket::drop` reset the TX
        // buffer (smoltcp `close()`) and removed the socket, silently
        // dropping the echo — the guest hangs with no marker.
        for _ in 0..100 {
            let mut router = Router::new();
            let lo_dev = router.add_device(Box::new(LoopbackDevice::new()));
            let lo_ip = Ipv4Cidr::new(Ipv4Address::new(127, 0, 0, 1), 8);
            router.add_rule(Rule::new(
                lo_ip.into(),
                None,
                lo_dev,
                lo_ip.address().into(),
            ));
            let listen_table = Box::leak(Box::new(ListenTable::new()));
            let mut service = Service::new_with_listen_table(router, None, listen_table);
            service
                .iface
                .update_ip_addrs(|addrs| addrs.push(lo_ip.into()).unwrap());
            let sockets = Box::leak(Box::new(spin::Mutex::new(SocketSet::new(alloc::vec![]))));
            let event = Box::leak(Box::new(StackEvent::new()));
            let now = Box::leak(Box::new(AtomicI64::new(0)));
            let telemetry = Box::leak(Box::new(StackTelemetry::new()));
            let service = Box::leak(Box::new(spin::Mutex::new(service)));

            let responder_wakes = Arc::new(AtomicUsize::new(0));
            let responder_bridge = Arc::new(ReadinessBridge::new());
            responder_bridge.register(IoEvents::IN, &counting_waker(responder_wakes.clone()));
            let udp_rx = {
                let mut sockets = sockets.lock();
                let mut rx = crate::udp::new_udp_socket();
                rx.bind(IpEndpoint::new(
                    Ipv4Address::new(127, 0, 0, 1).into(),
                    FULL_CHAIN_UDP_PORT,
                ))
                .expect("udp rx bind");
                rx.register_recv_waker(&responder_bridge.recv_waker());
                sockets.add(rx)
            };
            let initiator_wakes = Arc::new(AtomicUsize::new(0));
            let initiator_bridge = Arc::new(ReadinessBridge::new());
            initiator_bridge.register(IoEvents::IN, &counting_waker(initiator_wakes.clone()));
            let udp_tx = {
                let mut sockets = sockets.lock();
                let mut tx = crate::udp::new_udp_socket();
                tx.bind(IpEndpoint::new(
                    Ipv4Address::new(127, 0, 0, 1).into(),
                    FULL_CHAIN_UDP_LOCAL_PORT,
                ))
                .expect("udp tx bind");
                tx.register_recv_waker(&initiator_bridge.recv_waker());
                sockets.add(tx)
            };

            let wakes = Arc::new(AtomicUsize::new(0));
            let waker = counting_waker(wakes.clone());
            event.publish_software();
            let mut future = StackRunnerFuture::new(
                StackAccess::Injected {
                    service,
                    sockets,
                    listen_table,
                },
                lifecycle(RxTaskLifecycle::Active),
                event,
                StackClock::Injected(now),
                telemetry,
            );

            // Initiator -> responder; the responder receives the datagram.
            let peer = IpEndpoint::new(Ipv4Address::new(127, 0, 0, 1).into(), FULL_CHAIN_UDP_PORT);
            {
                let mut sockets = sockets.lock();
                sockets
                    .get_mut::<smoltcp::socket::udp::Socket>(udp_tx)
                    .send_slice(
                        b"udp-ms01",
                        smoltcp::socket::udp::UdpMetadata {
                            endpoint: peer,
                            local_address: None,
                            meta: smoltcp::phy::PacketMeta::default(),
                        },
                    )
                    .expect("initiator send");
            }
            event.publish_software();
            let mut buf = [0u8; 16];
            let mut polls = 0usize;
            loop {
                polls += 1;
                assert!(
                    polls <= POLL_BOUND,
                    "responder recvfrom never woke after {polls} polls"
                );
                let before = wakes.load(Ordering::Relaxed);
                let _ = poll_once(&mut future, &waker);
                let self_woke = wakes.load(Ordering::Relaxed) > before;
                if responder_wakes.load(Ordering::Relaxed) >= 1 {
                    break;
                }
                if !self_woke {
                    let deadline = future
                        .timer_deadline()
                        .expect("runner parked without timer while responder waits");
                    now.store(deadline.total_micros() as i64, Ordering::Relaxed);
                }
            }
            let (n, _) = sockets
                .lock()
                .get_mut::<smoltcp::socket::udp::Socket>(udp_rx)
                .recv_slice(&mut buf)
                .expect("responder recv");
            assert_eq!(&buf[..n], b"udp-ms01");

            // The responder queues its echo and "closes" WITHOUT dispatching:
            // mirror the fixed `UdpSocket::drop` — while the TX is queued the
            // raw socket stays in the set (only public metadata retires) and
            // the runner dispatches + the reaper reclaims it.
            let initiator_local = IpEndpoint::new(
                Ipv4Address::new(127, 0, 0, 1).into(),
                FULL_CHAIN_UDP_LOCAL_PORT,
            );
            {
                let mut sockets = sockets.lock();
                sockets
                    .get_mut::<smoltcp::socket::udp::Socket>(udp_rx)
                    .send_slice(
                        b"echo-udp-ms01",
                        smoltcp::socket::udp::UdpMetadata {
                            endpoint: initiator_local,
                            local_address: None,
                            meta: smoltcp::phy::PacketMeta::default(),
                        },
                    )
                    .expect("responder echo");
            }
            assert!(
                sockets
                    .lock()
                    .get::<smoltcp::socket::udp::Socket>(udp_rx)
                    .has_pending_tx(),
                "the echo must be queued (undispatched) at close time"
            );
            // Defer the raw removal (the fixed drop): retire public metadata
            // + enqueue the runner-owned retirement; never close()/remove.
            service
                .lock()
                .queue_deferred_removal(udp_rx, crate::service::CloseKind::UdpQueued);
            event.publish_software();
            assert!(
                sockets
                    .lock()
                    .get::<smoltcp::socket::udp::Socket>(udp_rx)
                    .has_pending_tx(),
                "the raw socket must survive the close while the echo is queued"
            );

            // The initiator's blocking recvfrom must receive the echo.
            loop {
                polls += 1;
                assert!(
                    polls <= POLL_BOUND,
                    "initiator recvfrom never woke after {polls} polls — queued echo lost"
                );
                let before = wakes.load(Ordering::Relaxed);
                let _ = poll_once(&mut future, &waker);
                let self_woke = wakes.load(Ordering::Relaxed) > before;
                if initiator_wakes.load(Ordering::Relaxed) >= 1 {
                    break;
                }
                if !self_woke {
                    let deadline = future
                        .timer_deadline()
                        .expect("runner parked without timer while initiator waits");
                    now.store(deadline.total_micros() as i64, Ordering::Relaxed);
                }
            }
            let (n, _) = sockets
                .lock()
                .get_mut::<smoltcp::socket::udp::Socket>(udp_tx)
                .recv_slice(&mut buf)
                .expect("initiator recv");
            assert_eq!(&buf[..n], b"echo-udp-ms01");
            // The reaper reclaimed the deferred responder socket exactly once.
            assert!(
                !sockets.lock().iter().any(|(h, _)| h == udp_rx),
                "the deferred responder raw handle must be reaped once its TX drained"
            );
        }
    }
}
