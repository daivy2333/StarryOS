# tasks.md — 任务追踪

> 任务状态最后同步: 2026-10-14 | 路线规划更新: 2026-10-06 | 分支: k3 | grep: `<!-- T{编号} -->`
> 来源: R41、R47、R49、R51、R53、M41、D22、K31-K32、K37、K41、K43、I06、I13-I19；MS01-MS11、MS16 与 MS17 已归档。

---

## 当前：K3 网卡与串口异步化

每个 milestone 只引入一个主要调试变量。执行前必须通过 OpenSpec Plan 建立 BDD、RTM、测试见证和获批 change。完成状态只依据对应 change 的新鲜证据更新。

NIC 任务按 T01→T24 的依赖推进；T25 已转为 I19，不再是执行任务。当前排期仅余 MS14（MS12、MS13 网卡基线与 MS17 串口基线均已验收），MS14 同时验收网卡与 UART 多 hart 异步行为，是当前 K3 异步化路线的最终交付。QEMU 与真板是不同证据类别。前置 Gate 未通过时，后续 milestone 保持等待。

| ID | Milestone | 交付范围 | 验证 Gate | 前置 | 状态 |
|---|---|---|---|---|---|
| <!-- T07 --> T07 | QEMU 异步 TX | 增加 TX submit、reclaim、completion 和 flush；不改 packet slot | queue full 产生背压；completion 不等于 peer delivery；flush 不永久 Pending | T06 | ✅ 完成（MS05） |
| <!-- T08 --> T08 | 有界 packet slot | 建立 RX/TX slot、occupancy、drop reason 和 partial write 契约 | 满载时内存有上界；背压可见；descriptor 不跨 await 泄漏 | T07 | ✅ 完成（MS05） |
| <!-- T09 --> T09 | stack runner | 独立推进 smoltcp ingress、egress、maintenance 和 timer | device、software、timer 唤醒可复现；空闲不轮询；持续流量不饥饿 | T08 | ✅ 完成（MS06 Iteration 000 `001-rework` accepted；Tasks 1.1–1.5 全部闭合） |
| <!-- T10 --> T10 | socket readiness | 将 smoltcp 单槽 waker 桥接到 `axpoll::PollSet` | 多 waiter、overflow、close 和 error 下，poll/select 与实际 I/O 一致 | T09 | ✅ 完成（MS06；最终 Cycle `001-replan` accepted，单 hart QEMU 手工验收全过；host/QEMU 进程证据完整性按用户明确授权豁免） |
| <!-- T11 --> T11 | reset 与取消 | 引入 generation、stale completion 丢弃、cancel、timeout 和 link flap | fault injection 下无 UAF、重复回收、永久 Pending 或静默丢包 | T10 | ✅ 完成（MS07；Iteration 000–007 全部 accepted；最终 Cycle 006 六 case + MS01/MS04/MS05/MS06 回归全过；change 已归档于 `openspec/changes/archive/2026-09-02-ms07-qemu-single-hart-recovery-semantics/`） |
| <!-- T12 --> T12 | QEMU 多 hart | 定义 queue affinity、跨 hart wake、控制面同步和 ordering 理由 | 多 hart 双向压力与 reset/I/O 交错无 race；单 hart 结果不计通过 | T11 | ✅ 完成（MS08；Iteration 000–005 共 18 个 Cycle 全部 accepted；最终 Cycle `005/003-rework` 的 `SMP=16` 六 case network-only 运行 exit 0。UART 专项、受控迁移稳定性、reset/link 交错与旧阶段回归按用户明确授权豁免） |
| <!-- T13 --> T13 | 目标板事实 Gate | 记录启动介质、DTS/ACPI、MAC、PHY、MMIO、IRQ、DMA/cache 和 CPU/hart 拓扑 | 每项来自真板、固件描述或手册；未知项阻塞后端选择 | T12；目标硬件可用 | ✅ 完成（MS09；板级事实、安全 RAM 边界与可重复临时启动已 accepted） |
| <!-- T14 --> T14 | 目标板启动与 MAC 寄存器 | 接通 feature、镜像和 early console；只验证目标 MAC 寄存器访问 | 重复启动稳定；寄存器非全零/全一；异常访问可定位 | T13 | ✅ 完成（MS09；FIT RAM 启动可重复，MAC version=0x1054 可读可解释） |
| <!-- T15 --> T15 | 目标板 Clock/Reset/PHY | 依据 T13 的 bootloader handoff 决定保留或恢复 clock/reset；只建立链路 | preserved 状态有原值；PHY/link 或等效链路结果可重复 | T14 | ✅ 完成（MS09；APMU 两步使能 + IO-PD 按 R69 用法设置，PHYID=0x001cc916 RTL8211F，插拔循环可重复） |
| <!-- T16 --> T16 | 目标板设备中断 delivery | 只接 MAC IRQ claim、handler、device status 和 EOI | IRQ claim 与设备 status 对齐；无中断风暴；CPU/hart 初始化可区分 | T15 | ✅ 完成（MS10；AIA 软件底座 + PHY 设备 IRQ 插拔对齐板上验证，change 已归档） |
| <!-- T17 --> T17 | 目标板 DMA/cache 基线 | 建立 DMA 地址转换、cache/barrier 和硬件队列 ownership | CPU/设备看到同一 descriptor/queue entry；ownership 转移有日志和断言 | T16 | ✅ 完成（MS11；checked 18 页独占 arena、线性 PA/DMA、Zicbom 发布/回收序、高位地址与 4 GiB 环检查，host harness 全绿 + 板测成立） |
| <!-- T18 --> T18 | 目标控制器轮询 RX | 只实现最小 RX refill/reap 或等效接收队列 | 抓包与硬件队列计数一致；坏帧和队列满有明确结果 | T17 | ✅ 完成（MS11；16 slot RX publish/reap/refill/wrap、分类 drop、stage11 真实 ARP RX 与 link-down 有界失败） |
| <!-- T19 --> T19 | 目标控制器轮询 TX | 只实现最小 TX submit/reclaim 或等效发送队列 | ARP/ICMP 发包可抓取；回收不重复；timeout 可诊断 | T18 | ✅ 完成（MS11；单次回收 terminal、Full 不覆盖、001-rework doorbell 排他语义修复、stage12 20/20 terminal + 真实 ARP reply） |
| <!-- T20 --> T20 | 目标后端异步 RX | 将 T06 的 RX queue task 接到真板 IRQ | RX burst 无 lost wakeup；budget、drop 和 occupancy 可观测 | T19 | ✅ 完成（MS12；k3-async 双镜像、真实 RI/TI→deferred owner、V4 run3 20 ARP TX 零丢、IoC/owner/slot 契约全过） |
| <!-- T21 --> T21 | 目标后端异步 TX | 将 T07 的 TX completion 和背压接到真板 | TX burst、queue full 和 flush 通过；无硬件队列双重所有权 | T20 | ✅ 完成（MS12；NetDriver adapter+唯一 owner+64/16 容量+C4 flush，V3 run3 板测全项；真板四协议与 quiet 见 T22 前置 MS12 收口） |
| <!-- T22 --> T22 | 真板恢复语义 | 单 hart 验证 link flap、设备 reset、generation 和 stale completion | reset 前后对象不混用；无重复回收、永久 Pending 或静默丢包 | T21 | ✅ 完成（MS13；受控 reset 双轮 committed fault=0、旧对象 connection-reset 终态、容量第 1 轮+两级边界+flush c4、source-60 拔插端到端与插回上升沿可达、插回后 new-2 真实会话交换；容量第 2 轮/fail-stop 按用户收口裁决留档） |
| <!-- T23 --> T23 | 真板网卡与 UART 多 hart | 分别验证 NIC queue/stack 与 UART copier 的 affinity、跨 hart wake、控制面同步和 ordering | 两设备双向异步 I/O 均有专项见证；网络 reset 交错无 race；每项 ordering 有角色说明 | T22；MS17 | ⏳ 等待 MS13（MS17 已于 2026-10-08 验收） |
| <!-- T24 --> T24 | 真板组合长稳压力 | 网卡双向流量与串口交互并行，覆盖各自容量/背压边界及网络 reset 交错 | 长时间运行无 stall、永久 Pending 或资源泄漏；两设备持续推进，错误与 occupancy 可解释 | T23 | ⏳ 等待 T23 |
| <!-- T25 --> T25 | 已转为 I19 | 不再作为执行任务；方向见 `.agents/memory/improvements.md` I19 | 不参与当前路线完成判定 | 不适用 | superseded — 2026-10-06 用户批准退出路线 |

