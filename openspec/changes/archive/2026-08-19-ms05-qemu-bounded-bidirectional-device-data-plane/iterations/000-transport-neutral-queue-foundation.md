# Iteration 000: Transport-Neutral Queue Foundation

## Plan Context

- Status: ready
- Round: 000
- Parent: None

**Objective**

建立后续 axnet 双向 queue service 可直接使用的底层稳定边界：公共 contract 能按方向控制
RX/TX completion notification，以 opaque cookie 表达单步 TX submit/reclaim；VirtIO adapter
在 QueueFull、oversize、submit/reclaim error 和乱序 completion 下保持 token/buffer/cookie
唯一 ownership；`RING_EVENT_IDX` 同时具备双 used-ring 控制和 wrap-safe 的 TX kick 判定。

本轮结束时不启用 MS05 slots 或双向 task。当前 MS04 RX task 与同步 TX 仍是产品运行基线，
从而把失败面限定在 driver contract、VirtIO adapter 和 VirtQueue notification。

**Background**

MS05 的 queue service 需要独立限制 TX reclaim budget，并以 ticket 等待 C4 buffer reclaim。
现有 `recycle_tx_buffers()` 一次 drain 全部 completion 且不返回 identity，上层无法判断哪个
ticket 已经达到 C4。当前 adapter 还有三项会让后续 Full→恢复账本失真：

- `QueueFull` 映射为 `BadState`，把正常压力误判为 fatal。
- `alloc_tx_buffer()` 在检查 oversize 前从 `free_tx_bufs` pop，错误 drop 后 buffer 只回到
  `NetBufPool`；该 pool 不再被运行期 allocation 使用。
- `transmit()` 在 `transmit_begin` 失败时同样不能把 buffer 恢复到 `free_tx_bufs`，且成功
  写入 `tx_buffers[token]` 前不检查旧 slot 是否为空。

MS04 已为 receive queue 增加 `used_event` suppress/arm，但公共 control 是 RX-only，send
queue 还没有对应入口。另一方面，`VirtQueue::should_notify()` 在 EVENT_IDX 模式只比较
当前 avail index 与 event，没有 old/new 窗口；普通大小比较在 `u16` wrap 时不符合 VirtIO
event arithmetic。

**Current Baseline**

- Revision: `3e181464fc76b562a5c4e7e8dd7bb27313fa8a11`，branch `net-k3`。
- Worktree: 用户已有 `CLAUDE.md` 修改；本 change 为 untracked。两者都不得被本轮覆盖。
- `NetQueueControl` 只有 `has_rx_completion`、`suppress_rx_notify`、
  `arm_rx_notify_and_check`。
- `NetDriverOps` 只有全量 `recycle_tx_buffers() -> DevResult` 与无 completion identity 的
  `transmit(NetBufPtr)`。
- `VirtIoNetDev<QS>` 初始化 `QS` RX buffers 和 `QS` `free_tx_bufs`，并以
  `tx_buffers[token]` 持有 device-owned buffer。
- `VirtIONetRaw::transmit_begin()` 返回 transport token；`poll_transmit()` peek used token，
  `transmit_complete()` pop 同一 token。
- `VirtQueue` 已有 used-event suppress/arm 和 `last_used_idx` wrap tests，但
  `should_notify()` 只看 new avail index，所有 device caller 仍调用无 old/new 参数的接口。

2026-08-12 fresh baseline：

| Command | Result | Exit |
|---|---|---:|
| `cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline` | 4 passed | 0 |
| `cargo check --manifest-path crates/axdriver_net/Cargo.toml --offline` | finished | 0 |
| `cargo check --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net` | finished | 0 |
| `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib queue::tests -- --nocapture` | 15 passed | 0 |
| `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib -- --nocapture` | 34 passed | 0 |
| `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --nocapture` | 109 passed | 0 |
| `cargo check --offline -p starry-kernel --features qemu` | finished | 0 |
| `make LOG=info build` | release image generated；tool setup的只读/禁网诊断未阻止正式 build | 0 |
| `make host-test` | Rust/C/protocol前置项通过；UDP loopback socket被 sandbox `EPERM` 拒绝 | 2，ENV-BLOCKED |

**Current-State Evidence**

- 公共入口：`crates/axdriver_net/src/lib.rs::NetQueueControl` 和 `NetDriverOps`。调用者通过
  `AxNetDevice` enum dispatch 使用接口；当前 implementor 包括 VirtIO、FXmac、ixgbe、
  axnet `FakeNic` 与 crate内 dummy/control fakes。
- VirtIO ownership：`crates/axdriver_virtio/src/net.rs::VirtIoNetDev` 的
  `free_tx_bufs`、`tx_buffers`、`alloc_tx_buffer`、`transmit`、`recycle_tx_buffers`。
  当前 state path 是 `free_tx_bufs → NetBufPtr → tx_buffers[token] → free_tx_bufs`；两个
  error path会把 buffer落到未消费的 `NetBufPool::free_list`。
