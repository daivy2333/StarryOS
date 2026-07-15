## Context

`AsyncUartDriver` 公开持有 `RingBufTx`，其 `writer: UnsafeCell<Writer<'static>>` 依赖 SPSC 单 producer 前提。当前 `RingBufTx::push(&self)` 是 safe public API，`AsyncUartWriter` 又同时提供 safe `new()`、`Clone` 和 `TtyWrite::write(&self)`，因此调用者可以在不经过 unsafe 边界的情况下制造多个并发 producer，现有 `unsafe impl Sync` 的证明不成立。

StarryOS 已实际创建两个共享 TX producer 入口：`Tty::new()` 将 writer 克隆为 direct-write writer 和 line-discipline echo writer；`DeviceOps: Send + Sync` 还允许多个任务共享同一 `/dev/console`。Q27 已完成 writable wait/backpressure，本变更必须保持这些语义，并避免把 OS 锁或 fd 语义下沉到通用 `uart_16550` crate。

## Goals / Non-Goals

**Goals:**

- 让 `AsyncUartWriter`、`RingBufTx` 与 SPSC 的单 producer 安全前提在 API 和 unsafe 契约上完全一致。
- 让 StarryOS direct write、line-discipline echo 和共享 fd 写入通过同一个 task-context producer lock 串行进入原始 writer。
- 保持 Q27 blocking/nonblocking、ONLCR、readiness、poll/epoll 和 drain 行为。
- 用可复现的并发见证证明无重入、无重复、无丢失和 accepted-prefix 破坏。
- 保持 TX ring 为 SPSC，并验证串行化对 QEMU/D1 热路径没有超过既定阈值的退化。

**Non-Goals:**

- 不实现 MPSC ring、producer 公平调度或 syscall 级写原子性。
- 不修改 RX 数据路径；`AsyncUartReader` 的唯一 consumer 风险作为独立后续项。
- 不修改 registry 外部 crate，不向 `uart_16550` 引入 StarryOS VFS、`axpoll` 或 `kspin` 依赖。
- 不用单 hart QEMU/D1 证据声明多 hart 正确性；Q24 继续承担真 SMP 复验。

## Decisions

### 1. 原始 writer 成为唯一 producer capability

`uart_16550::AsyncUartWriter` 移除 `Clone` 和直接的 `TtyWrite` 实现。新增或重命名一个同步非阻塞写入口 `try_write(&mut self, buf: &[u8]) -> usize`，`embedded_io_async::Write::write(&mut self)` 复用该入口。

`AsyncUartWriter::new()` 改为 `unsafe` 构造函数，其 Safety 契约要求同一 `AsyncUartDriver` 只能构造一个 writer。StarryOS 仅在 `ASYNC_TTY` 初始化时构造一次，并在紧邻 unsafe 块的 `// SAFETY:` 注释中给出唯一性证据。

`RingBufTx::push()` 收紧为 crate-private；crate 外部只能通过原始 writer capability 提交 TX 数据。这样 safe 外部调用者不能绕过唯一性边界，crate 内的 copier 仍可使用 reader 侧方法。

启动时 ring microbenchmark 是唯一例外：它在 `ASYNC_TTY` 构造 raw writer 之前同步执行，通过带 `# Safety` 契约的 `unsafe bench_tx_push()` 临时承担 producer 唯一性。该入口不得成为 safe API，调用点必须证明 benchmark 期间不存在 raw writer 或其他 producer。

替代方案：只删除 `Clone`。该方案不能阻止重复 `new(driver.clone())` 或直接 `driver.tx.push()`，拒绝。

替代方案：在 driver 中增加 `AtomicBool` 和一次性 `take_writer()`。该方案能把构造保持为 safe，但会增加永久状态、失败分支和初始化 API；Q28 采用更小的 unsafe capability 边界，并用单一构造点证明契约。

### 2. StarryOS 使用 cloneable serialized wrapper

将现有 `ArceOsWriter` type alias 拆成：

- cfg-specific `RawArceOsWriter = AsyncUartWriter<...>`；
- 本地 `ArceOsWriter` newtype，内部持有 `Arc<SpinNoPreempt<RawArceOsWriter>>` 并实现 `Clone`、`TtyWrite`、`TtyWriteReady`。

`Tty::new()` 继续按现有结构克隆 `ArceOsWriter`，但所有 clone 共享同一 `SpinNoPreempt` 和唯一 raw writer。该模式与当前 `PtyWriter(Arc<SpinNoPreempt<Prod<_>>>, ...)` 一致，不改变通用 TTY 泛型。

