# Iteration 001 / Cycle 006: bounded listener reconciliation

## Plan Context

- Status: ready
- Approval: approved by user on 2026-08-24（原话：“已经拆分完了么，认可你的思路”）；批准范围为
  修订后的Iteration Map与只执行Task 2.6的Cycle 006；ready for an explicit `openspec-act` invocation
- Iteration: 001-socket-and-listener-readiness-bridge
- Cycle: 006-replan
- Cycle Type: replan
- Parent cycle: `005-replan.md`

**Iteration Scope**

- Change tasks: 2.1–2.6
- Revised task: 2.6
- Depends on: Iteration 000 accepted；Tasks 2.1–2.5与Cycle 004已验证的ownership/scale基线
- Stable baseline: 产品socket不主动推进协议栈；listener reconciliation跨全部active ports共享每round
  32-entry budget；passive RST后的hidden socket不滞留pending；其他runner stage仍有机会运行
- Verification boundary: 单/多listener 31/32/33/512、port/slot cursor、changed-tail、RST-to-Listen
  恢复、stage count和quiet path由host/model tests独立证明，两profile full suites通过
- Diagnostic boundary: 失败限制在ListenTable cursor/state转换、Service listener stage或runner调度
- Deferred tasks: 2.7、2.8、3.1–3.4

**Cycle Scope**

- Trigger: Cycle 005重新平衡审计`replan-required`
- Acceptance gaps: Cycle 005 Acceptance 1中的listener部分
- Revised task: 2.6
- Inherited scope: R3、R4、R6；D4、D5、D7；Tasks 2.1–2.5；single timestamp、TCP deferred
  budget、raw-handle独占、atomic accept/refill与caller零progress
- Excluded scope: smoltcp pending-TX API、UDP queued-TX drop/reap、MS01 payload、manual QEMU、Task 3
  terminal fault广播、SO_LINGER、reset/cancellation、scheduler、SMP、真板、性能、全局文档和归档

**Objective**

把listener reconciliation改为每个runner round一次、跨全部active listeners共享最多32次pending-slot
检查，并让passive-open socket从`SynReceived`回到`Listen`时恢复idle ownership或安全移除冗余slot。
完成后，大backlog不能让listener stage独占round，也不能因完整queue持续self-wake。

**Scenario Sketch**

| Scenario | 前置状态 | 动作 | 可观察结果 | 失败边界 |
|---|---|---|---|---|
| S1 budget edge | 31/32/33/512个pending slots | 执行一个runner round | 合计checked≤32，后续round最终访问全部slot | 每port各获32或一次全表扫描 |
| S2 multi-listener fairness | 多个active ports均有pending | 连续执行round | port/slot cursor持久推进，每个listener最终获服务 | active-port clone/scan不计入budget或尾部饥饿 |
| S3 ready/reset tail | 变化slot位于cursor尾部 | runner推进 | Ready/Reset最终提交且只处理一次 | cursor swap/remove后跳过或重复处理 |
| S4 passive RST | hidden socket从SynReceived回到Listen | reconcile | 无idle时转为idle；已有idle时安全移除冗余raw socket | slot永久Pending或误删仍活跃连接 |
| S5 stage progress | listener backlog超过budget | 执行round | deferred、egress和dispatch等后续stage仍运行 | listener耗尽round或持锁wake/yield |
| S6 quiet | 没有状态变化且完整sweep结束 | runner判断后续工作 | 不持续self-wake | cursor本身被误报为backlog |

**Current Baseline**

- Branch `net-k3`；Cycle 005计划已交接但Act Response仍为`pending`，没有按Cycle 005执行产品修改。
- Tasks 2.1–2.5已完成；Cycle 004的single timestamp、TCP deferred budget、atomic accept/refill、
  exact-512 recovery与cleanup→UDP host witnesses保持既有GREEN基线。
- `Service::stack_round`仍在每个非idle ingress step后调用完整listener reconcile，并在round结束前再次
  调用；512 queue可在一轮内重复扫描。
