# Iteration 006 / Cycle 002: Close recovery protocol race windows

## Plan Context

- Status: ready
- Iteration: 006-recovery-probe-and-validator
- Cycle: 002-rework
- Cycle Type: rework
- Parent cycle: `001-rework.md`

**Iteration Scope**

- Change tasks: 4.1
- Depends on: Iteration 005；Cycle 001 已形成的 reset request state、V4 双 tuple、guest/peer
  payload 和 typed validator。
- Stable baseline: append-only recovery ABI、guest marker、peer protocol和纯输出validator形成
  可冻结、可负向验证的单hart QEMU资格协议。
- Verification boundary: request交错模型、Rust/C wire布局、C/Python协议mutation、MS03–MS07
  host seams、axnet串行全量与kernel build。
- Diagnostic boundary: request gate/lifecycle transition、V4实际Rust类型、guest绝对deadline与连续
  observation、peer identity/phase、validator首个协议差异。
- Deferred tasks: 4.2

**Cycle Scope**

- Trigger: rework-required
- Acceptance gaps: A1 request cleanup与terminal CAS之间仍有竞态；A2缺实际Rust V4布局见证；
  A3稳定采样、绝对I/O deadline和peer identity/phase未闭合；A4/A5 parser仍接受无效状态、
  marker乱序和部分fatal/noise边界。
- Repair items: T4.1-R5、T4.1-R6、T4.1-R7、T4.1-R8
- Inherited scope: Task 4.1、R4/R5/R6/R8、D8、A1–A7，以及Cycle 001全部preserve、
  forbidden、手工QEMU边界和72-u64 V3 prefix/15-u64 V4 tail。
- Excluded scope: recovery算法、deadline语义、socket terminal映射、自动QEMU/HMP、真实runtime
  资格、MS01/MS04/MS05/MS06 guest回归和Task 4.2。

**Objective**

关闭Cycle 001剩余的可接受窗口：terminal transition与request cleanup真正共享一把gate；
实际Rust V4类型有逐字段布局证据；probe/peer的每个判定受连续样本、同一绝对deadline和固定
identity约束；validator只接受唯一有序协议并拒绝所有单点mutation。

**Background**

Cycle 001第二次Act补齐了大部分协议字段和happy path，但Review发现request mutex在
`clear_for_recovery()`后、lifecycle CAS前已释放，原A1竞态仍存在。新增request测试只设置
“CAS前已有pending”，没有制造“clear后、CAS前提交”的交错，并在默认并行执行时因共享全局
request state互相污染而失败。Probe和validator也仍能把非连续Active样本、相对socket等待、
错误peer phase或乱序marker当作成功。缺口仍属于Task 4.1既有Acceptance，不修改Iteration
目标或验证边界。

**Current Baseline**

- Revision `05528313`，branch `net-k3`；Cycle 001实现作为staged工作树，未提交。
- `enter_recovery()`已在持有`RECOVERY_RESET_REQUEST` guard时执行clear与
  `begin_recovery()`；`enter_drift_quarantine()`和`transition_fatal()`却用临时guard清理后
  再执行`recover_fault()`/`fatal()`，中间仍可接受一个Active request。
- `recovery_snapshot_v4_from(ServiceAccess)`已提供current/fault可注入tuple见证；C wire有
  offset 72、80与87-u64 size assert。实际`IrqSnapshotV4`尚无Rust size/逐tail offset test。
- Probe已读取`v3[10]` lifecycle、打印完整V4字段并增加overall/operator常量；snapshot waiter
  只在Active样本更新previous，但遇到中间非Active样本不会清除previous。pre-reset与
  new-epoch drained marker仍基于单次read，`peer_exchange`/`expect_terminal`继续各自使用
  30秒相对poll timeout。
- Peer支持可选`--expected-run`，但默认未固定run；它接受没有网络交换语义的
  `reset_request`/`old_socket_terminal` phase，并把foreign address当作新合法peer。
- Validator已拒绝embedded FAIL、unknown MS07 marker和非法validity域，但不强制marker内部
  顺序；pre current_valid=0、terminal/link阶段非零owner ledger、大小写变化的fatal仍可通过。
  Probe的`hmp_link_up`当前输出`READY -> V4 -> OBSERVED -> SOCKET -> PEER`，validator
  canonical使用`READY -> OBSERVED -> V4 -> SOCKET -> PEER`，parser会接受这两种顺序。

**Current-State Evidence**

- `async_rx.rs::{enter_recovery,enter_drift_quarantine,transition_fatal}`：只有
  `enter_recovery`把clear与lifecycle transition放在同一request guard词法作用域。
- 聚焦request测试默认并行运行：`pending_request_absorbed`为1 passed/1 failed，失败是第二
  test在`request(...).unwrap()`收到`ResourceBusy`；`--test-threads=1`时2/2通过，证明见证
  依赖共享全局顺序且未覆盖transition窗口。
- `ms03-irq-host-harness.rs`直接导入`virtio_net_irq_logic.rs`，已对V1–V3使用
  `size_of/offset_of`；可直接增加V4实际类型测试，无需在no_std kernel内建测试。
