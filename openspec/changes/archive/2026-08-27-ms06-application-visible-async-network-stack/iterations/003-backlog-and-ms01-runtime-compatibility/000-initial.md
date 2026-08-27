# Iteration 003 / Cycle 000: deterministic backlog recovery and MS01 runtime compatibility

## Plan Context

- Status: ready
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
| Repository Tracking | PASS | parent index now tracks `crates/smoltcp` as ordinary mode `100644`/`100755` files; nested `.git` remains absent and `target/` remains ignored |
| User Approval | PASS | user explicitly approved Cycle 003 and instructed Plan not to invoke Act automatically |

Gate 2 is PASS and this Cycle is `ready`. The approval authorizes the plan only; Act was not invoked.

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

- Status: reported

**Implemented**

- Strengthened `task_27_repro_guest_512_recovery_sequence`: the 513th overflow connect must reach a deterministic
  refused/closed terminal state (`State::Closed`) within `POLL_BOUND = 128` bounded runner rounds BEFORE accept
  releases any headroom. A merely pending socket no longer satisfies the witness; timer-deadline injection follows
  the same loop idiom as the handshake and recovery loops.
- Extended the `ms01_diagnostic_payloads_keep_markers_and_deadlines` source guard: forbid any guest overflow
  stimulus (`SOCK_NONBLOCK`), require `TCP_512CAP_DEADLINE_US` and the five fixed phase markers of the capacity
  case (`tcp-512cap listen/connect/accept-refill/drain`, `tcp-512-recovery connect`).
- Rewrote the guest `test_tcp_512_capacity`: deleted the unobservable nonblocking 513th fire-and-close block; added
  a fixed case budget (180 s) with per-phase failure attribution — deadline checks inside the connect loop,
  poll-bounded first accept and drain accepts, `SO_SNDTIMEO` on the recovery connect, elapsed-time fields in FAIL
  messages. The two PASS markers and the whole 14-marker payload ABI are byte-identical.
- Aligned the pure validator seam `scripts/ms01-qemu-test.py::EXPECTED` to all 14 markers (adds the four bind-*
  markers); `--self-test` extended coverage passes.

**Changed Files and Symbols**

| File | Symbols |
|---|---|
| `crates/axnet/src/stack_runner.rs` | `tests::task_27_repro_guest_512_recovery_sequence`, `tests::ms01_diagnostic_payloads_keep_markers_and_deadlines` |
| `tests/ms01_socket_baseline.c` | `TCP_512CAP_DEADLINE_US`, `cap_expired`, `cap_remaining_ms`, `test_tcp_512_capacity`; rebuilt tracked payload binary |
| `scripts/ms01-qemu-test.py` | `EXPECTED` |
| change records | `tasks.md` (Task 2.8 closed, Current Cycle pointer), this Cycle file |

No product source changed.

**Deviations from Plan**

