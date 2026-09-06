## ADDED Requirements

### Requirement: 唯一常驻 queue owner 承载恢复生命周期

系统 MUST 在单 hart QEMU VirtIO-MMIO 上由同一个已激活 queue owner 执行正常服务、quiesce、reset 和 reinitialize。恢复生命周期 MUST 至少区分 `Active`、`Quiescing`、`Resetting`、`Reinitializing` 和 `Faulted`，状态提交后 MUST 通过既有事件协议唤醒相关 runner 和 waiter。恢复不得创建第二个 queue task、不得恢复 caller-driven descriptor polling，也不得在 `Service`、socket registry 或 driver guard 跨 `await`、`Pending` 或 wake。

#### Scenario: 成功恢复保持唯一 owner
- **WHEN** Active owner 收到合法 reset 请求并依次完成 quiesce、reset 确认、queue 重建和 RX refill
- **THEN** 同一个 owner MUST 以递增 epoch 回到 Active 并继续服务 RX/TX
- **AND** 系统 MUST NOT spawn 第二 queue task 或启用 polling fallback

#### Scenario: 恢复期间出现数据面事件
- **WHEN** used-ring、software slot、timer 或 config-change event 在任一恢复状态转换窗口到达
- **THEN** register-recheck 或等效提交后重检 MUST 使 owner 最终观察该事件
- **AND** 事件 MUST NOT 使旧状态重新成为 Active 或绕过当前阶段

#### Scenario: 恢复阶段失败
- **WHEN** 任一恢复阶段返回 fatal error 或达到 deadline
- **THEN** owner MUST 进入可观察 Faulted、唤醒相关 waiter 并拒绝新提交
- **AND** 系统 MUST NOT 通过第二 owner 或无界重试掩盖失败

### Requirement: Reset epoch 绑定 descriptor、cookie 与 ticket owner ledger

每次可服务 queue 实例 MUST 具有单调 reset epoch。TX cookie、software ticket、driver buffer slot 和 transport descriptor/token 的 owner ledger MUST 能证明其所属 epoch；transport token MUST 保持 driver-private。completion 只有在 epoch、cookie/ticket、descriptor slot 和当前 owner 状态全部匹配时才能完成 reclaim；stale、duplicate、unknown 或 mismatched completion MUST NOT 修改当前 epoch 的 ticket、flush target、buffer/descriptor 计数或 socket 结果。

#### Scenario: 当前 epoch 正常 completion
- **WHEN** epoch N 的合法 descriptor completion 携带与 ledger 匹配的 cookie/ticket
- **THEN** 系统 MUST 恰好一次关闭对应 device-owned owner 并把同一 buffer 返回可分配集合
- **AND** buffer、descriptor、ticket 和 completion telemetry MUST 守恒

#### Scenario: 旧 epoch completion 迟到
- **WHEN** epoch N 已关闭且 epoch N+1 已激活后观察或注入 epoch N 的 completion
- **THEN** completion MUST 被识别为 stale 且 MUST NOT 完成 N+1 的 ticket、释放 N+1 buffer 或满足 N+1 flush
- **AND** 该事件 MUST 产生含旧/当前 epoch 和 owner identity 的有界诊断

#### Scenario: 重复或未知 completion
- **WHEN** 同一 owner 已回收后再次收到相同 completion，或 token/cookie 在当前 ledger 中不存在
- **THEN** 系统 MUST NOT 重复释放资源或静默忽略 ownership 破坏
- **AND** owner MUST 进入稳定 Faulted quarantine 并唤醒受影响 waiter

#### Scenario: epoch 计数边界
- **WHEN** 测试 seam 将 epoch 推进到表示范围边界
- **THEN** 实现 MUST 通过不复用 live identity 的 checked policy 保持新旧 owner 可区分
- **AND** MUST NOT 用可能与 live epoch 冲突的静默 wrapping 作为恢复策略

### Requirement: 取消必须按 waiter、pre-submit 与 device-owned 分层

取消 MUST 根据 packet 当前 owner 分层处理。waiter cancellation MUST 只撤销调用者的等待注册；pre-submit cancellation MUST 从 software queue 中恰好一次移除对应 packet/ticket并返回稳定取消结果；device-owned packet MUST NOT 由普通 cancellation 回收，只能经 completion 或有界 quiesce/reset 关闭。任何 future drop MUST NOT 隐式释放 driver/device-owned backing。

#### Scenario: 丢弃 flush 或其他 waiter
- **WHEN** 等待 completion 的 future 被 drop、poll/select timeout 或调用者取消
- **THEN** 系统 MUST 只清除该 waiter identity 并保持 packet、ticket 和 buffer ownership 不变
- **AND** 后续 completion/reclaim MUST 仍能关闭原 owner

