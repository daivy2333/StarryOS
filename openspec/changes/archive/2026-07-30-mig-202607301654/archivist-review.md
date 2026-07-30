# Archivist Review

Reviewed: 2026-07-30

## 分析报告

| 文档 | 条目 | Archive | Compress | Delete | Stale | Promote | Merge | Artifact | Keep |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| Active legacy sources | 2 | 2 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| Previous MIG + historical ARC files | 39 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 39 |
| Rebuilt CLAUDE/SNAPSHOT | 2 | 0 | 0 | 0 | 0 | 0 | 0 | 0 | 2 |
| Migration carrier | 1 | 1 | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

### HIGH 置信度

- `openspec/project.md`：完整 Archive 后退出活动路径；原文、full-file hash、61 个信息单元和恢复映射齐全；恢复入口为 `active-originals/openspec-project-original.md`。
- `.claude/docs/tasks.md` 旧版：完整 Archive，活动路径由 MS01-MS04 roadmap 就地替换；原文、full-file hash、156 个信息单元和 Legacy Task Mapping 齐全。
- 39 个历史 MIG/ARC 文件：Keep；全部 hash 稳定，逐单元进入当前目标，不复制、不改写、不重复归档。
- `mig-202607301654`：只允许 Archive；2,743/2,743 单元验证通过，`unmapped=0`、`skipped=0`，strict validation 与恢复协议完整。

### 需要判定

- 无。用户通过 `$openspec-init` 明确授权按最新版标准升级旧体系，migration 规则又明确要求完整 Archive；不存在 Delete、Compress-Archive 或额外产品变更授权。

### OpenSpec changes

- `mig-202607301654`：ready to archive with `--skip-specs`；audit capability 仅用于本 carrier，不应合并为项目产品 capability。
- `q17-smp-memory-ordering`：Keep active；18/19 tasks，multi-hart hardware boundary 与本次文档迁移无关。

## 执行报告

| 条目 | 动作 | 目标 | 验证 | 结果 |
|---|---|---|---|---|
| MIG source originals | Archive | 当前 migration carrier | 2/2 SHA-256、217 units | PASS |
| Historical carriers | Keep | 原归档路径 | 39/39 SHA-256、逐单元映射 | PASS |
| Current targets | Merge / rebuild | M/D/K/R/I, SNAPSHOT, tasks, CLAUDE, templates | forward/reverse audit | PASS |
| Skill entries | Shared canonical copy | `.claude/skills`, `.agents/skills` symlink | 0 missing/mismatch/frontmatter error | PASS |
| MIG carrier | Archive | CLI-resolved archive path | strict specs/changes + diff Gate | READY |

附带：

- Carrier ID：`mig-202607301654`。
- 归档命令：`openspec archive mig-202607301654 --skip-specs -y`。
- 归档路径：由 OpenSpec CLI 决定，归档后写入 R47、SNAPSHOT/tasks 指针。
- arc 指引：R47 是统一恢复索引；逐来源、逐编号和逐单元位置由本 carrier 保存。
- 未执行条目：产品代码构建、真板测试；原因是无产品代码变化，且本迁移不声称新硬件行为。
- 失败恢复：archive 失败则保留活动 carrier 和 `openspec/project.md`；archive 成功后退出失败则从已归档 carrier 继续精准完成，不重新迁移。

## Source Boundary

| Path | Pre-exit mtime | Baseline |
|---|---|---|
| `openspec/project.md` | 2026-07-02 15:29:06 +08:00 | 6,484 bytes / SHA-256 `bf5fcffd...` |
| old `.claude/docs/tasks.md` | original captured before replacement | 19,860 bytes / SHA-256 `d7e7df29...` |

Current `.claude/docs/tasks.md` is the migrated target and therefore intentionally differs from its archived original.
