# ARC-202609171804 — 持久化文档生命周期清理

## 目的

归档已经被现行规则替代的 M32、目标文件已删除的 R54，以及完成超过 30 天的 T01–T06、MS01–MS04 和 MS16。源文档只保留批次墓碑；完整原文和恢复条件保存在本 carrier。

## 源文档基线

| 源文档 | 预检 mtime |
|---|---|
| `.claude/docs/tasks.md` | `1789635638`（2026-09-17 17:00:38 +08:00） |
| `openspec/specs/project-model/spec.md` | `1786275743`（2026-08-09 19:42:23 +08:00） |
| `openspec/specs/references/spec.md` | `1789638065`（2026-09-17 17:41:05 +08:00） |

## 映射

| 条目 | 源文档 | 动作 | Carrier 位置 | 判断理由 | 活跃交叉引用 | 恢复条件 |
|---|---|---|---|---|---|---|
| M32 | `openspec/specs/project-model/spec.md` | Archive | `specs/archived-project-model/spec.md` | 条目仍为“候选”，不属于当前有效模型；Gate 分层已由 `CLAUDE.md` 统一规定 | 无活跃 M32 引用 | 形成独立、获批且当前有效的跨产物 lint/test 模型约束 |
| R54 | `openspec/specs/references/spec.md` | Archive | `specs/archived-references/spec.md` | 两个目标脚本和测试已删除；`Makefile` 明确禁止该身份型证据工具链重新出现 | R44 将其标为悬空引用；归档后通过本 proposal 恢复 | 当前规则重新允许该工具链，且目标实现和有效用途均已恢复 |
| T01 | `.claude/docs/tasks.md` | Archive | `specs/archived-tasks/spec.md` | 已随 MS01 完成超过 30 天 | MS08 保留 MS01 行为回归名称 | 需要重新规划同步协议栈基线工作 |
| T02 | `.claude/docs/tasks.md` | Archive | `specs/archived-tasks/spec.md` | 已随 MS02 完成超过 30 天 | 历史 Runbook 和归档 change | 需要重新规划 QEMU I/O 边界见证 |
| T03 | `.claude/docs/tasks.md` | Archive | `specs/archived-tasks/spec.md` | 已随 MS02 完成超过 30 天 | 历史 Runbook 和归档 change | 需要重新规划 MMIO 轮询网络基线 |
| T04 | `.claude/docs/tasks.md` | Archive | `specs/archived-tasks/spec.md` | 已随 MS03 完成超过 30 天 | MS08 回归仍使用 MS03 名称 | 需要重新规划 MMIO IRQ 事实工作 |
| T05 | `.claude/docs/tasks.md` | Archive | `specs/archived-tasks/spec.md` | 已随 MS04 完成超过 30 天 | MS08 延续既有 wake 行为 | 需要重新规划 IRQ 唤醒原语 |
| T06 | `.claude/docs/tasks.md` | Archive | `specs/archived-tasks/spec.md` | 已随 MS04 完成超过 30 天 | MS08 延续既有 RX 行为 | 需要重新规划 QEMU 异步 RX 基线 |
| MS01 | `.claude/docs/tasks.md` | Archive | `specs/archived-tasks/spec.md` | 2026-07-29 完成，详细 change 已归档 | MS08 最终回归仍引用 MS01 | MS01 行为基线需要重新成为路线任务 |
| MS02 | `.claude/docs/tasks.md` | Archive | `specs/archived-tasks/spec.md` | 2026-07-29 完成，详细 change 已归档 | R45 等历史 Runbook | MS02 行为基线需要重新成为路线任务 |
| MS03 | `.claude/docs/tasks.md` | Archive | `specs/archived-tasks/spec.md` | 2026-08-03 完成，详细 change 已归档 | MS08 复用 MS03 harness | MS03 行为基线需要重新成为路线任务 |
| MS04 | `.claude/docs/tasks.md` | Archive | `specs/archived-tasks/spec.md` | 2026-08-12 完成，详细 change 已归档 | MS08 复用 MS04 harness | MS04 行为基线需要重新成为路线任务 |
| MS16 | `.claude/docs/tasks.md` | Archive | `specs/archived-tasks/spec.md` | 2026-08-06 完成，详细 change 已归档 | R47、R49、I16 和 benchmark spec | MS16 本身重新进入路线；后续 benchmark 清理不要求恢复本里程碑 |

## 明确排除

- 活跃 change `ms08-qemu-multi-hart-correctness-baseline` 及其全部 Iteration、Cycle 和任务。
- T07–T25、MS05–MS15；其中近期完成项继续保留，未完成项不得归档。
- `CLAUDE.md`、SNAPSHOT 正文、Runbook 正文、Analysis 正文和 Incident 正文。
- R47/R49 benchmark 身份机制；它跨行为规格和产品测试代码，必须由单独的正常 OpenSpec change 处理。
- 既有 OpenSpec archive 和 `.claude/**/_archive/` 内容。

## 恢复

恢复由 `openspec-docs-maintainer` 执行：从源文档的 `arc` 指引定位本 proposal，再按映射表中的原编号和 carrier spec 原文插回原位置。恢复长期完成任务时，应先确认它确实重新成为当前路线的一部分。
