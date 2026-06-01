// kernel/src/drivers/mod.rs

//! AsyncUart 异步串口驱动模块
//!
//! 模块结构：
//! - uart_init: UART 硬件初始化（替代 axplat）
//! - isr: ISR 分发机制（IRQ 10 → rx_waker/tx_waker）
//! - ring_buffer: AsyncBuffer（rx_buf + tx_buf + PollSet）
//! - async_driver: RX/TX copier 任务
//! - device_ops: AsyncUartReader/Writer（TtyRead/TtyWrite trait）
//! - ntty_async: AsyncTty 类型别名
//! - benchmark: 性能测试统计模块
//! - benchmark_cmd: Benchmark 命令接口

pub mod async_driver;
pub mod benchmark;
pub mod device_ops;
pub mod isr;
pub mod ntty_async;
pub mod ring_buffer;
pub mod uart_init;
pub use ntty_async::ASYNC_TTY;
pub type AsyncTty = crate::pseudofs::dev::tty::Tty<crate::drivers::device_ops::AsyncUartReader, crate::drivers::device_ops::AsyncUartWriter>;
