# tasks.md — 任务追踪

> 最后更新: 2026-07-25 | 分支: net-k3 | grep: `<!-- T{编号} -->`
> 来源: R41；NIC 开发 N0-N5 已展开为 13 个可执行 milestones。

---

## 当前：异步 NIC 开发

每个 milestone 是一个主要变更边界。执行前必须通过 OpenSpec Plan 建立 BDD、RTM、测试见证和获批 change。完成状态只依据对应 change 的新鲜证据更新。

执行顺序固定为 T01→T13。QEMU 与真板是不同证据类别。前置 Gate 未通过时，后续 milestone 保持等待。

| ID | Milestone | 交付范围 | 验证 Gate | 前置 | 状态 |
|---|---|---|---|---|---|
| <!-- T01 --> T01 | N0-A smoltcp/axnet 基线 | 纳入本地 smoltcp 0.13.1；本地化 axnet；移除 `RxToken::preprocess` 私有依赖 | TCP listen/accept、UDP、nonblocking 和 poll 与同步基线一致 | 无 | ⏳ 可进入 Plan |
| <!-- T02 --> T02 | N0-B QEMU PCI 见证 | 显式选择 VirtIO PCI；记录 bus、device ID、IRQ、feature、queue 和最低计数器 | IRQ、RX、TX completion 可重复；pcap 对齐；无 busy loop | T01 | ⏳ 等待 T01 |
| <!-- T03 --> T03 | N1-A queue task | 建立 `NetQueueControl`、设备 IRQ handler、AtomicWaker、唯一 owner 和 bounded budget | event-before-register、register-during-event、spurious IRQ 无 lost wakeup；ISR 有界 | T02 | ⏳ 等待 T02 |
| <!-- T04 --> T04 | N1-B 有界 packet slot | 建立 RX/TX slot、`StackNetDevice`、partial write、drop reason 和 occupancy | ring/slot 满时背压可见；内存有上界；无 descriptor 跨 await 泄漏 | T03 | ⏳ 等待 T03 |
| <!-- T05 --> T05 | N2-A stack runner | 独立推进 smoltcp ingress、egress、maintenance 和 timer | device、software、timer 唤醒均可复现；空闲无轮询；持续流量不饥饿 | T04 | ⏳ 等待 T04 |
| <!-- T06 --> T06 | N2-B socket readiness | 将 smoltcp 单槽 waker 桥接到 `axpoll::PollSet`；观测 64 waiter 容量边界 | 多 waiter、overflow、close 和 error 下，poll/select 与实际 I/O 一致 | T05 | ⏳ 等待 T05 |
| <!-- T07 --> T07 | N3 恢复语义 | 引入 reset generation、stale completion 丢弃、cancel、timeout 和 link flap | fault injection 下无 UAF、重复回收、永久 Pending 或静默丢包 | T06 | ⏳ 等待 T06 |
| <!-- T08 --> T08 | N3-MMIO parity | 给 VirtIO MMIO 补设备 IRQ facts、handler 和 rearm | PCI/MMIO 功能集一致；MMIO 不依赖 socket 主动 poll | T07 | ⏳ 等待 T07 |
| <!-- T09 --> T09 | N3-SMP | 定义 queue affinity、跨 hart wake、控制面同步和 ordering 理由 | 多 hart 双向压力与 reset/I/O 交错无 race；单 hart 结果不计 SMP 通过 | T08 | ⏳ 等待 T08 |
| <!-- T10 --> T10 | N4-A VF2 平台 | 接通 VF2 feature、kernel descriptor、启动链、寄存器、clock/reset/PHY 和 PLIC | 真板 B0-B4：重复启动、寄存器可访问、PHY 可见、IRQ 可重复 | T09；硬件可用 | ⏳ 等待 T09 |
| <!-- T11 --> T11 | N4-B DWMAC 收发 | 建立 DMA 地址、cache/barrier、descriptor ownership 和最小 RX/TX | 真板 B5-B6：descriptor 移动与包抓取一致；ARP/ICMP/UDP/TCP 通过 | T10 | ⏳ 等待 T10 |
| <!-- T12 --> T12 | N4-C 真板恢复与压力 | 验证 burst、双向、ring full、link/reset、长时间运行和多 hart | 真板 B7：原始日志、drop/stall/p99/generation 和环境可复现 | T11 | ⏳ 等待 T11 |
| <!-- T13 --> T13 | N5 数据驱动优化 | 按测量结果逐项评估 descriptor token、batch、moderation、offload、zero-copy 和 multiqueue | 每项独立 A/B；正确性 Gate 不退化；无数据则 SKIPPED 并记录原因 | T12；指标触发 | ⏳ 等待 T12 |

共同约束：

- M36/D20：ISR、queue task、stack runner、socket readiness 分层。
- K09：只采用 `embassy-sync::AtomicWaker`；不引入第二套 executor。
- K26：以 packet buffer 和 DMA descriptor 为单位，不复制 UART 字节 ring。
- M37/M38：VF2 保留 U-Boot 的 PLIC/Clock 状态，并分离 primary/per-hart 初始化。
- M39：跨 hart ordering 按同步角色说明；QEMU 单 hart 不能作为 SMP 证据。
- I06 只在 T10-T12 的触发条件满足时评估，避免提前复制 ArceOS 样例。

---

## UART 文档已归档

UART 文档已归档；q17 multi-hart SMP 验证 deferred（task 6.1 未完成）。完整任务见 `uart-lichee` 分支。归档载体见 `openspec/changes/archive/2026-07-25-cleanup-uart-docs/`。

## 活跃 Change

无活跃 change。2026-07-25 已归档：`cleanup-uart-documentation-system`（含 carrier `cleanup-uart-docs`）、`q17-smp-memory-ordering`（deferred, task 6.1 未完成）、`ARC-202607251326`。NIC T01/N0-A 启动时创建新 change。