<!-- arc: ARC-202609171804 --> T01-T06 已归档 (2026-09-17) → ../../openspec/changes/archive/2026-09-17-ARC-202609171804/proposal.md

共同约束：

- M36/D20：ISR、queue task、stack runner、socket readiness 分层。
- M41/D22：QEMU 以 VirtIO-MMIO 起步；transport 适配不得泄漏到异步队列语义。
- K31：串口终端与 hostfwd 分开取证。
- K32：当前 feature 合并实际选择 MMIO；PCI 不计已验证。
- K09：只采用 `embassy-sync::AtomicWaker`；不引入第二套 executor。
- K26：以 packet buffer 和 DMA descriptor 为单位，不复制 UART 字节 ring。
- M37/M38/D21 只保留为 VF2 平台知识，不应用到未确认的目标板；T13 必须重新取得 bootloader、IRQ 和 clock/reset 事实。
- M39：跨 hart ordering 按同步角色说明；QEMU 单 hart 不能作为 SMP 证据。
- R53：W1C/clear-on-read cause 保留只补充 register-recheck，不替代 descriptor/cookie completion ledger；单请求 DMA fail-stop 只作为恢复语义的安全下限，不扩张 MS05。
- I06 只在 T13-T24 的触发条件满足时评估。
- I13-I19 未承诺，不得混入当前执行路线；I17 仅在 MS08+MS07 accepted 且唯一 spawn seam 稳定后评估，I18 仅在 MS08 accepted + 至少一个其他 async 设备稳定后评估。I19 不阻塞 MS14 收口。

---

## Milestone Roadmap

本节按稳定基线组织项目阶段。现有 T01-T24 保留为 NIC 单变量执行分解，MS17 和 MS14 补齐 UART；一个
milestone 可以由一个或多个 change 完成，不预先绑定数量。

路线先完成 QEMU 异步基线，再进入目标板：

```text
QEMU:  MS01 -> MS02 -> MS03 -> MS16 -> MS04 -> MS05 -> MS06 -> MS07 -> MS08
K3:    MS08 -> MS09 -> MS10 -> MS11 -> MS12 -> MS13 -> MS17 -> MS14（当前交付终点）
       MS14 的依赖为 MS13 + MS17；MS17 的技术前置仍为 MS10
独立存储方向: MS17 -> MS18（不进入当前异步化交付排期）
未承诺优化: I19（原 MS15/T25，不作为 MS14 的前置或后继任务）
```

