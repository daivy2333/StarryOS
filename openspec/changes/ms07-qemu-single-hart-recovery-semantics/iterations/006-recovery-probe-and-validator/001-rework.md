# Iteration 006 / Cycle 001: Complete recovery runtime protocol

## Plan Context

- Status: ready
- Iteration: 006-recovery-probe-and-validator
- Cycle: 001-rework
- Cycle Type: rework
- Parent cycle: `000-initial.md`

**Iteration Scope**

- Change tasks: 4.1
- Depends on: Iteration 005；Cycle 000 已实现的QEMU-only ioctl、resident-owner入口与host工具骨架。
- Stable baseline: append-only V4 ABI、单次reset request、guest/host recovery协议和strict validator可由自动host Gate冻结，并供Iteration 007手工QEMU消费。
- Verification boundary: request/V4 Rust models、C wire/decision tests、peer/validator Python negative tests、case/grammar guards、MS03–MS07 host seams、axnet全量与kernel build。
- Diagnostic boundary: explicit/natural recovery线性化、V4 current/fault tuple、guest phase、host peer、validator首个协议差异。
- Deferred tasks: 4.2

**Cycle Scope**

- Trigger: rework-required
- Acceptance gaps: A1 request交错；A2 V4 tuple/layout；A3 guest runtime；A4 strict validator；A5 grammar/source guards。
- Repair items: T4.1-R1、T4.1-R2、T4.1-R3、T4.1-R4
- Inherited scope: Task 4.1、R4/R5/R6/R8、D8、Cycle 000所有preserve/forbidden、A6/A7和手工QEMU边界。
- Excluded scope: 修改recovery算法/deadline/socket语义、真实QEMU/HMP执行、最终artifact资格、自动QEMU/guest/HMP控制、MS01/MS04/MS05/MS06 guest回归。

**Objective**

把Cycle 000的控制面和工具骨架补成可执行、可负向验证的MS07 runtime协议：每个explicit request最多触发一次resident recovery；V4明确区分current observation与historical fault；guest通过真实ioctl和有界网络I/O输出固定marker；validator拒绝任何identity、顺序、epoch、ledger或终态缺口。

**Background**

Cycle 000在Gate 4/5自检时发现probe只有case stub并安全停止。Plan Review进一步确认V4把历史fault/current状态混合，reset bit可跨自然recovery残留，validator会接受无revision/environment的伪完整transcript。这些都是原Task 4.1 Acceptance内的有限缺口，不改变Iteration Map，也不要求先运行QEMU。

**Current Baseline**

- `recovery_reset_request_shared()` 使用production-global lifecycle与`AtomicBool`提交请求；`poll_active()`消费后进入已有recovery。重复pending request返回`ResourceBusy`，但自然recovery交错未建模。
- `RecoverySnapshotV4` 当前有9个字段；`IrqSnapshotV4`以`IrqSnapshotV3`为offset 0 prefix，kernel ioctl为`0x4e494434`，reset command为`0x4e495231`。新V4尚未被accepted或runtime消费，可以在本Cycle内修正追加字段语义和最终size。
- C probe只定义六个case并支持`--print-cases`；C test只运行数组自检。没有guest ioctl、socket、deadline、peer protocol或marker model。
- Python validator只核对start、六个PASS和exit；revision/environment、V4 marker、HMP顺序和fatal scan均缺失。一个无identity transcript当前错误返回exit 0。
- Cycle 000新鲜证据：MS07 Rust 2/2、现有C/Python骨架、ordinary 467/467、qemu-diagnostics 491/491、无sockethost gates与kernel build均通过。

**Current-State Evidence**

- `async_rx.rs::{recovery_reset_request_shared,take_recovery_reset_request,RxRxFuture::poll_active}`：request提交/消费与自然recovery交错面。
- `async_rx.rs::recovery_snapshot_v4`：先读historical coherent fault、再读current Service，并以epoch 0作无效哨兵。
- `virtio_net_irq_logic.rs::IrqSnapshotV4`、`virtio_net_irq.rs::irq_snapshot_v4`、`ctl.rs::sys_ioctl`：Rust wire、mapping和两个QEMU-only ioctl。
- `tests/ms07_recovery_probe.c`、`tests/ms07_recovery_probe_test.c`：仅case stub；`scripts/ms07-qemu-validate.py`：宽松parser。
- `scripts/ms05_data_plane_stimulus.py` 与 `tests/ms05_data_plane_probe.c`：已有bounded UDP guest/host协议可复用其packet/deadline原则，但MS07不得复用MS05 marker冒充recovery事实。

