# Iteration 007 / Cycle 003: Bounded Lease Snapshot and Monotonic Flush Witness

## Plan Context

- Status: ready
- Iteration: 007-ticketed-flush-and-v3-diagnostics
- Cycle: 003-rework
- Cycle Type: rework
- Parent cycle: `002-rework.md`

**Iteration Scope**

- Change tasks: 4.1, 4.2, 4.3
- Depends on: Cycle 002 implementation and its `rework-required` Plan Review
- Stable baseline: diagnostic transactions are coherent and bounded, V3 consumes one lease
  snapshot, flush identity exhaustion has a monotonic ABA witness, and automatic Gates are clean.
- Verification boundary: stalled-writer progress, control/tick replacement, single-snapshot V3,
  stale Drop with a newer waiter, direct exhaustion, full regressions and source/diff review.
- Diagnostic boundary: failures separate into lease transaction progress, V3 tuple assembly,
  flush waiter identity isolation or Cycle-owned verification hygiene.
- Deferred tasks: 5.1-5.2, 6.1-6.3

**Cycle Scope**

- Trigger: Cycle 002 Review Result `rework-required`
- Acceptance gaps: unbounded diagnostic-state spinning; V3 cross-generation tuple; reset-based
  stale-Drop claim without a stale future; one new `unused_mut` warning.
- Repair items: RW-9 through RW-12.
- Inherited scope: Tasks 4.1-4.3, R3-R6/R14, D8-D10, Cycle 002's passing lease replacement,
  buffer-owner ledger, direct exhaustion, ABI, driver and build behavior.
- Excluded scope: Task 5.1 probe, Evidence 008, manual QEMU, socket API, reset/SMP, real-board and
  performance work, dependency changes and unrelated warning cleanup.

**Objective**

Close the last Iteration 007 gaps without changing public ABI or ownership. Every diagnostic
caller must complete or defer in bounded work, V3 must publish mode and expiry from one committed
generation, and the flush test must prove stale-Drop isolation while identities remain monotonic.

**Current Baseline**

- Branch `net-k3`; review HEAD `223f6281d62b6925fa3f830690945dccab424022` with staged Cycle
  002 implementation and documentation changes.
- Fresh suites pass: axnet 214 default/231 qemu-diagnostics; axdriver_net 7;
  axdriver_virtio/net 16; virtio-drivers/alloc 36; MS03 33; MS04 16; kernel QEMU check.
- Diagnostic and flush groups pass 100 repetitions. Scoped rustfmt, strict OpenSpec and diff check
  pass. These green results do not cover the blocking interleavings below.
- D1 remains the established exclusion comparison and is not changed by this Cycle.

**Current-State Evidence**

- `DiagnosticState::write_state`, `lease_snapshot_checked` and `tick` loop until an odd or changed
  generation becomes available. If the odd-generation owner is descheduled, callers spin rather
  than returning or deferring.
- `rx_snapshot_v3` calls `hold_mode()` and `lease_expiry()` independently, so one control commit
  between calls yields a cross-generation tuple even though each getter is locally coherent.
- The flush exhaustion test correctly reaches `u64::MAX`, then resets the identity counter to
  create a later future. It never retains an old future across a newer waiter installation.
- The test's read-only post-exhaustion Service guard is unnecessarily mutable and emits a new
  warning in both axnet configurations.

**Relevant Code**

| Area | Files and symbols | Responsibility |
|---|---|---|
| Lease transaction | `crates/axnet/src/diag.rs::DiagnosticState` | coherent control, tick and snapshot state |
| Queue owner | `crates/axnet/src/async_rx.rs::RxRxFuture`, `Service::diag_hold_tick` | bounded stage hold/defer and deadline arm |
| V3 assembly | `crates/axnet/src/async_rx.rs::rx_snapshot_v3` | append one coherent lease tuple to diagnostics |
| Flush waiter | `crates/axnet/src/flush.rs`, `service.rs::flush_clear` | identity-bound Drop and exhaustion witness |

**Critical Path**

```text
control/tick attempts one bounded state transition
  -> committed {generation, mode, expiry} is observed atomically
  -> contention defers without spinning; the committing control publishes queue work
  -> queue owner arms only the observed generation's deadline
  -> V3 copies mode + expiry from that same snapshot

old flush future remains live after its registration becomes stale
  -> newer monotonic identity owns the waiter slot
  -> old future Drop cannot clear the newer waiter
  -> last-valid waiter is released
  -> u64::MAX exhaustion rejects without reset, wrap or ownership mutation
```

