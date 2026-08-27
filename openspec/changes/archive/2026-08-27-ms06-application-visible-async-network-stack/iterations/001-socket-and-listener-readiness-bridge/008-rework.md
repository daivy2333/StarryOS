# Iteration 001 / Cycle 008: strict slot budget and lossless pass progress

## Plan Context

- Status: ready
- Approval: approved by user on 2026-08-25（原话：“批准”）；ready for an explicit openspec-act invocation
- Iteration: 001-socket-and-listener-readiness-bridge
- Cycle: 008-rework
- Cycle Type: rework
- Parent cycle: `007-rework.md`

**Iteration Scope**

- Change tasks: 2.1–2.6
- Depends on: Iteration 000 accepted; Tasks 2.1–2.5 GREEN; Cycle 007 topology/cursor baseline
- Stable baseline: one fixed listener stage visits at most 32 total port-head or queue-slot positions per round,
  preserves every progress event seen during any active pass, survives topology/queue mutation, and parks only
  after a quiet complete pass
- Verification boundary: pending and committed 31/32/33/512 queues, 33+ active listeners, progress during
  primary and later passes, topology mutation, RST-to-Listen, stage isolation and quiet parking pass in both
  host profiles
- Diagnostic boundary: failure is limited to ListenTable slot accounting, pass dirty-state semantics, Service
  listener outcome or runner continuation
- Deferred tasks: 2.7, 2.8, 3.1–3.4

**Cycle Scope**

- Trigger: Cycle 007 Review Result `rework-required`
- Acceptance gaps: committed-slot visits bypass the 32-token budget; progress during a follow-up pass is ignored;
  convergence tests do not distinguish a new progress event from quiet continuation
- Repair items: T2.6-R3, T2.6-R4
- Inherited scope: R3/R4/R6; D4/D5/D7; Task 2.6; Cycle 007 bounded port traversal and topology generation;
  RST ownership; one Service stage; guard-outside wake; caller-zero-progress; Tasks 2.1–2.5 GREEN
- Excluded scope: UDP pending-TX/drop/reap, MS01/manual QEMU, terminal faults, SO_LINGER, reset/cancellation,
  scheduler, SMP, real boards, performance, global docs/archive

**Objective**

Charge every listener queue-slot visit—Pending, Ready or Reset—to the shared 32-token round budget. Treat every
`protocol_progressed` observed during an active pass as dirty state that requires a later bounded pass. A pass
may park only after it completes without observing newer progress or topology invalidation.

**Background**

Cycle 007 removed the all-port pre-scan and added a topology generation, but it skips all committed slots inside
one unbudgeted loop. It also suppresses progress while `follow_up` is true so tests that pass `true` on every
continuation can terminate. Those choices conflict with the approved contract: queue traversal is bounded by
positions, and progress cannot be discarded based on which pass happens to be running.

**Current Baseline**

- Branch `net-k3`; HEAD `0acc08137a5df9d3e1ebce709f3760e6d4471d2d`; Cycle 006 is staged and Cycle 007
  product changes are unstaged in `listen_table.rs` and `stack_runner.rs`.
- `ReconcileCursor` has `port/slot/head_visited/generation/sweeping/rescan/follow_up`; topology mismatch restarts
  from port 0 without a full active-port pre-pass.
- Each head and Pending examination increments `checked`; a `while` over Ready/Reset slots does not.
- Progress sets `rescan` only while `!follow_up`; follow-up completion can park even when that pass observed
  progress.
- Fresh targeted tests are GREEN, but none creates 512 committed slots under one pass or injects a new progress
  event during the follow-up pass.
- Fresh fmt, strict OpenSpec and whitespace checks pass. Full suites contain only the three deferred Task 2.7
  UDP RED tests.

**Current-State Evidence**

- `ListenTable::reconcile` is called once after ingress/egress and receives
  `ingress.socket_changed || egress.socket_changed`; this boolean represents a new observation for that round,
  not a mode flag that tests may keep true during quiet continuation.
