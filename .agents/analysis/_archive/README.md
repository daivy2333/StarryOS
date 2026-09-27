# Analysis Archive Index

> Last updated: 2026-08-09

This directory stores archived analysis documents by batch. Active-path tombstones have been consolidated into this index so the active `.claude/analysis/` tree stays minimal.

## Batches

- `starryos-network-delivery-estimate.md` — [ARCHIVED 2026-08-09] 旧 PCI-first、VF2/DWMAC 固定路线的历史工期估算；当前目标板路线不得沿用其数字。
- `starryos-network-knowledge-gaps.md` — [ARCHIVED 2026-08-09] 旧 T01-T13、PCI-first、VF2/DWMAC 分组的历史知识缺口；当前 Plan 使用 tasks、R23、R25 和 R41。
- `2026-06-24-q0-q15-analysis/` — early async UART, module extraction, Q15/M4, performance, and architecture analysis (14 files).
- `2026-07-04-q19-lichee-analysis/` — Q18/Q19/Q19B Lichee plans, board captures, benchmark logs, and boot backup (8 files + `lichee/` subdir).
- `2026-07-11-q19c-d1-async-uart-closeout/` — Q19C planning, TX optimization, M1/M2/M3 analysis, and D1 board evidence logs after D1 async UART testing ended (8 files + `lichee/` subdir).

## Active-path Tombstones (consolidated 2026-07-04)

> 16 active-path `.md` / `.txt` tombstone files at `.claude/analysis/*.md` and `.claude/analysis/lichee/*` were merged into this table on 2026-07-04. Their content was identical: a `<!-- tombstone: ... -->` marker + new archive path + brief reason. The mapping is preserved here for grep / restore.

### Top-level (`.claude/analysis/<file>` → archive)

| Old active path | Archive location (relative to `.claude/analysis/`) | Reason |
|-----------------|----------------------------------------------------|--------|
| `arceos-borrowable-experience.md` | `_archive/2026-07-04-q19-lichee-analysis/arceos-borrowable-experience.md` | core borrowable patterns promoted into optimization / learned / architecture records; archive remains available for Q20/Q21 reference |
| `architecture-overview.md` | `_archive/2026-07-04-q19-lichee-analysis/architecture-overview.md` | stale summary superseded by `SNAPSHOT.md`, `openspec/project.md`, and `openspec/specs/architecture/spec.md` |
| `d1-axplat-bringup-plan.md` | `_archive/2026-07-04-q19-lichee-analysis/d1-axplat-bringup-plan.md` | D1 axplat bring-up blockers resolved; outcome recorded in Q19/Q19B learned notes (R7 in references) |
| `lichee-rv-dock-adaptation-plan.md` | `_archive/2026-07-04-q19-lichee-analysis/lichee-rv-dock-adaptation-plan.md` | Q19 Lichee early smoke test completed; outcome in archived OpenSpec change `q19-lichee-d1-early-smoke` (R5 in references) |
| `optimization-milestone-replan.md` | `_archive/2026-07-04-q19-lichee-analysis/optimization-milestone-replan.md` | roadmap decisions incorporated into `.claude/docs/tasks.md` and `openspec/specs/optimization/spec.md` (R2 in references) |
| `platform-parameter-decoupling.md` | `_archive/2026-07-04-q19-lichee-analysis/platform-parameter-decoupling.md` | Q18 platform descriptor / early console work completed; active decisions live in ADRs, learned notes, and tasks (R6 in references) |
| `q19b-current-blockers.md` | `_archive/2026-07-04-q19-lichee-analysis/q19b-current-blockers.md` | Q19B blockers resolved during Q19B completion; retained for historical debugging |
| `q19b-lichee-benchmark-plan.md` | `_archive/2026-07-04-q19-lichee-analysis/q19b-lichee-benchmark-plan.md` | Q19B benchmark stage completed; evidence preserved in archived change `q19b-lichee-d1-benchmark` and learned notes (L236-L258, ADR-047~ADR-051) |

### Lichee subdir (`.claude/analysis/lichee/<file>` → archive)

