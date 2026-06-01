//! UART 性能测试模块
//!
//! 提供内核态性能统计和测试接口
//! 用于测量异步串口驱动的吞吐量、延迟、内存占用、CPU 占用等指标

use core::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use axhal::time::monotonic_time_nanos;

// 统计计数器
static TX_BYTES: AtomicU64 = AtomicU64::new(0);
static RX_BYTES: AtomicU64 = AtomicU64::new(0);
static START_TIME: AtomicU64 = AtomicU64::new(0);
static BENCHMARK_ACTIVE: AtomicBool = AtomicBool::new(false);

// CPU 占用统计
static CPU_CYCLES_START: AtomicU64 = AtomicU64::new(0);
static CPU_CYCLES_END: AtomicU64 = AtomicU64::new(0);

/// 开始基准测试
pub fn start() {
    START_TIME.store(monotonic_time_nanos(), Ordering::Relaxed);
    TX_BYTES.store(0, Ordering::Relaxed);
    RX_BYTES.store(0, Ordering::Relaxed);
    BENCHMARK_ACTIVE.store(true, Ordering::Relaxed);
    ax_println!("[BENCH] Started");
}

/// 停止基准测试并报告结果
pub fn stop() {
    BENCHMARK_ACTIVE.store(false, Ordering::Relaxed);
    report();
}

/// 记录 TX 字节
pub fn record_tx(bytes: u64) {
    if BENCHMARK_ACTIVE.load(Ordering::Relaxed) {
        TX_BYTES.fetch_add(bytes, Ordering::Relaxed);
    }
}

/// 记录 RX 字节
pub fn record_rx(bytes: u64) {
    if BENCHMARK_ACTIVE.load(Ordering::Relaxed) {
        RX_BYTES.fetch_add(bytes, Ordering::Relaxed);
    }
}

/// 报告测试结果
pub fn report() {
    let elapsed_ns = monotonic_time_nanos() - START_TIME.load(Ordering::Relaxed);
    let tx = TX_BYTES.load(Ordering::Relaxed);
    let rx = RX_BYTES.load(Ordering::Relaxed);

    let elapsed_ms = elapsed_ns as f64 / 1_000_000.0;
    let elapsed_s = elapsed_ns as f64 / 1_000_000_000.0;

    ax_println!("=== UART Benchmark Report ===");
    ax_println!("Elapsed: {:.2} ms ({:.3} s)", elapsed_ms, elapsed_s);

    if tx > 0 {
        let tx_kbps = tx as f64 / elapsed_s / 1024.0;
        ax_println!("TX: {} bytes ({:.2} KB/s)", tx, tx_kbps);
    }

    if rx > 0 {
        let rx_kbps = rx as f64 / elapsed_s / 1024.0;
        ax_println!("RX: {} bytes ({:.2} KB/s)", rx, rx_kbps);
    }

    ax_println!("=============================");
}

/// 内存使用统计
pub fn memory_usage() {
    use super::ring_buffer::BUF_SIZE;

    let rx_buf = BUF_SIZE; // 64 KB
    let tx_buf = BUF_SIZE; // 64 KB
    let driver = core::mem::size_of::<super::async_driver::AsyncUartDriver>();

    ax_println!("=== Memory Usage ===");
    ax_println!("RX Buffer: {} KB ({} bytes)", rx_buf / 1024, rx_buf);
    ax_println!("TX Buffer: {} KB ({} bytes)", tx_buf / 1024, tx_buf);
    ax_println!("Driver Struct: {} bytes", driver);
    ax_println!("Total: {} KB ({} bytes)", (rx_buf + tx_buf + driver) / 1024, rx_buf + tx_buf + driver);
    ax_println!("====================");
}

/// 获取当前统计值（用于用户态查询）
pub fn get_stats() -> (u64, u64, u64) {
    let elapsed_ns = monotonic_time_nanos() - START_TIME.load(Ordering::Relaxed);
    let tx = TX_BYTES.load(Ordering::Relaxed);
    let rx = RX_BYTES.load(Ordering::Relaxed);
    (elapsed_ns, tx, rx)
}

/// 检查基准测试是否激活
pub fn is_active() -> bool {
    BENCHMARK_ACTIVE.load(Ordering::Relaxed)
}

/// CPU 占用测量
///
/// 使用 RISC-V cycle 计数器测量 CPU 周期数
pub fn measure_cpu_cycles<F: FnOnce()>(f: F) -> u64 {
    // 读取 RISC-V cycle 计数器
    let start: u64;
    let end: u64;

    unsafe {
        core::arch::asm!("csrr {}, cycle", out(reg) start);
        f();
        core::arch::asm!("csrr {}, cycle", out(reg) end);
    }

    end - start
}

/// 记录 CPU 周期开始
pub fn start_cpu_measurement() {
    let cycle: u64;
    unsafe {
        core::arch::asm!("csrr {}, cycle", out(reg) cycle);
    }
    CPU_CYCLES_START.store(cycle, Ordering::Relaxed);
}

/// 记录 CPU 周期结束并返回周期数
pub fn stop_cpu_measurement() -> u64 {
    let cycle: u64;
    unsafe {
        core::arch::asm!("csrr {}, cycle", out(reg) cycle);
    }
    CPU_CYCLES_END.store(cycle, Ordering::Relaxed);

    let start = CPU_CYCLES_START.load(Ordering::Relaxed);
    let end = CPU_CYCLES_END.load(Ordering::Relaxed);

    if end > start {
        end - start
    } else {
        0
    }
}

/// 计算 CPU 占用率
///
/// 假设 CPU 频率为 1GHz（QEMU 默认）
pub fn calculate_cpu_usage(cycles: u64, elapsed_ns: u64) -> f64 {
    // 假设 CPU 频率为 1GHz
    let cpu_freq_hz = 1_000_000_000.0;
    let elapsed_s = elapsed_ns as f64 / 1_000_000_000.0;
    let expected_cycles = cpu_freq_hz * elapsed_s;

    if expected_cycles > 0.0 {
        (cycles as f64 / expected_cycles) * 100.0
    } else {
        0.0
    }
}

/// 中断频率统计
pub fn get_irq_frequency(elapsed_ns: u64) -> f64 {
    let irq_count = super::uart_init::get_irq_count();
    let elapsed_s = elapsed_ns as f64 / 1_000_000_000.0;

    if elapsed_s > 0.0 {
        irq_count as f64 / elapsed_s
    } else {
        0.0
    }
}

/// NAPI 效果报告
pub fn report_napi_effect() {
    let irq_count = super::uart_init::get_irq_count();
    let (_, tx, rx) = get_stats();

    ax_println!("=== NAPI Effect Report ===");
    ax_println!("IRQ Count: {}", irq_count);
    ax_println!("TX Bytes: {}", tx);
    ax_println!("RX Bytes: {}", rx);

    if tx > 0 || rx > 0 {
        let total_bytes = tx + rx;
        let irq_per_kb = if total_bytes > 0 {
            irq_count as f64 / (total_bytes as f64 / 1024.0)
        } else {
            0.0
        };
        ax_println!("IRQs per KB: {:.2}", irq_per_kb);
    }

    ax_println!("NAPI Threshold: {}", super::uart_init::NAPI_THRESHOLD);
    ax_println!("NAPI Batch Size: {}", super::uart_init::NAPI_BATCH_SIZE);
    ax_println!("==========================");
}
