# Iteration 001 / Cycle 005: UDP Drain Ownership and Deterministic Backlog Recovery

## Plan Context

- Status: ready
- Approval: approved by user on 2026-08-24（原话：“批准”）；批准范围为本地smoltcp只读
  `has_pending_tx()`接口及MS01 overflow/recovery分层取证；ready for an explicit
  `openspec-act` invocation
- Iteration: 001-socket-and-listener-readiness-bridge
- Cycle: 005-replan
- Cycle Type: replan
- Parent cycle: `004-replan.md`

**Iteration Scope**

- Change tasks: 2.1–2.7
- Revised tasks: 2.6、2.7
- Depends on: Iteration 000 accepted；Tasks 2.1–2.5与Cycle 004已验证的ownership/scale基线
- Stable baseline: 产品socket不主动推进协议栈；listener和deferred retirement每轮有界；UDP
  send后立即drop不丢已提交datagram；512 backlog释放后recovery与先前overflow事件分开判定
- Verification boundary: listener 31/32/33/512 budget与RST恢复、UDP pending-TX drop/drain/reap、
  exact-512 headroom、host两profile、fresh single-hart QEMU diagnostic single/fork和MS01 14/14
- Diagnostic boundary: 失败限制在ListenTable cursor/state转换、UDP TX buffer生命周期、deferred
  sweep触发、runner调度或MS01 guest路径
- Deferred tasks: 3.1–3.4

**Cycle Scope**

- Trigger: Cycle 004 Plan Review `replan-required`
- Acceptance gaps: Cycle 004 Acceptance 2–6
- Revised tasks: 2.6、2.7
- Inherited scope: R1–R6、D1–D10、Tasks 2.1–2.5、single timestamp、TCP deferred budget、
  raw-handle独占、atomic accept/refill、exact-512与cleanup→UDP host witnesses
- Excluded scope: Task 3 terminal fault广播、SO_LINGER、unresponsive-peer abort、socket reset/
  cancellation、scheduler修改、SMP、真板、性能、全局文档、归档和commit

**Objective**

让listener reconciliation与deferred socket retirement在512规模下仍保持每轮有界；让UDP public
handle在send后立即drop时保留raw socket直到真实TX buffer排空；将满backlog overflow终态和
headroom recovery拆成可判定的两个场景。完成后，host/model和fresh single-hart QEMU都能证明
UDP echo不丢失、listener slot不泄漏、MS01 14/14结束。

**Scenario Sketch**

| Scenario | 前置状态 | 动作 | 可观察结果 | 失败边界 |
|---|---|---|---|---|
| S1 UDP queued drop | TX buffer含一个datagram，runner可用 | public UDP handle drop | peer收到完整datagram；raw handle随后回收一次 | drop清buffer、entry不重检或永久泄漏 |
| S2 UDP empty drop | TX buffer为空 | public UDP handle drop | 立即retire/remove，不创建deferred entry | 空socket被无条件延迟 |
| S3 UDP stale/retyped | `UdpQueued` entry的handle缺失或指向非UDP | runner reap | 只删除entry，不触碰新socket | 通用TCP分支先匹配或误删slot |
| S4 listener scale | 31/32/33/512个pending slots | 任意位置Ready/Reset/回到Listen | 每round检查≤32，cursor最终到达；其他stage运行 | 每ingress step全表扫描或quiet busy wake |
| S5 passive RST | pending hidden socket从SynReceived回到Listen | reconcile | 无idle时复用为idle；已有idle时安全移除冗余slot | 永久Pending并占backlog |
| S6 backlog recovery | overflow尝试已到终态，queue为512 | accept一个后立即新connect | 新connect成功，Ready只交付一次，queue≤512 | 依赖caller poll、提高backlog或ConnectionRefused |
| S7 QEMU failure | 任一marker缺失、FAIL、timeout或中断 | 运行single/fork/MS01 | Cycle保持blocked/reported前失败 | host GREEN替代runtime证据 |

**Current Baseline**

- Branch `net-k3`；HEAD `fb87c8d36b7c62e8d7156598defa08bce0db32d4`；MS06实现与文档位于
  staged worktree，Cycle 005计划变更暂未stage。
