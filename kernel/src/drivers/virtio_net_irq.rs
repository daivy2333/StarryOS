//! VirtIO-MMIO net IRQ diagnostic control plane.
//!
//! Initializes an IRQ 7 device handler for a single VirtIO-net MMIO
//! device.  The handler only reads the interrupt status, classifies the
//! cause, writes the MMIO ACK register, and updates pure-logic
//! telemetry counters — it never touches descriptors, queues, axnet or
//! wakers.  The MS02 polling data path remains the sole descriptor
//! owner.
//!
//! # Invariants
//!
//! - Exactly one `VirtIoNetDev`; `irq_num()` stays `None`.
//! - Handler does not wake a queue or stack task.
//! - MS02 10 ms polling fallback stays active.

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
    // Bits 1:0 carry used-ring (bit 0) and config-change (bit 1).
    let status_raw: u32 = unsafe {
        (base as *const u32)
            .add(MMIO_INTERRUPT_STATUS / 4)
            .read_volatile()
    };
    let status = (status_raw & 0x03) as u8;
    if status == 0 {
        TELEMETRY.record(0);
        return;
    }

    let _cause = TELEMETRY.record(status);

    // Acknowledge at device level — write 1 to clear handled bits.
    // SAFETY: InterruptACK is a 32-bit write-only register at offset 0x64.
    unsafe {
        (base as *mut u32)
            .add(MMIO_INTERRUPT_ACK / 4)
            .write_volatile(status_raw & 0x03);
    }
    TELEMETRY.ack_count.fetch_add(1, Ordering::Relaxed);
}

// ── Initialization ─────────────────────────────────────────────────────

/// Initialize the VirtIO-net IRQ diagnostic control plane.
///
/// # What it does
///
/// 1. Reads the optional MMIO net fact from the platform descriptor.
/// 2. Validates VirtIO magic, version and device ID at that address.
/// 3. Registers an IRQ handler that classifies cause and ACKs.
/// 4. Keeps MS02 polling fallback — this handler never touches the
///    polling data path.
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
    // physical address; QEMU VirtIO-MMIO region is 4 KiB.
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

    // Register IRQ handler.  On failure the polling fallback stays active.
    if !axhal::irq::register(cfg.irq, net_irq_handler) {
        ax_println!(
            "[NET IRQ] Failed to register IRQ {} handler; polling fallback remains active",
            cfg.irq
        );
        return;
    }

    ax_println!(
        "[NET IRQ] Diagnostic IRQ {} handler registered; polling fallback active",
        cfg.irq
    );
}

// ── Snapshot for ioctl ─────────────────────────────────────────────────

pub fn irq_snapshot() -> virtio_net_irq_logic::IrqSnapshot {
    let mut s = TELEMETRY.snapshot();
    s.uart_irq_count = uart_16550::async_::isr::irq_count();
    s
}
