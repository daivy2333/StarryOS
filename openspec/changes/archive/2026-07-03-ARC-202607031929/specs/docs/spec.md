# Spec Delta: docs — ARC-202607031929

## REMOVED Requirements

### Requirement: TASKS-key-experience historical section

`tasks.md` 的 `关键经验` 小节已从 active 任务追踪中移除。该段内容已经由 `learned/spec.md`、`architecture/spec.md`、`optimization/spec.md` 与 archived changes 覆盖。

#### Scenario: Restore tasks key experience

- **WHEN** 开发者需要恢复旧 `tasks.md` 关键经验小节
- **THEN** MUST use `openspec/changes/archive/2026-07-03-ARC-202607031929/specs/docs/spec.md`
- **AND** restore via the proposal mapping table.

### Requirement: SNAPSHOT-history-blocks compressed

`SNAPSHOT.md` 的阶段表、当前架构图、项目结构、技术栈、文档体系索引、关键代码路径速查已压缩归档。当前快照只保留状态、近期完成、当前待推进和最小关键事实。

#### Scenario: Restore snapshot historical blocks

- **WHEN** 开发者需要恢复旧快照结构/技术栈/路径表
- **THEN** MUST use this carrier spec and the source git history.

---

## 压缩保留（Compress-Archive 区）

### TASKS-key-experience (Compress-Archive, docs 2026-07-03)

`tasks.md` 旧 `关键经验` 小节包含：已验证模式 9 条、Q7 已解决问题、2026-06-11 审计待解决问题、已修正误判、方向 A M3 失败原因、2026-06-01 性能分析历史问题。状态：已由 learned/architecture/optimization/archive 覆盖，active tasks 不再保留。

### SNAPSHOT-history-blocks (Compress-Archive, docs 2026-07-03)

`SNAPSHOT.md` 旧历史块包含：Q0~Q23 阶段表、Q15 当前架构 ASCII 图、项目结构、技术栈、文档体系索引、关键代码路径速查。状态：这些信息分散保存在 `openspec/project.md`、`CLAUDE.md`、`tasks.md` milestone 表、`architecture/spec.md` ADR 与 `learned/spec.md`，active SNAPSHOT 压缩为当前态。
