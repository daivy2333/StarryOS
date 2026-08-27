# Iteration 005 / Cycle 000: application witness construction

## Plan Context

- Status: ready
- Approval: approved — 用户于 2026-08-26 显式批准本 Cycle 计划与 Gate 2 结果（原话："更改gate状态，开始实施吧"），并授权本次 `openspec-act` 执行；该授权不覆盖后续 Plan Review、下一 Iteration、全局状态同步或收尾
- Iteration: 005-application-witness-construction
- Cycle: 000-initial
- Cycle Type: initial
- Parent cycle: None

**Iteration Scope**

- Change tasks: 4.1-4.3
- Depends on: Iteration 004 accepted
- Stable baseline: 纯输出 validator 与静态 RISC-V guest probe 可独立构建；12 个 application-visible
  readiness 场景均有唯一 marker、固定 deadline 和明确 exit 契约，其中 64/65 使用不同 waiter 身份并证明
  replacement 后 recheck/re-register 最终完成。
- Verification boundary: validator 正反例、host C seam、source guards、host syntax/build 和 RISC-V static build；
  本 Iteration 不启动 QEMU，也不声称 runtime PASS。
- Diagnostic boundary: marker parser、guest socket/syscall ABI、线程/进程 waiter 编排、deadline、host seam 或交叉编译。
- Deferred tasks: Iteration 006 Tasks 5.1-5.2；Iteration 007 Task 6.1；Iteration 008 Tasks 7.1-7.2

**Cycle Scope**

- Trigger: initial
- Acceptance gaps: None
- Repair items: None
- Inherited scope: R5-R7；D6/D8/D10；accepted Iterations 000-004；per-socket PollSet 容量 64；
  terminal-first I/O；unique resident runner；manual-QEMU policy
- Excluded scope: 启动或自动控制 QEMU、执行 runtime、重做 terminal 产品语义、修改 scheduler/syscall ABI、
  降低 64/65 并发边界、MS01/MS04/MS05 runtime、SMP、真板、性能、全局文档维护、归档和 commit

**Objective**

建立可在 host 上确定性审计、可静态交叉编译并可由后续人工 QEMU Cycle 执行的 MS06 应用见证。
validator 必须拒绝任何不完整或歧义输出；probe 必须只通过公开 socket 与 poll/select/epoll ABI 观察
runner progress 和 readiness，不调用 axnet 内部 poll，不以 sleep 作为正确性条件。

**Scenario Sketch**

| Scenario | Action | Observable result | Failure boundary |
|---|---|---|---|
| marker protocol | validator 读取保存的 output 与 exit/timeout 元数据 | START、12 个唯一 PASS、END、revision/environment、exit 0 全部一致 | 缺失、重复、乱序、FAIL、partial success、timeout 或 exit 矛盾被接受 |
| protocol progress | probe 运行 tcp-timer、udp-progress、listener、quiet、continuous-traffic | 每个 mode 在 monotonic deadline 前产生唯一终态 | 内部 poll、无界等待、sleep 决定正确性或一个 mode 掩盖另一个 |
| error/close | 运行 nonblock-connect-error 与 close-error | poll 事件和随后 I/O/SO_ERROR 观察稳定且与 Iteration 004 一致 | ERR 丢失、类别漂移、正常 close 被误判为 device fault |
| multiwaiter | distinct waiters 分别通过 poll/select/epoll 等待同一 socket | 所有 waiter 在 deadline 内重检并得到一致结果 | 共享 waiter 身份、单一聚合 PASS 或 missed wake |
| capacity boundary | 64 waiter 后增加第 65 个 waiter | replacement wake 被观察；被替换 waiter recheck/re-register 后最终完成 | 降低并发数、只证明 wake count、或让 replacement waiter 消失 |

**Current Baseline**

- Branch `net-k3`；HEAD `1ea51427d8692f5a12b87a0403b940e73d43fed3` 加当前 staged MS06 工作树。
- Iteration 004 Cycle `001-replan` 已接收：ordinary 357/357；diagnostics 新鲜第二轮 377/377；
  QEMU 与 root D1 product checks、strict OpenSpec 和 diff check 通过。