- `ms07_recovery_probe.c::next_stable_observation`遇到lifecycle 5/6/7直接返回但保留
  `have_previous`；`run_probe`的pre/drained read和所有socket helper仍没有absolute deadline
  参数。
- Validator反例仍被接受：pre `current_valid=0`、`MS07_HMP_OBSERVED`先于READY、
  `KERNEL PANIC`；peer默认ledger接受wrong run、`reset_request`和`old_socket_terminal`。
- Cycle 001的Rust/C/Python happy-path host Gates通过；本Cycle不得把这些结果当成新修复证据。

**Relevant Code**

- `crates/axnet/src/async_rx.rs`
- `kernel/src/drivers/virtio_net_irq_logic.rs`、`virtio_net_irq.rs`
- `tests/ms03-irq-host-harness.rs`、`tests/ms07-recovery-host-harness.rs`
- `tests/ms07_recovery_probe.c`、`tests/ms07_recovery_probe_test.c`
- `scripts/ms07-recovery-peer.py`、`scripts/ms07-qemu-validate.py`
- `Makefile`

**Critical Path**

1. Request提交与任何`Active -> Quiescing/Faulted`决定都通过同一request gate；guard释放后
   lifecycle已经不是Active，新请求只能失败且不能留下pending。
2. Rust host harness直接实例化实际`IrqSnapshotV4`，证明V3 offset 0、V4 size 87 u64和
   tail字段72–86逐一对齐C wire。
3. Guest从pre开始只在连续两个目标Active/current样本后记录marker；任何中间非匹配样本
   清除稳定候选，Faulted和clock failure立即失败。
4. Overall、phase和operator形成checked absolute deadline；snapshot、peer、poll、terminal
   两次I/O都使用同一phase absolute deadline和remaining budget。
5. Peer固定expected run、允许且按序接受三个真实exchange phase，并拒绝foreign peer、
   wrong run/phase/sequence。
6. Validator按每个case的唯一marker序列消费协议；对全部raw lines大小写无关扫描fatal，
   同时容忍任意位置的非协议串口噪声。

**Implementation Guidance**

- 抽出所有离开Active路径复用的request-gated transition seam；测试用local request/lifecycle
  或带barrier的可注入对象制造clear/CAS竞争，不依赖production global的测试顺序。
- V4 Rust布局测试放入已经直接导入实际logic模块的MS03 harness；MS07 harness继续约束
  QEMU-only mapping/control seam，不用源码字符串代替实际size/offset。
- 稳定观察器必须在任何非目标样本后丢弃candidate。把clock/ioctl/poll/recv/send通过小型
  callback seam或纯decision state暴露给C host test，禁止仅用source guard证明deadline。
- Validator以每case ordered marker schema作为唯一authority；probe和Makefile导出的协议
  schema必须与其比较，而不只比较case名称。

**Ordered Marker Authority**

成功transcript只允许下列顺序。每个case由`MS07_CASE_START`开始、所列marker按序出现，随后
立即输出同名`PASS`；不得在列表中插入其他协议marker。

| Case | Ordered markers between START and PASS |
|---|---|
| `pre_reset_traffic` | `MS07_V4` → `MS07_PEER` |
| `reset_request` | `MS07_RESET` |
| `old_socket_terminal` | `MS07_V4` → `MS07_SOCKET` |
| `new_epoch_traffic` | `MS07_V4` → `MS07_SOCKET` → `MS07_PEER` |
| `hmp_link_down` | `MS07_HMP_READY` → `MS07_HMP_OBSERVED` → `MS07_V4` → `MS07_SOCKET` |
| `hmp_link_up` | `MS07_HMP_READY` → `MS07_HMP_OBSERVED` → `MS07_V4` → `MS07_SOCKET` → `MS07_PEER` |

完整成功顺序为`MS07_RECOVERY_START`、revision、environment、六个case、
`MS07_RECOVERY_END`、`MS07_HARNESS_EXIT: 0`。任一失败在当前case输出唯一`FAIL`并nonzero exit，
不得继续输出该case的`PASS`。非协议串口噪声可出现在任意协议项之间，但不改变上述协议投影。
Validator的schema/export是唯一authority；probe的schema export与Makefile必须逐项比较。

**Behavioral Change**

修复后，request不能在terminal cleanup与CAS之间重新进入；V4 wire由实际Rust/C类型共同
冻结；guest不会从被recovery样本隔开的两个Active observation、越过overall deadline的
socket wait或错误peer响应产生PASS；validator拒绝状态、ledger、顺序和fatal的单点漂移。

**Change Surface**

| Repair | Acceptance | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| T4.1-R5 | A1 | `async_rx.rs` terminal transitions/tests | 清理request后另行CAS | guard覆盖cleanup与CAS；独立交错见证 |
| T4.1-R6 | A2 | `ms03-irq-host-harness.rs` V4 ABI tests | 只实际测试V1–V3 | 测试V4 size、prefix和15个tail offset |
| T4.1-R7 | A3/A5 | C probe/test、peer、Makefile | 部分双采样和相对socket timeout | 连续样本、绝对I/O deadline、严格peer/schema |
| T4.1-R8 | A4/A5 | validator/self-test、Makefile | unordered marker set audit | ordered parser、fatal/noise和完整mutation |

**Task Contracts**

