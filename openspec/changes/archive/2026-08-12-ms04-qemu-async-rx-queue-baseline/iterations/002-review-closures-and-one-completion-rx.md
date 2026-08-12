# Iteration 002: Review closures and one-completion RX

## Plan Context

- Status: awaiting-gate-2
- Round: 002
- Parent: `001-critical-section-witness-closure.md`

**Objective**

先关闭 iteration 001 的两个 Review 缺口，再完成原定 T4.1：为 critical-section
production glue 增加永久 source guard，修复 kernel manifest 的 4 个机械 rustfmt
偏离，并把 axnet `Device::recv() -> bool` 改成一次调用最多推进一个物理 RX completion
的明确结果。本轮不实现 Router owner/space wake、异步 RX task、ISR publish 或 QEMU
手测。

**Background**

Iteration 001 已让生产 `KernelCriticalSection` 与 host harness 复用同一
`critical_section_policy::acquire/release` seam，6 个唯一场景及 QEMU/UART 回归通过。
Plan Review 发现现有 tests 不能防止生产 glue 将来重新内联 restore 决策；kernel
manifest fmt check 也在 4 个既有文件上稳定失败。

用户要求不要为这些小修单独创建 iteration，而是与原定下一轮工作合并。T4.1 是
async budget 的直接前置：当前 `EthernetDevice::recv` 会循环消费 ARP、malformed、
非目标和非 IPv4 frame，直到交付 IPv4 或 driver 为空，因此未来 task 无法按物理
completion 计数。

**Current Baseline**

- Revision: `917b40d1dce96d0a38cc9dfba79ed0c2e085822f`
- Branch: `net-k3`
- Worktree: iteration 001 尚未提交；修改
  `kernel/src/drivers/critical_section_policy.rs`、`kernel/src/lib.rs`、
  `tests/ms04-async-rx-host-harness.rs` 和 iteration 001 文档。002 必须在其上继续，
  不得回退或重写 001 的 Act Response/Plan Review。
- T1.1-T2.2 已完成。T3 host/QEMU/UART 见证已收紧；D1 的 7 个既有编译错误仍使
  tasks T3.1 保持未完成。
- Fresh baseline：MS04 6 unique、host 6+8+20+6、UART 62+18、T1 4、T2 15、
  kernel QEMU check、axnet 8 和 OpenSpec strict 全 PASS。
- `cargo fmt --manifest-path kernel/Cargo.toml -- --check` exit 1；rustfmt 只报告
  `drivers/mod.rs`、`drivers/uart_init.rs`、`drivers/virtio_net_irq.rs`、
  `syscall/fs/ctl.rs` 的 module/import 排序、换行和缩进。
- `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check` exit 0。

**Current-State Evidence**

Critical-section witness:

- `kernel/src/lib.rs::critical_impl::KernelCriticalSection` 当前调用
  `critical_section_policy::acquire(&AxhalIrqOps)` 和
  `critical_section_policy::release(&AxhalIrqOps, restore_state)`。
- harness 通过 `#[path]` 引入同一 seam，只测试 seam 行为；没有读取或断言真实
  production Impl caller。
- 直接 axhal IRQ primitive 在 `AxhalIrqOps` backend 中是合法的。source guard 必须
  只检查 Impl 方法体，不能对整个 `critical_impl` 模块禁止这些调用。

RX data path:

- `poll_interfaces -> Service::poll -> Router::poll`。
- `Router::poll` 对每个 device 执行
  `while !rx_buffer.is_full() && dev.recv(...) {}`；bool 只表达是否交付 IP packet。
- `EthernetDevice::recv` 内部 `loop`。每次 driver `receive()` 后解析 frame，随后
  `recycle_rx_buffer(rx_buf).unwrap()`；未交付 frame 会立即继续下一次 receive。
- `handle_frame` 对 IPv4 使用 `PacketBuffer::enqueue(...).unwrap()`；虽然当前 caller
  先检查 buffer 未满，接口本身仍会在容量不一致时 panic，无法返回 queue fault。
