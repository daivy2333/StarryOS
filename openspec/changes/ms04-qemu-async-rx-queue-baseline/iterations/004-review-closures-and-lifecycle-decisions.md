# Iteration 004: Review Closures and Lifecycle Decisions

## Plan Context

- Status: awaiting-gate-2
- Round: 004
- Parent: `003-review-closures-and-router-handoff.md`

**Objective**

关闭 iteration 003 的 queue-side seam 可达性、Router-space lost-wake 见证和两处定向
格式问题，并完成原定 T5.1：建立可独立 host/unit-test 的 RX lifecycle、单 waiter
generation/register-recheck 与 budget 调度决策层。本轮不创建 named axtask，不调用真实
queue-control，不切换生产 owner，也不修改 ISR。

**Background**

Iteration 003 已通过 41 个 axnet tests 和相关自动回归，Router one-step、owner skip、
target identity 与 space-edge wake 的实现可保留。Plan Review 发现 Router primitive 和
space signal 都停在 `service.rs` 私有边界后面，未来 sibling async RX 模块无法调用；
现有 space tests 也只是顺序路径，不能证明 register 与 waiting 发布之间释放空间不会
丢 wake。用户要求把这种小粒度修复和原本下一项合并，同时保持每轮可排障，并把所有
QEMU/sandbox 手测留到最终独立 iteration。

**Current Baseline**

- Branch/HEAD: `net-k3` / `917b40d1dce96d0a38cc9dfba79ed0c2e085822f`；工作树含
  既有 iterations 的未提交代码和 OpenSpec 修改，Act 必须保留无关改动。
- `Router::rx_one_step(dev, timestamp)` 已保证 full-before-receive、一次 completion 和
  invalid-index fault；`Service` 保存 `target_dev`，但没有转发入口。
- `RxSpaceSignal` 已含一个 `AtomicWaker` 和非 Relaxed waiting bit；类型、static 及
  register/wait methods 都是 module-private。`Service::poll` 在 ingress 后执行一次
  clear+wake。
- `poll_interfaces` 仍固定传 `PollingOwned`；生命周期、generation、async future 和
  named task 尚不存在，这是本轮入口基线而非缺陷。
- Fresh Review baseline：axnet 41、host-test 6+8+20+8、UART 62+8+10、axdriver_net 4、
  VirtQueue 15、QEMU compile、feature isolation、OpenSpec strict 与 diff check PASS。
  两个 host harness 的直接 rustfmt 仅各有一处机械差异。

**Current-State Evidence**

- Future call boundary: sibling module 可取得 `SERVICE` guard，但不能访问私有
  `Service::router/target_dev`，因此当前无法调用 `Router::rx_one_step`。
- Space race: 已批准顺序是锁外 register → Service 锁内 one-step/recheck → 仍 full 才
  publish waiting → 释放 guard 后 Pending。当前 `wait_for_space()` 无 recheck，且只在
  `service.rs` 自身测试中可调用。
- Owner handoff: D4 要求 `Polling/Spawned/Unavailable` 映射 `PollingOwned`，
  `Active/Faulted` 映射 `AsyncOwned`；生产 caller 映射延后到 T5.2，本轮只固定原子状态
  与纯决策。
- Event race: D5 要求 Release publish、Acquire 双读 generation 和一个 queue-task
  `AtomicWaker`。真实 notification arm/recheck 已由 T2 queue contract 提供，但到 T5.2
  才在 future 中调用；本轮用确定性输入/钩子验证决策，不伪造第二 executor。
- Budget boundary: 固定 32；只有“已处理 32 且 backlog 仍存在”才 self-wake/yield，
  Full 必须在下一次 reap 前等待软件 space wake，Fault 必须给出唯一 fault 决策。

**Relevant Code**

| File/Symbol | Current Responsibility |
|---|---|
| `crates/axnet/src/router.rs::{RxOutcome,RxOwnerView,Router::rx_one_step}` | target one-step 与 owner view |
| `crates/axnet/src/service.rs::{Service,RxSpaceSignal,RX_SPACE}` | target identity、ordinary poll、space wake |
| `crates/axnet/src/lib.rs::{SERVICE,poll_interfaces}` | global Service 与当前 polling owner |
| `crates/axnet/Cargo.toml` | no_std `embassy-sync` 与 host-only critical-section/std |
| `tests/ms03-irq-host-harness.rs` | MS03 IRQ host regression |
| `tests/ms04-async-rx-host-harness.rs` | critical-section production-binding regression |

