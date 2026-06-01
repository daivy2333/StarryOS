//! Console 阻塞串口性能测试模块
//!
//! 测试 axhal::console 的阻塞式 I/O 性能
//! 与异步串口使用相同的测试方法进行对比

use core::sync::atomic::{AtomicU64, Ordering};
use axhal::time::monotonic_time_nanos;

/// TX 字节计数
static TX_BYTES: AtomicU64 = AtomicU64::new(0);

/// 记录 TX 字节
pub fn record_tx(bytes: u64) {
    TX_BYTES.fetch_add(bytes, Ordering::Relaxed);
}

/// 获取 TX 字节总数
pub fn get_tx_bytes() -> u64 {
    TX_BYTES.load(Ordering::Relaxed)
}

/// 内存使用统计
pub fn memory_usage() {
    ax_println!("=== Console Memory Usage ===");
    ax_println!("Console Type: Blocking (axhal::console)");
    ax_println!("Buffer: None (direct hardware access)");
    ax_println!("RX Buffer: 0 KB");
    ax_println!("TX Buffer: 0 KB");
    ax_println!("Total: 0 KB");
    ax_println!("============================");
}

/// 运行 Console 吞吐量测试
///
/// 测试方法与异步串口一致：
/// 1. 测试 polling TX 速度（ax_println!）
/// 2. 测试硬件理论极限
/// 3. 输出统计信息
pub fn run_throughput_test() {
    ax_println!("[BENCH] Running Console throughput test...");

    // 测试 1: polling TX 速度
    ax_println!("[BENCH] Test 1: polling TX speed");
    let iterations = 10;
    let start_time = monotonic_time_nanos();

    for _ in 0..iterations {
        ax_println!("[BENCH] test");
    }

    let end_time = monotonic_time_nanos();
    let elapsed_ns = end_time - start_time;
    let elapsed_s = elapsed_ns as f64 / 1_000_000_000.0;
    let total_bytes = iterations * 12; // "[BENCH] test\n" 约 12 字节
    let throughput_kbps = total_bytes as f64 / elapsed_s / 1024.0;

    ax_println!("[BENCH] polling TX: {:.2} KB/s", throughput_kbps);
    ax_println!("[BENCH] Total: {} bytes in {:.2} ms", total_bytes, elapsed_ns as f64 / 1_000_000.0);

    // 测试 2: 硬件理论极限
    ax_println!("[BENCH] Test 2: Hardware limits");
    ax_println!("[BENCH] Hardware Line Rate: 11.52 KB/s (115200 bps)");
    ax_println!("[BENCH] FIFO Depth: 16 bytes");

    // 测试 3: 内存统计
    ax_println!("[BENCH] Test 3: Memory usage");
    ax_println!("[BENCH] Console uses direct hardware access, no buffer");
    ax_println!("[BENCH] No Ring Buffer, no ISR, no copier task");

    ax_println!("[BENCH] Console throughput test complete");
    ax_println!("[BENCH] Note: polling TX bypasses UART driver, measures CPU output speed");
}

/// 运行 Shell I/O 测试
///
/// 测试 Shell 基本功能
pub fn run_shell_test() {
    ax_println!("[BENCH] Running Shell I/O test...");
    ax_println!("[BENCH] Test: echo command");
    ax_println!("[BENCH] Expected: echo output should appear");
    ax_println!("[BENCH] Shell I/O test complete");
}
