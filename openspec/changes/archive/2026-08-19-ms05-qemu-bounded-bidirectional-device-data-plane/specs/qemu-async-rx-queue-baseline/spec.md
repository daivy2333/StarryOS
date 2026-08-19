## MODIFIED Requirements

### Requirement: RX queue control 与 transport 解耦

MS05 MUST 将现有 transport-neutral `NetQueueControl` 演进为同时表达 RX/TX completion 可见性、按方向通知抑制、通知重臂和重臂后 recheck 的 queue control，并继续通过 `NetDriverOps` 或等效 typed operation 执行 RX reap/refill 与 TX submit/reclaim。公共接口 MUST NOT 暴露 VirtIO available/used ring、descriptor token、MMIO 寄存器或 DWMAC descriptor 类型。VirtIO-MMIO 与 DWMAC 设备模型审查 MUST 能把各自的 RX/TX 通知和 ownership 语义映射到该 contract；MS05 MUST NOT 引入 DWMAC 产品代码。

#### Scenario: VirtIO-MMIO 适配 queue contract

- **WHEN** VirtIO-MMIO 的 RX 或 TX used ring 出现 completion
- **THEN** 适配层 MUST 通过 direction-aware queue control 向唯一 queue service 报告对应工作
- **AND** transport-specific token 和 ring 字段 MUST 留在 VirtIO 后端

#### Scenario: 共享 IRQ 无法提供方向信息

- **WHEN** VirtIO-MMIO used-ring IRQ 只能说明至少一个 queue 可能有 completion
- **THEN** ISR MUST 发布通用 queue event
- **AND** queue service MUST 通过 transport-neutral readiness mask 分别检查 RX/TX

#### Scenario: 使用 DWMAC 模型审查 contract

- **WHEN** 以 DWMAC 的 RX/TX descriptor ownership 与 IRQ mask/rearm 模型审查 queue control
- **THEN** 每项公共语义 MUST 能在不引用 VirtIO ring 布局的前提下解释
- **AND** 无法表达的 transport 差异 MUST 阻塞 Gate 2，不得留给 Act 扩展 trait

### Requirement: RX queue owner 唯一且切换有边界

系统 MUST 在任一时刻只允许一个 owner 推进同一硬件 RX queue，也 MUST 只允许一个 owner 推进同一硬件 TX queue。MS05 激活后，当前单 VirtIO-MMIO NIC 的 RX/TX descriptor service MUST 由同一个唯一、长驻的 queue service 执行；caller-driven `Service` 只能通过 RX/TX packet slots 交换 frame，不得直接调用 descriptor reap、refill、submit 或 reclaim。初始化失败若发生在 owner 切换前 MAY 保留既有基线；切换后 fatal MUST 保持 faulted queue service 为唯一 owner，不得自动创建 polling fallback 或第二 task。

#### Scenario: 异步 RX 成功激活

- **WHEN** 通用 IRQ event、direction-aware queue control、RX/TX slots 和 queue service 全部准备完成
- **THEN** RX/TX hardware queue owner MUST 收口为唯一 queue service
- **AND** socket 调用、Router、10ms fallback 或其他 task MUST NOT 直接推进任一 descriptor queue

#### Scenario: 激活前初始化失败

- **WHEN** slot 构造、queue control 或 queue service 启动在 owner 切换前失败
- **THEN** 系统 MUST 保持已有 MS04 RX 与同步 TX 基线，或在无法安全保持时停止初始化并报告阶段
- **AND** MUST NOT 留下 RX 已切换而 TX 未切换的未声明半激活状态

#### Scenario: 激活后发生 fatal queue error

- **WHEN** queue service 在已取得双向 ownership 后观察到 descriptor、token 或 buffer fatal error
- **THEN** 数据面 MUST 进入可观察 fault 状态并停止无界重试
- **AND** flush waiter MUST 被唤醒并收到稳定错误
- **AND** 系统 MUST NOT 自动启动另一 owner 并发访问任一 hardware queue

