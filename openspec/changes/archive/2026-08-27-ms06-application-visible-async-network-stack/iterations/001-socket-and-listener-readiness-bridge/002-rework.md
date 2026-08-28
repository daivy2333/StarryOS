# Iteration 001 / Cycle 002: Runner-Owned TCP Close Retirement and MS01 Closure

## Plan Context

- Status: ready
- Approval: approved by user on 2026-08-24（原话：“批准”）；ready for an explicit
  `openspec-act` invocation.
- Iteration: 001-socket-and-listener-readiness-bridge
- Cycle: 002-rework
- Cycle Type: rework
- Parent cycle: `001-rework.md`

**Iteration Scope**

- Change tasks: 2.1–2.5
- Depends on: Iteration 000 accepted
- Stable baseline: 产品 TCP/UDP/listener 不主动推进协议栈；resident stack runner 是唯一
  smoltcp 推进者；per-public-handle readiness、hidden listener accept 和普通 poll→I/O
  语义保持一致。
- Verification boundary: bridge/registry 生命周期、smoltcp one-shot rearm、listener hidden
  sockets、`SERVICE → SOCKET_SET` 锁序、post-commit wake、caller-driven progress 为零、
  TCP close payload/FIN/handle 回收和 MS01 14/14 single-hart QEMU 回归全部通过。
- Diagnostic boundary: 失败限制在 public TCP Drop、deferred handle retirement、runner
  round/定时器、smoltcp close 状态、wrapper metadata 回收或 MS01 guest close 路径。
- Deferred tasks: 3.1–3.4

**Cycle Scope**

- Trigger: rework-required
- Acceptance gaps: `001-rework.md` 的同步 close flush 重新引入反向锁序和 caller-driven
  `stack_round`；固定 12 轮无法等待 delayed FIN ACK；原 MS01 被中断，无 14/14 与 END。
- Repair items: T2.4-R2、T2.5-R2、T2.5-R3
- Inherited scope: R1/R2/R4–R6、Tasks 2.1–2.5、D1–D9、MS05 queue/slot/ticket/flush
  ownership，以及已通过的 full-chain、diagnostic single/fork 和 readiness 行为。
- Excluded scope: SO_LINGER 新契约、任意强制 abort/墙钟 close timeout、Task 3.1 terminal
  fault/ERR、Tasks 3.2–3.4 最终 MS06 probe、scheduler/fork 修改、SMP、真板、性能、全局文档
  维护、归档和 commit。

**Objective**

删除 public Drop 中的同步 stack progress，把已关闭 TCP handle 的协议推进和最终 raw socket
回收交给唯一 resident runner。close 调用者不得同时持有 Service 与 SocketSet，不等待固定
round，也不销毁未被 ACK 的 payload/FIN；runner 在协议状态证明本端 FIN 已被确认后回收
hidden raw handle。完成后，新镜像的 diagnostic single/fork 与原 MS01 必须完整结束，原
MS01 输出 14/14 PASS 和 END，且不再出现 close-flush bound warning。

**Background**

父 Cycle 通过 host full-chain 找到 guest fork 特有差异：child 在同一时间片执行
`send → close → _exit`，旧 Drop 立即移除 smoltcp handle，未派发 payload 随 buffer 丢失。
Act 在用户豁免后加入 12 轮同步 `flush_removal_tx`，使 guest fork payload PASS，但实际代码
先锁 SocketSet 再锁 Service，并从 Drop 调用 `Service::stack_round`。QEMU 每次 close 都稳定
耗尽 12 轮，因为 smoltcp 的 `send_queue` 只在收到 ACK 后出队，`FinWait1` 也要等 FIN ACK
才退出。该修复证明了 root cause，却不满足本 Iteration 的 ownership、锁序与 close 生命周期。

**Current Baseline**

- Branch: `net-k3`；HEAD: `fb87c8d36b7c62e8d7156598defa08bce0db32d4`；MS06 实现位于
  未提交工作树。
- `001-rework.md` Review Result 为 `rework-required`；Iteration Map 与 Tasks 2.1–2.5
  状态不变。
- Fresh Review：ordinary axnet 274/274、qemu-diagnostics serial 294/294、MS04 16/16、
  QEMU kernel、root D1、fmt、strict OpenSpec 与 diff check 均通过。
- Review sandbox 的 C compiler 被 `Bad system call` 拒绝；Act 已用相同源文件和
  `-Wall -Wextra` 零 warning 编译成功，当前两个 payload 是静态 RISC-V ELF。
- 最新手工 single-hart QEMU：diagnostic single PASS、fork PASS；每个连接 close 均输出
  `queued TX not flushed within bound`。MS01 的 tcp-accept、tcp-adjacent PASS 后被用户中断，
  缺剩余 12 个 PASS、END 与完整退出结果。
- Persisted Evidence 继续为 `none`；命令和 marker 可在 Act Response 内完整摘要。

**Current-State Evidence**

- `StackAccess::round` 的生产路径先锁 `SERVICE`，再锁 `SOCKET_SET.inner`，这是 Task 2.4
  已建立的全局顺序。
- `TcpSocket::drop` 当前先取得 `SOCKET_SET.inner`，再在 `flush_removal_tx` 参数求值时取得
  `service.lock()`；它与 runner 形成反向边，并在调用者上下文执行最多 12 个完整 round。
- `task_24_cutover_removed_socket_register_and_caller_driven_poll` 只拒绝
  `poll_interfaces`，没有拒绝 TCP/UDP/listener/wrapper 直接调用 `stack_round`，因此现有
  source Gate 对等价 caller-driven progress 漏检。
