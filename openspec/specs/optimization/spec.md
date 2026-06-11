# Spec: optimization — 优化记录

## Purpose

汇总 StarryOS 异步串口项目各阶段（Q5 / Q5.1 / Q7 已完成；Q6 / 远期待做）的性能优化条目，包含问题描述、当前影响、建议方案、优先级与状态。Q 编号对应 milestone（Q0~Q7）。

## Requirements

### Requirement: Q5 内核态性能优化 — 已完成

Q5 阶段（中断驱动 + NAPI 批量 I/O）所有优化 MUST 视为已落地且禁止回退；新增优化 MUST 在 Q5 基础上叠加，禁止重复造轮子。

**Q5.1 已完成（2026-05-31）**：

| 编号 | 内容 | 效果 |
|------|------|------|
| **O2 / O34** | NAPI 中断合并 | 连续成功 ≥16 次后切轮询模式，batch=64，高吞吐时减少 90%+ IRQ |
| **O4 / O35** | FCR 阈值日志 | ISR bits 6-7 检查 FIFO 状态，记录触发阈值 |
| **O7** | uart_16550 批量读写 API | `receive_bytes` / `send_bytes` 替代逐字节操作，减少函数调用开销 |
| **O34** | TX interleave 修复 | TX copier 用本地 cursor 追踪已发位置，避免与 ax_println! 输出交错 |

**Q5 已完成优化清单**：

| 编号 | 内容 | 效果 |
|------|------|------|
| **O25-O26** | RX/TX 批量 I/O | 单锁内排空/填满 FIFO |
| **O27** | IER 缓存（AtomicU8） | RMW → 单次 MMIO write |
| **O28** | ISR 合并 | 单临界区完成 read+write |
| **O29** | COPIER_BUF 256→1024 | 减少 lock 频率 |
| **O30** | TX 单次 buffer lock | 消除 double lock |
| **O31** | AtomicWaker skip | will_wake 检查 |
| **O33** | rx/tx 独立 Mutex | 消除伪竞争 |
| **O24** | stride=4 修复 | 已归档 |

#### Scenario: 优化热路径性能

- **WHEN** 开发者要提升 ISR / copier 性能
- **THEN** MUST 在 Q5 优化基础上叠加（IER 缓存、批量 I/O、waker skip、锁合并），禁止从零重写

### Requirement: Q7 用户态性能修复 — 已完成

Q7 优化 MUST 视为已落地；任何回退 MUST 附带 commit 证明性能回退可接受。

**Q7 用户态性能修复（2026-06-01 已完成）**：

| 编号 | 内容 | 优先级 | 影响 | 状态 |
|------|------|--------|------|------|
| **O42** | 修复 yield storm | 🔴 高 | 消除无数据时高频 yield-re-schedule | ✅ Manual→External |
| **O43** | 传播 FIONBIO nonblocking | 🔴 高 | ioctl(FIONBIO) 对 TTY 读生效 | ✅ Tty+ldisc+ctl |
| **O44** | 修正 benchmark | 🟡 中 | TX /dev/console + tcdrain + FIONBIO | ✅ 新建 benchmark.c |

**O42 实施细节**：

- `ntty_async.rs`：创建 `Arc<PollSet>`，传入 `ProcessMode::External(Box::new(move |waker| poll_rx.register(waker)))`
- `ldisc.rs`：External 模式自动创建 tty-reader 任务，`register_rx_waker` 使用 PollSet（不再 `wake_by_ref`）
- **代价**：多一个内核任务（与旧 Console 相同）

**O43 实施细节**：

- `tty/mod.rs`：Tty struct 加字段 `nonblocking: AtomicBool`，`read_at()` 内用 `self.nonblocking.load(Acquire)`
- `tty/mod.rs`：DeviceOps ioctl 处理 FIONBIO → set nonblocking
- `ldisc.rs`：`read()` 方法接受 `nonblocking: bool` 参数 → `block_on(poll_io(...))` 用该参数

#### Scenario: 修改 ntty_async / ldisc 模式

