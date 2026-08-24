# Iteration 001 / Cycle 000: Socket and Listener Readiness Bridge

## Plan Context

- Status: ready
- Approval: 用户于 2026-08-23 明确批准本 Cycle，原话：“批准”。
- Iteration: 001-socket-and-listener-readiness-bridge
- Cycle: 000-initial
- Cycle Type: initial
- Parent cycle: None

**Iteration Scope**

- Change tasks: 2.1–2.5
- Depends on: Iteration 000 accepted
- Stable baseline: 产品 TCP/UDP/listener 不再主动执行 `poll_interfaces()`；每个 public
  socket 通过独立 `PollSet` bridge 支持多 waiter，listener 的 hidden sockets 能唤醒
  public accept waiter，普通 data/EOF/writable readiness 与紧随其后的 I/O 一致。
- Verification boundary: bridge/registry 生命周期、smoltcp 单槽重臂、1/2/64/65 waiter、
  listener hidden socket、全局锁序、post-commit software wake、产品 caller-driven
  调用点为零以及 MS01 socket 兼容全部通过。
- Diagnostic boundary: 失败限制在 readiness registry、TCP/UDP bridge、ListenTable bridge、
  Service/SocketSet/listener 锁序、software wake 或普通 readiness mapping。
- Deferred tasks: 3.1–3.4

**Cycle Scope**

- Trigger: initial
- Acceptance gaps: None
- Repair items: None
- Inherited scope: proposal 的 multi-waiter、listener、caller-independent progress 和
  compatibility 场景；design D1–D9 中与 T10 普通 readiness 相关的约束；Iteration 000
  已接受的唯一 runner、独立 StackEvent、bounded round、timer/fallback 和 guard 生命周期；
  MS05 的唯一 queue owner、64-frame slots、typed backpressure 与 ticketed flush。
- Excluded scope: device-wide stable fault 广播、connect/listener terminal error 的最终
  `ERR` 映射、MS06 guest probe、最终 QEMU application acceptance、reset、SMP、真板、
  性能、全局文档维护和 change 归档。

**Objective**

把 Iteration 000 的 resident runner 原子切换为应用可依赖的普通 socket readiness：
TCP、UDP 和 listener 的 blocking I/O、poll、select 与 epoll 通过 per-public-handle bridge
获得重检机会；所有 socket mutation 在状态提交和解锁后只唤醒 stack runner；仓库产品
TCP/UDP/listener 不再主动推进 smoltcp。Cycle 完成后，普通数据、EOF、half-close、可写、
UDP 关闭和 listener accept/reset 已闭合；稳定数据面 fault 与最终 error matrix 留给
Iteration 002。

**Background**

Iteration 000 已建立唯一 stack runner，但为避免无 waker 的迁移中间态，仍保留 12 个
TCP/UDP 产品 `poll_interfaces()` 调用、`GeneralOptions::register_waker()`、
`Service::register_waker()` 和 Service-owned timeout。smoltcp 的 TCP/UDP recv/send waker
各只有一个槽，后注册者会覆盖前者；当前 socket waiter 因此只能依赖一个全局 stack
waker。`SocketSetWrapper::new_socket` 使用 `event_listener::Event` 发布创建事件，但仓库
没有 consumer。public listener 与实际接收 SYN 的 hidden sockets 分离，给 public handle
注册普通 TCP waker无法唤醒 accept。

TCP connect 还在 `SOCKET_SET.with_socket_mut` closure 内调用 `get_service()`，与 runner 的
`SERVICE → SOCKET_SET` 顺序相反。引入 resident runner 后，这条反向边会从潜在问题变成
可复现死锁风险。当前 `axtask::future::poll_io` 已执行 I/O check → application register →
I/O recheck；kernel epoll add/modify 也执行 poll → register → poll。Iteration 001 只需让
socket-level register 正确桥接 smoltcp 单槽并保持同样重检语义，不修改 syscall 层。

**Current Baseline**

- Revision: `b8e7bcae27579aa7ea7bf31698e3136f5856302d`，branch `net-k3`；MS06 实现位于
  未提交工作树。
- Iteration 000 / Cycle 001 Review 为 `accepted`；Tasks 1.1–1.5 已关闭。
- Fresh Review baseline：ordinary axnet 244/244、qemu-diagnostics 264/264、MS04 host
  harness 16/16、QEMU kernel check、root D1 target check、`make lichee`、fmt、strict
  OpenSpec 和 diff check 均通过。
- `make lichee` 的 Cargo home/联网安装探测告警属于已分类环境噪声；release build、
  objcopy 与 boot image inspect 完成且最终 exit 0。
- `rg 'poll_interfaces\('` 当前命中一个公共 helper 定义、TCP 8 个产品调用和 UDP 4 个
  产品调用。Iteration 001 的产品调用目标为 0；公共 helper 可保留用于兼容/测试。
- `wrapper.rs` 的 `new_socket: Event` 是 `event-listener` 在 axnet 内的唯一使用点，且无
  consumer。若 bridge 替换后全 crate 零引用，应删除该直接依赖；不得为清理扩大到其他 crate。

**Current-State Evidence**

- `SocketSetWrapper` 当前只保存 `SocketSet` 与 TCP bound endpoint map。`add()` 返回
  `SocketHandle` 后通知无 consumer 的 `new_socket`；`remove()` 先移除 smoltcp handle，
  再移除 bound metadata，没有 public-handle readiness registry。
- `TcpSocket` 只保存 handle、public state、`GeneralOptions`、local `rx_closed` 和一个
  `poll_rx_closed`。`UdpSocket` 没有任何方向性 PollSet。accepted hidden TCP handle 通过
  `TcpSocket::new_connected(handle)` 直接提升为 public socket，当前没有 adoption lifecycle。
- TCP/UDP `Pollable::register` 把 IN/OUT waiter交给
  `GeneralOptions::register_waker → Service::register_waker`。Service 在持有 Service guard
  时取得 SocketSet，替换单一 timeout，并注册 device/global stack waker；该路径无法保存
  同一 socket 的多个 waiter。
- smoltcp TCP/UDP 的 `register_recv_waker` 与 `register_send_waker` 是 async feature 下的
  one-shot single-slot API：后注册覆盖前者，wake 后必须重臂，spurious wake 合法。
- `axpoll::PollSet` 固定容量 64；第 65 个不同 waker 替换环形槽并唤醒被替换者。
  `wake()` 在释放内部 mutex 后唤醒全部保存项，适合用作 smoltcp 单槽后的应用扇出。
- `axtask::future::poll_io` 在首次 I/O 返回 `WouldBlock` 后调用 `Pollable::register`，再执行
  同一 I/O closure；poll/select 的 `FdPollSet` 注册全部文件，epoll add/modify 也有外层
  check-register-recheck。socket bridge 的 wake 只能表示“重新检查”，不能表示 I/O 成功。
