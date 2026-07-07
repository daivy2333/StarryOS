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

- [ ] 2.0a 在 `Cargo.toml` / `kernel/Cargo.toml` 新增 fullbench feature 组合：可用 `lichee-d1-fullbench` + compile-time mode，或显式 `lichee-d1-fullbench-mem`；必须继承 D1 async UART/PLIC、paging、task-ext、`dep:axfs`，且不得启用 `qemu`
- [ ] 2.0b 在 `Makefile` 新增 `lichee-fullbench-mem` 或等价 target，输出 `starry-lichee-fullbench-mem-boot.img`，保留 `DWARF=n` 和 Android boot image inspect
- [ ] 2.0c 在 `kernel/src/entry.rs` 新增 fullbench init 分支，串口日志打印 `lichee-memory-root-path`
- [ ] 2.0d 在 `kernel/src/pseudofs/mod.rs` 或等价位置选择 ELF 注入机制：优先 `include_bytes!("../resources/benchmark.elf")` + `FsContext::write()`
- [ ] 2.1 确认 benchmark ELF 格式：`readelf -l kernel/resources/benchmark.elf | grep INTERP` 输出为空；若有 `PT_INTERP`，先修复构建参数
- [ ] 2.2 fullbench-mem 实施后重新记录 boot image size；相对 1.5a baseline 的 delta 必须 < 0.5 MiB，否则精简 payload 或改方案
- [ ] 2.3 在 `mount_all()` 之前调用 `FS_CONTEXT.lock().create_dir("/bin", DIR_PERMISSION)` 创建 `/bin`
- [ ] 2.4 在 `mount_all()` 之前调用 `FS_CONTEXT.lock().write("/bin/benchmark", include_bytes!(...))` 写入 benchmark ELF；禁止直接调用 `MemoryNode::write_at()` / `append()`
- [ ] 2.5 注入后验证 `FS_CONTEXT.lock().resolve("/bin/benchmark")` 成功，再进入用户进程创建
- [ ] 2.6 调用 `pseudofs::mount_all()`，确认 `/dev`、`/dev/shm`、`/tmp`、`/proc`、`/sys` 正常挂载且未覆盖 `/bin`
- [ ] 2.7 使用 `load_user_app()` 创建用户地址空间，不使用 `load_embedded_user_app()` 作为 fullbench 成功路径
- [ ] 2.8 绑定 stdio：`Process::new_init()`、`ASYNC_TTY.bind_to()`、`add_stdio()` 语义与 Q19B/QEMU 对齐
- [ ] 2.9 spawn/join init process，并打印 exit code 与 benchmark section reached
- [ ] 2.10 增加缺失路径诊断：输出 root provider、requested path、resolve error
- [ ] 2.11 增加 loaded-process-before-first-section 诊断：若 `load_user_app()` 成功但进程未打印任何 benchmark section 就退出/abort，输出 exit status、stage reached，且不得记录 path-loader proof success
- [ ] 2.12 Gate M1 board: 串口日志必须出现 manifest、64/256/1024 TX throughput、TX latency、FIFO matrix 1/15/16/17/31/32/33/48/49、FIONBIO PASS、`benchmark exited with code: 0`，并证明 `/bin/benchmark` 通过 path loader 运行

## 3. Part A / M2 — Shell/script benchmark parity

- [ ] 3.1 选择 shell 策略：静态 `/bin/sh` 优先；若无已知良好的静态 shell，M2 标记 optional 并定义覆盖 argv/envp/stdio/exit/join 的等价脚本入口
- [ ] 3.2 在 memory-root 中提供 `/bin/sh`、`/init.sh` 或等价入口所需文件
- [ ] 3.2a 若使用 busybox/static shell，验证 `/proc/self/exe` 需求；若依赖该路径而当前 loader 未实现，必须降级为 documented equivalent entry
- [ ] 3.3 启动 args 对齐 QEMU 语义：优先 `["/bin/sh", "-c", "/init.sh"]`
- [ ] 3.4 验证 shell/interpreter 缺失时输出具体路径和错误阶段
- [ ] 3.5 验证 benchmark 从 shell/script 触发，而不是 kernel 直接替代执行
- [ ] 3.6 验证 stdin/stdout/stderr 仍通过 `/dev/console`，process exit/join 正常返回
- [ ] 3.7 Gate M2 board: 串口日志打印 `lichee-memory-root-shell`，显示 shell/script entry，并出现 manifest、64/256/1024 TX throughput、TX latency、FIFO matrix、FIONBIO PASS、`benchmark exited with code: 0`
- [ ] 3.8 若 M2 因 shell 来源或 `/proc/self/exe` blocker 未执行，记录 `SKIPPED: <blocker summary>`，不阻塞 M1 归档