- **WHEN** 开发者要改 `ProcessMode` 或 tty-reader 行为
- **THEN** MUST 保持 O42 的 External 模式（避免 yield storm），禁止回退到 Manual + `wake_by_ref`

#### Scenario: 修 FIONBIO 相关逻辑

- **WHEN** 开发者要改 nonblocking 状态传播
- **THEN** MUST 同时检查 `tty/mod.rs` / `ldisc.rs` / `syscall/fs/ctl.rs` 三个入口（O43 + L140 教训）

### Requirement: Q8 驱动引擎打磨 — 已完成

Q8 阶段（2026-06-11）MUST 视为已落地；任何回退 MUST 附带 commit 证明无正确性/性能退化。

**Wave 1 — 正确性修复**：

| 编号 | 内容 | 优先级 | 说明 |
|------|------|--------|------|
| **Q8.1** | NAPI 退出修复 | 🔴 BugFix | `async_driver.rs` — `total==0` 时重置 `consecutive=0` + `enable_rx_intr()`，消除 NAPI 永不退出导致 CPU 空转问题 |
| **Q8.2** | ISR 去锁化 | 🔴 BugFix | `isr.rs` — 消除 `SpinNoIrq` 锁，实现无锁 ISR 路径，符合 ISR 极简原则 |
| **Q8.3** | IER 写路径规范化 | 🔴 BugFix | `uart_init.rs` — 用 `uart_16550::set_ier()` 替代裸 `write_volatile`，消除规则违规；`uart_16550` crate 新增 `set_ier()` 公共方法 |

**Wave 2 — 热路径优化**：

| 编号 | 内容 | 优先级 | 说明 |
|------|------|--------|------|
| **Q8.4** | copier waker 去重简化 | 🟡 优化 | `async_driver.rs` — 仅 `will_wake` 不同时才 `clone()+register`，减少 ~20-40ns/poll |
| **Q8.5** | DRAIN_WAKER 条件唤醒 | 🟡 优化 | `isr.rs` — 仅在 tcdrain 活跃时 `DRAIN_WAKER.wake()`，减少无意义原子操作 |

**Wave 3 — O46 AtomicWaker 推广**：

| 编号 | 内容 | 说明 |
|------|------|------|
| **Q8.6** | signalfd PollSet→AtomicWaker | `signalfd.rs` — 1 PollSet → 1 AtomicWaker |
| **Q8.7** | event PollSet→AtomicWaker | `event.rs` — 2 PollSet → 2 AtomicWaker |
| **Q8.8** | pipe PollSet→AtomicWaker | `pipe.rs` — 3 PollSet → 3 AtomicWaker（交叉唤醒 read→TX / write→RX / close→close） |
| **Q8.9** | pidfd PollSet→AtomicWaker | `pidfd.rs` + `task/mod.rs` + `task/ops.rs` — Arc 共享重构，进程退出时 AtomicWaker::wake() |

**总收益**：唤醒延迟 ~200ns→~50ns（8 个唤醒点），ISR 延迟降低 ~200ns（去锁化），NAPI 空闲 CPU 归零，消除 2 处规则违规（IER 裸写 + ISR 锁）。

#### Scenario: NAPI 模式下数据流停止

- **WHEN** RX copier 在 NAPI 模式（consecutive ≥ NAPI_THRESHOLD）且 `receive_bytes()` 返回 0
- **THEN** consecutive 重置为 0，enable_rx_intr() 被调用，下次 ISR 正常触发

#### Scenario: ISR 无锁执行

- **WHEN** UART 产生中断
- **THEN** ISR 无锁读取 ISR 寄存器 → 禁用对应中断 → AtomicWaker::wake() → 返回（全流程 ~1.5 µs）

#### Scenario: IER 通过安全 API 写入

- **WHEN** copier 调用 enable/disable 中断函数
- **THEN** IER 通过 `uart_16550::Uart16550::set_ier()` 写入，CACHED_IER 与硬件 IER 一致

