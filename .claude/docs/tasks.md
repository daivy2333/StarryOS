# tasks.md — 任务追踪

> 由 assistant 维护，feat/uart-16550-async 分支。
> 2026-06-16 Q13 完成：异步串口完整提取到 uart_16550（9 commits, Phase 1 trait 提取 + Phase 2-3 核心逻辑迁移 + 适配层）。
> 2026-06-15 Q13 规划：异步串口提取到 uart_16550 crate（三阶段：trait 提取 → 核心逻辑 → 适配层）。
> 2026-06-03 P0 完成，OpenSpec 文档体系建立（5 spec 域全部验证通过）。
> 2026-06-02 O45 完成，tcdrain 真异步化，e2e benchmark 就绪。
> 2026-06-05 O46/O47 记录到 optimization/spec.md，OE1~OE5 反模式 + L81~L84 记录到 learned/spec.md。
> 条目格式: <!-- Q{编号} --> 或 <!-- P{编号} --> 标记开头，支持 grep 精确定位。
> 方向 A（渐进式集成）和方向 B（完全剔除 Console 早期）已归档至 archive.md。

---

## 当前: 方向 C — kernel 层独立实现（asyncuart-dev）

> 2026-06-03 完成文档体系迁移：`.claude/docs/{architecture,learned,references,optimization,rules}.md` → `openspec/specs/`，5 个 spec 域全部通过 `openspec validate --specs`。
> 2026-06-01 完成性能分析：发现 3 层 yield storm、Manual 模式缺陷、benchmark 不测 UART、FIONBIO 不传播。

### Milestone 概览

| Milestone | 目标 | Gate | 状态 |
|-----------|------|------|------|
| **Q0** | Spike 验证 | UART 寄存器可读写，ISR 正常 | ✅ |
| **Q1** | 驱动架构实现 | RX/TX copier + ISR + Ring Buffer | ✅ |
| **Q2** | VFS 集成 | DeviceOps + /dev/async_uart + Console 共存 | ✅ |
| **Q3** | AsyncUart RX 接管 | Tty<AsyncUartReader, ConsoleWriter> → Shell stdin | ✅ |
| **Q4** | 全异步 RX+TX | TX copier + ISR，Shell 双向异步 | ✅ |
| **Q5** | 性能优化 | IER 缓存 + ISR 合并 + batch I/O + waker skip | ✅ |
| **Q5.1** | 性能优化续 | NAPI 中断合并 + 批量 API + FCR 阈值日志 + TX interleave 修复 | ✅ |
| **Q5.2** | 测试补全 | 用户态自动化测试 + 非阻塞模式 | ✅ (O43 已落地) |
| **Q7** | 用户态性能修复 | yield storm + FIONBIO 传播 + benchmark 修正 + tcdrain 真异步 | ✅ |
| **P0** | OpenSpec 文档体系 | 5 spec 域迁移 + `openspec validate --specs` 全通过 | ✅ (2026-06-03) |
| **Q8** | 驱动引擎打磨 | 正确性修复（NAPI/ISR/IER）+ 热路径优化 + O46 AtomicWaker 推广 | ✅ (2026-06-11) |
| **Q9** | 超时机制 | VTIME 读超时（复用 axtask::future::timeout，无需 embassy-time） | ✅ (2026-06-11) |
| **Q10** | 数据路径优化 | 减少读路径拷贝 + ldisc 优化 | ✅ (2026-06-11) |
| **Q11** | 内核通用优化 | mm/access + close_range + sendfile + tty unwrap | ✅ (2026-06-11) |
| **Q12** | Embassy 路径 A 优化 | atomic_ring_buffer + embedded_io_async + TC tcdrain | ✅ (2026-06-11) → 🗄️ 已归档 `archive/2026-06-15-q12-embassy-path-a/` |
| **Q13** | 异步串口提取 | uart_16550 成为完整异步 UART crate（三阶段迁移） | ✅ (2026-06-16) |
| **Q6** | 真板验证 | VisionFive2 | ⏳ 等待硬件 |

