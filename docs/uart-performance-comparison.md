# UART 性能对比：Console vs Async

> 项目：StarryOS | 分支：feat/uart-async-dev2 | 日期：2026-06-01
> 测试环境：QEMU riscv64-virt · NS16550 @ 115200 bps · FIFO 16B
>
> **⚠️ 关键前提**：QEMU 的 NS16550 模拟**不仿真串口线延迟**（86.8 µs/byte @115200）。
> 因此 QEMU 上的用户态吞吐量无法反映真实串口性能，两者在真板都会收敛到 ~11.5 KB/s。
> 本文只讨论在 QEMU 上可可信测量的维度：内核态速度、CPU 效率、write() 延迟、功能覆盖。

---

## 1. 架构

| 维度 | Console（阻塞） | Async（异步，Q7 修复后） |
|------|----------------|------------------------|
| TX 路径 | `write()` → 轮询 LSR → 逐字节写 THR | `write()` → push ring buf → TX copier → 批量 `send_bytes()` |
| RX 路径 | ISR → tty-reader → ldisc → `read()` | ISR → RX copier → ring buf → tty-reader → ldisc → `read()` |
| 缓冲区 | 无 | 128 KB（RX/TX 各 64 KB HeapRb） |
| write() 行为 | 阻塞（等待 FIFO 有空位） | 非阻塞（push ring buf 即返回） |
| 空闲 CPU | 0% | 0%（O42 External 模式，无 yield storm） |
| tcdrain | 隐式（每字节等待 LSR） | ✅ TCSBRK 显式支持 |
| 非阻塞读 | ❌ | ✅ `open/fcntl/ioctl` 三入口 |
| 额外任务 | 1（tty-reader） | 2（RX copier + tty-reader） |

---

## 2. 内核态速度（Ring Buffer）

> 测量内核态直接操作 Ring Buffer 的速度，无系统调用开销。
> 这些数值**不受 QEMU 串口时序影响**，反映纯软件性能。
> TX: 1024 B × 100 次；RX: 64 KB 单次读取；RX 延迟: 单字节 × 100 次，索引法分位值。

| 指标 | Console | Async | 说明 |
|------|---------|-------|------|
| **TX Ring Buffer 写入** | 567 KB/s | ~215 MB/s | Console 逐字节轮询 `send_raw()`；Async 批量 `push()` |
| **TX CPU cycles/byte** | 3,835 | 265 | **Async 效率高 14.5 倍** |
| **RX Ring Buffer 读取** | 不可测¹ | ~413 MB/s | Async 直接 pop HeapRb |
| **RX 延迟 P50** | 不可测¹ | 600 ns | 单字节 pop 延迟 |

¹ Console 无 Ring Buffer，`read_bytes()` 为非阻塞 `try_receive()`，无法在内核态做可比较的 RX 吞吐/延迟测试。

---

## 3. 用户态 write() 延迟

> 测量单字节 `write()` 系统调用往返，**无 tcdrain**（只测系统调用开销，不测硬件发送时间）。
> 100 次迭代，无预热，分位值 = 简单索引法 `data[N×p/100]`。

| 指标 | Console | Async | 差异 |
|------|---------|-------|------|
| **P50** | 17.5 µs | 7.9 µs | Async 快 2.2x |
| **P95** | 32.8 µs | 12.2 µs | Async 快 2.7x |
| **P99** | 324.5 µs | 43.1 µs | Async 快 7.5x |

Async 更快的根因：`write()` 只 push 到 ring buffer（~1 µs + 锁开销），而 Console 的 `write()` 在调用栈内逐字节轮询 LSR 然后写 THR。

---

## 4. 功能覆盖

| 功能 | Console | Async (Q7) |
|------|---------|-----------|
| 阻塞读写 | ✅ | ✅ |
| `open(O_NONBLOCK)` + `read()` → EAGAIN | ❌ | ✅ |
| `ioctl(FIONBIO, 1)` + `read()` → EAGAIN | ❌ | ✅ |
| `fcntl(F_SETFL, O_NONBLOCK)` + `read()` → EAGAIN | ❌ | ✅ |
| `tcdrain()` （TCSBRK） | 隐式 | ✅ 显式 poll ring buf + LSR.TRANSMITTER_EMPTY |
| Shell 功能（`ls`/`cd`/`pwd`） | ✅ | ✅ |
| 内核日志（`ax_println!`） | ✅ | ✅（polling TX 共存） |

---

## 5. 用户态吞吐量（⚠️ QEMU 不可比）

> **本节数据在 QEMU 上无比较意义。** 原因：
> - Console 测试为 `write(/dev/console)` 逐字节轮询，QEMU 瞬时完成 → 测得纯 VFS+syscall 速度。
> - Async 测试为 `write(/dev/console) + tcdrain()`，tcdrain 调用 TCSBRK 做 poll 循环 → 多了一层 task yield 开销。
> - 两者测的是**不同的东西**——Console 测 VFS 速度，Async 测 VFS + tcdrain 速度。
>
> **真板预期**：VisionFive2 @ 115200 bps，两者均受波特率限制，吞吐量收敛到 ~11.5 KB/s。
> 区别是：Async 的 `write()` **立即返回**（pipeline 友好），Console 的 `write()` **阻塞到发送完成**。
> 测试方法：预热 5 次 → 100 次/大小。吞吐量 = 总字节 / 总时长。

| 数据大小 | Console `write()` only | Async `write + tcdrain` | 真板预期 |
|----------|----------------------|------------------------|---------|
| 64 B | ~13,600 KB/s | ~193 KB/s | ~11.5 KB/s |
| 256 B | ~49,600 KB/s | ~250 KB/s | ~11.5 KB/s |
| 1024 B | ~135,000 KB/s | ~282 KB/s | ~11.5 KB/s |
| 4096 B | ~250,000 KB/s | ~270 KB/s | ~11.5 KB/s |

---

## 6. 资源占用

| 指标 | Console | Async |
|------|---------|-------|
| 内存 | 0 KB | 128 KB（RX/TX ring buf） |
| 后台任务 | 1 | 2 |
| 数据完整性 | ✅ | ✅ |

---

## 7. 总结

| 对比维度 | 结果 | 说明 |
|---------|------|------|
| **CPU 效率** | Async ⬆ 14.5x | 265 vs 3,835 cycles/byte |
| **write() 延迟** | Async ⬆ 2.2–7.5x | P50 7.9 vs 17.5 µs |
| **非阻塞读** | Async  | Console 无此能力 |
| **吞吐量（真板）** | 持平 ~11.5 KB/s | 同受波特率限制 |
| **内存** | Console  | 0 KB vs 128 KB |
| **复杂度** | Console  | 更简单，但更局限 |

### Q7 修复项（本报告基准）

- **O42**：yield storm → `ProcessMode::External`，空闲 CPU 归零
- **O43**：FIONBIO 传播到 Tty/ldisc，非阻塞读全部入口生效
- **O44**：benchmark 修正 + TCSBRK 实现，tcdrain 正确等待硬件 drain
