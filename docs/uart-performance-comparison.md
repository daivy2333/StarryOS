# UART 串口性能对比报告

> 项目：StarryOS
> 分支：feat/uart-async-dev2
> 日期：2026-06-01
> 测试环境：QEMU riscv64-virt

---

## 1. 测试概述

### 1.1 测试目标

对比 Console 阻塞串口和 Async 异步串口的性能差异，包括：
- 内核态操作速度
- 用户态 write() 速度和延迟
- 内存占用
- Shell 功能

### 1.2 测试环境

| 项目 | 配置 |
|------|------|
| **目标架构** | RISC-V 64-bit |
| **平台** | qemu-riscv64-virt |
| **串口硬件** | NS16550 UART |
| **波特率** | 115200 bps |
| **FIFO 深度** | 16 字节 |
| **构建模式** | release (optimized) |

### 1.3 架构对比

**Console 阻塞串口**：
```
用户态 write()
    ↓
axhal::console::write_bytes()
    ↓
逐字节 send_raw()
    ↓
轮询等待 THR_EMPTY
    ↓
写入 THR 寄存器
```

**Async 异步串口**：
```
用户态 write()
    ↓
AsyncUartWriter::write()
    ↓
Ring Buffer push
    ↓
TX copier 任务
    ↓
批量 send_bytes()
    ↓
ISR 中断驱动
```

---

## 2. 测试结果对比

### 2.1 内核态性能

| 指标 | Console (阻塞) | Async (异步) | 差异 |
|------|---------------|--------------|------|
| **操作类型** | polling TX | Ring Buffer 写入 | 不同实现 |
| **操作速度** | 483 KB/s | 206,654 KB/s | Async 快 428x |
| **操作目标** | FIFO | Ring Buffer | 不同 |

**分析**：
- Console 的 polling TX 测量的是 CPU → FIFO 的速度（需要等待硬件）
- Async 的 Ring Buffer 写入测量的是 CPU → 内存的速度（不需要等待硬件）
- **差异原因**：Console 需要轮询等待 FIFO 有空位，Async 直接写入内存
- **对比意义**：Async 的内核态操作更快，因为它避免了硬件等待

### 2.2 用户态 write() 性能

| 指标 | Console (阻塞) | Async (异步) | 差异 |
|------|---------------|--------------|------|
| **write() 速度** | 572 KB/s | 46,296 KB/s | **Async 快 80x** |
| **write() P50** | 16.3 µs | 6.9 µs | **Async 快 2.4x** |
| **write() P95** | 29.5 µs | 10.8 µs | **Async 快 2.7x** |
| **write() P99** | 278.4 µs | 244.6 µs | Async 稍快 |

**分析**：
- **write() 速度**：Async 快 80 倍
  - Console：write() 需要轮询等待 FIFO 有空位，同步阻塞
  - Async：write() 只写入 Ring Buffer，立即返回
- **write() 延迟**：Async 快 2.4-2.7 倍
  - Console：每个字节都需要获取锁并等待硬件
  - Async：只写入 Ring Buffer，操作简单快速

**对比意义**：
- 两个串口都是执行相同的用户态操作：write(fd, buf, len)
- **Async 的 write() 更快，因为它不需要等待硬件 FIFO**
- 这是有意义的性能对比，反映了实际使用中的差异

### 2.3 内存占用

| 指标 | Console (阻塞) | Async (异步) | 差异 |
|------|---------------|--------------|------|
| **RX Buffer** | 0 KB | 64 KB | Async 需要缓冲 |
| **TX Buffer** | 0 KB | 64 KB | Async 需要缓冲 |
| **驱动结构体** | 0 KB | 640 字节 | Async 有额外结构 |
| **总计** | 0 KB | 128 KB | **Console 更省内存** |

**分析**：
- Console：无缓冲区，直接硬件访问
- Async：128 KB Ring Buffer，支持批量处理

### 2.4 Shell 功能

| 测试项 | Console (阻塞) | Async (异步) | 状态 |
|--------|---------------|--------------|------|
| **echo 命令** | ✅ 正常 | ✅ 正常 | 都正常 |
| **ls 命令** | ✅ 正常 | ✅ 正常 | 都正常 |
| **cd 命令** | ✅ 正常 | ✅ 正常 | 都正常 |
| **数据完整性** | ✅ 100% | ✅ 100% | 都完整 |

---

## 3. 性能指标详解

### 3.1 write() 速度对比

**Console 阻塞串口**：
```
write() → axhal::console::write_bytes() → send_raw() → 轮询等待 FIFO
```
- 每个字节都需要轮询等待 FIFO 有空位
- 同步阻塞，CPU 忙等
- 速度：572 KB/s

**Async 异步串口**：
```
write() → AsyncUartWriter::write() → Ring Buffer push → 立即返回
```
- 只写入 Ring Buffer，立即返回
- 异步非阻塞，CPU 可做其他事
- 速度：46,296 KB/s

**差异原因**：
- Console 需要等待硬件，Async 只写内存
- Async 的 write() 不等待数据真正发送

### 3.2 write() 延迟对比

