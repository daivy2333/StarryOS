use alloc::boxed::Box;
use core::{
    pin::Pin,
    task::{Context, Waker},
};

use axdriver::prelude::{DevError, DevResult};
use axhal::time::{NANOS_PER_MICROS, TimeValue, wall_time_nanos};
use axtask::future::sleep_until;
use smoltcp::{
    iface::{Interface, PollIngressSingleResult, PollResult, SocketSet},
    time::{Duration, Instant},
    wire::{HardwareAddress, IpAddress, IpListenEndpoint},
};

use crate::{
    LISTEN_TABLE, SOCKET_SET,
    async_rx::{RX_NOTIFY, SpaceDecision},
    router::{Router, RxOutcome, RxOwnerView},
};

const POLLING_FALLBACK: Duration = Duration::from_millis(10);

fn now() -> Instant {
    Instant::from_micros_const((wall_time_nanos() / NANOS_PER_MICROS) as i64)
}

fn select_wake_deadline(
    protocol_deadline: Option<Instant>,
    polling_deadline: Option<Instant>,
) -> Option<Instant> {
    match (protocol_deadline, polling_deadline) {
        (Some(protocol), Some(polling)) => Some(protocol.min(polling)),
        (Some(protocol), None) => Some(protocol),
        (None, Some(polling)) => Some(polling),
        (None, None) => None,
    }
}

/// `polling_capabilities` yields one `requires_polling()` result per device,
/// where bit `i` in `mask` selects device `i`.
fn any_masked_device_requires_polling(
    mask: u32,
    polling_capabilities: impl IntoIterator<Item = bool>,
) -> bool {
    polling_capabilities
        .into_iter()
        .enumerate()
        .any(|(i, requires_polling)| mask & (1 << i) != 0 && requires_polling)
}

pub struct Service {
    pub iface: Interface,
    router: Router,
    target_dev: Option<usize>,
    timeout: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
}
impl Service {
    pub fn new(mut router: Router, target_dev: Option<usize>) -> Self {
        let config = smoltcp::iface::Config::new(HardwareAddress::Ip);
        let iface = Interface::new(config, &mut router, now());

        Self {
            iface,
            router,
            target_dev,
            timeout: None,
        }
    }

    pub fn poll(&mut self, owner: RxOwnerView, sockets: &mut SocketSet) -> bool {
        let timestamp = now();
        let mut changed = false;

        self.router.poll(owner, self.target_dev, timestamp);
        self.iface.poll_maintenance(timestamp);
        LISTEN_TABLE.reconcile(sockets);
        loop {
            match self
                .iface
                .poll_ingress_single(timestamp, &mut self.router, sockets)
            {
                PollIngressSingleResult::None => break,
                PollIngressSingleResult::PacketProcessed => {}
                PollIngressSingleResult::SocketStateChanged => changed = true,
            }
            LISTEN_TABLE.reconcile(sockets);
        }
        loop {
            match self.iface.poll_egress(timestamp, &mut self.router, sockets) {
                PollResult::None => break,
                PollResult::SocketStateChanged => changed = true,
            }
        }
        LISTEN_TABLE.reconcile(sockets);
        RX_NOTIFY.wake_if_space(self.router.rx_buffer_has_space());
        self.router.dispatch(timestamp) || changed
    }

    /// RX-only one-step for the stored target device.
    ///
    /// Uses only the saved target index and generates the timestamp from the
    /// current time internally, so the caller never passes a raw device index,
    /// never obtains a second NIC handle and never copies the `now()`
    /// conversion. A missing target maps to `Fault(BadState)`.
    pub(crate) fn rx_one_step_target(&mut self) -> RxOutcome {
        let Some(dev) = self.target_dev else {
            return RxOutcome::Fault(DevError::BadState);
        };
        self.router.rx_one_step(dev, now())
    }

    fn target_index(&self) -> DevResult<usize> {
        self.target_dev.ok_or(DevError::BadState)
    }

    /// Activation-time preflight on the stored target: queue control must
    /// exist and accept suppression; no completion is reaped.
    pub(crate) fn rx_preflight_target(&mut self) -> DevResult {
        self.router.rx_control_preflight(self.target_index()?)
    }

    /// Suppresses RX notifications on the stored target device.
    pub(crate) fn rx_suppress_target(&mut self) -> DevResult {
        self.router.rx_control_suppress(self.target_index()?)
    }

    /// Rearms RX notifications on the stored target and reports whether a
    /// completion is still pending after the transport barrier.
    pub(crate) fn rx_arm_and_check_target(&mut self) -> DevResult<bool> {
        self.router.rx_control_arm_and_check(self.target_index()?)
    }

    /// Returns whether the stored target currently sees an RX completion.
    pub(crate) fn rx_completion_visible_target(&mut self) -> DevResult<bool> {
        self.router.rx_control_has_completion(self.target_index()?)
    }

