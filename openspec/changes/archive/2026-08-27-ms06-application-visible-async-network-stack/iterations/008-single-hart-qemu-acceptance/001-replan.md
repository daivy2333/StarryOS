# Iteration 008 / Cycle 001: repair witness contracts and rerun QEMU acceptance

## Plan Context

- Status: ready
- Approval: approved；用户于 2026-08-27 回复"更改gate状态，开始实施"，显式批准 Gate 2 并授权本 Cycle 的 `openspec-act` 执行
- Iteration: 008-single-hart-qemu-acceptance
- Cycle: 001-replan
- Cycle Type: replan
- Parent cycle: `000-initial.md`

**Iteration Scope**

- Change tasks: 7.3, 7.1-7.2
- Depends on: Iteration 007 accepted；Cycle 000 Review `replan-required`；用户可执行手工 QEMU batch
- Stable baseline: MS06 witness 判据与公开 ABI/TCP 语义一致；修订后的 probe 与 MS01/MS04/MS05 在
  同一 fresh single-hart VirtIO-MMIO 环境全部通过；最终 diff 无 Critical/Important finding。
- Verification boundary: 三项 witness 反例先有 RED→GREEN；完整原始串口直接通过 validator；每项
  runtime 有环境、revision、命令、完整 marker 和显式 exit 0；任一失败立即停止下游。
- Diagnostic boundary: 先区分 probe/validator 判据与产品行为，再定位 guest ABI、syscall waiter、
  runner wake、QEMU device model 或既有 runtime 兼容面。
- Deferred tasks: None

**Cycle Scope**

- Trigger: Cycle 000 Review `replan-required`
- Acceptance gaps: listener 与 close-error 结果被无效判据污染；原始串口无法直接审计；Task 7.2
  越过失败边界且缺完整 exit；boot artifact 未按 post-build 身份冻结。
- Repair items: None；replan 使用更新后的全局 Tasks 7.3、7.1、7.2。
- Inherited scope: R1-R7、D1-D11、Tasks 1.1-6.1 accepted；R44/R51/R56/R58；single-hart、单
  VirtIO-MMIO NIC；用户对 Iteration 007 普通测试残余 SIGSEGV 的原风险豁免保持不变。
- Excluded scope: axnet、smoltcp、kernel 产品修改；marker case 清单变化；自动 guest 输入；reset、
  SMP、多 NIC、PCI/DWMAC、真板、性能、commit、全局状态同步和归档。

**Objective**

先消除会把正确行为判成失败或拒绝原始输入的 witness 缺陷，再生成 fresh artifact。用户按严格顺序
完成 MS06 和兼容回归后，Act/Plan 依据完整原始串口、host 双边结果和显式 exit 作最终验收。

**Scenario Sketch**

| Scenario | Precondition | Action | Observable result | Failure boundary |
|---|---|---|---|---|
| S1 listener byte identity | 连接身份与回包都是一个 byte | host seam 穷举 1-4；guest 建立四连接 | 合法补码 byte 匹配，重复/错误 byte 拒绝；四连接唯一 accept | 32-bit promotion 制造假阴性或真实重复被接受 |
| S2 peer-FIN EOF | peer 发送 FIN，本地 write half 未 shutdown | poll 后连续 recv | `IN|RDHUP`、无 `ERR`、两次 recv 均为 0 | 要求 send 必须 EPIPE、EOF 漂移或正常 FIN 被标成 fatal |
| S3 raw serial validation | `script` 日志含 ANSI/CSI 控制序列 | validator 读取原始文件 | 仅剥离控制序列后按完整物理行识别协议 | 接受可打印前后缀、人工重构文本或仍报 START 缺失 |
| S4 MS06 runtime | S1-S3 GREEN；post-build artifact 已冻结 | 手工运行修订 probe | 12 个唯一 PASS、END、`MS06_HARNESS_EXIT: 0`；raw validator exit 0 | 任一 FAIL、缺 marker/exit、timeout 或 validator 非零 |
| S5 compatibility runtime | S4 GREEN；同一 session/artifact | 运行 MS01、MS04、MS05 | 14/14、4/4、6/6 及 guest/host exits 全闭合 | S4 前执行、handshake 重试覆盖首错或任一 exit 缺失 |

**Current Baseline**

- Branch `net-k3`；HEAD `1d0313ad8d0f36d918d1a101dd0ceda5c2ba336b`。工作树含用户授权的
  R44/R58 Runbook 改动、Cycle 000 Act 反馈和三份 Evidence；不得覆盖这些历史现场。
- Cycle 000 raw session 位于 `/tmp/ms06-iteration-008-qemu-serial.log`：949 行、159,308 bytes；
  无 panic/trap/fatal，probe 发布 10 PASS、2 FAIL 和 `MS06_HARNESS_EXIT: 1`。
