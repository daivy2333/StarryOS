# Iteration 008 / Cycle 000: Service-Owned Diagnostic Lease

## Plan Context

- Status: ready
- Iteration: 008-service-owned-diagnostic-lease
- Cycle: 000-initial
- Cycle Type: initial
- Parent cycle: none

**Iteration Scope**

- Change task: 4.4
- Depends on: Iteration 006 stable queue owner; Iteration 007 `replan-required` handoff and its
  accepted flush/V3 ABI plumbing
- Trigger: user-approved replan on 2026-08-15
- Stable result: QEMU diagnostic lease is a committed part of `Service`; control is bounded, V3
  is coherent, expiry cannot become permanently held, and stale timers cannot clear replacements.
- Deferred tasks: 5.1-5.2 and 6.1-6.3

Iteration 007 remains historical. Its accepted target-scoped flush, stale-Drop witness, V3 wire
layout and ledger plumbing are inputs, but its independent atomic/version lease protocol is not an
accepted baseline. This is a new logical design boundary, not a fourth repair Cycle.

**Objective**

Replace the rejected global diagnostic transaction with one Service-owned lease state while
preserving all external commands and layouts. Every successful control or snapshot must refer to
real committed state; every acquisition path must be bounded; explicit Release and expiry must
work for every reachable Hold; timer wakeups must never own state.

**Current Baseline**

- Branch `net-k3`; review HEAD `223f6281d62b6925fa3f830690945dccab424022` with staged prior
  implementation and Cycle documents plus the Cycle 003 Review.
- Fresh Review baseline: axnet default 214 and qemu-diagnostics 234 tests pass; axdriver_net 7,
  axdriver_virtio/net 16 and virtio-drivers/alloc 36 pass; diagnostic and flush groups pass 100×;
  MS03/MS04 harnesses pass 33/16; kernel QEMU check, rustfmt, strict OpenSpec and diff check exit 0.
- D1 is an expected exit-101 comparison with the established 25 axfs/axtask errors, never a PASS.
- Green suites do not cover committed V3 state or terminal active-Hold liveness. Kernel QEMU also
  reports a Cycle-owned orphan `lease_expiry` warning.

**Current-State Evidence**

- `diag.rs::lease_snapshot_checked` maps an unsuccessful coherent read to
  `{observed_generation, HOLD_NONE, 0}`; no committed state necessarily contains that tuple.
- `rx_snapshot_v3` publishes the fallback without a Busy marker, so probe logic could mistake
  contention for RELEASED/POST.
- a Hold committed at generation `u64::MAX - 1` can be cleared by neither explicit Release nor
  expiry tick.
- `Service` already serializes Router, flush and the V3 slot/ticket/driver ledger. V3 already
  acquires that guard, so copying lease fields under it adds no second lock acquisition.
- the queue future's timer only needs to wake the owner. Correctness can be decided by the current
  Service lease at poll time rather than a generation stored in the timer.
- `axsync::Mutex` provides a real `try_lock`; the current `ServiceAccess::try_lock` name does not
  prove bounded acquisition because its implementation uses blocking `lock`.

**Relevant Code**

| Area | Files and symbols | Responsibility |
|---|---|---|
| Lease owner | `crates/axnet/src/service.rs::Service` | committed mode, expiry, failure counter; guarded tick/snapshot |
| Diagnostic API | `crates/axnet/src/diag.rs`, `lib.rs::diagnostic_control` | validate command, bounded Service acquisition, post-unlock event |
| Queue future | `crates/axnet/src/async_rx.rs::RxRxFuture` | guarded stage hold/tick and wake-only timer arm/rearm |
| V3 snapshot | `crates/axnet/src/async_rx.rs::rx_snapshot_v3` | copy lease and ledgers from one Service guard |
| Syscall mapping | `kernel/src/syscall/fs/ctl.rs` | preserve `ResourceBusy` to `WouldBlock` and ABI |

**Critical Path**

```text
ioctl validates command and checked deadline
  -> one bounded Service try-lock
  -> commit mode/expiry under guard
  -> drop guard
  -> publish queue work
  -> owner polls Service, applies hold and arms current absolute deadline
  -> timer only wakes owner
  -> guarded tick clears the current lease exactly when expired

V3 takes existing Service guard
  -> copies slots/tickets/driver ledger and lease tuple in one round
  -> releases guard
  -> publishes only real committed state
```

