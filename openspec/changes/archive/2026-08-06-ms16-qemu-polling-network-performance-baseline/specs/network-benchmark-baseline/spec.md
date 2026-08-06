## ADDED Requirements

### Requirement: 版本化 benchmark 协议

benchmark 双端 MUST 在数据传输前交换协议版本、角色、配置 hash、capability 和 workload 参数。任何不一致 MUST 在传输前返回可诊断失败。

#### Scenario: 双端配置一致

- **WHEN** sender 和 receiver 的版本、角色、配置和 workload 参数一致
- **THEN** receiver MUST 先进入 ready 状态，再允许 warm-up 和测量

#### Scenario: 双端配置不一致

- **WHEN** 任一协议版本、角色、配置 hash 或 workload 参数不同
- **THEN** benchmark MUST 拒绝传输并输出差异字段
- **AND** benchmark MUST NOT 生成有效性能结果

### Requirement: 接收端确认的数据完整性

TCP 和 UDP headline 结果 MUST 使用 receiver 在 C6 完成点确认的唯一有效 payload。sender enqueue 字节 MUST NOT 作为 goodput。

#### Scenario: TCP partial I/O

- **WHEN** `send` 或 `recv` 只完成部分请求
- **THEN** benchmark MUST 继续推进剩余数据并统计 syscall 次数
- **AND** receiver MUST 以完整字节账本和 payload 校验决定 round 状态

#### Scenario: UDP 序号异常

- **WHEN** receiver 观察到缺失、重复、乱序、损坏或迟到 datagram
- **THEN** benchmark MUST 分别统计 loss、duplicate、reorder、corrupt 和 late
- **AND** benchmark MUST NOT 将不同错误折叠为单一 loss

#### Scenario: peer 提前退出

- **WHEN** peer 在测量结束前 EOF、超时或退出
- **THEN** benchmark MUST 保留已完成账本和失败原因
- **AND** 该 round MUST 标记为 invalid

### Requirement: 固定完成点和指标口径

每项原始记录 MUST 包含 C1-C6 中的 `completion_point`。goodput、PPS、RTT、delay variation、错误率、CPU 和指令指标 MUST 使用规范公式和来源。

#### Scenario: 计算 receiver goodput

- **WHEN** receiver 完成有效 round
- **THEN** goodput MUST 等于唯一校验 payload 字节乘八再除以 receiver 测量时间

#### Scenario: 计算指令效率

- **WHEN** 单 hart guest 提供有效 `/proc/instret` 起止值和读取开销
- **THEN** benchmark MUST 以 C6 校验 bit、byte、packet 和 syscall 为分母输出指令效率
- **AND** 结果 MUST 标记为 guest 整机代理

#### Scenario: 可选指标不可用

- **WHEN** 平台没有声明某项 measurement capability
- **THEN** 该指标 MUST 输出 `unavailable`
- **AND** benchmark MUST NOT 将缺失能力写成零

### Requirement: 稳定 workload 与 profile

benchmark MUST 提供 smoke、quick 和 standard profile。MS16 standard profile MUST 覆盖 R47 标为必测的 N00-N43 测试，且固定 payload、flow、warm-up、duration、round、seed 和 `TCP_NODELAY=1`。

#### Scenario: TCP 与 UDP 主矩阵

- **WHEN** 执行 standard profile
- **THEN** benchmark MUST 覆盖 TCP/UDP TX、RX、双向、多流、RTT、受控 UDP offered-load 和非阻塞背压
- **AND** 每项 MUST 输出逐 round 原始记录

#### Scenario: UDP offered-load 校准

- **WHEN** pilot 得到当前配置的零丢包基准
- **THEN** standard profile MUST 执行该基准的 25%、50%、75%、90% 和 100% 档位
- **AND** 每档 MUST 同时报告 offered、accepted 和 received

#### Scenario: standard 正确性边界

