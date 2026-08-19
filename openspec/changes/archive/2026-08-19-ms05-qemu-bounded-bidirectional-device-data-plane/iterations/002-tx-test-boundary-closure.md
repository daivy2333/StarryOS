# Iteration 002: TX Test Boundary Closure

## Plan Context

- Status: ready
- Round: 002
- Parent: Iteration 001

**Objective**

关闭 Iteration 001 Review 的两个 Important 缺口：移除仅为 host fixture 暴露的生产态
`VirtIONetRaw::transport_mut()`，并用真实 `VirtIoNetDev` 入口证明 owner 已匹配时发生
completion error，legacy/queue ledger 都不被消费且进入稳定 TX fault。完成后，Task 2.1-2.3
才能依赖一个没有测试后门、错误证据闭合的 TX contract。

**Background**

Iteration 001 已把 TX 状态合并为 `TxSlot::{Free, Legacy, Queue}`，区分 pre-accept recovery
与 post-accept fatal，并让正常 exhaustion 返回 `Again`。新鲜回归全部通过，但 Review 发现：

- fixture 通过公开 `VirtIONetRaw::transport_mut() -> &mut T` 写 used ring。这个方法会进入普通
  产品 API，让任意 raw-driver caller 可在 queue 活跃时直接修改 transport status、queue
  enablement 或 notification 状态；测试需求不应扩大生产态 capability。
- 当前 9 个 adapter tests 覆盖 cross-owner、duplicate、occupied、out-of-range、exhaustion 与
  成功 reclaim，却没有进入 `recycle_tx_buffers()` / `reclaim_tx()` 中 owner 已匹配、
  `transmit_complete()` 返回 error 的分支。Iteration 001 Required GREEN 对该路径有明确要求，
  Act Response 将实现分支存在误写成已被测试证明。

**Current Baseline**

- Revision: `1a2bc99f657986d554d21f496579476569de6368`，branch `net-k3`；Iteration 001 产品与
  OpenSpec 改动仍在 staged worktree。
- `VirtIoNetDev` 已有 tagged TX ledger、`tx_fault`、`tx_fault_buf`、net-local QueueFull mapper
  和仅在 adapter test build 存在的 `forced_tx_token`。
- `FakeTransport::complete_tx()` 只能经 `dev.transport_mut()` 取得 transport 后写 used ring；
  测试没有保留独立 device-side controller。
- 两条 reclaim path 都在调用 `transmit_complete()` 前保留 slot，error 时调用
  `enter_tx_fault(None)`；代码结构正确，但该分支尚无独立执行证据。
- delivered worktree 还包含 `.claude/runbooks/virtio-real-adapter-test-fixture.md` 与 R52 登记。
  它们未列入 Iteration 001 Changed Files，且该轮 Persisted Evidence 为 none；这是独立的
  workflow deviation，不属于本轮 Act 的可写范围。
- 用户明确不要求 `make LOG=info build`，并报告当前 `make run` 正常。本轮不运行或判定前者，
  也不把后者升级为独立 runtime Evidence。

2026-08-13 Review fresh baseline：

| Command | Result | Exit |
|---|---|---:|
| `cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net` | 9 passed | 0 |
| `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib -- --nocapture` | 36 passed | 0 |
| `cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline` | 7 passed | 0 |
| `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --nocapture` | 109 passed | 0 |
| `cargo check --offline -p starry-kernel --features qemu` | PASS | 0 |
| `openspec validate ms05-qemu-bounded-bidirectional-device-data-plane --strict` | valid | 0 |
| change-local `git diff --check` | clean | 0 |

**Current-State Evidence**

- Public seam: `crates/virtio-drivers/src/device/net/dev_raw.rs::transport_mut` is unconditional
  `pub fn` and returns unrestricted `&mut T`.
- Fixture coupling: `crates/axdriver_virtio/src/net.rs::tests::real_adapter_*` obtains the fake device
  only through the adapter's private wrapper over that public raw accessor.
- Error branch: both `recycle_tx_buffers()` and `reclaim_tx()` compute `completion_failed` while the
  matching `TxSlot` is still installed, then enter stable fault. No current test forces this boolean
  to true after a matching completion is visible.
