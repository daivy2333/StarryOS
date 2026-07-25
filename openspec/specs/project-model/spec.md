# Spec: project-model — 项目模型与跨模块约束

## Purpose

记录当前有效的跨模块模型、边界和约束。条目使用 `Mxx` 编号，不记录历史选择过程。对应 Legacy: `openspec/specs/architecture/spec.md` (hash: `5b054d98`) 中的当前有效约束。

## Requirements

### Requirement: M01 — 异步运行时选型

异步串口运行时 MUST 基于 `axtask::future`（`block_on` + `poll_io` + `register_irq_waker`），并 MUST 引入 `embassy-sync::AtomicWaker` 用于 ISR 中安全唤醒 Waker，禁止引入完整 Embassy 或 `embassy-executor`。

**Legacy**: ADR-001 (A001), 2026-05-24 | **状态**: ✅ 已落地

#### Scenario: 实现新的 UART 异步原语

- **WHEN** 开发者实现新的 `Future` 或 `Pollable` 用于 UART I/O
- **THEN** 必须基于 `axtask::future::poll_fn` + `embassy_sync::AtomicWaker` 模式编写，禁止引入 `embassy-executor`

### Requirement: M03 — 缓冲与并发策略

每个方向（RX / TX）MUST 使用各自独立的 ring buffer，硬件 FIFO 与内核 ring buffer 之间的搬运 MUST 由单一后台 copier 任务完成。当前实现使用 `atomic_ring_buffer`，TX ring 保持 SPSC，RX ring 保持 SPSC 且 unsafe 唯一 consumer。

**Legacy**: ADR-004 (A004), ADR-061 (A061), ADR-062 (A062) | **状态**: ✅ 已落地，TX/RX 契约由 Q28/Q29 收敛

#### Scenario: 设计新的数据通路

- **WHEN** 开发者实现 UART 与用户空间之间新的数据搬运路径
- **THEN** 必须确保硬件 FIFO 的读取/写入由单一 copier 任务完成，禁止在 ISR 中直接操作 ring buffer，禁止多个任务并发 drain 同一 FIFO

### Requirement: M07 — 内核日志同步阻塞约束

内核启动日志的同步阻塞开销 MUST 接受为既定约束（`ax_println!` 依赖外部 crate 的 `axhal::console::write_bytes`，不可修改）。用户态 Console 输出 MUST 可异步化。

**Legacy**: ADR-013 (A013), 2026-05-27 | **状态**: ✅ 关键约束

#### Scenario: 修改内核日志路径

- **WHEN** 开发者想改 `ax_println!` 走异步路径
- **THEN** 不可行 — 该路径依赖外部 crate；必须保留 Console polling TX 作为内核日志通道

### Requirement: M13 — LTO 延期启用

LTO MUST 在最终发布前重新启用。活跃开发期编译速度优先，`lto = true` 暂不开。

**Legacy**: ADR-034 (A034), 2026-06-16 | **状态**: ✅ 已记录

#### Scenario: 发布构建准备

- **WHEN** 项目进入开发冻结期
- **THEN** LTO MUST 在发布构建前重新启用，并验证 ring buffer 吞吐量回归基线

### Requirement: M14 — OS 抽象最小接口

OS abstraction layer MUST 只保留驱动代码实际调用的 trait。当前为 `OsRuntime` + `OsWakerSet` 二 trait 最小可移植接口。禁止保留未被 import 或调用的 trait。

**Legacy**: ADR-036 (A036), 2026-06-19 | **状态**: ✅ 已落地

#### Scenario: 检测到死 trait

- **WHEN** `cargo build` 报告 OS abstraction 类型的 dead_code warning
- **THEN** 未使用的 trait MUST 从 OS abstraction 层删除，对应的 adapter impl SHALL 删除

### Requirement: M32 — lint 与测试 Gate 分层

后续 clippy/test 清理 proposal MUST 按 artifact、feature、target 和平台配置分层。可复用 crate 用 host check/test/clippy；kernel 用目标架构 + feature compile gate；IRQ/TTY/rootfs 行为用 QEMU/真板 gate。

**Legacy**: ADR-059 (A059), 2026-07-13 | **状态**: 候选

#### Scenario: 定义 clippy 和测试 gate

- **WHEN** 后续 change 清理 StarryOS 或 `uart_16550` 的 warning、clippy 和 tests
- **THEN** MUST 为可复用 crate、kernel target build 和系统 runtime 定义分离的 gate

### Requirement: M33 — io_uring 同构点映射

后续 io_uring-inspired UART proposal MUST 先回答"借鉴哪个 io_uring 思想"以及"为什么当前实现不足"。高价值借鉴方向：backpressure、writer/SPSC 隐患、`TxCompletion` 全局 drain。user ring/CQ/zero-copy 当前不实施。

**Legacy**: ADR-060 (A060), 2026-07-14 | **状态**: 候选

#### Scenario: 为 async UART 复用 io_uring 思想

