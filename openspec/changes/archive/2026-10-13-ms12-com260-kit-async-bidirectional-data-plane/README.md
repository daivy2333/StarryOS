# ms12-com260-kit-async-bidirectional-data-plane

将已验收的 CoM260 Kit 板级 IRQ 底座、GMAC DMA 与轮询队列适配到既有异步 queue/stack 契约。需求、设计与三轮执行计划已于 2026-10-06 获批；2026-10-08 修订契约获批。Iteration 000（T1/T2：异步构建入口 + 真实 GMAC DMA IRQ→deferred task）与 Iteration 001（T3–T5：NetDriver adapter、唯一 queue owner、容量与 C4 flush）均已于 2026-10-12 完成 accepted（各含用户审计 PASS + Plan 独立 Review；host 14/14、axnet 三面 482/506/514、harness 31、MS11 回归 17、两次真板 run3 全项 + reset 确认）。最后一轮 Iteration 002（T6–T7：stack runner + socket 载荷、真板四协议与 quiet）已于 2026-10-13 完成 accepted（host 15/15 含 stack_socket、V3 两轮板测 A2 全项 + quiet 冻结逐字节直证、resume 行按用户裁决豁免）；MS12 R1–R7 全闭合，change 收尾归档。

串口与shell已在当前代码基线交付。复用timer/AIA/调度底座，保留独立网卡内核板测入口，网络单hart稳定后再在后续组合阶段合并UART/网络/shell并推进多hart。完整复用、边界与来源见[设计](design.md)。Iteration 000 历史现场见[000-initial](iterations/000-initial/000-initial.md)与[001-replan](iterations/000-initial/001-replan.md)。

入口：[需求与场景](proposal.md)、[设计与调查](design.md)、[三轮任务计划](tasks.md)、[当前执行契约](iterations/002-stack-socket-protocol/000-initial.md)。