- `SocketSetWrapper::remove` 同时移除 readiness bridge、raw smoltcp handle 和 bound
  metadata。deferred close 需要把“public metadata 退役”与“runner 最终移除 raw handle”
  分开；public waiters 在 Drop 后立即 wake/recheck，raw socket 在协议完成前保持不可见。
- 本仓库实际依赖 `crates/smoltcp`。`Socket::close` 对 Established/SynReceived 立即进入
  `FinWait1`，对 CloseWait 进入 `LastAck`；此时 FIN 尚可能未 emit。
- smoltcp `send_queue()` 返回 `tx_buffer.len()`；buffer 只在 ingress 处理 ACK 时
  `dequeue_allocated`。固定数量的同一墙钟 round 不能替代 peer ACK 或 delayed-ACK timer。
- 对主动 close，`FinWait2` 表示本端 FIN 已被 ACK；simultaneous close 的安全完成状态是
  `TimeWait`/`Closed`。对 `LastAck`，安全完成状态是 `Closed`。`Closing` 仍表示本端 FIN
  未被确认，不可移除。
- runner 已根据 `iface.poll_at` arm protocol timer，并对 loopback `rx_ready` self-wake；
  deferred raw handle 保留后，ACK、delayed ACK 和 retransmit 仍由同一 runner 推进。

**Relevant Code**

| File / Symbol | Current Responsibility | Cycle Use |
|---|---|---|
| `tcp.rs::TcpSocket::drop` | shutdown、同步 flush、立即 remove | 改为 metadata 退役、deferred request、software publish；不 poll |
| `tcp.rs::flush_removal_tx` | 12 轮 caller-driven stack round | 删除产品路径与 helper |
| `service.rs::Service` / `stack_round` | 唯一 bounded round owner | 保存并在 round 后处理 deferred TCP removals |
| `wrapper.rs::SocketSetWrapper` | bridge/raw handle/bound metadata 同步销毁 | 分离 public metadata 退役与 raw handle 回收 |
| `stack_runner.rs::StackAccess::round` | `SERVICE → SOCKET_SET` 唯一推进路径 | 驱动 close 状态并完成安全回收；扩展 source/lock/full-chain tests |
| `crates/smoltcp/src/socket/tcp.rs` | TCP close、ACK、buffer 与 poll_at 语义 | 只读责任边界；禁止修改 |
| `tests/ms01_loopback_diagnostic.c` | single/fork fixed-deadline witness | 复验；只有证据证明 marker/期限缺陷时才改 |
| `tests/ms01_socket_baseline.c` | 原 14-marker socket compatibility | 完整手工 14/14 + END 验收 |

**Critical Path**

```text
public TcpSocket drop
  -> shutdown commits smoltcp close and releases SocketSet guard
  -> detach public readiness/bound metadata; wake leftover waiters once
  -> enqueue raw handle retirement under Service only
  -> release Service guard -> publish StackEvent -> return from Drop
  -> resident runner locks SERVICE -> SOCKET_SET
  -> bounded ingress/egress/dispatch + protocol timer progress
  -> peer receives payload and FIN; ACK re-enters through loopback/device RX
  -> active close: FinWait2|TimeWait|Closed
     last-ack close: Closed|TimeWait
  -> same runner removes raw handle exactly once before recomputing poll_at

guest acceptance:
  host ownership/close witness GREEN
    -> diagnostic single PASS
    -> diagnostic fork PASS without close-bound warning
    -> original MS01 14/14 + END
```

**Implementation Guidance**

Service 持有 deferred removal entries，避免创建第二个 runner 或全局 polling task。Drop 中
每次锁只承担一个责任：smoltcp close commit、public metadata 退役、Service queue publish；
不得同时持有 Service 与 SocketSet。Service 的 runner round 在 egress/Router dispatch 后、
计算下一 `poll_at` 前检查 deferred entries，并按 entry 的 close kind 与当前 smoltcp state
决定是否移除 raw handle。

主动 close 只有进入 `FinWait2`、`TimeWait` 或 `Closed` 才可回收；`LastAck`/simultaneous
close 只有进入已确认终态才可回收。Idle、SynSent、Listen cleanup 等没有已提交 payload/FIN
的 handle 保持既有立即移除行为。若 Service 尚未安装，不存在 resident runner，保留安全的
立即移除 fallback。queue 待处理时由既有 StackEvent、loopback self-wake 和 protocol
deadline 推进，不加固定 retry tick、busy loop 或任意 close timeout。

**Behavioral Change**

- `close`/Drop 对调用者仍同步返回，不等待 peer ACK；其 raw smoltcp socket 变为 runner
  内部持有的短期 deferred resource，public readiness 与 bound metadata 立即退役。
- payload、FIN 与 ACK 只由 resident runner 推进；Drop 不再改变 runner telemetry rounds，
  不再持 Service+SocketSet 组合 guard，也不输出固定 12 轮耗尽 warning。
- 对响应的 peer，runner 在协议状态证明本端 close 已确认后精确回收 handle；handle reuse
  不继承旧 bridge、bound metadata 或 deferred entry。
- 不新增 SO_LINGER、强制 abort 或 unresponsive-peer 墙钟策略。若当前 smoltcp 状态/定时器
  无法为 loopback close 提供可达终态，本 Cycle 停止返回 Plan，不猜测 timeout。

**Change Surface**

| Repair | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| T2.4-R2 | R1/R2/R4 / 唯一推进与锁序 | `tcp.rs`、`stack_runner.rs` | Drop 反向锁并直接 round | 删除 caller round；source+并发 witness 固化唯一 runner/正向锁序 |
| T2.5-R2 | R5/R6 / send-close 生命周期 | `service.rs`、`wrapper.rs`、`tcp.rs`、runner tests | metadata/raw 同步 remove | Service-owned deferred raw removal；payload+FIN+EOF+cleanup witness |
| T2.5-R3 | R6 / MS01 compatibility | diagnostic、MS01、QEMU artifact | single/fork PASS，MS01 中断 | 新镜像三条手工命令完整 PASS/END |