MS17–MS18 属 NIC 单变量分解之外的平台方向（2026-10-02 用户批准新增，依据 R74 探索）。本次把 MS17 提到 MS14 之前，保留 MS18 的独立存储规划和原编号。

2026-10-06 用户批准本次顺序和范围调整，原话：“批准当前的工作，另外调整一下tasks里面的ms规划，我们把ms14当作最后的ms，优先把12、13、17做完，然后进入ms14，这样k3板子上的多hart网卡和串口的异步化就做完了，性能优化退化到I文档，开始这个更改吧给你豁免”。批准覆盖路线写入、I19 登记和 MS12 计划批准状态同步；不豁免行为测试或真板验收，也不把历史 UART 未验收项计为完成。

<!-- arc: ARC-202609171804 --> MS01-MS04、MS16 已归档 (2026-09-17) → ../../openspec/changes/archive/2026-09-17-ARC-202609171804/proposal.md

### MS05：QEMU 有界双向设备数据面

- Status: completed — 2026-08-19；最终 Review accepted，Evidence 与兼容性偏差按用户明确授权豁免
- Outcome: RX/TX 通过有界 packet slot 形成可背压、可回收的双向设备数据面。
- Rationale: T07 的 TX completion 与 T08 的 slot/backpressure 共同形成完整的设备侧双向基线。
- Dependencies: MS04
- Scope: T07-T08；TX submit/reclaim/completion/flush、RX/TX slot、occupancy、drop 和 partial write。
- Non-goals: 独立 stack runner、socket 多 waiter、reset 和零拷贝。
- Workload: TX 状态机、有界 handoff、压力边界和完成语义。
- Stable baseline: 内存有上界，queue/slot full 可观测并能恢复，descriptor 不跨 await 泄漏。
- Verification boundary: completion 不等于 peer delivery，flush 不永久 Pending，背压与实际容量一致。
- Diagnostic boundary: 失败限制在 TX ownership、slot handoff、回收或背压传播。
- Split signals: RX 与 TX slot 策略出现无法共享的验证或生命周期边界。
- Related changes: `ms05-qemu-bounded-bidirectional-device-data-plane`（已归档于 `openspec/changes/archive/2026-08-19-ms05-qemu-bounded-bidirectional-device-data-plane/`；12 iterations，27/27 tasks，最终 Review: accepted with explicit user Evidence/compatibility waiver）。

### MS06：应用可见的异步网络栈

- Status: completed — 2026-08-27；最终 Review accepted，缺失的 host/QEMU 进程级留档按用户明确授权豁免
- Outcome: stack runner 和 socket readiness 让应用在无主动轮询依赖下使用异步网络。
- Rationale: T09 单独只有协议栈内部推进，和 T10 合并后才形成应用可依赖的阶段成果。
- Dependencies: MS05
- Scope: T09-T10；ingress/egress/maintenance/timer、device/software/timer wake 和 axpoll bridge。
- Non-goals: reset、SMP、真板 transport 和多接口扩展。
- Workload: 协议栈 runner、唤醒合流、多 waiter 语义和 socket 回归。
- Stable baseline: 空闲无轮询，持续流量不饥饿，poll/select 与实际 I/O readiness 一致。
- Verification boundary: 多 waiter、overflow、close、error 和三类 runner 唤醒均有见证。
- Diagnostic boundary: 失败限制在 stack 推进、timer/software wake 或 socket event bridge。
- Split signals: readiness bridge 需要替换 axpoll 并形成独立的多 waiter 子系统。
- Related changes: `ms06-application-visible-async-network-stack`（归档于 `openspec/changes/archive/2026-08-27-ms06-application-visible-async-network-stack/`；9 iterations，全部 tasks 完成；最终 Iteration 008 Cycle `001-replan` Review accepted。MS06 12/12、MS01 14/14、MS04 4/4、MS05 六 mode guest runtime PASS；host/QEMU 进程级输出未完整留档，用户以逐步手工全过声明接受证据完整性风险）

### MS07：QEMU 单 hart 恢复语义

- Status: completed
- Outcome: reset、分层取消、阶段化超时和 link flap 下的异步对象生命周期封闭，迟到 completion 不跨 owner epoch 生效。
- Rationale: 恢复语义必须在 SMP 放大竞态前先形成可故障注入的稳定基线；R53 的单请求 DMA fail-stop 只提供无法安全收敛时的下限，不替代 NIC 多 packet ownership 设计。
- Dependencies: MS06
- Scope: T11；区分 waiter cancellation、pre-submit 撤销和 device-owned quiesce；generation 绑定 reset epoch 与 descriptor/cookie owner ledger；submit、completion、reclaim、quiesce、reset 分阶段 timeout；link flap 和 queue stall。
- Non-goals: 跨 hart 同步、真板 DMA 停止证明、自动 polling fallback 和性能优化。
- Workload: 生命周期状态机、epoch/owner ledger、阶段化 deadline 与错误传播、quiesce/reset fail-stop、故障注入和资源保留/回收。
- Stable baseline: reset 前后对象不混用；取消等待不改变 packet ownership；无法安全 quiesce 时保持 faulted owner、拒绝新提交且不提前释放 backing。
- Verification boundary: 在 waiter、pre-submit、device-owned、各 timeout stage 和 reset failure 注入下，无 UAF、重复回收、永久 Pending、静默丢包或 stale completion 误归属；结论限定于单 hart QEMU/VirtIO 模型。
- Diagnostic boundary: 失败限制在取消层级、epoch/descriptor/cookie 归属、具体 timeout stage、quiesce 或 reset 状态转换。
- Split signals: link 管理与设备 reset 形成两个可独立验收且可独立延期的控制面成果。
- Related changes: `ms07-qemu-single-hart-recovery-semantics`（已归档于 `openspec/changes/archive/2026-09-02-ms07-qemu-single-hart-recovery-semantics/`；Iteration 000–007 全部 accepted，最终 Cycle `Dma::new` 零化重建 queue + 六 case + MS01/MS04/MS05/MS06 回归通过）

