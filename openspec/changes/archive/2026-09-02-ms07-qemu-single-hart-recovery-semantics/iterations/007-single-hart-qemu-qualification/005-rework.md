# Iteration 007 / Cycle 005: Drive Bounded Recovery Progress Before Deadline

## Plan Context

- Status: ready
- Iteration: 007-single-hart-qemu-qualification
- Cycle: 005-rework
- Cycle Type: rework
- Parent cycle: `004-replan.md`

**Iteration Scope**

- Change tasks: 4.2
- Depends on: Iteration 006 accepted；Cycle 004 的 P8 zero-`nfds` 修复与 QEMU preflight 保持
- Stable baseline: 单 hart QEMU 的常驻 owner 在 reset/reinitialize deadline 内以有界 timer 重试
  driver step，成功时推进 epoch 并恢复 I/O，持续 Pending 时按原绝对 deadline 进入 Faulted。
- Verification boundary: host/model 先证明定时重试、非 busy loop、deadline 不续期和错误身份；自动
  Gate 通过后，手工 QEMU 完成 MS07 六 case、validator 与 MS01/MS04/MS05/MS06 回归。
- Diagnostic boundary: `RxRxFuture` recovery 调度、axtask timer wake、Service→driver step、VirtIO
  reset confirmation、epoch/owner/socket terminal，以及 QEMU raw serial。
- Deferred tasks: None

**Cycle Scope**

- Trigger: rework-required
- Acceptance gaps: A2 最终 run；A4 reset/link-up 后 peer phase；A5 epoch/owner 恢复；A6 old/new
  socket 与 validator；A7 四组兼容回归。
- Repair items: T4.2-R1、T4.2-R2
- Inherited scope: Task 4.2；R1、R4–R8；D2、D3、D5–D8；P5/P6/P8；V4 ABI、六 case、
  64/64/0 owner、terminal-before-wake、2 s reset/reinitialize absolute deadline。
- Excluded scope: 修改 recovery API 或 VirtIO reset 事务；延长 deadline；Active 数据面轮询；第二
  queue task；自动 QEMU/HMP；SMP、PCI/DWMAC、真板、性能；Runbook/R60 和全局文档维护。

**Objective**

让唯一常驻 owner 在 Resetting/Reinitializing 期间获得 deadline 前的有界执行机会，不依赖设备 IRQ，
也不通过连续 self-wake 忙轮询。修复后完成 Task 4.2 剩余的单 hart QEMU 资格与兼容回归。

**Background**

Cycle 004 已修复 probe 的 zero-`nfds` poll/ppoll，并在真实 QEMU 越过 pre-reset traffic。reset request
进入 Quiescing 和 Resetting 后，owner 直到 2 s 最终 deadline 才再次运行。当前代码在 timeout 检查
之后才会调用 driver step，因此该次 poll 直接 Faulted，`poll_recovery_step` 从未执行。

driver contract 本来允许 Resetting/Reinitializing 在多次 bounded step 后完成；这些状态不保证产生
IRQ。修复责任位于常驻 owner 的调度层，而不是 VirtIO transport。连续 `wake_by_ref` 会让 task 一直
runnable，因此不能作为同阶段 Pending 的重试机制。

**Current Baseline**

- 工作树基于 HEAD `b83e800a`，Cycle 004 的产品、测试、文档和诊断改动均已 staged；revision 只定位
  现场，不参与验证判定。
- `poll_recovery` 在每轮开头注册 queue waker并取得 Service guard；每轮至多调用一次
  `recovery_step_target()`，guard 不跨 Pending 或 wake。
- Quiescing→Resetting 时 `recovery_deadline=now+2s`，`arm_recovery_timer` 仅注册最终 deadline。
- `recovery_step` 对同阶段 Pending 保持原 deadline，对 Resetting→Reinitializing 重新建立 2 s
  deadline；成功或 Faulted 时取消 timer。
- `VirtIoNetDev::poll_recovery_step` 的 Resetting 分支只读一次 `reset_confirmed()`，未确认时返回同阶段
  progress；确认后切到 Reinitializing。该 bounded driver 行为不需要修改。
- 新鲜基线：MS07 host harness 5/5、axdriver_virtio recovery 5/5、现有 same-stage deadline focused
  test 1/1、OpenSpec strict 均 PASS。完整 `make host-test` 仅在 sandbox UDP socket `EPERM` 停止。

**Current-State Evidence**

1. `async_rx.rs::poll_recovery` 的 Resetting/Reinitializing 分支先以 `now>=recovery_deadline` Faulted，
   未到期才调用 `recovery_step`。
2. `async_rx.rs::arm_recovery_timer` 只对 `recovery_deadline` 建立 `sleep_until`；没有 progress retry
   instant，也没有状态转换后的立即软件 wake。
3. `service.rs::recovery_step_target` 只转发到唯一 target 的 `poll_recovery_step`，不存在其他动态调用者
   为 reset 提供进度。
