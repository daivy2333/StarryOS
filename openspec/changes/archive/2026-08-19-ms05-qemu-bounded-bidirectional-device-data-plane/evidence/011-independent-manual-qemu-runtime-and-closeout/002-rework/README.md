# MS05 Iteration 011 / Cycle 002 — Evidence index

> Cycle: 011-independent-manual-qemu-runtime-and-closeout-review / 002-rework
> Plan Context: `iterations/011-independent-manual-qemu-runtime-and-closeout-review/002-rework.md`
> Persisted Evidence mode: **required**

## Scope

Repair items 6.1-R2 (portable schema-v2 source/Evidence identity and
persistable required Evidence), 6.2-R3 (exact four-session R44 handoff) and
6.3-R2 (final review) of change
`ms05-qemu-bounded-bidirectional-device-data-plane`.

## Automatic qualification (Act, 2026-08-17) — schema v2

- Manifest: `manifest.json` — schema_version **2**, single source freeze with
  `evidence_exclusion` = the fixed change Evidence subtree, 44/44 records
  `pass`, six artifacts, 18 artifact file/stat/sha256 records. Captured by
  `scripts/ms05_evidence_capture.py --run automatic` from the repo root on the
  accepted (dirty) tree at HEAD `2af394e6cc8e6aa9ae7026d7ede136382258a98b`.
- Positive audit: `evidence-audit.log` + `qualification.json` (verdict PASS) +
  `env-blocked.json` (none) — `scripts/ms05_evidence_audit.py --root …
  --write-qualification`; binding re-verified with `--verify-qualification`
  (`qualification binding VERIFIED`).
- Historical v1 compatibility: the Cycle 001 v1 `qualification.json` binding
  still verifies with `--verify-qualification`.
- Freeze: `artifacts.sha256` — 6/6 verified with `sha256sum -c`.

## Identity / persistence contract (6.1-R2)

- Evidence is excluded from product source identity by an explicit Git
  pathspec (schema v2 `evidence_exclusion`), not by any `.git/info/exclude`
  local rule.
- The checkout-local exclude line for the Cycle 001 root was removed; the
  required Evidence subtree is Git-visible (`git check-ignore` returns nonzero,
  `git status --untracked-files=all` exposes the untracked files).
- Unit witnesses: `tests/test_ms05_evidence_tools.py::TestPortableIdentityContract`
  — self-reference exclusion, no-exclusion drift, info/exclude hidden source
  still drifts v2 identity, forged/broad/missing/out-of-tree exclusion rejected.
- Negative fixtures (audit self-test) include `EXCLUSION_MISSING`,
  `EXCLUSION_MISMATCH` and `EXCLUSION_UNEXPECTED`.

## Tooling diff (this Cycle)

- `scripts/ms05_evidence_capture.py`: schema v2; `evidence_exclusion()`
  derives the fixed change Evidence subtree and rejects roots outside it;
  `source_identity()` applies the exclusion via explicit Git pathspecs;
  untracked enumeration uses `--exclude-per-directory=.gitignore` only
  (ignores `.git/info/exclude` and the global excludes file); manifest records
  the normalized exclusion.
- `scripts/ms05_evidence_audit.py`: accepts v1 and v2; for v2 validates the
  recorded exclusion equals the derived fixed subtree (missing/different →
  `EXCLUSION_MISSING`/`EXCLUSION_MISMATCH`), rejects an exclusion on v1
  (`EXCLUSION_UNEXPECTED`); v1 binding preserved.
- `tests/test_ms05_evidence_tools.py`: `TestPortableIdentityContract` added.

## Manual R44 handoff (6.2-R3) — user-executed; **Cycle BLOCKED**

| File | Required by | Status |
|---|---|---|
| `commands.txt` | four-session exact handoff | prepared; static audit PASS |
| `qemu-ms05-serial.log` | Session MS05 | returned; contains failures (see `blocker.md`) |
| `qemu-wget-serial.log`, `wget.pcap` (net0 filter-dump) | Session WGET | pending (user) |
| five `ms05-*-host.log`, `ms05-markers.txt` | Session MS05 | pending (user) |
| `qemu-ms04-serial.log`, `ms04-burst-host.log`, `ms04-markers.txt` | Session MS04 | pending (user) |
| `qemu-network-serial.log`, `network.pcap`, `network-host.log`, `ms01-markers.txt` | Session NET | pending (user) |
| `host-test.log` (ordinary-terminal rerun) | Task 6.1 | pending (user) |
| `runtime-exits.txt`, `artifact-recheck-exits.txt` | exit ledger | pending (user) |

**BLOCKED (Act, 2026-08-17):** the returned `qemu-ms05-serial.log` shows only
`snapshot` and `flush` PASS; tx-only (`handshake`), bidirectional, slot-full
and descriptor-full (`full-deadline`) FAIL with `host_received=0`. See
[`blocker.md`](blocker.md). Recovery routes to `openspec-plan`.

No QEMU gate is PASS until the blocker is resolved and the relevant raw outputs
are audited (Task 6.3-R2).

## Limitations

- Conclusions are limited to the declared single-hart QEMU VirtIO-MMIO
  software/device model; nothing here qualifies SMP, DWMAC, real board,
  DMA/cache or performance behavior.
- QEMU `filter-dump` is a guest-side software-model witness, not hardware
  capture or performance evidence.
- The Cycle 000/001 evidence remains immutable diagnostic history; this
  Cycle 002 root owns the schema-v2 qualification and handoff.