#### Scenario: 恢复流程取消尚未提交的 packet
- **WHEN** recovery 请求关闭当前 queue epoch 且 ticket 仍处于 software queue、从未被 driver 接受
- **THEN** 系统 MUST 原子移除该 packet、以 cancelled outcome 终结 ticket并使当前 flush/socket epoch 观察稳定错误
- **AND** packet MUST NOT 在新 epoch 自动重发或产生 descriptor owner

#### Scenario: 取消已经 device-owned 的 packet
- **WHEN** ticket 已提交给 driver/device 后收到取消
- **THEN** 普通取消 MUST NOT 释放 buffer、伪造 completion 或把 ticket 改回 queued
- **AND** 系统 MUST 等待合法 completion，或进入有 deadline 的 quiesce/reset 流程

#### Scenario: 取消与 submit 交错
- **WHEN** cancel 与 software queued 到 device-owned 的提交点交错
- **THEN** 同一锁保护或等效线性化点 MUST 决定 packet 只走 pre-submit cancel 或 device-owned quiesce 其中一条路径
- **AND** MUST NOT 同时取消并提交同一 ticket

### Requirement: 分阶段 deadline 与错误传播可诊断

系统 MUST 分别跟踪 submit wait、completion wait、reclaim、quiesce、reset confirmation 和 reinitialize deadline。timeout 或 fatal error MUST 至少携带 stage、epoch、owner 状态和未闭合资源摘要，MUST 唤醒所有受影响 waiter，并在 socket/syscall 边界映射为稳定可观察错误。同步 `submit_tx` 调用本身 MUST NOT 被伪装成可抢占的异步 timeout；submit deadline 表示 packet 等待被 driver 接受的阶段。

#### Scenario: submit wait timeout
- **WHEN** pre-submit packet 在 deadline 前始终无法被 driver 接受
- **THEN** ticket MUST 以 submit-stage timeout 终结且 packet MUST 从 software queue 恰好移除一次
- **AND** MUST NOT 创建虚假的 descriptor/cookie owner

#### Scenario: completion 或 reclaim timeout
- **WHEN** packet 已 device-owned 但 completion 未到达，或 completion 可见后 ledger 无法在对应 deadline 内闭合
- **THEN** 系统 MUST 区分 completion 与 reclaim stage 并进入 quiesce/reset 或稳定 Faulted
- **AND** waiter MUST 返回稳定错误而不是永久 Pending

#### Scenario: quiesce、reset 或 reinitialize timeout
- **WHEN** device-owned owners 无法收敛、device status 未读回 0，或 queue/RX refill 未在对应 deadline 完成
- **THEN** 系统 MUST 报告准确阶段并进入 Faulted
- **AND** 不确定是否仍被设备访问的 backing MUST 保留且新 submit MUST 被拒绝

### Requirement: VirtIO-MMIO 整设备 reset 必须确认停止后再重建

MS07 MUST 以未协商 `VIRTIO_F_RING_RESET` 的 VirtIO-MMIO 整设备 reset 为基线。driver MUST 停止新提交、隔离当前 epoch、写 device status 0，并在读回 status 0 后才释放或重新使用旧 queue、descriptor 和 buffer backing；随后 MUST 重新执行 feature negotiation、queue 建立、RX refill、notification arm 和 `DRIVER_OK`。若设备报告 `DEVICE_NEEDS_RESET`，系统 MUST 将在途请求视为结果不确定并进入同一恢复协议。

#### Scenario: 整设备 reset 成功
- **WHEN** quiesce 已建立隔离且 device 在 reset deadline 内读回 status 0
- **THEN** driver MUST 关闭旧 queue 对设备的可达性并以新 epoch 重建 transport queues、RX buffers 和通知状态
- **AND** 新 epoch Active 前 MUST 完成必要 feature/status 校验和资源守恒检查

#### Scenario: reset 未确认
- **WHEN** 写 status 0 后在 deadline 前始终未读回 0
- **THEN** driver MUST NOT drop、queue_unset 后释放或复用可能仍被设备访问的 backing
- **AND** owner MUST 保持 Faulted 隔离并拒绝新 I/O

#### Scenario: DEVICE_NEEDS_RESET
- **WHEN** task context 观察到 `DEVICE_NEEDS_RESET`
- **THEN** 系统 MUST NOT 假定任何在途请求已经完成或尚未完成
- **AND** MUST 进入带 epoch 隔离和 deadline 的整设备恢复协议

#### Scenario: reset 期间 IRQ 交错
- **WHEN** used-ring 或 config-change IRQ 与 reset 状态转换交错
- **THEN** ISR MUST 继续只做 cause snapshot、ack 和事件发布
- **AND** task context MUST 根据当前 recovery state/epoch 决定处理、记 stale 或仅重检，不得由 ISR 访问 descriptor owner ledger

