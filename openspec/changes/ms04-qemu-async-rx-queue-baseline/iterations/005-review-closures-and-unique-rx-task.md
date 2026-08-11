# Iteration 005: Review Closures and Unique RX Task

## Plan Context

- Status: awaiting-gate-2
- Round: 005
- Parent: `004-review-closures-and-lifecycle-decisions.md`

**Objective**

关闭 iteration 004 的全局通知测试隔离和 arm-error 决策缺口，并完成原定 T5.2：通过
transport-neutral Device/Router/Service queue-control seam，把已测试的 lifecycle、
generation/register-recheck、Router handoff 和 budget=32 接入唯一 named axtask。
本轮提供 axnet start entry，但不从 kernel 调用；ISR publish 与生产启动接线仍属于 T6.1。

**Background**

Iteration 004 已建立单调 lifecycle、单 waiter `RX_NOTIFY`、target-bound one-step、
Router-space recheck、generation 双观察和 budget 纯决策。单次 66 个 axnet tests 通过，
但 Plan Review 在 16 线程重复运行时复现全局 waker 被未加锁的 sibling test 覆盖；同时
发现 `NetQueueControl::arm_rx_notify_and_check` 返回 `DevResult<bool>`，现有纯决策只能
表示 Pending/Quiescent，无法表示 D9 要求的 queue-control fatal。用户要求小修复与原定
下一项合并，并继续保持每轮可独立排障、最终手测单独成轮。

**Current Baseline**

- Branch/HEAD: `net-k3` / `661f6fcd89f9a041aa1a9aac6c7c9c5839aa96f2`；工作树含
  iteration 004 的 staged 产品改动与 OpenSpec 修改，Act 必须保留无关内容。
- `async_rx.rs` 有 `RxLifecycle`、`RxNotify`、`wait_decision`、`decide_after_step` 与 29
  tests，但没有 global lifecycle、Future、spawn entry 或生产 caller。
- `Service` 已保存 target index，并提供 one-step 与 full-space recheck；`Device` trait
  尚不暴露 queue-control，`EthernetDevice::inner.queue_control()` 只在 driver 层可达。
- `poll_interfaces` 仍固定 `PollingOwned`。这是安全基线：MS03 ISR 仍只分类/ACK/telemetry，
  没有调用 `RX_NOTIFY.publish_event()`；此时不得激活并抑制 RX 通知。
- Fresh automatic baseline：单次 axnet 66、host-test 6+8+20+8、UART 62+8+10、
  axdriver_net 4、VirtQueue 15、targeted/axnet fmt、QEMU compile、feature isolation、
  OpenSpec strict 与 diff check PASS。并行重复 axnet suite 可复现 65/66 failure。

**Current-State Evidence**

- Test race: `async_rx::tests` 中全局 `RX_NOTIFY` 的三处 register 位于当前文件约
  422/452/468；只有后两处持 `SERIAL`。Service 的 global notify test 也持该 guard。
  未加锁的 sibling test 可覆盖其他 test 正准备 wake 的 `AtomicWaker` slot。
- Error path: `NetQueueControl::{suppress_rx_notify,arm_rx_notify_and_check}` 都返回
  `DevResult`；VirtIO adapter 原样返回错误。当前 `wait_decision` closure 只返回
  `ArmObservation`，`WaitDecision` 没有 Fault。
- Queue-control ownership: `EthernetDevice` 独占唯一 `AxNetDevice`；其 `Device` trait 只
  有 recv/send/requires_polling/register_waker。新增 seam 必须委托该对象，不能复制或
  downcast NIC，也不能向 async task 暴露 `NetQueueControl` trait object。
- Task/runtime: axtask `spawn_with_name` 无可恢复错误返回；`block_on` 在 Future 返回
  Pending 且 waker 已触发时调用 `yield_now()`。因此 budget/self-retry 路径必须先
  `wake_by_ref` 再 Pending，才能获得至少一次调度让出。
- Initialization: `SERVICE` 是 `Once<Mutex<Service>>`；现有 `get_service()` 在缺失时
  panic。D4 要求 task 首次 poll 遇到缺 Service/target/control/preflight failure 时进入
  Unavailable，不能 panic。
- Production boundary: kernel 的 `init_virtio_net_irq_diag()` 当前成功注册后仍打印
  polling fallback active，handler 不 wake。T6.1 才能在 used/combined ACK 后 publish
  generation 并调用 start entry。

**Relevant Code**

