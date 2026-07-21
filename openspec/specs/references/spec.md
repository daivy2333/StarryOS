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
| <!-- R15 --> `.claude/analysis/uart-async-qemu-d1-first-replan.md` | UART async milestone 重排分析：将 QEMU/D1 可完成的 latency+jitter+CPU/RX 补测、用户态 completion queue、mmap ring/zero-copy 与性能决策前移；2026-07-13 起 Q21/Q22/Q23 当前排期由 ADR-058 取代，仅保留为历史输入 |
| <!-- R16 --> `.claude/analysis/q20-benchmark-gap-closure.md` | Q20 benchmark gap closure：现有 S10/S14/S20/S21/S31、TX debug ioctl、counter 输出和 raw evidence 缺口分析；为后续 propose/apply 提供任务拆分与 gate |
| <!-- R17 --> `.claude/analysis/clippy-test-baseline-cleanup.md` | Clippy 与测试基线清理：feature-scoped import、内嵌 uart manifest 漂移、workspace lint 泄漏、kernel host-test 边界、复现矩阵与分阶段 Gate |
| <!-- R18 --> `.claude/analysis/async-uart-vs-io_uring.md` | StarryOS 异步串口与 Linux io_uring 的设计异同：任务模型/批处理/ISR 极简等同构点；mmap/syscall/SQE-CQE 等差异来自当前 UART/VFS 架构取舍；按价值排序的可借鉴方向（backpressure 缺失、MPSC 隐患、TxCompletion drain snapshot、fixed buffer 精神已存在） |
| <!-- R19 --> `.claude/analysis/uart-backpressure-mpsc-plan.md` | UART backpressure 与 writer 并发边界分析：阻塞 fd writable wait、非阻塞 partial/WouldBlock、Tty poll/register 改造、`AsyncUartWriter::Clone` 与 SPSC 安全契约收敛、MPSC 后置条件 |
| <!-- R20 --> `docs/d1_out.md` | Q27 D1 真板 raw evidence：完整启动与 benchmark 输出；用于对照 Q20 同板 baseline，证明阻塞 TX backpressure 消除 S11 short write，关键吞吐与 p50 延迟无退化，slow-poll/yield fallback 未耗尽 |
| <!-- R23 --> `.claude/analysis/async-network-project-overview.md` | StarryOS 异步高性能网卡探索总览：UART 经验迁移、Embassy 模块数量、ArceOS 借鉴边界、目标架构、风险与 N0-N5 路线 |
| <!-- R24 --> `.claude/analysis/embassy-network-module-evaluation.md` | Embassy 网络模块评估：核对 12 个网络相关 crate/模块，归纳 8 类可用能力和 3 类近期采用候选，明确 executor/time 的本地适配边界 |
| <!-- R25 --> `.claude/analysis/arceos-async-network-driver-analysis.md` | ArceOS 异步网卡分析：NetDriverOps、NetBuf、smoltcp adapter、DWMAC、axdma 与真板证据；识别硬中断全栈 poll、lost wakeup 和全局锁风险 |
| <!-- R26 --> `.claude/analysis/starryos-async-network-roadmap.md` | StarryOS 异步高性能网卡初步路线：分层架构、RX/TX descriptor 状态机、IRQ budget、背压、completion、可观测性和分阶段 Gate |
| <!-- R27 --> `.claude/analysis/console-lichee-baseline-branch.md` | Console 性能基线分支分析：从冻结的当前异步提交选择性适配 polling Console，界定 TTY、生命周期、TEMT drain、benchmark 语义和 QEMU/D1 对照 Gate |
| <!-- R42 --> `.claude/analysis/console-performance-measurement-design.md` | I11/I12 Console 性能与测量设计：TX/RX 调用链、CPU/idle/内存口径、IRQ-off、延迟与抖动、线端完整性、QEMU/D1 对照矩阵和分阶段 Gate |

**已归档**（`.claude/analysis/_archive/`）：13 份一次性分析文档已于 2026-06-23 归档。核心经验已提取至 learned/architecture/optimization spec 中。

#### Scenario: 新生成 openspec-explorer 分析文档

- **WHEN** `openspec-explorer` 生成新的项目分析文档（写入 `.claude/analysis/`）
- **THEN** MUST 在本规范中注册：主题 / 路径 / 内容概要

---

## 子项目索引

<!-- 由 openspec-liaison 写入，由 openspec-assistant 日常维护，由 openspec-archivist 周期清理。 -->
<!-- 添加时格式: <!-- R{编号} --> | 子项目 | 路径 | 文档体系 | 摘要 | 最近更新 | -->

<!-- R1 --> | `uart_16550` | `../uart_16550` | OpenSpec✓ config✓ specs✓ changes✗ cg✓ | v0.6.0 path 依赖；4-domain OpenSpec 已建，changes 仅 archive，CodeGraph 729KB；旧 `.claude/docs/` 保留 SNAPSHOT/tasks 和 4 份 `.bak`。 | 2026-06-03 |

