# Iteration 001 / Cycle 009: listener queue mutation generation

## Plan Context

- Status: ready
- Approval: approved by user on 2026-08-25（原话：“很好，那就批准执行”）
- Iteration: 001-socket-and-listener-readiness-bridge
- Cycle: 009-replan
- Cycle Type: replan
- Parent cycle: `008-rework.md`

**Iteration Scope**

- Change tasks: 2.1–2.6
- Revised task: 2.6
- Depends on: Iteration 000 accepted; Tasks 2.1–2.5 GREEN; Cycle 008 bounded slot accounting and
  pass-independent progress latch
- Stable baseline: one fixed listener stage visits at most 32 port-head or queue-slot positions per round;
  protocol progress and listener queue mutations cannot make an active pass skip a live slot; a quiet complete
  pass parks without periodic polling
- Verification boundary: existing 31/32/33/512, topology, RST and later-pass progress witnesses remain GREEN;
  a large queue with a small accepted prefix removal is fully covered before the sweep parks in both host profiles
- Diagnostic boundary: failure is limited to listener queue structural generation, cursor invalidation,
  `accept_with` removal, or runner continuation after a software event
- Deferred tasks: 2.7, 2.8, 3.1–3.4

**Cycle Scope**

- Trigger: Cycle 008 Review Result `replan-required`
- Acceptance gaps: an active pass can skip a live queue slot when `accept_with` removes a small committed prefix
  but the remaining queue length stays above `cursor.slot`
- Revised task: 2.6
- Inherited scope: R3/R4/R6; D4/D5/D7/D9; 32-token all-state accounting; topology generation;
  pass-independent progress latch; head re-service; RST ownership; one Service listener stage; Tasks 2.1–2.5 GREEN
- Excluded scope: UDP queued-TX lifecycle, MS01/manual QEMU, terminal faults, scheduler, reset/cancellation, SMP,
  real boards, performance, global docs, Evidence, archive and product commits

**Objective**

Make listener queue structural mutation an explicit input to the persistent sweep. If `accept_with` removes a
Ready or Reset slot while a pass is active, the next bounded round must restart from a safe position even when
the runner was woken only by a software event and ingress/egress report no new socket transition.

**Scenario Sketch**

| Scenario | Precondition | Action | Observable result | Failure boundary |
|---|---|---|---|---|
| S1 small prefix removal | active sweep has advanced into a queue of at least 64 slots | accept one committed prefix slot; continue with quiet rounds | every remaining closed/Pending seed commits before park; each round checks at most 32 positions | `cursor <= len` hides the shift and one slot remains Pending |
| S2 no mutation | 512 committed slots and no queue/topology change | run one quiet pass | exactly 513 tokens across 17 rounds | mutation handling changes existing accounting |
| S3 protocol progress | primary or later pass is active | inject new protocol progress | a later bounded pass remains armed until a clean pass completes | structure generation replaces or drops the progress latch |
| S4 topology/RST | listener add/remove or Pending socket returns to Listen | continue bounded rounds | topology restart and idle/redundant ownership remain correct | queue generation breaks topology or Stay semantics |
| S5 software-only wake | accept commits and publishes software work; ingress/egress stay quiet | runner executes the next round | mutation is observed without caller polling or a fabricated protocol transition | sweep parks because `protocol_progressed=false` |

**Current Baseline**

- Branch `net-k3`; HEAD `0acc08137a5df9d3e1ebce709f3760e6d4471d2d`; working tree contains the
  uncommitted MS06 implementation and OpenSpec records.
- Cycle 008 charges every head/slot visit to the 32-token budget and latches progress during every active pass.
  Fresh ordinary and qemu-diagnostics `reconcile_` runs each pass 14/14.
- `ReconcileCursor.slot` advances by index. It resets after queue shrink only when
  `cursor.slot > entry.queue.len()`.
- `accept_with` uses `swap_remove_front(idx)` to remove the first committed slot, then the public accept path
  publishes software work after releasing guards.