- `ListenTableEntryInner` 保存一个 idle hidden socket 和最多 512 个 Pending/Ready/Reset
  slots。`Service::stack_round` 在持有 Service 与 SocketSet 时调用 reconcile；public
  listener 的 `poll_listener()` 只看 queue，当前没有 bridge 指向 hidden socket。
- TCP connect 在 `with_smol_socket` 持有 SocketSet 后于 closure 内三次取得 Service 数据或
  `Interface::context()`；这是已定位的反向锁边。runner 的正常顺序是 Service → SocketSet。
- 当前 readiness 映射把 TCP `!may_send` 误报为 OUT，只用 local `rx_closed` 产生 RDHUP；
  UDP 未绑定或关闭均可能返回空 events。send/recv 的真实成功条件分别使用
  `can_send`、`can_recv`、`may_recv` 和 `is_open/is_active`。
- kernel `file::net::Socket` 只委托 axnet `poll/register`；本 Iteration 不需要修改 kernel
  poll/select/epoll 实现。

**Relevant Code**

| File / Symbol | Current Responsibility | Cycle Use |
|---|---|---|
| `crates/axnet/src/wrapper.rs::SocketSetWrapper` | smoltcp handles 与 TCP bound metadata | public bridge install/adopt/remove registry |
| `crates/axnet/src/readiness.rs`（new） | 不存在 | read/write/terminal PollSet、direction waker 与生命周期 seam |
| `crates/axnet/src/tcp.rs::TcpSocket` | TCP I/O、public state、listener 与旧 poll/register | 保存 bridge、重臂 smoltcp、锁序修复、cutover 与 snapshot |
| `crates/axnet/src/udp.rs::UdpSocket` | UDP I/O、endpoint 与旧 poll/register | 保存 bridge、重臂 smoltcp、software wake 与 snapshot |
| `crates/axnet/src/listen_table.rs` | hidden listener pool、reconcile、accept/cleanup | accept bridge、hidden 重臂、post-commit wake |
| `crates/axnet/src/general.rs` | poll_io timeout 与旧 Service waker入口 | 保留 timeout/poll_io，删除全局 stack waker注册 |
| `crates/axnet/src/service.rs::register_waker/timeout` | 每 waiter protocol/device timeout | 删除 socket-owned timer/waker职责，timer仍由 runner独占 |
| `crates/axnet/src/lib.rs` | globals、init、compat poll helper | 注册新模块、有序网络 helper、source cutover guard |
| `axpoll-0.1.2::PollSet` | 64 waiter、overflow replacement、unlock-before-wake | 直接复用，不修改依赖 crate |
| `smoltcp::socket::{tcp,udp}::register_*_waker` | one-shot single waker slots | 指向每 socket bridge，不修改 smoltcp |

**Critical Path**

```text
blocking I/O / poll / select / epoll
  -> first readiness or I/O check returns not-ready
  -> register application waker in per-socket PollSet
  -> lock SocketSet and register bridge waker in smoltcp recv/send slot
  -> snapshot recheck
  -> unlock
  -> transition wake or recheck wake
  -> application retries actual I/O

VirtIO/loopback progress or socket mutation
  -> state commit
  -> release Service/SocketSet/listener/readiness guards
  -> StackEvent generation publish
  -> resident runner bounded round
  -> smoltcp single-slot bridge wake
  -> PollSet fanout

listener SYN
  -> hidden idle/pending smoltcp socket changes
  -> ListenTable reconcile commits Pending -> Ready/Reset and refills
  -> release Service -> SocketSet -> entry guards
  -> accept bridge wake
  -> one accept winner consumes the slot; other waiters recheck
```

**Implementation Guidance**

先建立可独立测试的 bridge 与 public-handle registry，再接入 TCP/UDP smoltcp 单槽；随后
把 listener hidden sockets 接到 public accept bridge。bridge 路径 GREEN 后，统一修正
`SERVICE → SOCKET_SET → ListenTable entry` 锁序和 post-commit software wake，并一次性
删除 12 个产品 inline-poll 调用与 Service-owned socket timeout/waker。最后让 poll 与
I/O 复用普通 readiness snapshot，关闭 data/EOF/writable/UDP close 语义。

listener reconcile 可能在同一 round 改变多个 entry。实现可返回待发布句柄，或记录
entry-local pending wake 并在网络 guards 全部释放后发布；不得在 Service、SocketSet、
entry 或 registry guard 内调用 application waker，也不得退化为 quiet path 周期扫描。

**Behavioral Change**

- 每个 public TCP/UDP handle 获得独立 read/write/terminal readiness sets；hidden listener
  socket 只有被 accept 为 public handle 后才进入 public registry。
- smoltcp recv/send slot 只保存对应 socket bridge waker；application waiter 数量不改变
  smoltcp 容量，第 65 个 waiter沿用 PollSet replacement + wake + recheck。
- socket 创建、bind/connect/listen、成功 send enqueue、非 PEEK recv dequeue、accept/refill、
  shutdown/remove 以及影响 smoltcp 进展或 timer 的 option mutation 在提交并解锁后发布
  stack-only software wake。纯查询、失败前置检查和 PEEK 不产生无条件 wake。
- TCP/UDP/listener 产品路径不再调用 `poll_interfaces()`；runner 独占协议推进和 timer。
- TCP `OUT` 只表示 `can_send` 或 connect completion；buffered data/EOF/half-close/HUP 和
  UDP IN/OUT/HUP 按 snapshot 映射。稳定 data-plane fault、connect failure `ERR` 和
  listener reset `ERR` 的最终 terminal code 留给 Iteration 002。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| 2.1 | R5 / bridge lifecycle、64/65 | `readiness.rs`、`wrapper.rs`、TCP/UDP constructors | 无 registry；Event 无 consumer | per-handle Arc bridge 与 install/adopt/remove |
| 2.2 | R5 / multi-waiter、register race | `tcp.rs::register`、`udp.rs::register` | 全局 Service waker | per-direction app register、smoltcp bridge register、recheck |
| 2.3 | R6 / listener accept/reset | `listen_table.rs`、`tcp.rs` listener paths | hidden socket 无 public waker | accept bridge、hidden rearm、post-unlock wake |
| 2.4 | R1,R2,R4 / caller-independent progress | `lib.rs`、`service.rs`、`general.rs`、`wrapper.rs`、TCP/UDP/listener mutations | 12 个 inline poll、reverse connect lock、Service timeout | ordered helper、post-commit wake、product cutover |
| 2.5 | R6 / data、EOF、writable、UDP close | `tcp.rs`、`udp.rs`、`readiness.rs` tests | readiness 与 I/O 不一致 | shared ordinary snapshots 与 poll→I/O matrix |

