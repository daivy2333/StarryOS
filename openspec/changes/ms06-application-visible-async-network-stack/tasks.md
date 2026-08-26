## 1. 常驻 stack runner（T09）

- [x] 1.1 在 `crates/axnet/src/stack_runner.rs`（新文件）、`async_rx.rs` 和 `lib.rs` 建立独立 `StackEvent`、runner lifecycle、固定 task name 与可注入 spawn seam。WHY 是普通 socket software work 不能继续污染 MS05 queue generation，且 runner 必须至多一个；HOW 是先以 counting waker/CAS RED tests覆盖 event-before-register、register-during-event、wrapping generation、并发/重复 start和production-global隔离，再实现 Release publish、Acquire recheck和唯一spawn；EXPECTED 是device/software publish只唤醒runner role，重复start返回稳定错误且不生成第二task。运行ordinary与qemu-diagnostics axnet lib tests；若需要改变queue owner lifecycle、让test推进production global或引入多runner，停止返回Plan。
- [x] 1.2 在 `crates/axnet/src/router.rs` 将无界RX poll和dispatch拆成保持MS05 typed outcome/packet ownership的单步或有界API，并加入持久round-robin device cursor。WHY 是当前loop可被持续流量无限占用且loopback可能挡住target NIC；HOW 是先用fake devices/packet buffers写31/32/33、loopback+Ethernet公平、Full保留队首、fault和ticket守恒RED tests，再返回显式work/backlog/fault outcome；EXPECTED 是每次调用工作量有上界、每个device最终获服务，MS05 Full/drop/fault语义不变。运行router/device/flush tests；若有界化要求改变64-slot、ticket或descriptor owner，停止返回Plan。
- [x] 1.3 在 `crates/axnet/src/service.rs` 建立每stage `STACK_STAGE_BUDGET=32` 的stack round，固定执行Router RX、maintenance、listener reconcile、smoltcp ingress、egress和dispatch，并返回结构化round outcome。WHY 是runner必须判断self-yield、timer、space release、socket change和stable fault而不能复用含混bool；HOW 是先写每stage 31/32/33、前stage耗尽不跳过后stage、RX space wake、TX enqueue和fault propagation RED tests，再组合D4固定顺序；EXPECTED 是单round有界、各stage都有机会、剩余backlog显式可见且现有queue wake/flush行为不变。运行service/router/listener host tests；若任何stage只能通过drain-to-empty保持正确性，停止返回Plan。
- [x] 1.4 在 `stack_runner.rs` 实现generation register-recheck、runner-owned `poll_at` timer、10ms Polling/Spawned/Unavailable或`requires_polling` fallback、Active quiet和Faulted no-fallback。WHY 是device/software/timer三源必须由唯一推进者无丢失合流；HOW 是用fake ServiceAccess、fake clock/timer和counting waker先覆盖deadline replacement、迟到timer、event交错、budget self-yield、Active idle与lifecycle矩阵RED tests，再确保所有guard释放后arm/wake/Pending；EXPECTED 是可推进工作不永久Pending、Active IRQ quiet不增长poll、Unavailable仍有有界progress、Faulted不创建第二owner。运行100×确定性交错和普通/qemu-diagnostics tests；若timer必须持Service guard或Active仍依赖固定tick，停止返回Plan。
- [x] 1.5 在 `crates/axnet/src/lib.rs` 的Service安装后启动runner，增加T09 telemetry/snapshot test seam并保持本Iteration内既有socket inline poll兼容；同步核对root QEMU与受支持D1 feature组合。WHY 是Iteration 000必须形成可运行的resident-progress基线，又不能在readiness bridge完成前制造无waker的中间状态；HOW 是先写init顺序、pre-Service access、single spawn和当前socket回归RED/source tests，再启动runner并保留旧socket register/timeout作为明确的临时兼容层；EXPECTED 是runner在scheduler-ready/Service-ready边界启动，QEMU IRQ激活前可fallback，旧TCP/UDP/listener仍通过。运行axnet两组lib tests、kernel QEMU check和受支持root D1 check；若axruntime实际顺序不能保证spawn安全或需要kernel专用第二启动点，停止返回Plan。

