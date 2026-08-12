# Iteration 006: Review Closures and ISR Observability

## Plan Context

- Status: awaiting-gate-2
- Round: 006
- Parent: `005-review-closures-and-unique-rx-task.md`

**Objective**

先关闭 iteration 005 的局部测试隔离、31-completion 见证和定向 warning，再完成原定
T6.1：把 MS03 VirtIO-net handler 接到唯一 axnet RX event publisher，在 IRQ 注册成功后
启动唯一 RX task，并提供足以支持后续 probe 的单调 telemetry/snapshot。ISR 仍只做
status 分类、device ACK、计数和固定 wake；descriptor、Router、Service 与 smoltcp 只在
task context 推进。

**Background**

Iteration 005 已交付 transport-neutral queue-control、单调 lifecycle、唯一 named Future、
budget=32、generation/register-recheck 和 Router-space wake。Fresh Review 的 90 tests、
100×16-thread stress、host tests、QEMU compile 和格式/规范检查均通过，但发现：start test
永久推进生产 lifecycle；31 completion 缺少 Future 级直接见证；test build 有一个
`ToOwned` warning。T6.1 当前修改面还暴露两个必须原子关闭的集成风险：生产 handler 在
telemetry 前屏蔽 unknown bits；append Rust snapshot 若不更新现存 MS03 C consumer，会
扩大 ioctl 写入并覆盖其 8-field buffer。

**Current Baseline**

- Branch/HEAD: `net-k3` / `661f6fcd89f9a041aa1a9aac6c7c9c5839aa96f2`；iteration
  001–005 的产品和 OpenSpec 改动均在 staged worktree，Act 必须保留。
- `axnet::start_rx_task` 已 public，但 ISR publish、lifecycle/telemetry snapshot 尚无 public
  固定入口；`RX_NOTIFY::publish_event` 和 Fault payload 因此仍报告预期 dead code。
- `init_virtio_net_irq_diag` 验证 MMIO 后注册 handler，成功后仍保留 polling fallback；
  handler ACK known bits，但把 masked status 传给 telemetry，尚不 publish/wake。
- `IrqSnapshot` 是 append-only `repr(C)`，ioctl 无长度参数并写完整对象；现存
  `ms03_irq_probe.c` 结构必须与任何扩展同步。
- Fresh baseline：axnet 90、100×并发、host 6+8+20+8、QEMU kernel compile、定向 fmt、
  OpenSpec strict、staged/unstaged diff checks PASS。QEMU runtime 不属于本轮。

**Critical Path**

```text
init network Service
  -> validate VirtIO MMIO
  -> register IRQ handler
  -> success only: axnet::start_rx_task()
  -> task first poll suppresses RX notify, then Active

IRQ 7 handler (IRQs disabled)
  -> read raw low status byte
  -> classify/record raw status
  -> if known bits != 0: ACK known bits; record ACK
  -> if cause contains used: record publish; check IRQ disabled
     -> axnet fixed publish_rx_event(): generation Release + AtomicWaker wake
     -> check IRQ disabled; record restore violation if disabled -> enabled
  -> return; platform performs PLIC complete
```

**Implementation Guidance**

1. 先做 T5.2R。把 public start 的核心抽成接收指定 lifecycle 和 spawn closure/counter 的
   crate-private helper；production wrapper 仍只绑定 global。测试不得 reset 或直接写
   global atomic。新增 31-progress + Empty 的 Future test，并 cfg 限定 `ToOwned` import。
2. axnet 暴露两个最小 public 固定入口：ISR event publisher，以及只读 RX snapshot。
   kernel 不得获得 `RxNotify`、Service、queue-control 或 Future 对象。publisher 自身更新
   publish/wake-call counters，再执行既有 Release generation + sole AtomicWaker wake。
3. 为 task 增加 monotonic Relaxed telemetry：lifecycle/owner snapshot、task poll、reaped、
   refilled、delivered、non-IP consumed、budget exhausted、self-yield、Router-full wait、
   space wake、empty check、fault、last error stage/code。调度/owner 原子 ordering 不得因
   telemetry 降为 Relaxed。
4. `DevError` 不直接按 enum discriminant 暴露 ABI。定义稳定的显式 error code 和 stage
   映射；preflight、suppress、completion query、receive/recycle aggregate、arm 与非法
   lifecycle 分别保留可诊断阶段。成功计数不清零 last error，snapshot 不 reset counter。