- Raw constraint: current `poll_transmit()` returns the same used-ring id that `transmit_complete()`
  passes to `VirtQueue::pop_used()`. A well-formed fake used entry therefore cannot naturally create
  `WrongToken`; a deterministic adapter-local test injection is needed to witness the error branch
  without corrupting raw queue internals.

**Relevant Code**

| File / Symbol | Current Responsibility | Planned Use |
|---|---|---|
| `crates/virtio-drivers/src/device/net/dev_raw.rs::transport_mut` | public mutable transport access | remove the test-only product API |
| `crates/axdriver_virtio/src/net.rs::FakeTransport` | in-memory transport and used-ring addresses | expose a separate test controller, not adapter internals |
| `VirtIoNetDev::recycle_tx_buffers` | legacy completion and buffer recycle | inject one test-only completion failure and prove ledger retention |
| `VirtIoNetDev::reclaim_tx` | queue completion and cookie return | inject one test-only completion failure and prove cookie retention |
| adapter tests | real submit/poll/reclaim witness | add source/API guard and two completion-error cases |

**Critical Path**

```text
test controller ──writes used completion──> fake used ring
                                           │
real VirtIoNetDev poll ─────────────────────┘
  → matching TxSlot owner
  → test-only one-shot completion error
  → ledger unchanged + tx_fault=true
  → BadState now and on later TX operations
```

The controller must be retained by the test before transport ownership moves into `VirtIoNetDev`.
It may share only fake-device state such as used-ring addresses/indexes; it must not expose a mutable
reference to the production raw driver or transport.

**Implementation Guidance**

1. Write RED tests first. Add a source/API assertion that the raw net driver no longer exposes
   `transport_mut`, plus queue-owner and legacy-owner completion-failure tests through the real adapter.
2. Refactor `FakeTransport` to share device-side completion state with a controller retained by the
   test. Store ring addresses in a representation safe for the chosen test synchronization primitive.
   The controller writes the same used-ring layout and release-ordered index as the current fixture.
3. Remove both adapter and raw `transport_mut` wrappers. Do not replace them with another product-visible
   `transport`, queue-pointer, token or callback accessor.
4. Add the smallest adapter-local `#[cfg(test)]` one-shot seam needed to make the next matching
   `transmit_complete` attempt fail. Production builds must contain neither field nor branch.
5. In each error test, perform real allocate + legacy/queue submit, publish a matching used completion,
   invoke the public reclaim entry, and assert: `BadState`, original slot owner/cookie remains, free count
   does not increase, `can_transmit()` is false, and a later TX operation returns stable `BadState`.
6. Keep Iteration 001's success, pressure, collision, cross-owner and EVENT_IDX tests unchanged except
   for fixture access mechanics.

**Behavioral Change**

- Product behavior: none. TX success, pressure and fault semantics remain those of Iteration 001.
- Public raw-driver surface: the newly added unrestricted `transport_mut()` is removed before it becomes
  an accepted baseline.
- Test behavior: queue and legacy completion errors gain direct real-adapter witnesses.

**Change Surface**

| Task | Requirement | File / Symbol | Planned Change |
|---|---|---|---|
| 1.5 | R3 completion ownership | `axdriver_virtio::VirtIoNetDev` tests and test-only seam | force matching completion error; prove ledger retention |
| 1.5 | R7 transport-neutral boundary | `virtio-drivers::VirtIONetRaw` | remove public mutable transport accessor |
| 1.5 | R14 verification order | iteration tests/Gates | RED→GREEN, full regression, source guard |

**Task Contract**

### Task 1.5 — Close the TX fixture and completion-error boundary

- Depends on: Task 1.4 implemented; Iteration 001 Plan Review `follow-up-required`.
- Current behavior: all automatic Gates pass, but the fixture widens product API and completion-error
  ledger retention is inferred from code rather than executed.
- Target behavior: fake device completion is driven out-of-band, production API has no test transport
  accessor, and both real adapter reclaim APIs prove stable fatal with unchanged ownership.
- Required RED:
  - source/API guard fails while `pub fn transport_mut` exists;
  - queue-owner completion error has no current injection/witness;
  - legacy-owner completion error has no current injection/witness.
