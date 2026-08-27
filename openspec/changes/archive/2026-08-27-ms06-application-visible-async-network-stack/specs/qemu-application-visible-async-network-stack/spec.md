## ADDED Requirements

### Requirement: 唯一常驻 stack runner

MS06 MUST 在网络 Service 安装后启动至多一个常驻 stack runner。runner MUST 是产品路径中唯一调用 smoltcp ingress、egress、maintenance 和 dispatch 推进循环的 task；TCP、UDP、listener 和 poll/select/epoll 的正确性 MUST NOT 依赖调用者主动执行 `poll_interfaces()`。

#### Scenario: 网络初始化启动 runner

- **WHEN** scheduler 已初始化且 `init_network` 完成 Service 安装
- **THEN** 实现 MUST 通过原子 lifecycle 决策生成一个固定名称的 stack runner
- **AND** runner MUST NOT 在 Service 安装前取得或轮询网络状态

#### Scenario: 重复启动 runner

- **WHEN** 启动入口被调用两次或两个调用者竞争启动
- **THEN** 只有 lifecycle transition 的唯一成功者 MUST spawn task
- **AND** 其余调用 MUST 返回稳定的 already-started 结果且不得生成第二推进者

#### Scenario: socket API 在无主动 poll 下产生进展

- **WHEN** 应用创建、连接、发送、关闭或监听 socket 后不再调用其他网络 API
- **THEN** software wake MUST 使 runner 观察已提交状态并继续协议栈推进
- **AND** 产品 TCP/UDP path MUST NOT 以同步 `poll_interfaces()` 作为提交或完成条件

### Requirement: Device、software 与 timer wake 合流

stack runner MUST 通过一个独立的 generation + single-runner waker 入口合流 device progress、software mutation 和 smoltcp `poll_at` deadline。runner MUST 使用 snapshot → register → work/arm → generation recheck 协议关闭 event-before-register 与 register-during-event 窗口；socket software wake MUST NOT 无条件唤醒 queue owner。

#### Scenario: Device progress 唤醒 runner

- **WHEN** queue service 使 RX slot 可读、TX slot 获得空间或提交稳定 fault
- **THEN** 它 MUST 在状态提交后发布 stack generation 并唤醒 runner
- **AND** device hint MUST 只触发重新检查，不得直接宣称 fd ready

#### Scenario: Software mutation 唤醒 runner

- **WHEN** socket 创建、bind/connect/listen、send enqueue、shutdown/close 或 listener 状态变更已提交
- **THEN** mutation path MUST 在释放相关锁后发布 stack-only software event
- **AND** queue task MUST NOT 仅因普通 socket mutation 被迫执行无关服务轮次

#### Scenario: Timer deadline 到期

- **WHEN** `poll_at` 返回未来 deadline 且此前没有 device 或 software event
- **THEN** runner-owned timer MUST 在该 deadline 唤醒同一 runner
- **AND** 旧 timer 的迟到 wake MUST 至多造成一次有界 spurious poll，不得修改当前 deadline 或 socket 状态

#### Scenario: Event 与注册交错

- **WHEN** 任一 stack event 发生在 generation snapshot、waker register、Service unlock 或 timer arm 附近
- **THEN** generation recheck MUST 使 runner 立即重试或由已注册 waker 唤醒
- **AND** runner MUST NOT 在可推进工作存在时永久 Pending

### Requirement: 有界推进、公平性与 quiet path

每个 stack runner poll MUST 分别限制 Router RX、smoltcp ingress、smoltcp egress、Router dispatch、listener pending reconciliation 和 deferred socket retirement 的工作量，并在一次轮次中为每个 stage 提供运行机会。隐藏listener由ingress触发的精确head repair MUST 与active-port/pending-queue sweep分开计数：每个已处理ingress packet至多消费一个O(1) head signal，每轮不得超过ingress budget，且不得扫描active ports或pending queue。达到 budget 且仍有可立即推进工作时 runner MUST self-wake 后返回 `Pending`；无工作、无到期 timer 且 IRQ-backed queue 已激活时 MUST 保持休眠。

#### Scenario: 精确 budget 边界

