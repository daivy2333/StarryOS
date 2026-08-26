## Context

MS05 已把 QEMU VirtIO-MMIO 的 RX/TX descriptor ownership 收口到唯一 queue service，并在它与 stack side 之间建立固定 64-frame slots。当前协议栈上半部仍是 caller-driven：`crates/axnet/src/lib.rs::poll_interfaces()` 同时取得 `SERVICE` 和 `SOCKET_SET`，循环调用 `Service::poll()`；TCP/UDP 的 connect、accept、send、recv、shutdown、drop 和 `Pollable::poll()` 都会主动调用它。

`Service::poll()` 当前依次执行 Router RX、smoltcp maintenance、ListenTable reconcile、无界 ingress loop、无界 egress loop和 Router dispatch。`Service::register_waker()` 把当前 socket waiter 同时注册到 protocol timer、设备和 `QUEUE_EVENT.stack_waker`，而 `AtomicWaker` 只保存最后一个 waiter。smoltcp TCP/UDP 已提供单槽 `register_recv_waker`/`register_send_waker`，axnet 尚未使用；`axpoll::PollSet` 能保存 64 个 waiter，并在 overflow 时唤醒被替换者。

关键实现事实如下：

| 事实 | 当前位置 | MS06 影响 |
|---|---|---|
| scheduler 先于 `axnet::init_network` 初始化 | registry `axruntime-0.3.0-preview.2/src/lib.rs` | Service 安装后可立即 spawn runner |
| queue task 只在 QEMU IRQ handler 注册成功后启动 | `kernel/src/drivers/virtio_net_irq.rs` | runner 必须覆盖 IRQ 激活前/失败时的 polling owner |
| queue 与 stack 各有一个 AtomicWaker，但共享 generation | `crates/axnet/src/async_rx.rs::QueueEvent` | software stack work 不能继续扰动 queue wait protocol |
| protocol timeout future 保存在 Service 且被各 socket waiter覆盖 | `crates/axnet/src/service.rs::register_waker` | timer ownership 必须收口到唯一 runner |
| TCP connect 在 SocketSet guard 内反向取得 Service | `crates/axnet/src/tcp.rs` | resident runner 引入后会形成实际锁序风险 |
| public listener 与真正 accept 的隐藏 smoltcp sockets 分离 | `crates/axnet/src/listen_table.rs` | listener 不能复用普通 public-handle bridge |
| `SocketSetWrapper::new_socket` 只 notify、无 consumer | `crates/axnet/src/wrapper.rs` | socket create 改由统一 software event 驱动 |
| queue fatal 已先持久化并发布 stack progress | `Service::flush_fault`、`RxRxFuture` | 可扩展为全 socket terminal readiness，不改变 owner |

本次调查在 revision `518acb8f82197d91ba8844c9c6a4e9eaae4b1dd7` 上已有新鲜基线：普通 axnet lib tests 218/218、`qemu-diagnostics` axnet lib tests 238/238、QEMU kernel check 通过。单独执行 `starry-kernel --features lichee-d1` 因根 workspace feature 组合未展开而出现既有 unresolved imports，该命令不作为 MS06 D1 Gate；MS06 只要求受支持的根 feature 组合继续编译。

## Goals / Non-Goals

**Goals:**

- 用一个常驻 runner 代替 socket API 主动推进 smoltcp。
- 在同一 runner 中无丢失地合流 device、software 和 timer wake。
- 为 Router RX、smoltcp ingress/egress 和 dispatch 提供固定 budget与公平让出。
- 用 per-socket `PollSet` bridge 支持 TCP、UDP 和 listener 多 waiter。
- 使 `IN/OUT/RDHUP/HUP/ERR` 与下一次实际 I/O、EOF、close 和稳定 fault 一致。
- 固定锁序并证明任何 guard 不跨 `await`/`Pending`。
- 保持 MS04/MS05 唯一 queue owner、slots、flush 和诊断 ABI。

**Non-Goals:**