**Task Contracts**

### T2.4-R2: 恢复唯一 runner 与全局锁序

- Requirement/Scenario: R1、R2、R4；Task 2.4；父 Cycle Acceptance 2、4、5。
- Depends on: `001-rework.md` root-cause evidence。
- Targets: `tcp.rs::TcpSocket::drop`、删除 `flush_removal_tx` 产品路径、
  `stack_runner.rs` source/lock-order tests。
- Current behavior: Drop 在持有 SocketSet 时等待 Service，并直接运行完整 stack round；现有
  source test 因只检查 `poll_interfaces` 而误报 GREEN。
- Required behavior: TCP/UDP/listener/wrapper public path 不调用 `stack_round` 或等价 stack
  progress；Drop 从不同时持有 Service/SocketSet；所有组合 guard 只由 runner 按
  `SERVICE → SOCKET_SET` 获取。
- Required changes: 先加入当前代码必 RED 的 source witness，拒绝 public socket 模块中的
  direct `stack_round` 和 Drop 反向锁片段；再删除同步 flush，改为只提交 deferred request
  并 publish software work。增加 bounded 并发 lock witness，证明 runner 与 close 可完成。
- Preserve: public close 返回类型、readiness/error 语义、唯一 runner lifecycle、
  commit→unlock→wake、Active quiet 和 MS05 ownership。
- Forbidden: 通过重命名绕过 source guard、在 Drop/yield/task 中 poll、增加第二 runner、
  固定 sleep/retry、改 scheduler/smoltcp 或反转 runner 锁序。
- Test witness: 当前 `tcp.rs` 中 `flush_removal_tx(... service.stack_round ...)` 与
  SocketSet-before-Service 次序使新 source/lock test RED。
- GREEN condition: public socket模块 direct stack progress 调用点为零；并发 witness
  100×完成，无死锁；Drop 返回前 runner round telemetry 不增长。
- Verification: targeted source/lock/runner tests ordinary 与 qemu-diagnostics 各 100×，
  随后两组 full axnet suites。
- Stop when: 需要在 caller 中推进协议栈、需要改变 scheduler/Service ownership，或无法在
  不同时持两把锁的情况下提交 removal request。

### T2.5-R2: 用 runner-owned deferred retirement 完成 payload、FIN 与 handle 回收

- Requirement/Scenario: R2、R4–R6；Tasks 2.1、2.3–2.5；父 Cycle Acceptance 1–5。
- Depends on: T2.4-R2 GREEN。
- Targets: `service.rs::Service` deferred entries/round reaper；
  `wrapper.rs::SocketSetWrapper` public metadata 与 raw removal 分离；`tcp.rs` close request；
  `stack_runner.rs` full-chain close tests。
- Current behavior: wrapper 同时销毁 bridge、bound metadata 和 raw handle；同步 flush 在 ACK
  可发生前耗尽固定 rounds，然后强制 remove。
- Required behavior: public bridge/bound metadata 在 Drop 时退役并 wake 一次；需要 FIN/ACK
  的 raw handle 留在不可公开的 deferred set，由 runner 推进并在安全状态移除一次。主动
  close 的安全状态是 `FinWait2|TimeWait|Closed`；`LastAck`/simultaneous close 只在本端 FIN
  已确认的终态回收。Idle/SynSent/已清理 listener 不产生 orphan。
- Required changes: Service 保存去重的 deferred handle + close kind；stack round 在真实
  ingress/egress/dispatch 后回收已完成项，再计算 `poll_at`。wrapper 提供只退役 public
  metadata 和 runner-only raw removal seam；handle reuse 不继承旧状态。
- Preserve: public handle 不可在 Drop 后访问、leftover waiters wake、512 listener backlog、
  SocketSet handle唯一性、one-shot rearm、loopback/设备packet ownership和固定 stage budget。
- Forbidden: 固定 round count、等待 `send_queue()==0` 作为“已 emit”证据、移除
  `FinWait1|Closing|LastAck`、任意 wall-clock linger、强制 abort、保留可公开 bridge、修改
  smoltcp 或让测试强制写 TCP state。
- Test witness: 重写 `closing_socket_queued_tx_reaches_peer_before_removal`，当前实现应 RED：
  Drop 直接增加 round telemetry、raw handle过早移除且没有 runner-owned retirement。
- GREEN condition: 实际 runner 在固定最多 128 polls（允许 injected clock跳至
  `poll_at`）内交付唯一 payload；peer 读完后观察 EOF/FIN；client raw handle 只在安全状态
  后移除；bridge wake/metadata/raw removal 各一次；caller 运行零 round。ordinary 与
  qemu-diagnostics targeted 各100×无 hang或 warning。
- Verification: close full-chain、CloseWait/LastAck、Idle/Connecting cleanup、duplicate queue、
  handle reuse、bridge wake、runner park/deadline和lock tests，再运行full suites。
- Stop when: loopback peer响应后仍无协议deadline/安全终态、需要新增对外 close/linger 契约、
  deferred资源只能靠busy polling释放，或证据指向Task 3.1 terminal fault范围。

### T2.5-R3: 用完整手工 QEMU 证据关闭 MS01 Acceptance

