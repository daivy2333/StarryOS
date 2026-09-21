# Iteration 002 / Cycle 001: Close network observation and UART park safety

## Plan Context

- Status: ready
- Iteration: 002-deterministic-network-placement-and-observability
- Cycle: 001-replan
- Cycle Type: replan
- Parent cycle: `000-initial.md`

**Iteration Scope**

- Change tasks: 3.1–3.6
- Depends on: Iteration 001
- Stable baseline: network owner/runner start after secondary-ready with deterministic singleton affinity; V5 contains direct task-hart observations; the bounded timer-disabled witness restores the target timer on every terminal path; UART TX park cannot lose a producer wake.
- Verification boundary: network lifecycle/fallback, V5 ABI and real telemetry, witness cleanup, UART park interleavings, affected regressions, and repeated bounded `SMP=16` startup.
- Diagnostic boundary: affinity lifecycle, runner/owner startup, V5 sources, timer-witness state/cleanup, or UART TX ring register/recheck/THRE park protocol.
- Deferred tasks: 4.1–7.3

**Cycle Scope**

- Trigger: replan-required
- Acceptance gaps: A2–A5 from Cycle 000 plus newly authorized A6 UART TX copier lost-wakeup.
- Repair items: None
- Inherited scope: Task 3.1 placement policy/adapter scaffolding; accepted Iterations 000–001 behavior; existing V1–V4, SPSC, queue-owner, recovery, early-console, and D1 contracts.
- Excluded scope: UART redesign beyond the TX park lost-wakeup, migration, guest protocol, full data-plane qualification, performance work, and physical-board claims.

**Objective**

Make the network placement baseline safe on every startup/failure path, replace inferred V5 task placement with direct observation, complete the timer-disabled witness safety protocol, and close the UART TX park race that makes the shared `SMP=16` startup Gate nondeterministic.

**Background**

Cycle 000 established useful placement, V5, and witness scaffolding, but Review found that schedulable-affinity spawn failure can poison lifecycle state, V5 fabricates task harts from affinity, and the witness binding lacks the approved single-flight and timeout cleanup. Act also captured a separate UART TX park race: the same binary produced both a zero-IPI `tx-copier-not-resumed` failure and a PASS. The user explicitly authorized repairing that Issue candidate in this Iteration, requiring a replan rather than reopening Cycle 000.

**Investigation Facts**

- Current Baseline:
  - Task 3.1's shared policy consumption and singleton placement scaffolding are reusable.
  - Reported host suites, axnet suites, UART suites, builds, and network placement smoke pass for the paths they cover; their conclusions remain usable until the corresponding code surface changes.
  - Cycle 000 Review is `replan-required`; Act must use this Cycle only.
- Current-State Evidence:
  - `start_stack_runner_affinity` and `start_rx_task_affinity` advance their once-only lifecycle before `spawn_with_name_affinity` can reject a configured-but-unpublished mask.
  - `init_virtio_net_irq_diag` continues after runner-start failure, and device/MMIO early returns occur before runner startup, regressing the previous always-present Service runner.
  - `net_placement::snapshot` assigns task last-hart from pinned affinity. Owner/runner poll sites increment counts but never record `this_cpu_id()`.
  - V5 embeds V4 at offset zero but current tests do not prove the complete fixed layout or byte-for-byte prefix; witness state is absent.
  - Witness `START` spawns before reserving state; target-side `try_start(...).ok()` ignores Busy; trigger/cancel flags survive runs; there is no bounded supervisor; terminal-before-restore is accepted.
  - UART `tx_copier_loop` on an empty ring registers the ring waker, publishes inactive, and returns `Pending` without a post-registration ring recheck. If the transmitter is already empty, it also leaves THRE disabled. A producer wake while the task is still Running does not create `Blocked → Ready`, so the subsequent park can strand bytes.
- Code and Critical Path:
  - Network start: `axruntime init_network → kernel entry → placement → runner start → IRQ register → owner start`.
  - Network observation: owner/runner future poll entry → hart counter → V5 assembly/ioctl.
  - Witness: ioctl reserve → target spawn/arm/park → distinct-hart trigger or supervisor cancel → target restore acknowledgment → terminal publication.
  - UART park: TX ring empty check → ring waker registration → ring recheck/THRE fallback → inactive publication → `Pending`; producer push must lead to an already registered wake or an explicit retry.

