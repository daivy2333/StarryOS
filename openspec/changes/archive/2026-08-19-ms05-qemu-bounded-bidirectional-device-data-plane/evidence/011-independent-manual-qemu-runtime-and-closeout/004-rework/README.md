# Evidence: 011-independent-manual-qemu-runtime-and-closeout / 004-rework

- Change: ms05-qemu-bounded-bidirectional-device-data-plane
- Iteration: 011-independent-manual-qemu-runtime-and-closeout-review
- Cycle: 004-rework
- Captured at: 2026-08-19
- Revision: worktree on `2af394e6` (net-k3); dirty worktree with staged axnet changes
- Environment: Linux host; rustc nightly-2026-02-25 (edition 2024); cc 11.4;
  qemu-system-riscv64; /opt/musl/riscv64-linux-musl-cross; python3.
- Persisted Evidence mode: required

## Scope

Repair items 6.2-R6 (bounded registration + exact DONE), 6.1-R1 (final automatic
package), 6.2-R7 (manual QEMU runtime) and 6.3-R5 (final closeout review).

The manual QEMU batch (Gates 5-7) was run by the user and all six modes produced
their terminal PASS markers. Formal qualification (6.1-R1 audit + qualification
binding) is blocked by the dirty worktree baseline and the kernel-image-hash
change; those decisions are carried to the Plan/Review.

| ID | Origin | Claim | Artifact | Result |
|---|---|---|---|---|
| EV-011-004-01 | plan-required | `listen_for_register` stays open across intermediate timeout and invalid pre-registration datagram under one absolute listen deadline (RED→GREEN) | [stimulus-self-test.log](stimulus-self-test.log) | PASS |
| EV-011-004-02 | plan-required | `udp_done_recv` rejects malformed/overflow/trailing/wrong-mode DONE and accepts exact DONE before ACK (RED→GREEN) | [probe-harness.log](probe-harness.log) | PASS |
| EV-011-004-03 | plan-required | Strict C syntax of the probe and harness user-build (host-tested) | [probe-harness.log](probe-harness.log) | PASS |
| EV-011-004-04 | plan-required | Real UDP loopback protocol self-test (REGISTER→READY→START→data→SENT→DONE→ACK); persistent-timeout fake models time advance so the bounded listen deadline binds (fixes self-test hang) | [stimulus-self-test.log](stimulus-self-test.log) | PASS |
| EV-011-004-05 | plan-required | Automatic pipeline runs once on final source: capture 44/44 gate records pass | (manifest + logs deleted under user evidence waiver) | PASS (run) — qualification unverifiable |
| EV-011-004-06 | plan-required | Automatic qualification verification (audit) | [audit.log](audit.log) | **BLOCKED** — `WORKTREE_DRIFT`; manifest deleted |
| EV-011-004-07 | plan-required | Six MS05 manual QEMU modes (snapshot/tx-only/bidirectional/slot-full/descriptor-full/flush) | [qemu-serial.log](qemu-serial.log) | PASS — all six terminal `MS05 PASS` + exit 0 |

## Artifacts

- `qemu-serial.log` — full manual QEMU serial (boot + all six modes + terminals).
- `stimulus-self-test.log` — Python self-test + loopback (bounded registration + DONE/ACK).
- `probe-harness.log` — C strict syntax + harness (22 decision + 18 seam + 5 done-parsing).
- `audit.log` — `ms05_evidence_audit.py` output (WORKTREE_DRIFT; later manifest missing).
- `artifacts.sha256` — six final artifacts frozen by the automatic pipeline.
- Deleted under user waiver (大内容证据，用户审计后豁免保存): `manifest.json`,
  `logs/`, and per-mode `ms05-<mode>-host.log` / `runtime-exits.txt` /
  `ms05-markers.txt`.

## Runtime result (6.2-R7)

Per-mode terminal results from `qemu-serial.log` (each ends `MS05 PASS mode=...`
with `exit with code: 0`):

| Mode | Result | Ledger / safety evidence |
|---|---|---|
| snapshot | PASS | pre/post hold=0, fault/owner safe |
| tx-only 96 64 | PASS | WITNESS sent=96 received=96; POST closed |
| bidirectional 96 64 | PASS | WITNESS tx_sent=96 rx_received=96 host_received=96; POST closed |
| slot-full | PASS | FULL tx_occ=64 → RELEASED → POST closed |
| descriptor-full | PASS | FULL buf_avail=0/inflight=64 → POST closed |
| flush | PASS | flush_ok=1, err/busy/cancel=0, POST closed |

All passed modes: fault=0, lifecycle_fault=0, ownership_invariant=0. Two initial
`MS05 FAIL mode=tx-only / bidirectional reason=handshake` (exit 256) occurred
before the host stimulus was running; the user confirmed this was operator timing
(no server), not a product failure. The same modes then passed on retry with the
stimulus up. This is a completeness note, not a product defect.

## Findings / Blocker

1. **WORKTREE_DRIFT + manifest deletion (6.1-R1)**: the automatic capture froze
   index/worktree identity; its own `build-payloads` gate then rebuilt the
   git-tracked payload binary, and the worktree is otherwise dirty, so the audit's
   source-freeze check fails. The user then deleted `manifest.json` and `logs/`
   under an explicit evidence-reduction waiver, so the 6.1-R1 automatic run's
   authoritative schema record and raw gate logs are no longer verifiable from this
   root. Formal qualification cannot be written from 004-rework as-is.
2. **Kernel image hash changed**: `StarryOS_riscv64-qemu-virt.bin` is
   `4018d326…` (004) vs `57b672cf…` (010). The freshly built image absorbs staged
   axnet changes (`async_rx.rs`, `device/tests.rs`, `service.rs`; 316 lines) that
   are outside this Cycle's repair scope. Per Plan 6.2-R7, a changed kernel image
   requires rerunning WGET / MS04-R51 / MS01 compatibility sessions in addition to
   the six MS05 modes. The user must decide how to treat these staged changes
   (commit / stash / provenance) before formal qualification.

## Limitations

- Six-mode results qualify only the single-hart QEMU VirtIO-MMIO software/device
  model; no SMP, DWMAC, real-board, DMA/cache or performance conclusion.
- Host stimulus serial log evidence was waived; assisted-mode host agreement is
  witnessed by the guest `host_received=<count>` field from the DONE/ACK handshake.
