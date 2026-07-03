# SNAPSHOT.md - 项目快照

> Last updated: 2026-07-03
> 分支：uart-16550-lichee — Q17 QEMU 修复完成，Q19/Q19B 已完成并归档，Q19C 重新探索完成；多 hart / 真板 SMP stress 待 Q20 复验

---

## 当前状态

**分支**: uart-16550-lichee（Lichee RV Dock 适配与验证分支；Q17 QEMU 修复完成，Q19/Q19B 真板验证已完成，Q19C fullbench 重新探索完成；多 hart / 真板 SMP stress 待 Q20 复验）
**前分支**: asyncuart-dev / feat/uart-16550-async（Q0~Q18 历史开发与整合分支）
**成果**:
- kernel 层异步串口适配层（~50 行），uart_16550 提供完整异步栈（~400 行）
- **OpenSpec 文档体系建立**（2026-06-03）：4 个 spec 域（architecture / learned / references / optimization），全部通过 `openspec validate --specs`；rules 已整合到 CLAUDE.md（迁移墓碑见 `openspec/changes/archive/rules-domain-2026-06-03/`）
- 原 `.claude/docs/{architecture,learned,references,optimization,rules}.md` 已迁移至 `openspec/specs/`，源文件以 `.bak` 保留
**Shell**: stdin/stdout 双向异步，`ls`/`cd`/`pwd` 全部正常。

**历史压缩**:
- Q5~Q15 的详细实现、性能数据与回退历史已压缩到 tasks milestone、architecture ADR、learned 和 archived changes；本快照只保留当前状态。恢复入口见 `openspec/changes/archive/2026-07-02-ARC-202607021648/` 与 `openspec/changes/archive/2026-07-02-ARC-202607021535/`。
- Q13 的 active OS abstraction 已由 ADR-036 修正为 `OsRuntime` + `OsWakerSet` 两个 trait；旧 5-trait 设计归档为历史。

**近期完成**:
- **Q17 ✅/⚠️**: QEMU 修复完成；`ier_cache` RMW 临界区化，TX completion 控制流原子序升级，QEMU rootfs benchmark 通过。多 hart / 真板 SMP stress 尚未实测。
- **Q18 ✅**: platform descriptor + early console 分层，QEMU 行为保持。
- **Q19 ✅**: Lichee RV Dock Android boot image + D1 axplat + UART0 polling early console 真板 smoke complete。
- **Q19B ✅**: D1 async UART userbench 真板完成；`/dev/console`、TTY、syscall、`tcdrain`、FIONBIO 全链路通过；大包 TX 达 97.7%~99.0% 线速。
- **Q19/Q19B 归档 ✅**: archived changes 位于 `openspec/changes/archive/2026-07-02-q19-lichee-d1-early-smoke/` 与 `openspec/changes/archive/2026-07-02-q19b-lichee-d1-benchmark/`。