- Cycle 004 Act Response为blocked，Plan Review为`replan-required`；不得恢复旧Cycle。
- `task_27_repro_guest_512_recovery_sequence`和
  `task_27_cleanup_storm_keeps_unrelated_udp_forward_progress` fresh各1/1 PASS。
- 三个UDP WIP tests fresh失败：两个`deferred_retirement_udp_*`与
  `task_27_repro_udp_child_close_keeps_queued_echo`，命令exit 101。
- diagnostic single/fork runtime PASS；原MS01在tcp-512-recovery FAIL，随后UDP无marker并中止。
- Persisted Evidence为`none`；没有change Evidence目录。

**Current-State Evidence**

- `smoltcp::socket::udp::Socket::can_send()`返回`!tx_buffer.is_full()`；它不是pending-TX谓词。
  `dispatch()`从`tx_buffer` dequeue一个packet，`poll_at()`已用`tx_buffer.is_empty()`区分Now/Ingress，
  但该状态没有public只读接口。
- `UdpSocket::drop`用`can_send()`决定是否提交`CloseKind::UdpQueued`；常见空buffer也会进入deferred。
- `Service::reap_deferred_removals`同样用`can_send()`决定Keep/Reap，因此drain后entry仍可永久Keep。
- reaper的通用TCP Keep arm位于`UdpQueued + TCP => Drop`之前；编译器报告后者unreachable。
- `Service::stack_round`在每个非idle ingress step后调用`ListenTable::reconcile`，round结束前又调用；
  `reconcile`遍历所有pending slots，512 queue可在一轮内重复扫描。
- pending slot对`State::Listen | State::SynReceived`统一保持Pending；smoltcp passive-open收到RST会从
  SynReceived回到Listen，造成仍可listen的socket脱离idle所有权。
- guest日志顺序为overflow client`:49668`提交、accept创建`#1025`、旧SYN使`#1025`进入
  SynReceived、recovery client`:49669`随后ConnectionRefused。atomic refill已发生，失败证据混入
  in-flight overflow排序。
- guest UDP日志显示responder收到8 bytes后退出；sendto与drop之间没有runner egress保证，现有
  smoltcp `close()`会reset TX buffer。

**Relevant Code**

| File / Symbol | Current Responsibility | Cycle Use |
|---|---|---|
| `crates/smoltcp/src/socket/udp.rs::Socket::{can_send,dispatch,poll_at}` | UDP TX容量、dequeue与deadline | 增加只读pending-TX查询及unit tests |
| `crates/axnet/src/udp.rs::UdpSocket::drop` | public retire、close与raw remove | 仅真实pending TX进入deferred |
| `crates/axnet/src/service.rs::reap_deferred_removals` | 有界TCP/UDP raw handle回收 | 修verdict顺序、完成谓词与重检触发 |
| `crates/axnet/src/listen_table.rs::reconcile` | active ports、hidden slot状态与accept bridge | 全局32-entry port/slot cursor、RST-to-Listen恢复、outcome |
| `crates/axnet/src/service.rs::stack_round` | 固定stage顺序 | 每round一次有界listener stage |
| `tests/ms01_socket_baseline.c::test_tcp_512_capacity` | guest backlog兼容 | 分开overflow终态与headroom recovery |
| `stack_runner.rs` / `service.rs` tests | injected full-chain与规模witness | RED/GREEN、100×与source guards |

**Critical Path**

```text
UDP sendto
  -> smoltcp TX buffer non-empty
  -> public drop reads has_pending_tx under SocketSet
  -> retire public metadata
  -> enqueue UdpQueued under Service
  -> publish software work
  -> runner egress dequeues datagram
  -> egress progress restarts bounded deferred sweep
  -> has_pending_tx == false
  -> remove raw handle + entry in one guarded commit

runner round
  -> bounded ingress/egress
  -> one bounded listener reconciliation batch (<=32)
       -> Ready/Reset commit
       -> SynReceived->Listen: restore idle or remove redundant slot
  -> one bounded deferred batch (<=32)
  -> unlock -> staged wakes/self-yield/timer

MS01 backlog
  -> fill 512
  -> overflow attempt reaches explicit terminal result
  -> accept one + atomic refill
  -> immediate recovery connect
  -> accept remaining 512 + recovery
```

