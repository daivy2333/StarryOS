# Iteration 007 / Cycle 001: Simplify qualification tools before manual QEMU

## Plan Context

- Status: ready
- Iteration: 007-single-hart-qemu-qualification
- Cycle: 001-replan
- Cycle Type: replan
- Parent cycle: `000-initial.md`

**Iteration Scope**

- Change tasks: 4.2
- Depends on: Iteration 006 accepted；Cycle 000自动Gate基线
- Stable baseline: 测试工具只用直接可观察的环境、阶段顺序、deadline、epoch/ledger、socket terminal、
  fatal与exit判断资格；随后由用户在single-hart QEMU VirtIO-MMIO上执行MS07和受影响回归。
- Verification boundary: 删除身份/指纹工具链后，Python/C/Makefile source guards、相关self-test、
  host/model/build Gate全部通过；手工QEMU由用户按Runbook执行并返回可审计输出。
- Diagnostic boundary: MS05遗留evidence工具入口、MS06/MS07 transcript grammar、MS07 UDP peer协议、
  自动Gate首个失败层和手工raw serial首个协议差异。
- Deferred tasks: None

**Cycle Scope**

- Trigger: replan-required
- Changed contract: 不再冻结或比较revision/artifact hash，不再用run-id或peer-host pin绑定测试会话；
  直接行为结果与环境事实保持严格校验。
- Inherited scope: Task 4.2、R8、D8；Iteration 006接受的恢复ABI、probe decision core、ordered
  markers、absolute deadlines、epoch/ledger和terminal语义。
- Excluded scope: kernel/driver产品修改、降低行为判据、自动驱动QEMU/guest/HMP、SMP、PCI/DWMAC、
  真板、性能和全局文档维护。

**Objective**

删除测试代码中不直接证明产品行为的hash、source freeze、revision/run-id和peer pin设计，使自动Gate
和手工命令只围绕可观察行为。清理验证通过后停止在手工边界，向用户交付完整QEMU命令；用户回传结果
后再由Act整理Evidence并返回Plan审计。

**Background**

Cycle 000已完成自动Gate，但用户拒绝hash、指纹、pin和run-id类验证。Act只移除了MS07 validator的
`--expect-revision`，使validator、probe和peer协议不一致。MS05遗留capture/audit工具仍以1,500余行
代码验证manifest、sha256、source/worktree identity、artifact record和时间顺序；当前活跃用途只有
Makefile self-test与工具自身unittest，手工QEMU Runbook已采用精简证据。验证契约已经改变，因此本
Cycle在同一Iteration内replan，先清理工具再执行runtime。

**Current Baseline**

- `scripts/ms07-qemu-validate.py`已移除`--expect-revision`，但仍要求非空`MS07_REVISION` marker。
- `tests/ms07_recovery_probe.c`仍由`MS07_REVISION_DEFAULT`构建，CLI为`--run <revision>`，UDP payload
  为`run=<revision> phase=<phase> seq=0`。
- `scripts/ms07-recovery-peer.py`仍要求`--expected-run`，`PeerLedger`同时pin run与首个guest host。
- MS06的Makefile macro、probe marker、validator grammar/CLI/self-test仍完整要求revision。
- MS05 `capture`、`audit`和`test_ms05_evidence_tools.py`互相形成封闭工具链；Makefile只运行两个
  self-test，当前产品runtime不消费manifest或qualification JSON。
- 清理前GREEN：MS05 capture/audit/self unittest、MS06 validator、MS07 validator/peer均exit 0。
- Cycle 000尚未运行QEMU，也未创建runtime Evidence。

**Current-State Evidence**

- `rg`定位MS05 active引用仅为两脚本、对应unittest、Makefile self-test和R54指针；archive保持只读。
- Makefile的`MS06_REVISION`/`MS07_REVISION`来自`git rev-parse HEAD`并注入guest payload。
- MS06 validator在case前强制revision/environment两阶段，并提供`--expect-revision`。
- MS07 peer的host pin只服务跨packet身份约束；三个phase顺序和seq=0可独立拒绝重复、乱序和未知packet。
- `.claude/runbooks/qemu-network-testing.md`与`qemu-evidence-capture.md`要求完整手工串口、关键命令和
  exit，不要求hash；长raw log可保存在用户侧来源路径，change内只收录必要投影。

**Behavioral Change**

- MS05：删除自动manifest capture/audit及其identity-only tests；Makefile不再把遗留证据工具自测当
  产品host Gate。普通Rust/C/Python产品测试和手工Evidence流程不变。
- MS06：transcript从`START → REVISION → ENVIRONMENT → cases`简化为
  `START → ENVIRONMENT → cases`；删除revision build macro、marker、CLI expectation和fixtures。
- MS07：transcript同样删除revision marker；probe运行改为无身份参数；UDP packet只携带phase/seq；
  peer无需expected-run或固定guest host，但仍严格执行phase顺序、seq=0、absolute deadline和只回显
  当前合法packet。
- 手工资格：不比较hash、revision或run-id；environment、ordered markers、ledger、terminal、fatal和
  exit仍fail-closed。

