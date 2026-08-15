# Iteration 007 / Cycle 001: Flush and Diagnostic Correctness Rework

## Plan Context

- Status: ready
- Iteration: 007-ticketed-flush-and-v3-diagnostics
- Cycle: 001-rework
- Cycle Type: rework
- Parent cycle: 000-initial

**Iteration Scope**

- Change tasks: 4.1, 4.2, 4.3
- Depends on: Cycle 000 implementation and `rework-required` Plan Review
- Stable baseline: lease 到期由 executor deadline 主动唤醒且不 busy loop；V3 反映真实
  buffer/descriptor ownership；terminal fault 与 waiter identity 可稳定审计；全部自动 Gate 包括
  rustfmt 退出 0。
- Verification boundary: fake-clock scheduler tests、hold liveness tests、真实 adapter ledger model、
  flush post-fault/identity exhaustion、V3 ABI/consumer、axnet full、QEMU/D1 feature boundary、fmt、
  strict OpenSpec 和 scoped diff review。
- Diagnostic boundary: 失败分别定位到 lease scheduler、driver resource snapshot、flush stable state
  或验证 hygiene，不进入 probe/runtime orchestration。
- Deferred tasks: 5.1-5.2、6.1-6.3

**Cycle Scope**

- Trigger: Cycle 000 Review Result `rework-required`
- Acceptance gaps: deadline-less lease；synthetic buffer/descriptor ledger；non-stable flush fault 与
  wrapping waiter identity；rustfmt Gate 失败。
- Repair items: RW-1 through RW-4。
- Inherited scope: Cycle 000 的 Tasks 4.1-4.3、R3-R6/R14、D8-D10、commands、V3 field order、
  2 秒上限、QEMU-only feature 与 V1/V2 compatibility。
- Excluded scope: Task 5.1 probe、Evidence 008、手工 QEMU、socket readiness、reset/SMP、真板与
  性能优化。

**Objective**

关闭 Cycle 000 Review 的四项验收缺口，使 bounded controls 在无外部 NIC 事件时仍按 deadline
自动恢复，使 V3 足以证明真实 driver 资源守恒，并让 flush 在 terminal fault 与 identity exhaustion
后保持确定结果；随后重跑完整自动 Gate。

**Current Baseline**

- Branch: `net-k3`
- HEAD: `e1fde918849111b47d96f6e91402a4ef96147a63`
- Worktree: modified；Iterations 005-007 的实现和 OpenSpec 修改尚未提交。
- Tasks 4.1-4.3 已由 Act 勾选，但 Cycle 000 Review 为 `rework-required`，不能进入 Task 5.1。
- Fresh passing checks：axnet default 208/208、feature 216/216、flush 20/20、diag 6/6、MS03
  33/33、MS04 16/16、register-recheck 100/100、kernel QEMU check、strict OpenSpec 和 diff check。
- Fresh failing check：changed Rust `rustfmt --check` exit 1，失败文件为
  `tests/ms03-irq-host-harness.rs`。
- D1 exclusion 对照仍为既有 25 errors，exit 101；不得记录为 PASS。

**Repair Items**

### RW-1 — Deadline-driven lease without busy waiting（Task 4.3）

- Gap: `tick()` 依赖下一次 queue poll；submit hold 可永久睡眠，reclaim hold 可因 visible completion
  自唤醒忙转。
- RED: 使用 fake clock + counting waker 覆盖 hold 后无外部事件、deadline 前不醒、deadline 到点
  主动醒、恰好一次 auto-release failure、release 后 stage 恢复；另覆盖 held reclaim + visible TX
  completion 在 deadline 前不形成重复 self-wake。
- GREEN: 把 lease deadline 纳入唯一 queue future 的 timer/wake decision；timer 只负责唤醒 owner，
  状态 release、failure counter 与 queue-work publication 仍由唯一 owner/受控 seam 提交。显式
  Release 取消或失效旧 deadline，不能让旧 timer 释放新 lease。
- Must not: 第二 executor、sleep loop、periodic polling、raw ring mutation、超过 2 秒 hold，或让
  stale deadline 操作新 generation 的 lease。
- Stop: 若现有 future 无法在不引入第二 owner 的情况下组合 timer，返回 Plan 调整设计。

### RW-2 — Real buffer/descriptor conservation ledger（Tasks 4.2, 4.3）

- Gap: V3 四个资源字段镜像 ticket backing，`ownership_invariant` 恒 0，completion/reclaim 同源。
- RED: adapter/device model 分别制造 buffer exhausted、descriptor `Again`、completion observed、
  successful reclaim、unknown/duplicate cookie 和 conservation mismatch；断言四个资源字段来自实际
  driver ledger，且 completion、reclaim、ownership fault 可独立变化。
