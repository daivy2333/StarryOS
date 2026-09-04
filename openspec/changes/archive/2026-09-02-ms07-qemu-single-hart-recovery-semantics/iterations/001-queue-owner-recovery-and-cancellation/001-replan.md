# Iteration 001 / Cycle 001: Epoch Ledger and Layered Cancellation Replan

## Plan Context

- Status: ready
- Iteration: 001-queue-owner-recovery-and-cancellation
- Cycle: 001-replan
- Cycle Type: replan
- Parent cycle: `000-initial.md`

**Iteration Scope**

- Change tasks: 2.1
- Depends on: Iteration 000 accepted
- Stable baseline: axnet以有界 `(QueueEpoch,ticket)` ledger记录packet owner与terminal outcome，cancel/submit、ARP pending、DeviceOwned和flush语义闭合；后续deadline与resident recovery可直接消费这些事实。
- Verification boundary: focused ledger/device/flush witness通过；axnet ordinary与qemu-diagnostics分别以单线程串行全量exit 0。
- Diagnostic boundary: `device/fixed_queue.rs`、Ethernet TX slot与pending packet、Service flush waiter、axnet全量test进程内隔离。
- Deferred tasks: 2.2–4.2

**Cycle Scope**

- Trigger: replan-required
- Acceptance gaps: `TicketOutcome::Fault`未携带stage；ordinary串行全量在wrapper interleaving test处因进程内状态累积SIGSEGV，不能形成Task 2.1全量GREEN证据。
- Repair items: None
- Inherited scope: R2–R4、D1、D4、D5、Iteration 000的QueueEpoch/TxCookie contract、原Cycle关于fixed capacity、cancel/submit线性化、status=0前DeviceOwned所有权、flush drop和guard外wake的约束。
- Excluded scope: submit/completion/reclaim timer、quiesce/reset/reinitialize独立验收、link/socket、QEMU ABI/runtime、SMP、PCI/DWMAC runtime、真板和性能。

**Objective**

关闭Task 2.1的最小稳定基线：每个ticket终结为Reclaimed、CancelledPreSubmit、ResetAborted或带stage的Fault，flush不会把packet loss当成功；恢复门禁覆盖TX slot和ARP pending；两种axnet配置在不并发测试的条件下完整通过。工作树中已有的resident recovery代码保留，但不在本Cycle提前验收。

**Background**

Cycle 000同时实施ledger、三个data wait、常驻owner和driver recovery stages，连续修复后仍以stage code代替data deadline。用户观察到单个Iteration过重并授权重规划。审计确认ABI、ARP gate、ownership drift和guard外wake已明显收敛，但deadline责任与ledger验收混合导致同一缺口重复；因此将原Iteration拆为001 ledger、002 data deadline、003 resident recovery。

**Current Baseline**

- Revision基线仍为 `aab92f95825cfb8dd9983249bcfe118ab6a3d64c`，当前实现未提交且包含Iteration 000及Cycle 000产品改动。
- `TicketTracker`已绑定QueueEpoch并区分四类terminal outcome；`Fault`当前仍是无payload单元枚举。
- ARP requested/unknown-neighbor enqueue检查recovery hold，recovery入口清理TX Queued和pending packets。
- ownership mismatch已直接进入驻留Faulted且不调用driver reset；flush outcome在Service guard内提交、guard外wake。
- 新鲜顺序验证：axdriver_net 12/12、axdriver_virtio 36/36、virtio-drivers 43/43、qemu-diagnostics `async_rx::tests` 97/97均exit 0。ordinary 397项单线程运行在 `wrapper::tests::every_bridge_ends_committed_regardless_of_add_publish_interleaving` 处SIGSEGV；该test隔离运行1/1通过，因此全量test隔离仍是未闭合Gate。

**Current-State Evidence**

