# k3-com260-kit-device-irq-baseline Specification

## Purpose
规定 StarryOS 在 CoM260 Kit（K3）上建立可诊断的设备中断基线：AIA 控制器底座（IMSIC/APLIC/`stopei` 原子 claim+EOI）经软件自测与真实设备事件验证，设备 cause 在清除前保留并审计，投递可重复且无中断风暴，失败按层定位；本能力不包含 DMA 收发、异步 queue task、多 hart 流量或 timer/UART IRQ 的调度用途。

## Requirements

### Requirement: AIA 控制器底座与 CPU/hart 初始化分离

K3 平台的 S-mode 外部中断底座 MUST 在 boot hart 上初始化 IMSIC 本地中断文件（eidelivery=1、eithreshold=0、清空全部 eie）并将 APLIC 配置为 MSI 模式（先 disable-all 与 clear-pending，再写 `smsicfgaddr/h` 与 `domaincfg` IE|DM）。一次性全局配置（APLIC 域与 MSI 地址）与 per-hart 配置（IMSIC 本地文件与 `sie.SEXT`）MUST 保持显式分离，per-hart 路径 MUST NOT 重复一次性构造。外部中断的 claim 与 EOI MUST 由单条 `csrrw stopei`（写 0）原子完成，且 MUST 在设备 handler 分发之前执行；S-mode 写入 `smsicfgaddrh` 的几何字段 MUST NOT 被假定生效（几何来自 M-mode 固件预配）。寄存器偏移、位域与操作次序 MUST 对齐 R69 参考实现已验证用法并标注来源。

#### Scenario: 初始化后软件自测投递

- **WHEN** AIA 底座初始化完成，自测 handler 已注册，并经 APLIC 软件自测入口（`SETIPNUM_LE` 或 `GENMSI`）触发
- **THEN** SEI trap MUST claim 到预期 EID 并分发到对应 handler，handler 执行有直接可观察输出
- **AND** EOI 已随 claim 原子完成，再次软件触发 MUST 再次投递

#### Scenario: handler 期间到达的新事件不丢失

- **WHEN** 一个 MSI 在 handler 执行期间到达同一中断文件
- **THEN** 该事件 MUST 保留在 pending 中，sret 恢复 SIE 后再次 trap 被收割
- **AND** MUST NOT 因 claim 与 EOI 分离或推迟而在间隙清除该事件

#### Scenario: 初始化失败分层

- **WHEN** 软件自测不投递或初始化 readback 异常
- **THEN** 失败 MUST 以 readback（domaincfg、smsicfgaddr/h、eidelivery/eie）与阶段标记定位到 APLIC 域配置、MSI 地址、IMSIC 文件或路由中的具体层
- **AND** 不得以另一组猜测常量重试或跳层宣告成功

### Requirement: 设备 IRQ claim 与设备 status 对齐

MS10 MUST 以 NIC 子系统的真实设备事件验证 claim 与设备 status 对齐：PHY RTL8211F link-change 中断链（INTN → pinctrl GPIO35 下降沿 → APLIC source 60）。GMAC macirq（source 133）与 eth_wake_irq（source 277）MUST 完成路由配置与状态观测，其事件源依赖 DMA 数据面的事实 MUST 显式记录。设备 handler 在处理 claim 时 MUST 读取中断链上各层的当前 status（pinctrl 边沿 status、PHY INSR），并与被 claim 的 source 一起输出对齐结果。

#### Scenario: 插拔产生对齐的 claim

- **WHEN** 设备链已启用且对链路执行插拔操作
- **THEN** 拔线与插回 MUST 引起真实链路变化和携带设备 status 快照的 claim 输出，claim 的 source 与 PHY link-change status 内容一致
- **AND** EOI 后的再次插拔 MUST 再次投递
- **AND** 人工动作次数 MUST NOT 作为 claim 次数的一对一验收条件；处理前的同类硬件 pending 允许合并

#### Scenario: claim 与 status 不一致判失败

- **WHEN** claim 到达但设备 status 无对应位，或 status 置位而 claim 未到达
- **THEN** MUST 判为对齐失败并定位到 pinctrl 边沿使能、PHY 中断使能或板级接线层
- **AND** 控制器底座（软件自测）已通过的结论 MUST NOT 因此被撤销或冒充设备路径已验证

#### Scenario: MAC 自身中断线路由但不伪造事件

- **WHEN** macirq（133）与 eth_wake_irq（277）完成 source 配置与使能
- **THEN** 其当前状态 MUST 可观测（无事件即无 claim），事件源依赖 DMA 的事实 MUST 记录
- **AND** MUST NOT 以软件手段伪造设备事件冒充 MAC 硬件投递

