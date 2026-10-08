## Plan status

需求批准：2026-10-06，用户原话“批准”；Gate1 PASS。计划批准：2026-10-06，用户原话“批准当前的工作”；Gate2 PASS，首轮Cycle为ready，所有实施项未开始。逻辑任务用T1–T7；checkbox编号与其一一对应。后两轮只作Map，不在本次创建Cycle或授权执行。路线调整不改变本计划的任务契约与验收。

当前排期：2026-10-06 用户要求先规划并完成 MS17。本计划保留，原首轮暂停执行；MS17 accepted 后，由 Plan 消费实际共用底座结果并复核执行契约。timer/IRQ/feature 或当前基线变化时创建 replan Cycle，不改写原首轮、不重复实现已验收底座，也不删减原验收。交接指针：MS17 已在 K3 上 accepted 交付 runtime timer/AIA 底座、PXA UART 异步 port、copier 数据面与物理 drain、tty canonical 与内存根内嵌用户态 stdio（各 Iteration 状态见 [../ms17-com260-kit-async-uart-interactive-shell/tasks.md](../ms17-com260-kit-async-uart-interactive-shell/tasks.md)）；本计划 T1 恢复时其 timer/IRQ/feature 接缝以该底座的 accepted 结论为输入，GMAC/网络 queue/socket 面仍全部归本计划。

## 1. Runtime 与真实 DMA IRQ（Iteration 000）

- [ ] 1.1 **T1**：在根/kernel Cargo feature、Makefile、K3 `irq.rs/time.rs` 接通既有timer handler和scheduler；提供dyn依赖的K3限定mem_iomap Unsupported ABI入口；精准修复 `axnet/async_rx.rs` 普通构建的诊断调用cfg。验证timer register/duplicate/mask/unregister/dispatch原生host测试、ordinary axnet既有event窗口GREEN、`make k3-async`链接/FIT成功；原K3polling feature不开放STIE。
- [ ] 1.2 **T2**：在 `k3_gmac/{mod,regs,desc}.rs` 与K3 IRQ/网络入口实现设备侧IOC、最小ISR、fatal latch、mask/arm/descriptor重查和deferred raw会话。验证production路径fake MMIO/cache竞态及MS11回归；构建后由操作者RAM启动，真实20ARP TX完成、RI/TI重复cause/133claim/deferred唤醒、最后完成可回收；外部reset回原系统。未提交板测结果时本task保持未完成。

## 2. 既有 queue 契约、容量与 C4 flush（Iteration 001）

- [ ] 2.1 **T3**：新增 `k3_gmac/adapter.rs` 实现既有 `NetDriverOps/NetQueueControl/NetTxQueue`；原生固定NetBufPool与16项cookie owner账本，恢复control为None。生产模块host见证覆盖长度、14/60/1514/1515、pool/descriptor16/17、wrap、单次回收、postpublish error、timeout与backing保留；原生Cargo GREEN。
- [ ] 2.2 **T4**：在 `platform/k3_async_net.rs` 接入 `AxDeviceContainer::from_one`、现有Service、唯一queue任务；旧polling session不并行运行。使用production adapter+axnet local Service fixture证明激活失败/重复、各stage31/32/33、RX64/65、TX Full后恢复、通知窗口、稳定fault；异步构建成功，保留32budget与64slots。
- [ ] 2.3 **T5**：把axnet已有submit/reclaim lease隔离到 `queue-diagnostics`，保留QEMU恢复专用入口；添加最小完整帧诊断enqueue和K3载荷，复用现有flush。验证ordinary/queue-diagnostics/qemu-diagnostics三面及flush target/Busy/cancel/timeout；真板200ms hold下TX64/65与driver16/17、flush Pending→release→C4成功、owner容量复原、auto-release=0。不得添加判定器。

## 3. Stack/socket 与真板四协议（Iteration 002）

- [ ] 3.1 **T6**：在K3入口启动既有stack runner，以 `SocketOps`+`axtask::future::poll_io/timeout` 实现UDP/TCP回显载荷，ARP/ICMP沿用既有栈。production local fixture证明device/software/timer三源wake、错误传播、正常TCP关闭及quiet；调用者不poll_interfaces，K3不走polling fallback。
- [ ] 3.2 **T7**：在诊断载荷完善产品侧cause/drop/occupancy/budget/flush/quiet直接snapshot；完成60s真板窗口（暖邻居后ping10/10、UDP/TCP完整echo、EOF）、2s有条件quiet→新IRQ恢复、容量结果及外部reset；运行实际影响面host/build回归与diff review。agent检查完成后交操作者；板测未执行或原生结果不符时不完成。

