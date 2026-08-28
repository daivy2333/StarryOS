# Iteration 007 / Cycle 000: qualify automatic integration and fresh artifacts

## Plan Context

- Status: ready
- Approval: 用户于 2026-08-27 显式批准并授权执行（原话一："更改gate状态，开始实施吧"；原话二（回退后继续）："我们继续实施，这次我们再次一次性执行三十次，看看是否有类似错误"）
- Iteration: 007-automatic-integration-qualification
- Cycle: 000-initial
- Cycle Type: initial
- Parent iteration: `006-axnet-host-test-isolation`

**Iteration Scope**

- Change tasks: 6.1
- Depends on: Iteration 006 accepted
- Stable baseline: 全部自动功能、ownership、兼容、build、format、OpenSpec和diff Gate通过；QEMU image与
  MS01-MS06 probe artifacts由当前working tree重新生成，可作为Iteration 008人工runtime唯一输入。
- Verification boundary: Task 6.1命令有明确exit/expected-result；default-parallel host suites无flake豁免；
  D1既有负基线由自动auditor精确判定；full diff无Critical/Important finding。
- Diagnostic boundary: axnet/smoltcp、driver、kernel feature build、MS01/MS04/MS05/MS06 host seam、artifact
  build、format/source guard、OpenSpec或diff；首次失败即停在对应层。
- Deferred tasks: Iteration 008 Tasks 7.1-7.2

**Cycle Scope**

- Trigger: Iteration 006 Cycle 001 Review `accepted`
- Acceptance gaps: 尚无当前working tree的一次完整automatic qualification和fresh artifact集合
- Inherited scope: Tasks 1.1-5.2已接受实现；R1-R7；D1-D11；MS05 automatic Gate先例；MS06 12-case
  validator/probe；default-parallel host isolation
- Excluded scope: 启动或操作QEMU、人工guest marker、修改产品行为以追逐Gate、known-flake豁免、串行full
  suite、reset/SMP、真板、性能、归档、全局状态同步和commit

**Objective**

在不修改产品实现的前提下，对当前完整working tree执行一次依赖有序的automatic qualification。只有所有
自动Gate通过并重建出当前revision artifacts，才允许Iteration 008开始single-hart QEMU runtime。

**Scenario Sketch**

| Scenario | Precondition | Action | Observable result | Failure boundary |
|---|---|---|---|---|
| S1 host stack | Iteration 006 accepted | default-parallel ordinary/diagnostics suites与100×竞争见证 | 无panic、signal、flake或串行豁免 | axnet/smoltcp/fixture/lock/wake |
| S2 compatibility seams | MS01-MS06 sources为当前树 | 运行host-test、driver suites、validator与probe seams | ownership、snapshot、idle/nudge/burst、Full/flush和12-case协议全绿 | 对应MS host/model层 |
| S3 product builds | host/model已通过 | check qemu kernel及受支持D1 contract | qemu exit 0；D1既有20×E0432/5×E0433由auditor精确接受，任何漂移失败 | kernel/feature graph |
| S4 fresh artifacts | source与revision冻结 | 强制重建QEMU image及MS01-MS06静态probe | 产物存在、非空、架构正确，MS06 embedded revision等于当前HEAD | toolchain/build/revision |
| S5 quality closeout | 所有功能Gate通过 | fmt、source guards、strict OpenSpec、diff/full Review | exit 0且无Critical/Important finding | 首个格式/spec/diff finding |

**Current Baseline**

- Branch `net-k3`；HEAD `832abfead57e7ae0870d5b729b6875665d588582`（MS06第八次提交）加当前
  Runbook/reference工作树。
- Iteration 006最终graph：ordinary 371/371 ×3、qemu-diagnostics 393/393 ×3；focused/regression两profile
  各×100。`cargo tree`确认test graph不启用smoltcp defaults，normal edge不启用`test-seeds`。