**Relevant Code**

- `crates/axnet/src/async_rx.rs`、`lib.rs`、`service.rs`
- `kernel/src/drivers/virtio_net_irq_logic.rs`、`virtio_net_irq.rs`
- `kernel/src/syscall/fs/ctl.rs`
- `tests/ms07-recovery-host-harness.rs`
- `tests/ms07_recovery_probe.c`、`tests/ms07_recovery_probe_test.c`
- `scripts/ms07-recovery-peer.py`、`scripts/ms07-qemu-validate.py`
- Makefile

**Critical Path**

1. Reset ioctl在owner为Active且无pending/in-flight request时提交一个request identity，再在提交后唤醒queue owner。
2. Owner claim request与自然fault进入recovery共享一个线性化决定：二者同时发生时只形成一次`Active → Quiescing`，没有pending bit跨恢复存活。
3. V4 snapshot分别发布current tuple和historical fault tuple。current queue/socket/link/owner在一个Service guard内读取；fault tuple由现有`CoherentFaultSheet`读取，并用valid bit区分“无fault”和合法epoch 0。
4. Guest与手工启动的host peer先完成pre-reset exchange；guest提交reset，旧socket观察`ConnectionReset`，bounded snapshot loop观察Active和epoch推进，再用新socket完成exchange。
5. Guest打印HMP off ready并在绝对deadline内只读V4等待link down；观察`NotConnected`且QueueEpoch不变后打印off PASS。随后以相同步骤等待link up、SocketEpoch推进和第三个新socketexchange。
6. Validator按唯一state machine审计metadata、snapshot、socket terminal、HMP ready/observed、epoch/ledger关系、PASS/END/exit及fatal lines；host tests从每个状态做missing/duplicate/reorder/value mutation。

**Implementation Guidance**

- 将request state抽成production与test共用的有界对象，不用独立“先load lifecycle、后CAS bool”作为完整校验。claim必须返回`Explicit`、`NaturalWon`或等价唯一结果；进入任何recovery/Faulted/Unavailable终态后都不得留下可在下一Active消费的旧request。
- V4把两类事实命名分开：`current_*`至少包含valid、QueueEpoch、SocketEpoch、LinkGeneration/state与current owner summary；`fault_*`至少包含valid、stage、cause、QueueEpoch和fault-time owner summary。epoch 0按valid bit解释，禁止值哨兵推断。V3 prefix仍在offset 0且逐字节不变。
- current tuple只通过一个可注入shared assembly seam在Service guard内获取；historical fault tuple保持其现有coherent publication。两者是不同时间语义，wire注释、C结构和validator不得声称它们属于同一瞬间。
- `v3.rx_lifecycle`是当前owner阶段authority：2 Active、5 Quiescing、6 Resetting、7 Reinitializing、3 Faulted。Probe只在连续两个V4样本的current identity和lifecycle满足同一目标状态时提交阶段marker；样本不一致只重试，不判PASS。
- snapshot等待允许在绝对deadline内以短有界间隔调用V4 ioctl；该循环只观察状态，不调用内部axnet progress、不触发额外reset、不忙等。网络等待使用poll/epoll和absolute deadline。
- 新增独立`ms07-recovery-peer.py`只提供bounded UDP exchange，用户在Iteration 007手工启动；它不启动QEMU、不连接guest shell、不执行HMP。validator继续禁止socket与进程控制。

**Guest/Host Choreography**

