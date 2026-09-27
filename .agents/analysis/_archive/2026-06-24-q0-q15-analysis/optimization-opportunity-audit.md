# 异步串口优化机会审计（摘要）

> ⚠️ **STALE [2026-06-17]** — 完整版已归档至 `_archive/optimization-opportunity-audit.md`（10K）
> **Q8~Q11 全部完成（2026-06-11）**，完整记录见 `optimization/spec.md` + `tasks.md`

---

## 核心结论

基于 4 个并行 agent 深度审计，发现 6+ 项未记录的优化机会（含 3 项 🔴 正确性 bug）。**全部已在 Q8~Q11 修复并归档**。

| 维度 | Agent | 发现数 | 状态 |
|------|-------|--------|------|
| UART 驱动内部 | bg_1a2d5d9b | 14 项（含 3 项 🔴 正确性） | Q8 ✅ |
| ldisc/ntty 异步模型 | bg_b5b46482 | 6 项 | Q10 ✅ |
| 全内核优化标记 | bg_6357311e | 142+ 信号 | Q11 ✅ |
| PollSet→AtomicWaker | bg_33bd3d20 | 8 个 PollSet 实例 | Q8.6-9 ✅ |

---

## Q8~Q11 修复汇总（✅ 全部完成）

| 阶段 | 任务 | 状态 | spec 位置 |
|------|------|------|-----------|
| **Q8** | NAPI 退出修复 (Q8.1) + ISR 去锁化 (Q8.2) + IER 规范化 (Q8.3) + copier waker 去重 (Q8.4) + DRAIN_WAKER 条件化 (Q8.5) + O46 8 处 PollSet→AtomicWaker (Q8.6-9) | ✅ | `optimization/spec.md` §Q8 |
| **Q9** | VTIME 读超时（复用 `axtask::future::timeout`，无需 embassy-time） | ✅ | `optimization/spec.md` §Q9 + O47 |
| **Q10** | 减少读路径拷贝 (C3/C4 合并) + ldisc 缓冲扩容 + ldisc 锁拆分 | ✅ | `tasks.md` §Q10 |
| **Q11** | tty unwrap 消除 + mm/access 批量页检查 + sendfile 栈缓冲 + close_range 优化 | ✅ | `tasks.md` §Q11 |

**OpenSpec 归档位置**：
- `openspec/changes/archive/2026-06-11-q8-driver-polish/`
- `openspec/changes/archive/2026-06-11-q9-timeout/`
- `openspec/changes/archive/2026-06-11-q10-data-path-optimize/`
- `openspec/changes/archive/2026-06-11-q11-kernel-optimize/`

---

## 已排除（远期评估 ROI 不足）

- O1/O36 零拷贝 RX（mmap 改动面大）
- O5 协程优先级调度（依赖 axtask 改造）
- O37 kernel log TX 合并（收益不确定）
- O32 poll_fn 闭包优化（编译器可能已优化）
- OE1~OE5 Embassy 包装替换（已验证为反优化）

---

## 当前权威文档（取代本文）

| 主题 | 当前权威文档 |
|------|-------------|
| Q8 修复 | `optimization/spec.md` §Q8 驱动引擎打磨 + `tasks.md` §Q8 |
| Q9/Q10/Q11 | `tasks.md` §Q9/Q10/Q11（已归档指向 openspec/changes/archive/） |
| 性能基线 | `optimization/spec.md` §性能指标基线与硬件理论极限 |

---

**恢复条件**：如需查看 4 个并行 agent 的详细审计数据、Q8.1~Q8.10 子任务表、Q10/Q11 关键文件索引，查阅 `_archive/optimization-opportunity-audit.md`
**生成日期**：2026-06-11（原始）→ 2026-06-17（摘要）