**Behavioral Change**

- control contention returns `DevError::ResourceBusy`, mapped to `AxError::WouldBlock`; it changes
  neither state nor event generation. Task 5.1 must later retry only within its fixed deadline.
- Hold/Release commits happen under Service ownership. Deadline overflow is rejected before any
  mutation. Queue-work publication occurs after unlock.
- queue tick clears an expired lease and saturating-increments auto-release failure once. No lease
  generation exists or can exhaust.
- timer carries only a deadline/wake obligation. A stale wake causes at most one bounded poll; the
  current replacement lease decides whether to remain held and which deadline is rearmed.
- V3 retains every byte/offset but obtains the lease tuple and ledger under one Service guard.
  Only pre-init missing Service uses the existing all-zero result.

**Change Surface and RTM**

| Contract | Requirement / Design | Code surface | Test witness |
|---|---|---|---|
| C1 Service-owned state | R15; D9 | `service.rs`, removal/reduction of global `diag.rs` state | injected Service snapshot/control tests |
| C2 bounded control | R15 contention and overflow; D9 | `diagnostic_control`, real Service `try_lock`, syscall mapping | held-lock Busy/no-event and checked-add RED/GREEN |
| C3 guarded expiry | R15 auto-release; D9 | `Service::diag_hold_tick`, queue service round | exact-deadline clear/count-once and no-generation-exhaustion witness |
| C4 wake-only timer | R15 stale timer; D9 | `RxRxFuture` timer fields/poll/rearm | Hold A timer after Hold B replacement cannot clear B |
| C5 committed V3 | R15 concurrent V3; D9/D10 | `rx_snapshot_v3` | control/tick interleaving returns old or new committed tuple, never synthetic |
| C6 compatibility | R6/R14, D9/D10 | V1/V2/V3 structs/ioctls/features | Rust/C ABI canary, default/D1 exclusion checks |

No requirement is Missing or Simplified.

**Task Contract: 4.4**

1. RED ownership and contention:
   - inject a Service whose lease can be read and mutated under the same guard used by the queue;
   - hold the Service lock and call the production-style diagnostic control seam; prove the current
     path blocks or lacks bounded Busy semantics, then require immediate `ResourceBusy`, unchanged
     state and unchanged queue-event generation;
   - force V3/control and V3/tick interleavings; prove the current global fallback can report a
     tuple that is neither the before nor after committed state.
2. RED liveness and time:
   - construct an active Hold at the old protocol's last usable generation and prove Release/tick
     cannot clear it;
   - commit Hold A, replace/release it with Hold B before A's armed deadline, then fire A's timer;
     prove any timer-owned clear would erase B;
   - make `now + lease` overflow and prove no partial mode/expiry mutation is allowed.
3. GREEN owner migration:
   - store lease mode, absolute expiry and counter directly in `Service` under
     `qemu-diagnostics`; eliminate independent lease generation and multi-atomic publication;
   - provide a genuinely nonblocking global Service acquisition for control. Do not reuse a seam
     whose name says `try_lock` while it calls blocking `lock`;
   - validate command and checked deadline, commit under guard, drop guard, then publish exactly
     one queue-work event. Busy/error paths publish none;
   - tick and V3-copy while the existing Service guard is held. Counter uses saturating monotonic
     semantics and expiry is counted exactly once;
   - timer only wakes. After any wake, current Service state determines hold and rearm. No guard,
     raw buffer, descriptor or ticket crosses `Pending`.
4. Cleanup and compatibility:
   - remove the rejected global state/version helpers and all newly orphaned methods/imports;
   - preserve V1/V2/V3 command, size, offset, sentinel and legacy write ranges;
   - preserve `qemu-diagnostics` feature propagation only through kernel QEMU; default axnet and
     D1 expose no control entry;
   - keep ISR free of Service locks and diagnostic state access.

**BDD Scenarios**

- Happy Hold: bounded try-lock succeeds, commits one tuple, unlocks, publishes; owner observes Hold,
  arms exact expiry, then guarded tick clears and counts once at expiry.
