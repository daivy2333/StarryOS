# Iteration 003 / Cycle 000: Controlled migration and ordering closure

## Plan Context

- Status: ready
- Iteration: 003-controlled-migration-and-ordering-closure
- Cycle: 000-initial
- Cycle Type: initial
- Parent cycle: `../002-deterministic-network-placement-and-observability/001-replan.md`

**Iteration Scope**

- Change tasks: 4.1–4.4
- Depends on: accepted Iteration 002
- Stable baseline: UART copier and network owner/runner can widen from their
  fixed singleton affinity to two valid harts, naturally resume on another hart
  without changing logical ownership, and return to their fixed mask; shared
  cross-hart state has explicit, tested ordering roles.
- Verification boundary: invalid-mask fail-closed behavior, task/endpoint and
  ledger continuity, migration observations, UART IER serialization, publish /
  observe ordering, coherent snapshots, generation wrap, terminal-before-wake,
  and 100-round migration stress.
- Diagnostic boundary: checked affinity update, saved task handles, scheduler
  wake selection, UART SPSC/completion/IER state, network lifecycle/ledger/event
  state, or snapshot consistency.
- Deferred tasks: 5.1–7.3

**Cycle Scope**

- Trigger: initial
- Acceptance gaps: None
- Repair items: None
- Inherited scope: accepted Iterations 000–002, including fixed singleton
  placement, unique UART/network roles, remote-ready IPI, direct hart telemetry,
  V5, timer-disabled witness safety, and UART register/recheck/THRE park safety.
- Excluded scope: new task instances, immediate forced migration, IRQ affinity,
  data-plane qualification protocols, reset/link qualification, D1 SMP claims,
  physical-board claims, and performance optimization.

**Objective**

Add QEMU-only, fail-closed controls that update the saved background tasks'
affinity and observe natural block/wake migration while preserving every logical
owner and resource ledger. Close the ordering audit with deterministic witnesses
and modify synchronization only where a witness exposes a real violation.

**Background**

Iteration 002 saved the four background task handles and records each real poll
hart, but their masks remain fixed singletons and the UART snapshot / network V5
migration fields do not yet describe a migration transaction. `axtask` already
provides `set_cpumask_checked`: it validates against the published schedulable
set and leaves the old mask unchanged on empty, out-of-range, or offline input.
The scheduler chooses a legal run queue on the next genuine wake; migration is
therefore an affinity update followed by natural block/wake, not a second task or
an immediate cross-CPU stop protocol.

**Investigation Facts**

- Current Baseline:
  - Iteration 002 Review is accepted. Its reported 92 host witnesses, host Gate,
    axnet 479/511 tests, UART 69+8+10 tests, ordinary/`SMP=16` builds, and bounded
    QEMU startup remain the inherited fixed-placement baseline.
  - `AxTaskRef::set_cpumask_checked` validates through the published
    schedulable mask, commits under the task cpumask lock, increments the shared
    affinity-reject counter on failure, and preserves the previous mask.
  - Runtime migration qualification remains in Iteration 005; this Iteration
    establishes the controls, bounded observations, ordering witnesses, and
    regression baseline they consume.
- Current-State Evidence:
  - `uart_init::{RX_COPIER_TASK, TX_COPIER_TASK}` retain the unique copier
    handles; `uart_smp_snapshot::record_harts` observes every actual copier poll.
    No public migration control exists, and `UartSmpSnapshot` has no migration
    transaction fields yet.
  - `net_placement::{OWNER_TASK, RUNNER_TASK}` retain the unique network role
    handles; `axnet::{owner_hart_tuple, runner_hart_tuple}` provide direct poll
    observations. V5 already reserves `migration_owner_state` and
    `migration_runner_state`, currently zero.
  - Wake placement is local-preferred: after a mask is widened, a wake issued on
    the second allowed hart selects that hart; a successful `Blocked -> Ready`
    transition moves the same task handle to that run queue. Updating a running
    mask does not itself force migration.
  - QEMU `ArceOsUartPort::update_ier` holds the UART `SpinNoIrq` guard while it
    loads/stores `ier_cache` and writes MMIO, so cache RMW and device write share
    one serialization boundary. D1's local-IRQ-only adapter is not an SMP claim.
  - UART ring indices and completion state, network queue/stack generations,
    lifecycle, cause, ticket/epoch, terminal, readiness, and telemetry already
    use a mixture of locks and atomics. Task 4.3 must classify each by role before
    changing any ordering: telemetry may remain Relaxed; publication requires a
    proven Release/Acquire edge; state-changing RMW requires AcqRel; coherent
    tuples require a lock or retrying snapshot.