- `StackRunnerFuture::poll` self-wakes on `socket_changed` and `listener_sweep_incomplete`. A transition seen
  during the pass may therefore lose its listener reconciliation if the pass parks and the next round has
  `socket_changed=false`.
- `ListenTableEntryInner::examine_slot` already returns `Advance { changed: false }` for committed slots. Calling
  it once per cursor position can charge the token without changing Ready/Reset ownership.
- `SlotExamine::Stay` preserves the current index after removal; `Advance` moves one position. Both can remain
  one-token operations.
- Cycle 007 topology generation and add/remove tests are GREEN and do not need redesign.

**Relevant Code**

| File / Symbol | Current Responsibility | Cycle Use |
|---|---|---|
| `crates/axnet/src/listen_table.rs::ReconcileCursor` | pass position, topology and dirty state | remove pass-class immunity; preserve every active-pass progress event |
| `ListenTable::reconcile` | token accounting and pass completion | charge all slot states; restart after any dirty pass; quiet park |
| `ListenTableEntryInner::examine_slot` | one slot verdict | reuse for Pending/Ready/Reset one-token visits |
| listener/service/runner tests | budget, stage and wake witnesses | add committed backlog and progress-during-later-pass RED cases |

**Critical Path**

```text
round input: protocol_progressed
  -> if pass active: mark pass dirty (independent of primary/follow-up label)
  -> one listener stage, <= 32 tokens
       -> one head token OR one queue-slot token, regardless of slot state
       -> persist cursor and topology generation
  -> pass end
       -> dirty/topology change: start another bounded pass
       -> no dirty state: park
  -> later Service stages still run every round
```

**Implementation Guidance**

Delete the inline committed-slot skip. Visit one queue index per loop iteration through `examine_slot` or an
equivalent O(1) verdict and increment `checked` for every state. Keep `Stay` at the same index after removal and
advance exactly one position otherwise.

Use one dirty/rescan rule for every active pass: any `protocol_progressed=true` after a pass starts requests a
later pass. Do not suppress progress based on a `follow_up` label. Quiet continuation calls use `false`; update
existing convergence tests accordingly. Continuous real progress may keep bounded passes active, but each round
remains capped and later stages run, so this is not busy polling. Park only after a complete pass with no newer
progress or topology change.

**Behavioral Change**

- Ready/Reset scanning becomes bounded exactly like Pending scanning; 512 committed slots cannot be read in one
  round.
- Progress during any primary or later pass is retained until a subsequent complete pass observes the table.
- Tests pass `true` only when modeling a new protocol change and `false` for quiet self-wake continuation.
- Public listener, accept, readiness, error and backlog semantics do not change.

**Change Surface**

| Repair | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| T2.6-R3 | R3/R4, S1/S5/S6 | `listen_table.rs::reconcile/examine_slot` | committed slots skip inline | one token for every slot state |
| T2.6-R4 | R3/R6, S2/S3/S6 | `ReconcileCursor/reconcile` and tests | follow-up ignores progress | pass-independent dirty latch and accurate event model |

**Task Contracts**

### T2.6-R3: charge every queue-slot visit to the stage budget

- Requirement/Scenario: R3, R4; Cycle 007 Acceptance 1 and 4.
- Depends on: Cycle 007 port-head/topology cursor baseline.
- Targets: `listen_table.rs::ListenTable::reconcile`, `ListenTableEntryInner::examine_slot`, outcome docs and
  listener/service tests.
- Current behavior: one `while` can read every committed slot without increasing `checked`.
- Required behavior: one head or one queue slot consumes one token regardless of Pending/Ready/Reset; each round
  performs at most 32 total listener positions; later stages retain their own budgets.
- Required changes: first add a RED witness with at least 512 committed slots; require 32-or-fewer checked/visited
  positions per round and multi-round convergence; then remove inline skip and use one O(1) slot verdict per token.
- Preserve: committed state remains final, unique accept/reset delivery, RST-to-Listen Stay behavior, backlog 512,
  topology generation, guard-outside wakes and one Service listener stage.
