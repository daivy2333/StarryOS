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
        registers::{FifoTriggerLevel, IER, ISR, LSR, Parity, WordLength, offsets},
    },
};

/// UART MMIO 物理地址（RISC-V QEMU virt 平台）
pub const UART_MMIO_BASE_PHYS: usize = 0x10000000;

/// UART 寄存器 stride（NS16550 是字节寻址设备，stride=1）
/// 注意：stride 不能设为 4——NS16550 寄存器仅 0x00-0x07 共 8 字节，
/// stride 4 会读到超出范围的总线错误（LoadFault）
pub const UART_STRIDE: u8 = 1;

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

// IER register manipulation helpers (direct MMIO, cached to reduce RMW)
use core::sync::atomic::{AtomicU8, Ordering};
static CACHED_IER: AtomicU8 = AtomicU8::new(0);

fn write_ier(value: u8) {
    CACHED_IER.store(value, Ordering::Relaxed);
    let ptr = get_uart_mmio_virt().as_mut_ptr();
    unsafe { core::ptr::write_volatile(ptr.add(offsets::IER as usize), value) };
}
pub fn enable_rx_intr()  { write_ier(CACHED_IER.load(Ordering::Relaxed) | IER::DATA_READY.bits()); }
pub fn disable_rx_intr() { write_ier(CACHED_IER.load(Ordering::Relaxed) & !IER::DATA_READY.bits()); }
pub fn enable_tx_intr()  { write_ier(CACHED_IER.load(Ordering::Relaxed) | IER::THR_EMPTY.bits()); }
pub fn disable_tx_intr() { write_ier(CACHED_IER.load(Ordering::Relaxed) & !IER::THR_EMPTY.bits()); }

/// 初始化 UART 硬件 — Phase 1: 只读寄存器验证
///
/// 2026-05-31 纠正：此前认为"MMIO 权限阻塞"的结论有误。
/// 经代码验证，UART MMIO（0x10000000）已被 new_kernel_aspace() 正确映射
/// 为 READ|WRITE|DEVICE。axmm::iomap() 作为安全保障，确保权限正确。
///
/// Phase 1 策略（只读）：
/// - 调用 axmm::iomap() 确保 UART MMIO 页表权限
/// - 读取 IER/ISR/LSR 寄存器验证 MMIO 读访问
/// - 不修改硬件配置（保护 Console 不受影响）
///
/// # When to Call
///
/// 在 entry.rs::init() 中调用，位于 Console 初始化之后、ISR 注册之前。
pub fn init_uart_hardware() {
    ax_println!("[UART INIT] Phase 1: MMIO read-only verification");

    // Step 1: Ensure UART MMIO is mapped with DEVICE|READ|WRITE
    match axmm::iomap(PhysAddr::from(UART_MMIO_BASE_PHYS), 0x1000) {
        Ok(vaddr) => {
            ax_println!("[UART INIT] ✅ iomap OK: UART MMIO at {:?}", vaddr);
        }
        Err(e) => {
            ax_println!(
                "[UART INIT] ⚠️ iomap returned: {:?} (mapping may already exist, continuing)",
                e
            );
        }
    }

    // Step 2: Direct raw pointer read test (bypass uart_16550 crate)
    let base_ptr: *const u8 = get_uart_mmio_virt().as_ptr() as *const u8;
    ax_println!("[UART INIT] base_ptr = {:?}", base_ptr);

    // Try reading LSR register at stride 1 (offset 5), same offset Console uses
    ax_println!("[UART INIT] Trying raw read at base+5 (stride 1, LSR)...");
    let lsr_raw: u8 = unsafe { base_ptr.add(5).read_volatile() };
    ax_println!("[UART INIT] ✅ Raw LSR read: {:#02x}", lsr_raw);

    // Now try the uart_16550 crate path
    ax_println!("[UART INIT] Trying uart_16550 crate access...");
    let mut uart = uart_instance().lock();
    log_uart_state(&mut uart);

    ax_println!("[UART INIT] ✅ Phase 1 PASSED: UART registers readable");
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
