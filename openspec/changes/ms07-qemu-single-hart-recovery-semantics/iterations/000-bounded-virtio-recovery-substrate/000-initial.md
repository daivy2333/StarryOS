# Iteration 000 / Cycle 000: Bounded VirtIO Recovery Substrate

## Plan Context

- Status: draft
- Iteration: 000-bounded-virtio-recovery-substrate
- Cycle: 000-initial
- Cycle Type: initial
- Parent cycle: None

**Iteration Scope**

- Change tasks: 1.1、1.2、1.3
- Depends on: None
- Stable baseline: transport-neutral bounded recovery contract和VirtIO adapter可在fake transport中安全完成或隔离整设备reset，epoch ledger与link snapshot独立可测。
- Verification boundary: `virtio-drivers`、`axdriver_net`、`axdriver_virtio`全量host tests通过；reset未确认无Drop/reuse，stale completion不命中新epoch。
- Diagnostic boundary: VirtIO status/config primitive、公共driver contract、adapter queue/buffer ledger。
- Deferred tasks: 2.1–4.2（axnet owner/cancel/deadline、link/socket epoch、QEMU qualification）

**Cycle Scope**

- Trigger: initial
- Acceptance gaps: None
- Repair items: None
- Inherited scope: change R1/R2/R4/R5/R6中属于driver substrate的场景；M36/D20、M41/D22、K09、M39、R53；既有MS05 buffer/descriptor/cookie守恒和EVENT_IDX。
- Excluded scope: axnet lifecycle/ticket/flush修改、kernel IRQ、socket registry、QEMU ioctl/probe/runtime、SMP、PCI runtime、DWMAC产品实现、性能。

**Objective**

建立一个不依赖无界自旋、不会在reset确认前释放DMA backing、不会把旧completion归给新owner的VirtIO-MMIO recovery底座。完成后上层queue owner可以只通过transport-neutral bounded step驱动恢复，不需要理解MMIO、VirtQueue token或adapter内部buffer布局。

**Background**

MS05把TX ownership固定为software ticket → `TxCookie` → adapter `TxSlot` → VirtQueue token，fatal drift后稳定fail-stop；MS06让唯一queue task和stack/socket readiness常驻。MS07要在进入SMP前加入reset和link flap，但当前transport只在初始化/Drop写status，adapter不可运行时重建，`TxCookie`也无epoch。VirtIO要求整设备reset写status=0并读回0后才可认为设备停止访问旧queue；当前未协商`VIRTIO_F_RING_RESET`，因此本Cycle只建立整设备reset基线。

用户已批准：整设备reset、旧socket终止/新epoch可用、pre-submit取消、device-owned只能quiesce/reset、link与reset分开验收、single-hart QEMU限定。本Cycle不提前实现上层语义，只提供其可依赖的driver事实。

**Current Baseline**

- Revision: `9d58bd422577959f84fc5e5a59db5a94bd7eb7fc`，branch `net-k3`。
- 工作树在Plan开始前已有已暂存的SNAPSHOT/runbook/M/D/K/R/I维护改动；这些不属于本Cycle，Act必须保留。
- 本轮Plan只新增`openspec/changes/ms07-qemu-single-hart-recovery-semantics/`，没有产品代码修改。
- 2026-08-28新鲜baseline：
  - `axdriver_net`：7 passed，exit 0。
  - `virtio-drivers --features alloc`：36 passed，exit 0；仅既有PCI lifetime warning。
  - `axdriver_virtio --features net`：16 passed，exit 0；仅上游同一warning。
  - 邻接axnet ordinary 371 passed、qemu-diagnostics 393 passed，均exit 0。
  - `make host-test`完成early-console 6、memtrack 8、MS03 33、MS04 Rust 16、MS04 C 10及non-socket self-test后，sandbox在UDP socket创建处返回`EPERM`，exit 2；无编译或产品断言失败。该环境事实不属于本Cycle GREEN。

**Current-State Evidence**

