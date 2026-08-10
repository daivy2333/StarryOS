# Iteration 001: Critical-section production witness closure

## Plan Context

- Status: awaiting-gate-2
- Round: 001
- Parent: `000-initial.md`

**Objective**

只收紧 T3 的测试见证：让 host tests 覆盖生产 `KernelCriticalSection` 实际使用的
IRQ restore 决策 seam，移除重复场景和 dead test-only model，并复验 QEMU compile、
UART 与 host regressions。本轮不进入 axnet one-completion、Router、async task、ISR
接线或任何 QEMU 手测。

**Why This Iteration Exists**

Iteration 000 的实现把 critical-section 从手写 ABI 改成官方
`critical_section::set_impl! + Impl`，源码逻辑会恢复进入前 IRQ 状态。Plan Review
发现现有 `IrqRestorePolicy` 只是独立模拟器：生产实现没有调用它；模块内 6 个测试
又被 host harness 重复一次，因此 `12 passed` 不能证明 production glue 与模型一致。

另有两个保留问题，但不扩大本轮 scope：

- D1 `lichee-d1-kbench` 在 HEAD 与当前工作树均有相同 7 个产品编译错误。它不是
  sandbox 问题，也没有 waiver，T3.1 仍不能整体勾选；本轮不顺手修 D1 平台代码。
- `make LOG=info build` 曾出现未充分分类的链接失败，留到全量自动 Gate iteration
  用原命令重跑；本轮只要求 `cargo check --features qemu`。

**Current Baseline**

- Revision: `16d9a16a2b65a574022faaee39b465f6f7aebd45`
- Branch: `net-k3`
- T1.1-T2.2 已由 iteration 000 实施并经 Plan Review 复验。
- T3 production code：`kernel/src/lib.rs::critical_impl` 使用 bool restore state；
  acquire 读取 IRQ 状态后 disable，release 仅在 restore state 为 true 时 enable。
- 当前 seam：`kernel/src/drivers/critical_section_policy.rs::IrqRestorePolicy`，仅被编译
  为 dead kernel module，生产 `critical_impl` 不调用。
- 当前 host harness 通过 `#[path]` 包含整个 seam 文件，使 seam 自带 6 tests 和
  harness 外层 6 tests 同时执行。
- 2026-08-10 fresh checks：host 6+8+20+12、UART 62+18、kernel QEMU check 全 PASS。
- 工作区含大量 staged change 和用户文档改动；Act 只能修改本轮列出的 4 个路径。

**Change Surface**

| File | Current | Target |
|---|---|---|
| `kernel/src/drivers/critical_section_policy.rs` | 独立 state/stack 模型，生产未使用 | 无状态、可注入 IRQ operations 的 production seam |
| `kernel/src/lib.rs::critical_impl` | 直接调用 axhal IRQ primitives | 通过同一 seam 完成 acquire/release |
| `tests/ms04-async-rx-host-harness.rs` | 重复 seam 内测试 | fake IRQ backend 驱动唯一场景集 |
| `Makefile::host-test` | 执行 harness | 保持入口，只接受不重复的测试数与结果 |

**Task Contract**

T3.1a — bind host witness to production restore seam:

- Depends on: iteration 000 T3 主体实现。
- RED：先增加 source/compile witness，证明生产 `critical_impl` 必须调用被 host harness
  包含的 seam；当前代码因直接调用 axhal 而失败。
- GREEN：把 IRQ enabled/read/disable/enable 抽象成最小可注入 backend，seam 提供
  acquire/release；生产 backend 委托 axhal，host fake backend 记录状态和调用次数。
- Required cases：enabled acquire/release、disabled ISR acquire/release、两层 nesting、
  enabled 状态只在最外层恢复、disabled 状态零 enable 调用、acquire 总会 disable。
- Cleanup：删除 seam 内嵌与 harness 外层的重复组，只保留一套唯一测试；删除 dead
  `IrqRestorePolicy` stack 模型。测试数不是验收目标，唯一场景和生产绑定才是。
- Preserve：`critical-section 1.2`、`restore-state-bool`、`set_impl! + Impl`、axhal
  primitive、UART 实现和所有 T1/T2 文件不变。
- Stop：需要全局 lock、heap、atomics、平台 mock 进入产品路径，改变 critical-section
  ABI，或 host test 仍只验证与生产无关的复制逻辑。