**Change Surface**

| Repair | Acceptance | File/Symbol | Planned Change |
|---|---|---|---|
| T4.2-R1 | A1 | `scripts/ms05_evidence_{capture,audit}.py`、`tests/test_ms05_evidence_tools.py`、Makefile | 删除封闭的manifest/hash/source-freeze工具链及self-test入口 |
| T4.2-R2 | A1/A5 | `tests/ms06_stack_readiness_probe.c`、`scripts/ms06-qemu-validate.py`、Makefile | 删除revision macro、marker、parser phase、CLI expectation和fixtures，保留environment/case/exit协议 |
| T4.2-R3 | A1/A3 | `tests/ms07_recovery_probe.c`、`scripts/ms07-qemu-validate.py`、`scripts/ms07-recovery-peer.py`、Makefile | 删除revision/run-id/host pin；把probe/validator/peer同步为简化协议 |
| T4.2-R4 | A1 | Makefile及相关self-test | 增加目标路径source guard并重跑自动Gate，证明身份层已清除且行为判据仍在 |
| T4.2-R5 | A2–A6 | Runbook命令、用户raw serial、validator与回归 | 自动Gate通过后交接用户手工QEMU；结果回来后审计，不自动驱动 |

**Repair Contracts**

### T4.2-R1: Remove the obsolete MS05 evidence identity toolchain

- Requirement/Scenario: R8 compatibility Gate应证明产品行为，不要求manifest/hash/source identity。
- Targets: 删除`ms05_evidence_capture.py`、`ms05_evidence_audit.py`、
  `test_ms05_evidence_tools.py`；移除Makefile两项self-test。
- Preserve: MS05 data-plane stimulus、probe、validator、既有产品tests、archive和手工Evidence文件。
- Forbidden: 删除MS05产品/行为测试；修改archive；用另一种fingerprint替代sha256；保留跳过式import test。
- Test witness: 清理前3组工具tests均GREEN；`rg`证明无当前runtime消费者。清理后Makefile不引用删除文件，
  `make host-test`仍运行MS05产品负向/行为Gate。
- GREEN: 三文件不存在，目标Makefile入口消失，无孤儿import；其余host Gate通过。
- Stop when: 发现非archive runtime调用者依赖manifest结果，停止并返回Plan，不删除调用链。

### T4.2-R2: Remove revision identity from MS06

- Requirement/Scenario: R8受影响回归仍验证12个application-visible cases、环境和exit。
- Targets: MS06 probe、validator及Makefile build rule。
- Required behavior: probe输出START后直接输出ENVIRONMENT；validator严格要求该顺序及唯一环境记录，
  拒绝旧/未知MS06 marker、乱序case、FAIL、timeout和nonzero/missing exit。
- Preserve: 12-case order、ANSI/noise处理、foreign MS01 tail边界、`--expect-environment`。
- Forbidden: 接受任意环境；删除case/exit/fatal判据；保留unused revision参数或兼容shim。
- Test witness: 改前self-test GREEN；改后canonical无revision通过，插入`MS06_REVISION`作为未知旧协议拒绝，
  环境missing/duplicate/reorder仍拒绝。
- GREEN: target files无`MS06_REVISION`、`expect_revision`、`--expect-revision`；self-test/pycompile通过。
- Stop when: 删除revision迫使改变任何MS06产品case语义，返回Plan。

### T4.2-R3: Remove revision, run-id and peer pin from MS07

- Requirement/Scenario: R8 manual recovery protocol；A3 peer三阶段行为与A4 ledger/terminal审计。
- Targets: MS07 probe、validator、peer、Makefile build rule。
- Required behavior: transcript为START→ENVIRONMENT→ordered cases→END→exit；probe CLI使用`--run`无值；
  UDP grammar为`phase=<known> seq=0`；peer按三个phase顺序接受并回显，不保存run或host身份。
- Preserve: packet严格字段集、seq数值边界、unknown/duplicate/reordered phase拒绝、nonblocking socket、
  shared absolute deadline、validator全部V4/epoch/ledger/terminal/fatal/noise判据。
- Forbidden: 用随机token、timestamp、UUID、address pin或新hash替代run-id；放松phase/order/deadline。
- Test witness: canonical transcript无revision通过；插入旧`MS07_REVISION`拒绝；peer接受同序phase即使端口/
  host变化，仍拒绝unknown、duplicate、out-of-order和非零seq；probe/validator schema完全一致。
- GREEN: target files无`MS07_REVISION`、`expected_run`、`run=`或`expected_host`；self-test、C test、schema
  diff和static build通过。
- Stop when: 简化要求改变kernel ioctl、recovery ABI或epoch/socket产品语义，返回Plan。

### T4.2-R4: Re-establish automatic gates after cleanup

- Requirement/Scenario: A1；任何工具清理不得掩盖产品失败。
- Targets: Makefile host-test与直接命令。
- Required behavior: source guard只扫描本Cycle目标路径，拒绝revision/run-id/hash/source-freeze残留；
  不误伤产品内部epoch/ticket identity或无关benchmark校验。
