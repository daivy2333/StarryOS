// kernel/src/drivers/uart_init.rs

//! UART 硬件初始化（替代 axplat UART init）
//!
//! 使用 uart_16550 crate 本地初始化，配置 AsyncUart 专用参数：
//! - 波特率：115200 bps
//! - FIFO：使能，触发阈值 14 字节
//! - 中断：IER::DATA_READY | IER::THR_EMPTY（RX + TX 中断）
//! - 数据格式：8-N-1

use core::ptr::NonNull;

use axhal::mem::phys_to_virt;
use axlog::info;
use kspin::SpinNoIrq;
use lazy_static::lazy_static;
use memory_addr::{PhysAddr, VirtAddr};
use uart_16550::{
    BaudRate, Config, Uart16550,
    backend::MmioBackend,
    spec::{
        CLK_FREQUENCY_HZ,
        registers::{FifoTriggerLevel, IER, ISR, LSR, Parity, WordLength},
    },
};

/// UART MMIO 物理地址（RISC-V QEMU virt 平台）
pub const UART_MMIO_BASE_PHYS: usize = 0x10000000;

/// UART 寄存器 stride（RISC-V MMIO 标准）
pub const UART_STRIDE: u8 = 4;

/// 获取 UART MMIO 虚拟地址
fn get_uart_mmio_virt() -> VirtAddr {
    phys_to_virt(PhysAddr::from(UART_MMIO_BASE_PHYS))
}

/// 全局 UART 实例（AsyncUart 独占访问）
lazy_static! {
    static ref UART: SpinNoIrq<Uart16550<MmioBackend>> = SpinNoIrq::new(unsafe {
        // SAFETY: get_uart_mmio_virt() returns the virtual address mapped from physical
        // UART MMIO address (0x10000000) on RISC-V QEMU virt platform. This mapping
        // is established by axruntime during boot, and we have exclusive access
        // protected by SpinNoIrq.
        Uart16550::new_mmio(
            NonNull::new(get_uart_mmio_virt().as_mut_ptr()).unwrap(),
            UART_STRIDE,
        )
        .expect("UART MMIO address invalid")
    });
}

/// 获取全局 UART 实例的引用
pub fn uart_instance() -> &'static SpinNoIrq<Uart16550<MmioBackend>> {
    &UART
}

/// 初始化 UART 硬件（AsyncUart 专用配置）
///
/// # Strategy Adjustment
///
/// **完全放弃访问 UART 寄存器**：
/// - UART MMIO 区域在内核启动后没有访问权限（LoadFault/StoreFault）
/// - 无法读取或写入 UART 寄存器
/// - 依赖 axplat 的 UART 配置（Console 已使能 RX 中断）
///
/// **后续策略**：
/// - P2 阶段通过 ISR 分发机制访问 UART（ISR 可能可以访问 UART）
/// - 或者在 boot 阶段修改 UART 配置（更早的阶段）
///
/// # When to Call
///
/// 当前阶段**不执行任何操作**，仅作为占位符。
pub fn init_uart_hardware() {
    ax_println!("[UART INIT] Skipped UART register access (MMIO permission issue)");
    ax_println!("[UART INIT] Will configure UART in ISR handler or boot stage");
}

/// 日志输出 UART 寄存器状态（调试验证）
fn log_uart_state(uart: &mut Uart16550<MmioBackend>) {
    let ier = uart.ier();
    let isr = uart.isr();
    let lsr = uart.lsr();

    info!(
        "[UART INIT] IER={:02x} ISR={:02x} LSR={:02x}",
        ier.bits(),
        isr.bits(),
        lsr.bits()
    );

    // 检查关键配置
    if !ier.contains(IER::DATA_READY) {
        info!("[UART INIT] ⚠️ RX interrupt NOT enabled!");
    }
    if !ier.contains(IER::THR_EMPTY) {
        info!("[UART INIT] ⚠️ TX interrupt NOT enabled!");
    } else {
        info!("[UART INIT] ✅ TX interrupt enabled (AsyncUart needs this)");
    }

    // 检查 FIFO 状态（ISR 的 FIFOS_ENABLED0 和 FIFOS_ENABLED1 位）
    if isr.contains(ISR::FIFOS_ENABLED0 | ISR::FIFOS_ENABLED1) {
        info!("[UART INIT] ✅ FIFO enabled (16 bytes)");
    } else {
        info!("[UART INIT] ⚠️ FIFO NOT enabled!");
    }

    // 检查 TX transmitter 状态
    if lsr.contains(LSR::TRANSMITTER_EMPTY) {
        info!("[UART INIT] ✅ TX transmitter empty (ready to send)");
    }
}
