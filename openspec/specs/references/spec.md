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
| <!-- R14 --> `.claude/analysis/arceos-true-board-validation.md` | ArceOS / VisionFive2 真板验证案例：启动链先可观测、平台事实来自真板日志、寄存器可访问性优先、bootloader 状态 dump/preserve、中断 claim/handler/status/EOI 分层；不代表当前目标板选择 |
| <!-- R23 --> `.claude/analysis/async-network-project-overview.md` | StarryOS 网络开发总览：当前 VirtIO-MMIO 基线、目标数据流、依赖边界、QEMU→目标板验证阶梯、ArceOS 分级价值和专题来源 |
| <!-- R24 --> `.claude/analysis/embassy-network-module-evaluation.md` | Embassy 网络模块评估：核对 12 个网络相关 crate/模块，归纳 8 类可用能力和 3 类近期采用候选，明确 executor/time 的本地适配边界 |
| <!-- R25 --> `.claude/analysis/arceos-async-network-driver-analysis.md` | ArceOS 网卡工作可复用性：区分 QEMU 直接代码、transport-neutral 抽象审查和目标真板经验；DWMAC 仅在兼容控制器上进入移植候选 |
| <!-- R26 --> `.claude/analysis/starryos-async-network-roadmap.md` | StarryOS 异步网卡架构路线：RX/TX ownership、IRQ budget、背压、completion、可观测性，以及 QEMU 基线后按目标板事实选择后端 |
| <!-- R41 --> `.claude/analysis/starryos-network-development-strategy.md` | StarryOS 网络开发实施探索：当前 axnet/smoltcp/VirtIO-MMIO 调用链、异步 queue/stack 数据流、MS04 边界和目标板 B0-B7 条件化 Gate |
| <!-- R42 --> `.claude/analysis/_archive/starryos-network-delivery-estimate.md` | [ARCHIVED 2026-08-09] 旧 T01-T13、PCI-first 和 VF2/DWMAC 固定路线的人周假设；目标板路线不得沿用其数字 |
| <!-- R43 --> `.claude/analysis/_archive/starryos-network-knowledge-gaps.md` | [ARCHIVED 2026-08-09] 旧 T01-T13、PCI-first 和 VF2/DWMAC 分组；当前 Plan 读取 tasks、R23、R25 和 R41 |
| <!-- R46 --> `.claude/analysis/starryos-device-specific-irq-waker-architecture.md` | StarryOS 设备专属 IRQ 与任务唤醒分析：UART 全局 hook 冲突、PLIC 设备 handler、设备私有 waker、MS03/MS04 分批边界和 Gate 2 未确认项 |
| <!-- R47 --> `.claude/analysis/starryos-virtio-mmio-network-benchmark-baseline.md` | MS16 统一网卡基线设计：QEMU/TAP 轮询 B0、跨轮询/异步/真板 workload、C1-C6 完成语义、吞吐/延迟/指令/CPU/复制/IRQ 指标、Evidence Schema、BDD/Gate 和 MS04 A/B 比较资格 |
| <!-- R53 --> `.claude/analysis/sdmmc-async-driver-external-reference.md` | xianxw/Final-NO-SDMMC 固定 commit 的异步 SDMMC 参考：W1C cause 保留、阶段化超时与终态验证、单请求 DMA fail-stop、同步/异步共用完成谓词，以及 MS07/MS10/MS11/MS13 的迁移边界；不扩张 MS05 |

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
| <!-- R44 --> | `.claude/runbooks/qemu-network-testing.md` | QEMU 网络测试 Runbook — QEMU guest 操作保持手工执行；定义 sandbox `ENV-BLOCKED` 与产品失败的分类、iteration 末尾手工交接、HTTP 下载流程和证据要求 |
| <!-- R45 --> | `.claude/runbooks/ms02-virtio-mmio-evidence.md` | MS02 VirtIO-MMIO 证据采集 Runbook — axnet 策略测试 + agent 静态验证 + QEMU 手工验证（无 hostfwd / user-net TCP+UDP / TAP ARP+ICMP / 空闲 CPU / MS01 runtime）完整流程与失败处理 |
| <!-- R48 --> | `.claude/runbooks/ms03-virtio-mmio-irq-evidence.md` | MS03 VirtIO-MMIO 可诊断中断基线证据采集 Runbook — 启动签名、guest C probe（5 modes）、MS02/MS01 回归、中断诊断排障（32-bit MMIO 寄存器、device_id 校验、port conflict） |
| <!-- R49 --> | `.claude/runbooks/network-benchmark-platform-qualification.md` | 网卡基准资格扫描 Runbook — 环境/treatment/test 分轴、C1/C6 口径、user-net 已验证路径、TAP 手工命令、多流/payload/profile/pacing 矩阵、可观测性、Evidence 和基础设施缺口分类 |
| <!-- R50 --> | `.claude/runbooks/git-stash-bisect.md` | Git Stash 二分排查 Runbook — 大改动构建失败时用 stash 分块隔离判定"改动是否引入失败"；含 cargo clean 防缓存污染、基线/二分/用户交叉验证、untracked 文件与恢复完整性与回滚 |
| <!-- R51 --> | `.claude/runbooks/ms04-qemu-async-rx-core-evidence.md` | MS04 QEMU 异步 RX 核心证据采集 Runbook — 唯一 queue task、quiet/nudge、96 包有界 burst、descriptor 守恒、budget/yield、证据边界与失败处理 |
| <!-- R52 --> | `.claude/runbooks/virtio-real-adapter-test-fixture.md` | 真实 adapter 测试 fixture Runbook — 为 virtio-drivers 依赖 crate 编写驱动真实 `VirtIoNetDev` 的测试：本地 TestHal/fake Transport + used-ring 设备模拟、依赖 crate 中 `cfg(test)` seam 不可见时的访问器/seam 处理、post-accept invariant 与 QueueFull 的驱动边界 |
| <!-- R54 --> | `.claude/runbooks/ms05-automatic-gate-manifest.md` | MS05 自动 Gate manifest 管线 Runbook — `ms05_evidence_capture.py --run automatic`（literal argv、100× child records、source freeze、artifact records）+ `ms05_evidence_audit.py --write-qualification/--verify-qualification`（14 个 exact-code fixtures、D1/R44 分类、qualification 绑定）生成机器可审计的资格 Evidence；含 artifact mtime drift 与 source-freeze 失败处理 |

<!-- arc: cleanup-uart-documentation-system --> 全部历史 R 条目已归档至 archive carrier（见上方已归档条目）。