- driver `Again` 返回 false；其他 receive error 只打印后返回 false，错误类别丢失。
- recycle error 会 panic。已取得的 `NetBufPtr` 在调用 recycle 时转交 driver，不能
  留到下一次 poll 或 future。
- `LoopbackDevice::recv` 每次最多 dequeue 一项，但也用 bool 表达 Empty/Delivered，
  destination enqueue 失败会 unwrap。
- T4.2 尚未开始；本轮只机械适配 `Router::poll` 这一现有 caller，不增加 target
  device index、owner skip、RX-only service 或 space wake。

Testability:

- axnet crate 当前只有 8 个 service deadline tests，没有 device/frame tests。
- standalone axnet tests 让 `axdriver` 使用 dummy static `AxNetDevice`。test-only
  dev-dependency 启用 `axdriver/dyn` 后，`AxNetDevice` 变为
  `Box<dyn NetDriverOps>`，可注入 fake NIC；产品 QEMU build 不启用该 test-only
  feature，仍使用静态 VirtIO device。
- 本地 `axdriver_net::NetBufPool` 可为 fake NIC 创建真实 `NetBufPtr`，并在 recycle
  后恢复 ownership。测试必须记录 receive/recycle/TX 次数及可注入错误。

**Behavioral Change**

Current:

```text
Device::recv -> bool
Ethernet recv -> receive/parse/recycle in an unbounded loop
Again/error -> false; recycle/enqueue error -> panic
Router poll -> continue only when an IP packet was delivered
```

Target:

```text
Device::recv -> Empty | Consumed | Delivered | Fault(DevError)
Ethernet recv -> at most one driver receive and one matching recycle
malformed/foreign/non-IPv4/ARP -> Consumed
IPv4 enqueue + recycle -> Delivered
Again -> Empty
receive/recycle/enqueue-state error -> Fault(error)
Router polling -> continue on Consumed/Delivered; stop on Empty/Fault
```

`Fault(DevError)` 保留错误类别，供后续 lifecycle/telemetry 使用。Router enqueue
失败映射为 `DevError::BadState`；无论 frame 分类或 enqueue 结果如何，已取得的 RX
buffer 都必须先交给一次 recycle，再返回最终结果。若 recycle 也失败，recycle error
优先成为最终 Fault，因为 descriptor refill 已失败。

**Change Surface**

| Task | Requirement | File/Symbol | Planned Change |
|---|---|---|---|
| T3.2 | R3/M4 production binding | `tests/ms04-async-rx-host-harness.rs` | actual Impl source guard + legacy negative fixture |
| T3.3 | R8 automatic Gate | four kernel files reported by rustfmt | mechanical fmt only |
| T4.1 | R2/R7 one completion | `crates/axnet/Cargo.toml`; `device/{mod,ethernet,loopback}.rs`; `router.rs::poll` | explicit RX result, fake NIC tests, caller adaptation |

**Task Contracts**

T3.2 — permanent production caller guard:

- Depends on: accepted iteration 001 implementation.
- RED：新增 pure guard helper 和 legacy direct-call fixture；fixture 必须被拒绝。没有
  guard/helper 时测试编译失败；只检查 seam 行为而不读取 production source 不算 RED。
- GREEN：guard 读取真实 `kernel/src/lib.rs`，定位
  `unsafe impl critical_section::Impl for KernelCriticalSection` 的方法体，要求 acquire
  和 release 分别委托被测 seam，并拒绝旧式 `was_enabled/disable_irqs` 与
  `if restore_state { enable_irqs() }` 决策复制。
- Preserve：`AxhalIrqOps` backend 内合法的 `irqs_enabled/disable_irqs/enable_irqs`
  调用；6 个现有行为场景；无新 parser crate、行号或完整文件哈希依赖。
- Verification：MS04 harness 应包含 6 个行为 tests、1 个 legacy-negative test 和
  1 个 actual-production test，全部通过；`make host-test` 通过。
