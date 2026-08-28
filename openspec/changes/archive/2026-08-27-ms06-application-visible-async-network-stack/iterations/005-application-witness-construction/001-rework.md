# Iteration 005 / Cycle 001: repair application witness protocol and orchestration

## Plan Context

- Status: ready
- Approval: approved — 用户于 2026-08-27 显式批准本 Cycle 计划与 Gate 2 结果（原话："更改gate状态，开始实施"），并授权本次 `openspec-act` 执行；该授权不覆盖后续 Plan Review、下一 Iteration、全局状态同步或收尾
- Iteration: 005-application-witness-construction
- Cycle: 001-rework
- Cycle Type: rework
- Parent cycle: `000-initial.md`

**Iteration Scope**

- Change tasks: 4.1-4.3
- Depends on: Iteration 004 accepted
- Stable baseline: 纯输出 validator 与静态 RISC-V guest probe 可独立构建；12 个 application-visible
  readiness 场景均有固定协议和 deadline，64/65 distinct waiter 的 replacement、recheck、re-register 与
  exactly-once completion 编排可由后续 QEMU Cycle 执行。
- Verification boundary: 新增缺口的 true RED→GREEN、既有 validator/seam 回归、host syntax、RISC-V static
  build、marker/source guards、strict OpenSpec 和完整 diff Review；本 Iteration 不启动 QEMU。
- Diagnostic boundary: transcript parser phase、UDP endpoint 初始化、quiet interest contract、multiwaiter
  parent/worker/peer choreography、deadline 或交叉编译。
- Deferred tasks: Iteration 006 Tasks 5.1-5.2；Iteration 007 Task 6.1；Iteration 008 Tasks 7.1-7.2

**Cycle Scope**

- Trigger: rework-required
- Acceptance gaps: 原 Cycle Review 的 A1-A5
- Repair items: 4.1-R1、4.2-R1、4.3-R1
- Inherited scope: R5-R7；D6/D10；Tasks 4.1-4.3；12-case marker set；public socket ABI；exact 64/65；
  fixed deadline；manual-QEMU policy；原 Cycle 未受 finding 影响的实现和 tests
- Excluded scope: kernel、axnet、smoltcp、scheduler 或 syscall ABI 修改；执行 QEMU；host-test isolation；
  automatic qualification；MS01/MS04/MS05 runtime；PollSet 容量变化；reset、SMP、真板、性能、全局状态同步和 commit

**Objective**

修复 transcript phase、UDP/quiet public-ABI 配置和 multiwaiter 触发协议，使 witness construction 本身满足
Tasks 4.1-4.3。修复后的 host/model tests 必须在执行 QEMU 前拒绝原 Cycle 的四类确定性错误。

**Background**

Cycle 000 的自测试和 22 项 seam 只验证 malformed transcript 的部分集合与最终 waiter record 聚合。Plan Review
发现 parser 接受 PASS-before-metadata，UDP 绑定未初始化 family，quiet 请求正常 writable 后把它判为噪声，
multiwaiter 则在 wait 返回后才上报 arm、同时让 parent 等 arm 后才发送唯一 trigger byte。后两项使 runtime
witness 无法按原契约收敛。

**Current Baseline**

- Branch `net-k3`；HEAD `1ea51427d8692f5a12b87a0403b940e73d43fed3` 加当前 MS06 工作树。
- Cycle 000 Act Response 为 `reported`，Review Result 为 `rework-required`；产品代码零改动。
- 新鲜 Review：validator self-test、22 项 seam、host syntax、marker diff、strict OpenSpec 和 diff check 通过；
  PASS-before-metadata 反例被错误接受。
- 当前 RISC-V artifact 是 static ELF 且含两个 64/65 marker；Review sandbox 无法重新执行 cross compiler，
  Act 必须在可用环境重新构建，不能复用历史 artifact 作为 GREEN。

**Current-State Evidence**

- `validate_output()` 在 body 中分别处理 PASS/revision/environment，只验证 environment 位于 revision 后，未用
  protocol phase 限制首个 PASS 必须位于两个 metadata 后。
- `run_udp_progress()` 清零 `aa/ba` 后直接 bind；与 `make_listener()` 的 AF_INET + loopback + port 0
  初始化不一致。
- `run_quiet()` 请求 `POLLIN|POLLRDHUP|POLLOUT` 并拒绝全部 revents；对 established TCP socket，OUT
  是正常 readiness，不能代表 runner 自唤醒或虚假 read progress。
- `do_poll()`、`do_select()` 与 epoll 均执行 check/register/recheck；event-before-register 不会丢失。因此
  4/64 waiter 不需要由 blocking waiter 证明“已进入 syscall”后才发送数据。
- 对 65 waiter，PollSet 第 65 个 distinct waker 会唤醒被替换者。首个 pre-data replacement 通知可作为
  “65 个初始 distinct registration 已到达容量边界”的控制信号；被替换者随后 recheck 并重新注册。