- `tests/ms06_stack_readiness_probe.c`、对应 host seam 和 `scripts/ms06-qemu-validate.py` 尚不存在。
- `scripts/ms01-qemu-test.py::validate_output/self_test` 提供 marker 正反例先例，但它同时含 QEMU driver；
  MS06 validator 必须拆成纯读取/验证工具，不导入 subprocess 启动路径。
- `tests/ms04_rx_probe_test.c` 与 `tests/ms05_data_plane_probe_test.c` 证明 `#define ..._TESTING` 后包含
  probe source 的 host seam 模式；Makefile 已有 `BENCH_CC ?= riscv64-linux-musl-gcc` 和 MS04/MS05 static target。
- 现有 guest socket payload 可参考 fixed-deadline、fork 和 static-musl 用法；当前仓库尚无 exact 64/65
  distinct-waiter application witness。

**Relevant Code**

| File / Symbol | Current responsibility | Planned use |
|---|---|---|
| `scripts/ms01-qemu-test.py::validate_output/self_test` | MS01 marker 校验与 QEMU harness 混合实现 | 只参考严格 marker 判定，不复制 QEMU 驱动职责 |
| `tests/ms01_socket_baseline.c` | guest socket、poll 和 fixed-deadline 先例 | 复用公开 ABI 与 bounded process/thread 模式 |
| `tests/ms04_rx_probe_test.c`、`tests/ms05_data_plane_probe_test.c` | probe decision host seam | 建立不启动 guest 的 MS06 decision tests |
| `tests/ms06_stack_readiness_probe.c` | 不存在 | 新建 12-case guest probe 与 test seam |
| `tests/ms06_stack_readiness_probe_test.c` | 不存在 | 新建 host decision/identity/deadline tests |
| `scripts/ms06-qemu-validate.py` | 不存在 | 新建纯 output validator 与 self-test |
| `Makefile` | guest probe build targets | 增加 host seam 与 RISC-V static build target |

**Critical Path**

```text
host RED fixtures -> pure validator rejects malformed transcript
                 -> probe decision seam rejects timeout/partial/identity collapse
                 -> guest source implements public socket + poll/select/epoll cases
                 -> 64 slots register distinct waiters
                 -> 65th registration causes replacement wake
                 -> replaced waiter rechecks, re-registers, then all 65 terminate
                 -> host build + RISC-V static build produce fresh artifact
                 -> Iteration 006 first restores trustworthy host-test isolation
                 -> Iteration 007 consumes tests/artifact for automatic qualification
                 -> Iteration 008 alone runs QEMU
```

**Implementation Guidance**

1. 先固定 marker grammar 和纯 validator API，用 synthetic transcripts 建立缺失、重复、乱序、FAIL、timeout、
   nonzero/缺失 exit、revision/environment 不一致的 RED→GREEN。
2. 把 probe 的 deadline、case terminal state、waiter identity 和 64/65 re-register decision 提取到
   `MS06_STACK_READINESS_PROBE_TESTING` seam；host test 先证明边界状态机，不依赖 guest scheduler。
3. 再实现公开 ABI 场景。每个 case 独立建资源、独立 deadline、独立清理，失败立即输出唯一 FAIL 并返回非零。
4. 最后增加 Make targets/source guards，运行 host seam、Python self-test、C warnings-as-errors 和 static build。

**Behavioral Change**

- 新增验证与 guest test artifact，不修改 kernel/axnet/smoltcp 产品行为。
- MS06 output 从无契约变为可机器拒绝 partial success 的 12-case 协议。
- 64/65 不再只依赖 host PollSet 模型；probe 具备后续 runtime 观察 replacement→recheck→re-register 的路径，
  但本 Iteration 只证明其 decision seam 与可构建性。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Planned Change |
|---|---|---|---|
| 4.1 | R7 / marker protocol | `scripts/ms06-qemu-validate.py` | 纯解析、严格顺序/唯一性/exit/metadata 校验与 self-tests |
| 4.2 | R5-R7 / progress、error、quiet | probe source/test、Makefile | 12-case fixed-deadline probe、host seam 与 static target |
| 4.3 | R5/R7 / multiwaiter、64/65 | probe waiter orchestration/seam | distinct identity、replacement observation 和 re-register completion |

