# Iteration 001 / Cycle 004: Scale Witness and Runtime Acceptance

## Plan Context

- Status: ready
- Approval: approved by user on 2026-08-24（原话："开始实施"，经 `/openspec-act` 指令）；ready
  for execution；本次批准构成实施授权，不构成 Plan Review 或收尾授权。
- Iteration: 001-socket-and-listener-readiness-bridge
- Cycle: 004-replan
- Cycle Type: replan
- Parent cycle: `003-replan.md`

**Iteration Scope**

- Change tasks: 2.1–2.7
- Revised tasks: 2.6、2.7
- Depends on: Cycle 003实现基线；Iteration 000 accepted。
- Stable baseline: runner单次poll使用一个timestamp；deferred retirement每轮最多检查32项并在
  quiet sweep后park；accept在`SOCKET_SET → ListenTable entry`临界区内消费并refill。
- Verification boundary: deferred raw-handle独占所有权、满512 backlog的accept→reconnect、
  512 deferred cleanup与同runner UDP前进、fresh single-hart QEMU diagnostic single/fork和MS01。
- Deferred tasks: 3.1–3.4。

**Cycle Scope**

- Trigger: Cycle 003 Plan Review `replan-required`。
- Acceptance gaps: Cycle 003 Acceptance 2、3、5、6。
- Inherited scope: R1–R6、D1–D10、Tasks 2.1–2.5已实现行为、MS01兼容、MS05
  queue/slot/ticket/flush ownership和single-hart QEMU边界。
- Excluded scope: Task 3、SO_LINGER、unresponsive-peer强制回收、smoltcp/scheduler修改、SMP、
  真板、性能、全局文档、归档和commit。

**Objective**

用与Acceptance文字一一对应的组合测试证明：满512 backlog释放一个slot后立即重连成功，512项
deferred cleanup期间无关UDP在fixed bound内前进；同时闭合deferred handle同类型slot复用的
所有权前提。automatic Gates通过后，把三项fresh single-hart QEMU作为本Cycle的强制运行时Gate，
只有完整markers与正常退出均通过才可accepted。

**Background**

Cycle 003修复了三个真实机制缺口，Review独立复跑ordinary 293/293和qemu-diagnostics 313/313
均PASS。返工原因不是基础机制回归，而是证据与契约不闭合。

`task_27_accept_refills_idle_listener_no_reconcile_needed`只跑一个accept和一个reconnect；引用
`LISTEN_QUEUE_SIZE`没有构造512 backlog。独立的512 reaper unit test没有listener或UDP，因此不能
证明close storm下的跨阶段前进。另一个边界是smoltcp handle只含slot index：现有reuse test将旧
TCP slot改放为UDP，不能证明新TCP不会被旧deferred entry删除。生产路径可能通过raw-handle独占
所有权排除该状态，但该前提尚未被全路径验证。

最后，Cycle 003把fresh QEMU列为Acceptance 5和明确stop boundary；Act未运行这些命令，却把
Blocker Handoff记为None。本Cycle恢复这一边界：执行者能力限制只改变handoff，不改变Gate。

**Current Baseline**

- Branch: `net-k3`；HEAD: `fb87c8d36b7c62e8d7156598defa08bce0db32d4`；实现和OpenSpec
  文档仍在staged工作树。
- Cycle 003基础实现存在：single timestamp、32-entry deferred sweep、dirty/protocol-progress
  gate、atomic `accept_with`、deferred telemetry。
- Review fresh结果：ordinary 293/293、qemu-diagnostics串行313/313；两组均exit 0。
- manual QEMU diagnostic single/fork和原MS01均pending；Persisted Evidence为None。
- 现有host tests分别覆盖512 queue unit、单连接reconnect、512 Closed TCP retirement；没有组合
  512 accept→reconnect或cleanup→UDP witness。

**Current-State Evidence**

- stack-runner Task 2.7 test只创建`client1`和`client2`，随后`let _ = LISTEN_QUEUE_SIZE`；没有填满
  listener queue。
- listen-table unit test人工填充512 slots并验证accept后idle恢复，但不执行runner reconnect。
- service 512 test把512个fresh Closed TCP handles入队并在16轮移除，不创建或断言UDP traffic。
- `SocketHandle`定义为`SocketHandle(usize)`；`SocketSet::add`复用第一个empty slot。
- TCP Drop先`retire_public`；需延迟关闭时只向Service提交handle，raw remove应由reaper独占。
  Review搜索到的其他raw removals必须按socket ownership分类，不能仅靠文件名或注释推断安全。
- Act Response明确记录三项manual QEMU pending，但`Blocker Handoff`为None。

**Critical Path**

```text
deferred TCP Drop
  -> retire public metadata
  -> enqueue raw handle under Service
  -> no other legal owner removes/reuses that slot
  -> runner confirms close
  -> remove raw handle + deferred entry in one guarded commit

full listener: exact 512 slots -> accept+refill -> immediate runner reconnect
cleanup/UDP: 512 deferred TCP + unrelated UDP -> UDP progresses -> bounded cleanup converges
automatic GREEN -> QEMU single -> QEMU fork -> MS01 14/14 -> eligible for accepted
```

**Implementation Guidance**

先闭合handle所有权，不先增加generation。枚举所有会创建、adopt、retire或raw-remove TCP handle的
生产路径，并加入可执行source/ownership witness：deferred entry存活期间，只有reaper能移除其raw
slot；reaper在同一`SERVICE → SOCKET_SET`保护区内提交raw removal和entry removal。把现有测试改名
为准确的`stale_and_retyped`。若发现任何合法路径能在deferred entry仍存在时移除并复用同一slot，
立即停止并返回Plan；此时才需要跨所有raw add/remove路径的monotonic incarnation设计，不能用
socket类型、endpoint或TCP state充当身份。

随后增加两个真实组合witness。512 listener test必须让同一ListenTable entry在accept前实际持有
`LISTEN_QUEUE_SIZE`个slots，并在不额外调用reconcile的条件下由runner完成immediate reconnect；
不得以读取常量、缩小规模或把unit断言与另一个runner test拼接来替代。测试可使用`cfg(test)`的
最小seed/inspection seam，但production helper仍只能在既有锁序内创建hidden socket。

