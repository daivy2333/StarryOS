//! VirtIO-MMIO net IRQ diagnostic control plane.
//!
//! Initializes an IRQ 7 device handler for a single VirtIO-net MMIO
//! device.  The handler only reads the raw interrupt status, classifies
//! the cause, writes the MMIO ACK register for known bits, updates
//! pure-logic telemetry counters, and — for used-ring causes — publishes a
//! generic queue event through the fixed `axnet` entry (the used ring is
//! direction-ambiguous; the queue task resolves RX/TX under the Service).
//! It never touches descriptors, queues, the Service, smoltcp or waker
//! internals.  The MS02 polling data path remains the sole descriptor
//! owner until the async RX task activates.
//!
//! # Invariants
//!
//! - Exactly one `VirtIoNetDev`; `irq_num()` stays `None`.
//! - Handler does not lock the Service or touch queue-control/descriptors.
//! - MS02 10 ms polling fallback stays active until the lifecycle is Active.
//! - Queue events are published only after device ACK telemetry.
//! - `AtomicWaker::wake()` must not re-enable IRQs before PLIC complete.

use core::sync::atomic::Ordering;

use axhal::mem::phys_to_virt;
use memory_addr::PhysAddr;

use super::virtio_net_irq_logic::{self, IrqTelemetry};
use crate::platform;

// ── VirtIO MMIO register offsets (32-bit aligned) ─────────────────────

const MMIO_MAGIC_VALUE: usize = 0x00;
const MMIO_VERSION: usize = 0x04;
const MMIO_DEVICE_ID: usize = 0x08;
const MMIO_INTERRUPT_STATUS: usize = 0x60;
const MMIO_INTERRUPT_ACK: usize = 0x64;

/// Global monotonic telemetry for the single VirtIO-net IRQ instance.
static TELEMETRY: IrqTelemetry = IrqTelemetry::new();

// ── Handler ────────────────────────────────────────────────────────────

/// Device handler called by the PLIC dispatch.
///
/// Resolves the net MMIO base from the platform descriptor on every
/// invocation (the descriptor is a compile-time constant reference —
/// cheaper than managing a mutable static).
///
/// Order is strict: record raw status -> ACK known bits -> ack telemetry ->
/// publish used-ring RX event.  IRQ enable state is read around the wake;
/// entering disabled and returning enabled increments the restore-violation
/// counter.  No Service/queue/descriptor/smoltcp operation happens here.
fn net_irq_handler() {
    let desc = platform::descriptor();
    let cfg = match &desc.virtio_net {
        Some(cfg) => cfg,
        None => return,
    };
    let vaddr = phys_to_virt(PhysAddr::from(cfg.base_paddr));
    let base = vaddr.as_ptr();

    // SAFETY: base was validated during init; MMIO region is 0x1000 bytes.
    // InterruptStatus is a 32-bit register at offset 0x60.
    let status_raw: u32 = unsafe {
        (base as *const u32)
            .add(MMIO_INTERRUPT_STATUS / 4)
            .read_volatile()
    };
    // The raw low byte goes to the classifier/telemetry; only known bits
    // (0x03) are ever ACKed, so unknown-only is never misrecorded as
    // spurious and known+unknown keeps its unknown observation.
    let status = (status_raw & 0xff) as u8;

    TELEMETRY.record(status);

    let mask = virtio_net_irq_logic::ack_mask(status);
    if mask != 0 {
        // Acknowledge at device level — write 1 to clear handled bits.
        // SAFETY: InterruptACK is a 32-bit write-only register at offset 0x64.
        unsafe {
            (base as *mut u32)
                .add(MMIO_INTERRUPT_ACK / 4)
                .write_volatile(mask as u32);
        }
        TELEMETRY.ack_count.fetch_add(1, Ordering::Relaxed);
    }

    // Publish used-ring queue events only after ACK telemetry. The used
    // ring is direction-ambiguous, so the ISR publishes one generic event;
    // the queue task resolves RX/TX under the shared lock (MS05 T3.3).
    // config-only, unknown-only and zero never publish (D5).
    if virtio_net_irq_logic::should_publish_rx(status) {
        let before = axhal::asm::irqs_enabled();
        axnet::publish_queue_event();
        let after = axhal::asm::irqs_enabled();
        let irq_state = virtio_net_irq_logic::observe_irq_state(before, after);
        if irq_state.enabled_on_entry {
            TELEMETRY.irq_enabled_entry.fetch_add(1, Ordering::Relaxed);
        }
        if irq_state.restore_violation {
            TELEMETRY.restore_violation.fetch_add(1, Ordering::Relaxed);
        }
    }
}

// ── Initialization ─────────────────────────────────────────────────────