## 2. Socket 与 listener readiness bridge（T10）

- [x] 2.1 在 `crates/axnet/src/wrapper.rs` 和新 `readiness.rs` 建立per-public-handle `Arc<ReadinessBridge>`（read/write/terminal `PollSet`）与handle registry，定义install/remove/snapshot/wake生命周期。WHY 是smoltcp单槽waker必须扇出且stable fault需要逐socket广播；HOW 是先写create/remove、drop wake、handle复用、registry快照解锁后wake、1/2/64/65 waiter RED tests，再实现不持registry lock调用waker的Arc生命周期；EXPECTED 是不同socket不共享64容量，remove不遗留永久waiter，overflow沿用wake-on-replacement。运行wrapper/readiness tests和Miri可用范围内的host tests；若需要修改axpoll容量或全局共享PollSet，停止返回Plan。
- [x] 2.2 在 `crates/axnet/src/tcp.rs`、`udp.rs` 把smoltcp recv/send单槽waker注册到对应bridge，并让blocking I/O、poll/select/epoll遵循application register→bridge register→recheck。WHY 是后注册socket waiter当前会覆盖前者；HOW 是先用fake smoltcp state/counting waker覆盖read/write不同interest、多waiter、spurious wake、ready-before-register和ready-during-register RED tests，再统一普通TCP/UDP register helper；EXPECTED 是一次smoltcp transition唤醒全部已登记waiter，wake后可重臂且实际I/O仍决定成功。运行TCP/UDP host tests；若需修改smoltcp存储多个waker，停止返回Plan。
- [x] 2.3 在 `crates/axnet/src/listen_table.rs` 和 `tcp.rs` 建立public listener accept bridge，对idle/pending隐藏socket在refill、reconcile和register时重臂，并保持512 backlog和唯一accept。WHY 是public TcpSocket handle不是真正SYN接收者；HOW 是先写hidden socket ready/reset、多个accept waiter、full→accept→refill、unlisten/drop和register race RED tests，再按SocketSet→entry顺序注册同一accept Arc；EXPECTED 是Ready唤醒并只交付一次，Reset报告error，cleanup唤醒遗留waiter且不泄漏handle。运行listener兼容和multiwaiter tests；若需要周期扫描listener或改变backlog语义，停止返回Plan。
- [x] 2.4 在 `service.rs`、`general.rs`、`wrapper.rs`、`tcp.rs`、`udp.rs` 完成 `SERVICE → SOCKET_SET → ListenTable entry` helper、post-commit software wake和产品cutover：移除仓库TCP/UDP/listener对同步 `poll_interfaces()` 与Service-owned socket timeout/global stack waker的依赖。WHY 是resident runner与现有TCP connect反向锁序会死锁，caller-driven path也违反MS06目标；HOW 是先加source assertions与并发RED tests定位全部mutation和`with_smol_socket`内`get_service`，再把connect放入有序临界区、状态提交后解锁publish、timer归runner；EXPECTED 是产品调用点为零、任何guard不跨wake/await/Pending，single-waiter与nonblocking行为不回归。运行100×runner/socket竞争、source guard、axnet两组tests和kernel QEMU check；若必须message passing、SO_ERROR新契约或恢复inline poll才能通过，停止返回Plan。
- [x] 2.5 在 `tcp.rs`、`udp.rs` 和readiness tests统一普通数据/EOF/writable snapshot：TCP buffered data=`IN`、peer EOF=`IN|RDHUP`、可实际send=`OUT`、双向终止=`HUP`；UDP按完整datagram `IN/OUT`、关闭=`HUP`。WHY 是当前`!may_send`伪报OUT且RDHUP只观察本地flag；HOW 是先对每个状态写poll→紧随I/O RED matrix、并发winner例外和poll/select/epoll 1/2/64/65 waiter tests，再让poll与I/O复用同一snapshot判定；EXPECTED 是普通readiness与下一I/O一致，spurious wake只重检，不改变TCP short-write或UDP原子性。运行MS01 socket baseline self-tests和axnet TCP/UDP tests；若正常EOF必须被当成ERR或现有应用依赖closed-as-OUT，停止返回Plan。
- [x] 2.6 在`stack_runner.rs`、`service.rs`和`listen_table.rs`保持单轮timestamp与32-entry deferred retirement，并把listener pending reconciliation改为每round一次、跨active ports共享固定32-entry budget的持久cursor。WHY 是Cycle 004虽关闭deferred全表扫描，但实际`Service::stack_round`仍在每个ingress step后扫描最多512个hidden slots，fresh guest出现重复`refill blocked`和约0.7ms round；`SynReceived → Listen`的RST路径还会永久滞留pending slot。HOW 是先写单/多listener的31/32/33/512 pending、changed-tail、port/slot cursor、RST-to-Listen复用、active sweep期间accept少量删除前缀但queue仍长于cursor、其他stage仍运行和quiet RED tests，再让reconcile返回checked/changed/backlog outcome并只在固定stage位置运行；listener增删和accept queue删除必须通过结构generation使cursor从安全位置继续，不依赖新的protocol progress。EXPECTED 是所有listener合计每轮检查不超过32，active-port列表不被每轮完整clone，queue结构变化后不漏扫live slot，回到Listen的slot恢复idle或安全移除，不因完整queue反复self-wake。运行ordinary/qemu-diagnostics targeted 100×和full suites；若必须周期轮询、改变512上限或让hidden waker直接取得Service/SocketSet，停止返回Plan。
- [x] 2.7 在本地`crates/smoltcp/src/socket/udp.rs`和axnet `udp.rs`/`service.rs`/`stack_runner.rs`闭合UDP queued-TX drain ownership。WHY 是`can_send()`表示TX未满而非pending TX，当前UDP drop/reaper会丢包或泄漏。HOW 是先为smoltcp增加只读`has_pending_tx()`及空/单包/dispatch RED→GREEN tests，再让UDP drop只在真实pending TX时提交`UdpQueued`、修正verdict顺序和egress后有界重检；host/model证明send→drop→peer receive→reap、empty drop立即回收和stale/retyped安全。EXPECTED 是已提交datagram由唯一runner派发，raw handle在drain后恰好回收一次，quiet sweep不busy-wake。若需要同步drop派发、axnet影子TX ledger、修改smoltcp dequeue/wire语义、scheduler、SO_LINGER或reset/cancellation，停止返回Plan。
- [x] 2.8 在listener head-signal、stack-runner host/model tests与`tests/ms01_socket_baseline.c`建立确定性的concurrent-SYN、backlog overflow/recovery兼容证据。WHY 是单个idle hidden socket在同一32-packet ingress batch的首个SYN后不会及时refill，后续SYN即使backlog有空间也被smoltcp RST；尚未判定的overflow SYN还会与recovery SYN竞争accept释放的新headroom。HOW 是先以同batch双SYN复现第二连接被拒，改用hidden waker登记精确、去重且预留容量的listener-head signal，并在每个ingress packet后最多执行一个O(1) head transition/refill/rearm；主listener sweep仍每round一次、共享32-token cursor。保留Cycle 000的overflow终态、exact-512和guest deadline/14-marker改动，fresh QEMU重新运行diagnostic与完整MS01。EXPECTED 是backlog仍为512，同batch相邻SYN不因瞬时无idle被RST，overflow与recovery分项判定，single-hart QEMU diagnostic single/fork及MS01 14/14+END全部通过。若signal不能在不分配、不丢失、不反向取锁的情况下精确路由，必须预分配idle pool、提高backlog、恢复全表扫描、sleep/caller-driven poll或修改scheduler/reset契约，停止返回Plan。完成于 Cycle `001-replan`（2026-08-26）：ordinary 326/326 + diagnostics 346/346 全绿，用户手动 QEMU diagnostic single/fork PASS 且 MS01 14/14（含 tcp-adjacent）START/END 齐全；Act Response 见 `iterations/003-backlog-and-ms01-runtime-compatibility/001-replan.md`。

