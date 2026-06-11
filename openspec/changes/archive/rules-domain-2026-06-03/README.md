# 归档墓碑 — rules domain

> **归档日期**: 2026-06-03
> **归档原因**: openspec-init skill 最新模板要求"rules 已整合到 CLAUDE.md"，不再单独维护 `openspec/specs/rules/` 目录
> **执行命令**: `mv openspec/specs/rules/spec.md openspec/changes/archive/rules-domain-2026-06-03/`
> **触发会话**: 当前会话（用户要求"全部都迁移到新格式"）

---

## 墓碑内容

- **原位置**: `openspec/specs/rules/spec.md`（9805 bytes，17 Requirements）
- **新位置**: 本目录（`openspec/changes/archive/rules-domain-2026-06-03/spec.md`）
- **规则去向**: 已整合到 `StarryOS/CLAUDE.md` 下方"规则（唯一事实来源）"章节
  - 一、Karpathy Guidelines（5 条）
  - 二、务实编码原则（10 大铁律）
  - 三、Workflow Designer（核心概念 + 执行铁律 + 工具映射）
  - 四、核心执行约束（8 条）
  - 五、技能执行规则（强制，6 条）
  - 六、项目特定规则（不可妥协：ISR / MMIO / stride / Git / 跨层状态 / 构建）
  - 七、检查清单 + Red Flags

## 恢复条件

如需回滚到旧格式（spec 化的规则）：

1. `mkdir -p openspec/specs/rules && mv openspec/changes/archive/rules-domain-2026-06-03/spec.md openspec/specs/rules/`
2. 从 `StarryOS/CLAUDE.md` 删除"规则"章节
3. 恢复 `StarryOS/CLAUDE.md` 中"完整规范见 `openspec/specs/rules/spec.md`"的引用

## 不应恢复的理由

- 规则由 spec 化迁移到 CLAUDE.md 后**只读一次**（CLAUDE.md 启动时全量加载），无额外 IO 开销
- 规则集中管理在 CLAUDE.md，更新更便捷（不需要走 `/opsx:propose` 流程）
- 与 openspec-init skill 最新模板对齐，跨项目统一
- 历史上本项目的 rules 域从未作为变更提案参与 `/opsx:propose` 流程（始终由 openspec-init 直接生成），不存在"活跃变更"问题
