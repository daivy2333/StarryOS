## Context

MS03 已把 QEMU VirtIO-MMIO net IRQ 7 接到设备 handler，但 handler 仍只做
status 分类、ACK 和 telemetry。RX descriptor 由
`Service::poll -> Router::poll -> EthernetDevice::recv -> VirtIoNetDev::receive`
推进；`EthernetDevice::requires_polling()` 因 MMIO probe 传入 `irq=None` 而保留
10ms fallback。

当前 QEMU 构建使用 axdriver 的静态设备模型：
`AxNetDevice` 是 `VirtIoNetDev<VirtIoHalImpl, MmioTransport, 64>` 的类型别名，
不是 `Box<dyn NetDriverOps>`。axnet 再把它封装成 `Box<dyn Device>` 放入 Router。
因此异步入口需要通过现有 `Device`/Router owner 到达唯一 NIC，不能创建第二个
VirtIO transport 或保存第二份 queue 状态。

当前实现还有四个直接影响 MS04 的边界：

- `EthernetDevice::recv` 在一次调用中循环消费 ARP、非 IPv4 和 malformed frame，
  直到交付一个 IPv4 packet 或队列为空；`bool` 返回值无法表达“一次 completion
  已消费但未交付 IP packet”，也无法执行 descriptor budget。
- Router RX buffer 有 64 个 packet slot。`Router::poll` 只在 buffer 未满时调用
  device，但异步 task 需要在 reap 下一个 descriptor 前检查容量，并在协议栈释放
  空间后获得软件 wake。
- `virtio-drivers 0.7.5` 在协商 `RING_EVENT_IDX` 后让
  `VirtQueue::set_dev_notify` 成为 no-op；`pop_used` 又会在每次 completion 后把
  `used_event` 写成新的 `last_used_idx`。现有 API 不能在有界 drain 期间抑制通知。
- kernel 手写的 critical-section ABI 在 acquire 时关闭 IRQ，在 release 时无条件
  开启 IRQ。`embassy-sync 0.6.2` 的 `AtomicWaker` 使用该 critical-section，ISR
  内 `wake()` 可能在 PLIC complete 前提前开中断。

当前 revision 为 `16d9a16a2b65a574022faaee39b465f6f7aebd45`。2026-08-09
新鲜基线为：`make host-test` 6+8+20 tests 通过，axnet service 8/8，UART
62 unit + 18 doctest 通过；`make LOG=info build` 最终退出 0，生成 39,792,832
bytes 的 QEMU 镜像。构建准备阶段出现 Cargo home 只读和联网失败，但已有
`rust-objcopy` 使最终构建成功，按 R44 记为 PASS 而非 `ENV-BLOCKED`。

## Goals / Non-Goals

**Goals:**

- 建立一个由 IRQ 7 used-ring cause 唤醒的、唯一且长驻的 RX queue task。
- 在 ISR、task 注册、通知重臂和 completion 到达的所有交错下关闭 lost wakeup。
- 每次调度最多处理 32 个 completion，并在 backlog 下自调度、至少让出一次 CPU。
- 复用现有 Router RX buffer 完成临时 handoff，保持每次 reap 后立即 refill。
- 保留 `RING_EVENT_IDX`，在工作区拥有的依赖中实现有效 suppression/rearm。
- 修复共享 critical-section 的 IRQ restore 语义，并保留 UART 与 console 回归。
- 保持 MS01/MS02 socket、同步 TX、ARP、MS03 cause/ACK/EOI 和 10ms 协议栈推进。
- 提供可区分 IRQ、task、descriptor、Router backpressure 和 fault 的运行见证。

**Non-Goals:**

- 异步 TX、TX completion、flush/drain 或 peer delivery。
- MS05 最终 packet slot、stack runner、socket readiness 或多 waiter。
- 多 NIC、multiqueue、RSS、SMP、跨 hart wake、PCI 或真板实现。
- reset、cancel、remove、link flap、热插拔或激活后自动回退。
- DWMAC、DMA/cache、PHY、clock/reset 产品代码和性能优化。
- 更换 executor；task 继续使用 axtask 的 `spawn_with_name + block_on`。

## Decisions

### D1：工作区本地化三个依赖层，公共 contract 保持 transport-neutral