- `Service::stack_round` passes only `ingress.socket_changed || egress.socket_changed` as
  `protocol_progressed`; a software wake alone does not make that boolean true.
- Existing `reconcile_cursor_survives_accept_removal_between_rounds` seeds 33 slots, advances to slot 31 and
  removes four entries. The resulting length falls below the cursor and exercises the reset branch, but it does
  not cover a large queue where one removed prefix shifts an unvisited slot behind a still-in-range cursor.

**Current-State Evidence**

- `listen_table.rs::reconcile` lines 448–454 reset only for `slot > len`; `slot <= len` is treated as a valid
  continuation even if a front removal shifted the next unvisited item to `slot - 1`.
- `listen_table.rs::accept_with` lines 552–568 finds a committed entry and structurally mutates the queue with
  `swap_remove_front`, but it does not invalidate the active reconcile cursor.
- `tcp.rs::accept` lines 366–390 publishes `StackEvent` software work after the mutation and guard release.
- `service.rs::stack_round` lines 465–467 does not translate that software event into
  `protocol_progressed=true`; relying on a future ingress/egress transition would violate caller-independent
  progress and quiet semantics.
- Fresh full ordinary suite: 315 passed, three planned Task 2.7 UDP RED, exit 101. Fresh qemu-diagnostics suite:
  334 passed, the same three UDP RED plus the acknowledged pre-existing `async_rx` flake, exit 101.

**Relevant Code**

| File / Symbol | Current responsibility | Planned use |
|---|---|---|
| `crates/axnet/src/listen_table.rs::ListenTable` | owns active listener topology and reconcile cursor | track listener structure generation covering topology and external queue removal |
| `ListenTable::reconcile` | performs the bounded cross-round pass | treat a generation mismatch as bounded restart work, including software-only rounds |
| `ListenTable::accept_with` | atomically consumes Ready/Reset and refills idle | publish queue structural mutation without taking cursor or Service locks |
| listener/service/runner tests | prove budget, ownership, wake and quiet behavior | add the large-queue small-removal RED witness and retain existing regressions |

**Critical Path**

```text
active bounded pass: cursor.slot = k
  -> application accept removes a committed prefix slot
       -> queue positions shift; structure generation increments
       -> unlock -> publish software work
  -> runner executes a quiet protocol round
       -> listener stage observes generation mismatch
       -> restart the active pass from a safe position, <= 32 tokens this round
       -> cover all remaining live slots
  -> final clean pass parks
```

**Implementation Guidance**

Generalize the existing topology generation into a listener-structure generation or add an equivalent queue
mutation generation. Increment it after every production mutation that can invalidate the global `(port, slot)`
cursor: listen/unlisten and successful `accept_with` queue removal. Do not acquire `reconcile_cursor` from
`accept_with`; reconcile currently holds cursor state before entry access, so the reverse order would create a
lock cycle.

On generation mismatch, restart the active pass from a safe position and keep it runnable even if
`protocol_progressed=false`. Mutations performed by the reconcile loop itself may update the cursor directly;
they must not cause an endless restart cascade. Duplicate bounded visits after external mutation are acceptable;
skipping a remaining live slot is not.

Add the RED witness before product changes: seed at least 64 closed/Pending slots, run one 32-token round, accept
exactly one committed prefix slot so the remaining length is still greater than the cursor, continue with
`protocol_progressed=false`, and require every remaining slot to commit before park. Preserve the exact no-mutation
513-token/17-round witness.

**Behavioral Change**

- Successful accept removal becomes a tracked listener structure mutation.
- An active listener pass invalidated by queue mutation restarts safely on the software-driven round, without
  requiring another packet transition or periodic fallback.