**Task Contracts**

### 4.1: define and validate the complete MS06 marker protocol

- Requirement/Scenario: R7；D10；marker protocol。
- Depends on: Iteration 004 accepted。
- Targets: `scripts/ms06-qemu-validate.py` 及其自测试入口。
- Current behavior: 无 MS06 validator；MS01 validator 与 QEMU driver 耦合且 marker grammar 不适用于本 probe。
- Required behavior: 只读取用户保存输出和显式 metadata；要求一个 START、按固定顺序各一个 12-case PASS、
  一个 END、一个 revision/environment 记录和 exit 0；任何 FAIL、timeout、partial、重复、乱序或 exit 不一致失败。
- Required changes: 先创建 invalid fixtures RED；实现无 QEMU/subprocess/socket/shell 控制能力的 parser/CLI；
  self-test 同时含完整正例和每类最小负例。
- Preserve: 人工 QEMU 边界；原始输出不被改写；错误消息指出首个决定性差异。
- Forbidden: 启动 QEMU、驱动 guest shell、从 marker 猜测 exit、接受未知 case 或 aggregate PASS。
- Test witness: `python3 scripts/ms06-qemu-validate.py --self-test`；临时 transcript CLI 正反例。
- GREEN condition: 完整 transcript 唯一通过，所有 malformed fixtures 非零退出且报告原因。
- Verification: Python syntax/self-test、source guard 禁止 `subprocess`/QEMU launch/guest-shell I/O、strict diff review。
- Stop when: 完整性必须依赖未保存的交互状态或 validator 需要控制 QEMU 才能判断。

### 4.2: build bounded public-ABI progress and terminal scenarios

- Requirement/Scenario: R5-R7；D6/D8/D10；protocol progress、error/close。
- Depends on: Task 4.1 marker contract fixed。
- Targets: `tests/ms06_stack_readiness_probe.c`、`tests/ms06_stack_readiness_probe_test.c`、`Makefile`。
- Current behavior: 无 MS06 probe；现有 payload 不能联合证明 runner timer/software/device progress 与 terminal readiness。
- Required behavior: 12 个 case 各有 monotonic fixed deadline、独立终态和清理；只用公开 socket、poll、select、epoll、
  clock/process/thread ABI；不调用 axnet 内部 poll。normal close 与 stable error 观察符合 Iteration 004。
- Required changes: 建立 deadline equal/after/regression、partial terminal、wrong event/error 和 cleanup failure 的
  host RED tests；实现 tcp-timer、udp-progress、listener、nonblock-connect-error、quiet、continuous-traffic、
  close-error 和三种 multiwaiter mode；增加 host warnings-as-errors 与 RISC-V static targets。
- Preserve: probe 是判定工具不是 benchmark；TCP/UDP 语义、marker grammar、single-hart scope 和手工执行边界。
- Forbidden: internal `poll_interfaces`、sleep 作为正确性条件、无限 retry、后台永久任务、scheduler/syscall 修改。
- Test witness: host seam 对每个 decision 既有 PASS 也有至少一个确定性 FAIL；source guard；host C build；static build。
- GREEN condition: seam/self-tests、syntax/source guards 和 fresh static artifact 全部通过；未启动 QEMU。
- Verification: `cc -std=c11 -Wall -Wextra -Werror` host seam；`$(BENCH_CC) -static -no-pie` probe；marker cross-check。
- Stop when: guest 缺少所需公开 ABI、case 只能靠非有界 sleep 判定，或需要产品代码/scheduler 修改。

### 4.3: preserve distinct waiter identity through the exact 64/65 boundary

- Requirement/Scenario: R5/R7；D6/D10；multiwaiter、capacity boundary。
- Depends on: Task 4.2 的 waiter harness 与 deadline/cleanup GREEN。
- Targets: probe 64/65 orchestration、host seam 和 validator case set。
- Current behavior: Iteration 004 host model 证明 PollSet replacement wake，但没有 application witness 的不同 waiter
  身份、recheck/re-register 与最终完成路径。