- listener raw trace 中四个 hidden sockets 依次转为 `SynReceived`，四次 accept 均消费 Ready；四个
  child 随后都 exit 3。现有 C 比较把单字节 echo 与 32-bit `~ident` 比较，合法回包必然失败。
- close-error 已观察 `IN|RDHUP` 与稳定 EOF 后，再要求本地 send 在八次内 EPIPE。peer FIN 只关闭
  receive half；D10/R6 没有这项要求。
- validator self-test 通过，但直接读取 raw log 返回 `start marker is missing`；ANSI reset 控制序列
  位于 START 同一物理行。人工提取文件不是完整 transcript。
- 当前 MS06 payload 是 RISC-V little-endian static `ET_EXEC`，内嵌旧 revision `832abfea…`；下一次
  runtime 必须使用 Task 7.3 后重建的 payload，不得复用。

**Current-State Evidence**

- `tests/ms06_stack_readiness_probe.c::run_listener`：child 发送 `&ident` 的 1 byte，parent 接收
  `unsigned char` 并回送 1 byte；child 的 `echo != ~ident` 在整数提升后宽度不一致。
- `run_close_error`：poll/双 EOF 已覆盖 R6 的 peer-FIN 行为；之后的 `saw_epipe` 循环和稳定 EPIPE
  复查是无 requirement 来源的额外判据。
- `tests/ms06_stack_readiness_probe_test.c` 只有 event-bit close test，没有 listener reply-width 或
  peer-FIN I/O verdict seam，因此 26 项 host test 未见证两条 runtime 判定路径。
- `scripts/ms06-qemu-validate.py::validate_output` 对每行只调用 `strip()`；它正确拒绝可打印边界噪声，
  但不区分 ANSI/CSI transport decoration。
- `Makefile::host-test` 已覆盖 validator self-test、C syntax、probe seam×2、case-list diff 和 source
  guards；`tests/ms06_stack_readiness_probe` target 负责 RISC-V static non-PIE build。
- `make run` 依赖 build；用于 acceptance 时必须先完成构建，再记录 post-build 身份。若要确保启动
  阶段不重建，应在同一配置下使用 `make ARCH=riscv64 justrun`；两者默认 `LOG=warn`。

**Relevant Code**

| Surface | Responsibility |
|---|---|
| `tests/ms06_stack_readiness_probe.c` | listener、peer-FIN/EOF runtime 与纯判定 seam |
| `tests/ms06_stack_readiness_probe_test.c` | 三项 witness 缺陷的 host RED→GREEN |
| `scripts/ms06-qemu-validate.py` | raw transcript ANSI/CSI normalization 与 marker grammar |
| `Makefile` | host gate、RISC-V payload build、revision 嵌入 |
| Cycle 001 Act Response / Evidence | 命令、exit、fresh identity 和 runtime 决定性结构 |

**Critical Path**

```text
listener/peer-FIN/ANSI negatives RED
  -> repair probe + validator only
  -> seam/syntax/validator GREEN
  -> RISC-V static payload + kernel build
  -> freeze post-build identity
  -> manual MS06 raw transcript -> validator 12/12 + exit 0
  -> only then MS01 -> MS04 -> MS05 with explicit guest/host exits
  -> strict OpenSpec + diff checks + full diff Review
```

**Implementation Guidance**

- 把 listener reply 判定提取为纯 helper，并让 child 与 host seam 共享；wire identity 和 reply 均按
  `unsigned char` 语义比较。
- close-error 保留 poll、event、双 EOF、child cleanup 和 no-ERR verdict；删除 send→EPIPE 收敛要求。
  可提取 peer-FIN observation helper供 seam 使用，不新增产品错误类别。
- validator 只剥离 ANSI/CSI 控制序列，不删除普通可打印字符。已有 whole-line、前导空白、前后 phase、
  duplicate/order/exit negatives 必须保持 GREEN。
- 先 build，再 freeze。`make run` 如会重新进入 build，不得把 build 前 mtime 当作运行输入身份；可用
  同配置 `make justrun` 启动已冻结镜像。

**Behavioral Change**

- listener：从必然拒绝合法单字节补码回包，改为按线宽验证唯一连接 echo。
- close-error：从要求 peer FIN 后本地 write half 稳定 EPIPE，改为验证 receive-half EOF readiness、
  双零读和无 device `ERR`。不改变产品 TCP 行为。
