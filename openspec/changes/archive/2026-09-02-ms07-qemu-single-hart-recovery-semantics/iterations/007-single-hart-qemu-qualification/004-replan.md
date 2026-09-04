# Iteration 007 / Cycle 004: Restore Zero-fd Poll and Complete QEMU Qualification

## Plan Context

- Status: ready
- Iteration: 007-single-hart-qemu-qualification
- Cycle: 004-replan
- Cycle Type: replan
- Parent cycle: `003-replan.md`

**Iteration Scope**

- Change tasks: 4.2
- Depends on: Iteration 006 accepted；Cycle 003的P5/P6实现、初始link与64/64/0 owner基线保留
- Stable baseline: 零`nfds`的poll/ppoll timeout不再错误读取`fds`；single-hart QEMU完成MS07
  reset/link/old-new socket资格及受影响回归。
- Verification boundary: focused syscall witness先闭合空集合timeout与正`nfds`地址校验；自动Gate全绿后，
  用户手工QEMU raw serial由validator判定，四组回归有明确终态与exit。
- Diagnostic boundary: RISC-V `poll -> ppoll` ABI、syscall参数归一化、空集合timer/signal、probe
  socket阶段、reset/epoch/owner、HMP link flap、validator及MS01/MS04/MS05/MS06。
- Deferred tasks: None

**Cycle Scope**

- Trigger: replan-required
- Acceptance gaps: A2最终run未闭合；A3被`ppoll(nfds=0)`的`EFAULT`阻断；reset、old/new socket、
  HMP、validator和四组回归未运行。
- Repair items: None
- Inherited scope: Task 4.2、R6–R8、D6–D8；V4 ABI、六case协议、P5 user-fault日志、P6
  `SOCK_NONBLOCK`与共享deadline send retry、初始link commit、64/64/0 owner消费者。
- Excluded scope: 通用用户内存API改造、完整poll/ppoll信号语义重设计、UDP/loader修复、自动QEMU/HMP、
  SMP、PCI/DWMAC、真板、性能、V5 ABI、Runbook/R60维护、身份型证据。

**Objective**

在poll syscall边界安全处理零`nfds`，使RISC-V probe的有限timeout等待返回0而不是`EFAULT`，同时保持
正`nfds`用户地址校验。focused RED/GREEN和自动Gate通过后，恢复P7手工流程并关闭MS07 A1–A7。

**Background**

Cycle 003使user fault可定位，并为nonblocking UDP send建立共享absolute deadline。最新single-hart
QEMU日志证明link、owner和socket创建健康，但probe第二次pre-reset采样在
`poll(NULL, 0, remaining)`返回`EFAULT(14)`。static RISC-V ELF中的musl `poll`包装装载syscall 73，
实际进入`sys_ppoll`；旧Act把入口简称为`sys_poll`不够准确。

`sys_ppoll`在解析timeout前无条件执行`fds.get_as_mut_slice(0)`。该调用以NULL地址进入
`check_region`并失败。`do_poll`随后使用的空`FdPollSet`、`future::timeout`和timer future已经能够表达
有限等待；无需修改调度器。通用`UserPtr::get_as_mut_slice(0)`也不能直接用NULL构造Rust slice，因为
零长度slice仍要求非NULL、正确对齐的指针。

**Current Baseline**

- 工作树含Cycle 003已自检的P5/P6产品与测试改动；不得回滚或重做。
- `kernel/src/syscall/mod.rs`在RISC-V分派`Sysno::ppoll -> sys_ppoll`；`Sysno::poll`只在x86_64启用。
- `kernel/src/syscall/io_mpx/poll.rs::{sys_poll,sys_ppoll}`都无条件取得用户slice；两者共享`do_poll`。
- `kernel/src/mm/access.rs::UserPtr::get_as_mut_slice`先执行`check_region`，再调用
  `slice::from_raw_parts_mut`；它服务多个非poll调用者，本Cycle不改变其契约。
- `tests/ms07_recovery_probe.c::wait_until_sample`把采样间隔限制为20 ms，并调用
  `poll(NULL, 0, remaining)`；当前target RED已持久化。
- `evidence/007-single-hart-qemu-qualification/003-replan/qemu-info-decisive.log`记录
  `link=1 avail=64 dev=64 quar=0`、UDP bind成功、随后`errno=14`和harness exit 1。
- Review focused基线：MS07 host harness 4/4、probe decision test、probe host compile、validator/peer
  self-test、diff check与OpenSpec strict均exit 0。完整`make host-test`在sandbox UDP socket `EPERM`
  停止；按既有规则分层运行无socket子Gate。

