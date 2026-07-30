# Iteration 000: Device-specific diagnostic IRQ implementation

## Plan Context

- Status: ready
- Round: 000
- Parent: None

**Objective**

完成 MS03 的运行就绪实现：

- QEMU UART 从全局 hook 迁到 IRQ 10 设备 handler。
- QEMU VirtIO-net 建立 IRQ 7 诊断 handler。
- 保持唯一 `VirtIoNetDev` 和 MS02 轮询数据面。
- 提供不在 ISR 打印的按需计数快照与 guest probe。
- 通过 agent 可执行的纯逻辑、UART、axnet 和静态 Gate。

本 iteration 不声明 MS03 运行通过。
target build 和 QEMU Evidence 位于用户能力边界。

**Background**

MS02 只证明 VirtIO-MMIO 轮询收发。
当前 MMIO probe 给 `VirtIoNetDev` 传入 `irq=None`，
axnet 因而使用 10 ms timer fallback。

QEMU UART 当前占用唯一 global IRQ hook。
若 MS03 直接把 IRQ 7 暴露为
`AxNetDevice::irq_num()`，axnet 会停用 timer fallback，
并进入仍依赖该 global hook 的 waker 路径。

用户批准 MS03 只建立诊断控制面，
并补充一条约束：
同一时间只能保留一个网卡实例和一个数据面 owner。
MS04 验证异步 RX 时，必须关闭或隔离旧轮询进度，
避免旧路径替异步路径完成测试。

**Current Baseline**

- Revision:
  `05dfcfc3ff29401290e666beffcfbe9aeca3267b`
- Branch: `net-k3`
- `axnet::init_network` 只取一个 net device。
- `EthernetDevice` 独占 `AxNetDevice` 的可变 queue 操作。
- MMIO `VirtIoNetDev` 当前 `irq_num() == None`。
- `Service::register_waker` 对该设备使用 10 ms fallback。
- QEMU UART 使用 `register_irq_hook(uart_isr_wrapper)`。
- D1 UART 已使用 `axhal::irq::register` 设备 handler。
- MS02 QEMU Evidence 已证明 MMIO net/block、网络协议和
  `RING_EVENT_IDX` 协商基线。

Fresh baseline results:

| Command | Result | Exit |
|---|---|---|
| `git rev-parse HEAD` | `05dfcfc...` | 0 |
| `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib service::tests -- --nocapture` | 8 passed | 0 |
| `cargo test --manifest-path crates/uart_16550/Cargo.toml --offline --features async` | 62 unit + 18 doc passed | 0 |
| QEMU target cold build | `lwext4_rust` cross-C process hit sandbox `Bad system call` | 101 |

冷构建失败发生在产品 Rust 编译前的既有 ext4 C
dependency，不是 MS03 source failure。
同 revision 的归档 MS02 Evidence 有成功 target build。
本 iteration 不把当前 sandbox 结果伪报为 PASS。

**Current-State Evidence**

Network entry and owner:

- `axruntime::rust_main` 先 `axdriver::init_drivers`，
  再调用 `axnet_ng::init_network`。
- `crates/axnet/src/lib.rs::init_network`
  调用一次 `net_devs.take_one()`。
- 设备被移动到
  `crates/axnet/src/device/ethernet.rs::EthernetDevice::inner`。
- `Service::poll -> Router::poll -> EthernetDevice::recv`
  调用 `inner.receive()` 并回收 RX。
- `EthernetDevice::send_to` 回收 TX、分配 buffer 并发送。
- 没有第二个运行时 queue owner。

Current IRQ capability:

- `axdriver::VirtIoDriver::probe_mmio` 调用
  `D::try_new(transport, None)`。
- registry `axdriver_virtio::VirtIoNetDev`
  保存该 `irq: Option<usize>`。
- `EthernetDevice::requires_polling`
  以 `inner.irq_num().is_none()` 决策。
- `EthernetDevice::register_waker`
  只在 IRQ 为 `Some` 时调用
  `axtask::future::register_irq_waker`。
- 当前路径没有触发 global net waker 冲突，
  因为 IRQ 为 `None`。

VirtIO status and rearm:

- MMIO interrupt status/ack 位于 offset `0x60/0x64`。
- status bit 0 是 used-ring，bit 1 是 config-change。
- registry `VirtIoNetDev::receive` 当前兼容调用
  `inner.ack_interrupt()`。
- `VirtQueue::set_dev_notify` 在 `event_idx=true` 时不改 flags。
- `VirtQueue::pop_used` 在 `event_idx=true` 时把
  `last_used_idx` 写入 `avail.used_event`。
- 因此 MS03 的有效 rearm 仍由唯一轮询 owner 消费
  used ring 完成。

PLIC and UART:

- QEMU axplat 外部中断顺序是
  claim -> handler table -> complete。
- `axhal` global hook 在 platform `handle` 返回后调用。
- `kernel/src/drivers/uart_init.rs::init_uart_hardware`
  当前为 QEMU 注册 global hook，且不检查结果。
- 同文件 D1 路径已有零参数设备 handler 范例。
- UART ISR 自有 RX/TX/drain `AtomicWaker` 和
  `IRQ_COUNT`，迁移不需修改 crate 行为。

Testing and observability:

- kernel 裸 host test 不是有效项目 Gate。
- `Makefile::host-test` 已使用独立 rustc harness
  测 kernel 纯逻辑。
- `kernel/src/syscall/fs/ctl.rs::sys_ioctl`
  已有 UART debug snapshot 命令。
- QEMU 测试按项目 Runbook 必须由用户在 guest shell
  手工执行，不允许 agent 脚本驱动。

**Relevant Code**

| Surface | Current Responsibility |
|---|---|
| `kernel/src/platform/descriptor.rs` | 通用平台硬件事实 |
| `kernel/src/platform/qemu.rs` | QEMU console、PLIC、memory 事实 |
| `kernel/src/drivers/uart_init.rs` | UART init、ISR 注册、copier 生命周期 |
| `kernel/src/entry.rs::init` | QEMU kernel startup ordering |
| `crates/axnet/src/lib.rs::init_network` | 选取唯一 NIC，建立 Service |
| `crates/axnet/src/device/ethernet.rs` | 唯一 NIC 数据面 owner |
| `crates/axnet/src/service.rs` | 轮询和 10 ms fallback |
| `kernel/src/syscall/fs/ctl.rs::sys_ioctl` | 只读/控制 debug ioctl |
| `Makefile::host-test` | kernel pure-logic host harness |

**Critical Path**

Initialization:

```text
axruntime probes MMIO devices
  -> creates one VirtIoNetDev with irq=None
  -> axnet takes one device into EthernetDevice
  -> axruntime initializes platform IRQ
  -> starry_kernel::entry::init
  -> UART register(10, qemu handler)
  -> net diagnostic register(7, net handler)
  -> UART benchmark and copier startup
```

Network event:

```text
device updates used ring and asserts IRQ 7
  -> PLIC claim 7
  -> net handler reads/classifies status
  -> net handler writes MMIO ACK
  -> handler returns
  -> PLIC complete
  -> existing timer wakes network waiter
  -> unique polling owner pops used descriptor
  -> pop_used updates used_event
  -> next independent device event may interrupt again
```

Snapshot:

```text
guest probe ioctl
  -> sys_ioctl validates user pointer
  -> net IRQ telemetry snapshot
  -> include UART irq_count
  -> copy fixed repr(C) struct to guest
```

**Implementation Guidance**

1. 先建立纯逻辑 host RED tests。
2. 增加 status decoder、telemetry 和平台事实后转 GREEN。
3. 在现有 UART GREEN witness 上做设备 handler 重构。
4. 注册 net handler，但保持 net driver `irq=None`。
5. 增加只读 snapshot 与 guest probe。
6. 完成 agent Gate 和完整 diff review。
7. 到 target build/QEMU 能力边界即停止并交接。

不要本地化 registry driver。
不要用 global hook 记录 EOI。
不要用 `set_dev_notify` 伪造 EVENT_IDX rearm。
不要从 IRQ handler 取得 axnet lock。

**Behavioral Change**

Current:

- UART 任意外部 IRQ 后经 global hook 检查一次。
- Net 没有设备 handler。
- Net 只有 timer polling 数据面。
- 没有 net IRQ cause snapshot。