- GREEN: 通过 transport-neutral typed snapshot/step metadata 将实际 buffer 与 descriptor
  available/inflight 带到 axnet；V3 单次 Service snapshot 映射这些值。`available + inflight` 必须
  等于各自固定容量，真实 `Again` 时对应资源 available 为 0；owner drift 增加
  `ownership_invariant` 并保持 terminal fault。
- Must not: 暴露 descriptor token/ring index/MMIO、直接读取 VirtIO raw ring、把 ticket capacity
  伪装成 driver capacity，或破坏非 VirtIO implementor。
- Stop: 若公共 queue contract 无法 transport-neutral 地表达只读守恒账本，返回 Plan 重新调查
  D1/DWMAC 映射，不得硬编码 QEMU VirtIO 容量到 axnet。

### RW-3 — Stable flush fault and checked waiter identity（Task 4.1）

- Gap: fatal 仅写当前 waiter；后建 flush 可能等待已停止 owner；identity wrap 可产生 ABA。
- RED: fatal-before-flush、fatal-with-waiter 后再次 flush、fault 后仍 live target、waiter identity
  `u64` exhaustion、旧 future Drop 不清新 waiter。所有路径必须确定 Ready/error，不依赖 2 秒
  ioctl timeout。
- GREEN: Service 持久化 terminal data-plane error，flush constructor/recheck 对当前及后续调用
  返回同一 stable error；waiter-local waker 只负责通知。identity 使用 checked allocation，耗尽时
  返回 stable error且不占 waiter slot，不得 wrap。
- Must not: fatal 后恢复 polling owner、cancel packet、动态 waiter list、清空 live tickets，或把
  timeout 当 terminal error 的替代。
- Stop: 若 stable fault 与既有 lifecycle fault source 无法保持单一权威，返回 Plan 收敛状态模型。

### RW-4 — Verification integrity and formatting（Tasks 4.1-4.3）

- Gap: Act 报告 FMT_OK，但 fresh changed-file check 失败。
- RED/GREEN: 先复现 `tests/ms03-irq-host-harness.rs` 的 rustfmt diff，再格式化所有本 Cycle changed
  Rust files；最终命令必须真实 exit 0，Act Response 记录命令、exit 和摘要，不得只写 marker。
- Must not: 修改 ABI assertions 的语义、格式化无关历史文件，或用缩小范围隐藏失败。

**Critical Path**

```text
typed driver resource ledger
  → Ethernet/Service single snapshot
  → V3 independent counters and conservation tests

control commits lease generation + deadline
  → unique queue future registers deadline wake
  → owner polls at expiry and auto-releases once
  → held stage resumes without busy loop

terminal queue fault
  → persist stable Service error
  → wake current flush waiter
  → current and later flush return same error
```

**Invariants**

- 唯一 queue task 仍是 RX/TX hardware queue owner；timer 只能唤醒，不能推进 descriptor。
- Hold 只暂停一个 stage；Release/expiry 不改 slot、ticket、ring 或 completion ownership。
- buffer、descriptor、ticket 是三套不同账本，不能互相冒充；V3 optional sentinel 仍为
  `u64::MAX`。
- terminal fault 先稳定提交再唤醒；future cancellation 不改变 packet owner。
- V1/V2 command、size、offset、write length 和 MS04 consumer 保持不变。
- QEMU 自动证据不能外推到真板、SMP、DMA/cache、PHY、时序或性能。

**Acceptance**

| Repair | Requirement | Proof | Status |
|---|---|---|---|
| RW-1 | R6/R14，D9 | fake-clock deadline wake、stale lease、no-busy-loop tests | Planned |
| RW-2 | R5/R6/R14，D9-D10 | real adapter ledger、Full/conservation/fault tests、V3 mapping | Planned |
| RW-3 | R3/R4，D8 | pre/post-fault flush、identity exhaustion/ABA tests | Planned |
| RW-4 | Gate 4/5 | full changed-file rustfmt exit 0 + truthful Act record | Planned |

没有新增 Missing 或 Simplified requirement。Cycle 000 的 V1/V2 ABI、target-scoped predicate、
feature graph 和成功 Gate 必须继续回归。

**Verification**

Act 至少执行并记录：

