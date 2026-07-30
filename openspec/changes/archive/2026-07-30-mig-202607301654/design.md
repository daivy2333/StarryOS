## Context

旧活动 experience specs 已由 `mig-20260720-legacy-specs` 归档，但该 carrier 只保存聚合覆盖结论。最新规则要求逐信息单元覆盖、来源 hash、编号映射、双向核对和可恢复入口。本轮不重新选择旧信息价值，也不改写任何历史 carrier。

## Goals

- 从历史原文和 carrier 建立可定位、可复算的来源单元全集。
- 证明每个单元至少进入一个当前权威目标。
- 保留重复、过时、已完成和 tombstoned 内容的来源、状态和时间边界。
- 升级规则、状态和三端入口，不修改产品代码。

## Non-goals

- 不删除或 Compress-Archive 旧经验。
- 不重写历史 carrier。
- 不清理与初始化无关的文档或代码。
- 不创建 Analysis、Runbook、Incident 或 Evidence 占位目录。

## Source Model

1. `openspec/project.md` 是已被 `config.yaml` 和 SNAPSHOT 取代的旧式项目上下文 carrier；`.claude/docs/tasks.md` 是待迁移为 MSxx/Txx 的旧任务 carrier。两份原文和 hash 固化在本 MIG。
2. 上一轮 MIG carrier 中的三份 `*-original.md` 是已退出旧 specs 的不可变归档副本。
3. 上一轮 proposal 指向的六个 ARC carrier 是 tombstoned 信息的不可变历史来源。
4. `CLAUDE.md` 与 SNAPSHOT 按模板重建，不进入覆盖清单。
5. 当前 M/D/K/R/I 是迁移目标；`docs/`、Analysis、Runbook 是已由 R 编号索引的持久化产物，不作为旧 carrier 重复装载。

## Unit Boundary

- 编号条目以编号标题/注释到下一同类编号为一个基础单元。
- 无编号 requirement、scenario、标题段落、表格数据行、checkbox、注释和代码块使用文件路径与标题/行锚点定位。
- 同一单元拆到多个目标时保留多行；多个来源合并到一个目标时保留每条来源记录。
- 单元 hash 对规范化前的原始字节片段计算 SHA-256。

## Migration Sequence

1. 固化历史 carrier 文件清单与 SHA-256。
2. 全文读取并拆分全部来源。
3. 生成 coverage checklist 和 numbering map。
4. 合并新目标并更新活动引用。
5. 执行逐单元正向核对和逐目标反向核对。
6. 验证 source/mapped/verified 计数、hash、OpenSpec 和文档 Gate。
7. 使用 OpenSpec 集成归档本 carrier。
8. 扫描旧活动路径和旧活动引用。

## Failure Handling

- 来源 hash 变化：停止并重建受影响映射。
- 单元无目标或目标不可定位：停止，不归档。
- validate 失败：保留 carrier 与所有旧活动路径。
- archive 失败：保留活动 carrier，不手工移动。
- archive 后旧引用清理失败：报告残留并继续精准退出，不重做迁移。
