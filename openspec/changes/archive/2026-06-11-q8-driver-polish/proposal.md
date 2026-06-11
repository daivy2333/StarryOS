# Q8 驱动引擎打磨

> Created: 2026-06-11
> Source: `.claude/analysis/optimization-opportunity-audit.md`（4 个并行 agent 深度扫描）
> Refs: L150~L155（learned）、O46（optimization）

## 为什么做

2026-06-11 优化审计发现 UART 驱动层存在 **3 项正确性 bug 和多项性能优化机会**，均可在硬件到之前修复：

| # | 发现 | 严重度 |
|---|------|--------|
| 1 | NAPI 模式永不退出 — consecutive 只增不减 | 🔴 Bug |
| 2 | ISR 获取 SpinNoIrq 锁 — 违反 ISR 极简原则 | 🔴 违规 |
| 3 | IER 裸 write_volatile — 绕过 uart_16550 API | 🔴 违规 |
| 4 | copier waker 去重过度 — 每 poll 2×clone | 🟡 Perf |
| 5 | DRAIN_WAKER 无条件唤醒 | 🟡 Perf |
| 6-9 | PollSet→AtomicWaker — pipe/signalfd/pidfd/event | 🟡 Perf |

## 做什么

分 3 个 Wave 实施，优先级从高到低：

**Wave 1 — 正确性修复（必须先做）：** Q8.1 NAPI 退出 / Q8.2 ISR 去锁 / Q8.3 IER 规范化

**Wave 2 — 热路径优化：** Q8.4 waker 简化 / Q8.5 DRAIN_WAKER 条件化

**Wave 3 — O46 AtomicWaker 推广：** Q8.6 signalfd / Q8.7 event / Q8.8 pipe / Q8.9 pidfd / Q8.10 benchmark / Q8.11 Gate

## 预期收益

- NAPI 空闲 CPU 归零
- ISR 延迟降低 ~200ns
- 唤醒延迟 ~200ns → ~50ns（8 个唤醒点）
- 消除 1 处规则违规（IER 裸写）、1 处锁违规（ISR 锁）

## BDD 缺口处理

用户选择"用默认假设补充"：
- G1: NAPI 无降温期，数据恢复后快速重入
- G2: ISR 中 isr() 是 read_volatile，单 ISR 无竞态
- G3: 本地 path 依赖修改 uart_16550
- G4: pidfd async 模型保证单 waiter
- G5: AtomicBool 标志位判断 tcdrain 等待状态

## 回滚方案

每个 Wave 独立可回滚（git revert 单次提交），不影响已有 Q0~Q7 功能。