### Requirement: Q6 真板性能优化 — 待做（VisionFive2 拿到后）

VisionFive2 真板拿到后 MUST 完成 O38 / O39 / O3 / O40 / O41 五项优化；其中 O38（时钟适配）为最高优先级。

| 编号 | 内容 | 优先级 | 说明 |
|------|------|--------|------|
| **O38** | VisionFive2 UART 时钟适配 | 🔴 高 | JH7110 时钟不同于 QEMU |
| **O39** | 真板 FIFO 深度验证 | 🟡 中 | 可能不同于 16 字节 |
| **O3** | DMA 支持 | 🟡 中 | 真板可能有 DMA 控制器 |
| **O40** | DMA 通道配置 | 🟡 中 | — |
| **O41** | 高速波特率支持 | 🟢 低 | 230400+ |

#### Scenario: 真板启动失败或串口无输出

- **WHEN** VisionFive2 上 UART 无输出 / 数据乱码
- **THEN** MUST 优先排查 O38（时钟配置）而非波特率或软件路径

### Requirement: 远期优化（优先级低，不确定是否做）

远期优化条目 MUST 在评估 ROI 后决定是否实现；不作为里程碑硬性要求。

| 编号 | 内容 | 优先级 | 说明 |
|------|------|--------|------|
| **O45** | tcdrain 真异步化 | 🟡 中 | ✅ PollSet + DRAIN_WAKER，消除 `wake_by_ref` 自旋 |
| **O46** | AtomicWaker 模式推广 | 🟡 中 | ✅ 已完成（2026-06-11 Q8）：pipe(3)/signalfd(1)/pidfd(1)/event(2) 共 8 个 PollSet→AtomicWaker，唤醒延迟 ~200ns→~50ns |
| **O47** | embassy-time 超时机制 | 🟡 中 | ✅ 已完成（2026-06-11 Q9）：改用 axtask::future::timeout() 实现 VTIME 读超时，无需 embassy-time 依赖 |
| **O1 / O36** | 零拷贝 RX | — | mmap ring buffer 到用户空间 |
| **O5** | 协程优先级调度 | — | 取决于 axtask 支持 |
| **O37** | kernel log TX 合并 | — | `ax_println!` 走 ring buffer |
| **O32** | poll_fn 闭包 | — | 编译器可能已优化 |

### Requirement: 2026-06-11 死代码审计后续优化

本次审计（清理 8 项真死代码，保留 5 项预留接口）发现的后续优化机会。

| 编号 | 内容 | 优先级 | 说明 |
|------|------|--------|------|
| **O48** | memtrack 模块集成 | 🟢 低 | `kernel/src/pseudofs/dev/memtrack.rs` — 内存追踪功能已编写但从未集成（`run_memory_analysis` 无调用者）。Q6 真板调试时可能需要，可评估是否启用 |
| **O49** | ProcessMode::Manual 移除 | 🟢 低 | `ldisc.rs:37` — Q7 后仅 External/None 模式被构造，Manual 变体可通过重构 match 分支移除（需更新 ldisc.rs:265 匹配） |
| **O50** | 预留接口评估 | 🟢 低 | `create_pty_master`（tty/mod.rs）、`DeviceMmap::ReadOnly`（device.rs）、`clear_elf_cache`/`cleanup_task_tables`（memtrack 引用链）— 当前用 `#[allow(dead_code)]` 标注，未来如有需求可恢复或彻底移除 |

#### Scenario: 评估 O48 memtrack 模块

- **WHEN** Q6 真板到位后需要内存调试工具
- **THEN** 可恢复 `memtrack.rs` 的集成调用（当前代码完整，仅缺 `/dev/memtrack` 的设备注册）

### Requirement: Q12 Embassy 调研驱动的近期优化 — 待实现（路径 A）

基于 2026-06-11 embassy UART 架构调研（`.claude/analysis/embassy-uart-evaluation.md`），路径 A（最小借鉴）的三项优化 MUST 在 Q12 阶段完成，无需修改架构即可获得可量化收益。

