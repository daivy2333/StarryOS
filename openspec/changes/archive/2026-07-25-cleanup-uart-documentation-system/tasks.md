# UART 文档体系清理任务

## 1. 冻结清单与基线

- [x] T1.1 生成全部批准源载体清单，记录路径、类别和 SHA-256。
  - Files: proposal 与 design 中列出的 active specs、docs、analysis、Runbook、changes。
  - Acceptance: 每个源路径存在；总数写入 `coverage.txt`。
  - Verification: `find`、`sha256sum`；输出写入 Evidence。
  - Requirements: R1, R8。
- [x] T1.2 检查 Git 基线和已归档历史范围。
  - Files: 工作树、`openspec/changes/archive/`、`.claude/**/_archive/`。
  - Acceptance: 用户已有改动被识别；既有归档正文进入禁止修改清单。
  - Verification: `git status --short`、路径级 diff 清单。
  - Requirements: R7。

## 2. 提取保留信息

- [x] T2.1 收敛 project-model 与 decisions。
  - Files: `project-model/spec.md`、`decisions/spec.md`。
  - Acceptance: 只保留 OS、NIC、VF2 和通用约束；UART 专属正文进入 carrier。
  - Verification: ID 清单前后对比、链接检查。
  - Requirements: R2, R5。
- [x] T2.2 收敛 knowledge 与 improvements。
  - Files: `knowledge/spec.md`、`improvements/spec.md`、`quality-gate-baseline/spec.md`。
  - Acceptance: K09 只限制第二套 executor；通用 SMP 和测量规则保留；I05、I12 归档。
  - Verification: requirement 与 scenario 审查、ID 清单前后对比。
  - Requirements: R2, R4, R5。
- [x] T2.3 收敛 references 和保留 Runbook。
  - Files: `references/spec.md`、`incremental-merge.md`、`regression-gate.md`、`board-bringup-ladder.md`。
  - Acceptance: UART/D1 命令与失效路径不留在活跃方法文档；NIC/VF2 指针有效。
  - Verification: 本地路径检查、UART 阶段词汇审计。
  - Requirements: R2, R5, R6。

## 3. 建立完整归档载体

- [x] T3.1 归档 17 个 UART、D1、Console 主 capability spec。
  - Files: design 的 Main capability specs 表。
  - Acceptance: 目标存在且哈希与源一致；主规格目录不再包含这些 spec。
  - Verification: manifest 哈希对比、`openspec list --specs`。
  - Requirements: R1, R3, R8。
- [x] T3.2 归档 `docs/` 下 8 份 UART 文档和原始输出。
  - Files: design 的 Documents and indexes 表。
  - Acceptance: 8 个目标可恢复；`docs/x11.md` 不变。
  - Verification: manifest 哈希对比、路径 diff。
  - Requirements: R1, R3, R7, R8。
- [x] T3.3 归档 Q31 analysis、Lichee tombstone 和 UART benchmark Runbook。
  - Files: `.claude/analysis/q31-console-cpu-efficiency-port.md`、`.claude/analysis/lichee/`、`.claude/runbooks/benchmark-guide.md`。
  - Acceptance: active index 不再列为 active；archive index 有恢复入口。
  - Verification: manifest、index 路径检查。
  - Requirements: R1, R3, R6, R8。
- [x] T3.4 收编 `ARC-202607251326`。
  - Files: 旧 ARC 的 metadata、proposal、tasks 和空 specs 目录。
  - Acceptance: 原文、非法命名事实和 16/18 状态均保留。
  - Verification: manifest 哈希、OpenSpec validation 记录。
  - Requirements: R1, R4, R8。
- [x] T3.5 归档 `q17-smp-memory-ordering`。
  - Files: q17 proposal、design、spec、tasks。
  - Acceptance: task 6.1 保持未完成；归档说明标为 deferred。
  - Verification: tasks 状态对比、归档路径检查。
  - Requirements: R3, R4, R8。

## 4. 修复活跃状态

- [x] T4.1 更新 analysis index 与 references。
  - Files: `.claude/analysis/README.md`、`references/spec.md`。
  - Acceptance: active、archived 和恢复入口与磁盘一致。
  - Verification: 本地路径检查。
  - Requirements: R5, R6。
- [x] T4.2 更新 SNAPSHOT 与 tasks。
  - Files: `.claude/docs/SNAPSHOT.md`、`.claude/docs/tasks.md`。
  - Acceptance: UART 清理状态如实；当前工作指向 NIC N0；不新增 N0 实施承诺。
  - Verification: 与 `openspec list`、主规格清单交叉检查。
  - Requirements: R6。

## 5. Gate 验证