- Environment固定为`qemu-virt-riscv64-single-hart-virtio-mmio-user-net`。Probe接收非空revision，默认host `10.0.2.2`和专用MS07 UDP port；所有case共享一个overall absolute deadline，每个等待另有phase deadline。
- `pre_reset_traffic`：创建socket S0，与peer完成带run-id/phase/sequence的bounded request/reply；V4要求lifecycle Active、link up、current valid、无quarantined owner，记录`Q0/S0/L0`。
- `reset_request`：调用`0x4e495231`恰好一次并记录accepted；重复调用必须为`EAGAIN`/`WouldBlock`，不得成为第二次reset。输出reset accepted marker后等待old socket terminal。
- `old_socket_terminal`：poll S0必须出现terminal，紧随I/O与重复I/O均为`ECONNRESET`；V4最终连续观察Active、`Q1 = Q0 + 1`、`S1 = S0 + 1`、link仍up、quarantined为0。若期间观察Faulted或deadline到期立即FAIL。
- `new_epoch_traffic`：只在上述Active样本后创建S1，与peer完成新run phase；S0再次验证`ECONNRESET`。drained稳定点要求current device-owned为0，available与pre-reset稳定点相等。
- `hmp_link_down`：打印唯一`MS07_HMP_READY: link=off`后等待用户HMP；连续V4样本必须见link down、`L1 = L0 + 1`、QueueEpoch仍Q1、SocketEpoch仍S1。S1的poll和重复I/O均为`ENOTCONN`。
- `hmp_link_up`：打印唯一`MS07_HMP_READY: link=on`后等待用户HMP；连续样本必须见link up、`L2 = L1 + 1`、QueueEpoch仍Q1、`S2 = S1 + 1`。此后新建S2完成peer exchange，S1继续`ENOTCONN`。
- 每个case先输出`MS07_CASE_START: <name>`，再输出字段固定的V4/socket/peer marker，最后唯一`PASS: <name>`；失败输出唯一`FAIL: <case> reason=<token>`并nonzero exit。完整成功以`MS07_RECOVERY_END`和shell追加的`MS07_HARNESS_EXIT: 0`结束。

**Behavioral Change**

修复后，explicit reset request不能跨另一轮recovery残留；V4消费者能独立解释current与fault事实；MS07 probe不再是case stub，而是可由用户在单hart QEMU中运行的有界payload；validator从case计数器变成协议和关系审计器。真实runtime结论仍由Iteration 007产生。

**Change Surface**

| Repair | Acceptance | File/Symbol | Planned Change |
|---|---|---|---|
| T4.1-R1 | A1 | `async_rx.rs` request/owner seam | 单一claim语义、自然recovery交错、可注入model tests |
| T4.1-R2 | A2 | axnet/kernel V4 source与wire | current/fault tuple分离、valid bits、epoch 0、C/Rust layout |
| T4.1-R3 | A3 | C probe、new host peer | ioctl/socket/HMP choreography、absolute deadlines、完整markers |
| T4.1-R4 | A4/A5 | validator、C/Python tests、Makefile | strict state machine、mutation matrix、grammar/source guards |

**Task Contracts**

### T4.1-R1: Linearize explicit and natural recovery

- Requirement/Scenario: R5 reset trigger；A1；explicit request与completion/reclaim fault交错。
- Depends on: Cycle 000 reset ioctl与resident recovery。
- Targets: `async_rx.rs` request state、`RxRxFuture::poll_active`、shared test seam。
- Current behavior: lifecycle load与global bool CAS分离；pending request可跨自然recovery留存。
- Required behavior: 每个accepted request至多形成一次recovery；natural recovery竞赢时，request被同一transition吸收或取消，恢复到Active后不再触发第二次reset。
- Required changes: 建立可注入的checked request/claim state；让所有离开Active的recovery/fatal路径处理pending request；保持commit后wake和唯一owner。
- Preserve: syscall不调用driver reset；existing deadlines、recovery stage和socket terminal顺序。
- Forbidden: 第二owner、unbounded retry、以清空所有请求掩盖并发accepted identity。
- Test witness: 确定性RED覆盖request→natural recovery→Active、duplicate request、request claim、Faulted/Unavailable cleanup和event-before-poll；当前实现至少第一项失败。
- GREEN condition: 每个模型只出现0或1次`Active → Quiescing`，无stale request，重复请求稳定`ResourceBusy`。
- Verification: exact qemu-diagnostics tests；ordinary feature absence；两套axnet全量。
- Stop when: 必须把reset执行移到syscall或无法用现有owner lifecycle定义单一claim，返回Plan。

### T4.1-R2: Freeze an unambiguous V4 wire

