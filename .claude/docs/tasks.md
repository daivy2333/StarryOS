# tasks.md — 任务追踪

> 最后更新: 2026-08-09 | 分支: net-k3 | grep: `<!-- T{编号} -->`
> 来源: R41、R47、R49、M41、D22、K31-K32、K37；MS01-MS03 与 MS16 已归档。

---

## 当前：异步 NIC 开发

每个 milestone 只引入一个主要调试变量。执行前必须通过 OpenSpec Plan 建立 BDD、RTM、测试见证和获批 change。完成状态只依据对应 change 的新鲜证据更新。

执行顺序固定为 T01→T25。QEMU 与真板是不同证据类别。前置 Gate 未通过时，后续 milestone 保持等待。

| ID | Milestone | 交付范围 | 验证 Gate | 前置 | 状态 |
|---|---|---|---|---|---|
| <!-- T01 --> T01 | smoltcp/axnet 同步基线 | 纳入本地 smoltcp 0.13.1；本地化 axnet；移除 `RxToken::preprocess` 私有依赖 | TCP listen/accept、UDP、nonblocking 和 poll 与当前同步行为一致 | 无 | ✅ 完成 |
| <!-- T02 --> T02 | QEMU I/O 边界见证 | 固化串口、网络和 hostfwd 的独立路径；固定 VirtIO-MMIO 启动签名 | 无 hostfwd 仍可进 shell；MMIO net/block 可探测；串口成功不计网络成功 | T01 | ✅ 完成 |
| <!-- T03 --> T03 | MMIO 轮询网络基线 | 保持轮询驱动；建立明确 guest 服务和宿主端到端用例 | ARP/ICMP、UDP、TCP 5555 各有独立见证；空闲 CPU 只作基线记录 | T02 | ✅ 完成 |
| <!-- T04 --> T04 | MMIO IRQ 事实 | 解析设备地址、PLIC IRQ、claim/ack/rearm；只增加计数器 | 注入 RX/TX 事件时 IRQ 可重复增长；错误 IRQ 不触碰异步队列 | T03 | ✅ 完成 |
| <!-- T05 --> T05 | IRQ 唤醒原语 | 建立 `NetQueueControl`、AtomicWaker 和 register-recheck；ISR 不搬包 | event-before-register、register-during-event、spurious IRQ 无 lost wakeup | T04 | 🔄 MS04 进行中（依赖本地化、NetQueueControl 契约、EVENT_IDX 已就绪） |
| <!-- T06 --> T06 | QEMU 异步 RX | queue task 只处理 RX reap/refill 和 budget；TX 保持基线 | 单向 RX burst 无 busy loop、饿死或 descriptor 泄漏；budget 可观测 | T05 | ⏳ 等待 T05 |
| <!-- T07 --> T07 | QEMU 异步 TX | 增加 TX submit、reclaim、completion 和 flush；不改 packet slot | queue full 产生背压；completion 不等于 peer delivery；flush 不永久 Pending | T06 | ⏳ 等待 T06 |
| <!-- T08 --> T08 | 有界 packet slot | 建立 RX/TX slot、occupancy、drop reason 和 partial write 契约 | 满载时内存有上界；背压可见；descriptor 不跨 await 泄漏 | T07 | ⏳ 等待 T07 |
| <!-- T09 --> T09 | stack runner | 独立推进 smoltcp ingress、egress、maintenance 和 timer | device、software、timer 唤醒可复现；空闲不轮询；持续流量不饥饿 | T08 | ⏳ 等待 T08 |
| <!-- T10 --> T10 | socket readiness | 将 smoltcp 单槽 waker 桥接到 `axpoll::PollSet` | 多 waiter、overflow、close 和 error 下，poll/select 与实际 I/O 一致 | T09 | ⏳ 等待 T09 |
| <!-- T11 --> T11 | reset 与取消 | 引入 generation、stale completion 丢弃、cancel、timeout 和 link flap | fault injection 下无 UAF、重复回收、永久 Pending 或静默丢包 | T10 | ⏳ 等待 T10 |
| <!-- T12 --> T12 | QEMU 多 hart | 定义 queue affinity、跨 hart wake、控制面同步和 ordering 理由 | 多 hart 双向压力与 reset/I/O 交错无 race；单 hart 结果不计通过 | T11 | ⏳ 等待 T11 |
| <!-- T13 --> T13 | 目标板事实 Gate | 记录启动介质、DTS/ACPI、MAC、PHY、MMIO、IRQ、DMA/cache 和 CPU/hart 拓扑 | 每项来自真板、固件描述或手册；未知项阻塞后端选择 | T12；目标硬件可用 | ⏳ 等待 T12 |
| <!-- T14 --> T14 | 目标板启动与 MAC 寄存器 | 接通 feature、镜像和 early console；只验证目标 MAC 寄存器访问 | 重复启动稳定；寄存器非全零/全一；异常访问可定位 | T13 | ⏳ 等待 T13 |
| <!-- T15 --> T15 | 目标板 Clock/Reset/PHY | 依据 T13 的 bootloader handoff 决定保留或恢复 clock/reset；只建立链路 | preserved 状态有原值；PHY/link 或等效链路结果可重复 | T14 | ⏳ 等待 T14 |
| <!-- T16 --> T16 | 目标板设备中断 delivery | 只接 MAC IRQ claim、handler、device status 和 EOI | IRQ claim 与设备 status 对齐；无中断风暴；CPU/hart 初始化可区分 | T15 | ⏳ 等待 T15 |
| <!-- T17 --> T17 | 目标板 DMA/cache 基线 | 建立 DMA 地址转换、cache/barrier 和硬件队列 ownership | CPU/设备看到同一 descriptor/queue entry；ownership 转移有日志和断言 | T16 | ⏳ 等待 T16 |
| <!-- T18 --> T18 | 目标控制器轮询 RX | 只实现最小 RX refill/reap 或等效接收队列 | 抓包与硬件队列计数一致；坏帧和队列满有明确结果 | T17 | ⏳ 等待 T17 |
| <!-- T19 --> T19 | 目标控制器轮询 TX | 只实现最小 TX submit/reclaim 或等效发送队列 | ARP/ICMP 发包可抓取；回收不重复；timeout 可诊断 | T18 | ⏳ 等待 T18 |
| <!-- T20 --> T20 | 目标后端异步 RX | 将 T06 的 RX queue task 接到真板 IRQ | RX burst 无 lost wakeup；budget、drop 和 occupancy 可观测 | T19 | ⏳ 等待 T19 |
| <!-- T21 --> T21 | 目标后端异步 TX | 将 T07 的 TX completion 和背压接到真板 | TX burst、queue full 和 flush 通过；无硬件队列双重所有权 | T20 | ⏳ 等待 T20 |
| <!-- T22 --> T22 | 真板恢复语义 | 单 hart 验证 link flap、设备 reset、generation 和 stale completion | reset 前后对象不混用；无重复回收、永久 Pending 或静默丢包 | T21 | ⏳ 等待 T21 |
| <!-- T23 --> T23 | 真板多 hart | 验证 queue affinity、跨 hart wake、控制面同步和 ordering | 双向流量与 reset 交错无 race；每项 ordering 有角色说明 | T22 | ⏳ 等待 T22 |
| <!-- T24 --> T24 | 真板长稳压力 | 组合 burst、双向和 ring full；只评估稳定性与指标 | 长时间运行无 stall；drop、p99、occupancy 和环境可复现 | T23 | ⏳ 等待 T23 |
| <!-- T25 --> T25 | 数据驱动优化 | 按测量逐项评估 batch、moderation、offload、zero-copy 和 multiqueue | 每项独立 A/B；正确性 Gate 不退化；无数据则记录 SKIPPED | T24；指标触发 | ⏳ 等待 T24 |

