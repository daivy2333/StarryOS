## Why

MS05 已建立唯一双向 queue service 和有界 packet slots，但 smoltcp 仍依赖 socket 调用主动执行 `poll_interfaces()`，设备事件也只到达一个可被后续注册覆盖的全局 stack waker。MS06 需要把协议栈推进交给独立 runner，并把 smoltcp 的单槽 socket waker 桥接为应用可依赖的多 waiter readiness。

## What Changes

- 增加单 hart QEMU 下唯一的 smoltcp stack runner，合流 device、software 和 `poll_at` timer wake；每轮有界推进 ingress、egress、maintenance 和 dispatch，持续工作时主动让出。
- 产品 TCP/UDP 路径不再以同步 `poll_interfaces()` 作为网络进展条件；socket 创建、连接、发送、关闭和 listener 变化只发布 software wake。
- 为 TCP、UDP 和 listener 建立 smoltcp 单槽 waker 到每 socket `axpoll::PollSet` 的 bridge，使用 check → register → recheck，支持同一 socket 多 waiter 和容量 64 的 overflow 唤醒后重注册。
- 明确 `IN`、`OUT`、`RDHUP`、`HUP` 和 `ERR` 与下一次实际 I/O 的对应关系；close、EOF、half-close、listener accept 和稳定数据面 fault 必须唤醒受影响 waiter。
- 修正 stack runner 与 socket path 的锁顺序；任何 runner 或 socket waiter 都不得持有 `Service`、`SocketSet` 或 listener guard 跨越 `await`。
- 保留 queue service 唯一硬件 owner。异步 queue 尚未激活或设备本来要求 polling 时允许有界 polling fallback；稳定 fault 后只传播错误，不自动创建第二个同步 owner。
- 增加 host/model 与单 hart QEMU 回归，证明三类 runner wake、空闲无轮询、连续流量公平、多 waiter/overflow 和 readiness/I/O 一致性。

## BDD Scenario Sketch

### Happy Path：事件驱动的协议栈推进

- **前置状态**：MS05 queue service 已激活，唯一 stack runner 已安装，TCP/UDP socket 存在。
- **触发动作**：设备收到 frame、应用提交 socket 状态变化，或 smoltcp timer 到期。
- **可观察结果**：对应 wake 使 runner 在 task context 内有界推进协议栈并发布准确 socket readiness；应用不调用额外网络 API 也能继续收发。
- **失败边界**：依赖主动 socket poll、ISR 直接调用 smoltcp、第二 stack owner、持锁跨 `await` 或任一 wake source 永久丢失均失败。

### Quiet Path：空闲不轮询

- **前置状态**：IRQ-backed queue 已激活，无 packet、software mutation 或到期 timer。
- **触发动作**：系统保持网络空闲超过既有 polling fallback 周期。
- **可观察结果**：runner 保持休眠，poll counter 不增长；下一次真实事件仍能唤醒。
- **失败边界**：活跃 IRQ 设备依赖固定 10ms tick、self-wake storm、busy loop 或永久睡眠均失败。

### Edge Case：连续流量与预算边界

- **前置状态**：RX、TX 或 socket dispatch 持续可推进，工作量跨越单轮 budget。
- **触发动作**：注入 budget−1、budget 和 budget+1 工作以及双向持续流量。
- **可观察结果**：每轮工作有上界，剩余工作通过 generation/self-yield 继续；其他任务和相反方向均获得运行机会。
- **失败边界**：单轮无界 drain、某方向饥饿、遗漏剩余工作或空闲时继续自让出均失败。

### 多 Waiter 与 Overflow

- **前置状态**：同一 TCP、UDP 或 listener socket 被 poll/select/epoll waiter 并发观察。
- **触发动作**：read/write/accept readiness 改变，或第 65 个 waiter 超出 `PollSet` 容量。
- **可观察结果**：已登记 waiter 均获通知；被 overflow 替换的 waiter 被唤醒并按 check → register → recheck 重新竞争，实际 I/O 决定最终结果。
- **失败边界**：后注册者静默覆盖前者、永久 Pending、虚假 ready 被当成 I/O 成功保证，或隐藏 listener socket 无法唤醒 public listener 均失败。

### Close、Half-Close 与 Error

- **前置状态**：socket waiter 已登记，连接可能收到 FIN、本地 shutdown/close，或 queue service 发布稳定 fatal。
- **触发动作**：状态转为 EOF、half-closed、fully closed 或 faulted。
- **可观察结果**：bridge 发布对应 `IN/RDHUP/HUP/ERR` 组合；紧随其后的 read/write/accept 返回匹配的 EOF、成功或稳定错误。
- **失败边界**：只唤醒全局最后一个 waiter、报告 `OUT` 后下一次写必然无关地阻塞、fault 被隐藏成普通 Full，或 close 后 waiter 永久 Pending 均失败。

### Startup、Fallback 与 Cancellation