## Iteration Plan

### Iteration 000：任务 deadline 与 GMAC completion IRQ 可工作

- Tasks：T1、T2；Depends on：None。
- Stable baseline：K3异步feature可构建；原生timer future真实恢复；IOC/cause/fatal/arm重查production接口可供adapter复用；deferred task独占Gmac，20次真实TX完成、真实RX事件、无最后事件丢失。
- Verification boundary：首轮Cycle的A1–A5；host→build→操作者RAM真板→Plan独立Review。只有accepted才能展开001。
- Diagnostic boundary：timer trap/handler/tick→source133/cause/ack→cache后OWN与deferred task；不把协议失败带入本轮。
- Non-goals：NetDriver adapter、64slots、C4flush、socket或MS12整体完成。
- Balance audit：timer与DMA IRQ共同形成“休眠任务由真实中断和期限恢复”的底层结果；单独做寄存器常量/IRQ观察过碎。将全部queue/socket放入首轮会跨三个独立故障域，因此拆出。

### Iteration 001：K3 接入唯一 queue owner 并闭合容量/flush

- Tasks：T3、T4、T5；Depends on：000 accepted；T4依赖T3，T5依赖T4。
- Stable baseline：既有Service持有K3adapter，owner Active，32/32/32budget、64/64slots、16/16descriptor与CPU池账本成立，C4flush目标集合与错误边界通过，真板两级TX Full→释放→恢复。
- Verification boundary：native adapter/Service/flush测试、feature三面、K3异步和诊断构建；操作者容量与flush板测；真实IRQ底座直接采信000。
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

T1/T2的完整执行契约在当前Cycle；以下闭合后续契约语义，不授权跨轮执行。

### T3：NetDriver traits 与 ownership

- Requirement/Scenario：R1/R3/R5/R7；S2–S4、S7–S9、S16–S18、S20。
- Depends on：T2 accepted。
- Targets：新 `kernel/src/platform/k3_gmac/adapter.rs`；Gmac单步及必要只读owner观察；新 `tests/k3-async-net-host/` path-includes production，使用本仓axdriver_net/critical-section std，不复制生产实现。
- Current/Required behavior：当前没有trait adapter；目标行为严格按design D3，固定TX16/RX1 CPU pool、actual descriptor slot→cookie、postpublish保留；queue control复用D2；无recovery control。
- Preserve/Forbidden：MS11 DMA/tail/cache、已发布backing保留、原traits及ABI；禁zero-copy、公共新queue协议、自动reset、动态扩容、修改参考仓库。
- Test witness：新增native Cargo测试预期RED（adapter缺失或行为失败）；GREEN覆盖16/17、wrap、长度、duplicate/epoch错误、LocalError、timeout/latecompletion与Backing Drop。
- Verification：`cargo test --manifest-path tests/k3-async-net-host/Cargo.toml --offline`；新增package lock后加`--locked`。失败时保持原owner，不扩大测试工具。
- Stop when：需要改公共trait语义、CPU/DMA owner分界或真实容量16失效，返回Plan。

### T4：唯一 queue 与有界双向推进

- Requirement/Scenario：R1/R2/R3/R5/R7；S2–S9、S12、S16–S17、S20。
- Depends on：T3。
- Targets：新 `platform/k3_async_net.rs`；`k3.rs`异步分支；`crates/axnet/src/async_rx.rs`既有local fixture；必要dev-dependency接入生产adapter fixture。
- Current/Required behavior：当前K3未进入Service；目标按D4准备顺序，actual schedulable mask、queue Active后才允许后续runner；初始化仅以yield_now和rx_snapshot在5s绝对上限内等Active，fault/unavailable/超时停；32/32/32阶段及64/64软件容量保留。
- Preserve/Forbidden：QueueEvent/wait protocol、Full保留帧、slots完整帧、failure-atomic start；禁第二owner/第二executor、以periodic polling补最后wake、扩大MS17。
- Test witness：native local Service fixture注入生产adapter，新增激活/31/32/33/64/65/双向/wake/Faulted测试RED→GREEN；既有成功测试直接采信。
- Verification：adapter包与axnet `--lib`相关测试，`make k3-async`；GREEN必须是生产路径与原生结果，不是source字符串guard或test-only重新实现。
- Stop when：Service/stack抢descriptor、fault回到Polling或需要改变budget/slots，返回Plan。

### T5：实际容量与 target-scoped C4 flush

