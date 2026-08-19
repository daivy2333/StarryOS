# Iteration 011 / Cycle 000: Independent Manual QEMU Runtime and Closeout Review

## Plan Context

- Status: ready
- Iteration: 011-independent-manual-qemu-runtime-and-closeout-review
- Cycle: 000-initial
- Cycle Type: initial
- Parent cycle: none

**Iteration Scope**

- Change tasks: 6.1, 6.2, 6.3
- Depends on: accepted Iteration 010 production-path deadline witnesses and
  automatic qualification contracts
- Stable baseline: the final source and current artifacts pass focused
  deadline/provenance checks; the only fresh automatic-suite interruption is
  an exact R44 UDP-socket `EPERM` in the Review environment
- Verification boundary: unchanged-argv ordinary-terminal rerun, six MS05
  modes, R51 MS04 regression, required network behavior and final provenance
  review all pass with raw evidence
- Diagnostic boundary: environment rerun, each runtime mode, network/MS04
  regression and final audit are independent stop layers
- Deferred work: change closeout, archive, SNAPSHOT and M/D/K/R/I maintenance

**Objective**

Independently prove the accepted MS05 data plane against one single-hart QEMU
VirtIO-MMIO NIC, preserve the complete manual execution record, and perform the
final specs-to-code and Evidence review. Runtime success must be attributable to
the reviewed revision and hashed image/payloads, not to a rebuilt or substituted
artifact.

**Background**

Iteration 010 closed the guest/host absolute-deadline paths and the automatic
capture/audit contracts. Its large temporary manifest and logs were inspected
and then deleted under the user's explicit persistence waiver. That waiver is
not a PASS record for this Iteration: Cycle 000 creates a new, compact manual
runtime package under
`evidence/011-independent-manual-qemu-runtime-and-closeout/000-initial/`.

Fresh Plan Review reproduced one exact environment boundary: `make host-test`
reached the MS04 loopback test and UDP socket creation returned `EPERM`. R44
requires the user to rerun the unchanged command in an ordinary terminal before
manual QEMU work begins.

**Relevant Runbooks and Surfaces**

| Area | Authority / surface | Responsibility |
|---|---|---|
| Manual boundary | R44 `.claude/runbooks/qemu-network-testing.md` | unchanged argv, environment delta, raw logs and manual-only QEMU interaction |
| MS05 runtime | `tests/ms05_data_plane_probe.c`, `scripts/ms05_data_plane_stimulus.py` | six modes, fixed deadline, traffic and ledger markers |
| MS04 regression | R51 `.claude/runbooks/ms04-qemu-async-rx-core-evidence.md` | snapshot, idle, nudge and burst witnesses |
| Network regression | R45 `.claude/runbooks/ms02-virtio-mmio-evidence.md` and MS01 payload | ARP/ICMP, TCP/UDP 5555, nonblocking and poll |
| Final review | change specs, tasks, Iteration 010 Review and full diff | traceability, compatibility, waiver and provenance closure |

**Critical Path**

```text
ordinary terminal reruns unchanged blocked argv
  -> final revision, image and payload hashes freeze
  -> user manually boots one-hart/one-NIC QEMU
  -> six independent MS05 mode/host exchanges
  -> MS04 and network regressions in the same declared environment
  -> raw serial/host logs, commands, exits and hashes collected
  -> Evidence index/hash audit and specs-vs-code/full-diff review
  -> user may decide change closeout
```

## Task Contracts

### 6.1: Resolve the R44 ordinary-terminal boundary

- Requirement/Scenario: R14 automatic failure/environment distinction and the
  exact R44 handoff produced by Iteration 010 Review.
- Depends on: accepted Iteration 010; no manual QEMU may precede this task.
- Required behavior:
  - in an ordinary user terminal, run the unchanged argv `make host-test` from
    the repository root and preserve complete stdout/stderr and exit status;
  - record the Review environment's `EPERM`, the ordinary-terminal environment
    difference, revision, tool versions and timestamps without broadening
    permissions or replacing the command;
  - if the rerun passes, freeze the kernel image and five guest payload paths,
    sizes and SHA-256 values before QEMU boot; do not rebuild after hashing;
  - if any Rust, C, link, assertion, audit or diff failure occurs, classify it
    as a product failure and stop before Task 6.2.
