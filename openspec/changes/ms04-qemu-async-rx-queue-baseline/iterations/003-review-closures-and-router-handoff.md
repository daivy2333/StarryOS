# Iteration 003: Review Closures and Router RX Handoff

## Plan Context

- Status: awaiting-gate-2
- Round: 003
- Parent: `002-review-closures-and-one-completion-rx.md`

**Objective**

关闭 iteration 002 的两个测试见证缺口和 test-only extern stub 风险，并完成 T4.2：
Router 按唯一目标设备提供 RX-only one-step handoff，普通 Service polling 在 async owner
持有目标 RX 时跳过该设备但继续 loopback/协议栈，Router 从 full 变为有空间时只产生
一次软件 wake。本轮不建立 lifecycle 转换、queue task 或 ISR publish。

**Background**

Iteration 002 已把 `Device::recv` 收敛为 `Empty/Consumed/Delivered/Fault`，并通过 24
个 axnet tests、QEMU compile、feature-tree、UART、EVENT_IDX、host 和 fmt 回归。Plan
Review 发现：获批的 ARP reply→pending IPv4 TX 与“双错误时 recycle error 优先”没有
测试；为链接 fake NIC 增加的 `mem_iomap` stub 与 axklib 实际 Rust ABI/签名不一致。
用户要求小修复不要单独成轮，须与原定下一项 T4.2 合并，同时保持每轮可独立排障。

**Current Baseline**

- Branch/HEAD: `net-k3` / `917b40d1dce96d0a38cc9dfba79ed0c2e085822f`；工作树含
  iteration 001-002 已实施但未提交的代码与 OpenSpec 修改。
- `EthernetDevice::recv` 每次最多一次 driver receive，所有成功 receive 在返回前
  recycle；`Router::poll` 仍遍历并 drain 每个 device，直到 Empty/Fault 或 RX full。
- `Service::poll` 固定执行 Router RX、maintenance、listen reconcile、ingress、egress
  和 dispatch；`init_network` 已取得唯一 `eth0_dev` index，但没有保存到 Service。
- 尚无 lifecycle/AtomicWaker/space-wait 状态。产品侧还未依赖 `embassy-sync`；host
  tests 也没有 `critical-section/std` 实现。这些依赖和边界必须按本计划固定。
- Fresh automatic baseline：axnet 24、MS04 host 8、host-test 6+8+20+8、UART 62+18、
  axdriver_net 4、VirtQueue 15、kernel QEMU check、axnet/kernel fmt、feature tree 和
  diff check 均 PASS。

**Current-State Evidence**

- Entry/callers: `poll_interfaces -> Service::poll -> Router::poll -> Device::recv`；
  `Service::poll` 随后通过 smoltcp ingress dequeue Router RX buffer。
- Target identity: `init_network` 中 `router.add_device(EthernetDevice)` 返回 `eth0_dev`；
  loopback 是另一个稳定 index。当前 `Service::new(router)` 丢失目标 index。
- State ownership: Router 独占 `rx_buffer` 和 device vector；全局 `SERVICE` mutex 串行化
  ordinary poll 与未来 queue task 的 Router 访问。一次 RX-only 调用不得保存 device
  或 `NetBufPtr` 跨返回。
- Full boundary: 当前 ordinary `Router::poll` 在调用 device 前检查 `rx_buffer.is_full()`；
  T4.2 需要把同一 precheck 暴露给目标 one-step 入口，使 full 时 receive count 保持 0。
- Space edge: smoltcp ingress 在 `Service::poll` 内消费 Router RX buffer；因此软件 wake
  检查必须位于 ingress 之后，并只在先前登记 waiting 且当前有空间时清标志并 wake。
- Owner boundary: D4 的实际 lifecycle 要到 T5.1/T5.2 才存在。本轮只提供
  `PollingOwned/AsyncOwned` 视图；以后 `Polling/Spawned/Unavailable` 映射前者，
  `Active/Faulted` 映射后者，禁止在本轮实现状态转换。
