# Spec: optimization — 优化记录

## Purpose

汇总 StarryOS 异步串口项目各阶段（Q0~Q15、Q20、Q27a/Q27/Q28/Q29 已完成；Q21/Q22/O80 退出当前规划；Q26 已实施并归档，部分运行时 Gate 为 ENV BLOCK；O77/Q30 保留为证据触发的后续优化）的性能与正确性优化条目。Q 编号对应 milestone，O 编号保留历史优化项身份。
## Requirements
### Requirement: UART 后续优化 Roadmap — Q27/Q28/Q29 已完成，Q30 证据触发

Q19/Q19B/Q19C 已完成并归档。2026-07-13 起，后续 UART 优化规划 MUST 不再把 user completion queue 与 `mmap` user ring / zero-copy 作为当前开发任务推进。Q27a/Q27/Q28 已于 2026-07-15 完成，Q29 于 2026-07-18 完成；Q26 于 2026-07-20 实施并归档，部分运行时 Gate 为 ENV BLOCK。Q24 multi-hart 复验等待真板；Q30 仍由 workload 证据触发。MPSC/MPMC 均不得作为默认方案。

**重排依据**: ADR-058、ADR-061、ADR-062、Q20 benchmark report、R19、L299、O82/O83/O84/O85/O86/O87。
**历史边界**: Q16~Q20 已完成；Q19D、M3/rootfs-probe、O79/O81、Q21/Q22 取消当前规划。Storage/rootfs 需要新 change。
**硬件边界**: D1 是单 hart，可验证 UART 语义、延迟、抖动和 CPU/counter proxy；不能证明 O63 multi-hart。

| Milestone | 目标 | 归属条目 | Gate |
|-----------|------|----------|------|
| **Q20** | Benchmark gap closure | O77 follow-up、ADR-057 | ✅ QEMU+D1 输出 latency、jitter、counter proxy 与 raw evidence |
| **Q21** | UART user completion queue MVP | ADR-056（A056，已归档）→ ADR-058 | 🧊 取消当前规划；现有 TX ring + copier + `TxCompletion` 已覆盖主要思想 |
| **Q22** | User ring + zero-copy prototype | O1/O36、ADR-056（A056，已归档）→ ADR-058 | 🧊 取消当前规划；D1 115200 bps 线速下收益不足 |
| **Q23** | Ring/completion performance decision | ADR-058、O82 | ✅ 决策完成：不实施 Q21/Q22，保留现有 write/writev/tcdrain/batch 路径 |
| **Q27a** | uart_16550 readiness 薄接口 | O83、ADR-061 | ✅ 2026-07-15 完成 RX/TX ring readiness hint + readable/writable waker 注册；不引入 OS fd 语义 |
| **Q27** | TX backpressure / writable wait MVP | O83、ADR-061 | ✅ 2026-07-15 完成并归档；阻塞 fd 等待 TX ring 空间，非阻塞 fd 保持 partial / WouldBlock，QEMU/D1 Gate 通过 |
| **Q28** | AsyncUartWriter writer 契约收敛 | O84、ADR-061 | ✅ 已归档 `2026-07-15-q28-async-uart-writer-contract`；MPSC 后置 O85 |
| **Q29** | AsyncUartReader consumer 契约审计 | O87、ADR-062 | ✅ 已归档 `2026-07-18-q29-async-uart-reader-contract`；unsafe unique reader、RX mutation 封闭、单次 copier 启动 |
| **Q30** | TX 多 producer 工业化语义决策 | O85/O86、ADR-062 | 单个 UART 也可有 kernel log、TTY echo、共享 fd 等多个逻辑 producer；仅在 Q24 或新 workload 证明原子性、公平性、锁竞争或吞吐需求时启动 |
| **Q24** | VisionFive2 / multi-hart revalidation | O63/O64/O65/O66/O71/O38/O39 | 等真板到手后建设并运行 stress；至少两个 hart 覆盖 read/write/flush/tcdrain 与 IER enable/disable |
| **Q25** | DMA / 高波特率决策 | O3/O40/O69/O41 | 用 Q24 或新硬件数据决定实施或拒绝 |
| **Q26** | 维护性清理 | O48/O49/O50、ADR-034 | <!-- Q26 --> ✅ 已归档；部分运行时 Gate 为 ENV BLOCK |

| 编号 | 当前结论 | 触发条件 |
|------|----------|----------|
| **O74/O75** | 平台 descriptor + early console 已落地 | 新平台适配时继续沿用 |
| **O77** | D1 THRE IRQ 不可靠时以 bounded slow-poll 保证 forward progress；功能和线速已达标，暂不为 CPU/MMIO 空轮询成本继续改动 | 后续出现 CPU/功耗需求时优化 D1 TX wake/watchdog，并验证其他真板是否需要同一 fallback |
| **O80** | Memory-root lazy COW SIGILL 属于 loader/mm，eager path 已满足当前 benchmark 与 UART 目标 | 退出当前规划；未来明确需要 lazy file-backed loader parity 时重新 propose |
| **O82** | io_uring-like user ring/completion 可借鉴但当前不实施 | 高波特率、多 writer 公平性、细粒度 completion 或 CPU 证据出现时 |
| **O83** | uart readiness 薄接口 + TX backpressure / writable wait 已完成 | Q27a/Q27 已归档 |
| **O84** | `AsyncUartWriter::Clone` 与 SPSC 契约已收敛 | ✅ Q28 完成 |
| **O85** | MPSC ring / 多逻辑 writer 公平性为工业化远期候选；不要求存在多个物理 UART | Q24 SMP 或新 workload 证明当前 producer serialization 不足时 |
| **O86** | 单 UART 上的 kernel log、TTY echo、共享 fd 等逻辑 producer 仍不保证 syscall/message 原子性、公平性与跨 write 不交错 | workload 明确要求且现有 accepted-prefix 契约不足时进入 Q30 |
| **O87** | RX SPSC 单 consumer capability 已收敛 | ✅ Q29 完成 |

