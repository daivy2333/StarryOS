//! Restore-policy seam for the shared kernel critical-section implementation.
//!
//! Kept dependency-free (no `axhal`, no `critical_section`, no atomics) so the
//! same file compiles both as the kernel module
//! `crate::drivers::critical_section_policy` and, via `#[path]` include, inside
//! `tests/ms04-async-rx-host-harness.rs`.
//!
//! ## Semantics
//!
//! The kernel critical section disables IRQs on acquire and must restore the
//! *prior* IRQ enable state on release, so that an `AtomicWaker::wake()` inside
//! an ISR (already inside a critical section, IRQs disabled) does not
//! re-enable IRQs before the platform completes the interrupt.
//!
//! `acquire`/`release` are the single source of that policy. The kernel glue
//! in `kernel/src/lib.rs::critical_impl` delegates its
//! `critical_section::Impl` methods through an `IrqOps` backend backed by
//! `axhal`; the host harness injects a fake backend that records the simulated
//! IRQ state and call counts. Both execute the same two functions, so the host
//! tests witness the exact production restore decision logic.

/// IRQ primitives the restore policy needs from its environment.
pub trait IrqOps {
    /// Returns whether IRQs are currently enabled.
    fn irqs_enabled(&self) -> bool;
    /// Disables IRQs.
    fn disable_irqs(&self);
    /// Enables IRQs.
    fn enable_irqs(&self);
}

/// Acquires the critical section.
///
/// Always disables IRQs before returning. Returns `true` (the restore state)
/// when IRQs were enabled on entry, i.e. the matching `release` must re-enable
/// them; returns `false` when they were already disabled (ISR context).
#[inline]
pub fn acquire<O: IrqOps + ?Sized>(ops: &O) -> bool {
    let was_enabled = ops.irqs_enabled();
    ops.disable_irqs();
    was_enabled
}

/// Releases the critical section.
///
/// Re-enables IRQs exactly once only when the matching `acquire` entered from
/// an enabled state. A `release(false)` never enables IRQs, so nested sections
/// and ISR wake paths cannot re-enable IRQs prematurely.
#[inline]
pub fn release<O: IrqOps + ?Sized>(ops: &O, was_enabled: bool) {
    if was_enabled {
        ops.enable_irqs();
    }
}
