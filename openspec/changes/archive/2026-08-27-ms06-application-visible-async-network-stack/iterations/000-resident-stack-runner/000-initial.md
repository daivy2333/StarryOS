# Iteration 000 / Cycle 000: Resident Stack Runner

## Plan Context

- Status: ready
- Iteration: 000-resident-stack-runner
- Cycle: 000-initial
- Cycle Type: initial
- Parent cycle: None

**Iteration Scope**

- Change tasks: 1.1-1.5
- Depends on: MS05 accepted baseline
- Stable baseline: 唯一runner可由device/software/timer唤醒，stack round有界且Polling fallback/Active quiet可判定；既有socket inline path暂时保留，因此TCP/UDP/listener兼容不退化。
- Verification boundary: lifecycle、三源register-recheck、31/32/33 budgets、timer replacement、fallback矩阵、guard释放和init顺序全部由host/model tests覆盖，ordinary/qemu-diagnostics tests与QEMU/root D1 checks通过。
- Diagnostic boundary: 失败限制在StackEvent、runner lifecycle/timer、Router/Service bounded round、fallback或启动顺序。
- Deferred tasks: 2.1-3.4

**Cycle Scope**

- Trigger: initial
- Acceptance gaps: None
- Repair items: None
- Inherited scope: proposal R1-R4、R7中T09相关场景，design D1-D5/D9-D10，MS05唯一queue owner/64-frame slots/typed outcome/ticketed flush与单hart结论边界。
- Excluded scope: 删除TCP/UDP/listener inline poll、per-socket PollSet、multi-waiter、listener accept bridge、terminal readiness、MS06 guest probe、QEMU application acceptance、reset、SMP、真板、性能和全局文档维护。

**Objective**

建立后续socket readiness可直接依赖的常驻stack推进基线：Service安装后至多启动一个runner；device、software和timer事件以generation register-recheck无丢失合流；Router RX、smoltcp ingress/egress和dispatch各自有固定budget；Polling owner保持10ms兼容进展，Active IRQ-backed owner在quiet观察窗内不周期轮询，Faulted不回退第二owner。Cycle完成后仍保留旧socket inline poll/waker/timeout兼容层，避免在Iteration 001 bridge切换前产生不可运行的中间状态。

**Background**

MS05只提供queue task→stack progress hint，没有独立consumer。当前每个TCP/UDP API和`Pollable::poll()`都主动调用`poll_interfaces()`，而`Service::poll()`包含多个drain-to-empty loop。协议timer由最后一个socket waiter在`Service.timeout`中拥有；普通socket waiter和未来runner若继续共用`QUEUE_EVENT.stack_waker`，software mutation会同时扰动queue generation。T09必须先把推进、timer、budget和quiet边界闭合，T10才能安全移除caller-driven path。

**Current Baseline**

- Revision: `518acb8f82197d91ba8844c9c6a4e9eaae4b1dd7`，branch `net-k3`。
- Worktree在规划开始前只有用户已有`CLAUDE.md`修改；本change由Plan创建，Act不得覆盖或还原用户修改。
- `poll_interfaces()`以`SERVICE → SOCKET_SET`顺序持锁并`while Service::poll`到无变化。
- `Service::poll()`的Router RX、smoltcp ingress、egress和Router dispatch均可能一轮处理多个/全部工作项。
- `QueueEvent`有queue/stack两个AtomicWaker但共享generation；普通`software_nudge()`只唤醒queue owner。
- `Service::register_waker()`计算`poll_at`与10ms device fallback，将唯一timeout future存回Service，并注册最后一个stack/socket waker。
- queue lifecycle为`Polling → Spawned → Active → Faulted`或`Spawned → Unavailable`；Active/Faulted保留async ownership，其他状态为polling owner。
- axruntime顺序为scheduler init → driver init → `axnet::init_network` → interrupt init → kernel main；因此Service安装后spawn安全，runner实际调度时interrupt init已在当前main task继续执行。
- Fresh baseline：ordinary axnet lib tests 218/218、qemu-diagnostics 238/238、`cargo check --locked --offline -p starry-kernel --features qemu` exit 0。
- `cargo check ... -p starry-kernel --features lichee-d1`不是受支持的独立feature组合，当前因root提供的平台/task组合缺失产生25个既有unresolved imports；本Cycle只用root `lichee-d1`组合检查，不能把该无效命令当产品failure或PASS。

**Current-State Evidence**

