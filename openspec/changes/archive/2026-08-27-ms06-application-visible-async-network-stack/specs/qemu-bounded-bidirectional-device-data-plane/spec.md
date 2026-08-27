## MODIFIED Requirements

### Requirement: 固定容量的二层 RX/TX packet slots

MS05 MUST 为每个 QEMU VirtIO-MMIO NIC 在 stack-side consumer 与唯一 queue service 之间建立独立的 RX 和 TX Ethernet frame slot。每个方向的容量 MUST 固定为 64 个完整 frame，并在设备初始化时预分配；数据路径 MUST NOT 通过动态扩容隐藏压力。slot MUST 只持有普通 frame 数据和必要的 packet metadata，MUST NOT 暴露或持有 VirtIO token、descriptor handle、`NetBufPtr` 或 transport-specific 状态。每次 slot 操作 MUST 以一个完整 frame 为原子单位。MS06 激活后 stack-side consumer MUST 是唯一 stack runner；queue service 的 descriptor/hardware ownership MUST 保持不变。

#### Scenario: TX frame 被 slot 接受

- **WHEN** TX slot 有容量且 Ethernet adapter 生成一个合法完整 frame
- **THEN** slot MUST 原子接受整个 frame并返回唯一接受 ticket
- **AND** slot occupancy 与 high-water telemetry MUST 反映本次转移
- **AND** queue service 之外的代码 MUST NOT 获得对应 descriptor 或 `NetBufPtr`

#### Scenario: TX slot 已满

- **WHEN** TX slot occupancy 已达到 64 且 stack runner 尝试交付下一 packet
- **THEN** slot MUST 返回可恢复 Full 且 MUST NOT 部分复制该 packet
- **AND** 上游 MUST 保留原 packet 的 ownership
- **AND** 内存占用 MUST 保持在已配置上界内

#### Scenario: RX slot 已满但 used ring 有 completion

- **WHEN** RX slot occupancy 已达到 64 且 RX used ring 仍有 completion
- **THEN** queue service MUST 在 reap 下一个 completion 前停止 RX 服务
- **AND** 未 reap completion 及其 buffer ownership MUST 留在硬件队列
- **AND** task MUST NOT 持有已 reap 但未交付的 `NetBufPtr` 跨越 `Pending`

#### Scenario: Slot 从满变为非满

- **WHEN** stack runner 消费一个满 RX slot，或 queue service 消费一个满 TX slot
- **THEN** 对应消费者 MUST 发布一次有界软件空间事件
- **AND** 因 slot Full 暂停的 owner MUST 通过分层 queue/stack event 获得重新运行机会

#### Scenario: Stack runner 切换不改变硬件 owner

- **WHEN** MS06 从 caller-driven stack polling 切换到常驻 stack runner
- **THEN** stack runner MUST 只通过普通 packet slots 与 Router/device adapter 交互
- **AND** VirtIO descriptor、token、reclaim 和 queue-control API MUST 仍只由唯一 queue service 调用