- 已accepted的Iterations 000-004以`1ea51427`为最近复用基线；另有Runbook、reference与当前Cycle文档的
  working-tree调整。Iteration 007必须同时审查`1ea51427..HEAD`和当前staged/unstaged diff，不能只看
  HEAD-to-working-tree或本Cycle增量，也不重复清理更早已accepted提交中的vendored空白。
- 新鲜只读基线：smoltcp lib、axnet lib、`starry-kernel --features qemu` checks exit 0；strict OpenSpec与
  `git diff HEAD --check`通过。
- D1 direct check当前仍exit 101，精确20个E0432与5个E0433，为MS05 capture/audit已登记负基线；本Cycle
  不把它伪装成产品PASS，也不扩展为D1修复。automatic auditor exit 0且计数完全匹配才算D1 Gate通过。

**Current-State Evidence**

- root `Makefile::host-test`顺序运行MS04 harness/stimulus、MS05 probe/stimulus/evidence tools和MS06
  validator、C syntax、decision tests、case-set diff及purity/source guards；它不启动QEMU。
- `scripts/ms05_evidence_capture.py::GATES`已列axnet两profile、driver/virtio/uart、MS03/MS04、MS05 tools、
  100×race、qemu kernel、D1 signature、image/payload build、rustfmt和diff Gate；D1由kind=`d1`精确计数。
- `Makefile`提供MS04/MS05/MS06 RISC-V static targets；MS01由既有musl命令构建。MS06 target把
  `git rev-parse HEAD`写入`MS06_REVISION_DEFAULT`。
- `scripts/ms06-qemu-validate.py`是纯输出审计器；Iteration 007只运行self-test并构造artifact，不给它runtime
  transcript。QEMU输入和12-case marker属于Iteration 008。
- 当前工作树已有历史artifact不构成证据；必须`-B`或先验证依赖后强制重建，并记录revision、size与`file`。
- Runbook调整只影响Iteration 008的手工采集方式，不改变本Cycle禁止启动QEMU的边界；作为当前工作树文档，
  仍进入full diff Review。Cycle级Evidence路径、`pipefail`和QEMU/guest exit分离必须保持一致。

**Critical Path**

```text
trusted Iteration 006 host graph
  -> axnet/driver/compatibility host Gates
  -> 100x wake/lock/ownership witnesses
  -> qemu + audited D1 build contracts
  -> force rebuild image and MS01-MS06 artifacts from current tree
  -> fmt/source/OpenSpec/diff/full Review
  -> Iteration 008 manual-QEMU authorization boundary
```

**Behavioral Change**

None。此Cycle只验证和构建；不得为使Gate通过而修改产品代码、测试断言或既有协议。任何真实失败返回Plan归因。

**Change Surface**

| Task | Surface | Responsibility | Planned action |
|---|---|---|---|
| 6.1 | axnet/smoltcp/driver Cargo suites | host功能、ownership与并行确定性 | 默认线程运行并重复指定竞争见证 |
| 6.1 | `Makefile::host-test`, MS01-MS06 seams | compatibility与协议 | 执行全部host/model/validator Gate |
| 6.1 | kernel qemu/D1 feature checks | 产品feature build | qemu正Gate；D1精确负基线经auditor判定 |
| 6.1 | QEMU image、MS01-MS06 targets | Iteration 008输入 | 从当前tree强制重建并检查revision/架构 |
| 6.1 | rustfmt/source/OpenSpec/git diff | closeout质量 | 全量检查与独立full diff Review |

**Task Contract**

### 6.1: run automatic qualification and freeze fresh runtime inputs

- Requirement/Scenario: R1-R7；D1-D11；S1-S5。
- Targets: Task 6.1所列Cargo/Make/Python/C/format/OpenSpec/diff命令与artifacts。
- Current behavior: 各前序Iteration已有分层GREEN，但尚未在当前最终tree上执行一套完整、有序、无豁免的
  automatic Gate；现存artifact可能来自旧revision。
