//! Async UART driver (M3 AsyncUart implementation)

mod ring_buffer;
mod console_driver;
mod device_ops;
mod async_uart;
mod uart16550_impl;
mod isr;
mod async_driver;  // M3: AsyncUartDriver with RX/TX copier

pub use device_ops::AsyncUartTestDevice;
pub use ring_buffer::AsyncBuffer;
pub use console_driver::ConsoleDriver;
pub use async_uart::AsyncUart;
pub use uart16550_impl::Uart16550Async;
pub use isr::{IsrContext, uart_isr_handler};
pub use async_driver::AsyncUartDriver;  // M3: Export AsyncUartDriver