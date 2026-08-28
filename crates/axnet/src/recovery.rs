//! Task 2.2: monotonic clock and per-test fixture clock for the resident
//! device-recovery owner.
//!
//! This module is always compiled, independent of the `qemu-diagnostics`
//! feature, so ordinary axnet and D1 builds carry the recovery deadline
//! clock without coupling to the QEMU diagnostic hold/lease machinery.

#[cfg(test)]
use alloc::boxed::Box;

#[cfg(not(test))]
use axhal::time::wall_time_nanos;

/// Wall-clock source for the queue owner's recovery deadlines (Task 2.2).
pub(crate) fn recovery_now() -> u64 {
    #[cfg(test)]
    {
        RECOVERY_NOW.load(core::sync::atomic::Ordering::Relaxed)
    }
    #[cfg(not(test))]
    {
        wall_time_nanos()
    }
}

/// Per-test-independent recovery clock (Task 2.2). Each fixture owns an
/// independent fake-clock instance so recovery-deadline tests can drive stage
/// timeouts deterministically without sharing process-global state.
#[cfg(test)]
#[derive(Clone, Copy)]
pub(crate) struct RecoveryTestClock {
    now: &'static core::sync::atomic::AtomicU64,
}

#[cfg(test)]
impl RecoveryTestClock {
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

#[cfg(test)]
static RECOVERY_NOW: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