将 crates.io 的以下确切版本复制到工作区并用根 `Cargo.toml [patch.crates-io]`
接管：

- `axdriver_net 0.1.4-preview.3` → `crates/axdriver_net`
- `axdriver_virtio 0.1.4-preview.3` → `crates/axdriver_virtio`
- `virtio-drivers 0.7.5` → `crates/virtio-drivers`

三个目录加入 workspace `exclude`，各自通过 `--manifest-path` 测试。不得修改
Cargo registry，不本地化 `axdriver 0.3.0-preview.2`，也不关闭
`RING_EVENT_IDX`。

`axdriver_net` 新增对象安全的 `NetQueueControl`。它只暴露：

- RX completion 当前是否可见；
- 抑制 RX used-buffer 通知；
- 重臂 RX 通知，并返回 barrier 后 recheck 是否仍有 completion；
- 操作失败时的 `DevResult`。

`NetDriverOps` 增加默认返回 `None` 的可选 queue-control accessor。
数据移动仍由现有 `receive()` 与 `recycle_rx_buffer()` 完成；两者与
`NetQueueControl` 共同构成 reap/refill contract。这样不会重复一套 buffer API，
也不会把 VirtIO token 或 ring 字段泄漏到 axnet。VirtIO 实现返回自身的 control；
其他 net driver 保持默认 `None`。notification-control 方法必须具备调用级原子语义：
返回错误时不得留下半 suppress/半 arm 状态；VirtIO 的共享 ring 写入本身不返回
可恢复错误。

DWMAC 模型可以把同一 contract 映射为：descriptor owner/completion 检查、现有
receive/recycle、DMA channel interrupt mask、重新 enable 后的 status/descriptor
recheck。该映射不需要 VirtIO ring 类型，满足接口审查；MS04 不加入 DWMAC 代码。

替代方案：在 axnet downcast 到 `VirtIoNetDev`。拒绝，因为当前虽是静态别名，
Router 已擦除为 `dyn Device`，且该方案会把 transport 细节写入公共数据面。

### D2：修正 EVENT_IDX 的 used-event 状态机，不把 flags no-op 当 rearm

本地 `VirtQueue` 记录 used-buffer notification 是否启用。对 split virtqueue：

- suppression 在 `event_idx=false` 时写 `VRING_AVAIL_F_NO_INTERRUPT`；在
  `event_idx=true` 时写 `used_event = last_used_idx.wrapping_sub(1)`，并标记
  suppressed；
- suppressed 期间 `pop_used` 只推进 `last_used_idx`，不得覆盖 `used_event`；
- arm 在 `event_idx=false` 时清 flag；在 `event_idx=true` 时写
  `used_event = last_used_idx`；随后执行跨设备共享 ring 所需的强 fence，再读取
  used index 并返回 pending 状态；
- wrap-around 使用 `u16::wrapping_*`，由 FakeTransport unit tests 覆盖。

`VirtIONetRaw` 只增加 RX queue 的 suppress/arm-and-check 方法；不得调用同时操作
TX queue 的现有 `disable_interrupts/enable_interrupts`。同步 TX 的通知和 completion
语义保持不变。

该选择依据 VirtIO used-event 语义：设备在写入 completion 前的 used idx 等于
`used_event` 时通知。arm 到当前 `last_used_idx` 使下一 completion 可通知；
suppression 值避开下一索引，且禁止 `pop_used` 在 drain 中重新 arm。

替代方案：关闭 `RING_EVENT_IDX` 或只调用 `set_dev_notify(false)`。两者分别改变
已批准 feature 基线或在当前版本中无效，因此拒绝。

### D3：使用官方 critical-section restore-state contract

kernel 直接依赖 `critical-section 1.2` 并启用 `restore-state-bool`。使用
`critical_section::set_impl!` 和 `critical_section::Impl` 替代手写符号：

```text
acquire: was_enabled = irqs_enabled(); disable_irqs(); return was_enabled
release: if was_enabled { enable_irqs() }
```

进入时 IRQ 已关闭，嵌套 release 仍保持关闭；只有最外层、进入时已启用的
critical-section 才恢复 IRQ。顺序保证遵循 critical-section crate contract。

