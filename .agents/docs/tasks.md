# tasks.md — 任务追踪

> 任务状态最后同步: 2026-09-26 | 路线规划更新: 2026-08-14 | 分支: mul-hart-k3 | grep: `<!-- T{编号} -->`
> 来源: R41、R47、R49、R51、R53、M41、D22、K31-K32、K37、K41、K43、I06、I13-I18；MS01-MS06 与 MS16 已归档。

---

## 当前：异步 NIC 开发

每个 milestone 只引入一个主要调试变量。执行前必须通过 OpenSpec Plan 建立 BDD、RTM、测试见证和获批 change。完成状态只依据对应 change 的新鲜证据更新。

执行顺序固定为 T01→T25。QEMU 与真板是不同证据类别。前置 Gate 未通过时，后续 milestone 保持等待。

| ID | Milestone | 交付范围 | 验证 Gate | 前置 | 状态 |
|---|---|---|---|---|---|
| <!-- T07 --> T07 | QEMU 异步 TX | 增加 TX submit、reclaim、completion 和 flush；不改 packet slot | queue full 产生背压；completion 不等于 peer delivery；flush 不永久 Pending | T06 | ✅ 完成（MS05） |
| <!-- T08 --> T08 | 有界 packet slot | 建立 RX/TX slot、occupancy、drop reason 和 partial write 契约 | 满载时内存有上界；背压可见；descriptor 不跨 await 泄漏 | T07 | ✅ 完成（MS05） |
| <!-- T09 --> T09 | stack runner | 独立推进 smoltcp ingress、egress、maintenance 和 timer | device、software、timer 唤醒可复现；空闲不轮询；持续流量不饥饿 | T08 | ✅ 完成（MS06 Iteration 000 `001-rework` accepted；Tasks 1.1–1.5 全部闭合） |
| <!-- T10 --> T10 | socket readiness | 将 smoltcp 单槽 waker 桥接到 `axpoll::PollSet` | 多 waiter、overflow、close 和 error 下，poll/select 与实际 I/O 一致 | T09 | ✅ 完成（MS06；最终 Cycle `001-replan` accepted，单 hart QEMU 手工验收全过；host/QEMU 进程证据完整性按用户明确授权豁免） |
| <!-- T11 --> T11 | reset 与取消 | 引入 generation、stale completion 丢弃、cancel、timeout 和 link flap | fault injection 下无 UAF、重复回收、永久 Pending 或静默丢包 | T10 | ✅ 完成（MS07；Iteration 000–007 全部 accepted；最终 Cycle 006 六 case + MS01/MS04/MS05/MS06 回归全过；change 已归档于 `openspec/changes/archive/2026-09-02-ms07-qemu-single-hart-recovery-semantics/`） |
| <!-- T12 --> T12 | QEMU 多 hart | 定义 queue affinity、跨 hart wake、控制面同步和 ordering 理由 | 多 hart 双向压力与 reset/I/O 交错无 race；单 hart 结果不计通过 | T11 | ✅ 完成（MS08；Iteration 000–005 共 18 个 Cycle 全部 accepted；最终 Cycle `005/003-rework` 的 `SMP=16` 六 case network-only 运行 exit 0。UART 专项、受控迁移稳定性、reset/link 交错与旧阶段回归按用户明确授权豁免） |
| <!-- T13 --> T13 | 目标板事实 Gate | 记录启动介质、DTS/ACPI、MAC、PHY、MMIO、IRQ、DMA/cache 和 CPU/hart 拓扑 | 每项来自真板、固件描述或手册；未知项阻塞后端选择 | T12；目标硬件可用 | ⏳ 等待目标硬件（T12 已完成） |
| <!-- T14 --> T14 | 目标板启动与 MAC 寄存器 | 接通 feature、镜像和 early console；只验证目标 MAC 寄存器访问 | 重复启动稳定；寄存器非全零/全一；异常访问可定位 | T13 | ⏳ 等待 T13 |
| <!-- T15 --> T15 | 目标板 Clock/Reset/PHY | 依据 T13 的 bootloader handoff 决定保留或恢复 clock/reset；只建立链路 | preserved 状态有原值；PHY/link 或等效链路结果可重复 | T14 | ⏳ 等待 T14 |
| <!-- T16 --> T16 | 目标板设备中断 delivery | 只接 MAC IRQ claim、handler、device status 和 EOI | IRQ claim 与设备 status 对齐；无中断风暴；CPU/hart 初始化可区分 | T15 | ⏳ 等待 T15 |
| <!-- T17 --> T17 | 目标板 DMA/cache 基线 | 建立 DMA 地址转换、cache/barrier 和硬件队列 ownership | CPU/设备看到同一 descriptor/queue entry；ownership 转移有日志和断言 | T16 | ⏳ 等待 T16 |
| <!-- T18 --> T18 | 目标控制器轮询 RX | 只实现最小 RX refill/reap 或等效接收队列 | 抓包与硬件队列计数一致；坏帧和队列满有明确结果 | T17 | ⏳ 等待 T17 |
| <!-- T19 --> T19 | 目标控制器轮询 TX | 只实现最小 TX submit/reclaim 或等效发送队列 | ARP/ICMP 发包可抓取；回收不重复；timeout 可诊断 | T18 | ⏳ 等待 T18 |
| <!-- T20 --> T20 | 目标后端异步 RX | 将 T06 的 RX queue task 接到真板 IRQ | RX burst 无 lost wakeup；budget、drop 和 occupancy 可观测 | T19 | ⏳ 等待 T19 |
| <!-- T21 --> T21 | 目标后端异步 TX | 将 T07 的 TX completion 和背压接到真板 | TX burst、queue full 和 flush 通过；无硬件队列双重所有权 | T20 | ⏳ 等待 T20 |
| <!-- T22 --> T22 | 真板恢复语义 | 单 hart 验证 link flap、设备 reset、generation 和 stale completion | reset 前后对象不混用；无重复回收、永久 Pending 或静默丢包 | T21 | ⏳ 等待 T21 |
| <!-- T23 --> T23 | 真板多 hart | 验证 queue affinity、跨 hart wake、控制面同步和 ordering | 双向流量与 reset 交错无 race；每项 ordering 有角色说明 | T22 | ⏳ 等待 T22 |
| <!-- T24 --> T24 | 真板长稳压力 | 组合 burst、双向和 ring full；只评估稳定性与指标 | 长时间运行无 stall；drop、p99、occupancy 和环境可复现 | T23 | ⏳ 等待 T23 |
| <!-- T25 --> T25 | 数据驱动优化 | 按测量逐项评估 batch、moderation、offload、zero-copy 和 multiqueue | 每项独立 A/B；正确性 Gate 不退化；无数据则记录 SKIPPED | T24；指标触发 | ⏳ 等待 T24 |

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
- I13-I18 未承诺，不得混入 T01-T25；I17 仅在 MS08+MS07 accepted 且唯一 spawn seam 稳定后评估，I18 仅在 MS08 accepted + 至少一个其他 async 设备稳定后评估。

