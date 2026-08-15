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
    device::{RxCopyStep, TxDropReason, TxReclaimStep, TxSubmitStep},
    flush::{FlushRecheck, FlushTicket, FlushWaiter, error_code, error_from_code},
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
        self.flush_waiter = Some(FlushWaiter::new(identity, target));
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
        let done = match self.target_dev {
            Some(dev) => self.router.tx_flush_done(dev, target),
            None => true,
        };
        if done {
            self.flush_success += 1;
            self.flush_waiter = None;
            FlushRecheck::Done
        } else {
            FlushRecheck::Pending
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
    /// waiter when its target is now satisfied. Caller holds the guard.
    pub(crate) fn flush_progress(&mut self) {
        let Some(waiter) = &self.flush_waiter else {
            return;
        };
        let done = match self.target_dev {
            Some(dev) => self.router.tx_flush_done(dev, waiter.target()),
            None => true,
        };
        if done {
            waiter.wake();
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
    /// host tests drive the lease deadline deterministically.
    ///
    /// An expired lease is cleared and `auto_release_failure` saturating-
    /// incremented exactly once; no lease generation exists, so no identity
    /// can exhaust and no reachable Hold is ever permanent. The timer is
    /// wake-only, so the queue task's own wake drives this round.
    #[cfg(feature = "qemu-diagnostics")]
    pub(crate) fn diag_hold_tick(&mut self) -> u64 {
        if self.diag_hold_mode != crate::diag::HOLD_NONE
            && self.diag_lease_expiry_nanos != 0
            && crate::diag::diag_now() >= self.diag_lease_expiry_nanos
        {
            self.diag_hold_mode = crate::diag::HOLD_NONE;
            self.diag_lease_expiry_nanos = 0;
            self.diag_auto_release_failure = self.diag_auto_release_failure.saturating_add(1);
        }
        self.diag_hold_mode
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

        /// Serialized Service + initialized fake clock: `diag_hold_tick`
        /// reads the global `diag_now()`, so these tests must not share the
        /// clock with a parallel sibling.
        fn serialized_service() -> (spin::MutexGuard<'static, ()>, Service) {
            let serial = crate::async_rx::SERIAL.lock();
            crate::diag::set_test_now(now());
            let mut router = Router::new();
            router.add_device(Box::new(LoopbackDevice::new()));
            (serial, Service::new(router, None))
        }

        #[test]
        fn control_rejects_out_of_range_lease_and_bad_ops() {
            let (_serial, mut s) = serialized_service();
            assert!(matches!(
                s.diag_control(OP_HOLD_TX_SUBMIT, 0, now()),
                Err(DevError::InvalidParam)
            ));
            assert!(matches!(
                s.diag_control(OP_HOLD_TX_SUBMIT, MAX_LEASE_MS + 1, now()),
                Err(DevError::InvalidParam)
            ));
            assert!(matches!(
                s.diag_control(OP_RELEASE, 1, now()),
                Err(DevError::InvalidParam)
            ));
            assert!(matches!(
                s.diag_control(99, 10, now()),
                Err(DevError::InvalidParam)
            ));
            assert_eq!(s.diag_hold_mode(), HOLD_NONE);
            assert_eq!(s.diag_lease_expiry(), 0);
        }

        #[test]
        fn hold_submit_and_reclaim_set_modes_and_expiry() {
            let (_serial, mut s) = serialized_service();
            s.diag_control(OP_HOLD_TX_SUBMIT, 100, now()).unwrap();
            assert_eq!(s.diag_hold_mode(), HOLD_SUBMIT);
            assert_eq!(s.diag_lease_expiry(), now() + 100 * NS_PER_MS);
            assert_eq!(s.diag_hold_tick(), HOLD_SUBMIT);
            s.diag_control(OP_HOLD_TX_RECLAIM, 1, now()).unwrap();
            assert_eq!(s.diag_hold_mode(), HOLD_RECLAIM);
            assert_eq!(s.diag_lease_expiry(), now() + NS_PER_MS);
        }

        #[test]
        fn release_clears_hold_and_never_counts_failure() {
            let (_serial, mut s) = serialized_service();
            s.diag_control(OP_HOLD_TX_SUBMIT, 2000, now()).unwrap();
            s.diag_control(OP_RELEASE, 0, now()).unwrap();
            assert_eq!(s.diag_hold_mode(), HOLD_NONE);
            assert_eq!(s.diag_lease_expiry(), 0);
            assert_eq!(s.diag_auto_release_failure(), 0);
            assert_eq!(s.diag_hold_tick(), HOLD_NONE);
        }

        #[test]
        fn expired_lease_auto_releases_and_counts_failure() {
            let (_serial, mut s) = serialized_service();
            s.diag_control(OP_HOLD_TX_SUBMIT, 2, now()).unwrap();
            crate::diag::set_test_now(now() + 2 * NS_PER_MS - 1);
            assert_eq!(s.diag_hold_tick(), HOLD_SUBMIT);
            crate::diag::set_test_now(now() + 2 * NS_PER_MS);
            assert_eq!(s.diag_hold_tick(), HOLD_NONE);
            assert_eq!(s.diag_auto_release_failure(), 1);
            assert_eq!(s.diag_hold_mode(), HOLD_NONE);
            assert_eq!(s.diag_lease_expiry(), 0);
            crate::diag::set_test_now(now() + 2 * NS_PER_MS + 1);
            assert_eq!(s.diag_hold_tick(), HOLD_NONE);
            assert_eq!(s.diag_auto_release_failure(), 1);
        }

        #[test]
        fn second_hold_after_expiry_reuses_the_state() {
            let (_serial, mut s) = serialized_service();
            s.diag_control(OP_HOLD_TX_RECLAIM, 1, now()).unwrap();
            crate::diag::set_test_now(now() + NS_PER_MS);
            assert_eq!(s.diag_hold_tick(), HOLD_NONE);
            s.diag_control(OP_HOLD_TX_RECLAIM, 1, now() + NS_PER_MS)
                .unwrap();
            assert_eq!(s.diag_hold_mode(), HOLD_RECLAIM);
            assert_eq!(s.diag_auto_release_failure(), 1);
        }

        #[test]
        fn hold_does_not_mutate_owner_or_completion_state() {
            let (_serial, mut s) = serialized_service();
            s.diag_control(OP_HOLD_TX_SUBMIT, 10, now()).unwrap();
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
            let (_serial, mut s) = serialized_service();
            let far_future = u64::MAX - 10;
            assert!(matches!(
                s.diag_control(OP_HOLD_TX_SUBMIT, MAX_LEASE_MS, far_future),
                Err(DevError::InvalidParam)
            ));
            assert_eq!(s.diag_hold_mode(), HOLD_NONE);
            assert_eq!(s.diag_lease_expiry(), 0);
            assert_eq!(s.diag_auto_release_failure(), 0);
        }

        #[test]
        fn any_reachable_hold_is_releasable_or_expirable() {
            // D9/C1: the Service lease carries no generation, so no identity
            // can exhaust. Every reachable Hold is releasable explicitly or
            // by expiry, even after many commit/release/expiry cycles.
            let (_serial, mut s) = serialized_service();
            for i in 0..200u64 {
                s.diag_control(OP_HOLD_TX_SUBMIT, 2, now() + i).unwrap();
                assert_eq!(s.diag_hold_mode(), HOLD_SUBMIT);
                crate::diag::set_test_now(now() + i + 2 * NS_PER_MS);
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