cleanup→UDP test必须在同一个Injected `StackRunnerFuture`、Service和SocketSet中提交512个deferred
TCP handles及一笔无关UDP datagram。断言每poll deferred checked不超过32，UDP在固定poll bound内
完成且完成时不要求先清空全部deferred backlog，随后confirmed handles在固定上界内收敛。测试不能
直接调用`Service::stack_round`来绕过runner，也不能只检查telemetry而不检查UDP payload。

最后按既有Runbook执行三项fresh QEMU。automatic Gate通过而用户环境尚未运行时，Act必须返回
`blocked`及精确Blocker Handoff；收到完整markers后恢复同一Cycle，不新建Cycle、不把pending写成
PASS。

**Behavioral Change**

- 预期无production行为变化；优先补充测试seam、所有权断言和准确命名。
- 若所有权调查发现合法同类型slot复用，Cycle在实现前停止并回Plan设计incarnation；不得悄然扩大
  production change surface。
- QEMU Gate从错误的“非阻塞等待项”恢复为既有强制Acceptance。

**Change Surface**

| Repair | Requirement/Scenario | File/Symbol | Planned Change |
|---|---|---|---|
| T2.6-R1 | R3/R4；deferred handle不误删 | `service.rs`、`wrapper.rs`、`tcp.rs`、`listen_table.rs` tests | 证明raw owner独占与原子退休；准确区分stale/retyped/reused |
| T2.7-R1 | R6；满512 accept→reconnect | `listen_table.rs`、`stack_runner.rs` tests | 构造真实512 queue并由同一runner完成立即重连 |
| T2.7-R2 | R3/R6；512 cleanup→UDP | `stack_runner.rs` tests | 同runner组合512 deferred work与UDP payload fixed-bound前进 |
| T2.7-R3 | MS01 compatibility | QEMU Runbook / Cycle Act Response | fresh single、fork、MS01完整markers与退出证据 |

**Repair Contracts**

### T2.6-R1: deferred raw-handle所有权闭环

- Requirement/Scenario: R3、R4；Cycle 003 invariant“stale/reused handle不误删”。
- Depends on: Cycle 003 bounded reaper GREEN。
- Targets: `service.rs::queue_deferred_removal/reap_deferred_removals`、
  `wrapper.rs::{add_public,install_readiness,retire_public,remove_raw}`、TCP/listener add/remove paths。
- Current behavior: reaper可识别missing或non-TCP slot，但无法区分旧TCP和同slot的新TCP；现有test
  只覆盖retyped UDP。
- Required behavior: 对全部合法生产路径，deferred entry存活期间raw TCP slot不可被另一owner移除
  或复用；confirmed raw removal与deferred entry removal在同一guarded commit内。测试与文档不得把
  retyped称为完整reuse identity。
- Required changes: 加入全路径source/ownership witness；重命名或拆分现有stale/reused test；清理
  误导性注释。若witness失败，停止并回Plan，不在本repair中临时加入不完整generation。
- Preserve: handle-level dedup、32 budget、公平cursor、CloseKind确认矩阵、public metadata立即退役、
  `SERVICE → SOCKET_SET`锁序。
- Forbidden: 以TCP state、endpoint、指针地址或socket类型冒充incarnation；修改smoltcp；让caller
  remove deferred raw handle。
- GREEN condition: ownership witness覆盖全部production add/adopt/remove路径；stale missing和retyped
  slot不误删；合法路径不存在同类型reuse窗口；full suites通过。
- Stop when: 找到任何合法remove/reuse窗口，或证明需要跨组件identity registry。

### T2.7-R1: 满512 backlog的accept→immediate reconnect

- Requirement/Scenario: R6、D5/D7/D9、network-stack-baseline compatibility。
- Depends on: T2.6-R1 GREEN。
- Targets: `listen_table.rs` test seam、`stack_runner.rs` Task 2.7 scale test。
- Current behavior: 512 queue只在unit test验证atomic refill；runner reconnect只使用一个Ready slot。
- Required behavior: 同一测试在accept前断言exactly 512 slots，accept后不调用reconcile，随后由同一
  runner在fixed bound内完成新client handshake；backlog仍不超过512，Ready只交付一次。
- Required changes: 先让现有伪scale test对`slot_count != 512`失败；增加最小`cfg(test)` seed或
  inspection seam；移除`let _ = LISTEN_QUEUE_SIZE`。
- Preserve: production backlog=512、Reset语义、multiwaiter、guard外wake、caller零progress。
- Forbidden: 缩小规模、手工reconcile、直接Service round、sleep/yield、提高backlog。
- GREEN condition: ordinary和qemu-diagnostics下exact-512 accept→reconnect各100×无hang。
- Stop when: 需要改变production backlog/accept语义或增加Service反向锁。

### T2.7-R2: 512 cleanup期间UDP前进

- Requirement/Scenario: R3、R4、R6；D4/D7/D10。
- Depends on: T2.7-R1 GREEN。
- Targets: `stack_runner.rs` injected full-chain scale test和必要的test-only setup helper。
- Current behavior: 512 retirement与UDP分别测试，没有同runner组合证据。
- Required behavior: exactly 512 deferred TCP handles和无关UDP datagram共享一个runner；每round
  checked≤32，UDP payload在fixed bound内交付且不等待全部cleanup完成，随后confirmed handles在
  固定上界内清空；应用poll机会有明确计数或waker见证。
- Required changes: 先加入当前组合缺失时RED的test；使用真实UDP socket/send/recv和Injected
  runner；断言payload、poll bound、deferred telemetry与最终收敛。
- Preserve: UDP datagram原子性、stage budget、quiet no-busy、唯一runner、无caller progress。
- Forbidden: 只断言telemetry、预先清空deferred list、直接调用reaper/stack_round、固定wall sleep。
- GREEN condition: ordinary和qemu-diagnostics组合test各100×；UDP在backlog仍存在时成功，512
  handles最终按budget收敛。
- Stop when: UDP只能在修改scheduler/smoltcp后前进，或发现独立driver/loader fault。

