## Why

`AsyncUartWriter` 当前可 `Clone`，而其 safe `write(&self)` 最终通过 `RingBufTx::push(&self)` 可变访问 `UnsafeCell<Writer>`；`RingBufTx` 的 `unsafe impl Sync` 仅以“单 producer”注释作为前提，类型和 API 均未强制该前提。StarryOS 的 `Tty::new()` 又会为 direct write 与 line-discipline echo 克隆 writer，因此该不一致已存在真实可达路径，必须在 Q27 backpressure 完成后收敛。

## What Changes

- **BREAKING**：将原始 `AsyncUartWriter` 收敛为不可 `Clone` 的唯一 TX producer capability，写入入口要求独占可变访问；其构造明确承担“每个 driver 仅一个 producer”的 unsafe 契约或等价的一次性获取保证。
- **BREAKING**：收紧 `RingBufTx::push()` 的可见性，禁止 crate 外部通过 safe API 绕过 producer capability。
- 在 StarryOS adapter 层引入可 `Clone` 的串行化 writer wrapper，复用 `PtyWriter` 的 `Arc<SpinNoPreempt<...>>` 模式，使 direct write 与 echo 共享同一 producer lock。
- 保持 Q27 的 blocking backpressure、nonblocking partial/`WouldBlock`、ONLCR、poll/epoll 和 readiness hint 语义；producer lock 不得跨 `poll_io`、await 或调度点持有。
- 增加并发 writer 的 RED/GREEN 见证与回归/性能 Gate；单次底层 push 的 accepted prefix 必须完整，不承诺 syscall 级原子性或 producer 公平性。
- 不引入 MPSC ring；相似的 RX consumer 唯一性风险仅记录为后续工作，不在本 change 中重构。

## Capabilities

### New Capabilities

- 无。

### Modified Capabilities

- `async-uart-core`：增加 TX producer 唯一性、共享 writer 串行化、accepted-prefix 完整性和 producer lock 生命周期要求；修订 `AsyncUartWriter`/`TtyWrite` 的既有契约。

## Impact

- `crates/uart_16550/src/async_/device_ops.rs`：`AsyncUartWriter` ownership、构造、`Clone`、`TtyWrite` 与 `embedded_io_async::Write` 接口。
- `crates/uart_16550/src/async_/ring_buffer.rs`：`RingBufTx::push()` 可见性与 SPSC SAFETY 边界。
- `kernel/src/drivers/uart_init.rs`、`kernel/src/drivers/ntty_async.rs`：ArceOS writer wrapper、readiness facade 与唯一 raw writer 构造。
- `kernel/src/pseudofs/dev/tty/`：保持现有通用 TTY/PTY 行为；新增或扩展并发 writer 与 Q27 回归测试。
- API 兼容性：本仓库本地 `uart_16550` async API 发生 breaking change；不修改 registry 外部 crate，不新增 MPSC 数据结构或 StarryOS VFS 依赖。
- 性能：producer push 热路径新增 task-context 串行化；按用户确认，由用户在 QEMU 与 D1 各手动执行一次候选测量并对照同环境 Q27 baseline，超过 3% 的退化阻塞验收。本 Gate 不声明统计显著性。