- Required GREEN:
  - no unconditional `transport_mut` or equivalent mutable transport/ring accessor exists in the raw
    net product API;
  - queue failure retains `TxSlot::Queue(buffer, original_cookie)`, does not return a cookie or increase
    `free_tx_bufs`, and all later TX operations return `BadState`;
  - legacy failure retains `TxSlot::Legacy(buffer)`, does not increase `free_tx_bufs`, and all later TX
    operations return `BadState`;
  - existing 9 adapter tests and full regressions remain GREEN.
- Must modify: `crates/virtio-drivers/src/device/net/dev_raw.rs` and
  `crates/axdriver_virtio/src/net.rs`; this iteration file's Act Response.
- May modify: a change-local source guard if one already exists and can express the API constraint
  without introducing a new harness.
- Must not modify: public queue traits, runtime TX semantics, VirtQueue algorithm, axnet Router/device,
  frame slots, ARP, lifecycle/ISR, Cargo registry, global Runbook/reference indexes, Evidence, or user
  `CLAUDE.md` changes.
- Stop condition: if completion failure cannot be injected without production code/API or raw ring
  corruption, stop and return to Plan with the exact constraint; do not substitute duplicate/cross-owner
  errors or helper-only ledger assertions.

**BDD Scenarios**

```gherkin
Scenario: Queue completion error retains its unique owner
  Given a real VirtIoNetDev has accepted one queue TX buffer with cookie C
  And the fake device publishes the matching used completion
  When the next completion attempt is forced to fail in test-only adapter code
  Then reclaim_tx returns BadState
  And the same queue slot still owns the buffer and cookie C
  And the free buffer count is unchanged
  And later TX operations return BadState

Scenario: Legacy completion error retains its unique owner
  Given a real VirtIoNetDev has accepted one legacy TX buffer
  And the fake device publishes the matching used completion
  When the next completion attempt is forced to fail in test-only adapter code
  Then recycle_tx_buffers returns BadState
  And the same legacy slot still owns the buffer
  And the free buffer count is unchanged
  And later TX operations return BadState

Scenario: Tests do not widen the product transport API
  Given the adapter tests need to emulate device-side completion
  When the crate is built outside cfg(test)
  Then VirtIONetRaw exposes no mutable transport accessor added for the fixture
  And the fake controller can still complete real adapter submissions in tests
```

**Invariants**

- A completion error must not call `mem::replace` on the slot, return a cookie, recycle or drop its buffer.
- The one-shot injection exists only under `cfg(test)` and cannot change production layout or branches.
- The fake controller cannot mutate the adapter ledger or raw queue bookkeeping directly.
- No public interface gains a VirtIO token, descriptor, ring pointer, transport reference or test control.
- Existing pre-accept recovery and post-accept fault semantics remain unchanged.

**Non-goals**

- 不创建 fixed frame slots、typed Router outcome 或 ARP transaction。
- 不修改 EVENT_IDX、descriptor allocation、queue size 或 DMA layout。
- 不运行 QEMU、不创建 Evidence、不声明真板/SMP/性能结论。
- 不创建、修改或登记 Runbook/M/D/K/R/I；只在 Act Response 返回新的 Experience Candidate。

**Acceptance**

| Requirement | Scenario | Design | Task | Code/Test | Status |
|---|---|---|---|---|---|
| R3 | completion error 保留 buffer/cookie owner | D1,D6 | 1.5 | queue + legacy real-adapter failure tests | Planned |
| R7 | test fixture 不泄漏 transport mutation | D1 | 1.5 | raw API removal + source guard | Planned |
| R14 | 自动 Gate 与 Review 缺口闭合 | D10 | 1.5 | focused/full tests, kernel, strict, diff | Planned |

No requirement is simplified. Task 2.1-2.3 moves to Iteration 003.

**Verification**

Record RED and GREEN command, failure reason, key output and exit code. Final Gates:

```text
cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net
cargo check --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net
cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib -- --nocapture
cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --nocapture
cargo check --offline -p starry-kernel --features qemu
! rg -n "pub fn transport_mut|pub.*&mut T" crates/virtio-drivers/src/device/net/dev_raw.rs
openspec validate ms05-qemu-bounded-bidirectional-device-data-plane --strict
git diff --check -- crates/axdriver_virtio crates/virtio-drivers openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane
```

