## Context

MS04 已把单个 QEMU VirtIO-MMIO NIC 的 RX descriptor service 收口到一个 IRQ 唤醒的
task，但边界仍然不对称：queue task 直接把 RX completion 交给 Router，TX 则由
caller-driven `Service` 同步分配 buffer、提交 descriptor，并在下一次发送时顺便回收。
MS05 要把硬件两条 queue 的 ownership 收到同一个长驻 queue service，同时继续让
Ethernet、ARP、IP 和 smoltcp 在现有 caller-driven 上下文中运行。

当前实现调查得到以下约束和缺口：

- `NetQueueControl` 只能观察和控制 RX used notification；共享 VirtIO IRQ 也无法在 ISR
  中可靠区分 RX/TX completion。
- `NetDriverOps::recycle_tx_buffers()` 一次无界回收所有 completion，且不返回与提交对应的
  transport-neutral identity，无法实现独立 reclaim budget 和乱序安全的 C4 flush。
- VirtIO TX submit 取得 buffer 后若 oversize 或 `transmit_begin` 失败，buffer 会落回
  `NetBufPool`，但不会回到驱动实际使用的 `free_tx_bufs`，重复错误会静默缩小容量。
  `QueueFull` 当前还被映射为 `BadState`。
- `can_transmit()` 要求至少两个 descriptor，但当前连续 TX buffer 实际只提交一个
  descriptor；readiness 与下一次真实 submit 不一致。
- `VirtQueue::should_notify()` 只用当前 `avail_idx >= avail_event + 1`，没有 old/new
  wrapping event 语义，跨 `u16` 回绕时不能作为正确的 TX device-notify 判定。
- Router 在 `dispatch()` 中先 dequeue 再发送，`Device::send()` 的 `bool` 又混合了 TX
  disposition 与 loopback RX-ready hint。Ethernet/ARP 的 Full 和 driver error 目前只打印
  warning，不能保留上游 packet。
- smoltcp `PacketBuffer` 的 payload ring 会因可变长度 packet 的连续窗口和 padding 提前
  Full，不能保证“64 个任意合法最大 frame”这一精确容量，因此不能复用为最终 frame
  slots。
- MS04 queue task 在持有全局 `Service` mutex 时执行单步 RX，但不会让 guard 跨越
  `Pending`。MS05 继续使用这一唯一 NIC handle 和锁边界，不能建立第二个 raw driver
  handle。

2026-08-12 在当前 revision `3e181464fc76b562a5c4e7e8dd7bb27313fa8a11` 运行的新鲜
基线如下：

| Command | Result |
|---|---|
| `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib -- --nocapture` | PASS，34 tests |
| `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --nocapture` | PASS，109 tests |
| `cargo check --manifest-path crates/axdriver_net/Cargo.toml --offline` | PASS |
| `cargo check --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net` | PASS |
| `cargo test --manifest-path crates/uart_16550/Cargo.toml --offline --features async` | PASS，62 unit + 18 doctests |
| `cargo check --offline -p starry-kernel --features qemu` | PASS |
| `cc -std=c11 -Wall -Wextra -Werror -fsyntax-only tests/ms03_irq_probe.c tests/ms04_rx_probe.c` | PASS |
| `python3 scripts/ms04_rx_stimulus.py --self-test` | PASS，96-packet protocol/sequence/bounds/malformed |
| `make LOG=info build` | PASS，正式 release build 和 binary 生成退出 0；前置工具探测曾因只读 Cargo metadata 和禁止联网安装报错，但未阻止已有本地工具完成产品构建 |
| `make host-test` | ENV-BLOCKED；前置 6+8+26+15 Rust tests、10 C decision tests 和 protocol self-test 已通过，最后的 UDP loopback socket 创建被 sandbox 以 `EPERM` 拒绝 |

该基线只说明当前 MS04 行为可复现，不构成任何 MS05 场景的实现证据。QEMU 运行证据仍须
来自本 change 的新产物和 change-local Evidence。

## Goals / Non-Goals

**Goals:**

- 为每个方向提供恰好 64 个预分配、packet-atomic 的 Ethernet frame slots，并让 stack
  side 永远不观察 raw token、descriptor 或 `NetBufPtr`。
- 用一个 transport-neutral、direction-aware contract 表达 RX/TX completion、通知控制、
  单步 submit/reclaim 和可恢复压力。
- 让同一个长驻 queue service 成为目标 NIC 两条 hardware queue 的唯一 owner，以独立
  budget 公平推进 TX reclaim、RX reap/refill 和 TX submit。
- 用 typed handoff 让 Router、ARP pending 和 loopback 区分 Accepted、Full、Dropped、
  fatal 与 RX-ready hint，并在 Full 时保留 packet。
- 以 ticket 集合定义内部 C4 flush，不依赖 queue empty、最大 completion 序号或有序完成。
- 保留 MS04 的最小 ISR、register-recheck、EVENT_IDX、V1/V2 ABI 与核心 QEMU 判定，并增加
  可确定复现的 slot/descriptor Full→恢复、flush 和网络功能证据。

**Non-Goals:**

- 不创建独立 smoltcp stack runner，不把 hardware/slot capacity 精确映射为 fd
  `POLLOUT` 或 `EAGAIN`；这些属于 MS06。
- 不实现 reset/recovery owner 切换、SMP queue ownership、多队列 RSS、PCI、DWMAC 产品
  driver、真板 DMA/cache 验证或性能资格。