- 启动入口：`crates/axnet/src/lib.rs::init_network`在构建Router/Interface后以`SERVICE.call_once`安装Service；没有runner start。
- 同步推进入口：同文件`poll_interfaces`读取`RX_LIFECYCLE.owner_view()`，取得Service和SocketSet并循环`Service::poll`。
- Service stage：`service.rs::Service::poll`调用`router.poll`、`iface.poll_maintenance`、`LISTEN_TABLE.reconcile`、`poll_ingress_single` loop、`poll_egress` loop、`router.dispatch`；RX slot space和TX slot first-enqueue会发布queue owner event。
- Router无界边：`router.rs::Router::poll`对每device执行`while !rx_buffer.is_full()`；`Router::dispatch`对tx_buffer执行外层loop直到empty/full/fault。
- queue event：`async_rx.rs::QueueEvent::publish_event`wake queue+stack；`publish_queue_work`只wake queue；`publish_progress`wake stack但仍增加shared generation。
- queue task调度模板：`RxRxFuture::poll_active`已证明“register outside Service lock → bounded service under guard → drop guard → self-wake/Pending”，可复用结构但不得复用queue lifecycle或waker。
- timer现状：`Service::register_waker`在Service guard内调用`iface.poll_at(timestamp, &SOCKET_SET.inner.lock())`，替换`self.timeout`并poll `sleep_until`；多个waiter会覆盖timer owner。
- fallback事实：`EthernetDevice::requires_polling()`仅根据driver irq capability；QEMU IRQ注册/preflight失败时设备仍可能声明irq number，因此runner必须同时检查queue lifecycle。
- task启动：`async_rx.rs::start_rx_task/start_with`提供CAS+injected spawn测试模式；kernel只在IRQ handler成功注册后调用它。
- mutation来源：`wrapper.rs::SocketSetWrapper::add`只notify无consumer的`new_socket Event`；TCP/UDP send/connect/listen/close后主要依赖随后的同步poll。本Cycle只需要最小StackEvent software seam和测试，不做全量mutation cutover。
- 已知锁序：runner目标顺序为Service→SocketSet；`tcp.rs::connect`当前在`with_smol_socket`的SocketSet guard内调用`get_service().iface.context()`，该反序在Iteration 001必须修复。本Cycle不得让新runner在持锁跨Pending时放大它。
- 测试入口：`async_rx.rs`已有QueueEvent/lifecycle/budget/fault tests，`service.rs`有deadline/fallback pure helper tests，`router.rs`和`device/tests.rs`有Full/fault/slot语义，均可扩展而不触碰production globals。

**Relevant Code**

| File / Symbol | Current Responsibility | Cycle Use |
|---|---|---|
| `crates/axnet/src/lib.rs::init_network/poll_interfaces/SERVICE` | Service安装与caller-driven推进 | 安装唯一runner，保留兼容helper |
| `crates/axnet/src/async_rx.rs::QueueEvent/RX_LIFECYCLE` | queue owner事件与生命周期 | device progress委托独立StackEvent；queue contract保持 |
| `crates/axnet/src/stack_runner.rs` | 不存在 | StackEvent、lifecycle、timer、future、telemetry |
| `crates/axnet/src/router.rs::poll/dispatch` | 无界RX与TX dispatch | 单步/有界API、cursor、结构化outcome |
| `crates/axnet/src/service.rs::poll/register_waker` | 无界stack round与每waitertimer | 新有界round；旧register/timeout暂保兼容 |
| `crates/axnet/src/listen_table.rs::reconcile` | hidden listener状态推进 | 每round一次；本Cycle不加accept bridge |
| registry `axruntime::rust_main` | scheduler/driver/net/interrupt初始化 | 启动顺序事实，不修改registry源码 |

**Critical Path**

当前路径：

```text
socket API / Pollable::poll
  -> poll_interfaces
  -> SERVICE lock
  -> SOCKET_SET lock
  -> Service::poll drain loops
  -> optional caller waker/timer
```

本Cycle新增并行推进路径：

```text
init_network installs SERVICE
  -> runner lifecycle CAS -> spawn axnet-stack-runner

queue progress / test software event / poll_at timer
  -> StackEvent Release generation + wake
  -> runner Acquire snapshot/register
  -> SERVICE -> SOCKET_SET
  -> bounded Router RX / maintenance / listener / ingress / egress / dispatch
  -> compute poll_at + lifecycle fallback
  -> drop all guards
  -> arm timer / generation recheck / self-yield / Pending
```

旧socket路径在本Cycle仍可同步poll。新runner与它通过同一Service/SocketSet locks串行化；任何future guard不得跨调度点。

**Implementation Guidance**

严格按1.1→1.2→1.3→1.4→1.5执行：先闭合event/lifecycle，再有界化Router和Service，之后实现future/timer，最后接入初始化。不要先删除旧`Service::register_waker`、`timeout`或socket内`poll_interfaces()`；它们是Iteration 000结束时的显式兼容层，Iteration 001 Task 2.4才原子移除。

`STACK_STAGE_BUDGET`固定32。Router结果不得继续用单bool表达loopback-ready、backlog、fault和work count；局部类型名由Act决定，但测试必须直接看到每stage processed/budget-hit/backlog。Router RX cursor至少覆盖当前loopback+target device，不宣称多物理接口扩展。