- Control contention: Service is busy; ioctl returns WouldBlock with no state or event change;
  later bounded retry may commit.
- Coherent V3: V3 racing a control/tick returns either the complete before tuple or complete after
  tuple together with the guarded ledger, never a synthetic RELEASED tuple.
- Explicit Release: Release clears under Service guard and publishes only after unlock; owner
  resumes the skipped stage.
- Stale timer: A is replaced by B; A's wake cannot clear B, and the next poll keeps/rearms B.
- Overflow: checked deadline calculation fails closed with no partial mutation/event.
- Pre-init: missing Service returns the legacy all-zero snapshot; runtime contention does not use
  that representation.
- Compatibility: V1/V2/V3 layout and QEMU-only scope are unchanged; no SMP/board claim is made.

**Invariants**

- the queue task remains the sole raw RX/TX hardware owner; diagnostic state controls only whether
  the owner executes a stage.
- one Service guard defines the committed lease and the V3 ledger view.
- control acquisition is bounded; no guard crosses `Pending`; ISR never acquires Service.
- timer is wake-only and stale wakeups cannot mutate a newer lease.
- every reachable Hold is releasable explicitly or by expiry; no identity/generation exhaustion
  can make it permanent.
- auto-release telemetry is monotonic and non-wrapping; it is not a synchronization primitive.
- V1/V2/V3 ABI, flush semantics, ticket/buffer/descriptor ownership and ordinary builds remain
  unchanged.

**Non-goals**

- no Task 5.1 probe, stimulus, Makefile target, guest artifact, manual QEMU or persisted Evidence.
- no socket readiness/public flush API, queue/event redesign, ISR expansion, generic mutex change,
  reset, SMP, DWMAC, board or performance claim.
- no fourth Iteration 007 Cycle, global SNAPSHOT/tasks/M-D-K-R-I update, archive, dependency upgrade
  or unrelated warning cleanup.

**Acceptance**

| Contract | Proof | Status |
|---|---|---|
| C1 | lease fields and mutations are Service-owned; rejected global/version protocol is absent | Planned |
| C2 | held Service returns Busy in bounded work with unchanged state/event; overflow is atomic | Planned |
| C3 | explicit and exact-deadline release work for every reachable Hold; counter changes once | Planned |
| C4 | stale A timer cannot clear B; B is retained/rearmed without generation identity | Planned |
| C5 | forced V3 interleavings return only committed before/after tuples under the ledger guard | Planned |
| C6 | ABI canaries, feature exclusion and full regressions pass with no new warning | Planned |

Any blocking control acquisition, synthetic V3 state, permanent Hold, timer-owned clear, guard
across Pending, ABI drift, ISR Service access or new Cycle-owned warning blocks acceptance. If the
Service boundary itself cannot satisfy these contracts without changing wire ABI or queue owner,
stop and return `replan-required`; do not reintroduce versioned global lease state.

**Verification**

Act must record exact command, working directory, key output and exit status for:

```text
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --features qemu-diagnostics --lib diag -- --nocapture
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --features qemu-diagnostics --lib service -- --nocapture
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --features qemu-diagnostics --lib async_rx -- --nocapture
repeat the control-contention, committed-V3, expiry and stale-timer tests 100 times
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --features qemu-diagnostics --lib
cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline
cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net
cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --features alloc
rustc --edition=2024 --test tests/ms03-irq-host-harness.rs -o /tmp/ms03-irq-host-test && /tmp/ms03-irq-host-test
rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test
cargo check --offline -p starry-kernel --features qemu
cargo check --offline -p starry-kernel --features lichee-d1
rustfmt --check --edition 2024 --config skip_children=true <all Iteration-owned changed Rust files>
openspec validate ms05-qemu-bounded-bidirectional-device-data-plane --strict
git diff --check -- crates/axnet kernel tests openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane
```