共同约束：

- M36/D20：ISR、queue task、stack runner、socket readiness 分层。
- M41/D22：QEMU 以 VirtIO-MMIO 起步；transport 适配不得泄漏到异步队列语义。
- K31：串口终端与 hostfwd 分开取证。
- K32：当前 feature 合并实际选择 MMIO；PCI 不计已验证。
- K09：只采用 `embassy-sync::AtomicWaker`；不引入第二套 executor。
- K26：以 packet buffer 和 DMA descriptor 为单位，不复制 UART 字节 ring。
- M37/M38/D21 只保留为 VF2 平台知识，不应用到未确认的目标板；T13 必须重新取得 bootloader、IRQ 和 clock/reset 事实。
- M39：跨 hart ordering 按同步角色说明；QEMU 单 hart 不能作为 SMP 证据。
- I06 只在 T13-T24 的触发条件满足时评估。
- I13-I16 未承诺，不得混入 T01-T25。

---

## Milestone Roadmap

本节按稳定基线组织项目阶段。现有 T01-T25 保留为单变量执行分解；一个
milestone 可以由一个或多个 change 完成，不预先绑定数量。

路线先完成 QEMU 异步基线，再进入目标板：

```text
QEMU:  MS01 -> MS02 -> MS03 -> MS16 -> MS04 -> MS05 -> MS06 -> MS07 -> MS08
BOARD: MS08 -> MS09 -> MS10 -> MS11 -> MS12 -> MS13 -> MS14 -> MS15 (指标触发)
```