- **前置状态**：网络 Service 可能尚未安装、runner 可能重复启动，或专用 IRQ/queue 激活失败。
- **触发动作**：执行启动、重复启动、wait future drop 和 polling fallback 路径。
- **可观察结果**：runner 至多一个；pre-init 操作安全失败或延后；取消只移除 waiter；未激活 queue 使用有界 fallback，稳定 fault 不切换 owner。
- **失败边界**：重复 runner、启动竞态 panic、取消 socket I/O 状态、fallback busy loop 或 fault 后双 owner 均失败。

### Compatibility 与验证超时

- **前置状态**：MS01/MS02 同步 socket 行为和 MS04/MS05 单 hart QEMU 数据面基线可用。
- **触发动作**：运行 TCP、UDP、listener、nonblocking、poll/select/epoll、MS04 核心模式和 MS05 数据面回归；任一运行可能达到固定 deadline。
- **可观察结果**：既有 socket 语义与 queue ownership 保持；新增结论限定于单 hart QEMU VirtIO-MMIO；超时或缺失 marker 记为未完成而非通过。
- **失败边界**：用编译代替 runtime readiness、把单次唤醒当成多 waiter 证明，或把 QEMU 结果扩大为 SMP、PCI/DWMAC、真板与性能结论均失败。

## Capabilities

### New Capabilities

- `qemu-application-visible-async-network-stack`: 定义 MS06 stack runner、三源 wake、预算、公平性、socket multi-waiter bridge、close/error readiness 和单 hart QEMU 验收。

### Modified Capabilities

- `network-stack-baseline`: 将既有 readiness 与 I/O 一致性扩展为异步推进、多 waiter、listener、close 和稳定错误契约。
- `qemu-bounded-bidirectional-device-data-plane`: 将 packet slots 的 stack-side consumer 从 caller-driven Service 演进为唯一 stack runner，同时保留 queue service 硬件 ownership 和 stack-progress hint 边界。

## Impact

- `crates/axnet` 的 Service 轮询入口、runner lifecycle、socket set、TCP/UDP/listener waker 注册、readiness 映射和锁顺序。
- `kernel` QEMU 网络启动路径、唯一 queue task/runner 的先后关系、poll/select/epoll 可观察行为和诊断见证。
- `axpoll::PollSet` 的既有容量与 overflow 语义会被直接复用并补充网络侧回归；不替换 axpoll、不新增外部依赖。
- MS04/MS05 的 queue event、packet slots、fault publication 和单 hart QEMU runtime Gate 需要回归，但硬件 queue ownership 不变。

## Non-goals

- reset generation、link flap、queue stall recovery、分层 cancel/timeout 或设备热插拔。
- SMP、跨 hart wake、multiqueue、RSS、多接口 runner、PCI、DWMAC 或真板 transport。
- 零拷贝、offload、IRQ moderation、性能优化或真实硬件性能结论。
- 替换 axpoll、改变其 64 waiter 容量，或建立新的通用多 waiter 子系统。
- 修改全局 tasks、SNAPSHOT、M/D/K/R/I，归档 change，或实施产品代码。

## Gate 1

- Status: approved.
- 默认决策：产品路径由唯一 stack runner 推进；TCP/UDP 不再依赖同步 `poll_interfaces()`；采用 `IN/OUT/RDHUP/HUP/ERR` 最小一致性契约；沿用 `PollSet` 64 容量与 overflow 唤醒重注册；listener 单独桥接隐藏 socket；未激活 queue 可有界 fallback，稳定 fault 不回退第二 owner。
- 用户于 2026-08-21 审计集中决策、BDD 草图和 MS06 范围后回复“同意，继续吧”，正式批准 Requirements and Scope。

## Initial Gate 2

- Status: approved.
- Investigation: PASS。已定位初始化顺序、Service/Router 推进链、QueueEvent/lifecycle、timer/fallback、TCP/UDP/listener waker、锁序、axpoll overflow 和现有测试入口，并记录当前 revision 的新鲜基线。
- Design: PASS。`design.md` 已明确唯一 runner、独立 StackEvent、runner-owned timer、每 stage budget 32、全局锁序、per-socket bridge、listener bridge、terminal readiness 和单 hart QEMU 边界；没有留给 Act 决定的实质语义。
- Task contracts and iteration balance: PASS。13 个任务分配到 3 个依赖有序 Iteration；Iteration 000 保留 legacy inline poll 作为可运行迁移基线，Iteration 001 原子切换 readiness，Iteration 002 完成 terminal/QEMU 验收。
- Traceability and verification: PASS。9 个 delta requirements 全部映射到 design、task、Iteration、代码位置和测试见证；无 Missing 或未批准 Simplified。当前 Cycle 的 RED/GREEN、100×交错、编译、兼容、OpenSpec 和 diff Gate 已明确。
- Persisted Evidence: `none`。Iteration 000 只有可低成本复现的 host/model、编译和审查 Gate，Act Response 足以记录决定性结果。
- 用户于 2026-08-21 审计完整计划后回复“批准”，正式批准 Execution Readiness。

