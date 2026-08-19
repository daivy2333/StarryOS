# Implementation — MS05 Iteration 011 / Cycle 003

- Change: ms05-qemu-bounded-bidirectional-device-data-plane
- Iteration: 011-independent-manual-qemu-runtime-and-closeout-review
- Cycle: 003-rework
- Revision: worktree on `2af394e6` (net-k3) carrying the accepted Cycle 001/002
  schema-v2 qualification and first-TX wake product diff; this Cycle adds the
  repair 6.2-R4 / 6.2-R5 host-side fixes described here.

## Root-cause attribution (Oracle consult + model witness)

The manual QEMU `descriptor-full` / `slot-full` failures in Cycle 002 were not a
queue-service scheduling bug. Direct verification of `async_rx.rs::service_round`
and `device/ethernet.rs::tx_submit_one` plus a new capacity-aware model test show
the service is correct: under `HOLD_RECLAIM` it drains a 64-capacity TX ledger to
exactly 64 in-flight (2 rounds of 32) and closes the ledger exactly on Release.

The unreachable part is the **probe's `tx_again > held->tx_again` FULL witness**:
the fixed TX-slot capacity equals the driver buffer/descriptor capacity (both
`MS05_QS = 64`) and the 32-submit round budget divides 64 exactly, so the service
reaches full at a budget boundary with no pending slot left to force the 65th
submit. `tx_again` therefore never increments, making that clause structurally
unsatisfiable.

Second root cause: the host stimulus started its short exchange deadline before
REGISTER, so a manual QEMU session (operator takes >10s to start the guest) timed
out with `host_received=0` and `tx-only` failed at `handshake`.

## Repair 6.2-R4 — descriptor-Full from the conserved ledger

- `crates/axnet/src/async_rx.rs` (test-only): added a capacity-aware
  `LedgerDevice` fake (real 64-buffer/descriptor ledger + fixed TX slot backlog,
  shared atomic counters) and `reclaim_hold_drains_to_real_driver_full_without_observing_again`
  (`#[cfg(feature = "qemu-diagnostics")]`). It proves under HOLD_RECLAIM the
  service drains to exactly 64 in-flight, `again_calls == 0` (no `tx_again`), and
  Release+reclaim closes the ledger.
- `tests/ms05_data_plane_probe.c::ms05_descriptor_full_proved`: dropped the
  unreachable `tx_again > held->tx_again` clause; the driver-Full proof now comes
  from the conserved ledger (`buffer/descriptor available == 0`, `inflight == 64`).
- `tests/ms05_data_plane_probe.c::run_held_mode`: on `full-deadline`, persist
  exactly one final/max-pressure V3 tuple labeled `MS05 TIMEOUT mode=<mode>` before
  cleanup so a timeout is attributable (submit/completion/occupancy/`tx_again`
  progression).
- `tests/ms05_data_plane_probe_test.c::test_descriptor_full_proof`: updated the
  `tx_again == 0` + fully-exhausted case from `!proved` (old, RED) to `proved`
  (new branch-3 semantics), and added a `tx_again` regression case proving the
  predicate ignores `tx_again`.

## Repair 6.2-R5 — bounded manual listen + DONE/ACK shared count

- `tests/ms05_data_plane_probe.c::udp_sent_done`: after receiving `MS05 DONE
  mode count`, the guest sends `MS05 ACK mode count` so the host reports PASS only
  after a valid ACK and the guest only after DONE+ACK.
- `scripts/ms05_data_plane_stimulus.py`:
  - split `serve_once` into an operator-paced listen phase (`MANUAL_LISTEN_TIMEOUT`,
    default 120s) that waits for the first valid REGISTER, then a fresh short
    exchange deadline (`EXCHANGE_TIMEOUT`) for READY/START/data/SENT/DONE/ACK;
  - after sending DONE, wait for and validate the guest ACK (peer/mode/count);
  - added `listen_for_register`, `parse_ack`, and self-test/loopback coverage:
    ack parsing, wrong-count/missing/wrong-peer/wrong-mode ACK, delayed and late
    registration against the listen deadline.

## Verification summary

| Gate | Result |
|---|---|
| Gate 3 RED | probe harness line 149 `!proved` failed before predicate fix; now GREEN. |
| Gate 3 model | `reclaim_hold_...` passes: drains to 64 in-flight, `again_calls==0`. |
| Gate 5 tests | axnet default 218 PASS, axnet qemu-diagnostics 238 PASS (incl. the new model); axdriver_net 7 PASS; virtio-drivers 36+8 PASS. |
| Gate 5 probe | syntax OK; host harness 18 seam PASS; RISC-V static probe builds (sha256 `a567ec91...`). |
| Gate 5 stimulus | self-test (incl. new ack/listen cases) PASS; loopback PASS. |
| Gate 4 | spec + code review PASS; no plan-outside changes, no product kernel/driver/wire changes. |

## Limitations and boundary

- The manual QEMU runtime gates (Plan Gates 5-7) are R44 user capabilities; Act
  stops at exact-command handoff below. Host-model and automatic-witness results
  above do not substitute for real VirtIO device-model IRQ/descriptor evidence.
- No SMP, DWMAC, real-board or performance claim is made.
