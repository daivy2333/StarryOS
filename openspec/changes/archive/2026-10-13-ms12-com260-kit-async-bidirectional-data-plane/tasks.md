## Plan status

原R1–R7/S1–S20已于2026-10-06获批；本次重新扫描Happy/Sad/Edge、错误、取消、deadline与兼容场景，范围不裁剪，Gate1沿用PASS。2026-10-08用户要求复核串口/shell交付后的复用，并明确禁止多余身份验收；调查和技术规划已完成，用户原话“批准”，修订计划获批，Gate2 PASS。

Iteration 000 与 Iteration 001 均已完成（2026-10-12 各自用户审计 PASS + Plan Review accepted）：T1–T5 全部闭合，含 Iteration 000 V4 run3（20 ARP TX、真实 RI/TI、reset 确认）与 Iteration 001 V3 run3 真板容量/flush 全项直证达标（软件 64→65 Full、driver 16→17 Full、release 后账本复原、同 waiter flush Pending→release→C4、auto-release=0、外部 RESET 回原系统确认）。详见 [001-replan](iterations/000-initial/001-replan.md) 与 [001 Cycle](iterations/001-queue-owner-capacity-flush/000-initial.md)。

当前逻辑Iteration为002（Map 最后一轮，T6–T7），最新Cycle为[000-initial](iterations/002-stack-socket-protocol/000-initial.md)，2026-10-12 用户原话"批准"，Gate2 PASS，Status=ready。2026-10-12 Act 完成：T6（host fixture 15/15 GREEN + 内核 stack start/echo 载荷 + 双镜像 exit 0）与 T7（V3 两轮板测：A2 全项、quiet 冻结直证、resume 行按用户裁决豁免）全部闭合，Act Response Status=reported。2026-10-13 用户指令"开始审计"，Plan Review accepted：MS12 R1–R7 全闭合、三个 Iteration 全部 accepted，change 收尾交 Maintainer（含 Finding：improvements.md 工作树内 I20 登记未在 Cycle 声明面，收尾时确认授权）。

采用当前已有shell的同一仓库底座，保持独立网络板测入口。网卡单hart稳定后，后续组合阶段先把UART、NIC与shell接入同一启动入口检查共同运行，再多hart；本变更不增加shell命令、rootfs、用户态网络工具或新的里程碑。合并镜像不能让两个启动分支各自再次初始化scheduler/handler，也不能启动第二网络owner。

## 1. Runtime 与真实 DMA IRQ（Iteration 000）

- [x] 1.1 **T1**：复用已验收的runtime timer/AIA/critical-section，新增隔离的`k3-async-net`/`make k3-async`入口，提供非paging dyn依赖所需的K3网络限定mem_iomap Unsupported ABI，修复普通axnet诊断调用cfg。验证ordinary event RED→GREEN、网络build/FIT、单hart与网络/UART模式拒绝、旧K3与shell兼容；不重写timer或新增重复timer harness。（2026-10-12 Act：V1普通event exit0；make k3-async exit0含FIT；SMP=2与网络+shell组合明确拒绝；make k3/k3-shell/QEMU build exit0；timer 8项与真实sleep按accepted结论采信。）
- [x] 1.2 **T2**：在 `k3_gmac/{mod,regs,desc}.rs` 与K3 IRQ/网络入口实现设备侧IOC、最小ISR、fatal latch、mask/arm/descriptor重查和deferred raw会话。验证production路径fake MMIO/cache竞态及MS11回归；构建后由操作者RAM启动，真实20ARP TX完成、RI/TI重复cause/133claim/deferred唤醒、最后完成可回收；外部reset回原系统。未提交板测结果时本task保持未完成。（2026-10-12 Act + 操作者板测：harness 31/31（含 bit10 伴随位裁决用例）；MS11 回归全过；V4 run3 `sent=20 terminals=20 rx-frames=19 irq-runs=39 ri=19 ti=20 timer-wakes=19 deferred-polls=20 spurious=0 backing=retained`，TX/RX 跨 16 wrap、最后帧无追加流量仍 terminal；操作者确认外部 RESET 已执行回原系统。经历 3 轮板测迭代：run1 暴露 arm 惰性/等待次序/RX 服务位次缺陷（Act 契约内修复）、run2 暴露 bit10 误分类（按 R69 参考裁决登记为已知良性位），详见 001-replan Act Response Resolution 1–3。）

