# SNAPSHOT.md — 项目快照

> Last updated: 2026-07-25
> Branch: net-k3 — 异步 NIC 开发主线；UART 阶段 (Q0-Q32) 全部完成并归档 (ARC-202607251326)

## 项目概览

- **项目**: StarryOS — 基于 RISC-V 的宏内核 OS（Rust / ArceOS 组件化架构）
- **技术栈**: Rust nightly-2026-02-25 / RISC-V 64-bit / ArceOS 0.3.0-preview.2 / `axtask::future`
- **构建**: Makefile (`make build`, `make run`)
- **测试**: QEMU virt（当前）；VisionFive2（后续真板）
- **格式化/Lint**: `cargo fmt` + `cargo clippy`
- **源码目录**: `kernel/`, `crates/uart_16550/`

## 当前分支

`net-k3`（从 `uart-lichee` 分出）— 异步 NIC 开发。UART 阶段 (Q0-Q32) 已全部完成并归档到 `openspec/changes/archive/`。UART 专属 spec 条目 (M/D/K/R/I) 已归档到 `ARC-202607251326`。

## 当前待推进

- **N0**: 基线固化 — 梳理 axnet-ng/axdriver 调用路径，确认 QEMU 首发设备，固化同步基线与计数器
- **N1**: QEMU virtio-net 异步 MVP — queue task + stack runner + driver readiness + token
- **N2**: 压力与恢复 — burst/bidirectional/ring full/reset/cancel/long soak
- **N3**: SMP 与 multiqueue — 跨 hart 唤醒、queue affinity
- **N4**: VisionFive2 DWMAC 真板 — DMA/cache/barrier/PLIC/clock
- **N5**: 数据驱动优化 — batching/moderation/offload/zero-copy

## 关键事实

| 主题 | 结论 |
|------|------|
| Async runtime | `axtask::future` + `embassy-sync::AtomicWaker`，禁止 embassy-executor |
| ISR 原则 | 最小化：读 cause → ack/mask → wake → 返回；数据搬运在任务上下文 |
| NIC 架构 (M36) | ISR → queue task (budget) → stack runner → socket readiness，4 层分离 |
| NIC 决策 (D20) | 保留 axnet-ng、smoltcp、axpoll、axtask；不引入 Embassy executor |
| UART→NIC 迁移 (K26) | ISR/waker/backpressure/completion 可迁移；字节 ring→DMA descriptor |
| SMP 内存序 (M39) | 按语义选 Ordering，不按架构分叉 |
| PLIC/Clock (M37/M38) | VF2 bring-up: trust-u-boot 保留 PLIC+Clock，init_primary/percpu 分离 |
| OS 接口 (M14) | 2-trait 最小接口 (`OsRuntime` + `OsWakerSet`)，只保留实际调用代码 |
| SPSC 边界 (K25) | unsafe unique constructor + crate-private mutation + exactly-once startup |

## UART 阶段回顾

Q0-Q32 已全部完成并归档。关键成果：
- D1 真板 async UART 96.6%-99.1% 线速
- 完整 backpressure/writer/reader 契约收敛
- SMP 内存序修复 (Q17, QEMU 完成，multi-hart 待 Q24/VF2)
- 跨模块 async 经验提取为 M/D/K 保留条目

详细历史见 `uart-lichee` 分支和 `openspec/changes/archive/`。

## OpenSpec 体系

| 域 | 条目数 | 备注 |
|----|--------|------|
| `openspec/specs/project-model/` | 12 (M01-M39) | 跨模块约束；UART 专属已归档 |
| `openspec/specs/decisions/` | 5 (D01-D21) | 决策记录；UART 专属已归档 |
| `openspec/specs/knowledge/` | 12 (K01-K27) | 已归档 18 条 UART 专属知识 |
| `openspec/specs/references/` | 活跃 ~10 | 已归档 11 条 UART 专属参考 |
| `openspec/specs/improvements/` | 3 (I05,I06,I12) | 活跃改进；UART 专属已归档 |
| `openspec/changes/` | 活跃: q17-smp-memory-ordering | 归档: Q0-Q32, ARC-202607251326 |
| `.claude/analysis/` | 6 | NIC 分析 4 篇 + 真板验证 + 移植分析 |
| `.claude/runbooks/` | 4 | benchmark-guide, board-bringup-ladder, incremental-merge, regression-gate |

## 证据文件

- `docs/benchmark-report-async.md` — async UART 与 polling Console 交叉对比报告（UART 阶段参考）
- NIC 证据待 N1+ 建立

## 迁移记录

2026-07-25：`net-k3` 分支从 `uart-lichee` 分出。UART 专属条目归档到 `openspec/changes/ARC-202607251326/`。
2026-07-20：旧体系 spec 迁移至 M/D/K/R/I。Migration carrier: `openspec/changes/archive/mig-20260720-legacy-specs/`。