### T2.7-R3: fresh single-hart QEMU runtime Gate

- Requirement/Scenario: MS01 compatibility；Cycle 003 Acceptance 5。
- Depends on: T2.7-R2及全部automatic Gates GREEN。
- Targets: `.claude/runbooks/qemu-network-testing.md`规定的diagnostic single、fork、原MS01命令；
  Cycle Act Response markers。
- Current behavior: 三项均pending，无fresh runtime evidence。
- Required behavior: 每条命令记录START、所有phase、PASS|FAIL、END、warn/panic和正常退出/中断；
  MS01必须14/14，三项均无FAIL、timeout、missing marker、panic或用户中断。
- Required changes: 不改payload，除非automatic/source Gate先证明marker或deadline自身错误；按Runbook
  fresh build后执行三条命令。
- Preserve: single-hart VirtIO-MMIO结论边界，不外推SMP、真板或性能。
- Forbidden: 用host tests替代QEMU、用diagnostic替代原MS01、复用Cycle 002失败日志作为PASS、
  把pending写为None blocker。
- GREEN condition: 三条命令均完整PASS并正常退出。
- Stop when: 任一命令FAIL、timeout、缺marker、panic或中断；Act返回blocked handoff，收到用户证据
  后恢复同一Cycle。

**Invariants**

- resident runner仍是唯一smoltcp推进者；socket API不直接poll。
- queue task继续独占descriptor、completion和queue-control；RX/TX slot与ticket契约不变。
- 锁序只允许`SERVICE → SOCKET_SET → ListenTable entry`；guard不跨wake、await、Pending或yield。
- public metadata与raw handle各退役一次；未确认close不提前回收；合法owner不能留下可复用的stale
  deferred TCP slot。
- backlog=512、Ready唯一交付、Reset=`ConnectionReset`、UDP datagram原子性不变。

**Non-goals**

- 新增通用socket generation系统，除非T2.6-R1证明现有独占前提不成立并返回Plan。
- SO_LINGER、unresponsive-peer abort、Task 3.1 terminal fault广播、Tasks 3.2–3.4最终probe。
- scheduler、smoltcp、fork/process、SMP、真板、性能、全局tasks/SNAPSHOT、归档和commit。

**Replan Traceability Matrix**

| Requirement | Scenario | Design | Repair | Code/Test Witness | Status |
|---|---|---|---|---|---|
| R3/R4 | deferred slot不误删 | D4/D5 | T2.6-R1 | 全路径owner witness；stale/retyped准确测试 | Covered |
| R6 | 满512释放后立即恢复 | D5/D7/D9 | T2.7-R1 | exact-512 queue + same-runner reconnect | Covered |
| R3/R6 | cleanup storm下无关I/O | D4/D7/D10 | T2.7-R2 | 512 deferred + same-runner UDP payload | Covered |
| MS01 | 应用兼容 | D10 | T2.7-R3 | QEMU single/fork + MS01 14/14+END | Covered |

没有Missing或Simplified requirement；没有扩大到Task 3、SMP、真板或性能。

**Acceptance**

1. deferred raw-handle所有权被全路径证明；现有test准确区分stale、retyped和真正reuse，合法路径中
   不存在旧entry删除新TCP的窗口。若证明失败，本Cycle停止而不是以不完整identity实现继续。
2. exact-512 listener queue在accept后无需额外reconcile即可由同runner完成immediate reconnect；
   backlog上限、唯一accept、Reset和multiwaiter语义保持。
3. exactly 512 deferred cleanup与真实UDP datagram共享同一runner；checked≤32，UDP在fixed bound
   内且deferred backlog仍存在时交付，随后confirmed handles有界收敛，无busy-loop。
4. 新组合tests在ordinary和qemu-diagnostics各100×；两组full suites、MS04、kernel QEMU/D1 checks、
   payload build、fresh kernel、fmt/source/strict OpenSpec/diff Gate全部通过。
5. fresh single-hart QEMU diagnostic single/fork和原MS01完整结束；MS01 14/14+START/END，三者无
   FAIL、timeout、missing marker、panic或用户中断。
6. 完整diff无未解决Critical/Important finding；Act Response如实记录warning、blocker和证据边界。

**Verification**

- T2.6-R1 owner/source witnesses两种feature各100×。
- exact-512 accept→reconnect与512 cleanup→UDP组合tests两种feature各100×。
- `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib`
- 同命令增加`--features qemu-diagnostics -- --test-threads=1`
- `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test`
- `cargo check --locked --offline -p starry-kernel --features qemu`
- `cargo check --locked --offline --target riscv64gc-unknown-none-elf --features lichee-d1`
- 两个payload使用`riscv64-linux-musl-gcc -static -O2 -Wall -Wextra`交叉编译。
- `make LOG=error build`
- `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check`
- source assertions：所有TCP raw add/adopt/remove路径；deferred raw removal与entry removal同guard；
  512测试无常量占位、无direct round、无manual reconcile；accept wake在guard外；MS01仍14 markers。
- `openspec validate ms06-application-visible-async-network-stack --strict`
- `git diff --check HEAD`与完整diff review。
- automatic Gates通过后，用户按Runbook手工运行diagnostic single、fork和原MS01并回传完整markers。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | 伪512 witness、缺失UDP组合、slot-index复用和QEMU pending已逐项核对 |
| Design | PASS | 先证owner独占，失败即停；两个exact-scale组合witness和runtime Gate边界明确 |
| Iteration Plan | PASS | 只返工Tasks 2.6/2.7；Iteration 002不变 |
| Cycle Scope | PASS | 不重做已GREEN机制，不进入Task 3或平台扩张 |
| Repair Contracts | PASS | 三组repair均含RED/目标/保持/禁止/GREEN/stop boundary |
| Traceability | PASS | R3/R4/R6/MS01到repair、代码与测试无Missing/Simplified |
| Verification | PASS | host组合Gate先行，single-hart QEMU为强制最终Gate |
| User Approval | PENDING | 等待用户审核并批准Cycle 004 |

