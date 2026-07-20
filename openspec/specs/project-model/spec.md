# Spec: project-model — 项目模型与跨模块约束

## Purpose

记录当前有效的跨模块模型、边界和约束。条目使用 `Mxx` 编号，不记录历史选择过程。对应 Legacy: `openspec/specs/architecture/spec.md` (hash: `5b054d98`) 中的当前有效约束。

## Requirements

### Requirement: M01 — 异步运行时选型

异步串口运行时 MUST 基于 `axtask::future`（`block_on` + `poll_io` + `register_irq_waker`），并 MUST 引入 `embassy-sync::AtomicWaker` 用于 ISR 中安全唤醒 Waker，禁止引入完整 Embassy 或 `embassy-executor`。

**Legacy**: ADR-001 (A001), 2026-05-24 | **状态**: ✅ 已落地

#### Scenario: 实现新的 UART 异步原语

- **WHEN** 开发者实现新的 `Future` 或 `Pollable` 用于 UART I/O
- **THEN** 必须基于 `axtask::future::poll_fn` + `embassy_sync::AtomicWaker` 模式编写，禁止引入 `embassy-executor`

### Requirement: M02 — VFS 接口规范

UART 设备 MUST 通过 `DeviceOps` trait + `Device` wrapper 注册到 `/dev`，禁止直接 impl `FileLike`。

**Legacy**: ADR-003 (A003), 2026-05-24 | **状态**: ✅ 已落地

#### Scenario: 注册新串口设备节点

- **WHEN** 开发者要把新串口实例暴露到用户空间
- **THEN** 必须实现 `DeviceOps`（含 `read_at` / `write_at` / `ioctl` / `as_pollable` / `flags`），通过 `Device::new` 包装后挂入 devfs builder

### Requirement: M03 — 缓冲与并发策略

每个方向（RX / TX）MUST 使用各自独立的 ring buffer，硬件 FIFO 与内核 ring buffer 之间的搬运 MUST 由单一后台 copier 任务完成。当前实现使用 `atomic_ring_buffer`，TX ring 保持 SPSC，RX ring 保持 SPSC 且 unsafe 唯一 consumer。

**Legacy**: ADR-004 (A004), ADR-061 (A061), ADR-062 (A062) | **状态**: ✅ 已落地，TX/RX 契约由 Q28/Q29 收敛

#### Scenario: 设计新的数据通路

- **WHEN** 开发者实现 UART 与用户空间之间新的数据搬运路径
- **THEN** 必须确保硬件 FIFO 的读取/写入由单一 copier 任务完成，禁止在 ISR 中直接操作 ring buffer，禁止多个任务并发 drain 同一 FIFO

### Requirement: M04 — termios 支持

UART 默认 MUST 运行在 raw 模式，termios 行规则 MUST 作为可选功能通过 ioctl 动态启用。默认路径零开销。

**Legacy**: ADR-005 (A005), 2026-05-24 | **状态**: ✅ 已通过 ldisc 集成

#### Scenario: 通过 ioctl 切换 termios

- **WHEN** 用户态程序对 `/dev/console` 调用 `ioctl(TCSETS, ...)` 设置终端属性
- **THEN** 必须保持 raw 数据通路零开销，行规则只在启用时介入

### Requirement: M05 — 硬件抽象层

异步串口 MUST 使用 `AsyncUartDriver<R, W, P>` + `UartPort` trait + `OsRuntime` + `OsWakerSet` 构成最小可移植接口（2-trait 架构）。初期支持 NS16550（QEMU byte-MMIO）和 DW APB UART（D1 32-bit MMIO）。

**Legacy**: ADR-006 (A006), ADR-033 (A033), ADR-036 (A036) | **状态**: ✅ 已落地

#### Scenario: 适配新的 UART 硬件型号

- **WHEN** 项目需要支持非 16550 的 UART 控制器
- **THEN** 必须实现 `UartPort` trait 的新后端，复用 ISR → AtomicWaker → copier 任务的上层架构

### Requirement: M06 — DMA 策略

DMA 探索 MUST 归入远期（Q25 决策阶段），当前 M0~Q29 MUST 全程基于中断驱动 + NAPI 批量轮询优化。

**Legacy**: ADR-012 (A012), 2026-05-25 | **状态**: ✅ 设计保留

#### Scenario: 性能优化路径选择

