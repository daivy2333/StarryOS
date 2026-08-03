//! MS03 host harness: pure-logic VirtIO-MMIO IRQ status decoder and
//! telemetry tests.
//!
//! Compiled and executed by `make host-test`:
//!   rustc --edition=2024 --test tests/ms03-irq-host-harness.rs \
//!     -o /tmp/ms03-irq-host-test && /tmp/ms03-irq-host-test
//!
//! RED state: before virtio_net_irq_logic.rs exists, `rustc --test`
//! fails because the `#[path]` module cannot be found.
//! GREEN state: all test cases pass.

#[path = "../kernel/src/drivers/virtio_net_irq_logic.rs"]
mod virtio_net_irq_logic;

use virtio_net_irq_logic::*;

// ── Status classification ─────────────────────────────────────────────

#[test]
fn classify_used_ring_only() {
    // Bit 0 set → UsedRing
    assert_eq!(classify_mmio_status(0x01), IrqCause::UsedRing);
}

#[test]
fn classify_config_change_only() {
    // Bit 1 set → ConfigChange
    assert_eq!(classify_mmio_status(0x02), IrqCause::ConfigChange);
}

#[test]
fn classify_combined() {
    // Both bits 0 and 1 set → Combined
    assert_eq!(classify_mmio_status(0x03), IrqCause::Combined);
}

#[test]
fn classify_no_status() {
    // Zero → None (spurious)
    assert_eq!(classify_mmio_status(0x00), IrqCause::None);
}

#[test]
fn classify_unknown_only_high_bits() {
    // Bit 2 set only → Unknown (neither bit 0 nor bit 1)
    assert_eq!(classify_mmio_status(0x04), IrqCause::Unknown);
}

#[test]
fn classify_unknown_multiple_high_bits() {
    // Bits 2 and 3 set → Unknown
    assert_eq!(classify_mmio_status(0x0C), IrqCause::Unknown);
}

#[test]
fn classify_used_ring_with_unknown_bits() {
    // Bit 0 + bit 2 → UsedRing (primary cause is UsedRing)
    assert_eq!(classify_mmio_status(0x05), IrqCause::UsedRing);
}

#[test]
fn has_unknown_bits_detects_reserved() {
    assert!(has_unknown_bits(0x04));
    assert!(has_unknown_bits(0x08));
    assert!(has_unknown_bits(0xFF));
}

#[test]
fn has_unknown_bits_false_for_known_only() {
    assert!(!has_unknown_bits(0x00));
    assert!(!has_unknown_bits(0x01));
    assert!(!has_unknown_bits(0x02));
    assert!(!has_unknown_bits(0x03));
}

// ── Telemetry ──────────────────────────────────────────────────────────

#[test]
fn telemetry_new_all_zero() {
    let t = IrqTelemetry::new();
    let s = t.snapshot();
    assert_eq!(s.total, 0);
    assert_eq!(s.used_ring, 0);
    assert_eq!(s.config_change, 0);
    assert_eq!(s.combined, 0);
    assert_eq!(s.unknown, 0);
    assert_eq!(s.spurious, 0);
    assert_eq!(s.ack_count, 0);
    assert_eq!(s.uart_irq_count, 0);
}

#[test]
fn telemetry_record_used_ring_increments_correct_counters() {
    let t = IrqTelemetry::new();
    let cause = t.record(0x01);
    assert_eq!(cause, IrqCause::UsedRing);
    let s = t.snapshot();
    assert_eq!(s.total, 1);
    assert_eq!(s.used_ring, 1);
    assert_eq!(s.config_change, 0);
    assert_eq!(s.combined, 0);
    assert_eq!(s.unknown, 0);
    assert_eq!(s.spurious, 0);
}

#[test]
fn telemetry_record_config_change_increments_correct_counters() {
    let t = IrqTelemetry::new();
    let cause = t.record(0x02);
    assert_eq!(cause, IrqCause::ConfigChange);
    let s = t.snapshot();
    assert_eq!(s.total, 1);
    assert_eq!(s.used_ring, 0);
    assert_eq!(s.config_change, 1);
}