- reset/cancel generation、link flap、queue recovery 或自动重新激活。
- SMP、multiqueue、多物理接口公平性、PCI、DWMAC 和真板行为。
- 替换或扩容 axpoll、修改 smoltcp 的单槽 waker 实现。
- 零拷贝、offload、IRQ moderation 或性能资格。
- 改变 Unix/vsock readiness；本次只处理 TCP、UDP 和 IP listener。

## Architecture

```text
VirtIO IRQ -> queue event -> unique queue service -> RX-ready/TX-space/fault
                                                   |
socket mutation -----------------------------------+-> StackEvent generation
smoltcp poll_at -------------------------------- timer -> unique stack runner
                                                        |
                          Service -> SocketSet -> ListenTable
                                                        |
                 smoltcp single-slot waker -> Arc<PollSet> -> app waiters
```

queue service 仍唯一持有 descriptor、token、reclaim 和 queue-control。stack runner 是唯一协议轮询 task，但同步 socket API 仍可在 `SOCKET_SET` 锁下修改自己的 smoltcp socket buffer/state；两者通过锁串行化，socket path 不直接执行协议推进。

## Decisions

### D1：在 Service 安装后启动唯一 runner

**Decision**：在 `init_network()` 完成 `SERVICE.call_once` 后调用 `start_stack_runner()`。runner lifecycle 使用独立原子状态 `NotStarted → Spawned → Running`，重复启动返回稳定 `AlreadyStarted`，固定 task name 为 `axnet-stack-runner`。产品 socket path 删除主动 `poll_interfaces()`；该函数保留为受锁保护的兼容/测试单轮 helper，不再被仓库产品代码调用。

**Reason**：axruntime 已先初始化 scheduler，再初始化 driver/axnet；这是 QEMU 与未来平台共享且不依赖 kernel 私有启动顺序的最早安全位置。此时 IRQ 可能尚未注册，但 queue lifecycle 仍明确为 polling owner，runner 可用有界 fallback，不会抢占 descriptor owner。

**Impact**：`crates/axnet/src/lib.rs` 新增 lifecycle/start API 和初始化调用；kernel 无需再维护第二个 stack task 启动点。host tests通过可注入 spawn seam 验证 CAS，不能推进 production global lifecycle。

**Alternatives**：

- 在 `kernel/src/entry.rs` 的 IRQ 初始化后启动：拒绝，因为 D1/其他 axnet consumer 没有共同入口，IRQ 注册失败也会失去 polling progress。
- 让每个 blocking socket operation 临时充当 runner：拒绝，因为仍是 caller-driven，timer 和无 socket 调用时不能进展。

### D2：建立独立 StackEvent，不复用 queue generation

**Decision**：新增只有唯一 runner 注册的 `StackEvent { generation, waker }`。queue service 在 RX slot ready、TX slot space和 fault commit 后调用 `publish_device()`；socket mutation 解锁后调用 `publish_software()`；两者都以 Release 增加 generation再 wake。现有 `QueueEvent` 继续只负责 queue owner，`software_nudge()` 保持其既有诊断/queue 语义，不能被普通 socket wake 复用。

runner 每次 poll 执行：

1. Acquire 读取 generation。
2. 在任何网络锁外注册 runner waker。
3. 取得有序锁并执行有界 round、计算 `poll_at` 和 fallback 状态。
4. 释放全部锁，arm/更新本地 timer。
5. Acquire 重读 generation；若变化、存在可推进 backlog 或 deadline 已到，则 self-wake；否则返回 `Pending`。

**Reason**：普通 socket send/config 只产生 stack work。若复用 MS05 shared generation，queue task会把每个 socket mutation误判为 queue event，增加无关 poll并模糊 quiet 证明。

**Impact**：MS05 的 stack-progress publication 改为委托 `StackEvent`；queue owner register-recheck及其 telemetry 不变。StackEvent 只携带提示，实际 readiness始终由 runner/sockets重检。

**Alternatives**：

