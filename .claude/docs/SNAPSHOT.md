# SNAPSHOT.md - 项目快照

> Last updated: 2026-06-01
> 分支：feat/uart-async-bench — Q0~Q7 ✅，Q6 等待硬件

---

## 当前状态

**分支**: feat/uart-async-dev2（性能分析完成，待实施优化）
**成果**: 在 kernel 层独立实现完整异步串口栈（~500 行），不修改任何外部 crate
**Shell**: stdin/stdout 双向异步，`ls`/`cd`/`pwd` 全部正常
**近期分析**: 完成用户态异步性能打平/反超阻塞串口的根因分析，完成 FIONBIO 非阻塞模式分析
**Q7 已完成**: yield storm 修复、FIONBIO 传播、benchmark 修正
**下一步**: Q6 VisionFive2 真板验证（等待硬件到位）

### 关键发现

| 发现 | 详情 |
|------|------|
| **stride=4 根因** | NS16550 仅 8 字节，stride=4 越界 → LoadFault |
| **copier/Console 竞争** | RX copier 抢先读 FIFO 导致 Shell 无输入；Q3 替换 Console 后独占 |
| **IER 控制** | uart_16550 v0.6.0 只有 ier() 读接口，需直接 MMIO write_volatile |
| **critical-section** | embassy-sync AtomicWaker 需要 critical-section 符号，disable_irqs/enable_irqs 实现 |
| **Tty 泛型绑定** | Tty<R,W> 的 reader/writer 直接替换 Console，无需修改伪终端框架 |
| **axmm::iomap** | 现成 API 用于映射设备 MMIO，无需修改 axplat |
| **IER 缓存** | AtomicU8 缓存 IER 值，enable/disable 只需一次 write_volatile |
| **rx/tx 独立锁** | 消除 false contention，提升并发性能 |
| **NAPI 中断合并** | 连续成功 ≥16 次后切轮询模式，高吞吐时减少 90%+ IRQ |
| **批量 API** | receive_bytes/send_bytes 替代逐字节操作，减少函数调用开销 |
| **TX interleave 修复** | TX copier 用本地 cursor 追踪已发位置，避免与 ax_println! 输出交错 |
| **AtomicWaker 直接唤醒** | ISR 中 O(1) 唤醒，无需 BTreeMap 分发（O17 不需要） |
| **Console 组件清理** | 删除 ntty.rs + ConsoleWriter，ASYNC_TTY 成为唯一串口实现 |
| **性能测试框架** | 内核态统计 + 用户态 benchmark.c + 自动化脚本 |
| **三重 yield storm** | 用户态 async read 路径有 3 层嵌套 block_on/poll_io，Manual 模式 waker.wake_by_ref() 导致无数据时高频 yield-re-schedule |
| **Manual 模式缺陷** | ProcessMode::Manual 的 register_rx_waker 立即唤醒调用者，产生 yield storm；应改为 External 模式消除 |
| **Benchmark 不测 UART** | TX 吞吐量写 /dev/null（绕过 UART），未测量真实串口吞吐量 |
| **FIONBIO 不生效** | nonblocking 标志在 File 层存储，但 Tty::read_at 和 ldisc::read 硬编码 false，不传播到内层 |
| **Async VS 阻塞上限** | 115200 bps = 11.52 KB/s 硬件上限，async 在吞吐量上不可能超越阻塞 Console |
| **Async RX 多一次拷贝** | UART FIFO → ring buffer → ldisc buf → user buf（3 次 vs Console 的 2 次） |

### 实施路径

| 阶段 | 内容 | 状态 |
|------|------|------|
| **Q0** | Spike（stride=1 + 寄存器 + ISR + axmm::iomap） | ✅ |
| **Q1** | 驱动架构（ring_buffer + ISR + copier + critical-section） | ✅ |
| **Q2** | VFS 集成（DeviceOps + /dev/async_uart + Console 共存） | ✅ |
| **Q3** | AsyncUart RX 接管（Tty<AsyncUartReader, ConsoleWriter> → Shell stdin） | ✅ |
| **Q4** | 全异步 RX+TX | TX copier 接管，Shell 双向异步 | ✅ |
| **Q5** | 性能优化 | IER 缓存 + ISR 合并 + batch I/O + waker skip + rx/tx 独立锁 | ✅ |
| **Q5.1** | 性能优化续 | NAPI 中断合并 + 批量 API + FCR 阈值日志 + TX interleave 修复 | ✅ |
| **Q5.2** | 测试补全 | 用户态自动化测试 + 非阻塞模式 | 📋 分析完成 |
| **Q7** | 用户态性能修复 | yield storm + FIONBIO 传播 + benchmark 修正 | ✅ |
| **Q6** | 真板验证 | VisionFive2 | ⏳ |

### 最终架构

