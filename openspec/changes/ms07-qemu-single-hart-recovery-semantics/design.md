## Context

当前基线 revision 为 `9d58bd422577959f84fc5e5a59db5a94bd7eb7fc`（`net-k3`）。Explorer 与本轮代码复核确认：

- `axdriver_net::TxCookie` 只有 owner-side `u64`，`NetQueueControl` 只有 completion/notification 操作，没有 link、quiesce 或 reset contract。
- `axdriver_virtio::VirtIoNetDev` 以 `TxSlot::Queue(buffer, cookie)` 和真实 descriptor ledger 保持守恒；ownership fault 后 `tx_fault` 永久锁死。RX backing 常驻 adapter，并在初始化时全部提交。
- `VirtIONetRaw` 私有持有 transport 和两条 `VirtQueue`，只在构造时协商/建队列；Drop 的 `queue_unset` 与 MMIO transport Drop 不是有 deadline 的运行时恢复 API。MMIO `queue_unset` 还会无界等待 `QueueReady=0`。
- queue task lifecycle 当前单向 `Polling → Spawned → Active → Faulted`，fatal 后 future 结束且 async owner 不回退；queue event 的 wrapping generation 仅用于 lost-wakeup，不是 reset epoch。
- `TicketTracker` 只有 `Queued/DeviceOwned` live state；flush future drop 已正确做到 waiter-only cancellation。
- `SocketSetWrapper::global_terminal` first-wins 且 boot-lifetime，不可能在旧 socket 保持 terminal 的同时让恢复后的新 socket工作。
- VirtIO-net ISR ack used/config cause，但只为 used-ring 发布 queue event；driver 协商 `STATUS` 却只在初始化打印 link status。MMIO/PCI header 都有 config generation 字段，但 transport contract 未暴露一致快照。
- 本机 QEMU 7.0.0 HMP 已确认支持 `set_link name on|off`；项目仍按 R44 使用手工 QEMU 网络运行时，不把 validator 变成 QEMU runner。

2026-08-28 新鲜 host 基线：`axdriver_net` 7/7、`virtio-drivers --features alloc` 36/36、`axdriver_virtio --features net` 16/16、axnet ordinary 371/371、axnet `qemu-diagnostics` 393/393，均 exit 0。`make host-test` 在完成 early-console 6/6、memtrack 8/8、MS03 33/33、MS04 Rust 16/16、C decision 10/10 与 non-socket stimulus self-test 后，因 sandbox 禁止创建 UDP socket在 MS04 loopback self-test 返回 `EPERM`；这是 K43 环境限制，不是产品断言失败。

约束来自 M36/D20（ISR、queue task、stack runner、readiness 分层）、M41/D22（transport-neutral queue contract，QEMU 首个后端为 VirtIO-MMIO）、K09（只用现有 executor/AtomicWaker）、M39（单 hart 不提供 SMP 证据）和 R53（cause snapshot 不替代 descriptor/cookie ledger；不安全停止时保留 DMA backing）。

## Goals / Non-Goals

**Goals:**

- 在唯一常驻 queue owner 内实现可测试的 quiesce/reset/reinitialize 生命周期。
- 以 checked queue epoch 和完整 owner ledger 阻止 reset 前后对象混用。
- 固定 waiter、pre-submit、device-owned 三层取消及六阶段 deadline 语义。
- 按 VirtIO 规范确认整设备停止后才关闭旧 owner并复用 backing。
- 将 config-change/link state 与 descriptor completion 分离，提供真实 QEMU link flap Gate。
- 让旧 socket 稳定失败、新 socket 在恢复或 link-up 后工作。
- 保持 MS01/MS04/MS05/MS06 的单 owner、quiet path、C4 和 readiness 基线。

**Non-Goals:**

- SMP、queue affinity、跨 hart ordering、multiqueue、RSS。
- PCI/DWMAC 产品实现、真板 reset/DMA 停止证明或自动 polling fallback。
- 保留旧 TCP 连接、透明重发 reset/link-down 前的 packet、peer delivery guarantee。
- 自动化控制 QEMU 进程、性能调优、长稳或 fault recovery retry policy。
- 修改外部 `axdriver_base::DevError`；恢复错误在本地类型中表达并在现有边界映射。

## Decisions

### D1：区分三种代次，而不复用现有 wake generation

**Decision**：引入 checked `u64` `QueueEpoch`、`SocketEpoch` 和既有/新增 `LinkGeneration`。`QueueEpoch` 只在成功整设备 reset 后推进并进入 `TxCookie { epoch, ticket }` 与 driver ledger；`SocketEpoch` 在 reset 开始或 link-down 关闭当前应用会话，恢复/link-up 后推进并允许新 socket；`LinkGeneration` 每次一致 link snapshot 变化时推进。任何计数器耗尽都进入 `Faulted`，不 wrapping。