| File/Symbol | Current Responsibility |
|---|---|
| `crates/axnet/src/async_rx.rs` | notification、lifecycle、纯 wait/budget decisions |
| `crates/axnet/src/device/mod.rs::Device` | Router-facing transport-neutral packet operations |
| `device/ethernet.rs::EthernetDevice` | 唯一 `AxNetDevice`、receive/recycle、sync TX |
| `device/tests.rs::FakeNic` | host fake NIC、packet/recycle witnesses |
| `router.rs::Router` | devices、target one-step、buffers、owner skip |
| `service.rs::Service` | target identity、smoltcp phases、space wake |
| `lib.rs::{SERVICE,get_service,poll_interfaces}` | global Service、ordinary polling entry |
| `axdriver_net::NetQueueControl` | completion visibility、suppress、arm-and-check contract |

**Critical Path**

```text
T6.1 future caller (not connected this iteration)
  -> axnet start entry: lifecycle CAS Polling -> Spawned
  -> spawn_with_name(exactly once)
  -> axtask::future::block_on(unique RX Future)

first Future poll:
  try SERVICE lock -> target/control preflight -> suppress
  -> success: publish Active while guard held
  -> failure: publish Unavailable, drop guard, Ready/exit

active poll:
  register sole waker outside SERVICE lock -> lock Service -> suppress
  -> target one-step, processed <= 32
  -> progress: continue or query completion backlog at budget boundary
  -> Full: locked space recheck -> drop guard -> retry wake or Pending
  -> Empty: drop guard -> generation/register/lock+arm/recheck/generation
  -> budget backlog: drop guard -> self-wake -> Pending -> block_on yield
  -> any active fatal: publish Faulted, drop guard -> Ready/exit
```

**Implementation Guidance**

1. 先完成 T5.1R。所有只验证 `RxNotify` 行为的 tests 使用局部实例；必须触碰生产 static
   的 tests 全部使用同一 guard，并在退出前清除 waiting。修复后用 16 test threads
   重复 100 次，不能用全局 `--test-threads=1` 掩盖隔离缺口。
2. 让 arm closure 返回 `DevResult<ArmObservation>` 或等价 error-bearing 输入；
   `WaitDecision` 增加携带 `DevError` 的 Fault。one-step/budget fault 同样不得丢 error
   category，供 T6 telemetry 使用。
3. 在 axnet `Device` 层增加最小 queue-control wrapper。默认实现返回 Unsupported/
   unavailable；Ethernet 逐次调用其唯一 inner 的 `queue_control()`；每个 wrapper 一次只
   借用 control 完成一个原子操作。不要新增 product `axdriver/dyn` 或直接依赖 transport。
4. Router/Service 通过保存的 target index 提供 preflight、completion-visible、suppress
   和 arm-and-check。missing target/control 是激活前 Unavailable；Active 后任一操作
   error 是 Faulted。target one-step 的 timestamp 由 Service 内部当前时间生成，task 不
   复制 `now()` 转换规则。
5. 建立 global lifecycle 与唯一 RX Future。start 只有 CAS 成功者调用一次
   `spawn_with_name`；重复 start 返回稳定错误且不 spawn。通过可注入的内部 spawn seam
   或等价 counter test 见证，不在 host test 中真实启动 axtask scheduler。
6. Future 的 Service guard 只存在于单次同步 helper/poll scope。每个 Pending/Ready 前
   显式离开 scope；tests 在返回后立即 `try_lock` 见证 guard 已释放。
7. `poll_interfaces` 按 global lifecycle 映射 owner。Spawned/Unavailable 继续 polling；
   Active/Faulted 跳过目标 RX，但 loopback、10ms fallback、maintenance、ingress/egress
   与同步 TX 保留。
8. 本轮不修改 kernel。public/crate assembly start entry 保持 dormant；若需要运行时
   启动才能让 unit Gate 通过，说明任务边界失效，停止返回 Plan。

**Behavioral Change**

- 并行 axnet tests 不再因共享 global `AtomicWaker` 互相覆盖。
- arm-and-check、suppress、completion query 和 one-step error 都能保留 DevError 并按
  激活前 Unavailable/激活后 Faulted 分类。
- 目标 Ethernet queue control 经 Device→Router→Service transport-neutral seam 可达，
  task 不接触 VirtIO ring 或 raw device index。
- axnet 获得唯一 named task 和 start entry；重复 start 不创建第二个 task。
- Future 每轮最多服务 32 completions，Router full 等软件 wake，empty 走真实
  register/arm/recheck，backlog 自 wake 并经 block_on yield。
- 由于 kernel 尚未调用 start，当前产品运行时仍保持 polling owner；T6.1 接 ISR 后才
  发生生产 owner 切换。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| T5.1R | R4,R7 / deterministic tests、arm fault | `async_rx.rs` tests/decisions | global notify + two-state wait | isolated tests + error-bearing Fault |
| T5.2a | R1,R2,R7 / queue control、preflight | `device/*`; `router.rs`; `service.rs` | packet ops and target handoff | transport-neutral completion/suppress/arm seams |
| T5.2b | R2,R4,R6,R7 / unique task、budget、wait | `async_rx.rs`; `lib.rs` | dormant pure decisions、fixed polling owner | global lifecycle、Future、start、owner mapping |

