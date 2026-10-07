// kernel/src/drivers/k3_uart.rs

//! K3 PXA UART raw port (MS17 Iteration 000, task T2): production binding of
//! the host-tested core ([`k3_uart_core`]) to the UART0 MMIO window, the
//! shared hardware guard (platform `console::with_uart_hardware`) and the
//! AIA source-42 route.
//!
//! Contract boundaries implemented here:
//! - one UART0 instance (`0xd4017000`, stride 4, 64-byte FIFO, 115200 8N1);
//!   the raw qualification payload is its only consumer — no generic rings,
//!   copiers or tty in this mode;
//! - the ISR (`k3_uart42_irq_handler`) runs the core's bounded classification
//!   batch inside the shared guard, then wakes the uart_16550 RX/TX wakers
//!   from the report — never reads RBR, writes THR, prints or allocates;
//! - the raw wait protocol is check → register waker → arm → recheck with no
//!   busy wait; the caller owns the outer native deadline;
//! - activation goes through [`K3UartCore::activate`] with the axhal IRQ
//!   registration closure (AIA route 42, opt-in `uart-irq`).

use core::future::poll_fn;
use core::task::Poll;

use alloc::vec::Vec;
use axplat_riscv64_k3::aia::{
    AiaBackend, HardwareBackend, APLIC_SETIP_BASE, bitmap_bit, bitmap_word, read_eip_snapshot,
};
use uart_16550::async_::driver::UartPort as UartPortTrait;
use uart_16550::async_::isr::{RX_WAKER, TX_WAKER};
use uart_16550::spec::registers::IER;

use super::k3_uart_core::{
    ActivateError, K3UartCore, PxHw, PxRegs, IER_THRI, RX_GROUP, RX_POLL_BUDGET, HW_CALL_LIMIT,
};

/// K3 UART0 physical base (board DT + MS09 Iteration 001 adjudication).
pub const UART0_PADDR: u64 = 0xd401_7000;

// ---------------------------------------------------------------------------
// Production hardware binding
// ---------------------------------------------------------------------------

/// Stride-4 u32 register window at the UART0 virtual address.
struct K3MmioRegs {
    base_va: usize,
}

impl PxRegs for K3MmioRegs {
    fn read_u32(&mut self, offset: usize) -> u32 {
        // SAFETY: UART0 is identity-mapped via `devices.mmio-ranges`
        // (0xd4017000/0x1000) and left in the bootloader handoff state; all
        // accesses run inside the shared hardware guard.
        unsafe { ((self.base_va + offset) as *const u32).read_volatile() }
    }

    fn write_u32(&mut self, offset: usize, value: u32) {
        // SAFETY: same window contract as `read_u32`.
        unsafe { ((self.base_va + offset) as *mut u32).write_volatile(value) }
    }
}

/// Production [`PxHw`]: every batch holds the platform's shared UART hardware
/// guard (IRQ-save), so early/fault console writes and port accesses never
/// interleave on the single hardware instance.
pub struct K3MmioHw {
    base_paddr: u64,
}

impl PxHw for K3MmioHw {
    fn with_lock<R>(&self, f: impl FnOnce(&mut dyn PxRegs) -> R) -> R {
        axplat_riscv64_k3::console::with_uart_hardware(|| {
            let mut regs = K3MmioRegs {
                base_va: self.base_paddr as usize + axconfig::plat::PHYS_VIRT_OFFSET,
            };
            f(&mut regs)
        })
    }
}

// ---------------------------------------------------------------------------
// Port singleton
// ---------------------------------------------------------------------------

/// The raw K3 port: host-tested core plus the production MMIO binding.
pub struct K3UartPort {
    pub core: K3UartCore,
    pub hw: K3MmioHw,
}

static PORT: K3UartPort = K3UartPort {
    core: K3UartCore::new(),
    hw: K3MmioHw { base_paddr: UART0_PADDR },
};

/// The port singleton (the qualification payload's only consumer).
pub fn port() -> &'static K3UartPort {
    &PORT
}

/// Zero-argument device handler for AIA source 42 (registered via
/// `axhal::irq::register`): classify inside the guard, then wake.
fn k3_uart42_irq_handler() {
    let report = PORT.core.isr(&PORT.hw);
    if report.rx_wake {
        RX_WAKER.wake();
    }
    if report.tx_wake {
        TX_WAKER.wake();
    }
}

/// Activates the port: prepare registers, register the ISR on AIA route 42
/// (the opt-in `uart-irq` route), then publish readiness. Notifications stay
/// masked until a task arms them.
pub fn activate_port() -> Result<(), ActivateError> {
    PORT.core
        .activate(&PORT.hw, || axhal::irq::register(axconfig::devices::UART_IRQ, k3_uart42_irq_handler))
}

/// Latch-aware diagnostics line for the shared console (post-activation safe;
/// the console polls keep the core's read-clear discipline).
pub fn diag_line(line: &str) {
    PORT.core.console_write(&PORT.hw, line.as_bytes());
    PORT.core.console_write(&PORT.hw, b"\n");
}

// ---------------------------------------------------------------------------
// One-shot AIA fault observation (rework T2-R2)
// ---------------------------------------------------------------------------