- Error mapping：`crates/axdriver_virtio/src/lib.rs::as_dev_err` 明确把
  `virtio_drivers::Error::QueueFull` 映射成 `DevError::BadState`。
- Raw TX path：`crates/virtio-drivers/src/device/net/dev_raw.rs::transmit_begin` 调用
  `send_queue.add`，随后 `send_queue.should_notify`；`poll_transmit` 和
  `transmit_complete` 分离提供了单 completion seam。
- RX used-event path：同一 raw device 的 `poll_rx_completion`、`suppress_rx_notify`、
  `arm_rx_notify_and_check` 只委托 `recv_queue`。`send_queue` 已是独立 `VirtQueue`，可用同一
  queue primitive 映射 TX completion control，不需要暴露 raw field 给 axnet。
- Notification primitive：`crates/virtio-drivers/src/queue.rs::VirtQueue` 在 `add` 中以
  wrapping add推进 `avail_idx`，在 `pop_used` 中推进 `last_used_idx`。used suppression和
  rearm已使用 `used_event`，但 device kick仍由 `should_notify()` 的普通 `>=` 决定。
- Notification callers：block、console、input、net、vsock、sound和`OwningQueue` 都调用
  `should_notify()`；修正必须保持完整 crate tests，不能只让 net caller编译。
- Existing tests：`axdriver_net` 4 个 RX control tests；VirtQueue 15 个 queue tests，其中
  `add_notify_event_idx` 没有 old/new wrap crossing，used-event tests已有 suppress、arm、
  pending与 `last_used_idx` wrap覆盖。

**Relevant Code**

| File / Symbol | Current Responsibility | Iteration Use |
|---|---|---|
| `crates/axdriver_net/src/lib.rs::NetQueueControl` | RX-only completion/notify control | 演进为 direction-aware control |
| `crates/axdriver_net/src/lib.rs::NetDriverOps` | sync/nonblocking NIC buffer operations | 增加 opaque cookie单步 submit/reclaim contract |
| `crates/axdriver_net/src/{fxmac,ixgbe}.rs` | 非目标 NIC implementor | 明确 Unsupported/default migration，保持编译 |
| `crates/axdriver_virtio/src/net.rs::VirtIoNetDev` | VirtIO buffer/token adapter | 保存 token-cookie-buffer mapping和单步回收 |
| `crates/axdriver_virtio/src/lib.rs::as_dev_err` | VirtIO→DevError mapping | QueueFull改为可恢复 Again |
| `crates/virtio-drivers/src/device/net/dev_raw.rs::VirtIONetRaw` | raw receive/send VirtQueues | 暴露两方向 completion control，保留 token内部化 |
| `crates/virtio-drivers/src/queue.rs::VirtQueue` | descriptor与EVENT_IDX primitive | old/new kick公式和双 used notification基础 |
| `crates/virtio-drivers/src/queue/owning.rs::OwningQueue` | owning buffer queue wrapper | 随 should-notify契约迁移并回归 |
| `crates/axnet/src/device/tests.rs::FakeNic` | axnet driver fake | 只做接口编译迁移，不改变 axnet行为 |

**Critical Path**

当前 TX 数据流：

```text
EthernetDevice::send_to
  → AxNetDevice::recycle_tx_buffers (while-loop drain)
  → AxNetDevice::alloc_tx_buffer (pop free_tx_bufs)
  → AxNetDevice::transmit(NetBufPtr)
  → VirtIONetRaw::transmit_begin
  → VirtQueue::add
  → VirtQueue::should_notify
  → Transport::notify
```

本轮目标数据流只改变底层责任：

```text
future queue owner
  → allocate/prepare NetBuf
  → submit(NetBufPtr, opaque cookie)
  → adapter stores token → cookie → NetBuf
  → device used completion token
  → reclaim_one validates and completes matching NetBuf
  → free_tx_bufs restored
  → return opaque cookie at C4
```

`opaque cookie` 在本轮 fake tests中使用普通值；它不是 MS05 acceptance ticket的实现。本轮
只保证后续 Iteration 001/003 能把 ticket作为 cookie交给 driver，而 public contract 不知道
ticket ordering或 flush语义。

RX/TX completion notification目标：

```text
NetQueueControl(direction mask)
  → VirtIoNetDev adapter
  → VirtIONetRaw recv_queue/send_queue
  → suppress_dev_notify / arm_dev_notify_and_check / can_pop
```

ISR、axnet queue task和 lifecycle 本轮不接入该路径。

**Implementation Guidance**

建议严格按 1.1→1.2→1.3 执行，避免在同一个 RED 中混合 trait compile failure、buffer
ownership和 EVENT_IDX错误：

