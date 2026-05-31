# optimization.md — 优化记录

> 由 project-docs-assistant 维护，feat/uart-async-dev2 分支。
> 条目格式: <!-- O{编号} --> 标记开头，支持 grep 精确定位。

---

## Q5 已完成（已归档）

<!-- tombstone: O25-O33 --> Archived to archive.md §optimization #O25-O33 2026-05-31 — 8 项 Q5 性能优化已完成

## Q5.1/Q5.2 待做（feat/uart-async-dev2 分支）

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

## 已完成优化（Q5）

| 编号 | 内容 | 效果 |
|------|------|------|
| O25-O26 | RX/TX 批量 I/O | 单锁内排空/填满 FIFO |
| O27 | IER 缓存（AtomicU8） | RMW → 单次 MMIO write |
| O28 | ISR 合并 | 单临界区完成 read+write |
| O29 | COPIER_BUF 256→1024 | 减少 lock 频率 |
| O30 | TX 单次 buffer lock | 消除 double lock |
| O31 | AtomicWaker skip | will_wake 检查 |
| O33 | rx/tx 独立 Mutex | 消除伪竞争 |
| O24 | stride=4 修复 | 已归档 |

---

## 待做优化（规划）


| 编号 | 内容 | 优先级 | 说明 |
|------|------|--------|------|
| O2/O34 | NAPI 中断合并 | 🟡 中 | 高吞吐时切轮询模式 |
| O4/O35 | FCR 阈值调优 | 🟢 低 | 确认 Console 设置的阈值 |
| O7 | uart_16550 批量读写 API | 🟡 中 | 已用单锁 batch 替代，可进一步优化 crate |
| O17 | 中断分发效率 | 🟢 低 | BTreeMap → 数组索引 |
| O21 | 用户态自动化测试 | 🟢 低 | Makefile test target |
| O22 | 非阻塞模式测试 | 🟢 低 | ioctl(FIONBIO) |

### Q6：真板拿到后

| 编号 | 内容 | 优先级 | 说明 |
|------|------|--------|------|
| O38 | VisionFive2 UART 时钟适配 | 🔴 高 | JH7110 时钟不同于 QEMU |
| O39 | 真板 FIFO 深度验证 | 🟡 中 | 可能不同于 16 字节 |
| O3 | DMA 支持 | 🟡 中 | 真板可能有 DMA 控制器 |
| O40 | DMA 通道配置 | 🟡 中 | — |
| O41 | 高速波特率支持 | 🟢 低 | 230400+ |

### 远期（优先级低，不确定是否做）

| 编号 | 内容 | 说明 |
|------|------|------|
| O1/O36 | 零拷贝 RX | mmap ring buffer 到用户空间 |
| O5 | 协程优先级调度 | 取决于 axtask 支持 |
| O37 | kernel log TX 合并 | ax_println! 走 ring buffer |
| O32 | poll_fn 闭包 | 编译器可能已优化 |

---

