## Why

上一轮旧体系迁移只给出了聚合统计，没有按最新标准保存逐信息单元的 hash、目标和双向核对结果；活动文档中也仍有旧路径与旧编号引用。需要在不改写历史 carrier 的前提下补齐可审计迁移证据，并升级公共规则与状态入口。

## What Changes

- 建立旧体系来源全集，区分已退出的活动来源与不可变历史 carrier。
- 对旧 architecture、learned、optimization 原文及六个历史 ARC carrier 逐信息单元建立映射、hash 和验证状态。
- 合并现有 M/D/K/R/I 目标，保留 Legacy ID、独有事实、状态与时间边界。
- 重建公共规则、SNAPSHOT 和 iteration 模板，合并 tasks、config 与薄 AGENTS 入口。
- 更新活动文档中的旧路径和旧编号引用。
- 在覆盖率 100%、`unmapped = 0`、`skipped = 0` 后仅使用 OpenSpec Archive 归档本 carrier。

### BDD 场景草图

- Happy Path：来源 hash 稳定；执行逐单元映射和正反向核对；结果为 100% 覆盖并归档 carrier。
- Sad Path：任一单元没有目标；停止迁移并保留所有旧活动路径，不归档 carrier。
- Edge Case：历史 carrier 内容重复或过时；仍建立来源映射，并在新目标保留状态和时间边界。
- Error：来源 hash、OpenSpec validate 或 archive 失败；停止下游步骤，保留工作载体和恢复入口。
- Timeout/Cancel：本地迁移没有网络超时路径；中断时保留未归档 carrier，恢复时重新校验来源 hash。

## Capabilities

### New Capabilities

- `legacy-document-migration-audit`: 定义旧体系全量迁移的来源完整性、逐单元映射、验证和归档约束。

### Modified Capabilities

无。

## Impact

- 文档：`CLAUDE.md`、`AGENTS.md`、`.claude/docs/`、五类项目记忆 spec、活动引用。
- OpenSpec：`openspec/config.yaml` 与本 migration carrier。
- 代码/API/依赖：无产品代码、API 或依赖变更。
- 用户工作区：不触碰既有未跟踪目录 `crates/smoltcp/`。

## Source Baseline

- Active legacy experience sources: `openspec/project.md` 与 `.claude/docs/tasks.md`；完整原文位于 `active-originals/`，hash 见 `active-sources.sha256`。
- Exited legacy specs: 旧 `openspec/specs/{architecture,learned,optimization}/spec.md` 已在上一轮迁移后退出，其原文由上一轮 MIG carrier 保存。
- Rebuilt and excluded: `CLAUDE.md`、`.claude/docs/SNAPSHOT.md`。
- Immutable historical carriers: 见 `historical-carriers.sha256`；不复制、不改写、不重复归档。
- Previous migration carrier: `openspec/changes/archive/mig-20260720-legacy-specs/`。
- Carrier ID 使用 CLI 接受的 lowercase kebab-case `mig-202607301654`；协议语义等同 `MIG-202607301654`。

完整的 41 文件路径、载体位置、SHA-256 和 unit 数见 `source-registry.tsv`；历史文件的独立校验清单见 `historical-carriers.sha256`。

## Migration Mapping

| Source | Action | Current target | Archive content | Reason | Recovery |
|---|---|---|---|---|---|
| `openspec/project.md` | Archive + exit active path | `openspec/config.yaml`, SNAPSHOT, M/D/K/R/I | `active-originals/openspec-project-original.md` | 旧式 project context 被新配置、状态与五域模型取代 | 从 carrier 取回完整原文，或按 `unit-coverage.tsv` 恢复目标 |
| `.claude/docs/tasks.md` | Archive original + replace in place | MS01-MS04 roadmap | `active-originals/tasks-original.md` | Qxx 长历史改为 MSxx 稳定路线与 capability boundary | 从 carrier 取回原文；当前路线按 `Legacy Task Mapping` 恢复 |
| Previous MIG carrier | Keep immutable + verify | M/D/K/R/I, R47 | 原路径不变 | 已归档来源不得复制、改写或重复归档 | 按 `source-registry.tsv` 直接定位 |
| Six historical ARC carriers | Keep immutable + verify | M/D/K/R/I/MS, R47 | 原路径不变 | tombstone、完成、取消和 superseded 信息仍需逐单元映射 | 按 source ID、unit 行和旧编号定位 |

逐单元目标映射见 `unit-coverage.tsv`；旧编号聚合映射见 `numbering-map.md`；反向目标核对见 `target-coverage.tsv`。

## Coverage Result

```text
source units = 2743
mapped source units = 2743
verified source units = 2743
unmapped = 0
skipped = 0
coverage = 100.00%
```

- 正向：每个来源单元有 SHA-256、Legacy ID/上下文、状态/时间边界、目标和 `V` 状态。
- 反向：每个目标存在，编号锚点可定位，且 `target-coverage.tsv` 记录来源文件数与单元数。
- 来源：活动原文与全部历史 carrier 的 full-file SHA-256 已复算通过。

## Active Exit List

Archive 成功后按顺序执行：

1. 移除已完整归档的旧活动路径 `openspec/project.md`；这是 Archive 后的生命周期退出，不是 Delete。
2. 保留已就地替换的 `.claude/docs/tasks.md` MSxx roadmap。
3. 将 R47、SNAPSHOT 与 tasks 的 carrier 指针更新为 CLI 返回的最终归档路径。
4. 扫描活动文档中的旧 `architecture` / `learned` / `optimization` 路径和已退役编号入口；历史 provenance、immutable carrier 和本 MIG 原文不计为活动残留。
5. 重新运行 strict OpenSpec、来源 hash、逐单元生成物与 diff Gate。

## Recovery

1. 从 R47 定位本 migration carrier。
2. 用 `source-registry.tsv` 找到逻辑来源和完整原文/不可变 carrier。
3. 用 `unit-coverage.tsv` 的 source ID、line range 和 SHA-256 定位信息单元。
4. 用 `numbering-map.md` 与 `target-coverage.tsv` 找到当前目标。
5. 整份恢复活动原文，或精准恢复目标单元；恢复后重新生成四份 audit 表并运行 strict validation。
