## Why

当前 IER（Interrupt Enable Register）由 StarryOS `CACHED_IER: AtomicU8` + `write_ier()` 和 uart_16550 ISR 共同管理，形成双 owner 架构。StarryOS 通过 `enable_rx_intr`/`enable_tx_intr` 回调向 copier 暴露中断控制，ISR 通过直接 MMIO 写禁中断。这种分裂阻止了 uart_16550 crate 作为独立可复用异步 UART crate 的完整性 — 任何新 OS 移植都需要自己实现 IER 管理逻辑。

## What Changes

- uart_16550 `UartPort` trait 新增 `fn update_ier(&self, set: IER, clear: IER)` — IER 单 owner 接口
- copier (`start_rx_copier`/`start_tx_copier`/`rx_copier_loop`/`tx_copier_loop`) 移除 `enable_rx_intr`/`enable_tx_intr` 回调参数，改用 `self.uart.update_ier()`
- ISR (`uart_isr_handler`) 移除 `cached_ier: &AtomicU8` 参数，改用 `fn_disable_rx: fn()`/`fn_disable_tx: fn()` 函数指针
- `IsrRegisters` 移除 `disable_rx_intr`/`disable_tx_intr` 方法（MMIO 写移到 port 实现中）
- StarryOS `ArceOsUartPort` 实现 `update_ier()`（内部持有 `AtomicU8` 缓存 + lock + set_ier）
- StarryOS 删除 `CACHED_IER`、`write_ier()`、`enable_rx_intr()`、`enable_tx_intr()`

## Capabilities

### New Capabilities

- `ier-port-ownership`: IER 状态由 `UartPort` 实现层独占管理，通过 `update_ier(set, clear)` 单一接口，外部不再需要 `CACHED_IER` 或回调

### Modified Capabilities

- `async-uart-core`: copier 启动接口移除 IER 回调参数；ISR handler 签名变更
- `arceos-adapter`: `ArceOsUartPort` 实现 `update_ier()` 替代 `write_ier`/`enable_*_intr`；ISR wrapper 适配新签名

## Impact

- uart_16550 `driver.rs`：UartPort trait +1 方法；copier 移除 2 个 fn 参数（~10 行改动）
- uart_16550 `isr.rs`：移除 IsrRegisters::disable_* 方法（~30 行删除）；handler 签名变更
- StarryOS `uart_init.rs`：删除 CACHED_IER/write_ier/enable_* 函数（~25 行删除）；ArceOsUartPort 新增 update_ier（~10 行）；ISR wrapper 适配
- uart_16550 成为真正独立可复用 crate（不再需要 OS 层提供 IER 管理）