- 继续使用 `QUEUE_EVENT.stack_waker`：拒绝，因为 software-only generation 会干扰 queue wait protocol。
- 为三类 source 各建一个 future并 `select`：拒绝，因为项目没有现成无分配 select primitive，而且 generation仍需关闭跨 source lost-wakeup。

### D3：runner 独占 timer ownership

**Decision**：从 `Service` 移除每 waiter覆盖的 `timeout` future与 `register_waker()` timer职责。runner 每轮只采样一次时钟，并把同一个 `Instant` 传给 Service 的 Router、maintenance、ingress、egress、dispatch 和 `Interface::poll_at`；释放锁后用该时间戳维护一个 deadline和一个 `sleep_until` future。deadline 变化时替换旧 future；迟到旧 wake只导致一次 spurious bounded round。host/model 的 injected clock 必须沿同一路径传入 Service，不能只控制 runner timer 而让协议栈继续读取另一时钟。

fallback deadline按以下顺序决定：

| queue/device 状态 | fallback |
|---|---|
| target lifecycle `Polling/Spawned/Unavailable` | 10ms |
| lifecycle `Active` 且设备 IRQ-backed | none |
| lifecycle `Faulted` | none；发布稳定 error |
| 非 target device 明确 `requires_polling()` | 10ms |
| smoltcp `poll_at` 更早 | 使用 protocol deadline |

**Reason**：timer 是协议栈推进条件，应由唯一推进者拥有。以 lifecycle 而非单独 `irq_num()` 决定 QEMU fallback，可覆盖 IRQ 注册/preflight 失败但设备仍声明 irq number 的情况。

**Impact**：`Service::register_waker()`被拆除；其 deadline helper保留为纯函数测试。runner tests使用 fake clock/timer seam，不依赖 wall-clock sleep。

**Alternatives**：

- 每 socket waiter各自 arm timer：拒绝，因为最后注册覆盖、重复 timer和无 waiter时停滞。
- Active 设备继续固定 10ms兜底：拒绝，因为无法证明 idle zero-poll。

### D4：每 stage 固定 budget，全部 stage 每轮都有机会

**Decision**：首版使用与 queue task一致的 `STACK_STAGE_BUDGET = 32`，分别约束 Router RX、smoltcp ingress、smoltcp egress、Router dispatch、ListenTable pending reconciliation 和 deferred socket retirement。maintenance每轮执行一次；listener queue 的主 reconciliation 只在各网络 stage 之后执行一个有界批次，不得在每个 ingress step 后重复全表扫描。ListenTable在active ports与各entry slots之间保存全局持久cursor，所有listener共享每轮32次slot检查预算，且不得每轮clone完整active-port列表；deferred retirement使用独立cursor与32-entry预算。listener增删或`accept`移除queue slot必须推进结构generation；active sweep观察generation变化后从安全位置有界重启，不能仅用`cursor > len`判断收缩，也不能依赖ingress/egress再次报告protocol progress。没有结构变化或未确认entry时不得让下一轮重新从列表头扫描。任一stage返回“仍可立即推进”时，runner在释放锁后self-wake。

隐藏 listener 的 one-shot recv waker 另行登记精确的 listener-head signal。每个已处理 ingress packet 之后，Service 至多消费一个去重 signal，并只对该 entry 执行 O(1) idle transition/refill/rearm；这一 micro-step 不扫描active ports或pending queue，也不推进协议。其每轮次数不得超过本轮已处理的 ingress packet 数，因此上限同为32；主 reconciliation 仍独立保留32-token预算。signal queue在listener注册路径预留容量，waker路径不得分配内存、取得entry/SocketSet/Service锁或直接唤醒application；entry状态提交后沿既有staged wake路径唤醒accept waiter。

`Router::poll` 和 `Router::dispatch` 拆出单步/有界 API；Router RX 保存 round-robin device cursor，避免 loopback backlog永久挡住 target NIC。结果结构显式返回 processed count、socket state change、backlog、RX-space release、TX enqueue 和 stable fault，不用 bool 同时表达多个含义。

