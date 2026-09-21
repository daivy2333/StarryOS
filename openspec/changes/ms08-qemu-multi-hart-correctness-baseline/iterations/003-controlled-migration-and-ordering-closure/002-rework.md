# Iteration 003 / Cycle 002: Coherent migration-view publication

## Plan Context

- Status: ready
- Iteration: 003-controlled-migration-and-ordering-closure
- Cycle: 002-rework
- Cycle Type: rework
- Parent cycle: `001-replan.md`

**Iteration Scope**

- Change tasks: 4.1–4.5
- Depends on: accepted Iteration 002 and the completed portions of Cycle 001
- Stable baseline: release-safe `AxCpuMask`; controlled UART/network role
  migration and singleton restore; compatible mask ordering/hash; coherent,
  data-race-free migration snapshots; user-waived single-pass verification.
- Verification boundary: close the remaining A4 tuple-coherence gap, then
  preserve Cycle 001's focused host, build, and bounded SMP=16 QEMU results.
- Diagnostic boundary: `MigrationSlot` view synchronization and its focused
  host witness.
- Deferred tasks: 5.1–7.3

**Cycle Scope**

- Trigger: rework-required
- Acceptance gaps: Cycle 001 replaced an unsynchronized `UnsafeCell` view with
  atomic fields, but its seqlock still lacks two-sided ordering. A Release fence
  after the relaxed odd marker does not keep following field stores after that
  fence, and a second Acquire sequence load does not keep preceding field loads
  before it. A reader may therefore accept a mixed tuple on weak memory.
- Repair items: 4.4-R1
- Inherited scope: Tasks 4.1–4.5, D8/D10, A1–A5, the user's explicit waiver of
  multi-round tests in favor of one serial pass, and every Cycle 001 invariant
  not contradicted below.
- Excluded scope: changing migration behavior or wire layout, adding roles,
  revisiting the accepted mask API, restoring 100-round testing, broad locks,
  blanket SeqCst, or entering Iteration 004.

**Objective**

Make every `MigrationSlot::load()` return one fully committed
`MigrationView` on weakly ordered SMP without relying on an incomplete custom
seqlock protocol.

**Background**

Cycle 001's first repair removed the Rust data race by making every view field
atomic. The next repair added a Release fence after the relaxed begin marker,
but Release only orders operations before it; operations after a release may
move before it. The read side has the inverse missing boundary before its final
sequence check. The same A4 gap therefore remained unchanged across the latest
current-Cycle repair and now requires a new self-contained repair contract.

**Investigation Facts**

- Current Baseline:
  - `MigrationSlot` has one long-lived `in_progress` flag for migration
    single-flight and six atomic view fields plus a sequence counter.
  - UART snapshots, network V5 snapshots, migration control, pinned stimulus
    tasks, and boot smokes call `load()` concurrently with the sole migration
    writer's short `store()` calls.
  - The latest Cycle 001 Act reports passing focused host tests, ordinary and
    SMP=16 builds, and a bounded QEMU smoke after the fence change. Those
    results remain valid for unaffected behavior but cannot prove a weak-memory
    ordering that the source contract does not establish.
  - Task 4.1's release-safe mask and Ord/Hash repair are accepted inputs. Task
    4.5's 100-round requirement is explicitly waived by the user; verification
    stays serial and single-pass.
- Current-State Evidence:
  - `store()` at `kernel/src/drivers/uart_migration_logic.rs:283-302` performs a
    relaxed odd RMW, Release fence, relaxed field stores, then a Release even
    store.
  - `load()` at `:247-267` performs an Acquire sequence load, relaxed field
    loads, then another Acquire sequence load.
  - Rust's Acquire/Release model does not provide the two directional barriers
    this custom seqlock needs. Continuing to adjust individual fences would
    retain an unnecessarily subtle proof obligation.
  - A separate short-lived view lock can serialize only the six-field copy. It
    must not reuse `in_progress`, because the pinned stimulus reads the view
    while a migration owns that long-lived single-flight flag.
- Code and Critical Path:
  - writer: migration control owns `in_progress` -> acquire short view lock ->
    replace six atomic fields -> release view lock;
  - reader: snapshot/stimulus -> acquire short view lock -> copy six fields ->
    release view lock;
  - migration wake, affinity, role identity, snapshot ABI, and wire packing do
    not change.

**Implementation Guidance**

Replace the sequence protocol with a dedicated short-lived lock that protects
only one `MigrationAtomicView` load/store. Keep all fields atomic if that avoids
unsafe storage; under the lock they may use Relaxed operations. Acquire the
lock with an Acquire compare-exchange loop and release it through an RAII guard
using a Release store so early returns cannot wedge readers. Do not reuse the
long-lived `in_progress` flag and do not hold the view lock across task wake,
yield, affinity changes, snapshot assembly, or any external call.

**Behavioral Change**

No external behavior changes. Internally, migration view copies become mutually
exclusive short critical sections, so a reader returns either the old complete
view or the new complete view on every supported memory model.

**Task Contracts**

### 4.4-R1: Replace the incomplete seqlock with a bounded coherent view lock

- Requirement/Scenario: A4 / coherent multi-field observation during UART and
  network migration.
- Depends on: Cycle 001's atomic-field `MigrationSlot` and accepted Task 4.1
  mask boundary.
- Targets: `kernel/src/drivers/uart_migration_logic.rs::MigrationSlot` and
  focused witnesses in `tests/ms04-async-rx-host-harness.rs`.
- Current behavior: all fields are atomic, but the custom sequence protocol
  lacks ordering that keeps field accesses inside both sequence observations.