<!-- tombstone: O76/O77 original Q19B scope --> Archived 2026-07-02 in ARC-202607021648 — Q19/Q19B 原范围已完成；O77 后因 Q19C/Q20/D1 polling 证据以 follow-up optimization 重新开放。
<!-- tombstone: O78/O79/O81 --> Archived 2026-07-11 in ARC-202607111510 — Q19C memory-root path/command 已完成，Q19D/O79 与 M3/O81 取消当前规划。

#### Scenario: Roadmap-driven scheduling

- **WHEN** 新增或重排 Q15 后优化项
- **THEN** MUST 先判定该项属于文档收敛、QEMU correctness、真板观测、真板验证、数据驱动决策、维护清理还是远期实验
- **AND** MUST 放入对应 active milestone 或 O82 远期候选，禁止只按 O 编号顺序排期
- **AND** 对 R19/ADR-061 中明确为近期的 backpressure / writer 契约项，MUST 放入 Q27/Q28，不得继续混入 O82 远期 user ring 桶

#### Scenario: QEMU-only work before hardware

- **WHEN** multi-hart 真板尚未到位
- **THEN** MUST 优先收敛 Q20 已有证据和文档，不得新开 Q21/Q22 user ring/completion 实施
- **AND** MUST NOT 在 QEMU 上声称 DMA、高波特率或绝对吞吐量结论

#### Scenario: 多平台常量进入驱动初始化

- **WHEN** 开发者为 QEMU、Lichee RV Dock、VisionFive2 增加 UART base / irq / stride / access width / boot image 参数
- **THEN** MUST 将其放入 platform descriptor 或等价集中配置
- **AND** MUST NOT 在 `kernel/src/drivers/uart_init.rs` 内继续追加板级常量或平台分支

#### Scenario: Lichee RV Dock smoke test 排期

- **WHEN** Q18 platform descriptor 与 early console 基础完成
- **THEN** Q19 MUST 优先做 Android boot image + D1 UART0 polling 输出
- **AND** rootfs / USB / SDMMC / async TTY / benchmark MUST remain deferred until `[starry-d1] smoke complete, halting.` is visible on serial
- **AND** after Q19 completion, further Lichee work MUST be planned as a new scoped stage instead of expanding O76

#### Scenario: Lichee RV Dock async UART benchmark 收尾

- **WHEN** Q19B claims Lichee async UART benchmark completion
- **THEN** both `lichee-kbench` and `lichee-userbench` MUST have true-board serial evidence
- **AND** the user benchmark MUST print TX throughput, TX latency, FIFO boundary matrix, and nonblocking read sections
- **AND** `tcdrain` MUST be validated on real D1 UART state, including THRE no-pending / edge-loss behavior
- **AND** later SDMMC/rootfs parity MUST be planned as a separate milestone, not folded back into Q19B

#### Scenario: Lichee async UART benchmark 收敛

- **WHEN** Q19C 进入实施
- **THEN** MUST prioritize D1 async UART kernel/user benchmark evidence, benchmark manifest cleanup, and memory-root path/command proof
- **AND** shell, SDMMC, block, and real rootfs work MUST NOT be required for Q19C completion
- **AND** full D1 SDMMC/block/rootfs implementation MUST be started only by a new storage/rootfs change after explicit goal confirmation
- **AND** lazy file-backed COW loader repair MUST be tracked as O80, not as an async UART correctness gate

#### Scenario: M3/rootfs-probe is not a current gate

- **WHEN** `lichee-d1-rootfs-probe` is used for Q19C M3
- **THEN** missing probe output MUST NOT block Q19C async UART benchmark completion
- **AND** further probe entry isolation MUST be deferred until storage/rootfs bring-up is explicitly re-opened

#### Scenario: 真板启动失败或串口无输出

- **WHEN** VisionFive2 上 UART 无输出 / 数据乱码
- **THEN** MUST 优先排查 O38（时钟配置）而非波特率或软件路径

#### O63 风险评估：QEMU 掩盖的内存序问题

**时间**: 2026-06-26 发现，2026-07-03 QEMU gate 收尾。
**原因**: QEMU 当前单 hart，`Relaxed` 风险被掩盖；VisionFive2 等多 hart 环境中 task 与 ISR 可能跨 hart 并发。`critical_section` 只关本地中断，不能提供 SMP 互斥。
**决定**: `ier_cache` RMW 必须进同一锁/临界区；`tx_copier_active` 用 Release/Acquire；`tx_staged_bytes` RMW 用 AcqRel/Acquire；纯 telemetry 可保留 Relaxed。
**已完成**: QEMU `update_ier()` cache RMW 与 MMIO IER 写同锁；D1 `update_ier()` 用 IRQ-off 临界区；QEMU rootfs benchmark 通过（64B TX 153.86 KB/s，1B latency avg 0.182 ms，FIONBIO PASS）。
**未关闭**: 多 hart stress 未跑。Q24 必须在至少两个 hart 上复验跨 hart write/flush/tcdrain，并覆盖 read 与 IER enable/disable；Gate 检查数据丢失、重复、`staged_bytes` 漂移与 hang。