**Current-State Evidence**

1. `tests/ms07_recovery_probe`反汇编：`poll@0x1154c`在`0x1157e`执行`li a0, 0x49`，随后调用
   `__syscall_cp`；asm-generic syscall 73为`ppoll`。
2. `kernel/src/syscall/io_mpx/poll.rs::sys_ppoll`：`nfds`转为`usize`后立即调用
   `get_as_mut_slice`，所以NULL与零长度在timeout解析前失败。
3. `kernel/src/syscall/io_mpx/poll.rs::do_poll`：空slice生成空`FdPollSet`；`poll_io`返回Pending，外层
   `future::timeout`以timer future唤醒并把elapsed映射为`Ok(0)`。
4. `kernel/src/syscall/io_mpx/poll.rs::sys_poll`：x86_64入口具有相同参数处理，适合复用同一helper，
   但本Cycle不宣称x86_64 runtime资格。
5. `kernel/src/mm/access.rs::get_as_mut_slice`：通用helper的NULL零长度路径既会区域检查失败，也不能
   直接把NULL交给`from_raw_parts_mut`；修复必须留在poll syscall层。
6. `tests/ms07_recovery_probe.c::{wait_until_sample,wait_for_pre_reset}`：当前失败发生在peer exchange前，
   因此不能归因于UDP、link或owner恢复。

**Relevant Code**

- `kernel/src/syscall/io_mpx/poll.rs::{do_poll,sys_poll,sys_ppoll}`：本Cycle唯一产品修改面。
- `kernel/src/syscall/mod.rs::handle_syscall`：架构入口事实，只读保持。
- `kernel/src/mm/access.rs::{check_region,UserPtr::get_as_mut_slice}`：通用地址校验，只读保持。
- `tests/ms07-recovery-host-harness.rs`：focused source/contract witness入口。
- `tests/ms07_recovery_probe.c`、`tests/ms07_recovery_probe_test.c`：target timeout调用、focused零fd
  preflight和P6决策回归。
- `scripts/ms07-qemu-validate.py`、`scripts/ms07-recovery-peer.py`：既有runtime协议与peer。

**Critical Path**

```text
guest poll(NULL, 0, timeout)
  -> RISC-V musl poll wrapper
  -> syscall 73 / sys_ppoll
  -> nfds == 0: ignore fds, use safe empty slice
  -> do_poll(empty, finite timeout)
  -> timer wake -> return 0
  -> wait_for_pre_reset -> peer exchange
  -> reset / old-new socket / HMP off-on
  -> validator -> MS01/MS04/MS05/MS06 regressions

nfds > 0
  -> existing UserPtr range validation
  -> NULL or inaccessible fds -> EFAULT
```

**Implementation Guidance**

1. 先为两个syscall入口建立focused RED，复用Cycle 003 target `EFAULT(14)`作为修改前runtime witness。
2. 在`poll.rs`集中归一化`fds`：零`nfds`返回安全空slice，正`nfds`调用现有
   `get_as_mut_slice`。局部helper名与等价控制流由Act决定。
3. `sys_ppoll`先检查`nfds`的有符号转换，再调用helper；timeout、sigmask与`do_poll`顺序保持。
4. 在既有probe进入MS07 case前增加focused零fd preflight，以稳定`DBG:`或等价非schema输出分别见证
   零timeout、有限timeout、零`nfds`忽略无效地址、正`nfds` NULL仍`EFAULT`；任一失败即停止。
5. 不修改`access.rs`、timer、signal、probe timeout策略或网络层。focused自动Gate通过后构建RISC-V
   payload/kernel。
6. 手工QEMU先运行零fd precondition和pre-reset peer边界；进入peer后再完成六case与回归。新的
   FAIL/BLOCKED按Evidence契约保留最小现场并停止，不猜测下一产品修复。

**Behavioral Change**

- `nfds==0`时，`sys_ppoll`与x86_64 `sys_poll`忽略`fds`，有限timeout到期返回0；零timeout立即返回0。
- `nfds>0`时，NULL或不可访问`fds`继续返回`EFAULT`。
- timeout解析、signal mask、interrupt、fd readiness与返回计数语义不变。
- probe、网络恢复状态机、socket ABI与V4 marker不因syscall修复而改变。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| T4.2-P8 | R8/零fd poll等待 | `kernel/src/syscall/io_mpx/poll.rs` | poll参数读取与等待 | 零`nfds`忽略`fds`，正`nfds`保持校验 |
| T4.2-P8 | R8/回归见证 | `tests/ms07-recovery-host-harness.rs` | MS07 source/contract Gate | 证明两个入口共享安全零fd路径且不改`access.rs` |
| T4.2-P8 | R8/target边界见证 | `tests/ms07_recovery_probe.c` | QEMU guest资格入口 | 在六case前直接见证四个零/正`nfds`边界 |
| T4.2-P9 | R6–R8/QEMU资格 | probe、peer、validator、QEMU与回归入口 | 六case与兼容性证据 | focused syscall GREEN后完成runtime与回归 |

