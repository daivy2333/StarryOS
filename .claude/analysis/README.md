# Analysis Index

> Last updated: 2026-08-09
> UART 文档体系已归档至 `openspec/changes/archive/2026-07-25-cleanup-uart-docs/analysis/`。活跃分析只保留当前 NIC 架构、QEMU 和目标板方法。

新 session 先读 `async-network-project-overview.md`，再按当前 milestone 进入专题文档。

## Active

- `async-network-project-overview.md` — 网络开发总览；当前跨 session 入口。
- `starryos-network-development-strategy.md` — 当前代码调用链、smoltcp 兼容和实施分片。
- `starryos-device-specific-irq-waker-architecture.md` — UART 与 NIC 设备专属 handler、waker 所有权和迁移边界。
- `arceos-async-network-driver-analysis.md` — ArceOS 异步网卡驱动分析（NIC 硬件参考）。
- `embassy-network-module-evaluation.md` — Embassy 网络模块评估（NIC 接口参考）。
- `starryos-async-network-roadmap.md` — StarryOS 异步高性能网卡路线图（NIC N0-N5 Gate）。
- `arceos-true-board-validation.md` — ArceOS 真板验证方法（NIC VF2 bring-up 适用）。

## Archived

- `_archive/starryos-network-delivery-estimate.md` — 旧 PCI-first、VF2/DWMAC 固定路线的历史工期估算。
- `_archive/starryos-network-knowledge-gaps.md` — 旧 T01-T13、PCI-first、VF2/DWMAC 分组的历史知识缺口。
- UART 阶段分析恢复路径见 `_archive/README.md` 和 `openspec/changes/archive/2026-07-25-cleanup-uart-docs/analysis/`。