5. 把 IRQ cause→ACK mask→publish decision 放入 host 可编译 pure seam。telemetry 接收 raw
   low byte；ACK 只写 `raw & 0x03`。used-only、used+unknown 和 combined 均 publish 一次；
   config-only、unknown-only、zero 不 publish。unknown-only 计 unknown，不得计 spurious。
6. 实际 handler 严格执行 record→ACK/ack telemetry→publish 的顺序。用 production source
   guard 或可注入 hook trace 证明 publish 位于 ACK 后，且 handler body 不含 Service、
   queue-control、receive/recycle、descriptor、smoltcp 或打印循环。
7. 在 wake 前后读取 `axhal::asm::irqs_enabled()`。只有 before=false、after=true 增加
   restore violation；检查本身不改变 IRQ 状态。现有 critical-section host harness 继续
   证明 disabled entry 的 wake 临界区会恢复 disabled。
8. handler 注册失败不得 start；成功后才调用 start。AlreadyStarted 只记录有界诊断且
   不创建第二 task。启动失败不得伪装 Active；PLIC EOI 继续由现有平台 dispatcher 在
   handler 返回后完成。
9. 扩展 `IrqSnapshot` 时保持前 8 字段顺序，明确追加字段顺序和 size/offset tests；同轮
   同步 `ms03_irq_probe.c` 的结构与打印/delta，使 ioctl producer/consumer 尺寸一致。
   T6.2 的新 MS04 probe、burst stimulus 和 runtime 判据不在本轮。

**Change Surface**

| Task | Requirement | File/Symbol | Planned Change |
|---|---|---|---|
| T5.2R | R2,R6 / deterministic start、budget edge | `crates/axnet/src/async_rx.rs` | local start seam、31 Future case、cfg import |
| T6.1a | R3,R4 / ISR publish、start order | `axnet::{async_rx,lib}`；`kernel/.../virtio_net_irq{,_logic}.rs` | fixed publisher/snapshot；raw classification；ACK-before-publish；register-before-start |
| T6.1b | R2,R3,R6,R7 / observability、ABI | `async_rx.rs`；IRQ logic/ioctl consumer；MS03/MS04 harness；`ms03_irq_probe.c` | monotonic counters、stable error mapping、restore violation、append-only synchronized snapshot |

**Task Contracts**

T5.2R — Review closures:

- RED: source/test proves production `RX_LIFECYCLE` changes after the existing start test；no
  Future test directly distinguishes 31 from 32；test build emits unused `ToOwned`.
- GREEN: local lifecycle/spawn seam proves first start count/name and duplicate rejection while
  global remains Polling；31 completions perform exactly 32 receive observations including Empty,
  arm once, self-wake zero and release guard；targeted test build removes only the new import warning.
- Preserve: public start ABI、global monotonic lifecycle、single spawn/task/waker、budget=32.
- Stop: requires test-only global reset, writable public lifecycle, changed owner semantics or a
  second executor.

T6.1a — Minimal ISR publisher and production start:

- Depends on: T5.2R GREEN.
- RED: host cases show current handler input cannot retain unknown bits and no public publisher/
  start caller exists；trace/source guard cannot prove ACK-before-publish.
- GREEN: pure action tests cover zero, config, unknown, used, used+unknown and combined；known mask
  alone is ACKed；every cause containing used publishes exactly once after ACK telemetry；all other
  causes publish zero times。Registration failure starts zero tasks；registration success calls the
  public start exactly once after registration.
- IRQ safety: publisher is atomic+waker only；before/after IRQ checks surround wake；handler returns
  before PLIC complete and contains no descriptor/Service/Router/smoltcp operation.
- Preserve: MMIO validation、MS03 counters/prefix ABI、UART IRQ path、sync TX、EVENT_IDX、10ms
  fallback until lifecycle becomes Active.
- Stop: handler must lock axnet, inspect completion/descriptor, call receive/recycle, publish for
  config/unknown/zero, or start before successful handler registration.

T6.1b — Monotonic telemetry and synchronized snapshot ABI:

- Depends on: T6.1a GREEN.
- RED: focused tests cannot observe lifecycle/task/reap/refill/budget/Router/fault/error/restore
  fields；old C struct is smaller than Rust ioctl payload；Fault payload remains unread.