**Implementation Guidance**

Close failure atomicity before adding more telemetry. Validate against the published schedulable set before lifecycle CAS, or introduce an injected spawn transaction whose failed spawn safely restores the pre-start lifecycle without permitting duplicates. A missing runner is a hard stop for owner startup; no-device and device-validation failures must still leave the Service runner available when networking/loopback is installed.

Record owner and runner harts at their real future poll entries with coherent `(last, mask, events)` counters. Keep affinity and execution fields separate. Extend V5 once, before qualification, with exact offsets and initialized witness fields.

For the witness, reserve state synchronously in `START`, reset all per-run flags, and make illegal transitions return errors without changing state. A supervisor on a different timer-enabled hart owns the bounded deadline and requests cancellation; terminal publication is impossible until the target acknowledges timer restoration. Spawn failure must release the reservation.

For UART, use the established event/register/recheck pattern: after registering the TX ring waker, recheck ring non-empty before committing to park and self-wake/retry if work appeared. Keep THRE enabled as the hardware fallback on the empty-transmitter park edge without creating busy polling or a second consumer.

**Behavioral Change**

- Invalid/unpublished affinity and spawn failure do not consume the unique runner/owner lifecycle. Owner startup cannot proceed without a valid saved runner.
- Service/loopback progress retains a pinned runner even when the NIC descriptor, MMIO header, or IRQ registration prevents async owner startup.
- V5 distinguishes configured affinity from directly recorded IRQ/owner/runner execution tuples and exposes bounded witness state; V1–V4 remain unchanged.
- Witness runs are single-flight, per-run state is reset, trigger is remote and phase-valid, deadline/cancel paths restore the target timer before terminal publication, and spawn failure releases the run.
- UART TX producer publication cannot be lost across copier park; new data after registration forces retry, and THRE remains a bounded hardware fallback.

**Task Contracts**

### 3.2: Make pinned network startup failure-atomic

- Requirement/Scenario: unique owner/runner, secondary-ready placement, fallback.
- Depends on: Task 3.1
- Targets: axnet runner/owner lifecycle and affinity start seams; kernel network startup adapter.
- Current behavior: lifecycle advances before schedulable-aware spawn; missing runner does not stop owner; early device validation can leave Service without a runner.
- Required behavior: invalid/unpublished mask and failed spawn preserve a retryable pre-start lifecycle; exactly one successful spawn commits it. Runner availability is required before IRQ/owner startup. Device/MMIO/IRQ failure leaves the pinned runner active and the owner unstarted.
- Preserve: Service-first install, register-before-owner, polling fallback, one owner/runner, recovery lifecycle.
- Forbidden: lifecycle rollback after a task may have enqueued, detached duplicate roles, or full-mask fallback.
- Test witness: injected spawn success/failure and concurrent-start model tests; device-missing/MMIO-invalid/IRQ-fail startup tests; source-only ordering checks are insufficient.
- GREEN condition: every pre-enqueue failure is retryable and creates zero tasks; one success creates one saved handle; all later starts fail without duplicates.
- Verification: focused axnet/model tests, both axnet suites, host harness, ordinary/SMP builds.
- Stop when: failure atomicity cannot be proven without changing public lifecycle semantics; return to Plan.

### 3.3: Record real network task harts and finalize V5

- Requirement/Scenario: direct placement/wake observation and ABI compatibility.
- Depends on: Task 3.2
- Targets: owner/runner future poll entries, coherent counters, V5 type/assembly/ioctl, ABI tests.
- Current behavior: task last/mask is inferred from affinity and witness state is absent.
- Required behavior: each real poll records `this_cpu_id()` into coherent `(last, mask, events)` telemetry; V5 carries separate affinity and actual tuples plus the approved IPI/wake/resume and witness fields. V4 is an exact byte prefix; every V5 byte and offset is defined.
- Preserve: Relaxed pure telemetry, scheduling independence, V1–V4 command/layout/meaning.
- Forbidden: affinity-as-execution evidence, revision/run identity, or telemetry-controlled behavior.
- Test witness: concurrent tuple tests, real future-poll fixtures, exact offset/size table, byte-prefix comparison, initialized reserved/witness fields.
- GREEN condition: actual tuple changes only when the future polls on that hart and all ABI tests pass.
- Verification: host ABI/model, axnet diagnostics, MS03/MS04/MS07 regression, target build.
- Stop when: a required field lacks a direct attributable source; return to Plan.

