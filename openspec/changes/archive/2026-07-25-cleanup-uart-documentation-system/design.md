# 文档清理设计

## Baseline

当前活跃文档存在四类问题：

- `ARC-202607251326` 名称非法，carrier specs 为空，任务为 16/18。
- `q17-smp-memory-ordering` 有 1 项 multi-hart 验证未执行。
- 主规格仍有 19 个 capability spec，其中 17 个仅服务 UART、D1 或 Console。
- SNAPSHOT、tasks、references 和 analysis index 对归档状态的描述不一致。

归档历史和 migration carrier 不可改写。本变更通过新 change 记录第二轮审计与收尾。

## Disposition rules

按以下顺序判断每个信息单元：

1. 当前 OS、NIC 或 VF2 是否仍依赖该约束。
2. 是否为已验证且可复用的工程知识。
3. 是否只是 UART 阶段计划、证据、原始输出或历史选择。
4. 是否已有其他权威位置。

前两项为真时保留或迁移。仅第三项为真时归档。已有权威位置时只保留指针。

## Carrier map

### Main capability specs

| Disposition | Paths |
|---|---|
| Keep | `openspec/specs/quality-gate-baseline/spec.md` |
| Keep | `openspec/specs/platform-descriptor-early-console/spec.md` |
| Archive | `arceos-adapter`, `async-uart-core`, `async-uart-traits` |
| Archive | `ier-isr-refactor`, `ier-port-ownership`, `uart-temt-query` |
| Archive | `tx-bounded-fast-retry`, `tx-completion-tracking`, `tty-tx-backpressure` |
| Archive | `inline-batch-optimize`, `maintenance-cleanup`, `benchmark-gap-closure` |
| Archive | `lichee-d1-early-smoke`, `lichee-d1-benchmark`, `lichee-d1-fullbench` |
| Archive | `uart-cpu-efficiency-benchmark`, `console-cpu-efficiency-benchmark` |

归档目标必须记录完整源路径、目标路径和 SHA-256。归档不能表示对应产品功能被删除。

### M/D/K/R/I

| Domain | Retain or normalize | Archive |
|---|---|---|
| M | OS runtime、日志、LTO、最小接口、Gate、NIC、VF2、SMP 通用规则 | UART ring、io_uring-UART 计划、Q28 后并发流 |
| D | runtime、LTO、NIC、VF2 | UART buffer 演进 |
| K | ISR、poll、waker、runtime 边界、OpenSpec、SMP、真板、SPSC、NIC 迁移、清理教训 | 仅解释 Q28 或 UART completion 的内容 |
| R | NIC 分析、VF2 分析、通用 Runbook、必要外部规范 | UART 硬件、serial、Q31/Q32、D1 和旧子项目状态 |
| I | VF2/NIC 候选 | Q17 UART stress、UART benchmark 改进 |

`K09` 必须收紧为“不得引入第二套 executor”。不得继续把所有 Embassy 网络原语视为反模式。

`M39` 与 `K16` 保留通用原子内存序规则。UART 字段细节进入归档 carrier。

`I12` 的通用测量规则迁入 `quality-gate-baseline` 或保留的验证 Runbook 后归档。`I05` 随 Q17 归档，不转写为已完成工作。

### Documents and indexes

| Disposition | Paths |
|---|---|
| Archive | `docs/async-uart-architecture.md` |
| Archive | `docs/benchmark-report-async.md` |
| Archive | `docs/manual-qa-report.md` |
| Archive | `docs/uart-async-learning-map.md` |
| Archive | `docs/d1_console.md`, `docs/d1_out.md` |
| Archive | `docs/qemu_console.md`, `docs/qemu_out.md` |
| Archive | `.claude/analysis/q31-console-cpu-efficiency-port.md` |
| Archive | `.claude/analysis/lichee/` |
| Archive | `.claude/runbooks/benchmark-guide.md` |
| Normalize | `.claude/runbooks/incremental-merge.md` |
| Normalize | `.claude/runbooks/regression-gate.md` |
| Normalize | `.claude/runbooks/board-bringup-ladder.md` |
| Normalize | `.claude/analysis/arceos-true-board-validation.md` |
| Update | `.claude/analysis/README.md` |
| Update | `openspec/specs/references/spec.md` |
| Update | `.claude/docs/SNAPSHOT.md`, `.claude/docs/tasks.md` |
| Unchanged | `docs/x11.md` |

保留的 Runbook 只保存可重复操作。UART 命令、D1 数值和失效路径进入归档副本。

## Change lifecycle

`q17-smp-memory-ordering` 以 deferred 状态归档。其 6.1 保持未完成，归档说明明确 QEMU single-hart 不能证明 multi-hart 正确性。

`ARC-202607251326` 作为非法且未完成的历史载体收编。其 proposal、tasks 和已有映射保持原文，新的归档清单补足目标路径和哈希。

本 change 的实施只整理内容和建立证据。最终更新全局状态和归档 change 仍需用户单独授权相应生命周期步骤。

## Ordering

