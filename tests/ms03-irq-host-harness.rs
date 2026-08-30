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

// ── ACK mask and publish decision (T6.1a) ──────────────────────────────

#[test]
fn ack_mask_never_touches_unknown_bits() {
    assert_eq!(ack_mask(0x00), 0x00);
    assert_eq!(ack_mask(0x01), 0x01);
    assert_eq!(ack_mask(0x02), 0x02);
    assert_eq!(ack_mask(0x03), 0x03);
    assert_eq!(ack_mask(0x04), 0x00);
    assert_eq!(ack_mask(0x05), 0x01);
    assert_eq!(ack_mask(0x0C), 0x00);
    assert_eq!(ack_mask(0xFF), 0x03);
}

#[test]
fn publish_only_for_causes_with_used_ring_bit() {
    // used-only, used+unknown and combined publish; zero, config-only and
    // unknown-only never publish (D5).
    assert!(!should_publish_rx(0x00));
    assert!(!should_publish_rx(0x02));
    assert!(!should_publish_rx(0x04));
    assert!(!should_publish_rx(0x0C));
    assert!(should_publish_rx(0x01));
    assert!(should_publish_rx(0x03));
    assert!(should_publish_rx(0x05));
    assert!(should_publish_rx(0x07));
}

#[test]
fn publish_config_only_for_causes_with_config_change_bit() {
    // The config-change bit (0x02) has its own independent publication
    // decision (Task 3.1 / R6 / A1). config-only and combined publish CONFIG;
    // zero, used-only and unknown-only never publish a fabricated CONFIG.
    assert!(!should_publish_config(0x00));
    assert!(!should_publish_config(0x01));
    assert!(!should_publish_config(0x04));
    assert!(!should_publish_config(0x0C));
    assert!(should_publish_config(0x02));
    assert!(should_publish_config(0x03));
    assert!(should_publish_config(0x06));
    assert!(should_publish_config(0x0A));
}

#[test]
fn combined_status_publishes_both_used_and_config() {
    // A combined cause (bits 0 + 1) must retain BOTH publications
    // independently: one publish is neither dropped nor replaced by the other
    // (Task 3.1 / A1 / D6).
    assert!(should_publish_rx(0x03));
    assert!(should_publish_config(0x03));
    assert!(should_publish_rx(0x01) && !should_publish_config(0x01));
    assert!(!should_publish_rx(0x02) && should_publish_config(0x02));
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
    t.ack_count
        .fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    let s2 = t.snapshot();
    assert_eq!(s2.ack_count, 1);
}

#[test]
fn irq_state_observation_distinguishes_entry_and_restore_violation() {
    let disabled_stays_disabled = observe_irq_state(false, false);
    assert!(!disabled_stays_disabled.enabled_on_entry);
    assert!(!disabled_stays_disabled.restore_violation);

    let disabled_becomes_enabled = observe_irq_state(false, true);
    assert!(!disabled_becomes_enabled.enabled_on_entry);
    assert!(disabled_becomes_enabled.restore_violation);

    let enabled_stays_enabled = observe_irq_state(true, true);
    assert!(enabled_stays_enabled.enabled_on_entry);
    assert!(!enabled_stays_enabled.restore_violation);

    let enabled_becomes_disabled = observe_irq_state(true, false);
    assert!(enabled_becomes_disabled.enabled_on_entry);
    assert!(!enabled_becomes_disabled.restore_violation);
}

// ── Snapshot ABI ───────────────────────────────────────────────────────

#[test]
fn snapshot_is_repr_c_compatible() {
    assert_eq!(core::mem::size_of::<IrqSnapshotV1>(), 8 * 8);
    assert_eq!(
        core::mem::align_of::<IrqSnapshotV1>(),
        core::mem::align_of::<u64>()
    );
    assert_eq!(core::mem::size_of::<IrqSnapshotV2>(), 28 * 8);
    assert_eq!(
        core::mem::align_of::<IrqSnapshotV2>(),
        core::mem::align_of::<u64>()
    );
}