4. `axdriver_virtio::poll_recovery_step` 每次执行有界：Resetting 只检查 status，Reinitializing 只执行
   一次 rebuild；它不阻塞、不 spin、不持有跨 await 状态。
5. 现有 deterministic test 在测试代码中主动调用 `poll_once`，能证明 deadline 不续期，却不能证明
   生产态 timer 会在最终 deadline 前唤醒 owner。
6. Cycle 004 raw serial 记录 Quiescing@10.022929、Resetting@12.024410、随后 Reset/TIMEOUT Faulted；
   driver entry marker 为零，定位到 owner 调度层。

**Relevant Code**

- `crates/axnet/src/async_rx.rs::{RxRxFuture,poll_recovery,recovery_step,arm_recovery_timer}`：恢复状态、
  最终 deadline、timer 和 owner poll。
- `crates/axnet/src/async_rx.rs` tests：`ScriptedRecovery`、deterministic recovery clock、counting waker。
- `crates/axnet/src/service.rs::recovery_step_target`：Service guard 下的唯一 driver step 转发。
- `crates/axdriver_virtio/src/net.rs::poll_recovery_step`：bounded reset confirmation/reinitialize；只清理
  Cycle 004 的临时诊断字段，不改变恢复事务。
- `tests/ms07_recovery_probe.c`、`scripts/ms07-recovery-peer.py`、`scripts/ms07-qemu-validate.py`：P9
  runtime 协议，保持行为。

**Critical Path**

```text
reset request
  -> owner: Active -> Quiescing
  -> bounded drain / begin_recovery(status=0)
  -> owner: Resetting + absolute deadline
  -> one-shot retry timer strictly after now and no later than deadline
  -> owner poll -> exactly one driver step
       reset still pending -> schedule next bounded retry, keep original deadline
       reset confirmed -> Reinitializing + fresh absolute deadline
       recovered -> commit QueueEpoch/SocketEpoch, reopen I/O, wake waiters
       fatal/deadline -> Faulted, retain backing, reject I/O
```

**Implementation Guidance**

1. 先添加 deterministic RED：同阶段 Pending 后必须留下 deadline 前的下一次 timer wake；counting
   waker证明本轮不直接 self-wake；原 stage deadline 不变。
2. 把最终 stage deadline 与下一次 progress wake 分开。Resetting/Reinitializing 仅在恢复期间使用
   10 ms one-shot cadence；每次 wake 最多执行一个 driver step，再注册下一次 one-shot，wake instant
   取 `min(now+10ms, stage_deadline)`。
3. timer/event 的 stale wake 最多增加一次 bounded poll。状态推进、完成或 Faulted 后取消旧 timer；
   正常 Active path 不获得周期 timer。
4. 保持 deadline 检查先于 driver step，保持“达到 deadline 即 Faulted”的既有边界；同阶段 Pending
   不得更新 stage deadline。
5. focused GREEN 后运行 full axnet/driver/host/build Gate。用现有手工协议重跑 QEMU；最终资格不依赖
   INFO marker。完成定位后移除 Cycle 004 增加的 `last_recovery_state_diag`、`last_reset_poll_diag` 及
   对应 import/log，除非 Review 前出现新的产品诊断 requirement。

**Behavioral Change**

- Resetting/Reinitializing 在最终 deadline 前每 10 ms 最多获得一次 timer-driven driver step。
- 同阶段 Pending 不续期；最终 timeout、Faulted、backing retention 和错误映射不变。
- Pending round 不直接 `wake_by_ref`，因此不会形成 continuous runnable loop。
- used/config/software event 可早于 timer 推进同一状态；stale timer 只产生一次 bounded poll。
- Active/normal RX/TX/stack runner、V4 ABI、probe schema、VirtIO reset transaction 和 syscall 不变。

**Change Surface**

| Repair | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| T4.2-R1 | R1/R4/R5/R8：delayed reset、永久 Pending、无 busy loop | `axnet::async_rx::{poll_recovery,arm_recovery_timer}`及 tests | stage deadline 与 owner wake | 分离 progress wake 和最终 deadline；新增 deterministic 调度见证 |
| T4.2-R1 | R8：诊断不成为产品依赖 | `axdriver_virtio::net::VirtIoNetDev`、`axnet::RxRxFuture` | Cycle 004 临时 INFO 诊断 | 最终 GREEN 后移除临时诊断状态；不改 driver transaction |
| T4.2-R2 | R6–R8：QEMU reset/link/socket/回归 | probe、peer、validator、QEMU 与回归入口 | 六 case 和兼容性验证 | 重跑 P8+P9，保存最小 PASS/BLOCKED 现场 |

**Task Contracts**

### T4.2-R1: Timer-drive bounded recovery progress without busy polling

- Requirement/Scenario: R1 唯一 owner；R4 stage deadline；R5 delayed/never-confirmed reset；R8 host/model
  fault matrix和无永久 Pending。