StackEvent必须独立于QueueEvent generation。MS05 queue task在slot progress/fault后可调用StackEvent，但普通socket mutation的完整接入延期；本Cycle可提供`publish_software` seam与单元测试。timer future属于runner struct，不放回Service。fallback deadline取`min(poll_at, lifecycle/device fallback)`。

所有`Context::waker`注册、timer poll和wake发生在网络guard外。runner从round返回后先drop guard，再执行self-wake或Pending。host future使用Injected Service/fake timer；production global不被tests推进。

**Behavioral Change**

- 新增常驻runner及telemetry；协议栈可在没有当前socket调用的情况下推进。
- Active IRQ-backed quiet path不再需要runner的10ms fallback；Polling/Spawned/Unavailable仍保留。
- stack每stage由无界drain改为固定32预算，剩余工作self-yield。
- queue owner、descriptor/slot/ticket ownership不变。
- socket外部行为在本Cycle不切换：caller-driven poll、single-slot waker和Service timeout仍保留，T10语义不在本Cycle声明完成。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| 1.1 | R1 unique start；R2 event race | `stack_runner.rs`, `lib.rs`, `async_rx.rs` | 无runner；shared stack generation | 独立event/lifecycle/spawn seam与device progress委托 |
| 1.2 | R3 budget/fairness；MS05 ownership | `router.rs::poll/dispatch` | drain loops | bounded step、cursor、outcome |
| 1.3 | R3 all stages；R4 guard | `service.rs::Service::poll` | 无界组合poll | budgeted stack round和结构化结果 |
| 1.4 | R2 timer/recheck；R3 quiet/fallback | `stack_runner.rs::Future` | 不存在 | timer、generation、lifecycle fallback、telemetry |
| 1.5 | R1 startup；R7 compile regression | `lib.rs::init_network`, Cargo feature paths | 只安装Service | spawn runner并验证init/feature边界 |

**Task Contracts**

### 1.1: 独立StackEvent与唯一runner生命周期

- Requirement/Scenario: R1启动/重复启动；R2 device/software publication与注册交错。
- Depends on: None。
- Targets: `crates/axnet/src/stack_runner.rs`（新）、`lib.rs` exports、`async_rx.rs` queue progress call sites/tests。
- Current behavior: stack role是QueueEvent内一个AtomicWaker，shared generation；没有runner lifecycle或spawn入口。
- Required behavior: 独立generation+waker只保存唯一runner；Release publish、Acquire snapshot/recheck；CAS唯一spawn；tests不修改production global。
- Required changes: 提供device/software publish、generation/register API、start error和Injected spawn seam；queue slot progress/fault在commit后委托device publish，普通queue-work contract不变。
- Preserve: QueueEvent queue waker、waiting bit、queue register-recheck、RX lifecycle状态码和MS05 telemetry ABI。
- Forbidden: socket multiwaiter、全局PollSet、修改queue owner lifecycle、在wake前发布未提交状态。
- Test witness: 当前代码无法表达独立software event或runner start；新增counting waker测试event-before-register、register-during、wrap、1次/重复/竞争start必须先RED。
- GREEN condition: device/software event分别使generation精确wrapping增加并wake已注册runner；重复start不执行spawn closure；production global保持初始状态。
- Verification: axnet ordinary/qemu-diagnostics targeted tests及100×event interleaving全部exit 0；QueueEvent既有tests不变通过。
- Stop when: 唯一runner需要多个stack waker、socket event必须增加queue generation，或test无法避免推进production global。

### 1.2: Router有界RX/dispatch与device公平

- Requirement/Scenario: R3 exact budget、continuous traffic；modified MS05 slot consumer/owner保持。
- Depends on: 1.1接口可用，但实现逻辑不依赖runner future。
- Targets: `crates/axnet/src/router.rs::{poll,dispatch}`及router/device fake tests。
- Current behavior: RX对每device循环到empty/full，dispatch循环到tx_buffer empty/full/fault；返回信息不足以区分backlog和loopback wake。
- Required behavior: 单次/有界API每stage最多32项，持久cursor让loopback和target均获服务，outcome显式包含work/backlog/rx-ready/fault。
- Required changes: 重构loop而不改变packet peek→preflight→commit→dequeue、Full保留队首、Drop计数一次、Fault稳定和ticket ownership。
- Preserve: 64-frame slots、typed TX outcomes、ARP deferred obligation、Router buffer容量、MS05 counters/flush/descriptor边界。
- Forbidden: drain-to-empty、动态扩容、把Full改fatal/drop、让stack持descriptor/token。
- Test witness: fake loopback持续ready+target packet、31/32/33 RX/dispatch、Full/fault/ticket守恒tests先RED或锁定旧GREEN。
- GREEN condition: 单调用≤budget；budget+1显式backlog；双device有限轮次内都progress；所有MS05 router/device/flush tests通过。
- Verification: targeted router/device/flush tests，随后两组axnet lib tests；任一ownership counter漂移为失败。
- Stop when: 保持typed ownership必须完成无界loop，或cursor需要承诺本change排除的多接口调度策略。