**Target Decision Flow**

```text
start decision: Polling --CAS(AcqRel)--> Spawned
first-task decision: Spawned --preflight ok--> Active
                                  \--fail--> Unavailable
active fatal: Active -----------------------> Faulted

empty/wait decision:
Acquire generation -> register one waker -> arm/recheck observation
  -> Acquire generation again
  -> pending or generation changed: self-wake/retry
  -> neither: sleep/Pending

Router full:
register waker outside SERVICE lock -> Service::rx_one_step
  -> Full -> recheck space inside lock
       -> still full: publish waiting
       -> space available: retry, do not sleep
```

**Implementation Guidance**

1. 先机械修复两个指定 harness 的 rustfmt；不得把全工作区 smoltcp baseline 或 warnings
   纳入修改面。
2. 在 `Service` 增加 crate-private target-bound one-step 方法，内部只使用已保存的
   target index；缺 target 返回 `Fault(BadState)`。future-side caller 不接受 raw index，
   不取得 Router 字段或第二个 NIC handle。
3. 把现有 space signal 收敛为一个 crate-private queue notification state，继续只有一个
   `AtomicWaker`。提供锁外 register/publish/generation snapshot seam，以及只能在
   `Service` guard 内调用的 full-space recheck 方法。若 recheck 已有空间，返回 Retry；
   只有仍 full 才 Release 发布 waiting。ordinary `Service::poll` 保留 ingress 后
   AcqRel clear+wake-once。
4. 新增独立 async RX 决策模块。lifecycle 用显式原子编码和合法 transition API：load
   Acquire，start CAS 成功 AcqRel、失败 Acquire；preflight 只允许 Spawned→Active 或
   Spawned→Unavailable；fatal 只允许 Active→Faulted。非法转换返回明确错误/决策，
   不 panic、不回退。
5. notification publish 对 `AtomicU64` generation 执行 wrapping `fetch_add(Release)` 后
   wake；观察用 Acquire。把 arm/recheck 的观察结果交给纯函数，结果只能是 retry/
   self-wake 或 sleep，不在本轮直接操作 queue control。
6. budget 纯决策固定为 32：Consumed/Delivered 且未到 budget 继续；Empty 进入
   register-recheck；Full 进入 space handoff；Fault 进入 fault；精确到 32 且 backlog
   为真时 self-wake+yield，为假时进入 empty/register-recheck。不得通过第 33 次 reap
   探测 backlog。
7. 按 T4.2R、T5.1a、T5.1b 分组完成 RED/GREEN 和 diff review；前一组未 GREEN 不进入
   后一组。

**Behavioral Change**

- sibling async RX 模块获得最小 crate-private Service/notification seam，但没有 public
  API，也没有生产 task caller。
- Router-space 等待增加 Service 锁内 recheck，释放发生在 register 与 waiting 发布之间
  时返回 retry，不形成 permanent Pending。
- axnet 获得单调 lifecycle 与 owner-view 映射的纯决策，以及 generation、arm observation
  和 budget=32 的确定性决策。
- `poll_interfaces` 在 T5.2 前仍使用 `PollingOwned`；本轮不改变运行时 RX owner。
- 两个指定 host harness 通过直接 rustfmt；产品行为不变。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Planned Change |
|---|---|---|---|
| T4.2R-fmt | Review Minor | two host harnesses | two mechanical rustfmt corrections only |
| T4.2R-seam | R2,R7 / callable handoff、space race | `service.rs`; sibling decision module tests | target-bound one-step + one-waiter register/recheck seam |
| T5.1a | R2 / lifecycle、unique owner | new axnet async RX decision module; owner mapping | atomic states、legal transitions、owner view |
| T5.1b | R4,R6,R7 / event windows、budget、full | decision module; notification state | generation publish/observe、wait and budget decisions |

**Task Contracts**

T4.2R-fmt — Targeted formatting closure:

- RED: direct `rustfmt --edition 2024 --check` on the two named harnesses exits 1 at the observed
  `fetch_add` wrapping and import ordering only.
- GREEN: the same direct command exits 0; `make host-test` remains GREEN.
- Preserve: tokens/semantics、production glue、other source formatting、smoltcp baseline.
- Stop: formatter requires bulk unrelated rewrite or any behavior-changing harness edit.

T4.2R-seam — Callable target and lossless space handoff:

- Depends on: targeted fmt GREEN.
- RED: a sibling-module test cannot call a target-bound Service one-step/register seam today；
  deterministic cases fail for missing target, register→space freed before waiting, still-full
  waiting, and space freed after waiting.
- GREEN: Service one-step uses only its stored target and maps missing target to BadState；caller
  registers without Service lock；inside the guard, Full is rechecked and yields exactly Retry or
  Waiting；Waiting followed by ingress-created space wakes once, while Retry does not publish
  waiting. A sibling module compiles against the crate-private seam.
- Preserve: Router full-before-receive、ordinary poll order、one AtomicWaker/one waiter、10ms
  fallback、no public API.
- Stop: raw device index escapes to caller, a second NIC handle/waker is needed, waiting is set
  outside the Service-serialized recheck, or a guard must cross a scheduling point.

T5.1a — Lifecycle and owner decisions:

- Depends on: T4.2R-seam GREEN.
- RED: tests enumerate all states and reject the absent transition API; duplicate start, preflight
  fail, invalid transitions and owner mapping have no implementation.
- GREEN: only `Polling→Spawned→Active→Faulted` and `Spawned→Unavailable` are legal；duplicate
  start returns AlreadyStarted/equivalent without a second spawn decision；preflight failure keeps
  polling owner；Active and Faulted map AsyncOwned, all other states map PollingOwned；no state
  rolls back.
- Ordering: Acquire loads, AcqRel successful CAS/transition and Acquire failure observation;
  telemetry-only counters may remain Relaxed.
- Preserve: no actual spawn、no `poll_interfaces` owner switch、no queue-control preflight.
- Stop: an unresolved state remains, invalid input silently changes state, or fatal restores polling.

T5.1b — Generation/register-recheck and budget decisions:

- Depends on: lifecycle GREEN.
- RED: deterministic cases cover event-before-register, event during register window, event after
  arm, pending found by arm recheck, empty/spurious wake, Full wait/retry, below-budget progress,
  exact budget with/without backlog and Fault.
- GREEN: publish increments generation with Release then wakes the sole waiter；two Acquire
  observations plus arm result always choose self-wake/retry or sleep；no event window produces
  permanent Pending。Budget is exactly 32；backlog at 32 chooses one self-wake/yield decision；
  Full chooses the T4.2R space decision without reaping；Fault is terminal for the decision layer.
- Witness: tests use explicit hooks/observations/counters, not sleeps, wall-clock scheduling or a
  second executor. A mutation removing the second generation observation makes at least one race
  test fail；changing budget from 32 makes the boundary test fail.
- Preserve: actual `NetQueueControl::{suppress,arm_and_recheck}` call、future polling、axtask
  spawn and ISR publish integration remain T5.2/T6.1.
- Stop: the pure layer must touch MMIO/smoltcp, requires the 33rd receive to know backlog, uses
  Relaxed for lifecycle/generation/space control, or leaves a catch-all undecided outcome.

**Invariants**

- 目标 NIC 仍只有 Router 内一个 owner object；caller 不传 device index、不复制 handle。
- `Polling/Spawned/Unavailable` 是 polling owner；`Active/Faulted` 是 async owner；状态单调。
- queue notification 只有一个 task waiter；event 与 Router-space wake 不建立第二套 waker。
- register 在 Service lock 外；Full recheck/wait publish 在 lock 内；任何未来 Pending 前必须
  释放 guard。
- generation、lifecycle 和 space handoff 不使用 Relaxed；telemetry 可使用 Relaxed。
- budget 固定 32；Full 时不 reap，成功 receive 仍在同次调用内 recycle。
- host-only `critical-section/std` 和 `axdriver/dyn` 不进入产品 QEMU tree。

**Non-goals**

- T5.2 named axtask、future poll loop、真实 preflight/suppress/arm、生产 owner 切换。
- T6 ISR publish caller、telemetry 扩展、probe/stimulus。
- QEMU runtime、sandbox 外复跑或任何手工测试；仍属于最终 user-only iteration。
- D1 baseline repair、MS05 packet slots、异步 TX、stack runner、SMP/真板。
- 全工作区 rustfmt、smoltcp warnings 或无关历史格式债清理。

**Acceptance**