### T4.1-R5: Make terminal transitions request-atomic

- Requirement/Scenario: R5；A1；request在terminal cleanup/CAS前后提交。
- Depends on: Cycle 001 request state和resident owner seam。
- Targets: `async_rx.rs::{enter_recovery,enter_drift_quarantine,transition_fatal}`及request tests。
- Current behavior: drift/fatal路径的临时mutex guard在lifecycle CAS前释放；新tests依赖全局
  state并在默认并行执行时失败。
- Required behavior: 对每个离开Active的路径，cleanup/claim决定和lifecycle transition在同一
  request guard内线性化；guard释放后request读取非Active并拒绝，不留pending/claimed。
- Required changes: 复用单一gated transition seam；测试结束不污染global request；覆盖
  explicit claim、natural recovery、drift/fatal、event-before-poll、clear/CAS窗口和不可达
  Unavailable request。
- Preserve: syscall只提交+wake；resident owner唯一；Service guard不跨request wake/transition。
- Forbidden: syscall执行reset、清理后释放gate再CAS、只用串行test隐藏global污染。
- Test witness: 当前默认并行`pending_request_absorbed`稳定复现`ResourceBusy`；新增确定性
  barrier/model应在旧实现接受窗口request或留下pending，修复后拒绝且0/1次transition。
- GREEN condition: 聚焦tests默认并行与串行均通过；每个交错0或1次transition且state清洁。
- Verification: 两种feature聚焦/全量串行，另跑聚焦默认并行和重复隔离检查。
- Stop when: 必须引入第二owner或改变RxLifecycle外部语义，返回Plan。

### T4.1-R6: Execute the Rust/C V4 layout contract

- Requirement/Scenario: R4–R6；A2/A6；append-only ABI。
- Depends on: Cycle 001的87-u64 V4定义。
- Targets: `tests/ms03-irq-host-harness.rs`、MS07/C wire tests。
- Current behavior: C只assert tail起点/size，Rust MS07 harness只搜索mapping字符串；实际
  `IrqSnapshotV4`没有size或offset见证。
- Required behavior: 实际Rust类型的V3 prefix offset为0、size为87 u64，current/fault tail
  15字段逐一位于72–86；C struct逐字段对应且旧V1–V3测试保持不变。
- Required changes: 在MS03 harness增加actual type `size_of/align_of/offset_of`与prefix copy
  见证；C test补足逐字段offset或等价完整layout表。
- Preserve: V1 8、V2 28、V3 72 u64及ioctl编号/feature gate。
- Forbidden: replicated Rust shadow struct、source substring充当实际layout、修改旧ABI。
- Test witness: 当前harness对V4无test并产生“never constructed”warning；新断言在字段错位时RED。
- GREEN condition: actual Rust/C全部15字段一致；old ABI tests继续GREEN。
- Verification: MS03/MS07 Rust harness、C compile/test、kernel build。
- Stop when: 任一字段不能append-only表达，返回Plan。

### T4.1-R7: Bound every guest and peer decision

- Requirement/Scenario: R5/R6/R8；A3/A5；reset、new epoch、HMP off/on。
- Depends on: T4.1-R6 wire字段。
- Targets: C probe/test、peer self-test、Makefile protocol guards。
- Current behavior: 非Active样本不清stable candidate；pre/drained只读一次；socket helpers使用
  独立相对30秒poll；peer expected run可选并接受非exchange phase/foreign peer。
- Required behavior: 每个阶段marker来自连续两个目标Active/current样本；所有blocking I/O
  使用checked absolute remaining deadline；peer固定run、peer identity和
  `pre_reset_traffic -> new_epoch_traffic -> hmp_link_up` phase顺序。
- Required changes: 重置不连续candidate；为pre/reset/drain/link全部建立稳定观察；把同一
  deadline传入send/poll/recv和重复terminal；使overall/operator常量可同时成立；peer正常
  serve必须要求expected run并拒绝wrong phase/IP/sequence；probe按`Ordered Marker Authority`
  输出并导出完整schema。
- Preserve: 用户手工启动peer/QEMU/HMP；validator纯输出；不调用内部axnet progress。
- Forbidden: 忙等、相对timeout串联越过overall、自动HMP、用source grep代替fake clock/I/O。
- Test witness: fake sample序列`Active A -> Resetting -> Active A`必须RED；fake clock在两次
  terminal之间耗尽deadline必须失败；peer wrong run/phase/IP/order均不echo。
- GREEN condition: C/Python decision negative matrix全绿，static probe可交叉编译，schema一致。
- Verification: C syntax/test/static build、peer self-test、case/schema diff、source guards。
- Stop when: public syscall无法提供必要字段或需要自动控制QEMU，返回Plan。

### T4.1-R8: Enforce one ordered transcript

- Requirement/Scenario: R8；A4/A5/A7；raw serial audit。
- Depends on: T4.1-R7固定marker schema。
- Targets: validator parser/self-test、Makefile pure-auditor/grammar gates。
- Current behavior: parser按prefix集合取marker，不审计case内部顺序；pre current-invalid、部分
  owner drift、uppercase fatal可通过，case外串口噪声被拒绝。