**Reason**：当前 Router和Service中的多个 while-loop都可能在持续流量下独占 CPU。分 stage budget比一个全局 budget更容易证明 ingress/egress双方都获得机会，并与 MS05 queue budgets形成一致诊断模型。

**Impact**：`router.rs`、`service.rs`和`listen_table.rs`增加round outcome和31/32/33边界tests；telemetry至少记录runner polls、各stage work、budget hit、self-yield、device/software/timer wake和fallback tick。listener reconciliation与deferred retirement的检查数、剩余backlog和回收数进入同一outcome/test seam，使512 backlog或close storm不能隐藏在既有stage budget之外。

**Alternatives**：

- 一个全局 32-work budget：拒绝，因为前置 Router RX可消耗全部额度并饿死socket egress。
- 继续 drain-to-empty后调用 `yield_now`：拒绝，因为到达率高于处理率时永远到不了 yield点。

### D5：固定 `SERVICE → SOCKET_SET → ListenTable entry` 锁序

**Decision**：所有需要两个以上网络对象的路径遵循：

```text
SERVICE
  -> SOCKET_SET.inner
       -> ListenTable active_ports / entry
```

新增有序 helper用于 runner round和 TCP connect。source address/device mask等只读 Service查询在取得 SocketSet前完成；需要 `Interface::context()` 的 connect在同一个有序临界区完成。listener helper在 caller已持SocketSet时只取 entry，不得内部再次取 SocketSet。所有 wake、timer arm、self-yield和`Pending`都发生在guard释放后。

**Reason**：当前 TCP connect在 `with_smol_socket()` closure内调用 `get_service()`，与 runner的 Service→SocketSet相反。常驻任务会把潜在顺序问题变成可复现 deadlock。

**Impact**：调查期间识别到的 TCP/UDP/listener调用点都进入source assertion和lock-order host tests。`axsync::Mutex`可能阻塞当前 task，因此不能把“最终能拿到锁”当成无死锁证明。

**Alternatives**：

- runner用 `try_lock` 并在失败时 self-wake：拒绝，因为会把锁序错误隐藏为高CPU retry并可能饿死socket owner。
- 把所有 socket操作改为message passing给runner：拒绝，本轮会扩大到完整异步socket命令队列和取消语义。

### D6：per-socket ReadinessBridge + registry

**Decision**：为public TCP/UDP handle建立 `Arc<ReadinessBridge>`，至少包含 read、write和terminal `PollSet`。`SocketSetWrapper`维护 handle→bridge registry；public socket持有同一Arc，remove时先从registry摘除并wake遗留waiter。smoltcp recv/send slot注册的Waker由对应 `Arc<PollSet>`构造。

`Pollable::register`执行：

1. application waker按interest登记到对应PollSet；ERR/HUP waiter也登记terminal set。
2. 在SocketSet guard内把bridge waker登记到smoltcp recv/send slot。
3. 释放guard后重检 `poll()`；若已ready，wake对应PollSet。

runner观察稳定全局fault后从registry取得Arc快照，释放registry lock，再wake全部terminal/read/write sets。这样wake callback不会在registry或Service guard内执行。

**Reason**：smoltcp每方向只有一个waker，适合指向一个扇出集合；application waiter不应互相覆盖。registry使设备级fatal可到达所有public IP sockets，而无需修改smoltcp或建立容量64的全局共享热点。

**Impact**：同一socket每方向继承`PollSet`容量64。第65个注册会唤醒被替换者；所有waiter都必须把wake视为重检提示。host tests使用counting wakers覆盖1、2、64、65和重复register。

**Alternatives**：

- 一个全局 PollSet：拒绝，因为不同socket互相消耗64容量并在高并发下产生无关wake。
- 修改smoltcp让它直接保存Vec<Waker>：拒绝，因为扩散OS策略、容量和锁语义到协议库。

### D7：listener由ListenTable独立桥接

