# Async 异步串口性能测试报告

> 项目：StarryOS
> 分支：feat/uart-async-bench
> 日期：2026-06-01（Q7 修复后更新）
> 测试环境：QEMU riscv64-virt
> **注意**：QEMU 不仿真真实串口时序（86.8 µs/byte @115200），吞吐量数值偏高。真板将接近 ~11.5 KB/s。

---

## 1. 测试概述

### 测试目标

测量 Async 异步串口在 Q7 修复后的性能指标：
- 用户态 TX 吞吐量（/dev/console + tcdrain）
- TX 单字节延迟（write + tcdrain）
- 非阻塞模式（FIONBIO）
- 内核态 Ring Buffer 速度
- 内存 / CPU / IRQ 统计

### Q7 修复项

| 编号 | 修复 | 影响 |
|------|------|------|
| O42 | yield storm：Manual→External ProcessMode | 空闲不再空转 |
| O43 | FIONBIO 传播到 TTY/ldisc | open/fcntl/ioctl 均可设置非阻塞 |
| O44 | benchmark 修正 + TCSBRK 实现 | 测量真实串口路径 |
| — | TCSBRK (tcdrain) 实现 | write + tcdrain 等待硬件发送完成 |

### 测试环境

| 项目 | 配置 |
|------|------|
| **目标架构** | RISC-V 64-bit |
| **平台** | qemu-riscv64-virt |
| **串口硬件** | NS16550 UART |
| **波特率** | 115200 bps |
| **FIFO 深度** | 16 字节 |
| **构建模式** | release (optimized) |
| **计时器** | `clock_gettime(CLOCK_MONOTONIC)`，纳秒精度 |

### 统计方法

| 参数 | 值 | 说明 |
|------|-----|------|
| **预热** | 5 次迭代（丢弃） | 消除冷启动偏差 |
| **延迟迭代** | 200 次（测量） | 统计显著性 |
| **吞吐量迭代** | 100 次/大小 | |
| **分位值计算** | 线性插值 | P = (N-1)×p/100，相邻元素线性插值 |
| **标准差** | 样本标准差（n-1） | σ = √(Σ(x-μ)²/(n-1)) |

---

## 2. 内核态测试结果

### 2.1 Ring Buffer 写入速度（TX）

**测试方法**：向 TX Ring Buffer 写入 102,400 字节数据（1024 × 100），测量总耗时和 CPU 占用

| 指标 | 值 | 说明 |
|------|-----|------|
| **Ring Buffer 写入** | 214,961 KB/s | 内核态写入 Ring Buffer |
| **测试数据量** | 102,400 字节 | 100 × 1024 |
| **测试耗时** | 0.47 毫秒 | 纳秒级精度 |
| **CPU Cycles** | 27,231,344 | RISC-V cycle 计数器 |
| **CPU Usage** | 58.5% | 每纳秒消耗的 cycle 数 |
| **硬件线速** | 11.52 KB/s | 115200 bps 理论极限 |

### 2.2 Ring Buffer 读取速度（RX）

**测试方法**：从 RX Ring Buffer 读取 65,536 字节数据，测量总耗时和 CPU 占用

| 指标 | 值 | 说明 |
|------|-----|------|
| **Ring Buffer 读取** | 588,776 KB/s | 内核态读取 Ring Buffer |
| **测试数据量** | 65,536 字节 | 64 KB |
| **测试耗时** | 0.11 毫秒 | 纳秒级精度 |
| **CPU Cycles** | 265,936 | RISC-V cycle 计数器 |
| **CPU Usage** | 2.45 cycles/ns | CPU 效率 |

### 2.3 Ring Buffer 读取延迟（RX）

**测试方法**：读取 100 个单字节（内核态直接操作 Ring Buffer，无系统调用开销）。
分位值：索引法 `data[N×p/100]`。

| 指标 | 值 | 说明 |
|------|-----|------|
| **P50 延迟** | 600 ns | 中位数延迟 |
| **P95 延迟** | 700 ns | 95 分位延迟 |
| **P99 延迟** | 98,800 ns | 99 分位延迟 |
| **最小延迟** | 500 ns | |
| **最大延迟** | 98,800 ns | |
| **平均延迟** | 1,606 ns | |
| **n** | 100 | 测量次数 |

### 2.4 内存占用

| 组件 | 大小 | 说明 |
|------|------|------|
| **RX Buffer** | 64 KB | 接收 Ring Buffer |
| **TX Buffer** | 64 KB | 发送 Ring Buffer |
| **驱动结构体** | 640 字节 | AsyncUartDriver + Mutex |
| **总计** | 128,640 字节 | 约 126 KB |

### 2.5 中断处理

| 指标 | 值 | 说明 |
|------|-----|------|
| **ISR Count** | 1 | 启动过程中触发 |
| **IRQ Frequency** | N/A | 只有 1 次 IRQ，无法计算频率 |
| **NAPI 配置** | 阈值=16, 批量=64 | 高吞吐时切换轮询模式 |

---

## 3. 用户态测试结果（Q7 修正后）

### 3.1 TX 吞吐量测试

