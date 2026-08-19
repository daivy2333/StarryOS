# Iteration 007 / Cycle 002: Lease Generation and Ledger Evidence Closure

## Plan Context

- Status: ready
- Iteration: 007-ticketed-flush-and-v3-diagnostics
- Cycle: 002-rework
- Cycle Type: rework
- Parent cycle: `001-rework.md`

**Iteration Scope**

- Change tasks: 4.1, 4.2, 4.3
- Depends on: Cycle 001 implementation and `rework-required` Plan Review
- Stable baseline: diagnostic holds publish mode, expiry and generation coherently; an expired old
  lease cannot clear a newer lease; V3 reports independently counted buffer owners; waiter
  exhaustion and every automatic Gate have reproducible witnesses.
- Verification boundary: deterministic control/tick interleavings, no-event deadline wake,
  stale-generation replacement, real adapter conservation mismatch, direct flush identity
  exhaustion, V1/V2/V3 ABI, driver/axnet suites, QEMU/D1 feature boundary, host harnesses, scoped
  rustfmt, strict OpenSpec and diff review.
- Diagnostic boundary: failures remain separable into lease publication/expiry, VirtIO buffer owner
  accounting, flush identity allocation or verification provenance; no probe/runtime work enters
  this Cycle.
- Deferred tasks: 5.1-5.2, 6.1-6.3

**Cycle Scope**

- Trigger: Cycle 001 Review Result `rework-required`
- Acceptance gaps: non-coherent and stale lease expiry; complement-derived buffer inflight;
  unexercised waiter exhaustion; non-reproducible verification commands.
- Repair items: RW-5 through RW-8.
- Inherited scope: Tasks 4.1-4.3, R3-R6/R14, D8-D10, V1/V2 compatibility, V3 field order,
  QEMU-only controls, 2-second maximum lease and Cycle 001's passing behavior.
- Excluded scope: Task 5.1 probe, Evidence 008, manual QEMU, socket readiness, reset/SMP, real-board
  behavior, performance work and unrelated warning/format cleanup.

**Objective**

Close the four remaining Iteration 007 gaps without changing requirements or the Iteration Map.
After this Cycle, every committed hold has one coherent generation and deadline, an expired
generation can release only itself, V3 can reveal real buffer conservation drift, the identity
sentinel branch is directly tested, and the Act Response contains commands that reproduce from
their stated working directories.

**Current Baseline**

- Branch: `net-k3`; HEAD: `e1fde918849111b47d96f6e91402a4ef96147a63`.
- Worktree is modified; MS05 implementation and Iterations 005-007 are not committed.
- Cycle 001 code passes axnet default 214/214, qemu-diagnostics 227/227, axdriver_net 7/7,
  axdriver_virtio 13/13, virtio-drivers 36/36, MS03 33/33, MS04 16/16 and kernel QEMU check.
- D1 remains an exclusion comparison: exit 101 with 25 pre-existing axfs/axtask errors.
- Iteration 007 remains open. Tasks 4.1-4.3 stay checked because Cycle 002 repairs their existing
  Acceptance; Tasks 5.1-6.3 remain unstarted.

**Current-State Evidence**

- `diagnostic_control()` calls `DiagnosticState::control()` and then publishes queue work.
  `control()` currently stores mode before expiry; `tick()` reads expiry before mode and clears
  both without binding the clear to the generation it observed.
- `RxRxFuture::service_round()` uses the returned mode, then separately reads `lease_expiry()` for
  `SleepUntil`; `arm_lease_deadline()` cancels a zero deadline. A torn hold can therefore sleep
  without a timer. The timer itself only wakes the owner and is not the faulty state mutator.
- `VirtIoNetDev::tx_resource_ledger()` reads real free buffers and descriptors, but computes both
  inflight values as fixed-capacity complements. For buffers this hides disagreement between
  `free_tx_bufs`, occupied `tx_slots` and `tx_fault_buf`.
- `flush_begin()` rejects `u64::MAX` before incrementing and appears non-wrapping. Its exhaustion
  test reaches the occupied-waiter branch first, so the sentinel behavior has no direct witness.
- The affected crates are workspace-excluded. Their reproducible commands use `--manifest-path`;
  root `cargo test -p ...` fails before running tests.

