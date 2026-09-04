# Iteration 007 / Cycle 002: Align Initial Link and VirtIO Owner Qualification

## Plan Context

- Status: ready
- Iteration: 007-single-hart-qemu-qualification
- Cycle: 002-replan
- Cycle Type: replan
- Parent cycle: `001-replan.md`

**Iteration Scope**

- Change tasks: 4.2
- Depends on: Iteration 006 accepted；Cycle 001已完成身份层清理与自动Gate基线
- Stable baseline: 精简后的行为协议以已提交的初始link state和真实VirtIO双向owner分类，在single-hart
  QEMU VirtIO-MMIO上证明reset、queue stall、link flap、old/new socket及受影响回归。
- Verification boundary: host/model先证明首次link commit和健康owner tuple；自动Gate全绿后，用户手工
  QEMU的raw serial由validator判定，并明确给出MS01/MS04/MS05/MS06终态与exit。
- Diagnostic boundary: queue owner首次link读取、VirtIO RX/TX owner分类、guest peer socket setup、
  QEMU user-net、runtime marker和四组回归。
- Deferred tasks: None

**Cycle Scope**

- Trigger: replan-required
- Acceptance gaps: Cycle 001的owner判据与driver contract冲突；Active V4在首次CONFIG IRQ前link unknown；
  UDP connect归因没有分阶段errno或可审计runtime log；A2–A6及四组回归未满足。
- Repair items: None
- Inherited scope: Task 4.2、R6、R8、D6、D8；Iteration 006接受的V4 ABI、恢复状态机、epoch/socket
  terminal、deadline、ordered marker和纯输出validator；Cycle 001已完成的identity-layer清理。
- Excluded scope: 新V4版本或wire字段、修改VirtIO owner分类迎合fixture、自动驱动QEMU/HMP、SMP、
  PCI/DWMAC、真板、性能、全局文档维护和无运行证据的UDP产品改动。

**Objective**

让Task 4.2的host与runtime资格使用实际产品语义：唯一queue owner启动后提交首个一致link snapshot；
probe和validator把常驻RX owners视为健康VirtIO基线；guest peer socket setup能指出准确失败层。自动Gate
通过后停在用户手工QEMU边界，只有完整MS07与四组回归结果满足判据才报告本Cycle完成。

**Background**

Cycle 001清除了revision、hash、run-id和peer-host pin，但手工根因报告建立在两个错误前提上。
`OwnerSummary.available`统计空闲TX buffers，`device_owned`统计已提交RX buffers和TX slots；初始化填充
`QS`个RX owners与`QS`个TX buffers，所以健康空闲态的64/64不是超配。另一个真实缺口是
`Service.link_state`从`None`开始，而owner只在CONFIG cause到达时读取link；启动没有CONFIG cause，
因此pre-reset V4不能取得已提交的up状态。现有Evidence只记录启动至shell，没有probe或DBG输出，
也不能证明`connect()`失败。

**Current Baseline**

- Branch `net-k3`，HEAD `05528313c413535ff7ba912867d08d7d9c3e392e`；工作树包含前序Iteration与
  Cycle 001的staged产品、测试和OpenSpec改动，本Cycle必须保留这些改动。
- `VirtIoNetDev::try_new()`建立`2 * QS` packet pool并由`refill_all()`填满RX和TX两侧；
  `committed_owner_count()`把已提交RX与TX owners相加。
- `NetRecoveryControl::owner_summary()`健康态返回空闲TX数量、已提交RX+TX数量及driver-held quarantine；
  它不是单队列occupancy，也没有`available + device_owned <= QS`契约。
- `Service::new()`设置`link_state=None`、`link_generation=0`；`link_policy_step_target()`是唯一提交
  link state/generation和link gate的路径。
- `RxRxFuture::poll_first()`激活双向slot mode并发布Active，但不安排link读取；`poll_active()`只有在
  `QueueEvent.cause_config`为true时执行一次link micro-step。
- `open_peer_socket()`把`connect()`和两个`fcntl()`放在同一个条件中，失败输出无法区分阶段。
- Cycle 001的`qemu-serial-dbg.log`仅证明QEMU成功启动到shell并退出；无MS07 probe marker或errno。
- 新鲜基线：axdriver_virtio owner focused tests 2/2 PASS；C probe decision test、MS07 validator self-test、
  peer self-test均exit 0。它们稳定复现当前错误fixture，不构成runtime PASS。

**Current-State Evidence**

1. `crates/axdriver_virtio/src/net.rs`：`free_tx_bufs`容量为`QS`，`rx_buffers[QS]`在健康态已提交；
   `committed_owner_count()`统计非Free TX slot与非空RX slot。`owner_summary()`在Active时返回
   `{ available: free_tx_bufs.len(), device_owned: committed_owner_count(), quarantined: held }`。
2. `crates/axdriver_net/src/lib.rs::OwnerSummary`只规定三类资源归属，没有规定三字段共享一个`QS`上限。
3. `tests/ms07_recovery_probe.c::ms07_drained_epoch_ok/wait_for_pre_reset/wait_for_drained_active`
   和`scripts/ms07-qemu-validate.py::_v4/_validate_protocol`都要求`device_owned==0`；canonical fixtures也
   固定为0，与生产VirtIO健康态冲突。
4. `crates/axnet/src/async_rx.rs::RxRxFuture::poll_first/poll_active`与
   `crates/axnet/src/service.rs::link_policy_step_target`构成首次link路径。已有fake recovery control提供
   `link_reads`、`link_again`、`link`和`link_hold` seam，可直接建立RED/GREEN。
5. `kernel/src/syscall/net/socket.rs::sys_socket`支持在socket type中携带`O_NONBLOCK`；
   `sys_connect`把UDP `WouldBlock`映射为`EINPROGRESS`，普通UDP connect只做implicit bind、source选择与
   peer commit。现有probe没有记录具体失败的是socket、connect、F_GETFL还是F_SETFL。

**Relevant Code**

