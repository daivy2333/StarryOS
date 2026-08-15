## ADDED Requirements

### Requirement: 固定容量的二层 RX/TX packet slots

MS05 MUST 在现有 caller-driven `Service` 与唯一 queue service 之间为每个 QEMU VirtIO-MMIO NIC 建立独立的 RX 和 TX Ethernet frame slot。每个方向的容量 MUST 固定为 64 个完整 frame，并在设备初始化时预分配；数据路径 MUST NOT 通过动态扩容隐藏压力。slot MUST 只持有普通 frame 数据和必要的 packet metadata，MUST NOT 暴露或持有 VirtIO token、descriptor handle、`NetBufPtr` 或 transport-specific 状态。每次 slot 操作 MUST 以一个完整 frame 为原子单位。

#### Scenario: TX frame 被 slot 接受

- **WHEN** TX slot 有容量且 Ethernet adapter 生成一个合法完整 frame
- **THEN** slot MUST 原子接受整个 frame 并返回唯一接受 ticket
- **AND** slot occupancy 与 high-water telemetry MUST 反映本次转移
- **AND** queue service 之外的代码 MUST NOT 获得对应 descriptor 或 `NetBufPtr`

#### Scenario: TX slot 已满

- **WHEN** TX slot occupancy 已达到 64 且上游尝试交付下一 packet
- **THEN** slot MUST 返回可恢复 Full 且 MUST NOT 部分复制该 packet
- **AND** 上游 MUST 保留原 packet 的 ownership
- **AND** 内存占用 MUST 保持在已配置上界内

#### Scenario: RX slot 已满但 used ring 有 completion

- **WHEN** RX slot occupancy 已达到 64 且 RX used ring 仍有 completion
- **THEN** queue service MUST 在 reap 下一个 completion 前停止 RX 服务
- **AND** 未 reap completion 及其 buffer ownership MUST 留在硬件队列
- **AND** task MUST NOT 持有已 reap 但未交付的 `NetBufPtr` 跨越 `Pending`

#### Scenario: Slot 从满变为非满

- **WHEN** stack side 消费一个满 RX slot，或 queue service 消费一个满 TX slot
- **THEN** 对应消费者 MUST 发布一次有界软件空间事件
- **AND** 因 slot Full 暂停的 owner MUST 通过统一 queue event 获得重新运行机会

### Requirement: Typed TX handoff 与可恢复背压

设备发送边界 MUST 使用 typed outcome 区分 `Accepted`、可恢复 `Full`、带稳定原因的 `Dropped` 和 fatal `DevError`。现有“发送前先 dequeue、失败仅记录 warning”的行为 MUST 被替换：Router 或等效调用者 MUST 在下游返回 `Accepted` 或明确 `Dropped` 后才提交 dequeue；返回 `Full` 时 MUST 保留队首 packet 并停止本轮 dispatch。现有 loopback 的 RX-ready 信号 MUST 与 TX disposition 分离表达。

#### Scenario: 下游接受 packet

- **WHEN** Device adapter 将完整 packet 接受进 TX slot 或合法的 ARP pending buffer
- **THEN** 它 MUST 返回 `Accepted`
- **AND** Router MUST 只在观察到该结果后移除原 packet

#### Scenario: 下游暂时无容量

- **WHEN** TX slot、正常 descriptor 资源或 ARP pending buffer 暂时耗尽
- **THEN** Device adapter MUST 返回可恢复 `Full` 或底层非阻塞 `Again`
- **AND** 该状态 MUST NOT 映射为 `BadState`、`NoMemory` 或静默 drop
- **AND** 上游 MUST 保留 packet 以供容量恢复后重试

#### Scenario: 策略明确丢弃 packet

- **WHEN** 实现依据本 spec 明确允许的 packet policy 丢弃一个已检查 packet
- **THEN** Device adapter MUST 返回 `Dropped` 和稳定 `TxDropReason`
- **AND** drop reason counter MUST 精确增加一次
- **AND** 无 reason 的 warning-only 丢弃 MUST 失败

#### Scenario: Loopback 发送产生 RX readiness

- **WHEN** loopback device 接受 packet 并使本地 RX 可读
- **THEN** outcome MUST 同时表达 `Accepted` 与 `rx_became_ready=true`
- **AND** Ethernet device 的普通 `Accepted` MUST NOT 伪造该 RX readiness

### Requirement: TX descriptor ownership、completion 与 buffer 守恒