1. `crates/virtio-drivers/src/transport/mod.rs::Transport`已有`get_status/set_status/queue_set/queue_unset/config_space`；`begin_init`直接写empty→ACKNOWLEDGE/DRIVER，`finish_init`设置DRIVER_OK，没有运行时reset progress或config generation contract。
2. `transport/mmio.rs::MmioTransport::queue_unset`在modern路径写`queue_ready=0`后`while read != 0 {}`；其Drop只写status empty，不确认读回。该路径不能作为有deadline的recovery proof。
3. MMIO `VirtIOHeader`和PCI `CommonCfg`都包含`config_generation`，但trait未暴露；net `Config.status`只在`VirtIONetRaw::new`日志读取一次。
4. `device/net/dev_raw.rs::VirtIONetRaw`私有拥有transport、recv_queue和send_queue；`new`完成feature negotiation、两queue创建和DRIVER_OK；Drop调用两次queue_unset。没有move-safe recovery holder或分步reinit。
5. `queue.rs::VirtQueue`拥有DMA queue backing、free descriptor chain和used index；`pop_used`按exact token回收。旧queue只有在设备确认停止访问后才可Drop/复用。
6. `axdriver_net::TxCookie(u64)`刻意与transport token解耦但没有epoch；`NetTxQueue`定义accept前后buffer owner，`NetQueueControl`只负责completion/notification，`NetDriverOps`没有recovery accessor。
7. `axdriver_virtio::VirtIoNetDev`持有`rx_buffers[QS]`、`tx_slots[QS]`、free buffers、fault quarantine和inner raw device。submit成功把`TxSlot::Queue(buffer,cookie)`绑定到transport token；reclaim只有exact occupied token才能返回cookie。post-accept drift永久`tx_fault`并保留buffer。
8. adapter test fake transport/device已能写used ring、forge returned token和注入completion failure，是扩展reset/status/config injection的直接fixture；不需要破坏QEMU ring。
9. 官方VirtIO 1.3边界：写device status 0发起reset；driver读回0才确认完成；DEVICE_NEEDS_RESET时不得假定in-flight请求结果；确认停止前不得释放queue资源。本Cycle不得以Drop写0或queue_unset写ready=0替代整设备确认。

**Relevant Code**

- `crates/virtio-drivers/src/transport/mod.rs::{Transport,DeviceStatus}`：公共transport寄存器/初始化contract。
- `crates/virtio-drivers/src/transport/mmio.rs::{VirtIOHeader,MmioTransport}`：当前QEMU MMIO status、config_generation、queue register实现。
- `crates/virtio-drivers/src/transport/pci.rs::{PciTransport,CommonCfg}`：保持编译兼容的第二transport；本Cycle不声明runtime验证。
- `crates/virtio-drivers/src/device/net/{mod.rs,dev_raw.rs}::{Config,Status,VirtIONetRaw}`：net status和raw queue owner。
- `crates/virtio-drivers/src/queue.rs::VirtQueue`：descriptor/DMA backing和used-token reclaim。
- `crates/axdriver_net/src/lib.rs::{TxCookie,NetTxQueue,NetQueueControl,NetDriverOps}`：transport-neutral owner边界。
- `crates/axdriver_virtio/src/net.rs::{VirtIoNetDev,TxSlot,enter_tx_fault,submit_tx,reclaim_tx}`：adapter buffer/cookie/token ledger和fake fixtures。

**Critical Path**

```text
future axnet queue owner
  -> AxNetDevice / NetDriverOps::recovery_control()
  -> NetRecoveryControl::begin_recovery / poll_recovery_step
  -> VirtIoNetDev recovery holder
  -> VirtIONetRaw / Transport one-shot status + config operations
  -> status readback == 0
  -> only now close old TxSlot/RX owners and rebuild VirtQueues
  -> publish Recovered(new QueueEpoch) or retain Faulted quarantine
```

TX normal identity保持：`(QueueEpoch,ticket)` → `TxCookie` → adapter token slot。transport token不向上返回；completion先验证token slot，再返回完整cookie，上层后续Cycle再验证ticket。