- Requirement/Scenario: R6、network-stack-baseline compatibility、父 Cycle Acceptance 3。
- Depends on: T2.4-R2、T2.5-R2 与全部自动 Gate GREEN。
- Targets: 当前 diagnostic/MS01 payload、fresh QEMU artifact、Runbook 手工入口。
- Current behavior: 旧修复镜像的 single/fork PASS，但每次 close 有 bound warning；MS01
  只完成前两个 case 后中断，没有最终结果。
- Required behavior: 新镜像 diagnostic single/fork 均完整 PASS/END；串口不出现
  `queued TX not flushed within bound`、panic 或 deadlock；原 MS01 输出 14/14 PASS、START、
  END 与明确退出结果。
- Required changes: payload 源码只有自动 Gate 证明期限或 marker 错误时才修改；否则只重编
  payload、构建新 kernel artifact，并按 Runbook 由用户手工执行三条命令。
- Preserve: 13 cases、14个PASS名称/顺序、127.0.0.1路径、static RISC-V ELF、15s diagnostic
  边界、single-hart VirtIO-MMIO结论范围和禁止自动QEMU政策。
- Forbidden: 自动驱动QEMU、跳过MS01 case、用diagnostic替代14/14、忽略warn/FAIL/timeout/
  中断、复用旧kernel结果或在本Cycle根据guest新失败继续猜测修复。
- Test witness: `001-rework.md` 的完整manual结果：single/fork PASS，close warn，MS01无END。
- GREEN condition: 三条guest命令均完整结束且markers满足Required behavior；无FAIL、timeout、
  missing marker或user interruption。
- Verification: payload source checks/交叉编译、fresh `make build`，随后按
  `.claude/runbooks/qemu-network-testing.md`手工运行并记录决定性markers与退出结论。
- Stop when: 任一命令失败/超时/缺marker/中断，出现新close warning，或证据需要范围外契约；
  写Blocker Handoff并返回Plan，不开始新的同类盲试。

**Invariants**

- resident stack runner 是唯一 smoltcp 推进者；queue task仍独占descriptor、completion与
  queue-control。
- Service与SocketSet组合guard只按 `SERVICE → SOCKET_SET` 获取；任何guard不跨wake、await、
  Pending或timer arm。
- Drop只提交状态和事件；wake是提示，协议状态与下一次I/O/cleanup recheck决定结果。
- public metadata退役与raw handle回收各一次；deferred raw socket对应用不可达且不可复用。
- TCP short write、UDP datagram原子性、512 backlog、PollSet 64/65和MS05 ownership不变。
- QEMU结果只覆盖single-hart VirtIO-MMIO软件模型，不扩大到SMP、真板或性能。

**Non-goals**

- 完整POSIX SO_LINGER、unresponsive-peer强制abort/墙钟回收策略。
- Task 3.1 terminal ERR/fault广播、Tasks 3.2–3.4最终MS06 probe。
- scheduler、fork/process、signal/ldisc、smoltcp内部、reset、SMP、PCI/DWMAC、真板、性能。
- qemu-diagnostics默认并行既有flake、非阻塞warning/注释清理、全局文档、Evidence目录、
  Runbook/Incident、archive和commit。

**Repair Traceability**

| Requirement / Acceptance | Evidence Gap | Repair | Code Surface | Witness | Status |
|---|---|---|---|---|---|
| R1/R2/R4唯一runner与锁序 | Drop反向锁并direct round | T2.4-R2 | TCP Drop/runner source+locks | caller zero-round + 100×lock | Covered |
| R5/R6 close/cleanup一致 | fixed rounds不等于ACK/FIN完成 | T2.5-R2 | Service/wrapper/TCP/runner | payload+EOF+safe-state removal | Covered |
| Acceptance 3 / MS01 14/14 | manual MS01被中断 | T2.5-R3 | payloads+fresh artifact | single/fork+original complete markers | Covered |

没有 Missing 或 Simplified requirement；未修改原 Task、requirement、Acceptance 或
Iteration Map。

**Acceptance**

1. T2.4-R2：public socket路径direct stack progress调用点为零；Drop不同时持Service与
   SocketSet；runner/close并发100×无死锁，caller返回前runner round计数不变。
2. T2.5-R2：真实runner在bounded host witness中交付send→close payload与FIN/EOF；public
   metadata立即退役，raw handle仅在安全close状态后回收一次；无fixed-round warning、busy
   loop、第二runner或handle复用污染。
3. T2.5-R3：fresh single-hart QEMU的diagnostic single/fork与原MS01均完整结束；原MS01
   14/14 PASS + START/END；无close-bound warning、FAIL、timeout或missing marker。
4. ordinary/qemu-diagnostics axnet suites、targeted 100×、MS04 harness、QEMU/D1 checks、
   payload build、fmt/source/strict OpenSpec/diff Gate全部通过。
5. 完整diff无未解决Critical/Important finding；结果不扩大到Task 3或硬件/SMP声明。

**Verification**

- T2.4-R2 source/lock/caller-zero-round targeted tests在ordinary与qemu-diagnostics各100×。
- T2.5-R2 close full-chain targeted tests在两个feature set各100×；覆盖active close、
  CloseWait/LastAck、Idle/Connecting、duplicate queue、handle reuse和bridge wake。
- `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib`
- 同命令增加 `--features qemu-diagnostics -- --test-threads=1`
- `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test`
- `cargo check --locked --offline -p starry-kernel --features qemu`
- `cargo check --locked --offline --target riscv64gc-unknown-none-elf --features lichee-d1`
- 两个payload使用 `riscv64-linux-musl-gcc -static -O2 -Wall -Wextra`交叉编译；若sandbox以
  `Bad system call`拒绝，按环境边界记录并由用户在同工具链复跑，不得冒充产品PASS/FAIL。
