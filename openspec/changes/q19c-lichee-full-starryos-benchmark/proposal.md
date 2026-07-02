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
- 复用 `load_user_app()`，覆盖 `FS_CONTEXT.resolve()`、`CachedFile`、ELF path loader、argv/envp、stdio、process exit/join。
- 在 memory-root 中提供 `/bin/sh`、`/init.sh` 或明确等价的脚本入口，让 benchmark 可通过 shell/script 触发。
- 保留 Q19B embedded userbench 作为 regression baseline，用于判断 async UART/TTY/syscall 是否退化。

### Part B: 真板 rootfs / SDMMC 探索

这一部分依赖真实硬件采集，重点是把 StarryOS 的 rootfs 从 memory-root 推进到 Lichee 的实际块设备。

- 采集 D1 SDMMC 控制器、时钟、reset、pinmux、IRQ、DMA/cache 相关状态。
- 判断 U-Boot 是否已初始化 SDMMC，以及 StarryOS 是否可以继承或必须重新初始化。
- 将 D1 SDMMC 或等价块设备接入 `AxBlockDevice` / axdriver 设备容器。
- 在具备真实 block device 后调用 `axfs-ng::init_filesystems()`，挂载 ext4/fat rootfs。
- 从真实 rootfs 解析 `/bin/sh`、`/init.sh`、`/bin/benchmark` 并运行 benchmark。
- 记录 QEMU、Q19B embedded、Q19C memory-root、Q19C shell/script、Q19C rootfs 五类数据，不混写。

## Capabilities

### New Capabilities

- `lichee-d1-fullbench`: 定义 Lichee D1 full StarryOS benchmark 的启动链路、rootfs 策略、shell/script 行为、真板采集要求和验证标准。

### Modified Capabilities

Q19C 不修改已归档的 Q19/Q19B capability。Q19B embedded userbench 继续作为独立可运行的真板回归路径。

## Scope

### In Scope

- 新增 Lichee fullbench feature/target/image 命名。
- memory-root 中的文件节点和应用布局。
- Lichee fullbench 使用 `load_user_app()` 的 path-based 启动。
- shell/script 触发 benchmark 的最小闭环。
- D1 SDMMC/rootfs 所需真板信息采集与驱动接入计划。
- benchmark 输出和证据格式标准化。

### Out of Scope

- 把 D1 直接启用 `qemu` feature。
- 删除 Q19B embedded userbench。
- 在缺少真实 block device 时强行调用 `axfs-ng::init_filesystems()`。
- 将 memory-root 结果标记为真实 rootfs 结果。
- 把 VisionFive2 的 Q20 工作混入 Lichee Q19C。

## User-Visible Behavior

- `make lichee-userbench` 或既有等价目标继续产生 Q19B embedded benchmark image。
- 新增 fullbench 目标产生独立 boot image，串口日志必须打印当前 mode。
- memory-root path fullbench 日志必须包含 benchmark 是通过 VFS path 启动，而非 embedded loader。
- shell/script mode 日志必须包含 shell/script 入口和 benchmark 命令。
- rootfs mode 日志必须包含 block device、filesystem type、rootfs mount 状态和 benchmark 路径。

## BDD Gap Scan

### Happy Path

- Lichee memory-root fullbench 从 `/bin/benchmark` 启动 benchmark，输出与 Q19B 同类 benchmark sections。
- Lichee shell/script mode 启动 `/bin/sh -c /init.sh` 或等价入口，并从脚本运行 benchmark。
- Lichee rootfs mode 枚举真实 block device，挂载 rootfs，从 rootfs 路径运行 shell/benchmark。

### Sad Path

- benchmark path 不存在时输出明确错误，不静默 halt，不记录成功。
- shell/interpreter/shared library 缺失时输出缺失路径和启动阶段。
- 没有 block device 时不得触发 `No block device found!` panic。
- SDMMC 初始化失败时输出阶段、寄存器/状态码和是否继承 U-Boot 状态。

### Edge

- Q19B embedded userbench 不能因 Q19C feature 新增而改变语义。
- memory-root mode 必须明确标记为 non-persistent root，不等同真实 rootfs。
- 如果 shell 是动态 ELF，dynamic linker 路径和依赖库必须由同一 rootfs 提供。
- Android boot image size 必须持续检查，避免 fullbench/rootfs payload 超过 boot 分区约束。

## Impact

- `Cargo.toml` — 新增或调整 Lichee fullbench feature，保持与 `qemu`、`lichee-d1-userbench` 分离。
- `kernel/Cargo.toml` — 暴露 fullbench 所需 kernel feature 组合。
- `Makefile` — 新增 fullbench image target，保留 `DWARF=n`、Android boot image 打包和 size inspect。
- `src/main.rs` — 作为 QEMU shell/script args 参考，不改变 QEMU 行为。
- `kernel/src/entry.rs` — 新增 Lichee fullbench 启动分支，复用 QEMU 的 process setup 语义。
- `kernel/src/mm/loader.rs` — 复用 `load_user_app()`；embedded loader 继续只用于 Q19B baseline。
- `kernel/src/pseudofs/mod.rs` — memory-root 需要可提供 `/bin/benchmark`、`/bin/sh`、`/init.sh` 等文件节点。
- `kernel/src/file/mod.rs` — stdio 继续通过 `/dev/console` 绑定。
- `crates/axfs-ng/src/lib.rs` — rootfs mode 只有在真实 block device 可用后调用。
- D1 SDMMC/axdriver 相关代码 — Part B 根据真板采集结果确定最小接入点。