- Evidence: `environment.txt`, `commands.txt`, `host-test.log`,
  `artifacts-before.txt` and their entries in `artifacts.sha256`.
- GREEN condition: unchanged `make host-test` exits 0 and all runtime artifacts
  exist with nonempty size and recorded SHA-256.
- Stop when: the ordinary terminal reproduces `EPERM` or another capability
  restriction; return an R44 handoff. Do not mark the task complete.

### 6.2: Collect independent manual QEMU runtime evidence

- Requirement/Scenario: R6 fixed-deadline runtime behavior and R14 complete,
  attributable Evidence for MS05 plus required compatibility regressions.
- Depends on: Task 6.1 GREEN and frozen artifact hashes.
- Capability boundary: all QEMU guest-shell commands are entered manually by
  the user under R44. The Act may prepare/check commands and later audit the
  returned files, but must stop with a Blocker Handoff while user execution is
  required.
- Environment: one hart, 1 GiB RAM, one VirtIO-MMIO NIC, QEMU user networking;
  retain the R44 command shape and declare any added host forward or packet
  capture explicitly. Do not infer SMP, hardware or performance behavior.
- MS05 execution:
  - build/download the reviewed `ms05_data_plane_probe` and start a fresh host
    `python3 scripts/ms05_data_plane_stimulus.py --host 0.0.0.0 --port 15557`
    process for each mode;
  - manually run `snapshot`, `tx-only 96 64`, `bidirectional 96 64`,
    `slot-full`, `descriptor-full` and `flush` as separate observations;
  - require exactly one terminal marker per mode, the expected nonzero/exact
    traffic counts, slot/ticket/buffer/descriptor conservation, Full→recovery,
    C4 flush closure, bounded work/yield telemetry, and zero fault/restore/IRQ
    entry errors;
  - a timeout, interrupted log, partial telemetry, mismatched peer/sequence or
    one successful mode cannot substitute for another mode.
- Compatibility execution:
  - follow R51 for MS04 `snapshot`, `idle`, `nudge` and `burst`, preserving its
    lifecycle/owner, zero-idle-delta, nudge, 96-packet burst and zero-fault
    criteria and existing historical waivers;
  - follow R45 for ARP/ICMP and TCP/UDP 5555; run the MS01 socket payload and
    require all 14 markers, including TCP/UDP nonblocking and poll readiness;
  - preserve full serial and host-side output. Packet captures may supplement
    but cannot replace the required markers and exits.
- Evidence: `qemu-serial.log`, six `ms05-*-host.log` files,
  `ms05-markers.txt`, `ms04-burst-host.log`, `ms04-markers.txt`, network host
  output/pcaps, `runtime-exits.txt`, and an updated `artifacts.sha256`.
- GREEN condition: every required MS05, MS04 and network criterion is present
  in complete raw output and remains bound to the frozen revision/artifacts.
- Stop when: a mode fails, its deadline expires, an artifact hash changes, the
  serial session is interrupted, or the environment differs materially. Keep
  partial evidence labelled FAIL/interrupted and return to Plan or Act repair.

### 6.3: Perform final change and Evidence review

- Requirement/Scenario: R14 final provenance and no unresolved implementation
  or coverage gap before user-directed closeout.
- Depends on: Tasks 6.1 and 6.2 GREEN with all required files present.
- Required behavior:
  - compare specs, accepted decisions and tasks against final code and runtime
    markers; verify V1/V2 compatibility and the QEMU-only control boundary;
  - review the complete staged and unstaged product diff, rerun strict change
    validation, and run non-Evidence whitespace checks;
  - audit every required Evidence file for nonempty content, timestamp range,
    revision/artifact identity and SHA-256; raw logs remain immutable;
  - record each Task, RTM row and Gate as PASS, FAIL, WAIVED or SKIPPED with its
    exact authority. Historical waivers remain historical and cannot fill a
    missing Iteration 011 observation;
  - write `review.md` and a complete `README.md` index. Do not archive the
    change or modify global OpenSpec state.
- GREEN condition: no Critical or Important finding, no unapproved Missing or
  Simplified result, all required tasks trace to raw Evidence, strict validation
  and both relevant diff checks exit 0, and `artifacts.sha256` verifies.
