# Iteration 006: Fatal Publication and Parallel Gate Closure

## Plan Context

- Status: ready
- Round: 006
- Parent: Iteration 005

**Objective**

关闭Iteration 005 Review留下的两个event correctness前置：fatal lifecycle必须先提交`Faulted`，
再发布generation并唤醒stack role；所有触碰生产态共享event或telemetry的host tests必须在默认
并行runner下互不污染。完成后，Iteration 007的flush/V3工作可依赖稳定的terminal wake与full
test Gate。

**Background**

Iteration 005完成了slot/raw owner分离、ticket mismatch fail-stop、TX/space事件、Again等待和
deferred ARP边界。Plan Review从实际diff和fresh tests发现：两条fatal路径的publish顺序与已
批准Task 3.5相反；新增deferred ARP Service tests未纳入共享`QUEUE_EVENT`/`RX_TELEMETRY`隔离，
默认并行full suite重复10次失败5次。这不是新需求，也不改变D3-D6，只修复Act偏差。

**Current Baseline**

- Branch: `net-k3`
- HEAD: `244803fb`
- Worktree: modified；Iteration 005的7个axnet文件和OpenSpec文档尚未commit。
- Change progress: 15/24 tasks；Task 3.7未开始，Tasks 4.1-6.3待后续轮。
- `poll_active()`与`poll_register_recheck()`均为`publish_progress()`后
  `transition_fatal()`；lifecycle CAS使用AcqRel，观察使用Acquire。
- `QUEUE_EVENT`、`RX_TELEMETRY`是生产态全局；测试隔离锁为cfg(test) `SERIAL`。
- Persisted Evidence: Iteration 005为`none`，本轮不要求历史Evidence目录。

Fresh Review baseline：

| Command | Exit | Result |
|---|---:|---|
| `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib` | 101 | 183 passed，1 shared-event test失败 |
| 默认并行full命令重复10次 | 1 | 5 PASS / 5 FAIL；同一wake断言失败 |
| 失败单项重复20次 | 0 | 20/20 PASS |
| full命令追加`-- --test-threads=1` | 0 | 184 passed |
| `git diff --cached --check` | 0 | 无whitespace错误 |
| `openspec validate ms05-qemu-bounded-bidirectional-device-data-plane --strict` | 0 | Change is valid |

**Current-State Evidence**

- `RxRxFuture::poll_active()`在持有`Service` guard时取得`RoundOutcome::Fault`，随后先
  `notify.publish_progress()`、再`lifecycle.fatal()`。stack waker可同步执行，观察到旧`Active`。
- `poll_register_recheck()`的arm fault不持有guard，但采用同样的publish→transition顺序。
- `fatal_wakes_stack_progress`只在整个future poll结束后断言wake count和最终lifecycle，无法观察
  wake callback当场读取到的状态。
- `future_rx_slot_full_waits_then_service_poll_wakes`持有`SERIAL`并依赖全局waiting bit；新增两个
  deferred ARP tests不持锁却调用`Service::poll()`，其`wake_if_space(true)`可并发清除此bit。
- retry test还读取全局`RX_TELEMETRY.non_ip_consumed`的delta；并行Service tests可改变该值。
- 单线程与单项稳定通过、默认并行高频失败，把故障层定位为test isolation；fatal顺序则是独立的
  产品lost-wake窗口，不能以串行测试修复。

**Relevant Code**

| File / Symbol | Current Responsibility | Iteration Use |
|---|---|---|
| `crates/axnet/src/async_rx.rs::RxLifecycle::fatal` | AcqRel `Active→Faulted`唯一提交点 | 固定成功提交与publication顺序 |
| `async_rx.rs::poll_active/poll_register_recheck` | terminal fault处理 | commit后精确发布一次stack progress |
| `async_rx.rs::QueueEvent::publish_progress` | Release generation + stack-role wake | 保持角色与ordering，不改变API |
| `async_rx.rs` tests | lifecycle/event/future见证和`SERIAL`定义 | 增加wake-time状态与非法transition tests |
| `device/tests.rs`、`service.rs` tests | 真实`Service::poll`集成见证 | 补齐共享event/telemetry隔离 |

**Critical Path**

```text
queue/arm fault
  → record stable fault telemetry
  → AcqRel Active→Faulted succeeds
  → Release generation
  → wake stack role
  → wake callback / stack waiter Acquire observes Faulted

parallel host tests
  → every production-global Service/event/telemetry test enters SERIAL
  → arrange local state
  → execute and assert
  → restore shared waiting/waker state before releasing SERIAL
```

**Implementation Guidance**

