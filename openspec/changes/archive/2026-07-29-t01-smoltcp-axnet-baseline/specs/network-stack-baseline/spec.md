## ADDED Requirements

### Requirement: 本地协议栈依赖边界

StarryOS MUST 使用仓库内 smoltcp 0.13.1 和本地 axnet 实现同步网络栈。
本地 axnet MUST NOT 依赖 `RxToken::preprocess`，smoltcp MUST NOT 为本轮
重新加入该私有接口。

#### Scenario: 构建当前 QEMU 网络配置

- **WHEN** 构建当前支持的 QEMU 网络 feature 组合
- **THEN** Cargo MUST 解析到仓库内 smoltcp 和本地 axnet
- **AND** 构建 MUST NOT 依赖 registry `starry-smoltcp` 的
  `RxToken::preprocess`

#### Scenario: 审计 transitive 网络依赖来源

- **WHEN** 从 StarryOS QEMU feature 反向检查 axnet 和 smoltcp 依赖
- **THEN** 依赖图 MUST 只包含一个本地 `axnet-ng` 和一个本地 `smoltcp`
- **AND** legacy `axnet`、registry `axnet-ng` 与 `starry-smoltcp`
  MUST NOT 出现在解析结果中

#### Scenario: 依赖切换暴露 API 不兼容

- **WHEN** smoltcp 0.13.1 与本地 axnet 存在未适配的 API 或 feature 差异
- **THEN** 对应编译 Gate MUST 失败并指出不兼容位置
- **AND** 实现 MUST NOT 通过恢复 `RxToken::preprocess` 绕过失败

#### Scenario: 独立检查本地 crate

- **WHEN** 分别以 manifest path 检查本地 smoltcp 和 axnet
- **THEN** 两个 crate MUST 具备明确的 workspace 边界
- **AND** axnet MUST 以 path dependency 解析到仓库内 smoltcp 0.13.1

### Requirement: TCP bind 状态兼容

本地 axnet MUST 在自身保存 POSIX TCP bind 状态，不得把该状态重新加入
smoltcp。显式 bind、隐式 ephemeral bind、local address 和地址冲突检查
MUST 保持同步 socket 调用者可观察的兼容语义。

#### Scenario: Bind 后 listen 或 connect

- **WHEN** TCP socket bind 到指定地址或端口后调用 listen 或 connect
- **THEN** 后续操作 MUST 使用该本地端点
- **AND** local address 查询 MUST 返回有效的当前端点

#### Scenario: 未 bind 的 connect

- **WHEN** TCP socket 未显式 bind 就发起 connect
- **THEN** axnet MUST 选择当前兼容的源地址和 ephemeral port
- **AND** 该端点 MUST 在连接生命周期内保持稳定

#### Scenario: 冲突端点重复 bind

- **WHEN** 不允许地址复用的 socket bind 到已占用的兼容冲突端点
- **THEN** bind MUST 返回当前兼容的 address-in-use 错误
- **AND** 失败 socket MUST remain usable according to current state semantics

### Requirement: TCP listener 兼容

本地 axnet MUST 在不修改 smoltcp phy trait 的前提下保持当前 TCP
listen/accept、backlog 和 listener 生命周期语义。

#### Scenario: Listener 接受连接

- **WHEN** 已 bind/listen 的 socket 收到有效 TCP 连接
- **THEN** accept MUST 返回一个独占该连接的 socket
- **AND** listener MUST 继续保留可接受后续连接的能力

#### Scenario: 多个连接相邻到达

- **WHEN** 多个连接在 accept 消费速度以内相邻到达
- **THEN** 每个成功连接 MUST 至多交给一个 accepted socket
- **AND** listener 状态 MUST remain valid for later connections

#### Scenario: Backlog 达到容量上限

- **WHEN** 待接连接达到现有固定 `LISTEN_QUEUE_SIZE` 512
- **THEN** 实现 MUST NOT 损坏 listener 或泄漏 socket handle
- **AND** 容量释放后 listener MUST 能继续接受连接

#### Scenario: 满容量释放后立即恢复

- **WHEN** 512 个连接占满队列且 accept 释放一个 slot 后立即到达新 SYN
- **THEN** listener MUST 在处理该 SYN 前恢复一个 listening slot
- **AND** 新连接 MUST 可被建立并且至多交付一次

#### Scenario: Syscall backlog 参数

- **WHEN** `listen(fd, backlog)` 收到合法 backlog
- **THEN** MS01 MUST 保持当前 syscall 只校验参数而不下传数值的行为
- **AND** 本轮 MUST NOT 把固定容量改成新的用户可配置语义

#### Scenario: Listener 关闭后重新监听

- **WHEN** listener 关闭后相同端点重新 bind/listen
- **THEN** 旧 listener 的 pending state MUST NOT 进入新 listener
- **AND** 已释放的 smoltcp handle MUST NOT 被再次使用

### Requirement: UDP 同步行为兼容

本地协议栈 MUST 保持当前 UDP send、receive、源地址和 nonblocking 行为。

#### Scenario: UDP 双向收发

- **WHEN** UDP socket 与可达 peer 交换 datagram
- **THEN** payload 和源地址 MUST 与发送内容一致
- **AND** datagram boundary MUST remain intact

#### Scenario: Nonblocking UDP 无数据

- **WHEN** nonblocking UDP receive 没有可读 datagram
- **THEN** 操作 MUST 返回当前兼容的 would-block 错误
- **AND** socket MUST remain usable for later datagrams

### Requirement: Readiness 与 I/O 一致

TCP 和 UDP 的 poll readiness MUST 与紧随其后的同步 I/O 结果一致。

#### Scenario: Socket 尚未 ready

- **WHEN** socket 没有可读数据、待接连接或可报告错误
- **THEN** poll MUST NOT 报告对应 readable/error readiness
- **AND** nonblocking I/O MUST 返回当前兼容错误

#### Scenario: Socket 变为 ready

- **WHEN** 连接、数据、发送容量或错误使 socket 状态改变
- **THEN** poll MUST 报告对应 readiness
- **AND** 紧随其后的 I/O MUST observe the same state or a documented race

### Requirement: MS01 范围隔离

本轮 MUST 保持现有同步轮询和 QEMU transport 边界。实现 MUST NOT 引入
IRQ、异步执行层、VF2/PCI 适配或未经基线支持的性能改动。

#### Scenario: 完成同步兼容迁移

- **WHEN** 本地 smoltcp/axnet 兼容 Gate 通过
- **THEN** 当前同步网络入口 MUST continue to work
- **AND** IRQ、queue task、stack runner 和异步 socket bridge MUST remain
  outside this change

#### Scenario: 当前 feature 含有未做运行验收的协议能力

- **WHEN** 当前 QEMU feature 启用未纳入本轮运行场景的协议能力
- **THEN** 该能力 MUST continue to compile
- **AND** 本轮 MUST NOT 声明其运行行为已验证