| 字段 | 原风险 | 处理 |
|------|--------|------|
| `ier_cache` | load-modify-store 互相覆盖，导致 RX/TX 中断位丢失 | RMW 与 MMIO IER 写入放同一锁/临界区 |
| `tx_copier_active` | flush/tcdrain 看见陈旧 active/inactive | store Release，load Acquire |
| `tx_staged_bytes` | staged 计数陈旧，drain 误判 | fetch_add/sub AcqRel，load Acquire |
| telemetry counters | 单写者诊断字段 | 保留 Relaxed |

#### Scenario: 真板多核下出现数据丢失或 hang

- **WHEN** multi-hart stress shows UART data loss, flush hang, tcdrain hang, or staged_bytes drift
- **THEN** O63 fields MUST be checked before changing UART semantics

#### Scenario: Q17 QEMU 修复完成但多 hart 未实测

- **WHEN** Q17 passes QEMU single-hart cargo check, shell, and `/bin/benchmark`
- **THEN** Q17 MAY be marked QEMU gate complete
- **AND** O63 cross-hart risk MUST remain open until Q24 or equivalent SMP stress passes

### Requirement: ArceOS 借鉴清单（从明扬 arceos 异步化工作获取经验）

本节 MUST 只登记从 arceos（`/home/daivy/projects/serial/others/arceos/`）识别、且 StarryOS 尚需新增工作的模式；等价实现标记“✅ 已采纳”。来源：`.claude/analysis/arceos-borrowable-experience.md` `[ARCHIVED 2026-07-04 → _archive/2026-07-04-q19-lichee-analysis/]`。

| ID | 来源 | 描述 | 优先级 | 触发条件 |
|----|------|------|--------|---------|
| **O64** | arceos ADR-004（PIT-007 / TIP-004） | **trust u-boot 仅用于 PLIC+Clock**：VF2 U-Boot 已配置全局状态；UART 仍可重设 FCR/IER/波特率（starfive UART 走 SBI，无 MMIO 先例）。7+ 次失败后由 commit `4334e41` 定档（ADR-040 Revised）。 | 🔴 P0 | VisionFive2 硬件到位 |
| **O65** | arceos ADR-002（PIT-003） | **PLIC primary/percpu 防御性分离**：当前 axplat 0.3.1-pre.6/0.1.0-pre.2 已用 `static PLIC: SpinNoIrq<Plic>` + 幂等 `init_by_context()`，不存在旧 `LazyInit`/percpu 重初始化反模式；保留验证（ADR-041 Revised）。 | 🟡 P1（防御性） | Q24 平台切换时验证 |
| **O66** | arceos TIP-004 | **`print_preserved_status()` 验证函数**：UART / PLIC / Clock init 前后 dump 当前寄存器状态，与 U-Boot/Linux 预期对比。arceos `DwmacHalImpl::configure_platform` 实现此模式。**Q20 真板观测必备**（O64 的前置依赖）。 | 🔴 P0 | VisionFive2 硬件到位 |
| **O69** | arceos axdma + DwmacHal | **DMA 一致性内存抽象**：`DMAInfo { cpu_addr, bus_addr }` 二元组 + UNCACHED 映射 + cache_flush_range。**⏳ 与 O3/O40 合并**：JH7110 是否有外部 DMA 控制器未知，Q25 按 O3/O40 决策树走。如引入，**借鉴** axdma + DwmacHal cache_flush_range 模式。 | ⏳ Q25 决策 | Q24 或新硬件数据 + O3 评估 |
| **O71** | arceos TIP-005 | **PAC 类型安全寄存器访问**：用 `jh7110_vf2_13b_pac` 而非 `write_volatile(magic_offset)`。编译期类型检查 + IDE 自动补全。**⏳ 待评估**：Q24 真板驱动开发时考虑引入，避免 magic offset。 | 🟡 P1 | Q24 真板驱动开发 |
| <!-- O77 --> **O77** | Q19C/Q20/Q29 后 D1 TX 诊断 | **已接受的正确性妥协，效率待优化**：D1 THRE IRQ/IIR no-pending 路径不能作为唯一进展来源，当前通用 copier 使用 `TX_FAST_RETRY_LIMIT=32` + `TX_SLOW_POLL_LIMIT=4096`×`TX_SLOW_POLL_SPINS=256` + bounded yield fallback。2026-07-18 D1 fullbench 达物理线速约 95.3%-99.1%，退出码 0，且 `slow_poll_exh=0`/`yield_exh=0`；但 13,834,811 次 hardware send 中 13,813,186 次返回 0（99.84%，41,834.7 zero/KiB），说明发送期间以大量 CPU/MMIO polling 换取 forward progress。QEMU 因不仿真真实线时几乎不进入 slow-poll；其他真板的 THRE IRQ 可靠性和 fallback 成本尚未验证。 | 🟡 功能保留 / 效率待优化 | 优先调查 D1 THRE/PLIC 时序；目标为 IRQ-first + 定时 watchdog，或将 D1 workaround 平台化；不得回退为无软件 fallback 的纯 IRQ |
| **O80** | Q19C-M1 loader/mm 后续 | **退出当前规划**：lazy file-backed COW 路径在 main 前触发 `SIGILL`，但 eager VFS mapping 已完整通过并满足当前 UART benchmark。该问题属于 loader/mm，不再占用 UART 或多 hart 真板前工作；未来明确需要按需文件映射/COW parity 时重新 propose。 | 🧊 当前不做 | 明确恢复 lazy file-backed loader parity 时 |
| **O83** | R19 / ADR-061 | **uart readiness 薄接口 + TX backpressure / writable wait MVP**：已由 Q27a/Q27 完成并归档。`uart_16550` 暴露 RX/TX ring readiness 与 waker 注册，OS 层复用 `poll_io`、`Pollable::OUT` 和 TX pop wake；UART 阻塞 fd 等待空间，非阻塞 fd 保持 partial/`WouldBlock`，PTY 保持非等待契约。QEMU、D1 Gate 通过，S11 short write 归零且关键性能无退化。 | ✅ 已完成（2026-07-15） | `2026-07-15-q27-tx-backpressure` |
| **O84** | R19 / ADR-061 | **`AsyncUartWriter::Clone` 与 SPSC 契约收敛**：Q28 已移除 raw writer `Clone`/共享 `TtyWrite`，改为 unsafe 唯一构造与 `&mut self` 提交；StarryOS direct-output/echo 通过共享 `SpinNoPreempt` adapter 串行化单次 push。compile-fail、并发 accepted-prefix、Q27 回归及 QEMU/D1 单次性能 Gate 均通过；不引入 MPSC。 | ✅ 已完成（2026-07-15） | `2026-07-15-q28-async-uart-writer-contract` |
<!-- tombstone: O67/O68/O70/O72/O73 --> Archived 2026-07-02 in ARC-202607021648 — 已采纳/已蕴含/已领先项从 active optimization 清单移除。

