// kernel/src/drivers/mod.rs

//! AsyncUart 异步串口驱动模块
//!
//! 模块结构：
//! - uart_init: UART 硬件初始化（替代 axplat）
//! - isr: ISR 分发机制（IRQ 10 → rx_waker/tx_waker）
//! - ring_buffer: AsyncBuffer（rx_buf + tx_buf + PollSet）
//! - async_uart: AsyncUart trait + Uart16550Async 实现
//! - async_driver: RX/TX copier 任务
//! - device_ops: AsyncUartDevice（DeviceOps + Pollable）

pub mod uart_init;    // UART 硬件初始化
pub mod isr;          // ISR 分发机制
pub mod ring_buffer;  // AsyncBuffer
pub mod async_uart;   // AsyncUart trait
pub mod async_driver; // RX/TX copier
pub mod device_ops;   // DeviceOps trait