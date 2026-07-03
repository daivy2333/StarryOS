# Spec Delta: optimization — ARC-202607031929

## REMOVED Requirements

### Requirement: O45/O46/O47 detailed historical plans

O45/O46/O47 的详细旧方案正文从 active `optimization/spec.md` 移除。O45/O46/O47 已有 tombstone，当前 active optimization 仅保留远期优化触发条件和当前 roadmap。

#### Scenario: Restore O45/O46/O47 details

- **WHEN** 开发者需要回查 O45 tcdrain、O46 AtomicWaker 或 O47 timeout 的旧详细方案
- **THEN** MUST use this carrier spec.

---

## 完整保留（Archive 区）

### O45/O46/O47-detail (Archive, optimization 2026-07-03)

```markdown
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
- DMA 失败时硬件可能永不唤醒（Q21 真板 O3/O40 风险）
- 用户态 poll() + SO_RCVTIMEO 需要内核支持 time 抽象

**优化方案**：

1. 引入 `embassy-time = "0.3"`（仅 Timer，不引入 Executor）
2. 在 axhal 实现 time driver 桩（依赖 axhal::time::current_ticks）
3. 改造 `poll_io` 接受 `Option<Duration>` 超时参数
4. 用 `embassy_futures::select!` 组合 poll_io + Timer

**风险评估**：

- 🔴 高：embassy-time 需要 time driver，必须在 axhal 适配 axtask 时钟
- 🟡 中：与现有 `axtask::future::block_on` 并存，引入两套 future 抽象
- 🟢 低：仅在用户态显式传递 timeout 时启用，向后兼容

**前置依赖**：

- Q21 DMA 决策完成（确认 DMA 失败路径是否真需要 timeout）
- axhal time driver 评估

**优先级**：🟡 中，Q20 触发条件性实现

</details>
```
