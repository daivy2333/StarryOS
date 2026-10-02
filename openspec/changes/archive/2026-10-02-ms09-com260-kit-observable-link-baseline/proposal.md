## Why

MS08 只证明 QEMU VirtIO 的多 hart 软件基线。用户已确认下一目标板为 CoM260 Kit，当前分支仍无 K3 平台入口。`k3` 参考仓库已整理可追溯到厂商文档和官方 Linux 源码的 K3/CoM260 资料，也保存了历史真机记录与 `others/` 中第三方成功实践；这些资料应直接成为 MS09 的参考基线，而不是被排除在里程碑之外。当前实物运行的 DTB、固件 handoff、内存与设备状态仍需在板载官方 Linux 和 bootloader 上核对，才能安全选择首次启动参数。

## What Changes

- 建立三层板级信息基线：默认采信 `k3/docs/` 中可追溯到厂商文档或官方 Linux 源码的内容，严格按其目标板型、源码版本和已取得正文的范围使用；将 `k3/others/` 的真板成功实践作为实现与排障参考；将当前 CoM260 Kit 的 Linux/固件运行信息作为实际实例选择与 RAM 安全边界依据。冲突和未取得正文的字段明确保留，不把三层资料混为同一种证据。
- 在 StarryOS 仓库规划并实现一个最小、只读的 Linux 用户态板级信息采集程序，交叉编译后在当前 Kit 的官方 Linux 上运行。直接报告运行 FDT 的 model/compatible/chosen、memory/reserved-memory、Linux 可见 CPU、UART/MAC/MDIO/PHY 资源及 phandle 关系、驱动绑定和网络 link 状态；缺失或无权限字段明确报告，不写设备寄存器、网络配置、固件或持久分区。串口启动日志、U-Boot 自占/RAM 加载区和复位恢复结果仍由现场只读/普通操作补齐。
- 保留当前板可用的 BootROM→FSBL/ESOS→OpenSBI→U-Boot 链，首轮仅用可撤销的 RAM/FIT 路径启动 StarryOS；核对加载、展开、入口及 FDT 地址不覆盖固件或保留区，并取得可重复的 polling 首字节与启动阶段诊断。
- 为 K3 增加独立的构建/平台分流，避免落入现有 QEMU/D1 路径；在已核实的 clock/reset 条件下读取目标 MAC 身份/状态寄存器，并建立 PHY/MDIO 与物理 link 的可重复观察。
- 对无输出、FDT/修订不匹配、MMIO 全零/全一、PHY 不可见、link down、超时或用户取消分别给出失败阶段；事实不足时停止当前阶段，不以猜测参数继续初始化。
- 本变更限于 tasks 中的 T13–T15。设备 IRQ delivery、DMA descriptor、网络包收发、异步队列、reset 恢复、多 hart 压力和持久刷写留给后续工作；现有 QEMU、D1 与 VF2 行为保持不变。

### Scenario Sketch

#### Happy Path：参考资料成为可用基线

- **前置状态**：`k3` 仓库有来源可追溯的官方文档/源码整理、历史真机记录及第三方工程。
- **触发动作**：按具体字段读取 `docs/` 的来源与适用范围、`others/` 的已运行路径，并列出它们对 MS09 的可用结论与缺口。
- **可观察结果**：官方资料在其直接支持的范围内成为默认参考；第三方经验提供构建、RAM 启动和故障定位线索；每个需要当前板核对的字段有明确原因和采集入口。
- **失败边界**：未取得官网正文的摘要、非目标板资料或第三方常量不得冒充已确认的当前板参数。

#### Happy Path：板载 Linux 采集与板级事实核对

- **前置状态**：实物 Kit 可启动官方 Linux，并有可用的串口或文件传输通道；官方参考资料基线已列出。
- **触发动作**：运行只读采集程序，配合启动日志、U-Boot 只读信息和板卡标识，核对内存、hart、UART、MAC、PHY 及 clock/reset。
- **可观察结果**：程序输出实际运行 FDT 和 Linux 设备状态；用于平台接入的字段能够与当前板或当前固件对应，冲突字段可见，未覆盖的 U-Boot/RAM 事实由现场补齐。
- **失败边界**：Linux 未暴露的字段、权限错误或设备不存在必须报告为未知；不能以默认 DTB 文件名、历史 IRQ 编号或 SoC 核数补值。

#### Sad Path：板载采集不完整或参考资料冲突

- **前置状态**：官方资料、第三方经验、运行 FDT 或 Linux 状态对同一必要字段不一致，或当前系统未开放某只读接口。
- **触发动作**：采集程序读取并进行字段对照。
- **可观察结果**：输出实际读到的值、来源及缺失/权限错误；仅阻止依赖冲突字段的后续硬件操作，已确认的独立字段仍可用于规划。
- **失败边界**：不得为“采齐数据”改写 sysfs、网络配置、DTB、U-Boot 环境或任意 MMIO。

#### Happy Path：RAM 启动与首字节

- **前置状态**：已取得当前内存/保留区和 U-Boot 占用范围，板载固件保持原样。
- **触发动作**：将匹配的 StarryOS payload 与 FDT 临时装入 RAM，由板载 U-Boot 交接。
- **可观察结果**：FIT/入口/DTB 边界可核对；polling UART 在可定位阶段输出首字节，重复启动与复位返回原系统的结果可观察。
- **失败边界**：镜像覆盖自身、固件或保留区；首字节缺失且不能区分未跳转、页表/MMIO 和 UART 状态，均不得宣称启动成功。