**Implementation Guidance**

先在本地smoltcp加入只读`has_pending_tx()`，直接返回`!tx_buffer.is_empty()`；API不得dequeue、wake或
改变poll_at。用空buffer、enqueue一个packet和dispatch成功后三态test固定语义。随后修axnet UDP
drop与reaper：类型/CloseKind专用arms必须位于通用TCP arms之前，成功egress或fresh enqueue启动
有界sweep，完整quiet sweep后park。

listener部分把跨active ports与entry slots的全局cursor/outcome放在ListenTable责任内，所有listener
共享每round 32次slot检查预算；不得通过clone完整active-port列表把无界工作移出计数。Service每round
只调用一次有界reconciliation，不能在ingress closure里重复扫描。状态回到Listen时从pending queue
移出；没有idle则直接转移handle ownership为idle，已有idle则移除冗余raw hidden socket。wake仍在
所有guard释放后发生。

MS01保留14个PASS marker与fixed ordering，只调整`test_tcp_512_capacity`的证据顺序：不得忽略一个
仍处于EINPROGRESS的overflow尝试后立即把它与recovery竞争。overflow应在独立host/model场景验证，
guest workload必须在其终态可判定后再释放headroom；若现有socket API无法可靠取得overflow终态，
本Cycle删除guest中的额外overflow刺激，但保留exact-512 recovery，并由host/model覆盖overflow RST。
该fallback属于本Plan已定义的验证分层，不得留给Act临时决定。

**Behavioral Change**

- 本地smoltcp新增只读UDP pending-TX观察，不改变wire、容量、dispatch或错误语义。
- UDP public drop不再清除已提交但未派发的datagram；空TX socket仍立即回收。
- listener reconciliation单轮检查量从“每个ingress step最多全扫512”改为固定最多32，并保持跨轮
  cursor；passive RST后的Listen socket恢复为idle或被安全移除。
- MS01把overflow与recovery分层取证；不承诺新headroom优先于已经排队的旧SYN。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| 2.6 | R3/S4/S5 | `listen_table.rs::reconcile`、`service.rs::stack_round` | 重复全表scan；Listen滞留Pending | 32-entry cursor/outcome；每round一次；RST恢复 |
| 2.7 | R6/S1–S3 | smoltcp UDP、`udp.rs::drop`、`service.rs::reap_*` | 无pending谓词；错误verdict顺序 | 只读谓词；真实drain ownership；bounded reap |
| 2.7 | R6/R7/S6–S7 | stack runner tests、MS01 payload、QEMU | overflow/recovery竞态混证据 | 分层场景、14/14 runtime Gate |

**Task Contracts**

### 2.6: 有界listener reconciliation与passive RST恢复

- Requirement/Scenario: R3、R4、R6；D4、D5、D7；S4、S5。
- Depends on: Cycle 004 atomic accept/refill与single timestamp GREEN。
- Targets: `listen_table.rs::ListenTableEntryInner/ListenTable::reconcile`、
  `service.rs::Service::stack_round`、outcome/telemetry与stack-runner tests。
- Current behavior: 每个非idle ingress step可触发一次完整pending queue扫描；State::Listen保持Pending。
- Required behavior: 所有active listeners合计每round最多检查32个pending slots并持久公平；不得完整
  clone/扫描active-port列表；Ready/Reset最终提交；回到Listen的slot恢复idle或安全删除；其他stage
  同round运行，quiet queue不持续self-wake。
- Required changes: 先加入单/多listener 31/32/33/512、changed-tail、port/slot cursor swap/remove、
  Listen恢复、stage count和guard外wake RED tests；再实现有界outcome并删除临时info flood。
- Preserve: backlog=512、Ready唯一accept、Reset错误、accept atomic refill、桥接waker重臂、
  `SERVICE → SOCKET_SET → entry`、caller零progress。
- Forbidden: 周期全表poll、提高backlog、hidden waker内取Service/SocketSet、guard内wake/yield。
- Test witness: 当前source中ingress closure含`reconcile(sockets)`；512 guest日志重复
  `refill blocked`；新增source与behavior tests必须先RED。
