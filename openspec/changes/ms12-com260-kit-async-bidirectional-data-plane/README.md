# ms12-com260-kit-async-bidirectional-data-plane

将已验收的 CoM260 Kit GMAC IRQ、DMA 与轮询队列适配到既有异步 queue/stack 契约。需求、设计与三轮执行计划已于 2026-10-06 获批；首轮执行契约 ready，实施未开始。

当前先完成 MS17，本计划保留并暂停执行；恢复前由 Plan 根据 MS17 的 accepted 共用底座结果复核执行契约。MS17 已交付共用底座（K3 runtime timer/AIA 底座、PXA UART 异步 port、copier 数据面与物理 drain、tty canonical 输入、内存根内嵌用户态 stdio），各 Iteration 的 accepted 状态以其产物为准：见 [../ms17-com260-kit-async-uart-interactive-shell/tasks.md](../ms17-com260-kit-async-uart-interactive-shell/tasks.md)。详见 [当前执行顺序](proposal.md#当前执行顺序2026-10-06)。

入口：[需求与场景](proposal.md)、[设计与调查](design.md)、[三轮任务计划](tasks.md)、[首轮执行契约](iterations/000-initial/000-initial.md)。
