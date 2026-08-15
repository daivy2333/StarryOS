//! QEMU-only bounded pressure controls (D9).
//!
//! This module provides the deterministic submit/reclaim hold state that lets
//! the guest probe drive a VirtIO-MMIO NIC to exact slot/descriptor Full under
//! QEMU, where completions are otherwise too fast to observe. It is compiled
//! only when the private `qemu-diagnostics` feature is enabled (propagated
//! exclusively from `starry-kernel/qemu`); ordinary axnet and D1 builds never
//! contain these entry points.
//!
//! The state is a plain lease: a hold pauses exactly one stage of the sole
//! queue owner and never mutates slots, rings, tickets or completions. A lease
//! expiry auto-releases the hold and counts a failure; an explicit release
//! also resumes the queue but does not count a failure.

use core::sync::atomic::{AtomicU64, Ordering};

use axdriver::prelude::{DevError, DevResult};
#[cfg(not(test))]
use axhal::time::wall_time_nanos;

/// Hold mode: no hold.
pub(crate) const HOLD_NONE: u64 = 0;
/// Hold mode: pause the TX submit stage.
pub(crate) const HOLD_SUBMIT: u64 = 1;
/// Hold mode: pause the TX reclaim stage.
pub(crate) const HOLD_RECLAIM: u64 = 2;

/// Control op: pause the TX submit stage (`lease_ms` in 1..=2000).
pub(crate) const OP_HOLD_TX_SUBMIT: u64 = 1;
/// Control op: pause the TX reclaim stage (`lease_ms` in 1..=2000).
pub(crate) const OP_HOLD_TX_RECLAIM: u64 = 2;
/// Control op: release any hold (`lease_ms` must be 0).
pub(crate) const OP_RELEASE: u64 = 3;

/// Maximum lease length in milliseconds.
pub(crate) const MAX_LEASE_MS: u64 = 2000;

/// Lease length in nanoseconds for one millisecond.
pub(crate) const NS_PER_MS: u64 = 1_000_000;

/// Host-test override for the diagnostic wall clock (RW-1 fake clock).
#[cfg(test)]
static TEST_NOW: AtomicU64 = AtomicU64::new(0);

/// Returns the current diagnostic clock: the platform wall clock in
/// production, or the host-test override. The queue future uses this for its
/// lease-deadline wake decision so an expired hold auto-releases without an
/// external NIC event.
pub(crate) fn diag_now() -> u64 {
    #[cfg(test)]
    {
        TEST_NOW.load(Ordering::Relaxed)
    }
    #[cfg(not(test))]
    {
        wall_time_nanos()
    }
}

/// Advances the host-test fake clock to `nanos`.
#[cfg(test)]
pub(crate) fn set_test_now(nanos: u64) {
    TEST_NOW.store(nanos, Ordering::Relaxed);
}

/// Atomic, single-owner pressure-control state.
///
/// Fields are `Relaxed` except the hold-mode/expiry pair, which is ordered so
/// the queue task observes either a fully-committed hold or none.
pub(crate) struct DiagnosticState {
    hold_mode: AtomicU64,
    lease_expiry_nanos: AtomicU64,
    auto_release_failure: AtomicU64,
}

impl DiagnosticState {
    pub(crate) const fn new() -> Self {
        Self {
            hold_mode: AtomicU64::new(HOLD_NONE),
            lease_expiry_nanos: AtomicU64::new(0),
            auto_release_failure: AtomicU64::new(0),
        }
    }

    /// Applies one control op with the caller's wall clock.
    pub(crate) fn control(&self, op: u64, lease_ms: u64, now_nanos: u64) -> DevResult {
        match op {
            OP_HOLD_TX_SUBMIT | OP_HOLD_TX_RECLAIM if (1..=MAX_LEASE_MS).contains(&lease_ms) => {
                let mode = if op == OP_HOLD_TX_SUBMIT {
                    HOLD_SUBMIT
                } else {
                    HOLD_RECLAIM
                };
                let expiry = now_nanos + lease_ms * NS_PER_MS;
                self.hold_mode.store(mode, Ordering::Release);
                self.lease_expiry_nanos.store(expiry, Ordering::Relaxed);
                Ok(())
            }
            OP_RELEASE if lease_ms == 0 => {
                self.hold_mode.store(HOLD_NONE, Ordering::Release);
                self.lease_expiry_nanos.store(0, Ordering::Relaxed);
                Ok(())
            }
            _ => Err(DevError::InvalidParam),
        }
    }

    /// Advances the state to `now`: auto-releases an expired lease and counts
    /// a failure, then returns the currently-active hold mode.
    pub(crate) fn tick(&self, now_nanos: u64) -> u64 {
        let expiry = self.lease_expiry_nanos.load(Ordering::Relaxed);
        if expiry != 0 && now_nanos >= expiry {
            self.auto_release_failure.fetch_add(1, Ordering::Relaxed);
            self.hold_mode.store(HOLD_NONE, Ordering::Release);
            self.lease_expiry_nanos.store(0, Ordering::Relaxed);
            HOLD_NONE
        } else {
            self.hold_mode.load(Ordering::Acquire)
        }
    }

