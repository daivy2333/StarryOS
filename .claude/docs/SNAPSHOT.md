# SNAPSHOT.md - 项目快照

> Last updated: 2026-06-15
> 分支：asyncuart-dev — Q0~Q12 ✅，Q12 OpenSpec 变更已归档（archive/2026-06-15-q12-embassy-path-a/），警告清零，死代码清理，Q6 ⏳ 等待硬件

---

## 当前状态

**分支**: asyncuart-dev（基于 `feat/uart-async-dev2`，Q0~Q7 完成，OpenSpec 文档体系建立）
**成果**:
- kernel 层独立实现完整异步串口栈（~500 行），不修改任何外部 crate
- **OpenSpec 文档体系建立**（2026-06-03）：4 个 spec 域（architecture / learned / references / optimization），全部通过 `openspec validate --specs`；rules 已整合到 CLAUDE.md（迁移墓碑见 `openspec/changes/archive/rules-domain-2026-06-03/`）
- 原 `.claude/docs/{architecture,learned,references,optimization,rules}.md` 已迁移至 `openspec/specs/`，源文件以 `.bak` 保留
**Shell**: stdin/stdout 双向异步，`ls`/`cd`/`pwd` 全部正常
**Q5.2 已完成**: 用户态自动化测试（O21）+ 非阻塞模式（O43 via Q7）
**Q7 已完成**: yield storm 修复（O42）、FIONBIO 传播（O43）、benchmark 修正（O44）、tcdrain 真异步化（O45）
**2026-06-05 文档补充**:
- O46 / O47 记录到 `optimization/spec.md`（Q8/Q9 远期优化）
- OE1~OE5 反模式（embassy Channel/Mutex/Watch/Semaphore/select!）记录到 `optimization/spec.md` "已排除优化"
- L81~L84 learned 踩坑档案（embassy 选型边界）记录到 `learned/spec.md` 新 Requirement
**2026-06-11 优化审计与阶段重规划**:
- 4 个并行 agent 深度扫描（UART 驱动 / ldisc 模型 / 全内核标记 / PollSet 迁移），发现 6+ 项未记录优化机会（含 3 项正确性 bug）
- 分析文档 `.claude/analysis/optimization-opportunity-audit.md` 生成
- L150~L155 新知识写入 `learned/spec.md`
- 阶段重规划：原 Q8（仅 O46）→ Q8 驱动引擎打磨（含 3 项正确性修复 + 热路径优化 + O46）；新增 Q10（数据路径优化）和 Q11（内核通用优化）
- Q9 解耦：time driver 基础设施（Q9.1~Q9.3）无需 Q6 硬件，可先行完成
**2026-06-11 Q8 完成**:
- 3 个并行 Agent 完成 Wave 1+2（正确性修复 + 热路径优化）：NAPI 退出、ISR 去锁、IER 规范化、waker 去重、DRAIN_WAKER 条件化
- 4 个并行 Agent 完成 Wave 3（O46 AtomicWaker 迁移）：signalfd/event/pipe/pidfd 共 8 个 PollSet→AtomicWaker
- uart_16550 添加 `set_ier()` 公共方法
- QEMU 实机验证通过：启动正常、Shell 交互正常、benchmark 无退化、FIONBIO PASS
- `cargo check` 0 错误 / `cargo clippy` 0 错误
**2026-06-11 Q10 完成**:
- BUF_SIZE 80→256（ldisc 缓冲扩容 3.2×）
- SimpleReader::poll 改用 push_slice 批量写入（减少 N 次 try_push 调用）
- LineDiscipline::read() / drain_input() 改为 &self（UnsafeCell 包装 buf_rx）
- QEMU 实机验证通过：Shell 正常、benchmark 性能提升
- `cargo check` 0 错误 / `cargo clippy` 0 错误
- **性能对比（Q8→Q10）**：256B TX 1332→1252 µs（↓6%），1024B TX 5170→4880 µs（↓5.6%），1B avg latency 145→122 µs（↓16%），overhead 58→35 µs（↓40%）
**2026-06-11 Q9 完成**:
- VTIME>0 读超时：复用 axtask::future::timeout()，无需 embassy-time
- ldisc.rs `todo!()` 替换为 `block_on(timeout(dur, poll_io(...)))` 
- `cargo check` 0 错误 / `cargo clippy` 0 错误
**2026-06-11 Q11 完成**:
- tty/mod.rs: 3 处 `.unwrap()` → `AxError` 传播
- mm/access.rs: 批量页验证（减少 aspace 锁获取，二进制搜索最大有效范围）
- syscall/fs/io.rs: `vec![0;4096]` → 栈数组
- syscall/fs/fd_ops.rs: close_range UNSHARE 范围优化
- terminal/mod.rs: `ws_col` 110→80（修复 QEMU 控制台显示换行错位）
- `cargo check` 0 错误 / `cargo clippy` 0 错误
**最终进度**: 全部可无硬件完成的优化已做完，仅剩 Q6 等待 VisionFive2 真板验证
- 性能趋势：1B avg latency Q8(145)→Q10(122)→Q11(118)→Q12(124)µs（Q12 去锁后小数据吞吐 ↑24%，software overhead ↓31%：53.9→37.1µs）
- 代码量：14 文件变更（StarryOS） + 1 文件（uart_16550），净增 ~450 行
**2026-06-11 代码质量收尾**:
- cargo 警告清零（21→0）：自动修复 6 项 + 死方法移除 5 项 + dead_code 标注 11 项
- 真死代码移除（8 方法，-76 行）：access.rs(3) + io.rs(2) + shm.rs(1) + ops.rs(1) + ring_buffer(5)
- 后续优化记录：O48(memtrack) + O49(Manual移除) + O50(预留接口) 写入 optimization/spec.md
**下一步**: Q6 VisionFive2 真板验证

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
| **三重 yield storm** | ✅ Q7 O42 修复：Manual→External ProcessMode |
| **Manual 模式缺陷** | ✅ Q7 O42 修复：External + PollSet 注册替代 wake_by_ref |
| **Benchmark 不测 UART** | ✅ Q7 O44 修正：/dev/console + tcdrain() |
| **FIONBIO 不生效** | ✅ Q7 O43 修复：三入口（open/fcntl/ioctl）全传播 |
| **Async VS 阻塞上限** | 115200 bps = 11.52 KB/s 硬件上限，async 在吞吐量上不可能超越阻塞 Console |
| **QEMU 时序限制** | QEMU 16550 不仿真串口线延迟，吞吐量数据偏高；真板才反映真实 ~11.5 KB/s |
| **TCSBRK 实现** | tcdrain 通过 poll 循环检查 ring buffer + LSR.TRANSMITTER_EMPTY（bit 6, TEMT） |
| **O_NONBLOCK 传播** | open()/fcntl/ioctl 三个入口都需转发 FIONBIO 到 Tty，缺一不可 |
| **LSR 位注意** | THR_EMPTY=bit5（可写），TRANSMITTER_EMPTY=bit6（THR+移位寄存器全空=真正 drain） |
| **DRAIN_WAKER** | 专用 AtomicWaker，ISR TX 中断时唤醒 tcdrain，替代 wake_by_ref 自旋 |
| **tcdrain 性能** | QEMU 上 64B 从 9 次切换降到 6 次，延迟 ~300→~200 µs（真板上可忽略） |
| **e2e 吞吐量** | 4096B 真板预测效率 97.7% 线速（软件开销 < 2.3%） |
| **e2e 延迟** | 单字节 139.5 µs avg（硬件理论 86.8 µs，软件开销 52.7 µs） |
| **O46 完成** | ✅ Q8 完成：pipe/signalfd/pidfd/event 共 8 处 PollSet→AtomicWaker（~200ns→~50ns） |
| **O47 完成** | ✅ Q9 完成：VTIME 读超时，复用 axtask::future::timeout()（无需 embassy-time） |
| **Embassy 选型边界** | 项目仅用 `embassy_sync::AtomicWaker`，禁用 executor/time/futures 其它子集（L81~L84 教训） |
| **OE1~OE5 反优化** | Channel/Mutex/Watch/Semaphore/select! 替换项目原语全部为反优化，记录在 optimization 已排除区 |

