# Iteration 000 / Cycle 001: Resident Runner Acceptance Closure

## Plan Context

- Status: ready
- Approval: 用户于 2026-08-23 批准本 rework Cycle 并指示执行，原话："请你开始实施这一轮cyc吧"。Gate 2 User Approval 由 BLOCKED 转为 PASS；Act 据此开始。
- Iteration: 000-resident-stack-runner
- Cycle: 001-rework
- Cycle Type: rework
- Parent cycle: `000-initial.md`

**Iteration Scope**

- Change tasks: 1.1–1.5
- Depends on: MS05 accepted baseline
- Stable baseline: 唯一 runner 可由 device/software/timer 唤醒，stack round 有界且
  Polling fallback/Active quiet 可判定；legacy socket inline path 暂时保留。
- Verification boundary: lifecycle、三源 register-recheck、31/32/33 budgets、完整
  stage 顺序、timer replacement/stale wake、fallback、guard 释放、init 顺序及受支持
  QEMU/D1 编译组合全部通过。
- Diagnostic boundary: 失败限制在 StackEvent/runner future、Router/Service bounded
  round、shared critical-section policy 的 feature 可达性或平台编译组合。
- Deferred tasks: 2.1–3.4

**Cycle Scope**

- Trigger: rework-required
- Acceptance gaps: Acceptance 2 的 future-level generation/timer witness；Acceptance 3
  的完整 stage/fault witness；Acceptance 7 的 root D1 compile。
- Repair items: T1.3-R1、T1.4-R1、T1.5-R1
- Inherited scope: proposal R1–R4、R7 的 T09 场景，design D1–D5/D9–D10，Task
  1.1–1.5 的保持/禁止边界，MS05 queue owner/slot/ticket/flush 契约。
- Excluded scope: Tasks 2.1–3.4、socket readiness cutover、reset、SMP、真板 runtime、
  性能、D1 UART 行为调整和全局文档维护。

**Objective**

保留 Cycle 000 的 resident runner 实现，关闭三个既有验收缺口：完整 stack round 在
前 stage 达预算后仍执行后 stage并传播 fault；完整 runner future 在 generation 交错
和 timer replacement/stale deadline 下无 lost wake；root D1 smoke 与 QEMU 均能访问
同一 critical-section restore policy 并编译通过。

**Background**

Cycle 000 完成 Tasks 1.1–1.4 与 Task 1.5 的代码接入，但 D1 Gate 阻塞。Plan Review
进一步发现，已报告的 100× runner tests 只重复直接 `StackEvent`/deadline 纯函数用例，
没有穿过完整 future；Service tests 也没有直接覆盖前 stage budget-hit 后的后续 stage
和 round fault outcome。这些都是原 Task Contract 和 Acceptance 的组成部分，不改变
MS06 需求或 Iteration 划分。

**Current Baseline**

- Revision: `b8e7bcae27579aa7ea7bf31698e3136f5856302d`，branch `net-k3`；MS06 实现位于
  未提交工作树。
- Tasks 1.1–1.4 已勾选；Task 1.5 保持未完成。Cycle 000 Review 为
  `rework-required`。
- Fresh Review：ordinary axnet 239/239、qemu-diagnostics 259/259、MS04 host harness
  16/16、QEMU kernel check、strict OpenSpec、diff check 均通过。
- 现有 `stack_runner::tests::` ordinary/qemu-diagnostics 各重复 100 次均通过，但不含
  future 内 generation race 或实际 timer replacement/stale deadline witness。
- root D1 RISC-V target check 稳定报 `E0432/E0433`：`critical_impl` 无条件导入
  `crate::drivers::critical_section_policy`，而 `lichee-d1-smoke` 排除了 `drivers`。
- Review 期间出现的 SNAPSHOT、regression Runbook、knowledge、references 与
  improvements 修改属于并发用户内容，不在本 Cycle 范围。

**Current-State Evidence**

- `Cargo.toml` 的 root `lichee-d1` 同时启用 `starry-kernel/lichee-d1` 和
  `starry-kernel/lichee-d1-smoke`；`kernel/src/lib.rs` 对 `drivers` 使用
  `#[cfg(not(feature = "lichee-d1-smoke"))]`。