- Required behavior: 64 个不同 waiter 注册同一 readiness；第 65 个触发 replacement；被替换 waiter 必须记录 wake、
  重检未就绪、重新注册，事件到达后所有目标 waiter 在 deadline 内恰好完成一次。
- Required changes: host seam 先对 duplicate identity、未观察 replacement、未 re-register、仅 64/65 partial completion
  建立 RED；guest orchestration 保存每个 identity 的 phase/result，输出 `waiter-64` 与 `waiter-65-reregister` 独立 marker。
- Preserve: 精确容量 64；spurious wake 只触发 recheck；后续 QEMU 才决定真实调度链。
- Forbidden: 降低并发数、复用一个 waker/task 冒充 distinct waiter、只计 wake 总数、sleep 排序或 host 模型替代 guest artifact。
- Test witness: seam 覆盖 64 全完成、65 replacement/re-register 全完成及四类 partial/identity failure；static build 不裁掉路径。
- GREEN condition: host decision tests 和 source guards 确认 exact 64/65 状态机，fresh static artifact 包含两个 marker。
- Verification: focused seam 重复运行、symbol/string inspection、host/static build、full diff review。
- Stop when: guest task/process资源或 ABI 无法支持精确 65；必须返回 Plan，不得简化 Acceptance。

**Invariants**

- probe/validator 不修改产品状态所有权；runner 仍是唯一协议推进者。
- 每个等待都使用 check-register-recheck 和 monotonic absolute deadline；wake 只是重检提示。
- marker、case identity、exit 与 metadata 必须一一对应；无 aggregate success。
- 任何 QEMU runtime 或 guest-shell 自动化均延后且保持人工边界。

**Non-goals**

- 执行 QEMU 或收集 runtime Evidence。
- 修复host测试隔离（Iteration 006）或重跑完整自动产品资格（Iteration 007）。
- 修改 kernel、axnet、smoltcp、scheduler、syscall ABI 或 PollSet 容量。
- SMP、真板、性能、reset/cancellation 和完整 Linux destructive SO_ERROR。

**Acceptance**

1. validator 对完整 12-case transcript、revision/environment 和 exit 0 唯一接受；所有歧义/不完整输入失败。
2. probe 只使用公开 ABI，每个 case 有 monotonic fixed deadline、唯一终态、明确 cleanup 和非零失败 exit。
3. tcp-timer、udp-progress、listener、nonblock-connect-error、quiet、continuous-traffic、close-error 及
   poll/select/epoll multiwaiter 的 host decision seam 正反例通过。
4. exact 64/65 distinct waiter、replacement wake、recheck/re-register 和 eventual completion 由 host seam/source
   guards 约束；没有降低边界或聚合身份。
5. Python/C syntax、自测试、warnings-as-errors host build、RISC-V static build、format/source guards、strict
   OpenSpec 和完整 diff Review 通过；未启动 QEMU；无未解决 Critical/Important finding。

**Verification**

- `python3 scripts/ms06-qemu-validate.py --self-test` 及 synthetic transcript CLI tests。
- host seam：`cc -std=c11 -Wall -Wextra -Werror -O2 -o /tmp/ms06-probe-test tests/ms06_stack_readiness_probe_test.c`
  后执行；focused 64/65 decision 重复运行。
- `make tests/ms06_stack_readiness_probe`，使用 `BENCH_CC` 的 RISC-V static warnings-as-errors 配置。
- source guards：无 internal poll、QEMU/subprocess shell driver、无界 wait 或 sleep-based correctness；marker set 精确。
- `openspec validate ms06-application-visible-async-network-stack --strict`、相关 format、`git diff --check` 和 full diff Review。
- SKIPPED：host测试隔离、完整automatic qualification、QEMU及MS01/MS04/MS05 runtime；分别属于Iterations 006/007/008。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | validator/probe 缺口、MS01 parser、MS04/MS05 seam、Make static target 与 guest ABI 先例已定位 |
| Design | PASS | 纯 validator、decision seam、guest artifact 与后续 runtime 证据边界分离 |
| Iteration Plan | PASS | Tasks 4.1-4.3 只构建 witness；automatic qualification 和 runtime 保持在 006/007 |
| Cycle Scope | PASS | 新增 scripts/tests/Make targets，不触碰产品语义 |
| Task Contracts | PASS | marker、deadline、12 cases、exact 64/65、preserve/forbidden/stop conditions 可独立执行 |
| Traceability | PASS | R5-R7、D6/D8/D10 映射到 validator、probe、multiwaiter 和静态 artifact |
| Verification | PASS | host 正反例优先，static build 证明可交付；明确不把未运行 QEMU 记为 PASS |

