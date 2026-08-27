# Iteration 006 / Cycle 000: isolate axnet host-test state

## Plan Context

- Status: draft
- Approval: pending — Gate 2 技术检查项已闭合，等待用户明确批准；本次 Plan 不构成 Act 授权
- Iteration: 006-axnet-host-test-isolation
- Cycle: 000-initial
- Cycle Type: initial
- Parent iteration: `005-application-witness-construction`

**Iteration Scope**

- Change tasks: 5.1-5.2
- Depends on: Iteration 005 accepted
- Stable baseline: public socket tests使用各自的socket/listener context，diagnostic tests使用各自的clock/state；
  ordinary与qemu-diagnostics默认并行套件不再出现陈旧handle、hashbrown断言、进程信号或随机hold分支。
- Verification boundary: R57失败子集、local-context churn、diagnostic clock交错、目标flake以及两profile默认
  并行full suites重复通过；不以串行full suite、skip、ignore或失败后无限重跑代替。
- Diagnostic boundary: test fixture注入、global reader/writer边界、SocketHandle生命周期、listener context、
  diagnostic clock/lease/telemetry；失败首次越过该边界时停止。
- Deferred tasks: Iteration 007 Task 6.1；Iteration 008 Tasks 7.1-7.2

**Cycle Scope**

- Trigger: Iteration 005 accepted 后展开既有Map
- Acceptance gaps: R57进程级socket/listener共享状态竞争；qemu-diagnostics fake-clock flake
- Inherited scope: Tasks 5.1-5.2；R57；产品static singleton、handle生命周期、锁序与socket语义
- Excluded scope: 修改产品readiness/terminal/PollSet语义；全局串行full suite；QEMU runtime；automatic
  integration qualification；scheduler、reset、SMP、真板、性能和commit

**Objective**

消除host测试之间的进程级可变状态共享，使默认并行ordinary和qemu-diagnostics结果能作为Iteration 007
自动Gate的可信输入，同时保持产品仍使用唯一`SOCKET_SET`、`LISTEN_TABLE`和系统时钟。

**Current Baseline**

- Branch `net-k3`；HEAD `1ea51427d8692f5a12b87a0403b940e73d43fed3` 加当前MS06工作树。
- R57在相同产品基线上记录：既有global-churn子集17/40失败，Cycle 000字节回换对照10/25失败；典型
  终态为smoltcp stale-handle panic、hashbrown断言、SIGSEGV或SIGABRT。单线程focused ×100通过只构成缓解。
- `reclaim_hold_drains_to_real_driver_full_without_observing_again`隔离运行通过，并行diagnostics full suite
  偶发失败。当前测试在取得`SERIAL`前先写进程级`TEST_NOW`，而`diag_hold_tick()`读取该全局clock。

**Current-State Evidence**

- `lib.rs`定义唯一`Lazy<SocketSetWrapper>`与`Lazy<ListenTable>`；产品TCP/UDP构造、I/O、poll、accept和Drop
  直接访问这些全局对象。
- `tcp.rs`/`udp.rs` tests调用产品`new()`，多个Rust test线程因此在同一SocketSet中并发add/remove/iterate；
  listener tests还共享LISTEN_TABLE。R57已确认该共调度是失败前置条件，但未确认最终UB所在符号。
- `SocketSetWrapper::new()`与`ListenTable::new()`可构造独立实例；`Service::new_with_listen_table`、
  `StackAccess::Injected`、`RxRxFuture`的local fault sink已提供per-test注入先例。
- TCP accepted socket由`new_connected()`构造；若只替换初始constructor而未把context传给accept、poll、I/O和
  Drop，仍会把local handle交给global set，必须作为完整读写方矩阵一起迁移。
- qemu-diagnostics的Service、QueueEvent、RxTelemetry和fault sink已有local fixture；已定位的剩余共享边是
  `diag::TEST_NOW`。`service::tests::diag::serialized_service`先锁再设时钟，而目标flake顺序相反。

**Critical Path**

```text
R57 global-churn witness
  -> per-test socket/listener context
  -> TCP/UDP create + I/O + poll + accept + Drop全程使用同一context
  -> independent handle namespaces survive parallel churn

diagnostic two-clock RED
  -> Service/Rx future读取fixture clock
  -> target hold test不再写TEST_NOW
  -> focused交错 + 默认并行full suites稳定
```

**Behavioral Change**

- Host tests从产品全局socket/listener对象迁移到各自fixture；两个fixture可拥有相同数值handle而互不影响。
- qemu-diagnostics host tests从共享fake clock迁移到fixture clock；推进一个fixture不改变另一个fixture的lease。
- 非test产品构造、static singleton、系统时钟、socket ABI、readiness、terminal、lock order和QEMU行为不变。

