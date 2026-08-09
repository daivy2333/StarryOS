# Spec: decisions - 决策记录

## Purpose

记录有替代方案且影响长期维护的重要选择、原因、替代方案、影响和状态。条目使用 `Dxx` 编号，被替代后保留历史并标记 `superseded`。Legacy ADR 原文：`openspec/changes/archive/mig-20260720-legacy-specs/architecture-original.md`（hash: `5b054d98`）。

## Requirements

### Requirement: D01 - 异步运行时选型

异步运行时 MUST 采用 `axtask::future`（`block_on` + `poll_io` + `register_irq_waker`）+ `embassy-sync::AtomicWaker` 方案。

**Legacy**: ADR-001 (A001), 2026-05-24 | **状态**: ✅ accepted
**关联模型**: M01

- **原因**: axtask 已有调度器，embassy-executor 会冲突；embassy-sync 无 OS 依赖可单独使用；Pipe / EventFd 已验证 axtask::future 模式可行。
- **影响**: 保留内核调度器独立性；需自定义 AsyncUart trait；ISR 唤醒走 AtomicWaker::wake，O(1) 复杂度。
- **替代方案**: 完整 Embassy（与 axtask 冲突，拒绝）、仅 embedded-io-async traits（仍需自建 IRQ 绑定，拒绝）。

#### Scenario: 评估异步运行时替换

- **WHEN** 开发者提议替换当前异步运行时
- **THEN** 必须证明新方案不与 axtask 调度器冲突，且 ISR 唤醒延迟不超过当前 AtomicWaker

<!-- arc: cleanup-uart-documentation-system --> D03 (UART buffer strategy evolution) archived 2026-07-25.

### Requirement: D11 - LTO 延期

`lto = true` MUST 暂不开启，记录为已知优化手段，最终发布前再加回。

**Legacy**: ADR-034 (A034), 2026-06-16 | **状态**: ✅ accepted
**关联模型**: M13

- **实测效果**: Ring buffer TX 385->652 MB/s（↑69%），RX P50 200ns-><100ns。
- **决策理由**: LTO 使 release build 时间增加 2-3×；当前活跃开发期编译速度更重要。

#### Scenario: 发布构建准备

- **WHEN** 项目进入开发冻结期
- **THEN** LTO MUST 在发布构建前重新启用

### Requirement: D20 - 异步 NIC 架构分层

异步 NIC MUST 采用队列任务与协议栈 runner 分层，不引入 Embassy executor。首阶段 MUST 保留 axnet-ng、smoltcp、axpoll 和 axtask。

**Legacy**: ADR-063 (A063), 2026-07-18 | **状态**: ✅ accepted，2026-07-27
**关联模型**: M36

- **分层**: 硬中断（cause/ack/mask/wake）-> queue task（descriptor reap/refill，有 budget）-> stack runner（smoltcp poll，task 上下文）-> socket readiness。
- **借鉴**: embassy-net-driver Context 感知 readiness 与 RxToken/TxToken 所有权；ArceOS DWMAC/DMA 硬件证据。
- **拒绝**: 硬中断内全栈 poll、平台 IRQ 硬编码、全局大锁。

#### Scenario: 规划首个异步 NIC change

- **WHEN** 创建首个 StarryOS 异步 NIC change
- **THEN** MUST 定义 descriptor 状态机、IRQ rearm、register-recheck、TX/RX backpressure、DMA/cache barrier、budget 和公平性 Gates

### Requirement: D21 - PLIC/Clock trust-u-boot 与 PLIC 防御

VisionFive2 bring-up MUST 保留 U-Boot 配置的 PLIC 和 Clock 状态（范围收紧为 PLIC + Clock，不包含 UART）。PLIC init_primary/init_percpu MUST 保持显式分离作为防御性设计。

**Legacy**: ADR-040 (A040), ADR-041 (A041), 2026-06-26 | **状态**: 🟡 Proposed / 防御性保留
**关联模型**: M37, M38

- **arceos 教训**: "trust u-boot" 模式仅用于 DWMAC（以太网），不是平台级模式。7+ 次失败后才定档。StarFive UART 走 SBI，不做 UART MMIO init。
- **NS16550 差异**: UART 初始化（设波特率/FCR/IER）是简单寄存器写入，重复设置无害，不像 DWMAC PHY 协商会破坏已建立链路。
- **PLIC 防御**: 当前 axplat 已用 `static SpinNoIrq<Plic>` + 幂等 init_by_context，安全。但旧 arceos `LazyInit<Plic>` 反模式若被重新引入会导致 SMP panic。

#### Scenario: 评估 trust-u-boot 范围扩展

- **WHEN** 开发者提议将 trust-u-boot 扩展到 UART 或其他外设
- **THEN** MUST 先证明重复初始化会破坏已建立状态，NS16550 寄存器写入通常无害

### Requirement: D22 - QEMU 首条异步 NIC 路径使用 VirtIO-MMIO

QEMU 首条异步 NIC 路径 MUST 使用 VirtIO-MMIO。PCI 仅在 I13 的构建和运行 Gate 通过后进入兼容性评估。真板 transport MUST 根据板级事实选择，不继承 QEMU 的总线选择。

**来源**: 2026-07-27 QEMU MMIO/PCI 对照验证 | **状态**: ✅ accepted
**关联模型**: M41 | **关联知识**: K31, K32

- **原因**: 当前 MMIO 网卡和块设备可启动到 shell。PCI 设备模型可创建，但 StarryOS 实际仍编译 MMIO probe。
- **替代方案**: PCI-first 会同时引入 feature、ECAM、BAR 和 IRQ 变量，暂不采用。
- **影响**: 先完成 MMIO IRQ、RX 和 TX。PCI 不阻塞 QEMU 主线；真板在 QEMU 异步基线完成后先通过板级事实 Gate，再选择控制器后端。

#### Scenario: 规划 QEMU NIC milestone

- **WHEN** change 涉及首条 QEMU 异步收发路径
- **THEN** MUST 使用 VirtIO-MMIO 基线
- **AND** MUST 先单独验证 MMIO IRQ，再引入异步队列
- **AND** MUST NOT 把 QEMU 结果声明为真板证据

<!-- arc: MIG-20260720-legacy-specs --> Legacy original: openspec/changes/archive/mig-20260720-legacy-specs/architecture-original.md (hash: 5b054d98). Decision rationale extracted as D01-D21. Tombstoned ADRs: A014-A017, A020-A021, A032, A035, A056, A063-A064 -> archive carriers ARC-202607081429, ARC-202607021648, arc-202607152005.
<!-- arc: ARC-202607251326 --> 16 D 条目已归档 (2026-07-25) -> openspec/changes/archive/2026-07-25-arc-202607251326/proposal.md
<!-- arc: cleanup-uart-documentation-system --> D03 archived (2026-07-25) -> openspec/changes/archive/2026-07-25-cleanup-uart-docs/