Run targeted `rustfmt --check --config skip_children=true` for Rust files changed in this iteration.
Do not run or report `make LOG=info build`; the user excluded it from this Review line.

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | public seam、fixture ownership、raw completion semantics与两个 reclaim分支已追踪 |
| Design | PASS | out-of-band fake controller + adapter-local cfg(test) one-shot injection avoids product API expansion |
| Task Contracts | PASS | Task 1.5 fixes both Important findings with RED/GREEN and explicit stop boundary |
| Traceability | PASS | R3/R7/R14 map to D1/D6/D10, exact symbols and tests |
| Verification | PASS | focused adapter, full dependency/consumer, kernel, source, strict and diff Gates fixed |

**Persisted Evidence**

- Mode: none

All evidence is reproducible unit/build output summarized in the Act Response. Do not create an Evidence
directory, Runbook or global reference entry.

**Risks and Notes**

- Shared fake state must not retain a Rust reference into a moved transport; use owned shared state and
  numeric ring addresses or another representation whose synchronization bounds are valid.
- The injected error must occur after a matching completion is visible and before raw completion consumes
  it. A pre-poll error does not prove ledger retention.
- The source guard is supplemental. The decisive evidence is successful product compilation without the
  accessor plus real-adapter tests through the retained fake controller.
- The staged Runbook/R52 remains outside this iteration. Act must not edit or cite it as accepted evidence.

**Deferred Tasks**

- Iteration 003: Tasks 2.1-2.3, fixed slots, typed Router/Device and ARP.
- Iteration 004: Tasks 3.1-3.3, bidirectional lifecycle/event/service/ISR cutover.
- Iteration 005: Tasks 4.1-4.3, ticketed flush, V3 and QEMU lease controls.
- Iteration 006: Tasks 5.1-5.2, probe and automatic product Gates.
- Iteration 007: Tasks 6.1-6.3, environment rerun, QEMU runtime and final Review.

## Act Response

- Status: reported

**Implemented**

- 移除 raw 产品 API `VirtIONetRaw::transport_mut()`（`crates/virtio-drivers/src/device/net/dev_raw.rs`），并移除 adapter 侧
  `VirtIoNetDev::transport_mut()` wrapper。产品构建不再暴露任何可变 transport/ring 访问器。
- 测试 fixture 改为独立共享控制句柄：`FakeDeviceState`（used-ring 地址 + 写索引）经 `Arc<Mutex<..>>` 在
  `FakeTransport`（移入设备）与 `FakeDevice`（测试保留）之间共享；`FakeDevice::complete_tx()` 在 adapter 之外写 used ring，
  不触碰 adapter ledger 或 raw queue 簿记。
- 新增 adapter-local `#[cfg(test)]` one-shot completion fault seam：`forced_completion_failure` 字段 +
  `fail_next_tx_completion()` + 统一 completion 入口 `tx_completion_failed(token)`。生产构建无该字段、无测试分支；
  注入发生在 `poll_transmit()` 已见匹配 used entry 之后、raw `transmit_complete()` 消费之前（不伪造成功 reclaim、
  不修改 raw queue 算法）。
- 两条 reclaim 路径（`recycle_tx_buffers()` legacy、`reclaim_tx()` queue）改为调用 `tx_completion_failed(token)`；
  失败时 `enter_tx_fault(None)`，ledger/cookie/buffer 全部保留。
- 新增 2 个真实 adapter 测试：`queue_completion_error_retains_owner_and_enters_fault`、
  `legacy_completion_error_retains_owner_and_enters_fault`；原 9 个测试仅迁移 fixture 访问机制。

**Changed Files and Symbols**

| 文件 | 符号 | 作用 |
|---|---|---|
| `crates/virtio-drivers/src/device/net/dev_raw.rs` | `VirtIONetRaw::transport_mut` | 删除公开可变 transport accessor |
| `crates/axdriver_virtio/src/net.rs` | `VirtIoNetDev::forced_completion_failure`（cfg(test)）、`fail_next_tx_completion`（cfg(test)）、`tx_completion_failed`、`recycle_tx_buffers`、`reclaim_tx`；tests 模块 `FakeDeviceState`/`FakeDevice`/`FakeTransport`/`test_dev` 及全部测试 | 移除 adapter `transport_mut`；加入 one-shot fault seam；fixture 改为共享控制句柄 |
| `crates/axdriver_virtio/src/lib.rs` | `#[cfg(test)] extern crate std;` | 使测试构建可在 no_std crate 内使用 `std::sync::Mutex` |
| `openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane/tasks.md` | Task 1.5 | 标记完成 |