/// One-shot fault-snapshot observation: the APLIC pending bit of source 42
/// and the local EIP word covering identity 42 (EIP0 spans identities 0–63).
/// Read-only; the payload calls it at most once per fault, right before
/// parking.
pub fn aia_pending42_and_eip() -> (bool, u64) {
    let source = axconfig::devices::UART_IRQ as usize;
    let mut b = HardwareBackend;
    let pending =
        b.aplic_read(APLIC_SETIP_BASE + 4 * bitmap_word(source) as u32) & bitmap_bit(source) != 0;
    let eip = read_eip_snapshot(&mut b);
    (pending, eip[0])
}

// ---------------------------------------------------------------------------
// UartPort trait binding (copier-ready seam; MS17 Iteration 001 wires the
// data-plane copiers over it)
// ---------------------------------------------------------------------------

/// K3 stall-wait interval for the copier policy seam: the native one-shot
/// timer hook the TX copier parks on when transmission cannot progress
/// (spin strategies are disabled for this port).
const K3_TX_TIMER_WAIT_MS: u64 = 1;

impl UartPortTrait for K3UartPort {
    // MS17 Iteration 001 T3 (copier policy seam): the K3 PXA UART has no D1
    // THRE errata, so both D1 spin fallbacks are disabled; the copier waits
    // on the native 1 ms timer hook instead of busy-spinning.
    const SLOW_POLL_LIMIT: u32 = 0;
    const TEMT_POLL_LIMIT: u32 = 0;

    fn receive_bytes(&self, buf: &mut [u8]) -> usize {
        self.core.receive_bytes(&self.hw, buf)
    }

    fn send_bytes(&self, buf: &[u8]) -> usize {
        self.core.send_bytes(&self.hw, buf)
    }

    fn transmitter_empty(&self) -> bool {
        self.core.transmitter_empty(&self.hw)
    }

    fn update_ier(&self, set: IER, clear: IER) {
        // The copier's abstract RX arm maps to the platform's full RX group:
        // the ISR masks RDI|RLSI|RTOIE on every wake, so re-arming only RDI
        // would strand short lines below the 8-byte trigger (no RTOIE).
        let (set, clear) = super::k3_uart_core::map_copier_ier(set.bits(), clear.bits());
        self.core.update_ier(&self.hw, set, clear);
    }

    fn tx_stall_wait(&self) -> impl core::future::Future<Output = ()> + Send {
        axtask::future::sleep(core::time::Duration::from_millis(K3_TX_TIMER_WAIT_MS))
    }
}

// ---------------------------------------------------------------------------
// Raw wait protocol (task context; the caller owns the outer deadline)
// ---------------------------------------------------------------------------

/// Raw RX accumulation: check → register `RX_WAKER` → arm the RX group →
/// recheck, no busy wait. The caller owns the accumulation: a deadline
/// cancellation drops the future but leaves the received bytes visible to
/// the caller (rework T2-R4). `pred` decides when the stage is complete.
pub async fn raw_rx_stage_into(
    port: &'static K3UartPort,
    acc: &mut Vec<u8>,
    mut pred: impl FnMut(&[u8]) -> bool,
) {
    let mut chunk = [0u8; HW_CALL_LIMIT];
    poll_fn(|cx| {
        let mut polled = 0usize;
        while polled < RX_POLL_BUDGET {
            let n = port.core.receive_bytes(&port.hw, &mut chunk);
            if n == 0 {
                break;
            }
            acc.extend_from_slice(&chunk[..n]);
            polled += n;
        }
        if pred(acc) {
            return Poll::Ready(());
        }
        RX_WAKER.register(cx.waker());
        port.core.update_ier(&port.hw, RX_GROUP, 0);
        // Recheck after arming: the last byte may have arrived before the
        // notification was enabled (1-byte threshold, Errata-20 window).
        let n = port.core.receive_bytes(&port.hw, &mut chunk);
        if n > 0 {
            acc.extend_from_slice(&chunk[..n]);
            if pred(acc) {
                return Poll::Ready(());
            }
        }
        Poll::Pending
    })
    .await
}

/// Owning variant of [`raw_rx_stage_into`]: the accumulated bytes are
/// returned on completion (unchanged external semantics).
pub async fn raw_rx_stage(port: &'static K3UartPort, pred: impl FnMut(&[u8]) -> bool) -> Vec<u8> {
    let mut acc = Vec::new();
    raw_rx_stage_into(port, &mut acc, pred).await;
    acc
}

/// Raw TX send: push until the hardware accepts everything; on a full FIFO
/// register `TX_WAKER`, arm `THRI`, attempt the final send once, then pend —
/// never busy-waits.
pub async fn raw_tx_send(port: &'static K3UartPort, data: &[u8]) {
    let mut sent = 0usize;
    poll_fn(|cx| {
        sent += port.core.send_bytes(&port.hw, &data[sent..]);
        if sent >= data.len() {
            return Poll::Ready(());
        }
        TX_WAKER.register(cx.waker());
        port.core.update_ier(&port.hw, IER_THRI, 0);
        // Final send attempt after arming: space may already be free.
        sent += port.core.send_bytes(&port.hw, &data[sent..]);
        if sent >= data.len() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    })
    .await
}

/// Reads exactly one raw byte (the isolated-byte stage).
pub async fn raw_rx_one_byte(port: &'static K3UartPort) -> Vec<u8> {
    raw_rx_stage(port, |acc| !acc.is_empty()).await
}
