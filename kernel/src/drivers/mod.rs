// kernel/src/drivers/mod.rs

//! AsyncUart 异步串口驱动模块
//!
//! 模块结构：
//! - uart_init: UART 硬件初始化 + 异步驱动集成（uart_16550::async_）
//! - d1_uart: D1 (Allwinner D1) DW APB UART 32-bit MMIO port 实现
//! - ntty_async: AsyncTty 类型别名
//! - os_arceos: ArceOS OS 抽象 trait 实现
//! - bench: 内核态性能测试
//! - k3_uart / k3_uart_core: K3 PXA UART raw port（MS17 Iteration 000）

// MS17 Iteration 000: the K3 async UART low-level mode opens `drivers` for
// the raw PXA port; MS17 Iteration 001 (`k3-uart`) additionally opens the
// copier/ring data-plane tree below for K3, and Iteration 002 (T5 ⑦) opens
// the tty singleton for the riscv64 k3-uart shape. The raw-only mode
// (`k3-async-uart` without `k3-uart`) still keeps that tree out; modules
// marked `k3 exclusion` stay out of every K3 async shape (rings/copiers
// beyond the data plane, axnet/net probes).
#[cfg(all(feature = "k3", feature = "k3-async-uart"))]
pub mod k3_uart;
#[cfg(all(feature = "k3", feature = "k3-async-uart"))]
pub mod k3_uart_core;

#[cfg(not(all(feature = "k3", feature = "k3-async-uart")))]
pub mod bench;
#[cfg(feature = "lichee-d1-async-uart")]
pub mod d1_uart;
#[cfg(not(all(feature = "k3-async-uart", not(feature = "k3-uart"))))]
pub mod copier_start_guard;
#[cfg(not(all(feature = "k3-async-uart", not(feature = "k3-uart"))))]
pub(crate) mod hart_counter;
#[cfg(not(all(feature = "k3", feature = "k3-async-uart")))]
pub(crate) mod net_placement;
#[cfg(feature = "qemu")]
pub mod net_wake_witness;
#[cfg(not(all(feature = "k3", feature = "k3-async-uart")))]
pub(crate) mod net_wake_witness_logic;
#[cfg(not(any(
    feature = "lichee-d1-smoke",
    feature = "lichee-d1-kbench",
    all(
        feature = "k3",
        feature = "k3-async-uart",
        not(all(target_arch = "riscv64", feature = "k3-uart")),
    ),
)))]
pub mod ntty_async;
#[cfg(not(all(feature = "k3-async-uart", not(feature = "k3-uart"))))]
pub mod os_arceos;
#[cfg(not(all(feature = "k3-async-uart", not(feature = "k3-uart"))))]
pub mod placement;
#[cfg(not(all(feature = "k3-async-uart", not(feature = "k3-uart"))))]
mod serialized_writer;
#[cfg(not(all(feature = "k3", feature = "k3-async-uart")))]
pub(crate) mod uart_migration_logic;
#[cfg(not(all(feature = "k3-async-uart", not(feature = "k3-uart"))))]
pub mod uart_init;
#[cfg(not(all(feature = "k3-async-uart", not(feature = "k3-uart"))))]
pub mod uart_smp_snapshot;
#[cfg(not(all(feature = "k3-async-uart", not(feature = "k3-uart"))))]
pub(crate) mod uart_snapshot_types;
#[cfg(not(any(feature = "lichee-d1", feature = "k3")))]
pub mod virtio_net_irq;
#[cfg(not(any(feature = "lichee-d1", feature = "k3")))]
pub(crate) mod virtio_net_irq_logic;
#[cfg(not(any(
    feature = "lichee-d1-smoke",
    feature = "lichee-d1-kbench",
    all(
        feature = "k3",
        feature = "k3-async-uart",
        not(all(target_arch = "riscv64", feature = "k3-uart")),
    ),
)))]
pub use ntty_async::ASYNC_TTY;
#[cfg(not(any(
    feature = "lichee-d1-smoke",
    feature = "lichee-d1-kbench",
    all(
        feature = "k3",
        feature = "k3-async-uart",
        not(all(target_arch = "riscv64", feature = "k3-uart")),
    ),
)))]
pub type AsyncTty =
    crate::pseudofs::dev::tty::Tty<uart_init::ArceOsReader, uart_init::ArceOsWriter>;
