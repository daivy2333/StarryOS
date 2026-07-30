# tasks.md — Milestone Roadmap

> Last updated: 2026-07-30
> Scope: 已批准的项目路线、稳定基线和当前阶段边界

## MS01 — 异步 UART 稳定基线

**Status**: completed

- 成果：interrupt-driven async UART、RX/TX copier、VFS/TTY 和 QEMU/D1 路径。
- 契约：blocking/nonblocking backpressure、raw reader/writer 唯一所有权和可解释的 benchmark 证据。

**Stable baseline and boundaries**

- QEMU 只证明功能与回归；D1 单 hart 只证明板级行为与性能，二者都不证明 SMP 正确性。
- TX/RX ring 维持 SPSC；cloneable OS adapter 在等待点外串行化 producer。
- Storage/rootfs、user CQ、mmap user ring 已取消当前规划；需要时重新 propose。
- Q31/Q32 Async/Console CPU-efficiency 只作同口径测量，不能把 polling Console 结果泛化为 async 架构结论。

**Evidence**

- 当前模型与决策：M01-M40、D01-D21。
- 已验证知识与索引：K01-K30、R01-R49（R8/R9 为统一 tombstone）。
- 改进状态：I01-I10、I12。
- 历史任务原文：`openspec/changes/archive/2026-07-30-mig-202607301654/active-originals/tasks-original.md`。

## MS02 — Multi-hart UART correctness

**Status**: blocked

**Active**: `q17-smp-memory-ordering`，18/19 tasks。

- 已完成：IER cache RMW 临界区、D1 同形态边界、TX copier/completion Release/Acquire 或 AcqRel/Acquire、QEMU 回归见证。
- 未完成：等价 multi-hart 硬件 stress。

**Capability boundary**

- 缺少可运行当前 UART 路径的 VisionFive2 或等价 SMP 真板。
- 等待不属于可执行 task；硬件与串口采集可用后，由 Plan 复核基线并创建任务。

**Resume condition**

- 至少两个 hart 可并发访问同一 UART 软件路径。
- 可采集完整启动、stress、timeout 和最终计数器/状态。
- 当前 change、相关 M/D/K/I 与代码基线重新验证无漂移。

**Acceptance**

- 覆盖跨 hart write、read、flush/tcdrain 和 IER enable/disable。
- 无丢失、重复、hang、`tx_staged_bytes` 漂移或 completion 提前可见。
- 记录 PLIC/UART preserved state，复跑 Q15 关键 Manual QA。
- 只有 fresh multi-hart evidence 通过后，才可关闭 I05 与 `q17-smp-memory-ordering`。

## MS03 — TX multi-producer semantics

**Status**: planned / evidence-triggered

**Trigger**: MS02 或 workload 证明 producer serialization 存在消息边界破坏、饥饿、交互延迟、锁竞争或吞吐问题。

**Planning contract**

- 分开定义 syscall atomicity、fairness、锁竞争、延迟和吞吐目标。
- 比较 SPSC + 串行化、提交粒度、调度队列和 MPSC；不得默认以 MPSC 代替全部问题。
- 未满足触发条件时维持 accepted-prefix 契约与 I03/I04 的远期状态。

## MS04 — DMA and high baud rate decision

**Status**: planned / evidence-triggered

**Trigger**: MS02 或新硬件数据证明 UART FIFO、PIO、CPU 成本或 115200 bps 限制目标 workload。

**Planning contract**

- 先确认目标 SoC DMA 控制器、UART FIFO 可达性、IRQ/completion 模型与内存一致性边界。
- 分别评估 PIO、DMA 和 230400+ 波特率，不把三者绑定为一次实现。
- 没有硬件数据时保持 I06/I08 的候选状态，不创建实施任务。

## Legacy Task Mapping

| Legacy scope | Current target |
|---|---|
| Q0-Q16、Q18-Q23、Q26-Q29、Q31-Q32 | MS01 completed；细节由 migration carrier 与各 archived change 恢复 |
| Q17.1-Q17.5 | MS01 已完成基线；实现证据保留在 `q17-smp-memory-ordering` |
| Q17.6、Q24 | MS02 blocked |
| Q30 | MS03 planned / evidence-triggered |
| Q25 | MS04 planned / evidence-triggered |
| Q19D storage/rootfs | canceled；需要时重新 propose |
| Q21/Q22 user CQ / mmap ring | canceled；D21 与 I02 保留重新评估边界 |
| M4 Sync rollback 与早期阶段细节 | archived history；不得恢复为当前任务 |