### Requirement: ISR 与 AtomicWaker 保持最小和 IRQ-safe

VirtIO-net ISR MUST 只读取和分类 cause、执行 device ack、保存有界 snapshot、更新 telemetry 并唤醒通用 queue service。ISR MUST NOT 推断 used-ring completion 属于 RX 或 TX，MUST NOT 读取、移动或回收 descriptor，MUST NOT 获取 axnet `Service` 锁，MUST NOT 运行协议栈。`AtomicWaker` 使用的 critical-section MUST 恢复进入临界区前的 IRQ enable 状态，ISR 中 `wake()` MUST NOT 在 handler 返回和 PLIC complete 前提前开启 IRQ。

#### Scenario: used-ring IRQ 唤醒 queue task

- **WHEN** IRQ 7 的 cause 包含 used-ring update
- **THEN** handler MUST ack cause、记录通用 queue event 并唤醒唯一 queue service
- **AND** RX/TX completion 分类与 descriptor service MUST 只在 task context 发生

#### Scenario: ISR 中调用 AtomicWaker

- **WHEN** CPU 在 IRQ disabled 状态进入 `AtomicWaker::wake()` 的 critical-section
- **THEN** critical-section release 后 IRQ MUST 仍保持 disabled
- **AND** PLIC complete 前 MUST NOT 因该 critical-section 恢复而允许嵌套设备中断

#### Scenario: task 上下文注册 waker

- **WHEN** queue service 在 IRQ enabled 或 disabled 的任一进入状态注册 waker
- **THEN** critical-section 退出后 MUST 恢复相同的进入状态
- **AND** UART 与 MS04 RX 现有 `AtomicWaker` 竞态回归 MUST 继续通过

#### Scenario: Config-only 或未知 IRQ

- **WHEN** handler 收到 config-only、unknown-only 或零 cause
- **THEN** 它 MUST 保持既有分类和 ack 规则
- **AND** MUST NOT 伪造 RX/TX completion 或启动持续 queue-service 进度

### Requirement: Register-recheck 关闭 lost-wakeup 窗口

queue service MUST 按“检查 RX/TX completion 与 slot 工作、注册 waker、按方向重臂通知、再次检查 completion、slot 与事件代次”的顺序进入 `Pending`。硬件 IRQ 和软件 slot 事件 MUST 汇入通用事件代次；发布和观察 MUST 使用与角色一致的原子内存序。任一事件发生在注册前、注册期间、任一 queue 重臂后或返回 `Pending` 前时，task MUST 最终观察到工作或再次被调度。

#### Scenario: event-before-register

- **WHEN** 任一方向 completion 或 slot 事件在 queue service 注册 waker 之前到达
- **THEN** 注册后的再次检查 MUST 观察到工作或已变化的事件代次
- **AND** task MUST NOT 返回永久 `Pending`

#### Scenario: register-during-event

- **WHEN** ISR/software wake 与 task 注册 waker 并发交错
- **THEN** task MUST 通过 AtomicWaker 或事件代次重新获得运行机会
- **AND** 每个 completion 与 slot packet MUST 只由其唯一 owner 消费一次

#### Scenario: rearm 后事件到达

- **WHEN** task 已重臂一个 used ring，而另一个 used ring 或 slot 在返回 `Pending` 前变为 ready
- **THEN** 最终 recheck MUST 观察到该工作或通用事件变化
- **AND** task MUST 继续服务而不是依赖 10ms fallback

#### Scenario: Slot Full 后容量恢复

- **WHEN** queue service 因 RX slot Full 或上游因 TX slot Full 停止推进，随后消费者释放一个 slot
- **THEN** 满到非满转换 MUST 发布软件事件
- **AND** 等待方 MUST 通过 register-recheck 协议恢复而不忙等

### Requirement: `RING_EVENT_IDX` 通知抑制与重臂有效

