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
- [ ] 2.6 在`stack_runner.rs`、`service.rs`和`listen_table.rs`保持单轮timestamp与32-entry deferred retirement，并把listener pending reconciliation改为每round一次、跨active ports共享固定32-entry budget的持久cursor。WHY 是Cycle 004虽关闭deferred全表扫描，但实际`Service::stack_round`仍在每个ingress step后扫描最多512个hidden slots，fresh guest出现重复`refill blocked`和约0.7ms round；`SynReceived → Listen`的RST路径还会永久滞留pending slot。HOW 是先写单/多listener的31/32/33/512 pending、changed-tail、port/slot cursor、RST-to-Listen复用、其他stage仍运行和quiet RED tests，再让reconcile返回checked/changed/backlog outcome并只在固定stage位置运行；EXPECTED 是所有listener合计每轮检查不超过32，active-port列表不被每轮完整clone，回到Listen的slot恢复idle或安全移除，不因完整queue反复self-wake。运行ordinary/qemu-diagnostics targeted 100×和full suites；若必须周期轮询、改变512上限或让hidden waker直接取得Service/SocketSet，停止返回Plan。
- [ ] 2.7 在本地`crates/smoltcp/src/socket/udp.rs`、axnet `udp.rs`/`service.rs`/`stack_runner.rs`和MS01 payload关闭guest兼容缺口。WHY 是`can_send()`表示TX未满而非pending TX，当前UDP drop/reaper会丢包或泄漏；MS01又让尚未判定的overflow SYN与recovery SYN竞争同一个新headroom，不能单独证明atomic refill。HOW 是先为smoltcp增加只读`has_pending_tx()`及空/单包/dispatch RED→GREEN tests，再让UDP drop只在真实pending TX时提交`UdpQueued`、修正verdict顺序和egress后有界重检；host/model分别证明send→drop→peer receive→reap、stale/retyped安全、overflow RST后listener恢复。MS01保留14 markers，但先闭合overflow终态再执行full→accept→immediate recovery。EXPECTED 是UDP echo不因child exit丢失且raw handle恰好回收，backlog仍为512，recovery不依赖caller-driven progress，fresh QEMU diagnostic single/fork与MS01 14/14全部通过。若需要同步drop派发、axnet影子TX ledger、scheduler修改、SO_LINGER或reset/cancellation语义，停止返回Plan。

## 3. Terminal readiness 与单 hart QEMU 验收

- [ ] 3.1 在 `readiness.rs`、`general.rs`、`service.rs`、`async_rx.rs`、`tcp.rs`、`udp.rs` 和`listen_table.rs` 加入socket-local terminal state、stable DevError→AxError映射和fault-before-wake全registry广播。WHY 是close、connect failure、listener reset和queue fatal必须唤醒所有waiter并让下一I/O返回匹配错误；HOW 是先写connect failure `OUT|ERR`、listener reset `IN|ERR`、normal EOF/HUP、UDP close、fault无waiter/多waiter/重复publish、wake观察已提交code RED tests，再实现单次stable transition；EXPECTED 是正常close不误报设备ERR，fatal不隐藏为WouldBlock/Full且Faulted不回退polling。运行terminal matrix、MS05 fatal/flush和100×publication ordering tests；若需要完整Linux SO_ERROR消费或reset semantics才能稳定错误，停止返回Plan。
- [ ] 3.2 在 `tests/ms06_stack_readiness_probe.c`（新文件）、对应host seam test和最小QEMU运行脚本中建立fixed-deadline application witness，覆盖无需主动poll的TCP/UDP/listener、poll/select/epoll多waiter、64/65 overflow、timer progress、quiet、continuous traffic和close/error。WHY 是host model不能证明VirtIO IRQ→queue task→runner→syscall waiter完整链；HOW 是先以fake platform让缺marker、超时、partial success和主动poll调用RED，再实现每场景唯一PASS/FAIL marker、环境/revision/exit输出并复用MS01/MS04/MS05启动模式；EXPECTED 是所有场景分项判定且probe不调用axnet内部poll。运行C syntax/seam tests与kernel QEMU build；若必须扩建I16 benchmark、依赖SMP或无法在fixed deadline区分overflow/quiet，停止返回Plan。
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
| R3/R6 规模化close与listener前进 | D3,D4,D7,D9,D11 | 2.6,2.7 | `stack_runner.rs`、`service.rs`、`listen_table.rs`、`tcp.rs`、`udp.rs`、smoltcp UDP | 单轮时钟、listener/deferred 31/32/33/512、overflow终态、accept→立即reconnect、send→drop→peer receive | Covered |
| R7 MS06验证边界 | D10 | 3.2,3.3,3.4 | guest probe、scripts、QEMU product paths | host seam、automatic gates、single-hart runtime | Covered |
| network-stack-baseline readiness | D3-D9 | 2.1-2.7,3.1 | TCP/UDP/listener/pollable | poll→I/O matrix、多waiter、512 recovery/close storm、stable fault | Covered |
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

