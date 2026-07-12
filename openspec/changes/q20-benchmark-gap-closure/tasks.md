## 1. Current-State Witness

- [ ] 1.1 Record current active changes with `openspec list`.
- [ ] 1.2 Record current Q20 analysis entry: R16 / L287 / ADR-057.
- [ ] 1.3 Record current code references for S10/S11/S14/S20/S21, TX debug ioctl, `tx_debug_snapshot()`, and `IRQ_COUNT`.
- [ ] 1.4 Confirm implementation scope excludes RX fixed payload and driver semantics changes.

## 2. TX Jitter Summary

- [ ] 2.1 Extend `print_tx_latency_diag()` or equivalent output to include `p99_p50_ratio` and `max_p50_ratio`.
- [ ] 2.2 Make S20 and S21 emit the same jitter diag shape as S10/S14.
- [ ] 2.3 Preserve existing avg/P50/P95/P99/max and `slow_over_line_plus10ms` fields.
- [ ] 2.4 Compile `tests/benchmark` after benchmark output changes.

## 3. TX Counter / CPU Proxy Summary

- [ ] 3.1 Define stable benchmark output fields for TX counter delta: user calls, ring pop, hw send, zero sends, no-progress, slow-poll exhausted, yield exhausted, drain state.
- [ ] 3.2 Add derived proxy fields: bytes/user-call, bytes/ring-pop, bytes/hw-send, zero/kB, no-progress/kB.
- [ ] 3.3 Make QEMU and D1 output the same field names; unavailable fields must be explicit rather than omitted.
- [ ] 3.4 Verify no change is made to UART TX copier, waker, IER, TTY, or drain semantics.

## 4. Evidence Layout

- [ ] 4.1 Create `.claude/analysis/q20-evidence/README.md` with build commands, run commands, macros, expected sections, and evidence status.
- [ ] 4.2 Save QEMU rootfs raw log as `.claude/analysis/q20-evidence/qemu-rootfs.log`.
- [ ] 4.3 Save D1 serial raw log as `.claude/analysis/q20-evidence/d1-fullbench-command.log`.
- [ ] 4.4 Mark RX fixed payload as intentionally not run in the evidence README.

## 5. Validation Gates

- [ ] 5.1 Run QEMU rootfs benchmark and confirm S10/S14/S20/S21 jitter + TX counter output is present.
- [ ] 5.2 Run D1 fullbench command-entry benchmark and confirm S10/S14/S20/S21 jitter + TX counter output is present.
- [ ] 5.3 Confirm Q20 does not claim SMP correctness and does not modify driver semantics.
- [ ] 5.4 Run `openspec validate q20-benchmark-gap-closure --strict`.
- [ ] 5.5 Run `openspec validate --changes` and `openspec validate --specs`.

## 6. Report and State Sync

- [ ] 6.1 Update `docs/benchmark-report-async.md` with Q20 summary tables and raw evidence links.
- [ ] 6.2 Update `.claude/docs/tasks.md` Q20 status after validation evidence exists.
- [ ] 6.3 Update `.claude/docs/SNAPSHOT.md` after Q20 gate passes.
- [ ] 6.4 Archive this change only after Q20 evidence and docs are complete.
