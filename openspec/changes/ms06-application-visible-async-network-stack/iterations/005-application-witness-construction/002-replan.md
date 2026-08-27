# Iteration 005 / Cycle 002: align exact-waiter witness with syscall semantics

## Plan Context

- Status: ready
- Approval: granted — 用户于 2026-08-27 显式批准并指示开始实施（原话："更新gate状态，开始实施"），Gate 2 获批，授权 openspec-act 执行本 Cycle
- Iteration: 005-application-witness-construction
- Cycle: 002-replan
- Cycle Type: replan
- Parent cycle: `001-rework.md`

**Iteration Scope**

- Change tasks: 4.1、4.3；Task 4.2 保持完成
- Depends on: Iteration 004 accepted
- Stable baseline: validator拒绝所有phase外protocol marker；静态probe提供普通poll/select/epoll multiwaiter，
  exact 64/65使用同步epoll注册barrier，host机制证据与guest eventual completion各自承担可观察范围。
- Verification boundary: validator START前负例、arm/unit-count model、既有PollSet/epoll源码契约、host syntax/seam、
  fresh RISC-V static build、source guards、strict OpenSpec和full diff Review；不启动QEMU。
- Diagnostic boundary: transcript head parser、epoll_ctl同步注册、arm/data control flow、waiter identity/units、交叉编译。
- Deferred tasks: Iteration 006 Tasks 5.1-5.2；Iteration 007 Task 6.1；Iteration 008 Tasks 7.1-7.2

**Cycle Scope**

- Trigger: replan-required
- Acceptance gaps: START前protocol marker；不可达pre-data replacement progress；缺失trigger-unit test
- Repair items: None
- Inherited scope: R5-R7；D6/D10；12-case marker set；public ABI；fixed deadline；exact 64/65；manual-QEMU policy；
  Cycle 001已关闭的UDP/quiet结果
- Excluded scope: 修改PollSet、kernel syscall、axnet/smoltcp产品行为；QEMU runtime；host-test isolation；
  automatic qualification；MS01/MS04/MS05 runtime；降低并发；SMP、真板、性能、全局状态同步和commit

**Objective**

让validator和exact-waiter probe符合实际公开ABI：protocol marker在START前即失败；64/65 registration通过同步
`epoll_ctl(ADD)`得到可观察barrier，不再等待内核不会返回给用户态的empty-event wake。

**Current Baseline**

- Branch `net-k3`；HEAD `1ea51427d8692f5a12b87a0403b940e73d43fed3` 加当前MS06工作树。
- Cycle 001已完成body phase、UDP/quiet和N-unit实现；Plan Review为`replan-required`。
- 新鲜验证：validator self-test PASS、26项seam ×2 PASS、C syntax PASS、strict OpenSpec PASS、diff check PASS；
  三个START前protocol反例仍被接受。
- focused axnet Rust test的直接Cargo命令因x86_64 per-CPU relocation链接配置失败；本Cycle不以该错误判定产品，
  但Act必须使用项目既有可运行命令或在Iteration 007重新取得host test证据。

**Current-State Evidence**

- validator在定位唯一START后丢弃`_head`，未区分shell noise与PASS/FAIL/MS06 protocol line。
- `poll_io()`只在readiness closure成功时返回；replacement wake后fd仍未ready时重新register并Pending。
  `poll`/`select`因此不能向worker返回empty event。
- epoll `EPOLL_CTL_ADD`同步调用`check_and_register_waker()`；worker可在ctl成功后报告arm。每worker独立epoll
  instance产生distinct InterestWaker，parent收齐64/65个arm即可证明精确注册数发生在data之前。
- 第65个InterestWaker触发PollSet replacement。被唤醒interest进入epoll ready queue；`consume(NoEvent)`会
  `register_waker_only()`。该mechanism由host/source证据承担，guest不伪造replacement record。
- Cycle 001 peer已发送N units，但seam未把unit count纳入release contract。

**Critical Path**