- validator：从拒绝带 ANSI/CSI 装饰的原始串口，改为剥离控制序列后执行原有严格 whole-line grammar。
- runtime：从失败后继续并用部分结果收口，改为 7.3 → 7.1 → 7.2 的硬依赖和首次失败即停。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| 7.3 | R6-R7/S1-S3 | probe、probe test、validator | witness 判定 | 三个 RED→GREEN，修正验证契约 |
| 7.1 | R1-R7/S4 | rebuilt MS06 payload、raw validator | application witness | fresh 12/12、END、exit 0 |
| 7.2 | R7/S5 | MS01/MS04/MS05 probes + host stimuli | compatibility regression | 仅在 7.1 GREEN 后完整重跑 |

**Task Contracts**

### 7.3: repair the MS06 witness without changing product behavior

- Requirement/Scenario: R6、R7；S1-S3；D10。
- Depends on: Cycle 000 raw evidence and Review `replan-required`。
- Targets: `tests/ms06_stack_readiness_probe.c`、`tests/ms06_stack_readiness_probe_test.c`、
  `scripts/ms06-qemu-validate.py`；必要的 Makefile host guard仅在现有入口不能执行新 witness 时修改。
- Current behavior: 合法 listener echo 必然假阴性；peer FIN 被错误升级为 EPIPE 契约；raw ANSI/CSI
  transcript 被判缺 START。
- Required behavior: 三类最小反例在修复前 RED，修复后 GREEN；case 名称、顺序、deadline、marker
  grammar 和产品代码保持不变。
- Required changes: 共享单字节 listener helper；peer-FIN EOF helper或等价共享判定；ANSI/CSI-only
  line normalization及正反 self-tests；删除无来源的 EPIPE runtime requirement。
- Preserve: 12-case registry、whole-line marker、防重复/乱序/partial/exit检查、无 sleep/caller poll、
  SIGPIPE 防护、fixed deadlines 和 cleanup。
- Forbidden: 修改 axnet/smoltcp/kernel、放宽 printable marker prefix/suffix、把 FAIL 改成 PASS、删 case、
  延长 deadline 掩盖失败或重写历史 Evidence。
- Test witness: 先只加 host tests并运行，预期 listener/peer-FIN helper缺失或旧行为断言失败，ANSI raw
  positive失败；记录决定性 RED。实现后运行 validator self-test、C syntax、probe seam×2和case-list diff。
- GREEN condition: 三类新 positives/negatives与原26项全部通过；无 warning；raw Cycle 000 transcript
  能识别 START 并以首个真实 payload FAIL退出，而不是 `start marker is missing`。
- Verification: `python3 scripts/ms06-qemu-validate.py --self-test`；两条 `cc` 命令；probe seam×2；
  validator与probe `--print-cases` diff；`make tests/ms06_stack_readiness_probe`。
- Stop when: 正确判据要求改变 R6/R7、case清单、产品错误语义或无法仅剥离控制序列保持 whole-line 安全。

### 7.1: obtain a valid fresh MS06 runtime witness

- Requirement/Scenario: R1-R7；S4。
- Depends on: Task 7.3 GREEN；fresh kernel/probe build；用户手工 QEMU 能力。
- Targets: post-build boot image和MS06 payload、R44/R58命令、完整 raw serial、validator。
- Current behavior: 旧artifact和Cycle 000结果不再能决定 Acceptance 1。
- Required behavior: post-build freeze 后手工运行；raw transcript直接验证12个唯一PASS、metadata、END和
  `MS06_HARNESS_EXIT: 0`。
- Required changes: 无产品改动；记录实际build/start/download/run/exit命令与前后artifact身份。
- Preserve: 手工guest输入、single-hart、单VirtIO-MMIO NIC、fixed deadline、首次失败现场。
- Forbidden: 人工重构 transcript、重用旧payload、自动 guest 输入、失败后进入7.2、把QEMU exit当guest exit。
- Test witness: 完整 raw serial 与当前 revision/environment validator命令。
- GREEN condition: validator exit 0；raw 中无 FAIL/panic/trap/fatal；QEMU与guest exit分层完整。
- Verification: `python3 scripts/ms06-qemu-validate.py --expect-revision <HEAD> --expect-environment
  qemu-virt-riscv64-single-hart <raw-serial>`。
- Stop when: build/identity漂移、任一FAIL/timeout/缺marker、validator非零或guest exit非零。

### 7.2: rerun compatibility and close final review

- Requirement/Scenario: R7；S5；MS01/MS04/MS05 compatibility/ownership。
- Depends on: Task 7.1 GREEN；同一 QEMU session 和 frozen artifact集合。
- Targets: MS01 14-case、MS04四mode、MS05六mode、15556/15557 host stimulus、完整diff。
- Current behavior: Cycle 000 observations不满足顺序、exit和identity契约，不能复用为Acceptance。
- Required behavior: MS01 14/14；MS04 4/4；MS05 6/6；每个guest workload和host pipeline显式exit 0；
  首次handshake/marker失败立即停止，不以重跑PASS覆盖。