## 3. Terminal readiness 与单 hart QEMU 验收

- [ ] 3.1 在 `readiness.rs`、`wrapper.rs`、`general.rs`、`flush.rs`、`service.rs`、`async_rx.rs`、`tcp.rs`、`udp.rs` 和`listen_table.rs` 加入socket-local terminal state、stable DevError→AxError映射和fault-before-wake全registry广播。WHY 是close、connect failure、listener reset和queue fatal必须唤醒所有waiter并让下一I/O返回匹配错误，且fault后新增socket也不能漏失终态；HOW 是先写connect failure `OUT|ERR`、listener reset `IN|ERR`、normal EOF/HUP、UDP close、fault无waiter/多waiter/重复publish/late-add、wake观察已提交code RED tests，再复用单一DevError编码并实现first-wins global/socket transition、snapshot解锁后wake和late socket继承；EXPECTED 是正常close不误报设备ERR，fatal不隐藏为WouldBlock/Full且Faulted不回退polling。运行terminal matrix、MS05 fatal/flush和100×publication ordering tests；若需要完整Linux SO_ERROR消费或reset semantics才能稳定错误，停止返回Plan。
- [ ] 3.2 在 `tests/ms06_stack_readiness_probe.c`（新文件）、对应host seam test和纯输出marker validator中建立fixed-deadline application witness，覆盖无需主动poll的TCP/UDP/listener、poll/select/epoll多waiter、64/65 overflow、timer progress、quiet、continuous traffic和close/error。WHY 是host model不能证明VirtIO IRQ→queue task→runner→syscall waiter完整链；HOW 是先以fake platform让缺marker、超时、partial success和主动poll调用RED，再实现每场景唯一PASS/FAIL marker、环境/revision/exit输出并复用MS01/MS04/MS05启动模式；validator只校验用户手工运行所得输出，不得启动QEMU或驱动guest shell。EXPECTED 是所有场景分项判定且probe不调用axnet内部poll。运行C syntax/seam/validator self-tests与kernel QEMU build；若必须扩建I16 benchmark、依赖SMP或无法在fixed deadline区分overflow/quiet，停止返回Plan。
- [ ] 3.3 依次运行ordinary和qemu-diagnostics axnet全量tests、100×lost-wakeup/lock竞争、MS01 socket、MS04 snapshot/idle/nudge/burst、MS05双向/Full/flush回归、probe self-tests、root QEMU与受支持D1 checks、fmt/source assertions、strict OpenSpec和full diff review。WHY 是QEMU runtime前必须关闭功能、ownership和兼容Gate；HOW 是记录每条命令、revision和exit到Act Response，任一产品/compile/assert/review失败立即停止且不把既有无效standalone D1命令当产品failure；EXPECTED 是无Missing、无未批准Simplified、Critical/Important finding为零且生成当前QEMU artifact。不得修改全局文档、归档change或用历史artifact替代当前结果。
- [ ] 3.4 在单hart、单VirtIO-MMIO NIC QEMU中运行MS06 probe与受影响MS01/MS04/MS05 runtime场景，核对runner三源wake、Active quiet、budget self-yield、多waiter/overflow、listener、close/error和网络功能。WHY 是只有真实axtask timer和QEMU device model能证明应用可见链路；HOW 是使用Task 3.2 fixed deadline/marker，保留完整serial/host stimulus/commands/environment/revision/exit于Act Response并执行最终full diff review；EXPECTED 是全部分项PASS且结论只覆盖单hartQEMU。超时、缺marker、partial telemetry、用户中断或环境阻塞必须记为未完成/blocked，不能提升为PASS或扩大到SMP/真板/性能。

