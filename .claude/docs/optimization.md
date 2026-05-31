# optimization.md — 优化记录

> 由 project-docs-assistant 维护，feat/uart-async-dev2 分支。
> 条目格式: <!-- O{编号} --> 标记开头，支持 grep 精确定位。

---

## Q5 性能优化：瓶颈分析（2026-05-31）

### 当前性能特征

| 指标 | 值 | 说明 |
|------|-----|------|
| 波特率 | 115200 bps | QEMU virt 默认 |
| FIFO 深度 | 16 字节 | NS16550A |
| Ring Buffer | 64 KiB × 2 | rx + tx |
| Copier Buf | 256 字节 | 中间搬运缓冲 |
| IRQ 频率 | ~823/秒 (估算) | FCR 阈值 14 字节时 |

---

## 瓶颈清单

<!-- O25 ✅ --> ### RX: 逐字节 FIFO 读取（高优先级）
- **位置**: `async_driver.rs:44-46` rx_copier_loop
- **问题**: `try_receive_byte()` 每字节一次 MMIO read，16 字节 FIFO 需要 16 次 `read_volatile`
- **改进**: 使用批量读取（`try_receive_bytes()` 或类似），一次 `read_volatile` 循环读取多个字节
- **预期收益**: 减少 90%+ RX MMIO 访问次数（16→1）

<!-- O26 ✅ --> ### TX: 逐字节 FIFO 写入（高优先级）
- **位置**: `async_driver.rs:67-68` tx_copier_loop
- **问题**: `try_send_byte()` 每字节一次 MMIO write，数据量大时开销显著
- **改进**: 批量写入，检查 FIFO 空余量后一次写满
- **预期收益**: 减少 TX MMIO 访问次数

<!-- O27 ✅ --> ### IER 操作：每次 read-modify-write（高优先级）
- **位置**: `uart_init.rs:69-72`
- **问题**: `enable_rx_intr()` = `read_ier()` + `write_ier()`，两次 MMIO。ISR 中 `disable_rx_intr()` + copier 中 `enable_rx_intr()` = 每 IRQ 4 次 IER MMIO
- **改进**: 缓存 IER 值在变量中，减少读操作；或考虑 per-CPU IER shadow register
- **预期收益**: 减少 50% IER MMIO 操作

<!-- O28 ✅ --> ### ISR：两次 MMIO critical section（中优先级）
- **位置**: `isr.rs:9-15`
- **问题**: ISR 先 `lock()` 读 ISR，再 `drop`，再 `disable_rx_intr()`（read+write IER）。SpinNoIrq 两次 acquire/release
- **改进**: 在同一个 SpinNoIrq 临界区内完成 ISR 读 + IER 写
- **预期收益**: ISR 延迟降低 ~30%

<!-- O29 ✅ --> ### copier buffer 过小（中优先级）
- **位置**: `async_driver.rs:13` COPIER_BUF_SIZE = 256
- **问题**: 256 字节对于 64 KiB ring buffer 太小。高吞吐时 copier 频繁 lock/unlock ring buffer
- **改进**: 增大到 1024 或与 ring buffer FIFO 阈值匹配
- **预期收益**: 减少 Mutex lock/unlock 频率

<!-- O30 ✅ --> ### double buffer lock in TX（中优先级）
- **位置**: `async_driver.rs:60-71`
- **问题**: TX copier 先 `pop_tx(lock)` → send → `push_tx(lock again)`。两段锁
- **改进**: 在单次锁内完成 pop + send + conditional pushback
- **预期收益**: 消除一次不必要的 Mutex acquire/release

<!-- O31 ✅ --> ### AtomicWaker 每次迭代注册（低优先级）
- **位置**: `async_driver.rs:50,75`
- **问题**: `RX_WAKER.register(cx.waker())` 每 loop 调用，内部使用 critical-section（disable_irqs/enable_irqs）
- **改进**: 仅在 waker 变化时注册（比较指针），或使用 `will_wake` 检查
- **预期收益**: 减少不必要的 critical-section 开销

<!-- O32 --> ### poll_fn closure 分配（低优先级）
- **位置**: `async_driver.rs:41,59`
- **问题**: 每次 loop 迭代创建新的 `poll_fn(|cx| {...})` 闭包
- **改进**: 提取为命名 async fn 或使用 loop + block_on 减少分配
- **预期收益**: 微小。Rust 编译器可能优化掉闭包分配

<!-- O33 --> ### Ring Buffer Mutex 竞争（中优先级）
- **位置**: `async_driver.rs:48,61,71` — `self.buffer.lock()`
- **问题**: copier 和 read_at/write_at 都竞争同一个 Mutex。高并发时 spin-wait
- **改进**: 考虑无锁 ring buffer（`ringbuf` 的 Producer/Consumer split），或使用 RwLock 区分读/写锁
- **预期收益**: 减少内核态阻塞时间

<!-- O34 --> ### NAPI 风格中断合并（中优先级，Q5 Q5.2）
- **位置**: ISR + copier 流程
- **问题**: 高吞吐时 IRQ 频率过高（>800/秒），每次触发完整 ISR + copier 路径
- **改进**: ISR 中禁用中断后切换到轮询模式，copier 处理完毕后切回中断
- **预期收益**: 高吞吐时 CPU 占用降低 50%+