- Stop when: any required raw file, marker, exit, hash or compatibility witness
  is absent or inconsistent; create a rework Cycle instead of editing a PASS.

## BDD Scenarios

- Environment rerun: the unchanged host-test command runs outside the Review
  sandbox. Exit 0 unlocks artifact freeze; a product diagnostic or repeated
  capability failure stops the Iteration with its original output.
- Normal modes: TX-only and bidirectional exchange the declared 96 packets of
  64 bytes with correct peer/sequence and exactly one PASS marker each.
- Pressure recovery: slot-full and descriptor-full observe the intended Full
  state, make bounded recovery progress and close every ownership ledger.
- Flush: target C4 completes before the fixed deadline with closed POST state
  and no fault/restore error.
- Regression: MS04 snapshot/idle/nudge/burst and ARP, ICMP, TCP/UDP 5555,
  nonblocking and poll each satisfy their own criteria; partial success is FAIL.
- Provenance damage: a missing/truncated log, changed artifact, wrong revision,
  absent marker or hash mismatch prevents final acceptance.

## Invariants

- One reviewed revision and one frozen artifact set own the entire runtime run.
- Every MS05 exchange retains its original absolute deadline; retries or mode
  transitions do not create a new budget.
- QEMU interaction is manual and single-hart/single-NIC; conclusions remain
  limited to the observed QEMU software/device model.
- Raw logs, command argv, exits and hashes are evidence; summaries cannot
  replace them.
- The Cycle 010 persistence waiver does not waive Iteration 011 Evidence.

## Non-goals

- No product, ABI, wire-protocol or probe-mode change is planned.
- No automated QEMU guest interaction, SMP, DWMAC/real-board, performance or
  exact fd-readiness claim.
- No change archive, SNAPSHOT refresh or M/D/K/R/I maintenance.

## Requirements Traceability Matrix

| Requirement / Scenario | Task | Witness | Status |
|---|---|---|---|
| automatic environment boundary is narrow | 6.1 | unchanged `make host-test` raw log and exit | Planned |
| fixed-deadline six-mode runtime | 6.2 | per-mode serial/host markers and ledgers | Planned |
| MS04 behavior is preserved | 6.2 | R51 snapshot/idle/nudge/burst evidence | Planned |
| network/socket behavior is preserved | 6.2 | R45 ARP/ICMP/TCP/UDP and MS01 14 markers | Planned |
| final source/artifact/Evidence provenance | 6.3 | hashes, strict validation and final review | Planned |

## Verification Gates

1. Task 6.1 unchanged-argv ordinary-terminal gate.
2. Artifact/revision freeze gate.
3. Six independent MS05 runtime gates.
4. R51 MS04 compatibility gate.
5. R45/MS01 network and socket compatibility gate.
6. Final Evidence hash/index, specs-vs-code, strict validation and diff Review.

Each Gate stops downstream work on failure. User-provided manual results resume
the same Cycle; they do not create a new Cycle unless Review finds a product or
Evidence defect that needs rework.

## Evidence Plan

- Mode: required
- Root:
  `evidence/011-independent-manual-qemu-runtime-and-closeout/000-initial/`
- Required index: `README.md`, `environment.txt`, `commands.txt`,
  `artifacts-before.txt`, `artifacts.sha256`, `host-test.log`, raw QEMU/host
  logs, marker/exit summaries and `review.md`.
- Persist raw outputs without editorial replacement. Large packet captures may
  be omitted only by a new explicit user waiver after their required facts are
  independently preserved elsewhere; serial, host stimulus, command, exit,
  marker and hash records remain required.

## Act Handoff

Start with Task 6.1. After automatic preflight and Evidence-path preparation,
stop at the R44 user capability boundary with the exact ordinary-terminal
command and requested output files. Resume the same Cycle after the user
returns the raw results; do not run QEMU autonomously.

## Act Response

- Status: blocked

**Preflight and Evidence preparation (Act)**

- Repository: `net-k3` @ `2af394e6cc8e6aa9ae7026d7ede136382258a98b` (MS05:第七次
  提交), worktree clean at baseline collection.