1. 先写wake callback在`wake`/`wake_by_ref`内立即Acquire读取lifecycle的RED test，分别覆盖
   service-round fault与arm/recheck fault；旧实现必须观察到`Active`或因顺序断言失败。
2. 把fatal处理收敛为“尝试transition；成功才publish”的明确顺序。非法transition继续记录
   `LIFECYCLE`诊断，但不得发布并不存在的新terminal状态；不要添加第二次wake补偿错误顺序。
3. 审计所有调用生产态`Service::poll()`、直接操作`QUEUE_EVENT`或比较`RX_TELEMETRY` delta的tests。
   仅这些tests共享`SERIAL`；使用局部`QueueEvent`的纯模型tests保持并行。
4. 每个全局event test在持锁后建立所需waiting/waker状态，并在释放锁前恢复quiescent状态。
   不依赖测试执行顺序或前一test清理。
5. RED/GREEN后先跑定向tests，再以默认并行full命令重复100次。`--test-threads=1`只作根因对照，
   不能作为GREEN验收。

**Behavioral Change**

- terminal stack wake从“可能观察Active”变为“wake发生时已可Acquire观察Faulted”。
- 非法fatal transition不产生伪stack progress publication。
- 产品接口、event role、generation类型和V1/V2 ABI不变。
- host full suite在默认并行runner下从可重复flaky变为100次零失败。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| 3.7 | R9/R10；fatal progress与确定性Gate | `async_rx::{poll_active,poll_register_recheck,RxLifecycle}`及相关tests | publish早于state commit；最终状态test不见证wake-time状态 | successful commit→publish；wake-time断言 |
| 3.7 | R14；默认并行host回归 | `async_rx.rs`、`device/tests.rs`、`service.rs` tests | 部分生产全局tests未统一隔离 | 精确共享`SERIAL`边界和独立初始化/清理 |

**Task Contracts**

### Task 3.7 — Close fatal publication ordering and parallel test isolation

- Depends on: Iteration 005；blocks Tasks 4.1-4.3。
- RED: 两条fatal路径的wake callback当场观察到非Faulted；默认并行axnet full suite在10次内出现
  waiting wake count 0；单项和单线程对照通过。
- GREEN: successful lifecycle CAS先于Release generation和stack wake；两条路径的wake-time
  观察均为Faulted且各发布一次；非法transition只记录诊断；所有生产全局tests有统一隔离，默认
  并行full suite与定向event/Service filters各重复100次零失败。
- Must modify: `async_rx.rs` fatal branches/tests；必要的`device/tests.rs`、`service.rs`测试锁边界。
- Must not modify: product `Service::poll`行为、waker角色数量、ISR、slot/ticket owner、V1/V2 ABI、
  flush/V3/QEMU control；不得全局强制单线程或加入sleep。
- Verify: wake-time RED/GREEN、默认并行full 100×、定向tests 100×、axnet full、rustfmt、strict
  OpenSpec和diff check。
- Stop: lifecycle commit失败时仍需要发布success wake，或不能在不改变产品event API的情况下隔离
  tests；保留最小复现并返回Plan。

**Invariants**

- Active/Faulted都保持AsyncOwned；fatal不恢复Polling owner。
- state先提交，event后发布；generation Release与lifecycle Acquire/AcqRel理由保持明确。
- queue-owner与stack-progress仍使用不同`AtomicWaker`；ISR仍只发布通用event。
- guard不跨`Pending`，raw queue唯一owner、slot/ticket ledger、deferred ARP语义保持Iteration 005结果。
- 测试隔离代码只在cfg(test)可见，不进入产品ABI或调度路径。

**Non-goals**

- 不实现C4 flush、V3 snapshot、QEMU diagnostics controls或probe。
- 不修改socket readiness、reset/cancel、SMP、真板或性能语义。
- 不运行手工QEMU，不创建Evidence、Runbook、Incident或全局M/D/K/R/I条目。
- 不修复change外smoltcp warnings或lichee-d1基线错误。

**Acceptance**

| Requirement | Scenario | Design | Task | Code/Test | Simplification | Status |
|---|---|---|---|---|---|---|
| R9/R10 | fatal stack progress不丢失 | D4/D5 | 3.7 | 两条fault路径的wake-time lifecycle witness | None | Covered |
| R9/R10 | 非法transition不伪造progress | D4/D5 | 3.7 | illegal fatal publication count test | None | Covered |
| R14 | 默认并行host Gate确定性 | D10 | 3.7 | global-state isolation audit + 100× full runner | None | Covered |

没有Missing或Simplified requirement。原Tasks 4.1-6.3分别由Iterations 007-009覆盖。

**Verification**

Act必须记录RED/GREEN命令、关键输出、退出码、修改文件/符号和full diff Review。至少执行：