- `kernel/src/lib.rs::critical_impl` 在所有上述 mode 编译，并通过
  `drivers::critical_section_policy::{acquire,release,IrqOps}` 实现官方
  `critical_section::Impl`。
- policy 文件无 `axhal`、heap、atomics 或 std 依赖；生产和
  `tests/ms04-async-rx-host-harness.rs` 引用同一文件。全仓引用只涉及该 kernel glue、
  `drivers/mod.rs` 和 host harness，适合提升为 crate-root shared policy。
- `Service::stack_round` 无条件按 Router RX → maintenance → listener reconcile →
  ingress → egress → listener reconcile → dispatch 执行；当前 tests 只分别覆盖
  `run_bounded_stage`、RX-space wake 和 TX enqueue。
- `StackRunnerFuture::poll` 执行 timer poll → generation snapshot → waker register →
  bounded round → self-yield/event recheck → deadline arm。当前 direct event tests 不经过
  这条路径；test-mode `arm_timer/poll_timer` 已使用确定性 clock，可直接验证 deadline
  replacement 和 stale expiry。
- `BurstDevice::recv` 位于完整 future round 内，可在 test device 中发布
  `StackEvent`，无需增加生产 hook 即可制造 register 后、recheck 前的确定性交错。

**Relevant Code**

| File / Symbol | Current Responsibility | Cycle Use |
|---|---|---|
| `kernel/src/lib.rs::{drivers,critical_impl}` | feature-gated drivers 与全局 critical-section glue | 让 shared policy 在 smoke/full mode 均可达 |
| `kernel/src/drivers/critical_section_policy.rs` | 无依赖 IRQ restore policy | 提升为 crate-root shared module，语义不变 |
| `tests/ms04-async-rx-host-harness.rs` | 同源 policy 与 production source guard | 跟随路径并保持 16 个 witness |
| `crates/axnet/src/service.rs::stack_round` | 固定顺序有界推进 | 增加完整 stage/fault 行为 witness |
| `crates/axnet/src/stack_runner.rs::StackRunnerFuture` | generation、timer、fallback、guard 生命周期 | 增加 future-level interleaving/timer witness |

**Critical Path**

```text
root lichee-d1
  -> starry-kernel/lichee-d1-smoke
  -> drivers module excluded
  -> critical_impl still compiled
  -> shared critical_section_policy must remain reachable

StackEvent publish during injected device recv
  -> StackRunnerFuture round returns with guards released
  -> generation recheck detects change
  -> event_retry + self-wake -> Pending

Router RX reaches budget
  -> maintenance/listener/ingress/egress still execute
  -> dispatch observable in same round
  -> structured outcome carries budget/fault state
```

**Implementation Guidance**

先用现有失败命令保留 D1 RED，再把 policy 从 driver ownership 提升为无条件 crate-root
shared module；生产 glue 与 host harness 必须继续引用同一源文件。随后只补原计划缺失的
行为 witnesses：用真实 `Service::stack_round` 构造 RX budget-hit + 后续 dispatch，用
faulting device 断言 round fault；用在 `recv` 内发布 event 的 test device 穿过完整
future，并直接驱动 test timer 的 replace/stale/expiry 状态。测试若暴露产品缺陷，只能
在原 Task 1.3/1.4 契约内修正；不得借机切换 readiness 或更改 owner/slot 语义。

**Behavioral Change**

- D1 smoke、D1 非 smoke 与 QEMU kernel 都编译同一 crate-root IRQ restore policy；
  acquire/release、nested/ISR 语义不变。
- resident runner 与 stack round 的产品契约不新增行为；本 Cycle 补足其原定测试见证，
  仅在 RED 暴露现有实现违反契约时作最小修正。
- 外部 socket API、queue lifecycle、descriptor/packet ownership 和 telemetry ABI 不变。

**Change Surface**

