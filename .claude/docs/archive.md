# archive.md — 文档归档（汇总）

> 自动归档记录，由 assistant 维护。
> 按源文档分节，每条含日期、编号、置信度、理由、恢复条件。
> 恢复方式: 用户说"恢复 §{文档名} #{编号}"。
> 搜索方式: grep "关键词" archive.md 或 grep "编号" archive.md

---

## 文档体系迁移归档

<!-- archive: docs-to-openspec-2026-06-03 -->
**日期**: 2026-06-03
**事件**: 文档体系从 `.claude/docs/{5 个文件}` 迁移到 `openspec/specs/`
**触发**: `openspec-init` skill 执行（commit `922e8fd` + `b65021f`）
**置信度**: HIGH
**理由**: 5 个传统 .claude/docs 文档（70 KB）已重新组织为 OpenSpec spec-driven 格式（84 KB，新增规范字段 + 验证器合规），`openspec validate --specs` 5/5 通过
**迁移映射**:

| 原文件 | 新位置 | 关键变化 |
|--------|--------|----------|
| `.claude/docs/rules.md` | `openspec/specs/rules/spec.md` | 17 个 Requirement（Karpathy + 务实编码 + Workflow Designer + 项目特定） |
| `.claude/docs/architecture.md` | `openspec/specs/architecture/spec.md` | 13 个 Requirement（按主题分组 ADR-001~031，tombstone 仅在 .bak 保留） |
| `.claude/docs/learned.md` | `openspec/specs/learned/spec.md` | 10 个 Requirement（API/文件/踩坑/技巧/性能/测试） |
| `.claude/docs/references.md` | `openspec/specs/references/spec.md` | 8 个 Requirement（依赖/子项目/规范/Embassy/Linux/分析） |
| `.claude/docs/optimization.md` | `openspec/specs/optimization/spec.md` | 6 个 Requirement（Q5/Q7 + Q6/远期/排除 + 性能基线） |
| `CLAUDE.md` | `CLAUDE.md` | 重写为 OpenSpec + .claude/docs/ 双索引（5.7 KB，规则全文只在 spec） |

**保留不迁移**:

- `SNAPSHOT.md` — 状态快照（OpenSpec 不替代）
- `tasks.md` — 任务追踪（OpenSpec 不替代，但已加入 P0 milestone）
- `archive.md` — 本文件

**新增加**：

- `openspec/project.md` — 项目上下文
- `openspec/config.yaml` — schema: spec-driven
- `openspec/changes/` — 变更提案目录
- `.claude/commands/opsx/` — 5 个 slash commands
- `.claude/skills/openspec-*` — 5 个 skills

**备份保留**：

- `.claude/docs/*.md.bak` (×5) — 5 个迁移源文件完整备份
- `CLAUDE.md.bak` — 迁移前 CLAUDE.md 备份

**恢复条件**:

- 如需查看原始 .md 风格（非 OpenSpec 格式），查阅对应 `*.md.bak` 文件
- 如需 git 历史回滚，运行 `git revert 922e8fd b65021f`

**更正参考**: `openspec/project.md`、`CLAUDE.md`（新版）

---

## rules domain 二次迁移（2026-06-03）

<!-- archive: rules-domain-to-claude-md-2026-06-03 -->
**日期**: 2026-06-03
**事件**: `openspec/specs/rules/spec.md` 整体归档至 `openspec/changes/archive/rules-domain-2026-06-03/`，规则全文整合到 `StarryOS/CLAUDE.md` 下方"规则"章节
**触发**: openspec-init skill 最新模板要求"rules 已整合到 CLAUDE.md"，不再单独维护 `openspec/specs/rules/` 目录
**置信度**: HIGH
**理由**:

- 旧 rules 域从未作为变更提案参与 `/opsx:propose` 流程（始终由 openspec-init 直接生成）
- 规则在 CLAUDE.md 中**只读一次**（启动时全量加载），无额外 IO 开销
- 与 openspec-init skill 最新模板对齐，跨项目统一
- 规则更新更便捷（不需要走 OpenSpec 流程）

**迁移映射**:

| 原位置 | 新位置 | 关键变化 |
|--------|--------|----------|
| `openspec/specs/rules/spec.md` (17 Reqs) | `openspec/changes/archive/rules-domain-2026-06-03/spec.md` | 文件本身完整保留作墓碑 |
| （无） | `StarryOS/CLAUDE.md` "规则"章节 | 新增 7 大节（Karpathy / 务实编码 / Workflow Designer / 核心约束 / 技能执行 / 项目特定 / 检查清单 + Red Flags）|
| `StarryOS/CLAUDE.md` "完整规范见 rules/spec.md" 引用 | 删除 | 改为"完整规范见本文件下方'规则'章节" |
| `StarryOS/CLAUDE.md` 索引表中 rules 行 | 删除 | 改为"规则（本文档）" |
| `StarryOS/CLAUDE.md` 读取顺序表 rules 引用 | 删除 | 改为"**规则**"指向本文档 |
| `StarryOS/SNAPSHOT.md` "5 spec 域" | "4 spec 域" | 计数更新 |
| `../CLAUDE.md` StarryOS 区 rules 引用 | 删除行 | 与子项目同步 |