- 不扩建 I16 通用 `network_benchmark`，也不把 C4 flush 解释为 peer、TCP ACK 或应用完成。
- 不原地修改 Cargo registry，不关闭 `RING_EVENT_IDX`，不借本 change 清理无关 warning
  或 vendor formatting。

## Decisions

### D1. 用 direction mask 和 opaque completion cookie 演进公共 queue contract

**Decision**

`NetQueueControl` 演进为按 `Rx`/`Tx` direction mask 执行以下原子操作：观察 completion、
抑制 used notification、重臂 notification 并在 barrier 后返回仍 pending 的 direction。
共享 IRQ 只发布通用 queue event，task 再通过该接口分类工作。

`NetDriverOps` 或一个由它提供的等效 typed queue-service 接口增加单步 TX 操作：submit
接收一个由上层生成的 opaque completion cookie，reclaim 每次最多返回一个完成 cookie。
cookie 只标识上层 ticket，不是 VirtIO descriptor token；VirtIO adapter 在内部维护
`descriptor token → cookie → NetBuf` 的一一对应。公共 API 不返回 ring index、MMIO 字段
或 transport token。

接口的 ownership 规则固定为：

- submit 成功后 driver/device 拥有 buffer；对应 frame slot 才能 commit dequeue。
- submit 在 transport 接受前返回 `Again` 或其他错误时，driver 必须已把准备中的 buffer 恢复
  到可分配集合，queue service 仍拥有 slot 队首。若 transport 已接受 buffer 后才发现 token
  range、slot collision 或 ledger mismatch，driver 必须把新 buffer 保留在不可复用的内部
  fault owner 中、记录稳定 fatal 并停止后续 submit；此路径不得 panic，也不得谎报 buffer 已
  恢复。queue service 保留逻辑 slot 并进入 fault，不再重试该 packet。
- reclaim 只有在匹配 token 完成、同一 buffer 已回到可用集合后才返回 cookie。
- `NoMemory` 只描述不可恢复的初始化分配失败；运行期 queue/buffer exhaustion 使用
  `Again`。oversize 是稳定 policy/error，不是压力。

非目标 NIC implementor 通过明确的 `Unsupported` 默认路径保持编译兼容；MS05 只为
VirtIO-MMIO 绑定双向实现。DWMAC 模型审查的映射是：CPU-owned descriptor 表示对应方向
completion 可见，channel RX/TX interrupt mask 表示 suppress/arm，barrier 后重读 channel
status/descriptor ownership 表示 arm-and-check。该映射不需要暴露任何 VirtIO 或 DWMAC
布局，因此公共 contract 足以承载后续真板实现。

**Reason**

无 completion identity 就无法对乱序 TX 做 C4 flush；直接公开 VirtIO token 又会把 packet
slot 和上层服务绑定到 transport。单步 reclaim 同时给 fixed budget 和故障定位提供自然
边界。

**Impact**

`axdriver_net`、VirtIO adapter、fake drivers 和 enum-dispatch implementor 都要编译迁移。
VirtIO adapter 的 legacy submit 与 queue submit 必须使用带 owner tag 的同一 ledger，避免
一条路径覆盖另一条路径。第一轮必须先用真实 adapter fixture 和 ownership model tests 固定
错误后的 buffer 守恒，再允许 axnet 使用新接口。`QueueFull → Again` 是 net TX pressure
语义；共享错误转换函数的 vsock 等其他调用者不得在没有独立 requirement 和测试时改变。

**Alternatives**

- 保留 `recycle_tx_buffers()` 的无返回、全量 drain：无法关联 ticket，也无法限制 budget，
  拒绝。
- 把 VirtIO token放进 frame slot：违反 transport-neutral 边界，拒绝。
- 假定 used completion 与 submit 同序，只维护最大完成 ticket：VirtIO/API 不提供该上层
  保证，乱序会使 flush 提前成功，拒绝。

### D2. 使用专用固定 slot ring，不复用 smoltcp `PacketBuffer`

**Decision**

在 `EthernetDevice` 内为 RX/TX 各创建一个专用 `FixedFrameQueue<64>`。初始化时一次性分配
64 个固定 backing slots；每个 slot 容纳当前支持的最大普通 Ethernet frame
`14 + 1500 = 1514` bytes、实际长度和最少 metadata。TX slot metadata 包含 acceptance
ticket，RX slot 不包含 transport 状态。

backing storage 必须在 heap 上直接构造并由 `Box` 或等价固定所有权容器持有，禁止在内核
栈上先物化约 194 KiB 的双向数组再移动进 `EthernetDevice`。初始化期允许固定次数分配；完成
初始化后不得扩容或按 packet 分配。ARP pending 与 loopback 若需要精确、无副作用的容量
preflight，复用同一 fixed-frame storage 机制，但使用各自既有逻辑容量。

queue 以 head/tail/occupancy 管理，每次 enqueue 先检查完整长度和空 slot，再复制整 frame
并一次 commit；dequeue 先 peek，consumer 成功后再 commit。初始化后不扩容、不分配 packet
buffer。oversize 返回稳定 `Dropped(FrameTooLarge)` 或 RX drop/fault policy，不能占用半个
slot。

slot 和 raw driver 继续受现有 `Service` mutex 保护。锁提供 packet metadata 与 ticket
状态的同步；occupancy/high-water/drop telemetry 是纯观测，可使用 Relaxed atomic。任何
软件事件都在状态 commit 后发布，guard 不跨 `Pending` 或 yield。

