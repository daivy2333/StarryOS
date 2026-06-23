## Why

Q15 增量重融合需要先建立可重复的性能见证（RED 基线）。当前 benchmark 覆盖粗粒度（64/256/1024/4096B），无法展示 NS16550 16B FIFO 边界（1/15/16/17/31/32/33/48/49/64B）的调度台阶效应。M4 Sync 回退前 73.9x 退化（64B write+tcdrain 406µs→29.99ms）的根因是每 16B refill 依赖 100Hz tick 调度，必须先用细粒度基准量化当前状态，才能在后继 M1 修复中证明改善。

## What Changes

- **StarryOS `tests/benchmark.c`**：增加 FIFO 边界尺寸矩阵（1/15/16/17/31/32/33/48/49/64/256/1024/4096B），输出 raw samples、P50/P95、每轮 commit/tick/FIFO 信息，支持机器可解析格式
- **uart_16550 新增 feature-gated 诊断计数器**：`#[cfg(feature = "telemetry")]` 下暴露 `tx_poll`、`tx_no_progress`、`tx_hw_bytes` 等计数器，不启用时零开销
- **不改任何生产行为**：tx_copier_loop、TtyWrite、IER、tcdrain 均保持 pre-M4 基线不变

## Capabilities

### New Capabilities

- `benchmark-fifo-matrix`: 细粒度 FIFO 边界基准测试（1B~4096B），输出 raw/P50/P95 + 元数据（commit、tick、FIFO 深度）
- `uart-telemetry`: Feature-gated 异步 UART 诊断计数器（tx_poll / tx_no_progress / tx_hw_bytes），idle 时不持续增长

### Modified Capabilities

<!-- M0 changes NO production behavior — no existing specs modified -->

## Impact

- **StarryOS**：`tests/benchmark.c` + 可能新增 `scripts/` 解析脚本
- **uart_16550**：`src/async_/driver.rs` 热路径增加 `#[cfg(feature = "telemetry")]` 原子计数器（不启用时零开销）；`Cargo.toml` 新增 `telemetry` feature
- **不涉及**：外部 crate（axtask/axpoll/embassy-sync）、全局 tick、ISR 逻辑
