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

#[cfg(not(test))]
use crate::stack_runner::STACK_EVENT;
use crate::{
    device::{RxCopyStep, TxReclaimStep, TxSubmitStep},
    router::RxOwnerView,
    service::{LinkStep, Service},
    stack_runner::StackEvent,
    wrapper::SocketSetWrapper,
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
    waiting: AtomicBool,
    generation: AtomicU64,
    /// Task 3.1: pending used-ring cause flag. Set by the ISR used publisher,
    /// cleared by the owner's bounded `take_causes`.
    cause_used: AtomicBool,
    /// Task 3.1: pending config-change cause flag. Set by the ISR config
    /// publisher, cleared by the owner's bounded `take_causes`.
    cause_config: AtomicBool,
}

/// The bounded, lock-free cause flags a queue-owner poll takes once.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct QueueCauses {
    /// A used-ring publication is pending.
    pub used: bool,
    /// A config-change publication is pending.
    pub config: bool,
}

impl QueueEvent {
    pub(crate) const fn new() -> Self {
        Self {
            queue_waker: AtomicWaker::new(),
            waiting: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            cause_used: AtomicBool::new(false),
            cause_config: AtomicBool::new(false),
        }
    }

    #[cfg(test)]
    fn with_generation(generation: u64) -> Self {
        Self {
            queue_waker: AtomicWaker::new(),
            waiting: AtomicBool::new(false),
            generation: AtomicU64::new(generation),
            cause_used: AtomicBool::new(false),
            cause_config: AtomicBool::new(false),
        }
    }