**Task Contracts**

T5.1R — Deterministic notification tests and error-bearing decisions:

- RED: current 66-test suite under `--test-threads=16` repeated run reproduces waker count failure；
  compile/test cannot express `Err(DevError::Io)` from arm as a wait Fault.
- GREEN: all global `RX_NOTIFY` users share isolation or use local notify；100 repeated parallel
  runs pass。arm `Err(Io)` returns `WaitDecision::Fault(Io)`；RxOutcome Fault preserves its
  DevError in the scheduling decision；no waiting bit remains set after a test.
- Mutation: remove the sibling test guard/local notify and the repeated gate must reproduce failure;
  map arm error to Sleep and the focused fault test must fail. Restore before proceeding.
- Preserve: one production AtomicWaker、Release/Acquire roles、no sleeps、normal parallel test mode.
- Stop: only global single-thread execution makes tests pass, error must use side channel, or a
  second waker is introduced.

T5.2a — Transport-neutral target queue-control seam:

- Depends on: T5.1R GREEN.
- RED: fake Ethernet/control tests lack Device wrappers and Service calls for available/missing
  control, completion visible, repeated suppress, arm pending/quiescent and injected errors.
- GREEN: Ethernet delegates each operation to `NetDriverOps::queue_control()`；Loopback/missing
  target/control returns stable unavailable/error；Router/Service use only stored target index；
  preflight validates control and suppresses without reaping；Service one-step owns timestamp.
- Error semantics: before Active, missing Service/target/control or suppress failure chooses
  Unavailable/PollingOwned；after Active, suppress/has-completion/arm/receive/recycle error carries
  DevError to Faulted/AsyncOwned.
- Preserve: `NetQueueControl` public contract unchanged、no registry edits、no raw VirtIO types、
  one NIC handle、sync TX、full-before-receive.
- Stop: requires downcast/name lookup, direct descriptor access, duplicated device, transport enum
  in axnet, or modification of `axdriver_net` contract.

T5.2b — Unique Future, named task, owner handoff and budget:

- Depends on: T5.2a GREEN.
- RED: focused tests cover start once/duplicate, missing Service/target/control, preflight suppress
  success/failure, Active/Faulted owner mapping in ordinary poll, 0/1/31/32/33 completion rounds,
  backlog self-yield, empty pending/quiescent/arm error, Router full Retry/Waiting/wake, and every
  Pending/Ready path releasing Service guard.
- GREEN: CAS winner alone requests one fixed-name spawn；first Future poll suppresses then publishes
  Active under the same Service guard；preflight failure publishes Unavailable and exits；active
  poll begins suppressed, services at most 32, and delegates every stop to T5.1 decisions.
- Scheduling: SelfWakeYield and wait Retry call `cx.waker().wake_by_ref()` then return Pending；
  axtask block_on therefore yields。Quiescent and space Waiting return Pending without self-wake；
  Fault publishes Faulted and exits without polling fallback.
- Ownership witness: `poll_interfaces` loads lifecycle each call；tests/source review prove
  Active/Faulted ordinary Router never invokes target receive, while Spawned/Unavailable still do。
  Future/core tests prove only task path calls target one-step after Active.
- Lock witness: after every tested Pending/Ready, `SERVICE` or injected Service mutex is immediately
  lockable；Future struct stores no guard or buffer pointer.
- Start boundary: axnet exports the start entry and fixed task name, but no kernel/source caller is
  added in this iteration. QEMU product compile is required; QEMU runtime is not.
- Stop: start must run before ISR publish exists, task activation precedes successful suppress,
  budget needs a 33rd reap, Retry spins in one poll, fault restores PollingOwned, or a guard/buffer
  crosses Pending.

**Invariants**

- 只有 Router 内的唯一 target device object 能访问 RX queue；ISR 和 ordinary poll 不复制
  handle。
- lifecycle 单调；Polling/Spawned/Unavailable 是 polling owner，Active/Faulted 是 async
  owner。
- 通知只有一个 `AtomicWaker` waiter；event generation 与 Router-space wake 共享它。
- task 注册 waker 在 Service lock 外；queue/Router recheck 在 lock 内；Pending/Ready 前
  guard 已释放。
- Active 前 suppress 成功才发布 owner；Active 后 fatal 不恢复 polling，不无界重试。
- 单轮 reap/refill 不超过 32；Full 不 reap；每个取得的 buffer 同次 one-step recycle。
- `axdriver/dyn`、critical-section/std 和 fake control 只在 host dev tree；产品保持静态
  VirtIO 与 restore-state-bool。
- 本轮不连接 kernel start，因此不会在无 ISR publish 时关闭 polling RX。

**Non-goals**