### MS08：QEMU 多 hart 正确性基线

- Status: completed — 2026-09-26；最终 Review accepted。交付结论限于 `SMP=16` 固定 placement 的六 case 网络数据面（placement、timer-disabled-wake、tcp/udp-bidirectional、full-recovery、readiness-quiet）；UART 专项 runtime、受控迁移稳定性、reset/link 交错、组合压力和旧阶段逐项回归按用户明确授权豁免，不计入本结论。
- Outcome: 异步网络在多 hart 下保持 queue ownership、跨 hart wake 和控制面同步正确。
- Rationale: SMP 是独立于单 hart 功能与恢复语义的并发故障域。
- Dependencies: MS07
- Scope: T12；queue affinity、跨 hart wake、reset/I/O 交错和 ordering 理由。
- Non-goals: multiqueue、RSS、真板 SMP 和吞吐优化。
- Workload: 并发模型、调度亲和性、原子序审计和多 hart 压力。
- Stable baseline: 多 hart 双向压力和 reset 交错不产生 race 或 ownership 冲突。
- Verification boundary: 单 hart 结果不计通过，每项 ordering 按同步角色解释。
- Diagnostic boundary: 失败限制在 CPU affinity、跨 hart 通知、共享控制面或内存序。
- Split signals: 引入 multiqueue 或 RSS，产生新的 queue-to-hart 分配成果。
- Related changes: `ms08-qemu-multi-hart-correctness-baseline`（已归档于 `openspec/changes/archive/2026-09-26-ms08-qemu-multi-hart-correctness-baseline/`；Iteration 000–005 共 18 个 Cycle，最终 Cycle `005/003-rework` Review accepted；delta spec 9 条 requirement 已合并入 `openspec/specs/qemu-multi-hart-async-io-correctness/spec.md`）

### MS09：目标板事实与可观测链路基线

- Status: completed — 2026-10-02；最终 Review accepted。交付结论限于 CoM260 Kit 的板级事实核对、安全 RAM 边界、可撤销临时 RAM 启动、MAC 只读寄存器基线（APMU 两步使能后 version=0x1054）与 PHY/link 可观测基线（IO-PD 按 R69 用法 ASAR 解锁+整字写、MDIO Clause 22、RTL8211F 身份与 ≥5 个完整插拔循环）；重复启动第三轮、部分 reset 观察与 U-Boot 控制台只读采集按用户明确授权豁免，不计入本结论。
- Outcome: 目标板可重复启动，MAC 控制器和链路状态可访问、可解释，并据此选定硬件后端。
- Rationale: T13 是 T14-T15 的调查输入；三者共同形成后端开发可依赖的平台基线。目标板尚未确认，不能预选 DWMAC 或继承 VF2 配置。
- Dependencies: MS08；目标硬件可用
- Scope: T13-T15；启动介质、DTS/ACPI、CPU/hart、feature、镜像、MAC、寄存器、clock/reset、bootloader handoff 和 PHY/链路。
- Non-goals: 设备中断 delivery、DMA 队列、网络收发和异步队列。
- Workload: 板级事实、启动链、控制器识别、寄存器观测、固件 handoff 和链路建立。
- Stable baseline: 重复启动稳定，目标 MAC 寄存器非全零/全一，PHY/link 或等效链路结果可重复，后端选择有板级依据。
- Verification boundary: 每项事实来自真板、固件描述或手册，未知项明确阻塞。
- Diagnostic boundary: 失败限制在启动链、MMIO 映射、clock/reset、PHY/链路或控制器识别。
- Split signals: 启动/MMIO 已形成可复用基线，但 PHY 因外部硬件长期独立阻塞。
- Related changes: `ms09-com260-kit-observable-link-baseline`（已归档于 `openspec/changes/archive/2026-10-02-ms09-com260-kit-observable-link-baseline/`；Iteration 000–004 共 6 个 Cycle 全部 accepted，11/11 tasks 闭合；行为规格已合并为 `openspec/specs/k3-com260-kit-observable-link-baseline/`）

### MS10：目标板可诊断设备中断基线

