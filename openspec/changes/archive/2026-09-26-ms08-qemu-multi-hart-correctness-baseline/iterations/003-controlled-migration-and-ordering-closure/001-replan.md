# Iteration 003 / Cycle 001: Safe CPU masks, controlled migration, and ordering closure

## Plan Context

- Status: ready
- Iteration: 003-controlled-migration-and-ordering-closure
- Cycle: 001-replan
- Cycle Type: replan
- Parent cycle: `000-initial.md`

**Iteration Scope**

- Change tasks: 4.1–4.5
- Depends on: accepted Iteration 002
- Stable baseline: `AxCpuMask` rejects indexed access outside its compile-time
  capacity in debug and release builds; UART copier and network owner/runner can
  widen from fixed singleton affinity to two valid harts, naturally resume on a
  second hart without changing logical ownership, and restore their fixed mask;
  shared cross-hart state has explicit, tested ordering roles.
- Verification boundary: mask boundary and compatibility tests in debug/release,
  invalid-affinity fail-closed behavior, task/endpoint and ledger continuity,
  migration observations, UART IER serialization, publish/observe ordering,
  coherent snapshots, generation wrap, terminal-before-wake, and 100-round
  migration stress.
- Diagnostic boundary: local mask wrapper, affinity validation/update, saved
  task handles, scheduler wake selection, UART SPSC/completion/IER state,
  network lifecycle/ledger/event state, or snapshot consistency.
- Deferred tasks: 5.1–7.3

**Cycle Scope**

- Trigger: replan-required
- Acceptance gaps: the handed-off Cycle 000 did not cover the user-approved
  requirement to eliminate `cpumask 0.1.0` release out-of-bounds behavior at the
  shared `axtask::AxCpuMask` API boundary.
- Repair items: None
- Inherited scope: accepted Iterations 000–002 and Cycle 000's controlled
  migration/ordering design, including fixed singleton placement, unique roles,
  direct hart telemetry, remote-ready IPI, V5, timer-disabled witness safety,
  and UART park safety.
- Excluded scope: patching/forking Cargo registry, replacing the `cpumask`
  dependency for unrelated reasons, CPU hotplug, IRQ affinity, new task
  instances, forced immediate migration, qualification protocols, full
  data-plane qualification, D1 SMP claims, physical-board claims, and
  performance optimization.

**Objective**

Make CPU-mask capacity safety an unconditional workspace invariant before
migration controls consume dynamic hart inputs. Then add fail-closed QEMU-only
migration controls and close the ordering audit without changing logical UART or
network ownership.

**Background**

Iteration 002 exposed a release-only capacity bug: placement iterated to a local
hardcoded 64 over `CpuMask<16>`, and `cpumask 0.1.0::get` only guarded the index
with `debug_assert`. An out-of-range read returned a garbage true bit, admitting
nonexistent hart 16. The immediate caller now uses
`axconfig::plat::MAX_CPU_NUM`, but `axtask::AxCpuMask` is still a public alias to
the registry type, so scheduler, kernel, driver, syscall, and future migration
code can call the same debug-only indexed API directly. The user explicitly
approved solving this shared risk in the current logical Iteration.

**Scenario Sketch**

- Valid mask compatibility: given only indices below `MAX_CPU_NUM`, construction,
  read/write, set operations, iteration, affinity validation, run-queue choice,
  and remote wake produce the same results as the current type alias.
- Out-of-range read: given `MAX_CPU_NUM` or `usize::MAX`, debug and release reads
  report false/not-a-member, do not panic, and do not change the mask.
- Out-of-range write: the same indices return a distinct error and leave all
  legal bits, length, iteration, and bytes unchanged.
- Migration input: controls build two-hart masks only through the safe wrapper;
  offline or out-of-capacity targets fail before task state changes.
- Compatibility boundary: no raw registry mask reference/conversion escapes the
  wrapper; D1 and single-hart builds retain legal-input behavior.

**Investigation Facts**

- Current Baseline:
  - `cpumask 0.1.0` is a crates.io dependency of workspace-owned
    `crates/axtask`. Its `CpuMask::get`, `set`, and `mask` use `debug_assert` for
    capacity checks before delegating to `bitmaps`.
  - `crates/axtask/src/api.rs` publicly aliases
    `AxCpuMask = cpumask::CpuMask<{ axconfig::plat::MAX_CPU_NUM }>`. Legal-mask
    behavior is consumed by axtask scheduler/task APIs, kernel placement,
    snapshots, wake witness and scheduling syscall, and axnet affinity seams.
  - The codebase has 117 textual `AxCpuMask`/affinity references; the indexed
    product calls are concentrated in axtask `api.rs`/`run_queue.rs`, kernel
    UART/network placement and witness/snapshot modules, and the scheduling
    syscall. Axnet uses the public type at its lifecycle seams and in tests.
  - `.axconfig.toml` currently sets `max-cpu-num = 16`; it is a usable explicit
    `AX_CONFIG_PATH` for focused debug/release axtask tests.
  - Iteration 002's placement fix and `placement_capacity_matches_axconfig`
    regression remain valid, but they test one caller rather than the shared API.
  - Cycle 000 has no Act implementation. Its migration/ordering investigation
    remains valid and is incorporated below; it must not be executed because its
    frozen contract lacks mask safety.