**Task Contracts**

### 2.1: 建立 per-public-handle ReadinessBridge 与 registry

- Requirement/Scenario: R5；per-socket multi-waiter、PollSet 容量边界、socket remove。
- Depends on: Iteration 000 accepted。
- Targets: 新 `crates/axnet/src/readiness.rs`；`wrapper.rs::SocketSetWrapper`；
  `lib.rs` module wiring；TCP/UDP constructors 与 accepted-handle adoption；若
  `event_listener` 零引用则删除 axnet manifest 中该直接依赖。
- Current behavior: public socket只有 handle；创建通知使用无 consumer Event；remove 不知道
  waiter；accepted hidden handle没有 public lifecycle 安装点。
- Required behavior: 每个 public handle 恰有一个共享 `Arc<ReadinessBridge>`，包含 read、
  write、terminal `PollSet`；new socket 原子安装 handle+bridge，accept adoption 为既有 hidden
  handle安装bridge，remove先摘除registry记录，释放相关 guards 后唤醒遗留waiter。
- Required changes: 提供 add/install-or-adopt/lookup/remove/snapshot 所需的最小 API；public
  TcpSocket/UdpSocket 保存registry中的同一Arc；registry只包含public IP sockets；任何
  registry lock内不得调用 waker。
- Preserve: `SocketHandle` 身份、TCP bound metadata、hidden listener ownership、axpoll 64
  容量和 wake-on-replacement；不修改 axpoll 或 smoltcp。
- Forbidden: 全局共享一个 PollSet、Weak-only lifecycle导致 public socket存活时bridge消失、
  hidden socket提前进入registry、持registry guard wake、扩大到 Unix/vsock。
- Test witness: RED tests覆盖 create/lookup identity、accepted-handle adoption、remove/drop wake、
  handle复用不继承旧bridge、1/2/64/65不同waker和重复register；旧 Event 无 consumer 的
  source witness在替换前为 RED。
- GREEN condition: registry/Arc生命周期与所有容量边界通过；remove后旧waiter获重检机会；
  全 crate 若无 Event 使用则依赖清理完成，无新增依赖。
- Verification: targeted readiness/wrapper tests；ordinary 与 qemu-diagnostics axnet lib tests；
  `rg` 检查 Event/registry引用和wake不在guard内。
- Stop when: 必须修改 axpoll 容量/overflow、smoltcp handle语义、Unix/vsock或建立全局共享
  PollSet 才能实现。

### 2.2: 桥接 TCP/UDP smoltcp 单槽到 application multi-waiter

- Requirement/Scenario: R5；同 socket 多 waiter、方向隔离、spurious wake、注册竞态。
- Depends on: Task 2.1 GREEN。
- Targets: `readiness.rs` direction waker/helper；`tcp.rs::Pollable::register`；
  `udp.rs::Pollable::register`；必要的 test-only local SocketSet seam。
- Current behavior: IN/OUT/RDHUP 都注册到最后一个全局 Service/device waker；smoltcp
  recv/send waker API尚未使用。
- Required behavior: application waker按 interest 登记到 socket-local set；在一次短
  SocketSet guard内把 read/write bridge waker分别注册到 smoltcp recv/send slot并取得
  readiness recheck；解锁后只对已ready方向执行wake。terminal waiter登记不能使普通
  read/write waiter丢失。
- Required changes: 复用 smoltcp one-shot语义，每次 application register和每次 wake后的
  retry均可重臂；TCP stream与UDP用同一 register-order contract，listener在Task 2.3接管。
- Preserve: `poll_io` 和 kernel poll/select/epoll 的外层 check-register-recheck、spurious
  wake语义、nonblocking立即 WouldBlock、TCP short write与UDP原子性。
- Forbidden: 把 wake 当 readiness成功、在 smoltcp 中保存多个waker、持 SocketSet/bridge
  lock唤醒 application、用周期poll弥补漏唤醒。
- Test witness: fake/local smoltcp socket + counting waker RED cases覆盖 read/write不同interest、
  1/2/64/65 waiter、ready-before-register、ready-during-register、spurious wake和wake后重臂。
- GREEN condition: 一个 smoltcp transition使全部已登记同方向 waiter获得重检机会；另一
  方向不被静默覆盖；最终 I/O/snapshot仍决定成功。
- Verification: targeted TCP/UDP bridge tests在ordinary与qemu-diagnostics下通过；两组
  axnet lib suites全量通过。
- Stop when: 需要修改 smoltcp waker storage、扩大 PollSet 或绕过 socket-local bridge。

### 2.3: 建立 listener accept bridge 与 hidden socket rearm

- Requirement/Scenario: R6；listener Ready/Reset、multiple accept waiter、backlog refill与cleanup。
- Depends on: Tasks 2.1–2.2 GREEN。
- Targets: `listen_table.rs::{ListenTableEntryInner,listen,reconcile,accept,unlisten}`；
  `tcp.rs` listen/poll/register/accept paths。
- Current behavior: public listener只扫描 queue；idle/pending hidden smoltcp socket没有指向
  public waiter的waker；reconcile状态变化不会精确唤醒accept。
- Required behavior: public listener把自己的accept set交给entry；idle/pending hidden sockets
  在create/refill/reconcile/register后注册同一accept bridge waker。Pending→Ready/Reset先提交
  queue状态，再在全部 Service/SocketSet/entry guards释放后wake；Ready仅交付一次，Reset使
  accept返回`ConnectionReset`，并让waiter获得`IN`重检机会。
- Required changes: listener register执行 application set register → SocketSet/entry有序
  register → readiness recheck → post-unlock wake；accept消费、full→accept→refill、unlisten
  和drop都重新发布必要 software/cleanup wake。
- Preserve: `LISTEN_QUEUE_SIZE=512`、一个idle补位、Pending/Ready/Reset、唯一accept winner、
  hidden handle直到accept才转为public registry。
- Forbidden: 给public listener自身的非接收smoltcp socket注册后冒充hidden witness、runner
  quiet时周期扫描、改变backlog容量、持entry guard wake。
- Test witness: RED tests覆盖 hidden ready/reset、2个及64/65 accept waiter、full→accept→refill、
  并发唯一winner、register race、unlisten/drop cleanup与handle泄漏。
- GREEN condition: hidden transition精确唤醒全部accept waiter；一个winner取得连接，其余
  返回/retry `WouldBlock`；Reset与cleanup不永久Pending。
- Verification: listener targeted tests、existing listener compatibility tests、ordinary与
  qemu-diagnostics axnet suites。
