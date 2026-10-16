//! K3 supervisor timer handler slot host harness (MS17 Iteration 000, T1).
//!
//! Uses `#[path]` to include the real production module
//! `crates/axplat-riscv64-k3/src/timer_irq.rs` without compiling the platform
//! crate. All SIE access goes through the core's [`TimerCsr`] seam; `FakeCsr`
//! below records every STIE transition and its order, so every test drives the
//! actual production slot logic — never a copy of it. Runs via
//! `rustc --edition=2024 --test` → /tmp binary (same convention as the MS09/
//! MS10 probe harnesses).
//!
//! Witness list (Cycle T1 test contract):
//! - register CAS success opens STIE exactly once; duplicate register fails
//!   without touching the CSR or overwriting the handler;
//! - unregister masks STIE before removing the handler and is reusable;
//! - dispatch calls the registered handler exactly once per call and returns
//!   the full timer vector; other vectors never reach the slot;
//! - the full vector is the canonical supervisor timer vector (bit 5).
#![allow(dead_code)]

#[path = "../crates/axplat-riscv64-k3/src/timer_irq.rs"]
mod timer_irq;

use core::sync::atomic::{AtomicUsize, Ordering};

use timer_irq::{TimerCsr, TimerSlot, TIMER_IRQ_VECTOR};

static CALLS_A: AtomicUsize = AtomicUsize::new(0);
static CALLS_B: AtomicUsize = AtomicUsize::new(0);

fn handler_a() {
    CALLS_A.fetch_add(1, Ordering::SeqCst);
}

fn handler_b() {
    CALLS_B.fetch_add(1, Ordering::SeqCst);
}

/// Deterministic STIE fake: records every transition in order.
struct FakeCsr {
    state: bool,
    ops: Vec<&'static str>,
}

impl FakeCsr {
    fn new() -> Self {
        FakeCsr { state: false, ops: Vec::new() }
    }
}

impl TimerCsr for FakeCsr {
    fn set_stie(&mut self, on: bool) {
        self.ops.push(if on { "stie-on" } else { "stie-off" });
        self.state = on;
    }
}

fn calls_a() -> usize {
    CALLS_A.load(Ordering::SeqCst)
}

fn calls_b() -> usize {
    CALLS_B.load(Ordering::SeqCst)
}

/// Resets the shared handler counters: the fn() handlers carry no state, so
/// the statics are the only counters and tests run sequentially.
fn reset_calls() {
    CALLS_A.store(0, Ordering::SeqCst);
    CALLS_B.store(0, Ordering::SeqCst);
}

#[test]
fn full_timer_vector_is_bit5() {
    // The complete supervisor timer vector: interrupt bit | cause 5. The
    // legacy K3 TimeIf::irq_num hands this exact value to axruntime's
    // init_interrupt registration.
    assert_eq!(TIMER_IRQ_VECTOR, (1usize << 63) | 5);
    assert_eq!(TIMER_IRQ_VECTOR, 0x8000_0000_0000_0005);
}

#[test]
fn register_cas_success_opens_stie_once() {
    let slot = TimerSlot::new();
    let mut csr = FakeCsr::new();
    assert!(slot.register(handler_a, &mut csr));
    assert_eq!(csr.ops, vec!["stie-on"], "STIE must be opened exactly once");
    assert!(csr.state, "STIE left masked after successful register");
    assert!(slot.registered());
    assert!(slot.enabled());
}

#[test]
fn duplicate_register_rejected_without_csr_effect() {
    reset_calls();
    let slot = TimerSlot::new();
    let mut csr = FakeCsr::new();
    assert!(slot.register(handler_a, &mut csr));
    assert!(!slot.register(handler_b, &mut csr), "duplicate accepted");
    assert_eq!(
        csr.ops,
        vec!["stie-on"],
        "duplicate register must not touch STIE"
    );
    // The first handler stays installed: dispatch still calls A, never B.
    assert_eq!(slot.handle(TIMER_IRQ_VECTOR), Some(TIMER_IRQ_VECTOR));
    assert_eq!(calls_a(), 1);
    assert_eq!(calls_b(), 0);
}

#[test]
fn unregister_masks_before_removal_and_is_reusable() {
    reset_calls();
    let slot = TimerSlot::new();
    let mut csr = FakeCsr::new();
    assert!(slot.register(handler_a, &mut csr));
    assert!(slot.unregister(&mut csr).is_some(), "lost an existing handler");
    assert_eq!(
        csr.ops,
        vec!["stie-on", "stie-off"],
        "unregister must mask STIE before removing the handler"
    );
    assert!(!csr.state, "STIE left open after unregister");
    assert!(!slot.registered());
    assert!(!slot.enabled());
    assert_eq!(slot.handle(TIMER_IRQ_VECTOR), None, "dispatch after removal");

    // The slot is reusable: a fresh handler registers and re-opens STIE.
    assert!(slot.register(handler_b, &mut csr));
    assert_eq!(csr.ops, vec!["stie-on", "stie-off", "stie-on"]);
    assert_eq!(slot.handle(TIMER_IRQ_VECTOR), Some(TIMER_IRQ_VECTOR));
    assert_eq!(calls_b(), 1);
    assert_eq!(calls_a(), 0, "previous handler must not fire after swap");
}

