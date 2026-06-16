//! UART 异步驱动性能测试模块
//!
//! 提供内核态性能统计：ring buffer 吞吐量/延迟、内存占用、NAPI/IRQ 报告。
//! 调用 uart_16550::async_::bench 导出的统计接口。

extern crate alloc;
use alloc::vec;

use crate::drivers::uart_init::{self, BUF_SIZE};

use axhal::time::monotonic_time_nanos;
use uart_16550::async_::bench;

/// 启动时运行完整 benchmark。
pub fn run_startup_benchmark() {
    ax_println!("[BENCH] Running startup benchmark...");

    let driver = uart_init::driver();

    // ── 测试 1: Ring buffer 写入吞吐量 ────────────────────────────
    let test_data = vec![0u8; 1024];
    let iterations = 100;

    let start_time = monotonic_time_nanos();
    for _ in 0..iterations {
        driver.tx.push(&test_data);
    }
    let elapsed_ns = monotonic_time_nanos() - start_time;
    let total_bytes = iterations * 1024;
    let elapsed_s = elapsed_ns as f64 / 1_000_000_000.0;
    let throughput_kbps = total_bytes as f64 / elapsed_s / 1024.0;

    ax_println!(
        "[BENCH] Ring buffer write: {:.2} KB/s ({} bytes in {:.2} ms)",
        throughput_kbps,
        total_bytes,
        elapsed_ns as f64 / 1_000_000.0
    );

    // ── 测试 2: 硬件理论极限 ──────────────────────────────────────
    ax_println!("[BENCH] Hardware line rate: 11.52 KB/s (115200 bps)");
    ax_println!("[BENCH] FIFO depth: 16 bytes");

    // ── 测试 3: 内存占用 ──────────────────────────────────────────
    let driver_size = core::mem::size_of_val(driver.as_ref());
    ax_println!(
        "[BENCH] Ring buffer: {} KB × 2 = {} KB total",
        BUF_SIZE / 1024,
        BUF_SIZE * 2 / 1024
    );
    ax_println!("[BENCH] Driver struct: {} bytes", driver_size);
    ax_println!(
        "[BENCH] Total memory: {} KB",
        (BUF_SIZE * 2 + driver_size) / 1024
    );

    // ── 测试 4: NAPI 配置 ─────────────────────────────────────────
    ax_println!(
        "[BENCH] NAPI threshold: {} consecutive reads",
        bench::NAPI_THRESHOLD
    );
    ax_println!(
        "[BENCH] NAPI batch size: {} bytes",
        bench::NAPI_BATCH_SIZE
    );
    ax_println!(
        "[BENCH] Copier buffer size: {} bytes",
        bench::COPIER_BUF_SIZE
    );

    // ── 测试 5: IRQ 统计 ──────────────────────────────────────────
    let irq_count = bench::irq_count();
    ax_println!("[BENCH] IRQ count: {}", irq_count);
    if elapsed_s > 0.0 && irq_count > 1 {
        ax_println!(
            "[BENCH] IRQ frequency: {:.2} IRQ/s",
            irq_count as f64 / elapsed_s
        );
    }

    // ── 测试 6: Ring buffer 读取吞吐量 ────────────────────────────
    run_rx_throughput_test(&driver.rx);

    // ── 测试 7: Ring buffer 读取延迟 ──────────────────────────────
    run_rx_latency_test(&driver.rx);

    ax_println!("[BENCH] Startup benchmark complete");
    ax_println!("[BENCH] Note: actual throughput limited by UART line rate (11.52 KB/s)");
}

use uart_16550::async_::ring_buffer::RingBufRx;
use uart_16550::os::OsWakerSet;

fn run_rx_throughput_test<W: OsWakerSet>(rx: &RingBufRx<W>) {
    ax_println!("[BENCH] Running RX ring buffer throughput test...");

    let test_data = vec![0u8; 1024];
    let iterations = 100;

    for _ in 0..iterations {
        rx.push_batch(&test_data);
    }

    let start_time = monotonic_time_nanos();
    let mut read_buf = vec![0u8; 1024];
    let mut total_read = 0;
    for _ in 0..iterations {
        total_read += rx.pop(&mut read_buf);
    }
    let elapsed_ns = monotonic_time_nanos() - start_time;

    let elapsed_s = elapsed_ns as f64 / 1_000_000_000.0;
    let throughput_kbps = total_read as f64 / elapsed_s / 1024.0;

    ax_println!(
        "[BENCH] RX ring buffer read: {:.2} KB/s ({} bytes in {:.2} ms)",
        throughput_kbps,
        total_read,
        elapsed_ns as f64 / 1_000_000.0
    );
}

fn run_rx_latency_test<W: OsWakerSet>(rx: &RingBufRx<W>) {
    ax_println!("[BENCH] Running RX ring buffer latency test...");

    let iterations = 100;
    let mut latencies = vec![0u64; iterations];
    let mut successful = 0;

    for i in 0..iterations {
        let test_byte = [b'A' + (i % 26) as u8];
        rx.push_batch(&test_byte);

        let mut read_buf = [0u8; 1];
        let start = monotonic_time_nanos();
        let n = rx.pop(&mut read_buf);
        let end = monotonic_time_nanos();

        if n == 1 {
            latencies[successful] = end - start;
            successful += 1;
        }
    }

    if successful == 0 {
        ax_println!("[BENCH] No successful reads");
        return;
    }

    latencies.truncate(successful);
    latencies.sort();

    let sum: u64 = latencies.iter().sum();
    let p50 = latencies[successful * 50 / 100];
    let p95 = latencies[successful * 95 / 100];
    let p99 = latencies[successful * 99 / 100];

    ax_println!(
        "[BENCH] RX latency (n={}): min={}ns avg={}ns P50={}ns P95={}ns P99={}ns",
        successful,
        latencies[0],
        sum / successful as u64,
        p50,
        p95,
        p99
    );
}
