# Evidence: 009-probe-and-automatic-product-gates / 000-initial

- Change: `ms05-qemu-bounded-bidirectional-device-data-plane`
- Iteration: `009-probe-and-automatic-product-gates`
- Cycle: `000-initial`
- Captured at: 2026-08-15T09:38Z (UTC) / 17:38 +0800
- Revision: `8dc3ef7d63da00c1966e9cb70820c337494d3c57`
- Branch: `net-k3` (worktree: Makefile + probe/stimulus additions uncommitted)
- Environment: WSL2 x86_64; Ubuntu cc/gcc 11.4.0; Python 3.10.12; rustc
  1.95.0-nightly; riscv64-linux-musl-gcc 11.2.1; QEMU 7.0.0; openspec 1.6.0

## Scope

Iteration 009 Cycle 000 implements Task 5.1 (deterministic MS05 probe, C
decision harness, bounded host stimulus, Makefile gates) and runs Task 5.2
(the complete automatic product Gate stack with fresh artifact provenance).
No kernel or library behavior changes were made in this Cycle; the probe is
tooling around the accepted V3/control/flush ABI and the MS04-compatible
synchronous data plane.

## Evidence index

| ID | Origin | Claim | Artifact | Result |
|---|---|---|---|---|
| EV-009-000-01 | plan-required | Strict C syntax and RISC-V static payload build pass | [probe-tests.log](probe-tests.log) | PASS |
| EV-009-000-02 | plan-required | Decision harness 12/12 mutation tests pass (missing PRE, reorder, fake Full, regression, ledger, deadline, marker, exit) | [probe-tests.log](probe-tests.log) | PASS |
| EV-009-000-03 | plan-required | Stimulus self-tests and real-loopback test reject malformed/peer/mode/count/dup/missing/reorder/timeout | [probe-tests.log](probe-tests.log) | PASS |
| EV-009-000-04 | plan-required | `make host-test` passes with MS05 gates integrated and MS03/MS04 unchanged | [automatic-gates.log](automatic-gates.log) | PASS |
| EV-009-000-05 | plan-required | axnet qemu-diagnostics 234/234, default 215/215, axdriver_net 7/7, axdriver_virtio net 16/16, virtio-drivers 36/36+8 doctests, uart_16550 62/62+8+10 | [automatic-gates.log](automatic-gates.log) | PASS |
| EV-009-000-06 | plan-required | MS03 host harness 33/33, MS04 host harness 16/16 | [automatic-gates.log](automatic-gates.log) | PASS |
| EV-009-000-07 | plan-required | race stability 100× each: control shared path, V3 shared snapshot, default-parallel full suite | [race-stability.log](race-stability.log) | PASS |
| EV-009-000-08 | plan-required | rustfmt, strict OpenSpec validation, scoped `git diff --check` all exit 0 | [automatic-gates.log](automatic-gates.log) | PASS |
| EV-009-000-09 | plan-required | kernel QEMU check exit 0; D1 comparison exit 101 with exactly 25 established axfs/axtask errors | [build.log](build.log) | PASS (expected comparison) |
| EV-009-000-10 | plan-required | fresh QEMU image + five payloads built, file/stat/sha256 captured | [build.log](build.log), [artifacts.sha256](artifacts.sha256) | PASS |
| EV-009-000-11 | plan-required | `make host-test` UDP loopback self-test passes in this sandbox (no EPERM observed) | [probe-tests.log](probe-tests.log) | PASS |
| EV-009-000-12 | plan-required | specs-vs-code and full diff review: no Missing, no unapproved Simplified, zero Critical/Important | [review.md](review.md) | PASS |
| EV-009-000-13 | plan-required | env-blocked handoff list is explicit | [env-blocked.txt](env-blocked.txt) | PASS (None) |

## Collection method

Raw command output was captured with `tee` into `probe-tests.log`,
`automatic-gates.log`, `race-stability.log` and `build.log` in the same
session as the implementation. Exact commands, environment and artifact
hashes are in `commands.txt`, `environment.txt` and `artifacts.sha256`.
Logs are byte-for-byte raw output (no whitespace normalization).

## Limits

- This Cycle proves the automatic Gate stack and artifact identity. It does
  not prove runtime TX/RX/flush correctness: those claims require Iteration
  010 QEMU serial evidence (`MS05 PASS mode=...` per mode).
- QEMU and the guest console were not started in this Cycle (R44 user
  terminal boundary). The probe payload and image are fresh and hashed for
  that handoff.
- Artifact hashes establish identity, not correctness. QEMU user-net UDP
  reorder/drop is a bounded-fail condition in the probe protocol, per D9.