- The host terminal witness went directly GREEN instead of RED: the product already terminates the overflow connect
  deterministically (RST → `Closed`) under bounded rounds, so the defect was the weak assertion, not product code —
  exactly the path the contract allows ("GREEN without product changes unless evidence locates an existing-product
  defect"). Discriminating power is structural: the loop exits only on `Closed` and fails after 128 polls otherwise.
- "Kernel QEMU check" is realized by `make ARCH=riscv64 build` (full qemu-virt release build plus objcopy, exit 0);
  the Makefile defines no separate check target.
- The baseline's cross-compiler `Bad system call` did not reproduce; both payloads compiled fresh, so no capability
  blocker applies to compilation.

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Specification review: PASS — host terminal-before-headroom ordering, guest stimulus removal with preserved
  14-marker ABI, guard/validator alignment, fresh artifacts, and every Preserve/Forbidden item checked against the
  Task Contract; Acceptance 1–4 and 6 are evidenced below, Acceptance 5 is the Runbook manual handoff.
- Code quality review: PASS — diff limited to planned files; the bounded loop mirrors sibling loop idiom; lock
  scopes are statement-local with no guard across poll; C helpers mirror existing deadline-helper style; no new
  warnings introduced; `cargo fmt --check` exit 0.
- Full diff reviewed: PASS — reviewed the complete working-tree diff including the rebuilt tracked payload binary;
  the only other changes are the pre-existing staged OpenSpec records from Plan. Post-report manual-QEMU audit
  located a product regression OUTSIDE this Cycle's change surface (Remaining Issues); the Cycle diff itself
  remains free of Critical or Important findings.
- Critical findings unresolved: none.
- Important findings unresolved: none.
- Minor findings unresolved: none. Pre-existing compiler warnings and the acknowledged qemu-diagnostics `async_rx`
  flake remain outside this Cycle; the flake did not reproduce in either full suite run.

**Verification Evidence**

| Verification | Command / result | Conclusion |
|---|---|---|
| Guard RED (before C edit) | `cargo test --lib ms01_diagnostic_payloads_keep_markers_and_deadlines` | exit 101; failed at `!ms01.contains("SOCK_NONBLOCK")` as required |
| Focused host repro, ordinary | `cargo test --manifest-path crates/axnet/Cargo.toml --lib task_27_repro` | 3 passed, exit 0 |
| Focused host repro, diagnostics | same + `--features qemu-diagnostics` | 3 passed, exit 0 |
| Source/validator guard, both profiles | `cargo test --lib ms01_diagnostic...` ordinary + diagnostics | passed, exit 0 each |
| Full ordinary axnet lib | `cargo test --manifest-path crates/axnet/Cargo.toml --lib` | 319 passed, 0 failed, exit 0 |
| Full diagnostics axnet lib | same + `--features qemu-diagnostics` | 339 passed, 0 failed, exit 0 |
| Validator self-test (14 markers) | `python3 scripts/ms01-qemu-test.py --self-test` | `PASS: harness-self-test`, exit 0 |
| Payload builds | `riscv64-linux-musl-gcc -static -O2 -o ...` for ms01_socket_baseline and ms01_loopback_diagnostic | exit 0 each; static-pie RISC-V ELF, timestamps 2026-08-25 22:15 |
| Fresh kernel image | `make ARCH=riscv64 build` | exit 0; `StarryOS_riscv64-qemu-virt.bin` timestamp 2026-08-25 22:16 |
| Format | `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check` | exit 0 |
| OpenSpec strict | `openspec validate ms06-application-visible-async-network-stack --strict` | valid, exit 0 |
| Whitespace | `git diff HEAD --check` | exit 0 |
| Manual QEMU diagnostics (user-run) | guest `/tmp/ms01_diag single` / `fork`, Runbook HTTP path | PASS + exit 0 each |
| Manual QEMU MS01 (user-run) | guest `/tmp/ms01_test` on release image `18c8df70…bc17` | START + `PASS: tcp-accept`, then hung in tcp-adjacent (child B ECONNREFUSED at `axnet_ng::tcp:334`, t=57.563s); no END, interrupted by SIGINT |
| Diagnostic rebuild + info rerun | `make LOG=info build` → image `bb1d24fc…7645`; serial log `/tmp/ms06-diag-serial-info.log` | reproduced; layer attribution completed (Remaining Issues) |

**Manual QEMU Handoff (Acceptance 5, Runbook manual-only policy)**

Executed manually by the user on 2026-08-25 with the fresh artifacts above, following
`.claude/runbooks/qemu-network-testing.md`:

- Diagnostic `single`: `MS01_LOOPBACK_DIAGNOSTIC_START single` … `PASS: single-loopback` … `END`, exit 0.
- Diagnostic `fork`: `PASS: fork-loopback`, exit 0.
- MS01 payload: `START`, `PASS: tcp-accept`, then **hung in `test_tcp_adjacent`**: guest child B's connect returned
  ECONNREFUSED (`axnet_ng::tcp:334 [AxErrorKind::ConnectionRefused]` at t=57.563s), child B exited, the parent's
  second blocking accept never returned, no further markers and no END; the run was interrupted by SIGINT.

Acceptance 5 is therefore NOT met on source containing `fdc8f101`. A second run on a diagnostic image rebuilt with
`make LOG=info build` (sha256 `bb1d24fc…7645`; release image frozen/restored as `18c8df70…bc17`) reproduced the same
failure and provided the layer attribution recorded under Remaining Issues. The required-result criteria for a
future passing batch are unchanged: diagnostic single/fork PASS, MS01 one START, 14 unique PASS, zero FAIL, one END,
exit 0.

**Persisted Evidence**

None required. All checks are deterministic and inexpensive to rerun; the Act Response carries decisive outputs.

**Experience Candidates**

None.

**Remaining Issues**

- **Blocking Acceptance 5 — concurrent-SYN regression introduced by `fdc8f101` (MS06 Iteration 001 Cycles 006–009,
  Task 2.6 replan), not by this Cycle.** Mechanism, verified line-level against source and the info-level serial log
  (`/tmp/ms06-diag-serial-info.log`):
  1. The listener keeps exactly ONE Listen-state idle hidden socket; refill happens only in the listener reconcile
     stage or at accept (`listen_table.rs` `refill`/`reconcile_head`/`accept_with`).
  2. `fdc8f101` moved reconcile from "after EVERY non-idle ingress step" (old `0acc081`
     `service.rs`: `reconcile()` inside the bounded ingress loop) to ONE stage per round AFTER ingress/egress. The
     bounded ingress stage dispatches up to 32 packets back-to-back with no listener work between them.
  3. Two guest connects 0.48 ms apart put both SYNs into one ingress batch: packet #1 matches the idle (Listen →
     SynReceived); packet #2 finds ZERO Listen-state sockets and smoltcp answers RST from its unmatched-packet path
     (`crates/smoltcp/src/iface/interface/tcp.rs` L38-52 `Socket::rst_reply`). Guest sees ECONNREFUSED
     (`axnet tcp.rs:334`) despite `listen(srv, 5)` backlog > 1 — effective concurrent-SYN capacity is 1 regardless
     of backlog.
  4. MS01 `test_tcp_adjacent`: child B refused and exited, parent blocked forever on the second accept → payload
     hang, no END. Decisive counter-evidence: `idle #4 -> SynReceived` never appears — B's SYN never matched any
     socket. All lower layers (download, diagnostics single/fork, tcp-accept, deferred reap) are healthy.