- Required changes: 只执行与记录；每个host stimulus先于对应guest mode启动并用`pipefail`+`tee`。
- Preserve: R51/R56 telemetry、96包守恒、Full→recovery、flush ledger、artifact身份和single-hart边界。
- Forbidden: 缺失exit、失败后继续、复用Cycle 000结果、修改测试/产品、扩大到SMP/真板/性能。
- Test witness: full serial、host outputs、artifact identity、最终full diff。
- GREEN condition: 六项Acceptance全部闭合且无Critical/Important finding。
- Verification: marker/exit/fault提取；host count/received/pipeline exit；strict OpenSpec、三组diff check和
  full diff Review。
- Stop when: 任一guest/host marker、telemetry、exit或identity失败。

**Invariants**

- probe修复不改变产品网络状态、socket API或runner ownership。
- 正常 peer FIN 不标记为queue/data-plane fatal；稳定产品 fatal 的 terminal-first契约保持不变。
- 首次失败不能被后续重跑覆盖；Task 7.2只能在Task 7.1 GREEN后执行。
- QEMU结论只覆盖single-hart、单VirtIO-MMIO NIC软件/设备模型。

**Non-goals**

- axnet/kernel修复、marker case增删、deadline放宽、自动QEMU harness、reset/SMP/真板/性能。
- 修改或删除Cycle 000 raw Evidence、用户授权的Runbook改动、commit、归档或全局状态同步。

**Requirements Traceability Matrix**

| Requirement | Scenario | Design | Task | Code Surface | Test Witness | Status |
|---|---|---|---|---|---|---|
| R6 listener唯一accept | S1 | D7,D10 | 7.3,7.1 | probe listener/helper | byte-width RED→GREEN + guest四连接 | Covered |
| R6 peer FIN/EOF | S2 | D8,D10 | 7.3,7.1 | close-error/helper | EOF/no-ERR RED→GREEN + guest | Covered |
| R7原始证据 | S3 | D10 | 7.3,7.1 | validator | ANSI/CSI正反矩阵 + raw log | Covered |
| R1-R7 runtime | S4 | D1-D10 | 7.1 | MS06 payload/QEMU | 12/12 + exit 0 | Covered |
| R7兼容回归 | S5 | D2,D4,D10 | 7.2 | MS01/MS04/MS05 | 14/14、4/4、6/6 + exits | Covered |

**Acceptance**

1. listener byte-width、peer-FIN EOF和ANSI/CSI raw transcript三项有真实RED→GREEN；旧host tests不回归。
2. 修订MS06 raw transcript直接通过validator：12/12、当前revision/environment、END、guest exit 0。
3. MS01 14/14，START/END和guest exit 0完整。
4. MS04四mode与MS05六mode满足R51/R56，全部guest/host pipeline exit 0。
5. runtime使用Task 7.3后同一post-build frozen artifact/session；命令、boot、marker、host结果可追溯。
6. 最终full diff无Critical/Important finding；结论严格限定single-hart QEMU VirtIO-MMIO。

**Verification**

- Task 7.3 focused RED→GREEN、validator self-test、probe seam×2、C syntax、case-list diff。
- RISC-V static payload `file`/`readelf`、内嵌revision和post-build size/mtime。
- 用户手工QEMU；raw transcript直接validator；所有guest/host显式exit。
- MS04 96包守恒；MS05 count/received、Full/recovery、flush/fault闭合。
- `openspec validate ms06-application-visible-async-network-stack --strict`。
- `git diff 1ea51427..HEAD --check`、`git diff --check`、`git diff --cached --check`和full diff Review。
- SKIPPED: 自动guest输入（R44硬边界；不是Acceptance豁免）。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | raw serial、probe判定路径、validator parser、build/run依赖和公开R6语义已核对 |
| Design | PASS | byte-width、peer-FIN、ANSI/CSI及严格失败顺序闭合，无产品语义TBD |
| Iteration Plan | PASS | 7.3→7.1→7.2共同形成原最终baseline；不新增逻辑Iteration |
| Cycle Scope | PASS | 仅修witness并重验既有Acceptance；禁止产品修改和平台扩张 |
| Task Contracts | PASS | 三个task含目标、RED、GREEN、验证、停止和手工边界 |
| Traceability | PASS | R6/R7、D7/D8/D10、三反例、三task和六项Acceptance闭合 |
| Verification | PASS | focused、artifact、raw validator、兼容回归和full diff递增覆盖 |

Gate 2技术检查项PASS；用户于2026-08-27 显式批准（原话："更改gate状态，开始实施"），Plan Context 状态更新为 `ready`，授权执行本 Cycle。

**Persisted Evidence**

