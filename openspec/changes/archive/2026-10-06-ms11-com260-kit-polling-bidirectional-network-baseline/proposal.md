## Why

CoM260 Kit 已有可撤销 RAM 启动、GMAC/PHY 链路和 AIA/PHY 中断基线，但尚无 StarryOS 真板 DMA 队列与网络包收发结论。下一里程碑需要以轮询双向收发闭合 DMA 地址、cache/barrier、descriptor 与 buffer 所有权，为后续异步适配提供硬件基线。

## What Changes

- 明确本板 GMAC 的 CPU 地址、设备地址、DMA 内存映射与可见性契约；分别追踪 descriptor 和 buffer 所有权。
- 实现最小轮询 RX refill/reap 与 TX submit/terminal/reclaim，提供有界容量、明确错误和分阶段超时。
- 先验证硬件队列移动及真实以太帧，再验证 ARP、ICMP、UDP 和 TCP 双向协议交换与对端抓包一致。
- 保持 RAM-only 启动和既有平台边界；未证明 DMA 停止时保留 backing、拒绝新提交并停留等待复位。
- 规划使用官方可追溯资料、同板参考实现与运行经验，遵守公共规则及 `.agents/memory/project-model.md` 的板级资料默认采信与用法对齐约束。

## Capabilities

### New Capabilities

- `k3-com260-kit-polling-network-baseline`：本板 GMAC DMA 内存、轮询队列、错误与真实双向网络基线。

### Modified Capabilities

无。既有 `k3-com260-kit-observable-link-baseline` 和 `k3-com260-kit-device-irq-baseline` 的验收范围保持；新能力在其上继续，不扩大既有能力的完成声明。

## Impact

影响 K3 平台 cache 操作、`kernel/src/drivers/k3_gmac/` 轮询后端、K3 前台板测入口、K3 独立 smoltcp 接入与相应 host 测试。调查结果、文件和接口见 `design.md` 与当前 Cycle；本次只写计划，不修改产品代码，不同步全局状态。

## Requirements and Scope

| 编号 | 必须交付的结果 |
|---|---|
| R1 | 地址转换与 DMA 内存可见性有明确依据；descriptor 与 buffer 分别同步，越界或不支持的映射明确拒绝 |
| R2 | 有界轮询 RX 能接收真实帧、校验长度/状态、补充队列；坏帧、队列满与 wrap 不破坏 owner |
| R3 | 有界轮询 TX 能发布完整帧、观察终态并仅回收一次；Full 不覆盖设备 owner，completion 不等于对端交付 |
| R4 | ARP/ICMP/UDP/TCP 双向交换成功，板上内容与对端抓包可对应；协议验证以硬件队列基线通过为前置 |
| R5 | 错误、submit/doorbell、terminal、reclaim 超时与取消按阶段报告；DMA 未确认停止时保留资源并 fail closed |
| R6 | RAM-only、单 hart、可复位恢复；保持既有平台与已验收 K3 行为，验证直接依据目标输出和原生退出结果 |

范围来自 `.agents/docs/tasks.md` 的“目标控制器轮询双向网络基线”（T17–T19）。非目标：异步 queue task/wake、运行时设备 reset 与 link-flap 恢复、SMP、offload、零拷贝、吞吐优化、交互 shell、存储/rootfs 和持久部署。初始化所需的有界 controller reset 与失败停止属于本次范围；恢复后的重新收发属于后续恢复里程碑。

## Scenario Sketch

以下场景及默认假设已获批准；无用户接受的缺口或豁免。

| 场景 | 前置状态 | 动作 | 可观察结果 | 失败边界 |
|---|---|---|---|---|
| S1 正常 DMA 发布/完成 | 本板资源、内存映射与 link 已有依据，slot 为 CPU-owned | 准备地址/内容，按契约同步并发布，轮询完成后同步回收 | 设备移动 descriptor，CPU 读取正确长度/内容，owner 单次转换闭合 | 地址、映射或同步契约冲突即停止，不试写猜测值 |
| S2 轮询 RX | RX 已补充，物理对端连接 | 对端发送已知内容的帧 | 接收内容与抓包一致，descriptor/buffer 回收并可再次补充 | 无真实接收不计 DMA/RX 通过 |
| S3 轮询 TX | TX 有空闲容量，对端可抓包 | 提交已知完整帧，轮询 terminal 并 reclaim | 对端抓得完整帧，descriptor 不重复回收 | OWN 清零只证明本地终态；没有对端帧不计交付通过 |
| S4 ring 满与 wrap | 有界 ring 中存在设备 owner 或索引到末尾 | 填满并尝试额外提交；完成后再次提交/接收 | Full 可见，无 owner 覆盖；容量恢复与 wrap 后继续工作 | 不无界分配、不以反复轮询掩盖 Full |
| S5 坏帧/长度边界 | RX 完成或准备 TX | 注入错误状态、短帧、合法边界长度及超长帧 | 接受合法完整帧；拒绝/丢弃非法帧并报告原因，缓冲区不越界 | host 覆盖不能冒充硬件 CRC/坏帧注入；实际硬件能力另作调查 |
| S6 四类协议 | RX/TX 真帧基线已通过，存在物理网络对端 | ARP 请求/应答，ICMP echo，UDP 请求/响应，TCP 连接/双向数据/关闭 | 各协议有完整双向内容与对端抓包，可区分 local completion 与 peer delivery | 不能以 PHY link、IRQ 或仅协议栈 host 测试代替包交换 |
| S7 无对端/link down | 控制器可访问但 link down | 开始轮询操作或等待协议响应 | 明确报告 link/对端状态与有界结束 | link down 不伪装为 MMIO/cache 故障；不做自动恢复 |
| S8 初始化与 DMA 错误 | 初始化或请求处于某一阶段 | 注入 reset timeout、DMA fatal、异常 descriptor/status | 阶段、原始状态、owner 和错误明确，拒绝依赖失败阶段的后续操作 | reset timeout 后不继续启动；设备仍可能访问的 backing 不释放 |
| S9 分阶段超时 | 请求已进入 submit、terminal 或 reclaim 阶段 | 在该阶段停止推进至 deadline | 报告超时阶段和 owner；device-owned backing 保留，不复用 | 不照搬 SDMMC 阶段/数值，不把 timeout 当完成 |
| S10 取消 | 请求未提交或已归设备 | 操作者取消观察/测试，或请求进入取消分支 | 未提交可撤销；已提交取消只结束等待，仍保留设备 owner；reset 后恢复原系统 | 取消不凭空取得 buffer 所有权，不扩展自动 reset 恢复 |
| S11 平台与旧基线兼容 | 既有 QEMU/D1/VF2 与 K3 boot/link/IRQ 基线存在 | 构建受影响平台并执行相关测试 | K3 资源只进入 K3 路径，旧 owner/wake 契约及板上既有阶段保持 | 既有失败单独记录，不虚报回归通过，也不顺带修复无关问题 |

