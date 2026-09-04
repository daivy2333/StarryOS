# Iteration 002 / Cycle 000: Data-Stage Deadlines and Coherent Fault Identity

## Plan Context

- Status: ready
- Iteration: 002-data-stage-deadlines-and-coherent-fault-identity
- Cycle: 000-initial
- Cycle Type: initial
- Parent cycle: None

**Iteration Scope**

- Change tasks: 2.2
- Depends on: Iteration 001 accepted
- Stable baseline: submit、completion、reclaim各有独立1s absolute deadline；timeout和ownership fault以单一提交边界产生一致可读的stage、cause、QueueEpoch和owner summary。
- Verification boundary: deterministic clock逐stage证明arm、保持和到期；submit cancel、completion/reclaim recovery、coherent fault读取和axnet ordinary/diagnostics串行全量均通过。
- Diagnostic boundary: active queue round的data-stage识别、deadline/timer、ticket与slot终结、recovery fault提交。
- Deferred tasks: 2.3–4.2

**Cycle Scope**

- Trigger: initial
- Acceptance gaps: None
- Repair items: None
- Inherited scope: R3–R4、D3–D5、Iteration 001接受的QueueEpoch ticket ledger、slot/ledger一致取消、非Reclaimed flush错误和guard外wake约束。
- Excluded scope: quiesce/reset/reinitialize状态机的独立验收、link/socket epoch、公开V4 snapshot、QEMU控制与runtime、SMP、PCI/DWMAC runtime、真板和性能。

**Objective**

让Active queue owner能够独立识别并计时submit wait、completion wait和reclaim wait。每个wait在进入时只arm一次1s absolute deadline，同一wait持续Pending不能续期；到期后按owner状态取消Queued或进入recovery/fault。同时以一个内部有界值原子提交完整fault identity，读取者不能拼接不同故障的字段。

**Background**

Iteration 001已闭合ticket、slot、ARP pending和flush的owner语义。当前工作树保留了后续resident recovery实现，但data path仍只在同步操作直接返回错误时按stage label进入recovery；没有测量“持续等待”本身。当前fault summary又由多个relaxed atomic分次发布，stage、epoch和owner summary可能来自不同提交。Iteration 002只建立data-stage触发与一致fault identity，不提前接受driver recovery lifecycle。

**Current Baseline**

- Revision基线为 `aab92f95825cfb8dd9983249bcfe118ab6a3d64c`；产品和OpenSpec改动尚未提交。
- Iteration 001 fresh Gate：真实slot/ledger witness 1/1、drift同guard witness 1/1、ordinary 402/402、qemu-diagnostics 424/424，均单线程exit 0。
- `RxRxFuture`只有一个 `recovery_deadline`，它仅在进入Quiescing/Resetting/Reinitializing后使用；Active状态没有submit/completion/reclaim deadline字段或timer选择。
- `service_round`依次执行reclaim、RX copy、submit。submit `Full`、DeviceOwned等待completion、completion可见但未reclaim都只进入现有sleep/wake协议，没有记录wait开始时间。
- `recover_stage::{SUBMIT_WAIT,COMPLETION_WAIT,RECLAIM}`目前只用于同步错误分类和诊断code，不是计时器。
- `freeze_recovery_summary`依次写 `recover_fault_stage`、epoch及三个owner counter；字段均为独立relaxed atomic，且没有一次读取完整fault的接口。
- 现有 `RecoveryTestClock`、queue timer seam、diagnostic submit/reclaim hold和 `RecoveringDevice` fixture可复用来建立deterministic RED，不新增周期polling。

**Current-State Evidence**