- Stop：guard 只能靠匹配整个文件而误报 backend、需要修改 production glue 才能让
  当前正确源码通过，或对空/截断 Impl 返回 PASS。

T3.3 — close kernel fmt baseline:

- Depends on: T3.2 GREEN，避免格式步骤干扰行为 Review。
- RED：fresh kernel manifest fmt check exit 1，并只列出已调查的 4 个文件。
- GREEN：运行 rustfmt 后同一命令 exit 0；diff 只包含 rustfmt 的 module/import 排序、
  换行和缩进。逐文件检查 cfg attribute 仍绑定原 item，MMIO volatile 地址、宽度、
  register/telemetry/syscall 表达式 token 语义不变。
- Preserve：UART init、VirtIO IRQ cause/ACK、syscall snapshot 和 module visibility。
- Verification：kernel fmt、kernel QEMU check、host-test、MS03 harness 和 diff check。
- Stop：工具触及这 4 个文件以外的产品文件，改变 cfg 归属/常量/unsafe expression，
  或任何 compile/test 回归。

T4.1 — one-completion Device primitive:

- Depends on: T3.2-T3.3 GREEN；失败时不得进入 T4.1。
- Interface：在 `device/mod.rs` 定义可匹配的 RX step enum，Fault 携带 `DevError`；
  `Device::recv` 返回该类型。不要增加 async trait、第二 buffer API 或 transport 类型。
- Ethernet：移除内部 loop；每次最多调用一次 `inner.receive()`。`Again -> Empty`，
  其他 receive error -> `Fault(err)`。对取得的 buffer 完成一次 frame 分类和一次
  recycle；ARP 仍可同步调用现有 TX path。
- Error precedence：frame enqueue/internal state error 先记为 BadState，但仍 recycle；
  recycle 失败覆盖为 recycle Fault。禁止 unwrap、panic、silent recycle failure 或
  buffer 跨返回。
- Loopback：一次 dequeue 映射 Empty/Delivered；destination enqueue 失败返回
  `Fault(BadState)`，不 panic；不获得 queue control 或 async owner。
- Router caller：现有 polling loop 在 Consumed/Delivered 时继续，在 Empty 时结束；
  Fault 记录 error 后结束。不得加入 T4.2 的 owner state、target index 或 space wake。
- Test setup：只在 tests/dev dependency 中启用 `axdriver/dyn` 并使用本地
  `axdriver_net::NetBufPool`；产品依赖和 QEMU static device model 不变。
- RED cases：连续两个 completion 的第一次调用 receive count 必须为 1；当前循环会
  读到第二个。另覆盖 Again、receive fault、recycle fault、malformed、foreign MAC、
  非 IPv4、ARP request/reply sync TX、IPv4 delivery、full destination enqueue、
  loopback Empty/Delivered/full。
- GREEN：所有分类正确；每个成功 receive 的 recycle count 恰好为 1；连续 completion
  需两次调用；ARP TX 发生但结果是 Consumed；full/recycle errors 不 panic。
- Stop：需要修改 registry `axdriver`、暴露 VirtIO token/ring、把 TX 改 async、持有
  NetBufPtr 跨调用、修改 Router packet slot 数量或提前实现 T4.2。

**Execution Order and Gates**

```text
T3.2 source guard RED -> GREEN -> MS04/host regression
  -> T3.3 kernel fmt RED -> mechanical GREEN -> kernel/MS03 regression
  -> T4.1 fake NIC/frame RED -> one-step GREEN -> axnet/full integration regression
  -> specs/code/full-diff Review
```

每个任务单独检查 diff。T3.2/T3.3 失败时停止，不开始 T4.1；T4.1 任一 ownership
test 失败时停止，不进入 Router T4.2。

**Invariants**

