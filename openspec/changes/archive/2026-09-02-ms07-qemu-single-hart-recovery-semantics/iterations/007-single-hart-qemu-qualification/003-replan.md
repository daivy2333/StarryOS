# Iteration 007 / Cycle 003: Establish a Trustworthy Guest Runtime Witness

## Plan Context

- Status: ready
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
| Authorization | PASS | 用户已明确批准 Cycle 003 执行（「更改gate状态，开始实施」）；Plan Context 改为 ready，Act 开始执行 Task 4.2 |

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

- Status: blocked

**Implemented**

本 Cycle 完成 P5 与 P6 的自动可实施部分，P7 的自动前置 Gate 全部通过；随后用户按 Plan 在
single-hart QEMU LOG=info 镜像上手工运行插桩 probe，采集到决定性 runtime 证据（下见 Blocker
Handoff），把 P7 首个 case 的真正根因从"产品网络恢复缺陷"更正为"内核 sys_poll 空 nfds 语义
缺陷"，并推翻了此前 R60 runbook 与 Cycle 001/002 的 `Service.link_state=None`/OwnerSummary
不守恒归因。

1. **P5（R8/A2）：guest user fault 指令可定位**。`kernel/src/task/user.rs` 不可恢复
   PageFault 分支由「只打印 fault VA」扩展为记录 `pc=`, `va=`, `sp=`, `ra=` 四个稳定 label：
   - 新增 `user_fault_pc_sp_ra(&uctx)`，从保存的 `UserContext` 读 `(pc, sp, ra)`，按架构分发：
     riscv64/loongarch64 读 `uctx.regs.ra`（x1/r1），aarch64 读 `uctx.x[30]`（LR），x86_64 以
     `uctx.rsp` 占位并注明「非真 RA、不用于 RISC-V runtime 对齐」。
   - `pc=` 来自 `uctx.ip()`（= `sepc`，即 faulting instruction），`va=` 来自 `addr`（stval），
     PC 与 VA 明确分开；`flags` 保留访问位；fault 处理顺序、SIGSEGV、退出码与 handled-fault
     语义不变；不改 loader 映射、不新增 hash/run-id/身份机制。
   - ELF 静态地址账本用 `readelf`/`llvm-objdump` 建立（详见 Persisted Evidence）：新 payload 为
     static ET_EXEC，entry `0x10252`；LOAD[0] R E @`0x10000`(filesz `0x723c`)，
     LOAD[1] RW @`0x18fd8`(filesz `0x2a0`/memsz `0x918`→end `0x198f0`)；该数据 LOAD 页对齐映射的
     首个越界 READ 页即 `0x1a000`，与 P5 目标一致（fault VA 不是 faulting PC，结论须等 runtime PC）。
   - 符号账本（新 binary）：`open_peer_socket@0x1048a`、`wait_fd@0x1069c`、
     `peer_exchange@0x10816`、`run_probe@0x10b0e`(栈帧 `0x520`)。这些地址随 -Os 重排，runtime
     PC 必须对照当前 binary 而非旧值。
2. **P6（R8/A3）：nonblocking UDP send 在同一 absolute deadline 下有界重试**。
   - 新增纯决策 seam `ms07_send_step(sent, err, want, now, deadline, &again)`：满长 datagram → 1；
     EINTR / EAGAIN / EWOULDBLOCK → 0（`again` 由 deadline 是否仍开放决定，EAGAIN 在 deadline 后
     `again=0` fail-closed）；其他 errno 或短写 → -1。`peer_exchange` 的 send 改为 `wait_fd(POLLOUT)
     → ms07_io_allowed 重采样 → send → ms07_send_step` 循环，EAGAIN 复用**同一 deadline** 重新
     poll，不 sleep、不自旋、不新增 deadline。
   - `open_peer_socket` 由 `SOCK_DGRAM | O_NONBLOCK` 改为 `SOCK_DGRAM | SOCK_NONBLOCK`
     （socket API 契约名），移除不再使用的 `#include <fcntl.h>`。
   - Makefile source guard 同步：要求 `SOCK_NONBLOCK`，并新增对 `SOCK_DGRAM | O_NONBLOCK`、
     `fcntl(fd, F_SETFL`、`#include <fcntl.h>` 的禁止检查。
   - RISC-V static payload 已用 `$(BENCH_CC)`（riscv64-linux-musl-gcc）重建，仍为 EXEC，entry 不变。