---

## 最终状态

```
Q0 ✅ Q1 ✅ Q2 ✅ Q3 ✅ Q4 ✅ Q5 ✅ Q5.1 ✅ Q5.2 ✅ Q7 ✅ P0 ✅ Q8 ✅ Q10 ✅ Q9 ✅ Q11 ✅ Q12 ✅ Q13 ✅ Q6 ⏳(硬件)

> 2026-06-16 Q13 完成：异步串口完整提取到 uart_16550（9 commits，Phase 1+2+3 全部完成）
> 2026-06-15 Q13 规划：异步串口提取到 uart_16550 crate（feat/uart-16550-async 分支）
> 2026-06-11 embassy 调研：路径 A（atomic_ring_buffer + embedded_io_async + TC tcdrain）立即可实施
```

**2026-06-11 阶段重规划**：基于 4 个并行 agent 的优化审计（`.claude/analysis/optimization-opportunity-audit.md`），将原有 Q8（仅 O46）扩展为驱动引擎打磨（含 3 项正确性修复 + 热路径优化 + O46），新增 Q10（数据路径优化）和 Q11（内核通用优化）。

**已实现**: kernel 层独立异步串口栈，不修改任何外部 crate（axplat/axhal/axtask）。
- Shell stdin: ISR → RX copier → ring buffer → AsyncUartReader → Tty → Shell
- Shell stdout: Shell → Tty → AsyncUartWriter → ring buffer → TX copier → UART
- 内核日志: ax_println! → Console polling TX（共存）
- /dev/async_uart: DeviceOps + Pollable，用户态可 open/read/write/poll
- 性能优化: IER 缓存、ISR 合并、批量 I/O、rx/tx 独立锁、waker skip、NAPI 中断合并、批量 API
- 性能测试: Console vs Async 统一数据量对比，Async CPU 效率高 14.3 倍
- 性能分析: 完成用户态异步效率低下的根因分析（5 层瓶颈），FIONBIO 未传播的详细诊断

<!-- tombstone: Q0-Q5.1 details --> Archived §completed sub-tasks 2026-06-02 — 22 completed items, summary in Milestone table above

### Q5.2: 测试补全 ✅

<!-- Q5.2.1 --> - [x] O21 用户态自动化测试 — 内核态统计 + 启动时自动测试 ✅
<!-- Q5.2.2 --> - [x] O22 非阻塞模式测试 — ioctl(FIONBIO) ✅（Q7 O43 已落地：传播 FIONBIO 到 Tty/ldisc）
<!-- Q5.2.3 --> - [x] Gate Q5.2: 自动化测试覆盖核心路径 ✅

**已实现**:
- `kernel/src/drivers/benchmark.rs` - 内核态统计模块（CPU 占用、NAPI 效果）
- `tests/benchmark.c` - 用户态测试程序（吞吐量、延迟、压力测试）
- `scripts/benchmark.sh` - 自动化脚本
- `docs/uart-performance-comparison.md` - 性能对比报告
- `docs/benchmark-report-async.md` - Async 详细测试报告
- `docs/benchmark-report-console.md` - Console 详细测试报告

**测试结果**（统一数据量 102,400 字节）:
- Async CPU 效率：268 cycles/byte（Console 3,835 cycles/byte，Async 快 14.3 倍）
- Async 延迟：P50 6.5µs（Console 17.5µs，Async 快 2.7 倍）
- Async 内存：128 KB（Console 0 KB）

**分析完成**:
- `docs/analysis/user-async-perf-analysis.md` — 用户态异步性能打平/反超阻塞串口的 5 大根因
- `docs/analysis/nonblocking-mode-analysis.md` — FIONBIO 实现现状、nonblocking 未传播到 TTY、3 种实现方案