- `make LOG=error build`
- source assertions：public TCP/UDP/listener/wrapper无direct `stack_round`/`poll_interfaces`；
  Drop无SocketSet→Service边；`flush_removal_tx`与bound warning字符串已删除；deferred raw
  removal仅在Service/runner路径；MS01仍有14个PASS marker。
- `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check`
- `openspec validate ms06-application-visible-async-network-stack --strict`
- `git diff --check HEAD`与完整diff review。
- 自动Gate全部通过后，用户按Runbook手工运行diagnostic `single`、`fork`、原MS01，记录
  START/phase/PASS|FAIL/END、warn/panic、退出或中断结论。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | 实际Drop/runner/wrapper锁与调用链、smoltcp close/ACK/buffer语义、host与manual QEMU证据已检查 |
| Design | PASS | public metadata退役与runner raw回收分离；safe-state、锁序、wake/timer和fallback边界明确 |
| Iteration Plan | PASS | 三项repair只关闭Iteration 001既有Acceptance；Tasks和Map不变，gap较父Cycle缩小 |
| Cycle Scope | PASS | T2.4-R2/T2.5-R2/T2.5-R3映射原Tasks 2.4–2.5；无SO_LINGER、Task 3或scheduler新目标 |
| Task Contracts | PASS | 每项含位置、RED witness、行为、保持/禁止、GREEN、验证和停止条件；Act无需回读父Cycle |
| Traceability | PASS | R1/R2/R4→唯一runner/锁序；R5/R6→close retirement；Acceptance 3→完整MS01；无Missing/Simplified |
| Verification | PASS | source+并发+full-chain+100×+compile+manual QEMU按依赖分层，missing marker不计PASS |
| User Approval | PASS | 用户于 2026-08-24 显式批准（原话：“批准”） |

**Persisted Evidence**

- Mode: none

host/unit/compile结果可低成本重跑；manual QEMU的决定性markers、命令、环境和退出状态可在
Act Response内摘要，不创建Evidence目录。若出现一次性环境或无法摘要的blocker，Act停止
返回Plan，不自行改变模式。

**Risks and Notes**

- deferred handle依赖peer ACK和既有protocol timer；本Cycle只承诺响应loopback peer的有界
  关闭。若证据要求unresponsive-peer强制回收策略，该选择会改变close契约，必须返回Plan。
- queue request与runner可能交错；实现必须以去重entry和当前TCP状态recheck关闭
  ready-before-register/handle reuse窗口，不能把一次wake当完成。
- public bridge在Drop时退役，raw handle稍后移除；任何按handle lookup的路径必须无法把旧
  bridge绑定到新socket。
- qemu-diagnostics默认并行flake是非阻塞Minor；本Cycle的确定性回归继续使用串行配置，
  不把该flake的修复夹带进close任务。
- 三次失败反思已完成：共享状态是smoltcp TX/FIN仍属于被立即remove的handle，耦合点是
  public Drop与runner ownership。下一次QEMU是机制修复后的验证，不是第四次盲试；失败即停。

## Act Response

- Status: blocked

**Implemented**

- T2.4-R2：`TcpSocket::drop` 不再运行同步 stack rounds，也不再同时持有 Service 与 SocketSet。删除 `flush_removal_tx` 及 `queued TX not flushed within bound` 告警；Drop 只做三件事：`shutdown` 提交关闭（SocketSet guard 内）→ `SOCKET_SET.retire_public` 退役 public bridge/bound 并唤醒遗留 waiter → 按 smoltcp 关闭态决定「Service-only 入队 deferred removal」或「立即 remove_raw」，最后 publish software work。锁序 witness：`drop_keeps_socket_set_and_service_guards_disjoint`（Drop 源码含 `let defer = { ... SOCKET_SET.inner.lock() ... }` 表达式块与独立 `queue_deferred_removal`，无 flush/stack_round）与 `task_24_r2_public_socket_paths_never_drive_stack_progress`（tcp/udp/listen_table/wrapper 无 direct `stack_round`/`poll_interfaces`）。
- T2.5-R2：Service 保存去重 deferred entries（`CloseKind::{Active,LastAck}` + `reap_deferred_removals`），`stack_round` 在 egress/Router dispatch 后、`poll_at` 重算前回收已确认 handle。`wrapper.rs` 拆分 `retire_public`（metadata+waiter wake）/`remove_raw`（runner-only）/`remove`（组合）。重写 `closing_socket_queued_tx_reaches_peer_before_removal`：见证 caller 零 round、close 后 raw handle 保留、payload 唯一交付、peer EOF、以及有界运行期 handle 不被提前回收。新增机制级 witness：`close_kind_decides_defer_vs_immediate_removal`、`close_kind_confirmation_matrix`、`deferred_close_reap_dedups_stale_and_confirmed_removal`、`stack_round_reaps_deferred_close_before_poll_at`、`drop_state_read_and_deferred_enqueue_compose_without_deadlock`（四角色 100× 无死锁）、`retire_public_keeps_raw_handle_and_remove_raw_removes_it`。
- T2.5-R3：两个 payload 重新交叉编译（`-Wall -Wextra` 零警告），`make LOG=error build` 生成含 deferred-retirement 的 fresh `StarryOS_riscv64-qemu-virt.bin`；手工 QEMU 三条命令已就绪待用户执行。

**Changed Files and Symbols**

- `crates/axnet/src/tcp.rs`：`TcpSocket::drop`、`close_kind`（新）；删除 `flush_removal_tx`。
- `crates/axnet/src/service.rs`：`CloseKind`、`DeferredRemoval`、`Service.deferred_removals`、`queue_deferred_removal`、`reap_deferred_removals`、`stack_round` 内 reap 接入。
- `crates/axnet/src/wrapper.rs`：`retire_public`、`remove_raw`、`remove` 组合。
- `crates/axnet/src/stack_runner.rs`：两个 source witness、close full-chain 重写、MS01 marker source test 维持。
- `tests/ms01_loopback_diagnostic`/`tests/ms01_socket_baseline`（二进制）已重新交叉编译。