- Status: completed — 2026-10-03；用户审计接受。交付结论限于 CoM260 Kit 的 AIA 软件投递底座（APLIC MSI + IMSIC S-mode，`stopei` 原子 claim+EOI 先于 handler，SETIPNUM/GENMSI 双入口、双 mask、retrap、unknown、风暴预算止损）与 PHY 设备 IRQ/status 基线（pin35 按 DTB 武装 GPIO+下降沿、CLEAR hold 释放、INSR COR 前快照、真实插拔 source60/INSR bit4/link 对齐、quiet、MAC 133/277 只读观察）；本板裁决事实：EIE 掩码窗口 MSI 丢弃（掩码只作止损、须触发后置掩码）、MFPR EDGE CLEAR 复位默认置位（hold-in-clear）、每物理事件一条 INSR=0 回声 claim（分类器排除）。stage10 与 stage8 观察窗按用户指示为 30 s。重复启动仅一轮、COMBO 中断组合与非预期 MAC 事件注入不在本轮结论内。
- Outcome: 目标 MAC 中断经板级中断控制器 claim/dispatch、handler、device status 和 EOI 可重复投递；破坏性 cause 在 ack 前可保留和审计。
- Rationale: 真板中断控制器和设备触发模式是独立高风险故障域，不能由 QEMU 证据替代。
- Dependencies: MS09
- Scope: T16；目标 MAC IRQ 路由、CPU/hart 初始化、cause、ack 和 EOI；当目标寄存器事实证明 cause 为 W1C/clear-on-read 时，在 ack 前保存并按本次 IRQ 累积 cause。
- Non-goals: DMA 收发、异步 queue task 和多 hart 流量。
- Workload: 板级中断路由、寄存器 cause 语义、ack 前状态保留、组合/重复触发和风暴诊断。
- Stable baseline: IRQ claim 与设备 status 对齐；破坏性 cause 在清除后仍可追溯；EOI 后可再次触发。
- Verification boundary: 组合 cause 不因 ack 丢失，无中断风暴，CPU/hart 初始化和目标触发模式可区分；cause snapshot 不作为 descriptor completion 结论。
- Diagnostic boundary: 失败限制在板级中断控制器、cause 寄存器语义、设备触发模式或 snapshot/ack/EOI 顺序。
- Split signals: 目标控制器暴露多个必须独立验收的中断路径。
- Related changes: `ms10-com260-kit-diagnosable-device-irq-baseline`（已归档于 `openspec/changes/archive/2026-10-03-ms10-com260-kit-diagnosable-device-irq-baseline/`；Iteration 000–001 全部 accepted，Tasks 1.1–2.3 闭合；行为规格已合并为 `openspec/specs/k3-com260-kit-device-irq-baseline/`）

### MS11：目标控制器轮询双向网络基线

- Status: completed — 2026-10-06；用户审计接受（原话"工作被接受"）。交付结论限于 CoM260 Kit 的独占 18 页 DMA arena 与地址/cache 契约（checked 布局、线性 PA、Zicbom 发布/回收序、高位地址与 4 GiB 环检查、发布后 backing 保留）、16 slot 轮询 RX/TX（publish/reap/refill/wrap、单次回收、Full 不覆盖、分类 drop、阶段化 deadline/cancel/fail-stop owner 语义）、smoltcp 0.13.1 轮询 Device 与有界四协议会话（token 预留/背压、静态 socket、echo 1024B 上限、5 秒有界等待/close）与 stage13 真板验收（60 s 会话窗：ping 10 请求 10 应答、UDP/TCP 回显字节级一致、tcp-close=Closed、零 drop、16/16 terminal）；本板裁决事实：TX tail 为排他 fetch 上界（doorbell 写 next slot）、RX tail 写为唤醒 doorbell（fetch 由 OWN 驱动）。ping 形态（5+5 手动 Ctrl-C，契约文本 `ping -n 20`）与 reset 描述按用户原话豁免；Wireshark 按用户原话永久豁免。
- Outcome: 在明确 DMA/cache ownership 的前提下完成目标控制器轮询 RX/TX 和 ARP/ICMP/UDP/TCP 协议包收发。
- Rationale: T17 只有通过 T18-T19 的 descriptor 移动和真实包才能验证其 DMA/cache 契约。
- Dependencies: MS10
- Scope: T17-T19；DMA 地址转换、cache/barrier、descriptor/queue ownership、最小 RX/TX、抓包，以及 submit/doorbell、DMA terminal、reclaim 的分阶段诊断。
- Non-goals: 异步 wake、reset、SMP、offload 和零拷贝优化。
- Workload: DMA 抽象、目标硬件队列、轮询收发、阶段化 timeout、controller/descriptor/buffer 终态验证、错误路径和协议验证；DWMAC 代码仅在控制器兼容时进入审计和移植候选。
- Stable baseline: CPU 与设备观察同一硬件队列状态；完成判定同时闭合 controller、descriptor 和 buffer ownership；ARP/ICMP/UDP/TCP 与抓包一致。
- Verification boundary: RX/TX 回收不重复，坏帧、ring full 和 timeout 均标明失败阶段与 owner；不照搬 SDMMC timeout 数值或阶段。
- Diagnostic boundary: 失败限制在 DMA 地址、cache/barrier、目标硬件队列、具体 submit/terminal/reclaim stage 或轮询数据面。
- Split signals: RX 或 TX 暴露独立硬件阻塞，且另一方向已形成可复用稳定基线。
- Related changes: `ms11-com260-kit-polling-bidirectional-network-baseline`（已归档于 `openspec/changes/archive/2026-10-06-ms11-com260-kit-polling-bidirectional-network-baseline/`；Iteration 000–002 全部 accepted，Tasks 1.1–3.2 闭合；行为规格已合并为 `openspec/specs/k3-com260-kit-polling-network-baseline/`）