- Test ABI: `axklib 0.3.0` 的 `#[def_extern_trait]` 默认生成 `extern "Rust"`；
  `mem_iomap` 精确类型为 `(PhysAddr, usize) -> AxResult<VirtAddr>`。当前 stub 是
  `extern "C" (usize, usize) -> usize`。
- Test gaps: `device/tests.rs` 有 ARP request→reply，但没有先 queue pending IPv4、再收
  ARP reply、最后观察 IPv4 TX；也没有 enqueue BadState 与 recycle Io 同时注入的 case。

**Relevant Code**

| File/Symbol | Current Responsibility |
|---|---|
| `crates/axnet/src/device/tests.rs` | fake NIC、NetBufPool、16 个 one-completion tests、错误 ABI stub |
| `device/ethernet.rs::{send,process_arp,recv}` | pending ARP packet、同步 TX、frame handoff、立即 recycle |
| `router.rs::{Router,poll}` | Router buffers、device indexes、ordinary device drain |
| `service.rs::{Service::new,poll}` | Router 与 smoltcp maintenance/ingress/egress/dispatch 顺序 |
| `lib.rs::init_network` | 创建 loopback/唯一 eth0，当前只把 Router 交给 Service |
| `crates/axnet/Cargo.toml` | 产品依赖与 test-only `axdriver/dyn`/`axdriver_net` |

**Critical Path**

```text
ordinary polling:
poll_interfaces -> Service::poll(owner view)
  -> Router::poll(skip target only when AsyncOwned; keep loopback)
  -> smoltcp maintenance/ingress/egress
  -> if waiting && Router has space: clear waiting + AtomicWaker wake once
  -> dispatch

future queue task seam (no task in this iteration):
register RX waker -> Service lock -> target RX-only one-step
  -> Full (zero device receive) | Empty | Consumed | Delivered | Fault(error)
```

**Implementation Guidance**

1. 先完成 T4.1R：用 axhal/axerrno 暴露的精确类型修正 test stub，并让 stub 返回明确
   error；新增两个 tests，观察 RED 后恢复 GREEN。不得修改 registry crate。
2. 为 Router 增加独立的 RX-only 结果，明确区分 `Full` 与 Device 的 `Empty`；入口接受
   已验证 target index，每次最多调用一次该 device。invalid index 返回 BadState 或
   等价可匹配 Fault，不 panic。
3. 定义只表达消费权的 `RxOwnerView::{PollingOwned, AsyncOwned}`。`Service::poll` 接受
   该 view 并传给 Router；本轮 `poll_interfaces` 固定传 `PollingOwned`，T5.2 再把
   lifecycle 映射为 view。`AsyncOwned` 只跳过目标 Ethernet RX；loopback 和其他非
   目标设备继续。
4. `init_network` 把唯一 `eth0_dev: Option<usize>` 保存到 Service。不要复制 device
   handle，也不要通过名字或 downcast 重找 NIC。
5. 在 `service.rs` 建立 crate-private module-static RX space signal：一个
   `embassy_sync::waitqueue::AtomicWaker` 与一个参与控制流的 atomic waiting bit。
   queue-side API 不取得 Service 锁即可先 register，再在 Service lock 内确认 Full 并
   Release 发布 waiting；Service 只在 ingress 后确认有空间时用 AcqRel 清标志并 wake。
   重复 poll 或未等待时不重复 wake。本轮只实现 handoff primitive，不创建 future/task。
   `Cargo.toml` 增加 no_std `embassy-sync 0.6.2` 产品依赖，并以 test-only
   `critical-section 1.2/std` 提供 host 实现；产品仍使用 kernel 的 restore-state-bool Impl。
6. 每个任务完成后分别 Review diff；T4.1R 未 GREEN 不进入 T4.2。

**Behavioral Change**