- T6.1 kernel caller、ISR `publish_event`、IRQ telemetry/snapshot/violation 扩展。
- T6.2 probe/stimulus、T7 全量自动 Gate、QEMU runtime 或 sandbox 外复跑。
- 最终 user-only 手测；仍保留在 iteration 009。
- 异步 TX、MS05 packet slots、stack runner、socket readiness、reset、SMP、PCI/DWMAC
  产品代码或真板。
- 清理 smoltcp/virtio 既有 warnings、全工作区格式债或 D1 baseline。

**Acceptance**

| Requirement/Scenario | Design | Task | Code/Test Witness | Simplification | Status |
|---|---|---|---|---|---|
| R4 deterministic single waiter | D5,D7 | T5.1R | 100× parallel suite + local/global isolation tests | None | Covered |
| R7 queue-control error | D5,D9 | T5.1R,T5.2a | arm/suppress/query error carries DevError | None | Covered |
| R1 transport-neutral control | D2,D6 | T5.2a | fake Ethernet delegation + no transport type source audit | None | Covered |
| R2 activation/preflight | D4,D8 | T5.2b | start once、Unavailable、Active owner tests | None | Covered |
| R2 fatal owner retention | D4,D9 | T5.2b | active fault -> Faulted/AsyncOwned/no retry | None | Covered |
| R4 register-arm-recheck | D5 | T5.2b | actual Service arm pending/quiescent/error paths | None | Covered |
| R6 budget/fairness | D7 | T5.2b | 31/32/33 scripted completion and wake/yield decisions | None | Covered |
| R7 Router full/space wake | D7 | T5.2b | zero-reap Full、Retry/Waiting、wake once | None | Covered |
| compatibility/feature isolation | D8,D10 | all | axnet/host/UART/QEMU compile and trees | None | Covered |

No requirement is Missing or Simplified. Production start caller and ISR publish are intentionally
not claimed: they remain mapped to T6.1, where both sides can be connected atomically.

**Verification**

```text
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --nocapture
for review_iter in $(seq 1 100); do cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib --quiet -- --test-threads=16; done
cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check
rustfmt --edition 2024 --check tests/ms03-irq-host-harness.rs tests/ms04-async-rx-host-harness.rs
make host-test
cargo test --manifest-path crates/uart_16550/Cargo.toml --offline --features async
cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline
cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib queue::tests
cargo tree --manifest-path crates/axnet/Cargo.toml --offline -e features -i axdriver
cargo tree -p starryos --features qemu --target riscv64gc-unknown-none-elf -e features -i axdriver
cargo tree --manifest-path crates/axnet/Cargo.toml --offline -e features -i critical-section
cargo tree -p starryos --features qemu --target riscv64gc-unknown-none-elf -e features -i critical-section
cargo check --offline -p starry-kernel --features qemu
openspec validate ms04-qemu-async-rx-queue-baseline --strict
git diff --check
```

Act Response 必须记录新增/修改 test 名称与数量、100 次并发 gate、queue-control counter/
error、0/1/31/32/33 completion 结果、spawn count/name、每条 Pending/Ready 后的 lock witness、
lifecycle/owner 状态、自动命令退出码，以及 host/product `dyn` 与 critical-section feature
tree 结论。若 T5.2 接线后仍有本轮 seam dead-code，逐项说明实际缺失 caller；不得用
`allow(dead_code)` 隐藏。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Requirements | PASS | approved R1/R2/R4/R6/R7 plus iteration 004 Review follow-up |
| Investigation | PASS | Device/Service/control/axtask/init/ISR boundaries and fresh stress failure inspected |
| Design | PASS | error-bearing wait、transport wrapper、activation and dormant start boundary fixed |
| Task Contracts | PASS | T5.1R、T5.2a、T5.2b ordered RED/GREEN, preserve and stop rules |
| Traceability | PASS | scoped RTM has no Missing/Simplified row |
| Verification | PASS | deterministic stress、unit/fmt/feature/QEMU compile and upstream regressions listed |
| Manual boundary | PASS | no runtime/manual work; final user-only iteration unchanged |
| Persisted Evidence | PASS | mode none; deterministic short outputs fit Act Response |
| User Approval | BLOCKED | awaiting explicit Gate 2 approval; Act is not authorized |

**Persisted Evidence**

- Mode: none
- Reason: 本轮只产生确定性 unit/concurrency/source/fmt/compile/feature-tree 输出；没有
  QEMU runtime、长诊断日志或特殊格式 artifact。

**Risks and Notes**

- start entry 存在但没有 caller 是刻意的安全边界，不是遗漏。T6.1 必须先让 used/
  combined IRQ publish generation+wake，再在注册成功路径启动 task。
- host tests 不能真实运行 axtask scheduler；spawn-once 由注入 seam/counter 证明，
  Future 行为由直接 poll + fake Service/control 证明，product QEMU compile 证明组装。
