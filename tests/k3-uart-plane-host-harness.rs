//! K3 UART data-plane start/ownership host harness (MS17 Iteration 001,
//! task T4).
//!
//! Uses `#[path]` to include the real production modules without compiling
//! the kernel:
//! - `kernel/src/drivers/copier_start_guard.rs` — the fail-closed
//!   exactly-once start guard that `uart_init::start_copiers` uses for the
//!   RX and TX copier roles (duplicate start must fail closed, a failed
//!   spawn must roll the role back);
//! - `kernel/src/drivers/placement.rs` — the shared background-role policy
//!   whose single-hart degeneration the K3 data-plane relies on (SMP=1
//!   means both copiers co-locate on the one schedulable hart, no special
//!   case).
//!
//! Runs via `rustc --edition=2024 --test` → /tmp binary.
//!
//! RED state: against the pre-task tree the guard module does not exist, so
//! this harness fails to compile.
//! GREEN state: all scenarios pass.

/// Stub for the kernel's `axconfig` crate: same `extern crate self`
/// mechanism as the MS04 harness. The value 16 is the QEMU qualification
/// platform; `placement.rs` must delegate its capacity bound here.
pub mod plat {
    pub const MAX_CPU_NUM: usize = 16;
}
extern crate self as axconfig;

#[path = "../kernel/src/drivers/copier_start_guard.rs"]
mod copier_start_guard;

#[path = "../kernel/src/drivers/placement.rs"]
mod placement;

use copier_start_guard::CopierStartGuard;
use placement::place_roles;

// ---------------------------------------------------------------------------
// CopierStartGuard: fail-closed exactly-once start semantics
// ---------------------------------------------------------------------------

#[test]
fn duplicate_start_claim_fails_closed() {
    let guard = CopierStartGuard::new();
    assert!(guard.claim(), "the first claim must win");
    assert!(guard.claimed(), "the claimed role must be visible");
    assert!(
        !guard.claim(),
        "a duplicate start claim must fail closed before a second copier exists"
    );
    assert!(
        guard.claimed(),
        "a failed duplicate claim must not reset or reopen the role"
    );
}

#[test]
fn failed_spawn_rolls_back_and_reopens_the_role() {
    let guard = CopierStartGuard::new();
    assert!(guard.claim());
    // The spawn-failure path rolls the role back so the boot retry can
    // claim it again (mirrors uart_init::start_copiers' rollback branch).
    guard.rollback();
    assert!(!guard.claimed(), "rollback must clear the claim");
    assert!(guard.claim(), "the role must be claimable again after rollback");
}

// ---------------------------------------------------------------------------
// Single-hart placement degeneration (the K3 SMP=1 data-plane reliance)
// ---------------------------------------------------------------------------

#[test]
fn single_hart_placement_co_locates_all_roles() {
    let p = place_roles(&[3], 3).expect("single schedulable hart is valid");
    assert_eq!(p.uart_rx, 3, "RX copier lands on the only schedulable hart");
    assert_eq!(
        p.uart_tx, 3,
        "single-hart placement must co-locate the TX copier without a special case"
    );
    assert_eq!(p.net_owner, 3);
    assert_eq!(p.net_runner, 3);
}