- 共享 stream 上 consuming read 会移除数据。若每个 waiter 都必须完成一次 I/O，peer 必须提交与 waiter 数
  相同的独立 trigger units，worker recheck/read 必须 nonblocking，避免 readiness race 后越过 deadline。

**Relevant Code**

| File / Symbol | Current responsibility | Rework responsibility |
|---|---|---|
| `scripts/ms06-qemu-validate.py::validate_output/self_test` | transcript 解析和部分负例 | 固定 phase state machine，覆盖 metadata/case 全序 |
| `tests/ms06_stack_readiness_probe.c::run_udp_progress` | UDP queued progress | 建立有效 AF_INET loopback endpoints |
| `tests/ms06_stack_readiness_probe.c::run_quiet` | idle public-ABI observation | 只观察 read/terminal/error，随后独立验证 liveness |
| `mw_wait_once/mw_worker_body/run_multiwaiter` | wait、replacement 记录、parent trigger | 消除循环等待，提供 65 replacement barrier 和 N-unit trigger |
| `tests/ms06_stack_readiness_probe_test.c` | 纯 verdict/record 聚合 tests | 增加 phase、endpoint/interest 和 choreography RED/GREEN |
| `Makefile::host-test` / static target | 自动 host/build Gate | 纳入新增 tests，重新生成当前 revision artifact |

**Critical Path**

```text
invalid phase transcript -> validator RED -> explicit phase parser GREEN
invalid UDP/quiet config -> seam RED -> valid endpoint/read-terminal interest GREEN
multiwaiter model RED -> fork distinct waiters
                      -> n<=64: trigger may precede registration; syscall recheck preserves data
                      -> n=65: first replacement progress proves capacity boundary
                              -> displaced waiter records wake + not-ready recheck
                              -> parent releases N trigger units
                      -> every waiter nonblocking-consumes one unit exactly once
                      -> all records unique and 65 includes replacement+reregister
                      -> host/static/source/OpenSpec/diff Gates
```

**Implementation Guidance**

1. 先扩充 validator 与 probe seam 的失败见证，确认 PASS-before-metadata、zero-family UDP、quiet OUT interest、
   arm-after-wait 循环和 single-trigger-for-N-consumers 在当前实现上 RED。
2. parser 使用显式 phase 或等价状态机；serial noise 可以忽略，但任何 protocol marker 必须只在其合法 phase 出现。
3. 修复 UDP 与 quiet 后保持 case 名称、顺序、deadline 和 reporter 不变。
4. multiwaiter 删除“所有 worker wait 返回后才报告 arm”的 barrier。4/64 在 connector ready 和 fork 完成后即可
   提交 N units；check-register-recheck 负责 event-before-register。65 使用独立、带类型的 replacement progress
   通道：parent 必须在首个 pre-data replacement 后才提交 N units。final result 通道只承载终态 record。
5. accepted socket 在 fork 前设为 nonblocking，或用等价方式保证所有 consuming recheck 都不会阻塞越过 deadline。
   replacement wake 只记录 not-ready + re-register；实际 I/O 成功仍决定 completion。

**Behavioral Change**

- validator 从“metadata 存在且 case 彼此有序”改为完整 protocol phase 顺序。
- UDP progress 使用有效 loopback endpoint；quiet 不再把正常 writable 当作异常活动。
- multiwaiter 从 1-byte/arm 循环等待改为 N-unit、register-recheck 驱动的 fan-out；65 以真实 replacement progress
  决定 trigger release，并保留 distinct pid、exactly-once 和 fixed deadline。
- marker set、CLI 基本输出、产品网络行为和后续人工 QEMU 边界不变。

**Change Surface**

| Repair | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| 4.1-R1 | R7 / marker protocol | validator parser/self-test | metadata 与 PASS 分支解析 | 完整 phase state machine 与乱序负例 |
| 4.2-R1 | R2/R3/R6/R7 / UDP、quiet | probe UDP/quiet + seam | endpoint 和 idle interest | 有效 AF_INET endpoint、read/terminal-only quiet contract |
| 4.3-R1 | R5/R7 / 4/64/65 waiter | multiwaiter worker/parent/peer + seam | arm、trigger、record 聚合 | replacement barrier、N units、nonblocking recheck、choreography tests |
| all | D10 / automatic witness construction | Makefile | host/static/source Gate | 纳入新增 tests 并生成新鲜 artifact |

**Task Contracts**

### 4.1-R1: enforce the complete transcript phase order

- Requirement/Scenario: R7；D10；marker protocol。
- Depends on: None。
- Targets: `scripts/ms06-qemu-validate.py::validate_output/self_test`。
- Current behavior: PASS-before-revision/environment 被接受。
- Required behavior: 只接受 START、REVISION、ENVIRONMENT、12 个固定顺序 PASS、END、EXIT 0；任何 protocol
  marker 提前、延后、重复、未知或缺失均报告首个决定性差异。