---

## Milestone Roadmap

本节按稳定基线组织项目阶段。现有 T01-T25 保留为单变量执行分解；一个
milestone 可以由一个或多个 change 完成，不预先绑定数量。

路线先完成 QEMU 异步基线，再进入目标板：

```text
QEMU:  MS01 -> MS02 -> MS03 -> MS16 -> MS04 -> MS05 -> MS06 -> MS07 -> MS08
BOARD: MS08 -> MS09 -> MS10 -> MS11 -> MS12 -> MS13 -> MS14 -> MS15 (指标触发)
```

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

- Status: planned
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
- Related changes: None

### MS10：目标板可诊断设备中断基线

- Status: planned
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
- Related changes: None

### MS11：目标控制器轮询双向网络基线

- Status: planned
- Outcome: 在明确 DMA/cache ownership 的前提下完成目标控制器轮询 RX/TX 和协议包收发。
- Rationale: T17 只有通过 T18-T19 的 descriptor 移动和真实包才能验证其 DMA/cache 契约。
- Dependencies: MS10
- Scope: T17-T19；DMA 地址转换、cache/barrier、descriptor/queue ownership、最小 RX/TX、抓包，以及 submit/doorbell、DMA terminal、reclaim 的分阶段诊断。
- Non-goals: 异步 wake、reset、SMP、offload 和零拷贝优化。
- Workload: DMA 抽象、目标硬件队列、轮询收发、阶段化 timeout、controller/descriptor/buffer 终态验证、错误路径和协议验证；DWMAC 代码仅在控制器兼容时进入审计和移植候选。
- Stable baseline: CPU 与设备观察同一硬件队列状态；完成判定同时闭合 controller、descriptor 和 buffer ownership；ARP/ICMP/UDP/TCP 与抓包一致。
- Verification boundary: RX/TX 回收不重复，坏帧、ring full 和 timeout 均标明失败阶段与 owner；不照搬 SDMMC timeout 数值或阶段。
- Diagnostic boundary: 失败限制在 DMA 地址、cache/barrier、目标硬件队列、具体 submit/terminal/reclaim stage 或轮询数据面。
- Split signals: RX 或 TX 暴露独立硬件阻塞，且另一方向已形成可复用稳定基线。
- Related changes: None

