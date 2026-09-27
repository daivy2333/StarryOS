# StarryOS 网络交付与工期估算

> Project: StarryOS
> Branch: net-k3
> Date: 2026-07-25
> Estimate baseline: tasks T01-T13 at `8558d836751e7ad2724809b03233ee8257e9a53a`
> Status: [ARCHIVED 2026-08-09] — PCI-first 与 VF2/DWMAC 固定路线已被 D22 和 2026-08-09 roadmap 纠正；本文数字不得作为当前承诺
> See also: [网络总览](../async-network-project-overview.md)、[实施探索](../starryos-network-development-strategy.md)、[任务状态](../../docs/tasks.md)

本文保留旧路线的工作量和资源假设，用于追溯估算来源。任务状态以 tasks 为准，技术边界以 R23、R25、R41 和 M/D/K 为准。QEMU MS04-MS08 的范围继续有效；目标板确定并完成板级事实 Gate 前，不对 MS09-MS15 沿用本文的 VF2 工期。

## 估算口径

基准模型是一名全职工程师，具备 Rust、内核、异步驱动和 RISC-V 调试经验。人周包含设计、测试见证、实现、Review、故障注入和 Evidence 整理。

估算成立需要：

- T10 前可持续使用 VisionFive2。
- 早期串口和 U-Boot 恢复路径可用。
- 本地 smoltcp、Embassy 和 ArceOS 基线不发生不兼容变化。
- 不在 T01-T12 内增加新板卡、热插拔、namespace 或用户态零拷贝。
- 上游 crate 本地化不等待外部合并。

不包含：

- 硬件采购和物流等待。
- 长期上游 review。
- 新 PHY 或新网卡型号。
- 发布级安全审计。
- T13 之外的性能专项。

## 阶段工作量

| 阶段 | 任务 | 工作内容 | 人周 |
|---|---|---|---:|
| 依赖与基线 | T01-T02 | smoltcp/axnet 兼容、同步回归、PCI IRQ 和计数器 | 3-5 |
| QEMU 异步路径 | T03-T06 | queue task、packet slot、stack runner、socket readiness | 6-10 |
| 恢复与 SMP | T07-T09 | generation、故障恢复、MMIO parity、跨 hart 同步 | 5-8 |
| VF2 与 DWMAC | T10-T12 | 平台、PLIC/PHY、DMA/cache、收发和压力 | 7-14 |
| 优化 | T13 | batching、moderation、zero-copy、multiqueue、offload | 2-6+ |

T01-T12 合计 21-37 人周。T13 后置且由数据触发，不计入 VF2 稳定收发的基础范围。

## Milestone 估算

| ID | 交付物 | 主要不确定性 | 人周 |
|---|---|---|---:|
| T01 | smoltcp 0.13.1 与本地 axnet 基线 | listener/backlog 适配 | 2-3 |
| T02 | QEMU VirtIO PCI 见证 | IRQ handler 注入点 | 1-2 |
| T03 | queue control 和 owner task | registry crate 本地化范围 | 2-3 |
| T04 | 有界 RX/TX slot | buffer 生命周期和背压 | 1-2 |
| T05 | stack runner | timer、software、device wake 合流 | 2-3 |
| T06 | socket readiness bridge | 多 waiter 和 64-slot 边界 | 1-2 |
| T07 | reset 与恢复 | late completion 和取消竞态 | 2-3 |
| T08 | VirtIO MMIO parity | 平台 IRQ facts | 1-2 |
| T09 | SMP | affinity、ordering 和真多 hart 见证 | 2-3 |
| T10 | VF2 平台 B0-B4 | feature、clock/reset、PHY、PLIC | 2-4 |
| T11 | DWMAC B5-B6 | DMA 地址、cache 和 descriptor | 3-6 |
| T12 | 真板 B7 | link/reset、压力和长时间运行 | 2-4 |
| T13 | 数据驱动优化 | 瓶颈位置和硬件能力 | 2-6+ |

逐项范围用于排期，不应机械相加替代阶段区间。部分测试基础设施会跨 milestone 复用，部分硬件故障会增加返工。

## 日历周期

单人全职的正常区间：

| 结果 | 累计周期 |
|---|---:|
| smoltcp 同步基线 | 2-4 周 |
| 可使用的 QEMU PCI 异步 MVP | 2-3 个月 |
| QEMU PCI/MMIO、恢复和 SMP | 3-5 个月 |
| VF2 DWMAC 稳定收发 | 5-9 个月 |
| 完成有数据支持的 T13 | 6-10 个月 |

“可使用的 MVP”不等于 T03-T06 全部压力 Gate 已关闭。T06 完成前不得把 demo 视为可靠性结论。

## 两人协作模型

两人投入不能把周期减半。T01-T08 存在顺序依赖，且同一 queue ownership 边界不适合多人并行修改。

可并行部分：

| 工程师 A | 工程师 B |
|---|---|
| smoltcp/axnet、queue、stack runner | host tests、QEMU harness、pcap 和故障注入 |
| socket readiness 和 reset | VF2 平台事实、启动脚本和寄存器检查 |
| SMP ownership | DWMAC PAC、PHY 和 DMA 证据整理 |

两名有经验工程师的正常日历区间为 3.5-6 个月。前提是接口边界由一人负责，T10 前硬件准备完成。

## 复估点

| 复估点 | 获得的信息 | 可收窄的范围 |
|---|---|---|
| T01 完成 | listener 方案和本地化规模 | T02-T06 |
| T02 完成 | PCI IRQ、queue 和 feature 事实 | T03-T09 |
| T06 完成 | 异步路径复杂度和压力结果 | T07-T09 |
| T10 完成 | VF2 clock/reset/PHY/PLIC 状态 | T11-T12 |
| T11 完成 | DMA/cache 和 descriptor 行为 | T12-T13 |
| T12 完成 | 真板瓶颈与稳定性 | T13 |

每个复估点记录已耗人周、剩余风险和新的区间。不得只移动目标日期而不更新假设。

## 进度风险

| 风险 | 影响 | 缓解 |
|---|---|---|
| `RxToken::preprocess` 兼容复杂 | T01 延长并阻塞全链路 | 先建立 listen/accept 见证 |
| driver seam 需要本地化多个 crate | T03 扩大 | 保持同步兼容面，按注入点拆分 |
| MMIO 缺少 IRQ facts | T08 阻塞 | PCI 路径先完成，不混入 MVP |
| smoltcp 单槽 waker 覆盖 waiter | T06 可靠性失败 | 使用 axpoll bridge 和 overflow 计数 |
| reset 迟到 completion | T07 出现所有权故障 | generation 和 fault injection |
| VF2 寄存器不可访问 | T10/T11 停滞 | 停在 B2，检查 mapping、clock、reset |
| DMA/cache 在 QEMU 未暴露 | T11 延长 | 真板按 descriptor 层取证 |
| PHY 或 PLIC 只能触发一次 | T10/T11 延长 | 分开 claim、cause、ack、complete |
| 优化缺少数据 | T13 无法立项 | 标记 SKIPPED，并保留基线 |

## 完成口径

工期完成以 Gate 为准，不以代码写完为准：

- 对应 change 的 BDD、RTM 和任务完整。
- 每个任务有测试见证。
- QEMU、SMP 和真板证据分开保存。
- 所有 blocker 和跳过项有原因。
- OpenSpec validate 通过。
- tasks 状态由 docs-maintainer 在收尾时同步。

当前没有活跃 change。T01 是下一项可进入 Plan 的工作。
