# k3-com260-kit-polling-network-baseline Specification

## Purpose
TBD - created by archiving change ms11-com260-kit-polling-bidirectional-network-baseline. Update Purpose after archive.

## Requirements

### Requirement: 本板 DMA 地址与内存可见性契约

CoM260 Kit GMAC 轮询后端 MUST 明确 CPU VA、CPU PA 与 device DMA address 的转换、合法范围、descriptor 和 buffer 的独立所有权及同步次序。依据 MUST 来自可追溯板级资料、同板参考用法和本板对应配置；对明确未知或相互冲突的事实 MUST 停止依赖操作。提交前 MUST 完成所需的内存可见性与发布排序，完成后 MUST 先恢复 CPU 可见性再读取内容，MUST NOT 以 IRQ、doorbell、单独 fence 或 link 状态代替完成与同步。

#### Scenario: 发布与完成后观察同一队列内容

- **WHEN** CPU 准备合法 descriptor 和 buffer 并按契约发布给设备，轮询观察设备完成
- **THEN** 设备和 CPU MUST 观察对应队列状态与正确 buffer 内容，descriptor 与 buffer 所有权分别闭合
- **AND** CPU MUST NOT 改写设备拥有的存储

#### Scenario: 地址或映射契约不成立

- **WHEN** 地址越界、地址宽度不支持，或 DMA 映射/可见性依据冲突
- **THEN** 后端 MUST 明确拒绝操作并报告相关地址或契约错误
- **AND** MUST NOT 截断地址、猜测映射或试写未确认参数

### Requirement: 有界轮询 RX 队列

后端 MUST 提供有界 RX refill/reap，依据硬件完成状态和长度校验交付完整帧。坏帧、非法长度、队列耗尽及 wrap MUST 有明确结果；完成、丢弃与再次 refill MUST 不导致 buffer 越界、双重回收或设备 owner 覆盖。

#### Scenario: 接收真实帧后再次补充

- **WHEN** 物理对端发送已知内容且 RX descriptor 可用
- **THEN** 后端 MUST 交付与对端抓包对应的帧并完成单次回收与 refill
- **AND** 后续帧 MUST 可继续接收

#### Scenario: 错误帧与容量边界

- **WHEN** 完成状态包含错误、长度非法、RX 容量耗尽或索引 wrap
- **THEN** 后端 MUST 明确报告丢弃或背压原因，保持访问界限与所有权
- **AND** 合法完成后恢复可用容量，MUST NOT 无界分配或重复回收

### Requirement: 有界轮询 TX 发布与回收

后端 MUST 提供 TX submit、doorbell、terminal 与 reclaim 的可区分阶段，完整保留合法帧内容。队列 Full MUST 不覆盖既有设备 owner；超长或不支持的帧 MUST 明确拒绝，不得静默截断。终态验证 MUST 闭合 controller/status、descriptor 和 buffer 的访问条件，每个完成只回收一次。local completion MUST NOT 等同于 peer delivery。

#### Scenario: 完整帧发送与单次回收

- **WHEN** 有空闲 TX slot，CPU 提交合法完整帧且设备到达完成终态
- **THEN** 后端 MUST 单次回收 slot，对端 MUST 可抓取与提交内容对应的帧
- **AND** 本地完成与对端收包 MUST 分别报告

#### Scenario: Full、wrap 与超长提交

- **WHEN** 队列满、索引 wrap 或帧长度超出支持范围
- **THEN** 后端 MUST 保持已提交对象所有权，Full 与非法长度明确可见
- **AND** 完成后 MUST 恢复容量，合法 wrap 后仍可再次提交

### Requirement: 真板双向协议包验证

RX/TX 硬件队列与真实帧基线通过后，CoM260 Kit MUST 完成 ARP、ICMP、UDP 与 TCP 的双向交换。验证 MUST 对应板上内容、硬件队列终态和物理对端抓包，区分链路、设备完成、协议完成与对端交付；MUST NOT 用 host 单元测试、PHY link 或中断事件替代真板包交换。

#### Scenario: 四类协议交换

- **WHEN** DMA 与真实帧基线通过，物理对端连通
- **THEN** MUST 观察 ARP 请求/应答、ICMP echo 请求/应答、UDP 请求/响应，以及 TCP 连接、双向数据与关闭
- **AND** 数据内容与对端抓包 MUST 一致

#### Scenario: 无对端或协议响应缺席

- **WHEN** link down、对端未连接或协议响应在限定时间内未到达
- **THEN** MUST 明确报告链路或协议等待阶段并有界结束
- **AND** MUST NOT 把未交付报告为成功或冒充 MMIO/cache 故障

### Requirement: 分阶段错误、超时与取消不夺取 DMA 所有权

初始化、submit/doorbell、DMA terminal、reclaim 与协议等待 MUST 各有有界停止和明确失败语义；错误报告 MUST 包含阶段、可取得的原始状态和 owner。初始化 reset 超时 MUST 阻止继续启动。未提交请求允许撤销；提交后的取消只结束等待。未确认设备停止访问时 MUST 保留 backing、拒绝新提交并等待外部复位，MUST NOT 通过 timeout、取消或软件清 OWN 提前释放/复用资源。运行时自动 reset/link 恢复不属于本能力。

#### Scenario: 各阶段超时或硬件错误

- **WHEN** 请求在某阶段不能推进或出现 DMA/controller 错误
- **THEN** MUST 以该阶段、原始状态和 owner 报告失败并有界停止后续操作
- **AND** 仍可能被设备访问的 descriptor/buffer MUST 保留且不可复用

#### Scenario: 提交前后取消

- **WHEN** 请求在提交前或设备拥有期间被取消
- **THEN** 未提交资源 MAY 安全撤销，已提交资源 MUST 保持设备所有权直到合法完成或确认停止
- **AND** 取消本身 MUST NOT 被视为完成或 DMA 停止证据

### Requirement: 临时启动、兼容性与直接验证

本能力 MUST 限定 CoM260 Kit 单 hart RAM 临时启动，保持既有 QEMU/D1/VF2 平台边界和已验收的 K3 boot/link/IRQ 行为。验证 MUST 使用项目测试框架原生输出/退出码与板上目标状态、对端包内容直接判定，遵守公共规则的验证与证据边界。失败或操作者取消后 MUST 报告有界停止与 reset 恢复原系统的实际结果，MUST NOT 写入持久固件或分区。

#### Scenario: 受影响平台与板上旧基线

- **WHEN** K3 DMA/GMAC 路径加入后构建既有平台并执行受影响测试
- **THEN** K3 资源 MUST 只参与 K3 路径，既有 owner/wake 契约和板上已验收阶段 MUST 保持
- **AND** pre-existing 失败 MUST 单独报告，不得计为通过

#### Scenario: 临时启动失败与恢复

- **WHEN** 板上测试失败、超时或被取消
- **THEN** MUST 输出实际失败阶段并有界停留，操作者复位后确认原系统恢复
- **AND** 本次测试 MUST 保持 RAM-only，部分结果不得计为整个能力通过