- Required changes: 先增加 PASS-before-revision、PASS-between-metadata、metadata-after-first-PASS、EXIT-before-END
  等最小 RED；实现显式 phase 检查并保持普通 serial noise 容忍。
- Preserve: 纯输出 auditor、stdin/file CLI、expect metadata、timeout 和现有 malformed fixtures。
- Forbidden: 启动 QEMU、驱动 guest、猜测 exit、改变 12-case set 或接受 aggregate PASS。
- Test witness: `python3 scripts/ms06-qemu-validate.py --self-test`；Cycle 000 Review 的最小反例。
- GREEN condition: 原正例通过；全部原负例和新增 phase 负例非零拒绝。
- Verification: Python syntax/self-test、CLI 正反例、validator purity guard。
- Stop when: 合法 runtime 输出需要改变 marker grammar；返回 Plan，不自行放宽顺序。

### 4.2-R1: repair UDP endpoint and quiet readiness contracts

- Requirement/Scenario: R2/R3/R6/R7；D10；UDP progress、Active quiet。
- Depends on: None。
- Targets: `run_udp_progress`、`run_quiet`、相关 pure/test seam。
- Current behavior: UDP bind 的 family/address 未初始化；quiet 请求 OUT 并把正常 writable 判为失败。
- Required behavior: 两个 UDP socket 绑定 AF_INET loopback/port 0，payload 与 source endpoint 仍严格核对；quiet
  窗口只观察 IN/RDHUP/HUP/ERR，不因正常 writable 失败，窗口后同一连接必须完成有界 ping/echo。
- Required changes: 先新增 endpoint family/address/port 与 quiet interest/event RED；修正配置并保留每 case 单一终态。
- Preserve: fixed deadlines、public ABI、无 internal poll、无 sleep-based correctness、case marker 和 cleanup。
- Forbidden: 用 POLLOUT counter 代替 quiet、跳过 source 校验、改产品代码或把 host 运行冒充 QEMU runtime。
- Test witness: host seam 对 invalid/valid endpoint 与 quiet OUT/read/terminal combinations 的正反例；host syntax。
- GREEN condition: 新增 seam tests 与既有 22 项 tests 全过；source guard 仍通过。
- Verification: warnings-as-errors seam、probe syntax、focused tests 重复运行、full diff Review。
- Stop when: target guest 不支持 AF_INET loopback 或必要 poll event；记录 ABI 证据并返回 Plan。

### 4.3-R1: make 4/64/65 waiter choreography convergent

- Requirement/Scenario: R5/R7；D6/D10；poll/select/epoll multiwaiter、PollSet 64/65 boundary。
- Depends on: 4.2-R1 提供可用 listener/stream helper。
- Targets: `mw_wait_once`、`mw_worker_body`、`run_multiwaiter`、waiter seam tests。
- Current behavior: worker 在 wait 返回后上报 arm，parent 等 arm 后才触发；peer 只发送 1 byte，多个 blocking
  consumers 无法各完成一次。
- Required behavior: 4/64 waiter 在 trigger 早于或晚于注册时都依靠 check-register-recheck 最终完成；65 waiter
  在 data absent 时产生至少一个 replacement progress，parent 观察该 progress 后才释放 trigger；peer 提供 N 个
  consumable units，每个 distinct pid nonblocking 消费一个并恰好完成一次，被替换者记录 wake→not-ready
  recheck→re-register→completion。
- Required changes: 增加 parent/worker choreography model RED；拆分 progress/final record；移除循环 arm barrier；
  trigger count 绑定 `n_waiters`；所有 readiness 后的 consuming read 使用 nonblocking recheck；cleanup 保持 deadline。
- Preserve: exact 4/64/65、三种 waiter API、一个 public socket、不同进程身份、PollSet 64 容量、spurious wake
  仅作提示、12-case marker set。
- Forbidden: sleep 排序、单 waiter 聚合冒充 fan-out、降低并发、修改 PollSet、给每 waiter 单独 socket、只计 wake
  不做实际 I/O，或在本 Cycle 运行 QEMU。
- Test witness: model 覆盖 event-before-register、4/64 无 replacement、65 trigger-before-replacement 拒绝、首个
  replacement 后放行、N−1 units 拒绝、duplicate pid、partial completion、replacement 未 re-register；既有 record tests。
- GREEN condition: 所有 model/seam tests 通过，source review 不存在 wait-return→arm / arm→trigger cycle、single-unit
  fan-out 或 blocking consuming read。
- Verification: focused seam ×2、marker diff、host syntax、RISC-V static build与 64/65 marker inspection。
- Stop when: guest fork/task ABI 无法形成 65 distinct waker，或 replacement progress 无法在不修改 syscall/PollSet
  契约下被观察；返回 Plan，不降低 Acceptance。

