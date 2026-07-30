# Spec: decisions — 决策记录

## Purpose

记录有替代方案且影响长期维护的重要选择、原因、替代方案、影响和状态。条目使用 `Dxx` 编号，被替代后保留历史并标记 `superseded`。Legacy ADR 原文保存在 `openspec/changes/archive/mig-20260720-legacy-specs/architecture-original.md`（hash: `5b054d98`）。

## Requirements

### Requirement: D01 — 异步运行时选型

异步运行时 MUST 采用 `axtask::future`（`block_on` + `poll_io` + `register_irq_waker`）+ `embassy-sync::AtomicWaker` 方案。

**Legacy**: ADR-001 (A001), 2026-05-24 | **状态**: ✅ accepted
**关联模型**: M01

- **原因**: axtask 已有调度器，embassy-executor 会冲突；embassy-sync 无 OS 依赖可单独使用；Pipe / EventFd 已验证 axtask::future 模式可行。
- **影响**: 保留内核调度器独立性；需自定义 AsyncUart trait；ISR 唤醒走 AtomicWaker::wake，O(1) 复杂度。
- **替代方案**: 完整 Embassy（与 axtask 冲突，拒绝）、仅 embedded-io-async traits（仍需自建 IRQ 绑定，拒绝）。

#### Scenario: 评估异步运行时替换

- **WHEN** 开发者提议替换当前异步运行时
- **THEN** 必须证明新方案不与 axtask 调度器冲突，且 ISR 唤醒延迟不超过当前 AtomicWaker

### Requirement: D02 — VFS 接口选择

UART 设备 MUST 通过 `DeviceOps` trait + `Device` wrapper 注册到 `/dev`。

**Legacy**: ADR-003 (A003), 2026-05-24 | **状态**: ✅ accepted
**关联模型**: M02

- **原因**: 所有现有 `/dev` 设备都通过 DeviceOps 注册；Device struct 自动处理转换链；`as_pollable()` 提供 poll/select/epoll 支持。
- **影响**: 注册代码与 event/fb 等设备一致；offset 参数对串口无意义可忽略。
- **替代方案**: 直接 impl FileLike（需重复实现 fd 管理逻辑，拒绝）。

#### Scenario: 评估新的设备注册方式

- **WHEN** 开发者提议改变设备注册机制
- **THEN** 必须证明新方式不破坏现有 poll/select/epoll 支持

### Requirement: D03 — 缓冲策略选型

缓冲策略 MUST 从 `ringbuf::HeapRb<u8>` + `axpoll::PollSet`（早期 ADR-004）演进为 `atomic_ring_buffer` + 专用 readiness/waker（ADR-061/062）。

**Legacy**: ADR-004 (A004), ADR-061 (A061), ADR-062 (A062) | **状态**: ✅ accepted → evolved
**关联模型**: M03

- **原因**: HeapRb 在 Pipe 中已验证、SPSC lock-free、零额外依赖；后期因中断安全与 SMP 需求演进为 atomic_ring_buffer。
- **影响**: 每端口 128 KiB 内存；硬件 FIFO 搬运由单一 copier 完成，禁止 ISR 直接操作 ring buffer。

#### Scenario: 评估缓冲方案替换

- **WHEN** 开发者提议替换 ring buffer 实现
- **THEN** 必须证明新方案支持 SPSC 无锁、中断安全、且不引入 MPMC 在没有 workload 证据的情况下

### Requirement: D04 — termios 策略

UART MUST 默认 raw 模式，termios 行规则作为可选功能通过 ioctl 动态启用。

**Legacy**: ADR-005 (A005), 2026-05-24 | **状态**: ✅ accepted
**关联模型**: M04

- **原因**: 高性能数据通道需要 raw 字节流零开销；终端交互需要 termios 行规则；两者兼得。
- **影响**: 默认路径零开销；termios 启用时复用现有 Termios 和 ldisc 逻辑。
- **替代方案**: 始终 raw（无法支持终端应用，拒绝）、始终 termios（所有数据路径都有开销，拒绝）。

#### Scenario: 修改 termios 默认行为

- **WHEN** 开发者提议改变默认模式
- **THEN** 必须证明不增加 raw 数据通路开销

### Requirement: D05 — 硬件抽象演进

硬件抽象 MUST 从早期 `AsyncUart` trait（ADR-006）演进为 2-trait 最小接口（ADR-036：`OsRuntime` + `OsWakerSet`），持续减少至最小可移植接口。