1. `async_rx.rs::RxRxFuture`只有 `recovery_deadline: Option<u64>`；其注释和 `arm_recovery_timer`均绑定recovery stage，Active round没有data deadline state。
2. `service_round`中 `TxSubmitStep::Full`只设置 `submit_full`，round末进入register/recheck；同一TX slot可无限等待driver接受。
3. `device_owned_len_target()`能观察software ledger中的DeviceOwned ticket；`completion_pending_both_target()`能区分TX completion是否可见，足以区分completion wait与reclaim wait，不得用stage code推断时间。
4. reclaim返回 `BadState` 已进入ownership drift，不能由timeout触发reset掩盖；其他直接错误仍沿既有recoverable fault分类。
5. `tx_cancel_queued_target()`已在同一Service guard内同步关闭Queued ticket与TX slot；submit timeout可复用该owner事务，不能另写只清ledger的旁路。
6. `freeze_recovery_summary`先后取得Service guard并写五个relaxed atomic；并发读取者可观察撕裂组合。现有测试只在故障完成后逐字段读取，不能证明提交一致性。
7. V1–V3 `RxSnapshot`/`RxSnapshotV3`已冻结；内部fault identity不得新增或重排其wire字段。

**Relevant Code**

- `crates/axnet/src/async_rx.rs::{RxRxFuture::service_round,poll_active,poll_register_recheck,arm_recovery_timer,enter_recovery,enter_drift_quarantine,publish_recovery_fault,freeze_recovery_summary}`：stage识别、计时、状态提交和wake顺序。
- `crates/axnet/src/recovery.rs::RecoveryTestClock`：host deterministic clock。
- `crates/axnet/src/device/{mod.rs,ethernet.rs,fixed_queue.rs}`：DeviceOwned/Queued观察、slot与ticket终结、fault stage。
- `crates/axnet/src/{service.rs,router.rs}`：target-scoped ledger、completion、cancel、owner summary与recovery capability转发。
- `crates/axnet/src/flush.rs`：非Reclaimed和recovery fault的稳定完成语义。
- `crates/axnet/src/async_rx.rs` tests及qemu-diagnostics hold fixture：三个data wait的可控阻塞入口。

**Critical Path**

```text
Active round
  reclaim:
    DeviceOwned + no visible TX completion -> completion wait deadline
    visible TX completion but no ticket closure -> reclaim wait deadline
    successful closure/no DeviceOwned -> clear corresponding wait
  submit:
    Queued slot + driver Full/held -> submit wait deadline
    accepted/no Queued slot -> clear submit wait
  arm earliest active data deadline without periodic polling
  deadline expires under Service guard:
    submit -> cancel Queued slot+ticket -> commit SubmitWait/Timeout fault -> flush/wake
    completion/reclaim -> validate known ledger -> enter resident recovery
    ownership/identity drift -> commit Faulted directly; never reset
```

**Implementation Guidance**

1. 先建立三个独立deadline state和最早到期timer选择；不能复用单一 `recovery_deadline`，因为submit与DeviceOwned wait可同时存在。
2. wait只在可观察阻塞条件第一次成立时arm；同一条件持续、无关事件或重复poll不得续期。owner闭合、stage成功或终结后清除对应deadline。
3. completion wait以存在DeviceOwned且无可见TX completion为依据；reclaim wait以TX completion可见但本轮没有闭合对应owner为依据。直接 `BadState` 仍立即进入drift quarantine。
4. submit timeout复用 `tx_cancel_queued_target()`；其ticket outcome保持 `CancelledPreSubmit`，fault identity记录 `SubmitWait + Timeout`。本Iteration不提前引入SocketEpoch或公开 `TimedOut` ABI。
5. 用内部有界 `RecoveryFault`（或等价单值）一次提交stage、local cause、QueueEpoch和owner summary，并提供一次读取接口。可用现有no_std同步原语，但不能继续依赖分散relaxed fields作为权威快照。
6. fault identity在Service guard内采集相关epoch/owner事实并形成一个值；状态/outcome先提交，guard释放后再wake。公开V1–V3 snapshot保持字节与字段不变。

**Behavioral Change**