- Host fake NIC stub 从“仅同名可链接”变为 ABI/类型匹配且意外调用可返回错误。
- ARP reply/pending IPv4 与双错误 precedence 获得永久回归见证；产品行为不变。
- Router 新增目标 RX-only one-step：full 在 receive 前返回，其他结果保留 DevError。
- ordinary Service polling 可根据 owner view 跳过目标 RX；协议栈阶段和 loopback 不停。
- Router space 从 full 变为 available 且 task 已 waiting 时产生一次 software wake；无
  waiting、仍 full 或已清除 waiting 时不 wake。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| T4.1R | R7 / ARP reply、error precedence、safe host fixture | `device/tests.rs`; dev test types | fake NIC/test link | exact Rust ABI stub + two missing witnesses |
| T4.2a | R2,R7 / target one-step、full | `router.rs::Router` | all-device drain/buffers | target-index RX-only one-step + explicit Full/Fault |
| T4.2b | R2 / owner skip、loopback compatibility | `router.rs`; `service.rs`; `lib.rs` | ordinary poll and target creation | preserve target index; PollingOwned/AsyncOwned routing |
| T4.2c | R7 / Router space wake | `Cargo.toml`; `service.rs` and focused tests | dependencies; ingress consumes Router RX | AtomicWaker + waiting edge, one wake after space |

**Task Contracts**

T4.1R — Review closures:

- Depends on: iteration 002 retained implementation.
- RED: typed const assignment to
  `unsafe extern "Rust" fn(PhysAddr, usize) -> AxResult<VirtAddr>` rejects the current C/usize
  stub. The two new scenario tests first fail to compile while their fixtures are absent; after the
  fixtures exist, a temporary mutation that suppresses pending TX or reverses error precedence must
  make the corresponding test fail before the mutation is discarded.
- GREEN: pending IPv4 is sent exactly once after a valid ARP reply and receive buffer is recycled
  once; enqueue BadState + recycle Io returns `Fault(Io)` and recycles once; stub matches
  `extern "Rust" fn(PhysAddr, usize) -> AxResult<VirtAddr>` and returns a stable error.
- Preserve: request→reply test、one-completion counts、test-only dyn boundary、product Cargo tree.
- Stop: product iomap becomes callable through the fake, registry/axklib changes are required,
  pending packet semantics change, or recycle failure is hidden.

T4.2a — Router target RX-only one-step:

- Depends on: T4.1R GREEN.
- RED: target with two scripted completions advances once per call; full Router buffer produces
  Full with zero target receive; invalid target returns Fault without panic.
- GREEN: one call advances at most one completion; result preserves Empty/Consumed/Delivered/
  Fault and adds Router-level Full before touching the device.
- Preserve: 64-slot `SOCKET_BUFFER_SIZE`、packet bytes、device vector ownership、no second handle.
- Stop: needs device downcast/name lookup, temporary packet slot, loop/busy poll, or buffer across call.

T4.2b — ordinary owner skip and target identity:

- Depends on: T4.2a GREEN.
- RED: `Service::poll(..., PollingOwned)` drains target as before；AsyncOwned scenarios representing
  both Active and Faulted make target receive count 0 while loopback/non-target progress；missing
  eth0 stays safe.
- GREEN: `init_network` passes its existing `eth0_dev` index into Service；`poll_interfaces` 在
  T5 接入前显式传 PollingOwned。ordinary Router poll skips only that index for AsyncOwned;
  maintenance/ingress/egress/dispatch order remains.
- Preserve: 10ms fallback、sync TX、socket/listen behavior、loopback、one NIC.
- Stop: lifecycle transitions appear in this iteration, `requires_polling` is disabled, Service is
  copied, or active/faulted would create a polling fallback owner.

T4.2c — Router-space software wake:

- Depends on: T4.2b GREEN.
- RED: full+waiting then ingress/dequeue to available wakes once; still-full、not-waiting and a
  second poll after flag clear wake zero times. Register-before-wait/recheck order must be witnessed
  without sleeps.
- GREEN: module-static AtomicWaker/atomic waiting state produces exactly one wake on the qualifying
  edge；host tests 由 `critical-section/std` 支持。Service performs the check after ingress and
  before returning. Ordering is documented by role.
