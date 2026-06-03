# SNAPSHOT.md - 项目快照

> Last updated: 2026-06-03
> 分支：asyncuart-dev — Q0~Q7 ✅，OpenSpec 体系建立（2026-06-03），Q6 等待硬件

---

## 当前状态

**分支**: asyncuart-dev（基于 `feat/uart-async-dev2`，Q0~Q7 完成，OpenSpec 文档体系建立）
**成果**:
- kernel 层独立实现完整异步串口栈（~500 行），不修改任何外部 crate
- **OpenSpec 文档体系建立**（2026-06-03）：5 个 spec 域（rules / architecture / learned / references / optimization），全部通过 `openspec validate --specs`
- 原 `.claude/docs/{architecture,learned,references,optimization,rules}.md` 已迁移至 `openspec/specs/`，源文件以 `.bak` 保留
**Shell**: stdin/stdout 双向异步，`ls`/`cd`/`pwd` 全部正常
**Q7 已完成**: yield storm 修复（O42）、FIONBIO 传播（O43）、benchmark 修正 + TCSBRK 实现（O44）
**O45 已完成**: tcdrain 真异步化（PollSet + DRAIN_WAKER，消除协作自旋）
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
| **QEMU 时序限制** | QEMU 16550 不仿真串口线延迟，吞吐量数据偏高；真板才反映真实 ~11.5 KB/s |
| **TCSBRK 实现** | tcdrain 通过 poll 循环检查 ring buffer + LSR.TRANSMITTER_EMPTY（bit 6, TEMT） |
| **O_NONBLOCK 传播** | open()/fcntl/ioctl 三个入口都需转发 FIONBIO 到 Tty，缺一不可 |
| **LSR 位注意** | THR_EMPTY=bit5（可写），TRANSMITTER_EMPTY=bit6（THR+移位寄存器全空=真正 drain） |
| **DRAIN_WAKER** | 专用 AtomicWaker，ISR TX 中断时唤醒 tcdrain，替代 wake_by_ref 自旋 |
| **tcdrain 性能** | QEMU 上 64B 从 9 次切换降到 6 次，延迟 ~300→~200 µs（真板上可忽略） |
| **e2e 吞吐量** | 4096B 真板预测效率 97.7% 线速（软件开销 < 2.3%） |
| **e2e 延迟** | 单字节 139.5 µs avg（硬件理论 86.8 µs，软件开销 52.7 µs） |

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

### 历史

> 方向 A（渐进式集成 Console）和方向 B（完全剔除）因 stride=4 + IRQ 风暴在 2026-05 中期放弃，最终采用方向 C（kernel 层独立实现，Q0-Q7 全部完成）。详见 `architecture.md` 和 `docs/analysis/async-uart-implementation-history.md`。

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
├── docs/analysis/        # 设计分析文档（13 份）
├── openspec/             # OpenSpec 规范（2026-06-03 初始化）
│   ├── project.md        # 项目上下文（技术栈、约束、约定）
│   ├── config.yaml       # schema: spec-driven
│   ├── specs/            # 5 个 domain spec
│   │   ├── rules/spec.md         # 三大规则 + ISR/MMIO/Git 项目特定
│   │   ├── architecture/spec.md  # ADR-001~031（按主题分组）
│   │   ├── learned/spec.md       # API/文件/踩坑/技巧/性能/测试
│   │   ├── references/spec.md    # 依赖/子项目/规范/Embassy/Linux/分析
│   │   └── optimization/spec.md  # Q5/Q7 完成 + Q6/远期/排除
│   └── changes/          # 变更提案
├── .claude/              # Claude Code / OpenSpec 工具链
│   ├── commands/opsx/    # OpenSpec slash commands（5）
│   ├── skills/openspec-*/# OpenSpec skills（5）
│   ├── docs/             # 状态文档（本文件所在）
│   │   ├── SNAPSHOT.md   # 本文件
│   │   ├── tasks.md      # 任务追踪（含 P0 OpenSpec milestone）
│   │   ├── archive.md    # 归档内容（含 2026-06-03 OpenSpec 迁移）
│   │   ├── *.md.bak (×5) # 迁移源备份
│   │   └── superpowers/  # 设计文档和实现计划
│   └── settings.local.json
└── CLAUDE.md             # 项目入口（OpenSpec + .claude/docs/ 双索引）
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

> **2026-06-03 重大变更**：原 `.claude/docs/{architecture,learned,references,optimization,rules}.md` 已迁移至 `openspec/specs/`，本节索引同步更新。

| 文档 | 内容 | 条目数 |
|------|------|--------|
| `openspec/specs/rules/spec.md` | 三大规则（Karpathy + 务实编码 + Workflow Designer） + ISR/MMIO/Git 项目特定 | 17 Requirements |
| `openspec/specs/architecture/spec.md` | ADR-001~031（按主题分组） | 13 Requirements |
| `openspec/specs/learned/spec.md` | API 路径、文件速查、踩坑档案、技巧模式、性能/测试 | 10 Requirements |
| `openspec/specs/references/spec.md` | 依赖、子项目索引、规范、Embassy、Linux serial、项目分析 | 8 Requirements |
| `openspec/specs/optimization/spec.md` | Q5/Q7 已完成 + Q6/远期 + 已排除 + 性能基线 | 6 Requirements |
| `openspec/project.md` | 项目上下文（技术栈、约束、目录、Git 规范） | — |
| `CLAUDE.md` | OpenSpec + .claude/docs/ 双索引入口 | 5.7 KB |
| `.claude/docs/tasks.md` | 任务追踪（含 P0 OpenSpec milestone） | Q0~Q7 + P0 |
| `.claude/docs/archive.md` | 已归档内容（含 2026-06-03 OpenSpec 迁移） | 持续累积 |
| `.claude/docs/*.md.bak` (×5) | OpenSpec 迁移前源文件备份 | 70 KB |
| `docs/uart-performance-comparison.md` | Console vs Async 对比报告 | ✅ Q7 更新 |
| `docs/benchmark-report-async.md` | Async 详细测试报告 | ✅ Q7 更新 |
| `docs/benchmark-report-console.md` | Console 详细测试报告 | - |

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
