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

Q19C 的目标是补齐这段差距：让 Lichee 先能运行一个完整的 StarryOS 用户态启动链路，进入 shell 或 shell 等价脚本入口，再从 shell/script 运行普通应用和 benchmark；随后再探索真实 SDMMC/block rootfs，把内存 root 过渡到真板 rootfs。

## What Changes

Q19C 分成两个工程部分。

### Part A: StarryOS 内部启动链路

这一部分不依赖真实 SD 卡/rootfs，重点是让 Lichee 在 memory-root 下具备接近 QEMU 的应用启动语义。

- 新增独立 Lichee fullbench runtime mode，与 Q19B `lichee-d1-userbench` 分离。
- 在 memory-root 中提供 `/bin/benchmark`，通过 VFS 路径解析启动 benchmark。
- M1 通过 `FS_CONTEXT.resolve()/read()` + eager ELF mapping 覆盖 memory-root 路径可见性、文件读取、argv/envp、stdio、process exit/join；`load_user_app()` 的 lazy file-backed COW 路径另作为 loader/mm 后续问题处理。
- 在 memory-root 中提供 `/bin/sh`、`/init.sh` 或明确等价的脚本入口，让 benchmark 可通过 shell/script 触发。

### Part B: 真板 rootfs / SDMMC 探针

这一部分依赖真实硬件采集，Q19C 内只做 probe-only 事实采集和 blocker 分类，不承诺在本 change 内完成 D1 SDMMC 驱动移植或真实 rootfs benchmark。完整 SDMMC/block/rootfs 实施应在后续独立 milestone 中展开。

- 采集 D1 SDMMC 控制器、时钟、reset、pinmux、IRQ、DMA/cache 相关状态。
- 判断 U-Boot 是否已初始化 SDMMC，以及 StarryOS 是否可以继承或必须重新初始化。
- 如已有可用 block device，则记录接入 `AxBlockDevice` / `axfs-ng::init_filesystems()` 的最小条件；如没有，则输出 `SKIPPED: <blocker summary>`，不得伪造 rootfs benchmark。
- 记录 QEMU、Q19B embedded、Q19C memory-root、Q19C shell/script 与 rootfs-probe/rootfs-skipped 五类证据，不混写。

## Capabilities

### New Capabilities

- `lichee-d1-fullbench`: 定义 Lichee D1 full StarryOS benchmark 的启动链路、rootfs 策略、shell/script 行为、真板采集要求和验证标准。

### Modified Capabilities

Q19C 不修改已归档的 Q19/Q19B capability。Q19B embedded userbench 继续作为独立可运行的真板回归路径。

## Scope

### In Scope

- 新增 Lichee fullbench feature/target/image 命名。
- memory-root 中的文件节点和应用布局。
- Lichee fullbench 使用 VFS-visible `/bin/benchmark` 启动；M1 当前采用 `FS_CONTEXT.resolve()/read()` + eager ELF mapping，后续 shell/rootfs 路径可继续推进 normal path loader parity。
- shell/script 触发 benchmark 的最小闭环。
- D1 SDMMC/rootfs 所需真板信息采集、probe-only blocker 分类与后续驱动接入计划。
- benchmark 输出和证据格式标准化。

### Out of Scope

- 把 D1 直接启用 `qemu` feature。
- 删除 Q19B embedded userbench。
- 在缺少真实 block device 时强行调用 `axfs-ng::init_filesystems()`。
- 将 memory-root 结果标记为真实 rootfs 结果。
- 把 VisionFive2 的 Q20 工作混入 Lichee Q19C。
- 在 Q19C 内承诺完成 D1 SDMMC 完整驱动移植、真实 block rootfs 挂载和 rootfs path benchmark。

## User-Visible Behavior

- `make lichee-userbench` 或既有等价目标继续产生 Q19B embedded benchmark image。
- 新增 fullbench 目标产生独立 boot image，串口日志必须打印当前 mode。
- memory-root path fullbench 日志必须包含 benchmark 是通过 VFS path 启动，而非 embedded loader。
- shell/script mode 日志必须包含 shell/script 入口和 benchmark 命令。
- rootfs/probe 日志必须包含 SDMMC/block probe 状态；只有真实 block device 已可用时才允许进入 rootfs path benchmark，否则必须记录 `SKIPPED` 和 blocker summary。

## BDD Gap Scan

### Happy Path

- Lichee memory-root fullbench 从 `/bin/benchmark` 启动 benchmark，输出与 Q19B 同类 benchmark sections。
- Lichee shell/script mode 在静态 shell 可用时启动 `/bin/sh -c /init.sh`；没有可靠 shell 时允许 documented equivalent command entry，但必须覆盖 argv/envp/stdio/exit/join。
- Lichee rootfs probe mode 采集 SDMMC/block facts；若真实 block device 尚不可用，则输出 SKIPPED blocker evidence。

### Sad Path

- benchmark path 不存在时输出明确错误，不静默 halt，不记录成功。
- shell/interpreter/shared library 缺失时输出缺失路径和启动阶段。
- 没有 block device 时不得触发 `No block device found!` panic。
- SDMMC probe 失败时输出阶段、寄存器/状态码和是否继承 U-Boot 状态；不得把 probe failure 记录成 rootfs benchmark failure。

### Edge

- Q19B embedded userbench 不能因 Q19C feature 新增而改变语义。
- memory-root mode 必须明确标记为 non-persistent root，不等同真实 rootfs。
- 如果 shell 是动态 ELF，dynamic linker 路径和依赖库必须由同一 rootfs 提供。
- Android boot image size 必须持续检查，避免 fullbench/rootfs payload 超过 boot 分区约束。

## Impact

- `Cargo.toml` — 新增或调整 Lichee fullbench feature，保持与 `qemu`、`lichee-d1-userbench` 分离；可用单一 `lichee-d1-fullbench` feature 配合编译期 mode 常量，也可拆成明确 feature，但必须映射到固定 log label。
- `kernel/Cargo.toml` — 暴露 fullbench 所需 kernel feature 组合。
- `Makefile` — 新增 fullbench image target，保留 `DWARF=n`、Android boot image 打包和 size inspect。
- `src/main.rs` — 作为 QEMU shell/script args 参考，不改变 QEMU 行为。
- `kernel/src/entry.rs` — 新增 Lichee fullbench 启动分支，复用 QEMU 的 process setup 语义。
- `kernel/src/mm/loader.rs` — 复用 eager ELF segment mapping 并提供 `load_user_app_eager_from_path()`；embedded loader 继续只用于 Q19B baseline，lazy file-backed `load_user_app()` 修复另列后续项。
- `kernel/src/pseudofs/mod.rs` — memory-root 需要可提供 `/bin/benchmark`、`/bin/sh`、`/init.sh` 等文件节点。
- `kernel/src/file/mod.rs` — stdio 继续通过 `/dev/console` 绑定。
- `crates/axfs-ng/src/lib.rs` — rootfs mode 只有在真实 block device 可用后调用。
- D1 SDMMC/axdriver 相关代码 — Q19C 只要求 probe-only 事实采集；完整 block/rootfs 接入作为后续 milestone 输入。