- Per-round budget, public accept behavior, backlog size, queue ownership and quiet behavior do not change.

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current responsibility | Planned change |
|---|---|---|---|---|
| 2.6 | R3/R6, S1/S5 | `ListenTable` generation and `reconcile` | topology-only invalidation and length clamp | cover external queue removal and software-only restart |
| 2.6 | R4/R6, S1/S5 | `accept_with` | consume committed slot under entry lock | increment structure generation without taking cursor/Service locks |
| 2.6 | R3/R6, S1–S4 | listener/service/runner tests | miss large-queue small-removal shift | add RED/GREEN mutation witness and retain all Cycle 008 gates |

**Task Contract**

### 2.6: close the active-pass queue-mutation gap

- Requirement/Scenario: R3/R4/R6; D4/D5/D7/D9; S1–S5.
- Depends on: Cycle 008 all-state accounting and progress-latch baseline.
- Targets: `listen_table.rs::ListenTable`, `ListenTable::reconcile`, `ListenTable::accept_with`, listener tests;
  source/full-chain runner tests only where needed to prove software-only continuation.
- Current behavior: a small front removal can shift the next unvisited slot behind an in-range index cursor;
  the pass can then park without examining that slot.
- Required behavior: every external queue shape change invalidates the active pass; the next bounded round restarts
  safely even with `protocol_progressed=false`; all remaining live slots are covered before quiet park.
- Required changes: add the large-queue one-removal RED witness; publish a lock-safe listener structure generation
  from successful accept mutation; consume it in reconcile as bounded restart work; keep internal Stay removal
  locally cursor-safe without restart loops.
- Preserve: maximum 32 listener operations per round; 513/17 quiet accounting; pass-independent progress latch;
  head re-service; topology add/remove; RST-to-Listen ownership; unique Ready/Reset delivery; backlog 512;
  `SERVICE -> SOCKET_SET -> entry`; guard-outside wake; one Service listener stage; Tasks 2.1–2.5 GREEN.
- Forbidden: acquiring cursor or Service locks from `accept_with`; restoring caller-driven poll; fixed ticks;
  unbounded pre-scan; changing `swap_remove_front`/accept ordering without a new Plan decision; changing backlog;
  UDP, scheduler, reset, QEMU workload or platform code.
- Test witness: `reconcile_cursor_survives_small_accept_removal_with_large_queue` or an equivalent deterministic
  test must fail before the product change because one remaining slot stays Pending when the current pass parks.
- GREEN condition: the new witness passes 100 times in ordinary and qemu-diagnostics profiles; no round exceeds
  32 positions; all remaining slots commit; existing exact accounting, later-pass progress, topology, RST, stage
  isolation and quiet tests remain GREEN.
- Verification: targeted listener tests in both profiles; both scoped full suites with only the three Task 2.7 UDP
  RED allowed and the separately reported pre-existing async_rx flake; fmt, source guards, strict OpenSpec,
  whitespace and complete diff review.
- Stop when: correctness needs `accept_with` to take cursor/Service locks, changes public accept ordering or queue
  data structure, introduces unbounded retry, or cannot distinguish external mutation from reconcile-local Stay;
  return to Plan instead of attempting another local repair.

**Invariants**

- The resident runner remains the only smoltcp progress owner.
- Listener, deferred and Router stages retain independent 32-item budgets.
- Ready/Reset delivery is unique; accept refill remains atomic; RST-to-Listen ownership remains closed.
- No Service, SocketSet, listener, cursor or readiness guard crosses wake, await, Pending or yield.
- TCP/UDP behavior, PollSet 64/65, backlog 512 and host/model evidence scope remain unchanged.

**Non-goals**

- Task 2.7 UDP queued-TX lifecycle and its three RED tests.
- Task 2.8 MS01 backlog compatibility or manual QEMU runtime.
- Terminal faults, reset/cancellation, SMP, real boards, performance, global state synchronization, Evidence,
  archive or commit.

**Replan Traceability Matrix**