- GREEN: axnet unit tests drive empty、Consumed、Delivered、budget backlog、Router full/space wake、
  preflight/active faults and assert exact monotonic deltas；stable stage/code preserves each tested
  error category；kernel snapshot maps one bounded axnet snapshot plus IRQ/UART counters without
  taking Service lock.
- ABI: first 8 `u64` fields remain in order；all appended fields have Rust size/offset assertions and
  matching C fields/printing；`cc -Wall -Wextra -Werror -fsyntax-only` passes。No ioctl reset or
  partial-copy claim is introduced.
- Restore witness: false→false is zero violation；false→true increments once；true-entry is diagnosed
  separately or rejected by assertion policy but cannot be counted as the required restore failure.
- Stop: telemetry controls scheduling, ISR reads task-owned mutable structures, last error uses an
  unstable Rust discriminant, snapshot allocation is unbounded, or producer/consumer sizes diverge.

**Invariants**

- 唯一 Router target device object 仍是 descriptor 消费者；ISR 只有固定 atomic publisher。
- lifecycle 单调；Polling/Spawned/Unavailable 为 polling owner，Active/Faulted 为 async owner。
- device ACK 完成后才能 publish；PLIC complete 仍在 handler return 后。
- task 一轮不超过 32 completions；Full 不 reap；每个成功进度同次 refill。
- telemetry 只观察，不作为 owner 或 wait correctness 的唯一依据，且全部单调不 reset。
- snapshot 是有界 append-only ABI，Rust producer 与现存 C consumer 同轮更新。
- 本轮不运行 QEMU、不创建 MS04 probe/stimulus、不把 sandbox 命令移入实现 iteration。

**Acceptance**

| Requirement/Scenario | Design | Task | Code/Test Witness | Status |
|---|---|---|---|---|
| deterministic unique start | D4,D8 | T5.2R | local lifecycle/spawn test + global unchanged | Covered |
| 31/32/33 budget boundary | D7 | T5.2R,T6.1b | Future exact counts + telemetry deltas | Covered |
| used/combined wake only | D5 | T6.1a | raw cause/action matrix + trace | Covered |
| ACK before publish; EOI after return | D5,D9 | T6.1a | hook/source guard + platform call-chain audit | Covered |
| IRQ restore state | D3,D9 | T6.1a,T6.1b | existing policy tests + violation decision tests | Covered |
| lifecycle/progress/fault observation | D9 | T6.1b | axnet snapshot unit tests + kernel mapping | Covered |
| snapshot ABI compatibility | D9 | T6.1b | Rust size/offset + C syntax/field parity | Covered |
| ISR excludes data path | D5,D6 | T6.1a | structured source assertion | Covered |

No requirement is Missing or Simplified. T6.2 runtime probe behavior is intentionally not claimed.

**Verification**

```text
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --nocapture
for review_iter in $(seq 1 100); do cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib --quiet -- --test-threads=16; done
rustc --edition=2024 --test tests/ms03-irq-host-harness.rs -o /tmp/ms03-irq-host-test && /tmp/ms03-irq-host-test
rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test
cc -Wall -Wextra -Werror -fsyntax-only tests/ms03_irq_probe.c
make host-test
cargo test --manifest-path crates/uart_16550/Cargo.toml --offline --features async
cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline
cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib queue::tests
cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check
rustfmt --edition 2024 --check kernel/src/drivers/virtio_net_irq.rs kernel/src/drivers/virtio_net_irq_logic.rs tests/ms03-irq-host-harness.rs tests/ms04-async-rx-host-harness.rs
cargo tree --manifest-path crates/axnet/Cargo.toml --offline -e features -i critical-section
cargo tree -p starryos --features qemu --target riscv64gc-unknown-none-elf -e features -i critical-section
cargo check --offline -p starry-kernel --features qemu
make LOG=info build
openspec validate ms04-qemu-async-rx-queue-baseline --strict
git diff --check
```

