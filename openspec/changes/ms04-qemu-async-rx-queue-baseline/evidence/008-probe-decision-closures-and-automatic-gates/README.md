# Evidence: 008-probe-decision-closures-and-automatic-gates

Iteration 008 completed T6.2R and T7.1-T7.3 without running QEMU. All product
Gates passed. Two commands reached sandbox capability failures and are handed to
T8.1 unchanged; neither result is a product PASS.

- Change: `ms04-qemu-async-rx-queue-baseline`
- Iteration: `008-probe-decision-closures-and-automatic-gates`
- Collection window: 2026-08-12T16:17:43+08:00 through 2026-08-12T16:33:38+08:00
- Change review baseline: `16d9a16a2b65a574022faaee39b465f6f7aebd45`
- Act HEAD: `78e1f7abfa1614c188a24ebe7150ffb7c71e46d0`, plus the recorded working-tree diff
- Environment: WSL2 x86_64 host; Rust nightly-2026-02-24; restricted sandbox

| Evidence | Scope | Result | File |
|---|---|---|---|
| EV-008-01 | Toolchain, host and sandbox context | PASS | `environment.txt` |
| EV-008-02 | T6.2R and T7.1 tests/checks/stress/format | PASS; real UDP loopback ENV-BLOCKED | `automatic-gates.log` |
| EV-008-03 | T7.2 D1 and QEMU target builds | PASS; static probes ENV-BLOCKED | `build.log` |
| EV-008-04 | Fresh target artifact qualification | PASS for D1/QEMU; probes unqualified | `artifacts.sha256` |
| EV-008-05 | R44 classification and T8.1 handoff | PASS | `env-blocked.txt` |
| EV-008-06 | Specs/code/full-range review | PASS; 0 unresolved findings | `review.md` |
| EV-008-07 | Exact command and exit index | PASS | `commands.txt` |

The D1 and QEMU artifacts are fresh outputs of this iteration. The pre-existing
`tests/ms03_irq_probe` is explicitly stale and unqualified, and
`tests/ms04_rx_probe` was not produced. No guest runtime, rootfs or negotiated
feature claim is made here.