- pending slot把`State::Listen | State::SynReceived`都保持为Pending；passive-open收到RST并回到
  `Listen`后，socket仍脱离idle ownership。
- 本次只重新划分计划，没有产品代码变化；不重跑Cycle 005的3个UDP RED或QEMU基线，因为它们属于
  后续Iterations 002–003，且不判定本Cycle listener Acceptance。

**Current-State Evidence**

- `Service::stack_round`的ingress closure可调用`ListenTable::reconcile(sockets)`，固定listener stage也会
  调用；当前工作量随ingress step数与pending总数相乘。
- `ListenTable::reconcile`遍历active ports和各entry的pending slots；缺少跨round的全局port/slot cursor。
- 当前state match对`State::Listen | State::SynReceived`采用相同Pending verdict；smoltcp passive-open
  收到RST可从`SynReceived`回到`Listen`。
- listener entry同时拥有idle handle、pending queue和accept bridge；恢复必须遵循
  `SERVICE → SOCKET_SET → entry`锁序，并在guard释放后wake。

**Relevant Code**

| File / Symbol | Current Responsibility | Cycle Use |
|---|---|---|
| `crates/axnet/src/listen_table.rs::ListenTableEntryInner` | idle/pending hidden socket ownership | 保存并维护公平cursor与Listen恢复状态 |
| `crates/axnet/src/listen_table.rs::ListenTable::reconcile` | hidden slot状态转移与accept bridge | 改为全局32-entry bounded outcome |
| `crates/axnet/src/service.rs::Service::stack_round` | 固定runner stage顺序 | 每round只在listener stage调用一次reconcile |
| `crates/axnet/src/stack_runner.rs` tests | runner round与full-chain调度见证 | 证明budget-hit时后续stage和quiet语义 |

**Critical Path**

```text
runner round
  -> bounded ingress/egress
  -> one listener reconciliation batch (all ports total <= 32)
       -> resume persistent port/slot cursor
       -> Ready/Reset commit
       -> SynReceived -> Listen: install as idle or remove redundant raw socket
       -> return checked/changed/backlog outcome
  -> remaining bounded stages
  -> unlock -> staged wakes/self-yield/timer
```

**Implementation Guidance**

先建立单listener 31/32/33/512与多listener RED tests，计数必须覆盖实际检查的pending slots，不能把完整
active-port clone或预扫描移出budget。再覆盖cursor遇到swap/remove、changed-tail和listener增删时仍最终
到达所有slot。实现由ListenTable拥有跨round cursor和结构化outcome，Service每round只消费一次。

`State::Listen`不能继续走普通Pending分支。无idle handle时，把该raw handle从pending转移为idle；已有
idle时，从SocketSet与pending中一致移除冗余handle。所有accept bridge wake、software publish和yield都在
相关guard释放后执行。最后删除或降回Cycle 004加入的临时info洪泛，并用source assertion固定唯一调用点。

**Behavioral Change**

- listener reconciliation从“每个ingress step可能完整扫描pending queue”变为“每round一次，全部active
  listeners合计最多检查32个pending slots”。
- passive RST后回到`Listen`的hidden socket不再永久占用pending backlog；它恢复idle ownership，或在已有
  idle时安全回收。
- backlog存在时仍可请求后续runner工作；完整quiet sweep不会因cursor未归零而持续self-wake。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| 2.6 | R3/S1–S3 | `listen_table.rs::reconcile` | active ports与pending状态扫描 | 全局32-entry cursor/outcome |
| 2.6 | R6/S4 | `ListenTableEntryInner` state conversion | Listen仍视为Pending | 恢复idle或移除冗余slot |
| 2.6 | R3/R4/S5–S6 | `service.rs::stack_round`、runner tests | ingress内重复reconcile | 固定stage一次调用、guard外wake |

**Task Contracts**

### 2.6: 有界listener reconciliation与passive RST恢复