- **WHEN** 开发者考虑提升吞吐量
- **THEN** 必须优先尝试 NAPI / 批量 I/O / IER 缓存等中断驱动优化，DMA 推迟到 Q25 真板阶段

### Requirement: M07 — 内核日志同步阻塞约束

内核启动日志的同步阻塞开销 MUST 接受为既定约束（`ax_println!` 依赖外部 crate 的 `axhal::console::write_bytes`，不可修改）。用户态 Console 输出 MUST 可异步化。

**Legacy**: ADR-013 (A013), 2026-05-27 | **状态**: ✅ 关键约束

#### Scenario: 修改内核日志路径

- **WHEN** 开发者想改 `ax_println!` 走异步路径
- **THEN** 不可行 — 该路径依赖外部 crate；必须保留 Console polling TX 作为内核日志通道

### Requirement: M08 — MMIO 权限

UART MMIO `0x10000000` MUST 视为已通过 `axplat::mem::mmio_ranges` 在最终内核页表中正确映射（`READ | WRITE | DEVICE`），不需要修改 axplat。`axmm::iomap()` 可作为安全保障。

**Legacy**: ADR-024 (A024), 2026-05-31 | **状态**: ✅ 已验证

#### Scenario: 访问设备 MMIO

- **WHEN** 开发者需要操作非标准设备 MMIO 区域
- **THEN** 可以直接使用经过 `axplat` 注册的虚拟地址，或调用 `axmm::iomap(PhysAddr, size)` 作为冗余安全网

### Requirement: M09 — NS16550 stride 约束

NS16550 寄存器空间仅 8 字节，`UART_STRIDE` MUST 配置为 1。任何 stride 超过 1 的实现 MUST 视为 bug。

**Legacy**: ADR-026 (A026), 2026-05-31 | **状态**: ✅ 已落地

#### Scenario: 配置新的 UART 实例

- **WHEN** 开发者初始化 `Uart16550<MmioBackend>::new_mmio(NonNull<u8>, stride)`
- **THEN** stride 参数必须传 1（NS16550 寄存器物理布局），禁止传 4 或其他值

### Requirement: M10 — Console 与 Async 共存

Console 与 Async 串口 MUST 共存。Console 负责内核日志 / 早期启动 / panic 处理；Async 负责 Shell I/O / 用户态程序 / 高性能数据传输。两者共享 UART THR 互不冲突。

**Legacy**: ADR-030 (A030), 2026-06-01 | **状态**: ✅ 已落地

#### Scenario: 增加新的输出路径

- **WHEN** 开发者考虑添加新的日志或数据输出通道
- **THEN** 必须先判断属于"内核态可靠日志"还是"用户态高性能数据"，前者走 Console，后者走 Async

### Requirement: M11 — RX 性能测试方法

Async RX 性能测试 MUST 在内核态直接测试 Ring Buffer，绕过 TTY。Console RX MUST 跳过测试（无 Ring Buffer 且非阻塞）。用户态 RX MUST 跳过（TTY 回显竞争）。

**Legacy**: ADR-031 (A031), 2026-06-01 | **状态**: ✅ 已落地

#### Scenario: 设计新的 I/O 性能测试

- **WHEN** 开发者要测量 I/O 性能
- **THEN** 必须先判断测试对象是"硬件吞吐 / 软件路径 / 端到端延迟"，再选择对应位置

### Requirement: M12 — uart_16550 crate 异步栈

uart_16550 MUST 通过 `async` feature gate 提供完整异步 UART 栈（ISR handler, ring buffer, copier driver, device_ops），通过 `OsRuntime` + `OsWakerSet` 两个 OS 抽象 trait 实现跨平台可移植性。

**Legacy**: ADR-033 (A033), ADR-036 (A036) | **状态**: ✅ 已落地

#### Scenario: 新 OS 集成异步 UART

- **WHEN** 开发者要在新 OS 项目中使用异步 UART
- **THEN** 必须实现 `OsRuntime` + `OsWakerSet` 两个 trait 并启用 `async` feature gate

### Requirement: M13 — LTO 延期启用

LTO MUST 在最终发布前重新启用。活跃开发期编译速度优先，`lto = true` 暂不开。

**Legacy**: ADR-034 (A034), 2026-06-16 | **状态**: ✅ 已记录

#### Scenario: 发布构建准备

