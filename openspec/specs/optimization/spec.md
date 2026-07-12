# Spec: optimization — 优化记录

## Purpose

汇总 StarryOS 异步串口项目各阶段（Q0~Q15 已完成；Q20~Q26 规划中）的性能优化条目，包含问题描述、当前影响、建议方案、优先级与状态。Q 编号对应 milestone，O 编号保留历史优化项身份。
## Requirements
### Requirement: Q20~Q26 后续优化 Roadmap — 已重排

Q19/Q19B/Q19C 已完成并归档。2026-07-12 起，后续 UART 工作 MUST 先推进 QEMU+D1 当前可验证项，再等待 multi-hart 真板。

**重排依据**: ADR-056、R15、L286。
**历史边界**: Q16~Q19C 已完成；Q19D、M3/rootfs-probe、O79/O81 取消当前规划。Storage/rootfs 需要新 change。
**硬件边界**: D1 是单 hart，可验证 UART 语义、延迟、抖动、CPU/counter 和 user ring 原型；不能证明 O63 multi-hart。

| Milestone | 目标 | 归属条目 | Gate |
|-----------|------|----------|------|
| **Q20** | Benchmark gap closure | O77 follow-up、ADR-056、R15、L286 | QEMU+D1 输出 latency、jitter、CPU/IRQ/copy 开销与 RX fixed payload 证据 |
| **Q21** | UART user completion queue MVP | ADR-056 | TX submit/completion queue 可用；read/write fallback 不退化 |
| **Q22** | User ring + zero-copy prototype | O1/O36、ADR-056 | `mmap` ring 在 QEMU+D1 可用，并说明减少的 user/kernel copy |
| **Q23** | Ring/completion performance decision | ADR-056 | 对比 write/tcdrain、no-drain、batch-drain、writev、user ring；决定保留、收窄或回滚 |
| **Q24** | VisionFive2 / multi-hart revalidation | O63/O64/O65/O66/O71/O38/O39 | 并发 read/write、flush/tcdrain、IER enable/disable 无数据丢失或 hang |
| **Q25** | DMA / 高波特率决策 | O3/O40/O69/O41 | 用 Q23/Q24 数据决定实施或拒绝 |
| **Q26** | 维护性清理 | O48/O49/O50、ADR-034 | memtrack、Manual mode、预留接口、release LTO 有结论 |

| 编号 | 当前结论 | 触发条件 |
|------|----------|----------|
| **O74/O75** | 平台 descriptor + early console 已落地 | 新平台适配时继续沿用 |
| **O77** | D1 P99 长尾根因未探明但不阻塞；Q20 补测 | 继续改 D1 TX copier、THRE wake、retry policy 时 |
| **O80** | Memory-root lazy COW SIGILL 是 loader/mm 后续，不是 UART gate | 需要恢复 lazy file-backed loader parity 时 |

<!-- tombstone: O76/O77 --> Archived 2026-07-02 in ARC-202607021648 — Q19/Q19B 已完成并归档，active roadmap 不再保留已完成 Lichee 条目。
<!-- tombstone: O78/O79/O81 --> Archived 2026-07-11 in ARC-202607111510 — Q19C memory-root path/command 已完成，Q19D/O79 与 M3/O81 取消当前规划。

#### Scenario: Roadmap-driven scheduling

- **WHEN** 新增或重排 Q15 后优化项
- **THEN** MUST 先判定该项属于文档收敛、QEMU correctness、真板观测、真板验证、数据驱动决策、维护清理还是远期实验
- **AND** MUST 放入对应 Q20~Q26 milestone，禁止只按 O 编号顺序排期

#### Scenario: QEMU-only work before hardware

- **WHEN** multi-hart 真板尚未到位
- **THEN** MUST 优先推进 Q20~Q23 中可在 QEMU 和 D1 上验证的工作
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
**未关闭**: 多 hart stress 未跑。Q24 必须复验并发 read/write、flush/tcdrain、IER enable/disable。

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

从 arceos（`/home/daivy/projects/serial/others/arceos/`，明扬异步化工作）已识别可借鉴的设计模式、踩坑教训、抽象机制。本节 MUST 集中登记真正需要新增工作的项；已等价实现的项标注 "✅ 已采纳"。