D1 must remain exit 101 with exactly the established 25 errors. Act must additionally perform a
source/diff review proving V3 field offsets, QEMU-only feature propagation, post-unlock event
publication, absence of independent diagnostic generation/global state, and absence of Service
access in the IRQ path.

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Authorization | PASS | user approved replan on 2026-08-15; no implementation/runtime authority inferred |
| Investigation | PASS | global state, Service owner, control/syscall, queue timer, V3 guard and mutex try-lock are located |
| Design | PASS | Service-owned state, bounded control, guarded snapshot/tick and wake-only timer close all Review gaps |
| Iteration Plan | PASS | Task 4.4 is isolated in new Iteration 008; probe/manual work is deferred to 009/010 |
| Task Contract | PASS | RED/GREEN, invariants, prohibitions, stop conditions and exact verification are explicit |
| Traceability | PASS | all diagnostic lease scenarios map to D9, Task 4.4, code surface and test witnesses |
| Verification | PASS | deterministic interleavings, 100× repeats, ABI/build/driver/kernel and diff Gates cover the boundary |

**Persisted Evidence**

- Mode: none

The Act Response carries Iteration 008 evidence. Do not create an Evidence directory.

**Risks and Notes**

- `try_lock` fairness is not required; bounded failure is the contract. Task 5.1 owns fixed-deadline
  retries and cannot reinterpret contention as RELEASED.
- a stale timer may cause one extra bounded poll; this is acceptable because it neither clears
  state nor creates periodic polling.
- host/QEMU checks do not prove SMP scheduler fairness, real-board timing, DMA/cache or performance.

## Act Response

- Status: reported

**Implemented**

- C1 Service-owned lease: the rejected global multi-atomic/version protocol
  (`DiagnosticState` + `DIAGNOSTIC` + generation CAS) is removed. The committed
  hold mode, absolute expiry and auto-release failure counter now live directly
  in `crate::axnet::service::Service` under `qemu-diagnostics`. No lease
  generation exists, so no identity can exhaust and every reachable Hold is
  releasable explicitly or by expiry.
- C2 bounded control: `lib.rs::diagnostic_control` acquires the Service with the
  genuine nonblocking `axsync::Mutex::try_lock` (not a misnamed blocking seam).
  While the Service is busy it returns `DevError::ResourceBusy` (mapped to
  `WouldBlock`) with unchanged state and no event publication. Command and
  deadline are validated with `checked_mul`/`checked_add` before any mutation;
  an overflowing deadline fails closed with `InvalidParam` and no partial
  mode/expiry change. On success the lease commits under the guard, the guard is
  dropped, then exactly one `QUEUE_EVENT.publish_queue_work()` fires.
- C3 guarded expiry: `Service::diag_hold_tick()` advances the Service-owned
  lease once per round under the existing Service guard, clears an expired
  lease and saturating-increments `auto_release_failure` exactly once. The
  queue tick no longer publishes queue work: the wake-only timer is the wake
  source (C4).
- C4 wake-only timer: `RxRxFuture.lease_deadline` is `Option<u64>` (deadline
  only, no generation). `arm_lease_deadline`/`lease_deadline_elapsed` carry no
  generation identity; a stale wake costs at most one bounded poll and the
  current Service lease decides whether to remain held and which deadline is
  rearmed.
- C5 committed V3: `rx_snapshot_v3` copies the lease tuple (mode, expiry,
  failure counter) and the slot/ticket/flush/drop ledger under ONE Service
  guard, so contention can never be encoded as a synthetic no-hold tuple. Only
  a missing (pre-init) Service uses the all-zero fallback.
- C6 compatibility: V1/V2/V3 wire structs, ioctl commands, offsets and the
  `qemu-diagnostics` feature boundary are unchanged; default axnet and D1
  builds expose no control entry and carry no diagnostic fields.
- Naming: the blocking acquisition previously named `try_lock` is renamed
  `ServiceAccess::lock`; the genuinely nonblocking acquisition used by the
  control path is the axsync mutex `try_lock`, so no seam whose name says
  `try_lock` calls blocking `lock`.

**Changed Files and Symbols**

- `crates/axnet/src/diag.rs`：`DiagnosticState`、`DIAGNOSTIC`、generation
  protocol、`lease_snapshot*`、`tick`/`claim_and_clear`/`write_state` 及全部
  RW-5/RW-9/RW-10 test seams 移除；保留常量、`diag_now`/`set_test_now`/`TEST_NOW`。
