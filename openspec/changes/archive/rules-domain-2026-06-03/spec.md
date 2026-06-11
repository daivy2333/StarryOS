# Spec: rules — 项目编码规范

## Purpose

定义 StarryOS 项目的编码规范、开发流程和工作约束。整合三大规则体系（Karpathy Guidelines、务实编码原则、Workflow Designer），是所有规则的唯一事实来源。CLAUDE.md 与子文档只做索引或专项展开，不重复本规范全文。

## Requirements

### Requirement: Karpathy 行为准则 — Think Before Coding

开发者在编码前 MUST 明确假设、暴露困惑、对比方案，禁止在不确定的情况下静默推进。

#### Scenario: 假设不明确时遇到决策点

- **WHEN** 开发者收到的任务存在多种合理解读、技术栈选型不明、或硬件行为未验证
- **THEN** 必须停下来命名困惑点、向用户提问或并列呈现所有解读，禁止 silently 选一种推进

#### Scenario: 发现更简单的方案

- **WHEN** 开发者在实施前评估到比用户指定方案更简单的路径（更少代码、复用现有原语、避免新依赖）
- **THEN** 必须向用户说明对比并 push back，由用户决定是否采用

### Requirement: Karpathy 行为准则 — Simplicity First

实现 MUST 用最小代码解决问题，禁止投机性功能、未要求的灵活性、不可能场景的错误处理。

#### Scenario: 实现新功能时考虑扩展性

- **WHEN** 开发者实现一个具体功能，想要顺手抽象出通用接口或加配置开关
- **THEN** 只实现当前用例所需的最小代码，不为"未来可能用到"添加抽象，不为单次使用代码做泛化

#### Scenario: 代码超过预期长度

- **WHEN** 实现完成后代码行数明显超出"资深工程师会写的量"
- **THEN** 必须重新审视并重写，目标是把 200 行的实现缩到 50 行

### Requirement: Karpathy 行为准则 — Surgical Changes

开发者 MUST 只改必须改的代码，禁止顺手"改进"无关代码或重构没坏的东西。

#### Scenario: 编辑现有代码时发现相邻代码风格问题

- **WHEN** 开发者修改 A 函数时注意到附近 B 函数命名不好、注释过时、格式不一致
- **THEN** 必须忽略 B 函数，匹配现有风格继续改 A；除非用户要求清理 B，否则不动

#### Scenario: 改动引入孤儿代码

- **WHEN** 开发者的改动导致某些 import / 变量 / 函数不再被使用
- **THEN** 必须删除自己改动引起的孤儿代码；先前已存在的死代码不删除，但可以提及

### Requirement: Karpathy 行为准则 — Goal-Driven Execution

任务 MUST 转化为可独立验证的目标，定义成功标准后循环直到验证通过。

#### Scenario: 接到模糊任务

- **WHEN** 用户要求"添加验证"、"修复 bug"、"重构 X"等模糊任务
- **THEN** 必须先把任务翻译成可验证目标（写无效输入测试 → 让它们通过 / 写复现 bug 测试 → 让它通过 / 重构前后测试都过），然后基于该目标循环

### Requirement: 务实编码 — 命名即文档

所有标识符 MUST 使用精准、可读、可搜索的名称，符合 Rust 惯用。

#### Scenario: 命名 Rust 构造与转换函数

- **WHEN** 开发者编写构造函数或类型转换
- **THEN** 用 `new` / `from_*` 构造，`as_*` 表示无损转换（如借用），`into_*` 表示有损/消耗转换（如所有权转移）

#### Scenario: 命名布尔值与集合

- **WHEN** 开发者命名布尔字段、方法或集合变量
- **THEN** 布尔值用 `is_` / `has_` / `can_` / `should_` 开头；集合用复数形式（`bytes`、`wakers`）

### Requirement: 务实编码 — 函数单一职责

函数 MUST 短小、只做一件事、无副作用、抽象层级一致。

#### Scenario: 函数超过 20 行或包含多层抽象

- **WHEN** 函数代码超过 20 行，或同时混合了硬件操作、业务逻辑、错误处理多个层级
- **THEN** 必须拆分为多个小函数，每个函数只处理单一抽象层级

### Requirement: 务实编码 — DRY 与正交性

开发者 MUST 消除重复，保持驱动层与内核层、硬件抽象与业务逻辑相互独立。

#### Scenario: 第三次复制相同逻辑

- **WHEN** 开发者发现同一段逻辑已经在两处复制，正要写第三处
- **THEN** 必须先抽象出共同接口，再让三处都使用

### Requirement: 务实编码 — 显式胜于隐式

依赖 MUST 通过参数或构造函数显式传入，常量必须命名，禁止全局状态和隐式上下文。

#### Scenario: 出现魔法数字

- **WHEN** 代码中出现没有命名的硬编码数字（如 `16`、`0x10000000`）
- **THEN** 必须提取为命名常量（如 `UART_FIFO_DEPTH`、`UART_MMIO_BASE`），并附带说明其含义

#### Scenario: 使用 inline 优化

- **WHEN** 开发者考虑添加 `#[inline]`
- **THEN** 仅在性能关键路径（ISR、热循环、零成本封装）使用，不在通用函数上滥用

### Requirement: 务实编码 — 健壮边界

业务核心 MUST 通过接口与抽象隔离硬件，MMIO 必须封装，临界区最小化，DMA 缓冲区所有权清晰。

