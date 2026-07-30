# Analysis Index

> Last updated: 2026-07-25
> 2026-07-25 relocate: `q20-evidence/`、`q31-cpu-efficiency-evidence/`、`q32-console-cpu-efficiency-evidence/` → 各自归档 change 的 `evidence/` 目录（符合 OpenSpec 规范：Change Evidence 属于 change，不驻留 `.claude/analysis/`）。
> 2026-07-22: Q31/Q32 evidence 与 comparison 从 `console-lichee` 同步。
> 2026-07-22: Q27-Q32 完成后，5 份 UART analysis 和 1 份 Console runbook 移入 `_archive/`。
> 2026-07-21 update: `console-performance-measurement-design.md` → `_archive/2026-07-21-console-performance-measurement-design.md`（console 分支专属分析）。
> 2026-07-11 update: Q19C/D1 async UART closeout analysis moved to `_archive/2026-07-11-q19c-d1-async-uart-closeout/`.
> 2026-07-04 update: 16 active-path tombstones (8 top-level + 8 in `lichee/`) were consolidated into `_archive/README.md`.

## Active

- `q17-smp-memory-ordering.md` — Q17 SMP / memory-ordering analysis; still referenced by active change Q17 and Q24 multi-hart revalidation.
- `q20-benchmark-gap-closure.md` — Q20 focused analysis; maps current benchmark code, jitter/counter/RX fixed-payload gaps, and plan inputs. Evidence relocated to `openspec/changes/archive/2026-07-13-q20-benchmark-gap-closure/evidence/`.
- `async-uart-cpu-efficiency-metrics.md` — CPU efficiency measurement methodology: submit fraction, overlap, instret/byte, counter deltas, and evidence boundaries. Evidence relocated to `openspec/changes/archive/2026-07-22-q31-async-uart-cpu-efficiency-benchmark/evidence/`.
- `q31-console-cpu-efficiency-port.md` — Q31→Q32 Console benchmark 移植分析：范围、差异、D1 time 修复、S43 hang 根因（IRQ stub）。Evidence relocated to `openspec/changes/archive/2026-07-22-q32-console-cpu-efficiency-benchmark/evidence/`.
- `arceos-true-board-validation.md` — ArceOS 真板验证方法：启动链可观测、寄存器优先、U-Boot 状态 dump、中断分层；Q17/Q24 参考。
- `async-network-project-overview.md` — StarryOS 异步高性能网卡探索总览。
- `arceos-async-network-driver-analysis.md` — ArceOS 异步网卡驱动分析。
- `embassy-network-module-evaluation.md` — Embassy 网络模块评估。
- `starryos-async-network-roadmap.md` — StarryOS 异步高性能网卡路线图。

## Special binary tombstone

- `lichee/boot-official-backup.img.tombstone.md` — pointer to the archived 10 MiB official boot partition backup. Kept in active path on purpose: the original `.img` is intentionally NOT replaced with text to avoid accidental flashing of a tombstone file. Archive copy at `_archive/2026-07-04-q19-lichee-analysis/lichee/boot-official-backup.img`.

## Archived (2026-07-22 — UART-completed cleanup)

- `_archive/uart-async-qemu-d1-first-replan.md` — Q20-Q24 roadmap 重排（已全部执行：Q20 完成、Q21/Q22 取消、Q23 决策完成）。
- `_archive/uart-backpressure-mpsc-plan.md` — TX backpressure + writer 并发边界分析（Q27a/Q27/Q28/Q29 全部完成并归档）。
- `_archive/async-uart-vs-io_uring.md` — async UART 与 io_uring 设计对比（backpressure/MPSC/writer 契约已落地）。
- `_archive/console-lichee-baseline-branch.md` — Console 基线分支设计（Q31/Q32 实验完成并归档）。
- `_archive/clippy-test-baseline-cleanup.md` — Clippy/测试基线清理（Q26 维护性清理完成）。

## Archived (earlier)

- `_archive/2026-07-21-console-performance-measurement-design.md` — Console 性能与测量设计（`console-lichee` 分支专属，I11/I12 输入材料）。
- `_archive/2026-07-04-q19-lichee-analysis/` — Q18/Q19/Q19B historical plans, Lichee platform capture logs, Q19B board evidence, and the official boot partition backup (8 files + `lichee/` subdir).
- `_archive/2026-07-11-q19c-d1-async-uart-closeout/` — Q19C planning, TX optimization, M1/M2/M3 analysis, and D1 board evidence logs after D1 async UART testing ended.
- `_archive/2026-06-24-q0-q15-analysis/` — older Q0~Q15 architecture, UART extraction, M4/Q15, and performance analysis material (14 files).
- `_archive/README.md` — single-source-of-truth index for all active-path → archive mappings; replaces the per-path tombstone files that used to live at the active paths.
- `.claude/runbooks/_archive/console-benchmark-qemu-d1.md` — Console benchmark QEMU/D1 部署 Runbook（async 版 `benchmark-qemu-d1.md` 为权威流程）。

## Restore Rule

- 在 `_archive/README.md` 的 “Active-path Tombstones” 查旧路径；更早记录用 `grep tombstone:`。
- 仅在用户明确要求时恢复；R 索引见 `openspec/specs/references/spec.md`。