## 4. Part B / M3 — D1 SDMMC/block probe-only discovery

- [ ] 4.1 汇总 D1/Lichee RV Dock SDMMC 控制器 base、IRQ、clock/reset、pinmux、card detect 和 U-Boot 初始化事实
- [ ] 4.2 加入只读探针日志：MMIO 可访问性、关键寄存器、clock/reset 状态、card detect 状态
- [ ] 4.3 判断是否可继承 U-Boot 初始化；若不可继承，列出 StarryOS 需要完成的初始化序列
- [ ] 4.4 记录 IRQ claim/complete 可行性；若只做 polling probe，日志明确标注 polling mode
- [ ] 4.5 评估 DMA/cache 要求，明确 Q19C 不以 DMA 或完整 SDMMC 驱动作为通过条件
- [ ] 4.6 Gate M3 board: 输出 SDMMC/block probe 表；若无可用 block device，记录 `SKIPPED: missing D1 SDMMC/block driver`，且无 `No block device found!` panic

## 5. Deferred / Conditional — Real rootfs fullbench

- [ ] 5.1 仅当 4.x 已证明有真实 block device 时，准备 ext4 或 FAT rootfs 镜像，包含 `/bin/benchmark`、可选 `/bin/sh`、`/init.sh` 和必要动态依赖
- [ ] 5.2 记录 rootfs 镜像格式、分区/偏移、benchmark binary build source 和 shell 来源
- [ ] 5.3 在 Lichee rootfs mode 中打印 `lichee-rootfs-path`、block device 名称、filesystem type 和 mount result
- [ ] 5.4 从 rootfs 解析 `/bin/benchmark` 并通过 `load_user_app()` 运行；若 block/rootfs 不可用，记录 `SKIPPED: <blocker summary>`
- [ ] 5.5 Gate future rootfs board: 若执行，串口日志必须出现 manifest、TX throughput、TX latency、FIFO matrix、FIONBIO PASS、`benchmark exited with code: 0`；若未执行，SKIPPED 不阻塞 Q19C M1 完成

## 6. Evidence and Documentation

- [ ] 6.1 建立 Q19B embedded result 表：image、mode、startup chain、benchmark summary、raw log
- [ ] 6.2 建立 Q19C memory-root path result 表：image、mode、startup chain、benchmark summary、raw log；如果 gate 未达成，输出 `SKIPPED: <blocker summary>`
- [ ] 6.3 建立 Q19C memory-root shell/script result 表：image、mode、startup chain、benchmark summary、raw log；如果 shell/equivalent blocker 存在，输出 `SKIPPED: <blocker summary>`
- [ ] 6.4 建立 Q19C SDMMC probe 表：controller facts、probe steps、blocker or success；如果未跑板，输出 `SKIPPED: <board access/blocker summary>`
- [ ] 6.5 建立 Q19C rootfs result 表：rootfs format、block device、startup chain、benchmark summary、raw log；默认允许 `SKIPPED: deferred after SDMMC/block driver`
- [ ] 6.6 建立 Q19C-M0 benchmark evidence 表：manifest fields、QEMU/Q19B 参数差异、RX witness mode、64B small-packet experiment matrix
- [ ] 6.7 更新 `.claude/analysis/q19c-lichee-full-starryos-benchmark.md` 或追加后续分析文档，记录最终方案和证据
- [x] 6.8 更新 `openspec/specs/learned/spec.md`、`openspec/specs/optimization/spec.md`、`.claude/docs/tasks.md`、`.claude/docs/SNAPSHOT.md`，保存 Q19C-M0 当前进度和 O77 剩余优化问题
- [ ] 6.9 归档 OpenSpec change，并将 `lichee-d1-fullbench` capability 合入主 specs