- Depends on: Cycle 004 P8 自动与 target GREEN。
- Targets: `crates/axnet/src/async_rx.rs::{RxRxFuture,poll_recovery,recovery_step,recovery timer}`及同文件
  tests；`crates/axdriver_virtio/src/net.rs`仅作临时诊断清理。
- Current behavior: stage timer只唤醒最终 deadline；没有外部 event时driver step在timeout前不可达。
- Required behavior: Resetting/Reinitializing 用 10 ms one-shot timer安排下一次 bounded step，且 wake
  不晚于最终 deadline；同阶段 Pending保留原deadline，完成/错误取消timer；Active无周期poll。
- Required changes: 分离“最终 deadline”和“下一 progress wake”；timer/event任一唤醒后每poll至多执行
  一个step；同阶段Pending重新安排下一one-shot；移除Cycle 004临时INFO字段和日志。
- Preserve: 1s quiesce、2s reset/reinitialize；timeout先于deadline时刻的step；Service guard不跨wake/
  Pending；唯一owner；epoch/owner/terminal提交顺序；driver API与VirtIO reset transaction。
- Forbidden: 同阶段Pending直接或循环`wake_by_ref`；blocking sleep/spin；Active polling fallback；第二
  task；续期deadline；放宽Faulted/backing retention；为测试增加身份型证据。
- Test witness: 新增RED证明当前实现只安排最终deadline；新增deterministic测试证明下一wake严格晚于
  now且不晚于deadline、counting waker在Pending round不增长、delayed confirmation在deadline前经历
  多次step后恢复、never-confirmed在原deadline Faulted；既有same-stage test修改前GREEN并保持GREEN。
- GREEN condition: delayed reset推进到Recovered且epoch恰好+1；stalled reset按原stage/cause Faulted；
  每轮至多一个driver step；无即时self-wake storm；既有recovery/link/socket tests不退化。
- Verification: axnet focused recovery tests；axnet ordinary与qemu-diagnostics串行全量；
  axdriver_virtio recovery/full net suite；`make host-test`或仅对sandbox socket `EPERM`分层运行无socket
  子Gate；RISC-V kernel/probe build；diff review；OpenSpec strict。
- Stop when: axtask timer不能表达独立于最终deadline的one-shot wake，修复需要改变driver contract、
  deadline/错误语义或普通Active调度，或新的wake机制导致continuous runnable；返回Plan。

### T4.2-R2: Complete single-hart QEMU qualification and affected regressions

- Requirement/Scenario: R6 initial/HMP link；R7 old/new socket；R8真实reset、peer和兼容回归。
- Depends on: T4.2-R1全部自动GREEN，诊断临时代码已清理，focused QEMU见证reset epoch推进。
- Targets: 既有probe、peer、validator、QEMU/HMP与MS01/MS04/MS05/MS06手工入口；默认不再修改产品。
- Current behavior: P8 preflight、pre-reset traffic和reset request通过；旧socket case因owner未推进失败。
- Required behavior: single hart QEMU 7.0.0 VirtIO-MMIO user-net、LOG=warn完成六case；reset后epoch与
  64/64/0 owner恢复，旧socket终止、新socket双向成功，HMP off/on不推进QueueEpoch；validator和四组
  回归明确PASS。
- Required changes: 只执行既有行为协议并采集最小证据；不得为通过资格再改变产品或测试判据。
- Preserve: R44手工QEMU/HMP；V4和case顺序；peer 15572不hostfwd；P8四边界；absolute deadline；
  terminal-before-wake；LOG=warn最终资格。
- Forbidden: 用INFO诊断run、pcap或driver completion替代guest syscall/peer结果；缺marker/exit判PASS；
  用hash/revision/run-id/freeze证明运行归属；遇新产品错误继续猜测修复。
- Test witness: T4.2-R1 focused GREEN是入口；raw serial、validator和回归终态为最终见证。
- GREEN condition: A2/A4–A7全部成立；validator exit 0；无panic、trap、fatal owner drift、永久Pending
  或未解释fault。
- Verification: 用户手工MS07六case；validator；MS01 14/14、MS04四mode、MS05六mode、MS06 12-case。
- Stop when: 任一runtime case、validator或回归失败，或用户尚未提供结果；保存本Cycle最小
  PASS/BLOCKED Evidence并停止。

**Invariants**

- 每个owner poll最多执行一个driver recovery step；ISR仍只处理cause/ack/event。
- stage deadline只在进入Resetting/Reinitializing时建立，same-stage Pending不续期。
- reset未确认不释放或复用backing；Faulted owner常驻并拒绝新I/O。
- progress retry只存在于恢复状态，不成为正常数据面的10ms polling fallback。
- QueueEpoch与SocketEpoch、old/new socket terminal、64/64/0 owner和V4 ABI语义不变。
- QEMU结论限定单hart VirtIO-MMIO，不外推SMP或真板时序。

