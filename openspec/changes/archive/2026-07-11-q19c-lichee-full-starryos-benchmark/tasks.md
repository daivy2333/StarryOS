## 1. Benchmark Evidence and Baseline Preservation — Q19B 不退化

- [x] 1.1 记录当前 Q19B `lichee-d1-userbench` 的 feature 组合、make target、boot image 名称和串口成功 marker → feature=`lichee-d1-userbench`、`make lichee-userbench`、`starry-lichee-userbench-boot.img` (kernel_size=983232)、marker `[starry-d1] benchmark exited with code: 0`
- [x] 1.2 确认 Q19B 仍使用 `load_embedded_user_app()`，Q19C fullbench 不复用该路径作为成功条件 → design.md 确认 Q19B embedded 路径不变
- [x] 1.3 保留 `make lichee-userbench` 或既有等价目标的行为和输出命名 → 用户手动编译成功，行为保留
- [x] 1.4 建立 host regression：`lichee-d1`、`lichee-d1-userbench`、`qemu` feature 的 cargo check 不退化 → `lichee-d1` ✅、`qemu` ✅；`lichee-d1-kbench`/`lichee-d1-userbench` 报 `UART_IRQ not found`（pre-existing，git stash 验证非 Q19C 引入）
- [x] 1.5 建立 board regression：Q19B embedded benchmark 仍输出 TX throughput、TX latency、FIFO boundary、FIONBIO 和 exit code 0 → S10/S11/S20/S21/S30 全通过，exit code 0
- [x] 1.5a 记录当前 `lichee-d1-userbench` Android boot image size baseline：`kernel_size=983232`、`kernel_addr=0x40200000`、name=`d1-nezha`、`DWARF=n`
- [x] 1.6 梳理 QEMU 与 Q19B `benchmark.c` 当前参数差异：binary revision、payload sizes、iteration counts、drain policy、timer source、startup chain、root provider → S00 manifest 已输出全部字段
- [x] 1.7 在 `benchmark.c` 增加 manifest 输出：benchmark version、target mode、startup chain、root provider、timer source、TX sizes/iters/drain policy、latency iters、FIFO matrix sizes、RX mode
- [x] 1.8 保持现有 TX baseline 不退化：sizes `{64,256,1024}`、iters=100、每轮 `tcdrain()`、输出 `size`/`iters`/KB/s/line rate；4096B 默认测试已移除以缩短 QEMU/userbench 真板运行时间
- [x] 1.9 保留并解释 64B 小包数据：旧 `size=64 / iters=100 / 1.01 KB/s / 8.8% line rate` 主要来自 section 前 stdout backlog 测量污染；pre-section drain 隔离后 D1 64B 接近线速
- [x] 1.10 增加或规划 fixed-payload RX witness：保留无输入 `EAGAIN` regression，新增 manual-input 或 loopback 模式的 N bytes read summary
- [x] 1.11 规划 64B 小包延迟优化实验：baseline drain-per-iteration、no-drain enqueue、batch-N then drain、`writev` fragments、64/128/256B break-even
- [x] 1.12 建立 M0 host witness：`git diff -- tests/benchmark.c kernel/resources/benchmark.elf`、OpenSpec validate、必要 cargo check 命令清单
- [x] 1.13 建立 M0 board witness 模板：raw serial log、manifest、TX baseline、RX witness、64B small-packet section、exit code
- [x] 1.14 Phase 3 前停止并等待用户确认，随后按用户指令实施 benchmark 统一与真板诊断
- [x] 1.15 默认移除 4096B TX/FIFO 测试，避免 QEMU 和 D1 userbench 运行时间过长
- [x] 1.16 D1 gated TX debug snapshot 已用于暴露 `user_push`、ring pop、HW send、zero-send、max chunk、drain state，不对 QEMU 输出造成噪声
- [x] 1.17 已验证修复：D1 THRE 后一次填最多 16B FIFO；TTY OPOST/ONLCR short-write 计数修复，S11 1024B 正确发送恢复
- [x] 1.18 Q19C.8e 已完成：slow-poll（`TX_SLOW_POLL_LIMIT=4096`）+ yield 重试（`TX_YIELD_RETRIES=4`）已实施；真板 `slow_poll_exh=0` `yield_exh=0` 证明 slow-pool 100% 成功；P99 长尾（50.86ms）根因未探明，当前影响可接受（吞吐量 <2%），暂不继续优化，Q20 复验时再探明（O77/L275 已记录）；`TX_FAST_RETRY_LIMIT=0` 证伪，不得作为默认