- driver持续拒绝Queued submit满1s后，该packet从slot与ticket ledger恰好取消一次，flush稳定失败；不得产生descriptor/cookie owner或在后续epoch发送。
- DeviceOwned持续无completion满1s，或completion可见但持续无法reclaim满1s时，分别以CompletionWait或Reclaim身份进入既有resident recovery；若已观察ownership drift则直接Faulted。
- deadline是进入wait时的absolute instant；同stage Pending不会延长。
- fault诊断从分散字段变为一次提交、一次读取的内部一致值；公开V1–V3 ABI不变。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| 2.2 | R3/R4 submit wait | `async_rx.rs::service_round/poll_active`；Ethernet cancel | submit Full后事件等待 | 独立1s deadline、Queued slot+ticket取消和timeout提交 |
| 2.2 | R4 completion/reclaim | `async_rx.rs`；Service/Router owner observers | 直接错误分类，无持续wait计时 | 分别识别、arm、保持、到期并选择recovery或drift |
| 2.2 | D5 coherent fault | `async_rx.rs::RxTelemetry/freeze_recovery_summary`；`recovery.rs` | 多relaxed atomic分次写 | 内部有界fault单值与一致读取 |
| 2.2 | compatibility | `RxSnapshot{,V3}`及tests | 冻结wire ABI | 保持布局/字段和序列化不变 |

**Task Contracts**

### 2.2: Independent data-stage deadlines and coherent recovery fault

- Requirement/Scenario: R3 submit cancellation与device-owned timeout；R4 submit/completion/reclaim诊断；D3、D4、D5。
- Depends on: Task 2.1已接受的QueueEpoch ledger、slot/ledger取消、flush outcome。
- Targets: `crates/axnet/src/async_rx.rs`、`recovery.rs`、`device/{fixed_queue.rs,ethernet.rs,mod.rs}`、`service.rs`、`router.rs`及deterministic fixtures。
- Current behavior: Active data waits没有absolute deadline；同步错误虽带stage code，但持续Pending无到期；fault summary由分散relaxed atomics组成。
- Required behavior: submit、completion、reclaim各自进入时arm一次1s absolute deadline并可同时存在；同一wait不续期。submit到期取消仍Queued的slot/ticket并记录timeout；completion/reclaim到期在已知owner ledger上进入recovery，ownership drift直接Faulted。fault identity作为一个有界值一致提交和读取。
- Required changes: 增加data deadline state、earliest timer arm/clear、stage进入和退出判据、到期动作、local timeout cause及coherent fault store/read seam；调整测试fixture但不扩展公开ABI。
- Preserve: stage budgets；QueueEpoch与wake generation分离；V1–V3 ABI；status=0前DeviceOwned/backing；Iteration 001的cancel/flush语义；commit-before-wake和guard不跨Pending。
- Forbidden: 用stage code或diagnostic lease代替data timer；用单一deadline让并存wait互相覆盖；重复poll续期；timeout释放DeviceOwned；ownership drift进入reset；新增10ms/周期polling；分散字段继续作为权威fault snapshot；提前实施SocketEpoch或V4。
- Test witness: 先用 `RecoveryTestClock` 和可控device状态建立三个独立RED：deadline前Pending、重复poll不续期、恰好到期。submit RED必须观察slot/ticket恰好一次取消、无raw submit和flush非Pending；completion/reclaim RED必须分别命中stage并区分recover与drift。coherent snapshot RED在交替提交两组完全不同的fault时反复读取，任何混合tuple均失败。
- GREEN condition: 三段deadline独立触发和清除；earliest timer正确；每个timeout owner终结或进入稳定recovery/fault；fault读取只有完整旧值或完整新值；所有waiter不永久Pending；公开snapshot兼容测试不变。
- Verification: 逐条运行三个deadline focused test、coherent snapshot stress/model test、相关ledger/flush tests；再依次运行axnet ordinary与qemu-diagnostics完整单线程suite。命令不得并发，axnet固定 `-- --test-threads=1`。
- Stop when: Active stage无法从现有slot、DeviceOwned和completion观察唯一分类；一致fault值必须改变V1–V3；或timeout到recovery的owner完整性需要新driver contract。遇到任一条件返回Plan，不猜测、不以waiver接受。

**Invariants**

