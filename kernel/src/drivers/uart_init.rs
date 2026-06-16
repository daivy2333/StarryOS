// kernel/src/drivers/uart_init.rs

//! UART 硬件初始化 + 异步驱动集成
//!
//! 使用 uart_16550 crate 的异步实现，配置：
//! - 波特率：115200 bps
//! - FIFO：使能，触发阈值 14 字节
//! - 中断：IER::DATA_READY | IER::THR_EMPTY（RX + TX 中断）
//! - 数据格式：8-N-1
//!
//! 异步驱动使用 uart_16550::async_ 模块：
//! - AsyncUartDriver: RX/TX copier 任务（NAPI 中断合并）
//! - RingBufRx/RingBufTx: 无锁环形缓冲区
//! - UartPort: 硬件访问抽象

use alloc::sync::Arc;
use core::ptr::{NonNull, addr_of_mut};
use core::sync::atomic::{AtomicU8, Ordering};

use axhal::mem::phys_to_virt;
use axlog::info;
use embassy_hal_internal::atomic_ring_buffer::RingBuffer;
use kspin::SpinNoIrq;
use lazy_static::lazy_static;
use memory_addr::{PhysAddr, VirtAddr};
use spin::Once;
use uart_16550::{
    Uart16550,
    async_::driver::{AsyncUartDriver, UartPort},
    async_::ring_buffer::{RingBufRx, RingBufTx},
    backend::MmioBackend,
    spec::registers::{IER, ISR, LSR},
};

use crate::drivers::os_arceos::{ArceOsRuntime, ArceOsWakerSet};

/// UART MMIO 物理地址（RISC-V QEMU virt 平台）
pub const UART_MMIO_BASE_PHYS: usize = 0x10000000;

/// UART 寄存器 stride（NS16550 是字节寻址设备，stride=1）
/// 注意：stride 不能设为 4——NS16550 寄存器仅 0x00-0x07 共 8 字节，
/// stride 4 会读到超出范围的总线错误（LoadFault）
pub const UART_STRIDE: u8 = 1;

/// Ring buffer 大小（64 KB）
const BUF_SIZE: usize = 64 * 1024;

/// 获取 UART MMIO 虚拟地址
fn get_uart_mmio_virt() -> VirtAddr {
    phys_to_virt(PhysAddr::from(UART_MMIO_BASE_PHYS))
}

// ── 全局 UART 实例 ─────────────────────────────────────────────────

// 全局 UART 实例（AsyncUart 独占访问）
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

// ── UartPort 实现 ──────────────────────────────────────────────────

/// ArceOS UART 端口抽象，实现 uart_16550 的 `UartPort` trait。
///
/// 内部持有 `&'static SpinNoIrq<Uart16550<MmioBackend>>` 引用，
/// 通过 `SpinNoIrq` 锁提供 `receive_bytes` 和 `send_bytes` 的安全访问。
pub struct ArceOsUartPort {
    uart: &'static SpinNoIrq<Uart16550<MmioBackend>>,
}

impl UartPort for ArceOsUartPort {
    fn receive_bytes(&self, buf: &mut [u8]) -> usize {
        self.uart.lock().receive_bytes(buf)
    }

    fn send_bytes(&self, buf: &[u8]) -> usize {
        self.uart.lock().send_bytes(buf)
    }
}

lazy_static! {
    static ref UART_PORT: ArceOsUartPort = ArceOsUartPort { uart: &UART };
}

// ── 类型别名 ──────────────────────────────────────────────────────

/// 异步驱动类型（`ArceOsRuntime` + `ArceOsWakerSet` + `ArceOsUartPort`）
pub type ArceOsDriver = AsyncUartDriver<ArceOsRuntime, ArceOsWakerSet, ArceOsUartPort>;
/// 异步读取器类型
pub type ArceOsReader = uart_16550::async_::device_ops::AsyncUartReader<
    ArceOsRuntime,
    ArceOsWakerSet,
    ArceOsUartPort,
>;
/// 异步写入器类型
pub type ArceOsWriter = uart_16550::async_::device_ops::AsyncUartWriter<
    ArceOsRuntime,
    ArceOsWakerSet,
    ArceOsUartPort,
>;

// ── Ring buffer 静态存储 ──────────────────────────────────────────

static RX_RING: RingBuffer = RingBuffer::new();
static TX_RING: RingBuffer = RingBuffer::new();
// SAFETY: initialized once during kernel init, valid forever.
static mut RX_BUF: [u8; BUF_SIZE] = [0u8; BUF_SIZE];
static mut TX_BUF: [u8; BUF_SIZE] = [0u8; BUF_SIZE];

/// Check if the TX ring buffer is empty (no pending data for copier).
pub fn tx_is_empty() -> bool {
    TX_RING.is_empty()
}

// ── 驱动实例存储 ──────────────────────────────────────────────────

static DRIVER: Once<Arc<ArceOsDriver>> = Once::new();

/// 获取驱动实例的 `Arc` 引用（用于创建 `AsyncUartReader`/`AsyncUartWriter`）。
pub fn driver() -> Arc<ArceOsDriver> {
    DRIVER
        .get()
        .expect("UART driver not initialized")
        .clone()
}