1. 冻结源清单和哈希。
2. 提取仍有用的信息。
3. 建立完整 archive carrier。
4. 移动 UART 专属载体。
5. 更新 M/D/K/R/I、索引和状态文档。
6. 验证路径、内容覆盖和 OpenSpec 状态。
7. Review 后再决定收尾与归档。

这个顺序避免先移动来源后才发现可复用信息没有落点。

## Evidence

Persisted Evidence 为 `required`，目录为：

`openspec/changes/cleanup-uart-documentation-system/evidence/000-initial/`

必须包含：

- `archive-manifest.tsv`：源路径、分类、目标路径、源 SHA-256、目标 SHA-256。
- `coverage.txt`：总数、mapped、unmapped、skipped。
- `link-check.txt`：活跃 Markdown 本地路径检查命令、输出和退出码。
- `openspec-validation.txt`：status、validate、list 的命令、输出和退出码。
- `diff-check.txt`：`git diff --check`、产品代码未修改检查和退出码。
- `README.md`：采集环境、结果摘要和 Gate 结论。

通过条件：

- 源目标哈希一致。
- `unmapped=0` 且 `skipped=0`。
- 活跃本地引用不存在已知断链。
- OpenSpec 验证无 error。
- 产品代码和已归档历史正文无改动。

## Requirements traceability

| Requirement | Tasks | Coverage | Simplification | Status |
|---|---|---:|---|---|
| R1：逐载体映射 | T1.1, T3.1-T3.5, T5.1, T7.5, T8.1, T8.4, T9.5 | 100% | None | Covered |
| R2：唯一权威位置 | T2.1-T2.3, T7.2-T7.4, T8.2, T8.3, T9.3, T9.4 | 100% | None | Covered |
| R3：UART 开发流归档 | T3.1-T3.3, T3.5 | 100% | None | Covered |
| R4：未完成状态如实保留 | T3.4, T3.5, T5.3, T7.4, T8.3, T9.2, T9.5 | 100% | None | Covered |
| R5：保留 OS/NIC/VF2 信息 | T2.1-T2.3, T4.1, T7.2-T7.4, T8.2, T8.3, T9.2-T9.5 | 100% | None | Covered |
| R6：修复活跃状态 | T2.3, T4.1, T4.2, T5.2, T5.3, T7.1-T7.4, T8.2, T8.3, T9.2-T9.5 | 100% | None | Covered |
| R7：限制修改范围 | T1.2, T3.2, T5.4, T6.2, T7.5, T8.4, T9.1, T9.5, T9.6 | 100% | None | Covered |
| R8：持久化验证证据 | T1.1, T3.1-T3.5, T5.1-T5.4, T6.1, T7.5, T8.1, T8.4, T9.1, T9.5, T9.6 | 100% | None | Covered |
| R9：机械验收与 RED/GREEN | T9.1-T9.6 | 100% | None | Covered |

## Iteration 003 验收契约

前三轮失败来自局部替换和手写结论。003 使用以下边界：

- `references/spec.md` 的 R 编号只能是 R14、R23-R26、R38-R40。
- references 只保留一个 cleanup carrier 路径。历史 R 正文由 meta-spec 基线和 carrier 恢复。
- SNAPSHOT 与全局 tasks 使用“UART 文档已归档；q17 multi-hart SMP 验证 deferred（task 6.1 未完成）”。
- SNAPSHOT 不保留 UART 阶段回顾、旧性能数字或 Q0-Q32 完成声明。
- 全局 tasks 不保留 UART Q0-Q32 任务段。
- I06 不使用 Q17-Q25。触发条件只使用 N3-N5 或硬件条件。
- M/D/K/R/I、quality-gate 和 platform descriptor 不使用 Q0-Q32 描述当前约束。
- 旧 ARC 的活跃路径全部改为 `openspec/changes/archive/2026-07-25-arc-202607251326/`。
- analysis index 的 Active 列表只包含磁盘上的 5 份活跃分析。
- board bring-up Runbook 可保留 generic early serial/Console，但不保留 D1 专属步骤、UART benchmark 或 Q 阶段。
- q17 与旧 ARC 的 `ARCHIVE_NOTE.md` 保留未完成计数，不再写 UART 阶段完成。

当前 UART 产品行为、`uart_16550` 质量 Gate、D1 capability 约束和 UART→NIC 可复用知识不因关键词命中而删除。它们只移除历史阶段编号和失效路径。

003 Evidence 写入 `evidence/003-contract-closeout/`。000-002 Evidence 不修改。若修正两个 `ARCHIVE_NOTE.md`，003 manifest 必须重算 48 个 carrier 文件；除这两个 note 外，其余文件必须与 002 manifest 哈希一致。

## Risks

- 误把当前有用约束当作 UART 历史。通过先提取后归档降低风险。
- 主 spec 归档后导航丢失。通过 references 和 manifest 保留恢复入口。
- 非法旧 ARC 无法通过 CLI。新 change 负责记录其生命周期，不伪造旧 change 状态。
- 大批移动掩盖无关修改。实施前后都要检查 Git diff 路径范围。
