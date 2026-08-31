# Iteration 007 / Cycle 003: Establish a Trustworthy Guest Runtime Witness

## Plan Context

- Status: draft
- Iteration: 007-single-hart-qemu-qualification
- Cycle: 003-replan
- Cycle Type: replan
- Parent cycle: `002-replan.md`

**Iteration Scope**

- Change tasks: 4.2
- Depends on: Iteration 006 accepted；Cycle 002的owner与初始link自动实现保留
- Stable baseline: change-local probe本身无未解释user fault，nonblocking UDP遵守共享absolute deadline，
  single-hart QEMU完成MS07 reset/link/old-new socket资格及受影响回归。
- Verification boundary: exact ELF地址证据和host/model先证明probe、deadline与errno决策；自动Gate全绿后，
  用户手工QEMU raw serial/pcap证明实际syscall、packet和MS07行为。
- Diagnostic boundary: user PC/fault VA/SP/RA、ELF program headers与反汇编、socket/connect/send/recv errno、
  UDP egress、owner/link snapshot、reset/HMP和MS01/MS04/MS05/MS06。
- Deferred tasks: None

**Cycle Scope**

- Trigger: replan-required
- Acceptance gaps: A3无可信send errno或pre-reset exchange；A4/A5未到达；A6未重跑；probe发生未定位
  `0x1a000 READ | USER`页故障。
- Repair items: None
- Inherited scope: Task 4.2、R6、R8、D6、D8；Cycle 002已实现的64/64 owner消费者、初始link commit、
  V4 ABI和六case协议。
- Excluded scope: 预判UDP产品缺陷、通用ELF loader修复、自动驱动QEMU/HMP、SMP、PCI/DWMAC、真板、
  性能、V5 ABI、身份型证据或全局文档维护。

**Objective**

先使guest probe成为可信事实源：任何user fault都能由保存的user PC定位到exact ELF，任何peer失败都能
由stage+errno定位；合法nonblocking `EAGAIN/EWOULDBLOCK`在既有phase deadline内有界重试。随后重新执行
完整single-hart QEMU资格。若证据指向未授权的UDP或loader产品修复，本Cycle保留现场并停止。

**Background**

Cycle 002关闭了错误owner fixture和启动link unknown，但手工runtime在首个peer exchange前失败。旧载荷
只输出`precondition`；带send DBG的新载荷又发生`VA=0x1a000`读故障。Act没有取得fault PC或send errno，
却据“约0.9 ms退出 + pcap无UDP”把问题归为UDP产品数据面。该归因不能区分合法`EAGAIN`和其他错误。
同时Cycle 002禁止send retry，与nonblocking socket及absolute deadline契约冲突。

**Current Baseline**

- 工作树含未提交的前序Cycle实现，必须原样保留；本Cycle不回滚P1/P2。
- `tests/ms07_recovery_probe.c::open_peer_socket()`使用`SOCK_DGRAM | O_NONBLOCK`，socket/connect失败已
  分阶段；API命名应改为`SOCK_NONBLOCK`。
- `peer_exchange()`等待`POLLOUT`后只send一次；`sent != n`立即打印send errno并失败，没有处理
  `EAGAIN/EWOULDBLOCK` readiness race。
- `UdpSocket::send()`通过`GeneralOptions::send_poller()`；nonblocking `poll_io`允许第一次
  `try_send_once()`的`WouldBlock`直接返回。`sys_sendto()`不改写该错误。
- `kernel/src/task/user.rs`处理不可恢复PageFault时只打印进程、fault VA和access flags；`UserContext`
  已持有`sepc`与RISC-V通用寄存器，可读取PC、SP、RA而无需改变trap ABI。
- 当前payload为static little-endian RISC-V `ET_EXEC`，entry `0x10252`；第二LOAD的
  `vaddr=0x18fd8, filesz=0x2a0, memsz=0x918`，页对齐有效区间到`0x1a000`为止。`run_probe`链接VMA
  `0x10aba`，栈帧`0x520`。fault VA不是PC。
- Review独立重跑C decision test和validator self-test均exit 0。axnet focused命令在当前host默认PIE
  与percpu relocation冲突处exit 101；执行时必须使用项目既有非PIE host测试配置。
- Cycle 002的一次性serial/pcap当前不可读，因此所有runtime推断保持未确认。

**Current-State Evidence**