**当前 OpenSpec 体系**（2026-06-03 修正）:

| 域 | 文件 | Requirements | 状态 |
|----|------|--------------|------|
| architecture | `openspec/specs/architecture/spec.md` | 13 | ✅ 活跃 |
| learned | `openspec/specs/learned/spec.md` | 10 | ✅ 活跃 |
| references | `openspec/specs/references/spec.md` | 8 | ✅ 活跃 |
| optimization | `openspec/specs/optimization/spec.md` | 6 | ✅ 活跃 |
| ~~rules~~ | `openspec/changes/archive/rules-domain-2026-06-03/spec.md` | 17 | 🪦 归档（墓碑）|

**恢复条件**: 如需回滚到 spec 化规则格式，按 `openspec/changes/archive/rules-domain-2026-06-03/README.md` 的"恢复条件"段操作
**更正参考**: `StarryOS/CLAUDE.md`（新版含规则章节）

---

## architecture.md 归档

<!-- archive: A22-A23-A24 -->
**日期**: 2026-05-31
**条目**: A22: UART MMIO 权限问题发现 + A23: ISR MMIO 权限测试失败 + A24: MMIO 权限重新分析
**原分类**: 架构决策
**置信度**: HIGH
**理由**: 2026-05-31 Q0 Spike 确认 LoadFault 根因是 `UART_STRIDE=4` 而非页表权限问题。ADR-022/023 的"MMIO 权限阻塞"诊断有误。ADR-024 的部分纠正仍归因为"测试代码 bug"，实际具体根因是 stride 错误（见 ADR-026）。
**恢复条件**: 如需回顾早期误判路径，可查看此记录
**更正参考**: ADR-026（stride 根因确认）、ADR-027（统一方向）

<!-- archive: A2 -->
**日期**: 2026-05-27
**条目**: A2: 串口与控制台关系——独立硬件 /dev/ttyS0
**原分类**: 架构决策
**置信度**: HIGH
**理由**: 已被 ADR-015 取代（渐进式开发策略：共用 UART0，M1/M2 用 Console 验证，M3 替换 AsyncUart）
**恢复条件**: 需要回顾早期"独立硬件"设计思路时

原始内容:

<!-- A2 --> ### 2026-05-24 - 串口与控制台关系——独立硬件 /dev/ttyS0

- **决策**: 新增独立硬件串口（QEMU `-serial` 多路配置），注册为 `/dev/ttyS0`，不影响现有 `/dev/console`
- **原因**: 隔离风险，独立开发测试；初期不破坏控制台稳定性
- **影响**: 需在 QEMU 启动参数添加第二个 `-serial`；`/dev/console` 和 `/dev/ttyS0` 是两个独立设备

---

<!-- archive: A7 -->
**日期**: 2026-05-27
**条目**: A7: Console与AsyncUart共存策略——先独立后统一
**原分类**: 架构决策
**置信度**: HIGH
**理由**: 已被 ADR-015 取代（渐进式开发策略：共用 UART0，不再有"先独立后统一"阶段）
**恢复条件**: 需要回顾早期共存策略时

原始内容:

<!-- A7 --> ### 2026-05-25 - Console与AsyncUart共存策略——先独立后统一

- **决策**: 采用方案 C"先独立后统一"——AsyncUart 作为独立 `/dev/ttyS0` 设备，Console 保持不变；远期统一
- **原因**: 初期隔离风险，不破坏控制台稳定性；AsyncUart 可独立开发测试

---

<!-- archive: A11 -->
**日期**: 2026-05-27
**条目**: A11: QEMU 双串口开发策略——独立硬件隔离风险
**原分类**: 架构决策
**置信度**: HIGH
**理由**: 已不适用（QEMU 第二串口需要补丁未合并，决策共用 UART0）
**恢复条件**: 需要回顾早期双串口策略时

原始内容:

<!-- A11 --> ### 2026-05-25 - QEMU 双串口开发策略——独立硬件隔离风险

- **决策**: M0-M2 阶段 QEMU 配置第二个 `-serial`，AsyncUart 操作第二 UART 硬件实例
- **原因**: Console 串口用于内核日志和 shell 交互，如果直接在上面测试中断驱动可能破坏调试信息输出