本结构在 stack-handoff iteration 中先以 dormant slot mode 完成 host tests，产品默认仍走
现有同步 polling TX/RX。只有 D4 的双向 activation preflight 成功后，后续 queue-service
iteration 才能一次性启用 RX/TX slot mode；不得直接读取 MS04 的 RX-only `Active` 并提前
切换 TX。

**Reason**

smoltcp `PacketBuffer` 的 payload ring 可因 wrap/padding 在 metadata 尚有空位时提前 Full，
不能证明 64 个最大 frame 的精确容量。专用 fixed slots 让 Full 边界、内存上界和 packet
atomicity 都可直接测试。

**Impact**

每个目标 NIC 的 frame backing 至少占用 `2 × 64 × 1514 = 193,792` bytes，另加少量
metadata。数据面在 driver DMA buffer 与 slot 间各有一次显式 copy；MS05 记录计数但不作
性能资格声明。

**Alternatives**

- 将现有 Router/ARP `PacketBuffer` 扩到 64：仍有连续窗口问题，也会混淆 L2/L3
  ownership，拒绝。
- slot 直接持有 `NetBufPtr`：会让 descriptor buffer 跨 task 边界和 `Pending`，拒绝。
- 每包 `Vec<u8>`：隐藏压力并破坏固定内存上界，拒绝。

### D3. Router 使用无副作用 preflight 加 typed commit

**Decision**

设备 TX 结果使用 `Accepted { rx_became_ready }`、`Full`、
`Dropped(TxDropReason)` 和 `Fault(DevError)`。loopback 只在接受 packet 且本地 RX 变为可读
时设置 `rx_became_ready=true`；Ethernet 普通接受始终为 false。

Router 改为 peek→plan/preflight→commit：

1. 先解析队首和路由，得到一个或多个目标设备。
2. 在同一个 `Service` guard 内对所有目标执行 packet-side-effect-free capacity preflight。
   preflight 可以回收已完成的同步 TX buffer，但不得发送 frame、占用 slot/pending entry、
   更新 neighbor、增加 drop counter 或消费 Router packet。
3. 任一目标 Full 时不调用任何 send、不 dequeue，并停止本轮 dispatch。
4. 所有目标 ready 后逐一 commit；全部 Accepted 或明确 Dropped 后才 dequeue。
5. preflight 后若仍出现不可能的 Full、ownership error 或中途 fatal，分类为 invariant
   fault，停止数据面并移除该 Router 队首，防止后续重试复制已经交付给先前目标的 packet。

Device contract 分开表达 `TxPreflight::{Ready, Full, Dropped(reason), Fault(error)}` 与
`TxOutcome::{Accepted { rx_became_ready }, Full, Dropped(reason), Fault(error)}`。稳定 drop
reason 至少区分 malformed IP、missing route、route-source mismatch、unsupported address
family 和 frame too large。Router 对每个逻辑 delivery disposition 只增加一次 reason counter；
preflight 为 Ready 后 commit 返回任何非 Accepted 结果都属于 invariant drift。

该两阶段规则覆盖 IPv4 broadcast、IPv6 multicast 和 loopback+Ethernet fanout。因为
queue task 与 stack side 使用同一个 mutex，preflight 和 commit 之间不存在 slot
consumer 或另一个 producer，capacity 结论在正常路径上稳定。无路由、明确不支持的网络
协议、malformed packet 和 frame oversize 使用固定 `TxDropReason`；普通 Full 不计 drop。

`smoltcp::PacketBuffer::is_full()` 只反映 metadata ring，不能证明下一个可变长度 packet 有
连续 payload window。Loopback 与 ARP pending 因而不能用 `is_full()` 充当 exact preflight；
它们迁移到 fixed-frame queue 后，preflight 才能对给定长度返回稳定 Ready/Full。

ARP 路径按一次最多生成一个 L2 frame 的事务拆分：

- outbound unknown neighbor 先同时 preflight 一个 TX slot 和一个 ARP pending entry；随后
  enqueue ARP request、记录 `neighbor=None`、commit pending IP。任一步若违背 preflight
  是 invariant fault，不能以 Full 重试造成重复 ARP。
- RX ARP request 在 TX slot Full 时保留 RX slot 队首；reply 被 TX slot 接受后才更新
  neighbor 并消费该 RX frame。
- RX ARP reply 不需要立即 TX，可更新 neighbor 并消费 RX frame。ARP pending flush 在
  每次 caller-driven poll 中独立使用 peek→typed send→commit，Full 时保留 pending 队首。
- expired neighbor 的新 ARP request 也只有在 request frame Accepted 后才更新状态；固定
  head-of-line 行为保留并计数，本 change 不做 pending reordering。

**Reason**

单个设备的 peek→commit 能保留 Full packet，但广播逐个发送会在后一个设备 Full 时留下
部分交付；直接重试会复制前面已经接受的 packet。锁内全目标 preflight 是当前单 Service
架构下最小且可证明的原子 fanout 方案。

**Impact**

`Device` trait、Loopback/Ethernet、Router tests 和所有 fake implementor 都要迁移。现有 TCP
short write 和 UDP datagram atomicity 不改；typed Full 仅在内部 Router/device 边界可见，
不承诺直接成为 fd readiness。

**Alternatives**

- 为每个 fanout packet 维护已发送 device bitmap：状态与 Router buffer 生命周期耦合，
  对 MS05 过重，拒绝。
