## Why

Q15 增量重融 M1：修复 TX copier 在 UART FIFO 满（16B 边界）时无条件 `enable_tx_intr` + `Poll::Ready(())` 导致的无限 busy-poll 性能退化。当前 `tx_copier_loop` 在 FIFO 满时无法在同一个 poll 内等待 UART 排空，每次 refill 依赖 scheduler tick（QEMU ~10ms），64B write+tcdrain 从 ~406us 退化到 ~29.99ms（约 74x）。M1 通过有界 fast retry（32 次）在同一 poll 内等待 FIFO 排空，消除 tick 台阶的同时避免无限 busy-poll。

## What Changes

- uart_16550 `driver.rs` 新增 `TX_FAST_RETRY_LIMIT: usize = 32` 常量
- `tx_copier_loop`：当 `send_bytes() == 0` 时在同一个 `poll_fn` 内最多 spin retry 32 次，预算耗尽后才启用 THRE 中断 + 注册 waker + `Poll::Pending`
- telemetry `tx_no_progress` 计数器语义保持：记录 `send_bytes() == 0` 的轮次（含 retry 内的每次尝试）
- **不改动**：`TtyWrite` 返回值、IER 所有权、三阶段 drain、StarryOS kernel 代码

## Capabilities

### New Capabilities

- `tx-bounded-fast-retry`: TX copier 在同一 poll 内执行最多 32 次有界 retry，在 FIFO 满时避免不必要的调度器 yield

### Modified Capabilities

- `async-uart-core`: TX copier loop 行为变更 — 从无条件 yield 变为有界 retry + yield，`send_bytes() == 0` 时不再立即 `Poll::Ready(())`

## Impact

- 仅影响 `uart_16550/src/async_/driver.rs` 的 `tx_copier_loop` 函数
- 不影响 UartPort trait、ring buffer、ISR、device ops、StarryOS 适配层
- telemetry 计数器日志量增加（retry 内部 `send_bytes` 调用也会计数），但不影响 public API
- 验收依赖 M0 建立的 FIFO 边界矩阵 benchmark（已就绪）
