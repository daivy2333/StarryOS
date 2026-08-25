# Iteration 003 / Cycle 000: deterministic backlog recovery and MS01 runtime compatibility

## Plan Context

- Status: draft
- Iteration: 003-backlog-and-ms01-runtime-compatibility
- Cycle: 000-initial
- Cycle Type: initial
- Parent cycle: None

**Iteration Scope**

- Change tasks: 2.8
- Depends on: Iteration 002 accepted; `crates/smoltcp` converted from the parent gitlink to ordinary parent-managed
  files before Act starts
- Stable baseline: backlog overflow reaches a deterministic terminal result before recovery headroom is released;
  exact-512 accept/refill and immediate reconnect remain compatible in single-hart QEMU.
- Verification boundary: host/model separates overflow terminal safety from recovery; diagnostic single/fork and
  original MS01 finish on a fresh single-hart VirtIO-MMIO image with 14/14 PASS, START/END and exit 0.
- Diagnostic boundary: failures are limited to listener backlog state, overflow terminal classification, guest
  workload ordering, axtask/runner scheduling, QEMU artifact compatibility or existing MS01 socket behavior.
- Deferred tasks: 3.1–3.4

**Cycle Scope**

- Trigger: initial
- Acceptance gaps: None
- Repair items: None
- Inherited scope: R3/R4/R6/R7; D3/D4/D5/D7/D9/D10/D11; backlog 512; unique resident runner; accepted
  Iterations 001–002; manual-QEMU policy in `.claude/runbooks/qemu-network-testing.md`
- Excluded scope: terminal/device fault readiness, new scheduler semantics, reset/cancellation, SMP, PCI/DWMAC,
  physical boards, performance, QEMU automation, global docs, Evidence, archive and commits

**Objective**

Separate backlog overflow from headroom recovery so neither scenario can mask the other. Host/model tests must
observe the overflow connection's terminal state before releasing a slot. The guest MS01 payload must omit its
unobservable fire-and-close overflow attempt, preserve all 14 compatibility markers, and prove exact-512
accept/refill plus immediate recovery on a fresh single-hart QEMU image.

**Scenario Sketch**

| Scenario | Precondition | Action | Observable result | Failure boundary |
|---|---|---|---|---|
| S1 overflow terminal | 512 real loopback handshakes fill the backlog | submit client 513 and drive bounded runner rounds without releasing headroom | overflow reaches a deterministic refused/closed terminal state; listener remains intact | merely “not Established”, timeout or release-before-terminal |
| S2 exact-512 recovery | overflow S1 is already decided; one accepted connection frees headroom and refills idle | connect immediately without sleep or caller-driven poll | recovery establishes and the listener accepts exactly the 512 initial connections plus recovery | overflow races recovery or reconnect is refused/stalls |
| S3 guest workload | fresh single-hart QEMU boots the matching image and payload | run diagnostic single, diagnostic fork and MS01 manually | diagnostics PASS; MS01 emits one START, 14 PASS, no FAIL, one END and exit 0 | missing/duplicate marker, timeout, interruption or stale artifact |
| S4 capability boundary | agent sandbox rejects cross compiler, QEMU shell or Git index write | finish all agent-executable Gates and hand off exact commands | user result is accepted only with command, decisive output and exit status | environment failure is mislabeled product PASS or old evidence is reused |

**Current Baseline**

- Branch `net-k3`; HEAD `fdc8f101b8ff777228a54c7b5cd7d26be7f8301f` (`MS06:第四次提交`), four commits
  ahead of `origin/net-k3`; Cycle 000 product changes remain uncommitted.
- Iteration 002 is accepted: smoltcp pending-TX 3/3, UDP module 37/37, ordinary axnet 319/319 and
  qemu-diagnostics 339/339 passed during fresh Plan Review.
- `crates/smoltcp/.git` is absent and backed up at `/tmp/starryos-smoltcp-git-backup.6P1wzY/.git`; the parent index
  still records gitlink `160000 f96a26b...` because `.git/index` is read-only in this environment.
- QEMU 7.0.0, a 39 MiB kernel image and 1 GiB raw disk are present. The artifacts date from 2026-08-24 and cannot
  substitute for a new artifact produced after this Cycle's source changes.
- `scripts/ms01-qemu-test.py --self-test` passes, but the active Runbook forbids scripted QEMU shell driving; only
  its pure output validator may be used as an automatic seam test.
- `riscv64-linux-musl-gcc` is installed but sandbox execution currently ends with `Bad system call` (exit 159).
  Cross-compilation and QEMU guest execution are therefore known user/manual capability boundaries.

**Current-State Evidence**

