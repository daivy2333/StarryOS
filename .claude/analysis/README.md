# Analysis Index

> Last updated: 2026-07-07
> 2026-07-07 update: added `q19c-d1-tx-optimization.md` (Q19C.8e D1 TX zero-send / P99 long-tail optimization analysis).
> 2026-07-04 update: 16 active-path tombstones (8 top-level + 8 in `lichee/`) were consolidated into `_archive/README.md`.

## Active

- `q17-smp-memory-ordering.md` — Q17 SMP / memory-ordering analysis; still referenced by Q20 multi-hart revalidation.
- `q19c-lichee-full-starryos-benchmark.md` — active Q19C fullbench planning and implementation reference.
- `q19c-d1-tx-optimization.md` — Q19C.8e D1 TX zero-send / P99 long-tail optimization analysis; root causes and 5 optimization directions with recommended A+B combination.

## Special binary tombstone

- `lichee/boot-official-backup.img.tombstone.md` — pointer to the archived 10 MiB official boot partition backup. Kept in active path on purpose: the original `.img` is intentionally NOT replaced with text to avoid accidental flashing of a tombstone file. Archive copy at `_archive/2026-07-04-q19-lichee-analysis/lichee/boot-official-backup.img`.

## Archived

- `_archive/2026-07-04-q19-lichee-analysis/` — Q18/Q19/Q19B historical plans, Lichee platform capture logs, Q19B board evidence, and the official boot partition backup (8 files + `lichee/` subdir).
- `_archive/2026-06-24-q0-q15-analysis/` — older Q0~Q15 architecture, UART extraction, M4/Q15, and performance analysis material (14 files).
- `_archive/README.md` — single-source-of-truth index for all active-path → archive mappings (16 entries); replaces the per-path tombstone files that used to live at the active paths.

## Restore Rule

1. Find the old path in `_archive/README.md` "Active-path Tombstones" section (or grep `tombstone:` for older records).
2. Copy the archived file back to the requested path.
3. Restore only on explicit user request.

The R-index entries in `openspec/specs/references/spec.md` (R2/R4/R5/R6/R7) and the `[ARCHIVED 2026-07-04 → …]` annotations in `learned/` / `architecture/` / `optimization` specs all carry the same archive paths, so cross-document navigation is preserved.