唯一 queue service MUST 按 `slot-queued → buffer-prepared → device-owned → completed → reclaimed` 状态推进每个 TX packet。只有成功加入 hardware available ring 后才能进入 `device-owned`；只有从 used ring 回收匹配 token 并使 buffer 可复用后才能达到 C4 `reclaimed`。每个 token、buffer 和 ticket MUST 同时属于且只属于一个状态。正常 queue/buffer exhaustion MUST 可恢复；oversize、submit error 和 reclaim error MUST NOT 使 driver 的长期可用 TX buffer 数量静默缩小。transport 接受 buffer 前的 submit error MUST 把 buffer 恢复到可分配集合；接受后才发现的 token/ledger invariant MUST 由 driver 保留该 buffer、返回稳定 fatal 并停止后续 submit，不得 panic、释放 device-owned buffer或谎报为可恢复 Full。

#### Scenario: 成功提交和完成 TX

- **WHEN** queue service 从 TX slot 观察到队首 frame 且 driver 有实际可提交容量
- **THEN** frame MUST 复制进一个 driver buffer 并成功提交为 device-owned
- **AND** TX slot MUST 只在成功提交后移除对应 frame
- **AND** completion MUST 使用匹配 token 回收同一 buffer 并发布 ticket 的 C4 状态

#### Scenario: Descriptor queue full

- **WHEN** driver 尝试提交 frame 但 hardware queue 没有足够 descriptor
- **THEN** driver MUST 返回非阻塞 `Again` 或等效 Full
- **AND** 准备中的 buffer MUST 回到可用 TX buffer 集合
- **AND** TX slot 队首 frame MUST 保持未提交状态

#### Scenario: Oversize 或 submit error

- **WHEN** TX frame 超过 driver buffer capacity，或 `transmit_begin` 在取得 buffer 后返回错误
- **THEN** driver MUST 恢复该 buffer 到可再次分配的 TX 集合
- **AND** 错误前后的总 buffer 守恒 MUST 可由测试观察
- **AND** 重复该错误 MUST NOT 逐次降低后续可提交容量

#### Scenario: Token 或 ownership 不变量破坏

- **WHEN** completion token 没有对应 device-owned buffer、token slot 将被覆盖，或同一 buffer 被重复回收
- **THEN** queue service MUST 把该事件分类为 fatal ownership error
- **AND** MUST 停止无界重试、进入可观察 fault 并唤醒 completion waiter

#### Scenario: Readiness 与实际提交容量一致

- **WHEN** driver 报告 TX ready 或 not ready
- **THEN** 该结果 MUST 与下一次单 owner `transmit_begin` 是否能接受当前连续 TX buffer 一致
- **AND** descriptor 数量、header 布局或 EVENT_IDX 模式差异 MUST NOT 造成永久假 ready 或假 Full

### Requirement: Ticketed C4 flush

MS05 MUST 为每个被 TX slot 接受的 packet 分配单调 ticket，并提供仅供设备数据面内部使用的 flush。flush MUST 捕获调用时的最高已接受 ticket，并等待所有不大于 target 的 ticket 达到 C4 reclaim；调用之后接受的 ticket MUST NOT 阻塞该 flush。实现 MUST NOT 用“全局 TX queue 为空”或“最后观察到某个 completion”代替目标集合判定，也 MUST NOT 假定 completion 按提交顺序到达。flush 成功 MUST 只表示 driver buffer 可安全复用，不表示 peer、TCP 或应用完成。

#### Scenario: 空数据面 flush

- **WHEN** 调用 flush 时没有任何未终结的已接受 TX ticket
- **THEN** flush MUST 立即返回成功
- **AND** MUST NOT 依赖硬件 IRQ 或周期轮询

#### Scenario: Slot 和 descriptor 中均有 target packet

- **WHEN** flush target 覆盖仍在 TX slot 和已经 device-owned 的多个 ticket
- **THEN** flush MUST 等待这些 ticket 被提交、完成并回收到 C4
- **AND** 不大于 target 的任一 ticket 未终结时 MUST NOT 提前成功

#### Scenario: Flush 后继续接受 packet

- **WHEN** flush 已捕获 target 后 TX slot 又接受更大的 ticket
- **THEN** 新 ticket MUST NOT 延迟旧 flush 的成功
- **AND** 后续 packet MUST 继续按普通 queue service 规则推进

#### Scenario: Completion 乱序

