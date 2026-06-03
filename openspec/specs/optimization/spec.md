# optimization/spec.md — 优化记录

> 迁移自 .claude/docs/optimization.md，2026-06-03
> 条目格式: O{编号} 标记开头，支持 grep 精确定位。

---

## Purpose

记录项目中发现的优化点和改进方向，持续提升代码质量和性能。

## Requirements

### Requirement: 优化点记录

发现的性能瓶颈、代码异味、技术债务 MUST 记录，包含当前影响和建议方案。

#### Scenario: 发现优化机会

- **WHEN** 开发者发现代码中存在性能问题、重复代码、过度复杂设计等
- **THEN** 必须记录到 optimization/spec.md，包含：问题描述、当前影响、建议方案、优先级

#### Scenario: 评估优化价值

- **WHEN** 开发者需要决定是否进行某项优化
- **THEN** 可以参考 optimization/spec.md 中的记录，评估影响范围和收益

### Requirement: 优化完成追踪

已完成的优化 MUST 记录完成状态，保留历史记录。

#### Scenario: 完成优化

- **WHEN** 开发者完成了某项优化工作
- **THEN** 必须更新 optimization/spec.md，标记为已完成，记录完成日期和实际效果

### Requirement: 优化优先级管理

优化点 MUST 有优先级排序，合理安排优化顺序。

#### Scenario: 规划优化计划

- **WHEN** 开发者制定优化计划时
- **THEN** 可以参考 optimization/spec.md 中的优先级标注，优先处理高优先级优化点

---

## 已完成优化

### Q5 性能优化（2026-05-31）

| 编号 | 内容 | 效果 |
|------|------|------|
| O25-O26 | RX/TX 批量 I/O | 单锁内排空/填满 FIFO |
| O27 | IER 缓存（AtomicU8） | RMW → 单次 MMIO write |
| O28 | ISR 合并 | 单临界区完成 read+write |
| O29 | COPIER_BUF 256→1024 | 减少 lock 频率 |
| O30 | TX 单次 buffer lock | 消除 double lock |
| O31 | AtomicWaker skip | will_wake 检查 |
| O33 | rx/tx 独立 Mutex | 消除伪竞争 |
| O34 | TX interleave 修复 | 本地 cursor 追踪已发位置 |
| O2/O34 | NAPI 中断合并 | 高吞吐时减少 90%+ IRQ |
| O4/O35 | FCR 阈值日志 | ISR bits 6-7 检查 FIFO 状态 |
| O7 | uart_16550 批量读写 API | receive_bytes/send_bytes 替代逐字节 |

### Q7 用户态性能修复（2026-06-01）

| 编号 | 内容 | 优先级 | 影响 | 状态 |
|------|------|--------|------|------|
| **O42** | 修复 yield storm | 🔴 高 | 消除无数据时高频 yield-re-schedule | ✅ Manual→External |
| **O43** | 传播 FIONBIO nonblocking | 🔴 高 | ioctl(FIONBIO) 对 TTY 读生效 | ✅ Tty+ldisc+ctl |
| **O44** | 修正 benchmark | 🟡 中 | TX /dev/console + tcdrain + FIONBIO | ✅ 新建 benchmark.c |

**O42 实施细节**:
- `ntty_async.rs`: 创建 `Arc<PollSet>`，传入 `ProcessMode::External(Box::new(move |waker| poll_rx.register(waker)))`
- `ldisc.rs`: External 模式自动创建 tty-reader 任务，register_rx_waker 使用 PollSet（不再 wake_by_ref）
- 代价：多一个内核任务（与旧 Console 相同）

**O43 实施细节**:
- `tty/mod.rs`: Tty struct 加字段 `nonblocking: AtomicBool`，`read_at()` 内用 `self.nonblocking.load(Acquire)`
- `tty/mod.rs`: DeviceOps ioctl 处理 FIONBIO → set nonblocking
- `ldisc.rs`: `read()` 方法接受 `nonblocking: bool` 参数 → `block_on(poll_io(...))` 用该参数

---

## 待做优化

### Q5.2 待做

| 编号 | 内容 | 优先级 | 说明 |
|------|------|--------|------|
| O21 | 用户态自动化测试 | 🟢 低 | Makefile test target（已完成） |

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

## 性能基线

### 硬件理论极限（NS16550 @ 115200 bps）

| 参数 | 值 |
|------|-----|
| 线速 | 11,520 B/s (10 bits/byte × 115200) |
| 单字节传输时间 | 86.8 µs |
| FIFO 深度 | 16 字节 |
| IRQ 频率 (阈值 14) | ~823/秒，间隔 1.22 ms |
| ISR 总延迟 | ~1.5 µs（< 0.1% 线时间） |
| MMIO 单次访问 | ~100-200 ns |

### 当前性能指标（QEMU async）

| 指标 | 目标 | 当前 |
|------|------|------|
| 吞吐量 @115200 | > 10 KB/s (90% 线速) | TX: 未准确测量（写 /dev/null） |
| 延迟 P50 | < 500 µs | ~1 µs（仅 ring buf push） |
| 延迟 P99 | < 2 ms | — |
| 空闲 CPU | **0%**（无 yield storm） | 偏高（yield storm） |
| 数据完整性 | 100% | ✅ |
| 非阻塞读 | `read()` 空数据立即 EAGAIN | ❌ 未生效 |

**已排除**: O17（中断分发效率）— 不需要实现。ISR 使用 AtomicWaker 直接唤醒（O(1)），无需 BTreeMap 分发机制。
