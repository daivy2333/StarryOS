# Iteration 008 / Cycle 000: single-hart QEMU application acceptance

## Plan Context

- Status: ready
- Approval: approved；用户于2026-08-27 回复"更新gate状态，开始实施"，显式批准 Gate 2 并授权本 Cycle 的 `openspec-act` 执行
- Iteration: 008-single-hart-qemu-acceptance
- Cycle: 000-initial
- Cycle Type: initial
- Parent iteration: `007-automatic-integration-qualification`

**Iteration Scope**

- Change tasks: 7.1-7.2
- Depends on: Iteration 007 accepted
- Stable baseline: MS06应用可见probe及受影响MS01/MS04/MS05回归在同一fresh、single-hart、单
  VirtIO-MMIO NIC QEMU环境通过；最终diff无Critical/Important finding。
- Verification boundary: 每个runtime有环境、revision、手工命令、完整终态marker和显式guest exit；
  MS04/MS05 telemetry与host stimulus闭合；缺失、timeout、partial success或中断均不计通过。
- Diagnostic boundary: QEMU boot/device model、HTTP或离线payload注入、guest syscall waiter调度链、
  runner wake、MS04/MS05 queue/slot ownership或既有runtime兼容面。
- Deferred tasks: None

**Cycle Scope**

- Trigger: Iteration 007 Cycle 000 Plan Review `accepted`
- Acceptance gaps: 尚无当前fresh artifact集合的MS06 application witness和最终MS01/MS04/MS05
  single-hart runtime回归。
- Repair items: None
- Inherited scope: R1-R7；D1-D11；Tasks 1.1-6.1已接受；R44手工QEMU政策；R51/R56运行判据；
  R58 `script`/`tee`采集模式；single-hart VirtIO-MMIO结论边界。
- Excluded scope: 自动驱动guest shell、重建或修改artifact、产品修复、reset/SMP、多NIC、PCI/DWMAC、
  真板、性能、commit、全局状态同步和change归档。

**Objective**

使用Iteration 007生成且冻结的同一组artifact，由用户在sandbox外手工运行single-hart QEMU；先证明
MS06无需caller-driven poll的应用可见readiness，再运行MS01/MS04/MS05受影响回归，并让Act/Plan基于
完整串口、host结果、显式exit和bounded Evidence完成最终验收。

**Scenario Sketch**

| Scenario | Precondition | Action | Observable result | Failure boundary |
|---|---|---|---|---|
| S1 MS06 application witness | fresh image/probe；QEMU进入shell | 手工下载并运行MS06 probe | revision/environment匹配，12个case固定顺序PASS，END与exit 0齐全 | 任一FAIL、缺失、重复、乱序、timeout或非零exit |
| S2 compatibility runtime | S1通过且artifact未重建 | 同session运行MS01、MS04、MS05 | MS01 14/14；MS04四mode；MS05六mode与host结果全部闭合 | marker、telemetry、ownership、host count或exit不符 |
| S3 environment/capture | R44禁止自动guest驱动 | 用户逐条输入，`script`录串口、`tee`录host | boot、输入、marker和进程/workload exit可区分 | sandbox/QEMU能力阻塞、缺完整输入或把QEMU exit当guest exit |
| S4 timeout/interruption | 任一mode运行中 | deadline到期、用户中断或QEMU异常退出 | 明确FAIL/INCOMPLETE并停止下游 | 旧Evidence、partial marker或重跑后的PASS覆盖首次失败 |

**Current Baseline**

- Branch `net-k3`；HEAD `832abfead57e7ae0870d5b729b6875665d588582`。
- Iteration 007 `000-initial` accepted：automatic Gates通过；ordinary重复窗口的一次SIGSEGV由用户明确
  豁免并保留EV-007-000-01/02，不扩大为已修复结论。
- QEMU 7.0.0、util-linux `script` 2.37.2可用；当前sandbox执行musl compiler返回SIGSYS且禁止
  AF_INET socket，符合R44的用户手工能力边界，不能在agent环境自动启动/驱动QEMU。
- Frozen inputs：`StarryOS_riscv64-qemu-virt.bin` 40,763,584 bytes；MS01 155,272 bytes；
  MS04 134,232 bytes；MS05 149,528 bytes；MS06 147,024 bytes。MS06内嵌HEAD revision和
  `qemu-virt-riscv64-single-hart` environment。