- Code and Critical Path:
  - UART control: ioctl/control entry -> saved RX/TX handle -> checked two-hart
    mask -> copier blocks -> wake from the second allowed hart -> scheduler
    selects that hart -> the same `record_harts` wrapper records a new poll ->
    restore original singleton mask.
  - Network control: ioctl/control entry -> saved owner/runner handle -> checked
    two-hart mask -> queue/stack event wakes the blocked role -> the same task
    resumes on the second hart -> direct tuple and V5 migration state advance ->
    restore original singleton mask.
  - Ordering: producer commits shared state -> Release/lock boundary -> wake ->
    observer Acquire/lock boundary -> work; terminal and compound snapshot fields
    must be committed before the wake or returned as one coherent observation.

**Implementation Guidance**

Keep one small QEMU-only control surface per driver. Build a two-hart mask from
the role's original singleton plus one caller-supplied published-schedulable
hart; reject an invalid role, missing handle, identical second hart, empty mask,
or any invalid/offline bit without changing the old mask or migration state.
Record a bounded state such as fixed, widened, observed-on-second-hart, restored,
or failed; do not add a run/session identifier.

The control must not claim migration immediately after updating the mask.
Success requires a later poll by the same saved handle on the second hart and
unchanged logical-role/resource invariants. Restore through
`set_cpumask_checked(original_singleton)` and observe a later poll on the fixed
hart before completing the bounded control.

For ordering, start with tests over existing seams. Preserve valid Relaxed
telemetry. In particular, test concurrent QEMU IER set/clear operations through
the real lock-bearing adapter seam or an equivalent extracted policy that proves
the cache RMW and MMIO write cannot diverge. Do not rewrite the QEMU adapter or
touch the D1 adapter unless a scoped witness fails.

**Behavioral Change**

- QEMU diagnostics can independently widen and restore each existing UART or
  network role's affinity using only published-schedulable harts.
- Migration changes the execution hart of the same task after block/wake; it
  never creates a second copier, owner, runner, ring endpoint, or queue owner.
- UART snapshot and network V5 expose bounded migration state sourced from the
  control and direct poll observations. Existing TXDBG and network V1–V4 remain
  unchanged.
- Cross-hart synchronization sites have tested ordering roles; any detected
  under-ordering is repaired at the smallest owning boundary.

**Task Contracts**

### 4.1: Add controlled UART copier migration

- Requirement/Scenario: UART unique SPSC roles; migrate copier; invalid target.
- Depends on: accepted Tasks 2.1–2.6 and 3.6.
- Targets: `kernel/src/drivers/uart_init.rs` saved copier handles and QEMU-only
  control; `uart_smp_snapshot.rs`; `uart_snapshot_types.rs`; ioctl wiring and
  host/model witnesses.
- Current behavior: RX/TX handles are saved and real poll harts are observed,
  but masks remain singleton and no migration state/control exists.
- Required behavior: independently widen RX or TX to `{original, second}` using
  `set_cpumask_checked`, observe the same task naturally poll on `second`, verify
  task/SPSC/ring-index/staged-state continuity, then restore and observe the
  original singleton. Invalid updates preserve the old mask and state.
- Required changes: add bounded QEMU-only operations and migration observations;
  extend the fully-defined UART wire layout without changing TXDBG.
- Preserve: one RX and one TX copier, unique SPSC endpoints, four-stage drain,
  register/recheck/THRE behavior, early console, and D1 workaround.
- Forbidden: a replacement copier, immediate forced migration, full-mask
  fallback, polling for progress, or D1 SMP qualification.
- Test witness: checked-mask reject/preserve cases; same-handle state model;
  block/wake migration and restore observations; exact UART wire offsets/size
  and zeroed reserved bytes; ring/staged continuity.
- GREEN condition: valid two-hart migration and restore advance direct poll
  observations on the same role; every invalid operation leaves the original
  singleton and logical state intact.
