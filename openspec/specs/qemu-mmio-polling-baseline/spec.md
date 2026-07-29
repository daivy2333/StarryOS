# qemu-mmio-polling-baseline Specification

## Purpose
TBD - created by archiving change ms02-virtio-mmio-polling-baseline. Update Purpose after archive.
## Requirements
### Requirement: QEMU 通道分证据

MS02 MUST 分开验证串口、设备探测、guest 服务和 hostfwd。
任一通道成功 MUST NOT 替代其他通道的证据。

#### Scenario: 无 hostfwd 启动

- **WHEN** QEMU 在不配置 hostfwd 时启动当前镜像
- **THEN** guest MUST 通过串口进入 shell
- **AND** 该结果 MUST 只计为串口证据

#### Scenario: 串口成功但端口失败

- **WHEN** 串口可用但宿主无法连接 5555
- **THEN** 结果 MUST 标记为网络路径失败
- **AND** 诊断 MUST 区分 probe、服务和 hostfwd

### Requirement: VirtIO-MMIO 启动基线

QEMU 基线 MUST 使用 `virt`、单 hart 和 VirtIO-MMIO net/block。
启动证据 MUST 标出 transport、设备和 `eth0` 初始化结果。

#### Scenario: 探测 MMIO 设备

- **WHEN** 当前 QEMU MS02 配置启动
- **THEN** net 与 block 设备 MUST 通过 MMIO 被探测
- **AND** 网络栈 MUST 创建 `eth0`

#### Scenario: transport 与配置不一致

- **WHEN** 启动日志未证明 MMIO 或预期设备缺失
- **THEN** probe Gate MUST 失败
- **AND** 网络协议用例 MUST NOT 计为有效基线

### Requirement: 无 IRQ 的同步网络进度

无 IRQ 的 VirtIO-MMIO 网卡 MUST 在外部流量到达后推进。
进度机制 MUST 保持同步 socket 语义。
空闲时 MUST NOT 形成无界 busy loop。

#### Scenario: 等待期间收到外部流量

- **WHEN** TCP accept 或 UDP receive 等待期间收到外部帧
- **THEN** 协议栈 MUST 在有界时间内处理该帧
- **AND** 等待操作 MUST 观察到新的 socket 状态

#### Scenario: 流量早于等待注册

- **WHEN** 外部帧在 socket 等待者注册前到达
- **THEN** 后续轮询 MUST 处理该帧
- **AND** 数据 MUST NOT 因缺少 IRQ waker 而永久停留

#### Scenario: 网络空闲

- **WHEN** 没有外部流量且没有待发送数据
- **THEN** 进度机制 MUST 主动让出 CPU
- **AND** MS02 MUST 记录其空闲 CPU 基线

### Requirement: 明确的 guest 网络服务

MS02 MUST 提供可识别的 guest TCP 与 UDP 服务。
服务启动、端口、payload 和超时 MUST 可从见证中确定。

#### Scenario: TCP 服务处理连接

- **WHEN** 宿主通过 TCP hostfwd 连接 5555 并发送规定 payload
- **THEN** guest MUST 返回规定响应
- **AND** 连接关闭后服务 MUST 能处理下一条连接

#### Scenario: UDP 服务处理 datagram

- **WHEN** 宿主通过 UDP hostfwd 向 5555 发送规定 datagram
- **THEN** guest MUST 返回规定响应
- **AND** 响应 MUST 保持 datagram 边界

#### Scenario: guest 服务未就绪

- **WHEN** 宿主在服务启动前发起用例
- **THEN** 用例 MUST 在有界 timeout 内失败
- **AND** 结果 MUST NOT 被归类为 MMIO probe 失败

### Requirement: 协议级独立见证

ARP、ICMP、UDP 与 TCP 5555 MUST 各有独立见证。
串口文本 MUST NOT 单独证明包级协议成功。

#### Scenario: ARP 邻居解析

- **WHEN** guest 与外部 peer 首次交换 IPv4 流量
- **THEN** 抓包 MUST 显示 ARP request 与有效 reply
- **AND** 后续 IPv4 帧 MUST 使用解析出的链路地址

#### Scenario: ICMP echo reply

- **WHEN** 外部 peer 向 guest 地址发送 ICMP echo request
- **THEN** guest MUST 返回匹配的 echo reply
- **AND** 该能力 MUST NOT 依赖 raw socket syscall

#### Scenario: UDP hostfwd

- **WHEN** 宿主通过 UDP hostfwd 执行规定用例
- **THEN** 请求与响应 MUST 通过
- **AND** 证据 MUST 与 TCP 结果分开

#### Scenario: TCP hostfwd

- **WHEN** 宿主通过 TCP hostfwd 执行规定用例
- **THEN** 连接、payload 和响应 MUST 通过
- **AND** 证据 MUST 与 UDP 结果分开

### Requirement: 失败与 timeout 可诊断

每个宿主网络用例 MUST 有有界 timeout。
失败结果 MUST 标出最早失效层。

#### Scenario: 宿主用例超时

- **WHEN** ARP、ICMP、UDP 或 TCP 用例超过规定 timeout
- **THEN** 用例 MUST 失败
- **AND** 结果 MUST 保留协议、服务和 QEMU 配置上下文

#### Scenario: payload 不匹配

- **WHEN** guest 响应与规定 payload 不一致
- **THEN** 对应用例 MUST 失败
- **AND** 其他协议的结果 MUST 保持独立

#### Scenario: 用例被中断

- **WHEN** QEMU 或宿主用例在完成前被终止
- **THEN** 结果 MUST 标记为未完成
- **AND** 未完成结果 MUST NOT 计为通过

### Requirement: 空闲 CPU 只作基线

MS02 MUST 记录无流量时的 QEMU CPU 使用环境、采样方法和结果。
该结果 MUST NOT 声明为性能优化或真实硬件证据。

#### Scenario: 采集空闲 CPU

- **WHEN** guest 服务已就绪且网络保持空闲
- **THEN** 采样 MUST 记录 QEMU 配置、时长和宿主观测值
- **AND** 本轮 MUST NOT 使用预设阈值判定网络功能

### Requirement: MS02 范围隔离

MS02 MUST 保持 VirtIO-MMIO 同步轮询边界。
实现 MUST NOT 引入 IRQ 驱动、PCI 或异步网络层。

#### Scenario: 完成同步轮询基线

- **WHEN** 所有 MS02 功能 Gate 通过
- **THEN** IRQ、PLIC、AtomicWaker 和 queue task MUST 保持未引入
- **AND** MS01 socket 兼容行为 MUST 继续通过

#### Scenario: 自动化升级扩大范围

- **WHEN** 验收需要新的通用 QEMU 自动化基础设施
- **THEN** 本 change MUST 停止并报告范围扩大
- **AND** I14 或 I15 MUST 由独立授权处理