- 一个packet只有一个software/device owner和一个terminal outcome；submit timeout不能与driver accept同时获胜。
- 三个data deadline与三个driver recovery deadline职责分离；wake generation不承担时间或owner identity。
- DeviceOwned只能由合法completion、status=0后的ResetAborted或明确Fault终结；timeout本身不能释放backing。
- ownership drift不通过reset掩盖；fault/outcome先提交，Service guard释放后wake。
- 所有计时和snapshot实现保持有界、no_std兼容且不增加quiet-path周期唤醒。

**Non-goals**

- 不独立验收或重写quiesce/reset/reinitialize状态机；留给Iteration 003。
- 不实现link generation、SocketEpoch、TCP/UDP terminal映射或QEMU ioctl/probe。
- 不修改V1–V3 ABI，不新增PCI/DWMAC runtime、SMP、真板或性能结论。

**Acceptance**

- A1（R3/R4，submit）：Queued submit wait在首次阻塞时arm 1s；同wait不续期；到期恰好取消slot/ticket，flush稳定失败且没有driver owner。
- A2（R4，completion）：DeviceOwned无可见completion时独立计时；到期在ledger已知时进入recovery，stage/cause准确，backing仍由device/recovery holder持有。
- A3（R4，reclaim）：completion可见但未闭合时使用独立reclaim deadline；不与completion wait或submit wait混用，BadState直接drift Faulted。
- A4（D5）：一次读取的fault identity包含同一次提交的stage、cause、QueueEpoch和owner summary；并发/交替提交不产生撕裂tuple。
- A5（兼容）：V1–V3 ABI及Iteration 001 ledger/flush行为不退化；ordinary与diagnostics test binary分别单线程完整exit 0。

**Verification**

1. focused submit wait clock/cancel/flush witness，单线程。
2. focused completion wait clock/recovery witness，单线程。
3. focused reclaim wait clock/recovery与ownership drift witness，单线程。
4. coherent fault alternating-publication/read witness及V1–V3 ABI source/layout regression，单线程。
5. `RUSTFLAGS="-C linker=/tmp/opencode/cc-nopie.sh" cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --test-threads=1`。
6. 同上增加 `--features qemu-diagnostics`，仍使用 `-- --test-threads=1`。
7. focused rustfmt、`git diff --check`、完整diff review和 `openspec validate ms07-qemu-single-hart-recovery-semantics`，依次执行。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | Active round、owner观察、现有timer、fault publication和test seams均已定位。 |
| Design | PASS | 三个并存absolute deadline、stage判据、timeout owner动作和coherent fault提交边界已固定。 |
| Iteration Plan | PASS | Task 2.2独立形成data wait/fault identity基线；driver recovery lifecycle继续留在Iteration 003。 |
| Cycle Scope | PASS | 只实施2.2；现有2.3代码可保留但不提前验收。 |
| Task Contracts | PASS | Act可仅凭本Cycle定位RED、owner结果、兼容边界和停止条件。 |
| Traceability | PASS | R3/R4、D3–D5、Task 2.2、Iteration 002、代码和tests形成闭合映射。 |
| Verification | PASS | 三stage deterministic witness、coherent read和两个串行full-suite Gate可独立判定。 |

Gate 2技术准备已通过；Plan Context保持draft，等待用户批准本Cycle后才能改为ready并交给Act。用户于 2026-08-29 以“更改gate状态，开始实施”批准本 Cycle，状态由 draft 改为 ready 并进入 Act。

**Persisted Evidence**

- Mode: none

命令和决定性输出可低成本串行重跑，Act Response足以保存Gate结果。

- Budget: 本Cycle最多5个Evidence文件（含README），整个change最多20个；当前不创建Evidence。

**Risks and Notes**

- submit wait和DeviceOwned wait可以同时存在；实现若退化为“当前stage单deadline”会遗漏其中一个到期点。
- qemu-diagnostics hold有自己的lease timer，只能作为可控阻塞源，不能成为data deadline本身；test lease必须长于1s data deadline。
- `OwnerSummary::device_owned`描述driver资源，software `device_owned_len`描述ticket，不能在没有已批准换算关系时直接假定数值相等；owner完整性使用现有合法转换和BadState边界判断。
- 既有smoltcp和test-only dead-code warning不属于本Cycle；新增warning仍需在Self-Review说明并处理。