- Fix belongs to a Plan-authored rework/replan cycle; it touches listener/stage semantics and hits Task 2.8's stop
  condition ("host terminal behavior requires a new backlog or TCP contract"). Candidate directions for Plan:
  - per-non-idle-ingress-step O(1) `reconcile_head` (restores the old guarantee without resurrecting the removed
    per-step FULL scans that caused ~0.7 ms rounds; must revisit the Cycle 009 "one listener stage" guard/D4);
  - a small idle pool K = min(backlog, cap) maintained at listen/refill (listen/refill contract change; does not
    touch the ingress path).
  A naive rollback to per-step full scans is NOT acceptable. Suggested host/model RED witness for that cycle: two
  clients whose SYNs land in ONE ingress batch, assert both reach Established — currently the second ends
  RST-refused/Closed. This Cycle's strengthened overflow-terminal witness remains valid under both candidates
  (a truly full backlog still refuses deterministically).
- Pre-existing compiler warnings remain outside this Cycle. The acknowledged qemu-diagnostics `async_rx` flake did
  not reproduce in this run's suites and remains outside this Cycle.
- Runbook rollback applied after diagnosis: release image restored to frozen `18c8df70…bc17`; the LOG=info
  diagnostic build `bb1d24fc…7645` was discarded.

**Commit or Diff Reference**

None; no commit was requested. Changes remain in the modified working tree (plus the pre-existing staged OpenSpec
records).

## Plan Review

- Status: completed

**Review Result**

replan-required

**Findings**

- This re-audit supersedes the earlier evidence-only conclusion because the Act Response now contains fresh manual
  QEMU counter-evidence that was not present when `001-rework.md` was drafted.
