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

// MS14 Iteration 001 (T5): the k3-smp-net role-composition seam — the same
// `#[path]` include pattern; the module pulls the real placement policy as
// its own child module, so this harness exercises the production composition
// (shared `place_roles` return, never a hardcoded hart).
#[path = "../kernel/src/platform/k3_smp_net.rs"]
mod k3_smp_net;

// MS14 Iteration 002 (T7): the k3-smp-uart role-composition seam — the same
// `#[path]` include pattern; the module re-exports the shared placement
// policy in kernel builds and includes it verbatim here (no features), so
// this harness exercises the production composition (shared `place_roles`
// return, never a hardcoded hart).
#[path = "../kernel/src/platform/k3_smp_uart.rs"]
mod k3_smp_uart;

use copier_start_guard::CopierStartGuard;
use k3_smp_net::{place_smp_net_roles, SmpNetRoleError};
use k3_smp_uart::{SmpUartRoleError, place_smp_uart_roles, shell_hart_for};
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

// ---------------------------------------------------------------------------
// MS14 Iteration 001 (T5): the k3-smp-net singleton composition seam
// ---------------------------------------------------------------------------

/// The board qualification shape: 16 ascending harts, anchor = boot hart 0.
fn smp16() -> Vec<usize> {
    (0..16).collect()
}

#[test]
fn smp16_placement_gives_distinct_owner_runner_and_the_app_hart() {
    let roles = place_smp_net_roles(&smp16(), 0).expect("16 ascending harts are valid");
    assert_eq!(
        roles.net_owner, 2,
        "shared place_roles must decide the owner (role index 2), never a hardcode"
    );
    assert_eq!(
        roles.net_runner, 3,
        "shared place_roles must decide the runner (role index 3)"
    );
    assert_ne!(
        roles.net_owner, roles.net_runner,
        "A5: owner and runner singletons must land on distinct harts"
    );
}

#[test]
fn anchor_outside_the_set_starts_at_index_zero_without_topology_invention() {
    // An anchor inside the set starts from its own position (5 → owner=7).
    let roles = place_smp_net_roles(&smp16(), 5).expect("anchor 5 inside the set is valid");
    assert_eq!(roles.net_owner, 7, "in-set anchor consumes its own position");
    assert_eq!(roles.net_runner, 8);
    // An anchor outside the set is equivalent to index 0 (D4, no
    // topology invention): owner=2, runner=3.
    let roles = place_smp_net_roles(&smp16(), 42).expect("anchor 42 outside the set is valid");
    assert_eq!(
        roles.net_owner, 2,
        "D4: an anchor outside the set is equivalent to index 0 (owner=2)"
    );
    assert_eq!(roles.net_runner, 3);
}

#[test]
fn app_hart_nine_must_be_schedulable_and_distinct() {
    assert_eq!(
        place_smp_net_roles(&smp16(), 0).map(|r| r.app_hart()),
        Ok(k3_smp_net::APP_HART),
        "the qualification shape carries the fixed application hart 9"
    );
    // SMP=2 diagnostics: placement stays valid, but hart 9 is not schedulable
    // — the mode must fail closed instead of inventing a hart.
    assert_eq!(
        place_smp_net_roles(&[0, 1], 0),
        Err(SmpNetRoleError::AppHartMissing),
        "SMP<10 has no hart 9: fail closed"
    );
}

#[test]
fn invalid_schedulable_sets_fail_closed() {
    assert_eq!(
        place_smp_net_roles(&[], 0),
        Err(SmpNetRoleError::InvalidSchedulable),
        "empty set is rejected by the shared policy"
    );
    assert_eq!(
        place_smp_net_roles(&[0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14], 0),
        Err(SmpNetRoleError::InvalidSchedulable),
        "duplicates are rejected by the shared policy"
    );
    assert_eq!(
        place_smp_net_roles(&[15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0], 0),
        Err(SmpNetRoleError::InvalidSchedulable),
        "descending order is rejected by the shared policy"
    );
    // MAX_CPU_NUM = 16 in this harness stub: hart 16 is out of range.
    assert_eq!(
        place_smp_net_roles(&[0, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16], 0),
        Err(SmpNetRoleError::InvalidSchedulable),
        "out-of-range hart ids are rejected (release-build garbage-bit defense)"
    );
}

// ---------------------------------------------------------------------------
// MS14 Iteration 002 (T7): the k3-smp-uart singleton composition seam
// ---------------------------------------------------------------------------

#[test]
fn smp16_uart_placement_values_come_from_place_roles() {
    let roles = place_smp_uart_roles(&smp16(), 0).expect("16 ascending harts are valid");
    let p = place_roles(&smp16(), 0).expect("the shared policy decides the copier harts");
    assert_eq!(
        roles.uart_rx, p.uart_rx,
        "the RX copier hart is the shared policy's return, never a hardcode"
    );
    assert_eq!(
        roles.uart_tx, p.uart_tx,
        "the TX copier hart is the shared policy's return"
    );
    assert_eq!(roles.uart_rx, 0, "A9 qualification shape: rx=0");
    assert_eq!(roles.uart_tx, 1, "A9 qualification shape: tx=1");
    assert_ne!(
        roles.uart_rx, roles.uart_tx,
        "A9: copier singletons must land on distinct harts"
    );
    assert_eq!(
        roles.alarm_hart, 0,
        "the alarm rides the boot-hart anchor (D5), not a third fixed hart"
    );
}