- GREEN condition: 31/32/33/512每round checked≤32且最终收敛；RST-to-Listen不泄漏slot；两profile
  targeted各100×，其他stage/quiet assertions通过。
- Verification: targeted两profile→full suites→fmt/source/diff checks。
- Stop when: 精确事件识别需要per-hidden callback持锁、改变backlog/accept语义或scheduler。

### 2.7: UDP queued-TX drain ownership与确定性guest兼容

- Requirement/Scenario: R3、R6、R7；D7、D8、D9、D10、D11；S1–S3、S6、S7。
- Depends on: Task 2.6 GREEN。
- Targets: `crates/smoltcp/src/socket/udp.rs`；axnet `udp.rs::drop`、
  `service.rs::CloseKind/reap_deferred_removals`；stack-runner full-chain tests；
  `tests/ms01_socket_baseline.c::test_tcp_512_capacity`。
- Current behavior: `can_send()`误作pending谓词；3个fresh tests RED；guest UDP echo丢失；MS01
  overflow旧SYN与recovery争用新slot。
- Required behavior: pending datagram在drop后由唯一runner派发，peer收到后raw handle回收一次；空TX
  drop立即回收；stale/retyped不误删；guest overflow与recovery分别判定，MS01 14/14+END。
- Required changes: smoltcp accessor unit RED/GREEN；修drop、verdict顺序、reap重启；保留/扩展3个
  现有RED tests；增加overflow RST/listener recovery witness；按Plan分层调整MS01。
- Preserve: UDP datagram原子性、buffer容量、socket readiness、唯一runner、32 budget、raw handle
  ownership、MS05 slots/tickets/flush、single-hart结论边界。
- Forbidden: drop内同步dispatch、axnet影子TX ledger、smoltcp协议行为变化、caller-driven poll、
  sleep掩盖race、reset/cancellation/SO_LINGER、把diagnostic替代MS01。
- Test witness: accessor不存在；两个`deferred_retirement_udp_*`与
  `task_27_repro_udp_child_close_keeps_queued_echo` fresh exit 101；runtime UDP无marker。
- GREEN condition: accessor三态、3个现有RED、empty-drop、stale/retyped、send→drop→peer receive→reap
  全GREEN；ordinary/qemu-diagnostics full suites通过；fresh QEMU single/fork与MS01 14/14+END。
- Verification: smoltcp UDP unit→axnet targeted 100×两profile→full automatic Gates→用户手工QEMU。
- Stop when: 只读accessor不足以判断dispatch completion；需要修改smoltcp dequeue/wire语义、
  scheduler、backlog上限或新增取消/reset契约。

**Invariants**

- resident runner仍是唯一smoltcp推进者；queue task仍独占descriptor/completion。
- UDP raw handle只在public metadata退役后由reaper移除；empty immediate remove不创建entry。
- listener、deferred和Router stages各自有界；任何guard不跨wake、await、Pending或yield。
- 512 backlog、TCP short write、UDP datagram原子性、PollSet 64/65和single-hart范围不变。

**Non-goals**

- SO_LINGER、unresponsive peer abort、完整SO_ERROR、Task 3 terminal fault广播。
- reset、SMP、多接口、PCI/DWMAC、真板、DMA/cache、性能、全局状态维护和归档。

**Replan Traceability Matrix**

| Requirement | Scenario | Design | Task | Code Surface | Test Witness | Simplification | Status |
|---|---|---|---|---|---|---|---|
| R3 bounded/fair | S4 | D4 | 2.6 | listener cursor、Service round | 31/32/33/512、changed-tail、quiet | None | Covered |
| R6 listener lifecycle | S5 | D7 | 2.6 | pending state conversion | SynReceived→Listen idle/redundant paths | None | Covered |
| R6 UDP close | S1–S3 | D8,D9,D11 | 2.7 | smoltcp UDP、drop、reaper | accessor三态、send→drop→receive→reap | None | Covered |
| R6 backlog recovery | S6 | D7 | 2.7 | listener、MS01 payload | overflow终态分层、exact-512 recovery | None | Covered |
| R7 runtime | S7 | D10 | 2.7 | QEMU artifact/payload | single、fork、MS01 14/14+END | None | Covered |

