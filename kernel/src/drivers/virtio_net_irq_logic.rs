//! Pure logic seam for VirtIO-MMIO net IRQ diagnostic control plane.
//!
//! Contains zero MMIO access, zero axnet dependencies, zero wakers.
//! Compiles as both no_std (kernel target) and std (host tests via
//! `#[path]` include from `tests/ms03-irq-host-harness.rs`).
//!
//! ## Responsibilities
//!
//! - `classify_mmio_status`: decode VirtIO MMIO interrupt status byte
//!   (offset 0x60): bit 0 = used-ring, bit 1 = config-change.
//! - `IrqTelemetry`: monotonic relaxed-atomics counters for total
//!   invocations, used-ring, config-change, combined, unknown-bits,
//!   spurious, and ACK count.
//! - `IrqSnapshot`: `repr(C)` read-only snapshot for guest ioctl.

use core::sync::atomic::{AtomicU64, Ordering};

// ── Status classification ─────────────────────────────────────────────

/// Classified VirtIO MMIO interrupt cause.
///
/// Derived from the interrupt status byte at MMIO offset `0x60`:
/// - Bit 0 → used ring update
/// - Bit 1 → config change
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqCause {
    /// No status bits set (spurious).
    None,
    /// Used ring update only (bit 0).
    UsedRing,
    /// Config change only (bit 1).
    ConfigChange,
    /// Both used ring and config change (bits 0 + 1).
    Combined,
    /// Status byte is non-zero but neither bit 0 nor bit 1 is set.
    Unknown,
}

/// Classify a VirtIO MMIO interrupt status byte.
///
/// # Bit layout
///
/// - Bit 0: `USED_RING_UPDATE`
/// - Bit 1: `CONFIG_CHANGE`
/// - Bits 2-7: reserved / implementation-defined
///
/// Bits 2-7 do not change the primary classification but
/// are recorded separately via [`has_unknown_bits`].
pub fn classify_mmio_status(status: u8) -> IrqCause {
    let ring = (status & 0x01) != 0;
    let cfg = (status & 0x02) != 0;
    match (ring, cfg) {
        (false, false) => {
            if status == 0 {
                IrqCause::None
            } else {
                IrqCause::Unknown
            }
        }
        (true, false) => IrqCause::UsedRing,
        (false, true) => IrqCause::ConfigChange,
        (true, true) => IrqCause::Combined,
    }
}

/// Returns `true` when any unsupported/reserved status bit (bit ≥ 2)
/// is set.
pub fn has_unknown_bits(status: u8) -> bool {
    (status & !0x03u8) != 0
}

/// Known status bits (0x03) that must be acknowledged at the device.
///
/// Unknown/reserved bits are never ACKed; they are only observed through
/// [`has_unknown_bits`] so unknown-only causes are not misrecorded as
/// spurious.
pub fn ack_mask(status: u8) -> u8 {
    status & 0x03
}

/// Whether a raw status byte must publish an RX queue event.
///
/// Only causes carrying the used-ring bit (0x01) publish: used-only,
/// used+unknown and combined each publish exactly once; config-only,
/// unknown-only and zero never publish.
pub fn should_publish_rx(status: u8) -> bool {
    (status & 0x01) != 0
}

/// Whether a raw status byte must publish a config-change queue event
/// (Task 3.1 / R6 / A1).
///
/// Only causes carrying the config-change bit (0x02) publish:
/// config-only, config+unknown and combined each publish exactly once;
/// used-only, unknown-only and zero never publish a fabricated CONFIG cause.
pub fn should_publish_config(status: u8) -> bool {
    (status & 0x02) != 0
}

/// Diagnostic classification of the IRQ enable state around the RX wake.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IrqStateObservation {
    /// The handler unexpectedly entered the wake section with IRQs enabled.
    pub enabled_on_entry: bool,
    /// The wake changed an IRQ-disabled entry state to enabled.
    pub restore_violation: bool,
}

/// Classify the IRQ state before and after the fixed RX wake.
pub fn observe_irq_state(before: bool, after: bool) -> IrqStateObservation {
    IrqStateObservation {
        enabled_on_entry: before,
        restore_violation: !before && after,
    }
}

// ── Monotonic telemetry ────────────────────────────────────────────────