- **WHEN** 某 stage 分别具有 budget−1、budget 和 budget+1 个连续工作项
- **THEN** 单轮处理数 MUST 不超过该 stage 的固定 budget
- **AND** budget+1 的剩余工作 MUST 通过下一次调度继续而不得丢失

#### Scenario: 双向持续流量

- **WHEN** ingress、egress 或 dispatch 中任一方向持续保持 backlog
- **THEN** runner MUST 在固定 stage 顺序中仍执行其他 stage 并定期让出 CPU
- **AND** queue task、应用 task 和相反方向 MUST NOT 因单轮无界 drain 被饿死

#### Scenario: Deferred close storm

- **WHEN** 至少 512 个 TCP handle 已提交 deferred close，且其中部分仍等待 peer ACK
- **THEN** 每个 runner poll 检查的 deferred entry 数 MUST 不超过固定 budget，并从持久 cursor 继续后续 entry
- **AND** listener refill、UDP progress、Router RX/dispatch 和应用 task MUST NOT 被全表扫描或仅由未确认 entry 触发的 busy self-wake 饿死

#### Scenario: Listener reconciliation budget

- **WHEN** listener queue含31、32、33或512个pending hidden slots，且其中任意位置发生Ready、Reset或`SynReceived → Listen`转换
- **THEN** 每个runner poll检查的pending slot数 MUST 不超过固定budget，并从持久cursor继续
- **AND** 同一round MUST NOT 在每个ingress step后重新扫描active ports或完整listener queue；只允许消费由hidden waker精确标识且按ingress packet一对一计费的O(1) head signal

#### Scenario: Listener queue在有界pass中收缩

- **WHEN** listener reconciliation pass尚未完成，应用accept从已访问前缀移除Ready或Reset slot，且剩余queue长度仍大于当前cursor
- **THEN** queue结构变化 MUST 使runner从安全位置继续或有界重启，quiet park前仍须访问每个剩余live slot
- **AND** software wake MUST NOT 因本轮没有新的ingress/egress state change而丢失该结构变化

#### Scenario: 活跃 IRQ 设备空闲

- **WHEN** queue lifecycle 为 Active、无 stack event、无 socket work 且 `poll_at` 无 deadline
- **THEN** runner poll counter MUST 在观察窗口内保持不变
- **AND** 实现 MUST NOT 使用固定 10ms tick、busy loop 或连续 self-wake 维持进展

#### Scenario: Polling owner fallback

- **WHEN** queue lifecycle 仍为 Polling/Spawned/Unavailable，或选中设备明确要求 polling
- **THEN** runner MUST 以有界 10ms fallback 提供旧数据路径进展
- **AND** lifecycle 变为 Active 后该 fallback MUST 停止，变为 Faulted 后 MUST 保持 async ownership 并传播错误而不得回退第二 owner

#### Scenario: 单轮时间戳一致

- **WHEN** runner 开始一次 poll 并推进协议栈、计算 `poll_at` 和维护 timer
- **THEN** Router、smoltcp、Service outcome 与 timer decision MUST 使用该轮同一次时钟采样
- **AND** host/model injected clock MUST 进入同一 Service 路径，使时间推进能确定性触发 delayed ACK、retransmit 和 close confirmation

### Requirement: 锁序与 await 生命周期

runner 和 socket paths MUST 采用 `SERVICE → SOCKET_SET → ListenTable entry` 的全局锁序。需要同时访问 `Interface::context` 和 smoltcp socket 的操作 MUST 通过同一有序 helper 完成；任何 `Service`、`SocketSet`、listener 或 readiness registry guard MUST NOT 跨越 `await`、`Pending`、waker wake 或 task yield。

#### Scenario: TCP connect 同时需要 Interface 和 socket

- **WHEN** TCP connect 需要 source route、`Interface::context` 和 socket handle
- **THEN** 实现 MUST 先取得 Service 再取得 SocketSet 并在同一同步临界区提交 connect
- **AND** MUST NOT 从 `with_smol_socket` closure 内反向取得 Service

#### Scenario: runner 返回 Pending

- **WHEN** runner 选择 event sleep、timer sleep 或 budget self-yield
- **THEN** 它 MUST 在 wake、arm 或返回 `Pending` 前释放全部网络锁
- **AND** host/model test MUST 能证明 guard 不跨调度点存活