## Requirement Traceability Matrix

| Requirement | Design | Tasks | Iteration | Code surface | Test witness | Status |
|---|---|---|---|---|---|---|
| R1 唯一常驻runner | D1,D5,D9 | 1.1,1.5,2.4 | 000,001 | `lib.rs`, `stack_runner.rs`, TCP/UDP mutation paths | CAS/spawn、init顺序、source guard、caller-independent progress | Covered |
| R2 三源wake与register-recheck | D2,D3 | 1.1,1.4,2.4 | `stack_runner.rs`, `async_rx.rs`, `service.rs` | event交错、timer replacement、software-only wake | Covered |
| R3 budget、公平与quiet | D4 | 1.2,1.3,1.4 | `router.rs`, `service.rs`, runner telemetry | 31/32/33、双向backlog、Active idle | Covered |
| R4 锁序与guard生命周期 | D5,D9 | 1.3,1.4,2.4 | Service/SocketSet/connect/listener helpers | 100×竞争、source assertions、no-guard-across-Pending | Covered |
| R5 per-socket multi-waiter bridge | D6 | 2.1,2.2,2.5 | `readiness.rs`, `wrapper.rs`, TCP/UDP register | 1/2/64/65、register races、remove wake | Covered |
| R6 listener/close/error一致 | D7,D8,D9 | 2.3,2.5,3.1 | ListenTable、terminal snapshot、fault registry | accept/reset、EOF/RDHUP/HUP/ERR、fatal ordering | Covered |
| R3/R6 规模化close与listener前进 | D3,D4,D7,D9,D11 | 2.6,2.7,2.8 | `stack_runner.rs`、`service.rs`、`listen_table.rs`、`tcp.rs`、`udp.rs`、smoltcp UDP、MS01 payload | listener/deferred 31/32/33/512、send→drop→peer receive、overflow终态、accept→立即reconnect | Covered |
| R7 MS06验证边界 | D10 | 2.8,3.2,3.3,3.4 | MS01 payload、guest probe、scripts、QEMU product paths | 分层兼容、host seam、automatic gates、single-hart runtime | Covered |
| network-stack-baseline readiness | D3-D9 | 2.1-2.8,3.1 | TCP/UDP/listener/pollable | poll→I/O matrix、多waiter、512 recovery/close storm、stable fault | Covered |
| MS05 slot consumer/owner保持 | D2,D4,D10 | 1.2,1.3,3.3,3.4 | Router、Service、queue event/slots | MS05 Full/flush/ownership regression | Covered |