- Requirement/Scenario: R3、R4、R6；D4、D5、D7；S1–S6。
- Depends on: Tasks 2.1–2.5 GREEN；Cycle 004 atomic accept/refill与single timestamp基线。
- Targets: `listen_table.rs::ListenTableEntryInner/ListenTable::reconcile`、
  `service.rs::Service::stack_round`、outcome/telemetry与stack-runner tests。
- Current behavior: 每个非idle ingress step可触发完整pending扫描；`State::Listen`保持Pending。
- Required behavior: 所有active listeners合计每round最多检查32个pending slots并持久公平；不得完整
  clone/扫描active-port列表；Ready/Reset最终提交；回到Listen的slot恢复idle或安全删除；其他stage同round
  运行，quiet queue不持续self-wake。
- Required changes: 先加入单/多listener 31/32/33/512、changed-tail、port/slot cursor swap/remove、
  Listen恢复、stage count和guard外wake RED tests；再实现有界outcome并删除临时info洪泛。
- Preserve: backlog=512、Ready唯一accept、Reset错误、accept atomic refill、桥接waker重臂、
  `SERVICE → SOCKET_SET → entry`、caller零progress、TCP deferred budget与单round timestamp。
- Forbidden: 周期全表poll、提高backlog、hidden waker内获取Service/SocketSet、guard内wake/yield、
  smoltcp UDP API、UDP drop/reaper、MS01 payload和manual QEMU修改。
- Test witness: 当前source中ingress closure仍调用`reconcile(sockets)`；新增behavior/source tests先RED。
- GREEN condition: 31/32/33/512每roundchecked≤32且最终收敛；RST-to-Listen不泄漏slot；两profile
  targeted各100×，其他stage/quiet assertions与full suites通过。
- Verification: targeted两profile→full suites→compile/fmt/source/OpenSpec/diff checks。
- Stop when: 精确事件识别需要per-hidden callback持锁、改变backlog/accept语义、修改scheduler，或发现
  listener Acceptance依赖UDP lifecycle或manual QEMU才能判定。

**Invariants**

- resident runner仍是唯一smoltcp推进者；产品socket不恢复`poll_interfaces()`。
- listener Ready只交付一次，Reset保持既有错误，accept返回前atomic refill保持backlog headroom。
- listener、deferred、Router stages各自有界；任何guard不跨wake、await、Pending或yield。
- backlog 512、TCP short write、UDP datagram原子性、PollSet 64/65和single-hart范围不变。

**Non-goals**

- 本地smoltcp `has_pending_tx()`、UDP queued-TX lifecycle和3个现有UDP RED tests。
- MS01 overflow/recovery workload、diagnostic single/fork和manual QEMU runtime。
- terminal fault广播、SO_LINGER、reset/cancellation、SMP、多接口、真板和性能。

**Replan Traceability Matrix**

| Requirement | Scenario | Design | Task | Code Surface | Test Witness | Simplification | Status |
|---|---|---|---|---|---|---|---|
| R3 bounded/fair | S1–S3,S5,S6 | D4 | 2.6 | listener cursor、Service round | 31/32/33/512、multi-listener、changed-tail、quiet | None | Covered |
| R4 lock/wake | S5 | D5 | 2.6 | reconcile commit与outcome | guard外wake、后续stage count | None | Covered |
| R6 listener lifecycle | S4 | D7 | 2.6 | pending state conversion | SynReceived→Listen idle/redundant paths | None | Covered |

没有Missing或Simplified requirement。本Cycle没有修改UDP或QEMU验证契约；它们已明确分配到后续Iteration。

**Acceptance**

1. 全部active listeners合计每runner round检查不超过32个pending slots；31/32/33/512与多listener
   cases均最终收敛，cursor在entry swap/remove和listener增删后不跳过、不重复持有无效handle。
2. Ready/Reset保持既有唯一提交语义；`SynReceived → Listen`后，无idle时恢复为idle，已有idle时安全移除
   冗余raw socket，pending/backlog计数不泄漏。
3. listener budget-hit时其他runner stages仍在同round执行；完整quiet sweep不持续self-wake；所有wake、
   software publish和yield发生在guard释放后。