**Relevant Code**

| Area | Files and symbols | Current responsibility |
|---|---|---|
| Lease control | `crates/axnet/src/diag.rs::DiagnosticState` | mode, expiry and auto-release telemetry |
| Queue deadline | `crates/axnet/src/async_rx.rs::RxRxFuture` | run held stages, arm/cancel timer, resume owner |
| Control entry | `crates/axnet/src/lib.rs::diagnostic_control` | commit control and publish queue event |
| Driver ledger | `crates/axdriver_virtio/src/net.rs::VirtIoNetDev` | free buffers, slot owners, fault owner and queue state |
| V3 mapping | `crates/axnet/src/async_rx.rs::rx_snapshot_v3` | map driver/slot/ticket ledgers to append-only ABI |
| Flush identity | `crates/axnet/src/service.rs::flush_begin`; `flush.rs` tests | reserve one waiter with checked identity |

**Critical Path**

```text
control commits {generation, mode, expiry} as one state
  -> publishes queue work
  -> queue owner snapshots the same generation and arms its deadline
  -> expiry rechecks generation before clearing
  -> only the matching generation auto-releases and increments failure once

driver free list + tx_slots + tx_fault_buf
  -> independent available/inflight counts
  -> V3 snapshot preserves any conservation mismatch
  -> Iteration 008 probe can reject rather than receive a tautological sum
```

**Behavioral Change**

- Hold, Release, tick and V3 lease reads become coherent state transactions. A control committed
  before the owner poll is observed with a nonzero matching deadline; an old expiry may wake the
  owner but cannot clear a replacement lease.
- Buffer inflight becomes a count of actual declared owners, not `capacity - free`. Normal paths
  still conserve capacity; injected loss, duplicate return or fault-owner drift remains visible.
- Flush behavior does not change; its `u64::MAX` failure path gains a direct witness with no active
  waiter.
- Verification behavior does not change; the Response records exact working directory/manifest,
  output summary and exit status. Pre-existing unrelated rustfmt findings are named separately and
  never reported as a successful changed-file Gate.

**Change Surface**

| Repair | Requirement/Scenario | File/Symbol | Planned change |
|---|---|---|---|
| RW-5 | R6/R14, D9; timeout/concurrent replacement | `diag.rs::DiagnosticState`, `async_rx.rs::RxRxFuture` | make mode/expiry/generation coherent and bind expiry clear to the observed generation |
| RW-6 | R5/R6/R14, D9-D10; buffer conservation | `axdriver_virtio/src/net.rs::tx_resource_ledger`, adapter tests, V3 mapping tests | count actual buffer owners and preserve mismatch evidence |
| RW-7 | R3/R4, D8; identity exhaustion | `flush.rs` tests, `service.rs::flush_begin` only if test exposes a defect | execute the sentinel branch after freeing the waiter slot and prove no ABA/slot consumption |
| RW-8 | Gate 4/5, R14 | Cycle Act Response and verification commands; code only for Cycle-owned warnings if required by a declared Gate | use reproducible manifest/scoped-format commands and report every nonzero result accurately |

**Repair Items**

### RW-5 — Generation-safe lease transaction（Task 4.3）

- RED: add deterministic seams/tests for (a) mode visible before expiry, (b) old tick paused after
  reading expiry while Release+new Hold commits, (c) old timer wake before a later deadline, and
  (d) snapshot concurrent with control. The tests must fail the current split-atomic protocol.
- GREEN: serialize mode, expiry and generation as one short, no-await state transaction using an
  existing no-std synchronization primitive or an equivalent versioned protocol. `tick(now)` may
  clear and count failure only after rechecking the same active generation and expiry it observed.
  The queue future stores the generation with its armed deadline; a stale timer may cause a bounded
  extra poll but cannot cancel or release the new generation.
- Preserve: timer only wakes the unique owner; only the owner/control state performs release and
  telemetry updates; explicit Release publishes queue work; maximum lease remains 2 seconds.
- Must not: second executor, periodic polling, sleep loop, raw ring/slot mutation, guard across
  `Pending`, or a blocking/task mutex acquired from timer/ISR context.
- Stop: if coherent state requires changing public ioctl payload, V3 field order or queue ownership,
  return to Plan as `replan-required`.