- Forbidden: excluding any slot state from traversal accounting; clearing committed slots merely to reduce scan;
  changing backlog or accept ordering; adding a second scan/pre-pass; UDP or scheduler changes.
- Test witness: source lines 447–453 contain the unbudgeted committed-slot loop; a 512-committed test must RED on
  this baseline.
- GREEN condition: one quiet pass over 512 committed slots plus its head consumes exactly 513 tokens across 17
  bounded rounds; no round exceeds 32; Service downstream stages still execute.
- Verification: new tests 100 times in both profiles, existing 31/32/33/512 and topology/RST tests, both scoped
  full suites and full diff review.
- Stop when: bounding committed slots requires changing public accept/error semantics or removing queue entries;
  return to Plan.

### T2.6-R4: retain progress observed during every active pass

- Requirement/Scenario: R3, R6; Cycle 007 Acceptance 2–3.
- Depends on: T2.6-R3 GREEN.
- Targets: `listen_table.rs::ReconcileCursor/ListenTable::reconcile`, progress/quiet tests and runner source witness.
- Current behavior: `follow_up` prevents progress from setting `rescan`; tests continuously pass `true` while also
  requiring convergence.
- Required behavior: each new progress observation during any active pass requires a subsequent bounded pass;
  quiet continuation alone does not; the runner parks after a complete pass with no newer progress.
- Required changes: add RED for progress during a second/later pass at a previously visited listener; remove
  follow-up immunity or replace it with equivalent pass-independent dirty/epoch state; change convergence harnesses
  to pass `true` only for actual injected changes and `false` for self-wake continuation.
- Preserve: event-before-register safety, bounded work per round, topology restart, no periodic fallback, no raw
  handle in cursor state and no guard across wake/Pending.
- Forbidden: capping dirty passes to a fixed count; dropping progress to satisfy a round limit; treating continuous
  progress as quiet; adding fixed ticks, busy loops or caller-driven polling.
- Test witness: current condition `!cursor.follow_up` and follow-up completion are a static RED witness; new
  behavioral case must fail before implementation.
- GREEN condition: progress during primary and later passes always leaves `sweep_incomplete=true` until a later
  quiet pass completes; continuous progress remains round-bounded; after progress stops, one final clean pass parks.
- Verification: targeted 100× both profiles, listener runner/quiet/source tests, fmt/OpenSpec/diff checks and full
  code review.
- Stop when: correct latching needs scheduler changes, a new wake source or an unbounded single-round loop.

**Invariants**

- The resident runner remains the only smoltcp progress owner; socket callers do not regain inline polling.
- Every listener head/slot operation, deferred entry and Router stage is independently bounded to 32 per round.
- Ready/Reset delivery is unique; accept refill remains atomic; RST-to-Listen ownership remains closed.
- Service, SocketSet, listener entry and cursor guards do not cross wake, await, Pending or yield.
- TCP/UDP I/O semantics, PollSet 64/65, backlog 512 and host/model evidence scope remain unchanged.

**Non-goals**

- Task 2.7 UDP queued-TX lifecycle and its three RED tests.
- Task 2.8 MS01 compatibility or manual QEMU runtime.
- Terminal faults, reset/cancellation, SMP, real boards, performance, global docs, Evidence, archive or commit.

**Repair Traceability Matrix**

| Requirement / Acceptance | Gap | Repair | Code Surface | Witness | Status |
|---|---|---|---|---|---|
| R3/R4 bounded stage | committed queue bypasses tokens | T2.6-R3 | reconcile/examine_slot | 512 committed = 513 tokens / 17 rounds | Covered |
| R3/R6 lossless progress | follow-up suppresses dirty state | T2.6-R4 | cursor/pass completion | progress during later pass then clean park | Covered |
| Acceptance 4 regression | later stages/readiness must survive | T2.6-R3/R4 | Service/runner tests | stage isolation + existing topology/RST suite | Covered |

No Missing or Simplified requirement. Iteration Map and public behavior remain unchanged.

**Acceptance**

1. Every listener port-head and queue-slot visit consumes one token; Pending, Ready and Reset are counted equally,
   and no round exceeds 32 listener operations.