- Automatic preflight `make host-test` ran in this environment with **exit 0**
  and every harness PASS (early-console, memtrack, ms03-irq, ms04-async-rx,
  ms05 data-plane decision+seam tests, stimulus self/loopback, evidence tools
  unittest, capture/audit self-tests including all 25 negative fixtures). No
  R44 `EPERM` reproduced in this sandbox — recorded as `host-test-preflight.log`.
- Evidence root prepared: `evidence/011-independent-manual-qemu-runtime-and-
  closeout/000-initial/` with `README.md`, `environment.txt`, `commands.txt`
  (full manual command list per R44/R51/R45), `artifacts-before.txt`,
  `artifacts.sha256`, `host-test-preflight.log`. Change-level and iteration-
  level evidence README indexes updated (010 added, 011 in-progress).
- Artifact freeze at revision `2af394e6` (fresh hashes; note
  `tests/ms05_data_plane_probe` sha256 `db27b567…` differs from Iteration 010
  evidence `8505e467…` because the payload was rebuilt by the 7th commit — the
  new freeze is the authority for this Cycle): kernel image 40190144 B,
  ms01/ms02/ms03/ms04/ms05 payloads present, sizes + SHA-256 in
  `artifacts-before.txt` / `artifacts.sha256`. Do not rebuild after this freeze.

**Blocker Handoff**

- Task/Step: 6.1 (unchanged-argv ordinary-terminal `make host-test` rerun +
  artifact freeze confirmation), then 6.2 (manual QEMU six modes + MS04 R51 +
  R45/MS01 network regression).
- Gate: R44 capability boundary — QEMU guest-shell commands and the ordinary
  terminal run are user-executed per `.claude/runbooks/qemu-network-testing.md`.
  The Act must not run QEMU autonomously or mark these PASS without raw output.
- Plan expected vs actual: no deviation — the Plan's Act Handoff explicitly
  requires stopping at this boundary with the exact command list and requested
  output files, which is `evidence/011-…/000-initial/commands.txt`.
- Impact: no product code changed. Evidence directory and baseline files are
  prepared; no Gate is PASS until the user's raw outputs are returned.
- Completed: preflight + Evidence-path preparation (Tasks 6.1 evidence prep).
  Partial: none. Not started: user rerun (6.1 GREEN), QEMU runtime (6.2),
  final review (6.3).
- Recovery: user runs the commands in `commands.txt` in an ordinary terminal
  and returns the raw files (`host-test.log`, `qemu-serial.log`, six
  `ms05-*-host.log`, `ms04-burst-host.log`, `ms05-markers.txt`,
  `ms04-markers.txt`, `runtime-exits.txt`, pcaps if any) plus any environment
  delta; Act then audits, runs Task 6.3 final review and resumes the Cycle.

**Blocker Resolution**

None — awaiting user manual run.

---

**Diagnostic Addendum (2026-08-17) — QEMU runtime reveals a data-plane blocker**

User executed the manual QEMU run with the frozen artifact set and, after
`wget` hung, a layered host/guest diagnostic was performed. Raw records are in
`evidence/011-independent-manual-qemu-runtime-and-closeout/000-initial/`
(`qemu-serial-info.log`, `qemu-serial-snapshot.log`, `qemu-serial-debug.log`,
`ms05-diag.pcap`).

**Facts established**

- eth0 registration is correct: `mac: 52-54-00-12-34-56`, `ip: 10.0.2.15/24`
  (`qemu-serial-info.log` lines 167-179).
- Queue task activated: MS05 snapshot reports `lifecycle=2 owner=1` (Active,
  async-owned), `fault=0 lc_fault=0 owner_inv=0` (`qemu-serial-snapshot.log`
  lines 311-317).
- IRQ→wake→reap chain works: MS04 snapshot shows `isr_publish=1 isr_wake=1
  reaped=1 refilled=1 non_ip=1` (an ARP frame was reaped and delivered as
  non-IP) (`qemu-serial-snapshot.log` lines 353-357).
- TCP path is broken: `wget` printed `Connecting to 10.0.2.2:18765` and hung
  until Ctrl-C; the pcap (`ms05-diag.pcap`) contains exactly two frames — guest
  ARP request `who-has 10.0.2.2` and slirp ARP reply `10.0.2.2 is-at
  52:55:0a:00:02:02` — and **no TCP SYN follows**. The guest's ARP request
  reached the wire (TX works), the reply entered the NIC, but the stack never
  consumed it into a TCP connection attempt.
