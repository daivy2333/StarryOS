## ADDED Requirements

### Requirement: OpenSpec 文档升级 MUST 保持项目描述单一权威

升级后的文档体系 MUST 只在 `.claude/docs/SNAPSHOT.md` 保存当前项目描述，并将工作状态、约束、理由、操作步骤、证据和历史路由到各自权威载体。

#### Scenario: 新会话读取项目上下文

- **WHEN** agent 按公共读取顺序恢复项目上下文
- **THEN** `SNAPSHOT.md` MUST 只提供项目身份、技术栈、组成与职责、支持范围、交付形态和仓库现场
- **AND** 工作状态 MUST 从 `tasks.md` 与 active changes 取得

### Requirement: Legacy carrier 迁移 MUST 可逐单元审计

迁移载体 MUST 为所有发现的 legacy payload 信息单元保存 source carrier、unit ID、生命周期目标、反向键、完整性哈希与恢复协议，并达到 `unmapped=0`、`skipped=0`。

#### Scenario: 回查已归档 legacy 信息

- **WHEN** 开发者使用历史 ID 或 carrier 名查询旧信息
- **THEN** migration coverage MUST 定位唯一 source carrier 和生命周期目标
- **AND** source carrier 的完整性 MUST 能由登记的 SHA-256 复核

#### Scenario: 覆盖或完整性检查失败

- **WHEN** 任一 payload 单元未映射、被跳过或 source hash 不一致
- **THEN** migration change MUST 保持 active
- **AND** 归档操作 MUST 停止