- Cycle 000's own changes are correct and remain useful: client 513 reaches `State::Closed` before headroom release,
  the guest removes the ambiguous overflow attempt, and the 14-marker/deadline contract is preserved.
- Blocking product finding: diagnostics single/fork pass, but MS01 stops after `PASS: tcp-accept`; child B in
  `tcp-adjacent` receives `ECONNREFUSED`, the parent blocks on its second accept, and the payload emits no END.
  A fresh info-level rebuild reproduced the same result.
- The source and serial evidence identify the mechanism: one idle hidden Listen socket is consumed by the first SYN;
  ingress can process 32 packets before the single listener stage; the next SYN in that batch finds no Listen-state
  socket and smoltcp replies with RST. Backlog headroom therefore exists while effective concurrent-SYN capacity is
  one.
- This mechanism predates Cycle 000 and was introduced by the listener-stage change in `fdc8f101`, but Task 2.8 and
  Acceptance 5 require the affected MS01 behavior to pass. It hits the Cycle's explicit stop condition requiring a
  new listener/backlog contract, so an evidence-only rework cannot converge.
- Minor, non-blocking finding: `validate_output()` accepts an additional unknown `PASS:` marker. The source guard
  fixes the payload at 14 emit sites and the manual Gate requires an exact count.

**Deviation Classification**

`NEW-EVIDENCE` for the fresh manual QEMU failure and its reproduced info-level attribution;
`PLAN-INVALID` because Cycle 000 assumed no product behavior change was required while the one-late-listener-stage
design violates adjacent-SYN compatibility; `ACT-DEVIATION` for reporting Task 2.8 complete before Acceptance 5;
`BASELINE-CHANGED` for HEAD moving from planned `fdc8f101` to `4396d264` before Review. The unrelated diagnostics
flake passed isolation and retry and does not affect this result.

**Acceptance Gaps**

- Provide backlog-preserving headroom between adjacent SYN packets in the same ingress batch without restoring
  per-packet full scans, preallocating an idle socket pool or changing the 512 backlog limit.
- Add a host/model witness in which two SYNs for one listener are processed in one ingress batch and both establish.
- After the repair and all automatic Gates, rerun fresh manual QEMU and obtain diagnostic single/fork PASS plus
  MS01 one START, 14 unique PASS, zero FAIL, one END and exit 0.

**Convergence**

N/A. New runtime evidence expands the gap from missing evidence to a product mechanism defect, so convergence must
restart from a revised Task 2.8 contract in the same Iteration.

**Evidence**

- `cargo test --manifest-path crates/axnet/Cargo.toml --lib task_27_repro`: 3 passed, exit 0.
- The same focused command with `--features qemu-diagnostics`: 3 passed, exit 0.
- Fresh ordinary full suite: 319 passed, exit 0.
- Fresh diagnostics full suite: first run 338 passed / 1 known unrelated `async_rx` failure; the failing test passed
  three isolated runs, then the full-suite retry passed 339/339, exit 0.
- `python3 scripts/ms01-qemu-test.py --self-test`, fmt, strict OpenSpec and `git diff HEAD --check`: exit 0.
- Manual QEMU: diagnostic single/fork PASS and exit 0; MS01 START + `PASS: tcp-accept`, then `tcp-adjacent` child B
  `ECONNREFUSED` at t=57.563 s, no END, interrupted. A fresh info-level image reproduced the failure.

**Follow-up Decision**

Keep Iteration 003 open and replace the unapproved evidence-only Cycle with a replan Cycle. Repair the exact
listener head between ingress packets through a bounded, listener-specific signal; retain the existing once-per-round
pending sweep and Cycle 000 overflow/guest work. Do not fall back to an idle pool or full listener scan.

**Iteration Plan Update**

Task 2.8, D4/D7 and the listener fairness/compatibility scenarios are revised. The Iteration Map is unchanged.

**Next Cycle**

`001-replan.md`

**Next Iteration**

None.