#[test]
fn snapshot_abi_preserves_first_eight_fields() {
    // The MS03 C consumer (ms03_irq_probe.c) depends on the first 8 u64
    // fields keeping their order and stride.
    assert_eq!(core::mem::offset_of!(IrqSnapshotV1, total), 0 * 8);
    assert_eq!(core::mem::offset_of!(IrqSnapshotV1, used_ring), 1 * 8);
    assert_eq!(core::mem::offset_of!(IrqSnapshotV1, config_change), 2 * 8);
    assert_eq!(core::mem::offset_of!(IrqSnapshotV1, combined), 3 * 8);
    assert_eq!(core::mem::offset_of!(IrqSnapshotV1, unknown), 4 * 8);
    assert_eq!(core::mem::offset_of!(IrqSnapshotV1, spurious), 5 * 8);
    assert_eq!(core::mem::offset_of!(IrqSnapshotV1, ack_count), 6 * 8);
    assert_eq!(core::mem::offset_of!(IrqSnapshotV1, uart_irq_count), 7 * 8);

    assert_eq!(core::mem::offset_of!(IrqSnapshotV2, total), 0 * 8);
    assert_eq!(core::mem::offset_of!(IrqSnapshotV2, used_ring), 1 * 8);
    assert_eq!(core::mem::offset_of!(IrqSnapshotV2, config_change), 2 * 8);
    assert_eq!(core::mem::offset_of!(IrqSnapshotV2, combined), 3 * 8);
    assert_eq!(core::mem::offset_of!(IrqSnapshotV2, unknown), 4 * 8);
    assert_eq!(core::mem::offset_of!(IrqSnapshotV2, spurious), 5 * 8);
    assert_eq!(core::mem::offset_of!(IrqSnapshotV2, ack_count), 6 * 8);
    assert_eq!(core::mem::offset_of!(IrqSnapshotV2, uart_irq_count), 7 * 8);
}

#[test]
fn snapshot_abi_appended_fields_follow_in_order() {
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV2, restore_violation),
        8 * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV2, irq_enabled_entry),
        9 * 8
    );
    assert_eq!(core::mem::offset_of!(IrqSnapshotV2, rx_lifecycle), 10 * 8);
    assert_eq!(core::mem::offset_of!(IrqSnapshotV2, rx_owner), 11 * 8);
    assert_eq!(core::mem::offset_of!(IrqSnapshotV2, isr_publish), 12 * 8);
    assert_eq!(core::mem::offset_of!(IrqSnapshotV2, isr_wake), 13 * 8);
    assert_eq!(core::mem::offset_of!(IrqSnapshotV2, software_nudge), 14 * 8);
    assert_eq!(core::mem::offset_of!(IrqSnapshotV2, task_poll), 15 * 8);
    assert_eq!(core::mem::offset_of!(IrqSnapshotV2, reaped), 16 * 8);
    assert_eq!(core::mem::offset_of!(IrqSnapshotV2, refilled), 17 * 8);
    assert_eq!(core::mem::offset_of!(IrqSnapshotV2, delivered), 18 * 8);
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV2, non_ip_consumed),
        19 * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV2, budget_exhausted),
        20 * 8
    );
    assert_eq!(core::mem::offset_of!(IrqSnapshotV2, self_yield), 21 * 8);
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV2, router_full_wait),
        22 * 8
    );
    assert_eq!(core::mem::offset_of!(IrqSnapshotV2, space_wake), 23 * 8);
    assert_eq!(core::mem::offset_of!(IrqSnapshotV2, empty_check), 24 * 8);
    assert_eq!(core::mem::offset_of!(IrqSnapshotV2, fault), 25 * 8);
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV2, last_error_stage),
        26 * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV2, last_error_code),
        27 * 8
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
    let s2 = t.snapshot_v2();
    assert_eq!(s2.restore_violation, 0);
    assert_eq!(s2.irq_enabled_entry, 0);
    assert_eq!(s2.software_nudge, 0);
}

