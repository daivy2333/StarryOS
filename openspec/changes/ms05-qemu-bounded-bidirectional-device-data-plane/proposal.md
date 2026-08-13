## Why

MS04 已建立由 IRQ 唤醒的唯一 RX queue task，但 TX 仍由 caller-driven `Service` 同步提交，并且完成只在后续发送时顺便回收。MS05 需要把 RX/TX descriptor ownership 收口到同一个有界 queue service，建立可恢复的 packet-slot 背压和明确的 C4 completion/flush 语义，为 MS06 stack runner 提供稳定设备边界。

## What Changes

- 将 RX-only queue task 与事件入口演进为当前单 VirtIO-MMIO 设备的唯一双向 queue service；共享 used-ring IRQ 只发布通用 queue event，task 在 task context 中分别检查 RX/TX completion。
- 在 queue service 与现有 caller-driven `Service` 之间增加固定容量 64、预分配、包原子的 RX/TX Ethernet frame slots；raw descriptor、VirtIO token 和 `NetBufPtr` 不跨 slot 或 `await`。
- 将设备发送结果改为可区分 `Accepted`、可恢复 `Full`、带原因 `Dropped` 和 fatal error 的 typed outcome；Full 时上游保留 packet，不再先 dequeue 后静默丢失。
- 增加有界 TX submit、completion/reclaim 和 buffer 守恒；正常 descriptor/buffer exhaustion 表达为重试背压，ownership 或 token 破坏才进入 fault。
- 为每个 TX slot 接受的 packet 分配 ticket；内部单 waiter flush 捕获调用时的最高已接受 ticket，并等待此前所有 packet 达到 C4 reclaim 或返回明确错误。flush 不表示 peer delivery、TCP ACK 或应用处理。
- 为 RX completion、TX completion、TX slot 可读、RX slot 可写、software nudge 和 budget self-yield 建立统一 generation/register-recheck；RX、TX reclaim 和 TX submit 各自有有界 budget。
- 增加 occupancy/high-water/full/drop、submit/completion/reclaim、flush 和 budget telemetry，并定义 packet slot、TCP byte-stream short write 与 UDP datagram 原子性的分层语义。
- 增加确定性的 Full→恢复 host/model 与 QEMU 见证，并重跑受影响的 MS04 snapshot、idle、nudge、burst 及网络功能回归；不扩建 I16 通用 benchmark。

## BDD Scenario Sketch

### Happy Path：双向有界推进

- **前置状态**：单 hart QEMU VirtIO-MMIO NIC 已完成 MS04 激活，RX/TX slots 均有容量，唯一 queue service 持有两条硬件队列。
- **触发动作**：guest 与 host 同时交换有限 RX/TX burst。
- **可观察结果**：queue service 在各方向 budget 内完成 RX reap/refill、TX submit/reclaim；slot、buffer 和 descriptor 数量守恒，两个方向均持续推进。
- **失败边界**：ISR 搬 descriptor、第二 owner 访问任一硬件队列、单轮无界 drain、任一方向饿死或资源计数漂移均失败。

### TX Slot 或 Descriptor Full

- **前置状态**：TX slot 或 VirtIO TX queue 达到可配置容量边界。
- **触发动作**：caller-driven `Service` 继续生成 packet，或 queue service 尝试提交队首 frame。
- **可观察结果**：上游得到可恢复 `Full`，队首 frame 保留且 occupancy/full counter 增长；completion/reclaim 释放容量后软件事件使提交恢复。
- **失败边界**：Full 被映射为 fatal、packet 在未接受时被 dequeue、静默 drop、busy loop 或恢复后容量永久缩小均失败。

### RX Slot Full

- **前置状态**：RX slot 已满，VirtIO RX used ring 仍有 completion。
- **触发动作**：queue service 被硬件或软件事件唤醒。
- **可观察结果**：task 在 reap 下一个 completion 前停止；协议栈消费 slot 后发布空间事件，task 恢复 reap/refill。
- **失败边界**：持有未交付 `NetBufPtr` 跨 `await`、覆盖 slot、无原因 drop 或依赖轮询忙等恢复均失败。

### Shared IRQ 与 Register-Recheck

- **前置状态**：RX/TX 任一 completion 可在 task 注册、通知重臂或返回 `Pending` 附近发生。
- **触发动作**：注入 RX-only、TX-only、双向、spurious 和 software-nudge 事件。
- **可观察结果**：ISR 只 ack/snapshot/wake；task 通过通用 generation、双队列 arm/recheck 和 slot 状态复查观察全部工作。
- **失败边界**：依赖 ISR 区分 RX/TX、lost wakeup、每 completion 无界 IRQ 或空闲持续自唤醒均失败。

### Completion 与 Flush

- **前置状态**：TX slot 已接受若干带 ticket 的 packet，其中部分未提交或已提交未完成。
- **触发动作**：调用内部 flush，随后允许 completion 乱序到达并继续接受更新 ticket。
- **可观察结果**：flush 只等待调用时 target 及以前的 ticket 达到 C4；之后接受的 packet 不阻塞旧 target；空 target 立即完成。
- **失败边界**：以全局队列为空作为条件、把 C4 声明为 peer delivery、lost wakeup 导致永久 Pending，或 fatal 后仍返回成功均失败。

### Error、Cancellation 与并发边界