1. `tests/ms07_recovery_probe.c::peer_exchange`：`wait_fd(POLLOUT)`后单次
   `send(..., MSG_DONTWAIT)`；没有errno分支或重新poll。
2. `crates/axnet/src/udp.rs::{send,try_send_once,udp_readiness}`：bound/open且TX buffer有空间才OUT；
   terminal、not-open、full和smoltcp send error映射到不同`AxError`。
3. `crates/axnet/src/general.rs::send_poller`：nonblocking参数传给`poll_io`，因此WouldBlock是正常可观察
   结果，不足以单独证明产品故障。
4. `kernel/src/syscall/net/io.rs::send_impl`：传播socket send错误；`socket.rs::sys_socket`按
   `O_NONBLOCK`数值掩码设置general nonblocking。
5. `kernel/src/task/user.rs::new_user_task`：page-fault分支在`uctx.run()`返回后仍能访问保存的context，
   适合在现有fault日志中加入user PC/SP/RA。
6. `readelf`与`llvm-objdump`：static ELF无需relocation slide；只有取得runtime PC后才能执行
   `PC -> linked VMA -> symbol/file offset/instruction`对齐。

**Relevant Code**

- `kernel/src/task/user.rs`：用户fault的首个稳定记录点；只增加可观察上下文，不改变fault处理结果。
- `tests/ms07_recovery_probe.c`：socket flag、阶段诊断、deadline wait/send/recv和完整MS07流程。
- `tests/ms07_recovery_probe_test.c`：fake-clock deadline与nonblocking决策测试。
- `Makefile`：probe source guard、host test和RISC-V payload构建入口。
- `crates/axnet/src/{udp.rs,general.rs,readiness.rs}`：只读错误与writable语义；没有新runtime证据前不改。
- `kernel/src/mm/loader.rs`：只读LOAD映射依据；本Cycle不预授权修改。
- `scripts/ms07-qemu-validate.py`、`tests/ms07-recovery-host-harness.rs`：既有纯输出和schema Gate。

**Critical Path**

```text
exact RISC-V ELF
  -> kernel ELF PT_LOAD mappings
  -> user execution
  -> on fault: {user_pc, fault_va, access, sp, ra}
  -> PC-to-ELF symbol/instruction ledger

probe peer phase
  -> socket(SOCK_DGRAM | SOCK_NONBLOCK)
  -> connect
  -> wait_fd(POLLOUT, phase_deadline)
  -> send(MSG_DONTWAIT)
     -> EAGAIN/EWOULDBLOCK: re-enter wait_fd with same deadline
     -> other errno: stage+errno, stop
     -> success: wait/recv under same deadline
  -> MS07 reset/link cases -> validator -> regressions
```

**Implementation Guidance**

1. 先增加fault PC上下文和host可验证的send decision seam，不先改axnet/kernel UDP数据面。
2. 将socket API标志改为`SOCK_NONBLOCK`，移除不再需要的`<fcntl.h>`，同步Makefile source guard。
3. send只对`EAGAIN/EWOULDBLOCK`重新poll；所有尝试共享已有deadline。EINTR可按现有规则重试，但每次
   wait返回后和实际I/O前仍要重采样时钟。成功datagram不得短写；其他errno原样报告并停止。
4. 自动Gate后先让用户运行最小startup/peer诊断边界。若发生fault，先保存现场并做PC地址账本；若得到
   非EAGAIN errno，停止返回Plan。只有payload稳定且peer exchange成功才继续完整reset/HMP和回归。

**Behavioral Change**

- 不可恢复user page fault日志增加保存的user PC、SP和RA，fault终结语义不变。
- probe使用socket API的`SOCK_NONBLOCK`名称；send readiness race不再被误报为立即产品失败。
- `EAGAIN/EWOULDBLOCK`只消耗现有absolute deadline，不产生sleep、自旋或新budget。
- UDP/loader产品实现默认不变；只有本Cycle stop condition触发后的新Plan才能授权修改。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| T4.2-P5 | R8/probe user fault | `kernel/src/task/user.rs`；ELF检查 | 只打印fault VA | 记录user PC/SP/RA并建立exact ELF地址账本 |
| T4.2-P6 | R8/nonblocking send | probe、probe test、Makefile | 单次send即失败 | `SOCK_NONBLOCK`与同deadline EAGAIN重试 |
| T4.2-P7 | R8/runtime qualification | probe/validator/QEMU/regressions | runtime停在precondition/fault | 分层诊断后完成六case与四组回归 |