没有Missing requirement。验证契约不删除512 recovery或overflow safety；只把两个异步事件分开判定。

**Acceptance**

1. Listener reconciliation每runner poll检查≤32个pending slots；31/32/33/512、cursor公平、其他stage
   和quiet path通过；State::Listen不永久滞留Pending。
2. smoltcp只读pending-TX查询在空、enqueue、dispatch后三态准确，不改变既有can_send与dispatch语义。
3. UDP send后立即drop仍使peer收到完整datagram；raw handle在drain后恰好回收一次，empty drop立即
   回收，stale/retyped不误删。
4. exact-512 accept/refill与cleanup→UDP既有witness保持GREEN；overflow终态与recovery分证据，
   recovery不依赖caller-driven progress。
5. ordinary/qemu-diagnostics、targeted 100×、MS04 harness、QEMU/D1 checks、payload、fmt/source、
   strict OpenSpec与full diff review全部PASS，无Critical/Important finding。
6. fresh single-hart QEMU diagnostic single/fork与MS01完整结束；MS01 14/14+START/END，无FAIL、
   timeout、missing marker、panic或用户中断。
7. 结论只覆盖single-hart QEMU VirtIO-MMIO，不扩大到Task 3、SMP、真板或性能。

**Verification**

- `cargo test --manifest-path crates/smoltcp/Cargo.toml --locked --offline --lib udp`
- 两profile分别运行listener budget/RST、`deferred_retirement_udp_*`、
  `task_27_repro_udp_child_close_keeps_queued_echo`及scale/full-chain tests各100×。
- `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib`
- 同命令增加`--features qemu-diagnostics -- --test-threads=1`。
- MS04 host harness、kernel QEMU check、root lichee-d1 check、两payload交叉编译、`make LOG=error build`。
- source assertions：每round只一个listener reconciliation stage；pending-TX不用`can_send()`；
  UdpQueued arm先于通用TCP arm；产品socket无`poll_interfaces()`。
- `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check`
- `openspec validate ms06-application-visible-async-network-stack --strict`
- `git diff --check HEAD`与完整diff review。
- automatic Gates通过后，用户按Runbook手工运行diagnostic single/fork与MS01并回传完整markers、
  warn/panic和退出/中断结论。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | actual code、3个fresh RED、2个fresh GREEN与guest日志已核对 |
| Design | PASS | local read-only accessor、bounded listener stage、RST恢复和验证分层均无TBD |
| Iteration Plan | PASS | 仍为Tasks 2.1–2.7；两项修订共同形成Iteration 001 socket/listener稳定基线 |
| Cycle Scope | PASS | 只关闭既有R3/R6/R7；不进入Task 3、reset、SMP或硬件 |
| Task Contracts | PASS | 2.6/2.7含入口、RED、目标、保持/禁止、GREEN、验证与停止条件 |
| Traceability | PASS | R3/R6/R7→S1–S7→D4/D7/D11→Tasks→代码→tests无Missing |
| Verification | PASS | host/model先于single-hart QEMU；overflow/recovery与UDP lifecycle分项判定 |
| User Approval | PASS | 用户于2026-08-24回复“批准”，接受本地smoltcp只读accessor与MS01验证契约变化 |

Gate 2全部PASS。Cycle已从`draft`更新为`ready`，等待显式`openspec-act`调用；本次批准不构成
自动实施、Plan Review、全局状态同步或收尾授权。

**Persisted Evidence**

- Mode: none

自动结果可低成本重跑；QEMU markers可在Act Response内摘要。`/tmp/ms06-info-serial.log`只作为本次
Plan调查输入，不提升为required Evidence。

**Risks and Notes**

- smoltcp accessor是本地依赖API变化，但只读且已有`poll_at()`内部同一判定；若实现需要暴露buffer
  长度或dispatch回调，立即停止回Plan。
- 512 SocketSet使smoltcp自身egress/poll_at遍历成本仍随socket数增长；本Cycle只消除axnet重复
  listener全表scan，不声明性能资格。
- MS01 overflow原刺激存在合法网络排序竞态。计划保留overflow safety host witness和14-marker guest
  compatibility，但不把旧SYN优先占slot误判为atomic refill失败。