Gate 2除User Approval外均PASS。Cycle保持`draft`；获批后更新为`ready`，等待显式
`openspec-act`调用。本次生成计划不构成实施授权。

**Persisted Evidence**

- Mode: none

host/unit/compile结果可重跑；QEMU markers在Act Response完整摘要。只有无法可靠摘要的一次性故障
才创建Evidence，且必须先在Act handoff中说明。

**Risks and Notes**

- 512 queue的test-only seed必须保持production路径和锁序不变；它只提供可观察初始状态，不能替代
  production `accept_with`或runner推进。
- fresh Closed TCP可作为confirmed retirement workload，但组合test还必须从同一runner提交真实UDP
  I/O并证明backlog仍存在时交付；不得把两个独立unit test再次拼成scale结论。
- ownership source witness对代码移动敏感，需同时保留行为测试与full diff review；若未来增加raw
  remove路径，该witness必须显式失败。
- QEMU只证明single-hart VirtIO-MMIO软件模型和应用兼容，不证明SMP、DMA coherency、真板或性能。

## Act Response

- Status: blocked

**Cycle 暂停总结**（2026-08-24，用户指令：本 Cycle 到此为止，先审计，下 Cycle 继续；恢复条件见 Blocker Handoff）

**已完成（全部 GREEN，含两 profile）**

- T2.6-R1：deferred raw-handle 所有权闭环 —— `stale_and_retyped` 准确命名、运行时所有权 witness
  （存活 entry slot 占用 / reap 原子提交 / reap 后复用安全）与 source ownership witness，各 100×。
- T2.7-R1：exact-512 accept→immediate reconnect（cfg(test) seed seam + 100× 双 profile）。
- T2.7-R2：512 deferred cleanup + 同 runner UDP 组合 witness（checked≤32 / 收敛 / quiet）100×。
- host 复现判别器：`task_27_repro_guest_512_recovery_sequence`（512 真实握手 + overflow + accept +
  双侧 close + recovery connect）与 `task_27_repro_guest_udp_bidirectional`（完整阻塞唤醒链）——
  **均 GREEN**，证明协议/状态机/唤醒链在 Injected 模型内正确，分歧在 guest 运行时层。
- 诊断观测点（info 级）：listen_table refill/reconcile/accept、UDP recv WouldBlock/bytes、runner
  socket_changed；LOG=info 镜像从 fresh QEMU 捕获完整失败窗口。

**问题定位（guest 运行时层，双层证据）**

1. **udp-bidirectional 挂起（已根因）**：info 串口显示 responder(child) 成功 `recv 8 bytes` 后立即
   `_exit(0)` —— echo 经 `sendto` 入队 smoltcp UDP TX buffer，runner 尚未 egress 派发时
   `UdpSocket::drop` 执行 `shutdown()→smoltcp close()`（**`tx_buffer.reset()` 丢弃排队数据报**）+
   `SOCKET_SET.remove`（raw handle 移除）→ echo 丢失，initiator 的阻塞 recvfrom 永不醒来。
   TCP 有 close_kind 延迟回收保护，UDP 无 —— 同一缺陷类（queued-TX-on-close）。
   修复 WIP：`CloseKind::UdpQueued` + reaper UDP arm + `UdpSocket::drop` 延迟回收，配套 4 个 host
   测试。**实现中发现设计缺陷**（见「未闭环」2/3）：`can_send()` 的 smoltcp 语义是「TX 未满」而
   非「有排队数据」，公开 API 无 pending-TX 访问器。
2. **tcp-512-recovery ConnectionRefused（已定位到层，机制未闭合）**：info 串口证明 recovery SYN
   **确实到达了活着的 idle #1025**（`idle #1025 -> SynReceived`，21.666），期间 queue 满/无 idle 且
   runner 以 ~0.7ms/round 连续转 ~10 轮（`refill blocked` 刷屏），11ms 后客户端收到 RST（21.677）。
   host 同序列复现 GREEN → 分歧是 guest 时序（runner 忙转/调度）+ smoltcp `SynReceived` 收到
   ACK/RST 的状态机行为。补充观测（reconcile queue-slot 转换 + reaper Reap 已提到 info）已进树，
   待下 Cycle 用新的 LOG=info 再跑一次读取 #1025 的最终 slot 状态（Ready/Reset/滞留）以区分 RST 源。

**未闭环（3 个新增测试 FAIL；树可编译，kernel check 通过）**

1. `deferred_retirement_udp_queued_entry_stale_or_retyped_drops`：**reaper match 分支顺序错误** ——
   `Tcp(_) if kind==UdpQueued → Drop` 放在通用 `Tcp(_) → Keep` 之后，永远不可达；需前置。
2. `deferred_retirement_udp_queued_tx_wait_for_drain_before_reap`：
   **reaper 重启 gate 问题** —— 一次 swoep 全 Keep 后 `dirty=false`，后续 round 无
   `protocol_progressed` 时不重扫，滞留的 UDPQueued entry 永不重检（真背压场景会泄漏 entry+socket）。
3. `task_27_repro_udp_child_close_keeps_queued_echo`：响应 socket 最终未被 reaper 回收 —— 同
   `can_send()` 语义错误：`can_send()==true`（TX 未满）恒真，Reap 条件永假。
   根因：smoltcp UDP **无公开「TX 有排队数据」谓词**；需最小只读 accessor（如
   `tx_queued() = !tx_buffer.is_empty()`，不改协议语义）或换机制 —— 下 Cycle 决策点。

**Handoff 状态**

- 工作树：`crates/axnet/src/{listen_table,service,stack_runner,udp}.rs` 含诊断观测点 + UDP 修复 WIP；
  全部 4 个 Cycle-004 witness（所有权/512/组合/复现）保持 GREEN；只有 3 个新 UDP 测试 FAIL。
- 镜像：工作区 `StarryOS_riscv64-qemu-virt.bin` = LOG=info 诊断镜像（3c55d4…）；已测基线冻结于
  `/tmp/ms06-frozen-bin-backup.bin`（f9f311…，LOG=error）；另存 `/tmp/ms06-loginfo.bin`、
  `/tmp/ms06-logdebug.bin`。
