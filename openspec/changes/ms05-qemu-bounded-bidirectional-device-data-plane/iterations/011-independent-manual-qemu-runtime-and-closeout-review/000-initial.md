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

- Status: pending

Pending.

## Plan Review

- Status: pending

**Review Result**

Pending.

**Findings**

Pending.

**Deviation Classification**

Pending.

**Acceptance Gaps**

Pending.

**Convergence**

Pending.

**Evidence**

Pending.

**Follow-up Decision**

Pending.

**Iteration Plan Update**

None.

**Next Cycle**

Pending.

**Next Iteration**

Pending.