**Deviations from Plan**

1. **T2.5-R2 运行期终态 witness 未收敛（本轮阻塞根因）**：重写的 full-chain 测试在 payload 交付、EOF、caller 零 round、handle 保留方面全部 GREEN，但 **loopback 优雅关闭无法在有界 poll 内到达确认终态（FinWait2）**：probe 显示 `iface.poll_at` 恒为 `Some(10000)`（peer delayed-ACK 期限），`now` 推进至 50000µs 仍不触发 ACK 发出/回灌（client `csend=8` 恒不变）。根因链：关闭方 socket 因「未 ACK 数据 + 窗口恒开」在每个 round 都满足 `seq_to_transmit`（`flight_size = remote_last_seq - local_seq_no = 0` → `cwnd_remaining` 恒满）→ `PollAt::Now` → `iface.poll_at` 映射为绝对 `Some(0)`；runner `arm_timer(deadline<=now)` 恒自唤醒；`Router::receive` 依赖 `!tx_buffer.is_full`，与 egress 重灌共用同一 tx buffer，peer 的 delayed-ACK 回复被饥饿。真实 guest 因墙钟推进（10ms 后 ACK 自然到期）可收敛；注入冻结时钟 harness 无法以有界轮次建模。**命中 Plan T2.5-R2 自身 Stop：`loopback peer 响应后仍无协议 deadline/安全终态…停止返回 Plan，不猜测 timeout`。**
2. **reaper 修复（新 witness 捕到产品缺陷）**：`reap_deferred_removals` 首版在「handle 已消失」分支仍调用 `sockets.remove`，smoltcp 对无效 handle panic（`deferred_close_reap_dedups_stale_and_confirmed_removal` 首跑 RED）。改为 `iter().find` 找不到时只移除 entry、不触碰 set。该修复叠加在 T2.5-R2 契约内，未扩大范围。
3. 执行器时钟模型：injected-clock 测试在 runner 自唤醒且无 parked timer（Now 期限）时必须推进注入时钟模拟墙钟；已在 close full-chain 的 bounded 阶段固化（`None if self_woke => now += 10ms`），与该冻结时钟发现一致并记录为遗留 Minor 的 harness 约束。

**Blocker Handoff**

- Task/step/Gate：T2.5-R2 的「实际 runner 在有界 poll 内使 client raw handle 在安全确认态后移除」运行期 witness（Acceptance 2 的确认态部分）；以及 T2.5-R3 的手工 single-hart QEMU 三命令（Acceptance 3，用户能力边界）。
- Plan 预期：deferred handle 依赖既有 protocol timer 与 peer ACK，在 loopback 有界关闭内可达 `FinWait2|TimeWait|Closed` 终态。
- 实际：(a) 注入 loopback 在有界轮次内无法完成 FIN/ACK 交换——`poll_at` 恒 `Some(10000)`、peer ACK 从不发出/回灌、client 恒 `FinWait1`（csend=8）；机制层 witness 证明 deferral/保活正确，但确认态终态步骤不可达。(b) 用户 fresh artifact 手工 QEMU（2026-08-24）结果：`single` 与 `fork` **全 PASS**（deferred-retirement 修复了 payload 交付路径）；原 MS01 在 `tcp-accept`、`tcp-adjacent` PASS 后，`tcp-512-capacity` 打印 `FAIL: tcp-512-recovery: connect: Connection refused`，随后 `tcp-close-relisten` PASS，再下一 case `udp-bidirectional` 中**永久挂起**（无 marker、无 END），用户 `^C` 中止——14/14 未达成。
- 完成项：T2.4-R2 全部 witness；T2.5-R2 机制 + 全部非终态运行期 witness + 机制级 unit/source/lock witness；T2.5-R3 的 payload 与 artifact 准备。
- 证据：见 Verification Evidence（probe 行已摘要）。`flush_removal_tx` 产品路径与该轮 ACK 动态相较父 Cycle 已从「同步 flush 误报」收敛为「deferral 保活正确、终态受注入时钟阻断」——同一 loopback close-ACK 动态第三次未完全收敛，触发三次失败反思：共享状态是`Router::receive` 的 tx_buffer 满门控与关闭方 `seq_to_transmit` 恒真重灌；本轮不再进行第四次盲试。
- 恢复条件：用户调用 `openspec-plan` Review 本 blocked Response。Plan 需决策：(1) T2.5-R2 loopback/deferred FinWait1 收敛路径（在 guest 放大为 listener-slot 与 512 规模 I/O 饥饿，证据见 Verification Evidence），(2) 基于新证据的下一修复 Cycle 边界。T2.5-R3 的手工 `single`/`fork` 已 PASS；重新手工运行原 MS01 需在 Plan 决策后的修复镜像上执行。恢复后 Act 将 `blocked → pending` 并继续。
- Evidence：None required（全部自动结果为确定性低成本命令；手工 QEMU 由用户回传）。

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: blocked（T2.5-R2 运行期终态未收敛 + T2.5-R3 手工待用户）
- Full diff reviewed: PASS
- Critical findings unresolved: 0
- Important findings unresolved: 1（loopback 终态非收敛 —— 已作为 Blocker Handoff 记录，责任归属 Plan）
- Minor findings unresolved: 1（injected-clock harness 对 `PollAt::Now` 需要主动推进时钟的约束，已固化在测试执行器）