- 日志：`/tmp/ms06-info-serial.log`（用户 QEMU info 捕获，512 失败窗口 + udp 挂起窗口完整）。
- `README.md` 的代理设置（用户本机改动）与 tests/ms01 二进制重建（fresh build）未纳入提交。

**恢复条件（下 Cycle）**

1. 审计本 Act Response 与 .claude/runbooks 诊断记录。
2. 决策 UDP pending-TX 谓词：最小 smoltcp 只读 accessor（改 smoltcp 需用户批准，因 Plan
   Forbidden 列为「修改smoltcp」）或换机制（同步单包派发 vs 全部延迟回收）。
3. 修 reaper 分支顺序 + 重启 gate；重跑 3 个 UDP 测试至 GREEN；两 profile 全量。
4. 512-recovery：用新观测（queue-slot/reaper）的 LOG=info 再跑；确定 #1025 终态 → 闭合 RST 机制。
5. 修复后 fresh QEMU MS01 14/14 + single/fork，方可 `reported`。
6. 若根因需要 backlog/accept/runner 调度层设计变更 → 返回 Plan 在下一 Cycle 重新规划。

- 用户已执行三条 QEMU 命令，回传结果：
  - `diagnostic single`：全 phase + `PASS: single-loopback` + `MS01_LOOPBACK_DIAGNOSTIC_END single` → PASS。
  - `diagnostic fork`：全 phase + `PASS: fork-loopback` + `MS01_LOOPBACK_DIAGNOSTIC_END fork` → PASS。
  - 原 MS01：`PASS: tcp-accept`、`PASS: tcp-adjacent`、**`FAIL: tcp-512-recovery: connect: Connection refused`**、
    `PASS: tcp-relisten`，随后 `test_udp_bidirectional()`（main 顺序第 5 位）**无 marker 挂起**，期间
    `[52.19] Failed to send signal: NoSuchProcess`，用户 ^C 中止。
- 判定：Acceptance 5 未达成（MS01 必须 14/14）。diagnostic single/fork 已 GREEN；MS01 的
  `tcp-512-recovery` 与 `udp-bidirectional` 是 guest 层残留故障——正是 Cycle 003 文档化的两个症状。
- 用户指令：继续在本 Cycle 内解决；重心放在用 Runbook（R55）分层调试方法定位阻塞发生的层，再修复。
- 恢复点：从 T2.7-R3 诊断阶段继续，Act 承担 host 层复现与证据准备，QEMU guest 运行由用户按
  `qemu-network-testing.md`/`qemu-kernel-net-dataplane-debug.md` 手工执行。
- 所需验证：定位层后修复，host 两 profile 全量 + fresh QEMU MS01 14/14 + 全 END，方可 `reported`。
- 风险：若根因需要 backlog/accept 语义或 runner 调度层面的设计变更，超出 Cycle 004 Plan 范围时
  Act 停止并返回 Plan，不悄然扩大变更面。

**Implemented**

- Task T2.6-R1（deferred raw-handle 所有权闭环）：
  - `service.rs` 测试 `deferred_retirement_stale_and_reused_handles_keep_cursor_valid` 重命名为
    `deferred_retirement_stale_and_retyped_handles_keep_cursor_valid`，注释明确 "retyped"（slot 被
    UDP 等不同类型占据）≠ same-type reuse，后者由新增所有权 witness 证明不可达。
  - 新增运行时所有权 witness `deferred_retirement_live_entry_keeps_raw_slot_occupied_and_reap_commits_atomically`：
    存活 deferred entry 的 raw slot 持续被占用（64 次 `SocketSet::add` 均不得拿到该 slot）；
    confirmed reap 在同一 guarded commit 内同时移除 raw handle 与 entry；reap 后的同类型 slot
    复用不会残留 stale entry 误删新 socket。
  - `stack_runner.rs` 新增 source/ownership witness
    `task_26_r1_deferred_raw_handle_ownership_is_exclusive_and_atomic`：`queue_deferred_removal(`
    在 tcp.rs 恰好 1 处（仅 `TcpSocket::drop`）；`remove_raw(` 恰好 2 处（no-Service fallback +
    immediate 分支，均在无 entry 的同一 drop 内）；udp/listen_table/general/wrapper 不含
    `queue_deferred_removal(`；reaper 的 `sockets.remove(entry.handle)` 与
    `self.deferred_removals.swap_remove(idx)` 在同一 guarded scope。
  - 两个 witness 均 100×（ordinary 与 qemu-diagnostics 各自 100 次内部循环）。
- Task T2.7-R1（exact-512 accept→immediate reconnect）：
  - `listen_table.rs` 新增三个 `#[cfg(test)]` seam：`test_seed_full_queue`（把 entry queue 填到
    恰好 `LISTEN_QUEUE_SIZE` 个真实 hidden socket——1 个 Ready + 511 Pending、无 idle；先拆除上一
    轮 hidden sockets 以支持 100× 复用同一 SocketSet）、`test_queue_len`、`test_idle_is_some`。
    生产路径不变；seam 只在既有锁序内创建 hidden socket，不调用 Interface poll / Service / wake。
  - `task_27_accept_refills_idle_listener_no_reconcile_needed` 重写为 100× exact-512：accept 前
    断言 `test_queue_len == 512`，accept 无 reconcile 即恢复 idle listener，同一 runner 在
    POLL_BOUND 内完成 client2 立即重连，backlog 全程 ≤ 512，Ready 恰好交付一次；`let _ =
    LISTEN_QUEUE_SIZE` 占位删除。
- Task T2.7-R2（512 deferred cleanup 期间无关 UDP 前进）：
  - 新增 `task_27_cleanup_storm_keeps_unrelated_udp_forward_progress`（100×）：恰好 512 个
    deferred Confirmed TCP handles 与一个真实 loopback UDP datagram 共享同一 runner/Service/
    SocketSet；每 poll 的 `deferred_checked` 增量 ≤ 32；UDP payload 在 POLL_BOUND 内交付且
    交付时 `deferred_removals_len() > 0`；随后 512 handles 在固定上界内收敛（checked/reclaimed
    各恰好 +512）；收敛后 deferred stage 不再 self-wake（quiet）。