- 广播/组播继续 best-effort drop：不满足已批准的 Full 保留语义，拒绝。
- preflight 后把意外 Full 当普通 Full：会复制部分 fanout，拒绝。

### D4. 复用一个 NIC handle，并执行全有或全无的双向 owner 切换

**Decision**

MS04 的 lifecycle 演进为双向 data-plane lifecycle，保持既有数值和 V2 snapshot 字段可
解释：`Polling → Spawned → Active → Faulted/Unavailable`。新的 queue task 仍通过全局
`Service` 中保存的 target device index 访问同一个 `EthernetDevice`；不移动、复制或
downcast raw driver，也不保存第二个 NIC handle。

激活顺序是：构造两组 slots 和 ticket state；确认 target/driver；对 RX/TX queue control
执行 preflight suppress；注册 task 与 event；最后一次原子状态转换将两条 queue 的 owner
同时切为 `Active`。在该转换前，普通 Router RX 与同步 TX fallback 仍是唯一 owner；转换后
它们只能访问 slots。不存在公开的 RX-active/TX-polling 半状态。

stack-handoff iteration 只准备并测试 dormant slot mode，不执行上述转换。当前 MS04
`RX_LIFECYCLE::Active` 只代表 RX descriptor owner，不能作为启用 TX slots 的条件。双向
lifecycle iteration 必须在同一个 `Service` guard 内完成 queue preflight、local slot-mode
切换与 lifecycle publication；任一步失败时保持 polling fallback，不能留下半激活设备。

激活后的 queue fatal 进入 `Faulted` 并保留双向 ownership，唤醒 flush/stack progress
waiter，停止无界 retry；不回退到 10ms descriptor polling。`Unavailable` 只允许在 owner
切换前发布。

**Reason**

把 driver 移出 `EthernetDevice` 会同时破坏 ARP/MAC state 和现有 enum-dispatch；建立第二
handle 又会造成 descriptor double-owner。复用 MS04 已验证的 target-index seam 可以在
最小改动下延伸为双向 owner。

**Impact**

caller-driven `Service` 仍可能与 queue task竞争同一 mutex，但每次 task poll 工作有界，
guard 不跨 await。Faulted 状态会停止目标 NIC 数据面，这是比隐式双 owner 更安全、也更
容易诊断的失败方式。

**Alternatives**

- RX task 与独立 TX task：共享 IRQ、driver 和锁，增加跨 task ordering 与 flush 同步，
  拒绝。
- 激活后 fatal 自动回退同步路径：无法证明旧 task 已完全放弃 token/buffer，拒绝。
- 在初始化中先切 RX、后切 TX：暴露半激活状态，拒绝。

### D5. 一个事件代次、两个 waker role 关闭硬件与 slot 的 lost-wakeup 窗口

**Decision**

现有 RX notify 演进为通用 `QueueEvent`：一个 wrapping generation、queue-owner waker、
stack-progress waker 和有界 event-kind telemetry。事件来源包括共享 used-ring IRQ、stack
enqueue TX、stack 释放满 RX slot、queue enqueue RX、queue 释放满 TX slot、software
nudge 和测试控制释放。发布先 commit 状态，再以 Release 推进 generation，并按事件方向
唤醒需要的 role；task 以 Acquire 观察 generation。纯 counter 使用 Relaxed。

queue task 返回 `Pending` 前固定执行：

1. 读取 generation 并检查 TX completion、RX completion、RX capacity 和 TX backlog。
2. 注册 queue-owner waker。
3. 分别重臂 RX/TX used notification。
4. 重新检查两个 completion mask、两个 slot 条件和 generation。
5. 只有全部稳定为空才返回 `Pending`；否则继续或 self-wake/yield。

stack socket waker 通过 stack-progress role 注册。RX slot empty→nonempty、TX slot
full→nonfull 或 fatal 会唤醒它，使现有 caller-driven future 有机会再次执行
`poll_interfaces()`；最终可读/可写判断仍由 smoltcp/socket state 决定，不能把该 hint
描述为精确 fd readiness。硬件事件可以有界地同时唤醒两个 role，spurious task poll 必须
一次检查后重新等待。

**Reason**

只有 queue-task waker 时，TX slot 释放不能唤醒因 smoltcp send buffer 满而等待的 caller；
只有 socket waker又不能推进 descriptor。共享 generation 加两个角色既复用一个事件
ordering，又不覆盖彼此 waker。

**Impact**

需要确定性交错测试覆盖 event-before-register、register-during-event、单方向 arm 后另一
方向到达、slot Full→space、generation wrap 和 cancellation。`AtomicWaker` critical-section
继续沿用 MS04 的 IRQ state restore 实现，并重跑 UART tests。

**Alternatives**

- 继续依赖 10ms fallback：Active owner 下普通路径不能碰 descriptor，且不能关闭 lost
  wakeup，拒绝。
- 所有事件共享一个 `AtomicWaker`：queue task 与 socket future 会相互覆盖，拒绝。
- 每方向完全独立 generation：共享 IRQ 仍需额外合并协议，复杂度更高，拒绝。

### D6. 每轮固定为 reclaim→RX→submit，各阶段独立 budget 32

**Decision**

queue service 每次 poll 依次执行：最多 32 个 TX reclaim、最多 32 个 RX
completion/refill、最多 32 个 TX submit。一个阶段 budget 用尽时记录 exhaustion，但仍给
本轮后续阶段自己的 budget，避免固定顺序造成方向饥饿；本轮结束若任一 backlog 仍可见，
发布 self-event、唤醒自身并 yield 至少一次。

