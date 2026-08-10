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
#[cfg(not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench")))]
use core::task::Waker;
use core::{
    ptr::{NonNull, addr_of_mut},
    sync::atomic::{AtomicU8, Ordering},
};

use axlog::info;
use embassy_hal_internal::atomic_ring_buffer::RingBuffer;
use lazy_static::lazy_static;
use memory_addr::VirtAddr;
use spin::Once;
#[cfg(not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench")))]
use uart_16550::{TtyWrite, async_::device_ops::AsyncUartWriter};
use uart_16550::{
    async_::{
        driver::{AsyncUartDriver, UartPort},
        ring_buffer::{RingBufRx, RingBufTx},
    },
    spec::registers::IER,
};
// ── QEMU NS16550 path ────────────────────────────────────────────────
#[cfg(not(feature = "lichee-d1-async-uart"))]
use {
    axhal::mem::phys_to_virt,
    kspin::SpinNoIrq,
    memory_addr::PhysAddr,
    uart_16550::{Uart16550, backend::MmioBackend, spec::registers::ISR},
};

#[cfg(not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench")))]
use super::serialized_writer::SerializedWriter;
// ── D1 DW APB UART path ──────────────────────────────────────────────
#[cfg(feature = "lichee-d1-async-uart")]
use crate::drivers::d1_uart::{ArceOsD1UartPort, d1_uart_isr_handler};
#[cfg(not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench")))]
use crate::pseudofs::dev::tty::terminal::ldisc::TtyWriteReady;
use crate::{
    drivers::os_arceos::{ArceOsRuntime, ArceOsWakerSet},
    platform,
};

/// Ring buffer 大小（64 KB）
pub const BUF_SIZE: usize = 64 * 1024;

/// 获取 UART MMIO 虚拟地址（从 platform descriptor 读取）
#[cfg(not(feature = "lichee-d1-async-uart"))]
fn get_uart_mmio_virt() -> VirtAddr {
    let desc = platform::descriptor();
    phys_to_virt(PhysAddr::from(desc.console.base_paddr))
}

/// D1 UART 使用平台 direct-map VA，避免在应用入口阶段触发 axmm::iomap/kernel_aspace。
#[cfg(feature = "lichee-d1-async-uart")]
fn get_uart_mmio_virt() -> VirtAddr {
    let desc = platform::descriptor();
    VirtAddr::from(desc.console.base_paddr + axconfig::plat::PHYS_VIRT_OFFSET)
}

// ── QEMU: 全局 UART 实例 ─────────────────────────────────────────────

#[cfg(not(feature = "lichee-d1-async-uart"))]
lazy_static! {
    static ref UART: SpinNoIrq<Uart16550<MmioBackend>> = SpinNoIrq::new(unsafe {
        // SAFETY: get_uart_mmio_virt() returns the virtual address mapped from
        // the platform descriptor console base (0x10000000 for QEMU).
        // This mapping is established by axruntime during boot.
        let desc = platform::descriptor();
        Uart16550::new_mmio(
            NonNull::new(get_uart_mmio_virt().as_mut_ptr()).unwrap(),
            desc.console.reg_stride,
        )
        .expect("UART MMIO address invalid")
    });
}

#[cfg(not(feature = "lichee-d1-async-uart"))]
pub fn uart_instance() -> &'static SpinNoIrq<Uart16550<MmioBackend>> {
    &UART
}

// ── QEMU: UartPort 实现 ──────────────────────────────────────────────

/// ArceOS UART 端口抽象，实现 uart_16550 的 `UartPort` trait。
#[cfg(not(feature = "lichee-d1-async-uart"))]
pub struct ArceOsUartPort {
    uart: &'static SpinNoIrq<Uart16550<MmioBackend>>,
    ier_cache: AtomicU8,
}

#[cfg(not(feature = "lichee-d1-async-uart"))]
impl UartPort for ArceOsUartPort {
    #[inline(always)]
    fn receive_bytes(&self, buf: &mut [u8]) -> usize {
        self.uart.lock().receive_bytes(buf)
    }

    #[inline(always)]
    fn send_bytes(&self, buf: &[u8]) -> usize {
        self.uart.lock().send_bytes(buf)
    }