#### Scenario: Q17-Q20 真板启动顺序（O63 + O74/O75/O76 + O64/O66 协同，Revised 2026-06-28）

- **WHEN** VisionFive2 硬件到位启动真板验证
- **THEN** MUST 按顺序实施：(1) Q17 / O63 内存序修复（P0 — 先修 `ier_cache` RMW 竞争，再修 `tx_copier_active`/`tx_staged_bytes`）→ (2) Q18 / O74-O75 平台参数解耦与 early console 基础 → (3) Q19 / O76 Lichee RV Dock 单核 smoke test 演练启动链 → (4) Q20 / O66 `print_preserved_status()` 验证 U-Boot 已配置 PLIC/Clock 状态 → (5) Q20 / O64 PLIC+Clock trust-u-boot 模式（**不限制 UART 初始化**）→ (6) Q20 / O65 验证 axplat crate PLIC 初始化路径 → (7) Q20 跑通 Q15 Manual QA 全部 12 项
- **AND** MUST 失败时优先排查 O63（内存序），其症状（staged_bytes 漂移 / flush hang / RX 停滞）最难定位
- **AND** UART 可正常重新初始化 FCR/IER/波特率，无需 trust-u-boot

#### Scenario: Q19C 评估 O77（D1 TX zero-send / P99 长尾）

- **WHEN** 开发者继续优化 D1 TX copier、THRE wake、`tcdrain` 或 retry policy
- **THEN** MUST 先证明 `make lichee-userbench` 产物在真板上能越过 `benchmark process spawned` 并完整输出 benchmark exit code 0
- **AND** MUST 用 gated TX debug snapshot 记录 `hw_send_zero`、`no_progress_budget_exhausted`、`hw_send_max_chunk`、`ring_pop_bytes` 与 final/second drain
- **AND** MUST 不得把 `TX_FAST_RETRY_LIMIT=0` + drain-side `TX_WAKER` 注册作为默认修复，除非另有软件 fallback 证明不会丢失 forward progress
- **AND** MUST 区分 D1-specific THRE/PLIC workaround、QEMU timing model 与其他真板实测结果，不得因 QEMU fast path 正常就声明所有硬件可纯 IRQ 前进
- **AND** SHOULD 优先比较 IRQ-first + timer watchdog、D1 platform-gated fallback 与现有 continuous slow-poll；验收同时覆盖 forward progress、线速比例、zero/KiB、copier busy time/cycles 和 P99

#### Scenario: Q25 评估 O69（DMA 决策树）

- **WHEN** Q24 或新的高波特率硬件数据完成后需要重新评估 DMA
- **THEN** MUST 按 O3/O40 决策树走：(1) JH7110 是否有 DMA 控制器 → (2) DMA 是否能访问 UART FIFO → (3) PIO+中断 vs DMA 开销对比 → (4) 是否需要更高波特率（O41）
- **AND** 如决定引入 DMA，**借鉴** arceos `axdma` 的 `DMAInfo` 二元组模式与 `DwmacHal::cache_flush_range` 处理

#### O82: io_uring-like user ring/completion 优化 — 当前不实施

**2026-07-13 / 🧊 远期候选**：`write()`→TX ring、`uart-tx-copier`、`tcdrain()`/`flush()`+`TxCompletion` 已形成提交/执行分离；D1 达 95.2%-99.1% 线速，user CQ 或 `mmap` ring 当前无可见收益。