没有 Missing、未批准 Simplified 或依赖后续MS07/MS08才能满足的当前Requirement。

## Iteration Plan

### Iteration 000: resident-stack-runner

- Tasks: 1.1-1.5
- Depends on: MS05 accepted baseline
- Stable baseline: 唯一runner可由device/software/timer唤醒，stack round有界且Polling fallback/Active quiet可判定；既有socket inline path暂时保留，因此TCP/UDP/listener兼容不退化。
- Verification boundary: lifecycle、三源register-recheck、31/32/33 budgets、timer replacement、fallback矩阵、guard释放和init顺序全部由host/model tests覆盖，ordinary/qemu-diagnostics tests与QEMU/root D1 checks通过。
- Diagnostic boundary: 失败限制在StackEvent、runner lifecycle/timer、Router/Service bounded round、fallback或启动顺序。
- Non-goals: 删除caller-driven socket poll、per-socket bridge、multi-waiter、listener bridge、terminal mapping和QEMU application acceptance。

### Iteration 001: socket-and-listener-readiness-bridge

- Tasks: 2.1-2.6
- Depends on: Iteration 000 accepted
- Stable baseline: 产品TCP/UDP/listener不再主动推进协议栈；普通data/EOF/writable readiness支持多waiter与listener accept；listener reconciliation跨全部active ports共享每round 32-entry budget，passive RST后的hidden socket不滞留pending。
- Verification boundary: Tasks 2.1–2.5既有GREEN保持；单/多listener 31/32/33/512、port/slot cursor、RST-to-Listen恢复、其他stage前进和quiet path由host/model tests独立证明。
- Diagnostic boundary: 失败限制在readiness registry、TCP/UDP bridge、ListenTable bridge/cursor、锁序、software wake、round timestamp或普通readiness mapping。
- Non-goals: UDP queued-TX lifecycle、MS01 backlog兼容、device-wide terminal fault广播、完整close/error matrix和最终QEMU acceptance。