StarryOS 顶层 `smp` feature 显式传播到 `starry-kernel/smp`，后者启用 `axfeat/smp`，确保 `kspin/smp` 不会在 multi-hart 构建中退化为仅依赖 no-preempt 的单核锁。host SMP integration test 验证原子锁路径；真实 multi-hart 仍由 Q24 复验。

替代方案：在 `RingBufTx` 内使用 `CriticalSectionRawMutex`。该锁会覆盖 buffer copy；StarryOS 当前 critical-section 实现只切换本地 IRQ，既不能证明跨 hart 互斥，也可能扩大关中断区，拒绝。

替代方案：直接实现 MPSC ring。当前没有吞吐或公平性数据证明需要新队列；ADR-061/O85 已将其后置，拒绝。

### 3. 锁只覆盖一次 producer push

`ArceOsWriter::write(&self)` 获取 `SpinNoPreempt`，调用一次 `RawArceOsWriter::try_write(&mut self, buf)`，随后立即释放。readiness 查询和 waker 注册只允许短暂访问 wrapper 内部状态。

锁不得跨 `poll_io`、await、blocking retry 或调度点持有。Q27 的 blocking write 每次重试重新获取锁，因此另一个 producer 可以在两次 push 之间插入；这不构成数据损坏，也不提供 syscall 级原子性。

### 4. 并发正确性以 accepted prefix 为边界

每次 `try_write()` 返回的 `n` 是唯一提交事实：逻辑 TX 字节流必须包含调用 buffer 的 `buf[..n]` 完整前缀，不得在该前缀内部混入其他 producer 字节。多个调用之间的全局顺序由锁获取顺序决定，不承诺公平性。

空 buffer 继续返回 0；`writable_len()` 继续是 hint 而非 reservation；echo 继续允许 best-effort short write。

### 5. 测试和性能采用 Q27 基线

Phase 3 先建立能暴露 producer 重入/accepted-prefix 破坏的 RED 见证，再修改生产代码。验证覆盖 raw writer API、两个 cloned StarryOS wrapper、direct write 与 echo 的共享关系，以及 Q27 既有 backpressure/ONLCR/readiness 场景。

性能 Gate 按用户在 2026-07-15 的明确调整执行：复用同环境 Q27 baseline，由用户在 QEMU 与 D1 各手动运行一次 candidate。QEMU 使用 ring microbenchmark 与 latency 回归，不声明 UART 线速；D1 使用既有 TX throughput 与 p50 场景。任一关键指标超过 3% 时 Gate BLOCK；单次结果只作为本 change 的验收 Gate，不声明统计显著性。

## Risks / Trade-offs

- [producer lock 增加热路径成本] → 只在 task context 获取 `SpinNoPreempt`，锁范围限定为一次 push；按用户确认用 QEMU/D1 各一次手测 Gate 约束退化，并明确单样本边界。
- [大 buffer copy 延长禁止抢占时间] → 不关闭 IRQ，不跨等待持锁；保留 short-write/backpressure 分段。若仍超过性能 Gate，停止并回到设计，不静默引入 MPSC 或新 chunk 策略。
- [unsafe 构造函数被重复调用] → 收紧 `RingBufTx::push()` 可见性，StarryOS 只保留一个带 SAFETY 证明的构造点，并增加 API/构造点审计任务。
- [多 hart 构建未启用真正的 SMP spin lock] → Phase 3 检查目标 feature 传播；Q28 仅声明当前可验证环境正确性，多 hart 实测仍由 Q24 完成。
- [breaking API 影响其他调用者] → CodeGraph impact 和编译 Gate 覆盖本仓库全部调用点；本地 crate 与 kernel 在同一 change 中原子迁移。

## Migration Plan

1. 建立并发 RED 见证和当前 Q27 功能/性能 baseline。
2. 在 `uart_16550` 收紧 raw writer capability、构造契约与 `RingBufTx::push()` 可见性。
3. 在 StarryOS adapter 引入 serialized wrapper，并迁移 `ASYNC_TTY`、readiness 和 TTY trait 实现。
4. 运行 crate、kernel、OpenSpec Gate；由用户手动运行一次 QEMU 与一次 D1 candidate，并与同环境 Q27 baseline 比较。
5. 若任一 correctness 或性能 Gate 失败，回退本 change 的 raw API 与 wrapper 改动；Q27 已归档行为保持为回退基线。

## Open Questions

无。BDD 默认假设和 Scenario Sketch 已由用户确认；RX consumer 唯一性明确后置，不在实施阶段扩展范围。