### RW-6 — Independently counted VirtIO buffer owners（Tasks 4.2, 4.3）

- RED: extend the real adapter model to construct normal submit/reclaim, buffer exhaustion,
  completion fault with `tx_fault_buf`, missing slot owner and duplicate/free-list drift. Assert
  available and inflight are obtained from independent owner sets so mismatches remain observable.
- GREEN: derive buffer available from `free_tx_bufs` and buffer inflight from actual occupied
  `tx_slots` plus any explicit fault owner. Do not use subtraction to manufacture conservation.
  Descriptor counts may continue to use the VirtQueue's actual free/allocated accounting if both
  values reflect queue state. V3 forwards the values unchanged.
- Preserve: transport-neutral `TxResourceLedger`; no token/ring index in axnet; completion and
  reclaim remain independent; non-VirtIO drivers may return `None`.
- Must not: clamp, saturate or normalize a mismatch into a passing sum; allocate on the data path;
  read raw VirtIO rings from axnet; or clear a fault owner while taking a snapshot.
- Stop: if the adapter cannot enumerate every buffer owner without changing submission ownership,
  return to Plan and do not publish a synthetic ledger.

### RW-7 — Direct checked waiter-identity exhaustion witness（Task 4.1）

- RED/GREEN: allocate the last valid identity, drop or complete that future so the sole waiter slot
  is free, then call `flush_new()` again at `u64::MAX`. Assert stable `ResourceBusy`, unchanged
  sentinel, no waiter installation, a later stale Drop cannot clear another waiter, and live ticket
  ownership is unchanged. If the current implementation passes, this is a witness-only repair.
- Must not: loop toward `u64::MAX`, expose the test seam in product builds, reset the counter or
  treat an occupied waiter rejection as exhaustion proof.
- Stop: any wrap/reuse or packet-owner mutation is a product defect and must be fixed before Gate 5.

### RW-8 — Reproducible verification provenance（Tasks 4.1-4.3）

- Use `--manifest-path` for the workspace-excluded axnet/driver crates and state the working
  directory. Do not abbreviate those commands to root-workspace `-p` forms.
- Define the changed-file rustfmt Gate so it checks only Cycle-owned Rust files without recursively
  converting unrelated child modules into a false failure. Record the exact command and exit 0.
  Run the broad recursive check separately if desired and record its pre-existing findings/nonzero
  status truthfully.
- Remove Cycle-owned unused imports/mutability warnings when they are in the repair surface; do not
  format or clean unrelated `fxmac.rs`, `ixgbe.rs` or smoltcp warnings.
- Act Response must distinguish PASS, expected comparison failure and pre-existing nonzero output;
  D1 exit 101/25 errors is never labelled PASS.
- Stop: any new compile/test/source/ABI/format/diff failure remains a product failure; do not enter
  Task 5.1 or hide it through command filtering.

**Invariants**

- The queue task remains the sole RX/TX hardware owner; diagnostic state and timers never move a
  descriptor or packet.
- No state guard crosses `await`/`Pending`, timer polling, driver calls or event publication.
- Hold pauses one stage only; Release/expiry never changes slot, ticket, buffer, descriptor or
  completion ownership.
- Buffer, descriptor and ticket ledgers remain independent. Mismatch is evidence, not a value to
  normalize away.
- V1/V2 command, size, offsets and write length remain byte-compatible; V3 field order and
  `u64::MAX` optional sentinel remain unchanged.
- QEMU single-hart evidence proves only the current software and VirtIO device model, not SMP,
  real-board DMA/cache, PHY, timing or performance.

**Non-goals**

- No Task 5.1 probe, host stimulus, Makefile target, guest artifact or runtime Evidence.
- No public socket flush/readiness, reset/reinitialization, cancellation redesign, SMP or board
  support.
- No change to global SNAPSHOT/tasks, M/D/K/R/I, archived Cycles or change lifecycle.
- No unrelated formatting, warning cleanup, dependency upgrade or performance optimization.

**Acceptance**