| Repair | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| T1.3-R1 | R3 / stage fairness、fault | `service.rs::tests`、必要时 `stack_round` | helper-level budget tests | 完整 round stage/fault witness |
| T1.4-R1 | R2 / register race、timer | `stack_runner.rs::tests`、必要时 future | direct event/deadline tests | future-level race与真实 timer state witness |
| T1.5-R1 | R7 / supported D1 compile | kernel policy/lib、MS04 harness | policy 隶属被 smoke 排除的 drivers | 提升 shared policy 并复验平台组合 |

**Task Contracts**

### T1.3-R1: 闭合完整 stack round 的 stage 与 fault 见证

- Requirement/Scenario: R3、Task 1.3、Acceptance 3。
- Depends on: Cycle 000 Tasks 1.2–1.3 的产品实现。
- Targets: `crates/axnet/src/service.rs::tests`，必要时原范围内的
  `Service::stack_round`/test-only observation。
- Current behavior: 单 helper 31/32/33、RX-space 和 TX-enqueue tests 通过；没有一次
  完整 round 同时证明前 stage budget-hit 后的后 stage执行，也没有 round fault 断言。
- Required behavior: Router RX 处理 32/33 backlog 时，同轮后续 dispatch 仍产生可观察
  结果；RX 或 dispatch fault 使 `StackRoundOutcome::faulted = true`，不隐藏为 idle。
- Required changes: 先增加使用实际 `Service::stack_round` 的失败见证；优先复用 fake
  device、预装 Router TX 和现有 test helpers。只有 RED 暴露实现错误时才修改原函数。
- Preserve: stage 顺序、budget=32、listener reconcile、RX-space/first-TX wake、typed
  Full/drop/fault、ticket/flush 和 owner 语义。
- Forbidden: 仅测试新的复制 helper、drain-to-empty、动态扩容、改变 socket/listener
  契约或用 telemetry 参与同步。
- Test witness: 新 test 在缺少完整观察或故意跳过后 stage/fault propagation 的旧实现上
  RED；当前 helper tests 作为变更前 GREEN 保留。
- GREEN condition: 完整 round 的 budget-hit→later-stage 与 RX/TX fault assertions
  通过，所有既有 service/router/device/flush tests 继续通过。
- Verification: ordinary 与 qemu-diagnostics axnet lib suites；新 tests 单独运行并记录
  名称、exit 与关键 assertions。
- Stop when: 见证需要改变 smoltcp、listener/backlog、slot/descriptor ownership 或新增
  不属于 Task 1.3 的状态语义。

### T1.4-R1: 闭合 future-level generation 与 timer 见证

- Requirement/Scenario: R2、Task 1.4、Acceptance 2。
- Depends on: Cycle 000 Tasks 1.1、1.3–1.4 与 T1.3-R1 GREEN。
- Targets: `crates/axnet/src/stack_runner.rs::tests`，必要时同文件 test-only device/seam。
- Current behavior: direct `StackEvent` 和 deadline-selection tests 通过；完整 future 未观察
  event-retry，test timer 未验证 earlier→later replacement、stale deadline 与一次 expiry。
- Required behavior: test device 在完整 future 的 register 后、round 内发布 event；
  future 释放 guard 后观察 generation 变化、增加 `event_retry` 并 self-wake。test timer
  替换 deadline 后，旧 deadline 到达不触发，当前 deadline 到达只计一次并可重新 arm。
- Required changes: 使用 injected Service/device 与现有 fake clock；直接驱动私有
  `arm_timer/poll_timer` 状态。不得增加 production global race hook。
- Preserve: Release/Acquire generation、单 AtomicWaker、lifecycle matrix、Active quiet、
  Polling/Spawned/Unavailable 10ms fallback、Faulted no-fallback、guard-before-wake/Pending。
- Forbidden: 第二 stack waker/executor、blocking sleep、固定 Active tick、tests 推进生产
  global 或只重跑 direct `StackEvent` 纯函数冒充 future witness。
- Test witness: 新 future/timer cases 对缺少相应 retry/replacement/stale 处理的实现 RED；
  当前 direct tests 保持 GREEN。