```text
pre-START protocol RED -> head protocol scan -> validator GREEN
exact worker creates private epoll -> EPOLL_CTL_ADD shared socket -> arm
parent collects exactly 64/65 arms -> verifies trigger_units == waiter_count -> releases N units
kernel replacement/no-event path rechecks and re-registers internally
all distinct guest waiters consume one unit and complete exactly once
host seam + source contract + static build + OpenSpec/diff Review
```

**Behavioral Change**

- 普通serial/shell noise仍可位于START前；PASS、FAIL或MS06 marker位于START前时validator失败。
- 4-waiter poll/select/epoll场景不变。exact 64/65从poll + 不可达progress channel改为独立epoll同步注册barrier。
- exact guest record只声明distinct exactly-once completion；replacement/re-register由PollSet与epoll内核路径证据承担。
- marker名称、顺序、public ABI、deadline、产品代码和QEMU边界不变。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| 4.1 | R7 / marker protocol | validator parser/self-test | body phase | 扫描START前protocol marker并新增负例 |
| 4.3 | R5/R7 / exact 64/65 | probe multiwaiter orchestration | progress pipe + poll wait | private epoll预注册、exact arm barrier、N-unit release |
| 4.3 | R5/R7 / model witness | probe seam/test | barrier boolean | arm count、mode、unit count与guest record分层判定 |
| all | D10 | Makefile/source guards | host/static Gate | 禁止empty-event progress依赖并重建artifact |

**Task Contracts**

### 4.1: reject protocol markers before witness START

- Requirement/Scenario: R7；D10；完整marker protocol。
- Depends on: Cycle 001 body phase实现。
- Targets: `scripts/ms06-qemu-validate.py::validate_output/self_test`。
- Current behavior: `_head`中的PASS、FAIL和MS06 lines被忽略。
- Required behavior: START前普通shell/serial noise允许；任何trim后以`PASS:`、`FAIL:`或`MS06_`开头的line失败，
  并报告首个protocol差异。START后的现有phase、END和EXIT规则保持。
- Required changes: 先为三类START前marker建立RED；实现head scan；保留合法noise正例。
- Preserve: 纯auditor、stdin/file CLI、metadata expectations、timeout、12-case set和原全部负例。
- Forbidden: 禁止所有START前输出、启动QEMU、驱动guest或放宽body/tail规则。
- Test witness: validator self-test与三个审计反例。
- GREEN condition: 合法noise transcript通过，三类pre-START marker和全部既有负例被拒绝。
- Verification: Python syntax/self-test、CLI正反例、purity guard。
- Stop when: 实际manual harness必须在START前输出MS06 protocol line；返回Plan核对harness，不静默接受。

### 4.3: establish exact 64/65 registration through synchronous epoll arms

- Requirement/Scenario: R5/R7；D6/D10；multiwaiter与PollSet容量边界。
- Depends on: 已接受的readiness bridge与Cycle 001 N-unit基础。
- Targets: probe exact-waiter mode、worker/parent/peer control flow、waiter seam tests、Makefile source guards。
- Current behavior: exact 65等待用户态empty-event `'R'`；该signal按syscall contract不可达。
- Required behavior: 普通4-waiter poll/select/epoll保持；exact 64/65的每个worker创建独立epoll fd，同步
  `EPOLL_CTL_ADD`共享socket成功后写一个arm，再进入有界epoll wait。parent收齐exact N arms并验证N units后才放行peer；
  每个distinct pid非阻塞消费一个unit并exactly-once完成。guest不声称看见replacement empty event。
- Required changes: 移除prog/replacement barrier及guest replacement record要求；增加exact-epoll预注册模式、arm pipe、
  `arm_count == waiter_count`和`trigger_units == waiter_count` seam RED/GREEN；保持失败清理有deadline。
- Preserve: 一个public socket、64/65不同进程、12-case markers、register-recheck、fixed deadline和N-unit payload。
- Forbidden: sleep排序、poll/select空事件假设、每waiter独立socket、降低并发、修改PollSet/kernel或仅以record count
  代替arm/unit witness。
