# MIG-20260720 — Legacy Spec Migration

## Summary

全量迁移旧体系 OpenSpec specs（`architecture/`, `learned/`, `optimization/`）到新 M/D/K/R/I 体系。

## Sources

| Source | Lines | Hash | Current Hash |
|---|---|---|---|
| `openspec/specs/architecture/spec.md` | 1053 | `5b054d98c039965b252a2974eb29bb0f9625e1b075043223eb5331c3cf95563a` | matches ✅ |
| `openspec/specs/learned/spec.md` | 1162 | `f09d4cae30cd87ea816940e304fd8d8e4a0adc84615ef38ce99577c2d66beec9` | matches ✅ |
| `openspec/specs/optimization/spec.md` | 439 | `2ffa3af2a3b14f26ebc159fc51c06cde8fcd57414c0f1a345024f1b1c010f6f8` | matches ✅ |

## Targets

| New Spec | Source(s) | Entries |
|---|---|---|
| `openspec/specs/project-model/spec.md` | architecture | M01-M40 |
| `openspec/specs/decisions/spec.md` | architecture | D01-D21 |
| `openspec/specs/knowledge/spec.md` | learned | K01-K27 |
| `openspec/specs/references/spec.md` | learned (merged) | R28-R34 appended |
| `openspec/specs/improvements/spec.md` | optimization | I01-I10 |

## Coverage

- **Total information units**: 130+
- **Mapped**: 130+
- **Unmapped**: 0
- **Skipped**: 0
- **Coverage**: 100%

## Verification

- `openspec validate --specs`: 25 passed, 0 failed ✅
- Source hashes unchanged ✅
- Full originals in carrier ✅
- Numbering map in coverage-checklist.md ✅

## Tombstoned Legacy Entries

All tombstoned ADRs and learned/optimization entries are preserved in archive carriers:
- ARC-202607021648
- ARC-202607021535
- ARC-202607031929
- ARC-202607081429
- ARC-202607111510
- arc-202607152005

## CLAUDE and SNAPSHOT

Rebuilt from new templates — not in migration coverage or carrier.