### 1.3: Service每stage有界round

- Requirement/Scenario: R3 stage fairness、budget；R4锁生命周期；listener reconcile兼容。
- Depends on: 1.2 GREEN。
- Targets: `crates/axnet/src/service.rs::Service::poll`及tests。
- Current behavior: 一个bool包含socket/dispatch变化；ingress/egress loop无budget；caller外层仍while。
- Required behavior: Router RX、ingress、egress、dispatch各≤32且同轮都有机会；maintenance/listener各一次；返回self-yield、deadline inputs、space release、socket change、fault等结构化结果。
- Required changes: 用1.2 bounded APIs，保留RX telemetry delta、LISTEN_TABLE reconcile位置、RX space wake、first TX-slot queue wake和flush/fault路径。
- Preserve: Service→SocketSet调用约束、queue waiting bit、diagnostic lease/flush、MS05 snapshot字段。
- Forbidden: 在Service round内await/yield/wake application waiter，跳过后续stage，或把telemetry作为同步条件。
- Test witness: 每stage 31/32/33、前stage budget-hit仍执行后stage、RX full→space、first TX enqueue和fault tests先RED。
- GREEN condition: outcome精确描述剩余工作；任何stage单轮不超32；现有service deadline/lease/flush tests通过。
- Verification: service/router tests和两组axnet lib tests；source review无新增无界while数据路径。
- Stop when: smoltcp API无法单步egress/ingress，或有界round需要改变listener/backlog/socket语义。

### 1.4: Runner future、timer、fallback与quiet

- Requirement/Scenario: R2三源wake/timer/interleaving；R3Active quiet、Polling fallback、Faulted no fallback；R4no guard across Pending。
- Depends on: 1.1、1.3 GREEN。
- Targets: `stack_runner.rs` future、fake ServiceAccess/clock/timer、telemetry tests。
- Current behavior: 没有runner；每socket waiter覆盖Service timeout并按device `requires_polling`选择10ms。
- Required behavior: runner按snapshot→register→bounded round/arm→generation recheck；本地timer跟随`poll_at`；Polling/Spawned/Unavailable或polling device用10ms，Active IRQ无tick，Faulted只报告错误；guard释放后self-wake/Pending。
- Required changes: fakeable time/deadline seam、timer replacement/elapsed处理、lifecycle矩阵、poll/work/budget/wake/fallback counters；旧Service timeout暂不删除。
- Preserve: wall-time转换、queue lifecycle owner view、spurious wake允许、single-hart结论。
- Forbidden: 持guard poll timer/return Pending、blocking sleep、Active固定tick、Faulted raw polling、unbounded self-wake。
- Test witness: deadline earlier/later replacement、stale wake、generation races、Active idle、Unavailable fallback、Faulted no fallback、budget self-yield的future-level RED tests。
- GREEN condition: 可推进work在所有交错中最终被poll；quiet counter稳定；fake time达到deadline只wake一次有界round；100×重复稳定。
- Verification: targeted future tests×100、普通/qemu-diagnostics全量tests；source assertion检查drop guard先于wake/Pending。
- Stop when: timer必须由socket waiter拥有、Active仍需10ms才能收包，或ServiceAccess不能在Pending前释放。

### 1.5: 初始化接入与T09兼容Gate

- Requirement/Scenario: R1 Service-ready start、pre-init、duplicate；R7automatic compile boundary。
- Depends on: 1.1-1.4 GREEN。
- Targets: `crates/axnet/src/lib.rs::init_network`、runner exports/telemetry、root feature checks。
- Current behavior: init只安装Service；kernel后续只启动queue task；socket调用主动poll。
- Required behavior: Service安装后唯一spawn runner；queue IRQ激活前fallback，激活后event/quiet；旧socket inline path、Service register/timeout在本Iteration保留并通过兼容tests。
- Required changes: 接入start并记录bounded diagnostic；增加pre-Service/repeated init test seam和T09 snapshot，不改变既有V1/V2/V3 layout；必要时只追加内部test observation。
- Preserve: axruntime registry依赖不本地化、kernel queue start顺序、QEMU诊断ABI、D1代码不引入QEMU cfg。
- Forbidden: 修改kernel为第二runner启动点、提前删除inline poll、修改全局tasks/SNAPSHOT、以invalid standalone D1命令判定产品。
- Test witness: start-before-Service安全结果、Service-ready one spawn、duplicate no second task、ordinary TCP/UDP/listener regression和feature compile tests。
- GREEN condition: runner随init安装，旧socket测试全绿；QEMU kernel check和root-supported D1 check通过；git diff无产品socket cutover。
- Verification: 两组axnet lib tests、`cargo check --locked --offline -p starry-kernel --features qemu`、root `cargo check --locked --offline --features lichee-d1`或项目等价受支持命令、strict OpenSpec和full diff review。
- Stop when: scheduler并未在init_network前可用、runner必须等kernel IRQ注册才能安全存在，或兼容必须提前实现T10 bridge。