- **WHEN** 项目进入开发冻结期
- **THEN** LTO MUST 在发布构建前重新启用，并验证 ring buffer 吞吐量回归基线

### Requirement: M14 — OS 抽象最小接口

OS abstraction layer MUST 只保留驱动代码实际调用的 trait。当前为 `OsRuntime` + `OsWakerSet` 二 trait 最小可移植接口。禁止保留未被 import 或调用的 trait。

**Legacy**: ADR-036 (A036), 2026-06-19 | **状态**: ✅ 已落地

#### Scenario: 检测到死 trait

- **WHEN** `cargo build` 报告 OS abstraction 类型的 dead_code warning
- **THEN** 未使用的 trait MUST 从 OS abstraction 层删除，对应的 adapter impl SHALL 删除

### Requirement: M15 — TxCompletion 四阶段 drain

`flush()` 实现 MUST 等待四阶段排空：ring empty → copier inactive → staged bytes zero → transmitter empty。`tcdrain` SHALL 使用 `driver().tx_completion()` 替代直接 MMIO 访问。

**Legacy**: ADR-037 (A037), 2026-06-23 | **状态**: ✅ 已落地

#### Scenario: Flush 等待全部 drain 阶段

- **WHEN** caller 对异步 UART writer 调用 `flush()`
- **THEN** 实现 MUST poll 直到四阶段（ring, copier, FIFO, shift register）全部排空
- **AND** TEMT corner-case 唤醒窗口 SHALL 通过非 polling 方式处理

### Requirement: M16 — TtyWrite 短写契约

`TtyWrite::write` MUST 返回实际接受的字节数（`usize`），使 VFS caller 可观察 short write。`Tty::write_at()` MUST 传播实际接受数到 `sys_write`。

**Legacy**: ADR-038 (A038), 2026-06-23 | **状态**: ✅ 已实施

#### Scenario: TX ring 无法接受完整 buffer

- **WHEN** TTY writer 接受的字节数少于用户 buffer 长度
- **THEN** `TtyWrite::write` MUST 返回实际接受字节数
- **AND** `Tty::write_at` MUST 将该计数传播到 VFS/sys_write caller

### Requirement: M17 — 增量融合策略

合并多个 async-uart 优化 commit 时 MUST 采用"增量融合"策略：按依赖关系排序 → 摘取原子 commit → cargo check → QEMU benchmark → 无退化才继续。禁止一次性 apply 多个优化 commit。

**Legacy**: ADR-039 (A039), 2026-06-21 | **状态**: ✅ 已验证有效

#### Scenario: 未来 async-uart 优化合并

- **WHEN** 开发者需要合并其他分支的 async-uart 优化 commit
- **THEN** MUST 遵循增量融合策略，每步 Gate 通过后才继续，保留源分支作为参考

### Requirement: M18 — 平台描述符集中表达

平台事实（UART kind、base、irq、stride、MMIO width、early console strategy、boot image strategy）MUST 由 build-time platform descriptor 集中表达。`uart_init.rs` 只消费 descriptor，不再追加板级常量。

**Legacy**: ADR-044 (A044), 2026-06-28 | **状态**: ✅ 已落地

#### Scenario: 板级常量集中化

- **WHEN** driver 需要板级特定的 base、IRQ、stride、width 或 boot image 参数
- **THEN** MUST 从 platform descriptor 或等价集中平台模块读取
- **AND** MUST NOT 在驱动初始化代码中引入板级常量

### Requirement: M19 — D1 axplat 启动层

Lichee 构建 MUST 通过 `MYPLAT` / `PLAT_CONFIG` 选择本地 `axplat-riscv64-lichee-d1`，由其负责 `_start`、早期页表、MMU、内存布局、D1 polling console 和平台初始化。禁止继续走 QEMU virt axplat。

**Legacy**: ADR-045 (A045), 2026-06-28 | **状态**: ✅ 已落地

#### Scenario: D1 boot 路径验证

- **WHEN** StarryOS 构建 Lichee RV Dock boot image
- **THEN** host inspection MUST 证明引用 `axplat_riscv64_lichee_d1`
- **AND** 若 linker base、ELF entry 或 Android boot image address 不匹配 D1 合约，禁止烧板

### Requirement: M20 — D1/C906 页表 PTE flags