### 3.4: Complete the bounded timer-disabled witness

- Requirement/Scenario: event-driven remote ready without timer rescue; cancel/timeout cleanup.
- Depends on: Tasks 3.2–3.3
- Targets: pure witness machine, QEMU binding/control, supervisor, V5 fields, focused tests.
- Current behavior: START races target reservation; flags persist; trigger may be local/early; no supervisor exists; terminal can publish without restore acknowledgment.
- Required behavior: START reserves single-flight state before spawn and rolls back spawn failure; each run resets flags; only Armed accepts a trigger from a non-target hart; a timer-enabled remote supervisor enforces the fixed deadline and requests cancel; target always restores its timer and acknowledges cleanup before Completed/Failed becomes visible. Illegal or duplicate transitions do not mutate the run.
- Preserve: single S_SOFT owner, NIC ownership isolation, bounded execution, QEMU-only control.
- Forbidden: unbounded wait/spin, all-hart timer disable, terminal-before-restore, or deferring safety mechanics to Iteration 005.
- Test witness: transition table, concurrent START, spawn failure, early/local trigger, success, cancel, timeout, stale flags, duplicate terminal, and restore failure models; target compile and source symmetry.
- GREEN condition: every accepted run reaches one cleanup-acknowledged terminal or remains under an active bounded supervisor; timer-disabled orphan is unreachable.
- Verification: focused model/binding tests, host harness, QEMU build; full workload qualification remains deferred.
- Stop when: target timer restoration cannot be acknowledged after timeout; return to Plan.

### 3.6: Close UART TX copier park lost-wakeup

- Requirement/Scenario: UART cross-hart TX readiness and event/register/recheck; Issue candidate authorized by the user for this Iteration.
- Depends on: accepted UART singleton copier and local-preferred scheduler wake.
- Targets: `crates/uart_16550/src/async_/driver.rs::tx_copier_loop`; real copier future witnesses; MS08 startup harness.
- Current behavior: empty-ring path registers the ring waker, publishes inactive, and returns Pending without rechecking the ring. A producer wake while the task remains Running can be consumed before block; with an already-empty transmitter, THRE is not enabled to provide a later wake.
- Required behavior: after ring-waker registration, recheck ring availability before committing to park; if data appeared, self-wake or retry without losing SPSC ownership. The empty-transmitter park path leaves a bounded THRE wake fallback. Active/inactive and four-stage completion remain correctly ordered.
- Preserve: one TX copier/consumer, short-write behavior, no busy loop, D1 slow-poll/yield workaround, QEMU early console, TXDBG/snapshot ABI, and `tcdrain` semantics.
- Forbidden: second copier, unbounded polling, producer-side scheduler special case, or treating periodic timer as the fix.
- Test witness: deterministic real-future interleaving injects a push after register but before Pending and must observe a subsequent poll/drain; THRE empty-transmitter fallback test; existing Full→recovery/drain tests remain green. RED must fail on the current implementation.
- GREEN condition: no payload remains stranded in the ring for the controlled interleaving; wake/progress occurs without timer rescue; repeated bounded `SMP=16` startup has zero `tx-copier-not-resumed` failures.
- Verification: UART focused/async suites, MS04/MS08 harness, ordinary/SMP/D1 builds, repeated bounded QEMU startup.
- Stop when: the repair changes SPSC ownership, requires a second producer/consumer, or regresses D1 completion; return to Plan.

### 3.5: Re-run the expanded Iteration integration Gate

- Requirement/Scenario: Tasks 3.2–3.4 and 3.6 together satisfy the revised stable baseline.
- Depends on: Tasks 3.2–3.4, 3.6
- Targets: affected unit/host suites, builds, repeated bounded startup.
- Current behavior: prior PASS results do not cover lifecycle failure, real task-hart telemetry, witness cleanup, or deterministic UART park interleaving.
- Required behavior: all new focused witnesses pass, predecessor regressions remain green, and repeated `SMP=16` startup reports both network and UART smoke PASS without timer orphan, duplicate role, or stranded TX byte.
- Preserve: Evidence mode `none`; no identity-style run protocol.
- Forbidden: accepting one lucky boot, replacing direct witnesses with source guards, or claiming full data-plane qualification.
- Test witness: the expanded Gate itself with decisive per-run behavior output.
- GREEN condition: all focused and regression checks pass; repeated startup count and failures are reported in Act Response.
- Verification: task-specific suites → host integration → ordinary/SMP/D1 builds → repeated bounded QEMU → diff/strict OpenSpec.
- Stop when: any product failure persists; do not enter Iteration 003.

