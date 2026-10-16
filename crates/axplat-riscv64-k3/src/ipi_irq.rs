//! Fixed K3 supervisor software-interrupt (reschedule IPI) slot (MS14
//! Iteration 000, task T2).
//!
//! The full S_SOFT vector (`interrupt bit | cause 1`) gets one CAS-owned
//! handler: axtask's scheduler owns it (single-owner contract — the primary
//! `init_scheduler` registers it before any secondary boots). Registration
//! opens only the CALLING hart's `SSIE`; every other hart enables its own
//! bit in its later-init hook. Dispatch acknowledges `SSIP` before calling
//! the handler, so an IPI injected during the handler survives as a new
//! pending and is taken by the next trap. Under SMP the runtime never
//! unregisters the slot (the caller-local `SSIE` mask is the stop
//! mechanism); on single-hart builds the vector stays rejected exactly as
//! before this module existed.
//!
//! The module contains only `core` code, so host harnesses run the real
//! slot logic against a fake CSR (same shape as `timer_irq.rs`).

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Complete supervisor software vector (`interrupt bit | cause 1`),
/// matching the `ipi-irq` platform config axtask's IPI owner registers.
pub const SOFT_IRQ_VECTOR: usize = (1usize << 63) | 1;

/// `SSIE`/`SSIP` CSR access seam. Production drives the local hart's CSRs;
/// host harnesses record the transitions.
pub trait SoftCsr {
    /// Sets or clears `SSIE` of the current hart.
    fn set_ssie(&mut self, on: bool);
    /// Acknowledges the `SSIP` pending bit.
    fn clear_ssip(&mut self);
}

/// Independent S_SOFT handler slot: one handler for the full soft vector.
/// Shared between the registering thread and IRQ context; all state is
/// atomic.
pub struct SoftSlot {
    handler: AtomicUsize,
    registered: AtomicBool,
}

impl SoftSlot {
    pub const fn new() -> Self {
        SoftSlot { handler: AtomicUsize::new(0), registered: AtomicBool::new(false) }
    }

    /// Registers `handler` for the soft vector. On a successful CAS the
    /// slot opens the CALLER's `SSIE`; a duplicate fails and leaves both
    /// the handler and the CSR state untouched.
    pub fn register(&self, handler: fn(), csr: &mut impl SoftCsr) -> bool {
        if self
            .handler
            .compare_exchange(0, handler as usize, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            return false;
        }
        csr.set_ssie(true);
        self.registered.store(true, Ordering::Release);
        true
    }

    /// Dispatches the soft vector: acknowledges `SSIP` first, then calls
    /// the registered handler exactly once and returns the vector. With no
    /// handler the pending is still acknowledged (no trap storm) but no
    /// call happens and `None` is returned. Any other vector returns `None`
    /// without touching anything.
    pub fn handle(&self, vector: usize, csr: &mut impl SoftCsr) -> Option<usize> {
        if vector != SOFT_IRQ_VECTOR {
            return None;
        }
        csr.clear_ssip();
        let handler = self.handler.load(Ordering::Acquire);
        if handler == 0 {
            return None;
        }
        let handler: fn() = unsafe { core::mem::transmute(handler) };
        handler();
        Some(vector)
    }

    /// Whether a handler is currently registered.
    pub fn registered(&self) -> bool {
        self.registered.load(Ordering::Acquire)
    }
}

// ---------------------------------------------------------------------------
// Production bindings (riscv64 builds only; host harnesses supply fakes)
// ---------------------------------------------------------------------------

/// The process-wide soft slot bound by the axplat `IrqIf` under the `smp`
/// feature.
pub static SOFT_SLOT: SoftSlot = SoftSlot::new();

/// Production `SSIE`/`SSIP` backend: the local hart's CSRs.
#[cfg(target_arch = "riscv64")]
pub struct ProductionCsr;

#[cfg(target_arch = "riscv64")]
impl SoftCsr for ProductionCsr {
    fn set_ssie(&mut self, on: bool) {
        if on {
            // SAFETY: opens the supervisor software interrupt enable; only
            // the soft slot registration and the caller-local enable paths
            // run this.
            unsafe { riscv::register::sie::set_ssoft() }
        } else {
            // SAFETY: masks the supervisor software interrupt enable.
            unsafe { riscv::register::sie::clear_ssoft() }
        }
    }

    fn clear_ssip(&mut self) {
        // SAFETY: acknowledges the supervisor software interrupt pending
        // bit (the SBI re-raises it for every new injection).
        unsafe { riscv::register::sip::clear_ssoft() }
    }
}

/// Registers the reschedule-IPI handler on the production slot.
#[cfg(target_arch = "riscv64")]
pub fn register_irq(handler: fn()) -> bool {
    SOFT_SLOT.register(handler, &mut ProductionCsr)
}

/// MS14 T2 SMP contract: runtime unregistration never removes the global
/// soft slot and never touches any hart's CSR — the local `SSIE` mask
/// (caller-local `set_enable`) is the only stop mechanism. The axplat
/// `IrqIf::unregister` SMP branch returns `None` through this contract.
pub fn unregister_irq() -> Option<fn()> {
    None
}

/// Dispatches the soft vector on the production slot.
#[cfg(target_arch = "riscv64")]
pub fn handle_irq(vector: usize) -> Option<usize> {
    SOFT_SLOT.handle(vector, &mut ProductionCsr)
}

/// Caller-local `SSIE` control only: no slot state, no other hart. The
/// secondary later-init path opens it; SMP stop paths mask it.
pub fn set_local_soft_enabled(on: bool, csr: &mut impl SoftCsr) {
    csr.set_ssie(on);
}

/// Production caller-local `SSIE` control.
#[cfg(target_arch = "riscv64")]
pub fn set_local_soft(on: bool) {
    set_local_soft_enabled(on, &mut ProductionCsr);
}