- **WHEN** 未来 async UART proposal 以 io_uring 为动机
- **THEN** MUST 声明复用哪个 io_uring 思想以及为什么当前 UART 路径不足
- **AND** MUST NOT 在未满足 ADR-058 和 O82 证据 gate 的情况下重新引入 user ring、CQE 或 zero-copy

### Requirement: M35 — TX/RX 并发契约分流

Q28 后 UART 并发工作 MUST 将 multi-hart correctness（Q24）、TX scheduling semantics、queue producer model（Q30）和 RX consumer safety（Q29）分开评估，禁止合并为单一 MPSC 方案。

**Legacy**: ADR-062 (A062), 2026-07-18 | **状态**: Q29 ✅ / Q24 ⏳ / Q30 🧊

#### Scenario: 规划 Q28 后并发工作

- **WHEN** 后续工作涉及跨 hart UART、TX 多 writer 语义或 RX 多 consumer 风险
- **THEN** MUST 先归类为 Q24、Q29 或 Q30
- **AND** MUST 保持当前 accepted-prefix 和 SPSC 边界直到对应 evidence Gate 通过

### Requirement: M36 — 异步 NIC 分层架构

异步高性能网卡 MUST 将硬中断、硬件队列服务、协议栈轮询和 socket readiness 作为分离的执行层。硬中断只处理 cause/ack/mask/wake；descriptor reap/refill 由有 budget 的 queue task 完成；smoltcp poll 由 task 上下文中的 stack runner 完成。不引入 Embassy executor。

**Legacy**: ADR-063 (A063), 2026-07-18 | **状态**: 候选

#### Scenario: 规划首个异步 NIC change

- **WHEN** 创建首个 StarryOS 异步 NIC change
- **THEN** MUST 保留 axnet-ng、smoltcp、axpoll 和 axtask 用于初始 MVP
- **AND** 硬中断工作 MUST 限定在 cause、ack/mask、snapshot 和 wake
- **AND** descriptor service 和 protocol-stack polling MUST 在硬中断上下文之外运行

### Requirement: M37 — PLIC/Clock trust-u-boot

VisionFive2 bring-up MUST 保留 U-Boot 配置的 PLIC 和 Clock 状态，除非诊断证明保留的状态无效。UART 寄存器初始化仍然允许（NS16550 寄存器重写无害）。范围收紧为 PLIC + Clock，不包含 UART。

**Legacy**: ADR-040 (A040), 2026-06-26 | **状态**: 🟡 Proposed

#### Scenario: VisionFive2 bring-up 保留 bootloader 状态

- **WHEN** StarryOS 通过 U-Boot 在 VisionFive2 上启动
- **THEN** PLIC 和 Clock setup MUST 遵循 trust-u-boot 策略，除非 Q18 诊断证明保留状态无效
- **AND** UART 寄存器初始化 MAY 仍可为 async driver 重配 FCR、IER 和 baud rate

### Requirement: M38 — PLIC 防御性设计

PLIC 初始化 MUST 保持 `init_primary()`（全局一次性初始化）与 `init_percpu()`（per-hart 配置）显式分离。`init_percpu()` MUST NOT 执行一次性 PLIC 构造或调用 `init_once()`。

**Legacy**: ADR-041 (A041), 2026-06-26 | **状态**: 🟡 防御性保留

#### Scenario: PLIC 初始化审查

- **WHEN** StarryOS 切换或更新 VisionFive2 平台 crate
- **THEN** PLIC 初始化路径 MUST 保持全局一次性初始化与 per-hart 初始化分离
- **AND** `init_percpu()` MUST NOT 调用 `init_once()` 或等效一次性 PLIC 构造

### Requirement: M39 — SMP 原子内存序按语义选择

跨 hart 共享的 async UART 状态 MUST 按同步角色使用 Rust 原子内存序，禁止按架构分叉。纯 telemetry 保持 Relaxed；发布/观察状态用 Release/Acquire；参与同步判断的 RMW 用 AcqRel；ier_cache 非原子 RMW 必须通过锁内 RMW 修复。

**Legacy**: ADR-042 (A042), 2026-06-27 | **状态**: ✅ QEMU 完成 / ⚠️ multi-hart 待验证

#### Scenario: Q17 atomic ordering review

- **WHEN** Q17 修改 async UART 原子字段
- **THEN** 所选内存序 MUST 根据字段角色说明理由
- **AND** MUST NOT 引入按架构分叉的内存序分支

<!-- arc: ARC-202607251326 --> 27 M 条目已归档 (2026-07-25) -> openspec/changes/ARC-202607251326/proposal.md
<!-- arc: MIG-20260720-legacy-specs --> Legacy: openspec/specs/architecture/spec.md (hash: 5b054d98), 1053 lines, ADR-001~063. Current valid constraints extracted as M01-M40. Decisions rationale preserved in decisions/spec.md. Tombstoned ADRs (A014-A017, A020-A021, A032, A063-A064) noted here — details in archive carriers ARC-202607081429 and arc-202607152005. Also see ARC-202607251326 for M02-M40 partial archival.