- `MS05 FAIL mode=tx-only reason=handshake` occurred while no host stimulus was
  listening — environment, not a data-plane verdict.
- `MS04 FAIL mode=nudge` shows `isr_publish=1 isr_wake=1 nudge=1 task=2
  reaped=1 refilled=1 empty=2` in DELTA — the nudge gate is not met in this
  build (extra ISR publish/refill during nudge). This is a regression-gate
  failure candidate to confirm after the main blocker is resolved.
- `ifconfig`/`ping` fail due to missing kernel surface (`/proc/net/dev`,
  `SIOCGIF*`, `SOCK_RAW`) — tooling gap, not the data-plane blocker.

**Blocker Handoff (revised)**

- Task/Step: 6.2 manual QEMU runtime — TCP downloads hang; no SYN after ARP.
- Gate: Gate 5/6 — runtime evidence cannot reach GREEN for the MS05 modes and
  the R51/R45/MS01 network regressions while TCP stays dead in the real QEMU
  VirtIO device model.
- Plan expected vs actual: Plan assumed the slot-mode data plane works in QEMU
  (host tests pass). Actual: host-model tests pass, but the real VirtIO device
  model does not drive the ARP-reply→stack-consumption→TCP-SYN path. This is a
  **new product-level defect**, not an environment waiver.
- Impact: Tasks 6.1-6.3 cannot complete; all TCP/UDP-through-socket runtime
  evidence is blocked until the data plane is repaired. No product code was
  changed by this Cycle; frozen kernel image and disk are restored
  (`StarryOS_riscv64-qemu-virt.bin` = `fe20b5b2…`, `make/disk.img` untouched).
- Completed: automatic preflight (host-test exit 0), artifact freeze, layered
  diagnostics (info/debug/snapshot serial + pcap). Partial: 6.1 rerun,
  six-mode runtime, R51/R45/MS01 regressions (blocked). Not started: 6.3 final
  review.
- Recovery: `openspec-plan` investigates the data plane — TX is confirmed
  working (ARP on the wire), the broken stage is between RX reap/refill and
  smoltcp consuming the ARP reply to emit a TCP SYN; candidates are
  `rx_copy_one` slot delivery, `Service::poll` RX-slot drain, or the
  socket-waker/progress bridge. After a fix, this Cycle resumes at Task 6.2
  with the same frozen artifacts (hashes unchanged).

## Plan Review

- Status: reviewed

**Review Result**

rework-required

**Findings**

- **Blocking — first-TX queue wake is missed after ARP resolution.** In
  `Service::poll`, `router.poll()` runs before `tx_pending_before` is sampled.
  An ARP reply consumed by `router.poll()` can resolve the neighbor and flush
  the pending TCP SYN into the first dormant TX slot; the later sample already
  sees that slot, so `!tx_pending_before` is false and
  `QUEUE_EVENT.publish_queue_work()` is skipped. With no TX completion yet,
  the sole queue owner has no hardware event that can submit this first slot.
  This directly violates Task 6.2's QEMU network/runtime Acceptance.
- **Blocking — the current pcap contradicts the Act Addendum's final packet
  count but sharpens the same defect.** `tcpdump` decodes more than the stated
  two frames: after the initial ARP request/reply it contains later guest SYNs,
  slirp SYN-ACK retransmissions and a final RST. The SYN appears only after a
  long gap and later manual activity, consistent with software nudge advancing
  the stranded TX slot. The supported boundary is therefore “ARP-created
  first TX slot is not self-published,” not “ARP reply can never reach the
  stack.”
- **Blocking — the manual command plan incorrectly pairs `snapshot` with a
  host stimulus.** `run_snapshot()` performs only bounded diagnostic snapshots
  and never sends `MS05 REGISTER`; a host `serve_once()` for that mode waits
  forever. The literal zero-byte `ms05-<mode>-host.log` is a template artifact,
  not required Evidence. Snapshot is guest-only; the other five modes require
  distinct host stimulus logs.