**Change Surface**

| Task | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|
| 5.1 | `tcp.rs::TcpSocket` constructors/helpers/accept/Drop | hard-coded globals | test-only context route，accepted socket继承context |
| 5.1 | `udp.rs::UdpSocket` constructors/helpers/Drop | hard-coded global | test-only local SocketSet route |
| 5.1 | TCP/UDP/listener tests | production singleton churn | fixture constructor与parallel isolation witnesses |
| 5.1 | `wrapper.rs`, `listen_table.rs` | constructible registries | 复用现有实例，不改变产品所有权 |
| 5.2 | `diag.rs`, `service.rs`, `async_rx.rs` clock access | shared `TEST_NOW` | test-injected clock，production仍读wall clock |
| 5.2 | async_rx/service diagnostic tests | partial `SERIAL` coverage | local clocks与显式interleaving witnesses |

**Task Contracts**

### 5.1: isolate public socket and listener tests

- Requirement/Scenario: Task 5.1；R57；D6/D10的host Gate可信度。
- Targets: TCP/UDP context accessors、constructors、accepted-socket propagation、I/O/poll/Drop call sites及相关tests。
- Current behavior: independent tests allocate and retire handles in the same process-global SocketSet/ListenTable；R57
  在默认并行调度下观察stale handle、allocator/hashbrown failure和signals。
- Required behavior: test fixture拥有独立`SocketSetWrapper`与`ListenTable`；fixture创建的TCP/UDP socket在create、
  bind/connect/listen/accept、send/recv、poll/register和Drop中始终访问同一context。production constructor继续绑定
  全局singleton，非test布局可用`cfg(test)`避免新增运行时状态。
- RED witness: 新增两个fixture使用相同数值handle并并行执行add/remove/iterate/listen/accept/drop的测试；修复前
  缺少local constructor/context propagation而RED。保留R57既有失败子集作为回归，不以概率复现作为唯一RED。
- Preserve: `SocketHandle`语义、first-wins terminal、readiness bridge、512 backlog、`SERVICE -> SOCKET_SET ->
  listener entry`锁序、产品public API和static singleton。
- Forbidden: 修改smoltcp handle算法或产品socket语义；reset全局状态；让一个测试清理另一个测试的handle；
  把整个full suite设为单线程；仅用不同port、skip或retry掩盖共享状态。
- GREEN condition: local-context身份/并发/churn tests在ordinary和diagnostics各×100通过；R57命名子集默认线程
  重复通过且无panic/signal；源码审查确认所有fixture socket读写方与accepted child均保留context。
- Verification: focused tests、R57 subset、两profilefull suites、format/source guards和full diff Review。
- Stop when: 隔离需要改变产品SocketHandle或锁序，local handle仍到达global set，或失败在完整隔离后仍复现；
  返回Plan重新归因，不切换为全局串行Gate。

### 5.2: isolate diagnostic clock and hold state

- Requirement/Scenario: Task 5.2；R57伴随flake；D9 diagnostics仅限QEMU/test。
- Targets: `diag_now` test seam、Service/Rx future的clock access、diagnostic fixtures与目标test。
- Current behavior:目标test先写共享`TEST_NOW`再取`SERIAL`；其他diagnostic tests也读写该clock，并行调度可让
  1500ms hold在首轮前过期或进入错误分支。
- Required behavior: 每个diagnostic fixture持有独立clock；Service的`diag_hold_tick`与Rx future deadline均读取
  该fixture clock。目标test不依赖进程级`TEST_NOW`或套件级串行化，production继续读取wall clock。
- RED witness: 两个barrier-coordinated fixture各持不同时间，推进A不得改变B的hold mode、expiry、round count或
  auto-release counter；修复前没有per-fixture clock且RED。另保留目标test与lease tests并行交错见证。
- Preserve: HOLD_NONE/SUBMIT/RECLAIM值、lease边界、checked overflow、32/64预算、telemetry与fault语义。
- Forbidden: 放宽目标assertion、增加sleep、skip/ignore、失败后重跑、仅扩大SERIAL临界区或把diagnostics full
  suite改为单线程。
- GREEN condition: two-clock/interleave tests在qemu-diagnostics ×100通过；目标test focused ×100通过；默认并行
  diagnostics full suite连续三次通过且无随机hold/round drift。