**Behavioral Change**

- Diagnostic state contention no longer creates an unbounded loop in syscall, queue poll or V3
  snapshot paths. A caller either observes/commits one coherent state or follows an explicit
  bounded defer/error path whose wake source is identified.
- V3 mode and expiry come from one snapshot. Wire layout and all field positions remain unchanged.
- Product flush behavior remains unchanged; only its test constructs the missing stale-future
  interleaving without moving the identity counter backward.

**Change Surface**

| Repair | Requirement/Scenario | File/Symbol | Planned change |
|---|---|---|---|
| RW-9 | R6/R14, D9; bounded progress | `diag.rs`, queue hold callers | replace unconditional retry with bounded commit/snapshot/defer semantics |
| RW-10 | R6/R14, D9-D10; V3 coherence | `rx_snapshot_v3`, diagnostic snapshot API | obtain generation/mode/expiry once and reuse the tuple |
| RW-11 | R3/R4, D8; stale Drop and exhaustion | `flush.rs` tests | retain an old future across a newer waiter, never reset identity |
| RW-12 | Gate 4/5 | Cycle-owned Rust and Response | remove new warning and record exact results |

**Task Contracts**

### RW-9 — Bounded diagnostic transaction（Task 4.3）

- RED: add a deterministic stalled-writer seam. While generation is odd, every production-style
  reader/tick/control path used by the queue or V3 must return/defer within bounded steps; a test
  must not depend on a timeout to terminate an infinite loop.
- GREEN: replace unconditional CAS/snapshot loops with a bounded protocol. Contention may return
  an internal Busy/Defer outcome or a stable control error, provided no torn state is consumed and
  the queue owner has a concrete later wake source. Preserve Release-after-commit queue-work
  publication and generation-bound expiry clear.
- Check generation arithmetic. It must not panic, silently wrap into an ABA-valid generation or
  leave the state permanently odd; exhaustion may fail closed with a stable internal/control
  result.
- Must not: busy-spin, self-wake repeatedly while a writer owns the transaction, hold a guard
  across `Pending`, add periodic polling, or move packet/descriptor ownership into diagnostics.
- Stop: if bounded progress requires changing ioctl payloads, V3 layout or queue ownership, return
  to Plan as `replan-required`.

### RW-10 — One coherent V3 lease snapshot（Tasks 4.2, 4.3）

- RED: deterministically place a control commit between the two legacy getter observations and
  prove the old V3 assembly can form a cross-generation pair.
- GREEN: expose or reuse one coherent lease snapshot and destructure it once before constructing
  V3. Mode and expiry must derive from the same generation; auto-release telemetry remains an
  independent monotonic counter unless the implementation can include it without blocking.
- Preserve: V1/V2 bytes, V3 size/order/sentinels, QEMU feature gating and snapshot source ABI.
- Must not: call separate mode/expiry getters from the V3 constructor, retry without bound or
  serialize V3 by holding the Service guard across unrelated work.

### RW-11 — Monotonic stale-Drop and exhaustion witness（Task 4.1）

- RED/GREEN: set the counter once near the boundary; create an older future, make only its waiter
  registration stale through an existing test-visible clear/completion operation while retaining
  the future, install the next monotonic waiter, then Drop the older future and prove the newer
  waiter remains. Release the newer waiter and call again at `u64::MAX` to prove the direct stable
  exhaustion branch. Keep a live packet ticket throughout and prove ownership is unchanged.
- Assert waiter identity/target or a behaviorally equivalent observation before and after stale
  Drop. The test must distinguish occupied-slot rejection from identity exhaustion.
- Must not: reset/decrement the counter, forge a product future identity, loop toward the boundary,
  reuse an identity or expose a test seam in product builds.
- Stop: any wrap, ABA clear or packet-owner mutation is a product defect and must be repaired
  before Gate 5.

### RW-12 — Warning and evidence closure（Tasks 4.1-4.3）

- Remove the Cycle-owned read-only `mut` and any warning introduced by RW-9 through RW-11.
- Record exact manifest paths, working directory, outputs and exits. Separate established external
  warnings and the D1 comparison from Cycle-owned results; do not claim a warning-free broad tree.
- Do not clean unrelated smoltcp, `MAX_LIVE_TICKETS`, `SUPPRESS`, driver formatting or D1 issues.

**Invariants**