- reclaim：每次从 driver 取一个 completion cookie，验证 ticket/buffer state，发布 C4。
- RX：只在 RX slot 有空间时 reap 一个 `NetBufPtr`，复制完整 frame 后立即 refill；slot
  Full 时不碰 used descriptor，recycle 失败为 fatal。
- submit：peek TX slot，分配/准备 driver buffer并携带 ticket cookie submit；成功后才
  dequeue slot。`Again` 保留 slot 并停止本阶段，其他 error 进入稳定 fault。

task 在三个阶段之间不运行 Ethernet/ARP/IP/smoltcp。持续双向 tests 必须观察 RX
delivered/refilled 与 TX submitted/reclaimed 都增长；spurious/nudge 无工作只能形成一个
有界空轮次，不能 self-wake loop。

**Reason**

沿用 MS04 的 RX budget 32 可保留已验证的 burst/yield尺度；三个同样大小的独立 budget
让每轮上界清晰，并保证早期阶段繁忙时后续方向仍得到机会。

**Impact**

task 每轮最多推进 96 个数据面动作，另加固定 recheck。单 hart QEMU 可验证软件公平性，
但该数值不是硬件性能调优结论。

**Alternatives**

- 一个共享 32 budget：reclaim burst 可耗尽全部预算并饿死 RX/TX submit，拒绝。
- 每阶段无界 drain：可能长期持锁并形成软锁死，拒绝。
- submit 优先于 reclaim：放大假 Full 和 buffer pressure，拒绝。

### D7. RX/TX 分别控制 used_event，TX kick 使用规范的 old/new wrapping 公式

**Decision**

VirtIO adapter 为 receive/send queue 分别提供 suppress 和 arm-and-check。已协商
`RING_EVENT_IDX` 时通过各自 `used_event` 控制 device-to-driver completion notification；
非 EVENT_IDX 模式使用对应 flags。不能把 event_idx 模式下 `set_dev_notify` 的 no-op 计为
控制成功。

TX driver-to-device notify 改为标准 wrapping 判定：概念上使用
`(new - event - 1) < (new - old)` 的 `u16` wrapping arithmetic。queue add 路径必须把本批
新增 descriptor 之前和之后的 avail index 传给判定，而不是普通 `>=` 或只保存 new。
RED tests 固定 event 位于窗口外、窗口内、equal boundary 和跨 `u16::MAX` 四类情况。

**Reason**

used_event 与 avail_event 是两个方向不同的通知机制。MS04 只补齐 RX used-event；MS05 若
不同时补 TX used completion 和正确 kick 公式，会在 Full/recovery 中留下 lost wakeup 或
持续多余 notify。

**Impact**

修改工作区内的 `virtio-drivers` snapshot 和 `axdriver_virtio` adapter，并运行完整
virtio queue tests。依赖若无法暴露 send queue 的 used-event 控制，Act 必须停止返回 Plan，
不能穿透私有 raw fields 或关闭 feature。

**Alternatives**

- 关闭 EVENT_IDX：改变已协商功能并掩盖真实路径，拒绝。
- 每次 submit 无条件 notify：可维持功能但违反已批准通知语义，也无法验证 wrap，拒绝。
- 修改 `$CARGO_HOME` registry：不可复现且违反项目规则，拒绝。

### D8. 用最多 128 个 live ticket 的有界集合实现 C4 flush

**Decision**

每个 TX frame 成功进入 slot 时分配一个 checked-increment `u64` ticket。最多同时存在
64 个 slot-queued 和 64 个 device-owned ticket，因此 tracker 固定容纳 128 个 live
records；每个 record 只处于 `Queued` 或 `DeviceOwned`，C4 reclaim 后从 live set 移除。
ticket 到达 `u64::MAX` 而仍需继续分配时进入稳定 fatal，不能静默 wrap/reuse。

flush 捕获调用瞬间 `last_accepted` 为 target。判定条件是 live set 中不存在
`ticket <= target`；target 之后接受的 record 不参与。reclaim cookie 可按任意顺序删除
匹配 record，因此不维护会掩盖 hole 的“最大完成 ticket”。

flush 只允许一个内部 waiter：注册 waker 后在同一 `Service` guard 内重查 live set、fatal
和 target；第二个 waiter立即返回 `ResourceBusy`。future drop 只清除匹配 waiter 的
registration，不修改 live records。submit/reclaim fatal 保存稳定错误并唤醒 waiter。

ARP pending 中尚未形成 Ethernet frame 的 IP packet 不分配 ticket；只有 frame 被 TX slot
Accepted 后才进入 flush target。C4 表示对应 driver buffer 已被 reclaim，可再次安全使用，
不表示 wire、peer、TCP ACK 或 application completion。

**Reason**

slot dequeue 后仍有最多 64 个 device-owned packet，单看 slot/queue empty 会提前完成。
固定 live set 同时覆盖乱序、target 截止和无分配数据路径。

**Impact**

需要 model tests 覆盖 empty、queued+inflight、post-target acceptance、乱序、second waiter、
cancel 和 fatal。flush API 保持 axnet/driver 内部；QEMU diagnostic ioctl 只为本 change 的
运行见证调用它。

**Alternatives**

- 等待 TX slot 和 descriptor queue 同时为空：会被 flush 后的新 traffic 无限延迟，拒绝。
- 只比较最大 reclaimed ticket：乱序 completion 可跨过 hole，拒绝。
- 为每个 flush 分配 waiter list：MS05 没有多 waiter需求并增加同步面，拒绝。