- Verification: focused clock/hold/target tests、diagnostics suite、ordinary non-diagnostics回归与source review。
- Stop when: flake在clock完全隔离后仍复现，或修复需要改变产品lease/queue ownership；返回Plan单独归因，
  不假定与Task 5.1同根。

**Invariants**

- 产品路径只有一个SOCKET_SET/LISTEN_TABLE；test injection不得成为第二个产品registry。
- 同一socket从handle创建到Drop只访问一个context；accepted child继承listener的context。
- wake发生时不持registry/listener guard，既有锁序不变。
- diagnostic clock决定lease时间，不决定queue ownership；telemetry只观测，不参与同步。

**Non-goals**

- 产品network行为、guest probe、QEMU runtime、automatic qualification、scheduler/reset/SMP、真板和性能。
- 修复外部smoltcp/hashbrown、重新设计handles、清理全部test helpers或建立通用依赖注入框架。

**Acceptance**

1. R57 global-churn prerequisite被per-test context消除；并行fixture可复用数值handle而无交叉访问。
2. TCP/UDP全部fixture路径含accept child和Drop均使用创建时context；产品static singleton和锁序不变。
3. diagnostic fixture clock隔离；推进一个clock不改变另一个hold/expiry/telemetry。
4. R57 subset、socket churn、clock interleave和目标flake在预定重复次数内通过，无panic、SIGSEGV或SIGABRT。
5. ordinary与qemu-diagnostics默认并行full suites各连续三次通过；无skip/ignore、无限重跑、串行full suite或
   Critical/Important finding。
6. format、strict OpenSpec、diff check和full diff Review通过；未启动QEMU、未改产品对外语义。

**Verification**

- ordinary/diagnostics local-context tests各×100；R57命名subset默认线程×40。
- qemu-diagnostics two-clock/interleave与目标test各×100。
- `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib`连续三次。
- 同命令增加`--features qemu-diagnostics`连续三次；不得增加`--test-threads=1`。
- axnet范围format、相关source guards、strict OpenSpec、`git diff --check`和本Cycle full diff Review。
- host tests使用既有non-PIE linker wrapper；wrapper缺失是环境准备，不是产品失败。
- SKIPPED: QEMU、MS01/MS04/MS05/MS06 runtime与root全量产品资格；分别属于Iterations 008和007。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | R57对照、global调用面、既有injection seam、target clock-before-lock已定位 |
| Design | PASS | test-only socket/listener context与fixture clock移除两类共享边；产品global路径不变 |
| Iteration Plan | PASS | Tasks 5.1/5.2共同形成可信host Gate，仍保留独立Acceptance |
| Cycle Scope | PASS | 只处理test isolation；automatic qualification与QEMU保持后续Iteration |
| Task Contracts | PASS | 两项均有RED、GREEN、保持/禁止、验证、停止条件和完整调用面 |
| Traceability | PASS | R57与Tasks 5.1/5.2映射到context、clock、target和默认并行Gate |
| Verification | PASS | focused ×100、subset ×40、两profile连续三次，禁止串行/skip/retry masking |

Gate 2技术检查项PASS；用户批准仍为BLOCKED，Plan Context保持`draft`。

**Persisted Evidence**

- Mode: none

结果可低成本重跑，由Act Response记录命令、决定性输出、exit、文件和符号；不创建Evidence占位目录。

**Risks and Notes**

- R57确认共享global churn是失败前置条件，但最终UB符号仍未确认；本设计消除该前置条件，不声称修复smoltcp。
- 若完整context隔离后R57 subset仍失败，必须停止并返回Plan，不能继续扩大串行边界。
- `ListenTable::new()`分配port-indexed状态；fixture数量只由并行tests持有，Act需检查清理和内存峰值但不得因此
  退回global reset。

## Act Response

- Status: pending

**Implemented**

Pending.

**Changed Files and Symbols**

Pending.

**Deviations from Plan**

None yet.

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

Pending.

**Verification Evidence**

Pending.

**Persisted Evidence**

None required.

**Experience Candidates**

None yet.

**Remaining Issues**

Pending.

**Commit or Diff Reference**

未提交；等待Act。

## Plan Review

- Review Result: pending

**Findings**

None yet.

**Deviation Classification**

None yet.

**Acceptance Gaps**

Tasks 5.1-5.2尚未实施或Review。

**Convergence**

N/A.

**Evidence**

None yet.

**Follow-up Decision**

等待用户批准Gate 2后显式调用`openspec-act`。

**Iteration Plan Update**

None.

**Next Cycle**

None.

**Next Iteration**

None；Iteration 007保持map-only，直到Iteration 006 accepted。
