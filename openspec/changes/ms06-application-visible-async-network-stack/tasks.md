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

## 3. Terminal readiness 闭合

- [x] 3.1 在 `readiness.rs`、`wrapper.rs`、`flush.rs`、`service.rs`、`async_rx.rs` 和 `stack_runner.rs` 分离socket-local与global terminal所有权。WHY 是Cycle 000把global code写入first-wins local slot，导致已有local error的bridge可能跳过global wake；HOW 是保留wrapper first-wins global code和bridge first-wins local code，global提交后快照registry、解锁并无条件wake全部bridge，poll/I/O用global优先的effective snapshot；EXPECTED 是0/1/2/64/65 waiter、重复publish、add/install并发和late socket都观察同一个global category，wake callback看到已提交global code。以Cycle 000现有实现为变更前见证，新增local-before-global RED test后修正；运行两profile targeted与full suites。若正确性需要覆盖local code、持registry锁wake、增加第二个global registry或引入reset epoch，停止返回Plan。完成于 Iteration 004 Cycle `001-replan`（2026-08-26）：true RED→GREEN，ordinary 357/357，focused publication ×100 全过，Act Response 见 `iterations/004-terminal-readiness-and-qemu-acceptance/001-replan.md`。
- [x] 3.2 在 `general.rs`、`tcp.rs`、`udp.rs`、`listen_table.rs` 和readiness tests闭合terminal-first I/O与connect/listener语义。WHY 是Cycle 000的UDP blocking recv只在进入函数时检查terminal，fault唤醒后的poll_io重试仍可再次Pending；HOW 是让send/recv/connect/accept在API入口和每次blocking重试都读取同一effective terminal，global fatal先于地址/绑定/状态检查和协议提交，connect的smoltcp wake只作recheck hint，recheck先提交local error再暴露`OUT|ERR`，listener Reset仍为一次性`IN|ERR`。EXPECTED 是normal EOF/HUP无设备ERR，global优先于local错误，所有后续I/O返回相同映射类别，不出现WouldBlock/Full/NotConnected遮蔽或永久Pending。把实际poll_io闭包使用的单次I/O attempt提取为可测试路径，以“首次WouldBlock→发布fatal→第二次调用返回稳定错误”的model witness覆盖fault-during-wait；另加connect hint→commit→result和listener consume-once RED tests。运行两profile terminal matrix、MS05 fatal/flush和100×ordering。若必须运行host axtask scheduler、实现完整Linux SO_ERROR消费、引入message passing或修改scheduler/reset/cancellation语义，停止返回Plan。完成于 Iteration 004 Cycle `001-replan`（2026-08-26）：UDP two-attempt model 与入口次序 5 项 true RED→GREEN，attempt 提取为 `try_recv_once`/`try_send_once`，focused terminal ×100 单线程全过，Act Response 见同上。

## 4. Application witness 构建

- [x] 4.1 新建纯输出marker validator及self-tests，固定MS06 START、revision、环境、12个唯一case PASS/FAIL、END和exit协议。validator只读取用户保存的输出；任何PASS/FAIL/MS06 protocol marker在START前、合法phase外、END后或exit后出现，以及缺失、重复、乱序、超时、partial success和exit不一致都必须失败；普通shell/serial noise可保留，不得启动QEMU或驱动guest shell。完成于 Iteration 005 Cycle `002-replan`（2026-08-27）：统一trim分类并按完整物理行识别唯一START/END；pre-START、phase、前导空白、边界子串、metadata、12-case和exit正反矩阵通过独立复审。
- [x] 4.2 新建`tests/ms06_stack_readiness_probe.c`及host seam，覆盖tcp-timer、udp-progress、listener、nonblock-connect-error、quiet、continuous-traffic和close-error。每个mode使用monotonic fixed deadline，不调用axnet内部poll，不以sleep作为正确性条件；host seam、C syntax和RISC-V static build通过后形成可运行artifact。Cycle `001-rework` 修复AF_INET loopback UDP endpoint与quiet read/terminal interest；Plan Review已接受该部分结果，后续replan不得重做。
- [x] 4.3 在同一probe中加入普通poll/select/epoll multiwaiter及exact waiter-64/waiter-65-reregister。普通场景证明不同waiter在event-before-register下最终完成；exact 64/65场景中每个不同进程先用独立epoll instance同步执行`epoll_ctl(ADD)`并报告arm，parent收齐精确arm数后才发布N=waiter数的consumable units。第65次注册的replacement/no-event recheck/re-register由既有PollSet host/model与epoll内核路径证明，guest证明65个distinct waiter最终各完成一次；不得等待用户态empty-event notification、降低并发或用final record count代替trigger-unit witness。完成于 Iteration 005 Cycle `002-replan`（2026-08-27）：exact 64/65 切 MS06_WAIT_EPOLL，worker 私有 epoll 同步 ADD 后写 'A' arm，parent 收齐 N arms 且 units==waiters 才发布，26 项 seam（含 63/64、64/64、64/65、65/65、N-1/N、错误 mode、duplicate/partial/double）×2 全过。