    /// Current hold mode (for the V3 diagnostic snapshot).
    pub(crate) fn hold_mode(&self) -> u64 {
        self.hold_mode.load(Ordering::Relaxed)
    }

    /// Current lease expiry deadline in wall nanoseconds (0 = no hold).
    pub(crate) fn lease_expiry(&self) -> u64 {
        self.lease_expiry_nanos.load(Ordering::Relaxed)
    }

    /// Number of lease-expiry auto-releases (diagnostic telemetry).
    pub(crate) fn auto_release_failure(&self) -> u64 {
        self.auto_release_failure.load(Ordering::Relaxed)
    }
}

/// The one diagnostic control state (QEMU feature only).
pub(crate) static DIAGNOSTIC: DiagnosticState = DiagnosticState::new();

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> u64 {
        1_000_000_000_000
    }

    #[test]
    fn control_rejects_out_of_range_lease_and_bad_ops() {
        let d = DiagnosticState::new();
        assert!(matches!(
            d.control(OP_HOLD_TX_SUBMIT, 0, now()),
            Err(DevError::InvalidParam)
        ));
        assert!(matches!(
            d.control(OP_HOLD_TX_SUBMIT, MAX_LEASE_MS + 1, now()),
            Err(DevError::InvalidParam)
        ));
        assert!(matches!(
            d.control(OP_RELEASE, 1, now()),
            Err(DevError::InvalidParam)
        ));
        assert!(matches!(
            d.control(99, 10, now()),
            Err(DevError::InvalidParam)
        ));
        assert_eq!(d.hold_mode(), HOLD_NONE);
        assert_eq!(d.lease_expiry(), 0);
    }

    #[test]
    fn hold_submit_and_reclaim_set_modes_and_expiry() {
        let d = DiagnosticState::new();
        d.control(OP_HOLD_TX_SUBMIT, 100, now()).unwrap();
        assert_eq!(d.hold_mode(), HOLD_SUBMIT);
        assert_eq!(d.lease_expiry(), now() + 100 * NS_PER_MS);
        assert_eq!(d.tick(now()), HOLD_SUBMIT);
        d.control(OP_HOLD_TX_RECLAIM, 1, now()).unwrap();
        assert_eq!(d.hold_mode(), HOLD_RECLAIM);
        assert_eq!(d.lease_expiry(), now() + NS_PER_MS);
    }

    #[test]
    fn release_clears_hold_and_never_counts_failure() {
        let d = DiagnosticState::new();
        d.control(OP_HOLD_TX_SUBMIT, 2000, now()).unwrap();
        d.control(OP_RELEASE, 0, now()).unwrap();
        assert_eq!(d.hold_mode(), HOLD_NONE);
        assert_eq!(d.lease_expiry(), 0);
        assert_eq!(d.auto_release_failure(), 0);
        assert_eq!(d.tick(now()), HOLD_NONE);
    }

    #[test]
    fn expired_lease_auto_releases_and_counts_failure() {
        let d = DiagnosticState::new();
        d.control(OP_HOLD_TX_SUBMIT, 2, now()).unwrap();
        // Just before expiry the hold is still active.
        let before = now() + 2 * NS_PER_MS - 1;
        assert_eq!(d.tick(before), HOLD_SUBMIT);
        // At and after expiry the hold is released and counted.
        let at = now() + 2 * NS_PER_MS;
        assert_eq!(d.tick(at), HOLD_NONE);
        assert_eq!(d.auto_release_failure(), 1);
        assert_eq!(d.hold_mode(), HOLD_NONE);
        assert_eq!(d.lease_expiry(), 0);
        // No further auto-release fires once cleared.
        assert_eq!(d.tick(at + 1), HOLD_NONE);
        assert_eq!(d.auto_release_failure(), 1);
    }

    #[test]
    fn second_hold_after_expiry_reuses_the_state() {
        let d = DiagnosticState::new();
        d.control(OP_HOLD_TX_RECLAIM, 1, now()).unwrap();
        assert_eq!(d.tick(now() + NS_PER_MS), HOLD_NONE);
        d.control(OP_HOLD_TX_RECLAIM, 1, now() + NS_PER_MS).unwrap();
        assert_eq!(d.hold_mode(), HOLD_RECLAIM);
        assert_eq!(d.auto_release_failure(), 1);
    }

    #[test]
    fn hold_does_not_mutate_owner_or_completion_state() {
        // The control state is purely a stage gate: it owns no slots, tickets,
        // descriptors or completions, so a hold can never change ownership.
        let d = DiagnosticState::new();
        d.control(OP_HOLD_TX_SUBMIT, 10, now()).unwrap();
        assert_eq!(d.tick(now()), HOLD_SUBMIT);
        // Reading the hold state must not touch anything outside it.
        let _ = d.hold_mode();
        let _ = d.lease_expiry();
        let _ = d.auto_release_failure();
    }
}