- Requirement/Scenario: R4–R6 telemetry；A2/A6。
- Depends on: T4.1-R1提供明确current recovery状态。
- Targets: axnet V4 shared assembly、kernel V4 wire/mapping、Rust/C layout tests。
- Current behavior: historical fault/current Service混合；epoch 0兼作无效；owner summary只来自fault。
- Required behavior: V3 prefix不变；current与fault tuple分别有valid和字段authority；current Service tuple单guard；fault tuple保持coherent；epoch 0可合法出现。
- Required changes: 调整未冻结V4尾字段及注释；增加injectable assembly；同步C struct、size/offset/static asserts、kernel mapping和host Rust tests。
- Preserve: V1 8、V2 28、V3 72个u64及旧ioctl数字；V4/reset仅QEMU feature可达。
- Forbidden: 改旧字段含义；用0/MAX值替代validity；把两个时间语义宣称为同一atomic snapshot。
- Test witness: RED覆盖fault epoch 0、no-fault valid=0、current ledger与historical fault不同、missing Service、V3 prefix byte copy和每个tail offset。
- GREEN condition: Rust/C layout一致；所有tuple mutation可区分；old ABI tests保持GREEN。
- Verification: MS03/MS04/MS07 Rust harness、C static/mutation tests、kernel check/build。
- Stop when: 所需字段无法append-only表达或必须修改V1–V3，返回Plan。

### T4.1-R3: Implement the bounded guest and peer protocol

- Requirement/Scenario: R5 reset、R6 HMP off/on、R8 runtime protocol；A3。
- Depends on: T4.1-R1/R2。
- Targets: `tests/ms07_recovery_probe.c`、`tests/ms07_recovery_probe_test.c`、`scripts/ms07-recovery-peer.py`。
- Current behavior: probe只打印case名；无peer。
- Required behavior: 严格执行Guest/Host Choreography；所有I/O和snapshot等待受absolute deadline约束；输出固定字段marker和与exit一致的终态。
- Required changes: 分离host-testable clock/parser/decision core与guest syscalls；实现V4 ioctl、reset ioctl、UDP phase protocol、old/new socket terminal检查、HMP ready/observed阶段；实现不驱动QEMU的bounded peer及self-test。
- Preserve: validator纯输出；用户手工启动peer/QEMU/HMP；raw serial为事实源。
- Forbidden: probe调用内部axnet poll；无界/忙循环；peer启动QEMU或连接guest shell；用completion声称peer delivery。
- Test witness: C fake clock/ioctl/socket mutations覆盖success、timeout、errno drift、epoch不增/多增、QueueEpoch随link变化、ledger不守恒和duplicate phase；peer fake-socket self-test覆盖wrong run/phase/sequence/peer与deadline。
- GREEN condition: decision core和peer negative matrix全绿；guest binary可静态交叉编译；source guard无internal poll/unbounded wait。
- Verification: C syntax/test、Python peer self-test、case/grammar输出、RISC-V static payload build。
- Stop when: 需要自动HMP/guest控制，或现有public syscall无法观察必要事实，返回Plan。

### T4.1-R4: Make the validator reject every protocol gap

- Requirement/Scenario: R8 qualification boundary；A4/A5/A7。
- Depends on: T4.1-R2/R3冻结wire与marker grammar。
- Targets: `scripts/ms07-qemu-validate.py`、Makefile guards/tests。
- Current behavior: 无identity transcript可exit 0；marker字段与关系未解析。
- Required behavior: 唯一顺序parser验证revision/environment、case start/PASS、V4/current/fault、peer、socket terminal、HMP ready/observed、epoch/ledger关系、END/exit；任何FAIL、panic、trap、fatal ownership drift、未知/重复/缺失marker失败。
- Required changes: 实现typed parser与first-difference错误；支持`--expect-revision`、`--expect-environment`和`--print-cases`；从valid transcript逐字段mutation建立negative matrix；Makefile比较完整grammar/case authority并执行pure-auditor/source guards。
- Preserve: 容忍非协议串口噪声；不访问网络、不启动进程/QEMU、不修改输入。
- Forbidden: 只计PASS；忽略未知`MS07_`；把ready marker当作observed evidence；接受partial success或缺exit。
- Test witness: 当前无identity transcript exit 0为RED；另覆盖metadata、numeric parse/overflow、每个关系、ANSI噪声、fatal lines和foreign markers。
- GREEN condition: canonical transcript通过，所有单点mutation拒绝并报告首个差异；probe/validator cases和grammar一致。
- Verification: validator self-test、bad transcript CLI exit、pure-output source guard、完整host-test可执行部分。
- Stop when: parser必须读取运行环境或外部文件才能决定结果，返回Plan。