Target:

- UART 只在 PLIC IRQ 10 进入自身 handler。
- Net 只在 PLIC IRQ 7 进入诊断 handler。
- Net handler ACK device cause，但不唤醒 queue task。
- Timer polling 仍是唯一 descriptor progress owner。
- Guest 可按需读取单调 IRQ snapshot。
- UART handler 注册失败时 fail-fast；
  net handler 失败时轮询 fallback 继续。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| T1 | R1; R3; R5; R6 | `platform/*`; `virtio_net_irq_logic`; host harness | 平台事实；无 net IRQ pure seam | 增加 QEMU net fact、decoder、telemetry tests |
| T2 | R2; R7 | `uart_init::init_uart_hardware` | QEMU global hook | IRQ 10 handler + checked registration |
| T3 | R1-R6 | `virtio_net_irq`; `drivers/mod`; `entry::init` | 无 net IRQ control | IRQ 7 validate/register/cause/ACK |
| T4 | R6; R8 | `sys_ioctl`; `ms03_irq_probe.c`; `Makefile` | UART debug；无 net probe | 只读 snapshot 和有界刺激 payload |
| T5 | R1-R8 | change artifacts and full diff | 无 MS03 Gate | agent verification and handoff |

**Task Contracts**

T1 — pure logic and platform facts:

- Depends on: None.
- Current: no net IRQ config, decoder or telemetry seam.
- Target: QEMU config is explicit; status and counters host-testable.
- Must change: platform descriptor initializers, re-exports,
  pure logic module, host harness and host-test target.
- Must preserve: console/PLIC facts and all non-QEMU descriptors.
- Must not: access MMIO/axnet/wakers in pure logic.
- RED: host harness fails because required symbols/behavior are absent.
- GREEN: used/config/combined/unknown/spurious/residual and
  monotonic snapshot cases all pass.
- Verify:
  `rustc --edition=2024 --test tests/ms03-irq-host-harness.rs
  -o /tmp/ms03-irq-host-test && /tmp/ms03-irq-host-test`.
- Stop: pure logic cannot be compiled outside the kernel target,
  or platform address is not unique.

T2 — QEMU UART migration:

- Depends on: None; execute after T1 for review order.
- Current: QEMU uses unchecked single global hook.
- Target: IRQ 10 device handler, checked registration, same ISR/wakers.
- Must change: QEMU-only registration and zero-arg wrapper.
- Must preserve: D1 path, UART crate, rings, copier, early/panic console.
- Must not: change UART ISR semantics or waker ownership.
- Pre-refactor GREEN: current UART unit/doc tests.
- Post-refactor GREEN: same tests and structural absence of the QEMU
  `register_irq_hook` call.
- Verify:
  `cargo test --manifest-path crates/uart_16550/Cargo.toml
  --offline --features async`;
  source check with `rg`.
- Stop: migration requires changing the reusable UART ISR,
  or copier can start after registration failure.

T3 — net diagnostic handler:

- Depends on: T1, T2.
- Current: no IRQ 7 handler; polling receive performs compatibility ACK.
- Target: header validation, checked registration, minimal handler,
  monotonic telemetry and polling fallback.
- Must change: new QEMU net IRQ module, module export and startup call.
- Must preserve: one `VirtIoNetDev`, `irq_num=None`, 10 ms fallback,
  queue state and negotiated features.
- Must not: implement `NetDriverOps`, hold Service lock, touch descriptors,
  wake a task, register a global hook or disable `RING_EVENT_IDX`.
- RED/GREEN: T1 tests define classification/counter behavior;
  integration GREEN is deferred to QEMU Evidence.
- Verify: host harness, source audit, later target build/QEMU.
- Stop: second device/queue owner is required; registry behavior changed;
  compatibility ACK steals observable cause in controlled evidence.

T4 — snapshot ABI and guest probe:

- Depends on: T3.
- Current: no user-visible net IRQ counters.
- Target: read-only command `0x4e49_4431` returns fixed snapshot;
  guest probe captures quiet windows.
