# tasks.md — 任务追踪

> 由 assistant 维护，asyncuart-dev 分支。
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
| **Q9** | 超时机制 | embassy-time 集成（O47，部分无需 Q6） | 📋 计划中 |
| **Q10** | 数据路径优化 | 减少读路径拷贝 + ldisc 优化 | ✅ (2026-06-11) |
| **Q11** | 内核通用优化 | mm/access + clone/fd 优化 + unwrap 消除 | 📋 计划中（可选） |
| **Q6** | 真板验证 | VisionFive2 | ⏳ 等待硬件 |

---

## 最终状态

```
Q0 ✅ Q1 ✅ Q2 ✅ Q3 ✅ Q4 ✅ Q5 ✅ Q5.1 ✅ Q5.2 ✅ Q7 ✅ P0 ✅ Q8 ✅ Q10 ✅ Q9 📋 Q11 📋 Q6 ⏳(硬件)
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

### Q6: 真板验证 ⏳ 等待硬件

<!-- Q6.1 --> - [ ] O38 VisionFive2 UART 时钟适配
<!-- Q6.2 --> - [ ] O39 真实硬件 FIFO 深度验证
<!-- Q6.3 --> - [ ] O3/O40 DMA 通道发现与配置
<!-- Q6.4 --> - [ ] O41 高速波特率支持（>115200）
<!-- Q6.5 --> - [ ] Gate Q6: 真板正常运行

### Q8: 驱动引擎打磨 ✅ (2026-06-11)

> 2026-06-11 完成。7 个并行 Agent 分 2 个 Wave 执行，QEMU 实机验证通过。13 文件变更（+235/-70）+ uart_16550（+12 行）。

**Wave 1 — 正确性修复（必须先做）**：

| 子任务 | 描述 | 关键文件 | 类型 |
|--------|------|----------|------|
| **Q8.1** | NAPI 退出修复 | `async_driver.rs:51` — total==0 时重置 consecutive+enable_rx_intr | 🔴 BugFix |
| **Q8.2** | ISR 去锁化 | `isr.rs:10` — 消除 SpinNoIrq 锁，实现无锁 ISR 路径 | 🔴 BugFix |
| **Q8.3** | IER 写路径规范化 | `uart_init.rs:72` — 用 uart_16550 API 替代裸 write_volatile | 🔴 BugFix |
| **Q8.3a** | uart_16550 添加 set_ier() | `uart_16550/src/lib.rs` — 暴露 IER 写入 API | 🔴 依赖 Q8.3 |

**Wave 2 — 热路径优化**：

| 子任务 | 描述 | 关键文件 | 预期收益 |
|--------|------|----------|----------|
| **Q8.4** | copier waker 去重简化 | `async_driver.rs:53-55` — 仅 will_wake 不同时才 clone+register | ~20-40ns/poll |
| **Q8.5** | DRAIN_WAKER 条件唤醒 | `isr.rs:20` — 仅在 tcdrain 活跃时 wake | 减少无意义原子操作 |

**Wave 3 — O46 AtomicWaker 推广（按风险从低到高）**：

| 子任务 | 描述 | 关键文件 | 风险 |
|--------|------|----------|------|
| **Q8.6** | signalfd PollSet→AtomicWaker | `signalfd.rs` — 1 PollSet → 1 AtomicWaker | 🟢 低 |
| **Q8.7** | event PollSet→AtomicWaker | `event.rs` — 2 PollSet → 2 AtomicWaker | 🟡 中 |
| **Q8.8** | pipe PollSet→AtomicWaker | `pipe.rs` — 3 PollSet → 3 AtomicWaker（交叉唤醒） | 🟡 中 |
| **Q8.9** | pidfd PollSet→AtomicWaker | `pidfd.rs` + `task/mod.rs` + `task/ops.rs` — Arc 共享重构 | 🔴 高 |
| **Q8.10** | 性能回归测试 | 新增/更新 benchmark，对比 Q5.1 基线 | — |
| **Q8.11** | Gate Q8 | cargo test + clippy + benchmark 全部通过 | — |

**实施风险**：
- Q8.9 pidfd 的 exit_event 是 `Arc<PollSet>` 共享于 Thread/ProcessData 中，修改影响 3 个文件的类型定义 + 唤醒路径
- Q8.8 pipe 的跨操作唤醒（read→wakeTX, write→wakeRX）需要 3 个独立 AtomicWaker
- Q8.3 需要修改 `uart_16550` crate，需评估对 StarryOS 的影响

**预期总收益**：
- 唤醒延迟：~200ns → ~50ns（pipe/signalfd/pidfd/event 共 8 个唤醒点）
- ISR 延迟降低 ~200ns（去锁化）
- NAPI 空闲 CPU 归零
- 消除 1 处规则违规（IER 裸写）、1 处锁违规（ISR 锁）

### Q9: 超时机制 📋 计划中

> 基于 O47（2026-06-05 记录于 `optimization/spec.md`），引入 embassy-time 修复 `block_on(poll_io(...))` 永久阻塞问题。
> **2026-06-11 更新**：Q9.1~Q9.3（time driver 基础设施）无需 Q6 硬件，可先行完成。

| 子任务 | 描述 | 前置 | 硬件依赖 |
|--------|------|------|----------|
| **Q9.1** | axhal time driver 评估 | — | 无 |
| **Q9.2** | 引入 embassy-time 依赖 | Q9.1 | 无 |
| **Q9.3** | poll_io 接受 `Option<Duration>` | Q9.2 | 无 |
| **Q9.4** | select! 组合 poll_io + Timer | Q9.3 | Q6.3（DMA 失败路径确认） |
| **Q9.5** | 用户态 SO_RCVTIMEO 支持 | Q9.4 | Q6 真板验证 |
| **Q9.6** | Gate Q9 | cargo test + 真板超时测试 | — |

**触发条件**：Q6.3（DMA 通道发现与配置）完成后评估是否真的需要 timeout。如果 DMA 在真板有失败保护（如硬件 timeout），Q9.4+ 可能不需要实现。

### Q10: 数据路径优化 📋 计划中

> 2026-06-11 优化审计新发现。减少用户态串口读路径拷贝次数，优化 ldisc 层并发性能。
> 详细分析见 `.claude/analysis/optimization-opportunity-audit.md`

| 子任务 | 描述 | 关键文件 | 预期收益 |
|--------|------|----------|----------|
| **Q10.1** | 合并 C3/C4 拷贝 | `ldisc.rs:83-90` — InputReader::poll 中合并 buf→ringbuf 两次拷贝为一次 | 每字节减 1 次 memcpy |
| **Q10.2** | ldisc 缓冲扩容 | `ldisc.rs` — StaticRb 80→256（或可配置） | 突发吸收能力提升 |
| **Q10.3** | ldisc 锁拆分 | `tty/mod.rs:88` — read() 中 ldisc 锁不跨越 block_on，改用内部可变性 | poll/select 并发不阻塞 |
| **Q10.4** | 性能基准重测 | benchmark 对比 Q5.1 数据 | 量化拷贝减少 + 锁拆分的收益 |
| **Q10.5** | Gate Q10 | cargo test + benchmark 通过 | — |

**关键依赖**：Q10.3（锁拆分）需要仔细分析 ldisc 的并发安全，可能涉及 ringbuf 内部改为 lock-free 或细粒度锁。

### Q11: 内核通用优化 📋 计划中（可选，非 UART 特定）

> 2026-06-11 全内核优化审计发现。非 UART 特定的通用优化，优先级低于 Q8~Q10。

| 子任务 | 描述 | 关键文件 | 类型 |
|--------|------|----------|------|
| **Q11.1** | tty unwrap() 消除 | `tty/mod.rs:78,160,170` — Weak::upgrade().unwrap() → 错误传播 | 🟡 Safety |
| **Q11.2** | mm/access 批量页检查 | `mm/access.rs:88` — 消除逐页 aspace 锁 | 🟡 Perf |
| **Q11.3** | sendfile 缓冲区复用 | `syscall/fs/io.rs:264` — vec![0;4096] → 静态缓冲 | 🟡 Perf |
| **Q11.4** | close_range UNSHARE 优化 | `syscall/fs/fd_ops.rs:167` — 避免全表 clone | 🟡 Perf |
| **Q11.5** | Gate Q11 | cargo test + clippy 通过 | — |

> Q11 可推迟到 Q6 之后或作为独立的小任务穿插执行。

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

### 新发现的架构问题（2026-06-01 性能分析）

1. **用户态性能上限是波特率**：115200 bps = 11.52 KB/s，异步在吞吐量上不可能超越阻塞 Console
2. **Async RX 多一次拷贝**：UART FIFO → ring buffer → ldisc buf → user buf（3 次 vs Console 的 2 次）
3. **ProcessMode::Manual 在空闲时产生 yield storm**：waker.wake_by_ref() 导致 yield-re-schedule 循环
4. **FIONBIO 对 TTY 不生效**：nonblocking 标志未传播到 Tty/ldisc 层
5. **benchmark.c 不测真实 UART 吞吐量**：TX 测试写 /dev/null，不经过 UART