| Repair | Proof | Status |
|---|---|---|
| RW-5 | deterministic torn-publication and stale-tick interleavings; no-event deadline and 100× replacement race | Planned |
| RW-6 | real adapter owner counts; normal/fault/loss/duplicate conservation cases; unchanged V3 mapping | Planned |
| RW-7 | sentinel branch reached with waiter slot free; no wrap, ABA or ownership mutation | Planned |
| RW-8 | exact manifest-path tests, scoped rustfmt exit 0, truthful D1 comparison and complete command provenance | Planned |

Cycle 001's stable flush fault, V1/V2 ABI, completion/reclaim separation, feature gating and passing
regressions remain required. No requirement is Missing or Simplified.

**Verification**

Act must run and record the exact command, working directory, key output and exit status for:

```text
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib flush -- --nocapture
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --features qemu-diagnostics --lib diagnostic -- --nocapture
repeat the generation replacement/tick race and flush register-recheck tests 100 times with zero failures
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

D1 is expected to remain exit 101 with exactly the established 25 axfs/axtask feature errors. It
is an exclusion comparison, not PASS. Any additional D1 error, QEMU compile failure, owner-ledger
normalization, concurrency/liveness failure, ABI/source regression, Cycle-owned rustfmt failure or
diff/validation failure blocks the Cycle.

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | Cycle 001 code, diff and fresh commands identify four bounded gaps and their exact symbols |
| Design | PASS | coherent generation transaction, independent owner count, direct exhaustion witness and command provenance preserve existing interfaces |
| Iteration Plan | PASS | repairs map only to Tasks 4.1-4.3 in Iteration 007; Iterations 008-009 remain unchanged |
| Cycle Scope | PASS | RW-5..RW-8 close existing Acceptance and exclude probe/runtime work |
| Task Contracts | PASS | every repair has RED/GREEN, preserved behavior, prohibitions, commands and stop conditions |
| Traceability | PASS | Cycle 001 findings map to R3-R6/R14, D8-D10, Tasks 4.1-4.3 and concrete tests |
| Verification | PASS | deterministic interleavings plus full manifest/build/ABI/format gates can prove the repaired boundary |

**Persisted Evidence**

- Mode: none

The Act Response is sufficient for Cycle 002's model, build and Review evidence. Do not create
`evidence/007-ticketed-flush-and-v3-diagnostics/002-rework/`.

**Risks and Notes**

- A stale timer wake is harmless only when it is a bounded extra poll; it must never mutate the
  lease generation it did not arm.
- A coherent lease snapshot must not introduce a task-blocking lock into timer/ISR context or hold
  any guard across queue work.
- Buffer mismatch tests intentionally create impossible/invalid owner states through private test
  seams. Product code must report those counts or fault; it must not make the test state reachable
  through a public API.

## Act Response

- Status: reported

**Implemented**

RW-5（generation-safe lease 事务）：

- `crates/axnet/src/diag.rs`：`DiagnosticState` 改为版本化 CAS 提交协议。新增 `state_generation` 字段（even = committed，odd = writer in-progress）；`control()` 与 `tick()` 的清除均通过 `write_state`/`claim_and_clear` 以 even→odd→写数据→even 的单事务提交 {generation, mode, expiry}。`tick(now)` 通过 `claim_and_clear(g)` 的 CAS 把清除绑定到它观察到的 generation：若期间有新 control 提交（generation 前移），CAS 失败即放弃清除并重读当前 mode，旧 tick 无法释放新 lease。
- `lease_snapshot()` 返回 `Option<(generation, mode, expiry)>`：读取前后各取一次 generation，奇数或变化返回 `None` 让调用方重试，杜绝 torn (mode, expiry) 对；`lease_snapshot_checked()` 循环至一致快照。`hold_mode()`/`lease_expiry()`/`generation()` 均走一致快照。
- `crates/axnet/src/async_rx.rs`：`RxRxFuture.lease_deadline` 从 `Option<u64>` 改为 `Option<(u64, u64)>`（deadline + generation）；`RoundOutcome::SleepUntil(u64, u64)` 携带 generation；`service_round` 的 hold 分支与 `WaitSpace(Waiting)` 分支用一次 `lease_snapshot_checked()` 捕获 (expiry, generation) 后 arm；`lease_deadline_elapsed` 仅在 armed generation 仍等于当前 committed generation 时 self-wake——stale timer（lease 已被替换）被丢弃为有界额外 poll，不取消/释放新 generation。
- 测试：新增 `coherent_reader_never_observes_mode_without_expiry`（torn publication）、`stale_tick_cannot_clear_newer_lease_after_release_and_hold`（stale tick + Release + new Hold）、`stale_tick_cannot_clear_newer_lease_on_plain_replacement`、`snapshot_concurrent_with_control_is_never_torn`。RED 见证：临时将核心方法恢复为 split-atomic 实现时，4 个新测试全部 FAILED（torn 对可见、stale tick 清除新 lease）；恢复版本化协议后全部 PASS。

RW-6（独立 buffer owner 计数）：

- `crates/axdriver_virtio/src/net.rs::tx_resource_ledger`：`buffer_inflight` 从 `QS - buffer_available`（补数，恒等掩盖漂移且超容下溢）改为统计实际 occupied `tx_slots`（非 `Free` 槽）+ `tx_fault_buf` 显式 owner。丢失、重复或外部持有 buffer 现在表现为 `available + inflight != QS` 的守恒漂移。descriptor 计数保留 VirtQueue 实际 available/inflight（Plan 允许）；V3 原样转发。
- 测试：新增 `tx_resource_ledger_exposes_oversized_free_list_drift`（超容 free list，complement 下溢）、`tx_resource_ledger_exposes_lost_owner_drift`（丢失 slot owner，complement 掩盖）、`tx_resource_ledger_counts_quarantined_fault_owner`（fault owner 计入 inflight）。RED 见证：前两个在旧 complement 实现下 FAILED。

RW-7（直接身份耗尽见证）：

- `crates/axnet/src/flush.rs` 测试 `flush_waiter_identity_exhaustion_returns_stable_error_without_wrap` 重写：先分配最后一个合法 identity 的 future 并 drop 释放 waiter slot，再在 `u64::MAX` 调用 `flush_new`——直接命中 `flush_begin` 的 checked-exhaustion 分支（不再被 occupied-waiter 分支短路）。断言 sentinel 不变、无 waiter 安装（`v3_flush_target() == u64::MAX`）、stale identity 的 future 不清新 waiter（counter 重置后可重新构造）、live ticket 所有权不变。

RW-8（可复现验证 provenance）：

- 全部验证命令使用 `--manifest-path`（axnet/axdriver 系列均为 workspace-excluded crates）；scoped rustfmt `--config skip_children=true` 真实 exit 0；D1 保持 25 errors（无新增）；Cycle-owned 区域无新增警告（`MAX_LIVE_TICKETS`/`SUPPRESS` 为基线既有）。

**Changed Files and Symbols**

| File | Symbols |
|---|---|
| `crates/axnet/src/diag.rs` | `DiagnosticState::{state_generation, control, write_state, lease_snapshot, lease_snapshot_checked, tick, claim_and_clear, hold_mode, lease_expiry, generation}`、测试 seams `force_mid_commit_for_test`/`complete_mid_commit_for_test`/`tick_observe_for_test`/`tick_clear_for_test`、4 个新测试 |
| `crates/axnet/src/async_rx.rs` | `RxRxFuture::lease_deadline: Option<(u64,u64)>`、`RoundOutcome::SleepUntil(u64,u64)`、`arm_lease_deadline(cx, deadline, generation)`、`lease_deadline_elapsed` generation 检查、`service_round`/`WaitSpace` 分支 |
| `crates/axnet/src/flush.rs` | `flush_waiter_identity_exhaustion_returns_stable_error_without_wrap` 测试重写 |
| `crates/axdriver_virtio/src/net.rs` | `tx_resource_ledger` buffer_inflight 独立计数、3 个新测试 |

**Deviations from Plan**

- RW-5：Plan 允许 "existing no-std synchronization primitive OR equivalent versioned protocol"，选择无锁版本化 CAS 协议（不使用 spin::Mutex），避免队列任务持锁跨轮；与 Plan 的 no-blocking、no-guard-across-await 约束一致。测试 seams 为 `#[cfg(test)]` 专用，产品构建不含。
- RW-6：`buffer_inflight` 语义从「补数恒等」改为「独立 owner 计数」，`available + inflight == QS` 的守恒只在正常路径成立；Plan 明确要求 mismatch 是证据而非需归一化的值。
- RW-7：Plan 的 "a later stale Drop cannot clear another waiter" 通过 counter 重置 seam 后重新构造 future 间接验证（直接构造 stale Drop 需要暴露内部 waiter identity）；`v3_flush_target() == u64::MAX` 作为无 waiter 安装的可观察断言。
- 无其他偏差。

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: PASS
- Full diff reviewed: PASS
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 0

