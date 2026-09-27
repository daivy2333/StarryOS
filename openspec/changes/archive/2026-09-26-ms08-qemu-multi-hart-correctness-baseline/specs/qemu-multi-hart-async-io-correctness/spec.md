## Purpose

规定 QEMU 多 hart 下的共享 SMP 原语，以及异步 UART 和 VirtIO-MMIO 网络各自的 placement、所有权、跨 hart wake、背压、完成、迁移与恢复正确性。

## ADDED Requirements

### Requirement: placement 只使用已在线且可调度的 hart

系统 MUST 区分 configured、present、online 和 schedulable hart。驱动后台任务 MUST 在 secondary harts 就绪后，从实际 online/schedulable 集合计算入队前 affinity，不得假设 boot hart、IRQ hart 或 hart 0 是同一个对象。固定资格场景的任务 MUST 使用 singleton affinity；不得为每个 hart 创建驱动 owner/copy task。

#### Scenario: 单 hart 退化

- **WHEN** 平台只报告一个 online/schedulable hart
- **THEN** UART ISR/copy tasks/TTY caller 和 NIC IRQ/owner/runner MAY 共置
- **AND** 既有单 hart UART 和网络语义 MUST 保持

#### Scenario: 多 hart 固定 placement

- **WHEN** 平台报告至少三个 online/schedulable hart
- **THEN** UART RX/TX copier 和网络 owner/runner MUST 按确定 policy 选择有效 singleton mask，并尽可能分离具有直接通知关系的角色
- **AND** 未选中的 hart MUST 保持可供普通调度使用

#### Scenario: 稀疏或无效集合

- **WHEN** hart ID 不连续，或候选集包含 offline、reserved、未初始化或不可调度 hart
- **THEN** policy MUST 只返回有效集合中的 ID
- **AND** 无法形成安全 placement 时 MUST fail closed，不得 panic、越界或先入队后纠正

#### Scenario: secondary scheduler 尚未初始化时普通 task 入队

- **WHEN** configured hart 已知但只有当前 hart 的 run queue 完成初始化
- **THEN** 普通 spawn MUST 只从已经发布为 schedulable 的 run queue 中选择首次入队目标
- **AND** task 的默认 full affinity MUST 保留，使后续 block/wake 可在新发布的 schedulable hart 上自然选择
- **AND** 显式 affinity 若只包含未初始化 hart MUST fail closed，不得解引用未初始化 run queue

### Requirement: CPU mask 索引访问在 release 构建中也必须 fail closed

工作区公开的 `axtask::AxCpuMask` MUST 在访问 registry mask 前无条件校验索引。越界读取 MUST 返回“不在集合”；越界写入 MUST 返回明确错误并保持全部合法 bit 不变。该行为 MUST 不依赖 debug assertion，且原始 registry mask MUST NOT 从公开 API 泄漏给 scheduler、kernel、driver 或 syscall 调用方。

#### Scenario: 容量边界读取

- **WHEN** debug 或 release 构建读取索引 `MAX_CPU_NUM` 或 `usize::MAX`
- **THEN** 结果 MUST 表示该 CPU 不在 mask 中
- **AND** 不得 panic、读取垃圾 bit 或改变 mask

#### Scenario: 容量边界写入

- **WHEN** debug 或 release 构建写入索引 `MAX_CPU_NUM` 或 `usize::MAX`
- **THEN** 操作 MUST 返回明确错误
- **AND** mask 的合法 bit、位数、迭代结果和原始字节 MUST 保持不变

#### Scenario: 合法 mask 兼容性

- **WHEN** scheduler、placement、syscall 或迁移 control 对合法索引执行构造、读取、写入、集合运算或迭代
- **THEN** 结果 MUST 与现有 `AxCpuMask` 合法输入语义一致
- **AND** affinity validation、run-queue selection 和 remote wake MUST 不回归

### Requirement: kernel critical-section 在 SMP 下提供全局互斥