### MS12：目标后端异步双向数据面

- Status: completed — 2026-10-13；用户审计接受（原话"工作被接受"）。交付结论限于 CoM260 Kit 单 hart：k3-async/k3-async-probe 独立入口（SMP>1 与 shell 组合拒绝）、GMAC source-133 IOC 底座（arm/recheck/deferred owner，V4 run3 20 ARP TX 真实 RI/TI 零丢）、K3NetAdapter 接入既有 NetDriverOps/queue/stack 契约（64 slot/16 desc 账本、C4 flush、Pending→release→C4，V3 run3 板测全项）、stack runner+socket 载荷（UDP 26001/TCP 26002 echo、主动关闭 EOF）与真板四协议（两轮 ping 10/10、UDP/TCP 全字节、quiet 经沉降相 2 s 逐字节冻结直证、resume 行按用户裁决豁免、SETTLE 修复在案）；host stack_socket 15/15、axnet 三面 482/506/514 采信未变。结论不含真板 reset/link-flap 恢复、DMA quiesce 证明、多 hart 与性能（MS13/MS14/I19 范围）。
- Outcome: 已验证的 QEMU queue/stack 契约适配到目标控制器异步 RX/TX。
- Rationale: T20-T21 共享同一真板 IRQ、DMA 和队列适配边界，合并后形成双向 transport parity。
- Dependencies: MS11
- Scope: T20-T21；目标后端 RX/TX completion、budget、slot、backpressure 和 flush。
- Non-goals: 真板 reset、多 hart、长稳压力和数据驱动优化。
- Workload: transport 适配、真板队列服务、双向压力和 QEMU 契约回归。
- Stable baseline: 真板双向异步收发无 lost wakeup、descriptor 双重所有权或永久 Pending。
- Verification boundary: RX/TX burst、queue full、drop、occupancy 和 flush 均可观测。
- Diagnostic boundary: 失败限制在目标 transport 适配、真板 IRQ/DMA 或既有异步契约回归。
- Split signals: RX 与 TX 依赖不同硬件能力，且任一方向可独立成为后续稳定前置。
- Related changes: `ms12-com260-kit-async-bidirectional-data-plane`（已归档于 `openspec/changes/archive/2026-10-13-ms12-com260-kit-async-bidirectional-data-plane/`；Iteration 000/001/002 全部 accepted，T1–T7 闭合；行为规格已合并为 `openspec/specs/k3-com260-kit-async-network-data-plane/`）

### MS13：目标板单 CPU/hart 恢复语义

- Status: completed — 2026-10-14；用户审计接受（原话"工作被接收"）。交付结论限于 CoM260 Kit 单 hart：k3-async/k3-async-probe 独立入口下的受控恢复矩阵（18 页 arena 原地 stop/SWR/重建、QueueEpoch/cookie 隔离、resident owner、socket terminal、容量第 1 轮两级边界、C4 flush、真实 source-60 拔插事件含插回上升沿可达（INSR/INTN ack：arm 清 pending + owner link 读消费 latch）、插回后 new-2 真实逐字节 UDP/TCP 会话交换）；host 面生产 adapter/Service/socket 契约全绿（host 全包含 recovery_socket、axnet 三面 486/516/519、harness 37/37+32/32）。残余按用户收口原话"本轮change就做这么多"留档不追：容量/C4 第 2 轮、fail-stop 末场景、矩阵形态 new-2 判定行、插回后 link 不稳定三候选（事件通路单次失手/末次 Down 门永久关/DMA 静默停摆，有 trace 证据未定位）。结论不含多 hart（MS14）、长稳与性能（I19）。
- Outcome: 真板 link flap 和设备 reset 下保持 epoch、DMA quiesce 与资源回收正确；无法确认 DMA 停止时不释放 backing。
- Rationale: 先将 QEMU 恢复契约迁移到真板并取得物理 DMA/cache 证据，再引入多 hart 和长稳压力。
- Dependencies: MS12
- Scope: T22；link flap、设备 reset、stale completion、等待者错误传播、bus mastering/DMA 停止证明，以及 reset failure 下的 backing retention。
- Non-goals: 多 hart、长时间 soak 和性能优化。
- Workload: 真板故障注入、设备控制面、DMA quiesce/stop 见证、reset epoch、stale completion 丢弃、backing 保留和生命周期证据。
- Stable baseline: reset 前后对象不混用；确认 DMA 停止后才释放或复用资源；无法确认时保持 faulted owner、保留 backing 并拒绝新提交。
- Verification boundary: 真板 fault injection 下无重复回收、永久 Pending、静默丢包、stale completion 误归属或 reset 后 DMA 越界；单纯寄存器读回和 QEMU reset 不计 DMA 停止证明。
- Diagnostic boundary: 失败限制在真板 link 状态、reset epoch、DMA quiesce/stop、backing retention 或 owner 重建。
- Split signals: link 恢复与完整设备 reset 出现不可共享的生命周期和验证边界。
- Related changes: `ms13-com260-kit-single-hart-network-recovery`（已归档于 `openspec/changes/archive/2026-10-14-ms13-com260-kit-single-hart-network-recovery/`；Iteration 000/001 accepted、Iteration 002 经 001-rework accepted（A3-R 折价收口：残余豁免与"不得表述为已板上验证"边界记录在该 Review）；行为规格已合并为 `openspec/specs/k3-com260-kit-single-hart-network-recovery/` 并更新 `k3-com260-kit-async-network-data-plane/`）