- Stop when: 必须改变512 backlog、周期轮询listener、增加public-to-hidden全局扫描或引入
  新accept错误契约才能完成。

### 2.4: 原子切换到 runner-owned progress 并固定全局锁序

- Requirement/Scenario: R1、R2、R4；socket无主动poll、software wake、TCP connect锁序。
- Depends on: Tasks 2.1–2.3 GREEN。
- Targets: `lib.rs`有序 Service/SocketSet helper和compat helper；
  `service.rs::register_waker/timeout`；`general.rs::register_waker`；`wrapper.rs`；
  TCP/UDP/listener全部mutation与12个产品 `poll_interfaces()` 调用。
- Current behavior: TCP 8处、UDP 4处主动poll；Service每waiter替换timeout并注册旧global stack
  waker；connect持SocketSet后反向取得Service；mutation没有统一post-commit event。
- Required behavior: 所有跨对象路径遵循 `SERVICE → SOCKET_SET → ListenTable entry`；TCP
  connect在一个有序同步临界区取得route/context并提交。产品TCP/UDP/listener调用点为0；
  runner独占timer。需要协议推进的mutation在commit和解锁后发布一次可合并的
  `StackEvent::publish_software`。
- Required changes: 清点并覆盖 socket add/bind/connect/listen、成功send enqueue、非PEEK
  recv dequeue、accept/refill、shutdown/remove和影响smoltcp进展/timer的option mutation；
  删除 Service timeout/register_waker 与 General旧入口。纯query、失败前置检查、PEEK和
  只改本地无协议影响的flag不得无条件wake。
- Preserve: public `poll_interfaces()`兼容/test helper可保留；legacy socket错误/短I/O、
  runner唯一性、queue owner、slot/ticket/flush、D1/QEMU feature组合和Unix/vsock行为。
- Forbidden: message-passing重写、`try_lock` self-wake掩盖反向锁、在guard内wake、恢复
  caller-driven poll作为fallback、第二runner或第二timer owner。
- Test witness: source RED assertions精确列出12个产品调用和connect反向边；mutation source/
  behavior tests覆盖commit-before-publish、失败不publish、read-only quiet、pre-Service安全；
  100× runner/socket/connect/listener竞争证明无死锁且有进展。
- GREEN condition: `rg` 只剩compat helper定义和测试字符串；Service无socket timeout/global
  register入口；全部必要mutation有post-unlock event，100×竞争无hang/lost wake。
- Verification: targeted source/concurrency tests；ordinary与qemu-diagnostics suites；kernel
  QEMU check、root D1 target check；fmt和diff review。
- Stop when: 需要message passing、修改外部socket ABI、恢复inline poll、让guard跨
  `Pending`/wake或引入第二owner才能通过。

### 2.5: 统一普通 TCP/UDP readiness snapshot 与下一 I/O

- Requirement/Scenario: R6与network-stack-baseline；TCP data/EOF/writable/HUP、UDP datagram
  IN/OUT/close、poll→I/O一致性。
- Depends on: Task 2.4 GREEN。
- Targets: `tcp.rs::{poll_connect,poll_stream,Pollable,send,recv}`；
  `udp.rs::{Pollable,send,recv,shutdown}`；`readiness.rs` test helpers。
- Current behavior: TCP用`!may_send || can_send`报告OUT，closed/idle难区分，RDHUP只看local
  flag；UDP closed返回空events。poll与I/O各自计算条件。
- Required behavior: Idle TCP无普通 readiness；connected buffered data=`IN`；peer EOF且
  buffer空=`IN|RDHUP`；local read shutdown=`RDHUP`并保留现有read错误；`can_send`才=`OUT`；
  双向终止=`HUP`并保留应有EOF bits。UDP open的完整datagram能力映射IN/OUT，closed=`HUP`。
  connect成功=`OUT`；connect failure的稳定`ERR`与listener reset `ERR`留给Task 3.1。
- Required changes: 让poll与紧随I/O共用同一短guard snapshot/predicate；register末尾用同一
  snapshot recheck；spurious wake只触发重检。记录并发consumer winner后I/O回到WouldBlock
  为允许race。
- Preserve: TCP buffered-data-before-EOF、short write、PEEK、local shutdown既有错误；UDP
  datagram原子发送/接收与TRUNCATE/PEEK；nonblocking错误类别。
- Forbidden: `!may_send`伪报OUT、把正常EOF当ERR、为SO_ERROR建立新消费契约、把wake当成功、
  改变UDP为字节流。
- Test witness: 每个状态的 poll→immediate I/O RED matrix；TCP data/peer EOF/local shutdown/
  writable/full/HUP、UDP readable/writable/closed；1/2/64/65 waiter和并发winner例外。
- GREEN condition: readiness与下一I/O结果一致或只出现明确并发winner；MS01 TCP/UDP/
  listener/nonblocking/poll场景无回归。
- Verification: targeted snapshot/I/O tests、ordinary与qemu-diagnostics suites、MS01 payload
  cross-compile与single-hart QEMU 14/14 markers。
- Stop when: 正常EOF必须映射ERR、现有应用依赖closed-as-OUT、SO_ERROR消费成为必要条件，
  或需要Task 3.1 stable fault语义才能满足普通路径。

**Invariants**

- resident stack runner仍是唯一smoltcp推进者；queue task仍是唯一hardware descriptor、token、
  reclaim和queue-control owner。
- StackEvent只承载提示；readiness和I/O在SocketSet/ListenTable当前状态上重检。
- 所有Service、SocketSet、ListenTable entry和readiness registry guard在wake、await、
  `Pending`、timer arm或task yield前释放。
- 每public TCP/UDP handle独占自己的64-capacity PollSet组；不同socket不共享容量。
- hidden listener sockets不进入public registry；accept adoption后只安装一次public bridge。
- listener backlog保持512；TCP short write和UDP datagram原子性不变。
- stable data-plane fault、最终ERR映射、MS06 guest probe和QEMU最终结论留给Iteration 002。
- QEMU single-hart结果不声明SMP、真板、DMA/cache或性能资格。
- 用户并发修改、SNAPSHOT、M/D/K/R/I、Runbook、archive和commit不属于本Cycle。

**Non-goals**

- Task 3.1 的device-wide fault registry广播、稳定DevError→AxError和connect/listener最终ERR。
- Tasks 3.2–3.4 的MS06 guest probe、最终自动Gate与完整QEMU acceptance。
- reset/cancel/link flap、SMP、multiqueue、多接口、PCI/DWMAC、真板和性能。
- 修改axpoll或smoltcp、扩容PollSet、引入新executor/依赖或改变Unix/vsock readiness。
- 全局tasks/SNAPSHOT/M/D/K/R/I维护、Evidence目录、Runbook/Incident、归档或提交。