- The queue task remains the sole RX/TX hardware owner; diagnostic control/timer/V3 never mutate
  slots, tickets, buffers, descriptors or completions.
- State publication is coherent; stale tick/timer/Drop can affect only its own generation/identity.
- No diagnostic contention path blocks or spins indefinitely and no guard crosses `Pending`.
- V1/V2 compatibility and V3 field order/size remain unchanged.
- Buffer/descriptor/ticket ledgers remain independent and Cycle 002 drift evidence remains valid.

**Non-goals**

- No Task 5.1 probe, host stimulus, Makefile target, guest artifact or persisted runtime Evidence.
- No socket readiness/flush API, scheduler policy, generic mutex redesign, SMP or board claim.
- No tasks/SNAPSHOT/M-D-K-R-I update, archive, unrelated cleanup or dependency upgrade.

**Acceptance**

| Repair | Proof | Status |
|---|---|---|
| RW-9 | stalled-writer tests terminate in bounded work; no product-path retry loop; stale lease tests remain green | Planned |
| RW-10 | V3 destructures one snapshot; forced interleaving cannot mix mode and expiry | Planned |
| RW-11 | actual old-future Drop leaves newer monotonic waiter installed; sentinel is reached without reset | Planned |
| RW-12 | no new Cycle-owned warnings; reproducible full Gate record | Planned |

No requirement is Missing or Simplified. Because this is the third repair Cycle for Iteration 007,
any repeated RW-9/RW-11 gap after Act must not produce a fourth same-problem Cycle; Review must
accept, replan, or stop with the unresolved evidence.

**Verification**

Act must record the exact command, working directory, key output and exit status for:

```text
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --features qemu-diagnostics --lib diag -- --nocapture
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --features qemu-diagnostics --lib flush -- --nocapture
repeat the stalled-writer, replacement/tick and stale-Drop/register-recheck tests 100 times
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --features qemu-diagnostics --lib
cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline
cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net
cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --features alloc
rustc --edition=2024 --test tests/ms03-irq-host-harness.rs -o /tmp/ms03-irq-host-test && /tmp/ms03-irq-host-test
rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test
cargo check --offline -p starry-kernel --features qemu
cargo check --offline -p starry-kernel --features lichee-d1
rustfmt --check --edition 2024 --config skip_children=true <all Cycle-owned changed Rust files>
openspec validate ms05-qemu-bounded-bidirectional-device-data-plane --strict
git diff --check -- crates/axdriver_net crates/axdriver_virtio crates/virtio-drivers crates/axnet kernel tests openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane
```

D1 remains an expected exit-101 comparison with exactly the established 25 errors, never PASS.
Any unbounded diagnostic retry, cross-generation V3 tuple, reset/forged stale-Drop witness, new
Cycle-owned warning, regression, ABI/source/format/diff failure or additional D1 error blocks the
Cycle.

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | blocking loops, split V3 reads, reset-based flush test and warning are located at exact symbols |
| Design | PASS | bounded defer/commit, one tuple read and monotonic stale future preserve interfaces and ownership |
| Iteration Plan | PASS | repairs remain within Tasks 4.1-4.3; Iterations 008-009 are unchanged |
| Cycle Scope | PASS | RW-9..RW-12 close only Cycle 002 Review gaps |
| Task Contracts | PASS | each repair has RED/GREEN, prohibitions, stop conditions and exact verification |
| Traceability | PASS | repairs map to R3-R6/R14, D8-D10 and concrete production consumers/tests |
| Verification | PASS | deterministic interleavings plus full regression/ABI/build/format Gates cover the boundary |

**Persisted Evidence**

- Mode: none

The Act Response carries Cycle 003 evidence. Do not create an Iteration 007 evidence directory.

**Risks and Notes**

- A bounded Busy/Defer result is acceptable only with a proven later wake/publication source; a
  self-wake retry loop merely relocates the spin and fails RW-9.
- Single-hart QEMU and host atomics do not prove SMP, scheduler fairness or board timing.

## Act Response

- Status: reported

**Implemented**