- Required behavior: 先通过host功能与竞争Gate，再通过build/compatibility，最后重建artifact和质量Review。
  每项记录命令、关键输出和exit；一个失败阻止下游artifact/runtime资格。
- Witness: 复用已接受的behavioral tests作为回归见证；本Cycle不新增产品测试。artifact freshness以强制重建、
  当前HEAD revision、`file`/nonempty和validator case-set一致性见证。
- Preserve: default-parallel调度、测试assertion、产品features、D1已登记signature、marker协议、artifact路径和
  Iteration 008人工边界。
- Forbidden: `--test-threads=1` full suite、ignore/skip、失败后无限重跑、只跑失败项后宣称full PASS、使用历史
  artifact、启动QEMU、修改产品/测试/spec来消除RED、把D1 raw exit 101误报为普通PASS。
- GREEN condition: 所有正Gate exit 0；D1 raw结果精确匹配既有signature且auditor exit 0；fresh artifacts
  完整；full diff无Critical/Important finding。
- Stop when: 任一产品、compile、assert、ownership、artifact或Review failure；记录首个失败层并返回Plan，
  不继续QEMU runtime，也不在qualification Cycle直接修代码。

**Invariants**

- 自动Gate失败是诊断输入，不是本Cycle修改授权。
- ordinary/diagnostics axnet full suites保持默认并行且使用产品等价smoltcp graph。
- artifact必须在全部前置Gate通过后从当前tree生成；历史文件存在不构成freshness。
- D1结论只覆盖既有audited signature，不声明D1产品build成功。

**Non-goals**

- QEMU启动、guest shell、runtime transcript与marker判定；属于Iteration 008。
- 修复任何新发现、改变D1支持状态、reset/SMP、真板、性能、commit或archive。

**Acceptance**

1. axnet ordinary/diagnostics default-parallel full suites、指定100×竞争/ownership见证全部通过，无flake豁免。
2. MS01 socket build、MS04 snapshot/idle/nudge/burst、MS05 bidirectional/Full/flush及MS06 seam/validator Gate通过。
3. driver/virtio/uart、smoltcp/axnet、root qemu checks通过；D1 signature由auditor精确接受且无漂移。
4. QEMU image及MS01-MS06 artifacts从当前tree强制重建，存在、非空、架构/revision符合预期。
5. fmt、source assertions、strict OpenSpec、`1ea51427..HEAD`提交范围与当前working-tree diff check，以及
   两部分合并后的full diff Review通过。
6. 未启动QEMU、未串行full suite、未使用旧artifact、未修改产品实现；无Critical/Important finding。

**Verification**

- `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib`及加
  `--features qemu-diagnostics`，默认线程；运行Task 6.1指定100×race/ownership集合。
- `make host-test`；axdriver_net、axdriver_virtio net、virtio-drivers alloc、uart async与相关smoltcp tests。
- MS05 capture/audit声明的automatic Gate，或逐项等价命令；D1必须保留raw exit/count和auditor exit。
- `cargo check --locked --offline -p starry-kernel --features qemu`；产品smoltcp/axnet checks。
- 强制构建QEMU image、MS01、MS04、MS05、MS06 probes；记录`git rev-parse HEAD`、`file`、size与MS06
  embedded revision。MS02/MS03按既有automatic bundle一并重建。
- `cargo fmt`/`rustfmt --check`适用范围、Makefile source guards、strict OpenSpec、
  `git diff 1ea51427..HEAD --check`、working-tree/staged check和两部分full diff Review。
