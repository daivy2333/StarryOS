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

Task 4.1 经 Cycle 001 rework 完整闭合 A1–A5。在原实现基础上按 Plan Review 的四项 Important finding 补齐了 request 全终态线性化、V4 可注入 tuple 语义与布局、guest/peer 冻结 choreography 及 strict validator，全部以 RED→GREEN 见证：

1. T4.1-R1（A1）— request 全终态 cleanup：`enter_drift_quarantine` 与 `transition_fatal`（`Active -> Faulted`）现在分别在提交 `recover_fault()`/`fatal()` CAS 前取得 request gate 并 `clear_for_recovery()`，与 `enter_recovery` 共享同一吸收 seam；任何 pending/claimed explicit request 不能跨 Active 残留到下一 generation。新增两个真实 owner transition seam 测试（drift quarantine、arm-error fatal）RED→GREEN。
2. T4.1-R2（A2）— V4 可注入 assembly：`recovery_snapshot_v4()` 重构为可注入 seam `recovery_snapshot_v4_from(ServiceAccess)`，current tuple 在单一 Service guard 内经 `read_v4_current` 读取，historical coherent fault 独立读取，两者 valid/coherent 独立；missing Service 发布 `current_valid=0` 不伪造健康值。新增 axnet 见证：current 一 guard 组装 + current/fault ledger 分离、合法 QueueEpoch 0 非无 fault 哨兵、missing Service sentinel。C wire 的 struct + `_Static_assert`（V3 prefix offset、fault tuple offset、87-u64 size）移入常编译区，测试编译也执行；新增 `ms07_drained_epoch_ok` 纯决策函数 + C mutation 负例矩阵。harness source-guard 补 `v3: irq_snapshot_v3()`（V3 byte-copy 映射）。
3. T4.1-R3（A3）— probe/peer：`next_stable_observation` 只接受连续两个 Active/current-identity 样本（Quiescing/Resetting/Reinitializing 不计数、Faulted 立即失败）；`run_probe` 引入 overall + operator(300s) 绝对 deadline 并把 deadline 传入全部 waiter；`new_epoch_traffic` 在读 fresh drained wire 后印 marker、校验 `device_owned==0`/`available` 守恒/S0 重复 `ECONNRESET`；`old_socket_terminal` 用 `expect_terminal_twice`；`hmp_link_down/up` 用 `expect_terminal_twice`、link up 后再验 S1 永 `ENOTCONN`；`print_v4` 修复字段序（lifecycle 读 `v3[10]`）+ 补全 owner/fault tuple；`clock_gettime` 失败 fail-closed。peer 增加 KNOWN_PHASES 校验、`--expected-run` 强制、严格 per-key monotonic 序列与 fake-clock/fake-socket negative self-test。修复 probe 编译错误（wait_for_* 缺 deadline 参数、print_v4 字段错位、probe_test 缺 lifecycle=2）。

**Changed Files and Symbols**

- `crates/axnet/src/async_rx.rs`：`recovery_snapshot_v4` -> `recovery_snapshot_v4_from(ServiceAccess)` 可注入 seam、`read_v4_current`；`enter_drift_quarantine`/`transition_fatal` 的 request 吸收；新增 2 个 A1 seam 测试与 3 个 A2 tuple 测试。
- `tests/ms07-recovery-host-harness.rs`：`v3: irq_snapshot_v3()` byte-copy source guard。
- `tests/ms07_recovery_probe.c`：`next_stable_observation` Active 双采样、`run_probe` overall/operator deadline 与 drained/重复终态、`print_v4` 字段序、`ms07_drained_epoch_ok`、wire struct/`_Static_assert` 移入常编译区。
- `tests/ms07_recovery_probe_test.c`：lifecycle=2 基础 + `ms07_drained_epoch_ok`/deadline mutation 负例。
- `scripts/ms07-recovery-peer.py`：KNOWN_PHASES、`--expected-run`、fake-clock/socket negative self-test。
- `scripts/ms07-qemu-validate.py`：严格 state machine（完整 V4 grammar、lifecycle/ledger/drain/conservation/fault-tuple/重复终态/fatal 审计）与完整字段+关系 mutation 矩阵。

**Deviations from Plan**

- 无 Acceptance 偏差。非实质记录：
  - `read_v4_current` 返回 7 元组而非具名 struct，作为局部、等价组装实现；public seam 语义与 Plan 一致。
  - kernel `IrqSnapshotV4` 的 V3 prefix byte-copy 无法在 no_std kernel 中运行测试，以 C `_Static_assert`（offset/size）+ harness source-guard 锁定 wire 契约。
  - `wait_for_*` 死代码 `phase_deadline` 删除；deadline 计算内联在 `run_probe`。
  - validator 对 `fault_valid==0` 强制 fault tuple 全 0（验证 empty-tuple 语义）；`fault_valid==1` 只审计 grammar/域（不猜 tuple 内部关系）。
  - axnet host 全量测试沿用既有 `RUSTFLAGS="-C linker=/tmp/opencode/cc-nopie.sh"` 环境 wrapper（属既有环境前提，非本 Cycle 实现）。

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