### MS12：目标后端异步双向数据面

- Status: planned
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
- Related changes: None

### MS13：目标板单 CPU/hart 恢复语义

- Status: planned
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
- Related changes: None

### MS14：目标板多 CPU/hart 稳定性与性能基线

- Status: planned
- Outcome: 真板多 hart 异步网络在组合压力和长时间运行下形成可复现的稳定性与性能基线。
- Rationale: T24 只提供 T23 的完成证据和后续优化输入，不单独形成产品能力。
- Dependencies: MS13
- Scope: T23-T24；queue affinity、跨 hart wake、双向压力、ring full、reset 交错和 soak。
- Non-goals: batching、moderation、offload、zero-copy 和 multiqueue。
- Workload: 真板并发、长稳测试、指标采集、环境固定和证据整理。
- Stable baseline: 长时间运行无 stall，drop、p99、occupancy、IRQ 和 CPU 指标可复现。
- Verification boundary: 真板多 hart 证据独立保存，每项 ordering 有角色说明。
- Diagnostic boundary: 失败限制在跨 hart 同步、真板调度、恢复交错或稳定性退化。
- Split signals: SMP 正确性与 soak 环境分别需要长期独立交付，且前者已能成为后续稳定基线。
- Related changes: None

### MS15：首个数据驱动优化闭环

- Status: planned
- Outcome: 关闭 MS14 数据确认的一个主要瓶颈，并建立不退化正确性的 A/B 基线。
- Rationale: T25 包含多个独立故障域；每个 milestone 只接纳一个有数据支持的优化方向。
- Dependencies: MS14；指标达到明确触发条件
- Scope: T25 中首个被数据触发的内聚候选，例如 batch、moderation、offload、zero-copy 或 multiqueue。
- Non-goals: 同时打包多个独立优化；无数据时预先实现候选能力。
- Workload: 瓶颈归因、单项实现、A/B 测量、正确性回归和收益记录。
- Stable baseline: 一个优化方向有可复现收益，且 MS14 正确性 Gate 不退化。
- Verification boundary: 同环境独立 A/B；无触发数据时记录 SKIPPED，不实施。
- Diagnostic boundary: 失败限制在被选中的单项优化及其直接交互面。
- Split signals: 两个或更多独立瓶颈同时达到触发条件；为后续候选创建 MS16+。
- Related changes: None

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

无活跃 change。`ms08-qemu-multi-hart-correctness-baseline`（QEMU 多 hart 正确性基线，覆盖 T12）已于 2026-09-26 正常完成并归档至 `openspec/changes/archive/2026-09-26-ms08-qemu-multi-hart-correctness-baseline/`；Iteration 000–005 共 18 个 Cycle 全部 accepted，Tasks 1.1–7.3 全部闭合（7.1–7.3 及 6.1 为用户明确豁免的 SKIPPED）。行为规格已合并为 `openspec/specs/qemu-multi-hart-async-io-correctness/`。后续里程碑为 MS09（目标板事实与可观测链路基线，等待 T12 前置即已完成，且需目标硬件可用）。