- Current-State Evidence:
  - The safe containment boundary is workspace-owned `axtask`, not the Cargo
    registry. A repository newtype can keep the registry `CpuMask` private while
    delegating legal constructors, value access needed for comparison/wire use,
    set algebra, copy/equality/order/hash/debug, and iteration.
  - Out-of-range reads need a total, non-panicking membership result because
    placement and validation naturally treat an unknown CPU as absent.
    Out-of-range writes need a distinct error because the legacy return value is
    the previous bit and cannot distinguish rejection from a previous false bit.
  - `AxTaskRef::set_cpumask_checked` already validates non-empty masks against
    the published schedulable set and preserves the old task mask on failure;
    migration controls must retain that second, platform-state validation after
    capacity-safe mask construction.
  - `uart_init::{RX_COPIER_TASK, TX_COPIER_TASK}` and
    `net_placement::{OWNER_TASK, RUNNER_TASK}` retain the unique task handles.
    UART/network hart counters record each real poll. UART snapshot lacks
    migration fields; network V5 reserves two zero migration-state fields.
  - Wake placement is local-preferred. Widening a mask does not itself prove
    migration; a genuine wake issued from the intended second allowed hart must
    produce a later direct poll observation there.
  - QEMU `ArceOsUartPort::update_ier` holds one UART `SpinNoIrq` guard across
    cache RMW and MMIO write. D1's local-IRQ-only adapter remains outside SMP
    qualification.
- Code and Critical Path:
  - Mask safety: public `axtask::AxCpuMask` constructor/access -> unconditional
    capacity check -> private registry mask access -> result/error; all consumers
    compile only against the wrapper.
  - Affinity commit: safe mask construction -> non-empty/capacity validation ->
    published-schedulable validation -> task cpumask lock -> scheduler selection.
  - UART migration: saved handle -> checked two-hart mask -> block/wake from
    second hart -> same recording future polls there -> state continuity check ->
    checked singleton restore.
  - Network migration: saved owner/runner handle -> same mask/wake flow -> direct
    tuple and V5 migration state -> lifecycle/ledger/readiness continuity ->
    singleton restore.
  - Ordering: producer commit -> Release/lock boundary -> wake -> observer
    Acquire/lock boundary -> work; coherent tuples use one lock or retry protocol.

**Implementation Guidance**

Implement `AxCpuMask` as a transparent workspace-owned newtype whose inner
`cpumask::CpuMask<MAX_CPU_NUM>` stays private. Preserve the legal operations used
by this repository. Membership reads must explicitly test `index < MAX_CPU_NUM`
and return false otherwise. Indexed writes must return `Result<previous,
AxCpuMaskError>` (or an equivalently distinct error) and must not call the inner
`set` on failure. Provide checked construction for dynamic indices; infallible
constant/internal constructors may remain only where their precondition is
unconditionally asserted in release. Do not expose `Deref`, `AsRef` to the raw
mask, an unchecked constructor, or a public raw-inner conversion.

Update every workspace indexed write to handle the result deliberately. A
caller iterating an already-proven `0..MAX_CPU_NUM` range may use an explicit
`expect` that documents the invariant; external/dynamic inputs must propagate a
fail-closed error. Add compile/source guards only to prevent raw-type leakage;
behavior tests remain the primary evidence.

After Task 4.1 is green in debug and release, implement the migration and
ordering work inherited from Cycle 000. Migration controls must never use mask
capacity as a substitute for published-schedulable validation.

**Behavioral Change**

- Public `axtask::AxCpuMask` no longer exposes registry `CpuMask` methods
  directly. Out-of-range membership is false; out-of-range mutation is an error
  with byte-for-byte mask preservation in debug and release.
- Legal mask construction, iteration, set algebra, affinity validation,
  scheduling, and remote wake retain their current behavior.
- QEMU diagnostics can widen and restore each existing UART/network role using
  only capacity-valid, published-schedulable masks and direct poll observations.
- UART snapshot/network V5 report bounded migration state; cross-hart shared
  state has tested ordering roles. Existing TXDBG and network V1–V4 are unchanged.

**Task Contracts**

### 4.1: Contain `cpumask 0.1.0` behind a release-safe `AxCpuMask`

- Requirement/Scenario: CPU mask indexed access fails closed in release;
  capacity-bound read/write; legal mask compatibility.
- Depends on: accepted Iteration 002 capacity finding and placement fix.
- Targets: `crates/axtask/src/api.rs` or a dedicated local mask module;
  `crates/axtask/src/{run_queue.rs,task.rs,tests.rs}`; direct workspace
  `AxCpuMask` consumers in kernel, axnet, and scheduling syscall; focused host
  harness/source guards only where necessary.
- Current behavior: `AxCpuMask` is a public registry-type alias; out-of-range
  `get/set` are protected only by `debug_assert`, and the hart-16 incident proved
  a release caller can consume a garbage bit.
- Required behavior: the registry mask is private behind a repository newtype;
  out-of-range read returns false, out-of-range write returns a distinct error
  without mutation, checked dynamic construction rejects invalid indices, and
  legal operations remain compatible.
- Required changes: implement/delegate the used legal API and traits; migrate all
  direct indexed writes to explicit error handling; prevent public raw-inner
  escape; retain `cpumask` only as a private dependency.
- Preserve: `Copy` mask snapshots, deterministic iteration/set algebra,
  affinity semantics, run-queue selection, task cpumask locking, rejection
  telemetry, public task/spawn signatures, and no_std support.
- Forbidden: editing registry files, `[patch.crates-io]` for cpumask, vendoring
  the crate, `Deref`/raw public escape, silent out-of-range write, blanket panic
  for membership reads, or capacity truncation that admits an invalid mask.
- Test witness: first add tests that fail with the alias in release: reads at
  `MAX_CPU_NUM` and `usize::MAX`; writes at both indices return error and preserve
  legal bits/len/iterator/backing bytes; legal new/full/singleton/set algebra and
  scheduler-selection fixtures remain equal. Add a guard that public
  `AxCpuMask` is a struct and no raw escape is exported.