- GREEN condition: future event interleaving targeted test 连续 100 次通过且每次 retry/wake
  计数一致；timer replacement/stale/expiry assertions 稳定通过。
- Verification: ordinary 与 qemu-diagnostics 下单独运行新 cases，各自 100×；随后两组
  axnet lib suites全量通过。
- Stop when: 需要 Service guard 跨 timer/wake/Pending、引入生产测试 hook或改变 lifecycle
  ownership 才能建立见证。

### T1.5-R1: 恢复 shared critical-section policy 的 feature 可达性

- Requirement/Scenario: R7、Task 1.5、Acceptance 7；原 Task 1.5 D1 stop condition。
- Depends on: None；可先于 axnet test-only repair 实施。
- Targets: `kernel/src/lib.rs`、`kernel/src/drivers/critical_section_policy.rs` 移至
  `kernel/src/critical_section_policy.rs`、`kernel/src/drivers/mod.rs`、
  `tests/ms04-async-rx-host-harness.rs`。
- Current behavior: root D1 RISC-V check在 `kernel/src/lib.rs:66` 以 `E0432/E0433`
  失败；QEMU 与 MS04 host harness 通过。
- Required behavior: policy 是无条件可达的 crate-root shared module；production
  `critical_impl` 和 host harness 编译并执行同一 `IrqOps/acquire/release` 源文件。
- Required changes: 先记录当前 D1 RED；移动模块并更新 crate-root declaration、production
  import、harness `#[path]`、相关注释/source guard；从 drivers module 删除旧声明和孤儿。
- Preserve: `critical-section` 的 `restore-state-bool`、`set_impl! + Impl`、acquire 总
  disable、release(false) 不 enable、release(true) 只 enable 一次，以及 QEMU/D1
  共用行为。
- Forbidden: 在 smoke 中重新启用完整 drivers、复制 policy、把 axhal 注入纯 policy、
  删除 critical-section impl、绕过 D1 Gate或修改 D1 UART/平台行为。
- Test witness: 当前 root D1 target check 是 RED；MS04 host harness 16/16 与 QEMU
  kernel check 是变更前 GREEN。
- GREEN condition: root D1 target check、正式 `make lichee` 产品构建、QEMU kernel
  check 与 MS04 host harness 全部 exit 0；全仓只有一个 policy 实现来源。
- Verification: 下列 Verification 中的 host、D1、QEMU、source 与 diff Gate。
- Stop when: 修复需要改变 critical-section ABI/restore 语义、重新纳入完整 drivers、修改
  平台 IRQ primitive或扩大到 D1 runtime。

**Invariants**

- runner 只通过 Router/device adapter 和 packet slots推进协议栈，不访问 transport
  descriptor/token。
- StackEvent 与 QueueEvent generation 分离；software event 不唤醒 queue owner。
- 所有 guard 在 wake、timer arm、await/Pending 前释放。
- QEMU 与 D1 共用 critical-section restore policy，但编译成功不声明 D1 runtime。
- legacy socket inline poll、Service timeout/register_waker 保留到 Iteration 001。
- 用户 staged improvement 与其他无关工作树内容不修改、不归因。

**Non-goals**

- Tasks 2.1–3.4、multiwaiter/readiness、guest probe或 QEMU runtime acceptance。
- D1 真板启动、UART、clock/reset/IRQ/DMA 验证。
- reset、SMP、性能、依赖更新、全局 tasks/SNAPSHOT、archive或 commit。
- 修复现有 warning或清理与三个 repair item 无关的代码。

**Acceptance**

1. T1.3-R1 / R3：完整 `stack_round` 证明 RX budget-hit 后后续 stage 仍执行，RX/TX
   fault 可从 outcome 观察；既有 budget/space/TX/ownership tests 不退化。
2. T1.4-R1 / R2：完整 future 的 generation interleaving 在 guard 释放后 retry/self-wake，
   targeted ordinary/diagnostic 各 100×无 lost wake；timer replacement、stale deadline 和
   exactly-once expiry 通过。
3. T1.5-R1 / R7：root D1 target check 与正式 D1 build通过；QEMU kernel check 和同源
   MS04 restore-policy harness 继续通过。
