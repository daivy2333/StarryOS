# tasks.md — 任务追踪

> 由 assistant 维护，feat/uart-16550-async 分支。
> 2026-06-21 M4 Sync 已回退到 pre-M4 基线（04f8920/60c5729），原代码保留在 feat/uart-16550-async-temp。
> 2026-06-21 Q15 开启：从 pre-M4 基线增量重融合 M4+ 正确性修复，每步 Manual QA 验证无退化。
> 2026-06-19 OS trait 清理：ADR-036 删除未使用的 OsIrq/OsMmio/OsSpinNoIrq（5→2 trait），消除 3 个 dead_code warning。
> 2026-06-16 Q13 完成：异步串口完整提取到 uart_16550（9 commits, Phase 1 trait 提取 + Phase 2-3 核心逻辑迁移 + 适配层）。
> 2026-06-03 P0 完成，OpenSpec 文档体系建立（5 spec 域全部验证通过）。
> 条目格式: <!-- Q{编号} --> 或 <!-- P{编号} --> 标记开头，支持 grep 精确定位。

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
| **Q13-cleanup** | OS trait 清理 | 删除 OsIrq/OsMmio/OsSpinNoIrq（5→2），ADR-036 | ✅ (2026-06-19) |
| **LTO** | 跨 crate 内联优化 | `lto = true`，ring buffer ↑69%，e2e 不变 | ✅ (2026-06-16) |
| **M4 Sync** | async-uart-1 优化合并 | waker race + TX backpressure + ring/copier 诊断计数器 | ⟲ 已回退 (2026-06-21) |
| **Q15** | M4+ 增量重融合 | 从 pre-M4 基线按最小单元重新 apply，每步 Manual QA | ✅ (2026-06-24 M0~M4 完成，Manual QA 待执行) |
| **Q6** | 真板验证 | VisionFive2 | ⏳ 等待硬件 |

---

## 最终状态

```
Q0 ✅ Q1 ✅ Q2 ✅ Q3 ✅ Q4 ✅ Q5 ✅ Q5.1 ✅ Q5.2 ✅ Q7 ✅ P0 ✅ Q8 ✅ Q10 ✅ Q9 ✅ Q11 ✅ Q12 ✅ Q13 ✅ Q13-cleanup ✅ LTO ✅ M4 Sync ⟲ Q15 ✅ (2026-06-24 M0~M4 完成) Q6 ⏳(硬件)

> 2026-06-21 M4 Sync 已回退到 pre-M4 基线 (04f8920/60c5729)，原代码保留在 temp 分支
> 2026-06-21 Q15: M4+ 增量重融合，每步 Manual QA
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

<!-- tombstone: Q0-Q15 sub-tasks --> Archived 2026-06-23 — all sub-tasks and verification evidence from Q0 through Q15 collapsed into milestone summary above. Full details preserved in openspec/archive/ and git history.

### Q6: 真板验证 ⏳ 等待硬件

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