**Invariants**

- runner 仍是唯一协议推进者；probe 不调用 `poll_interfaces()`。
- 每次 wake 都只触发实际 I/O recheck；replacement 本身不等于 readiness。
- 所有 case 使用 monotonic absolute deadline；错误、partial result 或 cleanup failure 返回非零并产生唯一 FAIL。
- marker、revision、environment、case 和 operator-provided exit 一一对应。

**Non-goals**

- QEMU runtime、host-test global isolation、automatic qualification 或 MS01/MS04/MS05 runtime。
- 修改 kernel/axnet/smoltcp、PollSet、scheduler、syscall、产品 socket 语义或 benchmark infrastructure。
- reset、SMP、真板、性能和全局 OpenSpec 状态维护。

**Acceptance**

1. Cycle 000 Review 的 PASS-before-metadata 反例及全部 phase 乱序被拒绝，完整 transcript 唯一通过。
2. UDP endpoint 与 quiet interest 的新增 true RED→GREEN tests 通过；UDP source/payload 和 quiet 后 liveness 保留。
3. multiwaiter model 证明 4/64 无循环 barrier、65 replacement-before-trigger、N-unit fan-out、nonblocking recheck、
   distinct pid 和 exactly-once completion；原 post-hoc record tests 保持 GREEN。
4. Python/C warnings-as-errors、既有与新增 host tests、marker/purity/source guards、RISC-V static build、strict
   OpenSpec、`git diff --check` 和完整 diff Review 通过；无未解决 Critical/Important finding。
5. 未启动 QEMU，未修改产品代码，Persisted Evidence 维持 `none`。

**Verification**

- validator self-test、Cycle 000 phase-order counterexample 和 CLI 正反例。
- host seam warnings-as-errors；focused endpoint/quiet/choreography 与完整 seam 各重复运行。
- probe C syntax；purity/internal-poll/sleep/blocking-consume/source-cycle guards。
- validator/probe case set diff；fresh RISC-V static build；`file`/marker inspection。
- `openspec validate ms06-application-visible-async-network-stack --strict`、相关 format、`git diff --check` 和
  current Cycle full diff Review。
- SKIPPED：QEMU、host-test isolation、automatic qualification、MS01/MS04/MS05 runtime；它们分别属于
  Iterations 008、006、007、008。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | parser phase、UDP/quiet 配置、poll/select/epoll register-recheck、PollSet 65 replacement 与 multiwaiter control/data flow 已定位 |
| Design | PASS | 65 replacement progress barrier、N-unit trigger、nonblocking consuming recheck 和完整 parser phase 已闭合 |
| Iteration Plan | PASS | 只修复 Iteration 005 既有 Tasks 4.1-4.3；006-008 依赖与边界不变 |
| Cycle Scope | PASS | 三个 repair items 一一对应 A1-A5，不修改产品语义或进入 runtime |
| Task Contracts | PASS | 每项含目标、RED/GREEN、保持/禁止、验证与停止条件；Act 无需回读父 Cycle |
| Traceability | PASS | R5-R7、D6/D10、Tasks 4.1-4.3 映射到 parser、UDP/quiet、multiwaiter 和 tests |
| Verification | PASS | 新增缺口见证、既有回归、static artifact、source/OpenSpec/diff Gate 均有通过条件 |

Gate 2 技术检查项全部 PASS；用户于 2026-08-27 批准（原话："更改gate状态，开始实施"），Gate 2 BLOCKED 解除，Plan Context 已置为 `ready`，`openspec-act` 获授权执行。

**Persisted Evidence**

- Mode: none

所有结果可低成本重跑，由 Act Response 记录命令、每项不超过 20 行的决定性输出、exit、文件和符号；不创建
Evidence 占位目录。

**Risks and Notes**

- Review sandbox 的 cross compiler 受 seccomp 限制；Act 若也无法执行 fresh static build，必须 blocked 返回，
  不能用 Cycle 000 artifact 替代。
- 65 waiter 的 runtime 资源可行性仍属于 Iteration 008。本 Cycle 证明可构建的 choreography 与 model contract，
  不声称 QEMU runtime PASS。
- Cycle 000 的两个 Minor finding继续保留给 Iteration 008 观察，不阻塞本 rework Acceptance。

## Act Response

- Status: reported（2026-08-27）

**Implemented**

Repair items 4.1-R1、4.2-R1、4.3-R1 全部完成，未启动 QEMU，未修改产品代码（kernel/axnet/smoltcp 零改动）。