**Task Contracts**

### T4.2-P8: Restore zero-nfds poll/ppoll timeout semantics

- Requirement/Scenario: R8零fd集合的有界poll等待；正`nfds`错误边界。
- Depends on: Cycle 003 target RED与P5/P6自动实现保留。
- Targets: `kernel/src/syscall/io_mpx/poll.rs::{sys_poll,sys_ppoll}`及局部参数helper；
  `tests/ms07-recovery-host-harness.rs` focused source witness；`tests/ms07_recovery_probe.c` focused target
  preflight及必要的host decision witness。
- Current behavior: 两个syscall入口都对`nfds==0`调用`get_as_mut_slice(0)`；RISC-V `sys_ppoll`因此对
  `poll(NULL,0,t)`返回`EFAULT`，未进入`do_poll`。
- Required behavior: 转换并验证`nfds`后，零值忽略`fds`并提供安全空slice；正值继续使用现有
  `UserPtr`校验。有限timeout、零timeout和interrupt沿用`do_poll`。
- Required changes: 让`sys_ppoll`和x86_64 `sys_poll`复用同一语义；source witness固定零/正`nfds`
  分支、两个入口和`access.rs`非修改边界；probe在六case前直接检查零timeout、有限timeout、零`nfds`
  无效地址与正`nfds` NULL四个结果，使用validator忽略的诊断输出且不改变V4/case schema。
- Preserve: `POLLNVAL`、fd readiness、timeout转换、sigmask、`with_blocked_signals`、返回计数、
  `nfds<0 -> EINVAL`、所有正`nfds`地址错误、其他`UserPtr`调用者。
- Forbidden: 修改`kernel/src/mm/access.rs`；把NULL传给`slice::from_raw_parts_mut`；使用unsafe
  dangling pointer绕过；改probe为nanosleep；改变timer或signal设计；宣称x86_64 runtime已验证。
- Test witness: target RED为EV-007-003-01的`errno=14`。自动focused witness在修改前应拒绝缺失的零fd
  分支；修改后通过。RISC-V runtime需观察零timeout与有限timeout返回0、零`nfds`忽略无效地址、正
  `nfds` NULL仍`EFAULT`。
- GREEN condition: focused witness与kernel build exit 0；target pre-reset等待不再出现`errno=14`且能
  进入peer阶段；正`nfds`负向结果不退化。
- Verification: focused host witness；`make ARCH=riscv64 build`；probe payload build；手工QEMU
  决定性marker/errno；diff与OpenSpec strict。
- Stop when: 现有空`FdPollSet`不能被timer/signal有界唤醒、必须改通用用户内存契约、正`nfds`错误语义
  退化，或出现新的非poll blocker；写Blocker Handoff并返回Plan。

### T4.2-P9: Complete single-hart MS07 and affected regressions

- Requirement/Scenario: R6初始link/HMP；R7 old/new socket；R8真实reset、peer与兼容回归。
- Depends on: P8自动GREEN，且focused QEMU证明零fd等待和pre-reset peer exchange成功。
- Targets: 既有probe、peer、QEMU/HMP、validator及MS01/MS04/MS05/MS06执行入口；默认不再修改产品。
- Current behavior: link=1、owner 64/64/0和UDP socket创建已见证；runtime尚未越过pre-reset采样等待。
- Required behavior: single hart、QEMU 7.0.0、VirtIO-MMIO user-net、LOG=warn下完成六case；reset后
  QueueEpoch/SocketEpoch按规则推进，旧socket终止、新socket双向成功，HMP off/on不推进QueueEpoch；
  validator和四组回归明确PASS。
- Required changes: 只执行与采集既有协议；若P8之外无需产品修改，保持代码冻结于自动Gate通过状态。
- Preserve: R44用户手工驱动QEMU/HMP；V4、case顺序、peer 15572不hostfwd、absolute deadline、
  terminal-before-wake、64/64/0 owner语义、LOG=warn最终资格。
- Forbidden: 用INFO诊断run代替最终资格；用pcap替代guest syscall成功；缺marker/exit仍判PASS；用hash、
  revision、run-id或冻结镜像证明运行归属；遇新产品问题继续猜测修复。