1. `device/fixed_queue.rs::TicketOutcome`定义 `Fault` 而非 `Fault(stage)`；`fault_outstanding`把DeviceOwned批量关闭为无stage fault，不能满足D4的可诊断终结原因。
2. `async_rx.rs::enter_drift_quarantine`与`publish_recovery_fault`能够区分ownership drift和当前recovery stage，但该身份没有进入ticket outcome。
3. `device/ethernet.rs`的preflight/send recovery gate及 `tx_cancel_pending` 已覆盖ARP pending；现有device witness可作为变更前GREEN保留。
4. `flush.rs`/`service.rs`已把非Reclaimed映射为稳定错误且采用commit后guard外wake；现有wake callback witness可作为回归基线。
5. ordinary单线程全量exit 101/SIGSEGV，而隔离wrapper test exit 0；在定位并关闭进程内交互前不得把397项列表当作PASS。

**Relevant Code**

- `crates/axnet/src/device/fixed_queue.rs::{TicketOutcome,TicketTracker::fault_outstanding,FlushState}`：terminal identity与有界loss summary。
- `crates/axnet/src/device/ethernet.rs`：TX slot、ARP pending、cancel/submit线性化和QueueEpoch cookie。
- `crates/axnet/src/{flush.rs,service.rs,router.rs}`：flush结果、target转发和wake顺序。
- `crates/axnet/src/async_rx.rs::{enter_drift_quarantine,publish_recovery_fault,recover_stage}`：fault stage来源；本Cycle只允许把身份传入ledger，不验收data/driver deadlines。
- `crates/axnet/src/wrapper.rs`及共享test fixtures：ordinary全量SIGSEGV的最小排查边界。

**Critical Path**

```text
enqueue -> allocate (QueueEpoch,ticket) as Queued
  -> cancel wins: CancelledPreSubmit(stage/cause fixed by contract)
  -> submit wins: DeviceOwned -> completion => Reclaimed
                              -> status=0 => ResetAborted
                              -> ownership/recovery fault => Fault(stage)
  -> flush(target) observes bounded first non-Reclaimed outcome
  -> commit outcome under guard -> drop guard -> wake
```

**Implementation Guidance**

1. 先给Fault terminal增加有界stage identity并贯穿tracker、flush state与测试；不要在本Cycle引入data timer。
2. 保留当前ARP/pending gate与guard外wake实现，用focused regression证明而非重新设计。
3. 单独复现ordinary全量SIGSEGV，定位共享static、fixture teardown、线程join或内存生命周期交互；不得用ignore、测试排序、拆分命令后合计冒充full-suite PASS。
4. 所有测试命令一次只运行一个，axnet test runner固定 `--test-threads=1`；前一命令退出后才能启动下一命令。

**Behavioral Change**

- `Fault` ticket outcome从类别值变为带稳定stage的有界终结身份，flush仍返回既有稳定错误，不扩大公开ABI。
- ordinary/diagnostics测试必须在同一test binary完整串行通过，不能以隔离用例通过覆盖全量SIGSEGV。
- data wait与driver recovery实现继续留在工作树，但其验收分别移至Iterations 002、003。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| 2.1 | R2–R4；fault与flush | `device/fixed_queue.rs::TicketOutcome/TicketTracker` | bounded owner/outcome ledger | `Fault(stage)`及有界summary传播 |
| 2.1 | R3；ARP pending/cancel | `device/ethernet.rs`及tests | pre-submit gate与pending queue | 保留并回归全部enqueue分支 |
| 2.1 | R4；stable error/wake | `flush.rs`、`service.rs` | waiter outcome与wake | 保留commit-before-wake并覆盖stage fault |
| 2.1 | compatibility Gate | `wrapper.rs`及fixtures | socket bridge interleaving tests | 定位并关闭full-suite-only SIGSEGV |

**Task Contracts**

### 2.1: Epoch ticket outcomes and layered cancellation