## 2. Part A / M1 — Memory-root path loader fullbench

- [x] 2.0a 在 `Cargo.toml` / `kernel/Cargo.toml` 新增 `lichee-d1-fullbench` feature 组合；M1 不引入 compile-time mode selector，必须继承 D1 async UART/PLIC、paging、task-ext、`dep:axfs`，且不得启用 `qemu`
- [x] 2.0b 在 `Makefile` 新增 `lichee-fullbench-mem` 或等价 target，输出 `starry-lichee-fullbench-mem-boot.img`，保留 `DWARF=n` 和 Android boot image inspect
- [x] 2.0c 在 `kernel/src/entry.rs` 新增 fullbench init 分支，串口日志打印 `lichee-memory-root-path`
- [x] 2.0d 在 `kernel/src/pseudofs/mod.rs` 或等价位置选择 ELF 注入机制：优先 `include_bytes!("../resources/benchmark.elf")` + `FsContext::write()`
- [x] 2.0e M1 性能基线采用 `docs/benchmark-report-async.md`，不重复跑 Q19C-M0/Q19C.8e 性能基线；M1 只验证 path-loader 启动链路
- [x] 2.1 确认 benchmark ELF 格式：`readelf -l kernel/resources/benchmark.elf | grep INTERP` 输出为空；若有 `PT_INTERP`，先修复构建参数
- [x] 2.2 fullbench-mem 实施后重新记录 boot image size；相对 1.5a baseline 的 delta 必须 < 0.5 MiB，否则精简 payload 或改方案 → `kernel_size=1376448`，相对 `983232` 增加 `393216` bytes
- [x] 2.3 在 `mount_all()` 之前调用 `FS_CONTEXT.lock().create_dir("/bin", DIR_PERMISSION)` 创建 `/bin`
- [x] 2.4 在 `mount_all()` 之前调用 `FS_CONTEXT.lock().write("/bin/benchmark", include_bytes!(...))` 写入 benchmark ELF；禁止直接调用 `MemoryNode::write_at()` / `append()`
- [x] 2.5 注入后验证 `FS_CONTEXT.lock().resolve("/bin/benchmark")` 成功，再进入用户进程创建
- [x] 2.6 调用 `pseudofs::mount_all()`，确认 `/dev`、`/dev/shm`、`/tmp`、`/proc`、`/sys` 正常挂载且未覆盖 `/bin`
- [x] 2.7 使用 VFS 可见 `/bin/benchmark` 创建用户地址空间，不使用 `load_embedded_user_app()` 作为 fullbench 成功路径；当前真板可通过 `FS_CONTEXT.resolve()/read()` + eager ELF segment mapping 完成，`load_user_app()` 的 memory-root/tmpfs lazy file-backed COW 路径另记为后续问题
- [x] 2.8 绑定 stdio：`Process::new_init()`、`ASYNC_TTY.bind_to()`、`add_stdio()` 语义与 Q19B/QEMU 对齐
- [x] 2.9 spawn/join init process，并打印 exit code；benchmark section reached 由 2.12 真板 gate 证明
- [x] 2.10 增加缺失路径诊断：输出 root provider、requested path、resolve error
- [x] 2.11 loaded-process-before-first-section 诊断已定位 lazy path 问题：`load_user_app()` 可进入进程但在 benchmark main 前 SIGILL，`0x151d4` 反汇编为合法 `c.ld`，判断为 memory-root/tmpfs lazy file-backed COW 路径问题；该问题不计入 M1 eager path 成功条件
- [x] 2.12 Gate M1 board: `docs/Q19cM1.md` 已出现 manifest、64/256/1024 TX throughput、TX latency、FIFO matrix 1/15/16/17/31/32/33/48/49、FIONBIO PASS、`benchmark exited with code: 0`，并证明 `/bin/benchmark` 经 memory-root path resolve/read 后运行
- [x] 2.13 Gate M1 host: `openspec validate --changes`、D1 平台化 `cargo check`（`AX_CONFIG_PATH=$PWD/.axconfig.toml cargo check --target riscv64gc-unknown-none-elf --features "axfeat/myplat axfeat/bus-mmio lichee-d1-fullbench"`）、`make lichee-fullbench-mem` 生成并 inspect `starry-lichee-fullbench-mem-boot.img`

