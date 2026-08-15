//! Async RX queue-task decision layer.
//!
//! This module hosts the crate-private seam between the future RX queue task
//! and [`crate::service::Service`]: a single-waiter queue notification state,
//! pure lifecycle/event/budget decisions, the unique named queue task wiring,
//! and fixed ISR/software event publication entry points.

#[cfg(not(test))]
use alloc::{borrow::ToOwned, boxed::Box};
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
    device::{RxCopyStep, TxReclaimStep, TxSubmitStep, fixed_queue::MAX_LIVE_TICKETS},
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
        RX_TELEMETRY.queue_wake.fetch_add(1, Ordering::Relaxed);
    }

    /// Publishes a queue-owner work hint: bumps the shared generation
    /// (Release) and wakes only the queue-owner role. Software producers
    /// (stack TX enqueue, software nudge) call this after committing state
    /// so the wait protocol's generation recheck closes the
    /// event-before-register window (Task 3.5).
    pub(crate) fn publish_queue_work(&self) {
        self.generation.fetch_add(1, Ordering::Release);
        self.queue_waker.wake();
        RX_TELEMETRY.queue_wake.fetch_add(1, Ordering::Relaxed);
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
    /// TX reclaim stage budget exhaustion rounds.
    pub reclaim_exhausted: AtomicU64,
    /// RX copy stage budget exhaustion rounds.
    pub rx_exhausted: AtomicU64,
    /// TX submit stage budget exhaustion rounds.
    pub submit_exhausted: AtomicU64,
    /// Queue-owner wake publications (Task 4.2 V3 telemetry).
    pub queue_wake: AtomicU64,
    /// Illegal lifecycle transitions (Task 4.2 V3 telemetry).
    pub lifecycle_fault: AtomicU64,
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
    /// RW-2: terminal ownership-invariant faults (unknown/duplicate reclaim
    /// cookie, or a ticket that cannot transition). Counts how many times the
    /// device-side cookie→ticket ledger drifted, independent of the raw
    /// completion and reclaim counters.
    pub ownership_invariant: AtomicU64,
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
            reclaim_exhausted: AtomicU64::new(0),
            rx_exhausted: AtomicU64::new(0),
            submit_exhausted: AtomicU64::new(0),
            queue_wake: AtomicU64::new(0),
            lifecycle_fault: AtomicU64::new(0),
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
            ownership_invariant: AtomicU64::new(0),
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

/// MS05 V3 snapshot: the MS04 `RxSnapshot` fields plus the slot/ticket/flush
/// ledger appended by the kernel ioctl.
///
/// The appended fields are taken from the Service target device under its
/// guard; a missing target reports zeros. The kernel maps these onto the
/// append-only `IrqSnapshotV3` wire type; no field here replaces or reorders
/// the V2 prefix.
pub fn rx_snapshot_v3() -> RxSnapshotV3 {
    let v2 = rx_snapshot();
    let (ledger, tx_ledger, flush_target, flush_counters, drop_reasons) = match crate::SERVICE.get()
    {
        Some(service) => {
            let mut guard = service.lock();
            let ledger = guard.v3_slot_ledger();
            // RW-2: the real driver buffer/descriptor ledger, not a
            // synthesis from slot or ticket capacities.
            let tx_ledger = guard.v3_tx_resource_ledger();
            let target = guard.v3_flush_target();
            let counters = guard.v3_flush_counters();
            let drops = guard.v3_drop_reasons();
            (ledger, tx_ledger, target, counters, drops)
        }
        None => (
            crate::device::SlotLedger::default(),
            None,
            u64::MAX,
            [0; 4],
            [0; 5],
        ),
    };
    let (
        tx_buffer_available,
        tx_buffer_inflight,
        tx_descriptor_available,
        tx_descriptor_inflight,
        tx_completion,
    ) = match tx_ledger {
        Some(l) => (
            l.buffer_available,
            l.buffer_inflight,
            l.descriptor_available,
            l.descriptor_inflight,
            l.completions_seen,
        ),
        // A driver without an observable ledger reports zeros; the snapshot
        // never fabricates conservation numbers from ticket capacities.
        None => (
            0,
            0,
            0,
            0,
            RX_TELEMETRY.tx_reclaimed.load(Ordering::Relaxed),
        ),
    };
    RxSnapshotV3 {
        lifecycle: v2.lifecycle,
        owner: v2.owner,
        isr_publish: v2.isr_publish,
        isr_wake: v2.isr_wake,
        software_nudge: v2.software_nudge,
        task_poll: v2.task_poll,
        reaped: v2.reaped,
        refilled: v2.refilled,
        delivered: v2.delivered,
        non_ip_consumed: v2.non_ip_consumed,
        budget_exhausted: v2.budget_exhausted,
        self_yield: v2.self_yield,
        router_full_wait: v2.router_full_wait,
        space_wake: v2.space_wake,
        empty_check: v2.empty_check,
        fault: v2.fault,
        last_error_stage: v2.last_error_stage,
        last_error_code: v2.last_error_code,
        rx_slot_occupancy: ledger.rx_occupancy,
        rx_slot_high_water: ledger.rx_high_water,
        rx_slot_full: ledger.rx_full,
        rx_slot_enqueue: ledger.rx_enqueue,
        rx_slot_dequeue: ledger.rx_dequeue,
        rx_slot_space_event: ledger.rx_space_event,
        tx_slot_occupancy: ledger.tx_occupancy,
        tx_slot_high_water: ledger.tx_high_water,
        tx_slot_full: ledger.tx_full,
        tx_slot_enqueue: ledger.tx_enqueue,
        tx_slot_dequeue: ledger.tx_dequeue,
        tx_slot_space_event: ledger.tx_space_event,
        tx_submit: RX_TELEMETRY.tx_submitted.load(Ordering::Relaxed),
        tx_again: RX_TELEMETRY.tx_again.load(Ordering::Relaxed),
        // RW-2: completion is the transport-observed used-ring count, reclaim
        // is the successful cookie→ticket reclaim; they are independent.
        tx_completion,
        tx_reclaim: RX_TELEMETRY.tx_reclaimed.load(Ordering::Relaxed),
        tx_buffer_available,
        tx_buffer_inflight,
        tx_descriptor_available,
        tx_descriptor_inflight,
        reclaim_exhausted: RX_TELEMETRY.reclaim_exhausted.load(Ordering::Relaxed),
        rx_exhausted: RX_TELEMETRY.rx_exhausted.load(Ordering::Relaxed),
        submit_exhausted: RX_TELEMETRY.submit_exhausted.load(Ordering::Relaxed),
        queue_generation: QUEUE_EVENT.generation(),
        queue_wake: RX_TELEMETRY.queue_wake.load(Ordering::Relaxed),
        last_accepted: ledger.last_accepted,
        live: ledger.live,
        queued: ledger.queued,
        device_owned: ledger.device_owned,
        flush_target,
        flush_success: flush_counters[0],
        flush_error: flush_counters[1],
        flush_busy: flush_counters[2],
        flush_cancel: flush_counters[3],
        #[cfg(feature = "qemu-diagnostics")]
        hold_mode: crate::diag::DIAGNOSTIC.hold_mode(),
        #[cfg(feature = "qemu-diagnostics")]
        lease_expiry: crate::diag::DIAGNOSTIC.lease_expiry(),
        #[cfg(feature = "qemu-diagnostics")]
        auto_release_failure: crate::diag::DIAGNOSTIC.auto_release_failure(),
        #[cfg(not(feature = "qemu-diagnostics"))]
        hold_mode: 0,
        #[cfg(not(feature = "qemu-diagnostics"))]
        lease_expiry: 0,
        #[cfg(not(feature = "qemu-diagnostics"))]
        auto_release_failure: 0,
        lifecycle_fault: RX_TELEMETRY.lifecycle_fault.load(Ordering::Relaxed),
        ownership_invariant: RX_TELEMETRY.ownership_invariant.load(Ordering::Relaxed),
        drop_malformed_ip: drop_reasons[0],
        drop_no_route: drop_reasons[1],
        drop_route_source_mismatch: drop_reasons[2],
        drop_unsupported_address: drop_reasons[3],
        drop_frame_too_large: drop_reasons[4],
    }
}

/// MS05 V3 diagnostic snapshot source (Task 4.2).
///
/// The first 18 fields mirror [`RxSnapshot`]; the appended fields expose the
/// fixed slot ledger, TX buffer/descriptor conservation, stage exhaustions,
/// queue generation/wake, ticket and flush state, plus stable drop reasons.
/// `repr(C)` and all-u64 so the kernel wire mapping stays trivially aligned.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RxSnapshotV3 {
    pub lifecycle: u64,
    pub owner: u64,
    pub isr_publish: u64,
    pub isr_wake: u64,
    pub software_nudge: u64,
    pub task_poll: u64,
    pub reaped: u64,
    pub refilled: u64,
    pub delivered: u64,
    pub non_ip_consumed: u64,
    pub budget_exhausted: u64,
    pub self_yield: u64,
    pub router_full_wait: u64,
    pub space_wake: u64,
    pub empty_check: u64,
    pub fault: u64,
    pub last_error_stage: u64,
    pub last_error_code: u64,
    pub rx_slot_occupancy: u64,
    pub rx_slot_high_water: u64,
    pub rx_slot_full: u64,
    pub rx_slot_enqueue: u64,
    pub rx_slot_dequeue: u64,
    pub rx_slot_space_event: u64,
    pub tx_slot_occupancy: u64,
    pub tx_slot_high_water: u64,
    pub tx_slot_full: u64,
    pub tx_slot_enqueue: u64,
    pub tx_slot_dequeue: u64,
    pub tx_slot_space_event: u64,
    pub tx_submit: u64,
    pub tx_again: u64,
    pub tx_completion: u64,
    pub tx_reclaim: u64,
    pub tx_buffer_available: u64,
    pub tx_buffer_inflight: u64,
    pub tx_descriptor_available: u64,
    pub tx_descriptor_inflight: u64,
    pub reclaim_exhausted: u64,
    pub rx_exhausted: u64,
    pub submit_exhausted: u64,
    pub queue_generation: u64,
    pub queue_wake: u64,
    pub last_accepted: u64,
    pub live: u64,
    pub queued: u64,
    pub device_owned: u64,
    pub flush_target: u64,
    pub flush_success: u64,
    pub flush_error: u64,
    pub flush_busy: u64,
    pub flush_cancel: u64,
    pub hold_mode: u64,
    pub lease_expiry: u64,
    pub auto_release_failure: u64,
    pub lifecycle_fault: u64,
    pub ownership_invariant: u64,
    pub drop_malformed_ip: u64,
    pub drop_no_route: u64,
    pub drop_route_source_mismatch: u64,
    pub drop_unsupported_address: u64,
    pub drop_frame_too_large: u64,
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
    // Task 3.5: a software nudge advances the shared generation and wakes
    // the queue owner, so the wait protocol's generation recheck closes the
    // event-before-register window instead of relying on the wake alone.
    notify.publish_queue_work();
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

    pub(crate) fn try_lock(&self) -> Option<ServiceGuard<'_>> {
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
    /// RW-1: the QEMU diagnostic hold state this queue owner drives.
    /// Production passes the global `&DIAGNOSTIC`; host tests inject their own
    /// instance so a hold committed by one test can never leak into a parallel
    /// sibling that services a round.
    #[cfg(feature = "qemu-diagnostics")]
    diag: &'static crate::diag::DiagnosticState,
    /// RW-1: armed QEMU diagnostic lease deadline (wall nanos) the owner is
    /// sleeping on. When it elapses the next round's `diag_hold_tick`
    /// auto-releases the expired hold.
    #[cfg(feature = "qemu-diagnostics")]
    lease_deadline: Option<u64>,
    /// RW-1: axtask timer that wakes the queue owner at `lease_deadline`.
    /// Production only; host tests drive the fake clock and re-poll instead.
    #[cfg(all(feature = "qemu-diagnostics", not(test)))]
    lease_timer: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
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
    /// RW-1: sleep purely on the QEMU diagnostic lease deadline (wall nanos).
    /// The held stage's completion must not drive the arm/recheck protocol
    /// (it would retry forever); the only exits are the lease timer or an
    /// explicit Release publishing queue work.
    #[cfg(feature = "qemu-diagnostics")]
    SleepUntil(u64),
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
        // QEMU diagnostic hold (D9): a hold pauses exactly one stage of the
        // sole queue owner. The lease is advanced once per round; an expired
        // lease auto-releases and counts a failure. The state is the future's
        // own instance (`self.diag`), so a hold only ever gates this owner.
        #[cfg(feature = "qemu-diagnostics")]
        let hold = service.diag_hold_tick(self.diag);
        #[cfg(not(feature = "qemu-diagnostics"))]
        let hold = 0u64;

        // Stage 1: TX completion reclaim (≤32). Releasing a completion
        // frees a driver buffer and its live ticket.
        let mut reclaimed = 0usize;
        #[cfg(feature = "qemu-diagnostics")]
        let reclaim_held = hold == crate::diag::HOLD_RECLAIM;
        #[cfg(not(feature = "qemu-diagnostics"))]
        let reclaim_held = false;
        if !reclaim_held {
            loop {
                match service.tx_reclaim_one_target() {
                    TxReclaimStep::Reclaimed => {
                        reclaimed += 1;
                        self.telemetry.tx_reclaimed.fetch_add(1, Ordering::Relaxed);
                        // D8: a reclaimed ticket may satisfy a pending C4 flush.
                        service.flush_progress();
                        if reclaimed >= RECLAIM_BUDGET {
                            self.telemetry
                                .budget_exhausted
                                .fetch_add(1, Ordering::Relaxed);
                            self.telemetry
                                .reclaim_exhausted
                                .fetch_add(1, Ordering::Relaxed);
                            break;
                        }
                    }
                    TxReclaimStep::Empty => break,
                    TxReclaimStep::Fault(err) => {
                        self.telemetry
                            .record_fault(rx_error_stage::RECEIVE_RECYCLE, &err);
                        // RW-2: an ownership-invariant reclaim fault (unknown,
                        // duplicate or still-Queued cookie) is a terminal
                        // cookie→ticket ledger drift; count it independently
                        // of raw completions and successful reclaims.
                        if matches!(err, DevError::BadState) {
                            self.telemetry
                                .ownership_invariant
                                .fetch_add(1, Ordering::Relaxed);
                        }
                        // D8: a terminal reclaim fault wakes the flush waiter.
                        service.flush_fault(&err);
                        return RoundOutcome::Fault(err);
                    }
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
                        self.telemetry.rx_exhausted.fetch_add(1, Ordering::Relaxed);
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
                    service.flush_fault(&err);
                    return RoundOutcome::Fault(err);
                }
            }
        }

        // Stage 3: TX slot submit (≤32). A successful submit pops the slot
        // and keeps its ticket live; `Again` retains the slot frame and
        // stops this stage.
        let mut submitted = 0usize;
        let mut submit_full = false;
        #[cfg(feature = "qemu-diagnostics")]
        let submit_held = hold == crate::diag::HOLD_SUBMIT;
        #[cfg(not(feature = "qemu-diagnostics"))]
        let submit_held = false;
        if !submit_held {
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
                            self.telemetry
                                .submit_exhausted
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
                        service.flush_fault(&err);
                        return RoundOutcome::Fault(err);
                    }
                }
            }
        } else {
            // A held submit stage behaves like `Again` for scheduling: the
            // driver capacity is not advancing, so a TX backlog must not
            // self-wake into a busy loop.
            submit_full = true;
        }

        // Round-end scheduling decision (Task 3.5 + RW-1).
        //
        // Self-wake only for backlog that can advance WITHOUT an external
        // resource: a visible completion, or a TX slot backlog that submit
        // was not blocked on (`Again`). A submit `Again` with no visible
        // completion registers/arms/rechecks and sleeps; a completion event
        // resumes it. RX-slot Full waits for stack drain, but never before a
        // still-advanceable TX backlog.
        //
        // RW-1: a stage held by the QEMU diagnostic lease cannot advance.
        // Its resource must not drive self-wake (busy loop) nor the
        // arm/recheck protocol (it would retry forever on the held
        // completion). A held stage can only resume via lease expiry or an
        // explicit Release, so the round sleeps until the lease deadline.
        #[cfg(feature = "qemu-diagnostics")]
        let hold_active = hold != crate::diag::HOLD_NONE;
        #[cfg(not(feature = "qemu-diagnostics"))]
        let hold_active = false;
        let pending = match service.completion_pending_both_target() {
            Ok(pending) => pending,
            Err(err) => {
                self.telemetry
                    .record_fault(rx_error_stage::COMPLETION_QUERY, &err);
                service.flush_fault(&err);
                return RoundOutcome::Fault(err);
            }
        };
        let tx_pending = service.tx_slot_pending_target();
        // RW-1: a visible TX completion is consumed by the reclaim stage;
        // under a reclaim hold it can never advance, so it must not
        // self-wake. TX slots are consumed by submit; under a submit hold
        // (`submit_full`) they cannot advance either.
        let tx_completion_advanceable = pending.contains(NetQueueDirection::TX) && !reclaim_held;
        let tx_slot_advanceable = tx_pending && !submit_full;
        if pending.contains(NetQueueDirection::RX) || tx_completion_advanceable {
            // A visible completion can advance reclaim/RX/submit: retry.
            self.telemetry.self_yield.fetch_add(1, Ordering::Relaxed);
            RoundOutcome::SelfWakeYield
        } else if tx_slot_advanceable {
            // More TX slots remain and submit was not blocked on `Again`:
            // the backlog advances next round without a completion.
            self.telemetry.self_yield.fetch_add(1, Ordering::Relaxed);
            RoundOutcome::SelfWakeYield
        } else if rx_full {
            // Only RX is blocked on full slot space; nothing else can
            // advance. Wait for the stack to drain the slots. The lease
            // deadline is additionally armed by the future when a hold is
            // active, so an expired hold still auto-releases while waiting.
            let decision = service.rx_slot_space_recheck_or_wait();
            if decision == SpaceDecision::Waiting {
                self.telemetry
                    .router_full_wait
                    .fetch_add(1, Ordering::Relaxed);
            }
            RoundOutcome::WaitSpace(decision)
        } else if hold_active {
            // RW-1: a hold lease is active and the held stage blocks the
            // remaining work. Sleep until the lease deadline; never self-wake
            // and never run the register/arm/recheck protocol on a held
            // completion (it would retry forever). The deadline timer only
            // wakes the owner; `diag_hold_tick` on the next round performs
            // the release, failure counter and queue-work publication.
            #[cfg(feature = "qemu-diagnostics")]
            {
                RoundOutcome::SleepUntil(self.diag.lease_expiry())
            }
            #[cfg(not(feature = "qemu-diagnostics"))]
            {
                let _ = hold;
                RoundOutcome::RegisterRecheck
            }
        } else if submit_full {
            // Submit hit `Again` with no visible completion: the driver is
            // full. Arm/register/recheck and sleep; a completion resumes.
            RoundOutcome::RegisterRecheck
        } else {
            self.telemetry.empty_check.fetch_add(1, Ordering::Relaxed);
            RoundOutcome::RegisterRecheck
        }
    }

    /// First poll: acquire the Service, run the all-or-nothing bidirectional
    /// activation (suppress BOTH + slot-mode switch), publish Active (or
    /// Unavailable) under the guard, then hand off to the active servicing
    /// loop.
    fn poll_first(&mut self, cx: &mut Context<'_>) -> Poll<()> {
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
            self.telemetry
                .lifecycle_fault
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Active poll: register the sole waker outside the Service lock, then
    /// service at most RX_BUDGET completions under the guard.
    fn poll_active(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        self.notify.register_queue(cx.waker());
        let Some(mut service) = self.service.try_lock() else {
            return Poll::Pending;
        };
        match self.service_round(&mut service) {
            RoundOutcome::SelfWakeYield => {
                drop(service);
                // Not a lease sleep: cancel any stale deadline so an explicit
                // Release invalidates the old timer (RW-1).
                #[cfg(feature = "qemu-diagnostics")]
                self.cancel_lease_deadline();
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            RoundOutcome::WaitSpace(SpaceDecision::Retry) => {
                drop(service);
                #[cfg(feature = "qemu-diagnostics")]
                self.cancel_lease_deadline();
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            RoundOutcome::WaitSpace(SpaceDecision::Waiting) => {
                drop(service);
                // RW-1: while waiting for RX slot space the lease may also
                // expire; arm the deadline so an expired hold still
                // auto-releases without an external NIC event. A 0 deadline
                // (no hold) cancels any stale one.
                #[cfg(feature = "qemu-diagnostics")]
                self.arm_lease_deadline(cx, self.diag.lease_expiry());
                Poll::Pending
            }
            RoundOutcome::RegisterRecheck => {
                drop(service);
                #[cfg(feature = "qemu-diagnostics")]
                self.cancel_lease_deadline();
                self.poll_register_recheck(cx)
            }
            #[cfg(feature = "qemu-diagnostics")]
            RoundOutcome::SleepUntil(deadline) => {
                drop(service);
                self.arm_lease_deadline(cx, deadline);
                Poll::Pending
            }
            RoundOutcome::Fault(_err) => {
                // Task 3.7: commit `Active -> Faulted` first, publish only on
                // success, so a woken stack waiter observes Faulted.
                self.publish_fatal();
                drop(service);
                Poll::Ready(())
            }
        }
    }

    /// RW-1: cancels any armed lease deadline and its timer.
    #[cfg(feature = "qemu-diagnostics")]
    fn cancel_lease_deadline(&mut self) {
        self.lease_deadline = None;
        self.cancel_lease_timer();
    }

    /// RW-1: arms (or cancels) the QEMU diagnostic lease deadline wake.
    ///
    /// The lease expiry is the only reason the owner must wake without an
    /// external NIC event: an expired hold must auto-release so the paused
    /// stage resumes. In production this registers an axtask timer that
    /// wakes the queue waker at `deadline`; host tests drive the fake clock
    /// instead. The timer only wakes the owner: the release, failure counter
    /// and queue-work publication stay in [`Service::diag_hold_tick`].
    ///
    /// A `deadline` of 0 (no active hold) cancels any previously armed
    /// deadline, so an explicit Release invalidates the old timer and a
    /// stale timer can never release a newer lease.
    #[cfg(feature = "qemu-diagnostics")]
    fn arm_lease_deadline(&mut self, cx: &mut Context<'_>, deadline: u64) {
        if deadline == 0 || crate::diag::diag_now() >= deadline {
            self.lease_deadline = None;
            self.cancel_lease_timer();
            return;
        }
        if self.lease_deadline == Some(deadline) {
            return;
        }
        self.lease_deadline = Some(deadline);
        self.arm_lease_timer(cx, deadline);
    }

    /// RW-1: drops any previously armed lease timer, cancelling it.
    #[cfg(all(feature = "qemu-diagnostics", not(test)))]
    fn cancel_lease_timer(&mut self) {
        self.lease_timer = None;
    }

    /// Host-test counterpart: there is no axtask timer to cancel.
    #[cfg(all(feature = "qemu-diagnostics", test))]
    fn cancel_lease_timer(&mut self) {}

    /// RW-1: registers an axtask timer that wakes the owner at `deadline`.
    #[cfg(all(feature = "qemu-diagnostics", not(test)))]
    fn arm_lease_timer(&mut self, cx: &mut Context<'_>, deadline: u64) {
        use axhal::time::TimeValue;
        use axtask::future::sleep_until;

        // Drop any previous timer future, which cancels its registration.
        self.lease_timer = None;
        let mut timer = Box::pin(sleep_until(TimeValue::from_nanos(deadline)));
        let mut timer_cx = Context::from_waker(cx.waker());
        if timer.as_mut().poll(&mut timer_cx).is_ready() {
            cx.waker().wake_by_ref();
        } else {
            self.lease_timer = Some(timer);
        }
    }

    /// Host-test counterpart: the fake clock drives the wake instead.
    #[cfg(all(feature = "qemu-diagnostics", test))]
    fn arm_lease_timer(&mut self, _cx: &mut Context<'_>, _deadline: u64) {}

    /// RW-1: if an armed lease deadline has elapsed, clear it and self-wake
    /// so the round runs, whose `diag_hold_tick` auto-releases the expired
    /// hold. The self-wake is observable by a counting waker in host tests.
    #[cfg(feature = "qemu-diagnostics")]
    fn lease_deadline_elapsed(&mut self, cx: &mut Context<'_>) {
        let Some(deadline) = self.lease_deadline else {
            return;
        };
        if crate::diag::diag_now() >= deadline {
            self.lease_deadline = None;
            self.cancel_lease_timer();
            cx.waker().wake_by_ref();
        }
    }

    /// Attempts the `Active -> Faulted` transition and publishes
    /// stack-progress only when the CAS commits.
    ///
    /// Task 3.7: the terminal wake ordering is state-first, event-after. An
    /// illegal transition (lifecycle already terminal) records the
    /// LIFECYCLE-stage diagnostic but never publishes a fake terminal state.
    fn publish_fatal(&self) {
        if self.transition_fatal() {
            self.notify.publish_progress();
        }
    }

    /// Records an illegal `Active -> Faulted` transition as LIFECYCLE-stage.
    /// Returns whether the transition committed.
    fn transition_fatal(&self) -> bool {
        match self.lifecycle.fatal() {
            Ok(()) => true,
            Err(TransitionError::Illegal(state)) => {
                self.telemetry
                    .record_last_error_code(rx_error_stage::LIFECYCLE, state.code() as u64);
                self.telemetry
                    .lifecycle_fault
                    .fetch_add(1, Ordering::Relaxed);
                false
            }
        }
    }

    /// Empty-queue wait: acquire generation, register, arm/recheck BOTH
    /// directions under the Service lock, then observe the generation again.
    fn poll_register_recheck(&mut self, cx: &mut Context<'_>) -> Poll<()> {
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
                // Task 3.7: the arm fault path holds no Service guard but
                // follows the same commit-then-publish ordering.
                self.publish_fatal();
                Poll::Ready(())
            }
        }
    }
}

impl Future for RxRxFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        // `self` is Unpin: every field is either a `&'static` reference, a
        // Copy access handle, or an owned `Pin<Box<..>>` timer that is Unpin.
        let this = self.get_mut();
        this.telemetry.task_poll.fetch_add(1, Ordering::Relaxed);
        // RW-1: an elapsed lease deadline clears itself and self-wakes so the
        // round below runs and `diag_hold_tick` auto-releases the expired
        // hold. The wake is observable by a counting waker in host tests.
        #[cfg(feature = "qemu-diagnostics")]
        this.lease_deadline_elapsed(cx);
        match this.lifecycle.load() {
            RxTaskLifecycle::Spawned => this.poll_first(cx),
            RxTaskLifecycle::Active => this.poll_active(cx),
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
                #[cfg(feature = "qemu-diagnostics")]
                diag: &crate::diag::DIAGNOSTIC,
                #[cfg(feature = "qemu-diagnostics")]
                lease_deadline: None,
                #[cfg(all(feature = "qemu-diagnostics", not(test)))]
                lease_timer: None,
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
        sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering},
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
        device::{Device, RxCopyStep, RxStep, TxOutcome, TxPreflight, TxReclaimStep, TxSubmitStep},
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

    /// Waker that samples the lifecycle state *inside* the wake callback.
    ///
    /// Task 3.7: the terminal wake ordering can only be witnessed by reading
    /// the lifecycle at the moment the wake fires, not after the future
    /// returns. `wake`/`wake_by_ref` record the observed lifecycle code and
    /// the wake count in shared atomics.
    struct LifecycleObservingWake {
        lifecycle: &'static RxLifecycle,
        observed: Arc<AtomicU8>,
        woken: Arc<AtomicUsize>,
    }

    impl alloc::task::Wake for LifecycleObservingWake {
        fn wake(self: Arc<Self>) {
            self.observed
                .store(self.lifecycle.load().code(), Ordering::Relaxed);
            self.woken.fetch_add(1, Ordering::Relaxed);
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.observed
                .store(self.lifecycle.load().code(), Ordering::Relaxed);
            self.woken.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn lifecycle_observing_waker(
        lifecycle: &'static RxLifecycle,
        observed: Arc<AtomicU8>,
        woken: Arc<AtomicUsize>,
    ) -> Waker {
        Waker::from(Arc::new(LifecycleObservingWake {
            lifecycle,
            observed,
            woken,
        }))
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
    fn software_nudge_advances_generation_and_wakes_queue() {
        let notify = QueueEvent::new();
        let telemetry = RxTelemetry::new();
        let count = Arc::new(AtomicUsize::new(0));
        notify.register_queue(&counting_waker(count.clone()));
        let generation_before = notify.generation();

        software_nudge_impl(&notify, &telemetry);

        assert_eq!(count.load(Ordering::Relaxed), 1);
        // Task 3.5: a software nudge advances the shared generation so the
        // event-before-register window is closed by the wait protocol's
        // generation recheck (D5), not just by the wake.
        assert_eq!(notify.generation(), generation_before + 1);
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
        /// TX-only completion visibility (RW-1 busy-loop witness): when set,
        /// only the TX direction reports a visible completion, independently
        /// of `completion_visible`.
        tx_completion_visible: AtomicBool,
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
                if self.stats.completion_visible.load(Ordering::Relaxed)
                    || self.stats.tx_completion_visible.load(Ordering::Relaxed)
                {
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
                && (self.stats.completion_visible.load(Ordering::Relaxed)
                    || self.stats.tx_completion_visible.load(Ordering::Relaxed))
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
                // A retained deferred head blocks the copy stage just like a
                // full slot: nothing is reaped and the head stays put.
                RxStep::Blocked => RxCopyStep::Full,
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

        fn tx_submit_calls_for_test(&self) -> usize {
            self.submit_calls.load(Ordering::Relaxed)
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
        #[cfg(feature = "qemu-diagnostics")]
        {
            leaked_future_diag(service_mutex, notify, leaked_diag())
        }
        #[cfg(not(feature = "qemu-diagnostics"))]
        {
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
    }

    /// A fresh per-test QEMU diagnostic state so a hold committed by one test
    /// never leaks into a parallel sibling that services a round (RW-1).
    #[cfg(feature = "qemu-diagnostics")]
    fn leaked_diag() -> &'static crate::diag::DiagnosticState {
        Box::leak(Box::new(crate::diag::DiagnosticState::new()))
    }

    /// Builds an injected Future over a caller-provided diagnostic state, so
    /// the QEMU hold tests control exactly the instance the owner services.
    #[cfg(feature = "qemu-diagnostics")]
    fn leaked_future_diag(
        service_mutex: &'static spin::Mutex<Service>,
        notify: &'static QueueEvent,
        diag: &'static crate::diag::DiagnosticState,
    ) -> (&'static RxLifecycle, RxRxFuture) {
        let lifecycle: &'static RxLifecycle = Box::leak(Box::new(RxLifecycle::new()));
        lifecycle.start().unwrap();
        let telemetry: &'static RxTelemetry = Box::leak(Box::new(RxTelemetry::new()));
        let fut = RxRxFuture {
            service: ServiceAccess::Injected(service_mutex),
            lifecycle,
            notify,
            telemetry,
            diag,
            lease_deadline: None,
            #[cfg(all(feature = "qemu-diagnostics", not(test)))]
            lease_timer: None,
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
            #[cfg(feature = "qemu-diagnostics")]
            diag: leaked_diag(),
            #[cfg(feature = "qemu-diagnostics")]
            lease_deadline: None,
            #[cfg(all(feature = "qemu-diagnostics", not(test)))]
            lease_timer: None,
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
    fn service_poll_router_space_alone_does_not_wake_waiting() {
        // Finding 6 (Task 3.5): the waiting bit is published only for
        // RX-slot Full. Router-buffer space alone must not clear it; the
        // wake condition is RX-slot space only, never a Router-buffer OR.
        let _serial = SERIAL.lock();
        let (mutex, _, control) = leaked_service(vec![RxStep::Consumed], true);
        // RX slots still full: the queue task's RX copy stays blocked.
        control.rx_slot_full.store(true, Ordering::Relaxed);
        let count = Arc::new(AtomicUsize::new(0));
        QUEUE_EVENT.register_queue(&counting_waker(count.clone()));
        QUEUE_EVENT.publish_waiting();

        let mut sockets = smoltcp::iface::SocketSet::new(vec![]);
        // The Router RX buffer has space (nothing delivered), but the RX
        // slots are still full: no space wake may be delivered.
        mutex.lock().poll(RxOwnerView::AsyncOwned, &mut sockets);
        assert_eq!(count.load(Ordering::Relaxed), 0);
        assert!(!QUEUE_EVENT.wake_if_space(false));
        // Clean up the shared waiting bit so sibling tests start quiescent.
        assert!(QUEUE_EVENT.wake_if_space(true));
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
    fn round_tx_again_without_completion_sleeps() {
        // Task 3.5: a `Full` (Again) TX submit with no visible completion
        // must arm/register/recheck and sleep, not self-wake — the driver is
        // still full, so a self-wake would busy-loop (D6 forbids it).
        let (mutex, copy_calls, control) =
            leaked_service_tx(vec![RxStep::Empty], vec![TxSubmitStep::Full], vec![], true);
        control.tx_slot_pending.store(true, Ordering::Relaxed);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        // RX copy ran (Empty probe) and the submit stage hit `Full`.
        assert_eq!(copy_calls.load(Ordering::Relaxed), 1);
        // No completion and the driver full: sleep via BOTH-direction
        // register/arm/recheck, with zero self-wakes.
        assert_eq!(count.load(Ordering::Relaxed), 0);
        assert_eq!(control.arm_calls.load(Ordering::Relaxed), 2);
        assert!(mutex.try_lock().is_some());
    }

    #[test]
    fn round_tx_again_with_completion_retries() {
        // Task 3.5: `Again` with a visible completion must retry — reclaim
        // can free driver space, so the round self-wakes once instead of
        // sleeping (fresh recheck, not static enum classification).
        let (mutex, copy_calls, control) =
            leaked_service_tx(vec![RxStep::Empty], vec![TxSubmitStep::Full], vec![], true);
        control.tx_slot_pending.store(true, Ordering::Relaxed);
        control.completion_visible.store(true, Ordering::Relaxed);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        assert_eq!(copy_calls.load(Ordering::Relaxed), 1);
        // Visible completion: retry via one self-wake, no arm.
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert_eq!(control.arm_calls.load(Ordering::Relaxed), 0);
        assert!(mutex.try_lock().is_some());
    }

    #[test]
    fn round_rx_full_does_not_block_tx_backlog() {
        // Task 3.5: an RX-slot Full must not starve a still-advanceable TX
        // backlog. The submit stage runs its full budget and the round
        // self-wakes, instead of returning WaitSpace on the RX full first.
        let submit_steps: Vec<_> = (0..=SUBMIT_BUDGET)
            .map(|_| TxSubmitStep::Submitted)
            .collect();
        let (mutex, copy_calls, control) =
            leaked_service_tx(vec![RxStep::Consumed], submit_steps, vec![], true);
        control.rx_slot_full.store(true, Ordering::Relaxed);
        control.tx_slot_pending.store(true, Ordering::Relaxed);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        // RX stage stopped on Full without reaping; TX advanced its budget.
        assert_eq!(copy_calls.load(Ordering::Relaxed), 0);
        assert_eq!(
            fut.telemetry.tx_submitted.load(Ordering::Relaxed),
            SUBMIT_BUDGET as u64
        );
        // The TX backlog self-wakes the round; no WaitSpace was published.
        assert_eq!(count.load(Ordering::Relaxed), 1);
        assert!(mutex.try_lock().is_some());
    }

    #[test]
    fn fatal_wakes_stack_progress() {
        // Task 3.5: a terminal fault must publish stack-progress so waiting
        // socket callers observe the stable fault (D4/D5), not just the
        // queue-owner role.
        let (mutex, _, control) = leaked_service(vec![RxStep::Empty], true);
        control.arm_error.store(true, Ordering::Relaxed);
        let notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));
        let stack_count = Arc::new(AtomicUsize::new(0));
        notify.register_stack(&counting_waker(stack_count.clone()));
        let (lifecycle, mut fut) = leaked_future(mutex, notify);
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(
            poll_once(&mut fut, count.clone()),
            Poll::Ready(())
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        assert_eq!(stack_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn fatal_service_round_wake_observes_faulted_lifecycle() {
        // Task 3.7: the RX-copy stage fault must commit `Active -> Faulted`
        // before releasing the generation and waking the stack role. The
        // observer samples the lifecycle inside the wake callback, so the old
        // publish-before-transition order observes `Active` and fails here.
        let (mutex, ..) = leaked_service(vec![RxStep::Fault(DevError::Io)], true);
        let notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));
        let (lifecycle, mut fut) = leaked_future(mutex, notify);
        lifecycle.preflight(true).unwrap();
        let observed = Arc::new(AtomicU8::new(u8::MAX));
        let woken = Arc::new(AtomicUsize::new(0));
        notify.register_stack(&lifecycle_observing_waker(
            lifecycle,
            observed.clone(),
            woken.clone(),
        ));
        let count = Arc::new(AtomicUsize::new(0));

        assert!(matches!(
            poll_once(&mut fut, count.clone()),
            Poll::Ready(())
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        assert_eq!(woken.load(Ordering::Relaxed), 1);
        assert_eq!(
            RxTaskLifecycle::from_code(observed.load(Ordering::Acquire)),
            RxTaskLifecycle::Faulted,
            "the stack waker must observe Faulted at wake time, not Active"
        );
    }

    #[test]
    fn fatal_arm_recheck_wake_observes_faulted_lifecycle() {
        // Task 3.7: the arm/recheck fault path (no Service guard) must also
        // commit Faulted before publishing the stack wake.
        let (mutex, _, control) = leaked_service(vec![RxStep::Empty], true);
        control.arm_error.store(true, Ordering::Relaxed);
        let notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));
        let (lifecycle, mut fut) = leaked_future(mutex, notify);
        let observed = Arc::new(AtomicU8::new(u8::MAX));
        let woken = Arc::new(AtomicUsize::new(0));
        notify.register_stack(&lifecycle_observing_waker(
            lifecycle,
            observed.clone(),
            woken.clone(),
        ));
        let count = Arc::new(AtomicUsize::new(0));

        assert!(matches!(
            poll_once(&mut fut, count.clone()),
            Poll::Ready(())
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        assert_eq!(woken.load(Ordering::Relaxed), 1);
        assert_eq!(
            RxTaskLifecycle::from_code(observed.load(Ordering::Acquire)),
            RxTaskLifecycle::Faulted,
            "the stack waker must observe Faulted at wake time, not Active"
        );
    }

    // ---- RW-2: ownership-invariant counting and real V3 ledger ----

    #[cfg(feature = "qemu-diagnostics")]
    #[test]
    fn reclaim_ownership_fault_increments_invariant_and_keeps_fault() {
        // A reclaim of an unknown cookie is a terminal cookie→ticket drift.
        // The queue round must count it once in `ownership_invariant` and
        // enter Faulted; the V3 snapshot reports the same counter.
        let _serial = SERIAL.lock();
        let (mutex, _, _stats) = leaked_service_tx(
            vec![RxStep::Empty],
            vec![],
            vec![TxReclaimStep::Fault(DevError::BadState)],
            true,
        );
        let notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));
        let (lifecycle, mut fut) = leaked_future(mutex, notify);
        let count = Arc::new(AtomicUsize::new(0));

        let before = fut.telemetry.ownership_invariant.load(Ordering::Relaxed);
        let fault_before = fut.telemetry.fault.load(Ordering::Relaxed);
        assert!(matches!(
            poll_once(&mut fut, count.clone()),
            Poll::Ready(())
        ));
        assert_eq!(
            fut.telemetry.fault.load(Ordering::Relaxed),
            fault_before + 1,
            "reclaim fault must be recorded"
        );
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        assert_eq!(
            fut.telemetry.ownership_invariant.load(Ordering::Relaxed),
            before + 1,
            "ownership drift must be counted exactly once"
        );
    }

    #[cfg(feature = "qemu-diagnostics")]
    #[test]
    fn successful_reclaim_never_counts_ownership_invariant() {
        // A well-formed reclaim (matching ticket) is not an ownership drift:
        // the counter must stay flat while `tx_reclaimed` grows.
        let _serial = SERIAL.lock();
        let (mutex, _, _stats) = leaked_service_tx(
            vec![RxStep::Empty],
            vec![],
            vec![TxReclaimStep::Reclaimed],
            true,
        );
        let notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));
        let (lifecycle, mut fut) = leaked_future(mutex, notify);
        let count = Arc::new(AtomicUsize::new(0));

        let before_inv = fut.telemetry.ownership_invariant.load(Ordering::Relaxed);
        let before_reclaim = fut.telemetry.tx_reclaimed.load(Ordering::Relaxed);
        // One round: reclaim succeeds, the round ends Pending.
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        assert_eq!(
            fut.telemetry.ownership_invariant.load(Ordering::Relaxed),
            before_inv,
            "a successful reclaim is not an ownership drift"
        );
        assert_eq!(
            fut.telemetry.tx_reclaimed.load(Ordering::Relaxed),
            before_reclaim + 1
        );
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
        // The RX stage exhausts its budget exactly once; the round-end yield
        // records only the self-wake, never a second exhaustion (Task 3.5).
        assert_eq!(fut.telemetry.budget_exhausted.load(Ordering::Relaxed), 1);
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
            #[cfg(feature = "qemu-diagnostics")]
            diag: leaked_diag(),
            #[cfg(feature = "qemu-diagnostics")]
            lease_deadline: None,
            #[cfg(all(feature = "qemu-diagnostics", not(test)))]
            lease_timer: None,
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
    fn active_stack_preflight_source_has_no_raw_tx_entry_points() {
        // Task 3.4: in slot mode the stack TX preflight must never touch raw
        // driver TX state (recycle/alloc/capacity/submit/reclaim are the
        // queue task's alone). The DormantSlots preflight branch must contain
        // none of these entry points; polling may legitimately recycle.
        let source = include_str!("device/ethernet.rs");
        let start = source.find("fn preflight_ready_tx").unwrap();
        let end = source[start..]
            .find("fn preflight_unknown_neighbor")
            .map(|offset| start + offset)
            .unwrap();
        let body = &source[start..end];
        let dormant = body
            .find("TxMode::DormantSlots")
            .expect("slot-mode preflight branch exists");
        let slot_branch = &body[dormant..];
        for raw in [
            "recycle_tx_buffers",
            "alloc_tx_buffer",
            "can_transmit",
            "submit_tx",
            "reclaim_tx",
        ] {
            assert!(
                !slot_branch.contains(raw),
                "slot-mode preflight must not call raw TX entry point {raw}"
            );
        }
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

        assert!(!fut.transition_fatal());
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
    fn illegal_fatal_transition_publishes_no_progress() {
        // Task 3.7: an illegal Active->Faulted transition must record the
        // LIFECYCLE diagnostic but never publish a fake terminal stack wake.
        let (mutex, ..) = leaked_service(vec![], true);
        let notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));
        let stack_count = Arc::new(AtomicUsize::new(0));
        notify.register_stack(&counting_waker(stack_count.clone()));
        let (lifecycle, fut) = leaked_future(mutex, notify);
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Spawned);

        fut.publish_fatal();
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Spawned);
        assert_eq!(stack_count.load(Ordering::Relaxed), 0);
        assert_eq!(fut.telemetry.fault.load(Ordering::Relaxed), 0);
        assert_eq!(
            fut.telemetry.last_error(),
            (
                rx_error_stage::LIFECYCLE,
                RxTaskLifecycle::Spawned.code() as u64,
            )
        );
    }

    #[test]
    fn fatal_paths_commit_before_publish_in_source() {
        // Task 3.7 source guard: neither terminal path may call
        // `publish_progress()` directly; `publish_fatal` is the single
        // commit-then-publish seam gated on a successful `transition_fatal()`.
        let source = include_str!("async_rx.rs");

        let seam_start = source.find("fn publish_fatal").unwrap();
        let seam_end = source[seam_start..]
            .find("fn transition_fatal")
            .map(|offset| seam_start + offset)
            .unwrap();
        let seam = &source[seam_start..seam_end];
        assert!(seam.contains("if self.transition_fatal()"));
        assert!(
            seam.find("transition_fatal()").unwrap() < seam.find("publish_progress()").unwrap(),
            "publish_fatal must commit the lifecycle before publishing progress"
        );

        let poll_active_start = source.find("fn poll_active").unwrap();
        let poll_active_end = source.find("fn publish_fatal").unwrap();
        let poll_active = &source[poll_active_start..poll_active_end];
        let round_fault = &poll_active[poll_active.find("RoundOutcome::Fault").unwrap()..];
        assert!(round_fault.contains("self.publish_fatal()"));
        assert!(
            !round_fault.contains("publish_progress()"),
            "poll_active fault branch must not publish directly"
        );

        let arm_start = source.find("fn poll_register_recheck").unwrap();
        let arm_end = source.find("impl Future for RxRxFuture").unwrap();
        let arm_region = &source[arm_start..arm_end];
        let arm_fault = &arm_region[arm_region.find("WaitDecision::Fault").unwrap()..];
        assert!(arm_fault.contains("self.publish_fatal()"));
        assert!(
            !arm_fault.contains("publish_progress()"),
            "poll_register_recheck fault branch must not publish directly"
        );
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

    // ── Task 4.3: QEMU diagnostic holds pause exactly one stage ─────────

    #[cfg(feature = "qemu-diagnostics")]
    #[test]
    fn hold_submit_pauses_submit_stage_but_not_reclaim_or_rx() {
        // The QEMU diagnostic state is per-future; a hold only ever gates this
        // owner, so parallel siblings servicing a round stay unaffected.
        let diag = leaked_diag();
        let t0 = crate::diag::diag_now();
        let (mutex, _copy_calls, _stats) = leaked_service_tx(
            vec![RxStep::Consumed, RxStep::Empty],
            (0..4).map(|_| TxSubmitStep::Submitted).collect(),
            vec![],
            true,
        );
        let notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));
        let (lifecycle, mut fut) = leaked_future_diag(mutex, notify, diag);
        let count = Arc::new(AtomicUsize::new(0));

        // Commit a long-lived submit hold (well under the 2 s max lease).
        diag.control(crate::diag::OP_HOLD_TX_SUBMIT, 1000, t0)
            .unwrap();
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        // RX copy still ran (stage 2), TX submit was paused (stage 3).
        {
            let mut guard = mutex.lock();
            let submits = guard.router_for_test().devices[0].tx_submit_calls_for_test();
            assert_eq!(submits, 0);
        }
        assert!(mutex.try_lock().is_some());
        // Release the hold; the sole owner resumes the paused stage.
        diag.control(crate::diag::OP_RELEASE, 0, t0).unwrap();
        diag.tick(u64::MAX);
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        {
            let mut guard = mutex.lock();
            // Release resumes the paused stage: the queued submits drain.
            let submits = guard.router_for_test().devices[0].tx_submit_calls_for_test();
            assert!(submits >= 1);
        }
    }

    #[cfg(feature = "qemu-diagnostics")]
    #[test]
    fn hold_reclaim_pauses_reclaim_stage_and_again_still_backpressures() {
        // The QEMU diagnostic state is per-future; a hold only ever gates this
        // owner, so parallel siblings servicing a round stay unaffected.
        let diag = leaked_diag();
        let t0 = crate::diag::diag_now();
        let (mutex, _, _stats) = leaked_service_tx(
            vec![RxStep::Empty],
            vec![TxSubmitStep::Full],
            (0..4).map(|_| TxReclaimStep::Reclaimed).collect(),
            true,
        );
        let notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));
        let (lifecycle, mut fut) = leaked_future_diag(mutex, notify, diag);
        let count = Arc::new(AtomicUsize::new(0));

        diag.control(crate::diag::OP_HOLD_TX_RECLAIM, 1000, t0)
            .unwrap();
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        // The reclaim stage was paused but the round stays Active and the held
        // submit `Again` backpressures without a busy loop or a fault.
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        assert!(mutex.try_lock().is_some());
        // Release the hold; the per-future state never leaks anywhere.
        diag.control(crate::diag::OP_RELEASE, 0, t0).unwrap();
        diag.tick(u64::MAX);
    }

    // ── RW-1: lease deadline drives the owner wake (fake clock) ─────────

    /// Advances the fake diagnostic clock for the RW-1 tests.
    #[cfg(feature = "qemu-diagnostics")]
    fn fake_clock(nanos: u64) {
        crate::diag::set_test_now(nanos);
    }

    #[cfg(feature = "qemu-diagnostics")]
    #[test]
    fn hold_submit_lease_deadline_wakes_and_auto_releases_exactly_once() {
        let _serial = SERIAL.lock();
        // Fake clock at T0: commit a 100 ms submit hold. Without an external
        // event, the only way the owner can wake is the lease deadline.
        let t0 = 1_000_000_000_000u64;
        fake_clock(t0);
        let diag = leaked_diag();
        let (mutex, _copy_calls, _stats) = leaked_service_tx(
            vec![RxStep::Empty],
            (0..4).map(|_| TxSubmitStep::Submitted).collect(),
            vec![],
            true,
        );
        let notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));
        let (lifecycle, mut fut) = leaked_future_diag(mutex, notify, diag);
        let count = Arc::new(AtomicUsize::new(0));

        diag.control(crate::diag::OP_HOLD_TX_SUBMIT, 100, t0)
            .unwrap();

        // Poll before the deadline: the future sleeps with the deadline armed
        // and must not self-wake or auto-release yet.
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        assert_eq!(count.load(Ordering::Relaxed), 0, "no wake before deadline");
        assert_eq!(diag.auto_release_failure(), 0);
        assert!(mutex.try_lock().is_some());

        // Just before the deadline: still sleeping, no wake, no auto-release.
        fake_clock(t0 + 99 * crate::diag::NS_PER_MS);
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(count.load(Ordering::Relaxed), 0, "no wake before deadline");
        assert_eq!(diag.auto_release_failure(), 0);

        // At the deadline the fake clock elapses: the future wakes exactly
        // once and the next round auto-releases the expired hold exactly once.
        fake_clock(t0 + 100 * crate::diag::NS_PER_MS);
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(count.load(Ordering::Relaxed), 1, "deadline wake fires once");
        assert_eq!(diag.auto_release_failure(), 1);
        assert_eq!(diag.hold_mode(), crate::diag::HOLD_NONE);
        // The resumed submit stage drains the queued frames.
        {
            let mut guard = mutex.lock();
            let submits = guard.router_for_test().devices[0].tx_submit_calls_for_test();
            assert!(submits >= 1);
        }
        // A later poll must not auto-release a second time.
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(diag.auto_release_failure(), 1);
        assert!(mutex.try_lock().is_some());
    }

    #[cfg(feature = "qemu-diagnostics")]
    #[test]
    fn held_reclaim_visible_tx_completion_does_not_busy_loop_before_deadline() {
        let _serial = SERIAL.lock();
        let t0 = 1_000_000_000_000u64;
        fake_clock(t0);
        let diag = leaked_diag();
        // A TX completion is visible, but the reclaim stage is held: the
        // completion can never advance, so the round must not self-wake into
        // a busy loop before the lease deadline.
        let (mutex, _copy_calls, control) = leaked_service_tx(
            vec![RxStep::Empty],
            vec![TxSubmitStep::Empty],
            vec![TxReclaimStep::Reclaimed],
            true,
        );
        control.tx_completion_visible.store(true, Ordering::Relaxed);
        let notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));
        let (lifecycle, mut fut) = leaked_future_diag(mutex, notify, diag);
        let count = Arc::new(AtomicUsize::new(0));

        diag.control(crate::diag::OP_HOLD_TX_RECLAIM, 100, t0)
            .unwrap();

        // Poll many times before the deadline: no self-wake may ever fire.
        for _ in 0..10 {
            assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
            assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
            assert_eq!(
                count.load(Ordering::Relaxed),
                0,
                "held TX completion must not busy-loop self-wake"
            );
            assert_eq!(diag.auto_release_failure(), 0);
            assert!(mutex.try_lock().is_some());
        }

        // At the deadline the hold auto-releases exactly once.
        fake_clock(t0 + 100 * crate::diag::NS_PER_MS);
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(diag.auto_release_failure(), 1);
        assert_eq!(diag.hold_mode(), crate::diag::HOLD_NONE);
    }

    #[cfg(feature = "qemu-diagnostics")]
    #[test]
    fn explicit_release_invalidates_stale_deadline_and_new_lease_is_not_released() {
        let _serial = SERIAL.lock();
        let t0 = 1_000_000_000_000u64;
        fake_clock(t0);
        let diag = leaked_diag();
        let (mutex, _copy_calls, _stats) = leaked_service_tx(
            vec![RxStep::Empty],
            (0..4).map(|_| TxSubmitStep::Submitted).collect(),
            vec![],
            true,
        );
        let notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));
        let (_lifecycle, mut fut) = leaked_future_diag(mutex, notify, diag);
        let count = Arc::new(AtomicUsize::new(0));

        // Hold A with a 100 ms lease.
        diag.control(crate::diag::OP_HOLD_TX_SUBMIT, 100, t0)
            .unwrap();
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(diag.hold_mode(), crate::diag::HOLD_SUBMIT);

        // Explicit Release before the deadline: the stage resumes and the
        // stale deadline must be invalidated.
        diag.control(crate::diag::OP_RELEASE, 0, t0).unwrap();
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(diag.hold_mode(), crate::diag::HOLD_NONE);
        assert_eq!(diag.auto_release_failure(), 0);
        {
            let mut guard = mutex.lock();
            let submits = guard.router_for_test().devices[0].tx_submit_calls_for_test();
            assert!(submits >= 1, "release resumes the paused stage");
        }

        // A new hold B with a longer lease must not be released by the stale
        // deadline from hold A.
        diag.control(crate::diag::OP_HOLD_TX_SUBMIT, 200, t0)
            .unwrap();
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(diag.hold_mode(), crate::diag::HOLD_SUBMIT);

        // Advance past hold A's old deadline: B must stay held.
        fake_clock(t0 + 100 * crate::diag::NS_PER_MS);
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(diag.hold_mode(), crate::diag::HOLD_SUBMIT);
        assert_eq!(diag.auto_release_failure(), 0);

        // Only B's own deadline releases it, exactly once.
        fake_clock(t0 + 200 * crate::diag::NS_PER_MS);
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(diag.auto_release_failure(), 1);
        assert_eq!(diag.hold_mode(), crate::diag::HOLD_NONE);
        assert!(mutex.try_lock().is_some());
    }
}