| Requirement / Acceptance | Gap | Design | Task | Code surface | Witness | Status |
|---|---|---|---|---|---|---|
| R3/R6 lossless bounded listener pass | small prefix removal invalidates index position | D4 structure generation | 2.6 | `ListenTable/reconcile/accept_with` | large queue + one accepted prefix + quiet convergence | Covered |
| R4 lock/wake ordering | accept must invalidate without reverse lock order | D5/D9 | 2.6 | accept mutation and post-unlock software publish | source/lock-order guards | Covered |
| Cycle 008 Acceptance 1–3 | budget, progress latch and quiet accounting remain | D4 | 2.6 | cursor and runner outcome | existing 513/17 and later-pass tests | Covered |
| Cycle 008 Acceptance 4 | queue shrink/Stay survives | D4/D7 | 2.6 | queue structural generation | old shrink test plus new in-range shrink RED | Covered |

No Missing or Simplified requirement. The revised design closes an existing Task 2.6 Acceptance gap; it does not
add a global task or change the Iteration Map.

**Acceptance**

1. With at least 64 queue slots and an active cursor beyond the head, accepting exactly one committed prefix slot
   cannot leave any remaining closed/Pending slot unexamined when the sweep parks; each round reports at most 32
   listener operations.
2. The accept mutation is observed on a software-only runner round. No new ingress/egress socket transition,
   caller-driven poll, fixed timer or unbounded scan is required.
3. The existing 512 committed-slot quiet pass remains exactly 513 tokens across 17 rounds; progress during primary
   and later passes remains latched until a subsequent clean pass.
4. Cycle 007/008 topology add/remove, queue shrink/Stay, RST ownership, unique delivery, head re-service,
   guard-outside wake, stage isolation and caller-zero-progress tests remain GREEN in both profiles.
5. New witness is RED on Cycle 008 and GREEN after the repair; fmt, source guards, strict OpenSpec and whitespace
   checks exit 0; complete diff review has no unresolved Critical or Important finding.
6. The three Task 2.7 UDP RED remain SKIPPED. No QEMU runtime, SMP, board or performance claim is made.

**Verification**

- Run the new large-queue small-removal witness 100 times in ordinary and qemu-diagnostics profiles.
- Re-run all `reconcile_`, `listener_stage_` and `task_26_listener_` tests in both profiles.
- Run ordinary and qemu-diagnostics axnet lib suites. Only the three named Task 2.7 UDP RED may remain; report the
  acknowledged pre-existing async_rx flake separately if it occurs.
- Source guards: successful accept invalidates listener structure without cursor/Service lock acquisition; a
  generation mismatch can continue a software-only active pass; one listener stage; no guard-local wake/yield;
  product sockets contain no `poll_interfaces()`.
- `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check`.
- `openspec validate ms06-application-visible-async-network-stack --strict`.
- `git diff HEAD --check` and complete diff review.
- SKIPPED: UDP Task 2.7, MS01/manual QEMU and later-platform Gates; they do not decide this host/model repair.

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | actual cursor, accept removal, software publish, Service input and inadequate shrink witness inspected |
| Design | PASS | structure generation closes queue-position invalidation without reverse locks or periodic polling |
| Iteration Plan | PASS | revised Task 2.6 stays in Iteration 001; Iterations 002–004 remain unchanged |
| Cycle Scope | PASS | only the existing queue-mutation Acceptance gap and its witness are included |
| Task Contract | PASS | RED, targets, behavior, preserve/forbidden, GREEN, verification and stop conditions are self-contained |
| Traceability | PASS | R3/R4/R6 -> D4/D5/D7/D9 -> Task 2.6 -> code -> mutation witness, no Missing/Simplified |
| Verification | PASS | direct deterministic RED plus both profiles and retained Cycle 008 gates |
| User Approval | PASS | 用户于2026-08-25明确批准Cycle 009并授权立即执行（原话：“很好，那就批准执行”） |

Gate 2全部PASS。Cycle已从`draft`更新为`ready`，用户同时明确授权本次`openspec-act`执行；该授权不覆盖
后续Plan Review、下一Iteration、全局状态同步或收尾。

**Persisted Evidence**

- Mode: none