### MS01：smoltcp/axnet 同步兼容基线

- Status: completed
- Outcome: 本地 smoltcp 0.13.1 与本地化 axnet 保持现有同步 socket 行为。
- Rationale: T01 单独形成后续设备和异步改造共同依赖的协议栈基线。
- Dependencies: None
- Scope: T01；依赖接入、axnet 本地化、listener/backlog 兼容和同步回归。
- Non-goals: QEMU IRQ、异步队列、stack runner、真板适配。
- Workload: 依赖兼容、socket 语义迁移、构建集成和回归证据。
- Stable baseline: TCP listen/accept、UDP、nonblocking 和 poll 可重复通过。
- Verification boundary: 本地化前后同步行为一致，编译和功能 Gate 均有证据。
- Diagnostic boundary: 失败限制在 smoltcp API、axnet 注入点和 listener 语义。
- Split signals: listener/backlog 迁移产生可独立交付且不依赖 smoltcp 接入的第二项成果。
- Related changes: `t01-smoltcp-axnet-baseline`（已归档于 `openspec/changes/archive/2026-07-29-t01-smoltcp-axnet-baseline/`；3 iterations，14/14 QEMU 手测 PASS）。

### MS02：VirtIO-MMIO 轮询网络基线

- Status: completed
- Outcome: 在串口、网络和 hostfwd 分证据的 QEMU 环境中建立同步轮询收发基线。
- Rationale: T02 的环境见证只为 T03 的可复现轮询结果服务，二者共享验证和诊断边界。
- Dependencies: MS01
- Scope: T02-T03；启动签名、设备探测、guest 服务、ARP/ICMP、UDP、TCP 和空闲 CPU 基线。
- Non-goals: IRQ 驱动、PCI 兼容、异步收发和性能优化。
- Workload: QEMU 环境固化、端到端 payload、包级证据和基线测量。
- Stable baseline: MMIO net/block 可探测，轮询网络功能和测试环境可重复。
- Verification boundary: 串口成功不计网络成功，各网络协议与 hostfwd 路径独立取证。
- Diagnostic boundary: 失败限制在 QEMU 环境、MMIO probe、guest 服务或同步数据面。
- Split signals: 自动化环境需要提升 I14 或 I15，形成独立且已承诺的基础设施成果。
- Related changes: `ms02-virtio-mmio-polling-baseline`（已归档于 `openspec/changes/archive/2026-07-29-ms02-virtio-mmio-polling-baseline/`；4 iterations，8/8 unit + 14/14 MS01 runtime + QEMU no-hostfwd + user-net TCP/UDP + TAP ARP/ICMP + 30 秒空闲 CPU）。Runbook `ms02-virtio-mmio-evidence.md` (R45) 已发布。

### MS03：VirtIO-MMIO 可诊断中断基线

- Status: completed
- Outcome: MMIO 网卡中断可以重复投递、确认来源并正确 ack/rearm。12/12 QEMU gates PASS，UART IRQ 10 设备 handler + net IRQ 7 诊断 handler，guest C probe 5 modes 全部通过，MS01/MS02 回归零退化。
- Dependencies: MS02
- Scope: T04
- Non-goals: waker、queue task、descriptor 搬运和协议栈推进。
- Related changes: `ms03-virtio-mmio-diagnostic-irq-baseline`（归档于 `openspec/changes/archive/2026-08-03-ms03-virtio-mmio-diagnostic-irq-baseline/`；1 iteration，Plan Review: no-follow-up，12/12 QEMU gates PASS）。Runbook `ms03-virtio-mmio-irq-evidence.md` (R48) 已发布。

