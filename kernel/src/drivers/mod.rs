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
pub(crate) mod hart_counter;
pub(crate) mod net_placement;
#[cfg(feature = "qemu")]
pub mod net_wake_witness;
pub(crate) mod net_wake_witness_logic;
#[cfg(not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench")))]
pub mod ntty_async;
pub mod os_arceos;
pub mod placement;
#[cfg(not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench")))]
mod serialized_writer;
pub(crate) mod uart_migration_logic;
pub mod uart_init;
pub mod uart_smp_snapshot;
pub(crate) mod uart_snapshot_types;
#[cfg(not(any(feature = "lichee-d1", feature = "k3")))]
pub mod virtio_net_irq;
#[cfg(not(any(feature = "lichee-d1", feature = "k3")))]
pub(crate) mod virtio_net_irq_logic;
#[cfg(not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench")))]
pub use ntty_async::ASYNC_TTY;
#[cfg(not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench")))]
pub type AsyncTty =
    crate::pseudofs::dev::tty::Tty<uart_init::ArceOsReader, uart_init::ArceOsWriter>;