1. 先固定公共类型与方法的 ownership/error contract，迁移所有 implementor到可编译状态。
2. 再用 VirtIO adapter fake transport/ledger测试把 cookie、buffer和 token闭合，并修改
   QueueFull mapping；在完成前不让 axnet依赖新方法。
3. 最后修改 VirtQueue notify窗口与两条 used queue control，运行所有非-net caller回归。

局部 API 形状可以由 Act 选择，只要满足以下语义：direction可组合；completion cookie完全
opaque且不等于 transport token；reclaim一次最多一个；submit error返回时唯一 owner已明确；
arm操作在写通知状态、barrier、recheck之间是一个调用级原子契约。

`should_notify` 可以显式接收 old/new，也可以让 queue内部追踪“自上次判定后新增的 avail
window”；无论局部表达如何，tests必须证明判定实际使用 old/new wrapping interval，而不是
只看 new。

**Behavioral Change**

- Public queue control：从 RX-only 三方法变为可分别/组合控制 RX和TX completion。
- TX completion：从 caller触发的全量无 identity recycle，变为 queue owner可逐个 reclaim
  opaque cookie。
- Pressure error：真实 descriptor/buffer暂时耗尽返回 `Again`；ownership/token不变量才是
  fatal `BadState`或等效稳定错误。
- Buffer recovery：oversize和 submit error不再把 buffer移出实际可用集合。
- Readiness：对当前单连续 TX buffer，ready结果与下一次单-owner submit一致；不能继续用
  `available_desc >= 2` 表达实际只需一个 descriptor的请求。
- EVENT_IDX：两条 used queue可独立 suppress/arm；TX device kick跨 `u16` wrap正确。
- Product runtime：本轮不切换 owner，因此 Router、Ethernet、MS04 task和socket外部行为应
  保持 baseline。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| 1.1 | R7 双向contract、共享IRQ分类、DWMAC model | `axdriver_net::NetQueueControl/NetDriverOps` | RX-only control、全量无identity TX recycle | direction mask、opaque cookie、单步 submit/reclaim与defaults |
| 1.1 | R7 implementor兼容 | FXmac、ixgbe、dummy、`FakeNic` | 实现旧trait | 编译迁移；非目标双向control明确Unsupported |
| 1.2 | R3 submit/reclaim与守恒 | `axdriver_virtio::VirtIoNetDev` | free/inflight buffers、无cookie | token-cookie-buffer mapping、单completion C4 |
| 1.2 | R2/R3正常Full | `axdriver_virtio::as_dev_err` | QueueFull→BadState | QueueFull→Again，invariant error保持fatal |
| 1.2 | R3 readiness | `VirtIONetRaw::can_send`、adapter `can_transmit` | descriptor阈值与实际submit不一致 | 与下一连续buffer submit容量一致 |
| 1.3 | R11 TX kick wrap | `VirtQueue::add/should_notify`及所有caller | new-only普通比较 | old/new wrapping event公式 |
| 1.3 | R7/R11双 used queues | `VirtIONetRaw` recv/send queue controls | 仅recv queue导出 | 两方向completion/suppress/arm-and-check |

**Task Contracts**

### Task 1.1 — Direction-aware contract and opaque completion cookie

- Depends on: None。
- Current behavior: `NetQueueControl` 只能观察/控制 RX；`NetDriverOps` 的 TX completion只通过
  无返回、无界 `recycle_tx_buffers` 表达。
- Target behavior: 公共类型可组合表示 Rx/Tx，按方向返回 pending mask；submit接收 opaque
  cookie，reclaim一次返回零或一个 cookie；每种 error明确 buffer owner。
- Required RED:
  - RX/TX/双方向 fake control contract在当前代码中无法编译或无法表达。
  - fake TX ledger不能从当前 `recycle_tx_buffers()` 获得 completion identity。
  - DWMAC descriptor owner+interrupt mask fake model不能映射现有 RX-only接口。
- Required GREEN:
  - Rx、Tx、Rx|Tx 的 pending/suppress/arm-recheck结果可分别断言。
  - opaque cookie round-trip不暴露 transport token类型。
  - DWMAC fake只使用direction、completion visible、mask/arm/barrier/recheck语义。
  - FXmac、ixgbe、VirtIO、dummy、control fake与axnet `FakeNic`全部编译；未绑定方向明确
    `Unsupported`，不伪造ready。
- Must modify: `axdriver_net` contract/tests和所有因trait breaking change必须迁移的
  workspace implementor。
