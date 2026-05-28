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
    ax_println!("[UART ISR] ISR handler called (IRQ {})", irq);

    // 🔴 关键测试：尝试访问 UART 寄存器（不检查 IRQ 号）
    ax_println!("[UART ISR] Attempting to access UART registers...");

    // 关键测试：尝试访问 UART 寄存器
    let mut uart = uart_instance().lock();

    ax_println!("[UART ISR] UART lock acquired, attempting to read ISR register...");

    // 尝试读 ISR 寄存器
    // 如果成功 → ISR 可以访问 UART
    // 如果失败 → 触发 LoadFault，会看到 panic 或错误日志
    let isr = uart.isr();

    ax_println!("[UART ISR] ✅ ISR register read SUCCESS! ISR={:02x}", isr.bits());

    // 检查中断类型
    match isr.interrupt_type() {
        Some(InterruptType::ReceivedDataReady) => {
            ax_println!("[UART ISR] RX data ready interrupt");
        }
        Some(InterruptType::TransmitterHoldingRegisterEmpty) => {
            ax_println!("[UART ISR] THR empty interrupt");
        }
        Some(InterruptType::ReceptionTimeout) => {
            ax_println!("[UART ISR] RX timeout interrupt");
        }
        Some(InterruptType::ReceiverLineStatus) => {
            ax_println!("[UART ISR] Line status error interrupt");
        }
        None => {
            ax_println!("[UART ISR] No pending interrupt (spurious)");
        }
        _ => {
            ax_println!("[UART ISR] Other interrupt type");
        }
    }
}