- `crates/axnet/src/service.rs`：`Service` 新增 `diag_hold_mode`/
  `diag_lease_expiry_nanos`/`diag_auto_release_failure`；新增 `diag_control`
  （checked commit）、无参 `diag_hold_tick`、`diag_hold_mode`/
  `diag_lease_expiry`/`diag_auto_release_failure` 访问器；tests 新增
  `mod diag`（8 个 Service-owned lease 契约测试，`serialized_service` 持
  SERIAL + 固定 fake clock）。
- `crates/axnet/src/async_rx.rs`：`RxRxFuture` 移除 `diag` 字段；
  `lease_deadline: Option<u64>`；`SleepUntil(u64)`；`arm_lease_deadline`/
  `lease_deadline_elapsed` 无 generation；`service_round` 调 `diag_hold_tick()`
  并在 guard 内读 `diag_lease_expiry`；`rx_snapshot_v3` 单 guard 复制；
  `ServiceAccess::lock`（阻塞，改名）供 queue task/flush；tests 改写 RW-1
  hold 组为 Service-owned（SERIAL + 固定时钟），新增 C5 committed-tuple
  interleaving 测试，删除 `leaked_diag`/`leaked_future_diag`。
- `crates/axnet/src/lib.rs`：`diagnostic_control` 改为单次非阻塞
  `SERVICE.get()?.try_lock()` + guard 内 `diag_control` + 解锁后单次 event。
- `crates/axnet/src/flush.rs`：`flush_new`/`poll_impl`/`Drop` 的
  `service.try_lock()` → `service.lock()`（阻塞语义，避免 lost-wakeup）。

**Deviations from Plan**

- `ServiceAccess::try_lock` 被保留为阻塞语义并改名 `lock()`，真实非阻塞获取
  由 control 路径直接使用 axsync `Mutex::try_lock()`。Plan 要求 "provide a
  genuinely nonblocking global Service acquisition for control. Do not reuse a
  seam whose name says `try_lock` while it calls blocking `lock`"——实现满足
  该语义：control 不再使用阻塞 seam，且阻塞 seam 不再伪装为 `try_lock`。
  queue task 与 flush 保持阻塞获取以避免 Service 短暂被 stack 持有时
  lost-wakeup（`poll_impl` 的 None 分支不注册 waker，非阻塞会静默延迟
  flush 完成）。
- Iteration 008 原 RED 测试（`active_hold_at_terminal_generation_*`、
  `control_deadline_overflow_is_rejected_atomically`、V3 contention）在旧
  全局协议上运行并观察 RED 后，随 `DiagnosticState` 移除而删除；其契约由
  新 Service-owned 测试（`any_reachable_hold_is_releasable_or_expirable`、
  `control_deadline_overflow_fails_closed_atomically`、
  `v3_lease_tuple_is_committed_service_state_under_interleavings`）等价覆盖。

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: PASS
- Full diff reviewed: PASS
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 1

全量 diff review（async_rx.rs + service.rs + diag.rs + lib.rs + flush.rs，相对
HEAD 530 增 / 351 删）：Service 单 guard 同时序列化 Router、flush、V3 账本与
lease（C1/C5）；`diagnostic_control` 的 busy/error 路径均不发布 event，成功
路径 drop guard 后恰好发布一次（C2）；`diag_hold_tick` 只读 Service 自身
状态、saturating 计数恰好一次（C3）；`lease_deadline_elapsed` 无条件自醒且
无 generation 比对，stale wake 由当前 Service lease 在 poll 时决定（C4）；
`rx_snapshot_v3` 的 lease 与账本来自同一 guard（C5）。跨任务交互无遗漏实现、
无计划外修改；ISR 路径（kernel）零修改、零 Service 访问。

遗留 Minor（不阻塞，未伪装为已解决）：

1. `flush.rs` 3 处 `unused_mut`（pre-existing，Iteration 007 记录于
   003-rework Remaining Issues；Plan 禁止清理）。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| RED terminal Hold | `cargo test --features qemu-diagnostics --lib terminal_generation` | `active_hold_at_terminal_generation_cannot_be_released_or_expired`/`..._cannot_expire` FAILED（Release 返回 ResourceBusy；tick 无法清除） | RED 确认 |