```text
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib fatal -- --nocapture
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib service_poll -- --nocapture
repeat both targeted filters 100 times with zero failures
repeat `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib --quiet` 100 times with zero failures
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --test-threads=1
rustfmt --check --edition 2024 crates/axnet/src/async_rx.rs crates/axnet/src/device/tests.rs crates/axnet/src/service.rs
openspec validate ms05-qemu-bounded-bidirectional-device-data-plane --strict
git diff --check -- crates/axnet openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane
```

增加source/behavior guard：两条terminal路径都不能在successful `transition_fatal()`之前调用
`publish_progress()`；wake-time observer必须在callback内采样，不能只在future返回后检查。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | 已定位两条fatal调用边、CAS ordering、共享event/telemetry producers与并行失败条件 |
| Design | PASS | 固定successful commit→publish、illegal transition不publish和精确test isolation；无接口选择 |
| Task Contracts | PASS | Task 3.7含RED/GREEN、目标符号、禁止项、重复Gate与stop条件 |
| Traceability | PASS | R9/R10/R14→D4/D5/D10→3.7→fatal/tests链路完整，无Missing/Simplified |
| Verification | PASS | wake-time见证证明产品顺序，默认并行100×证明Gate稳定；单线程仅作对照 |

技术Gate 2全部PASS。用户已要求重大问题先形成当前修复轮；执行仍需在本Plan Review审计通过后
显式调用`openspec-act`。

**Persisted Evidence**

- Mode: none

本轮结果由Act Response保存；不要创建`evidence/006-*`目录。

**Risks and Notes**

- Rust `Wake::wake`可同步执行，不能假设woken task一定晚于当前函数返回；这正是state-first的依据。
- `SERIAL`只解决host tests共享生产全局的隔离，不是产品同步机制，也不能掩盖产品竞态。
- 100×默认并行full suite是本轮Gate；仅报告单次PASS或`--test-threads=1` PASS不满足验收。
- Gate 1沿用已批准MS05 requirements；没有新增用户决策或风险豁免。

**Deferred Tasks**

- Iteration 007: Tasks 4.1-4.3，ticketed C4 flush、V3 snapshot、QEMU-only controls。
- Iteration 008: Tasks 5.1-5.2，probe与自动产品Gate，required Evidence
  `evidence/008-probe-and-automatic-product-gates/`。
- Iteration 009: Tasks 6.1-6.3，普通终端环境复跑、手工QEMU runtime和最终Review，required
  Evidence `evidence/009-independent-manual-qemu-runtime-and-closeout/`。

## Act Response

- Status: reported

**Implemented**

Task 3.7 完整实施，关闭 Iteration 005 Review 遗留的两个 event correctness 前置：

1. **fatal publication 顺序修复**：`RxRxFuture` 新增 `publish_fatal()` seam，作为唯一的
   commit→publish 路径——先执行 `transition_fatal()`（`Active→Faulted` AcqRel CAS），
   仅在 CAS 成功后才 `publish_progress()`（Release generation + stack-role wake）。
   `poll_active()` 的 service-round fault 分支与 `poll_register_recheck()` 的 arm fault
   分支均改为调用 `self.publish_fatal()`，不再直接 publish。
2. **非法 transition 语义**：`transition_fatal()` 从 `()` 改为返回 `bool`；非法 transition
   （lifecycle 非 Active，如已 terminal）仍记录 LIFECYCLE-stage 诊断、不改变状态、不递增
   fault 计数，且 `publish_fatal()` 因返回 false 而不再发布任何伪 terminal stack wake。
3. **并行测试隔离**：`device/tests.rs` 的 `service_poll_deferred_arp_attempts_once_and_stops_round`
   与 `service_poll_deferred_arp_retries_once_after_tx_space` 补上 `SERIAL.lock()`，与既有
   持锁测试共享同一隔离边界，避免并发 `Service::poll()` 清除共享 `QUEUE_EVENT.waiting` 或
   扰动 `RX_TELEMETRY.non_ip_consumed` delta。
4. **wake-time 见证**：新增 `LifecycleObservingWake` 测试 waker，在 `wake`/`wake_by_ref`
   callback 内当场采样 lifecycle（非 future 返回后）；两条 fatal 路径各有测试断言
   wake-time 观察到的已是 `Faulted`。

**Changed Files and Symbols**

