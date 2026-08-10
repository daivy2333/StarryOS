use alloc::boxed::Box;
use core::{
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Waker},
};

use axhal::time::{NANOS_PER_MICROS, TimeValue, wall_time_nanos};
use axtask::future::sleep_until;
use embassy_sync::waitqueue::AtomicWaker;
use smoltcp::{
    iface::{Interface, PollIngressSingleResult, PollResult, SocketSet},
    time::{Duration, Instant},
    wire::{HardwareAddress, IpAddress, IpListenEndpoint},
};

use crate::{
    LISTEN_TABLE, SOCKET_SET,
    router::{Router, RxOwnerView},
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

/// Router-space software wake for the future RX queue task.
///
/// The queue side registers a waker without taking the `SERVICE` lock, then
/// publishes the waiting bit (Release) inside the Service lock only after
/// confirming the Router RX buffer is full. `Service::poll` clears the bit
/// (AcqRel) and wakes the task exactly once, after ingress has freed Router
/// buffer space. No `Relaxed` ordering is used because the waiting bit
/// participates in control flow.
struct RxSpaceSignal {
    waker: AtomicWaker,
    waiting: AtomicBool,
}

impl RxSpaceSignal {
    const fn new() -> Self {
        Self {
            waker: AtomicWaker::new(),
            waiting: AtomicBool::new(false),
        }
    }

    fn register(&self, waker: &Waker) {
        self.waker.register(waker);
    }

    fn wait_for_space(&self) {
        self.waiting.store(true, Ordering::Release);
    }

    fn wake_if_space(&self, has_space: bool) -> bool {
        if has_space && self.waiting.swap(false, Ordering::AcqRel) {
            self.waker.wake();
            true
        } else {
            false
        }
    }
}

static RX_SPACE: RxSpaceSignal = RxSpaceSignal::new();

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
        RX_SPACE.wake_if_space(self.router.rx_buffer_has_space());
        self.router.dispatch(timestamp) || changed
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

    use super::{RxSpaceSignal, any_masked_device_requires_polling, select_wake_deadline};
    use crate::{
        device::LoopbackDevice,
        router::{Router, RxOwnerView},
        service::Service,
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
    fn space_signal_full_waiting_then_space_wakes_once() {
        let signal = RxSpaceSignal::new();
        let count = Arc::new(AtomicUsize::new(0));
        signal.register(&counting_waker(count.clone()));
        signal.wait_for_space();
        assert!(signal.wake_if_space(true));
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert!(!signal.wake_if_space(true));
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn space_signal_still_full_does_not_wake() {
        let signal = RxSpaceSignal::new();
        let count = Arc::new(AtomicUsize::new(0));
        signal.register(&counting_waker(count.clone()));
        signal.wait_for_space();
        assert!(!signal.wake_if_space(false));
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn space_signal_not_waiting_does_not_wake() {
        let signal = RxSpaceSignal::new();
        let count = Arc::new(AtomicUsize::new(0));
        signal.register(&counting_waker(count.clone()));
        assert!(!signal.wake_if_space(true));
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn space_signal_second_poll_after_clear_does_not_wake() {
        let signal = RxSpaceSignal::new();
        let count = Arc::new(AtomicUsize::new(0));
        signal.register(&counting_waker(count.clone()));
        signal.wait_for_space();
        assert!(signal.wake_if_space(true));
        signal.wait_for_space();
        assert!(signal.wake_if_space(true));
        assert!(!signal.wake_if_space(true));
        assert_eq!(count.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn service_poll_wakes_waiting_rx_task_after_ingress_frees_space() {
        let mut router = Router::new();
        router.add_device(Box::new(LoopbackDevice::new()));
        router.fill_rx_buffer_for_test();
        let mut service = Service::new(router, None);

        let count = Arc::new(AtomicUsize::new(0));
        super::RX_SPACE.register(&counting_waker(count.clone()));
        super::RX_SPACE.wait_for_space();

        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
        service.poll(RxOwnerView::PollingOwned, &mut sockets);

        assert_eq!(count.load(Ordering::Relaxed), 1);
    }
}
