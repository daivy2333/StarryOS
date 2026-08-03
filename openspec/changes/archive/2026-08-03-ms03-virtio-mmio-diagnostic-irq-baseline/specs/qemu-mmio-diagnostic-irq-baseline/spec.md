## ADDED Requirements

### Requirement: QEMU MMIO IRQ 平台事实

MS03 MUST 从 QEMU 平台事实取得 VirtIO-net 的
MMIO 地址和 PLIC IRQ。
通用 VirtIO 驱动 MUST NOT 写入 QEMU 专属常量。
启动诊断 MUST 输出 transport、地址和 IRQ。

#### Scenario: 解析 VirtIO-net IRQ

- **WHEN** 当前 QEMU `virtio-net-device` 被 MMIO probe 识别
- **THEN** 平台适配 MUST 将 `0x10007000` 映射到 PLIC IRQ 7
- **AND** IRQ 诊断控制面 MUST 收到该 IRQ
- **AND** MS03 数据面 MUST 继续报告轮询 capability

#### Scenario: IRQ 映射缺失

- **WHEN** MMIO 地址没有唯一的平台 IRQ 映射
- **THEN** MS03 IRQ 初始化 MUST 失败并输出地址
- **AND** 该设备 MUST 保留 MS02 轮询模式
- **AND** 结果 MUST NOT 计为 MS03 通过

### Requirement: UART 与网卡使用设备 handler

QEMU UART 与 VirtIO-net MUST 通过
`axhal::irq::register` 绑定各自的设备 handler。
两者 MUST NOT 依赖单槽全局 IRQ hook。
每次注册结果 MUST 被检查。

#### Scenario: 两个 handler 同时注册

- **WHEN** QEMU 完成 UART 与 VirtIO-net IRQ 初始化
- **THEN** UART IRQ 10 与网卡 IRQ 7 的注册 MUST 都成功
- **AND** 两个 handler MUST 能独立增长所属计数

#### Scenario: handler 注册冲突

- **WHEN** 任一 IRQ 已被其他 handler 占用
- **THEN** 对应初始化 MUST 输出失败 IRQ 和设备
- **AND** 依赖该 handler 的路径 MUST NOT 声明可用

#### Scenario: 网卡 IRQ 到达

- **WHEN** PLIC claim 返回 IRQ 7
- **THEN** 只允许 VirtIO-net handler 处理网卡 cause
- **AND** UART handler 计数 MUST NOT 因该事件增长

### Requirement: MS03 网卡 ISR 保持诊断边界

VirtIO-net handler MUST 只读取 cause、执行 transport ack、
保存 snapshot 和更新原子计数。
它 MUST NOT 搬运 descriptor 或 packet。
它 MUST NOT 唤醒网卡 queue task。
`RING_EVENT_IDX` rearm MUST 继续由唯一数据面 owner
在消费 used ring 时完成。

#### Scenario: used-ring 中断

- **WHEN** MMIO interrupt status 包含 used-ring update
- **THEN** handler MUST 增加 used-ring 与 ack 计数
- **AND** handler MUST 在返回前清除设备 cause

#### Scenario: 配置变化中断

- **WHEN** MMIO interrupt status 包含 config-change
- **THEN** handler MUST 增加 config-change 与 ack 计数
- **AND** 该事件 MUST 与 used-ring 分开记录

#### Scenario: 无 pending cause

- **WHEN** handler 运行但设备没有 pending cause
- **THEN** handler MUST 增加 spurious 计数
- **AND** handler MUST NOT 修改 descriptor 或协议栈状态

### Requirement: 网卡实例与数据面 owner 唯一

MS03 MUST 只保留一个 VirtIO-net 设备实例。
MS02 轮询路径 MUST 继续作为唯一数据面 owner。
IRQ control MUST NOT 持有或修改 queue 数据面状态。
IRQ control MUST NOT 实现第二个 `NetDriverOps` 设备。

#### Scenario: MS03 启动网卡

- **WHEN** VirtIO-net 完成 probe 和 IRQ 初始化
- **THEN** 系统 MUST 只创建一个网卡设备实例
- **AND** descriptor 进度 MUST 仍由 MS02 轮询路径负责
- **AND** `AxNetDevice::irq_num()` MUST 在 MS03 保持 `None`

#### Scenario: IRQ control 处理事件

- **WHEN** 网卡设备 handler 读取和 ack interrupt status
- **THEN** IRQ control MUST NOT 获取第二份 queue 所有权
- **AND** IRQ control MUST NOT 与轮询路径并发修改 descriptor

#### Scenario: 分类 MS03 证据