**Legacy**: ADR-006 (A006), ADR-033 (A033), ADR-035 (A035, tombstoned), ADR-036 (A036) | **状态**: ✅ accepted → evolved
**关联模型**: M05, M12, M14

- **原因**: ADR-035 的 5-trait "最小完备接口"被证伪——OsIrq/OsMmio/OsSpinNoIrq 从未被 import 或调用；实际最小集是 2。
- **影响**: uart_16550 os 模块 112→61 行；StarryOS adapter 123→63 行；新 OS 移植认知负担减半。
- **关键设计**: `UartPort` trait 解决 `&mut self` 问题；`OsSpinNoIrq` 用回调模式；ring buffer 静态变量由 OS 拥有；驱动用 `&'static Self` 兼容 no-alloc。

#### Scenario: 添加新的 OS abstraction trait

- **WHEN** 开发者提议新增 OS abstraction trait
- **THEN** 必须先追踪驱动代码，证明新增 trait 被实际调用，不得仅凭"未来可能需要"添加

### Requirement: D06 — DMA 策略

DMA 探索 MUST 归入远期 M6（现 Q25），M0~Q29 MUST 全程基于中断驱动 + NAPI 批量轮询优化。

**Legacy**: ADR-012 (A012), 2026-05-25 | **状态**: ✅ accepted
**关联模型**: M06

- **原因**: QEMU virt 平台没有真正的 16550 DMA 通道；DMA 需要真板或 virtio-console 方案。
- **影响**: 高吞吐场景用 NAPI 替代 DMA；性能优化聚焦中断驱动。

#### Scenario: 重新评估 DMA

- **WHEN** Q24 或新的高波特率硬件数据完成后需要重新评估 DMA
- **THEN** 必须按 O3/O40 决策树走：JH7110 是否有 DMA 控制器 → DMA 是否能访问 UART FIFO → PIO+中断 vs DMA 开销对比

### Requirement: D07 — 内核日志同步约束

内核启动日志的同步阻塞开销 MUST 接受为既定约束。外部 crate 层次 `axruntime → axplat → axhal → axtask → axpoll` 不可修改。

**Legacy**: ADR-013 (A013), 2026-05-27 | **状态**: ✅ accepted
**关联模型**: M07

- **原因**: 外部 crate 均来自 crates.io，不可修改；Console polling TX 是唯一的可靠内核日志路径。
- **影响**: 内核启动日志始终走同步 polling TX；用户态 Console 输出可通过 AsyncUart 异步化。

#### Scenario: 修改内核日志路径

- **WHEN** 开发者想改内核日志走异步路径
- **THEN** 不可行 — 必须保留 Console polling TX 作为内核日志通道

### Requirement: D08 — MMIO 权限误判纠正

UART MMIO 诊断 MUST 纠正 ADR-022/023 的误判——UART MMIO 在最终页表中已正确映射，真正根因是 stride=4 导致 LoadFault。

**Legacy**: ADR-024 (A024), ADR-026 (A026), 2026-05-31 | **状态**: ✅ accepted → key correction
**关联模型**: M08, M09

- **背景**: ADR-022/023 认为 axplat 限制 MMIO 权限导致方向 B P1/P2 阻塞。经深入验证发现：同一 4K 页表映射内 stride=1 raw read 成功而 stride=4 失败，排除页表问题。
- **根因**: NS16550 寄存器仅 0x00-0x07 共 8 字节，stride=4 下 ISR 偏移 2×4=8 访问 base+8 超出寄存器范围，QEMU 总线错误被 RISC-V 解释为 LoadFault。
- **影响**: 方向 A M3 和方向 B P1/P2 的"MMIO 权限阻塞"诊断全部有误。stride=1 后全部测试通过。

#### Scenario: UART 操作触发 LoadFault

- **WHEN** UART 读写出现 LoadFault / StoreFault
- **THEN** 必须先排查 stride/base 地址等代码 bug，再考虑页表权限

### Requirement: D09 — Console 与 Async 共存

Console（内核日志/早期启动/panic）与 Async（Shell I/O/用户态/高性能数据）MUST 共存。

**Legacy**: ADR-029 (A029), ADR-030 (A030), 2026-05-31 ~ 2026-06-01 | **状态**: ✅ accepted
**关联模型**: M10

- **原因**: ax_println! 依赖外部 crate 的 Console；早期启动需要 Console；panic handler 需要可靠输出。
- **实现**: AsyncUartWriter → ring buffer → TX copier → UART THR；ax_println! 走 polling TX，共享 UART THR 互不冲突。