- SKIPPED: QEMU runtime及MS01/MS04/MS05/MS06 guest markers；属于Iteration 008授权边界。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | Make host-test、MS05 Gate清单、kernel feature checks与artifact targets已定位 |
| Design | PASS | 依赖有序automatic Gate；失败即停；artifact只在前置通过后重建 |
| Iteration Plan | PASS | Task 6.1单独形成runtime前资格；Iteration 008保留人工QEMU |
| Cycle Scope | PASS | 只验证/构建，不改产品、不启动QEMU |
| Task Contract | PASS | 正Gate、D1负基线、artifact freshness、禁止项与停止条件明确 |
| Traceability | PASS | R1-R7映射到host、competition、build、artifact和quality五层 |
| Verification | PASS | default suites、100×、compatibility、features、fresh artifacts和full Review完整 |

Gate 2技术检查项PASS；状态改为`ready`；用户于 2026-08-27 显式批准（原话一
"更改gate状态，开始实施吧"；原话二"我们继续实施，这次我们再次一次性执行三十次，
看看是否有类似错误"）并授权 `openspec-act`。

**Persisted Evidence**

- Mode: none

命令与结果可重跑；Act Response记录每项命令、决定性输出、exit、artifact revision/size和Review。现有MS05
capture工具可生成其自身Evidence，但本Cycle不复制或新建MS06 Evidence占位目录。

**Risks and Notes**

- automatic suite较长；失败后只允许在修复或环境恢复后重新执行完整受影响Gate，不得无限重跑找GREEN。
- D1 raw check为预期101；只有精确20×E0432/5×E0433且auditor成功才算受支持contract PASS。
- cross-musl工具链或image构建能力缺失属于capability blocker；必须记录命令和首个缺失层，不能复用旧artifact。
- 当前Runbook/reference变更不授权本Cycle执行QEMU；只作为full diff文档质量输入，Iteration 008展开时再读取
  R44/R58的手工流程。

## Act Response

- Status: reported

**Implemented**

None；本 Cycle 只执行 qualification 与 artifact 构建。Gate 2 由用户于 2026-08-27
显式批准并授权（原话一："更改gate状态，开始实施吧"）。

**Changed Files and Symbols**

- `iterations/007-automatic-integration-qualification/000-initial.md`：
  Plan Context `Status: draft`→`ready`，`Approval` 与 Gate 2 Readiness 行记录用户批准
  （含回退后续批原话）。
- `openspec/changes/.../evidence/007-automatic-integration-qualification/000-initial/`
  （README.md + blocker.md + recheck-30x.md）：阻塞现场与 30× 复核记录（EV-007-000-01
  / EV-007-000-02）。
- fresh artifacts：`StarryOS_riscv64-qemu-virt.bin`、`tests/ms01_socket_baseline`、
  `tests/ms02_guest_service`、`tests/ms03_irq_probe`、`tests/ms04_rx_probe`、
  `tests/ms05_data_plane_probe`、`tests/ms06_stack_readiness_probe`（本 Cycle 重建）。
- 一次性环境工具 `/tmp/opencode/cc-nopie.sh`（K44 非 PIE linker wrapper，不入库）。

**Deviations from Plan**

- D1 (environment, K44 已登记)：axnet host 测试冷重建后 percpu 绝对重定位
  （`R_X86_64_32S`）无法链接 PIE 可执行文件；按 K44 用
  `RUSTFLAGS="-C linker=/tmp/opencode/cc-nopie.sh"`（遇 `-shared` 透传、否则补
  `-no-pie`）运行 axnet host suites。wrapper 重建于 `/tmp/opencode/cc-nopie.sh`。
  属执行环境事项，不计产品失败。
- D2 (user waiver 1)：full suite 竞争/ownership 见证由 100× 豁免为 20×（原话：
  "100次太多了，且没必要跑这么多次，豁免到20次，继续吧"）；两个 targeted 竞争测试
  仍按 100× 完整执行。