**Decision**：public `TcpSocket`进入Listening时把自己的accept `Arc<PollSet>`交给 `ListenTableEntryInner`。entry为每个listener建立内部head signal；创建idle socket、refill和reconcile时，把该signal waker登记到隐藏smoltcp socket的recv slot。waker只把该listener的去重signal提交到预留queue，不读取或修改entry/socket，也不直接唤醒application。Service在同一ingress batch的下一个packet前消费signal，执行该entry的O(1) idle transition/refill/rearm；普通有界reconcile继续处理pending queue。状态从Pending变为Ready/Reset时先更新queue，再通过guard外staged wake唤醒accept set。被动打开在`SynReceived`收到RST并回到`Listen`时，该pending slot不构成连接：entry在没有idle时把它恢复为idle，已有idle时移除冗余hidden socket，不能把它永久留在pending queue。满backlog的accept消费一个Ready/Reset slot时，在同一`SOCKET_SET → ListenTable entry`临界区恢复一个idle hidden listener，再释放guard和发布software wake。

满backlog验证把两个网络事件分开：额外overflow connect必须先达到成功、拒绝或超时等可判定终态，之后才释放headroom并发起recovery connect。已经在runner ingress中排队的旧SYN可以合法占用稍后释放的slot，不能用这种竞态证明atomic refill失败。host/model另行覆盖overflow RST后hidden socket返回`Listen`的恢复路径；QEMU recovery场景只证明headroom提交后新的connect不依赖额外caller-driven progress。

注册顺序为application PollSet register → 取得SocketSet和entry → hidden socket bridge register → entry readiness recheck → 解锁后必要wake，遵循D5。

**Reason**：public listener自己的smoltcp handle不接收SYN；只给它注册waker永远不会得到accept transition。隐藏socket在每次wake后还必须重臂单槽waker。

**Impact**：ListenTable entry增加accept bridge、内部head signal与pending cursor，但固定512 backlog语义不变；Ready connection仍只交付一次，Reset继续由accept返回`ConnectionReset`。head-signal/refill与accept/refill helper接收现有SocketSet guard，只做hidden socket生命周期提交，不调用smoltcp progress，也不反向取得Service guard。MS01 payload保留14个marker，但不再让相邻SYN因同一ingress batch内缺少Listen-state socket而被错误RST，也不让未判定的overflow SYN与recovery SYN竞争同一个新headroom。

**Alternatives**：

- runner每轮扫描并直接wake public listener：拒绝，因为public handle与entry当前无反向索引，而且会把精确事件退化为全量扫描。
- listener周期poll：拒绝，违反quiet path。

### D8：readiness映射由状态快照统一计算

**Decision**：TCP/UDP各提供一个在短SocketSet临界区内计算的readiness snapshot，`poll()`只映射该snapshot，不推进协议栈。规则如下：

| 状态 | Events | 下一 I/O |
|---|---|---|
| TCP buffered data | `IN` | read返回bytes |
| TCP peer EOF，buffer已空 | `IN|RDHUP` | read返回0 |
| TCP local read shutdown | `RDHUP`，保留当前兼容read错误 | read返回现有shutdown错误 |
| TCP established且`can_send` | `OUT` | write接受至少1 byte或记录并发race |
| TCP connect成功 | `OUT` | completion成功 |
| TCP connect失败 | `OUT|ERR` | completion返回保存的连接错误 |
| TCP双向终止 | `HUP`，需要时同时`IN/RDHUP/ERR` | read EOF或稳定错误；write失败 |
| listener Ready | `IN` | accept唯一连接 |
| listener Reset | `IN|ERR` | accept返回`ConnectionReset` |
| UDP `can_recv/can_send` | `IN/OUT` | 完整datagram recv/send |
| UDP closed | `HUP` | recv/send返回关闭错误 |
| stable data-plane fault | `ERR`（所有IP socket） | 操作返回稳定DevError→AxError映射 |

连接级错误保存在socket-local terminal state；设备级fatal使用D6 registry广播的稳定code。`OUT`不再由`!may_send`伪造。`poll_io`和epoll已有外层check-register-recheck，bridge内部仍按D6重臂，两个层次共同关闭race。