| Requirement/Scenario | Design | Task | Code/Test Witness | Status |
|---|---|---|---|---|
| R2 target seam/unique identity | D4,D6 | T4.2R | sibling caller + missing-target/one-step tests | Covered |
| R7 Router-full race | D7 | T4.2R | freed-before-wait Retry；still-full Waiting；wake once | Covered |
| R2 lifecycle/owner | D4,D8 | T5.1a | transition matrix、duplicate start、owner mapping | Covered |
| R4 event-before-register | D5 | T5.1b | deterministic generation/arm observation cases | Covered |
| R4 register-during/after-arm | D5 | T5.1b | second-observation mutation-sensitive tests | Covered |
| R6 budget/fairness decision | D7 | T5.1b | 31/32、backlog/no-backlog、self-yield decision | Covered |
| R7 fault/full outcomes | D7,D9 | T5.1b | explicit Full/Empty/Fault terminal decisions | Covered |
| compatibility/feature isolation | D8 | all | axnet/host/UART/QEMU tree and compile regressions | Covered |

No requirement is Missing or Simplified. T5.1 只交付可调用 seam 与纯状态/调度决策；真实
task、queue control 和 ISR integration 按既有 allocation 留在 T5.2/T6.1。

**Verification**

```text
rustfmt --edition 2024 --check tests/ms03-irq-host-harness.rs tests/ms04-async-rx-host-harness.rs
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --nocapture
cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check
make host-test
cargo test --manifest-path crates/uart_16550/Cargo.toml --offline --features async
cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline
cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib queue::tests
cargo tree --manifest-path crates/axnet/Cargo.toml --offline -e features -i critical-section
cargo tree -p starryos --features qemu --target riscv64gc-unknown-none-elf -e features -i critical-section
cargo check --offline -p starry-kernel --features qemu
openspec validate ms04-qemu-async-rx-queue-baseline --strict
git diff --check
```

Act Response 必须记录新增 test 名称/数量、transition matrix、每个 race/budget case 的明确
decision、定向 rustfmt 退出码、自动命令退出码，以及 host/product critical-section
feature-tree 结论。若未接 T5.2 导致 crate-private seam 仍有 dead-code warning，应如实记录，
但 warning 本身不授权 `allow(dead_code)` 或虚构生产 caller。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Requirements | PASS | approved R2/R4/R6/R7 plus iteration 003 Review follow-up |
| Investigation | PASS | Service privacy、target identity、space race、D4/D5/D7 boundaries inspected |
| Design | PASS | one waiter、locked space recheck、monotonic lifecycle and pure outcomes fixed |
| Task Contracts | PASS | T4.2R-fmt/seam、T5.1a/b have ordered RED/GREEN and stop rules |
| Traceability | PASS | scoped RTM has no Missing/Simplified row |
| Verification | PASS | focused unit/fmt/feature/QEMU compile plus upstream regressions listed |
| Manual boundary | PASS | no QEMU runtime; final user-only manual iteration unchanged |
| Persisted Evidence | PASS | mode none; deterministic short outputs fit Act Response |
| User Approval | BLOCKED | awaiting explicit Gate 2 approval; Act is not authorized |

**Persisted Evidence**

- Mode: none
- Reason: 本轮只有确定性 unit/source/type/fmt/compile/feature-tree 见证，不产生 QEMU
  runtime、长日志或特殊格式 artifact。

**Risks and Notes**

- T5.1 的 start/preflight/fault 只是原子决策，不得为了消除 unused warning 提前 spawn
  task 或切生产 owner。
- `AtomicWaker` 的实现保证 register/wake 并发安全，但本轮仍必须用 generation 双观察
  证明 queue event 语义；只测 AtomicWaker wake count 不足以关闭 D5。
- waiting bit 与 generation 可位于同一 notification state，但职责不同：generation 关闭
  hardware event 窗口，waiting 关闭 Router full 软件唤醒窗口。
- 全工作区 fmt 和 smoltcp warnings 是既有宽范围基线；本轮只验证并修复两个指定
  harness，避免把可诊断 iteration 扩大为无关清理。

## Gate 2 Approval Addendum

- Status: PASS
- Approved: 2026-08-10
- User instruction: “批准”
- Effect: 本追加记录取代上文审批前的 `Status: awaiting-gate-2` 和
  `User Approval: BLOCKED` 快照；Gate 2 全部检查项现为 PASS，iteration 004 可由后续
  明确的 `openspec-act` 请求执行。
- Scope: 批准范围包括 T4.2R 的定向格式、target-bound Service seam、Router-space
  register/recheck 见证，以及 T5.1 lifecycle、generation/register-recheck 和 budget=32
  纯决策层。批准不启动 Act，也不授权 T5.2 named task、真实 queue-control、ISR/probe、
  QEMU runtime 或其他手工测试、Maintainer、Recorder 或归档工作。