Link read保持另一条路径：config event（后续Cycle）→ recovery control单次`generation-before/status/generation-after` snapshot；generation不一致只返回retry，不改变QueueEpoch或descriptor ledger。

**Implementation Guidance**

1. 先用fake MMIO/transport写RED，明确reset start与complete readback是两个步骤，config snapshot可在generation变化时返回retry。
2. 扩展Transport时保持现有implementor可编译；适合的default只能表达Unsupported/无generation，不能伪造成功。运行时recovery不得调用会无界自旋的queue_unset或依赖Drop。
3. 在axdriver_net定义typed queue epoch/cookie/recovery progress。字段名可调整，但epoch/ticket必须分别可检查且counter exhaustion不可wrap。
4. adapter内部使用显式holder/enum/Option等move-safe结构保留transport、old queues和all backing。局部表示由Act决定，但reset failure测试必须证明资源仍有唯一Rust owner且未进入free set。
5. status=0后remaining old TX的语义是reset-aborted，不是completion；本Cycle至少向future recovery contract报告owner summary，具体ticket outcome由Iteration 001完成。
6. reinit按现有`new`顺序复用feature negotiation、queue create、RX full refill和DRIVER_OK，不复制第二套不一致初始化流程；可抽取共享helper。
7. link snapshot一次最多读固定次数；generation不一致返回Pending/Again让future task重试，不能循环直到稳定。

**Behavioral Change**

- Transport从“构造/Drop时隐式reset”扩展为“调用者可显式发起并逐步确认的bounded reset primitive”。
- Driver contract从裸cookie和queue notification扩展为独立recovery control；普通driver不支持时稳定Unsupported。
- VirtIO adapter从永久active/fault对象扩展为可隔离old epoch、成功重建或失败quarantine的owner；正常submit/reclaim仍保持既有contract。
- link status从初始化日志事实变为可由task context取得的一致snapshot；本Cycle不改变ISR或socket行为。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| 1.1 | R5 reset success/failure；R6 config snapshot | `transport::{Transport,mmio,pci}` | init/status/queue寄存器 | bounded reset start/readback与config generation primitive |
| 1.1 | R5 backing boundary | `dev_raw::VirtIONetRaw`、`queue::VirtQueue` | raw queue owner/Drop | 提供运行时重建所需move-safe primitive，不在未确认时Drop |
| 1.2 | R1/R2/R4 typed contract | `axdriver_net::{TxCookie,NetDriverOps}` | transport-neutral queue owner接口 | QueueEpoch、epoch cookie、recovery stage/progress/ledger/accessor |
| 1.3 | R2 stale/duplicate | `axdriver_virtio::VirtIoNetDev::{submit_tx,reclaim_tx}` | token-buffer-cookie ledger | epoch match、old completion隔离、checked conservation |
| 1.3 | R5 reset/reinit | `VirtIoNetDev` recovery holder | static active/fault adapter | step state、status=0 boundary、rebuild或quarantine |
| 1.3 | R6 link snapshot | raw/adapter net config accessor | init-only status read | generation-consistent bounded status observation |

**Task Contracts**

### 1.1：Bounded transport reset/config primitives