完整 diff 已逐文件审查：T2.4-R2 锁序/零 round 契约保持；reaper 的 stale-分支修复为最小正确化（不触碰无效 handle）；deferral 不提前回收、不泄漏 handle；`remove` 组合路径（UDP/idle）行为与原 `take_readiness+inner.remove` 等价；smoltcp 与 axpoll 未修改；用户无关工作树内容未归因。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| axnet ordinary | `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib` | `test result: ok. 282 passed` | PASS |
| axnet qemu-diagnostics | 同命令 + `--features qemu-diagnostics -- --test-threads=1` | `test result: ok. 302 passed` | PASS |
| 新 close 相关 targeted ×100（ordinary） | `-- closing_socket_queued_tx_reaches_peer_before_removal` / `drop_state_read_and_deferred_enqueue_compose_without_deadlock` ×100 | 100/100 ok | PASS |
| 新 close 相关 targeted（qemu-diag 串行） | 同两条 ×30（每次两测试） | 30/30 ok | PASS |
| reaper targeted ×30 | `deferred_close_reap_dedups_stale_and_confirmed_removal` + `stack_round_reaps_deferred_close_before_poll_at` ×30 | 30/30 ok | PASS |
| MS04 host harness | `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs ... && 运行` | `16 passed`, exit 0 | PASS |
| kernel QEMU check | `cargo check --locked --offline -p starry-kernel --features qemu` | Finished, exit 0 | PASS |
| root D1 check | `cargo check --locked --offline --target riscv64gc-unknown-none-elf --features lichee-d1` | Finished, exit 0 | PASS |
| payload 交叉编译 | `riscv64-linux-musl-gcc -static -O2 -Wall -Wextra`（两个 payload） | 均为 BUILD OK，零警告 | PASS |
| QEMU artifact | `make LOG=error build` | `Finished release` + objcopy 生成 `StarryOS_riscv64-qemu-virt.bin`, exit 0 | PASS |
| fmt / OpenSpec / diff | `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check`；`openspec validate ... --strict`；`git diff --check HEAD` | 全 exit 0 | PASS |
| source assertions | `task_24_r2_*`/`drop_keeps_*`/`ms01_diagnostic_payloads_*`；`rg flush_removal_tx/not flushed within bound`（产品源码零命中） | 全 ok | PASS |
| 阻塞证据（probe 摘录） | closing full-chain 诊断 | `post-close: now=0 poll_at=Some(0) client=FinWait1 csend=8`；`post-eof: poll_at=Some(10000)`；`reap-1..5 (now=10000..50000) poll_at=Some(10000) client=FinWait1 csend=8 peer=CloseWait` —— ACK 从不回灌，确认态不可达 | BLOCK（loopback 终态） |
| 用户手工 QEMU `single` | runbook HTTP 下载 + 执行 | `MS01_LOOPBACK_DIAGNOSTIC_START single` … 6 个 PHASE … `PASS: single-loopback` … `END single` | PASS |
| 用户手工 QEMU `fork` | 同上 | `START fork` … all PHASE … `PASS: fork-loopback` … `END fork`（无 close-bound warning） | PASS |
| 用户手工 QEMU 原 MS01 | 同上 | `PASS: tcp-accept`；`PASS: tcp-adjacent`；`[ConnectionRefused] connection refused`；`FAIL: tcp-512-recovery: connect: Connection refused`；`PASS: tcp-relisten`；随后 `udp-bidirectional` 永久挂起（无 marker/END），用户 `^C`（2026-08-24 说明：非主动中断，是挂载空转） | FAIL（Acceptance 3 未闭合） |

**Persisted Evidence**

None required。全部自动结果为确定性 host/unit/compile/build 命令可低成本重跑；手工 QEMU 由用户回传 marker。未创建 Evidence 目录。

**Experience Candidates**

- Incident 候选：注入 loopback 优雅关闭在 `PollAt::Now`（未 ACK 数据 + 恒开窗口 → `seq_to_transmit` 恒真 → `poll_at=Some(0)`）与 `Router::receive` 的 `!tx_buffer.is_full` 门控下无法达成 FIN/ACK 确认态——冻结时钟 harness 的第 3 次同类非收敛（前两次见父 Cycle 文档）。含系统性诊断价值（probe 差分法定位到共享 tx_buffer 饥饿）。待 Plan 决策后由 Recorder 评估登记。引用：本 Act Response Blocker Handoff + Verification Evidence。
- Runbook 候选：None（手工 QEMU 尚未端到端验证）。

**Remaining Issues**

- **T2.5-R2 loopback 确认态终态非收敛（阻塞项）**：`PollAt::Now`（未 ACK 数据 + 恒开窗口 → `seq_to_transmit` 恒真）与 `Router::receive` 的 `!tx_buffer.is_full` 门控共同饥饿 peer delayed-ACK；注入 loopback 无法有界收敛。**用户 guest 复跑证实该根因在真实环境放大**：tcp-512-capacity 中 accept+close 释放的 backlog 槽位不能被及时 refill（runner 被 deferred close 的 Now 洪泛阻塞）→ recovery connect 被拒；FAIL 分支随后 close 511 个连接 → ~511 条 deferred FinWait1 条目洪泛 runner → 后续 `udp-bidirectional` 阻塞 I/O 被永久饿死（hang）。这是 MS01 14/14 验收的第 4 次非收敛，按三次失败规则停止，不再盲试；需 `openspec-plan` 决策（真实 QEMU 证据边界、listener-slot reconcile 与 deferred 收敛的互动、或 Acceptance 再定义）。
- **T2.5-R3 未闭合**：原 MS01 FAIL + hang + ^C（无 14/14、无 END）；`single`/`fork` 已 PASS。恢复路径依赖 Plan 决策后的下一轮修复 + 用户重新手工运行三命令。
- 既有非本 Cycle 问题：qemu-diagnostics 默认并行下 `reclaim_hold_drains_to_real_driver_full_without_observing_again` flake（串行稳定）；smoltcp 本地副本 7 个既有 internal warning；guest 复跑出现既有 `ldisc NoSuchProcess` 日志（父 Cycle 已记录为疑似独立噪声，本轮未处理）。