3. **P7 runtime 诊断插桩（依据 R44/R60 分层诊断 + user 采集的 LOG=info 镜像）**：给 probe
   的静默失败路径补 `DBG:`，把 `pre_reset_traffic` 的 precondition 从单一 reason 拆为
   `open_peer_socket` / `wait_for_pre_reset` / `peer_exchange` 三个独立 fail reason；给
   `wait_for_pre_reset` 循环、`wait_fd` 各静默分支、`peer_exchange` 的 send/recv/gate/issue
   分支补观测打印。这些只增观测、不改行为、不加身份字段，validator 仍只消费
   `MS07_`/`PASS`/`FAIL`，`DBG:` 是既有串口噪声。

**Changed Files and Symbols**

- `kernel/src/task/user.rs`：`user_fault_pc_sp_ra`（新增 cfg 分发）；PageFault 分支日志扩为
  pc/va/sp/ra。
- `tests/ms07-recovery-host-harness.rs`：新增 `unrecoverable_user_fault_log_is_instruction_addressable`
  源见证（标签 + 同一 uctx + 助手存在）。
- `tests/ms07_recovery_probe.c`：`ms07_send_step`（新增纯 seam）；`peer_exchange` send 循环；
  `open_peer_socket` SOCK_NONBLOCK；删除 `fcntl.h`。
- `tests/ms07_recovery_probe_test.c`：新增 P6 fake-clock EAGAIN/EINTR/短写/other-errno/满长矩阵。
- `Makefile`：P6 source guard 改查 SOCK_NONBLOCK 与禁 fcntl。
- `tests/ms07_recovery_probe.c`（P7 runtime 诊断插桩）：precondition 拆分 + `wait_for_pre_reset`
  循环/`wait_fd` 静默分支/`peer_exchange` send/recv/gate/issue 的 `DBG:` 观测。
- `tests/ms07_recovery_probe`（payload，P6 后两次重建：RISC-V 插桩版）。
- `iterations/007-single-hart-qemu-qualification/003-replan.md`：Plan Context status 与 Gate 2
  Authorization 按用户授权更新。

**Deviations from Plan**

- 无实质偏差。非实质记录：
  - x86_64 无专用 RA 寄存器，`ra=` 以 `uctx.rsp` 占位并在代码注释与 Self-Review 注明不用于
    RISC-V runtime 对齐；本 Cycle 目标平台为 RISC-V。
  - `ms07_send_step` 用 `size_t want` 表达 datagram 长，测试用 `(ssize_t)4` 直接验证，等价于计划
    的「成功必须等于完整 datagram 长度」；短写恒为 -1。

**Blocker Handoff**

P7（`T4.2-P7`）被一个**超出本 Cycle 契约的产品级缺陷**阻断，必须返回 Plan 决定修复路径。
用户已在 single-hart QEMU `LOG=info` 镜像上运行插桩 probe（`/tmp/ms07-qemu-info.log`），
决定性 runtime 证据与源码核对如下：

1. **网络数据面健康**：`axnet_ng` info 日志 `eth0 ip: 10.0.2.15/24`、`mac: 52-54-00-12-34-56`，
   `Device: eth0` 初始化成功。
2. **UDP socket 创建成功**：`axnet_ng::udp` `UDP socket #0: bound on *:49152`——probe 的
   `open_peer_socket()` 的 socket+bind+connect 已成功，不是 runbook 此前猜测的 connect 失败。
3. **失败点**：`wait_for_pre_reset` 第 2 次采样 `wait_until_sample()` → errno=14：
   ```
   DBG: read_v4=0 errno=0 lifecycle=2 current_valid=1 q=0 s=0 l=1 link=1 avail=64 dev=64 quar=0
   DBG: wait_pre_reset iter=0 stable=0 link=1 avail=64 dev=64 quar=0
   DBG: wait_pre_reset iter=1 wait_until_sample_fail errno=14
   FAIL: pre_reset_traffic reason=wait_for_pre_reset
   ```
