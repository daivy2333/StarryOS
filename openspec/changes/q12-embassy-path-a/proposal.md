## Why

基于 2026-06-11 embassy UART 架构深度调研（`.claude/analysis/embassy-uart-evaluation.md`），embassy 的 `BufferedUart` 与 StarryOS 自研异步串口栈在宏观架构上同构，但 embassy 在 ring buffer 实现（lock-free SPSC）、tcdrain 硬件优化（TC interrupt）和标准化接口（`embedded_io_async` trait）三个层面提供了可低成本借鉴的组件。本次变更实施路径 A（最小借鉴），不改 ISR 逻辑、不引入 embassy-executor，立即可获得可量化收益。

## What Changes

- **O51**: 用 `embassy_hal_internal::atomic_ring_buffer::RingBuffer`（lock-free SPSC，~768 行纯 Rust）替换 `kernel/src/drivers/ring_buffer.rs` 的 `HeapRb<u8>` + `axsync::Mutex`，消除 ring buffer 操作中的 mutex 开销
- **O52**: 为 `AsyncUartReader`/`AsyncUartWriter` 新增 `embedded_io_async::Read`/`Write`/`BufRead` trait 实现，不改动核心数据路径
- **O53**: 用 NS16550 `LSR::TRANSMITTER_EMPTY`（bit 6）+ TX ISR 直接唤醒替代 `TCDRAIN_ACTIVE: AtomicBool` 软件状态标志

## Capabilities

### New Capabilities
- `atomic-ring-buffer`: lock-free SPSC ring buffer 替换现有 `HeapRb + Mutex` 实现，消除 UART 数据路径中的 mutex 争用
- `embedded-io-async-traits`: 为 UART reader/writer 实现社区标准异步 I/O trait，标准化接口层
- `hardware-tcdrain`: 用硬件 TC (Transmission Complete) 中断替代软件 `TCDRAIN_ACTIVE` 标志

### Modified Capabilities
- `optimization`: O51/O52/O53 新增为已完成优化条目（参见 `openspec/specs/optimization/spec.md` 已记录的 Q12 条目）

## Impact

- **Affected code**: `kernel/src/drivers/ring_buffer.rs`（重写）, `kernel/src/drivers/device_ops.rs`（新增 trait impl）, `kernel/src/drivers/isr.rs`（删除 TCDRAIN_ACTIVE）, `kernel/src/syscall/fs/ctl.rs`（tcdrain 逻辑简化）
- **New dependency**: `embassy-hal-internal`（仅 `atomic_ring_buffer` 模块），`embedded-io-async`（标准 trait）
- **No breaking changes**: 现有 `TtyRead`/`TtyWrite` trait 保持不动，`RingBufRx`/`RingBufTx` 公共 API 签名不变
- **Performance**: 预期消除每次 push/pop ~100ns mutex 开销，tcdrain 删除 `load-acquire` 软件状态检查
