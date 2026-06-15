# Spec Delta: optimization

> 本 delta 记录 Q12 Embassy 路径 A 三项优化（O51/O52/O53）的落地完成。
> 原主 spec 中"Q12 Embassy 调研驱动的近期优化 — 待实现（路径 A）"条目于 2026-06-11 提交时**未**同步更新，本 delta 在归档时通过 sync 统一回流到主 spec。

## MODIFIED Requirements

### Requirement: Q12 Embassy 调研驱动的近期优化 — 待实现（路径 A）

本条目原在主 spec 中以"待实现"姿态存在（spec.md:165）。由 2026-06-11 提交 `e7d93f8` / `04483fe` 实现完毕后，本条目标记为**已完成**，MUST 不再视为"待实现"，相关性能收益 MUST 记录在归档摘要中。

#### Scenario: O51 atomic_ring_buffer 替换 HeapRb + Mutex 落地

- **WHEN** StarryOS 启动并加载 `kernel/src/drivers/ring_buffer.rs`
- **THEN** 缓冲区 MUST 使用 `embassy_hal_internal::atomic_ring_buffer::RingBuffer`（lock-free SPSC），而**禁止**使用 `HeapRb<u8>` + `axsync::Mutex` 组合
- **AND** `RX_RING` / `TX_RING` MUST 为 `static RingBuffer` 实例（`ring_buffer.rs:12,13`）
- **AND** `Cargo.toml` MUST 包含 `embassy-hal-internal = { version = "0.2", default-features = false }`
- **AND** `async_driver.rs` 中 MUST 移除 `self.rx.lock()` / `self.tx.lock()` 调用

#### Scenario: O52 embedded_io_async trait 落地

- **WHEN** 第三方 Rust 嵌入式库调用 `AsyncUartReader` / `AsyncUartWriter`
- **THEN** `AsyncUartReader` MUST 实现 `embedded_io_async::Read`（`device_ops.rs:32`）
- **AND** `AsyncUartWriter` MUST 实现 `embedded_io_async::Write`（`device_ops.rs:42`）
- **AND** 两个类型 MUST 各自实现 `embedded_io_async::ErrorType`（`device_ops.rs:28,38`）
- **AND** `Cargo.toml` MUST 包含 `embedded-io-async = "0.6.1"`

#### Scenario: O53 硬件 TC tcdrain 落地

- **WHEN** 用户态调用 `tcdrain()` 并等待 TX 真正完成
- **THEN** ISR MUST 在 TX 中断中检查 `LSR::TRANSMITTER_EMPTY`（bit 6）并 `DRAIN_WAKER.wake()`（`isr.rs:19`）
- **AND** `tcdrain` 实现 MUST 使用 `LSR::TRANSMITTER_EMPTY` 轮询（`ctl.rs:53,60`）
- **AND** `TCDRAIN_ACTIVE: AtomicBool` MUST 已被删除（ISR 与 ctl.rs 中均无此符号）

#### Scenario: Q12 性能基线记录

- **WHEN** 对比 Q11 与 Q12 benchmark 结果（`docs/benchmark-report-async.md`）
- **THEN** software overhead MUST 从 53.9 µs 降至 ≤ 40 µs（实测 37.1 µs，**↓31%**）
- **AND** 256B TX 延迟 MUST 从 1332 µs 降至 ≤ 1300 µs（实测 1252 µs，**↓6%**）
- **AND** 1024B TX 延迟 MUST 从 5170 µs 降至 ≤ 5000 µs（实测 4880 µs，**↓5.6%**）
- **AND** `cargo check` MUST 0 错误 / `cargo clippy` MUST 0 新增 warning / QEMU `make run` MUST 启动正常 / Shell MUST 交互正常