- Must change: `sys_ioctl`, C payload and Makefile target.
- Must preserve: existing UART ioctl behavior and user pointer checking.
- Must not: reset counters, mutate device state, print from ISR,
  automate QEMU or add a background reporter.
- RED/GREEN: snapshot counter/field tests are RED before T1/T3,
  GREEN after; C payload must pass host syntax check.
- Verify:
  `cc -Wall -Wextra -Werror -fsyntax-only
  tests/ms03_irq_probe.c`.
- Stop: ABI cannot be QEMU-isolated; payload must print between PRE/POST;
  a test mode requires a second network owner.

T5 — agent Gate and handoff:

- Depends on: T1-T4.
- Current: no implementation diff.
- Target: all agent-executable tests and reviews pass.
- Must run: fmt, host harness, axnet tests, UART tests,
  C syntax, strict OpenSpec validation and diff check.
- Must review: specs vs code first, then full code diff.
- Must not: claim target build or runtime PASS from archived evidence.
- Evidence: command summaries stay in Act Response.
- Stop: any Gate fails; target build is attempted and hits the known
  sandbox boundary; new interface/ownership decision appears.

**Invariants**

- Exactly one selected `VirtIoNetDev`.
- Exactly one mutable descriptor/packet owner.
- Net `irq_num()` remains `None` in MS03.
- MS02 10 ms polling fallback remains active.
- Net ISR does not move descriptors or packets.
- Net ISR does not wake a queue or stack task.
- UART and net use distinct PLIC handler entries.
- UART wakers and copier semantics remain unchanged.
- `RING_EVENT_IDX` remains negotiated.
- EOI is not claimed from a fabricated ISR counter.
- QEMU evidence is not SMP or board evidence.

**Non-goals**

- Async RX/TX and queue waker.
- Removing the polling net path.
- Registry driver fork/localization.
- Stack runner and socket readiness redesign.
- PCI, VF2, SMP, hotplug and performance.
- Global tasks/SNAPSHOT update or change archival.

**Acceptance**

Iteration acceptance:

- T1-T5 source work is complete.
- All agent-executable commands pass.
- Full diff review finds no unresolved critical/important issue.
- The source is ready for user target build and QEMU runtime.
- Act Response states that MS03 is not runtime-complete.

Full change acceptance remains:

- QEMU target build passes.
- UART IRQ 10 and net IRQ 7 register.
- RX2/TX2 prove independent repeat delivery and ACK.
- UART-only and concurrent windows prove device isolation.
- Idle window has no IRQ storm.
- `RING_EVENT_IDX`, MS01 and MS02 regressions pass.

Compact RTM:

| Requirement | Scenario | Design | Task | Code/Test | Status |
|---|---|---|---|---|---|
| R1 platform fact | map/missing | D2,D6 | T1,T3 | platform config + startup | Covered |
| R2 handlers | register/conflict/isolation | D3,D4 | T2,T3 | UART GREEN + QEMU | Covered |
| R3 ISR boundary | used/config/spurious | D1,D4,D5 | T1,T3 | host cause tests + snapshot | Covered |
| R4 one owner | startup/control/classification | D1,D2 | T1,T3,T5 | source audit + MS02 | Covered |
| R5 ACK/EOI/rearm | repeat/EVENT_IDX/storm | D4,D6 | T1,T3,T4 | RX2/TX2/idle | Covered |
| R6 counters | RX/TX/shared cause | D4,D5 | T1,T3,T4 | snapshot deltas | Covered |
| R7 compatibility | fallback/UART fail/regression | D2,D3,D6 | T2,T3,T5 | UART + MS01/MS02 | Covered |
| R8 Evidence | concurrent/interrupted/scope | D5,D7 | T4,T5 | probe + Evidence index | Covered |

**Verification**

Agent-executable:

```text
cargo fmt --all -- --check
rustc --edition=2024 --test tests/ms03-irq-host-harness.rs \
  -o /tmp/ms03-irq-host-test
/tmp/ms03-irq-host-test
cargo test --manifest-path crates/axnet/Cargo.toml \
  --locked --offline --lib service::tests -- --nocapture
cargo test --manifest-path crates/uart_16550/Cargo.toml \
  --offline --features async
cc -Wall -Wextra -Werror -fsyntax-only tests/ms03_irq_probe.c
openspec validate ms03-virtio-mmio-diagnostic-irq-baseline --strict
git diff --check
```