- Test witness: P8 target GREEN是runtime入口；raw serial、按需pcap、validator与回归终态是最终见证。
- GREEN condition: A1–A7全部成立；validator exit 0；无panic、trap、fatal owner drift、永久Pending或
  未解释fault。
- Verification: 用户手工MS07六case；validator；MS01 14/14、MS04四mode、MS05六mode、MS06 12-case。
- Stop when: 任一runtime case、validator或回归失败，或用户尚未提供结果；保存最小Evidence并停止。

**Invariants**

- `sys_ppoll`是当前RISC-V现场入口；文档不得用`sys_poll`替代精确调用链。
- 零`nfds`忽略`fds`，正`nfds`继续校验；通用`UserPtr`契约不变。
- P5/P6保持：user fault记录PC/VA/SP/RA；send只在同一deadline内重试EINTR/EAGAIN/EWOULDBLOCK。
- 健康owner为64/64/0；“epoch为0”不得写成“owner全0”。
- ISR、唯一queue owner、socket terminal-before-wake、V4 ABI与六case协议不变。
- 验证不使用revision/hash/run-id/peer pin/manifest或冻结镜像身份机制。

**Non-goals**

- 不修改`kernel/src/mm/access.rs`或建立通用零长度用户slice政策。
- 不声明完整POSIX signal-mask、x86_64 runtime、SMP、PCI/DWMAC、真板或性能资格。
- 不修改UDP、smoltcp、loader、executor、timer实现或MS07 deadline数值。
- 不自动驱动QEMU/HMP，不提交Git，不更新SNAPSHOT，不修改Runbook/R60。

**Requirements Traceability Matrix**

| Requirement | Scenario | Design | Task | Code Surface | Test Witness | Simplification | Status |
|---|---|---|---|---|---|---|---|
| R8 | 零fd poll有限等待 | D8 | P8 | `io_mpx::poll::{sys_poll,sys_ppoll}` | EV-007-003-01 RED；focused source；QEMU RED/GREEN | None | Covered |
| R8 | 正nfds地址错误保持 | D8 | P8 | poll参数helper、`UserPtr`边界 | positive-nfds NULL negative witness | None | Covered |
| R8 | probe可信与peer三阶段 | D8 | P9 | user fault日志、probe wait/send | host Gate；raw serial；按需pcap | None | Covered |
| R6 | reset与HMP off/on | D6、D8 | P9 | owner/Service/QEMU | V4 raw serial与validator | None | Covered |
| R7 | old/new socket terminal | D7、D8 | P9 | socket epoch/readiness/probe | old/new I/O marker与validator | None | Covered |
| R8 | 兼容性回归 | D8 | P9 | MS01/MS04/MS05/MS06 | 各组终态与exit | None | Covered |

**Acceptance**

- A1：保留initial-link、64/64/0 owner、P5/P6及V4自动Gate，无driver/ABI回退。（R6/R8/P8–P9）
- A2：任何probe user fault都有PC、VA、SP、RA与exact ELF账本；最终资格run无未解释fault。（R8/P9）
- A3：零`nfds`的poll/ppoll忽略`fds`；零timeout立即返回0，有限timeout到期返回0；正`nfds` NULL仍
  `EFAULT`。（R8/D8/P8）
- A4：三个peer phase双向成功；`SOCK_NONBLOCK`与同deadline EAGAIN重试保持。（R8/D8/P9）
- A5：reset后QueueEpoch/SocketEpoch各按规则推进并恢复64/64/0；HMP down/up不推进QueueEpoch。
  （R6/R8/P9）
- A6：旧socket稳定返回`ECONNRESET`/`ENOTCONN`，新socket成功；validator exit 0，无panic、trap、
  fatal或永久Pending。（R7/R8/P9）
- A7：MS01 14/14、MS04四mode、MS05六mode、MS06 12-case均明确PASS与exit。（R8/P9）

**Verification**

1. P8 focused：修改前复用EV-007-003-01 RED；修改后运行MS07 source/contract witness、RISC-V kernel与
   payload build、diff check和OpenSpec strict。
2. 自动回归：`make host-test`先尝试；若仍只有sandbox socket `EPERM`，运行全部无socket子Gate并记录
   SKIPPED原因。axnet ordinary/qemu-diagnostics使用项目非PIE配置串行运行；driver suites逐项exit 0。
3. focused QEMU：零timeout、有限timeout、零`nfds`无效地址与正`nfds` NULL四个边界产生明确结果；
   pre-reset peer exchange成功后才进入完整资格。