- `crates/axnet/src/async_rx.rs`：唯一queue owner启动/Active loop、QueueEvent与host lifecycle fixtures。
- `crates/axnet/src/service.rs`：link snapshot提交、LinkGeneration/SocketEpoch与link gate。
- `crates/axdriver_virtio/src/net.rs`：生产VirtIO RX/TX owner分类和固定capacity基线；只读依据，不计划修改。
- `tests/ms07_recovery_probe.c`、`tests/ms07_recovery_probe_test.c`：guest状态等待、peer socket与C决策见证。
- `scripts/ms07-qemu-validate.py`：纯输出协议和negative fixtures。
- `scripts/ms07-recovery-peer.py`：有界三阶段UDP echo；保持Cycle 001简化后的phase/seq协议。
- `Makefile`、`tests/ms07-recovery-host-harness.rs`：自动Gate、schema/source seam与RISC-V payload build。

**Critical Path**

```text
kernel start_rx_task
  -> RxRxFuture::poll_first
  -> Service::activate_target
  -> task-context initial link micro-step
  -> Service::link_policy_step_target
  -> driver read_link_status(config generation guard)
  -> link_state/link_generation commit
  -> Active service + V4 recovery_snapshot_v4

guest probe
  -> nonblocking UDP socket creation
  -> connect(10.0.2.2:15572)
  -> stable V4 {up, available=QS, device_owned=QS, quarantined=0}
  -> peer exchange -> reset -> old/new socket checks -> HMP down/up
  -> ordered transcript -> pure validator
```

初始link读取和硬件CONFIG IRQ共用同一Service提交路径；初始化不调用ISR publisher，也不增加硬件IRQ
telemetry。owner snapshot始终来自同一Service guard下的driver control，不复制或重算driver ledger。

**Implementation Guidance**

1. 先修正C/Python owner fixtures，使健康idle/current tuple以pre-reset观测到的固定capacity为基线；
   每个成功marker等待并比较`available`、`device_owned`和`quarantined`，不修改driver。
2. 在owner首次成功激活后安排一次task-context link micro-step。`Again`每poll最多读取一次并保留工作；
   `Unsupported`不阻塞非MS07 driver；up/down使用既有gate、epoch和guard外wake语义。
3. guest socket直接以`SOCK_DGRAM | O_NONBLOCK`创建，分别检查并打印socket与connect阶段errno；不要新增
   run-id、地址pin或专用审计协议。没有实际errno时不修改axnet/kernel UDP产品路径。
4. 完成host/model/build Gate后才交接手工QEMU。任何runtime失败保留首个产品/环境层并停止，不把启动、
   wget或peer静默当作MS07通过。

**Behavioral Change**

- 支持link snapshot的设备在async owner激活后最终具有已提交up/down状态；首次读竞争不会忙等或丢失。
- V4 wire不变，但其健康VirtIO解释从“无DeviceOwned”修正为“RX常驻owners + 无quarantine + TX已回到
  baseline capacity”。reset/link资格比较同阶段tuple，而不是错误的单`QS`总和。
- peer socket在创建时即nonblocking；失败输出标明`socket`或`connect`阶段及errno。该变化不修改UDP
  syscall、axnet socket语义或peer协议。
- 手工QEMU步骤和六case顺序保持不变；只有owner数值与初始LinkGeneration基线按真实产品语义更新。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| T4.2-P1 | R8/健康VirtIO双向owner | probe、probe test、validator | 以`device_owned==0`判健康 | 以pre-reset固定capacity和常驻RX owner关系判健康/守恒 |
| T4.2-P2 | R6/初始link snapshot | `RxRxFuture::poll_first/poll_active`、既有link fixtures | 只消费硬件CONFIG cause | 激活后安排一次task-context初始读取，`Again`有界保留 |
| T4.2-P3 | R8/peer失败分层 | `open_peer_socket`、C test/source seam | 合并connect/fcntl失败 | 创建即nonblocking，socket/connect分阶段errno |
| T4.2-P4 | R8/QEMU与兼容回归 | Makefile、payload、Runbook命令、用户输出 | 自动Gate绿，runtime无有效Evidence | 重跑自动Gate并交接/审计MS07与四组runtime |

**Task Contracts**

### T4.2-P1: Align probe and validator with the VirtIO owner contract

- Requirement/Scenario: R8健康VirtIO双向owner、reset前后资源守恒、link flap不改变queue ownership。
- Depends on: None。
- Targets: `tests/ms07_recovery_probe.c::{ms07_drained_epoch_ok,wait_for_pre_reset,
  wait_for_reset,wait_for_drained_active,wait_for_link_down,wait_for_link_up}`、
  `tests/ms07_recovery_probe_test.c`、`scripts/ms07-qemu-validate.py::{_v4,_validate_protocol,canonical}`。
- Current behavior: 所有成功V4要求`device_owned==0`；canonical把健康tuple写成64/0/0；Act据此把
  64/64/0误判为owner drift。
- Required behavior: pre-reset稳定tuple必须为Active、link up、quarantine 0，且在当前固定VirtIO模型
  中`available==device_owned==QS`；后续成功tuple在无流量采样点恢复同一available/device-owned基线。
  reset推进Q/S但不改变link generation；link down/up只按既有规则推进L/S且保持owner baseline。
- Required changes: 删除所有健康态`device_owned==0`和三字段单`QS`总和假设；C/Python canonical与negative
  fixtures增加`device_owned=64`合法样本，以及63/64、64/63、quarantine非零和跨阶段drift拒绝样本。
- Preserve: V4 layout/field名、fault tuple独立一致性、六case顺序、deadline、terminal、fatal和exit判据。
- Forbidden: 修改`VirtIoNetDev::owner_summary()`迎合fixture；把TX/RX owner混成新字段；降低quarantine、
  epoch、link或terminal检查。
- Test witness: 先新增健康64/64应通过、64/0应拒绝的C/Python fixtures并观察当前实现RED；
  `axdriver_virtio owner_summary` focused tests在修改前保持GREEN，证明产品contract基线。
- GREEN condition: C/Python fixtures接受64/64/0并拒绝错误分类；生产driver focused tests不变且通过；
  probe/validator case/schema一致。
- Verification: C decision test、validator self-test、probe/validator schema diff、axdriver_virtio
  owner focused/full net tests全部exit 0。