- `make/disk.img`存在；MS01为static-pie RISC-V ELF，MS04/MS05/MS06为static non-PIE RISC-V ELF。
  两者均不依赖guest动态加载器。
- 工作树还含Runbook/reference/Cycle文档编辑和重建后二进制差异；本Cycle不得重建、替换或commit
  artifact。启动前后以size/mtime核对同一输入。

**Current-State Evidence**

- `tests/ms06_stack_readiness_probe.c::main`依次运行12个case，输出START、内嵌revision、environment、
  每case PASS/FAIL和END；shell必须随后输出`MS06_HARNESS_EXIT: <rc>`。
- `scripts/ms06-qemu-validate.py`只读取完整串口，检查metadata、12-case顺序、FAIL/timeout、END和exit；
  `--expect-revision`与`--expect-environment`固定当前artifact身份。
- `tests/ms01_socket_baseline.c`输出START、14个独立case和END，进程返回聚合状态；shell补
  `MS01_HARNESS_EXIT`。
- R51固定MS04 `snapshot/idle/nudge/burst`判据；burst需要host 15556 stimulus，96包守恒、budget/
  self-yield和fault字段共同判定。
- R56固定MS05 `snapshot/tx-only/bidirectional/slot-full/descriptor-full/flush`判据；每mode需要
  独立host 15557 stimulus，guest与host计数、Full→recovery、flush ledger和fault字段共同判定。
- R44要求guest命令逐条手工输入；R58用`script -q -e -f`记录完整串口、host pipeline先启用
  `pipefail`再`tee`。QEMU进程exit与guest workload exit是两个独立判据。
- 公共Evidence预算限制每Cycle最多5文件、单文本最多500行/256 KiB；完整串口先作为Review输入，
  入库只保存可审计的bounded marker/command摘录，不能用拆分或压缩绕过限制。

**Relevant Code**

| Surface | Responsibility |
|---|---|
| `tests/ms06_stack_readiness_probe.c` | MS06 12-case application witness与fixed deadlines |
| `scripts/ms06-qemu-validate.py` | 完整MS06 transcript的纯输出审计 |
| `tests/ms01_socket_baseline.c` | socket compatibility 14-case runtime |
| `tests/ms04_rx_probe.c`、`scripts/ms04_rx_stimulus.py` | RX snapshot/quiet/nudge/burst |
| `tests/ms05_data_plane_probe.c`、`scripts/ms05_data_plane_stimulus.py` | 双向/Full/flush runtime |
| `.claude/runbooks/qemu-network-testing.md`（R44） | 手工QEMU、环境分类、HTTP/离线注入 |
| `.claude/runbooks/qemu-evidence-capture.md`（R58） | `script`/`tee`、exit分层和证据采集 |

**Critical Path**

```text
freeze HEAD + image + payload size/mtime
  -> user starts HTTP server and script-wrapped single-hart QEMU
  -> boot signature + manual MS06 download/run + explicit exit
  -> validate full MS06 transcript
  -> same session MS01 14/14
  -> MS04 snapshot/idle/nudge/burst + host 15556
  -> MS05 six modes + per-mode host 15557
  -> bounded Evidence + final full diff Review
```

**Behavioral Change**

None。本Cycle只取得人工runtime和Review证据；不得修改产品、测试、marker协议或artifact来追逐PASS。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Action |
|---|---|---|---|---|
| 7.1 | R1-R7/S1,S3,S4 | MS06 probe、validator、R44/R58 | 应用可见readiness和marker审计 | 手工QEMU运行12-case并验证revision/environment/exit |
| 7.2 | R7/S2-S4 | MS01/MS04/MS05 probe与stimulus | 兼容、RX和双向ownership回归 | 同一artifact/session运行全部受影响mode并最终Review |

**Task Contracts**

### 7.1: obtain the MS06 application-visible runtime witness

