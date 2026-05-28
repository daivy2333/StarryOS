# SNAPSHOT.md - 项目快照

> Generated at 2026-05-24
> Last updated: 2026-05-28

---

## 当前状态

**Milestone**: M3 (异步引擎实现) — ⚠️ **回滚状态**
**Status**: AsyncUart 驱动代码已实现（Task 1-5），编译通过，**完全未集成**
**Branch**: feat/uart-async
**Rollback commit**: d29a28f（M3 Task 5 - module exports 完成）

**回滚原因**：
- M3 替换尝试（Console → AsyncUart）失败
- **IRQ 风暴问题**：IRQ 10 触发异常，RX-COPIER 循环唤醒
- **TX busy-loop 问题**：TX FIFO 满，UART 状态异常（LSR=0x00）
- **UART 硬件未正常发送数据**：FIFO 满后 retry 无效

**当前代码状态**：
- ✅ AsyncUart trait + Uart16550Async 实现
- ✅ AsyncBuffer（Ring Buffer + PollSet）实现
- ✅ ISR（IsrContext + AtomicWaker）实现
- ✅ AsyncUartDriver（RX/TX copier）实现
- ✅ Module exports 完成
- ⚠️ **完全未集成**：ISR 未注册，copier 任务未启动
- ✅ OS 仍使用 **Console 阻塞输出**（正常工作）

**下一步决策**：
- 需重新评估整体方案
- IRQ 风暴 + TX busy-loop 根因未完全明确
- 可能需要更根本的设计改动（ADR 待更新）

**验证结果**:
- ✅ DeviceOps trait 实现正确（write_at 成功）
- ✅ Pollable trait 实现正确（poll 返回正确事件）
- ✅ TX 路径正常（Console 输出可见）
- ✅ poll IN/OUT 事件正确返回

---

## 项目结构

```
StarryOS/
├── kernel/src/
│   ├── config/           # 内核配置
│   ├── drivers/          # 设备驱动（新增）
│   │   └── serial/       # 异步串口驱动（M1）
│   │       ├── mod.rs         # 模块导出
│   │       ├── ring_buffer.rs # AsyncBuffer (Ring Buffer + PollSet)
│   │       ├── console_driver.rs # ConsoleDriver + RX copier
│   │       └── device_ops.rs  # AsyncUartTestDevice (DeviceOps + Pollable)
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
│   ├── architecture.md    # 架构决策（ADR-001~015）
│   ├── rules.md          # 编码规范
│   └── optimization.md   # 优化记录
│   └── superpowers/      # 设计文档和实现计划
│       ├── specs/        # Spec 文档
│       └── plans/        # Plan 文档
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
| 2026-05-27 | kernel/src/drivers/serial/* | M1 实现（AsyncBuffer + ConsoleDriver + DeviceOps） |
| 2026-05-27 | kernel/src/pseudofs/dev/mod.rs | 注册 async_uart_test 设备到 devfs |
| 2026-05-27 | kernel/Cargo.toml | 新增 uart-async feature |
| 2026-05-27 | .claude/docs/learned.md | 新增 L74（Console 共用数据竞争现象） |
| 2026-05-27 | .claude/docs/SNAPSHOT.md | 更新 Milestone 为 M1 完成 |
| 2026-05-27 | .claude/docs/tasks.md | 更新 T1.1-T1.6 状态（M1 完成） |
| 2026-05-27 | .claude/docs/learned.md | 新增 L73（uart_16550 版本共存说明） |
| 2026-05-27 | .claude/docs/learned.md | 新增 L65-L72（构建环境、rootfs、踩坑经验、命令速查） |
| 2026-05-27 | .claude/docs/references.md | 新增 R38-R40（uart_16550、musl 工具链、rootfs） |
| 2026-05-27 | kernel/Cargo.toml | 新增 embassy-sync v0.6.2 依赖（T0.2） |
| 2026-05-27 | kernel/Cargo.toml | 新增 uart_16550 path 依赖（T0.1） |
| 2026-05-27 | disk.img + make/disk.img | rootfs 部署（1GB） |
| 2026-05-27 | .claude/docs/architecture.md | 新增 A15（渐进式开发策略） |
| 2026-05-25 | .claude/docs/tasks.md | 重写（M0~M6 milestone 规划） |

---

## 下一步

**M1 已完成**，进入 M2 VFS 验证：

1. **T2.1**: ConsoleDriver 实现 DeviceOps（read_at/write_at/as_pollable）
2. **T2.2**: 注册测试设备到 devfs
3. **T2.3**: 用户态验证（open/read/write）
4. **T2.4**: termios 支持框架（可选）

> 渐进式策略：M1/M2 用 Console 同步引擎验证架构，M3 替换为 AsyncUart 异步引擎（参见 ADR-015）
> 已知约束：Console 共用数据竞争（L74），M3 解决
