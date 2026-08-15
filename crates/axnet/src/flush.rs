//! Target-scoped C4 flush future (D8).
//!
//! A flush captures the driver-buffer ownership line at construction: the
//! `last_accepted` TX ticket at that instant. The future completes once every
//! live ticket `<= target` has been reclaimed (its completion C4), regardless
//! of completion order, and never waits on tickets accepted after the target.
//! It does not mean wire, peer, TCP ACK or application completion.
//!
//! Exactly one internal waiter is allowed. The waiter identity is monotonic
//! so a stale future can never clear a newer waiter's registration.

use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

use axdriver::prelude::DevError;
use embassy_sync::waitqueue::AtomicWaker;

use crate::async_rx::ServiceAccess;

/// The synchronously-captured flush target and its sole waiter identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FlushTicket {
    pub(crate) identity: u64,
    pub(crate) target: Option<u64>,
}

/// Stable error code for a [`DevError`] (mirrors `async_rx::rx_error_code`).
pub(crate) fn error_code(err: &DevError) -> u64 {
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

/// Reconstructs a [`DevError`] from [`error_code`].
pub(crate) fn error_from_code(code: u64) -> DevError {
    match code {
        1 => DevError::AlreadyExists,
        2 => DevError::Again,
        3 => DevError::BadState,
        4 => DevError::InvalidParam,
        5 => DevError::Io,
        6 => DevError::NoMemory,
        7 => DevError::ResourceBusy,
        _ => DevError::Unsupported,
    }
}

/// Single-slot flush waiter state owned by the [`Service`](crate::service::Service).
pub(crate) struct FlushWaiter {
    identity: u64,
    target: Option<u64>,
    waker: AtomicWaker,
    fault: Option<u64>,
}

impl FlushWaiter {
    pub(crate) fn new(identity: u64, target: Option<u64>) -> Self {
        Self {
            identity,
            target,
            waker: AtomicWaker::new(),
            fault: None,
        }
    }

    /// Registers the sole waker, then rechecks under the Service guard.
    pub(crate) fn register(&mut self, waker: &Waker) {
        self.waker.register(waker);
    }

    /// Wake sources: a reclaim that may satisfy the target, or any terminal
    /// submit/reclaim fault. The waker wakes at most once per registration.
    pub(crate) fn wake(&self) {
        self.waker.wake();
    }

    pub(crate) fn identity(&self) -> u64 {
        self.identity
    }

    pub(crate) fn target(&self) -> Option<u64> {
        self.target
    }

    pub(crate) fn take_fault_code(&mut self) -> Option<u64> {
        self.fault.take()
    }

    pub(crate) fn set_fault(&mut self, err: &DevError) {
        self.fault = Some(error_code(err));
        self.waker.wake();
    }
}

/// Outcome of one flush recheck under the Service guard.
#[derive(Debug)]
pub(crate) enum FlushRecheck {
    /// Every live ticket `<= target` has been reclaimed.
    Done,
    /// A terminal submit/reclaim fault was recorded.
    Faulted(DevError),
    /// The target is not yet satisfied; the future must sleep.
    Pending,
    /// The waiter identity no longer owns the slot (stale future).
    Stale,
}

/// A target-scoped C4 flush future.
///
/// Construction synchronously captures the target; every poll registers the
/// sole waker and rechecks the live set under the same Service guard, then
/// releases the guard before returning `Pending`. Dropping the future clears
/// only its own waiter registration and never changes packet ownership.
pub struct FlushFuture {
    service: ServiceAccess,
    identity: u64,
    target: Option<u64>,
}

/// Reserves the sole flush waiter and returns a future bound to `service`.
///
/// `ResourceBusy` means a flush is already in flight; the caller must retry
/// after the current one completes or is dropped.
pub(crate) fn flush_new(service: ServiceAccess) -> Result<FlushFuture, DevError> {
    let Some(mut guard) = service.try_lock() else {
        return Err(DevError::ResourceBusy);
    };
    let ticket = guard.flush_begin()?;
    Ok(FlushFuture {
        service,
        identity: ticket.identity,
        target: ticket.target,
    })
}

impl FlushFuture {
    fn poll_impl(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), DevError>> {
        let Some(mut service) = self.service.try_lock() else {
            return Poll::Pending;
        };
        service.flush_register(self.identity, cx.waker());
        match service.flush_recheck(self.identity, self.target) {
            FlushRecheck::Done => Poll::Ready(Ok(())),
            FlushRecheck::Faulted(err) => Poll::Ready(Err(err)),
            FlushRecheck::Pending => {
                drop(service);
                Poll::Pending
            }
            FlushRecheck::Stale => {
                service.flush_clear(self.identity);
                Poll::Ready(Err(DevError::BadState))
            }
        }
    }
}

impl Future for FlushFuture {
    type Output = Result<(), DevError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // The future is !Unpin-free state that never borrows itself, so the
        // pinned projection is safe.
        self.get_mut().poll_impl(cx)
    }
}