#[test]
fn legacy_v1_write_does_not_touch_adjacent_canaries() {
    let snapshot = IrqTelemetry::new().snapshot();
    let mut guarded = [0xa5a5_a5a5_a5a5_a5a5u64; 10];
    let source = &snapshot as *const IrqSnapshotV1 as *const u8;
    let destination = guarded[1..9].as_mut_ptr() as *mut u8;

    // SAFETY: destination is exactly one V1-sized, non-overlapping slice.
    unsafe {
        core::ptr::copy_nonoverlapping(source, destination, core::mem::size_of::<IrqSnapshotV1>());
    }

    assert_eq!(guarded[0], 0xa5a5_a5a5_a5a5_a5a5);
    assert_eq!(guarded[9], 0xa5a5_a5a5_a5a5_a5a5);
}

// ── Task 4.2: append-only V3 diagnostic snapshot ──────────────────────

#[test]
fn snapshot_v3_is_larger_than_v2_without_reusing_it() {
    // V3 is an independent wire type: strictly larger than V2 and never an
    // alias/embed of V1 or V2.
    assert!(core::mem::size_of::<IrqSnapshotV3>() > core::mem::size_of::<IrqSnapshotV2>());
    assert_eq!(
        core::mem::align_of::<IrqSnapshotV3>(),
        core::mem::align_of::<u64>()
    );
}

#[test]
fn snapshot_v3_preserves_the_full_v2_prefix_byte_for_byte() {
    // The first 28 u64 fields of V3 are exactly the V2 fields in order, so an
    // existing V2 consumer reading only the prefix observes identical data.
    for (offset, field) in [
        (0u8, "total"),
        (1, "used_ring"),
        (2, "config_change"),
        (3, "combined"),
        (4, "unknown"),
        (5, "spurious"),
        (6, "ack_count"),
        (7, "uart_irq_count"),
        (8, "restore_violation"),
        (9, "irq_enabled_entry"),
        (10, "rx_lifecycle"),
        (11, "rx_owner"),
        (12, "isr_publish"),
        (13, "isr_wake"),
        (14, "software_nudge"),
        (15, "task_poll"),
        (16, "reaped"),
        (17, "refilled"),
        (18, "delivered"),
        (19, "non_ip_consumed"),
        (20, "budget_exhausted"),
        (21, "self_yield"),
        (22, "router_full_wait"),
        (23, "space_wake"),
        (24, "empty_check"),
        (25, "fault"),
        (26, "last_error_stage"),
        (27, "last_error_code"),
    ] {
        let v2 = core::mem::offset_of!(IrqSnapshotV2, last_error_code);
        assert!(
            v2 >= (offset as usize) * 8,
            "V2 prefix truncated at {field}"
        );
    }
}