- Stop when: driver在同一健康idle阶段不能给出稳定RX/TX owner关系，或必须修改V4 ABI才能区分；返回Plan。

### T4.2-P2: Commit the initial link snapshot in the resident owner

- Requirement/Scenario: R6初始link snapshot、config generation retry、down/up与combined cause。
- Depends on: P1可并行建立测试，但进入runtime前两者都必须GREEN。
- Targets: `crates/axnet/src/async_rx.rs::RxRxFuture::{poll_first,poll_active}`及既有QueueEvent/link fixtures；
  必要时`Service`增加不改变公共接口的局部observer/helper。
- Current behavior: activation直接发布Active；没有CONFIG cause时`link_policy_step_target()`从不运行，
  V4 link永久unknown。
- Required behavior: 成功激活的唯一owner在task context安排首个一致link读取。单poll最多读一次；`Again`
  保留工作并self-wake/后续重试；成功提交up/down与LinkGeneration后不重复推进。硬件CONFIG cause仍独立，
  且事件窗口、SocketEpoch、link gate和guard外wake保持既有语义。
- Required changes: 用现有owner状态或最小布尔pending状态表达初始化工作；首次成功读取通过
  `link_policy_step_target()`提交，不能直接写`link_state`。若初始状态为down，必须执行既有down gate与
  terminal提交；若`Unsupported`，普通非MS07设备仍可Active且V4保持unknown。
- Preserve: 唯一spawn、ISR只ack/publish、register-recheck、无10ms polling fallback、每poll bounded、
  QueueEpoch不因link变化推进、V1–V4 layout不变。
- Forbidden: 在ISR或snapshot ioctl内读取config；伪增硬件config IRQ telemetry；busy loop直到snapshot
  成功；为初始化spawn第二task；把unknown直接伪造成up。
- Test witness: 新owner-level测试在不publish CONFIG cause时先观察当前实现Active但`link_reads=0`/unknown
  的RED；增加initial up/down、一次Again后成功、成功后不重复、Unsupported不阻塞与no fabricated ISR
  telemetry见证。
- GREEN condition: initial up/down各只提交一次；Again每poll一次且最终提交；后续真实config cause仍
  down/up各推进一次；所有既有link/lifecycle/event-window tests通过。
- Verification: focused initial-link与现有Task 3.1 tests，随后axnet ordinary与qemu-diagnostics串行全量，
  均`--test-threads=1`、exit 0；kernel RISC-V build通过。
- Stop when: 首次读取必须跨Service guard等待设备、需要第二owner或会破坏Unsupported driver启动；返回Plan。

### T4.2-P3: Make peer socket setup atomic and failure-specific

- Requirement/Scenario: R8 reset前流量、guest peer失败分层、无证据不归因。
- Depends on: P1。
- Targets: `tests/ms07_recovery_probe.c::open_peer_socket`、`tests/ms07_recovery_probe_test.c`及RISC-V payload。
- Current behavior: blocking socket创建后把connect/F_GETFL/F_SETFL合并；单一errno不能定位失败调用。
- Required behavior: 使用kernel已支持的`SOCK_DGRAM | O_NONBLOCK`创建socket，分别处理socket与connect；
  失败输出`DBG: peer_socket stage=<socket|connect> errno=<n>`并关闭已创建fd。成功路径不增加协议marker。
- Required changes: 删除peer setup中的fcntl链；保留`10.0.2.2:15572`、AF_INET和guest-client模型。
- Preserve: phase/seq payload、absolute deadline、nonblocking send/recv、peer无host pin、15572不hostfwd。
- Forbidden: 新增retry loop、sleep、run-id/address pin、hostfwd 15572、在没有分阶段errno时修改kernel/axnet UDP。
- Test witness: 修改前source witness显示合并条件和fcntl链；修改后C以`-Werror`编译，source guard确认
  O_NONBLOCK在socket创建处且peer setup无fcntl。真实成功/errno由手工QEMU见证。
- GREEN condition: host C tests/compile通过；runtime成功进入pre-reset peer exchange，或失败时给出唯一
  stage+errno并按stop condition返回Plan，不能伪记产品PASS。
- Verification: C decision test、host probe compile、RISC-V static payload build；手工raw serial检查。
- Stop when: socket/connect仍失败，或QEMU user-net/host peer不可达；保留stage+errno并返回Plan，不猜修复。

### T4.2-P4: Re-run automatic gates and manual single-hart qualification

- Requirement/Scenario: R8单hart QEMU reset/link flap、旧/新socket、兼容性回归。
- Depends on: P1–P3 GREEN，payload与kernel重新构建。
- Targets: Makefile host gates、RISC-V payload/kernel build、用户手工peer/QEMU/HMP/validator与
  MS01/MS04/MS05/MS06回归；不再修改产品语义。
- Current behavior: Cycle 001自动Gate通过；唯一persisted log只到shell，A2–A6没有runtime证据。
- Required behavior: 自动Gate先证明修订契约；Act给出完整手工命令并在用户能力边界停止。用户回传后，
  Act核对环境、六case、owner tuple、epoch/link/terminal、fatal与exit，再保存最小Evidence投影。
- Preserve: single hart、VirtIO-MMIO、QEMU 7.0.0、user-net、`LOG=warn`、手工HMP与R44边界。
- Forbidden: agent自动驱动QEMU/HMP；用boot/wget/host tests替代MS07 runtime；恢复hash/revision/run-id；
  缺marker、validator exit或四组回归终态仍判PASS。
- Test witness: P1–P3的RED/GREEN与清理后source guard；手工run是runtime witness。
- GREEN condition: validator exit 0；六case及MS01 14/14、MS04四mode、MS05六mode、MS06 12-case明确
  PASS；无panic/trap/fatal owner drift/永久Pending；命令和exit可审计。
- Verification: `make host-test`（仅精确socket EPERM可分层）、两套axnet全量、driver suites、RISC-V
  payload和kernel build、format/diff/OpenSpec strict；随后用户手工runtime。
- Stop when: 任一自动Gate失败；或用户runtime尚未完成、环境不符、validator/回归失败；写Blocker Handoff。

**Invariants**