## Act Response

- Status: reported

**Implemented**

T4.2R-fmt、T4.2R-seam、T5.1a、T5.1b 全部完成：

- T4.2R-fmt：两处机械 rustfmt（`ms03-irq-host-harness.rs` 的 `fetch_add` 折行、
  `ms04-async-rx-host-harness.rs` 的 import 排序）。RED 时直接
  `rustfmt --edition 2024 --check` 恰好在这两处退出 1；修复后同一命令退出 0，
  `make host-test` 保持 GREEN。
- T4.2R-seam：`Service::rx_one_step_target` 只使用保存的 target index，缺 target
  返回 `Fault(BadState)`；`Service::rx_space_recheck_or_wait` 仅能在 Service guard
  内调用，有空间返回 `Retry`（不发布 waiting），仍 full 才 Release 发布 waiting
  并返回 `Waiting`。原 `RxSpaceSignal` 收敛为新模块 `async_rx.rs` 的 crate-private
  `RxNotify`：单 `AtomicWaker`，Release 发布 waiting，普通 `Service::poll` 保留
  ingress 后 AcqRel clear+wake-once。sibling 模块编译与调用见证由
  `async_rx::tests` 自身承担。
- T5.1a：`RxLifecycle` 单调原子状态机，`Polling→Spawned→Active→Faulted` 与
  `Spawned→Unavailable` 是唯一合法转换；重复 start 返回
  `StartError::AlreadyStarted(current)`，非法转换返回
  `TransitionError::Illegal(current)`，不 panic、不回退。`owner_view`：
  `Active/Faulted→AsyncOwned`，`Polling/Spawned/Unavailable→PollingOwned`。
  load 用 Acquire，CAS 成功 AcqRel、失败观察 Acquire。
- T5.1b：`RxNotify` 增加 `AtomicU64` generation，`publish_event` 执行 wrapping
  `fetch_add(Release)` 后 wake 唯一 waiter，`generation()` 为 Acquire 快照；
  `wait_decision` 按 Acquire generation → register → arm/recheck → Acquire
  generation 顺序，`Pending` 或 generation 变化返回 `Retry`，否则 `Sleep`；
  `RX_BUDGET = 32`；`decide_after_step` 纯决策：进度未到 budget 为 `Continue`，
  恰好 32 且 backlog 为真为 `SelfWakeYield`、为假为 `RegisterRecheck`，`Empty`
  为 `RegisterRecheck`，`Full` 为 `WaitSpace`（不 reap），`Fault` 终态。backlog
  是决策输入，不靠第 33 次 reap 探测。

**Changed Files and Symbols**

| 文件 | 符号 | 变化 |
|---|---|---|
| `crates/axnet/src/async_rx.rs` | `RxNotify`、`RX_NOTIFY`、`SpaceDecision`、`RxTaskLifecycle`、`StartError`、`TransitionError`、`RxLifecycle`、`ArmObservation`、`WaitDecision`、`RX_BUDGET`、`RxDecision`、`decide_after_step`、`SERIAL`(cfg test) | 新增模块：通知状态 + 纯决策层 + 29 个测试 |
| `crates/axnet/src/service.rs` | 删除 `RxSpaceSignal`/`RX_SPACE`；`Service::poll` 改用 `RX_NOTIFY`；新增 `rx_one_step_target`、`rx_space_recheck_or_wait`；4 个 `space_signal_*` 测试随 signal 迁出并改名；`service_poll_wakes_*` 测试加 `SERIAL` 串行化 | seam 收敛 + target 入口 |
| `crates/axnet/src/lib.rs` | `mod async_rx;` | 注册模块 |
| `tests/ms03-irq-host-harness.rs` | `fetch_add` 折行 | 机械 rustfmt |
| `tests/ms04-async-rx-host-harness.rs` | import 排序 | 机械 rustfmt |

**Deviations from Plan**

1. generation publish/snapshot 落在 T5.1b 组而非 T4.2R-seam 组。Plan item 3 描述了
   seam 终态（含 generation snapshot），但 T4.2R-seam 的 GREEN 判据只要求
   register/recheck/Retry/Waiting/wake-once；为保持每组先 RED 后 GREEN，
   `publish_event`/`generation()`/`wait_decision` 与 T5.1b 的 RED 用例同组落地。
   seam 终态与 item 3 一致。
2. `TransitionError::Illegal(RxTaskLifecycle)` 用无名载荷携带当前状态（即 Plan 文字
   中的 `from`）；信息量相同。
