# SNAPSHOT.md - 项目快照

> Generated at 2026-05-24
> Last updated: 2026-05-25

---

## 当前状态

**Milestone**: M0 (基础设施就绪) - 准备中
**Status**: 分支已创建，文档体系已建立，milestone 规划已完成，尚未开始编码
**Branch**: feat/uart-async (from main)

---

## 项目结构

```
StarryOS/
├── kernel/src/
│   ├── config/           # 内核配置
│   ├── entry.rs          # 内核入口
│   ├── file/             # 文件系统核心
│   │   ├── pipe.rs       # 异步管道（参考）
│   │   └── event.rs      # EventFd（参考）
│   ├── lib.rs            # 模块注册
│   ├── mm/               # 内存管理
│   ├── pseudofs/         # 伪文件系统
│   │   └── dev/          # /dev 设备注册
│   │       └── tty/      # TTY/Console/ldisc
│   ├── syscall/          # 系统调用
│   └── task/             # 任务管理
├── docs/analysis/        # 设计分析文档（15 个 md）
├── .claude/docs/         # 开发文档体系
│   ├── tasks.md          # 任务跟踪（M0~M6 milestone 规划）
│   ├── learned.md        # 学习记录
│   ├── references.md     # 参考资料
│   ├── architecture.md    # 架构决策（ADR-001~009）
│   ├── rules.md          # 编码规范
│   └── optimization.md   # 优化记录
└── CLAUDE.md             # 项目约束规则
```

---

## 技术栈

| 类别 | 技术 | 版本 |
|------|------|------|
| 语言 | Rust | nightly-2026-02-25 |
| 目标 | RISC-V 64-bit | qemu-riscv64 |
| 异步 | axtask::future | 0.3.0-preview.2 |
| 轮询 | axpoll | 0.1.2 |
| 硬件 | NS16550 UART | QEMU virt |
| UART 驱动 | uart_16550 (本地 v0.6.0) | path 依赖 |
| 缓冲 | ringbuf | 0.4.8 |
| 构建 | Make + Cargo | - |

---

## 关键代码路径速查

| 模块 | 路径 | 说明 |
|------|------|------|
| Console UART | axplat-riscv64-qemu-virt console.rs | MmioSerialPort + try_receive/send_raw |
| PLIC 中断 | axplat-riscv64-qemu-virt irq.rs | PLIC claim/complete + HandlerTable |
| IRQ Hook | axhal irq.rs | register_irq_hook → irq_handler 分发 |
| register_irq_waker | axtask future/poll.rs | IRQ → PollSet.wake() → 任务唤醒 |
| Pipe 异步模式 | kernel/file/pipe.rs | block_on + poll_io + PollSet 参考实现 |
| N_TTY Console | kernel/pseudofs/dev/tty/ntty.rs | register_irq_waker 使用范例 |
| tty-reader copier | kernel/pseudofs/dev/tty/terminal/ldisc.rs | poll_fn 循环 + spawn_with_name |
| 设备注册 | kernel/pseudofs/dev/mod.rs | builder() 中注册 /dev 设备 |
| DeviceOps trait | kernel/pseudofs/device.rs | 设备操作 trait |
| QEMU 配置 | axplat-riscv64-qemu-virt axconfig.toml | UART_PADDR=0x10000000, UART_IRQ=0x0a |

---

## Git 状态

**当前分支**: feat/uart-async
**基线分支**: main (2e075ac)
**未提交更改**: CLAUDE.md + .claude/docs/ (新增) + docs/analysis/ (新增)

---

## 关键文件

| 文件 | 作用 | 状态 |
|------|------|------|
| kernel/src/file/pipe.rs | 异步管道参考 | 稳定（已验证 poll_io 模式） |
| kernel/src/file/event.rs | EventFd 参考 | 稳定（已验证 Pollable 模式） |
| kernel/src/pseudofs/device.rs | 设备注册框架 | 稳定（DeviceOps trait） |
| kernel/src/pseudofs/dev/tty/ntty.rs | Console + register_irq_waker | 稳定（中断驱动 tty-reader） |
| kernel/src/pseudofs/dev/tty/terminal/ldisc.rs | tty-reader copier 任务 | 稳定（poll_fn + spawn_with_name） |
| kernel/src/pseudofs/dev/mod.rs | /dev 设备注册 builder | 稳定（添加新设备入口） |
| kernel/src/drivers/serial/ | 新增异步串口驱动 | 待创建（M1） |

---

## 当前工作

### 进行中

- [x] 创建 feat/uart-async 分支
- [x] 建立 .claude/docs 文档体系
- [x] 生成 CLAUDE.md 项目约束
- [x] 完成 M0~M6 milestone 规划

### 待办

- [ ] M0.1: 添加 uart_16550 本地 path 依赖
- [ ] M0.2: 添加 embassy-sync 依赖
- [ ] M0.3: QEMU 添加第二个串口
- [ ] M0.4: UART 中断注册与回调触发验证

### 阻塞

- 无

---

## 技术决策记录

| 决策 | 选择 | 原因 | 时间 |
|------|------|------|------|
| 异步运行时 | axtask::future + embassy-sync::AtomicWaker | 最小侵入，复用现有 | 2026-05-24 |
| 与控制台关系 | 先独立后统一（M2→M3） | 隔离风险，渐进演化 | 2026-05-25 |
| VFS 接口 | DeviceOps trait | 与现有设备一致 | 2026-05-24 |
| 缓冲策略 | ringbuf::HeapRb + PollSet | 已验证，零额外依赖 | 2026-05-24 |
| termios | 可切换，默认 raw | 高性能与功能兼得 | 2026-05-24 |
| 硬件抽象 | AsyncUart trait | 可扩展多硬件 | 2026-05-24 |
| 中断分发 | ISR → AtomicWaker → copier 任务 | ISR 极简，数据安全 | 2026-05-25 |
| uart_16550 | 本地最新版 path 依赖 | 完整中断控制 API | 2026-05-25 |
| DMA 策略 | 远期 M6，M0-M4 全中断驱动 | QEMU 无真正 DMA | 2026-05-25 |

---

## 性能目标

| 指标 | 目标 | 基线 |
|------|------|------|
| 最大波特率 | 1 Mbps (可扩展至 2 Mbps) | - |
| RX 延迟 | < 500 µs | 115200 bps |
| 吞吐量 | > 90% 线速 | 115200 bps |
| CPU 利用率（空闲） | 0% | - |
| 多端口并发 | 4 端口 | - |
| 缓冲区大小 | 可配置，默认 64 KiB | - |

---

## 最近修改

| 时间 | 文件 | 改动类型 |
|------|------|----------|
| 2026-05-25 | .claude/docs/tasks.md | 重写（M0~M6 milestone 规划） |
| 2026-05-25 | .claude/docs/SNAPSHOT.md | 更新（状态快照） |
| 2026-05-25 | .claude/docs/architecture.md | 新增 A07-A09 |
| 2026-05-24 | CLAUDE.md | 新增（项目约束规则） |
| 2026-05-24 | .claude/docs/learned.md | 新增（学习记录） |
| 2026-05-24 | .claude/docs/references.md | 新增（参考资料） |

---

## 下一步

1. **M0.1**: 在 kernel/Cargo.toml 添加 uart_16550 path 依赖 + embassy-sync 依赖
2. **M0.2**: cargo check 验证编译通过
3. **M0.3**: QEMU 添加第二串口配置
4. **M0.4**: 验证第二串口中断回调触发
5. **Gate M0**: `make run` 编译通过 + 中断回调触发
