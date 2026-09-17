//! SMP-safe restore-policy seam for the shared kernel critical-section.
//!
//! Kept dependency-free (no `axhal`, no `critical_section`) so the same file
//! compiles both as the kernel module `crate::critical_section_policy` and,
//! via `#[path]` include, inside `tests/ms04-async-rx-host-harness.rs`.
//! Only `core::sync::atomic` (part of `core`) and `core::hint::spin_loop` are
//! used; the harness injects a fake `IrqOps` backend, the kernel uses an
//! `axhal` backend, and both execute the same two functions.
//!
//! ## Semantics
//!
//! The kernel critical section must serialize concurrent access to shared
//! async state (Embassy `AtomicWaker`) across *all* online harts, while still
//! restoring the *prior* IRQ enable state on release so that an ISR wake path
//! never re-enables IRQs before the platform completes the interrupt.
//!
//! `acquire` saves and disables the local IRQ state, then acquires a global
//! owner lock once when the current hart's nesting depth transitions 0 → 1.
//! Same-hart nesting only increments that hart's depth. `release` decrements
//! the depth and, when it transitions 1 → 0, releases the global owner lock
//! and re-enables IRQs only if the matching `acquire` entered from an enabled
//! state. A nested or ISR `release(false)` never re-enables IRQs.
//!
//! Out-of-range hart id, depth underflow/overflow and releasing a depth of
//! zero (never acquired) fail closed rather than corrupting another hart's
//! ownership.

use core::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// Upper bound on the number of hart ids the policy can index.
///
/// The kernel platform caps its own CPU count below this bound (QEMU `SMP=16`),
/// and the host harness uses a small fixed set, so 64 is a safe capacity that
/// never depends on a platform crate. A hart id at or above this bound is a
/// policy violation and halts.
pub const MAX_CPU_NUM: usize = 64;

/// Global cross-hart owner lock. `true` while some hart holds the critical
/// section. Set with Acquire on outer acquire / Release on outer release.
static GLOBAL_LOCK: AtomicBool = AtomicBool::new(false);

/// Per-hart nesting depth. 0 means this hart does not hold the critical
/// section. Synchronized with AcqRel so the depth and the owner lock together
/// gate the protected state.
static NEST_DEPTH: [AtomicU32; MAX_CPU_NUM] =
    [const { AtomicU32::new(0) }; MAX_CPU_NUM];

/// IRQ primitives and current-hart identity the restore policy needs.
pub trait IrqOps {
    /// Returns whether IRQs are currently enabled.
    fn irqs_enabled(&self) -> bool;
    /// Disables IRQs.
    fn disable_irqs(&self);
    /// Enables IRQs.
    fn enable_irqs(&self);
    /// Returns this hart's id, used to index per-hart state.
    fn current_cpu_id(&self) -> usize;
}

fn depth_mut(cpu: usize) -> &'static AtomicU32 {
    if cpu < MAX_CPU_NUM {
        &NEST_DEPTH[cpu]
    } else {
        // Out-of-range hart id: fail closed rather than index UB.
        panic!("critical-section: hart id {cpu} exceeds MAX_CPU_NUM");
    }
}

/// Pure checked transition for the nesting-depth counter.
///
/// Rejects the next nesting level once the counter would overflow the real
/// `u32` width, never wrapping. `acquire` hydrates this seam with the actual
/// counter value via `fetch_update`; tests exercise it directly at the `u32`
/// boundary so the overflow path is witnessed without driving a live counter to
/// `u32::MAX`.
#[inline]
pub(crate) fn checked_increment_depth(depth: u32) -> Option<u32> {
    depth.checked_add(1)
}

/// Acquires the critical section.
///
/// Always disables IRQs before returning. Returns `true` (the restore state)
/// when IRQs were enabled on entry, i.e. the matching `release` must re-enable
/// them; returns `false` when they were already disabled (nested or ISR
/// context).
#[inline]
pub fn acquire<O: IrqOps + ?Sized>(ops: &O) -> bool {
    let was_enabled = ops.irqs_enabled();
    ops.disable_irqs();
    let cpu = ops.current_cpu_id();
    // Checked increment: a nesting depth at the true `u32` counter ceiling must
    // fail closed (panic) instead of wrapping. `fetch_update` with `None` leaves
    // the counter untouched, so neither depth nor the global owner is corrupted,
    // and IRQs remain disabled on this hart. The transition is delegated to the
    // pure `checked_increment_depth` seam so its overflow boundary is directly
    // host-testable at the real width without driving a live counter to `MAX`.
    let prev = depth_mut(cpu).fetch_update(
        Ordering::AcqRel,
        Ordering::Acquire,
        checked_increment_depth,
    ).unwrap_or_else(|depth| {
        core::panic!("critical-section: nesting depth {depth} would overflow on cpu {cpu}")
    });
    if prev == 0 {
        // This hart is entering the outermost level: acquire global ownership.
        while GLOBAL_LOCK
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
    }
    was_enabled
}

/// Releases the critical section.
///
/// Re-enables IRQs exactly once only when the matching `acquire` entered from
/// an enabled state. A `release(false)` never enables IRQs, so nested sections
/// and ISR wake paths cannot re-enable IRQs prematurely. Releasing a depth of
/// zero (an outer release without a matching acquire on this hart) fails closed.
#[inline]
pub fn release<O: IrqOps + ?Sized>(ops: &O, was_enabled: bool) {
    let prev = depth_mut(ops.current_cpu_id()).fetch_update(
        Ordering::AcqRel,
        Ordering::Acquire,
        |d| Some(d.checked_sub(1).expect("critical-section: depth underflow")),
    );
    if let Ok(prev) = prev {
        if prev == 1 {
            // Leaving the outermost level: release global ownership.
            GLOBAL_LOCK.store(false, Ordering::Release);
        }
    }
    if was_enabled {
        ops.enable_irqs();
    }
}