- Test witness: model覆盖63/64、64/64、64/65、65/65 arms，N−1/N units，错误mode，duplicate pid、partial和double
  completion；source guard确认exact cases走同步epoll arm且无`prog_fd`/pre-data replacement wait。
- GREEN condition: model/seam与既有tests通过；full source review显示trigger只在exact arms齐全且units匹配后发布。
- Verification: focused seam ×2、C syntax、marker diff、fresh static build、artifact marker inspection、full diff Review。
- Stop when: guest epoll_ctl不是同步注册点、独立epoll instance不能产生distinct waker，或65进程/epoll资源在构建前即
  被ABI限制；返回Plan，不降低Acceptance。

**Invariants**

- replacement wake只作内核recheck hint；用户态不得把empty event当作公开ABI。
- exact 64/65的registration、stimulus和completion是三个独立判定量。
- probe不调用内部stack poll；runner仍是唯一推进者。
- 所有case保留absolute monotonic deadline、唯一marker和非零失败exit。

**Non-goals**

- 修改kernel/axnet/smoltcp、PollSet容量、syscall语义或产品socket行为。
- 执行QEMU、host-test isolation、automatic qualification、MS01/MS04/MS05 runtime。
- reset、SMP、真板、性能、全局文档维护和commit。

**Acceptance**

1. validator拒绝三类pre-START protocol marker，合法serial noise与所有既有正例保持通过。
2. exact 64/65 source与model证明每个distinct epoll interest在data前同步arm，arm数与waiter数相等。
3. trigger-unit model直接拒绝N−1并接受N；guest final aggregation保持distinct pid与exactly-once。
4. implementation不再等待用户态empty-event progress，也不把host replacement机制伪装为guest observation。
5. Python/C tests、source guards、host syntax、fresh RISC-V static build、strict OpenSpec、diff check和full diff Review
   通过；未启动QEMU、未改产品代码、无Critical/Important finding。

**Verification**

- validator self-test、三个pre-START反例、合法noise CLI正例。
- seam warnings-as-errors与focused exact arm/unit tests ×2；probe syntax。
- source guards：无`prog_fd`/pre-data `'R'` barrier；exact cases使用epoll同步arm；无sleep/internal poll。
- validator/probe marker diff；fresh RISC-V static build与marker inspection。
- strict OpenSpec、相关format、`git diff --check`和current Cycle full diff Review。
- SKIPPED：QEMU、完整host product qualification、MS01/MS04/MS05 runtime；属于Iterations 008/007/008。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | validator head、poll_io、poll/select、epoll ctl/consume、PollSet replacement和probe control flow已定位 |
| Design | PASS | synchronous epoll arm提供公开可达barrier；host机制与guest completion证据分层 |
| Iteration Plan | PASS | 只重开4.1/4.3；4.2与后续006-008依赖不变 |
| Cycle Scope | PASS | 修正验证契约，不修改requirement、marker set或产品代码 |
| Task Contracts | PASS | parser与exact waiter均有RED/GREEN、保持/禁止、验证和停止条件 |
| Traceability | PASS | R5/R7、D6/D10映射到validator、epoll arm、unit model与guest completion |
| Verification | PASS | pre-START、arm count、unit count、source/static/OpenSpec/diff Gate均有通过条件 |

Gate 2技术检查项PASS；用户批准仍为BLOCKED，Plan Context保持`draft`。

**Persisted Evidence**

- Mode: none

结果可低成本重跑，由Act Response记录命令、决定性输出、exit、文件和符号；不创建Evidence占位目录。

**Risks and Notes**

- focused axnet host命令需沿项目既有可链接配置执行；本Cycle不以错误的standalone Cargo链接命令作为Gate。
- QEMU中的65进程/epoll资源可行性仍由Iteration 008验证；本Cycle只构造并静态验证artifact。
- Cycle 001已接受的UDP/quiet实现不得在本Cycle重做。

## Act Response

- Status: reported（2026-08-27；含两轮 Plan Review 打回修复：前导空白 + START/END 完整行；完整快照）

**Implemented**

Tasks 4.1、4.3 全部完成；Task 4.2 保持 Cycle 001 已接受结果。未启动 QEMU，未修改产品代码（kernel/axnet/smoltcp/scheduler/PollSet 零改动）。