- Required behavior: 唯一顺序parser验证metadata、每case marker顺序/字段/关系、fatal和exit；
  未知协议行失败，非协议串口噪声可在start前、case间和exit后存在；fatal扫描覆盖全部raw
  lines且大小写无关。
- Required changes: 按`Ordered Marker Authority`逐行消费；所有V4 marker要求current_valid=1、Active和
  对应owner约束；补pre/old/down/up、marker reorder、duplicate/missing、fatal case、noise
  placement、numeric overflow和全部tuple字段单点mutation。
- Preserve: 不访问网络/进程/QEMU，不修改输入；first-difference错误。
- Forbidden: unordered prefix set、只计PASS、忽略foreign MS07/FAIL、把noise当协议证据。
- Test witness: 当前反例pre invalid、observed-before-ready和uppercase panic均exit 0；修复后
  逐一非零，canonical与合法noise通过。
- GREEN condition: canonical/合法noise通过；全部单点mutation拒绝；probe schema完全一致。
- Verification: self-test、独立bad transcript CLI、pure source guard、host-test可执行部分。
- Stop when: parser需要读取运行环境或外部状态才能判断，返回Plan。

**Invariants**

- Request gate覆盖cleanup、claim与所有Active exit transition；请求不跨generation或terminal。
- V1–V3 ABI、V4 V3-prefix和QEMU-only控制面不变。
- 连续样本指原始相邻V4 reads；任何非目标read都会打断稳定性。
- 每个阻塞调用只消费传入的absolute deadline，不自行续期。
- S0永久ECONNRESET，S1在link down后永久ENOTCONN，S2只在link up后成功。
- Validator/peer不控制QEMU/HMP；真实runtime仍仅属于Iteration 007。

**Non-goals**

- 不运行QEMU，不采集raw serial，不声明qualification PASS。
- 不修改recovery stage、queue/socket/link epoch算法或terminal错误映射。
- 不新增SMP、PCI/DWMAC、真板、性能或自动runner范围。

**Acceptance**

- A1：request/natural/fatal所有确定性交错至多一次transition，无stale state；tests并行/串行隔离。
- A2：actual Rust与C V4 prefix/size/15 tail offsets一致；tuple语义与epoch 0/missing Service见证通过。
- A3：probe/peer实现连续样本、绝对deadline、ledger、永久terminal和严格identity/phase。
- A4：validator只接受唯一有序协议，拒绝全部状态、关系、fatal和字段mutation。
- A5：probe/peer/validator共享case与ordered grammar authority，pure/source guards通过。
- A6：MS03–MS07、axnet ordinary/qemu-diagnostics、kernel build不退化。
- A7：自动Gate只声明protocol ready；Iteration 007仍是唯一runtime资格边界。

**Verification**

1. R5默认并行RED、确定性clear/CAS交错RED，再运行并行/串行GREEN。
2. R6实际Rust V4 size/offset RED→GREEN及C逐字段layout测试。
3. R7 fake sample/clock/socket/peer mutation矩阵、C syntax与static RISC-V build。
4. R8 canonical、合法noise和每状态/字段/顺序/fatal single mutation。
5. MS03、MS04、MS07 Rust host harness及完整无sockethost Gate；`make host-test`若仅既有
   loopback EPERM则精确分层。
6. axnet ordinary/qemu-diagnostics串行全量与production checks；聚焦request另跑默认并行。
7. `make ARCH=riscv64 build`、相关format、Python syntax、`git diff --check`和strict validate。
8. 不运行QEMU。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | request窗口、test污染、sample/deadline、peer/parser false accept均有源码与新鲜反例 |
| Design | PASS | 同gate transition、actual ABI、absolute I/O、ordered schema责任已闭合 |
| Iteration Plan | PASS | 只关闭Task 4.1既有A1–A5；Task 4.2与Map不变 |
| Cycle Scope | PASS | R5–R8映射Cycle 001未收敛gap，不引入新成果 |
| Task Contracts | PASS | 每项含current/required、RED/GREEN、preserve/forbidden和stop |
| Traceability | PASS | Findings → A1–A5 → R5–R8 → Rust/C/Python/build Gates |
| Verification | PASS | 并发、ABI、fake I/O、protocol mutation与回归判据明确 |

Gate 2技术维度通过；用户于 2026-08-30 明确要求“更改gate状态，开始实施”，当前状态改为
`ready`并交给Act。

**Persisted Evidence**

- Mode: none

所有host结果可低成本重跑；真实runtime Evidence仍属于Iteration 007。

- Budget: 本Cycle最多5个文件（含README），整个change最多20个Evidence文件；单个文本文件
  最多500行且不超过256 KiB；超限需要用户明确批准。

**Risks and Notes**

- request gate不得与Service guard形成反向锁序；transition seam只包request state与lifecycle CAS。
- Peer的新socket可能改变UDP source port；peer identity应固定可稳定观察的host/IP部分，不把
  每个新socket端口误判为foreign。
- 合法串口噪声不能包含`MS07_`、`PASS:`或`FAIL:`前缀；这些始终属于协议并fail-closed。

## Act Response

- Status: reported

**Implemented**

