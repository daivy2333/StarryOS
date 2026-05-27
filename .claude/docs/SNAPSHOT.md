# SNAPSHOT.md - 项目快照

> Generated at 2026-05-24
> Last updated: 2026-05-27

---

## 当前状态

**Milestone**: M0 (基础设施就绪) - ✅ 完成
**Status**: 基础依赖已就绪，中断机制已确认，可进入 M1 架构验证
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
| 异步同步 | embassy-sync | v0.6.2 ✅ 2026-05-27 |
| 轮询 | axpoll | 0.1.2 |
| 硬件 | NS16550 UART | QEMU virt |
| UART 驱动 | uart_16550 (本地 v0.6.0) | path 依赖 ✅ 2026-05-27 |
| 缓冲 | ringbuf | 0.4.8 |
| 构建 | Make + Cargo | - |
| 交叉编译 | riscv64-linux-musl | /opt/musl/riscv64-linux-musl-cross ✅ 2026-05-27 |
| rootfs | rootfs-riscv64.img | 1GB (disk.img) ✅ 2026-05-27 |

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
**未提交更改**: 
- kernel/Cargo.toml (新增 uart_16550 + embassy-sync 依赖)
- disk.img (1GB rootfs)
- make/disk.img (副本)
- CLAUDE.md + .claude/docs/ (新增) + docs/analysis/ (新增)

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

- [x] T0.1: 添加 uart_16550 本地 path 依赖 ✅ 2026-05-27
- [x] T0.2: 添加 embassy-sync 依赖 ✅ 2026-05-27
- [x] T0.3: UART 中断注册验证（IRQ 10 共存语义） ✅ 2026-05-27
- [x] T0.4: Gate M0 验证 ✅ 2026-05-27

**M0 完成，准备进入 M1 架构验证**

### 阻塞

- 无

---

## 技术决策记录

| 决策 | 选择 | 原因 | 时间 |
|------|------|------|------|
| 异步运行时 | axtask::future + embassy-sync::AtomicWaker | 最小侵入，复用现有 | 2026-05-24 |
| VFS 接口 | DeviceOps trait | 与现有设备一致 | 2026-05-24 |
| 缓冲策略 | ringbuf::HeapRb + PollSet | 已验证，零额外依赖 | 2026-05-24 |
| termios | 可切换，默认 raw | 高性能与功能兼得 | 2026-05-24 |
| 硬件抽象 | AsyncUart trait | 可扩展多硬件 | 2026-05-24 |
| 中断分发 | ISR → AtomicWaker → copier 任务 | ISR 极简，数据安全 | 2026-05-25 |
| uart_16550 | 本地最新版 path 依赖 | 完整中断控制 API | 2026-05-25 |
| DMA 策略 | 远期 M6，M0-M4 全中断驱动 | QEMU 无真正 DMA | 2026-05-25 |
| axhal::console | 外部 crate，不可修改 | 内核日志同步阻塞不可避免 | 2026-05-27 |
| Console 统一 | 内核同步 + 用户态异步 | 软件路径分离，共用硬件 | 2026-05-27 |
| 渐进式开发 | M1/M2 用 Console 验证，M3 替换 AsyncUart | 调试能力保留，风险分摊 | 2026-05-27 |

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
| 2026-05-27 | .claude/docs/learned.md | 新增 L73（uart_16550 版本共存说明） |
| 2026-05-27 | .claude/docs/learned.md | 新增 L65-L72（构建环境、rootfs、踩坑经验、命令速查） |
| 2026-05-27 | .claude/docs/references.md | 新增 R38-R40（uart_16550、musl 工具链、rootfs） |
| 2026-05-27 | .claude/docs/SNAPSHOT.md | 更新技术栈、Git 状态、最近修改 |
| 2026-05-27 | .claude/docs/tasks.md | 更新 T0.1-T0.4 状态（全部完成） |
| 2026-05-27 | kernel/Cargo.toml | 新增 embassy-sync v0.6.2 依赖（T0.2） |
| 2026-05-27 | .claude/docs/learned.md | 新增 L63-L64（register_irq_waker 共存机制） |
| 2026-05-27 | kernel/Cargo.toml | 新增 uart_16550 path 依赖（T0.1） |
| 2026-05-27 | disk.img + make/disk.img | rootfs 部署（1GB） |
| 2026-05-27 | .claude/docs/architecture.md | 新增 A15（渐进式开发策略） |
| 2026-05-27 | .claude/docs/tasks.md | 重构 M1-M3（渐进式验证 + 异步引擎替换） |
| 2026-05-27 | .claude/docs/architecture.md | 新增 A13-A14（axhal::console 外部 crate 约束） |
| 2026-05-27 | .claude/docs/learned.md | 新增 L60-L62（外部 crate 层次、路径分离、earlycon） |
| 2026-05-25 | .claude/docs/tasks.md | 重写（M0~M6 milestone 规划） |
| 2026-05-25 | .claude/docs/SNAPSHOT.md | 更新（状态快照） |
| 2026-05-25 | .claude/docs/architecture.md | 新增 A07-A12 |
| 2026-05-24 | CLAUDE.md | 新增（项目约束规则） |
| 2026-05-24 | .claude/docs/learned.md | 新增（学习记录） |
| 2026-05-24 | .claude/docs/references.md | 新增（参考资料） |

---

## 下一步

**M0 已完成**，进入 M1 架构验证：

1. **T1.1**: Ring Buffer 实现（rx_buf + tx_buf）
2. **T1.2**: 中断机制验证（IRQ 10 共存）
3. **T1.3**: RX copier 任务模型验证
4. **T1.4**: TX 路径模拟验证

> 渐进式策略：M1/M2 用 Console 同步引擎验证架构，M3 替换为 AsyncUart 异步引擎（参见 ADR-015）