- 4.1-R1：`scripts/ms06-qemu-validate.py` 用显式 `phase` 状态机（0=await REVISION → 1=await ENVIRONMENT → 2+=await PASS n）替换互不约束的分支解析。PASS 只能在两个 metadata 之后出现（`phase < 2` 拒绝）；REVISION 只能在 phase 0（`phase != 0` 拒绝）；ENVIRONMENT 只能在 phase 1（`phase != 1` 拒绝，同时覆盖"environment 早于 revision"与"environment 在 PASS 之后"）。`self_test` 新增 4 类 phase 反例：passes_before_meta（Plan Review 被错误接受的 A1 反例，全 12 PASS 先于 metadata）、pass_between_meta（PASS 夹在两个 metadata 之间）、metadata_after_pass（metadata 出现在首个 PASS 后）、exit_before_end（EXIT 先于 END）。serial noise 容忍与既有全部负例行为保留。
- 4.2-R1：修复 UDP 与 quiet 契约。
  - 新增 seam 决策 `ms06_udp_bind_spec_valid()`：bind spec 必须有显式 `AF_INET` + loopback 地址（port 0 为合法 ephemeral 规格，固定 port 仍有效，INADDR_ANY/NULL/zeroed 拒绝）。`run_udp_progress` 的 `aa/ba` 从零化直接 bind 改为设置 `sin_family=AF_INET`、`htonl(INADDR_LOOPBACK)`、`sin_port=0` 并经 seam 校验后再 bind；payload/source endpoint 严格核对保留。
  - `ms06_events_satisfy(QUIET)` 的 forbidden 集合去掉 `MS06_EV_OUT`（established socket 正常可写不再判为噪声），只禁 IN/ERR/HUP/RDHUP；新增 seam `ms06_quiet_interest()` 返回 `POLLIN|POLLRDHUP`（不 arm OUT）。`run_quiet` 窗口用 `ms06_quiet_interest()`，观测到的 revents 交由 `ms06_events_satisfy(QUIET, ev)` 判决，窗口后 liveness ping/echo 保留。
- 4.3-R1：multiwaiter choreography 收敛。
  - 移除 wait-return→arm / arm→trigger 循环：worker 不再等首个 wait 返回后写 "A" arm，parent 不再等待 n 个 arm 报告。
  - 管道 `arm` 语义改为带类型 progress 通道 `prog`：worker 仅在首次提交 replacement-class 记录（wake→not-ready recheck）后写一个 `'R'`；parent 对 65 case 先读一个 `'R'` 再放行（`read_byte_deadline` + 超时），对 4/64 case 无任何 barrier 直接放行。
  - 新增 seam 决策 `ms06_trigger_release_ok(n_waiters, capacity, replacement_required, saw_replacement)`：n≤capacity 无需 barrier（`!replacement_required`），n>capacity 必须 `replacement_required && saw_replacement`；parent 以 `MS06_WAITER_CAPACITY=64` 调用并接受 EVENT_MISMATCH 拒绝。
  - trigger 从单字节改为 N-unit：connector 读 `'G'` 后 `read_full_deadline(down[0], units, n_waiters)` 再逐字节 send，N=n_waiters，每个 distinct waiter 恰好有一个可消费单元。
  - 所有消费 recheck 改 nonblocking：IN 路径 `recv(MSG_DONTWAIT)`，EAGAIN 时 re-check 循环（收敛）而非失败；replacement 路径 peek 后 `recv(MSG_DONTWAIT)`；不引入 blocking consuming read。
  - 被替换 worker 记录 `WOKEN→RECHECK_NG→REREGISTERED→completion` 全链且 `progress_reported` 只发布一次 `'R'`，写管总量有界（≤65 bytes），无满管阻塞风险。

**Changed Files and Symbols**

| 文件 | 性质 | 关键符号 |
|---|---|---|
| `scripts/ms06-qemu-validate.py` | 修改 | `validate_output`（phase 状态机）、`self_test`（4 个 phase 反例 + `transcript_lines`） |
| `tests/ms06_stack_readiness_probe.c` | 修改 | `MS06_WAITER_CAPACITY`、`ms06_events_satisfy`(QUIET)、`ms06_udp_bind_spec_valid`、`ms06_quiet_interest`、`ms06_trigger_release_ok`、`run_udp_progress`、`run_quiet`、`mw_cfg.prog_fd`、`mw_worker_body`、`run_multiwaiter`（connector/worker/parent 编排） |
| `tests/ms06_stack_readiness_probe_test.c` | 修改 | `test_quiet_contract_ignores_writable_rejects_activity`、`test_quiet_interest_excludes_writable`、`test_udp_bind_spec_rejects_zeroed_and_accepts_loopback`、`test_trigger_release_at_or_below_capacity_needs_no_barrier`、`test_trigger_release_overflow_requires_replacement_progress`（22→26 项） |
| `tests/ms06_stack_readiness_probe` | 重建 | fresh RISC-V static artifact，`MS06_REVISION_DEFAULT='1ea51427…'` |
| `openspec/changes/…/tasks.md` | 文档 | Current Cycle 段状态 |
| 本 Cycle 文件 | 文档 | Plan Context（Gate 2 批准记录）与本 Response |

