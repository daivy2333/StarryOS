# Iteration 008 / Cycle 001: Production-Path Diagnostic Witnesses

## Plan Context

- Status: ready
- Iteration: 008-service-owned-diagnostic-lease
- Cycle: 001-rework
- Cycle Type: rework
- Parent cycle: `000-initial.md`

**Iteration Scope**

- Change task: 4.4
- Depends on: Cycle 000 implementation and its `rework-required` Plan Review
- Stable result: Service-owned diagnostic control and V3 snapshot semantics are exercised through
  the same private seams used by production entry points; contention and interleaving tests fail if
  blocking acquisition, synthetic tuples or incorrect event publication return.
- Verification boundary: focused production-path tests and 100× repeats pass, followed by all
  Iteration 008 compatibility, driver, harness, kernel, format and OpenSpec gates.
- Diagnostic boundary: testability seams for bounded control and committed V3 assembly only.
- Deferred tasks: 5.1-5.2 and 6.1-6.3

**Cycle Scope**

- Trigger: Cycle 000 `rework-required`
- Acceptance gaps: C2 has no held-Service contention/event witness; C5 has no V3 assembly
  interleaving witness.
- Repair items: T4.4-R1, T4.4-R2
- Inherited scope: R15, D9/D10, Task 4.4 C1-C6, Service-owned lease, bounded control, committed V3,
  wake-only timer, V1/V2/V3 compatibility and QEMU-only feature boundary.
- Excluded scope: Task 5.1 probe/stimulus, runtime QEMU, Evidence creation, public API changes,
  queue/event redesign, warning cleanup, SMP, DWMAC and true-board claims.

**Objective**

Close C2 and C5 with deterministic tests that execute private seams shared by the production
`diagnostic_control` and `rx_snapshot_v3` entry points. Direct calls to Service getters or a
test-only reimplementation of the production algorithm do not satisfy this Cycle.

**Current Baseline**

- Branch `net-k3`; HEAD `223f6281d62b6925fa3f830690945dccab424022`; Cycle 000 code and
  OpenSpec documents are uncommitted in the current worktree.
- `diagnostic_control` already performs one axsync `try_lock`, commits under the Service guard,
  drops it and publishes queue work. No test calls this entry or a shared production seam.
- `rx_snapshot_v3` already copies the lease tuple and ledger while holding one Service guard.
  `v3_lease_tuple_is_committed_service_state_under_interleavings` only reads Service getters under
  an injected guard and cannot detect a regression in snapshot assembly.
- Fresh Review baseline: axnet feature/default `231/214`, driver suites `7/16/36`, MS03/MS04
  harnesses `33/16`, kernel QEMU, rustfmt, strict OpenSpec and diff check pass. D1 remains the
  expected exit-101 comparison with 25 established axfs/axtask errors.

**Current-State Evidence**

- `lib.rs::diagnostic_control` resolves the global Service directly, so an injected test cannot
  currently exercise its acquisition/event sequence.
- `async_rx::ServiceAccess` already represents production-global and test-injected Service owners.
  Its `lock()` is intentionally blocking for queue/flush progress; adding a distinctly real
  nonblocking acquisition for diagnostic control can reuse the same guard enum without changing
  queue or flush behavior.
- `async_rx::rx_snapshot_v3` performs both Service acquisition and V3 assembly inline. Extracting a
  private shared assembly path allows an injected Service to exercise the exact production mapping
  without initializing or replacing the global `Once`.
- `QUEUE_EVENT::generation()` is observable inside the crate and can prove no publication on Busy
  or invalid input and one publication on a successful commit.
- `Service::diag_control` and `diag_hold_tick` already provide deterministic fake-clock state
  transitions. The missing proof is the control/snapshot boundary, not lease-state arithmetic.

**Relevant Code**

| Area | Files and symbols | Responsibility |
|---|---|---|
| Control entry | `crates/axnet/src/lib.rs::diagnostic_control` | global lookup, bounded acquisition, commit, post-unlock event |
| Service access | `crates/axnet/src/async_rx.rs::ServiceAccess/ServiceGuard` | global/injected lock adapter |
| V3 assembly | `crates/axnet/src/async_rx.rs::rx_snapshot_v3` | Service-guarded ledger and lease tuple mapping |
| Lease state | `crates/axnet/src/service.rs::{diag_control,diag_hold_tick}` | checked commit and expiry transition |
| Event | `crates/axnet/src/async_rx.rs::QueueEvent` | queue generation and owner wake publication |