这是 Cycle 002（T4.1-R5–R8）本轮修复后的完整当前终态，合并原始四项 repair item、上一版审计修复 pass，以及本轮按 Plan Review 最新两条重要 finding（A3 recv→send 边界、A5 link-down 维护基线）的修复。所有改动均有 RED→GREEN 见证。

1. **T4.1-R5（A1 request 原子性）**：`async_rx.rs` 抽出 `with_recovery_request_transition` 单一 gated transition seam，
   三个离开 Active 的终端路径——`enter_recovery`（自然恢复）、`enter_drift_quarantine`（ownership-drift quarantine）、
   `transition_fatal`（非法 Active→Faulted）——都在同一个 `RECOVERY_RESET_REQUEST` guard 内先 `clear_for_recovery()`
   再提交 lifecycle CAS。guard 释放后 lifecycle 已非 Active，新请求只能失败且不留 pending/claimed。syscall 侧
   `ctl.rs::NET_RECOVERY_RESET_REQUEST` 仍只提交 event + wake，不执行 reset。
2. **T4.1-R6（A2 actual Rust/C V4 ABI）**：`recovery_snapshot_v4_from(ServiceAccess)` 注入 seam 在一个 Service guard
   内读 current tuple（queue/socket/link/owner），`coherent_fault` 独立读，二者不再被 `fault.queue_epoch == 0` 混淆；
   missing Service 发布 `current_valid = 0` 且不伪造健康值。`read_v4_current` 与 `recovery_snapshot_v4_from` 同受
   `qemu-diagnostics` gate。ms03 harness 新测试 `snapshot_v4_preserves_v3_and_appends_the_fixed_recovery_tail`
   以实际 `IrqSnapshotV4` 的 `size_of/align_of/offset_of` 逐字段冻结：V3 prefix offset 0、size 87 u64、
   15 个 tail 字段恰好位于 72–86。
3. **T4.1-R7（A3/A5 guest/peer bounded decisions）**：C probe `next_stable_observation`/`ms07_stable_candidate_step`
   要求连续两个目标 Active/current 样本，任何中间非目标样本清除稳定候选；pre/reset/drain/link 全部稳定采样。
   `make_deadline`/`ms07_deadline_remaining`/`wait_fd` 让每个 blocking I/O 消费同一整体/阶段/operator 绝对 deadline；
   `peer_exchange` 携带 run/phase/seq；reset request 断言重复提交返回 EAGAIN。peer 固定 expected-run + guest IP、
   允许新 UDP source port、接受恰好 `pre_reset_traffic → new_epoch_traffic → hmp_link_up` 三阶段、拒绝 foreign peer/
   wrong phase/sequence。Probe 增加 `--print-schema` 导出与 validator 相同的 Ordered Marker Authority，Makefile
   `host-test` 增加 schema diff guard 与纯审计/纯决策 source guard。
4. **T4.1-R8（A4/A5 one ordered transcript）**：validator 改为唯一 ordered parser：`ORDERED_MARKERS` 按每个 case 的
   固定 marker 序列逐项消费；要求 revision/environment identity、`MS07_RECOVERY_START`/`END`/`MS07_HARNESS_EXIT`；
   对所有 raw lines 大小写无关扫描 fatal；容忍 case 间任意非协议串口噪声但协议行 fail-closed。self-test 覆盖
   missing identity、reordered case、marker 内顺序、per-field V4 mutation（lifecycle/current_valid/q/s/l/link/
   available/device_owned/quarantined/fault_*）、link-generation 单步/link state、conservation/drain 关系、
   HMP ready/observed 一致性、duplicate/missing marker、embedded/uppercase fatal、噪声位置。

**Audit-fix pass（上一版针对 Plan Review 的 A3/A4/A5 find）**

- **A3 peer select/latency**：`_serve_until_deadline` 在 `select_fn` 返回后复核 `now() < deadline`
   （不再只依赖进入循环时的检查），使 select 报 readable 时已过期的包不会产生 stale echo；新增对抗
   `LateReadableSelect`（把时钟从 100 推到 102、deadline=101）断言 `sent == 0`。
- **A3 C post-poll re-sample**：`wait_fd` 在 `poll` 返回后**重新采样时钟**再判定，保证在/过期后的 wake 按 stale
   处理（`ms07_wait_token_ok` 要求 `wake_ms < deadline_ms`），而不是用 `poll` 前的旧时钟误判成功；该超时规则与
   host-test 见证的 `ms07_wait_step` 共用同一决策。
- **A3 runtime-waiter witness**：`ms07_wait_step` 以可注入 fake clock 的纯决策暴露 polling 后的判断，
   `ms07_recovery_probe_test.c` 追加 late-at-deadline（`now==deadline` 拒绝）与 two-terminal-exhaustion
   （第一次 terminal 在预算内、第二次 terminal 在/过期后拒绝）两个场景，实际执行 runtime waiter 决策。
- **A4/A5 historical fault tuple**：validator `_fault_tuple` 对每个 session 的 six V4 marker 冻结同一
   coherent fault；仅单点把 `old_socket_terminal` 的 `fault_valid` 0→1（合法域内）即拒绝。Probe 侧
   `ms07_link_down_transition_valid` 与 `wait_for_link_up` 在 link flap 阶段冻结 `available`（link 不拥有/释放 slot）。