- `OwnerSummary`保持transport-neutral；V4不新增字段，不把RX/TX owner解释塞进wire ABI。
- 唯一queue owner拥有link读取和恢复状态；ISR不读取config、descriptor、ledger或socket registry。
- 初始link读取与CONFIG cause均使用同一Service提交函数；`Again`不续期、不自旋、不丢事件。
- healthy owner资格证明“分类与capacity关系”，不是`device_owned==0`，也不是三个字段共用一个QS上限。
- 旧socket terminal在wake前提交；新epoch socket不继承旧terminal；link flap不推进QueueEpoch。
- 手工与自动证据不使用hash、revision、run-id、peer/host pin、manifest或时间顺序身份系统。

**Non-goals**

- 不修改VirtIO pool大小、RX refill、TX completion或owner分类。
- 不扩展到SMP、PCI/DWMAC、真板DMA/cache、性能、自动QEMU runner或连接透明迁移。
- 不凭Cycle 001文字修复UDP产品路径；只有P3取得具体stage+errno后才能返回Plan决定新范围。
- 不同步SNAPSHOT、全局tasks/R54，不创建Runbook/Incident，不提交Git commit。

**Requirements Traceability Matrix**

| Requirement | Scenario | Design | Task | Code Surface | Test Witness | Simplification | Status |
|---|---|---|---|---|---|---|---|
| R6 | 初始link、Again、down/up | D6 | P2 | owner poll + Service link step | initial up/down/Again/no-fake-IRQ | None | Covered |
| R8 | 健康VirtIO owner与守恒 | D8 | P1 | driver summary + C/Python consumers | 64/64 valid、错误tuple negative | None | Covered |
| R8 | peer setup失败分层 | D8 | P3 | guest socket/connect | compile/source witness + runtime errno | None | Covered |
| R8 | single-hart与四组回归 | D8 | P4 | payload/QEMU/validator/gates | raw serial、validator、终态/exit | None | Covered |

**Acceptance**

- A1：host/model证明唯一owner在无硬件CONFIG cause时提交初始link；Again有界重试；硬件IRQ telemetry
  不被伪增；既有down/up和event-window tests不退化。（R6/D6/P2）
- A2：C/Python资格接受生产VirtIO健康idle的`available=64, device_owned=64, quarantined=0`，拒绝
  RX owner缺失、TX capacity未恢复、quarantine和跨阶段drift；driver contract不被修改。（R8/D8/P1）
- A3：guest peer socket创建即nonblocking；失败时raw serial给出socket或connect阶段与errno；成功时三个
  peer phase按唯一顺序完成且共享absolute deadline。（R8/D8/P3）
- A4：single-hart QEMU V4从已提交link up开始；reset后QueueEpoch/SocketEpoch各推进一次，owner恢复
  baseline；HMP down/up不推进QueueEpoch并按规则推进LinkGeneration/SocketEpoch。（R6/R8/P1–P4）
- A5：旧socket分别稳定返回`ECONNRESET`、`ENOTCONN`，新epoch socket完成双向peer exchange；validator
  exit 0且无panic/trap/fatal/永久Pending。（R7/R8/P4）
- A6：MS01 14/14、MS04四mode、MS05六mode和MS06 12-case均有明确PASS与exit。（R8/P4）

**Verification**

1. P1 RED/GREEN：axdriver_virtio owner focused基线；C decision test；validator self-test；case/schema diff。
2. P2 RED/GREEN：initial up/down/Again/Unsupported/no-fake-IRQ focused tests；全部link/event/lifecycle tests。
3. P3：host C `-Werror` compile、peer setup source guard、RISC-V static payload build。
4. 自动回归：`make host-test`；axnet ordinary与qemu-diagnostics串行全量；virtio/driver focused/full；
   `make ARCH=riscv64 build`；format、diff check与`openspec validate ... --strict`。
5. 自动Gate全绿后，Act提供peer、HTTP、QEMU（无15572 hostfwd）、guest probe、HMP、validator和四组
   回归命令并改为blocked。用户回传结果后，Act只审计输出；全部Acceptance成立才改为reported。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Requirements | PASS | R6/R8补齐初始link、双向owner与失败归因；不改变MS07目标 |
| Investigation | PASS | 实际owner实现、owner调用链、首次link调用链、sys_socket/connect与唯一log已核对 |
| Design | PASS | 复用既有Service link提交；V4不变；probe以真实capacity关系取证 |
| Iteration Plan | PASS | 四项共同关闭Task 4.2 runtime资格，依赖有序且诊断边界可分层 |
| Cycle Scope | PASS | 产品改动限初始link；其余为错误fixture、guest setup与既有runtime Gate |
| Task Contracts | PASS | 每项包含targets、行为、preserve/forbidden、RED/GREEN、验证和停止条件 |
| Traceability | PASS | R6/R8到D6/D8、P1–P4、代码与见证均Covered |
| Verification | PASS | 从driver/owner unit到guest/QEMU/兼容回归逐层递增，无身份型证据工程 |
| Evidence | PASS | required仅保存难重跑手工runtime的最小协议投影与回归终态 |
| Authorization | PASS | 用户已审计并批准本计划（「更改gate状态，开始实施吧」），Plan Context 改为 `ready`，Act 开始执行 T4.2-P1–P4。 |

**Persisted Evidence**

- Mode: required
- Path: `openspec/changes/ms07-qemu-single-hart-recovery-semantics/evidence/007-single-hart-qemu-qualification/002-replan/`
- `README.md`：支持A3–A6；记录环境、完整命令、exit、用户侧raw serial来源及首个失败层。Act Response
  不足以保存手工session来源，session不能低成本重跑；缺失时不能判定runtime资格。
- `ms07-protocol.txt`：支持A3–A5；保存START至HARNESS_EXIT的协议投影和validator结果。marker顺序与
  owner/epoch关系不能仅靠摘要保留；缺失时不能接受MS07 runtime。
- `regressions.txt`：支持A6；保存MS01/MS04/MS05/MS06终态与exit。四组手工回归不能低成本重跑；
  缺失时不能接受兼容性Gate。