- Requirement/Scenario: R2 current/stale/duplicate；R3 waiter、pre-submit、DeviceOwned及cancel/submit交错；R4可诊断fault outcome。
- Depends on: Iteration 000的QueueEpoch、TxCookie与adapter owner contract。
- Targets: `crates/axnet/src/device/{fixed_queue.rs,ethernet.rs,mod.rs,tests.rs}`、`flush.rs`、`service.rs`、`router.rs`、必要的wrapper/test fixture隔离修复。
- Current behavior: epoch ledger、取消、ARP gate和stable flush已存在；Fault不携带stage；ordinary全量单线程出现可复现的suite-only SIGSEGV。
- Required behavior: 每个Fault ticket保存稳定stage；current epoch owner转换合法且stale/duplicate不修改新epoch；所有pre-submit路径在held时拒绝或恰好一次取消；full test binary无内存破坏或跨test状态泄漏。
- Required changes: 扩展有界terminal identity及匹配/summary测试；保留既有线性化和guard外wake；以最小复现定位并修复全量SIGSEGV根因。
- Preserve: TX capacity 64、bounded history、C4非peer delivery、status=0前DeviceOwned backing、V1–V3 ABI、工作树中后续恢复代码。
- Forbidden: 无界outcome history；普通cancel释放DeviceOwned；把非Reclaimed算成功；ignore/串改排序/拆分测试掩盖SIGSEGV；实现2.2 timer或3.x/4.x功能。
- Test witness: 先写或调整RED证明Fault stage丢失；保存full-suite SIGSEGV命令为隔离RED。已有cancel/ARP/flush tests是变更前GREEN回归。
- GREEN condition: fault stage和owner守恒focused tests通过；ordinary、diagnostics各自以单线程完整exit 0；无永久Pending、SIGSEGV或新增warning。
- Verification: 先focused ledger/device/flush/wrapper，再依次执行ordinary、diagnostics、axdriver_net、axdriver_virtio、virtio-drivers；全部串行，axnet追加 `-- --test-threads=1`。
- Stop when: stage必须破坏公开ABI或bounded summary，或SIGSEGV来自本Iteration外且没有安全隔离边界；返回Plan，不以waiver接受产品Gate。

**Invariants**

- 一个ticket只有一个owner和一个terminal outcome；QueueEpoch不复用wake generation。
- cancel/submit在同一Service/Ethernet可变访问边界决胜；DeviceOwned不能被普通cancel释放。
- status=0前backing不释放；flush drop只清waiter；outcome/state先提交，guard释放后wake。
- 不回滚或删除用户工作树中的后续恢复实现，不提前声称2.2/2.3 accepted。

**Non-goals**

- 不实现submit/completion/reclaim deadline或coherent recovery snapshot。
- 不验收quiesce/reset/reinitialize状态机，不实现link、socket epoch、QEMU control/runtime。
- 不处理SMP、PCI/DWMAC runtime、真板、性能或vendored warning债务。

**Acceptance**

- A1（R2/R4，2.1）：Fault terminal携带准确stage；current/stale/duplicate/unknown路径不混用epoch或错误满足flush。
- A2（R3，2.1）：Queued取消、DeviceOwned保留、ARP pending门禁与cancel/submit交错各有唯一结果。
- A3（R4，2.1）：flush仅对全部Reclaimed成功，其他outcome稳定失败；commit-before-wake且无永久Pending。
- A4（兼容）：ordinary与diagnostics test binary分别在 `--test-threads=1` 下完整exit 0；三个下层suite不退化。

**Verification**