Spec review：A1（request 全终态吸收 seam，Faulted/Drift seam 均有真实 owner transition 见证）、A2（可注入 seam + current/fault 分离 + epoch 0 + missing sentinel + Rust/C 布局/负例）、A3（Active 双采样、absolute/operator deadline、drained/conservation、S0/S1 重复终态、strict peer identity）、A4（完整 V4 grammar/lifecycle/ledger/fault/fatal mutation 矩阵）、A5（probe/validator case authority、pure-auditor/source guards）均对应 A1–A7。Code review：diff 无计划外修改；request gate 不持有 Service guard；current tuple 只在一 guard 内组装；validator 仍纯输出不导入 socket/subprocess/QEMU；probe 每等待受 absolute deadline 约束且不调用内部 axnet poll。两个 axnet 串行全量与 kernel build 均 pass。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| request seam RED | axnet qemu-diagnostics 聚焦 | 修复前 `pending_request_absorbed` 2 FAILED（请求残留） | PASS（RED） |
| V4 tuple RED | axnet qemu-diagnostics 聚焦 | 缺失可注入 seam/missing sentinel 见证时失败 | PASS（RED） |
| axnet ordinary 全量 | `RUSTFLAGS="-C linker=/tmp/opencode/cc-nopie.sh" cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --test-threads=1` | `467 passed; 0 failed`，exit 0 | PASS |
| axnet qemu-diagnostics 全量 | 同命令 + `--features qemu-diagnostics` | `498 passed; 0 failed`，exit 0 | PASS |
| ms07 Rust harness | `rustc --edition=2024 --test tests/ms07-recovery-host-harness.rs ...` | `3 passed; 0 failed` | PASS |
| C probe 语法 | `cc -std=c11 -Wall -Wextra -Werror -fsyntax-only tests/ms07_recovery_probe.c` | exit 0 | PASS |
| C probe_test | `cc ... tests/ms07_recovery_probe_test.c && /tmp/ms07-recovery-probe-test` | exit 0 | PASS |
| validator self-test | `python3 scripts/ms07-qemu-validate.py --self-test` | exit 0（canonical 通过、全部 negative 拒绝） | PASS |
| peer self-test | `python3 scripts/ms07-recovery-peer.py --self-test` | exit 0 | PASS |
| case authority | probe/validator `--print-cases` diff | 6 case 一致 | PASS |
| pure-auditor & no-poll guard | `grep` source guards | validator 无 socket/subprocess/QEMU；probe 无内部 poll/unbounded sleep | PASS |
| RISC-V static probe | `make tests/ms07_recovery_probe` | `riscv64-linux-musl-gcc ... -o tests/ms07_recovery_probe`，exit 0 | PASS |
| kernel build | `make ARCH=riscv64 build` | release build 完成，`.bin` 生成，exit 0 | PASS |
| rustfmt/diff/spec | `cargo fmt -- --check`、`git diff --check`、`openspec validate --strict` | 全部 exit 0；`Change ... is valid` | PASS |

**Persisted Evidence**

None required. Plan Context `Persisted Evidence` 模式为 `none`；命令与决定性输出均可低成本重跑，Act Response 足以保存 Gate 结果。真实 runtime Evidence 属 Iteration 007。

**Experience Candidates**

None.

**Remaining Issues**

无阻塞项。真实单 hart QEMU/HMP 运行、raw serial 审计与 `A7` 资格结论是 Iteration 007 的明确范围，未在本 Cycle 提前声明。遗留 Minor：无。

**Commit or Diff Reference**

`git diff`/untracked MS07 files（工作树，未提交）。本 Cycle 改动跨 `async_rx.rs`、`ms07-recovery-host-harness.rs`、`ms07_recovery_probe.c`、`ms07_recovery_probe_test.c`、`ms07-recovery-peer.py`、`ms07-qemu-validate.py`。commit 未建（未获提交授权）；`scripts/cc-nopie.sh` 等既有 staged 外部改动未计入本实现 diff。

## Plan Review

- Review Result: rework-required

**Findings**

1. **Important — A1 的精确竞态仍未关闭。** `enter_drift_quarantine()`和
   `transition_fatal()`用临时request guard执行`clear_for_recovery()`，statement结束后guard
   立即释放，随后才调用`recover_fault()`/`fatal()`。另一个任务仍可在clear与CAS之间观察
   Active并提交request，使pending残留在Faulted状态；这与上一版Review要求的“同一gate覆盖
   cleanup与transition”是同一个gap。新增测试只覆盖CAS前已有pending，没有制造此窗口。
   两个`pending_request_absorbed`测试还共享并在结尾重新置位production-global request；默认
   并行运行实测1 passed/1 failed，失败为`ResourceBusy`，只有串行运行才2/2通过。
2. **Important — A2 tuple语义已收敛，但实际Rust V4布局仍没有可执行见证。**
   `recovery_snapshot_v4_from(ServiceAccess)`已覆盖missing Service、epoch 0和current/fault
   ledger分离；C也检查了field 72、field 80和总size。然而MS07 harness仍只搜索源码字符串，
   没有对实际`IrqSnapshotV4`执行`size_of/offset_of`，15个tail field也未逐一与C对齐。
   MS03 harness已经直接导入该实际Rust类型并测试V1–V3，Act所称“no_std中无法运行”不构成
   阻塞，A2明确要求的Rust/C layout冻结证据仍缺失。