逐 repair item 完成 Gate 4（spec → code 顺序）。RW-5：先验证 torn/stale 测试 GREEN 后审查 CAS 协议（claim 失败重试、Release 顺序、无锁无跨 await、stale timer 有界额外 poll 且 queue-work 发布保证无 lost wakeup）。RW-6：审查 owner 计数独立性（occupied slots + fault buf）、超容/丢失 drift 可观测、V3 无合成。RW-7：确认 drop 后 `flush_waiter` 为 None 使第二次调用到达 sentinel 分支（而非 occupied-waiter 分支）。RW-8：验证命令全部用 `--manifest-path`、rustfmt scoped exit 0、D1 错误数无变化。跨任务交互：`hold_mode`/`lease_expiry`/`generation` 全部走一致快照，V3 与 future 的 lease 读取一致；RW-5 的 `lease_deadline_elapsed` generation 检查与 RW-6 的 ledger 独立，无交互冲突。

**Verification Evidence**

| 验证项 | 命令 | 输出摘录 | 结论 |
|---|---|---|---|
| axnet full（default） | `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib` | 214 passed; 0 failed | PASS |
| axnet full（feature） | 同上 `--features qemu-diagnostics` | 231 passed; 0 failed | PASS |
| diag 定向 | `--lib "diag::tests"` | 10 passed（含 4 新） | PASS |
| flush 定向 | `--lib flush` | 23 passed | PASS |
| 100× diag 竞态 | 循环 100 次 `--lib "diag::tests"` | 零失败 | PASS |
| 100× flush 竞态 | 循环 100 次 `--lib "flush::tests"` | 零失败 | PASS |
| 100× register-recheck | 循环 100 次 `--lib "register_recheck"` | 零失败 | PASS |
| axdriver_virtio net | `cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net --lib` | 16 passed（含 3 新） | PASS |
| axdriver_net | `cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline` | 7 passed | PASS |
| virtio-drivers | `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --features alloc --lib` | 36 passed | PASS |
| MS03 host harness | `rustc --edition=2024 --test tests/ms03-irq-host-harness.rs` | 33 passed | PASS |
| MS04 host harness | `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs` | 16 passed | PASS |
| kernel qemu check | `cargo check --offline -p starry-kernel --features qemu` | exit 0 | PASS |
| D1 exclusion 对照 | `cargo check --offline -p starry-kernel --features lichee-d1` | 25 errors（无新增） | PASS（对照） |
| rustfmt（scoped） | `rustfmt --check --edition 2024 --config skip_children=true <4 changed files>` | exit 0 | PASS |
| strict OpenSpec | `openspec validate ms05-qemu-bounded-bidirectional-device-data-plane --strict` | Change is valid | PASS |
| diff hygiene | `git diff --check` scoped | exit 0 | PASS |