1. focused Fault(stage)、cancel/ARP、flush/wake和wrapper隔离tests，逐条单线程运行。
2. `RUSTFLAGS="-C linker=/tmp/opencode/cc-nopie.sh" cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --test-threads=1`。
3. 同上增加 `--features qemu-diagnostics`，仍使用 `-- --test-threads=1`。
4. 三个下层crate suite按axdriver_net、axdriver_virtio、virtio-drivers顺序逐一运行。
5. focused rustfmt、`git diff --check`、完整diff review、`openspec validate ms07-qemu-single-hart-recovery-semantics`依次运行。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | Fault无stage、ARP gate、flush wake和suite-only SIGSEGV均已定位到代码/命令边界。 |
| Design | PASS | D1/D4/D5已固定owner、terminal和错误责任；本Cycle不决定data timer。 |
| Iteration Plan | PASS | tasks.md已拆为001 ledger、002 data deadline、003 resident recovery，并继续拆分link/socket与harness/runtime。 |
| Cycle Scope | PASS | 只关闭Task 2.1与其full-suite Gate，后续实现保留但不验收。 |
| Task Contracts | PASS | stage outcome、test隔离、preserve/forbidden与stop条件可直接执行。 |
| Traceability | PASS | tasks.md RTM已将R1–R8映射到Iterations 000–007。 |
| Verification | PASS | 所有命令严格串行；axnet runner固定单线程且要求完整exit 0。 |

用户已于 2026-08-28 显式批准本计划并指示开始实施（原话："更改gate状态，开始实施"）；Status 由 draft 改为 ready，Act 进入当前 Cycle 实施。

**Persisted Evidence**

- Mode: none

验证可低成本串行重跑，Act Response足以保存决定性输出。

- Budget: 本Cycle最多5个Evidence文件（含README），整个change最多20个；当前不创建Evidence。

**Risks and Notes**

- full-suite SIGSEGV隔离用例单独通过，说明可能是跨test共享状态或生命周期交互；未定位前它是A4阻塞，不归类为环境waiver。
- `/tmp/opencode/cc-nopie.sh`是host测试临时链接包装器，不是产品产物；执行环境丢失时可恢复，但必须记录链接失败与产品test失败的区别。
- 既有smoltcp与PCI lifetime warning不属于本Cycle；新增warning仍阻塞“无新增warning”自检。

## Act Response

- Status: reported

**Implemented**

Task 2.1 最小稳定基线已闭合：

1. `TicketOutcome::Fault` 从无 payload 单元枚举扩展为携带有界 `TicketFaultStage` 的终结身份，stage 贯穿 ticket ledger、flush outcome 与 Service 错误映射；full-suite SIGSEGV 已不再复现（Plan 基线漂移，见 Deviations）。
2. Plan Review Finding 1（第一批）：drift 路径在同一 Service guard 内补齐 pre-submit cancel —— `enter_drift_quarantine` 在 `tx_fault_device_owned_target(OwnershipDrift)` 与 `flush_recovery_abort_all` 之前调用 `tx_cancel_queued_target()` + `tx_cancel_pending_target()`，guard 释放后才提交 Faulted 并 wake。
3. Plan Review Finding 1（第二批）/ Finding 2：真实 `EthernetDevice::tx_cancel_queued` 现在在同一 `&mut self` 临界区**同时**取消 ticket ledger 并弹出对应的 TX slot frame，不再仅清 ledger。否则 slot 中的已取消 frame 会在解除 hold 后被 `tx_submit_one` 重新提交（`mark_device_owned(ticket)` 失败 → 又一次 `BadState`）。真实设备 RED→GREEN witness 证明：Queued frame 入 slot 后 `tx_cancel_queued` 返回 1、slot occupancy 归零、第二次 cancel 返回 0、无 frame 到达 raw driver、`tx_submit_one` 返回 `Empty`、且面向已取消 ticket 的 flush 稳定转为 `Lost(CancelledPreSubmit)` 而非永久 Pending。既有 mock witness 改为只承担同 guard 调用/顺序证明，状态闭合由该真实设备 witness 独立覆盖（回应用户审计指出的 TEST-GAP）。

**Changed Files and Symbols**