User capability boundary:

```text
make LOG=info build
manual QEMU boot with one virtio-net-device
manual guest probe modes and MS01/MS02 regression
```

Any interrupted runtime witness is incomplete.
Archived MS02 runtime may establish the baseline,
but cannot replace fresh MS03 Gate 5 evidence.

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Requirements | PASS | Gate 1 approved; single-NIC constraint added |
| Investigation | PASS | current call paths, ownership, registry behavior and fresh tests recorded |
| Design | PASS | D1-D7 close IRQ facts, owner, ACK/EOI/rearm, snapshot and failures |
| Task Contracts | PASS | T1-T5 include files, behavior, tests, commands and stops |
| Traceability | PASS | R1-R8 have no Missing or Simplified rows |
| Verification | PASS | RED/GREEN, regression and user runtime boundaries are explicit |
| Persisted Evidence | PASS | current iteration `none`; later QEMU execution requires Evidence |
| User Approval | PASS | 用户于 2026-07-29 回复“批准 Gate 2” |

**Persisted Evidence**

- Mode: none

T1-T5 command, output, exit code, changed files and symbols
must be recorded in the Act Response.

The later user runtime boundary requires persisted Evidence,
but it is not part of this implementation iteration.
After implementation Review, the next execution context must name:

- build log and environment;
- full QEMU serial log;
- guest probe PRE/MID/POST/DELTA markers;
- MS01/MS02 regression logs;
- Evidence README with Gate mapping.

**Risks and Notes**

- Registry `receive()` still performs compatibility ACK.
  Controlled windows must expose any interference.
- QEMU address depends on device ordering.
  Header validation is mandatory.
- The snapshot ioctl is diagnostic and QEMU-only.
- Current sandbox blocks the ext4 cross-C build.
  This is a known capability boundary, not a waived Gate.
- Gate 2 approved on 2026-07-29.

## Act Response

- Status: pending

**Implemented**

<实际完成内容>

**Changed Files and Symbols**

<文件、符号和作用>

**Deviations from Plan**

<偏差、原因和影响；没有则写 None>

**Blocker Handoff**

<正常完成写 None；blocked 时填写：>

- Discovered at: <task / step / Gate>
- Expected: <Plan 预期>
- Actual: <实际情况>
- Impact: <为何不能按原计划继续>
- Completed work: <已完成任务>
- Partial work: <部分修改>
- Unstarted work: <未开始任务>
- Worktree state: <修改文件和安全状态>
- Gates: <已通过和阻塞的 Gate>
- Evidence: <证据编号、路径或 None required>
- Plan decision needed: <需要 Plan 重新决定的问题>
- Resume condition: <后续 iteration 的恢复条件>

**Self-Review**

- Plan compliance: PASS | BLOCKED
- Full diff reviewed: PASS | BLOCKED
- Critical findings unresolved: <数量>
- Important findings unresolved: <数量>
- Minor findings unresolved: <数量>

<记录 Act 自检发现、已修复内容、重跑验证和遗留 Minor 问题>

**Verification Evidence**

<命令或操作、关键输出、退出码和结论>

**Persisted Evidence**

None required.

**Experience Candidates**

| Type | Candidate | Evidence | Reason |
|---|---|---|---|
| Runbook / Incident | <候选主题> | <Act Response 或 Evidence> | <满足产物门槛的原因> |

<没有候选时写 None。候选不构成创建授权>

**Remaining Issues**

<未解决问题或 None>

**Commit or Diff Reference**

<可选引用；本字段不要求创建 Git commit>

## Plan Review

- Status: pending

**Review Result**

<follow-up-required | no-follow-up>

**Findings**

<基于代码、diff 和验证证据的发现>

**Deviation Classification**

<PLAN-OMISSION | PLAN-INVALID | ACT-DEVIATION | BASELINE-CHANGED | NEW-EVIDENCE | None>

**Evidence**

<文件、符号、命令和输出>

**Follow-up Decision**

<下一步和范围>

**Next Iteration**

<新 iteration 路径或 None>