### Iteration 002: udp-queued-tx-drain-ownership

- Tasks: 2.7
- Depends on: Iteration 001 accepted
- Stable baseline: UDP public handle drop不清除已提交datagram；唯一runner派发后，raw handle和deferred entry恰好回收一次，empty/stale/retyped路径不泄漏或误删。
- Verification boundary: smoltcp pending-TX三态、axnet drop/reaper verdict、send→drop→peer receive→reap、empty drop、stale/retyped和quiet sweep均由host/model tests覆盖，两profile full suites通过。
- Diagnostic boundary: 失败限制在本地smoltcp只读观察、UDP public/raw ownership、deferred verdict顺序、egress后重检或runner software work。
- Non-goals: MS01 payload、manual QEMU、backlog overflow/recovery、terminal fault广播和最终application probe。

### Iteration 003: backlog-and-ms01-runtime-compatibility

- Tasks: 2.8
- Depends on: Iteration 002 accepted
- Stable baseline: 精确head signal保证同batch相邻SYN在backlog有空间时不被误RST；exact-512 accept/refill、overflow安全和headroom recovery由互不混淆的场景证明；受影响MS01在single-hart QEMU保持14/14兼容。
- Verification boundary: host/model同batch双SYN、signal去重/锁序/预算、overflow RST与immediate recovery先通过；随后fresh QEMU diagnostic single/fork和MS01 14/14+START/END通过，无FAIL、timeout、missing marker或panic。
- Diagnostic boundary: 失败限制在hidden listener waker、head-signal queue、ingress micro-step、listener backlog状态、guest workload事件排序、QEMU调度链或既有MS01兼容面。
- Non-goals: MS06新guest probe、terminal fault广播、SMP、真板和性能结论。

### Iteration 004: terminal-readiness-and-qemu-acceptance

- Tasks: 3.1-3.4
- Depends on: Iteration 003 accepted
- Stable baseline: close、EOF、half-close、connect/listener错误和stable data-plane fault均唤醒全部相关waiter；单hartQEMU证明应用无需主动poll即可通过TCP/UDP/listener与poll/select/epoll验收。
- Verification boundary: terminal/fault-before-wake host matrix、probe seam、全部自动Gate、MS01/MS04/MS05回归和fixed-deadline QEMU分项marker通过，最终full diff无Critical/Important finding。
- Diagnostic boundary: 失败限制在terminal snapshot/error映射、fault registry广播、syscall waiter链、guest probe或QEMU环境/调度链。
- Non-goals: reset、SMP、multiqueue、多接口、PCI/DWMAC、真板、性能和归档/全局文档维护。

## Current Cycle

- Current Iteration: `004-terminal-readiness-and-qemu-acceptance`
- Cycle: `000-initial.md`
- Persisted Evidence: none
- Gate 2: BLOCKED；Iteration 004 Cycle 000 已展开，等待用户审计与明确批准；不会自动调用 Act。
- Previous Cycle: Iteration 003 Cycle `001-replan.md` Review Result = `accepted`。Task 2.8 已闭合；用户手工
  QEMU diagnostic single/fork及MS01 14/14+END+exit 0通过，Evidence见
  `evidence/003-backlog-and-ms01-runtime-compatibility/001-replan/`。
- Previous Iteration: Iteration 003 accepted。当前 Cycle 只执行Tasks 3.1–3.4，不包含reset、SMP、真板、
  性能、全局文档维护或归档。