2. A 512-committed-slot listener plus head completes one quiet pass in exactly 513 tokens across 17 rounds; Router
   and deferred stages continue to run during the sweep.
3. Progress observed during any active pass—including a later rescan—causes a subsequent bounded pass. After
   progress stops, a final clean pass parks without periodic wake.
4. Cycle 007 topology add/remove, queue shrink/Stay, RST ownership, unique delivery, guard-outside wake and caller
   zero-progress tests remain GREEN in ordinary and qemu-diagnostics profiles.
5. New tests RED on Cycle 007 and GREEN after repair; fmt, source guards, strict OpenSpec and whitespace checks exit
   0; full diff has no unresolved Critical/Important finding.
6. The three Task 2.7 UDP RED remain explicitly SKIPPED. No QEMU runtime, SMP, board or performance claim is made.

**Verification**

- Run new 512-committed accounting and progress-during-later-pass tests 100 times in ordinary and
  qemu-diagnostics profiles.
- Re-run all `reconcile_`, `listener_stage_` and `task_26_listener_` tests in both profiles.
- Run ordinary and qemu-diagnostics axnet lib suites; only the three named Task 2.7 UDP RED may remain and must be
  reported as SKIPPED, not PASS.
- Source guards: no committed-slot skip outside accounting; no pass-class progress immunity; one listener stage;
  no guard-local wake/yield; product sockets contain no `poll_interfaces()`.
- `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check`.
- `openspec validate ms06-application-visible-async-network-stack --strict`.
- `git diff --check HEAD` and complete diff review.
- SKIPPED: UDP Task 2.7, MS01/manual QEMU and later-platform Gates; they do not decide this host/model repair.

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | committed loop, follow-up condition, Service input and runner continuation inspected in actual code |
| Design | PASS | all-state token accounting plus pass-independent dirty state closes both existing gaps |
| Iteration Plan | PASS | two repair items stay inside Task 2.6 and preserve Iterations 001–004 |
| Cycle Scope | PASS | only listener accounting/progress and their witnesses are included |
| Task Contracts | PASS | RED, targets, behavior, preserve/forbidden, GREEN and stop conditions are self-contained |
| Traceability | PASS | R3/R4/R6 -> T2.6-R3/R4 -> cursor/slot/tests -> Acceptance, no Missing/Simplified |
| Verification | PASS | direct RED cases, both host profiles, scoped full suites and static/full-diff Gates are defined |
| User Approval | PASS | 用户于 2026-08-25 明确批准 Cycle 008（原话：“批准”） |

Gate 2 全部 PASS。Cycle 008 已从 `draft` 更新为 `ready`，等待显式调用 `openspec-act`；本次批准不构成自动实施、Plan Review、全局状态同步或收尾授权。

**Persisted Evidence**

- Mode: none

All checks are deterministic and inexpensive to rerun; Act Response can carry decisive output and exit codes.

**Risks and Notes**

- Continuous real progress may keep bounded passes active. This is expected progress-driven work, not a busy loop;
  each round remains capped and other stages run.
- Cycle 008 is the third implementation attempt for this Task 2.6 gap. If the same boundedness/progress issue does
  not shrink, apply the three-failure rule and do not create Cycle 009 for another equivalent attempt.
- Existing warnings and Task 2.7 UDP RED are outside this repair and must not be cleaned or suppressed.

## Act Response

- Status: reported

**Implemented**

- T2.6-R3 (charge every queue-slot visit to the 32-token stage budget): removed
  the unbudgeted inline committed-slot skip loop; every pending/Ready/Reset slot
  position now costs exactly one token via the O(1) `examine_slot` verdict, so a
  512-committed-slot listener plus head completes one quiet pass in exactly 513
  tokens across 17 bounded rounds.
- T2.6-R4 (retain progress during every active pass): removed the `follow_up`
  pass-class immunity from the rescan latch. Any `protocol_progressed=true`
  observed while a pass is active now latches `rescan`; the sweep restarts after
  a dirty pass and parks only after a clean pass with no newer progress.
