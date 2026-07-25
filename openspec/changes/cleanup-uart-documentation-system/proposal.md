# 清理 UART 文档体系

## Why

`net-k3` 已转向异步 NIC 开发，但活跃文档仍混有 UART 阶段规格、计划、原始输出和未完成 change。现有 `ARC-202607251326` 也未完成归档：名称不符合 OpenSpec 的小写 kebab-case 约束，carrier specs 为空，任务仍有 2 项未完成。

这些残留会让 `openspec list`、SNAPSHOT、tasks、主规格和引用索引给出不同状态。本变更在进入 N0 前整理文档体系，只保留对当前 OS、NIC、VisionFive2 或后续维护有用的信息。

## Approved scope

用户在 2026-07-25 批准归档旧信息、未完成 UART change 和 UART 开发流，只保留当前有用信息。

- UART 专属 capability spec 不因仍对应现有代码而默认保留。
- UART 阶段计划、旧基准、原始输出和一次性分析进入完整归档。
- 未完成 UART change 可归档，但必须保留未完成或 deferred 状态。
- 仅保留当前 OS 约束、NIC 输入、VisionFive2 输入和可复用方法。
- 不修改产品代码，不删除或压缩丢失历史信息。

## Requirements

- R1：建立逐载体映射，覆盖率必须为 100%，且 `unmapped=0`、`skipped=0`。
- R2：每项内容只能有一个权威位置；提取可复用信息后再归档原载体。
- R3：归档 UART、D1 和 Console 的开发流、历史规格、旧报告、原始输出和一次性分析。
- R4：归档未完成 UART change 时保留未完成任务、验证边界和恢复路径。
- R5：保留并收敛 OS、NIC、VisionFive2 和通用工程方法。
- R6：修复活跃索引、引用、SNAPSHOT、tasks 与 OpenSpec 状态的不一致。
- R7：不修改产品代码、已归档历史正文或无关文档。
- R8：持久化归档清单、源文件哈希、目标路径和验证结果。
- R9：最终验收使用可执行 allowlist、禁用模式和 RED/GREEN 见证，不以手写 PASS 摘要代替检查。

## Scenario sketch

### Happy path

- **Given** 活跃文档同时包含 NIC 输入和 UART 阶段材料
- **When** 执行逐载体分类、提取、归档和索引更新
- **Then** 活跃路径只保留当前有用信息
- **And** 每项 UART 材料都能从归档清单恢复

### Current capability with no current planning value

- **Given** UART capability spec 仍描述仓库中的现有行为
- **When** 该 spec 仅服务 UART 阶段，且没有 OS、NIC 或 VF2 复用价值
- **Then** 允许完整归档
- **And** 不把归档解释为删除产品功能

### Reusable information inside a UART carrier

- **Given** UART 文档同时包含通用中断、waker、SMP 或验证方法
- **When** 归档该文档
- **Then** 可复用结论必须先进入唯一权威 M、D、K、R 或 Runbook
- **And** 原载体完整归档，不复制为第二份活跃正文

### Incomplete change

- **Given** `q17-smp-memory-ordering` 仍有 multi-hart 验证未执行
- **When** 归档该 change
- **Then** 归档记录必须标为 incomplete 或 deferred
- **And** 不得声称已完成真板 SMP 验证

### Missing mapping or broken reference

- **Given** 某个源文件没有目标路径、哈希或恢复说明
- **When** 执行归档 Gate
- **Then** 当前批次停止
- **And** 不移动该源文件

### Interrupted execution

- **Given** 清理在文件移动或索引更新中被中断
- **When** 尚未通过清单与链接验证
- **Then** 不声明完成
- **And** 根据 Git diff 和归档清单恢复一致状态

### Compatibility boundary

- **Given** 已归档 change、migration carrier 或无关文档
- **When** 执行本变更
- **Then** 其正文保持不变
- **And** 仅在活跃索引中修正必要指针

### Repeated partial cleanup

- **Given** 三轮实施均出现回复与磁盘内容不一致
- **When** 创建新的实施轮次
- **Then** 必须先冻结精确 allowlist、禁用模式和允许保留的 UART 语义
- **And** 必须保存修改前 RED 与修改后 GREEN 的命令、输出和退出码

## Scope

### Archive

- 17 个 UART、D1 或 Console 主 capability spec。
- `docs/` 下 8 份 UART 架构、报告、学习地图和原始输出。
- `.claude/analysis/q31-console-cpu-efficiency-port.md`。
- `.claude/analysis/lichee/` 的活跃 tombstone。
- `.claude/runbooks/benchmark-guide.md`。
- `q17-smp-memory-ordering` 与非法载体 `ARC-202607251326`。

### Retain and normalize

- M/D/K/R/I 中对 OS、NIC、VF2 和通用方法仍有效的条目。
- `quality-gate-baseline` 与 `platform-descriptor-early-console`。
- 4 份 NIC 分析和 `arceos-true-board-validation.md`。
- `incremental-merge.md`、`regression-gate.md`、`board-bringup-ladder.md` 中可复用部分。
- `docs/x11.md` 与其他无关文档。

## Non-goals

- 不修改 Rust、C、构建脚本、rootfs 或二进制。
- 不删除 UART 功能。
- 不重写已归档 change、migration carrier 或历史 Evidence。
- 不规划或启动 NIC N0。
- 不自动执行 change 收尾或归档。
