//! Async UART driver (M1 architecture validation)

mod ring_buffer;
mod console_driver;
mod device_ops;
mod async_uart;
mod uart16550_impl;

pub use device_ops::AsyncUartTestDevice;
pub use ring_buffer::AsyncBuffer;
pub use console_driver::ConsoleDriver;
pub use async_uart::AsyncUart;
pub use uart16550_impl::Uart16550Async;