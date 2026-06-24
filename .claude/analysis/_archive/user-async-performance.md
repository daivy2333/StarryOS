# 用户态异步串口性能分析（摘要）

> ⚠️ **STALE [2026-06-17]** — 完整版已归档至 `_archive/user-async-performance.md`（18K）
> **Q7 已完成（2026-06-01）**，完整记录见 `optimization/spec.md` §Q7 + `tasks.md` §Q7

---

## 核心结论

用户态 Async UART 性能**不优于阻塞 Console UART**，根本原因是 **115200 bps 硬件线速上限**（11.52 KB/s）。**Q7 四项修复**解决了所有非硬件瓶颈：

| 瓶颈 | 修复 | 收益 |
|------|------|------|
| **1. 三重 yield storm** | O42 ProcessMode::Manual → External | 空闲 CPU 归零（无数据时 0%） |
| **2. 锁竞争** | Q10.3 ldisc 锁拆分 | RX 路径不互斥 |
| **3. 拷贝链过长** | Q10.1 C3/C4 合并 | 减少 1 次数据拷贝 |
| **4. FIONBIO 不传播** | O43 三入口（open/fcntl/ioctl）传播 | TTY read 立即 EAGAIN |

**异步架构的真实优势**：(1) write 不阻塞 (2) NAPI 中断合并 (3) 为 DMA/多队列铺路。

---

## Q7 四项修复（✅ 全部完成）

| 编号 | 内容 | 关键文件 | 验证 |
|------|------|---------|------|
| **O42** | yield storm 修复 | `ntty_async.rs` + `ldisc.rs` | `top` 确认无数据时 CPU 0% |
| **O43** | FIONBIO nonblocking 三入口传播 | `tty/mod.rs` + `ldisc.rs` + `ctl.rs` | `ioctl(FIONBIO) + read()` 立即 EAGAIN |
| **O44** | benchmark 修正 | `tests/benchmark.c` | 测真实 UART 吞吐量（/dev/console + tcdrain） |
| **O45** | tcdrain 真异步化 | `isr.rs` + `ctl.rs` | 64B 切换 9→6 次，~300→~200 µs |

---

## 性能基线（Q7 修复后，QEMU）

| 指标 | 测量方法 | 修复后值 |
|------|---------|---------|
| TX 吞吐量 @115200 | `write → tcdrain()` 5 秒批量 | ~11.5 KB/s（线速） |
| TX 延迟 P50 | 单字节 `write+tcdrain` | ~1 µs（ring buf push） |
| RX 吞吐量（内核态） | 绕过 TTY 直测 ring buffer | 588 MB/s |
| RX 延迟（内核态） | ring buffer P50 | 600 ns |
| 空闲 CPU | 无数据 10 秒 | **0%**（O42 后） |
| CPU 效率 | 102,400 字节 | 268 cycles/byte（Console: 3,835，快 14.3×）|
| 非阻塞读 | `ioctl(FIONBIO) + read()` | 立即 EAGAIN（O43 后） |
| tcdrain 延迟（64B） | QEMU | ~200 µs（O45 后）|

**QEMU vs 真板可信度**：
- ✅ 可信：内核态 ring buffer / write() 延迟 / CPU cycles
- ❌ 不可信：串口吞吐量 / tcdrain 延迟（QEMU 不仿真线延迟）
- 真板预期：VisionFive2 @ 115200 bps → ~11.5 KB/s（硬上限）

---

## 跨层状态教训（FIONBIO 案例）

> **任何跨层状态（如 O_NONBLOCK）MUST 穷举所有入口（open / fcntl / ioctl）并逐个验证。一个入口遗漏 = 功能不完整。**（FIONBIO 教训，参见 `learned` L140）

| 入口 | 位置 | 状态 |
|------|------|------|
| `open(O_NONBLOCK)` | `fd_ops.rs:106-108` | ✅ 传播 |
| `fcntl(F_SETFL, O_NONBLOCK)` | `fd_ops.rs:253-254` | ✅ 传播 |
| `ioctl(FIONBIO)` | `ctl.rs:28-38` | ✅ 传播 |
| File → TTY | `tty/mod.rs:86-104` | ✅ 修复 O43 |
| TTY → ldisc | `ldisc.rs:328-370` | ✅ 修复 O43 |

---

## 当前权威文档（取代本文）

| 主题 | 当前权威文档 |
|------|-------------|
| Q7 修复落地 | `optimization/spec.md` §Q7 用户态性能修复（含 O42~O45 详情）|
| Q7 任务条目 | `tasks.md` §Q7 |
| 性能基线 | `optimization/spec.md` §性能指标基线与硬件理论极限 |
| 跨层状态教训 | `learned/spec.md` L140（FIONBIO 跨层传播）|

---

**恢复条件**：如需查看完整 TX/RX 路径追踪图、5 层瓶颈分解、Async vs Console 对比表、QEMU vs 真板可信度矩阵，查阅 `_archive/user-async-performance.md`
**生成日期**：2026-06-11（原始）→ 2026-06-17（摘要）
