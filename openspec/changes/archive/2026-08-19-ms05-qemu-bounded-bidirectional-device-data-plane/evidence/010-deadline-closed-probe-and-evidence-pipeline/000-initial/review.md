# MS05 Iteration 010 / Cycle 000 — Specs-vs-code and full diff Review

Review window: 2026-08-15 | HEAD: 8dc3ef7d (net-k3, worktree modified)
Reviewer: openspec-act (Iteration 010 / Cycle 000)

## Scope

Tasks 5.3 (production-path absolute deadline + held cleanup) and 5.4
(manifest-driven automatic Gate and Evidence qualification) of change
`ms05-qemu-bounded-bidirectional-device-data-plane`.

## Specs-vs-code

### Task 5.3 — R6/R14 fixed deadline; D9/D11

| Plan contract | Implementation | Verdict |
|---|---|---|
| production runners obtain clock/sleep/ioctl/socket timeout/send/recv through one injectable boundary | `g_ms05_ops` seam in `tests/ms05_data_plane_probe.c`; prod_* impls default, harness fakes override | PASS |
| precheck before every side effect; zero/equal/regressed/overflowed fails without invoking the operation | `ms05_precheck_budget` / `ms05_ctx_budget_ms` (regression/overflow -> 0) | PASS |
| clamp socket timeout and retry sleep to min positive budget; postcheck after every op | `ms05_bounded_send/recv/sleep/control` use `ms05_clamp_timeout_ms` + `ms05_postcheck` | PASS |
| control checks before first and every retry ioctl; EAGAIN near expiry cannot sleep fixed then late ioctl | `ms05_bounded_control` re-prechecks each loop iteration, sleep clamped | PASS |
| all guest control/data sends, receives, flush, snapshot use the same rules | `udp_*`, `flush_wait`->`ms05_bounded_flush`, `snapshot_or_fail` all route through bounded helpers | PASS |
| after Hold success: explicit active state, one cleanup owner, at most one Release under original mode deadline | `run_held_mode` sets `hold_active`, all exits `goto out`, cleanup block with phase-disabled `cleanup_ctx`; Release failure only WARNs | PASS |
| Python checks deadline before/after READY/data/DONE sends and every receive; delayed sends/recvs cannot renew | `send_bounded` pre/post checks; `bounded_recv` clamps; `DripFeedSocket`/`ProtocolSocket` model delayed send | PASS |
| Preserve: six mode names, terminal marker, network byte order, peer/sequence, exact/held traffic, Full/Again, POST/flush closure, V3 ABI, 2s lease ceiling | preserved; `tests/ms05_data_plane_probe_test.c` 22 decision + 14 seam tests GREEN | PASS |

RED witnesses: control-send budget edge, EAGAIN-then-expired-sleep, late
ioctl no-next-side-effect, post-Hold failures (HELD snapshot, held clock,
hold-mode mismatch, Full wait), Python delayed READY/data/DONE — all covered
by seam tests; see `red/` for pre-fix records.

### Task 5.4 — R6/R14 automatic product failure/env classification/runtime Evidence; D10/D11

| Plan contract | Implementation | Verdict |
|---|---|---|
| one subprocess capture primitive: versioned manifest record with stable gate ID, literal argv, cwd, RFC3339 start/end, exit, classification, raw-log path+hash | `scripts/ms05_evidence_capture.py::run_record/run_d1/run_repeat100` | PASS |
| required Gate IDs declared in code/schema; audited for exact set and order | `GATES`/`REQUIRED_GATE_IDS` in capture; `audit_manifest` checks missing + relative order | PASS |
| sequential shell expressions become separate records | ms03/ms04 compile and run are separate records; no `&&` in argv | PASS |
| every 100x Gate has 100 indexed child records with complete stdout/stderr and hashes | `run_repeat100` writes `logs/<gate>/NNNN.log` per child; audit checks 100 unique indexes | PASS |
| source-freeze paths/content hashes/index+worktree identity before Gates; later edit invalidates | `freeze_source` recorded before gates; `audit_source_freeze` re-hashes and fails on drift | PASS |
| artifact records bind path/size/mtime/hash/generating Gate; literal file/stat/sha256sum records | `run_artifact_records` + `artifacts` records; audit verifies all six against live disk | PASS |
| D1 exit 101 qualifies only with exactly 20 E0432 + 5 E0433 and no unclassified error | `classify_d1`/`audit_d1` exact counts + unclassified scan | PASS |
| R44 classification needs own raw log, earliest capability-failure layer, unchanged argv | `audit_env_blocked` earliest marker; `env-blocked.json` empty array here | PASS |
| negative fixtures in temp copies with exact stable error codes | 14 fixtures in `run_fixtures`, each asserts its named code | PASS |
| freeze manifest, audit log, qualification binding; final verifier | `--write-qualification`/`--verify-qualification` PASS | PASS |
| both git diff --check and --cached --check as independent manifest Gates | `diff-check`, `diff-cached-check` records both exit 0 | PASS |

## Full diff review

- Staged + unstaged diff reviewed (60 files, +80k lines dominated by
  Iteration 009 Evidence re-staging and this Iteration's scripts).
- Change-owned source: `tests/ms05_data_plane_probe.c` (seam + bounded
  helpers + unified cleanup), `tests/ms05_data_plane_probe_test.c` (14 new
  seam tests), `scripts/ms05_data_plane_stimulus.py` (deadline-aware send),
  `scripts/ms05_evidence_capture.py` (new), `scripts/ms05_evidence_audit.py`
  (rewritten), `tests/test_ms05_evidence_tools.py` (new), `Makefile`
  (host-test additions).
- No product kernel/driver/ABI/wire changes; V1/V2/V3 layout untouched.
- No `as any`/panic/warning suppression; `-Werror` clean on all builds.
- Cross-task interactions verified: manifest capture -> audit -> qualification
  -> sha256sum re-read all PASS.

## Findings

- Critical: 0
- Important: 0
- Minor: none unresolved (fake-clock call-count anchor in one seam test is
  documented; a `MS05 WARN` release-in-cleanup-failed message is intentional
  and tested).

## Verdict

PASS — no Critical or Important findings; all required Evidence files exist
and are hash-consistent.
