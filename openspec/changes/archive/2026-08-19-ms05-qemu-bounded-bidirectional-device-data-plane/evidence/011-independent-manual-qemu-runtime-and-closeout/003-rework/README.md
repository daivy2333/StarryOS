# Evidence: 011-independent-manual-qemu-runtime-and-closeout / 003-rework

- Change: ms05-qemu-bounded-bidirectional-device-data-plane
- Iteration: 011-independent-manual-qemu-runtime-and-closeout-review
- Cycle: 003-rework
- Captured at: 2026-08-18
- Revision: worktree on `2af394e6` (net-k3)
- Environment: Linux host; rustc nightly-2026-02-25 (edition 2024); cc 11.4;
  qemu-system-riscv64; /opt/musl/riscv64-linux-musl-cross; python3.
- Persisted Evidence mode: required

## Scope

Repair items 6.2-R4 (descriptor-Full progression witness + probe predicate),
6.2-R5 (bounded manual-listen / DONE-ACK shared-count protocol) and the affected
automatic forward-gates (6.3-R3 host-visible part). The manual QEMU runtime
(Plan Gates 5-7) is a user boundary; Act stops at exact-command handoff.

| ID | Origin | Claim | Artifact | Result |
|---|---|---|---|---|
| EV-011-003-01 | plan-required | Queue service reaches real driver Full under HOLD_RECLAIM but `tx_again` never fires (probe predicate unsatisfiable) | [axnet-tests.log](axnet-tests.log) | PASS |
| EV-011-003-02 | plan-required | Probe branch-3 predicate + timeout max tuple compile clean; harness 18 seam GREEN | [probe-stimulus.log](probe-stimulus.log) | PASS |
| EV-011-003-03 | plan-required | Stimulus listen/exchange split + DONE/ACK self-test and loopback GREEN | [probe-stimulus.log](probe-stimulus.log) | PASS |
| EV-011-003-04 | act-added | RISC-V static probe builds (artifact hash) | [probe-stimulus.log](probe-stimulus.log) | PASS |
| EV-011-003-05 | plan-required | Driver/virtio/axnet regressions GREEN | [driver-regressions.log](driver-regressions.log) | PASS |

## Artifacts

- `implementation.md` — files/symbols changed, root-cause attribution, verification.
- `runbook-manual-qemu.md` — exact R44 manual QEMU commands for the user boundary.
- `axnet-tests.log` — axnet default + qemu-diagnostics + model-test results.
- `probe-stimulus.log` — C syntax/harness, stimulus self-test/loopback, RISC-V build.
- `driver-regressions.log` — axdriver_net / virtio-drivers test results.

## Limitations

- Host-model and automatic-witness results do not substitute for real VirtIO
  device-model IRQ/descriptor progression; that is the outstanding user-run gate.
- No SMP, DWMAC, real-board or performance conclusion is made here.

## Outstanding (user boundary)

- Plan Gates 5-7 manual QEMU execution per `runbook-manual-qemu.md`, followed by
  final specs/code/Evidence review (Task 6.3).