**Critical Path**

```text
production diagnostic_control
  -> shared private control path
  -> one nonblocking Service try-lock
  -> Busy/error: no mutation, no event
  -> success: commit -> unlock -> exactly one queue event

production rx_snapshot_v3
  -> shared private V3 assembly path
  -> one Service guard
  -> copy ledger + lease tuple
  -> return one committed before/after state

tests inject the same Service access and event types
  -> force Busy/success and control/tick ordering
  -> exercise shared production paths, not duplicated test logic
```

**Behavioral Change**

None. This Cycle adds testability seams and witnesses for the existing C2/C5 behavior. Public
commands, errors, event ordering, V3 bytes, queue ownership and timer behavior must remain unchanged.

**Change Surface**

| Repair | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| T4.4-R1 | R15/C2 control contention | `lib.rs::diagnostic_control`, `async_rx::ServiceAccess`, focused tests | direct global try-lock and event | share the real bounded path with injected tests |
| T4.4-R2 | R15/C5 committed V3 | `async_rx.rs::rx_snapshot_v3`, focused tests | inline global guard and mapping | share the real assembly path with deterministic interleaving tests |

**Task Contracts**

### T4.4-R1: Bounded production control witness

- Requirement/Scenario: R15/C2; Control contention, invalid control and successful Hold/Release.
- Depends on: Cycle 000 Service-owned state and `QueueEvent` generation.
- Targets: `crates/axnet/src/lib.rs::diagnostic_control`,
  `crates/axnet/src/async_rx.rs::ServiceAccess/ServiceGuard`, focused crate tests.
- Current behavior: production code visibly uses `try_lock`, but tests call only
  `Service::diag_control`; Busy/error/event ordering is untested.
- Required behavior: a test-injected call through the same private control path used by production
  returns `ResourceBusy` immediately while Service is held, changes neither lease nor queue
  generation, rejects invalid/overflow input without publication, and publishes exactly once after
  a successful unlocked commit. A wake-time observer must be able to `try_lock` the injected Service
  when the success event fires, so a source-order assertion alone cannot masquerade as unlock proof.
- Required changes: provide one shared private control path with global and injected Service access;
  keep blocking `ServiceAccess::lock()` for queue/flush and give diagnostic control a genuinely
  nonblocking acquisition. Add deterministic Busy/error/success tests and a source guard proving the
  public entry delegates to this path.
- Preserve: `DevError` to syscall mapping, checked deadline, Service-owned fields, post-unlock
  publication, one queue owner, QEMU-only feature gating and ordinary build exclusion.
- Forbidden: blocking fallback, retry loop, sleep, global test initialization, second diagnostic
  owner, public test API, direct ring/slot mutation or changes to flush lock semantics.
- Test witness: before repair, inventory shows no test caller for `diagnostic_control` and no shared
  production seam. Add
  `diagnostic_control_shared_path_is_bounded_and_publishes_after_unlock`; it must hold the injected
  Service, call the shared path and assert Busy/state/generation plus wake-time unlock invariants.
  Repeat it 100×.
- GREEN condition: all Busy/error/success assertions pass and a regression to blocking acquisition,
  event-on-error or pre-unlock publication fails the focused tests or source guard.
- Verification: focused qemu-diagnostics control tests, 100× repetition, feature/default full axnet
  suites, QEMU/D1 feature checks and diff review.
- Stop when: the production path cannot be shared without changing public errors, queue ownership,
  axsync semantics or the wire ABI; return to Plan instead of adding a test-only duplicate.

### T4.4-R2: Committed production V3 interleaving witness

- Requirement/Scenario: R15/C5; V3 racing Hold/Release or expiry tick returns a real before/after
  committed tuple together with the guarded ledger.
- Depends on: Cycle 000 `rx_snapshot_v3`, Service-owned lease and fake clock.
- Targets: `crates/axnet/src/async_rx.rs::rx_snapshot_v3`, its private assembly helper and focused
  crate tests.
- Current behavior: production assembly holds one Service guard, but the existing test reads Service
  getters directly and never executes snapshot assembly.
- Required behavior: deterministic injected tests execute the same private V3 assembly path used by
  public `rx_snapshot_v3` while control and tick are ordered on either side of acquisition. Every
  result is the complete before or after committed tuple; no contention path emits the pre-init
  all-zero/synthetic no-hold representation.
