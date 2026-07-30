# Migration Coverage Checklist

Status: `verified`

## Source Classes

| Class | Count | Treatment |
|---|---:|---|
| Active legacy experience sources | 2 | Full originals in `active-originals/` |
| Rebuilt exclusions | 2 | `CLAUDE.md`, `.claude/docs/SNAPSHOT.md` |
| Previous MIG carrier | 1 | Immutable pointer and per-unit verification |
| Referenced historical ARC carriers | 6 | Immutable pointers and per-unit verification |

## Active Source Baseline

| Source | Lines | SHA-256 | Original Copy |
|---|---:|---|---|
| `openspec/project.md` | 124 | `bf5fcffdf0ec52c61c9a2cdf56aacb405c9452f94ad16836e39208ef820ca3c0` | `active-originals/openspec-project-original.md` |
| `.claude/docs/tasks.md` | 211 | `d7e7df29d21d01970c9aca50ced8e725d035594981fa43b7bb7b0f1a2411592f` | `active-originals/tasks-original.md` |

## Audit Artifacts

| Artifact | Purpose |
|---|---|
| `source-registry.tsv` | 41 source files, logical path, immutable carrier/copy path, full-file SHA-256 and unit count |
| `unit-coverage.tsv` | Every non-empty semantic unit with source ID, line range, kind, SHA-256, Legacy IDs, state/time boundary, target IDs and verification status |
| `numbering-map.md` | Legacy A/ADR/L/O/Q/R identifiers aggregated to current M/D/K/R/I/MS targets |
| `target-coverage.tsv` | Reverse target index with target existence, source-file count, unit count and Legacy-ID count |
| `migration_unit_audit.py` | Read-only reproducible splitter, mapper and forward/reverse verifier |

Unit kinds are `heading`, `paragraph`, `list-item`, `table-row`, `comment`, `code-block` and `metadata`. Blank separators contain no information and are covered by the full-file hash rather than counted as semantic units.

## Required Equality

```text
source units = 2743
mapped source units = 2743
verified source units = 2743
unmapped = 0
skipped = 0
coverage = 100.00%
```

## Forward Verification

- Every row in `unit-coverage.tsv` has a non-empty target set.
- Every current target file exists.
- Every numbered target anchor (`Mxx`, `Dxx`, `Kxx`, `Rxx`, `Ixx`, `MSxx`) is present in its target file.
- Rebuilt state targets use explicit `SNAPSHOT`, `TASKS` or `CLAUDE` tokens.
- Every row status is `V`; there are no `U` or skipped states.

## Reverse Verification

- `target-coverage.tsv` was generated from all 2,743 source rows.
- Every target has `verified=yes` and at least one source unit.
- Historical carrier administration maps to R47; domain content additionally maps to the applicable M/D/K/R/I/MS target.
- Duplicate, superseded, completed, canceled and blocked content retains its state/date boundary in the unit row and its full source bytes in the immutable carrier or active original.

## Source Integrity

- `(cd <carrier> && sha256sum -c active-sources.sha256)` verifies the two full originals saved in this carrier before or after Archive.
- `sha256sum -c historical-carriers.sha256` verifies the previous MIG and all six historical ARC carriers without modifying them.
- `source-registry.tsv` repeats full-file hashes and unit counts so the audit can be regenerated independently.

## Exit Decision

Coverage requirements are satisfied. Archive remains conditional on strict OpenSpec validation, skill entry Gate, diff checks, Archivist review and a final regeneration showing the same equality.