#[test]
fn telemetry_record_combined_increments_combined() {
    let t = IrqTelemetry::new();
    let cause = t.record(0x03);
    assert_eq!(cause, IrqCause::Combined);
    let s = t.snapshot();
    assert_eq!(s.total, 1);
    assert_eq!(s.used_ring, 0);
    assert_eq!(s.config_change, 0);
    assert_eq!(s.combined, 1);
}

#[test]
fn telemetry_record_spurious_increments_spurious() {
    let t = IrqTelemetry::new();
    let cause = t.record(0x00);
    assert_eq!(cause, IrqCause::None);
    let s = t.snapshot();
    assert_eq!(s.total, 1);
    assert_eq!(s.spurious, 1);
}

#[test]
fn telemetry_record_unknown_increments_unknown() {
    let t = IrqTelemetry::new();
    let cause = t.record(0x04);
    assert_eq!(cause, IrqCause::Unknown);
    let s = t.snapshot();
    assert_eq!(s.total, 1);
    assert_eq!(s.unknown, 1);
}

#[test]
fn telemetry_record_combined_with_unknown_bits_counts_both() {
    // Bit 0 + bit 1 + bit 2: primary = Combined, unknown bits also set.
    let t = IrqTelemetry::new();
    let cause = t.record(0x07);
    assert_eq!(cause, IrqCause::Combined);
    let s = t.snapshot();
    assert_eq!(s.total, 1);
    assert_eq!(s.combined, 1);
    assert_eq!(s.unknown, 1);
    // used_ring and config_change should NOT be incremented when cause is Combined
    assert_eq!(s.used_ring, 0);
    assert_eq!(s.config_change, 0);
}

#[test]
fn telemetry_monotonic_across_mixed_events() {
    let t = IrqTelemetry::new();
    t.record(0x01); // used_ring
    t.record(0x02); // config_change
    t.record(0x03); // combined
    t.record(0x00); // spurious
    t.record(0x01); // used_ring again
    t.record(0x04); // unknown
    t.record(0x03); // combined again

    let s = t.snapshot();
    assert_eq!(s.total, 7);
    assert_eq!(s.used_ring, 2);
    assert_eq!(s.config_change, 1);
    assert_eq!(s.combined, 2);
    assert_eq!(s.spurious, 1);
    assert_eq!(s.unknown, 1); // only the 0x04 event has unknown-only bits
}

#[test]
fn telemetry_ack_count_requires_explicit_increment() {
    let t = IrqTelemetry::new();
    // record() does NOT increment ack_count — that's the ISR's job
    t.record(0x01);
    let s = t.snapshot();
    assert_eq!(s.ack_count, 0);

    // explicit ACK increment (simulating MMIO write to 0x64)
    t.ack_count.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let s2 = t.snapshot();
    assert_eq!(s2.ack_count, 1);
}

// ── Snapshot ABI ───────────────────────────────────────────────────────

#[test]
fn snapshot_is_repr_c_compatible() {
    // Verify field layout is stable by checking size and offset.
    assert_eq!(core::mem::size_of::<IrqSnapshot>(), 8 * 8);
    assert_eq!(
        core::mem::align_of::<IrqSnapshot>(),
        core::mem::align_of::<u64>()
    );
}

#[test]
fn snapshot_zero_fields_on_new_telemetry() {
    let t = IrqTelemetry::new();
    let s = t.snapshot();
    assert_eq!(s.total, 0);
    assert_eq!(s.used_ring, 0);
    assert_eq!(s.config_change, 0);
    assert_eq!(s.combined, 0);
    assert_eq!(s.unknown, 0);
    assert_eq!(s.spurious, 0);
    assert_eq!(s.ack_count, 0);
    assert_eq!(s.uart_irq_count, 0);
}