### Requirement: Link config-change 形成独立控制面

VirtIO config-change IRQ MUST 在 ack 后发布给 task context；task context MUST 通过一致的 config snapshot 读取 `VIRTIO_NET_S_LINK_UP` 并提交 link generation/state。link down MUST 关闭当前 socket epoch、取消所有 pre-submit packet、阻止新的 software enqueue/driver submit并使 socket I/O 返回 `NotConnected`；已经 device-owned 的 packet MUST 继续由原 queue epoch completion/reclaim，只能用于资源闭合而不能证明 peer delivery。link up MUST 保持当前 queue epoch、推进 socket epoch并只允许新建 socket 恢复 I/O，不得自动触发 device reset。link transition MUST NOT 伪造 used-ring completion，也 MUST NOT 在 ISR 中读取或移动 descriptor。

支持link status的目标在唯一queue owner首次进入可服务状态时 MUST 由task context安排一次相同的一致snapshot读取，使当前link state不永久停留在unknown；`Again` MUST 保留为有界后续工作，不得自旋。首次unknown到up/down的提交 MUST 推进一次link generation，但 MUST NOT 伪造硬件config IRQ或used-ring completion。

#### Scenario: 初始 link snapshot
- **WHEN** 支持一致link读取的目标完成异步queue owner激活，且尚未收到config-change IRQ
- **THEN** owner MUST 在task context提交首个up/down snapshot，或在config generation竞争时保留一次有界重试
- **AND** 已提交Active诊断不得永久报告unknown，ISR telemetry不得把该初始化记作硬件config IRQ

#### Scenario: QEMU link down
- **WHEN** QEMU monitor 对当前 NIC 执行 `set_link net0 off` 并产生 config-change
- **THEN** ISR MUST ack/config-publish，task context MUST 观察 link down 并提交新 link generation
- **AND** 当前 socket epoch MUST 关闭，已排队但未提交的 packet MUST 取消，随后 send MUST 返回 `NotConnected`

#### Scenario: QEMU link up
- **WHEN** monitor 执行 `set_link net0 on` 且 task context 读取一致的 link-up config
- **THEN** 系统 MUST 发布 link-up event、推进 socket epoch并允许新建 socket 重新执行网络 I/O
- **AND** MUST NOT reset 设备、创建第二 queue owner或令已经终止的旧 socket 恢复

#### Scenario: config-change 与 used-ring 同时发生
- **WHEN** 同一次或相邻 IRQ 同时包含 config-change 和 used-ring cause
- **THEN** 两类事件 MUST 都被 ack、记录并最终由 task context 处理
- **AND** config path MUST NOT 替代 RX/TX completion ledger

### Requirement: Socket 错误按数据面 epoch 隔离

每个 public socket MUST 绑定创建时的数据面 epoch。reset/fatal recovery 关闭 epoch N 时，所有 N 的现有 TCP/UDP/listener handle MUST 在 wake 前获得稳定 terminal error，随后 I/O 和 readiness MUST 一致报告该错误；成功建立 epoch N+1 后新建 socket MUST 不受 N 的 registry-wide terminal 污染。既有 TCP 连接、listener 和已排队 datagram MUST NOT 跨 reset 透明续传或自动重发。

#### Scenario: 旧 socket 在 reset 后使用
- **WHEN** epoch N 的 socket 在成功恢复到 N+1 后执行 poll、read、write、connect、accept 或 flush
- **THEN** readiness MUST 报告 terminal/ERR 且紧随其后的操作 MUST 返回匹配稳定错误
- **AND** socket MUST NOT 使用 N+1 device queues 继续旧连接

#### Scenario: 新 epoch 创建 socket
- **WHEN** epoch N+1 已 Active 后应用创建新的 TCP、UDP 或 listener socket
- **THEN** 新 handle MUST 绑定 N+1 且不继承 N 的 terminal code
- **AND** 它 MUST 能通过既有 stack runner/readiness 路径完成正常网络 I/O

#### Scenario: reset 提交错误与 waiter wake 顺序
- **WHEN** recovery 决定关闭当前 socket epoch
- **THEN** terminal code 和 epoch closure MUST 在任何 bridge wake 之前提交
- **AND** 多 waiter、overflow 和 socket removal MUST 保持 MS06 的无 lost-wakeup 语义

### Requirement: 故障注入、回归与结论边界