- **A4/A5 link owner ledger**：validator 对 `hmp_link_down/up` 分别断言 `available == pre.available`
   （down/up 单点把 `available` 64→63 即拒绝）；`new_epoch_traffic` 同样要求 `available` 守恒且 owner drained。

**本轮修复（针对 Plan Review 最新两条 finding）**

- **A3 peer recv→send 边界**：`_serve_until_deadline` 在 `recvfrom` 之后、`sendto` 之前**再次**复核
   `now() < deadline`；生产 listener 通过 `setblocking(False)` 置为 nonblocking，使任何 receive 都不能
   跨 absolute deadline 驻留。新增 `RecvCrossesDeadlineSocket`+`ReadableSelect` 对抗：select 在时钟 100 报
   readable、`recvfrom` 把时钟推到 102（deadline=101）时，旧实现仍会 echo，修复后断言 `sent == 0`。这是与
   `LateReadableSelect`（在 select 处跨期）不同的又一个跨期窗口（在 receive 本身处跨期）。
- **A3 C pre-syscall 边界**：新增纯决策 `ms07_io_allowed(now, deadline)`（即 `ms07_wait_token_ok`），在
   `wait_fd` 成功返回之后、`send`/`recv(MSG_DONTWAIT)` 之前作为最终 boundary 检查，接入 `peer_exchange`
   （send 前、recv 前各一次）与 `expect_terminal`（recv 前一次）。`ms07_recovery_probe_test.c` 增加
   fake-clock 见证：99 允许、100（=deadline）拒绝、101 拒绝、`deadline==now` 拒绝。这关闭了 `ms07_wait_step`
   只见证 poll 决策而不能覆盖 wait-return 与 syscall 之间调用边界的问题。
- **A5 link-down 维护基线**：`run_probe` 的 `wait_for_link_down` 改为传入 new-epoch 之后的 fresh drained
   observation（`&up`，`available==pre==64`），不再传 `reset` 快照（此时 `available=63` 仍含 reset 瞬态
   in-flight slot）。由于 `ms07_link_down_transition_valid` 要求 `before.available == after.available`，旧基线
   （63 vs 64）恒为 0、会把真实 link-down 拖到 deadline；新基线与 validator 的 `down/up.available == pre`
   守恒语义一致。`wait_for_link_up` 仍用 `reset` 作为 q/s epoch 基线，与 validator `up.s == old.s+1` 的
   关系判定一致，无需改。

**Changed Files and Symbols**

- `crates/axnet/src/async_rx.rs`（R5/R6 产品 + 测试）：`with_recovery_request_transition`、
  `enter_recovery/enter_drift_quarantine/transition_fatal` request-gated seam；`RecoverySnapshotV4`、
  `recovery_snapshot_v4`/`recovery_snapshot_v4_from`、`read_v4_current`（qemu-gated）；V4 current/fault
  tuple 分离；R5 测试 `explicit_request_is_absorbed...`、`request_claim_rejects_duplicate...`、
  `terminal_transition_holds_request_gate_through_lifecycle_commit`、`pending_request_absorbed_when_drift...`、
  `pending_request_absorbed_when_owner_ends_in_faulted...`；R6 测试 `v4_injected_seam_reads_current_and_fault_tuples...`、
  `v4_fault_epoch_zero_is_a_valid_historical_fault`、`v4_missing_service_is_current_invalid...`。
  并行隔离：三个 V4 测试 + 两个 pending-request 实测加入 `RECOVERY_REQUEST_TEST_LOCK`。
- `crates/axnet/src/wrapper.rs`（测试修复）：`fault_publishers_carry_captured_epoch_to_registry_commit` 由固定
  2200-char 窗口改为扫描到下一个顶层 `fn`，保留 captured-epoch 断言语义。R5 合法增长不再误触发该 source guard。
- `tests/ms03-irq-host-harness.rs`：新增 `snapshot_v4_preserves_v3_and_appends_the_fixed_recovery_tail`。
- `tests/ms07-recovery-host-harness.rs`：V4 wire/control、reset request owner、current/fault 分离 witness。
- `tests/ms07_recovery_probe.c` / `ms07_recovery_probe_test.c`：连续样本、绝对 deadline、稳定观察、
  Ordered Marker Authority 输出与 `--print-schema`；本轮新增 `ms07_io_allowed` 纯决策并接入
  `peer_exchange`/`expect_terminal` 的 send/recv 前最后 boundary；`wait_for_link_down` 改以 fresh `up` 为
  link-down 维护基线。测试新增 A3 I/O-boundary fake-clock 见证与 A5 `reset/fresh/down` available 关系见证；
  并保留原 late-at-deadline、two-terminal、drain 负矩阵。
- `scripts/ms07-recovery-peer.py`：`_serve_until_deadline` 在 select 后、recv/decode 后 send 前均复核
   deadline，生产 listener `setblocking(False)`；`RecvCrossesDeadlineSocket`+`ReadableSelect` 对抗 self-test；
  PeerLedger 固定 run/IP、三阶段顺序、`--expected-run` 必填。
- `scripts/ms07-qemu-validate.py`：ordered parser、REQUIRED_V4_FIELDS、FATAL_LINES、`--print-schema`、
  `_fault_tuple` 冻结、link-phase `available` 守恒、扩展 self-test（含合法域内单字段 mutation）。