- Must not modify: DWMAC产品代码、axnet Router/Ethernet/task、kernel ISR、Cargo registry。
- Verification:
  - `cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline`
  - `cargo check --manifest-path crates/axdriver_net/Cargo.toml --offline --features fxmac`
    （只有依赖已在offline cache时执行；缺依赖记环境事实，不改为产品PASS）
  - `cargo check --manifest-path crates/axdriver_net/Cargo.toml --offline --features ixgbe`
    （同一规则）
  - `cargo check --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net`
  - `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib device::tests -- --nocapture`
- Pass: mandatory默认-feature tests/check全部exit 0，optional implementor若cache可用也exit 0；
  source audit无VirtIO/DWMAC token/ring/MMIO public type。
- Failure meaning: trait语义或implementor migration未闭合；不能用默认返回false掩盖支持状态。
- Stop condition: 若 direction contract必须公开transport layout，或 submit error后的 buffer
  ownership不能在接口层唯一规定，停止本轮返回 Plan。

### Task 1.2 — VirtIO TX ownership and one-completion reclaim

- Depends on: Task 1.1 GREEN。
- Current behavior: adapter在alloc后错误可丢失实际free capacity；submit不检查token slot；
  completion while-loop无cookie；QueueFull→BadState；readiness要求两个descriptor。
- Target behavior: 每个成功submit建立唯一 token-cookie-buffer record，每次reclaim至多完成一个
  record并在buffer可复用后返回cookie；所有submit前错误恢复同一free list；正常Full→Again。
- Required RED:
  - 重复 oversize 后可用buffer数量单调下降。
  - submit注入 QueueFull/error 后buffer不回到 `free_tx_bufs`。
  - 当前接口无法观测一个 completion cookie，也无法给reclaim budget计步。
  - 未知/重复token或occupied token slot没有统一fatal见证。
  - `available_desc == 1` 时 raw one-buffer request可提交但 `can_transmit`报告false。
- Required GREEN:
  - success path逐状态验证 free→prepared→device-owned→completed→free，cookie原样返回。
  - QueueFull、oversize和可注入 submit error重复至少 `2 × QS` 后，总buffer与后续capacity不变。
  - 乱序used tokens各自返回匹配cookie；unknown、duplicate、overwrite和double reclaim稳定fatal。
  - reclaim空queue返回None/Again的既定非fatal结果，不busy loop。
  - readiness与下一次相同连续buffer submit一致。
- Must modify: `axdriver_virtio` net adapter/error mapping、必要的 raw net seam和test-only fake。
- Must not modify: frame slots、ticket ordering/flush、Router、MS04 task、实际queue size、DMA
  layout或VirtIO token公开边界。
- Verification:
  - `cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net`
  - `cargo check --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net`
  - `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib -- --nocapture`
  - `cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline`
- Pass: ownership scenarios全部GREEN，命令exit 0，测试能直接观察错误前后buffer count和cookie
  mapping；无 warning-only invariant处理。
- Failure meaning: buffer/token owner仍不唯一或正常pressure仍被fatal化，后续slots/flush不能开始。
- Stop condition: 若底层completion无法在adapter内部关联cookie，或失败buffer必须交还给已无
  handle的caller才能恢复，停止本轮返回 Plan。

### Task 1.3 — Bidirectional used-event control and wrap-safe TX kick

- Depends on: Task 1.1 GREEN；在Task 1.2后执行，避免同时编辑 adapter同一surface。
- Current behavior: recv queue有used suppress/arm，send queue没有transport-neutral入口；
  `should_notify`用new-only普通比较，11类device caller共用它。
- Target behavior: recv/send queue都能独立执行pending/suppress/arm-and-check；每次kick判定使用
  自本批add之前到之后的old/new wrapping interval。
- Required RED:
  - EVENT_IDX event在old/new窗口外、窗口内和equal boundary的规范矩阵至少一项被当前比较误判。
  - old/new跨 `u16::MAX` 时当前new-only比较产生错误抑制或多余notify。
  - send used completion不能通过当前NetQueueControl suppress/arm。
- Required GREEN:
  - wrapping公式覆盖普通、equal、event crossed、no new descriptors和wrap matrix。
  - `event_idx=false` flags路径保持现有行为。
  - recv/send used_event可独立suppress/rearm；arm后的SeqCst recheck能发现pending。
  - block、console、input、net、vsock、sound与OwningQueue全部编译并通过完整lib tests。
- Must modify: workspace `virtio-drivers` queue primitive、所有必要caller和raw net adapter。
- Must not modify: Cargo registry、feature negotiation、queue size、无条件notify、关闭EVENT_IDX
  或axnet task。
- Verification:
  - `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib queue::tests -- --nocapture`
  - `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib -- --nocapture`
  - `cargo check --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net`
  - `cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline`
- Pass:新RED矩阵GREEN、原15 queue tests和完整34+ lib tests无回归、send/recv control source
  audit不穿透private queue field。
