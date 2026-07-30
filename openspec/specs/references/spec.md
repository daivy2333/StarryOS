# Spec: references — 外部参考与依赖索引

## Purpose

汇总 StarryOS 当前使用的外部依赖、平台规范、异步生态参考和项目内部文档索引。历史 UART 参考只保留归档入口。每条 MUST 可被 `grep` 精确定位。

## Requirements

### Requirement: 核心 Rust 依赖与构建工具

项目核心依赖版本 MUST 与本规范一致；新增 / 升级依赖 MUST 同步更新版本记录。

| 依赖 | 版本 | 链接 | 备注 |
|------|------|------|------|
| `embassy-sync` | v0.6.2 | [crates.io](https://crates.io/crates/embassy-sync) | 已验证与 nightly-2026-02-25 兼容 ✅ |
| `ringbuf` | 0.4.8 | [crates.io](https://crates.io/crates/ringbuf) | 无锁环形缓冲区 |
| `axtask` | 0.3.0-preview.2 | 项目内部 crate | 异步任务调度器 |
| `axpoll` | 0.1.2 | 项目内部 crate | 轮询/事件通知 |
<!-- arc: cleanup-uart-documentation-system --> `uart_16550` dep entry archived 2026-07-25. Crate remains active at `crates/uart_16550/`.

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
- **THEN** MUST 按 `quality-gate-baseline` 的 ENV BLOCK 规则报告缺失前置条件，禁止修改项目代码绕过

### Requirement: 硬件与平台规范

调试或新增平台支持时 MUST 先查阅对应规范。

| 规范 | 链接 | 用途 |
|------|------|------|
| [RISC-V PLIC Specification](https://github.com/riscv/riscv-plic-spec) | riscv 官方 | 中断控制器编程 |

<!-- arc: cleanup-uart-documentation-system --> NS16550A UART and VirtIO Console hardware specs archived 2026-07-25 → archive carrier.

#### Scenario: 调试串口寄存器行为

- **WHEN** 开发者发现串口状态异常
- **THEN** 优先查对应 datasheet；已归档的 NS16550A 规范可供参考

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
- **THEN** MUST 按 K09 拒绝第二套 executor；改用 `axtask::future + AtomicWaker` 模式

### Requirement: Rust 异步与系统编程参考

Rust 异步核心机制（async/await、Pin、UnsafeCell）MUST 查官方文档而非第三方总结；新代码使用自引用结构时 MUST 谨慎评估 `Pin` / `Unpin`。

| 资源 | 链接 | 用途 |
|------|------|------|
| [Rust Async Book](https://rust-lang.github.io/async-book/) | 官方 | async/await 原理 |
| [Pin and Unpin](https://doc.rust-lang.org/std/pin/index.html) | 官方 | 自引用结构安全 |

#### Scenario: 使用 Pin 或自引用结构

- **WHEN** 开发者要使用 `Pin<&mut Self>` 或自引用结构
- **THEN** MUST 查 Rust Async Book 与 Pin 文档，理解 `Unpin` 边界条件

<!-- arc: cleanup-uart-documentation-system --> Linux serial/8250 driver references archived 2026-07-25.

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

`.claude/analysis/` 的分析文档 MUST 在此登记。UART 阶段分析已归档（见下方已归档条目）。

| 文档 | 主题 |
|------|------|
| <!-- R14 --> `.claude/analysis/arceos-true-board-validation.md` | ArceOS / VisionFive2 真板验证方法：启动链先可观测、平台事实来自真板日志、寄存器可访问性优先、U-Boot 状态 dump/preserve、中断 claim/handler/status/EOI 分层 — 当前作为 N4 VisionFive2 DWMAC 真板参考 |
| <!-- R23 --> `.claude/analysis/async-network-project-overview.md` | StarryOS 网络开发总览：新 session 读取顺序、当前调用链、目标数据流、依赖边界、QEMU→VF2 验证阶梯、工作量摘要和专题来源 |
| <!-- R24 --> `.claude/analysis/embassy-network-module-evaluation.md` | Embassy 网络模块评估：核对 12 个网络相关 crate/模块，归纳 8 类可用能力和 3 类近期采用候选，明确 executor/time 的本地适配边界 |
| <!-- R25 --> `.claude/analysis/arceos-async-network-driver-analysis.md` | ArceOS 异步网卡分析：NetDriverOps、NetBuf、smoltcp adapter、DWMAC、axdma 与真板证据；识别硬中断全栈 poll、lost wakeup 和全局锁风险 |
| <!-- R26 --> `.claude/analysis/starryos-async-network-roadmap.md` | StarryOS 异步高性能网卡初步路线：分层架构、RX/TX descriptor 状态机、IRQ budget、背压、completion、可观测性和分阶段 Gate |
| <!-- R41 --> `.claude/analysis/starryos-network-development-strategy.md` | StarryOS 网络开发实施探索：当前 axnet/smoltcp/VirtIO 调用链、本地 smoltcp 0.13.1 兼容缺口、Embassy 采用边界、异步 queue/stack 数据流、QEMU PCI→MMIO→VisionFive2 分阶段 Gate |
| <!-- R42 --> `.claude/analysis/starryos-network-delivery-estimate.md` | StarryOS 网络交付与工期估算：T01-T13 人周、单人和双人日历周期、估算假设、进度风险与阶段复估点 |
| <!-- R43 --> `.claude/analysis/starryos-network-knowledge-gaps.md` | NIC 开发待收集信息清单：30 个待决问题按 milestone (T01-T13) 分组，每项含已知事实、待读代码、测试见证、解决判据和结果落点 — 进入 Plan 前逐项调查 |
| <!-- R46 --> `.claude/analysis/starryos-device-specific-irq-waker-architecture.md` | StarryOS 设备专属 IRQ 与任务唤醒分析：UART 全局 hook 冲突、PLIC 设备 handler、设备私有 waker、MS03/MS04 分批边界和 Gate 2 未确认项 |

**已归档**：UART 阶段全部分析文档。完整归档载体见 `openspec/changes/archive/2026-07-25-cleanup-uart-docs/`（48 文件，含 analysis、docs、meta-specs、runbooks、specs）、q17: `openspec/changes/archive/2026-07-25-q17-smp-memory-ordering/`、旧 ARC: `openspec/changes/archive/2026-07-25-arc-202607251326/`。

#### Scenario: 新生成 openspec-explorer 分析文档

- **WHEN** `openspec-explorer` 生成新的项目分析文档（写入 `.claude/analysis/`）
- **THEN** MUST 在本规范中注册：主题 / 路径 / 内容概要

---

## 子项目索引

| 条目 | 路径 | 摘要 |
|------|------|------|
| <!-- R38 --> | `.claude/runbooks/incremental-merge.md` | 增量融合 Runbook — 多 commit 合入的依赖排序、逐步 apply、Gate 与退化处理 |
| <!-- R39 --> | `.claude/runbooks/regression-gate.md` | 回归验证 Gate Runbook — Phase/change 收尾标准五层验证链与 ENV BLOCK 处理 |
| <!-- R40 --> | `.claude/runbooks/board-bringup-ladder.md` | 真板 bring-up 阶梯 Runbook — 新板 L0-L7 逐层适配、每层单变量约束与 Gate |
| <!-- R44 --> | `.claude/runbooks/qemu-network-testing.md` | QEMU 网络测试 Runbook — 硬性政策：全部 QEMU 测试手动执行（三重证据：OS shell 阻塞 + sandbox EPERM + 串口分帧不可靠）；HTTP 下载法流程与排障 |
| <!-- R45 --> | `.claude/runbooks/ms02-virtio-mmio-evidence.md` | MS02 VirtIO-MMIO 证据采集 Runbook — axnet 策略测试 + agent 静态验证 + QEMU 手工验证（无 hostfwd / user-net TCP+UDP / TAP ARP+ICMP / 空闲 CPU / MS01 runtime）完整流程与失败处理 |

<!-- arc: cleanup-uart-documentation-system --> 全部历史 R 条目已归档至 archive carrier（见上方已归档条目）。