<!-- R28 --> | async UART API 路径速查 | `uart_16550/src/os/mod.rs`（OsRuntime + OsWakerSet 2-trait）、`uart_16550/src/async_/`（isr / ring_buffer / driver / device_ops）、`kernel/src/drivers/os_arceos.rs`（ArceOS adapter）、`kernel/src/drivers/d1_uart.rs`（D1 ArceOsD1UartPort） | 异步栈核心 API 定位 |
| <!-- R29 --> | 构建与部署环境 | musl 工具链 `/opt/musl/riscv64-linux-musl-cross/bin`；rootfs `rootfs-riscv64.img.xz`（GitHub releases）；disk.img 位置项目根 + `make/disk.img` | 构建前环境验证 |
| <!-- R30 --> | D1 平台关键事实与编译烧录流程 | **编译**: `make lichee-userbench`（userbench 模式）、`make lichee-kbench`（内核 benchmark）、`make lichee-fullbench-command`（fullbench command-entry）。产物: `starry-lichee-*-boot.img`。**构建 Gate**: `DWARF=n`（否则 image 超 10M 分区限制）、`BUS=mmio`（D1 无 PCI）、通过 `MYPLAT`/`PLAT_CONFIG` 选择本地 `axplat-riscv64-lichee-d1`。**烧录**: ① 将 .img 拷到 TF 卡 exUDISK 分区；② D1 官方 Linux 中 `dd if=/dev/by-name/boot of=/mnt/exUDISK/boot-official-backup.img bs=1M`（**先备份**）；③ `dd if=/mnt/exUDISK/starry-lichee-*.img of=/dev/by-name/boot bs=1M conv=fsync`（烧录）；④ `sync && reboot -f`。**恢复**: `dd if=/mnt/exUDISK/boot-official-backup.img of=/dev/by-name/boot bs=1M conv=fsync && reboot -f`。注意 `/dev/mmcblk0p4` 不可直接用于 dd，by-name 路径才是稳定接口。**平台事实**: D1/C906 单核 Sv39、RAM `0x40000000+512MiB`、UART0 `0x02500000` IRQ 18、Android boot image `kernel_addr=0x40200000` magic `ANDROID!` name `d1-nezha` page_size `2048`、OpenSBI v0.6 + U-Boot 2018.05。 | Lichee RV Dock bring-up 完整基线 |
| <!-- R31 --> | D1 构建与 feature gate | **三模式并行验证**: `cargo check --features lichee-d1 --target riscv64gc-unknown-none-elf`（smoke）、`cargo check --features lichee-d1-kbench --target riscv64gc-unknown-none-elf`（kbench）、`cargo check --features qemu --target riscv64gc-unknown-none-elf`（QEMU）。三模式全部通过 `cargo check` + `cargo clippy` 后才能声明 Phase 完成。**构建参数**: `MYPLAT`/`PLAT_CONFIG` 选择本地 `axplat-riscv64-lichee-d1`、`DWARF=n`、linker base `0xffffffc040200000`、`BUS=mmio` 必需（D1 无 PCI）、硬件能力 feature 与运行模式 feature 分离、axfs-ng 本地 patch（`default-features=false, features=["block","bus-mmio"]`） | D1 构建验证 |
| <!-- R32 --> | D1 async UART 行为数据 | D1 THRE/no-pending（IIR 常 0xc1）、tcdrain 注册 DRAIN_WAKER、Q19B 基线（256B 11.25KB/s）、Q19C 纠正（64B 93-97% 线速）、slow-poll forward progress 未丢 | D1 async UART 性能基准 |
| <!-- R33 --> | io_uring 映射与 NIC 迁移 | R18 `.claude/analysis/async-uart-vs-io_uring.md`、R23-R26 异步网卡分析、Embassy driver-channel packet slot 模式、ArceOS DWMAC/axdma 硬件参考 | 架构借鉴与 NIC 路线 |
| <!-- R34 --> | Q26 维护性清理记录 | memtrack 三态 session + `axalloc::tracking` API、ProcessMode::Manual 删除教训、`docs/d1_out.md` D1 command-entry benchmark evidence | 维护性清理参考 |
| <!-- R35 --> | QEMU 构建与运行 Runbook | `.claude/runbooks/qemu-build.md` | QEMU riscv64-virt 编译、运行、benchmark 与失败处理 |
| <!-- R36 --> | D1 真板构建与烧录 Runbook | `.claude/runbooks/d1-build-and-flash.md` | Lichee RV Dock/D1 编译、Android boot image 打包、烧录、恢复与失败处理 |
| <!-- R37 --> | 基准测试 Runbook | `.claude/runbooks/benchmark-guide.md` | S 系列测试运行方式、结果判读、QEMU/D1 可信度边界与通过条件 |
| <!-- R38 --> | 增量融合 Runbook | `.claude/runbooks/incremental-merge.md` | 多 commit 合入的依赖排序、逐步 apply、Gate 与退化处理 |
| <!-- R39 --> | 回归验证 Gate Runbook | `.claude/runbooks/regression-gate.md` | Phase/change 收尾标准五层验证链与 ENV BLOCK 处理 |
| <!-- R40 --> | 真板 bring-up 阶梯 Runbook | `.claude/runbooks/board-bringup-ladder.md` | 新板 L0-L7 逐层适配、每层单变量约束与 Gate |
| <!-- R41 --> | Console benchmark QEMU/D1 部署 Runbook | `.claude/runbooks/console-benchmark-qemu-d1.md` | 当前分支 musl payload 构建、QEMU rootfs 注入、D1 TF 卡复制与 boot 备份/烧录、串口取证和恢复 |
| <!-- arc: MIG-20260720-legacy-specs --> Learned reference entries merged: Legacy `openspec/specs/learned/spec.md` (hash: f09d4cae) → new R28-R34. |
<!-- arc: ARC-202607021648 --> 1 组 references 条目已归档/压缩 (2026-07-02) → ../changes/archive/2026-07-02-ARC-202607021648/proposal.md
