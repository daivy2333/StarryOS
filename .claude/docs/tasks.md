# tasks.md — 任务追踪

> 最后更新: 2026-07-25 | 分支: net-k3 | grep: `<!-- N{编号} -->`
> UART 文档已归档 (cleanup-uart-documentation-system, 2026-07-25)；q17 multi-hart SMP 验证 deferred。NIC 开发 N0-N5 已规划。

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

## UART 文档已归档

UART 文档已归档；q17 multi-hart SMP 验证 deferred（task 6.1 未完成）。完整任务见 `uart-lichee` 分支。归档载体见 `openspec/changes/archive/2026-07-25-cleanup-uart-docs/`。

### 活跃 Change

无活跃 change。2026-07-25 已归档：`cleanup-uart-documentation-system`（含 carrier `cleanup-uart-docs`）、`q17-smp-memory-ordering`（deferred, task 6.1 未完成）、`ARC-202607251326`。NIC N0 启动时创建新 change。