- GREEN condition: focused debug and `--release` tests pass with identical
  boundary results; all consumers compile through the wrapper; the previous
  placement-capacity regression stays green.
- Verification: with `AX_CONFIG_PATH` set to the repository `.axconfig.toml`, run
  focused axtask mask/affinity/selection tests in debug and release; run axtask's
  serial suite, MS04 host harness, root/standalone axnet metadata/build/tests,
  and ordinary/`SMP=16` builds.
- Stop when: a required consumer needs raw registry storage semantics that cannot
  be represented without reopening the public bypass; return to Plan.

### 4.2: Add controlled UART copier migration

- Requirement/Scenario: unique UART SPSC roles; migrate copier; invalid target.
- Depends on: Task 4.1.
- Targets: `kernel/src/drivers/uart_init.rs` saved handles and QEMU control;
  `uart_smp_snapshot.rs`; `uart_snapshot_types.rs`; ioctl wiring and witnesses.
- Current behavior: handles and real poll harts are retained, but masks remain
  singleton and the snapshot has no migration transaction fields.
- Required behavior: independently widen RX or TX to `{original, second}` through
  the safe wrapper and `set_cpumask_checked`, observe the same task naturally
  poll on `second`, verify SPSC/ring-index/staged-state continuity, then restore
  and observe the original singleton. Invalid updates preserve old state.
- Required changes: bounded QEMU-only operations and migration observations;
  extend the fully-defined UART wire layout without changing TXDBG.
- Preserve: one RX/TX copier, unique SPSC endpoints, four-stage drain,
  register/recheck/THRE behavior, early console, and D1 workaround.
- Forbidden: replacement copier, immediate forced migration, full-mask fallback,
  polling for progress, or D1 SMP qualification.
- Test witness: capacity/offline reject-and-preserve cases; same-handle state
  model; block/wake migration and restore; wire offsets/size/zeroed reserves;
  ring/staged continuity.
- GREEN condition: valid two-hart migration and restore advance direct poll
  observations on the same role; every invalid operation preserves the original
  singleton and logical state.
- Verification: focused harness/UART suites, ordinary/`SMP=16`/affected D1
  compile, and one bounded QEMU control smoke per copier.
- Stop when: migration requires a second endpoint, immediate scheduler stop, or
  changes the D1 contract; return to Plan.

### 4.3: Add controlled network role migration

- Requirement/Scenario: controlled migration preserves network ownership;
  invalid target.
- Depends on: Tasks 4.1–4.2.
- Targets: `kernel/src/drivers/net_placement.rs` saved handles/control; V5
  assembly and reserved migration fields; ioctl wiring; axnet lifecycle/ledger
  observations and tests.
- Current behavior: handles and real poll tuples exist; V5 migration fields are
  zero and no update control exists.
- Required behavior: independently widen the existing owner/runner to
  `{original, second}`, observe a natural second-hart poll by the same handle,
  prove lifecycle/generation/descriptor-slot-ticket/readiness continuity, then
  restore the singleton. Invalid updates fail closed.
- Required changes: bounded operations/state through the existing QEMU
  diagnostic boundary and populated V5 migration fields.
- Preserve: one owner/runner, Service/SocketSet lock order, descriptor/slot/ticket
  ownership, recovery epochs, and V1–V4 bytes/semantics.
- Forbidden: second owner/runner, descriptor movement outside the owner,
  lifecycle restart, or mask-update-only success.
- Test witness: invalid-mask preservation; saved-handle identity; direct tuple
  transition/restore; lifecycle/ledger/generation/epoch/readiness continuity;
  unchanged exact V5 layout contract.
- GREEN condition: the same role runs on both allowed harts and returns to its
  fixed hart with no second instance or resource/readiness drift.
- Verification: both axnet suites, host harness, ordinary/`SMP=16` build, and a
  bounded QEMU control smoke per network role.
- Stop when: continuity cannot be observed without changing queue/socket
  contracts; return to Plan.

### 4.4: Close the cross-hart ordering audit

- Requirement/Scenario: state publication before wake; coherent multi-field
  observation.
- Depends on: Tasks 4.2–4.3 expose migration interleavings.
- Targets: UART completion/ring/placement/IER paths; network queue/stack event
  generations, lifecycle, cause, fault, ticket/epoch/readiness paths; snapshot
  tuple implementations and focused tests.
- Current behavior: many sites use the intended ordering, and QEMU IER cache plus
  MMIO are serialized by the UART lock; no consolidated witness covers every
  Task 4.4 boundary.
- Required behavior: classify each reviewed field as telemetry, publication,
  transition RMW, or coherent tuple and prove its synchronization edge. Repair
  only failing witnesses with the smallest suitable ordering/lock change.
- Required changes: deterministic register/publish, generation-wrap,
  terminal-before-wake, tuple-consistency, and concurrent IER-RMW witnesses.
- Preserve: valid Relaxed telemetry, lock ordering, event-driven waits, and
  single-hart behavior.
- Forbidden: blanket SeqCst, broad new locks, sleep-polling, D1 SMP claims, or
  style-only synchronization changes.
- Test witness: producer/observer interleavings, two concurrent IER bit updates,
  wrap boundaries, terminal visibility before wake, and coherent tuple reads.
- GREEN condition: every publication has an observing edge, transitions are
  indivisible, tuples do not tear, and IER cache/MMIO end at the merged bitset.
- Verification: focused UART/axnet/model tests; optional existing sanitizer/model
  tools may supplement but are not required.