- Verification: focused host harness and UART suites, ordinary/`SMP=16`/affected
  D1 compile, and one bounded QEMU control smoke per copier.
- Stop when: migration requires a second endpoint, immediate scheduler stop, or
  changes the D1 synchronization contract; return to Plan.

### 4.2: Add controlled network role migration

- Requirement/Scenario: controlled migration preserves network ownership;
  invalid target.
- Depends on: Task 4.1 control semantics and accepted Tasks 3.1–3.5.
- Targets: `kernel/src/drivers/net_placement.rs` saved owner/runner handles and
  QEMU control; V5 assembly/reserved migration fields; ioctl wiring; axnet
  lifecycle/ledger observations and tests.
- Current behavior: handles and real poll tuples exist; both V5 migration fields
  are zero and there is no mask update control.
- Required behavior: independently widen the existing owner or runner to
  `{original, second}`, observe a natural second-hart poll by the same handle,
  prove lifecycle/generation/descriptor-slot-ticket/readiness continuity, then
  restore the singleton. Invalid updates fail closed.
- Required changes: expose bounded migration operations/state through the
  existing QEMU diagnostic boundary and populate V5's reserved fields.
- Preserve: exactly one owner and runner, Service/SocketSet lock order,
  descriptor/slot/ticket ownership, recovery epochs, and V1–V4 byte semantics.
- Forbidden: a second owner/runner, descriptor movement outside the owner,
  lifecycle restart, or treating a mask update alone as migration success.
- Test witness: invalid-mask preservation; saved-handle identity; owner/runner
  direct tuple transition and restore; unchanged lifecycle, ledger, generation,
  epoch, and readiness fixtures; exact V5 layout remains stable.
- GREEN condition: the same role runs on both allowed harts and returns to its
  fixed hart with no second instance or ledger/readiness drift.
- Verification: focused axnet ordinary/qemu-diagnostics suites, host harness,
  ordinary/`SMP=16` build, and bounded QEMU control smoke per network role.
- Stop when: ownership continuity cannot be observed without changing queue or
  socket contracts; return to Plan.

### 4.3: Close the cross-hart ordering audit

- Requirement/Scenario: state publication before wake; coherent multi-field
  observation.
- Depends on: Tasks 4.1–4.2 expose the migration interleavings.
- Targets: UART completion/ring/placement/IER paths; network queue and stack
  event generations, lifecycle, cause, fault, ticket/epoch/readiness paths;
  snapshot tuple implementations and their focused tests.
- Current behavior: many sites already use the intended ordering, and QEMU IER
  cache plus MMIO are serialized by the UART lock; no consolidated witness yet
  proves all Task 4.3 boundaries under cross-hart interleavings.
- Required behavior: classify every reviewed field as telemetry, publication,
  transition RMW, or coherent tuple; prove its synchronization edge. Add the
  smallest Release/Acquire, AcqRel, or lock/snapshot repair only for a failing
  witness. Prove the existing QEMU IER boundary prevents lost updates.
- Required changes: deterministic register/publish, generation-wrap,
  terminal-before-wake, tuple-consistency, and IER concurrent-RMW witnesses;
  product changes only where RED demonstrates an ordering defect.
- Preserve: valid Relaxed telemetry, established lock ordering, event-driven
  waits, and all single-hart behavior.
- Forbidden: blanket SeqCst conversion, broad new locks, sleep-polling, D1 SMP
  claims, or changes justified only by style.
- Test witness: controlled producer/observer interleavings, two concurrent IER
  bit updates, wrap boundaries, terminal wake visibility, and snapshot retry /
  lock consistency.
- GREEN condition: every publication has a demonstrated observing edge, every
  transition RMW is indivisible, coherent tuples never tear, and IER cache/MMIO
  end at the same merged bitset.
- Verification: focused UART/axnet/model tests plus sanitizer/model tooling only
  if already available; absence of optional tooling is not a failure.
- Stop when: a repair changes an external driver/socket contract or lock order;
  return to Plan.

### 4.4: Run the migration and ordering integration Gate

- Requirement/Scenario: layered MS08 migration/ordering proof.
- Depends on: Tasks 4.1–4.3.
- Targets: all affected focused suites, host integration, builds, and bounded
  migration smoke.
- Current behavior: fixed placement is qualified for this stage; migration and
  consolidated ordering stress are not.