**Task Contracts**

### T4.2-P5: Make a guest user fault instruction-addressable

- Requirement/Scenario: R8 guest probe用户态页故障；QEMU runtime事实源可信。
- Depends on: None。
- Targets: `kernel/src/task/user.rs::new_user_task` page-fault branch；必要的host source/model witness；
  payload `readelf/llvm-objdump/addr2line`命令。
- Current behavior: fault日志只有fault VA/access；Act把VA与源码阶段关联，缺少faulting instruction。
- Required behavior: 不可恢复user page fault在raise SIGSEGV前记录task、user PC、fault VA、access、SP、
  RA。字段使用稳定label，PC和VA明确区分；不改变handled fault、signal或退出码。
- Required changes: 从现有`UserContext`读取保存值；增加host witness固定日志字段来源；对exact payload保存
  ELF header/LOAD范围、PC所属symbol、PC前后raw bytes与反汇编。static ET_EXEC的slide为0，若实际artifact
  不是该类型则重新建立ledger，不能套用0 slide。
- Preserve: user fault处理顺序、SIGSEGV、page population、其他架构可编译；不泄漏kernel地址。
- Forbidden: 把fault VA当PC；只跑addr2line不看program headers；为制造证据生成hash/run-id；修改loader
  映射或payload布局碰运气；没有runtime PC就宣布根因。
- Test witness: source/model test先证明旧日志缺PC字段；修改后host检查日志包含PC/VA/SP/RA且取自同一
  `uctx`。静态地址账本用当前payload建立，但runtime结论等待用户serial。
- GREEN condition: host/build Gate通过；手工fault若重现，raw serial含完整context且PC落到exact ELF可解码
  指令；若不再重现，startup/peer路径连续执行到明确stage结果。
- Verification: kernel host/build相关检查；`readelf -h -l -S -s`；`llvm-objdump -d`；手工raw serial。
- Stop when: runtime bytes/artifact不匹配、PC不在可解释映射、或首个未解释边要求通用loader修复；保存
  Evidence并返回Plan。

### T4.2-P6: Handle nonblocking UDP send under one absolute deadline

- Requirement/Scenario: R8 nonblocking peer send背压与分阶段errno。
- Depends on: P5自动Gate；runtime最终判定依赖P5可信payload。
- Targets: `tests/ms07_recovery_probe.c::{open_peer_socket,peer_exchange,wait_fd}`、
  `tests/ms07_recovery_probe_test.c`、Makefile source guards和RISC-V payload。
- Current behavior: socket type写`O_NONBLOCK`；poll后send一次，任何错误都失败。
- Required behavior: `SOCK_DGRAM | SOCK_NONBLOCK`创建；send的EINTR重试和
  `EAGAIN/EWOULDBLOCK -> wait_fd(POLLOUT) -> send`均使用同一phase deadline。每次wait后、I/O前检查
  deadline；成功必须等于完整datagram长度。其他errno立即输出`stage=send phase errno sent want`并失败。
- Required changes: 提取host可驱动的pure send-step decision或等价seam，覆盖success、EINTR、EAGAIN后
  success、repeated EAGAIN到deadline、其他errno和短写；source guard改查`SOCK_NONBLOCK`且无fcntl。
- Preserve: 10.0.2.2:15572、phase/seq、无host pin/hostfwd 15572、共享deadline、recv规则和marker schema。
- Forbidden: sleep retry、无界busy loop、新deadline、吞掉非EAGAIN errno、在witness前改
  `axnet::udp`/`poll_io`/syscall映射。
- Test witness: 当前single-send seam对EAGAIN为RED；新增fake-clock序列。C test、host compile和source
  guard形成GREEN，真实errno/packet由手工QEMU证明。
- GREEN condition: 自动测试证明EAGAIN有界重试与deadline fail-closed；runtime peer exchange成功，或
  以非EAGAIN exact errno触发stop condition。
- Verification: C `-Wall -Wextra -Werror` test、`make host-test`、RISC-V static payload、serial/pcap。
- Stop when: runtime send返回非EAGAIN错误、反复EAGAIN到deadline、或pcap与成功返回矛盾；保存Evidence
  并返回Plan，不扩大产品范围。