- D3 (user direction after Blocked): run 16/20 SIGSEGV 后曾按 Gate 6 置 `blocked`
  并保存 EV-007-000-01；用户回退该 Cycle 文档并指示继续（原话："我们继续实施，这次
  我们再次一次性执行三十次，看看是否有类似错误"）。执行 30× 复核：30/30 PASS、0
  crash。首次 crash 未再现；残余事件是否闭合由 Plan Review 裁决。
- D4 (working-tree 外部编辑观察，非本 Cycle 产物)：change `tasks.md` 存在未暂存编辑
  指向尚不存在的 `001-replan.md`、将 Task 5.1 重开为 `[ ]` 并标注 Gate 2 BLOCKED。
  该编辑与本 Cycle 实际执行状态（000-initial ready、Task 6.1 全部 Gate 通过）不一致；
  Act 不修改全局 tasks 状态，交 Plan Review 统一裁决。

**Blocker Handoff**

- 已消费。run 16/20 的 SIGSEGV 现场、归因诊断与恢复条件记录在
  EV-007-000-01（blocker.md）；用户回退并指示按 30× 复核后，该 Blocker 不再作为
  本 Cycle 的终止状态。原始现场与结论保留为 Evidence，不删除、不覆盖。

**Blocker Resolution**

- 用户指令（原话）："我已经把 ...000-initial.md 回退了，我们继续实施，这次我们再次
  一次性执行三十次，看看是否有类似错误"。随后执行 30× full suite 复核，
  **30/30 PASS、0 crash、无 timeout/挂起**（单次 wall 5.0–10.7s）。
- 用户豁免（原话）："我觉得这是机器偶发错误，因为触发几率很小排查困难，我们不在这里
  阻塞，更改回复为通过，记录我的原话豁免"。SIGSEGV 按用户判定为机器偶发错误，
  不作为 Task 6.1 Acceptance 1「无 flake 豁免」的阻塞项；本 Cycle 结论为**通过
  （用户豁免）**。原始现场保留在 EV-007-000-01 供追溯，不删除、不覆盖。
- 残余内存安全事件由用户明确豁免，不要求本 Cycle 继续排查或修复；若后续多次复现，
  可按 R57/Incident 流程另行归因。

**Self-Review**

- Spec compliance：未做任何产品或测试修改；验证命令均来自 Plan Verification 与既有
  GATES 清单；test witness 复用已接受 suites；未创建空 Evidence 占位；D1 负基线未
  伪装为产品 PASS。
- Code quality：N/A（无产品 diff）。
- 遗留 Minor：无已识别产品问题。残余 SIGSEGV 为需 Plan 裁决的已记录观察，非本 Cycle
  可允许的静默跳过项。

**Verification Evidence**

| 验证项 | 命令 | 输出摘录 | 结论 |
|---|---|---|---|
| ordinary full suite | `RUSTFLAGS="-C linker=/tmp/opencode/cc-nopie.sh" cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib` | `test result: ok. 371 passed; 0 failed` | PASS |
| diagnostics full suite | 同上 + `--features qemu-diagnostics` | `test result: ok. 393 passed; 0 failed` | PASS |
| race-control 100× | 同上 + `...diagnostic_control_shared_...` ×100 | `fail=0`，每迭代 1 passed | PASS |
| race-v3 100× | 同上 + `...v3_shared_snapshot_...` ×100 | `fail=0`，每迭代 1 passed | PASS |
| full-suite 20×（豁免1） | full ordinary suite ×20 | run 1–15 PASS；run 16 SIGSEGV；diag rerun PASS | 见 EV-007-000-01 |
| full-suite 30×（用户指令） | full ordinary suite ×30 | **30/30 PASS，0 crash** | PASS（EV-007-000-02） |
| 残余 SIGSEGV 处理 | 用户豁免 | 用户判定为机器偶发错误，不阻塞；原话保留于 Blocker Resolution | PASS（用户豁免） |
| make host-test | `make host-test` | ms05 negative fixtures PASS、ms06 validator/probe 26 decision tests、case-set diff 全绿 | PASS |
| driver suites | axdriver_net/axdriver_virtio(net)/virtio-drivers(alloc)/uart_16550(async) | 全部 exit 0（0/0、0/0、8、10 passed） | PASS |
| smoltcp lib | `cargo test --manifest-path crates/smoltcp/Cargo.toml --locked --offline --lib` | `test result: ok. 675 passed; 0 failed` | PASS |
| kernel qemu check | `cargo check --locked --offline -p starry-kernel --features qemu` | `Finished dev profile` | PASS |
| D1 负基线 | `cargo check -p starry-kernel --features lichee-d1` + `classify_d1` | raw exit 101；E0432=20、E0433=5；classify `pass`、无未分类 error | PASS |
| image build | `make LOG=info build` | `.bin` 40,763,584 B，RISC-V | PASS |
| payloads | MS01 musl + `make -B` ms02–ms06 | 全部静态 RISC-V ELF、非空；MS06 embedded revision = HEAD | PASS |
| rustfmt | GATES rustfmt-check 清单 | exit 0 | PASS |
| strict OpenSpec | `openspec validate ms06-application-visible-async-network-stack --strict` | `Change ... is valid` | PASS |
| diff check | `git diff 1ea51427..HEAD --check` / worktree / staged | 全部 exit 0 | PASS |

