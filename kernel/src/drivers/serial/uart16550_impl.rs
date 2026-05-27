// kernel/src/drivers/serial/uart16550_impl.rs

use uart_16550::{Uart16550, Config};
use uart_16550::backend::MmioBackend;
use uart_16550::spec::registers::{InterruptType, IER, LSR, offsets};
use super::async_uart::AsyncUart;
use core::ptr::NonNull;

/// AsyncUart implementation for 16550 UART hardware
///
/// This implementation wraps uart_16550 v0.6.0 and provides
/// the AsyncUart trait interface. It also maintains MMIO address
/// for direct IER register manipulation (runtime interrupt control).
pub struct Uart16550Async {
    inner: Uart16550<MmioBackend>,
    mmio_addr: NonNull<u8>,  // For direct IER write
    stride: u8,
}

impl Uart16550Async {
    /// Create a new Uart16550Async instance
    ///
    /// # Safety
    ///
    /// The caller must ensure that the MMIO address is valid and
    /// that exclusive access to the hardware is maintained.
    pub unsafe fn new(mmio_addr: usize, stride: u8) -> Self {
        let ptr = core::ptr::with_exposed_provenance_mut::<u8>(mmio_addr);
        let addr = NonNull::new(ptr).expect("invalid MMIO address");
        // SAFETY: Caller guarantees valid MMIO address and exclusive access
        let uart = unsafe { Uart16550::new_mmio(addr, stride).expect("failed to create UART") };

        Self {
            inner: uart,
            mmio_addr: addr,
            stride,
        }
    }

    /// Initialize the UART hardware
    pub fn init(&mut self, config: Config) {
        self.inner.init(config).expect("UART init failed");
    }

    /// Direct MMIO write to IER register
    ///
    /// # Safety
    ///
    /// This performs direct MMIO write. Must be called with proper
    /// hardware access guarantees.
    unsafe fn write_ier(&self, value: u8) {
        let ier_offset = offsets::IER as usize * self.stride as usize;
        // SAFETY: Caller guarantees proper hardware access
        let ier_addr = unsafe { self.mmio_addr.as_ptr().add(ier_offset) };
        // SAFETY: Caller guarantees proper hardware access
        unsafe { core::ptr::write_volatile(ier_addr, value) };
    }

    /// Direct MMIO read from IER register
    ///
    /// # Safety
    ///
    /// This performs direct MMIO read.
    unsafe fn read_ier(&self) -> u8 {
        let ier_offset = offsets::IER as usize * self.stride as usize;
        // SAFETY: Caller guarantees proper hardware access
        let ier_addr = unsafe { self.mmio_addr.as_ptr().add(ier_offset) };
        // SAFETY: Caller guarantees proper hardware access
        unsafe { core::ptr::read_volatile(ier_addr) }
    }
}

impl AsyncUart for Uart16550Async {
    fn try_read(&mut self, buf: &mut [u8]) -> usize {
        self.inner.receive_bytes(buf)
    }

    fn try_write(&mut self, data: &[u8]) -> usize {
        self.inner.send_bytes(data)
    }

    fn enable_rx_intr(&mut self) {
        // SAFETY: IER register modification through MMIO
        unsafe {
            let ier = self.read_ier();
            let new_ier = ier | IER::DATA_READY.bits();
            self.write_ier(new_ier);
        }
    }

    fn disable_rx_intr(&mut self) {
        unsafe {
            let ier = self.read_ier();
            let new_ier = ier & !IER::DATA_READY.bits();
            self.write_ier(new_ier);
        }
    }

    fn enable_tx_intr(&mut self) {
        unsafe {
            let ier = self.read_ier();
            let new_ier = ier | IER::THR_EMPTY.bits();
            self.write_ier(new_ier);
        }
    }

    fn disable_tx_intr(&mut self) {
        unsafe {
            let ier = self.read_ier();
            let new_ier = ier & !IER::THR_EMPTY.bits();
            self.write_ier(new_ier);
        }
    }

    fn intr_identification(&mut self) -> Option<InterruptType> {
        let isr = self.inner.isr();
        isr.interrupt_type()
    }

    fn rx_ready(&mut self) -> bool {
        let lsr = self.inner.lsr();
        lsr.contains(LSR::DATA_READY)
    }

    fn tx_ready(&mut self) -> bool {
        let lsr = self.inner.lsr();
        lsr.contains(LSR::THR_EMPTY)
    }
}

// SAFETY: Uart16550<MmioBackend> is Send (see uart_16550 lib.rs:950)
unsafe impl Send for Uart16550Async {}