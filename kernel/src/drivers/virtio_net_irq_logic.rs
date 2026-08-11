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

    /// Take a read-only snapshot of all counters.
    ///
    /// Individual counter loads are not atomic with respect to each
    /// other — this is acceptable for diagnostic telemetry.
    pub fn snapshot(&self) -> IrqSnapshot {
        IrqSnapshot {
            total: self.total.load(Ordering::Relaxed),
            used_ring: self.used_ring.load(Ordering::Relaxed),
            config_change: self.config_change.load(Ordering::Relaxed),
            combined: self.combined.load(Ordering::Relaxed),
            unknown: self.unknown.load(Ordering::Relaxed),
            spurious: self.spurious.load(Ordering::Relaxed),
            ack_count: self.ack_count.load(Ordering::Relaxed),
            uart_irq_count: 0,
            restore_violation: self.restore_violation.load(Ordering::Relaxed),
            // The appended RX fields are filled by the kernel glue from the
            // bounded `axnet::rx_snapshot()`; the pure seam initializes them
            // to zero so the ABI is total before the mapping runs.
            rx_lifecycle: 0,
            rx_owner: 0,
            isr_publish: 0,
            isr_wake: 0,
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

/// Read-only IRQ diagnostic snapshot for guest ioctl `0x4e49_4431`.
///
/// # ABI stability
///
/// This is a diagnostic-only, QEMU-cfg-gated interface.
/// The first 8 `u64` fields are append-only and must not be reordered or
/// removed; MS03 guest consumers depend on their layout. All fields appended
/// by MS04 (T6.1) keep the same 8-byte stride and must stay in order. The C
/// consumer in `tests/ms03_irq_probe.c` is synchronized in the same iteration.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IrqSnapshot {
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
    /// Async RX lifecycle code (0 Polling .. 4 Unavailable).
    pub rx_lifecycle: u64,
    /// Async RX owner view (0 polling-owned, 1 async-owned).
    pub rx_owner: u64,
    /// ISR event publishes.
    pub isr_publish: u64,
    /// ISR wake calls.
    pub isr_wake: u64,
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
