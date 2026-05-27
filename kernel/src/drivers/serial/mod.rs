//! Async UART driver (M1 architecture validation)

mod ring_buffer;
mod console_driver;
mod device_ops;
mod test;

pub use device_ops::AsyncUartTestDevice;
pub use ring_buffer::AsyncBuffer;
pub use console_driver::ConsoleDriver;
pub use test::run_m2_verification_test;