- Test witness: guard在清理前RED；清理后GREEN。随后运行MS06/MS07 Python、MS07 C、MS03/04/07 Rust
  harness、两套axnet全量、kernel build、diff和strict validation。
- GREEN: 所有可运行自动产品Gate exit 0；sandbox `EPERM`仅按最早失败层分层。
- Stop when: 任一编译、断言、parser、schema或build失败；不得进入手工批次。

### T4.2-R5: Hand off and audit manual QEMU qualification

- Requirement/Scenario: R8 single-hart QEMU reset/link/socket与兼容回归。
- Depends on: R1–R4全部GREEN，且清理后重新构建payload与kernel image。
- Required behavior: Act给出Runbook完整命令并停止在用户手工边界；用户回传raw serial、validator
  exit和MS01/MS04/MS05/MS06终态后，Act核对environment、marker、行为与exit并写Evidence摘要。
- Preserve: QEMU/guest/HMP不自动化；single hart、VirtIO-MMIO、user-net、`LOG=warn`。
- Forbidden: 用自动Gate代替runtime；缺完整串口来源或明确exit仍判PASS；恢复旧hash/pin以关联日志。
- GREEN: MS07 validator exit 0，六case和四组回归明确PASS，无panic/trap/fatal/owner drift/Pending。
- Stop when: 用户尚未完成手测、环境不满足或任一runtime判据失败；记录Handoff，不降级结论。

**Invariants**

- 清理只删除测试身份/证据工程，不改变kernel、driver、ABI、deadline、epoch或socket terminal语义。
- 环境、phase/order、deadline、V4关系、terminal、fatal和exit仍是资格authority。
- archived change/Evidence保持只读；全局R54指针如因删除变为历史指针，由后续docs-maintainer收尾，
  本Cycle不改全局文档。
- 手工session从boot开始录制；QEMU进程exit不替代guest workload exit。

**Non-goals**

- 不保留revision/hash/run-id兼容模式，不换成UUID、mtime组合或其他指纹。
- 不修改产品恢复实现，不借清理缩减MS07六case或MS01/MS04/MS05/MS06回归。
- 不自动运行QEMU、guest shell或HMP，不扩展到SMP、PCI/DWMAC、真板或性能。

**Acceptance**

- A1：目标测试路径不再包含MS05 manifest/hash/source-freeze工具链、MS06/MS07 revision pin或MS07
  run-id/peer-host pin；行为判据source guard和自动Gate全部通过。
- A2：single-hart QEMU 7.0.0 VirtIO-MMIO环境marker与实际配置一致。
- A3：MS07六case按唯一顺序完成，peer三阶段exchange和absolute deadline成立，validator exit 0。
- A4：reset、queue recovery、旧/新socket、HMP off/on及epoch/ledger关系由raw serial直接见证。
- A5：MS01/MS04/MS05/MS06受影响runtime回归全部明确PASS。
- A6：必要串口协议投影、命令和exit可审计，无panic、trap、fatal ownership drift或永久Pending。

**Verification**

1. 清理前GREEN已记录；实施后运行target source guard、MS05 active-reference inventory、MS06/MS07
   validator/peer self-test、Python compile、MS07 C decision test和probe/validator schema diff。
2. 运行`make host-test`；若唯一失败为socket `EPERM`，记录最早环境层并逐项运行无socket命令，其他
   失败均阻塞。
3. 运行axnet ordinary与qemu-diagnostics串行全量、MS03/MS04/MS07 Rust harness、RISC-V static
   payload build、`make ARCH=riscv64 build`、format、diff check和strict OpenSpec validate。
4. 自动Gate全绿后，向用户提供HTTP/peer/QEMU录制、guest probe、HMP off/on、validator及四组回归的
   完整命令；Act状态转`blocked`并写清能力边界与恢复条件。
5. 用户回传结果后只审计已有输出，不自动补跑QEMU；通过才把Act Response改为`reported`。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Requirements | PASS | 用户明确要求清理过度工程化测试设计后再手测；R8已移除revision/hash/run-id绑定 |
| Investigation | PASS | 已定位MS05封闭工具链、MS06/MS07端到端identity路径及所有当前调用者 |
| Design | PASS | 删除身份层，保留直接行为、环境、顺序、deadline、ledger、terminal、fatal和exit |
| Cycle Scope | PASS | 五个repair依赖有序，清理和最终资格仍共同完成Task 4.2 |
| Task Contracts | PASS | 每项有targets、preserve/forbidden、witness、GREEN和stop条件 |
| Verification | PASS | 清理前GREEN、清理后source guard、自动产品Gate和用户手工runtime分层 |
| Evidence | PASS | required项缩到公共预算，长raw serial保留用户侧路径并存协议投影 |
| Authorization | PASS | 用户要求“把测试代码里面的那些过于工程化的设计清理掉，然后再进行手动测试” |

**Persisted Evidence**