- `Makefile`：`host-test` 增加 `--print-schema` diff 与 validator 纯审计/probe 纯决策 source guard。
- `openspec/specs/knowledge/spec.md`（K44）：把 `cc-nopie.sh` 的「一次性本地工具、不入库」更新为
  「已入库 `scripts/cc-nopie.sh`」（用户授权直接修改）。
- `kernel/src/syscall/fs/ctl.rs`、`kernel/src/drivers/virtio_net_irq{,_logic}.rs`：V4 snapshot（`0x4e49_4434`）
  与 reset request（`0x4e49_5231`）ioctl 路径（来自 Cycle 001 已 staged 基线，本 Cycle 未改）。

**Deviations from Plan**

无实质偏差。符合 Gate 的非实质记录：
- 本轮 A3/A5 修复落点与 Plan Review Follow-up 完全一致：peer listener 置 nonblocking + recv/decode 后
  send 前同一 absolute deadline 复核；C 侧每个 `MSG_DONTWAIT` send/recv 前最后 deadline 检查并用 fake
  clock/recv 推进见证跨界不发送；link-down 使用 new-epoch fresh drained observation 而非 reset 快照；
  新增 `reset.available != fresh.available == down.available` 见证。
- `wait_fd` 的 poll-后时钟 re-sample 与 `ms07_io_allowed` 的 pre-syscall 检查共同覆盖 wait-return 到
  syscall 的调用边界，保证 socket 调用有界（nonblocking 或无界阻塞被 deadline 拒绝）。
- 修复了 R5 对既有 Iteration 005 source-guard 测试的窗口回归：`wrapper.rs` 的固定 2200-char 窗口因
  `enter_drift_quarantine` 合法增长把 captured-epoch publish 调至 2222 字符；改为扫描到下一个顶层 `fn`，
  保留同一断言语义。
- `read_v4_current` 增加 `#[cfg(feature = "qemu-diagnostics")]`，消除非 qemu-diagnostics build 的 dead-code 警告。
- 对三个 Rust harness 应用 `rustfmt --edition 2024` 以消除格式漂移（纯布局，无语义变化）。
- `tests/ms07_recovery_probe` 为已 tracked 二进制工件（HEAD 已含）；本 Cycle 的 staged 更新反映重编译产物，
  不在产品 diff 中声明。
- `scripts/cc-nopie.sh` 已入库（commit `05528313`），不再是 `/tmp` 一次性工具；axnet host 测试以
  `RUSTFLAGS="-C linker=.../scripts/cc-nopie.sh"` 运行，并据用户授权同步更新 K44。

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: PASS
- Full diff reviewed: PASS
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 2

逐 repair item 与全量 diff 复核。A1：request/natural/fatal 交错在同一 guard 内线性化，无 stale state，
并行/串行隔离均通过。A2：actual Rust `IrqSnapshotV4` 与 C 逐字段一致（ms03 size/offset 测试 +
REQUIRED_V4_FIELDS + C probe wire）。A3：本次闭合；peer 在 select 后**且** recv/decode 后 send 前均复核
deadline（`LateReadableSelect` 与 `RecvCrossesDeadlineSocket` 两个跨期窗口对抗均拒绝 stale echo），生产
listener nonblocking；C 侧 `ms07_io_allowed` 在 `wait_fd` 成功返回后、每个 `MSG_DONTWAIT` send/recv 前
执行最后一次 deadline 检查（fake-clock 见证 99 允许 / 100、101 拒绝）。A4：validator 唯一有序 parser 拒绝
状态/关系/顺序/fatal/字段单点 mutation，fault tuple 冻结 + link `available` 守恒。A5：本次闭合；
`wait_for_link_down` 以 fresh `up`（available==pre==64）为基线，`(reset,down)` 因 63≠64 拒绝、
`(fresh,down)` 通过，与 validator 的 `down/up.available == pre` 语义一致。A6：axnet ordinary 467、
qemu-diagnostics 499、完整 `make host-test`、kernel build、strict validate 及 diff check 均为 exit 0。
A7：未运行 QEMU，Iteration 007 仍是唯一 runtime 资格边界。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| axnet ordinary 全量 | `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --test-threads=1`（RUSTFLAGS=cc-nopie linker） | `test result: ok. 467 passed; 0 failed`，exit 0 | PASS |
| axnet qemu-diagnostics 全量 | 同命令 + `--features qemu-diagnostics` | `test result: ok. 499 passed; 0 failed`，exit 0 | PASS |
| peer self-test | `python3 scripts/ms07-recovery-peer.py --self-test` | exit 0（含 `LateReadableSelect` 与 `RecvCrossesDeadlineSocket` 两个跨期对抗） | PASS |
| C probe 测试 | `cc -std=c11 -Wall -Wextra -Werror tests/ms07_recovery_probe_test.c` | exit 0（含 `ms07_io_allowed` fake-clock 与 A5 available 关系见证） | PASS |
| probe RISC-V | `riscv64-linux-musl-gcc -std=c11 -Wall -Wextra -Werror -o /tmp/ms07-probe-riscv tests/ms07_recovery_probe.c` | exit 0 | PASS |
| validator self-test | `python3 scripts/ms07-qemu-validate.py --self-test` | exit 0（canonical 通过、全 negative 拒） | PASS |
| host Gate | `make host-test` | exit 0（全部 Rust/C host harness、ms06/ms07 validator/peer/probe、schema/case diff、SOURCE_FREEZE 等负向 fixtures 全过） | PASS |
| kernel QEMU build | `make ARCH=riscv64 build` | `.bin` 生成，exit 0 | PASS |
| py 语法 | `python3 -m py_compile scripts/ms07-qemu-validate.py scripts/ms07-recovery-peer.py` | exit 0 | PASS |
| diff 白测 | `git diff --check` + `git diff --cached --check` | exit 0 | PASS |
| OpenSpec validate | `openspec validate ms07-qemu-single-hart-recovery-semantics --strict` | `Change ... is valid`，exit 0 | PASS |

