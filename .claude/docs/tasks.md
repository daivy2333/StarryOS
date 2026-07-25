# tasks.md — 任务追踪

> 最后更新: 2026-07-25 | 分支: net-k3 | grep: `<!-- N{编号} -->`
> UART 阶段 (Q0-Q32) 全部完成并归档 (ARC-202607251326)。NIC 开发 N0-N5 已规划。

---

## 当前: 异步 NIC 开发 (net-k3)

### NIC 路线图

| Milestone | 目标 | Gate | 状态 |
|-----------|------|------|------|
| **N0** | 基线与契约 | axnet-ng/axdriver 路径梳理、QEMU 首发设备确认、同步基线固化 | ⏳ 待启动 |
| **N1** | QEMU virtio-net 异步 MVP | queue task + stack runner + driver readiness + token；无 busy loop/lost wakeup | ⏳ 待 N0 |
| **N2** | 压力与恢复 | burst/bidirectional/ring full & empty/reset/link flap/cancel/long soak | ⏳ 待 N1 |
| **N3** | SMP 与 multiqueue | 跨 hart 唤醒、queue affinity、per-queue lock | ⏳ 待 N2 |
| **N4** | VisionFive2 DWMAC 真板 | DMA/cache/barrier/PLIC/clock；稳定收发 | ⏳ 待 N3 |
| **N5** | 数据驱动优化 | batching/moderation/offload/zero-copy；按指标决策 | ⏳ 待 N4 |

### N0: 基线与契约 ⏳

<!-- N0.1 --> - [ ] 梳理 StarryOS 当前 axnet-ng/axdriver 调用路径
<!-- N0.2 --> - [ ] 确认 QEMU 首发设备（virtio-net legacy / modern MMIO / PCI）
<!-- N0.3 --> - [ ] 固化同步基线和计数器（吞吐、IRQ、丢包）
<!-- N0.4 --> - [ ] 写出 descriptor ownership、IRQ rearm、backpressure 契约
<!-- N0.5 --> - [ ] Gate N0: 路径、指标和失败注入可复现

---

## UART 阶段 (Q0-Q32) — 已归档

<!-- arc: ARC-202607251326 --> UART 阶段全部完成 (2026-07-25)。里程碑摘要保留如下，完整任务见 `uart-lichee` 分支与 `openspec/changes/archive/`。

### 里程碑摘要

| Milestone | 目标 | 状态 |
|-----------|------|------|
| Q0-Q18 | Spike/驱动/VFS/Shell/性能/文档/QEMU修复/平台解耦 | ✅ |
| Q19-Q23 | D1 bring-up/benchmark/决策（Q21/Q22 取消，Q23 决策完成） | ✅ |
| Q24 | VisionFive2 multi-hart revalidation | ⏳ 等待硬件 |
| Q25 | DMA / 高波特率决策 | ⏳ 等待数据 |
| Q26 | 维护性清理 | ✅ 已归档 |
| Q27a/Q27 | readiness 薄接口 + TX backpressure MVP | ✅ 已归档 |
| Q28 | AsyncUartWriter writer 契约收敛 | ✅ 已归档 |
| Q29 | AsyncUartReader consumer 契约审计 | ✅ 已归档 |
| Q30 | TX 多逻辑 producer 工业化语义 | 🧊 证据触发 |
| Q31/Q32 | Async/Console CPU efficiency D1 benchmark | ✅ 已归档 |

### 跨模块保留经验

UART 开发中验证的方法论已提取至 M/D/K 保留条目，供 NIC 开发参考：
- M01/M03: async runtime + ring buffer 模式
- M35/M36: 并发分流 + NIC 分层架构
- M39/K16: SMP 内存序规则
- K01/K03/K04: ISR 极简 / poll_io / AtomicWaker 模式
- K09: Embassy 选型边界
- K23/K24/K25/K26: io_uring 映射 / 并发矩阵 / SPSC 边界 / UART→NIC 迁移
- K21: 真板验证分层

### 活跃 Change

- `q17-smp-memory-ordering`: 1 deferred task (6.1 multi-hart SMP stress, 需 Q24/VF2)
