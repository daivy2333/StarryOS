## Why

Q19C M2 已经完成，M3/rootfs-probe 不再作为当前验收目标。

用户已确认：StarryOS 当前没有自研 shell，QEMU 的 shell 来自现有 rootfs 镜像。Q19C 不应把“自制或引入静态 `/bin/sh`”作为必达目标。M2 应收敛为 `lichee-memory-root-command`，true shell 只作为后续可选升级。

同时，当前实现暴露了几个验收风险：

- 全局任务状态可能把 host gate 与 board gate 混成已完成。
- M2/M3 feature 是独立运行模式，但组合 feature 可能出现不可达路径或编译失败。
- M2 的 argv/envp 证据偏 kernel 侧，若要声称用户态观察到参数，需要 payload 证据。
- M3 是 storage/rootfs probe-only 方向，不再属于 async UART 性能验证 gate。

本 change 用来补齐这些边界，避免 Q19C 归档时把简化项写成已完成能力。

## What Changes

- 将 Q19C M2 必达目标明确为 `lichee-memory-root-command`。
- 将 true shell path 从 Q19C 必达项移到 future optional。
- 将 M3/rootfs-probe 从 Q19C 必达项移到取消当前规划。
- 明确 M2/M3 feature 组合策略：互斥或显式 `compile_error!`，不得依赖偶然构建行为。
- 修正任务状态表达：M2 board gate 已完成；M3 board gate 取消当前规划。
- 约束 M2 证据口径：kernel 侧 argv/envp 与用户态 argv/envp 观察必须分开记录。
- 约束 M3 证据口径：历史 rootfs-probe 不得声称寄存器 probe 或 rootfs benchmark 成功，也不得阻塞 Q19C。

## Capabilities

### Modified Capabilities

- `lichee-d1-fullbench`: 补充 M2/M3 acceptance alignment。该补丁不新增用户可见功能，只修正 fullbench capability 的验收边界和执行任务。

### New Capabilities

无。

## Scope

### In Scope

- OpenSpec Q19C proposal/design/spec/tasks 的 M2 语义修正与 M3 取消口径。
- `.claude/docs/tasks.md` 中 Q19C 全局任务状态与 board gate 待测状态对齐。
- feature 互斥 guard 或等价文档/构建保护。
- M2 argv/envp 证据增强，或将现有证据降级为 kernel-side argv construction。
- M3 probe-only 日志与取消口径对齐。
- host 验证命令和 D1 board gate 模板更新。

### Out of Scope

- 自制 shell。
- 引入 busybox 或其他静态 shell payload。
- 修复 memory-root/tmpfs lazy file-backed COW loader bug。
- 实现 D1 SDMMC/block driver。
- 完成真实 rootfs benchmark。

## BDD Gap Scan

2026-07-10：当前 Default mode 下不使用交互式 AskUserQuestion。本 change 按用户明确决策补齐场景：放弃 Q19C true shell 必达目标，保留 command-entry 作为 M2。

### Happy Path

- M2 command image 单独构建并启动，日志标记 `lichee-memory-root-command`。
- 日志明确 `shell_status=SKIPPED: no known-good static /bin/sh`。
- M2 证据覆盖 stdio、spawn/join、exit code，并区分 kernel-side argv construction 与 user-observed argv。
- M3 probe image 的历史 host gate 可保留，但 board gate 取消当前规划。
- 全局任务状态显示 M2 board done、M3 canceled.

### Sad Path

- 任意 true shell 文案不得被记录为 Q19C 成功条件。
- 不兼容 feature 同时启用时，构建必须失败并给出明确错误，或任务文档必须禁止组合。
- M3 若没有寄存器读或 PIO block read，不得记录为 SDMMC register probe success。
- 取消 M3 不得被写成 SDMMC/rootfs 成功。

### Edge

- 如果后续用户提供可用静态 shell，应作为新的 follow-up change，而不是回写为本 change 的完成条件。
- 如果 benchmark payload 不读取 argv/envp，M2 仍可通过 command-entry host/board gate，但证据名称必须写成 kernel-side argv construction。
- `lichee-d1-fullbench`、`lichee-d1-fullbench-command`、`lichee-d1-rootfs-probe` 等 mode feature 应按单模式构建处理。

## Impact

- `openspec/changes/q19c-lichee-full-starryos-benchmark/*` — 修正 Q19C M2/M3 目标、任务和验收语义。
- `openspec/changes/q19c-lichee-full-starryos-benchmark/specs/lichee-d1-fullbench/spec.md` — 增量要求收敛为 command-entry，M3/rootfs-probe 取消当前 gate。
- `.claude/docs/tasks.md` — 修正 Q19C.10/Q19C.11 或等价任务状态。
- `Cargo.toml` / `kernel/Cargo.toml` / `kernel/src/lib.rs` 或等价位置 — 可加入 feature 互斥 guard。
- `tests/benchmark.c` 或等价 payload — 可选增强 argv/envp user-observed witness；若不增强，文档必须降级证据口径。