    #[inline(always)]
    fn transmitter_empty(&self) -> bool {
        self.uart
            .lock()
            .lsr()
            .contains(uart_16550::spec::registers::LSR::TRANSMITTER_EMPTY)
    }

    #[inline(always)]
    fn update_ier(&self, set: IER, clear: IER) {
        let mut uart = self.uart.lock();
        let mut val = self.ier_cache.load(Ordering::Relaxed);
        val |= set.bits();
        val &= !clear.bits();
        self.ier_cache.store(val, Ordering::Relaxed);
        uart.set_ier(IER::from_bits_truncate(val));
    }
}

#[cfg(not(feature = "lichee-d1-async-uart"))]
lazy_static! {
    static ref UART_PORT: ArceOsUartPort = ArceOsUartPort {
        uart: &UART,
        ier_cache: AtomicU8::new(0),
    };
}

// ── D1: UartPort 实例 ────────────────────────────────────────────────

#[cfg(feature = "lichee-d1-async-uart")]
lazy_static! {
    static ref D1_UART_PORT: ArceOsD1UartPort = {
        let desc = platform::descriptor();
        // SAFETY: D1 platform MMIO is identity-mapped by OpenSBI; UART0 clock
        // and pins are configured by U-Boot.
        unsafe {
            ArceOsD1UartPort::new(
                NonNull::new(get_uart_mmio_virt().as_mut_ptr()).unwrap(),
                desc.console.reg_stride,
            )
        }
    };
}

// ── 类型别名 ─────────────────────────────────────────────────────────

#[cfg(not(feature = "lichee-d1-async-uart"))]
pub type ArceOsDriver = AsyncUartDriver<ArceOsRuntime, ArceOsWakerSet, ArceOsUartPort>;
#[cfg(feature = "lichee-d1-async-uart")]
pub type ArceOsDriver = AsyncUartDriver<ArceOsRuntime, ArceOsWakerSet, ArceOsD1UartPort>;

#[cfg(not(feature = "lichee-d1-async-uart"))]
pub type ArceOsReader =
    uart_16550::async_::device_ops::AsyncUartReader<ArceOsRuntime, ArceOsWakerSet, ArceOsUartPort>;
#[cfg(feature = "lichee-d1-async-uart")]
pub type ArceOsReader = uart_16550::async_::device_ops::AsyncUartReader<
    ArceOsRuntime,
    ArceOsWakerSet,
    ArceOsD1UartPort,
>;

#[cfg(all(
    not(feature = "lichee-d1-async-uart"),
    not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench"))
))]
pub(crate) struct RawArceOsWriter(
    pub(crate) AsyncUartWriter<ArceOsRuntime, ArceOsWakerSet, ArceOsUartPort>,
);
#[cfg(all(
    feature = "lichee-d1-async-uart",
    not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench"))
))]
pub(crate) struct RawArceOsWriter(
    pub(crate) AsyncUartWriter<ArceOsRuntime, ArceOsWakerSet, ArceOsD1UartPort>,
);

#[cfg(not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench")))]
#[derive(Clone)]
pub struct ArceOsWriter(SerializedWriter<RawArceOsWriter>);

#[cfg(not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench")))]
impl ArceOsWriter {
    pub(crate) fn new(raw: RawArceOsWriter) -> Self {
        Self(SerializedWriter::new(raw))
    }
}

#[cfg(not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench")))]
impl TtyWrite for ArceOsWriter {
    fn write(&self, buf: &[u8]) -> usize {
        self.0.with_lock(|raw| raw.0.try_write(buf))
    }
}

#[cfg(not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench")))]
impl TtyWriteReady for ArceOsWriter {
    fn waits_for_write_completion(&self) -> bool {
        true
    }

    fn can_write(&self) -> bool {
        self.0.with_lock(|raw| raw.0.can_write())
    }

    fn writable_len(&self) -> usize {
        self.0.with_lock(|raw| raw.0.writable_len())
    }

    fn register_writable_waker(&self, waker: &Waker) {
        self.0.with_lock(|raw| raw.0.register_writable_waker(waker));
    }
}

// ── Ring buffer 静态存储 ─────────────────────────────────────────────