**Commit or Diff Reference**

- Working-tree diff at HEAD `fb87c8d3`；本 Cycle 改动集中于 `crates/axnet/src/{tcp,service,wrapper,stack_runner}.rs` 与 MS06 Iteration 001 文档；未创建 commit。

## Plan Review

- Status: reviewed

**Review Result**

replan-required

**Findings**

1. **Blocking — runner round新增无界工作，违反D4和Acceptance 4。**
   `Service::reap_deferred_removals` 使用 `while i < deferred_removals.len()`，每轮在
   Service+SocketSet guards 内扫描全部entry，且不进入 `STACK_STAGE_BUDGET`、`self_yield` 或
   telemetry。MS01 cleanup可产生至少511个deferred close；即使每个entry状态判断正确，单轮
   工作和锁持有时间也不再有界，无法证明listener、UDP和应用task不被饿死。
2. **Blocking — host close终态证据使用两个时间源。** `StackRunnerFuture::poll` 从
   `StackClock::Injected`读取测试时间，但`StackAccess::round`没有传递timestamp，
   `Service::stack_round`重新调用`wall_time_nanos()`。因此Act把注入时钟推进到50ms时，smoltcp
   round仍可能观察0；`FinWait1`和delayed ACK不收敛可以由test seam解释，不能证明QEMU产品
   具有相同计时根因。当前test还把“bounded run后raw handle仍存在”写成GREEN，未满足父Cycle
   原定的confirmed-state removal条件。
3. **Blocking — MS01 Acceptance 3实际失败。** 用户fresh single-hart QEMU中diagnostic
   `single`、`fork`均PASS且无close-bound warning，但原MS01在释放一个512 backlog slot后立即
   reconnect得到`ConnectionRefused`，随后UDP场景永久挂起并被用户中止；没有14/14或END。
   现有`full_queue_accept_frees_headroom_and_reconcile_refills_idle`只在accept后由test手工调用
   `reconcile`，没有覆盖应用立即reconnect的调度边界。
4. **Non-blocking — 已完成部分符合原契约。** public Drop不再调用`stack_round`，Service与
   SocketSet guard分离，public metadata/raw handle生命周期拆分，stale reaper不再对无效handle
   调用smoltcp remove；fresh ordinary 282/282和qemu-diagnostics 302/302通过。

**Deviation Classification**

PLAN-INVALID；NEW-EVIDENCE

**Acceptance Gaps**

- Acceptance 2：缺少使用同一协议时钟完成payload、FIN/ACK和confirmed raw removal的有效
  host witness；deferred retirement本身未满足单轮有界和跨entry公平。
- Acceptance 3：原MS01无14/14和END，`tcp-512-recovery`失败且后续UDP挂起。
- Acceptance 4：新增deferred reaper绕过D4 stage budget；现有自动suite没有512 close storm
  后listener/UDP/application progress见证。
- Acceptance 5：Act记录1个未解决Important finding；本Review新增两个阻塞设计/验证finding。

**Convergence**

reduced。相较父Cycle，caller-driven close progress、反向锁序、payload丢失、close-bound warning
和diagnostic single/fork已经闭合；剩余问题已收敛到round时间一致性、deferred retirement
budget以及满backlog/close storm下的应用前进。但原验证契约失效且同类close动态已达到三次失败
边界，不能创建第四个rework Cycle。

**Evidence**

- 实际代码：`service.rs::stack_round`、`reap_deferred_removals`；
  `stack_runner.rs::StackAccess::round`、`StackClock`、
  `closing_socket_queued_tx_reaches_peer_before_removal`；
  `listen_table.rs::accept`、`reconcile`；`tcp.rs::TcpSocket::accept/drop`。
- fresh Review命令：ordinary axnet `282 passed`，exit 0；qemu-diagnostics串行
  `302 passed`，exit 0；三个targeted tests各1/1 PASS；`openspec validate --strict`和
  `git diff --check HEAD` exit 0。
- runtime证据：Act Response记录的fresh QEMU `single`/`fork` PASS；原MS01
  `tcp-512-recovery: Connection refused`，随后`udp-bidirectional`无marker/END并由用户中止。
- Persisted Evidence：none；缺少Evidence目录不构成finding。

**Follow-up Decision**

原目标仍是Iteration 001既有的caller-independent socket/listener兼容，但达到它需要修订设计和
验证契约：每轮单一时间戳必须进入Service；deferred retirement必须成为有budget的stage；满
backlog accept必须在返回前恢复headroom。三项变化修改D3/D4/D7、delta spec、全局change tasks
和Iteration 001验证边界，因此使用replan，而不是把范围变化伪装成rework。下一Cycle先用RED
witness固定这些边界，再复验fresh QEMU；不得沿用Act对QEMU hang根因的未证实推断。

**Iteration Plan Update**

Iteration 001增加Tasks 2.6–2.7，并更新D3/D4/D7、相关delta scenarios、稳定基线、验证边界和
诊断边界。用户可见目标、512 backlog上限、Iteration 002、SMP/真板/性能Non-goals均不变。

**Next Cycle**

`003-replan.md`

**Next Iteration**

None; expand Iteration 002 only after `003-replan.md` is accepted.