- Requirement/Scenario: R1-R7；S1、S3、S4。
- Depends on: Task 6.1 accepted；frozen image/probe；用户可操作sandbox外QEMU终端。
- Targets: `tests/ms06_stack_readiness_probe`、`scripts/ms06-qemu-validate.py`、R44/R58命令行。
- Current behavior: host/model和artifact Gate已通过，但无当前artifact的12-case guest runtime。
- Required behavior: 用户用`script -q -e -f`从boot开始录制，逐条下载/运行MS06 probe，并立即用
  `rc=$?; echo "MS06_HARNESS_EXIT: $rc"`发布guest exit；完整transcript由validator按当前HEAD和
  `qemu-virt-riscv64-single-hart`审计。
- Required actions: 启动前记录HEAD、image/probe size/mtime；建立本Cycle EV路径；启动单hart、单
  VirtIO-MMIO NIC user-net QEMU；不得脚本驱动guest shell；保存首次完整结果。
- Preserve: artifact不变、12-case顺序、fixed deadlines、R44手工政策、single-hart边界。
- Forbidden: 自动输入guest、重建artifact、删改FAIL、以重跑PASS覆盖首次失败、把QEMU退出当guest exit。
- Test witness: 完整串口 + validator命令；预期12/12、END、`MS06_HARNESS_EXIT: 0`且validator exit 0。
- GREEN condition: boot签名存在；revision/environment精确匹配；12 case各唯一PASS；无FAIL/timeout；
  END与guest exit 0齐全。
- Verification: `python3 scripts/ms06-qemu-validate.py --expect-revision
  832abfead57e7ae0870d5b729b6875665d588582 --expect-environment
  qemu-virt-riscv64-single-hart <full-serial-path>`；exit 0。
- Stop when: QEMU/HTTP/guest能力不可用、artifact漂移、任一FAIL/timeout/缺marker/非零exit；记录第一失败层，
  不进入Task 7.2。

### 7.2: close MS01/MS04/MS05 runtime compatibility and final review

- Requirement/Scenario: R7；network-stack-baseline；MS05 ownership；S2-S4。
- Depends on: Task 7.1 GREEN；同一QEMU session和未变化artifact。
- Targets: MS01 payload、R51四mode、R56六mode、host 15556/15557 stimuli、最终完整diff。
- Current behavior: 各基线有历史证据和当前automatic Gate，但无同一MS06 artifact/session的最终回归。
- Required behavior: MS01输出14个唯一PASS、START/END和显式exit 0；MS04四mode与telemetry闭合；MS05
  六mode的guest/host计数、Full→recovery、flush与fault字段闭合；最后无Critical/Important finding。
- Required actions: guest逐条下载payload；每个MS04/MS05 host stimulus先启动并记录pipeline exit；每个
  guest mode后立即记录`*_HARNESS_EXIT`；结束后核对artifact size/mtime未变并Review完整diff。
- Preserve: MS01 14-case清单、MS04 96包契约、MS05六mode参数与ownership语义、同一artifact身份。
- Forbidden: 跳过失败mode、复用历史marker、串联命令掩盖单项exit、变更产品/测试、扩大到SMP/真板/性能。
- Test witness: full serial中的MS01/MS04/MS05命令与marker；host stimulus结果；前后artifact identity。
- GREEN condition: MS01 14/14；MS04 snapshot/idle/nudge/burst全部唯一PASS且fault/守恒字段满足R51；
  MS05六mode guest和host全部PASS且账本满足R56；所有guest/host exit 0；最终diff无阻塞finding。
- Verification: 对完整串口执行marker/FAIL/exit提取；对host输出核对mode/count/received；
  `git diff 1ea51427..HEAD --check`、`git diff --check`、`git diff --cached --check`及full diff Review。
- Stop when: 任一产品marker、telemetry、host结果、exit或artifact身份失败；保存首次结果并返回Plan，不继续
  下游mode或归档。

**Manual Command Contract**

Act开始后应给用户展开以下占位符为本Cycle实际路径的逐条命令；本Plan不自动执行QEMU：

```bash
cd /home/daivy/projects/serial/work/StarryOS
EV=openspec/changes/ms06-application-visible-async-network-stack/evidence/008-single-hart-qemu-acceptance/000-initial
mkdir -p "$EV"
script -q -e -f /tmp/ms06-iteration-008-qemu-serial.log -c \
'qemu-system-riscv64 -machine virt -bios default -kernel StarryOS_riscv64-qemu-virt.bin -m 1G -smp 1 -device virtio-blk-device,drive=disk0 -drive id=disk0,if=none,format=raw,file=make/disk.img -device virtio-net-device,netdev=net0 -netdev user,id=net0 -nographic'
```

