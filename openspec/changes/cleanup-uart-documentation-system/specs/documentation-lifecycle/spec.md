# Documentation lifecycle delta

## ADDED Requirements

### Requirement: UART development carriers are archived with full mapping

UART、D1 和 Console 的开发流、历史规格、旧报告、原始输出与一次性分析 MUST 在退出 UART 开发阶段时归档。每个源载体 MUST 记录目标路径和源目标 SHA-256。

#### Scenario: Archive a UART-only carrier

- **WHEN** 一个活跃载体仅服务 UART 阶段
- **THEN** 它 MUST 移入可恢复的归档位置
- **AND** 归档清单 MUST 记录源路径、目标路径和哈希

### Requirement: Reusable information is retained before carrier archival

对当前 OS、NIC、VisionFive2 或通用工程方法仍有效的信息 MUST 在原载体归档前进入唯一权威位置。活跃文档 MUST NOT 保留重复正文。

#### Scenario: UART analysis contains NIC-relevant knowledge

- **WHEN** UART 文档包含 ISR、waker、SMP、验证或所有权方面的可复用结论
- **THEN** 该结论 MUST 先进入对应 M、D、K、R 或 Runbook
- **AND** 原 UART 文档 MUST 完整归档

### Requirement: Current UART behavior does not force active retention

UART capability spec MAY 在功能仍存在时归档，只要该 spec 不再服务当前规划，且恢复路径完整。归档 MUST NOT 表示删除或废弃产品功能。

#### Scenario: Implemented UART capability leaves the active roadmap

- **WHEN** UART capability 已实现但不再服务当前 OS 或 NIC 规划
- **THEN** 对应 spec MAY 完整归档
- **AND** 产品代码 MUST 保持不变

### Requirement: Incomplete changes retain truthful status

未完成 UART change MAY 归档。未完成任务、环境阻塞、验证边界和恢复条件 MUST 保留，且 MUST NOT 改写为完成。

#### Scenario: Archive Q17 with deferred SMP validation

- **WHEN** Q17 的 multi-hart stress 未执行
- **THEN** 归档记录 MUST 保留该任务未完成
- **AND** MUST 明确 single-hart QEMU 不能证明 multi-hart 正确性

### Requirement: Cleanup coverage is complete

归档批次 MUST 达到 `mapped=total`、`unmapped=0`、`skipped=0`。缺少映射、哈希或恢复路径 MUST 阻塞该批次。

#### Scenario: A source has no archive target

- **WHEN** 清单发现源载体没有目标路径
- **THEN** 归档 Gate MUST 失败
- **AND** 该源载体 MUST 保持原位

### Requirement: Active documentation is internally consistent

清理后 SNAPSHOT、tasks、references、analysis index、Runbook 和 `openspec list` MUST 对活跃工作给出一致状态。活跃本地路径引用 MUST 可解析。

#### Scenario: Cleanup finishes

- **WHEN** 所有载体完成分类和移动
- **THEN** 活跃索引 MUST 只指向存在的活跃或归档路径
- **AND** OpenSpec 状态 MUST 不再把已归档 UART change 列为活跃工作

### Requirement: Cleanup does not alter product code or immutable history

本变更 MUST NOT 修改产品代码、构建资产、已归档 change 正文或 migration carrier。无关文档 MUST 保持不变。

#### Scenario: Verify cleanup scope

- **WHEN** 检查最终 diff
- **THEN** 变更路径 MUST 仅包含批准的文档、change 和 Evidence
- **AND** 产品代码及既有归档历史 MUST 无修改

### Requirement: Active control documents are stage-independent

活跃状态、索引、meta spec 与通用 Runbook MUST NOT 使用已退出的 UART Q 阶段编号或已删除 change 路径描述当前工作。当前 UART 产品约束、通用 early serial 可观测性和 D1 平台事实 MAY 保留在仍有效的 capability spec 中。

#### Scenario: Retain behavior without retaining the old workflow

- **WHEN** 活跃 capability spec 仍约束现有 UART、Console 或 D1 行为
- **THEN** 该行为约束 MAY 保留
- **AND** Q0-Q32 阶段编号、旧任务状态与已删除 change 路径 MUST 被当前 N 阶段、硬件条件或归档路径替代

#### Scenario: Keep the board bring-up Runbook generic

- **WHEN** Runbook 使用 early serial 或 Console 作为新板可观测性 Gate
- **THEN** 通用串口表述 MAY 保留
- **AND** D1 专属步骤、UART benchmark 流程与旧 Q 阶段 MUST NOT 保留

### Requirement: Final evidence is mechanically reproducible

最终 Evidence MUST 保存修改前 RED、修改后 GREEN、实际命令、关键输出和退出码。references allowlist、状态禁用短语、plain path、任务状态、归档哈希与产品代码范围 MUST 由命令检查。

#### Scenario: A written PASS disagrees with the repository

- **WHEN** Evidence 摘要声明 PASS，但机械检查失败
- **THEN** Gate MUST 失败
- **AND** tasks 与 Act Response MUST NOT 声明完成