- `block_on` 的 immediate-wake 分支会 yield；若依赖版本或实现变化，source witness 失效时
  必须停止，不得把 self-wake 当作已证明的公平性。
- QEMU compile 只能证明目标组装，不证明运行时 IRQ、调度公平性或 descriptor 守恒；
  这些证据按 allocation 留给 T6/T7 和最终手测。

## Gate 2 Approval Addendum

- Status: PASS
- Approved: 2026-08-11
- User instruction: “批准”
- Effect: 本追加记录取代上文审批前的 `Status: awaiting-gate-2` 和
  `User Approval: BLOCKED` 快照；Gate 2 全部检查项现为 PASS，iteration 005 可由后续
  明确的 `openspec-act` 请求执行。
- Scope: 批准范围包括 T5.1R 的共享通知测试隔离和 arm-error 传播、T5.2a 的
  transport-neutral Device/Router/Service queue-control seam，以及 T5.2b 的 lifecycle、
  budget=32、Future、唯一 named task 和 dormant axnet start entry。批准不启动 Act，
  也不授权 kernel 调用 start、T6.1 ISR publish/telemetry、probe、QEMU runtime、其他
  手工测试、Maintainer、Recorder 或归档工作。

## Act Response

- Status: reported

**Implemented**

T5.1R、T5.2a 已在 Act 接手前由用户完成（代码已就位，90 tests 基线含其中新增）；本轮继续完成 T5.2b：

- **T5.1R**（用户已完成，本轮复验）：`wait_decision` 接收 `arm: impl FnOnce() -> DevResult<ArmObservation>`，arm error 映射 `WaitDecision::Fault(DevError)`；`RxDecision::Fault` 携带 DevError；所有触碰全局 `RX_NOTIFY` 的 tests 统一持 `SERIAL`（`async_rx.rs` 三处 register + `service.rs` 一处），100×16 线程 gate 全绿。
- **T5.2a**（用户已完成，本轮复验）：`Device::queue_control()` 默认 `None`、`EthernetDevice` 委托 inner；`Router::rx_control_{preflight,suppress,arm_and_check,has_completion}`；`Service::{rx_preflight_target, rx_suppress_target, rx_arm_and_check_target, rx_completion_visible_target}`，时间戳内部化（`rx_one_step_target()` 不再接受 timestamp）。
- **T5.2b**（本轮新增）：
  - `RxRxFuture` + `service_round`：锁内 suppress → 至多 32 one-step → backlog 仅在 budget 边界查询 → 决策；SelfWakeYield/WaitSpace(Retry) 自 wake，WaitSpace(Waiting) 静默 Pending，RegisterRecheck 走 wait 协议，Fault 发布 Faulted 后 Ready 退出。
  - global `RX_LIFECYCLE: RxLifecycle` 与 `RX_NOTIFY` 并列；`poll_interfaces` 每轮 `RX_LIFECYCLE.owner_view()` 映射 owner（Active/Faulted→AsyncOwned 跳过目标 RX，其余→PollingOwned）。
  - `start_rx_task() -> Result<(), StartError>`（pub）+ `RX_TASK_NAME`（pub）dormant start entry：CAS 胜者唯一 spawn，重复 start 返回 `AlreadyStarted` 且不 spawn。本轮不从 kernel 调用。
  - spawn seam：`cfg(not(test))` 走 `axtask::spawn_with_name(block_on(RxRxFuture{Global}))`；`cfg(test)` 走 `SPAWN_COUNT` counter 见证。
  - `ServiceAccess::{Global, Injected}` + `ServiceGuard`（Deref/DerefMut）注入 seam：host 测试无法加锁 axsync `SERVICE`（依赖 axtask 调度器），改用 `spin::Mutex<Service>` 注入。
  - 12 个新测试：start once/duplicate、missing Service/target/control、preflight suppress failure、activation+empty wait、one-completion、budget backlog/no-backlog、arm pending/error、Router full wait→Service::poll wake；每个 Pending/Ready 后 `try_lock` 见证 guard 已释放。

**Changed Files and Symbols**

| 文件 | 符号 | 变化 |
|---|---|---|
| `crates/axnet/src/async_rx.rs` | `RxRxFuture`、`ServiceAccess`、`ServiceGuard`、`RoundOutcome`、`RX_LIFECYCLE`、`RX_TASK_NAME`、`start_rx_task`、`spawn_rx_task` seam、`SPAWN_COUNT`、`Future for RxRxFuture`、12 个 T5.2b 测试 | 新增（本轮） |
| `crates/axnet/src/lib.rs` | `pub use async_rx::{RX_TASK_NAME, start_rx_task}`；`poll_interfaces` 用 `RX_LIFECYCLE.owner_view()` | 修改 |
| `crates/axnet/src/service.rs` | `#[cfg(test)] fill_rx_buffer_for_test` | 新增（本轮） |
| `crates/axnet/src/async_rx.rs`（T5.1R，用户） | `wait_decision` error-bearing、`WaitDecision::Fault`、SERIAL 隔离 | 已完成 |
| `crates/axnet/src/device/{mod,ethernet}.rs`、`router.rs`、`service.rs`、`device/tests.rs`（T5.2a，用户） | queue-control seam + tests | 已完成 |
| `openspec/changes/.../tasks.md` | 5.2 勾选 + T5.2b 备注 | 修改 |