4. **根因（源码核对）**：`wait_until_sample()` 调用 `poll(NULL, 0, remaining)`（probe 被 Makefile
   source guard 禁止 `usleep/nanosleep/sleep(`，故用 poll 空数组作有界睡眠）。内核
   `kernel/src/syscall/io_mpx/poll.rs::sys_poll` 无条件 `fds.get_as_mut_slice(nfds)`，即使
   `nfds=0` 也走 `check_region(NULL, Layout::array::<pollfd>(0), R|W)` → NULL 地址 → `Err(BadAddress)`
   → `AxError::code() = EFAULT = 14`。**`poll(NULL, 0, timeout)`（POSIX 合法睡眠）未实现**。

这不是 probe 或 P6 契约问题，是**内核 syscall 层产品缺陷**（`sys_poll`/`sys_ppoll` 对
`nfds==0` 且 `fds==NULL` 的错误处理），阻塞所有在 QEMU 上依赖空 poll 睡眠的确定性路径。
该缺陷同时推翻了 R60 runbook 与 Cycle 001/002 的归因（`link_state=None`、OwnerSummary
不守恒、`open_peer_socket` connect 失败）——那些是前置错误推论；最新串口证明 link 健康、
owner 全 0、socket 已建、卡点仅是 poll 空数组。

修复需触碰 `kernel/src/syscall/io_mpx/poll.rs`（`nfds==0` 时若 `fds` 为空应纯睡眠返回 0）
与 `kernel/src/mm/access.rs`（空 slice 对 NULL 的语义），属产品 syscall 修复，超出 Cycle 003
Non-goals（"不预授权 syscall/loader 修复"），故 `blocked` 并返回 Plan。

**Blocker Resolution**

- 2026-08-31：用户按 Plan 手工运行插桩 probe 于 `LOG=info` 镜像，证据 `/tmp/ms07-qemu-info.log`；
  peer-host.log 为空（probe 未进 peer 阶段），HTTP/pcap 未到。据此判定首个 case 卡在
  `wait_for_pre_reset` 的 poll 空数组，非连接/网络/owner/link 问题。
- 恢复条件：Plan Review 决定修复 `sys_poll`/`sys_ppoll` 的空 nfds 语义（`nfds==0` 且 `fds==NULL`
  时按 timeout 纯睡眠返回 0，对齐 Linux/POSIX），并考虑 `get_as_mut_slice(0)` 对 NULL 的
  `check_region` 收敛；修复后本 Cycle（或新 Cycle）在冻结 warn 镜像上重跑 P7 六 case 与回归。
- 本 Cycle 保留的插桩 probe（`tests/ms07_recovery_probe`）可复用为诊断工具，不增加后续契约负担。

**Self-Review**

- Plan compliance: BLOCKED（P7 被产品 `sys_poll` 空 nfds 缺陷阻断，返回 Plan 决定修复）
- Full diff reviewed: PASS
- Critical findings unresolved: 1（内核 `sys_poll` 不支持 `poll(NULL,0,t)` → EFAULT，阻塞 P7）
- Important findings unresolved: 0
- Minor findings unresolved: 1（x86_64 `ra=` 为 rsp 占位，非真返回值——仅 x86_64 路径，注明不用于
  RISC-V runtime 对齐）