4. Cycle 000 已通过的 Tasks 1.1–1.4 行为、MS05 owner/slot/ticket/flush、legacy socket
   compatibility 和 OpenSpec 边界保持。
5. fmt/source/diff/strict OpenSpec 通过，完整 diff 无未解决 Critical/Important finding。

**Verification**

- `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib`
- 同命令增加 `--features qemu-diagnostics`
- 新 future generation/timer targeted cases在 ordinary、qemu-diagnostics 下各重复 100 次
- `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test`
- `cargo check --locked --offline -p starry-kernel --features qemu`
- `cargo check --locked --offline --target riscv64gc-unknown-none-elf --features lichee-d1`
- `make lichee`
- `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check`
- `cargo fmt --manifest-path kernel/Cargo.toml -- --check`；若全清单仅在本 Cycle 未修改文件
  上有基线 diff，必须逐文件证明并对本 Cycle 路径运行 `rustfmt --check`，不得越界格式化
- source assertions：policy 单一来源且两端引用同一文件；smoke 不重启完整 drivers；
  future interleaving test经过完整 poll；stage test经过实际 `stack_round`
- `openspec validate ms06-application-visible-async-network-stack --strict`
- `git diff --check` 与完整 diff review；单独排除用户 staged improvement 的归属

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | feature flow、policy 全部引用、future/stage 实际路径和测试缺口已定位；fresh RED/GREEN 基线已记录 |
| Design | PASS | policy 仅提升模块 ownership；runner/stack 行为不变，测试策略与允许的最小修复边界闭合 |
| Iteration Plan | PASS | 三项 repair 均关闭 Iteration 000 原 Acceptance；Iteration Map 与后续依赖不变 |
| Cycle Scope | PASS | T1.3-R1、T1.4-R1、T1.5-R1 映射原 Tasks 1.3–1.5，无新全局 task |
| Task Contracts | PASS | 每项包含位置、RED/GREEN、保持/禁止、验证和停止条件，可仅据本 Cycle 执行 |
| Traceability | PASS | R2→T1.4-R1→future tests；R3→T1.3-R1→round tests；R7→T1.5-R1→D1/QEMU/host Gates |
| Verification | PASS | host/unit/compile/build/source/diff Gate 分层，QEMU compile不冒充 runtime或真板证据 |
| User Approval | PASS | 用户 2026-08-23 明确批准并指示执行（原话见 Plan Context Approval） |

**Persisted Evidence**

- Mode: none

全部失败和通过结果可由确定性 host/unit/compile/build 命令低成本重跑；Act Response 记录
命令、每项不超过 20 行的决定性输出、exit、revision 和 changed symbols 即可。没有
runtime、一次性环境或无法摘要的结构，因此不创建 Evidence 目录。

**Risks and Notes**

- `make lichee` 还包含产品 build 后的 Android boot image pack/inspect；失败时必须按
  compile、objcopy、pack 或 inspect 分层记录，不能把后层失败归因于 policy。
- 物理移动 policy 后必须用 `rg` 确认旧路径无引用和孤儿；不允许保留复制文件。
- 当前 axnet warnings 和 kernel format 基线不自动成为本 Cycle 修复范围；只有本 Cycle
  修改路径的新 warning/format diff 阻塞。
- 本 Cycle 是同一 Acceptance gap 的第一次 rework；三次失败规则尚未触发。

## Act Response

- Status: reported

**Implemented**