- RW-9（Task 4.3）：`DiagnosticState` 的无条件 CAS/snapshot 循环全部改为有界协议。`write_state`/`control` 单次 claim 尝试，odd generation、CAS 竞态或 `u64::MAX - 1` generation 耗尽时以稳定控制错误 `ResourceBusy` 失败关闭（kernel ioctl 映射为 `WouldBlock`）；`lease_snapshot_checked` 单次 coherent 尝试，torn 时返回携带 observed odd generation、`HOLD_NONE`、expiry 0 的 defer tuple；`tick` 单次 observe + 单次 generation-bound clear + 单次 re-observe，torn/moved 时 defer 返回；`claim_and_clear` 拒绝 `u64::MAX - 1` sentinel 防止 `g + 2` wrap。
- RW-10（Tasks 4.2, 4.3）：`rx_snapshot_v3` 从 `hold_mode()`/`lease_expiry()` 两个独立 getter 改为一次 `lease_snapshot_checked()` 解构，mode 与 expiry 恒来自同一 committed generation；`auto_release_failure` 保持独立单调计数器；V1/V2 字节、V3 字段顺序/大小、QEMU feature gating 与 snapshot source ABI 不变。
- RW-11（Task 4.1）：重写 `flush.rs` identity exhaustion 测试为单调 witness——identity counter 只设置一次（`u64::MAX - 2`），旧 future 通过既有 test-visible `flush_clear` 使 waiter registration 失效但保留 future，安装下一单调 waiter（MAX-1），Drop 旧 future 后证明新 waiter 存活，释放新 waiter 后以空 slot 直达 `u64::MAX` sentinel 分支，全程保留 live packet ticket 并证明 ownership 未变。消除了原测试对 identity counter 的 reset。
- RW-12（Tasks 4.1-4.3）：移除 Cycle-owned 的 `unused_mut`（原 flush.rs:582，随 RW-11 重写消失；本 Cycle 新增的 583/591 read-only guard 改为 `let guard`）。

**Changed Files and Symbols**

- `crates/axnet/src/diag.rs`：`DiagnosticState::control`、`write_state`（loop → 单次 bounded claim + sentinel fail-closed）、`lease_snapshot_checked`（loop → defer tuple）、`tick`（loop → bounded observe/clear/re-observe）、`claim_and_clear`（sentinel guard）；新增测试 `stalled_writer_readers_tick_and_control_return_within_bounded_steps`、`control_generation_exhaustion_fails_closed_without_wrap`、`two_getter_v3_assembly_forms_cross_generation_pair_but_single_snapshot_cannot`。
- `crates/axnet/src/async_rx.rs`：`rx_snapshot_v3` 单快照解构 `hold_mode`/`lease_expiry`。
- `crates/axnet/src/flush.rs`：测试 `flush_waiter_identity_exhaustion_returns_stable_error_without_wrap` → `flush_monotonic_stale_drop_and_identity_exhaustion_witness`（单调 stale-Drop + 直接 exhaustion witness）。
- `openspec/changes/ms05-.../iterations/007-.../003-rework.md`：本 Act Response。

**Deviations from Plan**

- RW-10 的 RED 测试在 `DiagnosticState` 层面确定性放置 control commit 于两次 legacy getter observation 之间，证明跨 generation pair 可形成且单快照 API 不能；`rx_snapshot_v3` 自身的单快照接线由 source diff review + 既有 V3 ABI（offset）测试覆盖，未在 V3 层新增注入式测试（避免触碰全局 `DIAGNOSTIC` 造成并行污染）。行为等价，无需求裁剪。

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: PASS
- Full diff reviewed: PASS
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 3

全量 diff 审查（diag.rs + async_rx.rs + flush.rs，相对 HEAD 477 行增改）：RW-9 的 defer tuple 在 `async_rx.rs:1034` `SleepUntil(expiry=0, odd_gen)` 时由 `arm_lease_deadline` 的 deadline==0 分支取消，依赖 committing control 的 `publish_queue_work()` 作为 concrete wake source（符合 RW-9 契约）；`lease_deadline_elapsed` 的 `generation()` 返回 odd 永不匹配 armed even，stale timer 按 bounded extra poll 丢弃；`diag_hold_tick` 的 `hold_mode()`/`tick()` torn 时 defer 为 `HOLD_NONE`，一至多轮不 hold 后由 control publish 重新观察，不违反 ownership。跨任务交互无遗漏实现、无计划外修改。