| 可借鉴项 | 适用条件 | 当前处理 |
|----------|----------|----------|
| completion 观测增强 | 需要按单次提交追踪完成 | 保留 `TxCompletion` 全局 drain，暂不加 CQ |
| submit batch id / watermark | 需要判断某次 write 是否已物理发送 | 暂不加 request id 或 offset watermark |
| backpressure 可观测性 | blocking write / poll/select 需要更细 writable 信息 | 已由 O83 / Q27a+Q27 完成并通过 QEMU、D1 Gate |
| counter 分阶段细化 | 需要定位 P99 tail 或 CPU proxy | 继续使用 S40；需要时再扩展 |
| 多 writer 公平性 | 日志刷屏影响交互或多 producer 抢占 | API 契约已由 O84 / Q28 收敛；MPSC ring 仍为 O85 远期候选 |

#### Scenario: 评估 O82 user ring/completion

- **WHEN** 开发者重新提出 UART completion queue、`mmap` user ring、zero-copy TX/RX 或每请求 completion
- **THEN** MUST 先证明当前 `write()` / `writev()` / batch-drain / `tcdrain()` 路径在目标硬件上不是线速瓶颈
- **AND** MUST 说明收益来自减少 copy、减少 syscall、改善 tail latency、改善 CPU proxy 还是多 writer 公平性
- **AND** MUST 保留 `/dev/console` read/write fallback
- **AND** MUST NOT 因为概念类似 `io_uring` 而实施通用 SQ/CQ 机制

### Requirement: Q28 后并发契约 backlog

Q28 后续优化 MUST 将 TX 调度语义、队列 producer 模型与 RX consumer 安全分别评估，禁止把它们合并为“改多方 ring”单一任务。Q30 面向工业化的多逻辑 producer 语义：即使只有一个物理 UART，kernel log、TTY echo、共享/复制 fd 和多个任务也可能形成多个 producer；但当前 StarryOS 没有证据表明 serialized SPSC adapter 不足，因此只保留远期能力入口。

| 编号 | 问题与当前边界 | 优先级/状态 | 触发条件 |
|------|----------------|-------------|----------|
| **O85** | TX ring 仍是 SPSC；MPSC 只可能改善单 UART 上多个逻辑 producer 的锁竞争、吞吐或调度策略，不自动提供 syscall 原子性 | 🧊 工业化远期候选 / Q30 | Q24 或新 workload 证明现有 producer serialization 不足 |
| <!-- O86 --> **O86** | Q28 只保证每次 raw submission 的 accepted prefix 连续；blocking syscall 可分段，kernel log、TTY echo、共享 fd 等 producer 可在重试间提交，不保证整个 syscall/message 原子性、公平性或跨 write 不交错 | 🧊 证据触发 / Q30 | 真实应用要求消息边界，或观测到饥饿、交互延迟、优先级反转 |
| <!-- O87 --> **O87** | RX ring 保持 SPSC；`AsyncUartReader::new()` 已改为 unsafe unique constructor，RX mutation 已封闭，StarryOS 唯一 reader 移入单 `tty-reader`，共享 fd 只消费 ldisc ring | ✅ Q29 完成 | 新 raw multi-consumer 需求出现时重新规划，不默认引入 MPMC |

#### Scenario: TX workload requires stronger multi-producer semantics

- **WHEN** Q24 或新 workload 观测到 syscall/message 边界破坏、producer 饥饿、交互延迟或 producer lock 吞吐不足
- **THEN** Q30 MUST first state whether the target is atomicity, fairness, latency, or throughput
- **AND** it MUST compare SPSC serialization, submission granularity, explicit scheduling, and MPSC instead of assuming MPSC solves every target
- **AND** absent such evidence, the current accepted-prefix contract and O85 far-future status MUST remain unchanged

#### Scenario: RX consumer contract remains enforced

- **WHEN** 后续修改 `AsyncUartReader` construction、RX mutation 或 TTY reader ownership
- **THEN** it MUST preserve the unique raw consumer witness and audit every constructor and RX pop path
- **AND** it MUST preserve readiness register/recheck semantics and test for duplicate, lost, or concurrently consumed bytes
- **AND** it MUST NOT introduce MPMC unless a real multi-consumer requirement is demonstrated

### Requirement: 远期优化（优先级低，不确定是否做）

远期优化条目 MUST 在评估 ROI 后决定是否实现；不作为里程碑硬性要求。

| 编号 | 内容 | 优先级 | 说明 |
|------|------|--------|------|
| **O1 / O36** | 零拷贝 RX | — | mmap ring buffer 到用户空间；当前由 O82 判定为远期候选 |
| **O5** | 协程优先级调度 | — | 取决于 axtask 支持 |
| **O37** | kernel log TX 合并 | — | `ax_println!` 走 ring buffer |
| **O32** | poll_fn 闭包 | — | 编译器可能已优化 |
| **O82** | user ring / completion 可借鉴项 | — | completion 观测、watermark、counter；backpressure 已提升 O83，MPSC 公平性后置 O85 |
| **O85** | MPSC ring / 多 writer producer model | — | 仅当 Q24 SMP 或新 workload 证明 producer 侧串行化不足时评估；原子性/公平性目标另由 O86 定义 |

<!-- tombstone: O45 --> Archived in optimization/spec.md #O45 2026-06-16 — ✅ 已完成（2026-06-11 Q8），tcdrain 真异步化
<!-- tombstone: O46 --> Archived in optimization/spec.md #O46 2026-06-16 — ✅ 已完成（2026-06-11 Q8），AtomicWaker 推广 8 处
<!-- tombstone: O47 --> Archived in optimization/spec.md #O47 2026-06-16 — ✅ 已完成（2026-06-11 Q9），VTIME 超时机制