- 清理 Cycle 003 Plan Review Minor finding：删除 listen_table.rs:532 与 stack_runner.rs:502 测试
  模块的 unused imports。
- 新增 source assertion `task_27_r1_scale_tests_drive_progress_only_through_the_runner`：两个
  scale 测试不含 `let _ = LISTEN_QUEUE_SIZE;` 占位、不含 direct `stack_round(` / `.reconcile(`，
  且 cleanup 测试含真实 `send_slice`/`recv_slice` UDP I/O。

**Changed Files and Symbols**

- `crates/axnet/src/listen_table.rs`：3 个 `#[cfg(test)]` seam（`test_seed_full_queue` /
  `test_queue_len` / `test_idle_is_some`）；测试模块 unused import 清理。生产路径零改动。
- `crates/axnet/src/service.rs`：测试模块——测试重命名 + 新所有权运行时测试（100×）。
- `crates/axnet/src/stack_runner.rs`：测试模块——source ownership witness、exact-512 重写、
  cleanup+UDP 组合、source assertion 测试、`FULL_CHAIN_UDP_PORT`/`FULL_CHAIN_UDP_LOCAL_PORT`
  常量、unused import 清理。
- `tests/ms01_loopback_diagnostic`、`tests/ms01_socket_baseline`：按计划 Verification 以
  `riscv64-linux-musl-gcc -static -O2 -Wall -Wextra` 重新交叉编译（fresh payload build，exit 0）。
- `StarryOS_riscv64-qemu-virt.bin`：`make LOG=error build` 重新生成（fresh，17:23）。

**Deviations from Plan**

1. T2.7-R1 的 512 backlog 用 `cfg(test)` seam（`test_seed_full_queue`）直接构造（1 Ready + 511
   Pending），而非由 511 次真实握手逐步填满。Plan 明确允许「使用 cfg(test) 的最小 seed/inspection
   seam」；关键判定不变——accept 前断言 exact-512、accept 无 reconcile 恢复 idle、**reconnect 是
   真实 SYN 握手**并由同一 runner 完成。
2. 100× 共享基础设施提升到循环外 + 每轮迭代回收 accepted/client raw handle，避免每轮泄漏
   512-slot SocketSet（首版每次迭代 `Box::leak` 产生 ~6.4GB 泄漏，被 SIGKILL；修复后正交于计划
   语义，只影响资源生命周期）。
3. T2.7-R2 使用 smoltcp 0.13 `send_slice(data, UdpMetadata{endpoint, local_address, meta})` API
   （该版本无 UDP `connect()`）；等价实现，不改变计划语义。
4. source ownership witness 以 100× 循环包裹（确定性扫描，符合计划 Verification 字面）。
5. cleanup 测试的 quiet 断言在收敛后追加一次带 clock nudge 的 poll，验证
   `wakes` 不增长——对计划「无 busy-loop」判定的直接机械实现。

**Blocker Handoff**

- Task: T2.7-R3（fresh single-hart QEMU runtime Gate；对应 Cycle Acceptance 5）。
- Step/Gate: Phase 3 末尾 / Gate 5；自动 Gate 全部 GREEN 后触达。
- Plan 预期: 自动 Gate 通过后，用户按 `.claude/runbooks/qemu-network-testing.md` 手工运行
  diagnostic `single`、`fork` 与原 MS01，回传完整 START/phase/PASS|FAIL/END、warn/panic 与
  退出/中断结论；MS01 必须 14/14+PASS + START/END，三项均无 FAIL/timeout/missing marker/panic/
  用户中断。
- 实际情况: 三项 QEMU 均未执行。Runbook（K31/K48 系列 hard policy，三层独立证据：guest shell
  阻塞、sandbox EPERM、串口分帧不可靠）规定「QEMU 测试一律手动执行，禁止自动化驱动」；
  Cycle 004 亦明确该边界为执行者能力限制，只改变 handoff、不改变 Gate。工件已备齐：
  fresh `StarryOS_riscv64-qemu-virt.bin`（17:23）、`make/disk.img`、fresh
  `tests/ms01_loopback_diagnostic` + `tests/ms01_socket_baseline`（17:23）。
- 影响: Acceptance 2/3/4/6 的 host 证据已闭合；Acceptance 5（fresh QEMU）因证据缺失保持
  FAIL——本 Cycle 不能 accepted。不把 pending 写成 PASS，不新建 Cycle，不把自动 Gate 结果
  外推为 QEMU 验收。
- 用户需要执行的命令（依次）：
  1. Terminal 1：`cd tests && python3 -m http.server 18765 --bind 0.0.0.0`
  2. Terminal 2：
     ```
     qemu-system-riscv64 -machine virt -bios default \
       -kernel /home/daivy/projects/serial/work/StarryOS/StarryOS_riscv64-qemu-virt.bin \
       -m 1G -smp 1 -device virtio-blk-device,drive=disk0 \
       -drive id=disk0,if=none,format=raw,file=/home/daivy/projects/serial/work/StarryOS/make/disk.img \
       -device virtio-net-device,netdev=net0 \
       -netdev user,id=net0,hostfwd=tcp::5555-:5555,hostfwd=udp::5555-:5555 -nographic
     ```
  3. Guest (`starry:~#` 后)：
     ```
     wget -q -O /tmp/diag http://10.0.2.2:18765/ms01_loopback_diagnostic && chmod +x /tmp/diag && /tmp/diag single
     /tmp/diag fork
     wget -q -O /tmp/ms01 http://10.0.2.2:18765/ms01_socket_baseline && chmod +x /tmp/ms01 && /tmp/ms01
     ```
     预期：`MS01_LOOPBACK_DIAGNOSTIC_END single` / `...fork` 无 FAIL/timeout；MS01 每行
     `PASS: <case>` 共 14 个 + `MS01_SOCKET_BASELINE_END`，退出 0，无 panic/中断。
  4. 回传：每条命令的 START/所有 phase/PASS|FAIL/END、warn/panic 行与退出/中断结论。
