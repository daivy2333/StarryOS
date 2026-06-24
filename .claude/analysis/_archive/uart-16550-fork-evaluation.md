# uart_16550 本地 Fork 必要性评估（摘要）

> ⚠️ **STALE [2026-06-17]** — 完整版已归档至 `_archive/uart-16550-fork-evaluation.md`（8K）

---

## 核心决策

本地 fork 仅含一个 **12 行**的 `set_ier()` 方法。**建议短期保留 fork**（成本可忽略），**中期提交 PR 上游化**（symmetric API 补全，merge 概率极高）。**无需等待硬件**。

| 决策 | 状态 |
|------|------|
| **当前**：保留本地 fork | ✅ 已落地（基于 v0.6.0） |
| **中期**：提交 PR 上游 → 切换 crates.io | 🔍 评估中（无时间窗口要求） |

---

## 关键事实

| 维度 | 详情 |
|------|------|
| **唯一代码变更** | `set_ier()` 方法（`src/lib.rs:820-830`，12 行） |
| **必要性** | 缺少会导致 MMIO 规则违规（裸 `write_volatile`）— Q8.3 专门修复 |
| **调用模式** | ISR 禁用中断 → copier 处理完毕 → 重新启用中断（每次 Shell 交互数十次 IER 切换） |
| **上游版本** | v0.6.0（commit `68b0be4`，2026-03-28）|
| **维护者** | @phip1611（活跃，2026-04-02 最近 push）|
| **上游化可行性** | 极高（symmetric API 补全，与现有 `ier()` getter 对称） |

---

## 为何不能绕过 `set_ier()`

| 替代方案 | 可行性 | 问题 |
|---------|:---:|------|
| `uart.init(Config { interrupts: ... })` | ❌ | 重新初始化全部配置，破坏运行时状态，drain TX 阻塞 |
| 裸 `write_volatile` | ❌ | **违反项目规则**（CLAUDE.md §六 ISR 极简原则） |
| 访问 `backend` 字段 | ❌ | `backend` 是私有字段，`Backend` trait 已 sealed |
| 上游添加 `set_ier()` | ✅ | 12 行，对称 API 补全 |

---

## Q13 后的状态变化

Q13 完成后（2026-06-16），`set_ier()` 仍是必需的，但**调用位置变化**：
- 之前：StarryOS `kernel/src/drivers/uart_init.rs:102-109` 直接调用
- 现在：作为 `Uart16550::set_ier()` 公开 API，被 uart_16550 内部 ISR 处理代码使用

**中期行动**（不依赖硬件）：
1. 准备 PR 描述：> Add `set_ier()` as the symmetric setter to the existing `ier()` getter. This is needed for kernel drivers that dynamically enable/disable individual interrupts at runtime.
2. 提交到 `rust-osdev/uart_16550` 上游
3. merge 后修改 `kernel/Cargo.toml`：`uart_16550 = { path = "../../uart_16550" }` → `uart_16550 = "0.7"`

---

## 当前权威文档（取代本文）

| 主题 | 当前权威文档 |
|------|-------------|
| uart_16550 API 体系 | `uart-16550-integration.md` §1 uart_16550 Crate 基础架构 |
| 迁移到 Q13 后的代码 | `kernel/src/drivers/uart_init.rs` + `uart_16550/src/async_/isr.rs` |
| 决策依据 | `architecture/spec.md` §Q13 相关 ADR |

---

**恢复条件**：如需查看上游状态详细分析、决策矩阵（方案 A/B/C）、PR 说明模板、风险提示详述，查阅 `_archive/uart-16550-fork-evaluation.md`
**生成日期**：2026-06-12（原始）→ 2026-06-17（摘要）