    /// Registers the queue-owner task waker. Callable without the Service
    /// lock. The stack role lives in `StackEvent` (Iteration 000).
    pub(crate) fn register_queue(&self, waker: &Waker) {
        self.queue_waker.register(waker);
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

    /// Publishes a used-ring queue event: stores the used cause flag, wraps
    /// the shared generation (Release) and wakes the queue owner. Called by the
    /// ISR path. The used cause is never replaced by a config publish.
    pub(crate) fn publish_event(&self) {
        self.cause_used.store(true, Ordering::Release);
        self.generation.fetch_add(1, Ordering::Release);
        self.queue_waker.wake();
        RX_TELEMETRY.queue_wake.fetch_add(1, Ordering::Relaxed);
    }

    /// Publishes a config-change queue event (Task 3.1): stores the config
    /// cause flag, wraps the shared generation (Release) and wakes the queue
    /// owner. A config-only cause must wake the owner even with no used-ring
    /// completion, and a combined cause keeps both flags so neither publish
    /// mutates the other.
    pub(crate) fn publish_config(&self) {
        self.cause_config.store(true, Ordering::Release);
        self.generation.fetch_add(1, Ordering::Release);
        self.queue_waker.wake();
        RX_TELEMETRY.queue_wake.fetch_add(1, Ordering::Relaxed);
    }

    /// Bounded take of the pending cause flags (Task 3.1). The owner runs this
    /// inside one poll after registering its waker, then reads a consistent
    /// link snapshot at most once when the config flag is set. An AcqRel swap
    /// clears both flags; a transient snapshot error ("Again") is retained by
    /// a re-publish so the next poll retries without losing the cause.
    pub(crate) fn take_causes(&self) -> QueueCauses {
        QueueCauses {
            used: self.cause_used.swap(false, Ordering::AcqRel),
            config: self.cause_config.swap(false, Ordering::AcqRel),
        }
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
    /// queue wait protocol observes the change. The stack-wake role lives in
    /// `StackEvent`; here only the generation carries the hint.
    pub(crate) fn publish_progress(&self) {
        self.generation.fetch_add(1, Ordering::Release);
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
#[cfg(feature = "qemu-diagnostics")]
static RECOVERY_RESET_REQUEST: spin::Mutex<RecoveryRequestState> =
    spin::Mutex::new(RecoveryRequestState::new());

/// The one bounded explicit-recovery request.  It shares the lifecycle
/// transition lock with the resident owner so a request cannot survive a
/// natural recovery and trigger a second reset after the owner becomes Active
/// again.
#[cfg(feature = "qemu-diagnostics")]
#[derive(Debug, Clone, Copy, Default)]
struct RecoveryRequestState {
    pending: bool,
    owner_claimed: bool,
}

#[cfg(feature = "qemu-diagnostics")]
impl RecoveryRequestState {
    const fn new() -> Self {
        Self {
            pending: false,
            owner_claimed: false,
        }
    }

    fn request(&mut self, lifecycle: RxTaskLifecycle) -> DevResult {
        if lifecycle != RxTaskLifecycle::Active {
            return Err(DevError::BadState);
        }
        if self.pending || self.owner_claimed {
            return Err(DevError::ResourceBusy);
        }
        self.pending = true;
        Ok(())
    }

    fn claim(&mut self, lifecycle: RxTaskLifecycle) -> bool {
        if lifecycle != RxTaskLifecycle::Active || !self.pending || self.owner_claimed {
            return false;
        }
        self.pending = false;
        self.owner_claimed = true;
        true
    }

    /// A natural recovery wins any pending request; an explicit recovery also
    /// absorbs a request submitted between claim and the lifecycle CAS.
    fn clear_for_recovery(&mut self) {
        self.pending = false;
        self.owner_claimed = false;
    }
}

#[cfg(feature = "qemu-diagnostics")]
fn with_recovery_request_transition<T>(
    request: &spin::Mutex<RecoveryRequestState>,
    transition: impl FnOnce() -> T,
) -> T {
    let mut request = request.lock();
    request.clear_for_recovery();
    transition()
}

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

/// Stable D3 stage codes for a recovery/ownership fault summary (F2).
///
/// These are internal diagnostic codes ONLY — never serialized into the frozen
/// V1–V3 wire snapshot. They identify which bounded recovery stage the fault
/// was frozen at, so a fault summary is diagnosable without needing a new wire
/// field this iteration.
pub mod recover_stage {
    /// Submit wait timed out (a Queued frame was never accepted).
    pub const SUBMIT_WAIT: u64 = 1;
    /// Completion wait timed out (a DeviceOwned completion did not arrive).
    pub const COMPLETION_WAIT: u64 = 2;
    /// Reclaim timed out (a reclaimable completion could not be reaped).
    pub const RECLAIM: u64 = 3;
    /// Quiesce window (bounded DeviceOwned drain before reset) elapsed.
    pub const QUIESCE: u64 = 4;
    /// Reset confirmation (status == 0) or `begin_recovery` failed.
    pub const RESET: u64 = 5;
    /// Reinitialize (queue/backing rebuild) failed.
    pub const REINITIALIZE: u64 = 6;
    /// An ownership/identity/ledger drift detected (no reset attempted).
    pub const OWNERSHIP_DRIFT: u64 = 7;
    /// A checked QEMU control request consumed by the resident owner.
    pub const EXPLICIT_REQUEST: u64 = 8;
    /// Unclassified recovery fault.
    pub const UNKNOWN: u64 = 0;
}

/// Bounded local cause of a recovery/data fault (Task 2.2 / A4). Distinct from
/// the stage: the stage says *where* the owner is, the cause says *why* it
/// stopped. These codes are stable and never derived from an enum discriminant.
/// They are internal telemetry only and MUST NOT serialize into the V1–V3 ABI.
pub mod fault_cause {
    /// Cause not otherwise classified.
    pub const UNKNOWN: u64 = 0;
    /// A submit/completion/reclaim data-stage or driver recovery stage
    /// absolute deadline expired while the condition was still blocked.
    pub const TIMEOUT: u64 = 1;
    /// An ownership/identity/ledger drift was detected (never masked by reset).
    pub const OWNERSHIP_DRIFT: u64 = 2;
}

/// The complete, coherent identity of one recovery/data fault (Task 2.2 / A4).
///
/// A fault is committed as a single value under the Service guard so a reader
/// can never combine the stage of one fault with the epoch or owner summary of
/// another. `queue_epoch` is the software ticket epoch; `available`,
/// `device_owned` and `quarantined` are the driver's real owner resources at
/// commit time.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RecoveryFaultIdentity {
    /// Stage code from [`recover_stage`] where the owner stopped.
    pub stage: u64,
    /// Local cause from [`fault_cause`].
    pub local_cause: u64,
    /// Software ticket epoch observed at commit time.
    pub queue_epoch: u64,
    /// Driver owner summary: available buffers/descriptors.
    pub available: u64,
    /// Driver owner summary: device-owned buffers/descriptors.
    pub device_owned: u64,
    /// Driver owner summary: quarantined buffers/descriptors.
    pub quarantined: u64,
}

/// Lock-free, single-writer coherent publication of the most recent recovery
/// fault identity (Task 2.2 / A4 / D5; Findings 1–2 of the second review).
///
/// This is a two-pass seqlock with a **bounded** read. `publish` bumps
/// `generation` to ODD, writes the six fields, then bumps `generation` to
/// EVEN. `read` loads `generation`; if it is odd (a writer is mid-publish) it
/// defers; otherwise it snapshots the fields and loads `generation` again. A
/// match on a nonzero even value proves the snapshot came from one complete
/// publication.
///
/// Ordering argument: every atomic operation in the publication and read
/// protocol uses `Ordering::SeqCst`, so they all share one total order that
/// also preserves each thread's program order. If a reader's two generation
/// loads both return the same nonzero even value, the writer's ODD store, its
/// six field stores and its EVEN store cannot interleave between those two
/// loads; therefore `Some` is returned only for one complete publication. A
/// reader mid-publication observes the ODD marker or a generation mismatch
/// and defers rather than accepting a partial tuple.
///
/// Boundedness (Finding 1): `read` performs at most [`READ_BOUND`] attempts; if
/// it cannot obtain a clean even snapshot it returns `None` (a defer/recheck
/// result) instead of spinning. It never waits on a possibly-preempted writer:
/// a writer paused after ODD and before EVEN yields `None`, and the reader
/// returns. The writer is a single non-concurrent owner, so after it resumes
/// and finishes EVEN, a later `read` returns the complete tuple.
#[derive(Debug)]
pub(crate) struct CoherentFaultSheet {
    generation: AtomicU64,
    stage: AtomicU64,
    local_cause: AtomicU64,
    queue_epoch: AtomicU64,
    available: AtomicU64,
    device_owned: AtomicU64,
    quarantined: AtomicU64,
}

/// Upper bound on `read` attempts before it defers (returns `None`). Kept
/// deliberately small: a torn tuple can only be observed while a single writer
/// is between ODD and EVEN, so two attempts cover the in-flight window and
/// never spin across a preempted writer.
const READ_BOUND: usize = 2;

impl CoherentFaultSheet {
    pub(crate) const fn new() -> Self {
        Self {
            generation: AtomicU64::new(0),
            stage: AtomicU64::new(0),
            local_cause: AtomicU64::new(0),
            queue_epoch: AtomicU64::new(0),
            available: AtomicU64::new(0),
            device_owned: AtomicU64::new(0),
            quarantined: AtomicU64::new(0),
        }
    }

    /// Commits `fault` as one coherent publication. Callers must serialize
    /// (the resident owner publishes under the Service guard). The ODD marker
    /// is SeqCst-stored first; the EVEN marker SeqCst-stored last.
    pub(crate) fn publish(&self, fault: RecoveryFaultIdentity) {
        self.mark_in_progress();
        self.write_fields(fault);
        self.finish_in_progress();
        debug_assert!(self.generation.load(Ordering::SeqCst) & 1 == 0, "even");
    }

    /// Marks a publication as in progress: bumps `generation` to ODD, before any
    /// field is written, so a reader never accepts a partially-updated tuple.
    /// Split out of `publish` and kept callable so the deterministic test seam
    /// (Finding 2) can pause the writer exactly here.
    fn mark_in_progress(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    /// Stores the six identity fields (after the ODD marker, before the EVEN
    /// marker). `Ordering::SeqCst`: every protocol operation shares one total
    /// order, so no field store can escape the reader's validating generation
    /// loads.
    fn write_fields(&self, fault: RecoveryFaultIdentity) {
        self.stage.store(fault.stage, Ordering::SeqCst);
        self.local_cause.store(fault.local_cause, Ordering::SeqCst);
        self.queue_epoch.store(fault.queue_epoch, Ordering::SeqCst);
        self.available.store(fault.available, Ordering::SeqCst);
        self.device_owned
            .store(fault.device_owned, Ordering::SeqCst);
        self.quarantined.store(fault.quarantined, Ordering::SeqCst);
    }

    /// Completes a publication: bumps `generation` from ODD to EVEN (SeqCst) so
    /// the whole identity becomes readable as one publication.
    fn finish_in_progress(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    /// Reads the most recent fault identity, or `None` before any publish / when
    /// it defers. `None` is a bounded, non-blocking defer (Finding 1): if a
    /// writer is paused mid-publication this returns without spinning, and a
    /// later call returns the complete tuple once the writer finishes EVEN.
    pub(crate) fn read(&self) -> Option<RecoveryFaultIdentity> {
        for _ in 0..READ_BOUND {
            let g1 = self.generation.load(Ordering::SeqCst);
            if g1 == 0 {
                return None;
            }
            // Odd generation = a writer is mid-publish; never trust the fields.
            if g1 & 1 == 1 {
                continue;
            }
            let fault = self.snapshot_fields();
            let g2 = self.generation.load(Ordering::SeqCst);
            // Both even and unchanged => no writer touched the fields during the
            // snapshot => the tuple is one complete publication.
            if g1 == g2 && g2 & 1 == 0 {
                return Some(fault);
            }
        }
        None
    }

    fn snapshot_fields(&self) -> RecoveryFaultIdentity {
        RecoveryFaultIdentity {
            stage: self.stage.load(Ordering::SeqCst),
            local_cause: self.local_cause.load(Ordering::SeqCst),
            queue_epoch: self.queue_epoch.load(Ordering::SeqCst),
            available: self.available.load(Ordering::SeqCst),
            device_owned: self.device_owned.load(Ordering::SeqCst),
            quarantined: self.quarantined.load(Ordering::SeqCst),
        }
    }
}

/// The three concurrent data-stage waits an `Active` queue owner can be blocked
/// on (Task 2.2): a Queued frame the driver is not accepting (submit), a
/// DeviceOwned ticket with no visible completion (completion), and a visible TX
/// completion that is not being reclaimed (reclaim). Each carries the absolute
/// monotonic deadline armed when the wait was first observed; `None` means the
/// wait is not active. A wait is armed exactly once on first observation and
/// cleared when it resolves, so a same-stage pending never renews it (Find 3).
pub(crate) struct DataStageDeadlines {
    pub submit: Option<u64>,
    pub completion: Option<u64>,
    pub reclaim: Option<u64>,
}

impl DataStageDeadlines {
    pub(crate) const fn new() -> Self {
        Self {
            submit: None,
            completion: None,
            reclaim: None,
        }
    }
}

/// Stable diagnostic code for a [`DevError`].
///
/// The codes are explicit and never derived from the enum discriminant, so
/// they stay stable across dependency updates. Do not renumber them.
pub fn rx_error_code(err: &DevError) -> u64 {
    crate::readiness::dev_error_code(err)
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
    /// F2: frozen structured summary of the most recent recovery/ownership
    /// fault, read in one pass so a snapshot never combines the stage from one
    /// fault with the owner from another. Each value is the count observed at
    /// the fault commit. Internal only: these MUST NOT serialize into the
    /// V1–V3 wire snapshot (frozen ABI); they are diagnostic-only.
    pub recover_fault_stage: AtomicU64,
    pub recover_fault_epoch: AtomicU64,
    pub recover_available: AtomicU64,
    pub recover_device_owned: AtomicU64,
    pub recover_quarantined: AtomicU64,
    /// F2: the origin stage (submit wait / completion wait / reclaim /
    /// unknown) of the fault that triggered the resident recovery, preserved
    /// so a later quiesce/reset failure still records why the owner entered
    /// recovery. Internal diagnostic only; never serialized to the V1–V3 ABI.
    pub recover_origin_stage: AtomicU64,
    /// A4 / D5: coherent single-value publication of the most recent recovery
    /// fault identity (stage, local cause, queue epoch, owner summary),
    /// committed under the Service guard and read race-free by [`read_identity`].
    pub coherent_fault: CoherentFaultSheet,
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
            recover_fault_stage: AtomicU64::new(0),
            recover_fault_epoch: AtomicU64::new(0),
            recover_available: AtomicU64::new(0),
            recover_device_owned: AtomicU64::new(0),
            recover_quarantined: AtomicU64::new(0),
            recover_origin_stage: AtomicU64::new(0),
            coherent_fault: CoherentFaultSheet::new(),
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
    rx_snapshot_v3_from(rx_snapshot(), ServiceAccess::Global)
}

/// Append-only recovery state consumed by the QEMU-only V4 kernel snapshot.
/// Current owner state and the last historical fault have intentionally
/// separate validity bits and tuples: they are coherent independently, but
/// are not asserted to describe the same instant.
#[cfg(feature = "qemu-diagnostics")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoverySnapshotV4 {
    pub current_valid: u64,
    pub current_queue_epoch: u64,
    pub current_socket_epoch: u64,
    pub current_link_generation: u64,
    pub current_link_state: u64,
    pub current_owner_available: u64,
    pub current_owner_device_owned: u64,
    pub current_owner_quarantined: u64,
    pub fault_valid: u64,
    pub fault_stage: u64,
    pub fault_cause: u64,
    pub fault_queue_epoch: u64,
    pub fault_owner_available: u64,
    pub fault_owner_device_owned: u64,
    pub fault_owner_quarantined: u64,
}

#[cfg(feature = "qemu-diagnostics")]
pub fn recovery_snapshot_v4() -> RecoverySnapshotV4 {
    recovery_snapshot_v4_from(ServiceAccess::Global)
}

/// Reads the current V4 identity tuple from a [`Service`] under one guard:
/// queue epoch, socket epoch, link generation/state and the live owner ledger.
#[cfg(feature = "qemu-diagnostics")]
fn read_v4_current(service: &mut Service) -> (u64, u64, u64, u64, u64, u64, u64) {
    let owner = service.recovery_owner_summary_target();
    (
        service.queue_epoch_target().current(),
        service.socket_epoch(),
        service.link_generation(),
        service.link_state_code(),
        owner.available,
        owner.device_owned,
        owner.quarantined,
    )
}

/// Injectable V4 assembly seam (A2 rework): the current queue/socket/link/owner
/// tuple is read under a SINGLE Service guard from the supplied access; the
/// historical coherent fault is read independently. The two tuples are valid
/// and coherent per-side and must never be read as one instant. A missing
/// Service publishes `current_valid = 0` (no forged healthy values).
#[cfg(feature = "qemu-diagnostics")]
pub(crate) fn recovery_snapshot_v4_from(access: ServiceAccess) -> RecoverySnapshotV4 {
    let current = match access {
        ServiceAccess::Global => crate::SERVICE.get().map(|mutex| {
            let mut guard = mutex.lock();
            read_v4_current(&mut guard)
        }),
        #[cfg(test)]
        ServiceAccess::Injected(mutex) => {
            let mut guard = mutex.lock();
            Some(read_v4_current(&mut guard))
        }
    };
    let (
        current_valid,
        current_queue_epoch,
        current_socket_epoch,
        current_link_generation,
        current_link_state,
        current_owner_available,
        current_owner_device_owned,
        current_owner_quarantined,
    ) = match current {
        Some((q, s, l, ls, avail, owned, quar)) => (1, q, s, l, ls, avail, owned, quar),
        None => (0, 0, 0, 0, 0, 0, 0, 0),
    };
    let fault = RX_TELEMETRY.coherent_fault.read();
    let (
        fault_valid,
        fault_stage,
        fault_cause,
        fault_queue_epoch,
        fault_owner_available,
        fault_owner_device_owned,
        fault_owner_quarantined,
    ) = fault
        .map(|fault| {
            (
                1,
                fault.stage,
                fault.local_cause,
                fault.queue_epoch,
                fault.available,
                fault.device_owned,
                fault.quarantined,
            )
        })
        .unwrap_or_default();
    RecoverySnapshotV4 {
        current_valid,
        current_queue_epoch,
        current_socket_epoch,
        current_link_generation,
        current_link_state,
        current_owner_available,
        current_owner_device_owned,
        current_owner_quarantined,
        fault_valid,
        fault_stage,
        fault_cause,
        fault_queue_epoch,
        fault_owner_available,
        fault_owner_device_owned,
        fault_owner_quarantined,
    }
}

/// C5/T4.4-R2 shared V3 assembly seam: builds the V3 payload from a V2 base
/// snapshot plus a Service access. The public entry and host tests with an
/// injected Service execute this same path, so a regression to a synthetic
/// or cross-state tuple is witnessed once. The lease tuple (mode, expiry,
/// failure counter) is copied from the committed Service under the SAME
/// guard as the ledger, so a control or tick between two observations can
/// never form a synthetic or cross-generation tuple in the V3 payload. Only
/// a missing (pre-init) Service uses the all-zero fallback.
pub(crate) fn rx_snapshot_v3_from(base: RxSnapshot, service: ServiceAccess) -> RxSnapshotV3 {
    let (
        ledger,
        tx_ledger,
        flush_target,
        flush_counters,
        drop_reasons,
        hold_mode,
        lease_expiry,
        auto_release_failure,
    ) = match service {
        ServiceAccess::Global => match crate::SERVICE.get() {
            Some(mutex) => {
                let mut guard = mutex.lock();
                read_v3_ledger_and_lease(&mut guard)
            }
            None => default_v3_ledger_and_lease(),
        },
        #[cfg(test)]
        ServiceAccess::Injected(mutex) => {
            let mut guard = mutex.lock();
            read_v3_ledger_and_lease(&mut guard)
        }
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
        lifecycle: base.lifecycle,
        owner: base.owner,
        isr_publish: base.isr_publish,
        isr_wake: base.isr_wake,
        software_nudge: base.software_nudge,
        task_poll: base.task_poll,
        reaped: base.reaped,
        refilled: base.refilled,
        delivered: base.delivered,
        non_ip_consumed: base.non_ip_consumed,
        budget_exhausted: base.budget_exhausted,
        self_yield: base.self_yield,
        router_full_wait: base.router_full_wait,
        space_wake: base.space_wake,
        empty_check: base.empty_check,
        fault: base.fault,
        last_error_stage: base.last_error_stage,
        last_error_code: base.last_error_code,
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
        hold_mode,
        lease_expiry,
        auto_release_failure,
        lifecycle_fault: RX_TELEMETRY.lifecycle_fault.load(Ordering::Relaxed),
        ownership_invariant: RX_TELEMETRY.ownership_invariant.load(Ordering::Relaxed),
        drop_malformed_ip: drop_reasons[0],
        drop_no_route: drop_reasons[1],
        drop_route_source_mismatch: drop_reasons[2],
        drop_unsupported_address: drop_reasons[3],
        drop_frame_too_large: drop_reasons[4],
    }
}

/// Reads the V3 ledger and lease tuple from a locked Service in one guard.
fn read_v3_ledger_and_lease(service: &mut Service) -> V3LedgerAndLease {
    let ledger = service.v3_slot_ledger();
    // RW-2: the real driver buffer/descriptor ledger, not a synthesis from
    // slot or ticket capacities.
    let tx_ledger = service.v3_tx_resource_ledger();
    let target = service.v3_flush_target();
    let counters = service.v3_flush_counters();
    let drops = service.v3_drop_reasons();
    #[cfg(feature = "qemu-diagnostics")]
    let lease = (
        service.diag_hold_mode(),
        service.diag_lease_expiry(),
        service.diag_auto_release_failure(),
    );
    #[cfg(not(feature = "qemu-diagnostics"))]
    let lease = (0u64, 0u64, 0u64);
    (
        ledger, tx_ledger, target, counters, drops, lease.0, lease.1, lease.2,
    )
}

/// Pre-init fallback: a missing Service reports zeros, never a synthetic
/// lease tuple that could be mistaken for a committed no-hold.
fn default_v3_ledger_and_lease() -> V3LedgerAndLease {
    #[cfg(feature = "qemu-diagnostics")]
    let hold_none = crate::diag::HOLD_NONE;
    #[cfg(not(feature = "qemu-diagnostics"))]
    let hold_none = 0u64;
    (
        crate::device::SlotLedger::default(),
        None,
        u64::MAX,
        [0; 4],
        [0; 5],
        hold_none,
        0u64,
        0u64,
    )
}

/// Ledger and lease tuple read under one Service guard for the V3 payload.
type V3LedgerAndLease = (
    crate::device::SlotLedger,
    Option<axdriver_net::TxResourceLedger>,
    u64,
    [u64; 4],
    [u64; 5],
    u64,
    u64,
    u64,
);

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

/// Publish a config-change queue event (Task 3.1 / R6). Called by the ISR path
/// after ACK when the config-change cause bit is set; the owner wakes and reads
/// a consistent link snapshot at most once per poll. It sets only the config
/// cause, so a combined used+config interrupt keeps both flags independent.
pub fn publish_config_event() {
    RX_TELEMETRY.isr_publish.fetch_add(1, Ordering::Relaxed);
    RX_TELEMETRY.isr_wake.fetch_add(1, Ordering::Relaxed);
    QUEUE_EVENT.publish_config();
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

    /// Blocking acquisition for the sole owner (queue task, flush): the
    /// caller waits for the brief stack hold instead of returning early, so
    /// a lost lock never silently delays queue or flush progress. `None`
    /// only when the global Service is not initialized.
    pub(crate) fn lock(&self) -> Option<ServiceGuard<'_>> {
        match self {
            Self::Global => crate::SERVICE.get().map(|m| ServiceGuard::Global(m.lock())),
            #[cfg(test)]
            Self::Injected(m) => Some(ServiceGuard::Injected(m.lock())),
        }
    }
}

/// C2/T4.4-R1 shared bounded control path: one nonblocking Service
/// acquisition, a checked commit under the guard, unlock, then exactly one
/// queue-work publication. Both the production [`diagnostic_control`]
/// (crate::diagnostic_control) entry and host tests with an injected Service
/// execute this path, so Busy / validation-error / success event ordering is
/// witnessed once. A missing global Service is `BadState`; a held Service is
/// `ResourceBusy` and changes neither state nor event generation.
#[cfg(feature = "qemu-diagnostics")]
pub(crate) fn diagnostic_control_shared(
    service: ServiceAccess,
    notify: &QueueEvent,
    op: u64,
    lease_ms: u64,
) -> DevResult {
    let mut guard = match service {
        ServiceAccess::Global => ServiceGuard::Global(
            crate::SERVICE
                .get()
                .ok_or(DevError::BadState)?
                .try_lock()
                .ok_or(DevError::ResourceBusy)?,
        ),
        #[cfg(test)]
        ServiceAccess::Injected(mutex) => {
            ServiceGuard::Injected(mutex.try_lock().ok_or(DevError::ResourceBusy)?)
        }
    };
    let now = guard.diag_now();
    guard.diag_control(op, lease_ms, now)?;
    drop(guard);
    notify.publish_queue_work();
    Ok(())
}

/// QEMU-only reset control: atomically queue one request for the resident
/// owner. The syscall path merely commits this event and wakes that owner; it
/// never accesses a transport or performs recovery itself.
#[cfg(feature = "qemu-diagnostics")]
pub(crate) fn recovery_reset_request_shared() -> DevResult {
    if crate::SERVICE.get().is_none() {
        return Err(DevError::BadState);
    }
    RECOVERY_RESET_REQUEST.lock().request(RX_LIFECYCLE.load())?;
    QUEUE_EVENT.publish_queue_work();
    Ok(())
}

#[cfg(feature = "qemu-diagnostics")]
fn claim_recovery_reset_request() -> bool {
    RECOVERY_RESET_REQUEST.lock().claim(RX_LIFECYCLE.load())
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
    stack_notify: &'static StackEvent,
    stack_progress_pending: bool,
    /// P2 / R6: set when the owner has activated but has NOT yet committed a
    /// consistent initial link snapshot. Cleared once the first
    /// `link_policy_step_target` resolves (Up/Down/Unsupported/Fault/NoEvent);
    /// retained on `Again` so the next bounded poll retries.
    initial_link_pending: bool,
    telemetry: &'static RxTelemetry,
    /// Task 3.1: publication target for terminal queue faults. Production
    /// points at the global socket registry; tests inject a local wrapper.
    fault_sink: &'static SocketSetWrapper<'static>,
    /// C4: armed QEMU diagnostic lease deadline (wall nanos) the owner is
    /// sleeping on. The timer is wake-only: it carries no generation, so a
    /// stale wake costs at most one bounded poll and the current Service
    /// lease decides whether to remain held and which deadline is rearmed.
    #[cfg(feature = "qemu-diagnostics")]
    lease_deadline: Option<u64>,
    /// Task 5.2 (Iteration 006): per-test fixture clock shared with the
    /// injected Service. `lease_deadline` decisions read this when attached;
    /// production never sets it (wall clock).
    #[cfg(all(test, feature = "qemu-diagnostics"))]
    diag_test_clock: Option<crate::diag::DiagTestClock>,
    /// C4: axtask timer that wakes the queue owner at `lease_deadline`.
    /// Production only; host tests drive the fake clock and re-poll instead.
    #[cfg(all(feature = "qemu-diagnostics", not(test)))]
    lease_timer: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
    /// Task 2.2: in-progress staged device recovery driven by this resident
    /// owner. `None` means the data plane is not recovering. When `Some`, the
    /// owner reuses this future across polls and never exits until recovery
    /// commits or the owner is permanently quarantined.
    recovery: Option<RecoveryState>,
    /// Task 2.2: monotonic deadline (nanos) for the current recovery stage.
    /// The owner quarantines when the stage does not progress past it.
    recovery_deadline: Option<u64>,
    /// Cycle 005 / T4.2-R1: the next bounded one-shot wake instant (nanos) the
    /// owner should be awakened at while a reset/reinitialize stage is Pending.
    /// It is `min(now + RECOVERY_PROGRESS_CADENCE_NS, recovery_deadline)`, so a
    /// delayed driver reset gets a deadline-bounded cadence of driver-step
    /// retries strictly before the absolute deadline, without a busy poll and
    /// without renewing the deadline. `None` when recovery is not in a
    /// reset/reinitialize stage or one is not currently armed.
    recovery_progress_wake: Option<u64>,
    /// Task 2.2: per-test recovery clock. Production never sets it (wall clock).
    #[cfg(all(test))]
    recovery_test_clock: Option<crate::recovery::RecoveryTestClock>,
    /// Task 2.2: axtask timer that wakes the owner at the current recovery
    /// stage deadline (production only; host tests drive the fake clock).
    #[cfg(all(not(test)))]
    recovery_timer: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
    /// Task 2.2: armed absolute deadlines for the three concurrent Active
    /// data-stage waits (submit / completion / reclaim). Each is armed once on
    /// first observation and cleared when it resolves; `None` means inactive.
    data_deadlines: DataStageDeadlines,
    /// Task 2.2 / A1–A3: axtask timer that wakes the owner at the earliest
    /// active data-stage deadline. Production only; host tests drive the fake
    /// recovery clock and re-poll instead.
    #[cfg(not(test))]
    data_stage_timer: Option<Pin<Box<dyn Future<Output = ()> + Send>>>,
}

/// A stage of the device-recovery flow the resident queue owner is driving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryState {
    /// Recovery requested; the next poll cancels Queued tickets and begins the
    /// bounded quiesce reclaim before any reset.
    Quiescing,
    /// Driver reset in progress (`Resetting`), under the 2 s reset deadline.
    Resetting,
    /// Queues/backing being rebuilt after a confirmed reset (driver
    /// `Reinitializing`), under the 2 s reinitialize deadline.
    Reinitializing,
    /// Recovery failed or a stage deadline exceeded: the owner is quarantined
    /// resident in `Faulted`, holds the I/O gate and never resumes stepping.
    Faulted,
}

/// Per-poll outcome of the resident recovery loop.
#[derive(Debug)]
enum RecoveryRound {
    /// Recovery committed; the owner can resume the Active service loop.
    Finished,
    /// Still recovering; the future stays resident.
    Pending,
    /// A recovery step failed; the owner must quarantine after the driver
    /// recovery fault. The error is committed+published only after the Service
    /// guard is dropped (F5: never wake under a held guard).
    Fault(DevError),
}

/// Quiesce/data-stage deadline in nanoseconds (Task 2.2: 1 s for the
/// submit/completion/reclaim and quiesce window before the reset starts).
const QUIESCE_STAGE_DEADLINE_NS: u64 = 1_000_000_000;

/// Reset/reinitialize stage deadline in nanoseconds (Task 2.2: 2 s for the
/// device-reset and queue-rebuild stages).
const RESET_STAGE_DEADLINE_NS: u64 = 2_000_000_000;

/// Cycle 005 / T4.2-R1: bounded cadence between retries of a Pending
/// reset/reinitialize driver step. The resident owner re-arms a one-shot
/// axtask wake at `min(now + this cadence, absolute stage deadline)` so a
/// delayed reset can fully confirm within its 2 s window strictly before the
/// deadline, without a busy poll and without renewing the deadline.
const RECOVERY_PROGRESS_CADENCE_NS: u64 = 10_000_000;

/// Outcome of one RX servicing round before releasing the guard.
enum RoundOutcome {
    /// A self-wake plus Pending is required (visible backlog remains).
    SelfWakeYield,
    /// Run the empty-queue register/arm/recheck protocol.
    RegisterRecheck,
    /// Wait for a resource release (slot space or Router space), possibly
    /// retrying.
    WaitSpace(SpaceDecision),
    /// C4: sleep purely on the QEMU diagnostic lease deadline (wall nanos).
    /// The held stage's completion must not drive the arm/recheck protocol
    /// (it would retry forever); the only exits are the lease timer or an
    /// explicit Release publishing queue work.
    #[cfg(feature = "qemu-diagnostics")]
    SleepUntil(u64),
    /// Terminal queue/device fault.
    Fault(DevError),
    /// A recoverable data-plane fault on a recovery-capable device: the owner
    /// must stay resident and drive the staged device recovery instead of
    /// exiting and dropping the RX owner. Carries the D3 fault-origin stage
    /// (submit-wait / completion-wait / reclaim) so the fault summary is
    /// diagnosed at the stage that actually failed (F2).
    Recover(DevError, u64),
    /// An ownership/identity/ledger drift (`BadState`): D3 forbids masking a
    /// corrupt ledger with a reset, so the owner quarantines resident in
    /// `Faulted` without calling driver recovery (F4).
    Drift(DevError),
    /// Task 2.2 / A1: a Queued submit wait hit its 1 s absolute deadline. The
    /// stuck slot+ticket were cancelled exactly once and the flush aborted
    /// stably under the guard; the owner stays Active (a full driver is not an
    /// ownership corruption) and commits the `SubmitWait + Timeout` fault after
    /// the guard drops.
    SubmitTimeout(DevError),
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
    fn service_round(&mut self, service: &mut Service) -> RoundOutcome {
        self.stack_progress_pending = false;
        // QEMU diagnostic hold (D9): a hold pauses exactly one stage of the
        // sole queue owner. The lease is Service-owned and advanced once per
        // round under the Service guard; an expired lease auto-releases and
        // counts a failure. No lease generation exists, so no identity can
        // exhaust and no reachable Hold is ever permanent.
        #[cfg(feature = "qemu-diagnostics")]
        let hold = service.diag_hold_tick();
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
                        return self.classify_fault(service, err, recover_stage::RECLAIM);
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
                    self.stack_progress_pending = true;
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
                    return self.classify_fault(service, err, recover_stage::COMPLETION_WAIT);
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
                        self.stack_progress_pending = true;
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
                        return self.classify_fault(service, err, recover_stage::SUBMIT_WAIT);
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
                // F2: a completion-query failure is not itself a bounded
                // recovery stage; classify it as unclassified (UNKNOWN) so the
                // recoverable path still carries a stable stage in the summary.
                return self.classify_fault(service, err, recover_stage::UNKNOWN);
            }
        };
        let tx_pending = service.tx_slot_pending_target();
        // RW-1: a visible TX completion is consumed by the reclaim stage;
        // under a reclaim hold it can never advance, so it must not
        // self-wake. TX slots are consumed by submit; under a submit hold
        // (`submit_full`) they cannot advance either.
        let tx_completion_advanceable = pending.contains(NetQueueDirection::TX) && !reclaim_held;
        let tx_slot_advanceable = tx_pending && !submit_full;
        // Task 2.2 / A1–A3: arm/clear the three concurrent data-stage deadlines
        // and, if any still-blocked wait has elapsed past its absolute deadline,
        // act on it now (returning early).
        // Find 4: this runs even while a QEMU diagnostic hold is active, so the
        // hold's lease (> 1 s) only blocks the held stage itself — it cannot
        // shield that stage's 1 s data deadline. A held reclaim/submit reads as
        // a stall (`reclaimed == 0` / `submit_held`), so the data deadline fires
        // before the lease; a data deadline with no real wait stays unarmed and
        // the hold falls through to its own lease sleep below.
        if let Some(outcome) = self.arm_and_handle_data_deadlines(
            service,
            pending,
            submit_full,
            tx_pending,
            submit_held,
            reclaim_held,
            reclaimed,
        ) {
            return outcome;
        }
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
            // C4: a hold lease is active and the held stage blocks the
            // remaining work. Sleep until the lease deadline; never self-wake
            // and never run the register/arm/recheck protocol on a held
            // completion (it would retry forever). The deadline timer only
            // wakes the owner; `diag_hold_tick` on the next round performs
            // the release and failure counter. The expiry comes from the
            // committed Service lease under the same guard, so it is always
            // the deadline of the lease that armed it.
            #[cfg(feature = "qemu-diagnostics")]
            {
                let expiry = service.diag_lease_expiry();
                RoundOutcome::SleepUntil(expiry)
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
        let Some(mut service) = self.service.lock() else {
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
            // P2 / R6: the resident owner must commit a consistent initial link
            // snapshot in task context on activation, independent of any
            // hardware CONFIG IRQ cause (a configuration-change interrupt may
            // never fire until the link later flaps).
            self.initial_link_pending = true;
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
        let access = self.service;
        let Some(mut service) = access.lock() else {
            return Poll::Pending;
        };
        #[cfg(feature = "qemu-diagnostics")]
        if claim_recovery_reset_request() {
            // The ioctl only queued an event.  This resident owner is the sole
            // context allowed to enter the driver recovery state machine.
            drop(service);
            self.enter_recovery(&DevError::Io, recover_stage::EXPLICIT_REQUEST);
            return self.poll_recovery(cx);
        }
        // Task 3.1 / R6: bounded config micro-step. Take the cause flags once
        // per poll and, on a pending CONFIG cause or an unresolved initial-link
        // flag, read a consistent link snapshot at most once. A transient
        // `Again` retains the cause (a re-publish bumps the generation, so
        // whichever sleep path follows observes the change and retries) and
        // keeps the initial-link flag for the next bounded poll. A link down/up
        // publishes stack progress after the round so readiness re-evaluates.
        let causes = self.notify.take_causes();
        let mut link_change = false;
        if causes.config || self.initial_link_pending {
            match service.link_policy_step_target() {
                LinkStep::Again => {
                    // Retain the retry work: re-publish the CONFIG cause to
                    // self-wake. An initial-link `Again` keeps its pending flag
                    // (only the non-Again arms below clear it), so the next
                    // bounded poll retries the very first snapshot.
                    self.notify.publish_config();
                }
                LinkStep::Down | LinkStep::Up => {
                    self.initial_link_pending = false;
                    link_change = true;
                }
                LinkStep::NoEvent | LinkStep::Unsupported | LinkStep::Fault => {
                    self.initial_link_pending = false;
                }
            }
        }
        let outcome = self.service_round(&mut service);
        let socket_epoch_wake = service.take_socket_epoch_wake();
        #[cfg(feature = "qemu-diagnostics")]
        let waiting_lease_expiry = if matches!(&outcome, RoundOutcome::WaitSpace(_)) {
            service.diag_lease_expiry()
        } else {
            0
        };
        drop(service);
        if let Some((registry, epoch)) = socket_epoch_wake {
            registry.wake_socket_epoch(epoch);
        }
        if core::mem::take(&mut self.stack_progress_pending) || link_change {
            self.notify.publish_progress();
            self.stack_notify.publish_device();
        }
        match outcome {
            RoundOutcome::SelfWakeYield => {
                // Not a lease sleep: cancel any stale deadline so an explicit
                // Release invalidates the old timer (RW-1).
                #[cfg(feature = "qemu-diagnostics")]
                self.cancel_lease_deadline();
                self.cancel_data_stage_timer();
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            RoundOutcome::WaitSpace(SpaceDecision::Retry) => {
                #[cfg(feature = "qemu-diagnostics")]
                self.cancel_lease_deadline();
                self.cancel_data_stage_timer();
                cx.waker().wake_by_ref();
                Poll::Pending
            }
            RoundOutcome::WaitSpace(SpaceDecision::Waiting) => {
                // C4: while waiting for RX slot space the lease may also
                // expire; arm the deadline so an expired hold still
                // auto-releases without an external NIC event. A 0 deadline
                // (no hold) cancels any stale one. The expiry is read from
                // the committed Service lease while the guard is still held.
                #[cfg(feature = "qemu-diagnostics")]
                self.arm_lease_deadline(cx, waiting_lease_expiry);
                self.arm_data_stage_timer(cx);
                Poll::Pending
            }
            RoundOutcome::RegisterRecheck => {
                #[cfg(feature = "qemu-diagnostics")]
                self.cancel_lease_deadline();
                let poll = self.poll_register_recheck(cx);
                if poll.is_pending() {
                    self.arm_data_stage_timer(cx);
                }
                poll
            }
            #[cfg(feature = "qemu-diagnostics")]
            RoundOutcome::SleepUntil(deadline) => {
                self.arm_lease_deadline(cx, deadline);
                // Find 4: keep a concurrent data-stage deadline armed so a held
                // stage whose 1 s data deadline is earlier than the lease still
                // wakes the owner (via the data timer) instead of waiting out
                // the whole lease. If no data deadline is armed, this is a no-op.
                self.arm_data_stage_timer(cx);
                Poll::Pending
            }
            RoundOutcome::Fault(err) => {
                // Task 3.7: commit `Active -> Faulted` first, publish only on
                // success, so a woken stack waiter observes Faulted. This is
                // the non-recovery terminal path (unreachable-owner device,
                // driver without recovery support).
                self.publish_fatal(&err);
                Poll::Ready(())
            }
            RoundOutcome::Recover(err, stage) => {
                // Task 2.2 resident-recovery path: the device exposes a
                // bounded recovery control, so the owner must not exit and
                // drop the RX owner. Drive the staged recovery loop.
                self.enter_recovery(&err, stage);
                self.poll_recovery(cx)
            }
            RoundOutcome::Drift(err) => {
                // F4/D3: an ownership/identity/ledger drift on a
                // recovery-capable device must NOT be masked by a reset. The
                // owner quarantines resident in `Faulted`, holds the gate and
                // never calls driver recovery.
                self.enter_drift_quarantine(&err);
                self.poll_recovery(cx)
            }
            RoundOutcome::SubmitTimeout(err) => {
                // A1: the Queued slot+ticket were cancelled and the flush was
                // aborted under the guard. Commit the `SubmitWait + Timeout`
                // fault identity and wake the flush waiter now, outside the
                // guard, then continue the Active loop (the owner is not
                // quarantined).
                self.freeze_recovery_summary(recover_stage::SUBMIT_WAIT, fault_cause::TIMEOUT);
                self.telemetry
                    .record_fault(rx_error_stage::RECEIVE_RECYCLE, &err);
                let _ = self.service.lock().map(|s| s.flush_wake_pending());
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    /// Task 2.2: classifies a data-plane fault. A recovery-capable device keeps
    /// the resident owner alive and drives recovery; an unrecoverable device
    /// keeps the historical terminal-exit path. F4/D3: an ownership/identity/
    /// ledger drift (`BadState`) must NOT be masked by a reset — the owner
    /// quarantines resident in `Faulted` without calling driver recovery.
    fn classify_fault(&self, service: &mut Service, err: DevError, stage: u64) -> RoundOutcome {
        if matches!(err, DevError::BadState) {
            if service.target_can_recover() {
                RoundOutcome::Drift(err)
            } else {
                RoundOutcome::Fault(err)
            }
        } else if service.target_can_recover() {
            RoundOutcome::Recover(err, stage)
        } else {
            RoundOutcome::Fault(err)
        }
    }

    /// Task 2.2 / A1–A3: arms, clears and enforces the three concurrent Active
    /// data-stage deadlines. Each wait is armed exactly once (to
    /// `now + QUIESCE_STAGE_DEADLINE_NS`) the first time its blocking condition
    /// holds, and cleared when the condition resolves, so a same-stage pending
    /// or repeated poll never renews it. When a still-blocked wait has elapsed
    /// past its absolute deadline this round, the timeout action runs and the
    /// resulting `RoundOutcome` is returned; otherwise `None`.
    ///
    /// Callers hold the Service guard. The fault *commit* for a submit timeout
    /// is deferred to the guard-dropped handler (the deadline data is only
    /// observed here), matching F5's commit-before-wake ordering. Recoverable
    /// completion/reclaim timeouts return `Recover`, whose `enter_recovery`
    /// preserves the origin stage for the eventual fault summary.
    fn arm_and_handle_data_deadlines(
        &mut self,
        service: &mut Service,
        pending: NetQueueDirection,
        submit_full: bool,
        tx_pending: bool,
        submit_held: bool,
        reclaim_held: bool,
        reclaimed: usize,
    ) -> Option<RoundOutcome> {
        let now = self.recovery_now();
        let owned = service.device_owned_len_target();
        let tx_completion_advanceable = pending.contains(NetQueueDirection::TX) && !reclaim_held;
        let submit_blocked =
            (submit_full || submit_held) && tx_pending && !tx_completion_advanceable;
        let completion_blocked = owned > 0 && !pending.contains(NetQueueDirection::TX);
        // A3 / Find 3: a reclaim wait is only a stall when a DeviceOwned owner
        // exists, a TX completion is visible AND nothing was reclaimed this round.
        // Sustained progress (`reclaimed > 0`) clears the deadline; zero owners
        // (`owned == 0`) never starts one. Under a reclaim hold the reclaim loop is
        // skipped so `reclaimed == 0`, reading the held stage as a genuine stall.
        let reclaim_blocked =
            owned > 0 && pending.contains(NetQueueDirection::TX) && reclaimed == 0;

        if submit_blocked {
            self.data_deadlines
                .submit
                .get_or_insert(now + QUIESCE_STAGE_DEADLINE_NS);
        } else {
            self.data_deadlines.submit = None;
        }
        if completion_blocked {
            self.data_deadlines
                .completion
                .get_or_insert(now + QUIESCE_STAGE_DEADLINE_NS);
        } else {
            self.data_deadlines.completion = None;
        }
        if reclaim_blocked {
            self.data_deadlines
                .reclaim
                .get_or_insert(now + QUIESCE_STAGE_DEADLINE_NS);
        } else {
            self.data_deadlines.reclaim = None;
        }

        if submit_blocked && self.data_deadlines.submit.is_some_and(|d| now >= d) {
            let err = DevError::Io;
            // A1: cancel the Queued slot+ticket exactly once and abort the
            // flush waiter stably, all under the guard; the owner does NOT
            // quarantine (a full driver is not ownership corruption).
            service.tx_cancel_queued_target();
            service.flush_recovery_abort_all(&err);
            self.data_deadlines.submit = None;
            return Some(RoundOutcome::SubmitTimeout(err));
        }
        if completion_blocked && self.data_deadlines.completion.is_some_and(|d| now >= d) {
            // A2: a DeviceOwned completion did not arrive within the deadline;
            // enter resident recovery so the driver-stage deadlines bound it.
            self.data_deadlines.completion = None;
            return Some(RoundOutcome::Recover(
                DevError::Io,
                recover_stage::COMPLETION_WAIT,
            ));
        }
        if reclaim_blocked && self.data_deadlines.reclaim.is_some_and(|d| now >= d) {
            // A3: a visible TX completion could not be reclaimed within the
            // deadline; enter resident recovery. An ownership drift already
            // returned earlier via `classify_fault` on the synchronous path.
            self.data_deadlines.reclaim = None;
            return Some(RoundOutcome::Recover(DevError::Io, recover_stage::RECLAIM));
        }
        None
    }

    /// Task 2.2: a recoverable data-plane fault begins the resident recovery
    /// phase. The owner gates the I/O path, commits `Active -> Quiescing`,
    /// publishes the pending quarantine to waiters, then drives the staged
    /// recovery across polls. Called once, right after the faulting round.
    fn enter_recovery(&mut self, err: &DevError, origin_stage: u64) {
        // Find 2: gate the TX enqueue before any recovery window opens so no
        // new Queued ticket enters a data plane being cleared.
        self.set_recovery_hold(true);
        #[cfg(feature = "qemu-diagnostics")]
        let lifecycle_transition =
            with_recovery_request_transition(&RECOVERY_RESET_REQUEST, || {
                self.lifecycle.begin_recovery()
            });
        #[cfg(not(feature = "qemu-diagnostics"))]
        let lifecycle_transition = self.lifecycle.begin_recovery();
        if !lifecycle_transition.is_ok() {
            self.telemetry.record_last_error_code(
                rx_error_stage::LIFECYCLE,
                self.lifecycle.load().code() as u64,
            );
            self.telemetry
                .lifecycle_fault
                .fetch_add(1, Ordering::Relaxed);
        }
        self.recovery = Some(RecoveryState::Quiescing);
        self.recovery_deadline = None;
        self.recovery_progress_wake = None;
        // F2: the origin stage of the fault that triggered recovery (submit
        // wait / completion wait / reclaim) is preserved for the fault
        // summary, so a later quiesce/reset failure still records why the
        // owner entered recovery.
        self.telemetry
            .recover_origin_stage
            .store(origin_stage, Ordering::Relaxed);
        self.telemetry
            .record_fault(rx_error_stage::RECEIVE_RECYCLE, err);
        // A recoverable reset closes only the current SocketEpoch. The
        // resident owner will open a fresh epoch after the driver reports
        // Recovered; old handles therefore remain ConnectionReset forever.
        let epoch = self.fault_sink.current_socket_epoch();
        self.publish_fault_epoch_terminal(
            epoch,
            crate::readiness::NetworkTerminal::ConnectionReset.code(),
        );
        self.notify.publish_progress();
        self.stack_notify.publish_device();
        self.cancel_recovery_timer();
    }

    /// F4/D3: quarantines a recovery-capable owner on an ownership/identity/
    /// ledger drift without calling driver recovery. Holds the I/O gate,
    /// commits `Faulted` from the active state, and makes the future resident
    /// so the drifting owner never resumes stepping and never resets.
    fn enter_drift_quarantine(&mut self, err: &DevError) {
        // F4: close the DeviceOwned ledger as `Fault` WITHOUT releasing the
        // driver backing (the recovery holder keeps it quarantined), so a new
        // flush on the faulted owner fails stably instead of pending forever.
        // The ledger and flush commit happen under a brief guard; the waiter
        // wake happens after the guard is dropped (F5).
        if let Some(mut service) = self.service.lock() {
            service.tx_set_recovery_hold_target(true);
            service.tx_cancel_queued_target();
            service.tx_cancel_pending_target();
            service.tx_fault_device_owned_target(crate::device::TicketFaultStage::OwnershipDrift);
            service.flush_recovery_abort_all(err);
        }
        // F2: commit `Faulted` first, then freeze the summary, so
        // `freeze_recovery_summary` observes the Faulted lifecycle and records
        // the real software ticket epoch instead of `u64::MAX`.
        // A1 rework: absorb any pending/claimed request on the Active->Faulted seam.
        #[cfg(feature = "qemu-diagnostics")]
        let lifecycle_transition =
            with_recovery_request_transition(&RECOVERY_RESET_REQUEST, || {
                self.lifecycle.recover_fault()
            });
        #[cfg(not(feature = "qemu-diagnostics"))]
        let lifecycle_transition = self.lifecycle.recover_fault();
        if !lifecycle_transition.is_ok() {
            self.telemetry.record_last_error_code(
                rx_error_stage::LIFECYCLE,
                self.lifecycle.load().code() as u64,
            );
            self.telemetry
                .lifecycle_fault
                .fetch_add(1, Ordering::Relaxed);
        }
        self.recovery = Some(RecoveryState::Faulted);
        // F2: an ownership/identity drift is its own structured fault stage,
        // distinct from a recoverable reset-stage failure.
        self.freeze_recovery_summary(recover_stage::OWNERSHIP_DRIFT, fault_cause::OWNERSHIP_DRIFT);
        self.recovery_deadline = None;
        self.recovery_progress_wake = None;
        self.telemetry
            .record_fault(rx_error_stage::RECEIVE_RECYCLE, err);
        let epoch = self.fault_sink.current_socket_epoch();
        self.publish_fault_epoch_terminal(epoch, crate::readiness::dev_error_code(err));
        self.notify.publish_progress();
        self.stack_notify.publish_device();
        self.cancel_recovery_timer();
        // F5: lifecycle resolved; wake the flush waiter only now, outside any
        // Service guard.
        let _ = self.service.lock().map(|s| s.flush_wake_pending());
    }

    /// Sets or clears the recovery I/O gate on the underlying device (Find 2).
    /// Locks the Service briefly; safe because no guard is held when the
    /// owner calls this.
    fn set_recovery_hold(&mut self, held: bool) {
        if let Some(mut service) = self.service.lock() {
            service.tx_set_recovery_hold_target(held);
        }
    }

    /// The clock used for recovery-deadline decisions: the attached per-test
    /// fixture clock when present, else the wall monotonic clock.
    fn recovery_now(&self) -> u64 {
        #[cfg(test)]
        {
            if let Some(clock) = self.recovery_test_clock {
                return clock.load();
            }
        }
        crate::recovery::recovery_now()
    }

    /// Task 2.2: drives the staged device recovery as the resident owner. Each
    /// poll performs at most one bounded stage transition and reclaims a
    /// bounded number of quiesce completions, then returns `Pending`; the
    /// future stays alive across `Quiescing / Resetting / Reinitializing /
    /// Faulted` and never exits until recovery commits or the owner is
    /// quarantined. Each stage deadline is an absolute instant armed once on
    /// entry; a same-stage `Pending` never renews it (Find 3).
    fn poll_recovery(&mut self, cx: &mut Context<'_>) -> Poll<()> {
        self.notify.register_queue(cx.waker());
        let access = self.service;
        let Some(mut service) = access.lock() else {
            return Poll::Pending;
        };
        let now = self.recovery_now();
        // Resident quarantine (Find 2): the owner stays but never resumes
        // stepping; the future keeps returning Pending.
        if self.recovery == Some(RecoveryState::Faulted) {
            drop(service);
            return Poll::Pending;
        }
        match self.recovery {
            Some(RecoveryState::Quiescing) => {
                if self.recovery_deadline.is_none() {
                    // Entering quiesce: arm the 1 s absolute deadline once and
                    // linearize the pre-submit cancel with the current epoch.
                    self.recovery_deadline = Some(now + QUIESCE_STAGE_DEADLINE_NS);
                    service.tx_cancel_queued_target();
                    service.tx_cancel_pending_target();
                    service.flush_progress();
                    self.arm_recovery_timer(cx);
                }
                // Bounded grace drain of DeviceOwned completions within the
                // quiesce window until the ledger is stable or the 1 s expires.
                let mut reclaimed = 0usize;
                loop {
                    match service.tx_reclaim_one_target() {
                        TxReclaimStep::Reclaimed => {
                            reclaimed += 1;
                            service.flush_progress();
                            if reclaimed >= RECLAIM_BUDGET {
                                break;
                            }
                        }
                        TxReclaimStep::Empty => break,
                        TxReclaimStep::Fault(err) => {
                            // F5: commit under the guard, drop, then publish.
                            service.flush_recovery_abort_all(&err);
                            drop(service);
                            self.publish_recovery_fault(&err, fault_cause::UNKNOWN);
                            return Poll::Pending;
                        }
                    }
                }
                let reclaimed_at_budget = reclaimed >= RECLAIM_BUDGET;
                let drained = service.device_owned_len_target() == 0;
                let expired = self
                    .recovery_deadline
                    .is_some_and(|d| self.recovery_now() >= d);
                if drained || expired {
                    if !self.lifecycle.quiescing_to_resetting().is_ok() {
                        self.telemetry
                            .lifecycle_fault
                            .fetch_add(1, Ordering::Relaxed);
                    }
                    match service.recovery_begin_target() {
                        Ok(_epoch) => {
                            self.recovery = Some(RecoveryState::Resetting);
                            self.recovery_deadline =
                                Some(self.recovery_now() + RESET_STAGE_DEADLINE_NS);
                            self.arm_recovery_progress(cx);
                        }
                        Err(err) => {
                            // The reset-begin handoff failed. The lifecycle has
                            // already committed Active -> Quiescing -> Resetting;
                            // mirror the recovery state so the fault stage reports
                            // RESET (not a QUIESCE/lifecycle split).
                            self.recovery = Some(RecoveryState::Resetting);
                            // F5: commit under the guard, drop, then publish.
                            service.flush_recovery_abort_all(&err);
                            drop(service);
                            self.publish_recovery_fault(&err, fault_cause::UNKNOWN);
                            return Poll::Pending;
                        }
                    }
                } else if reclaimed_at_budget {
                    // The bounded budget cut the grace drain short with
                    // DeviceOwned still outstanding. Self-wake so the owner
                    // converges on the next poll instead of stalling the backlog
                    // until the quiesce deadline or an external NIC event.
                    drop(service);
                    cx.waker().wake_by_ref();
                    return Poll::Pending;
                }
                drop(service);
                Poll::Pending
            }
            Some(RecoveryState::Resetting) | Some(RecoveryState::Reinitializing) => {
                if self
                    .recovery_deadline
                    .is_some_and(|d| self.recovery_now() >= d)
                {
                    let err = DevError::Io;
                    // F5: commit under the guard, drop, then publish.
                    service.flush_recovery_abort_all(&err);
                    drop(service);
                    self.publish_recovery_fault(&err, fault_cause::TIMEOUT);
                    return Poll::Pending;
                }
                let outcome = self.recovery_step(cx, &mut service);
                drop(service);
                match outcome {
                    RecoveryRound::Finished => {
                        // F5: commit any pending flush outcome under the
                        // (already dropped) guard before reopening the gate and
                        // self-waking.
                        self.service.lock().map(|s| s.flush_wake_pending());
                        // The new epoch was committed; reopen the I/O gate.
                        self.set_recovery_hold(false);
                        cx.waker().wake_by_ref();
                        Poll::Pending
                    }
                    RecoveryRound::Pending => Poll::Pending,
                    RecoveryRound::Fault(err) => {
                        // F5: the Service guard is already dropped; commit the
                        // residency + publish the recovery fault now.
                        self.publish_recovery_fault(&err, fault_cause::UNKNOWN);
                        Poll::Pending
                    }
                }
            }
            _ => {
                drop(service);
                Poll::Pending
            }
        }
    }

    /// Task 2.2 / Find 3: one bounded driver recovery step for the
    /// reset/reinitialize stage. A stage change re-arms the 2 s absolute
    /// deadline; a same-stage `Pending` keeps the already-mounted deadline, so
    /// a stalled driver eventually times out rather than being renewed forever.
    fn recovery_step(&mut self, cx: &mut Context<'_>, service: &mut Service) -> RecoveryRound {
        let now = self.recovery_now();
        let current = self.recovery.unwrap_or(RecoveryState::Resetting);
        match service.recovery_step_target() {
            Ok(progress) if progress.stage == axdriver_net::RecoveryStage::Recovered => {
                let epoch = progress.epoch;
                if let Err(err) = service.open_socket_epoch_after_recovery(self.fault_sink) {
                    return RecoveryRound::Fault(err);
                }
                service.tx_close_device_owned_target();
                // Finding 1: settle the old-epoch flush BEFORE the epoch
                // advances so its outcome is not corrupted by the reset.
                service.flush_recovery_close();
                service.tx_advance_epoch_target(epoch);
                if !self.lifecycle.recovery_committed().is_ok() {
                    self.telemetry
                        .lifecycle_fault
                        .fetch_add(1, Ordering::Relaxed);
                }
                self.recovery = None;
                self.recovery_deadline = None;
                self.recovery_progress_wake = None;
                self.cancel_recovery_timer();
                RecoveryRound::Finished
            }
            Ok(progress) => {
                let next = match progress.stage {
                    axdriver_net::RecoveryStage::Resetting => RecoveryState::Resetting,
                    axdriver_net::RecoveryStage::Reinitializing => RecoveryState::Reinitializing,
                    _ => current,
                };
                if next != current {
                    self.recovery = Some(next);
                    self.recovery_deadline = Some(now + RESET_STAGE_DEADLINE_NS);
                    if next == RecoveryState::Reinitializing {
                        let _ = self.lifecycle.resetting_to_reinitializing();
                    }
                }
                self.arm_recovery_progress(cx);
                RecoveryRound::Pending
            }
            Err(err) => {
                // F5: commit the flush outcome under the guard but DON'T wake
                // (commit_fault, no wake); publish the recovery fault only
                // after poll_recovery drops the Service guard. The error is
                // returned so the caller orders drop->commit->wake correctly.
                service.flush_recovery_abort_all(&err);
                RecoveryRound::Fault(err)
            }
        }
    }

    /// F2: freezes the structured recovery-fault summary into internal (non-ABI)
    /// telemetry. The stage comes from the running recovery state; the software
    /// ticket epoch and the driver owner summary are read together under ONE
    /// Service guard so the identity reflects a single commit-time snapshot, and
    /// the real epoch is always recorded (never `u64::MAX` for a non-Faulted
    /// owner such as an `Active` submit timeout).
    fn freeze_recovery_summary(&self, stage: u64, local_cause: u64) {
        let (epoch, summary) = self
            .service
            .lock()
            .map(|mut s| {
                let epoch = s.queue_epoch_target().current();
                let summary = s.recovery_owner_summary_target();
                (epoch, summary)
            })
            .unwrap_or((u64::MAX, axdriver_net::OwnerSummary::default()));
        // A4 / D5: commit the whole fault identity as one coherent value so a
        // reader never assembles stage, cause, epoch and owner from different
        // faults. The legacy per-field atomics stay for existing diagnostics.
        self.telemetry
            .coherent_fault
            .publish(RecoveryFaultIdentity {
                stage,
                local_cause,
                queue_epoch: epoch,
                available: summary.available,
                device_owned: summary.device_owned,
                quarantined: summary.quarantined,
            });
        self.telemetry
            .recover_fault_stage
            .store(stage, Ordering::Relaxed);
        self.telemetry
            .recover_fault_epoch
            .store(epoch, Ordering::Relaxed);
        self.telemetry
            .recover_available
            .store(summary.available, Ordering::Relaxed);
        self.telemetry
            .recover_device_owned
            .store(summary.device_owned, Ordering::Relaxed);
        self.telemetry
            .recover_quarantined
            .store(summary.quarantined, Ordering::Relaxed);
    }

    /// The R6/S4 stage identity for the current recovery state (F2).
    fn recovery_fault_stage(&self) -> crate::device::TicketFaultStage {
        match self.recovery {
            Some(RecoveryState::Quiescing) => crate::device::TicketFaultStage::Quiesce,
            Some(RecoveryState::Resetting) => crate::device::TicketFaultStage::Reset,
            Some(RecoveryState::Reinitializing) => crate::device::TicketFaultStage::Reinitialize,
            Some(RecoveryState::Faulted) | None => crate::device::TicketFaultStage::Unknown,
        }
    }

    /// Task 2.2 / Find 2: commits the quarantine. The same owner stays
    /// resident in `Faulted`, holds the I/O gate, and never resumes stepping;
    /// the error, stage and epoch are published so nothing pends forever.
    fn publish_recovery_fault(&mut self, err: &DevError, local_cause: u64) {
        // F4: close the DeviceOwned ledger as `Fault` WITHOUT releasing the
        // driver backing (the recovery holder keeps it quarantined), so a new
        // flush on the faulted owner fails stably instead of pending forever.
        // Capture the fault stage from the CURRENT recovery state BEFORE it
        // transitions to Faulted (the Faulted code maps to Unknown), then pass
        // it into the ledger closure so the Fault terminal is diagnosable.
        let stage = self.recovery_fault_stage();
        if let Some(mut service) = self.service.lock() {
            service.tx_fault_device_owned_target(stage);
            service.flush_recovery_abort_all(err);
        }
        // F2: commit `Faulted`, and only then freeze the summary so
        // `freeze_recovery_summary` observes the Faulted lifecycle and records
        // the real software ticket epoch instead of `u64::MAX`.
        if self.recovery != Some(RecoveryState::Faulted) {
            if !self.lifecycle.recover_fault().is_ok() {
                self.telemetry.record_last_error_code(
                    rx_error_stage::LIFECYCLE,
                    self.lifecycle.load().code() as u64,
                );
                self.telemetry
                    .lifecycle_fault
                    .fetch_add(1, Ordering::Relaxed);
            }
            self.recovery = Some(RecoveryState::Faulted);
        }
        self.freeze_recovery_summary(stage.code(), local_cause);
        self.recovery_deadline = None;
        self.recovery_progress_wake = None;
        self.telemetry
            .record_fault(rx_error_stage::RECEIVE_RECYCLE, err);
        let epoch = self.fault_sink.current_socket_epoch();
        self.publish_fault_epoch_terminal(epoch, crate::readiness::dev_error_code(err));
        self.notify.publish_progress();
        self.stack_notify.publish_device();
        self.cancel_recovery_timer();
        // F5: lifecycle resolved; wake the flush waiter only now, outside any
        // Service guard.
        let _ = self.service.lock().map(|s| s.flush_wake_pending());
    }

    /// The clock used for lease-deadline decisions: the attached per-test
    /// fixture clock when present, else `diag::diag_now()`.
    #[cfg(feature = "qemu-diagnostics")]
    fn diag_now(&self) -> u64 {
        #[cfg(test)]
        {
            if let Some(clock) = self.diag_test_clock {
                return clock.load();
            }
        }
        crate::diag::diag_now()
    }

    /// C4: cancels any armed lease deadline and its timer.
    #[cfg(feature = "qemu-diagnostics")]
    fn cancel_lease_deadline(&mut self) {
        self.lease_deadline = None;
        self.cancel_lease_timer();
    }

    /// C4: arms (or cancels) the QEMU diagnostic lease deadline wake.
    ///
    /// The lease expiry is the only reason the owner must wake without an
    /// external NIC event: an expired hold must auto-release so the paused
    /// stage resumes. In production this registers an axtask timer that
    /// wakes the queue waker at `deadline`; host tests drive the fake clock
    /// instead. The timer only wakes the owner: the release and failure
    /// counter stay in [`Service::diag_hold_tick`].
    ///
    /// A `deadline` of 0 (no active hold) cancels any previously armed
    /// deadline, so an explicit Release invalidates the old timer. The
    /// timer carries no lease generation: a stale wake costs at most one
    /// bounded poll, and the current Service lease decides at poll time
    /// whether to remain held and which deadline to rearm.
    #[cfg(feature = "qemu-diagnostics")]
    fn arm_lease_deadline(&mut self, cx: &mut Context<'_>, deadline: u64) {
        if deadline == 0 || self.diag_now() >= deadline {
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

    /// Drops any armed recovery-deadline timer (Task 2.2). Host tests drive
    /// the recovery clock instead.
    #[cfg(not(test))]
    fn cancel_recovery_timer(&mut self) {
        self.recovery_timer = None;
    }
    #[cfg(test)]
    fn cancel_recovery_timer(&mut self) {}

    /// Drops any armed data-stage-deadline timer (Task 2.2). Host tests drive
    /// the recovery clock and re-poll instead.
    #[cfg(not(test))]
    fn cancel_data_stage_timer(&mut self) {
        self.data_stage_timer = None;
    }
    #[cfg(test)]
    fn cancel_data_stage_timer(&mut self) {}

    /// Wakes the owner at the earliest active data-stage deadline (Task 2.2 /
    /// A1–A3). The timer is wake-only and carries no generation; a stale wake
    /// costs at most one bounded poll, and the next round re-arms from the
    /// still-blocked condition. It is armed whenever the owner sleeps on a
    /// data wait (`RegisterRecheck` / `WaitSpace(Waiting)`) so a stalled driver
    /// or missing completion eventually times out without periodic polling.
    #[cfg(not(test))]
    fn arm_data_stage_timer(&mut self, cx: &mut Context<'_>) {
        use axhal::time::TimeValue;
        use axtask::future::sleep_until;

        self.data_stage_timer = None;
        let deadline = [
            self.data_deadlines.submit,
            self.data_deadlines.completion,
            self.data_deadlines.reclaim,
        ]
        .into_iter()
        .flatten()
        .min();
        let Some(deadline) = deadline else {
            return;
        };
        if self.recovery_now() >= deadline {
            cx.waker().wake_by_ref();
            return;
        }
        let mut timer = Box::pin(sleep_until(TimeValue::from_nanos(deadline)));
        let mut timer_cx = Context::from_waker(cx.waker());
        if timer.as_mut().poll(&mut timer_cx).is_ready() {
            cx.waker().wake_by_ref();
        } else {
            self.data_stage_timer = Some(timer);
        }
    }
    #[cfg(test)]
    fn arm_data_stage_timer(&mut self, _cx: &mut Context<'_>) {}

    /// Wakes the owner at the recovery-stage absolute deadline (Task 2.2 / Find
    /// 3). The timer is wake-only; host tests advance the deterministic
    /// recovery clock and re-poll instead of running an axtask timer.
    #[cfg(not(test))]
    fn arm_recovery_timer(&mut self, cx: &mut Context<'_>) {
        use axhal::time::TimeValue;
        use axtask::future::sleep_until;

        self.recovery_timer = None;
        let Some(deadline) = self.recovery_deadline else {
            return;
        };
        if self.recovery_now() >= deadline {
            cx.waker().wake_by_ref();
            return;
        }
        let mut timer = Box::pin(sleep_until(TimeValue::from_nanos(deadline)));
        let mut timer_cx = Context::from_waker(cx.waker());
        if timer.as_mut().poll(&mut timer_cx).is_ready() {
            cx.waker().wake_by_ref();
        } else {
            self.recovery_timer = Some(timer);
        }
    }
    #[cfg(test)]
    fn arm_recovery_timer(&mut self, _cx: &mut Context<'_>) {}

    /// Cycle 005 / T4.2-R1: schedules the next bounded one-shot wake while a
    /// reset/reinitialize stage stays Pending. The wake instant is
    /// `min(now + RECOVERY_PROGRESS_CADENCE_NS, recovery_deadline)`, so a
    /// delayed driver reset gets deadline-bounded retries strictly before the
    /// absolute deadline without a busy poll. `recovery_deadline` is NOT
    /// modified here, so a same-stage Pending never renews it. Production
    /// registers an axtask timer; host tests record only the decision and
    /// re-poll on the deterministic recovery clock.
    #[cfg(not(test))]
    fn arm_recovery_progress(&mut self, cx: &mut Context<'_>) {
        use axhal::time::TimeValue;
        use axtask::future::sleep_until;

        self.recovery_timer = None;
        let Some(deadline) = self.recovery_deadline else {
            self.recovery_progress_wake = None;
            return;
        };
        let now = self.recovery_now();
        let wake = now
            .saturating_add(RECOVERY_PROGRESS_CADENCE_NS)
            .min(deadline);
        self.recovery_progress_wake = Some(wake);
        if now >= deadline {
            // Already at/past the deadline; let the next poll's deadline check
            // decide. Wake once so that poll runs promptly.
            cx.waker().wake_by_ref();
            return;
        }
        let mut timer = Box::pin(sleep_until(TimeValue::from_nanos(wake)));
        let mut timer_cx = Context::from_waker(cx.waker());
        if timer.as_mut().poll(&mut timer_cx).is_ready() {
            cx.waker().wake_by_ref();
        } else {
            self.recovery_timer = Some(timer);
        }
    }
    #[cfg(test)]
    fn arm_recovery_progress(&mut self, _cx: &mut Context<'_>) {
        let Some(deadline) = self.recovery_deadline else {
            self.recovery_progress_wake = None;
            return;
        };
        let now = self.recovery_now();
        self.recovery_progress_wake = Some(
            now.saturating_add(RECOVERY_PROGRESS_CADENCE_NS)
                .min(deadline),
        );
    }

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

    /// C4: if an armed lease deadline has elapsed, clear it and self-wake
    /// so the round runs, whose `diag_hold_tick` auto-releases the expired
    /// hold. The self-wake is observable by a counting waker in host tests.
    ///
    /// The timer is wake-only and carries no generation: a stale wake (a
    /// newer lease replaced the armed one) costs at most one bounded poll,
    /// and the current Service lease decides at poll time whether to remain
    /// held and which deadline to rearm. The newer control already published
    /// queue work, so the owner was or will be woken anyway.
    #[cfg(feature = "qemu-diagnostics")]
    fn lease_deadline_elapsed(&mut self, cx: &mut Context<'_>) {
        let Some(deadline) = self.lease_deadline else {
            return;
        };
        if self.diag_now() >= deadline {
            self.lease_deadline = None;
            self.cancel_lease_timer();
            cx.waker().wake_by_ref();
        }
    }

    /// Attempts the `Active -> Faulted` transition, publishes the concrete
    /// error to the fault sink, then publishes stack-progress only when the
    /// CAS commits.
    ///
    /// Task 3.7: the terminal wake ordering is state-first, event-after. An
    /// illegal transition (lifecycle already terminal) records the
    /// LIFECYCLE-stage diagnostic but never publishes a fake terminal state.
    fn publish_fatal(&self, err: &DevError) {
        if self.transition_fatal() {
            let epoch = self.fault_sink.current_socket_epoch();
            self.publish_fault_epoch_terminal(epoch, crate::readiness::dev_error_code(err));
            self.notify.publish_progress();
            self.stack_notify.publish_device();
        }
    }

    /// Commits the registry terminal, applies its first-wins result to hidden
    /// listener ownership through the paired Service, then wakes matching
    /// bridges only after the Service guard has been released.
    fn publish_fault_epoch_terminal(&self, epoch: u64, code: u64) {
        let mut handled_by_service = false;
        let mut committed = false;
        if let Some(service) = self.service.lock() {
            if let Some(registry) = service.socket_registry() {
                if core::ptr::eq(registry, self.fault_sink) {
                    if let Some(did_commit) =
                        service.commit_socket_epoch_terminal_for(registry, epoch, code)
                    {
                        handled_by_service = true;
                        committed = did_commit;
                    }
                }
            }
        }
        if !handled_by_service {
            committed = self
                .fault_sink
                .commit_socket_epoch_fault_code(epoch, code)
                .is_some_and(|outcome| outcome.committed);
        }
        if committed {
            self.fault_sink.wake_socket_epoch(epoch);
        }
    }

    /// Records an illegal `Active -> Faulted` transition as LIFECYCLE-stage.
    /// Returns whether the transition committed.
    fn transition_fatal(&self) -> bool {
        // A1 rework: the Active->Faulted terminal path absorbs any pending
        // explicit recovery request on the same seam that commits the
        // transition, so an accepted request cannot survive to a later
        // Active generation.
        #[cfg(feature = "qemu-diagnostics")]
        let lifecycle_transition =
            with_recovery_request_transition(&RECOVERY_RESET_REQUEST, || self.lifecycle.fatal());
        #[cfg(not(feature = "qemu-diagnostics"))]
        let lifecycle_transition = self.lifecycle.fatal();
        match lifecycle_transition {
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
            let Some(mut service) = self.service.lock() else {
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
                self.publish_fatal(&err);
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
            // Task 2.2: while a staged device recovery is in flight the owner
            // must drive it; the normal Active round would step no recovery.
            // The resident owner stays in Quiescing/Resetting/Reinitializing
            // (and quarantined Faulted) across polls until it resolves.
            RxTaskLifecycle::Quiescing
            | RxTaskLifecycle::Resetting
            | RxTaskLifecycle::Reinitializing
            | RxTaskLifecycle::Faulted
                if this.recovery.is_some() =>
            {
                this.poll_recovery(cx)
            }
            RxTaskLifecycle::Active if this.recovery.is_some() => this.poll_recovery(cx),
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
                stack_notify: &STACK_EVENT,
                stack_progress_pending: false,
                initial_link_pending: false,
                telemetry: &RX_TELEMETRY,
                fault_sink: &crate::SOCKET_SET,
                #[cfg(feature = "qemu-diagnostics")]
                lease_deadline: None,
                #[cfg(all(test, feature = "qemu-diagnostics"))]
                diag_test_clock: None,
                #[cfg(all(feature = "qemu-diagnostics", not(test)))]
                lease_timer: None,
                recovery: None,
                recovery_deadline: None,
                recovery_progress_wake: None,
                #[cfg(not(test))]
                recovery_timer: None,
                data_deadlines: DataStageDeadlines::new(),
                #[cfg(not(test))]
                data_stage_timer: None,
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
/// Activation is monotonic: `Polling -> Spawned -> Active -> Faulted`, or
/// `Spawned -> Unavailable` when preflight fails. After a recoverable
/// data-plane fault on a recovery-capable device, the same unique owner moves
/// `Active -> Quiescing -> Resetting -> Reinitializing -> Active` under a new
/// device-reset epoch, or into `Faulted` when any recovery stage exhausts its
/// deadline or the driver faults. All recovery/faulted states keep the async
/// owner resident (never roll back to a polling owner).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RxTaskLifecycle {
    Polling,
    Spawned,
    Active,
    /// Bounded reclaim/cancel of the current epoch before a device reset
    /// (Task 2.2, 1 s data/quiesce deadline).
    Quiescing,
    /// Driver reset in progress (Task 2.2, 2 s reset deadline).
    Resetting,
    /// Queue/backing rebuild after a confirmed reset (2 s reinitialize
    /// deadline).
    Reinitializing,
    Faulted,
    Unavailable,
}

impl RxTaskLifecycle {
    /// Stable V1–V3 lifecycle ABI. The first five codes are frozen and MUST
    /// NOT change: `RxSnapshot`/`IrqSnapshotV2/V3` and the kernel V2 mapping
    /// document `0 Polling, 1 Spawned, 2 Active, 3 Faulted, 4 Unavailable`.
    /// The resident-recovery states occupy the previously unoccupied codes
    /// `5 Quiescing, 6 Resetting, 7 Reinitializing` (Task 2.2). Do not reorder
    /// or repurpose these; `rx_snapshot_impl` publishes `code()` verbatim.
    const fn code(self) -> u8 {
        match self {
            Self::Polling => 0,
            Self::Spawned => 1,
            Self::Active => 2,
            Self::Faulted => 3,
            Self::Unavailable => 4,
            Self::Quiescing => 5,
            Self::Resetting => 6,
            Self::Reinitializing => 7,
        }
    }

    fn from_code(code: u8) -> Self {
        match code {
            0 => Self::Polling,
            1 => Self::Spawned,
            2 => Self::Active,
            3 => Self::Faulted,
            4 => Self::Unavailable,
            5 => Self::Quiescing,
            6 => Self::Resetting,
            7 => Self::Reinitializing,
            _ => unreachable!("lifecycle code out of range"),
        }
    }

    /// Consumption-right view: the async task owns RX once `Active` and keeps
    /// it through every recovery stage (population/drain/quiesce/reset/
    /// reinitialize) and a fatal fault, so a recovering or faulted owner is
    /// never rolled back to a polling owner. Only the un-started / un-available
    /// states are polling-owned.
    pub(crate) fn owner_view(self) -> RxOwnerView {
        match self {
            Self::Polling | Self::Spawned | Self::Unavailable => RxOwnerView::PollingOwned,
            Self::Active
            | Self::Quiescing
            | Self::Resetting
            | Self::Reinitializing
            | Self::Faulted => RxOwnerView::AsyncOwned,
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

    /// `Active -> Quiescing`: a recoverable data-plane fault begins the
    /// bounded cancel/reclaim window before the device reset (Task 2.2).
    pub(crate) fn begin_recovery(&self) -> Result<(), TransitionError> {
        self.transition(RxTaskLifecycle::Active, RxTaskLifecycle::Quiescing)
    }

    /// `Quiescing -> Resetting`: quiesce finished (drained or deadline);
    /// driver `begin_recovery` starts the reset.
    pub(crate) fn quiescing_to_resetting(&self) -> Result<(), TransitionError> {
        self.transition(RxTaskLifecycle::Quiescing, RxTaskLifecycle::Resetting)
    }

    /// `Resetting -> Reinitializing`: status == 0 confirmed; queues/backing
    /// rebuild under the reinitialize deadline.
    pub(crate) fn resetting_to_reinitializing(&self) -> Result<(), TransitionError> {
        self.transition(RxTaskLifecycle::Resetting, RxTaskLifecycle::Reinitializing)
    }

    /// `Resetting | Reinitializing -> Active`: recovery committed. Handles a
    /// driver that reports `Recovered` from either reset stage; the resident
    /// owner resumes normal service in both cases.
    pub(crate) fn recovery_committed(&self) -> Result<(), TransitionError> {
        let current = self.load();
        match current {
            RxTaskLifecycle::Resetting | RxTaskLifecycle::Reinitializing => {
                self.state
                    .swap(RxTaskLifecycle::Active.code(), Ordering::AcqRel);
                Ok(())
            }
            _ => Err(TransitionError::Illegal(current)),
        }
    }

    /// Any non-terminal recovery state `-> Faulted`: a recovery stage deadline
    /// or driver fault quarantines the resident owner. Returns `Ok` only on a
    /// committed swap, so a fault is never published from an already-terminal
    /// lifecycle.
    pub(crate) fn recover_fault(&self) -> Result<(), TransitionError> {
        let current = self.load();
        if matches!(
            current,
            RxTaskLifecycle::Active
                | RxTaskLifecycle::Quiescing
                | RxTaskLifecycle::Resetting
                | RxTaskLifecycle::Reinitializing
        ) {
            self.state
                .swap(RxTaskLifecycle::Faulted.code(), Ordering::AcqRel);
            Ok(())
        } else {
            Err(TransitionError::Illegal(current))
        }
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
    extern crate std;

    use alloc::{boxed::Box, collections::VecDeque, sync::Arc, vec, vec::Vec};
    use core::{
        pin::Pin,
        sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering},
        task::{Context, Poll, Waker},
    };

    use axdriver::prelude::{DevError, DevResult};
    use axdriver_net::{
        NetQueueControl, NetQueueDirection, NetRecoveryControl, QueueEpoch, RecoveryProgress,
        RecoveryStage, TxCookie,
    };
    use smoltcp::{storage::PacketBuffer, time::Instant, wire::IpAddress};

    #[cfg(feature = "qemu-diagnostics")]
    use super::recovery_snapshot_v4_from;
    use super::{
        ArmObservation, CoherentFaultSheet, DataStageDeadlines, QUEUE_EVENT, QueueEvent,
        RECLAIM_BUDGET, RX_BUDGET, RX_LIFECYCLE, RX_TELEMETRY, RecoveryFaultIdentity,
        RecoveryState, RxLifecycle, RxRxFuture, RxTaskLifecycle, RxTelemetry, SERIAL,
        SUBMIT_BUDGET, ServiceAccess, SpaceDecision, StartError, TransitionError, WaitDecision,
        fault_cause, recover_stage, rx_error_code, rx_error_stage, software_nudge_impl, start_with,
    };
    #[cfg(feature = "qemu-diagnostics")]
    use super::{RECOVERY_RESET_REQUEST, RecoveryRequestState, with_recovery_request_transition};

    #[cfg(feature = "qemu-diagnostics")]
    static RECOVERY_REQUEST_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    use crate::{
        device::{
            Device, FlushState, RxCopyStep, RxStep, TicketOutcome, TxOutcome, TxPreflight,
            TxReclaimStep, TxSubmitStep,
            fixed_queue::{FixedFrameQueue, TicketTracker},
        },
        flush::FlushRecheck,
        readiness,
        router::{Router, RxOwnerView},
        service::{LinkStep, Service},
        stack_runner::StackEvent,
        wrapper::SocketSetWrapper,
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

    // ---- QueueEvent: queue-owner role (stack role lives in StackEvent) ----

    #[test]
    fn queue_event_publish_wakes_queue_role_and_bumps_generation() {
        let event = super::QueueEvent::new();
        let queue_count = Arc::new(AtomicUsize::new(0));
        let before = event.generation();
        event.register_queue(&counting_waker(queue_count.clone()));
        event.publish_event();
        assert_eq!(event.generation(), before + 1);
        assert_eq!(queue_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn queue_register_does_not_lose_generation_change() {
        let event = super::QueueEvent::new();
        let queue_count = Arc::new(AtomicUsize::new(0));
        event.publish_progress();
        event.register_queue(&counting_waker(queue_count.clone()));
        event.publish_queue_work();
        assert_eq!(queue_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn queue_generation_wraps() {
        let event = super::QueueEvent::with_generation(u64::MAX);
        let queue_count = Arc::new(AtomicUsize::new(0));
        event.register_queue(&counting_waker(queue_count.clone()));
        event.publish_event();
        assert_eq!(event.generation(), 0);
        assert_eq!(queue_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn used_publish_sets_only_used_cause_and_take_clears_it() {
        let event = super::QueueEvent::new();
        event.publish_event();
        let causes = event.take_causes();
        assert!(causes.used);
        assert!(!causes.config);
        assert_eq!(event.take_causes(), super::QueueCauses::default());
    }

    #[test]
    fn config_publish_sets_only_config_cause_and_wakes_owner() {
        let event = super::QueueEvent::new();
        let count = Arc::new(AtomicUsize::new(0));
        event.register_queue(&counting_waker(count.clone()));
        let gen_before = event.generation();
        event.publish_config();
        assert_eq!(event.generation(), gen_before + 1);
        assert_eq!(count.load(Ordering::Relaxed), 1);
        let causes = event.take_causes();
        assert!(!causes.used);
        assert!(causes.config);
    }

    #[test]
    fn combined_used_and_config_retain_both_causes() {
        // A combined interrupt may wake once but must not drop either cause
        // (Task 3.1 / A1). Publishing used then config keeps both flags.
        let event = super::QueueEvent::new();
        event.publish_event();
        event.publish_config();
        let causes = event.take_causes();
        assert!(causes.used);
        assert!(causes.config);
        assert_eq!(event.take_causes(), super::QueueCauses::default());
    }

    #[test]
    fn config_cause_is_retained_for_snapshot_retry() {
        // A transient "Again" snapshot result is retained by a re-publish so
        // the next poll retries without losing the cause (Task 3.1 / A3).
        let event = super::QueueEvent::new();
        event.publish_config();
        assert!(event.take_causes().config);
        event.publish_config();
        assert!(event.take_causes().config);
        assert_eq!(event.take_causes(), super::QueueCauses::default());
    }

    #[test]
    fn space_wait_wakes_queue_role_but_progress_hint_does_not() {
        let event = super::QueueEvent::new();
        let queue_count = Arc::new(AtomicUsize::new(0));
        event.register_queue(&counting_waker(queue_count.clone()));
        event.publish_waiting();
        assert!(event.wake_if_space(true));
        assert_eq!(queue_count.load(Ordering::Relaxed), 1);
        event.publish_progress();
        assert_eq!(queue_count.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn wait_decision_retries_on_any_generation_change() {
        let event = super::QueueEvent::new();
        let queue_count = Arc::new(AtomicUsize::new(0));
        let before = event.generation();
        let decision = event.wait_decision(&counting_waker(queue_count.clone()), || {
            event.publish_progress();
            Ok(ArmObservation::Quiescent)
        });
        // The queue wait observes the generation change and retries.
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
            RxTaskLifecycle::Quiescing
            | RxTaskLifecycle::Resetting
            | RxTaskLifecycle::Reinitializing => {
                // Recovery states are only reachable by running a staged
                // recovery on a real future (see the recovery lifetime tests);
                // the standalone helper cannot synthesize them.
                panic!("recovery lifecycle states require a running recovery")
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
    fn lifecycle_frozen_v1_v3_abi_round_trips() {
        // F1 / A6: the V1–V3 wire ABI freezes `0 Polling, 1 Spawned,
        // 2 Active, 3 Faulted, 4 Unavailable` (kernel `virtio_net_irq_logic`
        // maps `rx_lifecycle` verbatim). The resident-recovery states must use
        // the unoccupied codes `5/6/7`, never shift the frozen ones, or the
        // ioctl/validator would misread Faulted/Unavailable as unknown.
        for (state, code) in [
            (RxTaskLifecycle::Polling, 0),
            (RxTaskLifecycle::Spawned, 1),
            (RxTaskLifecycle::Active, 2),
            (RxTaskLifecycle::Faulted, 3),
            (RxTaskLifecycle::Unavailable, 4),
            (RxTaskLifecycle::Quiescing, 5),
            (RxTaskLifecycle::Resetting, 6),
            (RxTaskLifecycle::Reinitializing, 7),
        ] {
            assert_eq!(state.code(), code, "lifecycle code drift");
            assert_eq!(RxTaskLifecycle::from_code(code), state, "round-trip drift");
        }
    }

    #[test]
    fn lifecycle_recovery_states_keep_async_owner_resident() {
        // F1 / A1/E5: even with the frozen codes, a committed recovery state
        // must never roll the async owner back to a polling owner. This is the
        // observable contract the ioctl relies on beyond the raw code.
        for state in [
            RxTaskLifecycle::Quiescing,
            RxTaskLifecycle::Resetting,
            RxTaskLifecycle::Reinitializing,
            RxTaskLifecycle::Faulted,
            RxTaskLifecycle::Active,
        ] {
            assert_eq!(state.owner_view(), RxOwnerView::AsyncOwned);
        }
        for state in [
            RxTaskLifecycle::Polling,
            RxTaskLifecycle::Spawned,
            RxTaskLifecycle::Unavailable,
        ] {
            assert_eq!(state.owner_view(), RxOwnerView::PollingOwned);
        }
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
            (RxTaskLifecycle::Quiescing, RxOwnerView::AsyncOwned),
            (RxTaskLifecycle::Resetting, RxOwnerView::AsyncOwned),
            (RxTaskLifecycle::Reinitializing, RxOwnerView::AsyncOwned),
            (RxTaskLifecycle::Faulted, RxOwnerView::AsyncOwned),
            (RxTaskLifecycle::Unavailable, RxOwnerView::PollingOwned),
        ] {
            assert_eq!(state.owner_view(), expected);
            if !matches!(
                state,
                RxTaskLifecycle::Quiescing
                    | RxTaskLifecycle::Resetting
                    | RxTaskLifecycle::Reinitializing
            ) {
                assert_eq!(drive_to(state).owner_view(), expected);
            }
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
    fn config_event_before_register_is_caught_by_arm_recheck() {
        // Task 3.1 / R6 / A2 (Plan Review Finding 2): a CONFIG cause published
        // before the owner registers must be observed by the arm/recheck — the
        // sole owner re-takes the cause instead of sleeping through the change.
        let notify = QueueEvent::new();
        let count = Arc::new(AtomicUsize::new(0));
        notify.publish_config();

        let decision = notify.wait_decision(&counting_waker(count.clone()), || {
            Ok(ArmObservation::Pending)
        });
        assert!(matches!(decision, WaitDecision::Retry));
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn config_event_during_register_window_retries() {
        // Task 3.1 / R6 / A2 (Plan Review Finding 2): a CONFIG publication
        // inside the arm/recheck window must force a retry so the owner
        // re-takes the cause instead of sleeping through the link change.
        let notify = QueueEvent::new();
        let count = Arc::new(AtomicUsize::new(0));

        let decision = notify.wait_decision(&counting_waker(count.clone()), || {
            notify.publish_config();
            Ok(ArmObservation::Quiescent)
        });
        assert!(matches!(decision, WaitDecision::Retry));
    }

    #[test]
    fn config_event_after_arm_wakes_sleep_decision() {
        // Task 3.1 / R6 / A2 (Plan Review Finding 2): a CONFIG publication
        // after a quiescent sleep decision must wake the owner so the config
        // cause is serviced on the next poll rather than an indefinite sleep.
        let notify = QueueEvent::new();
        let count = Arc::new(AtomicUsize::new(0));

        let decision = notify.wait_decision(&counting_waker(count.clone()), || {
            Ok(ArmObservation::Quiescent)
        });
        assert!(matches!(decision, WaitDecision::Sleep));

        notify.publish_config();
        assert_eq!(count.load(Ordering::Relaxed), 1);
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

    /// Shared, test-observable driver state for the resident-recovery fixture.
    #[derive(Default)]
    struct RecoveryDriverStats {
        begin_calls: AtomicUsize,
        step_calls: AtomicUsize,
        /// Epoch written by `Device::tx_advance_epoch` at the commit.
        committed_epoch: AtomicU64,
        /// Driver epoch offset from `QueueEpoch::MIN` (survives re-fetch).
        lasted_epoch: AtomicU64,
        /// Driver-visible stage (RecoveryStage discriminant).
        stage: AtomicU8,
        /// One-shot injectable step fault for the quarantine path.
        step_error: AtomicBool,
        /// Whether the next RX-copy raises a data-plane fault.
        fault_pending: AtomicBool,
        /// Whether the next RX-copy raises an ownership-drift (`BadState`)
        /// data-plane fault (F4: must quarantine without a reset).
        drift_pending: AtomicBool,
        /// Driver-visible owner resources reported by `owner_summary()` (F2);
        /// the recovery fault must freeze these real values.
        owner_available: AtomicU64,
        owner_device_owned: AtomicU64,
        owner_quarantined: AtomicU64,
        /// Recovery I/O gate mirror observed by the test (Find 2).
        recovery_hold: AtomicBool,
        /// When set, `poll_recovery_step` reports the same stage without
        /// advancing, so a test can prove a stage deadline is not renewed
        /// (Find 3).
        stall_stage: AtomicBool,
        /// Count of pre-submit owner-cancellation passthroughs invoked on the
        /// mocked device. The drift-quarantine witness asserts these are
        /// called exactly once under the Service guard, so same-epoch Queued
        /// tickets and ARP-pending packets never survive a resident fault.
        cancel_queued_calls: AtomicUsize,
        cancel_pending_calls: AtomicUsize,
        /// Count of DeviceOwned fault-closure invocations (F4). The witness
        /// asserts the drift path terminates every outstanding DeviceOwned
        /// ticket as `Fault(OwnershipDrift)` exactly once.
        fault_device_owned_calls: AtomicUsize,
        /// Outstanding DeviceOwned tickets reported by `tx_device_owned_len`
        /// and drained by `tx_reclaim_one` (Task 2.3 quiesce witness; 0 means
        /// the device is already drained, matching the historic fixture).
        device_owned: core::sync::atomic::AtomicU64,
        /// One-shot: the next `tx_reclaim_one` faults the quiesce reclaim,
        /// so the owner must quarantine without a reset (Task 2.3 quiesce
        /// drift / reclaim-fault witness).
        reclaim_error: core::sync::atomic::AtomicBool,
        /// When set, `tx_reclaim_one` reports `Empty` while `device_owned`
        /// stays positive, modelling a device with no visible completion
        /// (Task 2.3 1 s-expiry-remaining-owner witness).
        reclaim_stall: core::sync::atomic::AtomicBool,
        /// One-shot: the next `begin_recovery` fails, exercising the
        /// reset-begin fault identity (Task 2.3 begin-error stage witness).
        begin_error: core::sync::atomic::AtomicBool,
        /// Epoch observed by a post-recovery TX submit, proving the data path
        /// runs at the new epoch (Task 2.3 A5 witness).
        submit_epoch: core::sync::atomic::AtomicU64,
        /// The ticket identity recorded by a post-recovery submit, reused to
        /// build the epoch-bound reclaim cookie (Task 2.3 A5 witness).
        submitted_ticket: core::sync::atomic::AtomicU64,
        /// One-shot completion on the real TX ledger: when set, the next
        /// post-recovery `tx_reclaim_one` releases the submitted DeviceOwned
        /// ticket via an epoch-bound cookie (Task 2.3 A5 witness).
        completion_armed: core::sync::atomic::AtomicBool,
        /// Link snapshot reported by `read_link_status` (Task 3.1; true = up).
        link: AtomicBool,
        /// Number of `read_link_status` calls observed (at-most-once witness).
        link_reads: AtomicUsize,
        /// One-shot: the next `read_link_status` returns `Again` (A3 witness).
        link_again: AtomicBool,
        /// One-shot: the next `read_link_status` returns `Unsupported`,
        /// modelling a driver without link control (P2 non-blocking witness).
        link_unsupported: AtomicBool,
        /// Link I/O gate mirror observed by the test (Task 3.1 / D6).
        link_hold: AtomicBool,
    }

    /// Scripted driver recovery machine (mirrors `axdriver_net::RecoveryModel`).
    ///
    /// The generated epoch is a raw offset from [`QueueEpoch::MIN`] held in the
    /// shared stats (the real `QueueEpoch` value is produced only via the
    /// public [`QueueEpoch::advance`] chain; the tuple field is private, and a
    /// fresh `ScriptedRecovery` is re-fetched on each `recovery_control()` call).
    struct ScriptedRecovery {
        stats: Arc<RecoveryDriverStats>,
    }

    impl ScriptedRecovery {
        fn stage(&self) -> u8 {
            self.stats.stage.load(Ordering::Relaxed)
        }
        fn current_epoch(&self) -> QueueEpoch {
            let mut e = QueueEpoch::MIN;
            for _ in 0..self.stats.lasted_epoch.load(Ordering::Relaxed) {
                e = e.advance().expect("test epoch headroom");
            }
            e
        }
        fn progress_view(&self) -> RecoveryProgress {
            let stage = match self.stage() {
                1 => RecoveryStage::Resetting,
                2 => RecoveryStage::Reinitializing,
                3 => RecoveryStage::Recovered,
                _ => RecoveryStage::Idle,
            };
            RecoveryProgress {
                stage,
                epoch: self.current_epoch(),
            }
        }
    }

    impl NetRecoveryControl for ScriptedRecovery {
        fn progress(&self) -> RecoveryProgress {
            self.progress_view()
        }

        fn begin_recovery(&mut self) -> DevResult<RecoveryProgress> {
            if self.stats.begin_error.swap(false, Ordering::Relaxed) {
                return Err(DevError::Io);
            }
            if self.stage() != 0 {
                return Err(DevError::BadState);
            }
            self.stats.begin_calls.fetch_add(1, Ordering::Relaxed);
            self.stats.stage.store(1, Ordering::Relaxed);
            Ok(self.progress_view())
        }

        fn poll_recovery_step(&mut self) -> DevResult<RecoveryProgress> {
            self.stats.step_calls.fetch_add(1, Ordering::Relaxed);
            if self.stats.step_error.swap(false, Ordering::Relaxed) {
                return Err(DevError::Io);
            }
            // Find 3: a stalled driver reports the same reset stage without
            // advancing, so the absolute deadline must eventually expire.
            if self.stats.stall_stage.load(Ordering::Relaxed) && matches!(self.stage(), 1 | 2) {
                return Ok(self.progress_view());
            }
            match self.stage() {
                1 => {
                    self.stats.stage.store(2, Ordering::Relaxed);
                    Ok(self.progress_view())
                }
                2 => {
                    self.stats.lasted_epoch.fetch_add(1, Ordering::Relaxed);
                    self.stats.stage.store(3, Ordering::Relaxed);
                    Ok(self.progress_view())
                }
                _ => Err(DevError::BadState),
            }
        }

        fn owner_summary(&self) -> axdriver_net::OwnerSummary {
            axdriver_net::OwnerSummary {
                available: self.stats.owner_available.load(Ordering::Relaxed),
                device_owned: self.stats.owner_device_owned.load(Ordering::Relaxed),
                quarantined: self.stats.owner_quarantined.load(Ordering::Relaxed),
            }
        }

        fn read_link_status(&mut self) -> DevResult<bool> {
            self.stats.link_reads.fetch_add(1, Ordering::Relaxed);
            if self.stats.link_again.swap(false, Ordering::Relaxed) {
                return Err(DevError::Again);
            }
            if self.stats.link_unsupported.swap(false, Ordering::Relaxed) {
                return Err(DevError::Unsupported);
            }
            Ok(self.stats.link.load(Ordering::Relaxed))
        }
    }

    /// A fake NIC that faults once on RX-copy, then goes quiet, but exposes a
    /// scripted bounded recovery control so the resident owner must recover
    /// instead of exiting.
    struct RecoveringDevice {
        stats: Arc<RecoveryDriverStats>,
        recovery: ScriptedRecovery,
        queue_control: ScriptedControl,
    }

    impl Device for RecoveringDevice {
        fn name(&self) -> &str {
            "recovering"
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
            TxOutcome::Accepted {
                rx_became_ready: false,
            }
        }

        fn rx_copy_one(&mut self) -> RxCopyStep {
            // F4: an ownership-drift RX-copy fault must quarantine the owner
            // without a reset mask.
            if matches!(
                self.stats.drift_pending.swap(false, Ordering::Relaxed),
                true
            ) {
                RxCopyStep::Fault(DevError::BadState)
            } else if matches!(
                self.stats.fault_pending.swap(false, Ordering::Relaxed),
                true
            ) {
                RxCopyStep::Fault(DevError::Io)
            } else {
                RxCopyStep::Empty
            }
        }

        fn tx_submit_one(&mut self) -> TxSubmitStep {
            TxSubmitStep::Empty
        }

        fn tx_reclaim_one(&mut self) -> TxReclaimStep {
            // Only the recovery/quiesce drain owns this fixture's DeviceOwned:
            // the ordinary Active round must not consume reclaim_error or drain
            // the ledger before the owner enters recovery.
            if !self.stats.recovery_hold.load(Ordering::Relaxed) {
                return TxReclaimStep::Empty;
            }
            if self.stats.reclaim_error.swap(false, Ordering::Relaxed) {
                return TxReclaimStep::Fault(DevError::Io);
            }
            if self.stats.device_owned.load(Ordering::Relaxed) == 0 {
                return TxReclaimStep::Empty;
            }
            if self.stats.reclaim_stall.load(Ordering::Relaxed) {
                return TxReclaimStep::Empty;
            }
            self.stats.device_owned.fetch_sub(1, Ordering::Relaxed);
            TxReclaimStep::Reclaimed
        }

        fn rx_slot_has_space(&self) -> bool {
            true
        }

        fn tx_slot_pending(&self) -> bool {
            false
        }

        fn tx_close_device_owned(&mut self) -> usize {
            let closed = self.stats.device_owned.swap(0, Ordering::Relaxed) as usize;
            closed
        }

        fn tx_cancel_queued(&mut self) -> usize {
            self.stats
                .cancel_queued_calls
                .fetch_add(1, Ordering::Relaxed);
            0
        }

        fn tx_cancel_pending(&mut self) -> usize {
            self.stats
                .cancel_pending_calls
                .fetch_add(1, Ordering::Relaxed);
            0
        }

        fn tx_fault_device_owned(&mut self, _stage: crate::device::TicketFaultStage) -> usize {
            self.stats
                .fault_device_owned_calls
                .fetch_add(1, Ordering::Relaxed);
            0
        }

        fn tx_advance_epoch(&mut self, next: QueueEpoch) {
            self.stats
                .committed_epoch
                .store(next.current(), Ordering::Relaxed);
        }

        fn queue_epoch(&self) -> QueueEpoch {
            let mut e = QueueEpoch::MIN;
            for _ in 0..self.stats.committed_epoch.load(Ordering::Relaxed) {
                e = e.advance().expect("test epoch headroom");
            }
            e
        }

        fn tx_set_recovery_hold(&mut self, held: bool) {
            self.stats.recovery_hold.store(held, Ordering::Relaxed);
        }

        fn tx_set_link_hold(&mut self, held: bool) {
            self.stats.link_hold.store(held, Ordering::Relaxed);
        }

        fn tx_device_owned_len(&self) -> u64 {
            self.stats.device_owned.load(Ordering::Relaxed)
        }

        fn recovery_control(&mut self) -> Option<&mut dyn NetRecoveryControl> {
            Some(&mut self.recovery)
        }

        fn queue_control(&mut self) -> Option<&mut dyn NetQueueControl> {
            Some(&mut self.queue_control)
        }

        fn register_waker(&self, _waker: &Waker) {}
    }

    fn leaked_service_recovering() -> (&'static spin::Mutex<Service>, Arc<RecoveryDriverStats>) {
        let stats = Arc::new(RecoveryDriverStats::default());
        // A functional NIC is link-up by default; only link-specific tests
        // choose a down/up state explicitly.
        stats.link.store(true, Ordering::Relaxed);
        stats.fault_pending.store(true, Ordering::Relaxed);
        let queue_stats = Arc::new(ScriptedControlStats::default());
        let device = RecoveringDevice {
            stats: stats.clone(),
            recovery: ScriptedRecovery {
                stats: stats.clone(),
            },
            queue_control: ScriptedControl {
                stats: queue_stats.clone(),
            },
        };
        let mut router = Router::new();
        let idx = router.add_device(Box::new(device));
        let service = Service::new(router, Some(idx));
        (Box::leak(Box::new(spin::Mutex::new(service))), stats)
    }

    /// A clean RecoveringDevice Service for the Task 3.1 link-policy seam
    /// (`fault_pending` is NOT forced, so the ordinary Active round runs).
    fn leaked_service_link() -> (&'static spin::Mutex<Service>, Arc<RecoveryDriverStats>) {
        let stats = Arc::new(RecoveryDriverStats::default());
        // A functional NIC is link-up by default; only link-specific tests
        // choose a down/up state explicitly.
        stats.link.store(true, Ordering::Relaxed);
        let queue_stats = Arc::new(ScriptedControlStats::default());
        let device = RecoveringDevice {
            stats: stats.clone(),
            recovery: ScriptedRecovery {
                stats: stats.clone(),
            },
            queue_control: ScriptedControl {
                stats: queue_stats.clone(),
            },
        };
        let mut router = Router::new();
        let idx = router.add_device(Box::new(device));
        let service = Service::new(router, Some(idx));
        (Box::leak(Box::new(spin::Mutex::new(service))), stats)
    }

    fn leaked_service_paired_link() -> (
        &'static spin::Mutex<Service>,
        &'static SocketSetWrapper<'static>,
        Arc<RecoveryDriverStats>,
    ) {
        let (service, stats) = leaked_service_link();
        let registry = Box::leak(Box::new(SocketSetWrapper::new()));
        service.lock().set_socket_registry(registry);
        (service, registry, stats)
    }

    // ── Task 3.1 link-policy seam (R6 / D6 / A3 / A5) ───────────────────

    #[test]
    fn link_policy_down_gates_cancels_presubmit_and_advances_seam() {
        let (mutex, stats) = leaked_service_link();
        let mut s = mutex.lock();
        let gen0 = s.link_generation();
        let epoch0 = s.socket_epoch();
        let qepoch0 = s.queue_epoch_target();
        // A4: a link-down must NOT close DeviceOwned tickets — they keep being
        // reclaimed until a device reset — so seed some and assert they survive.
        stats.device_owned.store(3, Ordering::Relaxed);
        stats.link.store(false, Ordering::Relaxed);
        let step = s.link_policy_step_target();
        assert_eq!(step, crate::service::LinkStep::Down);
        assert!(stats.link_hold.load(Ordering::Relaxed));
        assert_eq!(stats.cancel_queued_calls.load(Ordering::Relaxed), 1);
        assert_eq!(stats.cancel_pending_calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            stats.device_owned.load(Ordering::Relaxed),
            3,
            "link-down reclaims DeviceOwned; it never closes them"
        );
        assert_eq!(s.link_generation(), gen0 + 1);
        assert_eq!(s.socket_epoch(), epoch0 + 1);
        // A link-down is not a device reset: QueueEpoch is untouched.
        assert_eq!(s.queue_epoch_target(), qepoch0);
    }

    #[test]
    fn link_policy_stable_value_does_not_advance_seam() {
        let (mutex, stats) = leaked_service_link();
        stats.link.store(true, Ordering::Relaxed);
        let mut s = mutex.lock();
        assert_eq!(s.link_policy_step_target(), crate::service::LinkStep::Up);
        let gen1 = s.link_generation();
        let epoch1 = s.socket_epoch();
        assert_eq!(
            s.link_policy_step_target(),
            crate::service::LinkStep::NoEvent
        );
        assert_eq!(s.link_generation(), gen1);
        assert_eq!(s.socket_epoch(), epoch1);
    }

    #[test]
    fn link_policy_stable_down_cancels_each_owner_once() {
        // Task 3.1 / A4 / D6 (Plan Review Finding 2): a link-down transition
        // must cancel the pre-submit Queued and ARP-pending owners exactly once,
        // and a subsequent stable-down (same value) must not cancel again or
        // advance the seam — the gate stays held for the whole down interval.
        let (mutex, stats) = leaked_service_link();
        let mut s = mutex.lock();
        stats.link.store(false, Ordering::Relaxed);
        assert_eq!(s.link_policy_step_target(), crate::service::LinkStep::Down);
        assert_eq!(stats.cancel_queued_calls.load(Ordering::Relaxed), 1);
        assert_eq!(stats.cancel_pending_calls.load(Ordering::Relaxed), 1);
        assert!(stats.link_hold.load(Ordering::Relaxed));
        let gen1 = s.link_generation();
        let epoch1 = s.socket_epoch();
        assert_eq!(
            s.link_policy_step_target(),
            crate::service::LinkStep::NoEvent
        );
        assert_eq!(
            stats.cancel_queued_calls.load(Ordering::Relaxed),
            1,
            "stable down must not re-cancel Queued owners"
        );
        assert_eq!(
            stats.cancel_pending_calls.load(Ordering::Relaxed),
            1,
            "stable down must not re-cancel ARP-pending owners"
        );
        assert_eq!(s.link_generation(), gen1);
        assert_eq!(s.socket_epoch(), epoch1);
        assert!(stats.link_hold.load(Ordering::Relaxed));
    }

    #[test]
    fn link_policy_again_maps_through_and_counts_one_read() {
        let (mutex, stats) = leaked_service_link();
        stats.link_again.store(true, Ordering::Relaxed);
        let mut s = mutex.lock();
        assert_eq!(s.link_policy_step_target(), crate::service::LinkStep::Again);
        assert_eq!(stats.link_reads.load(Ordering::Relaxed), 1);
        assert!(!stats.link_hold.load(Ordering::Relaxed));
    }

    #[test]
    fn link_policy_up_after_down_opens_new_epoch_without_reset() {
        let (mutex, stats) = leaked_service_link();
        let mut s = mutex.lock();
        stats.link.store(false, Ordering::Relaxed);
        assert_eq!(s.link_policy_step_target(), crate::service::LinkStep::Down);
        let epoch_down = s.socket_epoch();
        stats.link.store(true, Ordering::Relaxed);
        assert_eq!(s.link_policy_step_target(), crate::service::LinkStep::Up);
        assert!(!stats.link_hold.load(Ordering::Relaxed));
        assert_eq!(s.socket_epoch(), epoch_down + 1);
    }

    #[test]
    fn paired_service_and_registry_keep_epoch_identity_across_flaps() {
        use crate::{readiness::NetworkTerminal, tcp::new_tcp_socket};

        let (mutex, registry, stats) = leaked_service_paired_link();
        let qepoch = mutex.lock().queue_epoch_target();
        let (_, old_bridge) = registry.add_public(new_tcp_socket());
        let epoch0 = registry.current_socket_epoch();

        stats.link.store(false, Ordering::Relaxed);
        assert_eq!(mutex.lock().link_policy_step_target(), LinkStep::Down);
        assert_eq!(mutex.lock().socket_epoch(), registry.current_socket_epoch());

        stats.link.store(true, Ordering::Relaxed);
        assert_eq!(mutex.lock().link_policy_step_target(), LinkStep::Up);
        assert_eq!(mutex.lock().socket_epoch(), registry.current_socket_epoch());
        assert_eq!(registry.current_socket_epoch(), epoch0 + 1);

        let (_, fresh_bridge) = registry.add_public(new_tcp_socket());
        stats.link.store(false, Ordering::Relaxed);
        assert_eq!(mutex.lock().link_policy_step_target(), LinkStep::Down);
        assert_eq!(mutex.lock().socket_epoch(), registry.current_socket_epoch());
        assert_eq!(
            fresh_bridge.network_terminal_code(),
            NetworkTerminal::LinkDown.code()
        );

        stats.link.store(true, Ordering::Relaxed);
        assert_eq!(mutex.lock().link_policy_step_target(), LinkStep::Up);
        let (_, newest_bridge) = registry.add_public(new_tcp_socket());
        assert_eq!(mutex.lock().socket_epoch(), registry.current_socket_epoch());
        assert_eq!(
            newest_bridge.network_terminal_code(),
            readiness::TERMINAL_NONE
        );
        assert_eq!(
            old_bridge.network_terminal_code(),
            NetworkTerminal::LinkDown.code()
        );
        assert_eq!(
            fresh_bridge.network_terminal_code(),
            NetworkTerminal::LinkDown.code()
        );
        assert_eq!(mutex.lock().queue_epoch_target(), qepoch);
    }

    #[test]
    fn link_policy_socket_epoch_overflow_fail_stops_consistently() {
        // Task 3.1 / A5 / D1 (Plan Review Finding 1): when the SocketEpoch
        // checked identity is exhausted, the transition must fail-stop as a
        // WHOLE: persist the fault, keep the data plane closed, advance and
        // commit nothing, and never let a later link-up reopen the gate.
        let (mutex, stats) = leaked_service_link();
        let mut s = mutex.lock();
        s.set_socket_epoch_for_test(u64::MAX);
        let gen_before = s.link_generation();
        stats.link.store(false, Ordering::Relaxed);
        // A down transition on an exhausted SocketEpoch must NOT return a
        // successful Down: it must report the fail-stop directly.
        assert_eq!(
            s.link_policy_step_target(),
            crate::service::LinkStep::Fault,
            "exhausted seam must fail-stop, not commit a Down transition"
        );
        assert!(s.link_seam_fault());
        assert_eq!(s.socket_epoch(), u64::MAX, "socket epoch must not advance");
        assert_eq!(
            s.link_generation(),
            gen_before,
            "the other checked identity must not advance on fail-stop"
        );
        assert!(
            stats.link_hold.load(Ordering::Relaxed),
            "data plane must stay closed on fail-stop"
        );
        // After the fail-stop, a later link-up must NOT reopen the gate,
        // commit an epoch-shifting transition, or advance any identity.
        stats.link.store(true, Ordering::Relaxed);
        assert_eq!(
            s.link_policy_step_target(),
            crate::service::LinkStep::Fault,
            "a later link-up must never reopen a fail-stopped seam"
        );
        assert!(
            stats.link_hold.load(Ordering::Relaxed),
            "the link gate must stay held after a fail-stop"
        );
        assert_eq!(s.socket_epoch(), u64::MAX);
        assert_eq!(s.link_generation(), gen_before);
    }

    #[test]
    fn link_policy_link_generation_overflow_fail_stops_and_stays_closed() {
        // Task 3.1 / A3 / D1 (Plan Review Finding 1/2): LinkGeneration
        // overflow must fail-stop identically to SocketEpoch — persist the
        // fault, keep the data plane closed, advance/commit nothing, and never
        // let a later transition reopen the gate. QueueEpoch stays unchanged.
        let (mutex, stats) = leaked_service_link();
        let mut s = mutex.lock();
        s.set_link_generation_for_test(u64::MAX);
        let epoch_before = s.socket_epoch();
        let qepoch = s.queue_epoch_target();
        stats.link.store(true, Ordering::Relaxed);
        assert_eq!(
            s.link_policy_step_target(),
            crate::service::LinkStep::Fault,
            "an exhausted LinkGeneration must fail-stop, not commit an Up"
        );
        assert!(s.link_seam_fault());
        assert_eq!(
            s.link_generation(),
            u64::MAX,
            "LinkGeneration must not advance"
        );
        assert_eq!(
            s.socket_epoch(),
            epoch_before,
            "the other checked identity must not advance on LinkGeneration fail-stop"
        );
        assert!(
            stats.link_hold.load(Ordering::Relaxed),
            "a link-up attempt on an exhausted LinkGeneration must not open the gate"
        );
        assert_eq!(
            s.queue_epoch_target(),
            qepoch,
            "QueueEpoch must stay unchanged"
        );
        // Post-fault permanence: further transitions stay Fail and gate closed.
        stats.link.store(false, Ordering::Relaxed);
        assert_eq!(
            s.link_policy_step_target(),
            crate::service::LinkStep::Fault,
            "post-overflow transitions must remain fail-stopped"
        );
        assert!(stats.link_hold.load(Ordering::Relaxed));
        assert_eq!(s.link_generation(), u64::MAX);
        assert_eq!(s.socket_epoch(), epoch_before);
    }

    fn preset_queued_owner(mutex: &'static spin::Mutex<Service>) -> (IpAddress, [u8; 16], Instant) {
        // A real `Device::send` enqueues a Queued ticket into the real
        // FixedFrameQueue/TicketTracker ledger (the owner awaiting the next
        // Active submit). Used to prove a seam fail-stop closes Queued
        // ownership and a same-round submit cannot move it to DeviceOwned.
        let frame = [0xABu8; 16];
        let hop = IpAddress::Ipv4(smoltcp::wire::Ipv4Address::new(10, 0, 0, 9));
        let ts = Instant::from_millis(0);
        let accepted = {
            let mut s = mutex.lock();
            s.router_for_test().devices[0].send(hop, &frame, ts)
        };
        assert!(matches!(accepted, TxOutcome::Accepted { .. }));
        let _ = accepted;
        assert!(mutex.lock().router_for_test().devices[0].tx_slot_pending());
        (hop, frame, ts)
    }

    fn drive_link_fault_owner_round(
        fut: &mut RxRxFuture,
        notify: &'static super::QueueEvent,
    ) -> bool {
        // One owner round with a pending CONFIG cause: register/recheck, take
        // causes, run the link-policy step (fail-stop on overflow) and then the
        // normal service_round (reclaim/rx/submit). Returns whether the round
        // reached the Active data path at all.
        notify.publish_config();
        let count = Arc::new(AtomicUsize::new(0));
        let res = poll_once(fut, count.clone());
        res.is_pending()
    }

    #[test]
    fn link_policy_socket_epoch_overflow_closes_queued_and_blocks_submit() {
        // Plan Review (rework) Finding: a seam fail-stop must close pre-existing
        // Queued/ARP-pending ownership AND stop a same-round submit from moving
        // them to DeviceOwned; already-DeviceOwned tickets still reclaim.
        let (mutex, stats) = leaked_service_ledger_link();
        preset_queued_owner(mutex);
        let qepoch0 = mutex.lock().queue_epoch_target();
        let submit_calls_before = stats.submitted_ticket.load(Ordering::Relaxed);

        {
            let mut s = mutex.lock();
            s.set_socket_epoch_for_test(u64::MAX);
        }
        let notify: &'static super::QueueEvent = Box::leak(Box::new(super::QueueEvent::new()));
        let (_lifecycle, mut fut) = leaked_future(mutex, notify);
        drive_link_fault_owner_round(&mut fut, notify);

        // The fail-stop canceled the pre-existing Queued owner exactly once.
        assert_eq!(
            stats.cancel_queued_calls.load(Ordering::Relaxed),
            1,
            "fail-stop must cancel the Queued owner exactly once"
        );
        assert_eq!(
            stats.cancel_pending_calls.load(Ordering::Relaxed),
            1,
            "fail-stop must cancel ARP-pending exactly once"
        );
        // No submit reached DeviceOwned: the Queued slot was closed, not moved.
        assert_eq!(
            stats.submitted_ticket.load(Ordering::Relaxed),
            submit_calls_before,
            "no driver submit must fire after the SocketEpoch fail-stop"
        );
        assert_eq!(
            mutex.lock().device_owned_len_target(),
            0,
            "the Queued owner must not become DeviceOwned after fail-stop"
        );
        assert!(
            stats.link_hold.load(Ordering::Relaxed),
            "the fail-stop must hold the link gate"
        );
        assert_eq!(
            mutex.lock().queue_epoch_target(),
            qepoch0,
            "QueueEpoch must be unchanged by the fail-stop"
        );
        assert!(
            mutex.lock().router_for_test().devices[0].tx_slot_pending() == false,
            "the Queued slot must be drained by the fail-stop cancel"
        );
        // A later link-up must not reopen the data plane.
        stats.link.store(true, Ordering::Relaxed);
        assert_eq!(
            mutex.lock().link_policy_step_target(),
            crate::service::LinkStep::Fault
        );
        assert!(stats.link_hold.load(Ordering::Relaxed));
    }

    #[test]
    fn link_policy_link_generation_overflow_closes_queued_and_blocks_submit() {
        // Plan Review (rework) Finding: same as the SocketEpoch overflow but
        // through the LinkGeneration identity.
        let (mutex, stats) = leaked_service_ledger_link();
        preset_queued_owner(mutex);
        let qepoch0 = mutex.lock().queue_epoch_target();
        let submit_calls_before = stats.submitted_ticket.load(Ordering::Relaxed);

        {
            let mut s = mutex.lock();
            s.set_link_generation_for_test(u64::MAX);
        }
        let notify: &'static super::QueueEvent = Box::leak(Box::new(super::QueueEvent::new()));
        let (_lifecycle, mut fut) = leaked_future(mutex, notify);
        drive_link_fault_owner_round(&mut fut, notify);

        assert_eq!(
            stats.cancel_queued_calls.load(Ordering::Relaxed),
            1,
            "LinkGeneration fail-stop must cancel the Queued owner exactly once"
        );
        assert_eq!(stats.cancel_pending_calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            stats.submitted_ticket.load(Ordering::Relaxed),
            submit_calls_before,
            "no driver submit must fire after the LinkGeneration fail-stop"
        );
        assert_eq!(mutex.lock().device_owned_len_target(), 0);
        assert!(stats.link_hold.load(Ordering::Relaxed));
        assert_eq!(mutex.lock().queue_epoch_target(), qepoch0);
        assert!(mutex.lock().router_for_test().devices[0].tx_slot_pending() == false);
        stats.link.store(true, Ordering::Relaxed);
        assert_eq!(
            mutex.lock().link_policy_step_target(),
            crate::service::LinkStep::Fault
        );
        assert!(stats.link_hold.load(Ordering::Relaxed));
    }

    #[test]
    fn owner_config_cause_reads_link_once_and_gates_on_down() {
        let (mutex, stats) = leaked_service_link();
        stats.link.store(false, Ordering::Relaxed);
        let notify: &'static super::QueueEvent = Box::leak(Box::new(super::QueueEvent::new()));
        let (lifecycle, mut fut) = leaked_future(mutex, notify);
        let count = Arc::new(AtomicUsize::new(0));
        notify.publish_config();
        let res = poll_once(&mut fut, count.clone());
        assert!(res.is_pending());
        assert_eq!(
            stats.link_reads.load(Ordering::Relaxed),
            1,
            "one config cause"
        );
        assert!(stats.link_hold.load(Ordering::Relaxed));
        assert!(matches!(lifecycle.load(), super::RxTaskLifecycle::Active));
        assert_eq!(notify.take_causes(), super::QueueCauses::default());
    }

    #[test]
    fn owner_config_cause_again_retains_cause_for_next_poll() {
        let (mutex, stats) = leaked_service_link();
        let notify: &'static super::QueueEvent = Box::leak(Box::new(super::QueueEvent::new()));
        let (_lifecycle, mut fut) = leaked_future(mutex, notify);
        let count = Arc::new(AtomicUsize::new(0));
        stats.link_again.store(true, Ordering::Relaxed);
        notify.publish_config();
        let res = poll_once(&mut fut, count.clone());
        assert!(res.is_pending());
        assert_eq!(stats.link_reads.load(Ordering::Relaxed), 1);
        // A transient snap-race retained the config cause for the next poll.
        assert!(notify.take_causes().config);
    }

    #[test]
    fn owner_activation_commits_initial_link_without_config_cause() {
        // P2 / R6: the resident owner must commit a consistent initial link
        // snapshot in task context on activation, even when no CONFIG IRQ cause
        // has arrived. Without a CONFIG cause the owner never called
        // `link_policy_step_target`, so the link stayed unknown (<<< RED).
        let (mutex, stats) = leaked_service_link();
        stats.link.store(true, Ordering::Relaxed);
        let gen0 = mutex.lock().link_generation();
        let notify: &'static super::QueueEvent = Box::leak(Box::new(super::QueueEvent::new()));
        let (_lifecycle, mut fut) = leaked_future(mutex, notify);
        let count = Arc::new(AtomicUsize::new(0));
        let res = poll_once(&mut fut, count.clone());
        assert!(res.is_pending());
        assert_eq!(
            stats.link_reads.load(Ordering::Relaxed),
            1,
            "activation must read the initial link once without a CONFIG cause"
        );
        assert_eq!(mutex.lock().link_generation(), gen0 + 1);
        assert_eq!(mutex.lock().link_state_code(), 1);
        assert!(!stats.link_hold.load(Ordering::Relaxed));
        // The initial read is self-initiated (P2 / R6): it must NOT fabricate a
        // hardware CONFIG IRQ cause or touch the shared event cause flags.
        assert_eq!(notify.take_causes(), super::QueueCauses::default());
    }

    #[test]
    fn owner_activation_initial_link_down_commits_and_gates() {
        // P2 / R6 / D6: if the device is actually down at activation, the owner
        // must commit the down state (closing the SocketEpoch seam and holding
        // the I/O gate) rather than forcing "up" for boot convenience.
        let (mutex, stats) = leaked_service_link();
        stats.link.store(false, Ordering::Relaxed);
        let gen0 = mutex.lock().link_generation();
        let qepoch0 = mutex.lock().queue_epoch_target();
        let notify: &'static super::QueueEvent = Box::leak(Box::new(super::QueueEvent::new()));
        let (_lifecycle, mut fut) = leaked_future(mutex, notify);
        let count = Arc::new(AtomicUsize::new(0));
        let res = poll_once(&mut fut, count.clone());
        assert!(res.is_pending());
        assert_eq!(stats.link_reads.load(Ordering::Relaxed), 1);
        assert_eq!(mutex.lock().link_state_code(), 0);
        assert_eq!(mutex.lock().link_generation(), gen0 + 1);
        assert!(stats.link_hold.load(Ordering::Relaxed));
        // An initial down is a link event, not a device reset.
        assert_eq!(mutex.lock().queue_epoch_target(), qepoch0);
    }

    #[test]
    fn owner_activation_initial_link_again_retries_until_commit() {
        // P2 / A3: a transient config-generation race on the very first read is
        // retained and retried once per later bounded poll (no spinning, no
        // lost event), then commits once the snapshot is consistent.
        let (mutex, stats) = leaked_service_link();
        stats.link.store(true, Ordering::Relaxed);
        let gen0 = mutex.lock().link_generation();
        let notify: &'static super::QueueEvent = Box::leak(Box::new(super::QueueEvent::new()));
        let (_lifecycle, mut fut) = leaked_future(mutex, notify);
        let count = Arc::new(AtomicUsize::new(0));
        stats.link_again.store(true, Ordering::Relaxed);
        let res = poll_once(&mut fut, count.clone());
        assert!(res.is_pending());
        assert_eq!(stats.link_reads.load(Ordering::Relaxed), 1);
        assert_eq!(
            mutex.lock().link_generation(),
            gen0,
            "Again must not commit"
        );
        // A transient first-read race retains the retry work by re-publishing
        // the CONFIG cause (self-wake), exactly like a later CONFIG `Again`.
        assert!(notify.take_causes().config);
        // Second bounded poll: the one-shot `Again` is consumed, the snapshot
        // is now consistent, and the initial link commits exactly once.
        let res = poll_once(&mut fut, count.clone());
        assert!(res.is_pending());
        assert_eq!(stats.link_reads.load(Ordering::Relaxed), 2);
        assert_eq!(mutex.lock().link_generation(), gen0 + 1);
        assert_eq!(mutex.lock().link_state_code(), 1);
        assert!(!stats.link_hold.load(Ordering::Relaxed));
    }

    #[test]
    fn owner_activation_initial_link_does_not_repeat_after_commit() {
        // P2 / no-repeat: once the initial link has committed, the owner must
        // NOT re-read it on later polls without a new CONFIG cause (the flag is
        // cleared), so LinkGeneration is not advanced spuriously.
        let (mutex, stats) = leaked_service_link();
        stats.link.store(true, Ordering::Relaxed);
        let notify: &'static super::QueueEvent = Box::leak(Box::new(super::QueueEvent::new()));
        let (_lifecycle, mut fut) = leaked_future(mutex, notify);
        let count = Arc::new(AtomicUsize::new(0));
        let res = poll_once(&mut fut, count.clone());
        assert!(res.is_pending());
        assert_eq!(stats.link_reads.load(Ordering::Relaxed), 1);
        let gen1 = mutex.lock().link_generation();
        let res = poll_once(&mut fut, count.clone());
        assert!(res.is_pending());
        assert_eq!(
            stats.link_reads.load(Ordering::Relaxed),
            1,
            "initial link must not be re-read without a CONFIG cause"
        );
        assert_eq!(mutex.lock().link_generation(), gen1);
    }

    #[test]
    fn owner_activation_initial_link_unsupported_does_not_block_owner() {
        // P2 / Unsupported: a driver without link control reports the initial
        // link as `Unsupported`; the owner stays Active and keeps servicing,
        // with the link snapshot left unknown (no fabricated up) and no gate.
        let (mutex, stats) = leaked_service_link();
        stats.link_unsupported.store(true, Ordering::Relaxed);
        let gen0 = mutex.lock().link_generation();
        let notify: &'static super::QueueEvent = Box::leak(Box::new(super::QueueEvent::new()));
        let (_lifecycle, mut fut) = leaked_future(mutex, notify);
        let count = Arc::new(AtomicUsize::new(0));
        let res = poll_once(&mut fut, count.clone());
        assert!(res.is_pending());
        assert_eq!(stats.link_reads.load(Ordering::Relaxed), 1);
        assert_eq!(mutex.lock().link_generation(), gen0);
        assert_eq!(mutex.lock().link_state_code(), u64::MAX);
        assert!(!stats.link_hold.load(Ordering::Relaxed));
    }

    /// A faithful TX-ledger recovery device (Task 2.3 A5 witness): a real
    /// [`TicketTracker`] (the project's epoch-bound owner ledger) feeding a
    /// real [`FixedFrameQueue`] TX slot, plus a scripted recovery. `send` truly
    /// enqueues a frame with an epoch-bound ticket; `tx_submit_one` moves
    /// Queued -> DeviceOwned (recording the epoch); `tx_reclaim_one` releases
    /// the DeviceOwned ticket via an epoch-bound `TxCookie` when a completion
    /// is armed. This proves the recovered data path really moves a frame at
    /// the new epoch and the owner ledger conserves, instead of relying on
    /// independent counters.
    struct LedgerRecoveryDevice {
        stats: Arc<RecoveryDriverStats>,
        recovery: ScriptedRecovery,
        queue_control: ScriptedControl,
        tx_slots: FixedFrameQueue<64>,
        tx_tickets: TicketTracker,
    }

    impl Device for LedgerRecoveryDevice {
        fn name(&self) -> &str {
            "ledgerrecover"
        }
        fn recv(&mut self, _b: &mut PacketBuffer<()>, _t: Instant) -> RxStep {
            RxStep::Empty
        }
        fn preflight_send(&mut self, _n: IpAddress, _p: &[u8], _t: Instant) -> TxPreflight {
            TxPreflight::Ready
        }
        fn send(&mut self, _n: IpAddress, packet: &[u8], _t: Instant) -> TxOutcome {
            // Compound gate (D6): the fixture models the product — a resetting
            // or link-down plane rejects new enqueue.
            if self.stats.recovery_hold.load(Ordering::Relaxed)
                || self.stats.link_hold.load(Ordering::Relaxed)
            {
                return TxOutcome::Full;
            }
            if self.tx_slots.preflight(packet.len()).is_err() || !self.tx_tickets.can_alloc() {
                return TxOutcome::Full;
            }
            let ticket = self.tx_tickets.alloc().expect("test ticket headroom");
            if self
                .tx_slots
                .fill((), Some(ticket), |r| {
                    r[..packet.len()].copy_from_slice(packet);
                    Ok(packet.len())
                })
                .is_err()
            {
                return TxOutcome::Full;
            }
            TxOutcome::Accepted {
                rx_became_ready: false,
            }
        }
        fn rx_copy_one(&mut self) -> RxCopyStep {
            if self.stats.drift_pending.swap(false, Ordering::Relaxed) {
                RxCopyStep::Fault(DevError::BadState)
            } else if self.stats.fault_pending.swap(false, Ordering::Relaxed) {
                RxCopyStep::Fault(DevError::Io)
            } else {
                RxCopyStep::Empty
            }
        }
        fn tx_submit_one(&mut self) -> TxSubmitStep {
            // Mid-recovery or link-down the I/O gate is held: no new submit can
            // reach DeviceOwned (D6). This models the product's submit gate.
            if self.stats.recovery_hold.load(Ordering::Relaxed)
                || self.stats.link_hold.load(Ordering::Relaxed)
            {
                return TxSubmitStep::Full;
            }
            let Some((_, Some(ticket), _)) = self.tx_slots.peek_full() else {
                return TxSubmitStep::Empty;
            };
            if !self.tx_tickets.mark_device_owned(ticket) {
                return TxSubmitStep::Fault(DevError::BadState);
            }
            let _ = self.tx_slots.pop();
            self.stats
                .submit_epoch
                .store(self.tx_tickets.current_epoch().current(), Ordering::Relaxed);
            self.stats.submitted_ticket.store(ticket, Ordering::Relaxed);
            TxSubmitStep::Submitted
        }
        fn tx_reclaim_one(&mut self) -> TxReclaimStep {
            // No completion until the test arms one.
            if !self.stats.completion_armed.swap(false, Ordering::Relaxed) {
                return TxReclaimStep::Empty;
            }
            // The reclaim releases the submitted DeviceOwned ticket through
            // the epoch-bound cookie. A stale/unknown/duplicate cookie is owner
            // drift, never a success.
            let ticket = self.stats.submitted_ticket.load(Ordering::Relaxed);
            let cookie = TxCookie::with_epoch(self.tx_tickets.current_epoch(), ticket);
            if self.tx_tickets.release_device_owned(cookie) {
                TxReclaimStep::Reclaimed
            } else {
                TxReclaimStep::Fault(DevError::BadState)
            }
        }
        fn rx_slot_has_space(&self) -> bool {
            true
        }
        fn tx_slot_pending(&self) -> bool {
            !self.tx_slots.is_empty()
        }
        fn tx_last_accepted(&self) -> Option<u64> {
            self.tx_tickets.last_accepted()
        }
        fn tx_flush_state(&self, target: Option<u64>) -> FlushState {
            self.tx_tickets.flush_state(target)
        }
        fn queue_epoch(&self) -> QueueEpoch {
            self.tx_tickets.current_epoch()
        }
        fn tx_cancel_queued(&mut self) -> usize {
            let cancelled = self.tx_tickets.cancel_queued();
            for _ in 0..cancelled {
                let _ = self.tx_slots.pop();
            }
            self.stats
                .cancel_queued_calls
                .fetch_add(1, Ordering::Relaxed);
            cancelled
        }
        fn tx_cancel_pending(&mut self) -> usize {
            self.stats
                .cancel_pending_calls
                .fetch_add(1, Ordering::Relaxed);
            0
        }
        fn tx_close_device_owned(&mut self) -> usize {
            self.tx_tickets.close_device_owned()
        }
        fn tx_fault_device_owned(&mut self, stage: crate::device::TicketFaultStage) -> usize {
            self.tx_tickets.fault_outstanding(stage)
        }
        fn tx_advance_epoch(&mut self, next: QueueEpoch) {
            self.tx_tickets.advance_epoch(next);
            self.stats
                .committed_epoch
                .store(next.current(), Ordering::Relaxed);
        }
        fn tx_set_recovery_hold(&mut self, held: bool) {
            self.stats.recovery_hold.store(held, Ordering::Relaxed);
        }
        fn tx_set_link_hold(&mut self, held: bool) {
            self.stats.link_hold.store(held, Ordering::Relaxed);
        }
        fn tx_device_owned_len(&self) -> u64 {
            self.tx_tickets.device_owned_len() as u64
        }
        fn recovery_control(&mut self) -> Option<&mut dyn NetRecoveryControl> {
            Some(&mut self.recovery)
        }
        fn queue_control(&mut self) -> Option<&mut dyn NetQueueControl> {
            Some(&mut self.queue_control)
        }
        fn register_waker(&self, _w: &Waker) {}
    }

    fn leaked_service_ledger_recovering()
    -> (&'static spin::Mutex<Service>, Arc<RecoveryDriverStats>) {
        let stats = Arc::new(RecoveryDriverStats::default());
        // A functional NIC is link-up by default; only link-specific tests
        // choose a down/up state explicitly.
        stats.link.store(true, Ordering::Relaxed);
        stats.fault_pending.store(true, Ordering::Relaxed);
        let queue_stats = Arc::new(ScriptedControlStats::default());
        let device = LedgerRecoveryDevice {
            stats: stats.clone(),
            recovery: ScriptedRecovery {
                stats: stats.clone(),
            },
            queue_control: ScriptedControl {
                stats: queue_stats.clone(),
            },
            tx_slots: FixedFrameQueue::new(),
            tx_tickets: TicketTracker::new(),
        };
        let mut router = Router::new();
        let idx = router.add_device(Box::new(device));
        let service = Service::new(router, Some(idx));
        (Box::leak(Box::new(spin::Mutex::new(service))), stats)
    }

    /// A real-ledger Service in an Active (non-faulting) round with link
    /// snapshot + link-hold support, so fail-stop tests can preset a real
    /// Queued owner and prove the same-round submit is blocked.
    fn leaked_service_ledger_link() -> (&'static spin::Mutex<Service>, Arc<RecoveryDriverStats>) {
        let stats = Arc::new(RecoveryDriverStats::default());
        // A functional NIC is link-up by default; only link-specific tests
        // choose a down/up state explicitly.
        stats.link.store(true, Ordering::Relaxed);
        let queue_stats = Arc::new(ScriptedControlStats::default());
        let device = LedgerRecoveryDevice {
            stats: stats.clone(),
            recovery: ScriptedRecovery {
                stats: stats.clone(),
            },
            queue_control: ScriptedControl {
                stats: queue_stats.clone(),
            },
            tx_slots: FixedFrameQueue::new(),
            tx_tickets: TicketTracker::new(),
        };
        let mut router = Router::new();
        let idx = router.add_device(Box::new(device));
        let service = Service::new(router, Some(idx));
        (Box::leak(Box::new(spin::Mutex::new(service))), stats)
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

    /// Capacity-aware fake driver (repair 6.2-R4). Shared atomic counters let
    /// the Service's device and the test observe the same state. `tx_submit_one`
    /// returns `Submitted` while `inflight < capacity`, `Full` (`Again`) when the
    /// ledger is full; `tx_reclaim_one` frees one in-flight buffer. The slots
    /// counter models the fixed TX slot backlog and is capped at the driver
    /// capacity, matching the production relation (both `MS05_QS = 64`), so a full
    /// 32-submit round budget drains exactly to capacity without a pending slot to
    /// force a real `Again`.
    #[cfg(feature = "qemu-diagnostics")]
    #[derive(Default)]
    struct LedgerCounters {
        capacity: AtomicUsize,
        inflight: AtomicUsize,
        slots: AtomicUsize,
        submit_calls: AtomicUsize,
        reclaim_calls: AtomicUsize,
        again_calls: AtomicUsize,
    }

    #[cfg(feature = "qemu-diagnostics")]
    struct LedgerDevice {
        counters: Arc<LedgerCounters>,
        control: Option<ScriptedControl>,
    }

    #[cfg(feature = "qemu-diagnostics")]
    impl Device for LedgerDevice {
        fn name(&self) -> &str {
            "ledger"
        }

        fn queue_control(&mut self) -> Option<&mut dyn NetQueueControl> {
            self.control.as_mut().map(|c| c as &mut dyn NetQueueControl)
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
            TxOutcome::Accepted {
                rx_became_ready: false,
            }
        }

        fn rx_copy_one(&mut self) -> RxCopyStep {
            RxCopyStep::Empty
        }

        fn tx_submit_one(&mut self) -> TxSubmitStep {
            let c = &self.counters;
            c.submit_calls.fetch_add(1, Ordering::Relaxed);
            if c.slots.load(Ordering::Relaxed) == 0 {
                return TxSubmitStep::Empty;
            }
            if c.inflight.load(Ordering::Relaxed) >= c.capacity.load(Ordering::Relaxed) {
                c.again_calls.fetch_add(1, Ordering::Relaxed);
                return TxSubmitStep::Full;
            }
            c.slots.fetch_sub(1, Ordering::Relaxed);
            c.inflight.fetch_add(1, Ordering::Relaxed);
            TxSubmitStep::Submitted
        }

        fn tx_reclaim_one(&mut self) -> TxReclaimStep {
            let c = &self.counters;
            c.reclaim_calls.fetch_add(1, Ordering::Relaxed);
            if c.inflight.load(Ordering::Relaxed) > 0 {
                c.inflight.fetch_sub(1, Ordering::Relaxed);
                TxReclaimStep::Reclaimed
            } else {
                TxReclaimStep::Empty
            }
        }

        fn tx_slot_pending(&self) -> bool {
            self.counters.slots.load(Ordering::Relaxed) > 0
        }

        fn tx_resource_ledger(&mut self) -> Option<crate::device::TxResourceLedger> {
            let c = &self.counters;
            let inflight = c.inflight.load(Ordering::Relaxed) as u64;
            let cap = c.capacity.load(Ordering::Relaxed) as u64;
            Some(axdriver_net::TxResourceLedger {
                buffer_available: cap - inflight,
                buffer_inflight: inflight,
                descriptor_available: cap - inflight,
                descriptor_inflight: inflight,
                completions_seen: 0,
            })
        }

        fn slot_ledger(&self) -> crate::device::SlotLedger {
            let mut ledger = crate::device::SlotLedger::default();
            ledger.tx_occupancy = self.counters.slots.load(Ordering::Relaxed) as u64;
            ledger
        }

        fn register_waker(&self, _waker: &Waker) {}
    }

    /// Builds a leaked Service wrapping a [`LedgerDevice`], returning the
    /// Service mutex, the shared counters handle and the control stats.
    #[cfg(feature = "qemu-diagnostics")]
    fn leaked_service_ledger(
        capacity: usize,
        with_control: bool,
    ) -> (
        &'static spin::Mutex<Service>,
        Arc<LedgerCounters>,
        Arc<ScriptedControlStats>,
    ) {
        let counters = Arc::new(LedgerCounters::default());
        counters.capacity.store(capacity, Ordering::Relaxed);
        let stats = Arc::new(ScriptedControlStats::default());
        let control = with_control.then(|| ScriptedControl {
            stats: stats.clone(),
        });
        let device = LedgerDevice {
            counters: counters.clone(),
            control,
        };
        let mut router = Router::new();
        let idx = router.add_device(Box::new(device));
        let service = Service::new(router, Some(idx));
        let mutex: &'static spin::Mutex<Service> = Box::leak(Box::new(spin::Mutex::new(service)));
        (mutex, counters, stats)
    }

    /// Builds an injected Future: local leaked lifecycle/notify/telemetry,
    /// spin service mutex, lifecycle already driven to `Spawned`. Faults
    /// publish to a fresh LOCAL sink so fault-driving tests never mutate the
    /// shared global registry (tests needing the sink use
    /// [`Self::leaked_future_with_sink`]).
    fn leaked_future(
        service_mutex: &'static spin::Mutex<Service>,
        notify: &'static QueueEvent,
    ) -> (&'static RxLifecycle, RxRxFuture) {
        let lifecycle: &'static RxLifecycle = Box::leak(Box::new(RxLifecycle::new()));
        lifecycle.start().unwrap();
        let telemetry: &'static RxTelemetry = Box::leak(Box::new(RxTelemetry::new()));
        let fut = RxRxFuture {
            service: ServiceAccess::Injected(service_mutex),
            lifecycle,
            notify,
            stack_notify: Box::leak(Box::new(StackEvent::new())),
            stack_progress_pending: false,
            initial_link_pending: false,
            telemetry,
            fault_sink: Box::leak(Box::new(SocketSetWrapper::new())),
            #[cfg(feature = "qemu-diagnostics")]
            lease_deadline: None,
            #[cfg(all(test, feature = "qemu-diagnostics"))]
            diag_test_clock: None,
            #[cfg(all(feature = "qemu-diagnostics", not(test)))]
            lease_timer: None,
            recovery: None,
            recovery_deadline: None,
            recovery_progress_wake: None,
            #[cfg(test)]
            recovery_test_clock: None,
            data_deadlines: DataStageDeadlines::new(),
        };
        (lifecycle, fut)
    }

    fn leaked_future_with_stack(
        service_mutex: &'static spin::Mutex<Service>,
        notify: &'static QueueEvent,
        stack_notify: &'static StackEvent,
    ) -> (&'static RxLifecycle, RxRxFuture) {
        let lifecycle: &'static RxLifecycle = Box::leak(Box::new(RxLifecycle::new()));
        lifecycle.start().unwrap();
        let telemetry: &'static RxTelemetry = Box::leak(Box::new(RxTelemetry::new()));
        let fut = RxRxFuture {
            service: ServiceAccess::Injected(service_mutex),
            lifecycle,
            notify,
            stack_notify,
            stack_progress_pending: false,
            initial_link_pending: false,
            telemetry,
            fault_sink: Box::leak(Box::new(SocketSetWrapper::new())),
            #[cfg(feature = "qemu-diagnostics")]
            lease_deadline: None,
            #[cfg(all(test, feature = "qemu-diagnostics"))]
            diag_test_clock: None,
            #[cfg(all(feature = "qemu-diagnostics", not(test)))]
            lease_timer: None,
            recovery: None,
            recovery_deadline: None,
            recovery_progress_wake: None,
            #[cfg(test)]
            recovery_test_clock: None,
            data_deadlines: DataStageDeadlines::new(),
        };
        (lifecycle, fut)
    }

    fn poll_once(fut: &mut RxRxFuture, count: Arc<AtomicUsize>) -> Poll<()> {
        let waker = counting_waker(count.clone());
        let mut cx = Context::from_waker(&waker);
        Pin::new(fut).poll(&mut cx)
    }

    fn poll_observe(fut: &mut RxRxFuture, count: Arc<AtomicUsize>) -> (Poll<()>, usize) {
        let waker = counting_waker(count.clone());
        let mut cx = Context::from_waker(&waker);
        let res = Pin::new(fut).poll(&mut cx);
        (res, count.load(Ordering::Relaxed))
    }

    #[cfg(feature = "qemu-diagnostics")]
    #[test]
    fn reclaim_hold_drains_to_real_driver_full_without_observing_again() {
        // Repair 6.2-R4 diagnostic witness. The TX-slot capacity equals the
        // driver buffer/descriptor capacity (both `MS05_QS = 64`) and the
        // 32-submit round budget divides 64 exactly: under HOLD_RECLAIM the
        // queue service drains exactly 64 in-flight at a budget boundary, so no
        // pending slot remains to force the 65th submit that would raise a real
        // `Again`. This proves the probe's `tx_again > held->tx_again` FULL
        // predicate is structurally unreachable and the driver-Full witness must
        // come from the conserved ledger instead.
        //
        // Task 5.2 (Iteration 006): a per-test fixture clock (not the
        // process-global `TEST_NOW`) drives the lease, so this test no longer
        // needs the suite-level `SERIAL`.
        let clock = crate::diag::DiagTestClock::new();
        clock.store(1_000_000_000_000);
        let (mutex, counters, _stats) = leaked_service_ledger(64, true);
        mutex.lock().attach_test_clock(clock);
        counters.slots.store(64, Ordering::Relaxed);
        {
            let mut s = mutex.lock();
            s.diag_control(crate::diag::OP_HOLD_TX_RECLAIM, 1500, clock.load())
                .unwrap();
            assert_eq!(s.diag_hold_mode(), crate::diag::HOLD_RECLAIM);
        }
        let (_, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.diag_test_clock = Some(clock);

        // Drive service rounds: reclaim is held, submit advances 32/round. The
        // round self-wakes while TX slots remain, then sleeps on the lease once
        // the ledger is full and no slot is pending.
        let mut rounds = 0u32;
        loop {
            let outcome;
            {
                let mut s = mutex.lock();
                outcome = fut.service_round(&mut s);
            }
            rounds += 1;
            match outcome {
                super::RoundOutcome::SelfWakeYield => continue,
                super::RoundOutcome::SleepUntil(_) => break,
                _ => panic!("unexpected round outcome"),
            }
        }

        // The driver reached exactly-full (64 in-flight, zero slots) ...
        assert_eq!(counters.inflight.load(Ordering::Relaxed), 64);
        assert_eq!(counters.slots.load(Ordering::Relaxed), 0);
        // ... but no real `Again` ever fired:
        assert_eq!(counters.again_calls.load(Ordering::Relaxed), 0);
        assert_eq!(rounds, 2, "expected two 32-submit rounds to reach capacity");
        assert_eq!(counters.reclaim_calls.load(Ordering::Relaxed), 0);

        // Release then reclaim: the ledger closes exactly (conservation).
        {
            let mut s = mutex.lock();
            s.diag_control(crate::diag::OP_RELEASE, 0, 1_000_000_000_000)
                .unwrap();
        }
        for _ in 0..64 {
            let mut s = mutex.lock();
            let step = s.tx_reclaim_one_target();
            assert!(matches!(step, TxReclaimStep::Reclaimed));
        }
        assert_eq!(counters.inflight.load(Ordering::Relaxed), 0);
        assert_eq!(
            mutex
                .lock()
                .v3_tx_resource_ledger()
                .unwrap()
                .buffer_available,
            64
        );
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
            stack_notify: Box::leak(Box::new(StackEvent::new())),
            stack_progress_pending: false,
            initial_link_pending: false,
            telemetry,
            fault_sink: &crate::SOCKET_SET,
            #[cfg(feature = "qemu-diagnostics")]
            lease_deadline: None,
            #[cfg(all(test, feature = "qemu-diagnostics"))]
            diag_test_clock: None,
            #[cfg(all(feature = "qemu-diagnostics", not(test)))]
            lease_timer: None,
            recovery: None,
            recovery_deadline: None,
            recovery_progress_wake: None,
            #[cfg(test)]
            recovery_test_clock: None,
            data_deadlines: DataStageDeadlines::new(),
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
    fn rx_copy_publishes_the_independent_stack_event() {
        let (mutex, _, control) = leaked_service(vec![RxStep::Consumed, RxStep::Empty], true);
        control.completion_visible.store(true, Ordering::Relaxed);
        let lifecycle: &'static RxLifecycle = Box::leak(Box::new(RxLifecycle::new()));
        lifecycle.start().unwrap();
        let queue_notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));
        let stack_notify: &'static StackEvent = Box::leak(Box::new(StackEvent::new()));
        let stack_wakes = Arc::new(AtomicUsize::new(0));
        stack_notify.register(&counting_waker(stack_wakes.clone()));
        let mut fut = RxRxFuture {
            service: ServiceAccess::Injected(mutex),
            lifecycle,
            notify: queue_notify,
            stack_notify,
            stack_progress_pending: false,
            initial_link_pending: false,
            telemetry: Box::leak(Box::new(RxTelemetry::new())),
            fault_sink: &crate::SOCKET_SET,
            #[cfg(feature = "qemu-diagnostics")]
            lease_deadline: None,
            #[cfg(all(test, feature = "qemu-diagnostics"))]
            diag_test_clock: None,
            #[cfg(all(feature = "qemu-diagnostics", not(test)))]
            lease_timer: None,
            recovery: None,
            recovery_deadline: None,
            recovery_progress_wake: None,
            #[cfg(test)]
            recovery_test_clock: None,
            data_deadlines: DataStageDeadlines::new(),
        };

        let owner_wakes = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, owner_wakes), Poll::Pending));
        assert_eq!(stack_notify.generation(), 1);
        assert_eq!(stack_wakes.load(Ordering::Relaxed), 1);
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
        // queue-owner role. The stack wake now lands on `StackEvent`.
        let (mutex, _, control) = leaked_service(vec![RxStep::Empty], true);
        control.arm_error.store(true, Ordering::Relaxed);
        let queue_notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));
        let stack_notify: &'static StackEvent = Box::leak(Box::new(StackEvent::new()));
        let stack_count = Arc::new(AtomicUsize::new(0));
        stack_notify.register(&counting_waker(stack_count.clone()));
        let (lifecycle, mut fut) = leaked_future_with_stack(mutex, queue_notify, stack_notify);
        let count = Arc::new(AtomicUsize::new(0));
        assert!(matches!(
            poll_once(&mut fut, count.clone()),
            Poll::Ready(())
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        assert_eq!(stack_count.load(Ordering::Relaxed), 1);
        assert_eq!(stack_notify.generation(), 1);
    }

    #[test]
    fn fatal_service_round_wake_observes_faulted_lifecycle() {
        // Task 3.7: the RX-copy stage fault must commit `Active -> Faulted`
        // before releasing the generation and waking the stack role. The
        // observer samples the lifecycle inside the wake callback, so the old
        // publish-before-transition order observes `Active` and fails here.
        let (mutex, ..) = leaked_service(vec![RxStep::Fault(DevError::Io)], true);
        let queue_notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));
        let stack_notify: &'static StackEvent = Box::leak(Box::new(StackEvent::new()));
        let (lifecycle, mut fut) = leaked_future_with_stack(mutex, queue_notify, stack_notify);
        lifecycle.preflight(true).unwrap();
        let observed = Arc::new(AtomicU8::new(u8::MAX));
        let woken = Arc::new(AtomicUsize::new(0));
        stack_notify.register(&lifecycle_observing_waker(
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
        let queue_notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));
        let stack_notify: &'static StackEvent = Box::leak(Box::new(StackEvent::new()));
        let (lifecycle, mut fut) = leaked_future_with_stack(mutex, queue_notify, stack_notify);
        let observed = Arc::new(AtomicU8::new(u8::MAX));
        let woken = Arc::new(AtomicUsize::new(0));
        stack_notify.register(&lifecycle_observing_waker(
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

    fn leaked_future_with_sink(
        service_mutex: &'static spin::Mutex<Service>,
    ) -> (
        &'static RxLifecycle,
        &'static SocketSetWrapper<'static>,
        RxRxFuture,
    ) {
        let lifecycle: &'static RxLifecycle = Box::leak(Box::new(RxLifecycle::new()));
        lifecycle.start().unwrap();
        let telemetry: &'static RxTelemetry = Box::leak(Box::new(RxTelemetry::new()));
        let sink: &'static SocketSetWrapper<'static> = Box::leak(Box::new(SocketSetWrapper::new()));
        let fut = RxRxFuture {
            service: ServiceAccess::Injected(service_mutex),
            lifecycle,
            notify: Box::leak(Box::new(QueueEvent::new())),
            stack_notify: Box::leak(Box::new(StackEvent::new())),
            stack_progress_pending: false,
            initial_link_pending: false,
            telemetry,
            fault_sink: sink,
            #[cfg(feature = "qemu-diagnostics")]
            lease_deadline: None,
            #[cfg(all(test, feature = "qemu-diagnostics"))]
            diag_test_clock: None,
            #[cfg(all(feature = "qemu-diagnostics", not(test)))]
            lease_timer: None,
            recovery: None,
            recovery_deadline: None,
            recovery_progress_wake: None,
            #[cfg(test)]
            recovery_test_clock: None,
            data_deadlines: DataStageDeadlines::new(),
        };
        (lifecycle, sink, fut)
    }

    #[test]
    fn receive_fault_publishes_concrete_code_to_fault_sink() {
        let (mutex, ..) = leaked_service(vec![RxStep::Fault(DevError::Io)], true);
        let (lifecycle, sink, mut fut) = leaked_future_with_sink(mutex);
        lifecycle.preflight(true).unwrap();
        let count = Arc::new(AtomicUsize::new(0));

        assert!(matches!(poll_once(&mut fut, count), Poll::Ready(())));

        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        assert_eq!(
            sink.global_terminal_code(),
            readiness::dev_error_code(&DevError::Io)
        );
        // The published code maps to a retryable-backpressure-free category.
        assert!(matches!(
            readiness::terminal_ax_error(sink.global_terminal_code()),
            axerrno::AxError::Io
        ));
    }

    #[test]
    fn arm_fault_publishes_concrete_code_to_fault_sink() {
        let steps: Vec<RxStep> = (0..RX_BUDGET).map(|_| RxStep::Consumed).collect();
        let (mutex, _, control) = leaked_service(steps, true);
        control
            .missing_after_first_control_call
            .store(true, Ordering::Relaxed);
        let (lifecycle, sink, mut fut) = leaked_future_with_sink(mutex);
        lifecycle.preflight(true).unwrap();
        let count = Arc::new(AtomicUsize::new(0));

        assert!(matches!(poll_once(&mut fut, count), Poll::Ready(())));

        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        assert_eq!(
            sink.global_terminal_code(),
            readiness::dev_error_code(&DevError::Unsupported)
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
            stack_notify: Box::leak(Box::new(StackEvent::new())),
            stack_progress_pending: false,
            initial_link_pending: false,
            telemetry,
            fault_sink: &crate::SOCKET_SET,
            #[cfg(feature = "qemu-diagnostics")]
            lease_deadline: None,
            #[cfg(all(test, feature = "qemu-diagnostics"))]
            diag_test_clock: None,
            #[cfg(all(feature = "qemu-diagnostics", not(test)))]
            lease_timer: None,
            recovery: None,
            recovery_deadline: None,
            recovery_progress_wake: None,
            #[cfg(test)]
            recovery_test_clock: None,
            data_deadlines: DataStageDeadlines::new(),
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
        // Task 3.7: an illegal Active->Faulted transition records the
        // LIFECYCLE diagnostic but never publishes a fake terminal stack wake.
        let (mutex, ..) = leaked_service(vec![], true);
        let queue_notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));
        let stack_notify: &'static StackEvent = Box::leak(Box::new(StackEvent::new()));
        let stack_count = Arc::new(AtomicUsize::new(0));
        stack_notify.register(&counting_waker(stack_count.clone()));
        let (lifecycle, fut) = leaked_future_with_stack(mutex, queue_notify, stack_notify);
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Spawned);

        fut.publish_fatal(&DevError::Io);
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Spawned);
        assert_eq!(stack_count.load(Ordering::Relaxed), 0);
        assert_eq!(stack_notify.generation(), 0);
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
        let poll_active_end = source.find("fn classify_fault").unwrap();
        let poll_active = &source[poll_active_start..poll_active_end];
        let round_fault = &poll_active[poll_active.find("RoundOutcome::Fault").unwrap()
            ..poll_active.find("RoundOutcome::Recover").unwrap()];
        assert!(round_fault.contains("self.publish_fatal(&err)"));
        assert!(
            !round_fault.contains("publish_progress()"),
            "poll_active fault branch must not publish directly"
        );

        let arm_start = source.find("fn poll_register_recheck").unwrap();
        let arm_end = source.find("impl Future for RxRxFuture").unwrap();
        let arm_region = &source[arm_start..arm_end];
        let arm_fault = &arm_region[arm_region.find("WaitDecision::Fault").unwrap()..];
        assert!(arm_fault.contains("self.publish_fatal(&err)"));
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
        // The QEMU diagnostic lease is Service-owned; a hold committed under
        // the injected Service guard only ever gates this owner, so parallel
        // siblings servicing a round stay unaffected.
        //
        // Task 5.2 (Iteration 006): a per-test fixture clock drives the
        // lease; no process-global `TEST_NOW` and no suite `SERIAL`.
        let clock = crate::diag::DiagTestClock::new();
        let t0 = clock.load();
        let (mutex, _copy_calls, _stats) = leaked_service_tx(
            vec![RxStep::Consumed, RxStep::Empty],
            (0..4).map(|_| TxSubmitStep::Submitted).collect(),
            vec![],
            true,
        );
        mutex.lock().attach_test_clock(clock);
        let notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));
        let (lifecycle, mut fut) = leaked_future(mutex, notify);
        fut.diag_test_clock = Some(clock);
        let count = Arc::new(AtomicUsize::new(0));

        // Commit a long-lived submit hold (well under the 2 s max lease).
        mutex
            .lock()
            .diag_control(crate::diag::OP_HOLD_TX_SUBMIT, 1000, t0)
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
        mutex
            .lock()
            .diag_control(crate::diag::OP_RELEASE, 0, t0)
            .unwrap();
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
        // The QEMU diagnostic lease is Service-owned; a hold committed under
        // the injected Service guard only ever gates this owner, so parallel
        // siblings servicing a round stay unaffected.
        let clock = crate::diag::DiagTestClock::new();
        let t0 = clock.load();
        let (mutex, _, _stats) = leaked_service_tx(
            vec![RxStep::Empty],
            vec![TxSubmitStep::Full],
            (0..4).map(|_| TxReclaimStep::Reclaimed).collect(),
            true,
        );
        mutex.lock().attach_test_clock(clock);
        let notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));
        let (lifecycle, mut fut) = leaked_future(mutex, notify);
        fut.diag_test_clock = Some(clock);
        let count = Arc::new(AtomicUsize::new(0));

        mutex
            .lock()
            .diag_control(crate::diag::OP_HOLD_TX_RECLAIM, 1000, t0)
            .unwrap();
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        // The reclaim stage was paused but the round stays Active and the held
        // submit `Again` backpressures without a busy loop or a fault.
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        assert!(mutex.try_lock().is_some());
        // Release the hold; the Service-owned lease never leaks anywhere.
        mutex
            .lock()
            .diag_control(crate::diag::OP_RELEASE, 0, t0)
            .unwrap();
    }

    // ── Iteration 008 rework: shared production control/V3 witnesses ────

    /// Waker that probes whether the injected Service is unlocked at the
    /// moment a queue-work publication fires.
    ///
    /// `publish_queue_work()` wakes synchronously inside the shared control
    /// path. If the Service guard were still held at publication time, the
    /// probe would observe `try_lock` failing, so a successful success event
    /// proves the guard was dropped before the single post-unlock
    /// publication without relying on source order.
    struct UnlockObservingWake {
        mutex: &'static spin::Mutex<Service>,
        unlocked: Arc<AtomicBool>,
        woken: Arc<AtomicUsize>,
    }

    impl alloc::task::Wake for UnlockObservingWake {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.woken.fetch_add(1, Ordering::Relaxed);
            self.unlocked
                .store(self.mutex.try_lock().is_some(), Ordering::Relaxed);
        }
    }

    fn unlock_observing_waker(
        mutex: &'static spin::Mutex<Service>,
        unlocked: Arc<AtomicBool>,
        woken: Arc<AtomicUsize>,
    ) -> Waker {
        Waker::from(Arc::new(UnlockObservingWake {
            mutex,
            unlocked,
            woken,
        }))
    }

    #[cfg(feature = "qemu-diagnostics")]
    #[test]
    fn diagnostic_control_shared_path_is_bounded_and_publishes_after_unlock() {
        // T4.4-R1: the production `diagnostic_control` entry and this test
        // share `diagnostic_control_shared`. The test holds the injected
        // Service and forces the Busy / validation-error / success event
        // ordering that the public entry cannot be driven through in a host
        // test (the production global `SERVICE` is never initialized here).
        let clock = crate::diag::DiagTestClock::new();
        let t0 = 1_000_000_000_000u64;
        clock.store(t0);
        let (mutex, _copy_calls, _stats) =
            leaked_service_tx(vec![RxStep::Empty], vec![], vec![], true);
        mutex.lock().attach_test_clock(clock);
        let notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));

        // Busy: while the Service is held, the shared path must return
        // `ResourceBusy` immediately, change neither the lease nor the queue
        // generation, and publish no event.
        let held = mutex.lock();
        let woken = Arc::new(AtomicUsize::new(0));
        let unlocked = Arc::new(AtomicBool::new(false));
        notify.register_queue(&unlock_observing_waker(
            mutex,
            unlocked.clone(),
            woken.clone(),
        ));
        let before = notify.generation();
        let err = super::diagnostic_control_shared(
            ServiceAccess::Injected(mutex),
            notify,
            crate::diag::OP_HOLD_TX_SUBMIT,
            100,
        );
        assert!(matches!(err, Err(DevError::ResourceBusy)));
        assert_eq!(held.diag_hold_mode(), crate::diag::HOLD_NONE);
        assert_eq!(held.diag_lease_expiry(), 0);
        assert_eq!(
            notify.generation(),
            before,
            "Busy must not advance the queue generation"
        );
        assert_eq!(woken.load(Ordering::Relaxed), 0, "Busy must not publish");
        drop(held);

        // Validation error: an invalid control also publishes nothing and
        // leaves the committed no-hold state untouched.
        let before = notify.generation();
        let err = super::diagnostic_control_shared(
            ServiceAccess::Injected(mutex),
            notify,
            crate::diag::OP_RELEASE,
            1,
        );
        assert!(matches!(err, Err(DevError::InvalidParam)));
        assert_eq!(mutex.lock().diag_hold_mode(), crate::diag::HOLD_NONE);
        assert_eq!(
            notify.generation(),
            before,
            "validation error must not publish"
        );
        assert_eq!(woken.load(Ordering::Relaxed), 0);

        // Success: a valid Hold commits under the guard, and the exactly-once
        // queue-work publication fires only after the guard is dropped — the
        // wake-time probe must observe the Service unlocked.
        let before = notify.generation();
        let res = super::diagnostic_control_shared(
            ServiceAccess::Injected(mutex),
            notify,
            crate::diag::OP_HOLD_TX_SUBMIT,
            100,
        );
        assert!(res.is_ok());
        assert_eq!(
            notify.generation(),
            before + 1,
            "success publishes exactly one event"
        );
        assert_eq!(woken.load(Ordering::Relaxed), 1, "woken exactly once");
        assert!(
            unlocked.load(Ordering::Relaxed),
            "wake-time probe must observe the Service unlocked (post-unlock publication)"
        );
        {
            let guard = mutex.lock();
            assert_eq!(guard.diag_hold_mode(), crate::diag::HOLD_SUBMIT);
            assert_eq!(guard.diag_lease_expiry(), t0 + 100 * crate::diag::NS_PER_MS);
        }

        // Overflow: a checked-deadline overflow fails closed before any
        // mutation or publication; the previous committed Hold survives.
        clock.store(u64::MAX - 10);
        let before = notify.generation();
        let err = super::diagnostic_control_shared(
            ServiceAccess::Injected(mutex),
            notify,
            crate::diag::OP_HOLD_TX_SUBMIT,
            crate::diag::MAX_LEASE_MS,
        );
        assert!(matches!(err, Err(DevError::InvalidParam)));
        assert_eq!(
            notify.generation(),
            before,
            "overflow rejection must not publish"
        );
        assert_eq!(woken.load(Ordering::Relaxed), 1, "no second wake");
        assert_eq!(mutex.lock().diag_hold_mode(), crate::diag::HOLD_SUBMIT);
    }

    #[cfg(feature = "qemu-diagnostics")]
    #[test]
    fn v3_shared_snapshot_path_returns_only_committed_tuples_under_control_and_tick() {
        // T4.4-R2: the public `rx_snapshot_v3` and this test share
        // `rx_snapshot_v3_from`. Every snapshot is assembled under ONE
        // Service guard, so a control or tick ordered on either side of the
        // acquisition can only expose the complete before or after committed
        // tuple — never a synthetic no-hold, torn pair or cross-state mix.
        let clock = crate::diag::DiagTestClock::new();
        let t0 = 1_000_000_000_000u64;
        clock.store(t0);
        let (mutex, _copy_calls, _stats) =
            leaked_service_tx(vec![RxStep::Empty], vec![], vec![], true);
        mutex.lock().attach_test_clock(clock);
        let base = super::rx_snapshot();

        // Before any control: only the committed no-hold tuple is observable.
        let snap = super::rx_snapshot_v3_from(base, ServiceAccess::Injected(mutex));
        assert_eq!(snap.hold_mode, crate::diag::HOLD_NONE);
        assert_eq!(snap.lease_expiry, 0);
        assert_eq!(snap.auto_release_failure, 0);

        // Control ordered BEFORE the snapshot: the shared assembly sees the
        // complete committed after-tuple of Hold A.
        mutex
            .lock()
            .diag_control(crate::diag::OP_HOLD_TX_SUBMIT, 100, t0)
            .unwrap();
        let snap = super::rx_snapshot_v3_from(base, ServiceAccess::Injected(mutex));
        assert_eq!(snap.hold_mode, crate::diag::HOLD_SUBMIT);
        assert_eq!(snap.lease_expiry, t0 + 100 * crate::diag::NS_PER_MS);
        assert_eq!(snap.auto_release_failure, 0);

        // Tick ordered AFTER the snapshot: the snapshot at A's deadline still
        // returns the complete held tuple (it never mutates); only the queue
        // owner's guarded tick commits the after-state.
        clock.store(t0 + 100 * crate::diag::NS_PER_MS);
        let snap = super::rx_snapshot_v3_from(base, ServiceAccess::Injected(mutex));
        assert_eq!(snap.hold_mode, crate::diag::HOLD_SUBMIT);
        assert_eq!(snap.auto_release_failure, 0);
        let mode = mutex.lock().diag_hold_tick();
        assert_eq!(mode, crate::diag::HOLD_NONE);
        let snap = super::rx_snapshot_v3_from(base, ServiceAccess::Injected(mutex));
        assert_eq!(snap.hold_mode, crate::diag::HOLD_NONE);
        assert_eq!(snap.lease_expiry, 0);
        assert_eq!(snap.auto_release_failure, 1);

        // A replacement Hold B ordered before the snapshot: only B's
        // committed tuple is observable, never a stale A.
        mutex
            .lock()
            .diag_control(crate::diag::OP_HOLD_TX_RECLAIM, 200, t0)
            .unwrap();
        let snap = super::rx_snapshot_v3_from(base, ServiceAccess::Injected(mutex));
        assert_eq!(snap.hold_mode, crate::diag::HOLD_RECLAIM);
        assert_eq!(snap.lease_expiry, t0 + 200 * crate::diag::NS_PER_MS);
        assert_eq!(snap.auto_release_failure, 1);
    }

    #[cfg(feature = "qemu-diagnostics")]
    #[test]
    fn diagnostic_control_public_entry_delegates_to_shared_path_in_source() {
        // T4.4-R1 source guard: the production control entry must delegate to
        // the shared bounded path. Re-implementing acquisition or publication
        // in the entry would silently bypass the witnessed Busy / unlock-order
        // contract.
        let source = include_str!("lib.rs");
        let start = source.find("pub fn diagnostic_control").unwrap();
        let rest = &source[start..];
        let end = rest.find("/// Reserves the sole C4 flush waiter").unwrap();
        let entry = &rest[..end];
        assert!(
            entry.contains("diagnostic_control_shared"),
            "production entry must call the shared bounded control path"
        );
        assert!(
            !entry.contains("try_lock"),
            "production entry must not implement Service acquisition itself"
        );
        assert!(
            !entry.contains("publish_queue_work"),
            "production entry must not publish the event itself"
        );
    }

    #[test]
    fn rx_snapshot_v3_public_entry_delegates_to_shared_assembly_in_source() {
        // T4.4-R2 source guard: the public V3 entry must delegate to the
        // shared one-guard assembly seam. Re-inlining Service reads in the
        // entry would bypass the committed-tuple/ledger witness.
        let source = include_str!("async_rx.rs");
        let start = source.find("pub fn rx_snapshot_v3()").unwrap();
        let rest = &source[start..];
        let end = rest.find("/// C5/T4.4-R2 shared V3 assembly seam").unwrap();
        let entry = &rest[..end];
        assert!(
            entry.contains("rx_snapshot_v3_from"),
            "public entry must delegate to the shared V3 assembly seam"
        );
        assert!(
            !entry.contains("v3_slot_ledger"),
            "public entry must not read the ledger itself"
        );
        assert!(
            !entry.contains("diag_hold_mode"),
            "public entry must not read the lease itself"
        );
    }

    // ── RW-1: lease deadline drives the owner wake (fixture clock) ──────

    /// Task 5.2 (Iteration 006): per-test fixture clock attached to both the
    /// injected Service and the future it drives, so lease-deadline decisions
    /// never share process-global `TEST_NOW` across parallel tests.
    #[cfg(feature = "qemu-diagnostics")]
    fn fixture_clock(
        mutex: &'static spin::Mutex<Service>,
        fut: &mut RxRxFuture,
        nanos: u64,
    ) -> crate::diag::DiagTestClock {
        let clock = crate::diag::DiagTestClock::new();
        clock.store(nanos);
        mutex.lock().attach_test_clock(clock);
        fut.diag_test_clock = Some(clock);
        clock
    }

    #[cfg(feature = "qemu-diagnostics")]
    #[test]
    fn hold_submit_lease_deadline_wakes_and_auto_releases_exactly_once() {
        // Fake clock at T0: commit a 100 ms submit hold. Without an external
        // event, the only way the owner can wake is the lease deadline.
        let t0 = 1_000_000_000_000u64;
        let (mutex, _copy_calls, _stats) = leaked_service_tx(
            vec![RxStep::Empty],
            (0..4).map(|_| TxSubmitStep::Submitted).collect(),
            vec![],
            true,
        );
        let notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));
        let (lifecycle, mut fut) = leaked_future(mutex, notify);
        let clock = fixture_clock(mutex, &mut fut, t0);
        let count = Arc::new(AtomicUsize::new(0));

        mutex
            .lock()
            .diag_control(crate::diag::OP_HOLD_TX_SUBMIT, 100, t0)
            .unwrap();

        // Poll before the deadline: the future sleeps with the deadline armed
        // and must not self-wake or auto-release yet.
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        assert_eq!(count.load(Ordering::Relaxed), 0, "no wake before deadline");
        assert_eq!(mutex.lock().diag_auto_release_failure(), 0);
        assert!(mutex.try_lock().is_some());

        // Just before the deadline: still sleeping, no wake, no auto-release.
        clock.store(t0 + 99 * crate::diag::NS_PER_MS);
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(count.load(Ordering::Relaxed), 0, "no wake before deadline");
        assert_eq!(mutex.lock().diag_auto_release_failure(), 0);

        // At the deadline the fake clock elapses: the future wakes exactly
        // once and the next round auto-releases the expired hold exactly once.
        clock.store(t0 + 100 * crate::diag::NS_PER_MS);
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(count.load(Ordering::Relaxed), 1, "deadline wake fires once");
        assert_eq!(mutex.lock().diag_auto_release_failure(), 1);
        assert_eq!(mutex.lock().diag_hold_mode(), crate::diag::HOLD_NONE);
        // The resumed submit stage drains the queued frames.
        {
            let mut guard = mutex.lock();
            let submits = guard.router_for_test().devices[0].tx_submit_calls_for_test();
            assert!(submits >= 1);
        }
        // A later poll must not auto-release a second time.
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(mutex.lock().diag_auto_release_failure(), 1);
        assert!(mutex.try_lock().is_some());
    }

    #[cfg(feature = "qemu-diagnostics")]
    #[test]
    fn held_reclaim_visible_tx_completion_does_not_busy_loop_before_deadline() {
        let t0 = 1_000_000_000_000u64;
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
        let (lifecycle, mut fut) = leaked_future(mutex, notify);
        let clock = fixture_clock(mutex, &mut fut, t0);
        let count = Arc::new(AtomicUsize::new(0));

        mutex
            .lock()
            .diag_control(crate::diag::OP_HOLD_TX_RECLAIM, 100, t0)
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
            assert_eq!(mutex.lock().diag_auto_release_failure(), 0);
            assert!(mutex.try_lock().is_some());
        }

        // At the deadline the hold auto-releases exactly once.
        clock.store(t0 + 100 * crate::diag::NS_PER_MS);
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(mutex.lock().diag_auto_release_failure(), 1);
        assert_eq!(mutex.lock().diag_hold_mode(), crate::diag::HOLD_NONE);
    }

    #[cfg(feature = "qemu-diagnostics")]
    #[test]
    fn explicit_release_invalidates_stale_deadline_and_new_lease_is_not_released() {
        let t0 = 1_000_000_000_000u64;
        let (mutex, _copy_calls, _stats) = leaked_service_tx(
            vec![RxStep::Empty],
            (0..4).map(|_| TxSubmitStep::Submitted).collect(),
            vec![],
            true,
        );
        let notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));
        let (_lifecycle, mut fut) = leaked_future(mutex, notify);
        let clock = fixture_clock(mutex, &mut fut, t0);
        let count = Arc::new(AtomicUsize::new(0));

        // Hold A with a 100 ms lease.
        mutex
            .lock()
            .diag_control(crate::diag::OP_HOLD_TX_SUBMIT, 100, t0)
            .unwrap();
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(mutex.lock().diag_hold_mode(), crate::diag::HOLD_SUBMIT);

        // Explicit Release before the deadline: the stage resumes and the
        // stale deadline must be invalidated.
        mutex
            .lock()
            .diag_control(crate::diag::OP_RELEASE, 0, t0)
            .unwrap();
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(mutex.lock().diag_hold_mode(), crate::diag::HOLD_NONE);
        assert_eq!(mutex.lock().diag_auto_release_failure(), 0);
        {
            let mut guard = mutex.lock();
            let submits = guard.router_for_test().devices[0].tx_submit_calls_for_test();
            assert!(submits >= 1, "release resumes the paused stage");
        }

        // A new hold B with a longer lease must not be released by the stale
        // deadline from hold A.
        mutex
            .lock()
            .diag_control(crate::diag::OP_HOLD_TX_SUBMIT, 200, t0)
            .unwrap();
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(mutex.lock().diag_hold_mode(), crate::diag::HOLD_SUBMIT);

        // Advance past hold A's old deadline: B must stay held.
        clock.store(t0 + 100 * crate::diag::NS_PER_MS);
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(mutex.lock().diag_hold_mode(), crate::diag::HOLD_SUBMIT);
        assert_eq!(mutex.lock().diag_auto_release_failure(), 0);

        // Only B's own deadline releases it, exactly once.
        clock.store(t0 + 200 * crate::diag::NS_PER_MS);
        assert!(matches!(poll_once(&mut fut, count.clone()), Poll::Pending));
        assert_eq!(mutex.lock().diag_auto_release_failure(), 1);
        assert_eq!(mutex.lock().diag_hold_mode(), crate::diag::HOLD_NONE);
        assert!(mutex.try_lock().is_some());
    }

    #[test]
    fn recoverable_fault_commits_new_epoch_with_resident_owner() {
        // Task 2.2 / A4 / Find 2: a data-plane fault on a recovery-capable
        // device must NOT exit the owner; the same future walks
        // `Quiescing -> Resetting -> Reinitializing -> Active`, commits the new
        // epoch, and reopens the I/O gate.
        let (mutex, stats) = leaked_service_recovering();
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);

        // poll1: fault -> Quiescing -> drained quiesce -> begin Resetting.
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Resetting);
        assert_eq!(stats.begin_calls.load(Ordering::Relaxed), 1);
        assert_eq!(stats.step_calls.load(Ordering::Relaxed), 0);
        assert!(
            stats.recovery_hold.load(Ordering::Relaxed),
            "gate held mid-recovery"
        );

        // poll2: step -> Reinitializing.
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Reinitializing);
        assert_eq!(stats.step_calls.load(Ordering::Relaxed), 1);

        // poll3: step -> Recovered -> commit epoch, return to Active, reopen gate.
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(stats.committed_epoch.load(Ordering::Relaxed), 1);
        assert_eq!(stats.step_calls.load(Ordering::Relaxed), 2);
        assert!(fut.recovery.is_none(), "recovery cleared after commit");
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        assert!(
            !stats.recovery_hold.load(Ordering::Relaxed),
            "gate reopened after commit"
        );
        // A2: the pre-submit cancellation is exactly-once at the quiesce entry;
        // the reset-begin handoff and the success commit must not repeat it.
        assert_eq!(stats.cancel_queued_calls.load(Ordering::Relaxed), 1);
        assert_eq!(stats.cancel_pending_calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn recovery_entry_preserves_origin_stage_for_fault_summary() {
        // F2 / R4: the data-plane fault that triggered the resident recovery
        // (here an RX-copy failure classified as COMPLETION_WAIT) must be
        // preserved as the origin stage, so a fault summary records why the
        // owner entered recovery even if a later reset stage fails.
        let (mutex, _stats) = leaked_service_recovering();
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);

        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert!(fut.recovery.is_some());
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Resetting);
        assert_eq!(
            fut.telemetry.recover_origin_stage.load(Ordering::Relaxed),
            recover_stage::COMPLETION_WAIT,
            "origin stage of the triggering data-plane fault is preserved"
        );
    }

    #[test]
    fn recovery_step_error_quarantines_owner_in_faulted_and_holds_gate() {
        // Task 2.2 / A5 / Find 2: a driver recovery-step failure quarantines
        // the same owner in `Faulted`, holds the I/O gate, and never resumes
        // stepping (the future stays resident and Pending).
        let (mutex, stats) = leaked_service_recovering();
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);

        // poll1: quiesce + begin Resetting.
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Resetting);

        stats.step_error.store(true, Ordering::Relaxed);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        assert_eq!(fut.recovery, Some(RecoveryState::Faulted));
        assert!(
            stats.recovery_hold.load(Ordering::Relaxed),
            "gate stays held on quarantine"
        );

        // A later poll must NOT resume stepping: the owner is resident.
        let before = stats.step_calls.load(Ordering::Relaxed);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(stats.step_calls.load(Ordering::Relaxed), before);
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
    }

    #[test]
    fn same_stage_pending_does_not_renew_absolute_deadline_then_times_out() {
        // Task 2.2 / A4 / Find 3: a recovery stage that stays in the same
        // Pending must NOT re-arm its absolute deadline; a stalled driver
        // eventually times out and quarantines instead of being renewed forever.
        let (mutex, stats) = leaked_service_recovering();
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);

        // poll1: quiesce + begin Resetting; reset deadline armed at 0 + 2 s.
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(fut.recovery, Some(RecoveryState::Resetting));
        assert_eq!(fut.recovery_deadline, Some(2_000_000_000));

        // Stall the driver at Resetting; advance partway into the deadline.
        stats.stall_stage.store(true, Ordering::Relaxed);
        clock.store(1_000_000_000);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        // Same-stage Pending must NOT renew the absolute deadline.
        assert_eq!(
            fut.recovery_deadline,
            Some(2_000_000_000),
            "same-stage pending must not re-arm the deadline"
        );
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Resetting);

        // Advance past the deadline: the stage times out and quarantines.
        clock.store(2_000_000_000);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        assert_eq!(fut.recovery, Some(RecoveryState::Faulted));
        assert!(fut.recovery_deadline.is_none());
    }

    #[test]
    fn pending_reset_schedules_next_progress_wake_before_deadline() {
        // Cycle 005 / T4.2-R1: after a same-stage Pending at the Resetting
        // stage, the resident owner must record a next one-shot progress wake
        // that is strictly after `now` and no later than the absolute stage
        // deadline. Without it the owner sleeps untouched until the final
        // deadline and faults, even though a delayed reset could have recovered
        // within the window.
        let (mutex, stats) = leaked_service_recovering();
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);

        // poll1: quiesce + begin Resetting; reset deadline armed at 0 + 2 s.
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(fut.recovery, Some(RecoveryState::Resetting));
        assert_eq!(fut.recovery_deadline, Some(2_000_000_000));

        // Stall the driver at Resetting and advance 100 ms into the window.
        stats.stall_stage.store(true, Ordering::Relaxed);
        clock.store(100_000_000);
        let (res, wakes) = poll_observe(&mut fut, Arc::new(AtomicUsize::new(0)));
        assert!(matches!(res, Poll::Pending));
        assert_eq!(
            wakes, 0,
            "same-stage Pending must not immediately self-wake (no busy loop)"
        );
        // The owner must schedule a progress wake strictly after `now` (so no
        // immediate self-wake / busy loop) and no later than the final deadline.
        let wake = fut
            .recovery_progress_wake
            .expect("same-stage Pending must leave a next progress wake");
        assert!(
            100_000_000 < wake && wake <= 2_000_000_000,
            "progress wake must be strictly after now and <= deadline, got {wake}"
        );
        // Same-stage Pending must NOT renew the absolute deadline.
        assert_eq!(fut.recovery_deadline, Some(2_000_000_000));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Resetting);
    }

    #[test]
    fn delayed_reset_confirmation_recovers_after_multiple_steps_before_deadline() {
        // Cycle 005 / T4.2-R1: a delayed reset that requires several bounded
        // driver steps (same-stage Pending across polls) must eventually
        // confirm and recover within the absolute stage deadline, driven by the
        // progress cadence. The owner runs one driver step per poll, never hops
        // straight to the deadline, and commits the new epoch only after the
        // reset confirms.
        let (mutex, stats) = leaked_service_recovering();
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);

        // poll1: quiesce + begin Resetting.
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(fut.recovery, Some(RecoveryState::Resetting));
        assert_eq!(fut.recovery_deadline, Some(2_000_000_000));

        // Delayed confirmation: stall at Resetting across several polls, each
        // of which performs exactly one bounded driver step.
        stats.stall_stage.store(true, Ordering::Relaxed);
        clock.store(100_000_000);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        clock.store(500_000_000);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        // Two stalled polls produced two bounded same-stage steps within deadline.
        assert_eq!(stats.step_calls.load(Ordering::Relaxed), 2);
        assert!(fut.recovery_progress_wake.is_some());
        assert_eq!(fut.recovery, Some(RecoveryState::Resetting));

        // Reset finally confirms; the owner advances to Reinitializing (one
        // more bounded step), then to Recovered and commits a fresh epoch.
        stats.stall_stage.store(false, Ordering::Relaxed);
        clock.store(600_000_000);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(stats.step_calls.load(Ordering::Relaxed), 3);
        assert_eq!(fut.recovery, Some(RecoveryState::Reinitializing));

        clock.store(1_000_000_000);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(stats.step_calls.load(Ordering::Relaxed), 4);
        assert!(
            fut.recovery.is_none(),
            "recovery committed after confirmation"
        );
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        assert_eq!(stats.committed_epoch.load(Ordering::Relaxed), 1);
        assert!(
            fut.recovery_progress_wake.is_none(),
            "progress wake cleared after commit"
        );
    }

    #[test]
    fn recovery_stage_timeout_quarantines_stalled_driver() {
        // Task 2.2 / A4 / Find 2: a stage that does not advance past its
        // deadline must quarantine the owner resident in `Faulted` (never
        // block it forever), driven by the deterministic recovery clock.
        let (mutex, _stats) = leaked_service_recovering();
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);
        // poll1: quiesce + begin Resetting.
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert!(fut.recovery.is_some());
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Resetting);
        // Pass the reset deadline without the stage advancing.
        clock.store(3_000_000_000);
        let count2 = Arc::new(AtomicUsize::new(0));
        assert!(matches!(poll_once(&mut fut, count2.clone()), Poll::Pending));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        assert_eq!(fut.recovery, Some(RecoveryState::Faulted));
        assert!(fut.recovery_deadline.is_none());
    }

    #[test]
    fn ownership_drift_quarantines_without_driver_recovery() {
        // F4 / A1/A5 / D3: an ownership/identity drift (`BadState`) on a
        // recovery-capable device must commit `Faulted` resident and hold the
        // gate WITHOUT calling driver recovery (a reset must never mask a
        // corrupt ledger). This is distinct from the `Recover(Io)` path that
        // drives `begin_recovery`.
        let (mutex, stats) = leaked_service_recovering();
        stats.drift_pending.store(true, Ordering::Relaxed);
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);

        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        assert_eq!(fut.recovery, Some(RecoveryState::Faulted));
        assert!(
            stats.recovery_hold.load(Ordering::Relaxed),
            "gate must stay held on drift quarantine"
        );
        assert_eq!(
            stats.begin_calls.load(Ordering::Relaxed),
            0,
            "ownership drift must never call driver recovery"
        );
        assert_eq!(
            stats.committed_epoch.load(Ordering::Relaxed),
            0,
            "ownership drift must never advance the epoch"
        );

        // A later poll must NOT resume stepping or reset.
        let before = stats.step_calls.load(Ordering::Relaxed);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(stats.step_calls.load(Ordering::Relaxed), before);
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
    }