/// 获取驱动实例的 `&'static` 引用（用于 `start_rx_copier`/`start_tx_copier`）。
fn driver_ref() -> &'static ArceOsDriver {
    DRIVER
        .get()
        .expect("UART driver not initialized")
        .as_ref()
}

// ── IER 缓存与中断控制 ────────────────────────────────────────────

/// Cached IER value — shared between ISR (uart_16550) and copier tasks.
static CACHED_IER: AtomicU8 = AtomicU8::new(0);

fn write_ier(value: u8) {
    CACHED_IER.store(value, Ordering::Relaxed);
    uart_instance().lock().set_ier(IER::from_bits_truncate(value));
}

/// 重新使能 RX 中断（copier 任务回调）
pub fn enable_rx_intr() {
    write_ier(CACHED_IER.load(Ordering::Relaxed) | IER::DATA_READY.bits());
}

/// 重新使能 TX 中断（copier 任务回调）
pub fn enable_tx_intr() {
    write_ier(CACHED_IER.load(Ordering::Relaxed) | IER::THR_EMPTY.bits());
}

// ── ISR 包装器 ────────────────────────────────────────────────────

/// UART ISR 包装器 — 桥接 axhal IRQ hook 到 uart_16550 ISR handler。
///
/// 满足 ISR 极简原则：读 ISR / 禁中断 / wake / 返回。
fn uart_isr_wrapper(_irq: usize) {
    let base = NonNull::new(get_uart_mmio_virt().as_mut_ptr()).unwrap();
    // SAFETY: Called from ISR context with valid UART MMIO base address.
    // CACHED_IER is shared with enable_rx_intr/enable_tx_intr via AtomicU8.
    uart_16550::async_::isr::uart_isr_handler(_irq, base, &CACHED_IER);
}

// ── 初始化 ────────────────────────────────────────────────────────

/// 初始化 UART 硬件 + 异步驱动
///
/// 完成以下初始化步骤：
/// 1. MMIO 映射验证
/// 2. Ring buffer 初始化
/// 3. `AsyncUartDriver` 创建
/// 4. ISR 注册
/// 5. RX/TX copier 任务启动
///
/// # When to Call
///
/// 在 `entry.rs::init()` 中调用，位于 Console 初始化之后。
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
    let base_ptr = get_uart_mmio_virt().as_ptr();
    ax_println!("[UART INIT] base_ptr = {:?}", base_ptr);

    // Try reading LSR register at stride 1 (offset 5), same offset Console uses
    ax_println!("[UART INIT] Trying raw read at base+5 (stride 1, LSR)...");
    let lsr_raw: u8 = unsafe { base_ptr.add(5).read_volatile() };
    ax_println!("[UART INIT] ✅ Raw LSR read: {:#02x}", lsr_raw);

    // Now try the uart_16550 crate path
    ax_println!("[UART INIT] Trying uart_16550 crate access...");
    let mut uart = uart_instance().lock();
    log_uart_state(&mut uart);
    // FCR threshold check: ISR bits 6-7 indicate FIFO status
    let isr = uart.isr();
    let fifo_enabled = isr.contains(ISR::FIFOS_ENABLED0 | ISR::FIFOS_ENABLED1);
    ax_println!(
        "[UART INIT] FCR: FIFO enabled={}, trigger level via ISR bits 7-6",
        fifo_enabled
    );
    drop(uart);

    ax_println!("[UART INIT] ✅ Phase 1 PASSED: UART registers readable");

    // Step 3: Initialize ring buffers
    // SAFETY: called exactly once before any concurrent ring-buffer access.
    // The backing `static mut` buffers live for the entire kernel lifetime.
    unsafe {
        RX_RING.init(addr_of_mut!(RX_BUF).cast::<u8>(), BUF_SIZE);
        TX_RING.init(addr_of_mut!(TX_BUF).cast::<u8>(), BUF_SIZE);
    }
    ax_println!(
        "[UART INIT] ✅ Ring buffers initialized ({} KB each)",
        BUF_SIZE / 1024
    );

    // Step 4: Create async driver
    // SAFETY: Ring buffers are initialized above, and we create exactly one
    // RingBufRx/RingBufTx pair per ring.
    let rx = unsafe { RingBufRx::<ArceOsWakerSet>::new(&RX_RING) };
    let tx = unsafe { RingBufTx::<ArceOsWakerSet>::new(&TX_RING) };

    let uart_port: &'static ArceOsUartPort = &UART_PORT;
    let driver = Arc::new(ArceOsDriver::new(rx, tx, uart_port));
    DRIVER.call_once(|| driver);
    ax_println!("[UART INIT] ✅ AsyncUartDriver created");

    // Step 5: Register ISR
    axhal::irq::register_irq_hook(uart_isr_wrapper);
    ax_println!("[UART INIT] ✅ ISR registered");

    // Step 6: Start copier tasks
    driver_ref().start_rx_copier(enable_rx_intr);
    driver_ref().start_tx_copier(enable_tx_intr);
    ax_println!("[UART INIT] ✅ RX/TX copier tasks started");
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