## 2. 既有 queue 契约、容量与 C4 flush（Iteration 001）

- [x] 2.1 **T3**：新增 `k3_gmac/adapter.rs` 实现既有 `NetDriverOps/NetQueueControl/NetTxQueue`；原生固定NetBufPool与16项cookie owner账本，恢复control为None。生产模块host见证覆盖长度、14/60/1514/1515、pool/descriptor16/17、wrap、单次回收、postpublish error、timeout与backing保留；原生Cargo GREEN。（2026-10-12 Act：k3-async-net-host adapter_contract 12/12 RED→GREEN；gmac harness 31/31、polling-net-host 17/17 回归全过。`TerminalTimeout`→`Io` 映射偏差见 Act Response Deviations。）
- [x] 2.2 **T4**：在 `platform/k3_async_net.rs` 接入 `AxDeviceContainer::from_one`、现有Service、唯一queue任务；旧polling session不并行运行。使用production adapter+axnet local Service fixture证明激活失败/重复、各stage31/32/33、RX64/65、TX Full后恢复、通知窗口、稳定fault；异步构建成功，保留32budget与64slots。（2026-10-12 Act：host service_owner 场景 1/1（重复拒绝、Active/owner=1、通知窗口、RX 64/65 背压、稳定 Faulted）；flush_contract 1/1；`make k3-async` exit0（FIT 468512B）；axnet --lib 482 全过。TX Full→恢复的 Service 层见证由 T3 adapter 16/17 wrap→reclaim→恢复 + axnet queue-diagnostics 面 506 既有 submit-Full 恢复测试采信；socket 层端到端属 Iteration 002。queue owner round 不含 Router/dispatch（stack runner 职责）为调查补充，与 Map 分轮一致。）
- [x] 2.3 **T5**：把axnet已有submit/reclaim lease隔离到 `queue-diagnostics`，保留QEMU恢复专用入口；添加最小完整帧诊断enqueue和K3载荷，复用现有flush。验证ordinary/queue-diagnostics/qemu-diagnostics三面及flush target/Busy/cancel/timeout；真板200ms hold下TX64/65与driver16/17、flush Pending→release→C4成功、owner容量复原、auto-release=0。不得添加判定器。（2026-10-12 Act agent 侧全过：三面 482/506/514；flush_contract 1/1（target set/new ticket/后续 ticket/waiter drop/Pending→reclaim→C4/FBE 唤醒）；Busy/cancel 由 queue 面既有 flush 测试保持 GREEN 采信；`make k3-async-probe` exit0；受影响构建 make k3/k3-shell/SMP=1 build exit0；git diff --check exit0。真板 V3 经 3 轮迭代达标（run1 打印耗时耗尽 lease + flush waiter 未释放、run2 帧数未达 slot 边界，均 Act 契约内修复载荷）；run3 操作者原样输出全项达标：i=80 accepted=false tx-slot=64 tx-full=1、capacity tx-submit=16 tx-again=64、restored completions=80 账本复原、flush Pending→release→C4、auto-release=0、owner 恒 Active；操作者确认"reset可以恢复"。详见当前 Cycle Act Response。）

## 3. Stack/socket 与真板四协议（Iteration 002）

