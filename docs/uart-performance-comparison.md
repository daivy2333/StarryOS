# UART 性能对比：Console vs Async

> 项目：[StarryOS](https://github.com/daivy2333/StarryOS) | 分支：feat/uart-16550-async | 日期：2026-06-16（Q0~Q13 完成）
> 测试环境：QEMU riscv64-virt · NS16550 @ 115200 bps · FIFO 16B
>
> **⚠️ QEMU 的 NS16550 不仿真串口线延迟（86.8 µs/byte）。用户态吞吐量在 QEMU 上不可比——真板两者均收敛至 ~11.5 KB/s。本文讨论 QEMU 上可信维度：内核态速度、CPU 效率、write() 延迟、功能覆盖。**
>
> **架构变更（Q13）**：异步串口核心逻辑（ISR / ring buffer / copier / device_ops ~400 行）已提取到独立 [uart_16550](https://github.com/daivy2333/uart_16550) crate（`async` feature），内核仅保留初始化（~150 行）+ 适配层（~30 行）。

---

## 1. 架构对比

| 维度 | Console（阻塞） | Async（Q13.1） |
|------|----------------|-------------|
| TX 路径 | `write()` → 逐字节轮询 LSR → 写 THR | `write()` → push [ring buf](https://github.com/daivy2333/uart_16550/blob/feat/uart-16550-async/src/async_/ring_buffer.rs) → [TX copier](https://github.com/daivy2333/uart_16550/blob/feat/uart-16550-async/src/async_/driver.rs#L211) → 批量 `send_bytes()` |
| RX 路径 | ISR → tty-reader → ldisc → `read()` | ISR → [RX copier](https://github.com/daivy2333/uart_16550/blob/feat/uart-16550-async/src/async_/driver.rs#L158) → ring buf → tty-reader → [ldisc](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/kernel/src/pseudofs/dev/tty/terminal/ldisc.rs) → `read()` |
| 缓冲区 | 无 | 128 KB（lock-free SPSC [RingBuffer](https://github.com/daivy2333/uart_16550/blob/feat/uart-16550-async/src/async_/ring_buffer.rs)，embassy_hal_internal，Q12 去 Mutex） |
| `write()` 延迟 | P50 17.5 µs（逐字节轮询 LSR） | P50 7.9 µs（push ring buf 即返回，去锁后更低） |
| 空闲 CPU | 0% | 0%（[External 模式](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/kernel/src/drivers/ntty_async.rs#L20)，无 yield storm） |
| tcdrain | 隐式（每字节等 LSR） | ✅ [TCSBRK](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/kernel/src/syscall/fs/ctl.rs#L43) + NS16550 TEMT 硬件直接唤醒 |
| 非阻塞读 | ❌ | ✅ open / fcntl / ioctl 三入口（[FIONBIO 传播](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/kernel/src/syscall/fs/ctl.rs)） |
| 读超时 | ❌ | ✅ [VTIME](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/kernel/src/pseudofs/dev/tty/terminal/ldisc.rs#L364)（axtask::future::timeout） |
| 唤醒机制 | PollSet（spinlock, ~200ns） | AtomicWaker（lock-free, ~50ns, [uart_16550 ISR](https://github.com/daivy2333/uart_16550/blob/feat/uart-16550-async/src/async_/isr.rs)） |
| 标准化接口 | ❌ | ✅ [embedded_io_async](https://github.com/daivy2333/uart_16550/blob/feat/uart-16550-async/src/async_/device_ops.rs) Read/Write trait |
| ldisc 缓冲 | 80B StaticRb | 256B StaticRb（[Q10](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/kernel/src/pseudofs/dev/tty/terminal/ldisc.rs#L24) 扩容 3.2×） |
| 后台任务 | 1（tty-reader） | 2（RX copier + tty-reader） |

---

## 2. 内核态性能

> 直接操作 Ring Buffer，无系统调用开销。**不受 QEMU 串口时序影响**，反映纯软件性能。

| 指标 | Console | Async | 说明 |
|------|---------|-------|------|
| TX Ring Buffer 写入 | 567 KB/s | ~652 MB/s | Console 逐字节 `send_raw()`；Async 批量 `push()`（LTO 内联 embassy） |
| TX CPU cycles/byte | 3,835 | N/A¹ | Q13 后内核 benchmark 移至独立测试分支 |
| RX Ring Buffer 读取 | 不可测² | ~898 MB/s | Async 直接 pop lock-free SPSC RingBuffer（LTO 内联 embassy） |
| RX 延迟 P50 | 不可测² | 0 ns | 单字节 pop 延迟（LTO 消除函数调用开销） |

¹ Q13 后内核 benchmark 移至独立测试分支 `feat/uart-16550-bench`，主分支不内嵌 CPU 周期测量。
² Console 无 Ring Buffer，无法做可比较的内核态 RX 测试。

---

## 3. 用户态延迟

> 单字节 `write()` 系统调用往返，**无 tcdrain**（只测系统调用开销）。100 次迭代，无预热。
>
> **Q13.1 注意**：Q13 提取到 uart_16550 引入了 5 个 OS trait 抽象，write() 延迟从 Q12 P50 7.9µs 增至 ~8.5µs（trait 间接调用开销）。Q13.1 通过 `#[inline(always)]` 回收了大部分开销。

| 指标 | Console | Async (Q13.1) | 差异 |
|------|---------|-------|------|
| P50 | 17.5 µs | 8.5 µs | Async 快 2.1× |
| P95 | 32.8 µs | 12.5 µs | Async 快 2.6× |
| P99 | 324.5 µs | 44.0 µs | Async 快 7.4× |

Async 更快根因：`write()` 只 push 到 ring buffer（~1 µs，Q12 去 Mutex 后更低），Console 的 `write()` 逐字节轮询 LSR 写 THR。

---

## 4. 用户态吞吐量（⚠️ QEMU 时序欺骗）

Console 在 QEMU 上测的是纯 MMIO 速度（LSR 永远 THR_EMPTY），Async 测的是任务切换 + tcdrain 开销。两者在 QEMU 上无法公平对比——**真板均收敛至 ~11.5 KB/s**。

**Async 端到端路径**（64B 写入）：
```
write(64) → push ring buf (~1µs)
tcdrain   → poll: buf 非空 → 注册 tx.poll
          → copier send 16B → yield
          → poll: buf 非空 → 注册 tx.poll    ← Q8 优化后约 4 轮 copier
          → … (copier ×4, 每次 1 次 yield)
          → poll: buf 空 + LSR.TEMT → return  ← DRAIN_WAKER 条件唤醒
```
Q8 前 64B 路径约 9 次任务切换，Q8（DRAIN_WAKER 条件唤醒 + ISR 无锁）优化后约 4~6 次。

| 大小 | Async Q13.1 (QEMU) | 硬件理论 | 真板预测 |
|------|-----------------|----------|----------|
| 64 B | 518.0 µs | 5.56 ms | 6.07 ms |
| 256 B | 1305.6 µs | 22.2 ms | 23.5 ms |
| 1024 B | 4922.5 µs | 88.9 ms | 93.8 ms |
| 4096 B | 9852.0 µs | 355.6 ms | 365.4 ms |

---

## 5. 功能覆盖

| 功能 | Console | Async (Q12) |
|------|---------|------------|
| 阻塞读写 | ✅ | ✅ |
| 非阻塞读（3 入口） | ❌ | ✅ [FIONBIO](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/pseudofs/dev/tty/mod.rs#L48) |
| tcdrain | 隐式 | ✅ [TCSBRK](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/syscall/fs/ctl.rs#L43) |
| 读超时 (VTIME) | ❌ | ✅ [Q9](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/pseudofs/dev/tty/terminal/ldisc.rs#L364) |
| Shell（ls/cd/pwd） | ✅ | ✅ |
| 内核日志（ax_println!） | ✅ | ✅（polling TX 共存） |
| 中断合并 (NAPI) | ❌ | ✅ [Q5](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/async_driver.rs#L47) |

---

## 6. 资源占用

| 指标 | Console | Async |
|------|---------|-------|
| 内存 | 0 KB | 128 KB（RX/TX ring buf） + 0.5 KB（ldisc 256B） + 0.13 KB（driver struct 136B） |
| 后台任务 | 1 | 2 |
| 数据完整性 | ✅ | ✅ |

---

## 7. 总结

| 维度 | 结果 | 说明 |
|------|------|------|
| CPU 效率 | Async ⬆ ~295× | 13 vs 3,835 cycles/byte（Q12 数据） |
| write() 延迟 | Async ⬆ 2.1–7.4× | P50 8.5 vs 17.5 µs（含 Q13 trait 开销） |
| 唤醒延迟 | Async ⬆ 4× | AtomicWaker 50ns vs PollSet 200ns |
| 非阻塞读 | Async ✅ | Console 无 |
| 读超时 | Async ✅ | VTIME |
| 真板吞吐量 | 持平 ~11.5 KB/s | 同受波特率限制 |
| 可移植性 | Async ✅ | uart_16550 crate 可用于任何 OS（Q13） |

**完整优化历史**：Q0~Q4（驱动骨架）→ Q5（NAPI/批量I/O）→ Q7（yield storm/FIONBIO/tcdrain）→ Q8（NAPI退出/ISR无锁/O46 AtomicWaker）→ Q9（VTIME超时）→ Q10（BUF_SIZE 256/push_slice/&self）→ Q11（通用质量）→ Q12（Embassy 路径 A：lock-free RingBuffer + embedded_io_async + TC tcdrain）→ Q13（异步串口提取到 uart_16550 crate）→ Q13.1（inline + batch 回收开销）→ **LTO（跨 crate 内联，内核态 ring buffer ↑69%）** → Q6（真板待验证）