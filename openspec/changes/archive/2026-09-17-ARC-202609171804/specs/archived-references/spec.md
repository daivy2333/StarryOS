## Purpose

保存失效参考 R54 的完整索引行和恢复边界。

## ADDED Requirements

### Requirement: R54 原文可恢复

Carrier MUST 保留以下原始表格行：

```markdown
| <!-- R54 --> | `scripts/ms05_evidence_capture.py`、`scripts/ms05_evidence_audit.py` | MS05 自动 Gate manifest 与审计工具入口 — capture 记录 literal argv、child records、source freeze 和 artifacts；audit 校验 schema、资格与漂移分类。原 Runbook 已移除，工具与归档 change 保留实现和历史 Evidence |
```

#### Scenario: 恢复 R54

- **WHEN** 当前规则重新允许该工具链，且两个脚本已经恢复为有效入口
- **THEN** Maintainer MAY 按原编号和原始表格行恢复 R54
