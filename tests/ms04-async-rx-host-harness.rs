//! MS04 host harness: pure-logic critical-section restore policy tests.
//!
//! Compiled and executed by `make host-test`:
//!   rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs \
//!     -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test
//!
//! RED state: before `critical_section_policy.rs` exists, `rustc --test`
//! fails because the `#[path]` module cannot be found.
//! GREEN state: all test cases pass.

#[path = "../kernel/src/drivers/critical_section_policy.rs"]
mod critical_section_policy;

use critical_section_policy::*;

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