- Requirement/Scenario: R5 reset成功/未确认；R6一致link snapshot。
- Depends on: None。
- Targets: `crates/virtio-drivers/src/transport/{mod.rs,mmio.rs,pci.rs}`、`device/net/{mod.rs,dev_raw.rs}`、`queue.rs`和tests。
- Current behavior: `begin_init`可写status=0；MMIO Drop只写0，modern queue_unset无界自旋；config generation未进入trait，net status只初始化读取。
- Required behavior: reset start和status=0 readback分离；每次调用只有固定数量volatile access；一致config snapshot遇generation变化返回可重试结果；运行时reset失败不触发old queue/backing Drop。
- Required changes: 添加transport-level one-shot primitives及fake可控status/config seam；为raw net queue提供move-safe runtime recovery支撑；保留PCI实现编译，允许未验证路径返回Unsupported但不可假成功。
- Preserve: 现有初始化、EVENT_IDX、queue layout、PCI编译和普通Drop语义；不新增executor依赖。
- Forbidden: 单次poll自旋到设备响应；暴露MMIO header给axnet；以queue_ready归零替代device status归零；声明PCI runtime已验证。
- Test witness: 先新增fake RED，分别卡在缺reset pending/readback和config generation retry；baseline命令当前36/36。
- GREEN condition: model逐步观察pending→complete、generation mismatch→retry；reset未确认的drop/free counters保持0；既有36项及新增项全绿。
- Verification: `env CARGO_TARGET_DIR=/tmp/ms07-act-virtio cargo test --manifest-path crates/virtio-drivers/Cargo.toml --locked --offline --lib --features alloc` exit 0；source check确认recovery path无unbounded wait。
- Stop when: MMIO无法在保留旧queue owner时读写status，或实现需要改变规范确认边界，填写Blocker Handoff返回Plan。

### 1.2：Transport-neutral recovery contract and epoch cookie

- Requirement/Scenario: R1 bounded driver step；R2正常/stale/overflow identity；R4 stage identity。
- Depends on: 1.1提供的事实；contract RED可先写。
- Targets: `crates/axdriver_net/src/lib.rs`及tests。
- Current behavior: `TxCookie(u64)`无epoch；NetQueueControl只负责通知；DevError无stage；NetDriverOps没有recovery accessor。
- Required behavior: typed checked QueueEpoch、可分离epoch/ticket的TxCookie、RecoveryStage/Progress/OwnerSummary和NetRecoveryControl；default accessor为None/Unsupported。
- Required changes: 设计不泄漏transport的最小公共数据类型；扩展现有DWMAC/legacy model fixture验证default兼容、round-trip、overflow和bounded step。
- Preserve: transport token私有、legacy driver source兼容的default accessor、NetQueueControl原职责、submit accept前后buffer语义。
- Forbidden: patch外部Cargo registry；把MMIO/descriptor类型放入公共contract；silent wrapping；用DevError单值替代stage/owner摘要。
- Test witness: API/model RED覆盖missing recovery accessor、epoch cookie round-trip和counter exhaustion；baseline 7/7。
- GREEN condition: legacy/DWMAC model不实现reset仍编译，typed模型通过，现有7项不退化。
- Verification: `env CARGO_TARGET_DIR=/tmp/ms07-act-axdriver-net cargo test --manifest-path crates/axdriver_net/Cargo.toml --locked --offline --lib` exit 0。
- Stop when: contract必须泄漏transport token才能证明owner，或需要修改外部DevError，返回Plan。

### 1.3：VirtIO adapter recovery transaction

- Requirement/Scenario: R2正常/stale/duplicate；R5整设备reset、NEEDS_RESET、IRQ交错边界；R6 link snapshot。
- Depends on: 1.1、1.2。
- Targets: `crates/axdriver_virtio/src/net.rs`、`crates/virtio-drivers/src/device/net/dev_raw.rs`、`queue.rs`及fake device/transport tests。
- Current behavior: inner不可运行时重建；RX全部device-owned，TX slot/fault ledger稳定但永久；forced token/completion seam已存在。
- Required behavior: holder在status=0前保留完整旧对象；成功后关闭old owners并rebuild/refill；失败后全部资源计入quarantine；completion只匹配同epoch cookie；link status按一致snapshot返回。
- Required changes: 实现NetRecoveryControl、adapter step状态和reinit共享helper；扩展fake transport控制status、generation、reinit failure和资源drop counters；保持每step bounded。
- Preserve: pre-accept Again归还buffer、post-accept invariant稳定fault、真实buffer/descriptor ledger、固定QS、正常completion exact token。
- Forbidden: reset未确认即drop/reuse old queue/buffer；把ResetAborted计为completion/reclaimed；unknown/duplicate token自动reset；旧cookie命中新slot。
- Test witness: 在真实adapter fake transport先写RED：delayed-zero、never-zero、reinit failure、old-cookie-after-new-epoch、duplicate、link-generation-race和conservation。
- GREEN condition: 成功路径新epoch资源完整；失败路径old backing唯一quarantine；stale不改新ledger；全量net suite通过。
- Verification: `env CARGO_TARGET_DIR=/tmp/ms07-act-axdriver-virtio cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --locked --offline --lib --features net` exit 0，并复跑1.1、1.2命令。
- Stop when: Rust所有权布局迫使reset failure提前Drop backing，或reinit无法复用现有init contract；不得用未证明unsafe规避，返回Plan。

