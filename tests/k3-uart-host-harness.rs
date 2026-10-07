//! K3 PXA UART core host harness (MS17 Iteration 000, task T2).
//!
//! Uses `#[path]` to include the real production module
//! `kernel/src/drivers/k3_uart_core.rs` without compiling the kernel. All
//! MMIO access goes through the core's [`PxHw`]/[`PxRegs`] seams; `FakeHw`
//! below records every access (offset + direction) and models the PXA
//! register semantics (IIR cause sequence, read-clear LSR, THRE/DR), so every
//! test drives the actual production core — never a copy of it. Runs via
//! `rustc --edition=2024 --test` → /tmp binary.
//!
//! Fake hardware model (PXA UART facts from the Cycle contract):
//! - registers at stride-4 byte offsets: RBR/THR 0x00, IER 0x04, IIR (read)
//!   /FCR (write) 0x08, LCR 0x0c, MCR 0x10, LSR 0x14, MSR 0x18;
//! - LSR is read-clear for OE/PE/FE/BI (consumed by every read) and shows
//!   DR/THRE/TEMT levels;
//! - IIR: bit0 NO_INT, cause in the low 4 bits; `iir_sequence` pops one
//!   scripted value per read (last one repeats);
//! - writes are logged as (offset, value); reads are logged as (offset).
#![allow(dead_code)]

#[path = "../kernel/src/drivers/k3_uart_core.rs"]
mod k3_uart_core;

use std::sync::Mutex;

use k3_uart_core::{
    ActivateError, K3UartCore, PxHw, PxRegs, IER_RTOIE, IER_THRI, IER_UUE, IIR_CTI, IIR_NO_INT,
    IIR_RDI, IIR_RLS, IIR_THRI, LSR_BI, LSR_DR, LSR_FE, LSR_FIFOERR, LSR_OE, LSR_PE, LSR_TEMT,
    LSR_THRE, RAW_LINE_LIMIT, REG_FCR, REG_IER, REG_IIR, REG_LCR, REG_LSR, REG_MCR, REG_MSR,
    REG_RBR_THR, RX_GROUP, raw_line_complete,
};

// ---------------------------------------------------------------------------
// Fake hardware
// ---------------------------------------------------------------------------

struct Regs {
    rbr_thr: u32,
    ier: u32,
    lcr: u32,
    mcr: u32,
    /// Read-clear LSR error bits (OE|PE|FE|BI|FIFOERR).
    lsr_errors: u32,
    /// Static LSR levels (THRE|TEMT).
    lsr_level: u32,
    /// When > 0, TEMT stays clear for the first `temt_after_reads` LSR reads
    /// and only then reflects `lsr_level` (TX-drain polling witness).
    temt_after_reads: usize,
    /// Total LSR read counter (TEMT-delay bookkeeping).
    lsr_reads: usize,
    msr: u32,
    /// Scripted IIR read values (last repeats); empty → NO_INT.
    iir_sequence: Vec<u32>,
    /// FIFO of received bytes; RBR reads pop from it.
    rx_fifo: Vec<u8>,
    /// Every access: true = write (offset, value), false = read (offset).
    log: Vec<(bool, usize, u32)>,
    /// IIR read counter (cause-budget witness).
    iir_reads: usize,
}

impl Regs {
    fn new() -> Self {
        Regs {
            rbr_thr: 0,
            ier: 0,
            lcr: 0,
            mcr: 0,
            lsr_errors: 0,
            // A real PXA UART in the bootloader handoff state has an idle
            // transmitter (THRE|TEMT set); tests override when modelling a
            // busy or full FIFO.
            lsr_level: (LSR_THRE | LSR_TEMT) as u32,
            temt_after_reads: 0,
            lsr_reads: 0,
            msr: 0,
            iir_sequence: Vec::new(),
            rx_fifo: Vec::new(),
            log: Vec::new(),
            iir_reads: 0,
        }
    }

    fn reads_at(&self, offset: usize) -> usize {
        self.log.iter().filter(|(w, o, _)| !w && *o == offset).count()
    }

    fn last_write(&self, offset: usize) -> Option<u32> {
        self.log
            .iter()
            .rev()
            .find(|(w, o, _)| *w && *o == offset)
            .map(|(_, _, v)| *v)
    }

