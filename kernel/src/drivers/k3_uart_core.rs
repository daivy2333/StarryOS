//! K3 PXA UART production core (MS17 Iteration 000, task T2).
//!
//! Host-testable `core`-only logic for the SpacemiT K3 UART0 (PXA-family
//! 16550 with 32-bit stride-4 MMIO, 64-byte FIFO, `UUE` unit-enable bit).
//! All hardware access goes through the [`PxHw`]/[`PxRegs`] seams — the
//! production binding lives in the kernel (`k3_uart.rs`); host harnesses
//! drive this real module against fakes.
//!
//! Cycle contract implemented here:
//! - one short lock batch per hardware operation (IER cache RMW and the LSR
//!   error latch live inside the same section);
//! - `IER` RMW always preserves `UUE`; the RX group (`RDI|RLSI|RTOIE`) is
//!   armed by the receive task and masked by the ISR, `THRI` likewise for
//!   the send task — notifications stay masked until a task arms them;
//! - every `LSR` read (RX, TX and TEMT polls alike) latches the current
//!   head `PE|FE|BI`; the next RBR read consumes the latch, classifies and
//!   drops the errored byte once. `OE` counts independently and the normal
//!   byte still delivers; `FIFOERR` aggregates only;
//! - the ISR is bounded to eight classified causes, never reads RBR, never
//!   writes THR, prints or allocates; `NO_INT+DR` counts as RX work, an
//!   unknown cause is counted and closes UART notifications down to `UUE`;
//! - receive runs the PXA Errata-20 order (RTOIE cleared before the RBR
//!   batch, restored after), RX budget is 256 bytes per poll cycle and 64
//!   per hardware call; TX gates every byte on `THRE`;
//! - activation is check-not-active → prepare (bounded TX TEMT drain,
//!   reference three-step FCR round EN → EN|clear → off, stale LSR/RBR
//!   drains, runtime FCR = EN|PXAR8, MCR OUT2, IER = UUE) → IRQ
//!   registration → ready; duplicates are explicit and touch nothing, a
//!   failed TX drain or registration rolls back leaving notifications
//!   masked.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

// ---------------------------------------------------------------------------
// Register map (byte offsets inside the stride-4 window)
// ---------------------------------------------------------------------------

/// RBR (read) / THR (write).
pub const REG_RBR_THR: usize = 0x00;
/// IER (interrupt enable; PXA `UUE` at bit 6).
pub const REG_IER: usize = 0x04;
/// IIR (read) / FCR (write).
pub const REG_IIR: usize = 0x08;
/// FCR alias of [`REG_IIR`] for write clarity.
pub const REG_FCR: usize = 0x08;
/// LCR.
pub const REG_LCR: usize = 0x0c;
/// MCR.
pub const REG_MCR: usize = 0x10;
/// LSR.
pub const REG_LSR: usize = 0x14;
/// MSR.
pub const REG_MSR: usize = 0x18;

// IER bits (PXA).
pub const IER_RDI: u8 = 1 << 0;
pub const IER_THRI: u8 = 1 << 1;
pub const IER_RLSI: u8 = 1 << 2;
/// RX timeout interrupt enable (PXA Errata-20 subject).
pub const IER_RTOIE: u8 = 1 << 4;
/// UART unit enable — never cleared by this core.
pub const IER_UUE: u8 = 1 << 6;
/// RX notification group armed by the receive task.
pub const RX_GROUP: u8 = IER_RDI | IER_RLSI | IER_RTOIE;

// LSR bits.
pub const LSR_DR: u8 = 1 << 0;
pub const LSR_OE: u8 = 1 << 1;
pub const LSR_PE: u8 = 1 << 2;
pub const LSR_FE: u8 = 1 << 3;
pub const LSR_BI: u8 = 1 << 4;
pub const LSR_THRE: u8 = 1 << 5;
pub const LSR_TEMT: u8 = 1 << 6;
pub const LSR_FIFOERR: u8 = 1 << 7;
/// Head-byte error set consumed by the next RBR read.
pub const LSR_HEAD_ERRORS: u8 = LSR_PE | LSR_FE | LSR_BI;

// IIR bits.
pub const IIR_NO_INT: u8 = 1 << 0;
pub const IIR_CAUSE_MASK: u8 = 0x0f;
pub const IIR_MSI: u8 = 0x0;
pub const IIR_THRI: u8 = 0x2;
pub const IIR_RDI: u8 = 0x4;
pub const IIR_RLS: u8 = 0x6;
pub const IIR_CTI: u8 = 0xc;