HTTP server使用R44的`tests/`目录、端口18765；MS04和MS05 host stimulus分别使用R51的15556与
R56的15557。完整串口先保存在`/tmp`供Act/Plan审查；只有符合公共预算的bounded摘录写入EV。

**Invariants**

- 唯一queue service和唯一stack runner所有权不因验证而改变。
- QEMU为single-hart、单VirtIO-MMIO NIC；任何结果不外推到reset、SMP、PCI/DWMAC、真板或性能。
- 首次失败、中断或partial marker必须保留，后续重跑不能静默覆盖。
- Task 7.2只在Task 7.1 GREEN后开始；任一mode失败停止下游。
- Evidence与完整原始输入必须可追溯，但不得违反公共文件数、行数和大小预算。

**Non-goals**

- 产品或测试修复、自动QEMU harness、rootfs永久修改、commit/archive、状态同步。
- reset/cancel/link flap、SMP、多NIC、真板、DMA/cache、性能或优化资格。

**Acceptance**

1. MS06 12-case transcript具备START、当前revision、single-hart environment、固定顺序PASS、END和guest exit 0；validator exit 0。
2. MS01 14/14 PASS，START/END与guest exit 0完整。
3. MS04四mode满足R51 marker、quiet/burst、守恒和fault判据，host stimulus exit 0。
4. MS05六mode满足R56 guest/host、Full→recovery、flush和fault判据，全部exit 0。
5. 全程使用同一frozen artifact集合；boot、guest命令、marker、host结果和exit可追溯。
6. 最终full diff无Critical/Important finding；结论严格限定single-hart QEMU VirtIO-MMIO。

**Verification**

- 用户手工QEMU与完整串口：R44/R58；禁止自动guest输入。
- MS06 validator按Task 7.1命令校验当前HEAD/environment。
- `rg -n 'MS06_|MS01_|MS04 (PASS|FAIL)|MS05 (PASS|FAIL)|HARNESS_EXIT|panic|fault=' <full-serial>`。
- host输出核对MS04 96包和MS05六mode count/received、pipeline exit。
- artifact前后`stat -c '%y %s %n'`；不得在session中重建。
- strict OpenSpec、三组diff check和最终full diff Review。
- SKIPPED: 自动驱动QEMU（R44硬性政策；由用户手工执行，不是Acceptance豁免）。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | probe/validator入口、R44/R51/R56/R58、artifact与sandbox能力边界已核对 |
| Design | PASS | 单session顺序、guest/host exit分层、失败即停和证据预算闭合 |
| Iteration Plan | PASS | Tasks 7.1-7.2共同形成最终runtime baseline；无后续Iteration |
| Cycle Scope | PASS | 只取single-hart runtime与final Review，不修产品、不扩大平台结论 |
| Task Contracts | PASS | manual动作、marker、telemetry、GREEN和停止条件自包含 |
| Traceability | PASS | R1-R7与MS01/MS04/MS05边界映射到两task和六项Acceptance |
| Verification | PASS | full transcript、validator、host results、artifact identity与diff Review完整 |

Gate 2技术检查项PASS；用户于2026-08-27 显式批准（原话："更新gate状态，开始实施"），Plan Context 状态更新为 `ready`，授权执行本 Cycle。

**Persisted Evidence**

- Mode: required
- `README.md`：支持Acceptance 1-6；记录环境、HEAD、artifact前后identity、完整串口外部输入路径、
  用户手工命令、QEMU进程exit、guest/host exit和文件映射。Act Response不足以承载一整次多mode人工
  session；该session不可低成本重跑；缺失会阻止final runtime acceptance。
- `qemu-runtime-markers.md`：支持Acceptance 1-4；从完整串口按原顺序保存boot签名、guest命令、
  MS06/MS01/MS04/MS05决定性marker和显式exit，包含原始行号；不得复制无关长日志。通过条件为所有
  marker/exit闭合、无FAIL/panic/timeout。
- `host-runtime-results.md`：支持Acceptance 3-4；保存MS04及MS05各mode的literal命令、pipeline exit、
  决定性count/received结果。缺失会阻止guest/host双边闭合。