- critical-section restore 继续恢复进入前 IRQ 状态；ISR-disabled release 不 enable。
- 每个成功取得的 RX buffer 在同一次 `recv` 返回前恰好调用一次 recycle。
- 一次 Ethernet `recv` 最多调用一次 driver receive，不因 frame 类型继续 drain。
- 同步 TX、ARP neighbor/pending packet 语义和 10ms polling fallback 保持。
- Router 普通 polling 仍会 drain 当前可用工作，只是用 Consumed/Delivered 表达进度。
- test-only dynamic device model 不进入产品 QEMU dependency graph。
- T1 EVENT_IDX/RX queue control、MS03 ACK/EOI 和 UART 代码行为不变。

**Non-goals**

- 修复或豁免 D1 7 个编译错误；复判历史 `make LOG=info build` 链接失败。
- T4.2 target RX-only service、Router full wait、space wake 或 owner skip。
- lifecycle、generation、AtomicWaker、budget=32、axtask 或 ISR publish。
- QEMU 启动、guest command、sandbox 外复跑、runtime Evidence 或性能结论。
- 修改 packet slot 数量、async TX、SMP、PCI、DWMAC 或真板路径。
- 提交、归档、SNAPSHOT/global tasks 更新或无关 warning 清理。

**Acceptance and Traceability**

| Requirement/Scenario | Design | Task | Code/Test Witness | Status |
|---|---|---|---|---|
| R3/M4 production restore binding | D3 | T3.2 | legacy-negative + actual Impl source guard；6 restore tests | Covered |
| R8 automatic fmt Gate | D10 | T3.3 | kernel manifest fmt RED/GREEN；kernel/MS03 regression | Covered |
| R2 one physical progress | D6 | T4.1 | two queued frames require two recv calls | Covered |
| R7 consumed vs delivered | D6 | T4.1 | malformed/foreign/non-IP/ARP/IPv4 fake frames | Covered |
| R7 refill conservation | D6,D9 | T4.1 | receive/recycle counters；recycle error precedence | Covered |
| Compatibility: loopback/polling/TX | D6,D8 | T4.1 | loopback tests；Router caller tests；ARP sync TX | Covered |

No requirement is Missing or Simplified. Iteration acceptance requires T3.2、T3.3 and T4.1
all GREEN with no unresolved Critical/Important finding. It does not complete change task T3.1
because D1 remains unresolved, and it does not complete T4.2.

**Verification**

```text
rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test
/tmp/ms04-async-rx-host-test
make host-test
cargo fmt --manifest-path kernel/Cargo.toml -- --check
cargo check --offline -p starry-kernel --features qemu
cargo test --manifest-path crates/uart_16550/Cargo.toml --offline --features async
cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline
cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib queue::tests
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --nocapture
cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check
cargo tree --manifest-path crates/axnet/Cargo.toml --offline -e features -i axdriver
cargo tree -p starryos --features qemu --target riscv64gc-unknown-none-elf -e features -i axdriver
openspec validate ms04-qemu-async-rx-queue-baseline --strict
git diff --check
```

Feature-tree review must show `axdriver/dyn` only in the axnet test context; the StarryOS QEMU
tree must remain static VirtIO. Act records test names/counts, exact exits, changed files/symbols
and per-task diff Review in Act Response.

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Requirements | PASS | approved MS04 R2/R3/R7/R8 and user-directed combined follow-up |
| Investigation | PASS | actual critical Impl/seam, fmt diff, Device callers/errors/tests inspected |
| Design | PASS | source guard boundary, Fault error/precedence and test-only dyn model fixed |
| Task Contracts | PASS | T3.2、T3.3、T4.1 each has RED/GREEN, stops and ordered Gates |
| Traceability | PASS | scoped RTM has no Missing/Simplified row |
| Verification | PASS | host/unit/fmt/feature-tree/QEMU compile and diff commands fixed |
| Manual boundary | PASS | no QEMU/manual work; final user-only iteration unchanged |
| Persisted Evidence | PASS | mode none; short deterministic outputs fit Act Response |
| User Approval | BLOCKED | awaiting explicit Gate 2 approval; Act is not authorized |

**Persisted Evidence**