**Invariants**

- Exactly one network owner, one network runner, one UART TX copier, and one S_SOFT owner exist.
- Failed startup before enqueue does not consume lifecycle; successful enqueue cannot be rolled back.
- Actual execution telemetry is recorded at execution, never inferred from affinity.
- Target timer restoration precedes witness terminal publication.
- UART register/recheck closes the lost-edge window without busy polling; SPSC and drain semantics remain intact.
- QEMU evidence is not physical-board evidence.

**Non-goals**

- Other UART refactors, UART migration, full UART/network qualification, combined pressure, or recovery interleave.
- CPU hotplug, multiqueue/RSS, physical timer/IRQ claims, or performance optimization.

**Requirements Traceability Matrix**

| Requirement / scenario | Design | Task | Code surface | Test witness | Status |
|---|---|---|---|---|---|
| unique failure-atomic network startup | D4–D5 | 3.2 | axnet lifecycle + kernel adapter | injected spawn/fallback models | Covered |
| direct V5 placement/wake observation | D6 | 3.3 | poll counters + V5 | real poll tuple + ABI bytes | Covered |
| timer-disabled remote wake cleanup | D7 | 3.4 | witness state/binding | transition and cleanup models | Covered |
| UART event/register/recheck | UART readiness requirement | 3.6 | TX copier park path | deterministic real-future race | Covered |
| revised integration Gate | D9 | 3.5 | suites/build/startup | repeated bounded boots | Covered |

**Acceptance**

- A2: network affinity startup is failure-atomic; a valid runner exists on every post-Service path, and owner starts only after IRQ success.
- A3: V5 contains directly recorded owner/runner task-hart tuples and complete witness observations while preserving V1–V4.
- A4: every accepted witness run is single-flight, bounded, remotely triggered, and cleanup-acknowledged before terminal state.
- A6: UART TX publication cannot be lost across the copier park transition; register/recheck and THRE fallback preserve progress without timer rescue.
- A5: focused tests and repeated `SMP=16` startup prove the combined revised baseline; no full data-plane claim is made.

**Verification**

- New RED/GREEN witnesses for lifecycle spawn failure, real task-hart counters, witness transition/cleanup, and UART register/recheck interleaving.
- Axnet ordinary and qemu-diagnostics suites; UART async/focused suites.
- MS03/MS04/MS07/MS08 and early-console regressions.
- Ordinary, `SMP=16`, and D1 builds.
- Repeated bounded QEMU `SMP=16` startup; record total attempts, network/UART PASS counts, and any first decisive failure without adding run identity infrastructure.
- `git diff --check` and strict OpenSpec validation.

**Gate 2 Readiness**

- No Missing requirements: PASS — A2–A6 map to Tasks 3.2–3.6.
- Simplified requirements approved: PASS — no existing requirement is weakened.
- Investigation complete: PASS — each gap is tied to an actual symbol, state transition, and test seam.
- Design closed: PASS — lifecycle commit, direct telemetry, witness cleanup, and UART park semantics are explicit.
- Tasks executable: PASS — each task has targets, behavior, witnesses, GREEN, and stop conditions.
- Iteration plan ordered and balanced: PASS — the UART addition is a narrow prerequisite for the same SMP startup Gate and is explicitly authorized; other UART work stays excluded.
- Traceability complete: PASS — requirement, evidence, task, code, and test are mapped.
- Verification sufficient: PASS — deterministic models precede repeated QEMU startup; one lucky boot is insufficient.
- No identity-style evidence engineering: PASS — repeated attempts are summarized as behavior counts only.
- No material TBD for Act: PASS — failure and cleanup semantics are decided.
- Change/Cycle consistency: PASS — `tasks.md` now assigns Task 3.6 to Iteration 002 and updates its boundaries.
- Persisted Evidence mode valid: PASS — results remain reproducible and fit in Act Response.
- User scope approval: PASS — 用户明确表示“虽然这是给Issue但是我授权在这里进行修复”。
- User plan approval: BLOCKED — revised Cycle awaits audit after this Plan delivery.