- T1.3-R1：在 `crates/axnet/src/service.rs::tests` 新增使用真实 `Service::stack_round` 的 3 个完整 round 见证：`full_round_executes_dispatch_after_rx_budget_hit`（33 项 RX 消耗 32 预算后，同轮 5 个 malformed TX 仍被 dispatch，`work == 37`、`backlog`、`self_yield`、`drop_count(MalformedIp) == 5`）、`full_round_rx_fault_is_not_hidden_as_idle`（FaultingRx → `faulted == true` 且非 self_yield）、`full_round_dispatch_fault_surfaces_in_outcome`（FaultingTx preflight fault → `faulted == true` 且 `tx_faulted()`）。三个测试均立即 GREEN，证明 Cycle 000 已实现的 `stack_round` stage 顺序/budget/fault 契约正确，无需产品代码修改。
- T1.4-R1：在 `crates/axnet/src/stack_runner.rs::tests` 新增 `PublishOnceDevice`（在 `recv` 内发布 `StackEvent`，构造 register-后/recheck-前确定性交错）与 2 个见证：`event_published_inside_round_retries_after_guard_release`（完整 `StackRunnerFuture::poll` 路径，`event_retry == 1`、`rounds == 1`，且 UnlockWake 证明 retry self-wake 发生在 Service guard 释放后）、`timer_replacement_ignores_stale_and_expires_exactly_once`（直接驱动私有 `arm_timer`/`poll_timer` 与 injected clock：earlier→later replacement 后旧 deadline 不触发、当前 deadline exactly-once 触发并取消、之后可重新 arm 且不重复计数）。两者 ordinary/qemu-diagnostics 下均 100/100 通过，验证 Cycle 000 已实现的 generation recheck 与 timer 逻辑正确，无需产品代码修改。
- T1.5-R1：把 shared critical-section restore policy 从 `drivers` 提升为无条件 crate-root shared module。工作树中文件已在索引完成 rename（`R kernel/src/drivers/critical_section_policy.rs -> kernel/src/critical_section_policy.rs`）；本 Cycle 补齐全部引用：`kernel/src/lib.rs` 新增无条件 `mod critical_section_policy`、`critical_impl` import 改为 `crate::critical_section_policy`；`kernel/src/drivers/mod.rs` 删除孤儿 `pub(crate) mod critical_section_policy`；`tests/ms04-async-rx-host-harness.rs` 的 `#[path]` 与注释更新到 crate root；policy 文件 doc 注释同步。QEMU、MS04 host harness 与 root D1 现在编译执行同一 policy 源文件，D1 smoke 不再被 `drivers` 排除所阻塞。

**Changed Files and Symbols**

- `crates/axnet/src/service.rs`：tests 模块新增 `CountingRxDevice`、`FaultingRxDevice`、`FaultingTxDevice`、`broadcast_ipv4_packet()` 与 3 个 `full_round_*` tests；测试 imports 扩展。
- `crates/axnet/src/stack_runner.rs`：tests 模块新增 `PublishOnceDevice` 与 `event_published_inside_round_retries_after_guard_release`、`timer_replacement_ignores_stale_and_expires_exactly_once`。
- `kernel/src/lib.rs`：`critical_section_policy` 模块声明（无 feature gate）、`critical_impl` 的 import 与注释路径。
- `kernel/src/drivers/mod.rs`：删除 `critical_section_policy` 模块声明。
- `kernel/src/critical_section_policy.rs`：doc 注释模块路径。
- `tests/ms04-async-rx-host-harness.rs`：`#[path]` 与 doc 注释。
- 无产品数据路径符号改动。

**Deviations from Plan**

- policy 文件物理移动在 Cycle 开始前已存在于工作树索引（可能来自 Cycle 000 收尾前的手工 `git mv`）；本 Cycle 按契约完成了移动的全部下游引用修复，与 Plan 的 Change Surface 一致。
- T1.4-R1 timer 测试首版断言在 re-arm 处 RED：我用 `arm_timer(now=t20, deadline=Some(t20))`，而 `arm_timer` 对 `deadline <= now` 定义为立即触发（正确语义），导致 `timer_deadline` 为 None。这是测试断言错误而非产品缺陷；改为 arm 未来 deadline `Some(t20)` at `now=t10` 后断言通过。replacement/stale/exactly-once 断言在一次通过中即验证正确。
- root D1 使用 `cargo check --locked --offline --target riscv64gc-unknown-none-elf --features lichee-d1`（Plan 明确该命令需 RISC-V target，宿主 target 的 `sbi-rt` 报错不计 Gate）。

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: PASS
- Full diff reviewed: PASS
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 0