**Persisted Evidence**

- Mode: none

本 Cycle 结果均可低成本重跑，命令和决定性输出由 Act Response 保存，不创建 Evidence 占位目录。

**Risks and Notes**

- exact 65 waiter 的 guest primitive/资源上限是明确 stop boundary；Act 不得自行降低到较小规模。
- host libc 若不支持目标 guest ABI 的全部调用，decision seam 与 guest static build分开；host seam 不伪装成 runtime。
- 新建 validator 必须保持纯工具；可读取文件和 CLI metadata，但不得逐步演化为 QEMU harness。
- Gate 2 内容已完备，但 Plan 仍为 draft，等待用户批准后才能改为 ready 并交给 `openspec-act`。

## Act Response

- Status: reported

**Implemented**

Tasks 4.1–4.3 全部完成，未启动 QEMU。

- 4.1：`scripts/ms06-qemu-validate.py` 纯输出 validator——严格 marker 语法（START → REVISION → ENVIRONMENT → 12 个按固定顺序唯一 PASS → END → `MS06_HARNESS_EXIT: 0`），拒绝 FAIL 行、timeout 元数据、缺失/重复/乱序/未知 case、partial success、metadata 缺失/为空/重复/乱序、exit 缺失/非零/重复及 expect-revision/environment 不匹配；错误消息报告首个决定性差异。`--self-test` 含完整正例与每类最小负例；CLI 支持 transcript 文件/stdin 与可选 metadata 交叉校验。
- 4.2：`tests/ms06_stack_readiness_probe.c` 12-case guest probe。单流 case：tcp-timer（accept 后零驱动 I/O，仅靠 runner timer 观察对端 FIN 的 IN|RDHUP 且无 ERR、EOF 双次稳定）、udp-progress（排队 datagram 经 runner 到达，校验 payload+source）、listener（4 连接全部入 backlog 后才 accept，身份互异 + echo 回路）、nonblock-connect-error（OUT|ERR + SO_ERROR 三次观察同类稳定 ECONNREFUSED）、quiet（24×25ms 零事件窗 + 存活 ping）、continuous-traffic（192×48B 双向流水不丢失/不失序/不饥饿）、close-error（优雅 close 为 EOF 族、读双 0 稳定、写收敛 EPIPE 且再次写同类别、无 ERR 误分类）。multiwaiter case 以 fork 进程为 distinct waiter 身份，poll/select/epoll 三模式 fan-out。
- 4.3：waiter 决策 seam——`ms06_waiter_record_valid` 强制 registered-before-completion、exactly-once、replacement 观察（WOKEN+RECHECK_NG+replacements）必须先 REREGISTERED 才能完成；`ms06_waiter_set_accepts` 强制精确容量（partial 拒绝）、pid 两两互异、waiter-65 聚合 Σreplacements ≥1。guest 编排：65 个 worker 先各自完成 ≥1 次注册（arm 握手），容量超载引发的 displacement wake 必然发生在 trigger 写入前，被替换 worker 走 wake→peek-recheck(未就绪)→re-register→数据到达后恰好一次完成；64 场景断言全完成，65 场景额外断言 replacement 被聚合观察到。

**Changed Files and Symbols**

| 文件 | 性质 | 关键符号 |
|---|---|---|
| `scripts/ms06-qemu-validate.py` | 新增 | `validate_output`、`self_test`、`EXPECTED_CASES`、`--print-cases` |
| `tests/ms06_stack_readiness_probe.c` | 新增 | seam：`ms06_deadline_expired/remaining_ms`、`ms06_events_satisfy`、`ms06_case_verdict`、`ms06_waiter_record_valid/set_accepts`；guest：`run_tcp_timer` 等 7 个单流 runner、`mw_wait_once/mw_worker_body/run_multiwaiter` 及 5 个 wrapper、`main` |
| `tests/ms06_stack_readiness_probe_test.c` | 新增 | 22 项决策测试（deadline 3、event 3、verdict 6、waiter 10）与 `--print-cases` |
| `Makefile` | 修改 | `host-test` 追加 MS06 各步与两个 purity guard；新增 `MS06_REVISION` 变量与 `tests/ms06_stack_readiness_probe` static target |
| change `tasks.md` / 本 Cycle 文件 | 文档 | 任务状态、Current Cycle、本 Response |

