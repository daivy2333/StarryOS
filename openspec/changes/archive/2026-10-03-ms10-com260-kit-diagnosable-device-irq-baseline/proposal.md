## Why

MS09 已在 CoM260 Kit 上建立可重复 RAM 启动、MAC 寄存器与 PHY/link 可观测基线，但 `crates/axplat-riscv64-k3` 的 `IrqIf` 仍是 fail-closed stub，真板中断投递尚无任何证据。真板中断控制器（K3 为 RISC-V AIA：APLIC MSI-mode + IMSIC）和设备触发模式是独立高风险故障域，QEMU 的 PLIC 证据不能替代（tasks.md MS10 Rationale、M41）。板上 DTB 与同板参考实现（R69 tgoskits，按 M42 默认采信）给出完整事实与已验证用法：APLIC 仅支持 MSI 模式、`stopei` 必须单条 `csrrw` 原子 claim+complete 且先于 handler、PHY link-change 是无 DMA 时 NIC 子系统唯一可达的真实设备事件。本 change 建立 MS10（T16）的目标板可诊断设备中断基线。

## What Changes

- 在 K3 平台组件中把 `IrqIf` 从 fail-closed stub 实化为 AIA 底座：IMSIC 本地中断文件初始化（`siselect/sireg` 间接访问，eidelivery=1、eithreshold=0、清空 eie）、APLIC 进入 MSI 模式（disable-all + clear-pending + `smsicfgaddr/h` + `domaincfg` IE|DM）、设备 source 的 register/set_enable（触发类型 + MSI target 配置）、外部中断 claim/dispatch（单条 `csrrw stopei` 原子 claim+EOI，先于 handler）、SBI `send_ipi`。一次性全局阶段（APLIC 域配置）与 per-hart 阶段（IMSIC 本地文件 + `sie.SEXT`）显式分离。
- 沿用 R69 已验证的寄存器契约与用法语义（寄存器偏移、target 位域、触发类型编码、清 pending 后再 enable），并保留其负面样本为审查反例：不引入 `try_lock` 丢事件、不推迟 EOI 到 handler 之后。
- 控制器底座验收用 APLIC 软件自测入口（`SETIPNUM_LE` 有线源 pending 置位、`GENMSI` 直接 MSI）：证明 claim/dispatch/EOI 链、EOI 后再触发、disable 后停止投递、无风暴。
- 真实设备对齐按用户裁决采用 PHY link-change 链：RTL8211F INTN → pinctrl GPIO35 下降沿边沿检测 → APLIC source 60（板上 DTB `ethernet-phy@1` 的 `interrupt-parent=<pinctrl>`、`interrupts=<35 EDGE_FALLING>`）。handler 在清除前保存 pinctrl 边沿 status 与 PHY INSR（clear-on-read）快照，claim 行与设备 status 对齐；插拔循环提供可重复触发。
- MAC 自身 macirq（source 133）与 eth_wake_irq（source 277）完成路由配置与状态观测；其事件源依赖 DMA 数据面（MS11 范围）的事实显式记录，不伪造事件。
- 失败按层定位（固件 handoff / APLIC 域配置 / IMSIC 文件 / source 路由 / 设备链），以 readback 与阶段标记直接判定；板上验证经 R68 启动环，输出走 polling console。
- 验证遵守公共规则：禁止任何身份型证据机制（hash、指纹、revision pin、run-id、manifest、时间证明、审计链、资格判定工具或专用判定脚本）；host 侧用现有测试框架与原生退出码，板上以串口原始输出直接判定。板上 IRQ 观察日志按用户裁决以 `required` Evidence 留档（纯串口日志，无身份字段）。
- 本变更限于 tasks 中的 T16。DMA descriptor、网络收发、异步 queue task、多 hart 流量、timer/UART IRQ 的调度与交互用途（MS17）、reset 恢复与性能留给后续工作；QEMU/D1/VF2 行为保持不变。

### Scenario Sketch

#### Happy Path：AIA 控制器底座初始化与软件自测投递

- **前置状态**：R68 RAM 启动环可用；MS09 阶段（boot/APMU/MAC/PHY 探针）输出正常；OpenSBI 完成 M-mode 交接（板上 Linux 以同一固件链运行 AIA，MSI 几何已预配为合理假设，板上首触裁决）。
- **触发动作**：K3 平台完成 IMSIC 本地文件 + APLIC MSI 模式初始化，注册自测 handler，经 `SETIPNUM_LE`/`GENMSI` 触发。
- **可观察结果**：SEI trap → claim 返回预期 EID → handler 执行（计数/标记）→ EOI 完成；重复触发均到达；disable 后不再投递；静默窗口无 claim。
- **失败边界**：自测不投递时按 readback（domaincfg/smsicfgaddr/eidelivery）分层定位，不盲试参数。

