use alloc::boxed::Box;
use core::{
    pin::Pin,
    sync::atomic::Ordering,
    task::{Context, Waker},
};

use axdriver::prelude::{DevError, DevResult};
use axdriver_net::NetQueueDirection;
use axhal::time::{NANOS_PER_MICROS, TimeValue, wall_time_nanos};
use axtask::future::sleep_until;
use smoltcp::{
    iface::{Interface, PollIngressSingleResult, PollResult, SocketSet},
    time::{Duration, Instant},
    wire::{HardwareAddress, IpAddress, IpListenEndpoint},
};

use crate::{
    LISTEN_TABLE, SOCKET_SET,
    async_rx::{QUEUE_EVENT, RX_TELEMETRY, SpaceDecision},
    device::{RxCopyStep, TxReclaimStep, TxSubmitStep},
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
        // Waking the queue task is a release of the resource it is blocked
        // on. The waiting bit is published only for a full RX slot (Task 3.2
        // slot-mode copy); Router-buffer space is drained by the stack itself
        // and must never clear it (Task 3.5 Finding 6).
        let space = self.rx_slot_has_space_target();
        if QUEUE_EVENT.wake_if_space(space) {
            RX_TELEMETRY
                .space_wake
                .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        }
        // Task 3.5 (Finding 2): a stack TX dispatch that fills an empty TX
        // slot must publish a queue-owner event. A sleeping queue task has no
        // hardware completion to wait on for the first frame, so without this
        // event the frame would sit in the slot forever.
        let tx_pending_before = self.tx_slot_pending_target();
        let dispatched = self.router.dispatch(timestamp) || changed;
        if !tx_pending_before && self.tx_slot_pending_target() {
            QUEUE_EVENT.publish_queue_work();
        }
        dispatched
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

    #[cfg(test)]
    pub(crate) fn router_for_test(&mut self) -> &mut Router {
        &mut self.router
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

        // The active NIC's socket waker registers as the stack-progress role
        // (Task 3.3): RX-slot-ready, TX-slot-space and fatal events then wake
        // the caller so smoltcp re-evaluates readiness. It is a hint, never
        // exact fd readiness, and it never overwrites the queue-owner waker.
        if let Some(dev) = self.target_dev {
            if mask & (1 << dev) != 0 && !self.router.devices[dev].requires_polling() {
                QUEUE_EVENT.register_stack(waker);
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
        async_rx::{QUEUE_EVENT, SERIAL},
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
        QUEUE_EVENT.register_queue(&counting_waker(count.clone()));
        QUEUE_EVENT.publish_waiting();

        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
        service.poll(RxOwnerView::PollingOwned, &mut sockets);

        assert_eq!(count.load(Ordering::Relaxed), 1);
    }
}
