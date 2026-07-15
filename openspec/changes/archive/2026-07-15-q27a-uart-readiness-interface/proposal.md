## Why

Q27 需要让 StarryOS 的阻塞 fd 在 TX ring 满时等待 writable，而不是只能 short write；但 OS 层不能直接窥探 `uart_16550` 的内部 ring 状态，也不能把 VFS / poll / syscall 语义塞进 crate。Q27a 作为前置 change，只在 `uart_16550` crate 暴露最小 readiness hint 与 waker 注册接口，让后续 Q27 可以按 check -> register -> recheck 协议接入 `Tty::poll()` / `Tty::register()`。

当前依据：

- R19：`.claude/analysis/uart-backpressure-mpsc-plan.md`
- ADR-061：UART backpressure 与 writer 并发边界分阶段处理
- O83：uart readiness 薄接口 + TX backpressure / writable wait MVP
- tasks Q27a.1-Q27a.4

## What Changes

- `RingBufTx` 增加 `vacant_len()` / `has_space()`，只表达 TX ring 当前可接收空间。
- `RingBufRx` 增加 `occupied_len()` / `has_data()`，只表达 RX ring 当前可读取数据。
- `AsyncUartWriter` 增加 `can_write()` / `register_writable_waker()`。
- `AsyncUartReader` 增加 `can_read()` / `register_readable_waker()`。
- 文档明确 readiness 是 hint，不保证后续 push/pop 必然成功；OS 层必须 register 后 recheck。

## Non-Goals

- 不实现 Q27 的 StarryOS TTY backpressure，不修改 `Tty::poll()` / `Tty::register()` / `Tty::write_at()`。
- 不改变 `AsyncUartWriter::write()` 或 `embedded_io_async::Write::write()` 的 short-write 语义。
- 不处理 Q28 的 `AsyncUartWriter::Clone` 与 `RingBufTx` SPSC 契约收敛。
- 不引入 MPSC ring、completion queue、user ring、`mmap` zero-copy。
- 不让 `uart_16550` crate 依赖 `axpoll`、VFS、syscall 或 StarryOS fd 状态。

## Capabilities

### Modified Capabilities

- `async-uart-core`: 增加 RX/TX ring readiness hint 与 reader/writer waker registration 契约。

## Impact

- `crates/uart_16550/src/async_/ring_buffer.rs`：新增 ring 状态观测方法；复用既有 `poll: W` waker set。
- `crates/uart_16550/src/async_/device_ops.rs`：新增 reader/writer readiness facade；不改变现有 trait impl 行为。
- 测试：优先补 `uart_16550` crate 级 unit tests；如当前 crate test 被既有 dev-dependency 阻塞，至少完成 `cargo check --manifest-path crates/uart_16550/Cargo.toml --features async` 并记录阻塞。
- StarryOS 行为：`/dev/console` 当前路径应保持不变；Q27 才接入 OS 层 poll/backpressure。

## Workflow Phase 1 BDD Gap Scan

> 2026-07-15：用户选择“用默认假设补充”。默认假设来自 R19/ADR-061/O83：Q27a 只做 crate 层 readiness hint + waker 注册，不做 StarryOS TTY backpressure，不处理 Q28 writer Clone 契约。

### Happy Path

- RX ring 已有数据时，`AsyncUartReader::can_read()` 返回 true，OS 层可将其映射为 readable。
- TX ring 有空位时，`AsyncUartWriter::can_write()` 返回 true，OS 层可将其映射为 writable。
- RX/TX 不 ready 时，OS 层可注册 waker；之后 copier 或 producer/consumer 推进 ring 状态时，既有 `poll.wake()` 唤醒 waiter。
- register 后状态已经 ready 的竞态由 OS 层 recheck 关闭，crate 文档明确 hint 语义。

### Sad Path

- 如果接口返回的是 completion/drain 语义，而不是 readiness 语义，视为设计错误。
- 如果 crate API 暴露 StarryOS `IoEvents`、VFS、fd nonblocking 或 syscall 错误码，视为越层。
- 如果新增方法要求持有 OS 层锁或 await，视为不合格；readiness 查询必须是有限、非阻塞操作。

### Edge

- `occupied_len()` / `vacant_len()` 是瞬时快照；允许 spurious wakeup，不允许把 hint 当作后续 push/pop 成功保证。
- 空 buffer read/write 行为保持现状，Q27a 不借机修改。
- 单槽或多槽 waker 行为由 `OsWakerSet` 实现决定；Q27a 只复用现有 abstraction，不更改 `OsWakerSet` trait。
- 当前 `RingBufTx::pop()` / `pop_batch()` 已在释放空间后 wake，Q27a 应复用这个事实，不新增并行 wake 通道。