- **WHEN** target 范围内的 completion 以不同于提交顺序的顺序返回
- **THEN** flush MUST 依据有界 outstanding ticket 集合确认所有 target 已终结
- **AND** 单一最大完成序号 MUST NOT 掩盖仍未完成的较小 ticket

#### Scenario: Fatal error 或第二 waiter

- **WHEN** target ticket 发生 fatal submit/reclaim error，或已有一个 flush waiter 时出现第二个内部 waiter
- **THEN** 第一个 flush MUST 被唤醒并返回稳定错误
- **AND** 第二个 waiter MUST 返回 `ResourceBusy`
- **AND** MUST NOT 覆盖第一个 waiter 的 waker

#### Scenario: Flush future 被取消

- **WHEN** 等待中的 flush future 被 drop
- **THEN** 取消 MUST 只停止该 waiter 的等待
- **AND** 已接受或 device-owned packet MUST 继续推进且 MUST NOT 被隐式取消、重发或提前回收

### Requirement: Packet 语义与 telemetry 可解释

MS05 MUST 分层定义 packet slot、socket byte-stream 和 datagram 语义。RX/TX slots MUST 全有或全无地接受一个 frame；TCP MUST 保持现有 byte-stream short-write 行为；UDP MUST 保持 datagram 原子性。MS05 的“背压可见” MUST 限定为 slot、Device adapter、queue service 和 telemetry 可见，MUST NOT 声明 fd `POLLOUT/EAGAIN` 已与 hardware capacity 精确一致。telemetry MUST 至少覆盖 slot occupancy/high-water/full、稳定 drop reason、TX submit/completion/reclaim、buffer/descriptor 守恒、各方向 budget/self-yield 和 flush target/result。

#### Scenario: Packet slot 不接受部分 frame

- **WHEN** slot 剩余容量不足以接受下一个完整 frame
- **THEN** 操作 MUST 返回 Full 且写入长度 MUST 为零
- **AND** MUST NOT 暴露部分 frame 给任一消费者

#### Scenario: TCP short write

- **WHEN** TCP socket send buffer 只能接受部分用户 bytes
- **THEN** 现有 send path MUST 返回实际接受 byte count
- **AND** 该结果 MUST NOT 被解释为 packet slot 接受了部分 Ethernet frame

#### Scenario: UDP slot 或 socket 满

- **WHEN** UDP datagram 无法完整进入现有 socket buffer 或后续 packet slot
- **THEN** 对应层 MUST 保持 datagram 原子性并返回其定义的 WouldBlock/Full
- **AND** MUST NOT 返回已接受部分 datagram 的长度

#### Scenario: Full 后恢复可观测

- **WHEN** 确定性压力见证使任一 slot 或 TX descriptor queue 达到 Full 并随后释放容量
- **THEN** full counter、occupancy high-water、submit/completion/reclaim 与恢复事件 MUST 形成一致账本
- **AND** 普通吞吐成功但缺少 Full 和恢复证据 MUST NOT 计为该场景通过

#### Scenario: Telemetry 不参与同步

- **WHEN** 实现新增纯观测 counter
- **THEN** 纯 telemetry MAY 使用 Relaxed ordering
- **AND** owner、event generation、ticket completion 或 flush 判定 MUST 使用与其同步角色一致的 ordering 或既有锁保护

### Requirement: QEMU diagnostic lease 使用 Service-owned committed state

只在 `qemu-diagnostics` feature 下，MS05 diagnostic hold lease MUST 由现有 queue `Service`
拥有。control、expiry tick 和 V3 snapshot MUST 在同一 Service ownership boundary 下读写
mode、expiry 和 auto-release counter；成功 V3 snapshot MUST 只包含真实 committed state。
控制路径 MUST 使用有界 Service acquisition，最长 lease MUST 可由显式 Release 或到期 tick
解除，timer MUST 只负责唤醒且 MUST NOT 拥有或直接清理 lease。V1/V2/V3 wire layout、设备
queue owner 与普通/D1 build MUST 保持不变。

#### Scenario: Control 成功提交后唤醒 owner

- **WHEN** diagnostic ioctl 成功 try-lock Service 并提交 Hold 或 Release
- **THEN** mode 与 checked absolute expiry MUST 在同一 guard 内一次提交，并在解锁后发布 queue event
- **AND** overflow 或无效输入 MUST fail closed，且不得留下部分状态

