# rules.md — 项目编码规范

> 由 project-rules-generator 初始化，由 project-docs-assistant 日常维护。
> 这是三大规则的唯一事实来源。CLAUDE.md 只做索引，不重复规则内容。

---

## Karpathy Guidelines

行为准则，减少 LLM 编码常见错误。

### 1. Think Before Coding

**不假设。不隐藏困惑。暴露权衡。**

- 明确陈述假设，不确定就问
- 多种解读存在时，全部呈现 - 不 silently 选择
- 更简单的方法存在时，说出来。必要时 push back
- 不清楚时，STOP。命名困惑点。问。

### 2. Simplicity First

**最小代码解决问题。无投机性功能。**

- 不添加未被要求的功能
- 单次使用代码不抽象
- 未要求的"灵活性"或"可配置性"不加
- 不可能场景的错误处理不加

### 3. Surgical Changes

**只改必须改。只清理自己的烂摊子。**

- 不"改进"相邻代码、注释、格式
- 不重构没坏的东西
- 匹配现有风格，即使你做法不同
- 删除 YOUR 改动导致未用的代码，不删除先前存在的死代码

### 4. Goal-Driven Execution

**定义成功标准。循环直到验证。**

- "添加验证" → "写无效输入测试，然后让它们通过"
- "修复 bug" → "写复现它的测试，然后让它通过"
- "重构 X" → "确保前后测试都通过"

---

## 务实编码原则

整洁代码与务实原则的软件工匠准则。

### 十大铁律

1. **命名即文档** — 精准、可读、可搜索的名称。Rust 惯用：`new`/`from_*` 构造，`as_*` 无损转换，`into_*` 有损转换；布尔值用 `is_`/`has_`/`can_` 开头
2. **函数单一职责** — < 20 行，只做一件事，无副作用，抽象层级一致
3. **DRY & 正交性** — 三次法则，驱动层与内核层正交，硬件抽象与业务逻辑分离
4. **显式胜于隐式** — 依赖注入，常量命名（`UART_FIFO_DEPTH` 而非 `16`），`#[inline]` 仅在性能关键路径
5. **健壮边界** — MMIO 封装不裸写地址，临界区只做最小操作，DMA 缓冲区对齐和所有权清晰
6. **可测试设计** — 纯逻辑与硬件操作分离，关键路径写测试，`#[cfg(test)]` 模块组织
7. **尽早重构** — 小步重构，每次可独立测试，每次提交让代码更好
8. **务实破窗** — 看到问题立即修，曳光弹：先端到端最小可用
9. **自动化检查** — 提交前 `cargo fmt` + `cargo clippy`，`make run` 验证内核启动
10. **注释解释意图** — unsafe 块必须 SAFETY 注释，注释解释硬件时序约束，过时注释比无注释更糟

---

## Workflow Designer

工作流概念框架，定义执行流程。

### 核心概念

- **Phase** — 逻辑分组的工作容器（进入/退出条件明确）
- **Gate** — 检查点（PASS 或 BLOCK，BLOCK 必须记录原因）
- **Task** — 最小执行单元（可独立验证，完成必须展示证据）
- **Loop** — 重复处理（clarification / review-fix / iteration / retry）

### 执行铁律

1. Phase 进入前必须 Gate PASS
2. Task 开始前必须 Gate PASS
3. Task 完成必须展示证据
4. Loop 退出必须条件 PASS
5. Gate BLOCK 必须记录原因
6. 声明完成必须验证证据

---

## 项目特定规范

### unsafe 规则

- unsafe 块必须有 `// SAFETY:` 注释解释安全性
- MMIO 寄存器操作封装在安全 API 后面
- 临界区使用 `interrupt::free()` 或架构特定原语
- DMA 缓冲区使用 `PageBox` 或对齐分配器

### ISR 极简原则

- 中断只做三件事：清标志 → 唤醒 Waker → 退出
- ISR 中不做数据拷贝，不做锁操作
- 数据搬运推迟到任务上下文

### Git 提交规范

- 格式: `feat(uart-async): / fix(uart-async): / refactor(uart-async):`
- 禁止把 Claude 列为 co-author

---

## Red Flags

```
❌ 假设不明确 → STOP，问
❌ 过度复杂 → 简化
❌ 改动超出请求 → 回滚
❌ unsafe 无 SAFETY 注释 → Iron Law 违规
❌ 顺手添加功能 → Karpathy 违规
❌ Gate BLOCK 不记录 → Workflow 违规
❌ 硬件地址裸写 → 安全违规
❌ 中断处理中有阻塞操作 → 性能违规
```
