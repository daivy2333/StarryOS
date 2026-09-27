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

- Status: reported

**Implemented**

Repair 4.4-R1: replaced the incomplete custom seqlock in `MigrationSlot` with a
dedicated short-lived Acquire/Release view lock (`ViewLock`/`ViewGuard`) that
serializes only the six-field `MigrationAtomicView` copy in `load()` and
`store()`. The `seq` counter and both fence-based publication steps are removed.
Release is RAII-safe via `Drop`; `in_progress` remains the independent
migration single-flight flag and is untouched by view access.

Sequence: (1) rewrote the structural witness
`migration_slot_has_no_unsafe_cell_sync` to reject any remaining
`seq`/fence/`fetch_add` publication and require a distinct view lock with
Acquire CAS acquisition and RAII Release, plus a body-level check that neither
`load()` nor `store()` touches `in_progress`; observed RED against the then
current seqlock source (`must not keep the incomplete seqlock (self.seq)`).
(2) Implemented the view lock in `uart_migration_logic.rs`; observed GREEN.

**Changed Files and Symbols**

- `kernel/src/drivers/uart_migration_logic.rs`: new private `ViewLock` (Acquire
  `compare_exchange` acquire loop) and `ViewGuard` (`Drop` → Release store);
  `MigrationSlot` field `seq: AtomicUsize` replaced by `view_lock: ViewLock`;
  `load()`/`store()` now route their six-field copy through the guard with
  Relaxed field accesses; `MigrationSlot` doc updated. Public API, wire
  packing, reject counters, single-flight semantics unchanged.
- `tests/ms04-async-rx-host-harness.rs`: structural witness
  `migration_slot_has_no_unsafe_cell_sync` updated per the contract; seeded
  concurrent tuple witness `migration_slot_concurrent_load_is_always_coherent`
  retained unchanged.

**Deviations from Plan**

None. One observation outside the repair scope: run 2 of the QEMU combined
smoke failed `UART-SMP-SMOKE` check `ipi_received_causal` (ipi_sent=10 vs
ipi_received=9 at snapshot — an IPI receipt-accounting lag in the smoke's
timing window, counters live in `axtask` IPI accounting, unrelated to
`MigrationSlot`). The user then ran the same bounded smoke manually with all
four markers PASS (see Verification Evidence). Recorded as a Minor finding
below; no source change made.

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: PASS — only repair item 4.4-R1 implemented; all Preserve
  items held (public methods, no_std, wire bytes, reject counters,
  single-flight); all Forbidden items respected (no `in_progress` reuse —
  witness-enforced; no lock held across wake/yield/MMIO/affinity work — the
  critical section contains only six field copies; no `UnsafeCell`, no broad
  driver lock, no blanket SeqCst, no sleep-polling, no contract changes).
- Full diff reviewed: PASS — diff confined to the two contract files; no
  plan-outside modifications; no new warnings (host compile reports the same
  16 pre-existing warnings before and after).
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 1 — pre-existing `ipi_received_causal` timing
  window in the UART-SMP smoke (IPI receipt counter can lag delivery at
  snapshot time); observed once in run 2, PASS in run 1 and in the user's
  manual run. Outside this Cycle's repair surface; candidate for a later
  smoke-harness wait condition, not fixed here per excluded scope.

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 覆盖范围 | 结论 |
|---|---|---|---|---|
| 结构见证 RED | `rustc --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-harness && /tmp/ms04-harness migration`（修复前） | `panicked ... must not keep the incomplete seqlock (self.seq)`；8 passed / 1 failed | 见证拒绝旧 seqlock | PASS (RED) |
| migration host 子集 GREEN | `/tmp/ms04-harness migration`（修复后） | `test result: ok. 9 passed; 0 failed; 95 filtered out` | 并发 seeded tuple、view lock 结构 guard、迁移状态机/选择/v5 packing | PASS |
| 构建 ordinary | `make build` | `Finished release profile`（`.axconfig.toml` max-cpu-num=1） | kernel+qemu features，含 uart_migration_logic | PASS |
| 构建 SMP=16 | `make defconfig SMP=16 && make build SMP=16` | `Finished release profile`（max-cpu-num=16） | qemu+smp 特征 | PASS |
| QEMU 组合 smoke（有界） | `timeout 120 make justrun SMP=16` | `[NET-SMP-SMOKE] PASS`、`[NET-MIG-SMOKE] PASS`（owner/runner phase=2 Restored）、`[UART-SMP-SMOKE] PASS`（含 ipi_received_causal）、`[UART-MIG-SMOKE] PASS`（rx/tx phase=2 Restored）；用户手动运行同命令全 PASS | 固定 placement + 双驱动迁移，SMP=16 | PASS |
| diff 检查 | `git diff --check && git diff --cached --check` | 无输出，exit 0 | 补丁格式 | PASS |
| OpenSpec 严格验证 | `openspec validate ms08-qemu-multi-hart-correctness-baseline --strict` | `Change 'ms08-qemu-multi-hart-correctness-baseline' is valid`，exit 0 | change 结构 | PASS |