### D9. V3 保留 V1/V2 前缀，QEMU-only lease controls 提供确定性压力

**Decision**

保留现有 `NET_IRQ_SNAPSHOT_V1` 和 `V2` 的 command、size、offset 与写入长度，新增 V3
command。V3 的前 28 个 `u64` 与 V2 完全相同，之后追加：

- 两个 slot 的 occupancy、high-water、Full、enqueue/dequeue 和 space event；
- TX submit/Again/completion/reclaim、available/inflight buffer、descriptor conservation；
- 三阶段 budget/exhaustion/self-yield 与通用 event/wake；
- accepted/live/target ticket、flush success/error/busy/cancel；
- lifecycle fault、ownership invariant 和各稳定 drop reason。

Rust/C 通过 `repr(C)`、size/offset static assertions 和 legacy canary tests固定 ABI。现有
MS04 probe 继续只读 V2；新 MS05 probe 读 V3，R51 重跑仍使用原 V2 consumer，证明向后
兼容而不是只测新解析器。

为确定复现压力，新增 axnet 私有 `qemu-diagnostics` feature，并只由
`starry-kernel/qemu` 传递启用；D1 与普通 axnet build 不启用它。该 feature 提供带最长
2 秒 lease 的 test control：

- `HoldTxSubmit` 暂停 submit stage，让普通 guest network traffic 填满 64 个 TX slots。
- `HoldTxReclaim` 暂停 reclaim stage，让已完成 descriptor 不被 pop，直至实际 submit
  返回 `Again` 并证明 buffer 恢复。
- `Release` 解除 hold、发布通用 event并触发有界恢复；lease 超时自动 release 并记失败
  counter，避免 probe 异常永久停网。
- 内部 flush ioctl 在固定 deadline 内等待 D8 的 C4 future；它不能重置或伪造 counter。

lease 是现有 `Service` 的普通内部状态，至少包含 mode、absolute expiry 和单调的
auto-release failure counter；它不使用独立全局原子事务或 generation token。该选择使
control、queue tick 与 V3 snapshot 都以同一个 Service guard 观察真实已提交状态，不存在
odd generation、synthetic no-hold tuple、ABA 或 terminal generation 中永久 Hold。

diagnostic ioctl 使用真正的 bounded `try_lock` 获取全局 Service：成功时在 guard 内校验并
一次性提交 `{mode, expiry}`，用 `checked_add` 拒绝 deadline overflow，释放 guard 后才发布
queue work；竞争失败立即返回 `ResourceBusy`（syscall 映射 `WouldBlock`），状态与 event 均
不变，probe 只能在固定总 deadline 内有界重试。queue task 已持有 Service guard，因此 tick
在同一 guard 内判断 expiry、清除 lease，并以 saturating counter 精确记录一次自动释放。

lease timer 只保存 absolute deadline 并负责到期唤醒，不清除状态，也不以 generation 判断
所有权。旧 timer 在 Hold 被 Release 或替换后到期，只促成一次有界 Service poll；poll 读取
当前 lease，若新 lease 尚未到期则保持它并重臂其 deadline。显式 Release 同样先在 Service
guard 内提交，再在解锁后发布 queue work。future 返回 `Pending` 前不持有 Service guard。

V3 在既有 `rx_snapshot_v3()` Service guard 内把 lease tuple、auto-release counter 与 slots、
tickets、driver ledger 一次复制；因此成功 snapshot 永远表示真实 committed Service state，
不会因锁竞争伪造 RELEASED。Service 尚未初始化时沿用既有全零 snapshot 语义，该状态必须
与运行期 contention 区分。V1/V2/V3 布局和 ioctl command 不变。

controls 只在 axnet queue service 层暂停正常 owner 的某一阶段，不要求 VirtIO backend
增加 raw ring test hook，不创建第二 owner、不直接编辑 slot/ring index、
不伪造 completion。MS05 guest probe 仍通过正常 UDP/TCP/ARP 路径产生 traffic，并以 PRE、
HELD、FULL、RELEASED、POST snapshots 证明 packet retention、精确容量和账本闭合。host/model
tests 另行覆盖无法稳定由 QEMU 调度制造的交错与乱序。

**Reason**

QEMU device completion 太快，普通吞吐不能保证 slot 或 descriptor 精确达到 Full。暂停
submit/reclaim 是在保持同一 owner 和真实 enqueue/reclaim 代码的前提下最小的确定性控制。
`Service` 已经串行化 Router、flush 和设备账本，把 lease 放在相同 ownership boundary 可让
控制、tick 与 V3 共享一个 committed-state 定义；wake-only timer 避免 stale timer 获得状态
所有权，最长 lease 则避免诊断工具失败改变后续系统状态。

**Impact**

新增 `tests/ms05_data_plane_probe.c`、host decision harness 和有界 host stimulus。probe
每个 mode 只能输出一个 `MS05 PASS|FAIL mode=...`，超时或 lease 自动释放一律 FAIL。
controls 不编入 D1/真实板范围，不能作为硬件能力证据。

**Alternatives**

- 仅发送大流量并等待偶然 Full：不可复现且普通成功不能证明内部背压，拒绝。
- ioctl 直接写 ring index或伪造 telemetry：绕过真实 owner/descriptor 路径，拒绝。
- 复用通用 `network_benchmark`：其 schema 不观察 slot、ticket 或 buffer conservation，拒绝。
- 独立全局多原子 state + version/seqlock：bounded reader 无法在 contention 时同时保证真实
  committed tuple，version exhaustion 还可能使 active Hold 永久不可释放，拒绝。