### Requirement: 每 socket 多 waiter readiness bridge

每个 public TCP/UDP socket MUST 具有 read、write 和 terminal readiness bridge；每个 bridge MUST 使用 `Arc<axpoll::PollSet>` 将 smoltcp 单槽 recv/send waker 扇出给同一 socket 的多个 poll/select/epoll 或阻塞 I/O waiter。注册 MUST 遵循 check → application-waker register → smoltcp bridge register → recheck；socket 移除 MUST 唤醒遗留 waiter。

#### Scenario: 同一 socket 多 waiter

- **WHEN** 两个或更多 waiter 在同一 TCP/UDP socket 的相同或不同方向注册
- **THEN** smoltcp MUST 只保存对应方向的一个 bridge waker
- **AND** readiness transition MUST 通过 PollSet 唤醒所有已登记 application waker

#### Scenario: PollSet 容量边界

- **WHEN** 第 64 和第 65 个不同 waker 向同一 readiness bridge 注册
- **THEN** 第 64 个 MUST 可被正常保存，第 65 个 MUST 沿用 axpoll 的 wake-on-replacement 行为
- **AND** 被替换 waiter MUST 重新检查状态并在仍未 ready 时重新注册，不得静默永久丢失

#### Scenario: 注册期间状态改变

- **WHEN** readiness 在首次 check 后、bridge register 前后或最终 recheck 前改变
- **THEN** smoltcp wake 或最终 recheck MUST 至少触发一次 application wake/ready observation
- **AND** spurious wake MUST 重新执行实际 I/O 检查而不得被视为成功保证

### Requirement: Listener、close 与 error readiness 一致

TCP listener MUST 为其隐藏 smoltcp listener sockets 维护独立 accept bridge，并在 refill/reconcile/register 后重臂。TCP/UDP poll MUST 把应用可观察状态映射到 `IN`、`OUT`、`RDHUP`、`HUP` 和 `ERR`，且紧随其后的 nonblocking I/O MUST 返回匹配结果或记录明确的并发 race。稳定 queue/data-plane fault MUST 先提交错误再唤醒全部受影响 socket bridge。

#### Scenario: Listener 接收连接

- **WHEN** idle 或 pending 隐藏 listener socket 变为可接受连接
- **THEN** public listener 的 accept PollSet MUST 唤醒全部 waiter并报告 `IN`
- **AND** 紧随其后的 accept MUST 返回唯一连接或在并发 winner 已消费时返回 `WouldBlock`

#### Scenario: 同一 ingress batch 的相邻 SYN

- **WHEN** 同一listener仍有backlog headroom，两个或最多一个ingress budget内的多个不同client SYN在同一runner round连续到达
- **THEN** 每个已消费SYN触发的精确listener-head signal MUST 在下一packet处理前提交idle transition并补充一个Listen-state hidden socket，使后续SYN不因瞬时无idle而收到RST
- **AND** 每轮head repair次数 MUST 不超过已处理ingress packet数和ingress budget，不得预分配idle socket池、执行active-port/full-queue扫描或改变512 backlog上限

#### Scenario: 满 backlog 释放后立即恢复容量

- **WHEN** 512个listener slot已满，先前overflow尝试已达到可判定终态，应用accept一个连接并立即发起新的loopback connect
- **THEN** accept 提交必须在返回前恢复一个 idle hidden listener，且 recovery connect MUST NOT 因等待 runner reconcile 而返回 `ConnectionRefused`
- **AND** refill 不得调用 caller-driven stack progress、改变 512 backlog 上限或违反 `SOCKET_SET → ListenTable entry` 的锁序子序列

#### Scenario: 满 backlog overflow 与 RST 恢复

- **WHEN** 满backlog期间的额外connect仍在runner ingress排队，或hidden listener在`SynReceived`收到RST并回到`Listen`
- **THEN** 验证 MUST 把该overflow尝试的终态与后续headroom recovery分开判定
- **AND** 回到`Listen`的pending hidden socket MUST 恢复为idle或被安全移除，不得永久占用pending slot

#### Scenario: TCP 数据、EOF 与 half-close