### 已批准的默认假设

- 沿用已批准路线：单 hart、已有物理网口、现有 RAM 启动环；不新增 shell/rootfs 前置。
- 四类协议全部保留，不因最小后端实现简单而裁剪；完成前同时要有本地 owner 闭合与对端收包结果。
- 可稳定注入的边界与错误先用直接调用生产逻辑的项目 host 测试证明；真板另外证明 DMA 和真实双向内容。硬件专有错误若无法安全注入，调查时明确实际能力并返回需求决策，不静默缩减。
- 参考资料中明标 unknown/inference 的内容不升格。调查优先检索同板参考代码和经验，不能仅以“未经本仓验证”新增采信前置。
- 没有固定性能门槛或 soak 次数；观察窗、ring 容量、MTU、具体网络拓扑与超时值在实现调查后以明确契约写入设计，不留给实施者决定。

## Reference Inputs

- 本仓 `.agents/analysis/k3-reference-repository-information-value.md` 的轮询网络优先读取表；`.agents/runbooks/com260-uboot-usb-starryos-ram-boot.md` 的临时启动与恢复路径。
- `/home/daivy/projects/serial/work/k3/docs/dma/k3-dma-and-memory-ownership.md` 与 `k3-cache-pma-address-translation.md`：地址层、可见性与 descriptor/buffer ownership。
- 同仓 `docs/network/k3-gmac-dma-irq.md`、`openspec/specs/k3-com260-gmac-network-baseline/spec.md`：GMAC 数据面、错误与通知边界。
- 同仓 `others/Rt-Async-AMP/tgoskits/`、`.claude/analysis/`、`.claude/runbooks/` 与 `openspec/` 中相关代码及真板过程：按本仓资料采信约束消费，保留来源和负面样本。

## Approval and Gates

- 用户指令：“开始计划下一个ms的change，记得遵守各种规范，参考资料仓库的信息和经验”。这授权创建计划产物；既有路线确定候选范围，不等于已批准新增场景假设。
- 用户批准：“那就批准当前的工作，开始计划吧”。批准当前 R1–R6、S1–S11 及默认假设，授权调查和形成完整计划；不构成尚未形成的执行计划批准或 Act 授权。
- 2026-10-03 用户计划审计原话：“批准”。批准本次交付的设计、七任务/三轮Map、首轮Cycle与验证契约；无范围修改、简化或风险豁免。
- Gate 1: PASS — 需求、BDD、场景和范围获批；无简化、缺口或豁免。
- Gate 2: PASS — 技术调查、设计、追踪、首轮契约与验证均已闭合，用户批准计划；当前 Cycle 为 `ready`，可按后续 Act 指令执行。
- 下一边界：Act 指令。板测操作由操作者执行；结果通过 Review 后才展开下一 Iteration。

## Planning Progress

| Phase / Step | Status | 结果 |
|---|---|---|
| CLARIFY：路线与范围 | completed | 读取已批准路线与现有行为规格，候选为轮询双向网络 |
| CLARIFY：BDD 与场景草图 | completed | S1–S11 覆盖正常、错误、边界、超时、取消与兼容性 |
| CLARIFY：用户场景决策 | completed | 用户批准 R1–R6、S1–S11 和默认假设，无豁免 |
| 创建需求提案与增量规格 | completed | 六项需求全部保留 |
| INVESTIGATE：代码、参考、经验与基线 | completed | 实际入口、分配器、DMA/cache、GMAC、PHY、smoltcp 与测试已调查 |
| PLAN：设计、RTM、Iteration Map、首个 Cycle | completed | 三个逻辑 Iteration，仅展开首轮；Evidence 为 none |
| 结构与一致性检查 | completed | OpenSpec strict 验证和计划自检；命令结果见 tasks |
| Gate 2：用户计划审计 | completed | 用户原话“批准”；Gate2 PASS，当前 Cycle ready |
| 产品实施 / 全局状态同步 | SKIPPED | 当前请求为计划，遵守公共规则阶段边界 |
