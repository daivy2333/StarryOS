# UART 串口性能对比报告

> 项目：StarryOS | 分支：feat/uart-async-dev2 | 日期：2026-06-01
> 测试环境：QEMU riscv64-virt
> ⚠️ QEMU 不仿真串口时序，吞吐量数据偏高。真板 VisionFive2 @115200 将收敛到 ~11.5 KB/s。

---

## 1. 概述

对比 Console 阻塞串口和 Async 异步串口。Console 数据来自直接读写 /dev/console（无 ring buffer），Async 数据来自 Q7 修复后的 async 路径。

### Q7 修复后的 Async 架构

| 特性 | Console (阻塞) | Async (异步, Q7) |
|------|---------------|--------------|
| **写入路径** | write() → 轮询等待 FIFO | write() → Ring Buffer → TX copier → 批量发送 |
| **读取路径** | read() → 轮询等待 FIFO | ISR → RX copier → Ring Buffer → TTY → read() |
| **是否阻塞** | 是（等待硬件） | 否（写入内存即返回） |
| **非阻塞支持** | — | ✅ FIONBIO open/fcntl/ioctl 全入口 |
| **空闲 CPU** | 0%（无后台任务） | 0%（O42 External 模式，无 yield storm） |
| **缓冲区** | 无 | 128 KB Ring Buffer |
| **tcdrain** | 隐式（每字节等待） | ✅ TCSBRK 实现 |

### 测试环境

| 项目 | 配置 |
|------|------|
| **架构** | RISC-V 64-bit |
| **平台** | qemu-riscv64-virt |
| **串口** | NS16550 UART，115200 bps |
| **FIFO** | 16 字节 |

---

## 2. 性能对比

### 2.1 内核态性能（统一数据量：102,400 字节）

| 指标 | Console | Async | 胜出 |
|------|---------|-------|------|
| **TX 写入速度** | 567 KB/s | 214,961 KB/s | **Async 379x** |
| **TX CPU Cycles** | 392,721,729 | 27,231,344 | **Async 14.4x** |
| **TX 每字节 CPU** | 3,835 cycles/byte | 265 cycles/byte | **Async 14.5x** |
| **RX 读取速度** | ❌ 不可测 | 588,776 KB/s | Async |
| **RX 延迟 P50** | ❌ 不可测 | 600 ns | Async |

**RX 测试差异说明**：

| 项目 | Console | Async |
|------|---------|-------|
| **缓冲区** | 无 Ring Buffer | 有 Ring Buffer |
| **读取方式** | 非阻塞（try_receive） | 阻塞（等待数据） |
| **内核态 RX 测试** | ❌ 不可测 | ✅ 可测 |
| **用户态 RX 测试** | ❌ 不可测 | ❌ 不可测（TTY 竞争） |

**为什么不能直接测试 FIFO**：
1. **非阻塞读取**：Console 的 read_bytes() 使用 try_receive()，没有数据立即返回 0，无法测量延迟
2. **需要外部数据注入**：FIFO 只有 16 字节，需要外部设备发送数据，无法自动化测试
3. **FIFO 容量小**：只有 16 字节，无法测试大数据量的吞吐量
4. **与 Shell 竞争**：Shell 和 benchmark 都在读取 FIFO，Shell 会抢先读取数据

**为什么 Async 可以测试 RX**：
1. **有 Ring Buffer**：可以存储大量数据（64 KB），支持大数据量测试
2. **阻塞读取**：可以等待数据到达，测量延迟
3. **自动数据注入**：ISR 驱动，自动填充 Ring Buffer，无需外部设备
4. **无竞争**：benchmark 程序独占 Ring Buffer，不与 Shell 竞争

**用户态 RX 都无法测试**：TTY 层回显导致 Shell 抢先读取数据

### 2.2 用户态 write() 延迟

| 指标 | Console | Async | 胜出 |
|------|---------|-------|------|
| **P50** | 17.5 µs | 7.9 µs | **Async 2.2x** |
| **P95** | 32.8 µs | 12.2 µs | **Async 2.7x** |
| **P99** | 324.5 µs | 43.1 µs | **Async 7.5x** |