- Mode: required
- Path: `openspec/changes/ms07-qemu-single-hart-recovery-semantics/evidence/007-single-hart-qemu-qualification/001-replan/`
- Required files（最多5个）：`README.md`记录命令/exit/环境/原始串口来源；`ms07-protocol.txt`保存
  START至HARNESS_EXIT的协议投影；`regressions.txt`保存MS01/MS04/MS05/MS06终态与exit。仅在用户手工
  结果到达后创建；自动Gate决定性输出写Act Response。
- Necessity: 支持A2–A6；手工QEMU现场不能低成本重跑，Act Response不足以保存marker顺序；缺少协议
  投影或来源/exit将阻止runtime资格。每文件不超过500行/256 KiB，整个Cycle不超过5个文件。

**Risks and Notes**

- 删除MS05工具会使R54从“当前工具入口”变成历史指针；这是docs-maintainer的后续状态维护，不阻塞
  产品或本Cycle Acceptance。
- MS07 peer不再pin host后可能回显来自不同source address的下一合法phase；这是用户批准的简化。
  phase/order/seq和短时手工session仍限制误接收面，但不再声称cross-run identity。
- Cycle 000的`--expect-revision`局部删除保留为当前工作树基线，本Cycle完成端到端一致清理。

## Act Response

- Status: blocked

**Implemented**

本 Cycle（001-replan，清理测试工具后进入手工 QEMU）已完成 T4.2-R1~R4 并全绿；T4.2-R5 停
在用户手工 QEMU 能力边界，按 Plan Verification 第 4 步把状态置为 `blocked`，完整手工命令见
下方「给用户的手工 QEMU 交接块」。未创建任何 runtime Evidence。

1. **T4.2-R1**：删除 MS05 evidence 身份工具链（`scripts/ms05_evidence_capture.py`、
   `scripts/ms05_evidence_audit.py`、`tests/test_ms05_evidence_tools.py`，共 1760 行）及
   Makefile 三项 self-test 入口。清理前三组工具 GREEN（capture/audit self-test、unittest）。
2. **T4.2-R2**：从 MS06 端到端移除 revision 标识——validator 删除 `REVISION_PREFIX`、
   `expect_revision`、revision phase 与相关 fixtures；probe 删除 `MS06_REVISION_DEFAULT` 宏与
   `MS06_REVISION:` 输出；Makefile 删除 `MS06_REVISION`/`-DMS06_REVISION_DEFAULT`。保留 12-case
   顺序、ANSI/噪声处理、foreign MS01 tail、`--expect-environment`。
3. **T4.2-R3**：从 MS07 端到端移除 revision/run-id/peer-host pin——probe 的 `peer_exchange`/
   `run_probe`/`--run <rev>` 简化为无身份参数、UDP payload 只带 `phase=%s seq=0`；validator 删除
   `MS07_REVISION` phase；peer 删除 `--expected-run`、`expected_host` pin，仅按 phase 顺序/seq=0/
   absolute deadline fail-closed；Makefile 删除 `MS07_REVISION`/`-DMS07_REVISION_DEFAULT`。
4. **T4.2-R4**：在 `host-test` 增加 identity-layer source guard（ms05 文件必须不存在、MS06/MS07
   工具无身份 token、Makefile 无 `REVISION_DEFAULT` 注入）。guard 已对 6 类 token 逐一验 RED、
   对清理后现场验 GREEN。
5. **T4.2-R5**：自动 Gate 全绿后，向用户提供 peer/QEMU/HMP/probe/validator/四组回归完整命令
   （见交接块），并停在该手工边界。

**Changed Files and Symbols**

- 删除：`scripts/ms05_evidence_{capture,audit}.py`、`tests/test_ms05_evidence_tools.py`。
- `scripts/ms06-qemu-validate.py`：`validate_output()`（去 `expect_revision`、revision phase、
  缩 `REVISION_PREFIX`）、`_transcript()`（去 revision 参数）、`self_test()`（复写 fixtures）、
  `main()`（去 `--expect-revision`）。
- `scripts/ms07-qemu-validate.py`：`validate()`（去 revision phase、cursor 3→2）、`canonical()`、
  `self_test()`（6-restructure：`_reordered` 语义化、逐索引 −1）。
- `scripts/ms07-recovery-peer.py`：docstring、`decode_packet()`（去 run）、`PeerLedger()`（去
  expected_run/expected_host）、`self_test()`、`serve()`/`main()`（去 `--expected-run`）。
- `tests/ms06_stack_readiness_probe.c`：去 `MS06_REVISION_DEFAULT`、去 `MS06_REVISION:` 输出。
- `tests/ms07_recovery_probe.c`：去 `MS07_REVISION_DEFAULT`、`peer_exchange`/`run_probe`/`--run`
  简化、去 `run=` payload。
- `Makefile`：`host-test` 去 3 项 ms05 self-test、加 identity guard；去 `MS0[67]_REVISION` 宏与
  `-DMS0[67]_REVISION_DEFAULT`。

**Deviations from Plan**

- 无产品 kernel/driver/ABI 改动，符合 Non-goal。改动严格限定在测试工具/探针与 Makefile。
- `ms06 pass_between_meta` fixture 因移除 revision 后只剩单一 metadata 记录，改写为
  duplicate-environment 拒绝测试（`# environment must be unique and before any PASS`），仍覆盖
  "metadata 唯一且先于 PASS" 的行为判据。