**当前待推进**:
- **Q19C 📝**: OpenSpec change `q19c-lichee-full-starryos-benchmark` 重新探索完成，先做 benchmark evidence cleanup（`benchmark.c` manifest/参数对齐、真板 RX witness、64B 小包优化探索），再推进 memory-root path loader → shell/script parity → SDMMC/block rootfs → true rootfs benchmark；尚未进入源码实现。
- **Q20 ⏳**: VisionFive2 / 等价多 hart 环境到位后，复验 Q17 O63：并发 UART read/write、flush/tcdrain 与 IER enable/disable 无数据丢失或 hang。

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
| **e2e 吞吐量** | ⏳ Q20 VisionFive2 真板验证后回填（QEMU 不仿真串口线延迟，绝对吞吐不可信） |
| **e2e 延迟** | 单字节 139.5 µs avg（硬件理论 86.8 µs，软件开销 52.7 µs） |
| **O46 完成** | ✅ Q8 完成：pipe/signalfd/pidfd/event 共 8 处 PollSet→AtomicWaker（~200ns→~50ns） |
| **O47 完成** | ✅ Q9 完成：VTIME 读超时，复用 axtask::future::timeout()（无需 embassy-time） |
| **Embassy 选型边界** | 项目仅用 `embassy_sync::AtomicWaker`，禁用 executor/time/futures 其它子集（L81~L84 教训） |
| **正确性修复** | ✅ 2026-06-20: uart_16550 12 项审计发现全部修复（2 Critical + 7 High + 3 Medium） |
| **ArceOsRawMutex** | SpinNoIrq-based RawMutex，保护 TX ring writer，支持 Clone-safe AsyncUartWriter |
| **Per-port ISR** | driver.handle_irq() 替代全局 waker + raw MMIO；UartPort 抽象保留 stride/架构语义 |
| **⚠️ 性能退化** | benchmark write+tcdrain 5.4x 开销（29.99ms vs 5.56ms），疑似 RingBufTx Mutex + ISR SpinNoIrq 竞争 |
| **D1 boot image 尺寸** | `DWARF=n` 必须保留；否则 boot image 曾达 `25.6M`，超过官方 boot 分区约 `10.1M` |
| **D1 IrqIf stub** | `irq-if` + no-op `IrqIf` 只解决 axruntime/axtask/axhal 符号需求，不代表 PLIC 已启用 |
| **D1/C906 PTE 属性** | DDR early page table 需要 T-Head normal-memory `SH|B|C` bits 60/61/62；否则 `percpu` AMO 写 `.bss` 会 `Store/AMO access fault` |
| **D1 final page table 属性** | `page_table_entry/xuantie-c9xx` 必须在 `lichee-d1` feature 下启用；否则最终页表切换后可能在 `.bss`/全局数据访问上 fault |
| **D1 virtio 空 MMIO** | D1 无 virtio-mmio，`virtio-mmio-ranges` 必须是空数组 `[]`，不能写成 `[[0,0]]`；否则会访问 `phys_to_virt(0)` |
| **D1 smoke feature gate** | Lichee smoke 阶段必须禁用 fs/net/display/axdriver/PCI/task-ext，直到真实 block/PLIC/TTY 路径实现；否则会触发 `No block device found` 或 PCI 常量缺失 |
| **Q17 QEMU 收尾** | `make run` 默认 `BUS=mmio` 后，rootfs `/bin/benchmark` 通过；64B TX 159.25 KB/s，1B latency avg 0.177ms，FIONBIO 双入口 PASS。该结果只证明单 hart QEMU 功能/性能无明显退化，不证明多 hart 内存序。 |

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
| **Q9** | 超时机制 | VTIME 读超时（复用 axtask::future::timeout，无需 embassy-time） | ✅ (2026-06-11) |
| **Q10** | 数据路径优化 | 减少读路径拷贝 + ldisc 锁拆分 + 缓冲扩容 | ✅ |
| **Q11** | 内核通用优化 | tty unwrap + mm/access 批页检查 + sendfile 栈缓冲 + close_range 优化 + ws_col 修复 | ✅ |
| **Q12** | Embassy 路径 A | atomic_ring_buffer 去锁 (O51) + embedded_io_async (O52) + TC tcdrain (O53) | ✅ (2026-06-11) → 🗄️ 归档 2026-06-15 |
| **Q13** | 异步串口提取 | uart_16550 成为完整异步 UART crate（三阶段迁移） | ✅ (2026-06-16) |
| **LTO** | 跨 crate 内联 | `lto = true`，ring buffer ↑69%，e2e 不变 | ✅ (2026-06-16) |
| **M4 Sync** | async-uart-1 优化合并 | waker race + TX backpressure + ring/copier 诊断计数器 | ⟲ 已回退 (2026-06-21) |
| **Q15** | M4+ 增量重融合 | 从 pre-M4 基线按最小单元重新 apply，每步 Manual QA | ✅ (2026-06-25 M0~M4 + Manual QA 全部完成) |
| **Q16** | Roadmap / spec rebaseline | 任务重排 + stale spec 标注 + validate 噪音记录 | ✅ (2026-06-27) |
| **Q17** | SMP / 内存序正确性 | O63：ier_cache RMW + tx completion 原子序 | ⏳ 待做 |
| **Q18** | 平台参数解耦 / early console 基础 | platform descriptor + QEMU 行为保持 + early console 抽象 | ✅ (2026-06-28) |
| **Q19** | Lichee RV Dock early smoke test | D1 axplat crate + build wiring + Android boot image + UART0 smoke output | ✅ 真板 `[starry-d1] smoke complete` |
| **Q19B** | Lichee D1 async UART benchmark | 模式拆分 + D1 32-bit MMIO UART port + PLIC IRQ + embedded user benchmark image | ✅ 真板 userbench complete |
| **Q19C** | Lichee full StarryOS benchmark | benchmark evidence cleanup + memory-root path loader + shell/script parity + SDMMC/rootfs 探索 | 📝 重新探索 / 待实施 |
| **Q20** | VisionFive2 UART 验证 | O66/O64/O65/O71 + O38/O39 + Q15 Manual QA 真板复跑 | ⏳ 等待硬件 |
| **Q21** | DMA / 高波特率决策 | O3/O40/O69 + O41，依赖 Q20 数据 | ⏳ 等待硬件数据 |
| **Q22** | 维护性清理 | O48/O49/O50 + release LTO 检查 | ⏳ 待做 |
| **Q23** | 远期预研池 | O1/O36、O54/O55、O58/O59、O37 | 🧊 按数据触发 |

### 当前架构（Q15 M0~M4 + Manual QA 已验证 — 2026-06-25）

```
IRQ 10 → ISR handler (uart_init.rs)
            ├─ read_isr() → 识别中断类型
            ├─ RX: UartPort::update_ier 禁用 DATA_READY → RX_WAKER.wake()
            ├─ TX: UartPort::update_ier 禁用 THR_EMPTY → TX_WAKER.wake()
            ├─ TEMT: DRAIN_WAKER.wake()
            └─ Line/Modem: read LSR/MSR to clear source

RX copier (UartPort::update_ier)      TX copier (UartPort::update_ier)
  poll_fn:                              poll_fn:
    UART.read FIFO                        buf.pop_batch
    buf.push_rx                           UART.write THR
    NAPI budget check                     FIFO满→update_ier(THR_EMPTY)→Pending
    register waker                        register waker
    update_ier(DATA_READY)                → Shell stdout ✅
    → Shell stdin ✅

tcdrain/flush: PollFn 等 ring + TEMT (DRAIN_WAKER) → 返回

AsyncUartReader::read → ring_buffer pop
AsyncUartWriter::write → ring_buffer push
Tty<AsyncUartReader, AsyncUartWriter> → /dev/console

内核日志: ax_println! → Console polling TX（共存）
```

