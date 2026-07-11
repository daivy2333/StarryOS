# Proposal: ARC-202607111510 — Q19C D1 async UART 收尾归档

## 归档概述

日期: 2026-07-11
类型: Archive / Compress-Archive / Analysis-Archive
触发: 用户确认 D1 真板异步 UART 测试正式结束，并要求执行归档器报告的全部操作。

## 映射表

### tasks.md -> carrier spec

| 原条目 | carrier spec | 理由 |
|--------|--------------|------|
| Q19C.1-Q19C.12 | `specs/tasks/spec.md` | Q19C 已完成并归档，active tasks 只保留结束摘要 |
| Q19D.1-Q19D.6 | `specs/tasks/spec.md` | Q19D 已取消当前规划，active tasks 只保留边界 |

### optimization/spec.md -> carrier spec

| 原编号 | carrier spec | 理由 |
|--------|--------------|------|
| O78 | `specs/optimization/spec.md` | Q19C memory-root path/command 已完成 |
| O79 | `specs/optimization/spec.md` | D1 SDMMC/rootfs 已取消当前规划 |
| O81 | `specs/optimization/spec.md` | M3/rootfs-probe 已取消当前规划 |

### Analysis-Archive

以下文件移动到 `.claude/analysis/_archive/2026-07-11-q19c-d1-async-uart-closeout/`：

| 原路径 | 归档路径 |
|--------|----------|
| `.claude/analysis/q19c-lichee-full-starryos-benchmark.md` | `.claude/analysis/_archive/2026-07-11-q19c-d1-async-uart-closeout/q19c-lichee-full-starryos-benchmark.md` |
| `.claude/analysis/q19c-d1-tx-optimization.md` | `.claude/analysis/_archive/2026-07-11-q19c-d1-async-uart-closeout/q19c-d1-tx-optimization.md` |
| `.claude/analysis/q19c-m1-memory-root-path-loader.md` | `.claude/analysis/_archive/2026-07-11-q19c-d1-async-uart-closeout/q19c-m1-memory-root-path-loader.md` |
| `.claude/analysis/q19c-m2-m3-shell-sdmmc-probe.md` | `.claude/analysis/_archive/2026-07-11-q19c-d1-async-uart-closeout/q19c-m2-m3-shell-sdmmc-probe.md` |
| `.claude/analysis/lichee/M2.md` | `.claude/analysis/_archive/2026-07-11-q19c-d1-async-uart-closeout/lichee/M2.md` |
| `.claude/analysis/lichee/NewDate.md` | `.claude/analysis/_archive/2026-07-11-q19c-d1-async-uart-closeout/lichee/NewDate.md` |
| `.claude/analysis/lichee/Q19cM1.md` | `.claude/analysis/_archive/2026-07-11-q19c-d1-async-uart-closeout/lichee/Q19cM1.md` |
| `.claude/analysis/lichee/licheerv-dock-bringup.md` | `.claude/analysis/_archive/2026-07-11-q19c-d1-async-uart-closeout/lichee/licheerv-dock-bringup.md` |

## 排除项

- `CLAUDE.md`: 规则文档不由 archivist 自驱归档。
- `architecture/spec.md` ADR-052~055: 仍是 Q19C 结束和 rootfs/shell 边界的当前决策。
- `learned/spec.md` L259-L280: 仍是 D1 UART 结论、loader 边界、M3 取消边界和 Q20 复验入口；本次只修正路径引用。
- `.claude/analysis/q17-smp-memory-ordering.md`: Q17/Q20 仍活跃。
- `.claude/analysis/lichee/boot-official-backup.img.tombstone.md`: 按 README 规则保留在活跃路径，避免误把 tombstone 当 boot image。

## 恢复协议

1. 在源文档末尾 grep `ARC-202607111510` 找到 carrier spec 路径。
2. 读取 `openspec/changes/archive/2026-07-11-ARC-202607111510/specs/<源域>/spec.md`。
3. 用原编号或标题定位条目。
4. 将条目复制回源文档原位置，并更新 arc 指引。