MS07 MUST 以 host/model tests 覆盖 owner lifecycle、epoch ledger、stale/duplicate completion、三层取消、所有 timeout stage、reset success/failure、link state 和 socket epoch；单 hart QEMU VirtIO-MMIO runtime MUST 覆盖真实整设备 reset、`set_link net0 off/on`、reset 前后新旧 socket 结果、双向流量和资源守恒。真实 QEMU 不必产生规范禁止的旧 queue completion，此项 MUST 由 fake transport/model injection 证明。既有 MS01、MS04、MS05、MS06 受影响 Gate MUST 回归，历史 evidence 缺口不得自动提升为 PASS。

runtime资源守恒 MUST 使用driver定义的owner分类。VirtIO健康空闲态的`device_owned`包含常驻RX queue owners，MUST NOT以`device_owned==0`作为idle或资格条件；probe和validator MUST 校验同阶段capacity、owner分类、quarantine及epoch关系。guest peer路径失败 MUST 保留失败的具体syscall阶段与errno，缺少该证据时不得把“peer未收到包”归因为UDP产品缺陷。nonblocking send的`EAGAIN/EWOULDBLOCK` MUST 视为共享absolute deadline内可重试的背压：每次重试前重新等待writable且不得sleep-spin；其他errno MUST 保留并停止猜测修复。若probe自身发生user fault，runtime结论 MUST 同时记录faulting user PC与fault VA并对齐exact ELF，不能把fault VA当作PC或继续使用不可信payload作资格证据。

change-local probe以零`nfds`的poll timeout执行有界采样等待时，kernel MUST 忽略`fds`参数并在timeout到期后返回0；正`nfds`的用户范围校验 MUST 保持。实现 MUST NOT通过从NULL构造Rust slice、改用独立sleep预算或probe特判来伪造该行为。

#### Scenario: 零 fd 集合的有界 poll 等待

- **WHEN** change-local RISC-V probe调用`poll(NULL, 0, finite_timeout)`，其libc包装进入`ppoll`
- **THEN** kernel MUST 忽略`fds`、按既有timer路径有界等待并返回0
- **AND** `nfds>0`且`fds`为NULL或不可访问时 MUST 继续返回`EFAULT`

#### Scenario: nonblocking peer send背压
- **WHEN** guest peer socket在writable等待后执行nonblocking send且返回`EAGAIN/EWOULDBLOCK`
- **THEN** probe MUST 使用同一phase absolute deadline重新等待writable并有界重试
- **AND** deadline耗尽 MUST 报告send阶段和最终errno，不得busy loop或改写为UDP产品故障

#### Scenario: guest probe用户态页故障
- **WHEN** change-local probe在资格流程前或流程中发生不可处理user page fault
- **THEN** raw serial MUST 至少记录exact payload对应的user PC、fault VA、access、SP与RA，并用program headers和PC附近反汇编定位首个未解释执行边
- **AND** 在artifact/runtime不匹配、PC未定位或需要通用loader修复时 MUST 停止资格流程并返回Plan

#### Scenario: 健康 VirtIO 双向 owner 基线
- **WHEN** VirtIO RX queue已填充`QS`个buffer且TX无在途请求
- **THEN** owner summary MUST 报告`available=QS`、`device_owned=QS`、`quarantined=0`或与实际固定capacity等价的关系
- **AND** probe、validator与negative fixtures MUST NOT要求健康态`device_owned==0`或`available+device_owned+quarantined<=QS`

#### Scenario: Host/model fault matrix
- **WHEN** fake transport 分别注入 stale/duplicate token、status reset 延迟或失败、completion/reclaim stall、config generation 变化和 cancel/submit 交错
- **THEN** 每项 MUST 命中指定 stage/state 且证明无 UAF、重复回收、owner 混用、永久 Pending或静默丢包
- **AND** fault matrix MUST 能独立定位到 epoch ledger、cancel、deadline、reset 或 link 控制面

#### Scenario: 单 hart QEMU reset 与 link flap
- **WHEN** change-local probe 在单 hart QEMU VirtIO-MMIO 上完成 reset 前流量、受控 reset、reset 后新 socket 流量及 link off/on
- **THEN** raw serial MUST 包含环境、阶段 marker、epoch/ledger 摘要、旧 socket terminal、新 socket成功和明确退出码；不得要求revision/hash/run-id等内容身份绑定
- **AND** panic、trap、fatal ownership drift、validator mismatch 或任一 workload failure MUST 判定失败

#### Scenario: 兼容性回归
- **WHEN** 未注入 reset、stall 或 link flap并运行受影响的 MS01、MS04、MS05、MS06 Gate
- **THEN** 既有单 owner、最小 ISR、EVENT_IDX、budget、quiet path、C4 flush、stack runner、socket readiness 和应用结果 MUST 保持通过
- **AND** MS07 MUST NOT 声明 SMP、PCI、DWMAC、真板 reset/DMA 停止、自动 polling fallback或性能资格
