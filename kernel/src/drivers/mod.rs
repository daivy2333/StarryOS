// kernel/src/drivers/mod.rs

//! AsyncUart 异步串口驱动模块
//!
//! 模块结构：
//! - uart_init: UART 硬件初始化 + 异步驱动集成（uart_16550::async_）
//! - ntty_async: AsyncTty 类型别名
//! - os_arceos: ArceOS OS 抽象 trait 实现
//! - bench: 内核态性能测试

pub mod bench;
pub mod ntty_async;
pub mod os_arceos;
pub mod uart_init;
pub use ntty_async::ASYNC_TTY;
pub type AsyncTty = crate::pseudofs::dev::tty::Tty<uart_init::ArceOsReader, uart_init::ArceOsWriter>;