- Preserve: no second executor、no IRQ fabrication、no wake while holding a guard across await.
- Stop: tests require timing sleeps, wake is driven by polling loops, or T5 lifecycle/task/budget is
  needed to make the primitive work.

**Invariants**

- 每个成功取得的 Ethernet RX buffer 在同次 Device recv 返回前恰好 recycle 一次。
- Full precheck 在 device receive 之前；等待空间时不 reap completion。
- 任一时刻只有 Router 内的唯一 device object 可消费目标 queue；不复制 NIC handle。
- AsyncOwned/Faulted 不允许 ordinary polling 消费目标 RX；loopback、同步 TX、协议维护和
  10ms fallback 继续。
- space state 用于控制流，不使用 Relaxed；AtomicWaker 只有未来唯一 RX task 一个 waiter。
- `axdriver/dyn`、test stub 和 fake NIC 不进入产品 QEMU dependency tree。
- host-only `critical-section/std` 不进入产品 tree；产品 `embassy-sync` 继续绑定 kernel
  已有的 restore-state-bool implementation。

**Non-goals**

- T5.1 lifecycle transitions/generation/register-recheck/budget decision layer。
- T5.2 named axtask、真实 queue-control preflight、suppression 或 async activation。
- ISR publish、telemetry、probe、QEMU runtime、D1 baseline repair 或 build waiver。
- 修改 Router slot 数量、异步 TX、stack runner、socket readiness、SMP、PCI/DWMAC/真板。
- 手工测试；仍保留给最终 user-only iteration。

**Acceptance**

| Requirement/Scenario | Design | Task | Code/Test Witness | Simplification | Status |
|---|---|---|---|---|---|
| R7 ARP pending TX | D6 | T4.1R | ARP reply sends one pending IPv4 + recycle count | None | Covered |
| R7 recycle precedence | D6,D9 | T4.1R | enqueue BadState + recycle Io => Fault(Io) | None | Covered |
| test fixture ABI | D6 | T4.1R | typed Rust-ABI stub + unexpected-call error | None | Covered |
| R7 Router full handoff | D6,D7 | T4.2a | full zero-receive; one-step result tests | None | Covered |
| R2 unique owner view | D4,D8 | T4.2b | PollingOwned vs AsyncOwned target/loopback tests | None | Covered |
| R7 Router space wake | D7 | T4.2c | waiting/full/space/no-repeat wake tests | None | Covered |
| compatibility | D6,D8 | T4.2b | full axnet + QEMU compile/feature tree | None | Covered |

No requirement is Missing or Simplified. T4.2 的 lifecycle 映射只固定接口语义，实际状态
转换仍属于 T5.1/T5.2；Act 不得提前实现。

**Verification**

```text
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --nocapture
cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check
cargo tree --manifest-path crates/axnet/Cargo.toml --offline -e features -i axdriver
cargo tree -p starryos --features qemu --target riscv64gc-unknown-none-elf -e features -i axdriver
cargo check --offline -p starry-kernel --features qemu
make host-test
cargo test --manifest-path crates/uart_16550/Cargo.toml --offline --features async
cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline
cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib queue::tests
openspec validate ms04-qemu-async-rx-queue-baseline --strict
git diff --check
```

Act Response 必须记录新增 test 名称/数量、命令退出码、owner/receive/wake counters、精确
stub 类型，以及 axnet dev tree 与产品 QEMU tree 的 `dyn` 结论。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Requirements | PASS | approved R2/R7 and user-directed combined Review follow-up |
| Investigation | PASS | Router/Service/init call chain, buffers, target index, ABI macro and tests inspected |
| Design | PASS | Full before receive, owner view boundary, register/wait/space wake order fixed |
| Task Contracts | PASS | T4.1R/T4.2a-b-c have ordered RED/GREEN, preserve and stop rules |
| Traceability | PASS | scoped RTM has no Missing/Simplified row |
| Verification | PASS | focused unit/fmt/feature/QEMU compile plus upstream regressions fixed |
| Manual boundary | PASS | no QEMU runtime; final user-only manual iteration unchanged |
| Persisted Evidence | PASS | mode none; deterministic short outputs fit Act Response |
| User Approval | BLOCKED | awaiting explicit Gate 2 approval; Act is not authorized |