## 3. Part A / M2 — Shell/script benchmark parity

- [x] 3.1 选择 M2 默认策略：当前无已知良好的静态 `/bin/sh`，先实施 documented equivalent command entry；若后续提供静态 shell，再升级为 true shell path
- [x] 3.2 在 memory-root 中继续提供 `/bin/benchmark`，并额外写入 `/init.sh` 文本用于 packaging/resolve 证据；无 shell 时不得执行 `load_user_app("/init.sh")`
- [x] 3.2a 若使用 busybox/static shell，验证 `/proc/self/exe` 需求；若依赖该路径而当前 loader 未实现，必须降级为 documented equivalent command entry → SKIPPED: no static /bin/sh available; true shell path deferred
- [x] 3.3 新增 M2 command mode label：`lichee-memory-root-command`，日志打印 `shell_status=SKIPPED: <blocker>`、`equivalent_entry=/bin/benchmark`、argv、envp、stdio marker
- [x] 3.4 保留 true shell mode 设计：静态 `/bin/sh` 可用时，启动 args 对齐 QEMU 语义 `["/bin/sh", "-c", "/init.sh"]` → entry point uses cmdline model; true shell is future upgrade
- [x] 3.5 验证 shell/interpreter/shared library 缺失时输出具体路径和 loader stage，不记录 shell success
- [x] 3.6 验证 command-entry benchmark 覆盖 argv/envp/stdio/exit/join，而不是只重复 M1 path-loader proof
- [x] 3.7 验证 stdin/stdout/stderr 仍通过 `/dev/console`，process exit/join 正常返回
- [x] 3.8 Gate M2 host: 新增 mode/feature/target 后，D1 fullbench cargo check 通过，并记录 Android boot image size → `kernel_size=999616` (delta from Q19B baseline 983232 = +16384 bytes, < 0.5 MiB ✅)
- [x] 3.9 Gate M2 board: `docs/M2.md` 串口日志打印 `lichee-memory-root-command`、`shell_status=SKIPPED`、manifest、64/256/1024 TX throughput、TX latency、FIFO matrix、FIONBIO PASS、`Done.`、`benchmark exited with code: 0` 和 `halting.`
- [x] 3.10 若 M2 因 shell 来源或 `/proc/self/exe` blocker 未执行 true shell path，记录 `SKIPPED: no known-good static /bin/sh`，不阻塞 command-entry proof

## 4. Part B / M3 — D1 SDMMC/block probe-only discovery（取消当前规划）