产品代码（kernel/crates/smoltcp/src）零改动。

**Deviations from Plan**

1. Plan 建议"拆分 progress/final record"：实际将原 `arm` 管道整体重命名为带类型 `prog` 通道并只承载 `'R'` progress，final record 仍走 `res` 管道——语义与 Plan 一致，命名更贴合职责。
2. quiet 窗口事件判定原为"任何 revents 即 mismatch"；为消除 guest 判决与 seam contract 的理论分歧，改为以 `ms06_events_satisfy(QUIET, ev)` 作为唯一真值源（对 IN/ERR/HUP/RDHUP 等价，对理论上的 OUT-only revents 不再误判）。等价实现，不改变验收语义。
3. 验证命令按 Makefile 既有配方（seam 不带 `-O2`）执行，另按 Cycle 000 先例补跑两次 focused；等价调整，不影响判定。

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

Spec compliance review（逐 repair item 对照 Task Contract）：

- 4.1-R1：只接受完整 phase 顺序（START→REVISION→ENVIRONMENT→12 PASS→END→EXIT 0）✓；任何 marker 提前/延后/重复/未知/缺失报告首个决定性差异 ✓；保留纯输出 auditor、stdin/file CLI、expect metadata、timeout 与既有 malformed fixtures ✓；未启动 QEMU、未驱动 guest、未改变 12-case set、无 aggregate PASS ✓；Cycle 000 的最小反例（PASS-before-metadata）现被拒绝 ✓。
- 4.2-R1：两个 UDP socket 绑定 AF_INET loopback/port 0 ✓；payload + source endpoint 严格核对保留 ✓；quiet 窗口只 arm/观察 read/terminal/error 方向，正常 writable 不失败 ✓；窗口后同一连接完成有界 ping/echo ✓；未用 POLLOUT counter 代替 quiet、未跳过 source 校验、未改产品代码 ✓。
- 4.3-R1：4/64 在 trigger 早于或晚于注册时均凭 check-register-recheck 收敛（无 barrier）✓；65 在 data absent 时产生 ≥1 replacement progress，parent 观察后才释放 trigger ✓；peer 提供 N 个可消费单元，每个 distinct pid nonblocking 消费一个且恰好完成一次 ✓；被替换者记录 wake→not-ready recheck→re-register→completion 全链 ✓；model 覆盖 event-before-register、4/64 无 replacement、65 trigger-before-replacement 拒绝、首个 replacement 后放行、N−1 units 拒绝（set count）、duplicate pid、partial completion、replacement 未 re-register ✓。
- Invariants：runner 仍是唯一协议推进者，probe 不调用 `poll_interfaces()` ✓；wake 只触发实际 I/O recheck ✓；所有 case 使用 monotonic absolute deadline，失败产生唯一 FAIL 与非零退出 ✓；marker/revision/environment/case/exit 一一对应 ✓。
- Non-goals：未启动 QEMU ✓；未做 host-test isolation（006）/automatic qualification（007）/runtime（008）✓；未修改 kernel/axnet/smoltcp/scheduler/PollSet ✓。

Code quality review（完整 unstaged diff，即本 Cycle 全部改动）：

- diff 仅覆盖 4.1-R1/4.2-R1/4.3-R1 目标文件 + gate 记录 + artifact 重建，无计划外修改 ✓。
- parser phase 状态机与既有 dup/order/unknown/missing 分支正交，错误消息均为首个决定性差异 ✓。
- 三个新 seam 函数单一职责、无副作用、guest 与 host 共用同一契约 ✓。
- multiwaiter：progress 通道写入有界（每 worker 至多 1 字节 `'R'`），消费路径全部非阻塞，`continue` 收敛有 deadline 兜底 ✓；connector 读 N units 经 `read_full_deadline`，失败路径退出码非零，parent 检测后重分配/kill ✓。
- `test_quiet_contract_ignores_writable_rejects_activity` 覆盖 OUT 单独与组合；`test_udp_bind_spec_*` 覆盖 zeroed/ephemeral/fixed/ANY/NULL；`test_trigger_release_*` 覆盖 barrier hold/unlock/misconfig ✓。
- 编译零警告（`-Wall -Wextra -Werror`），无死代码、无重复实现。

修复的发现（本 Cycle 内）：
- （已修复，Critical）原 `run_udp_progress` bind 前仅 memset——已补全 AF_INET/loopback/port 并经 seam 校验。
- （已修复，Critical）原 quiet 窗口 arm POLLOUT 并判任何 revents 为噪声——已移除 OUT、以 seam contract 为唯一判决。
- （已修复，Critical）原 multiwaiter 循环 barrier + 单字节 trigger + blocking consume——已改为 4/64 无 barrier、65 replacement-progress barrier、N-unit trigger、nonblocking recheck。