新增可被 host harness 引入的纯 restore-policy seam，测试 enabled、disabled 和
两层嵌套。QEMU net handler 在调用 `AtomicWaker::wake()` 前后读取 IRQ enable
状态，任何“进入 disabled、返回 enabled”都增加 violation counter 并使运行 Gate
失败。host harness 还必须审计真实 production glue 仍委托该 seam；guard 用一个
legacy direct-call fixture 证明能拒绝复制 restore 决策，且不得依赖行号或把 backend
中合法的 axhal primitive 调用误判为绕过。UART unit/doctest 和 QEMU
UART-only/concurrent 继续作为共享实现回归。

替代方案：继续导出 ABI 并只改返回类型。拒绝，因为官方 macro/trait 能让 feature
选择与函数签名由同一依赖校验，减少 ABI 漂移。

### D4：task 自己完成激活握手，owner 只发生一次切换

axnet 保存一个单调生命周期：

```text
Polling -> Spawned -> Active -> Faulted
                   \-> Unavailable
```

T5.2 由 axnet 提供 start entry。start 用 CAS 将 `Polling` 改为 `Spawned`，只创建一个
有固定名称的 axtask；重复调用不得创建第二个 task。T5.2 不从 kernel 调用该入口，
因为 MS03 handler 尚不发布 queue event；T6.1 把 handler 升级为 publish/wake 后，才在
IRQ 注册成功路径调用 start。这样不会出现通知已抑制但 ISR 仍只做诊断的半接线状态。

新 task 第一次运行时取得 `SERVICE` 锁，确认恰好一个目标 Ethernet device、
queue control 可用且 Router/driver 状态可服务；随后先 suppress RX 通知，再在同一
锁内把状态发布为 `Active`。在此之前普通 Router polling 仍是 owner。单 hart 加
Service 锁保证 polling 不会和切换临界区并发消费。

缺 Service、缺 NIC、缺 queue control 或 preflight 失败时状态变为 `Unavailable`，
task 退出，polling owner 保持。`spawn_with_name` 没有可恢复的错误返回；在 task
第一次 poll 前状态仍是 `Spawned`，Router 按 polling 路径运行，因此“task 尚未
开始”不会形成半切换。

`Active` 和 `Faulted` 都禁止普通 Router 路径消费目标 NIC。激活后 fatal error
进入 `Faulted`，保留 async owner 身份、保持通知抑制并停止 task，不自动回退。

替代方案：start 调用者先切 owner 再 spawn。拒绝，因为 task 尚未运行或 preflight
失败时会留下无人推进的半切换状态。

### D5：ISR 只发布 used-ring 事件，task 使用 generation + AtomicWaker

axnet 提供 ISR-safe 的固定通知入口。used-ring 或 combined cause 的 handler 在完成
设备 ACK 和 telemetry 后：

1. 对 `AtomicU64` generation 执行 `fetch_add(1, Release)`；
2. 调用唯一 `AtomicWaker::wake()`；
3. 返回，由平台执行 PLIC complete。

config-only、unknown-only 和 spurious cause 不发布 RX generation，也不伪造
completion。ISR 不取得 Service 锁，不调用 queue control/receive/recycle，不运行
smoltcp。handler 把 MMIO status 的 raw low byte交给 classifier 和 telemetry，只用
`raw & 0x03` 写 ACK；否则 unknown-only 会被误记为 spurious，known+unknown 也会丢失
unknown 观测。

task 准备等待时按以下顺序执行：

```text
确认无 completion
  -> Acquire 读取 generation
  -> 注册 AtomicWaker
  -> arm RX notification，并取得 barrier 后 completion recheck
  -> Acquire 再读 generation
  -> 若 pending 或 generation 变化则自唤醒
  -> 否则返回 Pending
```

`arm_rx_notify_and_check` 的错误属于 queue-control fatal，必须由 wait decision 连同
`DevError` 明确返回，不得映射为 Quiescent/Sleep、藏在 closure side channel 或依赖
10ms fallback。激活前 arm/suppress/preflight 失败进入 Unavailable；Active 后失败进入
Faulted 并保留 async owner。

task 开始 service、budget 用尽或 Router 满时保持通知 suppressed。AtomicWaker
只有该 queue task 一个 waiter；MS05 以后 socket waiter 由 stack runner 分发，
不在本轮复用这个单槽。