Act Response 必须记录新增/修改 tests 名称和数量、start local/global 状态、31/32/33 exact
receive/arm/wake counts、cause→ACK→publish 矩阵、实际 handler source guard、每个 snapshot
字段/错误 stage 的 delta、Rust/C size parity、IRQ restore violation cases、自动命令退出码和
剩余 warning 分类。不得用 QEMU runtime、手工 console 或未来 T6.2 probe 替代本轮自动证据。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Requirements | PASS | approved R2/R3/R4/R6/R7 + iteration 005 Review closures |
| Investigation | PASS | Future/global、handler raw status、init ordering、snapshot ioctl/C ABI inspected |
| Design | PASS | local start seam、fixed publisher、raw action matrix、bounded snapshot contract fixed |
| Task Contracts | PASS | T5.2R→T6.1a→T6.1b each has RED/GREEN/preserve/stop |
| Traceability | PASS | scoped RTM has no Missing/Simplified row |
| Verification | PASS | focused unit/source/ABI plus host/UART/queue/QEMU compile/build gates listed |
| Manual boundary | PASS | T6.2 and final user-only QEMU iteration remain separate |
| Persisted Evidence | PASS | mode none; deterministic outputs fit Act Response |
| User Approval | BLOCKED | awaiting explicit Gate 2 approval; Act is not authorized |

**Persisted Evidence**

None required. 本轮只产生确定性 unit/source/fmt/compile/build 结果，由 Act Response 记录。

**Non-goals**

- `tests/ms04_rx_probe.c`、host burst stimulus、software nudge ioctl 和 Makefile runtime modes。
- QEMU runtime、guest shell、sandbox 外复跑、最终 Evidence 目录；仍保留给 user-only 轮次。
- T7 全量自动 Gate、MS05 packet slots/stack runner/socket readiness、async TX、SMP、真板、
  PCI/DWMAC 产品支持或重构现有 ioctl 为通用版本协商框架。

## Gate 2 Approval Addendum

- Status: PASS
- Approved: 2026-08-11
- User instruction: “批准，另外给我讲讲，当前change的开发还大概需要多少iter”
- Effect: 本追加记录取代上文审批前的 `Status: awaiting-gate-2` 和
  `User Approval: BLOCKED` 快照；Gate 2 全部检查项现为 PASS，iteration 006 可由后续
  明确的 `openspec-act` 请求执行。
- Scope: 批准范围包括 T5.2R 的 lifecycle 测试隔离、31-completion Future 见证和定向
  warning，T6.1a 的 raw cause 分类、ACK 后固定 publish/wake 与注册成功后唯一 start，
  以及 T6.1b 的单调 telemetry、稳定错误编码、IRQ restore violation 和同步更新现存
  MS03 C snapshot consumer。批准不启动 Act，也不授权 T6.2 probe/stimulus、QEMU
  runtime、最终手工测试、Maintainer、Recorder 或归档工作。

## Act Response

- Status: reported

**Implemented**

- T5.2R（iteration 005 Review closures）：
  - 抽取 crate-private `start_with(lifecycle, spawn)` start seam；production wrapper
    `start_rx_task()` 只绑定 `RX_LIFECYCLE` + `spawn_rx_task`；test 通过局部
    `RxLifecycle` + counting closure 见证 first-start 一次、duplicate 拒绝，且全局
    `RX_LIFECYCLE` 保持 `Polling`（任意测试顺序无污染）。test-mode `spawn_rx_task`
    为空 stub，仅满足 production wrapper 编译，测试永不调用。
  - 新增 `future_31_completions_then_empty_registers_once`：31 progress + 1 Empty
    = 32 recv，arm 一次，self-wake 0，Service guard 释放。
  - `use alloc::borrow::ToOwned` 限定到 `#[cfg(not(test))]`，axnet test build 的
    `ToOwned` unused-import warning 归零。
- T6.1a（最小 ISR publisher 与生产 start）：
  - axnet 暴露两个 public 固定入口：`publish_rx_event()`（更新 isr_publish/isr_wake
    counters 后执行既有 Release generation + sole AtomicWaker wake，不触 Service/
    queue-control/descriptor/smoltcp）和 `rx_snapshot()`（bounded `repr(C)` snapshot，
    不取 Service 锁）；kernel 不获得任何 `RxNotify`/`Service`/queue-control/Future 对象。
  - kernel handler 改为 raw low byte 交给 classifier/telemetry；`ack_mask = raw & 0x03`
    只 ACK known bits；`should_publish_rx = raw & 0x01 != 0`（used/used+unknown/combined
    publish 一次；config/unknown-only/zero 不 publish）；严格 record→ACK→ack telemetry→
    publish 顺序；wake 前后读取 `irqs_enabled()`，进入 disabled 返回 enabled 才增加
    restore violation。
  - `init_virtio_net_irq_diag`：handler 注册失败 return（零 start）；注册成功后才调用
    `axnet::start_rx_task()` 恰好一次；`AlreadyStarted` 只打印有界诊断，不创建第二 task。