供 `AtomicWaker` 等共享异步状态使用的 critical-section MUST 在所有 online hart 之间互斥，并提供至少 Acquire/Release 语义。进入时 MUST 保存并关闭本 hart IRQ；退出时 MUST 只恢复与该入口匹配的 IRQ 状态。同 hart 嵌套不得自锁，非 owner 或 depth 异常不得释放他人所有权。

#### Scenario: 两个 hart 并发 register/wake

- **WHEN** 一个 hart 注册 waker，另一 hart 同时发布事件并 wake
- **THEN** 非原子 waker cell 访问 MUST 串行化
- **AND** 不得数据竞争、永久丢 wake 或破坏 waker 状态

#### Scenario: ISR 和 task 嵌套

- **WHEN** IRQ-disabled handler 或 task 上下文嵌套进入 critical-section
- **THEN** 只有最外层 release 可释放全局所有权
- **AND** 本 hart IRQ MUST 恢复到匹配入口的状态

### Requirement: 远端 ready task 由事件直接获得调度机会

当成功的 `Blocked → Ready` 转换把 task 放入远端 run queue 时，scheduler MUST 通知目标 hart；本地 ready MUST 只提交本地 reschedule。重复 wake、未发生状态转换的 wake 和纯 telemetry 不得额外发送 IPI。周期 timer、busy polling 或连续 self-wake MUST NOT 成为正确性依赖。

#### Scenario: IRQ 唤醒远端 copier/owner

- **WHEN** UART 或 NIC IRQ 发布 cause 并唤醒位于另一 hart 的后台任务
- **THEN** 目标 hart MUST 因此次 ready transition 获得 IPI 并在有界 deadline 内运行 task
- **AND** 禁止目标周期 timer 的 witness 仍 MUST 成功

#### Scenario: event/register 交错

- **WHEN** publish/wake 与 generation snapshot、waker register、recheck 或 park 跨 hart 并发
- **THEN** task MUST 观察事件或再次获得调度
- **AND** spurious wake MUST 只导致有界检查

### Requirement: 跨 hart 状态按同步角色选择 ordering

纯 telemetry MAY 使用 Relaxed。在 wake 前发布并由另一 hart 观察的状态 MUST 使用 Release/Acquire 或更强的已证明同步关系；参与状态迁移的 RMW MUST 使用 AcqRel 或更强 ordering；必须一致观察的多字段状态 MUST 使用同一锁或一致快照协议。

#### Scenario: 状态发布后 wake

- **WHEN** publisher 更新 UART completion/placement 或 network cause/lifecycle/epoch/readiness 后 wake 另一 hart
- **THEN** observer MUST 在处理工作前获取对应同步关系
- **AND** 不得组合新 generation 与未提交的旧字段

### Requirement: UART 保持唯一 SPSC 角色和一次启动

每个 UART driver MUST 恰好启动一个 RX copier 和一个 TX copier。RX ring MUST 保持唯一 copier producer 和唯一 TTY reader consumer；TX ring MUST 保持逻辑唯一 TTY writer producer 和唯一 copier consumer。task migration 不得创建第二个逻辑 endpoint。

#### Scenario: copier 启动

- **WHEN** QEMU UART hardware、ring 和 IRQ 已初始化且 secondary harts 就绪
- **THEN** kernel adapter MUST 在首次入队前分别为 RX/TX copier 提交有效 affinity
- **AND** 重复启动 MUST fail closed，不得构造第二 ring endpoint

#### Scenario: 迁移 copier

- **WHEN** 现有 copier 的 affinity 从 singleton 扩展到两个有效 hart
- **THEN** 同一 task identity MUST 在 block/wake 后可于新 hart 继续
- **AND** SPSC producer/consumer identity 、ring index 和 staged state MUST 连续

### Requirement: UART 跨 hart 数据面保持 readiness 与物理 drain 语义

QEMU NS16550 下，跨 hart RX、TX、poll/readiness、short write、Full→恢复和 `flush/tcdrain` MUST 保持现有行为。`flush/tcdrain` 只能在 TX ring empty、copier inactive、staged bytes 为零且 hardware transmitter empty 时成功。无工作时 copier MUST 保持休眠。

