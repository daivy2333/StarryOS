## Context

`RingBufRx` 将 `embassy_hal_internal::atomic_ring_buffer` 的一个 `Writer` 和一个 `Reader` 保存在 `UnsafeCell` 中，并以 unsafe `Sync` 实现允许 driver 跨任务共享。该实现只有在唯一 RX copier 调用 producer、唯一 raw reader 调用 consumer 时才成立。

当前 StarryOS 正常路径已经满足该拓扑：`ASYNC_TTY` 构造一个 `AsyncUartReader`，`ProcessMode::External` 将其移动到唯一 `tty-reader` 任务；用户共享 fd 只在 `ldisc` mutex 下消费第二级 ring。然而 `AsyncUartReader::new` 是 safe 且接收可 clone 的 `Arc<AsyncUartDriver>`，`AsyncUartDriver::rx` 又暴露 safe RX push/pop，因此 crate 外 safe 代码能够破坏 SPSC 前提。

Q29 是类型/API soundness 收敛，不依赖 Q24 multi-hart 证据。Q24 仍负责验证锁、原子序和任务/ISR 在真实 SMP 上的运行时表现。

## Goals / Non-Goals

**Goals:**

- 让 crate 外 safe API 无法创建第二个 raw RX consumer。
- 让 crate 外 safe API 无法绕过 reader/copyer 直接执行 RX pop/push。
- 用 StarryOS 唯一构造点和数据流证明当前单 consumer 拓扑。
- 保持 reader readiness、waker register-recheck、TTY/ldisc 读语义和 RX hot path 成本。
- 用 compile-fail、单元/并发 witness 和 QEMU 功能回归覆盖契约。

**Non-Goals:**

- 不引入 MPMC ring、cloneable raw reader、reader mutex 或 consumer 调度器。
- 不改变 TX producer、syscall 原子性、公平性或 Q30 范围。
- 不以 QEMU/D1 单 hart结果替代 Q24 multi-hart Gate。
- 不重构 TTY/ldisc、poll framework 或 RX copier 状态机。

## Decisions

### 1. Raw reader 使用显式 unsafe 唯一构造契约

`AsyncUartReader::new(Arc<Driver>)` 改为 unsafe constructor，并在 `# Safety` 中要求：同一个 driver/RX ring 同时最多存在一个 raw reader，且不得通过其他入口直接消费该 ring。reader 保持不可 `Clone`，`TtyRead` 和 `embedded_io_async::Read` 继续通过 `&mut self` 消费。

这与 Q28 raw writer 的契约对称，改动小且不增加运行时成本。unsafe constructor 不能物理阻止调用方故意重复构造，但会把当前由 safe API 隐藏的责任移动到可审计边界。

**替代方案：** driver 提供运行时 one-shot `take_reader()`。它可以保持 safe API，但需要新增 acquisition 状态、失败类型和初始化时序，且仍需封闭 direct pop；当前收益不足。

**替代方案：** 在 reader 或 ring consumer 上加共享锁。它会把 SPSC 转换为隐式多 consumer，并给 RX hot path增加锁成本，同时掩盖而非表达唯一能力；拒绝。

### 2. 封闭 RX ring 的角色变更操作

`RingBufRx::pop` 仅供 `AsyncUartReader` 使用，RX producer push 操作仅供 driver copier 使用，因此这些改变 ring 状态的方法收窄为 crate-private。非消费 readiness 快照、waker 注册及 OS 当前需要的通知表面保持可用。

该边界确保 crate 外 safe 代码即使持有 `Arc<AsyncUartDriver>`，也只能观察/注册 RX readiness，不能取得第二 producer/consumer 角色。

**替代方案：** 将整个 `AsyncUartDriver::rx` 字段私有化并增加完整 facade。封装更强，但会扩大 Q27a readiness 和 StarryOS waker glue 的迁移范围；Q29 先收窄变更状态的方法，避免无关 API 重构。

### 3. StarryOS 保持唯一 `tty-reader`，不增加 reader adapter

`ASYNC_TTY` 的唯一 lazy initialization 是 raw reader 的唯一构造点。该点增加 `SAFETY` 注释，说明 driver 初始化、benchmark/TTY 顺序以及无第二 raw reader。reader 随后被移动进 `ProcessMode::External` 创建的唯一 `tty-reader`。

共享 fd 不需要 UART reader lock：它们消费的是 ldisc ring，`Tty::read_at` 和 poll 路径通过同一个 ldisc mutex 串行。只有出现真实的 raw 多 consumer 需求时，才单独规划 OS adapter 或 MPMC。

### 4. Readiness 保持观察者语义

`can_read`、occupied length 和 readable waker registration 不取得 consumer capability，也不消费数据。外部 reader task 继续执行 drain/check -> register -> recheck，允许 spurious wake，但不允许 lost wakeup 导致永久休眠。

Q29 不用 readiness 结果作为数据 reservation；真正提交的读取量仍以 raw reader read/pop 返回值为准。

### 5. 验证以 API witness 为主，运行时回归为辅

- compile-fail 证明 safe 构造、Clone 和 direct RX mutation 不可用。
- ring/reader 测试覆盖空读、顺序、wrap-around、无重复/丢失与 waker 注册。
- StarryOS 静态 witness 证明唯一构造点和单 `tty-reader` 所有权转移。
- crate fmt/check/test/clippy/rustdoc 和 kernel build/QEMU Shell 输入验证行为保持。
- 不设置 D1 性能阈值：设计不在 RX hot path 增加指令，且 Q29 不是性能 change；真板 SMP 仍进入 Q24。

## Risks / Trade-offs

- **[Breaking API 影响 crate 外调用方]** -> 在编译错误处迁移到单一 unsafe 构造点，并要求相邻 `SAFETY` witness。
- **[unsafe constructor 仍可被错误重复调用]** -> 封闭 direct pop，提供明确 Safety 文档和 compile-fail witness；当前不为 misuse 增加运行时状态。
- **[收窄 push/pop 影响已有外部测试或 adapter]** -> 先以 workspace build、doctest 和 CodeGraph impact 验证调用面；合法 OS 路径改走 reader/readiness facade。
- **[readiness 改动引入 lost wakeup]** -> 不改变算法，只增加/保持 register-recheck 测试和 QEMU 输入回归。
- **[把 Q29 误解为 SMP 证明]** -> proposal/spec 明确 Q24 是唯一 multi-hart runtime Gate。

## Migration Plan

1. 先添加 compile-fail/API RED witness，证明当前 safe 多构造和 direct mutation 可通过编译。
2. 收窄 RX ring 角色操作，并将 raw reader constructor 改为 unsafe。
3. 迁移 StarryOS 唯一构造点并补充 safety witness。
4. 运行 crate、kernel、OpenSpec 和 QEMU 回归；不满足任一契约时回滚本 change 的 API 收敛提交。

## Open Questions

Gate 1 无未决实现问题。若 impact 检查发现 crate 外合法 RX producer/consumer 调用方，必须回到 Gate 1 重新确认迁移策略，不得在实施阶段临时放宽 safe API。