```text
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib flush -- --nocapture
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --features qemu-diagnostics --lib diagnostic -- --nocapture
repeat flush register/recheck and lease deadline/no-busy-loop tests 100 times with zero failures
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --features qemu-diagnostics --lib
run affected axdriver_net, axdriver_virtio and virtio-drivers ledger tests/checks
rustc --edition=2024 --test tests/ms03-irq-host-harness.rs -o /tmp/ms03-irq-host-test && /tmp/ms03-irq-host-test
rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test
cargo check --offline -p starry-kernel --features qemu
cargo check --offline -p starry-kernel --features lichee-d1
rustfmt --check --edition 2024 <all changed Rust files>
openspec validate ms05-qemu-bounded-bidirectional-device-data-plane --strict
git diff --check -- crates/axdriver_net crates/axdriver_virtio crates/virtio-drivers crates/axnet kernel tests openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane
```

D1 当前预期仍 exit 101/25 errors，只作为错误集合与 feature exclusion 对照。任何新增 D1 error、
QEMU compile error、V1/V2 canary failure、resource ledger assertion、race/liveness failure、fmt 非零或
diff/validation failure都停止本 Cycle。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | Cycle 000 diff 与 fresh Gate 已定位四个确定 gap |
| Design | PASS | timer 只 wake owner、typed real ledger、stable Service fault、checked identity 均不改变既有 ownership |
| Cycle Scope | PASS | repair items 全部映射回 Tasks 4.1-4.3，不进入 probe/runtime |
| Task Contracts | PASS | 每项包含 RED/GREEN、禁止项、停止条件与验证 |
| Traceability | PASS | Findings → RW-1..4 → R3-R6/R14、D8-D10 → tests/Gates |
| Verification | PASS | 自动命令可独立证明修复；D1 失败边界明确 |

**Persisted Evidence**

- Mode: none

本返工 Cycle 只在 Act Response 和 Plan Review 中记录自动验证摘要；不得创建
`evidence/007-ticketed-flush-and-v3-diagnostics/001-rework/`。

**Non-goals**

- 不实现 `tests/ms05_data_plane_probe.c`、stimulus、Makefile probe target 或 Task 5.1。
- 不采集 runtime PASS，不手工运行 QEMU，不修复 D1 既有 25 errors。
- 不新增公共 socket flush、准确 `POLLOUT/EAGAIN`、reset/cancel、SMP 或真板支持。
- 不刷新 SNAPSHOT、不修改 M/D/K/R/I、不归档 change。

**Risks and Notes**

- timer future 必须按 lease generation 防 stale expiry；只比较 wall time 不足以区分被替换的 lease。
- real ledger 是只读 transport-neutral observation，不得演变为 axnet 操纵 descriptor 的能力。
- `u64::MAX` 已用于 V3 optional sentinel；waiter identity exhaustion 应显式失败，不能借 sentinel
  表示有效 identity。

## Act Response

- Status: reported

**Implemented**

RW-1（deadline-driven lease，无 busy wait）：

- `crates/axnet/src/diag.rs`：新增可注入时钟 `diag_now()`（生产用 `wall_time_nanos`，test 用 `TEST_NOW` 覆盖）+ `set_test_now`；`NS_PER_MS` 改 `pub(crate)`。
- `RxRxFuture` 持有 `diag: &'static DiagnosticState` 引用（生产传全局 `&DIAGNOSTIC`，测试注入各自独立实例，消除跨测试全局 hold 污染）+ `lease_deadline: Option<u64>` + 生产态 `lease_timer: Pin<Box<dyn Future>>`。
- `service_round` 调度新增 `SleepUntil(deadline)`：hold 激活时既不 self-wake（busy loop）也不走 register/arm/recheck（对 held completion 无限重试），只睡到 lease deadline。
- `arm_lease_deadline`/`cancel_lease_deadline`/`lease_deadline_elapsed`：deadline 到点由 executor 主动 poll 触发（test 由 fake clock 驱动），stale deadline 失效（显式 Release 取消旧 timer，旧 timer 不能释放新 lease）；timer 只唤醒 owner，release/failure counter/queue-work 发布仍由 `Service::diag_hold_tick` 提交。
- `Service::diag_hold_tick(diag)` 接受实例参数。

RW-2（真实 buffer/descriptor 守恒账本）：

