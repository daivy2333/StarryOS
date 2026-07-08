# Proposal: ARC-202607081429 — 文档体系归档精简

## 归档概述

**日期**: 2026-07-08
**类型**: Compress-Archive（条目级别文档瘦身）
**执行人**: openspec-archivist skill
**触发**: 用户显式调用（"现在对文档体系，分析文档等等进行归档精简和文档瘦身等等操作"）
**上轮清理**: 2026-07-02 (ARC-202607021648) + 2026-07-03 (ARC-202607031929)

## 归档条目映射表

### architecture/spec.md → carrier spec（4 条 Compress-Archive）

| 原编号 | 原位置（行范围） | carrier spec 位置 | 理由 | 替代引用 |
|--------|-----------------|-------------------|------|----------|
| A014/A015 | L130-L143 | carrier architecture/spec.md | 方向 A 已关闭 | ADR-025/027 |
| A016/A017 | L145-L162 | carrier architecture/spec.md | 方向 A 失败教训 | ADR-026 + L79 |
| A020/A021 | L163-L182 | carrier architecture/spec.md | 方向 B 已纠正 | ADR-026 + ADR-025/027 |
| A032 | L321-L362 | carrier architecture/spec.md | 原始提取决策 | ADR-033 + ADR-036 |

### learned/spec.md → carrier spec（16 条 Compress-Archive）

| 原编号 | 原位置（内容摘要） | carrier spec 位置 | 理由 | 替代引用 |
|--------|-------------------|-------------------|------|----------|
| L078 | M3 替换失败 | carrier learned/spec.md | > 180d 已修复 | ADR-026 + L79 |
| L122 | critical-section 实现 | carrier learned/spec.md | > 180d 已稳定 | — |
| L123 | copier/Console 竞争 | carrier learned/spec.md | > 180d 已解决 | ADR-028 |
| L126 | TX copier 交错 | carrier learned/spec.md | > 180d 已解决 | ADR-029 |
| L131 | 数据量统一 | carrier learned/spec.md | > 180d 已解决 | — |
| L134 | yield storm | carrier learned/spec.md | > 180d 已解决 | — |
| L150 | NAPI 退出 | carrier learned/spec.md | > 180d 已修复 | — |
| L151 | ISR 锁 | carrier learned/spec.md | > 180d 已修复 | — |
| L152 | IER 裸写 | carrier learned/spec.md | > 180d 已修复 | — |
| L153 | 读路径拷贝 | carrier learned/spec.md | > 180d 已优化 | — |
| L154 | PollSet 迁移 | carrier learned/spec.md | > 180d 已完成 | — |
| L155 | waker 去重 | carrier learned/spec.md | > 180d 已完成 | — |
| L158 | 5-trait 表 | carrier learned/spec.md | 被替代 | ADR-036 + L188-L192 |
| L159 | D1 决策推翻 | carrier learned/spec.md | > 180d 已执行 | ADR-033/036 |
| L221 | D1 无输出根因 | carrier learned/spec.md | > 180d 已解决 | ADR-045 |
| L229 | D1 AMO fault | carrier learned/spec.md | > 180d 已解决 | ADR-046 |

## 排除项

以下操作不在本次归档范围内：

- **Simplify-Keep L169**（5-trait → 2-trait 描述修正）— 原地修改，不走 carrier spec
- **Stale-Warn L136**（benchmark 不测 UART）— 原地标记，不走 carrier spec
- **OpenSpec 变更归档** — q17-smp-memory-ordering 和 q19c-lichee-full-starryos-benchmark 均仍活跃
- **分析文档归档** — 3 份活跃文档（q17/q19c-lichee-full-starryos-benchmark/q19c-d1-tx-optimization）仍被引用
- **references/optimization/SNAPSHOT/tasks** — 已在上轮清理中维护，本轮无候选

## 恢复协议

1. 在源文档末尾 grep `<!-- arc: ARC-202607081429` 找到 carrier spec 路径
2. 读取 `openspec/archive/<日期>-ARC-202607081429/specs/<源域>/spec.md`
3. 在 carrier spec 中用 `### {原编号}` grep 定位条目
4. 用 Edit 精准复制回源文档原位置
5. 更新 arc 指引计数 -1 + 追加 `<!-- restored: {原编号} 2026-07-08 -->`

## 交叉引用说明

上述 20 条 Compress-Archive 候选的交叉引用均为历史上下文引用（被后续 ADR 或 learned 条目引用作为"曾经发生过什么"），不是功能性依赖。Compress-Archive 保留原编号的 ≤3 行骨架，grep 仍可定位，不会断链。