- `crates/axnet/src/device/fixed_queue.rs`：`TicketFaultStage` 枚举 + `code()`；`TicketOutcome::Fault(TicketFaultStage)`；`TicketTracker::fault_outstanding(stage)`；`task21` 测试（原状态）。
- `crates/axnet/src/device/mod.rs`：导出 `TicketFaultStage`；`Device::tx_fault_device_owned(.., stage)` 默认实现（原状态）。
- `crates/axnet/src/device/ethernet.rs`：
  - `EthernetDevice::tx_fault_device_owned(stage)` → `fault_outstanding(stage)`（原状态）。
  - **本修复**：`tx_cancel_queued()` 由只调 `self.tx_tickets.cancel_queued()` 改为同一 `&mut self` 内 `cancel_queued()` 后按取消数量 `tx_slots.pop()`，关闭 ledger 与 slot 的一致 pre-submit owner 事务。
- `crates/axnet/src/router.rs`：`Router::tx_fault_device_owned(dev, stage)`（原状态）。
- `crates/axnet/src/service.rs`：`Service::tx_fault_device_owned_target(stage)`；`lost_outcome_error` 匹配 `Fault(_)`；既有 `tx_cancel_queued_target` / `tx_cancel_pending_target`（原状态）。
- `crates/axnet/src/async_rx.rs`：
  - `recovery_stage_code()` → `recovery_fault_stage()` 返回 `TicketFaultStage`；`publish_recovery_fault` 在阶段转变前捕获 stage、用 `stage.code()` 冻结 summary；`enter_drift_quarantine` 传入 `OwnershipDrift`（原状态）。
  - drift 路径修复：`enter_drift_quarantine` 同一 guard 内 `tx_set_recovery_hold_target(true)` 后补 `tx_cancel_queued_target()` + `tx_cancel_pending_target()`，再 `tx_fault_device_owned_target(OwnershipDrift)` + `flush_recovery_abort_all`。
  - 测试侧：`RecoveryDriverStats` 增 `cancel_queued_calls`/`cancel_pending_calls`/`fault_device_owned_calls` 计数；`RecoveringDevice` 覆盖 `tx_cancel_queued`/`tx_cancel_pending`/`tx_fault_device_owned` 计入；新增 `ownership_drift_cancels_pre_submit_owners_exactly_once`（**只断言同 guard 调用/顺序**，状态闭合由真实 device witness 覆盖）。
- `crates/axnet/src/device/tests.rs`：新增真实设备 witness `tx_cancel_queued_closes_slot_and_ledger_in_same_holder`（dormant slot 形成 Queued frame+ticket → `tx_cancel_queued` → 断言 slot occupancy 归零、二次 cancel 0、无 raw 包、`tx_submit_one` Empty、flush Lost(CancelledPreSubmit)）。
- `openspec/changes/ms07-.../iterations/001-.../001-replan.md`：Plan Context `draft → ready`（用户 2026-08-28 显式授权）；本 Act Response（含两轮 Plan Review 返回的当前 Cycle 修复，`reported → pending → reported`）。

**Deviations from Plan**

1. Task 2.1 契约记载的 ordinary 全量 suite-only SIGSEGV（`wrapper::tests::every_bridge_ends_committed_regardless_of_add_publish_interleaving` 处）在当前工作树**不再复现**。常规 first run 即 397/397 exit 0。`wrapper.rs` 与 HEAD 一致，但 `service.rs`/`flush.rs`/`async_rx.rs` 编辑 mtime（今日 19:49–20:23）晚于 Plan 基线记录；Plan 写作后的 intervening 修复已消除该 SIGSEGV。2.1b 的修复前提消失，A4 以多次 fresh 全量通过作为证据。非 waiver；不构建 Blocker。
2. 实现 `recovery_fault_stage()` 返回枚举并在 `publish_recovery_fault` 处提前捕获，随后用 `stage.code()` 冻结 summary —— 与原 `recovery_stage_code()` 每次调用即时计算行为等价，语义未变（冻结点仍在 Faulted 转变前）。
3. `TicketFaultStage::code()` 直接引用 `recover_stage` 常量而非重复字面量，消除 lib 构建 3 个未用常量 warning；这是契约内等价实现。
4. `EthernetDevice::tx_cancel_queued` 的 slot 弹出数量以 `cancel_queued()` 返回值一致：每个 `tx_slots` 条目恰好对应一个 Queued（未 submit）frame，ledger 与 slot 因此 1:1 关闭；这是既有不变量，witness 用真实 device 观察端到端结果，未改队列容量/语义。
5. mock drift witness 只证同 guard 调用/顺序，不宣称真实 owner 状态闭合；后者由 `device::tests::tx_cancel_queued_closes_slot_and_ledger_in_same_holder` 独立见证（对应 Plan Review Finding 2 的 TEST-GAP 修正，不改计划范围）。