- `tests/ms01_socket_baseline.c::test_tcp_512_capacity` opens 512 connections, fires a nonblocking 513th connect
  and immediately closes it without `poll`, `SO_ERROR` or any terminal marker. It then releases headroom, so the
  overflow SYN/RST can race the recovery SYN and the guest cannot attribute failure to either scenario.
- The same guest case preserves two required markers: `tcp-512cap` and `tcp-512-recovery`. The whole payload emits
  14 PASS markers between `MS01_SOCKET_BASELINE_START/END`; these names and count are compatibility ABI.
- `stack_runner::tests::task_27_repro_guest_512_recovery_sequence` already drives 512 real loopback handshakes,
  overflow, accept/refill and immediate recovery. Its overflow assertion only checks `!= Established` after eight
  polls, so a still-pending connection currently satisfies the test.
- The host model already proves immediate refill and recovery after one accepted slot. It needs a deterministic
  bounded wait for overflow refusal/closure before headroom release, not another backlog mechanism.
- `scripts/ms01-qemu-test.py` validates START/END and process exit but its `EXPECTED` set contains only ten markers;
  it is not the authority for the required manual 14-marker QEMU result. The Cycle may update the pure validator
  seam if useful, but must not create an automated QEMU runner.
- `.claude/runbooks/qemu-network-testing.md` fixes the runtime environment: RISC-V `virt`, `-smp 1`, 1 GiB,
  VirtIO-MMIO block/net, user networking, serial console and manual guest commands. QEMU results prove only this
  emulated single-hart configuration, not SMP, PCI/DWMAC, physical DMA/cache or performance.

**Relevant Code**

| File / Symbol | Current responsibility | Planned use |
|---|---|---|
| `crates/axnet/src/stack_runner.rs::task_27_repro_guest_512_recovery_sequence` | host/model reproduction of guest sequence | split overflow terminal and recovery assertions; retain bounded runner ownership |
| `tests/ms01_socket_baseline.c::test_tcp_512_capacity` | exact-512 guest compatibility case | remove ambiguous 513th fire-and-close; add fixed phase/deadline failure boundaries without changing 14 markers |
| `tests/ms01_loopback_diagnostic.c` | fixed-deadline single/fork layer witness | source/compile review and manual rerun; change only if a proven marker defect exists |
| `scripts/ms01-qemu-test.py::validate_output` | pure marker validator plus prohibited automated runner | optionally align pure expected-marker seam to 14; do not use `run()` for Acceptance |
| QEMU kernel/image build inputs | fresh runtime artifact | rebuild and label revision/config before manual guest execution |

**Critical Path**

```text
host/model:
  512 Established -> backlog full -> overflow SYN -> bounded rounds -> overflow terminal
    -> accept one Ready + atomic refill -> immediate recovery SYN -> Established -> accept totals 513

guest/manual:
  fresh source -> host/compile/build Gates -> fresh single-hart VirtIO-MMIO image
    -> diagnostic single PASS -> diagnostic fork PASS
    -> MS01 START -> 14 unique PASS / no FAIL -> END -> exit 0
```

**Implementation Guidance**

Strengthen the existing host model before touching the guest payload. Require a specific bounded terminal outcome
for overflow while the backlog remains full, then start recovery only after that assertion. In the guest payload,
delete the unobservable overflow attempt; host/model owns overflow safety, while guest owns exact-512 capacity and
immediate recovery compatibility. Add phase markers and a fixed total/step deadline to the capacity case without
renaming or duplicating its two PASS markers. Keep QEMU execution manual per Runbook.

**Behavioral Change**

- Host/model rejects a pending overflow as evidence; refusal/closure must be observed before recovery.
- Guest MS01 no longer injects a 513th connection whose outcome it cannot classify.
- Exact-512 accept/refill and immediate reconnect remain unchanged and become independently attributable.
- No network product behavior, backlog size, runner ownership, readiness contract or platform configuration changes.

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| 2.8 | R3/R6, S1/S2 | stack-runner host/model test | combined but weak overflow/recovery witness | require overflow terminal before releasing headroom, then prove recovery |
| 2.8 | R6/R7, S2/S3 | MS01 capacity case | ambiguous overflow plus exact-512 recovery | remove guest overflow stimulus; add bounded phases; retain 14-marker ABI |
| 2.8 | R7, S3/S4 | validator/build/manual QEMU gates | partial automatic seam and historical artifacts | verify all markers, build fresh artifacts and collect manual runtime result |

**Task Contract**

### 2.8: separate overflow safety from exact-512 QEMU recovery