// Setup values (bootloader keeps clock/pins/divisor; never touched here).
/// FIFO enable only (trigger field 00) — reference startup step 1.
pub const FCR_FIFO_ON: u32 = 0x01;
/// FIFO enable | clear RX | clear TX — reference startup step 2; the clear
/// round is legal only inside the activation prepare batch, before business.
pub const FCR_SETUP: u32 = 0x07;
/// FIFO off — reference startup step 3; the stale-RBR drain runs against the
/// holding register while the FIFO is off.
pub const FCR_FIFO_OFF: u32 = 0x00;
/// Runtime FIFO config for the fixed 115200 8N1 mode: FIFO enable | PXAR8
/// (8-byte RX trigger). Reference tiering (R69 `pxa_uart/regs.rs`): PXAR1
/// (trigger 00) only below 2400 baud, PXAR8 for 2400×16 ≤ rate < 230400×16,
/// PXAR32 above. 115200 lands in the PXAR8 tier and the divisor is fixed by
/// the bootloader handoff — the runtime FCR is the constant 0x41.
pub const FCR_PXAR8: u32 = 0x41;
/// OUT2 only; AFE stays off.
pub const MCR_SETUP: u32 = 0x08;
/// 8 data bits, no parity, 1 stop; DLAB off.
pub const LCR_SETUP: u32 = 3;

/// Bounded TX-drain poll cap: a full 64-byte FIFO leaves in ≈5.6 ms at
/// 115200 8N1 (10 bits/byte); each poll is one volatile LSR read plus a spin
/// hint, so this cap bounds the worst-case wait at tens of ms — an order of
/// magnitude above the drain time.
pub const TX_DRAIN_POLL_LIMIT: usize = 1_000_000;
/// Bounded stale-RBR drain: with the FIFO off, `DR` reflects at most the
/// single holding register; eight reads are a generous bound.
pub const STALE_RBR_DRAIN_LIMIT: usize = 8;

/// RX bytes per poll cycle (task-side budget).
pub const RX_POLL_BUDGET: usize = 256;
/// Bytes per hardware call (RX and TX).
pub const HW_CALL_LIMIT: usize = 64;
/// Maximum classified causes per ISR invocation.
pub const ISR_CAUSE_BUDGET: usize = 8;
/// Raw line-accumulation cap: the raw line stage completes at this many
/// bytes even without a terminator (the payload's bounded-accumulation
/// contract, unchanged by the CR/LF acceptance).
pub const RAW_LINE_LIMIT: usize = 512;

/// Raw line completion predicate (rework T2-R4): the operator terminal may
/// end a line with CR, LF or CRLF, so a trailing `\r` or `\n` byte
/// completes; the accumulation also completes at [`RAW_LINE_LIMIT`]. Pure
/// helper so the host harness drives the exact production predicate. This
/// is qualification-payload semantics only — driver/tty CR/LF translation
/// stays with the Iteration 002 tty canonical layer.
pub fn raw_line_complete(acc: &[u8]) -> bool {
    matches!(acc.last(), Some(b'\n') | Some(b'\r')) || acc.len() >= RAW_LINE_LIMIT
}

/// Maps the copier-side abstract IER arm/mask to this port's notification
/// groups (MS17 Iteration 001, task T4). The crate's `IER::DATA_READY`
/// (bit 0) denotes "RX notification armed"; on the PXA UART the ISR masks
/// the whole RX group (`RDI|RLSI|RTOIE`) on every wake, so re-arming only
/// RDI would leave RTOIE masked and strand short lines below the 8-byte
/// trigger (no RDI, no timeout IRQ). `THRI` maps 1:1. Pure helper so the
/// host harness drives the exact production mapping.
pub fn map_copier_ier(set: u8, clear: u8) -> (u8, u8) {
    let mut set = set;
    let mut clear = clear;
    if set & IER_RDI != 0 {
        set |= RX_GROUP;
    }
    if clear & IER_RDI != 0 {
        clear |= RX_GROUP;
    }
    (set, clear)
}

// Activation states.
const STATE_IDLE: u8 = 0;
const STATE_PREPARING: u8 = 1;
const STATE_READY: u8 = 2;

// ---------------------------------------------------------------------------
// Hardware seams
// ---------------------------------------------------------------------------