#### Scenario: 评估 O1/O36 零拷贝 RX

- **WHEN** StarryOS 演进到需要减少 RX 路径内存拷贝，且 O82 的触发条件成立
- **THEN** MUST 评估 `mmap ring buffer 到用户空间` 的实现复杂度与安全边界
- **AND** 收益 MUST 量化（当前 RX 路径 5 次拷贝的减少数）
- **AND** 禁止在未评估前直接实施

### Requirement: 2026-06-11 死代码审计后续优化

本次审计发现的后续优化机会 MUST 在评估 ROI 后决定是否启用或彻底移除；死代码 SHALL 不长期保留。

| 编号 | 内容 | 优先级 | 说明 |
|------|------|--------|------|
| **O48** | memtrack 模块集成 | 🟢 低 | <!-- Q26 --> ✅ feature 路径、三态 session 和 `axalloc::tracking` API 已修复；8 个 host tests 通过，运行时交互为 ENV BLOCK |
| **O49** | ProcessMode::Manual 移除 | 🟢 低 | <!-- Q26 --> ✅ Q26 已完成：Manual 变体、内部 match 分支、ProcessMode eval 已删除，TTY/PTY 行为保持 |
| **O50** | 预留接口评估 | 🟢 低 | <!-- Q26 --> ✅ Q26 已完成：create_pty_master / DeviceMmap::ReadOnly 已删除，memtrack helpers 收敛到 feature 内部 |

#### Scenario: 修复 O48 memtrack 模块

- **WHEN** Q26 启用 `MEMTRACK=y` 验证内存调试工具
- **THEN** MUST 修复 Cargo feature 传播和当前 API 适配
- **AND** `/dev/memtrack` 的非法命令或状态转换 MUST NOT 导致内核 panic

#### Scenario: 决定是否移除死代码

- **WHEN** 开发者发现标注 `#[allow(dead_code)]` 的预留接口超过 90 天未被使用
- **THEN** MUST 评估是否彻底移除，禁止无限期保留

### Requirement: 远期优化（路径 B — 未来评估）

embassy 调研中识别的架构级优化，MUST 在路径 A（Q12）完成并量化收益后再评估实施。

| 编号 | 内容 | 优先级 | 说明 | 前置条件 |
|------|------|--------|------|------|
| **O54** | ISR 直接搬运（移除 copier 任务） | 🟡 中 | ISR 中直接执行 FIFO→ring buffer 搬运（embassy `BufferedUart` 模式），消除 RX/TX 两个 copier 任务的任务切换开销。需重新评估 NS16550 ISR 延迟（当前 ~1.5 µs，加搬运后预估 ~3-5 µs @ 16 字节 FIFO）。 | O51 就位 + benchmark 验证无退化 |
| **O55** | 半满/IDLE 唤醒策略 | 🟢 低 | 引入 embassy 的 `is_half_full()` 唤醒阈值 + IDLE line 检测（`ReceptionTimeout` 中断），减少高频 RX 下的 copier 唤醒次数。当前 NAPI 阈值（16 次连续成功）效果类似但更复杂。 | O51 或 O54 就位 |

**评估标准**：路径 B 的每项 MUST 在 Q12 完成后用 benchmark 量化路径 A 收益，再决定是否投入。禁止在未验证路径 A 收益的情况下直接实施路径 B。

<!-- tombstone: O45/O46/O47-detail --> Archived 2026-07-03 in ARC-202607031929 — 旧详细方案从 active optimization spec 移除；当前只保留触发条件与现行结论。

#### Scenario: 评估远期优化 ROI

- **WHEN** 开发者考虑实现 O45 / O46 / O47 / O1 / O5 / O37 / O32 之一
- **THEN** MUST 评估实施成本 vs 性能收益（O1 / O37 高成本低收益需充分论证）

### Requirement: 已排除优化 — 不实施

通用分发结构类优化 MUST 在专用驱动场景下禁止实施。`O17`（中断分发效率）已明确排除：ISR 使用 AtomicWaker 直接唤醒（O(1)），无需 BTreeMap 分发机制。详见 `learned` L128。

**Embassy 误用场景**（2026-06-05 评估，详见 `learned` L81~L84）：

| 反优化 | 当前实现 | Embassy 替代 | 排除原因 |
|--------|----------|--------------|----------|
| **OE1** Channel 替换 HeapRb | `ringbuf::HeapRb<u8>` (SPSC) | `embassy_sync::Channel<u8, N>` (MPMC) | 失去 lock-free SPSC，多一层间接，heap 灵活性丧失 |
| **OE2** Mutex 替换 SpinNoPreempt | `Arc<SpinNoPreempt<...>>` | `embassy_sync::Mutex` | 同步临界区加异步 Mutex 反而更慢，且无法跨 `.await` 持有 |
| **OE3** Watch 替换 AtomicBool | `AtomicBool` (FIONBIO) | `embassy_sync::Watch<bool>` | 单 bool 用 Watch 是杀鸡用牛刀，AtomicBool 更直接 |
| **OE4** Semaphore 计数 NAPI | 状态机 + 计数器 | `embassy_sync::Semaphore` | 错误工具（Semaphore 是资源计数，不是事件计数）|
| **OE5** select! 替换手动 poll | 手动 `block_on(poll_io(...))` | `embassy_futures::select!` | axtask::future 不可与 select! 宏组合，需切换 executor |

