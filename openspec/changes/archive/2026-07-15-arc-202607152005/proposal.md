# Proposal: arc-202607152005 — UART 架构历史决策归档

## Why

异步 UART 已完成 crate 提取、两 trait OS 抽象以及 Q21/Q22/Q23 路线收敛。active architecture 仍保留三条已被替代或仅适用于早期阶段的强制性决策，容易把历史约束误读为当前规范。本次归档完整保留原文，同时用墓碑和交叉引用维持恢复能力。

## What Changes

- Archive：旧 ADR-025/027 “kernel 层独立实现”分配编号 A063，由 ADR-033/ADR-036 取代其实现边界。
- Archive：旧 ADR-028 “Q2 copier/Console 互斥”分配编号 A064；Q2/Q3 阶段已结束，单一 FIFO drainer 教训仍由 learned 与 Q29 承接。
- Archive：A056 原始 Q21/Q22/Q23 排期；当前排期由 A058 取代，Q24 multi-hart 边界由 A062 继续约束。
- In-place：ADR-004、ADR-006 和 R14 添加 STALE 提示；references 中两份已移动分析文档补编 R21/R22 并改为归档路径；CLAUDE.md 仅添加 review 提示。

## 归档条目映射

| 源条目 | 归档动作 | 替代/保留入口 | 恢复条件 |
|---|---|---|---|
| legacy ADR-025/027 → A063 | Archive | ADR-033、ADR-036 | 需要回查 kernel-only 实施阶段或早期 copier 架构时恢复 |
| legacy ADR-028 → A064 | Archive | L123、A062/Q29 | 需要回查 Q2/Q3 Console 与 copier 竞争原文时恢复 |
| A056 | Archive | A058；A062/Q24 | 需要回查 user ring/completion 原始排期时恢复 |

## Cross-reference Maintenance

- A014/A015、A020/A021 的替代说明指向 A063 归档墓碑。
- L123 指向 A064 归档墓碑。
- tasks Q20、optimization Q21/Q22 和 A058 中的 ADR-056 引用保留，并标明已归档。
- R21/R22 只修正分析索引；分析文件已在 `_archive/`，无需再次移动。

## Recovery

从 `openspec/changes/archive/2026-07-15-arc-202607152005/specs/architecture/spec.md` 的“完整保留”区复制目标条目回 active architecture，移除相应墓碑并更新源文档末尾 arc 指引，然后运行 `openspec validate --specs` 与 `openspec validate --changes`。