### T4.2-P7: Re-run single-hart MS07 and affected regressions

- Requirement/Scenario: R6/R8初始link、reset、link flap、old/new socket、兼容性回归。
- Depends on: P5/P6自动GREEN，且最小手工startup/peer witness无fault并成功exchange。
- Targets: 既有peer/QEMU/HMP/probe/validator命令与MS01/MS04/MS05/MS06；不再修改产品语义。
- Current behavior: Cycle 002只到pre-reset失败；没有可信六case或回归结果。
- Required behavior: 使用single hart、QEMU 7.0.0、VirtIO-MMIO user-net和本轮fresh payload完成六case；
  validator exit 0。随后记录MS01 14/14、MS04四mode、MS05六mode、MS06 12-case的终态与exit。
- Preserve: R44只由用户手工驱动QEMU/HMP；LOG=warn最终资格；V4、case顺序、owner/epoch/terminal判据。
- Forbidden: 用wget/TCP、host test或pcap替代peer echo；用INFO诊断run当最终warn资格；缺case/exit仍PASS。
- Test witness: 自动Gate是进入runtime的前置；raw serial/pcap和validator是runtime witness。
- GREEN condition: A1–A6全部成立，无panic、trap、fatal owner drift或永久Pending。
- Verification: 两套axnet非PIE串行全量、driver suites、host-test、payload/kernel build、format/diff/
  OpenSpec strict；然后手工MS07与回归。
- Stop when: 任一自动或runtime Gate失败，或用户尚未提供结果；写Blocker Handoff并保存最小现场。

**Invariants**

- P1/P2已实现语义保持：健康idle为64/64/0；初始link通过Service路径提交；V4不变。
- fault PC、fault VA、symbol VMA和file offset必须标注地址空间；static ET_EXEC也不能省略program-header核对。
- nonblocking重试只处理EINTR和EAGAIN/EWOULDBLOCK，所有循环受同一absolute deadline约束。
- ISR仍只ack/publish；唯一queue owner和socket epoch/terminal-before-wake不变。
- 不引入revision/hash/run-id/peer pin/manifest等身份型证据。

**Non-goals**

- 不在没有PC/errno证据时修复loader、axnet UDP、smoltcp或syscall错误映射。
- 不增加通用GDB runner、自动QEMU编排、SMP或真板证明。
- 不改变MS07 ABI、owner分类、peer协议或deadline数值。
- 不提交Git、不更新SNAPSHOT、不创建Runbook/Incident。

**Requirements Traceability Matrix**

| Requirement | Scenario | Design | Task | Code Surface | Test Witness | Simplification | Status |
|---|---|---|---|---|---|---|---|
| R6 | 初始link与HMP down/up | D6 | P7 | owner/Service/QEMU | retained owner tests + runtime V4 | None | Covered |
| R8 | guest probe user fault | D8 | P5 | user fault log + ELF | PC/VA source witness + address ledger | None | Covered |
| R8 | nonblocking send背压 | D8 | P6 | probe wait/send | fake-clock EAGAIN matrix + runtime errno | None | Covered |
| R8 | reset/socket/回归 | D8 | P7 | probe/validator/QEMU | raw serial/pcap + regressions | None | Covered |

**Acceptance**

- A1：Cycle 002保留的initial-link与owner自动Gate继续通过；无driver/ABI回退。（R6/R8/P7）
- A2：任何probe user fault都有user PC、fault VA、SP、RA和exact ELF指令账本；最终资格run无未解释
  fault。（R8/D8/P5）
- A3：socket使用`SOCK_NONBLOCK`；EAGAIN在同一deadline内有界重试；其他失败保留stage+errno；三个peer
  phase均双向成功。（R8/D8/P6）
- A4：reset后QueueEpoch/SocketEpoch各推进一次并恢复64/64/0；HMP down/up不推进QueueEpoch，按规则推进
  LinkGeneration/SocketEpoch。（R6/R8/P7）
- A5：旧socket稳定返回`ECONNRESET`/`ENOTCONN`，新socket成功；validator exit 0，无panic/trap/fatal/
  permanent Pending。（R7/R8/P7）
- A6：MS01 14/14、MS04四mode、MS05六mode、MS06 12-case明确PASS与exit。（R8/P7）

**Verification**