**判定原则**：项目采用极简 embassy-sync 子集（仅 `AtomicWaker`），任何"用 embassy 包装替换简单 Rust 原语"的提案 MUST 先用 `codegraph_impact` 评估改动范围 + 性能基准，否则禁止实施。

#### Scenario: 评估 O17 类"通用分发"优化

- **WHEN** 开发者考虑引入 BTreeMap / HashMap 等通用分发结构
- **THEN** MUST 评估 waker 数量：固定少数 → AtomicWaker；通用动态 → register_irq_waker。专用驱动场景下禁止过度设计

#### Scenario: 评估 embassy 包装替换

- **WHEN** 开发者提议用 embassy 同步原语（Channel / Mutex / Watch / Semaphore）替换现有实现
- **THEN** MUST 先证明：(1) 当前实现有可测性能问题，(2) embassy 方案在该场景下更快/更简洁，(3) 不与 axtask 架构冲突。**禁止**为"用 embassy"而替换

### Requirement: 性能指标基线与硬件理论极限

Performance benchmarks and comparisons MUST use the baseline data below. All metric claims MUST label QEMU vs real-hardware credibility. QEMU throughput data SHALL be explicitly marked untrusted.

**NS16550 @ 115200 bps 硬件理论极限**：

| 参数 | 值 |
|------|-----|
| 线速 | 11,520 B/s（10 bits/byte × 115200） |
| 单字节传输时间 | 86.8 µs |
| FIFO 深度 | 16 字节 |
| IRQ 频率（阈值 14） | ~823/秒，间隔 1.22 ms |
| ISR 总延迟 | ~1.5 µs（< 0.1% 线时间） |
| MMIO 单次访问 | ~100~200 ns |

**当前 QEMU async 性能指标**：

| 指标 | 目标 | 测量方法 | 当前 |
|------|------|---------|------|
| 吞吐量 @115200 | > 10 KB/s（90% 线速） | `write → tcdrain()`，5 秒批量 | TX: 未准确测量（写 /dev/null） |
| 延迟 P50 | < 500 µs | 单字节 `write+tcdrain` | ~1 µs（仅 ring buf push） |
| 延迟 P99 | < 2 ms | 同上 | — |
| 空闲 CPU | **0%**（无 yield storm） | 无数据 10 秒 | 偏高（yield storm） |
| 数据完整性 | 100% | 1 MB MD5 | ✅ |
| **非阻塞读（Q7 后）** | `read()` 空数据立即 EAGAIN | `ioctl(FIONBIO)` + `read()` | ❌ 未生效 |

**CPU 占用对比**（统一数据量 102,400 字节）：

- Console：3,835 cycles/byte
- Async：268 cycles/byte（效率高 14.3 倍）

**RX 性能**（内核态 Ring Buffer 直接测，绕过 TTY）：

- 吞吐量：588,776 KB/s
- 延迟 P50：600 ns

**QEMU 时序欺骗边界**（`learned` L141）：

- QEMU 16550 不仿真串口线延迟，所有 tcdrain/LSR 轮询的吞吐量测试在 QEMU 上**不可信**
- **📐 物理定律**（100% 准确）：真板 NS16550 @ 115200 bps 线速上限 = 11,520 B/s（单字节 86.8 µs），实测值受调度/IRQ 延迟影响可能低于此值
- **可靠指标（QEMU 也可测）**：内核态 ring buffer 速度、`write()` 延迟、CPU cycles/byte

#### Scenario: 声明性能数字

- **WHEN** 开发者 / 用户要声明某项性能指标
- **THEN** MUST 注明：(1) QEMU 还是真板，(2) 测试方法（绕过 TTY / 完整链路），(3) 数据量。**禁止**用 QEMU 吞吐量冒充真板吞吐

### Requirement: Q13 Trait 抽象开销优化 — 短期已规划 / 中长期待探索

Q13 完成后性能测试显示 trait 抽象开销导致 +13% avg latency 退化（124µs → 140.1µs）。短期优化（inline + batch）MUST 实施为 Q13.1；中长期优化点 SHALL 记录在下方表格。

**Q13.1 短期优化（已完成）**：

| 编号 | 内容 | 预期收益 | 实际收益 | 可移植性影响 | 状态 |
|------|------|----------|----------|-------------|------|
<!-- tombstone: O56/O57/O61 --> Archived 2026-07-02 in ARC-202607021648 — Q13.1/LTO 短期优化已完成，active 表仅保留中长期候选。

**Q13.1 性能验证结果**（2026-06-16）：

| 指标 | Q13 优化前 | Q13.1 优化后 | 变化 | Q12 基线 |
|------|-----------|-------------|------|----------|
| 1B avg | 140.1µs | 129.5µs | -10.6µs (-7.6%) | 124µs |
| 1B P50 | 138.8µs | 125.5µs | -13.3µs (-9.6%) | 115.7µs |
| overhead | 53.3µs | 42.6µs | -10.7µs (-20.1%) | 37.1µs |

**结论**：
- ✅ 目标达成：1B avg = 129.5µs ≤ 130µs
- ✅ overhead 减少 20%（53.3 → 42.6µs）
- ⚠️ 与 Q12 差距 +5.5µs（129.5 vs 124），为可移植性合理代价
- **批量操作在内嵌时就该做**：算法优化（批量）应尽早实施，编译器优化（inline）可等需要时再加

**O61 LTO 跨 crate 内联详情**（2026-06-16）：