- Head re-service under progress (non-Planned deviation, see Deviations): while a
  pass is active, a new observation also clears `head_visited` so the current
  port's head (idle transition + refill) is re-served on the same round. This
  restores the guest connect-storm path that R3's per-slot charging alone
  regressed (`task_27_repro_guest_512_recovery_sequence`).

**Changed Files and Symbols**

| File | Symbols |
|---|---|
| `crates/axnet/src/listen_table.rs` | `ReconcileCursor` (removed `follow_up`), `ListenTable::reconcile` (pass-independent rescan latch, head re-service, per-state token accounting, pass-end restart/park), `ListenerReconcileOutcome`, tests (2 new witnesses, converged harnesses updated) |

Only `crates/axnet/src/listen_table.rs` changed. `service.rs` / `stack_runner.rs`
are unchanged from the Cycle 007 staged baseline (verified `git diff` = 0 for both).

**Deviations from Plan**

1. Head re-serve on progress (T2.6-R4): the Plan's R4 "Required changes" specified
   the pass-independent rescan latch and quiet-`false` harnesses. The Plan did not
   specify that a new observation must also re-serve the current port's head on the
   same round. During execution, the deterministic regression
   `task_27_repro_guest_512_recovery_sequence` surfaced: with R3's per-slot
   charging, a pass over ~98 committed slots spans ~4 rounds, so the head (idle
   transition + refill) was re-served only once per long pass; under the 512-connect
   storm the next SYN arrived while the idle hidden socket was still busy
   (SynReceived/Established) and no fresh `Listen` socket had been refilled — smoltcp
   answered the SYN with RST (`connect: Connection refused`), the exact guest symptom
   the test models. Fix: while sweeping, `protocol_progressed=true` also clears
   `head_visited`, re-serving the head that round (one token, within the 32 budget,
   no new wake source, no guard-crossing). This is required to satisfy the Cycle's
   own Verification boundary (only the three named Task 2.7 UDP RED may remain;
   `task_27_repro_guest_512_recovery_sequence` is not one of them), and it preserves
   the 513/17 quiet-pass accounting because quiet continuations never enter the
   progress branch. Added as a necessary in-contract implementation adjustment; no
   public interface, error semantics, state ownership, architecture, scope, test
   strategy or Acceptance criterion changed.
2. Convergence-containment tests (`reconcile_checks_at_most_32...`,
   `reconcile_rotates_across_multiple_listeners_fairly`,
   `reconcile_cursor_survives_accept_removal_between_rounds`,
   `reconcile_bounded_33_active_listeners_without_pre_pass`,
   `reconcile_cursor_survives_listener_removed_mid_sweep`,
   `reconcile_latches_progress_during_sweep_into_follow_up_pass`) now pass `true`
   only on the first (pass-starting) round and `false` for continuation rounds, per
   the Plan's explicit instruction to model `true` as a real new observation and
   `false` as quiet self-wake continuation.

**Blocker Handoff**

None

**Blocker Resolution**

None

**Self-Review**

- Plan compliance: PASS — Repair items T2.6-R3/R4 closed; R3/R4 test witnesses RED
  on the Cycle 007 baseline and GREEN after repair; quiet-pass 513/17 accounting
  exact; no UDP/scheduler/backlog/accept-order changes.
- Full diff reviewed: PASS — reviewed the complete working-tree diff of
  `listen_table.rs` vs the staged Cycle 007 baseline plus the runner/service
  products; only the intended product and test changes present.
- Critical findings unresolved: none
- Important findings unresolved: none
- Minor findings unresolved: see Remaining Issues (pre-existing async_rx flake and
  pre-existing warnings, both outside this Cycle and present on the baseline).

**Verification Evidence**