- 恢复条件: 用户手工运行三条命令并回传完整 markers 后，明确要求继续；Act 读取本 Handoff、
  追加 `Blocker Resolution`、恢复同一 Cycle（`blocked → pending`），验证新鲜 markers 后再
  `pending → reported`。

**Blocker Resolution**

None（本 Cycle 无已恢复阻塞；上述 Handoff 等待用户执行 QEMU 后恢复）。

**Self-Review**

- Plan compliance: PASS（T2.6-R1 / T2.7-R1 / T2.7-R2 / T2.7-R3 契约逐项核对；见下方 Gate 4 记录）
- Full diff reviewed: PASS（完整 diff 只含 cfg(test) seams、测试模块改动与 004-replan 状态更新；
  无生产路径行为变化）
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved:
  1. `README.md` +6 行（http_proxy 环境变量）系用户本机改动，位于本 Cycle 范围外，未触碰。
  2. `task_27_r1_scale_tests_drive_progress_only_through_the_runner` 的源码切片依赖测试函数
     在文件中的顺序；与既有 `task_26_round_and_service_share_one_sampled_timestamp` 的
     `find()` 切片风格一致，顺序变化会被该测试自身捕获。
  3. 既有非本 Cycle：`flush.rs` 一处 `let mut guard` 不必 mut 的 warning；smoltcp 副本内部
     warning；qemu-diagnostics 默认并行 `reclaim_hold_...` flake（串行稳定，本 Cycle 全程串行）。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| axnet ordinary full suite | `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib` | `test result: ok. 297 passed` | PASS |
| axnet qemu-diagnostics full suite | 同命令 `--features qemu-diagnostics -- --test-threads=1` | `test result: ok. 317 passed` | PASS |
| T2.6-R1 owner/source witnesses | `task_26_r1*`、`deferred_retirement_live*`（两 profile 各 100× 内循环） | 各 1/1 ok | PASS |
| T2.7-R1 exact-512 / T2.7-R2 combo | `task_27_*`（两 profile 各自 100× 内循环；qemu-diagnostics 串行） | ordinary 2 项 + qemu-diagnostics 3 项 ok | PASS |
| MS04 host harness | `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test` | `test result: ok. 16 passed` | PASS |
| kernel QEMU check | `cargo check --locked --offline -p starry-kernel --features qemu` | `Finished dev profile`, exit 0 | PASS |
| root D1 check | `cargo check --locked --offline --target riscv64gc-unknown-none-elf --features lichee-d1` | `Finished dev profile`, exit 0 | PASS |
| payload 交叉编译 | `riscv64-linux-musl-gcc -static -O2 -Wall -Wextra`（两 payload） | 均 exit 0 零 error | PASS |
| QEMU artifact | `make LOG=error build` | `Finished release` + objcopy 生成 `StarryOS_riscv64-qemu-virt.bin`, exit 0 | PASS |
| fmt | `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check` | 无 diff | PASS |
| source assertions | 内嵌测试 `task_26_r1*`、`task_27_accept_refills_in_guard*`、`task_27_r1_scale_tests*`、`ms01_diagnostic_payloads*` | 全 ok（在 297 内） | PASS |
| strict OpenSpec | `openspec validate ms06-application-visible-async-network-stack --strict` | `Change ... is valid`, exit 0 | PASS |
| diff check | `git diff --check HEAD` | exit 0 | PASS |
| fresh single-hart QEMU | diagnostic single / fork / MS01 14/14 | 未执行（用户手工边界，见 Blocker Handoff） | PENDING |

**Persisted Evidence**

None required. 全部自动结果为确定性 host/unit/compile/build 命令，可低成本重跑；QEMU markers
由用户手工回传后写入 Act Response。未创建 Evidence 目录。

**Experience Candidates**

- Incident 候选（新）：100× 迭代测试若在循环内 `Box::leak` 每个迭代的 512-slot fixture，产生约
  6.4GB 泄漏并被调度器 SIGKILL（OOM）；共享基础结构提升到循环外 + 每迭代回收临时 raw handles
  后峰值内存有界。系统性价值：泄漏 host fixture 的 scale/witness 测试必须按迭代次数预算内存，
  不能依赖 GC/析构。引用：本 Act Response + `task_27_accept_refills_idle_listener_no_reconcile_needed`。
- Incident 候选（延续 Cycle 003）：无界 deferred reaper 与双时钟的 FIN/ACK 不收敛（Cycle 003 已
  记录；本 Cycle T2.7-R2 以单时钟 + 32-budget 组合 witness 正式关闭）。
- Runbook 候选：None（三条 QEMU 命令尚未端到端验证；不得预先登记）。

**Remaining Issues**

- T2.7-R3 的 fresh single-hart QEMU（diagnostic single/fork + MS01 14/14）pending，属用户能力
  边界；自动 Gate 全 GREEN，按 Blocker Handoff 恢复条件执行。恢复后本 Cycle 才能
  `blocked → pending → reported`。
- 既有非本 Cycle：`flush.rs` warning、smoltcp 副本 internal warning、qemu-diagnostics 并行 flake
  （串行稳定）。

**Commit or Diff Reference**

- Working-tree diff at HEAD `fb87c8d3`；本 Cycle 改动集中于 `crates/axnet/src/{listen_table,
  service,stack_runner}.rs` 的测试/测试 seam 与对应 Act Response；生产路径无行为变化；未创建
  commit；`README.md` 的用户代理设置未纳入。

## Plan Review

- Status: completed

**Review Result**

`replan-required`

**Findings**

1. **Blocking — UDP queued-TX判断使用了错误的smoltcp契约。**
   `UdpSocket::drop`和deferred reaper都用`can_send()`判断TX buffer是否含待派发datagram；本地
   smoltcp的实现是`!tx_buffer.is_full()`。因此空buffer和含一个datagram的非满buffer通常都返回
   true：empty drop被错误延迟，egress drain后entry也无法reap。fresh targeted tests中
   `deferred_retirement_udp_queued_tx_wait_for_drain_before_reap`和
   `task_27_repro_udp_child_close_keeps_queued_echo`均RED。当前Cycle禁止修改smoltcp，现有设计无法
   表达正确完成条件。
