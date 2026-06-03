# references/spec.md — 外部参考与依赖

> 迁移自 .claude/docs/references.md，2026-06-03
> 条目格式: R{编号} 标记开头，支持 grep 精确定位。

---

## Purpose

记录项目依赖和外部参考资源，确保依赖可追溯，资源可获取。

## Requirements

### Requirement: 依赖版本锁定

所有项目依赖 MUST 记录版本信息，确保构建可重现。

#### Scenario: 添加新依赖

- **WHEN** 开发者引入新的外部依赖
- **THEN** 必须记录到 references/spec.md，包含：依赖名称、版本、官方链接、用途说明

#### Scenario: 更新依赖版本

- **WHEN** 开发者升级或降级依赖版本
- **THEN** 必须更新 references/spec.md 中的版本记录，标注更新原因

**当前依赖**:

| 依赖 | 版本 | 来源 | 用途 |
|------|------|------|------|
| embassy-sync | v0.6.2 | crates.io | AtomicWaker ISR 安全唤醒 |
| ringbuf | 0.4.8 | crates.io | 无锁环形缓冲区 |
| axtask | 0.3.0-preview.2 | 项目内部 | 异步任务调度器 |
| axpoll | 0.1.2 | 项目内部 | 轮询/事件通知 |
| uart_16550 | 本地 path | ../../uart_16550 | 16550 UART 驱动库 |

### Requirement: 外部资源记录

项目使用的外部资源和文档 MUST 记录，方便查阅。

#### Scenario: 参考外部文档

- **WHEN** 开发者参考了重要的外部文档或资源
- **THEN** 必须记录到 references/spec.md，包含：资源名称、链接、关键内容摘要

**规范文档**:

| 文档 | 链接 | 内容 |
|------|------|------|
| NS16550A UART Specification | [ti.com](https://www.ti.com/lit/ds/symlink/pc16550d.pdf) | 寄存器定义与时序 |
| RISC-V PLIC Specification | [GitHub](https://github.com/riscv/riscv-plic-spec) | 中断控制器编程 |
| VirtIO Console Specification | [OASIS](https://docs.oasis-open.org/virtio/virtio/v1.2/csd01/virtio-v1.2-csd01.html) | DMA 传输协议 |

**Embassy 生态**:

| 文档 | 链接 | 内容 |
|------|------|------|
| Embassy Book | [embassy.dev](https://embassy.dev/book/) | 异步运行时文档 |
| embassy-sync AtomicWaker | [docs.embassy.dev](https://docs.embassy.dev/embassy-sync/git/default/struct.AtomicWaker.html) | 中断安全唤醒 |
| Embassy GitHub | [GitHub](https://github.com/embassy-rs/embassy) | 源码与 release 说明 |

**Linux 参考**:

| 文件 | 链接 | 内容 |
|------|------|------|
| serial_core.c | [GitHub](https://github.com/torvalds/linux/blob/master/drivers/tty/serial/serial_core.c) | 串口驱动参考实现 |
| 8250_core.c | [GitHub](https://github.com/torvalds/linux/blob/master/drivers/tty/serial/8250/8250_core.c) | NS16550 驱动参考 |

### Requirement: 子项目文档索引

uart_16550 子项目的文档体系 MUST 建立索引。

#### Scenario: 查阅 uart_16550 文档

- **WHEN** 开发者需要了解 uart_16550 的 API 或实现细节
- **THEN** 可以通过 references/spec.md 中的索引快速定位

**uart_16550 文档索引**:

| 文档 | 路径 | 内容 |
|------|------|------|
| 项目入口 | `../uart_16550/CLAUDE.md` | 项目概览、no_std 驱动库规范 |
| 状态快照 | `../uart_16550/.claude/docs/SNAPSHOT.md` | v0.6.0 状态、核心 API 速查 |
| 学习记忆 | `../uart_16550/.claude/docs/learned.md` | API 路径、寄存器速查、中断速查 |
| 寄存器定义 | `../uart_16550/src/spec.rs` | 所有 bitflags + 常量 + InterruptType |
| 后端抽象 | `../uart_16550/src/backend/mod.rs` | Backend trait (sealed) |

### Requirement: 项目分析文档索引

深度分析文档 MUST 建立索引，方便查找。

#### Scenario: 生成分析文档

- **WHEN** openspec-explorer 生成了项目分析文档
- **THEN** 必须在 references/spec.md 中注册索引条目，包含：主题、路径、内容概要

**分析文档索引**:

| 文档 | 路径 | 内容 |
|------|------|------|
| 项目概览 | `docs/analysis/project-overview.md` | 仓库结构、构建系统、依赖图 |
| 启动流程 | `docs/analysis/boot-init.md` | axruntime → mount → spawn init |
| 设备注册 | `docs/analysis/device-registration.md` | DeviceOps trait、Device struct、devfs |
| TTY/Console 栈 | `docs/analysis/tty-console-stack.md` | N_TTY、ldisc、termios |
| 异步 I/O 框架 | `docs/analysis/async-io-framework.md` | poll_io、PollSet、Pollable、register_irq_waker |
| 异步 UART 设计 | `docs/analysis/async-uart-design-context.md` | 现有模式、目标架构、关键文件索引 |
| 性能测试报告 | `docs/benchmark-report-async.md` | 内核态和用户态测试结果 |
| 用户态性能分析 | `docs/analysis/user-async-perf-analysis.md` | 三嵌套 block_on、yield storm、benchmark 缺陷 |
| 非阻塞模式分析 | `docs/analysis/nonblocking-mode-analysis.md` | FIONBIO 传播、实现方案 |

### Requirement: 构建环境配置

构建环境依赖 MUST 记录，确保可重现。

#### Scenario: 配置构建环境

- **WHEN** 开发者需要搭建构建环境
- **THEN** 可以参考 references/spec.md 中的环境配置说明

**环境依赖**:

| 依赖 | 路径/版本 | 用途 |
|------|-----------|------|
| RISC-V musl 工具链 | `/opt/musl/riscv64-linux-musl-cross/bin` | 编译 lwext4_rust C 代码 |
| rootfs-riscv64.img.xz | [GitHub releases](https://github.com/Starry-OS/rootfs/releases/download/20260214/rootfs-riscv64.img.xz) | QEMU 磁盘镜像 (1GB) |
| Rust nightly | nightly-2026-02-25 | 编译器版本 |