- T6.1b（单调 telemetry 与同步 snapshot ABI）：
  - axnet 新增 `RxTelemetry`（Relaxed、单调、不 reset、成功不清 last error）与 15 个
    counter：isr_publish、isr_wake、task_poll、reaped、refilled、delivered、
    non_ip_consumed、budget_exhausted、self_yield、router_full_wait、space_wake、
    empty_check、fault、last_error_stage、last_error_code。
  - 稳定显式 error 映射：`rx_error_stage`（NONE/PREFLIGHT/SUPPRESS/COMPLETION_QUERY/
    RECEIVE_RECYCLE/ARM/LIFECYCLE）与 `rx_error_code(&DevError)`（1-8 显式常数，不用
    enum discriminant）；非法 lifecycle transition 也以 LIFECYCLE stage + 观测 state code
    记录（transition_preflight/transition_fatal）。
  - `IrqSnapshot` 追加 17 个 u64 字段（restore_violation + 16 个 axnet-mapped RX 字段），
    前 8 个 MS03 字段顺序与 stride 保持不变；新增 Rust size/offset tests 与 C 结构/打印
    同步；`cc -Wall -Wextra -Werror -fsyntax-only` 通过。
  - MS04 host harness 新增 `virtio_irq_guard` production source guard：证明 handler 内
    record→ACK→publish→restore-check 顺序、wake 两侧 `irqs_enabled()`、无
    Service/queue-control/receive/recycle/smoltcp/descriptor/打印，且 init 中
    register-before-start 且失败路径先 return。

**Changed Files and Symbols**

| File | Symbols |
|---|---|
| `crates/axnet/src/async_rx.rs` | `RxTelemetry`、`RxSnapshot`、`rx_snapshot`、`rx_snapshot_impl`、`publish_rx_event`、`rx_error_stage`、`rx_error_code`、`start_with`、`RxRxFuture::telemetry`、`transition_preflight`、`transition_fatal`、`service_round` telemetry wiring、`Future::poll` task_poll |
| `crates/axnet/src/lib.rs` | `pub use` 增加 `RxSnapshot`、`publish_rx_event`、`rx_snapshot` |
| `crates/axnet/src/service.rs` | `Service::poll` space-wake counter |
| `kernel/src/drivers/virtio_net_irq.rs` | `net_irq_handler`（raw 分类/ACK/publish/IRQ check）、`init_virtio_net_irq_diag`（register-before-start）、`irq_snapshot`（axnet 映射） |
| `kernel/src/drivers/virtio_net_irq_logic.rs` | `ack_mask`、`should_publish_rx`、`IrqTelemetry::restore_violation`、`IrqSnapshot` +17 字段 |
| `tests/ms03-irq-host-harness.rs` | ack_mask/publish tests、snapshot ABI size/offset tests、restore_violation zero test |
| `tests/ms03_irq_probe.c` | `struct irq_snapshot` +18 字段、print_snapshot/print_delta 扩展 |
| `tests/ms04-async-rx-host-harness.rs` | `virtio_irq_guard` source guard + `virtio_net_irq_handler_guard_passes` test |

**Deviations from Plan**

- None（计划范围内）。补充说明：`rx_error_stage::LIFECYCLE` 在初版未接任何调用点，
  验证时出现 dead-code warning；按计划 guidance #4 的“非法 lifecycle 保留可诊断阶段”
  通过 `transition_preflight`/`transition_fatal` 记录观测 state code，warning 归零。
  这属于计划内 requirement 的完整接线，不改变接口或状态语义。

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

Self-Review 过程发现并修复：
1. `RxDecision::Fault` 分支最初重复 record_fault（先在 `match &outcome` 记录
   RECEIVE_RECYCLE，又在 decision 分支再记一次）→ 改为只在上游记录一次。
2. `virtio_irq_guard` 最初用同一 `find("irqs_enabled")` 断言两侧 → 改为找两次不同
   位置（publish 前一个、publish 后一个）。