**Requirements Traceability Matrix**

| Requirement | Scenario | Design | Task | Code Surface | Test Witness | Status |
|---|---|---|---|---|---|---|
| R1 caller-independent progress | socket mutation无主动poll | D1,D9 | 2.4 | TCP/UDP/listener、`lib.rs` | product-call source guard、software progress | Covered |
| R2 software wake | commit→unlock→publish | D2,D9 | 2.4 | mutation paths、StackEvent | ordering/quiet/race tests | Covered |
| R4 lock序与guard | TCP connect、runner竞争 | D5,D9 | 2.4 | Service/SocketSet/listener helpers | source guard、100×竞争 | Covered |
| R5 per-socket bridge | multiwaiter、64/65、register race | D6 | 2.1,2.2 | `readiness.rs`、wrapper、TCP/UDP | lifecycle、1/2/64/65、rearm | Covered |
| R6 listener普通readiness | hidden Ready/Reset、cleanup | D7 | 2.3 | ListenTable、TCP listener | accept race、reset、refill | Covered |
| R6 TCP/UDP普通映射 | data、EOF、writable、UDP close | D8 | 2.5 | TCP/UDP snapshots与I/O | poll→I/O matrix | Covered |
| network-stack-baseline | multiwaiter与I/O一致 | D6–D8 | 2.1–2.5 | public IP sockets | host matrix、MS01 14 markers | Covered |
| MS05 owner保持 | stack consumer切换 | D2,D5 | 2.4 | runner/socket mutation边界 | MS05 regressions、source guard | Covered |

没有 Missing 或未批准 Simplified。Task 3.1 的stable fault/最终ERR是既有Iteration 002
范围，不是本Cycle需求缺口。

**Acceptance**

1. Task 2.1 / R5：每public handle的read/write/terminal bridge与registry生命周期闭合；
   1/2/64/65、remove/drop、adoption和handle复用测试通过。
2. Task 2.2 / R5：TCP/UDP smoltcp recv/send单槽扇出全部application waiter；
   ready-before/during-register、spurious wake和rearm无lost wake。
3. Task 2.3 / R6：hidden listener Ready/Reset唤醒public accept bridge；唯一accept、512 backlog、
   full→accept→refill、unlisten/drop和多waiter无泄漏或永久Pending。
4. Task 2.4 / R1,R2,R4：产品TCP/UDP/listener `poll_interfaces()`调用为0；runner独占timer；
   mutation commit后且解锁后发布software wake；全局锁序与100×竞争通过。
5. Task 2.5 / R6：普通TCP data/EOF/RDHUP/OUT/HUP与UDP IN/OUT/HUP和紧随I/O一致，只有
   已记录并发winner例外；正常EOF不误报ERR。
6. ordinary/qemu-diagnostics axnet suites、kernel QEMU check、root D1 check、MS04/MS05
   受影响host regressions、fmt/source/strict OpenSpec/diff review全部通过。
7. MS01 payload在当前single-hart QEMU VirtIO-MMIO上14/14 markers PASS；该结果只证明
   socket兼容，不替代Iteration 002的MS06 multiwaiter/quiet/fault最终QEMU acceptance。

**Verification**

- Targeted readiness/wrapper/TCP/UDP/listener tests，包含每项RED witness名称与GREEN结果。
- `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib`
- 同命令增加 `--features qemu-diagnostics`
- lost-wakeup、register race、runner/socket lock竞争 targeted cases在ordinary与
  qemu-diagnostics下各重复100次。
- `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test`
- `cargo check --locked --offline -p starry-kernel --features qemu`
- `cargo check --locked --offline --target riscv64gc-unknown-none-elf --features lichee-d1`
- `riscv64-linux-musl-gcc -static -O2 -o tests/ms01_socket_baseline tests/ms01_socket_baseline.c`
- 自动Gate全部通过后，按 `.claude/runbooks/qemu-network-testing.md` 在single-hart
  VirtIO-MMIO QEMU guest手工运行MS01；要求START/END、14个PASS、0个FAIL、环境、命令和
  明确退出结果。agent sandbox无法可靠驱动guest shell时，按能力边界写Blocker Handoff，
  等待用户提交同一revision的新鲜结果；缺marker或中断不计PASS。
- `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check`
- source assertions：产品 `poll_interfaces()`调用为0；connect closure无反向
  `get_service()`；Service socket timeout/register入口消失；wake不在network/registry
  guards内；smoltcp与axpoll未修改；hidden socket不在public registry。
- `openspec validate ms06-application-visible-async-network-stack --strict`
- `git diff --check` 与完整diff review；排除用户并发文档修改的归属。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | 实际入口、12个产品poll调用、single-slot waker、PollSet 64/65、listener hidden模型、锁序和syscall重检已定位 |
| Design | PASS | per-handle bridge、listener桥接、ordered helper、post-commit wake与普通snapshot语义已由D5–D9闭合 |
| Iteration Plan | PASS | Tasks 2.1–2.5共同形成原子readiness cutover；stable fault/QEMU最终验收保持在Iteration 002 |
| Cycle Scope | PASS | 只展开既有Iteration 001；不增加全局task、不修改Iteration Map |
| Task Contracts | PASS | 每项包含目标、当前/目标行为、RED/GREEN、保持/禁止、验证和停止条件 |
| Traceability | PASS | R1/R2/R4/R5/R6及两个modified baseline均映射到代码和测试；无Missing/Simplified |
| Verification | PASS | host/unit、100×并发、QEMU/D1 compile、MS01 manual runtime和source/diff Gate分层 |
| User Approval | PASS | 用户于 2026-08-23 明确回复“批准” |

**Persisted Evidence**

- Mode: none

host/unit/compile/source结果可低成本重跑；MS01手工QEMU结果可由Act Response记录命令、
环境、START/END、14个marker和明确结果。当前没有一次性环境或无法摘要的决定性结构，
不创建Evidence目录。若执行时证明确需持久化，Act必须停止返回Plan，不得自行改变模式。

**Risks and Notes**

- PollSet第65项会主动唤醒被替换者；测试必须把该wake解释为recheck提示，不能要求65个
  waker同时驻留或把spurious wake判为ready。
- listener transition发生在runner持有Service/SocketSet期间；任何post-reconcile发布机制
  都必须证明application wake在这些guards释放后，不能用“PollSet内部先解锁”替代外层证明。
- `StateLock::Busy`不是Service/SocketSet锁序的一部分，但Task 2.4必须避免在Busy状态下等待
  反向network guard造成活锁或错误状态遗留。
- successful recv dequeue会释放协议buffer并可能要求window/ACK进展；非PEEK路径需要
  software wake，PEEK与纯query保持quiet。