4. Tasks 2.1–2.5既有readiness、锁序、atomic accept/refill和caller-zero-progress tests保持GREEN；ordinary与
   qemu-diagnostics axnet full suites、fmt/source、strict OpenSpec和full diff review通过。
5. 结论仅覆盖host/model listener机制；UDP drain、MS01 runtime、Task 3、SMP、真板和性能仍未验收。

**Verification**

- 两profile分别运行listener budget/RST、stack stage、cursor与quiet targeted tests各100×。
- `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib`
- 同命令增加`--features qemu-diagnostics -- --test-threads=1`。
- kernel QEMU check、root lichee-d1 check与`make LOG=error build`只作为受影响编译/产品集成Gate，不把
  它们解释为manual QEMU runtime证据。
- source assertions：每round只有一个listener reconciliation stage；active-port traversal受budget约束；
  产品socket无`poll_interfaces()`；guard内无wake/yield。
- `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check`
- `openspec validate ms06-application-visible-async-network-stack --strict`
- `git diff --check HEAD`与完整diff review。
- SKIPPED: smoltcp UDP unit、3个UDP RED和manual QEMU；它们分别属于Iterations 002、003，不判定本轮。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | 实际调用点、重复scan、state transition和测试入口已核对 |
| Design | PASS | ListenTable拥有全局cursor/outcome，Service固定stage消费；RST ownership无TBD |
| Iteration Plan | PASS | Tasks 2.6、2.7、2.8分属三个依赖有序且可独立验证的Iteration |
| Cycle Scope | PASS | Cycle只执行Task 2.6；UDP、guest和manual runtime均明确排除 |
| Task Contracts | PASS | 入口、RED、目标、保持/禁止、GREEN、验证与停止条件完整 |
| Traceability | PASS | R3/R4/R6→S1–S6→D4/D5/D7→2.6→代码→tests无Missing |
| Verification | PASS | host/model可独立判定listener Acceptance；两profile与full suites构成回归边界 |
| User Approval | PASS | 用户于2026-08-24明确认可拆分思路与Cycle 006范围 |

Gate 2全部PASS。Cycle已从`draft`更新为`ready`，等待显式`openspec-act`调用；本次批准不构成
自动实施、Plan Review、全局状态同步或收尾授权。

**Persisted Evidence**

- Mode: none

自动结果可低成本重跑，Act Response足以记录决定性命令、输出与退出码。

**Risks and Notes**

- smoltcp自身SocketSet遍历成本仍可能随socket数增长；本Cycle只限制axnet listener reconciliation，
  不声明整体性能资格。
- cursor必须在listener/slot删除和swap后保持有效；局部数据结构选择可由Act决定，但不得改变公平、
  budget或ownership语义。
- 临时info日志会改变QEMU调度并造成串口洪泛；本Cycle只要求清除本机制相关的临时诊断，不执行runtime。

## Act Response

- Status: reported

**Implemented**

- Task 2.6：`ListenTable` 的 listener reconciliation 改为有界、跨 round 公平的
  单 stage。`Service::stack_round` 每 round 只在 ingress/egress 之后的固定 stage
  调用一次 `reconcile`（删除 before-ingress、ingress closure 内与 after-egress
  之前的重复调用），并以 `ingress.socket_changed || egress.socket_changed`
  作为启动 sweep 的协议进展信号。
- 引入 `ReconcileCursor`（跨 round `(port, slot)` 旋转 cursor + `remaining` +
  `sweeping`）。sweep 启动时以 `Σ(queue.len()+1)` 在 active ports 上精确计数
  （O(ports)，非 pending 扫描），每 round 最多消费 `STACK_STAGE_BUDGET`（32）
  个 position；`checked == 32 && remaining > 0` 才报告 `sweep_incomplete`，
  让 runner 有界 self-wake 续扫，完整 sweep 后停机（quiet 不自唤醒）。
