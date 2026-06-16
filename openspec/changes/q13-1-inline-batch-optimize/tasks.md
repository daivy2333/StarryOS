## 1. 热路径内联优化（uart_16550）

- [ ] 1.1 `uart_16550/src/async_/driver.rs`: 为 `rx_copier_loop` 和 `tx_copier_loop` 中的热路径代码块添加 `#[inline(always)]`
- [ ] 1.2 `uart_16550/src/async_/ring_buffer.rs`: 为 `RingBufRx::push`、`RingBufRx::pop`、`RingBufTx::push`、`RingBufTx::pop` 添加 `#[inline(always)]`
- [ ] 1.3 `uart_16550/src/os/mod.rs`: 评估是否需要为 trait 方法添加 `#[inline]` 提示（可选）
- [ ] 1.4 `kernel/src/drivers/os_arceos.rs`: 为 `ArceOsUartPort::receive_bytes`、`ArceOsUartPort::send_bytes` 添加 `#[inline(always)]`
- [ ] 1.5 Gate: `cargo check --features async` + `cargo clippy --features async` 通过

## 2. 批量操作优化（uart_16550）

- [ ] 2.1 `uart_16550/src/async_/ring_buffer.rs`: 添加 `RingBufRx::push_batch` 方法，接受 `&[u8]` 返回 `usize`
- [ ] 2.2 `uart_16550/src/async_/ring_buffer.rs`: 添加 `RingBufTx::pop_batch` 方法，接受 `&mut [u8]` 返回 `usize`
- [ ] 2.3 `uart_16550/src/async_/driver.rs`: 修改 `rx_copier_loop` 使用 `push_batch` 替代逐字节 push
- [ ] 2.4 `uart_16550/src/async_/driver.rs`: 修改 `tx_copier_loop` 使用 `pop_batch` 替代逐字节 pop
- [ ] 2.5 Gate: `cargo check --features async` + `cargo clippy --features async` 通过

## 3. StarryOS 集成

- [ ] 3.1 `kernel/src/drivers/uart_init.rs`: 验证 `ArceOsUartPort` 方法已添加 `#[inline(always)]`
- [ ] 3.2 Gate: `cargo check` + `cargo clippy` 通过
- [ ] 3.3 Gate: QEMU `make run` 启动正常

## 4. 性能验证

- [ ] 4.1 Gate: 运行 benchmark，对比 Q12 基线（1B avg ≤ 130µs）
- [ ] 4.2 Gate: 运行 benchmark，对比 Q13 优化前（1B avg 140.1µs）
- [ ] 4.3 Gate: FIONBIO 测试通过
- [ ] 4.4 Gate: Shell 交互正常

## 5. 文档更新

- [ ] 5.1 更新 `.claude/docs/SNAPSHOT.md` 记录 Q13.1 优化结果
- [ ] 5.2 更新 `.claude/docs/tasks.md` 记录 Q13.1 完成状态
- [ ] 5.3 更新 `openspec/specs/optimization/spec.md` 记录优化详情
- [ ] 5.4 Gate: 提交文档更新
