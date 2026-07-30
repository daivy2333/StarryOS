# Analysis Index

> Last updated: 2026-07-29
> UART 文档体系已归档至 `openspec/changes/archive/2026-07-25-cleanup-uart-docs/analysis/`。活跃分析只保留 NIC 和 VF2 相关内容。

新 session 先读 `async-network-project-overview.md`，再按当前 milestone 进入专题文档。

## Active

- `async-network-project-overview.md` — 网络开发总览；当前跨 session 入口。
- `starryos-network-delivery-estimate.md` — T01-T13 人周、日历周期、人员模型和复估点。
- `starryos-network-development-strategy.md` — 当前代码调用链、smoltcp 兼容和实施分片。
- `starryos-network-knowledge-gaps.md` — NIC 开发待确认问题、证据与解决判据。
- `starryos-device-specific-irq-waker-architecture.md` — UART 与 NIC 设备专属 handler、waker 所有权和迁移边界。
- `arceos-async-network-driver-analysis.md` — ArceOS 异步网卡驱动分析（NIC 硬件参考）。
- `embassy-network-module-evaluation.md` — Embassy 网络模块评估（NIC 接口参考）。
- `starryos-async-network-roadmap.md` — StarryOS 异步高性能网卡路线图（NIC N0-N5 Gate）。
- `arceos-true-board-validation.md` — ArceOS 真板验证方法（NIC VF2 bring-up 适用）。

## Archived

UART 阶段分析已全部归档。恢复路径见 `_archive/README.md` 和 `openspec/changes/archive/2026-07-25-cleanup-uart-docs/analysis/`。