- 临时info诊断日志会改变guest调度并产生串口洪泛；Task 2.6 GREEN前必须删除或降回非产品诊断级。

## Act Response

- Status: pending

**Implemented**

None

**Changed Files and Symbols**

None

**Deviations from Plan**

None

**Blocker Handoff**

None

**Blocker Resolution**

None

**Self-Review**

- Plan compliance: pending
- Full diff reviewed: pending
- Critical findings unresolved: pending
- Important findings unresolved: pending
- Minor findings unresolved: pending

**Verification Evidence**

None

**Persisted Evidence**

None required

**Experience Candidates**

None

**Remaining Issues**

None

**Commit or Diff Reference**

None

## Plan Review

- Status: completed

**Review Result**

replan-required

**Findings**

1. **Blocking — Cycle 005聚合了三个可独立验收的故障域。** Task 2.6修改ListenTable cursor和
   Service固定stage；Task 2.7同时要求本地smoltcp API、UDP public/raw handle lifecycle、guest workload
   事件排序和manual QEMU。任一部分失败都不能由同一诊断边界定位，且host机制与外部runtime Gate没有
   共同的最小稳定基线。
2. **Blocking — 原Iteration平衡审计不成立。** listener budget可以由host/model独立验收；UDP drain
   ownership依赖listener稳定基线但不依赖guest workload；MS01 compatibility又依赖前两项GREEN。把三者
   保留在一个Cycle会让Act跨机制、依赖API和运行环境连续实施，违反“一个Iteration形成内聚、可独立验证
   与排障的阶段结果”。
3. **Non-blocking — 没有Cycle 005实施需要保留或回滚。** Act Response仍为`pending`，Implemented、
   Changed Files and Symbols、Verification Evidence均为None；本次Review没有按Cycle 005修改产品代码。

**Deviation Classification**

- PLAN-INVALID：Cycle 005的Iteration Plan和Cycle Scope未通过重新平衡审计。
- NEW-EVIDENCE：用户于2026-08-24明确指出当前Cycle任务过重并要求重新审计、拆分工作。

**Acceptance Gaps**

- Acceptance 1–6均未按Cycle 005执行；既有3个UDP RED、listener重复scan与QEMU/MS01失败基线不变。
- Acceptance 7是范围限定，不构成已完成产品验收。

**Convergence**

`unchanged`。Cycle 005未进入Act，没有新的实现或测试结果；本次只把既有gap按依赖和验证边界拆分。

**Evidence**

- `Act Response: pending`，且Cycle 005的Implemented、Changed Files、Verification Evidence均为None。
- 实际计划面：Task 2.6涉及`listen_table.rs`与`service.rs::stack_round`；Task 2.7原范围同时涉及本地
  smoltcp UDP、axnet drop/reaper、MS01 payload和manual QEMU。
- `openspec validate ms06-application-visible-async-network-stack --strict`：valid，exit 0。
- `git diff --check HEAD`：无输出，exit 0。
- 产品验证SKIPPED：Cycle 005未执行且本次只改计划；不以重跑UDP RED或QEMU证明Iteration平衡。

**Follow-up Decision**

停止Cycle 005并创建同一Iteration的`006-replan.md`。Cycle 006只执行Task 2.6，先形成有界listener
reconciliation稳定基线；Task 2.7的UDP drain ownership和Task 2.8的backlog/MS01 runtime compatibility
分别进入后续Iteration。新Cycle须由用户批准后才能交给Act。

**Iteration Plan Update**

- Iteration 001改为Tasks 2.1–2.6，只收口listener reconciliation与RST-to-Listen恢复。
- 新Iteration 002包含Task 2.7，只处理smoltcp pending-TX观察和UDP raw-handle drain ownership。
- 新Iteration 003包含Task 2.8，只处理overflow/recovery分层证据与MS01 single-hart QEMU兼容。
- 原terminal readiness和最终QEMU acceptance保持Tasks 3.1–3.4，顺延为Iteration 004。
- Requirement和设计行为不变；没有新增或裁剪用户可见Acceptance。

**Next Cycle**

`006-replan.md`（draft，等待用户批准）。

**Next Iteration**

None; expand Iteration 002 only after `006-replan.md` is accepted.