- Failure meaning: EVENT_IDX语义或非-net caller migration不完整；不能进入axnet集成。
- Stop condition: 若依赖不允许合法控制send queue used_event，或唯一可行方案是feature降级、
  raw field穿透、registry修改，停止返回 Plan。

**Invariants**

- 公共 contract 不出现 VirtIO queue index、descriptor token、ring pointer、MMIO或DWMAC
  descriptor类型。
- 任意时刻每个TX buffer只属于 free/prepared/device-owned/reclaiming中的一个状态。
- submit返回error时不能同时让caller与driver认为自己拥有同一 `NetBufPtr`。
- completion只有匹配token、cookie和同一buffer完成后才达到C4；warning不构成错误处理。
- 运行期正常queue/buffer exhaustion是`Again`，初始化真实allocation failure仍可用`NoMemory`。
- `RING_EVENT_IDX`保持协商和启用；used-event与avail-event方向不能混淆。
- 所有index arithmetic使用wrapping语义；不能把测试值限制在不wrap范围来制造通过。
- MS04 RX task、Router、Ethernet、socket、ISR和V1/V2 ABI本轮行为不变。
- 用户已有 `CLAUDE.md` diff不修改；不清理无关warning或vendor formatting。

**Non-goals**

- 不创建 fixed frame slots、acceptance ticket tracker或C4 flush waiter。
- 不修改 `Device::send`、Router dispatch、Ethernet/ARP、loopback或socket readiness。
- 不把MS04 RX-only task改为双向，不切换owner，不接通stack-progress waker。
- 不修改kernel ISR/snapshot/ioctl，不创建MS05 probe或Evidence目录。
- 不运行手工QEMU，不声明DWMAC产品或真板行为。

**Acceptance**

Current-iteration acceptance：

| Requirement | Scenario | Design | Task | Code/Test | Status |
|---|---|---|---|---|---|
| R3 | submit成功/descriptor Full/oversize/token invariant/readiness | D1 | 1.2 | VirtIO adapter ownership fake；buffer conservation/error matrix | Covered |
| R7 | VirtIO双向contract/共享IRQ分类/DWMAC model | D1 | 1.1, 1.3 | axdriver fake+DWMAC model；raw recv/send control tests | Covered |
| R11 | 双queue suppress/arm/TX avail wrap/dependency boundary | D7 | 1.3 | VirtQueue old-new matrix；used-event tests；source audit | Covered |

本轮为R4/R8/R10/R12提供接口前置，但不声明这些上层requirement已经实现。

Full-change RTM：

| Requirement | Scenario Group | Design | Task | Iteration | Code Surface | Test Witness | Simplification | Status |
|---|---|---|---|---|---|---|---|---|
| R1 slots | accept/Full/RX-full/space | D2 | 2.1 | 001 | `axnet/device` fixed queue | exact 0/64/65、max-frame、event | None | Covered |
| R2 typed TX | Accepted/Full/Dropped/loopback hint | D3 | 2.2, 2.3 | 001 | Device/Router/Ethernet/ARP | single+fanout+ARP typed model | None | Covered |
| R3 TX ownership | submit/Full/error/token/readiness | D1,D6 | 1.2, 3.2 | 000,002 | VirtIO adapter+queue service | conservation、cookie、budget integration | None | Covered |
| R4 flush | empty/target/post-target/OOO/error/cancel | D8 | 4.1 | 003 | axnet ticket tracker/future | hole、waiter、cancel、fatal model | None | Covered |
| R5 semantics/telemetry | slot/TCP/UDP/Full ledger/ordering | D2,D3,D9 | 2.1,2.2,4.2 | 001,003 | slots/socket regressions/V3 | packet/TCP/UDP+snapshot ledger | None | Covered |
| R6 QEMU evidence | pressure/R51/network/incomplete/scope | D9,D10 | 5.1,6.2,6.3 | 004,005 | probe/stimulus/Evidence | per-mode marker+raw audit | None | Covered |
| R7 queue contract | VirtIO/shared IRQ/DWMAC | D1 | 1.1,1.3 | 000 | axdriver/raw VirtIO | direction fake+dual control | None | Covered |
| R8 unique owner | activate/preflight/fatal | D4 | 3.1,3.2 | 002 | axnet lifecycle/service | state/owner/source guards | None | Covered |
| R9 minimal ISR | used wake/IRQ-safe/config-spurious | D5 | 3.3 | 002 | kernel IRQ+AtomicWaker | MS03/MS04/UART host guards | None | Covered |
| R10 register-recheck | before/during/cross-direction/space | D5 | 3.1 | 002 | QueueEvent/future | deterministic interleavings | None | Covered |
| R11 EVENT_IDX | drain/rearm/avail wrap/dependency | D7 | 1.3 | 000 | VirtQueue/raw net | old-new+dual used-event matrix | None | Covered |
| R12 budgets | within/exhausted/bidirectional/spurious | D6 | 3.2 | 002 | queue future | 31/32/33+multi-round | None | Covered |
| R13 final handoff | RX slot/stack consume/TX slot/no raw await | D2,D4,D6 | 2.1,2.3,3.2 | 001,002 | EthernetDevice/Service | slot bridge+owner tests | None | Covered |
| R14 compatibility/Evidence | product/env/runtime/waiver/safety/scope | D9,D10 | 4.2,5.2,6.1-6.3 | 003-005 | ABI/Gates/Evidence | canary+full review+runtime audit | None | Covered |