- Stop when: a repair changes an external driver/socket contract or lock order;
  return to Plan.

### 4.5: Run the mask-safety, migration, and ordering Gate

- Requirement/Scenario: layered mask and migration/ordering proof.
- Depends on: Tasks 4.1–4.4.
- Targets: all affected focused suites, host integration, builds, and bounded
  migration smoke.
- Current behavior: fixed placement is qualified; shared mask capacity safety,
  migration, and consolidated ordering stress are not.
- Required behavior: debug/release mask boundary tests pass; invalid masks fail
  closed; publish/observe, wrap, terminal, snapshot, and IER witnesses pass; 100
  migrations retain one role and state; fixed regressions remain green.
- Required changes: record decisive commands/counts/first failure in Act
  Response; no persisted Evidence is required.
- Preserve: later protocol/runtime qualification remains deferred.
- Forbidden: testing only debug assertions, one lucky migration, final output
  without direct state observations, or identity-style evidence machinery.
- Test witness: release mask boundary suite, 100-round deterministic/model
  stress, bounded per-role QEMU smoke, then fixed-placement regression.
- GREEN condition: zero raw-mask leaks, garbage bits, rejected-write mutations,
  invalid commits, duplicate roles, lost events, torn tuples, IER lost updates,
  or resource discontinuities.
- Verification: mask-focused debug/release -> axtask/UART/axnet focused suites ->
  host Gate -> ordinary/`SMP=16`/affected D1 builds -> bounded QEMU smoke -> diff
  checks -> strict OpenSpec validation.
- Stop when: any product failure persists; do not enter Iteration 004.

**Invariants**

- Registry `CpuMask` is private implementation detail; all indexed workspace
  access passes through unconditional capacity checks.
- Rejected writes preserve the complete mask; invalid affinity never replaces
  the last valid task mask.
- Mask changes apply to saved task handles and never create/restart a role.
- Migration success requires a direct later poll on the second hart; mask state
  alone is not execution evidence.
- UART SPSC/completion/IER consistency and network lifecycle/ledger/generation/
  epoch/readiness survive migration.
- Pure telemetry does not drive synchronization; QEMU is not hardware evidence.

**Non-goals**

- Upstreaming or replacing `cpumask`, general-purpose bitmap redesign, CPU
  hotplug, IRQ migration, multiqueue/RSS, guest/host protocol, combined pressure,
  reset/link qualification, performance work, or physical-board behavior.

**Acceptance**

- A1 / Task 4.1: public `AxCpuMask` has unconditional capacity safety; boundary
  reads are false, boundary writes return error without mutation, legal behavior
  is compatible, and debug/release tests agree.
- A2 / Task 4.2: each existing UART copier widens, naturally migrates, and
  restores while retaining task and SPSC identity; invalid masks fail closed.
- A3 / Task 4.3: each network role does the same without lifecycle, resource,
  generation, epoch, or readiness discontinuity.
- A4 / Task 4.4: every audited shared state has ordering appropriate to its role;
  QEMU IER cache RMW plus MMIO is proven serialized.
- A5 / Task 4.5: focused checks, builds, fixed regressions, bounded role smokes,
  release mask suite, and 100-round stress pass without forbidden outcomes.

**Requirements Traceability Matrix**

| Requirement / scenario | Design | Task | Code surface | Test witness | Simplification | Status |
|---|---|---|---|---|---|---|
| release-safe mask read/write and legal compatibility | D10 | 4.1 | local `AxCpuMask`, all indexed consumers | capacity/usize::MAX, immutable reject, legal ops, release suite | None | Covered |
| UART copier migration and invalid target | D8/D10 | 4.2 | UART handles/control/snapshot | same-handle migrate/restore, wire/state continuity | None | Covered |
| network role migration and ownership continuity | D8/D10 | 4.3 | network handles/control/V5/ledgers | tuple transition, lifecycle/ledger continuity | None | Covered |
| synchronization-role ordering | D6/D8 | 4.4 | UART IER/completion; network event/state/snapshot | publish/wake, wrap, terminal, IER, tuple tests | None | Covered |
| combined Iteration Gate | D9/D10 | 4.5 | affected suites/build/smoke | release mask + 100 rounds + per-role smoke | None | Covered |

**Verification**

- With `AX_CONFIG_PATH` pointing to the repository `.axconfig.toml`, run focused
  axtask mask/affinity/selection tests in both debug and `--release`, followed by
  the serial axtask suite under its documented host-safe feature set.
- Run RED/GREEN witnesses for invalid-mask preservation, same-handle migration /
  restore, ownership/resource continuity, publication ordering, generation
  wrap, terminal-before-wake, coherent snapshots, and QEMU IER RMW.
- Run UART async/unit/doc/compile-fail, both axnet suites, MS04 host harness, and
  affected predecessor regressions.
- Build ordinary, `SMP=16`, and affected D1 surfaces.
- Run bounded QEMU per-role migration smoke plus fixed-placement regressions;
  formal data-plane migration qualification remains Iteration 005.
- Run `git diff --check`, staged diff check, full diff review, and strict
  OpenSpec validation.

**Gate 2 Readiness**

- No Missing requirements: PASS — Tasks 4.1–4.5 cover mask safety,
  migration, ordering, and integration.
- Simplified requirements approved: PASS — none.
- Investigation complete: PASS — dependency source/behavior, alias leakage,
  indexed callers, local containment boundary, saved handles, wake selection,
  telemetry, and synchronization surfaces are identified.
- Design closed: PASS — false-on-OOB read, error-and-no-mutation write, private
  inner type, legal compatibility, migration semantics, and ordering roles are
  explicit.
