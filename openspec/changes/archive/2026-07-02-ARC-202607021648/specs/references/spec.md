# Spec Delta: references — ARC-202607021648

## REMOVED Requirements

### Requirement: Completed planning references move out of active reference index

References to completed planning/blocker documents SHOULD be compressed out of the active reference index once final specs and archived changes exist.

#### Scenario: Looking for current Lichee benchmark guidance

- **WHEN** developers need current Q19C guidance
- **THEN** they MUST use R10 and the active Q19C change rather than old Q19B planning/blocker references.

## 压缩保留（Compress-Archive 区）

### R8 / R9 (Compress-Archive, Q19B plan and blockers)

- **状态**: 已完成历史引用。Q19B plan/blocker docs helped reach final userbench; final facts now live in `lichee-d1-benchmark` spec and Q19B archived change.
- **当前入口**: R10 for Q19C, plus `openspec/changes/archive/2026-07-02-q19b-lichee-d1-benchmark/`.
- **恢复条件**: 需要复盘 Q19B 从阻塞到完成的历史路径时恢复。
