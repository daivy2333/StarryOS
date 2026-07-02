# Spec Delta: tasks — ARC-202607021648

## REMOVED Requirements

### Requirement: Completed Q18 and Q19 task details are archived

Completed Q18 and Q19 detailed task checklists MUST be removed from the active task view once archived OpenSpec changes and main specs contain the same evidence.

#### Scenario: Looking up Q18/Q19 details

- **WHEN** developers need Q18/Q19 implementation details
- **THEN** they MUST use the archived changes and main specs linked from the compact task summary.

## 压缩保留（Compress-Archive 区）

### Q18 detailed tasks (Compress-Archive, tasks.md)

- **状态**: 已完成并归档。Q18 platform descriptor + early console 细节保留在 `openspec/changes/archive/2026-06-28-q18-platform-descriptor-early-console/` 与 `platform-descriptor-early-console` spec。
- **源内容**: tasks.md Q18.1-Q18.6。
- **恢复条件**: 需要把 Q18 子任务重新展开到 active tasks 时恢复。

### Q19 detailed tasks (Compress-Archive, tasks.md)

- **状态**: 已完成并归档。Q19 Lichee smoke 细节保留在 `openspec/changes/archive/2026-07-02-q19-lichee-d1-early-smoke/` 与 `lichee-d1-early-smoke` spec。
- **源内容**: tasks.md Q19.1-Q19.13。
- **恢复条件**: 需要把 Q19 子任务重新展开到 active tasks 时恢复。