/// One register window of the UART (stride-4, u32 accesses).
pub trait PxRegs {
    /// Reads one u32 register at the byte offset.
    fn read_u32(&mut self, offset: usize) -> u32;
    /// Writes one u32 register at the byte offset.
    fn write_u32(&mut self, offset: usize, value: u32);
}

/// Hardware binding: serializes one operation batch behind the shared
/// short IRQ-save guard (console and port share the one UART0 instance).
pub trait PxHw: Sync {
    fn with_lock<R>(&self, f: impl FnOnce(&mut dyn PxRegs) -> R) -> R;
}

// ---------------------------------------------------------------------------
// Activation lifecycle
// ---------------------------------------------------------------------------

/// Activation failure with the exact stage. `TxDrain` = the transmitter
/// never drained within the poll cap — no FCR was written and the port is
/// back to idle. `IrqRegister` = the IRQ registration step; notifications
/// stay masked and only this attempt's resources are rolled back.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivateError {
    AlreadyActive,
    TxDrain,
    IrqRegister,
}

// ---------------------------------------------------------------------------
// ISR report and counters
// ---------------------------------------------------------------------------

/// What one ISR invocation decided; the caller (IRQ context) performs the
/// actual waker wakes — the core never touches executors or allocates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IsrReport {
    pub rx_wake: bool,
    pub tx_wake: bool,
    pub unknown: bool,
    pub notifications_closed: bool,
}

/// Snapshot of the core counters for diagnostics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CoreCounts {
    pub irq_count: usize,
    pub rx_wakes: usize,
    pub tx_wakes: usize,
    pub unknown: usize,
    pub overruns: usize,
    pub parity_errors: usize,
    pub framing_errors: usize,
    pub break_errors: usize,
    pub fifo_errors: usize,
}

/// One-shot fault snapshot of the diagnostic surface. Read-only for the
/// core: no latch update, no cache/counter writes, at most one register read
/// each. The raw `LSR`/`IIR` reads consume hardware read-clear bits by
/// nature, which is why the payload runs this at most once per fault and
/// parks right after.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiagSnapshot {
    pub counts: CoreCounts,
    pub notifications_closed: bool,
    /// Hardware `IER` readback (the write shadow behind `ier_cache`).
    pub ier_readback: u8,
    pub ier_cache: u8,
    pub iir: u8,
    pub lsr: u8,
    pub lsr_dr: bool,
}

/// K3 PXA UART core: activation state, IER cache, LSR latch and counters.
/// All methods take `&self` and run their hardware batch inside one
/// [`PxHw::with_lock`] section; shareable between tasks and IRQ context.
pub struct K3UartCore {
    state: AtomicU8,
    ier_cache: AtomicU8,
    /// Latched head-byte `PE|FE|BI` consumed by the next RBR read.
    head_errors: AtomicU8,
    irq_count: AtomicUsize,
    rx_wakes: AtomicUsize,
    tx_wakes: AtomicUsize,
    unknown: AtomicUsize,
    overruns: AtomicUsize,
    parity_errors: AtomicUsize,
    framing_errors: AtomicUsize,
    break_errors: AtomicUsize,
    fifo_errors: AtomicUsize,
    notifications_closed: AtomicBool,
}

impl K3UartCore {
    pub const fn new() -> Self {
        K3UartCore {
            state: AtomicU8::new(STATE_IDLE),
            ier_cache: AtomicU8::new(0),
            head_errors: AtomicU8::new(0),
            irq_count: AtomicUsize::new(0),
            rx_wakes: AtomicUsize::new(0),
            tx_wakes: AtomicUsize::new(0),
            unknown: AtomicUsize::new(0),
            overruns: AtomicUsize::new(0),
            parity_errors: AtomicUsize::new(0),
            framing_errors: AtomicUsize::new(0),
            break_errors: AtomicUsize::new(0),
            fifo_errors: AtomicUsize::new(0),
            notifications_closed: AtomicBool::new(false),
        }
    }

    /// Whether the port finished activation (IRQ registered, ready state).
    pub fn is_ready(&self) -> bool {
        self.state.load(Ordering::Acquire) == STATE_READY
    }

    /// Whether an activation attempt owns the port (preparing or ready).
    pub fn is_active(&self) -> bool {
        self.state.load(Ordering::Acquire) != STATE_IDLE
    }