    fn writes_at(&self, offset: usize) -> Vec<u32> {
        self.log
            .iter()
            .filter(|(w, o, _)| *w && *o == offset)
            .map(|(_, _, v)| *v)
            .collect()
    }
}

impl PxRegs for Regs {
    fn read_u32(&mut self, offset: usize) -> u32 {
        self.log.push((false, offset, 0));
        match offset {
            REG_RBR_THR => {
                let v = self.rx_fifo.first().copied().unwrap_or(0) as u32;
                if !self.rx_fifo.is_empty() {
                    self.rx_fifo.remove(0);
                }
                v
            }
            REG_IER => self.ier,
            REG_IIR => {
                self.iir_reads += 1;
                let v = if self.iir_sequence.is_empty() {
                    IIR_NO_INT as u32
                } else {
                    let i = self.iir_reads.min(self.iir_sequence.len()) - 1;
                    self.iir_sequence[i]
                };
                v
            }
            REG_LCR => self.lcr,
            REG_MCR => self.mcr,
            REG_LSR => {
                // DR mirrors the FIFO; error bits are read-clear.
                let dr = if self.rx_fifo.is_empty() {
                    0
                } else {
                    LSR_DR as u32
                };
                self.lsr_reads += 1;
                let level = if self.temt_after_reads > 0
                    && self.lsr_reads <= self.temt_after_reads
                {
                    self.lsr_level & !(LSR_TEMT as u32)
                } else {
                    self.lsr_level
                };
                let v = dr | self.lsr_errors | level;
                self.lsr_errors = 0;
                v
            }
            REG_MSR => self.msr,
            _ => 0,
        }
    }

    fn write_u32(&mut self, offset: usize, value: u32) {
        self.log.push((true, offset, value));
        match offset {
            REG_RBR_THR => self.rbr_thr = value,
            REG_IER => self.ier = value,
            REG_LCR => self.lcr = value,
            REG_MCR => self.mcr = value,
            _ => {}
        }
    }
}

/// Shared-lock fake: one lock batch per `with_lock`, direct access.
struct FakeHw {
    regs: Mutex<Regs>,
}

impl FakeHw {
    fn new() -> Self {
        FakeHw { regs: Mutex::new(Regs::new()) }
    }