**Console 阻塞串口**：
- P50: 16.3 µs
- P95: 29.5 µs
- P99: 278.4 µs

**Async 异步串口**：
- P50: 6.9 µs
- P95: 10.8 µs
- P99: 244.6 µs

**差异原因**：
- Console 的 write() 需要获取锁并等待 FIFO
- Async 的 write() 只写入 Ring Buffer，操作简单

### 3.3 内存占用对比

**Console 阻塞串口**：
- 无缓冲区
- 直接硬件访问
- 每个字节都获取锁

**Async 异步串口**：
- 128 KB Ring Buffer（RX/TX 各 64 KB）
- 支持批量处理
- 中断驱动

**权衡**：
- Console：省内存，但性能低
- Async：耗内存，但性能高

---

## 4. 测试方法说明

### 4.1 内核态测试

**Console polling TX 测试**：
```rust
let iterations = 10;
let start_time = monotonic_time_nanos();

for _ in 0..iterations {
    ax_println!("[BENCH] test");
}

let end_time = monotonic_time_nanos();
let throughput_kbps = total_bytes as f64 / elapsed_s / 1024.0;
```

**Async Ring Buffer 测试**：
```rust
let test_data = vec![0u8; 1024];
let iterations = 100;
let start_time = monotonic_time_nanos();

for _ in 0..iterations {
    let mut tx_buf = DRIVER.tx.lock();
    tx_buf.push(&test_data);
    drop(tx_buf);
}

let end_time = monotonic_time_nanos();
let throughput_kbps = total_bytes as f64 / elapsed_s / 1024.0;
```

### 4.2 用户态测试

**write() 速度测试**：
```c
const int test_size = 1024;
const int iterations = 10;

for (int i = 0; i < iterations; i++) {
    write(fd, buf, test_size);
}
```

**write() 延迟测试**：
```c
for (int i = 0; i < 100; i++) {
    clock_gettime(CLOCK_MONOTONIC, &start);
    write(fd, &tx, 1);
    clock_gettime(CLOCK_MONOTONIC, &end);
    latencies[i] = end - start;
}
```

### 4.3 测量局限性

**无法测量的指标**：
- 串口线速（QEMU 不等待）
- 端到端延迟（需要硬件支持）
- CPU 占用（需要性能计数器）

**可以测量的指标**：
- write() 系统调用速度
- write() 系统调用延迟
- 内存占用
- Shell 功能

---

## 5. 结论

### 5.1 性能对比总结

| 指标 | Console | Async | 胜出 | 说明 |
|------|---------|-------|------|------|
| **write() 速度** | 572 KB/s | 46,296 KB/s | **Async** | Async 不等待硬件 |
| **write() P50** | 16.3 µs | 6.9 µs | **Async** | Async 操作更简单 |
| **write() P95** | 29.5 µs | 10.8 µs | **Async** | Async 无硬件等待 |
| **write() P99** | 278.4 µs | 244.6 µs | **Async** | 都受 QEMU 调度影响 |
| **内存占用** | 0 KB | 128 KB | **Console** | Async 有 Ring Buffer |
| **Shell 功能** | ✅ | ✅ | 平局 | 都正常工作 |
| **数据完整性** | ✅ | ✅ | 平局 | 都 100% 正确 |

**核心结论**：
- **Async 的 write() 快 80 倍**，因为它不需要等待硬件 FIFO
- **Console 更省内存**，因为它没有 Ring Buffer
- **两者都能正常工作**，Shell 功能和数据完整性都正确

### 5.2 选择建议

**选择 Async 异步串口的场景**：
- ✅ 高性能要求（write() 快 80 倍）
- ✅ 低延迟要求（P50 快 2.4 倍）
- ✅ 批量数据传输
- ✅ CPU 资源紧张（异步非阻塞）
- ✅ 多任务环境

**选择 Console 阻塞串口的场景**：
- ✅ 低内存要求（0 KB vs 128 KB）
- ✅ 简单实现
- ✅ 启动阶段日志
- ✅ 资源受限系统

### 5.3 实际应用建议

**StarryOS 当前方案**：
- **内核日志**：使用 Console polling TX（ax_println!）
- **Shell I/O**：使用 Async 异步串口
- **用户态程序**：使用 Async 异步串口

**理由**：
- 内核日志需要在启动早期可用，Console 简单可靠
- Shell 和用户态程序需要高性能，Async 更优
- 两者共存，各取所长

---

## 6. 后续优化方向

### 6.1 Async 优化

1. **NAPI 调优**：调整阈值和批量大小
2. **Ring Buffer 优化**：根据实际负载调整大小
3. **DMA 支持**：真板可探索 DMA 传输

### 6.2 Console 优化

1. **批量输出**：减少锁获取次数
2. **缓冲输出**：添加简单缓冲区

### 6.3 测试完善

1. **真板验证**：在 VisionFive2 上测试
2. **CPU 占用测量**：添加性能计数器
3. **中断频率统计**：优化 NAPI 参数

---

**报告版本**：1.0
**最后更新**：2026-06-01
**测试分支**：feat/uart-async-bench (Async) / feat/uart-bench (Console)