VirtIO-MMIO 后端 MUST 保留已协商的 `RING_EVENT_IDX`。RX 与 TX used-buffer completion 的通知抑制和重臂 MUST 分别控制对应 queue 的 `used_event` 或等效规范机制；实现 MUST NOT 把 `set_dev_notify` 在 `event_idx=true` 时的 no-op 当作有效控制，也 MUST NOT 通过关闭 feature 取得通过。TX driver-to-device notify 的 `avail_event` 判断 MUST 使用 wrap-safe 的 old/new event 语义，不能用不含旧 index 的普通大小比较替代。对 registry 依赖的扩展 MUST 由工作区拥有的 adapter、vendor 或 Cargo patch 承载，MUST NOT 原地修改 Cargo registry。

#### Scenario: queue task 开始有界 drain

- **WHEN** queue service 因通用 used-ring event 开始服务 completion
- **THEN** VirtIO 后端 MUST 对待服务的 RX/TX used ring 抑制新通知或设置等效阈值
- **AND** 持续双向 burst MUST NOT 造成每个 descriptor 一次无界 IRQ 重入

#### Scenario: 空队列准备 Pending

- **WHEN** queue service 已服务至无立即工作并准备等待
- **THEN** 后端 MUST 在 `RING_EVENT_IDX` 模式下分别重臂 RX 与 TX 下一次 completion 通知
- **AND** 每次重臂后及最终返回前 MUST 执行 completion、slot 与事件代次 recheck

#### Scenario: TX avail_event 跨 u16 wrap

- **WHEN** TX available index、旧 index 或 device 提供的 `avail_event` 跨越 `u16::MAX`
- **THEN** driver-to-device notify 判定 MUST 使用 wrapping event 公式
- **AND** MUST NOT 因普通 `>=` 比较造成错误抑制或持续多余通知

#### Scenario: 依赖不暴露有效 EVENT_IDX 控制

- **WHEN** 当前依赖无法在不关闭 `RING_EVENT_IDX` 的条件下分别控制 RX/TX completion 通知
- **THEN** 实施 MUST 停止并返回 Plan
- **AND** 不得以 raw 私有字段穿透、registry 原地修改或 feature 降级继续

### Requirement: RX queue task 以有界 budget 推进

唯一 queue service MUST 为 TX reclaim、RX completion/refill 和 TX submit 设置独立、固定且可观察的每轮 budget。每个阶段 MUST NOT 超过自己的 budget；任一阶段 budget 用尽且仍有工作时，task MUST 保持相关通知抑制、自调度并让出 CPU，不得在一次 poll 中无界 drain。阶段顺序 MUST 先释放已完成 TX 资源，再推进 RX，再提交新 TX；固定顺序结合独立 budget MUST 保证持续双向负载下两个方向均可推进。纯 telemetry MAY 使用 Relaxed；事件、owner、ticket 和 flush 状态 MUST 使用与同步角色一致的 ordering 或锁保护。

#### Scenario: completion 数量小于等于 budget

- **WHEN** TX completion、RX completion 与待提交 TX frame 均不超过各自 budget 且 slots 有容量
- **THEN** task MUST 在同轮按 reclaim、RX、submit 顺序完成对应工作
- **AND** 无工作后 MUST 进入双向 register-recheck

#### Scenario: budget exhausted

- **WHEN** TX reclaim、RX completion 或 TX submit 中任一 backlog 超过该阶段 budget
- **THEN** task MUST 精确停止在该阶段 budget 边界并记录对应 exhaustion
- **AND** task MUST 通过软件调度继续且至少让出一次 CPU
- **AND** 其他方向和其他 runnable task MUST 获得调度机会

#### Scenario: 持续双向负载

- **WHEN** RX completion 与 TX slot backlog 在多个调度轮次中同时非空
- **THEN** RX delivered/refilled 与 TX submitted/reclaimed counter MUST 均持续增长
- **AND** 任一方向不得因固定阶段顺序永久饥饿