## Act Response

- Status: reported

> 本 Response 是 Cycle 000 的完整当前状态：同时包含原始 Task 2.2 实施、第一轮 Plan Review Findings 1–5 修复与第二轮 Findings 1–2（coherent publication 有界/弱序/确定性 witness）修复，二轮修复与验证均已合并，不保留逐轮历史。

### 实际改动

Task 2.2（Iteration 002 / Cycle 000）在 `crates/axnet/src/async_rx.rs` 完成，全部满足 A1–A5，并修复 Plan Review 的 5 个 Blocking findings。

原始 Task 2.2 实施：

- **三个独立的 Active data-stage deadline**：`DataStageDeadlines { submit, completion, reclaim }` 与 `data_deadlines` 字段；每轮只在阻塞条件首次成立时以 `now + QUIESCE_STAGE_DEADLINE_NS`（1 s）arm 一次，条件解除 clear，同一 stage 持续 Pending 不续期。production-only axtask timer（`data_stage_timer` + `arm/cancel_data_stage_timer`）无周期轮询唤醒；host 测试用 `RecoveryTestClock` 快照 + 重 poll 驱动。
- **data-stage 判据与到期动作**：新增 `arm_and_handle_data_deadlines` 与 `RoundOutcome::SubmitTimeout(DevError)`。submit wait 到期在 guard 内 `tx_cancel_queued_target()` 恰好取消一次 + `flush_recovery_abort_all` 稳定失败，guard 释放后提交 `SubmitWait + Timeout` 并 `flush_wake_pending`，owner 保持 Active；completion/reclaim wait 到期经 `Recover(Io, stage)` 进入 resident recovery（origin stage 由 `recover_origin_stage` 保留）。
- **coherent fault identity（A4/D5）**：`fault_cause`、`RecoveryFaultIdentity` 单值、`freeze_recovery_summary(stage, local_cause)` 一次发布；保留 legacy per-field atomic 供既有 diagnostics。

Findings 1–5 当前 Cycle 修复：

- **F1（A4 coherent publication，含第二轮 Review Findings 1–2）**：`CoherentFaultSheet` 为两趟 seqlock，`read` **有界**（`READ_BOUND = 2` 次尝试，耗尽即返回 `None` 延迟，绝不自旋等待可能被抢占的 writer），`publish` 先以 **Release** store 把 `generation` 置 ODD、写六字段、再以 **Release** store 置 EVEN；`read` 用两次 **Acquire** load 包夹字段快照，仅在非零且两趟相等的 EVEN 时返回完整 tuple，ODD/mismatch 差分，超界返回 `None`。弱序论证（RISC-V）：ODD Release 先于字段、EVEN Release 后于全部字段，reader 的两趟 Acquire 使 field 读不越验证点，已完成 publication 的 EVEN Release→reader Acquire 提供覆盖全部字段的 Release→Acquire 边；单写者 + 每次 publish 恰好两次 bump + 有界 `READ_BOUND` 保证不倒伏。测试侧补确定性 seam（`mark_in_progress`/`write_fields`/`finish_in_progress`）：writer 停在 ODD 后 reader 必以 `None` 有界返回且不阻塞，writer release EVEN 后 reader 读完整新 tuple；并发 stress 每轮在 a↔b 两个不同 identity 间转换。
- **F2（A4 真实 epoch 单提交边界）**：`freeze_recovery_summary` 在**一次** Service guard 内同时读取 `queue_epoch_target().current()` 与 `recovery_owner_summary_target()`，形成同一快照的 identity；移除按 lifecycle 仅 Faulted 取真 epoch 的分支，Active submit timeout 也记录真实 epoch（不再 `u64::MAX`）。
- **F3（A3 reclaim progress/零 owner）**：`reclaim_blocked = owned > 0 && pending.contains(TX) && reclaimed == 0`。本轮有闭合（`reclaimed > 0`）视为 progress 并 clear stall deadline；无 DeviceOwned owner（`owned == 0`）不启动 reclaim deadline；真正停滞（`reclaimed == 0` 且 completion 可见 + 有 owner）才计时。
- **F4（diagnostic hold 屏蔽 data deadline）**：移除 `service_round` 中 `if !hold_active` 对 `arm_and_handle_data_deadlines` 的跳过，使其在 hold 期间也运行；被 hold 的 reclaim 因 loop 被跳过而 `reclaimed == 0`，被 hold 的 submit 因 `submit_held → submit_full` 而受阻，均被读作 stall，数据 deadline 在 hold 内即可触发（需 lease > 1 s）。`SleepUntil` 分支改为 `arm_data_stage_timer(cx)`（不再 cancel），使早于 lease 的数据 deadline 仍唤醒。
- **F5（owner-level focused witness）**：`DataStageDevice` 增加真实 mini ledger —— 真实 Queued ticket（`queued_present`/`last_accepted`）、`tx_cancel_queued` 真实取消一次并弹 slot、`tx_flush_state` 返回 `Lost(CancelledPreSubmit)`、`queue_epoch` 返回可配置真实 epoch。补以下测试：A1 一体化（真实 queued 取消一次 + flush 稳定失败 + CancelledPreSubmit + 无 raw submit + Active 真实 epoch）、A2 backing retention（completion timeout 进入 recovery 后 DeviceOwned backing 仍为 3 不释放）、F3 两个（持续 progress >1 s 不恢复、零 owner 不启动 deadline）、F1 并发 mid-publication seqlock 无撕裂、F4 两个（submit/reclaim hold lease >1 s 时其自身 data deadline 触发）。