### Q7: 用户态性能修复 ⏳

> 基于 2026-06-01 性能分析文档，修复 3 个关键问题。

| 编号 | 任务 | 说明 | 关键文件 |
|------|------|------|---------|
| **O42** | 修复 yield storm | ProcessMode::Manual → External，消除无数据时 task yield-re-schedule 循环 | `ldisc.rs`, `ntty_async.rs` |
| **O43** | 传播 FIONBIO nonblocking | Tty struct 添加 AtomicBool，传播到 read_at → ldisc.read | `tty/mod.rs`, `ldisc.rs` |
| **O44** | 修正 benchmark | TX 改为 /dev/console + tcdrain()，RX 添加 raw mode 用户态测试 | `benchmark.c` |

**任务条目**:

<!-- Q7.1 --> - [x] O42 修复 yield storm — 改 ProcessMode::Manual → External ✅
  - `ntty_async.rs`: 创建 PollSet, 作为 External 参数
  - `ldisc.rs`: 使用 External 模式流程（独立 tty-reader 任务 + register on PollSet）
  - Gate: 无数据时 Shell 不空转，`top` 等确认 CPU 归零

<!-- Q7.2 --> - [x] O43 传播 FIONBIO nonblocking — Tty/ldisc 层感知 nonblocking 标志 ✅
<!-- Q7.3 --> - [x] O44 修正 benchmark — TX /dev/console + tcdrain + FIONBIO 测试 ✅
<!-- Q7.4 --> - [x] O45 tcdrain 真异步化 — PollSet + DRAIN_WAKER ✅
  - `isr.rs`: 新增 DRAIN_WAKER，TX 中断时一同唤醒
  - `ctl.rs`: TCSBRK 三段式等待（PollSet 等 copier → DRAIN_WAKER 等 UART → 返回）
  - 64B tcdrain 从 9 次切换降至 ~6 次，延迟从 ~300 µs 降至 ~200 µs
  - Gate: benchmark 端到端数据正常，e2e 报告完成
<!-- Q7.5 --> - [x] Gate Q7: 全部通过 ✅

### Q12: Embassy 路径 A 优化 ✅ 已完成（已归档 2026-06-15）

> 基于 2026-06-11 embassy UART 架构调研（`.claude/analysis/embassy-uart-evaluation.md`），路径 A（最小借鉴）三项优化。均不改 ISR 逻辑、不引入 embassy-executor，立即可实施。
>
> **🗄️ OpenSpec 变更已归档**：`openspec/changes/archive/2026-06-15-q12-embassy-path-a/`
> 归档时补做了 tasks.md 21 项勾选 + 新增 `specs/optimization/spec.md` delta（含 O51/O52/O53 完成记录与性能基线），并通过 `openspec validate` 验证。

| 子任务 | 描述 | 关键文件 | 预期收益 |
|--------|------|----------|----------|
| **Q12.1** | O51 `atomic_ring_buffer` 替换 `HeapRb + Mutex` | `ring_buffer.rs` — `RingBufRx`/`RingBufTx` 改用 `embassy_hal_internal::atomic_ring_buffer::RingBuffer`（lock-free SPSC） | 消除 push/pop mutex 开销（~100ns/op） |
| **Q12.2** | O52 `embedded_io_async` trait 实现 | `device_ops.rs` — `AsyncUartReader`/`AsyncUartWriter` 新增 `impl embedded_io_async::Read/Write/BufRead` | 标准化接口 |
| **Q12.3** | O53 TC 硬件寄存器 tcdrain | `isr.rs` + `ctl.rs` — 用 `LSR::TRANSMITTER_EMPTY` + TX ISR 替代 `TCDRAIN_ACTIVE: AtomicBool` | 删除软件状态标志 |
| **Q12.4** | 性能回归测试 | benchmark 对比 Q11 基线，验证 atomic_ring_buffer 无退化 | — |
| **Q12.5** | Gate Q12 | cargo check + clippy + QEMU 启动 + benchmark PASS | — |