#### Scenario: 评估 Console 移除

- **WHEN** 开发者提议完全剔除 Console
- **THEN** 不可行 — ax_println! 依赖外部 crate，必须保留 Console 作为内核日志通道

### Requirement: D10 — uart_16550 crate 提取

异步串口实现 MUST 从 StarryOS 提取到 uart_16550 crate（推翻原 ADR-007 D1 决策）。

**Legacy**: ADR-033 (A033), 2026-06-16 | **状态**: ✅ accepted
**关联模型**: M12

- **原因**: 其他 OS 项目也需要异步 UART；Q12 已完成基础设施迁移。
- **影响**: uart_16550 代码量增加 ~400 行；StarryOS 删除 ~370 行本地代码；其他 OS 只需实现 trait 即可使用异步 UART。
- **替代方案**: 保持异步在 StarryOS（复用性差，拒绝）、独立 crate uart_16550-async（维护负担，拒绝）。

#### Scenario: 评估 uart_16550 架构回退

- **WHEN** 开发者提议将异步栈移回 StarryOS
- **THEN** 必须证明当前 crate 提取方案造成无法解决的维护负担

### Requirement: D11 — LTO 延期

`lto = true` MUST 暂不开启，记录为已知优化手段，最终发布前再加回。

**Legacy**: ADR-034 (A034), 2026-06-16 | **状态**: ✅ accepted
**关联模型**: M13

- **实测效果**: Ring buffer TX 385→652 MB/s（↑69%），RX P50 200ns→<100ns。
- **决策理由**: LTO 使 release build 时间增加 2-3×；当前活跃开发期编译速度更重要。

#### Scenario: 发布构建准备

- **WHEN** 项目进入开发冻结期
- **THEN** LTO MUST 在发布构建前重新启用

### Requirement: D12 — TxCompletion 四阶段 drain

flush() 与 tcdrain MUST 使用 `TxCompletion { ring_empty, copier_active, staged_bytes, transmitter_empty }` 快照结构体轮询四阶段排空。

**Legacy**: ADR-037 (A037), 2026-06-23 | **状态**: ✅ accepted
**关联模型**: M15

- **原因**: 原 tcdrain 直接读 LSR 判断 TEMT 绕过 driver 架构；flush() 无任何等待；缺少 copier staging 可见性。
- **决策**: TxCompletion 4 字段 Relaxed 独立读取（flush 是 polling 语义）；TEMT corner-case 由 copier bounded spin 256 次处理；flush 保持纯事件驱动。
- **替代方案**: 在 flush() 中做 TEMT polling（拒绝，flush 应保持纯事件驱动）、暴露完整 LSR 寄存器（拒绝，增加 trait 耦合）。

#### Scenario: 修改 drain 语义

- **WHEN** 开发者提议修改 flush/tcdrain 的 drain 判断逻辑
- **THEN** 必须覆盖 ring→copier→FIFO→shift register 全部四阶段

### Requirement: D13 — TtyWrite 短写契约

`TtyWrite::write(&[u8])` MUST 改为 `write(&[u8]) -> usize`，让 `Tty::write_at()` 返回 writer 实际接受的字节数。

**Legacy**: ADR-038 (A038), 2026-06-23 | **状态**: ✅ accepted
**关联模型**: M16

- **原因**: RingBufTx::push() 已返回实际写入数但被丢弃；满 ring 时用户态看到"完整写入成功"导致 silent data loss。
- **影响**: uart_16550 公共 trait breaking change；用户态 write(2) 可能开始返回短写；PTY 溢出从 warn 变为返回短写。
- **替代方案**: 保持 void trait（继续 silent data loss，拒绝）、Tty::write_at 循环直到写完（隐式阻塞，拒绝）。

#### Scenario: 评估 TtyWrite 契约修改

- **WHEN** 开发者提议修改 TtyWrite 返回值语义
- **THEN** 必须证明不破坏现有 VFS/sys_write 的短写处理逻辑

### Requirement: D14 — Q15 增量融合策略

Q15 MUST 采用"增量重融合"策略：按 M0→M1→M2→M4→M3 顺序的 5 个原子 milestone 增量融合恢复 pre-M4 基线后丢失的 M4+ 修复。

**Legacy**: ADR-039 (A039), 2026-06-21~25 | **状态**: ✅ completed
**关联模型**: M17