    /// Full-space recheck, callable only while holding the Service guard.
    ///
    /// Returns `Retry` when space is already available; otherwise publishes
    /// the waiting bit (Release) and returns `Waiting`.
    pub(crate) fn rx_space_recheck_or_wait(&self) -> SpaceDecision {
        if self.router.rx_buffer_has_space() {
            SpaceDecision::Retry
        } else {
            RX_NOTIFY.publish_waiting();
            SpaceDecision::Waiting
        }
    }

    #[cfg(test)]
    pub(crate) fn fill_rx_buffer_for_test(&mut self) {
        self.router.fill_rx_buffer_for_test();
    }

    pub fn get_source_address(&self, dst_addr: &IpAddress) -> IpAddress {
        let Some(rule) = self.router.table.lookup(dst_addr) else {
            panic!("no route to destination: {dst_addr}");
        };
        rule.src
    }

    pub fn device_mask_for(&self, endpoint: &IpListenEndpoint) -> u32 {
        match endpoint.addr {
            Some(addr) => self
                .router
                .table
                .lookup(&addr)
                .map_or(0, |it| 1u32 << it.dev),
            None => u32::MAX,
        }
    }

    pub fn register_waker(&mut self, mask: u32, waker: &Waker) {
        let timestamp = now();
        let protocol_deadline = self.iface.poll_at(timestamp, &SOCKET_SET.inner.lock());
        let polling_deadline = any_masked_device_requires_polling(
            mask,
            self.router.devices.iter().map(|d| d.requires_polling()),
        )
        .then_some(timestamp + POLLING_FALLBACK);
        let next = select_wake_deadline(protocol_deadline, polling_deadline);

        if let Some(t) = next {
            let next = TimeValue::from_micros(t.total_micros() as _);

            // drop old timeout future
            self.timeout = None;

            let mut fut = Box::pin(sleep_until(next));
            let mut cx = Context::from_waker(waker);

            if fut.as_mut().poll(&mut cx).is_ready() {
                waker.wake_by_ref();
                return;
            } else {
                self.timeout = Some(fut);
            }
        }

        for (i, device) in self.router.devices.iter().enumerate() {
            if mask & (1 << i) != 0 {
                device.register_waker(waker);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::{boxed::Box, sync::Arc, task::Wake, vec};
    use core::{
        sync::atomic::{AtomicUsize, Ordering},
        task::Waker,
    };

    use smoltcp::time::Instant;

    use super::{Service, any_masked_device_requires_polling, select_wake_deadline};
    use crate::{
        async_rx::{RX_NOTIFY, SERIAL},
        device::LoopbackDevice,
        router::{Router, RxOwnerView},
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

    #[test]
    fn no_deadline_without_protocol_timer_or_polling_fallback() {
        assert_eq!(select_wake_deadline(None, None), None);
    }

    #[test]
    fn preserves_protocol_deadline_without_polling_fallback() {
        let protocol = Instant::from_millis_const(25);

        assert_eq!(select_wake_deadline(Some(protocol), None), Some(protocol));
    }

    #[test]
    fn uses_polling_fallback_without_protocol_deadline() {
        let fallback = Instant::from_millis_const(10);

        assert_eq!(select_wake_deadline(None, Some(fallback)), Some(fallback));
    }

    #[test]
    fn chooses_earlier_protocol_or_polling_deadline() {
        let earlier = Instant::from_millis_const(10);
        let later = Instant::from_millis_const(25);

        assert_eq!(
            select_wake_deadline(Some(later), Some(earlier)),
            Some(earlier)
        );
        assert_eq!(
            select_wake_deadline(Some(earlier), Some(later)),
            Some(earlier)
        );
    }

    #[test]
    fn masked_non_polling_device_does_not_trigger_fallback() {
        let mask = 0b001;
        let capabilities = [false];

        assert!(!any_masked_device_requires_polling(mask, capabilities));
    }

    #[test]
    fn unmasked_polling_device_does_not_trigger_fallback() {
        let mask = 0b010;
        let capabilities = [true, false];

        assert!(!any_masked_device_requires_polling(mask, capabilities));
    }

    #[test]
    fn masked_polling_device_triggers_fallback() {
        let mask = 0b001;
        let capabilities = [true];

        assert!(any_masked_device_requires_polling(mask, capabilities));
    }

    #[test]
    fn mixed_devices_only_masked_polling_decides() {
        let mask = 0b101;
        let capabilities = [true, true, false];

        assert!(any_masked_device_requires_polling(mask, capabilities));

        let mask = 0b101;
        let capabilities = [false, true, false];

        assert!(!any_masked_device_requires_polling(mask, capabilities));
    }

    #[test]
    fn service_poll_wakes_waiting_rx_task_after_ingress_frees_space() {
        let _serial = SERIAL.lock();
        let mut router = Router::new();
        router.add_device(Box::new(LoopbackDevice::new()));
        router.fill_rx_buffer_for_test();
        let mut service = Service::new(router, None);

        let count = Arc::new(AtomicUsize::new(0));
        RX_NOTIFY.register(&counting_waker(count.clone()));
        RX_NOTIFY.publish_waiting();

        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
        service.poll(RxOwnerView::PollingOwned, &mut sockets);

        assert_eq!(count.load(Ordering::Relaxed), 1);
    }
}