- Mode: required
- `README.md`：支持Acceptance 1-6；记录RED→GREEN摘要、环境、HEAD/dirty状态、post-build artifact
  identity、完整raw串口外部路径、手工命令、QEMU/guest/host exits和文件映射。人工session不可低成本
  重跑，缺失会阻止最终Acceptance。
- `qemu-runtime-markers.md`：支持Acceptance 2-4；按raw行号保存boot、命令、MS06/MS01/MS04/MS05
  决定性marker与exit。缺少原始结构会阻止runtime判定。
- `host-runtime-results.md`：支持Acceptance 4；保存MS04/MS05 literal host命令、pipeline exit和决定性
  count/received。缺失会阻止双边闭合。
- 完整raw串口保存在`/tmp/ms06-iteration-008-cycle-001-qemu-serial.log`供Act/Plan直接审查；超过公共
  行数或大小预算时不入库，也不得拆分或压缩绕过限制。
- Budget: 本Cycle计划3文件（含README），本change完成后11/20；每个文本≤500行且≤256 KiB。

**Risks and Notes**

- 当前sandbox的RISC-V musl compiler曾返回SIGSYS，AF_INET也被禁止。host pure tests可在agent环境运行；
  cross build/QEMU若仍受限，Act按Gate 6 blocked并交给用户，不把环境失败归为产品失败。
- `make run`会构建后启动；若Acceptance需要已冻结输入，先`make build`并记录identity，再用相同配置
  `make justrun`。这不改变用户要求的默认`LOG=warn`，但与Runbook当前单命令示例存在非阻塞差异。
- Cycle 000的MS01/MS04/MS05结果只用于定位影响层，不能替代本Cycle按序重跑。

## Act Response

- Status: reported

**Implemented**

- 按用户 2026-08-27 显式批准（原话："更改gate状态，开始实施"）将本 Cycle Plan Context
  由 `draft` 更新为 `ready` 并登记 Approval；Gate 2 段落同步记录授权。
- Task 7.3 witness 修复（先 RED 后 GREEN，只改 probe/validator，不触碰 axnet/smoltcp/kernel）：
  1. listener 单字节 echo：`ms06_listener_reply_matches(ident, echo)` 按 unsigned-char 语义比较，
     消除 `~ident` 整数提升造成的必然假阴性。
  2. close-error peer-FIN：`ms06_peer_fin_eof_valid(events, recv1, recv2)` 只要求 IN|RDHUP 且无 ERR
     且两次零读，删除 send→EPIPE 收敛要求。
  3. validator ANSI/CSI：`_normalize()` 只剥离 ESC 控制序列后执行严格 whole-line grammar。
  4. validator 外来 workload 容忍（修复 3 的必要补充，见 Deviations）：只有成功的 MS06 exit 后出现
     精确 `MS01_SOCKET_BASELINE_START` 才进入外来阶段；边界前未知 `PASS:` 仍拒绝，MS06 case、`FAIL:`、
     `MS06_*` 在 END 后始终拒绝。
- 生成 fresh artifact：`make tests/ms06_stack_readiness_probe`（147,128 B，内嵌 HEAD `1d0313ad…`）与
  `make ARCH=riscv64 build`（boot bin 40,763,584 B）均 exit 0；冻结 post-build 身份。
- 用户于 sandbox 外完成手工 QEMU session（`make justrun`，冻结镜像）：MS06 12/12、MS01 14/14、
  MS04 4/4、MS05 6/6 均 PASS 且 guest exit 0（MS05 snapshot 重复一次），完整 raw 串口经 validator 判定 exit 0。

**Changed Files and Symbols**

- `tests/ms06_stack_readiness_probe.c`：+`ms06_listener_reply_matches`、+`ms06_peer_fin_eof_valid`
  （seam，pure）；`run_listener` 子进程 echo 判定；`run_close_error` 移除 EPIPE 段。
- `tests/ms06_stack_readiness_probe_test.c`：+`test_listener_reply_matches_byte_width`、
  +`test_peer_fin_eof_valid_contract`；main 计数 26→28。
- `scripts/ms06-qemu-validate.py`：+`import re`、+`ANSI_CSI_RE`、+`_normalize`；tail 以成功 exit 后的
  `MS01_SOCKET_BASELINE_START` 作为外来阶段边界；+ANSI/CSI、共享 session、未知 PASS 与尾随 MS06 正反例。
- `iterations/008-single-hart-qemu-acceptance/001-replan.md`：Plan Context ready + Approval、本 Act Response。
- 重新构建产物：`tests/ms06_stack_readiness_probe`、`StarryOS_riscv64-qemu-virt.bin`。

**Deviations from Plan**

- peer-FIN helper 签名取 `(uint32_t events, int recv1, int recv2)`（两次 recv 返回值为共享判定输入），
  属契约允许的"peer-FIN EOF helper 或等价共享判定"等价局部实现，无实质差异。
