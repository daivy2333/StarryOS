# Review: 009-probe-and-automatic-product-gates / 000-initial

- Review at: 2026-08-15, revision `8dc3ef7d63da00c1966e9cb70820c337494d3c57` +
  uncommitted worktree (Makefile, probe, stimulus)
- Reviewer: openspec-act Phase 4 (self-review of the full Cycle diff)

## Specs-vs-code matrix

| Contract | Requirement / design | Code surface | Witness | Status |
|---|---|---|---|---|
| C1 | R6, R14, D9: exact V3 layout + commands; V1/V2 consumers unchanged | `struct ms05_snapshot` with 72 u64 fields, `_Static_assert` size/offsets; ioctl constants `0x4e49_4433/4331/4631` | decision harness ABI asserts; MS03/MS04 regressions | PASS |
| C2 | R6, D9: per-mode PRE/HELD/FULL/RELEASED/POST + exactly one terminal marker | `run_*` mode runners record phase sequences; `finish_mode`/`fail_mode` emit one `MS05 PASS|FAIL mode=` | `test_phase_order_valid`, `test_marker_parse`, `test_exit_consistency` | PASS |
| C3 | R1-R5, D9: slot/descriptor Full proven by exact ledger, then recovery closes ownership | `ms05_slot_full_proved` (occupancy==64, full transition, high-water), `ms05_descriptor_full_proved` (buffer avail==0, Again delta, inflight==QS), `drain_condition`; runtime FULL/RELEASED/POST wait | `test_slot_full_proof`, `test_descriptor_full_proof`, `test_ledger_conservation` | PASS |
| C4 | R4, D8-D9: flush bounded, succeeds only for closed construction-time target | `ms05_flush_proved` (flush_success +1, live/queued/device_owned == 0, ledger closed); `flush_wait` ioctl | `test_flush_proof` | PASS |
| C5 | R6, R14, D10: bounded ordered host traffic; rejects malformed/duplicate/out-of-order | `scripts/ms05_data_plane_stimulus.py` strict parser + sequence validator + bounded timeouts | `--self-test` (malformed/peer/mode/count/dup/missing/reorder), `--loopback-self-test` | PASS |
| C6 | R14, D10, R44/R51: traceable automatic results + narrow env blocks | raw logs, commands.txt, environment.txt, artifacts.sha256, env-blocked.txt | audit in this review | PASS |

Requirement coverage: R1-R5 (slot/backpressure/completion/ticket semantics via
ledger proofs and probe validation), R6 (QEMU probe modes + bounded protocol),
R7-R12 (queue contract, owner, ISR/waker, EVENT_IDX, budgets — existing
product code committed in prior iterations), R13 (final slots), R14
(verification/evidence order), R15 (diagnostic lease — product code committed
in Iteration 008). No Missing requirement and no unapproved Simplified
behavior for this Cycle's scope.

## Full diff review

Baseline: HEAD `8dc3ef7d` (Iteration 008 accepted, Iteration 009 Plan committed
by the MS06 commit). Worktree additions for this Cycle:

- `Makefile`: +5 lines in `host-test` (MS05 syntax/decision/stimulus gates),
  +1 static RISC-V payload target `tests/ms05_data_plane_probe`.
- `tests/ms05_data_plane_probe.c` (new): V3 ABI struct + decision core +
  guest runtime (6 modes). Uses only published V3/control/flush ioctls and
  normal UDP socket traffic. No product code, no ring/slot mutation, no
  telemetry reset, no fake completion.
- `tests/ms05_data_plane_probe_test.c` (new): 12 mutation/decision tests
  with `MS05_DATA_PLANE_PROBE_TESTING` guard (mirrors MS04 pattern).
- `scripts/ms05_data_plane_stimulus.py` (new): bounded host stimulus with
  strict parse/sequence validation and self-tests (mirrors MS04 pattern).

Unchanged in this Cycle: `crates/axnet`, `crates/axdriver_*`,
`crates/virtio-drivers`, `kernel/` (all committed at `8dc3ef7d`), and every
MS01-MS04 payload source.

## Findings and disposition

| Severity | Finding | Disposition |
|---|---|---|
| Critical | none | — |
| Important | none | — |
| Minor | runtime `tx_submit >= sent` uses `>=` because the queue task drains asynchronously with budget 32/round; POST is read after `drain_tx` (which waits for submit to reach `pre + sent` and the TX ledger to close) | accepted; `drain_tx` makes the assertion stable; `>=` is the conservative bound |
| Minor | smoltcp buffers frames beyond the 64 TX slots; residue frames commit to the driver only when a socket op runs `poll_interfaces`. `drain_tx` issues non-blocking recvs (O_NONBLOCK toggled via fcntl) as the wake source, and the host stimulus applies a bounded `GRACE_TIMEOUT` after SENT to collect in-flight residue | accepted; this is the deterministic drain protocol between probe and stimulus |
| Minor | probe datagram payload capped at 64 bytes (matches MS04 payload conventions; wire header 12 bytes + 64 = 76-byte datagrams) | accepted; documented in the probe header |

## Acceptance-gap check

- Missing PRE / reordered phase / duplicate terminal marker / exit mismatch:
  rejected by the decision harness (C2, C6). PASS.
- fake Full (occupancy < capacity, or no full transition): rejected (C3).
- counter regression / non-closed buffer/descriptor/slot ledger: rejected
  (C3/C6).
- equal/after-deadline completion: rejected as expired (C4).
- malformed control / wrong peer / dup / missing / reorder / timeout on the
  host protocol: rejected by stimulus self-tests (C5).
- runtime QEMU execution: intentionally not performed (R44 user boundary);
  handed off to Iteration 010 with fresh, hashed artifacts.