- 4.1：`scripts/ms06-qemu-validate.py::validate_output` 在丢弃 `_head` 之前新增 head protocol scan —— 对 START 前的每一行，凡 trim 后以 `PASS:`、`FAIL:` 或 `MS06_` 开头的直接失败并报告首个 protocol 差异（原实现丢弃 `_head` 把三类 marker 当 serial noise）。`self_test` 新增 `with_prestart()` 三正一反：`FAIL: stale-before-start`（Plan Review 审计反例 1）、`MS06_HARNESS_EXIT: 1`（审计反例 2）、`PASS: tcp-timer`（审计反例 3）必须 rejected，`starry:~# ./ms06` 纯 noise 保持合法。START 后 phase/END/EXIT 规则、CLI、expect metadata、timeout、12-case set 与全部既有负例不动。
- 4.1（Plan Review 打回修复 1——前导空白）：head、body、tail 三处均把 `rstrip()` 统一改为 `strip()` 全 trim，使带前导空白的 protocol marker 不再伪装成 serial noise。`self_test` 新增前导空白负例矩阵：pre-START 三类 `indented("FAIL:…")`/`indented("MS06_HARNESS_EXIT: 1")`/`indented("PASS: tcp-timer")` 拒绝、`indented("starry:~# ./ms06")` 纯 noise 保留；body 非法 phase 的 `indented("FAIL: drifted-into-body")` 与 `indented("MS06_UNKNOWN: x")` 拒绝；END 后 `indented("PASS: tcp-timer")` 拒绝；EXIT 后 `indented("FAIL: stale-after-exit")` 拒绝。`transcript_lines()` helper 上移到 `self_test` 顶部（消除前向引用）。
- 4.1（Plan Review 打回修复 2——START/END 完整行）：废弃 `output.count(START)`/`split(START)`/`partition(END)` 的子串搜索，改为扫描 `output.splitlines()` 并按 trim 后完整物理行识别恰好一个 START 与其后恰好一个 END；带 `shell-noise-` 前缀或 `-trailing-noise` 后缀、仅含 marker 子串的行不得充当结构边界。head（`lines[:start_at]`）、body（`lines[start_at+1:end_at]`）、tail（`lines[end_at+1:]`）分别按行列表处理，前导空白协议行仍经 `strip()` 分类。`self_test` 新增四个结构反例（START 带前缀、START 带后缀、END 带前缀、END 带后缀），并保留合法 noise 行位于 START 前/END 后的正例。
- 4.3：exact 64/65 从"poll wait + 用户态 pre-data replacement progress"改为"per-worker 私有 epoll 同步注册 barrier"。
  - seam 区上移 `enum ms06_wait_mode`；`ms06_waiter_set` 移除 `require_replacement` 字段（guest 不再有 replacement record 要求）；新增三个决策 seam：`ms06_exact_mode_ok(mode)`（exact 必须 `MS06_WAIT_EPOLL`）、`ms06_exact_arms_complete(armed, n_waiters)`（arm_count == waiter_count，拒绝 63/64、64/65、0、66/65）、`ms06_trigger_units_valid(units, n_waiters)`（trigger_units == waiter_count，拒绝 N−1）。
  - `struct mw_cfg`：`prog_fd`/'R' 通道移除，改为 `arm_fd`（worker→parent 精确 'A' arm 字节）+ `exact` 标志。
  - `mw_worker_body`：exact 分支先 `epoll_create1(0)` + `EPOLL_CTL_ADD(sfd)` 同步注册，成功后置 `MS06_PHASE_REGISTERED` 并写一个 `'A'` 到 arm_fd，再进入持久 `mw_epoll_wait_loop`；IN 后只以 `MSG_DONTWAIT` 消费一个 unit，完全不存在 prog/R 写路径，guest 不生成 replacement 记录。`mw_epoll_wait_loop` 抽为共享 helper，`mw_wait_once` 复用。
  - `run_multiwaiter`（parent）：`prog` 管道改 `arm`；worker fork 后 parent 先读 connector "L"，再对 exact case 用 `read_full_deadline(arm[0], ..., n_waiters)` 收齐精确 N 个 arm、校验全为 `'A'`、经 `ms06_exact_mode_ok && ms06_exact_arms_complete` 门控，随后 `ms06_trigger_units_valid(n_waiters, n_waiters)` 才放行 'G' + N units；4-waiter 无 arm barrier（`exact=0` 跳过 arm 收集，仅 `exact &&` 才查 mode），unit 计数仍绑定 waiter 数。
  - `run_waiter_64`/`run_waiter_65_reregister` 模式切为 `MS06_WAIT_EPOLL`；`MS06_WAITER_CAPACITY` 死宏移除。