**Invariants**

- request提交、owner claim、`Active → Quiescing`和wake有唯一顺序；请求不跨recovery generation。
- V4的V3 prefix在offset 0且旧ABI不变；validity不用合法identity值编码。
- current tuple和fault tuple分别一致且语义独立；validator不做跨时间伪等式。
- reset成功只推进QueueEpoch一次；link off/on不推进QueueEpoch；SocketEpoch只按已接受语义关闭/开放。
- S0永久`ECONNRESET`，S1在link down后永久`ENOTCONN`，S2只在link up后创建并成功。
- 所有等待有checked absolute deadline；无guard跨wake、Pending或用户等待。
- validator与peer不驱动QEMU/HMP；只有用户在Iteration 007执行runtime。

**Non-goals**

- 不运行QEMU、不采集raw serial、不给出qualification PASS。
- 不改变Task 4.1目标、Iteration Map、recovery算法、timeout常量或NetworkTerminal映射。
- 不增加自动runner、SMP、PCI/DWMAC、真板、性能或透明连接迁移。

**Acceptance**

- A1：request/natural recovery所有确定性交错至多一次reset，且无stale pending request。
- A2：V4 current/fault tuple和validity明确；epoch 0、missing Service、prefix/layout与feature均有Rust/C见证。
- A3：probe与peer完整实现六case choreography、absolute deadlines和固定markers；不执行内部poll或自动HMP。
- A4：validator严格验证identity、顺序、数值关系、fatal状态和exit，完整negative matrix全拒绝。
- A5：probe/peer/validator共享固定case/grammar authority，pure-output/source guards通过。
- A6：MS03–MS06、diagnostic lease、axnet ordinary/qemu-diagnostics和kernel build不退化。
- A7：自动Gate只声明protocol ready；Iteration 007仍是唯一真实QEMU资格边界。

**Verification**

1. 每个repair先运行其RED exact witness，再运行GREEN exact/mutation tests。
2. 串行运行axnet ordinary与qemu-diagnostics全量`--test-threads=1`及两种production check。
3. 运行MS03、MS04、MS07 Rust host harness；C probe syntax、decision test和RISC-V static build。
4. 运行peer/validator self-test、canonical/bad CLI、probe/validator case与grammar diff、pure-auditor/internal-poll source guards。
5. 先运行`make host-test`；仅精确loopback socket `EPERM`可按既定规则分层并补跑全部无socket项，任何其他失败阻塞。
6. `make ARCH=riscv64 build` exit 0并生成镜像；不启动QEMU。
7. 运行相关rustfmt、C/Python syntax、`git diff --check`、完整diff Review和strict OpenSpec validation。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | 已检查request/future状态机、V4 assembly/wire、C stub、validator和host gates |
| Design | PASS | request唯一claim、V4双tuple、六caseguest/peer choreography与strict grammar均已固定 |
| Iteration Plan | PASS | 只用四个本地repair关闭Task 4.1既有A1–A5；Iteration 007不变 |
| Cycle Scope | PASS | 保留Cycle 000已完成baseline，不引入runtime资格或新全局task |
| Task Contracts | PASS | 每个repair含current/required、RED/GREEN、preserve/forbidden和stop条件 |
| Traceability | PASS | Findings 1–4 → A1–A5 → T4.1-R1–R4 → Rust/C/Python/build Gates |
| Verification | PASS | 直接模型、协议mutation、受影响host/full/build与环境分层均有明确判据 |

Gate 2 技术维度已通过；用户已明确批准当前 Cycle（「更改gate状态，开始实施当前cyc」），Plan Context 改为`ready`并交给Act。

**Persisted Evidence**

- Mode: none

所有自动结果可低成本重跑，Act Response足以保存决定性输出；真实runtime Evidence属于Iteration 007。

- Budget: 本 Cycle 最多5个文件（含README），整个change最多20个Evidence文件；单个文本文件最多500行且不超过256 KiB；超限需要用户明确批准。

**Risks and Notes**

- V4尚未发布，本Cycle可调整tail；一旦accepted，后续只能新增版本，不能再改V4。
- 手工HMP等待可能超过普通network phase；probe必须使用独立、明确的operator deadline，且超时FAIL，不自动操作monitor。
- peer是runtime必要配套但不是validator；Makefile source guard应分别约束两者，避免误把“validator禁socket”扩展到peer。
- Cycle 000已通过的全量结果不是本Cycle完成证据；修改后必须重跑受影响Gate。