/// High-watermark telemetry counters for VirtIO-net IRQ diagnostics.
///
/// All counters use `Relaxed` ordering — they are telemetry only
/// and never participate in synchronization or control-flow decisions.
pub struct IrqTelemetry {
    /// Total handler invocations (every entry, including spurious).
    pub total: AtomicU64,
    /// Used-ring events.
    pub used_ring: AtomicU64,
    /// Config-change events.
    pub config_change: AtomicU64,
    /// Combined events (both bits set simultaneously).
    pub combined: AtomicU64,
    /// Events where any unknown/reserved bit was set.
    pub unknown: AtomicU64,
    /// Spurious events (status byte zero).
    pub spurious: AtomicU64,
    /// ACK write count (MMIO write to offset 0x64).
    pub ack_count: AtomicU64,
    /// IRQ restore violations: an ISR `AtomicWaker::wake()` re-enabled IRQs
    /// before the platform completed the interrupt.
    pub restore_violation: AtomicU64,
    /// Handler wake sections entered with IRQs unexpectedly enabled.
    pub irq_enabled_entry: AtomicU64,
}

impl IrqTelemetry {
    /// Create a new zeroed telemetry set.
    pub const fn new() -> Self {
        Self {
            total: AtomicU64::new(0),
            used_ring: AtomicU64::new(0),
            config_change: AtomicU64::new(0),
            combined: AtomicU64::new(0),
            unknown: AtomicU64::new(0),
            spurious: AtomicU64::new(0),
            ack_count: AtomicU64::new(0),
            restore_violation: AtomicU64::new(0),
            irq_enabled_entry: AtomicU64::new(0),
        }
    }

    /// Record one interrupt event from a raw status byte.
    ///
    /// Increments `total`, the matching cause counter, and optionally
    /// `unknown` (multi-hit if cause is `Combined` *and* unknown bits
    /// are set — this is intentional: a single interrupt can carry
    /// multiple diagnostic signals).
    ///
    /// Returns the classified cause so the caller can act on it
    /// without decoding the status byte a second time.
    pub fn record(&self, status: u8) -> IrqCause {
        self.total.fetch_add(1, Ordering::Relaxed);
        let has_unknown = has_unknown_bits(status);
        let cause = classify_mmio_status(status);
        match cause {
            IrqCause::None => {
                self.spurious.fetch_add(1, Ordering::Relaxed);
            }
            IrqCause::UsedRing => {
                self.used_ring.fetch_add(1, Ordering::Relaxed);
            }
            IrqCause::ConfigChange => {
                self.config_change.fetch_add(1, Ordering::Relaxed);
            }
            IrqCause::Combined => {
                self.combined.fetch_add(1, Ordering::Relaxed);
            }
            IrqCause::Unknown => {}
        }
        if has_unknown {
            self.unknown.fetch_add(1, Ordering::Relaxed);
        }
        cause
    }

    /// Take the fixed MS03 V1 snapshot.
    ///
    /// Individual counter loads are not atomic with respect to each
    /// other — this is acceptable for diagnostic telemetry.
    pub fn snapshot(&self) -> IrqSnapshotV1 {
        IrqSnapshotV1 {
            total: self.total.load(Ordering::Relaxed),
            used_ring: self.used_ring.load(Ordering::Relaxed),
            config_change: self.config_change.load(Ordering::Relaxed),
            combined: self.combined.load(Ordering::Relaxed),
            unknown: self.unknown.load(Ordering::Relaxed),
            spurious: self.spurious.load(Ordering::Relaxed),
            ack_count: self.ack_count.load(Ordering::Relaxed),
            uart_irq_count: 0,
        }
    }

    /// Take the fixed MS04 V2 snapshot before axnet fields are mapped.
    pub fn snapshot_v2(&self) -> IrqSnapshotV2 {
        let v1 = self.snapshot();
        IrqSnapshotV2 {
            total: v1.total,
            used_ring: v1.used_ring,
            config_change: v1.config_change,
            combined: v1.combined,
            unknown: v1.unknown,
            spurious: v1.spurious,
            ack_count: v1.ack_count,
            uart_irq_count: v1.uart_irq_count,
            restore_violation: self.restore_violation.load(Ordering::Relaxed),
            irq_enabled_entry: self.irq_enabled_entry.load(Ordering::Relaxed),
            rx_lifecycle: 0,
            rx_owner: 0,
            isr_publish: 0,
            isr_wake: 0,
            software_nudge: 0,
            task_poll: 0,
            reaped: 0,
            refilled: 0,
            delivered: 0,
            non_ip_consumed: 0,
            budget_exhausted: 0,
            self_yield: 0,
            router_full_wait: 0,
            space_wake: 0,
            empty_check: 0,
            fault: 0,
            last_error_stage: 0,
            last_error_code: 0,
        }
    }
}