    /// Whether the last ISR closed notifications (unknown-cause stop).
    pub fn notifications_closed(&self) -> bool {
        self.notifications_closed.load(Ordering::Acquire)
    }

    /// Counter snapshot for diagnostics.
    pub fn counts(&self) -> CoreCounts {
        CoreCounts {
            irq_count: self.irq_count.load(Ordering::Relaxed),
            rx_wakes: self.rx_wakes.load(Ordering::Relaxed),
            tx_wakes: self.tx_wakes.load(Ordering::Relaxed),
            unknown: self.unknown.load(Ordering::Relaxed),
            overruns: self.overruns.load(Ordering::Relaxed),
            parity_errors: self.parity_errors.load(Ordering::Relaxed),
            framing_errors: self.framing_errors.load(Ordering::Relaxed),
            break_errors: self.break_errors.load(Ordering::Relaxed),
            fifo_errors: self.fifo_errors.load(Ordering::Relaxed),
        }
    }

    /// One-shot fault snapshot: a single lock batch reading the diagnostic
    /// surface (IER readback, IER cache, one IIR, one LSR) over the core
    /// counters. Deliberately bypasses the latch discipline — a diagnostic
    /// must not latch head errors, count OE/FIFOERR or touch any state.
    pub fn diag_snapshot(&self, hw: &impl PxHw) -> DiagSnapshot {
        hw.with_lock(|regs| {
            let ier_readback = regs.read_u32(REG_IER) as u8;
            let ier_cache = self.ier_cache.load(Ordering::Relaxed);
            let iir = regs.read_u32(REG_IIR) as u8;
            let lsr = regs.read_u32(REG_LSR) as u8;
            DiagSnapshot {
                counts: self.counts(),
                notifications_closed: self.notifications_closed.load(Ordering::Relaxed),
                ier_readback,
                ier_cache,
                iir,
                lsr,
                lsr_dr: lsr & LSR_DR != 0,
            }
        })
    }

    /// Activates the port: check-not-active → prepare registers → IRQ
    /// registration → ready. Duplicate attempts fail explicitly without a
    /// hardware batch (the active FIFO is never re-cleared). The prepare
    /// batch follows the reference production startup (R69 `pxa_uart`):
    /// bounded TX drain, three-step FCR clear round, stale-status and
    /// stale-RBR drains, then the runtime PXAR8 trigger. A failed TX drain
    /// or IRQ registration rolls back to idle and leaves notifications
    /// masked; no FIFO-clearing write happens on the TX-drain path.
    pub fn activate(
        &self,
        hw: &impl PxHw,
        register_irq: impl FnOnce() -> bool,
    ) -> Result<(), ActivateError> {
        if self
            .state
            .compare_exchange(STATE_IDLE, STATE_PREPARING, Ordering::AcqRel, Ordering::Relaxed)
            .is_err()
        {
            return Err(ActivateError::AlreadyActive);
        }
        let prepared = hw.with_lock(|regs| {
            // Bounded TX drain before any FIFO-clearing write: the tail of a
            // pre-activation console line may still be shifting out, and the
            // clear-TX step below would truncate it mid-line.
            let mut polls = 0usize;
            while regs.read_u32(REG_LSR) as u8 & LSR_TEMT == 0 {
                if polls >= TX_DRAIN_POLL_LIMIT {
                    return Err(ActivateError::TxDrain);
                }
                polls += 1;
                core::hint::spin_loop();
            }
            // DLAB off first so IER/FCR are addressed; 8N1. Clock, pins and
            // divisor stay in the bootloader handoff state.
            regs.write_u32(REG_LCR, LCR_SETUP);
            // Reference three-step startup: FIFO on, both directions
            // cleared, FIFO off.
            regs.write_u32(REG_FCR, FCR_FIFO_ON);
            regs.write_u32(REG_FCR, FCR_SETUP);
            regs.write_u32(REG_FCR, FCR_FIFO_OFF);
            // Drain stale pre-business status without latching it: nothing
            // received before activation belongs to this port's byte stream.
            let _ = regs.read_u32(REG_LSR);
            regs.write_u32(REG_MCR, MCR_SETUP);
            // Bounded stale-RBR drain: the FIFO is off, so `DR` covers at
            // most the holding register; discarded bytes never enter the
            // stream.
            for _ in 0..STALE_RBR_DRAIN_LIMIT {
                if regs.read_u32(REG_LSR) as u8 & LSR_DR == 0 {
                    break;
                }
                let _ = regs.read_u32(REG_RBR_THR);
            }
            // Runtime trigger: the fixed 115200 8N1 mode lands on PXAR8 (see
            // `FCR_PXAR8`); no clear bits — the FIFO stays live from here on.
            regs.write_u32(REG_FCR, FCR_PXAR8);
            // Notifications masked, unit enabled — armed only by the tasks.
            self.write_ier_locked(regs, IER_UUE, RX_GROUP | IER_THRI);
            Ok(())
        });
        if let Err(e) = prepared {
            self.state.store(STATE_IDLE, Ordering::Release);
            return Err(e);
        }
        // The byte stream starts here: error bits latched by pre-activation
        // console polls (same UART, same latch discipline) belong to no
        // business byte and must not drop the first received one.
        self.head_errors.store(0, Ordering::Release);
        if !register_irq() {
            self.state.store(STATE_IDLE, Ordering::Release);
            return Err(ActivateError::IrqRegister);
        }
        // Handler readiness is only now published; tasks arm notifications
        // after this point, never before.
        self.state.store(STATE_READY, Ordering::Release);
        Ok(())
    }