- entry 级 reconcile 拆为 `reconcile_head`（idle 转移/refill/重臂）与
  `examine_slot`（逐 slot 状态读取提交）。`State::Listen` 不再走普通 Pending
  分支：无 idle 时把该 raw handle 恢复为 idle 并从 backlog 移除 slot；已有 idle
  时从 SocketSet 与 queue 一起移除冗余 socket（passive RST → Listen 恢复）。
- 删除/降级临时 info 洪泛：逐 slot 的 `info!` 改为 `debug!`。
- `reconcile` 返回 `ListenerReconcileOutcome { checked, sweep_incomplete }`；
  `StackRoundOutcome` 增加 `listener_checked`/`listener_sweep_incomplete`，
  runner telemetry/snapshot 增加 `listener_checked`，self-wake 条件并入
  `listener_sweep_incomplete`。

**Changed Files and Symbols**

- `crates/axnet/src/listen_table.rs`：`ListenTable::reconcile`（有界+outcome）、
  `ReconcileCursor`、`ListenerReconcileOutcome`、`SlotExamine`、
  `ListenTableEntryInner::{reconcile_head, examine_slot}`、`reconcile_cursor`
  字段、`test_seed_closed_slots`/`test_park_idle_as_pending_slot` seams、
  reconcile 相关 6 个新测试。
- `crates/axnet/src/service.rs`：`StackRoundOutcome::{listener_checked,
  listener_sweep_incomplete}`、`stack_round` 单 stage 调用、
  `listener_stage_budget_does_not_steal_router_or_deferred_stage_budget` 测试。
- `crates/axnet/src/stack_runner.rs`：`StackTelemetry/StackSnapshot::listener_checked`、
  self-wake 条件、`task_26_listener_sweep_self_wakes_to_finish_then_parks`、
  `task_26_passive_rst_returns_hidden_socket_to_listen_and_recovers`、
  `task_26_listener_stage_is_single_bounded_call_without_guard_wake` 测试。

**Deviations from Plan**

1. fmt 归一化：`cargo fmt --check` Gate 要求 0 diff，而 axnet 基线即存在 11 处
   pre-existing 长字符串换行偏差；本次运行 rustfmt 顺带重排了若干既有字符串行
   （纯换行，无语义变化）。
2. 测试构造：budget/fairness/quiet 测试以"fresh Closed socket + Pending 标记 slot"
   作为即时可提交工作种子（与既有 deferred budget 测试同型），因为无法低成本批量
   构造真实 SynReceived slot（无 pause_synack feature）；真实 SynReceived→RST
   路径由全链路 loopback 测试覆盖（`task_26_passive_rst...`）。
3. S4 promote-as-idle 路径仅在 backlog 满（refill 受阻）时触发；refill 可行时 head
   先重建 idle，二者结果等价（idle 存在、slot 移除），测试以满队列约束确定性覆盖
   promote 分支。
4. `reconcile_head` 在每次 port 访问时无条件执行（O(1) 额外），与旧代码每次调用
   检查 idle 的行为一致；预算 token 由随后的 `k==len` checkpoint 记一次。

**Self-Review**

- Plan compliance: PASS —— Task 2.6 契约的入口、行为变化、Preserve 项与 Forbidden 项
  均已覆盖；删除了重复 reconcile、每 round 唯一 stage、32-position 上限、跨 round
  cursor 公平、RST-to-Listen 恢复、guard 外 wake、quiet 不持续 self-wake。
- Full diff reviewed: PASS —— `git diff crates/axnet/src/{listen_table,service,
  stack_runner}.rs` 全量复核，无计划外生产修改；测试与 seam 均 cfg(test)。
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 无（已修复参数/绑定/注释等小问题）。
- Gate 3（RED 见证）：budget/sweep 测试在中间实现上观察到 RED（32-processed
  边界、full_pass 误判）后推进 GREEN；S4/RST 测试在全表扫描基线上无法通过（RED）。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| ordinary 全量 | `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib` | `test result: FAILED. 309 passed; 3 failed` | PASS（3 failed 为既有 Task 2.7 UDP RED，Cycle 006 明确 SKIPPED） |