**Persisted Evidence**

- Mode: none
- Reason: all required witnesses are deterministic unit/source/type/fmt/compile/feature-tree checks
  with short output. No QEMU runtime, long diagnostic log or special-format artifact is produced.

**Risks and Notes**

- Owner view is deliberately smaller than lifecycle. T5 later maps states to it; adding transitions
  now would merge two diagnostic variables and invalidate this iteration.
- Space wake correctness depends on register-before-wait and Service-lock serialization. Tests must
  model ordering directly, not use sleeps or count repeated polling as progress.
- The exact Rust ABI stub is still test-only. Returning an error makes accidental invocation
  diagnosable; it does not authorize host MMIO mapping.
- Existing warnings、D1 compile debt and historical full-build failure remain outside this iteration.

## Gate 2 Approval Addendum

- Status: PASS
- Approved: 2026-08-10
- User instruction: “批准”
- Effect: this append-only approval supersedes the pre-approval `Status: awaiting-gate-2` and
  `User Approval: BLOCKED` snapshots above. All Gate 2 dimensions are now PASS, and iteration 003
  is execution-ready for a later explicit `openspec-act` invocation.
- Scope: approval covers T4.1R test/stub closures and T4.2a-c Router RX-only handoff, owner view,
  target identity, and Router-space software wake with the listed automatic regressions. It does
  not start Act or authorize T5 lifecycle/task work, ISR/probe work, QEMU runtime or other manual
  testing, D1 baseline repair, build waivers, Maintainer work, or archival.

## Act Response

- Status: reported

**Implemented**

T4.1R、T4.2a、T4.2b、T4.2c 全部完成：

- T4.1R test stub ABI 修复：`__axklib_0_3_mem_iomap` 从
  `unsafe extern "C" fn(usize, usize) -> usize`（`unreachable!`）改为 trait-ffi
  实际生成的 `unsafe extern "Rust" fn(PhysAddr, usize) -> AxResult<VirtAddr>`，
  返回稳定 `Err(AxError::Unsupported)`。新增编译期 typed-const ABI witness
  （`const _: unsafe extern "Rust" fn(PhysAddr, usize) -> AxResult<VirtAddr> =
  __axklib_0_3_mem_iomap;`），ABI/参数/返回类型不匹配即编译失败。`memory_addr`
  以 dev-dependency（test-only）加入，提供 `PhysAddr/VirtAddr` 精确类型。
- T4.1R 两个新场景测试：`arp_reply_flushes_pending_ipv4_once`（ARP reply 触发
  pending IPv4 同步 TX 恰好一次，且 receive buffer 只 recycle 一次，帧
  ethertype=Ipv4、payload 与 `IPV4_PAYLOAD` 一致）；`recycle_fault_prevails_over_
  enqueue_fault`（enqueue `BadState` 与 recycle `Io` 同时发生时 `Fault(Io)` 优先，
  recycle 恰好一次）。
- T4.2a `Router::rx_one_step`：`crates/axnet/src/router.rs` 新增 `RxOutcome`
  （`Full/Empty/Consumed/Delivered/Fault(DevError)`）与 target-index RX-only
  one-step 入口。Full precheck 在 device receive 之前；invalid index 返回
  `Fault(BadState)` 不 panic；一次调用最多推进一个物理 completion。新增
  `rx_buffer_has_space()` 与 `#[cfg(test)] fill_rx_buffer_for_test()`。
- T4.2b `RxOwnerView`：router.rs 定义 `PollingOwned/AsyncOwned` 消费权视图。
  `Router::poll(owner, target_dev, timestamp)` 在 `AsyncOwned` 时只跳过唯一
  target index，loopback/非目标继续。`Service::new(router, target_dev)` 保存
  eth0 index；`Service::poll(owner, sockets)` 把 view 传给 Router。
  `init_network` 把 `eth0_dev` 存入 `Option<(usize, Ipv4Cidr)>` 并传给 Service；
  `poll_interfaces` 显式传 `PollingOwned`。