替代方案：只依赖 AtomicWaker 而没有 generation/recheck。拒绝，因为事件可能在
首次检查与注册之间发生。

### D6：Device 接口改为“一次物理进度”，Router 提供 RX-only 服务入口

把当前 `Device::recv() -> bool` 的含义拆成一次调用最多处理一个底层 RX 单元的
结果：

- `Empty`：没有 completion；
- `Consumed`：消费并 refill 一个 completion，但没有向 Router 交付 IP packet，
  例如 ARP、非目标帧或 malformed frame；
- `Delivered`：消费、交付一个 IP packet 并 refill；
- `Fault(DevError)`：receive/recycle、Router enqueue 或 queue 状态错误；保留错误类别
  供后续 lifecycle telemetry 使用。

Ethernet 路径每次只调用一次 driver `receive`。取得 `NetBufPtr` 后，无论 frame
是否交付，都在该次调用返回前调用一次 `recycle_rx_buffer`；recycle 失败是 fatal。
若 precheck 后 Router enqueue 仍失败，必须先 recycle 再报告 fatal，不得 unwrap、
静默 drop 或持有 buffer 跨返回。
ARP reply 和 pending ARP packet 仍可走现有同步 TX。Loopback 映射到相同结果，
不获得异步 queue owner。

axnet host tests 为 fake NIC 链接 `axdriver/dyn` 时，test-only axklib stub 必须匹配
trait-ffi 生成的 `extern "Rust" fn(PhysAddr, usize) -> AxResult<VirtAddr>`。stub 只返回
明确错误，不执行 iomap；ABI、参数或返回类型不允许仅凭“当前不调用”而缩减。

Router 增加按目标 device index 的 RX-only service：它只检查现有 RX buffer 容量、
调用一次 device RX、返回进度，不调用 smoltcp maintenance、ingress、egress 或
socket readiness。普通 `Router::poll` 在状态为 `Active/Faulted` 时跳过目标
Ethernet RX，但继续服务 loopback。

该 Router primitive 由 `Service` 通过其已保存的唯一 target index 转发给未来同 crate
的 async RX 模块；caller 不传 raw index，也不取得或复制 NIC handle。缺少 target 时
返回可匹配的 `BadState` fault。这个 crate-private seam 必须由 sibling-module compile/
unit witness 证明可达，不能只在 `service.rs` 自身测试里调用私有字段或私有 signal。

T5.2 在 axnet `Device` 层增加 transport-neutral queue-control wrapper：目标
`EthernetDevice` 只把 completion-visible、suppress 和 arm-and-check 委托给其内部
`NetDriverOps::queue_control()`；Loopback 与不支持的设备返回明确 unavailable/error。
Router/Service 仍按保存的 target index 调用 wrapper，不向 task 暴露 VirtIO 类型、raw
descriptor、device index 或第二个 NIC handle，也不修改 registry axdriver。

T4.2 只引入 `PollingOwned/AsyncOwned` 消费权视图，不提前实现 lifecycle 转换。
`poll_interfaces` 在 T5 接入前显式使用 PollingOwned；T5 再把
Polling/Spawned/Unavailable 映射为 PollingOwned，把 Active/Faulted 映射为 AsyncOwned。

替代方案：给现有 `recv(bool)` 外层加 32 次循环。拒绝，因为一次 `recv` 内部可能
无界消费 ARP/非 IPv4 completion，budget 不成立。

### D7：固定 budget=32，并用软件 wake 处理 Router full

RX queue size 是 64，MS04 固定每次 future poll 最多服务 32 个 completion。
32 是半个 queue 的保守调度边界，不声明为性能最优值。

每轮 task：

1. 确认通知 suppressed；
2. 在 reap 前检查 Router RX buffer 是否满；
3. 最多执行 32 次“一 completion + handoff + refill”；
4. 遇到 `Empty` 进入 D5 register-recheck；
5. 精确到 32 且仍有 completion 时增加 exhaustion/yield counter，保持 suppressed，
   对自身 waker 执行一次 wake 并返回 `Pending`。

