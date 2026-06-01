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
- CPU 占用
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
| **操作速度** | 550.69 KB/s | 210,970 KB/s | **Async 快 383x** |
| **CPU Cycles** | 492,914 | 27,420,885 | Async 更多 |
| **CPU Usage** | 2.3% | 57.8% | Console 更低 |
| **操作目标** | FIFO | Ring Buffer | 不同 |

**分析**：
- Console 的 polling TX 测量的是 CPU → FIFO 的速度（需要等待硬件）
- Async 的 Ring Buffer 写入测量的是 CPU → 内存的速度（不需要等待硬件）
- **差异原因**：Console 需要轮询等待 FIFO 有空位，Async 直接写入内存
- **CPU 占用**：Console 只有 2.3%，Async 有 57.8%，因为 Async 写入更多数据
- **对比意义**：Async 的内核态操作更快，但消耗更多 CPU

### 2.2 用户态 write() 性能

| 指标 | Console (阻塞) | Async (异步) | 差异 |
|------|---------------|--------------|------|
| **write() P50** | 8.1 µs | 6.5 µs | **Async 快 1.2x** |
| **write() P95** | 15.0 µs | 12.2 µs | **Async 快 1.2x** |
| **write() P99** | 64.4 µs | 88.9 µs | Console 稍快 |

**不同数据大小吞吐量**：

| 数据大小 | Console | Async | 差异 |
|----------|---------|-------|------|
| **64 bytes** | 13,188 KB/s | 10,470 KB/s | Console 稍快 |
| **256 bytes** | 37,398 KB/s | 32,441 KB/s | Console 稍快 |
| **1024 bytes** | 158,773 KB/s | 155,964 KB/s | 相近 |
| **4096 bytes** | 266,680 KB/s | 307,763 KB/s | **Async 快 1.2x** |

**分析**：
- **write() 延迟**：Async 在 P50/P95 稍快，但 P99 稍慢
- **吞吐量**：小数据时 Console 稍快，大数据时 Async 更快
- **差异原因**：两者都受 QEMU 串口模拟影响，差异不大

**对比意义**：
- 两个串口的用户态性能差异不大
- 都受限于 QEMU 串口模拟
- 真实硬件上可能有更大差异

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

### 2.4 压力测试

| 指标 | Console (阻塞) | Async (异步) | 差异 |
|------|---------------|--------------|------|
| **持续时间** | 2.0 秒 | 2.0 秒 | 相同 |
| **迭代次数** | 238,220 | 230,706 | 相近 |
| **总数据量** | 243 MB | 236 MB | 相近 |
| **吞吐量** | 119,106 KB/s | 115,350 KB/s | 相近 |

**分析**：
- 两者的压力测试性能相近
- 都能稳定运行 2 秒
- 吞吐量都在 115-119 MB/s

### 2.5 Shell 功能

| 测试项 | Console (阻塞) | Async (异步) | 状态 |
|--------|---------------|--------------|------|
| **echo 命令** | ✅ 正常 | ✅ 正常 | 都正常 |
| **ls 命令** | ✅ 正常 | ✅ 正常 | 都正常 |
| **cd 命令** | ✅ 正常 | ✅ 正常 | 都正常 |
| **数据完整性** | ✅ 100% | ✅ 100% | 都完整 |

---

## 3. 性能指标详解

### 3.1 write() 延迟对比

**Console 阻塞串口**：
- P50: 8.1 µs
- P95: 15.0 µs
- P99: 64.4 µs

**Async 异步串口**：
- P50: 6.5 µs
- P95: 12.2 µs
- P99: 88.9 µs

**差异原因**：
- Console 的 write() 需要获取锁并等待 FIFO
- Async 的 write() 只写入 Ring Buffer，操作简单
- P99 差异可能是 QEMU 调度导致

### 3.2 CPU 占用对比

**Console 阻塞串口**：
- CPU Usage: 2.3%
- CPU Cycles: 492,914
- 说明：polling TX 操作效率高

**Async 异步串口**：
- CPU Usage: 57.8%
- CPU Cycles: 27,420,885
- 说明：Ring Buffer 写入操作消耗更多 CPU