---

## learned.md 归档

<!-- archive: L24 -->
**日期**: 2026-05-27
**条目**: Q1: QEMU virt 平台是否支持第二个 16550 UART？
**原分类**: 存疑问题
**置信度**: HIGH
**理由**: 已解决（QEMU 第二串口需要补丁未合并，决策共用 UART0，参见 ADR-013、ADR-014、ADR-015）
**恢复条件**: 需要回顾第二串口调研结果时

原始内容:

- Q1: QEMU virt 平台是否支持第二个 16550 UART？ — 决定是否需要独立硬件还是复用同一 UART — QEMU 文档/实验
  → **已解决：标准 QEMU 不支持，需补丁。决策共用 UART0。**

---

<!-- archive: L25 -->
**日期**: 2026-05-25
**条目**: Q2: 修改 axplat/axhal crate 的方式？
**原分类**: 存疑问题
**置信度**: HIGH
**理由**: 已决策（ADR-009：不修改，直接 path 依赖本地 uart_16550 crate）
**恢复条件**: 需要回顾此决策的上下文时

原始内容:

- Q2: 修改 axplat/axhal crate 的方式？~~fork 还是提 PR？~~ **已决策：不修改，内核直接用本地最新 uart_16550 crate** — ADR-009

---

<!-- archive: L28 -->
**日期**: 2026-05-25
**条目**: Q5: trap 上下文中读 MMIO 是否安全？
**原分类**: 存疑问题
**置信度**: HIGH
**理由**: 已解决（uart_16550 crate 封装了 volatile read，ISR 中安全）
**恢复条件**: 涉及 ISR 中 MMIO 读写的安全检查时

原始内容:

- Q5: trap 上下文中读 MMIO 是否安全？~~是否有内存序问题？~~ **已解决：uart_16550 crate 封装了 volatile read，ISR 中安全**

---

<!-- archive: L42 -->
**日期**: 2026-05-25
**条目**: Q19: 本地 uart_16550 crate 是否可发布到 crates.io？
**原分类**: 存疑问题
**置信度**: HIGH
**理由**: 已决策（ADR-009：使用 path 依赖，暂不发布）
**恢复条件**: 需要发布到 crates.io 时

原始内容:

- Q19: 本地 uart_16550 crate 是否可发布到 crates.io？ — **已决策：使用本地 path 依赖，暂不发布** — ADR-009

---

<!-- archive: L43 -->
**日期**: 2026-05-25
**条目**: Q20: StarryOS 的 uart_16550 v0.4.0 和本地版本是否有 API 兼容性？
**原分类**: 存疑问题
**置信度**: HIGH
**理由**: 已确认（本地 v0.6.0 完整覆盖 v0.4.0 API，额外增加中断控制）
**恢复条件**: 需要确认 API 差异时

原始内容:

- Q20: StarryOS 的 uart_16550 v0.4.0 和本地版本是否有 API 兼容性？ — **已确认：本地 v0.6.0 完整覆盖 v0.4.0 API，额外增加中断控制** — ADR-009

---

<!-- archive: L44 -->
**日期**: 2026-05-25
**条目**: Q21: Console 和 AsyncUart 同时操作同一 UART 的协调方案？
**原分类**: 存疑问题
**置信度**: HIGH
**理由**: 已决策（ADR-007：先独立后统一，QEMU 配第二个 -serial）
**恢复条件**: 需要回顾共存策略时

原始内容:

- Q21: Console 和 AsyncUart 同时操作同一 UART 的协调方案？ — **已决策：先独立后统一（方案 C），QEMU 配第二个 -serial** — ADR-007

---

## docs/analysis/ 归档

<!-- archive: docs-analysis-batch-2026-05-28 -->
**日期**: 2026-05-28
**操作**: 批量归档过时分析文档
**文档数量**: 11
**置信度**: HIGH
**理由**: 已标记"⚠️ 此文档为早期分析，部分内容已过时"，M3 替换失败后分析失效
**恢复条件**: 需要回顾早期探索分析时（从 git history 恢复）

归档文档列表（已删除原文件）:

1. **async-io-framework.md** — 异步 IO 框架分析，M3 替换失败后失效
2. **async-runtime.md** — 异步运行时分析，axtask::future 选型已定
3. **async-uart-design-context.md** — AsyncUart 设计上下文，ADR-018 失败后需重新设计
4. **comparison-with-sdmmc.md** — SDMMC 对比分析，部分过时
5. **device-registration.md** — 设备注册分析，已标记过时
6. **feasibility-assessment.md** — 可行性评估，M3 失败后部分失效
7. **serial-interfaces-overview.md** — 串口接口概览，已标记过时
8. **serial-optimization-preview.md** — 性能优化预览，M4 待重启
9. **syscall-interface.md** — syscall 接口分析，已标记过时
10. **tty-console-stack.md** — Console 栈分析，ADR-018 失败后部分失效
11. **uart-hardware-driver.md** — UART 硬件驱动分析，状态异常未解决