**Non-goals**

- 不改变recovery trait、VirtIO status/reset primitive、deadline数值或socket错误映射。
- 不优化恢复延迟/CPU，不建立自适应backoff，不声明物理设备reset时序。
- 不修改poll/ppoll、loader、UDP、smoltcp、executor或timer实现。
- 不更新Runbook、Incident、references、SNAPSHOT、tasks全局状态或提交Git。

**Acceptance**

- A2：最终LOG=warn资格run无未解释user fault、kernel trap或panic。
- A4：pre-reset、post-reset、post-link-up三个peer phase均双向成功并受原absolute deadline约束。
- A5：delayed reset在2s内由timer重试推进；成功后QueueEpoch/SocketEpoch各按规则推进并恢复
  64/64/0；HMP flap不推进QueueEpoch；永久Pending仍在原deadline Faulted。
- A6：旧socket返回稳定terminal，新socket成功；validator exit 0；无owner drift或永久Pending。
- A7：MS01 14/14、MS04四mode、MS05六mode、MS06 12-case均有明确PASS与exit。
- 调度安全：同阶段Pending不即时self-wake；Active无recovery timer；每poll最多一个driver step。

**Verification**

1. RED/GREEN：focused timer policy、counting waker、delayed/never-confirmed recovery和既有deadline tests。
2. 邻接：axnet ordinary/qemu-diagnostics串行全量，axdriver_net、axdriver_virtio、virtio-drivers受影响suite。
3. 集成：`make host-test`；若唯一失败仍是sandbox UDP socket `EPERM`，逐项运行全部无socket子Gate并
   记录`SKIPPED: sandbox socket EPERM`；kernel和probe build exit 0。
4. runtime：用户手工single-hart QEMU六case、validator、MS01/MS04/MS05/MS06回归。
5. Review：完整diff、临时诊断清理、Evidence预算与`openspec validate ... --strict`。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | 代码、调用者、timer注册、driver bounded step、host test缺口和raw serial已独立核对 |
| Design | PASS | 10ms one-shot progress wake与2s最终deadline分离；无连续self-wake或Active fallback |
| Iteration Plan | PASS | 仍为Task 4.2/Iteration 007；repair只关闭既有A2/A4–A7，Map不变 |
| Cycle Scope | PASS | R1修owner调度并清临时诊断，R2恢复既有QEMU资格；无新成果 |
| Task Contracts | PASS | 两个repair含位置、RED/GREEN、保持/禁止项、验证和停止条件 |
| Traceability | PASS | R1/R4–R8→D2/D3/D5–D8→T4.2-R1/R2→代码/host/QEMU见证，无Missing |
| Verification | PASS | unit→crate→host/build→QEMU→compat按依赖递增，直接观察step、epoch、terminal和exit |
| Evidence | PASS | 新Cycle最多README/raw serial/regressions/按需pcap四文件；全change不超过20 |
| Authorization | PASS | 用户已审计并批准本rework执行契约（「更改gate状态，开始实施」），Plan Context 改为 `ready`，Act 开始执行 T4.2-R1 |

**Persisted Evidence**

- Mode: required
- Path: `evidence/007-single-hart-qemu-qualification/005-rework/`
- `README.md`：支持A2/A4–A7；汇总命令、环境、exit、首个失败层和artifact判定。Act Response无法保存
  一次性手工session的完整marker关系，缺失时不能Review runtime终态。
- `qemu-serial.log`：支持A2/A4–A6；保存P8 preflight与完整MS07 marker/exit，或BLOCKED时首个决定性
  失败段。手工HMP session不能低成本重跑，缺失时不能判断reset/epoch/socket资格。
- `regressions.txt`：支持A7；仅在进入四组回归后保存各组终态和exit。多组QEMU手工运行不能低成本
  重跑，缺失时不能接受兼容性Gate。
- `usernet.pcap`：仅当serial不足以判断A4/A6的packet交付或出现新网络失败时创建；缺少packet层证据
  会阻止对应归因。serial充分时在README写`not needed`，不创建占位。
- Budget: 本Cycle最多4个文件；整个change当前10个，完成后最多14个。Cycle 004既有6文件超限只作为
  历史ACT-DEVIATION报告，不向其追加，也不把它们当作本Cycle PASS Evidence。

**Risks and Notes**

- 10ms是QEMU恢复控制面的有界调度节拍，不是设备物理时序结论；最终正确性仍由2s absolute deadline
  和状态结果决定。
- event与timer可能同时wake；stale timer允许额外一次bounded poll，不得推进已关闭状态或重复step。
- sandbox `EPERM`只能跳过精确socket子Gate，不能跳过无socket host tests、build或QEMU结果。
- Cycle 004诊断日志已足以定位；新Cycle不得为证明同一根因继续堆叠raw日志。

## Act Response

- Status: blocked

**Implemented**

