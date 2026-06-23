## Why

当前 `flush()` / `tcdrain` 在数据写入 TX ring buffer 后不等待硬件发送完成即返回成功。`flush()` 直接返回 `Ok(())`，`tcdrain` 绕开 driver 直接读 UART LSR 寄存器，且不检查 TX copier 是否正在搬运数据（ring data → staging → FIFO → shift register）。这导致上层调用方无法确认数据已真正发送到线路上，`write+tcdrain` 语义等价于 `write`，是正确性 bug。

## What Changes

- uart_16550 `driver.rs`：新增加 `TxCompletion` 快照结构体（`ring_empty`、`copier_active`、`staged_bytes`、`transmitter_empty`）和 `tx_copier_active`/`tx_staged_bytes` 状态字段；`tx_copier_loop` 跟踪 active/staged 状态；新增 `fn tx_completion()` 方法
- uart_16550 `UartPort` trait：新增 `fn transmitter_empty(&self) -> bool` 查询 UART TEMT 位
- uart_16550 `device_ops.rs`：`flush()` 从直接 `Ok(())` 改为轮询 `tx_completion()` 直到四条件全部满足，使用 `DRAIN_WAKER` + 协作 yield
- StarryOS `uart_init.rs`：`ArceOsUartPort` 实现 `transmitter_empty()`
- StarryOS `ctl.rs`：`tcdrain` 改用 `driver().tx_completion()` 替代直接 MMIO 访问

## Capabilities

### New Capabilities

- `tx-completion-tracking`: TX copier 在 poll 期间跟踪 `tx_copier_active`（是否在处理数据）和 `tx_staged_bytes`（已从 ring pop 但未确认发送的字节数），供 flush/tcdrain 判定排空完成
- `uart-temt-query`: `UartPort` trait 新增 `transmitter_empty()` 方法，查询 UART LSR TRANSMITTER_EMPTY 位，确认 shift register 是否已空

### Modified Capabilities

- `async-uart-core`: TX copier loop 维护 active/staged 状态；新增 `tx_completion()` completion 快照 API；flush 等待四阶段排空
- `arceos-adapter`: `ArceOsUartPort` 实现 `transmitter_empty()`；tcdrain syscall 改用 driver completion 快照

## Impact

- uart_16550 `driver.rs`：新增 ~60 行（结构体 + 状态字段 + 方法 + copier 修改）
- uart_16550 `device_ops.rs`：flush 从 3 行变为 ~20 行轮询循环
- uart_16550 `UartPort` trait：新增 1 个方法
- StarryOS `uart_init.rs`：新增 ~5 行 `transmitter_empty()` 实现
- StarryOS `ctl.rs`：tcdrain 简化为调用 driver API（~15 行替换 ~25 行）
- 不改变 `TtyWrite` 返回值（M3）、IER 所有权（M4）、M1 fast retry 行为