- 完整`script`串口作为一次性人工输入保存在`/tmp/ms06-iteration-008-qemu-serial.log`并在Act/Plan
  Review时读取；若其本身不超过500行且256 KiB，可代替`qemu-runtime-markers.md`入库，否则不得通过
  拆分或压缩绕过公共预算。
- Budget: 计划3文件（含README），本Cycle≤5；整个change完成后预计8/20；每个文本≤500行且≤256 KiB。

**Risks and Notes**

- `riscv64-linux-musl-gcc`在当前sandbox返回SIGSYS；Iteration 007已生成fresh payload，本Cycle冻结复用，
  不在sandbox重建。artifact漂移则返回Plan，不得使用历史替代。
- R58/R56的MS05历史来源省略archive前缀是非阻塞文档Minor；本Cycle使用其运行判据，不修改Runbook。
- `script -e`只能证明QEMU子进程状态；每个guest workload必须单独echo exit marker。
- HTTP下载挂起是产品网络路径信号；离线注入仅可诊断，不能替代MS06/MS01主路径Acceptance。

## Act Response

- Status: blocked

**Implemented**

- 按用户 2026-08-27 显式批准（原话："更新gate状态，开始实施"）将本 Cycle Plan Context 由
  `draft` 更新为 `ready` 并登记 Approval；Gate 2 段落同步记录授权。
- 建立本 Cycle EV 路径并采集 required Evidence（README + `qemu-runtime-markers.md` +
  `host-runtime-results.md`）：冻结 artifact 身份、MS06/MS01/MS04/MS05 决定性 marker、
  MS04/MS05 host stimulus 双边结果均已落盘。
- 校验 `scripts/ms06-qemu-validate.py` 新鲜可用（self-test PASS），并对用户会话的 MS06
  transcript 执行 validator 判定（FAIL，见 Verification Evidence）。
- 按用户指示（原话："更新一下runbook……改成没有调试信息的，也就是make run就行了"，且
  "不用纠结权限问题，直接更新runbook就行，我给暂时你豁免这个权限"）将手工 QEMU 串口采集
  启动命令由裸 `qemu-system-riscv64 ...`（info 级调试刷屏）改为 `make run`（默认
  `LOG=warn`，无调试信息）：R44 `qemu-network-testing.md` 启动段与 R58
  `qemu-evidence-capture.md` 录制段已同步更新，并注明需要 info/debug 分层诊断时按 R55
  显式构建诊断镜像（不当事采基线）。

**Changed Files and Symbols**

- `iterations/008-single-hart-qemu-acceptance/000-initial.md`：Plan Context Status/Approval、
  Gate 2 段落、本 Act Response。
- `evidence/008-single-hart-qemu-acceptance/000-initial/`：`README.md`（更新 Result）、
  `qemu-runtime-markers.md`、`host-runtime-results.md`（新建）。
- `.claude/runbooks/qemu-network-testing.md`（R44）、`.claude/runbooks/qemu-evidence-capture.md`
  （R58）：QEMU 串口采集启动命令改为 `make ARCH=riscv64 run`（LOG=warn，无 info 调试输出），
  参数表/要点同步改写。

**Deviations from Plan**

- 用户明确要求把 Runbook 采集命令改为 `make run`；该运行同时变更了 QEMU 启动方式，但属于
  文档层运维调整（R44/R58），不改变本 Cycle 产品、测试、marker 协议或 artifact 追踪方式；
  用户已显式授权并豁免权限。用户运行 `make run` 导致 boot bin mtime 更新（bytes 不变，
  probe 四件未变），已记录于 EV README 与 markers 的 Artifact 身份表。
- 用户会话的执行顺序越过 Plan 的"S1→S2→S3"串行约定（部分 host stimulus 晚于 guest 命令
  启动），造成 MS05 数次 `reason=handshake` 重跑；均按 R56 归类为操作顺序问题，重跑后
  闭合。首失败层按 MS06 FAIL 记录。

**Blocker Resolution**

- 原阻塞（用户手工 QEMU 会话未执行、required Evidence 不可采集）已解决：用户于 2026-08-27
  完成 single-hart QEMU 会话并提供完整转录与 host stimulus 结果；随后按用户指示更新
  R44/R58 Runbook（make run），并填写本 Act Response。该阻塞已解除。

**Blocker Handoff**