T4.2-R1 完成；T4.2-R2 等待用户手工 single-hart QEMU runtime（见 Blocker Handoff）。T4.2-R1 拆解如下：

1. **分离最终 stage deadline 与 bounded progress wake（R1/R4/R5/R8）**：`RxRxFuture` 新增
   `recovery_progress_wake: Option<u64>`（reset/reinitialize 阶段 Pending 时下一次 one-shot wake 时刻），
   常量 `RECOVERY_PROGRESS_CADENCE_NS = 10_000_000`（10 ms）。新增 `arm_recovery_progress(cx)`：wake 时刻取
   `min(now + 10 ms, recovery_deadline)`，生产态注册 axtask `sleep_until` one-shot timer，测试态只记录决策供
   deterministic clock。它**不修改** `recovery_deadline`，因此同阶段 Pending 绝不续期最终 deadline。
2. **调用点接线**：quiesce→resetting 过渡 与 `recovery_step` 的 Pending 分支由 `arm_recovery_timer`（仅最终
   deadline）改为 `arm_recovery_progress`（progress wake）。quiesce 入口仍用 `arm_recovery_timer`（grace drain
   靠 budget self-wake，不引入 cadence）。`Active` 正常路径不获得任何进度 timer。
3. **终端清理**：`recovery_progress_wake` 在 `enter_recovery`、`enter_drift_quarantine`、
   `publish_recovery_fault`、`recovery_step` Recovered 分支统一置 `None` 并 `cancel_recovery_timer`，保证
   reset 确认/Fault/第一次进入恢复前没有残留进度 timer 或 stale wake。
4. **deadline 语义不变**：`poll_recovery` 的 Resetting/Reinitializing 分支仍先查 `recovery_deadline`，
   未到才执行 driver step；达到 deadline 仍 Faulted + quarantine。1 s quiesce / 2 s reset/reinit 数值未改。
5. **移除 Cycle 004 临时诊断（T4.2-R1 收尾）**：删除 `async_rx.rs` 的 `last_recovery_state_diag` 字段、其
   `RX recovery state` 日志块与 `use log::info`；删除 `axdriver_virtio/net.rs` 的 `last_reset_poll_diag`
   字段、其 `poll_recovery_step` 日志块与 `use log::info`。`reset_confirmed()` 的产品判定路径保留。

**Changed Files and Symbols**

- `crates/axnet/src/async_rx.rs`：`RxRxFuture::recovery_progress_wake`、
  `RECOVERY_PROGRESS_CADENCE_NS`、`arm_recovery_progress`（prod+test）、`enter_recovery`/
  `enter_drift_quarantine`/`publish_recovery_fault`/`recovery_step` 的清零与 `cancel_recovery_timer`、
  两处 `arm_recovery_progress(cx)` 调用点、7 处构造器 init、删除 `last_recovery_state_diag` 与
  `use log::info`；新增测试 `pending_reset_schedules_next_progress_wake_before_deadline`、
  `delayed_reset_confirmation_recovers_after_multiple_steps_before_deadline`。
- `crates/axdriver_virtio/src/net.rs`：删除 `last_reset_poll_diag`、其 `poll_recovery_step` 日志块与
  `use log::info`；无其他改动。

**Deviations from Plan**

- 无实质偏差。非实质记录：
  - `arm_recovery_progress` 生产态在 `now >= deadline` 防御分支调用 `cx.waker().wake_by_ref()`，与既有
    `arm_recovery_timer` 行为一致；从本 Cycle 调用点看该项实际不可达（deadline 已于进入前建立），仅是防御。
  - 测试态 `arm_recovery_progress` 与生产态重复计算 wake 时刻，镜像既有 `arm_recovery_timer`/
    `arm_data_stage_timer` 的 prod/test 成对模式，保证 deterministic witness 看到与生产相同的决策值。
  - `cargo fmt --manifest-path crates/axdriver_virtio` 曾对同 crate 无关文件（blk/gpu/input/lib/socket）产生
    rustfmt 重排，已全部 `git checkout` 还原，未纳入本 Cycle diff。
  - `recovery_progress_wake` 为非常cfg字段（生产也用），默认 `None`；仅实际 one-shot timer 受 prod/test 分隔。

**Blocker Handoff**

- Task/Step/Gate：T4.2-R2（single-hart QEMU 资格）——能力边界 + **真实产品 bug**。
- Gate 6 类型：能力边界 + 新增产品缺陷。T4.2-R1 全部自动 Gate（axnet ordinary 474、
  qemu-diagnostics 506、axdriver_virtio 36、virtio-drivers 43、axdriver_net 12、`make host-test` exit 0、
  kernel build exit 0、rustfmt、`git diff --check`、`openspec validate --strict`）均 GREEN。