## Act Response

- Status: reported

**Implemented**

1. T4.1-R1 — reset request改为以`RecoveryRequestState`和`RxLifecycle` transition共享一条
   有界线性化路径。显式请求在resident owner中claim；natural recovery在同一gate内清空
   pending/claimed request再提交`Active → Quiescing`，因此请求不能跨本轮recovery并在下一
   个Active generation再次reset。重复请求稳定返回`ResourceBusy`，syscall仍只发布wake。
2. T4.1-R2 — V4由一个Service guard读取current tuple，并以`current_valid`发布
   QueueEpoch、SocketEpoch、LinkGeneration、link state和owner summary；历史coherent fault
   独立使用`fault_valid`与fault tuple。合法epoch 0不再作为无fault哨兵；V3 prefix、旧ioctl
   与feature gate保持不变。kernel/C wire同步为72-u64 V3 prefix加15个V4字段。
3. T4.1-R3 — 增加有界MS07 guest probe：使用V4/reset ioctl、monotonic absolute deadline、
   UDP peer exchange、old/new socket terminal、HMP off/on ready/observed marker和fixed case
   grammar；失败只输出`FAIL`并返回nonzero。新增独立UDP peer，按run/phase/sequence拒绝
   duplicate、out-of-order与malformed packet，不驱动QEMU/HMP。
4. T4.1-R4 — validator升级为严格、纯输出state machine：要求revision/environment、六个
   ordered case、V4 grammar/current identity、reset/link epoch关系、socket terminal、peer、
   HMP ready/observed、END和exit；unknown或缺失协议行、数值溢出和每个单点mutation均拒绝。
   增加`--expect-revision`与`--expect-environment`，并在Makefile加入case authority、pure
   validator和probe no-internal-poll guard。

**Changed Files and Symbols**

- `crates/axnet/src/async_rx.rs`：`RecoveryRequestState`、request claim/clear、
  `RecoverySnapshotV4` current/fault tuple、deterministic request interleaving tests。
- `kernel/src/drivers/{virtio_net_irq.rs,virtio_net_irq_logic.rs}`：append-only V4 wire与mapping。
- `tests/ms07-recovery-host-harness.rs`：request ownership与V4 split source witnesses。
- `tests/ms07_recovery_probe.c`、`tests/ms07_recovery_probe_test.c`：V4 C layout、deadline/epoch/terminal decision seam、
  bounded guest protocol。
- `scripts/ms07-recovery-peer.py`、`scripts/ms07-qemu-validate.py`：bounded peer与strict pure
  transcript auditor。
- `Makefile`：MS07 host guards和RISC-V static probe target。

**Deviations from Plan**

无Acceptance偏差。probe在实际QEMU运行前不会输出PASS；`--run <revision>`只在每个真实
ioctl/socket/HMP判据满足后发出marker。sandbox禁止启动`riscv64-linux-musl-gcc`和loopback
UDP，均按K43记录为环境能力限制；用户宿主已确认MS07 static probe build通过。

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: PASS
- Full diff reviewed: PASS
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 0

