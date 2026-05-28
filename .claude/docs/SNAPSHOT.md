# SNAPSHOT.md - 项目快照

> Generated at 2026-05-24
> Last updated: 2026-05-28

---

## 当前状态

**Branch**: feat/uart-async-dev2
**Status**: 探索完全剔除 Console 的方案（从零开始）
**Base branch**: feat/uart-async（保留文档体系，回滚所有代码）

**分支策略变更**：
- **原 feat/uart-async 分支**：渐进式集成方案（复用 Console UART 初始化）
- **新 feat/uart-async-dev2 分支**：完全剔除 Console 方案（从零开始实现）

**当前目标**：
- 完全剔除 Console（axplat 外部 crate）
- 使用本地 uart_16550 crate + 自实现 UART 初始化
- 实现独立的异步串口架构（不依赖 Console）

**代码状态**：
- ✅ 所有 AsyncUart 驱动代码已删除（回滚到 main 分支状态）
- ✅ kernel/Cargo.toml 已恢复（无 uart_16550 + embassy-sync 依赖）
- ✅ kernel/src/drivers/ 目录已删除
- ✅ 文档体系完整保留（Console UART 研究 + 架构决策）

**下一步**：
- 重新规划 Milestone（完全剔除 Console 方案）
- 设计新的 UART 初始化流程（替代 axplat）
- 实现独立的异步串口架构

---

## 项目结构

```
StarryOS/
├── kernel/src/
│   ├── config/           # 内核配置
│   ├── drivers/          # 设备驱动（待创建）
│   ├── entry.rs          # 内核入口
│   ├── file/             # 文件系统核心
│   │   ├── pipe.rs       # 异步管道（参考）
│   │   └── event.rs      # EventFd（参考）
│   ├── lib.rs            # 模块注册
│   ├── mm/               # 内存管理
│   ├── pseudofs/         # 伪文件系统
│   │   └── dev/          # /dev 设备注册
│   │       └── tty/      # TTY/Console/ldisc（待剔除或保留？）
│   ├── syscall/          # 系统调用
│   └── task/             # 任务管理
├── docs/analysis/        # 设计分析文档
│   ├── console-uart-mechanism.md  # Console UART 研究（保留作为参考）
│   └── ...               # 其他分析文档
├── .claude/docs/         # 开发文档体系
│   ├── tasks.md          # 任务跟踪（待更新）
│   ├── learned.md        # 学习记录（待清理）
│   ├── references.md     # 参考资料（待清理）
│   ├── architecture.md   # 架构决策（待更新）
│   ├── rules.md          # 编码规范
│   └── optimization.md   # 优化记录
│   └── superpowers/      # 设计文档
│       └── specs/        # Spec 文档
│           └── 2026-05-28-async-uart-integration-design.md  # 渐进式集成设计（归档参考）
└── CLAUDE.md             # 项目约束规则
```

---

## 技术栈

| 类别 | 技术 | 版本 |
|------|------|------|
| 语言 | Rust | nightly-2026-02-25 |
| 目标 | RISC-V 64-bit | qemu-riscv64 |
| 异步 | axtask::future | 0.3.0-preview.2 |
| 异步同步 | embassy-sync | v0.6.2 |
| 轮询 | axpoll | 0.1.2 |
| 硬件 | NS16550 UART | QEMU virt |
| UART 驱动 | uart_16550 (本地 v0.6.0) | 待添加 path 依赖 |
| 缓冲 | ringbuf | 0.4.8 |
| 构建 | Make + Cargo | - |

---

## 关键代码路径速查

| 模块 | 路径 | 说明 |
|------|------|------|
| Console UART | axplat-riscv64-qemu-virt console.rs | 外部 crate（待剔除）|
| PLIC 中断 | axplat-riscv64-qemu-virt irq.rs | PLIC claim/complete + HandlerTable |
| IRQ Hook | axhal irq.rs | register_irq_hook → irq_handler 分发 |
| register_irq_waker | axtask future/poll.rs | IRQ → PollSet.wake() → 任务唤醒 |
| Pipe 异步模式 | kernel/file/pipe.rs | block_on + poll_io + PollSet 参考实现 |
| N_TTY Console | kernel/pseudofs/dev/tty/ntty.rs | register_irq_waker 使用范例（待剔除或保留？）|
| tty-reader copier | kernel/pseudofs/dev/tty/terminal/ldisc.rs | poll_fn 循环 + spawn_with_name（待剔除或保留？）|
| 设备注册 | kernel/pseudofs/dev/mod.rs | builder() 中注册 /dev 设备 |
| DeviceOps trait | kernel/pseudofs/device.rs | 设备操作 trait |
| UART MMIO | 0x10000000, IRQ 10 | QEMU virt UART 硬件配置 |

---

## Git 状态

**当前分支**: feat/uart-async-dev2
**基线分支**: feat/uart-async（文档提交 a5cd778）
**代码状态**: 完全回滚到 main 分支状态
**文档状态**: 保留完整（Console UART 研究 + 渐进式集成设计）

---

## 当前工作

### 进行中

- [ ] 重新规划 Milestone（完全剔除 Console 方案）
- [ ] 设计新的 UART 初始化流程（替代 axplat）
- [ ] 评估 Console剔除的影响范围

### 待办

- [ ] 确定哪些 Console 相关代码需要保留/剔除
- [ ] 实现独立的 UART 初始化（使用 uart_16550 crate）
- [ ] 设计新的异步串口架构

---

## 技术决策记录

| 决策 | 选择 | 原因 | 时间 |
|------|------|------|------|
| 异步运行时 | axtask::future + embassy-sync::AtomicWaker | 最小侵入，复用现有 | 2026-05-24 |
| VFS 接口 | DeviceOps trait | 与现有设备一致 | 2026-05-24 |
| 缓冲策略 | ringbuf::HeapRb + PollSet | 已验证，零额外依赖 | 2026-05-24 |
| 硬件抽象 | AsyncUart trait | 可扩展多硬件 | 2026-05-24 |
| 中断分发 | ISR → AtomicWaker → copier 任务 | ISR 极简，数据安全 | 2026-05-25 |
| **分支策略** | **完全剔除 Console** | **避免集成冲突，从零开始** | **2026-05-28** |

---

## 性能目标

| 指标 | 目标 | 基线 |
|------|------|------|
| 最大波特率 | 1 Mbps (可扩展至 2 Mbps) | - |
| RX 延迟 | < 500 µs | 115200 bps |
| 吞吐量 | > 90% 线速 | 115200 bps |
| CPU 利用率（空闲） | 0% | - |

---

## 关键问题

**完全剔除 Console 的关键问题**：
1. **UART 初始化替代**：如何替代 axplat 的 UART 初始化？
2. **内核启动日志**：如何实现 earlycon（启动日志输出）？
3. **Console 软件路径剔除范围**：哪些 Console 相关代码需要剔除？
4. **用户态串口访问**：如何提供用户态串口 API？

---

## 参考资料

- Console UART 研究文档：`docs/analysis/console-uart-mechanism.md`
- 渐进式集成设计（归档）：`.claude/docs/superpowers/specs/2026-05-28-async-uart-integration-design.md`
- UART 16550 规范：`uart_16550/src/spec.rs`