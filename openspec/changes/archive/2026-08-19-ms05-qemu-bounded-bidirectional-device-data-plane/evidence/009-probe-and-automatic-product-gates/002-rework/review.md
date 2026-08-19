# Review — MS05 Cycle 002 rework Evidence

Date: 2026-08-15 19:00:40 +0800
Reviewer: Act (openspec-act), Cycle 002 — full diff and Evidence audit
Scope: `tests/ms05_data_plane_probe.c`, `tests/ms05_data_plane_probe_test.c`,
`scripts/ms05_data_plane_stimulus.py`, `scripts/ms05_evidence_audit.py`,
Evidence `009-probe-and-automatic-product-gates/002-rework/`.

## specs-vs-code review

| Contract | Result | Evidence |
|---|---|---|
| T5.1-R3 non-vacuous traffic | PASS | `ms05_traffic_proved` enforces EXACT (`count>0 && sent==count && received==count`) and HELD (`0<sent<=count && received==sent`); all four runtime runners delegate; Python `parse_sent` mode-aware with `count`. RED fixtures prove zero/partial normal traffic and zero held traffic were accepted before the fix. |
| T5.1-R4 complete absolute deadlines | PASS | all six runners create `mode_abs` and reject a final decision at/after it; `control_apply` re-reads the clock on success; `flush_wait` preflights budget against the 2s kernel timeout and post-checks; drain uses a fresh `drain_start`; every UDP timeout clamps via `ms05_ctx_budget_ms` + `ms05_clamp_timeout_ms`. |
| T5.1-R4 bounded Release after Hold | PASS | held-mode `full-deadline` and `drain-deadline` error paths attempt exactly one `control_apply(RELEASE, ...)` within the remaining mode budget; release path itself is bounded. |
| T5.1-R4 host exchange deadline | PASS | `serve_once`/`_serve_exchange` clamp every receive to `min(GRACE_TIMEOUT, deadline - now)`; drip-feed RED fixture proves the old per-datagram timeout renewed the total lifetime. |
| T5.2-R2 literal Evidence + audit | PASS | `commands.txt` literal (no placeholders, real timestamps, EXIT records); RED raw output in `traffic-and-deadline.log`; `evidence-audit.log` shows 7 negative fixtures failing for intended reasons and positive audit PASS; artifact hashes re-read and match `build.log`. |
| Product/ABI unchanged | PASS | diff touches only the three probe/stimulus files and the new audit script; no V1/V2/V3, ioctl, kernel or driver change; MS01-MS04 regression Gates green. |

## Full diff review

Reviewed every hunk of the working-tree diff for the three changed files plus
the new audit script. Findings:

- `ms05_data_plane_probe.c` (+389/-121): shared decision functions
  (`ms05_traffic_proved`, `ms05_deadline_ctx`, `ms05_ctx_budget_ms`,
  `ms05_clamp_timeout_ms`, `ms05_flush_affordable`) placed in the
  testing-visible section so the harness pins them; UDP helpers now thread
  `const struct ms05_deadline_ctx *`; all six runners create the absolute
  deadline, capture a fresh drain start and delegate traffic/ledger/closure
  decisions. `released_at` removed (was dead after the fresh drain start).
- `ms05_data_plane_probe_test.c` (+78): five new decision test groups
  (22 total), each assertion maps to a named plan mutation; counts updated.
- `scripts/ms05_data_plane_stimulus.py` (+141/-26): `HELD_MODES` +
  mode-aware `parse_sent(data, mode, count)`, `EXCHANGE_TIMEOUT`, `FakeClock`
  + `DripFeedSocket`, exchange-deadline-clamped `bounded_recv` in
  `_serve_exchange`, self-test grows drip=PASS.
- `scripts/ms05_evidence_audit.py` (new): executable audit with 7 negative
  fixtures and a positive audit of the Cycle 002 Evidence set.

No out-of-scope modification found. No new warnings (`-Wall -Wextra -Werror`
clean; python compile clean). No dead code, no duplicated logic, no
test-passing-for-the-wrong-reason (each RED fixture fails only via its
intended `AuditFailure`; C RED aborts are genuine assertion failures).

## Evidence audit

`scripts/ms05_evidence_audit.py --write-log evidence-audit.log` exit 0:
- 7 negative fixtures each fail for their intended reason (missing RED,
  empty raw log, hash mismatch, source newer than artifact, placeholder
  command, missing required file, unjustified ENV-BLOCKED).
- positive audit PASS: all required files present and non-empty, RED/GREEN
  markers present, literal commands, six artifact hashes match build.log,
  no unjustified environment blocks.
- re-verified artifact hashes against the live binaries: all 6 match.

## Findings

- Critical: 0
- Important: 0
- Minor: 0

## Conclusion

Cycle 002 repairs satisfy T5.1-R3, T5.1-R4 and T5.2-R2 with zero unresolved
Critical/Important findings. Iteration 009 automatic Gates complete;
Iteration 010 (Tasks 6.1-6.3 manual QEMU runtime and closeout) is the next
authorized unit, using the six fresh qualified artifacts.