- stable fault、SO_ERROR消费和listener reset的最终ERR bit不在本Cycle；若普通readiness
  无法在不引入这些语义时成立，停止并返回Plan判定`replan-required`。
- MS01 QEMU是用户手工能力边界。Act应先完成全部自动Gate，再以同revision交接；历史
  14/14不能替代本Cycle结果。

## Act Response

- Status: blocked

**Implemented**

- Task 2.1：新建 `readiness.rs` 的 `ReadinessBridge`（read/write/terminal 三组 `PollSet`，`DirectionNotify` 让一个 smoltcp 单槽 wake 同时到达方向与 terminal waiter），`SocketSetWrapper` 增加 per-public-handle registry（`add_public` 原子安装 handle+bridge、`install_readiness` 供 accept adoption、`take_readiness` 在 registry guard 释放后唤醒遗留 waiter），`TcpSocket`/`UdpSocket` 构造函数持有 registry 中同一 Arc；删除无 consumer 的 `event_listener` 直接依赖。
- Task 2.2：`OneShotSocket` trait + `ReadinessBridge::rearm` 把 smoltcp one-shot recv/send 单槽指向 bridge；TCP/UDP `register` 统一为「application register → 短 SocketSet guard 内 rearm+recheck → 解锁后只 wake 已 ready 方向」，RDHUP/HUP/ERR 搭乘 recv 槽且独立 terminal 组不挤占 read/write 容量。
- Task 2.3：`ListenTableEntryInner` 持有 public listener 的 accept bridge；hidden socket 在 refill 与每次 reconcile 后 rearm recv 槽；`reconcile` 提交 Pending→Ready/Reset 并把发生 transition 的 port 记入 `pending_accept_wakes`，由 stack runner 在轮次（Service/SocketSet guards）释放后 `drain_accept_wakes` 唤醒；accept 成功/Reset 消费、unlisten/drop、register 的 can_accept recheck 全部 post-lock 唤醒。
- Task 2.4：产品 TCP/UDP/listener 的 12 处 `poll_interfaces()` 调用归零（仅保留 `lib.rs` 公共 compat helper）；mutation 在 commit 且解锁后发布一次可合并 `publish_software_work()`（bind/connect/listen/accept/send/non-PEEK recv/shutdown/Drop、UDP connect/bind/shutdown；PEEK 与纯查询/poll 保持 quiet；失败不 publish）；删除 `Service::register_waker`/`timeout` 与 `GeneralOptions::register_waker`/`device_mask`（及 `Service::device_mask_for`）；修复 TCP `connect` 反向锁（Service guard 先于 SocketSet，`iface.context` 与 route 在 guard 内取得）；清除被覆盖注册路径残留的 `QueueEvent::register_stack`/`stack_waker` 孤儿（stack 角色完全归 `StackEvent`）。
- Task 2.5：统一 `tcp_readiness`/`udp_readiness` snapshot（axnet-Idle 无普通 readiness、buffered data=IN、peer EOF 空 buffer=IN|RDHUP、`can_send` 才 OUT、双向终止=HUP、本地 shutdown=RDHUP；UDP open 完整 datagram IN/OUT、closed=HUP），`poll` 与 `register` 末尾 recheck 复用同一 predicate；spurious wake 只触发重检。

**Changed Files and Symbols**

- `crates/axnet/src/readiness.rs`（新）：`ReadinessBridge`、`OneShotSocket`、`DirectionNotify`、`SLOT_READ_INTEREST`、`rearm`。
- `crates/axnet/src/wrapper.rs`：`SocketSetWrapper` registry（`add_public`/`install_readiness`/`lookup_readiness`/`take_readiness`），`remove` 先摘 bridge 再移除 smoltcp handle；删除 `new_socket` Event。
- `crates/axnet/src/tcp.rs`：`tcp_readiness`、`poll_stream`/`poll`/`register` 复用 snapshot、`new_connected` adoption（不再取 Service）、`connect` 锁序修复 + publish、bind/listen/send/recv/shutdown/Drop publish 化、OneShotSocket impl。
- `crates/axnet/src/udp.rs`：`udp_readiness`、`poll`/`register` 复用、bind/connect/send/recv/shutdown publish、OneShotSocket impl。
- `crates/axnet/src/listen_table.rs`：entry accept bridge、hidden rearm（refill + reconcile）、`reconcile` transition staging、`drain_accept_wakes`、`listen(_, accept)` 签名。
- `crates/axnet/src/stack_runner.rs`：runner 轮次后 `LISTEN_TABLE.drain_accept_wakes`；`publish_software_work` 移除 allow(dead_code)。
- `crates/axnet/src/service.rs`：删除 `register_waker`/`timeout`/`select_wake_deadline`/`any_masked_device_requires_polling`/`device_mask_for` 与对应纯函数测试。
- `crates/axnet/src/general.rs`：删除 `register_waker`/`device_mask`/`set_device_mask`。
- `crates/axnet/src/async_rx.rs`：删除 `QueueEvent::stack_waker`/`register_stack`，重写 5 个 stack-role 测试到 `StackEvent` 观察。
- `crates/axnet/src/lib.rs`：`mod readiness`、compat `poll_interfaces` 末尾 drain。
- `crates/axnet/Cargo.toml` + `Cargo.lock`：移除 `event-listener` 依赖。

**Deviations from Plan**

- `QueueEvent` 的 stack 角色（`register_stack`/`stack_waker`）在 Task 2.4 删除 `Service::register_waker` 后成为不可达孤儿；按「清理本次改动产生的孤儿」移除并迁移 5 个 T3.1/T3.5/T3.7 测试到 `StackEvent` 观察（`rx_copy_publishes_the_independent_stack_event` 已有等价覆盖）。
- tcp `accept` 的 ConnectionReset 分支同样消耗 backlog slot，因此在错误路径也发布 software wake（属于「已提交 mutation 必须发布」而非「失败不 publish」）。
- Task 2.1 的 `repeated_register` 期望值修正：axpoll `PollSet::register` 对同一 waker 重复注册不合并，每次注册是独立 one-shot recheck 提示（2 次注册 → 2 次 recheck wake）。
- 工作区未 commit；产品实现与 `b8e7bcae` 后产生的 git commit 保持独立，Act 不提交。

**Blocker Handoff**

