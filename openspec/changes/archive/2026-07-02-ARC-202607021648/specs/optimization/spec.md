# Spec Delta: optimization — ARC-202607021648

## REMOVED Requirements

### Requirement: Archived optimization entries are not active roadmap items

Optimization entries that were already adopted, completed, or superseded MUST be removed from the active optimization queue and remain recoverable from this carrier spec.

#### Scenario: Reviewing completed optimization history

- **WHEN** developers need details for an archived optimization item
- **THEN** they MUST read this carrier spec or the linked archived change rather than re-adding it to the active roadmap.

## 压缩保留（Compress-Archive 区）

### O67 / O68 / O70 / O72 / O73 (Compress-Archive, adopted ArceOS learnings)

- **状态**: 已采纳或已蕴含。`axtask::future::timeout` 覆盖 O67；AtomicWaker/Pending-register 不变量覆盖 O68；CodeGraph 规则进入 CLAUDE.md；硬件中断日志和 benchmark 框架已部分/完整采纳。
- **当前入口**: CLAUDE.md CodeGraph 规则、SNAPSHOT 关键发现、benchmark 文档。
- **恢复条件**: 需要重新审计 arceos 借鉴清单时恢复。

### O76 / O77 (Compress-Archive, completed Lichee Q19/Q19B optimization items)

- **状态**: 已完成。Q19 smoke 与 Q19B async UART userbench 均已真板通过并归档。
- **当前入口**: `openspec/specs/lichee-d1-early-smoke/spec.md`、`openspec/specs/lichee-d1-benchmark/spec.md`、Q19/Q19B archived changes。
- **恢复条件**: 需要恢复 active roadmap 中的已完成 Lichee 条目时恢复。

### O56 / O57 / O61 (Compress-Archive, completed Q13.1/LTO short-term optimizations)

- **状态**: 已完成。inline、batch、LTO 的效果已记录；active roadmap 只保留中长期项 O58/O59/O60。
- **当前入口**: Q13.1 archived change、ADR-034、learned L180-L187。
- **恢复条件**: 需要恢复 Q13.1 短期优化表时恢复。