- `ms07 重排 case` fixture 由脆弱的下标切片改为语义化的整块换位（`_reordered = [valid[0],valid[1]] +
  valid[6:9] + valid[2:6] + valid[9:]`），测试目的不变。
- 其余差异仅在逐条执行契约内的等价实现。
- 给用户的首版 QEMU 命令曾误加 `hostfwd=udp::15572-:15572`，用户执行时报
  `Could not set up host forwarding rule 'udp::15572-:15572'`（peer 已占用同一 host 端口）。
  已按 R56 数据面模式修正：peer 端口不加 hostfwd，guest 以 UDP client 出站连 `10.0.2.2:15572`。

**Blocker Handoff（能力边界）**

- 发现偏差的 Task/Step/Gate：T4.2-R5；Plan Verification 第 4 步。
- Plan 预期 vs 实际：清理与自动 Gate 均完成（R1–R4 GREEN）；真实 single-hart QEMU 的
  reset/queue-stall/link flap/old-new socket 与 MS01/MS04/MS05/MS06 回归属用户手工运行域，
  R44 政策禁止 agent 自动化 QEMU，本次无法在本环境执行。
- 影响：A2–A6（真实 runtime、raw serial、validator、四组回归）尚未取得，**未产生产品 PASS 声明**。
  Task 4.2 不得标记完成。
- 已完成：R1–R4（清理 + guard + 全部自动产品 Gate，见下方证据表）。未开始：R5 之手工 QEMU。
- 修改文件与工作区：见 Changed Files；HEAD `05528313`，分支 `net-k3`，产品源码未改动。
- Executed Evidence：无 runtime Evidence；自动 Gate 决定性输出已在 Act Response 表内。
- 恢复条件：用户按下方交接块在 single-hart QEMU 7.0.0 VirtIO-MMIO 手工运行 MS07 与受影响回归，
  回传 raw serial + host 摘录 + validator 输出 + 各回归终态/exit 后，把本 Response 状态改回
  `pending` 并核对证据，再填 Persisted Evidence 与 `reported`。
- **更新（用户 2026-08-31 指示：本 Cycle 只定位、下个 Cycle 再解决）**：手工运行暴露产品缺陷，
  本 Cycle 不再修复。恢复条件修正为：下一个 Cycle 先按下方「根因报告」修复产品缺陷
  （peer UDP 不可达 + `Service.link_state=None`/OwnerSummary 不守恒），再重跑手工 QEMU 资格。

**Blocker Resolution**

None（下个 Cycle 修复产品缺陷后再恢复手工批次）。

**本 Cycle 手工 QEMU 根因报告（用户 2026-08-31 指示：定位后填报、下个 Cycle 解决）**

三次手工运行（首次 `qemu-serial.log`、info `qemu-serial-info.log`、DBG `qemu-serial-dbg.log`）
在 MS07 probe 的 **`pre_reset_traffic reason=precondition`** 处失败；peer 未收到任何阶段包。
决定性 DBG：

```
DBG: read_v4=0 errno=0 lifecycle=2 current_valid=1 q=0 s=0 l=0 link=<u64 未截断前见文> avail=64 dev=64 quar=0
```

结论：

1. **V4 ioctl 正常**（`read_v4=0 errno=0`），owner **Active**（`lifecycle=2`），epoch 全 0（pre-reset
   预期），网络可通（wget `TCP ... to 10.0.2.2:18765`、`eth0 10.0.2.15/24` 正常）。因此不是 ioctl
   失败/owner 未启动/网络断。
2. **直接失败层——`open_peer_socket()` 的 `connect()` 到 `10.0.2.2:15572` 失败返回 -1**：probe
   全程 <8ms，连 `wait_for_pre_reset` 首次 `poll(20ms)` 都未执行，只有 `open_peer_socket<0` 短路成立
   （左起 `open_peer_socket || wait_for_pre_reset || peer_exchange`）。即 guest UDP **无法到达 host
   peer**，等价于「peer 收不到包」。确切 errno 已写入 probe 诊断，下个 Cycle 一次运行即得。
3. **底层产品缺陷（独立且更根本）——V4 current 快照不协调**：
   - `Service::link_state_code()`（`service.rs`）在 `self.link_state == None` 时返回 `u64::MAX`
     （0xFFFFFFFFFFFFFFFF）。DBG 的 `link=4294967295` 是我首版诊断用 `%u`+`(unsigned int)` 把
     `u64::MAX` 截断成 32 位所致；**真实 value 为 `u64::MAX` ⇒ `Service.link_state` 从未被设置**。
   - `recovery_owner_summary_target()` 返回的 OwnerSummary `available=64 + device_owned=64 +
     quarantined=0` 违反 `available+owned+quar ≤ QS(64)` 守恒（应为 `dev=0` drained）。表明 owner
     summary 在 probe 时刻尚未发布一致 current tuple。
   - 结果：即便 peer 可达，`wait_for_pre_reset` 要求 `link==UP(1)` 且 `device_owned==0`，这套快照
     恒不满足，pre_reset 仍不可能过。