static RX_RING: RingBuffer = RingBuffer::new();
static TX_RING: RingBuffer = RingBuffer::new();
// SAFETY: initialized once during kernel init, valid forever.
static mut RX_BUF: [u8; BUF_SIZE] = [0u8; BUF_SIZE];
static mut TX_BUF: [u8; BUF_SIZE] = [0u8; BUF_SIZE];

// ── 驱动实例存储 ─────────────────────────────────────────────────────

static DRIVER: Once<Arc<ArceOsDriver>> = Once::new();

pub fn driver() -> Arc<ArceOsDriver> {
    DRIVER.get().expect("UART driver not initialized").clone()
}

fn driver_ref() -> &'static ArceOsDriver {
    DRIVER.get().expect("UART driver not initialized").as_ref()
}

// ── QEMU: ISR 包装器 ─────────────────────────────────────────────────

#[cfg(not(feature = "lichee-d1-async-uart"))]
fn uart_isr_wrapper(_irq: usize) {
    let base = NonNull::new(get_uart_mmio_virt().as_mut_ptr()).unwrap();
    uart_16550::async_::isr::uart_isr_handler(
        _irq,
        base,
        || UART_PORT.update_ier(IER::empty(), IER::DATA_READY),
        || UART_PORT.update_ier(IER::empty(), IER::THR_EMPTY),
    );
}

/// Zero-argument QEMU UART device handler for `axhal::irq::register`.
#[cfg(not(feature = "lichee-d1-async-uart"))]
fn qemu_uart_irq_handler() {
    let desc = platform::descriptor();
    uart_isr_wrapper(desc.console.irq.unwrap_or(10));
}

// ── D1: ISR 包装器 ───────────────────────────────────────────────────

#[cfg(feature = "lichee-d1-async-uart")]
fn uart_isr_wrapper(_irq: usize) {
    d1_uart_isr_handler(
        _irq,
        &D1_UART_PORT,
        || D1_UART_PORT.update_ier(IER::empty(), IER::DATA_READY),
        || D1_UART_PORT.update_ier(IER::empty(), IER::THR_EMPTY),
    );
}

#[cfg(feature = "lichee-d1-async-uart")]
fn d1_uart_irq_handler() {
    uart_isr_wrapper(axconfig::devices::UART_IRQ);
}

// ── 初始化 ───────────────────────────────────────────────────────────

