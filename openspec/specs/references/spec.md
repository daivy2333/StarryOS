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

### Requirement: uart_16550 子项目文档索引

`uart_16550` 是 StarryOS 串口子系统的底层驱动模块；其文档体系 MUST 按子项目独立维护，本规范 MUST 保持单一入口的指针。

**子项目文档体系路径**：

| 文档 | 路径 | 内容概要 |
|------|------|----------|
| 项目入口 | `../uart_16550/CLAUDE.md` | 项目概览、no_std 驱动库规范 |
| 状态快照 | `../uart_16550/.claude/docs/SNAPSHOT.md` | v0.6.0 状态、核心 API 速查 |
| 学习记忆 | `../uart_16550/.claude/docs/learned.md` | API 路径、寄存器速查、中断速查 |
| 架构决策 | `../uart_16550/.claude/docs/architecture.md` | Backend trait 设计、寄存器分层 |
| 编码规范 | `../uart_16550/.claude/docs/rules.md` | 三大规则 + Rust embedded 规范 |
| 外部参考 | `../uart_16550/.claude/docs/references.md` | 16550 规范、依赖文档 |
| 优化记录 | `../uart_16550/.claude/docs/optimization.md` | 自旋阻塞、批量 API、DMA API |

**关键定位**：

- **寄存器定义**：`../uart_16550/src/spec.rs` — 所有 bitflags + 常量 + InterruptType
- **后端抽象**：`../uart_16550/src/backend/mod.rs` — Backend trait (sealed)
- **RISC-V 使用**：`Uart16550<MmioBackend>` + `new_mmio(NonNull<u8>, stride)`
- **中断处理**：`isr().interrupt_type()` → `InterruptType` 枚举分发

#### Scenario: 查找 16550 寄存器或 API

- **WHEN** 开发者要查 16550 寄存器定义或 API 行为
- **THEN** MUST 按"本索引"先定位文档，再去 `../uart_16550/src/spec.rs` 确认源码

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

`docs/analysis/` 下的所有分析文档 MUST 在本规范登记；新增分析文档 MUST 同步注册索引条目。

**项目分析文档**（`docs/analysis/`）：

| 文档 | 主题 |
|------|------|
| `project-overview.md` | 项目概览：仓库结构、构建系统、依赖图 |
| `boot-init.md` | 启动流程：axruntime → mount → spawn init |
| `device-registration.md` | 设备注册：DeviceOps trait、Device struct、devfs |
| `tty-console-stack.md` | TTY/Console 栈：N_TTY、ldisc、termios |
| `async-io-framework.md` | 异步 I/O 框架：poll_io、PollSet、Pollable、register_irq_waker |
| `syscall-interface.md` | 系统调用接口：FileLike、FD_TABLE、poll/select/epoll |
| `task-process-model.md` | 任务与进程模型：Thread、ProcessData、AsThread |
| `async-uart-design-context.md` | 异步 UART 设计上下文：现有模式、目标架构、关键文件索引 |
| `serial-interfaces-overview.md` | 串口相关接口概览：Console/PTY/vsock 三种串口接口分析 |
| `serial-optimization-preview.md` | 串口优化预览：从同步阻塞到异步高性能的优化分析 |
| `project-knowledge-map.md` | 项目知识地图：宏内核 OS 分层架构、ArceOS 组件架构、cargo 依赖图 |

**UART 初始化与 IRQ 设计文档**：

| 文档 | 主题 |
|------|------|
| `uart-init-design.md` | UART 硬件初始化替代方案设计：uart_16550 API 分析、UART 硬件配置设计 ✅ |
| `earlycon-design.md` | earlycon 内核日志设计方案：polling TX 实现、UART 硬件独占机制、panic 安全机制 ✅ |
| `async-uart-device-registration.md` | AsyncUart 设备注册设计方案：DeviceOps trait 分析、Pollable trait 实现 ✅ |
| `irq-waker-mechanism-verification.md` | IRQ waker 机制验证方案：register_irq_waker 机制、ISR + AtomicWaker 分发设计 ✅ |

**实施历程与可行性评估**：

| 文档 | 主题 |
|------|------|
| `async-uart-implementation-history.md` | AsyncUart 异步串口实现历程：两分支探索历程、渐进式集成失败、完全剔除 Console 方案 ✅ |

**性能与测试分析**：

| 文档 | 主题 |
|------|------|
| `user-async-perf-analysis.md` | 用户态异步串口性能分析：三嵌套 block_on/poll_io、yield storm、Manual 模式问题、benchmark 缺陷、对比阻塞 Console |
| `nonblocking-mode-analysis.md` | 非阻塞模式 FIONBIO 分析：当前实现、nonblocking 标志未传播到 TTY 层、实现方案、测试用例 |
| `uart-benchmark-optimization.md` | 性能测试优化方案：CPU 占用测量、中断频率统计、测试方法改进 |
| `benchmark-report-async.md` | Async 异步串口性能测试报告：内核态和用户态测试结果、与 Console 对比 |

**Embargo**：

| 文档 | 主题 |
|------|------|
| `embassy.md` | Embassy 可用组件分析 |

#### Scenario: 新生成 openspec-explorer 分析文档

- **WHEN** `openspec-explorer` 生成新的项目分析文档
- **THEN** MUST 在本规范中注册：主题 / 路径 / 内容概要