- `crates/axnet/src/async_rx.rs`
  - `RxRxFuture::publish_fatal`（新增）：commit-then-publish 唯一 seam，CAS 成功才发布。
  - `RxRxFuture::transition_fatal`：返回 `bool`，非法 transition 记录 LIFECYCLE 诊断。
  - `RxRxFuture::poll_active`（`RoundOutcome::Fault` 分支）：改用 `publish_fatal()`。
  - `RxRxFuture::poll_register_recheck`（`WaitDecision::Fault` 分支）：改用 `publish_fatal()`。
  - tests：`LifecycleObservingWake`、`lifecycle_observing_waker`、
    `fatal_service_round_wake_observes_faulted_lifecycle`、
    `fatal_arm_recheck_wake_observes_faulted_lifecycle`、
    `illegal_fatal_transition_publishes_no_progress`、
    `fatal_paths_commit_before_publish_in_source`；更新 `telemetry_illegal_fatal_records_lifecycle_stage`
    适配 `bool` 返回值。
- `crates/axnet/src/device/tests.rs`
  - `service_poll_deferred_arp_attempts_once_and_stops_round`：补 `SERIAL` 隔离。
  - `service_poll_deferred_arp_retries_once_after_tx_space`：补 `SERIAL` 隔离。
- `openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane/tasks.md`：Task 3.7 标记完成。

**Deviations from Plan**

None。实现与 Plan Task Contracts、Invariants、Non-goals 一致；`transition_fatal` 返回类型
从 Plan 隐含的无返回改为 `bool`，属于任务契约内（"successful CAS 才 publish"需要结果判断），
未改变产品接口语义。

**Blocker Handoff**

None

**Blocker Resolution**

None

**Self-Review**

- Plan compliance: PASS
- Full diff reviewed: PASS
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 0

Spec compliance：两条 fatal 路径 wake-time 观察均为 `Faulted` 且各发布一次；非法 transition
只记录 LIFECYCLE 诊断、不发布伪进展；所有生产全局 tests（调用 `Service::poll`、读写
`QUEUE_EVENT`、比较 `RX_TELEMETRY` delta）共享 `SERIAL` 边界；`Must not modify` 项（产品
`Service::poll` 行为、waker 角色数量、ISR、slot/ticket owner、V1/V2 ABI、flush/V3/QEMU
control）均未触碰；未全局强制单线程、未加入 sleep/retry、未弱化 wake 断言。
Code quality：diff 仅限 2 个 axnet 文件 + change 文档，无计划外修改；`publish_fatal` 收敛
两条路径的 commit→publish 顺序，消除重复；axnet 自身无新增 warning；测试通过原因正确
（wake-time observer 在 callback 内采样，`woken==1` 与观察值断言同时成立）。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| RED（wake-time） | `cargo test -p axnet --lib fatal -- --nocapture`（修复前） | `fatal_service_round_wake... left: Active, right: Faulted`；`fatal_arm_recheck... left: Active` | RED 确认：旧实现 wake 观察到 Active |
| 定向 fatal tests | `cargo test -p axnet --lib fatal`（修复后） | `test result: ok. 7 passed; 0 failed` | PASS |
| 定向 fatal ×100 | 重复 100 次 `--lib fatal --quiet` | `fatal filter 100x: 0 failures`（最终代码态） | PASS |
| 定向 service_poll ×100 | 重复 100 次 `--lib service_poll --quiet` | `service_poll filter 100x: 0 failures`（最终代码态） | PASS |
| 默认并行 full suite ×100 | 重复 100 次 `--lib --quiet`（默认 runner） | `default-parallel full suite 100x: 0 failures`（最终代码态） | PASS |
| 单线程对照 | `--lib -- --test-threads=1` | `188 passed; 0 failed` | PASS（仅对照，非验收） |
| rustfmt | `rustfmt --check --edition 2024`（3 个 axnet 文件） | exit 0 | PASS |
| OpenSpec validate | `openspec validate ms05-... --strict` | `Change 'ms05-...' is valid`；exit 0 | PASS |
| diff check | `git diff --check -- crates/axnet openspec/changes/...` | exit 0，无 whitespace 错误 | PASS |

**Persisted Evidence**

None required（Plan Persisted Evidence mode: `none`，验证摘要由本 Act Response 承载，未创建
`evidence/006-*` 目录）。

**Experience Candidates**

None

**Remaining Issues**

None。Task 3.7 完成；Tasks 4.1-4.3（Iteration 007）、5.1-5.2（008）、6.1-6.3（009）待后续轮次。

**Commit or Diff Reference**

工作树未提交；本轮 diff = `crates/axnet/src/async_rx.rs` + `crates/axnet/src/device/tests.rs`
（未暂存部分）。HEAD 仍为 `244803fb`。

## Plan Review

- Status: pending

**Review Result**

Pending.

**Findings**

Pending.

**Deviation Classification**

Pending.

**Evidence**

Pending.

**Follow-up Decision**

Pending.

**Next Iteration**

Pending.