**Invariants**

- acquire 返回进入前 IRQ enable state，并在返回前 disable IRQ。
- release(false) 绝不 enable IRQ；release(true) 恰好执行一次 enable。
- ISR 内的 `AtomicWaker::wake()` 不能提前打开 IRQ。
- 生产 backend 仍只使用 axhal 的 `irqs_enabled/disable_irqs/enable_irqs`。
- 不修改 UART ring/waker、VirtIO queue、axnet、kernel IRQ handler 或平台代码。

**Non-goals**

- 修复或豁免 D1 预存编译错误。
- 复判 `make LOG=info build` 的历史链接失败。
- T4 one-completion、Router handoff、lifecycle、axtask、ISR publish 或 telemetry。
- QEMU 启动、guest shell、sandbox 外命令、Runbook 或 Evidence 收集。
- 清理工作区 index、提交代码或维护 SNAPSHOT/global tasks。

**Acceptance**

- 生产 `KernelCriticalSection::{acquire,release}` 调用 host tests 覆盖的同一 seam。
- 只有一套唯一 restore 场景；不再用重复测试数放大覆盖结论。
- enabled、disabled/ISR、nested 和调用次数语义全部有确定性 RED/GREEN witness。
- QEMU kernel check、UART unit/doctest、完整 host-test 与 T1/T2 regressions 全部退出 0。
- diff 只包含本轮 4 个允许路径，且没有未解决 Critical/Important finding。
- 本轮完成后只代表 T3 的 QEMU/host witness 收紧；D1 未通过仍使 tasks T3.1 保持
  未完成，后续不得把它误报为全 PASS。

**Verification**

```text
rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test
/tmp/ms04-async-rx-host-test
make host-test
cargo test --manifest-path crates/uart_16550/Cargo.toml --offline --features async
cargo check --offline -p starry-kernel --features qemu
cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline
cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib queue::tests
cargo fmt --manifest-path kernel/Cargo.toml -- --check
cargo fmt --manifest-path crates/uart_16550/Cargo.toml -- --check
openspec validate ms04-qemu-async-rx-queue-baseline --strict
git diff --check -- kernel/src/lib.rs kernel/src/drivers/critical_section_policy.rs \
  tests/ms04-async-rx-host-harness.rs Makefile
```

Act must record exact exits and unique MS04 test names in Act Response. Any product/test failure
blocks completion; none may be reclassified as manual or `ENV-BLOCKED` because this iteration has
no sandbox-dependent operation.

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Requirement | PASS | closes T3 production-witness gap found by iteration 000 Review |
| Investigation | PASS | actual production impl, dead seam, duplicate harness and fresh tests inspected |
| Scope | PASS | one production-bound test seam; four allowed paths |
| TDD | PASS | current direct-call implementation is RED for binding witness; fake backend gives GREEN |
| Verification | PASS | host, UART, QEMU check and T1/T2 regressions are agent-executable |
| Manual boundary | PASS | no QEMU/manual work; final user-only iteration remains separate |
| Persisted Evidence | PASS | none required; exact summaries belong in Act Response |
| User Approval | BLOCKED | awaiting explicit Gate 2 approval; Act is not authorized |

**Persisted Evidence**

- Mode: none
- Reason: all witnesses are short deterministic host/unit/check commands; Act Response records
  commands, exits, test names and relevant output. No QEMU/runtime or long diagnostic log exists.

**Risks and Notes**

- Avoid designing a second critical-section framework. The seam should remain a few pure generic
  operations around the existing bool restore contract.
- A host fake can prove call order and restore decisions, not real RISC-V CSR behavior; the QEMU
  target compile and later runtime restore-violation telemetry cover the remaining layers.
- D1 remains a visible Gate debt, not a hidden manual task. It must be resolved or explicitly
  waived before final automatic Gate acceptance.
- Manual QEMU work remains reserved for the final independent iteration described in `tasks.md`.

## Act Response

- Status: pending

**Implemented**

Pending.

**Verification Evidence**

Pending.

**Persisted Evidence**

Pending.

**Remaining Issues**

Pending.

## Plan Review

- Status: pending

**Review Result**

Pending.

**Findings**

Pending.

**Deviation Classification**

Pending.

**Evidence**

Pending.

**Follow-up Decision**

Pending.

**Next Iteration**

Pending.
