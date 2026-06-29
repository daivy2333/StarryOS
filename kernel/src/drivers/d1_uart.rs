// kernel/src/drivers/d1_uart.rs

//! D1 (Allwinner D1 / C906) DesignWare APB UART 32-bit MMIO port implementation.
//!
//! Implements `uart_16550::async_::driver::UartPort` using direct 32-bit volatile
//! MMIO at register stride 4, matching the DW APB UART access model on Lichee RV Dock.
//! The external `uart_16550::Uart16550<MmioBackend>` uses 8-bit byte access (stride 1),
//! which is unsafe on D1 hardware.

use core::ptr::NonNull;
use core::sync::atomic::{AtomicU8, Ordering};

use uart_16550::{
    async_::driver::UartPort,
    spec::registers::IER,
};

// DW APB UART register offsets (in units of stride):
// Physical byte offset = offset * stride
const UART_RBR_THR: usize = 0;
const UART_IER: usize = 1;
const UART_IIR: usize = 2;
const UART_LSR: usize = 5;

// LSR bit definitions (same as NS16550, but accessed as u32)
const LSR_DR: u32 = 1 << 0;
const LSR_THRE: u32 = 1 << 5;
const LSR_TEMT: u32 = 1 << 6;

// IIR interrupt IDs (bits 3:1)
const IIR_RX_DATA: u32 = 0x04;
const IIR_TX_EMPTY: u32 = 0x02;

/// D1 UART port wraps raw MMIO base pointer with stride-aware 32-bit access.
pub struct ArceOsD1UartPort {
    base: NonNull<u8>,
    stride: u8,
    ier_cache: AtomicU8,
}

// SAFETY: The base pointer is immutable after construction; the ier_cache uses
// atomic operations; hardware MMIO is accessed via volatile operations which
// are safe to share across threads in a single-core context.
unsafe impl Send for ArceOsD1UartPort {}
unsafe impl Sync for ArceOsD1UartPort {}

impl ArceOsD1UartPort {
    pub unsafe fn new(base: NonNull<u8>, stride: u8) -> Self {
        Self {
            base,
            stride,
            ier_cache: AtomicU8::new(0),
        }
    }

    #[inline(always)]
    fn read_reg(&self, offset: usize) -> u32 {
        unsafe {
            self.base
                .as_ptr()
                .add(offset * self.stride as usize)
                .cast::<u32>()
                .read_volatile()
        }
    }

    #[inline(always)]
    fn write_reg(&self, offset: usize, val: u32) {
        unsafe {
            self.base
                .as_ptr()
                .add(offset * self.stride as usize)
                .cast::<u32>()
                .write_volatile(val);
        }
    }

    /// Read IIR register (offset 2), extract interrupt ID from bits 3:1.
    #[inline(always)]
    pub fn read_iir(&self) -> u32 {
        self.read_reg(UART_IIR) & 0x0e
    }

    /// Read LSR register, clear line/modem interrupt sources.
    #[inline(always)]
    pub fn read_lsr_clear(&self) -> u32 {
        self.read_reg(UART_LSR)
    }
}

impl UartPort for ArceOsD1UartPort {
    fn receive_bytes(&self, buf: &mut [u8]) -> usize {
        for (i, slot) in buf.iter_mut().enumerate() {
            let lsr = self.read_reg(UART_LSR);
            if lsr & LSR_DR == 0 {
                return i;
            }
            let rbr = self.read_reg(UART_RBR_THR);
            *slot = rbr as u8;
        }
        buf.len()
    }

    fn send_bytes(&self, buf: &[u8]) -> usize {
        for (i, &byte) in buf.iter().enumerate() {
            let lsr = self.read_reg(UART_LSR);
            if lsr & LSR_THRE == 0 {
                return i;
            }
            self.write_reg(UART_RBR_THR, byte as u32);
        }
        buf.len()
    }

    fn transmitter_empty(&self) -> bool {
        let lsr = self.read_reg(UART_LSR);
        lsr & LSR_TEMT != 0
    }

    fn update_ier(&self, set: IER, clear: IER) {
        let mut val = self.ier_cache.load(Ordering::Relaxed);
        val |= set.bits();
        val &= !clear.bits();
        self.ier_cache.store(val, Ordering::Relaxed);
        self.write_reg(UART_IER, val as u32);
    }
}

/// D1 UART ISR handler — called from IRQ context for D1 benchmark modes.
///
/// Reads IIR via 32-bit MMIO at stride 4, dispatches to RX/TX wake paths.
/// Uses the same global wakers from `uart_16550::async_::isr`.
pub fn d1_uart_isr_handler(
    _irq: usize,
    port: &'static ArceOsD1UartPort,
    fn_disable_rx: fn(),
    fn_disable_tx: fn(),
) {
    use uart_16550::async_::isr;

    isr::IRQ_COUNT.fetch_add(1, Ordering::Relaxed);

    let iir = port.read_iir();

    match iir {
        IIR_RX_DATA => {
            fn_disable_rx();
            isr::RX_WAKER.wake();
        }
        IIR_TX_EMPTY => {
            fn_disable_tx();
            isr::TX_WAKER.wake();
            if port.transmitter_empty() {
                isr::DRAIN_WAKER.wake();
            }
        }
        _ => {
            // Clear line/modem interrupt sources by reading LSR
            let _ = port.read_lsr_clear();
        }
    }
}
