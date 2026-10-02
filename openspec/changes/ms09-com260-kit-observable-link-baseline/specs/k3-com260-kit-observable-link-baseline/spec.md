## Purpose

规定 StarryOS 如何复用可追溯的 K3 官方参考资料和真板成功经验，在 CoM260 Kit 的官方 Linux 上只读采集运行资源，再以已核对的板级事实完成可撤销的首次启动，并使早期串口、MAC 控制器及 PHY/link 状态可直接观察；本能力不包含设备中断、DMA 或网络收发。

## ADDED Requirements

### Requirement: 目标板事实先于硬件后端选择

StarryOS 的 CoM260 Kit 适配 MUST 将 `k3` 仓库中可追溯到厂商文档和官方 Linux 源码的已观察内容作为其适用范围内默认采信的参考资源，并将 `others/` 中可核对的真板成功实践用于实现和排障参考。当前实物板的 DTB 选择、启动介质、boot/可用 hart、DRAM 与保留区、UART、MAC/PHY、clock/reset 和物理网口实例 MUST 再与该板实际运行的 Linux/固件或实物标识核对。影响地址、接口、初始化或安全边界的字段未知或互相冲突时，MUST 阻止依赖该字段的后端进入下一阶段；未取得正文的来源、非目标板参数、SoC 最大能力和第三方固定参数 MUST NOT 自动成为当前板运行参数。

#### Scenario: 官方资料形成默认参考

- **WHEN** `k3` 仓库的字段能追溯到已观察的厂商文档或官方 Linux 源码，且目标范围与 CoM260/K3 相符
- **THEN** MS09 MUST 将该字段作为参考基线使用，并记录适用范围与尚需当前板核对的部分
- **AND** 不得仅因本执行环境尚未连接实物板就丢弃该参考结论

#### Scenario: 当前板事实足以选择后端

- **WHEN** 板卡与运行固件提供可核对的 MAC/PHY、内存和启动信息
- **THEN** 适配 MUST 能说明所选资源与当前 Kit 的对应关系
- **AND** 不得依靠另一个板型或候选文件名补齐必要字段

#### Scenario: FDT 与板卡资料冲突

- **WHEN** 运行 FDT、板卡修订或物理连接对某必要资源给出冲突值
- **THEN** 相关后端 MUST 保持未选定或不可用，并报告冲突字段
- **AND** 不得通过试写候选 MMIO 地址掩盖冲突

### Requirement: 官方 Linux 上的只读板级信息采集

MS09 MUST 提供可在当前 CoM260 Kit 官方 Linux 用户态运行的最小采集程序。程序 MUST 只读报告实际运行 FDT 的 model/compatible/chosen、memory/reserved-memory、Linux 可见 CPU、UART/MAC/MDIO/PHY 节点及资源关系，以及可取得的驱动绑定和网络 link 状态；对于缺失、权限不足或 Linux 不能观察的字段 MUST 明确报告未知或采集失败。程序 MUST NOT 改写设备树、sysfs、网络设置、设备寄存器、固件或持久分区，也不得把 Linux 用户态信息冒充 U-Boot 自占内存和安全 RAM 加载区的完整证据。

#### Scenario: 当前板可采集

- **WHEN** 当前 Kit 已启动官方 Linux 且所需只读接口可访问
- **THEN** 采集程序 MUST 输出与运行 FDT 和 Linux 设备状态直接对应的关键字段
- **AND** 这些字段可与官方参考资料逐项核对，以确定后续 StarryOS 平台接入的输入

#### Scenario: 接口缺失、权限不足或固件事实不可见

- **WHEN** 某字段在 Linux 不存在、访问被拒绝，或只存在于启动前固件阶段
- **THEN** 程序 MUST 明确区分缺失、访问错误与不可从 Linux 判定的项目
- **AND** 不得猜测值、写入系统状态或宣称 RAM 启动边界已经闭合

### Requirement: 首次启动保持板载持久固件不变

首次 StarryOS 真板启动 MUST 使用当前板已验证可用的临时 RAM 路径，MUST 在跳转前确认 payload、FDT、入口、固件和保留区不发生地址重叠，并 MUST NOT 修改板载固件或持久分区。启动成功 MUST 有可重复的 polling 串口输出和明确的阶段位置；取消或失败 MUST 给出已验证的恢复结果，不能假定复位必然恢复。