**Invariants**

- 任一buffer、descriptor、cookie和queue backing在任一时刻只有一个声明owner。
- device status未读回0前，old queue/backing既不Drop也不进入free/reinit集合。
- completion必须先由transport exact token验证，再由adapter epoch/cookie验证；stale/duplicate不产生成功副作用。
- queue/link snapshot API不泄漏VirtIO实现，其他driver可稳定Unsupported。
- 每次recovery/config step有固定工作上限，无busy wait、sleep或guard跨Pending。
- 不修改MS05正常submit/reclaim、EVENT_IDX、queue budget或MS06 socket/runner行为。
- 当前Cycle不把fake/model结论扩大为QEMU、PCI、DWMAC、真板或SMP资格。

**Non-goals**

- axnet TicketTracker outcome、flush error、queue task状态与deadline。
- config-change IRQ publish、link-down enqueue policy、SocketEpoch/NetworkTerminal。
- QEMU control ABI、probe、validator、手工runtime或全项目closeout。
- queue reset feature、PCI runtime、其他NIC恢复、真板DMA stop和性能。

**Acceptance**

- A1（R5/R6，Task 1.1）：transport test证明reset start/readback和一致config snapshot都是bounded step；never-zero不释放old backing；recovery source无无界wait。
- A2（R1/R2/R4，Task 1.2）：公共contract表达checked epoch、cookie、stage/progress/owner摘要，legacy driver默认Unsupported且transport token不泄漏。
- A3（R2/R5，Task 1.3）：adapter成功reset/reinit得到新epoch和完整RX/TX capacity；reset未确认/重建失败保持唯一quarantine；old/duplicate completion不改新ledger。
- A4（R6，Task 1.3）：config generation变化被检测并retry，稳定snapshot返回link状态且不改变descriptor/QueueEpoch。
- A5（兼容）：三个crate全量tests无产品失败；现有warning不升级，git diff仅包含本Cycle相关产品/测试与既有用户改动。

**Requirements Traceability Matrix**

| Requirement | Scenario | Design | Task | Iteration | Code Surface | Test Witness | Simplification | Status |
|---|---|---|---|---|---|---|---|---|
| R1 | bounded same-owner recovery step | D2 | 1.2、1.3 | 000 | `NetRecoveryControl`、adapter step | contract/adapter progress model | None | Covered |
| R2 | current、stale、duplicate、overflow | D1、D3 | 1.2、1.3 | 000 | `QueueEpoch/TxCookie/TxSlot` | old-cookie、duplicate、checked overflow | None | Covered |
| R4 | stage/progress/owner diagnosis | D2、D5 | 1.2、1.3 | 000 | recovery typed contract | stage/result model assertions | None | Covered |
| R5 | reset success、never-zero、NEEDS_RESET boundary | D3 | 1.1、1.3 | 000 | Transport、Raw、VirtIoNetDev | delayed/never-zero、reinit failure、drop counter | None | Covered |
| R6 | consistent link snapshot | D6 | 1.1、1.3 | 000 | config generation/status accessor | generation race/retry | None | Covered |

**Verification**

按依赖顺序执行并把每项不超过20行的决定性输出、exit code和支持Acceptance写入Act Response：