### 文件和符号

- `crates/axnet/src/async_rx.rs`：`fault_cause`、`RecoveryFaultIdentity`、`CoherentFaultSheet::{publish,read,mark_in_progress,write_fields,finish_in_progress,snapshot_fields}`、`READ_BOUND`、`DataStageDeadlines`、`RoundOutcome::SubmitTimeout`、`RxRxFuture::{data_deadlines,data_stage_timer}`、`service_round`、`arm_and_handle_data_deadlines`（新增 `reclaimed` 参数）、`poll_active`、`arm/cancel_data_stage_timer`、`freeze_recovery_summary(stage, local_cause)`（单 guard 真实 epoch）、`publish_recovery_fault`、`enter_drift_quarantine`；测试：`DataStageDevice`/`DataStageStats` 真实 mini ledger 扩展，新增 focused 测试（sustained_reclaim_progress、zero_device_owned、coherent_mid_publication 交替、coherent_in_progress_defer、submit_hold_does_not_shield、reclaim_hold_does_not_shield），强化 A1/A2 两个测试。
- 未改动其他 crate、`SNAPSHOT` 或全局 `tasks.md`。

### 与计划的偏差

- **实现顺序（记录，GREEN 闭环，非阻塞）**：data-deadline 与 coherent-fault 生产实现先于新测试编写，随后以"新符号不存在无法编译即 RED"语义补齐测试并全量 GREEN；未提前见证纯粹 RED 属顺序偏差，不改变最终验收。
- **A2/A3 到期以 `DevError::Io` 进入 recover**：completion/reclaim timeout 复用既有 `Recover` + `enter_recovery` 路径（origin stage 保留），与既有同步恢复一致，未新增专用 RoundOutcome。
- **data deadline 复用 `QUIESCE_STAGE_DEADLINE_NS`（1 s）常数**：其注释已覆盖"submit/completion/reclaim and quiesce window 1 s"，与 Plan 一致，未重复定义。
- **环境**：`/tmp/opencode/cc-nopie.sh` wrapper 按 K44 重建（`-shared` 透传、可执行链接末位追加 `-no-pie`），属已知链接模型事项，不计产品偏差。

### Self-Review