**Persisted Evidence**

- Mode: none
- Budget: commands, decisive output, exit status, files, and symbols belong in Act Response; do not create an Evidence directory.

**Risks and Notes**

- Enabling THRE on an empty transmitter may cause an immediate IRQ; the handler/recheck path must disable or consume it without an interrupt storm.
- A witness supervisor must never depend on the target hart's disabled timer.
- Repeated QEMU startup demonstrates this virtual SMP schedule only; it is not a physical UART timing claim.

## Act Response

- Status: reported

**Implemented**

完成 Cycle 001-replan 的 Tasks 3.2/3.3/3.4/3.6/3.5，落实第一轮 Plan Review 的全部 6 个
blocking findings、第二轮 Review 的 7 项 Follow-up 修复，以及第三轮（本轮）Review 的
park-commit 闭合修复。A2–A6 在确定性层与 `SMP=16` 运行时层闭合。

**本轮 Review 修复（Follow-up Decision 1–6：park-commit 门控 + 原子提升）**

**R8（指令 1–3）— `Armed` 只能由 TRIGGER 对已提交 `Blocked` 的目标原子提升。**
- `net_wake_witness.rs::witness_target`：target 的 `poll_fn` 只做
  `WAKER.register` + 标志检查 + `Pending`，**不再触碰状态机**（删除自发布 Armed 的
  `on_armed` 调用与 `armed` 分支）；machine 停留在 `Starting` 直到 TRIGGER 提升。
  统一后置路径保持：恢复 timer →（仅当 phase 确实为 `Armed` 时）`on_woken` →
  `restore_ack` → 按 `CANCELLED` 收敛 `Failed`/`Completed`。取消路径不再把预期的
  收敛计成非法转换（V5 `witness_illegal_transitions` 不被污染）。
- `op::TRIGGER` 三门控顺序：① saved `WITNESS_TASK` handle 存在且
  `task.state() == axtask::TaskState::Blocked`（拒绝 Running/Ready/缺失 handle，
  不改变任何状态、不 wake）；② saved task 的 singleton cpumask 仍覆盖当前 run 的
  `target_hart`（拒绝跨 run/陈旧任务）；③ 机器锁下原子提升
  `Starting -> Armed` 并记录触发方 hart——仅未取消的 `Starting` run 接受，
  取消/terminal/重复提升拒绝（`NotArmed`/`Cancelled`）且不 wake。提升成功后才
  `TRIGGERED.store` + `WAKER.wake()`：wake 必然命中已注册 waiter，真实
  `Blocked -> Ready`、目标 hart 收到 reschedule IPI（`after > before`）。
  `AxWaker::wake_by_ref` 在 target 仍 `Running` 时只置 `woke` 标志、无
  `Blocked -> Ready`、无 IPI 的窗口被彻底关闭。
- `net_wake_witness_logic.rs`：`trigger` 重定义为原子提升（`Starting` && 未取消 &&
  非目标 hart → `Armed` + 记录触发方）；`TriggerReject` 新增 `Cancelled`；删除
  `on_armed`（`Armed` 语义改为"已接受一次针对真实停放目标的 remote trigger"）；
  阶段文档与模块文档同步。

**R9（指令 4）— 取消与提升在机器锁下串行，trigger 干净地输给取消。**
- `request_cancel`（`Starting|Armed|Woken`）与 `trigger` 提升都持有机器锁：监督先到
  则提升返回 `Cancelled` 且 run 收敛 `Failed`；提升先到则监督看到 `Armed`。监督
  `Starting|Armed` 所有权（finding 1 修复）保持不变。

**R10（指令 5）— 确定性见证。**
- 机器模型新测试：`trigger_is_atomic_promotion_from_starting`（Idle 拒绝、本地拒绝、
  远端提升记录触发方、重复提升拒绝）、`trigger_loses_cleanly_to_cancellation`、
  `timeout_vs_trigger_serialization_is_deterministic`（两种交错各收敛恰好一个
  terminal，归因不丢）；协议迁移更新全部既有模型测试（promotion 取代自 arm）。