    #[test]
    fn ownership_drift_cancels_pre_submit_owners_exactly_once() {
        // Plan Review Finding 1 (Task 2.1 / R2-R4): entering drift quarantine
        // must invoke the pre-submit owner-cancellation passthroughs
        // (`tx_cancel_queued_target` + `tx_cancel_pending_target`) under the
        // same single Service guard that terminates DeviceOwned and commits
        // flush, each exactly once, before the permanently-Faulted owner is
        // committed. This witness proves the same-guard call sequence and
        // ordering at the async_rx layer; the real state closure (Queued slot
        // and ledger closing together, CancelledPreSubmit flush outcome) is
        // separately witnessed on a real `EthernetDevice` in
        // `device::tests::tx_cancel_queued_closes_slot_and_ledger_in_same_holder`.
        let (mutex, stats) = leaked_service_recovering();
        stats.drift_pending.store(true, Ordering::Relaxed);
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);

        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        assert_eq!(fut.recovery, Some(RecoveryState::Faulted));
        assert!(
            stats.recovery_hold.load(Ordering::Relaxed),
            "gate must stay held on drift quarantine"
        );
        // The pre-submit cancellation passthroughs must be invoked exactly once
        // inside the same guard that terminates DeviceOwned and commits flush.
        assert_eq!(
            stats.cancel_queued_calls.load(Ordering::Relaxed),
            1,
            "Queued pre-submit tickets must be cancelled exactly once on drift"
        );
        assert_eq!(
            stats.cancel_pending_calls.load(Ordering::Relaxed),
            1,
            "ARP-pending pre-submit packets must be dropped exactly once on drift"
        );
        assert_eq!(
            stats.fault_device_owned_calls.load(Ordering::Relaxed),
            1,
            "DeviceOwned must terminate as Fault(OwnershipDrift) exactly once"
        );
        // Drift must still never drive driver recovery.
        assert_eq!(stats.begin_calls.load(Ordering::Relaxed), 0);
        assert_eq!(stats.step_calls.load(Ordering::Relaxed), 0);
        assert_eq!(stats.committed_epoch.load(Ordering::Relaxed), 0);