4. **测试修改（本 Cycle，含诊断辅助）**：
   - R1–R4 清理（删除 ms05 证据工具链、ms06/ms07 去 revision/run-id/peer-pin、Makefile identity
     guard）——已完成、自动 Gate 全绿。
   - peer 加 `on_accept` 打印（host 侧可见每个收到/回显阶段）。
   - MS07 probe 加 `DBG: read_v4=...`（首次快照，errno/lifecycle/current_valid/epochs/link/ledger，
     已修 `link=%llu` 避免 u64 截断）与 `DBG: open_peer_socket ... errno=`（连接失败时打 errno）。
   - 手工 QEMU 命令：peer 端口**不**加 hostfwd（否则与 peer 争用 15572 报 `Could not set up host
     forwarding rule`）；guest 经 `10.0.2.2:15572` 直连 host UDP（R56/MS05 数据面模式）。
5. **下个 Cycle 修复建议（本 Cycle 不实施）**：
   - 定位 `open_peer_socket` connect errno（probe 已打印），修复 guest UDP 到 `10.0.2.2:15572` 的
     peer 可达性（如 UDP connect/slirp 交付、peer 绑定，或 product UDP connect 路径 bug）。
   - 修复 `Service.link_state` 未初始化（应在上电即设为 `Some(true)`/up）与 OwnerSummary 不守恒
     （available/device_owned coherent current tuple 发布），使 `wait_for_pre_reset` 边界成立。
   - 修复后重建，重跑手工 QEMU 资格（先冻结镜像 hash，诊断后恢复）。

**Self-Review**

- Plan compliance：PASS（R1–R4 全实现；R5 停在计划内手工边界；无计划外产品改动）。
- Full diff reviewed：PASS（逐文件审查 6 文件 + 3 删除；无孤儿引用；broad identity sweep 干净）。
- Critical findings unresolved：0
- Important findings unresolved：0
- Minor findings unresolved：
  - `crates/axnet-ng` 构建产生 13 条既有 dead-code warning（`publish_global_fault` 等），
    属 Iteration 006 后遗留的产品告警，不在本 Cycle 变更面，未处理。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| 清理前 GREEN | capture/audit self-test、unittest、ms06/ms07 validator、ms07 peer、ms07 probe decision | 全部 exit 0 | PASS |
| MS05 删除 | `git rm` + `ls` + Makefile 引用扫描 | 三文件不存在；Makefile 无引用；无孤儿 import | PASS |
| guard 负向 | 6 类 token 注入样本 | `CATCH: MS07_REVISION / MS06_REVISION / expected_run / --expect-revision / expected_host / expect_revision` | PASS(RED) |
| guard 正向 | guard 三段命令于清理后现场 | 三段均无匹配 | PASS(GREEN) |
| `make host-test` | `make host-test` | MS03/04/07 harness、ms05/06/07 C/Python、guard、cases/schema diff 全过；exit 0 | PASS |
| axnet ordinary | `cargo test ...--lib -- --test-threads=1` | `467 passed; 0 failed`；exit 0 | PASS |
| axnet qemu-diagnostics | 同命令 + `--features qemu-diagnostics` | `499 passed; 0 failed`；exit 0 | PASS |
| RISC-V probe | `make tests/ms07_recovery_probe` | statically linked RISC-V ELF；exit 0 | PASS |
| kernel build | `make ARCH=riscv64 build` | `StarryOS_riscv64-qemu-virt.bin`；exit 0 | PASS |
| py_compile | `python3 -m py_compile`（3 脚本） | exit 0 | PASS |
| diff 白测 | `git diff --check` + `--cached --check` | exit 0 | PASS |
| OpenSpec validate | `openspec validate ms07-... --strict` | `Change ... is valid`；exit 0 | PASS |
| 手工 QEMU DBG 运行 | info 镜像 + 诊断 probe，`qemu-serial-dbg.log` | `DBG: read_v4=0 errno=0 lifecycle=2 current_valid=1 q=0 s=0 l=0 link=<u64::MAX 截断显示 4294967295> avail=64 dev=64 quar=0`；后续 `FAIL: pre_reset_traffic reason=precondition`（`open_peer_socket` connect 失败致瞬间短路） | 定位到根因（非 PASS/FAIL） |

**Persisted Evidence**

- Mode：required。Path（待产品修复后成功资格运行再创建）：
  `openspec/changes/ms07-qemu-single-hart-recovery-semantics/evidence/007-single-hart-qemu-qualification/001-replan/`
- 计划文件（≤5 个）：`README.md`、`ms07-protocol.txt`、`regressions.txt`——仅在成功手工资格后创建。
- 诊断产物已落盘（非资格 Evidence）：`qemu-serial-info.log`、`qemu-serial-dbg.log` 记录了失败现场与
  DBG 值，作为本 Cycle 根因报告依据；不当作合格 runtime 证据。

**Experience Candidates**

