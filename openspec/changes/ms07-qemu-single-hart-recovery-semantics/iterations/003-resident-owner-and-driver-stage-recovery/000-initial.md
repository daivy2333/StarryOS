# Iteration 003 / Cycle 000: Resident Owner and Driver-Stage Recovery

## Plan Context

- Status: draft
- Iteration: 003-resident-owner-and-driver-stage-recovery
- Cycle: 000-initial
- Cycle Type: initial
- Parent cycle: None

**Iteration Scope**

- Change tasks: 2.3
- Depends on: Iteration 002 accepted
- Stable baseline: 唯一常驻 queue owner 以有界 poll 驱动 quiesce/reset/reinitialize，成功后提交新 QueueEpoch 并恢复 queue I/O，失败后保留 backing、驻留 Faulted 并稳定拒绝提交。
- Verification boundary: 三个 driver stage 的 success、Pending、timeout、owner/backing、event 与 commit-before-wake witness；两个 axnet 串行全量和三个下层 suite 全部通过。
- Diagnostic boundary: `RxRxFuture` recovery lifecycle、Service/Router recovery forwarding、driver step、timer wake、owner/flush commit。
- Deferred tasks: 3.1–4.2

**Cycle Scope**

- Trigger: initial
- Acceptance gaps: None
- Repair items: None
- Inherited scope: R1、R3–R5、D2–D5；Iteration 002 接受的 data-stage trigger、QueueEpoch ticket ledger、coherent fault identity、slot/flush outcome 和 guard 外 wake。
- Excluded scope: config IRQ/link policy、SocketEpoch/terminal registry、公开 V4、QEMU control/runtime、SMP、PCI/DWMAC runtime、真板与性能。

**Objective**

独立验收并补齐工作树中已有的 resident recovery 状态机。相同 `RxRxFuture` 必须以每 poll 有界的 ledger 工作和至多一个 driver step 完成 `Active -> Quiescing -> Resetting -> Reinitializing -> Active|Faulted`，不产生第二 owner、不 busy-wait，也不在 guard 内 wake。

**Background**

Iteration 002 已让 completion/reclaim timeout 在 owner ledger 完整时进入 resident recovery，并接受 fault identity。Task 2.3 的产品代码已驻留在工作树，但此前只作为后续实现保留，不能因普通 full suite 通过而提前验收。现有 recovery fixture 覆盖成功、reset Pending timeout、step error、Faulted residency 和部分 wake ordering；它的 DeviceOwned 永远为零，尚未见证 quiesce budget/timeout/backing，且没有独立 reinitialize timeout 或新 epoch queue-I/O witness。

**Current Baseline**

- Revision：`2a303eaa3d0b2dc3044b32c22eeb5e49a355bbf5`；Iteration 000–002 产品改动仍在 staged + unstaged worktree，尚未提交。
- `RxRxFuture` 已包含 `recovery: Option<RecoveryState>`、单一当前 driver-stage `recovery_deadline`、production axtask timer 和 test clock。
- `poll_recovery` 每次先注册 queue waker；Quiescing 首次进入取消 Queued/ARP pending、arm 1s deadline，并每 poll 最多 reclaim `RECLAIM_BUDGET=32`；drained 或到期后调用一次 `recovery_begin_target`。
- Resetting/Reinitializing 在每 poll 调用一次 `recovery_step_target`；stage 变化重新 arm 2s，same-stage Pending 保持原 deadline；timeout/error 调用 `publish_recovery_fault` 驻留 Faulted。
- Recovered 路径在 Service guard 内关闭旧 DeviceOwned/flush、推进 QueueEpoch、提交 Active；guard 释放后 wake flush、清 recovery hold 并 self-wake。
- 现有 focused `recovery` filter 13/13、spawn seam 1/1、ordinary 413/413、qemu-diagnostics 437/437 均 exit 0。它们尚未覆盖下列 test gap。

**Current-State Evidence**