| 阶段 | 内容 | 状态 |
|------|------|------|
| **Q0** | Spike（stride=1 + 寄存器 + ISR + axmm::iomap） | ✅ |
| **Q1** | 驱动架构（ring_buffer + ISR + copier + critical-section） | ✅ |
| **Q2** | VFS 集成（DeviceOps + /dev/async_uart + Console 共存） | ✅ |
| **Q3** | AsyncUart RX 接管（Tty<AsyncUartReader, ConsoleWriter> → Shell stdin） | ✅ |
| **Q4** | 全异步 RX+TX | TX copier 接管，Shell 双向异步 | ✅ |
| **Q5** | 性能优化 | IER 缓存 + ISR 合并 + batch I/O + waker skip + rx/tx 独立锁 | ✅ |
| **Q5.1** | 性能优化续 | NAPI 中断合并 + 批量 API + FCR 阈值日志 + TX interleave 修复 | ✅ |
| **Q5.2** | 测试补全 | 用户态自动化测试 + 非阻塞模式 | ✅ (O43 via Q7) |
| **Q7** | 用户态性能修复 | yield storm + FIONBIO 传播 + benchmark 修正 + tcdrain 真异步 | ✅ |
| **P0** | OpenSpec 文档体系 | 4 spec 域迁移 + `openspec validate --specs` 全通过 | ✅ (2026-06-03) |
| **Q8** | 驱动引擎打磨 | NAPI 退出修复 + ISR 去锁化 + IER 规范化 + 热路径优化 + O46 AtomicWaker 推广 | ✅ |
| **Q9** | 超时机制 | embassy-time 集成（部分无需 Q6） | 📋 计划中 |
| **Q10** | 数据路径优化 | 减少读路径拷贝 + ldisc 锁拆分 + 缓冲扩容 | ✅ |
| **Q11** | 内核通用优化 | tty unwrap + mm/access 批页检查 + sendfile 栈缓冲 + close_range 优化 + ws_col 修复 | ✅ |
| **Q12** | Embassy 路径 A | atomic_ring_buffer 去锁 (O51) + embedded_io_async (O52) + TC tcdrain (O53) | ✅ (2026-06-11) → 🗄️ 归档 2026-06-15 |
| **Q6** | 真板验证 | VisionFive2 | ⏳ |