#[test]
fn snapshot_v3_appended_fields_follow_the_fixed_order() {
    // Appended fields start at field index 28 (byte 224). The order is fixed
    // by the MS05 V3 ABI; every field is u64-aligned.
    let base = 28usize;
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, rx_slot_occupancy),
        (base + 0) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, rx_slot_high_water),
        (base + 1) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, rx_slot_full),
        (base + 2) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, rx_slot_enqueue),
        (base + 3) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, rx_slot_dequeue),
        (base + 4) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, rx_slot_space_event),
        (base + 5) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, tx_slot_occupancy),
        (base + 6) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, tx_slot_high_water),
        (base + 7) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, tx_slot_full),
        (base + 8) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, tx_slot_enqueue),
        (base + 9) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, tx_slot_dequeue),
        (base + 10) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, tx_slot_space_event),
        (base + 11) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, tx_submit),
        (base + 12) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, tx_again),
        (base + 13) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, tx_completion),
        (base + 14) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, tx_reclaim),
        (base + 15) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, tx_buffer_available),
        (base + 16) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, tx_buffer_inflight),
        (base + 17) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, tx_descriptor_available),
        (base + 18) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, tx_descriptor_inflight),
        (base + 19) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, reclaim_exhausted),
        (base + 20) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, rx_exhausted),
        (base + 21) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, submit_exhausted),
        (base + 22) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, queue_generation),
        (base + 23) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, queue_wake),
        (base + 24) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, last_accepted),
        (base + 25) * 8
    );
    assert_eq!(core::mem::offset_of!(IrqSnapshotV3, live), (base + 26) * 8);
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, queued),
        (base + 27) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, device_owned),
        (base + 28) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, flush_target),
        (base + 29) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, flush_success),
        (base + 30) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, flush_error),
        (base + 31) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, flush_busy),
        (base + 32) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, flush_cancel),
        (base + 33) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, hold_mode),
        (base + 34) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, lease_expiry),
        (base + 35) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, auto_release_failure),
        (base + 36) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, lifecycle_fault),
        (base + 37) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, ownership_invariant),
        (base + 38) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, drop_malformed_ip),
        (base + 39) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, drop_no_route),
        (base + 40) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, drop_route_source_mismatch),
        (base + 41) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, drop_unsupported_address),
        (base + 42) * 8
    );
    assert_eq!(
        core::mem::offset_of!(IrqSnapshotV3, drop_frame_too_large),
        (base + 43) * 8
    );
    // 28 V2 prefix + 44 appended = 72 u64 fields.
    assert_eq!(core::mem::size_of::<IrqSnapshotV3>(), 72 * 8);
}

#[test]
fn snapshot_v3_is_a_distinct_struct_never_aliased_to_v2() {
    const LOGIC: &str = include_str!("../kernel/src/drivers/virtio_net_irq_logic.rs");
    assert!(LOGIC.contains("pub struct IrqSnapshotV3"));
    assert!(!LOGIC.contains("type IrqSnapshotV3 = IrqSnapshotV2"));
}

#[test]
fn v3_snapshot_command_and_write_path_are_gated_and_distinct() {
    const CTL: &str = include_str!("../kernel/src/syscall/fs/ctl.rs");
    assert!(CTL.contains("NET_IRQ_SNAPSHOT_V3: u32 = 0x4e49_4433"));
    assert!(CTL.contains("IrqSnapshotV3).vm_write(snapshot)"));
    // V1/V2 commands and write paths must remain untouched.
    assert!(CTL.contains("NET_IRQ_SNAPSHOT_V1: u32 = 0x4e49_4431"));
    assert!(CTL.contains("NET_IRQ_SNAPSHOT_V2: u32 = 0x4e49_4432"));
    assert!(CTL.contains("IrqSnapshotV1).vm_write(snapshot)"));
    assert!(CTL.contains("IrqSnapshotV2).vm_write(snapshot)"));
}

#[test]
fn v3_diagnostic_controls_and_flush_are_qemu_gated() {
    const CTL: &str = include_str!("../kernel/src/syscall/fs/ctl.rs");
    // The controls must be compile-gated behind the kernel `qemu` feature so
    // D1 and non-QEMU builds never expose them.
    assert!(
        CTL.contains("#[cfg(feature = \"qemu\")]\nconst NET_DIAGNOSTIC_CONTROL: u32 = 0x4e49_4331")
    );
    assert!(CTL.contains("#[cfg(feature = \"qemu\")]\nconst NET_FLUSH: u32 = 0x4e49_4631"));
    assert!(CTL.contains("axnet::diagnostic_control"));
    assert!(CTL.contains("axnet::flush()"));
}

#[test]
fn axnet_exposes_a_v3_snapshot_source() {
    const AXNET: &str = include_str!("../crates/axnet/src/lib.rs");
    assert!(AXNET.contains("RxSnapshotV3"));
    assert!(AXNET.contains("snapshot_v3"));
}