- Makefile：host-test 新增三条 source guard —— 无 `prog_fd`/`read_byte_deadline(prog`/`progress_reported`；`run_waiter_64`/`run_waiter_65_reregister` 必须含 `MS06_WAIT_EPOLL`（同步 epoll arm）。

**Changed Files and Symbols**

| 文件 | 性质 | 关键符号 |
|---|---|---|
| `scripts/ms06-qemu-validate.py` | 修改 | `validate_output`（head scan 新增）、`self_test`（`with_prestart` 三反一正） |
| `tests/ms06_stack_readiness_probe.c` | 修改 | seam：`enum ms06_wait_mode` 上移、`ms06_waiter_set`（去 require_replacement）、新增 `ms06_exact_mode_ok`/`ms06_exact_arms_complete`/`ms06_trigger_units_valid`、移除 `ms06_trigger_release_ok`；guest：`mw_cfg`（arm_fd/exact）、`mw_epoll_wait_loop`、`mw_wait_once`、`mw_worker_body`（exact 分支）、`run_multiwaiter`（arm 收集+门控）、`run_waiter_64/65`（epoll）、`MS06_ARM_BYTE`、移除 `MS06_WAITER_CAPACITY` |
| `tests/ms06_stack_readiness_probe_test.c` | 修改 | 移除 `test_set_rejects_unobserved_replacement_on_65`、`test_trigger_release_*`×2；改写 `test_set_accepts_exact_65_all_complete` 等 set 契约；新增 `test_exact_mode_requires_epoll`/`test_exact_arms_complete_matrix`/`test_exact_trigger_units_matrix`（26 项总数不变） |
| `Makefile` | 修改 | host-test 三条 ms06 source guards |
| `tests/ms06_stack_readiness_probe` | 重建 | fresh RISC-V static artifact（`MS06_REVISION_DEFAULT='1ea51427…'`） |
| `openspec/changes/.../tasks.md` | 文档 | Task 4.1、4.3 标记完成并附完成记录 |
| 本 Cycle 文件 | 文档 | Plan Context（Gate 2 批准记录，用户原话）、本 Response |

产品代码（kernel/crates/smoltcp/src）零改动。

**Deviations from Plan**

1. Plan 建议"exact 64/65 每个 worker 独立 epoll 并`epoll_ctl(ADD)`成功后写 arm"：实现为 worker 在首次进入 `mw_worker_body` 即完成 create+ADD+arm 一次性预注册，此后循环复用同一 `exact_ep` 持久 fd（`mw_epoll_wait_loop`），不再逐轮重建。语义与 Plan 一致（同步注册发生在任何 data 之前，arm 字节即注册完成证明），减少重复注册开销。
2. Plan 的 model 列表含"错误 mode"：以 `ms06_exact_mode_ok` + 条件门控 `exact && !ms06_exact_mode_ok(mode)` 实现，4-waiter poll/select 不受影响。
3. 验证命令按 Makefile 既有配方执行（seam 不带 `-O2`），另补跑两次 focused；等价调整，不影响判定。

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

Spec compliance review（逐 Task Contract）：