**Deviations from Plan**

1. `crates/axdriver_virtio/src/lib.rs` 增加 `#[cfg(test)] extern crate std;`（不在 Must-modify 列表）。Plan 实现指引要求
   “Store ring addresses in a representation safe for the chosen test synchronization primitive”，选择 `std::sync::Mutex`
   作为同步原语；no_std crate 的测试构建需要该声明才能将 `std` 纳入作用域。仅 cfg(test)，生产零影响。
2. source guard 以 `grep -E` 替代 `rg`（本环境未安装 rg），模式与语义一致：`pub fn transport_mut|pub.*&mut T`。
3. seam 实现细节：`tx_completion_failed` 在 armed 时短路返回 error，不实际调用 raw `transmit_complete`。这正对应 Plan
   Critical Path 的 “injected error ... before raw completion consumes it” 且不破坏 raw queue 内部；观测契约
   （BadState、ledger/cookie/buffer 保留、后续 TX 稳定 fatal）与真实 completion error 完全一致。

**Blocker Handoff**

None

**Blocker Resolution**

None

**Self-Review**

- Plan compliance: PASS
- Full diff reviewed: PASS
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 2
  - `crates/axdriver_virtio/src/lib.rs` 存在 HEAD 即有的 crate-context rustfmt 偏差（`pub use` 分组、`as_dev_err`
    内层 import 合并、`probe_mmio_device` 空行）。同一内容在独立文件检查通过、被标记块未被本次或 Iteration 001 改动触碰，
    判定为上游既有偏差；按外科手术原则不扩大 diff 修复，迭代 rustfmt gate 只覆盖本次改动的 Rust 文件。
  - rg→grep 工具替代（见 Deviations）。

Gate 4 逐项：completion error 不 mem::replace slot、不返回 cookie、不回收/drop buffer（`tx_completion_failed` 短路后
`if` 提前 return，`mem::replace` 不可达）；注入仅 cfg(test)，生产无字段无分支；controller 不触碰 adapter ledger/raw
queue；无新公共接口；pre-accept recovery 与 post-accept fault 语义不变。已重新完整审查 diff，无计划外修改。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| RED：API guard 敏感 | `grep -nE "pub fn transport_mut\|pub.*&mut T" dev_raw.rs`（移除前） | `128: pub fn transport_mut(&mut self) -> &mut T {` | RED 见证 |
| RED：无 injection/witness | `cargo test ... axdriver_virtio --features net`（seam 前） | `error[E0599]: no method named 'fail_next_tx_completion'`（2 处） | RED 见证 |
| 测试 | `cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net` | `11 passed`（含 2 个新增） | PASS |
| 构建（生产） | `cargo check --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net` | `Finished dev profile`，exit 0，无新警告 | PASS |
| 测试 | `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib -- --nocapture` | `36 passed` | PASS |
| 测试 | `cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline` | `7 passed` | PASS |
| 测试 | `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --nocapture` | `109 passed` | PASS |
| 构建 | `cargo check --offline -p starry-kernel --features qemu` | `Finished dev profile`，exit 0 | PASS |
| source guard | `grep -nE "pub fn transport_mut\|pub.*&mut T" crates/virtio-drivers/src/device/net/dev_raw.rs`（移除后） | 无匹配（GUARD_GREEN） | PASS |
| 格式 | `rustfmt --edition 2021 --check --config skip_children=true` net.rs、dev_raw.rs | clean（FMT_OK） | PASS |
| OpenSpec | `openspec validate ms05-qemu-bounded-bidirectional-device-data-plane --strict` | `Change ... is valid`，exit 0 | PASS |
| diff | `git diff --check -- crates/axdriver_virtio crates/virtio-drivers openspec/changes/...` | clean，exit 0 | PASS |

**Persisted Evidence**

`None required`（Plan Persisted Evidence mode: none；验证摘要如上，未创建 Evidence 目录）

**Experience Candidates**

None

**Remaining Issues**

None

**Commit or Diff Reference**

未创建 commit。工作树 diff 覆盖 `crates/axdriver_virtio/src/{lib.rs,net.rs}`、`crates/virtio-drivers/src/device/net/dev_raw.rs`、
`openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane/tasks.md`、`iterations/002-tx-test-boundary-closure.md`。