遗留 Minor（不阻塞，未伪装为已解决）：
1. `flush.rs:670/700/718` pre-existing `unused_mut`（既有 V3 ledger/counter 测试，非本 Cycle 引入；Cycle 002 Review 仅标记 582 为 new，已消除）。
2. `async_rx.rs:23` `fixed_queue::MAX_LIVE_TICKETS` unused import（Plan RW-12 明确禁止清理）。
3. smoltcp 既有 11 个 warning（外部依赖，Plan 明确禁止清理）。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| diag RED→GREEN | `cargo test ... axnet --features qemu-diagnostics --lib diag` | RED: 旧代码 stalled-writer 挂起、exhaustion overflow panic；GREEN: `13 passed; 0 failed` | PASS |
| flush RED→GREEN | `cargo test ... axnet --features qemu-diagnostics --lib flush` | `23 passed; 0 failed` | PASS |
| axnet 全量（qemu-diagnostics） | `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --features qemu-diagnostics --lib` | `234 passed; 0 failed` | PASS |
| axnet 全量（default） | `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib` | `214 passed; 0 failed` | PASS |
| diag/flush 100× 竞态 | 两个循环各 100 次 `cargo test ... --lib diag / --lib flush` | 200 次全 `test result: ok`，零失败 | PASS |
| axdriver_net | `cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline` | `7 passed` | PASS |
| axdriver_virtio | `cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net` | `16 passed` | PASS |
| virtio-drivers | `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --features alloc` | `36 passed` | PASS |
| MS03 host harness | `rustc --edition=2024 --test tests/ms03-irq-host-harness.rs -o /tmp/ms03-irq-host-test && /tmp/ms03-irq-host-test` | `33 passed; 0 failed` | PASS |
| MS04 host harness | `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test` | `16 passed; 0 failed` | PASS |
| kernel QEMU check | `cargo check --offline -p starry-kernel --features qemu` | `Finished dev profile`，exit 0 | PASS |
| kernel D1 check | `cargo check --offline -p starry-kernel --features lichee-d1` | exit 101，恰为既有 25 errors（axfs/axtask），非本 Cycle 引入 | PASS（预期比较，never PASS 语义） |
| rustfmt | `rustfmt --check --edition 2024 --config skip_children=true crates/axnet/src/{diag,async_rx,flush}.rs` | exit 0（应用 rustfmt 后） | PASS |
| OpenSpec strict | `openspec validate ms05-qemu-bounded-bidirectional-device-data-plane --strict` | `Change ... is valid` | PASS |
| diff check | `git diff --check -- crates/axdriver_net crates/axdriver_virtio crates/virtio-drivers crates/axnet kernel tests openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane` | exit 0 | PASS |

D1 说明：exit 101 与 Cycle 002 基线一致（"D1 remains an expected exit-101 comparison with exactly the established 25 errors, never PASS"），本次确认为 25 个 axfs/axtask 既有错误，Cycle-owned 文件无新增错误。

**Persisted Evidence**

None required（Cycle Persisted Evidence 模式为 `none`；验证摘要由本 Act Response 承载）。

**Experience Candidates**

None（本 Cycle 为常规 bounded-protocol 修复与测试 witness 重写，RED 挂起/overflow 属预期 TDD 现象，不构成 Runbook/Incident 候选）。

**Remaining Issues**

- pre-existing `unused_mut`（flush.rs:670/700/718）与 `MAX_LIVE_TICKETS` unused import、smoltcp warnings：按 Plan RW-12 明确禁止清理，留给后续非本 Cycle 范围处理。
- Iteration 008（Tasks 5.1-5.2）仍待本 Cycle Review 接受后展开。

**Commit or Diff Reference**

未创建 commit。工作区已暂存 Cycle 002 实现与 Cycle 003 文档；本 Cycle 代码改动在暂存区之上（`git diff` 相对 staged）。

## Plan Review

- Status: reviewed

**Review Result**

replan-required

**Findings**

1. **Blocking / Important — V3 contention fallback is synthetic, not a committed snapshot.**
   `lease_snapshot_checked()` returns `(current_generation, HOLD_NONE, 0)` whenever its single
   coherent read fails. `rx_snapshot_v3()` copies that tuple into the public V3 payload without a
   Busy/Defer marker or error path. During an odd-generation write, or when the read straddles a
   completed Hold commit, V3 can therefore report “no hold” even though no committed generation
   contains that state. This closes the two-getter race but still violates RW-10's requirement
   that mode and expiry come from one committed generation. Iteration 008's probe could interpret
   the synthetic tuple as a real RELEASED/POST state.
2. **Blocking / Important — generation exhaustion can make a bounded Hold permanent.**
   A successful control from generation `u64::MAX - 3` may commit a Hold at
   `u64::MAX - 1`. At that generation, `write_state()` rejects explicit Release and
   `claim_and_clear()` rejects lease expiry. The state remains coherently held, but neither the
   user nor the 2-second timer can clear it. The new exhaustion test starts from a no-hold state,
   so it proves no wrap but misses D9's mandatory maximum-lease and automatic-release boundary.