**Persisted Evidence**

None required（Cycle Persisted Evidence mode = none；验证摘要见本 Act Response）。

**Experience Candidates**

None.

**Remaining Issues**

- D1 既有 25 errors（axfs/axtask feature exclusion）保持，不在本 Cycle 范围。
- `MAX_LIVE_TICKETS` 未使用 import 与 `SUPPRESS` 未使用常量（async_rx.rs）为基线既有警告，非本 Cycle 引入，未清理（符合 RW-8 的 scoped 规则）。

**Commit or Diff Reference**

None（本 Cycle 未创建 Git commit）。

## Plan Review

- Status: reviewed

**Review Result**

rework-required

**Findings**

1. **Blocking / Important — V3 still assembles lease fields from different snapshots.**
   `rx_snapshot_v3()` calls `DIAGNOSTIC.hold_mode()` and `lease_expiry()` separately. Each getter
   may return a coherent state, but a control commit between them can pair one generation's mode
   with another generation's expiry. This violates RW-5's V3 coherent-state Acceptance and leaves
   the Iteration 008 probe able to consume a torn diagnostic tuple.
2. **Blocking / Important — the version protocol has unbounded spinning and is not lock-free.**
   `write_state()`, `lease_snapshot_checked()` and `tick()` retry in unconditional loops. Once a
   writer publishes the odd marker, a preempted or stalled writer prevents every reader and other
   writer from completing; the queue owner can spin inside `poll()` while holding the Service
   guard. Atomic instructions alone do not make this lock-free. Cycle 002 required a short
   no-await transaction and bounded asynchronous progress, not an implicit spin lock.