3. 4 个原 `service.rs` 的 `space_signal_*` 测试随 signal 收敛迁入 `async_rx.rs` 并
   改名 `notify_*`；`service.rs` 只保留 Service 行为测试。

**Blocker Handoff**

None

**Blocker Resolution**

None

**Self-Review**

- Plan compliance: PASS
- Full diff reviewed: PASS
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 2

已修复发现：实现期 `cargo fmt --check` 在新增 `async_rx.rs` 上退出 1（`send`
签名折行），已做机械修复并复跑 GREEN；mutation 见证后两个文件均已还原并复跑确认。

遗留 Minor：

1. 非 test 构建下 crate-private seam 有 13 项 dead-code warning（`RxLifecycle`、
   `RxNotify` 方法组、`rx_one_step_target`/`rx_space_recheck_or_wait`、
   `decide_after_step`、各决策枚举与 `RX_BUDGET` 等），T5.2 接生产 caller 前按
   Plan 要求如实保留，未加 `allow(dead_code)`，也未虚构生产 caller。
2. `SpaceDecision::Retry` 与 `WaitDecision::Retry` 同名不同层（space handoff 与
   empty-wait 协议）；按 Plan 词汇保留，未自行改名。

**Verification Evidence**

| 验证项 | 命令 | 输出摘录 | 结论 |
|---|---|---|---|
| 定向 fmt（RED） | `rustfmt --edition 2024 --check tests/ms03-irq-host-harness.rs tests/ms04-async-rx-host-harness.rs` | 恰好 `fetch_add` 折行与 import 排序两处 diff | 退出 1，符合 Plan 预期 |
| 定向 fmt（GREEN） | 同上 | 无 diff | 退出 0 |
| axnet lib tests | `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib` | 66 passed; 0 failed | PASS（基线 41 → seam 46 → T5.1a 53 → T5.1b 66） |
| axnet fmt | `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check` | 无 diff | 退出 0 |
| host-test | `make host-test` | 6+8+20+8 passed；`cc -fsyntax-only ms03_irq_probe.c` | 退出 0 |
| UART | `cargo test --manifest-path crates/uart_16550/Cargo.toml --offline --features async` | 62+8+10 passed | 退出 0 |
| axdriver_net | `cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline` | 4 passed | 退出 0 |
| VirtQueue | `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib queue::tests` | 15 passed | 退出 0 |
| host feature tree | `cargo tree --manifest-path crates/axnet/Cargo.toml --offline -e features -i critical-section` | `std` 仅在 `[dev-dependencies]` 下；`restore-state-bool` 存在 | PASS |
| product feature tree | `cargo tree -p starryos --features qemu --target riscv64gc-unknown-none-elf -e features -i critical-section` | `"std"` 出现 0 次；`restore-state-bool` 存在 | PASS |
| kernel compile | `cargo check --offline -p starry-kernel --features qemu` | Finished，仅 dead-code/smoltcp 基线 warnings | 退出 0 |
| OpenSpec | `openspec validate ms04-qemu-async-rx-queue-baseline --strict` | `Change 'ms04-qemu-async-rx-queue-baseline' is valid` | 退出 0 |
| diff check | `git diff --check` | 无输出 | 退出 0 |

Mutation 见证（T5.1b Witness 要求）：

| Mutation | 命令 | 结果 | 处置 |
|---|---|---|---|
| 删除 `wait_decision` 第二次 Acquire generation 观察 | `cargo test ... --lib event_during_register_window` | `event_during_register_window_retries` FAILED（断言 Retry≠Sleep） | 已还原并复跑 GREEN |
| `RX_BUDGET` 32→33 | `cargo test ... --lib budget_exact` | `budget_exact_with_backlog_self_wakes_and_yields`、`budget_exact_without_backlog_goes_register_recheck` FAILED | 已还原并复跑 GREEN |

新增 test 名称/数量：新增 25 个，迁移改名 4 个，全套 66 个。

- seam（5 新增 + 4 迁移）：`sibling_caller_reaches_target_one_step_and_register_seam`、
  `one_step_missing_target_maps_to_bad_state`、`one_step_full_router_buffer_does_not_touch_device`、
  `space_freed_before_waiting_rechecks_to_retry_without_publish`、
  `still_full_publishes_waiting_then_space_wakes_once`；迁移：
  `notify_full_waiting_then_space_wakes_once`、`notify_still_full_does_not_wake`、
  `notify_not_waiting_does_not_wake`、`notify_second_publish_after_clear_wakes_again`。