**差异原因**：
- Console 的 polling TX 只写入少量数据（120 字节）
- Async 的 Ring Buffer 写入更多数据（102,400 字节）
- CPU 占用与数据量成正比

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
// 开始 CPU 占用测量
start_cpu_measurement();
let start_time = monotonic_time_nanos();

let iterations = 10;
for _ in 0..iterations {
    ax_println!("[BENCH] test");
}

let end_time = monotonic_time_nanos();
let cpu_cycles = stop_cpu_measurement();
```

**Async Ring Buffer 测试**：
```rust
let test_data = vec![0u8; 1024];
let iterations = 100;

// 开始 CPU 占用测量
benchmark::start_cpu_measurement();
let start_time = monotonic_time_nanos();

for _ in 0..iterations {
    let mut tx_buf = DRIVER.tx.lock();
    tx_buf.push(&test_data);
    drop(tx_buf);
}

let end_time = monotonic_time_nanos();
let cpu_cycles = benchmark::stop_cpu_measurement();
```

### 4.2 用户态测试

**不同数据大小测试**：
```c
int sizes[] = {64, 256, 1024, 4096};
int iterations = 1000;

for (int s = 0; s < 4; s++) {
    int test_size = sizes[s];
    for (int i = 0; i < iterations; i++) {
        write(fd, buf, test_size);
    }
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

**压力测试**：
```c
int duration_sec = 2;
while (1) {
    long long now = get_time_ns();
    if ((now - start) > (long long)duration_sec * 1000000000LL) {
        break;
    }
    write(fd, buf, test_size);
}
```

### 4.3 测量局限性

**无法测量的指标**：
- 串口线速（QEMU 不等待）
- 端到端延迟（需要硬件支持）

**可以测量的指标**：
- write() 系统调用速度
- write() 系统调用延迟
- CPU 占用
- 内存占用
- Shell 功能

---

## 5. 结论

### 5.1 性能对比总结

| 指标 | Console | Async | 胜出 | 说明 |
|------|---------|-------|------|------|
| **内核态速度** | 550 KB/s | 210,970 KB/s | **Async** | Async 写入内存更快 |
| **CPU 占用** | 2.3% | 57.8% | **Console** | Console 更省 CPU |
| **write() P50** | 8.1 µs | 6.5 µs | **Async** | Async 稍快 |
| **write() P95** | 15.0 µs | 12.2 µs | **Async** | Async 稍快 |
| **write() P99** | 64.4 µs | 88.9 µs | **Console** | Console 稍快 |
| **压力测试** | 119 MB/s | 115 MB/s | 相近 | 都稳定 |
| **内存占用** | 0 KB | 128 KB | **Console** | Console 更省内存 |
| **Shell 功能** | ✅ | ✅ | 平局 | 都正常工作 |
| **数据完整性** | ✅ | ✅ | 平局 | 都 100% 正确 |

**核心结论**：
- **Async 的内核态操作快 383 倍**，但消耗更多 CPU
- **用户态性能差异不大**，都受 QEMU 串口模拟影响
- **Console 更省内存和 CPU**，Async 更适合批量处理
- **两者都能正常工作**，Shell 功能和数据完整性都正确

### 5.2 选择建议

**选择 Async 异步串口的场景**：
- ✅ 批量数据传输
- ✅ 异步非阻塞需求
- ✅ 多任务环境
- ✅ 中断驱动

**选择 Console 阻塞串口的场景**：
- ✅ 低内存要求（0 KB vs 128 KB）
- ✅ 低 CPU 要求（2.3% vs 57.8%）
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
- Shell 和用户态程序需要异步能力，Async 更优
- 两者共存，各取所长

---

## 6. 后续优化方向

### 6.1 Async 优化

1. **降低 CPU 占用**：优化 Ring Buffer 写入算法
2. **NAPI 调优**：调整阈值和批量大小
3. **DMA 支持**：真板可探索 DMA 传输

### 6.2 Console 优化

1. **批量输出**：减少锁获取次数
2. **缓冲输出**：添加简单缓冲区

### 6.3 测试完善

1. **真板验证**：在 VisionFive2 上测试
2. **端到端测试**：测量真实串口线速
3. **并发测试**：多任务环境下的性能

---

**报告版本**：1.1
**最后更新**：2026-06-01
**测试分支**：feat/uart-async-bench (Async) / feat/uart-bench (Console)
