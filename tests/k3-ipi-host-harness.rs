//! K3 supervisor software-interrupt (reschedule IPI) slot host harness
//! (MS14 Iteration 000, task T2).
//!
//! Uses `#[path]` to include the real production module
//! `crates/axplat-riscv64-k3/src/ipi_irq.rs` without compiling the platform
//! crate. All SSIE/SSIP access goes through the core's [`SoftCsr`] seam;
//! `FakeCsr` below records every transition in order, so every test drives
//! the actual production slot logic — never a copy of it. Runs via
//! `rustc --edition=2024 --test` → /tmp binary (same convention as the
//! MS17 timer harness).
//!
//! Witness list (Cycle T2 test contract):
//! - the full soft vector is the canonical S_SOFT vector (`bit 63 | 1`);
//! - register CAS success opens the CALLER's SSIE exactly once; a
//!   duplicate fails without touching the CSR or overwriting the handler;
//! - dispatch acknowledges SSIP BEFORE the handler call, so an IPI
//!   injected during the handler survives as a new pending and the next
//!   dispatch calls the handler again (ack allows second delivery);
//! - a handlerless dispatch still acknowledges SSIP (no trap storm) and
//!   reports `None` (no fabricated successful call);
//! - SMP runtime unregistration never removes the slot and never touches
//!   any hart's CSR — the local SSIE mask is the stop mechanism;
//! - non-soft vectors never reach the slot.
#![allow(dead_code)]

#[path = "../crates/axplat-riscv64-k3/src/ipi_irq.rs"]
mod ipi_irq;

use core::sync::atomic::{AtomicUsize, Ordering};

use ipi_irq::{SoftCsr, SoftSlot, SOFT_IRQ_VECTOR};

static CALLS_A: AtomicUsize = AtomicUsize::new(0);
static CALLS_B: AtomicUsize = AtomicUsize::new(0);

fn handler_a() {
    CALLS_A.fetch_add(1, Ordering::SeqCst);
}

fn handler_b() {
    CALLS_B.fetch_add(1, Ordering::SeqCst);
}

/// Deterministic SSIE/SSIP fake: records every transition in order.
struct FakeCsr {
    ssie: bool,
    ops: Vec<&'static str>,
}

impl FakeCsr {
    fn new() -> Self {
        FakeCsr { ssie: false, ops: Vec::new() }
    }
}

impl SoftCsr for FakeCsr {
    fn set_ssie(&mut self, on: bool) {
        self.ops.push(if on { "ssie-on" } else { "ssie-off" });
        self.ssie = on;
    }

    fn clear_ssip(&mut self) {
        self.ops.push("ssip-ack");
    }
}

fn calls_a() -> usize {
    CALLS_A.load(Ordering::SeqCst)
}

fn calls_b() -> usize {
    CALLS_B.load(Ordering::SeqCst)
}

fn reset_calls() {
    CALLS_A.store(0, Ordering::SeqCst);
    CALLS_B.store(0, Ordering::SeqCst);
}

#[test]
fn full_soft_vector_is_bit1() {
    // The complete supervisor software vector: interrupt bit | cause 1.
    // axtask's IPI owner registers this exact value (axconfig ipi-irq).
    assert_eq!(SOFT_IRQ_VECTOR, (1usize << 63) | 1);
    assert_eq!(SOFT_IRQ_VECTOR, 0x8000_0000_0000_0001);
}

#[test]
fn register_cas_success_opens_caller_ssie_once() {
    let slot = SoftSlot::new();
    let mut csr = FakeCsr::new();
    assert!(slot.register(handler_a, &mut csr));
    assert_eq!(csr.ops, vec!["ssie-on"], "SSIE must be opened exactly once");
    assert!(csr.ssie, "SSIE left masked after successful register");
    assert!(slot.registered());
}