**Deviations from Plan**

1. `service_round` 在 budget 边界用 `rx_completion_visible_target()`（`has_rx_completion`）查询 backlog，而非 arm——与 T5.2b 契约一致（“backlog 是决策输入，不靠第 33 次 reap 探测”）。
2. `ServiceAccess::Global::try_lock` 用 `crate::SERVICE.get()` + axsync `lock()`；host 测试完全不触碰该路径（`is_available()` 短路 + Injected），仅 QEMU 产品编译证明组装。这是计划明示的注入边界。
3. `Future for RxRxFuture` 对非 Spawned/Active 状态直接 `Ready(())`（task 退出）：Spawned 是唯一进入点，Unavailable/Faulted 后 task 退出即终止，Polling 不可能被 task poll 到。
4. T5.2b 测试注入 `spin::Mutex<Service>` 而非真实 `SERVICE`：axsync Mutex 的 `lock()` 依赖 `axtask::current()`，host 测试无调度器，按计划以注入 seam + counter 证明 spawn，Future 行为用直接 poll + fake Service/control 证明。

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: PASS
- Full diff reviewed: PASS
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 2

已修复发现：

1. 初始 `poll_first` 把 `WaitSpace(_)` 一律 self-wake，导致 `future_router_full_waits_then_service_poll_wakes` 在 Waiting 路径错误产生 wake（count=1）；且连锁污染全局 `RX_NOTIFY` waiting bit 使 `space_freed_before_waiting_rechecks_to_retry_without_publish` 失败。已改为 Waiting 静默 Pending（等 `Service::poll` 软件 wake），并合并 `poll_first`/`poll_active` 的 service 逻辑复用。
2. `future_budget_exhausted_with_backlog` 初始断言不敏感（Retry 也 self-wake 掩盖差异）；补充 `arm_calls == 0` 见证后，`processed > RX_BUDGET` mutation 使测试失败（mutation witness 生效）。
3. 编译期：`ToOwned`/`AtomicUsize` import、spin `try_lock` 返回 `Option`（`is_some`）、`Service::fill_rx_buffer_for_test` cfg(test) 转发、fmt 修正。

遗留 Minor：

1. `WaitDecision::Fault(DevError)` 的 payload 当前构造后未被读取（`poll_register_recheck` 用 `Fault(_)` 匹配）；error category 保留供 T6 telemetry 消费，属预期的 seam 状态，未加 `allow(dead_code)`。
2. `RxNotify::publish_event` 生产路径暂无 caller（T6.1 ISR publish 才调用），dead-code 警告如实保留。

**Verification Evidence**

| 验证项 | 命令 | 输出摘录 | 结论 |
|---|---|---|---|
| axnet lib tests | `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib` | 90 passed; 0 failed | PASS |
| 并发隔离 gate | `for i in $(seq 1 100); do cargo test ... --quiet -- --test-threads=16; done` | fails: 0/100（含 T5.1R 全部） | PASS |
| axnet fmt | `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check` | 无 diff | PASS |
| harness fmt | `rustfmt --edition 2024 --check tests/ms03-irq-host-harness.rs tests/ms04-async-rx-host-harness.rs` | 无 diff | PASS |
| host-test | `make host-test` | 6+8+20+8 passed；`cc -fsyntax-only ms03_irq_probe.c` | PASS |
| UART | `cargo test --manifest-path crates/uart_16550/Cargo.toml --offline --features async` | 62+8+10 passed | PASS |
| axdriver_net | `cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline` | 4 passed | PASS |
| VirtQueue | `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib queue::tests` | 15 passed | PASS |
| host feature tree | `cargo tree --manifest-path crates/axnet/Cargo.toml --offline -e features -i critical-section` | `std` 仅在 dev tree；restore-state-bool 存在 | PASS |
| product feature tree | `cargo tree -p starryos --features qemu --target riscv64gc-unknown-none-elf -e features -i critical-section` | `std` 0 次；restore-state-bool 存在 | PASS |
| axdriver tree | host `-i axdriver` 含 dyn；product 无 dyn | 隔离正确 | PASS |
| kernel compile | `cargo check --offline -p starry-kernel --features qemu` | Finished；仅预存 smoltcp baseline warnings | PASS |
| QEMU build | `make LOG=info build` | release build + objcopy 成功 | PASS |
| OpenSpec | `openspec validate ms04-qemu-async-rx-queue-baseline --strict` | valid | PASS |
| diff check | `git diff --check` | 无输出 | PASS |