- 采集启动命令：Plan Risks 明确授权"先 `make build` 并记录 identity，再用同配置 `make justrun`"；
  本 Cycle 已 `make build` 冻结，用户用 `make justrun` 启动（不重建）。与 R44/R58 单命令示例
  `make run` 存在 Plan 已声明的非阻塞差异。
- **第 4 处 validator 修复（tail 扫描容忍外来 workload）**：Cycle 000 的 ANSI 问题掩盖了该缺陷，
  修复 ANSI 后 validator 对完整多 workload 串口报 `protocol marker after the end marker:
  'PASS: tcp-accept'`（MS01 在共享 session 中 MS06 END 之后的 PASS 行）。Plan Task 7.1 GREEN 条件
  "完整 raw 串口由 validator 直接判定且 exit 0" 不修此点无法成立；该修复属于 Task 7.3 目标
  "raw transcript 直接判定" 的必要后果，未改 case 清单、marker grammar、产品代码。已按 TDD 加
  RED→GREEN self-test（显式 foreign boundary 正例 + unknown PASS / trailing MS06 case 反例）。Plan Review
  发现初版过度放宽后，本次 Act 先加入未知尾随 PASS 反例，旧实现以 `invalid synthetic output was accepted`
  真实 RED，再以显式边界状态收紧并 GREEN。

**Blocker Handoff**

- Task/Step/Gate：Task 7.1 / Phase 3 EXECUTE / Gate 5（runtime acceptance）。
- Plan 预期：post-build freeze 后用户手工运行修订 MS06 probe，raw transcript 由 validator 判定
  12/12、END、`MS06_HARNESS_EXIT: 0` 且 validator exit 0。
- 实际情况（当时）：Task 7.3 已 GREEN，fresh artifact 已冻结；但 single-hart QEMU 必须由用户在
  sandbox 外手工输入 guest 命令（R44 硬性政策：禁止脚本/pipe/pexpect 驱动 guest shell；sandbox
  拉起 QEMU 返回 EPERM/受环境拒绝）。能力边界，非产品失败。
- 恢复条件（已满足）：用户完成下述 Manual Command Contract 的 MS06/MS01/MS04/MS05 手工 QEMU，
  提供完整 raw serial 与 host 结果。

**Blocker Resolution**

- 用户于 2026-08-27 在 sandbox 外完成整个手工 QEMU session 并提供完整 raw 串口
  `/tmp/ms06-iteration-008-cycle-001-qemu-serial.log`（264 行 / 86,723 B）。Act 恢复
  `blocked → pending`，grep 完整串口核对 MS06/MS01/MS04/MS05 决定性 marker、guest exit 与无
  fatal，并按 Task 7.1 命令用 validator 对完整 raw 串口判定。期间发现并修复第 4 处 validator
  tail 扫描缺陷（见 Deviations），并在 Plan Review 指出过度放宽后按本次恢复要求收紧；修复后 validator
  exit 0。本轮重跑 Task 7.3 seam/self-test/syntax/case-diff 均 GREEN，Task 7.1 validator 对完整串口 exit 0。
  Task 7.2 要求的 host transcript/pipeline exit 与 QEMU 命令退出码未完整采集；用户明确选择不重复采集并
  接受该风险，见 Remaining Issues 与 Evidence。

**Self-Review**

- Spec compliance：改动都落在 Plan Change Surface（probe、probe test、validator），未触碰
  axnet/smoltcp/kernel；case 名称、顺序、deadline、marker grammar 不变；未删 case、未把 FAIL 改
  PASS、未放宽 deadline、未引入 sleep/caller poll。listener 语义符合 D7/D10 单字节 echo；
  close-error 符合 D8/D10 peer-FIN 半关闭（不要求 EPIPE）；validator 只剥离控制序列、只容忍外来
  workload 阶段的外来 `PASS:` 行；未知 PASS 在显式边界前拒绝，MS06 自身的 `PASS:`/`FAIL:`/`MS06_*`
  乱序/重复/越界仍被严格拒绝。
- Code quality：`cc -Wall -Wextra -Werror` 无新增警告；新 seam helper 纯函数无 syscall；移除 EPIPE
  段未留下未使用变量；validator 正反 self-test 覆盖新分支；测试因正确原因通过。
- 完整 diff Review：`git diff HEAD`（probe.c +49/-…、probe_test.c +34、validate.py +ANSI+tail）
  仅含计划内改动，无跨任务交互、无计划外修改、无回归风险；三路 `git diff --check` exit 0。