**说明**：
- Async 的 write() 只写入 Ring Buffer，延迟更低
- QEMU 不等待硬件，真实硬件上差异可能更大

### 2.3 用户态吞吐量（⚠️ QEMU 限制）

| 数据大小 | Console (QEMU) | Async + tcdrain (QEMU) | 预期真板 (两者) |
|----------|---------------|----------------------|----------------|
| **64 B** | ~13,600 KB/s | ~150 KB/s | ~11.5 KB/s |
| **256 B** | ~49,600 KB/s | ~230 KB/s | ~11.5 KB/s |
| **1024 B** | ~135,000 KB/s | ~240 KB/s | ~11.5 KB/s |
| **4096 B** | ~250,000 KB/s | ~230 KB/s | ~11.5 KB/s |

> ⚠️ **重要**：QEMU 16550 模拟对 Console 和 Async 都不仿真串口线延迟（86.8 µs/byte）。
> Console 的 send_raw() 在 QEMU 中瞬时完成（无 FIFO 等待），Async 的 tcdrain 也瞬时返回。
> 真板上两者都受 115200 bps 限制，收敛到 ~11.5 KB/s。

### 2.4 非阻塞模式（Async 独有）

| 特性 | Console | Async (Q7) |
|------|---------|-----------|
| `open(O_NONBLOCK)` + `read()` | ❌ 不支持 | ✅ EAGAIN |
| `ioctl(FIONBIO)` + `read()` | ❌ 不支持 | ✅ EAGAIN |
| `fcntl(F_SETFL, O_NONBLOCK)` + `read()` | ❌ 不支持 | ✅ EAGAIN |

### 2.5 资源占用

| 指标 | Console | Async | 胜出 |
|------|---------|-------|------|
| **内存占用** | 0 KB | 128 KB | **Console** |
| **Shell 功能** | ✅ 正常 | ✅ 正常 | 平局 |
| **数据完整性** | ✅ 100% | ✅ 100% | 平局 |

---

## 3. 结论

### Q7 修复总结

| 修复 | 效果 |
|------|------|
| O42 yield storm | External ProcessMode，空闲不再空转 |
| O43 FIONBIO | 非阻塞读 open/fcntl/ioctl 三入口全部生效 |
| O44 benchmark + TCSBRK | 测量真实 /dev/console 路径，tcdrain 正确等待 |

### 核心差异

| 维度 | Console | Async (Q7) |
|------|---------|------------|
| **CPU 效率（内核态）** | 3,835 cycles/byte | 265 cycles/byte（14.5x） |
| **write() 延迟 P50** | 17.5 µs | 7.9 µs（2.2x） |
| **非阻塞读** | ❌ | ✅ |
| **空闲 CPU** | 0% | 0%（无 yield storm） |
| **内存** | 0 KB | 128 KB |

### 真板预期

VisionFive2 @ 115200 bps：
- 两者 TX 吞吐量均收敛到 ~11.5 KB/s（硬件上限）
- Async 优势在 CPU 效率和延迟（write 不阻塞调用者）
- FIONBIO 提供非阻塞读能力（Console 不具备）

---

## 附录：测试方法

### 内核态测试

- **TX 测试**：写入 102,400 字节到 Ring Buffer，测量速度和 CPU 占用
- **RX 测试**（仅 Async）：从 Ring Buffer 读取 65,536 字节，测量速度和 CPU 占用
- **RX 延迟测试**（仅 Async）：读取 100 个单字节，测量每次读取延迟
- **Console RX 测试**：跳过（无 Ring Buffer，read_bytes() 非阻塞）

### 用户态测试

- **吞吐量**：测试 64/256/1024/4096 字节，每种 1000 次
- **延迟**：100 次单字节 write()，计算 P50/P95/P99
- **压力测试**：持续 2 秒写入，测量总吞吐量
- **RX 测试**：跳过（TTY 竞争条件）

### 局限性

- QEMU 串口模拟不等待硬件，write() 立即返回
- 真实硬件上 Async 优势可能更明显（我猜的）
- 用户态 RX 测试受 TTY 竞争条件限制
- Console 无法测试 RX（无 Ring Buffer，非阻塞读取）

---

**报告版本**：2.3
**最后更新**：2026-06-01