- 绑定源序新守卫：`witness_target_parks_without_publishing_armed`（poll 闭包不含
  `MACHINE`/`on_armed`）、`witness_trigger_gates_blocked_then_promotes`
  （handle/Blocked 门控 → 提升 → TRIGGERED → wake 的严格顺序）；
  `witness_binding_preserves_timer_restore_symmetry` 更新（timer 恰好禁用一次、
  target 永不发布 Armed、restore→ack→terminal 顺序）。

**Changed Files and Symbols**

- `kernel/src/drivers/net_wake_witness.rs`: `witness_target` 纯 register+park；
  `op::TRIGGER` 三闸门控 + 锁下原子提升；模块文档。
- `kernel/src/drivers/net_wake_witness_logic.rs`: `trigger` 原子提升语义；
  `TriggerReject::Cancelled`；删除 `on_armed`；阶段/模块文档。
- （前轮已交付并保留：T3.2 失败原子、T3.3 真实 hart/V5 精确 ABI、监督 `Starting`
  所有权、THRE one-shot、smoke 直接 tuple 比对、placement 容量单一权威
  `axconfig::plat::MAX_CPU_NUM`、`UNKNOWN_SCOPE` 未用 re-export 清除。）

**Deviations from Plan**

- 提升成功后 target 唤醒时仅在 phase 确实为 `Armed` 时调用 `on_woken`，避免取消
  路径产生预期外的 `illegal_transitions` 计数污染 V5 观测；行为与机器转换表一致。
- 其余按 Follow-up Decision 逐条执行，无偏离。

**Blocker Handoff**

None。

**Blocker Resolution**

None。

**Self-Review**

- Plan compliance: PASS — Follow-up Decision 6 项指令全部落实；A2–A6 保持闭合。
- Full diff reviewed: PASS — 本轮 diff 仅 2 个产品文件 + harness；无计划外修改；
  无身份型证据机制（无 run/session ID）；无嵌套锁（WITNESS_TASK 与 MACHINE 锁
  顺序获取、不重叠）；无新增警告。
- Critical/Important unresolved: 0。
- Minor unresolved: 1 — `make host-test` 整批偶发
  `cross_hart_second_acquire_waits_for_first_release` 时序 flaky（历史底噪）。

**Verification Evidence**

| 验证项 | 命令 | 输出摘录 | 覆盖范围 | 结论 |
|---|---|---|---|---|
| ms04 host harness | `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs` | 92 passed; 0 failed | promotion/Blocked 门控/串行化/源序守卫/V5 ABI/placement 容量 | PASS |
| make host-test | `make host-test`（用户手动整批） | 全部通过 | 全部确定性回归 | PASS |
| make justrun SMP=16 | `make justrun SMP=16 NET=y GRAPHIC=n`（用户手动） | NET-SMP-SMOKE PASS + UART-SMP-SMOKE PASS | 修复后无启动回归（witness TRIGGER 由 ioctl 驱动，smoke 不覆盖） | PASS |
| kernel build | `make build` / `make build SMP=16` | 0 errors | 普通 + smp（含新 TRIGGER 门控编译） | PASS |
| axnet ordinary / qemu-diagnostics | `cargo test --manifest-path crates/axnet/Cargo.toml [--features qemu-diagnostics]` | 479 / 511 passed; 0 failed | 覆盖范围未变化，采信前轮结论 | PASS |
| uart async | `cargo test --manifest-path crates/uart_16550/Cargo.toml --features async` | 69 unit + 8 doc + 10 compile-fail passed | 覆盖范围未变化，采信前轮结论 | PASS |
| OpenSpec strict | `openspec validate ms08-…. --strict` | is valid | change 结构 | PASS |
| git diff check | `git diff --check` | exit 0 | whitespace | PASS |

前轮运行时 Gate（4/4 次 `SMP=16` 启动双 smoke PASS、anchor 多样化、直接 tuple 全一致）
覆盖的代码表面未被本轮修改（本轮仅 witness 控制面），按公共规则采信；witness TRIGGER
的端到端 remote-wake/IPI 因果在 Iteration 005 资格（Task 6.2 的 timer-disabled wake
case）中消费。

**Persisted Evidence**

None required（Cycle Persisted Evidence mode = none；结果可低成本复现且短于 Act
Response 上限，未创建 Evidence 目录）。

**Experience Candidates**