- Task/step/Gate：Acceptance 7 —— single-hart QEMU VirtIO-MMIO 上手工运行 MS01 payload（14 个 PASS marker），按 `.claude/runbooks/qemu-network-testing.md`。
- Plan 预期：MS01 14/14 markers PASS，作为 socket 兼容回归证据。
- 实际（两次手工运行，2026-08-24 01:08 前/01:14）：用户按 runbook 手工执行，两次均在 `MS01_SOCKET_BASELINE_START` 后停滞（无任何 PASS/FAIL 行、无 END），并伴随一次 `starry_kernel::pseudofs::dev::tty::terminal::ldisc` 的 `Failed to send signal: AxErrorKind::NoSuchProcess` 日志。这属于**产品 Gate 失败**（非环境阻塞）。
- 决定性差分（第二次运行）：guest 内 `wget -q -O /tmp/ms01_test http://10.0.2.2:18765/ms01_socket_baseline` **成功**（host HTTP server 记录 `GET /ms01_socket_baseline 200`）——真实 NIC（VirtIO）的 RX/TX、runner 事件唤醒、TCP connect/send/recv 与 blocking I/O 链路在 guest 内均工作；而 payload 首测（loopback 127.0.0.1:18002，`fork` 出 client 进程 + parent server 阻塞 accept）停滞。**问题收敛到 loopback 路径**。
- 已保留的中间修复（本轮实施，带 host witness，需下一 Cycle 在 guest 验证）：(a) `StackRunnerFuture` 在 `rx_ready`/`socket_changed` 时自唤醒续跑，镜像 `Service::poll` drain 语义（新增 `progress_wake` 遥测与 `StackSnapshot` 字段，回归测试 `loopback_tx_making_rx_ready_self_wakes_to_drain`）；(b) `ListenTable::reconcile` 总是重臂存活 hidden socket 的 recv 槽（不再被 idle-LISTEN 提前返回跳过）；(c) `tcp_readiness(Connecting)` 静默（connecting 仅由 `poll_connect` 报 OUT）。自动套件 271/271 + qemu-diagnostics 291/291 通过，artifact 已重建。
- 排除项：`Router::poll_bounded` 以 round-robin cursor 服务全部设备（loopback 未被 async owner 跳过）；runner 自唤醒机制在 guest 的 queue task（MS04/MS05 已运行）中被证实可用。
- 下一 Cycle 起点（按用户指示延后到下一轮解决）：
  1. 建立 host 全链 loopback 见证（local `Service`+`SocketSet`+loopback device，连续 `stack_round` + `drain_accept_wakes` 模拟 runner）驱动 smoltcp connect/accept/send/recv 握手；若 host 复现 → 直接在 axnet 侧修；若 host 通过 → 聚焦 guest 侧。
  2. 若 host 通过，guest 侧候选：`axtask::future::block_on` 对自唤醒（`cx.waker().wake_by_ref()` + `Pending`）的重调度语义（loopback 链完全依赖自唤醒续跑）；单 hart 下 `fork` 后 client/server 双用户进程与 runner/queue task 的调度交错；`bind`/`listen` 具体端口的注册路径。
  3. 必要时按 runbook「直接挂载注入」在 guest 内跑 `ms04_rx_probe`/`ms05_data_plane_probe` 的 loopback 场景，或输出 `stack_snapshot()` 遥测以区分「round 未发生」与「round 发生但帧未被 ingest」。
  4. 确认 `ldisc NoSuchProcess` 是否为 fork/进程组相关的持续性噪声（两次运行均出现，可能是独立于网络停滞的表象）。
- 影响：Tasks 2.1-2.5 实现完成且自动 Gate 通过；MS01 runtime 是本 Cycle 唯一未闭合 Acceptance，且按用户决定延至下一 Cycle 修复后再验。缺 marker 不能计 PASS，历史 14/14 不能替代本 Cycle 结果。
- 已完成的自动 Gate：axnet ordinary 271/271、qemu-diagnostics 291/291、100× lock-competition、MS04 host harness 16/16、kernel QEMU check、root D1 target check、MS01 payload 交叉编译、`make build`（QEMU artifact）exit 0、fmt/strict OpenSpec/diff check。
- 恢复条件：下一 Cycle 修复 loopback 停滞根因（见「下一 Cycle 起点」）后，用户重新手工运行 MS01 并回传 14/14 marker；本 Response 才能转 `pending → reported`。当前按用户指示保持 `blocked`，不进入 openspec-plan。
- Evidence：None required（结果可低成本重跑，见 runbook「证据精简原则」；MS01 运行结果由用户回传命令输出）。

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: PASS（Tasks 2.1-2.5 已实现，契约内 Deviations 已记录；MS01 runtime Gate 暴露 loopback 路径产品缺陷，根因修复按用户指示延至下一 Cycle）
- Full diff reviewed: PASS
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 0

完整 diff（10 个源码/Cargo 文件 + 新 readiness.rs，约 +1500/-400）已逐文件审查：锁序（Service→SocketSet→entry）、guard 内不 wake、hidden socket 不进 public registry、accepted handle 安装一次 bridge、mutation commit→unlock→publish、PEEK/query/poll quiet、无新增产品警告（仅 smoltcp 与 diag-gated 既有警告）；Runbook QEMU 政策与用户并发文档未修改、未归因。

**Verification Evidence**

| 验证项 | 命令 | 输出摘录 | 结论 |
|---|---|---|---|
| axnet ordinary | `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib` | `test result: ok. 269 passed` | PASS |
| axnet qemu-diagnostics | 同上加 `--features qemu-diagnostics` | `test result: ok. 289 passed` | PASS |
| lock-competition ×100 | `--quiet runner_connect_listener_lock_orders_compose_without_deadlock` ×100 | 100/100 `test result: ok` | PASS |
| MS04 host harness | `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test && ...` | `16 passed` exit 0 | PASS |
| kernel QEMU check | `cargo check --locked --offline -p starry-kernel --features qemu` | `Finished dev profile` exit 0 | PASS |
| root D1 check | `cargo check --locked --offline --target riscv64gc-unknown-none-elf --features lichee-d1` | `Finished dev profile` exit 0 | PASS |
| MS01 payload 交叉编译 | `riscv64-linux-musl-gcc -static -O2 -o tests/ms01_socket_baseline tests/ms01_socket_baseline.c` | exit 0 | PASS |
| QEMU artifact | `make build` | `Finished release profile`，objcopy 生成 `StarryOS_riscv64-qemu-virt.bin` exit 0 | PASS |
| source assertions | `stack_runner::tests::`（12 调点归零、connect 锁序、Service 无 register_waker、publish gating、poll quiet） | 全部 ok | PASS |
| fmt | `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check` | 0 diffs | PASS |
| OpenSpec validate | `openspec validate ms06-application-visible-async-network-stack --strict` | `Change ... is valid` | PASS |
| git diff | `git diff --check` | exit 0 | PASS |
| MS01 single-hart QEMU runtime | 手工（runbook 硬性政策），用户两次执行 | `MS01_SOCKET_BASELINE_START` 后停滞，无 marker/END；`wget` 200 成功（NIC 路径正常）；判定为 loopback 路径产品缺陷 | FAIL（loopback；修复延至下一 Cycle） |