P5/P6 逐 Acceptance A2/A3 核对通过。P7 的自动前置 Gate 通过，但 runtime 首 case 被 `sys_poll`
空数组缺陷卡在 `wait_for_pre_reset`（非连接/网络/owner/link）。插桩 diff 仅增 `DBG:` 观测与
precondition 拆分，不改变 peer/owner/deadline/ABI 语义，host-test 4/4 与 source guard 未破坏。
跨任务审计：未改动 axnet/udp/smoltcp/syscall 产品实现（产品 `sys_poll` 属待 Plan 修复项，未在
契约内擅自修改）；未引入身份型证据。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| P5 host witness RED→GREEN | `rustc --test tests/ms07-recovery-host-harness.rs` | 修改前 `pc={:#x}` label 缺失 FAILED；修改后 `4 passed; 0 failed` | PASS |
| P6 seam host test | `cc -Wall -Wextra -Werror tests/ms07_recovery_probe_test.c` | `ms07_send_step` EAGAIN/EINTR/短写/errno 矩阵全部 assert 通过 | PASS |
| probe standalone compile | `cc ... tests/ms07_recovery_probe.c` | no errors, exit 0 | PASS |
| RISC-V payload | `make tests/ms07_recovery_probe` | ET_EXEC entry `0x10252`，exit 0 | PASS |
| `make host-test` | 全量 host Gate | early-console 6/6, memtrack 8/8, ms03 36/36, ms04 16/16, ms07 harness 4/4 + P6/maker guards，exit 0 | PASS |
| axnet ordinary 全量 | `cargo test ... --lib -- --test-threads=1`（cc-nopie 非 PIE） | `472 passed; 0 failed`，exit 0 | PASS |
| axnet qemu-diagnostics | 同命令 + `--features qemu-diagnostics` | `504 passed; 0 failed`，exit 0 | PASS |
| driver suites | `cargo test ... axdriver_virtio --features net` / `virtio-drivers --features alloc` | `36 passed; 0 failed` / `43 passed; 0 failed`（含 link 测试） | PASS |
| kernel/QEMU build | `make ARCH=riscv64 build` | starry-kernel release 构建成功，.bin 生成，exit 0 | PASS |
| rustfmt | `rustfmt --edition 2024 kernel/src/task/user.rs` | exit 0 | PASS |
| diff 白测 | `git diff --check` | exit 0 | PASS |
| OpenSpec strict | `openspec validate ... --strict` | `Change ... is valid`，exit 0 | PASS |
| validator self-test | `python3 scripts/ms07-qemu-validate.py --self-test` | exit 0 | PASS |
| P7 runtime（info 镜像） | 用户单 hart QEMU `LOG=info` 跑插桩 probe，`/tmp/ms07-qemu-info.log` | `eth0 ip 10.0.2.15/24`、`UDP socket #0 bound *:49152`、`wait_pre_reset iter=1 wait_until_sample_fail errno=14`、`FAIL reason=wait_for_pre_reset` | BLOCKER（定位到产品的 poll 空 nfds） |
| P7 runtime（peer） | `ms07-recovery-peer.py --port 15572` 输出 `/tmp/ms07-peer-host.log` | 0 字节（probe 未进入 peer 阶段，故 peer 收不到包） | 佐证失败在 pre-reset 等待而非 peer |

**Persisted Evidence**

Plan 将本 Cycle 的 Persisted Evidence 设为 `required`（A2-A6 的一次性手工 QEMU runtime 现场）。
当前已产生决定性故障现场 `/tmp/ms07-qemu-info.log`（82 KB info 串口，含 eth0/IP、UDP socket、
`wait_pre_reset`/`wait_until_sample_fail errno=14`）和空 peer 日志（0 字节，证明 probe 未进 peer
阶段）。经证据精简原则，info 串口已足以证明网络健康 + 卡点为 poll 空数组；因此创建 Cycle Evidence
目录并收录最小 raw 串口摘录，供 Plan Review 复核产品 `sys_poll` 修复决策。目录路径：
`evidence/007-single-hart-qemu-qualification/003-replan/`（README + qemu-serial 关键段）。

**Experience Candidates**

None. P5/P6 的验证命令均可低成本重跑且已在 Act Response 记录决定性输出；cc-nopie wrapper 属既有
环境前提（脚本已存在，非本 Cycle 新增验证路径）。

**Remaining Issues**

- 唯一遗留 Minor：x86_64 平台的 `ra=` 占位语义（不影响 RISC-V P7 资格）。
- **BLOCKER（待 Plan）：内核 `sys_poll`/`sys_ppoll` 未实现 `poll(NULL, 0, timeout)` 的 POSIX
  睡眠语义**，对 `nfds==0` 仍走 `check_region(NULL, ...)` → EFAULT(14)，导致 probe
  `wait_until_sample` 失败、P7 首 case 卡在 `wait_for_pre_reset`。修复面：
  `kernel/src/syscall/io_mpx/poll.rs`（`nfds==0` 纯睡眠返回 0）、`kernel/src/mm/access.rs`
  （空 slice 对 NULL 语义）。产品修复决策交 Plan Review。
- P7 其余 runtime（六 case / HMP / 回归）在 `sys_poll` 修复后以冻结 warn 镜像重跑。

**Commit or Diff Reference**

Diff reference: `git diff`（工作树，未提交）。改动跨 P5/P6 六个文件 + payload 两次重建 +
P7 插桩；commit 未建（未获提交授权）。注意：`openspec/specs/references/spec.md`（R60 登记）与
`.claude/runbooks/ms07-qemu-single-hart-recovery-evidence.md` 属并发/先前 Recorder 登记，非本
Cycle 实现 diff；其旧归因（`Service.link_state=None`）已被本 Cycle 最新证据推翻，待 Plan/Recorder
根据最新现场修正。

