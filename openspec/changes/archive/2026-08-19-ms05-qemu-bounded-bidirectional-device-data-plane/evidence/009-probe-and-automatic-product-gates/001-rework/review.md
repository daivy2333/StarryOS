# Review: Iteration 009 / Cycle 001-rework

Independent-review section of the Cycle 001 Evidence package. Covers the
specs-vs-code matrix, the full git diff review and the Evidence audit. All
checks were run against the final source/worktree state captured in
`environment.txt` (HEAD `8dc3ef7d`, source mtimes before the Gate window).

## Specs-vs-code matrix

| Contract | Requirement/design | Cycle 001 witness | Result |
|---|---|---|---|
| C1 | R6, R14, D9 — V3 layout and ioctl commands exact; V1/V2 consumers unchanged | unchanged probe ABI; C size/offset `_Static_assert`; MS03/MS04 harness + host-test + axnet/driver regressions | PASS |
| C2 | R6, D9 — each mode records PRE/HELD/FULL/RELEASED/POST and exactly one terminal marker | phase-order + marker/exit harness tests (unchanged) + 17-decision harness | PASS |
| C3 | R1-R5, D9 — slot/descriptor Full proven by exact ledger, then recovery closes ownership | `ms05_descriptor_full_proved` now requires descriptor availability 0 AND inflight QS + Again; `ms05_post_closed` requires zero-inflight/zero-ticket closure; mutation harness rejects descriptor-headroom and conserved-but-inflight states | PASS |
| C4 | R4, D8-D9 — flush bounded and succeeds only for closed target | `ms05_flush_proved` now requires success delta == 1, zero error/busy/cancel delta, tickets zero, ledger closed + post closure, u64-max wrap guard; drain/wait use absolute mode deadline | PASS |
| C5 | R6, R14, D10 — host traffic/control bounded, ordered, peer-strict | C/Python share `!III` network order (known-byte witness + `ms05_be32`); SENT/START source validated before parse; finite per-phase socket timeout (timeout = failure); wrong-peer SENT/START/timeout/grace witnesses | PASS |
| C6 | R14, D10, R44/R51 — traceable automatic results, narrow environment handoff | final-source identity + mtimes before gates; literal commands.txt with timestamps/raw-log paths/exits; lossless race split logs; build.log/artifacts.sha256 match re-read hashes; env-blocked.txt = explicit None | PASS |

## Full diff review

Scope reviewed: the complete worktree delta of this Cycle relative to the
staged Cycle 000 state, i.e. `git diff` on `tests/ms05_data_plane_probe.c`,
`tests/ms05_data_plane_probe_test.c`, `scripts/ms05_data_plane_stimulus.py`
and `Makefile` (probe binary `144240 -> 144520` bytes).

Reviewed hunks:

- Shared decision core: `ms05_be32` (wire byte order), `ms05_budget_remaining_ms`
  (checked arithmetic), `ms05_mode_deadline_abs` (overflow guard),
  `ms05_deadline_expired` (4-arg phase+mode bound), `ms05_post_closed` (exact closure),
  `ms05_descriptor_full_proved` (buffer AND descriptor exhaustion + Again),
  `ms05_flush_proved` (success delta 1, no error/busy/cancel delta, wrap guard, closure).
- Wire path: `ms05_validate_datagram` decodes with `ms05_be32`; `udp_send_data` encodes
  with `ms05_be32`; `udp_recv_data`/`udp_control_recv`/`udp_done_recv` clamp the receive
  timeout to the remaining mode budget.
- Production ordering: `wait_for_condition` checks the clock before reading the snapshot
  and re-checks both phase and mode deadline after a success; `drain_tx` uses the checked
  submit target, `ms05_post_closed`, and the same fresh-clock recheck; `send_until_full`
  is bounded by the FULL phase deadline (held mode cannot spend `count * SO_SNDTIMEO`
  beyond the lease) and the mode budget; all six mode runners capture one absolute
  `mode_start`/`mode_abs` and thread it through handshake/send/control/wait/drain/DONE.
- Python host: `serve_once` installs a finite per-phase timeout before the first receive
  and converts any `socket.timeout` to `ValueError`; `_serve_exchange` validates the
  source peer before parsing START and before recognizing SENT; the grace loop is bounded
  and rejects duplicate SENT/foreign peers. `self_test` adds wrong-peer SENT/START,
  first-recv timeout, missing-SENT timeout and short-grace witnesses.
- Harness: `test_datagram_validation` builds network-order packets; `test_wire_network_order`
  proves the fixed bytes `4d5330350000000300000060` decode as `MS05/3/96` and native-order
  bytes are rejected; `test_descriptor_full_proof`/`test_flush_proof` reject the false
  positive states; `test_post_closure`/`test_conservation_is_not_closure` separate
  conservation from closure; `test_deadline_boundaries`/`test_mode_deadline_abs`/
  `test_budget_remaining` pin equal/late expiry and wrap protection.

Findings:

| Severity | Finding | Disposition |
|---|---|---|
| Critical | none | — |
| Important | none | — |
| Minor | `ms05_budget_remaining_ms` equal-budget returns 0 (exhausted) while callers break out of send loops on 0; this is the intended strictly-before semantics and is pinned by `test_budget_remaining` | accepted, documented |
| Minor | `drain_tx` uses `tx_submit >= submit_target` (not `==`); the exact-submit boundary is proven by `ms05_post_closed` and the conservative `>=` matches the budgeted-async drain reality recorded in Cycle 000 | accepted, documented |

Scope exclusions confirmed: no product/kernel/driver/axnet edits, no ABI change,
no MS01-MS04 source change, no QEMU console execution, no telemetry reset, no raw
ring/slot access, no fake completion.

## Evidence audit

Performed on the final worktree (after all Gates):

- `artifacts.sha256` hashes re-read from disk: all six match byte-for-byte
  (`57b672cf…`, `16803680…`, `c2a252f9…`, `9cd43fa8…`, `11b567a1…`, `6a7189e2…`).
- `build.log` records the same binary identity (file type, mtime, size, hash) as
  `artifacts.sha256`; source mtimes (18:15-18:19) precede the build window (18:21+).
- `commands.txt` lists literal commands with timestamps, raw-log paths and exits; the
  rustfmt command uses the literal 18-file change-owned list (no placeholders).
- Raw logs are non-empty and unedited: protocol-and-decision 847 B, automatic-gates
  67.7 KB, build 27.1 KB, race-stability index 14.9 KB + split raw logs
  (control-100x 591 KB, v3-100x 592 KB, full-suite-100x 2.28 MB).
- `env-blocked.txt` is explicit `None`: loopback and musl cross-builds completed
  successfully in this run; the D1 comparison retained full raw diagnostics and shows
  exactly 25 established axfs/axtask errors (20 E0432 + 5 E0433) with the expected
  exit 101.
- Cycle 000 Evidence is preserved unmodified as historical input; its hash mismatch
  (144136/144240 B vs the final 144520 B binary) is not reused.

## Conclusion

All Iteration 009 automatic Gates pass with the final source; no Critical or
Important finding is unresolved; the remaining items are explicit Minor notes.
Iteration 010 (manual QEMU runtime + closeout) is the only deferred boundary.