- Required changes: extract only the minimum private snapshot seam needed to share production
  assembly with an injected Service. Replace or rename the misleading direct-getter test, add
  control/tick interleaving cases and a source guard proving the public entry uses the shared seam.
- Preserve: V1/V2/V3 sizes, offsets and sentinel values; one guard for ledger and lease; pre-init
  missing-Service all-zero behavior; default-build zero lease fields; blocking snapshot semantics;
  ISR freedom from Service access.
- Forbidden: test-only reimplementation, direct getters presented as V3 proof, Busy encoded as
  RELEASED, a second snapshot lock, public injection API, ABI changes or mutation from snapshot.
- Test witness: before repair, the named V3 interleaving test contains no `rx_snapshot_v3` or shared
  assembly call. Add
  `v3_shared_snapshot_path_returns_only_committed_tuples_under_control_and_tick`; the shared path
  must be exercised under forced before/after control/tick ordering and repeated 100×.
- GREEN condition: observed tuples and ledger always match one committed Service state; synthetic,
  torn or cross-state tuples fail deterministically.
- Verification: focused V3 tests and 100× repetition, Rust/C ABI harness, axnet feature/default full
  suites, kernel QEMU/D1 comparison and source/diff review.
- Stop when: a testable shared path requires changing V3 wire layout, nonblocking snapshot semantics,
  queue ownership or public APIs; return to Plan.

**Invariants**

- Service remains the sole owner of diagnostic lease fields and the V3 ledger view.
- Diagnostic control acquisition is bounded; queue task and flush retain their existing blocking
  Service acquisition and no guard crosses `Pending`.
- Busy and validation errors publish no queue event; successful Hold/Release publishes once after
  unlock.
- V3 returns a committed tuple or the documented pre-init all-zero result; runtime contention never
  becomes RELEASED.
- Timer remains wake-only. No independent lease generation or global diagnostic state returns.
- V1/V2/V3 ABI, QEMU-only scope and single-hart evidence limits remain unchanged.

**Non-goals**

- No runtime probe, guest payload, manual QEMU, persisted Evidence or Task 5.1 work.
- No warning cleanup, driver behavior change, queue/event redesign, reset, SMP, DWMAC or board work.
- No global task, spec, design, proposal, SNAPSHOT, M/D/K/R/I or Iteration Map update.

**Acceptance**

| Repair | Proof | Status |
|---|---|---|
| T4.4-R1 | shared production control path proves Busy/no-event, invalid/no-event and success/post-unlock single event; contention passes 100× | Planned |
| T4.4-R2 | shared production V3 assembly returns only committed before/after control/tick tuples; interleavings pass 100× | Planned |
| Regression | C1/C3/C4/C6, ABI, drivers, harnesses and kernel feature boundaries remain green with no new warning | Planned |

Any duplicated test algorithm, blocking diagnostic acquisition, synthetic V3 tuple, public API or
ABI drift, queue/flush lock change, new warning, missing repeat evidence or unrelated cleanup blocks
acceptance.

**Verification**

Act must record exact commands, key output and exit status for:

```text
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --features qemu-diagnostics --lib diagnostic_control_shared_path_is_bounded_and_publishes_after_unlock -- --nocapture
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --features qemu-diagnostics --lib v3_shared_snapshot_path_returns_only_committed_tuples_under_control_and_tick -- --nocapture
repeat each contention and V3 interleaving test 100 times
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --features qemu-diagnostics --lib
cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline
cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net
cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --features alloc
rustc --edition=2024 --test tests/ms03-irq-host-harness.rs -o /tmp/ms03-irq-host-test && /tmp/ms03-irq-host-test
rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test
cargo check --offline -p starry-kernel --features qemu
cargo check --offline -p starry-kernel --features lichee-d1
rustfmt --check --edition 2024 --config skip_children=true <all Cycle-owned Rust files>
openspec validate ms05-qemu-bounded-bidirectional-device-data-plane --strict
git diff --check -- crates/axnet kernel tests openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane
```