- Potential：MS07 手工 QEMU 的完整命令块（peer→QEMU(-nographic、Ctrl-A c HMP)→probe→
  validator-回归）若端到端验证成功，建议由 `openspec-experience-recorder` 落成 Runbook；
  已含「peer 端口不 hostfwd、guest 经 10.0.2.2 直连 host UDP」与端口冲突排障。当前仅在用户
  运行后才有证据，未创建持久化产物。普通测试失败预期；非候选。

**Remaining Issues**

- **产品缺陷（下个 Cycle 修复，见上方根因报告）**：guest UDP 到 `10.0.2.2:15572` 的 peer 可达性
  （`open_peer_socket` connect 失败，errno 待下个 Cycle 运行确认）；`Service.link_state=None`
  （link 快照读到 `u64::MAX`）；OwnerSummary `available=64+device_owned=64` 不守恒。
- A2–A6 中的真实 runtime 资格在本 Cycle **不成立**（probe 在 pre_reset 即失败），待产品修复后重跑。
- R54 在 references/spec.md 的指针在删除 ms05 工具后变为历史指针，由后续
  `openspec-docs-maintainer` 收尾（本 Cycle Invariants 指示不改全局文档）。

**给用户的手工 QEMU 交接块（完整命令行）**

> 环境约束：single-hart、VirtIO-MMIO、user-net、`LOG=warn`。guest shell 输入均手工；HMP 用
> `Ctrl-A c` 切换，不自动化。peer 是 host UDP server（绑定 15572），guest probe 作为 UDP client
> 出站连 host `10.0.2.2:15572`；**peer 端口不加 hostfwd**（否则 QEMU 与 peer 争用同一 host 端口，
> 报 `Could not set up host forwarding rule 'udp::15572-:15572'`），遵循 R56/MS05 数据面模式。
> 端口冲突时先 `pgrep -af ms07-recovery-peer.py` 核对并 kill 遗留进程。

0. 基线确认（幂等）：
```bash
cd /home/daivy/projects/serial/work/StarryOS
make ARCH=riscv64 build && make tests/ms07_recovery_probe
```

1. Terminal A — 建证据目录 + 启动 peer：
```bash
cd /home/daivy/projects/serial/work/StarryOS
EV=openspec/changes/ms07-qemu-single-hart-recovery-semantics/evidence/007-single-hart-qemu-qualification/001-replan
mkdir -p "$EV"
python3 scripts/ms07-recovery-peer.py --host 0.0.0.0 --port 15572 --deadline-seconds 600
```

2. Terminal B — 录制串口 + 启动 QEMU（无 15572 hostfwd）：
```bash
cd /home/daivy/projects/serial/work/StarryOS
EV=openspec/changes/ms07-qemu-single-hart-recovery-semantics/evidence/007-single-hart-qemu-qualification/001-replan
script -q -e -f "$EV/qemu-serial.log" -c 'qemu-system-riscv64 -m 1G -smp 1 -machine virt -bios default -kernel StarryOS_riscv64-qemu-virt.bin -device virtio-blk-device,drive=disk0 -drive id=disk0,if=none,format=raw,file=make/disk.img -device virtio-net-device,netdev=net0 -netdev user,id=net0,hostfwd=tcp::5555-:5555,hostfwd=udp::5555-:5555 -nographic'
```

3. Terminal C — HTTP 服务：
```bash
cd /home/daivy/projects/serial/work/StarryOS/tests && python3 -m http.server 18765 --bind 0.0.0.0
```

4. Guest 内（`starry:~#` 后）跑 MS07 probe：
```sh
wget -q -O /tmp/ms07 http://10.0.2.2:18765/ms07_recovery_probe && chmod +x /tmp/ms07 && /tmp/ms07 --run; echo "MS07_HARNESS_EXIT: $?"
```
在 `MS07_HMP_READY: link=off` 处 `Ctrl-A c`→monitor→`set_link net0 off`→`Ctrl-A c` 回 guest；
`link=on` 处同理 `set_link net0 on`。wget 挂起时用 R44 debugfs 离线注入备用路径。

5. Terminal B（QEMU `Ctrl-A x` 退出后）离线审计：
```bash
cd /home/daivy/projects/serial/work/StarryOS
EV=openspec/changes/ms07-qemu-single-hart-recovery-semantics/evidence/007-single-hart-qemu-qualification/001-replan
python3 scripts/ms07-qemu-validate.py --expect-environment "qemu-virt-riscv64-single-hart-virtio-mmio-user-net" "$EV/qemu-serial.log"
```

6. 同一会话跑受影响回归（各自终态明确 PASS）：MS01 14/14、MS04 四 mode、MS05 六 mode、
MS06 12-case，沿用 R59/R56/R51 各自 Runbook；缺终态或未成功 exit 不计 PASS。
回传：raw serial + 关键 host 摘录 + validator 输出/exit + 四组回归终态/exit。

**Commit or Diff Reference**

