## ADDED Requirements

### Requirement: RX queue control 与 transport 解耦

MS04 MUST 提供 transport-neutral `NetQueueControl`，并与 `NetDriverOps` 现有的 `receive` / `recycle_rx_buffer` 共同表达 RX completion 可见性、reap、refill、通知抑制、通知重臂和队列错误。公共接口 MUST NOT 暴露 VirtIO available/used ring、descriptor token、MMIO 寄存器或 DWMAC descriptor 类型。VirtIO-MMIO 与 DWMAC 设备模型审查 MUST 能把各自的通知和 ownership 语义映射到该 contract；MS04 MUST NOT 引入 DWMAC 产品代码。

#### Scenario: VirtIO-MMIO 适配 queue contract

- **WHEN** VirtIO-MMIO RX used ring 出现 completion
- **THEN** 适配层 MUST 通过 `NetQueueControl` 向唯一 queue task 报告可服务工作
- **AND** transport-specific token 和 ring 字段 MUST 留在 VirtIO 后端

#### Scenario: 使用 DWMAC 模型审查 contract

- **WHEN** 以 DWMAC 的 descriptor ownership 与 IRQ mask/rearm 模型审查 `NetQueueControl`
- **THEN** 每项公共语义 MUST 能在不引用 VirtIO ring 布局的前提下解释
- **AND** 无法表达的 transport 差异 MUST 阻塞 Gate 2，不得留给 Act 扩展 trait

### Requirement: RX queue owner 唯一且切换有边界

系统 MUST 在任一时刻只允许一个 owner 消费同一 RX queue。异步激活前，MS02 轮询路径 MAY 保持 owner；IRQ、waker、task 和 queue control 全部初始化成功后，所有权 MUST 一次性切换给唯一、长驻的 RX queue task。异步激活后，10ms fallback、socket 调用或其他任务 MUST NOT 直接 reap 同一 RX queue。

#### Scenario: 异步 RX 成功激活

- **WHEN** IRQ handler、waker、queue task 和 transport queue control 全部准备完成
- **THEN** RX owner MUST 从轮询路径切换为唯一 queue task
- **AND** 后续 polling fallback MUST NOT 消费该 queue 的 completion

#### Scenario: 激活前初始化失败

- **WHEN** IRQ 注册、queue control 构造或 queue task 启动在所有权切换前失败
- **THEN** RX owner MUST 保持为 MS02 轮询路径
- **AND** 启动诊断 MUST 报告失败阶段和当前 owner

#### Scenario: 激活后发生 fatal queue error

- **WHEN** queue task 在已取得所有权后观察到 descriptor 或 queue fatal error
- **THEN** RX 路径 MUST 进入可观察 fault 状态并停止无界重试
- **AND** 系统 MUST NOT 自动启动轮询 owner 与故障 task 并发访问 queue

### Requirement: ISR 与 AtomicWaker 保持最小和 IRQ-safe

VirtIO-net ISR MUST 只读取和分类 cause、执行 device ack、保存有界 snapshot、更新 telemetry 并唤醒 RX queue task。ISR MUST NOT 读取、移动或回收 descriptor，MUST NOT 获取 axnet `Service` 锁，MUST NOT 运行协议栈。`AtomicWaker` 使用的 critical-section MUST 恢复进入临界区前的 IRQ enable 状态，ISR 中 `wake()` MUST NOT 在 handler 返回和 PLIC complete 前提前开启 IRQ。

#### Scenario: used-ring IRQ 唤醒 queue task

- **WHEN** IRQ 7 的 cause 包含 used-ring update
- **THEN** handler MUST ack cause、记录事件并调用固定 RX waker
- **AND** descriptor service MUST 只在 queue task 上下文发生

#### Scenario: ISR 中调用 AtomicWaker

- **WHEN** CPU 在 IRQ disabled 状态进入 `AtomicWaker::wake()` 的 critical-section
- **THEN** critical-section release 后 IRQ MUST 仍保持 disabled
- **AND** PLIC complete 前 MUST NOT 因该 critical-section 恢复而允许嵌套设备中断

#### Scenario: task 上下文注册 waker

- **WHEN** queue task 在 IRQ enabled 或 disabled 的任一进入状态注册 waker
- **THEN** critical-section 退出后 MUST 恢复相同的进入状态
- **AND** UART 现有 `AtomicWaker` 路径 MUST 继续通过回归