- **前置状态**：队列已激活，存在 oversize、submit/reclaim error、第二 flush waiter 或等待 future 被 drop 的可能。
- **触发动作**：逐项注入这些边界。
- **可观察结果**：正常 exhaustion 返回 Full/Again；buffer 回到可用集合；fatal 进入可观察 fault 并唤醒 flush 返回错误；第二 waiter 返回 `ResourceBusy`；drop future 只取消等待，不取消、重发或释放设备仍拥有的 packet。
- **失败边界**：可用容量逐次缩小、双重回收、UAF、自动启动 polling owner 或隐式重发均失败。

### Packet、Socket 与兼容性边界

- **前置状态**：现有 Router、TCP/UDP socket、MS04 RX probe、UART 和 early/panic console 可用。
- **触发动作**：执行 packet-slot 边界、TCP short write、UDP send、MS04 四模式和网络功能回归。
- **可观察结果**：slot 只整包接受；TCP 保留 byte-stream short write，UDP 保留 datagram 原子性；MS04 RX 字段和模式继续可判定。
- **失败边界**：把 slot Full 直接宣称为准确 fd `POLLOUT/EAGAIN`、破坏旧 RX 证据读取，或把 QEMU 结果扩大为 SMP、真板、DMA/cache 或性能结论均失败。

### Timeout 与专用压力见证

- **前置状态**：通用 benchmark 不具备 fill-to-EAGAIN 和 slot 容量控制，验证可能在固定 deadline 前未完成。
- **触发动作**：运行 change-local 的确定性 Full→恢复 probe，或验证被用户/环境中断。
- **可观察结果**：probe 对容量边界、恢复、计数守恒和完成 marker 给出明确 PASS/FAIL；超时、中断或原始证据缺失记为未完成。
- **失败边界**：用普通吞吐成功替代 Full→恢复、把环境阻塞记为产品 PASS，或为本 change 扩建 I16 性能基础设施均失败。

## Capabilities

### New Capabilities

- `qemu-bounded-bidirectional-device-data-plane`: 定义 MS05 的固定容量 RX/TX frame slots、typed backpressure、TX ownership/completion、ticketed C4 flush、双向 budget、telemetry 和单 hart QEMU 验收。

### Modified Capabilities

- `qemu-async-rx-queue-baseline`: 将 RX-only 通知、task、临时 Router handoff 与运行证据约束演进为双向 queue service 和最终 packet-slot 边界，同时保留 MS04 核心回归判定。

## Impact

- `axdriver_net` 的 queue control、非阻塞 TX 错误和 completion contract。
- `axdriver_virtio` 与 `virtio-drivers` 的 TX buffer/token ownership、EVENT_IDX 通知和 queue-full 恢复。
- `axnet` 的 device outcome、Ethernet/ARP handoff、Router dispatch、slot、queue task lifecycle、waker 和 telemetry。
- VirtIO-MMIO IRQ handler 的通用 queue event 与既有 MS04 诊断字段。
- host/model tests、change-local QEMU probe、MS04 R51 核心模式和网络功能回归。
- 不新增 executor、外部依赖、PCI/DWMAC 产品代码、公共 socket flush API 或通用 benchmark 模式。

## Non-goals

- 独立 smoltcp stack runner、准确 socket readiness、多 socket waiter 或移除 caller-driven `poll_interfaces()`。
- reset generation、link flap、设备热插拔、队列 stall timeout 或自动恢复。
- SMP、跨 hart wake、multiqueue、RSS、PCI、DWMAC、真板 transport 或真实 DMA/cache 结论。
- 零拷贝、descriptor-backed smoltcp token、offload、batching、IRQ moderation 或性能优化。
- 对端接收、TCP ACK 或应用处理完成语义。
- 扩展 I16 的 RTT、exact benchmark burst、通用 fill-to-EAGAIN、性能或长期稳定性基础设施。
- 修改全局 tasks、SNAPSHOT、M/D/K/R/I，归档 change，或实施产品代码。

## Gate 1

- Status: approved.
- 默认假设：slot 容量为每方向 64；ARP pending 满返回可恢复 Full；fatal 后 queue service 保持唯一 owner 且不回退 polling；内部 flush 只允许一个 waiter；专用压力见证属于本 change，通用 benchmark 扩展不属于本 change。
- 用户于 2026-08-12 审计 proposal、BDD 场景和 delta specs 后回复“批准”，正式批准 Requirements and Scope。

## Gate 2

- Status: approved.
- Investigation: PASS。已定位公共 trait、全部 implementor、Router/Ethernet/ARP、MS04 task、ISR、snapshot/probe、VirtIO token/buffer 和 EVENT_IDX 调用链，并在当前 revision 运行新鲜基线。
- Design: PASS。`design.md` 已闭合 fixed slots、typed fanout/ARP、双向 owner、event/waker、budget、ticketed flush、V3 ABI、QEMU lease controls 和证据边界，没有影响实现的未知项。
- Task contracts and iteration balance: PASS。17 个任务分配到 6 个依赖有序 iteration；每项包含 WHERE、WHY、HOW、EXPECTED、RED/GREEN、验证和停止条件，只展开 Iteration 000。
- Traceability and verification: PASS。R1-R14 均映射到 design、task、iteration、code surface 和 test witness；没有 Missing 或未批准 Simplified。Iteration 000 的自动验证与 `Persisted Evidence: none` 已明确。
- Approval record: 用户于 2026-08-12 审计 Gate 2 检查项、Iteration Plan、RTM 与 Iteration 000 后回复“批准”，正式批准 Execution Readiness。