- [x] 3.1 **T6**：在K3入口启动既有stack runner，以 `SocketOps`+`axtask::future::poll_io/timeout` 实现UDP/TCP回显载荷，ARP/ICMP沿用既有栈。production local fixture证明device/software/timer三源wake、错误传播、正常TCP关闭及quiet；调用者不poll_interfaces，K3不走polling fallback。（2026-10-12 Act：host fixture RED（E0432 payload 缺失）→GREEN `stack_socket` 1/1，既有14项（12 adapter+1 flush+1 service_owner）保持GREEN，k3-async-net-host 15/15 exit 0；`make k3-async`/`k3-async-probe` exit 0，queue/stack 任务各一（stack start 公共路径、重复启动拒绝）。三源中的timer源按用户2026-10-12原话裁决移至板测V3取证（host axhal 无平台后端 `wall_time()≡0ns`，timer wheel 在宿主结构性不触发；板上 runtime timer IRQ 驱动同一 wheel，MS17 native timer 既有结论覆盖该机制）。详见当前 Cycle Act Response Deviations。）
- [x] 3.2 **T7**：在诊断载荷完善产品侧cause/drop/occupancy/budget/flush/quiet直接snapshot；完成60s真板窗口（暖邻居后ping10/10、UDP/TCP完整echo、EOF）、2s有条件quiet→新IRQ恢复、容量结果及外部reset；运行实际影响面host/build回归与diff review。agent检查完成后交操作者；板测未执行或原生结果不符时不完成。（2026-10-12 V3 两轮板测收口：**A2 全项两轮直证**（ping 10/10×2、UDP/TCP 全字节 echo、EOF、snapshot 行齐备、容量/flush 与 Iteration 001 一致）；第一轮 quiet 前提被 PC 关闭尾流打破（在案如实报告），经用户豁免并在当前 Cycle 实施沉降修复（claim 流静止 500 ms 再开 2 s 冻结窗），**第二轮 quiet-before/after 逐字节一致**（irq-runs=113、task_poll=58、fallback=0 恒定）；孤立 `resume` 行两轮 timeout，按用户裁决豁免——唤醒机制由两轮会话全程行使（每包经真实 source-133 中断处理，含第一轮 quiet 窗内到达即处理的关闭尾流）；外部 RESET 按用户在案指示顺手执行不阻塞。`make k3-async`/`k3-async-probe` exit 0（FIT 783904B，probe 最后构建）、`git diff --check`/`openspec validate` 通过。详见 Cycle Act Response V3 两轮记录与 Blocker Resolution/Handoff。）

## Iteration Plan

### Iteration 000：复用任务底座，闭合 GMAC completion IRQ

- Tasks：T1、T2；Depends on：None。
- Stable baseline：K3异步feature可构建；原生timer future真实恢复；IOC/cause/fatal/arm重查production接口可供adapter复用；deferred task独占Gmac，20次真实TX完成、真实RX事件、无最后事件丢失。
- Verification boundary：最新replan Cycle的A1–A5；host→build→操作者RAM真板→Plan独立Review。只有accepted才能展开001。
- Diagnostic boundary：timer trap/handler/tick→source133/cause/ack→cache后OWN与deferred task；不把协议失败带入本轮。
- Non-goals：NetDriver adapter、64slots、C4flush、socket或MS12整体完成。
- Balance audit：网络feature接入与DMA IRQ共同形成“休眠任务由真实中断和期限恢复”的底层结果；单独做寄存器常量/IRQ观察过碎。将全部queue/socket放入首轮会跨三个独立故障域，因此拆出。

### Iteration 001：K3 接入唯一 queue owner 并闭合容量/flush

- Tasks：T3、T4、T5；Depends on：000 accepted；T4依赖T3，T5依赖T4。
- Stable baseline：既有Service持有K3adapter，owner Active，32/32/32budget、64/64slots、16/16descriptor与CPU池账本成立，C4flush目标集合与错误边界通过，真板两级TX Full→释放→恢复。
- Verification boundary：native adapter/Service/flush测试、feature三面、K3异步和诊断构建；操作者容量与flush板测；真实IRQ底座直接采信000最终accepted结果。
- Diagnostic boundary：driver pool/cookie/descriptor→软件slot/ticket→queue wake/flush waiter；不要求socket或用协议吞吐证明容量。
- Non-goals：四协议最终Acceptance、协议quiet、runtime恢复、吞吐优化。
- Balance audit：adapter、owner激活与压力/flush共同证明同一所有权线；拆为单独测试轮不会形成可交付结果。独立stack/socket runner与四协议有不同故障域，留002。

### Iteration 002：既有 stack/socket 在真板异步推进