// ── Snapshot ABI ───────────────────────────────────────────────────────

/// Fixed MS03 IRQ diagnostic snapshot for guest ioctl `0x4e49_4431`.
///
/// # ABI stability
///
/// This type is exactly eight `u64` fields (64 bytes). It must never grow:
/// existing binaries call a lengthless ioctl with a 64-byte destination.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IrqSnapshotV1 {
    pub total: u64,
    pub used_ring: u64,
    pub config_change: u64,
    pub combined: u64,
    pub unknown: u64,
    pub spurious: u64,
    pub ack_count: u64,
    pub uart_irq_count: u64,
}

/// Fixed MS04 extended snapshot for guest ioctl `0x4e49_4432`.
///
/// The first eight fields match [`IrqSnapshotV1`] byte-for-byte. This is an
/// independent wire type and must not be aliased to or embedded in V1.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IrqSnapshotV2 {
    pub total: u64,
    pub used_ring: u64,
    pub config_change: u64,
    pub combined: u64,
    pub unknown: u64,
    pub spurious: u64,
    pub ack_count: u64,
    pub uart_irq_count: u64,
    /// IRQ restore violations (wake re-enabled IRQs before EOI).
    pub restore_violation: u64,
    /// Handler wake sections entered with IRQs enabled.
    pub irq_enabled_entry: u64,
    /// Async RX lifecycle code (0 Polling .. 4 Unavailable).
    pub rx_lifecycle: u64,
    /// Async RX owner view (0 polling-owned, 1 async-owned).
    pub rx_owner: u64,
    /// ISR event publishes.
    pub isr_publish: u64,
    /// ISR wake calls.
    pub isr_wake: u64,
    /// Explicit software-only wake requests.
    pub software_nudge: u64,
    /// Queue-task polls.
    pub task_poll: u64,
    /// Completions reaped.
    pub reaped: u64,
    /// Descriptors refilled.
    pub refilled: u64,
    /// IP packets delivered.
    pub delivered: u64,
    /// Non-IP completions consumed.
    pub non_ip_consumed: u64,
    /// Budget-exhausted rounds with backlog.
    pub budget_exhausted: u64,
    /// Self-yield wakes.
    pub self_yield: u64,
    /// Router-full waits.
    pub router_full_wait: u64,
    /// Space wakes.
    pub space_wake: u64,
    /// Empty-queue rechecks.
    pub empty_check: u64,
    /// Terminal faults.
    pub fault: u64,
    /// Last error stage code.
    pub last_error_stage: u64,
    /// Last error code.
    pub last_error_code: u64,
}