每条返回 `Pending`、task 退出或进入 Faulted 的路径都必须先释放 `SERVICE` guard；
future 不保存 MutexGuard，也不在持锁状态下跨调度点。

axtask `block_on` 在 future 返回 `Pending` 且已 self-wake 时调用 `yield_now()`，
因此 backlog 至少产生一次调度让出，不在同一次 poll 无界 drain。

Router 满时 task 在 reap 下一个 completion 前设置 `waiting_for_space` 并返回
`Pending`，未 reap descriptor 留在 used ring。现有 `Service::poll` 在 smoltcp
ingress 后检测“task 等空间且 buffer 已有空位”，清该标志并软件 wake 同一 task。
该 wake 不依赖新硬件 IRQ。telemetry 使用 Relaxed；生命周期、generation 和
space handoff 使用 Release/Acquire 或 AcqRel CAS。

event generation 与 Router-space wake 共享同一个 queue-task waiter 状态，而不是各自
建立单槽 waker。task 在锁外注册同一个 `AtomicWaker`；取得 `SERVICE` 锁后执行 target
one-step，若得到 Full，则在锁内重新检查 Router space，只有仍满时才 Release 发布
`waiting_for_space`。若已出现空间则返回 retry/continue 决策，不睡眠。这样释放发生在
register 后、waiting 发布前时也不会丢失进度。

替代方案：Router 满后仍 reap 到临时 `NetBufPtr`。拒绝，因为会在 task 睡眠期间
长期占有已完成 buffer，扩大 ownership 和泄漏边界。

### D8：10ms fallback 继续推进协议栈，但不再推进目标 RX queue

`requires_polling()` 对 MS04 目标 NIC 仍返回 true，保留现有 10ms deadline、
smoltcp timers、ingress/egress 和 socket progress。owner 判断位于 Router RX 路径，
不是 timeout selection：

- `Polling/Spawned/Unavailable`：普通 Router poll 可以调用目标 NIC RX；
- `Active/Faulted`：普通 Router poll 跳过目标 NIC RX；
- 所有状态：同步 TX、loopback、protocol maintenance 和 socket polling 保留。

这避免提前实现 MS05 stack runner，也避免把 `irq_num=Some(7)` 交给 axtask 全局 IRQ
waker。IRQ 7 继续由 MS03 device handler 管理。

替代方案：激活后让 `requires_polling=false`。拒绝，因为当前没有 stack runner，
会同时丢失协议 timer 和 Router 消费进度。

### D9：错误、telemetry 与 probe 分层

激活前错误记录阶段和 `Unavailable/Polling owner`。激活后以下错误进入
`Faulted`：非 `Again` 的 driver receive error、recycle error、queue-control error、
owner 状态异常或不可能的多 owner。malformed、非目标 MAC、非 IPv4 和正常 ARP
属于已消费 frame，不是 queue fault。

QEMU-only snapshot 在现有 MS03 cause/ACK counter 之外至少包含：lifecycle/owner、
ISR publish/wake、task poll、reaped、refilled、delivered、non-IP consumed、budget
exhaustion、self-yield、Router-full wait、space wake、empty check、fault、last error、
critical-section IRQ restore violation。计数只观测，不作为 owner 正确性的唯一来源。
现有 snapshot ioctl 没有长度参数并写完整 `repr(C)` 对象，不能通过原地 append 保持
二进制兼容。`0x4e49_4431` 固定为 MS03 V1，只写原有 8 个 `u64`；MS04 使用新的
`0x4e49_4432` V2 command 和独立固定结构。旧 probe、MS16 workload 和已编译 payload
继续安全读取 V1，新 MS04 probe 只读取 V2。每个 command 的 Rust/C size、offset 和
全部 consumer buffer 都必须有 source/type witness；未来扩展不得增大已有 command 的
写入尺寸。

新增 MS04 guest probe 和 host burst stimulus。probe 提供 idle、software nudge、
RX burst/fairness 和 snapshot 模式。nudge 使用独立 `0x4e49_4e31` command，只调用
不增加 generation 或 ISR counter 的 software wake，并单独增加 software-nudge counter；
不得复用 ISR publisher 或伪造 completion。
运行时以 `reaped delta == refilled delta`、fault/restore violation 为零、burst 下 IRQ
和 task 都推进、budget/yield 可见、idle/nudge 不 busy-loop 为判据。MS01/MS02/MS03
使用既有 payload 做独立回归。

