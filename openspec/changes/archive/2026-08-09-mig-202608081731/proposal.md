# Proposal: MIG-202608081731 — OpenSpec 文档体系升级

## Why

项目已经使用 M/D/K/R/I、Runbook、Analysis、Evidence 与归档 change，但公共规则、项目描述职责和 iteration 恢复协议落后于当前技能体系。2026-07-20 的迁移载体覆盖了当时仍活跃的三份 legacy spec，却只用汇总方式指向更早的 ARC carrier；最新迁移 Gate 要求不可变历史 carrier 也有逐信息单元、可双向核对的映射。

## What Changes

- 更新 `CLAUDE.md` 的 SNAPSHOT 职责、Maintainer 边界与 blocked iteration 恢复协议。
- 将 `.claude/docs/SNAPSHOT.md` 重建为唯一的当前项目描述，移出工作状态、约束正文、操作步骤、证据和历史。
- 将 `openspec/project.md` 收敛为兼容指针，并更新 `openspec/config.yaml` 的当前项目上下文。
- 更新 change iteration 模板，增加 `Blocker Resolution`。
- 用十个最新 OpenSpec role skills 替换五个 1.4.0 时代的 CLI workflow skills；`.agents/skills` 继续复用 `.claude/skills`，供 Claude Code、Codex 与 OpenCode 使用同一份内容。
- 登记七份不可变 legacy carrier，校验目录哈希，并将 239 个信息单元逐项路由到 M/D/K/R/I、Analysis 或后继归档载体。
- 不修改、移动、删除或再次归档既有历史 carrier；本 MIG 仅补齐审计索引与升级证明。

## Scope

### Included

- 公共 OpenSpec 规则与模板。
- 当前项目描述及兼容入口。
- 项目内三端共用的 OpenSpec skills 入口。
- 2026-07-02 至 2026-07-20 的六份 ARC carrier 和一份 MIG carrier。
- 2026-07-25 UART 文档清理载体作为后继归档目标与 48/48 覆盖证明。

### Excluded

- 产品代码、测试代码、构建逻辑和当前 milestone 内容。
- 已归档 carrier 内容的重写。
- 用户尚未提交的 `tasks.md` 与 `knowledge/spec.md` 修改。
- 创建新的 Runbook、Incident、M/D/K/R/I 条目；本次只升级体系和补全迁移证明。

## BDD Scenario Sketches

### Happy Path：读取当前项目

- 前置状态：新会话需要恢复项目上下文。
- 动作：依次读取 `CLAUDE.md`、`SNAPSHOT.md`、`tasks.md` 和 active changes。
- 结果：SNAPSHOT 只提供当前项目描述，工作状态和规则从各自权威入口取得。
- 失败边界：SNAPSHOT 再次包含进度、操作步骤、约束正文或历史时失败。

### Sad Path：回查历史 legacy 信息

- 前置状态：目标条目只存在于不可变 ARC carrier。
- 动作：从 `coverage-checklist.md` 按 carrier 和 unit ID 查询。
- 结果：能定位原始载体、生命周期目标、哈希和恢复方法。
- 失败边界：任一信息单元没有目标、无法反向定位或源哈希变化时失败。

### Edge Case：历史信息已在后续清理中归档

- 前置状态：旧 M/D/K/R/I 中的 UART 条目已由 2026-07-25 cleanup 移出 active specs。
- 动作：沿本 MIG 指向 cleanup carrier 的 meta-spec 或归档 payload。
- 结果：历史正文保持可恢复，不会被误当成当前约束或工作状态。
- 失败边界：只指向已经移除的 active ID、没有归档目标时失败。

### Error / Timeout / Cancel

- 哈希、覆盖率或 OpenSpec validation 任一失败时，不得归档本 MIG。
- 本地检查不存在外部等待或 timeout；命令异常时保留 active MIG 并停止。
- 用户取消时保留已写入载体，不移动历史来源，也不声明升级完成。

## Source Registry