**Reason**：当前TCP把`!may_send`当OUT，会让关闭连接看似可写；RDHUP只观察本地flag；UDP关闭后返回空events。显式snapshot让poll与send/recv/accept共用判定来源。

**Impact**：`GeneralOptions::Error`仍不在本轮扩展为完整Linux `SO_ERROR`消费语义；但blocking/nonblocking操作必须读取同一terminal state并返回稳定错误。若实现发现兼容必须依赖SO_ERROR消费，停止并返回Plan重规划。

**Alternatives**：

- 保留现有IN/OUT并只修waker：拒绝，因为多waiter会被唤醒到与I/O矛盾的状态，无法满足T10。
- 把所有close都映射ERR：拒绝，因为正常EOF/HUP不是设备或连接错误。

### D9：software wake在状态提交和解锁后发布

**Decision**：socket add/bind/connect/listen/send enqueue/shutdown/remove和listener refill等mutation必须遵循“commit under lock → drop guard → `StackEvent::publish_software`”。允许合并同一API调用内的重复wake，但不得在状态提交前wake。read-only poll/local_addr/peer_addr不得无条件wake。

**Reason**：wake-before-commit可使runner运行后再次睡眠，随后状态提交却无event；在锁内wake可能立即调度竞争同一mutex。统一post-commit规则也是source review可检查的边界。

**Impact**：删除`SocketSetWrapper::new_socket`的无consumer Event或把它替换为StackEvent；mutation helper返回是否需要wake，避免closure内直接发布。

**Alternatives**：

- 所有`with_socket_mut`调用都wake：拒绝，因为option/read-only查询也走mutable smoltcp API，会产生quiet噪声。

### D10：验证以host/model为主，QEMU证明应用可见链路

**Decision**：Iteration内先建立确定性host/model tests，再运行单hartQEMU。新增MS06 guest probe覆盖无需主动poll的timer/traffic progress、poll/select/epoll多waiter、64/65 overflow、listener、close/error和fixed deadline；复用MS01 socket payload与MS04/MS05 probe模式做回归，不扩建I16 benchmark。

QEMU runtime原始命令和marker写入Act Response；本计划默认 `Persisted Evidence: none`，除非执行时发现运行不可低成本复现并经Plan重新批准。任何历史artifact只作基线参考，不能替代当前revision结果。

**Reason**：host tests能穷举generation和budget交错，但不能证明axtask timer、VirtIO device-model IRQ、queue task、runner和syscall waiter的完整调度链。反过来，单次QEMU成功不能穷举lost-wakeup。

**Impact**：自动Gate包括ordinary/qemu-diagnostics axnet tests、kernel QEMU check、受支持root feature checks、probe self-tests、fmt、strict OpenSpec、source assertions和full diff review。runtime结论只覆盖单hartQEMU。

**Alternatives**：

- 只运行已有MS01 poll：拒绝，它不证明多waiter、overflow、idle或caller-independent progress。
- 只依赖QEMU计数器：拒绝，无法稳定制造所有register交错。

### D11：UDP drop按真实pending TX延迟raw handle回收

**Decision**：本地smoltcp UDP socket提供一个只读`has_pending_tx()`查询，语义仅为“TX packet buffer非空”。`can_send()`继续表示“TX buffer未满”，不得用于判断是否存在待派发datagram。public UDP drop先在SocketSet guard内读取`has_pending_tx()`：没有pending TX时沿用立即close/remove；存在pending TX且runner可用时，只退役public metadata并提交`UdpQueued` deferred entry，由唯一runner完成egress，待`has_pending_tx()`变为false后在同一guarded commit中移除raw handle和entry。

UDP deferred verdict必须先按`CloseKind::UdpQueued`与实际socket类型匹配，再进入通用TCP分支；stale或retyped entry只删除deferred记录，不触碰新socket。每次成功UDP egress、fresh enqueue或未完成sweep都可启动下一次有界检查；空队列和完整quiet sweep不能持续self-wake。