- Mode: none
- Reason: all required witnesses are deterministic host/unit/fmt/compile/source checks with short
  output. Act Response must record commands、exits、test names/counts and feature-tree conclusions.
  No QEMU runtime、long diagnostic log or special artifact is produced.

**Risks and Notes**

- Source guard is intentionally structural. It protects delegation but does not prove RISC-V CSR
  behavior; later runtime restore-violation telemetry remains mandatory.
- Test-only `axdriver/dyn` changes the device representation during axnet unit tests. The static
  QEMU compile and feature-tree audit are required to prevent that feature from leaking to product.
- Rustfmt may reorder modules/imports. Review cfg attachment and MMIO expressions after formatting;
  do not mix warning cleanup or semantic edits into T3.3.
- `DevError` lacks `PartialEq`; tests should match variants rather than changing the shared error
  type solely for assertions.
- D1 and historical QEMU build failures remain visible Gate debt; neither becomes a manual task in
  this iteration.

## Gate 2 Approval Addendum

- Status: PASS
- Approved: 2026-08-10
- User instruction: “同意。批准”
- Effect: this append-only approval supersedes the pre-approval `Status: awaiting-gate-2` and
  `User Approval: BLOCKED` snapshots above. All Gate 2 dimensions are now PASS, and iteration 002
  is execution-ready for a later explicit `openspec-act` invocation.
- Scope: approval covers T3.2 production caller guard, T3.3 four-file mechanical kernel formatting
  closure, T4.1 one-completion RX primitive, and the listed automatic regressions. It does not
  start Act or authorize T4.2, async task/ISR work, QEMU manual testing, D1 baseline repair, build
  waivers, Maintainer work, or archival.

## Act Response

- Status: reported

**Implemented**

T3.2、T3.3、T4.1 全部完成：

- T3.2 production caller guard：MS04 harness 新增 `production_guard` 纯源码
  guard。RED 为 guard 缺失时 E0433 编译失败；GREEN 后 guard 通过
  `include_str!("../kernel/src/lib.rs")` 读取真实生产源码，brace 定位
  `unsafe impl critical_section::Impl for KernelCriticalSection` 的 acquire/release
  方法体，要求两者分别委托 `critical_section_policy::acquire/release`，并拒绝
  内联 `disable_irqs/irqs_enabled/enable_irqs` 与空/截断 impl。legacy direct-call
  fixture 与 truncated fixture 均被拒绝。harness 现为 8 测试（6 行为 +
  legacy-negative + actual-production）。
- T3.3 kernel fmt closure：`cargo fmt --manifest-path kernel/Cargo.toml` 后
  `-- --check` exit 0。改动仅限 4 个预存文件
  （`drivers/mod.rs`、`drivers/uart_init.rs`、`drivers/virtio_net_irq.rs`、
  `syscall/fs/ctl.rs`），全部为 module/import 排序、换行与缩进；cfg 归属与 MMIO
  volatile 表达式 token 语义不变（逐文件人工核对）。
- T4.1 one-completion RX primitive：`device/mod.rs` 定义
  `RxStep { Empty, Consumed, Delivered, Fault(DevError) }`，
  `Device::recv` 改为此类型。`EthernetDevice::recv` 移除内部 loop，每次最多一次
  driver receive 与一次 recycle；Again→Empty、其他 receive error→Fault、
  malformed/非目标/非 IPv4/ARP→Consumed、IPv4→Delivered、enqueue 失败→
  `Fault(BadState)`，recycle error 优先覆盖。`LoopbackDevice::recv` 映射
  Empty/Delivered/`Fault(BadState)`，不再 unwrap。`Router::poll` 对
  Consumed/Delivered 继续、Empty 停止、Fault 记录后停止。