- 仅在用户结果到达后创建。完整raw serial保留用户侧，change内不复制长日志；失败且现场即将消失时
  可用上述预算保存决定性BLOCKED投影。
- Budget: 本Cycle最多5个文件（含README），整个change最多20个Evidence文件；单文件最多500行且
  不超过256 KiB。

**Risks and Notes**

- 初始snapshot从unknown提交为up会使pre-reset LinkGeneration从0变为1；validator只校验相对关系，
  不固定绝对起始值。
- 初始down会沿既有D6关闭当前SocketEpoch；这是设备实际状态，不得为启动方便伪造成up。
- P3之后若connect仍失败，具体errno可能揭示新的kernel/axnet或QEMU user-net问题；该证据触发stop
  condition，不能由Act扩张产品范围。
- 当前Cycle 001 Evidence目录中的启动log不迁移、不删除，也不作为Cycle 002资格证据。
- 轻量模式SKIPPED：跨axnet owner、driver contract消费者、guest probe和QEMU资格，且触及并发/
  ownership关键路径，不满足轻量条件。

## Act Response

- Status: blocked

**Implemented**

T4.2-P1、T4.2-P2、T4.2-P3 已按 `ready` Plan Context 实施并全量验证；T4.2-P4 的自动
host/model/build Gate 全绿，但真实单 hart QEMU runtime（R44 规定仅可由操作员手工驱动）尚未
运行，因 `required` Evidence 依赖用户手工 session 而停止于用户能力边界。

1. **P1：probe/validator 与 VirtIO owner contract 对齐。**
   - `tests/ms07_recovery_probe.c`：新增 `MS07_OWNER_SLOTS 64u`；`ms07_drained_epoch_ok`
     改为 `(obs, q, s, expected_available)`，健康判据由 `device_owned==0` 修正为
     `available==device_owned==expected` 且 `quarantine==0`；`wait_for_pre_reset` 等待
     `available==device_owned==MS07_OWNER_SLOTS && quarantine==0`（不再以 `device_owned==0`
     为 idle）；`wait_for_link_down/up` 与 `ms07_link_down_transition_valid` 同时在
     `available` 与 `device_owned` 上守恒（link flap 不拥有/释放任何 slot）。
   - `tests/ms07_recovery_probe_test.c`：`drain_fixture` 改为健康 64/64；`drained_epoch_ok`
     负样本覆盖 `available==0`、`device_owned==0`(RX 缺失)、`63/64`、`64/63`、quarantine 非零；
     新增 down 过渡在 available/device_owned 双通道上的漂移拒绝，并固定「reset snapshot
     (available=63) 不作 down baseline」。
   - `scripts/ms07-qemu-validate.py`：新增 `OWNER_BASELINE=64` 与 `_healthy_owner()`；
     `_v4` 由 `device_owned!=0` 改为健康基线判据；new-epoch 与 link 阶段同时守恒
     `available` 与 `device_owned`；canonical 与 self-test 增加 64/64 合法样本及
     `63/64`、`64/63` 非法样本。product driver `VirtIoNetDev::owner_summary()` 未改动，
     axdriver_virtio owner focused 7/7、full 36/36 保持 GREEN（P1 契约基线）。
2. **P2：resident owner 在激活时提交首个一致 link snapshot。**
   - `crates/axnet/src/async_rx.rs`：`RxRxFuture` 新增 `initial_link_pending` 字段；
     `poll_first` 在 `activate_target()` 成功后置位；`poll_active` 以
     `causes.config || self.initial_link_pending` 每 poll 至多一次读 link 快照，
     `Again` 只 re-publish CONFIG 自唤醒并保留 retry（不把 config-Again 误标为 initial），
     `Up/Down/NoEvent/Unsupported/Fault` 清除标志，`Down/Up` 发布 stack progress。
     初始 down 走既有 D6 关闭 SocketEpoch 并 hold link gate，`QueueEpoch` 永不推进。
     测试 fake 增加 `link_unsupported` one-shot；新增 5 个 owner 级 witness：初始 up 提交一次
     且不伪造 CONFIG cause、初始 down 提交并 gate、一次 Again 后有界重试至提交、成功后不再重复
     读取、Unsupported 不阻塞且保持 unknown。为修复 recovery/lifecycle 夹具因新初始读而误触发
     link-down gate，把各 `RecoveryDriverStats` factory 默认 `link=true`（健康 NIC 为 up），
     现有 link 专项测试均显式置位，不受影响。
3. **P3：guest peer socket 原子创建 + 分阶段 errno。**
   - `tests/ms07_recovery_probe.c::open_peer_socket`：以 `socket(AF_INET, SOCK_DGRAM |
     O_NONBLOCK, 0)` 创建（kernel 支持 socket type 携带 O_NONBLOCK），分别处理 socket 与
     connect 失败并打印 `DBG: peer_socket stage=<socket|connect> errno=<n>`，connect 失败
     关闭 fd；删除原 `connect && fcntl(F_SETFL)` 链。保留 `10.0.2.2:15572`、AF_INET、阶段/
     seq 载荷、absolute deadline、nonblocking send/recv、无 host pin、无 15572 hostfwd；
     未改动 kernel/axnet UDP 产品路径。
   - `Makefile host-test`：新增两条 source guard——peer socket 必须在 `socket()` 处带
     `O_NONBLOCK`，且 peer setup 不得使用 `fcntl(F_SETFL)`。
4. **P4（自动部分）：全部自动 Gate 全绿**，随后停止于手工 QEMU 边界。

**Changed Files and Symbols**

- `tests/ms07_recovery_probe.c`：`MS07_OWNER_SLOTS`；`ms07_drained_epoch_ok`（签名+判据）；
  `ms07_link_down_transition_valid`（device_owned 守恒）；`wait_for_pre_reset`；
  `wait_for_link_up`（device_owned 守恒）；`wait_for_drained_active`；`open_peer_socket`
  （O_NONBLOCK + stage errno）。
- `tests/ms07_recovery_probe_test.c`：`drain_fixture`、down 双通道守恒、drained 健康/负样本。
- `scripts/ms07-qemu-validate.py`：`OWNER_BASELINE`、`_healthy_owner`、`_v4`、消费端守恒、
  `canonical`、self-test 负样本。