```
IRQ 10 → uart_isr_handler
           ├─ RX: disable_rx_intr → RX_WAKER.wake
           └─ TX: disable_tx_intr → TX_WAKER.wake

RX copier                     TX copier
  poll_fn:                     poll_fn:
    UART.read FIFO               buf.pop_tx
    buf.push_rx                  UART.write THR
    enable_rx_intr               enable_tx_intr (if partial)
    RX_WAKER.register            TX_WAKER.register
    → Shell stdin ✅             ← Shell stdout ✅

AsyncUartReader::read → ring_buffer pop
AsyncUartWriter::write → ring_buffer push
Tty<AsyncUartReader, AsyncUartWriter> → /dev/console

内核日志: ax_println! → Console polling TX（共存）
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
│   │   ├── mod.rs         # 模块声明（19 行）
│   │   ├── uart_init.rs   # UART 初始化 + IER 缓存（155 行）✅
│   │   ├── isr.rs         # ISR handler + AtomicWaker（22 行）✅
│   │   ├── ring_buffer.rs # RingBufRx/Tx + PollSet（58 行）✅
│   │   ├── async_driver.rs# AsyncUartDriver + RX/TX copier（99 行）✅
│   │   ├── device_ops.rs  # AsyncUartReader/Writer + TtyRead/TtyWrite（33 行）✅
│   │   └── ntty_async.rs  # AsyncTty 类型别名 + lazy_static（21 行）✅
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
├── docs/analysis/        # 设计分析文档（~16 份）
├── .claude/docs/         # 开发文档体系（本文件所在）
│   ├── SNAPSHOT.md       # 本文件
│   ├── architecture.md   # 架构决策记录（ADR-001~029，19 条有效）
│   ├── tasks.md          # 任务追踪（M0~M6 + P0~P6）
│   ├── learned.md        # 学习记忆（81 条目）
│   ├── references.md     # 外部参考（53 条目）
│   ├── optimization.md   # 优化记录（23 条目）
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
| architecture.md | ADR-001~029，两个方向的全部决策历史 | 19 |
| tasks.md | Q0~Q6 任务追踪（方向 C） | 37 |
| learned.md | API 路径、文件速查、踩坑档案、技巧模式 | 81 |
| references.md | 依赖文档、规范、设计文档索引 | 53 |
| optimization.md | 性能洞察、优化方向、基准目标 | 20 |
| rules.md | Karpathy Guidelines + 十大铁律 + Workflow | 唯一事实来源 |
| docs/uart-performance-comparison.md | Console vs Async 性能对比报告 | - |
| docs/benchmark-report-async.md | Async 详细测试报告 | - |
| docs/benchmark-report-console.md | Console 详细测试报告 | - |
| archive.md | 已归档的过时内容 | ~15 |

---

## 关键代码路径速查

| 模块 | 路径 | 用途 |
|------|------|------|
| **异步串口驱动** | | |
| UART 初始化 | kernel/src/drivers/uart_init.rs | UART 硬件初始化 + IER 缓存 |
| ISR handler | kernel/src/drivers/isr.rs | 中断处理 + AtomicWaker 唤醒 |
| Ring Buffer | kernel/src/drivers/ring_buffer.rs | RX/TX 环形缓冲区 + PollSet |
| AsyncUartDriver | kernel/src/drivers/async_driver.rs | RX/TX copier 任务 |
| TtyRead/TtyWrite | kernel/src/drivers/device_ops.rs | AsyncUartReader/Writer trait 实现 |
| AsyncTty | kernel/src/drivers/ntty_async.rs | Tty<AsyncUartReader, AsyncUartWriter> |
| **参考实现** | | |
| Pipe 异步参考 | kernel/src/file/pipe.rs | poll_io + register_irq_waker 模式 |
| EventFd 参考 | kernel/src/file/event.rs | 轻量异步通知 |
| DeviceOps | kernel/src/pseudofs/device.rs | 设备注册 trait |
| **硬件相关** | | |
| UART 硬件 | axhal/src/platform/riscv64_qemu_virt/uart.rs | MMIO 寄存器 |
| PLIC 中断 | axhal/src/platform/riscv64_qemu_virt/mod.rs | 中断号映射 |
| Console 驱动 | kernel/src/pseudofs/dev/tty/ntty.rs | Console struct（已删除） |
| TTY ldisc | kernel/src/pseudofs/dev/tty/terminal/ldisc.rs | 行规则处理 + Manual/External 模式 |
| **新分析文档** | | |
| 用户态性能分析 | docs/analysis/user-async-perf-analysis.md | yield storm、Manual 模式缺陷、benchmark 问题 |
| 非阻塞模式分析 | docs/analysis/nonblocking-mode-analysis.md | FIONBIO 实现、nonblocking 未传播、实现方案 |