- T4.1 测试基础设施：axnet dev-dependencies 启用 test-only `axdriver/dyn` +
  `axdriver_net`；`device/tests.rs` 用 `NetBufPool` 提供真实 `NetBufPtr` 的 fake
  NIC，16 个测试覆盖 RED cases（连续两帧单步、Again、receive fault、recycle
  fault、malformed、foreign MAC、非 IPv4、ARP 同步 TX、IPv4 交付、Router enqueue
  fault、loopback Empty/Delivered/full、Router 三种停止语义）。

**Changed Files and Symbols**

| 文件 | 符号 | 变化 |
|---|---|---|
| `tests/ms04-async-rx-host-harness.rs` | `production_guard`；`check()`；`block_after()`；legacy/truncated fixtures；`PRODUCTION_SOURCE` | 新增 guard 与 2 个 guard 测试（共 8 测试） |
| `kernel/src/drivers/mod.rs` 等 4 文件 | — | 机械 rustfmt（模块/import 排序、换行） |
| `crates/axnet/Cargo.toml` | dev-deps `axdriver(dyn)`、`axdriver_net` | 新增 test-only 依赖 |
| `crates/axnet/src/device/mod.rs` | `RxStep`；`Device::recv` | 新增显式 RX step 结果类型 |
| `crates/axnet/src/device/ethernet.rs` | `handle_frame`；`Device::recv` | 单步 recv；enqueue/recycle 错误映射 |
| `crates/axnet/src/device/loopback.rs` | `Device::recv` | Empty/Delivered/`Fault(BadState)`，去 unwrap |
| `crates/axnet/src/router.rs` | `Router::poll` | RxStep 分派（continue/stop/fault log） |
| `crates/axnet/src/device/tests.rs` | `FakeNic`、`FakeStats`、`ScriptedDevice`、`__axklib_0_3_mem_iomap` stub | 新增 16 个测试与 test-only stub |

**Deviations from Plan**

1. **axklib kernel-ABI 符号 stub（测试基础设施）**：`axdriver/dyn` 拉入 `axklib`，
   host 测试二进制链接需要内核提供的 `__axklib_0_3_mem_iomap`。fake NIC 永不调用
   iomap，故在 `device/tests.rs` 提供 `#[cfg(test)]` 死代码 stub（`unreachable!`）
   仅满足链接器。不修改 registry axdriver、不影响产品构建（feature-tree 审计确认
   `dyn` 不进 starryos 树）。Plan 未预见该链接要求，属必要的机械补充。
2. **测试共享状态用 `spin::Mutex` 而非 `Cell`/`RefCell`**：`Device: Send + Sync`
   要求 fake NIC 可跨线程共享，Cell/RefCell 不满足 Sync。
3. **错误注入简化为 bool 开关**：`DevError` 无 `Copy`/`Clone`，required cases 只需
   固定错误变体（Io），故 fake 以 `receive_error/recycle_error: bool` 注入。
4. T3.3 fmt 运行与 change tasks.md 3.2/3.3/4.1 勾选按计划执行；3.1 保持未勾选。

**Blocker Handoff**

None. 无技术阻塞。

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: PASS（T3.2/T3.3/T4.1 契约、RED/GREEN、Stop 条件全部满足；
  未实现 T4.2，未触碰 UART/VirtIO/IRQ handler/平台代码）
- Full diff reviewed: PASS（产品改动仅 4 个 axnet 文件 + kernel 4 个 fmt 文件 +
  harness；无计划外行为修改）
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 1
  - axklib stub 的返回类型（`usize`）与真实 `AxResult<VirtAddr>` ABI 不同；该符号
    在测试中永不调用（`unreachable!` 兜底），链接期无影响，留作已知 Minor。

**Verification Evidence**

