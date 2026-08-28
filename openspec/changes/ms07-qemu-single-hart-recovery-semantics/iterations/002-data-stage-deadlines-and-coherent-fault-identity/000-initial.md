# Iteration 002 / Cycle 000: Data-Stage Deadlines and Coherent Fault Identity

## Plan Context

- Status: draft
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

Gate 2技术准备已通过；Plan Context保持draft，等待用户批准本Cycle后才能改为ready并交给Act。

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

- Status: pending

## Plan Review

- Review Result: pending