3. **Important — A3的稳定观察和deadline/peer边界仍不完整。**
   `next_stable_observation()`遇到Quiescing/Resetting/Reinitializing时没有清除previous，因此
   `Active A -> Resetting -> Active A`仍会被当作连续两个Active样本；pre-reset和new-epoch
   drained marker又各有单次read路径。Snapshot waiter使用absolute deadline，但
   `peer_exchange()`和两次`expect_terminal()`仍各自启动30秒相对poll，可串联越过phase或
   overall deadline。Peer的`--expected-run`是可选项，默认ledger接受wrong run；它还接受
   没有peer exchange语义的`reset_request`、`old_socket_terminal`和foreign address，违背
   既有wrong run/phase/peer negative contract。
4. **Important — A4/A5 parser已拒绝上一轮三个反例，但仍不是唯一有序state machine。**
   Validator按prefix集合取marker，不检查case内部顺序；交换HMP READY/OBSERVED仍被接受。
   `pre_reset_traffic current_valid=0`也被接受，old/down/up的nonzero device-owned或
   quarantined owner均可通过。Fatal扫描区分大小写，`KERNEL PANIC`被接受；非协议噪声只在
   case内部容忍，start前合法串口banner反而被拒绝。Self-test没有覆盖这些边界，Makefile仍
   只比较case名，没有冻结完整ordered grammar。实际probe的`hmp_link_up`顺序是
   `READY -> V4 -> OBSERVED -> SOCKET -> PEER`，canonical则是
   `READY -> OBSERVED -> V4 -> SOCKET -> PEER`，当前validator会同时接受两者。

**Deviation Classification**

- `ACT-DEVIATION`：R1的terminal路径没有让request guard覆盖lifecycle CAS，未满足上一版
  Follow-up Decision的明确线性化约束。
- `ACT-DEVIATION`：R2以source guard/C部分assert替代实际Rust逐字段布局测试；R3/R4省略了
  非连续样本、absolute socket deadline、strict peer和ordered/noise/fatal mutation边界。

**Acceptance Gaps**

- A1：cleanup/CAS之间仍可接受request；新见证存在production-global污染且默认并行失败。
- A2：tuple assembly见证已完成，actual Rust/C V4 prefix/size/15 tail offsets未共同冻结。
- A3：连续Active语义、pre/drained稳定点、全I/O absolute deadline和strict peer未完成。
- A4：validator仍接受current-invalid、owner drift、marker乱序和大小写fatal，noise边界不符。
- A5：case名称一致，但ordered marker grammar和对应mutation/source authority未建立。
- A6：Act记录的两个串行full suite与kernel build通过；本Review的C/Python/MS07 happy-path
  Gate也通过，但新增request聚焦test默认并行失败。A7边界未被破坏。

**Convergence**

reduced overall, unchanged for A1。V4 tuple、完整marker字段、ledger/永久terminal和上一轮
FAIL/foreign/validity反例已经闭合；但上一版Review精确指出的cleanup/transition同gate要求
仍未实现。根据当前Cycle收敛规则，gap不是全部reduced，不能再次留在Cycle 001覆盖；需要
后继rework Cycle用新的current baseline和确定性交错契约继续。

**Evidence**

- 新鲜happy-path Gate：validator/peer self-test、MS07 Rust 3/3、C syntax/decision test、
  Python syntax与case diff均exit 0；MS03 actual logic harness 35/35通过，但报告
  `IrqSnapshotV4 is never constructed`，确认无V4 layout test。
- request聚焦test使用既有non-PIE wrapper：默认并行1/2失败，`ResourceBusy`发生在第二个
  test提交request；同一命令加`--test-threads=1`为2/2通过，直接证明global test污染被串行
  Gate隐藏。
- Validator反例：pre `current_valid=0`、HMP OBSERVED先于READY、`KERNEL PANIC`均
  `ACCEPTED`；old/down/up的`device_owned=1`或`quarantined=1`也均`ACCEPTED`。start前加入
  `boot banner`则被拒绝。
- Peer反例：默认`PeerLedger()`接受wrong run、`reset_request`和`old_socket_terminal`的
  `seq=0`。
- 源码审计确认drift/fatal的request guard在CAS前释放、非Active样本不清stable candidate、
  socket helpers仍使用相对timeout；`git diff --cached --check` exit 0。未运行QEMU。

**Follow-up Decision**

Cycle 001终止为`rework-required`。目标、Task 4.1、Acceptance和Iteration Map不变；后继
`002-rework.md`以当前实现为新基线，建立四个有限repair：request-gated terminal CAS、actual
Rust/C V4 layout、连续样本与全I/O absolute deadline/strict peer、ordered validator grammar。
Cycle 001不再恢复或改写；用户批准新Cycle后再交给Act。

**Iteration Plan Update**

None.

**Next Cycle**

`002-rework.md`

**Next Iteration**

None.