| 验证项 | 命令 | 输出摘录 | 结论 |
|---|---|---|---|
| T3.2 RED | `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs` | `E0433 cannot find module production_guard` | PASS（预期 RED） |
| T3.2 GREEN | 同上 + `/tmp/ms04-async-rx-host-test` | `running 8 tests ... 8 passed` | PASS |
| T3.3 RED | `cargo fmt --manifest-path kernel/Cargo.toml -- --check` | exit 1，仅 4 个预存文件 | PASS（预期 RED） |
| T3.3 GREEN | `cargo fmt --manifest-path kernel/Cargo.toml` 后 `-- --check` | exit 0 | PASS |
| T4.1 RED | `cargo test --manifest-path crates/axnet/Cargo.toml --offline --no-run` | `E0432 unresolved import RxStep` | PASS（预期 RED） |
| T4.1 GREEN | `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --nocapture` | `running 24 tests ... 24 passed` | PASS |
| host-test | `make host-test` | `6 + 8 + 20 + 8 passed`，exit 0 | PASS |
| UART 回归 | `cargo test --manifest-path crates/uart_16550/Cargo.toml --offline --features async` | 62 + 8 + 10 passed，exit 0 | PASS |
| kernel QEMU check | `cargo check --offline -p starry-kernel --features qemu` | `Finished dev profile`，exit 0 | PASS |
| T1 回归 | `cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline` | 4 passed，exit 0 | PASS |
| T2 回归 | `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib queue::tests` | 15 passed，exit 0 | PASS |
| axnet fmt | `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check` | exit 0 | PASS |
| feature-tree（axnet test） | `cargo tree --manifest-path crates/axnet/Cargo.toml --offline -e features -i axdriver` | `axdriver feature "dyn"` 存在 | PASS（test-only） |
| feature-tree（产品） | `cargo tree -p starryos --features qemu --target riscv64gc-unknown-none-elf -e features -i axdriver` | 无 `dyn`；静态 virtio-net | PASS |
| OpenSpec validate | `openspec validate ms04-qemu-async-rx-queue-baseline --strict` | `Change ... is valid`，exit 0 | PASS |
| diff 检查 | `git diff --check` | 无输出，exit 0 | PASS |

**Persisted Evidence**

`None required`（iteration 002 模式为 none：全部见证为确定性 host/unit/fmt/compile/
feature-tree/source 检查，摘要已记录于本 Act Response；无 QEMU runtime 或长日志）。

**Experience Candidates**

| Type | Candidate | Evidence | Reason |
|---|---|---|---|
| Incident | host 测试启用 `axdriver/dyn` 时 axklib 内核 ABI 符号 `__axklib_0_3_mem_iomap` 链接缺失，需 `#[cfg(test)]` dead stub | 本 Act Response（T4.1 RED→GREEN 过程） | 可复现（任何 host 测试二进制链接 axdriver/dyn 都触发）、含系统性诊断信息（undefined symbol）、需非常规恢复（test-only stub 绕过） |

**Remaining Issues**

1. D1 `lichee-d1-kbench` 7 个预存编译错误未修复/未豁免；change `tasks.md` 3.1
   保持未勾选（未误报为全 PASS）。
2. 历史 `make LOG=info build` 链接失败仍留待全量自动 Gate iteration 用原命令复判
   （继承 iteration 000/001）。
3. iteration 001/002 文档与 change tasks.md/design.md 中的 Plan 侧修改（Review、
   Gate 2 批准附录、任务描述）已保留，未覆盖。

**Commit or Diff Reference**

未提交。工作树包含本 iteration 产品改动（axnet 5 个文件、kernel 4 个 fmt 文件、
harness）与 Plan 侧文档修改；index 未做任何 staging 操作。

## Plan Review

- Status: follow-up-required

**Review Result**

follow-up-required

**Findings**

Iteration 002 的生产改动可以保留。独立审查确认 one-completion、立即 recycle、
Router polling caller 适配、production source guard 和四文件机械格式修复均符合本轮
边界；fresh host/unit/fmt/QEMU compile 回归全部通过。后续问题集中在测试见证和
test-only 链接 stub，不要求回退已有产品实现。

1. **PASS — one-completion 与 recycle 主路径成立。** Ethernet 每次最多一次
   `receive`，取得 buffer 后在返回前调用一次 `recycle_rx_buffer`；Again、普通错误、
   malformed、foreign、非 IPv4、ARP、IPv4 和 Router enqueue full 的结果映射符合
   D6。Router 对 Consumed/Delivered 继续，对 Empty/Fault 停止。