The D1 command remains an expected exit-101 comparison and must contain exactly the established 25
axfs/axtask errors. Review the complete Cycle diff for production delegation, post-unlock event
publication, one-guard V3 assembly, absence of independent diagnostic state and absence of ISR
Service access.

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Authorization | PASS | user requested Iteration 008 audit; Review may create the required local rework Cycle |
| Investigation | PASS | production control, Service access, V3 assembly, fake clock, event generation and current test inventory inspected |
| Design | PASS | existing Service-owned design is retained; only shared private seams and missing witnesses are required |
| Iteration Plan | PASS | both repair items close Task 4.4 Acceptance inside Iteration 008; later Iterations remain blocked |
| Cycle Scope | PASS | C2/C5 test gaps map directly to Cycle 000 findings; no new requirement or behavior |
| Task Contracts | PASS | targets, required witnesses, preserved boundaries, prohibitions and stop conditions are explicit |
| Traceability | PASS | R15/D9-D10 -> Task 4.4 -> T4.4-R1/R2 -> production paths -> focused tests |
| Verification | PASS | deterministic tests, 100× repeats, ABI, driver, harness, kernel and diff gates cover the repair |

**Persisted Evidence**

- Mode: none

The Act Response must carry commands, key output, exit status, changed symbols and full-diff review.
Do not create an Evidence directory.

**Risks and Notes**

- Test seams must remain private and compile away or stay crate-private; a public injection API is
  outside this repair.
- The public V3 snapshot remains blocking by design. Only diagnostic control requires bounded
  acquisition; do not apply its `try_lock` semantics to snapshot, queue or flush.
- Existing warnings are recorded baseline, not repair scope. Any Cycle-owned new warning blocks.

## Act Response

- Status: reported

**Implemented**

- T4.4-R1（共享 bounded control path）：新增 `async_rx::diagnostic_control_shared(service,
  notify, op, lease_ms)`，生产 `lib.rs::diagnostic_control` 与注入式 host tests 共用同一私有
  路径。单次非阻塞 `try_lock`（axsync/spin 的 `try_lock`，不是伪装为 `try_lock` 的阻塞 seam）；
  Busy 时立即返回 `DevError::ResourceBusy`，缺全局 Service 返回 `BadState`（保持既有 syscall
  映射），两种失败路径都不改变 lease 或 queue generation、不发布 event；成功路径在 guard 内
  checked commit、drop guard 后恰好发布一次 `publish_queue_work`。
- T4.4-R2（共享 V3 assembly seam）：新增 `async_rx::rx_snapshot_v3_from(base, service)`，把原
  `rx_snapshot_v3` 的 Service 获取与 V3 装配抽成共享私有路径，`pub fn rx_snapshot_v3()` 一行
  委托。lease tuple 与 slot/ticket/flush/drop ledger 在同一 Service guard 下复制；pre-init 缺失
  Service 仍返回既有 all-zero fallback。V1/V2/V3 wire layout、QEMU-only feature 边界、阻塞
  snapshot 语义、ISR 无 Service 访问均未改变。
- 测试：删除仅读 Service getters 的误导性 `v3_lease_tuple_is_committed_service_state_under_
  interleavings`；新增 `diagnostic_control_shared_path_is_bounded_and_publishes_after_unlock`
  （Busy/validation/overflow 不发布 + wake-time unlock probe 证明成功路径在解锁后单次发布）、
  `v3_shared_snapshot_path_returns_only_committed_tuples_under_control_and_tick`（control/tick
  在获取前后强序，只返回 committed before/after tuple）、以及两个 source guards
  （`diagnostic_control_public_entry_delegates_to_shared_path_in_source`、
  `rx_snapshot_v3_public_entry_delegates_to_shared_assembly_in_source`，用 `include_str!` 结构
  断言生产入口只委托、不自行 try_lock/publish/读 ledger/lease）。

**Changed Files and Symbols**

- `crates/axnet/src/lib.rs`：`diagnostic_control` 改为一行委托
  `diagnostic_control_shared(ServiceAccess::Global, &QUEUE_EVENT, op, lease_ms)`；
  `BadState`/`ResourceBusy` 语义由共享路径保持。
- `crates/axnet/src/async_rx.rs`：
  - 新增 `diagnostic_control_shared`（`qemu-diagnostics` feature gated）。
  - 新增 `rx_snapshot_v3_from`、`read_v3_ledger_and_lease`、`default_v3_ledger_and_lease`、
    类型别名 `V3LedgerAndLease`；`rx_snapshot_v3` 改为委托。
  - tests：删除 `v3_lease_tuple_is_committed_service_state_under_interleavings`；新增
    `UnlockObservingWake`/`unlock_observing_waker`、
    `diagnostic_control_shared_path_is_bounded_and_publishes_after_unlock`、
    `v3_shared_snapshot_path_returns_only_committed_tuples_under_control_and_tick`、
    `diagnostic_control_public_entry_delegates_to_shared_path_in_source`、
    `rx_snapshot_v3_public_entry_delegates_to_shared_assembly_in_source`。