### MS16：QEMU 轮询网卡性能基线

- Status: completed — 2026-08-06 按用户确认收口；未生成 TAP standard B0
- Outcome: 固定跨 QEMU、真板、polling 和 async treatment 复用的测试矩阵、完成点、portable workload、结果协议和资格 Runbook。
- Rationale: 异步 RX 引入前先固定测试语义和重跑方法。当前不要求运行完整矩阵，也不修复 smoke 暴露的网卡问题。
- Dependencies: MS03
- Scope: R47 测试目录与指标口径；版本化 manifest、C1-C6、TCP/UDP portable workload、host 采集与报告工具；user-net 六方向执行资格；R49 的 TAP、矩阵和证据操作。
- Non-goals: 异步 waker、queue task、协议栈 readiness、删除 10ms 轮询兜底、改变队列/socket 容量或网络行为、自动化 QEMU runner、仅为基准定位注册表具体驱动、netem 故障注入、长时间 soak、真实硬件性能和性能优化；user-net 不作为绝对性能结论。
- Workload: 后续环境按 R49 选择协议、方向、payload、flow 和 profile，分别判定 execution、correctness 和 performance 资格。
- Stable baseline: 主 `network-benchmark-baseline` spec、R47、R49、portable workload 和归档 Evidence。TAP、真板或 async 运行时复用这些口径。
- Verification boundary: host/local tests 通过；guest artifact 可执行；N00-N03 与 user-net 六方向产生结构化结果。invalid 保留，但不生成性能结论。
- Diagnostic boundary: 将失败限制在基准协议/校验、QEMU 拓扑与 Runbook、host peer/采样、socket/axnet、轮询数据面或 MS03 IRQ 快照；不混淆 TAP/user-net/loopback，也不混淆 host 与 guest CPU。
- Split signals: 已有入口但未运行的项目见 R49。RTT、exact burst、背压指标和内部遥测等基础设施缺口见 I16，获批后另建 change。
- Related changes: `ms16-qemu-polling-network-performance-baseline`（归档于 `openspec/changes/archive/2026-08-06-ms16-qemu-polling-network-performance-baseline/`；保留 6/25 已完成 tasks。已有入口但未运行的项目见 R49；基础设施缺口见 I16）

### MS04：QEMU 异步 RX 队列基线

- Status: planned
- Outcome: MMIO RX 由最小 ISR 唤醒唯一 queue task，以有界 budget 推进。
- Rationale: T05 的唤醒原语与 T06 的 RX 服务共同证明第一条可用的异步队列路径。
- Dependencies: MS16
- Scope: T05-T06；transport-neutral `NetQueueControl`、AtomicWaker、register-recheck、RX reap/refill 和 budget；公共接口不暴露 VirtIO descriptor 类型。
- Non-goals: 异步 TX、最终 packet slot、stack runner 和 socket readiness。
- Workload: 唤醒协议、队列所有权、RX completion、竞态测试，以及用 VirtIO 与 DWMAC 两种设备模型审查 contract；不引入 DWMAC 代码。
- Stable baseline: 单向 RX burst 无 lost wakeup、busy loop、饥饿或 descriptor 泄漏。
- Verification boundary: event-before-register、register-during-event、spurious IRQ 和 budget exhausted 可复现。
- Diagnostic boundary: 失败限制在 IRQ 到 queue task 的通知、RX ownership 或调度公平性。
- Split signals: queue contract 需要同时支持多个互不兼容的 transport 语义。
- Related changes: None

### MS05：QEMU 有界双向设备数据面