All checks are deterministic and inexpensive to rerun; the Act Response can carry decisive output and exit codes.

**Risks and Notes**

- Cycle 009 follows the three-failure design review; it is not a fourth equivalent cursor-clamp attempt. The
  generation mechanism changes the invalidation model and adds the missing in-range shrink witness.
- External mutation may cause bounded duplicate visits. This is acceptable; unbounded restart or tail starvation
  is not. The new witness and existing multi-listener tests must demonstrate convergence after mutation stops.
- Existing Task 2.7 UDP RED, compiler warnings and the qemu-diagnostics async_rx flake remain outside this Cycle.

## Act Response

- Status: reported

**Implemented**

- Added the deterministic Cycle 009 witness with 64 closed/Pending slots: one 32-token round, one committed-prefix
  accept, then quiet continuation rounds. It reproduced the skipped live slot on the Cycle 008 implementation and
  passes 100 iterations after the repair.
- Generalized `topology_generation` to `structure_generation`. Listen, unlisten and successful external accept
  removal publish a release update; reconcile observes it with acquire ordering and restarts an active pass from a
  safe position without requiring fabricated protocol progress.
- Kept `accept_with` independent of the cursor and Service locks. Internal reconcile-local `Stay` removal continues
  to update its cursor directly and does not create a generation/restart loop.
- Extended the Task 2.6 source guard to require accept-side generation publication and forbid cursor-lock acquisition
  in `accept_with`.

**Changed Files and Symbols**

| File | Symbols |
|---|---|
| `crates/axnet/src/listen_table.rs` | `ListenTable::structure_generation`, `listen_to`, `unlisten`, `unlisten_with`, `reconcile`, `accept_with`, `reconcile_cursor_survives_small_accept_removal_with_large_queue` |
| `crates/axnet/src/stack_runner.rs` | `task_26_listener_stage_is_single_bounded_call_without_guard_wake` source guard |

The Cycle record and change `tasks.md` were also updated with the user's Gate 2 approval and current-Cycle pointer.

**Deviations from Plan**

None in implementation or scope. The allowed guidance to generalize the topology generation was selected instead
of adding a second counter.

Two command corrections were non-evidence and did not change the implementation: the first RED invocation used an
incomplete `--exact` filter and selected zero tests, so the direct name filter was rerun and produced the required
failure; a later grouped regression command was interrupted before a result and every affected command was rerun
individually.

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Specification review: PASS — reviewed the implementation against the listener queue-shrink scenario, D4 bounded
  stage decision, D5 lock order and D7 listener ownership before code-quality review. The generation restart closes
  the specified in-range prefix-shift gap without changing accept/refill or readiness semantics.
- Plan compliance: PASS — direct RED preceded product repair; every listener round remains at most 32 operations;
  exact 513-token/17-round accounting, later-pass progress latch, topology/RST behavior and quiet parking remain
  GREEN in both profiles.
- Full diff review: PASS — reviewed the complete affected product/test diff and the OpenSpec delta in the dirty
  worktree. No unrelated user changes were reverted or rewritten.
- Critical findings unresolved: none.
- Important findings unresolved: none.
- Minor findings introduced by Cycle 009: none.

**Verification Evidence**

| Verification | Command / result | Conclusion |
|---|---|---|
| Direct RED | `cargo test --lib reconcile_cursor_survives_small_accept_removal_with_large_queue` | exit 101; 1 failed: live slot remained behind cursor |
| Direct GREEN, ordinary | same command after repair | exit 0; 1 passed (100× inner loop) |
| Direct GREEN, diagnostics | same plus `--features qemu-diagnostics` | exit 0; 1 passed (100× inner loop) |
| Listener reconcile regressions | `cargo test --lib reconcile_` in both profiles | 15 passed in each profile |
| Stage and source guards | `listener_stage_` and `task_26_listener_` in both profiles | 2 passed per filter/profile |
| Full ordinary lib | `cargo test --lib` | 316 passed; only the 3 planned Task 2.7 UDP RED failed; exit 101 as expected |
| Full diagnostics lib | same plus `--features qemu-diagnostics` | 336 passed; the same 3 planned UDP RED failed; no async_rx flake occurred |
| Format | `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check` | exit 0 |
| Product caller-poll guard | `rg 'poll_interfaces\\('` over `tcp.rs`, `udp.rs`, `service.rs` | no matches (rg exit 1) |
| OpenSpec | `openspec validate ms06-application-visible-async-network-stack --strict` | valid; exit 0 |
| Whitespace | `git diff HEAD --check` | exit 0 |