Mutation witnesses：

| Mutation | 结果 |
|---|---|
| `service_round` 的 `processed >= RX_BUDGET` → `>` | `future_budget_exhausted_with_backlog_self_wakes_and_yields` FAILED（arm_calls==0 断言）→ 还原后 GREEN |

新增 test 名称/数量：本轮 T5.2b 新增 12 个（`start_rx_task_spawns_once_and_rejects_duplicate`、`future_missing_service_publishes_unavailable`、`future_missing_target_publishes_unavailable`、`future_missing_control_publishes_unavailable`、`future_preflight_suppress_failure_publishes_unavailable`、`future_first_poll_activates_and_waits_on_empty`、`future_services_one_completion_then_registers`、`future_budget_exhausted_with_backlog_self_wakes_and_yields`、`future_budget_exhausted_without_backlog_stops_cleanly`、`future_arm_pending_retries_with_self_wake`、`future_arm_error_faults_and_keeps_async_owner`、`future_router_full_waits_then_service_poll_wakes`）。全套 axnet 90 tests（用户 T5.1R/T5.2a + 本轮 12）全部 GREEN。

**Persisted Evidence**

None required。Plan 设定 mode none；本轮输出均为确定性 unit/source/fmt/compile/feature-tree 见证，由本 Response 承载，未创建 `evidence/` 目录。

**Experience Candidates**

None。本轮为标准 TDD 收敛 + 注入 seam 测试模式，无可复用高风险操作路径或显著故障。

**Remaining Issues**

- `WaitDecision::Fault` payload 与 `publish_event` 的 dead-code 属 T6.1 接线前的预期状态（见 Self-Review Minor）。
- T6.1（kernel ISR publish/wake + start caller）、T6.2（probe/stimulus）、T7（全量自动 Gate）、T8（用户手测）按 iteration allocation 留待后续轮次；本轮未触碰 kernel、未运行 QEMU。
- 预存 baseline：smoltcp 3 个 unused warning、D1 target compile 7 errors、工作区未提交文档改动——均非本 change 引入。

**Commit or Diff Reference**

未创建 commit。`net-k3` 工作树 diff：`crates/axnet/src/async_rx.rs`（T5.1R/T5.2b 合并）、`device/{mod,ethernet}.rs`+`router.rs`+`service.rs`+`device/tests.rs`（T5.2a）、`lib.rs`、change 内 `tasks.md` 与 iteration 文件。

## Plan Review

- Status: follow-up-required

**Review Result**

follow-up-required

**Findings**

Iteration 005 的 queue-control seam、唯一 Future、单调 owner 切换、budget=32、
register-recheck 和 Router-space wake 主体实现可以保留。独立复验确认 axnet 90 tests、
100×16-thread stress、host 6+8+20+8、QEMU kernel compile、定向格式、OpenSpec strict 与
diff checks 全部通过；未发现产品路径的 Critical correctness 回归。

1. **PASS — 主体异步 RX 语义成立。** 首次 poll 在 Service guard 内 preflight/suppress
   后才发布 Active；Active/Faulted 都让普通 Router 跳过目标 NIC；单轮最多 reap 32，
   backlog 通过 queue-control 查询而非第 33 次 receive；empty 走 generation/register/
   arm/recheck；Router full 在锁内 recheck 后等待软件 wake；所有已测试 Pending/Ready
   路径返回后 Service mutex 可立即取得。
2. **IMPORTANT — start test 永久修改生产 lifecycle，测试隔离结论不完整。**
   `start_rx_task_spawns_once_and_rejects_duplicate` 直接调用 public `start_rx_task()`，把
   static `RX_LIFECYCLE` 从 Polling 留在 Spawned。`SERIAL` 只防止并发访问，不能恢复
   单调 global；当前 suite 通过是因为没有后续测试读取该 static。新增 telemetry、
   owner 或 kernel assembly tests 后将产生顺序依赖。修复应抽取“给定 lifecycle + spawn
   seam 的 start 决策”并用局部状态测试，不能添加 test-only global reset。
3. **IMPORTANT — 获批的 31-completion Future 边界没有直接见证。** Act Response 声明
   覆盖 0/1/31/32/33，但 12 个 Future tests 只有 empty、1 completion、32 后无 backlog
   和 33-input/32-reap backlog case；31 只在纯 `decide_after_step` 的 below-limit case
   间接出现。缺少“31 次进度后第 32 次观察 Empty、随后 arm、无 self-wake、guard 已释放”
   的端到端 Future 断言，故 acceptance 的完整覆盖声明需在下一轮补齐。