| RED overflow | `cargo test --features qemu-diagnostics --lib control_deadline_overflow` | `attempt to add with overflow` panic | RED 确认 |
| RED V3 synthetic | `cargo test --features qemu-diagnostics --lib v3_snapshot_contention` | `left: 0 right: 0` assertion failed（synthetic HOLD_NONE 被报告） | RED 确认 |
| axnet full (feature) | `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --features qemu-diagnostics --lib` | `231 passed; 0 failed` | PASS |
| axnet full (default) | `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib` | `214 passed; 0 failed` | PASS |
| diag 组 | `cargo test ... --lib diag` | `9 passed` | PASS |
| service 组 | `cargo test ... --lib service` | `32 passed` | PASS |
| async_rx 组 | `cargo test ... --lib async_rx` | `80 passed` | PASS |
| 竞态 100× | diag/flush/service::tests::diag/async_rx/hold/v3/stale-timer 各 100 次循环 | 全部 `test result: ok`，零失败 | PASS |
| axdriver_net | `cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline` | `7 passed` | PASS |
| axdriver_virtio | `cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net` | `16 passed` | PASS |
| virtio-drivers | `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --features alloc` | `36 passed` | PASS |
| MS03 harness | `rustc --edition=2024 --test tests/ms03-irq-host-harness.rs -o /tmp/ms03-irq-host-test && /tmp/ms03-irq-host-test` | `33 passed` | PASS |
| MS04 harness | `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test` | `16 passed` | PASS |
| kernel QEMU | `cargo check --offline -p starry-kernel --features qemu` | exit 0；本 Cycle 文件无新增 warning（diag.rs AtomicU64 unused 已修复） | PASS |
| kernel D1 | `cargo check --offline -p starry-kernel --features lichee-d1` | exit 101，恰为既有 25 个 axfs/axtask 错误，非本 Cycle 引入 | PASS（预期比较） |
| rustfmt | `rustfmt --check --edition 2024 --config skip_children=true crates/axnet/src/{diag,service,async_rx,lib,flush}.rs` | exit 0 | PASS |
| OpenSpec strict | `openspec validate ms05-qemu-bounded-bidirectional-device-data-plane --strict` | `Change ... is valid` | PASS |
| diff check | `git diff --check -- crates/axnet kernel tests openspec/changes/ms05-...` | exit 0 | PASS |

**Persisted Evidence**

None required（Iteration 008 Persisted Evidence 模式为 `none`；验证摘要由本
Act Response 承载，未创建 Evidence 目录）。

**Experience Candidates**

| Type | Candidate | Evidence | Reason |
|---|---|---|---|
| Incident | 全局多原子/version lease 协议在 generation `u64::MAX - 1` 处使 active Hold 永久无法 Release/expiry，且 read contention 时 V3 报告 synthetic no-hold | 本 Act Response + Iteration 008 Plan Context（Cycle 003 Review 的 BLOCKING 发现 1/2） | 该故障模式在 3 次修复 Cycle 内无法在同一全局协议中收敛（replan-required），最终以 Service-owned 设计消除；具有系统性诊断价值（generation 耗尽 + 无 committed-state 表示） |

**Remaining Issues**

- 既有 `flush.rs` 3 处 `unused_mut`、`MAX_LIVE_TICKETS`/`SUPPRESS` unused
  warning 与 smoltcp warnings：按 Iteration 007 Plan 记录禁止清理，留待
  后续非本 Cycle 范围处理。

**Commit or Diff Reference**

未创建 commit。Iteration 008 工作基于 HEAD `223f6281`；代码改动在
`crates/axnet/src/{diag,service,async_rx,lib,flush}.rs`，Cycle 文档为
`iterations/008-service-owned-diagnostic-lease/000-initial.md`。

## Plan Review

- Status: reviewed

**Review Result**

rework-required

**Findings**

1. BLOCKING — C2 的生产控制路径没有测试见证。`lib.rs::diagnostic_control` 的代码使用
   `SERVICE.get()?.try_lock()`，并在成功提交和释放 guard 后发布一次 queue event；独立
   source review 未发现阻塞获取或错误路径发布事件。但仓库内没有测试调用该函数或与其
   共用的生产控制 seam。现有 Service tests 只直接调用 `Service::diag_control`，不能证明
   Service contention 立即返回 `ResourceBusy`、状态和 queue generation 不变，也不能证明
   成功路径只在解锁后发布一次。Plan Context 明确要求 held-lock Busy/no-event GREEN witness
   和 100× control-contention 重复，因此 C2 的 Gate 3/Acceptance 证据未闭合。