        // A later poll must not repeat the cancellations or resume recovery.
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(
            stats.cancel_queued_calls.load(Ordering::Relaxed),
            1,
            "pre-submit cancellation must happen exactly once, not once per poll"
        );
        assert_eq!(stats.cancel_pending_calls.load(Ordering::Relaxed), 1);
        assert_eq!(stats.step_calls.load(Ordering::Relaxed), 0);
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
    }

    #[test]
    fn ownership_drift_freezes_structured_fault_summary() {
        // F2 / A5 / R4: a recovery/ownership fault must freeze the stage, epoch
        // and the driver's real owner/resource summary (available / device-
        // owned / quarantined) into internal telemetry, so a fault is
        // diagnosable without a new wire field (the V1–V3 ABI stays frozen).
        let (mutex, stats) = leaked_service_recovering();
        stats.drift_pending.store(true, Ordering::Relaxed);
        stats.owner_available.store(10, Ordering::Relaxed);
        stats.owner_device_owned.store(3, Ordering::Relaxed);
        stats.owner_quarantined.store(5, Ordering::Relaxed);
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);

        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        assert_eq!(
            fut.telemetry.recover_fault_stage.load(Ordering::Relaxed),
            recover_stage::OWNERSHIP_DRIFT
        );
        assert_eq!(fut.telemetry.recover_available.load(Ordering::Relaxed), 10);
        assert_eq!(
            fut.telemetry.recover_device_owned.load(Ordering::Relaxed),
            3
        );
        assert_eq!(fut.telemetry.recover_quarantined.load(Ordering::Relaxed), 5);
        // F2: the summary must freeze the real software ticket epoch (read after
        // the Faulted commit), never the `u64::MAX` unreadable sentinel.
        assert_eq!(
            fut.telemetry.recover_fault_epoch.load(Ordering::Relaxed),
            0,
            "epoch frozen from the faulted target, not the unavailable sentinel"
        );
    }

    #[test]
    fn recover_stage_codes_are_distinct_and_stable() {
        // F2 / R4: the six D3 recovery stages must each map to a distinct,
        // stable internal code so a fault summary identifies the exact stage
        // (submit wait / completion wait / reclaim / quiesce / reset /
        // reinitialize) plus the ownership-drift separator.
        assert_ne!(recover_stage::SUBMIT_WAIT, recover_stage::COMPLETION_WAIT);
        assert_ne!(recover_stage::COMPLETION_WAIT, recover_stage::RECLAIM);
        assert_ne!(recover_stage::RECLAIM, recover_stage::QUIESCE);
        assert_ne!(recover_stage::QUIESCE, recover_stage::RESET);
        assert_ne!(recover_stage::RESET, recover_stage::REINITIALIZE);
        assert_ne!(recover_stage::REINITIALIZE, recover_stage::OWNERSHIP_DRIFT);
        assert_ne!(recover_stage::OWNERSHIP_DRIFT, recover_stage::UNKNOWN);
        for code in [
            recover_stage::SUBMIT_WAIT,
            recover_stage::COMPLETION_WAIT,
            recover_stage::RECLAIM,
            recover_stage::QUIESCE,
            recover_stage::RESET,
            recover_stage::REINITIALIZE,
            recover_stage::OWNERSHIP_DRIFT,
        ] {
            assert!(code >= 1 && code <= 7, "stage code in diagnostic range");
        }
    }

    #[test]
    fn recovery_step_error_wakes_only_after_guard_released_and_faulted_committed() {
        // F5 / A3–A5 / R1: the recovery-step fault path must NOT wake the
        // queue/stack/flush waiters while the Service guard is still held, and
        // must commit `Faulted` before publishing. An UnlockObservingWake on
        // the stack role samples `try_lock` of the injected Service inside the
        // wake callback: a success proves the guard was dropped before the wake.
        let (mutex, stats) = leaked_service_recovering();
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let queue_notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));
        let stack_notify: &'static StackEvent = Box::leak(Box::new(StackEvent::new()));
        let (lifecycle, mut fut) = leaked_future_with_stack(mutex, queue_notify, stack_notify);
        fut.recovery_test_clock = Some(clock);

        // poll1: fault -> Quiescing -> begin Resetting (guard dropped between
        // polls; no step error yet).
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Resetting);

        // Register an unlock-observing waker on the stack role so the fault
        // publication calls it.
        let unlocked = Arc::new(AtomicBool::new(false));
        let woken = Arc::new(AtomicUsize::new(0));
        stack_notify.register(&unlock_observing_waker(
            mutex,
            unlocked.clone(),
            woken.clone(),
        ));

        // poll2: a driver step error must publish only after the guard drop.
        stats.step_error.store(true, Ordering::Relaxed);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        assert_eq!(fut.recovery, Some(RecoveryState::Faulted));
        assert_eq!(
            woken.load(Ordering::Relaxed),
            1,
            "recovery-step fault must publish a stack wake"
        );
        assert!(
            unlocked.load(Ordering::Relaxed),
            "F5: the stack wake must observe the Service guard released"
        );
        assert!(
            stats.recovery_hold.load(Ordering::Relaxed),
            "gate stays held after quarantine"
        );
    }

    #[test]
    fn recovery_commit_wakes_flush_only_after_epoch_and_active_committed() {
        // F5 / A3 / R1: on a successful recovery commit, the old-epoch flush is
        // settled and woken only AFTER the epoch advanced and the lifecycle
        // returned to Active — never before, so a woken observer never reads a
        // half-committed state. The flush waiter events land outside the guard.
        let (mutex, _stats) = leaked_service_recovering();
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);

        // poll1: quiesce + begin Resetting.
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Resetting);
        // poll2: Reinitializing.
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Reinitializing);
        // poll3: Recovered -> commit Active + reopen gate; the flush close +
        // advance happens before the lifecycle returns to Active.
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        assert!(fut.recovery.is_none());
    }

    #[test]
    fn quiesce_budget_self_wakes_so_backlog_converges() {
        // Task 2.3 / R3-D3 / gap 3: a quiesce backlog larger than the per-poll
        // RECLAIM_BUDGET must NOT stall until the 1 s expiry. Poll 1 enters
        // recovery; poll 2, already in Quiescing, reclaims the next bounded
        // budget and must self-wake (woken grows) so the executor keeps
        // converging instead of waiting on the timer or an external event.
        let (mutex, stats) = leaked_service_recovering();
        stats.device_owned.store(300, Ordering::Relaxed);
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (_lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);

        let wakes = Arc::new(AtomicUsize::new(0));
        let (r1, c1) = poll_observe(&mut fut, wakes.clone());
        assert!(r1.is_pending());
        assert!(fut.recovery.is_some(), "recovery entered in the first poll");
        assert_eq!(stats.begin_calls.load(Ordering::Relaxed), 0);
        assert_eq!(stats.cancel_queued_calls.load(Ordering::Relaxed), 1);

        let (r2, c2) = poll_observe(&mut fut, wakes);
        assert!(r2.is_pending());
        assert!(
            c2 > c1,
            "a budget-exhausted quiesce poll already in recovery must self-wake to keep converging"
        );
        assert_eq!(stats.begin_calls.load(Ordering::Relaxed), 0);

        while stats.device_owned.load(Ordering::Relaxed) != 0 {
            assert!(matches!(
                poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
                Poll::Pending
            ));
        }
        assert_eq!(stats.begin_calls.load(Ordering::Relaxed), 1);
        assert_eq!(fut.recovery, Some(RecoveryState::Resetting));
    }

    #[test]
    fn quiesce_natural_drain_begins_reset_within_budget() {
        // Task 2.3 / R3-D3 / gap 1: a DeviceOwned backlog smaller than the
        // per-poll budget drains in one poll and the owner goes straight to
        // Resetting (begin exactly once), never waiting for the quiesce deadline.
        let (mutex, stats) = leaked_service_recovering();
        stats.device_owned.store(16, Ordering::Relaxed);
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (_lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);

        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(stats.device_owned.load(Ordering::Relaxed), 0);
        assert_eq!(stats.begin_calls.load(Ordering::Relaxed), 1);
        assert_eq!(fut.recovery, Some(RecoveryState::Resetting));
    }

    #[test]
    fn quiesce_1s_expiry_begins_reset_with_remaining_owner() {
        // Task 2.3 / R3-D4 / gap 3: a device with DeviceOwned yet no visible
        // completion drains nothing. The owner must wait for the 1 s quiesce
        // deadline (no busy-loop, no begin before it) and begin reset exactly
        // once at expiry with the full remaining ledger.
        let (mutex, stats) = leaked_service_recovering();
        stats.device_owned.store(64, Ordering::Relaxed);
        stats.reclaim_stall.store(true, Ordering::Relaxed);
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (_lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);

        let wakes = Arc::new(AtomicUsize::new(0));
        let (r1, c1) = poll_observe(&mut fut, wakes.clone());
        assert!(r1.is_pending());
        assert_eq!(stats.begin_calls.load(Ordering::Relaxed), 0);
        assert_eq!(stats.device_owned.load(Ordering::Relaxed), 64);

        clock.store(999_000_000);
        let (r2, c2) = poll_observe(&mut fut, wakes);
        assert!(r2.is_pending());
        assert_eq!(
            c2, c1,
            "a stalled quiesce must not self-pump before the timer"
        );
        assert_eq!(stats.begin_calls.load(Ordering::Relaxed), 0);

        clock.store(1_000_000_000);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(stats.begin_calls.load(Ordering::Relaxed), 1);
        assert_eq!(fut.recovery, Some(RecoveryState::Resetting));
    }

    #[test]
    fn quiesce_reclaim_fault_quarantines_without_reset() {
        // Task 2.3 / R3-D4 / quiesce drift: a reclaim fault during quiesce must
        // commit `Faulted` resident (hold held, no begin, no epoch advance) and
        // record the QUIESCE stage — never mask it with a reset.
        let (mutex, stats) = leaked_service_recovering();
        stats.device_owned.store(16, Ordering::Relaxed);
        stats.reclaim_error.store(true, Ordering::Relaxed);
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);

        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        assert_eq!(fut.recovery, Some(RecoveryState::Faulted));
        assert!(stats.recovery_hold.load(Ordering::Relaxed));
        assert_eq!(
            stats.begin_calls.load(Ordering::Relaxed),
            0,
            "no reset on reclaim fault"
        );
        assert_eq!(stats.committed_epoch.load(Ordering::Relaxed), 0);
        assert_eq!(
            fut.telemetry.recover_fault_stage.load(Ordering::Relaxed),
            recover_stage::QUIESCE
        );
    }

    #[test]
    fn reinitialize_stage_timeout_quarantines_owner() {
        // Task 2.3 / R4-D2 / gap 2: the reinitialize stage owns a distinct 2 s
        // absolute deadline (re-armed on entry), so a reinit stall must time out
        // into resident `Faulted` and record the REINITIALIZE identity.
        let (mutex, stats) = leaked_service_recovering();
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Reinitializing);
        assert_eq!(fut.recovery_deadline, Some(2_000_000_000));

        stats.stall_stage.store(true, Ordering::Relaxed);
        clock.store(1_000_000_000);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(
            fut.recovery_deadline,
            Some(2_000_000_000),
            "same-stage reinit pending must not renew the absolute deadline"
        );
        clock.store(2_000_000_000);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        assert_eq!(fut.recovery, Some(RecoveryState::Faulted));
        assert_eq!(
            fut.telemetry.recover_fault_stage.load(Ordering::Relaxed),
            recover_stage::REINITIALIZE,
            "a reinit-stage timeout carries the REINITIALIZE stage identity"
        );
    }

    #[test]
    fn reinitialize_step_error_quarantines_with_reinit_identity() {
        // Task 2.3 / R4-D2 / gap 2: a driver step error at the reinitialize
        // stage quarantines the owner resident and records REINITIALIZE.
        let (mutex, stats) = leaked_service_recovering();
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Reinitializing);

        stats.step_error.store(true, Ordering::Relaxed);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        assert_eq!(fut.recovery, Some(RecoveryState::Faulted));
        assert_eq!(
            fut.telemetry.recover_fault_stage.load(Ordering::Relaxed),
            recover_stage::REINITIALIZE
        );
    }

    #[test]
    fn begin_error_quarantines_with_reset_stage() {
        // Task 2.3 / R4-D2 / gap 4: a failure at the reset-begin handoff must
        // be reported with the RESET stage, matching the lifecycle that already
        // advanced `active -> quiescing -> resetting`, not an inconsistent
        // quiesce-stage identity. A2: the pre-submit cancellation happens
        // exactly once (at quiesce entry) and never repeats on the reset-begin
        // handoff or on a later Faulted-resident poll.
        let (mutex, stats) = leaked_service_recovering();
        stats.begin_error.store(true, Ordering::Relaxed);
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);

        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        assert_eq!(fut.recovery, Some(RecoveryState::Faulted));
        assert!(stats.recovery_hold.load(Ordering::Relaxed));
        assert_eq!(
            stats.cancel_queued_calls.load(Ordering::Relaxed),
            1,
            "A2: Queued pre-submit tickets are cancelled exactly once, at the quiesce entry"
        );
        assert_eq!(
            stats.cancel_pending_calls.load(Ordering::Relaxed),
            1,
            "A2: ARP-pending pre-submit packets are dropped exactly once"
        );
        assert_eq!(stats.committed_epoch.load(Ordering::Relaxed), 0);
        assert_eq!(
            fut.telemetry.recover_fault_stage.load(Ordering::Relaxed),
            recover_stage::RESET,
            "a reset-begin failure carries the RESET stage, not a quiesce/lifecycle split"
        );

        // A later Faulted-resident poll must NOT repeat the cancellation.
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(stats.cancel_queued_calls.load(Ordering::Relaxed), 1);
        assert_eq!(stats.cancel_pending_calls.load(Ordering::Relaxed), 1);
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
    }

    #[test]
    fn recovery_success_reopens_gate_and_serves_new_epoch() {
        // Task 2.3 / R1-A5 / gaps 5 & 6: after a full successful recovery the
        // device epoch advances, the queue owner ledger is live at the new
        // epoch, the I/O gate reopens, and the commit wake fires only after the
        // Service guard is released. A follow-up Active poll stays in service.
        let (mutex, stats) = leaked_service_recovering();
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let queue_notify: &'static QueueEvent = Box::leak(Box::new(QueueEvent::new()));
        let stack_notify: &'static StackEvent = Box::leak(Box::new(StackEvent::new()));
        let (_lifecycle, mut fut) = leaked_future_with_stack(mutex, queue_notify, stack_notify);
        fut.recovery_test_clock = Some(clock);

        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        let unlocked = Arc::new(AtomicBool::new(false));
        let woken = Arc::new(AtomicUsize::new(0));
        {
            let waker = unlock_observing_waker(mutex, unlocked.clone(), woken.clone());
            let mut cx = Context::from_waker(&waker);
            assert!(Pin::new(&mut fut).poll(&mut cx).is_pending());
        }
        assert!(
            woken.load(Ordering::Relaxed) > 0,
            "the successful recovery commit publishes a self-wake"
        );
        assert!(
            unlocked.load(Ordering::Relaxed),
            "the commit self-wake fires only after the Service guard is released"
        );
        assert_eq!(stats.committed_epoch.load(Ordering::Relaxed), 1);
        assert!(
            !stats.recovery_hold.load(Ordering::Relaxed),
            "gate reopened"
        );
        assert_eq!(fut.recovery, None);
        assert_eq!(
            mutex.lock().queue_epoch_target().current(),
            1,
            "the queue owner ledger is live at the new epoch"
        );
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(
            fut.recovery, None,
            "the data path keeps serving without re-entering recovery"
        );
    }

    #[test]
    fn recovery_success_resumes_new_epoch_send_submit_reclaim() {
        // Task 2.3 / R1-A5 / gap 5: after a full successful recovery the data
        // path must really move a frame at the new epoch through the real
        // `Device::send` enqueue seam into the epoch-bound `TicketTracker`
        // ledger: send -> submit (Queued -> DeviceOwned, observing the new
        // epoch) -> reclaim (DeviceOwned -> released via an epoch-bound cookie,
        // terminal Reclaimed), with the owner ledger returned to conservation
        // and the I/O gate reopened.
        let (mutex, stats) = leaked_service_ledger_recovering();
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);

        // Drive Quiescing -> Resetting -> Reinitializing -> Recovered (Active).
        for _ in 0..3 {
            assert!(matches!(
                poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
                Poll::Pending
            ));
        }
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        assert_eq!(
            stats.committed_epoch.load(Ordering::Relaxed),
            1,
            "new epoch committed"
        );
        assert!(
            !stats.recovery_hold.load(Ordering::Relaxed),
            "gate reopened"
        );
        assert_eq!(
            mutex.lock().queue_epoch_target().current(),
            1,
            "the queue owner ledger is live at the new epoch"
        );

        // A real Device::send enqueues the frame with an epoch-bound ticket.
        let frame = [0xABu8; 16];
        let hop = IpAddress::Ipv4(smoltcp::wire::Ipv4Address::new(10, 0, 0, 1));
        let ts = Instant::from_millis(0);
        let accepted = {
            let mut s = mutex.lock();
            s.router_for_test().devices[0].send(hop, &frame, ts)
        };
        assert!(
            matches!(accepted, TxOutcome::Accepted { .. }),
            "send accepted into the TX slot"
        );
        assert!(
            mutex.lock().router_for_test().devices[0].tx_slot_pending(),
            "a queued frame awaits submit"
        );

        // The next Active round submits it: Queued -> DeviceOwned at epoch 1.
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        let ticket = stats.submitted_ticket.load(Ordering::Relaxed);
        assert_eq!(
            stats.submit_epoch.load(Ordering::Relaxed),
            1,
            "post-recovery submit runs at the new epoch"
        );
        assert_eq!(ticket, 0, "the first epoch-bound ticket is recorded");
        assert_eq!(
            mutex.lock().device_owned_len_target(),
            1,
            "the submitted frame is device-owned"
        );
        assert!(
            !mutex.lock().router_for_test().devices[0].tx_slot_pending(),
            "the submitted slot is consumed"
        );
        assert!(
            matches!(
                mutex.lock().router_for_test().devices[0].tx_flush_state(Some(ticket)),
                FlushState::Pending
            ),
            "a DeviceOwned ticket not yet reclaimed is pending"
        );

        // A completion arrives; the next round reclaims it through the
        // epoch-bound cookie, returning the ledger to conservation with the
        // Reclaimed terminal outcome (flush reads Done, first_lost stays None).
        stats.completion_armed.store(true, Ordering::Relaxed);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(
            mutex.lock().device_owned_len_target(),
            0,
            "owner ledger conserved after reclaim"
        );
        assert_eq!(
            mutex.lock().queue_epoch_target().current(),
            1,
            "epoch unchanged by a normal reclaim"
        );
        assert!(
            matches!(
                mutex.lock().router_for_test().devices[0].tx_flush_state(Some(ticket)),
                FlushState::Done
            ),
            "the reclaimed ticket reaches the Reclaimed/Done terminal outcome"
        );
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        assert_eq!(
            fut.recovery, None,
            "the resident owner stays Active in the new epoch"
        );
    }

    #[derive(Default)]
    struct DataStageStats {
        device_owned: core::sync::atomic::AtomicU64,
        slot_pending: core::sync::atomic::AtomicBool,
        submit_full: core::sync::atomic::AtomicBool,
        cancel_queued_calls: core::sync::atomic::AtomicUsize,
        /// Advance count from `QueueEpoch::MIN` reported by `queue_epoch()`.
        epoch_offset: core::sync::atomic::AtomicU64,
        queued_present: core::sync::atomic::AtomicBool,
        last_accepted: core::sync::atomic::AtomicU64,
        cancelled_pre_submit: core::sync::atomic::AtomicBool,
        /// When set, `tx_reclaim_one` reports progress (Reclaimed) every round.
        progress_reclaim: core::sync::atomic::AtomicBool,
    }

    /// Task 2.2 / A1–A3 fixture: independently drives the three Active
    /// data-stage waits. `stats.device_owned` (with no visible completion)
    /// blocks the completion wait; `control::tx_completion_visible` plus
    /// `stats.reclaim_empty` blocks the reclaim wait; `stats.slot_pending` plus
    /// `stats.submit_full` blocks the submit wait. The device is
    /// recovery-capable so a completion/reclaim timeout enters resident
    /// recovery, while a submit timeout must cancel the Queued slot without
    /// quarantining.
    struct DataStageDevice {
        stats: Arc<DataStageStats>,
        control: ScriptedControl,
        recovery: ScriptedRecovery,
    }

    impl Device for DataStageDevice {
        fn name(&self) -> &str {
            "datastage"
        }
        fn recv(&mut self, _b: &mut PacketBuffer<()>, _t: Instant) -> RxStep {
            RxStep::Empty
        }
        fn preflight_send(&mut self, _n: IpAddress, _p: &[u8], _t: Instant) -> TxPreflight {
            TxPreflight::Ready
        }
        fn send(&mut self, _n: IpAddress, _p: &[u8], _t: Instant) -> TxOutcome {
            TxOutcome::Accepted {
                rx_became_ready: false,
            }
        }
        fn rx_copy_one(&mut self) -> RxCopyStep {
            RxCopyStep::Empty
        }
        fn tx_submit_one(&mut self) -> TxSubmitStep {
            if self.stats.submit_full.load(Ordering::Relaxed) {
                TxSubmitStep::Full
            } else {
                TxSubmitStep::Empty
            }
        }
        fn tx_reclaim_one(&mut self) -> TxReclaimStep {
            if self.stats.progress_reclaim.load(Ordering::Relaxed) {
                TxReclaimStep::Reclaimed
            } else {
                TxReclaimStep::Empty
            }
        }
        fn rx_slot_has_space(&self) -> bool {
            true
        }
        fn tx_slot_pending(&self) -> bool {
            self.stats.slot_pending.load(Ordering::Relaxed)
        }
        fn tx_last_accepted(&self) -> Option<u64> {
            if self.stats.queued_present.load(Ordering::Relaxed) {
                Some(self.stats.last_accepted.load(Ordering::Relaxed))
            } else {
                None
            }
        }
        fn tx_flush_state(&self, target: Option<u64>) -> crate::device::FlushState {
            use crate::device::{FlushState, TicketOutcome};
            if self.stats.cancelled_pre_submit.load(Ordering::Relaxed) {
                FlushState::Lost(TicketOutcome::CancelledPreSubmit)
            } else if self.stats.queued_present.load(Ordering::Relaxed)
                && target == Some(self.stats.last_accepted.load(Ordering::Relaxed))
            {
                FlushState::Pending
            } else {
                FlushState::Done
            }
        }
        fn queue_epoch(&self) -> QueueEpoch {
            let mut e = QueueEpoch::MIN;
            for _ in 0..self.stats.epoch_offset.load(Ordering::Relaxed) {
                e = e.advance().expect("test epoch headroom");
            }
            e
        }
        fn tx_cancel_queued(&mut self) -> usize {
            self.stats
                .cancel_queued_calls
                .fetch_add(1, Ordering::Relaxed);
            // A1 owner outcome: a real Queued ticket is cancelled exactly once —
            // the slot is popped and the ticket marked CancelledPreSubmit in the
            // same call, so a later poll cannot re-submit it.
            if self.stats.queued_present.swap(false, Ordering::Relaxed) {
                self.stats.slot_pending.store(false, Ordering::Relaxed);
                self.stats
                    .cancelled_pre_submit
                    .store(true, Ordering::Relaxed);
                1
            } else {
                0
            }
        }
        fn tx_fault_device_owned(&mut self, _stage: crate::device::TicketFaultStage) -> usize {
            0
        }
        fn tx_advance_epoch(&mut self, _next: QueueEpoch) {}
        fn tx_set_recovery_hold(&mut self, _held: bool) {}
        fn tx_device_owned_len(&self) -> u64 {
            self.stats.device_owned.load(Ordering::Relaxed)
        }
        fn recovery_control(&mut self) -> Option<&mut dyn NetRecoveryControl> {
            Some(&mut self.recovery)
        }
        fn queue_control(&mut self) -> Option<&mut dyn NetQueueControl> {
            Some(&mut self.control)
        }
        fn register_waker(&self, _w: &Waker) {}
    }

    fn leaked_service_datastage() -> (
        &'static spin::Mutex<Service>,
        Arc<DataStageStats>,
        Arc<RecoveryDriverStats>,
        Arc<ScriptedControlStats>,
    ) {
        let stats = Arc::new(DataStageStats::default());
        let rec = Arc::new(RecoveryDriverStats::default());
        rec.link.store(true, Ordering::Relaxed);
        let ctl = Arc::new(ScriptedControlStats::default());
        let device = DataStageDevice {
            stats: stats.clone(),
            control: ScriptedControl { stats: ctl.clone() },
            recovery: ScriptedRecovery { stats: rec.clone() },
        };
        let mut router = Router::new();
        let idx = router.add_device(Box::new(device));
        let service = Service::new(router, Some(idx));
        (
            Box::leak(Box::new(spin::Mutex::new(service))),
            stats,
            rec,
            ctl,
        )
    }

    #[test]
    fn submit_wait_deadline_cancels_queued_once_without_recovery() {
        // A1 / Findings 2 & 5: a Queued submit the driver never accepts must
        // time out after 1 s and cancel the real Queued slot+ticket exactly
        // once, terminal `CancelledPreSubmit`; a flush whose target is that
        // ticket must fail stably (not pend forever); no driver (raw) submit
        // owner is created; the owner stays Active and the coherent fault
        // records the REAL software epoch (not `u64::MAX`).
        let (mutex, ds, rec, _ctl) = leaked_service_datastage();
        ds.epoch_offset.store(1, Ordering::Relaxed); // real epoch == 1
        ds.queued_present.store(true, Ordering::Relaxed);
        ds.last_accepted.store(7, Ordering::Relaxed);
        ds.slot_pending.store(true, Ordering::Relaxed);
        ds.submit_full.store(true, Ordering::Relaxed);
        let flush = { mutex.lock().flush_begin().unwrap() };
        assert_eq!(flush.target, Some(7), "flush target is the queued ticket");
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);

        // poll1: submit blocked, deadline armed once at 0 + 1 s, nothing
        // cancelled, the flush is still pending.
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(fut.data_deadlines.submit, Some(1_000_000_000));
        assert_eq!(ds.cancel_queued_calls.load(Ordering::Relaxed), 0);
        assert!(
            matches!(
                mutex.lock().flush_recheck(flush.identity, flush.target),
                FlushRecheck::Pending
            ),
            "flush still pending before the submit deadline"
        );

        // poll2 past the deadline: the stuck Queued slot+ticket cancels exactly
        // once (CancelledPreSubmit), the flush fails stably, no recovery begins,
        // the real epoch is recorded and the owner stays Active.
        clock.store(1_000_000_000);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(
            ds.cancel_queued_calls.load(Ordering::Relaxed),
            1,
            "Queued slot+ticket cancelled exactly once"
        );
        assert!(
            !ds.queued_present.load(Ordering::Relaxed),
            "the queued ticket ledger is drained"
        );
        assert!(
            !ds.slot_pending.load(Ordering::Relaxed),
            "the cancelled slot is popped"
        );
        assert!(
            matches!(
                mutex.lock().flush_recheck(flush.identity, flush.target),
                FlushRecheck::Faulted(_)
            ),
            "a flush whose target was cancelled fails stably (no permanent Pending)"
        );
        assert!(
            matches!(
                mutex.lock().router_for_test().devices[0].tx_flush_state(flush.target),
                FlushState::Lost(TicketOutcome::CancelledPreSubmit)
            ),
            "the cancelled ticket resolves to CancelledPreSubmit"
        );
        assert_eq!(
            rec.begin_calls.load(Ordering::Relaxed),
            0,
            "no driver recovery"
        );
        assert!(
            !rec.recovery_hold.load(Ordering::Relaxed),
            "owner is not quarantined on a submit timeout"
        );
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        let identity = fut.telemetry.coherent_fault.read().unwrap();
        assert_eq!(identity.stage, recover_stage::SUBMIT_WAIT);
        assert_eq!(identity.local_cause, fault_cause::TIMEOUT);
        assert_eq!(
            identity.queue_epoch, 1,
            "the Active submit timeout records the REAL software epoch, not u64::MAX"
        );

        // poll3: the packet was cancelled, so the wait resolves and must not
        // cancel a second time on a later poll.
        ds.submit_full.store(false, Ordering::Relaxed);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(ds.cancel_queued_calls.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn submit_wait_same_stage_pending_does_not_renew_absolute_deadline() {
        // A1 / Find 3: repeated polls while the wait is still blocked must NOT
        // move the absolute deadline forward; only the first observation arms it.
        let (mutex, ds, _rec, _ctl) = leaked_service_datastage();
        ds.slot_pending.store(true, Ordering::Relaxed);
        ds.submit_full.store(true, Ordering::Relaxed);
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);

        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(fut.data_deadlines.submit, Some(1_000_000_000));

        // Partway into the deadline: the wait is still blocked, but the absolute
        // deadline must remain armed at 0 + 1 s, not be renewed to now + 1 s.
        clock.store(500_000_000);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(
            fut.data_deadlines.submit,
            Some(1_000_000_000),
            "same-stage pending must not re-arm the deadline"
        );
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
    }

    #[test]
    fn completion_wait_deadline_enters_recovery_with_origin_stage() {
        // A2: a DeviceOwned completion that never arrives must time out after
        // 1 s and enter resident recovery, preserving COMPLETION_WAIT as the origin.
        let (mutex, ds, _rec, _ctl) = leaked_service_datastage();
        ds.device_owned.store(3, Ordering::Relaxed);
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (_lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);

        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(
            fut.data_deadlines.completion,
            Some(1_000_000_000),
            "completion wait armed once on first DeviceOwned-without-completion round"
        );

        // Past the deadline: enter resident recovery with the completion-wait origin.
        clock.store(2_000_000_000);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert!(
            fut.recovery.is_some(),
            "a completion timeout must enter resident recovery"
        );
        assert_eq!(
            fut.telemetry.recover_origin_stage.load(Ordering::Relaxed),
            recover_stage::COMPLETION_WAIT,
            "origin stage of the completion timeout preserved for the fault summary"
        );
        // A2 / Finding 5: entering recovery must NOT release the DeviceOwned
        // backing — the recovery holder keeps it until a confirmed reset or
        // fault. The timeout itself never frees driver buffers.
        assert_eq!(
            ds.device_owned.load(Ordering::Relaxed),
            3,
            "DeviceOwned backing is still retained by the recovery holder after the timeout"
        );
    }

    #[test]
    fn reclaim_wait_deadline_enters_recovery_with_origin_stage() {
        // A3: a visible TX completion that is never reclaimed must time out
        // after 1 s and enter resident recovery with the RECLAIM origin.
        let (mutex, ds, _rec, ctl) = leaked_service_datastage();
        ds.device_owned.store(3, Ordering::Relaxed);
        ctl.tx_completion_visible.store(true, Ordering::Relaxed);
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (_lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);

        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(
            fut.data_deadlines.reclaim,
            Some(1_000_000_000),
            "reclaim wait armed on a visible-but-unreclaimed completion"
        );

        // Past the deadline: enter resident recovery with the reclaim origin.
        clock.store(2_000_000_000);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert!(
            fut.recovery.is_some(),
            "a reclaim timeout must enter resident recovery"
        );
        assert_eq!(
            fut.telemetry.recover_origin_stage.load(Ordering::Relaxed),
            recover_stage::RECLAIM,
            "origin stage of the reclaim timeout preserved"
        );
    }

    #[test]
    fn sustained_reclaim_progress_over_1s_does_not_enter_recovery() {
        // A3 / Finding 3: reclaim progress every round is not a stall. A visible
        // completion with `reclaimed > 0` must clear the reclaim deadline, so
        // even past 1 s of sustained progress the owner must NOT enter recovery.
        let (mutex, ds, rec, ctl) = leaked_service_datastage();
        ds.device_owned.store(3, Ordering::Relaxed);
        ds.progress_reclaim.store(true, Ordering::Relaxed);
        ctl.tx_completion_visible.store(true, Ordering::Relaxed);
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (_lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);

        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(
            fut.data_deadlines.reclaim, None,
            "successful reclaim progress clears the reclaim deadline"
        );

        // Multiple rounds of sustained progress past the 1 s point: no recovery.
        for now in [500_000_000u64, 1_000_000_000, 2_000_000_000] {
            clock.store(now);
            assert!(matches!(
                poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
                Poll::Pending
            ));
            assert_eq!(
                fut.data_deadlines.reclaim, None,
                "progress at {now} ns must not arm a reclaim deadline"
            );
            assert!(
                fut.recovery.is_none(),
                "sustained reclaim progress at {now} ns must not enter recovery"
            );
            assert_eq!(rec.begin_calls.load(Ordering::Relaxed), 0);
        }
    }

    #[test]
    fn zero_device_owned_never_arms_reclaim_deadline() {
        // A3 / Finding 3: with no DeviceOwned owner there is nothing to reclaim,
        // so a visible completion is not a reclaim stall and never arms the
        // reclaim deadline (and never enters recovery).
        let (mutex, _ds, _rec, ctl) = leaked_service_datastage();
        ctl.tx_completion_visible.store(true, Ordering::Relaxed);
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (_lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);

        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        clock.store(2_000_000_000);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(
            fut.data_deadlines.reclaim, None,
            "no reclaim deadline with zero DeviceOwned owners"
        );
        assert!(fut.recovery.is_none(), "no recovery with zero owners");
    }

    #[test]
    fn coherent_fault_sheet_never_returns_torn_tuple_mid_publication() {
        // A4 / Findings 1–2: under a genuinely concurrent mid-publication reader,
        // the bounded seqlock never returns a tuple mixed across two faults.
        // Every round publishes between two DIFFERENT identities (a<->b), so the
        // stress actually exercises transitions both ways; a None (defer) is a
        // legitimate bounded result, but any Some must be a whole identity.
        let sheet = Arc::new(CoherentFaultSheet::new());
        let a = RecoveryFaultIdentity {
            stage: recover_stage::SUBMIT_WAIT,
            local_cause: fault_cause::TIMEOUT,
            queue_epoch: 10,
            available: 100,
            device_owned: 5,
            quarantined: 0,
        };
        let b = RecoveryFaultIdentity {
            stage: recover_stage::OWNERSHIP_DRIFT,
            local_cause: fault_cause::OWNERSHIP_DRIFT,
            queue_epoch: 90,
            available: 0,
            device_owned: 0,
            quarantined: 50,
        };
        sheet.publish(a);
        for i in 0..64 {
            // Alternate the published identity each round so every round is a
            // real a<->b transition, never a same-identity rewrite.
            let publish_b = i % 2 == 0;
            let (base, transit) = if publish_b { (a, b) } else { (b, a) };
            sheet.publish(base);
            std::thread::scope(|scope| {
                let reader = scope.spawn(|| {
                    for _ in 0..50_000 {
                        if let Some(f) = sheet.read() {
                            assert!(
                                f == a || f == b,
                                "torn tuple returned mid-publication: {f:?}"
                            );
                        }
                    }
                });
                sheet.publish(transit);
                reader.join().unwrap();
            });
        }
    }

    #[test]
    fn coherent_fault_sheet_in_progress_defer_is_bounded_and_non_blocking() {
        // A4 / Findings 1–2 (deterministic seam): a writer marked in progress
        // and paused (as when preempted between ODD and EVEN) must yield a
        // bounded None from `read` — never a torn tuple, and never a spin — and
        // the complete tuple only after the writer releases EVEN.
        let sheet = CoherentFaultSheet::new();
        let a = RecoveryFaultIdentity {
            stage: recover_stage::SUBMIT_WAIT,
            local_cause: fault_cause::TIMEOUT,
            queue_epoch: 10,
            available: 100,
            device_owned: 5,
            quarantined: 0,
        };
        let b = RecoveryFaultIdentity {
            stage: recover_stage::OWNERSHIP_DRIFT,
            local_cause: fault_cause::OWNERSHIP_DRIFT,
            queue_epoch: 90,
            available: 0,
            device_owned: 0,
            quarantined: 50,
        };
        sheet.publish(a);
        assert_eq!(sheet.read(), Some(a));

        // Writer starts a publication and is paused at the in-progress marker.
        sheet.mark_in_progress();
        // Bounded, non-blocking: the reader defers instead of waiting on the
        // paused writer, and never observes a partial tuple.
        assert_eq!(
            sheet.read(),
            None,
            "defers (bounded) while the writer is paused"
        );
        assert_eq!(sheet.read(), None, "defers again, never blocks or spins");

        // Writer writes all fields but still has not released EVEN.
        sheet.write_fields(b);
        assert_eq!(
            sheet.read(),
            None,
            "still in progress before the EVEN release"
        );

        // Writer releases EVEN: a later read now returns the complete new tuple.
        sheet.finish_in_progress();
        assert_eq!(sheet.read(), Some(b));
        assert_eq!(sheet.generation.load(Ordering::Relaxed) & 1, 0, "even");
    }

    #[cfg(feature = "qemu-diagnostics")]
    #[test]
    fn submit_hold_does_not_shield_its_own_data_deadline() {
        // A1 / Finding 4: a diagnostic submit hold with a lease longer than the
        // 1 s data deadline must NOT shield the held stage's data deadline — the
        // held submit still times out (cancel + flush + no quarantine) as soon as
        // the deadline elapses, before the lease expires.
        let t0 = 1_000_000_000_000u64;
        let (mutex, ds, _rec, _ctl) = leaked_service_datastage();
        ds.queued_present.store(true, Ordering::Relaxed);
        ds.last_accepted.store(3, Ordering::Relaxed);
        ds.slot_pending.store(true, Ordering::Relaxed);
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(t0);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);
        let diag_clock = crate::diag::DiagTestClock::new();
        diag_clock.store(t0);
        mutex.lock().attach_test_clock(diag_clock);
        // A >1 s lease: the data deadline (1 s) must fire before this lease.
        mutex
            .lock()
            .diag_control(crate::diag::OP_HOLD_TX_SUBMIT, 1500, t0)
            .unwrap();

        // poll1: submit held, deadline armed, still sleeping on the lease.
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(fut.data_deadlines.submit, Some(t0 + 1_000_000_000));
        assert_eq!(ds.cancel_queued_calls.load(Ordering::Relaxed), 0);

        // poll2: 1 s of the data deadline elapses while the 1.5 s lease has not.
        // The held submit times out (data deadline fires during the hold).
        clock.store(t0 + 1_000_000_000);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(
            ds.cancel_queued_calls.load(Ordering::Relaxed),
            1,
            "the held submit data-deadline fires, cancelling the queued ticket"
        );
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Active);
        assert_eq!(mutex.lock().diag_hold_mode(), crate::diag::HOLD_SUBMIT);
    }

    #[cfg(feature = "qemu-diagnostics")]
    #[test]
    fn reclaim_hold_does_not_shield_its_own_data_deadline() {
        // A3 / Finding 4: a diagnostic reclaim hold with a lease longer than the
        // 1 s data deadline must NOT shield the held reclaim stage — the held
        // reclaim reads as a stall (`reclaimed == 0`) and times out into resident
        // recovery before the lease expires.
        let t0 = 1_000_000_000_000u64;
        let (mutex, ds, _rec, ctl) = leaked_service_datastage();
        ds.device_owned.store(3, Ordering::Relaxed);
        ctl.tx_completion_visible.store(true, Ordering::Relaxed);
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(t0);
        let (_lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);
        let diag_clock = crate::diag::DiagTestClock::new();
        diag_clock.store(t0);
        mutex.lock().attach_test_clock(diag_clock);
        mutex
            .lock()
            .diag_control(crate::diag::OP_HOLD_TX_RECLAIM, 1500, t0)
            .unwrap();

        // poll1: reclaim held and stalled; deadline armed, sleeping on the lease.
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(fut.data_deadlines.reclaim, Some(t0 + 1_000_000_000));

        // poll2: the reclaim data deadline elapses during the hold -> recovery.
        clock.store(t0 + 1_000_000_000);
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert!(
            fut.recovery.is_some(),
            "the held reclaim data-deadline fires into resident recovery"
        );
        assert_eq!(
            fut.telemetry.recover_origin_stage.load(Ordering::Relaxed),
            recover_stage::RECLAIM
        );
        assert_eq!(mutex.lock().diag_hold_mode(), crate::diag::HOLD_RECLAIM);
    }

    #[test]
    fn coherent_fault_sheet_reads_only_whole_identities() {
        // A4 / D5: the coherent fault sheet must publish each identity in one
        // atomic publication and never return a tuple mixing two faults. On
        // the single-hart scope the "concurrent" reader is an alternating
        // interleaved reader, so rapid alternation must always observe exactly
        // one whole identity (old or new), never a mixture of fields.
        let sheet = CoherentFaultSheet::new();
        let a = RecoveryFaultIdentity {
            stage: recover_stage::SUBMIT_WAIT,
            local_cause: fault_cause::TIMEOUT,
            queue_epoch: 10,
            available: 100,
            device_owned: 5,
            quarantined: 0,
        };
        let b = RecoveryFaultIdentity {
            stage: recover_stage::OWNERSHIP_DRIFT,
            local_cause: fault_cause::OWNERSHIP_DRIFT,
            queue_epoch: 90,
            available: 0,
            device_owned: 0,
            quarantined: 50,
        };
        assert_eq!(sheet.read(), None, "no fault published yet");
        sheet.publish(a);
        assert_eq!(sheet.read(), Some(a));
        sheet.publish(b);
        assert_eq!(sheet.read(), Some(b));
        // Alternate writers is not possible on single scope; alternate readers
        // interleaved with publishes must still always read a whole identity.
        for _ in 0..1_000 {
            sheet.publish(a);
            let r = sheet.read().unwrap();
            assert!(r == a || r == b);
            sheet.publish(b);
            let r = sheet.read().unwrap();
            assert!(r == a || r == b);
        }
    }

    #[test]
    fn coherent_fault_sheet_publication_uses_fixed_seqcst_protocol() {
        // A4 / 2.2-R1 source guard: the coherent publication and read protocol
        // is fixed to one total order. Every atomic operation inside the
        // `CoherentFaultSheet` impl must use `Ordering::SeqCst`; any weaker
        // ordering reopens the weak-memory hole that three Cycle-000
        // implementations failed to close. Only the impl block is extracted,
        // so unrelated Relaxed telemetry elsewhere in this file stays out of
        // scope.
        let source = include_str!("async_rx.rs");
        let impl_start = source
            .find("impl CoherentFaultSheet")
            .expect("CoherentFaultSheet impl must stay in async_rx.rs");
        let body = &source[impl_start..];
        let mut depth = 0usize;
        let mut end = None;
        for (idx, ch) in body.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(idx + 1);
                        break;
                    }
                }
                _ => {}
            }
        }
        let impl_body = &body[..end.expect("impl CoherentFaultSheet must close")];
        for forbidden in [
            "Ordering::Relaxed",
            "Ordering::Acquire",
            "Ordering::Release",
        ] {
            assert!(
                !impl_body.contains(forbidden),
                "CoherentFaultSheet publication/read must use only Ordering::SeqCst; found \
                 {forbidden}"
            );
        }
        assert!(
            impl_body.contains("Ordering::SeqCst"),
            "CoherentFaultSheet publication/read must use Ordering::SeqCst"
        );
    }

    #[test]
    fn ownership_drift_publishes_coherent_fault_identity() {
        // A4: the drift path must freeze the whole stage/cause/epoch/owner
        // identity as one coherent value (not just the legacy per-field atomics).
        let (mutex, stats) = leaked_service_recovering();
        stats.drift_pending.store(true, Ordering::Relaxed);
        stats.owner_available.store(10, Ordering::Relaxed);
        stats.owner_device_owned.store(3, Ordering::Relaxed);
        stats.owner_quarantined.store(5, Ordering::Relaxed);
        let clock = crate::recovery::RecoveryTestClock::new();
        clock.store(0);
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        fut.recovery_test_clock = Some(clock);

        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        assert_eq!(
            fut.telemetry.coherent_fault.read(),
            Some(RecoveryFaultIdentity {
                stage: recover_stage::OWNERSHIP_DRIFT,
                local_cause: fault_cause::OWNERSHIP_DRIFT,
                queue_epoch: 0,
                available: 10,
                device_owned: 3,
                quarantined: 5,
            })
        );
    }

    #[cfg(feature = "qemu-diagnostics")]
    #[test]
    fn explicit_request_is_absorbed_when_natural_recovery_wins() {
        let lifecycle = drive_to(RxTaskLifecycle::Active);
        let mut request = RecoveryRequestState::new();
        request.request(lifecycle.load()).unwrap();

        // This models the natural-fault linearization under the same request
        // gate: it clears the accepted request before changing lifecycle, so
        // no request can survive and reset the later Active generation.
        request.clear_for_recovery();
        lifecycle.begin_recovery().unwrap();
        assert!(!request.claim(lifecycle.load()));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Quiescing);
    }

    #[cfg(feature = "qemu-diagnostics")]
    #[test]
    fn request_claim_rejects_duplicate_until_recovery_linearizes() {
        let lifecycle = drive_to(RxTaskLifecycle::Active);
        let mut request = RecoveryRequestState::new();
        request.request(lifecycle.load()).unwrap();
        assert!(matches!(
            request.request(lifecycle.load()),
            Err(DevError::ResourceBusy)
        ));
        assert!(request.claim(lifecycle.load()));
        assert!(matches!(
            request.request(lifecycle.load()),
            Err(DevError::ResourceBusy)
        ));
        request.clear_for_recovery();
        lifecycle.begin_recovery().unwrap();
        assert!(matches!(
            request.request(lifecycle.load()),
            Err(DevError::BadState)
        ));
    }

    #[cfg(feature = "qemu-diagnostics")]
    #[test]
    fn terminal_transition_holds_request_gate_through_lifecycle_commit() {
        let lifecycle = drive_to(RxTaskLifecycle::Active);
        let request = spin::Mutex::new(RecoveryRequestState::new());
        request.lock().request(RxTaskLifecycle::Active).unwrap();

        let committed = with_recovery_request_transition(&request, || {
            assert!(
                request.try_lock().is_none(),
                "request gate reopened before lifecycle commit"
            );
            lifecycle.fatal()
        });

        assert!(committed.is_ok());
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        let mut request = request.lock();
        assert!(!request.pending && !request.owner_claimed);
        assert!(matches!(
            request.request(lifecycle.load()),
            Err(DevError::BadState)
        ));
    }

    #[cfg(feature = "qemu-diagnostics")]
    #[test]
    fn pending_request_absorbed_when_drift_quarantines_owner() {
        // A1 rework: a checked explicit recovery request left pending when the
        // owner leaves Active via the ownership-drift quarantine must be
        // absorbed on the SAME seam that commits `Active -> Faulted`, so it
        // cannot survive into a later Active generation and trigger a second
        // reset. This is the real owner transition seam (drift_pending device),
        // not a standalone `RecoveryRequestState` unit check.
        let _test_guard = RECOVERY_REQUEST_TEST_LOCK.lock().unwrap();
        RECOVERY_RESET_REQUEST.lock().clear_for_recovery();
        let (mutex, stats) = leaked_service_recovering();
        stats.drift_pending.store(true, Ordering::Relaxed);
        RECOVERY_RESET_REQUEST
            .lock()
            .request(RxTaskLifecycle::Active)
            .unwrap();
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Pending
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        let mut req = RECOVERY_RESET_REQUEST.lock();
        assert!(
            !req.pending && !req.owner_claimed,
            "explicit request must not survive Active->Faulted drift quarantine"
        );
        assert!(
            req.request(RxTaskLifecycle::Active).is_ok(),
            "accepted request slot was not freed"
        );
        req.clear_for_recovery();
    }

    #[cfg(feature = "qemu-diagnostics")]
    #[test]
    fn pending_request_absorbed_when_owner_ends_in_faulted_terminal() {
        // A1 rework: the `Active -> Faulted` non-recovery terminal path
        // (`publish_fatal`/`transition_fatal`) must clear a pending explicit
        // request at the same seam that commits the transition, so the accepted
        // request cannot survive into a later generation. This drives the real
        // `Fault` round outcome (arm error on a satisfying-service device).
        let _test_guard = RECOVERY_REQUEST_TEST_LOCK.lock().unwrap();
        RECOVERY_RESET_REQUEST.lock().clear_for_recovery();
        let (mutex, _, control) = leaked_service(vec![RxStep::Empty], true);
        control.arm_error.store(true, Ordering::Relaxed);
        RECOVERY_RESET_REQUEST
            .lock()
            .request(RxTaskLifecycle::Active)
            .unwrap();
        let (lifecycle, mut fut) = leaked_future(mutex, Box::leak(Box::new(QueueEvent::new())));
        assert!(matches!(
            poll_once(&mut fut, Arc::new(AtomicUsize::new(0))),
            Poll::Ready(())
        ));
        assert_eq!(lifecycle.load(), RxTaskLifecycle::Faulted);
        let mut req = RECOVERY_RESET_REQUEST.lock();
        assert!(
            !req.pending && !req.owner_claimed,
            "explicit request must not survive Active->Faulted terminal path"
        );
        assert!(
            req.request(RxTaskLifecycle::Active).is_ok(),
            "accepted request slot was not freed"
        );
        req.clear_for_recovery();
    }

    #[cfg(feature = "qemu-diagnostics")]
    #[test]
    fn v4_injected_seam_reads_current_and_fault_tuples_separately() {
        // A2 rework: the V4 current tuple must be assembled by the injectable
        // seam exactly as the one-guard Service read (queue/socket/link/owner),
        // and the historical coherent fault must be a separate, unchanged tuple
        // even when it disagrees with the current ledger.
        let (mutex, stats) = leaked_service_link();
        let src_current_available = stats.owner_available.load(Ordering::Relaxed);
        let historical = RecoveryFaultIdentity {
            stage: recover_stage::OWNERSHIP_DRIFT,
            local_cause: fault_cause::OWNERSHIP_DRIFT,
            queue_epoch: 0,
            available: src_current_available.wrapping_add(1),
            device_owned: 7,
            quarantined: 13,
        };
        // Publish the historical fault first (overwriting any residue from a
        // sibling test), then read the snapshot once: current comes from the
        // injected one-guard Service read, fault from the separate coherent sheet.
        // `RX_TELEMETRY.coherent_fault` is process-global: serialize the V4
        // fault tests so parallel runs do not race it.
        let _test_guard = RECOVERY_REQUEST_TEST_LOCK.lock().unwrap();
        RX_TELEMETRY.coherent_fault.publish(historical);
        let v4 = recovery_snapshot_v4_from(ServiceAccess::Injected(mutex));
        assert_eq!(v4.current_valid, 1, "present Service must be current-valid");
        {
            let mut s = mutex.lock();
            let owner = s.recovery_owner_summary_target();
            assert_eq!(v4.current_queue_epoch, s.queue_epoch_target().current());
            assert_eq!(v4.current_socket_epoch, s.socket_epoch());
            assert_eq!(v4.current_link_generation, s.link_generation());
            assert_eq!(v4.current_link_state, s.link_state_code());
            assert_eq!(v4.current_owner_available, owner.available);
            assert_eq!(v4.current_owner_device_owned, owner.device_owned);
            assert_eq!(v4.current_owner_quarantined, owner.quarantined);
        }
        assert_eq!(v4.fault_valid, 1);
        assert_eq!(v4.fault_stage, historical.stage);
        assert_eq!(v4.fault_cause, historical.local_cause);
        assert_eq!(v4.fault_queue_epoch, historical.queue_epoch);
        assert_eq!(v4.fault_owner_available, historical.available);
        assert_eq!(v4.fault_owner_device_owned, historical.device_owned);
        assert_eq!(v4.fault_owner_quarantined, historical.quarantined);
    }

    #[cfg(feature = "qemu-diagnostics")]
    #[test]
    fn v4_fault_epoch_zero_is_a_valid_historical_fault() {
        // A2 rework: QueueEpoch 0 is a legitimate epoch; a fault at epoch 0 must
        // stay `fault_valid = 1` with `fault_queue_epoch == 0` (no validity bit
        // abuse of a zero sentinel). `coherent_fault` is process-global: serialize.
        let _test_guard = RECOVERY_REQUEST_TEST_LOCK.lock().unwrap();
        let (mutex, ..) = leaked_service_link();
        RX_TELEMETRY.coherent_fault.publish(RecoveryFaultIdentity {
            stage: recover_stage::RESET,
            local_cause: fault_cause::TIMEOUT,
            queue_epoch: 0,
            available: 2,
            device_owned: 0,
            quarantined: 9,
        });
        let v4 = recovery_snapshot_v4_from(ServiceAccess::Injected(mutex));
        assert_eq!(v4.fault_valid, 1, "epoch 0 must not be read as no-fault");
        assert_eq!(v4.fault_queue_epoch, 0);
        assert_eq!(v4.fault_stage, recover_stage::RESET);
    }

    #[cfg(feature = "qemu-diagnostics")]
    #[test]
    fn v4_missing_service_is_current_invalid_without_forged_values() {
        // A2 rework: a missing Service (`ServiceAccess::Global` before install)
        // must publish `current_valid = 0` and no forged healthy epoch/link/owner
        // value, rather than pretending an empty tuple is a healthy observation.
        assert!(
            crate::SERVICE.get().is_none(),
            "these host tests never install the global Service"
        );
        // `coherent_fault` is process-global and read by the shared seam: serialize.
        let _test_guard = RECOVERY_REQUEST_TEST_LOCK.lock().unwrap();
        let v4 = recovery_snapshot_v4_from(ServiceAccess::Global);
        assert_eq!(v4.current_valid, 0);
        assert_eq!(v4.current_queue_epoch, 0);
        assert_eq!(v4.current_socket_epoch, 0);
        assert_eq!(v4.current_link_generation, 0);
        assert_eq!(v4.current_owner_available, 0);
        assert_eq!(v4.current_owner_device_owned, 0);
        assert_eq!(v4.current_owner_quarantined, 0);
    }
}