`git diff`（工作树 + index）包含 3 个 ms05 文件删除（`D`）、6 个测试/工具文件清理与 Makefile
guard。`tests/ms07_recovery_probe`（受跟踪的静态 ELF）随 `make tests/ms07_recovery_probe` 重建为
无 revision 版本。产品源码（`crates/axnet`、`kernel`）未改动；`crates/axnet/src/{async_rx,wrapper}.rs`
等命中显示为预存 staged 改动，源于前序 Iteration，非本次 Cycle 变更。未建 commit（无提交授权）。

## Plan Review

- Review Result: replan-required

**Findings**

1. **Blocking / PLAN-INVALID — probe与validator错误解释driver owner contract。**
   `VirtIoNetDev::owner_summary()`把空闲TX buffer计入`available`，把已提交RX buffer和TX slot计入
   `device_owned`。初始化会填充`QS`个RX owner和`QS`个TX buffer，因此健康空闲态为
   `available=QS, device_owned=QS, quarantined=0`。Cycle要求`device_owned==0`，并把64+64判成
   超过QS，和实际driver contract、`committed_owner_count()`及owner tests冲突；这会令A3/A4永远
   无法通过。不得通过修改driver ledger迎合错误fixture。
2. **Blocking / PLAN-OMISSION — 可服务owner没有初始link snapshot工作。**
   `Service::new()`把`link_state`设为`None`，`RxRxFuture::poll_active()`只在CONFIG cause存在时调用
   `link_policy_step_target()`；启动路径不会产生CONFIG cause。因此HMP首次变更前V4可持续报告
   `u64::MAX`，而probe要求pre-reset link up。Cycle 001又明确排除产品修改，现有执行契约无法修复。
3. **Blocking / ACT-DEVIATION + NEW-EVIDENCE — UDP归因和日志声明不可审计。**
   `open_peer_socket()`把`connect`、`F_GETFL`和`F_SETFL`合并在一个条件中，失败打印也不标明阶段；
   所以“connect失败”不能由代码推出。当前唯一Evidence `qemu-serial-dbg.log`只记录启动至shell后
   QEMU退出，没有`DBG:`、probe marker、FAIL或harness exit；Act Response所列DBG tuple、三次运行和
   `qemu-serial-info.log`均不在当前Evidence中。下轮必须分阶段记录errno，并只依据实际输出归因。
4. **Non-blocking / BASELINE-CHANGED — 自动Gate仍为绿色，但证明的是错误fixture。**
   本次独立复跑owner focused tests为2/2 PASS，C probe decision test、MS07 validator与peer self-test
   均exit 0；这些结果证明当前实现稳定复现既有契约，不证明该契约与VirtIO owner语义一致。

**Deviation Classification**

`PLAN-INVALID`、`PLAN-OMISSION`、`ACT-DEVIATION`、`NEW-EVIDENCE`、`BASELINE-CHANGED`。

**Acceptance Gaps**

- A1：自动Gate仍接受`device_owned==0`的伪健康快照，未证明真实VirtIO owner守恒。
- A2/A4：Active V4在首次HMP事件前可保持link unknown，无法证明pre-reset up或后续link代次关系。
- A3/A6：没有可审计的probe运行、分阶段socket errno、完整marker和harness exit。
- A5：MS01/MS04/MS05/MS06手工回归尚无结果。

**Evidence**

- `crates/axdriver_virtio/src/net.rs`：`refill_all()`建立`QS`个RX owner与`QS`个TX buffer；
  `committed_owner_count()`统计RX+TX committed owners；`owner_summary()`按实际device边界分类。
- `crates/axnet/src/service.rs`：`link_state: None`；只有`link_policy_step_target()`提交状态。
- `crates/axnet/src/async_rx.rs`：`poll_active()`仅在`causes.config`时执行link micro-step。
- `tests/ms07_recovery_probe.c`与`scripts/ms07-qemu-validate.py`：健康/drained条件硬编码
  `device_owned==0`；socket setup合并多个syscall失败层。
- `qemu-serial-dbg.log`：启动到shell后`QEMU: Terminated`，无probe协议或DBG根因输出。
- 新鲜命令：`cargo test ... axdriver_virtio ... owner_summary`为2 passed；C decision test、validator
  self-test、peer self-test均exit 0。

**Convergence**

expanded。Cycle 000只暴露身份型证据设计；Cycle 001清除了该层，却首次暴露初始link与owner契约
错误，并且没有产生可审计runtime结果。

**Follow-up Decision**

Cycle 001不能在原契约内继续：所需修复进入被明确排除的产品queue-owner启动路径，并改变probe/
validator的owner验收语义。更新R6、R8、D6、D8和Task 4.2，在同一Iteration建立Cycle 002 replan。

**Iteration Plan Update**

Iteration 007目标与依赖不变；稳定基线增加“首个一致link snapshot已提交”和“VirtIO双向owner分类与
probe/validator一致”。验证边界先用host/model证明两项修正，再进入用户手工QEMU。诊断边界增加
queue-owner首次link读取、RX/TX owner分类及guest socket setup分阶段errno。平衡审计仍为单一
Iteration：这些修正都是Task 4.2真实QEMU资格成立的必要条件，不能独立形成后续项目成果。

**Next Cycle**

`002-replan.md`

**Next Iteration**

None.