#### Scenario: 访问硬件寄存器

- **WHEN** 开发者需要读写 MMIO 寄存器
- **THEN** 必须通过安全封装（如 `uart_16550` crate 的 API），禁止裸写地址（`*(0x10000000 as *mut u8) = x`）

#### Scenario: 持有锁执行复杂操作

- **WHEN** 开发者在临界区内执行操作
- **THEN** 临界区只做最小必要操作，禁止在锁内做 I/O、await、长循环

### Requirement: 务实编码 — 可测试设计

每个单元 MUST 可独立测试，纯逻辑与硬件操作分离。

#### Scenario: 实现新的核心逻辑

- **WHEN** 开发者实现影响功能正确性的核心逻辑
- **THEN** 必须用 `#[cfg(test)]` 模块组织对应测试，且测试通过后才能合并

### Requirement: 务实编码 — 尽早重构

开发者 MUST 持续小步重构消除技术债务，每次提交让代码比之前更好。

#### Scenario: 看到代码坏味道

- **WHEN** 开发者在阅读或修改时发现重复、过长函数、命名不清等问题，且该问题在改动范围内
- **THEN** 必须立即小步重构，且每次重构后保证测试仍通过

### Requirement: 务实编码 — 务实破窗

团队 MUST 不容忍劣化代码，看到问题立即修复。

#### Scenario: 发现 broken window

- **WHEN** 开发者发现代码中已有的明显劣化（破注释、TODO 堆积、明显 bug 但未修）
- **THEN** 必须立即修复或开 ticket 记录，不可"以后再说"

### Requirement: 务实编码 — 自动化检查

开发者提交前 MUST 运行格式化、静态分析与测试。

#### Scenario: 准备提交

- **WHEN** 开发者准备 `git commit`
- **THEN** 必须依次运行 `cargo fmt`、`cargo clippy`、`make run` 验证内核启动，全部通过才能提交

### Requirement: 务实编码 — 注释解释意图

注释 MUST 只解释"为什么"，不注释"做什么"；过时注释比无注释更糟。

#### Scenario: 写 unsafe 块

- **WHEN** 开发者编写 `unsafe { ... }` 块
- **THEN** 必须紧邻 `// SAFETY:` 注释解释为何这段代码满足 unsafe 契约（如对齐、生命周期、互斥访问）

#### Scenario: 修改代码后发现旧注释失效

- **WHEN** 开发者改动代码后发现已有注释描述与新行为不符
- **THEN** 必须同步更新或删除注释，禁止保留过时注释

### Requirement: Workflow Designer — Phase / Gate / Task / Loop 概念

所有工作流 MUST 按 Phase（阶段）→ Gate（门控）→ Task（任务）→ Loop（循环）四层概念组织。

#### Scenario: 进入新阶段

- **WHEN** 工作流从一个 Phase 切换到下一个 Phase
- **THEN** 必须先通过对应 Gate 检查（auto_check / user_approval / evidence_required），Gate PASS 才能进入

#### Scenario: 任务完成声明

- **WHEN** 开发者声明某个 Task 完成
- **THEN** 必须展示证据（命令输出、测试结果、文件内容），禁止仅口头声明"完成"

### Requirement: Workflow Designer — Gate BLOCK 必须记录原因

每次 Gate BLOCK MUST 在 tasks.md 或对应 change proposal 中记录原因。

#### Scenario: 检查不通过

- **WHEN** Gate 检查失败（测试不过、用户不批准、证据缺失）
- **THEN** 必须把 BLOCK 原因写入跟踪文档，禁止 silently 继续推进

### Requirement: 项目特定 — ISR 极简原则

中断处理函数 MUST 只做最小工作：清标志 → 唤醒 Waker → 退出，禁止数据搬运和锁操作。

#### Scenario: 实现 UART 中断处理

- **WHEN** 开发者编写或修改 ISR 代码
- **THEN** ISR 中只能：(1) 读 ISR 寄存器判断中断类型，(2) 禁用对应中断防止重入，(3) 调用 `AtomicWaker::wake()`，(4) 立即返回；数据从 FIFO 到 ring buffer 的搬运必须在 copier 任务里做

### Requirement: 项目特定 — MMIO 封装

所有 MMIO 寄存器操作 MUST 封装在安全 API 后面，禁止裸写硬件地址。

#### Scenario: 访问 UART 寄存器

- **WHEN** 开发者需要操作 UART 硬件寄存器
- **THEN** 必须通过 `uart_16550::Uart16550<MmioBackend>` 提供的安全 API，禁止直接 `read_volatile` / `write_volatile` 裸地址

### Requirement: 项目特定 — Git 提交规范

提交信息 MUST 遵循 conventional commits 子集，且禁止把 Claude 列为 co-author。

#### Scenario: 写提交信息

- **WHEN** 开发者准备 commit message
- **THEN** 必须用 `feat(uart-async): / fix(uart-async): / refactor(uart-async): / docs(uart-async):` 等前缀；禁止在 message footer 添加 `Co-Authored-By: Claude` 或任何形式标记 Claude 为共同作者

#### Scenario: 选择分支

- **WHEN** 开发者开始新功能开发
- **THEN** 必须基于 `main` 或当前活跃 dev 分支创建 `feat/uart-async-*` 形式的分支，最终通过 PR 合入 main