D1 axplat 的 DDR identity/high-half mapping MUST 使用 T-Head normal-memory PTE flags：`PTE_DDR = 0xef | (1 << 60) | (1 << 61) | (1 << 62)`。低地址/MMIO 不套用 normal-memory 属性。

**Legacy**: ADR-046 (A046), 2026-06-28 | **状态**: ✅ 已落地

#### Scenario: D1/C906 AMO fault 诊断

- **WHEN** Lichee RV Dock 在 `Starting kernel ...` 后报 `Store/AMO access fault`
- **THEN** EPC/TVAL MUST 先符号化
- **AND** 早期和最终页表 MUST 检查 T-Head C9xx normal-memory PTE flags

### Requirement: M21 — Q19B 先嵌入 benchmark payload

Q19B MUST 拆成 smoke、kernel benchmark、user benchmark 三个阶段。先做 D1-safe async UART（stride 4 / 32-bit MMIO）和 PLIC IRQ 18，再经 `/dev/console` 跑 embedded benchmark ELF。SDMMC/rootfs parity 后置。

**Legacy**: ADR-047 (A047), 2026-06-29 | **状态**: ✅ 已完成

#### Scenario: Q19B 首个 benchmark 数据集

- **WHEN** Q19B 采集首个 D1 async UART benchmark 数据
- **THEN** MUST 使用 D1 async UART、PLIC IRQ、`/dev/console` 和 user benchmark payload
- **AND** embedded benchmark ELF SHOULD 在 SDMMC/rootfs 可用前先行使用

### Requirement: M22 — D1 平台专用 UartPort

D1 async UART MUST 使用 `ArceOsD1UartPort` 与 `d1_uart_isr_handler`，以 stride-aware 32-bit volatile MMIO 访问 DW APB UART 寄存器。QEMU `Uart16550<MmioBackend>` U8 路径保持不变。两条路径用 feature gate 互斥。

**Legacy**: ADR-048 (A048), 2026-06-29 | **状态**: ✅ 已落地

#### Scenario: D1 async UART 寄存器访问

- **WHEN** Lichee D1 benchmark mode 初始化 async UART
- **THEN** MUST 使用 D1-specific `ArceOsD1UartPort`
- **AND** MUST 以 stride-aware 32-bit volatile MMIO 访问 DW APB UART
- **AND** MUST NOT 将 D1 路由到 QEMU byte-MMIO 路径

### Requirement: M23 — D1 userbench 最小 runtime

`lichee-d1-userbench` MUST 启用 `dep:axfs` 和 `axfeat/task-ext`，恢复 pseudofs、ASYNC_TTY、FD_TABLE、用户任务与 syscall。不启用 QEMU `qemu` feature。本地 patch `axfs-ng` 启用 `axdriver/block` + `axdriver/bus-mmio`。

**Legacy**: ADR-049 (A049), 2026-06-29 | **状态**: ✅ 已落地

#### Scenario: D1 userbench 到达 devfs

- **WHEN** Q19B 从 kbench 推进到 userbench
- **THEN** MUST 包含 `axfs` 和 `task-ext`，不含 QEMU PCI/virtio/display 假设
- **AND** `make lichee-userbench` MUST 在烧板前通过

### Requirement: M24 — D1 feature 能力与模式分离

D1 async UART/PLIC 硬件能力 feature 与 smoke/kbench/userbench 运行模式 feature MUST 分离。`lichee-d1-userbench` 可复用硬件能力，但不能继承会排除用户态 runtime 模块的 kbench-only feature。

**Legacy**: ADR-050 (A050), 2026-06-29 | **状态**: ✅ 已落地

#### Scenario: D1 userbench feature 选择

- **WHEN** `lichee-d1-userbench` 启用
- **THEN** MUST 包含 D1 async UART 和 PLIC 能力
- **AND** MUST 保持 user/process/filesystem 模块可见
- **AND** MUST NOT 继承会排除 benchmark runtime 模块的 kbench-only feature

### Requirement: M25 — D1 THRE 边沿丢失兼容

D1 backend 启用 `IER::THR_EMPTY` 后若 LSR 已 THRE/TEMT，MUST 立即 wake TX_WAKER / DRAIN_WAKER。ISR 对 IIR bit0=1 no-pending 不能当有效中断，但可基于 LSR 补 wake。`flush()` / `TCSBRK` 注册 DRAIN_WAKER。

