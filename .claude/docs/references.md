# references.md — 外部参考与依赖

> 由 project-rules-generator 初始化，由 project-docs-assistant 日常维护。
> 条目格式: <!-- R{编号} --> 标记开头，支持 grep 精确定位。

---

## 依赖文档

<!-- 添加时格式: <!-- R{编号} --> | 依赖 | 版本 | 链接 | 用途 | -->

<!-- R1 --> | embassy-sync | git | [docs.embassy.dev](https://docs.embassy.dev/embassy-sync/git/default/struct.AtomicWaker.html) | AtomicWaker 中断安全唤醒 |
<!-- R2 --> | ringbuf | 0.4.8 | [crates.io](https://crates.io/crates/ringbuf) | 无锁环形缓冲区 |
<!-- R3 --> | axtask | 0.3.0-preview.2 | 项目内部 | 异步任务调度器 |
<!-- R4 --> | axpoll | 0.1.2 | 项目内部 | 轮询/事件通知 |

## 领域知识笔记

<!-- 添加时格式: <!-- R{编号} --> 笔记内容 -->

### 规范

<!-- R11 --> [NS16550A UART Specification](https://www.ti.com/lit/ds/symlink/pc16550d.pdf) - 寄存器定义与时序
<!-- R12 --> [RISC-V PLIC Specification](https://github.com/riscv/riscv-plic-spec) - 中断控制器编程
<!-- R13 --> [VirtIO Console Specification](https://docs.oasis-open.org/virtio/virtio/v1.2/csd01/virtio-v1.2-csd01.html#x1-2900003) - DMA 传输协议

### Embassy

<!-- R14 --> [Embassy Book](https://embassy.dev/book/) - 异步运行时文档
<!-- R15 --> [embassy-sync AtomicWaker API](https://docs.embassy.dev/embassy-sync/git/default/struct.AtomicWaker.html) - 中断安全唤醒
<!-- R28 --> [Embassy GitHub](https://github.com/embassy-rs/embassy) - 源码与 release 说明
<!-- R29 --> [embassy-executor v0.10.0](https://github.com/embassy-rs/embassy/releases) - 执行器最新版，含定时队列独立 crate
<!-- R30 --> [probe-rs 调试工具](https://probe.rs/) - Embassy 推荐的调试/烧录工具链
<!-- R31 --> [defmt 日志框架](https://defmt.ferrous-systems.com/) - Embassy 生态推荐的格式化日志

### Rust 异步

<!-- R16 --> [Rust Async Book](https://rust-lang.github.io/async-book/) - async/await 原理
<!-- R17 --> [Pin and Unpin](https://doc.rust-lang.org/std/pin/index.html) - 自引用结构安全

### Linux 参考

<!-- R18 --> [Linux serial_core.c](https://github.com/torvalds/linux/blob/master/drivers/tty/serial/serial_core.c) - 串口驱动参考实现
<!-- R19 --> [Linux 8250 driver](https://github.com/torvalds/linux/blob/master/drivers/tty/serial/8250/8250_core.c) - NS16550 驱动参考

### 设计文档

<!-- R10 --> | docs/embassy.md | Embassy 可用组件分析 |
<!-- R20 --> | docs/analysis/project-overview.md | 项目概览：仓库结构、构建系统、依赖图 |
<!-- R21 --> | docs/analysis/boot-init.md | 启动流程：axruntime → mount → spawn init |
<!-- R22 --> | docs/analysis/device-registration.md | 设备注册：DeviceOps trait、Device struct、devfs |
<!-- R23 --> | docs/analysis/tty-console-stack.md | TTY/Console 栈：N_TTY、ldisc、termios |
<!-- R24 --> | docs/analysis/async-io-framework.md | 异步 I/O 框架：poll_io、PollSet、Pollable、register_irq_waker |
<!-- R25 --> | docs/analysis/syscall-interface.md | 系统调用接口：FileLike、FD_TABLE、poll/select/epoll |
<!-- R26 --> | docs/analysis/task-process-model.md | 任务与进程模型：Thread、ProcessData、AsThread |
<!-- R27 --> | docs/analysis/async-uart-design-context.md | 异步 UART 设计上下文：现有模式、目标架构、关键文件索引 |