**Persisted Evidence**

None required.

**Experience Candidates**

None. This was a local cursor-invalidation repair with deterministic tests, not a new operational procedure or
incident workflow.

**Remaining Issues**

- The three Task 2.7 UDP queued-TX RED tests remain intentionally failing and are outside Cycle 009.
- Existing compiler warnings remain outside this Cycle.
- Manual QEMU runtime, SMP, physical-board and performance validation were not run and are not claimed.

**Commit or Diff Reference**

None; no commit was requested. The implementation remains in the existing modified working tree.

## Plan Review

- Status: completed

**Review Result**

accepted

**Findings**

- No Critical or Important finding remains. The implementation tracks listen, unlisten and successful external
  accept removal through one structure generation and restarts the bounded listener pass from a safe position.
- The new 64-slot witness covers the Cycle 008 in-range cursor invalidation gap. Existing accounting, topology,
  RST ownership, later-pass progress and quiet-path witnesses remain GREEN in both profiles.
- The initially suspected generation/entry race is not reachable through production paths: the runner holds the
  global `SOCKET_SET` guard for the complete `stack_round`, while public accept acquires the same guard before
  `accept_with`. The generation update therefore occurs between stack rounds, matching the test model.
- The qemu-diagnostics full suite reproduced the acknowledged `async_rx` flake once. Its exact test passed on an
  immediate isolated rerun; this is non-blocking and outside Task 2.6.

**Deviation Classification**

None. The implementation and verification follow the revised Task 2.6 contract.

**Acceptance Gaps**

None.

**Convergence**

reduced. Cycle 008 exposed an in-range queue-shift gap; Cycle 009 adds the missing RED witness and closes it without
changing the Iteration target or Acceptance boundary.

**Evidence**

- Code review: `ListenTable::structure_generation`, `ListenTable::reconcile`, `ListenTable::accept_with` and the
  Task 2.6 source guard were checked against S1-S5, the 32-token bound and lock-order constraints.
- `cargo test --lib reconcile_cursor_survives_small_accept_removal_with_large_queue`: exit 0, 1 passed; the test
  contains 100 deterministic iterations.
- The same direct test with `--features qemu-diagnostics`: exit 0, 1 passed.
- `reconcile_` in ordinary and qemu-diagnostics profiles: exit 0, 15 passed in each profile.
- `listener_stage_` and `task_26_listener_` in both profiles: exit 0, 2 passed per filter/profile.
- Ordinary full suite: exit 101, 316 passed and only the three planned Task 2.7 UDP RED failed.
- qemu-diagnostics full suite: exit 101, 335 passed; the same three UDP RED plus the acknowledged `async_rx` flake
  failed. The exact `async_rx` test then passed in isolation with exit 0.
- `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check`, strict OpenSpec validation,
  `git diff HEAD --check` and the product caller-poll source guard all exited 0.
- Persisted Evidence remains `none`; the absent Evidence directory conforms to the Plan Context.

**Follow-up Decision**

Accept Cycle 009 and complete Iteration 001. Its listener-readiness baseline is independently verified, and the
remaining three failing tests belong to the already planned Iteration 002 Task 2.7 scope.

**Iteration Plan Update**

None. The existing Iteration Map remains balanced and dependency-ordered.

**Next Cycle**

None.

**Next Iteration**

`../002-udp-queued-tx-drain-ownership/000-initial.md`