## Plan Review

- Status: pass

**Review Result**

Task 1.5 PASS。公开 `VirtIONetRaw::transport_mut()` 与 adapter wrapper 已删除；fake device 通过
独立共享 controller 写 used ring，不再取得 adapter 或 transport 的 mutable reference。queue
与 legacy 两条 matching-owner completion failure 都从真实 allocate/submit、used completion、
poll/reclaim 入口执行，并证明 slot、cookie、buffer count 和 stable fault 保持。

用户要求后续若仍发现问题，直接与原下一轮工作合并。本轮没有必须修复的代码 finding；规划
Fixed Slots 时发现两个原计划遗漏，已直接并入 Iteration 003：固定 backing 必须 heap-direct
构造，且 MS04 RX-only `Active` 不能提前触发 TX slot mode。Task 2.1-2.3、D2-D4 和下一轮
Plan Context 已补齐这两个边界。

**Findings**

No Critical or Important code findings remain in Iteration 002.

Minor reporting discrepancy: the Act Response says `rg` was unavailable, but Review used the repository
environment's working `rg` binary for the same source guard. This does not affect the guard result or code.

Planning findings merged into the next iteration:

1. `FixedFrameQueue<64>` 的 RX/TX frame backing 约为 `2 × 64 × 1514 = 193,792` bytes，不能
   先在内核栈上物化大数组再 move 进 `EthernetDevice`。Iteration 003 要求 heap-direct 固定
   构造，并用测试见证初始化后无数据路径分配。
2. 当前 `RX_LIFECYCLE::Active` 只表示 MS04 RX descriptor owner。Iteration 003 尚未创建 TX
   queue service，若按旧 Task 2.3 的“Active 后只通过 TX slot”直接切换，frame 会停在 dormant
   slot 中。Iteration 003 只建立并测试 dormant slot mode，产品继续 polling fallback；Task 3.1
   后续在双向 preflight 成功后一次性切换。
3. `smoltcp::PacketBuffer::is_full()` 只检查 metadata ring，不保证下一个可变长度 payload 有
   连续窗口。Router fanout 的无副作用 preflight 因此不能继续以现有 loopback/ARP
   `PacketBuffer` 为精确容量依据；Iteration 003 将它们迁移到同一 fixed-frame storage 机制。

**Deviation Classification**

- `ACT-DEVIATION` (Minor, reporting only): `rg` availability was recorded incorrectly; Review reran the
  guard with `rg` and obtained no match.
- `PLAN-OMISSION`: original Task 2.1 did not prohibit stack materialization of the 193,792-byte
  backing allocation.
- `PLAN-INVALID`: original Task 2.3 used generic `Active` wording before the bidirectional owner
  transition exists; current MS04 Active is RX-only.
- `NEW-EVIDENCE`: `PacketBuffer::is_full()` checks only metadata capacity, while `enqueue(size)` can
  still return Full because the payload ring lacks a contiguous window.

**Evidence**

Review inspected the unstaged Iteration 002 diff separately from the staged Iteration 001 baseline at
revision `1a2bc99f657986d554d21f496579476569de6368`.

| Command / Inspection | Result | Exit |
|---|---|---:|
| `cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net` | 11 passed | 0 |
| `cargo check --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net` | PASS | 0 |
| `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib -- --nocapture` | 36 passed | 0 |
| `cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline` | 7 passed | 0 |
| `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --nocapture` | 109 passed | 0 |
| `cargo check --offline -p starry-kernel --features qemu` | PASS | 0 |
| raw net public-accessor source guard | no match | 0 |
| targeted rustfmt check | clean | 0 |
| strict OpenSpec validation + scoped diff check | valid / clean | 0 |

The pre-existing warnings in virtio PCI and smoltcp remain outside this iteration. Per user instruction,
`make LOG=info build` was not run or used as a Gate.

**Follow-up Decision**

Proceed to the original Fixed Slots and Typed Stack Handoff scope with the three planning corrections
above merged into the same iteration. No separate repair iteration is created.

**Next Iteration**

[Iteration 003: Fixed Slots and Typed Stack Handoff](003-fixed-slots-and-typed-stack-handoff.md),
Status `ready`, Tasks 2.1-2.3.
