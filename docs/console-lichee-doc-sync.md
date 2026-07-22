# Console 分支文档同步清单

本文用于从 `uart-lichee` 向 `console-lichee` 同步 Q31 所需文档。同步来源固定为提交 `7d44cb1`。

不要整体 cherry-pick `7d44cb1`。该提交同时包含 benchmark、D1 time conversion、binary 和日志更新。产品代码应由后续 Console iteration 按计划处理。

## 必须同步

| 路径 | 用途 |
| --- | --- |
| `openspec/changes/q31-async-uart-cpu-efficiency-benchmark/` | Q31 proposal、design、delta spec、tasks 和 000-003 iteration。003 是切换分支后的当前执行入口 |
| `.claude/analysis/async-uart-cpu-efficiency-metrics.md` | 指标范围、现有基础设施和结论边界 |
| `.claude/analysis/q31-cpu-efficiency-evidence/` | Async baseline、无效历史、有效历史、iteration 002 当前日志和 Console 冻结 baseline |
| `.claude/runbooks/benchmark-qemu-d1.md` | QEMU payload 注入、D1 烧录、日志采集和恢复流程 |
| `.claude/runbooks/qemu-build.md` | 当前 benchmark 构建与 rootfs 注入说明 |

在 `console-lichee` 工作树干净时执行：

```bash
git restore --source 7d44cb1 -- \
  openspec/changes/q31-async-uart-cpu-efficiency-benchmark \
  .claude/analysis/async-uart-cpu-efficiency-metrics.md \
  .claude/analysis/q31-cpu-efficiency-evidence \
  .claude/runbooks/benchmark-qemu-d1.md \
  .claude/runbooks/qemu-build.md
```

`003-async-evidence-declaration-closeout.md` 仍为 pending。先执行并 Review 003，再为 Console 创建下一 iteration。

## 手工合并索引

不要用当前分支文件覆盖 `openspec/specs/references/spec.md`。Console 分支需要保留自己的 R42 和 polling 文档。

只合并以下内容：

```markdown
| <!-- R43 --> `.claude/analysis/async-uart-cpu-efficiency-metrics.md` | 异步 UART CPU 效率指标与测试落地：盘点现有 S00-S40 覆盖，定义 submit fraction、通信—计算重叠、instret/byte、分段 counter delta 与证据边界 |
```

如果采用通用 Runbook，将 R41 的路径改为：

```markdown
| <!-- R41 --> | Benchmark QEMU/D1 部署 Runbook | `.claude/runbooks/benchmark-qemu-d1.md` | musl payload 构建、QEMU rootfs 注入、D1 TF 卡复制与 boot 备份/烧录、串口取证和恢复 |
```

确认 R41 已更新后，再决定是否删除旧的 `.claude/runbooks/console-benchmark-qemu-d1.md`。两份 Runbook 不应长期同时作为权威入口。

`.claude/analysis/README.md` 也应手工增加 Q31 分析入口。不要整体覆盖，因为 Console 的 `console-performance-measurement-design.md` 仍是该分支的有效输入，不应按当前分支规则归档。

## 必须保留 Console 版本

以下文件具有分支语义，不得从 `uart-lichee` 整体覆盖：

- `.claude/docs/SNAPSHOT.md`：必须继续描述 `console-lichee`、polling Console 和当前 Console 状态。
- `.claude/docs/tasks.md`：由 Console 分支现状决定，本次不做全局任务同步。
- `openspec/specs/improvements/spec.md`：保留 Console 专属 I11；当前分支版本已移除 I11。
- `openspec/specs/references/spec.md`：保留 Console R42，仅按上一节合并 R41/R43。
- `openspec/specs/polling-console-baseline/spec.md`：保留 Console polling capability。
- `openspec/changes/archive/2026-07-21-console-polling-baseline/`：保留 Console baseline 的归档 change。
- `.claude/analysis/console-performance-measurement-design.md`：保留为 Console 工作输入，不要替换为当前分支的 archive 路径。

`CLAUDE.md`、`AGENTS.md` 和 `.claude/docs/tasks.md` 在两个分支之间没有本轮必需差异，无需同步。

## 不同步现有 docs 日志

不要从当前分支覆盖以下文件：

- `docs/d1_out.md`
- `docs/qemu_out.md`
- `docs/d1_console.md`
- `docs/qemu_console.md`
- `docs/benchmark-report-async.md`

Q31 已在 `.claude/analysis/q31-cpu-efficiency-evidence/baseline/` 保存四份冻结输入。Console 新日志应按后续 iteration 写入 evidence 的 `console/`，用户需要时再覆盖临时 docs 日志。

## 同步后检查

```bash
test -f openspec/changes/q31-async-uart-cpu-efficiency-benchmark/iterations/003-async-evidence-declaration-closeout.md
test -f .claude/analysis/q31-cpu-efficiency-evidence/async/d1-fullbench-command.log
test -f .claude/analysis/q31-cpu-efficiency-evidence/baseline/console-d1.md
rg -n 'R41|R42|R43' openspec/specs/references/spec.md
rg -n 'I11|I12' openspec/specs/improvements/spec.md
openspec validate q31-async-uart-cpu-efficiency-benchmark --strict
openspec validate --changes
openspec validate --specs
git diff --check
git status --short
```

检查结果必须满足：

- Q31 active change 和 Async evidence 完整。
- Console 的 R42、I11、polling capability 和归档 change 仍存在。
- R43 指向同步后的 Async CPU-work 分析。
- 没有覆盖 Console 原始日志。
- 同步 diff 只包含计划中的文档文件。