### MS17：K3 单 hart 异步串口与交互 shell 基线

- Status: completed — 2026-10-08；用户审计接受（原话"工作被接受"）。交付结论限于 CoM260 Kit 单 hart：runtime timer/AIA 底座（独立 timer handler 槽、set_timer(0) 首 tick、与 UART42 隔离）、PXA UART 异步 port（115200 8N1、三步 FCR PXAR8、Errata20 序、唯一 owner 激活）、完整 copier 数据面（64KiB rings、check-register-arm-recheck、wrap/Full/drop 恢复、四条件物理 drain 与 1 ms timer hook）、tty canonical 输入契约（空格/tab、ERASE/KILL、1024B 行界整行 discard、EOF 0-read、readiness）、内存根内嵌用户态 stdio（ET_EXEC 静态 ELF、失败分层未发布、argv/EAGAIN/ppoll/nanosleep/exit 板测）与最小交互 shell（help/echo/sleep/exit、异常输入恢复、B7 两轮板测全场景 + SIGINT 对照 + R68 复位）；native：crate 74/74、tty 11/11 + write 6、shell 20/20、embedded-user-host 9/9；受影响构建门 k3-stdio/k3/build/host-test 全过，`make k3-shell` 为交付镜像形态。结论不含 GMAC 数据面（MS12/MS13）、多 hart（MS14）、存储/rootfs（MS18）与性能优化（I19）。
- Outcome: CoM260 Kit 上 StarryOS 具备首个可交互用户面：串口异步 RX/TX 接通，console 可输入命令并得到输出，timer 超时/调度唤醒在 K3 生效，R68 RAM 启动环内可复现。
- Rationale: 交互性是与 NIC 数据面正交的独立故障域（IRQ 扩展、UART RX、tty/用户态）；R74 探索证明最小路径零存储依赖（用户程序内嵌内核镜像），PXA UART/AIA 机制按 M42 从 tgoskits 默认采信，异步 UART 契约沿用本仓 QEMU/D1 已验证实现。
- Dependencies: MS10；排期在 MS12、MS13 完成之后，优先复用 MS12 已接通的 timer/task 底座，不新增对网络恢复语义的技术依赖
- Scope: K3 `IrqIf` 实化到 timer + UART RX（复用 MS10 的 AIA claim/dispatch 底座）；K3 PXA UART 硬件层适配接入现有 `UartPort`/RX-TX copier/waker/poll 契约，包含双向异步 I/O、背压与 flush/drain 等待语义；tty console 输入路径；用户程序内嵌载体与 spawn 在 K3 接通（syscall/VFS/进程加载沿用 QEMU 已验证实现）。
- Non-goals: 存储驱动与 rootfs（MS18）、GMAC 数据面（MS11–MS13）、多 hart（MS14）、网络投递、持久部署、第二 UART。
- Stable baseline: 单 hart 串口双向异步 I/O 与交互稳定可重复（命令→输出、背压解除后恢复、flush/drain 和 sleep/超时生效），既有 QEMU/D1/VF2 构建与行为不回归；后续 K3 开发获得交互观察面。
- Verification boundary: 真板 UART RX/TX、等待唤醒与交互直接观察 + host 生产路径测试；普通启动日志不作为异步 UART 验收；单 hart 证据（SMP 属 MS14）。
- Diagnostic boundary: 失败限于 AIA 底座扩展、UART RX/TX 驱动、tty/用户态 spawn 或内嵌程序载体。
- Split signals: 存储提前进入范围；或 UART 交互与用户态进程面各自独立受阻。
- Related changes: `ms17-com260-kit-async-uart-interactive-shell`（已归档于 `openspec/changes/archive/2026-10-08-ms17-com260-kit-async-uart-interactive-shell/`；Iteration 000–003 全部 accepted，T1–T8 闭合；行为规格已合并为 `openspec/specs/k3-com260-kit-async-uart-interactive-shell/`）

### MS14：K3 多 hart 网卡与串口异步化最终验收

- Status: planned
- Outcome: CoM260 Kit 上网卡和 UART 均在多 hart 下完成双向异步 I/O、跨 hart 唤醒与组合长稳验收；成为当前 K3 异步化路线的最终交付。
- Rationale: MS12/MS13 提供网卡功能与恢复基线，MS17 提供串口单 hart 基线；本阶段只推进这些既有能力的多 hart 正确性和共同运行稳定性，不重新承担设备 bring-up。
- Dependencies: MS13、MS17（含传递前置 MS12）；两条单 hart 基线均验收后才进入。
- Scope: T23-T24；NIC queue/stack 与 UART RX/TX copier 的 affinity、跨 hart wake、共享状态同步与 ordering；分别覆盖两设备双向 I/O、容量/背压、等待取消/超时和 flush/drain；网络 reset/link 恢复交错；网卡流量与串口交互共同运行的长稳压力。
- Non-goals: 新设备 bring-up、存储/rootfs（独立 MS18）、新增 UART 硬件 reset 能力、受控任务迁移、第二 UART，以及性能资格/优化（I19：batching、moderation、offload、zero-copy、multiqueue 等）。
- Workload: 先分别完成网卡与 UART 的多 hart 适配和专项验证，再验证组合压力及长稳；复用既有 executor/copier/queue/stack，不引入通用驱动框架。
- Stable baseline: 网卡与 UART 均有 K3 多 hart 双向异步运行结果；跨 hart wake 不丢失，各自资源/owner 账本闭合；网络恢复交错和组合长稳无 stall、永久 Pending、重复回收或资源泄漏。
- Verification boundary: 网卡、UART 专项和组合运行分别观察目标行为；普通串口日志、QEMU SMP 或单 hart 结果均不代替 K3 UART 多 hart 验收。保留错误、drop、occupancy、IRQ/wake 等正确性观测；不以吞吐/p99/CPU 收益或完整性能矩阵作为完成门槛。
- Diagnostic boundary: 分别定位 NIC queue/stack、UART copier/tty、平台跨 hart IRQ/IPI/调度和组合恢复边界，不以网络通过推定 UART 通过。
- Balance audit: 网卡与 UART 各自具有独立故障域，专项验收必须保留；用户明确指定一个最终集成阶段，允许通过多个 change/Iteration 分开诊断，再共同验收，不新增后继 milestone。
- Split signals: 任一设备多 hart 适配或组合长稳长期独立受阻时，在本阶段内拆分 change/Iteration 并记录未完成项；不因另一设备通过就收口，也不自动扩展新 milestone。
- Related changes: None