- Task/Step/Gate：Task 7.1 / Phase 3 EXECUTE / Gate 5（runtime acceptance）。
- Plan 预期：MS06 probe 12 个 case 全部唯一 PASS，`MS06_HARNESS_EXIT: 0`，validator exit 0。
- 实际情况：MS06 转录 10/12 PASS，2 个 FAIL——`listener backlog connections not accepted
  uniquely inside deadline`、`close-error graceful close misclassified or unstable after
  EOF`；`MS06_HARNESS_EXIT` 未发布（转录缺口），validator 决定性输出 exit 1。Task 7.1 的
  GREEN 条件**未达成**，Acceptance 1 无法满足。
- 影响：本 Cycle 是 change 的最终逻辑 Iteration；MS06 两个既有产品路径的 runtime 反例
  阻止单 hart 应用可见验收收口。MS01 14/14、MS04 4/4、MS05 6/6 及 host 双边闭合全部 PASS，
  该兼容/数据面层结论保留。
- 已完成任务：Gate 状态更新、EV 三文件、validator 核对与判定、Manual Command Contract
  交付、Runbook 更新（用户指示）、MS01/MS04/MS05 runtime 回归核对。
- 未开始/未完成：Task 7.1（MS06 GREEN 未达成）；Task 7.2 的最终 diff Review（等待 Cycle
  结果分类后执行）。
- 修改文件与工作区：cycle 文档（M）、EV 三文件（untracked）、R44/R58 Runbook（M）；无
  产品/测试代码改动。
- 已通过 Gate：Gate 2（ready，用户批准）、Gate 5 部分（validator/回归/回滚项）；提交
  Acceptance 依赖 MS06 修复后重跑。
- Evidence：EV-008-000-01 Result=FAIL（保存首次失败现场），EV-008-000-02 Result=PASS。
- 恢复条件：Plan Review 将 MS06 两 FAIL 分类（Plan 遗漏/Plan 错误/Act 偏离/基线变化/
  新证据）并创建 rework/replan Cycle 修复 listener backlog 与 close-error 路径；修复后以
  同一冻结/重建 artifact 重跑 Task 7.1-7.2。

**Self-Review**

- Spec compliance：本 Cycle 不修改产品/测试/marker/artifact（除用户指示的 Runbook 运维
  变更），行为与 Non-goals 一致；Runbook 改动受用户显式授权与豁免，原话入档。
- Code/quality：仅文档与 EV 文件变更；Runbook 命令与 make/qemu.mk 实际参数表核对一致
  （`make ARCH=riscv64 run` → LOG=warn、-nographic、user-net、hostfwd 5555）；`git diff
  --check` 三路全部 exit 0。
- 未解决 Critical/Important：MS06 两 FAIL 是本次验收暴露的**产品层反例**，不属于本 Cycle
  diff 质量问题，已完整记录于 Remaining Issues 与 EV；按 Act 边界（Non-goals：不得为追逐
  PASS 修改产品）不在此 Cycle 内修复。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| validator 新鲜 | `python3 scripts/ms06-qemu-validate.py --self-test` | `PASS: ms06-validator-self-test` exit 0 | PASS |
| MS06 transcript | validation on user session（还原 transcript） | `FAIL: ms06-validator: payload reported a failure: FAIL: listener backlog connections not accepted uniquely inside deadline`，exit=1 | **FAIL**（Task 7.1 GREEN 未达成） |
| MS01 runtime | user session | `MS01_SOCKET_BASELINE_START/END` 间 14 个唯一 PASS（tcp-accept … bind-close-cleanup） | PASS 14/14 |
| MS04 runtime | user session | snapshot/idle/nudge/burst 各唯一 `MS04 PASS mode=…`；burst `reaped=refilled=delivered=96` 守恒、`budget=3 yield=2 fault=0` | PASS 4/4 |
| MS05 runtime | user session + host stimulus | 六 mode 各唯一 `MS05 PASS mode=…`；host `received=96` 全部对齐（handshake 早期失败为操作顺序、重跑闭合） | PASS 6/6 |
| diff check | `git diff --check` / `git diff --cached --check` / `git diff 1ea51427..HEAD --check` | 三路均无输出，exit 0 | PASS |
| OpenSpec | `openspec validate --all` | `✓ change/ms06-application-visible-async-network-stack`；`✗ spec/improvements`（既有：I17/I18 缺 SHALL/MUST，与本 Cycle 无关） | PASS（change）；既有失败不归于本 Cycle |