    /// Arms/masks notification groups through the cache; `UUE` is preserved
    /// even if a caller asks to clear it.
    pub fn update_ier(&self, hw: &impl PxHw, set: u8, clear: u8) {
        hw.with_lock(|regs| self.write_ier_locked(regs, set, clear));
    }

    /// Reads up to [`HW_CALL_LIMIT`] received bytes. Runs the PXA Errata-20
    /// order around the RBR batch (RTOIE cleared before, restored after when
    /// it was armed) and the head-error latch discipline per byte.
    pub fn receive_bytes(&self, hw: &impl PxHw, buf: &mut [u8]) -> usize {
        hw.with_lock(|regs| {
            let cap = buf.len().min(HW_CALL_LIMIT);
            let mut n = 0;
            let rtoie_armed = self.ier_cache.load(Ordering::Acquire) & IER_RTOIE != 0;
            if rtoie_armed {
                self.write_ier_locked(regs, 0, IER_RTOIE);
            }
            while n < cap {
                let lsr = self.read_lsr_locked(regs);
                if lsr & LSR_DR == 0 {
                    break;
                }
                let byte = regs.read_u32(REG_RBR_THR) as u8;
                let head = self.head_errors.swap(0, Ordering::AcqRel);
                if head != 0 {
                    // Classify once and drop: the errored byte never enters
                    // the stream; later bytes recover.
                    if head & LSR_PE != 0 {
                        let _ = self.parity_errors.fetch_add(1, Ordering::Relaxed);
                    }
                    if head & LSR_FE != 0 {
                        let _ = self.framing_errors.fetch_add(1, Ordering::Relaxed);
                    }
                    if head & LSR_BI != 0 {
                        let _ = self.break_errors.fetch_add(1, Ordering::Relaxed);
                    }
                    continue;
                }
                buf[n] = byte;
                n += 1;
            }
            if rtoie_armed {
                self.write_ier_locked(regs, IER_RTOIE, 0);
            }
            n
        })
    }

    /// Writes up to [`HW_CALL_LIMIT`] bytes, gating every byte on `THRE`.
    /// Returns the number of bytes accepted.
    pub fn send_bytes(&self, hw: &impl PxHw, buf: &[u8]) -> usize {
        hw.with_lock(|regs| {
            let cap = buf.len().min(HW_CALL_LIMIT);
            let mut n = 0;
            while n < cap {
                let lsr = self.read_lsr_locked(regs);
                if lsr & LSR_THRE == 0 {
                    break;
                }
                regs.write_u32(REG_RBR_THR, buf[n] as u32);
                n += 1;
            }
            n
        })
    }

    /// Whether the transmitter is fully drained (`TEMT`). The LSR read keeps
    /// the head-error latch discipline like every other read.
    pub fn transmitter_empty(&self, hw: &impl PxHw) -> bool {
        hw.with_lock(|regs| self.read_lsr_locked(regs) & LSR_TEMT != 0)
    }