1. P5：fault-log source/model witness；kernel RISC-V build；exact ELF `readelf`/`llvm-objdump`账本。
2. P6：C fake-clock/decision test、source guard、validator self-test、RISC-V static payload build。
3. 自动回归：`make host-test`；项目非PIE配置下axnet ordinary/qemu-diagnostics串行全量；VirtIO/driver
   focused/full；`make ARCH=riscv64 build`；format、diff check、`openspec validate ... --strict`。
4. 用户手工先跑startup/peer最小边界；无fault且peer成功后再跑完整MS07、validator和四组回归。
5. 任何FAIL/BLOCKED也按本Cycle Evidence契约保存；不得等待PASS才保留一次性现场。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | probe、UDP/poll/syscall、user fault、ELF LOAD与测试入口已核对 |
| Design | PASS | fault只增观测；EAGAIN共享deadline；产品修复由stop condition隔离 |
| Iteration Plan | PASS | 仍为Task 4.2/Iteration 007，先可信payload再runtime，依赖有序 |
| Cycle Scope | PASS | 修订诊断/验证契约，不预授权UDP或loader修复 |
| Task Contracts | PASS | P5–P7含targets、行为、witness、GREEN与stop condition |
| Traceability | PASS | R6/R8到D6/D8、任务、代码与证据均Covered |
| Verification | PASS | 静态地址、host决策、build、QEMU packet与回归分层 |
| Evidence | PASS | 一次性手工FAIL/PASS现场使用4文件，满足白名单与预算 |
| Authorization | BLOCKED | 用户尚未明确批准Cycle 003执行；保持draft |

**Persisted Evidence**

- Mode: required
- Path: `openspec/changes/ms07-qemu-single-hart-recovery-semantics/evidence/007-single-hart-qemu-qualification/003-replan/`
- `README.md`：支持A2–A6；记录exact artifact的现有Build ID（若有）、build/QEMU命令、环境、exit、
  fault或首个失败层。Act Response不足以保存一次性手工session与地址换算；缺失时不能决定payload、UDP
  或loader责任。通过条件是每个结论可追到下列raw文件。
- `qemu-serial.log`：支持A2–A5；保存本轮决定性raw serial，包含fault context或完整MS07 marker与exit。
  手工session不能低成本重跑，摘要会丢失trap/marker顺序；缺失时不能Review runtime。
- `usernet.pcap`：支持A3/A5；仅在UDP仍失败或需要证明peer双向帧时保存本轮filter-dump。packet层结构
  不能由文字摘要替代；缺失时不能在syscall成功与线上交付间作决定。若serial已完整PASS且无需packet
  判定，可在README写`not needed`而不创建占位文件。
- `regressions.txt`：支持A6；仅在进入完整资格后保存四组终态与exit；缺失时不能接受兼容性Gate。
- PASS或FAIL/BLOCKED均按实际分支保存需要的文件，不创建无内容占位。Budget：本Cycle最多5个文件
  （含README），整个change最多20个Evidence文件；文本文件最多500行且不超过256 KiB。

**Risks and Notes**

- fault VA位于当前数据LOAD首个越界页是事实，不代表faulting instruction或根因；必须等待PC。
- `SOCK_NONBLOCK`与`O_NONBLOCK`通常数值相同，本轮改名是API契约修正，不把它宣传为runtime根因。
- 若EAGAIN重试后成功，Cycle 002的“UDP产品缺陷”被否证；若得到其他errno，只记录新证据并返回Plan。
- 轻量模式SKIPPED：跨user trap观测、guest probe、QEMU和异步socket deadline，且涉及低层fault与网络
  runtime边界。

## Act Response

- Status: pending

**Implemented**

None yet.

**Changed Files and Symbols**

None yet.

**Deviations from Plan**

None.

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: BLOCKED
- Full diff reviewed: BLOCKED
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 0

**Verification Evidence**

None yet.

**Persisted Evidence**

None yet.

**Experience Candidates**

None.

**Remaining Issues**

None yet.

**Commit or Diff Reference**

None.

## Plan Review

- Review Result: pending

**Findings**

None yet.

**Deviation Classification**

None.

**Acceptance Gaps**

None yet.

**Convergence**

Not reviewed.

**Evidence**

None yet.

**Follow-up Decision**

None.

**Iteration Plan Update**

None.

**Next Cycle**

None.

**Next Iteration**

None.