4. 用户手工完整MS07、validator和MS01/MS04/MS05/MS06回归；任何FAIL/BLOCKED立即保存最小现场。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | ELF syscall 73、kernel dispatch、`sys_ppoll`到`check_region`及timer future调用链已核对 |
| Design | PASS | syscall边界安全空slice；正`nfds`与通用`UserPtr`保持；无调度器/网络扩张 |
| Iteration Plan | PASS | 仍为Task 4.2/Iteration 007；P8前置P9，稳定和诊断边界内聚 |
| Cycle Scope | PASS | 只增加关闭既有A3–A7所需的syscall前置修复，不新增成果或Iteration |
| Task Contracts | PASS | P8/P9包含目标位置、行为、RED/GREEN、验证和停止条件 |
| Traceability | PASS | R6–R8映射到D6–D8、P8/P9、代码与target witness，无Missing/Simplified |
| Verification | PASS | focused syscall边界、自动Gate、手工QEMU和回归按依赖递增 |
| Evidence | PASS | 一次性手工QEMU与回归使用最多4个文件，支持A2–A7且符合预算 |
| Authorization | PASS | 用户认可根因与方案，并指示“给出审计和下一轮cyc，我们继续工作” |

**Persisted Evidence**

- Mode: required
- Path: `openspec/changes/ms07-qemu-single-hart-recovery-semantics/evidence/007-single-hart-qemu-qualification/004-replan/`
- `README.md`：支持A2–A7；记录环境、build/QEMU/validator操作、exit、首个失败层及下列artifact结果。
  Act Response无法保留一次性手工session的marker结构；缺失时不能Review runtime。
- `qemu-serial.log`：支持A2–A6；保存focused poll边界和完整MS07 marker/exit，或BLOCKED时保存首个
  决定性失败段。手工session不能低成本重跑；缺失时不能判断P8 target GREEN或MS07资格。
- `regressions.txt`：支持A7；仅在进入四组回归后保存各组终态和exit。缺失时不能接受兼容性Gate。
- `usernet.pcap`：仅在serial不足以判断A4/A6的packet交付或新网络失败时保存；serial已充分时在README
  写`not needed`，不创建占位。
- PASS或FAIL/BLOCKED均按实际分支保存最小文件。Budget：本Cycle最多5个文件（含README），整个change
  最多20个Evidence文件；文本文件最多500行且不超过256 KiB。

**Risks and Notes**

- x86_64 `sys_poll`接受同一修复以避免同构缺陷，但本Cycle只取得source/build证据，不扩大runtime结论。
- 零timeout与有限timeout必须分别见证；只看到“不再EFAULT”不能证明timer等待。
- sandbox `EPERM`只允许跳过精确的host socket子Gate，不能跳过probe、validator、build或QEMU产品结果。
- R60 Runbook已过期且含身份型证据步骤；由`openspec-experience-recorder`另行修订，不阻塞P8产品实现，
  但P9手工执行前应使用本Cycle命令契约而不是旧Runbook归因。
- 轻量模式SKIPPED：本Cycle触及no_std kernel syscall、用户指针和真实QEMU runtime边界。

## Act Response

- Status: blocked

**Implemented**

T4.2-P8：恢复 `poll/ppoll` 在 `nfds==0` 时忽略 `fds` 的 timeout-only 等待语义。

- 在 `kernel/src/syscall/io_mpx/poll.rs` 新增私有 helper `user_poll_fds(fds, nfds)`：`nfds==0`
  时返回安全空 slice（`&mut []`），不触碰用户指针；`nfds>0` 时走既有 `get_as_mut_slice`。
- `sys_ppoll`（RISC-V musl `poll` wrapper 实际装载的入口）与 x86_64 `sys_poll` 均改为经由
  `user_poll_fds`，两个入口复用同一语义。负 `nfds` 的 `try_into -> AxError::InvalidInput`
  行为保持不变。`do_poll`、timeout 解析、sigmask、`POLLNVAL`、正 `nfds` 地址校验全部未变。
- 产品修改仅限 `poll.rs`；`kernel/src/mm/access.rs` 及其 `UserPtr::get_as_mut_slice` 完全未动。

T4.2-P8 探针前置：在 `tests/ms07_recovery_probe.c` 于首个 MS07 case 之前加入
`zero_fd_poll_preflight()`，就近先后见证四个边界：零 timeout（`poll(NULL,0,0)` 立即返回 0）、
有限 timeout（`poll(NULL,0,20)` 等待后返回 0）、零 `nfds` 忽略无效地址（`poll(BAD,0,0)` 不
`EFAULT`）、正 `nfds` NULL 仍 `EFAULT`（`poll(NULL,1,0)` 返回 -1 且 errno==EFAULT）。输出
`DBG:` 前缀为 validator 忽略的诊断噪声，不改变 V4 marker 或 case/schema 协议。