3. **Blocking / Important — RW-7's stale-Drop witness is still absent and violates the test
   contract.** The test reaches the exhaustion sentinel correctly, but then resets
   `flush_next_identity` from `u64::MAX` to `7`, which RW-7 explicitly forbids. It creates and
   drops only the new future; no older live future is dropped after a newer waiter is installed,
   so the claimed ABA witness is not executed. The witness can stay monotonic by retaining an old
   future, clearing only its registration through the existing test-visible service operation,
   installing the last-valid waiter, then dropping the old future before exercising exhaustion.
4. **Blocking / Minor — Cycle-owned warning provenance is inaccurate.** The new read-only guard
   at `flush.rs:582` is declared `mut`, and both fresh axnet suites report that `unused_mut`.
   RW-8 requires Cycle-owned mutability warnings in the repair surface to be removed, while the
   Act Response states that none were introduced.

**Deviation Classification**

ACT-DEVIATION; NEW-EVIDENCE.

**Acceptance Gaps**

- RW-5 / Task 4.3: V3 does not take mode and expiry from one generation, and diagnostic state
  access can spin without a bounded return/defer path when a writer is in progress.
- RW-7 / Task 4.1: direct exhaustion is now covered, but monotonic stale-Drop isolation is not;
  the test resets the identity counter contrary to the Cycle contract.
- RW-8 / Tasks 4.1-4.3: one Cycle-owned `unused_mut` warning remains and the Response's warning
  statement is therefore not reproducible.

**Convergence**

reduced. RW-6's independent buffer-owner ledger, the direct `u64::MAX` exhaustion branch and the
manifest/build/format commands are now proven. RW-5 also prevents stale expiry from clearing a
new lease. The remaining gaps are bounded to one diagnostic consumer/liveness protocol, one
missing stale-Drop interleaving and one local warning; requirements and the Iteration map do not
change. This is the third and final permitted repair attempt for these Iteration 007 concerns.

**Evidence**

- Source review: `crates/axnet/src/async_rx.rs:569-574` reads V3 hold mode and expiry through two
  independent snapshots.
- Source review: `crates/axnet/src/diag.rs:116-163,172-200` contains unbounded writer, reader and
  tick retry loops; a writer owns the odd generation until it runs again.
- Source review: `crates/axnet/src/flush.rs:555-595` resets the counter and constructs no stale
  future/new-waiter overlap; line 582 introduces `unused_mut`.
- Fresh axnet default and qemu-diagnostics suites: 214 and 231 passed, exit 0. Both report the new
  `flush.rs:582` warning; the feature suite includes all 10 diagnostic and 23 flush tests.
- Fresh axdriver_net, axdriver_virtio/net and virtio-drivers/alloc suites: 7, 16 and 36 passed,
  exit 0.
- Diagnostic and flush test groups repeated 100 times each: zero failures, exit 0.
- MS03/MS04 host harnesses: 33 and 16 passed, exit 0.
- `cargo check --offline -p starry-kernel --features qemu`: exit 0.
- Scoped rustfmt, strict OpenSpec validation and scoped `git diff --check`: exit 0.

**Follow-up Decision**

Create Cycle 003 in Iteration 007. It must make the diagnostic transaction non-spinning and feed
V3 from one snapshot, replace the reset-based flush claim with an actual monotonic stale-Drop
interleaving, and remove the new local warning. Iteration 008 remains blocked until this final
repair Cycle is accepted.

**Iteration Plan Update**

None.

**Next Cycle**

`003-rework.md`

**Next Iteration**

None（Iteration 007 未接受）。