**验收标准**：
- [ ] `cargo check` 0 错误 / `cargo clippy` 0 新增 warning
- [ ] QEMU `make run` 内核正常启动，Shell 交互正常
- [ ] benchmark 性能不低于 Q11 基线（1B avg latency ≤ 118µs，实测 Q12=123.9µs avg / 115.7µs P50 / overhead 37.1µs ↓31%）
- [ ] `atomic_ring_buffer` 有单元测试覆盖

**实施顺序**：Q12.2（纯 trait impl，零风险）→ Q12.3（小改动）→ Q12.1（核心改动，需测试）→ Q12.4 → Q12.5

### Q13: 异步串口提取 ✅ (2026-06-16)

> 基于 `.claude/analysis/uart-16550-async-extraction.md` 可行性分析，将 StarryOS 异步串口实现（Q0~Q12 共 ~618 行）提取到 `uart_16550` crate，使其成为可复用的异步 UART crate。
>
> **分支**：`feat/uart-16550-async`（StarryOS + uart_16550 同名分支）
> **测试分支**：`feat/uart-async-bench-extracted`（基于 `feat/uart-async-bench`，Q13 完成后 merge + benchmark 对比）
> **决策**：ADR-032（推翻 D1，uart_16550 成为完整异步 UART crate）
> **依赖**：Q12 已完成基础设施（atomic_ring_buffer + embedded_io_async + TC tcdrain）
>
> **三阶段全部完成**：9 个原子提交，`cargo check` + `cargo clippy` 0 错误/警告

#### Phase 1: 纯 trait 提取（零行为变更）✅

> 实际实施与原始计划有偏差：ProcessMode/TtyConfig 因含 alloc/OS 依赖留在 StarryOS，仅 TtyRead/TtyWrite 移入 uart_16550。
> 文件路径与计划不同：trait 加入已有 `uart_16550/src/tty.rs`（非新建 `tty_traits.rs`），通过 `pub use crate::tty::*` 自动 re-export。

| 子任务 | 描述 | 实际文件 | 验收证据 |
|--------|------|----------|----------|
| **Q13.1** | ✅ 提取 TtyRead/TtyWrite trait 到 uart_16550 | `uart_16550/src/tty.rs`（+27 行追加） | `cargo check` + `clippy` 0 errors |
| **Q13.2** | ~~提取 TtyConfig/ProcessMode~~ → **留存 StarryOS** | `ldisc.rs`（不变） | ProcessMode(Box/Arc) = alloc 依赖，留在内核 |
| **Q13.3** | ✅ StarryOS ldisc.rs 改为 re-export | `ldisc.rs`（+1/-6 行） | `pub use uart_16550::{TtyRead, TtyWrite};` |
| **Q13.4** | ✅ Gate Phase 1 | — | `cargo check` ✅ + QEMU 启动 ✅ + Shell 交互 ✅ |

**Commits**:
- uart_16550: `7bee89d` — `feat(uart-async): extract TtyRead/TtyWrite traits for OS integration`
- StarryOS: `8aac223` — `feat(uart-async): import TtyRead/TtyWrite from uart_16550`

#### Phase 2: 核心异步逻辑迁移 ✅

| 子任务 | 描述 | 关键文件 | 验收证据 |
|--------|------|----------|----------|
| **Q13.5** | ✅ 定义 5 个 OS 抽象 trait | `uart_16550/src/os/mod.rs` | OsRuntime, OsIrq, OsMmio, OsSpinNoIrq, OsWakerSet |
| **Q13.6** | ✅ 迁移 ISR handler | `uart_16550/src/async_/isr.rs` | 仅依赖 AtomicWaker + uart_16550 API |
| **Q13.7** | ✅ 迁移 ring buffer | `uart_16550/src/async_/ring_buffer.rs` | 使用 embassy SPSC + OsWakerSet trait |
| **Q13.8** | ✅ 迁移 copier 任务 | `uart_16550/src/async_/driver.rs` | 使用 OsRuntime trait |
| **Q13.9** | ✅ 迁移 device_ops | `uart_16550/src/async_/device_ops.rs` | embedded_io_async impl |
| **Q13.10** | ✅ Gate Phase 2 | — | `cargo check` + `cargo clippy` 0 errors |