### Requirement: Register-recheck 关闭 lost-wakeup 窗口

RX queue task MUST 按“检查工作、注册 waker、重臂通知、再次检查工作与事件代次”的顺序进入 `Pending`。ISR 和任务之间的事件代次 MUST 使用与发布/观察角色一致的原子内存序。事件发生在注册前、注册期间或重臂后时，任务 MUST 最终观察到工作或再次被调度。

#### Scenario: event-before-register

- **WHEN** completion 在 queue task 注册 waker 之前到达
- **THEN** 注册后的再次检查 MUST 观察到 completion 或已变化的事件代次
- **AND** task MUST NOT 返回永久 `Pending`

#### Scenario: register-during-event

- **WHEN** ISR wake 与任务注册 waker 并发交错
- **THEN** task MUST 通过 AtomicWaker 或事件代次重新获得运行机会
- **AND** completion MUST 只由唯一 RX owner reap 一次

#### Scenario: rearm 后事件到达

- **WHEN** task 重臂通知后、返回 `Pending` 前出现新 completion
- **THEN** recheck MUST 观察到该 completion 或 ISR 事件
- **AND** task MUST 继续服务而不是依赖 10ms fallback

### Requirement: `RING_EVENT_IDX` 通知抑制与重臂有效

VirtIO-MMIO 后端 MUST 保留已协商的 `RING_EVENT_IDX`。通知抑制和重臂 MUST 控制该模式下的 `used_event` 或等效规范机制；实现 MUST NOT 把 `set_dev_notify` 在 `event_idx=true` 时的 no-op 当作有效控制，也 MUST NOT 通过关闭 feature 取得通过。对 registry 依赖的扩展 MUST 由工作区拥有的 adapter、vendor 或 Cargo patch 承载，MUST NOT 原地修改 Cargo registry。

#### Scenario: queue task 开始有界 drain

- **WHEN** queue task 因 used-ring event 开始服务 RX completion
- **THEN** VirtIO 后端 MUST 抑制新的 used-buffer 通知或设置等效阈值
- **AND** 持续 burst MUST NOT 造成每个 descriptor 一次无界 IRQ 重入

#### Scenario: 空队列准备 Pending

- **WHEN** queue task 已服务至空并准备等待
- **THEN** 后端 MUST 在 `RING_EVENT_IDX` 模式下重臂下一次 completion 通知
- **AND** 重臂后 MUST 执行 completion 与事件代次 recheck

#### Scenario: 依赖不暴露有效 EVENT_IDX 控制

- **WHEN** 当前依赖无法在不关闭 `RING_EVENT_IDX` 的条件下实现通知抑制和重臂
- **THEN** 实施 MUST 停止并返回 Plan
- **AND** 不得以 raw 私有字段穿透、registry 原地修改或 feature 降级继续

### Requirement: RX queue task 以有界 budget 推进

唯一 RX queue task MUST 对每次调度设置固定、可观察的 completion budget。单轮服务 MUST NOT 超过 budget。budget 用尽且仍有工作时，task MUST 保持通知抑制、自调度并让出 CPU；不得先重臂 IRQ，也不得在一次 poll 中无界 drain。纯 telemetry MAY 使用 Relaxed，参与调度和 ownership 的状态 MUST 使用与同步角色一致的 ordering。

#### Scenario: completion 数量小于等于 budget

- **WHEN** 一次激活可服务的 completion 数量不超过 budget 且 Router RX 缓冲有足够空间
- **THEN** task MUST 在同轮完成 reap、handoff 和 refill
- **AND** 队列为空后 MUST 进入 register-recheck 流程

#### Scenario: budget exhausted

- **WHEN** 一次激活的 completion 数量超过 budget
- **THEN** task MUST 精确停止在本轮 budget 边界并记录 budget exhaustion
- **AND** task MUST 通过软件调度继续处理且至少让出一次 CPU
- **AND** 其他 runnable task MUST 获得调度机会

#### Scenario: spurious 或 config-only IRQ

- **WHEN** ISR 唤醒后 RX queue 没有 completion，或 cause 只有 config-change
- **THEN** RX task MUST 至多完成一次无工作检查后回到等待
- **AND** MUST NOT 自唤醒形成 busy loop 或伪造 RX 进度

### Requirement: 使用现有 Router 缓冲完成临时 RX handoff

