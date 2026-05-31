# SNAPSHOT.md - 项目快照

> Last updated: 2026-05-31
> 分支：feat/uart-async-dev2 — Q0 ✅ Q1 ✅ Q2 ✅（Console 共存），Q3 准备中

---

## 当前状态

**分支**: feat/uart-async-dev2
**目标**: 在 kernel 层独立实现高性能异步串口，不修改外部 crate
**阶段**: Q0/Q1/Q2 通过，Q2 阶段 Console 与 AsyncUart 共存已验证

### 关键发现

| 发现 | 详情 |
|------|------|
| **stride=4 根因** | NS16550 仅 8 字节，stride=4 越界 → LoadFault。改 1 后正常 |
| **copier/Console 竞争** | RX copier 会抢先读 UART FIFO，导致 Shell 收不到输入。Q3 替换 Console 后由 copier 独占 |
| **Console 共存** | copier OFF 时 Console 正常工作，/dev/async_uart 设备已注册 |

### 实施路径

| 阶段 | 内容 | 状态 |
|------|------|------|
| **Q0** | Spike（stride=1 + 寄存器 + ISR） | ✅ |
| **Q1** | 驱动架构（ring_buffer + ISR + copier） | ✅ |
| **Q2** | VFS 集成（DeviceOps + /dev/async_uart + Console 共存） | ✅ |
| **Q3** | Console 替换（earlycon + AsyncUart 接管 UART） | ⏳ 当前 |
| **Q4** | 性能优化 | ⏳ |
| **Q5** | 真板验证 | ⏳ 远期 |

### Q2 共存架构

```
Console (axplat) — 独占 UART RX/TX
AsyncUart — /dev/async_uart 已注册（读写在 ring buffer，无 UART 操作）
copier 任务 — OFF（Q3 启用，届时接管 UART）
```

### 两个历史探索方向总结

| 方向 | 分支 | 策略 | 结果 | 真实根因 |
|------|------|------|------|----------|
| **A: 渐进式集成** | feat/uart-async | 复用 Console，逐步替换 | M0-M2 ✅, M3 ❌ | IRQ 风暴 + TX busy-loop + stride=4 |
| **B: 完全剔除 Console** | feat/uart-async-dev2 | 从零开始独立初始化 | P0 ✅, P1-P2 阻塞 | **stride=4 导致 LoadFault** |

---

## 项目结构

```
StarryOS/
├── kernel/src/
│   ├── config/           # 内核配置
│   ├── drivers/          # 异步串口驱动模块
│   │   ├── mod.rs         # 模块声明
│   │   ├── uart_init.rs   # UART 初始化 ✅
│   │   ├── isr.rs         # ISR handler ✅
│   │   ├── ring_buffer.rs # ⏳ 占位符
│   │   ├── async_uart.rs  # ⏳ 占位符
│   │   ├── async_driver.rs# ⏳ 占位符
│   │   └── device_ops.rs  # ⏳ 占位符
│   ├── entry.rs          # 内核入口
│   ├── file/             # 文件系统核心
│   │   ├── pipe.rs       # 异步管道（参考实现）
│   │   └── event.rs      # EventFd（参考实现）
│   ├── lib.rs            # 模块注册
│   ├── mm/               # 内存管理
│   ├── pseudofs/         # 伪文件系统
│   │   └── dev/          # /dev 设备注册
│   │       └── tty/      # TTY/Console/ldisc
│   ├── syscall/          # 系统调用
│   └── task/             # 任务管理
├── docs/analysis/        # 设计分析文档
├── .claude/docs/         # 开发文档体系（本文件所在）
│   ├── SNAPSHOT.md       # 本文件
│   ├── architecture.md   # 架构决策记录（ADR-001~023）
│   ├── tasks.md          # 任务追踪（M0~M6 + P0~P6）
│   ├── learned.md        # 学习记忆（116+ 条目）
│   ├── references.md     # 外部参考
│   ├── optimization.md   # 优化记录
│   ├── rules.md          # 编码规范
│   ├── archive.md        # 归档内容
│   └── superpowers/      # 设计文档和实现计划
│       ├── specs/        # Spec 文档
│       └── plans/        # Plan 文档
└── CLAUDE.md             # 项目约束规则
```

---

## 技术栈

| 类别 | 技术 | 版本 | 备注 |
|------|------|------|------|
| 语言 | Rust | nightly-2026-02-25 | |
| 目标 | RISC-V 64-bit | qemu-riscv64 | |
| 异步 | axtask::future | 0.3.0-preview.2 | 项目内部 |
| 异步同步 | embassy-sync | v0.6.2 | AtomicWaker |
| 轮询 | axpoll | 0.1.2 | PollSet + Pollable |
| 硬件 | NS16550 UART | QEMU virt | |
| UART 驱动 | uart_16550（本地 v0.6.0） | path 依赖 | |
| 缓冲 | ringbuf | 0.4.8 | HeapRb |
| 构建 | Make + Cargo | | |
| 交叉编译 | riscv64-linux-musl | /opt/musl/riscv64-linux-musl-cross | |
| rootfs | rootfs-riscv64.img | 1GB | |

---

## 文档体系索引

| 文档 | 内容 | 条目数 |
|------|------|--------|
| architecture.md | ADR-001~023，两个方向的全部决策历史 | 23 |
| tasks.md | M0~M6（方向 A）+ P0~P6（方向 B）任务追踪 | ~30 |
| learned.md | API 路径、文件速查、踩坑档案、技巧模式 | 116+ |
| references.md | 依赖文档、规范、设计文档索引 | 48+ |
| optimization.md | 性能洞察、优化方向、基准目标 | 23 |
| rules.md | Karpathy Guidelines + 十大铁律 + Workflow | 唯一事实来源 |
| archive.md | 已归档的过时内容 | ~15 |

---

## 关键代码路径速查

| 模块 | 路径 | 用途 |
|------|------|------|
| Pipe 异步参考 | kernel/src/file/pipe.rs | poll_io + register_irq_waker 模式 |
| EventFd 参考 | kernel/src/file/event.rs | 轻量异步通知 |
| DeviceOps | kernel/src/pseudofs/device.rs | 设备注册 trait |
| UART 硬件 | axhal/src/platform/riscv64_qemu_virt/uart.rs | MMIO 寄存器 |
| PLIC 中断 | axhal/src/platform/riscv64_qemu_virt/mod.rs | 中断号映射 |
| Console 驱动 | kernel/src/pseudofs/dev/tty/ntty.rs | Console struct |
| tty-reader | kernel/src/pseudofs/dev/tty/terminal/ldisc.rs | RX copier 参考 |