**Invariants**

- queue service仍唯一访问VirtIO descriptor/token/reclaim/queue-control；runner只通过Router/device adapter和slots。
- Active/Faulted保持async owner，Unavailable保留polling owner；本Cycle不增加恢复transition。
- 每stage budget固定32，达到budget不丢工作；telemetry不参与同步。
- StackEvent只唤醒runner，普通software event不唤醒queue task。
- wake/arm/Pending前释放Service、SocketSet和listener guards。
- MS01 TCP/UDP/listener、MS04 queue modes、MS05Full/flush/fault与V1/V2/V3 ABI保持。
- 用户`CLAUDE.md`修改、全局OpenSpec状态和归档不在Cycle写入范围。

**Non-goals**

- Tasks 2.1-3.4和所有T10最终readiness语义。
- 删除或弃用public `poll_interfaces()`、Service timeout或GeneralOptions register。
- 多waiter、PollSet overflow、listener hidden socket bridge、ERR/HUP/RDHUP修订。
- guest probe、runtime Evidence、reset/SMP/真板/性能。

**Acceptance**

1. R1/D1/Task1.1+1.5：Service-ready唯一spawn和重复start tests GREEN；runner固定名称且pre-init不panic/pend。
2. R2/D2-D3/Task1.1+1.4：device/software/timer与关键register交错100×无lost wake；timer由runner拥有但legacy socket timer暂保兼容。
3. R3/D4/Task1.2-1.4：Router RX、ingress、egress、dispatch的31/32/33 tests GREEN；持续backlogself-yield且后stage获机会。
4. R3 fallback/quiet：Polling/Spawned/Unavailable在fake 10ms推进；Active IRQ idle observation无poll增长；Faulted不raw poll。
5. R4/D5：future在所有Pending/self-wake/timer路径前释放guard；无新反向锁序。
6. MS05 delta：slots/typed outcome/ticket/flush/queue owner回归GREEN；runner不持transport object。
7. R7/D10：两组axnet tests、QEMU kernel check、受支持root D1 check、fmt/source/strict OpenSpec/full diff通过，无Critical/Important finding。
8. Iteration边界：TCP/UDP/listener旧inline poll仍在；不得错误宣称T10或应用可见multiwaiter完成。

**Verification**

- `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib`
- `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib --features qemu-diagnostics`
- 对StackEvent/runner future register-recheck与锁竞争targeted tests重复100次，要求每次exit 0且计数一致。
- `cargo check --locked --offline -p starry-kernel --features qemu`
- `cargo check --locked --offline --features lichee-d1`，或仓库现行等价root D1 build命令；不得使用已知无效的kernel-only feature命令替代。
- `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check`
- source assertions：stack数据路径无新增无界drain；guard在wake/Pending前drop；普通software event不调用queue-only publish；socket inline poll仍保留到下一Iteration。
- `openspec validate ms06-application-visible-async-network-stack --strict`
- `git diff --check`与full diff review；用户已有`CLAUDE.md`修改不归因于本Cycle。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | 已定位init/runtime顺序、Service/Router全部推进loop、QueueEvent/lifecycle、timer/fallback、socket mutation和测试seam |
| Design | PASS | D1-D5明确唯一runner、独立StackEvent、timer owner、budget、锁序与compatibility staging；无实施语义未知项 |
| Iteration Plan | PASS | 13 tasks分配到3个依赖有序Iteration；000只形成T09 resident baseline，T10 bridge与terminal/QEMU独立后置 |
| Cycle Scope | PASS | initial Cycle只含1.1-1.5；明确保留legacy socket path，排除T10和runtime acceptance |
| Task Contracts | PASS | 每项包含当前/目标行为、必须/保持/禁止、RED/GREEN、验证和停止条件，Act无需回读Explorer |
| Traceability | PASS | tasks.md RTM无Missing/Simplified；本Cycle覆盖R1-R4、R7的T09边界和MS05 owner delta |
| Verification | PASS | host/model、100×interleaving、ordinary/diagnostic、QEMU/root D1 compile、source/OpenSpec/diff Gate已定义 |

- Approval: 用户于 2026-08-21 回复“批准”，批准本 Cycle 的 Execution Readiness。

**Persisted Evidence**

- Mode: none

Act Response记录命令、关键输出、exit和revision即可；本Cycle没有昂贵或不可低成本复现的runtime Gate。

**Risks and Notes**

- Iteration 000同时存在runner与legacy inline poll，但两者由同一Service/SocketSet locks串行；这是明确的迁移态，不是最终single progress API声明。
- root D1只做受支持feature编译兼容，不声明真板网络runtime。
- 若runner启动早于interrupt init导致实际timer primitive不可安全构造，命中Task1.5 stop条件并返回Plan，不把启动点私自移到kernel。
- 当前worktree中的`CLAUDE.md`修改属于用户，Act full diff必须分开审计。