**Reason**：guest日志证明fork child在`sendto`后立即drop，现有`close()`清空smoltcp TX buffer并丢失echo。`can_send()`只检查buffer是否未满，空buffer和含一个datagram的buffer通常都返回true，无法形成正确的drop或reap判定。axnet侧影子计数无法在不新增dispatch回调和第二份ownership状态的情况下可靠观察dequeue。

**Impact**：修改本地`crates/smoltcp/src/socket/udp.rs`的只读API及unit test，并修改axnet UDP drop、deferred verdict和host witnesses；不改变UDP发送、队列容量、dispatch、错误或wire语义。该本地依赖变化需要用户批准后才能执行。

**Alternatives**：

- drop内同步派发一个datagram：拒绝，因为socket caller会成为第二个协议栈推进者并可能形成反向锁序。
- 所有UDP drop无条件延迟一轮：拒绝，因为仍缺少可靠完成判定，空socket会增加不必要生命周期并可能泄漏raw handle。
- axnet维护pending计数：拒绝，因为send enqueue可计数，但smoltcp dequeue没有现成的axnet回调；复制状态会引入新的失配和恢复问题。

## Risks / Trade-offs

- [每stage budget仍可能使一次runner poll较长] → 每个stage独立限制32，固定执行顺序并记录work/budget-hit；若自动Gate显示调度延迟不可接受，先调小常量而不改变contract。
- [PollSet overflow会产生spurious wake与重注册竞争] → 沿用axpoll既有wake-on-replacement，不把wake当ready；以64/65 counting-waker和blocking waiter测试证明无静默丢失。
- [bridge registry增加handle生命周期状态] → public socket创建时原子安装handle+Arc，remove先摘除再wake；hidden listener sockets不进入public registry。
- [全局fault广播可能产生wake storm] → fault为稳定单次transition，先取Arc快照再解锁wake；重复观察同一code不得重复广播。
- [runner启动早于IRQ注册] → lifecycle Polling/Spawned使用10ms fallback；queue激活后停止fallback，Faulted不回退。
- [重构Router单步API可能影响MS05 slot/dispatch语义] → 保持typed outcome、packet head ownership和ticket规则，重跑MS05Full/recovery、flush和守恒tests。
- [close语义与现有应用兼容有差异] → snapshot明确区分正常EOF/HUP和error；MS01 TCP/UDP/listener/nonblocking回归为阻塞Gate，发现SO_ERROR等新契约需求时停止重规划。

## Migration Plan

1. 建立StackEvent、runner lifecycle、timer和有界Service/Router round，保留caller-driven helper作为对照；host tests先证明T09。
2. 在`init_network`启动runner，加入software mutation wake；确认Active quiet和Polling fallback后，删除TCP/UDP产品路径中的主动`poll_interfaces()`调用。
3. 加入per-socket bridge registry和普通TCP/UDP recv/send waker重臂，先关闭read/write多waiter。
4. 加入listener accept bridge、terminal snapshot、fault广播和close/error映射。
5. 在Iteration 001收口listener reconciliation：统一runner/Service round timestamp，跨active listeners共享固定budget，并恢复passive RST后回到Listen的hidden socket ownership。
6. 在Iteration 002单独闭合UDP queued-TX drain ownership；host/model GREEN后，Iteration 003再分开验证overflow终态、exact-512 recovery和MS01 single-hart QEMU兼容。
7. 在Iteration 004完成terminal readiness、完整自动Gate和单hartQEMU application acceptance；发现回归时保持runner lifecycle可诊断，不以重新启用socket内同步poll作为修复。

回滚只能按Iteration稳定边界进行：Iteration 000未被后续依赖前可回到caller-driven基线；一旦Iteration 001切除产品inline poll，回滚必须同时恢复所有调用点和旧timer ownership，不能形成“无runner且无caller poll”的混合状态。

## Open Questions

无影响实施的开放问题。`SO_ERROR`消费、多接口runner、SMP wake和reset后的runner lifecycle明确留给后续change；若实现表明它们是当前Acceptance的必要条件，Plan Review必须判定`replan-required`而不是扩大Cycle。