<!-- O35 --> ### FCR 阈值优化（低优先级，Q5 Q5.1）
- **问题**: 当前 FCR 阈值未知（Console 设置的默认值），可能不是最优
- **改进**: 设置为 14（最大 FIFO 阈值），减少 IRQ 频率
- **预期收益**: IRQ 频率从 ~1640/秒（阈值 8）降至 ~823/秒（阈值 14）

<!-- O36 --> ### 零拷贝 RX 路径（远期，Q5 Q5.3）
- **问题**: 数据路径 = UART FIFO → copier buf → ring buf → 用户空间。两次 memcpy
- **改进**: mmap ring buffer 物理页到用户空间，消除用户态拷贝
- **预期收益**: 用户态 read 延迟降低 50%+

<!-- O37 --> ### kernel log TX 竞争（低优先级）
- **位置**: `ax_println!` → `axhal::console::write_bytes()`（Console polling TX）
- **问题**: 内核日志和 Shell stdout 都写 THR。Console TX 逐字节 polling，Async TX 也逐字节
- **改进**: 短期可接受；长期将 ax_println! 也走 ring buffer TX 路径
- **预期收益**: 消除罕见的输出字符交错

---

## 性能基准目标

| 指标 | 目标 | 测量方法 | 参考值 |
|------|------|---------|--------|
| 吞吐量 @115200 | > 10 KB/s (90% 线速) | 5 秒批量传输 | 线速 11,520 B/s |
| 延迟 P50 | < 500 µs | 100 次单字节 echo | 线延迟 86.8 µs/byte |
| 延迟 P99 | < 2 ms | 同上 | — |
| 空闲 CPU | 0% (完全挂起) | 无数据 10 秒 | ISR ~1.5 µs vs 1.2ms 间隔 |
| 数据完整性 | 100% | 1 MB MD5 | — |

### 硬件理论极限（NS16550 @ 115200 bps）

| 参数 | 值 |
|------|-----|
| 线速 | 11,520 B/s (10 bits/byte × 115200) |
| 单字节传输时间 | 86.8 µs |
| FIFO 深度 | 16 字节 |
| IRQ 频率 (阈值 14) | ~823/秒，间隔 1.22 ms |
| ISR 总延迟 | ~1.5 µs（< 0.1% 线时间） |
| MMIO 单次访问 | ~100-200 ns |

---

## 已完成优化

<!-- tombstone: O24 --> Archived to archive.md §optimization #O24 2026-05-31 — stride=4 问题已解决

---

## 远期优化方向（从旧 M 系列迁移）

<!-- O1 --> - 零拷贝 RX 路径
  - 路径: 硬件 → ringbuf → 用户空间，两次 memcpy
  - 方案: mmap ring buffer 物理页到用户空间（需 VMA 支持）
  - 优先级: 中 | 阶段: Q5+

<!-- O2 --> - NAPI 风格批量轮询
  - 中断触发后切轮询模式，处理完切回中断
  - 优先级: 高 | 阶段: Q5

<!-- O3 --> - DMA 支持
  - 通过 virtio-console 流式 DMA 卸载 CPU
  - 优先级: 高 | 阶段: Q6+

<!-- O4 --> - 中断合并 (coalescing)
  - 硬件 FCR 阈值 + 软件延迟合并
  - 优先级: 中 | 阶段: Q5

<!-- O5 --> - 优先级调度
  - 提高 copier 协程优先级减少延迟抖动
  - 优先级: 低 | 阶段: Q5+

<!-- O7 --> - uart_16550 批量读写 API
  - try_receive_batch/try_send_batch 减少逐字节 MMIO
  - 优先级: 高 | 阶段: Q5

<!-- O17 --> - 中断分发效率
  - register_irq_waker BTreeMap 查找开销 → 数组索引
  - 优先级: 中 | 阶段: Q5

## VisionFive2 真板优化（Q6 准备）

<!-- O38 --> - VisionFive2 UART 时钟适配
  - JH7110 UART 时钟频率与 QEMU virt 不同（1.8432 MHz vs 实际板载时钟）
  - 优先级: 高 | 阶段: Q6

<!-- O39 --> - 真实硬件 FIFO 深度验证
  - VisionFive2 的 NS16550 兼容 UART 可能有不同 FIFO 深度
  - 优先级: 中 | 阶段: Q6

<!-- O40 --> - DMA 通道发现与配置
  - 真板可能有 DMA 控制器可用，QEMU virt 没有
  - 优先级: 中 | 阶段: Q6+

<!-- O41 --> - 高速波特率支持（>115200）
  - 真板支持 230400/460800/921600 等高速率
  - 优先级: 低 | 阶段: Q6+

---

## 性能洞察（保留）

<!-- O19 --> ### 中断频率
- FCR 阈值 14 字节时，115200 bps 下 ~823 IRQ/秒
- ISR 开销 < 100 ns（清 IIR + AtomicWaker::wake）
- 1 Mbps 下 ~7,143 IRQ/秒

<!-- O8 --> ### 延迟分解
- RX 总延迟 = T_ISR + T_WAKE + T_DRAIN + T_COPY + T_RETURN
- 目标: < 500 µs @ 115200 bps
- 瓶颈通常在 T_WAKE（协程调度延迟）

## 测试与自动化（保留）

<!-- O21 --> - 用户态自动化测试
  - Makefile target: `make test-uart-async`
  - 优先级: 低 | 阶段: Q5+

<!-- O22 --> - 非阻塞模式测试
  - ioctl(FIONBIO) + open(O_NONBLOCK) WouldBlock 场景
  - 优先级: 低 | 阶段: Q5+
