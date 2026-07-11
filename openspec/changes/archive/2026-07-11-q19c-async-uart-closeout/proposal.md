## Why

Q19C 的目标已经收敛为 D1 真板内核态和用户态异步 UART 性能验证。M0/M1/M2 已覆盖当前目标：benchmark manifest、D1 kernel benchmark、`/dev/console`/TTY/syscall/`tcdrain`/FIONBIO、memory-root path 和 command-entry。

M3/rootfs-probe 和 Q19D SDMMC/rootfs 已取消为当前规划。继续保留 `lichee-d1-rootfs-probe` feature、Makefile target、entry 分支和相关 cfg，会让后续维护者误以为 rootfs-probe 仍是 Q19C 收尾 gate。

这个 change 只做收尾：补最终证据表，删除取消范围的 M3/rootfs-probe 代码入口，归档已完成或被取代的 Q19C 相关 changes。

## What Changes

- 删除 `lichee-d1-rootfs-probe` feature、kernel feature、Makefile target 和 boot image 打包入口。
- 删除 `kernel/src/entry.rs` 中 Q19C-M3 rootfs-probe 分支。
- 删除因 rootfs-probe 单模式裁剪引入的 cfg 例外和互斥 guard。
- 保留 Q19C-M0/M1/M2 的 benchmark 路径：Q19B embedded、M1 memory-root path、M2 memory-root command。
- 补 Q19B embedded result 表和 Q19C-M0 evidence 表。
- 归档或准备归档 `q19c-m2-m3-acceptance-alignment`、`q19c-lichee-full-starryos-benchmark`，并确认旧 `q19c-m3-polling-console-isolation` 已归档。

## BDD Gap Scan

`request_user_input` 在当前 Default mode 不可用，无法执行交互式 AskUserQuestion。按 `openspec-plan` 默认假设补齐场景。

### Happy Path

- `make lichee-userbench`、`make lichee-fullbench-mem`、`make lichee-fullbench-command` 的源码入口仍存在。
- `lichee-d1-rootfs-probe` feature 和 `make lichee-rootfs-probe` 被删除。
- Q19C 文档记录 M0/M1/M2 是当前收尾证据。
- `openspec validate --changes` 和 `openspec validate --specs` 通过。

### Sad Path

- 不能删除 QEMU rootfs 支持、QEMU `/bin/sh` 启动链路或通用 rootfs 文档。
- 不能删除 VisionFive2 `vf2` / `axfeat/driver-sdmmc` 相关配置。
- 不能把 M3/rootfs-probe 删除写成 SDMMC/rootfs 成功。
- 不能删除 Q19B embedded userbench 或 M1/M2 fullbench 路径。

### Edge

- 旧 `q19c-m3-polling-console-isolation` 已在工作区表现为删除 + archive 目录，执行时只验证归档状态，不回滚。
- `q19c-m2-m3-acceptance-alignment` 已 Complete，执行时优先归档它，再处理主 Q19C change。
- 代码删除后，历史文档仍可保留 M3 事实，但必须标注为 canceled/historical。

## Scope

### In Scope

- Q19C 收尾 evidence 文档。
- M3/rootfs-probe feature、target、entry、cfg 清理。
- OpenSpec active changes 状态收敛和归档准备。
- 验证命令与剩余任务清单。

### Out of Scope

- 实现或调试 D1 SDMMC/block/rootfs。
- 自制或引入 shell。
- 修复 lazy file-backed COW SIGILL。
- 调整 async UART 性能路径。
- 修改 QEMU rootfs 启动链路。
- 修改 VisionFive2 SDMMC 规划。

## Impact

- `Cargo.toml`
- `kernel/Cargo.toml`
- `kernel/src/lib.rs`
- `kernel/src/drivers/mod.rs`
- `kernel/src/entry.rs`
- `Makefile`
- `.claude/docs/tasks.md`
- `.claude/docs/SNAPSHOT.md`
- `openspec/changes/q19c-lichee-full-starryos-benchmark/*`
- `openspec/changes/q19c-m2-m3-acceptance-alignment/*`
- `openspec/specs/{architecture,learned,optimization,references}/spec.md`

## Gate Notes

- `openspec new change q19c-async-uart-closeout` was used because slash command `/opsx:propose` is not available in this API session.
- Gate 1 BDD used default assumptions because interactive `request_user_input` is unavailable in Default mode.
- Gate 2 completeness is captured in `design.md` and `tasks.md`.