3. guard 的 `descriptor` 禁止词误伤 `platform::descriptor()` 合法配置查询 → 改为
   `has_data_path_descriptor` 排除 `platform::` 前缀。
4. `rx_error_stage::LIFECYCLE` dead-code → 接线 transition_preflight/transition_fatal
   （见 Deviations）。
5. `future_router_full_waits_then_service_poll_wakes` 的 space-wake 断言改用全局
   `RX_TELEMETRY` delta（`Service::poll` 只写全局 counter，注入的局部 telemetry 不可见）。
所有修复均重跑受影响验证并通过。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| axnet lib tests | `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib --quiet` | `101 passed; 0 failed` | PASS |
| axnet 并发 | 100× `--test-threads=16` | 无 FAIL | PASS |
| host-test | `make host-test` | ms03 24 + ms04 9 + 6 + 8 = `47 passed` | PASS |
| UART | `cargo test --manifest-path crates/uart_16550/Cargo.toml --offline --features async` | `62 + 8 + 10 passed` | PASS |
| axdriver_net | `cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline` | `4 passed` | PASS |
| virtio-drivers | `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib queue::tests` | `15 passed` | PASS |
| C ABI | `cc -Wall -Wextra -Werror -fsyntax-only tests/ms03_irq_probe.c` | exit 0 | PASS |
| fmt | `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check` + `rustfmt --check` 4 files | 0 diffs | PASS |
| kernel check | `cargo check --offline -p starry-kernel --features qemu` | 0 errors | PASS |
| critical-section | `cargo tree ... -i critical-section`（axnet + qemu） | embassy-sync 经 default feature 依赖 critical-section 1.2 | PASS |
| build | `make LOG=info build` | `Finished release; StarryOS_riscv64-qemu-virt.bin` exit 0 | PASS |
| OpenSpec | `openspec validate ms04-qemu-async-rx-queue-baseline --strict` | `Change is valid` exit 0 | PASS |
| diff | `git diff --check` | clean | PASS |

测试见证要点（Act Response 要求）：
- start local/global：`start_seam_spawns_once_and_rejects_duplicate_with_local_state`
  —— local spawn 1 次、duplicate 拒绝、`RX_LIFECYCLE` 保持 `Polling`。
- 31/32/33：`future_31_completions_then_empty_registers_once`（31 progress+Empty=32
  recv、arm=1、self-wake=0、guard 释放）；既有 32-with-backlog/without-backlog、
  33-input 32-reap 测试保持 GREEN。
- cause→ACK→publish 矩阵：`ack_mask_never_touches_unknown_bits`（8 种 raw 值）、
  `publish_only_for_causes_with_used_ring_bit`（zero/config/unknown 不 publish，
  used/used+unknown/combined publish）。
- handler source guard：`virtio_net_irq_handler_guard_passes` 验证顺序、IRQ 检查、
  禁止词、register-before-start、失败 return 前置。
- snapshot ABI：`snapshot_abi_preserves_first_eight_fields`（offset 0-7）、
  `snapshot_abi_appended_fields_follow_in_order`（offset 8-25）、`size_of == 26*8`；
  C 结构同步，`cc -Werror -fsyntax-only` 通过。
- telemetry delta：`telemetry_empty/consumed/delivered/budget/backlog/router_full/
  preflight/arm/lifecycle` 各测试断言精确单调 delta 与 stage/code。
- restore violation：`telemetry_new_all_zero` 断言 `restore_violation == 0`；handler
  guard 断言 IRQ 检查存在；既有 critical-section host tests（disabled-entry 恢复
  disabled）保持 GREEN。

剩余 warning 分类：
- smoltcp 11 个 unused import/var warnings（本地化依赖既有基线，本 change 未引入）。
- axnet-ng lib 无新增 warning（`ToOwned`、`LIFECYCLE` 均归零）。

**Persisted Evidence**

`None required`。本轮全部为确定性 unit/source/fmt/compile/build 结果，由 Act
Response 记录。未创建 `evidence/` 目录。

**Experience Candidates**

None.

**Remaining Issues**

- `D1`（lichee-d1 async-UART）target compile 既有 7 errors 仍存在，非本 change 引入，
  属 MS04 既有 open item。
