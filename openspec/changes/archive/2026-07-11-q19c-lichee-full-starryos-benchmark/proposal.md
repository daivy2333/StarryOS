## Why

Q19B 已证明 Lichee RV Dock / Allwinner D1 上的 StarryOS 可以完成最小用户态 benchmark：D1 async UART、PLIC IRQ 18、`/dev/console`、TTY、syscall、`tcdrain`、FIONBIO 和 embedded benchmark ELF 全链路通过。

但 Q19B 的启动链路仍然是专用路径：

```text
include_bytes!("../resources/benchmark.elf")
  -> load_embedded_user_app()
  -> Process::new_init()
  -> /dev/console stdio
  -> run benchmark
```

它绕过了 QEMU 正常运行 StarryOS 时最关键的用户态入口：

```text
pseudofs::mount_all()
  -> FS_CONTEXT.resolve(args[0])
  -> load_user_app()
  -> /bin/sh -c init.sh
  -> run applications / benchmark
```

Q19C 的目标已在 2026-07-11 收敛：在 Lichee RV Dock 上验证内核态和用户态异步 UART 性能参数。M0/M1/M2 已覆盖 benchmark manifest、内核态 benchmark、用户态 `/dev/console`/TTY/syscall/`tcdrain`/FIONBIO、memory-root path 和 command-entry。Shell、SDMMC、block、真实 rootfs 不再是 Q19C 必达目标。

## What Changes

Q19C 分成当前验收部分和取消部分。

### Part A: StarryOS 内部启动链路（保留）

这一部分不依赖真实 SD 卡/rootfs，重点是让 Lichee 在 memory-root 下具备接近 QEMU 的应用启动语义。

- 新增独立 Lichee fullbench runtime mode，与 Q19B `lichee-d1-userbench` 分离。
- 在 memory-root 中提供 `/bin/benchmark`，通过 VFS 路径解析启动 benchmark。
- M1 通过 `FS_CONTEXT.resolve()/read()` + eager ELF mapping 覆盖 memory-root 路径可见性、文件读取、argv/envp、stdio、process exit/join；`load_user_app()` 的 lazy file-backed COW 路径另作为 loader/mm 后续问题处理。
- 在 memory-root 中提供 `/bin/benchmark` 和 `/init.sh` 文本证据；M2 必达目标为 `lichee-memory-root-command`（documented equivalent command entry）。true shell path（`/bin/sh -c /init.sh`）仅作为 future optional，Q19C 不要求实现或引入静态 shell。

### Part B: 真板 rootfs / SDMMC 探针（取消当前规划）

这一部分曾用于采集 storage/rootfs blocker。2026-07-11 方向更新后，它不再属于异步 UART 性能验证目标，也不再作为 Q19C gate。已有 host/probe 代码和 `docs/M3.md` 只保留为历史事实。

- 不继续采集 D1 SDMMC controller / IRQ / clock / reset / pinmux / card-detect。
- 不继续调试 `lichee-rootfs-probe` board gate。
- 不创建 Q19D SDMMC/rootfs follow-up。
- 若以后目标转向 storage/rootfs bring-up，必须重新 propose。

## Capabilities

### New Capabilities

- `lichee-d1-fullbench`: 定义 Lichee D1 async UART benchmark 的启动链路、memory-root path/command 行为、真板采集要求和验证标准。Shell/rootfs 不作为当前 gate。

### Modified Capabilities

Q19C 不修改已归档的 Q19/Q19B capability。Q19B embedded userbench 继续作为独立可运行的真板回归路径。

## Scope

### In Scope

- 新增 Lichee fullbench feature/target/image 命名。
- memory-root 中的文件节点和应用布局。
- Lichee fullbench 使用 VFS-visible `/bin/benchmark` 启动；M1 当前采用 `FS_CONTEXT.resolve()/read()` + eager ELF mapping。
- command-entry benchmark 触发（documented equivalent command entry，覆盖 argv/envp/stdio/exit/join）；true shell path 作为 future optional。
- benchmark 输出和证据格式标准化。

### Out of Scope