**Deviations from Plan**

- `diagnostic_control_shared` 通过现成的 `ServiceGuard` enum 统一 axsync/spin 两种 mutex 类型
  （`match service` 分别 `try_lock` 后构造 `ServiceGuard::Global/Injected`），而不是给
  `ServiceAccess` 增加统一 `try_lock()` 方法；原因：统一方法返回 `Option` 无法区分"缺全局
  Service（`BadState`）"与"被持有（`ResourceBusy`）"，会改变 syscall 映射。该处理保持在
  T4.4-R1 契约内（bounded、无阻塞 seam、Busy/error 不发布、成功解锁后单次发布）。
- T4.4-R1 source guard 的 `include_str!` 路径初稿为 `../lib.rs`（解析到 crate 根），改为
  `lib.rs`（同目录）；T4.4-R2 source guard 的 entry 切片终点从 struct doc comment 改为
  `rx_snapshot_v3_from` 的 doc comment 起点，避免把共享 seam 内部读入 entry 切片导致误断言。

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: PASS
- Full diff reviewed: PASS
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 0（新增）

全量 diff review（lib.rs 9 行改动 + async_rx.rs 本 Cycle 部分，叠加在 Cycle 000 未提交改动
之上）：C2/C5 的生产入口均为一行委托，Busy/error 不发布、成功解锁后单次发布由共享路径统一
实现；wake-time unlock probe 是行为断言（wake 回调内 `try_lock` 注入 Service），不是源码顺序
断言；V3 装配只经共享 seam，pre-init fallback 保持，V1/V2/V3 布局未动；ISR 路径（kernel）零
修改、零 Service 访问；无跨任务遗漏实现、无计划外修改、无新增 warning（axnet 的 4 个 warning
均为既有基线：`flush.rs` 3 处 `unused_mut` + `MAX_LIVE_TICKETS` unused import，Plan 禁止清理）。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| RED (T4.4-R1) | `cargo test ... --features qemu-diagnostics --lib diagnostic_control_shared_path` | `error[E0425]: cannot find function \`diagnostic_control_shared\`` | RED 确认（生产 control 路径无注入 seam） |
| RED (T4.4-R2) | 同上 filter `v3_shared_snapshot_path` | `error[E0425]: cannot find function \`rx_snapshot_v3_from\`` | RED 确认（共享 assembly seam 不存在） |
| T4.4-R1 witness | `... --lib diagnostic_control_shared_path_is_bounded_and_publishes_after_unlock` | `1 passed`；Busy/InvalidParam/overflow 均不发布，success 后 generation+1、单次 wake、wake-time `try_lock` 成功 | PASS |
| T4.4-R2 witness | `... --lib v3_shared_snapshot_path_returns_only_committed_tuples_under_control_and_tick` | `1 passed`；control/tick 前后强序只返回 committed tuple | PASS |
| source guards | `... --lib delegates_to_shared` | `2 passed` | PASS |
| 竞态 100× | `for i in $(seq 1 100); cargo test ... shared_path_is_bounded...; done`（fmt 前后各一轮） | 全部 `test result: ok` | PASS |
| 竞态 100× | 同上 `v3_shared_snapshot_path`（fmt 前后各一轮） | 全部 `test result: ok` | PASS |
| axnet full (feature) | `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --features qemu-diagnostics --lib` | `234 passed; 0 failed` | PASS |
| axnet full (default) | `... --offline --lib` | `215 passed; 0 failed` | PASS |
| axdriver_net | `cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline` | `7 passed` | PASS |
| axdriver_virtio | `... --offline --features net` | `16 passed` | PASS |
| virtio-drivers | `... --offline --features alloc` | `36 passed` | PASS |
| MS03 harness | `rustc --edition=2024 --test tests/ms03-irq-host-harness.rs -o /tmp/ms03-irq-host-test && /tmp/ms03-irq-host-test` | `33 passed` | PASS |
| MS04 harness | `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test` | `16 passed` | PASS |
| kernel QEMU | `cargo check --offline -p starry-kernel --features qemu` | exit 0，无本 Cycle 新增 warning | PASS |
| kernel D1 | `cargo check --offline -p starry-kernel --features lichee-d1` | exit 101，恰为既有 25 个 axfs/axtask 错误 | PASS（预期比较） |
| rustfmt | `rustfmt --check --edition 2024 --config skip_children=true crates/axnet/src/lib.rs crates/axnet/src/async_rx.rs` | exit 0（先 apply 后 check） | PASS |
| OpenSpec strict | `openspec validate ms05-qemu-bounded-bidirectional-device-data-plane --strict` | `Change ... is valid` | PASS |
| diff check | `git diff --check -- crates/axnet kernel tests openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane` | exit 0 | PASS |