遗留 Minor 问题（保留自 Cycle 000 Review Finding 5，供 Iteration 008 观察）：
- (a) `run_multiwaiter` 成功路径以 `MS06_EV_IN` 填充 `r.events` 满足 verdict 形参契约，非实测快照。
- (b) epoll 分支遇 RDHUP-only wake 会走 replacement-class 路径并在 peek 得 0 时判败；waiter 场景对端不关闭，合同范围内无影响。
两项均不阻塞本 Cycle Acceptance。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| validator self-test（含 4 个新 phase 反例） | `python3 scripts/ms06-qemu-validate.py --self-test` | `PASS: ms06-validator-self-test`，exit 0 | PASS |
| Plan Review A1 反例（PASS-before-metadata） | `python3 scripts/ms06-qemu-validate.py /tmp/ms06-accepted-invalid.txt` | `FAIL: ... PASS marker 'tcp-timer' appears before revision/environment metadata`，exit 1 | PASS |
| validator CLI 正例 | full 12-case transcript + `--expect-revision --expect-environment` | `PASS: ms06-transcript-valid`，exit 0 | PASS |
| host seam 决策测试（新增 5 项含 UDP/quiet/choreography） | `cc -std=c11 -Wall -Wextra -Werror tests/ms06_stack_readiness_probe_test.c` 后执行 ×2 | `ms06 probe decision tests: 26 passed` ×2，exit 0 | PASS |
| probe C syntax | `cc -std=c11 -Wall -Wextra -Werror -fsyntax-only tests/ms06_stack_readiness_probe.c` | 无输出，exit 0 | PASS |
| RISC-V static build（fresh artifact） | `make tests/ms06_stack_readiness_probe` | musl gcc `-static -no-pie -Os -Werror` 通过；`file`：ELF 64-bit LSB executable, UCB RISC-V, statically linked | PASS |
| 64/65 marker 在产物中 | `strings tests/ms06_stack_readiness_probe \| grep -cE '^(waiter-64\|waiter-65-reregister)$'` | `2` | PASS |
| marker set 精确性 | validator/probe 双侧 `--print-cases` + `diff -u` | 无差异 | PASS |
| 全量 host-test 回归 | `make host-test`（含 MS01–MS05 host 套件 + MS06 各步 + purity guards） | 尾部步骤全过（validator self-test、syntax、seam ×2、case diff），退出码 0 | PASS |
| OpenSpec strict | `openspec validate ms06-application-visible-async-network-stack --strict` | `Change 'ms06-application-visible-async-network-stack' is valid`，exit 0 | PASS |
| diff 卫生 | `git diff --check` | 无输出，exit 0 | PASS |

**Persisted Evidence**

None required — Plan 设定 Mode 为 `none`；所有结果可低成本重跑（上表命令），决定性输出与退出码已在本 Response 保存，未创建 Evidence 目录。

**Experience Candidates**

None — 本 Cycle 为纯 witness-construction 修复，全部命令可重复且由 Act Response 完整记录，不构成 Runbook/Incident 候选。

**Remaining Issues**

- guest 运行时 65 并发进程与 PollSet 真实替换行为的资源可行性仍属 Iteration 008 验证边界；本 Cycle 只证明可构建的 choreography 与 model contract，不声称 QEMU runtime PASS（计划内既定边界）。
- 遗留 Minor (a)(b) 见 Self-Review，已记录且不阻塞 Acceptance。

**Commit or Diff Reference**

未提交（未获授权）。本 Cycle 工作树增量：修改 `scripts/ms06-qemu-validate.py`、`tests/ms06_stack_readiness_probe.c`、`tests/ms06_stack_readiness_probe_test.c`、change `tasks.md`、本 Cycle 文件（Gate 状态与本 Response）；重建 `tests/ms06_stack_readiness_probe`。

## Plan Review

- Review Result: replan-required

**Findings**

1. **Blocking — 65-waiter 的 pre-data progress 不可由用户态 `poll/select/epoll` 观察。** 当前 worker
   只有在 `mw_wait_once()` 返回 `rc > 0` 且没有 readiness bits 时才写 `'R'`。StarryOS syscall 通过
   `poll_io()` 执行 check → register → recheck；replacement waker 只使内核 future 重新 poll。fd 仍未 ready
   时 closure 再次返回 `WouldBlock`，syscall 保持 Pending，不会以“空事件”返回用户态。因此 parent 等待的
   `'R'` 不可达，`waiter-65-reregister` 必然在 barrier 超时，4.3-R1 与 Acceptance 3 未满足。
2. **Blocking — validator 仍接受 START 前的 protocol marker。** parser 丢弃 `_head`，因此 START 前的
   `FAIL:`、`MS06_HARNESS_EXIT: 1` 或 `PASS:` 均被当作普通 serial noise。4.1-R1 要求任何 protocol marker
   只能出现在合法 phase，Acceptance 1 尚未闭合。