- **原理**：Cargo `[profile.release] lto = true` 使编译器在链接阶段可跨 crate 边界内联函数调用。uart_16550 的 `[inline(always)]` 只能在 crate 内生效；LTO 消除了 embassy_hal_internal 等依赖的跨 crate 函数调用开销。
- **改动**：uart_16550 + StarryOS 双 repo `Cargo.toml` 各加 3 行
- **内核态 ring buffer 收益**：TX 385→652 MB/s（↑69%），RX P50 200ns→0ns（avg 316→195ns）
- **e2e 延迟**：129.4µs（不变），瓶颈在调度不在函数调用
- **副作用**：release build 时间增加（内核规模小，影响可控）
- **注意**：LTO 是顶层行为，依赖它的 OS 项目需自己在 `[profile.release]` 开启

| 指标 | LTO 前 | LTO 后 | 变化 |
|------|-------|--------|------|
| Ring buffer TX | 385 MB/s | 652 MB/s | ↑69% |
| Ring buffer RX | 864 MB/s | 898 MB/s | ↑4% |
| RX latency P50 | 200 ns | <100 ns | ✅（低于计时器分辨率） |
| RX latency avg | 316 ns | 195 ns | ↓38% |
| e2e 1B avg | 143.7µs | 129.4µs | 不变（噪声） |

**中长期优化点（待探索）**：

| 编号 | 内容 | 预期收益 | 可移植性影响 | 实施难度 | 状态 |
|------|------|----------|-------------|----------|------|
| **O58** | Feature gate 条件编译（ArceOS 特化） | -15~25µs | ⚠️ 略有降低 | 中 | 🔍 探索中 |
| **O59** | 零拷贝 ring buffer（MaybeUninit） | -5~10µs | ✅ 无影响 | 高 | 🔍 探索中 |
| **O60** | DMA 集成（VisionFive2） | ~0µs | ❌ 硬件依赖 | 高 | ⏳ 等待硬件 |

**O58 Feature gate 条件编译详情**：

- **原理**：为 ArceOS 提供特化实现，绕过 trait 抽象
- **改动**：`uart_16550/Cargo.toml` 添加 `arceos` feature，`driver.rs` 添加 `#[cfg(feature = "arceos")]` 特化路径
- **收益**：消除 trait 调用 + 锁优化，-15~25µs
- **风险**：增加维护负担，降低可移植性
- **建议**：仅在短期优化不达标时考虑

**O59 零拷贝 ring buffer 详情**：

- **原理**：使用 `MaybeUninit` 避免初始化开销，直接操作硬件缓冲区
- **改动**：重构 `ring_buffer.rs` 为零拷贝设计
- **收益**：减少内存拷贝，-5~10µs
- **风险**：unsafe 代码增加，维护复杂度高
- **建议**：作为长期优化方向，需要详细安全分析

**O60 DMA 集成详情**：

- **原理**：使用 DMA 替代 CPU 拷贝，彻底消除软件开销
- **改动**：需要 VisionFive2 DMA 控制器驱动
- **收益**：接近零软件开销
- **风险**：硬件依赖，实现复杂
- **建议**：等待 VisionFive2 真板验证与 Q25 DMA 决策

#### Scenario: 评估中长期优化

- **WHEN** 短期优化（Q13.1）不达标（1B avg > 130µs）
- **THEN** MUST 优先考虑 O58（feature gate），其次 O59（零拷贝），最后 O60（DMA）
- **WHEN** 短期优化达标
- **THEN** 中长期优化可作为远期目标，不影响当前开发

#### Scenario: Feature gate 决策

- **WHEN** 考虑实施 O58（feature gate 条件编译）
- **THEN** MUST 评估：(1) 可移植性损失是否可接受，(2) 维护负担是否可控，(3) 性能收益是否显著
- **THEN** MUST 先创建 OpenSpec 变更，获得用户 approval 后才实施

---

<!-- tombstone: Q5 --> Archived 2026-07-02 — Q5 内核态性能优化段（原 L9-25）已归档至 `openspec/changes/archive/2026-07-02-ARC-202607021535/specs/optimization/spec.md` (ARC-202607021535)
<!-- tombstone: Q7 --> Archived 2026-07-02 — Q7 用户态性能修复段（原 L26-58）已归档至 ARC-202607021535
<!-- tombstone: Q8 --> Archived 2026-07-02 — Q8 驱动引擎打磨段（原 L60-104）已归档至 ARC-202607021535
<!-- tombstone: Q12 --> Archived 2026-07-02 — Q12 Embassy 路径 A 段（原 L296-312）已归档至 ARC-202607021535
<!-- tombstone: Q15 --> Archived 2026-07-02 — Q15 M0~M4 增量重融合段（原 L628-694）已归档至 ARC-202607021535
<!-- arc: ARC-202607021535 --> 5 条已归档 (2026-07-02) → ../changes/archive/2026-07-02-ARC-202607021535/proposal.md
<!-- arc: ARC-202607021648 --> 4 组 optimization 条目已归档/压缩 (2026-07-02) → ../changes/archive/2026-07-02-ARC-202607021648/proposal.md
<!-- arc: ARC-202607031929 --> O45/O46/O47 旧详细方案已归档 (2026-07-03) → ../changes/archive/2026-07-03-ARC-202607031929/proposal.md
<!-- arc: ARC-202607111510 --> O78/O79/O81 已归档/压缩 (2026-07-11) → ../changes/archive/2026-07-11-ARC-202607111510/proposal.md