- **手工 QEMU 运行结果**：probe 的 preflight 四边界、pre_reset_traffic、reset_request 均 PASS；
  `old_socket_terminal` 期间 reset 推进到 `Reinitializing (lifecycle=7, dev=0 quar=64)` 后，
  **owner 重新打开 Active 收包时内核 panic**：
  `crates/axdriver_virtio/src/net.rs:638 receive() rx_buffers[token=28526]`（OOB，len=64）。
- **根因（已定位到源码，非猜测）**：
  `axdriver::virtio::VirtIoHalImpl::dma_alloc`（registry `axdriver-0.3.0-preview.2/src/virtio.rs:169`）
  直接返回 `global_allocator().alloc_pages(pages, 0x1000, UsageKind::Dma)` 的页，**未清零**，违反
  `virtio-drivers::Hal::dma_alloc` 契约（`crates/virtio-drivers/src/hal.rs:90` “The pages must be
  zeroed”）。`reinit_prepare → VirtQueue::new → VirtQueueLayout::allocate_flexible → Dma::new` 复用/重新
  分得旧的未清零 DMA 页，重建的 RX used-ring 含残留 `used.idx` 与 `ring[].id`；
  `pol_receive → peek_used` 在 `can_pop()`（`last_used_idx=0 != used.idx`）时读到垃圾 descriptor id
  28526（设备只能完成合法 id 0..63，故 28526 必然来自 guest 读到未写/残留的设备可见内存）→
  `rx_buffers[28526]` 越界 panic。
- **为何此前未暴露**：(1) T4.2-R1 的 progress-wake 修复让真 QEMU 上 reset 第一次真正走完
  `Resetting→Reinitializing` 并重开 Active RX，才触发重建队列收包；此前 reset 卡在 Resetting 2 s
  TIMEOUT Faulted，重建路径未在 runtime 执行过。(2) fake transport 测试用确定/干净 ring 内存，抓不到
  `dma_alloc` 未清零的真实情况。axdriver_virtio fake 36/36、virtio-drivers 43/43 全绿不冲突。
- **影响**：MS07 六 case 不能继续（panic 即 FAIL）；T4.2-R1 的修复本身符合预期（reset 已推进），但暴露
  下游重建 RX used-ring 的真实缺陷。此缺陷超出 T4.2-R1 争议（T4.2-R1 只改 owner 调度，未更动
  `dma_alloc`/VirtQueue），应按 T4.2-R2 契约 Forbidden（“遇新产品错误继续猜测修复”）回 Plan 修，
  不在此 Cycle 内盲改。
- 已完成：T4.2-R1（全部自动 Gate）；手工 QEMU preflight/pre_reset/reset_request 部分 PASS。未完成：
  old_socket_terminal/new_epoch/hmp_down/hmp_up 六 case 后续、validator、四组回归。
- 工作区状态：HEAD `b83e800a`；Cycle 004 staged；T4.2-R1 改动位于工作树（未 commit）；`scripts/cc-nopie.sh`
  在 `/tmp/opencode/cc-nopie.sh` 符号链接（既有环境前提）。手工 QEMU 现场保存于
  `evidence/007-single-hart-qemu-qualification/005-rework/qemu-serial.log`（含 panic）。
- `required` Evidence 未创建：QEMU 六 case/validator/四组回归未全部完成且遇真实 bug；遵循 evidence 规则
  阻塞不采集。
- **恢复条件（路由建议）**：调用 `openspec-plan` 就“重建 RX used-ring 未清零（dma_alloc 违反 Hal 契约）”
  开一个新的 rework/repair Cycle（可同时把 qemu-serial.log 的 panic 现场作为 Evidence），修复后重跑
  MS07 六 case + validator + 四组回归。修复方向（供 Plan 评估，不在此实施）：在 `dma_alloc`/`Dma::new`
  清零 DMA 页，或 `VirtQueue::new` 显式清零其 owned used-ring，符合 Hal 契约。

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance（T4.2-R1）: PASS
- Full diff reviewed: PASS
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 0

