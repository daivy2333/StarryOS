## Purpose

保存从活跃任务路线移出的 T01–T06、MS01–MS04 和 MS16 完整原文。

## ADDED Requirements

### Requirement: 长期完成任务原文可恢复

Carrier MUST 保留以下六个任务行：

```markdown
| <!-- T01 --> T01 | smoltcp/axnet 同步基线 | 纳入本地 smoltcp 0.13.1；本地化 axnet；移除 `RxToken::preprocess` 私有依赖 | TCP listen/accept、UDP、nonblocking 和 poll 与当前同步行为一致 | 无 | ✅ 完成 |
| <!-- T02 --> T02 | QEMU I/O 边界见证 | 固化串口、网络和 hostfwd 的独立路径；固定 VirtIO-MMIO 启动签名 | 无 hostfwd 仍可进 shell；MMIO net/block 可探测；串口成功不计网络成功 | T01 | ✅ 完成 |
| <!-- T03 --> T03 | MMIO 轮询网络基线 | 保持轮询驱动；建立明确 guest 服务和宿主端到端用例 | ARP/ICMP、UDP、TCP 5555 各有独立见证；空闲 CPU 只作基线记录 | T02 | ✅ 完成 |
| <!-- T04 --> T04 | MMIO IRQ 事实 | 解析设备地址、PLIC IRQ、claim/ack/rearm；只增加计数器 | 注入 RX/TX 事件时 IRQ 可重复增长；错误 IRQ 不触碰异步队列 | T03 | ✅ 完成 |
| <!-- T05 --> T05 | IRQ 唤醒原语 | 建立 `NetQueueControl`、AtomicWaker 和 register-recheck；ISR 不搬包 | event-before-register、register-during-event、spurious IRQ 无 lost wakeup | T04 | ✅ 完成（MS04） |
| <!-- T06 --> T06 | QEMU 异步 RX | queue task 只处理 RX reap/refill 和 budget；TX 保持基线 | 单向 RX burst 无 busy loop、饿死或 descriptor 泄漏；budget 可观测 | T05 | ✅ 完成（MS04 核心 Gate；兼容性重复项按用户授权豁免） |
```

Carrier MUST 同时保留以下五个 milestone 条目：

```markdown
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

- Status: completed — 2026-08-12，核心运行时 Gate PASS；完整 compatibility 重复项按用户授权豁免
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
- Related changes: `ms04-qemu-async-rx-queue-baseline`（归档于 `openspec/changes/archive/2026-08-12-ms04-qemu-async-rx-queue-baseline/`；10 iterations，25/25 tasks，最终 Review: PASS WITH EXPLICIT USER WAIVER）。Runbook `ms04-qemu-async-rx-core-evidence.md` (R51) 已发布。
```

#### Scenario: 恢复长期完成任务

- **WHEN** Maintainer 收到经批准的任务或 milestone 恢复请求
- **THEN** MUST 按原编号恢复对应完整行或完整 milestone 条目
- **AND** MUST NOT 因恢复历史条目改变当前 MS08 状态
