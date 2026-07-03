## 1. Benchmark Evidence and Baseline Preservation — Q19B 不退化

- [ ] 1.1 记录当前 Q19B `lichee-d1-userbench` 的 feature 组合、make target、boot image 名称和串口成功 marker
- [ ] 1.2 确认 Q19B 仍使用 `load_embedded_user_app()`，Q19C fullbench 不复用该路径作为成功条件
- [ ] 1.3 保留 `make lichee-userbench` 或既有等价目标的行为和输出命名
- [ ] 1.4 建立 host regression：`lichee-d1`、`lichee-d1-userbench`、`qemu` feature 的 cargo check 不退化
- [ ] 1.5 建立 board regression：Q19B embedded benchmark 仍输出 TX throughput、TX latency、FIFO boundary、FIONBIO 和 exit code 0
- [ ] 1.6 梳理 QEMU 与 Q19B `benchmark.c` 当前参数差异：binary revision、payload sizes、iteration counts、drain policy、timer source、startup chain、root provider
- [ ] 1.7 在 `benchmark.c` 增加 manifest 输出：benchmark version、target mode、startup chain、root provider、timer source、TX sizes/iters/drain policy、latency iters、FIFO matrix sizes、RX mode
- [ ] 1.8 保持现有 TX baseline 不退化：sizes `{64,256,1024,4096}`、iters=100、每轮 `tcdrain()`、输出 `size`/`iters`/KB/s/line rate
- [ ] 1.9 保留并解释 64B 小包数据：`size=64 / iters=100 / 1.01 KB/s / 8.8% line rate`
- [ ] 1.10 增加或规划 fixed-payload RX witness：保留无输入 `EAGAIN` regression，新增 manual-input 或 loopback 模式的 N bytes read summary
- [ ] 1.11 规划 64B 小包延迟优化实验：baseline drain-per-iteration、no-drain enqueue、batch-N then drain、`writev` fragments、64/128/256B break-even
- [ ] 1.12 建立 M0 host witness：`git diff -- tests/benchmark.c kernel/resources/benchmark.elf`、OpenSpec validate、必要 cargo check 命令清单
- [ ] 1.13 建立 M0 board witness 模板：raw serial log、manifest、TX baseline、RX witness、64B small-packet section、exit code
- [ ] 1.14 Phase 3 前停止并等待用户确认，不修改 `tests/benchmark.c`、`kernel/resources/benchmark.elf` 或 loader/rootfs 代码

## 2. Part A / M1 — Memory-root path loader fullbench

- [ ] 2.1 新增独立 fullbench mode，串口日志打印 `lichee-memory-root-path`
- [ ] 2.2 设计 fullbench feature/target，使其与 `qemu`、`lichee-d1-userbench` 分离
- [ ] 2.3 扩展 D1 memory-root 初始化，创建 `/bin` 目录和 `/bin/benchmark` 文件节点
- [ ] 2.4 将 benchmark ELF 作为 memory-root 文件内容提供，而不是直接传给 embedded loader
- [ ] 2.5 调用 `pseudofs::mount_all()` 后，通过 `FS_CONTEXT.resolve("/bin/benchmark")` 解析 benchmark
- [ ] 2.6 使用 `load_user_app()` 创建用户地址空间，保持 `Process::new_init()`、`ASYNC_TTY.bind_to()`、`add_stdio()`、spawn/join 语义
- [ ] 2.7 增加缺失路径诊断：输出 root provider、requested path、resolve error
- [ ] 2.8 Gate M1 board: 串口日志证明 `/bin/benchmark` 通过 path loader 运行，并输出完整 benchmark sections 与 exit code 0

## 3. Part A / M2 — Shell/script benchmark parity

