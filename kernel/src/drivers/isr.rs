// kernel/src/drivers/isr.rs

//! ISR 分发机制测试：验证 ISR 上下文是否可以访问 UART 寄存器
//!
//! **关键测试**：
//! - ISR handler 尝试读 UART ISR 寄存器
//! - 如果成功 → ISR 可以访问 UART，继续原设计
//! - 如果失败（LoadFault）→ ISR 也无法访问，调整架构策略
//!
//! ISR 执行原则：
//! 1. 读 ISR 寄存器判断 InterruptType
//! 2. 禁用对应中断（防止重入）
//! 3. 唤醒 rx_waker/tx_waker
//! 4. 数据搬运推迟到 copier 任务（ISR 最小工作）

use uart_16550::spec::registers::InterruptType;
use crate::drivers::uart_init::uart_instance;

/// UART ISR handler（测试 UART 访问权限）
///
/// # ISR 安全约束
///
/// - 无阻塞：ISR 在中断上下文中执行
/// - 无锁：使用 SpinNoIrq 保护 UART 访问
/// - 最小工作：读 ISR + 输出日志（测试阶段）
///
/// # Arguments
///
/// * `irq` - IRQ 号（由 axhal::irq_handler 传递）
pub fn uart_isr_handler(irq: usize) {
    use core::sync::atomic::{AtomicUsize, Ordering};
    static ISR_COUNT: AtomicUsize = AtomicUsize::new(0);
    let count = ISR_COUNT.fetch_add(1, Ordering::Relaxed);

    let mut uart = uart_instance().lock();
    let isr = uart.isr();

    // First ISR call: verify access + log details
    if count == 0 {
        ax_println!("[UART ISR] ✅ ISR={:02x} (IRQ {})", isr.bits(), irq);
    }

    // Clear RX interrupt by reading data
    match isr.interrupt_type() {
        Some(InterruptType::ReceivedDataReady)
        | Some(InterruptType::ReceptionTimeout) => {
            // Drain RX FIFO to clear interrupt
            let mut drained = 0u32;
            while let Ok(_) = uart.try_receive_byte() {
                drained += 1;
            }
            if count == 0 {
                ax_println!("[UART ISR] RX interrupt cleared (drained {} bytes)", drained);
            }
        }
        _ => {}
    }
    // ISR dropped here → SpinNoIrq lock released
}