#### Happy Path：MAC 与 PHY/link

- **前置状态**：运行 FDT/板级资料确定 MAC 实例、MMIO 与 PHY 连接；首字节稳定。
- **触发动作**：按已核实的 bootloader handoff 保留或恢复 clock/reset，读取 MAC 身份/状态、MDIO/PHY 和物理 link。
- **可观察结果**：寄存器不是全零/全一且身份可解释；PHY/link 或等效链路状态在相同接线条件下重复一致。
- **失败边界**：只有 link 灯亮、Linux 曾经联网，或仅凭静态 DTS 节点，不能判定 StarryOS 已访问正确控制器。

#### Sad Path：来源冲突或设备不可访问

- **前置状态**：板卡修订、运行 FDT 与候选 DTS 冲突，或 MAC/PHY 读取异常。
- **触发动作**：执行事实核对或受控寄存器/MDIO 观察。
- **可观察结果**：记录冲突或访问失败所在层，停止后端选择或后续硬件初始化；保留已可复现的串口/启动基线。
- **失败边界**：不得改用另一个候选地址、盲写 clock/reset 或继续进入 DMA/IRQ 来掩盖失败。

#### Edge Case：异构 hart、内存边界与外部链路

- **前置状态**：固件暴露的 hart 集合与 SoC 8 X100 + 8 A100 能力不同，或 RAM 可用区被保留段/U-Boot 占用切分；网络对端未连接或未完成协商。
- **触发动作**：选择 boot hart、payload/FDT 区间并观察 PHY/link。
- **可观察结果**：仅按固件实际可用 hart 和安全内存区间建立最小启动；link down 与 PHY/MDIO 不可访问分开报告。
- **失败边界**：不得假设 16 个 hart 全可调度、照搬示例 FIT 地址，或把缺少对端造成的 link down 当作 MAC 寄存器失败。

#### Error / Timeout / Cancel：有界停止和回退

- **前置状态**：Linux 只读采集、RAM stage、启动跳转、UART、MAC 或 PHY 观察尚未成功。
- **触发动作**：读取失败、步骤超时、返回错误，或操作者取消采集/临时启动。
- **可观察结果**：失败归属当前阶段；采集可保留已读到的独立字段并标明缺项；未执行持久写入。RAM 启动失败时按当前板已验证的复位/恢复路径返回原系统，无法恢复则明确报告并停机。
- **失败边界**：无界等待、把部分采集当成完整事实、静默转入下一阶段、把复位能力当作未经验证的保证，均不算通过。

#### Compatibility：既有平台保持

- **前置状态**：使用已有 QEMU、D1 或 VF2 配置构建与启动。
- **触发动作**：加入 K3 构建/平台分流。
- **可观察结果**：既有平台不选择 K3 的 MMIO、AIA 或 boot 参数；原有平台行为不回归。
- **失败边界**：K3 feature 被默认非 D1 分支误路由到 QEMU descriptor/UART/VirtIO handler，或修改既有异步网络契约，均不属于可接受结果。

## Capabilities

### New Capabilities

- `k3-com260-kit-observable-link-baseline`: 定义 CoM260 Kit 当前板事实、可恢复的 RAM 启动、polling 首字节、MAC 寄存器和 PHY/link 的可观测基线及失败边界。

### Modified Capabilities

- None. `platform-descriptor-early-console` 已要求新增板卡前集中平台事实并保持 early console 独立；本变更在 K3 专属能力中落实该约束，不改变既有通用 requirement。

## Impact

- 预计涉及一个可在官方 Linux 运行的最小只读采集程序及其构建/测试入口，随后才涉及根 Cargo/Make 构建选择、K3 平台组件、kernel 平台 descriptor/entry/early console 分流；具体代码面须经修订范围获批后的实现调查确定。
- 需要 CoM260 Kit 当前修订、官方 Linux 运行环境、固件/内存信息、串口访问与 RJ45 对端；参考仓库 `/home/daivy/projects/serial/work/k3` 的官方来源资料默认作为参考资源，`others/` 和历史报告作为分层工程经验。
- 不修改 OpenSBI、ESOS、U-Boot、持久分区、通用网络栈、VirtIO queue/recovery 或异步 NIC 接口。

## Gate 1 Approval

- Status: approved。用户在收到完整提案与三个默认条件后回复原话「批准」；批准范围为 T13–T15 的需求和场景，不授权实施或持久刷写。
- 获批默认条件：沿用板载固件，首轮只做 RAM/FIT 临时启动；用户能提供或协助采集串口日志、运行 FDT/板卡修订，并提供可观察 PHY/link 的 RJ45 对端。具体板上数值仍待现场取得；若现场条件与假设不符，返回 Plan 修订。

## Scope Revision Approval

- 2026-09-29 用户要求将可追溯的官方资料默认采信为参考资源，把“编写代码并在官方 Linux 上采集当前板信息”纳入 MS09，同时使用 `others/` 中已在 K3 真板成功运行的经验。先前「批准」仅覆盖原范围。
- 修订后的 Why、What Changes、Scenario Sketch 和 Impact 提交审定后，用户再次回复原话「批准」。Gate 1 对修订范围通过；此批准授权 Plan 完成调查和执行计划，不等同于批准尚未交付的详细计划或授权 Act、刷写/改动板上固件。