MS04 MUST 复用现有 Router 有界 RX 缓冲作为临时 handoff，不得创建 MS05 的最终 packet slot。queue task MUST 通过 axnet RX-only 入口处理必要的 Ethernet/ARP/IPv4 接收步骤，把可交付 payload 放入现有 Router 缓冲，并在同次服务中 refill 已 reap 的 descriptor。该入口 MUST NOT 执行 smoltcp ingress、egress、maintenance 或 socket readiness；由 RX 触发的 ARP 等发送 MAY 继续使用现有同步 TX 基线。

#### Scenario: IPv4 frame 完成临时 handoff

- **WHEN** queue task reap 一个有效 IPv4 RX completion 且 Router RX 缓冲有空间
- **THEN** payload MUST 进入现有 Router 缓冲
- **AND** 对应 descriptor buffer MUST 在本次服务结束前 refill
- **AND** smoltcp polling MUST NOT 在该 RX-only 入口运行

#### Scenario: Router RX 缓冲已满

- **WHEN** Router RX 缓冲没有可用 slot
- **THEN** queue task MUST 在 reap 下一个 completion 前停止
- **AND** 已完成 handoff 的 descriptor MUST 已被 refill
- **AND** 未 reap completion 的 ownership MUST 保持在 VirtIO queue

#### Scenario: 协议栈释放 Router 空间

- **WHEN** 现有 Service polling 消费 Router 中的 packet 并释放空间
- **THEN** 它 MUST 通过软件事件重新调度因 Router 满而暂停的 queue task
- **AND** 该软件 wake MUST 不依赖新的硬件 IRQ

### Requirement: 兼容性、验证顺序与 Evidence

MS04 MUST 保持 MS01/MS02 socket 行为、同步 TX、MS03 IRQ 分类、UART async 路径以及 early/panic console。所有 host tests、target build、静态检查、纯状态机竞态测试和可自动执行的 QEMU 准备 Gate MUST 先在当前环境尝试。自动 Gate 通过后方可进入用户手工批次；只有按 R44 有原始日志证明属于 sandbox 能力拒绝的 `ENV-BLOCKED` 项 MAY 延后到该批次，产品编译、链接、测试、source、validation 或 diff 失败 MUST NOT 延后。需要用户操作的验证 MUST 集中在 iteration 最后。QEMU 串口原始日志、probe 输出、环境/命令、退出状态和计数快照 MUST 保存为 change-local Evidence；中断或缺失的见证 MUST 标记未完成。

#### Scenario: 自动产品 Gate 未通过

- **WHEN** 任一前置 host、build、静态或自动竞态 Gate 出现产品编译、链接、断言、source、validation 或 diff 失败
- **THEN** iteration MUST 停止在对应任务
- **AND** MUST NOT 请求用户执行手动 QEMU 测试

#### Scenario: 自动 Gate 被 sandbox 环境阻塞

- **WHEN** 自动命令最终失败且原始日志按 R44 明确定位到只读路径、禁止联网或安装、`EPERM`、`SIGSYS`、`Bad system call` 或用户终端/权限边界
- **THEN** 对应项 MUST 标记 `ENV-BLOCKED` 并记录原命令、最终退出码和最早环境失败层
- **AND** 该项 MUST 移到 iteration 最后的手工批次，不得提前打断其他可执行自动 Gate
- **AND** 无法区分环境与产品原因时 MUST 按产品 Gate 失败处理

#### Scenario: 执行 iteration 最后的手动验收

- **WHEN** 所有自动产品 Gate 已通过，且其他自动项已通过或按 R44 标记 `ENV-BLOCKED`
- **THEN** 用户 MUST 先执行计划中固定的 `ENV-BLOCKED` 复跑命令，再执行 QEMU 操作步骤
- **AND** RX burst、budget、公平性、spurious、descriptor 守恒与网络回归结果 MUST 分项记录

#### Scenario: QEMU Evidence 不完整

- **WHEN** 串口日志、probe 输出、环境信息或完成 marker 任一缺失
- **THEN** 对应 runtime Gate MUST 标记为未完成或失败
- **AND** 部分 telemetry MUST NOT 计为 MS04 通过

#### Scenario: 证据范围声明

- **WHEN** MS04 QEMU Gate 全部通过
- **THEN** 结论 MUST 限定于当前单 hart VirtIO-MMIO 环境
- **AND** MUST NOT 声明 PCI、SMP、DWMAC 或真板已验证