/// Initialize the VirtIO-net IRQ diagnostic control plane.
///
/// # What it does
///
/// 1. Reads the optional MMIO net fact from the platform descriptor.
/// 2. Validates VirtIO magic, version and device ID at that address.
/// 3. Registers an IRQ handler that classifies cause, ACKs known bits and
///    publishes used-ring RX events.
/// 4. Starts the unique async RX task only after successful registration.
///    MS02 polling fallback stays active until the task activates.
///
/// # Safety
///
/// The caller must ensure the platform MMIO region is identity-mapped
/// (QEMU satisfies this via `axruntime`).
pub fn init_virtio_net_irq_diag() {
    let desc = platform::descriptor();
    let cfg = match &desc.virtio_net {
        Some(cfg) => cfg,
        None => {
            ax_println!(
                "[NET IRQ] No VirtIO-MMIO net in platform descriptor; skipping IRQ diagnostic"
            );
            return;
        }
    };

    let vaddr = phys_to_virt(PhysAddr::from(cfg.base_paddr));
    let base = vaddr.as_ptr();

    // Validate VirtIO transport header before trusting the IRQ.
    // SAFETY: base was converted from the platform descriptor's known-good
    // physical address; QEMU VirtIO-MMIO region is 4 KiB.
    let magic: u32 = unsafe {
        (base as *const u32)
            .add(MMIO_MAGIC_VALUE / 4)
            .read_volatile()
    };
    let version: u32 = unsafe { (base as *const u32).add(MMIO_VERSION / 4).read_volatile() };
    let device_id: u32 = unsafe { (base as *const u32).add(MMIO_DEVICE_ID / 4).read_volatile() };

    if magic != 0x74726976 {
        ax_println!(
            "[NET IRQ] VirtIO magic mismatch: expected 0x74726976, got 0x{:08x}",
            magic
        );
        return;
    }
    if version < 1 {
        ax_println!("[NET IRQ] VirtIO version too old: {}", version);
        return;
    }
    if device_id != 1 {
        ax_println!(
            "[NET IRQ] Not a network device (device_id={}, expected 1)",
            device_id
        );
        return;
    }

    ax_println!(
        "[NET IRQ] VirtIO-MMIO net validated: magic=0x{:08x} version={} device_id={} at {:#x}",
        magic,
        version,
        device_id,
        cfg.base_paddr
    );

    // Register IRQ handler.  On failure the polling fallback stays active
    // and no async task is started (register-before-start).
    if !axhal::irq::register(cfg.irq, net_irq_handler) {
        ax_println!(
            "[NET IRQ] Failed to register IRQ {} handler; polling fallback remains active",
            cfg.irq
        );
        return;
    }

    ax_println!(
        "[NET IRQ] IRQ {} handler registered; starting async RX queue task",
        cfg.irq
    );

    // Registration succeeded: start the unique RX task exactly once.  A
    // repeated start only records a bounded diagnostic and never spawns a
    // second task.
    if let Err(err) = axnet::start_rx_task() {
        ax_println!("[NET IRQ] start_rx_task: {err:?} (bounded diagnostic, no second task)");
    }
}

// ── Snapshot for ioctl ─────────────────────────────────────────────────

pub fn irq_snapshot_v1() -> virtio_net_irq_logic::IrqSnapshotV1 {
    let mut s = TELEMETRY.snapshot();
    s.uart_irq_count = uart_16550::async_::isr::irq_count();
    s
}

pub fn irq_snapshot_v2() -> virtio_net_irq_logic::IrqSnapshotV2 {
    let mut s = TELEMETRY.snapshot_v2();
    s.uart_irq_count = uart_16550::async_::isr::irq_count();

    // Map the bounded axnet snapshot (no Service lock) into V2 so the guest
    // probe sees lifecycle/task/backpressure state without changing V1.
    let rx = axnet::rx_snapshot();
    s.rx_lifecycle = rx.lifecycle;
    s.rx_owner = rx.owner;
    s.isr_publish = rx.isr_publish;
    s.isr_wake = rx.isr_wake;
    s.software_nudge = rx.software_nudge;
    s.task_poll = rx.task_poll;
    s.reaped = rx.reaped;
    s.refilled = rx.refilled;
    s.delivered = rx.delivered;
    s.non_ip_consumed = rx.non_ip_consumed;
    s.budget_exhausted = rx.budget_exhausted;
    s.self_yield = rx.self_yield;
    s.router_full_wait = rx.router_full_wait;
    s.space_wake = rx.space_wake;
    s.empty_check = rx.empty_check;
    s.fault = rx.fault;
    s.last_error_stage = rx.last_error_stage;
    s.last_error_code = rx.last_error_code;
    s
}