- **背景**: M4 Sync 一次性 apply 全部 M4+ 代码导致 64B write+tcdrain 退化 73.9x（406µs→29.99ms）。
- **根因**: unblock_task(task, false) + 100Hz tick → 每 16B FIFO refill 触发 ~10ms 调度台阶。
- **结果**: 5 天完成 5 个 milestone + Manual QA；无退化复现。
- **铁律**: 禁止一次性 apply 多个 async-uart 优化 commit，必须按依赖排序 + 每步 Gate。

#### Scenario: 未来 async-uart 优化合并

- **WHEN** 开发者需要合并其他分支的多个优化 commit
- **THEN** MUST 遵循 Q15 增量融合策略，禁止一次性 merge

### Requirement: D15 — 平台解耦方向

平台事实 MUST 由 platform descriptor 集中表达；D1 MUST 通过本地 axplat crate 启动。

**Legacy**: ADR-044 (A044), ADR-045 (A045), ADR-046 (A046), 2026-06-28 | **状态**: ✅ accepted
**关联模型**: M18, M19, M20

- **原因**: uart_init.rs 原硬编码 QEMU 常量；axconfig 不能完整表达 ConsoleKind、reg_width、boot image strategy。
- **D1 PTE 修复**: 真板 Store/AMO access fault 根因为 C906 需要 T-Head normal-memory PTE flags。
- **影响**: QEMU、Lichee、VisionFive2 共享 descriptor + early console + backend 边界。

#### Scenario: 新增平台适配

- **WHEN** 开发者为 StarryOS 新增真板平台
- **THEN** MUST 先把平台事实记录到 platform descriptor

### Requirement: D16 — D1 async UART 实施路线

Q19B MUST 拆三模式（smoke/kbench/userbench），先嵌入 benchmark payload 再追求 SDMMC/rootfs parity。Q19C MUST 以 memory-root path/command 收敛 UART 性能验证。

**Legacy**: ADR-047~055 (A047~A055), 2026-06-29 ~ 2026-07-10 | **状态**: ✅ completed
**关联模型**: M21-M29

- **Q19B**: D1-safe async UART（stride 4 / 32-bit MMIO）+ PLIC IRQ 18 + embedded benchmark ELF。
- **Q19C**: memory-root `/bin/benchmark` + eager ELF mapping。M2 command-entry 收尾。
- **D1 THRE 边沿丢失**: IIR 常为 no-pending，有效 THRE 偶发。启用 THRE 时若 LSR 已 ready 必须软件 wake。
- **问题**: D1 64B 旧 1KB/s 是测量污染（stdout backlog），隔离后 93-97% 线速。P99 长尾接受为 known limitation。

#### Scenario: 重新开启 D1 SDMMC/rootfs

- **WHEN** storage/rootfs bring-up 被显式重新打开
- **THEN** MUST 创建新的独立 change，不作为 async UART gate

### Requirement: D17 — Q21/Q22 取消决策

Q21 UART user completion queue MVP 与 Q22 mmap user ring / zero-copy prototype MUST 不实施。保留现有 TX ring + copier + TxCompletion 路径。

**Legacy**: ADR-058 (A058), ADR-057 (A057), 2026-07-12~13 | **状态**: ✅ accepted
**关联模型**: M30, M31

- **数据依据**: Q20 D1 TX 达 95.2%-99.1% 线速；主要瓶颈是 115200 bps 物理 UART。
- **Q20 边界**: 只收敛 benchmark 证据，不改变 UART 驱动语义。
- **影响**: tasks.md 不再把 Q21/Q22 列为待做；O82 记录可借鉴但不实施的优化点。

#### Scenario: 重新考虑 user ring/completion

- **WHEN** 未来提议重新引入 UART completion queue 或 user ring
- **THEN** MUST 引用新证据证明当前路径不足，并保留 `/dev/console` fallback

### Requirement: D18 — io_uring 借鉴边界

后续 io_uring-inspired UART proposal MUST 识别借鉴点并证明当前路径不足。高价值借鉴方向：backpressure、writer/SPSC 隐患、TxCompletion drain。

**Legacy**: ADR-060 (A060), 2026-07-14 | **状态**: 候选
**关联模型**: M33

- **同构点**: 任务模型（copier+ISR wake+ring）、批处理已和 io_uring 同构。差异来自 UART/VFS 取舍，不是当前缺陷。
- **影响**: 后续 async UART 优化 MUST 引用本 ADR/R18 §6。