- Tasks: 2.1-2.7
- Depends on: Iteration 000 accepted
- Stable baseline: 产品TCP/UDP/listener不再主动推进协议栈；普通data/EOF/writable readiness经per-socket PollSet支持多waiter、overflow和listener accept；单轮时间戳一致，listener reconciliation与deferred socket retirement有界，UDP send后立即drop不丢已提交datagram，512 backlog释放与close storm下仍保持poll→I/O和无关UDP前进。
- Verification boundary: handle/bridge生命周期、smoltcp重臂、1/2/64/65 waiters、listener hidden sockets、全锁序、post-commit wake、caller-driven调用点为零、listener/deferred 31/32/33/512 budget、overflow终态与recovery分证据、UDP send→drop→peer receive→reap以及MS01 14/14兼容全部通过。
- Diagnostic boundary: 失败限制在readiness registry、TCP/UDP bridge、ListenTable bridge/cursor、锁序、software wake、round timestamp、deferred retirement、UDP TX lifecycle或普通readiness mapping。
- Non-goals: device-wide terminal fault广播、完整close/error matrix、MS06 guest probe和最终QEMU acceptance。

### Iteration 002: terminal-readiness-and-qemu-acceptance

- Tasks: 3.1-3.4
- Depends on: Iteration 001 accepted
- Stable baseline: close、EOF、half-close、connect/listener错误和stable data-plane fault均唤醒全部相关waiter；单hartQEMU证明应用无需主动poll即可通过TCP/UDP/listener与poll/select/epoll验收。
- Verification boundary: terminal/fault-before-wake host matrix、probe seam、全部自动Gate、MS01/MS04/MS05回归和fixed-deadline QEMU分项marker通过，最终full diff无Critical/Important finding。
- Diagnostic boundary: 失败限制在terminal snapshot/error映射、fault registry广播、syscall waiter链、guest probe或QEMU环境/调度链。
- Non-goals: reset、SMP、multiqueue、多接口、PCI/DWMAC、真板、性能和归档/全局文档维护。

## Current Cycle

- Current Iteration: `001-socket-and-listener-readiness-bridge`
- Cycle: `005-replan.md`
- Persisted Evidence: none
- Gate 2: PASS；用户于2026-08-24显式批准（原话：“批准”）本地smoltcp只读pending-TX accessor及
  MS01 overflow/recovery分层取证；Cycle为`ready`，等待显式`openspec-act`调用。
- Parent Cycle: `004-replan.md` Review Result = `replan-required`。Cycle 004关闭了ownership与两个
  512 host组合witness，但fresh QEMU暴露UDP queued-TX drop和overflow SYN/recovery竞态；实际代码
  仍有3个RED tests。Iteration 002继续等待本Iteration accepted。