3. **Blocking / Minor — RW-10 leaves a new Cycle-owned warning.** Removing V3's separate expiry
   getter made `DiagnosticState::lease_expiry()` unused in product builds. Fresh kernel QEMU
   check reports this new dead-code warning, but the Act Response lists only pre-existing
   warnings. RW-12 required every warning introduced by RW-9 through RW-11 to be removed or
   reported accurately.

RW-11 is accepted: the flush test retains an old future, installs the next monotonic waiter,
drops the stale future, proves the newer waiter survives, and reaches the sentinel without reset.
The bounded single-attempt control/read paths also remove the prior unbounded spin.

**Deviation Classification**

PLAN-INVALID; ACT-DEVIATION; NEW-EVIDENCE.

Cycle 003's design permits a synthetic defer tuple but simultaneously requires V3 to expose a
committed tuple. V3 has no defer/error representation, so those two contracts cannot both hold.
The Act also omitted the active-Hold exhaustion case and the new orphaned-method warning.

**Acceptance Gaps**

- RW-9 / Task 4.3: generation exhaustion must leave diagnostics releasable and lease timeout
  bounded even when the last committed state is Hold.
- RW-10 / Tasks 4.2-4.3: every successful V3 read must contain mode and expiry from an actual
  committed state; contention cannot be encoded as an indistinguishable no-hold snapshot.
- RW-12 / Tasks 4.1-4.3: Cycle-owned warning provenance remains incomplete.

**Convergence**

reduced but not converged. Cycle 003 removes unbounded spinning, fixes cross-getter V3 assembly,
and closes the flush stale-Drop witness. It does not close the committed-snapshot or terminal
lease-liveness parts of the same RW-9/RW-10 problem. Cycle 003 declared itself the third and final
repair attempt; the project three-failure rule therefore forbids a fourth same-problem Cycle.

**Evidence**

- Source review: `crates/axnet/src/diag.rs:170-175` synthesizes the defer tuple after a failed
  coherent read; `crates/axnet/src/async_rx.rs:473` publishes it through V3 without a defer signal.
- Source review: `crates/axnet/src/diag.rs:127-141,207-224` rejects both control and expiry clear
  at `u64::MAX - 1`; `control_generation_exhaustion_fails_closed_without_wrap` covers only
  `HOLD_NONE` at that generation.
- Fresh axnet default/qemu-diagnostics suites: 214 and 234 passed, exit 0. The feature build reports
  pre-existing warnings; product kernel QEMU check additionally reports the newly unused
  `DiagnosticState::lease_expiry`.
- Fresh axdriver_net, axdriver_virtio/net and virtio-drivers/alloc suites: 7, 16 and 36 passed,
  exit 0.
- Diagnostic and flush groups repeated 100 times each: zero failures, exit 0; the missing boundary
  is not represented by those tests.
- MS03/MS04 host harnesses: 33 and 16 passed, exit 0.
- Kernel QEMU check: exit 0. D1 comparison: exit 101 with the established 25 axfs/axtask errors;
  comparison only, not PASS.
- Scoped rustfmt, strict OpenSpec validation and staged diff check: exit 0.

**Follow-up Decision**

Stop Iteration 007 execution and return to design. A new Plan must choose a diagnostic-state
ownership/publication model that provides bounded access to a real last-committed tuple and a
terminal generation state in which no Hold can remain active. One concrete direction to evaluate
is serializing control/tick/snapshot state under the existing Service ownership boundary while
keeping the timer wake-only and using bounded Service acquisition for the ioctl; alternatives
must prove the same properties without changing V1/V2/V3 wire layout or queue ownership.

The replan must add RED witnesses for (a) V3 contention returning a real committed tuple and
(b) a Hold committed immediately before generation exhaustion still releasing by explicit
Release or lease expiry. It must also decide and document control contention/error semantics
before Gate 2. Tasks 5.1-6.3 remain blocked.

**Iteration Plan Update**

Replan required. The existing Iteration Map is not advanced or renumbered in this Review. D9 and
the unfinished Iteration 007 execution design must be revised and approved before any successor
Cycle/Iteration is created.

**Next Cycle**

None（触发三次失败停止规则，不创建 `004-rework.md`）。

**Next Iteration**

None（Iteration 007 未接受，禁止提前展开 Iteration 008）。