**Legacy**: ADR-051 (A051), 2026-06-29 | **状态**: ✅ 已落地

#### Scenario: 真板 UART backend 启用 THRE 中断

- **WHEN** 真实 UART backend 启用 THRE 中断
- **THEN** MUST 同时检查当前 LSR readiness，若 THRE/TEMT 已就绪则 wake TX/drain waiter
- **AND** `tcdrain` / `flush` waiter MUST 注册在 drain completion 路径上

### Requirement: M26 — Q19C memory-root path 收敛

Q19C fullbench MUST 先在 D1 memory root 提供 `/bin/benchmark`，通过 `FS_CONTEXT.resolve()/read()` + eager ELF segment mapping 启动。M2 以 `lichee-memory-root-command` 收尾。shell、SDMMC、block、真实 rootfs 取消当前规划。

**Legacy**: ADR-052 (A052), 2026-07-02 | **状态**: ✅ 已完成

#### Scenario: D1 fullbench path loading

- **WHEN** Q19C 在 Lichee RV Dock 启动完整 StarryOS benchmark
- **THEN** 首个 fullbench gate MUST 从 VFS-visible `/bin/benchmark` 运行 benchmark
- **AND** shell、SDMMC、block、真实 rootfs parity MUST NOT 作为 Q19C 完成条件

### Requirement: M27 — D1 P99 长尾不阻塞主线

D1 size>=15 / drain-each P99 长尾 MUST NOT 作为 Q19C gate。归类为 D1 平台尾部行为或适配层调优项。若出现 hang、数据丢失、明显交互退化或吞吐显著下降，才提升为平台 tracing。

**Legacy**: ADR-053 (A053), 2026-07-08 | **状态**: ✅ 已接受

#### Scenario: D1 P99 tail 在 Q19C 期间出现

- **WHEN** D1 benchmark 显示 size>=15 TX P99 tail 但吞吐和正确性 gate 通过
- **THEN** Q19C MUST 将其记为 known platform limitation
- **AND** 仅在导致 hang、数据丢失、可见交互退化或在其他支持板卡复现时才成为 gate

### Requirement: M28 — Q19C-M1 FS API 注入 benchmark

M1 MUST 在 `init_memory_root()` 后用 `FsContext::create_dir()` / `write()` 注入 `/bin/benchmark`，用 `resolve()` 验证路径，再通过 eager ELF segment mapping 启动。不新增 tmpfs 专用写接口。

**Legacy**: ADR-054 (A054), 2026-07-08 | **状态**: ✅ 已落地

#### Scenario: M1 memory-root benchmark 注入

- **WHEN** Q19C-M1 准备 D1 fullbench image
- **THEN** MUST 通过 `FsContext` 创建和写入 `/bin/benchmark`
- **AND** MUST 在 spawn 前证明 `FS_CONTEXT.resolve("/bin/benchmark")` 成功

### Requirement: M29 — Q19C-M2 command-entry 收尾

M2 MUST 接受 documented equivalent command entry（记录 label、argv/envp、stdio、spawn/join、exit code），MUST NOT 伪称 shell success。M3/rootfs-probe、shell、SDMMC、block、real rootfs 不再是 Q19C gate。

**Legacy**: ADR-055 (A055), 2026-07-10 | **状态**: ✅ 已接受

#### Scenario: M2 缺少 shell

- **WHEN** Q19C-M2 缺少已知可用的静态 shell
- **THEN** MUST 将 shell/script proof 记为 `SKIPPED` 或 equivalent-command proof
- **AND** MUST NOT 声称 shell-launched benchmark success

### Requirement: M30 — Q20 只收敛 benchmark 证据

Q20 代码改动 MUST 限定在 benchmark、诊断输出、构建宏和证据归档路径。若需要修改 `tx_copier_loop()`、waker、IER 或 drain 语义，MUST 退出 Q20 并另开 change。Q20 输出可作为优化决策输入，但不能声明 SMP 正确性。

**Legacy**: ADR-057 (A057), 2026-07-12 | **状态**: ✅ 已完成

#### Scenario: 实施 Q20 benchmark closure

- **WHEN** Q20 实现改动 benchmark 输出
- **THEN** MUST 保持现有 write/read/tcdrain 语义
- **AND** MUST 将 QEMU 和 D1 证据分离开
- **AND** MUST NOT 在无 Q24 multi-hart stress 的情况下声称 SMP 正确性