- `crates/axnet/src/async_rx.rs`：`RxRxFuture.initial_link_pending`；`poll_first`/`poll_active`
  初始 link trigger；`RecoveryDriverStats.link_unsupported`；fake `read_link_status`；
  各 recovery factory `link=true` 默认；5 个新增 owner 级测试。
- `Makefile`：host-test 两条 P3 source guard。
- `openspec/…/iterations/007-…/002-replan.md`：Plan Context `draft→ready`、Authorization
  `PASS`（用户批准），Act Response 本次填写。

**Deviations from Plan**

- 无实质偏差。非实质记录：
  - P2 触发实现用单一 `initial_link_pending` 布尔在 `poll_first` 置位、`poll_active` 组合消费，
    符合「最小布尔 pending 状态」指引；`Again` 分支不重新置位（仅 re-publish CONFIG 保留 retry，
    初始标志只被非-Again 分支清除），避免把 config-Again 语义并入「initial」。
  - 为修复 6 个既有 recovery/lifecycle 测试在新初始读下的误触发 link-down，把 `RecoveryDriverStats`
    factory 默认 `link=true`（健康 NIC up）。这是夹具建模修正，不属于 driver/产品行为；所有 link
    专项测试均显式置位。
  - P4 手工 QEMU 所需完整命令在 `Blocker Handoff` 提供；`required` Evidence 仅在用户回传后创建。

**Blocker Handoff**

Task 4.2-P4 已在用户手工 QEMU 上执行，**未通过**：`pre_reset_traffic` 在
`FAIL: pre_reset_traffic reason=precondition` 失败（旧载荷），随后同一会话以 4-DBG 载荷重跑出现
probe 进程段错误。因此 runtime Acceptance（A3 peer 三阶段、A4 reset/old-new socket HMP）未达成，
按 Gate 6 阻塞交接。

恢复条件（已写入本段落下方）：根因判定分两层——主 finding 为 guest UDP TX 数据面问题（产品），
需 Plan 决定修复范围；次 finding 为 probe 4-DBG 载荷 guest 段错误，需单独隔离。两者解决且
自动/手工 Gate 全绿后，再创建 `required` Evidence。

**Diagnostic Addendum（runtime，R55 分层采集）**

问题定位基于 info 级 mirror（`make LOG=info build`）+ `-object filter-dump` pcap（层间客观证据，
不依赖日志级别）+ kernel 时间戳，按 R55 逐层对照表归因。证据源：
`/tmp/ms07-qemu-serial-info.log`（info 串口）、`/tmp/ms07-usernet.pcap`（filter-dump pcap）。

| 层 | 证据 | 判定 |
|---|---|---|
| 驱动注册 | info serial `eth0: mac: 52-54-00-12-34-56 ip: 10.0.2.15/24`、`registered a new Net device`、async RX queue task | PASS（eth0/IP 正确） |
| TCP RX/TX | info `TCP connection from 10.0.2.15:49152 to 10.0.2.2:18765` + pcap 中 wget 双向帧（SYN/ACK/data/guest ACK） | PASS（同网卡 TCP 全通） |
| UDP socket/bind/connect | info `UDP socket #0: bound on *:49152`；probe 无 `DBG: stage=socket/connect` → socket/connect 成功 | PASS |
| **UDP TX（send → 出网卡）** | **pcap 0 帧 UDP、0 帧 15572；probe 绑定后 ~0.9ms 退出（`Task(…,"ms07") exit code 256`），非 30s echo 超时** | **FAIL：`send()` 立即失败、零 datagram 交付网卡** |
| peer/HMP | 未到达（无帧出 guest） | N/A（无证据归因 peer） |

**主 finding（产品数据面症状，非环境）**：同一块 VirtIO-NIC 上 TCP RX+TX 全通，但非阻塞 connected-UDP
的 `send()` 在 guest 内核立即失败且无 datagram 出网卡。`axnet::udp::try_send_once` 走
`socket.is_open()` → `!can_send()` → `socket.send()`，非阻塞下 `poll_io` 只调用一次，任一步返回错误即
WouldBlock/NotConnected 立即回给 syscall，与 0.9ms 退出、零 pcap UDP 帧一致。这是首次在真实单 hart
QEMU 上暴露的 UDP 数据面缺陷（此前 Cycle 001 卡在 `connect()`，从没推进到 `send()`）。**exact
syscall errno 尚未取得**（被次 finding 阻断）；按 MS07 规范要求，缺该 errno 前不得把 UDP 判为
`connect` 产品缺陷——此处已用 pcap 客观证实 send 无包上线，且 connect 实为成功（无 socket/connect
errno），故定位到 send/egress 层。

**次 finding（probe 工具稳定性，独立信号）**：4-DBG 载荷在 guest 段错误
`Task(22) "ms07" segmentation fault at VA:0x1a000 READ | USER` → SIGSEGV → exit 139。崩溃点位于
`MS07_CASE_START` 之前（`open_peer_socket`/`peer_exchange` 之前，该区域与旧二进制逐字节相同），
故非本轮新增 `printf` 逻辑所致；二进制体检正常（合法 static RISC-V ELF，entry 0x10252，6 个 DBG
字符串均在，19:34 全新构建）。该崩溃是独立于 UDP 的第二个待查信号，需单独隔离，且它阻断通过 probe
重跑获取 `send()` errno 的手段。

**Blocker Resolution**

None。用户已执行手工诊断 QEMU 并回传失败（`pre_reset_traffic` precondition + 4-DBG 载荷
SIGSEGV）；阻塞（UDP TX 数据面 + probe 段错误）未解决，无恢复豁免或修复指令，故保持 `blocked`。

**Self-Review**

- Plan compliance: PASS（P1–P3 与 P4 自动 Gate）；P4 runtime 未通过（UDP TX 数据面 + probe 段错误）
- Full diff reviewed: PASS（覆盖下述所有文件，连贯无计划外修改）
- Critical findings unresolved: 1（guest UDP TX 数据面 `send()` 零上线——阻塞 A3/A4，记录于
  Diagnostic Addendum，未静默通过）