## Act Response

- Status: blocked

**Implemented**

- Task 1.1：新增独立 `StackEvent`、Release/Acquire generation、唯一 runner lifecycle、固定 task name 和可注入 spawn seam；device/software publication 不修改 queue generation。
- Task 1.2：Router RX 与 dispatch 改为预算 32 的有界推进，RX 使用持久 round-robin cursor，并保留 Full、drop、fault、ticket 和 packet ownership 语义。
- Task 1.3：Service 按固定顺序执行有界 stack round，返回 work、backlog、self-yield、socket change、space wake、TX enqueue、fault、protocol deadline 和 polling requirement。
- Task 1.4：实现 generation register-recheck、runner-owned `poll_at` timer、10 ms lifecycle fallback、Active IRQ quiet 和 Faulted no-fallback；wake、timer arm 和 Pending 均在 guard 释放后发生。
- Task 1.5 的代码部分：Service 安装后启动唯一 runner，导出只读 T09 snapshot，并保留本 Iteration 的 socket inline poll、register_waker 和 timeout 兼容层。该 task 因受支持 D1 Gate 未通过而未标记完成。

**Changed Files and Symbols**

- `crates/axnet/src/stack_runner.rs`：`StackEvent`、`StackRunnerLifecycle`、`StackRunnerFuture`、`StackTelemetry`、`StackSnapshot`、`start_stack_runner`。
- `crates/axnet/src/async_rx.rs`：`RxRxFuture::stack_notify`、guard 释放后的 stack progress/fatal publication。
- `crates/axnet/src/router.rs`：`RouterRxOutcome`、`RouterDispatchOutcome`、`Router::poll_bounded`、`Router::dispatch_bounded`、`rx_cursor`。
- `crates/axnet/src/service.rs`：`STACK_STAGE_BUDGET`、`StackRoundOutcome`、`Service::stack_round`。
- `crates/axnet/src/lib.rs`：runner module/export 与 `init_network` 启动顺序。
- `crates/axnet/src/device/tests.rs` 及上述模块 tests：31/32/33、round-robin、公平性、event/lifecycle/timer/quiet/lock 和 snapshot witnesses。
- `openspec/changes/ms06-application-visible-async-network-stack/tasks.md`：仅将已完成的 1.1–1.4 标记为完成；1.5 保持未完成。

**Deviations from Plan**

- root D1 原命令必须补充 RISC-V target，宿主 target 会在 `sbi-rt` 报无效寄存器。补充 target 后，仓库的 `lichee-d1-smoke` feature 组合仍因裁掉 `kernel::drivers` 却继续导入 `drivers::critical_section_policy` 而失败。
- 仓库正式 `make ... APP_FEATURES=lichee-d1 ... build` 同样复现该条件编译错误。这些路径没有编译到 axnet，未对 Cycle 代码作越界修复。
- Act 沙箱中的 `cargo-binutils` 安装告警来自只读 `$HOME` 与受限网络造成的依赖探测假阴性，不是产品工具链缺失。用户在正常宿主环境执行默认 QEMU `make build` 已成功生成 ELF/bin；该结果只解除工具链疑点，不替代 D1 Gate。

**Blocker Handoff**

- Task/step/Gate：Task 1.5，受支持 root D1 compile，Acceptance 7 / Gate 6。
- Plan 预期：root D1 支持组合 exit 0。
- 实际：不带 target 的 root 命令在宿主架构触发 `sbi-rt` invalid-register，属于无效 D1 诊断，不计产品 Gate。显式使用 `riscv64gc-unknown-none-elf` 后，`kernel/src/lib.rs:66` 报 `E0432/E0433`；仓库正式 D1 make build 复现相同错误。根因是提交 `661f6fcd8` 将 shared critical-section policy 放入 `drivers`，而 `lichee-d1-smoke` 会排除整个 `drivers` 模块。
- 影响：Tasks 1.1–1.4 已完成；Task 1.5 代码和除 D1 外的 Gate 已完成，但 Acceptance 7 未闭合，Cycle 不能 reported。
- 建议的 rework 边界：把 `critical_section_policy` 提升为不受 `lichee-d1-smoke` 排除的 kernel shared module，保持 production impl 与 MS04 host harness 复用同一 policy；不得把完整 drivers 子系统拉回 smoke，也不得简单删除 smoke 下的 critical-section impl。修复后重跑 MS04 harness、root D1 check、`make lichee`、QEMU kernel check 和本 Cycle 完整 Gate。
- 恢复条件：`openspec-plan` Review 当前 blocked Response，将该基线变化分类并在同一 Iteration 创建 rework Cycle。若用户改为停止维护 D1，则属于支持范围和 Acceptance 变化，必须 replan，不能在 Act 中直接删除 D1 Gate。
- Evidence：None required；错误可由下列命令和两行 kernel 条件编译关系低成本复现。