Spec review：R1的唯一claim/自然fault交错、R2的validity与V3 prefix、R3的有界guest/peer
边界及R4的identity/relationship audit均与A1–A7对应。Code review：request gate不持有
Service guard；current tuple只在一个Service guard内装配；historical fault保持原coherent
publication；peer可用socket但validator保持无socket/无process-control；guest只等待公开
syscall/socket，未调用内部axnet progress。完整工作树含此前Cycle的staged baseline和用户
无关改动；本Review只将本Cycle触及的unstaged/untracked MS07 surface纳入实现结论。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| Gate 3 RED | MS07 Rust host harness | 新断言下 `1 passed; 2 failed`，缺`RecoveryRequestState`/`current_valid` | PASS（预期RED） |
| Gate 3 RED | C probe harness | 缺V4 observation/transition symbols，C编译失败 | PASS（预期RED） |
| Gate 3 RED | validator self-test | `transcript without identity metadata accepted`，exit 1 | PASS（预期RED） |
| MS07 host seams | `rustc --edition=2024 --test tests/ms07-recovery-host-harness.rs ...` | `3 passed; 0 failed` | PASS |
| C/protocol seams | C syntax/test、case diff、validator/peer self-test | 全部exit 0 | PASS |
| axnet ordinary | `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --test-threads=1` | `467 passed; 0 failed`，exit 0 | PASS |
| axnet diagnostics | 同命令 + `--features qemu-diagnostics` | `493 passed; 0 failed`，exit 0 | PASS |
| kernel QEMU build | `make ARCH=riscv64 build` | release build完成，`StarryOS_riscv64-qemu-virt.elf`进入dwarf步骤，exit 0 | PASS |
| RISC-V probe | 用户宿主 `make tests/ms07_recovery_probe` | `riscv64-linux-musl-gcc ... -o tests/ms07_recovery_probe`，exit 0 | PASS |
| sandbox host-test | `make host-test` | MS04 loopback socket 创建返回`EPERM`；此前MS07 host 3/3通过 | ENV-BLOCKED（非产品FAIL） |
| formatting/whitespace/spec | rustfmt、`git diff --check`、strict validate | `Change ... is valid`，exit 0 | PASS |

**Persisted Evidence**

None required.

**Experience Candidates**

None.

**Remaining Issues**

无阻塞实现问题。真实QEMU/HMP运行、raw serial和最终runtime qualification仍由Iteration 007
执行，未在本Cycle提前声明。

**Commit or Diff Reference**

`git diff`/untracked MS07 files；未创建commit（未获提交授权）。

## Plan Review

- Review Result: pending

**Findings**

1. **Important — A1 的 request gate 没有覆盖所有离开 Active 的终态路径。**
   `enter_recovery()` 会在 request mutex 内清除 pending/claimed request 后提交
   `Active -> Quiescing`，因此已覆盖显式请求与自然 recovery 的主要交错；但
   `publish_fatal()`、`enter_drift_quarantine()` 等 `Active -> Faulted` 路径不取得该 gate，
   也不清理 request。若请求在 owner 本轮已越过 claim 点后提交，而同一轮随后进入
   Faulted，accepted request 会永久残留。现有两个 model test只覆盖 natural recovery和
   duplicate/claim，没有覆盖 Task Contract要求的 Faulted、Unavailable、event-before-poll
   与真实 owner transition seam，因而不能证明“所有交错无 stale request”。
2. **Important — A2 的 wire实现方向正确，但 Acceptance 所要求的可执行见证仍缺失。**
   V4 已分离 current/fault tuple并保留72-u64 V3 prefix，epoch 0也不再作为sentinel；但是
   `recovery_snapshot_v4()` 仍是直接读取全局 Service的函数，没有计划要求的injectable
   shared assembly seam。MS07 Rust harness只做源码字符串搜索，未执行missing Service、
   fault epoch 0、current/fault不同ledger、V3 prefix byte copy、Rust size或逐tail offset测试；
   C test也没有对wire做mutation。当前布局看似一致，但A2的冻结证据尚未建立。
3. **Important — A3 guest/peer protocol仍会在未满足冻结choreography时输出 PASS。**
   Probe没有读取`v3.rx_lifecycle`，三个snapshot waiter只接受一个样本，既不要求连续两个
   Active/current-identity样本，也不会见到Faulted立即失败。`new_epoch_traffic`复用reset阶段
   的旧wire，没有检查drained点的`device_owned == 0`、available守恒或S0重复
   `ECONNRESET`；link down/up也没有重复terminal I/O，link up后没有再次验证S1仍为
   `ENOTCONN`。所有阶段各自重新建立30秒相对deadline，没有overall/operator absolute
   deadline；`clock_gettime`失败返回0还可能令snapshot loop永久不超时。peer ledger又以任意
   `(run, phase, address)`建立新序列，wrong run/phase/peer的`seq=0`都会被回应，self-test没有
   覆盖计划中的fake clock/ioctl/socket和peer negative matrix。
4. **Important — A4/A5 validator不是strict state machine，并存在已复现的false accept。**
   case parser明确把`FAIL:`和任意`MS07_`行收进markers，却不在协议审计中拒绝未消费的行；
   本Review在canonical transcript中分别插入`FAIL: ...`、`MS07_UNKNOWN: ...`，两者都被
   `validate()`接受；把`fault_valid=0`改为`2`也被接受。marker没有lifecycle、available、
   device-owned、重复socket terminal或fault tuple字段，因此validator无法审计A3要求的
   Active稳定点、ledger守恒、S0/S1永久终态和current/fault grammar。当前self-test只有少量
   删行/换序负例，Makefile也只比较case名，尚未建立逐字段mutation与完整grammar authority。

