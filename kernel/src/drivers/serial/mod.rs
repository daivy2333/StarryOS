//! Async UART driver (M1 architecture validation)

mod ring_buffer;
mod console_driver;
mod device_ops;

pub use device_ops::AsyncUartTestDevice;