axnet host 命令按 K44 加 `RUSTFLAGS="-C linker=/tmp/opencode/cc-nopie.sh"`。

**Persisted Evidence**

- `evidence/007-automatic-integration-qualification/000-initial/README.md`
- `evidence/007-automatic-integration-qualification/000-initial/blocker.md`
- `evidence/007-automatic-integration-qualification/000-initial/recheck-30x.md`
- 编号: EV-007-000-01（act-added，SIGSEGV 现场）、EV-007-000-02（user-required，
  30× 复核）。目录共 3 文件 ≤ 5 预算；理由见 README 白名单。

**Experience Candidates**

- Candidate (Incident): "默认并行 axnet full suite 20× 窗口内出现非确定性 SIGSEGV
  (signal 11)（run 16/20），与 R57 家族一致；立即重跑通过、30× 复核未再现；概率
  < 1/31，Task 5.1 修复未在短窗口内证明覆盖"。Evidence: EV-007-000-01/02 +
  本 Act Response。未持久化；Recorder 决定。
- Candidate (Runbook): "axnet host unit suites 冷重建后需 K44 non-PIE link wrapper"
  — 已在 Iteration 004 D1 记录，本次复现重建过程一致。未持久化；Recorder 决定。

**Remaining Issues**

- 残余内存安全事件已由用户豁免为机器偶发错误（原话保留于 Blocker Resolution），
  不作为 Acceptance 阻塞项；原始现场保留于 EV-007-000-01 供追溯。
- `tasks.md` 外部编辑指向不存在的 `001-replan.md` 且重开 Task 5.1：需 Plan/Maintainer
  统一其与 Task 6.1 执行状态的关系。
- Iteration 008（Tasks 7.1–7.2 single-hart QEMU 验收）保持为下一个授权边界：需本
  Iteration 被 Plan Review `accepted` 后，按 R44/R58 手工流程展开。

**Commit or Diff Reference**

- HEAD `832abfead57e7ae0870d5b729b6875665d588582`；工作树含本 Cycle 文档、
  Runbook/reference staged 编辑、本 Cycle 重建 artifacts 与 evidence 目录；
  无产品代码 diff（`git diff 1ea51427..HEAD --check` 与全工作树 `--check` 均 exit 0）。

## Plan Review

- Review Result: accepted

**Findings**

- Blocking findings: None。
- 用户已明确豁免的风险：EV-007-000-01记录ordinary default-parallel full suite在20×窗口第16次
  SIGSEGV；EV-007-000-02随后30/30 PASS但未证明根因消失。该结果原本违反Acceptance 1的
  “无flake豁免”，用户于2026-08-27明确接受风险并要求改为通过；本Review保留原失败，不把
  30×未复现误写为根因修复。