### MS15：性能优化已转为改进方向

- Status: superseded — 2026-10-06 用户要求退出执行路线，转为 I19
- Replacement: `.agents/memory/improvements.md` I19；未承诺实施，不是 MS14 的前置或后继阶段。
- Related changes: None

### MS18：K3 存储与 rootfs 用户态基线

- Status: planned — 2026-10-08 RAM rootfs 普通 shell 基线已交付归档：单 hart 临时 RAM 启动、平台预留窗保护（D1-r1：rootfs 窗收档 256 MiB + DTB 窗上移 0x2ef000000）、ext4 挂载、动态 BusyBox `/bin/sh`（musl 解释器）与外部命令、k3-rootfs 隔离模式（互斥/SMP 拒绝、独立 FIT）、官方启动保持（RESET 回官方 Linux 用户实测）；真实块设备存储、分区与持久挂载仍按本条原 planned 范围保留后续，不因该基线标记完成。
- Scheduling: 保留既有独立存储规划；不列入当前 MS12→MS13→MS17→MS14 的异步化交付排期，不阻塞 MS14，也不作为其自动后继阶段。
- Outcome: K3 上从块设备挂载 rootfs 并运行完整用户态（shell 与用户程序从文件系统加载），形成可复现的存储与文件系统基线。
- Rationale: rootfs 路线（R74 增强方向）复用 Linux 生态与本仓 QEMU rootfs 经验（同源发行）；存储是独立故障域（块设备、分区、文件系统、DMA/cache 一致性），必须独立验收，不与交互性混同。
- Dependencies: MS17
- Scope: K3 块设备驱动（UFS 或 SD/eMMC 按当前板介质事实从 tgoskits 参考适配其一）；分区识别与挂载接入现有 axfs-ng/VFS；rootfs 镜像来源与写入边界裁决（外置介质优先，挂载出厂分区读写为独立风险决策）；用户程序从 rootfs 加载。
- Non-goals: 持久刷写固件分区（独立高危决策）、GMAC 数据面、多 hart、性能优化、双介质并行支持。
- Stable baseline: rootfs 挂载后 shell 与用户程序从文件系统加载运行，介质事实与挂载来源可解释、可重复。
- Verification boundary: 块设备、分区、挂载、程序加载逐层有直接观察；介质/分区事实来自当前板，不推定 R70 布局。
- Diagnostic boundary: 失败限于块设备驱动、分区/挂载、文件系统或程序加载路径。
- Split signals: UFS 与 SD/eMMC 需要不同驱动且其一已稳定；或挂载与程序加载各自独立受阻。
- Related changes: `ms18-com260-kit-standard-shell-rootfs`（已归档于 `openspec/changes/archive/2026-10-08-ms18-com260-kit-standard-shell-rootfs/`；Iteration 000–001 accepted，002 shell 行为与恢复按用户 2026-10-08 裁决取消，S3/S5/S7/S8 后半未取得正式见证；行为规格已按见证状态合并为 `openspec/specs/k3-com260-kit-standard-shell-rootfs/`）

Roadmap 共同 Non-goals：

- I13 的 PCI 兼容性不进入 MMIO 主线。
- I14-I15 只有在自动化 Gate 明确触发并获批后才进入新的 milestone。
- 不引入 Embassy executor、完整 embassy-net 或用户态 mmap RX/TX ring。
- QEMU 证据不替代目标板 DMA、cache、PHY/链路、IRQ、SMP 或性能证据。
- Milestone 不替代后续 Plan 的 BDD、RTM、Task Contract 和测试设计。

---

## UART 文档已归档

UART 文档已归档；q17 multi-hart SMP 验证 deferred（task 6.1 未完成）。完整任务见 `uart-lichee` 分支。归档载体见 `openspec/changes/archive/2026-07-25-cleanup-uart-docs/`。

## 活跃 Change

当前无活跃 change（MS13 已于 2026-10-14 完成并归档）。

当前交付终点为 MS14：MS12 网卡单 hart 异步双向数据面、MS13 真板恢复语义与 MS17 串口单 hart 基线均已完成，MS14（真板网卡与 UART 多 hart + 组合长稳）为剩余唯一 milestone。MS11、MS12、MS13、MS17、MS18 详情保留在各自条目。