#[test]
fn uart_anchor_inside_and_outside_the_set() {
    // An in-set anchor consumes its own position (5 → rx=5, tx=6); the alarm
    // stays on the anchor hart (the main task's hart).
    let roles = place_smp_uart_roles(&smp16(), 5).expect("anchor 5 inside the set is valid");
    assert_eq!(roles.uart_rx, 5);
    assert_eq!(roles.uart_tx, 6);
    assert_eq!(roles.alarm_hart, 5);
    // An out-of-set anchor is equivalent to index 0 for the copier roles (D4,
    // no topology invention) and stays literal for the alarm hart. The boot
    // always passes the running hart, so production never sees this input.
    let roles = place_smp_uart_roles(&smp16(), 42).expect("an out-of-set anchor is accepted");
    assert_eq!(roles.uart_rx, 0);
    assert_eq!(roles.uart_tx, 1);
    assert_eq!(roles.alarm_hart, 42);
}

#[test]
fn shell_hart_requires_at_least_nine_harts() {
    // Exactly nine harts (0..=8): the fixed shell hart is schedulable.
    let nine: Vec<usize> = (0..9).collect();
    let roles9 = place_smp_uart_roles(&nine, 0).expect("nine ascending harts are valid");
    assert_eq!(
        shell_hart_for(&nine, &roles9),
        Ok(k3_smp_uart::SHELL_HART),
        "the shell sub-form resolves to the fixed A100 application hart"
    );
    // Eight harts: hart 8 is missing — fail closed, no invented placement.
    let eight: Vec<usize> = (0..8).collect();
    let roles8 = place_smp_uart_roles(&eight, 0).expect("the copier placement itself stays valid");
    assert_eq!(
        shell_hart_for(&eight, &roles8),
        Err(SmpUartRoleError::ShellHartMissing),
        "A9: below nine harts there is no shell hart — fail closed"
    );
    // SMP=2 diagnostics on the raw-boundary shape: the same missing verdict.
    let roles2 = place_smp_uart_roles(&[0, 1], 0).expect("two ascending harts are valid");
    assert_eq!(
        shell_hart_for(&[0, 1], &roles2),
        Err(SmpUartRoleError::ShellHartMissing)
    );
}

#[test]
fn shell_hart_colliding_with_a_copier_is_rejected() {
    // anchor=7 places the TX copier on hart 8; anchor=8 places RX there.
    let roles = place_smp_uart_roles(&smp16(), 7).expect("valid");
    assert_eq!(roles.uart_tx, k3_smp_uart::SHELL_HART, "setup: tx on the shell hart");
    assert_eq!(
        shell_hart_for(&smp16(), &roles),
        Err(SmpUartRoleError::ShellHartCollide),
        "A9: the shell singleton must not share a copier hart"
    );
    let roles = place_smp_uart_roles(&smp16(), 8).expect("valid");
    assert_eq!(roles.uart_rx, k3_smp_uart::SHELL_HART, "setup: rx on the shell hart");
    assert_eq!(
        shell_hart_for(&smp16(), &roles),
        Err(SmpUartRoleError::ShellHartCollide)
    );
}

#[test]
fn uart_invalid_schedulable_sets_fail_closed() {
    assert_eq!(
        place_smp_uart_roles(&[], 0),
        Err(SmpUartRoleError::InvalidSchedulable),
        "empty set is rejected by the shared policy"
    );
    assert_eq!(
        place_smp_uart_roles(&[0, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14], 0),
        Err(SmpUartRoleError::InvalidSchedulable),
        "duplicates are rejected by the shared policy"
    );
    let descending: Vec<usize> = (0..16).rev().collect();
    assert_eq!(
        place_smp_uart_roles(&descending, 0),
        Err(SmpUartRoleError::InvalidSchedulable),
        "descending order is rejected by the shared policy"
    );
    // MAX_CPU_NUM = 16 in this harness stub: hart 16 is out of range.
    assert_eq!(
        place_smp_uart_roles(&[0, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16], 0),
        Err(SmpUartRoleError::InvalidSchedulable),
        "out-of-range hart ids are rejected"
    );
}

#[path = "../kernel/src/platform/k3_smp_combined.rs"]
mod k3_smp_combined;

#[test]
fn combined_roles_use_shared_policy_and_fixed_shell() {
    let roles = k3_smp_combined::place_combined_roles(&smp16(), 0).unwrap();
    let expected = place_roles(&smp16(), 0).unwrap();
    assert_eq!((roles.uart_rx, roles.uart_tx, roles.net_owner, roles.net_runner),
               (expected.uart_rx, expected.uart_tx, expected.net_owner, expected.net_runner));
    assert_eq!((roles.shell_hart, roles.alarm_hart), (8, 0));
}

#[test]
fn combined_rejects_invalid_sets_missing_boot_and_shell() {
    use k3_smp_combined::place_combined_roles as place;
    for invalid in [vec![], vec![0, 0, 8], vec![8, 0], vec![0, 8, 16]] {
        assert!(place(&invalid, 0).is_err());
    }
    assert!(place(&smp16(), 16).is_err());
    assert!(place(&(0..8).collect::<Vec<_>>(), 0).is_err());
}

#[test]
fn combined_rejects_background_and_shell_collisions() {
    use k3_smp_combined::place_combined_roles as place;
    assert!(place(&[0, 1, 8], 0).is_err());
    for boot in 5..=8 { assert!(place(&smp16(), boot).is_err()); }
    let roles = place(&smp16(), 9).unwrap();
    let expected = place_roles(&smp16(), 9).unwrap();
    assert_eq!(roles.net_owner, expected.net_owner);
    assert_eq!(roles.alarm_hart, 9);
}