### Requirement: M31 — Q21/Q22 user ring/completion 取消

当前规划 MUST 不实施 Q21 UART user completion queue MVP 或 Q22 `mmap` user ring / zero-copy prototype。保留现有 TX ring + copier + `TxCompletion` + `write()` / `writev()` / `tcdrain()` 路径。

**Legacy**: ADR-058 (A058), 2026-07-13 | **状态**: ✅ 决策完成

#### Scenario: 重新考虑 user ring 或 completion queue

- **WHEN** 未来提议重新引入 UART completion queue、user ring、zero-copy TX/RX 或 request-level completion
- **THEN** MUST 引用新证据证明当前 `write()` / `writev()` / batch / `tcdrain()` 路径不足
- **AND** MUST 保留 `/dev/console` read/write fallback

### Requirement: M32 — lint 与测试 Gate 分层

后续 clippy/test 清理 proposal MUST 按 artifact、feature、target 和平台配置分层。可复用 crate 用 host check/test/clippy；kernel 用目标架构 + feature compile gate；IRQ/TTY/rootfs 行为用 QEMU/真板 gate。

**Legacy**: ADR-059 (A059), 2026-07-13 | **状态**: 候选

#### Scenario: 定义 clippy 和测试 gate

- **WHEN** 后续 change 清理 StarryOS 或 `uart_16550` 的 warning、clippy 和 tests
- **THEN** MUST 为可复用 crate、kernel target build 和系统 runtime 定义分离的 gate

### Requirement: M33 — io_uring 同构点映射

后续 io_uring-inspired UART proposal MUST 先回答"借鉴哪个 io_uring 思想"以及"为什么当前实现不足"。高价值借鉴方向：backpressure、writer/SPSC 隐患、`TxCompletion` 全局 drain。user ring/CQ/zero-copy 当前不实施。

**Legacy**: ADR-060 (A060), 2026-07-14 | **状态**: 候选

#### Scenario: 为 async UART 复用 io_uring 思想

- **WHEN** 未来 async UART proposal 以 io_uring 为动机
- **THEN** MUST 声明复用哪个 io_uring 思想以及为什么当前 UART 路径不足
- **AND** MUST NOT 在未满足 ADR-058 和 O82 证据 gate 的情况下重新引入 user ring、CQE 或 zero-copy

### Requirement: M34 — UART backpressure 与 writer 并发分阶段

UART TX 优化 SHOULD 先做阻塞式 backpressure / writable wait MVP（Q27），再收敛 `AsyncUartWriter::Clone` 与 `RingBufTx` SPSC 安全契约（Q28）。MPSC ring MUST 等实际多 writer 数据或 Q24 SMP stress 证据后再设计。

**Legacy**: ADR-061 (A061), 2026-07-14 | **状态**: ✅ Q27/Q28 已完成

#### Scenario: Q19 后排期 UART backpressure 和 writer contract

- **WHEN** R19 之后计划 UART TX 正确性或 writer 并发工作
- **THEN** plan MUST 将 backpressure 排为 Q27，writer contract convergence 排为 Q28
- **AND** MUST 保持 MPSC ring 为 O85，除非新 SMP 或 workload 证据证明 producer-side serialization 不足

### Requirement: M35 — TX/RX 并发契约分流

Q28 后 UART 并发工作 MUST 将 multi-hart correctness（Q24）、TX scheduling semantics、queue producer model（Q30）和 RX consumer safety（Q29）分开评估，禁止合并为单一 MPSC 方案。

**Legacy**: ADR-062 (A062), 2026-07-18 | **状态**: Q29 ✅ / Q24 ⏳ / Q30 🧊

#### Scenario: 规划 Q28 后并发工作

- **WHEN** 后续工作涉及跨 hart UART、TX 多 writer 语义或 RX 多 consumer 风险
- **THEN** MUST 先归类为 Q24、Q29 或 Q30
- **AND** MUST 保持当前 accepted-prefix 和 SPSC 边界直到对应 evidence Gate 通过

### Requirement: M36 — 异步 NIC 分层架构

异步高性能网卡 MUST 将硬中断、硬件队列服务、协议栈轮询和 socket readiness 作为分离的执行层。硬中断只处理 cause/ack/mask/wake；descriptor reap/refill 由有 budget 的 queue task 完成；smoltcp poll 由 task 上下文中的 stack runner 完成。不引入 Embassy executor。