- Required behavior: invalid masks fail closed; all publish/observe, wrap,
  terminal, snapshot, and IER witnesses pass; 100 migrations retain one logical
  role and preserve state; fixed-placement regressions remain green.
- Required changes: record decisive commands, counts, and first failure in Act
  Response; no persistent Evidence is required.
- Preserve: later runtime protocol/qualification work remains deferred.
- Forbidden: one lucky migration, using final output without direct task/ledger
  observations, or adding identity-style evidence machinery.
- Test witness: 100-round deterministic/model stress and bounded per-role QEMU
  smoke, followed by fixed-placement regression.
- GREEN condition: zero invalid commits, duplicate roles, lost events, torn
  tuples, IER lost updates, or resource discontinuities.
- Verification: focused suites -> host Gate -> ordinary/`SMP=16`/affected D1
  builds -> bounded QEMU smoke -> diff checks -> strict OpenSpec validation.
- Stop when: any product failure persists; do not enter Iteration 004.

**Invariants**

- Mask changes apply to saved task handles; no operation creates or restarts a
  logical role.
- Invalid affinity never replaces the last valid mask.
- Migration success requires a direct later poll on the second hart; mask state
  is not execution evidence.
- UART SPSC identity, completion stages, and IER/cache-MMIO consistency survive
  migration.
- Network lifecycle, descriptor/slot/ticket ownership, generations, epochs, and
  readiness survive migration.
- Pure telemetry does not drive synchronization; QEMU evidence is not hardware
  evidence.

**Non-goals**

- Guest/host qualification protocol, combined driver pressure, network reset /
  link interleave, performance measurements, CPU hotplug, IRQ migration,
  multiqueue/RSS, or physical-board behavior.

**Acceptance**

- A1 / Task 4.1: each existing UART copier can widen, naturally migrate, and
  restore while retaining its task and SPSC identity; invalid masks fail closed.
- A2 / Task 4.2: each existing network role can do the same without lifecycle,
  resource-ledger, generation, epoch, or readiness discontinuity.
- A3 / Task 4.3: every audited shared state has the ordering required by its
  synchronization role; QEMU IER cache RMW plus MMIO write is proven serialized.
- A4 / Task 4.4: focused checks, builds, fixed regressions, bounded role smokes,
  and 100-round stress pass with no duplicate role, lost event, or torn state.

**Requirements Traceability Matrix**

| Requirement / scenario | Design | Task | Code surface | Test witness | Simplification | Status |
|---|---|---|---|---|---|---|
| UART copier migration and invalid target | D8 | 4.1 | UART saved handles/control/snapshot | mask preserve, same-handle migrate/restore, wire layout | None | Covered |
| network role migration and ownership continuity | D8 | 4.2 | network handles/control/V5 + axnet ledgers | tuple transition, lifecycle/ledger continuity | None | Covered |
| synchronization-role ordering | D6/D8 | 4.3 | UART IER/completion; network event/state/snapshot | publish/wake, wrap, terminal, IER, tuple tests | None | Covered |
| migration/ordering integration Gate | D9 | 4.4 | affected suites/build/smoke | 100 rounds + bounded per-role smoke | None | Covered |

**Verification**

- RED/GREEN focused witnesses for invalid-mask preservation, same-handle
  migration/restore, ownership/resource continuity, publication ordering,
  generation wrap, terminal-before-wake, coherent snapshots, and QEMU IER RMW.
- UART async/unit/doc/compile-fail and both axnet suites.
- MS04 host harness and affected prior milestone regressions.
- Ordinary, `SMP=16`, and affected D1 compile/build surfaces.
- Bounded QEMU per-role migration smoke plus fixed-placement regressions; formal
  data-plane migration qualification remains Iteration 005.
- `git diff --check`, staged diff check, and strict OpenSpec validation.

**Gate 2 Readiness**

- No Missing requirements: PASS — Tasks 4.1–4.4 map to D6/D8/D9 and the
  migration/ordering scenarios.
- Simplified requirements approved: PASS — none.
- Investigation complete: PASS — saved handles, checked mask update, wake
  selection, direct poll observations, reserved wire fields, IER boundary, and
  synchronization surfaces are identified.
- Design closed: PASS — widen/natural wake/observe/restore semantics and invalid
  update behavior are fixed by D8.