## Cycle 003 Replan Gate 2

- Status: approved。
- Trigger: Cycle 002 Review确认runner/Service双时钟、无界deferred reaper和满backlog
  accept→立即reconnect缺口；fresh QEMU原MS01无14/14或END。
- Requirements and Scope: Gate 1不变。replan只补齐达到既有R2/R3/R4/R6和MS01兼容所需的
  时间、budget和listener headroom契约，不增加用户可见目标。
- Task contracts and iteration balance: PASS。16个任务分配到3个Iteration；Iteration 001增加
  Tasks 2.6–2.7，Iteration 002保持原依赖和范围。
- Investigation、Design、Traceability、Verification和Persisted Evidence：PASS，详见
  `iterations/001-socket-and-listener-readiness-bridge/003-replan.md`。
- User Approval: PASS。用户于2026-08-24回复“批准”；Cycle 003已由`draft`更新为`ready`，
  等待显式`openspec-act`调用。

## Cycle 005 重新平衡审计

- Status: replan-required。
- Trigger: 用户于2026-08-24指出当前Cycle任务过重并要求重新审计、拆分工作；Cycle 005尚未调用Act，
  没有该Cycle产生的产品代码或验证结果。
- Requirements and Scope: Gate 1不变；不增加、不裁剪用户可见Requirement，也不改变D4、D7、D10、
  D11的行为契约。
- Iteration balance: 原Cycle把listener cursor、UDP raw-handle drain ownership和QEMU/guest事件排序放入
  同一次Act，跨越三个可独立验收的故障域。修订后17个tasks分配到5个依赖有序Iteration：Iteration
  001仅收口Task 2.6，Iteration 002执行Task 2.7，Iteration 003执行Task 2.8，原terminal/QEMU最终验收
  顺延为Iteration 004。
- User Approval: PASS。用户于2026-08-24回复“已经拆分完了么，认可你的思路”，批准修订后的
  Iteration Map与Cycle 006范围；Cycle 006已更新为`ready`。该批准不构成自动Act授权。

## Iteration 004 Cycle 001 Replan Gate 2

- Status: approved。
- Trigger: Cycle 000只实施Task 3.1后按用户指令停止。Plan Review发现global/local terminal所有权、UDP blocking recv重试、connect hint线性化和terminal 0/1/2/64/65见证仍有Acceptance gap；原Iteration还混合guest witness、自动资格和人工QEMU四个故障域。
- Requirements and Scope: Gate 1不变；不新增或裁剪R1-R7。D6/D8只澄清既有stable fault、readiness和I/O一致性要求。
- Iteration balance: 21个tasks分配到8个依赖有序Iteration。Iteration 004执行Tasks 3.1-3.2并关闭terminal host/model基线；Iteration 005执行Tasks 4.1-4.3并构建guest witness；Iteration 006执行Task 5.1并关闭自动资格；Iteration 007执行Tasks 6.1-6.2并完成人工single-hart QEMU验收。
- Investigation、Design、Task Contracts、Traceability、Verification和Persisted Evidence：PASS，详见`iterations/004-terminal-readiness-and-qemu-acceptance/001-replan.md`；Evidence模式为`none`。
- User Approval: PASS。用户于2026-08-26回复“认可，那就给出下一轮rework cyc和对iters map进行更新吧”；因Map和Acceptance边界发生变化，按规则创建replan而非rework Cycle。该批准不构成自动Act授权。

## Host 测试可靠性 Iteration 插入

- Status: approved-map-update。
- Trigger: Iteration 004 Review确认R57 Incident记录的并行全局`SOCKET_SET`/`LISTEN_TABLE`竞态与独立
  qemu-diagnostics flake不归因于terminal Cycle，但会削弱后续automatic qualification的可信度。
- Requirements and Scope: 不新增产品Requirement，不改变R1-R7或Iteration 005的guest witness契约；新增的
  Tasks 5.1-5.2只修复host test fixture、共享状态隔离和确定性，产品socket/readiness语义保持不变。
- Iteration balance: 23个tasks分配到9个依赖有序Iteration。Iteration 006在application witness之后、automatic
  qualification之前处理两个不预设同根的测试可靠性问题；原automatic qualification顺延为Iteration 007，
  原single-hart QEMU acceptance顺延为Iteration 008。
- Verification boundary: Iteration 006必须让R57失败子集、diagnostics目标测试和两profile默认并行full suites
  重复稳定通过；skip/ignore、无限重跑、隔离重跑豁免或把完整套件串行化均不构成修复。
- User Approval: PASS。用户于2026-08-26要求“更新iter map把这个问题的修复暂时规划一个iter，以待后续进行修复”。
  本次只更新Map，不展开Iteration 006 Cycle，不调用Act。
