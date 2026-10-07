//! K3 APLIC MSI + IMSIC S-mode external interrupt interface (MS10 Iteration
//! 000, task 1.2).
//!
//! Replaces the MS09 fail-closed stub: the production AIA core (`crate::aia`)
//! is driven by the shared riscv64 [`aia::HardwareBackend`] and exposed
//! through the axplat `IrqIf`.
//!
//! Compatibility contract (Cycle `001-replan`):
//! - Only the external vector (`interrupt bit | 9`) is claimed; timer (bit 5)
//!   and IPI (bit 1) traps keep the MS09 inert behavior, `STIE`/`SSIE` stay
//!   off and the axhal global hook path is untouched.
//! - Registration is limited to the four fixed routes of the AIA core; every
//!   other source (including the runtime timer IRQ number) is rejected like
//!   the stub did.
//! - The axplat `HandlerTable<512>` (lock-free CAS) is reused as the handler
//!   store; register auto-enables, duplicate registration fails without
//!   touching hardware, unregister masks before removing the handler.

use crate::aia::{self, AiaBackend, AiaChip, EXTERNAL_IRQ_VECTOR};
use axplat::irq::{HandlerTable, IpiTarget, IrqHandler, IrqIf};
use sbi_rt::HartMask;

static HANDLER_TABLE: HandlerTable<512> = HandlerTable::new();

struct TableAdapter;

impl aia::HandlerStore for TableAdapter {
    fn register(&self, source: usize, handler: fn()) -> bool {
        HANDLER_TABLE.register_handler(source, handler)
    }

    fn unregister(&self, source: usize) -> Option<fn()> {
        HANDLER_TABLE.unregister_handler(source)
    }

    fn call(&self, eid: usize) -> bool {
        HANDLER_TABLE.handle(eid)
    }
}

static TABLE_ADAPTER: TableAdapter = TableAdapter;
static CHIP: AiaChip<TableAdapter> = AiaChip::new(&TABLE_ADAPTER);

// ---------------------------------------------------------------------------
// Initialization (called from `init_later`, before kernel stage 1)
// ---------------------------------------------------------------------------

/// Global (per-domain) APLIC initialization. Single-hart builds call this
/// exactly once from `init_later`; failures close SEXT and park (never
/// continue toward stage 9 with an unverified controller). The bring-up
/// window probe and its serial lines run inside the core's init flow.
pub fn init_global() {
    console_line(b"[k3:irq] init=global begin\n");
    let mut backend = aia::HardwareBackend;
    let sink = |line: &str| console_line(line.as_bytes());
    match aia::init_global(&mut backend, sink) {
        Ok(()) => console_line(b"[k3:irq] init=global done\n"),
        Err(e) => irq_init_failed(format_args!("global: {e}")),
    }
}

/// Per-hart IMSIC initialization (SEXT off → EIE cleared, threshold 0,
/// delivery 1 → SEXT on). Failures close SEXT and park.
pub fn init_percpu() {
    console_line(b"[k3:irq] init=percpu begin\n");
    match aia::init_percpu(&mut aia::HardwareBackend) {
        Ok(()) => console_line(b"[k3:irq] init=percpu done\n"),
        Err(e) => irq_init_failed(format_args!("percpu: {e}")),
    }
}

/// Records the boot hartid before the first route registration.
pub fn record_boot_hart(hart: usize) {
    aia::record_boot_hart(hart);
}

/// Readback mismatch path: close SEXT, report the layer, and park. Reset is
/// the recovery path; stage 9 is never reached from here.
fn irq_init_failed(context: core::fmt::Arguments<'_>) -> ! {
    aia::HardwareBackend.set_sext(false);
    error!("k3-irq: init failed ({context}); SEXT closed, parking (reset to recover)");
    console_line(b"[k3:irq] init=failed parked\n");
    park_forever()
}

fn console_line(line: &[u8]) {
    axplat::console::write_bytes(line);
}