#### Scenario: spurious 或 config-only IRQ

- **WHEN** task 因 spurious/config-only IRQ 或 software nudge 醒来且两个 used ring 与 slots 均无可推进工作
- **THEN** task MUST 至多执行一次有界无工作检查后返回等待
- **AND** MUST NOT 自唤醒形成 busy loop 或伪造 descriptor 进度

### Requirement: 使用现有 Router 缓冲完成临时 RX handoff

MS05 MUST 用最终固定容量 RX/TX Ethernet frame slots 替换 MS04 queue task 到 Router 的临时直接 RX handoff。queue service MUST 只在 raw driver 与 frame slots 之间移动 packet；Ethernet/ARP/IPv4 处理和现有 Router 交互 MUST 回到 caller-driven `Service` 上下文。由 RX 触发的 ARP reply 或 pending packet flush MUST 通过 TX slot 和 typed outcome 交付，不得直接调用同步 descriptor submit。独立 smoltcp stack runner 与 socket readiness 仍属于 MS06。

#### Scenario: IPv4 frame 完成临时 handoff

- **WHEN** queue service reap 一个合法 RX completion 且 RX slot 有空间
- **THEN** 完整 Ethernet frame MUST 被复制进 RX slot
- **AND** descriptor buffer MUST 在本次有界服务中 refill
- **AND** queue service MUST NOT 运行 Ethernet/ARP/IP 或 smoltcp 处理

#### Scenario: Router RX 缓冲已满

- **WHEN** 最终 RX slot 没有可用空间
- **THEN** queue service MUST 在 reap 下一个 completion 前停止
- **AND** 已完成 handoff 的 descriptor MUST 已被 refill
- **AND** 未 reap completion 的 ownership MUST 保持在 VirtIO queue

#### Scenario: 协议栈释放 Router 空间

- **WHEN** 现有 socket 调用或其他既有入口执行 `poll_interfaces()` 且 RX slot 非空
- **THEN** Ethernet adapter MUST 在该上下文消费 frame 并执行既有 Ethernet/ARP/IPv4 与 Router handoff
- **AND** 消费导致 slot 从满变为非满时 MUST 发布 queue software event

#### Scenario: Service 生成 TX frame

- **WHEN** Router dispatch、ARP reply 或 ARP pending flush 生成完整 Ethernet frame
- **THEN** Ethernet adapter MUST 通过 typed outcome 尝试写入 TX slot
- **AND** Full 时 MUST 保留对应上游 packet 或 pending entry
- **AND** MUST NOT 直接推进 VirtIO TX descriptor

#### Scenario: Descriptor 不跨 await

- **WHEN** queue service 因 slot Full、队列空或 budget 用尽返回 `Pending` 或让出
- **THEN** 所有 raw descriptor、token 与 `NetBufPtr` MUST 仍由 driver/queue service 的明确状态持有
- **AND** frame slot 与 stack side MUST 只观察普通复制 packet

### Requirement: 兼容性、验证顺序与 Evidence

MS05 MUST 保持 MS04 最小 ISR、RX ownership、register-recheck、EVENT_IDX、snapshot/idle/nudge/burst 判定，保持 MS01/MS02 socket 行为、MS03 IRQ 分类、UART async 路径以及 early/panic console。现有 MS04 probe/schema 的 RX 字段 MUST 保持可读取，或新增明确版本且提供同一 change 内的兼容解析。所有 host/model tests、target build、静态检查、竞态/ownership 测试和可自动执行的 QEMU 准备 Gate MUST 先在当前环境尝试；产品失败 MUST 停止下游任务。QEMU TX-only、双向、Full→恢复、flush、网络功能与受影响的 R51 四模式 MUST 保存 change-local 原始 Evidence；历史 WAIVED/SKIPPED 项不得因本 change 自动提升为 PASS。

#### Scenario: 自动产品 Gate 未通过

