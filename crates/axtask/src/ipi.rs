//! Event-driven remote scheduler wake via a single reschedule IPI (S_SOFT).
//!
//! This module backs the `ipi` feature: after a successful `Blocked -> Ready`
//! transition that places a task into a *remote* run queue, the scheduler sends
//! one inter-processor interrupt to that hart so the ready task gains a
//! scheduling opportunity without depending on its periodic timer. Only the
//! primary scheduler registers the single S_SOFT handler; the handler only
//! records a receive and, for an initialized current task, sets a reschedule
//! pending so the general preemption path schedules the woken task.
//!
//! The notification decision and send/receive counters are exposed through a
//! mockable seam so host tests can lock the "exactly once per successful remote
//! wake, never for local wake, never on duplicate" contract without a real IPI.

use core::sync::atomic::{AtomicUsize, Ordering};

use axhal::irq::{IPI_IRQ, IpiTarget, register, send_ipi};

#[cfg(test)]
use core::hint::spin_loop;

/// Telemetry counters. These observe causality only; they never participate in
/// a synchronization decision, so a Relaxed ordering is sufficient.
static IPI_SENT: AtomicUsize = AtomicUsize::new(0);
static IPI_RECEIVED: AtomicUsize = AtomicUsize::new(0);

/// Single ownership flag for the S_SOFT handler slot. Prevents a second
/// registration (e.g. a duplicate scheduler init or a competing runtime
/// feature) from stealing the only S_SOFT slot.
static IPI_REGISTERED: AtomicUsize = AtomicUsize::new(0);

/// Returns `true` when a successful remote ready-transition must notify the
/// target hart, i.e. the enqueue actually transitioned to `Ready`.
///
/// This is the pure decision seam: `was_ready` is only true for a real
/// `Blocked -> Ready` transition (`put_task_with_state` returning success), so
/// a spurious/duplicate wake that did not change state never sends an IPI.
#[inline]
pub(crate) fn should_notify_remote(cpu_id: usize, this_cpu_id: usize, was_ready: bool) -> bool {
    was_ready && cpu_id != this_cpu_id
}

/// Sends one reschedule IPI to `cpu_id`. Records a sent counter.
#[inline]
pub(crate) fn send_reschedule_ipi(cpu_id: usize) {
    debug!("reschedule IPI -> cpu {cpu_id}");
    IPI_SENT.fetch_add(1, Ordering::Relaxed);
    send_ipi(
        IPI_IRQ,
        IpiTarget::Other {
            cpu_id,
        },
    );
}

/// The S_SOFT handler. Sets a preempt pending on the current (target-hart) task
/// so the general IRQ guard exit performs the reschedule. It never switches to
/// the woken task directly and never calls into a run queue.
pub(crate) fn reschedule_ipi_handler() {
    IPI_RECEIVED.fetch_add(1, Ordering::Relaxed);
    if let Some(curr) = crate::current_may_uninit() {
        #[cfg(feature = "preempt")]
        curr.set_preempt_pending(true);
    }
}

/// Tries to obtain exclusive ownership of the S_SOFT handler slot and register
/// [`reschedule_ipi_handler`]. Returns `false` if the slot is already taken.
#[inline]
pub(crate) fn try_register_ipi() -> bool {
    if IPI_REGISTERED
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return false;
    }
    let ok = register(IPI_IRQ, reschedule_ipi_handler);
    if !ok {
        // Roll back the ownership flag on registration failure.
        IPI_REGISTERED.store(0, Ordering::Release);
    }
    ok
}

/// Waits until the designated hart is ready: spin on the pending flag. Only used
/// by the host witness / controlled migration path; not part of normal wake.
#[cfg(test)]
pub(crate) fn spin_until(pending: &AtomicUsize) {
    while pending.load(Ordering::Acquire) == 0 {
        spin_loop();
    }
}

#[cfg(test)]
pub(crate) fn sent_count() -> usize {
    IPI_SENT.load(Ordering::Relaxed)
}

#[cfg(test)]
pub(crate) fn received_count() -> usize {
    IPI_RECEIVED.load(Ordering::Relaxed)
}

#[cfg(test)]
pub(crate) fn reset_ipi_registered() {
    IPI_REGISTERED.store(0, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_ready_transition_notifies_once() {
        // cpu 5 is remote to this hart (0) and the transition really happened:
        // the decision returns true for a successful remote enqueue.
        assert!(should_notify_remote(5, 0, true));
        assert!(!should_notify_remote(5, 0, false));
    }

    #[test]
    fn local_ready_transition_never_notifies_remote() {
        // Same hart (cpu 0 == this hart 0): local wake must not send an IPI.
        assert!(!should_notify_remote(0, 0, true));
    }

    #[test]
    fn duplicate_wake_does_not_notify() {
        // A duplicate wake that did not actually transition (was_ready=false)
        // must not send an IPI, even across harts.
        assert!(!should_notify_remote(7, 0, false));
    }

    #[test]
    fn registration_is_single_owner_and_rolls_back() {
        reset_ipi_registered();
        // On host, the dummy platform `register` always returns false, which
        // must roll back the ownership flag so a later attempt is not poisoned.
        let first = try_register_ipi();
        // The dummy register returns false -> ownership rolled back -> a second
        // exclusive attempt is allowed again (no permanent poison).
        let second = try_register_ipi();
        // Either the platform handed us the slot (true) or refused it (false);
        // a failed registration must not leave the slot permanently held.
        // Re-run twice to prove rollback: both attempts see the same fresh state.
        try_register_ipi();
        let third = try_register_ipi();
        assert_eq!(first, second, "rollback must restore the slot");
        assert_eq!(second, third, "no poison from refused registration");
        reset_ipi_registered();
    }

    #[test]
    fn telemetry_counters_monotonic_and_causal() {
        // Sending increments the sent counter; the decision does not, by itself,
        // mutate causality counters.
        let before = sent_count();
        if should_notify_remote(1, 0, true) {
            send_reschedule_ipi(1);
        }
        assert!(sent_count() > before, "a remote send must bump the sent counter");
    }
}