- **Issue 候选**：`cpumask 0.1` 的 `CpuMask::get/set` 越界仅有 debug_assert，release
  下越界索引返回/写入垃圾值；任何按独立容量常量迭代的调用方都会静默踩中（前轮
  `sched_hart_ids` 即因此把不存在的 hart 交给 placement）。本仓库其余循环已用
  axconfig 权威值，但属于值得落账的库级陷阱。
- **Issue 候选**：D1 full-app build（`lichee-d1-async-uart`）被 25 个 pre-existing
  `axfs`/`axtask::TaskExt` 未解析 import 阻塞，多轮 Cycle 均无法完成全 app D1 链接。
- **Runbook 候选**（重复登记，仍未落账）：QEMU boot/`make run` 端口 5555 残留；
  清理用 `pkill -9 -x qemu-system-riscv64`，避免 `pkill -f` 全命令行匹配自命中。

**Remaining Issues**

- `make host-test` 整批偶发 `cross_hart_second_acquire_waits_for_first_release` 失败为
  历史时序底噪（单独/小批量全绿；多轮整批手动运行均通过）。
- D1 full-app build 的 `axfs`/`TaskExt` 未解析 import 失败为 pre-existing（受影响
  UART 表面已在 D1 目标编译通过）。

**Commit or Diff Reference**

- 未提交工作树改动；涉及文件见 `Changed Files and Symbols`。未归档、未同步 SNAPSHOT/tasks、
  未清理分支（按 Act 阶段边界）。

## Plan Review

- Review Result: accepted

**Findings**

No blocking findings.

- The remaining park-commit gap is closed. `witness_target` only registers the
  waiter and returns `Pending`; it no longer publishes `Armed`. The `TRIGGER`
  path requires the saved target task to be `TaskState::Blocked`, checks that
  its affinity still covers the current target, then promotes
  `Starting -> Armed` under the witness-machine lock before setting
  `TRIGGERED` and waking it. `TaskInner::state` uses Acquire and
  `blocked_resched` publishes `Blocked` with Release; the subsequent
  `unblock_task` transition therefore performs the required
  `Blocked -> Ready` enqueue and remote IPI.
- Timeout and trigger serialize through the same machine lock. Cancellation
  first makes promotion return `Cancelled`; promotion first leaves the run
  supervised in `Armed`, and a later cancel converges through restore-ack to
  `Failed`. Neither order permits terminal publication before timer restore.
- The saved task cannot be a live blocked task from a released run: terminal
  publication happens in the resumed target, and a new reservation is accepted
  only after that terminal. A stale exited/running handle fails the scheduler
  state gate; the current target affinity is checked before promotion.
- No non-blocking code finding remains in this repair. The source-order guards
  are structural witnesses rather than runtime qualification, which is
  consistent with this Iteration's host/model plus target-build boundary; the
  full QEMU timer-disabled case remains assigned to Iteration 005.

**Deviation Classification**

None

**Acceptance Gaps**

None. A2–A6 are satisfied within Iteration 002's verification boundary.

**Convergence**

reduced to none — the last scheduler park-commit gap is closed without changing
the Task 3.4 contract or Iteration scope.

**Evidence**

- Independent review covered the final witness binding/state-machine repair,
  its callers, and the scheduler state/wake path (`TaskInner::state`,
  `blocked_resched`, `unblock_task`, and remote-ready IPI emission).
- The Act Response's reported 92 host witnesses, full host test, ordinary and
  `SMP=16` builds, axnet 479/511 tests, UART 69+8+10 tests, strict validation,
  diff checks, and post-fix QEMU smoke are adopted under the user's standing
  instruction that these test results are trusted. The repaired product surface
  is exactly the surface covered by the new 92-test witness/build results.
- This Review reran only structural checks: strict OpenSpec validation and both
  staged/unstaged `git diff --check` completed successfully with exit code 0.
- Persisted Evidence remains `none`; no Evidence directory is required.

**Follow-up Decision**

Accept Iteration 002. Its fixed network placement, direct V5 observations,
bounded witness safety, UART park repair, and combined startup Gate satisfy the
approved stable baseline. Expand the already-planned Iteration 003 for
controlled migration and ordering closure; do not reopen this Cycle.

**Iteration Plan Update**

None

**Next Cycle**

None

**Next Iteration**

`../003-controlled-migration-and-ordering-closure/000-initial.md`