T4.2-R1 逐项对照：10 ms one-shot progress wake 严格 `min(now+10ms, deadline)`；
`pending_reset...` RED→GREEN 证明原实现只安排最终 deadline，修复后 wake 严格晚于 now 且不晚于 deadline、
counting waker 在 Pending round 不增长（无即时 self-wake）；`delayed_reset_confirmation...` 证明 stall 多轮
（step_calls 2→3→4）后确认并恢复、epoch 恰好 +1、进度 wake 清零；`same_stage_pending_does_not_renew...`
保持 GREEN 证明 deadline 不续期、never-confirmed 在原 deadline Faulted；Active 无周期 timer；
reset 确认/Fault/首次进入前均清除进度 timer。既有 recovery/link/socket tests 全绿无退化。
**T4.2-R1 无遗留 Critical/Important；验证无误。T4.2-R2 不再只是等待用户**——用户已跑 QEMU，
在 `old_socket_terminal` 暴露重建 RX used-ring 真实 bug（见 Blocker Handoff），Cycle 因此 blocked，
该 bug 需 openspec-plan 开新 rework Cycle 修（超出 T4.2-R1 范围，禁止在此盲改）。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| focused RED→GREEN | `cargo test ... pending_reset_schedules...` 修复前 | FAILED（expect panic: no next progress wake）→ 修复后 `ok 1 passed` | PASS |
| focused progress | `cargo test ... delayed_reset_confirmation...` | `ok. 1 passed; 0 failed` | PASS |
| 既有同阶段 | `cargo test ... same_stage_pending/same_stage` | `ok. 2 passed; 0 failed` | PASS |
| axnet ordinary 全量 | `RUSTFLAGS=... cargo test -p axnet --lib -- --test-threads=1` | `ok. 474 passed; 0 failed` exit 0 | PASS |
| axnet qemu-diagnostics | 同命令 + `--features qemu-diagnostics` | `ok. 506 passed; 0 failed` exit 0 | PASS |
| axdriver_virtio net | `cargo test -p axdriver_virtio --features net` | `ok. 36 passed; 0 failed` | PASS |
| virtio-drivers alloc | `cargo test -p virtio-drivers --features alloc` | `ok. 43 passed; 0 failed` | PASS |
| axdriver_net | `cargo test -p axdriver_net` | `ok. 12 passed; 0 failed` | PASS |
| host-test（无 socket EPERM） | `make host-test` | `EXIT=0`（全链通过，本环境允许 socket） | PASS |
| kernel build | `make ARCH=riscv64 build` | `Finished release`，`.bin` 生成，exit 0 | PASS |
| 手工 QEMU（部分） | single-hart QEMU + probe `--run` | preflight 4 边界、pre_reset_traffic、reset_request PASS；old_socket_terminal 期间 reset 到 Reinitializing 后 `rx_buffers[28526]` OOB panic | FAIL（真实 bug） |
| 格式/静态 | rustfmt（axnet check；net.rs 单独 non-crate-wide） | 均 exit 0 | PASS |
| diff 白测 | `git diff --check` | exit 0 | PASS |
| OpenSpec | `openspec validate ms07-... --strict` | `Change ... is valid`, exit 0 | PASS |

**Persisted Evidence**

T4.2-R1 全部自动 Gate 已由 Act Response 可低成本重跑的验证表保存，不需单独证据。手工 QEMU 现场
（含 panic）已保存于 `evidence/007-single-hart-qemu-qualification/005-rework/qemu-serial.log`，但**不作为
本轮合格 Evidence**（六 case 因真实 bug 未完成）。Plan 的 `required` Evidence（`README.md`、
`regressions.txt`、按需 `usernet.pcap`）待新 rework Cycle 修复并通过 QEMU 六 case + 四组回归后按
evidence-format 采集。

**Experience Candidates**

- **Incident（候选）**：真 QEMU 上 reset→reinit 后重建 RX used-ring 读垃圾 descriptor id 导致
  `rx_buffers[28526]` OOB panic；根因为 `axdriver::VirtIoHalImpl::dma_alloc` 未清零 DMA 页、违反
  `virtio-drivers::Hal::dma_alloc` 契约。这是一次强诊断价值的系统性故障（跨 owner 调度→驱动→HAL→
  DMA 内存），`qemu-serial.log` 保留现场。是否落为 Incident 由 `openspec-experience-recorder` 评估，
  Act 不创建。
- `scripts/cc-nopie.sh`（`/tmp/opencode/cc-nopie.sh` 符号链接）为既有环境前提（Cycle 004 已记录），
  非本 Cycle 新增。

**Remaining Issues**

- 阻塞代码项：重建 RX used-ring 未清零（`dma_alloc` 违反 Hal 契约），需 openspec-plan 开新 rework Cycle。
- Cycle blocked 根因：能力边界已突破（用户已跑 QEMU），现为**真实产品 bug**；T4.2-R1 无遗留问题。

**Commit or Diff Reference**

Diff reference: `git diff`（工作树，未提交）——T4.2-R1 变更跨 `crates/axnet/src/async_rx.rs`、
`crates/axdriver_virtio/src/net.rs`。commit 未建（未获提交授权）；`crates/axdriver_virtio` 曾因 rustfmt 产生的
无关文件重排已还原。`openspec/.../007-single-hart-qemu-qualification/004-replan.md` 的未提交改动为既有工作树
内容，非本 Cycle 引入。手工 QEMU 现场 `qemu-serial.log`（含 panic）位于本 Cycle evidence 目录。

## Plan Review

- Review Result: rework-required

**Findings**