    /// Bounded ISR body: classifies at most [`ISR_CAUSE_BUDGET`] causes,
    /// masks the group it woke (so the FIFO drains without an IRQ storm),
    /// latches line status, never reads RBR / writes THR / prints / allocs.
    /// `NO_INT+DR` counts as RX work; an unknown cause counts and closes
    /// notifications down to `UUE`.
    pub fn isr(&self, hw: &impl PxHw) -> IsrReport {
        hw.with_lock(|regs| {
            let _ = self.irq_count.fetch_add(1, Ordering::Relaxed);
            let mut report = IsrReport::default();
            for _ in 0..ISR_CAUSE_BUDGET {
                let iir = regs.read_u32(REG_IIR) as u8;
                if iir & IIR_NO_INT != 0 {
                    // No IIR cause; the LSR decides whether RX work waits.
                    let lsr = self.read_lsr_locked(regs);
                    if lsr & LSR_DR != 0 {
                        self.write_ier_locked(regs, 0, RX_GROUP);
                        report.rx_wake = true;
                        let _ = self.rx_wakes.fetch_add(1, Ordering::Relaxed);
                    }
                    break;
                }
                match iir & IIR_CAUSE_MASK {
                    IIR_RDI | IIR_CTI => {
                        let _ = self.read_lsr_locked(regs);
                        self.write_ier_locked(regs, 0, RX_GROUP);
                        report.rx_wake = true;
                        let _ = self.rx_wakes.fetch_add(1, Ordering::Relaxed);
                    }
                    IIR_THRI => {
                        self.write_ier_locked(regs, 0, IER_THRI);
                        report.tx_wake = true;
                        let _ = self.tx_wakes.fetch_add(1, Ordering::Relaxed);
                    }
                    IIR_RLS => {
                        // Line status: latch the read-clear errors here; the
                        // errored byte is dropped by the task-side RBR read.
                        let lsr = self.read_lsr_locked(regs);
                        if lsr & LSR_DR != 0 {
                            self.write_ier_locked(regs, 0, RX_GROUP);
                            report.rx_wake = true;
                            let _ = self.rx_wakes.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    IIR_MSI => {
                        // Clear the modem-status source; no wake.
                        let _ = regs.read_u32(REG_MSR);
                    }
                    _ => {
                        let _ = self.unknown.fetch_add(1, Ordering::Relaxed);
                        report.unknown = true;
                        self.write_ier_locked(regs, 0, RX_GROUP | IER_THRI);
                        self.notifications_closed.store(true, Ordering::Release);
                        report.notifications_closed = true;
                        break;
                    }
                }
            }
            report
        })
    }

    /// Latch-aware polled byte output for post-activation diagnostics (the
    /// console shares the hardware; its `LSR` polls keep the same
    /// read-clear discipline). One lock batch per call; `\n` becomes `\r\n`.
    pub fn console_write(&self, hw: &impl PxHw, bytes: &[u8]) {
        hw.with_lock(|regs| {
            for &b in bytes {
                if b == b'\n' {
                    self.console_putbyte_locked(regs, b'\r');
                }
                self.console_putbyte_locked(regs, b);
            }
        })
    }

    fn console_putbyte_locked(&self, regs: &mut dyn PxRegs, b: u8) {
        while self.read_lsr_locked(regs) & LSR_THRE == 0 {
            core::hint::spin_loop();
        }
        regs.write_u32(REG_RBR_THR, b as u32);
    }

    // -- lock-batch helpers (callers hold the guard) ------------------------

    fn write_ier_locked(&self, regs: &mut dyn PxRegs, set: u8, clear: u8) {
        let mut val = (self.ier_cache.load(Ordering::Relaxed) | set) & !clear;
        // The PXA unit-enable is part of the port's own state, never a
        // notification: a clear request for it is refused here.
        val |= IER_UUE;
        self.ier_cache.store(val, Ordering::Relaxed);
        regs.write_u32(REG_IER, val as u32);
    }

    /// Reads LSR: consumes the read-clear error bits, latches `PE|FE|BI`
    /// onto the current head byte and counts `OE` / `FIFOERR` independently.
    fn read_lsr_locked(&self, regs: &mut dyn PxRegs) -> u8 {
        let lsr = regs.read_u32(REG_LSR) as u8;
        if lsr & LSR_HEAD_ERRORS != 0 {
            let _ = self
                .head_errors
                .fetch_or(lsr & LSR_HEAD_ERRORS, Ordering::AcqRel);
        }
        if lsr & LSR_OE != 0 {
            let _ = self.overruns.fetch_add(1, Ordering::Relaxed);
        }
        if lsr & LSR_FIFOERR != 0 {
            let _ = self.fifo_errors.fetch_add(1, Ordering::Relaxed);
        }
        lsr
    }
}