- [x] T5.1 验证归档覆盖。
  - Acceptance: `mapped=total`、`unmapped=0`、`skipped=0`，源目标哈希一致。
  - Verification: `archive-manifest.tsv` 与 `coverage.txt`。
  - Requirements: R1, R8。
- [x] T5.2 验证 Markdown 本地路径。
  - Acceptance: 活跃文档没有本次清理造成的断链。
  - Verification: 检查命令、输出和退出码写入 `link-check.txt`。
  - Requirements: R6, R8。
- [x] T5.3 验证 OpenSpec 状态。
  - Acceptance: new change validate 无 error；UART changes 不再出现在活跃列表。
  - Verification: `openspec status`、`openspec validate`、`openspec list`。
  - Requirements: R4, R6, R8。
- [x] T5.4 验证 diff 范围。
  - Acceptance: `git diff --check` exit 0；产品代码、构建资产和既有归档正文无修改。
  - Verification: 路径清单与 `diff-check.txt`。
  - Requirements: R7, R8。

## 6. Review handoff

- [x] T6.1 填写 iteration 的 Act Response。
  - Acceptance: 记录修改文件、偏差、命令、关键输出、退出码和剩余问题。
  - Verification: `iterations/000-initial.md` 内容检查。
  - Requirements: R8。
- [x] T6.2 停止并等待 Plan Review。
  - Acceptance: 不自动归档当前 cleanup change，不自动启动 NIC N0。
  - Verification: 当前 change 仍可审计。
  - Requirements: R7。

## 7. Iteration 001 Review 修正

- [x] T7.1 修复 6 个活跃分析断链。
  - Files: 4 份 NIC analysis、`arceos-true-board-validation.md`。
  - Acceptance: 链接指向现有 active 或 `_archive` 文件。
  - Verification: 活跃 Markdown 本地链接检查为 0 broken。
  - Requirements: R6。
- [x] T7.2 归档并通用化 3 份保留 Runbook。— SKIPPED: superseded by T9。
  - Files: `incremental-merge.md`、`regression-gate.md`、`board-bringup-ladder.md`。
  - Acceptance: 改写前版本完整归档；活跃版本不再保存 UART 阶段命令、D1 阈值或失效路径。
  - Verification: 源目标哈希、UART 开发流词汇审计、本地链接检查。
  - Requirements: R2, R5, R6, R8。
- [x] T7.3 收敛保留规格与 references。— SKIPPED: superseded by T9。
  - Files: M/D/K/R/I、`quality-gate-baseline`、`platform-descriptor-early-console`。
  - Acceptance: 活跃正文只保留 OS、NIC、VF2 和通用方法；PLIC 官方链接存在；失效 R 条目移除。
  - Verification: requirement 清单审查、UART 阶段词汇审计、OpenSpec validate。
  - Requirements: R2, R5, R6。
- [x] T7.4 收敛状态文档和新建归档说明。— SKIPPED: superseded by T9。
  - Files: SNAPSHOT、tasks、两个 `ARCHIVE_NOTE.md`。
  - Acceptance: tasks 不再保留 Q0-Q32 开发流；q17 deferred 状态与 I05 归档状态一致；旧 ARC 内容清单准确。
  - Verification: 与磁盘、manifest、`openspec list` 交叉检查。
  - Requirements: R4, R5, R6。
- [x] T7.5 生成 iteration 001 的 fresh Evidence。— SKIPPED: superseded by T9。
  - Files: `evidence/001-review-fixes/`。
  - Acceptance: 每个归档文件有 SHA-256；命令、关键输出和退出码齐全；产品代码无修改。
  - Verification: final manifest、coverage、link、OpenSpec、scope、diff 检查。
  - Requirements: R1, R7, R8。
- [x] T7.6 填写 iteration 001 Act Response 并停止。— SKIPPED: superseded by T9。
  - Acceptance: 记录偏差、验证和剩余问题；不归档当前 cleanup change。
  - Verification: `iterations/001-review-fixes.md` 内容检查。
  - Requirements: R7, R8。

## 8. Iteration 002 归档完整性收尾

- [x] T8.1 补齐被修改 meta spec 的基线归档。
  - Files: HEAD 中的 M/D/K/R/I、quality-gate、platform descriptor。
  - Acceptance: 清理前全文进入 cleanup carrier，源哈希可从 Git 基线复算。
  - Verification: `git show HEAD:<path>` 与归档副本 SHA-256。
  - Requirements: R1, R2, R8。