- **Blocking — a product repair invalidates the old artifact freeze.** The Act
  recovery text proposes resuming with unchanged hashes, but modifying
  `Service::poll` requires a rebuilt image, a new revision/worktree identity
  and a new artifact freeze before QEMU rerun. Cycle 000 Evidence remains
  diagnostic history and cannot qualify the repaired binary.
- **Non-conclusive — `MS04 FAIL mode=nudge` was collected while prior network
  traffic remained in flight.** Its extra ISR/reap/refill deltas match a real
  packet arriving during the nudge window, so it is neither a valid isolated
  R51 PASS nor sufficient evidence of a second product regression. Recheck it
  only after the repaired image reaches a quiescent baseline with no concurrent
  host traffic.
- Task 6.1's `host-test.log` contains passing output, and the Act preflight
  exited 0, but the ordinary-terminal pipeline did not persist its
  `${PIPESTATUS[0]}` in the log. Treat 6.1 as partial and capture the exit
  explicitly in the rework Evidence.

**Deviation Classification**

- `NEW-EVIDENCE`: manual QEMU exposed a first-TX wake gap not observable in the
  accepted host matrix.
- `PLAN-OMISSION`: no combined witness covered ARP resolution creating the
  first dormant TX slot while the owner slept.
- `PLAN-INVALID`: Cycle 000 required a host stimulus/log for guest-only
  `snapshot` and suggested retaining frozen artifacts after a product repair.
- `BASELINE-CHANGED`: the persisted pcap continued beyond the two-frame prefix
  described in the Act Addendum and now includes SYN/SYN-ACK traffic.
- No `ACT-DEVIATION` in product code: the Act stopped without modifying the
  implementation when the manual runtime Gate failed.

**Acceptance Gaps**

- A1: a TX slot created anywhere in one `Service::poll` round, including ARP
  pending flush during ingress, must publish queue work exactly on the
  empty→nonempty transition.
- A2: the repaired source/image/payload set must be rebuilt, independently
  qualified and refrozen; Cycle 000 hashes cannot qualify it.
- A3: manual Evidence must separate the clean wget/ARP/TCP witness, guest-only
  snapshot, five host-assisted MS05 modes and isolated R51 regression; every
  run needs raw serial/pcap/host output and an explicit exit.
- A4: Tasks 6.2 and 6.3 remain incomplete until all runtime and final review
  Gates pass against the repaired artifact set.

**Convergence**

Expanded in implementation surface but diagnostically narrowed. The Cycle
started as a manual capability handoff and uncovered one product wake-ordering
gap plus two execution/Evidence-plan defects. The first failing product path is
now localized to `Service::poll` empty→nonempty TX publication; no driver IRQ,
descriptor ownership, ABI or wire redesign is indicated.

**Evidence**

- Both ordinary and sandbox `make host-test` logs contain the complete passing
  host matrix; the current artifact hash file verifies 6/6 files.
- QEMU serial proves eth0 registration, Active lifecycle/async ownership and
  zero recorded data-plane fault. MS04 telemetry proves IRQ publish/wake and
  raw RX reap/refill occurred.
- Independent `tcpdump -nn -e -vvv -r ms05-diag.pcap` shows the initial ARP
  request/reply, a later SYN burst, repeated inbound SYN-ACK and final RST.
- Source review proves `process_arp()` flushes pending IPv4 into a dormant TX
  slot, while `Service::poll()` samples `tx_pending_before` only after that
  ingress action. Existing focused tests for ordinary dispatch wake and ARP
  pending flush each pass independently, confirming the missing combined
  witness rather than disproving the gap.
- Strict OpenSpec validation passes at revision
  `2af394e6cc8e6aa9ae7026d7ede136382258a98b`.

**Follow-up Decision**

Reject Cycle 000 Acceptance and create Cycle 001 in the same Iteration. Repair
the first-TX wake transition, rebuild/refreeze artifacts, then repeat the manual
runtime with isolated, non-appended Evidence before final closeout review.

**Iteration Plan Update**

None. Tasks 6.1-6.3, their requirements and the Iteration Map remain unchanged;
the next Cycle contains local repair items only.

**Next Cycle**

`001-rework.md`

**Next Iteration**

None.