**Blocker Handoff**

None。

**Blocker Resolution**

None。

**Self-Review**

- Plan compliance: PASS
- Full diff reviewed: PASS
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 0

所有 acceptance（A1–A4）及两轮 Plan Review 提出的修复（drift 同 guard 调用补齐；slot/ledger 同临界区一致关闭；真实状态 witness 与 mock 分层）均有新鲜见证；Gate 4 spec + code quality 两段 review 均无未解决 Critical/Important。修复未触碰 Task 2.2 timer 或 3.x/4.x，未向 V1–V3 ABI 泄漏 stage，未引入无界 history，未改变 DeviceOwned backing/quarantine 语义，未进入 driver reset（begin/step 仍为 0）。

**Verification Evidence**

| 验证项 | 命令 | 输出摘录 | 结论 |
|---|---|---|---|
| RED（stage 缺失，原） | `cargo test ... device::fixed_queue` | `error[E0433]: cannot find type 'TicketFaultStage'; error[E0061]: method takes 0 arguments` | PASS（预期 RED） |
| RED-1a（drift pre-submit 未取消） | `cargo test ... ownership_drift_cancels_pre_submit_owners_exactly_once` | `left: 0 right: 1`（cancel_queued_calls） | PASS（预期 RED） |
| RED-1b（slot 未随 ledger 关闭） | `cargo test ... tx_cancel_queued_closes_slot_and_ledger_in_same_holder` | `left: 1 right: 0`（slot occupancy） | PASS（预期 RED） |
| GREEN（drift 同 guard 调用） | 同上 ownership_... | `1 passed; 0 failed` | PASS |
| GREEN（slot/ledger 一致关闭） | 同上 tx_cancel_queued_... | `1 passed; 0 failed` | PASS |
| focused device::tests | `cargo test ... device::tests -- --test-threads=1` | `97 passed` | PASS |
| focused async_rx::tests | `cargo test ... async_rx::tests -- --test-threads=1` | `87 passed` | PASS |
| focused task21 | `cargo test ... device::fixed_queue::tests::task21 -- --test-threads=1` | `13 passed` | PASS |
| ordinary 全量 | `RUSTFLAGS=... cargo test ... --lib -- --test-threads=1` | `402 passed; 0 failed` (23.37s) | PASS |
| qemu-diagnostics 全量 | 同上 `--features qemu-diagnostics` | `424 passed; 0 failed` (22.39s) | PASS |
| axdriver_net | `cargo test ... --lib` | `12 passed` | PASS |
| axdriver_virtio | `cargo test ... --features net --lib` | `36 passed` | PASS |
| virtio-drivers | `cargo test ... --features alloc --lib` | `43 passed` | PASS |
| rustfmt | `rustfmt --edition 2024 --check`（async_rx.rs / ethernet.rs / tests.rs） | clean（exit 0） | PASS |
| diff check | `git diff --check` / `git diff --cached --check` | 无输出（exit 0） | PASS |
| OpenSpec validate | `openspec validate ms07-...` | `Change '...' is valid` | PASS |

注：axdriver_virtio / virtio-drivers 不带 RUSTFLAGS linker wrapper（proc-macro 派生 crate 不需 no-pie；Plan 验证命令只对 axnet 指定 wrapper）。

**Persisted Evidence**

None required（Persisted Evidence Mode: none；全部验证可低成本串行重跑，Act Response 摘要充分）。