2. **IMPORTANT — 两个获批错误/兼容场景缺少永久见证。** T4.1 要求覆盖收到 ARP
   reply 后发送 pending IPv4 packet，以及 frame/enqueue error 与 recycle error 同时
   出现时 recycle error 优先。当前 16 个 device tests 只有 ARP request→reply 和
   Delivered→recycle error；这两个场景未测试。现有实现路径看起来符合预期，但 Gate
   不能用源码推断替代测试见证。
3. **IMPORTANT — test-only `mem_iomap` stub 的 ABI 和签名不匹配。**
   `axklib 0.3.0` 的 `#[def_extern_trait]` 默认声明 `extern "Rust" fn(PhysAddr,
   usize) -> AxResult<VirtAddr>`；当前测试用 `extern "C" fn(usize, usize) -> usize` 导出
   同名符号。fake NIC 当前不调用 iomap，因此 24 tests 可以通过，但未来测试一旦触发
   该路径会跨不兼容 ABI，不能保留为安全测试夹具。
4. **PASS — feature 与产品边界未泄漏。** `axdriver/dyn` 只出现在 axnet
   dev-dependency tree；StarryOS QEMU tree 保持静态 VirtIO。kernel QEMU check、UART、
   queue contract、EVENT_IDX tests、两个 fmt Gate 和 diff check 全部通过。

**Deviation Classification**

- `ACT-DEVIATION`：T4.1 没有交付获批的 ARP reply/pending TX 与双错误 precedence
  测试见证。
- `PLAN-OMISSION`：Plan 预见了 test-only `axdriver/dyn`，但没有调查其 host 链接所需
  的 `axklib` extern symbol，也没有固定 stub ABI/签名。
- `ACT-DEVIATION`：Act 为解除链接阻塞增加了同名 stub，但签名和 ABI 不匹配；其
  “永不调用”假设只解释当前测试为何通过，不能证明夹具安全。
- 其余实现未发现 Plan invalid、产品 correctness 回归或新的 Critical finding。

**Evidence**

2026-08-10 独立复验：

| Command / inspection | Result |
|---|---|
| `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --nocapture` | PASS：24 tests，exit 0；test list 证明缺少上述两个 case |
| `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-review && /tmp/ms04-async-rx-host-review` | PASS：8 tests，exit 0 |
| `make host-test` | PASS：6 + 8 + 20 + 8，exit 0 |
| `cargo test --manifest-path crates/uart_16550/Cargo.toml --offline --features async` | PASS：62 + 18 doctests，exit 0 |
| `cargo check --offline -p starry-kernel --features qemu` | PASS，exit 0；仅既有 warnings |
| `cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline` | PASS：4，exit 0 |
| `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib queue::tests` | PASS：15，exit 0 |
| axnet/kernel fmt checks；`git diff --check` | PASS，exit 0 |
| axnet/QEMU `cargo tree ... -i axdriver` | PASS：`dyn` 只在 dev tree；产品树无 `dyn` |
| `axklib-0.3.0/src/lib.rs` + `trait-ffi-0.2.11/src/lib.rs` source inspection | expected symbol uses Rust ABI and exact `PhysAddr -> AxResult<VirtAddr>` signature |

Persisted Evidence 模式为 none；没有 Evidence 目录不构成问题。

**Follow-up Decision**

创建 iteration 003，并遵循用户要求把上述小粒度修复与原定 T4.2 合并。执行顺序先
修正 test stub 并补齐 T4.1 tests，再实现 target-index RX-only Router handoff、普通
polling 的 async-owner skip 和 Router-space software wake。每组都有独立 RED/GREEN
与停止条件；本轮不进入 T5 lifecycle/task、ISR 或 QEMU 手测。

**Next Iteration**

`iterations/003-review-closures-and-router-handoff.md`，等待 Gate 2 批准。