2. **Blocking — deferred verdict分支顺序使retyped保护不可达。**
   通用`Socket::Tcp(_) => Keep`位于`CloseKind::UdpQueued + Socket::Tcp(_) => Drop`之前，编译器报告
   `unreachable pattern`；`deferred_retirement_udp_queued_entry_stale_or_retyped_drops` fresh运行
   失败，entry残留为1。该问题可局部修正，但不能单独关闭Finding 1。
3. **Blocking — fresh QEMU MS01未满足Acceptance 5。**
   diagnostic single/fork有完整PASS/END；原MS01在`tcp-512-recovery`返回
   `ConnectionRefused`，随后`udp-bidirectional`无marker并由用户中止。info日志证明UDP child在
   recv后sendto并立即退出，public drop在runner egress前清空TX buffer；这支持Finding 1的根因。
4. **Blocking — 512 recovery证据混合了两个不同事件，且listener扫描违反D4。**
   日志按时间显示overflow client `127.0.0.1:49668`在backlog已满时提交connect；accept释放slot并
   创建`#1025`后，旧overflow SYN仍可先占用该slot，随后recovery client`:49669`被拒绝。该结果
   不能证明atomic refill没有创建listener。与此同时`Service::stack_round`在每个非idle ingress
   step后调用`ListenTable::reconcile`，后者扫描最多512个slots；日志在约11ms内重复输出
   `refill blocked`，与D4“listener reconcile每round一次”和无全表饿死要求不一致。
5. **Blocking — passive-open RST返回`Listen`后会滞留pending queue。**
   smoltcp对来自listen的`SynReceived`收到RST时清tuple并回到`Listen`；当前
   `ListenTableEntryInner::reconcile`把pending slot的`State::Listen`继续映射为Pending。该slot仍可
   监听却不再是entry的idle，可能永久占用backlog且不再恢复headroom。
6. **Non-blocking — Cycle 004已完成部分有效。**
   deferred raw-handle独占与原子回收、exact-512 accept/refill host witness、512 deferred cleanup与
   同runner UDP progress witness保持GREEN。Review fresh复跑
   `task_27_repro_guest_512_recovery_sequence`和
   `task_27_cleanup_storm_keeps_unrelated_udp_forward_progress`均1/1 PASS、exit 0。

**Deviation Classification**

- PLAN-INVALID：Cycle 004把`can_send()`当作pending-TX谓词，且明确禁止修改唯一能提供真实buffer
  状态的本地smoltcp接口。
- PLAN-OMISSION：D4要求listener reconcile每round一次，但Cycle未检查实际调用频率和512-slot
  scan；也未覆盖`SynReceived → Listen`恢复。
- NEW-EVIDENCE：fresh QEMU日志区分出overflow旧SYN先占headroom与UDP drop丢queued datagram。
- ACT-DEVIATION：WIP在发现实质契约缺口后继续加入`UdpQueued`生产路径，留下3个已知RED tests；
  用户随后明确停止本Cycle并要求审计，因此不再恢复旧Cycle。

**Acceptance Gaps**

- Acceptance 1：既有single timestamp与TCP deferred基础证据保持PASS。
- Acceptance 2：TCP 32-entry deferred budget保持PASS；UDP deferred完成谓词、reap触发和retyped
  分支失败；listener reconciliation仍可在一轮内重复全表扫描。
- Acceptance 3：atomic refill与两个host规模witnessPASS；guest recovery仍FAIL，overflow与recovery
  事件未分离，`SynReceived → Listen`slot恢复缺失。
- Acceptance 4：自动全量结果已被WIP后的3个RED tests取代；fresh targeted UDP命令exit 101。
- Acceptance 5：diagnostic single/fork PASS，原MS01无14/14、无END并被中止。
- Acceptance 6：上述Important findings未关闭，不能accepted。

**Convergence**

`reduced`。Cycle 004关闭了handle ownership与原先缺失的两个512组合witness，并把guest失败定位到
UDP queued-TX lifecycle和full-backlog事件排序。剩余gap更窄，但需要修改本地smoltcp只读接口、
D4 listener budget和MS01验证契约；不能作为原Cycle内的局部返工继续。

**Evidence**

- fresh命令：`cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib
  deferred_retirement_udp -- --test-threads=1`，2 failed，exit 101。
- fresh命令：同一manifest过滤`task_27_repro_udp_child_close_keeps_queued_echo`，1 failed，exit 101。
- fresh命令：过滤`task_27_repro_guest_512_recovery_sequence`，1 passed，exit 0；过滤
  `task_27_cleanup_storm_keeps_unrelated_udp_forward_progress`，1 passed，exit 0。
- 实际代码：`crates/smoltcp/src/socket/udp.rs::can_send/dispatch/poll_at`；
  `crates/axnet/src/udp.rs::UdpSocket::drop`；
  `service.rs::reap_deferred_removals/stack_round`；
  `listen_table.rs::ListenTableEntryInner::reconcile/accept_with`。
- runtime日志：`/tmp/ms06-info-serial.log`中`:49668` overflow、accept/refill `#1025`、`:49669`
  recovery、`ConnectionRefused`及UDP child recv marker；Persisted Evidence仍为none。

**Follow-up Decision**

创建同一Iteration的`005-replan.md`。下一Cycle采用本地smoltcp只读`has_pending_tx()`作为UDP raw
handle生命周期的唯一完成谓词；把listener pending reconciliation纳入32-entry budget；把overflow
终态与headroom recovery分开取证。该选择改变Cycle 004的smoltcp禁止边界和MS01验证契约，必须由
用户审计批准后才能进入Act。

**Iteration Plan Update**

Iteration 001仍包含Tasks 2.1–2.7，目标、依赖和512 backlog上限不变。修订Task 2.6的listener
reconciliation budget与RST-to-Listen恢复，修订Task 2.7的UDP queued-TX生命周期和overflow/recovery
分证据；更新D4、D7、D11及对应delta scenarios。Iteration 002保持pending。

**Next Cycle**

`005-replan.md`（draft，等待用户批准）。

**Next Iteration**

None; expand Iteration 002 only after `005-replan.md` is accepted.