- 未解决产品 Critical/Important：无。host/QEMU 进程级证据缺口由用户显式接受，不伪造为已采集。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| Task 7.3 RED (C) | `cc … probe_test.c -o /tmp/…` | `error: implicit declaration of function 'ms06_listener_reply_matches'` / `'ms06_peer_fin_eof_valid'`（exit 1） | RED |
| Task 7.3 RED (validator ANSI) | `python3 scripts/ms06-qemu-validate.py --self-test` | `FAIL: ms06-validator: start marker is missing`（exit 1） | RED |
| 第 4 修复 RED (tail) | `--self-test`（foreign_tail 正例加入后） | `FAIL: ms06-validator: protocol marker after the end marker: 'PASS: tcp-accept'`（exit 1） | RED |
| tail 收紧 RED | `--self-test`（unknown PASS 反例加入后） | `FAIL: ms06-validator: invalid synthetic output was accepted`（exit 1） | RED |
| seam 决策测试 | `cc … probe_test.c && /tmp/ms06-stack-readiness-probe-test` ×2 | `ms06 probe decision tests: 28 passed`（exit 0） | PASS |
| validator self-test | `python3 scripts/ms06-qemu-validate.py --self-test` | `PASS: ms06-validator-self-test`（exit 0） | PASS |
| C syntax | `cc -std=c11 -Wall -Wextra -Werror -fsyntax-only tests/ms06_stack_readiness_probe.c` | exit 0 | PASS |
| case-list diff | `--print-cases`（validator vs probe）`diff -u` | 无差异（exit 0） | PASS |
| RISC-V probe build | `make tests/ms06_stack_readiness_probe` | `-DMS06_REVISION_DEFAULT='"1d0313ad…"'` exit 0；statically linked RISC-V | PASS |
| kernel build | `make ARCH=riscv64 build` | `Finished release profile`；`rust-objcopy … -O binary` exit 0 | PASS |
| MS06 runtime | 用户手工 QEMU + validator 对完整 raw 串口 | 12/12、END、`MS06_HARNESS_EXIT: 0`；`PASS: ms06-transcript-valid`（validator exit 0） | PASS |
| MS01 runtime | raw 行 154–181 | 14 个唯一 PASS + `MS01_HARNESS_EXIT: 0` | PASS |
| MS04 runtime | raw 行 185–209 | snapshot/idle/nudge/burst 各唯一 PASS；burst `reaped=refilled=delivered=96`、`budget=3 yield=2 fault=0`；exit 0 | PASS |
| MS05 runtime | raw 行 210–262 | 六 mode 均 PASS（snapshot 重复一次）；`WITNESS host_received=96`；slot-full/descriptor-full Full→recovery 闭合；flush `flush_ok=1`；guest exit 0 | PASS |
| 无 fatal | grep raw 串口 | `FAIL|panic|trap|fatal|illegal|page fault` 均为空 | PASS |
| diff check | `git diff --check` / `--cached --check` / `1ea51427..HEAD --check` | 三路均 exit 0 | PASS |
| OpenSpec | `openspec validate ms06-application-visible-async-network-stack --strict` | `Change … is valid`（exit 0） | PASS |
| artifact identity | session 后 `stat -c '%y %s %n'` | 与冻结时一致（无重建漂移） | PASS |

**Persisted Evidence**

- `evidence/008-single-hart-qemu-acceptance/001-replan/README.md`（EV-008-001-01/02/03 索引、
  RED→GREEN 摘要、artifact identity、exit 汇总、限制）。
- `qemu-runtime-markers.md`（boot/命令与 MS06/MS01/MS04/MS05 决定性 marker + exit，含 raw 行号）。
- `host-runtime-results.md`（MS04 守恒、MS05 六 mode `WITNESS host_received=96` 与唯一 host log）。
- 完整 raw 串口 `/tmp/ms06-iteration-008-cycle-001-qemu-serial.log`（264 行 / 86,723 B）按 Plan
  保存在 /tmp 供审查，不入库。本 Cycle 3 文件，change 累计 11/20。

**Experience Candidates**

None（本 Cycle 为 witness 判定修复与 runtime 验证，非可复用运维路径；不满足 Runbook/Incident 候选条件）。

**Remaining Issues**

- 用户接受的证据完整性风险：MS04 与五个 MS05 mode 的 host transcript/pipeline exit、QEMU 命令退出码未
  留档；唯一 host 文件名与内容 mode 不一致。guest marker 不替代这些进程级证据。
- 用户 2026-08-27 原话："证据就不重复采集了，我逐步手动验证的全过，只是没采集完整而已，没必要重复
  工作"，并明确要求修复脚本后改为接收、正式结束 change。因此 Act 不补跑 QEMU，将上述缺口交由 Plan
  作为显式用户 waiver 审核。
- 无其他遗留产品 Critical/Important/Minor。

**Commit or Diff Reference**