#### Happy Path：PHY link-change 设备 IRQ 与 status 对齐

- **前置状态**：控制器底座已验证；MDIO/PHY 基线（PHYID=0x001cc916）可读；RJ45 对端与手工插拔可配合。
- **触发动作**：启用 pinctrl GPIO35 下降沿中断与 RTL8211F link-change 中断（INER：page `0xa42`、reg `0x12`、link 位 4；INSR：page `0xa43`、reg `0x1d`，clear-on-read），执行插拔循环。页地址按 [Linux RTL8211F 驱动](https://code.googlesource.com/linux/torvalds/linux/+/21e4675d9305f6ccd20b95d943882d607c8ae288/drivers/net/phy/realtek.c) 修正原稿的 page 3，不改变已批准设备路径。
- **可观察结果**：拔线与插回引起真实 link 变化，出现 source60 claim，claim 行携带 pinctrl 原始边沿 status 与 PHY INSR link-change 快照（清除前保存）；ack/EOI 后后续独立链路变化仍可投递。验收证明插拔有效，不要求人工插拔次数等于 claim 次数。
- **失败边界**：claim 到达但设备 status 无对应位，或 status 有位而 claim 未到达，均判为对齐失败并定位到对应层。

#### Happy Path：组合 cause 与重复触发无风暴

- **前置状态**：设备路径已通。
- **触发动作**：执行拔线与插回；若处理前又发生链路变化，允许同类 pending 合并。
- **可观察结果**：单次 claim 的快照保留读取时全部已置位 cause 位，后续独立事件仍可投递且无自激风暴；不设固定插拔循环次数，也不以快速变化的一对一计数作为验收条件。组合位保存由 host 测试覆盖，真板输出实际原始值。
- **失败边界**：静默窗口出现非零 claim、或 cause 位在快照中丢失，均为验收失败。

#### Sad Path：固件 handoff 或初始化失败分层

- **前置状态**：AIA 初始化尚未完成或自测不投递。
- **触发动作**：执行初始化并 readback 关键寄存器。
- **可观察结果**：失败归属具体层（APLIC 域配置 / MSI 地址配置 / IMSIC 文件 / 路由），阶段标记输出到串口；已完成的独立结论保留。
- **失败边界**：不得以另一组猜测常量重试或跳层宣告成功。

#### Sad Path：设备路径不触发

- **前置状态**：控制器底座软件自测已通过。
- **触发动作**：启用设备链后插拔。
- **可观察结果**：若 claim 未到达，定位到 pinctrl 边沿使能 / PHY INER / 板级接线层；控制器底座结论不受影响，设备路径结论保持未闭合。
- **失败边界**：不得把控制器自测通过冒充设备路径已验证，也不得改写其他候选引脚/常量反复试错。

#### Edge Case：spurious claim、未知 EID 与 handler 期间新事件

- **前置状态**：底座运行中。
- **触发动作**：`stopei` 返回 0、未知 EID 到达、handler 执行期间新 MSI 到达。
- **可观察结果**：spurious 与未知 EID 计数不 panic；handler 期间的新事件保留在 pending，sret 后再次 trap 收割，无丢失。
- **失败边界**：丢失 handler 期间事件或因 spurious 崩溃均为失败。

#### Error / Timeout / Cancel：有界停止与恢复

- **前置状态**：任一板上阶段未成功。
- **触发动作**：寄存器访问异常、有界等待超时或操作者取消插拔。
- **可观察结果**：失败归属当前阶段并 park；`reset` 经 R68 回原系统；已完成阶段的串口结论保留。
- **失败边界**：无界等待、静默进入下一阶段均不算通过。

#### Compatibility：既有平台与验证纪律

- **前置状态**：QEMU/D1/VF2 构建与 MS09 探针阶段正常。
- **触发动作**：合入 K3 IrqIf 实化与探针扩展。
- **可观察结果**：非 K3 平台不选择 AIA 常量；K3 构建上 MS09 各阶段标记不变；host 测试以现有框架原生退出码判定。
- **失败边界**：出现身份型证据机制（hash/manifest/run-id/判定器等）即违反本 change 约束，必须移除后以目标行为重新验证。

## Capabilities

### New Capabilities

- `k3-com260-kit-device-irq-baseline`: 定义 CoM260 Kit 上 AIA 控制器底座（IMSIC/APLIC/stopei claim+EOI）、软件自测、PHY link-change 设备 IRQ 与设备 status 对齐、破坏性 cause 审计、无风暴与失败分层的可验证基线及边界。

### Modified Capabilities

- None. `k3-com260-kit-observable-link-baseline`（MS09 合并）只覆盖启动/寄存器/链路可观测性，不涉及中断投递；本变更新增独立能力，不修改既有 requirement。

## Impact

- 代码面（Phase 2 调查后最终确定）：`crates/axplat-riscv64-k3/`（`irq_stub.rs` 替换为实化 irq 模块；`axconfig.toml` 增加 APLIC MMIO 窗口；`init.rs` 挂接 per-hart 初始化）、`kernel/src/platform/k3.rs`（park 路径增加 IRQ 观察阶段）、`kernel/src/platform/`（按需新增 pinctrl/设备 cause 探针模块，沿用 `ProbeEnv` 与 host 可测纯函数模式）、现有 host harness（`tests/`）新增 AIA 纯逻辑测试。
- 需要 CoM260 Kit、R68 启动环、RJ45 对端与手工插拔配合；板上 IRQ 观察日志按用户裁决以 `required` Evidence 留档。
- 参考输入（按 M42 默认采信并标注来源）：R69 `tgoskits/drivers/intc/{riscv_imsic,riscv_aplic}` 与 `somehal/arch/riscv64/imsic_aplic.rs`（寄存器契约、用法语义、claim/EOI 板上教训）、`tgoskits/drivers/ax-driver/src/soc/k3/pinctrl/regs.rs`（pad edge 位与 APBC 解锁协议）、板上 DTB `tools/k3_com260_ifx.dtb`（APLIC/IMSIC/GMAC/PHY/pinctrl 节点事实）、R68/R61/R65/R66、k3 仓 interrupts/network 主题文档。
- 不修改 OpenSBI/U-Boot、持久分区、通用网络栈、VirtIO queue/recovery、异步 NIC 契约；不引入 `rdrive`/`somehal` 框架代码（R69 框架边界）。

## Gate 1 Approval

- Status: approved。用户在收到 proposal 与 delta specs 摘要后回复原话「批准」（2026-10-02）；批准范围为 T16 的需求和场景，不授权实施。
- 已并入的用户决策（2026-10-02）：真实设备触发路径采用「控制器底座 + 软件自测 + PHY link-change（pinctrl→APLIC 60）」，MAC 133/277 路由配置并观测状态、事件源归属 MS10 后续里程碑之外；板上阶段 Persisted Evidence 为 `required`（纯串口日志）。用户同日明确要求全程遵守规范、禁止身份工程。

### 计划修订裁决（2026-10-03）

用户原话：「拔插计数是形式主义，核心问题是证明拔插有效就行，所以不用管这个小问题，缺少的源码依据可以用别的相关信息代替就行，不要把工作阻塞在这些问题上」。

- R2/R4 的人工插拔验收调整为真实 link 变化、source60/INSR 对齐、ack 后再次投递和静默无风暴；撤销固定循环次数和人工动作=N次claim。软件自测使用受控逐次触发，其直接计数仍用于证明控制器投递。
- 不再要求取得特定 vendor handler 文件。pinctrl 操作结合本板DTB、R69 K3位定义、SpacemiT K1同位字段说明和Marvell相关清边沿实现形成固定方案，推导的K3适用性在实际设备阶段直接验证，见design D5。该裁决授权采用替代资料；不把相关SoC事实宣称为已验证K3事实，不豁免真实设备投递/快照/恢复验收。
- 本次是需求与资料策略修订，不授权Act。原draft Cycle保留；修订Cycle为 `iterations/000-initial/001-replan.md`，Iteration Map不变。

## Gate 2 Plan Approval

- Status: approved。
- 用户在收到修订计划与当前Cycle后回复原话「批准」（2026-10-03），批准本change的设计、任务和两轮Iteration Map，包括 `iterations/000-initial/001-replan.md` 当前执行契约。
- Gate 2: PASS。没有Missing；R4 Simplified已由用户批准；资料替代策略与固定设备契约已纳入获批计划。当前Plan Context转为ready。
- 本次登记计划批准；按公共规则阶段边界，实施仍由后续Act指令启动。