## 5. Host 测试隔离与确定性

- [x] 5.1 依据R57 Incident定位并修复并行axnet测试共享进程级`SOCKET_SET`/`LISTEN_TABLE`产生的陈旧handle、hashbrown断言和SIGSEGV/SIGABRT。WHY 是当前并行full suite会制造假RED且存在host测试进程内存不安全，不能作为自动资格的可信前置；HOW 是先保持现有失败子集和Cycle 000产品对照建立确定性RED/归因，再优先把socket registry、listener table和fault sink改为per-test实例注入，只有能覆盖全部读写方时才允许统一串行边界；EXPECTED 是既有并行失败子集、add/remove/iterate churn和ordinary full suite重复运行均无invalid handle、panic或进程信号，同时产品static singleton、handle生命周期和锁序不变。若根因要求修改产品socket语义、PollSet容量、scheduler或reset/cancellation，停止返回Plan。完成于 Iteration 006 Cycles `000-initial`/`001-rework`（2026-08-27）：直接socket/listener context、accept child继承、fixture-paired deferred Service/SocketSet、TCP/UDP deferred Drop与等值handle local drain均闭合；test-only TCP state seed不进入normal产品图，测试图只新增`test-seeds`。focused/regression两profile各×100、ordinary 371/371 ×3、diagnostics 393/393 ×3通过；Cycle 001 Review `accepted`。
- [x] 5.2 将qemu-diagnostics的`reclaim_hold_drains_to_real_driver_full_without_observing_again`单独归因并消除flake，不预设其与Task 5.1同根。WHY 是该测试隔离运行恒过、并行full suite偶发失败，可能是diagnostic state、fake clock、telemetry或其他全局读写污染；HOW 是先用读写方矩阵和并行最小复现确定共享边，再优先注入每test独立`DiagnosticState`/clock/telemetry依赖，禁止通过skip、ignore、无限重跑或仅把full suite改为串行掩盖；EXPECTED 是ordinary和qemu-diagnostics默认并行full suites及focused交错重复稳定通过，失败时仍保留真实产品回归检测能力。若证据证明它与5.1同根可共享实现，但必须保留两套独立Acceptance见证。完成于 Iteration 006 Cycle `000-initial`（2026-08-27）：每 fixture 独立 `DiagTestClock` 挂到 Service 与 Rx future，目标测试与全部 diag/hold/deadline 测试脱离 `TEST_NOW`/`SERIAL`；two-clock/interleave ×100、目标 focused ×100、deadline 兄弟集 ×60 全过，diagnostics full ×3 384/384，Act Response 见同上。

## 6. 自动集成资格

- [x] 6.1 依次运行ordinary和qemu-diagnostics axnet默认并行全量tests、100×lost-wakeup/lock竞争、MS01 socket、MS04 snapshot/idle/nudge/burst、MS05双向/Full/flush、MS06 seam/validator、root QEMU与受支持D1 checks、fmt/source assertions、strict OpenSpec和full diff review。记录命令、决定性输出和exit；不得再用已知flake豁免、隔离重跑或串行full suite替代默认并行Gate，任一产品、compile、assert、ownership或review failure都阻止进入QEMU runtime。生成与当前working tree匹配的probe和QEMU artifact，不使用历史产物。完成于 Iteration 007 Cycle `000-initial`（2026-08-27）：automatic Gates 与 fresh artifacts PASS；ordinary full suite 曾在20×窗口第16次出现SIGSEGV，随后30/30未复现，用户明确豁免该残余风险；Plan Review `accepted`。