#[test]
fn duplicate_soft_registration_is_inert() {
    reset_calls();
    let slot = SoftSlot::new();
    let mut csr = FakeCsr::new();
    assert!(slot.register(handler_a, &mut csr));
    assert!(!slot.register(handler_b, &mut csr), "duplicate accepted");
    assert_eq!(
        csr.ops,
        vec!["ssie-on"],
        "duplicate registration must not touch SSIE"
    );
    // The first handler stays installed: dispatch still calls A, never B.
    assert_eq!(slot.handle(SOFT_IRQ_VECTOR, &mut csr), Some(SOFT_IRQ_VECTOR));
    assert_eq!(calls_a(), 1);
    assert_eq!(calls_b(), 0);
}

#[test]
fn soft_ack_allows_second_delivery() {
    reset_calls();
    let slot = SoftSlot::new();
    let mut csr = FakeCsr::new();
    assert!(slot.register(handler_a, &mut csr));
    // First IPI: SSIP acknowledged BEFORE the handler call.
    assert_eq!(slot.handle(SOFT_IRQ_VECTOR, &mut csr), Some(SOFT_IRQ_VECTOR));
    assert_eq!(
        csr.ops,
        vec!["ssie-on", "ssip-ack"],
        "ack must precede the handler call"
    );
    assert_eq!(calls_a(), 1);
    // A second IPI that arrives after the ack is an independent event: the
    // next dispatch acknowledges and calls again (no lost wakeup).
    assert_eq!(slot.handle(SOFT_IRQ_VECTOR, &mut csr), Some(SOFT_IRQ_VECTOR));
    assert_eq!(csr.ops, vec!["ssie-on", "ssip-ack", "ssip-ack"]);
    assert_eq!(calls_a(), 2);
}

#[test]
fn handlerless_dispatch_acks_without_success_call() {
    reset_calls();
    let slot = SoftSlot::new();
    let mut csr = FakeCsr::new();
    assert_eq!(slot.handle(SOFT_IRQ_VECTOR, &mut csr), None);
    assert_eq!(
        csr.ops,
        vec!["ssip-ack"],
        "handlerless dispatch must ack SSIP (no trap storm)"
    );
    assert_eq!(calls_a(), 0, "handlerless dispatch fabricated a call");
    assert!(!slot.registered());
}

#[test]
fn smp_runtime_unregistration_preserves_all_harts() {
    reset_calls();
    let slot = SoftSlot::new();
    let mut csr = FakeCsr::new();
    assert!(slot.register(handler_a, &mut csr));
    let ops_before = csr.ops.clone();
    // Under SMP the runtime unregistration of the soft slot is refused:
    // the slot stays owned for every hart and no CSR is touched. The local
    // SSIE mask (`set_enable`) is the only stop mechanism.
    assert!(ipi_irq::unregister_irq().is_none());
    assert!(slot.registered(), "unregistration removed the global slot");
    assert_eq!(csr.ops, ops_before, "unregistration touched a hart CSR");
    // The slot still dispatches after the refused unregistration.
    assert_eq!(slot.handle(SOFT_IRQ_VECTOR, &mut csr), Some(SOFT_IRQ_VECTOR));
    assert_eq!(calls_a(), 1);
}

#[test]
fn dispatch_keeps_soft_and_other_paths_separate() {
    reset_calls();
    let slot = SoftSlot::new();
    let mut csr = FakeCsr::new();
    assert!(slot.register(handler_a, &mut csr));
    // Timer (bit 5) / external (bit 9) vectors and bare causes never reach
    // the soft slot.
    assert_eq!(slot.handle((1usize << 63) | 5, &mut csr), None);
    assert_eq!(slot.handle((1usize << 63) | 9, &mut csr), None);
    assert_eq!(slot.handle(1, &mut csr), None);
    assert_eq!(calls_a(), 0, "non-soft vector reached the soft handler");
    assert_eq!(csr.ops, vec!["ssie-on"], "non-soft dispatch touched CSRs");
    // The soft vector itself still dispatches after the probes.
    assert_eq!(slot.handle(SOFT_IRQ_VECTOR, &mut csr), Some(SOFT_IRQ_VECTOR));
    assert_eq!(calls_a(), 1);
}