**Legacy**: ADR-063 (A063), 2026-07-18 | **状态**: 候选

#### Scenario: 规划首个异步 NIC change

- **WHEN** 创建首个 StarryOS 异步 NIC change
- **THEN** MUST 保留 axnet-ng、smoltcp、axpoll 和 axtask 用于初始 MVP
- **AND** 硬中断工作 MUST 限定在 cause、ack/mask、snapshot 和 wake
- **AND** descriptor service 和 protocol-stack polling MUST 在硬中断上下文之外运行

### Requirement: M37 — PLIC/Clock trust-u-boot

VisionFive2 bring-up MUST 保留 U-Boot 配置的 PLIC 和 Clock 状态，除非诊断证明保留的状态无效。UART 寄存器初始化仍然允许（NS16550 寄存器重写无害）。范围收紧为 PLIC + Clock，不包含 UART。

**Legacy**: ADR-040 (A040), 2026-06-26 | **状态**: 🟡 Proposed

#### Scenario: VisionFive2 bring-up 保留 bootloader 状态

- **WHEN** StarryOS 通过 U-Boot 在 VisionFive2 上启动
- **THEN** PLIC 和 Clock setup MUST 遵循 trust-u-boot 策略，除非 Q18 诊断证明保留状态无效
- **AND** UART 寄存器初始化 MAY 仍可为 async driver 重配 FCR、IER 和 baud rate

### Requirement: M38 — PLIC 防御性设计

PLIC 初始化 MUST 保持 `init_primary()`（全局一次性初始化）与 `init_percpu()`（per-hart 配置）显式分离。`init_percpu()` MUST NOT 执行一次性 PLIC 构造或调用 `init_once()`。

**Legacy**: ADR-041 (A041), 2026-06-26 | **状态**: 🟡 防御性保留

#### Scenario: PLIC 初始化审查

- **WHEN** StarryOS 切换或更新 VisionFive2 平台 crate
- **THEN** PLIC 初始化路径 MUST 保持全局一次性初始化与 per-hart 初始化分离
- **AND** `init_percpu()` MUST NOT 调用 `init_once()` 或等效一次性 PLIC 构造

### Requirement: M39 — SMP 原子内存序按语义选择

跨 hart 共享的 async UART 状态 MUST 按同步角色使用 Rust 原子内存序，禁止按架构分叉。纯 telemetry 保持 Relaxed；发布/观察状态用 Release/Acquire；参与同步判断的 RMW 用 AcqRel；ier_cache 非原子 RMW 必须通过锁内 RMW 修复。

**Legacy**: ADR-042 (A042), 2026-06-27 | **状态**: ✅ QEMU 完成 / ⚠️ multi-hart 待验证

#### Scenario: Q17 atomic ordering review

- **WHEN** Q17 修改 async UART 原子字段
- **THEN** 所选内存序 MUST 根据字段角色说明理由
- **AND** MUST NOT 引入按架构分叉的内存序分支

### Requirement: M40 — Lichee RV Dock 启动链

Lichee RV Dock bring-up MUST 先沿用官方 `BOOT0 -> OpenSBI -> U-Boot -> Android boot image`，load/link 基线为 `0x40200000`，用 D1 UART0 polling early console 输出首字节。暂不启用 UART IRQ、async TTY、rootfs、USB、SD/MMC、benchmark。

**Legacy**: ADR-043 (A043), 2026-06-28 | **状态**: ✅ smoke complete

#### Scenario: Lichee RV Dock early bring-up

- **WHEN** StarryOS 启动 Lichee RV Dock bring-up
- **THEN** 首个可运行目标 MUST 为 Android boot image + D1 UART0 polling early console
- **AND** async TTY、UART IRQ、rootfs、USB、SD/MMC、shell、benchmark MUST 在 polling 串口输出确认前保持禁用

<!-- arc: MIG-20260720-legacy-specs --> Legacy: openspec/specs/architecture/spec.md (hash: 5b054d98), 1053 lines, ADR-001~063. Current valid constraints extracted as M01-M40. Decisions rationale preserved in decisions/spec.md. Tombstoned ADRs (A014-A017, A020-A021, A032, A063-A064) noted here — details in archive carriers ARC-202607081429 and arc-202607152005.