- **WHEN** MS03 中网络流量通过
- **THEN** 结果 MUST 只证明轮询数据面与 IRQ 控制面共存
- **AND** 结果 MUST NOT 声明异步网卡已经可用

### Requirement: ack、EOI 和 rearm 顺序可验证

设备 cause MUST 在设备 handler 返回前 ack。
PLIC EOI MUST 在设备 handler 返回后发生。
queue rearm MUST 适配实际协商的 VirtIO features。
实现 MUST 保留 `RING_EVENT_IDX`。

#### Scenario: 重复投递 used-ring 中断

- **WHEN** 同一队列产生两次独立 used-ring 事件
- **THEN** 两次事件 MUST 各有 handler 与 ack 计数
- **AND** 第二次事件 MUST 在第一次 EOI 后到达

#### Scenario: `RING_EVENT_IDX` 已协商

- **WHEN** 设备协商结果包含 `RING_EVENT_IDX`
- **THEN** 轮询 owner MUST 通过消费 used ring
  更新该模式下的 `used_event`
- **AND** handler MUST NOT 把 `set_dev_notify` 当作有效 rearm
- **AND** 实现 MUST NOT 通过静默关闭 feature 获得通过

#### Scenario: cause 未清或 rearm 失效

- **WHEN** IRQ 持续重复但没有新设备事件
- **THEN** 中断风暴 Gate MUST 失败
- **AND** 诊断 MUST 区分 ack 与 rearm 失败

### Requirement: 中断计数支持分层诊断

MS03 MUST 提供单调的诊断计数。
计数至少覆盖 handler、used-ring、config-change、
ack、ack 后残留、unknown 和 spurious。
PLIC claim 与 EOI MUST 有可关联的观测记录。
rearm MUST 由两次独立事件之间的轮询消费和重复投递见证，
不得伪造无法直接观测的 rearm 计数。

#### Scenario: RX 事件归因

- **WHEN** 只向 guest 注入受控 RX 流量
- **THEN** IRQ 7、handler、used-ring 和 ack 计数 MUST 增长
- **AND** 证据 MUST 用事件时间线标记该次 RX 刺激

#### Scenario: TX completion 归因

- **WHEN** guest 只产生受控 TX completion
- **THEN** IRQ 7、handler、used-ring 和 ack 计数 MUST 再次增长
- **AND** 证据 MUST 用事件时间线标记该次 TX 刺激

#### Scenario: cause 不区分 RX 与 TX

- **WHEN** RX 和 TX 都表现为 used-ring update
- **THEN** 诊断 MUST 保留硬件 cause 的原始语义
- **AND** 实现 MUST NOT 创建不存在的 RX/TX cause 位

### Requirement: MS02 与 UART 恢复路径保持可用

MS03 MUST 保留 MS02 轮询数据面。
UART 迁移 MUST 保留现有 waker 和 copier 行为。
early 和 panic console MUST 不依赖设备 handler。

#### Scenario: 网卡 IRQ 初始化失败

- **WHEN** 网卡 IRQ 映射或注册失败
- **THEN** 网络 MUST 回退到 MS02 轮询路径
- **AND** 启动诊断 MUST 标记 IRQ baseline 不可用

#### Scenario: UART handler 初始化失败

- **WHEN** UART IRQ 10 注册失败
- **THEN** UART copier MUST NOT 启动
- **AND** 轮询 console MUST 继续提供诊断输出

#### Scenario: MS03 功能通过

- **WHEN** MS03 IRQ Gate 完成
- **THEN** MS01 与 MS02 网络回归 MUST 继续通过
- **AND** 网卡 waker、queue task 和 stack runner MUST 保持未引入

### Requirement: QEMU Evidence 有界且分通道

MS03 MUST 保存 QEMU 运行日志和诊断快照。
UART、PLIC、网卡和网络协议结果 MUST 分开判断。
运行见证 MUST 使用有界观察窗口。

#### Scenario: UART 与网络并发

- **WHEN** 串口输入输出与受控网络流量同时发生
- **THEN** UART 与网卡 handler MUST 各自推进
- **AND** 一个设备的 IRQ MUST NOT 增加另一设备的 handler 计数

#### Scenario: 运行见证中断

- **WHEN** QEMU 或宿主刺激在证据完成前终止
- **THEN** 对应 Gate MUST 标记为未完成
- **AND** 部分计数 MUST NOT 计为通过

#### Scenario: 证据范围声明

- **WHEN** MS03 QEMU Gate 通过
- **THEN** 结论 MUST 限定于当前 QEMU 设备模型
- **AND** 该结果 MUST NOT 作为 SMP 或真板证据
