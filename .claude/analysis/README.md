# Analysis Index

> Last updated: 2026-07-11
> 2026-07-11 update: Q19C/D1 async UART closeout analysis moved to `_archive/2026-07-11-q19c-d1-async-uart-closeout/`.
> 2026-07-04 update: 16 active-path tombstones (8 top-level + 8 in `lichee/`) were consolidated into `_archive/README.md`.

## Active

- `q17-smp-memory-ordering.md` — Q17 SMP / memory-ordering analysis; still referenced by Q20 multi-hart revalidation.
- `uart-async-qemu-d1-first-replan.md` — UART async milestone replan; moves QEMU/D1 benchmark gaps and user ring/completion/zero-copy ahead of multi-hart board validation.

## Special binary tombstone

- `lichee/boot-official-backup.img.tombstone.md` — pointer to the archived 10 MiB official boot partition backup. Kept in active path on purpose: the original `.img` is intentionally NOT replaced with text to avoid accidental flashing of a tombstone file. Archive copy at `_archive/2026-07-04-q19-lichee-analysis/lichee/boot-official-backup.img`.

## Archived

- `_archive/2026-07-04-q19-lichee-analysis/` — Q18/Q19/Q19B historical plans, Lichee platform capture logs, Q19B board evidence, and the official boot partition backup (8 files + `lichee/` subdir).
- `_archive/2026-07-11-q19c-d1-async-uart-closeout/` — Q19C planning, TX optimization, M1/M2/M3 analysis, and D1 board evidence logs after D1 async UART testing ended.
- `_archive/2026-06-24-q0-q15-analysis/` — older Q0~Q15 architecture, UART extraction, M4/Q15, and performance analysis material (14 files).
- `_archive/README.md` — single-source-of-truth index for all active-path → archive mappings (16 entries); replaces the per-path tombstone files that used to live at the active paths.

## Restore Rule

1. Find the old path in `_archive/README.md` "Active-path Tombstones" section (or grep `tombstone:` for older records).
2. Copy the archived file back to the requested path.
3. Restore only on explicit user request.

The R-index entries in `openspec/specs/references/spec.md` and the archive annotations in `learned/` / `architecture/` / `optimization` specs carry the archive paths, so cross-document navigation is preserved.