#### Scenario: 为 async UART 复用 io_uring 思想

- **WHEN** 未来 async UART proposal 以 io_uring 为动机
- **THEN** MUST 声明复用哪个思想及为什么当前路径不足

### Requirement: D19 — UART backpressure 与 writer 合同演进

UART TX 优化 MUST 分阶段处理：Q27 blocking backpressure MVP → Q28 writer contract convergence → Q29 reader contract audit。MPSC ring 后置 Q30。

**Legacy**: ADR-061 (A061), ADR-062 (A062), 2026-07-14~18 | **状态**: Q27/Q28/Q29 ✅ / Q30 🧊
**关联模型**: M34, M35

- **Q27 结果**: D1 S11 1024B short writes 36→0/102400B；无性能退化。
- **Q28 结果**: raw writer 收敛为不可 clone、unsafe 唯一构造。QEMU P50 改善 7.36%-15.75%。
- **Q29 结果**: raw reader unsafe unique constructor + crate-private mutation + exactly-once copier startup。62 unit + 8 doctest + 10 compile-fail 通过。
- **Q30 触发条件**: Q24 或真实 workload 证明需要 syscall 原子性、公平性或 MPSC 时才启动。

#### Scenario: 评估 MPSC ring 引入

- **WHEN** 开发者提议引入 MPSC ring
- **THEN** MUST 先区分目标是 atomicity、fairness、latency 还是 throughput
- **AND** MUST 比较 SPSC serialization、submission granularity、explicit scheduling 和 MPSC 四种方案

### Requirement: D20 — 异步 NIC 架构分层

异步 NIC MUST 采用队列任务与协议栈 runner 分层，不引入 Embassy executor。首阶段 MUST 保留 axnet-ng、smoltcp、axpoll 和 axtask。

**Legacy**: ADR-063 (A063), 2026-07-18 | **状态**: 候选
**关联模型**: M36

- **分层**: 硬中断（cause/ack/mask/wake）→ queue task（descriptor reap/refill，有 budget）→ stack runner（smoltcp poll，task 上下文）→ socket readiness。
- **借鉴**: embassy-net-driver Context 感知 readiness 与 RxToken/TxToken 所有权；ArceOS DWMAC/DMA 硬件证据。
- **拒绝**: 硬中断内全栈 poll、平台 IRQ 硬编码、全局大锁。

#### Scenario: 规划首个异步 NIC change

- **WHEN** 创建首个 StarryOS 异步 NIC change
- **THEN** MUST 定义 descriptor 状态机、IRQ rearm、register-recheck、TX/RX backpressure、DMA/cache barrier、budget 和公平性 Gates

### Requirement: D21 — PLIC/Clock trust-u-boot 与 PLIC 防御

VisionFive2 bring-up MUST 保留 U-Boot 配置的 PLIC 和 Clock 状态（范围收紧为 PLIC + Clock，不包含 UART）。PLIC init_primary/init_percpu MUST 保持显式分离作为防御性设计。

**Legacy**: ADR-040 (A040), ADR-041 (A041), 2026-06-26 | **状态**: 🟡 Proposed / 防御性保留
**关联模型**: M37, M38

- **arceos 教训**: "trust u-boot" 模式仅用于 DWMAC（以太网），不是平台级模式。7+ 次失败后才定档。StarFive UART 走 SBI，不做 UART MMIO init。
- **NS16550 差异**: UART 初始化（设波特率/FCR/IER）是简单寄存器写入，重复设置无害，不像 DWMAC PHY 协商会破坏已建立链路。
- **PLIC 防御**: 当前 axplat 已用 `static SpinNoIrq<Plic>` + 幂等 init_by_context，安全。但旧 arceos `LazyInit<Plic>` 反模式若被重新引入会导致 SMP panic。

#### Scenario: 评估 trust-u-boot 范围扩展

- **WHEN** 开发者提议将 trust-u-boot 扩展到 UART 或其他外设
- **THEN** MUST 先证明重复初始化会破坏已建立状态，NS16550 寄存器写入通常无害

<!-- arc: MIG-20260720-legacy-specs --> Legacy ADR source: `openspec/changes/archive/mig-20260720-legacy-specs/architecture-original.md` (hash: 5b054d98). Decision rationale extracted as D01-D21. Tombstoned ADRs: A014-A017, A020-A021, A032, A035, A056, A063-A064 → archive carriers ARC-202607081429, ARC-202607021648, arc-202607152005.