| Old active path | Archive location (relative to `.claude/analysis/`) | Reason |
|-----------------|----------------------------------------------------|--------|
| `lichee/629.txt` | `_archive/2026-07-04-q19-lichee-analysis/lichee/629.txt` | raw board boot capture preserved as historical evidence |
| `lichee/boot.txt` | `_archive/2026-07-04-q19-lichee-analysis/lichee/boot.txt` | raw official boot log compressed into platform notes and learned facts |
| `lichee/img.txt` | `_archive/2026-07-04-q19-lichee-analysis/lichee/img.txt` | partition/image facts incorporated into learned notes and Q19/Q19B records |
| `lichee/kbench` | `_archive/2026-07-04-q19-lichee-analysis/lichee/kbench` | Q19B kbench board log preserved as historical evidence |
| `lichee/probe` | `_archive/2026-07-04-q19-lichee-analysis/lichee/probe` | boot partition probe evidence preserved for Q19D; summarized facts in learned notes and Q19D direction |
| `lichee/public-platform-notes.md` | `_archive/2026-07-04-q19-lichee-analysis/lichee/public-platform-notes.md` | platform facts absorbed into Q19/Q19B/Q19C/Q19D docs; archive remains available for D1 SDMMC/rootfs work (R4 in references) |
| `lichee/userbench` | `_archive/2026-07-04-q19-lichee-analysis/lichee/userbench` | Q19B userbench board log preserved as historical evidence |
| `lichee/新建 文本文档.txt` | `_archive/2026-07-04-q19-lichee-analysis/lichee/新建 文本文档.txt` | raw Lichee official Linux explorer capture preserved; distilled facts live in public platform notes and learned records |

### Special-cased binary tombstone (kept in active path)

| Active path | Reason kept |
|-------------|-------------|
| `.claude/analysis/lichee/boot-official-backup.img.tombstone.md` | binary `boot-official-backup.img` marker — original `.img` path is intentionally NOT replaced with text to avoid accidental flashing of a tombstone file. Archive copy lives at `.claude/analysis/_archive/2026-07-04-q19-lichee-analysis/lichee/boot-official-backup.img`. |

## Active-path Tombstones (consolidated 2026-07-11)

### Top-level (`.claude/analysis/<file>` -> archive)

| Old active path | Archive location (relative to `.claude/analysis/`) | Reason |
|-----------------|----------------------------------------------------|--------|
| `q19c-lichee-full-starryos-benchmark.md` | `_archive/2026-07-11-q19c-d1-async-uart-closeout/q19c-lichee-full-starryos-benchmark.md` | Q19C completed and archived; final facts live in ADR-052/054/055, learned L259-L280, and lichee-d1-fullbench spec |
| `q19c-d1-tx-optimization.md` | `_archive/2026-07-11-q19c-d1-async-uart-closeout/q19c-d1-tx-optimization.md` | Q19C.8e completed; P99 tail kept as known limitation for Q20复验 |
| `q19c-m1-memory-root-path-loader.md` | `_archive/2026-07-11-q19c-d1-async-uart-closeout/q19c-m1-memory-root-path-loader.md` | M1 memory-root path loader passed on D1; lazy COW issue remains O80 |
| `q19c-m2-m3-shell-sdmmc-probe.md` | `_archive/2026-07-11-q19c-d1-async-uart-closeout/q19c-m2-m3-shell-sdmmc-probe.md` | M2 command-entry passed; M3/rootfs-probe canceled as UART gate |

### Lichee subdir (`.claude/analysis/lichee/<file>` -> archive)

| Old active path | Archive location (relative to `.claude/analysis/`) | Reason |
|-----------------|----------------------------------------------------|--------|
| `lichee/M2.md` | `_archive/2026-07-11-q19c-d1-async-uart-closeout/lichee/M2.md` | Q19C-M2 board log preserved as final command-entry evidence |
| `lichee/NewDate.md` | `_archive/2026-07-11-q19c-d1-async-uart-closeout/lichee/NewDate.md` | Q19C-M0/M0.8e board data preserved |
| `lichee/Q19cM1.md` | `_archive/2026-07-11-q19c-d1-async-uart-closeout/lichee/Q19cM1.md` | Q19C-M1 board log preserved |
| `lichee/licheerv-dock-bringup.md` | `_archive/2026-07-11-q19c-d1-async-uart-closeout/lichee/licheerv-dock-bringup.md` | Q19/Q19B/Q19C D1 board evidence preserved after final closeout |

## Restore Rule

1. Find the old path in the table above (or grep `tombstone:` for older records).
2. Copy the archived file back to the requested path.
3. Restore only on explicit user request.

## See also

- `openspec/specs/references/spec.md` §"项目内部分析与设计文档索引" — R2/R4/R5/R6/R7 and R10-R13 entries carry archive markers with the same archive paths.
- `openspec/specs/learned/spec.md` and `openspec/specs/architecture/spec.md` — every `.claude/analysis/*.md` reference now carries an inline `[ARCHIVED 2026-07-04 → …]` annotation.