| 验证项 | 命令 | 输出摘录 | 结论 |
|---|---|---|---|
| R3/R4 targeted | `cargo test --lib reconcile_` ordinary | 14 passed; 0 failed | PASS |
| R3/R4 targeted | same, `--features qemu-diagnostics` | 14 passed; 0 failed | PASS |
| runner/listener | `--lib task_26_listener_`, `listener_stage_` (both profiles) | 2+2 passed each | PASS |
| new 512-accounting witness | `--lib reconcile_charges_one_token_per_committed_slot_and_head` both profiles | 1 passed each (100× inner loop) | PASS |
| new later-pass witness | `--lib reconcile_retains_progress_observed_during_later_traversal_pass` both profiles | 1 passed each (100× inner loop) | PASS |
| regression | `--lib task_27_repro_guest_512_recovery_sequence` both profiles | 1 passed each | PASS |
| full ordinary lib | `cargo test --lib` | 315 passed; 3 failed | 3 = 唯一 deferred Task 2.7 UDP RED，符合 Cycle 基线 |
| full qemu-diagnostics lib | same, `--features qemu-diagnostics` | 335 passed; 3 failed | 同上；另有已知 pre-existing async_rx flake 偶发 |
| format | `cargo fmt -- --check` | 0 diffs | PASS |
| OpenSpec | `openspec validate --strict` | "Change ... is valid" | PASS |
| whitespace | `git diff HEAD --check` | exit 0 | PASS |
| source guards | `grep poll_interfaces` product TCP/UDP/service | none | PASS |
| LSP | `lsp_diagnostics listen_table.rs` | no diagnostics | PASS |

Decisive output for the 513/17 accounting and the regression driver was captured
during execution (see below); per-command exit codes were exit 0 for the passing
suites.

**Persisted Evidence**

None required (mode `none`; all checks are deterministic and cheaply rerunnable;
the decisive outputs are reproducible with the commands above).

**Experience Candidates**

- Runbook candidate: `task_27_repro_guest_512_recovery_sequence` proves the full
  listener chain (runner → Service round → ListenerTable reconcile → accept) on
  loopback. The Cycle 008 fix pattern — "per-slot accounting lengthens the pass;
  re-serve the head on progress so idle refill keeps pace with SYN rate" — is a
  reusable debugging entry for any future "connect refused under backlog fill"
  regression. Candidate only; requires explicit user authorization before
  Recorder creates the Runbook.

**Remaining Issues**

- Pre-existing: `async_rx::tests` flake (`hold_submit_...` /
  `reclaim_hold_drains_to_real_driver_full_without_observing_again`) is
  intermittent under lib-suite parallel runs and reproduces on the Cycle 007
  baseline (2/5 baseline runs failed) — outside Cycle 008 scope; not hidden or
  suppressed by this repair.
- Pre-existing: 9 compiler warnings (smoltcp unused imports/variants, `mut` in
  service.rs tests) are present on the baseline and untouched.

**Commit or Diff Reference**

None (no commit was requested). Modified working tree only: `crates/axnet/src/
listen_table.rs` (unstaged modifications on top of the staged Cycle 007 baseline
already present in the worktree).

## Plan Review

- Status: completed

**Review Result**

replan-required

**Findings**

1. **Blocking — active sweep can skip a live slot after a small accept removal.** `ReconcileCursor.slot` is an
   index into a mutable `VecDeque`. `accept_with` removes the first committed entry with
   `swap_remove_front(idx)`, which shifts queue positions, but the active cursor resets only when
   `cursor.slot > entry.queue.len()`. In a queue larger than the cursor, removing one or a few prefix entries keeps
   `slot <= len`; the next unvisited slot can move behind the cursor and is not examined before the pass parks.
2. **Blocking verification gap — the existing shrink test masks the defect.**
   `reconcile_cursor_survives_accept_removal_between_rounds` advances to slot 31 in a 33-slot queue and removes
   four entries. The new length falls below 31, so the test necessarily takes the reset branch. It does not cover
   a 64/512-slot queue with one prefix removal, where the cursor remains in range but points past a shifted live
   slot.
3. **Blocking wake gap — software publication does not repair the cursor.** The public accept path correctly
   publishes software work after releasing `SOCKET_SET` and listener guards, but `Service::stack_round` passes only
   `ingress.socket_changed || egress.socket_changed` to listener reconciliation. A software-only round therefore
   resumes the invalid cursor with `protocol_progressed=false`; correctness cannot rely on another packet or timer.