- lifecycle（7）：`lifecycle_start_moves_polling_to_spawned`、
  `lifecycle_duplicate_start_is_rejected_without_state_change`、
  `lifecycle_preflight_only_from_spawned`、`lifecycle_preflight_outcomes_from_spawned`、
  `lifecycle_fatal_only_from_active`、`lifecycle_terminal_states_accept_no_transition`、
  `lifecycle_owner_view_mapping`。
- generation/budget（13）：`publish_event_increments_generation_and_wakes`、
  `publish_event_generation_wraps`、`event_before_register_is_caught_by_arm_recheck`、
  `event_during_register_window_retries`、`event_after_arm_wakes_sleep_decision`、
  `pending_found_by_arm_recheck_retries_without_event`、`quiescent_arm_without_event_sleeps`、
  `budget_below_limit_continues_on_progress`、`budget_exact_with_backlog_self_wakes_and_yields`、
  `budget_exact_without_backlog_goes_register_recheck`、`empty_goes_register_recheck_at_any_count`、
  `full_goes_wait_space_without_reaping`、`fault_is_terminal_for_the_decision_layer`。

Transition matrix（全部有测试见证，`drive_to` 只经合法转换构造状态）：

| 操作 \ 当前态 | Polling | Spawned | Active | Faulted | Unavailable |
|---|---|---|---|---|---|
| start | → Spawned | Err(AlreadyStarted) | Err(AlreadyStarted) | Err(AlreadyStarted) | Err(AlreadyStarted) |
| preflight(true) | Err(Illegal) | → Active | Err(Illegal) | Err(Illegal) | Err(Illegal) |
| preflight(false) | Err(Illegal) | → Unavailable | Err(Illegal) | Err(Illegal) | Err(Illegal) |
| fatal | Err(Illegal) | Err(Illegal) | → Faulted | Err(Illegal) | Err(Illegal) |
| owner_view | PollingOwned | PollingOwned | AsyncOwned | AsyncOwned | PollingOwned |

Race/budget case 的明确 decision：

| Case | Decision |
|---|---|
| event-before-register（publish 先于 wait，arm 见 Pending） | Retry，不消耗 wake |
| register-during-event（arm 窗口内 publish） | Retry（第二次 Acquire 观察捕获；mutation 敏感） |
| event after arm（Sleep 后 publish） | publish_event wake 一次，task 重检 |
| arm 发现 Pending、无事件 | Retry |
| quiescent arm、无事件 | Sleep，无 wake |
| spurious/empty wake | Sleep（重检不依赖 wake 计数） |
| 进度 1..31（Consumed/Delivered） | Continue |
| 恰好 32 且 backlog | SelfWakeYield |
| 恰好 32 无 backlog | RegisterRecheck |
| Empty（任意计数） | RegisterRecheck |
| Full（任意计数） | WaitSpace，不 reap |
| Fault（任意计数） | Fault，决策层终态 |
| Router 空间在 waiting 发布前释放 | Retry，不发布 waiting |
| 仍 full | Waiting（Release 发布） |
| waiting 后空间释放 | 恰好 wake 一次 |

**Persisted Evidence**

None required。Plan 设定 mode none；本轮输出均为短确定性文本，由本 Response
承载，未创建 `evidence/` 目录。

**Experience Candidates**

None。本轮为标准 TDD 收敛，无可复用操作路径或故障。

**Remaining Issues**

- crate-private seam 的 13 项 dead-code warning 属 T5.2 接线前的预期状态（见
  Self-Review Minor 1）。
- change 内 3.1（D1 target compile）、5.2、6.x、7.x、8.x 按 iteration allocation
  留待后续轮次；本轮未触碰。

**Commit or Diff Reference**

`net-k3` 工作树 diff：`crates/axnet/src/lib.rs`、`crates/axnet/src/service.rs`
修改，`crates/axnet/src/async_rx.rs` 新增，`tests/ms03-irq-host-harness.rs`、
`tests/ms04-async-rx-host-harness.rs` 机械格式化。未创建 commit。

## Plan Review

- Status: follow-up-required

**Review Result**

follow-up-required

**Findings**

Iteration 004 的 lifecycle、owner mapping、Service target seam、space recheck、generation
双观察和 budget=32 决策实现可以保留。独立复验确认单次 axnet 66 tests、定向/axnet
fmt、host/UART/driver/queue 回归、QEMU compile、feature isolation、OpenSpec strict 与
diff check 均通过；未发现产品路径的 Critical correctness 回归。

