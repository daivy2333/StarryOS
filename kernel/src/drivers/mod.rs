// kernel/src/drivers/mod.rs

//! AsyncUart 异步串口驱动模块
//!
//! 模块结构：
//! - uart_init: UART 硬件初始化 + 异步驱动集成（uart_16550::async_）
//! - d1_uart: D1 (Allwinner D1) DW APB UART 32-bit MMIO port 实现
//! - ntty_async: AsyncTty 类型别名
//! - os_arceos: ArceOS OS 抽象 trait 实现
//! - bench: 内核态性能测试

pub mod bench;
#[cfg(feature = "lichee-d1-async-uart")]
pub mod d1_uart;
#[cfg(not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench", feature = "lichee-d1-rootfs-probe")))]
pub mod ntty_async;
pub mod os_arceos;
pub mod uart_init;
#[cfg(not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench", feature = "lichee-d1-rootfs-probe")))]
pub use ntty_async::ASYNC_TTY;
#[cfg(not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench", feature = "lichee-d1-rootfs-probe")))]
pub type AsyncTty =
    crate::pseudofs::dev::tty::Tty<uart_init::ArceOsReader, uart_init::ArceOsWriter>;