> 完整分析见 `.claude/analysis/arceos-borrowable-experience.md` `[ARCHIVED 2026-07-04 → _archive/2026-07-04-q19-lichee-analysis/]`。本节是该分析的优化待办部分。
>
> **背景**：StarryOS 脱胎于 arceos，明扬在 arceos 上做 DWMAC/网络/启动等模块的异步化推进，我们从其工作获取经验后应用到 StarryOS 异步串口后续阶段开发。

| ID | 来源 | 描述 | 优先级 | 触发条件 |
|----|------|------|--------|---------|
| **O64** | arceos ADR-004（PIT-007 / TIP-004） | **trust u-boot 模式（仅 PLIC + Clock）**：VisionFive2 启动时 U-Boot 已配置 PLIC 全局状态和 SoC 时钟树，OS 不应"重新初始化一切"。**不含 UART**：arceos starfive 的 UART 走 SBI console，无 MMIO init 先例；NS16550 寄存器（FCR/IER/波特率）重复设置无害（ADR-040 Revised）。arceos 反复失败 7+ 次后 commit `4334e41` "revolution" 锁定此决策。**Q20 真板观测必备**。 | 🔴 P0 | VisionFive2 硬件到位 |
| **O65** | arceos ADR-002（PIT-003） | **PLIC init_primary / init_percpu 防御性分离**：当前 StarryOS 使用的 axplat crates（0.3.1-pre.6 / 0.1.0-pre.2）已采用 `static PLIC: SpinNoIrq<Plic>`（编译时初始化）+ 幂等 `init_by_context()`，**当前代码安全**。旧 arceos 的 `LazyInit<Plic>` + `init_percpu()` 内调 `init_plic()` 反模式**不存在于当前代码中**，但作为防御性设计模式保留（ADR-041 Revised）。降至 P1。 | 🟡 P1（防御性） | Q24 平台切换时验证 |
| **O66** | arceos TIP-004 | **`print_preserved_status()` 验证函数**：UART / PLIC / Clock init 前后 dump 当前寄存器状态，与 U-Boot/Linux 预期对比。arceos `DwmacHalImpl::configure_platform` 实现此模式。**Q20 真板观测必备**（O64 的前置依赖）。 | 🔴 P0 | VisionFive2 硬件到位 |
| **O69** | arceos axdma + DwmacHal | **DMA 一致性内存抽象**：`DMAInfo { cpu_addr, bus_addr }` 二元组 + UNCACHED 映射 + cache_flush_range。**⏳ 与 O3/O40 合并**：JH7110 是否有外部 DMA 控制器未知，Q25 按 O3/O40 决策树走。如引入，**借鉴** axdma + DwmacHal cache_flush_range 模式。 | ⏳ Q21 决策 | Q23/Q24 数据 + O3 评估 |
| **O71** | arceos TIP-005 | **PAC 类型安全寄存器访问**：用 `jh7110_vf2_13b_pac` 而非 `write_volatile(magic_offset)`。编译期类型检查 + IDE 自动补全。**⏳ 待评估**：Q24 真板驱动开发时考虑引入，避免 magic offset。 | 🟡 P1 | Q24 真板驱动开发 |
| **O77** | Q19C D1 TX 诊断 | **D1 TX zero-send / P99 长尾优化（Q19C.8e 已完成）**：已实施 slow-poll（`TX_SLOW_POLL_LIMIT=4096` × `TX_SLOW_POLL_SPINS=256`）+ yield 重试（`TX_YIELD_RETRIES=4` 自唤醒）作为 budget exhausted 后的 fallback。真板三轮数据证明 slow-pool 100% 成功（`slow_poll_exh=0`），yield 重试从未触发（`yield_exh=0`），ISR 从未丢失。P99 长尾（size=256 P99=50.86ms，2.34x 线时）**根因未探明**——slow-pool/yield 重试均未改善，100 次迭代中约 1 次超出线时+10ms，确切成因待查。当前影响可接受（吞吐量 <2%），暂不继续优化，Q20 补测时再探明。`TX_FAST_RETRY_LIMIT=0` + drain `TX_WAKER` 已证伪，不得作为默认。 | ✅ 已完成（Q19C-M0） | P99 根因未探明，Q20 补测 |
| **O80** | Q19C-M1 loader/mm 后续 | **Memory-root lazy file-backed COW SIGILL**：`load_user_app()` 从 `/bin/benchmark` lazy mapping 可进程化并处理 page fault/syscall，但 main 前在合法 RV64C `c.ld` 地址 SIGILL；eager VFS read + segment mapping 已通过完整 benchmark，说明问题不在 async UART、benchmark ELF 或通用 syscall。后续应检查 `CachedFile` 页缓存、`FileBackend::Cached`、`Backend::new_cow`、tmpfs offset/权限/取指路径，不作为 Q19C-M1 或 UART gate。 | 🟡 P1 | 需要恢复 lazy file-backed loader parity 时 |
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