**测试方法**：写 `/dev/console`（非 /dev/null），每次后 `tcdrain()` 等待硬件发送完成。预热 5 次，100 次迭代 × 4 种大小。吞吐量 = 总字节 / 总时长。

| 数据大小 | 吞吐量 (QEMU) | 预期 (真板) | 说明 |
|----------|-------------|------------|------|
| **64 bytes** | ~153 KB/s | ~11.5 KB/s | 小数据，tcdrain 开销占比大 |
| **256 bytes** | ~230 KB/s | ~11.5 KB/s | |
| **1024 bytes** | ~238 KB/s | ~11.5 KB/s | |
| **4096 bytes** | ~230 KB/s | ~11.5 KB/s | |

> ⚠️ QEMU 的 16550 模拟不仿真串口线延迟（86.8 µs/byte），tcdrain 几乎立即返回。
> 真板（VisionFive2 @ 115200 bps）上所有数据大小都应收敛到 ~11.5 KB/s。

### 3.2 TX 单字节延迟（write + tcdrain）

**测试方法**：单字节 `write()` + `tcdrain()`，预热 5 次后测量 200 次。
分位值使用线性插值（`P = (N-1)×p/100`，插值相邻元素）。

| 指标 | 值 (QEMU) | 说明 |
|------|----------|------|
| **n** | 200 | 测量次数 |
| **min** | ~0.14 ms | |
| **max** | ~0.85 ms | |
| **avg** | ~0.16 ms | 算术平均 |
| **stddev** | ~0.07 ms | 样本标准差 (n-1) |
| **P50** | ~0.15 ms | 线性插值中位数 |
| **P95** | ~0.19 ms | |
| **P99** | ~0.85 ms | |
| **P999** | ~0.85 ms | 99.9 分位 |

包含路径：user write → ring buffer push → TX copier → UART THR → tcdrain (TCSBRK poll)。

### 3.3 非阻塞模式 (FIONBIO)

| 测试 | 结果 | 说明 |
|------|------|------|
| `open(O_NONBLOCK)` + `read()` | ✅ PASS (EAGAIN) | O43 修复后生效 |
| `ioctl(FIONBIO, 1)` + `read()` | ✅ PASS (EAGAIN) | |
| `fcntl(F_SETFL, O_NONBLOCK)` + `read()` | ✅ PASS (EAGAIN) | |

---

## 4. 用户态 RX 测试说明

**当前状态**：用户态 RX 测试在内核态完成（直接操作 Ring Buffer），绕过 TTY 回显问题。

- RX Ring Buffer 读取：~413 MB/s
- RX 延迟 P50：600 ns
- Ring Buffer 不是瓶颈（413 MB/s >> 串口线速 11.52 KB/s）

**未来方向**：设置终端 raw mode + 禁用 echo，可实现用户态 RX 测试。

---

## 5. 测试方法

### 内核态（benchmark.rs）
- **Ring Buffer TX**：push 102,400 bytes，测量速度 + CPU
- **Ring Buffer RX**：pop 65,536 bytes + 100 次单字节延迟
- 启动时自动运行，输出到串口日志

### 用户态（benchmark.c，Q7 修正后）
- **TX 吞吐量**：`write(/dev/console) + tcdrain()`，100 次 × 4 种大小
- **TX 延迟**：单字节 `write + tcdrain`，100 次，计算 P50/P95/P99
- **非阻塞测试**：`open(O_NONBLOCK)` / `ioctl(FIONBIO)` / `fcntl(F_SETFL)` 三种入口
- **编译**：`riscv64-linux-musl-gcc -static`

### QEMU 时序说明
QEMU 16550 模拟不仿真真实串口线延迟。`tcdrain()` 的 TCSBRK 实现正确（poll ring buffer + LSR.TRANSMITTER_EMPTY），但 QEMU 内部 UART 数据处理为瞬时。真板 VisionFive2 @ 115200 bps 将产生 ~11.5 KB/s 的准确吞吐量。

---

## 6. 结论

### Q7 修复总结

| 修复 | 状态 | 效果 |
|------|------|------|
| O42  yield storm | ✅ | External ProcessMode 消除空闲空转 |
| O43  FIONBIO 传播 | ✅ | 非阻塞读 open/fcntl/ioctl 全部生效 |
| O44  benchmark | ✅ | 真实 /dev/console 路径 + tcdrain |
| TCSBRK | ✅ | tcdrain 等待 ring buffer + UART drain |

### 性能（QEMU）

| 维度 | 结果 |
|------|------|
| TX 用户态 @ /dev/console + tcdrain | ~200 KB/s（QEMU，真板 ~11.5） |
| TX 延迟 P50 | ~0.15 ms |
| FIONBIO nonblocking read | ✅ EAGAIN |
| Ring Buffer TX | ~180 MB/s |
| Ring Buffer RX | ~413 MB/s |

### 待验证（真板 VisionFive2）

- 真实串口吞吐量 ~11.5 KB/s @ 115200 bps
- DMA 可行性
- 高速波特率（230400+）

---

**报告版本**：2.1
**最后更新**：2026-06-01
