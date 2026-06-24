## Context

Q15 增量重融 M1 是 5 个增量中唯一的性能主修复。当前 `tx_copier_loop`（`uart_16550/src/async_/driver.rs:226`）在 `send_bytes() == 0` 时无条件调用 `enable_tx_intr()` 并返回 `Poll::Ready(())`，外层 `loop` 立即重新 poll，形成无限 busy-poll。QEMU 环境下 UART FIFO 排空后 THRE 中断到达，但 dispatcher 在下一个 scheduler tick（~10ms）才恢复 copier 任务，导致每次 16B refill 产生 ~10ms 台阶。

M0 已建立 FIFO 边界矩阵 benchmark（`tests/benchmark.c`）和 telemetry counters（`tx_poll`、`tx_no_progress`、`tx_hw_bytes`），可量化验证 M1 效果。

## Goals / Non-Goals

**Goals:**
- 在同一次 poll 内执行最多 32 次有界 retry，覆盖 UART FIFO 从满到有空位的典型排空窗口
- 预算耗尽后正确挂起（register waker + enable THRE + final recheck + Poll::Pending）
- 有进展时不计 retry 上限（cursor 前进 → retry 预算不消耗）
- telemetry 计数器语义保持一致

**Non-Goals:**
- 不改 `TtyWrite::write` 返回值（`()` → `usize`）— 那是 M3
- 不实现三阶段 completion drain（`tx_copier_active`/`tx_staged_bytes`/TEMT）— 那是 M2
- 不移 IER 所有权到 `UartPort` — 那是 M4
- 不扩大重试上限追性能（固定 32）
- 不改 StarryOS kernel 代码

## Decisions

### D1: 在有进展时重置 retry 计数器

**选择**：retry 计数器仅在连续 `send_bytes() == 0` 时递增。一旦 `send_bytes() > 0`（有进展），隐式通过 `cursor` 前进继续 poll，不消耗 retry 预算。

**替代方案**：全局累积计数（无论有进展与否都递增）→ 否决：传输大 buffer 时会因为总轮次超过 32 而提前挂起，违背"有进展就继续"的语义。

### D2: retry 在 poll_fn 闭包内部执行

**选择**：retry 循环放在 `poll_fn` 闭包内部，`send_bytes` 调用不跨越 `.await` 点。意味着 33 次 send_bytes 都在同一次 poll 内完成。

**替代方案**：retry 放在外层 `async fn` loop 中，每次 retry 间有 `.await` 点 → 否决：`.await` 意味着交出控制权给调度器，等于回到当前"每次 refill 一个 tick"的问题。

### D3: 预算耗尽后的处理顺序

**选择**：`TX_WAKER.register(cx.waker())` → `enable_tx_intr()` → 最终 `send_bytes` recheck。recheck 成功则 `Poll::Ready(())`，失败则 `Poll::Pending`。

**替代方案**：先 enable THRE 再 register waker → 风险：waker 未注册时 THRE 中断到达，ISR wake 被丢弃（AtomicWaker race）。正确顺序：先 register waker，再 enable THRE（中断到达时 waker 已就绪），最后 recheck（防止 THRE 在 enable 前已触发但 ISR 还未处理的窗口）。

### D4: `enable_tx_intr` 调用位置不变

**选择**：`enable_tx_intr()` 仍在预算耗尽时调用（与当前 `cursor < pending` 时的调用语义一致）。retry 期间不调用，仅在最终放弃时才启用中断。

**理由**：retry 期间 UART FIFO 正在排空，不需要 THRE 中断。只有确认"等不及了"才启用中断。

## Risks / Trade-offs

| Risk | Mitigation |
|------|------------|
| 32 次 retry 在真板慢 MMIO 上可能浪费 CPU | 固定上限 32，telemetry tx_no_progress 可观测浪费量。M1 失败回滚而不是扩大预算 |
| `Poll::Pending` 后 ISR 未触发 → copier 挂起 | ring buffer push 会 wake TX_WAKER；scheduler tick 也会重新 poll；非无限挂起 |
| telemetry tx_no_progress 因 retry 内部调用而激增 | 预期行为：M1 后 tx_no_progress 数值会变大（因为 retry 内部每次 send_bytes==0 都计数），但 idle 时不持续增长 |
| retry 内部 send_bytes 与 ISR 竞争 IER | ISR 已由 `disable_tx_intr` 禁用 THRE 位；retry 期间不操作 IER，无竞争 |