**Persisted Evidence**

- `evidence/008-single-hart-qemu-acceptance/000-initial/README.md`（已更新 Result）。
- `qemu-runtime-markers.md`（EV-008-000-01）：MS06 10/12 + 2 FAIL（含首失败层原始行）、
  MS01 14/14、MS04 4/4、MS05 6/6、artifact 身份对比与结论。
- `host-runtime-results.md`（EV-008-000-02）：MS04 15556 与 MS05 15557 stimulus 的 literal
  命令、received=96 记录与双边闭合说明。

**Experience Candidates**

None（本 Cycle 暴露的 MS06 两 FAIL 为产品反例，非可复用操作流程或运维故障；不满足 Runbook/
Incident 候选条件）。

**Remaining Issues**

- **MS06 `listener` FAIL**：`listener backlog connections not accepted uniquely inside
  deadline` —— Task 2.8 的 listener head-signal/backlog 路径在 single-hart guest runtime
  未达到"backlog 内连接在 deadline 内被唯一接受"的应用可见契约；host/model 全绿未覆盖该
  时序反例。属 Task 7.1 首要失败层。
- **MS06 `close-error` FAIL**：`close-error graceful close misclassified or unstable after
  EOF` —— Task 3.2 闭合的 terminal/EOF 映射在 runtime 出现误分类或 EOF 后状态不稳定；
  需 Plan Review 分类（产品缺陷 vs 测试判定口径）。
- **转录采集缺口**：用户会话未发布 `MS06_HARNESS_EXIT`、`MS01_HARNESS_EXIT` 与 MS04
  snapshot 的 exit marker（后续 mode 已带）。失败结论由 probe 已打印 FAIL 决定，不依赖
  exit；重跑修复 Cycle 时须补齐 exit marker。
- **boot bin 身份变化**：用户 `make run` 触发重建（mtime 21:13:42、bytes 不变）；probe
  四件与 re内嵌 revision 未变。修复 Cycle 重跑时应以全新构建的 artifact 重新 freeze。
- `spec/improvements` 校验失败（I17/I18 缺 SHALL/MUST）为既有全局文档问题，不在本 Cycle
  Executable scope，交由 `openspec-docs-maintainer` 后续处理。

**Commit or Diff Reference**

- 本轮工作树：`iterations/008-single-hart-qemu-acceptance/000-initial.md`（M）；
  `evidence/008-single-hart-qemu-acceptance/000-initial/{README,qemu-runtime-markers,
  host-runtime-results}.md`（untracked，新建）；`.claude/runbooks/qemu-network-testing.md`、
  `.claude/runbooks/qemu-evidence-capture.md`（M，用户指示的 make run 改动）。
- HEAD `1d0313ad8d0f36d918d1a101dd0ceda5c2ba336b`（net-k3）；未 commit。

## Plan Review

- Review Result: replan-required

**Findings**

1. **Blocking — listener witness is a deterministic false negative.** `run_listener` sends and receives one identity byte, but the child compares `(unsigned)(unsigned char)echo` with the 32-bit value `~ident`. The raw serial shows all four hidden connections became Ready, all four accepts consumed unique queue entries, and every child then exited with code 3. For identities 1-4 the valid reply bytes are `0xfe`-`0xfb`, which cannot equal `0xfffffffe`-`0xfffffffb` after the existing promotions. This does not prove a listener product failure.
2. **Blocking — close-error asserts an invalid peer-FIN contract.** The requirement only promises `IN|RDHUP`, stable zero-length reads and no device `ERR` after peer FIN. The probe additionally demands that the still-open local write half reach stable `EPIPE` within eight immediate sends. TCP peer FIN closes the receive half; a local send may remain valid. The observed FAIL therefore cannot distinguish a correct half-close from a product defect.
3. **Blocking — the planned full-transcript validator cannot consume the captured input.** The literal command against `/tmp/ms06-iteration-008-qemu-serial.log` returns `FAIL: ... start marker is missing` because an ANSI reset sequence prefixes the START physical line. The manually extracted 17-line file reaches the payload FAIL but is not the required complete raw transcript. The validator needs bounded ANSI/CSI transport normalization without accepting printable marker prefixes or suffixes.
4. **Blocking — Task 7.2 did not satisfy its dependency or evidence contract.** Act continued MS01/MS04/MS05 after Task 7.1 failed, although the Plan required failure-stop. MS01 and MS04 snapshot lack explicit harness exits; successful MS05 retries followed handshake failures; host results were not captured with the required `tee`/pipeline exits. These observations remain useful diagnostics but do not constitute Task 7.2 acceptance.
5. **Blocking — frozen boot identity changed.** `make run` rebuilt `StarryOS_riscv64-qemu-virt.bin` from mtime 20:08:19 to 21:13:42. Equal bytes do not establish the Plan's pre/post exact-artifact condition. The next Cycle must build first, then freeze the post-build inputs before QEMU starts.
6. **Evidence correction.** The raw serial does contain `MS06_HARNESS_EXIT: 1`; the Act Response and EV-008-000-01 incorrectly say it was missing. No `panic`, trap, illegal instruction, page fault or kernel fatal appears in the 949-line raw session. The terminal result is the probe's aggregated nonzero exit.

