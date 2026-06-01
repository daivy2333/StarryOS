//! UART 性能测试模块
//!
//! 提供内核态性能统计和测试接口
//! 用于测量异步串口驱动的吞吐量、延迟、内存占用等指标

use core::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use axhal::time::monotonic_time_nanos;

// 统计计数器
static TX_BYTES: AtomicU64 = AtomicU64::new(0);
static RX_BYTES: AtomicU64 = AtomicU64::new(0);
static START_TIME: AtomicU64 = AtomicU64::new(0);
static BENCHMARK_ACTIVE: AtomicBool = AtomicBool::new(false);

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