#### Scenario: Control 与 Service 竞争

- **WHEN** diagnostic ioctl 无法立即取得 Service guard
- **THEN** control MUST 返回 `ResourceBusy`/`WouldBlock`，不得改变 lease 或发布伪 queue event
- **AND** probe MAY 只在固定总 deadline 内有界重试，不得 busy-spin 或无限阻塞

#### Scenario: V3 与控制并发

- **WHEN** V3 snapshot 与 Hold、Release 或 expiry tick 并发
- **THEN** V3 MUST 在既有 Service guard 下返回一个真实 committed lease tuple 和同轮账本
- **AND** contention MUST NOT 编码成无法区分的 synthetic no-hold tuple

#### Scenario: Lease 到期自动释放

- **WHEN** owner 在 Service guard 下观察到当前 Hold 的 deadline 已到
- **THEN** tick MUST 清除当前 lease并以 monotonic saturating counter 精确记录一次自动释放
- **AND** 实现 MUST NOT 存在令 active Hold 永久无法 Release 或 expiry 的 generation terminal state

#### Scenario: 旧 timer 晚于 replacement lease 唤醒

- **WHEN** Hold A 的 timer 已 armed，而 A 随后被 Release 或替换为未到期的 Hold B
- **THEN** A 的 timer MAY 唤醒 owner，但 MUST NOT 直接清除 B
- **AND** 下一次 Service poll MUST 保留 B 并重臂 B 的 deadline，不得依赖 generation identity

#### Scenario: Service 尚未初始化

- **WHEN** V3 snapshot 在全局 Service 安装前执行
- **THEN** MAY 沿用既有全零 snapshot 语义
- **AND** 该 pre-init 状态 MUST 与运行期 Service contention 分开处理

### Requirement: MS05 QEMU 验证与证据边界

MS05 MUST 提供 change-local 的确定性 Full→恢复见证，而不得依赖现有通用 `network_benchmark` 推断内部背压。host/model tests MUST 覆盖所有权、Full、错误恢复、乱序 completion、flush 和 lost-wakeup；单 hart QEMU VirtIO-MMIO runtime MUST 覆盖 TX-only、双向、slot/queue Full→恢复、flush C4、descriptor 守恒和网络功能。受影响的 MS04 snapshot、idle、nudge、burst MUST 按 R51 重跑，既有 RX probe/schema 字段 MUST 保持可判定或提供明确的向后兼容版本。原始命令、环境、串口日志、probe 输出、退出码、revision 和完成 marker MUST 保存为 change-local Evidence。

#### Scenario: 确定性 Full→恢复 probe

- **WHEN** change-local probe 控制 slot 或 queue 达到精确容量边界并随后允许 completion/reclaim
- **THEN** probe MUST 分别证明 Full、packet 保留、容量释放和恢复提交
- **AND** probe MUST 在 fixed deadline 内输出唯一 PASS/FAIL marker 和非歧义退出状态

#### Scenario: 重跑 MS04 核心模式

- **WHEN** queue task、事件入口、RX handoff 或 telemetry 因 MS05 发生变化
- **THEN** R51 snapshot、idle、nudge、burst MUST 在当前产物上重跑
- **AND** 用户过去豁免的 compatibility 或 exact-binary 项 MUST 保持 WAIVED/SKIPPED，除非本 change 取得新的完整证据

#### Scenario: 网络功能回归

- **WHEN** host/model 和 queue-level Gate 已通过
- **THEN** QEMU MUST 分别验证 ARP、ICMP、UDP、TCP 5555、nonblocking 和 poll 的受影响路径
- **AND** 串口成功、单个协议成功或 peer 侧单独成功 MUST NOT 替代其他场景

#### Scenario: Evidence 不完整或运行超时

- **WHEN** runtime 缺少原始日志、probe 输出、环境、revision、退出状态或完成 marker，或超过 fixed deadline
- **THEN** 对应 Gate MUST 标记未完成或失败
- **AND** 部分 telemetry 或普通吞吐 MUST NOT 提升为 PASS

#### Scenario: 证据范围声明

- **WHEN** MS05 的 QEMU runtime Gate 全部通过
- **THEN** 结论 MUST 限定于当前单 hart QEMU VirtIO-MMIO 软件 ownership、通知和数据面行为
- **AND** MUST NOT 声明 SMP、PCI、DWMAC、真板 DMA/cache、硬件性能或 fd readiness 已验证