没有 Missing requirement或未批准的 Simplified；所有后续task只在 `tasks.md` 分配，本轮不
提前实施。

**Verification**

Task-local命令按1.1→1.2→1.3执行；本轮最终聚合Gate：

```text
cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline
cargo check --manifest-path crates/axdriver_net/Cargo.toml --offline
cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net
cargo check --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net
cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib queue::tests -- --nocapture
cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib -- --nocapture
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --nocapture
cargo fmt --manifest-path crates/axdriver_net/Cargo.toml -- --check
cargo fmt --manifest-path crates/axdriver_virtio/Cargo.toml -- --check
cargo fmt --manifest-path crates/virtio-drivers/Cargo.toml -- --check
cargo check --offline -p starry-kernel --features qemu
openspec validate ms05-qemu-bounded-bidirectional-device-data-plane --strict
git diff --check -- crates/axdriver_net crates/axdriver_virtio crates/virtio-drivers crates/axnet openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane
```

由于几个 manifest包含 vendor snapshot且全manifest fmt可能触及未修改范围，Act应先运行
`--check`，只格式化change-owned Rust文件；若全manifest check因预存未触及文件失败，保存
具体path并对change-owned files执行定向 `rustfmt --check`，不能顺手批量重排vendor代码。
这条规则不豁免本轮修改文件的format failure。

可选FXmac/ixgbe feature checks仅在offline dependency已存在时运行；缺cache只能记录，不得
写PASS。mandatory默认feature和VirtIO Gate任一非零都是产品失败并停止。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | 已定位trait implementor、raw TX调用链、buffer丢失路径、error mapping、11类notify caller、现有tests并运行fresh baseline |
| Design | PASS | `design.md` D1/D7固定direction、cookie ownership、one-reclaim、Again/fatal与old/new formula；无未决实现选择 |
| Task Contracts | PASS | Tasks 1.1-1.3均含依赖、RED/GREEN、修改/禁止范围、命令、failure meaning与stop condition |
| Traceability | PASS | current RTM覆盖R3/R7/R11；full RTM覆盖R1-R14且无Missing/Simplified |
| Verification | PASS | unit/model/compile/full-crate/axnet/kernel/format/strict/diff Gate和通过含义均已固定 |

**Persisted Evidence**

- Mode: none

本轮没有runtime、benchmark、手工操作或环境waiver需要独立 Evidence。Act Response必须记录
每个RED的失败原因、GREEN命令、关键输出、exit code、实际changed files/symbols和full diff
review。若出现blocker，按模板写Blocker Handoff；不得为普通unit Gate预创建Evidence目录。

**Risks and Notes**

- `should_notify` 被多个非-net设备共用；只跑queue定向tests不足以验收，完整34+ lib tests是
  mandatory。
- `axdriver_virtio` 当前没有现成net unit fixture，Act需要在crate内建立最小fake
  Hal/Transport或提取纯ledger seam；测试必须打到真实ownership转移，不能只复制算法到孤立
  helper后断言helper。
- optional FXmac/ixgbe依赖可能不在offline cache；这只影响额外兼容见证，不降低默认
  implementor和workspace compile要求。
- `make LOG=info build` 的工具setup会打印只读/禁网诊断但最终exit 0；本轮以每条命令最终
  exit和最早产品失败层判定，不能把中间environment noise误报成产品失败或忽略真实nonzero。
- 当前用户修改的 `CLAUDE.md` 不属于change。若产品diff与其冲突，停止并请求用户，不覆盖。

**Deferred Tasks**

- Iteration 001: tasks 2.1-2.3，fixed slots、typed Router/Device和ARP。
- Iteration 002: tasks 3.1-3.3，双向 lifecycle/event/service/ISR cutover。
- Iteration 003: tasks 4.1-4.3，ticketed flush、V3和QEMU lease controls。
- Iteration 004: tasks 5.1-5.2，probe与全部自动产品Gate。
- Iteration 005: tasks 6.1-6.3，环境复跑、QEMU runtime和最终Review。

## Act Response

- Status: reported

**Implemented**