- Minor M1（非阻塞）：新R58及R56的MS05来源指针仍省略`archive/2026-08-19-`前缀；目标Evidence
  实际存在于归档change，运行命令和当前MS06资格不受影响。由Runbook职责后续按需修正，不扩大
  本Cycle。
- Minor M2（非阻塞）：R45对`file`示例写成`statically linked`，而当前MS01-MS03 musl产物显示
  `static-pie linked`；两者均为无需动态加载器的静态RISC-V ELF，当前artifact资格成立，但示例
  可在后续Runbook维护时放宽措辞。

**Deviation Classification**

- `ACT-DEVIATION`：用户把full-suite重复见证从100×豁免为20×；Act仍完整执行两个targeted 100×
  竞争见证，随后按用户指令执行30×复核。
- `NEW-EVIDENCE`：20×窗口内出现一次未归因SIGSEGV；原始现场和30×复核分别保存在
  EV-007-000-01/02。用户显式承担残余风险，因此不再阻塞本Iteration。
- `BASELINE-CHANGED`：Act Response提到的`tasks.md`外部replan编辑已不在当前工作树；当前权威
  `tasks.md`没有`001-replan.md`引用，故该项已自然消失，无需修复或新Cycle。

**Acceptance Gaps**

- None。Acceptance 1的非确定性SIGSEGV风险由用户显式WAIVED；Acceptance 2-6均有Act记录和
  本Review的独立检查支持。

**Convergence**

N/A.

**Evidence**

- 实际代码与diff：独立检查`git diff 1ea51427..HEAD`、staged及unstaged diff；terminal fault
  first-wins/global-priority、fixture-local socket/listener/service、UDP queued-TX deferred reap、
  MS06 validator/probe及artifact构建路径未发现Critical/Important finding。
- `RUSTFLAGS="-C linker=/tmp/opencode/cc-nopie.sh" cargo test --manifest-path
  crates/axnet/Cargo.toml --locked --offline --lib`：exit 0，371 passed。
- 同命令加`--features qemu-diagnostics`：exit 0，393 passed。
- `openspec validate ms06-application-visible-async-network-stack --strict`：exit 0，change valid。
- `git diff 1ea51427..HEAD --check`、`git diff --check`、`git diff --cached --check`：均exit 0。
- `make host-test`新鲜复核在`scripts/ms04_rx_stimulus.py --loopback-self-test`因当前sandbox禁止
  AF_INET socket返回`EPERM`，属于R44定义的ENV-BLOCKED；失败发生在MS06 seam之前，不覆盖Act在
  非受限环境记录的完整PASS，也不是产品断言失败。
- artifact独立检查：QEMU image 40,763,584 bytes；MS01/MS04/MS05/MS06均为静态RISC-V ELF；
  MS06内嵌revision为`832abfead57e7ae0870d5b729b6875665d588582`。
- Persisted Evidence：`evidence/007-automatic-integration-qualification/000-initial/README.md`、
  `blocker.md`、`recheck-30x.md`；本Cycle 3文件、整个change 5文件，均在公共预算内。

**Follow-up Decision**

接受当前Cycle。Task 6.1的全部自动资格和fresh artifact目标已达到；唯一原阻塞项由用户保留
Evidence并明确豁免，Minor文档发现不影响当前Acceptance。无需当前Cycle修复或rework/replan。
按既有Iteration Map展开Iteration 008，由用户手工QEMU批次完成Tasks 7.1-7.2；本接受不构成
Iteration 008的Act授权。

**Iteration Plan Update**

None；Iteration Map保持不变。

**Next Cycle**

None.

**Next Iteration**

`../008-single-hart-qemu-acceptance/000-initial.md`（draft；等待用户审计与Gate 2批准）。