pub fn init_uart_hardware() {
    let desc = platform::descriptor();

    #[cfg(not(feature = "lichee-d1-async-uart"))]
    {
        match axmm::iomap(PhysAddr::from(desc.console.base_paddr), 0x1000) {
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
    }

    #[cfg(feature = "lichee-d1-async-uart")]
    {
        ax_println!("[UART INIT] D1 async UART init");
    }

    let base_ptr = get_uart_mmio_virt().as_ptr();

    // ── QEMU: raw byte probe at stride 1 ──────────────────────────
    #[cfg(not(feature = "lichee-d1-async-uart"))]
    {
        ax_println!("[UART INIT] Trying raw read at base+5 (stride 1, LSR)...");
        let lsr_raw: u8 = unsafe { base_ptr.add(5).read_volatile() };
        ax_println!("[UART INIT] ✅ Raw LSR read: {:#02x}", lsr_raw);

        ax_println!("[UART INIT] Trying uart_16550 crate access...");
        let mut uart = uart_instance().lock();
        log_uart_state(&mut uart);
        let isr = uart.isr();
        let fifo_enabled = isr.contains(ISR::FIFOS_ENABLED0 | ISR::FIFOS_ENABLED1);
        ax_println!(
            "[UART INIT] FCR: FIFO enabled={}, trigger level via ISR bits 7-6",
            fifo_enabled
        );
        drop(uart);
    }

    // ── D1: skip byte probe, use 32-bit MMIO ───────────────────────
    #[cfg(feature = "lichee-d1-async-uart")]
    {
        // D1 DW APB UART LSR verify via 32-bit read
        let d1_port = &*D1_UART_PORT;
        d1_port.init_interrupt_mode();
        let (ier, iir, lsr) = d1_port.debug_regs();
        ax_println!(
            "[UART INIT] D1 MMIO base={:?} stride={} IER={:#x} IIR={:#x} LSR={:#x}",
            base_ptr,
            desc.console.reg_stride,
            ier,
            iir,
            lsr
        );
    }

    // Step 3: Initialize ring buffers
    unsafe {
        RX_RING.init(addr_of_mut!(RX_BUF).cast::<u8>(), BUF_SIZE);
        TX_RING.init(addr_of_mut!(TX_BUF).cast::<u8>(), BUF_SIZE);
    }

    // Step 4: Create async driver
    let rx = unsafe { RingBufRx::<ArceOsWakerSet>::new(&RX_RING) };
    let tx = unsafe { RingBufTx::<ArceOsWakerSet>::new(&TX_RING) };

    #[cfg(not(feature = "lichee-d1-async-uart"))]
    let uart_port: &'static ArceOsUartPort = &UART_PORT;
    #[cfg(feature = "lichee-d1-async-uart")]
    let uart_port: &'static ArceOsD1UartPort = &D1_UART_PORT;

    let driver = Arc::new(ArceOsDriver::new(rx, tx, uart_port));
    DRIVER.call_once(|| driver);

    // Step 5: Register ISR
    #[cfg(not(feature = "lichee-d1-async-uart"))]
    {
        let irq = desc.console.irq.expect("QEMU console IRQ must be known");
        if !axhal::irq::register(irq, qemu_uart_irq_handler) {
            panic!("[UART INIT] Failed to register UART IRQ {} handler", irq);
        }
        ax_println!(
            "[UART INIT] QEMU UART IRQ {} registered as device handler, buffers={}KBx2",
            irq,
            BUF_SIZE / 1024
        );
    }
    #[cfg(feature = "lichee-d1-async-uart")]
    {
        let registered = axhal::irq::register(axconfig::devices::UART_IRQ, d1_uart_irq_handler);
        ax_println!(
            "[UART INIT] D1 UART IRQ {} registered={}, buffers={}KBx2",
            axconfig::devices::UART_IRQ,
            registered,
            BUF_SIZE / 1024
        );
    }

    // Step 6: Start copier tasks (started after benchmark in entry.rs)
    ax_println!("[UART INIT] async UART hardware initialized (copiers not started yet)");
}

/// Start RX and TX copier tasks. Must be called after startup benchmarks
/// complete to avoid SPSC producer conflicts on the ring buffers.
///
/// # Safety
///
/// The caller must invoke this function exactly once after
/// [`init_uart_hardware`] and after all direct ring benchmarks complete.
pub unsafe fn start_copiers() {
    // SAFETY: The caller guarantees one startup per direction and that the
    // pre-copier benchmark no longer accesses either ring.
    unsafe {
        driver_ref().start_rx_copier();
        driver_ref().start_tx_copier();
    }
    ax_println!("[UART INIT] async UART copiers started");
}

// ── QEMU: 寄存器状态日志 ──────────────────────────────────────────────

#[cfg(not(feature = "lichee-d1-async-uart"))]
fn log_uart_state(uart: &mut Uart16550<MmioBackend>) {
    use uart_16550::spec::registers::{ISR, LSR};

    let ier = uart.ier();
    let isr = uart.isr();
    let lsr = uart.lsr();

    info!(
        "[UART INIT] IER={:02x} ISR={:02x} LSR={:02x}",
        ier.bits(),
        isr.bits(),
        lsr.bits()
    );

    if !ier.contains(IER::DATA_READY) {
        info!("[UART INIT] ⚠️ RX interrupt NOT enabled!");
    }
    if !ier.contains(IER::THR_EMPTY) {
        info!("[UART INIT] ⚠️ TX interrupt NOT enabled!");
    } else {
        info!("[UART INIT] ✅ TX interrupt enabled (AsyncUart needs this)");
    }

    if isr.contains(ISR::FIFOS_ENABLED0 | ISR::FIFOS_ENABLED1) {
        info!("[UART INIT] ✅ FIFO enabled (16 bytes)");
    } else {
        info!("[UART INIT] ⚠️ FIFO NOT enabled!");
    }

    if lsr.contains(LSR::TRANSMITTER_EMPTY) {
        info!("[UART INIT] ✅ TX transmitter empty (ready to send)");
    }
}