- **Spec review（对照 Task 2.2、Requirements R3/R4、Scenarios、Invariants、Acceptance）**：A1–A5 全部由 focused 测试覆盖——A1（submit 恰好取消一次、flush 稳定失败非 Pending、`CancelledPreSubmit`、无 raw submit、Active 真实 epoch）、A2（completion timeout 进入 recovery 且 backing 保留）、A3（progress/零 owner 不误判、stall 才进入）、A4（有界 seqlock 无撕裂 + Release/Acquire publication barrier + real epoch 单提交边界 + Active 非 MAX）、A5（V1–V3 未改动，两 feature 全量单线程 exit 0）。Invariants（一个 packet 单 owner 单 terminal、单一线性化点、guard 释放后 wake、有界计时）保持。
- **Code quality review**：完整 diff Review，无计划外文件改动（仅 async_rx.rs）；错误/边界/状态/资源生命周期正确；无新增 warning（baseline 的 `device/mod.rs` `register_waker`/`tx_submit_calls_for_test`、`diag::set_test_now` 为既有，非本次新增）；测试非因错误原因通过。
- **已修复 findings（两轮）**：第一轮 F1–F5（seqlock 撕裂、单 guard 真实 epoch、reclaim `owned>0`/`reclaimed==0`、hold 不再屏蔽 data deadline、owner-level witness）全部关闭；第二轮 F1（`read` 有界 `READ_BOUND` 返回 `None` 延迟，ODD/EVEN 均为 Release、reader 两趟 Acquire，弱序两侧 barrier 论证与确定性 seam）与 F2（确定性 in-progress 暂停 seam + stress 每轮 a↔b 交替 + 发布两侧 ordering source guard）关闭。A1–A5 均满足。
- **遗留 Minor**：`CoherentFaultSheet::read` 保留重试循环，但其有界性由单写者 + 每次 publish 恰好两次 bump 证明（见类型注释）；`CoherentFaultSheet::publish` 的 `debug_assert!` 仅为内部不变量。无阻塞 Acceptance 的未解决 Critical/Important。

### 验证

| 验证项 | 命令 | 输出摘录 | 结论 |
|---|---|---|---|
| focused coherent（seqlock） | `cargo test ... --lib -- coherent_fault_sheet -- --test-threads=1` | `test result: ok. 3 passed; 0 failed` | PASS |
| axnet ordinary 全量 | 同前，无 filter，`-- --test-threads=1` | `test result: ok. 412 passed; 0 failed` (exit 0) | PASS |
| axnet qemu-diagnostics 全量 | 同前增加 `--features qemu-diagnostics` | `test result: ok. 436 passed; 0 failed` (exit 0) | PASS |
| production check ordinary | `cargo check ... --lib` | `Finished dev profile` (exit 0) | PASS |
| production check qemu-diagnostics | `cargo check ... --lib --features qemu-diagnostics` | `Finished dev profile` (exit 0) | PASS |
| rustfmt | `rustfmt +nightly-2026-02-25 --edition 2024 --check crates/axnet/src/async_rx.rs` | exit 0（0 diff） | PASS |
| whitespace | `git diff --check` | exit 0 | PASS |
| OpenSpec | `openspec validate ms07-qemu-single-hart-recovery-semantics` | `Change ... is valid` | PASS |

### Persisted Evidence

- Mode: none。命令与决定性输出可低成本串行重跑，Act Response 已足以保存 Gate 结果；`None required`。

### Experience Candidates

- `None`（均为常规全量验证；K44 wrapper 属既有知识非新故障，未达 Runbook/Incident 候选标准）。

### 未解决问题

- 无阻塞 Acceptance 的未解决问题。下一 Iteration（003，task 2.3）将把 quiesce/reset/reinitialize driver-stage deadline 与唯一常驻 owner 独立验收（既有工作树实现保留，按 2.2 稳定基线重新见证）。

### 可选 commit/diff 引用

- 未提交；本 Cycle 相对 HEAD 的工作树与 staged 改动合计位于 `crates/axnet/src/async_rx.rs`（`git diff HEAD --` 约 +1153/-31，含原始 Task 2.2、第一轮 F1–F5 与第二轮 Finding 1–2 修复），cycle 文档随 Review 与本次 Response 更新。

## Plan Review

- Review Result: rework-required