- 把 D1 直接启用 `qemu` feature。
- 删除 Q19B embedded userbench。
- 在缺少真实 block device 时强行调用 `axfs-ng::init_filesystems()`。
- 将 memory-root 结果标记为真实 rootfs 结果。
- 把 VisionFive2 的 Q20 工作混入 Lichee Q19C。
- 在 Q19C 内承诺完成 D1 SDMMC 完整驱动移植、真实 block rootfs 挂载和 rootfs path benchmark。
- 继续把 M3/rootfs-probe 或 Q19D SDMMC/rootfs 作为当前规划。

## User-Visible Behavior

- `make lichee-userbench` 或既有等价目标继续产生 Q19B embedded benchmark image。
- 新增 fullbench 目标产生独立 boot image，串口日志必须打印当前 mode。
- memory-root path fullbench 日志必须包含 benchmark 是通过 VFS path 启动，而非 embedded loader。
- command-entry mode 日志必须包含 `lichee-memory-root-command` label、`shell_status=SKIPPED` blocker、argv/envp construction evidence、stdio marker 和 `benchmark exited with code: 0`。不得声称 shell-launched success。
- rootfs/probe 日志不再作为 Q19C 验收项。已有 M3 结果只记录为取消前的历史事实。

## BDD Gap Scan

### Happy Path

- Lichee memory-root fullbench 从 `/bin/benchmark` 启动 benchmark，输出与 Q19B 同类 benchmark sections。
- Lichee memory-root command-entry mode 从 `/bin/benchmark` 启动 benchmark，覆盖 argv/envp construction、stdio、spawn/join、exit code；无可靠 shell 时记录 `shell_status=SKIPPED`。
- Q19C 不要求 Lichee rootfs probe mode。M3/rootfs-probe 若未完成，不阻塞 Q19C。

**BDD update (2026-07-11)**: 用户确认当前目标是 D1 内核态 + 用户态异步 UART 性能参数验证。M2 documented equivalent command entry 保留；M3/rootfs-probe 和 Q19D storage/rootfs 取消当前规划。

### Sad Path

- benchmark path 不存在时输出明确错误，不静默 halt，不记录成功。
- shell/interpreter/shared library 缺失时输出缺失路径和启动阶段。
- 没有 block device 或 SDMMC probe 失败时，不得把它记录为 Q19C async UART benchmark failure。

### Edge

- Q19B embedded userbench 不能因 Q19C feature 新增而改变语义。
- memory-root mode 必须明确标记为 non-persistent root，不等同真实 rootfs。
- documented equivalent command entry 必须独立标记为 command/equivalent，不得记录为 shell-launched benchmark success。
- 如果 shell 是动态 ELF，dynamic linker 路径和依赖库必须由同一 rootfs 提供。
- Android boot image size 必须持续检查，避免 fullbench/rootfs payload 超过 boot 分区约束。

## Impact

- `Cargo.toml` — 新增或调整 Lichee fullbench feature，保持与 `qemu`、`lichee-d1-userbench` 分离；可用单一 `lichee-d1-fullbench` feature 配合编译期 mode 常量，也可拆成明确 feature，但必须映射到固定 log label。
- `kernel/Cargo.toml` — 暴露 fullbench 所需 kernel feature 组合。
- `Makefile` — 新增 fullbench image target，保留 `DWARF=n`、Android boot image 打包和 size inspect。
- `src/main.rs` — 作为 QEMU shell/script args 参考，不改变 QEMU 行为。
- `kernel/src/entry.rs` — 新增 Lichee fullbench 启动分支，复用 QEMU 的 process setup 语义。
- `kernel/src/mm/loader.rs` — 复用 eager ELF segment mapping 并提供 `load_user_app_eager_from_path()`；embedded loader 继续只用于 Q19B baseline，lazy file-backed `load_user_app()` 修复另列后续项。
- `kernel/src/pseudofs/mod.rs` — memory-root 需要可提供 `/bin/benchmark` 和可选 `/init.sh` 证据文本；`/bin/sh` 不作为当前要求。
- `kernel/src/file/mod.rs` — stdio 继续通过 `/dev/console` 绑定。
- `crates/axfs-ng/src/lib.rs` / D1 SDMMC/axdriver 相关代码 — 不再属于 Q19C 当前范围。