- Requirement/Scenario: R3/R4/R6/R7; D3/D4/D5/D7/D9/D10/D11; S1–S4.
- Depends on: Iteration 002 accepted; parent repository tracks `crates/smoltcp` as ordinary files before Act.
- Targets: stack-runner backlog model test; `tests/ms01_socket_baseline.c::test_tcp_512_capacity`; marker/source
  guards and pure validator seam where needed; fresh QEMU artifact and manual Runbook commands.
- Current behavior: host overflow may remain pending and still pass; guest injects an unclassified overflow then
  immediately releases headroom, so overflow and recovery compete; existing image predates this Cycle.
- Required behavior: host overflow reaches a deterministic terminal result before any headroom release; guest
  omits ambiguous overflow and proves exact-512 accept/refill plus immediate recovery within fixed deadlines;
  diagnostics and full MS01 complete on a matching single-hart image.
- Required changes: write a RED host assertion for terminal overflow; make it GREEN without product changes unless
  evidence locates an existing-product defect; remove guest overflow stimulus; add bounded phases/timeouts while
  preserving all 14 markers; align source/validator checks; rebuild and hand off manual QEMU only after auto Gates.
- Preserve: backlog 512; atomic accept/refill; unique runner; no caller-driven poll or sleep-based recovery;
  14 marker names/count; diagnostic single/fork protocol; MS01 socket semantics; single-hart VirtIO-MMIO boundary.
- Forbidden: raising backlog; sleeping to mask ordering; changing scheduler, reset/cancellation or terminal-fault
  semantics; automated QEMU guest shell; reusing old artifacts/evidence; claiming SMP, hardware or performance.
- Test witness: new host assertion must fail if overflow is accepted as merely `!= Established`; payload source
  guard must fail while the ambiguous nonblocking overflow block remains or marker/deadline contract is missing.
- GREEN condition: host overflow terminal and recovery tests pass in both profiles; C source/validator seams prove
  14 markers and fixed failure boundaries; full automatic Gates pass; fresh manual QEMU yields diagnostic single/fork
  PASS and MS01 14/14 + START/END + exit 0.
- Verification: focused backlog/model tests, both axnet full profiles, C source/validator tests, kernel QEMU check,
  fresh `make build`, payload cross-compile, fmt/source guards, strict OpenSpec, parent/smoltcp diff review, then the
  Runbook's manual QEMU commands with decisive markers and exit status.
- Stop when: host terminal behavior requires a new backlog or TCP contract; guest recovery needs sleep, scheduler or
  cancellation changes; the fresh image fails below the socket-workload layer; or manual output lacks attribution.
  Return to Plan rather than widening this Iteration.

**Invariants**

- Overflow and recovery are separate evidence categories and execute in dependency order.
- The resident runner remains the only smoltcp progress owner; no fixed polling fallback is introduced.
- Host/model evidence cannot replace QEMU application/runtime evidence.
- QEMU single-hart evidence cannot support SMP, physical-board, DMA/cache or performance claims.
- No Service, SocketSet, listener or readiness guard crosses wake, await, Pending or yield.

**Non-goals**

- Tasks 3.1–3.4 terminal readiness and final MS06 application probe.
- Scheduler redesign, SO_LINGER, reset/cancellation, SMP, PCI/DWMAC, physical boards and performance.
- Automated QEMU shell control, global docs synchronization, Evidence directory, archive or commit.

**Traceability Matrix**

| Requirement / Acceptance | Scenario | Design | Task | Code surface | Witness | Status |
|---|---|---|---|---|---|---|
| R3 bounded progress | S1/S2 | D3/D4 | 2.8 | host runner/backlog model | terminal overflow before bounded recovery | Covered |
| R4 ownership/order | S1/S2 | D5/D7/D9 | 2.8 | listener/runner sequence | no headroom release before overflow decision | Covered |
| R6 listener compatibility | S1–S3 | D7/D11 | 2.8 | host model and MS01 capacity case | exact-512 + immediate reconnect, 14 markers | Covered |
| R7 QEMU boundary | S3/S4 | D10 | 2.8 | build, diagnostics, MS01, Runbook | fresh single-hart marker/exit result | Covered |

No Missing or Simplified requirement exists. Repository tracking and user approval are execution-readiness blockers,
not requirement gaps.

**Acceptance**

1. With 512 established clients and no released headroom, host/model observes client 513 reach the planned
   refused/closed terminal state within a fixed poll bound; “not Established” alone is insufficient.
2. Only after Acceptance 1, accepting one initial connection atomically refills idle capacity; an immediate recovery
   connection establishes without sleep/caller polling, and the listener accepts 512 initial plus one recovery.
3. Guest MS01 contains no unclassified 513th fire-and-close stimulus. Its capacity case has fixed phase/deadline
   failures and preserves the exact 14-marker payload contract, including `tcp-512cap` and `tcp-512-recovery`.