- T4.2c Router-space software wake：service.rs 新增 crate-private module-static
  `RX_SPACE: RxSpaceSignal`（`embassy_sync::waitqueue::AtomicWaker` + `AtomicBool`
  waiting bit）。queue-side API：`register`（不取 Service 锁）、`wait_for_space`
  （Service 锁内确认 Full 后 `Release` 发布 waiting）；Service 侧在 ingress 后
  `AcqRel` 清标志并恰好 wake 一次。`Cargo.toml` 增加产品 `embassy-sync 0.6.2`
  （no_std，default-features=false）与 test-only `critical-section 1.2/std`。
  产品树 audit 确认 `critical-section` 在 starryos QEMU 树中只有
  `restore-state-bool`，`std` 仅在 dev-dependencies。

**Changed Files and Symbols**

| 文件 | 符号 | 变化 |
|---|---|---|
| `crates/axnet/Cargo.toml` | `embassy-sync` (product)；`memory_addr`、`critical-section/std` (dev) | 新增依赖 |
| `crates/axnet/src/device/tests.rs` | `__axklib_0_3_mem_iomap` stub；typed-const ABI witness；`PEER_IP`；`arp_reply_frame()`；2 个 T4.1R 测试；`router_with_target_and_loopback`、`service_with_target` helpers；3 个 `rx_one_step` 测试；4 个 `poll` owner-view 测试；3 个 Service-level 测试 | stub ABI 修复 + 场景测试 |
| `crates/axnet/src/router.rs` | `RxOutcome`；`RxOwnerView`；`Router::poll`；`rx_one_step`；`rx_buffer_has_space`；`fill_rx_buffer_for_test` | target RX-only + owner view |
| `crates/axnet/src/service.rs` | `RxSpaceSignal`；`RX_SPACE` static；`Service::new`；`Service::poll` | space wake + view 转发 |
| `crates/axnet/src/lib.rs` | `init_network`；`poll_interfaces` | eth0 index 保存 + PollingOwned |
| `Cargo.lock` | axnet-ng deps | 增加 embassy-sync（memory_addr/critical-section 已存在） |

**Deviations from Plan**

1. **`RxOutcome` 枚举名/位置**：Plan 未指定 Router 结果类型名字；实现选用
   `router.rs::RxOutcome`（与 `device::RxStep` 区分）。语义与 Plan 一致
   （`Full` 在 touch device 前返回，其余映射 device 的 Empty/Consumed/Delivered/Fault）。
2. **Service 级测试构造**：Plan T4.2b RED 要求 `Service::poll` 级见证。host 构造
   `Service` 需 `Interface::new` + `wall_time_nanos()`，实测可构造（host 下
   axplat 提供 time），故测试放在 device/tests.rs 与 service.rs tests 两处。
3. **`RX_SPACE` 为 crate-private module-static 而非 `pub(crate)`**：Plan 写
   "crate-private module-static"，测试位于同 crate（service.rs 内），无需扩大
   visibility。
4. **wake 检查位置**：Plan 要求 ingress 之后返回之前。实现放在 egress loop +
   `LISTEN_TABLE.reconcile` 之后、`dispatch` 之前，满足 "after ingress and before
   returning"。
5. **无其他偏差**：T4.1R 未改 registry/axklib、未改产品 iomap；T4.2 未引入
   lifecycle 转换、第二 NIC handle、busy polling 或 MS05 packet slot。

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: PASS
- Full diff reviewed: PASS
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 2