### 最终架构

```
IRQ 10 → uart_isr_handler
           ├─ RX: disable_rx_intr → RX_WAKER.wake
           └─ TX: disable_tx_intr → TX_WAKER.wake + DRAIN_WAKER.wake

RX copier                     TX copier
  poll_fn:                     poll_fn:
    UART.read FIFO               buf.pop_tx
    buf.push_rx                  UART.write THR
    enable_rx_intr               enable_tx_intr (if partial)
    RX_WAKER.register            TX_WAKER.register
    → Shell stdin ✅             ← Shell stdout ✅

tcdrain (O45): PollSet 等 copier → DRAIN_WAKER 等 UART → 返回
  64B tcdrain 切换 9→6 次，~300µs → ~200µs (QEMU)

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
| `CLAUDE.md` 规则章节 | 三大规则（Karpathy + 务实编码 + Workflow Designer）+ 核心约束 + 技能执行 + 项目特定 + 检查清单 + Red Flags | 7 大节（2026-06-03 整合） |
| `openspec/specs/architecture/spec.md` | ADR-001~031（按主题分组） | 17 Requirements |
| `openspec/specs/learned/spec.md` | API 路径、文件速查、踩坑档案、技巧模式、性能/测试、embassy 选型边界 | 12 Requirements |
| `openspec/specs/references/spec.md` | 依赖、子项目索引、规范、Embassy、Linux serial、项目分析 | 7 Requirements |
| `openspec/specs/optimization/spec.md` | Q5/Q7 已完成 + Q6/远期（含 O46/O47）+ 已排除（含 OE1~OE5）+ 性能基线 | 6 Requirements |
| `openspec/project.md` | 项目上下文（技术栈、约束、目录、Git 规范） | — |
| `CLAUDE.md`（索引部分） | OpenSpec + .claude/docs/ 双索引入口 | 9.7 KB（含规则） |
| `openspec/changes/archive/rules-domain-2026-06-03/` | rules spec 墓碑（17 Requirements） | 🪦 |
| `.claude/docs/tasks.md` | 任务追踪（含 P0 OpenSpec + Q8/Q9 计划） | Q0~Q7 + P0 + Q8/Q9 |
| `.claude/docs/archive.md` | 已归档内容（含 2026-06-03 OpenSpec 迁移 + rules domain 二次迁移） | 持续累积 |
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