- **WHEN** 任一 host、build、静态、ownership、flush 或自动竞态 Gate 出现编译、链接、断言、source、validation 或 diff 失败
- **THEN** iteration MUST 停止在对应任务
- **AND** MUST NOT 以手动 QEMU 或普通网络成功绕过该失败

#### Scenario: 自动 Gate 被 sandbox 环境阻塞

- **WHEN** 自动命令失败且原始日志明确定位到只读路径、禁止联网或安装、`EPERM`、`SIGSYS`、`Bad system call` 或用户终端/权限边界
- **THEN** 对应项 MUST 标记 `ENV-BLOCKED` 并记录原命令、退出码和最早环境失败层
- **AND** 无法区分环境与产品原因时 MUST 按产品 Gate 失败处理

#### Scenario: 执行 iteration 最后的手动验收

- **WHEN** 所有自动产品 Gate 已通过并执行 QEMU runtime
- **THEN** TX-only、双向、Full→恢复、flush C4、descriptor/buffer 守恒、网络功能与 R51 四模式 MUST 分项记录
- **AND** 串口原始日志、probe 输出、环境、命令、退出状态、revision 和完成 marker MUST 写入 change-local Evidence

#### Scenario: QEMU Evidence 不完整

- **WHEN** 串口日志、probe 输出、环境信息、退出状态或完成 marker 任一缺失且未获用户明确豁免
- **THEN** 对应 runtime Gate MUST 标记为未完成或失败
- **AND** 部分 telemetry MUST NOT 计为 MS05 通过

#### Scenario: 原始日志与 revision provenance 不可判定

- **WHEN** raw terminal Evidence 含 ANSI、CRLF 或终端行尾空格，或采集 HEAD、Act 基线与最终 Review revision 不同
- **THEN** source/document whitespace Gate MUST 排除 raw Evidence，并分别验证日志完整性与 revision provenance
- **AND** Evidence 索引 MUST 记录每个 revision 的角色和采集时间，不得把矛盾元数据或未纳入检查的日志声明为完整 diff PASS

#### Scenario: 历史 waiver 不自动转为 PASS

- **WHEN** MS05 重跑 R51 或网络功能但未完整执行 MS04 曾豁免的 compatibility、boot signature、termination metadata 或 exact-binary 项
- **THEN** 这些项目 MUST 继续标记 WAIVED/SKIPPED 或未验证
- **AND** 新 TX/RX 证据 MUST NOT 被描述为补齐了未执行的历史范围

#### Scenario: 测量窗口前已有安全失败

- **WHEN** probe 的 PRE snapshot 已包含非零 ownership fault、IRQ restore violation、IRQ-enabled entry、buffer leak 或 flush fault
- **THEN** 对应 snapshot、idle、nudge、burst、TX 或双向模式 MUST 失败
- **AND** 后续安静窗口 MUST NOT 掩盖同次启动已经发生的失败

#### Scenario: idle 或 nudge 出现额外进度

- **WHEN** idle 窗口出现 ISR、software-nudge、descriptor、budget、yield 或 slot backpressure 进度，或 nudge 窗口出现约定三项之外的进度
- **THEN** 对应模式 MUST 输出 FAIL 并返回非零
- **AND** nudge 的允许进度 MUST 仅为 software-nudge `+1`、task poll `+1` 和 empty check `+1`

#### Scenario: 稳定快照在 deadline 后才相等

- **WHEN** 两次 progress snapshot 相等，但第二次观察已达到或超过固定 deadline
- **THEN** probe MUST 把结果判为 timeout/FAIL
- **AND** MUST NOT 因相等检查先于 deadline 检查而接受过期 snapshot

#### Scenario: 证据范围声明

- **WHEN** MS05 QEMU Gate 全部通过
- **THEN** 结论 MUST 限定于当前单 hart VirtIO-MMIO 环境的双向设备数据面
- **AND** MUST NOT 声明独立 stack runner、准确 socket readiness、reset、SMP、PCI、DWMAC、真板或性能资格已验证