fn park_forever() -> ! {
    loop {
        riscv::asm::wfi();
    }
}

// ---------------------------------------------------------------------------
// axplat IrqIf implementation
// ---------------------------------------------------------------------------

struct IrqIfImpl;

#[impl_plat_interface]
impl IrqIf for IrqIfImpl {
    fn set_enable(irq: usize, enabled: bool) {
        CHIP.set_enable(&mut aia::HardwareBackend, irq, enabled);
    }

    fn register(irq: usize, handler: IrqHandler) -> bool {
        // MS17 task T1: under `runtime-irq` the full timer vector goes to the
        // independent slot (CAS + STIE), never into the AIA route table.
        #[cfg(feature = "runtime-irq")]
        if irq == crate::timer_irq::TIMER_IRQ_VECTOR {
            if crate::timer_irq::register_irq(handler) {
                return true;
            }
            warn!("k3-irq: timer handler duplicate rejected");
            return false;
        }
        if CHIP.register(&mut aia::HardwareBackend, irq, handler) {
            true
        } else {
            warn!("k3-irq: register source {irq} rejected (unsupported route or duplicate)");
            false
        }
    }

    fn unregister(irq: usize) -> Option<IrqHandler> {
        #[cfg(feature = "runtime-irq")]
        if irq == crate::timer_irq::TIMER_IRQ_VECTOR {
            return crate::timer_irq::unregister_irq();
        }
        CHIP.unregister(&mut aia::HardwareBackend, irq)
    }

    fn handle(irq: usize) -> Option<usize> {
        // MS17 task T1: the timer vector dispatches exactly once through the
        // independent slot and returns that vector.
        #[cfg(feature = "runtime-irq")]
        if irq == crate::timer_irq::TIMER_IRQ_VECTOR {
            return crate::timer_irq::handle_irq(irq);
        }
        // Only the external vector claims; timer/IPI vectors keep the MS09
        // inert behavior when the timer slot is not compiled in (no STIE/SSIE
        // is ever opened there).
        if irq != EXTERNAL_IRQ_VECTOR {
            return None;
        }
        CHIP.handle_external(&mut aia::HardwareBackend)
    }

    fn send_ipi(irq_num: usize, target: IpiTarget) {
        let _ = irq_num;
        let mask = match target {
            IpiTarget::Current { cpu_id } | IpiTarget::Other { cpu_id } => {
                HartMask::from_mask_base(1, cpu_id)
            }
            IpiTarget::AllExceptCurrent { cpu_id, cpu_num } => {
                let all = if cpu_num >= usize::BITS as usize {
                    usize::MAX
                } else {
                    (1usize << cpu_num) - 1
                };
                HartMask::from_mask_base(all & !(1usize << cpu_id), 0)
            }
        };
        // The SBI performs the actual MSI injection into the target hart's
        // interrupt file (EID 0 stays reserved for it; no raw MSI writes).
        let ret = sbi_rt::send_ipi(mask);
        if ret.is_err() {
            error!("k3-irq: send_ipi failed (error={:#x})", ret.error);
        }
    }
}

// ---------------------------------------------------------------------------
// Minimal observation interface for the stage 9 probe (MS10 task 1.3)
// ---------------------------------------------------------------------------

/// Read-only dispatch diagnostics for the kernel-side probe. The foreground
/// reads these between steps; the claim path itself stays exclusively in the
/// trap handler (`handle`).
pub mod probe {
    use super::CHIP;

    /// Raw `stopei` value of the most recent dispatch attempt.
    pub fn last_raw_claim() -> u64 {
        CHIP.last_raw_claim()
    }

    /// Zero-claim dispatches so far.
    pub fn spurious_count() -> usize {
        CHIP.spurious_count()
    }

    /// Claimed-but-handlerless dispatches so far.
    pub fn unknown_count() -> usize {
        CHIP.unknown_count()
    }
}