- [ ] 3.1 选择 shell 策略：静态 `/bin/sh` 优先；若不可用，定义覆盖 argv/envp/stdio/exit 的等价脚本入口
- [ ] 3.2 在 memory-root 中提供 `/bin/sh`、`/init.sh` 或等价入口所需文件
- [ ] 3.3 启动 args 对齐 QEMU 语义：优先 `["/bin/sh", "-c", "/init.sh"]`
- [ ] 3.4 验证 shell/interpreter 缺失时输出具体路径和错误阶段
- [ ] 3.5 验证 benchmark 从 shell/script 触发，而不是 kernel 直接替代执行
- [ ] 3.6 验证 stdin/stdout/stderr 仍通过 `/dev/console`，process exit/join 正常返回
- [ ] 3.7 Gate M2 board: 串口日志打印 `lichee-memory-root-shell`，显示 shell/script entry、benchmark sections、exit code 0

## 4. Part B / M3 — D1 SDMMC/block discovery

- [ ] 4.1 汇总 D1/Lichee RV Dock SDMMC 控制器 base、IRQ、clock/reset、pinmux、card detect 和 U-Boot 初始化事实
- [ ] 4.2 加入只读探针日志：MMIO 可访问性、关键寄存器、clock/reset 状态、card detect 状态
- [ ] 4.3 判断是否可继承 U-Boot 初始化；若不可继承，列出 StarryOS 需要完成的初始化序列
- [ ] 4.4 先实现或接入 PIO 单块读路径，证明能读取 LBA0 或已知 block
- [ ] 4.5 记录 IRQ claim/complete 行为；若先用 polling，日志明确标注 polling mode
- [ ] 4.6 评估 DMA/cache 要求；rootfs 首版允许 PIO-first，DMA 不作为首个通过条件
- [ ] 4.7 将可用设备注册为 `AxBlockDevice`，进入 axdriver/axfs-ng 设备容器
- [ ] 4.8 防止空 block list 调用 `axfs-ng::init_filesystems()`，无设备时输出 blocker summary
- [ ] 4.9 Gate M3 board: 有设备时能读块并进入 fs init；无设备时有完整探针日志且无 `No block device found!` panic

## 5. Part B / M4 — Real rootfs fullbench

- [ ] 5.1 准备 ext4 或 FAT rootfs 镜像，包含 `/bin/benchmark`、可选 `/bin/sh`、`/init.sh` 和必要动态依赖
- [ ] 5.2 记录 rootfs 镜像格式、分区/偏移、benchmark binary build source 和 shell 来源
- [ ] 5.3 在 Lichee rootfs mode 中打印 `lichee-rootfs-path`、block device 名称、filesystem type 和 mount result
- [ ] 5.4 从 rootfs 解析 `/bin/benchmark` 并通过 `load_user_app()` 运行
- [ ] 5.5 从 rootfs 解析 `/bin/sh -c /init.sh` 或等价脚本入口并运行 benchmark
- [ ] 5.6 验证 rootfs 缺失文件、坏 ELF、坏脚本时输出可定位错误
- [ ] 5.7 Gate M4 board: rootfs path benchmark 输出完整 benchmark sections 与 exit code 0
- [ ] 5.8 记录 raw serial log、benchmark summary、boot image size 和 rootfs 镜像信息

## 6. Evidence and Documentation

- [ ] 6.1 建立 Q19B embedded result 表：image、mode、startup chain、benchmark summary、raw log
- [ ] 6.2 建立 Q19C memory-root path result 表：image、mode、startup chain、benchmark summary、raw log
- [ ] 6.3 建立 Q19C memory-root shell/script result 表：image、mode、startup chain、benchmark summary、raw log
- [ ] 6.4 建立 Q19C SDMMC probe 表：controller facts、probe steps、block read result、blocker or success
- [ ] 6.5 建立 Q19C rootfs result 表：rootfs format、block device、startup chain、benchmark summary、raw log
- [ ] 6.6 建立 Q19C-M0 benchmark evidence 表：manifest fields、QEMU/Q19B 参数差异、RX witness mode、64B small-packet experiment matrix
- [ ] 6.7 更新 `.claude/analysis/q19c-lichee-full-starryos-benchmark.md` 或追加后续分析文档，记录最终方案和证据
- [ ] 6.8 更新 `openspec/specs/learned/spec.md`、`openspec/specs/architecture/spec.md`、`.claude/docs/tasks.md`
- [ ] 6.9 归档 OpenSpec change，并将 `lichee-d1-fullbench` capability 合入主 specs