4. **Non-blocking — Cycle 008 closes its two direct repair items.** Every Pending/Ready/Reset visit now consumes a
   token, the 512 committed queue completes in 513 tokens across 17 rounds, and progress during primary or later
   passes remains latched. Fresh `reconcile_` runs pass 14/14 in both profiles.
5. **Non-blocking — planned failures remain separated.** Fresh ordinary full suite reports 315 passed and the
   three Task 2.7 UDP RED. Fresh qemu-diagnostics reports those three plus the acknowledged pre-existing
   `async_rx::reclaim_hold_drains_to_real_driver_full_without_observing_again` flake. These do not create a new
   Cycle 008 Acceptance gap.

**Deviation Classification**

- PLAN-INVALID: Cycle 008 inherited the assumption that the Cycle 007 topology cursor and
  `slot > queue.len()` clamp already covered queue shrink. Index validity does not prove traversal validity after
  an in-range front mutation.
- PLAN-OMISSION: the Plan required queue-shrink regression coverage but did not require the large-queue,
  small-removal case that keeps `len` above the cursor.

**Acceptance Gaps**

- Acceptance 4 is not met: queue shrink can skip a remaining live slot, so topology/queue mutation safety is not
  closed.
- Acceptance 5 is not met: the current test set lacks a RED witness for in-range cursor invalidation and the full
  diff still has one unresolved Important finding.

**Convergence**

`reduced`. Cycle 008 closed committed-state budget accounting and pass-independent progress latching. The remaining
gap is isolated to external queue structural mutation versus the persistent index cursor. Because Cycles 006–008
are three attempts at Task 2.6 and the cursor invalidation design assumption is wrong, another equivalent clamp
repair is forbidden; the design must be revised before implementation.

**Evidence**

- `crates/axnet/src/listen_table.rs:448-454`: shrink handling only compares cursor index with current length.
- `crates/axnet/src/listen_table.rs:552-568`: successful accept structurally mutates the queue with
  `swap_remove_front` and does not invalidate reconciliation state.
- `crates/axnet/src/tcp.rs:366-390`: accept publishes software work after guards release.
- `crates/axnet/src/service.rs:465-467`: listener input excludes the software event and uses only protocol
  transition booleans.
- `crates/axnet/src/listen_table.rs:1058-1088`: existing shrink test forces `cursor > len` and cannot witness the
  in-range shift.
- Fresh `cargo test --lib reconcile_`: 14 passed, exit 0.
- Fresh `cargo test --lib reconcile_ --features qemu-diagnostics`: 14 passed, exit 0.
- Fresh full ordinary suite: 315 passed, 3 planned UDP RED, exit 101.
- Fresh full qemu-diagnostics suite: 334 passed, 3 planned UDP RED plus one acknowledged pre-existing async_rx
  flake, exit 101.
- `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check`, strict OpenSpec and `git diff HEAD --check`:
  exit 0 before Review documentation edits.
- Persisted Evidence is `none`; the absent Evidence directory is conforming.

**Follow-up Decision**

Revise D4 and Task 2.6 so listener topology and external queue removal advance an explicit structure generation.
Create `009-replan.md` in the same Iteration. Its RED witness uses at least 64 slots, advances one round, accepts
exactly one committed prefix slot while `len` remains greater than the cursor, then continues with
`protocol_progressed=false` and requires every remaining live slot to be examined before park. The implementation
must not take the cursor or Service lock from `accept_with`; the generation must provide lock-safe invalidation.

**Iteration Plan Update**

The Iteration Map, task ownership, dependencies and Acceptance boundary remain unchanged. D4 and Task 2.6 are
clarified to require structure-generation invalidation for listener add/remove and successful accept queue removal.
Iterations 002–004 remain unchanged.

**Next Cycle**

`009-replan.md`

**Next Iteration**

None; Iteration 001 remains incomplete until a successor Cycle is accepted.