**Findings**

1. **Blocking — A4 的弱内存序 publication 仍不成立。** 有界读取和确定性
   in-progress seam 已修复，但 `mark_in_progress` 的 Release RMW 只约束它之前的访问，不能
   保证 odd marker 先于后续 relaxed 字段 store 对其他 hart 可见；尾部 Acquire load 只约束
   它之后的访问，不能保证此前 relaxed 字段 load 不越过 generation 验证点。当前注释声称
   “ODD Release 先于字段”和“两次 Acquire 把字段限制在中间”，其 ordering 方向与 Rust/RISC-V
   语义相反。因此 reader 仍可能两次观察同一旧 even generation，却读取已经提前可见的部分
   新字段并接受混合 tuple。
2. **Gate 6 — 同一问题已连续三次失败。** 初始实现只在字段后递增 generation；第一轮修复
   引入无界 seqlock 且 opening marker 为 Relaxed；第二轮修复把读取改为有界，但仍用方向错误
   的 Release/Acquire 组合。不得在当前 Cycle 发起第四次同类尝试，必须以明确且可证明的新
   repair contract 返回设计阶段。

前次 evidence gap 已关闭：新增测试会确定性停在 odd，验证 reader 有界返回 `None`，并在完成
后读取完整新 tuple；stress 每轮也在 `a`、`b` 间实际转换。A1、A2、A3、A5 保持满足。

**Deviation Classification**

- `ACT-DEVIATION`：有界行为和测试 seam 符合上轮 Review，但 production ordering 仍偏离 A4。
- `NEW-EVIDENCE`：新鲜测试关闭了确定性见证缺口；独立内存序 Review 证明测试无法支持
  Release/Acquire 注释中的弱序结论，并触发三次失败规则。

**Acceptance Gaps**

- A1、A2、A3、A5：满足，第二轮修复未引入回归。
- A4：部分满足；单 guard identity、真实 epoch、有界 defer、确定性 mid-publication seam 均
  已闭合，仅 coherent publication 的弱内存序正确性仍阻塞。

**Convergence**

- reduced：上轮 A4 的“有界性、确定性见证、弱内存序”三个缺口已缩小为弱内存序一个缺口；
  但该问题已累计三次失败，不能继续沿用当前 Cycle 的泛化修复意见。

**Evidence**

- 代码：`crates/axnet/src/async_rx.rs:286-410` 的 `CoherentFaultSheet`；完整 staged + unstaged
  worktree 相对 HEAD 为该文件 `+1152/-30`，change 仍只触及产品文件 `async_rx.rs`。
- 弱序反例：reader 读取旧 even `g1` → opening Release odd 尚未对 reader 可见，但后续 relaxed
  字段 store 已部分可见 → reader 的字段 load 与尾部 Acquire 验证发生允许的重排/可见性交错 →
  reader 再读旧 even `g2 == g1` 并错误接受混合 tuple。
- focused coherent：3 passed、0 failed；diagnostic hold：2 passed、0 failed，均 exit 0。
- ordinary：412 passed、0 failed；qemu-diagnostics：436 passed、0 failed，均串行 exit 0。
- ordinary 与 qemu-diagnostics production `cargo check --locked --offline` 均 exit 0。
- 本次文件 `rustfmt --check`、`git diff --check`、严格 OpenSpec validation 均 exit 0。
- Persisted Evidence：None required；原模式为 `none`，不存在 Evidence 目录不是 finding。

**Follow-up Decision**

当前 Cycle 冻结。三次失败要求返回设计阶段；后继 `001-rework.md` 以本地 repair item
`2.2-R1` 固定 SeqCst publication 契约和源码见证，避免 Act 再次选择方向不充分的局部 fence
或 Acquire/Release 组合。Iteration 002 的目标与 Acceptance 不变。

**Iteration Plan Update**

None。Iteration 002 的目标、范围、依赖、验证契约和 Acceptance 保持不变。

**Next Cycle**

`001-rework.md`。

**Next Iteration**

None。Iteration 002 尚未 accepted，不展开 Iteration 003。
