# ARC-202607021648 — 精简已替代 ADR、历史 learned、已采纳 optimization、旧 Q19B 引用与状态文档

## Why

用户调用 `openspec-archivist` 要求对文档体系执行精简、归档与清理。分析阶段确认当前膨胀点集中在：

- `architecture/spec.md` 中 ADR-035 已被 ADR-036 明确替代；
- `learned/spec.md` 中 Q13 旧 5-trait API 与 Q19B 计划期/阻塞期条目仍占据主视图；
- `optimization/spec.md` 中已采纳/已完成优化项仍保留在 active roadmap；
- `references/spec.md` 中 Q19B 计划/阻塞分析已转为历史引用；
- `.claude/docs/tasks.md` 和 `SNAPSHOT.md` 中已完成阶段的展开细节与 archived OpenSpec changes、主 specs 高度重复。

## What Changes

- Archive：ADR-035 完整移入 carrier，源文档留下 tombstone。
- Compress-Archive：旧 5-trait API、Q19B 计划期/阻塞期 learned、已采纳/已完成 optimization、Q19B 旧 references、tasks 中 Q18/Q19 详细子任务进入 carrier 压缩保留。
- Simplify-Keep：`SNAPSHOT.md` 历史段压缩为当前状态 + 归档指引，保留关键架构/索引。
- Simplify-Keep：`tasks.md` Q19C 旧 approval 文案改为“规范已完整 / 待实施”，Q18/Q19 详细任务收敛到摘要。

## 归档条目映射表

| 源域 | 原编号/区域 | 动作 | 理由 | 恢复方式 |
|------|-------------|------|------|----------|
| architecture | A035 | Archive | 被 A036 替代，5-trait 结论已失效 | 恢复 A035 |
| learned | L161-L164, L189-L191, L194-L195 | Compress-Archive | CodeGraph 查无 `OsIrq`/`OsSpinNoIrq`/`ArceOsMmio` 等旧 trait/adapter | 恢复 learned 旧 5-trait API |
| learned | L236-L239, L243, L246, L247 | Compress-Archive | Q19B 计划/阻塞期信息已由 L240-L258、ADR-047~051、归档 change 覆盖 | 恢复 Q19B 历史路线 |
| optimization | O67/O68/O70/O72/O73/O76/O77/O56/O57/O61 | Compress-Archive | 已采纳/已完成，不再是 active optimization 队列 | 恢复 optimization 项 |
| references | R8/R9 | Compress-Archive | Q19B plan/blockers 已完成，当前入口转为归档 specs + R10 Q19C | 恢复 R8/R9 |
| tasks | Q18/Q19 detailed sub-tasks | Compress-Archive | 已完成且已有归档 change /主 specs；tasks 主视图只保留摘要 | 恢复 Q18/Q19 tasks |
| snapshot | Q5~Q15 historical expansion | Simplify-Keep | 当前快照应保持当前状态，历史证据已有归档 | 查看本 carrier + 既有 archive |

## 排除项

- `CLAUDE.md`：按 archivist 规则不自驱归档。
- `openspec/changes/q17-smp-memory-ordering`：活跃 change，保留。
- `openspec/changes/q19c-lichee-full-starryos-benchmark`：活跃 change，保留。
- `learned` L259-L261 / ADR-052 / R10：Q19C 当前工作入口，保留。
- `optimization` O63/O64/O66/O69/O71/O48/O49/O50/O54/O55/O58/O59/O60：未完成或仍等待硬件/维护决策，保留。

## 恢复协议

用户说“恢复 A035 / L243 / O67 / R8 / Q19 tasks”：

1. 在 `openspec/changes/archive/2026-07-02-ARC-202607021648/specs/<domain>/spec.md` 中定位条目；
2. 用精准 Edit 复制回源文档合适位置；
3. 删除或更新源文档 tombstone/arc 指引计数；
4. 运行 `openspec validate --specs` 与 `openspec validate --changes`。