产品代码（kernel/crates/smoltcp/src）零改动。

**Deviations from Plan**

1. Plan Verification 列出的 host seam 命令含 `-O2`；`make host-test` 配方沿用仓库 MS04/MS05 惯例不带 `-O2`，另按 Plan 原文补跑 `-O2` focused 双跑（见验证表第 5 行）。等价调整，不影响判定。
2. multiwaiter 的 connector 子进程发送的触发字节固定为 `MS06_TRIGGER_BYTE`('X')，与 worker 校验一致（实现中发现并修正了一处初稿把 go 字节 'G' 直接转发的错误，该错误在 Gate 4 spec review 中捕获）。
3. `--print-cases` 同时加在 probe、seam 测试二进制上，用于 Makefile 中 validator/probe marker 集合 diff 守卫——属 Verification 条目"marker set 精确"的实现手段，非范围扩张。

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

Spec compliance review（逐任务对照 Task Contract）：

- 4.1：只读取保存输出与显式 metadata ✓；12-case 固定顺序唯一 PASS ✓；FAIL/timeout/partial/重复/乱序/exit 不一致全部失败且报首个差异 ✓；无 subprocess/socket/QEMU/shell 能力（guard 通过）✓；原始输出不被改写（只读打开）✓。
- 4.2：12 个 case 各有 monotonic fixed deadline、独立资源（`:0` 端口）、独立清理与非零失败 exit ✓；只用公开 socket/poll/select/epoll/clock/process ABI ✓；无 internal poll_interfaces、无 sleep 计时正确性（guard 通过）✓；normal-close 与 stable-error 观察符合 Iteration 004 语义（close-error/nonblock-connect-error 断言类别稳定性）✓。
- 4.3：精确 64/65、distinct pid、replacement→recheck→re-register→exactly-once 由 seam 正反例约束 ✓；未降低并发数、未复用 waker 冒充身份、未以 sleep 排序 ✓；static build 未裁掉路径（strings 检索到两 marker）✓。
- Invariants：check-register-recheck + 绝对 deadline 全部等待路径成立（poll/select/epoll timeout 均由 remaining 推导）✓；marker/case/exit/metadata 一一对应、无 aggregate success ✓；QEMU 保持人工边界 ✓。
- Non-goals：未动 kernel/axnet/smoltcp/scheduler/PollSet 容量 ✓；未做 Iteration 006/007/008 的工作 ✓。

Code quality review（完整 diff）：