- **WHEN** standard profile 完成
- **THEN** TCP 字节账本 MUST 闭合
- **AND** UDP corruption、duplicate、reorder 和 loss MUST 为零

### Requirement: QEMU 拓扑与人工操作边界

user-net MUST 只用于功能 smoke。正式 B0 MUST 使用 TAP、`ICOUNT=n` 和固定 QEMU 配置。QEMU 启动和 guest shell 命令 MUST 遵守人工操作 Runbook。

#### Scenario: user-net smoke

- **WHEN** benchmark 在 user-net 上运行
- **THEN** 结果 MUST 只标记为 compatibility smoke
- **AND** 报告 MUST NOT 将其作为 headline 性能基线

#### Scenario: TAP 正式运行

- **WHEN** 执行正式 polling B0
- **THEN** manifest MUST 记录 QEMU、TAP、MTU、SMP、affinity、offload、vhost、rootfs 和源码事实
- **AND** 运行 MUST 使用 `ICOUNT=n`

#### Scenario: 人工 QEMU 边界

- **WHEN** 执行 QEMU runtime Gate
- **THEN** 用户 MUST 人工启动 QEMU 并输入 guest 命令
- **AND** host peer、采集和离线报告 MAY 自动执行，但 MUST NOT 控制 guest console

### Requirement: QEMU CPU、guest 指令和 IRQ 测量

MS16 MUST 分开记录 QEMU、host peer 和 collector 的 CPU。单 hart guest MUST 记录 `/proc/instret`，网络 IRQ MUST 使用 MS03 snapshot。

#### Scenario: 空闲控制组

- **WHEN** 测量轮询空闲成本
- **THEN** 测试 MUST 区分无网络设备、空闲网络设备和空闲 socket/service
- **AND** 只有控制组差值 MAY 用于归因网络轮询成本

#### Scenario: 负载效率

- **WHEN** TCP 或 UDP round 有有效 C6 字节
- **THEN** 报告 MUST 输出 QEMU CPU seconds、core equivalents、CPU seconds/GiB 和 guest instructions/bit

#### Scenario: SMP 指令统计

- **WHEN** guest 使用多 hart 而没有逐 hart 计数器
- **THEN** instructions/bit MUST 标记为 unavailable

### Requirement: Evidence 完整性

正式 B0 MUST 使用 required Evidence。原始记录 MUST 只追加、不覆盖，Evidence checker MUST 检查必需文件、字段、round、双端账本和摘要可重建性。

#### Scenario: Evidence 完整

- **WHEN** 正式 B0 的必需文件和字段存在且账本闭合
- **THEN** checker MUST 输出通过状态和配置 hash

#### Scenario: Evidence 缺失

- **WHEN** 任一必需文件、字段、round 或原始记录缺失
- **THEN** checker MUST 输出失败和缺失清单
- **AND** change MUST NOT 声明正式 B0 完成

#### Scenario: 无效 round 与补跑

- **WHEN** 某 round 无效且随后补跑
- **THEN** 两个 round MUST 同时保留
- **AND** 补跑 MUST 使用新的 round ID

### Requirement: A/B 比较资格

报告器 MUST 根据受控环境字段生成 `comparison_key`。除 `treatment` 外的字段不同 MUST 阻止自动生成改善比例。

#### Scenario: 可比的 polling 与 async 结果

- **WHEN** B0 与 A1 只有 `treatment` 不同
- **THEN** 报告器 MUST 允许生成成对比较

#### Scenario: 环境字段不同

- **WHEN** benchmark、kernel、rootfs、backend、MTU、QEMU、SMP、affinity、payload、duration、queue 或 telemetry 字段不同
- **THEN** 报告器 MUST 拒绝改善比例并列出差异字段

#### Scenario: QEMU 与真板结果

- **WHEN** 两组结果属于不同平台比较域
- **THEN** 报告 MUST 保留独立基线
- **AND** 报告 MUST NOT 生成跨平台绝对性能排名