1. Task focused RED→GREEN commands，名称由新增test确定。
2. `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --locked --offline --lib --features alloc`。
3. `cargo test --manifest-path crates/axdriver_net/Cargo.toml --locked --offline --lib`。
4. `cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --locked --offline --lib --features net`。
5. 邻接回归：axnet ordinary和`--features qemu-diagnostics`全量；若host linker要求，使用已知`-C linker=/tmp/opencode/cc-nopie.sh`或等价non-PIE wrapper并记录。
6. `cargo fmt --all -- --check`或仅对workspace受影响crate执行项目现有等价format check；任何实际format修改必须只限本Cycle文件。
7. `git diff --check`和完整diff review，确认无registry修改、无无界recovery wait、无unrelated change。

本Iteration不要求kernel build或QEMU；它们不能补偿host owner tests失败。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | Explorer调用链及本Cycle Current-State Evidence已定位transport、raw queue、adapter ledger、fake fixtures和调用边；新鲜baseline可执行。 |
| Design | PASS | D1–D8闭合epoch、bounded step、status=0、failure quarantine、link snapshot和错误责任；Open Questions无实质项。 |
| Iteration Plan | PASS | tasks.md四轮Map覆盖1.1–4.2并完成聚合/拆分审计；本轮形成独立driver substrate。 |
| Cycle Scope | PASS | 仅1.1–1.3，无axnet/socket/QEMU范围混入；Acceptance与stable baseline一致。 |
| Task Contracts | PASS | 每项含targets、current/required、preserve/forbidden、RED/GREEN、verification和stop boundary；Act只读本Cycle即可执行。 |
| Traceability | PASS | scoped RTM无Missing/Simplified；change-level RTM覆盖R1–R8。 |
| Verification | PASS | 三crate新鲜全量baseline均exit 0；fake fixture和命令已定位；环境EPERM与产品Gate分层。 |
| User plan approval | BLOCKED | Gate 1需求已批准，但完整design/tasks/Cycle尚待用户审计；批准前保持draft且不得交给Act。 |

**Persisted Evidence**

- Mode: none

Act Response足以保存可重复的host命令、每项不超过20行的决定性输出、exit code、修改文件/符号和diff review。无一次性环境、无法重跑数据或Incident现场；本Cycle不得创建`evidence/`占位目录。

- Budget: 本Cycle最多5个文件（含README），整个change最多20个Evidence文件；本CycleMode为none，不消费预算。

**Risks and Notes**

- 用户工作树已有暂存文档改动；Act必须在开始和结束记录status并避免覆盖。
- 当前baseline的Cargo warning与本Cycle无关；新增warning需要修复，但不要求清理既有PCI lifetime warning。
- fake transport必须能证明reset未确认时Rust Drop/allocator行为，而不只断言逻辑flag。
- `or equivalent`仅允许局部类型/holder实现，不允许改变status=0确认、epoch identity、error progress或quarantine语义。
- 本Cycle完成也不授权axnet接入或QEMU运行；Plan Review accepted后才展开Iteration 001。

## Act Response

- Status: pending

**Implemented**

None

**Changed Files and Symbols**

None

**Deviations from Plan**

None

**Blocker Handoff**

None

**Blocker Resolution**

None

**Self-Review**

- Plan compliance: BLOCKED
- Full diff reviewed: BLOCKED
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 0

Pending execution.

**Verification Evidence**

Pending execution.

**Persisted Evidence**

None required.

**Experience Candidates**

None

**Remaining Issues**

Pending execution.

**Commit or Diff Reference**

None

## Plan Review

- Review Result: pending

**Findings**

Pending Act Response.

**Deviation Classification**

None

**Acceptance Gaps**

Pending execution of A1–A5.

**Convergence**

N/A

**Evidence**

Plan-only; no Act evidence yet.

**Follow-up Decision**

Await user audit and Gate 2 approval. Do not execute while Plan Context is draft.

**Iteration Plan Update**

None

**Next Cycle**

None

**Next Iteration**

None
