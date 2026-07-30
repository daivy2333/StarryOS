## ADDED Requirements

### Requirement: 迁移来源完整

迁移载体 SHALL 登记全部活动旧经验来源与不可变历史 carrier；活动来源 SHALL 保存原始全文和 SHA-256，历史 carrier SHALL 保存逐文件路径和 SHA-256 指针且不得复制或改写。

#### Scenario: 固化迁移基线

- **WHEN** 初始化升级在修改旧来源或新目标前建立 migration carrier
- **THEN** carrier 可定位全部来源、排除 CLAUDE 与 SNAPSHOT，并区分活动原文副本和历史 carrier 指针

### Requirement: 信息单元全部映射

迁移 SHALL 为每个来源信息单元记录可定位锚点、SHA-256、一个或多个目标、Legacy ID、状态与验证结果，且不得使用跳过状态。

#### Scenario: 映射重复或过时信息

- **WHEN** 来源单元重复、过时、已完成或 tombstoned
- **THEN** 仍记录来源映射，并在目标中保留独有事实、状态和时间边界

### Requirement: 双向验证后归档

Archivist MUST 仅在 `source units = mapped source units = verified source units`、覆盖率 100%、`unmapped = 0`、`skipped = 0`、来源 hash 稳定且新目标可定位时，使用 OpenSpec 集成完整归档 migration carrier。

#### Scenario: 覆盖核验通过

- **WHEN** 正向与反向核对、OpenSpec validate 和来源 hash 复算全部通过
- **THEN** carrier 被完整归档，旧活动路径与活动旧引用随后退出

#### Scenario: 任一核验失败

- **WHEN** 单元、hash、目标、验证或 archive 任一条件失败
- **THEN** 流程停止并保留活动 carrier 与所有尚未退出的旧路径