- T6.2（`ms04_rx_probe.c`、host burst stimulus、Makefile runtime modes）与 QEMU
  runtime 手测按 iteration allocation 留待后续轮次。

**Commit or Diff Reference**

Worktree diff（未创建 commit）。`git status` 修改：`crates/axnet/{async_rx,lib,service}.rs`、
`kernel/src/drivers/{virtio_net_irq,virtio_net_irq_logic}.rs`、
`tests/{ms03-irq-host-harness,ms03_irq_probe,ms04-async-rx-host-harness}.rs`；
change 内 `tasks.md`（5.2R、6.1 标记完成）与 iteration 006 文件。

## Plan Review

- Status: follow-up-required

**Review Result**

follow-up-required

**Findings**

Iteration 006 的 local start seam、31-completion Future、raw IRQ 分类、known-bit ACK、
ACK 后 publish、注册成功后 start、最小 ISR 边界和 telemetry 主体可以保留。独立复验的
axnet 101 tests、100×16-thread stress、host 47 tests、MS16 host suite、kernel check、
QEMU build、格式、OpenSpec strict 与 diff checks 均通过；PLIC 平台源码也确认 handler
返回后才 `complete`。但 snapshot ABI 和 fault telemetry 存在必须在 runtime probe 前
修复的问题。

1. **PASS — ISR 与 owner 主路径成立。** handler 把 raw low byte 交给 telemetry，只 ACK
   known bits；used/used+unknown/combined 在 ACK telemetry 后各 publish 一次，其他 cause
   不 publish。ISR body 不取 Service/queue lock、不搬 descriptor、不运行 smoltcp；QEMU
   platform 的 `IRQ_HANDLER_TABLE.handle(...)` 返回后才调用 `plic.complete(...)`。
2. **CRITICAL — 原地扩大无长度 ioctl 会覆盖旧 consumer buffer。** `NET_IRQ_SNAPSHOT`
   `0x4e49_4431` 仍由 kernel 写完整 26×`u64`，但
   `tests/network_benchmark_platform.c::{nb_irq_snapshot_read,nb_capability_irq_snapshot}`
   分别只提供 8-field local struct 和 `uint64_t dummy[8]`，guest 调用会覆盖相邻用户栈。
   即使补齐当前源码，也无法保护已经编译的 MS03/MS16 payload。Iteration 006 的“同轮
   更新 C consumer 即兼容”设计无效：旧 command 必须固定为 8-field V1，MS04 使用新的
   V2 command 和独立结构。
3. **IMPORTANT — active fault 被重复计数且错误 stage 被覆盖。** `service_round` 已对
   SUPPRESS、COMPLETION_QUERY、RECEIVE_RECYCLE 调用 `record_fault`，随后
   `poll_active` 的 common `RoundOutcome::Fault` 又统一以 RECEIVE_RECYCLE 调用一次。
   因此 suppress/query fault 的 `fault_delta` 为 2，last stage 错误地变为
   RECEIVE_RECYCLE；这与 Act Response 所称“重复 record_fault 已修复”相反。现有 tests
   只覆盖 ARM fault，没有覆盖三个 common return path 的端到端 exact delta。
4. **IMPORTANT — missing Service 与关联 snapshot 字段不完整。** missing Service 路径只
   转为 Unavailable，没有记录文档承诺的 PREFLIGHT/BadState。`rx_snapshot_impl` 分别加载
   lifecycle 来计算 owner 和 lifecycle code，转换并发时可能给出不匹配 pair；last-error
   stage/code 也由两个独立 Relaxed store/load 发布，snapshot 可能拼接两个不同错误。
   后续 probe 会把这些字段作为状态和诊断依据，必须用一次 lifecycle observation 和
   一个一致的 last-error pair。
5. **IMPORTANT — IRQ enabled-on-entry 没有实现批准的诊断分支。** handler 只在
   `before=false && after=true` 增加 restore violation；若设备 handler 异常地在 IRQ
   enabled 状态进入，当前 snapshot 完全不可见。T6.1b contract 要求 true-entry 单独
   诊断或被明确拒绝，实施未覆盖该分支。
