// kernel/src/drivers/serial/isr.rs

//! UART Interrupt Service Routine (ISR) context and handler
//!
//! This module implements the ISR following ADR-008 "极简原则":
//! ISR only reads IIR, disables interrupt, wakes waker, and exits.
//! Data manipulation is deferred to copier tasks (safe context).

use uart_16550::spec::registers::InterruptType;
use embassy_sync::waitqueue::AtomicWaker;
use alloc::sync::Arc;
use spin::Mutex;

use super::uart16550_impl::Uart16550Async;
use super::async_uart::AsyncUart;

/// ISR context shared between ISR and copier tasks
///
/// Contains UART hardware access and ISR-safe wakers for
/// RX and TX copier tasks.
pub struct IsrContext {
    pub uart: Mutex<Uart16550Async>,
    rx_waker: AtomicWaker,
    tx_waker: AtomicWaker,
}

impl IsrContext {
    /// Create a new ISR context
    ///
    /// The context is wrapped in Arc for sharing between ISR
    /// and copier tasks.
    pub fn new(uart: Uart16550Async) -> Arc<Self> {
        Arc::new(Self {
            uart: Mutex::new(uart),
            rx_waker: AtomicWaker::new(),
            tx_waker: AtomicWaker::new(),
        })
    }
}

/// UART Interrupt Service Routine
///
/// ISR follows ADR-008 "极简原则":
/// 1. Read IIR → identify interrupt type
/// 2. Disable triggered interrupt (prevent re-entry)
/// 3. Wake corresponding waker
/// 4. Exit immediately
///
/// Data搬运推迟到 copier 任务上下文（安全）。
pub fn uart_isr_handler(ctx: &Arc<IsrContext>) {
    // SAFETY: spin::Mutex disables interrupts (ISR-safe, never sleeps).
    // Critical section is minimal: read IIR, disable interrupt, wake waker.
    let mut uart = ctx.uart.lock();

    // 1. Read IIR to identify interrupt type
    let intr_type = uart.intr_identification();

    match intr_type {
        Some(InterruptType::ReceivedDataReady) => {
            // 2. Disable RX interrupt (prevent re-entry)
            uart.disable_rx_intr();
            // 3. Wake RX waker (ISR-safe AtomicWaker)
            ctx.rx_waker.wake();
        }
        Some(InterruptType::TransmitterHoldingRegisterEmpty) => {
            // 2. Disable TX interrupt (prevent re-entry)
            uart.disable_tx_intr();
            // 3. Wake TX waker (ISR-safe AtomicWaker)
            ctx.tx_waker.wake();
        }
        // ModemStatus and LineStatus not used in M3 scope (no hardware flow control)
        _ => {}
    }
    // 4. Exit immediately (data搬运在 copier 任务)
}