#### Scenario: 安全 RAM 引导

- **WHEN** 当前板内存边界和固件交接已核实，并临时加载匹配的 StarryOS payload 与 FDT
- **THEN** 内核 MUST 在可定位阶段输出首字节，重复启动可观察
- **AND** 原有持久固件与分区 MUST 保持不变

#### Scenario: 地址重叠或交接失败

- **WHEN** payload、FDT、固件或保留区有重叠，或启动交接未到达内核
- **THEN** 本次启动 MUST 停止或报告具体失败阶段
- **AND** 不得把 UART 静默解释为已成功启动

#### Scenario: 操作者取消临时启动

- **WHEN** 操作者在临时加载或观察阶段取消
- **THEN** 系统 MUST 不执行持久写入，并报告恢复原系统的实际结果或无法恢复的状态

### Requirement: 早期串口与现有异步设备独立

CoM260 Kit 首字节 MUST 在异步 UART task、设备 IRQ、DMA、网络栈和 rootfs 尚未初始化时可观察。UART 基址、寄存器 stride 与访问宽度 MUST 分别来自当前板已核实的配置，不得由 stride 推断访问宽度。

#### Scenario: 仅最小平台服务可用

- **WHEN** 固件已把控制权交给 StarryOS，但异步设备和文件系统尚未启动
- **THEN** polling 串口 MUST 能输出可识别的启动阶段信息

#### Scenario: UART 没有输出

- **WHEN** 临时启动后未观察到首字节
- **THEN** 验证 MUST 区分是否到达入口、内存/MMIO 映射是否有效，以及 UART clock/pinmux/连接是否可用
- **AND** 不得把静默计为通过

### Requirement: MAC 寄存器与 PHY/link 构成独立可观测链路

目标 MAC 后端 MUST 在当前板已核实的资源上读取可解释的身份或状态寄存器，并 MUST 把全零/全一或访问异常判为未验证。PHY/MDIO 可访问性与物理 link 状态 MUST 分别报告；clock/reset 状态只可在已核实的 handoff 基础上保留或恢复，不得盲写。MS09 的 link 观察 MUST NOT 宣称设备 IRQ、DMA 或网络包收发已经工作。

#### Scenario: 控制器和链路均可观察

- **WHEN** 当前板 MAC/PHY 资源已核实且网络对端接好
- **THEN** MAC 身份/状态 MUST 可解释且不是全零/全一
- **AND** PHY/MDIO 与 link 结果 MUST 在相同条件下重复一致

#### Scenario: 对端未连接

- **WHEN** MAC 和 PHY/MDIO 可访问，但物理对端缺席或未协商
- **THEN** 系统 MUST 把 link down 与控制器不可访问区分报告
- **AND** 不得把 link down 解释成 DMA 或 IRQ 故障

#### Scenario: 寄存器或 PHY 访问失败

- **WHEN** MAC 读值全零/全一、访问异常，或 PHY/MDIO 无法读取
- **THEN** 对应阶段 MUST 报告失败并停止依赖它的后续初始化
- **AND** 不得自动尝试另一个未确认的板型参数

### Requirement: K3 平台选择不改变既有平台

K3 构建 MUST 选择独立的板级入口和资源，不得隐式落入 QEMU、D1 或 VF2 的 UART、中断或网络路径。既有平台 MUST 保持其现有启动与设备语义；本能力 MUST NOT 修改通用异步网络的所有权、唤醒或恢复契约。

#### Scenario: 选择 K3

- **WHEN** 构建目标明确选择 CoM260 Kit
- **THEN** 平台选择 MUST 使用 K3 对应的入口和资源
- **AND** 不得调用 QEMU VirtIO 或 D1 PLIC 专属初始化

#### Scenario: 构建既有平台

- **WHEN** 构建目标为 QEMU、D1 或 VF2
- **THEN** K3 资源 MUST 不参与这些平台的启动与设备初始化
- **AND** 既有平台可观察行为 MUST 保持