**Reason**：当前 queue/stack generation 都允许 wrapping且只承担 wake coalescing；把它们当 owner identity 会令旧 completion 误命中新对象。link flap 不重建设备 queue，因此也不能错误推进 QueueEpoch。

**Impact**：`TxCookie` 的公共构造/读取契约变化；ticket、adapter slot、telemetry、socket registry和测试 fixture 必须携带正确代次。

**Alternatives**：只使用一个全局 generation 会混淆 wake、queue reset、link 和 socket lifetime；把 epoch 打包进现有裸 `u64` 会隐藏溢出和类型错误，均拒绝。

### D2：axnet 管策略/deadline，driver 执行有界 recovery step

**Decision**：在 `axdriver_net` 增加 transport-neutral `NetRecoveryControl` accessor 和数据类型，至少表达 link snapshot、recovery stage/progress、queue epoch、old-owner ledger 摘要及 `begin_recovery`/`poll_recovery_step`。每次 driver step 必须有界且同步返回 `Pending(stage)`、`Recovered(new_epoch)` 或 `Faulted(stage, DevError)`；它不得自旋到设备响应。axnet queue owner持有状态、clock/deadline、触发原因和 socket/ticket policy，并在每次 poll 内只调用有界 step。

**Reason**：deadline、task wake、socket error 是 axnet policy；status/queue/backing 的安全顺序只有具体 driver 知道。该拆分保持 transport token 不泄漏到 axnet，也避免 guard 跨 await。

**Impact**：VirtIO adapter 实现新 accessor；其他 NIC 默认返回 `None/Unsupported`，不需要 MS07 产品实现。Router/Device/Service 只转发 recovery contract，不接触 MMIO/token。

**Alternatives**：把 reset 全放在 kernel ioctl 会绕过唯一 owner；把 deadline 放进 transport 会耦合 executor/time；直接扩张 `NetQueueControl` 会把 notification 和 lifecycle 两种职责混在一起，均拒绝。

### D3：VirtIO 整设备 reset 是分步事务

**Decision**：VirtIO path 按以下顺序推进，每次 poll 至多完成有界寄存器/ledger 工作：

1. `Quiescing`：停止新 submit，取消所有 Queued tickets；在 1 秒内继续 reclaim 已完成 TX，并冻结一份可证明完整的 remaining DeviceOwned ledger。quiesce 不要求自然 drain 到零，但必须证明每个在途 buffer/descriptor 的唯一 owner；无法建立该边界时直接 Faulted，不能用 reset 掩盖未知 owner。
2. `Resetting`：隔离旧 queue epoch并写 status 0；最多 2 秒反复由 task wake/poll 检查 status 是否读回 0，不调用无界 `queue_unset`。
3. status=0 后：设备已不可访问旧 queue，旧 epoch remaining TX 以 `ResetAborted` 终结；RX/TX buffers、descriptor memory 才能作为 driver-owned 资源移动或重建。
4. `Reinitializing`：最多 2 秒内重新协商 feature、建立 RX/TX queue、填充全部 RX backing、重臂通知并设置 `DRIVER_OK`；所有检查通过后提交新 QueueEpoch 和 Active。
5. reset confirmation 失败时，adapter 连同 transport、queues、buffers 和 cookies 整体留在 quarantined Faulted owner；不得 Drop 或复用 backing。

submit wait、completion wait 和 reclaim 各使用 1 秒 deadline。submit timeout取消 Queued ticket并触发当前 socket error；completion/reclaim timeout在ledger仍完整时触发上述 recovery。ownership mismatch、duplicate/unknown token和quiesce无法冻结完整ledger都直接 stable Faulted，不尝试用 reset掩盖已破坏的账本。

**Reason**：VirtIO 规定 status 读回 0 才证明 reset 完成；在此之前不能判断在途请求结果。Quiesce timeout 可以由 reset安全截断，但 reset timeout 没有安全释放依据。

**Impact**：`VirtIONetRaw`/adapter 必须支持在保留所有权的情况下分步移出和重建 transport/queues；MMIO Drop 和 `queue_unset` 不再被当作运行时恢复实现。现有普通 Drop 兼容语义保持，但 MS07 recovery path不依赖它证明安全。

**Alternatives**：协商 `VIRTIO_F_RING_RESET` 不适用于当前 feature baseline；立即 Drop/recreate 无法在 reset failure 时保留 DMA backing；unbounded spin 违反 timeout/quiet path，均拒绝。

### D4：ticket ledger 记录终结原因，flush 不把取消当成功