4. Both axnet profiles, relevant source/validator tests, kernel check, payload build and fresh image build pass before
   manual QEMU starts. Compiler or sandbox failures are recorded as capability blockers, not product results.
5. Manual RISC-V `virt`, `-smp 1`, VirtIO-MMIO QEMU on the same fresh revision reports diagnostic single PASS,
   diagnostic fork PASS, then MS01 one START, 14 PASS, zero FAIL, one END and exit 0.
6. No terminal-readiness, SMP, physical-board or performance claim is made; full diff review has no unresolved
   Critical or Important finding.

**Verification**

- Run focused host backlog/recovery tests in ordinary and qemu-diagnostics profiles, including repeated bounded
  terminal and recovery sequences where cost permits.
- Run the pure marker validator self-test and source guards for overflow removal, 14 markers and fixed deadlines.
- Run both full axnet suites, kernel QEMU check, axnet fmt, strict OpenSpec and parent/smoltcp whitespace/diff review.
- Build `tests/ms01_loopback_diagnostic`, `tests/ms01_socket_baseline` and a fresh
  `StarryOS_riscv64-qemu-virt.bin`; record commands, exit codes and artifact timestamps.
- After automatic Gates pass, follow `.claude/runbooks/qemu-network-testing.md` manually. Run diagnostic `single`,
  diagnostic `fork`, then MS01; record machine/device configuration, revision, decisive markers and exit results.
- SKIPPED: Tasks 3.1–3.4, SMP, board and performance Gates; they do not decide Task 2.8.

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | guest ambiguity, host assertion gap, marker ABI, build/runtime paths and capability boundaries inspected |
| Design | PASS | host owns overflow terminal proof; guest owns exact-512 recovery; QEMU remains manual and single-hart |
| Iteration Plan | PASS | Task 2.8 is one runtime-compatibility result; terminal readiness remains Iteration 004 |
| Cycle Scope | PASS | only backlog ordering, workload determinism and MS01 runtime compatibility are included |
| Task Contract | PASS | RED, targets, behavior, preservation, forbidden scope, GREEN and stop conditions are self-contained |
| Traceability | PASS | R3/R4/R6/R7 map to host, payload, build and manual QEMU witnesses with no Missing/Simplified |
| Verification | PASS | automatic dependency gates precede fixed-marker manual QEMU; evidence classes remain separate |
| Repository Tracking | BLOCKED | parent `.git/index` is read-only here; user must replace the smoltcp gitlink with ordinary files |
| User Approval | BLOCKED | this expanded Cycle awaits explicit approval; no Act authorization is inferred |

Gate 2 remains BLOCKED on repository tracking and user approval. After both are resolved, Plan may set this Cycle
to `ready`; the draft does not authorize Act.

**Persisted Evidence**

- Mode: none

The Act Response can retain decisive commands, markers, exit codes and manual handoff results. Raw serial logs are
not required unless a blocker would lose its decisive structure.

**Risks and Notes**

- The parent-index conversion is a repository prerequisite, not Task 2.8 product scope. In a writable Git
  environment run `git rm --cached crates/smoltcp` followed by `git add crates/smoltcp`, then confirm the staged
  diff replaces mode 160000 with ordinary files and excludes `target/`.
- The cross compiler produced `Bad system call` in this sandbox. Act must retry once after source changes; if it
  remains blocked, hand off compilation and QEMU together instead of using the 2026-08-24 binaries.
- `scripts/ms01-qemu-test.py::run()` conflicts with the active manual-only Runbook and is not an Acceptance path.
- The qemu-diagnostics `async_rx` flake remains pre-existing; isolate it if reproduced and do not use it to excuse
  a Task 2.8 failure.

## Act Response

- Status: pending

**Implemented**

Pending.

**Changed Files and Symbols**

Pending.

**Deviations from Plan**

Pending.

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: pending
- Full diff reviewed: pending
- Critical findings unresolved: pending
- Important findings unresolved: pending
- Minor findings unresolved: pending

**Verification Evidence**

Pending.

**Persisted Evidence**

None required.

**Experience Candidates**

Pending.

**Remaining Issues**

Pending.

**Commit or Diff Reference**

None.

## Plan Review

- Status: pending

**Review Result**

pending

**Findings**

Pending.

**Deviation Classification**

Pending.

**Acceptance Gaps**

Pending.

**Convergence**

N/A.

**Evidence**

Pending.

**Follow-up Decision**

Pending.

**Iteration Plan Update**

Pending.

**Next Cycle**

None.

**Next Iteration**

None.