- timer 携带 generation 并直接清理 lease：替换后的 stale timer 可能误清新 Hold，拒绝。

### D10. 分层 Gate，并把 QEMU 结论限制为软件与设备模型

**Decision**

验证顺序固定为：

1. driver/queue RED→GREEN：direction control、old/new EVENT_IDX、cookie/token/buffer 守恒、
   QueueFull/oversize/submit error recovery。
2. axnet model RED→GREEN：fixed slots、typed Router/ARP/fanout、event interleavings、budgets、
   lifecycle、ticket/flush 和 cancellation。
3. ABI/source/build：V1/V2 canary、V3 offsets、ISR 不碰 descriptor/Service、single owner、
   full host tests、UART、kernel checks、QEMU/D1 build、strict OpenSpec 和 full diff review。
4. change-local QEMU：TX-only、bidirectional、slot Full→recovery、descriptor Full→recovery、
   flush C4、ARP/ICMP/UDP/TCP 5555/nonblocking/poll，以及 R51 snapshot/idle/nudge/burst。

自动产品 Gate 非零立即停止；只有原始日志明确定位到只读路径、禁网、`EPERM`、`SIGSYS`
或用户终端边界时才能按 R44 记 `ENV-BLOCKED`。当前 `make host-test` 的 UDP socket `EPERM`
必须在最终手工批次复跑原命令。每个 runtime mode 保存 command、environment、revision、
artifact hash、完整串口、probe 输出、退出状态和唯一终态 marker。

QEMU PASS 只证明当前单 hart、单 VirtIO-MMIO NIC 下的软件 ownership、VirtIO device-model
notification 和有界数据面行为。MS04 曾被用户豁免的 boot signature、termination、完整
MS01/MS02/MS03 compatibility 或 exact-binary 项，除非本 change 重新取得完整原始证据，
仍保留 WAIVED/SKIPPED。

**Reason**

driver contract、stack handoff 和 runtime orchestration 的失败面不同；先通过确定性 model
Gate 可避免用 QEMU 成功掩盖 ownership bug。模拟器不能代替真板 DMA/cache 与性能证据。

**Impact**

实施拆为多个独立 iteration，最终 QEMU 和环境复跑单列为用户手工轮次。每轮只在前一轮
产品 Gate 通过后进入下一层。

**Alternatives**

- 一轮同时修改所有层并直接手测：故障定位面过大，MS04 已证明这种粒度不可取，拒绝。
- 用历史 MS04 Evidence 代替重跑：产物和 queue task 已变化，拒绝。
- 将 QEMU 结果外推真板/SMP：证据能力不支持，拒绝。

### D11. 用可注入 operation seam 闭合 probe deadline，用结构化 manifest 生成 Evidence

**Decision**

Iteration 009 已接受网络字节序、peer 校验、非空流量、精确 Full/POST 账本和 artifact hash，
但连续三次执行都没有证明所有真实阻塞路径受同一 deadline 约束，也没有形成可拒绝伪命令
和摘要日志的 Evidence。后续工作迁入新的逻辑 Iteration，不创建 Iteration 009 Cycle 003。

guest probe 在测试工具内部建立一个可注入 operation seam，覆盖 monotonic clock、bounded
sleep、diagnostic/flush ioctl、socket timeout、send 和 receive。生产实现调用现有 libc/syscall；
host harness 注入 fake clock 与 fake operations，执行与 RISC-V payload 相同的 mode runner，
不得再以纯 decision helper 或 source guard 代替生产路径测试。所有操作共享一个 absolute
mode deadline，并可附加更短的 phase deadline：

- 每次 ioctl、send、receive 和 sleep 前先读取 clock，剩余 budget 为零时不得启动操作；
- 可阻塞操作的 timeout 和 retry sleep clamp 到 `min(mode remaining, phase remaining, nominal)`；
- 操作返回后再次读取 clock，equal/late completion 失败，且不能继续下一 side effect；
- Python host 对 `recvfrom` 和 `sendto` 使用相同的 pre/post deadline rule；bidirectional 发送
  循环每个 datagram 都重新取 budget，fake socket 必须能模拟 send 阻塞跨界；
- held mode 用单一 cleanup 出口维护 `hold_active`。Hold 一旦提交，任意后续 success/error 都
  至多尝试一次 Release；Release 使用原 absolute mode deadline，不创建 cleanup deadline。

自动 Evidence 不再由手写 `commands.txt` 或拼接摘要作为权威来源。新增 capture runner 与
JSON manifest，复用仓库现有 network-benchmark Evidence 的“结构化 manifest + file hash +
fixture self-test”模式，但使用 MS05 Gate schema。每条 Gate record 至少包含稳定 gate ID、
literal argv array、cwd、RFC 3339 start/end、exit、expected result/classification、raw log 相对路径
和 log SHA-256。runner 直接以 argv 启动 subprocess；需要顺序的命令拆成独立 records，不把
`&&`、循环或重定向 prose 当作命令。100× Gate 为 100 个可枚举 child records，每次保留完整
stdout/stderr；汇总只从 records 生成。

artifact records 保存 path、size、mtime、SHA-256、生成 Gate ID，以及 source-freeze 的 path、
content hash 和 index/worktree identity。`file`、`stat`、`sha256sum` 也是带完整 argv/raw log 的
records。README 和人读摘要从 manifest 派生，不替代 raw log。