| 编号 | 内容 | 优先级 | 说明 | 预期收益 |
|------|------|--------|------|------|
| **O51** | `atomic_ring_buffer` 替换 `HeapRb + Mutex` | 🟡 中 | 引入 `embassy_hal_internal::atomic_ring_buffer::RingBuffer`（lock-free SPSC，~768 行纯 Rust），替换 `kernel/src/drivers/ring_buffer.rs` 的 `HeapRb<u8>` + `axsync::Mutex`。消除 ring buffer 操作中的 mutex 开销，为未来 ISR 直接搬运铺路。 | 消除每次 push/pop 的 mutex lock/unlock（~100ns/op） |
| **O52** | `embedded_io_async` trait 实现 | 🟢 低 | 为 `AsyncUartReader`/`AsyncUartWriter` 实现 `embedded_io_async::Read`/`Write`/`BufRead` trait（社区标准）。不改动核心数据路径，仅新增 trait impl 层。 | 标准化接口，可与 Rust 嵌入式生态互通 |
| **O53** | TC 硬件寄存器 tcdrain 优化 | 🟢 低 | 用 NS16550 `LSR::TRANSMITTER_EMPTY` (bit 6) + TX ISR 替代 `TCDRAIN_ACTIVE: AtomicBool` 软件标志。embassy 使用 `tcie`（TC interrupt enable）硬件寄存器做等价优化（[buffered.rs:102-115](https://github.com/embassy-rs/embassy/blob/a7d1e449207c184c5a27ca9da3490b492257dfb4/embassy-stm32/src/usart/buffered.rs#L102-L115)）。 | 删除 `TCDRAIN_ACTIVE` 软件状态，减少 `load-acquire` 开销 |

**实施约束**：
- O51/O52/O53 均不修改 ISR 逻辑，不引入 `embassy-executor`
- O52 为纯 trait impl，零改动核心路径
- O51 需要完整单元测试覆盖（参考 embassy `atomic_ring_buffer` 的 [L527-768 测试](https://github.com/embassy-rs/embassy/blob/a7d1e449207c184c5a27ca9da3490b492257dfb4/embassy-hal-internal/src/atomic_ring_buffer.rs#L527-L768)）

#### Scenario: 引入 atomic_ring_buffer 后性能退化

- **WHEN** `RingBufRx/RingBufTx` 从 `HeapRb + Mutex` 迁移到 `RingBuffer` 后 benchmark 出现退化
- **THEN** MUST 检查 `Acquire`/`Release` 内存序是否与 embassy 原实现一致（[L236-260](https://github.com/embassy-rs/embassy/blob/a7d1e449207c184c5a27ca9da3490b492257dfb4/embassy-hal-internal/src/atomic_ring_buffer.rs#L236-L260)），禁止降级为 `Relaxed`

### Requirement: 远期优化（路径 B — 未来评估）

embassy 调研中识别的架构级优化，MUST 在路径 A（Q12）完成并量化收益后再评估实施。

| 编号 | 内容 | 优先级 | 说明 | 前置条件 |
|------|------|--------|------|------|
| **O54** | ISR 直接搬运（移除 copier 任务） | 🟡 中 | ISR 中直接执行 FIFO→ring buffer 搬运（embassy `BufferedUart` 模式），消除 RX/TX 两个 copier 任务的任务切换开销。需重新评估 NS16550 ISR 延迟（当前 ~1.5 µs，加搬运后预估 ~3-5 µs @ 16 字节 FIFO）。 | O51 就位 + benchmark 验证无退化 |
| **O55** | 半满/IDLE 唤醒策略 | 🟢 低 | 引入 embassy 的 `is_half_full()` 唤醒阈值 + IDLE line 检测（`ReceptionTimeout` 中断），减少高频 RX 下的 copier 唤醒次数。当前 NAPI 阈值（16 次连续成功）效果类似但更复杂。 | O51 或 O54 就位 |

**评估标准**：路径 B 的每项 MUST 在 Q12 完成后用 benchmark 量化路径 A 收益，再决定是否投入。禁止在未验证路径 A 收益的情况下直接实施路径 B。

**O45 — tcdrain 真异步化 详细方案**：

当前 TCSBRK 实现（`ctl.rs:43-57`）：

```rust
block_on(poll_fn(|cx| {
    if DRIVER.tx.lock().is_empty()
        && uart.lsr().contains(LSR::TRANSMITTER_EMPTY) {
        return Ready(Ok(0));
    }
    cx.waker().wake_by_ref();  // ← 协作自旋，每次失败立即重调度
    Pending
}))
```

**问题**：`wake_by_ref()` + `Pending` 产生协作式自旋。64 字节数据需要 TX copier 发送 4 批（每批 16 字节 FIFO），tcdrain 每次检查 ring buffer 非空 → 重调度 → copier 发一批 → 重调度 → ... 共 9 次任务切换（~270 µs QEMU）。

**优化方案**：用 PollSet 注册替代自旋。

```rust
block_on(poll_fn(|cx| {
    let mut tx = DRIVER.tx.lock();
    if tx.is_empty() {
        drop(tx);
        if uart.lsr().contains(LSR::TRANSMITTER_EMPTY) { return Ready(Ok(0)); }
        TX_WAKER.register(cx.waker());  // UART 还在发 → 等 TX ISR 唤醒
    } else {
        tx.poll.register(cx.waker());   // ring buf 有数据 → 等 copier pop 唤醒
    }
    Pending
}))
```

**关键**：`RingBufTx::pop()` 已调用 `self.poll.wake()`（`ring_buffer.rs:48`）。只需在 TCSBRK 中注册到 `tx.poll`，copier 每清空一批数据就会唤醒 tcdrain。

**预期效果**：

- QEMU：切换次数从 9 降至 ~4，延迟从 ~300 µs 降至 ~130 µs
- 真板：9 µs → 4 µs（可忽略，但更优雅）

**注意**：TX_WAKER 是 AtomicWaker（单槽），TX copier 也注册在上面。tcdrain 注册会覆盖 copier。需添加独立的 drain PollSet 或改用定时器补偿。

**O46 — AtomicWaker 模式推广 详细方案**：

**现状**（2026-06-05 评估）：

| 驱动 | 当前唤醒机制 | ISR 复杂度 | 唤醒延迟 |
|------|------------|-----------|----------|
| `kernel/src/drivers/isr.rs` (UART) | `static AtomicWaker` × 3（RX/TX/DRAIN） | O(1)，~1.5 µs | ~50 ns |
| `kernel/src/file/pipe.rs:34-56` | `Arc<PollSet>` × 3（rx/tx/close） | 通用 API | ~200 ns |
| `kernel/src/file/signalfd.rs:85-93` | `Arc<PollSet>` | 通用 API | ~200 ns |
| `kernel/src/file/pidfd.rs:20` | `Arc<PollSet>` | 通用 API | ~200 ns |

**优化方案**：将 pipe / signalfd / pidfd 改造成与 UART 一致的 AtomicWaker 静态分发模式。

- `pipe.rs`：在 ISR 端（写者唤醒 rx、读者唤醒 tx、close 唤醒 close）增加 `static ATOMIC_WAKER_PIPE_{RX,TX,CLOSE}`，删除 `PollSet` 字段
- `signalfd.rs`：增加 `static SIGNAL_WAKER`，信号到达时 `wake()`
- `pidfd.rs`：增加 `static EXIT_WAKER`，进程退出时 `wake()`

**预期收益**：

- 唤醒延迟：~200 ns → ~50 ns（×3 文件 = 6 个唤醒点）
- 内存：~1 KB PollSet → 24 B × N（按 waker 数）
- 代码量：减少 ~30 行（PollSet 注册样板）
- 一致性：所有驱动统一 ISR 唤醒模式，code review 更简单

**风险评估**：

- ⚠️ 唤醒方变静态，需在 spawn 时绑定（pipe.rs 已是 spawn 模型，零影响）
- ⚠️ pipe 的 close 路径需要信号源在 file drop 时唤醒（无 ISR），但可用 `static` 即可

**优先级**：🟡 中，量化收益明确（~150ns × 6 唤醒点 + 一致性提升），但需逐文件验证

**O47 — 超时机制 详细方案**：

> ⚠️ **2026-06-11 更新**：以下方案描述的是最初计划的 embassy-time 路径，但 Q9 实际采用了更简单的方案——复用 `axtask::future::timeout()`（无需新依赖）。以下原方案归档保留供参考。

<details>
<summary>原 embassy-time 方案（未实施）</summary>

**现状问题**：

`axtask::future::block_on(poll_io(...))` 是**永久阻塞**的，调用者无 timeout 能力：

```rust
// kernel/src/file/pipe.rs:123
block_on(poll_io(self, IoEvents::IN, self.nonblocking(), || {
    self.poll_rx.poll_io(cx, ...);
    // 无 timeout 选项，无数据时永远 Pending
}))
```

**潜在影响**：

- 用户态 read() 卡死后只能 SIGKILL，无法 SIGALRM 解除（无 setitimer 集成）
- DMA 失败时硬件可能永不唤醒（Q6 真板 O3/O40 风险）
- 用户态 poll() + SO_RCVTIMEO 需要内核支持 time 抽象

**优化方案**：

1. 引入 `embassy-time = "0.3"`（仅 Timer，不引入 Executor）
2. 在 axhal 实现 time driver 桩（依赖 axhal::time::current_ticks）
3. 改造 `poll_io` 接受 `Option<Duration>` 超时参数
4. 用 `embassy_futures::select!` 组合 poll_io + Timer

**实施示例**：

```rust
use embassy_time::{Timer, Duration};

block_on(async {
    let res = embassy_futures::select::select(
        poll_io_future,
        Timer::after(Duration::from_millis(100)),
    ).await;
    match res {
        embassy_futures::select::Either::First(r) => r,
        embassy_futures::select::Either::Second(_) => Err(EAGAIN),
    }
})
```

**风险评估**：

- 🔴 高：embassy-time 需要 time driver，必须在 axhal 适配 axtask 时钟
- 🟡 中：与现有 `axtask::future::block_on` 并存，引入两套 future 抽象
- 🟢 低：仅在用户态显式传递 timeout 时启用，向后兼容

**前置依赖**：

- Q6 真板验证完成（确认 DMA 失败路径是否真需要 timeout）
- axhal time driver 评估

**优先级**：🟡 中，Q6 触发条件性实现

</details>

#### Scenario: 评估远期优化 ROI

- **WHEN** 开发者考虑实现 O45 / O46 / O47 / O1 / O5 / O37 / O32 之一
- **THEN** MUST 评估实施成本 vs 性能收益（O1 / O37 高成本低收益需充分论证）

### Requirement: 已排除优化 — 不实施

通用分发结构类优化 MUST 在专用驱动场景下禁止实施。`O17`（中断分发效率）已明确排除：ISR 使用 AtomicWaker 直接唤醒（O(1)），无需 BTreeMap 分发机制。详见 `learned` L128。

**Embassy 误用场景**（2026-06-05 评估，详见 `learned` L81~L84）：

| 反优化 | 当前实现 | Embassy 替代 | 排除原因 |
|--------|----------|--------------|----------|
| **OE1** Channel 替换 HeapRb | `ringbuf::HeapRb<u8>` (SPSC) | `embassy_sync::Channel<u8, N>` (MPMC) | 失去 lock-free SPSC，多一层间接，heap 灵活性丧失 |
| **OE2** Mutex 替换 SpinNoPreempt | `Arc<SpinNoPreempt<...>>` | `embassy_sync::Mutex` | 同步临界区加异步 Mutex 反而更慢，且无法跨 `.await` 持有 |
| **OE3** Watch 替换 AtomicBool | `AtomicBool` (FIONBIO) | `embassy_sync::Watch<bool>` | 单 bool 用 Watch 是杀鸡用牛刀，AtomicBool 更直接 |
| **OE4** Semaphore 计数 NAPI | 状态机 + 计数器 | `embassy_sync::Semaphore` | 错误工具（Semaphore 是资源计数，不是事件计数）|
| **OE5** select! 替换手动 poll | 手动 `block_on(poll_io(...))` | `embassy_futures::select!` | axtask::future 不可与 select! 宏组合，需切换 executor |

**判定原则**：项目采用极简 embassy-sync 子集（仅 `AtomicWaker`），任何"用 embassy 包装替换简单 Rust 原语"的提案 MUST 先用 `codegraph_impact` 评估改动范围 + 性能基准，否则禁止实施。

#### Scenario: 评估 O17 类"通用分发"优化

- **WHEN** 开发者考虑引入 BTreeMap / HashMap 等通用分发结构
- **THEN** MUST 评估 waker 数量：固定少数 → AtomicWaker；通用动态 → register_irq_waker。专用驱动场景下禁止过度设计

#### Scenario: 评估 embassy 包装替换

- **WHEN** 开发者提议用 embassy 同步原语（Channel / Mutex / Watch / Semaphore）替换现有实现
- **THEN** MUST 先证明：(1) 当前实现有可测性能问题，(2) embassy 方案在该场景下更快/更简洁，(3) 不与 axtask 架构冲突。**禁止**为"用 embassy"而替换

### Requirement: 性能指标基线与硬件理论极限

性能测试与对比 MUST 基于下表的基线数据；任何指标声明 MUST 标注 QEMU / 真板可信度。

**NS16550 @ 115200 bps 硬件理论极限**：

| 参数 | 值 |
|------|-----|
| 线速 | 11,520 B/s（10 bits/byte × 115200） |
| 单字节传输时间 | 86.8 µs |
| FIFO 深度 | 16 字节 |
| IRQ 频率（阈值 14） | ~823/秒，间隔 1.22 ms |
| ISR 总延迟 | ~1.5 µs（< 0.1% 线时间） |
| MMIO 单次访问 | ~100~200 ns |

**当前 QEMU async 性能指标**：

| 指标 | 目标 | 测量方法 | 当前 |
|------|------|---------|------|
| 吞吐量 @115200 | > 10 KB/s（90% 线速） | `write → tcdrain()`，5 秒批量 | TX: 未准确测量（写 /dev/null） |
| 延迟 P50 | < 500 µs | 单字节 `write+tcdrain` | ~1 µs（仅 ring buf push） |
| 延迟 P99 | < 2 ms | 同上 | — |
| 空闲 CPU | **0%**（无 yield storm） | 无数据 10 秒 | 偏高（yield storm） |
| 数据完整性 | 100% | 1 MB MD5 | ✅ |
| **非阻塞读（Q7 后）** | `read()` 空数据立即 EAGAIN | `ioctl(FIONBIO)` + `read()` | ❌ 未生效 |

**CPU 占用对比**（统一数据量 102,400 字节）：

- Console：3,835 cycles/byte
- Async：268 cycles/byte（效率高 14.3 倍）

**RX 性能**（内核态 Ring Buffer 直接测，绕过 TTY）：

- 吞吐量：588,776 KB/s
- 延迟 P50：600 ns

**QEMU 时序欺骗边界**（`learned` L141）：

- QEMU 16550 不仿真串口线延迟，所有 tcdrain/LSR 轮询的吞吐量测试在 QEMU 上**不可信**
- 真板预期：VisionFive2 @ 115200 bps → ~11.5 KB/s
- **可靠指标（QEMU 也可测）**：内核态 ring buffer 速度、`write()` 延迟、CPU cycles/byte

#### Scenario: 声明性能数字

- **WHEN** 开发者 / 用户要声明某项性能指标
- **THEN** MUST 注明：(1) QEMU 还是真板，(2) 测试方法（绕过 TTY / 完整链路），(3) 数据量。**禁止**用 QEMU 吞吐量冒充真板吞吐