## Plan Review

- Review Result: replan-required

**Findings**

1. **阻塞：RISC-V `ppoll`错误拒绝零`nfds`的NULL `fds`。** 当前static probe的`poll`包装在
   `0x1157e`装载syscall `0x49`（73），实际进入`sys_ppoll`；`sys_poll`仅在x86_64编译。
   `sys_ppoll`无条件调用`get_as_mut_slice(0)`，NULL地址经`check_region`返回`EFAULT(14)`，与
   `qemu-info-decisive.log`的`wait_until_sample_fail errno=14`一致。现有`do_poll`空集合仍由timer
   future唤醒，因此修复边界是poll syscall参数归一化，不是调度器、网络或通用内存访问层。
2. **阻塞：A3–A6尚未满足。** P5/P6自动实现与Gate通过，但runtime在首个pre-reset采样等待停止；三个
   peer phase、reset/epoch、old/new socket、HMP link flap与四组兼容回归均无PASS证据。A2也只能确认
   fault日志能力，仍需最终资格run无未解释fault。
3. **非阻塞文档发现：** R60 Runbook及其reference摘要仍保留已被新Evidence否证的
   `Service.link_state=None`、owner不守恒和connect失败归因，并使用hash/冻结镜像身份步骤。该修订属于
   `openspec-experience-recorder`及限定R登记，不由本Plan Review修改。
4. **非阻塞措辞发现：** Act/Evidence中的“owner全0”不准确；当前queue/socket epoch为0，健康双向owner
   snapshot为`available=64, device_owned=64, quarantined=0`。

**Deviation Classification**

NEW-EVIDENCE。Cycle 003按stop condition取得精确errno后停止，没有越权修改syscall；新runtime证据否证
此前网络/link/owner归因，并暴露原Plan未包含的产品syscall前置修复。

**Acceptance Gaps**

- A2：最终资格run尚未证明无未解释user fault。
- A3：零`nfds`等待返回`EFAULT`，三个peer phase尚未双向成功。
- A4：reset后的QueueEpoch/SocketEpoch与64/64/0恢复未运行。
- A5：旧socket terminal、新socket成功、HMP off/on及validator PASS未运行。
- A6：MS01/MS04/MS05/MS06 runtime回归未运行。

**Convergence**

reduced。相比父Cycle，P5已使fault可定位，P6已闭合nonblocking send决策，初始link、owner与socket创建均
由runtime证据确认健康；剩余首个阻塞点已收敛为`sys_ppoll(nfds=0)`的确定错误路径。

**Evidence**

- `evidence/007-single-hart-qemu-qualification/003-replan/qemu-info-decisive.log`：link=1、64/64/0、UDP
  bind成功，随后`wait_until_sample_fail errno=14`与harness exit 1。
- `tests/ms07_recovery_probe`反汇编：`poll`装载`a0=0x49`并调用`__syscall_cp`；asm-generic syscall 73为
  `ppoll`。
- `kernel/src/syscall/mod.rs`：RISC-V只分派`Sysno::ppoll`；`Sysno::poll`受x86_64 cfg保护。
- `kernel/src/syscall/io_mpx/poll.rs`、`kernel/src/mm/access.rs`：零长度NULL路径仍执行region检查；
  `do_poll`空集合使用既有`future::timeout`。
- Review focused结果：MS07 host harness 4/4、probe decision test、probe host compile、validator/peer
  self-test、`git diff --check`与OpenSpec strict均exit 0。完整`make host-test`仅在sandbox UDP socket
  `EPERM`处停止，按既有环境分层不构成产品失败。

**Follow-up Decision**

当前Plan Context明确禁止未授权syscall产品修复，且安全修复必须区分syscall参数归一化与通用零长度Rust
slice语义；Act需要新的自包含Current-State Evidence、Task Contract、target RED/GREEN和Gate 2。因此不在
Cycle 003恢复，创建同一Iteration的`004-replan`。用户已认可根因、修复边界与该路由。

**Iteration Plan Update**

Iteration 007仍承载Task 4.2且目标不变；Change Surface与验证契约增加`sys_poll`/`sys_ppoll`零`nfds`
前置修复和focused runtime witness。后续仍依次执行MS07六case与四组回归，不新增Iteration。

**Next Cycle**

`004-replan.md`

**Next Iteration**

None.