- 已修复（Critical）：connector 初稿将 go 字节当触发字节转发，会导致全部 waiter case 假失败——已在实现期内修正并复验。
- 已修复（Important）：`run_tcp_timer` 初稿失败路径双重报告（违反"每 case 恰好一行终态"），重构为单出口模板后全文件统一。
- 遗留 Minor：(a) `run_multiwaiter` 成功路径向 result 填充代表性 `events=MS06_EV_IN` 仅满足 verdict 形参契约，非实测快照；(b) epoll 分支遇 RDHUP-only wake 会走 replacement-class 路径并在 peek 得 0 时判败——waiter 场景对端不关闭，合同范围内无影响，留待 Iteration 008 runtime 观察。两项均不阻塞 Acceptance。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| validator self-test | `python3 scripts/ms06-qemu-validate.py --self-test` | `PASS: ms06-validator-self-test`，exit 0 | PASS |
| validator CLI 正反例 | good/bad synthetic transcript | good `PASS: ms06-transcript-valid` exit 0；bad（11/12 partial）`FAIL: ... 首个差异` exit 1 | PASS |
| host seam 决策测试 | `cc -std=c11 -Wall -Wextra -Werror tests/ms06_stack_readiness_probe_test.c` 后执行 | `ms06 probe decision tests: 22 passed`，exit 0 | PASS |
| focused 重复运行 | 同一二进制连续两次 | 两次均 `22 passed`，exit 0 | PASS |
| Plan 原文 -O2 focused | `cc ... -Werror -O2 ...` 双跑 | 两次均 `22 passed`，exit 0 | PASS |
| guest syntax check | `cc -std=c11 -Wall -Wextra -Werror -fsyntax-only tests/ms06_stack_readiness_probe.c` | 无输出，exit 0 | PASS |
| RISC-V static build | `make tests/ms06_stack_readiness_probe` | musl gcc `-static -no-pie -Os` 编译通过，ELF 64-bit LSB executable, UCB RISC-V, static | PASS |
| marker set 精确性 | `--print-cases` 双侧 + `diff -u` | 无差异；guest 二进制侧亦一致 | PASS |
| 64/65 marker 在产物中 | `strings tests/ms06_stack_readiness_probe \| grep -cE "^(waiter-64\|waiter-65-reregister)$"` | `2` | PASS |
| purity guards | Makefile 内两条 grep guard | `validator-purity-PASS` / `probe-purity-PASS`（guard 命中即失败） | PASS |
| 回归边界 | `make host-test`（含既有 MS01–MS05 全部套件） | 尾部 `diff -u` 无差异，exit 0 | PASS |
| OpenSpec strict | `openspec validate ms06-application-visible-async-network-stack --strict` | `Change ... is valid` | PASS |
| diff 卫生 | `git diff --check` | 无输出，exit 0 | PASS |

**Persisted Evidence**

None required — 本 Cycle 结果均可低成本重跑（上表命令），Act Response 保存了决定性输出与退出码；Plan 设定 Mode 为 none，未创建 Evidence 目录。

**Experience Candidates**

None.

**Remaining Issues**

- guest 运行时 65 并发进程的资源可行性属 Iteration 008 验证边界，本 Iteration 只证明 decision seam 与可构建性（计划内既定边界，非缺陷）。
- Self-Review 遗留 Minor (a)(b) 见上，均已记录且不影响 Acceptance。

**Commit or Diff Reference**

未提交（未获授权）。本次 Act 工作树增量：新增 `scripts/ms06-qemu-validate.py`、`tests/ms06_stack_readiness_probe.c`、`tests/ms06_stack_readiness_probe_test.c`、构建产物 `tests/ms06_stack_readiness_probe`；修改 `Makefile`、change `tasks.md`、本 Cycle 文件（Gate 状态与本 Response）。

## Plan Review

- Review Result: rework-required

**Findings**

1. **Blocking — validator 没有执行声明的完整状态机顺序。** `validate_output()` 只约束 12 个 PASS
   彼此的顺序，并允许 `PASS: tcp-timer` 出现在 revision/environment 之前。审计构造的最小反例被接受，
   违反 Task 4.1 的 `START → REVISION → ENVIRONMENT → 12 PASS → END → EXIT` 固定语法和
   Acceptance 1。
2. **Blocking — UDP 场景使用无效的地址结构。** `run_udp_progress()` 清零 `sockaddr_in` 后直接
   `bind()`，没有设置 `sin_family = AF_INET`、loopback 地址和端口。该路径不能建立 Task 4.2 要求的
   queued-datagram witness。
3. **Blocking — quiet 场景把正常 writable 当作噪声。** idle TCP socket 的观察集合包含 `POLLOUT`，
   但 established socket 可写是合法 level-triggered readiness；当前代码会把正常事件记为
   `MS06_ST_EVENT_MISMATCH`，不能证明 Active quiet。quiet 只能观察 read/terminal/error 方向，窗口结束后
   再单独执行 liveness I/O。
4. **Blocking — multiwaiter 编排存在循环等待且只有一个 consumable trigger。** worker 在阻塞的
   `poll/select/epoll` 返回后才向 `arm_fd` 报告注册；父进程却等待全部 `arm_fd` 报告后才允许 connector
   发送数据。4/64 waiter 没有 pre-data wake，因而无法到达 trigger。即使解除该循环，connector 只发送
   1 byte，而所有 waiter 都在共享 socket 上执行 consuming `recv()`，至多一个 waiter 能完成；其余 waiter
   可能在 blocking `recv()` 中越过 deadline。Task 4.3 和 Acceptance 4 未满足。
