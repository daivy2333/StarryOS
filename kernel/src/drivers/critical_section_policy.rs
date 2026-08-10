//! Pure restore-policy seam for the shared kernel critical-section
//! implementation.
//!
//! Contains zero platform calls, zero atomics, zero dependencies. Compiles
//! as both no_std (kernel target) and std (host tests via `#[path]`
//! include from `tests/ms04-async-rx-host-harness.rs`).
//!
//! ## Semantics
//!
//! The kernel critical section disables IRQs on acquire and must restore
//! the *prior* IRQ enable state on release, so that an `AtomicWaker::wake()`
//! inside an ISR (already inside a critical section, IRQs disabled) does not
//! re-enable IRQs before the platform completes the interrupt.
//!
//! `IrqRestorePolicy` simulates the global IRQ enable state together with a
//! per-context stack of restore states:
//!
//! - `acquire()` reads the current simulated state, pushes it, and disables
//!   IRQs in the simulation when they were enabled.
//! - `release()` pops the matching state and re-enables IRQs in the
//!   simulation only when the popped state was enabled.
//!
//! Nesting is handled naturally: a nested acquire observes IRQs already
//! disabled, pushes `false`, and its release never re-enables. Only the
//! outermost release from an enabled state restores IRQs. An ISR context
//! starts from `new_with_irqs_disabled()` so its releases never re-enable.

/// Per-context (per-CPU) restore policy for the shared critical section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IrqRestorePolicy {
    /// Simulated global IRQ enable state.
    irqs_enabled: bool,
    /// Stack of `was_enabled` flags pushed by matching acquires.
    stack: [bool; 4],
    depth: u8,
}

impl Default for IrqRestorePolicy {
    fn default() -> Self {
        Self::new()
    }
}

impl IrqRestorePolicy {
    /// Creates an empty policy with IRQs enabled.
    pub const fn new() -> Self {
        Self {
            irqs_enabled: true,
            stack: [false; 4],
            depth: 0,
        }
    }

    /// Creates an empty policy with IRQs disabled (ISR context).
    pub const fn new_with_irqs_disabled() -> Self {
        Self {
            irqs_enabled: false,
            stack: [false; 4],
            depth: 0,
        }
    }

    /// Returns the simulated IRQ enable state.
    pub const fn irqs_enabled(&self) -> bool {
        self.irqs_enabled
    }

    /// Acquires the critical section.
    ///
    /// Returns `true` when IRQs were enabled and must be disabled by the
    /// caller.
    pub fn acquire(&mut self) -> bool {
        assert!(self.depth < self.stack.len() as u8, "policy stack overflow");
        let was_enabled = self.irqs_enabled;
        self.stack[self.depth as usize] = was_enabled;
        self.depth += 1;
        if was_enabled {
            self.irqs_enabled = false;
        }
        was_enabled
    }

    /// Releases the critical section.
    ///
    /// Returns `true` when IRQs must be re-enabled by the caller.
    pub fn release(&mut self) -> bool {
        assert!(self.depth > 0, "policy underflow");
        self.depth -= 1;
        let was_enabled = self.stack[self.depth as usize];
        if was_enabled {
            self.irqs_enabled = true;
        }
        was_enabled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_from_enabled_disables() {
        let mut p = IrqRestorePolicy::new();
        assert!(p.acquire());
        assert!(!p.irqs_enabled());
    }

    #[test]
    fn nested_acquire_from_disabled_stays_disabled() {
        let mut p = IrqRestorePolicy::new();
        assert!(p.acquire());
        assert!(!p.acquire());
        assert!(!p.irqs_enabled());
    }

    #[test]
    fn inner_release_keeps_disabled_outer_reenables() {
        let mut p = IrqRestorePolicy::new();
        p.acquire();
        p.acquire();
        assert!(!p.release());
        assert!(!p.irqs_enabled());
        assert!(p.release());
        assert!(p.irqs_enabled());
    }

    #[test]
    fn isr_wake_context_never_reenables() {
        let mut p = IrqRestorePolicy::new_with_irqs_disabled();
        assert!(!p.acquire());
        assert!(!p.release());
        assert!(!p.irqs_enabled());
    }

    #[test]
    fn single_release_restores_enabled() {
        let mut p = IrqRestorePolicy::new();
        p.acquire();
        assert!(p.release());
        assert!(p.irqs_enabled());
    }

    #[test]
    fn many_nested_levels_track_depth() {
        let mut p = IrqRestorePolicy::new();
        assert!(p.acquire());
        assert!(!p.acquire());
        assert!(!p.acquire());
        assert!(!p.irqs_enabled());
        assert!(!p.release());
        assert!(!p.release());
        assert!(p.release());
        assert!(p.irqs_enabled());
    }
}
