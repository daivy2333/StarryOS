# Design: 可恢复的条目级架构归档

## Decision

归档载体使用 architecture delta 的 `REMOVED Requirements` 表达 active 规范删除意图，并在同一文件的 Archive 区以 Markdown code fence 完整保存原文。源 architecture 仅留下稳定编号、日期、carrier ID、替代入口和恢复路径。

旧条目没有编号时，按当前最大 A062 之后顺序分配 A063/A064；已有 A056 保持原编号。分析文件采用 Analysis-Archive 例外：只修正 references 索引，不创建 carrier 副本。

## Safety

先验证 carrier change，再执行 `openspec archive --skip-specs`。`--skip-specs` 是必要的，因为源条目的删除、墓碑和交叉引用需要一次精确事务完成；让 CLI 自动把历史 delta 合并回 active spec 会产生重复历史内容。

## Validation

- carrier 归档前：`openspec validate --changes`
- 源文档修改后：`openspec validate --specs`、`openspec validate --changes`
- 文本完整性：A063/A064/A056 在 carrier 唯一完整保留，active spec 各有唯一墓碑
- Markdown/patch：`git diff --check`
