// SPDX-License-Identifier: MIT OR Apache-2.0

//! UART 16550 ISR handler with AtomicWaker pattern.
//!
//! Provides a minimal ISR handler that reads the ISR register, disables the
//! corresponding interrupt, and wakes the appropriate async waker.

use core::{
    ptr::NonNull,
    sync::atomic::{AtomicU64, Ordering},
};

use embassy_sync::waitqueue::AtomicWaker;

use crate::spec::registers::{ISR, InterruptType, LSR, offsets};

/// RX data ready waker — woken when data arrives.
pub static RX_WAKER: AtomicWaker = AtomicWaker::new();

/// TX buffer empty waker — woken when THR is empty.
pub static TX_WAKER: AtomicWaker = AtomicWaker::new();

/// Drain complete waker — woken when transmitter is fully empty (for `tcdrain`).
pub static DRAIN_WAKER: AtomicWaker = AtomicWaker::new();

/// IRQ invocation counter — useful for benchmarking and diagnostics.
pub static IRQ_COUNT: AtomicU64 = AtomicU64::new(0);

/// Returns the total number of ISR invocations since boot.
pub fn irq_count() -> u64 {
    IRQ_COUNT.load(Ordering::Relaxed)
}

/// Lock-free ISR register access — safe in ISR context.
pub(crate) struct IsrRegisters {
    base: NonNull<u8>,
}

impl IsrRegisters {
    /// Create a new ISR register accessor.
    ///
    /// # Safety
    ///
    /// `base` must be a valid UART MMIO base address.
    pub(crate) const unsafe fn new(base: NonNull<u8>) -> Self {
        Self { base }
    }

    /// Read the ISR register (offset 2, stride 1).
    ///
    /// # Safety
    ///
    /// The base address must be valid and the ISR register must be mapped.
    pub(crate) unsafe fn read_isr(&self) -> ISR {
        // SAFETY: Caller guarantees base is valid and ISR is mapped.
        unsafe {
            let ptr = self.base.as_ptr().add(offsets::ISR);
            ISR::from_bits_retain(ptr.read_volatile())
        }
    }

    /// Read the LSR register (offset 5, stride 1).
    ///
    /// # Safety
    ///
    /// The base address must be valid and the LSR register must be mapped.
    pub(crate) unsafe fn read_lsr(&self) -> LSR {
        // SAFETY: Caller guarantees base is valid and LSR is mapped.
        unsafe {
            let ptr = self.base.as_ptr().add(offsets::LSR);
            LSR::from_bits_retain(ptr.read_volatile())
        }
    }
}

/// UART ISR handler — called from IRQ context.
///
/// This handler is minimal (ISR principle):
/// 1. Read ISR to determine interrupt type
/// 2. Disable the corresponding interrupt via callbacks
/// 3. Wake the appropriate waker
/// 4. Return immediately
pub fn uart_isr_handler(_irq: usize, base: NonNull<u8>, fn_disable_rx: fn(), fn_disable_tx: fn()) {
    IRQ_COUNT.fetch_add(1, Ordering::Relaxed);
    // SAFETY: Called from ISR context with a valid base address.
    unsafe {
        let regs = IsrRegisters::new(base);
        let isr = regs.read_isr();

        match isr.interrupt_type() {
            Some(InterruptType::ReceivedDataReady) | Some(InterruptType::ReceptionTimeout) => {
                fn_disable_rx();
                RX_WAKER.wake();
            }
            Some(InterruptType::TransmitterHoldingRegisterEmpty) => {
                fn_disable_tx();
                TX_WAKER.wake();
                if regs.read_lsr().contains(LSR::TRANSMITTER_EMPTY) {
                    DRAIN_WAKER.wake();
                }
            }
            _ => {}
        }
    }
}