**Blocker Resolution**

None. 用户手工 `make build` 只纠正了工具链判断；D1 产品 blocker 仍存在，因此本 Cycle 不恢复为 `pending`。

**Self-Review**

- Plan compliance: BLOCKED（受支持 D1 Acceptance 未通过）
- Full diff reviewed: PASS
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 0

完整代码 diff 与新增文件已审查；未发现 Cycle 范围内的 Critical、Important 或 Minor finding。用户已暂存的 `openspec/specs/improvements/spec.md` 未修改、未归因于本 Cycle。

**Verification Evidence**

- Revision：`b8e7bcae27579aa7ea7bf31698e3136f5856302d`；工作区实现未提交。
- `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib` → exit 0，239 passed。
- `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib --features qemu-diagnostics` → exit 0，259 passed。
- StackEvent/runner register、lifecycle、timer、quiet 和 lock targeted tests ×100 → 每次 exit 0；ordinary 与 qemu-diagnostics targeted suites 均通过。
- `cargo check --manifest-path crates/axnet/Cargo.toml --locked --offline` 及 `--features qemu-diagnostics` → exit 0。
- `cargo check --locked --offline -p starry-kernel --features qemu` → exit 0；仅证明 QEMU kernel compile，不是 runtime evidence。
- `cargo check --locked --offline --features lichee-d1` → exit 101，宿主 target 下 `sbi-rt` 43 个 invalid register；命令缺少 D1 target，不计产品 Gate。
- `cargo check --locked --offline --target riscv64gc-unknown-none-elf --features lichee-d1` → exit 101，`kernel/src/lib.rs:66` 的 `E0432/E0433`。
- `make ARCH=riscv64 APP_FEATURES=lichee-d1 MYPLAT=axplat-riscv64-lichee-d1 PLAT_CONFIG=$PWD/crates/axplat-riscv64-lichee-d1/axconfig.toml MEM=512M BUS=mmio DWARF=n build` → exit 2，复现相同 kernel 错误；沙箱中的 `cargo-binutils` 探测告警是环境噪声。
- 用户于 2026-08-23 在正常宿主环境执行默认 `make build` → exit 0，release profile 完成并由 `rust-objcopy` 生成 `StarryOS_riscv64-qemu-virt.bin`；证明本机工具链和 QEMU 产品构建正常，不证明 D1 feature。
- `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check`、`git diff --check` → exit 0。
- source assertions → PASS：新增 stack 数据路径仅含预算受限循环；guard 在 wake/Pending 前释放；software event 不调用 queue-only publication；TCP/UDP inline poll 和 register_waker 仍存在。
- `openspec validate ms06-application-visible-async-network-stack --strict` → exit 0，change valid。

**Persisted Evidence**

None required.

**Experience Candidates**

None.

**Remaining Issues**

- 当前 blocker：root D1 支持组合因 shared critical-section policy 的模块归属与 `lichee-d1-smoke` 条件编译冲突而无法编译；工具链不再是 blocker。
- Tasks 2.1–3.4 未开始，仍属于后续 Iteration；本 Cycle 未宣称 T10 multiwaiter/readiness 或 QEMU runtime 完成。

**Commit or Diff Reference**

- Working-tree diff at revision `b8e7bcae27579aa7ea7bf31698e3136f5856302d`; no commit created.

## Plan Review

- Status: reviewed

**Review Result**

rework-required

**Findings**

独立检查当前代码、完整 diff、Act Response、Self-Review 和 fresh 自动验证后，Tasks
1.1–1.4 的产品实现可以保留，但当前 Iteration 的 Acceptance 2、3、7 尚未全部满足。

1. **Important — D1 产品 Gate 仍失败。** 根 `lichee-d1` feature 会启用
   `starry-kernel/lichee-d1-smoke`；该 mode 在 `kernel/src/lib.rs` 排除整个
   `drivers`，无条件编译的 `critical_impl` 却仍导入
   `drivers::critical_section_policy`。显式 RISC-V target check 稳定报
   `E0432/E0433`。该错误在 Plan revision 前已经存在，但 Plan 没有运行获批契约要求
   的 root D1 基线，因此不是 MS06 Act 引入的回归，也不能豁免 Acceptance 7。
2. **Important — future-level interleaving/timer witness 不完整。** 当前
   `event_before_register_is_seen_by_generation_recheck` 和
   `event_during_register_window_wakes_and_changes_generation` 只直接测试 `StackEvent`；
   `deadline_selection_replaces_earlier_and_later_deadlines` 只测试 deadline 的 `min`。
   100×重跑能证明这些静态用例稳定，但没有让事件穿过
   `StackRunnerFuture::poll` 的 snapshot/register/round/recheck 路径，也没有验证实际
   timer replacement 与 stale deadline 不触发。Task 1.4 和 Acceptance 2 明确要求
   future-level register interleaving、replacement 和 stale-wake 见证。