## 7. 单 hart QEMU 验收

- [ ] 7.1 在单hart、单VirtIO-MMIO NIC QEMU中运行MS06 probe，逐项核对device/software/timer progress、Active quiet、continuous traffic、TCP/UDP/listener、nonblocking、poll/select/epoll、multiwaiter/overflow和close/error。每个mode必须有完整marker与exit 0；用户中断、timeout或partial marker记为未完成或失败。
- [ ] 7.2 在同一新鲜artifact和环境中运行受影响MS01/MS04/MS05 runtime回归，核对START/PASS/END、telemetry和显式exit，随后执行最终full diff review。结论只覆盖single-hart QEMU VirtIO-MMIO，不扩大到reset、SMP、真板或性能。

## Requirement Traceability Matrix

| Requirement | Design | Tasks | Iteration | Code surface | Test witness | Status |
|---|---|---|---|---|---|---|
| R1 唯一常驻runner | D1,D5,D9 | 1.1,1.5,2.4 | 000,001 | `lib.rs`, `stack_runner.rs`, TCP/UDP mutation paths | CAS/spawn、init顺序、source guard、caller-independent progress | Covered |
| R2 三源wake与register-recheck | D2,D3 | 1.1,1.4,2.4 | `stack_runner.rs`, `async_rx.rs`, `service.rs` | event交错、timer replacement、software-only wake | Covered |
| R3 budget、公平与quiet | D4 | 1.2,1.3,1.4 | `router.rs`, `service.rs`, runner telemetry | 31/32/33、双向backlog、Active idle | Covered |
| R4 锁序与guard生命周期 | D5,D9 | 1.3,1.4,2.4 | Service/SocketSet/connect/listener helpers | 100×竞争、source assertions、no-guard-across-Pending | Covered |
| R5 per-socket multi-waiter bridge | D6 | 2.1,2.2,2.5,3.1,4.3,7.1 | `readiness.rs`, `wrapper.rs`, TCP/UDP register、epoll kernel path、guest probe | 1/2/64/65、register races、global fault fan-out、host replacement/re-register + guest distinct completion | Covered |
| R6 listener/close/error一致 | D6-D9 | 2.3,2.5,3.1,3.2 | ListenTable、terminal snapshot、fault registry、TCP/UDP I/O | accept/reset、EOF/RDHUP/HUP/ERR、fatal ordering、fault-during-wait | Covered |
| R3/R6 规模化close与listener前进 | D3,D4,D7,D9,D11 | 2.6,2.7,2.8 | `stack_runner.rs`、`service.rs`、`listen_table.rs`、`tcp.rs`、`udp.rs`、smoltcp UDP、MS01 payload | listener/deferred 31/32/33/512、send→drop→peer receive、overflow终态、accept→立即reconnect | Covered |
| R7 MS06验证边界 | D10 | 2.8,4.1-4.3,5.1-5.2,6.1,7.1-7.2 | MS01 payload、guest probe、validator、axnet host harness、QEMU product paths | 分层兼容、host seam、默认并行确定性、automatic gates、single-hart runtime | Covered |
| network-stack-baseline readiness | D3-D9 | 2.1-2.8,3.1-3.2 | TCP/UDP/listener/pollable | poll→I/O matrix、多waiter、512 recovery/close storm、stable fault | Covered |
| MS05 slot consumer/owner保持 | D2,D4,D10 | 1.2,1.3,6.1,7.2 | Router、Service、queue event/slots | MS05 Full/flush/ownership regression | Covered |

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

### Iteration 004: terminal-readiness-closure

- Tasks: 3.1-3.2
- Depends on: Iteration 003 accepted
- Stable baseline: socket-local与global terminal拥有独立first-wins状态；global先提交后无条件唤醒全部已有bridge，late socket通过effective snapshot继承；blocking/nonblocking send/recv/connect/accept在fault前后都返回同一映射类别。
- Verification boundary: local-before-global、0/1/2/64/65、add/install并发、fault-during-wait、connect recheck、listener consume-once、normal EOF/HUP和MS05 fatal/flush由host/model tests覆盖，两profile full suites通过。
- Diagnostic boundary: 失败限制在terminal state ownership、registry快照/wake、TCP/UDP I/O recheck、connect/listener error映射或poll_io task-context。
- Non-goals: guest probe、自动产品资格、QEMU runtime、完整SO_ERROR消费、reset、SMP、真板和性能。