/// Fixed MS05 diagnostic snapshot for guest ioctl `0x4e49_4433`.
///
/// The first 28 `u64` fields are exactly [`IrqSnapshotV2`] byte-for-byte; the
/// remaining 44 fields append the MS05 slots/tickets/flush/drop/diagnostic
/// ledger. This is an independent wire type and must never be aliased to or
/// embed V1/V2.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IrqSnapshotV3 {
    pub total: u64,
    pub used_ring: u64,
    pub config_change: u64,
    pub combined: u64,
    pub unknown: u64,
    pub spurious: u64,
    pub ack_count: u64,
    pub uart_irq_count: u64,
    pub restore_violation: u64,
    pub irq_enabled_entry: u64,
    pub rx_lifecycle: u64,
    pub rx_owner: u64,
    pub isr_publish: u64,
    pub isr_wake: u64,
    pub software_nudge: u64,
    pub task_poll: u64,
    pub reaped: u64,
    pub refilled: u64,
    pub delivered: u64,
    pub non_ip_consumed: u64,
    pub budget_exhausted: u64,
    pub self_yield: u64,
    pub router_full_wait: u64,
    pub space_wake: u64,
    pub empty_check: u64,
    pub fault: u64,
    pub last_error_stage: u64,
    pub last_error_code: u64,
    /// RX slot occupancy (live frames in the fixed RX slots).
    pub rx_slot_occupancy: u64,
    /// RX slot high-water mark.
    pub rx_slot_high_water: u64,
    /// RX slot full transitions.
    pub rx_slot_full: u64,
    /// RX slot enqueue counter.
    pub rx_slot_enqueue: u64,
    /// RX slot dequeue counter.
    pub rx_slot_dequeue: u64,
    /// RX slot full→space events.
    pub rx_slot_space_event: u64,
    /// TX slot occupancy (live frames in the fixed TX slots).
    pub tx_slot_occupancy: u64,
    /// TX slot high-water mark.
    pub tx_slot_high_water: u64,
    /// TX slot full transitions.
    pub tx_slot_full: u64,
    /// TX slot enqueue counter.
    pub tx_slot_enqueue: u64,
    /// TX slot dequeue counter.
    pub tx_slot_dequeue: u64,
    /// TX slot full→space events.
    pub tx_slot_space_event: u64,
    /// TX submits accepted by the driver.
    pub tx_submit: u64,
    /// TX submit `Again` backpressures.
    pub tx_again: u64,
    /// TX completions observed from the driver.
    pub tx_completion: u64,
    /// TX completions reclaimed (matching DeviceOwned ticket).
    pub tx_reclaim: u64,
    /// TX buffer slots still available to the queue owner.
    pub tx_buffer_available: u64,
    /// TX buffer slots currently inflight (submitted, not reclaimed).
    pub tx_buffer_inflight: u64,
    /// TX descriptors still available.
    pub tx_descriptor_available: u64,
    /// TX descriptors currently inflight.
    pub tx_descriptor_inflight: u64,
    /// Queue rounds that exhausted the reclaim budget.
    pub reclaim_exhausted: u64,
    /// Queue rounds that exhausted the RX copy budget.
    pub rx_exhausted: u64,
    /// Queue rounds that exhausted the submit budget.
    pub submit_exhausted: u64,
    /// Shared queue event generation.
    pub queue_generation: u64,
    /// Queue-owner wake count.
    pub queue_wake: u64,
    /// Last accepted TX ticket (`u64::MAX` when none).
    pub last_accepted: u64,
    /// Live ticket count.
    pub live: u64,
    /// Queued ticket count.
    pub queued: u64,
    /// DeviceOwned ticket count.
    pub device_owned: u64,
    /// Flush target (`u64::MAX` when none).
    pub flush_target: u64,
    /// Flush successes.
    pub flush_success: u64,
    /// Flush faults.
    pub flush_error: u64,
    /// Flush `ResourceBusy` rejections.
    pub flush_busy: u64,
    /// Flush cancellations (future dropped before completion).
    pub flush_cancel: u64,
    /// Diagnostic hold mode (0 none, 1 submit, 2 reclaim).
    pub hold_mode: u64,
    /// Diagnostic lease expiry deadline (nanos; 0 when no hold).
    pub lease_expiry: u64,
    /// Auto-release failures after lease expiry.
    pub auto_release_failure: u64,
    /// Lifecycle transition faults.
    pub lifecycle_fault: u64,
    /// TX ownership invariant violations.
    pub ownership_invariant: u64,
    /// `TxDropReason::MalformedIp` counter.
    pub drop_malformed_ip: u64,
    /// `TxDropReason::NoRoute` counter.
    pub drop_no_route: u64,
    /// `TxDropReason::RouteSourceMismatch` counter.
    pub drop_route_source_mismatch: u64,
    /// `TxDropReason::UnsupportedAddress` counter.
    pub drop_unsupported_address: u64,
    /// `TxDropReason::FrameTooLarge` counter.
    pub drop_frame_too_large: u64,
}

/// QEMU-only MS07 recovery snapshot. `v3` is an immutable byte-for-byte
/// prefix. The appended current and historical-fault tuples are independently
/// coherent and explicitly valid; consumers must not treat them as one instant.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IrqSnapshotV4 {
    pub v3: IrqSnapshotV3,
    pub current_valid: u64,
    pub current_queue_epoch: u64,
    pub current_socket_epoch: u64,
    pub current_link_generation: u64,
    pub current_link_state: u64,
    pub current_owner_available: u64,
    pub current_owner_device_owned: u64,
    pub current_owner_quarantined: u64,
    pub fault_valid: u64,
    pub fault_stage: u64,
    pub fault_cause: u64,
    pub fault_queue_epoch: u64,
    pub fault_owner_available: u64,
    pub fault_owner_device_owned: u64,
    pub fault_owner_quarantined: u64,
}