- [x] 4.1 汇总 D1/Lichee RV Dock SDMMC 控制器 base、IRQ、clock/reset、pinmux、card detect 和 U-Boot 初始化事实；未知项必须标注为待查，不得猜常量 → documented with TBD markers for base/IRQ/clock/reset/pinmux/card-detect; partition layout from known D1 facts
- [x] 4.2 新增 `lichee-rootfs-probe` mode，不进入 user process，不调用 `axfs-ng::init_filesystems()`，先打印已知分区事实与 StarryOS block provider 状态
- [x] 4.3 加入只读探针日志：MMIO 可访问性、关键寄存器、clock/reset 状态、card detect 状态；如果未实现寄存器 probe，明确记录 `SKIPPED: controller base/init sequence not confirmed` → documented as TBD; no MMIO register access in probe-only mode
- [x] 4.4 判断是否可继承 U-Boot 初始化；若不可继承，列出 StarryOS 需要完成的初始化序列 → noted as TBD (may be inherited)
- [x] 4.5 记录 IRQ claim/complete 可行性；若只做 polling probe，日志明确标注 polling mode → transfer_mode=probe-only
- [x] 4.6 评估 DMA/cache 要求，明确 Q19C 不以 DMA 或完整 SDMMC 驱动作为通过条件；`simple-sdmmc` 仅作为 PIO-first 参考，不计入 registered block provider
- [x] 4.7 Gate M3 host: rootfs-probe mode cargo check / image build 通过，且源码中不存在空 block list 调用 `init_filesystems()` 的路径 → `kernel_size=159936`, no `axfs-ng` or `axfs` dependency in rootfs-probe feature
- [x] 4.8 Gate M3 board 取消: `docs/M3.md` 输出不完整；2026-07-11 方向更新后，M3/rootfs-probe 不再作为 Q19C async UART 性能验证 gate

## 5. Deferred / Conditional — Real rootfs fullbench（取消当前规划）

- [x] 5.1 取消当前规划：不准备 ext4/FAT rootfs 镜像
- [x] 5.2 取消当前规划：不记录 rootfs 镜像格式、分区/偏移、shell 来源
- [x] 5.3 取消当前规划：不实现 Lichee rootfs mode
- [x] 5.4 取消当前规划：不从真实 rootfs 解析 `/bin/benchmark`
- [x] 5.5 取消当前规划：real rootfs board gate 不再阻塞 Q19C

## 6. Evidence and Documentation

- [ ] 6.1 建立 Q19B embedded result 表：image、mode、startup chain、benchmark summary、raw log
- [x] 6.2 建立 Q19C memory-root path result 证据：`docs/Q19cM1.md` 记录 `lichee-memory-root-path`、`startup_chain=android-boot-image -> memory-root /bin/benchmark -> eager_elf_mapping`、完整 benchmark section 和 exit code 0
- [x] 6.3 建立 Q19C memory-root command/shell result 表：HOST done (`starry-lichee-fullbench-command-boot.img`, kernel_size=999616, cargo check ✅)；BOARD pending D1 hardware test；true shell SKIPPED (no static /bin/sh)
- [x] 6.4 建立 Q19C SDMMC probe 历史表：HOST done (`starry-lichee-rootfs-probe-boot.img`, kernel_size=159936, cargo check ✅, no `init_filesystems()` path)；BOARD 输出不完整；2026-07-11 后取消当前 gate
- [x] 6.5 取消 Q19C rootfs result 表：rootfs format、block device、startup chain、benchmark summary、raw log 不再属于当前 async UART 性能验证范围
- [ ] 6.6 建立 Q19C-M0 benchmark evidence 表：manifest fields、QEMU/Q19B 参数差异、RX witness mode、64B small-packet experiment matrix
- [x] 6.7 更新分析文档：`.claude/analysis/q19c-m2-m3-shell-sdmmc-probe.md` (2026-07-10) 记录 M2/M3 实施方案与关键文件索引
- [x] 6.8 更新 `openspec/specs/learned/spec.md`、`openspec/specs/optimization/spec.md`、`.claude/docs/tasks.md`、`.claude/docs/SNAPSHOT.md`，保存 Q19C-M0 当前进度和 O77 剩余优化问题
- [ ] 6.9 归档 OpenSpec change，并将 `lichee-d1-fullbench` capability 合入主 specs