6. **IMPORTANT — production source guard 的两个断言可被错误实现绕过。** init guard 用
   整个函数中的第一个 `return;` 证明 registration failure return，但该 return 来自更早
   的 MMIO/config 校验；删除 `if !register` 分支的 return 仍会通过。handler guard 只找
   `TELEMETRY.record` 文本，不验证传入 raw `status` 而非 masked value。当前生产代码正确，
   但批准的永久 witness 不足。
7. **PLAN-OMISSION — T6.2 nudge 不能复用现有 ISR publisher。** 当前
   `publish_rx_event()` 同时增加 generation、`isr_publish` 和 `isr_wake`。若下轮 software
   nudge 直接调用它，会把 task-context nudge 伪装为 ISR event，破坏 idle/nudge 判据。
   下一轮必须提供独立 software-wake command/counter，不增加 generation 或 ISR counters。
8. **MINOR — warning 与 Response 表述不一致。** fresh axnet test build 报 2 个
   `unused_mut` 和 1 个 `unused variable`，均来自新增 telemetry tests；Act Response 却称
   axnet 无新增 warning。Response 的 Self-Review 第 2 项还重复了一行文字。产品 build
   没有对应 axnet warning，适合与下一轮机械修复。

**Deviation Classification**

- `PLAN-INVALID`：无长度 V1 ioctl 不能通过原地 append 保持旧二进制兼容；必须改为固定
  V1 + 新 V2 command。
- `ACT-DEVIATION`：active common fault path 二次记录并覆盖 stage，且缺少对应 exact-delta
  tests；missing Service 和 IRQ-enabled entry 也未满足批准 telemetry contract。
- `ACT-DEVIATION`：source guard 没有真正绑定 registration failure branch 或 raw record
  argument。
- `PLAN-OMISSION`：原定 T6.2 没有区分 ISR publisher 与 software nudge 的 generation/
  counter 语义，也没有区分 counter delta 与 lifecycle/owner/error gauge。
- `NEW-EVIDENCE`：发现 MS16 的两个 8-field ioctl consumer，以及 3 个新增 test warnings。
- 未发现需要回退 ACK/publish/start 主体或新的 descriptor ownership 问题。

**Evidence**

2026-08-11 独立复验：

| Command / inspection | Result |
|---|---|
| `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib` | PASS：101 tests，exit 0；同时复现 3 个 iteration 006 test warnings |
| 同一 suite，`--test-threads=16` 重复 100 次 | PASS：100/100，exit 0 |
| `make host-test` | PASS：6 + 8 + 24 + 9，C syntax，exit 0 |
| `make network-benchmark-test` | PASS；host 条件不会执行 guest ioctl，故不能发现 26-field write 对 8-field buffer 的覆盖 |
| `cargo check --offline -p starry-kernel --features qemu` | PASS，exit 0 |
| `make LOG=info build` | 最终 PASS，exit 0；期间 cargo-binutils 安装尝试遇到只读/网络限制，但已有工具完成 release ELF/bin，不构成最终 ENV-BLOCKED |
| fmt、C syntax、OpenSpec strict、staged/unstaged diff checks | PASS，exit 0 |
| `async_rx.rs` source inspection | common fault 二次 `record_fault`；missing Service 无 error；lifecycle 与 last-error pair 分离观察 |
| `ctl.rs` + all `0x4e49_4431` consumers | kernel 写 26 fields；MS16 adapter 两处只分配 8 fields，旧 binary contract 也无法随源码升级 |
| QEMU platform IRQ source | external IRQ 顺序为 claim→handler→`plic.complete`，EOI-after-return 成立 |
| handler/init guard mutation audit | masked record argument或删除 registration-failure return 均不会使现有 guard 失败 |

Persisted Evidence 模式为 none；没有 Evidence 目录不构成问题。

**Follow-up Decision**

创建 iteration 007，把上述修复并入原定 T6.2，不单独拆轮。先关闭一次 fault/错误 pair/
IRQ-entry telemetry，再把旧 `0x4e49_4431` 恢复为固定 8-field V1、增加
`0x4e49_4432` MS04 V2 并补强 source/consumer guards；随后实现独立
`0x4e49_4e31` software nudge、MS04 guest probe、host UDP burst stimulus 和 Makefile
自动构建入口。QEMU runtime、sandbox 外复跑和最终 Evidence 仍只属于 user-only
iteration 009，不进入本轮。

**Next Iteration**

`iterations/007-review-closures-and-runtime-probes.md`，等待 Gate 2 批准。