**保留文档**（未标记过时，仍有参考价值）:

- boot-init.md — 启动流程通用知识
- interrupt-framework.md — 中断机制已实现
- project-overview.md — 项目概览
- reference-implementations.md — 参考实现
- task-process-model.md — 任务模型
- uart-16550-crate-reuse.md — uart_16550 已集成

---

## tasks.md 归档

### 方向 A: 渐进式集成（feat/uart-async）— 2026-05-31 归档

<!-- archive: tasks-A -->
**理由**: M3 因 stride=4 + Console UART 状态不兼容（IRQ 风暴 + TX busy-loop）失败回滚。已验证经验（M0-M2 Ring Buffer + copier + VFS 模式）被方向 C 继承。
**内容**: M0 基础设施准备 ✅ → M1 架构验证 ✅ → M2 VFS 验证 ✅ → M3 替换失败 ❌ → M4-M6 未执行

### 方向 B: 完全剔除 Console（feat/uart-async-dev2 早期）— 2026-05-31 归档

<!-- archive: tasks-B -->
**理由**: P1/P2 因 stride=4 LoadFault 阻塞。stride=1 修复后方向 C 继承 P0 的模块结构和依赖。
**内容**: P0 规划 ✅ → P1 硬件初始化 ⚠️ → P2 异步架构 ❌（stride=4 阻塞）→ P3-P6 未执行

---

## optimization.md 归档

<!-- archive: O25-O33 -->
**日期**: 2026-05-31
**条目**: O25-O31, O33 — Q5 性能优化（已完成）
**置信度**: HIGH
**理由**: 7 项优化已实现并验证
**恢复条件**: 需回顾具体优化实现

已完成: O25 batch RX, O26 batch TX, O27 IER cache, O28 ISR merge,
O29 buf 1024, O30 TX single lock, O31 waker skip, O33 split rx/tx locks

---

## CodeGraph 索引补建（2026-06-03 补救）

<!-- archive: codegraph-init-2026-06-03 -->
**日期**: 2026-06-03
**事件**: StarryOS 首次建立 CodeGraph 索引（之前 Phase 0 检查被遗漏，用户反馈后补救）
**触发**: 用户反馈"codegraph 呢，你怎么又忘了"——openspec-init skill Phase 0 Step 2 在源码项目 MUST 执行
**置信度**: HIGH
**理由**:

- openspec-init skill Phase 0 Step 2 明确：源码项目 MUST 检查 `codegraph --version` 并 `codegraph init`
- 初次执行时误判为"无 .codegraph 目录" = "不需要处理"（错误推断：CLI 已装应自动 init）
- CodeGraph MCP 工具已连接（session-start 暴露 8 个工具）但本地索引缺失 = MCP 仍能跑但无项目数据

**补救操作**：

| 命令 | 结果 |
|------|------|
| `codegraph --version` | v0.9.9 @ `/home/daivy/.npm-global/bin/codegraph` |
| `codegraph init .` | ✅ 119 文件 / 2,174 节点 / 5,781 边 / 870ms / 4.98 MB SQLite |
| `codegraph status .` | ✅ healthy，backend=node:sqlite built-in (full WAL) |
| `ls .codegraph/` | ✅ `codegraph.db` + `.gitignore` 已生成 |

**索引内容**（按节点类型）：

| 节点类型 | 数量 |
|---------|------|
| import | 707 |
| method | 683 |
| function | 315 |
| variable | 133 |
| struct | 122 |
| file | 114 |
| enum_member | 50 |
| type_alias | 20 |
| enum | 18 |
| trait | 12 |

**CLAUDE.md 同步**：新增"代码智能（CodeGraph，推荐）"章节，含工具表 + 不可用时处理流程
**新规则**（已加入 `StarryOS/CLAUDE.md` 第 5.5 节）：CodeGraph 可用时**优先**用 `codegraph_explore` 替代 Read+Grep；不可用时**不降级**，先查 status → 重连 → init 重建
**恢复条件**: 不适用（索引建立是前向操作）
**预防**: openspec-init skill Phase 0 Step 2 须**显式**执行（`codegraph --version` + `codegraph init`），禁止推断"无目录 = 无需处理"
**更正参考**: `StarryOS/CLAUDE.md` "代码智能（CodeGraph）"章节 + `openspec/specs/learned/`（未来可加"openspec-init Phase 0 完整清单" Requirement）
