# UART 性能对比：Console vs Async

> 项目：StarryOS | 分支：asyncuart-dev | 日期：2026-06-11（Q8~Q11 完成）
> 测试环境：QEMU riscv64-virt · NS16550 @ 115200 bps · FIFO 16B
>
> **⚠️ 关键前提**：QEMU 的 NS16550 模拟**不仿真串口线延迟**（86.8 µs/byte @115200）。
> 因此 QEMU 上的用户态吞吐量无法反映真实串口性能，两者在真板都会收敛到 ~11.5 KB/s。
> 本文只讨论在 QEMU 上可可信测量的维度：内核态速度、CPU 效率、write() 延迟、功能覆盖。

---

## 1. 架构

| 维度 | Console（阻塞） | Async（Q11 最新） |
|------|----------------|------------------------|
| TX 路径 | write() → 轮询 LSR → 逐字节写 THR | write() → push ring buf → TX copier → 批量 send_bytes() |
| RX 路径 | ISR → tty-reader → ldisc → read() | ISR → RX copier → ring buf → tty-reader → ldisc → read() |
| 缓冲区 | 无 | 128 KB（RX/TX 各 64 KB HeapRb） |
| write() 行为 | 阻塞（等待 FIFO 有空位） | 非阻塞（push ring buf 即返回） |
| 空闲 CPU | 0% | 0%（External 模式，无 yield storm） |
| **tcdrain** | 隐式（每字节等待 LSR） | ✅ TCSBRK + DRAIN_WAKER |
| 非阻塞读 | ❌ | ✅ open/fcntl/ioctl 三入口 |
| **读超时** | ❌ | ✅ VTIME（axtask::future::timeout） |
| **唤醒机制** | PollSet (spinlock) | AtomicWaker (lock-free, O46) |
| **ldisc 缓冲** | — | 256B（Q10 扩容 3.2×） |
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
| `tcdrain()` （TCSBRK） | 隐式 | ✅ TCSBRK + O45 async（PollSet + DRAIN_WAKER） |
| Shell 功能（`ls`/`cd`/`pwd`） | ✅ | ✅ |
| 内核日志（`ax_println!`） | ✅ | ✅（polling TX 共存） |

---

## 5. 用户态吞吐量（⚠️ QEMU 不可比）

> **本节数据在 QEMU 上无比较意义。** 原因：
>
> Console `write(64)` 在 QEMU 上的执行路径：
> ```
> syscall → 64× (check LSR.THR_EMPTY → write THR) → return
> ```
> QEMU 的 LSR 永远 THR_EMPTY → 64 次 MMIO 写全部瞬时 → 总耗时 ~5 µs。
> **测的是纯 VFS + MMIO 速度，不是吞吐量。**
>
> Async `write(64) + tcdrain` 在 QEMU 上的执行路径：
> ```
> write() → push ring buf
> tcdrain → poll(ring buf not empty) → yield
>        → copier(send 16B) → yield
>        → poll(ring buf not empty) → yield    ← 64B 需要 4 轮 copier
>        → ... (重复 4 次，每次 3 个任务切换)
>        → poll(buf empty + LSR.TEMT) → return
> ```
> 64 字节涉及 **9 次任务上下文切换**（~30 µs/次），总 ~300 µs。
> **测的是 VFS + 任务切换速度，在 QEMU 上切换比 MMIO 贵 100 倍。**
>
> **真板预期**：UART 传输 64 字节需 5.6 ms（64 × 86.8 µs/byte），
> 碾压 9 µs 的任务切换开销。两者均收敛到 ~11.5 KB/s。

| 大小 | Async Q11 (QEMU) | 硬件理论/次 | 真板预测 |
|------|-----------------|-----------|----------|
| 64 B | 427.1 µs | 5555.6 µs | 5.98 ms |
| 256 B | 1175.0 µs | 22222.2 µs | 23.4 ms |
| 1024 B | 4704.0 µs | 88888.9 µs | 93.6 ms |
| 4096 B | 9052.6 µs | 355555.6 µs | 364.6 ms |

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
| **非阻塞读** | Async ✅ | Console 无此能力 |
| **读超时 (VTIME)** | Async ✅ | Q9 实现 |
| **唤醒延迟** | Async ⬆ 4x | AtomicWaker 50ns vs PollSet 200ns (Q8) |
| **吞吐量（真板）** | 持平 ~11.5 KB/s | 同受波特率限制 |
| **内存** | Console 胜 | 0 KB vs 128 KB |
| **复杂度** | Console 胜 | 更简单，但更局限 |

### 完整优化历史

| 阶段 | 日期 | 关键变更 |
|------|------|---------|
| Q0~Q4 | 05-31 | 驱动骨架 / VFS 集成 / RX+TX 全异步 |
| Q5 | 05-31 | IER 缓存 / ISR 合并 / 批量 I/O / NAPI |
| Q7 | 06-01 | yield storm / FIONBIO / benchmark / tcdrain async |
| **Q8** | 06-11 | NAPI 退出 / ISR 去锁 / IER 规范化 / O46 AtomicWaker |
| **Q9** | 06-11 | VTIME 读超时 |
| **Q10** | 06-11 | BUF_SIZE 80→256 / push_slice / read(&self) |
| **Q11** | 06-11 | tty unwrap / mm/access 批页 / sendfile / close_range / ws_col |