- `axdriver_net`：新增 transport-neutral `TxResourceLedger { buffer_available/inflight, descriptor_available/inflight, completions_seen }`；`NetTxQueue::tx_resource_ledger()` 默认 `None`。
- `virtio-drivers`：`VirtIONetRaw::send_available_desc()`；`axdriver_virtio`：`tx_completions_seen` 计数器（reclaim 与 legacy recycle 在 poll 到 used-ring completion 时递增，与成功 reclaim 独立）、`tx_resource_ledger()` 返回真实 `free_tx_bufs`/`send_queue.available_desc` 计数。
- axnet `Device::tx_resource_ledger()` 默认 `None`、`EthernetDevice` 转发 driver、`Router::tx_resource_ledger`、`Service::v3_tx_resource_ledger`。
- `rx_snapshot_v3` 以真实 driver ledger 填充 buffer/descriptor 字段（`available + inflight == capacity`，driver 无账本时报告 0 而非合成）；`tx_completion` 与 `tx_reclaim` 独立（completion = driver used-ring 观察，reclaim = 成功 cookie→ticket reclaim）。
- `RxTelemetry::ownership_invariant`：reclaim `Fault(BadState)`（unknown/duplicate/仍 Queued cookie）时递增一次并保持 terminal fault；V3 `ownership_invariant` 字段读取该计数器（替代恒 0）。

RW-3（stable flush fault + checked waiter identity）：

- `Service` 新增 `flush_terminal_fault: Option<u64>` 持久化字段：`flush_fault` 无论有无 waiter 都记录错误（waiter 消费后仍保留），`flush_begin` 在已记录 fault 时对当前及后续调用返回同一 stable error（不再依赖 ioctl timeout）。
- `flush_next_identity` 改为 checked 分配：`u64::MAX` 为无效 sentinel（与 V3 optional sentinel 一致），耗尽时返回 `ResourceBusy` 且不占 waiter slot、不 wrap 复用（关闭 ABA）。
- `flush::error_code` 改 `pub(crate)` 供 Service 复用。

RW-4（verification integrity）：

- `tests/ms03-irq-host-harness.rs` 重排为 rustfmt 规范格式；本 Cycle 全部 changed Rust 文件 `rustfmt --check` 真实 exit 0。

**Changed Files and Symbols**

| File | Symbols |
|---|---|
| `crates/axdriver_net/src/lib.rs` | `TxResourceLedger`、`NetTxQueue::tx_resource_ledger` |
| `crates/axdriver_virtio/src/net.rs` | `tx_completions_seen`、`tx_resource_ledger`、reclaim/recycle 递增点、2 个 RW-2 adapter tests |
| `crates/virtio-drivers/src/device/net/dev_raw.rs` | `send_available_desc` |
| `crates/axnet/src/diag.rs` | `diag_now`、`set_test_now`、`TEST_NOW`、`NS_PER_MS` |
| `crates/axnet/src/async_rx.rs` | `RxRxFuture::{diag, lease_deadline, lease_timer}`、`RoundOutcome::SleepUntil`、`arm/cancel/elapsed_lease_deadline`、`service_round` hold/reclaim 分支、`rx_snapshot_v3` 真实 ledger 映射、`RxTelemetry::ownership_invariant`、RW-1/RW-2 tests、`poll_first/poll_active/poll_register_recheck` 改 `&mut self` |
| `crates/axnet/src/service.rs` | `flush_terminal_fault`、`flush_begin` checked identity、`flush_fault` 持久化、`diag_hold_tick(diag)`、`v3_tx_resource_ledger`、test seams |
| `crates/axnet/src/flush.rs` | `error_code` 可见性、3 个 RW-3 tests |
| `crates/axnet/src/device/mod.rs` | `Device::tx_resource_ledger` |
| `crates/axnet/src/device/ethernet.rs` | `tx_resource_ledger` 转发 |
| `crates/axnet/src/device/tests.rs` | `FakeStats::ledger`、`FakeTxQueue::tx_resource_ledger`、3 个 RW-2 tests |
| `crates/axnet/src/router.rs` | `Router::tx_resource_ledger` |
| `crates/axnet/src/lib.rs` | `diagnostic_control` 用 `diag_now()` |
| `tests/ms03-irq-host-harness.rs` | rustfmt 重排 |

**Deviations from Plan**

- RW-1：deadline 唤醒在 test 下由 fake clock + counting waker 驱动（无真实 axtask timer 注册），生产走 `axtask::future::sleep_until`。这与 Plan 的 "timer 只负责唤醒 owner" 一致，test 边界用编译条件隔离。
- RW-2：`completions_seen` 递增点放在 adapter 的 reclaim/recycle poll 处（driver 观察到 used-ring），而非独立 step metadata；语义等同（completion observed 先于 reclaim 成功判定），接口更小。
- RW-2：`ownership_invariant` 计数放在 queue task 的 reclaim `Fault(BadState)` 分支（cookie→ticket 漂移的检测点），而非 driver 内部；真实 VirtIO 路径 unknown/duplicate cookie 也会在该点触发。
- D1 对照保持既有 25 errors（axfs/axtask feature exclusion），exit 101，只作为 exclusion 对照，不记 PASS。

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