替代方案：在 ISR 或每个 packet 打印。拒绝，因为串口 I/O 会改变 IRQ 时序并污染
burst 结果。

### D10：自动 Gate 在前，R44 环境交接与 QEMU 手测使用最终独立 iteration

Act 必须先执行依赖 unit tests、MS04 host race/state tests、axnet tests、UART tests、
format/source/diff checks、QEMU target build 和 D1 async-UART compile check。自动
命令出现产品错误时停止，不进入手工任务。

实现按单一可诊断目标拆成多个 iteration；每轮只承担一个紧密内聚的接口、状态机
或集成面，并在进入下一轮前完成本轮自动回归和 Plan Review。全量自动 Gate 单独
收口。sandbox 外复跑与 QEMU runtime 手测再放入最后一个 user-only iteration，
不与产品实现、修复或自动 Gate 混排。

自动命令最终失败且原始日志满足 R44 `ENV-BLOCKED` 分类时，Act 继续完成其他可执行
自动 Gate，把原命令、退出码和最早环境失败层加入最终手工批次。用户先在 sandbox
外复跑这些命令，全部成功后再手工启动 QEMU、逐条输入 guest 命令和采集 Evidence。

最终 manual iteration 的 Persisted Evidence 模式为 `required`，路径在创建该轮时
固定。至少保存 README 索引、environment、commands、自动或手工 build 输出、
artifact hashes、完整 QEMU serial、MS04 probe、MS03 regression、MS01/MS02
regression 和完成状态。Plan 不预建 Evidence；Act/用户在执行时创建。

替代方案：把 target build 或 QEMU 交接放在实现 iteration。拒绝，因为用户明确要求
所有手工操作集中到最后一个独立 iteration。

## Current-State and Target Call Paths

当前 RX：

```text
socket call / 10ms timeout
  -> poll_interfaces
  -> Service::poll
  -> Router::poll
  -> EthernetDevice::recv (内部无界跳过非 IPv4)
  -> VirtIoNetDev::receive + recycle
```

目标 RX：

```text
VirtIO used completion
  -> IRQ 7 handler: cause + ACK + telemetry + generation + AtomicWaker
  -> PLIC complete
  -> unique axtask / block_on
  -> suppress notification
  -> Service lock
  -> Router RX-only one-completion service, budget <= 32
  -> Ethernet parse / optional sync ARP TX / Router handoff
  -> immediate recycle/refill
  -> self-yield | wait-for-space | register-arm-recheck
```

协议栈继续：

```text
socket call / 10ms timeout
  -> Service::poll
  -> skip active eth RX, keep loopback
  -> smoltcp maintenance + ingress + egress
  -> Router space transition software-wakes RX task
```

## Risks / Trade-offs

- **[EVENT_IDX suppression 值或 wrap 实现错误]** → FakeTransport 覆盖 enabled、
  suppressed-pop、arm-pending、空队列和 `u16` wrap；QEMU idle/burst 检查 storm 与
  lost wakeup。
- **[Service 锁内最多处理 32 帧增加其他网络调用延迟]** → budget 固定为 32，
  backlog self-wake 后由 axtask `yield_now` 让出；本轮不宣称性能优化。
- **[Router trait 改动影响 loopback/vsock]** → loopback one-step tests、axnet 全量
  lib tests 和 MS01 socket 回归；vsock 不进入 Device RX trait。
- **[critical-section feature 统一影响 UART/其他依赖]** → host 嵌套状态 tests、UART
  unit/doctest、D1 async-UART compile、QEMU UART-only/concurrent 与 restore violation。
- **[激活后 fault 使 RX 停止]** → 明确 fault telemetry 和启动日志；不自动回退以
  保持 owner 唯一，恢复留给后续 reset/cancel change。
- **[compatibility ACK 与 handler ACK 并存]** → handler 仍是 used cause 的正常 ACK
  点；task 不依赖 receive 内 ACK 完成唤醒，MS03 ack/handler counters 和 source review
  监测偏差。
- **[sandbox 能力差异]** → 按 R44 只延后明确 `ENV-BLOCKED` 项；最终成功的命令
  仍记 PASS，产品错误不得转交用户。
