// kernel/src/drivers/serial/async_uart.rs

use uart_16550::spec::registers::InterruptType;

/// Async UART abstraction for high-performance serial communication
///
/// This trait provides a high-level abstraction for UART hardware,
/// encapsulating non-blocking I/O operations and interrupt control.
/// It does not expose register-level details, making it suitable for
/// various UART hardware implementations (16550, DwApbUart, etc.).
pub trait AsyncUart: Send {
    /// Try to read bytes from hardware FIFO (non-blocking)
    ///
    /// Returns the number of bytes actually read. Returns 0 if no data
    /// is currently available in the hardware RX FIFO.
    fn try_read(&mut self, buf: &mut [u8]) -> usize;

    /// Try to write bytes to hardware FIFO (non-blocking)
    ///
    /// Returns the number of bytes actually written. Returns 0 if the
    /// hardware TX FIFO is currently full.
    fn try_write(&mut self, data: &[u8]) -> usize;

    /// Enable RX interrupt (Received Data Available)
    ///
    /// When enabled, the UART will generate an interrupt when data
    /// becomes available in the RX FIFO (reaching trigger level).
    fn enable_rx_intr(&mut self);

    /// Disable RX interrupt
    ///
    /// Prevents RX interrupts from being generated. Used by ISR to
    /// prevent re-entry after interrupt is triggered.
    fn disable_rx_intr(&mut self);

    /// Enable TX interrupt (Transmitter Holding Register Empty)
    ///
    /// When enabled, the UART will generate an interrupt when the
    /// TX FIFO becomes empty (ready to accept more data).
    fn enable_tx_intr(&mut self);

    /// Disable TX interrupt
    ///
    /// Prevents TX interrupts from being generated. Should be disabled
    /// when TX path is idle to avoid spurious interrupts.
    fn disable_tx_intr(&mut self);

    /// Get interrupt identification (IIR register)
    ///
    /// Returns the interrupt type that is currently pending, or None
    /// if no interrupt is pending. Used by ISR to identify interrupt source.
    fn intr_identification(&mut self) -> Option<InterruptType>;

    /// Check if RX FIFO has data (LSR::DATA_READY)
    ///
    /// Returns true if there is at least one byte available in the RX FIFO.
    fn rx_ready(&mut self) -> bool;

    /// Check if TX FIFO is empty (LSR::THR_EMPTY)
    ///
    /// Returns true if the TX FIFO (and transmitter holding register) is empty,
    /// meaning new data can be written.
    fn tx_ready(&mut self) -> bool;
}