逐 repair item 完成 Gate 4（spec → code 顺序）。RW-1 先验证 deadline 测试 GREEN 后审查 timer 生命周期（stale 失效、仅 wake owner）；RW-2 审查 ledger 守恒（available+inflight=容量）、completion/reclaim 独立、V3 无合成；RW-3 审查 stable fault 持久化与 identity 不 wrap；RW-4 复现 rustfmt 失败后全量重排。修复发现的 Minor：RW-1 测试共享全局 `DIAGNOSTIC`/`TEST_NOW` 导致并行污染（hold 测试未持 SERIAL、fake clock 未复位、reclaim hold 从不释放），改为 per-future diag 注入 + 测试内 release；RW-2 测试初版引用全局 `RX_TELEMETRY` 而非 future 本地 telemetry 导致断言错误，已修正。跨任务交互：flush waiter 在 reclaim fault 时由 `flush_fault` 持久化唤醒；V3 在 Service guard 内取真实 ledger 避免撕裂；`tx_completion` 独立后 MS04 V2 consumer 回归通过。

**Verification Evidence**

| 验证项 | 命令 | 输出摘录 | 结论 |
|---|---|---|---|
| axnet full（default） | `cargo test -p axnet-ng --locked --offline --lib` | 214 passed; 0 failed | PASS |
| axnet full（feature） | 同上 `--features qemu-diagnostics` | 227 passed; 0 failed | PASS |
| flush targeted | `--lib flush` | 23 passed | PASS |
| RW-3 flush tests | `--lib flush_after_terminal_fault` 等 | 3 passed | PASS |
| 100× flush race | 循环 100 次 `--lib flush` | 零失败 | PASS |
| 100× diag | 循环 100 次 `--lib diagnostic` | 零失败 | PASS |
| 100× register-recheck | 循环 100 次 `--lib register_recheck` | 零失败 | PASS |
| RW-1 lease tests | `--lib hold_submit_lease_deadline` 等 | 3 passed | PASS |
| RW-2 adapter tests | `cargo test -p axdriver-virtio --offline --features net` | 13 passed（含 2 新） | PASS |
| axdriver_net | `cargo test -p axdriver-net --offline` | 7 passed | PASS |
| virtio-drivers | `cargo test -p virtio-drivers --offline --features alloc` | 36 passed | PASS |
| MS03 host harness | `rustc --test tests/ms03-irq-host-harness.rs` | 33 passed | PASS |
| MS04 host harness | `rustc --test tests/ms04-async-rx-host-harness.rs` | 16 passed | PASS |
| kernel qemu check | `cargo check -p starry-kernel --features qemu` | exit 0 | PASS |
| D1 exclusion 对照 | `cargo check -p starry-kernel --features lichee-d1` | 仍为既有 25 errors（无新增） | PASS |
| rustfmt | `rustfmt --check --edition 2024 <全部 changed>` | exit 0（fxmac/ixgbe 为既有） | PASS |
| strict OpenSpec | `openspec validate ms05... --strict` | Change is valid | PASS |
| diff hygiene | `git diff --check` scoped | exit 0 | PASS |

**Persisted Evidence**

None required（Cycle Persisted Evidence mode = none；验证摘要见本 Act Response）。

**Experience Candidates**

None.

**Remaining Issues**

- `tx_completion` 现为 driver used-ring 观察计数（`completions_seen`），与 `tx_reclaim`（成功 reclaim）独立；V3 probe 可用 `completion >= reclaim` 判定一致性。
- D1 既有 25 errors 与无 feature kernel build 的 47 errors 均为 pre-existing，不在本 Cycle 修复范围。
- `flush_terminal_fault` 持久化后，fault 一旦发生所有后续 flush 永久失败——符合 Plan 的 stable fault 语义，恢复需 reset/重新初始化（后续 Iteration 处理）。

**Commit or Diff Reference**

None（本 Cycle 未创建 Git commit）。

## Plan Review

- Status: pending

**Review Result**

Pending.

**Findings**

Pending.

**Deviation Classification**

Pending.

**Acceptance Gaps**

Pending.

**Convergence**

Pending.

**Evidence**

Pending.

**Follow-up Decision**

Pending.

**Iteration Plan Update**

None.

**Next Cycle**

Pending.

**Next Iteration**

Pending.