- 4.1：START 前普通 shell/serial noise 允许 ✓；任何 trim 后以 `PASS:`/`FAIL:`/`MS06_` 开头的 line 失败并报告首个 protocol 差异 ✓；带前导空白的 protocol marker 在 START 前、body 非法 phase、END 后、EXIT 后均被拒绝（打回缺口 1 闭合）✓；只含 START/END 子串的行不得充当结构边界，带前缀/后缀的伪造边界被拒绝（打回缺口 2 闭合）✓；普通非协议 shell/serial noise（含前导空白版、含 START/END 子串的非 marker 行）仍通过 ✓；START 后现有 phase、END、EXIT 规则保持 ✓；保留纯 auditor、stdin/file CLI、metadata expectations、timeout、12-case set 与全部原负例 ✓；未启动 QEMU、未驱动 guest、未放宽 body/tail 规则 ✓；GREEN 条件达成 ✓。
- 4.3：exact 64/65 的每个 worker 私有 epoll + 同步 `EPOLL_CTL_ADD` 成功后写 arm，再进入有界 epoll wait ✓；parent 收齐 exact N arms 并验证 `trigger_units == waiter_count` 后才放行 peer ✓；每个 distinct pid 非阻塞消费一个 unit 并 exactly-once 完成 ✓；guest 不声称看见 replacement empty event ✓；replacement/no-event recheck 与 re-register 由既有 PollSet + epoll 内核路径证据承担（本 Cycle 未伪造 guest observation）✓；普通 4-waiter poll/select/epoll 保留无 barrier ✓；model 覆盖 63/64、64/64、64/65、65/65 arms、N−1/N units、错误 mode、duplicate pid、partial、double completion ✓；source guard 确认 exact cases 走同步 epoll arm 且无 `prog_fd`/pre-data `'R'` barrier ✓。
- 不变量：probe 不调用 `poll_interfaces()`、无 sleep 排序、所有 case 保留 absolute monotonic deadline 与唯一 marker ✓。
- Non-goals：未改产品代码、未改 PollSet 容量、未执行 QEMU、未做 host-test isolation（006）/automatic qualification（007）/runtime（008）✓。

Code quality review（完整 unstaged/本 Cycle diff）：

- diff 仅覆盖 4.1/4.3 目标文件 + gate 记录 + artifact 重建；无计划外文件 ✓。
- 结构解析改为单扫描确定 `start_at`/`end_at`，head/body/tail 均为行列表切片，消除子串歧义；错误消息具体化（missing/duplicated/appears-before）✓。body 的 phase 状态机、revision/environment 取值（前缀裁剪后再 `.strip()`）、tail 的 EXIT 判定均与全 trim 兼容 ✓。
- 三个新 seam 单一职责、无副作用；`ms06_exact_arms_complete`/`ms06_trigger_units_valid` 共享 `n_waiters==0` 拒绝 ✓。
- arm pipe 生命周期：connector child 关闭 arm 两侧；worker 仅持 arm[1] 写端且 exact 分支才写；parent 收齐即读，写量有界（≤65 字节），无满管风险 ✓。
- 移除 `require_replacement` 后 `ms06_waiter_set_accepts`/record_valid 契约自洽（record 仍可承载 replacement 链，set 不再强制）✓。
- 编译 `-Wall -Wextra -Werror` 零警告（host seam ×2 与 RISC-V static 均通过）✓；无死代码、无重复实现（`mw_epoll_wait_loop` 复用）✓。

修复的发现（本 Cycle 内）：
- （已修复，Critical）Task 4.3 首个实现把 `ms06_exact_mode_ok(mode)` 放在无条件门控，会误拒 4-waiter poll/select —— 改为 `exact &&` 条件后 4/64/65 契约分离。
- （已修复，Important）`write_full(cfg->arm_fd, MS06_ARM_BYTE, 1)` 把 char 常量当指针 —— 改为 `const char arm` 局部变量传址。
- （已修复，Important，Plan Review 打回项 1）head/body/tail 仅 `rstrip()`，前导空白的 `PASS:`/`FAIL:`/`MS06_` marker 被当 noise —— 三处统一 `strip()` 全 trim，并补齐全 phase 前导空白负例。
- （已修复，Important，Plan Review 打回项 2）START/END 用 `count`/`split`/`partition` 子串搜索，带前缀/后缀的行可伪造结构边界 —— 改为按 trim 后完整物理行识别恰好一个 START 与其后恰好一个 END，并补齐四类结构反例。