#### Scenario: RX 边界与 poll

- **WHEN** host 向 QEMU serial chardev 注入跨 FIFO/ring 边界的带序号 payload
- **THEN** guest reader MUST 按序且恰好一次收到所有字节
- **AND** poll/readiness MUST 通过 check-register-recheck 与实际可读状态一致

#### Scenario: TX Full 后恢复

- **WHEN** TX ring 填满导致 short write，随后远端 TX copier 释放容量
- **THEN** writer MUST 因容量事件恢复并只重试未接受字节
- **AND** 不得丢失、重复或忙轮询

#### Scenario: tcdrain 四阶段完成

- **WHEN** guest 在跨 hart TX 压力后调用 `tcdrain`
- **THEN** 调用 MUST 等待 ring、copier、staged bytes 和 hardware empty 全部完成
- **AND** 不得提前返回或永久 Pending

#### Scenario: UART quiet window

- **WHEN** 测试禁止额外 console 日志，UART ring、FIFO 和 waiter 都无工作
- **THEN** copier progress MUST 在观察窗口保持不变
- **AND** timer/IPI/spurious IRQ MUST NOT 被计为 UART 数据进度

### Requirement: 网络保持唯一 queue owner 和分层 runner

每个硬件 RX/TX queue 在任意时刻 MUST 只有一个逻辑 owner。IRQ、socket caller、stack runner、timer 和 reset caller MUST 只发布事件或稳定状态，不得直接推进 descriptor ownership。owner 和 runner MUST 在 secondary-ready 后以入队前 affinity 启动。

#### Scenario: 固定 placement 数据面

- **WHEN** owner、runner 和应用在不同 hart 上处理持续双向流量
- **THEN** descriptor、buffer、slot 和 ticket MUST 恰好一次消费与回收
- **AND** RX、TX、runner 和应用 MUST 持续获得进度

#### Scenario: owner fault

- **WHEN** 多 hart 中的唯一 owner 进入稳定 fatal 状态
- **THEN** faulted owner identity MUST 保持且 waiter MUST 被唤醒
- **AND** 不得自动创建 polling fallback 或第二 owner 并发接管

### Requirement: MS08 以分层 Gate 证明 QEMU 异步 I/O 多 hart 正确性

Host/model tests MUST 覆盖 1、2、3、4、8、16 个 hart、非零 boot/IRQ hart、稀疏集合、critical-section、remote wake 和 ordering。最终 QEMU runtime MUST 使用 `SMP=16`，并直接验证网络固定 placement、跨 hart wake、双向数据面、Full→恢复、readiness/quiet 和资源账本。UART 专项、受控迁移、组合压力、reset/link 和旧阶段逐项回归不属于本次最终 runtime Gate。

#### Scenario: UART 作为测试基础设施

- **WHEN** QEMU 通过 UART console 启动并输出网络测试结果
- **THEN** 完整有界输出 MAY 作为本轮环境可用性的前置条件
- **AND** 该输出 MUST NOT 被表述为 UART 异步 RX、背压、drain 或迁移的独立资格

#### Scenario: 网络资格

- **WHEN** QEMU 以 `SMP=16` 运行固定 placement、timer-disabled wake、TCP/UDP 双向、Full→恢复、readiness 和 quiet
- **THEN** 必须直接观察唯一 owner、runner、实际 IRQ/task hart、IPI 和资源账本
- **AND** 普通 ping、编译成功或历史单 hart evidence MUST NOT 替代

#### Scenario: 结论边界

- **WHEN** MS08 所有 Gate 通过
- **THEN** 结论 MUST 限定于 QEMU VirtIO-MMIO 在 16 个同构 hart 上的固定 placement 网络路径
- **AND** MUST NOT 声明 X100/A100 异构调度、RT24、K3 AIA、D1/K3 UART、真板 online 集合、DMA/cache、CPU hotplug、multiqueue 或性能已验证