1. `RecoveringDevice::tx_device_owned_len()` 固定返回 0，`tx_reclaim_one()` 固定 Empty，`tx_close_device_owned()` 返回 0；现有成功测试直接越过真实 quiesce owner/backing 边界。
2. reset Pending 的 absolute deadline 已有 witness；同一 fixture 能 stall stage 2，但没有独立 reinitialize deadline/错误身份 test。
3. Quiescing 在达到 32 次 reclaim budget 后直接返回 Pending，当前没有证明剩余可见 completion 会 self-wake 继续收敛；若无新事件，只靠 1s timer 再 poll 会把正常 backlog 延迟到 deadline。
4. `quiescing_to_resetting()` 在 `recovery_begin_target()` 前提交 lifecycle；begin error 时 `self.recovery` 仍是 Quiescing，当前 fault stage 可能记录 Quiesce 而 lifecycle 已为 Resetting，必须由 test 固定准确身份。
5. 成功测试只断言 epoch、Active 和 hold clear，没有通过 queue-level send/submit/reclaim 证明新 epoch 恢复 I/O；socket epoch 语义仍属于 Iteration 005，不在本轮提前处理。
6. step-error unlock-observing waker 已证明 fault wake 在 guard 外；成功路径尚无同等级 observer。唯一 spawn seam 与 lifecycle owner-view tests 可复用。

**Relevant Code**

- `crates/axnet/src/async_rx.rs::{RxRxFuture::enter_recovery,poll_recovery,recovery_step,publish_recovery_fault,arm_recovery_timer,RxLifecycle}` 及 recovery fixtures/tests。
- `crates/axnet/src/{service.rs,router.rs}`：recovery begin/step、hold、DeviceOwned、epoch 与 owner summary forwarding。
- `crates/axnet/src/device/{mod.rs,ethernet.rs,fixed_queue.rs}`：queue gate、ticket outcome、ResetAborted/Fault owner closure。
- `crates/axdriver_net/src/lib.rs::NetRecoveryControl`：transport-neutral bounded step contract。
- `crates/axdriver_virtio/src/net.rs::NetRecoveryControl for VirtIoNetDev`：reset confirmation、reinitialize、backing quarantine 与 owner summary。

**Critical Path**

```text
Active data fault
  hold queue I/O -> commit Quiescing -> publish event
  Quiescing: cancel pre-submit once; reclaim <= 32 per poll
    progress with backlog -> self-wake
    drained or 1s expiry with complete ledger -> begin_recovery once
    drift/unknown owner -> Faulted, never reset
  Resetting: <= 1 driver step/poll, fixed 2s deadline
  Reinitializing: <= 1 driver step/poll, new fixed 2s deadline
  Recovered under guard:
    old DeviceOwned -> ResetAborted; settle old flush; advance epoch; Active
  drop guard -> wake flush -> clear hold -> wake owner
  any fatal/timeout:
    commit owner/flush -> drop guard -> Faulted identity/wakes
    retain uncertain backing; future remains Pending resident
```

**Implementation Guidance**

先扩展现有 `RecoveringDevice` fixture，使 DeviceOwned 数量、可见 reclaim、stall stage、begin/step error、owner summary、queue submit 和 wake observation 可分别控制。用它补齐 quiesce、reset、reinitialize 和成功提交 witness；只有 RED 暴露产品缺口后才修改状态机。复用现有 Service/Router/driver contract，不新增 task、timer、锁或 recovery trait。

**Behavioral Change**

- 正常 recovery backlog 在每个 32-item budget 后主动让出并 self-wake，持续有界收敛，不等到 quiesce deadline 才继续。
- quiesce 可自然 drain，也可在 1s 后以完整 remaining owner ledger 进入 reset；unknown/drift 直接 Faulted。
- reset 与 reinitialize 各自拥有进入时 arm 的 2s absolute deadline；same-stage Pending 不续期，stage 转换只重置下一 stage 的 deadline。
- 成功只在旧 owner/flush、new epoch 和 Active 全部提交后开放 queue I/O/wake；失败保留未确认 backing、保持 hold，并驻留 Faulted。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| 2.3 | R1/R3 quiesce | `async_rx.rs::poll_recovery`、fixture | cancel、bounded reclaim、1s deadline | 见证/修复 budget self-wake、drain/expiry、drift与backing |
| 2.3 | R4/R5 reset/reinitialize | `poll_recovery/recovery_step/publish_recovery_fault` | one-step driver progression、2s deadline | 分 stage success/Pending/timeout/error与准确 fault identity |
| 2.3 | R1/R3 commit/wake | `RxLifecycle`、Service/Router/device forwarding | epoch/owner/flush/gate transition | 证明 guard 外 wake、新 epoch queue I/O、Faulted residency与唯一 spawn |
| 2.3 | lower contract | `axdriver_net`、`axdriver_virtio`、`virtio-drivers` tests | bounded reset与backing holder | 复跑既有 recovery suites，不扩展 driver API |