- **[QEMU 证据被误外推]** → Evidence 明确单 hart、单 VirtIO-MMIO NIC；不声明
  PCI、SMP、DWMAC 或真板结论。

## Migration Plan

1. 本地化依赖并用原版本 build/check 证明 patch 未改变基线。
2. 先写 EVENT_IDX RED tests，再实现 suppression/arm-and-check 和 RX-only adapter。
3. 先写 critical-section restore RED tests，再替换 kernel 实现并跑 UART 回归。
4. 把 Device RX 改为 one-completion，建立 Router full/space 和 ownership tests。
5. 实现 lifecycle、budget、generation/register-recheck future 和唯一 task。
6. 把 MS03 ISR used cause 接到固定 waker，扩展 snapshot/probe。
7. 完成所有自动 Gate、source/spec/code/full-diff review。
8. 仅在最后处理 R44 `ENV-BLOCKED` 复跑与手工 QEMU Evidence。

回滚按反序执行：先停用 ISR publish 和 task 启动，使状态保持 polling；再移除 axnet
async owner/one-step 接口；最后移除 Cargo patches 和 critical-section feature。回滚
必须保留 MS03 handler、MS02 polling、同步 TX、UART device handler 和 early/panic
console。激活后的运行时 fault 不执行在线回滚。

## Requirements Traceability Matrix

| Requirement | Scenario | Design | Task | Code Surface | Test Witness | Simplification | Status |
|---|---|---|---|---|---|---|---|
| R1 queue contract | VirtIO mapping；DWMAC review | D1 | T1 | `axdriver_net::NetQueueControl`; `axdriver_virtio::VirtIoNetDev` | trait compile；fake adapter tests；DWMAC mapping review | None | Covered |
| R2 unique owner | activate；preflight fail；fatal | D4,D8,D9 | T4,T5 | axnet lifecycle；`Router::poll`; start/task | lifecycle and duplicate-start tests；source audit；QEMU owner snapshot | None | Covered |
| R3 minimal IRQ-safe ISR | used wake；IRQ restore；UART | D3,D5,D9 | T3,T6 | kernel critical impl；`virtio_net_irq` | restore host tests；MS03 harness；UART tests；QEMU violation counter | None | Covered |
| R4 register-recheck | before/during/after arm | D2,D5 | T2,T5 | VirtQueue arm；axnet future | deterministic interleaving model tests；QEMU burst | None | Covered |
| R5 EVENT_IDX | suppress；arm；dependency boundary | D1,D2 | T1,T2 | local three crates；root patches | VirtQueue FakeTransport wrap/recheck tests；feature/source audit | None | Covered |
| R6 bounded task | <=budget；exhausted；spurious | D5,D7,D9 | T5,T6 | async RX future；telemetry | budget/state tests；nudge/idle/burst probe | None | Covered |
| R7 Router handoff | IPv4；full；space wake | D6,D7 | T4,T5 | Device result；Router RX-only；Service wake | fake device/Router tests；descriptor counters | None | Covered |
| R8 compatibility/evidence | product fail；ENV block；manual final；scope | D8-D10 | T3,T6,T7,T8 | tests/Makefile/probe/Evidence | host/build/UART/MS01-MS03/QEMU logs | None | Covered |
| M1 minimal ISR preserved | used/config/spurious | D5,D9 | T6 | `virtio_net_irq` | MS03 host harness + source audit + runtime counters | None | Covered |
| M2 one NIC/owner | startup/control/evidence | D1,D4,D8 | T1,T4,T5 | probe/init/Router | lifecycle tests；single-device source audit；runtime owner | None | Covered |
| M3 ACK/EOI/rearm | repeat；EVENT_IDX；window；failure | D2,D5,D9 | T2,T5,T6 | handler/PLIC/VirtQueue/task | queue tests；MS03 counters；burst/idle | None | Covered |
| M4 MS02/UART recovery | pre-switch fail；active；UART；regression | D3,D4,D8,D10 | T3,T5,T7,T8 | critical impl；axnet fallback；tests | UART/D1/QEMU builds；MS01-MS03 manual regression | None | Covered |

## Open Questions

None.