注：cpumask/mask 测试未重跑——`crates/axtask/src/cpumask.rs` 本次未改动，
Cycle 001 的 mask 结论继续有效（覆盖范围未变化）。

**Persisted Evidence**

None required（mode `none`；各项输出摘要已在本 Response 内）。

**Experience Candidates**

None。`ipi_received_causal` 时序窗口仅一次观察到且用户手动运行已通过，
证据不足以登记 Issue；若后续复现，可由 Recorder 评估。

**Remaining Issues**

- Minor（不阻塞）：UART-SMP smoke 的 `ipi_received_causal` 在快照时刻可能
  观察到 IPI 接收计数滞后（见 Deviations）。建议后续 Cycle 在
  drain-convergence 等待后对接收计数增加短等待或最终重查；不在本 Cycle
  范围内修复。

**Commit or Diff Reference**

Worktree diff（未提交）：
- `kernel/src/drivers/uart_migration_logic.rs`（+59/-44 行区域）
- `tests/ms04-async-rx-host-harness.rs`（+45/-12 行区域）

Pre-existing work committed before this Cycle's execution: `9efb2529`.

## Plan Review

- Review Result: accepted

**Findings**

- Blocking findings: None. Independent diff review confirms that `ViewLock` uses an
  Acquire compare-exchange and `ViewGuard::drop` releases with a Release store.
  `MigrationSlot::{load,store}` hold the separate guard only while copying the
  six atomic fields. The old sequence/fence protocol is absent; `in_progress`
  remains separate and neither view method calls external code while locked.
- Minor: the concurrent witness's introductory comment still calls the slot a
  seqlock, although its assertions and the product implementation use the new
  lock. This documentation mismatch does not affect A4.
- Minor: Act reported one `ipi_received_causal` snapshot lag in an initial QEMU
  smoke, followed by a bounded manual run with all four markers PASS. This is
  outside repair 4.4-R1 and does not contradict its tuple-coherence evidence.

**Deviation Classification**

None. The two Minor observations do not change the repair contract.

**Acceptance Gaps**

None. A4/4.4-R1 is closed by the lock's source ordering and the seeded
concurrent tuple witness; A1–A3 and the user-waived single-pass A5 remain
covered by Cycle 001's unchanged surfaces and the current Act verification.

**Convergence**

reduced — the parent Cycle's weak-memory tuple-coherence gap is closed.

**Evidence**

- Independently inspected `kernel/src/drivers/uart_migration_logic.rs` and its
  worktree diff: all six loads and stores are under `view_lock.acquire()`;
  RAII Release unlock and separate `in_progress` are explicit. Inspected the
  seeded tuple test and structural guard in
  `tests/ms04-async-rx-host-harness.rs`, plus UART/network call sites.
- Adopted the uninvalidated Act Response results: focused migration host subset
  9 passed, ordinary and SMP=16 builds passed, bounded SMP=16 combined smoke
  had four PASS markers in the user's final manual run, and strict OpenSpec
  validation exited 0. The worktree contains the same two product/test files
  identified by Act, with no additional product modification since that
  report. The QEMU result supports this QEMU SMP scope, not physical hardware.
- Review commands: `git diff --check` and `git diff --cached --check` both
  exited 0 with no output. Persisted Evidence mode is `none`; no directory is
  required.

**Follow-up Decision**

Accept Iteration 003. The short lock satisfies the existing A4 contract and
no blocking finding remains. Expand Iteration 004 as a draft protocol plan;
its execution still requires separate plan approval and Act instruction.

**Iteration Plan Update**

None.

**Next Cycle**

None.

**Next Iteration**

`../004-ms08-uart-and-network-qualification-protocols/000-initial.md`.