**Task Contracts**

### 2.3: Resident recovery owner and driver-stage deadlines

- Requirement/Scenario: R1 唯一 owner、恢复事件窗口和阶段失败；R3 device-owned quiesce；R4 三个 driver stage deadline；R5 reset/backing；D2–D5。
- Depends on: Task 2.2 accepted 的 data-stage trigger、coherent fault、ticket/flush/epoch baseline。
- Targets: `crates/axnet/src/async_rx.rs`、`service.rs`、`router.rs`、`device/{mod.rs,ethernet.rs,fixed_queue.rs}`、`stack_runner.rs`及现有 fixtures；下层只在 RED 证明 contract 缺口时修改。
- Current behavior: resident state machine 已存在且普通路径 GREEN，但缺少真实 quiesce/backing、budget continuation、reinitialize timeout、begin error stage、新 epoch queue I/O和成功 wake ordering的完整见证。
- Required behavior: 同一 future 驻留驱动全部 recovery states；每 poll ledger 工作有界且至多一个 driver begin/step；可见 backlog 在 budget 后 self-wake；三个 stage deadline 不续期；成功提交顺序为 old owner/flush → new epoch → Active → guard 外 wake/gate reopen；失败为 owner/flush commit → guard drop → Faulted/wake，并保留不确定 backing。
- Required changes: 扩展 deterministic fixture/tests；修复测试揭示的 budget wake、stage identity、owner closure或提交顺序缺口；不得无 RED 重写已 GREEN 状态机。
- Preserve: 唯一 `start_rx_task` CAS/spawn seam、ISR 只 ack/publish、register-recheck、quiet path 无 10ms polling、V1–V3 codes 0–4、Iteration 002 SeqCst fault identity和 data deadlines。
- Forbidden: 第二 queue task、caller-driven descriptor progress、blocking sleep/spin、一次 poll 多次 driver step、guard 跨 Pending/wake、reset 掩盖 drift、status=0 前释放 backing、Faulted future 退出、提前实现 SocketEpoch/link/V4。
- Test witness: deterministic tests分别覆盖 quiesce natural drain、>32 backlog budget self-wake、1s expiry remaining owner、quiesce drift；reset Pending/timeout；reinitialize Pending/timeout/error；begin error准确 stage；成功 owner/flush/epoch/Active/gate/wake顺序与新 epoch queue I/O；event before/during register；Faulted 后 step count不变；spawn count始终1。
- GREEN condition: lifecycle、deadline、epoch、ticket outcome、owner summary/backing、gate和wake observation全部匹配；成功 queue path继续服务，失败稳定拒绝且所有 waiter完成或稳定失败。
- Verification: focused recovery/lifecycle/source tests；axnet ordinary与qemu-diagnostics完整串行 suite；随后 axdriver_net、axdriver_virtio(net)、virtio-drivers(alloc) 三个下层 suite；production checks、scoped rustfmt、diff check和严格 OpenSpec validation。
- Stop when: driver step必须持锁等待设备、恢复需要第二 executor/task、现有 event 协议无法承载 deadline wake、或 owner 完整性需要修改已接受的 NetRecoveryControl/Iteration 002 contract；填写 Blocker Handoff 返回 Plan。

**Invariants**

- 同一时刻只有一个 queue owner；恢复状态仍为 AsyncOwned。
- 每 poll 最多 32 个 quiesce reclaim 和一个 driver begin/step；无无界循环。
- DeviceOwned 只由合法 completion、status=0 后 ResetAborted 或明确 Fault 终结；timeout 本身不释放 backing。
- lifecycle/owner/flush/epoch 先提交，Service guard 释放后再 wake；Faulted 永久 hold 且 future驻留。
- QueueEpoch、event generation、未来 SocketEpoch 保持不同 identity；V1–V3 ABI 不变。