- [x] T8.2 清理 references 和保留 Runbook 的剩余 UART 开发流。— SKIPPED: superseded by T9。
  - Files: `references/spec.md`、`board-bringup-ladder.md`，按审计需要连带另外两份 Runbook。
  - Acceptance: 不存在失效 R3/R43/R1 正文；bring-up Runbook 不保留 D1 阶段编号或 UART benchmark 阈值。
  - Verification: plain-path 审计、词汇审计、本地链接检查。
  - Requirements: R2, R5, R6。
- [x] T8.3 修正 SNAPSHOT、tasks 和 I06 的状态语义。— SKIPPED: superseded by T9。
  - Files: SNAPSHOT、tasks、improvements。
  - Acceptance: 不把 q17 deferred 表述为 UART 工作全部完成；旧 Q 触发条件改为当前 N 阶段或硬件条件。
  - Verification: 与 q17 ARCHIVE_NOTE、`openspec list` 交叉检查。
  - Requirements: R4, R5, R6。
- [x] T8.4 重建可复核的 final Evidence。— SKIPPED: superseded by T9。
  - Files: `evidence/002-closeout/`。
  - Acceptance: 三个归档目录逐文件覆盖；命令、输出、退出码齐全；覆盖统计与 manifest 行数一致。
  - Verification: manifest、hash、link、plain-path、OpenSpec、scope、diff 检查。
  - Requirements: R1, R7, R8。
- [x] T8.5 填写 iteration 002 Act Response 并停止。— SKIPPED: superseded by T9。
  - Acceptance: tasks 状态同步；不归档当前 cleanup change。
  - Verification: `iterations/002-closeout.md` 内容检查。
  - Requirements: R7, R8。

## 9. Iteration 003 契约化收尾

- [x] T9.1 保存 RED 基线并冻结修改范围。
  - Files: iteration 003 指定的活跃控制文档、两个 `ARCHIVE_NOTE.md`、三个 carrier、工作树。
  - Acceptance: 每个禁用模式在修改前的命中、R allowlist 差异、48 个 carrier 哈希和产品代码路径均有记录。
  - Verification: 命令、输出和退出码写入 `evidence/003-contract-closeout/red-baseline.txt`。
  - Requirements: R7, R8, R9。
- [x] T9.2 统一当前状态与 deferred 语义。
  - Files: `.claude/docs/SNAPSHOT.md`、`.claude/docs/tasks.md`、`openspec/specs/improvements/spec.md`、q17 与旧 ARC 的 `ARCHIVE_NOTE.md`。
  - Acceptance: 两份状态文档使用约定的 archived/deferred 句；无 UART 全部完成声明；I06 无 Q17-Q25；两个 note 保留 18/19、16/18 和未完成任务。
  - Verification: 状态禁用模式、约定句、I06 触发条件和归档任务计数检查。
  - Requirements: R4-R6, R9。
- [x] T9.3 收敛索引、R allowlist 与活跃 meta spec。
  - Files: `.claude/analysis/README.md`、M/D/K/R/I、`quality-gate-baseline`、`platform-descriptor-early-console`。
  - Acceptance: R 编号集合精确等于 R14、R23-R26、R38-R40；Active analysis 精确等于 5 个现存文件；无旧 ARC 活跃路径；无 Q0-Q32 当前阶段语义。
  - Verification: 集合 diff、plain-path 检查、Q 阶段禁用模式、`openspec validate --all`。
  - Requirements: R2, R5, R6, R9。
- [x] T9.4 将 board bring-up Runbook 收敛为通用流程。
  - Files: `.claude/runbooks/board-bringup-ladder.md`。
  - Acceptance: 保留 generic early serial/Console；移除 D1 专属步骤、UART benchmark、Q 阶段和旧阈值。
  - Verification: Runbook 禁用模式和本地路径检查。
  - Requirements: R2, R5, R6, R9。
- [x] T9.5 生成 GREEN Evidence 并执行全部 Gate。
  - Files: `evidence/003-contract-closeout/`。
  - Acceptance: 48 个 carrier 文件均在新 manifest 中且哈希匹配；只有两个 ARCHIVE_NOTE 可相对 002 manifest 改变；所有内容、OpenSpec、diff 与范围 Gate 通过。
  - Verification: `references-check.txt`、`state-check.txt`、`active-path-check.txt`、`runbook-check.txt`、`archive-integrity.txt`、`openspec-validation.txt`、`scope-and-diff-check.txt`。
  - Requirements: R1, R4-R9。
- [x] T9.6 同步任务状态并填写 Act Response。
  - Acceptance: T9.1-T9.5 全部通过后，T7.2-T7.6 与 T8.2-T8.5 标记 `SKIPPED: superseded by T9`；记录命令、输出、退出码和剩余问题；不归档 change。
  - Verification: `openspec list`、tasks 检查、iteration 003 内容检查。
  - Requirements: R7-R9。
