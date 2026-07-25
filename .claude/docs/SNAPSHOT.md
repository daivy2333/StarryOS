# SNAPSHOT.md — 项目快照

> Last updated: 2026-07-25
> Branch: net-k3 — 异步 NIC 开发主线；UART 文档已归档 (cleanup-uart-documentation-system, 2026-07-25)；q17 multi-hart SMP 验证 deferred

## 项目概览

- **项目**: StarryOS — 基于 RISC-V 的宏内核 OS（Rust / ArceOS 组件化架构）
- **技术栈**: Rust nightly-2026-02-25 / RISC-V 64-bit / ArceOS 0.3.0-preview.2 / `axtask::future`
- **构建**: Makefile (`make build`, `make run`)
- **测试**: QEMU virt（当前）；VisionFive2（后续真板）
- **格式化/Lint**: `cargo fmt` + `cargo clippy`
- **源码目录**: `kernel/`, `crates/smoltcp/`, `crates/uart_16550/`

## 当前分支

`net-k3`（从 `uart-lichee` 分出）— 异步 NIC 开发。UART 文档已归档；q17 multi-hart SMP 验证 deferred（task 6.1 未完成）。

## 当前待推进

- **T01 可进入 Plan**: smoltcp 0.13.1、本地 axnet、TCP listen/accept 同步基线
- **T02-T06**: QEMU VirtIO PCI 见证、queue task、packet slot、stack runner、socket readiness
- **T07-T09**: reset generation、VirtIO MMIO parity、SMP
- **T10-T12**: VisionFive2 平台、DWMAC 收发、真板恢复和压力
- **T13**: 由数据触发 batching、moderation、offload、zero-copy 和 multiqueue

## 关键事实

| 主题 | 结论 |
|------|------|
| Async runtime | `axtask::future` + `embassy-sync::AtomicWaker`，禁止 embassy-executor |
| Protocol baseline | 本地 smoltcp 0.13.1 是目标版本；T01 先消除 axnet 的 `RxToken::preprocess` 依赖 |
| QEMU device | 首个异步 IRQ 路径使用 VirtIO PCI；MMIO parity 在 T08 |
| ISR 原则 | 最小化：读 cause → ack/mask → wake → 返回；数据搬运在任务上下文 |
| NIC 架构 (M36) | ISR → queue task (budget) → stack runner → socket readiness，4 层分离 |
| NIC 决策 (D20) | 保留 axnet-ng、smoltcp、axpoll、axtask；不引入 Embassy executor |
| UART→NIC 迁移 (K26) | ISR/waker/backpressure/completion 可迁移；字节 ring→DMA descriptor |
| SMP 内存序 (M39) | 按语义选 Ordering，不按架构分叉 |
| PLIC/Clock (M37/M38) | VF2 bring-up: trust-u-boot 保留 PLIC+Clock，init_primary/percpu 分离 |
| OS 接口 (M14) | 2-trait 最小接口 (`OsRuntime` + `OsWakerSet`)，只保留实际调用代码 |
| SPSC 边界 (K25) | unsafe unique constructor + crate-private mutation + exactly-once startup |

## OpenSpec 体系

| 域 | 条目数 | 备注 |
|----|--------|------|
| `openspec/specs/project-model/` | 9 (M01-M39) | M03/M33/M35 已归档 |
| `openspec/specs/decisions/` | 4 (D01-D21) | D03 已归档 |
| `openspec/specs/knowledge/` | 10 (K01-K27) | K09 收紧；K23/K24 已归档 |
| `openspec/specs/references/` | 活跃 10 | R14、R23-R26、R38-R42 |
| `openspec/specs/improvements/` | 1 (I06) | I05/I12 已归档；I12 通用规则迁入 quality-gate-baseline |
| `openspec/changes/` | 无活跃 change | 2026-07-25 归档: cleanup-uart-documentation-system(+carrier cleanup-uart-docs)、q17-smp-memory-ordering(deferred)、ARC-202607251326 |
| `.claude/analysis/` | 7 | 网络总览、交付估算、4 NIC 专题和 1 VF2 专题 |
| `.claude/runbooks/` | 3 | benchmark/build 类已归档 (4 项 in `_archive/`) |

## 证据文件

- NIC 证据待 N1+ 建立
- UART 阶段证据已全部归档至 `openspec/changes/archive/2026-07-25-cleanup-uart-docs/`

## 迁移记录

2026-07-25：`cleanup-uart-documentation-system` — UART 文档体系清理：主载体位于 `openspec/changes/archive/2026-07-25-cleanup-uart-docs/`；q17 与旧 ARC 分别保存在各自归档目录。活跃文档只保留 OS/NIC/VF2 和通用方法。
2026-07-25：`net-k3` 分支从 `uart-lichee` 分出。UART 专属条目标记为归档，载体收编至 `openspec/changes/archive/2026-07-25-arc-202607251326/`。
2026-07-20：旧体系 spec 迁移至 M/D/K/R/I。Migration carrier: `openspec/changes/archive/mig-20260720-legacy-specs/`。