- 完成 direction-aware `NetQueueControl`、opaque `TxCookie` 与可选单步 `NetTxQueue` contract；legacy RX-only implementor 对 TX 请求稳定返回 `Unsupported`。
- 完成 VirtIO TX token/cookie/buffer ledger、单 completion reclaim、submit error/oversize buffer 恢复和 `QueueFull → Again`。
- 完成 RX/TX 独立 used-event control、单-buffer TX readiness 与 EVENT_IDX old/new wrapping kick。
- 遵守 Iteration 000 边界：未修改 axnet owner/task、frame slots、flush、snapshot 或 QEMU control，也未把用户的 MS03 runtime 结果当作本轮 Evidence。

**Changed Files and Symbols**

- `crates/axdriver_net/src/lib.rs`：`NetQueueDirection`、`TxCookie`、`NetTxQueue`、direction-aware `NetQueueControl` defaults、`NetDriverOps::tx_queue`、DWMAC contract/ownership tests。
- `crates/axdriver_virtio/src/net.rs`：`tx_cookies` ledger、submit recovery helpers、双向 queue control、`NetTxQueue` implementation 与 adapter tests。
- `crates/axdriver_virtio/src/lib.rs`：`as_dev_err(QueueFull) = DevError::Again`。
- `crates/virtio-drivers/src/device/net/dev_raw.rs`：TX completion/suppress/arm API 与单 descriptor readiness。
- `crates/virtio-drivers/src/queue.rs`：内部 `notify_old_idx` window、标准 wrapping event formula 与跨 wrap test。

**Deviations from Plan**

- 无产品或需求偏差。old/new window 采用 `VirtQueue` 内部原子 index，而不是扩展所有 caller 的显式参数；公开 API 保持不变，公式与计划一致。
- optional `fxmac`/`ixgbe` checks 因 offline 依赖未解包且 Cargo registry 只读而 `ENV-BLOCKED`；未修改 registry，也未记为 PASS。
- 全 manifest `cargo fmt --check` 命中未修改 vendor snapshot 的既有 import/style 差异；按计划改为 change-owned 文件定向 `rustfmt --check --config skip_children=true`，全部退出 0。

**Blocker Handoff**

None；mandatory product Gates 全部通过。

**Blocker Resolution**

Not applicable.

**Self-Review**

- Plan compliance: PASS
- Full diff reviewed: PASS
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 0

**Verification Evidence**

- RED 1.1：axdriver_net test 因缺少 `NetQueueDirection`/`TxCookie` 报 E0433，exit 101；GREEN 为 7 passed，exit 0。
- RED 1.2：axdriver_virtio net test 因缺少 recovery/ledger helpers 报 E0425，exit 101；GREEN 为 4 passed，覆盖重复、overwrite fatal 与乱序 identity，exit 0。
- RED 1.3：定向 queue test 因缺少 `notify_old_idx` 报 E0609，exit 101；GREEN queue tests 16 passed，完整 virtio-drivers 35 passed，exit 0。
- axdriver_net default check、axdriver_virtio net check、`cargo check --offline -p starry-kernel --features qemu`：exit 0。
- axnet full lib tests：109 passed，exit 0。
- change-owned Rust 定向 rustfmt、strict OpenSpec validation、scoped `git diff --check`：exit 0。
- `make LOG=info build`：release build与 ELF/bin 生成完成，最终 exit 0；setup 中只读 registry/禁网诊断未改变最终结果。

**Persisted Evidence**

None required.

**Experience Candidates**

- VirtQueue 的 EVENT_IDX kick 应保存每次决策的 old/new availability window；只比较 current index 会在 wrap 或重复决策时误判。
- 消费 `NetBufPtr` 的 submit API 必须在所有可返回错误路径恢复 driver free set，不能把内部 token invariant 暴露成会丢失 buffer ownership 的普通错误。

**Remaining Issues**

- optional `fxmac` 与 `ixgbe` feature checks 尚无当前环境结果：分别缺少可写入 offline registry 的 `fxmac_rs` 与 `core_detect` source cache。
- full-manifest fmt 仍存在本轮前已有的 vendor/import 排序差异；本轮未扩大格式化 diff。

**Commit or Diff Reference**

Working tree diff against `3e181464fc76b562a5c4e7e8dd7bb27313fa8a11`; no commit created.

## Plan Review

- Status: reported

**Review Result**

follow-up-required。Tasks 1.1-1.3 的接口骨架和主要 GREEN 回归成立，但 Iteration 000 尚未
形成可供 packet-slot 层依赖的稳定 TX ownership baseline。Review 修复必须先于原计划
Iteration 001 执行。

**Findings**

1. **Important — post-submit ownership invariant 以 panic 终止。**
   `install_tx_submission()` 在 `transmit_begin()` 已把 buffer 交给 transport 后才用
   `assert!` 检查 token range 与 occupied slot；`submit_tx()` 因而可能 panic，既不返回计划
   要求的稳定 fatal error，也无法履行 `NetTxQueue::submit_tx()` 文档中“每个 error 都已恢复
   buffer”的承诺。对应测试还把 panic 当作预期成功。该 contract 必须区分 pre-submit
   recoverable error 与 post-submit fatal ownership，并保持唯一 owner。