- Tasks executable: PASS — each task names targets, required behavior, witnesses,
  GREEN, preservation rules, and stop conditions.
- Iteration plan ordered and balanced: PASS — control and ownership tasks precede
  the ordering audit and one combined Gate; protocol/runtime qualification stays
  in Iterations 004–005.
- Traceability complete: PASS — all requirements map to design, task, code, and
  direct witnesses.
- Verification sufficient: PASS — host/model interleavings, target builds,
  bounded QEMU smokes, and 100-round stress match this Iteration's claims.
- No identity-style evidence engineering: PASS — migration is proven by saved
  handles, state continuity, and direct poll observations, not run IDs.
- No substantive TBD: PASS.
- Change consistency: PASS — tasks, D6/D8/D9, delta spec, and this Cycle agree.
- Persisted Evidence: PASS — mode `none` is sufficient and results are
  reproducible.
- User approval: PASS — this Iteration is part of the already approved change
  and Iteration Map; this Cycle only expands that existing scope after accepted
  Iteration 002.

**Persisted Evidence**

- Mode: none
- Budget: commands, decisive output, exit codes, changed files/symbols, and the
  100-round summary fit in Act Response and are reproducible.

**Risks and Notes**

- Local-preferred wake placement means the bounded migration stimulus must come
  from the intended second allowed hart; merely widening the mask does not prove
  a migration.
- A task may finish its current bounded round before moving. Controls must wait
  for a block/wake observation, not demand immediate preemption.
- UART snapshot growth occurs before Iteration 004 freezes its guest/host
  protocol. TXDBG remains the compatibility boundary.
- D1 `update_ier` remains outside the SMP claim and must not be generalized from
  the QEMU lock proof.

## Act Response

- Status: pending

**Implemented**

None

**Changed Files and Symbols**

None

**Deviations from Plan**

None

**Blocker Handoff**

None

**Blocker Resolution**

None

**Self-Review**

- Plan compliance: pending
- Full diff reviewed: pending
- Critical findings unresolved: pending
- Important findings unresolved: pending
- Minor findings unresolved: pending

**Verification Evidence**

None

**Persisted Evidence**

None required

**Experience Candidates**

None

**Remaining Issues**

Awaiting implementation.

**Commit or Diff Reference**

None

## Plan Review

- Review Result: replan-required

**Findings**

Blocking scope change before Act:

- After this Cycle was handed off, the user explicitly required the verified
  `cpumask 0.1.0` release out-of-bounds risk to be solved in the current logical
  Iteration. The original Plan Context only protected migration inputs through
  `set_cpumask_checked`; it did not replace the public `AxCpuMask` alias or close
  direct indexed access for scheduler, kernel, driver, and syscall callers.

**Deviation Classification**

BASELINE-CHANGED

**Acceptance Gaps**

- The newly approved mask-safety requirement has no Task Contract, RED/GREEN
  witness, release-mode Gate, or implementation boundary in this frozen Cycle.

**Convergence**

expanded by an explicit user-approved scope addition before implementation.

**Evidence**

- `cpumask 0.1.0::CpuMask::get/set` use `debug_assert(index < SIZE)` and then
  access the backing bit store.
- `axtask::AxCpuMask` is currently a public type alias, so every consumer can
  invoke those methods directly. The workspace has direct indexed calls in
  axtask run-queue/affinity code, kernel placement/witness/snapshot/syscall code,
  and axnet fixtures.
- The prior hart-16 incident proves the failure is reachable in release builds;
  the placement constant fix closes that caller but not the shared API hazard.

**Follow-up Decision**

Do not execute this Cycle. The approved scope changes the target, code surface,
tests, and Acceptance, so create a replan Cycle that first introduces a
repository-owned safe `AxCpuMask` boundary and then performs migration/ordering
work on top of it. Do not patch or fork the Cargo registry crate.

**Iteration Plan Update**

- Iteration 003 expands from Tasks 4.1–4.4 to 4.1–4.5.
- New Task 4.1 owns mask safety; the previous migration/ordering tasks shift to
  4.2–4.5.
- Stable baseline and verification boundary now require debug and release
  out-of-bounds witnesses before migration controls execute.

**Next Cycle**

`001-replan.md`

**Next Iteration**

None
