## Why

`RingBufRx` 依赖唯一 producer/consumer 的 SPSC unsafe 前提，但当前 safe `AsyncUartReader::new(Arc<AsyncUartDriver>)`、公开 `RingBufRx::pop()` 以及公开 RX producer 操作允许 crate 外安全代码创建第二个 consumer 或绕过角色边界。StarryOS 当前单 `tty-reader` 路径实际串行，但该事实没有由 `uart_16550` 的类型/API 契约证明，因此必须在等待 multi-hart 真板前先关闭 safe API soundness 缺口。

## What Changes

- **BREAKING**：将 raw `AsyncUartReader` 构造收敛为显式 unsafe 唯一 consumer 契约；每个 driver/RX ring 最多存在一个 raw reader capability。
- **BREAKING**：收窄 `RingBufRx` 的消费操作，使 crate 外安全代码不能绕过 `AsyncUartReader` 直接 pop。
- 收窄 RX producer 写入操作，使 crate 外安全代码不能与唯一 RX copier 竞争；readiness 快照和 waker 注册保持非消费、可共享。
- 在 StarryOS `ASYNC_TTY` 唯一构造点记录 safety witness；保留当前 `ProcessMode::External` 单 `tty-reader` 与共享 fd 通过 ldisc ring 串行读取的架构。
- 增加 compile-fail/API、RX 完整性和 readiness register-recheck witness；不引入 MPMC ring、新 reader lock 或多 consumer 语义。
- Q24 继续负责真实 multi-hart read/write/IER 验证；Q30 继续负责 TX 多 producer 调度语义，均不并入本 change。

## Capabilities

### New Capabilities

<!-- 无。Q29 收敛现有 async-uart-core 的 RX capability。 -->

### Modified Capabilities

- `async-uart-core`: 增加 RX raw consumer 唯一性、RX ring producer/consumer 不可绕过、StarryOS 单 consumer witness 与 readiness 保持要求。

## Impact

- `crates/uart_16550/src/async_/device_ops.rs`：`AsyncUartReader` 构造契约与 API 文档。
- `crates/uart_16550/src/async_/ring_buffer.rs`：RX producer/consumer 操作可见性与相关测试。
- `kernel/src/drivers/ntty_async.rs`：唯一 reader 构造点的 unsafe safety witness。
- `crates/uart_16550` 的 crate 外调用方：raw reader 构造成为 breaking API；直接 RX push/pop 不再是公开操作。
- 不新增依赖，不改变 ring 算法、UART ISR/copier 数据路径、TTY/ldisc 用户可见读语义或性能模型。

## BDD Scenario Sketch

### Happy Path

- OS adapter 为一个 driver 显式建立唯一 raw reader，将其移动到唯一 `tty-reader`；RX copier 写入、reader 读取，字节无重复或丢失。
- 多个共享 fd 仍只消费 ldisc 的第二级 ring，不会创建额外 UART RX consumer。
- readiness 查询与 waker 注册可以共享调用，但不消费 RX 数据；等待路径保持 register 后 recheck。

### Sad Path

- crate 外 safe 代码尝试为同一个 driver 构造 raw reader时必须编译失败，除非进入明确记录唯一性责任的 unsafe 边界。
- crate 外 safe 代码尝试直接调用 RX pop 或 producer push 时必须编译失败。
- 若实现仅把 reader 标为不可 `Clone`，但仍保留 safe 多次构造或公开 direct pop，则 Gate 失败。

### Edge

- 空 buffer 读取继续返回 0；空 ring 不制造数据。
- RX 数据跨 ring wrap-around 时仍保持顺序、无重复、无丢失。
- waker 在首次检查与注册之间到达时，注册后的 recheck 必须观察数据或得到后续 wake；允许 spurious wake。
- unsafe 调用方违反唯一 reader 契约不由运行时兜底；契约必须在 Safety 文档和唯一 StarryOS 构造点中可审计。
- 本 change 不承诺 multi-hart 实测正确性、不支持 MPMC reader，也不改变 syscall 级公平性。

## Workflow Decision

用户在 BDD 缺口选择中确认“用默认假设补充”：Q29 同时封闭 RX consumer 与 producer 的 safe public 破坏入口，但不引入 MPMC、共享 raw reader或新锁。
