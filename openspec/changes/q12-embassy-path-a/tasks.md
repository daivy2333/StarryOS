## 1. O52 — embedded_io_async trait 实现（零风险，先做）

- [ ] 1.1 在 `kernel/Cargo.toml` 添加 `embedded-io-async` 依赖
- [ ] 1.2 在 `device_ops.rs` 为 `AsyncUartReader` 新增 `impl embedded_io_async::Read`（方法：`read(&mut self, buf: &mut [u8]) -> Result<usize, Error>`）
- [ ] 1.3 在 `device_ops.rs` 为 `AsyncUartWriter` 新增 `impl embedded_io_async::Write`（方法：`write(&mut self, buf: &[u8]) -> Result<usize, Error>`, `flush(&mut self) -> Result<(), Error>`）
- [ ] 1.4 `cargo check` 0 错误验证

## 2. O53 — TC 硬件寄存器 tcdrain 优化

- [ ] 2.1 在 `isr.rs` TX handler 中追加 `LSR::TRANSMITTER_EMPTY` 检查：若 TEMT 置位则 `DRAIN_WAKER.wake()`
- [ ] 2.2 删除 `isr.rs` 中的 `TCDRAIN_ACTIVE: AtomicBool` 声明
- [ ] 2.3 删除 `isr.rs` TX handler 中对 `TCDRAIN_ACTIVE.load(Acquire)` 的条件检查，改为无条件检查 TEMT
- [ ] 2.4 在 `ctl.rs` tcdrain 路径中删除 `TCDRAIN_ACTIVE.store(true/false, Release)` 代码
- [ ] 2.5 `cargo check` + `cargo clippy` 0 错误验证

## 3. O51 — atomic_ring_buffer 替换 HeapRb + Mutex

- [ ] 3.1 在 `kernel/Cargo.toml` 添加 `embassy-hal-internal` 依赖（仅 `atomic_ring_buffer` 模块）
- [ ] 3.2 在 `ring_buffer.rs` 中替换：`HeapRb<u8>` → `RingBuffer`，移除 `axsync::Mutex` 包装（`async_driver.rs` 中 `self.rx.lock()` / `self.tx.lock()` → 直接访问）
- [ ] 3.3 `RingBufRx::new()` / `RingBufTx::new()` 改为使用 `static` 缓冲区 + `RingBuffer::init()`
- [ ] 3.4 重写 `push()`/`pop()`/`is_empty()`/`register_waker()` 适配 `RingBuffer` API（reader/writer iterator 模式）
- [ ] 3.5 更新 `async_driver.rs` 中所有 `rx.lock()` / `tx.lock()` 调用点（移除 Mutex 包装）
- [ ] 3.6 更新 `ntty_async.rs` 中 `DRIVER.rx.lock().poll.register(&waker)` → 直接访问
- [ ] 3.7 更新 `ctl.rs` tcdrain 中 `DRIVER.tx.lock().is_empty()` → 直接访问
- [ ] 3.8 新增 `#[cfg(test)]` 单元测试：满/空边界、并发 push/pop、内存序
- [ ] 3.9 `cargo check` + `cargo clippy` 0 错误

## 4. 集成验证

- [ ] 4.1 QEMU `make run` 内核启动正常，Shell 交互正常
- [ ] 4.2 benchmark 性能不低于 Q11 基线（1B avg latency ≤ 118µs）
- [ ] 4.3 `cargo clippy` 0 新增 warning