    /// Panic-free lock access (a poisoned lock means a test already failed).
    fn lock(&self) -> std::sync::MutexGuard<'_, Regs> {
        self.regs.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl PxHw for FakeHw {
    fn with_lock<R>(&self, f: impl FnOnce(&mut dyn PxRegs) -> R) -> R {
        let mut regs = self.lock();
        f(&mut *regs)
    }
}

fn core() -> K3UartCore {
    K3UartCore::new()
}

fn ok_register() -> bool {
    true
}

fn fail_register() -> bool {
    false
}

// ---------------------------------------------------------------------------
// Activation lifecycle
// ---------------------------------------------------------------------------

#[test]
fn activation_writes_pxa_setup_and_reaches_ready() {
    let c = core();
    let hw = FakeHw::new();
    assert!(c.activate(&hw, ok_register).is_ok());
    assert_eq!(hw.lock().last_write(REG_LCR), Some(3), "LCR 8N1, DLAB off");
    assert_eq!(
        hw.lock().last_write(REG_IER),
        Some(IER_UUE as u32),
        "notifications masked, UUE kept"
    );
    assert_eq!(
        hw.lock().last_write(REG_MCR),
        Some(0x08),
        "MCR OUT2 only, AFE off"
    );
    assert!(c.is_ready());
    assert!(c.is_active());
}

#[test]
fn duplicate_activate_is_explicit_and_touches_nothing() {
    let c = core();
    let hw = FakeHw::new();
    assert!(c.activate(&hw, ok_register).is_ok());
    let log_len = hw.lock().log.len();
    assert_eq!(c.activate(&hw, ok_register), Err(ActivateError::AlreadyActive));
    assert_eq!(
        hw.lock().log.len(),
        log_len,
        "duplicate activate must not clear the active FIFO or touch hardware"
    );
}

#[test]
fn activation_resets_pre_activation_console_latch() {
    let c = core();
    let hw = FakeHw::new();
    // Pre-activation diagnostics poll the same LSR; a stale error bit gets
    // latched by the console path and must not survive activation.
    hw.lock().lsr_level = LSR_THRE as u32;
    hw.lock().lsr_errors = LSR_PE as u32;
    c.console_write(&hw, b"diag");
    // Activation drains the stale status and resets the byte stream.
    hw.lock().lsr_level = (LSR_THRE | LSR_TEMT) as u32;
    c.activate(&hw, ok_register).unwrap();
    let mut buf = [0u8; 8];
    hw.lock().rx_fifo = vec![b'g'];
    assert_eq!(c.receive_bytes(&hw, &mut buf), 1, "clean byte after activation dropped");
    assert_eq!(buf[0], b'g');
    assert_eq!(c.counts().parity_errors, 0);
}

#[test]
fn failed_irq_register_reports_stage_and_rolls_back() {
    let c = core();
    let hw = FakeHw::new();
    assert_eq!(c.activate(&hw, fail_register), Err(ActivateError::IrqRegister));
    assert!(!c.is_active(), "failed activation must not stay half-active");
    assert_eq!(
        hw.lock().last_write(REG_IER),
        Some(IER_UUE as u32),
        "notifications stay masked after rollback"
    );
    // A retry after the failed attempt is possible (only this attempt's
    // resources were rolled back).
    assert!(c.activate(&hw, ok_register).is_ok());
    assert!(c.is_ready());
}

// ---------------------------------------------------------------------------
// IER RMW discipline
// ---------------------------------------------------------------------------

#[test]
fn ier_rmw_preserves_uue_and_notification_masks() {
    let c = core();
    let hw = FakeHw::new();
    c.activate(&hw, ok_register).unwrap();
    c.update_ier(&hw, RX_GROUP, 0);
    assert_eq!(
        hw.lock().ier,
        (IER_UUE | RX_GROUP) as u32,
        "RX group armed over UUE"
    );
    c.update_ier(&hw, IER_THRI, 0);
    assert_eq!(hw.lock().ier, (IER_UUE | RX_GROUP | IER_THRI) as u32);
    c.update_ier(&hw, 0, RX_GROUP | IER_THRI);
    assert_eq!(hw.lock().ier, IER_UUE as u32, "both groups masked back");
    // Even an explicit UUE clear is refused: the unit-enable stays.
    c.update_ier(&hw, RX_GROUP, IER_UUE);
    assert_eq!(hw.lock().ier, (IER_UUE | RX_GROUP) as u32);
}

// ---------------------------------------------------------------------------
// ISR cause classification
// ---------------------------------------------------------------------------

fn arm_rx(c: &K3UartCore, hw: &FakeHw) {
    c.update_ier(hw, RX_GROUP, 0);
}

#[test]
fn rdi_cause_masks_rx_group_and_requests_wake() {
    let c = core();
    let hw = FakeHw::new();
    c.activate(&hw, ok_register).unwrap();
    arm_rx(&c, &hw);
    hw.lock().iir_sequence = vec![IIR_RDI as u32];
    hw.lock().rx_fifo = vec![b'a'];
    let report = c.isr(&hw);
    assert!(report.rx_wake);
    assert!(!report.tx_wake);
    assert_eq!(
        hw.lock().ier,
        IER_UUE as u32,
        "ISR must mask the RX group so the FIFO drains without an IRQ storm"
    );
}

#[test]
fn cti_and_no_int_dr_deliver_the_last_byte() {
    let c = core();
    let hw = FakeHw::new();
    c.activate(&hw, ok_register).unwrap();
    // CTI with a pending byte: the isolated byte arrived; RX work.
    arm_rx(&c, &hw);
    hw.lock().iir_sequence = vec![IIR_CTI as u32];
    hw.lock().rx_fifo = vec![b'x'];
    assert!(c.isr(&hw).rx_wake);
    // NO_INT with DR: no IIR cause but the FIFO holds receive data.
    c.update_ier(&hw, RX_GROUP, 0);
    hw.lock().iir_sequence.clear();
    assert!(c.isr(&hw).rx_wake);
    // NO_INT with a clean LSR (FIFO drained): no work, no wake.
    hw.lock().rx_fifo.clear();
    let report = c.isr(&hw);
    assert!(!report.rx_wake && !report.tx_wake);
}

#[test]
fn mixed_causes_are_all_handled_within_budget() {
    let c = core();
    let hw = FakeHw::new();
    c.activate(&hw, ok_register).unwrap();
    arm_rx(&c, &hw);
    // One batch: TX empty, then RX data, then the terminating NO_INT.
    hw.lock().rx_fifo = vec![b'a'];
    hw.lock().iir_sequence =
        vec![IIR_THRI as u32, IIR_RDI as u32, IIR_NO_INT as u32];
    let report = c.isr(&hw);
    assert!(report.tx_wake, "THRI cause dropped");
    assert!(report.rx_wake, "RDI cause dropped");
    assert_eq!(hw.lock().iir_reads, 3, "batch must stop at NO_INT");
    assert_eq!(hw.lock().ier, IER_UUE as u32, "both groups end masked");
}

#[test]
fn unknown_cause_is_bounded_and_closes_notifications() {
    let c = core();
    let hw = FakeHw::new();
    c.activate(&hw, ok_register).unwrap();
    arm_rx(&c, &hw);
    hw.lock().iir_sequence = vec![0x0e]; // unimplemented/reserved cause
    let report = c.isr(&hw);
    assert!(report.unknown);
    assert_eq!(hw.lock().iir_reads, 1, "unknown cause must stop the batch");
    assert_eq!(
        hw.lock().ier,
        IER_UUE as u32,
        "unknown cause closes UART notifications down to UUE"
    );
    assert_eq!(c.counts().unknown, 1);
}

// ---------------------------------------------------------------------------
// LSR read-clear latch discipline
// ---------------------------------------------------------------------------

#[test]
fn read_clear_head_errors_drop_the_errored_byte_once() {
    let c = core();
    let hw = FakeHw::new();
    c.activate(&hw, ok_register).unwrap();
    // Head byte has a parity error: the byte is dropped and classified once.
    hw.lock().rx_fifo = vec![b'x'];
    hw.lock().lsr_errors = LSR_PE as u32;
    let mut buf = [0u8; 8];
    assert_eq!(c.receive_bytes(&hw, &mut buf), 0, "errored byte must not deliver");
    assert_eq!(c.counts().parity_errors, 1);
    // A subsequent clean byte delivers (errors do not stick beyond one byte).
    hw.lock().rx_fifo = vec![b'a', b'b'];
    assert_eq!(c.receive_bytes(&hw, &mut buf), 2);
    assert_eq!(&buf[..2], b"ab");
}

#[test]
fn overrun_is_counted_independently_and_byte_still_delivers() {
    let c = core();
    let hw = FakeHw::new();
    c.activate(&hw, ok_register).unwrap();
    hw.lock().rx_fifo = vec![b'z'];
    hw.lock().lsr_errors = LSR_OE as u32;
    let mut buf = [0u8; 8];
    assert_eq!(c.receive_bytes(&hw, &mut buf), 1);
    assert_eq!(buf[0], b'z', "OE must not drop the delivered byte");
    assert_eq!(c.counts().overruns, 1);
    assert_eq!(c.counts().parity_errors + c.counts().framing_errors, 0);
}

#[test]
fn break_and_fifo_error_are_classified_separately() {
    let c = core();
    let hw = FakeHw::new();
    c.activate(&hw, ok_register).unwrap();
    // Break: byte dropped, break counter.
    hw.lock().rx_fifo = vec![0];
    hw.lock().lsr_errors = LSR_BI as u32;
    let mut buf = [0u8; 8];
    assert_eq!(c.receive_bytes(&hw, &mut buf), 0);
    assert_eq!(c.counts().break_errors, 1);
    // FIFO error aggregate: byte delivers, aggregate counter only.
    hw.lock().rx_fifo = vec![b'q'];
    hw.lock().lsr_errors = LSR_FIFOERR as u32;
    assert_eq!(c.receive_bytes(&hw, &mut buf), 1);
    assert_eq!(buf[0], b'q');
    assert_eq!(c.counts().fifo_errors, 1);
    // Framing error from a TX-path TEMT poll is latched and applied to the
    // next received byte.
    hw.lock().lsr_level = LSR_TEMT as u32;
    hw.lock().lsr_errors = LSR_FE as u32;
    assert!(c.transmitter_empty(&hw));
    hw.lock().rx_fifo = vec![b'!'];
    assert_eq!(c.receive_bytes(&hw, &mut buf), 0, "latched FE must drop the head byte");
    assert_eq!(c.counts().framing_errors, 1);
}

// ---------------------------------------------------------------------------
// TX path
// ---------------------------------------------------------------------------

#[test]
fn send_bytes_gates_on_thre_and_caps_at_64() {
    let c = core();
    let hw = FakeHw::new();
    c.activate(&hw, ok_register).unwrap();
    // FIFO full (THRE clear): nothing is written.
    hw.lock().lsr_level = 0;
    assert_eq!(c.send_bytes(&hw, b"abc"), 0);
    assert!(hw.lock().reads_at(REG_RBR_THR) == 0);
    // THRE set: up to 64 bytes per call.
    hw.lock().lsr_level = (LSR_THRE | LSR_TEMT) as u32;
    let payload = [b'k'; 100];
    assert_eq!(c.send_bytes(&hw, &payload), 64, "per-call hardware budget");
    assert_eq!(c.send_bytes(&hw, &payload[64..]), 36);
    assert!(c.transmitter_empty(&hw));
}

#[test]
fn rtoie_errata_order_surrounds_the_rbr_batch() {
    let c = core();
    let hw = FakeHw::new();
    c.activate(&hw, ok_register).unwrap();
    arm_rx(&c, &hw); // cache = UUE | RX group (includes RTOIE)
    hw.lock().rx_fifo = vec![b'1', b'2'];
    let mut buf = [0u8; 8];
    assert_eq!(c.receive_bytes(&hw, &mut buf), 2);

    // The IER writes around the batch: RTOIE off before the first RBR read,
    // restored after the last, and the final cache keeps RTOIE armed.
    let first_rbr = hw.lock().log.iter().position(|(w, o, _)| !w && *o == REG_RBR_THR);
    let ier_clear_idx = hw
        .lock()
        .log
        .iter()
        .position(|(w, o, v)| *w && *o == REG_IER && *v == (IER_UUE | RX_GROUP & !IER_RTOIE) as u32);
    assert!(first_rbr.is_some() && ier_clear_idx.is_some());
    assert!(ier_clear_idx.unwrap() < first_rbr.unwrap(), "RTOIE must clear before RBR");
    assert_eq!(
        hw.lock().ier,
        (IER_UUE | RX_GROUP) as u32,
        "RTOIE restored per request after the batch"
    );
    assert_eq!(&buf[..2], b"12");
}

// ---------------------------------------------------------------------------
// Register access discipline
// ---------------------------------------------------------------------------

#[test]
fn all_accesses_use_pxa_stride4_offsets_only() {
    let c = core();
    let hw = FakeHw::new();
    c.activate(&hw, ok_register).unwrap();
    arm_rx(&c, &hw);
    hw.lock().iir_sequence = vec![IIR_RLS as u32];
    hw.lock().lsr_errors = LSR_PE as u32;
    hw.lock().rx_fifo = vec![b'e'];
    let _ = c.isr(&hw);
    let mut buf = [0u8; 4];
    let _ = c.receive_bytes(&hw, &mut buf);
    let _ = c.transmitter_empty(&hw);
    for (write, offset, _) in &hw.lock().log {
        let offset = *offset;
        assert!(
            matches!(offset, 0x00 | 0x04 | 0x08 | 0x0c | 0x10 | 0x14 | 0x18),
            "access outside the PXA stride-4 register set: {} at {:#x}",
            if *write { "write" } else { "read" },
            offset
        );
    }
}

#[test]
fn rls_cause_latches_errors_without_reading_rbr() {
    let c = core();
    let hw = FakeHw::new();
    c.activate(&hw, ok_register).unwrap();
    arm_rx(&c, &hw);
    hw.lock().iir_sequence = vec![IIR_RLS as u32];
    hw.lock().lsr_errors = LSR_FE as u32;
    hw.lock().rx_fifo = vec![b'f'];
    assert!(c.isr(&hw).rx_wake);
    assert_eq!(
        hw.lock().reads_at(REG_RBR_THR),
        0,
        "ISR must never read RBR"
    );
    // The task-side read consumes the latched FE with the byte.
    let mut buf = [0u8; 4];
    assert_eq!(c.receive_bytes(&hw, &mut buf), 0);
    assert_eq!(c.counts().framing_errors, 1);
    // MSR read clears the modem-status cause.
    hw.lock().iir_sequence = vec![0x0]; // MSI
    let report = c.isr(&hw);
    assert!(!report.rx_wake && !report.tx_wake && !report.unknown);
    assert!(hw.lock().reads_at(REG_MSR) >= 1, "MSI cause must clear MSR");
}

// ---------------------------------------------------------------------------
// Rework T2-R1: reference-aligned activation sequence
// ---------------------------------------------------------------------------

#[test]
fn activation_fcr_sequence_matches_reference_startup() {
    let c = core();
    let hw = FakeHw::new();
    assert!(c.activate(&hw, ok_register).is_ok());
    assert_eq!(
        hw.lock().writes_at(REG_FCR),
        vec![0x01, 0x07, 0x00, 0x41],
        "reference three-step startup (EN, EN|clear both, off) then the runtime FCR"
    );
    let final_fcr = hw.lock().last_write(REG_FCR).unwrap();
    assert_eq!(final_fcr & 0x06, 0, "the runtime FCR carries no clear bits");
    assert_eq!(final_fcr & 0x40, 0x40, "fixed 115200 runtime trigger is PXAR8");
}

#[test]
fn tx_drains_before_any_fifo_clear_write() {
    let c = core();
    let hw = FakeHw::new();
    hw.lock().lsr_level = (LSR_THRE | LSR_TEMT) as u32;
    // The first 3 LSR polls show TEMT=0; the drain loop must keep polling
    // until TEMT is observed before any clear-FIFO FCR write happens.
    hw.lock().temt_after_reads = 3;
    assert!(c.activate(&hw, ok_register).is_ok());
    let first_fcr = hw
        .lock()
        .log
        .iter()
        .position(|(w, o, _)| *w && *o == REG_FCR)
        .expect("activation must write FCR");
    let lsr_reads_before = hw.lock().log[..first_fcr]
        .iter()
        .filter(|(w, o, _)| !w && *o == REG_LSR)
        .count();
    assert!(
        lsr_reads_before > 3,
        "the TEMT=1 LSR observation must precede the first clear-FIFO write"
    );
}

#[test]
fn undrained_tx_fails_with_txdrain_and_writes_no_fcr() {
    let c = core();
    let hw = FakeHw::new();
    hw.lock().lsr_level = LSR_THRE as u32; // TEMT never sets
    assert_eq!(
        c.activate(&hw, ok_register),
        Err(ActivateError::TxDrain),
        "a transmitter that never drains must fail activation"
    );
    assert!(
        hw.lock().writes_at(REG_FCR).is_empty(),
        "no FCR write may happen on the TxDrain path"
    );
    assert!(!c.is_active(), "TxDrain rolls back to idle");
}

#[test]
fn stale_rbr_bytes_are_drained_and_never_enter_the_stream() {
    let c = core();
    let hw = FakeHw::new();
    hw.lock().lsr_level = (LSR_THRE | LSR_TEMT) as u32;
    hw.lock().rx_fifo = vec![0xa1, 0xa2, 0xa3];
    assert!(c.activate(&hw, ok_register).is_ok());
    assert!(
        hw.lock().rx_fifo.is_empty(),
        "stale bytes are read out; DR=0 after activation"
    );
    let mut buf = [0u8; 8];
    assert_eq!(
        c.receive_bytes(&hw, &mut buf),
        0,
        "drained stale bytes belong to no business stream"
    );
    // A byte arriving after activation delivers normally.
    hw.lock().rx_fifo = vec![b'g'];
    assert_eq!(c.receive_bytes(&hw, &mut buf), 1);
    assert_eq!(buf[0], b'g');
}

// ---------------------------------------------------------------------------
// Rework T2-R2: one-shot read-only fault snapshot
// ---------------------------------------------------------------------------

#[test]
fn diag_snapshot_reads_once_and_leaves_state_untouched() {
    let c = core();
    let hw = FakeHw::new();
    c.activate(&hw, ok_register).unwrap();
    arm_rx(&c, &hw);
    hw.lock().iir_sequence = vec![IIR_RDI as u32];
    hw.lock().lsr_errors = LSR_PE as u32;
    hw.lock().rx_fifo = vec![b'r'];
    let ier_before = hw.lock().ier;
    let log_start = hw.lock().log.len();
    let snap = c.diag_snapshot(&hw);
    // Window stats are taken in one short lock scope: holding the guard
    // across later hardware calls would deadlock the fake.
    let (ier_n, iir_n, lsr_n, writes_n) = {
        let regs = hw.lock();
        let window = &regs.log[log_start..];
        (
            window.iter().filter(|(_, o, _)| *o == REG_IER).count(),
            window.iter().filter(|(_, o, _)| *o == REG_IIR).count(),
            window.iter().filter(|(_, o, _)| *o == REG_LSR).count(),
            window.iter().filter(|(w, _, _)| *w).count(),
        )
    };
    assert_eq!(writes_n, 0, "the snapshot is strictly read-only");
    assert_eq!(ier_n, 1, "single IER readback");
    assert_eq!(iir_n, 1, "single IIR read");
    assert_eq!(lsr_n, 1, "single LSR read");
    assert_eq!(snap.ier_readback as u32, ier_before);
    assert_eq!(snap.ier_cache, (IER_UUE | RX_GROUP) as u8);
    assert_eq!(snap.iir, IIR_RDI as u8);
    assert!(snap.lsr_dr, "DR must reflect the pending byte");
    assert_eq!(snap.counts.irq_count, 0);
    // The snapshot must not latch head errors or classify: the pending byte
    // still delivers afterwards and no error counter moved.
    let mut buf = [0u8; 4];
    assert_eq!(
        c.receive_bytes(&hw, &mut buf),
        1,
        "the snapshot must not latch or drop the pending byte"
    );
    assert_eq!(buf[0], b'r');
    assert_eq!(c.counts().parity_errors, 0, "the snapshot must not classify");
}

// ---------------------------------------------------------------------------
// Rework T2-R4: raw line completion predicate (CR / LF / CRLF + 512 cap)
// ---------------------------------------------------------------------------

#[test]
fn raw_line_completes_on_lf_cr_and_crlf() {
    assert!(raw_line_complete(b"ms17-a\n"), "LF-terminated line");
    assert!(raw_line_complete(b"ms17-a\r"), "CR-terminated line (Enter as CR)");
    assert!(raw_line_complete(b"ms17-a\r\n"), "CRLF-terminated line");
}

#[test]
fn partial_raw_line_is_incomplete() {
    assert!(!raw_line_complete(b""), "empty accumulation");
    assert!(!raw_line_complete(b"ms17-a"), "no terminator yet");
    assert!(
        !raw_line_complete(b"ms17-a\rX"),
        "only the LAST byte may terminate the line"
    );
}

#[test]
fn raw_line_completes_at_the_cap_without_terminator() {
    let full = [b'x'; RAW_LINE_LIMIT];
    assert!(raw_line_complete(&full), "the 512-byte cap completes the line");
    assert!(
        !raw_line_complete(&full[..RAW_LINE_LIMIT - 1]),
        "below the cap a terminator is still required"
    );
}

// ---------------------------------------------------------------------------
// MS17 Iteration 001 (task T4): copier-side abstract IER mapping
// ---------------------------------------------------------------------------

#[test]
fn copier_ier_map_restores_the_full_rx_group() {
    use k3_uart_core::{map_copier_ier, IER_RDI, IER_RLSI, IER_RTOIE, IER_THRI, RX_GROUP};

    assert_eq!(RX_GROUP, IER_RDI | IER_RLSI | IER_RTOIE, "group shape");

    // Arming the abstract RX notification (crate DATA_READY = bit 0) must
    // restore the full RX group: the ISR masks RDI|RLSI|RTOIE on every wake,
    // so re-arming only RDI would leave RTOIE masked and strand short lines
    // below the 8-byte trigger (no RDI, no timeout IRQ).
    let (set, clear) = map_copier_ier(IER_RDI, 0);
    assert_eq!(set, RX_GROUP, "DATA_READY arm must restore RDI|RLSI|RTOIE");
    assert_eq!(clear, 0);

    // THRI stays a 1:1 mapping for the TX side.
    let (set, clear) = map_copier_ier(IER_THRI, 0);
    assert_eq!(set, IER_THRI, "THRE arm maps 1:1 to THRI");
    assert_eq!(clear, 0);

    // Clearing the abstract RX notification masks the whole group.
    let (set, clear) = map_copier_ier(0, IER_RDI);
    assert_eq!(set, 0);
    assert_eq!(clear, RX_GROUP, "DATA_READY clear masks the full RX group");
}