上一版已有且本轮未触动的证据保持：R5 request 聚焦默认并行 11 passed、MS03 36/36、MS04 16/16、
MS07 3/3、rustfmt `--check` exit 0。本轮改动仅限 peer.py / probe.c / probe_test.c，未触碰 Rust 产品与
harness，故这些结果代表除新增三项纯 Python/C 决策之外仍成立的回归基线。

**Persisted Evidence**

None required. Plan Context `Persisted Evidence` 模式为 `none`；命令与决定性输出均可低成本重跑，Act Response 足以保存 Gate 结果。未创建 Evidence 目录。

**Experience Candidates**

None. axnet host 测试所需的 `cc-nopie.sh` wrapper 已库内提交（`scripts/cc-nopie.sh`，commit `05528313`），
不再是一次性 `/tmp` 工具，因此不再是环境前置候选。

**Remaining Issues**

无阻塞项。遗留 Minor：
- `tests/ms07_recovery_probe` 为已 tracked 二进制工件（HEAD 已含），本 Cycle staged 更新只是重编译产物；其内容
  安全校验依赖重建环境一致性，不影响产品源码 diff。
- 非 `qemu-diagnostics` 的 axnet lib build 仍有预先存在的 dead-code 警告（READ_BOUND/SUPPRESS/EXPLICIT_REQUEST/
  flush/register_waker/readiness 等），均存在于 HEAD，来自 feature-gated 诊断符号，不是本 Cycle 引入；`read_v4_current`
  这一条已由本次 gate 修复消除。

**Commit or Diff Reference**

Diff reference: `git diff HEAD`（工作树，Cycle 001 staged 基线 + Cycle 002 改动 + 本轮 A3/A5 修复混合；未提交）。
本 Cycle 变更跨 async_rx.rs、wrapper.rs、ms03/ms04/ms07 harnesses、probe/test、peer、validator、Makefile 及 K44。
本轮仅新增 peer.py、probe.c、probe_test.c 三处改动。
commit 未建（未获提交授权）。`scripts/cc-nopie.sh`（库内，HEAD `05528313`）与 `tests/ms07_recovery_probe`（tracked 二进制）
不在产品源 diff 声明内。

## Plan Review

- Review Result: accepted

**Findings**

无阻塞 finding。保留 Act Self-Review 中两个不影响 Acceptance 的 Minor：tracked C probe
二进制仍是可重建工件；ordinary axnet build 的既有 feature-gated dead-code warning 未扩大。

**Deviation Classification**

None.

**Acceptance Gaps**

None. A1–A6 均由当前实现和自动 Gate 闭合；A7 的真实 QEMU 资格仍按原计划属于
Iteration 007，本 Cycle 未越界声明 runtime PASS。

**Convergence**

complete。上一版剩余的 A3/A5 均已关闭：peer 在 select 和 recv 返回后重新检查同一 absolute
deadline，生产 listener 为 nonblocking；C 在每个 `MSG_DONTWAIT` send/recv 前重新采样；link-down
以 new-epoch fresh/drained observation 为 ledger 基线。

**Evidence**

- 新鲜独立通过：peer self-test、validator self-test、C probe test、Python compile、strict
  OpenSpec validate，均 exit 0。
- `make host-test` 中 MS03 36/36、MS04 16/16、MS07 3/3 及此前项目 harness 全部通过；随后在
  MS04 UDP loopback 创建 socket 时因 sandbox `EPERM` 中止（exit 2）。这是明确环境能力限制，
  不能覆盖已通过项，也不构成产品失败或完整 host Gate PASS。
- Act 的完整快照另报告两套 axnet 全量、kernel build、RISC-V static probe、schema diff、diff
  check 和 strict validate 均 exit 0；本次修复只触及 peer/probe/test，审计未发现证据与代码不符。
- 未运行 QEMU；没有把 host/model 结果提升为 runtime 资格结论。

**Follow-up Decision**

接受 Cycle 002，完成 Iteration 006。按既有 Iteration Map 仅展开下一 Iteration 的初始草案；
真实 QEMU、HMP 和 guest shell 操作仍须由用户手工执行。

**Iteration Plan Update**

None.

**Next Cycle**

None.

**Next Iteration**

`../007-single-hart-qemu-qualification/000-initial.md`