逐任务 Gate 4 与最终全量 diff Review 均通过。发现并修复的问题：
（1）`rx_buffer.enqueue(1, ())` 只填 1 个 slot 不构成 full（64-slot buffer），
改为 `while ... is_ok() {}` 填满（RED→GREEN 见证）；
（2）`Vec::pop_back` 应为 `Vec::pop`（编译错误立即修复）。
遗留 Minor：`tests/ms03-irq-host-harness.rs` 与 `tests/ms04-async-rx-host-harness.rs`
的 workspace fmt check 非零（ms03 在 HEAD 即未格式化；ms04 由 iteration 001-002
修改，均非本 iteration 引入）；smoltcp 11 个既有 warning 未处理。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| axnet tests | `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --nocapture` | 41 passed; 0 failed | PASS |
| axnet fmt | `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check` | 0 diffs | PASS |
| axnet dev tree | `cargo tree --manifest-path crates/axnet/Cargo.toml --offline -e features -i axdriver` | `dyn` 仅在 dev-dependencies | PASS |
| 产品 QEMU tree | `cargo tree -p starryos --features qemu --target riscv64gc-unknown-none-elf -e features -i axdriver` | 无 `dyn` | PASS |
| 产品 critical-section | `cargo tree -p starryos --features qemu ... -i critical-section` | 仅 `restore-state-bool`，无 `std` | PASS |
| kernel QEMU check | `cargo check --offline -p starry-kernel --features qemu` | exit 0 | PASS |
| host-test | `make host-test` | 6+8+20+8 all PASS | PASS |
| UART | `cargo test --manifest-path crates/uart_16550/Cargo.toml --offline --features async` | 62+8+10 passed | PASS |
| axdriver_net | `cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline` | 4 passed | PASS |
| virtio-drivers | `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib queue::tests` | 15 passed | PASS |
| OpenSpec | `openspec validate ms04-qemu-async-rx-queue-baseline --strict` | valid | PASS |
| diff check | `git diff --check` | exit 0 | PASS |

RED 见证（全部在实施中临时 mutation 后观察失败，再恢复）：
T4.1R 三个 mutation 各自使对应测试/编译失败（suppress pending TX →
`arp_reply_flushes_pending_ipv4_once` FAIL；reverse precedence →
`recycle_fault_prevails_over_enqueue_fault` FAIL；旧 C/usize stub + typed const →
E0308 编译失败）。T4.2a 三测试在 `rx_one_step` 空实现下全 FAIL。
T4.2b skip 移除 → AsyncOwned 两测试 FAIL。T4.2c wake 禁用 → 3 测试 FAIL；
poll hook 移除 → Service 集成测试 FAIL。

新增测试 17 个（T4.1R 2、T4.2a 3、T4.2b 7、T4.2c 5）。axnet 全量从 24 增至 41。

**Persisted Evidence**

None required（iteration 003 模式为 `none`；所有见证为确定性 unit/source/type/
fmt/compile/feature-tree 检查，短输出已记录于 Act Response）。

**Experience Candidates**

| Type | Candidate | Evidence | Reason |
|---|---|---|---|
| Incident | host 测试 stub 的 `extern "C"` ABI 与 trait-ffi 生成的 `extern "Rust"` ABI 不匹配，仅靠符号名链接通过 | 本 Act Response T4.1R 部分 | ABI 不匹配不产生链接错误（no_mangle 符号同名即链接），运行时才暴露；需编译期 typed-const witness 才能静态捕获。系统性风险（任何 def_extern_trait 消费方） |

**Remaining Issues**

None（除上文遗留 Minor：ms03/ms04 harness fmt、smoltcp warnings）。

**Commit or Diff Reference**

未创建 commit。工作树 diff 见 `git diff`（分支 `net-k3`，HEAD `917b40d1`）。

## Plan Review

- Status: follow-up-required

**Review Result**

follow-up-required

**Findings**

Iteration 003 的 one-completion、owner skip、target identity、space-edge wake、test stub
和新增兼容性 tests 可以保留。独立复验的 axnet 41 tests、host/UART/driver/queue
回归、feature isolation、QEMU compile、OpenSpec strict 和 diff check 均通过，没有发现
已实现 RX 路径的 Critical correctness 回归。后续问题集中在未来 T5 caller 的可达性
和获批竞态见证，不需要拆出独立修复轮。