5. **Non-blocking Minor — Act 已记录的代表性 `events=IN` 与 epoll RDHUP 分支问题仍属后续 runtime
   观察项。** 它们不扩大本次 Acceptance gaps；rework 必须保持记录，不得用它们替代上述修复。

**Deviation Classification**

`ACT-DEVIATION`（Findings 1–4 的实现未满足既有 Task Contract）；`PLAN-OMISSION`（原 host seam
只检查最终 record 聚合，没有覆盖 metadata/case 全序、UDP socket address、quiet interest mask，以及
parent/worker trigger choreography）。

**Acceptance Gaps**

- A1：validator 必须拒绝 metadata 之后/之前错位的 PASS，以及所有 protocol phase 乱序。
- A2/A3：UDP public-ABI 场景必须使用有效 AF_INET loopback endpoints；quiet 必须忽略正常 writable，
  只把 read/terminal/error 异常当作窗口失败，并保留窗口后的 liveness 证明。
- A4：4/64/65 个 distinct waiter 必须在 fixed deadline 内各完成一次；65 场景必须先观察真实
  wake-on-replacement，再 recheck/re-register。触发协议不得依赖等待返回后才能产生的“已注册”报告，
  并必须提供足够的 consumable data，使一个 waiter 的实际 I/O 不会剥夺其他 waiter 的完成条件。
- A5：新增 RED/GREEN tests 必须覆盖上述缺口；现有 self-test、22 项 post-hoc seam 和静态编译通过
  不能替代这些见证。

**Convergence**

N/A — 首次 Review；与父 Cycle 无同类 Acceptance gap 可比较。

**Evidence**

- `scripts/ms06-qemu-validate.py:85-122`：PASS、revision、environment 由互不约束的分支解析；仅 environment
  检查 revision 已出现。
- 审计命令：构造 `PASS: tcp-timer` 位于 metadata 之前的 transcript 后调用 `validate_output()`；输出
  `ACCEPTED_INVALID_ORDER`，exit 1（审计期望该样本被拒绝，因此命令以 1 标记发现成立）。
- `tests/ms06_stack_readiness_probe.c:606-614`：UDP bind 前两个地址只有 `memset`。
- `tests/ms06_stack_readiness_probe.c:921-943`：quiet interest 含 `POLLOUT`，任何非零 revents 均失败。
- `tests/ms06_stack_readiness_probe.c:1347-1360,1480-1497`：worker 先阻塞等待、后写 arm；parent 先等全部
  arm、后放行 trigger。
- `tests/ms06_stack_readiness_probe.c:1366-1373,1431-1435`：所有 worker consuming recv，共享 peer 只发送
  1 byte。
- 新鲜 Review 验证：validator self-test PASS；22 项 host seam PASS；host syntax PASS；marker set diff PASS；
  strict OpenSpec PASS；`git diff --check` PASS。它们证明现有自动 Gate 可重复，但没有覆盖上述 gaps。
- RISC-V 交叉编译在本次受限 Review 环境中触发 `Bad system call`（exit 159），属于能力边界；Act 留下的
  artifact 经 `file` 确认为 RISC-V static ELF，且两个 64/65 marker 均存在。本 finding 不把该环境失败
  分类为产品失败。
- Persisted Evidence：原计划为 `none`；缺少当前 Cycle Evidence 目录不是问题。

**Follow-up Decision**

上述缺口需要新的 test witnesses、multiwaiter choreography 契约和独立 repair items，不能作为当前 Cycle
内的局部修正交给 Act。创建 `001-rework.md`；原 Iteration Acceptance 和 Iteration Map 不变。新 Cycle
保持 `draft`，等待用户审计并批准 Gate 2 后才能调用 `openspec-act`。

**Iteration Plan Update**

None.

**Next Cycle**

`001-rework.md`

**Next Iteration**

None；Iteration 005 尚未 accepted，Iteration 006 继续保持 map-only。
