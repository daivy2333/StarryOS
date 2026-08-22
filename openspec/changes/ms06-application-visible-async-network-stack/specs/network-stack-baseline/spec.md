## MODIFIED Requirements

### Requirement: Readiness 与 I/O 一致

TCP 和 UDP 的 poll readiness MUST 与紧随其后的同步或 nonblocking I/O 结果一致。MS06 以后，readiness MUST 由常驻 stack runner 推进，并通过每 socket multi-waiter bridge 交付；正确性 MUST NOT 依赖调用者主动轮询协议栈。

#### Scenario: Socket 尚未 ready

- **WHEN** socket 没有可读数据、可写容量、待接连接、EOF、关闭或可报告错误
- **THEN** poll MUST NOT 报告对应 readiness
- **AND** nonblocking I/O MUST 返回当前兼容的 would-block 错误

#### Scenario: Socket 变为 ready

- **WHEN** 连接、数据、发送容量、EOF、关闭或错误使 socket 状态改变
- **THEN** poll MUST 报告对应 `IN/OUT/RDHUP/HUP/ERR`
- **AND** 紧随其后的 I/O MUST observe the same state or a documented concurrent-consumer race

#### Scenario: 多个 waiter 观察同一 socket

- **WHEN** poll、select、epoll 或阻塞 I/O 同时登记同一 socket
- **THEN** 每个 waiter MUST 通过 per-socket PollSet 获得状态重检机会
- **AND** smoltcp 的单槽 waker MUST NOT 使后注册者静默覆盖已有 application waiter

#### Scenario: Listener readiness

- **WHEN** ListenTable 管理的隐藏 smoltcp socket 建立连接或 reset
- **THEN** public listener MUST 报告 accept 或 error readiness
- **AND** accept MUST 返回唯一连接、匹配错误，或在并发消费后返回 `WouldBlock`

#### Scenario: 稳定网络 fault

- **WHEN** 异步 queue/data-plane 进入稳定 fatal state
- **THEN** 受影响 TCP/UDP socket MUST 报告 `ERR` 并唤醒已登记 waiter
- **AND** 后续 I/O MUST 返回稳定映射错误，不得继续永久 Pending 或隐藏为普通背压