**Persisted Evidence**

None required（Iteration 008 Persisted Evidence 模式为 `none`；验证摘要由本 Act Response
承载，未创建 Evidence 目录）。

**Experience Candidates**

None（本 Cycle 是纯测试 seam 抽取与 witness 补全，无端到端操作路径或系统性故障，不构成
Runbook/Incident 候选）。

**Remaining Issues**

- 既有 4 个 axnet warning（`flush.rs` 3 处 `unused_mut`、`MAX_LIVE_TICKETS` unused import）与
  smoltcp warnings：按 Iteration 007/008 Plan 记录禁止清理，留待后续范围。

**Commit or Diff Reference**

未创建 commit。Cycle 001 工作叠加在 HEAD `223f6281` + Cycle 000 未提交改动之上；本 Cycle
修改 `crates/axnet/src/lib.rs` 与 `crates/axnet/src/async_rx.rs`（共享 seam 抽取 + 生产入口
委托 + witness/source-guard tests），Cycle 文档为
`iterations/008-service-owned-diagnostic-lease/001-rework.md`。

## Plan Review

- Status: reviewed

**Review Result**

Accepted.

**Findings**

- No blocking finding. The production `diagnostic_control` entry delegates to the same injected
  seam exercised by the contention test; the held-Service branch returns `ResourceBusy` without
  mutation or publication, and the success witness observes that the Service can be acquired from
  the wake callback after commit.
- No blocking finding. The production `rx_snapshot_v3` entry delegates to the shared assembly seam;
  its injected interleavings read the ledger and lease tuple under one Service guard and return only
  complete committed before/after states.
- Existing axnet/smoltcp warnings and the D1 25-error comparison are established baselines. Cycle
  001 introduced no warning, public API, wire-layout, ISR or feature-boundary change.

**Deviation Classification**

None. The Act used the existing `ServiceGuard` variants instead of adding a `ServiceAccess::try_lock`
method so missing-global `BadState` remains distinguishable from held-Service `ResourceBusy`. This
is a private implementation choice inside T4.4-R1 and preserves the planned behavior.

**Acceptance Gaps**

None. Cycle 000 C2 and C5 gaps are closed by production-path witnesses and source delegation guards.

**Convergence**

Reduced. This rework Cycle closes both inherited gaps without creating a new requirement, design
issue or adjacent fault domain.

**Evidence**

- Independent Review reran the two focused witnesses (`1/1` each), both delegation guards (`2/2`)
  and each focused race 100 times with zero failure.
- Fresh full regressions passed: axnet qemu-diagnostics `234/234`, axnet default `215/215`,
  axdriver_net `7/7`, axdriver_virtio net `16/16`, virtio-drivers alloc `36/36` plus `8/8`
  doctests, MS03 harness `33/33` and MS04 harness `16/16`.
- Kernel QEMU check exited 0. D1 exited 101 with exactly the established 25 axfs/axtask errors.
  Rustfmt, strict OpenSpec validation and scoped `git diff --check` all exited 0.
- Full Cycle diff review found zero unresolved Critical, Important or Minor findings. The Review
  confirmed post-unlock publication, one-guard V3 assembly, no independent diagnostic state and no
  ISR Service access.

**Follow-up Decision**

Accept Cycle 001 and Iteration 008. Proceed to the next planned logical boundary, Iteration 009,
for Tasks 5.1-5.2; do not create Cycle 002.

**Iteration Plan Update**

None.

**Next Cycle**

None.

**Next Iteration**

`../009-probe-and-automatic-product-gates/000-initial.md`
