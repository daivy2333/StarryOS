// kernel/src/drivers/uart_init.rs

//! UART 硬件初始化（替代 axplat UART init）
//!
//! 使用 uart_16550 crate 本地初始化，配置 AsyncUart 专用参数：
//! - 波特率：115200 bps
//! - FIFO：使能，触发阈值 14 字节
//! - 中断：IER::DATA_READY | IER::THR_EMPTY（RX + TX 中断）
//! - 数据格式：8-N-1

use core::ptr::NonNull;

use axlog::info;
use kspin::SpinNoIrq;
use lazy_static::lazy_static;
use uart_16550::{
    BaudRate, Config, Uart16550,
    backend::MmioBackend,
    spec::{
        CLK_FREQUENCY_HZ,
        registers::{FifoTriggerLevel, IER, ISR, LSR, Parity, WordLength},
    },
};

/// UART MMIO 基地址（RISC-V QEMU virt 平台）
pub const UART_MMIO_BASE: usize = 0x10000000;

/// UART 寄存器 stride（RISC-V MMIO 标准）
pub const UART_STRIDE: u8 = 4;

/// 全局 UART 实例（AsyncUart 独占访问）
lazy_static! {
    static ref UART: SpinNoIrq<Uart16550<MmioBackend>> = SpinNoIrq::new(unsafe {
        Uart16550::new_mmio(
            NonNull::new(UART_MMIO_BASE as *mut u8).unwrap(),
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
/// # Safety
///
/// 必须在内核启动早期调用，覆盖 axplat UART 初始化配置。
/// 此函数会重新配置所有 UART 寄存器。
pub fn init_uart_hardware() {
    let mut uart = UART.lock();

    let config = Config {
        baud_rate: BaudRate::Baud115200,              // 波特率：115200
        data_bits: WordLength::EightBits,             // 8 数据位
        extra_stop_bits: false,                       // 1 停止位
        parity: Parity::Disabled,                     // 无校验
        interrupts: IER::DATA_READY | IER::THR_EMPTY, // RX + TX 中断（关键！）
        fifo_trigger_level: Some(FifoTriggerLevel::Fourteen), // FIFO 触发 14 字节
        frequency: CLK_FREQUENCY_HZ,                  // 时钟频率：1.8432 MHz
        prescaler_division_factor: None,              // 无预分频
    };

    uart.init(config).expect("UART initialization failed");

    // 验证 UART 状态
    log_uart_state(&mut *uart);
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