audit 校验 schema、必需 Gate 集、唯一 ID、可解析时间、argv 非空、exit/classification、每个
log 的存在性/非空/hash、100× child 完整性、source-freeze→build 顺序、artifact identity、D1
精确诊断和 R44 原始失败。负向 fixtures 在临时复制中逐项变异，并必须返回预期的稳定 error
code；任何其他 AuditFailure 都算 fixture 失败。product manifest冻结后，runner用同一subprocess
capture primitive收集audit完整stdout/stderr，但不把audit追加回被审计manifest；随后生成只引用
manifest hash、audit-log hash和verdict的qualification record，避免自引用。最终 diff Gate同时
检查index与worktree，避免staged实现被普通`git diff --check`忽略。

**Reason**

前三个 Cycle 的 helper tests 能证明算术，却不能驱动真实 mode runner 的 ioctl/socket/cleanup
分支；手写 Evidence 又允许命令、时间和 raw output 脱节。operation seam 把 production control
flow 放进 deterministic host tests，manifest runner 则让 argv、exit、log 和 hash 在执行时一次
生成，审计无需从 prose 推断发生过什么。

**Impact**

新增 probe runtime harness、MS05 capture runner、manifest schema/fixtures，并重写现有 audit。
产品 kernel/driver、V1/V2/V3 ABI、wire protocol和六个 mode 名不变。Iteration 009 Evidence 保持
历史不可变；新资格证据写入新逻辑 Iteration。原手工 QEMU Iteration 顺延一位。

**Alternatives**

- 继续增加纯 helper 与 source regex：不能证明 side-effect 顺序、Release 次数或 syscall 前后
  deadline，拒绝。
- 继续手写 Markdown 命令索引并扩大 placeholder regex：无法证明 argv 与 raw log 同源，拒绝。
- 直接进入手工 QEMU 观察 timeout：环境调度不稳定且不能穷举 post-Hold error，拒绝。
- 修改 kernel/driver ABI以便测试：当前缺口位于 probe orchestration 与 Evidence，不需要扩大
  产品接口，拒绝。

## Risks / Trade-offs

- [每个 NIC 约增加 194 KiB frame backing 和两次 L2 copy] → 初始化时记录容量与分配失败
  stage；保持固定上界，并把性能优化留给有基线的后续 change。
- [queue task 与 caller-driven stack 竞争同一个 `Service` mutex] → 每阶段固定 budget 32，
  guard 不跨 await，持续双向 model/QEMU 场景必须证明两边都有进度。
- [fanout preflight 与 commit API 增加 Device 复杂度] → 所有目标在同一锁内完成；任何正常
  路径上的结论漂移作为 invariant fault，不以 Full 重试。
- [ARP pending 保留队首会产生 head-of-line blocking] → 明确计数并维持 bounded；MS05 不做
  reordering，功能回归覆盖 reply、pending flush 与 Full 恢复。
- [Faulted owner 不自动恢复会使 NIC 停止] → snapshot、stable error、flush/stack wake 和
  fault source stage 必须完整；避免更危险的双 owner fallback。
- [两个 waker role 可能增加 spurious poll] → direction-aware wake target 和 generation
  recheck；无工作 poll 不得自唤醒，idle/nudge Gate检查零 descriptor 进度。
- [QEMU hold control 改变时序] → 只编入 QEMU feature、最长 2 秒 lease、自动释放即测试
  FAIL；controls 不伪造 ring/slot/completion，也不进入真板结论。
- [`u64` ticket 最终耗尽] → checked increment 后稳定 fault；不允许 wrap 后与 live ticket
  alias。该边界有 model test，不宣称运行期可恢复。
- [工作区当前 `CLAUDE.md` 已有用户修改] → Act 和后续 Review 只检查/修改 change 明列的
  产品 surface，不覆盖或顺手格式化无关用户 diff。

## Migration Plan

1. 先迁移 transport-neutral contract、VirtIO cookie/reclaim 与 EVENT_IDX old/new 公式；此时
   axnet 仍保持 MS04 owner，完整 driver/queue tests 必须独立通过。
2. 加入 fixed slots、ticket tracker 和 typed Device/Router/ARP handoff，但在 lifecycle
   Active 前保留现有同步 fallback；用 fake device 完成 packet retention 和 fanout tests。
3. 将 MS04 RX task 演进为双向 queue service，执行一次性 owner 切换并接通 slots、通用
   event、独立 budgets 和 stack-progress wake。
4. 接入 flush、V3 telemetry、QEMU lease controls 与 probe；保留 V1/V2 原 consumer。
5. 通过全部自动 Gate 和 full diff review 后，再由用户按 R44 执行 change-local QEMU
   runtime 与环境阻塞复跑。

代码回滚以 iteration 边界进行：在 owner 切换 iteration 尚未完成时，删除未绑定的 slots
和新接口即可回到 MS04；双向 Active 已提交后若 Gate 失败，回退整个 queue-service
iteration，不能只恢复同步 TX 留下半迁移。Evidence 和首次失败日志不删除、不清洗。

## Open Questions

无。影响实现的 capacity、frame size、fanout 原子性、ARP Full 行为、budget、event/waker、
flush target、ABI 兼容、QEMU 压力控制与证据边界均已在 Gate 1 后闭合。若 Act 发现当前
依赖不能在保持 `RING_EVENT_IDX` 的条件下控制 TX used notification，必须停止并返回 Plan，
不能在实现中临时选择替代架构。