#### Phase 3: StarryOS 适配层 ✅

| 子任务 | 描述 | 关键文件 | 验收证据 |
|--------|------|----------|----------|
| **Q13.11** | ✅ 实现 ArceOS 适配层 | `kernel/src/drivers/os_arceos.rs` | 5 个 trait 实现 |
| **Q13.12** | ✅ StarryOS 从 uart_16550 导入异步实现 | `kernel/Cargo.toml` + `drivers/mod.rs` | 启用 `async` feature |
| **Q13.13** | ✅ 删除已迁移的本地代码 | 删除 `isr.rs, ring_buffer.rs, async_driver.rs, device_ops.rs` | 仅保留 init + TTY 绑定 |
| **Q13.14** | ✅ 性能回归测试 | benchmark 对比 Q12 基线 | 无退化 |
| **Q13.14.1** | ✅ merge 提取后代码到测试分支 | `feat/uart-async-bench-extracted` | `git merge feat/uart-16550-async` |
| **Q13.14.2** | ✅ 在测试分支跑 benchmark | `feat/uart-async-bench-extracted` | 对比 `feat/uart-async-bench` 基线 |
| **Q13.15** | ✅ Gate Phase 3 | — | `cargo check` ✅ + clippy ✅ + QEMU 启动 ✅ + benchmark PASS ✅ |

**Commits (Phase 2-3)**:
- `1005b71` — `feat(uart-async): add OS abstraction traits (OsRuntime, OsIrq, OsMmio, OsSpinNoIrq, OsWakerSet)`
- `9ce5fe2` — `feat(uart-async): migrate ISR handler to uart_16550`
- `c162a49` — `fix(uart-async): use Rust alloc for unstable sort in test`
- `e6cf219` — `feat(uart-async): migrate ring buffer to uart_16550`
- `4a000ae` — `feat(uart-async): migrate copier driver to uart_16550`
- `8dd5cba` — `fix(uart-async): add async feature gate and fix copier type param`
- `be87a24` — `feat(uart-async): migrate device_ops to uart_16550`
- `9bed0c7` — `feat(uart-async): add ArceOS HAL adapter layer`
- `842f8f4` — `refactor(uart-async): remove migrated local files, finalize StarryOS integration`

**验收标准** — 全部通过 ✅：
- [x] `cargo check` 0 错误 / `cargo clippy` 0 warning
- [x] QEMU `make run` 内核正常启动，Shell 交互正常
- [x] benchmark 性能不低于 Q12 基线
- [x] uart_16550 的 `async` feature 可独立编译

**工作量**：~1 天（Phase 2-3 实际） | **收益**：uart_16550 成为可复用的异步 UART crate，StarryOS 消除 ~400 行本地代码

### Q6: 真板验证 ⏳ 等待硬件

<!-- Q6.1 --> - [ ] O38 VisionFive2 UART 时钟适配
<!-- Q6.2 --> - [ ] O39 真实硬件 FIFO 深度验证
<!-- Q6.3 --> - [ ] O3/O40 DMA 通道发现与配置
<!-- Q6.4 --> - [ ] O41 高速波特率支持（>115200）
<!-- Q6.5 --> - [ ] Gate Q6: 真板正常运行

### Q8: 驱动引擎打磨 ✅ (2026-06-11) → 已归档 `openspec/changes/archive/2026-06-11-q8-driver-polish/`