> **Q15 已应用架构**（与原 pre-M4 的差异）：per-port `driver.handle_irq()`（替代全局 waker）、`ArceOsRawMutex`（保护 TX ring writer，支持 Clone-safe `AsyncUartWriter`）、`yield_now` 协作让步、`UartPort` 扩展（`update_ier` / `read_isr` / TEMT 检查）、IER 单 owner（`UartPort::update_ier()` 统一管理）。M0~M4 增量已 commit 落地并通过 QEMU Manual QA。

### 历史

> 方向 A（渐进式集成 Console）和方向 B（完全剔除）因 stride=4 + IRQ 风暴在 2026-05 中期放弃，最终采用方向 C（kernel 层独立实现，Q0-Q7 全部完成）。详见 `architecture.md` 和 `docs/analysis/async-uart-implementation-history.md`。

---

## 项目结构

```
StarryOS/
├── kernel/src/
│   ├── config/           # 内核配置
│   ├── drivers/          # StarryOS UART 适配 + benchmark
│   │   ├── mod.rs
│   │   ├── uart_init.rs   # QEMU / platform descriptor UART 初始化
│   │   ├── d1_uart.rs     # D1 DW APB UART 32-bit MMIO + IRQ 18 路径
│   │   ├── os_arceos.rs   # uart_16550 async OS runtime glue
│   │   ├── bench.rs       # 内核态 benchmark 入口
│   │   └── ntty_async.rs  # AsyncTty 类型别名 + lazy_static
│   ├── platform/         # platform descriptor + early console + QEMU/D1/VF2 facts
│   │   ├── descriptor.rs
│   │   ├── early_console.rs
│   │   ├── qemu.rs
│   │   ├── lichee_d1.rs
│   │   └── visionfive2.rs
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
│   ├── specs/            # core domains + archived capability specs
│   │   ├── architecture/spec.md  # ADR-001~031（按主题分组）
│   │   ├── learned/spec.md       # API/文件/踩坑/技巧/性能/测试
│   │   ├── references/spec.md    # 依赖/子项目/规范/Embassy/Linux/分析
│   │   └── optimization/spec.md  # Q0~Q15 完成 + Q16~Q23 roadmap
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
├── crates/
│   ├── uart_16550/       # 本仓库 async UART crate
│   └── axplat-riscv64-lichee-d1/ # Lichee RV Dock D1 axplat
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
| `openspec/specs/architecture/spec.md` | ADR-001~041（按主题分组） | 26 Requirements |
| `openspec/specs/learned/spec.md` | API 路径、文件速查、踩坑档案、技巧模式、性能/测试、embassy 选型边界 | 12 Requirements |
| `openspec/specs/references/spec.md` | 依赖、子项目索引、规范、Embassy、Linux serial、项目分析 | 7 Requirements |
| `openspec/specs/optimization/spec.md` | Q0~Q15 已完成 + Q16~Q23 roadmap + 已排除（含 OE1~OE5）+ 性能基线 | 11 Requirements |
| `openspec/project.md` | 项目上下文（技术栈、约束、目录、Git 规范） | — |
| `CLAUDE.md`（索引部分） | OpenSpec + .claude/docs/ 双索引入口 | 9.7 KB（含规则） |
| `openspec/changes/archive/rules-domain-2026-06-03/` | rules spec 墓碑（17 Requirements） | 🪦 |
| `.claude/docs/tasks.md` | 任务追踪（Q0~Q15 已完成，Q16~Q23 后续 roadmap） | Q0~Q23 |
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
| UART 初始化 | kernel/src/drivers/uart_init.rs | UART 硬件初始化 + IER 缓存 + ArceOsRawMutex |
| OS 适配层 | kernel/src/drivers/os_arceos.rs | ArceOsRawMutex + ArceOsRuntime + ArceOsWakerSet（63 行）|
| AsyncTty | kernel/src/drivers/ntty_async.rs | Tty<ArceOsReader, ArceOsWriter> |
| **uart_16550 crate** | | |
| UartPort trait | uart_16550/src/async_/driver.rs | IRQ-safe 寄存器访问（update_ier, read_isr, read_lsr, read_msr）|
| AsyncUartDriver | uart_16550/src/async_/driver.rs | Per-port waker + handle_irq + copier + NAPI budget |
| RingBufTx | uart_16550/src/async_/ring_buffer.rs | RawMutex 保护 writer，支持 Clone-safe 多 producer |
| ISR (legacy) | uart_16550/src/async_/isr.rs | 已弃用全局 waker，保留兼容 → 新路径: driver.handle_irq() |
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