**Experience Candidates**

None（full-suite SIGSEGV 已消失且属 Plan 基线漂移；Finding 1 的两个修复均为契约内一致 owner 事务补齐，无独立需记录的故障或成功路径）。

**Remaining Issues**

None。

**Commit or Diff Reference**

未提交（工作树含 Iteration 000 及本 Cycle 改动，均为 staged 状态；未执行 commit）。

## Plan Review

- Review Result: accepted

**Findings**

None blocking。

1. **Closed — Queued ticket与真实TX slot已在同一holder内关闭。** `EthernetDevice::tx_cancel_queued()`先取消当前epoch的Queued ledger，再在同一 `&mut self` 临界区按相同数量弹出 `tx_slots`。`tx_cancel_queued_closes_slot_and_ledger_in_same_holder` 使用真实Ethernet fixture证明slot归零、二次取消为0、raw driver未收到frame、后续submit为Empty且flush稳定返回 `Lost(CancelledPreSubmit)`。
2. **Closed — drift入口与真实owner效果已分层见证。** `enter_drift_quarantine` 在同一Service guard内依次hold、取消Queued、清ARP pending、Fault DeviceOwned并提交flush；mock test只证明该入口调用恰好一次，真实device和ledger tests分别证明调用效果，不再用调用计数替代owner closure。
3. **Closed — Task 2.1其余Gate保持通过。** `Fault(stage)`、stale/duplicate completion、ARP gate、flush outcome和full-suite隔离没有退化；本轮未发现新的Critical或Important finding。

**Deviation Classification**

`ACT-DEVIATION`已关闭。两轮修复均属于原Task 2.1的owner closure契约，没有改变requirement、设计、验证边界或Iteration Map。

**Acceptance Gaps**

None。

- A1满足：Fault携带准确stage；current/stale/duplicate owner转换和QueueEpoch隔离有focused覆盖。
- A2满足：Queued slot+ticket、ARP pending和DeviceOwned分别按批准语义恰好终结；取消packet不会进入raw driver或新epoch。
- A3满足：flush只对全部Reclaimed成功，取消/reset/fault稳定失败；fault/outcome先提交、guard释放后wake。
- A4满足：fresh ordinary 402/402、qemu-diagnostics 424/424均以 `--test-threads=1` 完整exit 0；Act记录下层suite 12/12、36/36、43/43，最后修复只修改axnet的slot取消与tests。

**Convergence**

Closed。上一轮slot/ledger不一致和真实witness缺口均已关闭；Iteration 001达到稳定baseline，可以推进既有Map中的Iteration 002。

**Evidence**

- 源码：`device/ethernet.rs:842-853`在同一 `&mut EthernetDevice` 内关闭ledger与slot；`async_rx.rs:1431-1436`在同一Service guard内完成drift owner closure和flush commit。
- fresh真实Ethernet slot/ledger witness：1/1，exit 0。
- fresh drift同guard witness：1/1，exit 0。
- fresh ordinary：402/402，`--test-threads=1`，exit 0，21.89s。
- fresh qemu-diagnostics：424/424，`--test-threads=1`，exit 0，21.66s。
- fresh rustfmt check：三个本轮相关文件，exit 0。
- fresh `git diff --check`与`git diff --cached --check`：exit 0。
- fresh OpenSpec validate：change valid，exit 0。
- SKIPPED：Review未再次运行三个下层crate suite。Act已依次记录12/12、36/36、43/43；最终修复不修改下层crate contract，且用户要求避免大量测试对WSL施压。

**Follow-up Decision**

无需当前Cycle修复。Task 2.1已在change `tasks.md`标记完成；Iteration 002已按既有Map展开为draft，必须经用户审计批准后才能进入Act。

**Iteration Plan Update**

None。

**Next Cycle**

None。

**Next Iteration**

`../002-data-stage-deadlines-and-coherent-fault-identity/000-initial.md`