- Tasks executable: PASS — every task names targets, required changes, witnesses,
  GREEN, preservation rules, and stop conditions.
- Iteration plan ordered and balanced: PASS — mask safety is a prerequisite for
  the migration controls consuming dynamic hart IDs; all five tasks form one
  shared affinity/ordering baseline, while protocols remain deferred.
- Traceability complete: PASS — requirements map to D8–D10, tasks, code surfaces,
  and direct debug/release/runtime witnesses.
- Verification sufficient: PASS — release behavior is tested directly rather
  than inferred from debug; migration and ordering claims retain layered tests.
- No identity-style evidence engineering: PASS.
- No substantive TBD: PASS.
- Change consistency: PASS — proposal, spec, design, tasks, Iteration Plan, and
  this replan Cycle contain the approved scope.
- Persisted Evidence: PASS — mode `none`; outputs are reproducible and fit the
  Act Response budget.
- User approval: PASS — 2026-09-20 instruction explicitly adds the cpumask risk
  to the current Cycle and requests that it be solved with the existing work.

**Persisted Evidence**

- Mode: none
- Budget: commands, decisive output, exit codes, files/symbols, and stress
  summaries fit in Act Response.

**Risks and Notes**

- A wrapper that implements `Deref`, exposes its inner value publicly, or allows
  unchecked conversion would leave the original defect reachable and fails A1.
- Some existing writes occur under a proven bounded loop; they still must handle
  the new write result so future refactors cannot silently discard rejection.
- Local-preferred wake means the migration stimulus must originate on the
  intended second allowed hart; widening the mask alone proves nothing.
- UART snapshot growth precedes Iteration 004 protocol freeze. TXDBG remains the
  compatibility boundary.
- D1 `update_ier` remains outside the SMP claim.

## Act Response

- Status: reported

**Implemented**

- **Task 4.1 (mask safety)**: replaced the public `AxCpuMask` registry alias with
  a workspace-owned bounded newtype `crates/axtask/src/cpumask.rs`. The registry
  `cpumask::CpuMask` stays private; `get` reports `false` for any index at or
  above `AX_CPU_MASK_CAPACITY`, `set` returns `Result<bool, AxCpuMaskError>` and
  never mutates on failure, and `try_one_shot` provides checked construction.
  No `Deref`/`AsRef`/raw-inner conversion escapes. Every workspace indexed write
  now handles the result deliberately: dynamic user input (sched_setaffinity)
  propagates `AxError::InvalidInput`; proven in-range loops use a documented
  `expect`; placement-derived singleton masks use a documented invariant expect.
- **Task 4.2 (UART copier migration)**: added pure decision/state module
  `kernel/src/drivers/uart_migration_logic.rs` (target selection + migration
  record + shared `MigrationSlot`), a QEMU-only `migrate_copier` control in
  `uart_smp_snapshot.rs` that widens an existing copier to `{orig, second}`,
  wakes it via a one-shot task pinned to the second hart through the driver's own
  `wake_rx_copier`/`wake_tx_copier` seams, observes a direct poll there, restores
  the singleton and observes a later poll on the origin. Extended the UART wire
  snapshot at offset 152 with migration fields (size 152 → 224); TXDBG and all
  pre-existing offsets unchanged. Added ioctl `UART_SMP_MIGRATE` and a
  `migration_boot_smoke` printing `[UART-MIG-SMOKE]`.
- **Task 4.3 (network role migration)**: added `migrate_role`/`migration_boot_smoke`
  in `net_placement.rs` for the owner and runner saved handles (never a second
  instance), waking through `axnet::software_nudge()` (owner) and a new public
  `axnet::runner_software_nudge()` (runner). V5 now populates
  `migration_owner_state`/`migration_runner_state` from the packed migration
  view instead of hard-zero. Added ioctl `NET_IRQ_MIGRATE` and a `[NET-MIG-SMOKE]`
  built-in smoke. Continuity of lifecycle/ledger/generation/fault and strict
  poll progress are checked per role.
- **Task 4.4 (order audit)**: classified shared state (telemetry Relaxed /
  publication Release+Acquire / transition RMW / coherent tuple) and added the
  missing focused witnesses: generation-wrap boundary for `QueueEvent::wait_decision`
  and `StackEvent::changed_since`; a concurrent IER cache-RMW model proved
  serialized-with-MMIO under one guard, plus a RED twin (unlocked model diverges
  under a forced interleaving) proving witness power, plus a source guard that
  `ArceOsUartPort::update_ier` holds one `SpinNoIrq` across cache RMW and MMIO
  write. Terminal-before-wake and coherent tuple reads are already witnessed by
  prior axtask `fatal_service_round_wake_observes_faulted_lifecycle`
  / `fatal_arm_recheck_wake_observes_faulted_lifecycle` / `HartCounter` tests;
  coverage unchanged, no failing witness found, so no repairs were required.
- **Task 4.5 (integration)**: full layered Gate below.

- **Cycle 001 repair / finding 1 (Task 4.4 — data-race-free & weak-memory correct
  `MigrationSlot`)**: replaced the raw `UnsafeCell<MigrationView>` + `unsafe impl
  Sync` with an atomic seqlock. Each `MigrationView` field is now an atomic
  (`AtomicU8`/`AtomicUsize`/`AtomicU64`). `store` marks in-progress (seq odd),
  then `core::sync::atomic::fence(Ordering::Release)` before the relaxed field
  stores — this orders the odd marker before every field store so no field is
  observable before the odd sequence is published on weakly ordered RISC-V SMP —
  then publishes the even commit with an `Ordering::Release` store. `load` retries
  until it reads a stable even `seq` twice (Acquire), so every reader returns one
  fully-committed tuple and no non-atomic read/write race exists. `unsafe impl
  Sync` removed (now derived). Added host witness
  `migration_slot_concurrent_load_is_always_coherent` (seeded with a coherent epoch
  0 tuple before either thread starts) that drives a writer + reader thread and
  asserts every `load()` yields a tuple whose fields all derive from one epoch
  (rejects torn views), and structural guard `migration_slot_has_no_unsafe_cell_sync`
  updated to require the Release fence + Release even commit instead of a bare
  relaxed marker.
