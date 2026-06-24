## 1. 核心实现

- [x] 1.1 在 `uart_16550/src/async_/driver.rs` 新增 `const TX_FAST_RETRY_LIMIT: usize = 32`
- [x] 1.2 修改 `tx_copier_loop`：在 `send_bytes() == 0` 且 cursor < pending 时，添加有界 retry 循环（最多 32 次），每次 retry 不跨越 `.await` 点
- [x] 1.3 预算耗尽后：按 D3 顺序（register waker → enable_tx_intr → final recheck → Pending）挂起

## 2. 验证

- [x] 2.1 运行 `cargo build` 确认 uart_16550 编译通过（async feature）
- [x] 2.2 运行 `cargo test` 确认 uart_16550 现有测试通过
- [x] 2.3 检查 telemetry `tx_no_progress` 计数器在 retry 内部语义正确（idle 10 秒不持续增长）
- [x] 2.4 运行 StarryOS `make build` 确认集成编译通过
- [x] 2.5 运行 StarryOS `make run` QEMU 启动验证，确认 shell 交互正常