- Status: planned
- Outcome: RX/TX 通过有界 packet slot 形成可背压、可回收的双向设备数据面。
- Rationale: T07 的 TX completion 与 T08 的 slot/backpressure 共同形成完整的设备侧双向基线。
- Dependencies: MS04
- Scope: T07-T08；TX submit/reclaim/completion/flush、RX/TX slot、occupancy、drop 和 partial write。
- Non-goals: 独立 stack runner、socket 多 waiter、reset 和零拷贝。
- Workload: TX 状态机、有界 handoff、压力边界和完成语义。
- Stable baseline: 内存有上界，queue/slot full 可观测并能恢复，descriptor 不跨 await 泄漏。
- Verification boundary: completion 不等于 peer delivery，flush 不永久 Pending，背压与实际容量一致。
- Diagnostic boundary: 失败限制在 TX ownership、slot handoff、回收或背压传播。
- Split signals: RX 与 TX slot 策略出现无法共享的验证或生命周期边界。
- Related changes: None

### MS06：应用可见的异步网络栈

- Status: planned
- Outcome: stack runner 和 socket readiness 让应用在无主动轮询依赖下使用异步网络。
- Rationale: T09 单独只有协议栈内部推进，和 T10 合并后才形成应用可依赖的阶段成果。
- Dependencies: MS05
- Scope: T09-T10；ingress/egress/maintenance/timer、device/software/timer wake 和 axpoll bridge。
- Non-goals: reset、SMP、真板 transport 和多接口扩展。
- Workload: 协议栈 runner、唤醒合流、多 waiter 语义和 socket 回归。
- Stable baseline: 空闲无轮询，持续流量不饥饿，poll/select 与实际 I/O readiness 一致。
- Verification boundary: 多 waiter、overflow、close、error 和三类 runner 唤醒均有见证。
- Diagnostic boundary: 失败限制在 stack 推进、timer/software wake 或 socket event bridge。
- Split signals: readiness bridge 需要替换 axpoll 并形成独立的多 waiter 子系统。
- Related changes: None

### MS07：QEMU 单 hart 恢复语义

- Status: planned
- Outcome: reset、取消、超时和 link flap 下的异步对象生命周期封闭。
- Rationale: 恢复语义必须在 SMP 放大竞态前先形成可故障注入的稳定基线。
- Dependencies: MS06
- Scope: T11；generation、stale completion、cancel、timeout、link flap 和 queue stall。
- Non-goals: 跨 hart 同步、真板 reset 和性能优化。
- Workload: 生命周期状态机、错误传播、故障注入和资源回收。
- Stable baseline: reset 前后对象不混用，等待者得到稳定完成或错误。
- Verification boundary: 无 UAF、重复回收、永久 Pending 或静默丢包。
- Diagnostic boundary: 失败限制在 generation、completion、取消或 reset 状态转换。
- Split signals: link 管理与设备 reset 形成两个可独立验收且可独立延期的控制面成果。
- Related changes: None

### MS08：QEMU 多 hart 正确性基线

- Status: planned
- Outcome: 异步网络在多 hart 下保持 queue ownership、跨 hart wake 和控制面同步正确。
- Rationale: SMP 是独立于单 hart 功能与恢复语义的并发故障域。
- Dependencies: MS07
- Scope: T12；queue affinity、跨 hart wake、reset/I/O 交错和 ordering 理由。
- Non-goals: multiqueue、RSS、真板 SMP 和吞吐优化。
- Workload: 并发模型、调度亲和性、原子序审计和多 hart 压力。
- Stable baseline: 多 hart 双向压力和 reset 交错不产生 race 或 ownership 冲突。
- Verification boundary: 单 hart 结果不计通过，每项 ordering 按同步角色解释。
- Diagnostic boundary: 失败限制在 CPU affinity、跨 hart 通知、共享控制面或内存序。
- Split signals: 引入 multiqueue 或 RSS，产生新的 queue-to-hart 分配成果。
- Related changes: None

### MS09：目标板事实与可观测链路基线