1. **Blocking — NEW-EVIDENCE：重建 virtqueue 消费了未清零的 DMA 页。** 手工 QEMU 已证明
   T4.2-R1 能把恢复从 Resetting 推进到 Reinitializing；紧接着 `VirtIoNetDev::receive()` 用
   `poll_receive()` 返回的 token `28526` 索引 64 项 `rx_buffers` 并 panic。合法队列只能返回
   `0..63`，该值不是正常 completion。调用链为
   `reinit_prepare -> VirtQueue::new -> VirtQueueLayout::allocate_flexible -> Dma::new ->
   axdriver::VirtIoHalImpl::dma_alloc`。本地 `Hal::dma_alloc` 明确要求页已清零，而 registry
   `axdriver-0.3.0-preview.2` 只调用 `global_allocator().alloc_pages`；后者只分配页，不清零。
   因此复用页中的旧 `used.idx/ring[].id` 被当作新队列 completion，直接阻塞 A2/A4–A7。
2. **Blocking — PLAN-OMISSION：现有 fake HAL 无法见证脏页重用。** `hal::fake::FakeHal` 使用
   `alloc_zeroed`，所以 virtio-drivers 43/43 与 axdriver_virtio 36/36 全绿并不覆盖 production HAL
   违反零页前置条件的现场。返工必须先用故意填充非零字节的 test HAL 建立 RED，再证明
   `Dma::new` 返回前清零整个 owned region，且新建 modern/legacy queue 不会凭空 `can_pop()`。
3. **Non-blocking — ACT-DEVIATION：Evidence 描述自相矛盾。** Blocker Handoff 称 required
   Evidence“未创建”，但本 Cycle 实际已有一个 `qemu-serial.log`，Act Response 后文也正确引用它。
   该文件是有效的 BLOCKED 原始现场，不是六 case PASS Evidence；不要求删除或补造 README。
4. **Non-blocking — 保护边界。** 在 `receive()` 对 token 加 bounds check 只能把 ring 损坏改成
   `BadState`/静默故障，不能恢复队列所有权；整包 vendoring registry `axdriver` 又会无必要扩大
   change surface。下一 Cycle 应在已由 workspace patch 使用的本地 `virtio-drivers::Dma::new`
   兑现零页后置条件，同时保持 `Hal` 契约和上层 token invariant，不修改 receive 路径。

**Deviation Classification**

NEW-EVIDENCE；PLAN-OMISSION；ACT-DEVIATION。

**Acceptance Gaps**

- A2：最终 LOG=warn 资格 run 在 Reinitializing 后发生 kernel panic。
- A4：仅 pre-reset peer phase 完成；post-reset 与 post-link-up 未进入。
- A5：恢复未提交新 QueueEpoch/SocketEpoch，也未恢复 64/64/0 owner ledger。
- A6：old/new socket、validator exit 0 与无 owner drift/panic 尚未取得。
- A7：MS01/MS04/MS05/MS06 手工兼容回归尚未执行。

T4.2-R1 的自动 Acceptance 已满足：10 ms one-shot 能驱动 bounded step，same-stage Pending 不续期、
不即时 self-wake，delayed confirmation 能恢复，永久 Pending 仍按原 deadline Faulted。

**Convergence**

reduced。相对 Cycle 004，Cycle 005 已关闭 owner 在最终 deadline 前没有执行机会的缺口，真实 QEMU
从 Resetting timeout 推进到 Reinitializing；当前失败位于下一层 DMA/virtqueue 初始化契约，不是
同一调度问题的重复失败。

**Evidence**

- BLOCKED runtime：
  `evidence/007-single-hart-qemu-qualification/005-rework/qemu-serial.log`。决定性记录为
  `pre_reset_traffic` 与 `reset_request` PASS，随后 `lifecycle=7, dev=0, quar=64`，并在
  `net.rs:638` 以 `len=64, index=28526` panic。
- 产品调用链：`crates/axdriver_virtio/src/net.rs::{reinit_prepare,receive}`；
  `crates/virtio-drivers/src/queue.rs::{VirtQueue::new,VirtQueueLayout::allocate_flexible}`；
  `crates/virtio-drivers/src/hal.rs::Dma::new`。
- 契约冲突：本地 `Hal::dma_alloc` 要求“The pages must be zeroed”；registry
  `axdriver::virtio::VirtIoHalImpl::dma_alloc` 和 `axalloc::GlobalAllocator::alloc_pages` 均无清零动作。
- 测试缺口：`hal::fake::FakeHal::dma_alloc` 使用 `alloc_zeroed`；新鲜本地基线
  `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --locked --offline --lib --features alloc`
  为 43/43 PASS，证明现有 suite 未覆盖脏页 postcondition，而不是否定 runtime 根因。

**Follow-up Decision**

创建同一 Iteration 的 `006-rework.md`。修复是满足既有“reset 确认后安全重建并恢复 I/O”的必要条件，
不改变 requirement、deadline、epoch、owner、socket 或 QEMU 验收边界，因此不 replan。新 Cycle 为
DMA 零页 postcondition、脏页 RED/GREEN、邻接回归和剩余手工资格提供新的自包含执行契约。

**Iteration Plan Update**

None.

**Next Cycle**

`006-rework.md`

**Next Iteration**

None.