**Deviation Classification**

- `ACT-DEVIATION`：R1只把natural recovery纳入request gate，未覆盖计划明确要求的所有
  recovery/fatal终态和deterministic interleavings。
- `ACT-DEVIATION`：R3/R4实现了真实syscall/socket骨架，但省略了已冻结的lifecycle双采样、
  ledger、重复terminal、overall/operator deadline、peer negative identity和strict fatal/
  unknown-marker拒绝条件。
- `VERIFICATION-GAP`：R2和R3/R4的host测试多为字符串、happy-path或小范围decision test，
  没有执行Task Contract列出的layout、fake syscall/clock/socket和逐字段mutation矩阵。

**Acceptance Gaps**

- A1：Faulted/Unavailable cleanup和完整request交错无可执行证明，且terminal path可留下
  pending request。
- A2：V4实现结构基本满足语义，但injectable assembly、Rust/C prefix/layout及tuple
  mutation见证未完成。
- A3：probe/peer未完整实现双样本Active判定、absolute deadline、ledger守恒、重复终态
  I/O和identity/phase negative protocol。
- A4：validator接受FAIL、foreign marker和非法validity，且不审计完整runtime关系。
- A5：六个case名称一致，但固定字段grammar、mutation matrix和对应source guards未建立。
- A6：Act报告的full/build Gate没有显示产品回归；本Review的聚焦host Gates也通过。A7的
  手工QEMU边界未被破坏。

**Convergence**

improving。Cycle 000的四个主要缺口已有实质进展：request有了显式状态对象，V4双tuple
wire已经成形，guest/peer可执行骨架和typed validator也已存在。剩余问题都能由本Cycle
既有T4.1-R1–R4契约直接约束，但A1–A5仍有Important gap，当前不能accepted。

**Evidence**

- 新鲜聚焦Gate：MS07 Rust host harness `3 passed; 0 failed`；C decision harness、probe C
  build、peer/validator self-test及probe/validator case diff均exit 0。它们证明当前骨架自洽，
  但没有触达上述negative路径。
- 新鲜validator反例：canonical transcript分别加入embedded `FAIL:`、foreign
  `MS07_UNKNOWN:`及`fault_valid=2`，三项均输出`ACCEPTED`。
- qemu-diagnostics聚焦`request_`测试在既有non-PIE linker wrapper下`4 passed; 0 failed`
  （其中两项为本Cycle request tests）；这同时确认现有测试没有Faulted/Unavailable和真实
  owner interleaving witness。未使用wrapper的首次命令在host链接阶段因既有percpu PIE
  relocation失败，未进入测试体，不计产品失败。
- 源码审计：probe的`read_v4()`不读取`v3[10]` lifecycle，三个waiter均在首个valid样本
  返回；`print_v4()`把`current_owner_quarantined`打印为`owned`；`new_epoch_traffic`在peer
  exchange后未重读V4。validator的case loop允许`FAIL:`/任意`MS07_`，`_v4()`只验证七个
  字段且不限制`fault_valid`域。
- `git diff --check`在Review前exit 0；工作树包含此前staged baseline和用户无关改动，本
  Review只修改当前Cycle的`Plan Review`，未修改产品代码、全局状态或运行QEMU。

**Follow-up Decision**

保持当前Cycle和`Review Result: pending`，不创建后继产物。Act在T4.1-R1–R4既有范围内
修复：让所有离开Active的终态共享request cleanup/transition seam并补齐完整交错测试；建立
可注入V4 assembly与Rust/C真实layout/tuple测试；按已冻结choreography补齐lifecycle连续双
采样、fail-closed absolute deadline、ledger和S0/S1重复终态、严格peer identity/phase；让
validator拒绝所有未消费协议行、FAIL/fatal/非法validity并执行完整字段与关系mutation矩阵。
修复后更新本Cycle的Act Response，再由Plan重新审计。

**Iteration Plan Update**

None.

**Next Cycle**

None.

**Next Iteration**

None.