- Tasks：T6、T7；Depends on：001 accepted；T7依赖T6。
- Stable baseline：既有双向queue+常驻stack+socket契约适配；真实四协议、正常close和quiet后IRQ恢复通过；MS12 R1–R7均验收。
- Verification boundary：三源wake/quiet/SocketOps生产fixture、受影响回归、60s真板直接交换与reset；不以host/QEMU冒充真板。
- Diagnostic boundary：Service/stack progress→readiness/SocketOps→PC内容与关闭；前轮已验收driver容量不重复证明。
- Non-goals：MS13/MS14/MS17、SMP、性能、驱动框架抽离、维护全局状态/收尾。
- Balance audit：socket载荷与协议/quiet系统验收是同一成果；纯echo实现和单项观测不足以独立成轮。稳定底层已拆出，最后一轮不会承担bring-up。

## Future task contracts（规划范围，待前轮 accepted 后转入自包含 Cycle）

T1/T2的完整执行契约在000-initial的[001-replan](iterations/000-initial/001-replan.md)；T3–T5在[001-queue-owner-capacity-flush/000-initial](iterations/001-queue-owner-capacity-flush/000-initial.md)（均已accepted）；T6/T7的完整执行契约已随 Iteration 001 accepted 展开为[002-stack-socket-protocol/000-initial](iterations/002-stack-socket-protocol/000-initial.md)（draft，待批准）。无剩余未展开契约。

## Requirements Traceability Matrix

| Requirement | Task | Test Witness | Status |
|---|---|---|---|
| R1 | T2、T3、T4 | IOC/cause/arm/last_completion生产fake tests；existing event窗口；真板RI/TI | Covered |
| R2 | T3、T4、T5 | owner_activation、stage_budget_31_32_33、slot_64_65、dual_backlog、rx_drop_wrap | Covered |
| R3 | T3、T5 | tx_pool_descriptor_16_17、complete_frame_reclaim、flush_target_set/new_ticket/cancel/busy/deadline | Covered |
| R4 | T1、T6、T7 | native_timer_wake、stack_three_wakes、quiet_then_irq、四协议/EOF | Covered |
| R5 | T1、T2、T3、T4、T5、T6 | stable_fatal_after_ack、postpublish_retention、stage_timeout、waiter_drop、backing_drop_witness | Covered |
| R6 | T2、T5、T7 | true_dma_cause、native容量/flush snapshot、PCecho、quiet计数 | Covered |
| R7 | T1–T7 | feature隔离、MS11生产回归、受影响build、RAM boot/reset | Covered |

Covered表示计划可追踪，不表示测试已经存在或运行通过；新增见证由Act先建立RED。没有Simplified/Missing。

## Boundaries and Gates

- Gate1：PASS（已批准需求与场景扫描，无需求简化）。Iteration 000/001 各自 Gate2 PASS 并于 2026-10-12 accepted（T1–T5 闭合）。Iteration 002 `000-initial`（Map 最后一轮）：Gate2 PASS（2026-10-12 用户原话"批准"）；2026-10-12 Act 完成（reported），2026-10-13 Plan Review accepted，Iteration 002 完成。
- 原000-initial与001-queue-owner-capacity-flush的Plan Context/Act Response保持原文；各 Plan Review 终态见对应 Cycle 文件。Map 无剩余未展开 Iteration。
- Iteration 002 的 T6 为 agent 可执行闭环；T7 的 60s 真板协议会话与 quiet（新 Cycle V3）是最近能力边界——ping 10/10、UDP/TCP 全字节 echo、EOF、quiet 冻结与恢复、外部 RESET 须直接观察；无结果时 T7 不完成、Act blocked，不收尾。
- 每轮各自停止于板测边界；缺少板测不以host/QEMU或shell运行替代。本轮 accepted 后 MS12 收尾由 Maintainer 承担，不自动执行。
- Persisted Evidence：none。命令、每项≤20行决定性输出、结果写Act Response，不建立Evidence目录或专用采集工具。
- 本次Phase：场景适用性扫描、已验收输入整理、实码/引用用法调查、设计与Task Contract修订、Map平衡审计均completed；结构验证与完整diff审查按proposal本次记录。产品实施/板测、经验落账、全局状态同步和归档均SKIPPED（本次只授权计划）。
- 复用不新增身份/字符串/时间顺序验收；仅模式守卫、原生目标测试、直接设备/ownership/peer行为作为通过条件。
- 范围外实质缺陷候选：None newly discovered。本范围普通axnet编译缺陷由T1负责；未修改或重开Issue。
