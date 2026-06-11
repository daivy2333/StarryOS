# UART 性能对比：Console vs Async

> 项目：[StarryOS](https://github.com/daivy2333/StarryOS) | 分支：asyncuart-dev | 日期：2026-06-11（Q0~Q12 完成）
> 测试环境：QEMU riscv64-virt · NS16550 @ 115200 bps · FIFO 16B
>
> **⚠️ QEMU 的 NS16550 不仿真串口线延迟（86.8 µs/byte）。用户态吞吐量在 QEMU 上不可比——真板两者均收敛至 ~11.5 KB/s。本文讨论 QEMU 上可信维度：内核态速度、CPU 效率、write() 延迟、功能覆盖。**

---

## 1. 架构对比

| 维度 | Console（阻塞） | Async（Q12） |
|------|----------------|-------------|
| TX 路径 | `write()` → 逐字节轮询 LSR → 写 THR | `write()` → push [ring buf](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/ring_buffer.rs) → [TX copier](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/async_driver.rs#L72) → 批量 `send_bytes()` |
| RX 路径 | ISR → tty-reader → ldisc → `read()` | ISR → [RX copier](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/async_driver.rs#L40) → ring buf → tty-reader → [ldisc](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/pseudofs/dev/tty/terminal/ldisc.rs) → `read()` |
| 缓冲区 | 无 | 128 KB（lock-free [RingBuffer](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/ring_buffer.rs) RX/TX 各 64 KB，Q12 去 Mutex） |
| `write()` 延迟 | P50 17.5 µs（逐字节轮询 LSR） | P50 7.9 µs（push ring buf 即返回，Q12 去锁后更低） |
| 空闲 CPU | 0% | 0%（[External 模式](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/ntty_async.rs#L20)，无 yield storm） |
| tcdrain | 隐式（每字节等 LSR） | ✅ [TCSBRK](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/syscall/fs/ctl.rs#L43) + NS16550 TEMT 硬件直接唤醒（Q12/O53 去 TCDRAIN_ACTIVE 软件标志） |
| 非阻塞读 | ❌ | ✅ open / fcntl / ioctl 三入口（[FIONBIO 传播](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/syscall/fs/ctl.rs)） |
| 读超时 | ❌ | ✅ [VTIME](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/pseudofs/dev/tty/terminal/ldisc.rs#L364)（axtask::future::timeout） |
| 唤醒机制 | PollSet（spinlock, ~200ns） | AtomicWaker（lock-free, ~50ns, [Q8/O46](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/isr.rs)） |
| 标准化接口 | ❌ | ✅ [embedded_io_async](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/device_ops.rs) Read/Write trait（Q12/O52） |
| ldisc 缓冲 | 80B StaticRb | 256B StaticRb（[Q10](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/pseudofs/dev/tty/terminal/ldisc.rs#L24) 扩容 3.2×） |
| 后台任务 | 1（tty-reader） | 2（RX copier + tty-reader） |

---

## 2. 内核态性能

> 直接操作 Ring Buffer，无系统调用开销。**不受 QEMU 串口时序影响**，反映纯软件性能。

| 指标 | Console | Async | 说明 |
|------|---------|-------|------|
| TX Ring Buffer 写入 | 567 KB/s | ~197 MB/s | Console 逐字节 `send_raw()`；Async 批量 `push_slice()` |
| TX CPU cycles/byte | 3,835 | ~13 | **Async 效率高 ~295 倍** |
| RX Ring Buffer 读取 | 不可测¹ | ~393 MB/s | Async 直接 pop HeapRb |
| RX 延迟 P50 | 不可测¹ | 600 ns | 单字节 pop 延迟 |

¹ Console 无 Ring Buffer，无法做可比较的内核态 RX 测试。

---

## 3. 用户态延迟

> 单字节 `write()` 系统调用往返，**无 tcdrain**（只测系统调用开销）。100 次迭代，无预热。

| 指标 | Console | Async | 差异 |
|------|---------|-------|------|
| P50 | 17.5 µs | 7.9 µs | Async 快 2.2× |
| P95 | 32.8 µs | 12.2 µs | Async 快 2.7× |
| P99 | 324.5 µs | 43.1 µs | Async 快 7.5× |

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

| 大小 | Async Q12 (QEMU) | 硬件理论 | 真板预测 |
|------|-----------------|----------|----------|
| 64 B | 325.4 µs | 5.56 ms | 5.88 ms |
| 256 B | 1171.8 µs | 22.2 ms | 23.4 ms |
| 1024 B | 4684.2 µs | 88.9 ms | 93.6 ms |
| 4096 B | 8812.4 µs | 355.6 ms | 364.4 ms |

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
| 内存 | 0 KB | 128 KB（RX/TX ring buf） + 0.5 KB（ldisc 256B StaticRb） |
| 后台任务 | 1 | 2 |
| 数据完整性 | ✅ | ✅ |

---

## 7. 总结

| 维度 | 结果 | 说明 |
|------|------|------|
| CPU 效率 | Async ⬆ ~295× | 13 vs 3,835 cycles/byte |
| write() 延迟 | Async ⬆ 2.2–7.5× | P50 7.9 vs 17.5 µs |
| 唤醒延迟 | Async ⬆ 4× | AtomicWaker 50ns vs PollSet 200ns |
| 非阻塞读 | Async ✅ | Console 无 |
| 读超时 | Async ✅ | VTIME |
| 真板吞吐量 | 持平 ~11.5 KB/s | 同受波特率限制 |

**完整优化历史**：Q0~Q4（驱动骨架）→ Q5（NAPI/批量I/O）→ Q7（yield storm/FIONBIO/tcdrain）→ Q8（NAPI退出/ISR无锁/O46 AtomicWaker）→ Q9（VTIME超时）→ Q10（BUF_SIZE 256/push_slice/&self）→ Q11（通用质量）→ **Q12（Embassy 路径 A：lock-free RingBuffer + embedded_io_async + TC tcdrain）** → Q6（真板待验证）