完整 diff（6 个文件，314+/13-）已逐文件审查：T1.3-R1/T1.4-R1 均为纯测试新增（无产品代码改动，符合契约"只有 RED 暴露实现错误时才修改原函数"）；T1.5-R1 为最小引用修复，无复制 policy、未重新启用 smoke 下完整 drivers、未修改 critical-section ABI。用户并发内容（SNAPSHOT、regression Runbook、knowledge/references/improvements）未修改、未归因。

**Verification Evidence**

| 验证项 | 命令 | 输出摘录 | 结论 |
|---|---|---|---|
| axnet ordinary tests | `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib` | `test result: ok. 244 passed` | PASS |
| axnet qemu-diagnostics tests | 同命令加 `--features qemu-diagnostics` | `test result: ok. 264 passed` | PASS |
| T1.4-R1 future interleaving ×100 (ordinary) | `--quiet event_published_inside_round_retries_after_guard_release` ×100 | 100/100 `ok. 1 passed` | PASS |
| T1.4-R1 timer ×100 (ordinary) | `--quiet timer_replacement_ignores_stale_and_expires_exactly_once` ×100 | 100/100 `ok. 1 passed` | PASS |
| T1.4-R1 ×100 (qemu-diagnostics) | 上述两条加 `--features qemu-diagnostics` ×100 | 各 100/100 | PASS |
| MS04 host harness | `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test` | `ok. 16 passed` | PASS |
| 相邻 host harness | `make host-test` | early-console 6 / memtrack 8 / ms03 33 / ms04 16，fixtures 均 FAILED AS EXPECTED，exit 0 | PASS |
| QEMU kernel check | `cargo check --locked --offline -p starry-kernel --features qemu` | `Finished dev profile` | PASS |
| root D1 check | `cargo check --locked --offline --target riscv64gc-unknown-none-elf --features lichee-d1` | `Finished dev profile` | PASS |
| D1 产品构建 | `make lichee` | release build + objcopy + Android boot img pack/inspect, exit 0 | PASS |
| fmt | `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check`；`cargo fmt --manifest-path kernel/Cargo.toml -- --check` | exit 0（运行 `cargo fmt` 修复 axnet import 排序后） | PASS |
| source assertions | grep `kernel/src/drivers/critical_section_policy\|crate::drivers::...` 无命中；`mod critical_section_policy` 仅 lib.rs:38 与 harness:20；lib.rs `mod drivers` 仍带 `#[cfg(not(feature = "lichee-d1-smoke"))]` | PASS（旧路径零引用、单一来源、smoke 不重启 drivers） | PASS |
| OpenSpec validate | `openspec validate ms06-application-visible-async-network-stack --strict` | `Change 'ms06-application-visible-async-network-stack' is valid` | PASS |
| git diff --check | `git diff --check` | exit 0，无空白错误 | PASS |

**Persisted Evidence**

None required.

Plan 的 Persisted Evidence Mode 为 `none`；全部失败/通过可由确定性 host/unit/compile/build 命令低成本重跑，Act Response 已记录命令、决定性输出与 exit。

**Experience Candidates**

None.

本 Cycle 全部结果均为可低成本重跑的确定性验证；T1.4 timer 断言的一次性修正属于测试编写错误，不构成系统诊断信息。

**Remaining Issues**

- 无未解决的 Critical/Important/Minor finding。
- Task 1.5 现可在本 Cycle 关闭（D1 已通过）；Tasks 2.1–3.4 仍属后续 Iteration，本 Cycle 未宣称 T10 readiness 或 QEMU runtime 完成。

**Commit or Diff Reference**

- Working-tree diff at revision `b8e7bcae27579aa7ea7bf31698e3136f5856302d`；未创建 commit。

## Plan Review

- Status: pending

**Review Result**

Pending.

**Findings**

Pending.

**Deviation Classification**

None.

**Acceptance Gaps**

Pending.

**Convergence**

Pending.

**Evidence**

Pending.

**Follow-up Decision**

Pending.

**Iteration Plan Update**

Pending.

**Next Cycle**

None.

**Next Iteration**

None; create Iteration 001 only after this Cycle is accepted.