- Important findings unresolved: 1（probe 4-DBG 载荷 guest SIGSEGV——阻断 errno 获取，待隔离）
- Minor findings unresolved: 2（见 Remaining Issues；均不阻塞）

逐任务契约核对：P1 删除全部健康态 `device_owned==0` 与三字段单 QS 假设，负样本覆盖
`63/64`、`64/63`、quarantine 与跨阶段 drift，product driver `owner_summary` 未改、其 focused
测试保持 GREEN；P2 的 initial up/down/Again/不重复/Unsupported 五类 witness 与 no-fabricated
ISR 断言齐备，两套 axnet 串行全量及 kernel build 通过；P3 的 stage+errno 与 source guard 满足。
V1–V4 ABI、V1–V3 字段均未改动；ISR 仍只 ack/publish；唯一 spawn seam、register-recheck、无
10ms polling fallback、不新增第二 task 均保持。`QueueEpoch` 不因 link 变化推进。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| P1 RED 见证 | `/tmp/p1_red_witness`（old vs new predicate） | healthy 64/64/0 old=FAIL new=PASS；RX-absent 64/0 old=PASS new=FAIL | PASS |
| C decision test | `cc -std=c11 -Wall -Wextra -Werror tests/ms07_recovery_probe_test.c` + run | exit 0 | PASS |
| validator self-test | `python3 scripts/ms07-qemu-validate.py --self-test` | exit 0 | PASS |
| schema/case 一致 | probe vs validator `--print-case/--print-schema` + diff | identical | PASS |
| driver 契约基线 | axdriver_virtio `owner_summary` focused / full | 7/7；全量 36/36，exit 0 | PASS |
| P2 RED | `owner_activation_commits_initial_link_without_config_cause` | 实现前 `link_reads==0`（RED）→ 实现后 PASS | PASS |
| P2 owner focused | 5 个 `initial_link` witness | 5 passed，0 failed | PASS |
| axnet ordinary 全量 | `cargo test … -- --test-threads=1` | 472 passed；0 failed，exit 0 | PASS |
| axnet qemu-diagnostics 全量 | 同命令 + `--features qemu-diagnostics` | 504 passed；0 failed，exit 0 | PASS |
| P3 source guards（RED→GREEN） | host-test 两条 guard | 修复前 RED（有 fcntl / 无 O_NONBLOCK at socket）；修复后 GREEN | PASS |
| RISC-V payload | `make tests/ms07_recovery_probe` | riscv64-linux-musl-gcc static，exit 0 | PASS |
| kernel build | `make ARCH=riscv64 build` | `.bin` 生成，exit 0 | PASS |
| host-test | `make host-test` | exit 0（含新 P3 guards、ms05/06/07 seams） | PASS |
| Rust host harness | `rustc --test tests/ms07-recovery-host-harness.rs` + run | 3 passed；0 failed | PASS |
| rustfmt | `cargo fmt … -- --check` + `git diff --check` | exit 0 | PASS |
| OpenSpec validate | `openspec validate ms07-… --strict` | `Change … is valid`，exit 0 | PASS |
| P4 runtime（user QEMU） | 手工 single-hart QEMU + `pre_reset_traffic` | `FAIL: pre_reset_traffic reason=precondition`（`MS07_HARNESS_EXIT: 1`） | FAIL |
| 诊断：UDP TX 上线 | `tcpdump -r /tmp/ms07-usernet.pcap -nn` | 120+ 帧全部 wget TCP；**0 UDP、0 15572** | FAIL（产品） |
| 诊断：send 时序 | info serial kernel 时间戳 | `Task(…,"ms07") exit code 256`，绑定后 ~0.9ms 退出（非 30s echo 超时） | FAIL（send 立即失败） |
| 诊断：次 finding | 4-DBG 载荷 guest | `Task(22) "ms07" segfault at VA:0x1a000 READ` → SIGSEGV，exit 139 | FAIL（工具稳定性，待隔离） |
| probe 二进制体检 | `file`/`readelf`/`strings` | 合法 static RISC-V ELF，entry 0x10252，6 个 DBG 字符串全在 | PASS |

**Persisted Evidence**

None created by Act。Cycle Plan 的 `required` Evidence（`README.md`、`ms07-protocol.txt`、
`regressions.txt`）按计划「仅在用户结果（PASS）到达后创建」；本轮 runtime 为 FAIL，不满足收尾
白名单，故未收集。决定性诊断摘录（pcap 0 UDP、0.9ms 时序、SIGSEGV）已写入上文 Diagnostic
Addendum；原始现场 `/tmp/ms07-qemu-serial-info.log`、`/tmp/ms07-usernet.pcap` 仍在临时目录，
如需给 Plan 复核可由用户/Recorder 按 budget（本 Cycle ≤5 文件，change ≤20）另行保存，Act 不
创建占位目录。

**Experience Candidates**

None。自动 Gate 与修复均可低成本重跑；recovery 夹具默认 link 修正是可复现的单点教训，但不构成
已验证端到端的 Runbook 或显著/难复现/含系统性诊断的 Incident 候选。

**Remaining Issues**

- **Critical（阻塞 A3/A4）**：guest UDP TX 数据面——非阻塞 connected-UDP `send()` 在 async NIC 上立即
  失败、零 datagram 出网卡（pcap 0 UDP + ~0.9ms 退出 + 同网卡 TCP 全通）。exact syscall errno 缺失
  （被次 finding 阻断），此处以 pcap 客观证实 send 无上线，定位到 send/egress 层。修复超出本轮
  T4.2-P1/P2/P3 契约，需 Plan 决定范围。
- **Important（阻断诊断手段）**：probe 4-DBG 载荷 guest SIGSEGV（VA 0x1a000 READ，`MS07_CASE_START`
  前、同旧二进制区域；二进制体检正常），需单独隔离，才能继续用 probe 拿 `send()` errno。
- Minor（非阻塞）：`pre_reset_link_generation` 因初始 link 从 0 提交为 up 而变为 1；validator 只
  校验相对 link-generation 关系，不固定绝对起始值，符合 plan 风险说明。