### Iteration 005: application-witness-construction

- Tasks: 4.1-4.3
- Depends on: Iteration 004 accepted
- Stable baseline: marker validator和静态RISC-V guest probe可独立构建；核心socket与普通poll/select/epoll场景有fixed-deadline，exact 64/65以同步epoll注册barrier、host replacement/re-register证据和guest distinct completion形成分层契约。
- Verification boundary: validator含START前protocol负例；host seam直接验证arm数与trigger-unit数；既有PollSet/epoll路径支持replacement/re-register结论；C syntax与static build通过；不要求启动QEMU。
- Diagnostic boundary: 失败限制在marker协议、guest syscall/task ABI、probe事件编排、deadline或交叉编译。
- Non-goals: QEMU启动、MS01/MS04/MS05 runtime、自动全量产品Gate和性能测试。

### Iteration 006: axnet-host-test-isolation

- Tasks: 5.1-5.2
- Depends on: Iteration 005 accepted
- Stable baseline: axnet host tests不再因进程级socket/listener/diagnostic共享状态产生陈旧handle、内存破坏或随机分支；ordinary与qemu-diagnostics默认并行结果可作为自动Gate依据。
- Verification boundary: R57失败子集、socket churn、diagnostics目标测试和两profile默认并行full suites重复稳定通过；不得依赖skip/ignore、无限重跑或全局串行full suite。
- Diagnostic boundary: 失败限制在test fixture实例化、global reader/writer边界、SocketHandle生命周期、diagnostic state/fake clock/telemetry隔离或测试专用并发控制。
- Non-goals: 产品socket/readiness行为、guest probe、QEMU runtime、scheduler、reset/SMP、真板和性能。

### Iteration 007: automatic-integration-qualification

- Tasks: 6.1
- Depends on: Iteration 006 accepted
- Stable baseline: 所有自动功能、ownership、兼容、build、format、OpenSpec和diff Gate通过，并生成与当前working tree一致的QEMU/probe artifact。
- Verification boundary: Task 6.1所列命令全部exit 0，默认并行host suites无flake豁免，Critical/Important finding为零；任一失败阻止QEMU runtime。
- Diagnostic boundary: 失败限制在axnet/smoltcp回归、kernel或D1 feature build、probe seam、格式/source guard、OpenSpec或diff质量。
- Non-goals: 人工QEMU输入、runtime marker、SMP、真板和性能结论。

### Iteration 008: single-hart-qemu-acceptance

- Tasks: 7.1-7.2
- Depends on: Iteration 007 accepted；用户可执行人工QEMU batch
- Stable baseline: MS06应用可见probe与受影响MS01/MS04/MS05在同一新鲜single-hart VirtIO-MMIO环境全部通过，最终diff无Critical/Important finding。
- Verification boundary: 每项runtime有环境、revision、命令、完整marker和exit 0；缺失、timeout、partial success或用户中断均不计通过。
- Diagnostic boundary: 失败限制在QEMU boot/device model、guest payload、syscall waiter调度链、runner wake或既有runtime兼容面。
- Non-goals: reset、SMP、multiqueue、多NIC、PCI/DWMAC、真板、DMA/cache和性能资格。

## Current Cycle

- Current Iteration: `008-single-hart-qemu-acceptance`
- Cycle: `000-initial.md`
- Persisted Evidence: required（README + bounded runtime marker/host-result extracts；完整串口先作为人工输入审查，入库受公共500行/256 KiB限制）
- Previous Cycle: Iteration 007 `000-initial.md` Act `reported`，Plan Review `accepted`；Task 6.1完成，残余SIGSEGV由用户明确豁免，fresh runtime artifacts已生成。
- Gate 2: `000-initial.md` 技术检查项 PASS；Plan status `draft`，等待用户审计和明确批准，未授权`openspec-act`。
- Initial scope: Tasks 7.1-7.2；按R44/R58由用户在single-hart、单VirtIO-MMIO NIC QEMU中手工运行MS06应用见证和MS01/MS04/MS05回归，随后由Act/Plan核对完整marker、exit、telemetry与最终diff。
- Deferred Iterations: None。
