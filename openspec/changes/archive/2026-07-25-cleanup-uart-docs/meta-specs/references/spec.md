# Spec: references — 外部参考与依赖索引

## Purpose

汇总 StarryOS 异步串口项目的所有外部依赖（crates / 工具链 / 镜像）、规范文档（NS16550A / RISC-V PLIC / VirtIO）、生态参考（Embassy / Linux serial）以及项目内部分析文档的索引。每条 MUST 可被 `grep` 精确定位。

## Requirements

### Requirement: 核心 Rust 依赖与构建工具

项目核心依赖版本 MUST 与本规范一致；新增 / 升级依赖 MUST 同步更新版本记录。

| 依赖 | 版本 | 链接 | 备注 |
|------|------|------|------|
| `embassy-sync` | v0.6.2 | [crates.io](https://crates.io/crates/embassy-sync) | 已验证与 nightly-2026-02-25 兼容 ✅ |
| `ringbuf` | 0.4.8 | [crates.io](https://crates.io/crates/ringbuf) | 无锁环形缓冲区 |
| `axtask` | 0.3.0-preview.2 | 项目内部 crate | 异步任务调度器 |
| `axpoll` | 0.1.2 | 项目内部 crate | 轮询/事件通知 |
| `uart_16550` | 本地 path | `../../uart_16550` | 16550 UART 驱动库 ✅ |

**构建工具链**：

| 资源 | 位置 | 用途 |
|------|------|------|
| RISC-V musl 工具链 | `/opt/musl/riscv64-linux-musl-cross` | [setup-musl releases](https://github.com/arceos-org/setup-musl/releases/tag/prebuilt) 编译 lwext4_rust C 代码 ✅ |
| rootfs 镜像 | `rootfs-riscv64.img.xz` | [GitHub releases](https://github.com/Starry-OS/rootfs/releases/download/20260214/rootfs-riscv64.img.xz) QEMU 磁盘镜像（1GB）✅ |

**Rust 工具链**（来自 `rust-toolchain.toml`）：`nightly-2026-02-25`

#### Scenario: 新增 Rust 依赖

- **WHEN** 开发者要在 `Cargo.toml` 添加新依赖
- **THEN** MUST 在本规范中登记：依赖名 / 版本 / 来源链接 / 用途说明 / 与工具链兼容性

#### Scenario: 构建失败提示 musl 编译器找不到

- **WHEN** `make build` 报 `riscv64-linux-musl-cc: command not found`
- **THEN** MUST 按 `learned` spec 中的"构建与部署环境踩坑"操作，禁止修改项目代码绕过

### Requirement: 硬件与平台规范

UART / 中断控制器 / 虚拟化控制器的官方规范 MUST 在本规范登记链接，调试或新增平台支持时 MUST 先查阅对应规范。

| 规范 | 链接 | 用途 |
|------|------|------|
| [NS16550A UART Specification](https://www.ti.com/lit/ds/symlink/pc16550d.pdf) | TI 官方 PDF | 寄存器定义与时序 |
| [RISC-V PLIC Specification](https://github.com/riscv/riscv-plic-spec) | riscv 官方 | 中断控制器编程 |
| [VirtIO Console Specification](https://docs.oasis-open.org/virtio/virtio/v1.2/csd01/virtio-v1.2-csd01.html#x1-2900003) | OASIS | DMA 传输协议 |

#### Scenario: 调试 UART 寄存器行为

- **WHEN** 开发者发现 UART 状态异常（如 THR_EMPTY 含义不明、LSR 位差异）
- **THEN** MUST 优先查 NS16550A 规范 PDF，**禁止**只依赖 crate 注释（`learned` L80 教训：crate 注释曾有错误）

### Requirement: Embassy 生态参考

本项目仅使用 `embassy-sync::AtomicWaker` 子模块；扩展 Embassy 用法前 MUST 先评估是否冲突现有 `axtask` 调度器。

| 资源 | 链接 | 用途 |
|------|------|------|
| [Embassy Book](https://embassy.dev/book/) | 官方 | 异步运行时文档 |
| [embassy-sync AtomicWaker API](https://docs.embassy.dev/embassy-sync/git/default/struct.AtomicWaker.html) | 官方 | 中断安全唤醒（本项目核心依赖） |
| [Embassy GitHub](https://github.com/embassy-rs/embassy) | 官方 | 源码与 release 说明 |
| [embassy-executor v0.10.0](https://github.com/embassy-rs/embassy/releases) | 官方 | 执行器最新版（**不引入**，与 axtask 冲突） |
| [probe-rs 调试工具](https://probe.rs/) | 官方 | Embassy 推荐的调试/烧录工具链 |
| [defmt 日志框架](https://defmt.ferrous-systems.com/) | 官方 | Embassy 生态推荐的格式化日志 |

#### Scenario: 评估引入 embassy-executor

- **WHEN** 开发者想引入 embassy-executor 替换 axtask
- **THEN** MUST 拒绝（`learned` L10：embassy-executor 与 axtask 调度器冲突）；改用 `axtask::future + AtomicWaker` 模式

### Requirement: Rust 异步与系统编程参考

Rust 异步核心机制（async/await、Pin、UnsafeCell）MUST 查官方文档而非第三方总结；新代码使用自引用结构时 MUST 谨慎评估 `Pin` / `Unpin`。

| 资源 | 链接 | 用途 |
|------|------|------|
| [Rust Async Book](https://rust-lang.github.io/async-book/) | 官方 | async/await 原理 |
| [Pin and Unpin](https://doc.rust-lang.org/std/pin/index.html) | 官方 | 自引用结构安全 |

#### Scenario: 使用 Pin 或自引用结构

- **WHEN** 开发者要使用 `Pin<&mut Self>` 或自引用结构
- **THEN** MUST 查 Rust Async Book 与 Pin 文档，理解 `Unpin` 边界条件

### Requirement: Linux serial 驱动参考

Linux 8250 / serial_core.c MUST 作为异步串口行为正确性的对照参考，但 MUST 不直接照抄（API 模型不同）。

| 资源 | 链接 | 用途 |
|------|------|------|
| [Linux serial_core.c](https://github.com/torvalds/linux/blob/master/drivers/tty/serial/serial_core.c) | Linux 内核 | 串口驱动参考实现 |
| [Linux 8250 driver](https://github.com/torvalds/linux/blob/master/drivers/tty/serial/8250/8250_core.c) | Linux 内核 | NS16550 驱动参考 |

#### Scenario: 评估 uart 行为是否符合预期

- **WHEN** 开发者对 16550 行为是否符合标准有疑问（如 tcdrain 实现、NAPI 设计）
- **THEN** MUST 参考 Linux 8250 源码作为"已知正确"对照

### Requirement: 上游 crate 源码位置（crates.io 不可修改）

项目使用 axtask / axhal / axplat / axpoll 等上游 crate 作为不可修改的外部依赖；调试时 MUST 用本地 cargo registry 路径定位源码。

| Crate | 路径 | 用途 |
|-------|------|------|
| `axtask-0.3.0-preview.2` | `~/.cargo/registry/.../axtask-0.3.0-preview.2/src/` | block_on + poll_io + register_irq_waker 实现 |
| `axhal-0.3.0-preview.2` | `~/.cargo/registry/.../axhal-0.3.0-preview.2/src/` | register_irq_hook + irq_handler 分发 |
| `axplat-riscv64-qemu-virt-0.3.1-pre.6` | `~/.cargo/registry/.../axplat-riscv64-qemu-virt-0.3.1-pre.6/src/` | PLIC + MmioSerialPort + axconfig.toml |
| `axpoll` | `axpoll` crate | PollSet + IoEvents + Pollable trait |

#### Scenario: 调试上游 crate 行为

- **WHEN** 开发者想了解 axtask / axhal / axplat 内部行为（如 ISR 分发细节）
- **THEN** MUST 用 `find ~/.cargo/registry -name "<crate>-<version>" -type d` 定位本地源码，**禁止**在项目内复制或 fork

### Requirement: 项目内部分析与设计文档索引

`.claude/analysis/` 的分析文档 MUST 在此登记。**2026-06-11 迁移**：`docs/analysis/` 9 份迁入并融合为 6 份，删除已覆盖的 `uart-16550-crate-reuse.md`。

| 文档 | 主题 |
|------|------|
| <!-- R21 --> `[ARCHIVED 2026-07-04]` `.claude/analysis/_archive/2026-07-04-q19-lichee-analysis/architecture-overview.md` | 架构概览摘要：仓库结构、构建系统、启动链、任务/进程模型、中断框架；文件内含完整旧版恢复指针 |
| <!-- R22 --> `[ARCHIVED 2026-07-04]` `.claude/analysis/_archive/2026-07-04-q19-lichee-analysis/arceos-borrowable-experience.md` | ArceOS 借鉴经验分析：DMA、HAL trait、真板 bring-up、PLIC 与异步 UART 对照；配套 optimization/spec.md O64~O73 借鉴清单 |
| <!-- R2 --> `[ARCHIVED 2026-07-04]` `.claude/analysis/_archive/2026-07-04-q19-lichee-analysis/optimization-milestone-replan.md` | Q15 后优化项 milestone 重规划：将 Q6 过载项拆分为 Q16 文档收敛、Q17 SMP 内存序、Q18 真板观测、Q19 VisionFive2 验证、Q20 DMA/高波特率决策、Q21 维护性清理、Q22 远期预研池 |
| <!-- R3 --> `.claude/analysis/q17-smp-memory-ordering.md` | Q17 / O63 SMP 内存序实施前分析：`ier_cache` RMW 竞争、TX completion Release/Acquire 语义、无需按架构分叉的 Rust 原子模型依据、验证 Gate；2026-07-03 复核补充 D1 `UartPort` 边界与当前源码行号漂移 |
| <!-- R4 --> `[ARCHIVED 2026-07-04]` `.claude/analysis/_archive/2026-07-04-q19-lichee-analysis/lichee/public-platform-notes.md` | Lichee RV Dock 公开资料与真板采集对照：D1 UART/PLIC/timer/boot image/RAM/启动链事实基线 |
| <!-- R5 --> `[ARCHIVED 2026-07-04]` `.claude/analysis/_archive/2026-07-04-q19-lichee-analysis/lichee-rv-dock-adaptation-plan.md` | Lichee RV Dock 适配方案：方向、技术路线、milestone、风险与下一步工程清单 |
| <!-- R6 --> `[ARCHIVED 2026-07-04]` `.claude/analysis/_archive/2026-07-04-q19-lichee-analysis/platform-parameter-decoupling.md` | 平台参数解耦分析：QEMU 常量耦合点、axconfig/axplat 复用边界、platform descriptor 与 early console 分层方案 |
| <!-- R7 --> `[ARCHIVED 2026-07-04]` `.claude/analysis/_archive/2026-07-04-q19-lichee-analysis/d1-axplat-bringup-plan.md` | D1 正路径 axplat bring-up 方案：解释 U-Boot 已跳转但无 Starry 输出的根因，规划本地 `axplat-riscv64-lichee-d1`、链接/启动/MMIO console/build gate |
<!-- tombstone: R8/R9 --> Archived 2026-07-02 in ARC-202607021648 — Q19B plan/blockers 已完成，当前入口为 `lichee-d1-benchmark` spec、Q19B archived change 与 R10 Q19C。
| <!-- R10 --> `[ARCHIVED 2026-07-11]` `.claude/analysis/_archive/2026-07-11-q19c-d1-async-uart-closeout/q19c-lichee-full-starryos-benchmark.md` | Q19C benchmark：manifest cleanup、RX witness、D1 memory-root path；最终收敛为 D1 async UART 性能验证，shell/SDMMC/rootfs 不作为 gate |
| <!-- R11 --> `[ARCHIVED 2026-07-11]` `.claude/analysis/_archive/2026-07-11-q19c-d1-async-uart-closeout/q19c-d1-tx-optimization.md` | Q19C.8e TX/P99：slow-poll + yield 已验证 forward progress；P99 未改善，作为 O77/L275 known limitation，Q20 复验 |
| <!-- R12 --> `[ARCHIVED 2026-07-11]` `.claude/analysis/_archive/2026-07-11-q19c-d1-async-uart-closeout/q19c-m1-memory-root-path-loader.md` | Q19C-M1：memory-root `/bin/benchmark` 通过 `resolve/read` + eager ELF mapping；lazy COW SIGILL 另列 O80/L277 |
| <!-- R13 --> `[ARCHIVED 2026-07-11]` `.claude/analysis/_archive/2026-07-11-q19c-d1-async-uart-closeout/q19c-m2-m3-shell-sdmmc-probe.md` | Q19C-M2/M3：M2 equivalent command-entry 通过；M3/rootfs-probe、SDMMC/rootfs 取消当前规划 |
| <!-- R14 --> `.claude/analysis/arceos-true-board-validation.md` | ⚠️ STALE [2026-07-15] — 摘要仍写 Q20，实际继续作为 Q24 输入；ArceOS / 明扬 VisionFive2 真板验证方法：启动链先可观测、平台事实来自真板日志、寄存器可访问性优先、U-Boot 状态 dump/preserve、中断 claim/handler/status/EOI 分层 |
| <!-- R15 --> `[ARCHIVED 2026-07-22]` `.claude/analysis/_archive/uart-async-qemu-d1-first-replan.md` | UART async milestone 重排分析：将 QEMU/D1 可完成的 latency+jitter+CPU/RX 补测、用户态 completion queue、mmap ring/zero-copy 与性能决策前移；Q20 已完成、Q21/Q22 取消、Q23 决策完成，重排已全部执行 |
| <!-- R17 --> `[ARCHIVED 2026-07-22]` `.claude/analysis/_archive/clippy-test-baseline-cleanup.md` | Clippy 与测试基线清理：feature-scoped import、内嵌 uart manifest 漂移、workspace lint 泄漏、kernel host-test 边界、复现矩阵与分阶段 Gate；Q26 维护性清理已完成并归档 |
| <!-- R18 --> `[ARCHIVED 2026-07-22]` `.claude/analysis/_archive/async-uart-vs-io_uring.md` | StarryOS 异步串口与 Linux io_uring 的设计异同：任务模型/批处理/ISR 极简等同构点；mmap/syscall/SQE-CQE 等差异来自当前 UART/VFS 架构取舍；backpressure/MPSC/writer 契约已通过 Q27a/Q27/Q28/Q29 实现 |
| <!-- R19 --> `[ARCHIVED 2026-07-22]` `.claude/analysis/_archive/uart-backpressure-mpsc-plan.md` | UART backpressure 与 writer 并发边界分析：阻塞 fd writable wait、非阻塞 partial/WouldBlock、Tty poll/register 改造、`AsyncUartWriter::Clone` 与 SPSC 安全契约收敛；全部 Q27a/Q27/Q28/Q29 已完成并归档 |
| <!-- R23 --> `.claude/analysis/async-network-project-overview.md` | StarryOS 异步高性能网卡探索总览：UART 经验迁移、Embassy 模块数量、ArceOS 借鉴边界、目标架构、风险与 N0-N5 路线 |
| <!-- R24 --> `.claude/analysis/embassy-network-module-evaluation.md` | Embassy 网络模块评估：核对 12 个网络相关 crate/模块，归纳 8 类可用能力和 3 类近期采用候选，明确 executor/time 的本地适配边界 |
| <!-- R25 --> `.claude/analysis/arceos-async-network-driver-analysis.md` | ArceOS 异步网卡分析：NetDriverOps、NetBuf、smoltcp adapter、DWMAC、axdma 与真板证据；识别硬中断全栈 poll、lost wakeup 和全局锁风险 |
| <!-- R26 --> `.claude/analysis/starryos-async-network-roadmap.md` | StarryOS 异步高性能网卡初步路线：分层架构、RX/TX descriptor 状态机、IRQ budget、背压、completion、可观测性和分阶段 Gate |
| <!-- R27 --> `[ARCHIVED 2026-07-22]` `.claude/analysis/_archive/console-lichee-baseline-branch.md` | Console 性能基线分支分析：从冻结的当前异步提交选择性适配 polling Console，界定 TTY、生命周期、TEMT drain、benchmark 语义和 QEMU/D1 对照 Gate；Q31/Q32 CPU-efficiency 对照实验已完成并归档 |
| <!-- R42 --> `.claude/analysis/_archive/2026-07-21-console-performance-measurement-design.md` | [ARCHIVED 2026-07-21] I11/I12 Console 性能与测量设计（console 分支专属，`uart-lichee` 不适用） |
| <!-- R43 --> `.claude/analysis/async-uart-cpu-efficiency-metrics.md` | 异步 UART CPU 效率指标与测试落地：盘点现有 S00-S40 覆盖，定义 submit fraction、通信—计算重叠、instret/byte、分段 counter delta 与证据边界 |

**已归档**（`.claude/analysis/_archive/`）：13 份一次性分析文档已于 2026-06-23 归档。核心经验已提取至 learned/architecture/optimization spec 中。

#### Scenario: 新生成 openspec-explorer 分析文档

- **WHEN** `openspec-explorer` 生成新的项目分析文档（写入 `.claude/analysis/`）
- **THEN** MUST 在本规范中注册：主题 / 路径 / 内容概要

---

## 子项目索引

<!-- 由 openspec-liaison 写入，由 openspec-assistant 日常维护，由 openspec-archivist 周期清理。 -->
<!-- 添加时格式: <!-- R{编号} --> | 子项目 | 路径 | 文档体系 | 摘要 | 最近更新 | -->

<!-- R1 --> | `uart_16550` | `../uart_16550` | OpenSpec✓ config✓ specs✓ changes✗ cg✓ | v0.6.0 path 依赖；4-domain OpenSpec 已建，changes 仅 archive，CodeGraph 729KB；旧 `.claude/docs/` 保留 SNAPSHOT/tasks 和 4 份 `.bak`。 | 2026-06-03 |

| <!-- R33 --> | io_uring 映射与 NIC 迁移 | `[ARCHIVED]` R18 `.claude/analysis/_archive/async-uart-vs-io_uring.md`、R23-R26 异步网卡分析、Embassy driver-channel packet slot 模式、ArceOS DWMAC/axdma 硬件参考 | 架构借鉴与 NIC 路线 |
| <!-- R37 --> | 基准测试 Runbook | `.claude/runbooks/benchmark-guide.md` | S 系列测试运行方式、结果判读、QEMU/D1 可信度边界与通过条件 |
| <!-- R38 --> | 增量融合 Runbook | `.claude/runbooks/incremental-merge.md` | 多 commit 合入的依赖排序、逐步 apply、Gate 与退化处理 |
| <!-- R39 --> | 回归验证 Gate Runbook | `.claude/runbooks/regression-gate.md` | Phase/change 收尾标准五层验证链与 ENV BLOCK 处理 |
| <!-- R40 --> | 真板 bring-up 阶梯 Runbook | `.claude/runbooks/board-bringup-ladder.md` | 新板 L0-L7 逐层适配、每层单变量约束与 Gate |
| <!-- R42 --> | Q31 Async CPU-Efficiency Benchmark Spec | `openspec/specs/uart-cpu-efficiency-benchmark/spec.md`（7 reqs）。Evidence: `openspec/changes/archive/2026-07-22-q31-async-uart-cpu-efficiency-benchmark/evidence/async/`。Frozen logs: QEMU `a9ce8a34...`, D1 `50a2a876...`。Archived: `openspec/changes/archive/2026-07-22-q31-async-uart-cpu-efficiency-benchmark/` | Async CPU-efficiency measurement contract |
| <!-- R43 --> | Q32 Console CPU-Efficiency Benchmark Spec | `openspec/specs/console-cpu-efficiency-benchmark/spec.md`（10 reqs）。Evidence: `openspec/changes/archive/2026-07-22-q32-console-cpu-efficiency-benchmark/evidence/`。Frozen logs: QEMU `67b7bb02...`, D1 `b3f11fce...`。Archived: `openspec/changes/archive/2026-07-22-q32-console-cpu-efficiency-benchmark/` | Console CPU-efficiency measurement contract |
| <!-- R44 --> | `[ARCHIVED 2026-07-22]` Console Benchmark QEMU/D1 Runbook | `.claude/runbooks/_archive/console-benchmark-qemu-d1.md` | Console 分支 benchmark 构建、QEMU rootfs 注入、D1 烧录和验证；Q31/Q32 实验完成，async 版 `.claude/runbooks/benchmark-qemu-d1.md` 为权威部署流程 |
| <!-- R45 --> | Q31→Q32 Console Port Analysis | `.claude/analysis/q31-console-cpu-efficiency-port.md` | 移植范围、D1 time 修复、S43 hang 根因（IRQ stub）、Console/Async 差异 |
| <!-- R46 --> | Q32 Doc Sync Checklist | `docs/q32-console-cpu-efficiency-doc-sync.md` | cross-branch 同步清单、comparison 边界、归档前验证 |
| <!-- arc: MIG-20260720-legacy-specs --> Learned reference entries merged: Legacy `openspec/specs/learned/spec.md` (hash: f09d4cae) → new R28-R34. |
<!-- arc: ARC-202607021648 --> 1 组 references 条目已归档/压缩 (2026-07-02) → ../changes/archive/2026-07-02-ARC-202607021648/proposal.md
<!-- arc: ARC-202607251326 --> 11 R 条目已归档 (2026-07-25) -> openspec/changes/ARC-202607251326/proposal.md