**Non-goals**

- 不实现 link/config IRQ 或 socket terminal epoch。
- 不新增 QEMU ioctl/probe/validator，不执行真实 QEMU qualification。
- 不证明 SMP、PCI/DWMAC、真板或性能。
- 不清理 baseline warning或优化 SeqCst telemetry。

**Acceptance**

- A1（R1）：同一 future 覆盖 Active/Quiescing/Resetting/Reinitializing/Faulted；spawn 恰好一次，Faulted 驻留且不再 step。
- A2（R3/D3）：quiesce cancel pre-submit 恰好一次，每 poll reclaim ≤32；backlog self-wake；drain或1s expiry后仅在 owner ledger完整时 begin reset，backing不提前释放。
- A3（R4/D2）：reset/reinitialize各自2s absolute deadline，same-stage Pending不续期；每 poll至多一个 driver step；timeout/error携带准确 stage/cause/epoch/owner summary。
- A4（R5/D3/D4）：status=0/Recovered 前旧 backing保持 device/recovery owner；成功后旧 DeviceOwned以 ResetAborted闭合、old flush稳定失败并提交新 QueueEpoch。
- A5（R1/D5）：成功在完整提交后 guard 外 wake、clear hold并恢复新 epoch queue I/O；失败先提交 Faulted/quarantine再 guard 外 wake，所有 waiter不永久 Pending。
- A6（兼容）：唯一 owner、ISR/register-recheck、quiet path、V1–V3、Iteration 002与三个下层 recovery contract不退化。

**Verification**

1. focused `recovery`、quiesce budget、reinitialize、wake ordering、spawn/lifecycle/source guards，均 `--test-threads=1`。
2. `env RUSTFLAGS="-C linker=/tmp/opencode/cc-nopie.sh" cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --test-threads=1`。
3. 同上增加 `--features qemu-diagnostics`，串行运行。
4. `cargo test --manifest-path crates/axdriver_net/Cargo.toml --locked --offline --lib`。
5. `cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --locked --offline --lib --features net`。
6. `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --locked --offline --lib --features alloc`。
7. ordinary 与 qemu-diagnostics production `cargo check`、相关文件 rustfmt、`git diff --check`、完整 diff Review、`openspec validate ... --strict`。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | resident入口、状态、timer、owner/flush/epoch forwarding、driver contract和现有/缺失tests已定位。 |
| Design | PASS | quiesce预算、三stage deadline、成功/失败提交顺序与queue-level恢复边界已固定。 |
| Iteration Plan | PASS | Task 2.3形成独立resident recovery baseline；link/socket/QEMU仍留后续 Iteration。 |
| Cycle Scope | PASS | 只验收2.3；既有代码允许保留，但必须以本轮独立 witness证明。 |
| Task Contracts | PASS | Act可仅凭本 Cycle 建立test seam、判断RED并实施；stop条件明确。 |
| Traceability | PASS | R1/R3–R5、D2–D5、Task 2.3、代码面和tests形成闭合映射。 |
| Verification | PASS | stage model、owner/backing/wake、新epoch queue I/O、双axnet与三下层suite覆盖Acceptance。 |

Gate 2 技术检查全部 PASS；Plan Context 保持 draft，等待用户批准本 Cycle 后才能改为 ready 并交给 Act。

**Persisted Evidence**

- Mode: none

命令与决定性输出可低成本重跑，Act Response 足以保存 Gate 结果。

- Budget: 本 Cycle 最多5个Evidence文件（含README），整个change最多20个；当前不创建 Evidence。

**Risks and Notes**

- 工作树已有2.3实现会让部分新增test直接GREEN；没有产品修改时不制造RED。任何实际修复仍须先有失败 witness。
- 单 hart QEMU不能证明SMP；本轮仅要求 no_std 原子/锁契约和单 owner事件正确性。
- public socket 在 recovery 后的 old/new epoch 语义属于 Iteration 005；A5 的“继续I/O”限定为 queue/device path，不提前接受 socket migration。
- baseline 的 smoltcp/test-only warning不属于本 Cycle；新增 warning必须处理或记录为阻塞偏差。

## Act Response

- Status: pending

## Plan Review

- Review Result: pending