#[test]
fn unregister_without_handler_is_none_without_csr() {
    let slot = TimerSlot::new();
    let mut csr = FakeCsr::new();
    assert!(slot.unregister(&mut csr).is_none());
    assert!(csr.ops.is_empty(), "empty-slot unregister touched STIE");
}

#[test]
fn dispatch_calls_handler_once_per_trap() {
    reset_calls();
    let slot = TimerSlot::new();
    let mut csr = FakeCsr::new();
    assert!(slot.register(handler_a, &mut csr));
    assert_eq!(slot.handle(TIMER_IRQ_VECTOR), Some(TIMER_IRQ_VECTOR));
    assert_eq!(calls_a(), 1);
    // Each SEI trap dispatches exactly once; the runtime handler re-arms the
    // periodic deadline itself, so repeated dispatches are expected.
    assert_eq!(slot.handle(TIMER_IRQ_VECTOR), Some(TIMER_IRQ_VECTOR));
    assert_eq!(calls_a(), 2);
}

#[test]
fn dispatch_keeps_timer_and_external_paths_separate() {
    reset_calls();
    let slot = TimerSlot::new();
    let mut csr = FakeCsr::new();
    assert!(slot.register(handler_a, &mut csr));
    // External vector (bit 9) and bare cause numbers never reach the slot.
    assert_eq!(slot.handle((1usize << 63) | 9), None);
    assert_eq!(slot.handle(5), None);
    assert_eq!(calls_a(), 0, "non-timer vector reached the timer handler");
    // The timer vector itself still dispatches after the probes.
    assert_eq!(slot.handle(TIMER_IRQ_VECTOR), Some(TIMER_IRQ_VECTOR));
    assert_eq!(calls_a(), 1);
}

#[test]
fn dispatch_without_handler_returns_none() {
    let slot = TimerSlot::new();
    let csr = FakeCsr::new();
    assert_eq!(slot.handle(TIMER_IRQ_VECTOR), None);
    assert!(csr.ops.is_empty(), "handlerless dispatch touched STIE");
    assert!(!slot.registered());
}

// ---------------------------------------------------------------------------
// MS14 Iteration 000 task T2: per-hart local STIE control (SMP later-init
// and SMP stop paths). The global slot stays owned by the registering hart;
// local enables/masks touch only the caller's own CSR fake.
// ---------------------------------------------------------------------------

#[test]
fn secondary_local_enable_does_not_register_handler_twice() {
    reset_calls();
    let slot = TimerSlot::new();
    // The boot hart registers the runtime handler and opens its own STIE.
    let mut boot = FakeCsr::new();
    assert!(slot.register(handler_a, &mut boot));
    // A secondary opens its own local STIE through the caller-local helper:
    // the global slot must not see a second registration attempt, and the
    // duplicate must still be refused without touching either CSR.
    let mut secondary = FakeCsr::new();
    timer_irq::set_local_timer_enabled(true, &mut secondary);
    assert_eq!(secondary.ops, vec!["stie-on"], "local enable must set STIE once");
    assert!(!slot.register(handler_b, &mut secondary), "local enable re-registered the slot");
    assert_eq!(
        secondary.ops,
        vec!["stie-on"],
        "refused duplicate must not touch the secondary CSR"
    );
    assert_eq!(calls_b(), 0);
    // The boot registration is untouched: the boot hart still dispatches.
    assert_eq!(slot.handle(TIMER_IRQ_VECTOR), Some(TIMER_IRQ_VECTOR));
    assert_eq!(calls_a(), 1);
    assert_eq!(calls_b(), 0);
    assert!(boot.state, "local secondary enable leaked into the boot CSR");
}

#[test]
fn local_timer_mask_preserves_other_hart() {
    reset_calls();
    let slot = TimerSlot::new();
    let mut hart_a = FakeCsr::new();
    assert!(slot.register(handler_a, &mut hart_a));
    timer_irq::set_local_timer_enabled(true, &mut hart_a);
    let mut hart_b = FakeCsr::new();
    timer_irq::set_local_timer_enabled(true, &mut hart_b);
    // Hart B masks its own STIE: the SMP stop mechanism is caller-local.
    timer_irq::set_local_timer_enabled(false, &mut hart_b);
    assert!(!hart_b.state, "hart B STIE not masked");
    assert_eq!(hart_b.ops, vec!["stie-on", "stie-off"]);
    // Hart A's CSR and the global slot are untouched: A still dispatches.
    assert!(hart_a.state, "hart A STIE changed by hart B's mask");
    assert_eq!(hart_a.ops, vec!["stie-on", "stie-on"]);
    assert!(slot.registered(), "local mask removed the global handler");
    assert_eq!(slot.handle(TIMER_IRQ_VECTOR), Some(TIMER_IRQ_VECTOR));
    assert_eq!(calls_a(), 1);
}
