// kernel/src/drivers/copier_start_guard.rs

//! Copier start ownership guard (MS17 Iteration 001, task T4).
//!
//! Fail-closed exactly-once semantics for `uart_init::start_copiers`: the
//! first claim wins, a duplicate claim fails without reopening the role,
//! and a failed spawn rolls the role back so a later boot retry can claim
//! it. Host-tested by `tests/k3-uart-plane-host-harness.rs`; the QEMU/D1/K3
//! `start_copiers` paths share this type for the RX/TX start guards.

use core::sync::atomic::{AtomicBool, Ordering};

/// Exactly-once start guard for one copier role.
#[derive(Default)]
pub struct CopierStartGuard(AtomicBool);

impl CopierStartGuard {
    pub const fn new() -> Self {
        Self(AtomicBool::new(false))
    }

    /// Claims the role exactly once. `false` = already claimed (duplicate
    /// start); the guard stays claimed so the duplicate remains visible.
    pub fn claim(&self) -> bool {
        !self.0.swap(true, Ordering::AcqRel)
    }

    /// Rolls a failed startup back to unclaimed (spawn-failure path).
    pub fn rollback(&self) {
        self.0.store(false, Ordering::Release);
    }

    /// Whether the role is currently claimed.
    pub fn claimed(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