目录哈希算法：对目录内所有普通文件按路径排序，逐文件计算 SHA-256，再对清单计算 SHA-256。完整清单见 `meta/source-hashes.tsv`。

| Source carrier | SHA-256 | Payload units |
|---|---|---:|
| `archive/2026-07-02-ARC-202607021535` | `7ffcc146c7782621c8c16baa2717a78a5bbbab3a077137356885d231ddc5559f` | 5 |
| `archive/2026-07-02-ARC-202607021648` | `bffd2f914d76d50c541ee0c71d685541afa4e3f41978f8a1efb955287674a5a9` | 32 |
| `archive/2026-07-03-ARC-202607031929` | `00a9dfb972f274df358c2c32c53f1bcbc44794a3cefb69fe2176b2598526b6cc` | 5 |
| `archive/2026-07-08-ARC-202607081429` | `fe2d37231df76b6d722acf12bb4400959880d8edc00815747222ca4c6517151e` | 23 |
| `archive/2026-07-11-ARC-202607111510` | `d057d5439e7ba3111495fb81250892c32f87f1931db36eb0aa69ce0bbdb53a89` | 40 |
| `archive/2026-07-15-arc-202607152005` | `9bf3e1f53b8af4b180731f9dc733b3df8566892f7f1b3344608ea09b610ce983` | 3 |
| `archive/mig-20260720-legacy-specs` | `168c3bd3d152ac169eea8d479f08cc84a0897b7f6fe67244fc53051342308cbc` | 131 |
| **Total** | — | **239** |

## Coverage and Routing

完整逐单元映射见 [`coverage-checklist.md`](coverage-checklist.md)。映射采用以下生命周期目标：

- 2026-07-20 carrier 宣称 130 个 legacy active-spec 单元；本次按原文复核发现 architecture 实有 41 条 Requirement，因此将覆盖基数纠正为 131。它们已进入 M/D/K/R/I，其中后来退出 active 视图的 UART 内容由 `archive/2026-07-25-cleanup-uart-docs/meta-specs/` 完整保留。
- 六份早期 ARC 的 108 个单元均为已结束、被替代或取消的历史信息，继续以原 ARC 作为不可变 Archive；本 MIG 增加统一检索和反向核对入口。
- 2026-07-25 cleanup 的 `archive-manifest.tsv` 已证明其归档 payload `total=48, mapped=48, unmapped=0, skipped=0`，因此它可作为后继 M/D/K/R/I 和 UART 产物的恢复入口。

覆盖统计：`total = 239`、`mapped = 239`、`verified = 239`、`unmapped = 0`、`skipped = 0`、`coverage = 100%`。

## Forward and Reverse Verification

- Forward：从每个 source carrier 和 unit ID 都能在 `coverage-checklist.md` 找到 lifecycle target。
- Reverse：从每个 target family 都能回到 source carrier、unit ID 和目录哈希。
- Integrity：归档前重新计算七份目录哈希，必须与 `meta/source-hashes.tsv` 一致。
- Exit：活跃树不得存在 `openspec/specs/architecture/`、`openspec/specs/learned/`、`openspec/specs/optimization/`，兼容入口不得再复制旧体系内容。
- Validation：`openspec validate mig-202608081731 --strict` 与 `openspec validate --specs` 必须通过。

## Recovery

1. 在 `coverage-checklist.md` 用历史 ID、标题或 carrier 名定位信息单元。
2. 读取对应不可变 source carrier；若需要新体系 UART 整理结果，再读取 `archive/2026-07-25-cleanup-uart-docs/` 的目标文件。
3. 只有新的用户授权和 OpenSpec change 才能把历史信息重新提升为 active M/D/K/R/I、Runbook、Incident 或任务。
4. 恢复后运行 specs、changes 和相关文档引用检查；不得直接改写本 MIG 或原 carrier 来伪造当前状态。

## Impact

- 项目文档入口与最新技能职责一致。
- 旧体系的历史信息保持不可变、可检索、可恢复，并具有 100% 审计覆盖。
- 不改变产品行为，不产生 TDD 代码见证要求。