- Required behavior: `load()` and `store()` serialize only their six-field copy
  through a dedicated short-lived lock; every returned tuple is one committed
  view; release is RAII-safe; migration single-flight remains independent.
- Required changes: remove `seq` and the fence-based seqlock; add a private view
  lock/guard; route every view load/store through it; update the structural and
  concurrent witnesses to assert the lock boundary and coherent seeded tuple.
- Preserve: public `MigrationView` and `MigrationSlot` methods, no_std, saved
  role identity, affinity/wake behavior, wire bytes, reject counters,
  single-flight semantics, and all Cycle 001 verification/waivers.
- Forbidden: reusing `in_progress` for view access, holding the view lock across
  external work, raw unsynchronized `UnsafeCell`, a new broad driver lock,
  blanket SeqCst, sleep-polling, or changing migration/wire contracts.
- Test witness: before the product repair, update the structural witness to
  reject `seq`/fence publication and require a distinct short view lock plus
  RAII release; retain the seeded concurrent tuple witness. Observe RED against
  the current seqlock, then GREEN after the replacement.
- GREEN condition: the source guard proves all view field copies are inside the
  dedicated lock and no sequence/fence protocol remains; the single serial
  concurrent witness returns only coherent epoch tuples.
- Verification: compile and run the filtered host migration tests once;
  ordinary and `SMP=16` builds; one bounded SMP=16 QEMU combined smoke; diff
  checks; strict OpenSpec validation. Previously accepted mask tests need not be
  rerun unless their files change.
- Stop when: a correct repair requires changing snapshot ABI, migration state,
  affinity semantics, or the user waiver; return to Plan.

**Invariants**

- `in_progress` owns a whole migration; the new view lock owns only one tuple
  load or store.
- No reader can observe a partially replaced phase/from/to/poll/reject tuple.
- The view lock is released on every path and never surrounds wake/yield/MMIO or
  task-affinity work.
- UART/network role identity, singleton restore, mask safety, and wire layout
  remain unchanged.

**Non-goals**

New migration controls, protocol changes, performance tuning, broader locking,
formal model-checking infrastructure, repeated startup tests, or Iteration 004.

**Acceptance**

- A4 / 4.4-R1: source and focused behavior witnesses show a separate
  Acquire/Release, RAII-released view lock around every six-field copy, with no
  remaining seq/fence seqlock and no torn tuple in the single serial concurrent
  test.
- A1, A2, A3, A5: retained from Cycle 001; A5 uses the user's explicit
  single-pass waiver.

**Requirements Traceability Matrix**

| Requirement / scenario | Design | Repair | Code surface | Test witness | Simplification | Status |
|---|---|---|---|---|---|---|
| coherent migration tuple on weak-memory SMP | D8 | 4.4-R1 | `MigrationSlot::{load,store}` and private view guard | seeded concurrent tuple + lock source guard | None | Covered |
| serial verification boundary | D9 | 4.4-R1 | focused host/build/QEMU gates | one pass per layer | user-approved multi-round waiver | Covered |

**Verification**

- Run the focused migration host subset once and require all selected tests to
  pass.
- Run ordinary and `SMP=16` builds; run one bounded SMP=16 combined smoke and
  require fixed placement plus UART/network migration PASS markers.
- Run `git diff --check`, `git diff --cached --check`, full repair diff review,
  and strict validation of this change.

**Gate 2 Readiness**

- No Missing requirements: PASS — the sole remaining A4 gap maps to 4.4-R1.
- Simplified requirements approved: PASS — no new simplification; the existing
  multi-round waiver is inherited verbatim.
- Investigation complete: PASS — writer, concurrent readers, both missing
  ordering directions, and affected tests are identified.
- Design closed: PASS — use a separate short-lived Acquire/Release view lock
  with RAII release; do not continue the custom seqlock.
- Task executable: PASS — targets, behavior, witness, GREEN, preserve/forbidden,
  verification, and stop conditions are explicit.
- Iteration plan ordered and balanced: PASS — one local repair closes the
  existing Iteration 003 Acceptance; the Iteration Map is unchanged.
- Traceability complete: PASS — A4/D8/4.4-R1/code/tests form one chain.
- Verification sufficient: PASS — source boundary plus a seeded concurrent
  witness and the affected runtime/build gates directly cover the repair.
- No identity-style evidence engineering: PASS.
- No substantive TBD: PASS.
- Change consistency: PASS — no requirement, design, task-map, or ABI update is
  needed.
- Persisted Evidence: PASS — mode `none`; results fit the Act Response.
- User approval: PASS — the user explicitly replied「批准」after reviewing this
  rework Cycle on 2026-09-20.

**Persisted Evidence**

- Mode: none
- Budget: focused command/output summaries fit in the Act Response.

**Risks and Notes**

- The view lock must remain shorter-lived than `in_progress`; merging them
  deadlocks the pinned stimulus reader while migration waits for its progress.
- A host concurrency pass cannot by itself prove memory ordering. The lock's
  Acquire/Release source contract is the proof boundary; the host witness checks
  tuple behavior and guard coverage.

## Act Response

- Status: pending

**Implemented**

Pending.

**Changed Files and Symbols**

Pending.

**Deviations from Plan**

None.

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

- Review Result: pending

**Findings**

Pending.

**Deviation Classification**

None.

**Acceptance Gaps**

Pending.

**Convergence**

N/A

**Evidence**

Pending.

**Follow-up Decision**

Awaiting user approval and Act Response.

**Iteration Plan Update**

None.

**Next Cycle**

None.

**Next Iteration**

None.