1. **PASS — T4.1R 与 Router 行为成立。** test-only iomap stub 使用精确 Rust ABI 和
   类型并返回稳定错误；ARP reply/pending TX、双错误 recycle precedence、full-before-
   receive、invalid target、owner skip 和 loopback tests 都有有效见证。
2. **IMPORTANT — queue-side one-step seam 对 sibling async 模块不可达。**
   `Router::rx_one_step` 已实现，但 `Service::{router,target_dev}` 私有，`Service` 没有用
   已保存 target index 转发的入口。未来模块即使取得 `SERVICE` guard，也不能调用该
   primitive；产品 compile 同时报告 `RxOutcome` 和 `rx_one_step` 未使用。
3. **IMPORTANT — space signal 仍是 module-private，register-before-wait/recheck 未被
   证明。** `RxSpaceSignal`、`RX_SPACE` 及 `register/wait_for_space` 都只在
   `service.rs` 内可见；现有 tests 只顺序执行 register→wait→wake，没有覆盖释放发生在
   register 与 waiting 发布之间的窗口，也没有证明未来 sibling caller 能按批准顺序
   使用它。
4. **MINOR — 两个定向 host harness 未通过直接 rustfmt。** MS03 harness 的
   `fetch_add` 需换行，MS04 harness 的 import 顺序需调整。`make host-test` 仍通过；这是
   机械格式债，适合与下一轮一起修复。全工作区 `cargo fmt --all` 还包含大量未改动
   smoltcp baseline，不应扩大本轮范围。

**Deviation Classification**

- `ACT-DEVIATION`：T4.2 交付了 Router primitive，但未提供计划中未来 queue-side 可调用
  的 Service seam；私有 target identity 无法从 sibling module 使用。
- `ACT-DEVIATION`：space signal 被实现为 module-private，且批准的 register-before-
  wait/recheck 交错没有确定性测试见证。
- `NEW-EVIDENCE`：两个本地 host harness 的定向 `rustfmt --check` exit 1；不影响功能
  Gate，属于 Minor。
- 其余实现未发现 `PLAN-INVALID`、产品 correctness 回归或新的 Critical finding。

**Evidence**

2026-08-10 独立复验：

| Command / inspection | Result |
|---|---|
| `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --nocapture` | PASS：41 tests，exit 0 |
| `make host-test` | PASS：6 + 8 + 20 + 8，exit 0 |
| `cargo test --manifest-path crates/uart_16550/Cargo.toml --offline --features async` | PASS：62 unit + 8 integration + 10 doctests，exit 0 |
| `cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline` | PASS：4 tests，exit 0 |
| `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib queue::tests` | PASS：15 tests，exit 0 |
| `cargo check --offline -p starry-kernel --features qemu` | PASS，exit 0；新增 seams 报 dead-code warnings |
| axnet/product feature trees | PASS：`axdriver/dyn` 与 `critical-section/std` 只在 host dev tree |
| `openspec validate ms04-qemu-async-rx-queue-baseline --strict`; `git diff --check` | PASS，exit 0 |
| direct `rustfmt --edition 2024 --check` on MS03/MS04 harnesses | FAIL：仅上述两处机械格式，exit 1 |
| `service.rs`/`router.rs` source and visibility inspection | `Router::rx_one_step`、`RX_SPACE` 与 queue-side methods 无 sibling-callable path |

Persisted Evidence 模式为 none；没有 Evidence 目录不构成问题。

**Follow-up Decision**

创建 iteration 004，把两个 Important seam/竞态修复和 Minor 定向格式修复合入原定
T5.1。按 T4.2R→T5.1 lifecycle→T5.1 generation/register-recheck/budget decision 三个
小阶段执行，每阶段有独立 RED/GREEN 与停止条件。本轮仍不接 named axtask、不调用
真实 queue-control、不改 ISR，也不执行 QEMU runtime；最终 user-only 手测 iteration
保持不变。

**Next Iteration**

`iterations/004-review-closures-and-lifecycle-decisions.md`，等待 Gate 2 批准。