- Status: planned
- Outcome: 目标板可重复启动，MAC 控制器和链路状态可访问、可解释，并据此选定硬件后端。
- Rationale: T13 是 T14-T15 的调查输入；三者共同形成后端开发可依赖的平台基线。目标板尚未确认，不能预选 DWMAC 或继承 VF2 配置。
- Dependencies: MS08；目标硬件可用
- Scope: T13-T15；启动介质、DTS/ACPI、CPU/hart、feature、镜像、MAC、寄存器、clock/reset、bootloader handoff 和 PHY/链路。
- Non-goals: 设备中断 delivery、DMA 队列、网络收发和异步队列。
- Workload: 板级事实、启动链、控制器识别、寄存器观测、固件 handoff 和链路建立。
- Stable baseline: 重复启动稳定，目标 MAC 寄存器非全零/全一，PHY/link 或等效链路结果可重复，后端选择有板级依据。
- Verification boundary: 每项事实来自真板、固件描述或手册，未知项明确阻塞。
- Diagnostic boundary: 失败限制在启动链、MMIO 映射、clock/reset、PHY/链路或控制器识别。
- Split signals: 启动/MMIO 已形成可复用基线，但 PHY 因外部硬件长期独立阻塞。
- Related changes: None

### MS10：目标板可诊断设备中断基线

- Status: planned
- Outcome: 目标 MAC 中断经板级中断控制器 claim/dispatch、handler、device status 和 EOI 可重复投递。
- Rationale: 真板中断控制器和设备触发模式是独立高风险故障域，不能由 QEMU 证据替代。
- Dependencies: MS09
- Scope: T16；目标 MAC IRQ 路由、CPU/hart 初始化、cause、ack 和 EOI。
- Non-goals: DMA 收发、异步 queue task 和多 hart 流量。
- Workload: 板级中断路由、设备中断状态、重复触发和风暴诊断。
- Stable baseline: IRQ claim 与设备 status 对齐，EOI 后可再次触发。
- Verification boundary: 无中断风暴，CPU/hart 初始化和目标触发模式可区分。
- Diagnostic boundary: 失败限制在板级中断控制器、设备触发模式或 ack/EOI 顺序。
- Split signals: 目标控制器暴露多个必须独立验收的中断路径。
- Related changes: None

### MS11：目标控制器轮询双向网络基线

- Status: planned
- Outcome: 在明确 DMA/cache ownership 的前提下完成目标控制器轮询 RX/TX 和协议包收发。
- Rationale: T17 只有通过 T18-T19 的 descriptor 移动和真实包才能验证其 DMA/cache 契约。
- Dependencies: MS10
- Scope: T17-T19；DMA 地址转换、cache/barrier、descriptor/queue ownership、最小 RX/TX 和抓包。
- Non-goals: 异步 wake、reset、SMP、offload 和零拷贝优化。
- Workload: DMA 抽象、目标硬件队列、轮询收发、错误路径和协议验证；DWMAC 代码仅在控制器兼容时进入审计和移植候选。
- Stable baseline: CPU 与设备观察同一硬件队列状态，ARP/ICMP/UDP/TCP 与抓包一致。
- Verification boundary: RX/TX 回收不重复，坏帧、ring full 和 timeout 有明确结果。
- Diagnostic boundary: 失败限制在 DMA 地址、cache/barrier、目标硬件队列或轮询数据面。
- Split signals: RX 或 TX 暴露独立硬件阻塞，且另一方向已形成可复用稳定基线。
- Related changes: None

### MS12：目标后端异步双向数据面

- Status: planned
- Outcome: 已验证的 QEMU queue/stack 契约适配到目标控制器异步 RX/TX。
- Rationale: T20-T21 共享同一真板 IRQ、DMA 和队列适配边界，合并后形成双向 transport parity。
- Dependencies: MS11
- Scope: T20-T21；目标后端 RX/TX completion、budget、slot、backpressure 和 flush。
- Non-goals: 真板 reset、多 hart、长稳压力和数据驱动优化。
- Workload: transport 适配、真板队列服务、双向压力和 QEMU 契约回归。
- Stable baseline: 真板双向异步收发无 lost wakeup、descriptor 双重所有权或永久 Pending。
- Verification boundary: RX/TX burst、queue full、drop、occupancy 和 flush 均可观测。
- Diagnostic boundary: 失败限制在目标 transport 适配、真板 IRQ/DMA 或既有异步契约回归。
- Split signals: RX 与 TX 依赖不同硬件能力，且任一方向可独立成为后续稳定前置。
- Related changes: None

### MS13：目标板单 CPU/hart 恢复语义