- **Cycle 001 repair / finding 2 (Task 4.1 — `Ord`/`Hash` compatibility)**:
  `AxCpuMask`'s `Ord` and `Hash` now delegate to the private registry
  `self.inner` (`self.inner.cmp(&other.inner)`, `self.inner.hash(state)`) instead
  of comparing/hashing the native byte slice, so legal masks order and hash
  exactly as the replaced `cpumask::CpuMask<16>`. Added `#[cfg(test)]` witnesses
  in `cpumask.rs`: `ord_matches_registry_numeric_store_across_byte_boundary`
  (cross-byte case bit7 < bit8, plus a cross-section ordering oracle against the
  registry type) and `hash_delegates_to_registry_backing_store`; both pass in
  debug and release. Added structural guard
  `mask_ord_hash_delegate_to_inner_in_source`.
- **Cycle 001 repair / finding 3 (Task 4.5 — 100-round migration stress → user
  waiver)**: the Plan Review required a 100-round deterministic/model migration
  stress. The user explicitly waived multi-round testing with the recorded
  instruction:「启动对于多轮测试的地方我给出豁免，各项测试串行测试且只测试一次通过就行」.
  Per the waiver I did not build a 100-round loop; instead added a deterministic
  single-cycle serial-pass witness
  `migration_single_cycle_serial_pass` (select → begin → observe → restore →
  invalid-op fail-closed → role-identity preserved, covering the shared seams
  used by both UART and network roles) and ran each layer of verification serially
  once. The per-role runtime migration is additionally witnessed by the bounded
  QEMU smoke below.

**Changed Files and Symbols**

- `crates/axtask/src/cpumask.rs` (new): `AxCpuMask`, `AxCpuMaskError`,
  `AX_CPU_MASK_CAPACITY`, `Iter`; `Ord`/`Hash` delegate to the private inner
  registry store; `#[cfg(test)]` module adds the cross-byte ordering oracle and
  backing-store hash compatibility witnesses.
- `crates/axtask/src/api.rs`: alias → newtype re-export; `cpu_mask_full` expect.
- `crates/axtask/src/lib.rs`: `mod cpumask;`.
- `crates/axtask/src/run_queue.rs`: `schedulable()` expect.
- `crates/axtask/src/tests.rs`: mask boundary/legal/guard/newtype-size/checked
  tests; rewritten `spawn_with_affinity_rejects_invalid_mask`; `set` result handling.
- `kernel/src/syscall/task/schedule.rs`: `sched_setaffinity` fail-closed.
- `kernel/src/drivers/uart_init.rs`: `copier_task`, `singleton_mask_for`, singleton expect.
- `kernel/src/drivers/uart_migration_logic.rs` (new): `select_second`,
  `MigrationPhase`, `MigrationRecord`, `MigrationView`, `pack_migration_view`,
  `unpack_migration_view`; `MigrationSlot` implemented as an atomic-field seqlock
  (private `MigrationAtomicView`), no `UnsafeCell`, no `unsafe impl Sync`; begin
  marker followed by `fence(Ordering::Release)`, even commit is `Ordering::Release`
  (weak-memory coherent on RISC-V SMP).
- `kernel/src/drivers/uart_snapshot_types.rs`: migration wire fields + reserved.
- `kernel/src/drivers/uart_smp_snapshot.rs`: `migrate_copier`,
  `migration_boot_smoke`, migration atomics + snapshot fields.
- `kernel/src/drivers/net_placement.rs`: `NetRole`, `migrate_role`,
  `migration_view`, `migration_boot_smoke`, continuity helpers.
- `kernel/src/drivers/virtio_net_irq.rs`: V5 migration population.
- `kernel/src/drivers/mod.rs`: `uart_migration_logic` module.
- `kernel/src/syscall/fs/ctl.rs`: `UART_SMP_MIGRATE`, `NET_IRQ_MIGRATE`.
- `kernel/src/entry.rs`: `migration_boot_smoke` hooks (UART + net).
- `crates/axnet/src/stack_runner.rs`: `runner_software_nudge` + wrap/nudge tests.
- `crates/axnet/src/lib.rs`: export `runner_software_nudge`.
- `crates/axnet/src/async_rx.rs`: `queue_event_wait_decision_retries_across_wrap_boundary`.
- `crates/uart_16550/src/async_/driver.rs`: `wake_rx_copier`, `wake_tx_copier`.
- `tests/ms04-async-rx-host-harness.rs`: harness `#[path]` include; mask/migration/
  packing/IER/guard tests; wire-layout extension 152 → 224; Cycle-001 repairs add
  concurrent-tuple coherence, single-cycle serial pass, and Ord/Hash + seqlock
  source guards.

**Deviations from Plan**

- Restore-observation wake is re-issued on every bounded probe (not a single
  wake): a wake against a still-Running task is dropped in axtask's block_on
  park, so the first wake can be lost in the migration window. This is a local
  implementation detail within the Task 4.2/4.3 contract; it surfaced as a real
  QEMU runtime failure on the first smoke and is now covered by repeated smokes.
