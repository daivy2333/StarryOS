//! Independent K3 supervisor timer (bit 5) handler slot (MS17 Iteration 000,
//! task T1).
//!
//! The AIA core (`crate::aia`) owns only the external vector; before this
//! module the runtime timer registration was rejected like any unsupported
//! route and `STIE` stayed off. This slot gives the full timer vector
//! ([`TIMER_IRQ_VECTOR`]) its own single handler entry, host-testable through
//! the [`TimerCsr`] seam: it contains only `core` code, so host harnesses run
//! the real register/duplicate/mask/dispatch logic against a fake CSR.
//!
//! Contract (Cycle T1):
//! - register succeeds only on a CAS from empty and opens `STIE` exactly
//!   once; a duplicate fails without touching the CSR or the handler;
//! - unregister masks `STIE` before removing the handler;
//! - dispatch claims only the timer vector, calls the handler exactly once
//!   per trap and returns that vector; every other vector returns `None`.
//!   `SSIE`/`SEXT` and the external claim path are never touched here.

use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

/// Complete supervisor timer vector (`interrupt bit | cause 5`), matching the
/// `timer-irq` platform config the runtime registers against.
pub const TIMER_IRQ_VECTOR: usize = (1usize << 63) | 5;

/// `STIE` CSR access seam. Production drives `sie.STIE`; host harnesses
/// record the transitions.
pub trait TimerCsr {
    /// Sets or clears `STIE` of the current hart.
    fn set_stie(&mut self, on: bool);
}

/// Independent timer handler slot: one handler for the full timer vector.
/// Shared between the registering thread and IRQ context; all state is
/// atomic.
pub struct TimerSlot {
    handler: AtomicUsize,
    enabled: AtomicBool,
}

impl TimerSlot {
    pub const fn new() -> Self {
        TimerSlot { handler: AtomicUsize::new(0), enabled: AtomicBool::new(false) }
    }

    /// Registers `handler` for the timer vector. On a successful CAS the
    /// slot opens `STIE`; a duplicate fails and leaves both the handler and
    /// the CSR state untouched.
    pub fn register(&self, handler: fn(), csr: &mut impl TimerCsr) -> bool {
        if self
            .handler
            .compare_exchange(0, handler as usize, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            return false;
        }
        csr.set_stie(true);
        self.enabled.store(true, Ordering::Release);
        true
    }

    /// Masks `STIE` first, then removes and returns the handler. Returns
    /// `None` without touching the CSR when nothing is registered.
    pub fn unregister(&self, csr: &mut impl TimerCsr) -> Option<fn()> {
        if self.handler.load(Ordering::Acquire) == 0 {
            return None;
        }
        csr.set_stie(false);
        self.enabled.store(false, Ordering::Release);
        let prev = self.handler.swap(0, Ordering::AcqRel);
        Some(unsafe { core::mem::transmute::<usize, fn()>(prev) })
    }

    /// Dispatches the timer vector: calls the registered handler exactly
    /// once and returns the vector. Any other vector — including the
    /// external one — returns `None` without calling anything.
    pub fn handle(&self, vector: usize) -> Option<usize> {
        if vector != TIMER_IRQ_VECTOR {
            return None;
        }
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
        self.handler.load(Ordering::Acquire) != 0
    }

    /// Mirrored `STIE` state of the slot.
    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }
}

// ---------------------------------------------------------------------------
// Production bindings (riscv64 builds only; host harnesses supply fakes)
// ---------------------------------------------------------------------------

/// The process-wide timer slot bound by the axplat `IrqIf` under the
/// `runtime-irq` feature.
pub static TIMER_SLOT: TimerSlot = TimerSlot::new();

/// Production `STIE` backend: the local hart's `sie.STIMER` bit.
#[cfg(target_arch = "riscv64")]
pub struct ProductionCsr;

#[cfg(target_arch = "riscv64")]
impl TimerCsr for ProductionCsr {
    fn set_stie(&mut self, on: bool) {
        if on {
            // SAFETY: opens the supervisor timer interrupt enable; only the
            // timer slot registration/unregistration paths run this.
            unsafe { riscv::register::sie::set_stimer() }
        } else {
            // SAFETY: masks the supervisor timer interrupt enable.
            unsafe { riscv::register::sie::clear_stimer() }
        }
    }
}

/// Registers the runtime timer handler on the production slot.
#[cfg(target_arch = "riscv64")]
pub fn register_irq(handler: fn()) -> bool {
    TIMER_SLOT.register(handler, &mut ProductionCsr)
}

/// Unregisters the runtime timer handler from the production slot.
#[cfg(target_arch = "riscv64")]
pub fn unregister_irq() -> Option<fn()> {
    TIMER_SLOT.unregister(&mut ProductionCsr)
}

/// Dispatches the timer vector on the production slot.
pub fn handle_irq(vector: usize) -> Option<usize> {
    // MS13 001-rework T5-R1 diagnostics: every timer trap that reached the
    // axplat dispatch counts here, before any slot-state decision (same
    // observation-only shape as `aia::AIA_DEBUG_TRAPS`). A frozen count with
    // `sip.STIP=1` splits the fault to trap delivery, not to the slot.
    TIMER_DEBUG_TICKS.fetch_add(1, Ordering::Relaxed);
    TIMER_SLOT.handle(vector)
}

// ---------------------------------------------------------------------------
// MS13 001-rework T5-R1 diagnostics: board discriminator observation
// ---------------------------------------------------------------------------

/// Timer traps that reached the platform dispatch ([`handle_irq`]). Read by
/// the board discriminator lines; never participates in control.
pub static TIMER_DEBUG_TICKS: AtomicU64 = AtomicU64::new(0);

/// Raw supervisor trap-state CSRs for the board discriminator lines
/// (observation-only). `sie`: `STIE` = bit 5, `SEXT` = bit 9; `sip`:
/// `STIP` = bit 5, `SEIP` = bit 9; `sstatus`: global `SIE` = bit 5.
#[cfg(target_arch = "riscv64")]
pub struct TrapCsrSnapshot {
    pub sie: usize,
    pub sip: usize,
    pub sstatus: usize,
}

/// Reads `sie`/`sip`/`sstatus` of the current hart in one snapshot.
#[cfg(target_arch = "riscv64")]
pub fn debug_trap_csr_snapshot() -> TrapCsrSnapshot {
    let sie: usize;
    let sip: usize;
    let sstatus: usize;
    // SAFETY: read-only CSR reads in S-mode; none of the three has a side
    // effect on this platform (`sip.SEIP` mirrors the IMSIC pending word).
    unsafe {
        core::arch::asm!(
            "csrr {sie}, sie",
            "csrr {sip}, sip",
            "csrr {st}, sstatus",
            sie = out(reg) sie,
            sip = out(reg) sip,
            st = out(reg) sstatus,
        )
    }
    TrapCsrSnapshot { sie, sip, sstatus }
}