遗留 Minor 问题（保留自 Cycle 000/001 Review，供 Iteration 008 观察）：
- (a) `run_multiwaiter` 成功路径以 `MS06_EV_IN` 填充 `r.events` 满足 verdict 形参契约，非实测快照。
- (b) 非 exact epoll 分支遇 RDHUP-only wake 走 replacement-class 路径并在 peek 得 0 时判败；waiter 场景对端不关闭，合同范围内无影响。
两项均不阻塞本 Cycle Acceptance。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| validator self-test（含结构/phase/前导空白全矩阵） | `python3 scripts/ms06-qemu-validate.py --self-test` | `PASS: ms06-validator-self-test`，exit 0 | PASS |
| 审计反例 1（FAIL 在 START 前） | `python3 scripts/ms06-qemu-validate.py /tmp/ms06-prestart-fail.txt` | `FAIL: ... protocol marker before the witness start marker: 'FAIL: stale-before-start'`，exit 1 | PASS |
| 审计反例 2（EXIT 在 START 前） | 同上 `ms06-prestart-exit.txt` | `... 'MS06_HARNESS_EXIT: 1'`，exit 1 | PASS |
| 审计反例 3（PASS 在 START 前） | 同上 `ms06-prestart-pass.txt` | `... 'PASS: tcp-timer'`，exit 1 | PASS |
| 合法 noise 正例 | `python3 scripts/ms06-qemu-validate.py /tmp/ms06-good.txt` | `PASS: ms06-transcript-valid`，exit 0 | PASS |
| 前导空白 PASS 在 START 前 | `python3 ... /tmp/ms06-ind-pre-start.txt` | `FAIL: ... 'PASS: tcp-timer'`，exit 1 | PASS |
| 前导空白 FAIL 在 body 非法 phase | `python3 ... /tmp/ms06-ind-body-fail.txt` | `FAIL: ... payload reported a failure: FAIL: drifted-into-body`，exit 1 | PASS |
| 前导空白 MS06_ 在 body 非法 phase | `python3 ... /tmp/ms06-ind-body-ms06.txt` | `FAIL: ... unknown MS06 protocol line inside witness body: 'MS06_UNKNOWN: x'`，exit 1 | PASS |
| 前导空白 PASS 在 END 后 | `python3 ... /tmp/ms06-ind-after-end.txt` | `FAIL: ... protocol marker after the end marker: 'PASS: tcp-timer'`，exit 1 | PASS |
| 前导空白 FAIL 在 EXIT 后 | `python3 ... /tmp/ms06-ind-after-exit.txt` | `FAIL: ... protocol marker after the end marker: 'FAIL: stale-after-exit'`，exit 1 | PASS |
| 前导空白 noise 在 START 前（合法） | `python3 ... /tmp/ms06-ind-noise.txt` | `PASS: ms06-transcript-valid`，exit 0 | PASS |
| START 带前缀（伪造边界） | `python3 ... /tmp/ms06-str-start-prefix.txt` | `FAIL: ... start marker is missing`，exit 1 | PASS |
| START 带后缀（伪造边界） | `python3 ... /tmp/ms06-str-start-suffix.txt` | `FAIL: ... start marker is missing`，exit 1 | PASS |
| END 带前缀（伪造边界） | `python3 ... /tmp/ms06-str-end-prefix.txt` | `FAIL: ... end marker is missing`，exit 1 | PASS |
| END 带后缀（伪造边界） | `python3 ... /tmp/ms06-str-end-suffix.txt` | `FAIL: ... end marker is missing`，exit 1 | PASS |
| 合法 transcript 仍通过 | `python3 ... /tmp/ms06-str-good.txt` | `PASS: ms06-transcript-valid`，exit 0 | PASS |
| exact arm/unit/mode seam（RED→GREEN） | `cc -std=c11 -Wall -Wextra -Werror tests/ms06_stack_readiness_probe_test.c` 后执行 ×2 | `ms06 probe decision tests: 26 passed` ×2，exit 0 | PASS |
| probe C syntax | `cc -std=c11 -Wall -Wextra -Werror -fsyntax-only tests/ms06_stack_readiness_probe.c` | 无输出，exit 0 | PASS |
| source guards（无 prog/R barrier、exact 用 epoll） | Makefile 三个新 guard + 既有 sleep/purity guards | 全部通过 | PASS |
| marker set 精确性 | validator/probe `--print-cases` + `diff -u` | 无差异 | PASS |
| RISC-V static build（fresh artifact） | `make tests/ms06_stack_readiness_probe` | musl gcc `-static -no-pie -Os -Werror` 通过；`file`：ELF 64-bit UCB RISC-V statically linked；64/65 marker 计数 `2` | PASS |
| OpenSpec strict | `openspec validate ms06-application-visible-async-network-stack --strict` | `Change ... is valid`，exit 0 | PASS |
| diff 卫生 | `git diff --check` | 无输出，exit 0 | PASS |

