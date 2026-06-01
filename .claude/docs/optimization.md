# optimization.md — 优化记录

> 由 project-docs-assistant 维护，feat/uart-async-dev2 分支。
> 2026-06-01 基于性能分析文档更新 Q7 优化计划。
> 条目格式: <!-- O{编号} --> 标记开头，支持 grep 精确定位。

---

## Q5 已完成（已归档）

<!-- tombstone: O25-O33 --> Archived to archive.md §optimization #O25-O33 2026-05-31 — 8 项 Q5 性能优化已完成

## Q5.1 已完成（2026-05-31）

| 编号 | 内容 | 效果 |
|------|------|------|
| O2/O34 | NAPI 中断合并 | 连续成功 ≥16 次后切轮询模式，batch=64，高吞吐时减少 90%+ IRQ |
| O4/O35 | FCR 阈值日志 | ISR bits 6-7 检查 FIFO 状态，记录触发阈值 |
| O7 | uart_16550 批量读写 API | receive_bytes/send_bytes 替代逐字节操作，减少函数调用开销 |
| O34 | TX interleave 修复 | TX copier 用本地 cursor 追踪已发位置，避免与 ax_println! 输出交错 |

## Q5.2 待做（feat/uart-async-dev2 分支）

| 编号 | 内容 | 优先级 | 说明 |
|------|------|--------|------|
| O21 | 用户态自动化测试 | 🟢 低 | Makefile test target（已完成） |
| O22 | 非阻塞模式测试 | 🟢 低 | ioctl(FIONBIO) → 升级为 Q7 O43 |

---

## Q7 用户态性能修复（2026-06-01 分析驱动）

> 基于 `docs/analysis/user-async-perf-analysis.md` 和 `docs/analysis/nonblocking-mode-analysis.md`
> 三大根因修复，预计在 dev2 分支实施。

| 编号 | 内容 | 优先级 | 说明 | 影响 |
|------|------|--------|------|------|
| **O42** | 修复 yield storm | 🔴 高 | `ProcessMode::Manual` → `External`，ldisc 独立 tty-reader 任务 + PollSet 注册 | 消除无数据时高频 yield-re-schedule，空闲 CPU 归零 |
| **O43** | 传播 FIONBIO nonblocking | 🔴 高 | Tty struct 添加 `nonblocking: AtomicBool`，传播到 `read_at()` → `ldisc.read()` | `ioctl(FIONBIO)` 对 TTY 读生效，无数据时立即返回 EAGAIN |
| **O44** | 修正 benchmark | 🟡 中 | TX 改 /dev/null → /dev/console，延迟加 tcdrain()，RX 加 raw mode 用户态测试 | benchmark 反映真实串口吞吐量（~11.5 KB/s） |

**O42 实施细节**:
- `ntty_async.rs`: 创建 `Arc<PollSet>`，传入 `ProcessMode::External(Box::new(move |waker| poll_rx.register(waker)))`
- `ldisc.rs`: External 模式自动创建 tty-reader 任务，register_rx_waker 使用 PollSet（不再 wake_by_ref）
- 代价：多一个内核任务（与旧 Console 相同）

**O43 实施细节**:
- `tty/mod.rs`: Tty struct 加字段 `nonblocking: AtomicBool`，`read_at()` 内用 `self.nonblocking.load(Acquire)`
- `tty/mod.rs`: DeviceOps ioctl 处理 FIONBIO → set nonblocking
- `ldisc.rs`: `read()` 方法接受 `nonblocking: bool` 参数 → `block_on(poll_io(...))` 用该参数

| 指标 | 目标 | 测量方法 | 当前（QEMU async） |
|------|------|---------|---------------------|
| 吞吐量 @115200 | > 10 KB/s (90% 线速) | write → tcdrain(), 5 秒批量 | TX: 未准确测量（写 /dev/null） |
| 延迟 P50 | < 500 µs | 单字节 write+tcdrain | ~1 µs（仅 ring buf push） |
| 延迟 P99 | < 2 ms | 同上 | — |
| 空闲 CPU | **0%**（无 yield storm） | 无数据 10 秒 | 偏高（yield storm） |
| 数据完整性 | 100% | 1 MB MD5 | ✅ |
| **非阻塞读 (Q7 后)** | `read()` 空数据立即 EAGAIN | `ioctl(FIONBIO)` + `read()` | ❌ 未生效 |

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

> O22 已升级为 Q7  O43（FIONBIO 传播 + 测试）。

| 编号 | 内容 | 优先级 | 说明 |
|------|------|--------|------|
| O21 | 用户态自动化测试 | 🟢 低 | Makefile test target（已完成） |

**已排除**: O17（中断分发效率）— 不需要实现。ISR 使用 AtomicWaker 直接唤醒（O(1)），无需 BTreeMap 分发机制。

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

