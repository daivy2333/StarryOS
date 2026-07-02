# ARC-202607021535 — 归档 Q5/Q7/Q8/Q12/Q15 已完成 milestone 详细叙述

## Why

`openspec/specs/optimization/spec.md` 中 Q5/Q7/Q8/Q12/Q15 五个 Q-milestone 详细叙述段（共 ~179 行）与 `tasks.md` Milestone 表 + `SNAPSHOT.md` §关键发现 + `learned/spec.md` L134-L159 + `architecture/spec.md` ADR-039 存在高度重复。所有 Q-milestone 状态已在多处持久化且明确"✅ 已完成"。

继续保留这些详细叙述会导致：(1) spec 文档持续膨胀；(2) 重复信息更新时容易遗漏；(3) 用户浏览 active spec 时被历史 milestone 细节干扰。

## What Changes

- REMOVED: `optimization/spec.md` 中 5 个 Q-milestone Requirement 段（Q5 L9-25 / Q7 L26-58 / Q8 L60-104 / Q12 L296-312 / Q15 L628-694）
- 原文完整保留于本 carrier spec "Archive 区"
- 源文档末尾追加 `<!-- arc: ARC-202607021535 -->` 指引 + 5 条 tombstone 标记
- `tasks.md` / `SNAPSHOT.md` / `learned/spec.md` / `architecture/spec.md` 不变（已含状态信息）
- 恢复协议：用户说"恢复 §optimization #Q{编号}" → 按 `openspec/changes/ARC-202607021535/proposal.md` 恢复协议

## 触发

用户调用 `openspec-archivist` skill，要求"把spec里面几个完成的归一下档"。
archivist Phase 1 分析后用户确认归档范围为 `openspec/specs/optimization/spec.md` 中 5 个已完成的 Q-milestone 大段。

## 归档条目映射表

| 原编号 | 源位置 | 标题 | 类型 | 行数 | 恢复方式 |
|--------|-------|------|------|------|---------|
| Q5 段 | `openspec/specs/optimization/spec.md` L9-25 | Q5 内核态性能优化 — 已完成 | Archive | ~17 | 用户说"恢复 §optimization #Q5" |
| Q7 段 | `openspec/specs/optimization/spec.md` L26-58 | Q7 用户态性能修复 — 已完成 | Archive | ~33 | 用户说"恢复 §optimization #Q7" |
| Q8 段 | `openspec/specs/optimization/spec.md` L60-104 | Q8 驱动引擎打磨 — 已完成 | Archive | ~45 | 用户说"恢复 §optimization #Q8" |
| Q12 段 | `openspec/specs/optimization/spec.md` L296-312 | Q12 Embassy 路径 A — 已完成 | Archive | ~17 | 用户说"恢复 §optimization #Q12" |
| Q15 段 | `openspec/specs/optimization/spec.md` L628-694 | Q15 M0~M4 增量重融合 + Manual QA — 已完成 | Archive | ~67 | 用户说"恢复 §optimization #Q15" |

**总计**：5 段 / ~179 行从 `optimization/spec.md` 移至本 carrier spec。

## 排除项

- **architecture/spec.md A033-A051**：19 条 ADR 全部"已接受/已落地"，是设计基线，不属"完成条目" → Keep
- **learned/spec.md L78-L258**：踩坑档案 + 技巧模式 + API 路径速查，全部当前有效或踩坑参考 → Keep
- **references/spec.md**：依赖/规范/分析索引全部当前有效 → Keep
- **tasks.md / SNAPSHOT.md**：已含 Q-milestone 简短摘要，不重复归档
- **.claude/docs/SNAPSHOT.md line 88** 已有墓碑：`<!-- tombstone: Q0-Q15 sub-tasks --> Archived 2026-06-23`

## 恢复协议

用户说"恢复 §optimization #Q{编号}"：

1. `grep "Q{编号}" openspec/changes/ARC-202607021535/specs/optimization/spec.md` 定位原文
2. 用 Edit 精准复制回 `openspec/specs/optimization/spec.md` 原位置
3. arc 指引计数 -1 + 追加 `<!-- restored: Q{编号} 2026-MM-DD -->` 注释行
4. 验证：`grep "Q{编号}" openspec/specs/optimization/spec.md` 应在源文档中可定位

## 状态信息保留

- `tasks.md` Milestone 表已含 Q5/Q5.1/Q5.2/Q7/Q8/Q12/Q15 ✅ 标记 + 日期 + 关键内容简述
- `SNAPSHOT.md` line 30-53 含 Q-milestone 关键发现 + 当前架构图（Q15 M0~M4 已验证）
- 关键设计决策留在 `architecture/spec.md`（ADR-032 ADR-033 ADR-035~A039 + ADR-044~A051）
- 关键经验教训留在 `learned/spec.md`（L134-L145 Q7 性能教训、L150-L159 Q8/Q13 教训、L201-L211 Q15 教训）

## 归档置信度

**HIGH** — 5 段全部含明确的"✅ 已完成/已落地"标记，时间跨度 2026-05 至 2026-06-25，
子任务已归档至 `openspec/changes/archive/2026-06-11-q8-driver-polish/` 与
`openspec/changes/archive/2026-06-15-q12-embassy-path-a/`，tasks.md 已有墓碑。