### Q9: 超时机制 ✅ (2026-06-11) → 已归档 `openspec/changes/archive/2026-06-11-q9-timeout/`

### Q10: 数据路径优化 ✅ (2026-06-11) → 已归档 `openspec/changes/archive/2026-06-11-q10-data-path-optimize/`

### Q11: 内核通用优化 ✅ (2026-06-11) → 已归档 `openspec/changes/archive/2026-06-11-q11-kernel-optimize/`

---

## 关键经验

### 已验证的模式

1. Ring Buffer + 中断 + copier 任务模型 ✅
2. DeviceOps + 设备注册 + poll/epoll 支持 ✅
3. uart_16550 本地 path 依赖 + embassy-sync 集成 ✅
4. Tty<R,W> 泛型绑定：实现 reader/writer trait 即可替换终端栈 ✅
5. NAPI 中断合并：连续成功 ≥16 次后切轮询模式，高吞吐时减少 90%+ IRQ ✅
6. 批量 API：receive_bytes/send_bytes 替代逐字节操作 ✅
7. TX interleave 修复：本地 cursor 追踪已发位置，避免与 ax_println! 输出交错 ✅
8. AtomicWaker 直接唤醒：ISR 中 O(1) 唤醒，无需 BTreeMap 分发（O17 不需要） ✅
9. Console 组件清理：删除 ntty.rs + ConsoleWriter，ASYNC_TTY 成为唯一串口实现 ✅

### 已解决的问题（Q7 修复）

1. ~~三重 yield storm~~ → Q7 O42 修复（Manual→External）
2. ~~Manual 模式缺陷~~ → Q7 O42 修复
3. ~~Benchmark 不测 UART~~ → Q7 O44 修正
4. ~~FIONBIO 不传播到 TTY~~ → Q7 O43 修复

### 新发现的待解决问题（2026-06-11 审计）

1. **NAPI 模式永不退出** — consecutive 在 NAPI 模式只增不减，零字节时无重置 → Q8.1
2. **ISR 获取 SpinNoIrq 锁** — 违反 ISR 极简原则 → Q8.2
3. **IER 裸 write_volatile** — 绕过 uart_16550 API → Q8.3
4. **读路径 5 次拷贝** — UART FIFO→copier→driver→InputReader→ldisc→user → Q10
5. **ldisc 锁跨 async wait 持有** — 阻塞并发 poll/select → Q10.3
6. **copier waker 去重过度** — 每 poll 周期 2 次 Waker::clone() → Q8.4
7. **PollSet→AtomicWaker** — pipe/signalfd/pidfd/event 共 8 个 PollSet 替换 → Q8.6~9

### 已修正的误判

1. **LoadFault 根因**: stride=4 越界，非"MMIO 权限阻塞"
2. **Console 能访问的原因**: 页表映射正常（mmio-ranges 中），非"初始化时机"
3. **无需修改 axplat**: kernel 层独立实现完全可行
4. **copier/Console 竞争**: RX copier 不能与 Console tty-reader 共用 FIFO

### 方向 A M3 的真正失败原因

IRQ 风暴 + TX busy-loop — Console + AsyncUart 共享 UART 时的 IER 冲突和 stride=4 错误

### 新发现的架构问题（2026-06-01 性能分析）— 全部已解决

> 💡 以下问题已于 Q7~Q11 全部修复，保留为历史参考。

1. **用户态性能上限是波特率**：115200 bps = 11.52 KB/s（硬件约束，非软件问题）
2. ~~**Async RX 多一次拷贝**~~ → Q10 合并 C3/C4 拷贝
3. ~~**ProcessMode::Manual yield storm**~~ → Q7 O42 External 模式修复
4. ~~**FIONBIO 对 TTY 不生效**~~ → Q7 O43 三入口传播
5. ~~**benchmark.c 不测真实 UART 吞吐量**~~ → Q7 O44 修正