3. **Important — 完整 stack round 的 stage/fault witness 不完整。** 当前
   `run_bounded_stage` 的 31/32/33 tests 证明单个 helper 有界；既有 RX-space 与
   TX-enqueue tests 证明各自路径，但没有直接证明 Router RX 达到 32 后，后续
   ingress/egress/dispatch 仍执行，也没有断言 `StackRoundOutcome::faulted`。Task 1.3
   和 Acceptance 3 要求前 stage 达预算不跳过后 stage及 fault propagation。
4. **PASS — 已实现的数据面和 runner 边界可保留。** Router/Service 循环均受 32
   budget 约束；device progress 在释放 Service guard 后发布独立 `StackEvent`；
   stack runner 在 wake、timer arm 和 Pending 前释放 Service/SocketSet guard；legacy
   socket inline poll、queue owner、64-frame slots、typed outcome、ticketed flush 和
   V1/V2/V3 ABI 未被切换。
5. **PASS — 未发现新的产品代码 Critical/Important finding。** ordinary 239/239、
   qemu-diagnostics 259/259、MS04 host harness 16/16、QEMU kernel check、strict
   OpenSpec 和 diff check 均通过。现有 warning 不属于本 Cycle 新增 Acceptance gap。
6. **Worktree note — 用户内容保持隔离。** Review 期间工作树还出现了 SNAPSHOT、
   regression Runbook、knowledge、references 与 improvements 修改；它们不属于本
   Cycle。Review 未修改、未归因这些路径，也不把 index 状态当作提交边界。

**Deviation Classification**

- `PLAN-OMISSION`：Plan 在记录 root D1 为必过 Gate 时没有建立该受支持 feature
  组合的 fresh baseline，遗漏了早已存在的 shared policy 模块可达性错误。
- `ACT-DEVIATION`：Act 报告 Task 1.3/1.4 的计划内 stage/fault 与 future-level
  interleaving/timer witnesses 已完成，但实际 tests 只覆盖单 helper 或纯函数层。
- `NEW-EVIDENCE`：本次 Review fresh 复验确认 axnet、MS04 host harness、QEMU compile
  通过，D1 target compile 以同一 `E0432/E0433` 失败，且现有 runner suite 100×稳定。

**Acceptance Gaps**

1. Acceptance 7 / Task 1.5：受支持 root D1 feature 组合必须在 RISC-V target 下编译
   通过，同时保持 QEMU 与 MS04 critical-section restore witness。
2. Acceptance 2 / Task 1.4：必须用完整 `StackRunnerFuture` 路径证明 generation
   interleaving 会 self-wake/retry，并证明实际 timer replacement、stale deadline 和
   exactly-once expiry。
3. Acceptance 3 / Task 1.3：必须证明前 stage 达到 budget 后不跳过后 stage，并让
   Router RX/TX fault 显式出现在 `StackRoundOutcome`。

**Convergence**

N/A（initial Cycle）。三个缺口均已定位到原 Task 1.3–1.5，已有实现和通过的回归可
保留；没有目标、范围、依赖或验收边界变化。

**Evidence**

- `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib` → exit 0，
  239 passed。
- 同命令增加 `--features qemu-diagnostics` → exit 0，259 passed。
- ordinary 与 qemu-diagnostics 的 `stack_runner::tests::` 各重复 100 次 →
  100/100 PASS；测试清单确认缺少完整 future interleaving 和 timer stale/replacement
  场景。
- `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs ...` 并执行 →
  exit 0，16 passed。
- `cargo check --locked --offline -p starry-kernel --features qemu` → exit 0。
- `cargo check --locked --offline --target riscv64gc-unknown-none-elf --features lichee-d1`
  → exit 101，`kernel/src/lib.rs:66` 的 `E0432/E0433`；`drivers` 被
  `lichee-d1-smoke` cfg 排除。
- `openspec validate ms06-application-visible-async-network-stack --strict`、
  `git diff --check` → exit 0。
- Persisted Evidence 仍为 `none`；以上结果均可低成本重跑，缺少 Evidence 目录不是
  finding。

**Follow-up Decision**

在当前 `000-resident-stack-runner` Iteration 创建一个 rework Cycle。三个 repair
item 只关闭既有 Acceptance：提升 shared critical-section policy 的模块可达性、补齐
完整 stack-round stage/fault witnesses、补齐 future-level generation/timer witnesses。
不新增全局 task，不修改 Iteration Map，不提前进入 readiness bridge。

**Iteration Plan Update**

None。Iteration 000 的目标、范围、依赖、稳定基线和验收边界保持不变。

**Next Cycle**

`001-rework.md`

**Next Iteration**

None。Iteration 000 尚未 accepted；不得创建 Iteration 001。