**Decision**：live ticket identity 变为 `(QueueEpoch, ticket)`，至少有 `Queued` 和 `DeviceOwned`；终结操作区分 `Reclaimed`、`CancelledPreSubmit`、`ResetAborted` 和 `Fault(stage)`。正常 completion 仅允许 `DeviceOwned → Reclaimed`。恢复开始批量取消 Queued；status=0 后批量关闭 remaining DeviceOwned 为 ResetAborted。任何非-Reclaimed target 都令当前 flush 返回稳定错误，future drop仍只清 waiter。

**Reason**：从 live set 删除 ticket 而不记录原因会让 flush 把 packet loss误判为 C4 成功；device-owned 又不能在 reset 确认前移除。

**Impact**：固定容量 tracker 增加有界 outcome/计数或当前 epoch terminal summary；不要求保存无界历史。QEMU snapshot/validator应观察各 outcome 计数和 live owner 守恒。

**Alternatives**：reset 后把所有 ticket 当 reclaimed 会违反 completion/C4；自动重发 Queued 会改变用户已批准的取消语义，均拒绝。

### D5：恢复错误使用本地结构化类型并稳定映射

**Decision**：在 axnet/driver recovery contract 中使用 `RecoveryFault { stage, cause, queue_epoch, owner_summary }` 或等价有界结构；底层 `cause` 保留现有 `DevError`。应用边界使用扩展的本地 `NetworkTerminal` 编码：reset/old epoch → `AxError::ConnectionReset`，link down → `AxError::NotConnected`，deadline → `AxError::TimedOut`，诊断取消 → `AxError::Interrupted`，ownership invariant → `AxError::BadState`，其他 device I/O → `AxError::Io`。readiness 和紧随 I/O 必须用同一 terminal identity。

**Reason**：外部 `DevError` 没有 reset/link/timeout/cancel variants；全部压成 `BadState` 会丢失 MS07 的诊断边界。`axerrno` 已有对应应用错误，无需修改外部 crate。

**Impact**：`readiness` encoding、TCP/UDP/listener guard、flush/ioctl mapping和 telemetry需要接受 `NetworkTerminal`，并保留既有 DevError 映射兼容测试。

**Alternatives**：patch `axdriver_base` 会扩大跨项目 API；只打印 stage 会使 waiter 看不到稳定类型，均拒绝。

### D6：link flap 关闭 socket epoch但不 reset queue epoch

**Decision**：ISR 对 used-ring 和 config-change 分别发布位于同一 AtomicWaker/recheck协议下的 event flags。task context 通过 transport `config_generation before → status → config_generation after` 单次尝试读取一致 snapshot；不一致返回 `Again` 并 self-wake，不在一个 poll 内无界循环。确认 link down 后关闭当前 SocketEpoch、取消 Queued tickets、阻止 software enqueue 和 driver submit，返回 `NotConnected`；旧 DeviceOwned 仍由同一 QueueEpoch completion/reclaim以便释放资源。link up只推进 SocketEpoch、重新检查/武装 queue和stack，允许新 socket；不自动 reset设备，也不恢复旧 socket。

**Reason**：VirtIO TX completion 只证明 buffer 可复用，link down 时仍可能 completion但不能证明 peer delivery。socket epoch closure消除静默接受，同时保持不需要重建设备的 link 管理独立成果。

**Impact**：IRQ event模型、raw driver link accessor、Service enqueue preflight、socket registry和 QEMU marker更新。config-only IRQ仍不搬 descriptor。

**Alternatives**：link down 时继续接受发送会静默丢包；每次 link flap 整设备 reset耦合两个独立验收域；旧 TCP 自动恢复没有协议依据，均拒绝。

### D7：Socket registry 采用 epoch-scoped terminal

**Decision**：`SocketSetWrapper` 保存当前 SocketEpoch及其 open/closed terminal；每个 public handle/bridge记录创建 epoch。关闭 epoch时先提交 terminal，再 snapshot/wake该 epoch所有 bridge。新 epoch开放后，新 handle不继承旧 terminal；旧 handle每次 readiness/I/O仍通过自身 epoch记录返回原 terminal。listener hidden sockets和 deferred raw sockets归属创建它们的 public/session epoch，关闭时恰好清理一次。

**Reason**：清空当前 global atomic 会令旧 socket错误消失；保留它又会污染新 socket。epoch绑定同时满足两边。

**Impact**：wrapper、TCP、UDP、listener、deferred retirement和 stack runner fault publication需要从 boot-global改为 epoch closure。锁序仍为 `SERVICE → SOCKET_SET → ListenTable entry`，guard不跨 wake。

**Alternatives**：重建整个全局 SocketSet 会破坏 handle/lock责任并扩大迁移；复用 boot-global terminal无法恢复，均拒绝。

### D8：model 与真实 QEMU 的证明职责分开

