## Context

Q19C 原始目标是让 D1 真板从 Q19B embedded userbench 走向更接近 QEMU 的用户态启动链路。M1 已证明 memory-root `/bin/benchmark` 可通过 VFS path resolve/read 和 eager ELF mapping 运行。

M2 原先写成 shell/script parity。但 StarryOS 当前没有自研 shell，QEMU 使用的是 rootfs 中的 `/bin/sh`。因此 Q19C 不应把“获得一个静态 `/bin/sh` 并跑 shell script”作为当前 change 的完成条件。

M3 原先写成 SDMMC/rootfs probe。当前实现是保守的 rootfs-probe mode：不猜 D1 SDMMC MMIO 常量，不调用空 block list 的 `init_filesystems()`，只记录 blocker。该边界应写清楚。

## Goals / Non-Goals

**Goals:**

- M2 验收目标收敛为 `lichee-memory-root-command`。
- true shell path 明确为 future optional。
- M2/M3 单模式 feature 组合有明确保护。
- host gate 与 board gate 在任务状态中分开。
- M2 argv/envp 证据不夸大。
- M3 probe-only 证据不夸大。

**Non-Goals:**

- 不实现 shell。
- 不引入 busybox。
- 不修复 loader lazy COW。
- 不实现 SDMMC/block/rootfs。
- 不要求 M2/M3 真板 gate 在规划阶段完成。

## Decisions

### D1: Q19C M2 使用 command-entry 作为完成目标

选择：Q19C M2 的必达目标是 `lichee-memory-root-command`，不是 true shell。

原因：

- StarryOS 没有 shell 实现。
- 现有 QEMU shell 来自 rootfs 镜像，不是内核能力。
- 自制 shell 或打包静态 shell 会扩大范围。
- command-entry 已能验证 M2 当前关心的内核路径：VFS-visible payload、argv/envp construction、stdio、spawn/join、exit code。

### D2: true shell 只保留为 future optional

选择：Q19C 文档中可以保留 true shell 的参考语义，但不得作为 Q19C gate。

接受条件：

- 文档必须写明 `shell_status=SKIPPED` 是 Q19C 可接受结果。
- 不得记录 `lichee-memory-root-shell` success，除非 `/bin/sh -c /init.sh` 真实运行。
- 后续若需要 true shell，应另开 change，输入为“已选定静态 shell payload 与依赖策略”。

### D3: M2/M3 mode feature 按互斥模式处理

选择：每个 Lichee fullbench mode 应单独构建。若实现保留多个 feature，应加互斥 guard 或等价构建约束。

已观察风险：

- `lichee-d1-fullbench-command + lichee-d1-fullbench` 可编译，但 M1 先进入 loop，M2 路径不可达。
- `lichee-d1-rootfs-probe + lichee-d1-fullbench-command` 会因 probe mode 裁剪模块而编译失败。

推荐实现：

```rust
#[cfg(all(feature = "lichee-d1-fullbench", feature = "lichee-d1-fullbench-command"))]
compile_error!("select exactly one Lichee fullbench mode feature");

#[cfg(all(feature = "lichee-d1-rootfs-probe", feature = "lichee-d1-fullbench-command"))]
compile_error!("select exactly one Lichee fullbench mode feature");
```

具体 feature 名以当前源码为准。

### D4: M2 argv/envp 证据分两级

选择：区分 kernel-side argv construction 和 user-observed argv。

验收口径：

- 如果 payload 不读取 argv/envp，可记录 kernel-side argv/envp construction。
- 如果要声明 user-observed argv/envp，payload 必须打印 `argc/argv/envp` 或等价 marker。
- M2 command-entry 不因 payload 未打印 argv 而失败，但证据名称不能夸大。

### D5: M3 是 rootfs readiness blocker report

选择：当前 M3 是 `lichee-rootfs-probe` 的 blocker report，不是 SDMMC register probe 或 rootfs path benchmark。

验收口径：

- 未确认 controller base/init sequence 时，日志应写 `SKIPPED: controller base/init sequence not confirmed`。
- 未注册 D1 `AxBlockDevice` 时，日志应写 `SKIPPED: missing D1 SDMMC/block driver`。
- 不调用 `axfs-ng::init_filesystems()`。
- 不记录 rootfs benchmark success。

## Requirements Traceability Matrix

| Requirement | Task(s) | Coverage | Simplification | Status |
|-------------|---------|----------|----------------|--------|
| R1 M2 command-entry is accepted target | 1.1-1.4, 4.1 | 100% | true shell deferred | Covered with user-approved simplification |
| R2 true shell not claimed | 1.2, 1.5, 4.1 | 100% | none | Covered |
| R3 feature modes are mutually exclusive | 2.1-2.4 | 100% | guard may be compile_error or documented single-mode target | Covered |
| R4 host/board task status separated | 3.1-3.3 | 100% | none | Covered |
| R5 argv/envp evidence is named accurately | 4.1-4.3 | 100% | user-observed argv optional | Covered |
| R6 M3 probe-only evidence is named accurately | 5.1-5.4 | 100% | no register probe unless implemented | Covered |

Gate 2 result: no uncovered requirement. The only simplification is user-approved: Q19C drops true shell as a required goal.

## Verification Plan

### Host Verification

- `openspec validate q19c-m2-m3-acceptance-alignment --strict`
- `openspec validate --changes`
- `openspec validate --specs`
- `AX_CONFIG_PATH=$PWD/.axconfig.toml cargo check --target riscv64gc-unknown-none-elf --features "axfeat/myplat axfeat/bus-mmio lichee-d1-fullbench-command"`
- `AX_CONFIG_PATH=$PWD/.axconfig.toml cargo check --target riscv64gc-unknown-none-elf --features "axfeat/myplat axfeat/bus-mmio lichee-d1-rootfs-probe"`
- Negative feature-combination check if compile_error guards are added.
- `make lichee-fullbench-command`
- `make lichee-rootfs-probe`

### Board Verification

M2 board log must include:

- image name,
- `lichee-memory-root-command`,
- `shell_status=SKIPPED: no known-good static /bin/sh` or more specific blocker,
- argv/envp evidence label,
- `/dev/console` stdio marker,
- benchmark sections,
- `benchmark exited with code: 0`.

M3 board log must include:

- image name,
- `lichee-rootfs-probe`,
- D1 SDMMC/block known facts,
- `rootfs_init=NOT called`,
- `SKIPPED: missing D1 SDMMC/block driver` or more specific blocker,
- no `No block device found!` panic.