| qemu-diagnostics 全量 | 同上 + `--features qemu-diagnostics -- --test-threads=1` | `test result: FAILED. 329 passed; 3 failed` | PASS（同上） |
| 产品构建 QEMU | `make LOG=error build` | `Finished release profile ... exit 0`，生成 StarryOS_riscv64-qemu-virt.elf/.bin | PASS |
| 产品构建 D1 | `make ARCH=riscv64 APP_FEATURES=lichee-d1 MYPLAT=axplat-riscv64-lichee-d1 ... build` | `Finished release ... exit 0`，生成 StarryOS_riscv64-lichee-d1.elf/.bin | PASS |
| fmt | `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check` | 0 diffs | PASS |
| diff whitespace | `git diff --check HEAD` | 无输出，exit 0 | PASS |
| OpenSpec | `openspec validate ms06-application-visible-async-network-stack --strict` | `Change ... is valid` | PASS |
| source 断言 | `task_26_listener_stage_is_single_bounded_call_without_guard_wake` | PASS（100×） | PASS |
| targeted 100× | budget/S2/S3/S4/S5/S6/source 测试内部 100× 循环 | 两 profile 全绿 | PASS |

既有 UDP RED 3 项（`deferred_retirement_udp_*`、`task_27_repro_udp_child_close_keeps_queued_echo`）
在裸基线（stash 我的改动后）同样失败，属 Iteration 002/Task 2.7 范围，本 Cycle 按
Plan SKIPPED。

**Persisted Evidence**

None required

（模式 `none`；全部自动结果可低成本重跑，决定命令/输出/退出码已摘要于上表。）

**Experience Candidates**

- Runbook：`make LOG=error build` 与 lichee-d1 feature 构建命令已在 QEMU+D1 双编译
  Gate 验证（见 Verification Evidence），可考虑记录到既有 MS06 构建 Runbook 路径。
- Incident：None。

**Remaining Issues**

None

**Commit or Diff Reference**

None（未提交；`git diff HEAD -- crates/axnet/src/` 为本次变更面）

## Plan Review

- Status: completed

**Review Result**

rework-required

**Findings**

1. **Blocking — listener stage 仍包含未计入 32-position budget 的全 active-port 预扫描。**
   `ListenTable::reconcile` 在新 sweep 开始时通过 `ports.iter()` 访问并锁定每个 active listener，
   计算 `Σ(queue.len()+1)` 后才进入 `checked < STACK_STAGE_BUDGET` 循环。active listener 数量可远大于
   32，因此一个 round 的 listener 工作量和持有 `active_ports` 的时间仍随全部 active ports 线性增长。
   这直接违反本 Cycle Task Contract 的“不得完整 clone/扫描 active-port 列表”、Verification 的
   “active-port traversal 受 budget 约束”和 S5 的其他 stage 不被 listener 独占要求。
2. **Blocking — unfinished sweep 会吞掉 sweep 期间的新 protocol progress。** 当
   `cursor.sweeping == true` 时，`protocol_progressed` 没有被记入 restart/dirty 状态；`remaining` 仍是旧
   topology/queue 快照。若该期间 listener 增删或旧快照外的 listener 发生 socket transition，当前 sweep
   完成后 cursor 直接停机；下一 self-wake round 的 `protocol_progressed` 已经消失，遗漏位置没有新 sweep
   保证。现有测试只覆盖固定两 listener 和 accept 删除，未覆盖 Plan/Acceptance 明列的 listener 增删与
   active sweep 中新 progress，不能证明“不跳过”。
3. **Blocking Gate — fresh fmt 与 Act Response 不一致。** Review 运行
   `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check` 得到
   `stack_runner.rs:3013` diff，命令非零；Act Response 记录“0 diffs / PASS”。Acceptance 4 的 fmt Gate
   因此当前未满足。
