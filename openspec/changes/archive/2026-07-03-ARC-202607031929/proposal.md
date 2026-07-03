# ARC-202607031929 — 精简 tasks/SNAPSHOT 与历史优化方案

## Why

2026-07-03 `openspec-archivist` 分析发现，7 月 2 日归档后 active 文档仍存在几类膨胀：

- `.claude/docs/tasks.md` 同时承担 milestone、历史流水和经验归档，且出现重复 `Q18.summary`。
- `.claude/docs/SNAPSHOT.md` 保存了大量与 `openspec/project.md`、`CLAUDE.md`、`learned/spec.md`、`architecture/spec.md` 重叠的历史结构和技术栈信息。
- `openspec/specs/optimization/spec.md` 已有 O45/O46/O47 tombstone，但 O45/O46/O47 的详细旧方案正文仍留在 active spec 中。

## What Changes

- Archive: `optimization/spec.md` 中 O45/O46/O47 详细旧方案正文移至本 carrier。
- Compress-Archive: `tasks.md` 中 `关键经验` 历史小节压缩归档。
- Compress-Archive: `SNAPSHOT.md` 中阶段表、当前架构图、项目结构、技术栈、文档体系索引、代码路径速查压缩归档。
- Simplify-Keep: `tasks.md` 和 `SNAPSHOT.md` 只保留当前状态、活跃 milestone、近期完成和下一步。
- Delete: `tasks.md` 重复的 `Q18.summary` 行。

## 归档条目映射表

| 原编号 | 源位置 | 标题 | 类型 | 恢复方式 |
|--------|--------|------|------|----------|
| O45/O46/O47-detail | `openspec/specs/optimization/spec.md` | tcdrain / AtomicWaker / timeout 旧详细方案 | Archive | "恢复 §optimization #O45-detail" |
| TASKS-key-experience | `.claude/docs/tasks.md` | 关键经验历史小节 | Compress-Archive | "恢复 §tasks #key-experience" |
| SNAPSHOT-history-blocks | `.claude/docs/SNAPSHOT.md` | 阶段表、架构图、结构、技术栈、索引、代码路径 | Compress-Archive | "恢复 §snapshot #history-blocks" |

## 排除项

- `q17-smp-memory-ordering`：18/19 tasks，仍有多 hart deferred，不归档。
- `q19c-lichee-full-starryos-benchmark`：活跃 change，不归档。
- `architecture/spec.md` ADR-047~ADR-052：近期且仍被 Q19C 引用，不归档。
- `learned/spec.md` L240~L264：近期 Q19B/Q19C/Q17 活跃知识，不归档。

## 恢复协议

用户说"恢复 §optimization #O45-detail"、"恢复 §tasks #key-experience" 或 "恢复 §snapshot #history-blocks"：

1. 读取本 change 对应 `specs/<domain>/spec.md` 条目。
2. 用精准 Edit 复制回原源文档。
3. 更新源文档末尾 `<!-- arc: ARC-202607031929 -->` 计数或追加 `<!-- restored: ... -->`。
4. 运行 `openspec validate --specs`。