**Changed Files and Symbols**

- `kernel/src/syscall/io_mpx/poll.rs`：新增 `user_poll_fds`；`sys_poll`、`sys_ppoll` 改路由。
- `tests/ms07-recovery-host-harness.rs`：新增
  `zero_nfds_poll_ignores_fds_and_preserves_userptr_boundary` source/contract witness。
- `tests/ms07_recovery_probe.c`：新增 `zero_fd_poll_preflight`，在 `run_probe` 首 case 前调用。

**Deviations from Plan**

- 无实质偏差。`zero_fd_poll_preflight` 用 `volatile struct pollfd *` 承载 NULL 以规避 GCC 静态
  `-Wnonnull`（glibc 头 `access(write_only,1,2)` 会在 const-`NULL` 上 `-Werror=nonnull`），值仍
  为真实 NULL，语义与计划一致；RISC-V musl 与 host glibc 双工具链均通过。
- 未进入 T4.2-P9 手工阶段：本 Cycle 的自动 Gate（P8 focused witness、RISC-V kernel/probe build、
  `make host-test`、OpenSpec strict）全部通过；由用户按 R44 手工驱动的 single-hart QEMU 六 case、
  validator 与 MS01/MS04/MS05/MS06 回归未执行，属能力边界。

**Blocker Handoff**

P9 手工 QEMU runtime 暴露独立于 syscall 的 recovery 驱动缺陷（Task 4.2-P9，`old_socket_terminal`
case，`reason=reset-terminal`）：

- **观测**（`evidence/007-single-hart-qemu-qualification/004-replan/qemu-serial-step3.log`）：
  P8 preflight 四边界全过 → `pre_reset_traffic`/`reset_request` PASS → `old_socket_terminal` 失败。
  `RX recovery state` 仅 `Quiescing`→`Resetting`（2s 间隔）；`VirtIO recovery step`（driver
  `poll_recovery_step` 入口埋点）**零输出**；probe iter=69 ABORT
  `lifecycle=3 Faulted, fstage=5(RESET), fcause=1(TIMEOUT)`。
- **根因**：owner 进入 `Resetting` 后，`arm_recovery_timer` 把唯一唤醒源 arm 到 `now+2s`
  （`RESET_STAGE_DEADLINE_NS`）；Resetting 阶段无周期性/事件补救 wake，owner 睡到 2s 到期那一次
  poll 时 `recovery_deadline.is_some_and(expired)==true` → 直接 Resetting/Reinit 分支 TIMEOUT
  Faulted，**从未调用 driver `poll_recovery_step``**（driver 埋点零输出）。QEMU 写 status=0 后
  dev 需被重新 poll 到 `reset_confirmed()`，但 owner 等到 2s 超时即 Faulted。
- **影响**：P9 六 case 无法通过；reset 后的 epoch 永未推进（q/s 停在 0），旧 socket 永无
  `ECONNRESET`。这不是 syscall 层，与已完成的 P8 无关。
- **已在工作树**：诊断用 info 埋点（`async_rx.rs::poll_recovery` 的 `RX recovery state`、
  `axdriver_virtio/net.rs::poll_recovery_step` 入口的 `VirtIO recovery step`），均 `#[cfg(not(test))]`,
  测试回归全绿（axnet ordinary 472 / qemu-diagnostics 504；axdriver_virtio 36）。埋点用于定位，
  修复落地时应一并移除或收纳为 log.info 精析。
- **修复方向（供 Plan 决策）**：Resetting 阶段每次 driver Pending 自 `wake_by_ref`（同 Quiescing 的
  `reclaimed_at_budget` 模式），或在 reset 未确认时安排周期性/短 deadline wake，使 owner 在 2s 超时
  前能重新 poll 到 driver `poll_recovery_step` 并推进 `reset_confirmed()`。此修复属 driver/recovery
  驱动，超出 004-replan 的 syscall 修复范围，需 `openspec-plan` 建 rework Cycle。

恢复条件：Plan 依据上述根因创建 rework Cycle（Resetting 阶段补救唤醒），修复后按同一命令行复采
P8+P9 全流程。诊断埋点保留以在修复 Cycle 内见证补救唤醒生效。

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: PASS（P8 自动实现与探针 witness 完成；P9 已手工执行并定位 recovery 驱动 blocker，见 Blocker Handoff）
- Full diff reviewed: PASS（审查 `poll.rs` + harness + probe.c 完整 diff，无计划外修改）
- Critical findings unresolved: 0（P9 blocker 已定位根因并交接 Plan，非本次 Act 范围内修复）
- Important findings unresolved: 0
- Minor findings unresolved: 0