1. **PASS — 主体决策语义成立。** 状态只沿批准路径单调转换；Active/Faulted 保持
   AsyncOwned；target-bound one-step 不泄漏 raw index；Router full 在 Service 锁内
   recheck 后才发布 waiting；Release/Acquire/AcqRel 角色与设计一致。
2. **IMPORTANT — 全局通知测试缺少统一隔离，确定性声明不成立。**
   `sibling_caller_reaches_target_one_step_and_register_seam` 会注册全局 `RX_NOTIFY`，但未
   获取 `SERIAL`；两个 space seam tests 和 Service 集成测试则依赖同一 static 中当前
   waker。16 线程重复运行时已复现 waker 被覆盖，
   `still_full_publishes_waiting_then_space_wakes_once` 观察到 wake count 0 而失败。
3. **IMPORTANT — arm/recheck error 没有进入纯决策层。**
   `NetQueueControl::arm_rx_notify_and_check` 返回 `DevResult<bool>`，D9 把 queue-control
   error 定义为 fatal；当前 `ArmObservation` 只有 Pending/Quiescent，`WaitDecision` 只有
   Retry/Sleep。T5.2 直接接线时只能旁路 `wait_decision`、用 side channel 保存 error，
   或错误地睡眠，均不满足“所有交错有 service/self-wake/sleep/fault 明确结果”。
4. **PASS — 报告中的预期 dead-code 不是本轮缺陷。** fresh QEMU compile 报告 17 个
   warning group，均来自尚未接 T5.2 caller 的 seams 或既有 smoltcp/virtio baseline；
   没有 `allow(dead_code)` 或虚构 caller。T5.2 正常接线后再按实际剩余项判断。

**Deviation Classification**

- `ACT-DEVIATION`：计划要求 tests 用确定性隔离见证交错，但一个写全局 `RX_NOTIFY` 的
  sibling test 未使用共享串行 guard，fresh 并发压力可复现失败。
- `PLAN-OMISSION`：Iteration 004 把实际 queue-control call 留给 T5.2，却没有让
  `ArmObservation/WaitDecision` 承载 `DevResult` error；该遗漏与 D9 fatal 语义冲突。
- 其余实现未发现 `PLAN-INVALID`、基线变化或 Critical finding。

**Evidence**

2026-08-11 独立复验：

| Command / inspection | Result |
|---|---|
| `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --nocapture` | PASS：66 tests，exit 0 |
| 同一 axnet suite，`--test-threads=16` 重复最多 100 次 | FAIL：首轮即复现 65 pass / 1 fail；`still_full_publishes_waiting_then_space_wakes_once` 期望 1、实际 0，exit 1 |
| direct harness rustfmt；axnet fmt | PASS，exit 0 |
| `make host-test` | PASS：6 + 8 + 20 + 8，exit 0 |
| UART async tests | PASS：62 + 8 + 10，exit 0 |
| axdriver_net；VirtQueue | PASS：4；15 tests，exit 0 |
| `cargo check --offline -p starry-kernel --features qemu` | PASS，exit 0；仅已记录 dead-code/baseline warnings |
| host/product critical-section feature trees | PASS：`std` 仅 dev tree；产品保留 restore-state-bool |
| `openspec validate ... --strict`; `git diff --check` | PASS，exit 0 |
| `async_rx.rs` source inspection | global `RX_NOTIFY` uses at lines 422/452/468；只有后两处持 `SERIAL`；wait enums cannot carry DevError |
| `axdriver_net::NetQueueControl` inspection | `arm_rx_notify_and_check(&mut self) -> DevResult<bool>`；suppress also returns DevResult |

Persisted Evidence 模式为 none；没有 Evidence 目录不构成问题。

**Follow-up Decision**

创建 iteration 005，把测试隔离和 arm-error 小修复合入原定 T5.2。按 T5.1R→T5.2a
transport-neutral Device/Service queue-control seam→T5.2b unique future/named task 三段执行，
每段有独立 RED/GREEN 与停止条件。为避免半接线，本轮提供 axnet start entry 但不从
kernel 调用；T6.1 在 ISR publish/wake 就绪后再接生产 caller。QEMU runtime 与 sandbox
复跑继续保留给最终 user-only iteration。

**Next Iteration**

`iterations/005-review-closures-and-unique-rx-task.md`，等待 Gate 2 批准。