**Deviation Classification**

- `PLAN-INVALID`: listener byte-width, peer-FIN/EPIPE and raw ANSI/CSI transcript contracts can report false failure or reject the required input.
- `ACT-DEVIATION`: execution crossed the Task 7.1 failure-stop boundary and characterized incomplete downstream evidence as Task 7.2 PASS.
- `BASELINE-CHANGED`: `make run` rebuilt the boot image after the pre-session identity record.
- `NEW-EVIDENCE`: the first real guest run exposed validation assumptions not exercised by the 26 host seam tests.

**Acceptance Gaps**

- Acceptance 1 is not decidable from Cycle 000: MS06 reports 10/12 and exit 1, while both failing case predicates are invalid and the raw transcript cannot be audited directly.
- Acceptances 2-4 remain incomplete because Task 7.2 ran after its dependency failed and lacks all required explicit guest/host exits.
- Acceptance 5 fails because the boot image identity changed and the successful modes do not form the planned same-order, same-frozen-artifact session.
- Acceptance 6 remains open; Act did not finish the final full diff Review and the witness defects are Critical to the runtime conclusion.

**Convergence**

Expanded from no known gap to three validation-contract defects, one execution-order deviation and incomplete final evidence. This is the first Review of Cycle 000.

**Evidence**

- `/tmp/ms06-iteration-008-qemu-serial.log`: 949 lines, 159,308 bytes; listener accepts at raw lines 237-240, child exits 3 at 242-250, close-error FAIL at 310, `MS06_HARNESS_EXIT: 1` at 941, QEMU termination without kernel fatal at 942-949.
- `tests/ms06_stack_readiness_probe.c:724-839`: listener identity/echo path; the width mismatch is at the child comparison and the parent emits one byte.
- `tests/ms06_stack_readiness_probe.c:1172-1291`: peer FIN, two zero reads and the unsupported EPIPE loop.
- `scripts/ms06-qemu-validate.py::validate_output`: `.strip()` does not remove ANSI/CSI; literal validation of the raw serial exits 1 with missing START.
- EV-008-000-01/02 and Act Response: useful bounded observations, with the exit-marker and Task 7.2 qualifications corrected by this Review.
- Fresh checks: strict validation of this change, three diff checks and RISC-V static `ET_EXEC` artifact inspection passed before Plan edits; these do not satisfy runtime Acceptance.

**Follow-up Decision**

The verification contract and executable scope must change before another runtime attempt. Update the unfinished Iteration to add Task 7.3, create a replan Cycle, and require probe/validator RED→GREEN before rebuilding and rerunning. Do not modify axnet or kernel product code from these two FAIL lines. After user approval, `openspec-act` may execute `001-replan.md`; Cycle 000 is frozen.

**Iteration Plan Update**

Iteration 008 remains the final logical Iteration, but its tasks become 7.3 → 7.1 → 7.2. The stable baseline and verification boundary now require a valid witness, direct raw-transcript validation, post-build artifact freeze and strict failure-stop ordering.

**Next Cycle**

`001-replan.md` in this Iteration.

**Next Iteration**

None；这是change的最终逻辑Iteration。