- HEAD `1d0313ad8d0f36d918d1a101dd0ceda5c2ba336b`（net-k3），未 commit。
- 本轮工作树：`tests/ms06_stack_readiness_probe.c`（M）、`tests/ms06_stack_readiness_probe_test.c`（M）、
  `scripts/ms06-qemu-validate.py`（MM）、`tests/ms06_stack_readiness_probe`（M，重建）、
  `StarryOS_riscv64-qemu-virt.bin`（M，重建）、`iterations/008-…/001-replan.md`（untracked）、
  `evidence/008-…/001-replan/`（untracked，新建）。另含 Cycle 000 遗留的 runbook（M）、design.md（M）、
  tasks.md（M）、000-initial.md（M）与 `evidence/008-…/000-initial/`（untracked），非本轮改动。



## Plan Review

- Review Result: accepted

**Findings**

1. **Accepted fix.** Validator tail handling now requires `MS06_HARNESS_EXIT: 0` followed by exact
   `MS01_SOCKET_BASELINE_START` before accepting foreign `PASS:` lines. Unknown PASS before that boundary,
   an early boundary, a trailing MS06 case, `FAIL:` and `MS06_*` are negative-tested. The original complete raw
   shared session still validates.
2. **Accepted runtime result.** Probe decision seam 28/28 ×2, C syntax, case-list diff, validator self-test,
   raw validation, strict OpenSpec validation and all diff checks pass. The raw transcript records MS06 12/12,
   MS01 14/14, MS04 4/4 and all six MS05 modes with guest exit 0 and no fatal class. Cycle 001 changes no
   axnet/smoltcp/kernel product code.
3. **User-waived evidence gap.** MS04 and five MS05 host transcripts/pipeline exits plus the QEMU command exit
   were not retained. Guest markers do not prove those process-level exits. The user explicitly declined recapture:
   "证据就不重复采集了，我逐步手动验证的全过，只是没采集完整而已，没必要重复工作"，and instructed
   acceptance and formal closeout. Acceptance therefore relies on the user's manual-pass attestation and accepts
   the residual evidence-completeness risk; Evidence states the limitation without fabricating missing output.
4. **Corrected metadata.** Evidence now records 264 lines / 86,723 bytes, the duplicate successful MS05 snapshot,
   and the missing QEMU exit instead of claiming uniqueness or a captured normal process exit.

**Deviation Classification**

- `ACT-DEVIATION` resolved: the over-broad foreign-tail rule is now bounded by explicit foreign phase.
- `USER-WAIVER`: required host stimulus/pipeline and QEMU process-exit evidence is incomplete; user accepts the risk
  and forbids redundant recapture.
- `NEW-EVIDENCE` incorporated: final raw metadata and duplicate successful snapshot are accurately documented.

**Acceptance Gaps**

- Acceptance 1: PASS; strict bounded validator grammar and MS06 12/12 guest witness.
- Acceptance 2-3: PASS for recorded guest behavior and MS01 compatibility; QEMU process exit is user-waived.
- Acceptance 4-5: accepted from recorded guest witnesses plus user's manual-pass attestation; incomplete host/QEMU
  process evidence remains an explicit, scoped waiver.
- Acceptance 6: PASS; final review finds no unwaived Critical/Important issue.

**Convergence**

Complete. The witness defects and validator over-relaxation are closed; the only remaining limitation is the
explicitly accepted evidence-completeness waiver, not an open implementation task.

**Evidence**

- Fresh focused gate: validator self-test and raw validation PASS; C syntax PASS; probe seam 28/28 ×2 PASS;
  case-list diff PASS; RISC-V payload is static RISC-V `ET_EXEC` with embedded HEAD `1d0313ad…`.
- Validator TDD: unknown post-END PASS test first failed with `invalid synthetic output was accepted`; after the
  explicit-boundary fix it and the early-boundary/trailing-MS06 negatives pass.
- Raw `/tmp/ms06-iteration-008-cycle-001-qemu-serial.log`: 264 lines / 86,723 bytes; MS06 lines 135-151, MS01 154-181, MS04 184-209, MS05 210-262; no FAIL/fatal class. It ends at `make[1]: Leaving` without a captured command exit.
- Host files: only `/tmp/ms05-snapshot-host.log` exists; its literal content is `PASS mode=flush … received=96`.
  Missing R51/R56/R58 process evidence is retained as the user waiver above.
- Strict change validation and all three diff checks PASS. The complete staged diff contains no axnet/smoltcp/kernel product edit in Cycle 001.

**Follow-up Decision**

Accept this Cycle. No successor Cycle or evidence recapture is required. Proceed to normal change closeout while
preserving the user waiver and the single-hart QEMU VirtIO-MMIO scope.

**Iteration Plan Update**

None；本Cycle已使用父Review批准的修订Map。

**Next Cycle**

None.

**Next Iteration**

None；这是change的最终逻辑Iteration。