### Requirement: 破坏性 cause 在 ack 前保留并按次累积

当设备中断链的 cause 寄存器经寄存器事实证明为 W1C 或 clear-on-read（PHY INSR 为 clear-on-read；pinctrl 边沿 status 以裁决事实为准）时，handler MUST 在执行清除动作前保存 cause 快照，并在同一 claim 内累积多位置位。cause 快照 MUST NOT 作为 descriptor completion 结论（无 DMA 数据面时的 R53 边界）。

#### Scenario: 清除前保存快照

- **WHEN** handler 处理一个携带破坏性 cause 的 claim
- **THEN** 输出 MUST 含清除前的 cause 快照，清除动作在快照之后发生
- **AND** 清除后再次读取不得用于否定快照内容

#### Scenario: 组合 cause 不因 ack 丢失

- **WHEN** 一次 claim 对应的窗口内发生多个 cause 置位
- **THEN** 该次快照 MUST 包含全部置位位，不因单次 ack 丢弃
- **AND** 后续独立事件 MUST 以新 claim 呈现，不与既有快照混写

### Requirement: 可重复投递且无中断风暴

设备与软件触发的投递 MUST 可重复：受控逐次软件触发均到达且计数一致；真实插拔 MUST 证明链路变化产生对齐的设备 claim，ack 后后续独立变化仍可投递，不要求人工插拔次数等于 claim 次数或固定循环次数。处理前同类 pending 允许合并，但读取时已置位的 cause MUST 完整保留。无事件窗口内 claim 计数 MUST 为零。禁用路径（APLIC `CLRIENUM` / IMSIC disable）生效后 MUST 停止投递。电平源持续 assert 时的行为 MUST 可观测并可解释，不得形成自激风暴或无界循环。

#### Scenario: 静默窗口无 claim

- **WHEN** 设备链启用后处于无事件观察窗口
- **THEN** claim 计数 MUST 保持为零
- **AND** 非零计数即判为风暴并进入诊断

#### Scenario: disable 后停止投递

- **WHEN** source 或对应 EID 被禁用后再次软件触发
- **THEN** 不得产生 handler 分发
- **AND** 重新使能后的触发恢复正常投递

#### Scenario: 受控软件重复触发一致到达

- **WHEN** 同一软件触发源被逐次触发 N 次，每次等待上一事件处理后再触发
- **THEN** claim/handler 计数 MUST 与触发次数一致
- **AND** 差异即判为丢失或重复投递并定位到对应层

#### Scenario: 真实插拔有效且可再次投递

- **WHEN** 已启用 PHY 中断链，并执行拔线与插回
- **THEN** MUST 观察到 link 状态变化与 source60/PHY INSR link-change 对齐的 claim，ack 后后续独立变化仍可投递
- **AND** MUST 保留原始 pinctrl/PHY cause 快照，稳定链路后不得持续自激 claim
- **AND** MUST NOT 因人工动作与 claim 次数不相等判失败

### Requirement: 验证纪律与平台兼容

MS10 的验证 MUST 以目标行为直接判定：host 侧用项目现有测试框架与其原生退出码，板上以串口原始输出的阶段标记、readback 与逐事件行直接判定；MUST NOT 引入任何身份型证据机制（hash/指纹、revision pin、run-id、session/execution ID、peer/host pin、artifact manifest、日志 hash 链、时间证明及其 capture、audit/qualification 工具或专用判定脚本）。既有 QEMU/D1/VF2 构建行为与 K3 上 MS09 各探针阶段输出 MUST 保持不回归；板上 IRQ 观察按用户裁决以 `required` Evidence 保存纯串口日志（不带身份字段）。

#### Scenario: 验证不引入判定层

- **WHEN** 为 MS10 建立 host 测试或板上验证
- **THEN** 判定 MUST 依据被测对象的原生输出与退出码，必要时用现成通用工具的单步管道过滤
- **AND** 出现身份型证据机制时 MUST 删除该机制并改为直接判定后重新验证

#### Scenario: 既有平台不回归

- **WHEN** K3 IrqIf 实化与探针扩展合入后构建既有平台
- **THEN** 非 K3 平台 MUST 不选择 AIA 常量或改变中断行为
- **AND** K3 构建上 MS09 阶段标记（boot/APMU/MAC/PHY/link）MUST 依次完整出现

#### Scenario: 板上失败可恢复

- **WHEN** 任一板上阶段失败、超时或被操作者取消
- **THEN** 系统 MUST 在报告失败阶段后有界停止（park），`reset` 回原系统
- **AND** 不得把部分完成的观察冒充为该阶段通过