3. **Blocking — N-unit witness 缺少对应的 model test。** 实现确实发送 `n_waiters` 个 units，但新增 seam
   只判断 replacement barrier，不校验 unit count。Act Response 所称“N−1 units 拒绝（set count）”只验证
   final record 数，不验证 peer stimulus，未满足 4.3-R1 Test witness。
4. **Accepted partial result — UDP endpoint 与 quiet interest 缺口已关闭。** 实际配置、seam tests 和
   host syntax 与 4.2-R1 一致，无需返工。
5. **Non-blocking Minor — Cycle 000 延续的代表性 events 与 epoll RDHUP-only finding 保留。** 它们不影响
   本次 replan 决策，也不进入当前 Acceptance gap。

**Deviation Classification**

`PLAN-INVALID`（用户态 pre-data replacement progress 与 syscall 契约冲突）；`ACT-DEVIATION`（validator
未扫描 START 前 protocol marker，N−1 unit test 未建立）；`NEW-EVIDENCE`（`poll_io` 与 epoll 内部重检路径）。

**Acceptance Gaps**

- Task 4.1：完整 transcript 必须拒绝 witness START 前的 PASS/FAIL/MS06 protocol marker，同时保留普通
  shell/serial noise。
- Task 4.3：不得等待用户态不可见的 empty-event progress。exact 64/65 必须使用可同步确认注册完成的公开 ABI
  建立 trigger barrier，并把 replacement/re-register 机制证据与 application eventual-completion 证据分层。
- Task 4.3 test witness：必须直接覆盖 `trigger_units == waiter_count`，不能用 final record count 代替 stimulus。

**Convergence**

expanded — UDP/quiet gap 已关闭，但 65-waiter 原 repair contract 被 syscall 新证据判定无效，并发现 START 前
protocol marker 与 stimulus-count 两个未覆盖缺口。

**Evidence**

- `axtask::future::poll_io`：同步 operation 返回 `WouldBlock` 后 register 并再次检查；replacement wake 后仍不
  ready 时返回 `Poll::Pending`，不会把空事件暴露给用户态。
- `kernel/src/syscall/io_mpx/poll.rs:55-81`、`select.rs:110-143`：只有 closure 得到非空 readiness 才返回成功，
  否则继续 `WouldBlock`。
- `kernel/src/file/epoll.rs:162-183,365-422`：replacement wake 将 interest 放入 ready queue；consume 发现
  NoEvent 后在内核 `register_waker_only()`，用户态 `epoll_wait` 继续 Pending。
- `tests/ms06_stack_readiness_probe.c:1347-1414,1510-1522`：worker 只有 wait 返回空事件才写 `'R'`，parent 在
  发送 data 前等待 `'R'`。
- 新鲜验证：validator self-test PASS；26 项 seam ×2 PASS；C syntax PASS；strict OpenSpec PASS；diff check
  PASS。这些结果证明现有 tests 可重复，但没有覆盖 syscall 可达性。
- 审计反例：分别在合法 transcript 前加 `FAIL: stale-before-start`、`MS06_HARNESS_EXIT: 1`、
  `PASS: tcp-timer`；三项均输出 `ACCEPTED_PRESTART_PROTOCOL`，validator 未拒绝。
- focused axnet Rust test 的直接 Cargo 调用因现有 x86_64 per-CPU relocation 链接配置失败（exit 101），属于
  命令/环境边界，不作为产品失败；当前结论由源码控制流和已存在的 PollSet tests 支持。
- Persisted Evidence：`none`；Evidence 目录不存在不是问题。

**Follow-up Decision**

当前 65-waiter contract 需要改变验证方法，不能在同一 rework Cycle 内要求 Act 继续实现不可达信号。更新
Task 4.1、Task 4.3、D10 和 Iteration 005 verification boundary，并创建 `002-replan.md`。Iteration 006 不展开。

**Iteration Plan Update**

Iteration 005 的目标与 requirement 不变；验证契约改为：

- exact 64/65 worker 使用每进程独立 epoll instance；`epoll_ctl(ADD)` 同步完成 socket InterestWaker 注册后
  才报告 arm，parent 收齐精确 64/65 个 arm 再发送 N units。
- 第 65 次同步注册必然触发 PollSet replacement；replacement/no-event recheck 与 re-register 由既有
  PollSet + `Epoll::consume(NoEvent) → register_waker_only()` host/source 证据证明；guest 只证明 65 个 distinct
  waiter 最终各完成一次，不伪造用户态 replacement notification。
- 普通 poll/select/epoll 4-waiter 场景仍保留；marker set 和 QEMU runtime 边界不变。

**Next Cycle**

`002-replan.md`

**Next Iteration**

None；Iteration 005 尚未 accepted，Iteration 006 保持 map-only。