2. BLOCKING — C5 的测试没有执行 V3 snapshot 路径。
   `v3_lease_tuple_is_committed_service_state_under_interleavings` 持有注入 Service guard 后直接
   调用 `diag_hold_mode`、`diag_lease_expiry` 和 `diag_auto_release_failure`；它既不调用
   `rx_snapshot_v3`，也不调用该函数共用的 snapshot assembly seam。测试名称中的
   “interleavings” 实际是同一 guard 内的顺序读写，无法在旧 synthetic fallback 或跨 guard
   读取重新出现时失败。`rx_snapshot_v3` 的当前代码确实在一个 Service guard 内复制 lease
   tuple 与 ledger，但 C5 明确要求 forced V3/control 和 V3/tick interleaving witness，source
   review 不能替代该测试契约。
3. NON-BLOCKING — `flush.rs` 三处 `unused_mut`、`MAX_LIVE_TICKETS`/`SUPPRESS` 和 smoltcp/
   virtio warning 均已在前序 Cycle 或依赖基线中存在。Iteration 008 没有引入新的 warning；
   本轮不得把无关清理混入返工。

**Deviation Classification**

ACT-DEVIATION — 实现保留了计划设计，但 Act 没有建立 C2/C5 Task Contract 要求的生产路径
测试见证，并把直接 Service getter 测试报告为 committed V3 interleaving 证据。

**Acceptance Gaps**

- T4.4-R1 / C2：缺少共用生产控制路径的 held-Service contention、错误不发布、成功解锁后
  单次发布测试，以及对应 100× 重复证据。
- T4.4-R2 / C5：缺少共用生产 V3 assembly 路径的 control/tick interleaving 测试；现有测试
  不能证明 snapshot 只返回 before/after committed tuple 而不返回 synthetic tuple。

C1、C3、C4、C6 的代码、测试和新鲜回归证据没有 Acceptance gap。

**Convergence**

N/A（initial Cycle）。两个 gap 都是既有 Task 4.4 Acceptance 的测试闭合问题，不改变
Service-owned 设计、wire ABI、queue ownership、Iteration 目标或后续依赖。

**Evidence**

- Source review：`crates/axnet/src/lib.rs::diagnostic_control`、
  `service.rs::{diag_control,diag_hold_tick}`、`async_rx.rs::{rx_snapshot_v3,RxRxFuture}`、
  `kernel/src/syscall/fs/ctl.rs`；未发现独立 `DiagnosticState`/`DIAGNOSTIC`/lease generation，
  IRQ 文件未访问 Service 或 diagnostic control。
- Test inventory：`rg "diagnostic_control\\("` 只返回生产函数和 kernel caller；
  `rx_snapshot_v3()` 只由 kernel mapping 调用。现有 C5 test 只读取 Service getters。
- Fresh review：axnet qemu-diagnostics `231 passed`、default `214 passed`；driver suites
  `7/16/36 passed`；MS03/MS04 harness `33/16 passed`；kernel QEMU check exit 0；D1 comparison
  exit 101 且恰为既有 25 个 axfs/axtask errors；rustfmt、strict OpenSpec 和 diff check exit 0。
- Persisted Evidence 模式为 `none`；Evidence 目录不存在符合 Plan，不构成 finding。

**Follow-up Decision**

保留当前实现设计，在 Iteration 008 内创建一个 rework Cycle，仅补齐 C2/C5 的生产路径测试
seam、确定性 interleaving witness 和重复 Gate。不得开始 Task 5.1、创建 runtime Evidence、
修改 wire ABI 或清理既有 warning。

**Iteration Plan Update**

None。Task 4.4 仍属于 Iteration 008，Iteration 009/010 的任务和依赖保持不变。

**Next Cycle**

`001-rework.md`

**Next Iteration**

None（Iteration 008 尚未 accepted）。