**Persisted Evidence**

None required.

全部自动结果可由确定性 host/unit/compile/build 命令低成本重跑；MS01 手工运行结果由用户回传命令、环境、START/END 与 marker。无一次性环境或无法摘要的结构，不创建 Evidence 目录。

**Experience Candidates**

None.

本 Cycle 的可复现失败均属测试期望修正或一次性机制理解；smoltcp 初始状态与 axpoll 重复注册语义作为测试见证固化在套件中，不构成 Runbook/Incident 候选。

**Remaining Issues**

- MS01 single-hart QEMU runtime marker（Acceptance 7）两次手工运行均失败：payload 在 `MS01_SOCKET_BASELINE_START` 后于 loopback（127.0.0.1）首测停滞；`wget`（真实 NIC 路径）正常。按用户指示，loopback 停滞根因修复延至下一 Cycle 解决（起点见 Blocker Handoff「下一 Cycle 起点」）；在此之前本 Cycle 保持 `blocked`。
- 已保留的中间修复（runner `rx_ready/socket_changed` 自唤醒、reconcile 总重臂、Connecting 静默）已随本 Cycle 代码保留并重建 artifact，但 guest 第二次运行仍停滞，需在下一 Cycle 判定其是否生效/是否足够。
- Tasks 3.1-3.4 属 Iteration 002，未开始。

**Commit or Diff Reference**

- Working-tree diff at `fb87c8d3`（含 MS06 前两次 commit 之上的本次产品修改；未创建新 commit）。

## Plan Review

- Status: reviewed

**Review Result**

rework-required

**Findings**

独立检查当前产品 diff、Act Response、Blocker Handoff、实际调用链和 fresh 自动验证后，
Tasks 2.1–2.5 的 host、compile 和所有权 Gate 通过，但 Acceptance 7 尚未满足。当前目标、
范围、依赖和验收边界不需要变化；loopback 缺口必须留在 Iteration 001 内返工。

1. **阻塞 — QEMU loopback 兼容性仍失败。** 用户在同一 single-hart VirtIO-MMIO
   环境中两次运行 MS01，均只出现 `MS01_SOCKET_BASELINE_START`，没有 PASS/FAIL/END。
   第二次运行前后的 `wget` 经 VirtIO NIC 成功，host HTTP server 收到 200，因此失败不能
   归为 QEMU 环境、rootfs 或 NIC 数据面阻塞。Acceptance 7 要求 14/14 markers，本项直接
   阻塞当前 Iteration。
2. **阻塞 — 原 Plan 缺少完整 loopback 链的 host 见证和有限时阶段定位。** 当前
   `loopback_tx_making_rx_ready_self_wakes_to_drain` 只证明 raw loopback frame 触发 runner
   续跑；listener tests 分别证明 hidden socket rearm、reconcile 和 accept bridge。没有一个
   test 穿过 client SYN → Router dispatch → loopback RX → smoltcp handshake → hidden
   listener → accept bridge → application recheck 的完整路径。MS01 首例也只有用例外层 marker，
   blocking connect/accept/recv 没有阶段 marker 或固定失败边界，现有日志无法区分 client、
   server、runner、listener 与进程调度层。
3. **PASS — 自动行为和编译基线可重复。** Review 新鲜运行 ordinary axnet 271/271、
   qemu-diagnostics 291/291 和 QEMU kernel check，均 exit 0。代码检查确认 loopback 路由选择
   `127.0.0.0/8`，`dispatch_bounded` 报告 `rx_ready`，runner 对 `rx_ready/socket_changed`
   self-wake，smoltcp TCP `set_state` 同时唤醒 recv/send slots，listener transition 在网络
   guards 释放后 drain accept wakes。
4. **排除 — 现有源码不支持直接归因给 `axtask::future::block_on`。** 锁定依赖
   `axtask 0.3.0-preview.2` 的 `AxWaker::wake_by_ref` 先记录 `woke=true` 并 unblock；
   `block_on` 在 future 返回 `Pending` 后观察该位并执行 `yield_now()`。没有新的 runtime
   反证前，不得修改 scheduler 或用固定 polling tick掩盖问题。
5. **非阻塞 Minor — 清理和说明未完全同步。** `async_rx.rs` 的 `QueueEvent` 顶层说明仍称
   有两个 AtomicWaker；fresh axnet tests 还报告本 Cycle 新增的 test-only unused imports，
   QEMU check 报告 caller-driven cutover 后未再使用的 `Device::register_waker`。这些不导致
   Acceptance 失败，也不单独创建 repair item。

**Deviation Classification**

NEW-EVIDENCE（两次 QEMU loopback 产品失败）与 PLAN-OMISSION（缺少完整 host 链和
phase-resolved finite-deadline witness）。第 5 项是非阻塞 ACT-DEVIATION。

**Acceptance Gaps**

Acceptance 7：当前 single-hart QEMU VirtIO-MMIO 上 MS01 仍为 0/14 可判定 PASS，缺少 END
和退出结果。Tasks 2.1–2.5 的代码勾选不替代该 runtime Gate。

**Convergence**

N/A（initial Cycle）。同一 Cycle 内两次手工运行均停在外层 START，第二次 `wget` 200 只把
故障域从 NIC 缩小到 loopback/stack/socket/task 路径，没有关闭 Acceptance。

**Evidence**

- `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib` → exit 0，
  271 passed。
- 同命令增加 `--features qemu-diagnostics` → exit 0，291 passed。
- `cargo check --locked --offline -p starry-kernel --features qemu` → exit 0。
- 实际代码：`crates/axnet/src/{stack_runner,service,router,listen_table,readiness,tcp}.rs`；
  smoltcp TCP one-shot/state wake；锁定的 axtask `future::block_on`/`AxWaker`。
- Blocker Handoff 中用户两次 QEMU 输出：START 后无 case marker/END；第二次 guest `wget`
  成功且 host 记录 HTTP 200。
- Persisted Evidence 为 `none`；缺少 Evidence 目录不是 finding。

**Follow-up Decision**

在同一 Iteration 创建 Cycle 001。先补完整 loopback TCP/listener host witness，再用固定期限、
phase marker 的 guest diagnostic 区分 single-process socket path 与 fork/task path；只有 RED
定位到原 Tasks 2.3–2.5 的 axnet 责任面时才修改产品代码。最终仍须原 MS01 14/14 PASS。

**Iteration Plan Update**

None。Iteration 001 的任务、目标、依赖、稳定基线和验收边界保持不变。

**Next Cycle**

`001-rework.md`

**Next Iteration**

None；只有本 Iteration 的后继 Cycle 被接受后才能展开 Iteration 002。
