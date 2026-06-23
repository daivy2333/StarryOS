## 1. uart_16550 核心状态跟踪

- [ ] 1.1 在 `driver.rs` 新增 `TxCompletion` 结构体（`ring_empty`、`copier_active`、`staged_bytes`、`transmitter_empty`）
- [ ] 1.2 在 `AsyncUartDriver` 新增 `tx_copier_active: AtomicBool` 和 `tx_staged_bytes: AtomicUsize` 字段
- [ ] 1.3 实现 `fn is_ring_empty(&self) -> bool`（via RingBufTx 新方法或 ring accessor）
- [ ] 1.4 实现 `fn tx_completion(&self) -> TxCompletion` 快照方法
- [ ] 1.5 修改 `tx_copier_loop`：set `tx_copier_active=true` 在 poll_fn 入口；clear `tx_copier_active=false` 在 Pending 路径前
- [ ] 1.6 修改 `tx_copier_loop`：pop_batch 后 `tx_staged_bytes += N`；send_bytes >0 后 `tx_staged_bytes -= S`
- [ ] 1.7 `RingBufTx` 新增 `fn is_empty(&self) -> bool` 方法

## 2. uart_16550 UartPort trait 扩展

- [ ] 2.1 在 `UartPort` trait 新增 `fn transmitter_empty(&self) -> bool`
- [ ] 2.2 更新 trait 文档注释

## 3. uart_16550 flush 实现

- [ ] 3.1 修改 `embedded_io_async::Write::flush()` 为 poll_fn 轮询 `tx_completion()`
- [ ] 3.2 flush 使用 DRAIN_WAKER + register-recheck-Pending 模式等待四条件

## 4. StarryOS 适配

- [ ] 4.1 `ArceOsUartPort` 实现 `transmitter_empty()`（lock → lsr() → TRANSMITTER_EMPTY）
- [ ] 4.2 `ctl.rs` tcdrain 改用 `driver().tx_completion()` + DRAIN_WAKER（替代直接 lock+lsr 读取）
- [ ] 4.3 删除 `ctl.rs` 中不再需要的 `uart_16550::spec::registers::LSR` 直接引用

## 5. 验证

- [ ] 5.1 `cargo build --features async` on uart_16550
- [ ] 5.2 `cargo test --features async` on uart_16550
- [ ] 5.3 `make build ARCH=riscv64` on StarryOS
- [ ] 5.4 `make run ARCH=riscv64` QEMU 启动验证（shell 交互正常、write+tcdrain 不卡死）