#### Scenario: Q21 评估 O69（DMA 决策树）

- **WHEN** Q23/Q24 数据完成后需要重新评估 DMA
- **THEN** MUST 按 O3/O40 决策树走：(1) JH7110 是否有 DMA 控制器 → (2) DMA 是否能访问 UART FIFO → (3) PIO+中断 vs DMA 开销对比 → (4) 是否需要更高波特率（O41）
- **AND** 如决定引入 DMA，**借鉴** arceos `axdma` 的 `DMAInfo` 二元组模式与 `DwmacHal::cache_flush_range` 处理

### Requirement: 远期优化（优先级低，不确定是否做）

远期优化条目 MUST 在评估 ROI 后决定是否实现；不作为里程碑硬性要求。

| 编号 | 内容 | 优先级 | 说明 |
|------|------|--------|------|
| **O1 / O36** | 零拷贝 RX | — | mmap ring buffer 到用户空间 |
| **O5** | 协程优先级调度 | — | 取决于 axtask 支持 |
| **O37** | kernel log TX 合并 | — | `ax_println!` 走 ring buffer |
| **O32** | poll_fn 闭包 | — | 编译器可能已优化 |

<!-- tombstone: O45 --> Archived in optimization/spec.md #O45 2026-06-16 — ✅ 已完成（2026-06-11 Q8），tcdrain 真异步化
<!-- tombstone: O46 --> Archived in optimization/spec.md #O46 2026-06-16 — ✅ 已完成（2026-06-11 Q8），AtomicWaker 推广 8 处
<!-- tombstone: O47 --> Archived in optimization/spec.md #O47 2026-06-16 — ✅ 已完成（2026-06-11 Q9），VTIME 超时机制

#### Scenario: 评估 O1/O36 零拷贝 RX

- **WHEN** StarryOS 演进到需要减少 RX 路径内存拷贝（如 Q21~Q23 ring/completion 路径）
- **THEN** MUST 评估 `mmap ring buffer 到用户空间` 的实现复杂度与安全边界
- **AND** 收益 MUST 量化（当前 RX 路径 5 次拷贝的减少数）
- **AND** 禁止在未评估前直接实施

### Requirement: 2026-06-11 死代码审计后续优化

本次审计发现的后续优化机会 MUST 在评估 ROI 后决定是否启用或彻底移除；死代码 SHALL 不长期保留。

| 编号 | 内容 | 优先级 | 说明 |
|------|------|--------|------|
| **O48** | memtrack 模块集成 | 🟢 低 | `kernel/src/pseudofs/dev/memtrack.rs` — 内存追踪功能已编写但从未集成（`run_memory_analysis` 无调用者）。Q22 维护性清理时评估；若 Q20/Q21 真板调试需要可提前启用 |
| **O49** | ProcessMode::Manual 移除 | 🟢 低 | `ldisc.rs:37` — Q7 后仅 External/None 模式被构造，Manual 变体可通过重构 match 分支移除（需更新 ldisc.rs:265 匹配） |
| **O50** | 预留接口评估 | 🟢 低 | `create_pty_master`（tty/mod.rs）、`DeviceMmap::ReadOnly`（device.rs）、`clear_elf_cache`/`cleanup_task_tables`（memtrack 引用链）— 当前用 `#[allow(dead_code)]` 标注，未来如有需求可恢复或彻底移除 |

#### Scenario: 评估 O48 memtrack 模块

- **WHEN** Q20/Q21 真板调试需要内存调试工具
- **THEN** 可恢复 `memtrack.rs` 的集成调用（当前代码完整，仅缺 `/dev/memtrack` 的设备注册）

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
- **建议**：等待 VisionFive2 真板验证与 Q21 DMA 决策

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