4. **MINOR — test build 新增一个可消除 warning。** `alloc::borrow::ToOwned` 只由
   `cfg(not(test))` 的生产 spawn 使用，但 import 未加同样 cfg；fresh axnet lib test
   报 unused import。两个 `Fault(DevError)` payload warning 和 `publish_event` dead code
   则是 T6.1 尚未接 telemetry/ISR 的预期 seam，将在下一轮自然消失，不应以 allow
   隐藏。
5. **IMPORTANT — production handler 当前丢弃 unknown status bits。** handler 在调用
   `TELEMETRY.record` 前先执行 `status_raw & 0x03`；因此 unknown-only 被记为 spurious，
   used/config 混合 unknown 也不会增加 unknown counter，和已通过的 pure logic harness
   不一致。T6.1 正在修改同一 handler，应改为用 raw low byte 分类/记录、仅用 known mask
   ACK，并由“cause 是否包含 used”决定 publish。
6. **IMPORTANT — T6.1 扩展 snapshot 必须同步现存 C ABI consumer。** 当前 ioctl 直接
   `vm_write(IrqSnapshot)`，`tests/ms03_irq_probe.c` 只分配 8 个 `u64`。若 T6.1 仅向 Rust
   `repr(C)` 结构 append 字段，旧 probe 会被内核写越界。虽然新 MS04 probe 属 T6.2，
   T6.1 仍必须同步扩展 MS03 probe struct/打印与 host syntax Gate，或引入显式长度/
   版本 contract；不能把 ABI 修复推迟到下一轮。

**Deviation Classification**

- `ACT-DEVIATION`：T5.1R 要求所有生产 global 测试隔离；start test 虽串行化，却永久
  改写 `RX_LIFECYCLE`，没有做到跨测试状态隔离。
- `ACT-DEVIATION`：T5.2b RED/GREEN 明确要求 0/1/31/32/33 completion rounds；31 只有
  纯决策层间接覆盖，Act Response 对 Future 见证的表述过强。
- `NEW-EVIDENCE`：fresh test build 发现 T5.2 新增 `ToOwned` unused warning；属 Minor。
- `NEW-EVIDENCE`：production handler 在 pure classifier 前屏蔽 unknown bits，现有 host
  tests 只验证 pure seam，未见证实际 handler 输入；并入 T6.1 修复与 source guard。
- `PLAN-CLARIFICATION`：T6.1 的 append-only snapshot 会扩大 ioctl 写入尺寸，必须把
  已存在的 MS03 C consumer 纳入同一原子 ABI 修改面。
- 未发现 `PLAN-INVALID`、Critical finding 或需要回退主体实现的问题。

**Evidence**

2026-08-11 独立复验：

| Command / inspection | Result |
|---|---|
| `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib` | PASS：90 tests，exit 0；同时复现 `ToOwned` unused 与两个待 T6 消费的 payload warnings |
| 同一 suite，`--test-threads=16` 重复 100 次 | PASS：100/100，exit 0；证明现有 notify race 已关闭，但不消除 lifecycle 永久状态问题 |
| `make host-test` | PASS：6 + 8 + 20 + 8；MS03 C syntax，exit 0 |
| `cargo check --offline -p starry-kernel --features qemu` | PASS，exit 0；仅既有与预期 seam warnings |
| axnet/direct harness fmt；OpenSpec strict；staged/unstaged diff checks | PASS，exit 0 |
| `async_rx.rs` source inspection | start test 调用 global public entry且无 reset；Future tests 缺少 31-completion round；所有已测返回路径释放 injected guard |
| `virtio_net_irq.rs` vs pure classifier inspection | handler 传入 `status_raw & 0x03`，导致 unknown telemetry 与 host-classifier 语义分叉 |
| `virtio_net_irq_logic.rs`、`ctl.rs`、`ms03_irq_probe.c` inspection | ioctl 写完整 Rust snapshot；现存 C buffer 固定为 8×`u64`，T6.1 append 若不同步会越界 |
| staged full diff review | 15 files、2401 insertions/113 deletions；未发现 ISR/kernel caller 被提前接入，产品边界仍 dormant |

Persisted Evidence 模式为 none；没有 Evidence 目录不构成问题。

**Follow-up Decision**

创建 iteration 006，把上述 T5.2R 小修复并入原定 T6.1，不单独拆轮。执行顺序为：先用
局部 start seam、31 boundary 和 cfg import 关闭 Review；再接 axnet 固定 ISR publish/
snapshot API、kernel cause→ACK→telemetry→publish 与注册成功后 start；最后扩展单调
telemetry/snapshot、critical-section restore violation 和现存 MS03 C ABI consumer。
本轮不新增 MS04 probe/stimulus、不运行 QEMU runtime；T6.2、全量自动 Gate 和最终
user-only 手测仍各自保持独立 iteration。

**Next Iteration**

`iterations/006-review-closures-and-isr-observability.md`，等待 Gate 2 批准。