impl Drop for FlushFuture {
    fn drop(&mut self) {
        if let Some(mut service) = self.service.try_lock() {
            service.flush_clear(self.identity);
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, collections::VecDeque, sync::Arc};
    use core::{
        sync::atomic::{AtomicUsize, Ordering},
        task::Waker,
    };

    use axdriver::prelude::DevError;
    use smoltcp::{storage::PacketBuffer, time::Instant, wire::IpAddress};

    use super::{FlushFuture, flush_new};
    use crate::{
        async_rx::ServiceAccess,
        device::{
            Device, RxStep, TxOutcome, TxPreflight, TxReclaimStep, TxSubmitStep,
            fixed_queue::TicketTracker,
        },
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

    /// Scripted ticket ledger shared between the fake device and the test.
    struct FlushInner {
        tracker: spin::Mutex<TicketTracker>,
        queued: spin::Mutex<VecDeque<u64>>,
        reclaim_cookies: spin::Mutex<VecDeque<u64>>,
    }

    impl FlushInner {
        fn new() -> Self {
            Self {
                tracker: spin::Mutex::new(TicketTracker::new()),
                queued: spin::Mutex::new(VecDeque::new()),
                reclaim_cookies: spin::Mutex::new(VecDeque::new()),
            }
        }
    }

    /// A scripted device whose ticket ledger is a real [`TicketTracker`].
    ///
    /// `send` allocates a Queued ticket (mirroring the dormant Ethernet emit),
    /// `tx_submit_one` transitions the oldest Queued ticket to DeviceOwned, and
    /// `tx_reclaim_one` replays scripted completion cookies.
    struct FlushDevice {
        inner: Arc<FlushInner>,
    }

    impl FlushDevice {
        fn new() -> (Self, Arc<FlushInner>) {
            let inner = Arc::new(FlushInner::new());
            (
                Self {
                    inner: inner.clone(),
                },
                inner,
            )
        }
    }

    impl Device for FlushDevice {
        fn name(&self) -> &str {
            "flush-scripted"
        }

        fn recv(&mut self, _buffer: &mut PacketBuffer<()>, _timestamp: Instant) -> RxStep {
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
            match self.inner.tracker.lock().alloc() {
                Ok(ticket) => {
                    self.inner.queued.lock().push_back(ticket);
                    TxOutcome::Accepted {
                        rx_became_ready: false,
                    }
                }
                Err(_) => TxOutcome::Full,
            }
        }

        fn register_waker(&self, _waker: &Waker) {}

        fn tx_last_accepted(&self) -> Option<u64> {
            self.inner.tracker.lock().last_accepted()
        }

        fn tx_flush_done(&self, target: Option<u64>) -> bool {
            self.inner.tracker.lock().flush_done(target)
        }

        fn slot_ledger(&self) -> crate::device::SlotLedger {
            let tracker = self.inner.tracker.lock();
            crate::device::SlotLedger {
                rx_occupancy: 0,
                rx_high_water: 0,
                rx_full: 0,
                rx_enqueue: 0,
                rx_dequeue: 0,
                rx_space_event: 0,
                tx_occupancy: self.inner.queued.lock().len() as u64,
                tx_high_water: 0,
                tx_full: 0,
                tx_enqueue: 0,
                tx_dequeue: 0,
                tx_space_event: 0,
                live: tracker.live_len() as u64,
                queued: tracker.queued_len() as u64,
                device_owned: tracker.device_owned_len() as u64,
                last_accepted: tracker.last_accepted().unwrap_or(u64::MAX),
            }
        }

        fn tx_submit_one(&mut self) -> TxSubmitStep {
            let Some(ticket) = self.inner.queued.lock().pop_front() else {
                return TxSubmitStep::Empty;
            };
            if self.inner.tracker.lock().mark_device_owned(ticket) {
                TxSubmitStep::Submitted
            } else {
                TxSubmitStep::Fault(DevError::BadState)
            }
        }

        fn tx_reclaim_one(&mut self) -> TxReclaimStep {
            let Some(cookie) = self.inner.reclaim_cookies.lock().pop_front() else {
                return TxReclaimStep::Empty;
            };
            if self.inner.tracker.lock().release_device_owned(cookie) {
                TxReclaimStep::Reclaimed
            } else {
                TxReclaimStep::Fault(DevError::BadState)
            }
        }
    }

    /// Leaks a Service over a scripted flush device; returns the shared ledger.
    fn leaked_service() -> (&'static spin::Mutex<Service>, Arc<FlushInner>) {
        let (dev, inner) = FlushDevice::new();
        let mut router = Router::new();
        let idx = router.add_device(Box::new(dev));
        let service = Service::new(router, Some(idx));
        (Box::leak(Box::new(spin::Mutex::new(service))), inner)
    }

    fn poll_once(fut: &mut FlushFuture) -> core::task::Poll<Result<(), DevError>> {
        let waker = counting_waker(Arc::new(AtomicUsize::new(0)));
        let mut cx = core::task::Context::from_waker(&waker);
        core::pin::Pin::new(fut).poll(&mut cx)
    }

    #[test]
    fn flush_empty_data_plane_completes_immediately() {
        let (service, _inner) = leaked_service();
        let mut fut = flush_new(ServiceAccess::Injected(service)).unwrap();
        assert!(matches!(
            poll_once(&mut fut),
            core::task::Poll::Ready(Ok(()))
        ));
        // A completed flush releases the sole waiter slot.
        assert!(flush_new(ServiceAccess::Injected(service)).is_ok());
    }

    #[test]
    fn flush_pends_until_queued_and_device_owned_tickets_reclaimed() {
        let (service, inner) = leaked_service();
        for _ in 0..3 {
            let ticket = inner.tracker.lock().alloc().unwrap();
            inner.queued.lock().push_back(ticket);
        }
        let mut fut = flush_new(ServiceAccess::Injected(service)).unwrap();
        assert!(matches!(poll_once(&mut fut), core::task::Poll::Pending));

        // Submitting the queued slots keeps the tickets live.
        {
            let mut guard = service.lock();
            let dev = &mut guard.router_for_test().devices[0];
            for _ in 0..3 {
                assert!(matches!(dev.tx_submit_one(), TxSubmitStep::Submitted));
            }
        }
        assert!(matches!(poll_once(&mut fut), core::task::Poll::Pending));

        // Reclaiming all three DeviceOwned tickets completes the flush.
        {
            let mut guard = service.lock();
            let dev = &mut guard.router_for_test().devices[0];
            for cookie in [0, 1, 2] {
                inner.reclaim_cookies.lock().push_back(cookie);
                assert!(matches!(dev.tx_reclaim_one(), TxReclaimStep::Reclaimed));
            }
        }
        assert!(matches!(
            poll_once(&mut fut),
            core::task::Poll::Ready(Ok(()))
        ));
    }

    #[test]
    fn flush_out_of_order_hole_blocks_until_all_target_tickets_reclaimed() {
        let (service, inner) = leaked_service();
        for _ in 0..3 {
            let ticket = inner.tracker.lock().alloc().unwrap();
            inner.queued.lock().push_back(ticket);
        }
        let mut fut = flush_new(ServiceAccess::Injected(service)).unwrap();
        {
            let mut guard = service.lock();
            let dev = &mut guard.router_for_test().devices[0];
            for _ in 0..3 {
                dev.tx_submit_one();
            }
            // Reclaim out of order: 2 then 0 leaves a hole at 1.
            for cookie in [2, 0] {
                inner.reclaim_cookies.lock().push_back(cookie);
                dev.tx_reclaim_one();
            }
        }
        assert!(matches!(poll_once(&mut fut), core::task::Poll::Pending));
        {
            let mut guard = service.lock();
            let dev = &mut guard.router_for_test().devices[0];
            inner.reclaim_cookies.lock().push_back(1);
            dev.tx_reclaim_one();
        }
        assert!(matches!(
            poll_once(&mut fut),
            core::task::Poll::Ready(Ok(()))
        ));
    }

    #[test]
    fn flush_post_target_tickets_do_not_block() {
        let (service, inner) = leaked_service();
        for _ in 0..2 {
            let ticket = inner.tracker.lock().alloc().unwrap();
            inner.queued.lock().push_back(ticket);
        }
        // Target is ticket 1; ticket 2 is accepted after construction.
        let mut fut = flush_new(ServiceAccess::Injected(service)).unwrap();
        {
            let mut guard = service.lock();
            let dev = &mut guard.router_for_test().devices[0];
            let ticket = inner.tracker.lock().alloc().unwrap();
            inner.queued.lock().push_back(ticket);
            for _ in 0..3 {
                dev.tx_submit_one();
            }
            for cookie in [0, 1] {
                inner.reclaim_cookies.lock().push_back(cookie);
                dev.tx_reclaim_one();
            }
        }
        // Ticket 2 stays live but is after the target.
        assert!(matches!(
            poll_once(&mut fut),
            core::task::Poll::Ready(Ok(()))
        ));
    }

    #[test]
    fn flush_fatal_wakes_and_returns_stable_error() {
        let (service, inner) = leaked_service();
        let ticket = inner.tracker.lock().alloc().unwrap();
        inner.queued.lock().push_back(ticket);
        {
            let mut guard = service.lock();
            let dev = &mut guard.router_for_test().devices[0];
            dev.tx_submit_one();
        }
        let mut fut = flush_new(ServiceAccess::Injected(service)).unwrap();
        // An unknown reclaim cookie is an ownership fatal.
        {
            let mut guard = service.lock();
            guard.flush_fault(&DevError::BadState);
        }
        assert!(matches!(
            poll_once(&mut fut),
            core::task::Poll::Ready(Err(DevError::BadState))
        ));
    }

    #[test]
    fn flush_second_waiter_is_resource_busy() {
        let (service, _inner) = leaked_service();
        let _fut = flush_new(ServiceAccess::Injected(service)).unwrap();
        let second = flush_new(ServiceAccess::Injected(service));
        assert!(matches!(second, Err(DevError::ResourceBusy)));
    }

    #[test]
    fn flush_after_terminal_fault_returns_stable_error_even_without_waiter() {
        // RW-3: a terminal data-plane fault must persist in the Service so a
        // flush constructed AFTER the fault returns the same stable error,
        // instead of waiting forever on a live target with no owner to wake it.
        let (service, inner) = leaked_service();
        // A live target stays behind: the flush would otherwise see nothing to
        // reclaim, but the fault must win.
        let ticket = inner.tracker.lock().alloc().unwrap();
        inner.queued.lock().push_back(ticket);
        // Fault recorded while no flush waiter exists (owner is already gone).
        {
            let mut guard = service.lock();
            guard.flush_fault(&DevError::Io);
        }
        // A later flush constructor must fail fast with the persisted error.
        let err = flush_new(ServiceAccess::Injected(service));
        assert!(matches!(err, Err(DevError::Io)));
        // ... and every subsequent flush must return the same stable error.
        let err = flush_new(ServiceAccess::Injected(service));
        assert!(matches!(err, Err(DevError::Io)));
        // The waiter slot is not consumed by the rejected constructions.
        assert!(flush_new(ServiceAccess::Injected(service)).is_err());
    }

    #[test]
    fn flush_terminal_fault_persists_after_first_waiter_consumes_it() {
        // RW-3: when a flush waiter consumes the fault, the stable error must
        // remain for the next flush, not be cleared with the waiter.
        let (service, inner) = leaked_service();
        let ticket = inner.tracker.lock().alloc().unwrap();
        inner.queued.lock().push_back(ticket);
        {
            let mut guard = service.lock();
            let dev = &mut guard.router_for_test().devices[0];
            dev.tx_submit_one();
        }
        // A flush is already in flight when the terminal fault lands.
        let mut fut = flush_new(ServiceAccess::Injected(service)).unwrap();
        {
            let mut guard = service.lock();
            guard.flush_fault(&DevError::BadState);
        }
        // The in-flight waiter observes the fault and returns it.
        assert!(matches!(
            poll_once(&mut fut),
            core::task::Poll::Ready(Err(DevError::BadState))
        ));
        // The persisted fault is still visible to a later flush.
        let err = flush_new(ServiceAccess::Injected(service));
        assert!(matches!(err, Err(DevError::BadState)));
    }

    #[test]
    fn flush_waiter_identity_exhaustion_returns_stable_error_without_wrap() {
        // RW-3: the waiter identity allocator must be checked. Exhaustion must
        // fail the construction without reusing an identity (ABA), and must
        // not consume the waiter slot.
        let (service, _inner) = leaked_service();
        {
            let mut guard = service.lock();
            // Drive the identity counter to the last valid value.
            guard.set_flush_next_identity_for_test(u64::MAX - 1);
        }
        // The last valid identity is handed out once.
        let _fut = flush_new(ServiceAccess::Injected(service)).unwrap();
        // Exhaustion: `u64::MAX` is the invalid sentinel, so the next
        // construction fails fast instead of wrapping back to 0.
        let err = flush_new(ServiceAccess::Injected(service));
        assert!(matches!(err, Err(DevError::ResourceBusy)));
        // No identity was reused: the counter stayed at the sentinel.
        {
            let mut guard = service.lock();
            assert_eq!(guard.flush_next_identity_for_test(), u64::MAX);
        }
    }

    #[test]
    fn flush_drop_releases_waiter_without_changing_packet_ownership() {
        let (service, inner) = leaked_service();
        let ticket = inner.tracker.lock().alloc().unwrap();
        inner.queued.lock().push_back(ticket);
        {
            let mut guard = service.lock();
            let dev = &mut guard.router_for_test().devices[0];
            dev.tx_submit_one();
        }
        {
            let _fut = flush_new(ServiceAccess::Injected(service)).unwrap();
        }
        // The sole waiter slot is free again...
        assert!(flush_new(ServiceAccess::Injected(service)).is_ok());
        // ...and the live ticket was never released by the flush Drop.
        assert!(!inner.tracker.lock().flush_done(Some(0)));
    }

    #[test]
    fn flush_register_recheck_sees_reclaim_before_first_poll() {
        let (service, inner) = leaked_service();
        let ticket = inner.tracker.lock().alloc().unwrap();
        inner.queued.lock().push_back(ticket);
        {
            let mut guard = service.lock();
            let dev = &mut guard.router_for_test().devices[0];
            dev.tx_submit_one();
            inner.reclaim_cookies.lock().push_back(0);
            dev.tx_reclaim_one();
        }
        // The target is already satisfied before the future is first polled:
        // the register-then-recheck must complete without sleeping.
        let mut fut = flush_new(ServiceAccess::Injected(service)).unwrap();
        assert!(matches!(
            poll_once(&mut fut),
            core::task::Poll::Ready(Ok(()))
        ));
    }

    #[test]
    fn v3_ledger_reports_tickets_and_slots_under_the_guard() {
        let (service, inner) = leaked_service();
        // Two queued tickets, one submitted -> DeviceOwned.
        for _ in 0..2 {
            let ticket = inner.tracker.lock().alloc().unwrap();
            inner.queued.lock().push_back(ticket);
        }
        {
            let mut guard = service.lock();
            let dev = &mut guard.router_for_test().devices[0];
            dev.tx_submit_one();
        }
        let mut guard = service.lock();
        let ledger = guard.v3_slot_ledger();
        assert_eq!(ledger.live, 2);
        assert_eq!(ledger.queued, 1);
        assert_eq!(ledger.device_owned, 1);
        assert_eq!(ledger.last_accepted, 1);
        assert_eq!(guard.v3_flush_target(), u64::MAX);
        assert_eq!(guard.v3_flush_counters(), [0, 0, 0, 0]);
    }

    #[test]
    fn v3_flush_counters_track_success_busy_and_cancel() {
        let (service, inner) = leaked_service();
        let ticket = inner.tracker.lock().alloc().unwrap();
        inner.queued.lock().push_back(ticket);
        {
            let mut guard = service.lock();
            let dev = &mut guard.router_for_test().devices[0];
            dev.tx_submit_one();
        }
        {
            let fut = flush_new(ServiceAccess::Injected(service)).unwrap();
            // A second concurrent flush is ResourceBusy.
            assert!(matches!(
                flush_new(ServiceAccess::Injected(service)),
                Err(DevError::ResourceBusy)
            ));
            drop(fut);
        }
        {
            let mut guard = service.lock();
            // One busy rejection, one cancel (drop before completion).
            assert_eq!(guard.v3_flush_counters(), [0, 0, 1, 1]);
        }
        // A completed flush counts a success.
        {
            let mut guard = service.lock();
            let dev = &mut guard.router_for_test().devices[0];
            inner.reclaim_cookies.lock().push_back(0);
            dev.tx_reclaim_one();
        }
        {
            let mut fut = flush_new(ServiceAccess::Injected(service)).unwrap();
            assert!(matches!(
                poll_once(&mut fut),
                core::task::Poll::Ready(Ok(()))
            ));
        }
        let mut guard = service.lock();
        assert_eq!(guard.v3_flush_counters(), [1, 0, 1, 1]);
    }
}