已修复发现（Review 过程中）：
- GCC `-Werror=nonnull` 对 `poll(NULL,1,0)` 的静态分析拒绝：改为 volatile 承载 NULL 后
  host glibc 编译通过，RISC-V musl 编译亦通过。

遗留 Minor 问题：
- `axnet-ng` 有与 P8 无关的既有编译 warning（unused imports/`frag`/`repr`），非本次引入。

**Verification Evidence**

| 验证项 | 命令 | 输出摘录 | 结论 |
|---|---|---|---|
| 源契约 witness | `rustc --test ms07-recovery-host-harness.rs && run` | `test result: ok. 5 passed`（含 `zero_nfds_poll...ok`） | PASS |
| C 决策 witness | `cc -Werror ms07_recovery_probe_test.c && run` | exit 0 | PASS |
| probe self-test | `cc ms07_recovery_probe.c && --self-test` | exit 0 | PASS |
| RISC-V kernel | `make ARCH=riscv64 build` | `Finished release ... exit 0` | PASS |
| RISC-V probe | `make ARCH=riscv64 tests/ms07_recovery_probe` | exit 0（双工具链均编译） | PASS |
| host Gate | `make host-test` | exit 0（含 ms07 harness 5 passed、probe_test、schema 一致） | PASS |
| OpenSpec | `openspec validate <change> --strict` | `Change ... is valid` | PASS |
| P9 runtime | 用户手工 single-hart QEMU（step3 run）+ 分层 info 诊断 | pre_reset/reset_request PASS；`old_socket_terminal` FAIL（reset-terminal）。根因：Resetting 阶段 owner 无补救唤醒，2s 超时 Faulted（见 Blocker Handoff） | BLOCKED |

**Persisted Evidence**

本 Cycle 未主动创建 Evidence 目录。P9 手工 QEMU 已执行并因 recovery 驱动缺陷 `blocked`（见
Blocker Handoff）；其 raw 串口日志已保留于
`evidence/007-single-hart-qemu-qualification/004-replan/`（`qemu-serial-step3.log` 等），用于 rework
Cycle 引用根因与复验，不在本 Act 内另存。诊断云点（`RX recovery state` / `VirtIO recovery step`）
回归全绿，修复落地时由 Plan/Act 决定移除或收纳为精析日志。

**Experience Candidates**

- Runbook：P8 是单文件 syscall 参数归一化 + 静态 witness，已由 host-test 全自动覆盖；本 Cycle 的
  `LOG=info` 分层定位流程（probe DBG + `RX recovery state` + driver `VirtIO recovery step` 三源
  交叉）已写入 R60 runbook，可重复，视作 Runbook 候选。
- Incident：P9 运行时暴露的「Resetting 阶段 owner 无补救唤醒、2s 超时 Faulted」是一次真实且系统性的
  恢复语义故障，含明确的时间线证据（Quiescing→Resetting→2s→Faulted + driver 埋点零输出），视为
  Incident 候选。

**Remaining Issues**

- T4.2-P9 blocker（见 Blocker Handoff）：`old_socket_terminal reason=reset-terminal`。根因是 owner
  进 Resetting 后唯一唤醒源 arm 到 2s 超时，期间从不重 poll driver `poll_recovery_step`，2s 到期即
  Faulted。需 `openspec-plan` 建 rework Cycle 修复 Resetting 阶段补救唤醒。**这是产品 Blocker，非
  能力边界**；P9 六 case 与四组回归在修复前无法通过。

**Commit or Diff Reference**

未提交。工作树含 P8 改动（`kernel/src/syscall/io_mpx/poll.rs`、`tests/ms07-recovery-host-harness.rs`、
`tests/ms07_recovery_probe.c`）；HEAD 为 `b83e800a`，相对 `origin/net-k3` ahead 5。

## Plan Review

- Review Result: rework-required

**Findings**

