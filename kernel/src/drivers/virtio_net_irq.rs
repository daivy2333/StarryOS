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

    // Publish used-ring and config-change queue events only after ACK
    // telemetry. Each cause has an independent gate (Task 3.1/R6): the used
    // ring is direction-ambiguous (the queue task resolves RX/TX), while a
    // config-only cause wakes the owner to read a consistent link snapshot.
    // unknown-only and zero never publish (D5).
    let publish_used = virtio_net_irq_logic::should_publish_rx(status);
    let publish_config = virtio_net_irq_logic::should_publish_config(status);
    if publish_used || publish_config {
        let before = axhal::asm::irqs_enabled();
        if publish_used {
            axnet::publish_queue_event();
        }
        if publish_config {
            axnet::publish_config_event();
        }
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

pub fn irq_snapshot_v3() -> virtio_net_irq_logic::IrqSnapshotV3 {
    // The V3 wire type carries the full V2 prefix first; the axnet V3 source
    // duplicates those fields, so map the prefix from the V2 function to keep
    // one authority for the IRQ/telemetry half.
    let v2 = irq_snapshot_v2();
    let mut s = virtio_net_irq_logic::IrqSnapshotV3 {
        total: v2.total,
        used_ring: v2.used_ring,
        config_change: v2.config_change,
        combined: v2.combined,
        unknown: v2.unknown,
        spurious: v2.spurious,
        ack_count: v2.ack_count,
        uart_irq_count: v2.uart_irq_count,
        restore_violation: v2.restore_violation,
        irq_enabled_entry: v2.irq_enabled_entry,
        rx_lifecycle: v2.rx_lifecycle,
        rx_owner: v2.rx_owner,
        isr_publish: v2.isr_publish,
        isr_wake: v2.isr_wake,
        software_nudge: v2.software_nudge,
        task_poll: v2.task_poll,
        reaped: v2.reaped,
        refilled: v2.refilled,
        delivered: v2.delivered,
        non_ip_consumed: v2.non_ip_consumed,
        budget_exhausted: v2.budget_exhausted,
        self_yield: v2.self_yield,
        router_full_wait: v2.router_full_wait,
        space_wake: v2.space_wake,
        empty_check: v2.empty_check,
        fault: v2.fault,
        last_error_stage: v2.last_error_stage,
        last_error_code: v2.last_error_code,
        rx_slot_occupancy: 0,
        rx_slot_high_water: 0,
        rx_slot_full: 0,
        rx_slot_enqueue: 0,
        rx_slot_dequeue: 0,
        rx_slot_space_event: 0,
        tx_slot_occupancy: 0,
        tx_slot_high_water: 0,
        tx_slot_full: 0,
        tx_slot_enqueue: 0,
        tx_slot_dequeue: 0,
        tx_slot_space_event: 0,
        tx_submit: 0,
        tx_again: 0,
        tx_completion: 0,
        tx_reclaim: 0,
        tx_buffer_available: 0,
        tx_buffer_inflight: 0,
        tx_descriptor_available: 0,
        tx_descriptor_inflight: 0,
        reclaim_exhausted: 0,
        rx_exhausted: 0,
        submit_exhausted: 0,
        queue_generation: 0,
        queue_wake: 0,
        last_accepted: u64::MAX,
        live: 0,
        queued: 0,
        device_owned: 0,
        flush_target: u64::MAX,
        flush_success: 0,
        flush_error: 0,
        flush_busy: 0,
        flush_cancel: 0,
        hold_mode: 0,
        lease_expiry: 0,
        auto_release_failure: 0,
        lifecycle_fault: 0,
        ownership_invariant: 0,
        drop_malformed_ip: 0,
        drop_no_route: 0,
        drop_route_source_mismatch: 0,
        drop_unsupported_address: 0,
        drop_frame_too_large: 0,
    };

    // Map the append-only axnet V3 source under the Service guard. The V2
    // prefix fields are identical by construction; only the appended fields
    // are copied, so no V2 field is ever reordered or overwritten.
    let v3 = axnet::rx_snapshot_v3();
    s.rx_slot_occupancy = v3.rx_slot_occupancy;
    s.rx_slot_high_water = v3.rx_slot_high_water;
    s.rx_slot_full = v3.rx_slot_full;
    s.rx_slot_enqueue = v3.rx_slot_enqueue;
    s.rx_slot_dequeue = v3.rx_slot_dequeue;
    s.rx_slot_space_event = v3.rx_slot_space_event;
    s.tx_slot_occupancy = v3.tx_slot_occupancy;
    s.tx_slot_high_water = v3.tx_slot_high_water;
    s.tx_slot_full = v3.tx_slot_full;
    s.tx_slot_enqueue = v3.tx_slot_enqueue;
    s.tx_slot_dequeue = v3.tx_slot_dequeue;
    s.tx_slot_space_event = v3.tx_slot_space_event;
    s.tx_submit = v3.tx_submit;
    s.tx_again = v3.tx_again;
    s.tx_completion = v3.tx_completion;
    s.tx_reclaim = v3.tx_reclaim;
    s.tx_buffer_available = v3.tx_buffer_available;
    s.tx_buffer_inflight = v3.tx_buffer_inflight;
    s.tx_descriptor_available = v3.tx_descriptor_available;
    s.tx_descriptor_inflight = v3.tx_descriptor_inflight;
    s.reclaim_exhausted = v3.reclaim_exhausted;
    s.rx_exhausted = v3.rx_exhausted;
    s.submit_exhausted = v3.submit_exhausted;
    s.queue_generation = v3.queue_generation;
    s.queue_wake = v3.queue_wake;
    s.last_accepted = v3.last_accepted;
    s.live = v3.live;
    s.queued = v3.queued;
    s.device_owned = v3.device_owned;
    s.flush_target = v3.flush_target;
    s.flush_success = v3.flush_success;
    s.flush_error = v3.flush_error;
    s.flush_busy = v3.flush_busy;
    s.flush_cancel = v3.flush_cancel;
    s.hold_mode = v3.hold_mode;
    s.lease_expiry = v3.lease_expiry;
    s.auto_release_failure = v3.auto_release_failure;
    s.lifecycle_fault = v3.lifecycle_fault;
    s.ownership_invariant = v3.ownership_invariant;
    s.drop_malformed_ip = v3.drop_malformed_ip;
    s.drop_no_route = v3.drop_no_route;
    s.drop_route_source_mismatch = v3.drop_route_source_mismatch;
    s.drop_unsupported_address = v3.drop_unsupported_address;
    s.drop_frame_too_large = v3.drop_frame_too_large;
    s
}

/// QEMU-only append-only recovery snapshot.  V1–V3 remain separate ioctl
/// types; V4 only appends the identity that the MS07 probe needs.
#[cfg(feature = "qemu")]
pub fn irq_snapshot_v4() -> virtio_net_irq_logic::IrqSnapshotV4 {
    let recovery = axnet::recovery_snapshot_v4();
    virtio_net_irq_logic::IrqSnapshotV4 {
        v3: irq_snapshot_v3(),
        current_valid: recovery.current_valid,
        current_queue_epoch: recovery.current_queue_epoch,
        current_socket_epoch: recovery.current_socket_epoch,
        current_link_generation: recovery.current_link_generation,
        current_link_state: recovery.current_link_state,
        current_owner_available: recovery.current_owner_available,
        current_owner_device_owned: recovery.current_owner_device_owned,
        current_owner_quarantined: recovery.current_owner_quarantined,
        fault_valid: recovery.fault_valid,
        fault_stage: recovery.fault_stage,
        fault_cause: recovery.fault_cause,
        fault_queue_epoch: recovery.fault_queue_epoch,
        fault_owner_available: recovery.fault_owner_available,
        fault_owner_device_owned: recovery.fault_owner_device_owned,
        fault_owner_quarantined: recovery.fault_owner_quarantined,
    }
}
