## Context

项目的 M/D/K/R/I 与归档体系已经投入使用，但公共规则仍把 SNAPSHOT 当成“当前状态”，iteration 模板禁止恢复 blocked 轮次，项目兼容描述仍复制一套过时 UART 上下文。本地 skills 也停留在 OpenSpec CLI 1.4.0 的五个 workflow skill，而 `CLAUDE.md` 已使用十个 role skill 的职责模型。

旧体系正文已在 2026-07-20 迁移，UART 专属 M/D/K/R/I 与工程文档又在 2026-07-25 完整归档。此次升级必须利用这些不可变载体补齐审计，不得改写历史来制造新的迁移结果。

## Goals / Non-Goals

**Goals:**

- 让当前项目描述、工作状态、规则、经验和历史各有唯一权威位置。
- 让 Claude Code、Codex 和 OpenCode 从同一份项目内 skill 内容进入新角色体系。
- 为全部已发现 legacy payload 建立可定位、带 hash、可正反向核对的迁移清单。
- 保留用户现有的 tasks 与 knowledge 修改。

**Non-Goals:**

- 不改变产品代码、运行行为或当前 milestone。
- 不重新评价已经结束的 UART 技术选择。
- 不改写或重复归档旧 ARC/MIG carrier。
- 不把临时 documentation-lifecycle delta 合并为新的产品主 spec。

## Decisions

### Decision 1：SNAPSHOT 只描述项目，不描述工作

- 决定：按最新公共模板重建 SNAPSHOT；任务、证据、约束、理由和历史只保留在 tasks/change、M/D/K/R/I、Runbook、Analysis 或 archive。
- 原因：项目描述与迭代状态的更新频率和写入角色不同，混放会产生重复与陈旧事实。
- 影响：旧 SNAPSHOT 的进度与历史段不再出现，但对应内容仍存在于既有权威载体。
- 替代方案：保留“项目 + 状态”混合快照；因违反最新职责边界而拒绝。

### Decision 2：兼容入口只保留指针

- 决定：`openspec/project.md` 不再复制项目技术栈、目录、约束和分支，只指向 SNAPSHOT、CLAUDE、tasks、changes 与 specs。
- 原因：旧内容已经明显偏离 `net-k3`，继续双写必然再次漂移。
- 影响：仍查找该文件的工具可获得稳定入口，人和 agent 必须沿链接读取权威内容。
- 替代方案：同步更新两份完整项目描述；因制造双重权威而拒绝。

### Decision 3：历史 carrier 用指针和 hash 审计，不重写

- 决定：MIG 保存 source path、unit、source file hash、target type/path 与状态，并另外保存 carrier 目录 hash。
- 原因：逐单元表满足迁移可追溯性，目录 hash 能证明不可变 source 在本轮未被修改。
- 影响：覆盖表较长，但可以机械核对 `source = mapped = verified`。
- 替代方案：复制七份 carrier 正文到新 MIG；会重复归档并扩大不可变数据，违反 carrier 协议。

### Decision 4：修正旧迁移的覆盖基数

- 决定：将 2026-07-20 architecture 原文按实际 41 条 Requirement 计数，因此总覆盖从先前推导的 238 修正为 239。
- 原因：旧 checklist 宣称 40 条 ADR，但原文含 A063 在内共有 41 条 Requirement；升级不能沿用错误计数。
- 影响：新 MIG 明确记录旧声明与复核结果，不改写旧 carrier。
- 替代方案：维持旧 130/238 数字；会留下已知 unmapped 单元，违反 Gate。

### Decision 5：三端共用一份最新 role skills

- 决定：以当前全局 skill 源完整复制十个 OpenSpec role skill 目录到 `.claude/skills/`；现有 `.agents/skills -> ../.claude/skills` 继续作为 Codex/OpenCode 入口。移除五个 1.4.0 workflow skill 的项目内副本。
- 原因：公共规则已经按 role skills 工作；混用旧 workflow skills 会产生职责冲突，维护三份副本也会漂移。
- 影响：项目内 skill 名称从 apply/propose/archive 等 CLI workflow 切换到 plan/act/maintainer/recorder 等角色。
- 替代方案：运行 `openspec update`；该命令需要写只读的用户级配置且生成的 CLI skills 不满足本体系只使用 `name`、`description` frontmatter 的 Gate。

## Risks / Trade-offs

- [旧 SNAPSHOT 中的事实被误认为丢失] → 通过 tasks、M/D/K/R/I 和 archive 路由检查确认其权威载体仍在。
- [迁移 unit ordinal 不足以直接表达旧 L/O 编号] → ordinal 按原文件顺序稳定定位，目标 meta-spec 保留原 Legacy ID 与正文。
- [项目 skill 副本日后与全局源漂移] → 三端只共享项目内一份；未来由 `openspec-init` 执行显式升级和 diff 核对。
- [documentation-lifecycle delta 污染产品 specs] → 归档时使用 `--skip-specs`，把它保留为本次迁移的验证契约。

## Migration Plan

1. 更新规则、SNAPSHOT、兼容入口、config 与 iteration 模板。
2. 替换项目内 skills，并核对十份目录与当前全局源一致。
3. 对七份旧 carrier 计算目录和 source file hash，建立 239 行覆盖表。
4. 正向检查每个 source unit，反向检查各 target family。
5. 运行 active legacy 路径/引用扫描、OpenSpec strict validation 与 diff review。
6. 使用 `openspec archive mig-202608081731 --skip-specs` 归档。
7. 归档后再次运行 specs、changes、路径、引用和 skill 入口检查。

回退时可通过 Git 恢复本轮活动文档和 skills；历史 carrier 从未改写，无需回滚其内容。

## Open Questions

None。