- Requirement/Scenario：R2/R3/R5/R6/R7；S6–S12、S16–S20。
- Depends on：T4。
- Targets：axnet `Cargo.toml`、`{lib,diag,service,async_rx}.rs`的hold/lease相关cfg和最小frame入口、Ethernet现有完整帧enqueue、K3诊断feature/载荷。
- Current/Required behavior：压力控制仅QEMU；目标D5共享hold，QEMU recovery仍隔离，200ms hold不修改OWN，原生flush原目标live集合决定结果；1s timer取消只结束等待。
- Preserve/Forbidden：QEMU API/数值/快照ABI、sole owner、checked ticket；禁日志判定层、压力协议/CLI、改变正常socket路径、board reset recovery。
- Test witness：新增K3诊断编译及actualadapter flush RED→GREEN；目标slot+device集合、新ticket、乱序、empty、Busy、drop、deadline、FBE均测试；原lease测试作保持GREEN见证。
- Verification：ordinary、queue-diagnostics、qemu-diagnostics原生axnet测试；真板64→第65Full、16→第17Full、release后原帧继续、flush成功和全部容量复原；每次hold auto-release=0。严格budget由host31/32/33证明。
- Stop when：安全200ms窗内不能到达边界、需改变deadline/容量、必须造DMA故障或新增控制框架；交Plan，不隐去未达项。

### T6：stack/socket 三源wake

- Requirement/Scenario：R4/R5/R7；S5、S12–S18、S20。
- Depends on：T5 accepted。
- Targets：K3 async入口/载荷、既有axnet `stack_runner.rs`、`SocketOps`/`Pollable`边界测试；必要timer/wake接入。
- Current/Required behavior：前轮queue稳定，尚未证明stack/socket；目标按D4启动独立runner，UDP26001/TCP26002应用用原生poll_io和5s等待，不主动poll；TCP完整echo后正常关闭。
- Preserve/Forbidden：协议timer、readiness错误语义、原SocketOps；禁旧polling_net session、userspace/shell/syscall扩张、固定网络poll timer。
- Test witness：既有per-test socket fixture接actualadapter，新增三源/close/fault/quiet测试RED→GREEN。
- Verification：axnet相应native tests、`make k3-async`；无active polling fallback，queue/stack各一，deadline真实唤醒。
- Stop when：须更改socket公共接口、协议正确性或引入用户态底座，返回Plan。

### T7：真实协议、quiet 和兼容性收尾

- Requirement/Scenario：R4/R6/R7；S1、S6–S10、S13–S15、S18–S20。
- Depends on：T6。
- Targets：K3诊断载荷的native snapshots/输出；现有build入口与受影响回归。不会写全局tasks/M/R/I或Runbook/Issue。
- Current/Required behavior：host可验证契约，真板最终结果待测；目标D6暖邻居后ping10/10、UDP/TCP完整内容、EOF、2s有效quiet及新IRQ恢复、reset回原Linux。既有容量结果若覆盖未变直接采信。
- Preserve/Forbidden：RAM-only、native判定、已批准Wireshark豁免；禁审计工具/身份字段、重复跑成功测试增强信心或把未测计为通过。
- Test witness：host载荷与真实生产socket交互RED→GREEN；人工目标有直接native网络与串口状态，未执行不完成。
- Verification：实际影响面native host与`make k3`/`make build`/`make lichee`；板侧60s会话与quiet，PC内容/EOF。VF2与host-test pre-existing失败独立记录，不声称全量GREEN。
- Stop when：无有效quiet前提、PC内容/关闭不符、reset未回系统；停并交Plan，不追加豁免。

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

- 原获批范围：首轮T1/T2，原Cycle ready；当前因用户选择先MS17而暂停执行。MS17 accepted 后先由Plan复核/调整契约，再恢复适用Cycle；原ready状态不覆盖本次排期边界。Act尚未调用，后续实施不得跨轮。
- T2 host/build完成后最近能力边界是真板操作者；等待原生RI/TI、20TX terminal、deferred wake与reset结果，T2未完成。操作者结果审核PASS、当轮代码与Acceptance独立Review accepted后才能展开001。
- 001、002同样停于其板测边界，不能仅host通过自动展开下轮。
- Persisted Evidence：none。命令、决定性输出、结果写Act Response，Plan不建Evidence占位目录。
- Gate2：**PASS**；技术计划已闭合，设计、Map与首轮Cycle于2026-10-06获批。产品实施、Recorder和归档均SKIPPED（本次为文档调整）；全局路线/I/状态同步已授权，但.agents写入受环境限制，补丁待应用。
- Issue候选：普通axnet未隔离的`net_link_step_diag`调用，当前原生Cargo exit101；它在本范围内且是必要前置，T1修复。未调用Recorder落账。