**Decision**：fake transport/adapter/axnet clock tests负责 stale/duplicate completion、status不归零、config generation变化、阶段 deadline和 cancel/submit交错；真实 QEMU负责规范内可观察的 status reset/reinitialize、真实 config IRQ/link off-on和 reset前后流量。QEMU-only control新增 versioned recovery command/snapshot，不修改已有 V1-V3 ABI；validator保持纯输出审计，HMP `set_link`由手工 runbook步骤执行。

**Reason**：合规设备 reset 后不应访问旧 queue，不能要求真实 QEMU制造非法 stale completion；同时 model不能替代真实 MMIO reset和 config IRQ。

**Impact**：host fixtures、QEMU probe/validator、kernel ioctl和手工命令都需要版本化 marker。Act Response足以保存可复现结果，因此默认不创建 change Evidence。

**Alternatives**：通过破坏 QEMU ring制造 stale completion既不稳定也不代表规范行为；让 validator启动 QEMU违反 R44，均拒绝。

## State and Ownership Model

| State | New enqueue/submit | Completion/reclaim | Backing disposition | Socket behavior |
|---|---|---|---|---|
| Active/link-up | allowed | normal epoch match | normal ledger | current epoch usable |
| Active/link-down | rejected | old device-owned may close | queue epoch unchanged | old epoch `NotConnected`; new disabled |
| Quiescing | rejected; queued cancelled | bounded drain | all device-owned retained | current epoch terminal |
| Resetting | rejected | observed event only; no success attribution after isolation | all old backing retained | old epoch terminal |
| Reinitializing | rejected | old epoch stale only | old backing is driver-owned after status=0; rebuild in progress | new epoch not yet open |
| Faulted before reset confirm | rejected | no ordinary progress | entire uncertain owner quarantined | stable mapped error |
| Active after recovery | allowed for new socket epoch | new QueueEpoch only | rebuilt ledger | old handles terminal, new handles usable |

## Implementation Order

1. 建立 transport 的 bounded reset/config primitives，再定义 transport-neutral recovery contract。
2. 让 VirtIO adapter独立通过 epoch ledger、reset success/failure和 link snapshot model tests。
3. 扩展 axnet ticket与唯一 owner lifecycle，接入 staged clock/deadline、quiesce和 driver step。
4. 改造 IRQ config event、link policy和 socket epoch terminal。
5. 补齐 versioned QEMU control/probe/validator，最后执行 host、build、single-hart runtime和 MS01/MS04/MS05/MS06回归。

该顺序保证每层先有可独立验证的下层 contract，避免在 QEMU 调试时同时猜测 transport、ledger和 socket语义。

## Risks / Trade-offs

- [Reset state中移动 generic transport/queues 易产生提前 Drop] → recovery holder必须拥有完整旧对象；reset确认失败路径用 model test证明 Drop/allocator均未发生。
- [1 秒 data-stage deadline可能在异常慢的调试环境误触发] → 仅承诺当前 single-hart QEMU baseline，deadline常量集中且 telemetry暴露；性能/真板另行定标。
- [link down与已入设备 packet之间仍无法证明 peer delivery] → 关闭 socket epoch并只把 completion计作资源回收，绝不声明发送成功或自动重发。
- [socket epoch改造影响 listener/deferred cleanup] → 单独 Iteration覆盖 TCP/UDP/listener、多 waiter、handle reuse和deferred retirement，再进入 QEMU。
- [reset成功但 reinitialize失败] → 设备已停止后 backing可安全由driver持有，但数据面保持 Faulted；本 change不自动重试。
- [sandbox禁止 loopback self-test] → Act先尝试完整 Gate；若仍是精确 `EPERM`，记录环境分层并运行全部无 socket子 Gate，任何编译/断言/validator失败仍阻塞。
- [真实 QEMU手工步骤的操作时序] → versioned marker明确何时执行 HMP off/on和何时发 reset ioctl；validator拒绝缺失、乱序或旧 revision输出。

## Migration Plan

本 change按四个逻辑 Iteration增量迁移。每个 Iteration必须保持编译和已完成 baseline：先让未实现 recovery的其他 driver返回 `Unsupported`；再让 VirtIO adapter具备 isolated model能力；随后切换唯一 queue owner和 socket registry；最后才暴露 QEMU control。任一 Iteration无法闭合时，停止在此前 stable baseline，不以兼容 shim启动第二 owner。

回滚仅允许回到最后 accepted Iteration对应代码状态；已经进入新 queue/socket epoch的运行中系统不支持动态降级，需重新启动 guest。OpenSpec Act不得使用破坏性 git回滚覆盖用户工作树。

## Open Questions

无实质开放问题。局部类型名、helper拆分、telemetry字段排列和有限状态机内部表示可由 Act在不改变上述接口责任、状态转换、错误映射与验收语义的前提下决定。
