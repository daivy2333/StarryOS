# SNAPSHOT.md — 项目快照

> Last updated: 2026-07-29
> Branch: net-k3 — 异步 NIC 开发主线；QEMU 首条路径改为 VirtIO-MMIO；PCI 转为未承诺改进

## 项目概览

- **项目**: StarryOS — 基于 RISC-V 的宏内核 OS（Rust / ArceOS 组件化架构）
- **技术栈**: Rust nightly-2026-02-25 / RISC-V 64-bit / ArceOS 0.3.0-preview.2 / `axtask::future`
- **构建**: Makefile (`make build`, `make run`)
- **测试**: QEMU virt（当前）；VisionFive2（后续真板）
- **格式化/Lint**: `cargo fmt` + `cargo clippy`
- **源码目录**: `kernel/`, `crates/smoltcp/`, `crates/uart_16550/`

## 当前分支

`net-k3`（从 `uart-lichee` 分出）— 异步 NIC 开发。UART 文档已归档；q17 multi-hart SMP 验证 deferred（task 6.1 未完成）。MS01 (smoltcp/axnet 同步基线) 已完成并归档于 `openspec/changes/archive/2026-07-29-t01-smoltcp-axnet-baseline/`。

## 当前待推进

- **MS01**: ✅ 完成 — smoltcp/axnet 同步基线（归档于 `2026-07-29-t01-smoltcp-axnet-baseline`）
- **T02-T05**: QEMU I/O 边界、MMIO 轮询基线、IRQ 事实和唤醒原语
- **T06-T12**: 异步 RX、异步 TX、packet slot、stack、socket、恢复和多 hart
- **T13-T17**: VF2 板级事实、启动、PHY、PLIC 和 DMA/cache
- **T18-T21**: DWMAC 轮询与异步收发
- **T22-T24**: 真板恢复、多 hart 和长稳压力
- **T25**: 由数据触发 batch、moderation、offload、zero-copy 和 multiqueue

## 关键事实

| 主题 | 结论 |
|------|------|
| Async runtime | `axtask::future` + `embassy-sync::AtomicWaker`，禁止 embassy-executor |
| Protocol baseline | 本地 smoltcp 0.13.1 是目标版本；T01 先消除 axnet 的 `RxToken::preprocess` 依赖 |
| QEMU terminal (K31) | `-nographic` 连接 MMIO UART 与宿主终端；5555 只属于网络转发 |
| QEMU device (D22/K32) | 首条异步路径使用 VirtIO-MMIO；当前 feature 合并实际选择 MMIO |
| PCI (I13) | QEMU 支持 PCI device；StarryOS 纯 PCI build/run 尚未通过 |
| ISR 原则 | 最小化：读 cause → ack/mask → wake → 返回；数据搬运在任务上下文 |
| NIC 架构 (M36) | ISR → queue task (budget) → stack runner → socket readiness，4 层分离 |
| NIC 决策 (D20) | 保留 axnet-ng、smoltcp、axpoll、axtask；不引入 Embassy executor |
| Transport 边界 (M41) | probe、IRQ、DMA 属于平台层；异步队列语义不依赖总线 |
| UART→NIC 迁移 (K26) | ISR/waker/backpressure/completion 可迁移；字节 ring→DMA descriptor |
| SMP 内存序 (M39) | 按语义选 Ordering，不按架构分叉 |
| PLIC/Clock (M37/M38) | VF2 bring-up: trust-u-boot 保留 PLIC+Clock，init_primary/percpu 分离 |
| OS 接口 (M14) | 2-trait 最小接口 (`OsRuntime` + `OsWakerSet`)，只保留实际调用代码 |
| SPSC 边界 (K25) | unsafe unique constructor + crate-private mutation + exactly-once startup |

## OpenSpec 体系

| 域 | 条目数 | 备注 |
|----|--------|------|
| `openspec/specs/project-model/` | 10 (M01-M41) | 新增 M41；M03/M33/M35/M40 已归档 |
| `openspec/specs/decisions/` | 5 (D01-D22) | 新增 D22；D03 已归档 |
| `openspec/specs/knowledge/` | 13 (K01-K34) | 新增 K34；K31/K32/K33 已验证 |
| `openspec/specs/references/` | 活跃 11 | R14、R23-R26、R38-R43 |
| `openspec/specs/improvements/` | 4 (I06,I13-I15) | PCI、QEMU 观测和覆盖层未承诺 |
| `openspec/changes/` | 0 个活跃 | MS01 `t01-smoltcp-axnet-baseline` 已归档 |
| `.claude/analysis/` | 8 | 网络总览、交付估算、知识缺口、4 NIC 专题和 1 VF2 专题 |
| `.claude/runbooks/` | 3 | benchmark/build 类已归档 (4 项 in `_archive/`) |

## 证据文件

- K31/K32 已记录 2026-07-27 QEMU 对照结果
- MS01 完成证据：三轮 evidence（000/001/002），14/14 QEMU PASS — 归档于 `openspec/changes/archive/2026-07-29-t01-smoltcp-axnet-baseline/evidence/`
- UART 阶段证据已全部归档至 `openspec/changes/archive/2026-07-25-cleanup-uart-docs/`

## 迁移记录

2026-07-29：MS01 smoltcp/axnet 同步基线完成 — 本地 smoltcp 0.13.1 + 本地 axnet，移除 `RxToken::preprocess` 私有依赖，TCP bind sidecar + listener 512 容量 + egress-until-none + 14/14 QEMU 手测 PASS。Change 归档于 `openspec/changes/archive/2026-07-29-t01-smoltcp-axnet-baseline/`（3 iterations，3 轮 evidence）。
2026-07-27：根据 QEMU 对照验证改用 VirtIO-MMIO 主线。任务由 T01-T13 拆分为 T01-T25；PCI 转为 I13。
2026-07-25：`cleanup-uart-documentation-system` — UART 文档体系清理：主载体位于 `openspec/changes/archive/2026-07-25-cleanup-uart-docs/`；q17 与旧 ARC 分别保存在各自归档目录。活跃文档只保留 OS/NIC/VF2 和通用方法。
2026-07-25：`net-k3` 分支从 `uart-lichee` 分出。UART 专属条目标记为归档，载体收编至 `openspec/changes/archive/2026-07-25-arc-202607251326/`。
2026-07-20：旧体系 spec 迁移至 M/D/K/R/I。Migration carrier: `openspec/changes/archive/mig-20260720-legacy-specs/`。