- The cumulative poll-mask continuity check asserts "no third hart" (mask ⊆
  {from, to}) rather than "mask == singleton", because the cumulative mask is
  historical and legitimately includes both migration harts after a cycle.
- `MigrationSlot` was moved into the shared pure logic module so UART and
  network controls reuse one fail-closed state holder (Task 4.1 review: avoid
  duplicating an atomic/UnsafeCell state machine in two drivers).
- `pack_migration_view` encodes the `AUTO_HART` sentinel as 0 on the wire
  (from/to are meaningless while `phase == None`).
- **Cycle 001 repair / environment (not a product change)**: the untracked
  generated `.axconfig.toml` had been left at `max-cpu-num = 1`, so the SMP=16
  firmware compiled with `AX_CPU_MASK_CAPACITY = 1` and the idle-task pin
  (`AxCpuMask::one_shot(cpu_id)` in `run_queue.rs:792`) panicked for hart 15.
  The Plan Context documents `.axconfig.toml` as `max-cpu-num = 16`. I regenerated
  it with `make defconfig SMP=16` (→ `plat.max-cpu-num=16`); afterwards the
  SMP=16 build and bounded QEMU smoke pass. No product code was changed for this.
- **Cycle 001 repair / waiver recorded**: the 100-round migration stress requested
  by the Plan Review was replaced with a single serial pass per the user's
  explicit waiver. See the repair 3 note above for the verbatim instruction.
- **Cycle 001 repair / Task 4.4 ordering (re-review)**: after the first review the
  seqlock's begin-write marker was `Ordering::Relaxed`, which does not prevent the
  relaxed field stores from becoming observable before the odd sequence on weak
  memory. This round it is followed by `core::sync::atomic::fence(Ordering::Release)`
  so no field store precedes the odd marker; the even commit is a Release store.
  Seeded the concurrent witness with a coherent epoch-0 tuple before starting the
  threads, and updated `migration_slot_has_no_unsafe_cell_sync` to require the
  Release fence + Release commit (not the bare relaxed marker).

**Blocker Handoff**

None

**Blocker Resolution**

None

**Self-Review**

- Plan compliance: passed — Tasks 4.1–4.5 all implemented against their contracts;
  non-goals (registry fork/patch, Deref escape, blanket SeqCst, broad locks, D1 SMP
  claims, forced migration, driver sleep-polling) not violated.
- Full diff reviewed: passed — reviewed every product diff for this cycle; no
  out-of-scope changes; accepted Iteration 000–002 staged work untouched.
- Critical findings unresolved: none
- Important findings unresolved: none
- Minor findings unresolved: "cross_hart_second_acquire_waits_for_first_release"
  (pre-existing Iteration 000 host harness) is intermittently flaky under high
  load and — because `GLOBAL_LOCK` is a non-RAII spinlock — a panic while holding
  it can wedge the whole subsequent harness. Pre-existing, out of Cycle 003 scope,
  reproducible in two of several runs, unrelated to the mask/migration/ordering
  changes. Recorded as an Issue candidate for `openspec-experience-recorder`.

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 覆盖范围 | 结论 |
|---|---|---|---|---|
| 4.1 边界（debug） | `AX_CONFIG_PATH=.axconfig.toml cargo test --features multitask,test mask_` | 9 passed; 0 failed | mask read/write/checked/legal/guard | PASS |
| 4.1 边界（release） | `... cargo test --release ... mask_` | 9 passed; 0 failed | OOB false/Err、legal compat release | PASS |
| 新增 Ord/Hash 兼容（debug） | `... cargo test --features multitask,test cpumask::tests` | 2 passed; 0 failed | 跨字节 ordering oracle + backing-store hash | PASS |
| 新增 Ord/Hash 兼容（release） | `... cargo test --release ... cpumask::tests` | 2 passed; 0 failed | 同上 release | PASS |
| axtask 串行套件 | `cargo test --features multitask,test,smp -- --test-threads=1` | 29 passed + 1 doctest | 选择/affinity/调度回归 | PASS |
| uart_16550 async | `cargo test --features async` | 69 unit + 8 doc + 10 compile-fail | driver wake seam/回归 | PASS |
| axnet ordinary | `RUSTFLAGS=-C linker=scripts/cc-nopie.sh cargo test --lib` | 482 passed; 0 failed | owner/runner、wrap、nudge、生命周期 | PASS |
| axnet qemu-diagnostics | `... --features qemu-diagnostics cargo test --lib` | 514 passed; 0 failed | 诊断面 | PASS |
| MS04/.. host harness | `rustc --test tests/ms04-async-rx-host-harness.rs` | 104 passed; 0 failed | +并发 coherence、single-cycle、Ord/Hash/seqlock guard | PASS |
| MS04 迁移子集（re-review 后） | `/tmp/ms04-harness migration` | 9 passed; 0 failed | 并发 tuple（seeded）+ 单次 pass + Release fence guard | PASS |
| make host-test | 用户手动运行 | 通过 | early-console/memtrack/ms03/04/07 + probes | PASS |
| 构建 ordinary | `make build` | Finished | kernel+qemu features | PASS |
| 构建 SMP=16 | `make build SMP=16`（`make defconfig SMP=16` 后 max-cpu-num=16） | Finished | qemu+smp 特征 | PASS |
| D1 编译 | `make lichee` | Finished（bin 更新） | lichee-d1 / non-qemu 面 | PASS |
| QEMU smoke（组合，单次，re-review 后） | `make justrun SMP=16`（`timeout 40` bounded） | NET-SMP:PASS NET-MIG:PASS UART-SMP:PASS UART-MIG:PASS（owner/runner/rx/tx phase=2 Restored） | 固定 placement + 双驱动迁移单次通过（去损 Release fence 版） | PASS |
| diff 检查 | `git diff --check`; `git diff --cached --check` | exit 0，无输出 | 空白/尾随 | PASS |
| 严格验证 | `openspec validate ms08-qemu-multi-hart-correctness-baseline --strict` | Change ... is valid; exit 0 | change 结构 | PASS |

