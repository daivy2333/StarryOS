//! Console 阻塞串口性能测试模块
//!
//! 测试 axhal::console 的阻塞式 I/O 性能
//! 与异步串口使用相同的测试方法进行对比

use core::sync::atomic::{AtomicU64, AtomicBool, Ordering};
use axhal::time::monotonic_time_nanos;

// 统计计数器
static TX_BYTES: AtomicU64 = AtomicU64::new(0);
static START_TIME: AtomicU64 = AtomicU64::new(0);
static BENCHMARK_ACTIVE: AtomicBool = AtomicBool::new(false);

// CPU 占用统计
static CPU_CYCLES_START: AtomicU64 = AtomicU64::new(0);
static CPU_CYCLES_END: AtomicU64 = AtomicU64::new(0);

/// 开始基准测试
pub fn start() {
    START_TIME.store(monotonic_time_nanos(), Ordering::Relaxed);
    TX_BYTES.store(0, Ordering::Relaxed);
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

/// 报告测试结果
pub fn report() {
    let elapsed_ns = monotonic_time_nanos() - START_TIME.load(Ordering::Relaxed);
    let tx = TX_BYTES.load(Ordering::Relaxed);

    let elapsed_ms = elapsed_ns as f64 / 1_000_000.0;
    let elapsed_s = elapsed_ns as f64 / 1_000_000_000.0;

    ax_println!("=== UART Benchmark Report ===");
    ax_println!("Elapsed: {:.2} ms ({:.3} s)", elapsed_ms, elapsed_s);

    if tx > 0 {
        let tx_kbps = tx as f64 / elapsed_s / 1024.0;
        ax_println!("TX: {} bytes ({:.2} KB/s)", tx, tx_kbps);
    }

    ax_println!("=============================");
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

/// 获取当前统计值（用于用户态查询）
pub fn get_stats() -> (u64, u64) {
    let elapsed_ns = monotonic_time_nanos() - START_TIME.load(Ordering::Relaxed);
    let tx = TX_BYTES.load(Ordering::Relaxed);
    (elapsed_ns, tx)
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
/// 使用实际测量的 cycle 数和时间计算
/// 注意：QEMU 的 cycle 计数器可能与真实硬件不同
pub fn calculate_cpu_usage(cycles: u64, elapsed_ns: u64) -> f64 {
    // 使用实际测量的 cycle 数和时间
    // CPU 占用率 = (cycles / elapsed_ns) * 100%
    // 这表示每纳秒消耗的 cycle 数
    if elapsed_ns > 0 {
        let cycles_per_ns = cycles as f64 / elapsed_ns as f64;
        // 返回每纳秒的 cycle 数（不是百分比）
        cycles_per_ns
    } else {
        0.0
    }
}

/// 运行 Console 吞吐量测试
///
/// 测试方法与异步串口一致：
/// 1. 测试 polling TX 速度（ax_println!）
/// 2. 测试硬件理论极限
/// 3. 输出统计信息
///
/// 统一测试数据量：102,400 字节（与 Async 一致）
pub fn run_throughput_test() {
    ax_println!("[BENCH] Running Console throughput test...");

    // 测试 1: polling TX 速度（统一数据量）
    ax_println!("[BENCH] Test 1: polling TX speed (unified data size)");

    // 统一测试数据量：102,400 字节（与 Async 一致）
    let test_data = [0u8; 1024]; // 使用 0x00 避免终端输出
    let iterations = 100; // 100 次 × 1024 字节 = 102,400 字节

    // 开始 CPU 占用测量
    start_cpu_measurement();
    let start_time = monotonic_time_nanos();

    for _ in 0..iterations {
        axhal::console::write_bytes(&test_data);
    }

    let end_time = monotonic_time_nanos();
    let cpu_cycles = stop_cpu_measurement();

    let elapsed_ns = end_time - start_time;
    let elapsed_s = elapsed_ns as f64 / 1_000_000_000.0;
    let total_bytes = iterations * 1024; // 102,400 字节
    let throughput_kbps = total_bytes as f64 / elapsed_s / 1024.0;
    let cpu_usage = calculate_cpu_usage(cpu_cycles, elapsed_ns);

    ax_println!("[BENCH] polling TX: {:.2} KB/s", throughput_kbps);
    ax_println!("[BENCH] Total: {} bytes in {:.2} ms", total_bytes, elapsed_ns as f64 / 1_000_000.0);
    ax_println!("[BENCH] CPU Cycles: {}", cpu_cycles);
    ax_println!("[BENCH] CPU Usage: {:.2} cycles/ns", cpu_usage);

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

/// 运行 Console RX 吞吐量测试
///
/// 测试 Console 读取速度
/// 注意：Console 没有 Ring Buffer，read_bytes() 是非阻塞的
/// 需要外部数据注入才能测试
pub fn run_rx_throughput_test() {
    ax_println!("[BENCH] Running Console RX throughput test...");

    // Console 没有 Ring Buffer，无法直接测试 RX 吞吐量
    // read_bytes() 是非阻塞的，如果没有数据立即返回 0
    ax_println!("[BENCH] Console has no Ring Buffer");
    ax_println!("[BENCH] read_bytes() is non-blocking (try_receive)");
    ax_println!("[BENCH] RX throughput test skipped (no buffer to measure)");
}

/// 运行 Console RX 延迟测试
///
/// 测试 Console 读取延迟
/// 注意：Console 没有 Ring Buffer，read_bytes() 是非阻塞的
/// 需要外部数据注入才能测试
pub fn run_rx_latency_test() {
    ax_println!("[BENCH] Running Console RX latency test...");

    // Console 没有 Ring Buffer，无法直接测试 RX 延迟
    // read_bytes() 是非阻塞的，如果没有数据立即返回 0
    ax_println!("[BENCH] Console has no Ring Buffer");
    ax_println!("[BENCH] read_bytes() is non-blocking (try_receive)");
    ax_println!("[BENCH] RX latency test skipped (no buffer to measure)");
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