**Persisted Evidence**

None required — Plan 设定 Mode 为 `none`；所有结果可低成本重跑（上表命令），决定性输出与退出码已在本 Response 保存，未创建 Evidence 目录。

**Experience Candidates**

None — 本 Cycle 为纯 witness-construction 修复，全部命令可重复且由 Act Response 完整记录，不构成 Runbook/Incident 候选。

**Remaining Issues**

- guest 运行时 65 并发进程与 PollSet 真实替换行为的资源可行性仍属 Iteration 008 验证边界；本 Cycle 只证明可构建的同步 epoll arm choreography 与 model contract，不声称 QEMU runtime PASS（计划内既定边界）。
- 2026-08-24 批准的 `has_pending_tx()`/MS01 分层取证等 Iteration 001 遗留事项由既有 cycle 记录持有；本 Cycle 不涉及。
- 遗留 Minor (a)(b) 见 Self-Review，已记录且不阻塞 Acceptance。

**Commit or Diff Reference**

未提交（未获授权）。本 Cycle 工作树增量：修改 `scripts/ms06-qemu-validate.py`（head scan、前导空白统一 trim、START/END 完整物理行识别、self-test 全矩阵）、`tests/ms06_stack_readiness_probe.c`、`tests/ms06_stack_readiness_probe_test.c`、`Makefile`、change `tasks.md`、本 Cycle 文件（Gate 状态、两轮打回修复与本 Response）；重建 `tests/ms06_stack_readiness_probe`。

## Plan Review

- Review Result: accepted

**Findings**

- None blocking。前两轮Review的前导空白分类与START/END子串边界均已闭合。
- 遗留Minor保持不变：multiwaiter成功结果中的`events`是聚合契约值；非exact epoll的RDHUP-only分支在
  当前peer不关闭场景不可达。两项留待Iteration 008 runtime观察，不阻塞本Cycle。

**Deviation Classification**

- None blocking；Act记录的三项局部偏差均保持原合同语义。

**Acceptance Gaps**

- None。

**Convergence**

- converged：Task 4.1的phase、前导空白与结构边界缺口全部闭合；Tasks 4.2/4.3保持通过。

**Evidence**

- 新鲜validator self-test输出`PASS: ms06-validator-self-test`，exit 0。
- 独立复测：START/END各自带前缀、带后缀的四个伪造边界均被拒绝；trim后的合法独立边界通过。
- seam 测试以 `-std=c11 -Wall -Wextra -Werror` 编译并连续执行两次，均输出
  `ms06 probe decision tests: 26 passed`。
- probe `-fsyntax-only`、RISC-V static artifact检查、marker set diff、strict OpenSpec与`git diff --check`通过。

**Follow-up Decision**

- 无当前Cycle修复；Iteration 005完成。按既有Map展开Iteration 006，未调用Act或Maintainer。

**Iteration Plan Update**

None.

**Next Cycle**

None.

**Next Iteration**

`../006-axnet-host-test-isolation/000-initial.md`