4. **Non-blocking — 已实现部分确实缩小了缺口。** 实际 diff 已把 Service 中 listener reconcile 收敛为
   每 round 一个固定 stage；逐 slot `checked` 不超过 32；Ready/Reset、RST-to-Listen idle/redundant
   ownership、guard 外 drain 和 quiet self-wake 的现有 targeted tests 均通过。三个 UDP RED 仍与
   Act Response 一致地属于后续 Iteration 002，不作为本 Cycle 新 finding。
5. **Non-blocking Minor — `test_queue_len` 上方有一行重复 doc comment。** 不影响 Acceptance，可在修复
   相邻代码时顺手移除，但不得单独扩大返工范围。

**Deviation Classification**

- ACT-DEVIATION：实现使用未受 budget 约束的全 active-port 预扫描，且 source witness 没有验证 Plan
  明确要求的 traversal 边界。
- PLAN-OMISSION：Cycle 没有把“active sweep 中再次出现 protocol progress”写成独立测试矩阵，虽已在
  listener 增删/不跳过 Acceptance 中隐含要求。
- NEW-EVIDENCE：fresh Review 发现 fmt Gate 非零，与 Act Response 的 PASS 记录不一致。

**Acceptance Gaps**

- Acceptance 1：listener stage 的完整 traversal 尚未受 32-position budget 约束；listener/slot 动态增删
  与 active-sweep progress 尚无不跳过证明。
- Acceptance 3：大量 active listeners 时，预扫描仍可能独占 round；active sweep 中的新 progress 未保证
  触发后继 bounded pass。
- Acceptance 4：fresh `cargo fmt --check` 未通过；source assertion 没有覆盖 active-port traversal。

**Convergence**

`reduced`。相较父 Cycle 005 的未实施状态，Cycle 006 已关闭单固定 stage、逐 slot budget、
RST-to-Listen ownership、guard 外 wake 和 quiet park 等大部分 listener gap；剩余问题集中在 bounded
topology traversal、progress 合流和 fmt Gate，可在同一 Iteration 内修复，不需要改变 requirement、
Iteration Map 或验收边界。

**Evidence**

- `crates/axnet/src/listen_table.rs:362-371`：sweep 开始时 `ports.iter()` 全量读取并锁定所有 entry，发生在
  `checked < STACK_STAGE_BUDGET` 循环之外。
- `crates/axnet/src/listen_table.rs:352-434`：sweeping 分支不保存新的 `protocol_progressed`，完成时仅以旧
  `remaining` 决定是否继续。
- `crates/axnet/src/stack_runner.rs:2997-3023`：source test 只检查一个 reconcile 调用、源码包含 budget
  常量/cursor 和 guard 内无 `.wake(`，未断言 active-port traversal 受 budget 约束。
- ordinary targeted：`cargo test ... --lib reconcile_ -- --test-threads=1`，8 passed，exit 0。
- ordinary Task 2.6 targeted：`cargo test ... --lib task_26 -- --test-threads=1`，8 passed，exit 0。
- qemu-diagnostics reconcile targeted：8 passed，exit 0。
- ordinary/qemu-diagnostics full suites：分别 309/329 passed、3 failed，exit 101；失败仍是计划显式
  SKIPPED 的 Task 2.7 UDP RED。
- `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check`：报告
  `stack_runner.rs:3013` formatting diff，非零。
- `openspec validate ms06-application-visible-async-network-stack --strict`：valid，exit 0；
  `git diff --check HEAD`：无输出，exit 0。
- Persisted Evidence 为 `none`；没有 Evidence 目录是符合计划的，不构成 finding。

**Follow-up Decision**

在同一 Iteration 创建 `007-rework.md`，只关闭 Task 2.6 的既有 bounded traversal、dynamic progress 和
verification gap。该修复不改变行为目标、依赖、requirement 或验证类别，因此不是 replan。下一 Cycle
在用户批准前保持 draft；未调用 Act。

**Iteration Plan Update**

None。Iteration 001 仍为 Tasks 2.1–2.6；Iterations 002–004 保持不变。

**Next Cycle**

`007-rework.md`（draft，等待用户批准）。

**Next Iteration**

None；只有 `007-rework.md` accepted 后才展开 Iteration 002。