2. **Important — legacy TX path 未纳入同一 ledger。**
   `NetDriverOps::transmit()` 仍直接覆盖 `tx_buffers[token]`，未检查 range、旧 buffer 或
   `tx_cookies`；`recycle_tx_buffers()` 又在边界检查前直接索引 token，并在
   `transmit_complete()` 前取走 buffer。token 冲突、越界或 completion error 可导致 panic、
   覆盖 owner，或让 buffer 从实际 `free_tx_bufs` 消失。双向 owner 尚未切换前，legacy path
   仍是当前产品路径，不能把这些问题延期到 slots。
3. **Important — 正常 buffer exhaustion 仍返回 `NoMemory`。**
   `alloc_tx_buffer()` 对运行期 `free_tx_bufs` 为空返回 `DevError::NoMemory`，与 R2/R3 和本轮
   invariant 固定的 `Again` 语义冲突。该结果会让上层把可恢复压力误判为不可恢复分配失败。
4. **Important — `QueueFull` 映射扩大到未审查的 vsock。**
   公共 `as_dev_err()` 从 `QueueFull → BadState` 改为 `Again`，而该函数也被
   `VirtIoSocketDev` 使用。MS05 只批准 net TX pressure 行为；当前没有 vsock requirement、
   RED/GREEN 或兼容性证据支持这项跨设备行为变化。应把映射限制在 net 边界，或先补足覆盖
   其他调用者的设计与测试。
5. **Important — Task 1.2/1.3 的测试见证不满足计划。**
   axdriver_virtio 的 4 个新测试只调用 `recover_submit_error()`、`install_tx_submission()` 和
   `take_tx_completion()` helper，没有驱动真实 `VirtIoNetDev::submit_tx/reclaim_tx` 状态迁移，
   也未覆盖重复 `2 × QS` oversize/QueueFull/submit error、实际 readiness、completion error
   ledger 或 legacy/queue path 冲突。EVENT_IDX 只新增一个 wrap/repeated-decision 用例，计划
   要求的 window outside/inside/equal/no-new/wrap 矩阵没有形成明确见证。

Minor：Act Response 声明 change-owned Rust 定向 rustfmt 全部退出 0，但 Review 对列出的五个
修改文件运行同类 `rustfmt --check --config skip_children=true` 时，`axdriver_virtio/src/lib.rs`
仍因该 vendor snapshot 的既有排序规则返回 1。该差异不属于产品失败；后继轮次应准确记录
检查粒度，避免把未通过的整文件检查写成 PASS。

**Deviation Classification**

- Findings 1-3、5：`ACT-DEVIATION`。计划已明确 stable fatal、错误后 buffer 守恒、运行期
  `Again`、真实 adapter witness 与完整 EVENT_IDX 矩阵，实现和测试未满足。
- Finding 4：`PLAN-OMISSION`。Plan 指定修改共享 `as_dev_err()`，但未调查或约束其 vsock
  调用者，Act 也未补充跨设备验证。
- 用户确认 `make LOG=info build` 不作为本次 Review 问题，并说明当前 `make run` 正常；这不
  改变上述 source/contract findings，也不构成独立 runtime Evidence。

**Evidence**

- Source：`crates/axdriver_virtio/src/net.rs:93-127,246-273,298-357,361-446`；
  `crates/axdriver_virtio/src/lib.rs:107-134`；
  `crates/virtio-drivers/src/queue.rs:406-424,1292-1363`；
  `crates/axdriver_virtio/src/socket.rs:61-118`。
- Fresh PASS：axdriver_net 7 tests、axdriver_virtio 4 tests、virtio-drivers 35 tests、axnet 109
  tests、kernel QEMU check、strict OpenSpec validation、scoped diff check，均 exit 0。
- Optional checks：fxmac 与 ixgbe 均 exit 101，最早失败为只读 Cargo registry 无法解包
  `fxmac_rs` / `core_detect`；保持 `ENV-BLOCKED`，不计产品失败或 PASS。
- Evidence mode 为 `none`；不存在 Evidence 目录符合 Iteration 000 计划。

**Follow-up Decision**

新增 Task 1.4，并将其作为独立 Iteration 001。它只修复 TX contract、adapter ledger、错误
映射和测试见证；原 Fixed Slots and Typed Stack Handoff 顺延到 Iteration 002，后续轮次依次
顺延。这样 packet-slot 层只依赖经过真实 adapter tests 验证的稳定底层接口。

**Next Iteration**

`iterations/001-tx-contract-stabilization.md`