- **WHEN** TCP socket 有 buffered receive data、peer FIN 或本地 read shutdown
- **THEN** buffered data或可观察 EOF MUST 报告 `IN`，peer/local receive-half 终止 MUST 报告 `RDHUP`
- **AND** read MUST 对应返回数据、零长度 EOF 或现有兼容的本地 shutdown 错误

#### Scenario: TCP writable 与完整关闭

- **WHEN** established socket 的 send buffer 可接受 bytes
- **THEN** poll MUST 报告 `OUT` 且下一次 write MUST 能接受至少一个 byte或观察到并发状态变化
- **AND** 双向均不可继续的 socket MUST 报告 `HUP`，不得仅因 `may_send=false` 继续伪报普通 writable

#### Scenario: Connect 或 listener error

- **WHEN** nonblocking connect 完成失败，或 listener pending slot 被 reset
- **THEN** socket MUST 报告 `ERR` 以及完成语义要求的 `OUT` 或 `IN`
- **AND** 下一次 connect completion check 或 accept MUST 返回匹配的稳定错误

#### Scenario: UDP readiness 与关闭

- **WHEN** UDP datagram 可读、完整 datagram buffer 可写或 socket 已关闭
- **THEN** poll MUST 分别报告 `IN`、`OUT` 或 `HUP`
- **AND** recv/send MUST 保持 datagram 原子性并与该 readiness 或并发 race 一致

#### Scenario: UDP send 后立即关闭

- **WHEN** UDP `sendto` 已把datagram提交到smoltcp TX buffer，但public handle在runner派发前关闭
- **THEN** raw socket MUST 保留到runner完成该datagram派发，peer在fixed deadline内 MUST 收到完整datagram
- **AND** TX buffer清空后raw handle和deferred entry MUST 恰好回收一次，空TX socket关闭不得被无条件延迟

#### Scenario: 稳定数据面 fault

- **WHEN** queue owner 从 Active 提交 fatal ownership、submit 或 reclaim error
- **THEN** fault code MUST 在任何 wake 前稳定发布，全部 public IP socket bridge MUST 被唤醒并报告 `ERR`
- **AND** 后续 send/recv/connect/accept MUST 返回相同映射类别而不得隐藏成 `WouldBlock`、普通 Full 或 polling fallback

### Requirement: MS06 验证与结论边界

MS06 MUST 以 host/model tests 覆盖 lifecycle、三源 wake、budget、timer、fallback、锁序、multi-waiter、overflow、listener、close/error 和 lost-wakeup，并在单 hart QEMU VirtIO-MMIO 上验证应用无需主动 poll 的 TCP/UDP/listener 与 poll/select/epoll 行为。受影响的 MS04 quiet/snapshot/nudge/burst、MS05 双向数据面和 MS01 socket 兼容路径 MUST 回归。

#### Scenario: 自动 Gate

- **WHEN** change 进入 QEMU runtime 验证前
- **THEN** axnet ordinary/qemu-diagnostics tests、kernel QEMU check、probe seam tests、format、strict OpenSpec 和 full diff review MUST 全部通过
- **AND** 任一 compile、assert、lost-wakeup、budget 或 lock-order failure MUST 阻止 runtime PASS

#### Scenario: 应用可见 QEMU 验收

- **WHEN** 单 hart QEMU guest 在固定 deadline 内运行 TCP/UDP、listener、nonblocking、poll、select 和 epoll 场景
- **THEN** 每个场景 MUST 在没有额外主动 stack poll 的情况下输出唯一完成 marker
- **AND** quiet、连续流量、多 waiter、overflow、close/error 与回归项 MUST 分项可判定

#### Scenario: 超时或证据不完整

- **WHEN** QEMU 运行超时、缺少命令/环境/revision/退出码/marker，或只有部分协议成功
- **THEN** 对应 Gate MUST 标记未完成或失败
- **AND** 编译成功、单个 wake counter 或历史 artifact MUST NOT 替代当前 runtime 结果

#### Scenario: 结论范围

- **WHEN** MS06 全部 Gate 通过
- **THEN** 结论 MUST 限定于当前单 hart QEMU VirtIO-MMIO 的 runner、socket readiness 和兼容行为
- **AND** MUST NOT 声明 reset、SMP、multiqueue、多接口、PCI/DWMAC、真板、DMA/cache 或性能资格已验证