\* `cross_hart_second_acquire_waits_for_first_release` 偶发 FAILED（见 Minor）。

**Persisted Evidence**

None required

**Experience Candidates**

- **Issue candidate**: 既有（Iteration 000）critical-section host harness
  `cross_hart_second_acquire_waits_for_first_release` 在高负载下偶发失败，
  且 `kernel/src/critical_section_policy.rs` 的 `GLOBAL_LOCK` 为 AtomicBool
  自旋锁、无 RAII Drop —— 测试线程持锁时 panic 会把整个后续 harness 卡死。
  与本 Cycle 改动无关（未触碰该文件或测试），但影响 `make host-test` 的
  稳定性。证据：本次 Cycle 两次运行 FAILED、其余多次 PASS；用户手动通过。
- Runbook candidate: None。

**Remaining Issues**

- All three Cycle-001 Plan-Review findings are repaired and re-verified above:
  (1) `MigrationSlot` is data-race-free and weak-memory coherent (atomic seqlock,
  no `UnsafeCell`, begin-marker ordered before field stores by a Release fence,
  even commit is Release) with a seeded concurrent torn-view-rejecting witness
  and updated structural guard; (2) `AxCpuMask` `Ord`/`Hash` delegate to the
  registry store with debug/release cross-byte + hash witness; (3) the 100-round
  stress is replaced by a serial single-pass per the user's multi-round waiver,
  with the runtime migration witnessed by the single QEMU smoke pass (re-run after
  the ordering change).
- Pre-existing Minor / Issue candidate: `cross_hart_second_acquire_waits_for_first_release`
  flake + `GLOBAL_LOCK` non-RAII spinlock (see Experience Candidates) — unchanged
  and outside this Cycle's scope.

**Commit or Diff Reference**

None（未提交；工作区保留全部本 Cycle + 已验收 Iteration 000–002 的 staged/未 staged 变更）

## Plan Review

- Review Result: rework-required

**Findings**

1. **Blocking / Task 4.4 — the added Release fence still does not close either
   side of the custom seqlock.** `MigrationSlot::store` now performs a relaxed
   odd RMW, `fence(Ordering::Release)`, then relaxed field stores. Release keeps
   operations before it from moving after it, but operations after a release
   may move before it; the field stores are therefore not required to remain
   after the odd marker. On the read side, the second Acquire sequence load
   orders later operations, not the preceding field loads, so those loads are
   not required to remain before the final check. The reader may still accept a
   mixed view on weak memory. The source guard proves that this incomplete fence
   exists; a passing host interleaving does not establish the missing ordering.

2. **Resolved / witness initialization.** The concurrent test now stores a
   coherent epoch-0 tuple before either thread starts, so its oracle no longer
   races against the `AUTO_HART` initial state.

3. **Previously resolved and unchanged.** Task 4.1's mask Ord/Hash compatibility
   remains closed. Task 4.5 remains satisfied under the user's explicit waiver
   for serial, single-pass tests.

The reported pre-existing `cross_hart_second_acquire_waits_for_first_release`
flake remains a non-blocking Issue candidate and is outside this Cycle.

**Deviation Classification**

ACT-DEVIATION — the remaining A4 defect is still an incomplete implementation
of the existing coherent-tuple contract. No requirement, ABI, or Iteration Map
change is needed.

**Acceptance Gaps**

- A4 remains incomplete: the current fence-based seqlock does not keep all field
  accesses between the two sequence boundaries on weak memory.
- A1 and A5: None.

**Convergence**

unchanged — compared with the previous Review, the same A4 weak-memory
coherence gap remains. The test-oracle initialization improved, but it does not
change the product ordering guarantee.

**Evidence**

- Independent source review:
  `kernel/src/drivers/uart_migration_logic.rs:247-267,283-302`; neither the
  Release fence after the odd marker nor the final Acquire load supplies the
  missing opposite-direction boundary. The official Rust Nomicon
  ["Atomics"](https://doc.rust-lang.org/nomicon/atomics.html) states that
  operations after a release may move before it and operations before an
  acquire may move after it.
- Independent test review:
  `tests/ms04-async-rx-host-harness.rs:3341-3393` now seeds a coherent tuple;
  `:3460-3479` asserts the incomplete Release-fence implementation rather than
  a complete synchronization boundary.
- Adopted from the latest Act Response where source remains consistent: the
  focused migration subset, ordinary/SMP=16 builds, and bounded SMP=16 QEMU
  smoke passed once. They establish functional regression coverage but cannot
  override the missing source-level ordering.
- Fresh structural checks: `git diff --check` exit 0;
  `git diff --cached --check` exit 0;
  `openspec validate ms08-qemu-multi-hart-correctness-baseline --strict` reports
  `Change ... is valid` with exit 0.

**Follow-up Decision**

Create `002-rework.md`. The same Acceptance gap did not shrink in the latest
current-Cycle repair, and continuing to tune a custom seqlock needs a new repair
contract. Cycle 002 replaces it with a dedicated short-lived Acquire/Release
view lock, distinct from the long-lived migration single-flight flag. The new
Cycle remains `draft` until the user approves it; Iteration 004 stays blocked.

**Iteration Plan Update**

None

**Next Cycle**

`002-rework.md`

**Next Iteration**

None