- Minor（非阻塞）：初始 link 成功提交后，`link_generation` 默认增 1；probe/validator 只校验相对
  关系。属观测语义，不影响 runtime 资格判定。

**Commit or Diff Reference**

Diff reference: 工作树（未提交）。本 Cycle 变更覆盖 `tests/ms07_recovery_probe.c`、
`tests/ms07_recovery_probe_test.c`、`scripts/ms07-qemu-validate.py`、`crates/axnet/src/async_rx.rs`、
`Makefile`（P1–P3 + host-test guards）与 002-replan.md（gate 状态 + 本 Response）。既有 staged
外部改动（`scripts/cc-nopie.sh` 等）不属于本 Cycle，未纳入声明。commit 未建（未获提交授权）。

## Plan Review

- Review Result: replan-required

**Findings**

1. **Critical — PLAN-INVALID：P3把合法的nonblocking backpressure写成失败。**
   `open_peer_socket()`启用nonblocking后，`peer_exchange()`虽先等待`POLLOUT`，却只调用一次
   `send(MSG_DONTWAIT)`；任何`EAGAIN/EWOULDBLOCK`都立即使`pre_reset_traffic`失败。Plan同时禁止
   retry loop，无法实现“同一absolute deadline内处理poll/send readiness race”的nonblocking语义。
   Act未取得send errno；pcap没有UDP datagram和约0.9 ms退出只能证明payload未产生线上UDP帧，不能在
   `WouldBlock`、`NotConnected`、用户缓冲错误或其他syscall结果之间做选择，因此“UDP产品数据面缺陷”
   尚未成立。
2. **Critical — NEW-EVIDENCE：新payload的用户态页故障尚未定位到指令。**
   Act只有fault VA `0x1a000 READ | USER`，没有fault PC、SP、RA、faulting instruction或runtime
   bytes。当前exact ELF是static RISC-V `ET_EXEC`，entry `0x10252`；第二个`PT_LOAD`为
   `vaddr=0x18fd8, memsz=0x918`，页对齐映射区间恰好是`[0x18000, 0x1a000)`，所以
   `0x1a000`是其首个越界页。缺fault PC时，不能证明它发生于旧代码、不能把fault VA当作PC，也不能
   区分probe/musl越界与loader映射问题。
3. **Important — PLAN-OMISSION：blocked runtime现场没有可复核的持久证据。**
   Cycle 002只为最终PASS规划required Evidence；本次一次性手工session的serial/pcap已不在当前
   `/tmp`，Plan Review只能核对Act Response摘录，无法复查packet类型、完整trap上下文或命令对应关系。
   后继Cycle必须允许FAIL/BLOCKED现场进入受预算约束的Evidence。
4. **Minor：socket type应使用接口名`SOCK_NONBLOCK`。**
   当前musl/Linux数值上`O_NONBLOCK`可被kernel的`raw_ty`掩码识别，但socket API的契约名称是
   `SOCK_NONBLOCK`；源码和Makefile guard应同步改名。该问题单独不阻塞Acceptance。
5. **已核实可保留的部分。** P1的64/64 owner消费者与P2初始link owner路径和现有设计一致；独立重跑
   C decision test与Python validator self-test均exit 0。当前环境下axnet focused test在host PIE/
   percpu relocation链接处exit 101，未进入测试执行；这是Review环境限制，不推翻Act记录的既有owner
   witness，但后继Cycle仍须用项目规定的非PIE命令重跑。

**Deviation Classification**

PLAN-INVALID；PLAN-OMISSION；NEW-EVIDENCE。

**Acceptance Gaps**

- A3未满足：没有可信的guest send stage errno，也没有成功的pre-reset双向UDP exchange。
- A4/A5未开始：reset、旧/新socket、HMP down/up均未到达。
- A6未满足：没有本次有效runtime之后的MS01/MS04/MS05/MS06明确终态。
- 用户payload自身发生未定位页故障，当前probe不能作为后续runtime事实源。

**Convergence**

reduced。相对Cycle 001，错误owner假设和初始link unknown已由自动实现与测试关闭；剩余缺口已缩小到
guest payload执行可靠性、UDP syscall精确结果和后续runtime，但Cycle 002的P3执行契约不足以继续。

**Evidence**

- `tests/ms07_recovery_probe.c::open_peer_socket/peer_exchange`：创建nonblocking socket，poll后send仅尝试
  一次，`sent != n`立即失败。
- `crates/axnet/src/udp.rs::UdpSocket::send/try_send_once`：nonblocking `poll_io`可将
  `WouldBlock`直接返回；该错误是接口结果之一，必须由errno区分。
- `kernel/src/syscall/net/io.rs::send_impl`：syscall直接传播`Socket::send()`错误。
- `kernel/src/task/user.rs`：当前不可处理页故障日志只打印进程、fault VA与flags，不打印保存的user PC。
- `readelf -h -l -S -s tests/ms07_recovery_probe`：`ET_EXEC`、entry `0x10252`、第二LOAD映射到
  `0x1a000`前；`llvm-objdump`确认`run_probe`位于`0x10aba`且栈帧为`0x520`。
- C decision test与Python validator self-test：exit 0。
- Review时`/tmp/ms07-qemu-serial-info.log`与`/tmp/ms07-usernet.pcap`均不存在。

**Follow-up Decision**

冻结Cycle 002并创建Cycle 003 replan。先建立可匹配exact ELF的fault PC/寄存器与分阶段syscall witness；
只有实际errno为`EAGAIN/EWOULDBLOCK`时才在共享absolute deadline内增加有界poll/send重试。其他errno、
runtime bytes不匹配或loader映射缺陷一律停止并返回Plan，不预授权UDP或通用ELF loader产品修复。

**Iteration Plan Update**

Task 4.2和Iteration 007的目标不变，但诊断与验证契约改为：probe首先证明自身可执行并记录fault PC或
精确syscall stage+errno；nonblocking `EAGAIN/EWOULDBLOCK`按同一deadline重试；一次性手工FAIL现场也
必须保存最小serial/pcap Evidence。自动Gate之后的完整MS07与四组回归仍是最终Acceptance。

**Next Cycle**

`003-replan.md`

**Next Iteration**

None.