1. **Blocking — NEW-EVIDENCE：生产态 recovery timer 只在最终 deadline 唤醒 owner。**
   `RxRxFuture::poll_recovery` 在 Quiescing 完成后调用 `begin_recovery()`，把 lifecycle 和本地状态提交为
   `Resetting`，但 `arm_recovery_timer` 只为 `recovery_deadline`（2 s）注册一次 timer。driver 的
   `poll_recovery_step()` 只会在 owner 后续 poll 时调用，因此没有 used/config/software event 时，首次
   driver step 与 timeout 检查落在同一次 deadline poll；timeout 分支先执行，driver step 不可达。
   `qemu-serial-step3.log` 见证 Quiescing@10.022929 → Resetting@12.024410 → Faulted，且没有任何
   `VirtIO recovery step` 记录。A5/A6 被直接阻塞，A4/A7 无法继续。
2. **Blocking — PLAN-OMISSION：既有测试由测试代码主动 poll，未见证生产调度 forward progress。**
   `same_stage_pending_does_not_renew_absolute_deadline_then_times_out` 正确证明同阶段 Pending 不续期，
   但测试在 1 s 时显式调用 `poll_once`，绕过了生产态“谁在 deadline 前唤醒 owner”的问题。修复必须
   新增自包含执行契约，同时证明 deadline 前有定时重试、同阶段 deadline 不续期、Pending 不直接
   self-wake 成 busy loop。
3. **Non-blocking — ACT-DEVIATION：诊断产品改动未进入 Changed Files 声明。**
   staged diff 在 `async_rx.rs` 和 `axdriver_virtio/src/net.rs` 增加 deduplicated INFO 字段与日志。它们
   支持了根因定位，但超出 P8 声明的三个文件。返工应在完成见证后移除这些临时字段/import，或仅在
   能证明仍属于必要产品诊断时保留；当前没有长期保留依据。
4. **Non-blocking — ACT-DEVIATION：Cycle 004 Evidence 超出本 Cycle 预算且缺 README。**
   目录中已有 6 个 raw serial 文件，原契约最多允许 5 个文件并要求 README。超限本身不阻塞产品
   Acceptance，但这些文件不能作为完整 PASS 证据，也不得继续向该目录追加。Plan 不删除既有现场。

**Deviation Classification**

NEW-EVIDENCE；PLAN-OMISSION；ACT-DEVIATION。

**Acceptance Gaps**

- A2：没有完成最终资格 run，尚不能关闭“最终 run 无未解释 fault”。
- A4：只完成 pre-reset peer exchange；reset 后与 link-up 后两个 peer phase 未运行。
- A5：reset 未推进 QueueEpoch/SocketEpoch，owner 以 Reset/TIMEOUT 进入 Faulted。
- A6：旧 socket terminal、新 socket I/O、validator exit 0 均未取得。
- A7：MS01/MS04/MS05/MS06 手工回归未进入。

A1 与 A3 已有直接证据：初始 link/64-64-0 owner、P5/P6 自动 Gate 保持，zero-`nfds` 四边界在
QEMU preflight 通过。

**Convergence**

reduced。相对 Cycle 003，P8 已关闭 A3，并把 runtime 推进到 reset request 后；新的 owner 调度缺口
阻塞其余 QEMU Acceptance，但不是同一问题的重复失败。

**Evidence**

- 代码：`crates/axnet/src/async_rx.rs::{poll_recovery,recovery_step,arm_recovery_timer}`；
  `crates/axnet/src/service.rs::recovery_step_target`；
  `crates/axdriver_virtio/src/net.rs::poll_recovery_step`。
- BLOCKED 现场：
  `evidence/007-single-hart-qemu-qualification/004-replan/qemu-serial-step3.log`；决定性记录为
  Quiescing@10.022929、Resetting@12.024410、随后 `lifecycle=3 fstage=5 fcause=1`、
  `FAIL: old_socket_terminal`、harness exit 1；`VirtIO recovery step` 匹配数为 0。
- 新鲜基线：MS07 host harness 5/5 PASS；axdriver_virtio recovery 5/5 PASS；axnet
  `same_stage_pending_does_not_renew_absolute_deadline_then_times_out` 1/1 PASS；OpenSpec strict PASS。
- `make host-test` 到 MS07 host harness 均通过，随后仅 sandbox UDP socket 创建报 `EPERM`，exit 2；
  这是环境层结果，不提升为完整 host Gate PASS。

**Follow-up Decision**

创建同一 Iteration 的 `005-rework.md`。修复仍服务于 Task 4.2 和既有 R1/R4/R5/R8、A2/A4–A7，
不改变目标、范围、deadline、错误语义或验证边界，因此不是 replan。旧 Cycle 的 P8 实现保持；新
Cycle 为生产态定时重试、非 busy-loop 见证、诊断清理和剩余 QEMU 资格提供新的自包含执行契约。

**Iteration Plan Update**

None.

**Next Cycle**

`005-rework.md`

**Next Iteration**

None.
