//! QEMU-only bounded pressure controls (D9): constants and clock.
//!
//! The deterministic submit/reclaim hold state lives in
//! [`crate::service::Service`] under the Service guard: a hold pauses exactly
//! one stage of the sole queue owner and never mutates slots, rings, tickets
//! or completions. A lease expiry auto-releases the hold and counts a failure;
//! an explicit release also resumes the queue but does not count a failure.
//!
//! This module is compiled only when the private `qemu-diagnostics` feature
//! is enabled (propagated exclusively from `starry-kernel/qemu`); ordinary
//! axnet and D1 builds never contain these entry points. It keeps the
//! constants, the wall clock and the host-test fake clock. There is no
//! independent lease generation or global diagnostic state: the lease is a
//! committed part of the Service, so no identity can exhaust and every
//! reachable Hold is releasable.

use alloc::boxed::Box;

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

/// Host-test override for the diagnostic wall clock.
#[cfg(test)]
static TEST_NOW: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Task 5.2 (Iteration 006): per-test fixture clock. Each fixture owns an
/// independent fake-clock instance; `Service::diag_hold_tick` and the RX
/// future's lease deadline read the fixture's clock when attached, so the
/// R57-companion flake (parallel diagnostics tests sharing the process-global
/// `TEST_NOW`) disappears without suite-level serialization.
#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) struct DiagTestClock {
    now: &'static core::sync::atomic::AtomicU64,
}

#[cfg(test)]
impl DiagTestClock {
    /// Leaks a fresh independent clock starting at 0.
    pub(crate) fn new() -> Self {
        Self {
            now: Box::leak(Box::new(core::sync::atomic::AtomicU64::new(0))),
        }
    }

    pub(crate) fn store(&self, nanos: u64) {
        self.now.store(nanos, core::sync::atomic::Ordering::Relaxed);
    }

    pub(crate) fn load(&self) -> u64 {
        self.now.load(core::sync::atomic::Ordering::Relaxed)
    }
}

/// Returns the current diagnostic clock: the platform wall clock in
/// production, or the host-test override. The queue future uses this for its
/// lease-deadline wake decision so an expired hold auto-releases without an
/// external NIC event.
pub(crate) fn diag_now() -> u64 {
    #[cfg(test)]
    {
        TEST_NOW.load(core::sync::atomic::Ordering::Relaxed)
    }
    #[cfg(not(test))]
    {
        wall_time_nanos()
    }
}

/// Advances the host-test fake clock to `nanos`. Intended for fixtures that
/// still drive the raw `diag_now()`; per-test clock users prefer
/// [`DiagTestClock`].
#[cfg(test)]
pub(crate) fn set_test_now(nanos: u64) {
    TEST_NOW.store(nanos, core::sync::atomic::Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_constants_are_stable() {
        assert_eq!(HOLD_NONE, 0);
        assert_eq!(HOLD_SUBMIT, 1);
        assert_eq!(HOLD_RECLAIM, 2);
        assert_eq!(OP_HOLD_TX_SUBMIT, 1);
        assert_eq!(OP_HOLD_TX_RECLAIM, 2);
        assert_eq!(OP_RELEASE, 3);
        assert_eq!(MAX_LEASE_MS, 2000);
        assert_eq!(NS_PER_MS, 1_000_000);
    }
}
