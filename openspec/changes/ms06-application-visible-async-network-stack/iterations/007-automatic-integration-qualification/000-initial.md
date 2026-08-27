# Iteration 007 / Cycle 000: qualify automatic integration and fresh artifacts

## Plan Context

- Status: draft
- Approval: pending；本Cycle尚未获得`openspec-act`授权
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

- Branch `net-k3`；HEAD `b1e248881b37fb81e460192f1d7add1d7f5448d3` 加当前MS06工作树。
- Iteration 006最终graph：ordinary 371/371 ×3、qemu-diagnostics 393/393 ×3；focused/regression两profile
  各×100。`cargo tree`确认test graph不启用smoltcp defaults，normal edge不启用`test-seeds`。
- 当前diff约14 files、1613 insertions/210 deletions；未提交。Iteration 007必须审查完整HEAD-to-working-tree
  diff，而不是只看本Cycle增量。
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
5. fmt、source assertions、strict OpenSpec、HEAD-to-working-tree diff check和full diff Review通过。
6. 未启动QEMU、未串行full suite、未使用旧artifact、未修改产品实现；无Critical/Important finding。

**Verification**

- `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib`及加
  `--features qemu-diagnostics`，默认线程；运行Task 6.1指定100×race/ownership集合。
- `make host-test`；axdriver_net、axdriver_virtio net、virtio-drivers alloc、uart async与相关smoltcp tests。
- MS05 capture/audit声明的automatic Gate，或逐项等价命令；D1必须保留raw exit/count和auditor exit。
- `cargo check --locked --offline -p starry-kernel --features qemu`；产品smoltcp/axnet checks。
- 强制构建QEMU image、MS01、MS04、MS05、MS06 probes；记录`git rev-parse HEAD`、`file`、size与MS06
  embedded revision。MS02/MS03按既有automatic bundle一并重建。
- `cargo fmt`/`rustfmt --check`适用范围、Makefile source guards、strict OpenSpec、`git diff HEAD --check`、
  staged check和full diff Review。
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

Gate 2技术检查项PASS；状态保持`draft`，等待用户审计和明确批准。未授权`openspec-act`。

**Persisted Evidence**

- Mode: none

命令与结果可重跑；Act Response记录每项命令、决定性输出、exit、artifact revision/size和Review。现有MS05
capture工具可生成其自身Evidence，但本Cycle不复制或新建MS06 Evidence占位目录。

**Risks and Notes**

- automatic suite较长；失败后只允许在修复或环境恢复后重新执行完整受影响Gate，不得无限重跑找GREEN。
- D1 raw check为预期101；只有精确20×E0432/5×E0433且auditor成功才算受支持contract PASS。
- cross-musl工具链或image构建能力缺失属于capability blocker；必须记录命令和首个缺失层，不能复用旧artifact。

## Act Response

- Status: pending

**Implemented**

None；本Cycle只执行qualification与artifact构建。

**Changed Files and Symbols**

None expected outside Act Response and generated artifacts.

**Deviations from Plan**

None yet.

**Blocker Handoff**

None yet.

**Blocker Resolution**

None yet.

**Self-Review**

None yet.

**Verification Evidence**

None yet.

**Persisted Evidence**

None.

**Experience Candidates**

None yet.

**Remaining Issues**

Iteration 008 manual QEMU runtime remains deferred.

**Commit or Diff Reference**

None yet.

## Plan Review

- Review Result: pending

**Findings**

None yet.

**Deviation Classification**

None yet.

**Acceptance Gaps**

Task 6.1尚未执行或Review。

**Convergence**

N/A.

**Evidence**

None yet.

**Follow-up Decision**

等待用户批准Gate 2后显式调用`openspec-act`。

**Iteration Plan Update**

None；Iteration Map保持不变。

**Next Cycle**

None.

**Next Iteration**

None；Iteration 008保持map-only，直到本Iteration accepted。
