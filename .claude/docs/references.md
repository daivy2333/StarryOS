# references.md — 外部参考与依赖（汇总）

> 由 project-docs-assistant 维护，汇总分支整合。
> 条目格式: <!-- R{编号} --> 标记开头，支持 grep 精确定位。

---

## 依赖文档

<!-- R1 --> | embassy-sync | v0.6.2 | [crates.io](https://crates.io/crates/embassy-sync) | 已验证与 nightly-2026-02-25 兼容 ✅ |
<!-- R2 --> | ringbuf | 0.4.8 | [crates.io](https://crates.io/crates/ringbuf) | 无锁环形缓冲区 |
<!-- R3 --> | axtask | 0.3.0-preview.2 | 项目内部 | 异步任务调度器 |
<!-- R4 --> | axpoll | 0.1.2 | 项目内部 | 轮询/事件通知 |
<!-- R38 --> | uart_16550 | 本地 path | ../../uart_16550 | 16550 UART 驱动库 ✅ |
<!-- R39 --> | RISC-V musl 工具链 | /opt/musl/riscv64-linux-musl-cross | [setup-musl releases](https://github.com/arceos-org/setup-musl/releases/tag/prebuilt) | 编译 lwext4_rust C 代码 ✅ |
<!-- R40 --> | rootfs-riscv64.img.xz | 1GB | [GitHub releases](https://github.com/Starry-OS/rootfs/releases/download/20260214/rootfs-riscv64.img.xz) | QEMU 磁盘镜像 ✅ |

---

## uart_16550 文档体系

<!-- R5 --> uart_16550 是 StarryOS 串口子系统的底层驱动模块，其文档体系路径:

| 文档 | 路径 | 内容概要 |
|------|------|----------|
| 项目入口 | `../uart_16550/CLAUDE.md` | 项目概览、no_std 驱动库规范 |
| 状态快照 | `../uart_16550/.claude/docs/SNAPSHOT.md` | v0.6.0 状态、核心 API 速查 |
| 学习记忆 | `../uart_16550/.claude/docs/learned.md` | API 路径、寄存器速查、中断速查 |
| 架构决策 | `../uart_16550/.claude/docs/architecture.md` | Backend trait 设计、寄存器分层 |
| 编码规范 | `../uart_16550/.claude/docs/rules.md` | 三大规则 + Rust embedded 规范 |
| 外部参考 | `../uart_16550/.claude/docs/references.md` | 16550 规范、依赖文档 |
| 优化记录 | `../uart_16550/.claude/docs/optimization.md` | 自旋阻塞、批量 API、DMA API |

关键定位:
- **寄存器定义**: `../uart_16550/src/spec.rs` — 所有 bitflags + 常量 + InterruptType
- **后端抽象**: `../uart_16550/src/backend/mod.rs` — Backend trait (sealed)
- **RISC-V 使用**: `Uart16550<MmioBackend>` + `new_mmio(NonNull<u8>, stride)`
- **中断处理**: `isr().interrupt_type()` → `InterruptType` 枚举分发

---

## 领域知识笔记

### 规范

<!-- R11 --> [NS16550A UART Specification](https://www.ti.com/lit/ds/symlink/pc16550d.pdf) - 寄存器定义与时序
<!-- R12 --> [RISC-V PLIC Specification](https://github.com/riscv/riscv-plic-spec) - 中断控制器编程
<!-- R13 --> [VirtIO Console Specification](https://docs.oasis-open.org/virtio/virtio/v1.2/csd01/virtio-v1.2-csd01.html#x1-2900003) - DMA 传输协议

### Embassy

<!-- R14 --> [Embassy Book](https://embassy.dev/book/) - 异步运行时文档
<!-- R15 --> [embassy-sync AtomicWaker API](https://docs.embassy.dev/embassy-sync/git/default/struct.AtomicWaker.html) - 中断安全唤醒
<!-- R28 --> [Embassy GitHub](https://github.com/embassy-rs/embassy) - 源码与 release 说明
<!-- R29 --> [embassy-executor v0.10.0](https://github.com/embassy-rs/embassy/releases) - 执行器最新版
<!-- R30 --> [probe-rs 调试工具](https://probe.rs/) - Embassy 推荐的调试/烧录工具链
<!-- R31 --> [defmt 日志框架](https://defmt.ferrous-systems.com/) - Embassy 生态推荐的格式化日志

### Rust 异步

<!-- R16 --> [Rust Async Book](https://rust-lang.github.io/async-book/) - async/await 原理
<!-- R17 --> [Pin and Unpin](https://doc.rust-lang.org/std/pin/index.html) - 自引用结构安全

### Linux 参考

<!-- R18 --> [Linux serial_core.c](https://github.com/torvalds/linux/blob/master/drivers/tty/serial/serial_core.c) - 串口驱动参考实现
<!-- R19 --> [Linux 8250 driver](https://github.com/torvalds/linux/blob/master/drivers/tty/serial/8250/8250_core.c) - NS16550 驱动参考

### 上游 crate 源码（crates.io，不可修改）

<!-- R32 --> | axtask-0.3.0-preview.2 | ~/.cargo/registry/.../axtask-0.3.0-preview.2/src/ | block_on + poll_io + register_irq_waker 实现 |
<!-- R33 --> | axhal-0.3.0-preview.2 | ~/.cargo/registry/.../axhal-0.3.0-preview.2/src/ | register_irq_hook + irq_handler 分发 |
<!-- R34 --> | axplat-riscv64-qemu-virt-0.3.1-pre.6 | ~/.cargo/registry/.../axplat-riscv64-qemu-virt-0.3.1-pre.6/src/ | PLIC + MmioSerialPort + axconfig.toml |
<!-- R35 --> | axpoll | axpoll crate | PollSet + IoEvents + Pollable trait |

---

## 设计文档索引

### 项目分析文档

<!-- R20 --> | docs/analysis/project-overview.md | 项目概览：仓库结构、构建系统、依赖图 |
<!-- R21 --> | docs/analysis/boot-init.md | 启动流程：axruntime → mount → spawn init |
<!-- R22 --> | docs/analysis/device-registration.md | 设备注册：DeviceOps trait、Device struct、devfs |
<!-- R23 --> | docs/analysis/tty-console-stack.md | TTY/Console 栈：N_TTY、ldisc、termios |
<!-- R24 --> | docs/analysis/async-io-framework.md | 异步 I/O 框架：poll_io、PollSet、Pollable、register_irq_waker |
<!-- R25 --> | docs/analysis/syscall-interface.md | 系统调用接口：FileLike、FD_TABLE、poll/select/epoll |
<!-- R26 --> | docs/analysis/task-process-model.md | 任务与进程模型：Thread、ProcessData、AsThread |
<!-- R27 --> | docs/analysis/async-uart-design-context.md | 异步 UART 设计上下文：现有模式、目标架构、关键文件索引 |
<!-- R36 --> | docs/analysis/serial-interfaces-overview.md | 串口相关接口概览：Console/PTY/vsock 三种串口接口分析 |
<!-- R37 --> | docs/analysis/serial-optimization-preview.md | 串口优化预览：从同步阻塞到异步高性能的优化分析 |

### Console UART 研究文档

<!-- R41 --> | docs/analysis/console-uart-mechanism.md | Console UART 工作机制：硬件配置、TX 阻塞机制、RX 中断驱动、数据竞争风险分析 ✅ |
<!-- tombstone: R42,R48,R50 --> Archived 3 outdated analysis refs 2026-06-02 — 前提已变更或已归档

### UART 初始化与 IRQ 设计文档

<!-- R43 --> | docs/analysis/uart-init-design.md | UART 硬件初始化替代方案设计：uart_16550 API 分析、UART 硬件配置设计 ✅ |
<!-- R44 --> | docs/analysis/earlycon-design.md | earlycon 内核日志设计方案：polling TX 实现、UART 硬件独占机制、panic 安全机制 ✅ |
<!-- R45 --> | docs/analysis/async-uart-device-registration.md | AsyncUart 设备注册设计方案：DeviceOps trait 分析、Pollable trait 实现 ✅ |
<!-- R46 --> | docs/analysis/irq-waker-mechanism-verification.md | IRQ waker 机制验证方案：register_irq_waker 机制、ISR + AtomicWaker 分发设计 ✅ |

### 实施历程与可行性评估

<!-- R47 --> | docs/analysis/async-uart-implementation-history.md | AsyncUart 异步串口实现历程：两分支探索历程、渐进式集成失败、完全剔除 Console 方案 ✅ |
<!-- tombstone: R48 --> Archived 2026-06-02 — 部分过时，stride=4 已纠正
<!-- R49 --> | docs/analysis/project-knowledge-map.md | 项目知识地图：宏内核 OS 分层架构、ArceOS 组件架构、cargo 依赖图 ✅ |
<!-- tombstone: R50 --> Archived 2026-06-02 — 已归档周报
<!-- R51 --> | docs/benchmark-report-async.md | Async 异步串口性能测试报告：内核态和用户态测试结果、与 Console 对比 |
<!-- R52 --> | docs/analysis/user-async-perf-analysis.md | 用户态异步串口性能分析：三嵌套 block_on/poll_io、yield storm、Manual 模式问题、benchmark 缺陷、对比阻塞 Console |
<!-- R53 --> | docs/analysis/nonblocking-mode-analysis.md | 非阻塞模式 FIONBIO 分析：当前实现、nonblocking 标志未传播到 TTY 层、实现方案、测试用例 |
<!-- R54 --> | docs/analysis/uart-benchmark-optimization.md | 性能测试优化方案：CPU 占用测量、中断频率统计、测试方法改进 |

---

## Embargo 文档

<!-- R10 --> | docs/embassy.md | Embassy 可用组件分析 |