- Status: planned
- Outcome: 真板 link flap 和设备 reset 下保持 generation 与资源回收正确。
- Rationale: 先将 QEMU 恢复契约迁移到真板，再引入多 hart 和长稳压力。
- Dependencies: MS12
- Scope: T22；link flap、设备 reset、stale completion 和等待者错误传播。
- Non-goals: 多 hart、长时间 soak 和性能优化。
- Workload: 真板故障注入、设备控制面、DMA quiesce 和生命周期证据。
- Stable baseline: reset 前后对象不混用，设备和软件 ownership 可重新建立。
- Verification boundary: 无重复回收、永久 Pending、静默丢包或 reset 后 DMA 越界。
- Diagnostic boundary: 失败限制在真板 reset、DMA quiesce、generation 或 link 状态。
- Split signals: link 恢复与完整设备 reset 出现不可共享的生命周期和验证边界。
- Related changes: None

### MS14：目标板多 CPU/hart 稳定性与性能基线

- Status: planned
- Outcome: 真板多 hart 异步网络在组合压力和长时间运行下形成可复现的稳定性与性能基线。
- Rationale: T24 只提供 T23 的完成证据和后续优化输入，不单独形成产品能力。
- Dependencies: MS13
- Scope: T23-T24；queue affinity、跨 hart wake、双向压力、ring full、reset 交错和 soak。
- Non-goals: batching、moderation、offload、zero-copy 和 multiqueue。
- Workload: 真板并发、长稳测试、指标采集、环境固定和证据整理。
- Stable baseline: 长时间运行无 stall，drop、p99、occupancy、IRQ 和 CPU 指标可复现。
- Verification boundary: 真板多 hart 证据独立保存，每项 ordering 有角色说明。
- Diagnostic boundary: 失败限制在跨 hart 同步、真板调度、恢复交错或稳定性退化。
- Split signals: SMP 正确性与 soak 环境分别需要长期独立交付，且前者已能成为后续稳定基线。
- Related changes: None

### MS15：首个数据驱动优化闭环

- Status: planned
- Outcome: 关闭 MS14 数据确认的一个主要瓶颈，并建立不退化正确性的 A/B 基线。
- Rationale: T25 包含多个独立故障域；每个 milestone 只接纳一个有数据支持的优化方向。
- Dependencies: MS14；指标达到明确触发条件
- Scope: T25 中首个被数据触发的内聚候选，例如 batch、moderation、offload、zero-copy 或 multiqueue。
- Non-goals: 同时打包多个独立优化；无数据时预先实现候选能力。
- Workload: 瓶颈归因、单项实现、A/B 测量、正确性回归和收益记录。
- Stable baseline: 一个优化方向有可复现收益，且 MS14 正确性 Gate 不退化。
- Verification boundary: 同环境独立 A/B；无触发数据时记录 SKIPPED，不实施。
- Diagnostic boundary: 失败限制在被选中的单项优化及其直接交互面。
- Split signals: 两个或更多独立瓶颈同时达到触发条件；为后续候选创建 MS16+。
- Related changes: None

Roadmap 共同 Non-goals：

- I13 的 PCI 兼容性不进入 MMIO 主线。
- I14-I15 只有在自动化 Gate 明确触发并获批后才进入新的 milestone。
- 不引入 Embassy executor、完整 embassy-net 或用户态 mmap RX/TX ring。
- QEMU 证据不替代目标板 DMA、cache、PHY/链路、IRQ、SMP 或性能证据。
- Milestone 不替代后续 Plan 的 BDD、RTM、Task Contract 和测试设计。

---

## UART 文档已归档

UART 文档已归档；q17 multi-hart SMP 验证 deferred（task 6.1 未完成）。完整任务见 `uart-lichee` 分支。归档载体见 `openspec/changes/archive/2026-07-25-cleanup-uart-docs/`。

## 活跃 Change

MS01-MS03 与 MS16 已归档。当前活跃 change：`ms04-qemu-async-rx-queue-baseline`（Gate 1 与 Gate 2 已批准，2026-08-10）。iteration 000 实施中：T1-T3.1 完成（依赖本地化、NetQueueControl 契约、EVENT_IDX 修复、critical-section restore），T4-T8 未开始。详见 `openspec/changes/ms04-qemu-async-rx-queue-baseline/iterations/000-initial.md` Act Response。
