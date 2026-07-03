# Spec: optimization — 优化记录

## Purpose

汇总 StarryOS 异步串口项目各阶段（Q0~Q15 已完成；Q16~Q23 规划中）的性能优化条目，包含问题描述、当前影响、建议方案、优先级与状态。Q 编号对应 milestone，O 编号保留历史优化项身份。
## Requirements
### Requirement: Q16~Q23 后续优化 Roadmap — 已重排

Q15 后续优化 MUST 按 Gate 类型拆分为 Q16~Q23，禁止继续把 O63/O64/O66/O38/O39/O3/O40/O41/O48/O49/O50 等不同触发条件的条目塞进单一 Q6。O 编号保留历史身份，Q 编号代表当前执行 milestone。

> 2026-06-28 二次重排：基于 `.claude/analysis/platform-parameter-decoupling.md` 与 `.claude/analysis/lichee-rv-dock-adaptation-plan.md`，在真板验证前新增 Q18 平台参数解耦和 Q19 Lichee RV Dock early smoke test。原 VisionFive2 / DMA / 维护 / 远期池顺延。
> 2026-06-29 更新：Q19 / O76 已在 Lichee RV Dock 真板完成，串口输出 `[starry-d1] smoke complete, halting.`。
> 2026-06-29 更新：Q19B / O77 已在 Lichee RV Dock 真板完成 async UART userbench，大包 TX 达 97.7%~99.0% 115200bps 线速。
> 2026-07-03 更新：Q17 / O63 已完成 QEMU 修复与回归验证；多 hart / 真板 SMP stress 尚未执行，不能声明跨 hart 内存序已被实测证明。

| Milestone | 目标 | 归属条目 | Gate |
|-----------|------|----------|------|
| **Q16** | Roadmap / spec rebaseline | 文档任务重排、stale spec 标注、validate 已知噪音记录 | tasks / SNAPSHOT / optimization 与分析文档一致；`openspec validate --specs` 的已知 parser 噪音不阻塞后续开发 |
| **Q17** | SMP / 内存序正确性 | **O63** | ✅ QEMU 修复完成；cargo check + QEMU benchmark 无明显退化；⚠️ 真板/多 hart SMP stress 待验证 |
| **Q18** | 平台参数解耦 / early console 基础 | **O74**, **O75** | QEMU 行为保持；`uart_init.rs` 不再新增板级 base/irq/stride/width 常量 |
| **Q19** | Lichee RV Dock early smoke test | **O76**, L213-L216, L231-L235, ADR-043 | ✅ Lichee 串口输出 `[starry-d1] smoke complete, halting.` |
| **Q19B** | Lichee RV Dock async UART benchmark | **O77**, ADR-047~ADR-051, L236-L258 | ✅ kbench/userbench 均在真板完成；`/dev/console`、TTY、`tcdrain`、FIONBIO 全链路通过 |
| **Q20** | VisionFive2 UART 验证 | **O66**, **O64**, **O65**, **O71**, **O38**, **O39**, Q15 Manual QA 真板复跑 | VisionFive2 串口稳定运行，真板基线数据落档 |
| **Q21** | DMA / 高波特率决策 | **O3**, **O40**, **O69**, **O41** | 用 Q20 真板数据决定实施或拒绝 |
| **Q22** | 维护性清理 | **O48**, **O49**, **O50**, ADR-034 release LTO | 维护性债务有明确处理结论 |
| **Q23** | 远期预研池 | **O1/O36**, **O54/O55**, **O58/O59**, **O37** | 仅在 Q20/Q21 数据证明当前路径不足时启动 |

| 新增编号 | 内容 | 优先级 | 说明 |
|----------|------|--------|------|
| **O74** | Platform descriptor 集中化 | 🔴 P0 | 抽出 QEMU/Lichee/VisionFive2 的 UART kind、base、irq、stride、MMIO access width、boot strategy；落实 ADR-044 |
| **O75** | Early console 分层 | 🔴 P0 | 新增不依赖 IRQ / async task / rootfs 的 polling early console；QEMU 用 NS16550 U8，Lichee/VF2 用 DW APB U32 |
<!-- tombstone: O76/O77 --> Archived 2026-07-02 in ARC-202607021648 — Q19/Q19B 已完成并归档，active roadmap 不再保留已完成 Lichee 条目。

#### Scenario: Roadmap-driven scheduling

- **WHEN** 新增或重排 Q15 后优化项
- **THEN** MUST 先判定该项属于文档收敛、QEMU correctness、真板观测、真板验证、数据驱动决策、维护清理还是远期实验
- **AND** MUST 放入对应 Q16~Q23 milestone，禁止只按 O 编号顺序排期

#### Scenario: QEMU-only work before hardware

- **WHEN** VisionFive2 硬件尚未到位
- **THEN** MUST 优先推进 Q16、Q17 和 Q18 中可在 QEMU 上验证的工作
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

#### Scenario: 真板启动失败或串口无输出

- **WHEN** VisionFive2 上 UART 无输出 / 数据乱码
- **THEN** MUST 优先排查 O38（时钟配置）而非波特率或软件路径

#### O63 风险评估：QEMU 掩盖的内存序问题（2026-06-26 bg_32a087c3 逐行代码验证）

QEMU 模拟单 hart（当前 `.axconfig.toml` `max-cpu-num = 1`），`Relaxed` 内存序足够。VisionFive2 是 4 核 RISC-V，tasks 和 ISR 可能跨 hart 并发。Q15 手动选择的 `Relaxed` 用法在多核下会导致 flush 看到陈旧值、staged_bytes 计数漂移、IER 位丢失。

**2026-06-26 代码验证关键发现**：
- `kspin` 当前编译 **未启用 `smp` feature**（features = `["default"]`），`SpinNoIrq` 是空操作
- `critical_section` 仅 `disable_irqs()`（本地中断），不提供 SMP 互斥
- 3 处 cross-hart Relaxed 经逐行验证确认为 real bug

**影响位置**（Q15 现状，2026-06-26 代码审查确认）：

| 位置 | 当前 | 风险等级 | 升级方案 |
|---|---|---|---|
| `uart_init.rs:106, 109` `ier_cache` 读写 | Relaxed load/store **在 SpinNoIrq 锁外面** | 🔴 **Q17 P0 — RMW 竞争**：ISR (hart 0) 和 copier (hart N) 并发调用 `update_ier()` 时，load-modify-store 可互相覆盖，导致中断使能位丢失，**RX 或 TX 彻底停滞** | 搬进 `SpinNoIrq::lock()` 内做 RMW，或改用 `AtomicU8::fetch_or`/`fetch_and` 配合 `AcqRel` |
| `driver.rs:121-123, 274-275` `tx_copier_active` | Relaxed store（TX copier）→ Relaxed load（`tx_completion()` 被 user task 调用） | 🟡 **P1 — 状态陈旧**：flush/tcdrain 可能看到 copier 仍 active 的陈旧值，hang；或看到 inactive 的陈旧值，提前返回 | store → `Release`；load → `Acquire` |
| `driver.rs:282-283, 299-300, 338-339` `tx_staged_bytes` fetch_add/sub | Relaxed RMW → Relaxed load（`tx_completion()`） | 🟡 **P1 — 计数漂移**：`is_drained()` 可能看到陈旧 count，误判 drain 完成 | fetch_add/sub → `AcqRel`；load → `Acquire` |
| `driver.rs:272, 305, 311` telemetry 计数 | Relaxed（单写者，纯诊断） | ✅ 安全（可保留 Relaxed） | 无需修改 |

**升级原则**：
- 写端 store：Relaxed → Release（保证之前写入对 Acquire 可见）
- 读端 load：Relaxed → Acquire（保证后续读看到 Release 之前的内容）
- RMW 操作（fetch_add/sub）：Relaxed → AcqRel
- 单 hart 内访问的纯诊断字段：可保留 Relaxed

**RISC-V 性能影响**：`fence r,rw` / `fence rw,w` 几条指令，热点路径增加几 ns。Q15 的 134 µs 1B e2e 基线不会被显著影响（噪声范围内）。

**关联模块**（已正确使用 AcqRel，无需修改）：
- `embassy_sync::AtomicWaker`（RX_WAKER / TX_WAKER / DRAIN_WAKER）
- `embassy_hal_internal::atomic_ring_buffer`（RingBufRx / RingBufTx SPSC 同步）

**2026-07-03 Q17 收尾状态**：
- ✅ QEMU `ArceOsUartPort::update_ier()` 已将 `ier_cache` RMW 与 `set_ier()` 放入同一个 `SpinNoIrq` 临界区。
- ✅ D1 `ArceOsD1UartPort::update_ier()` 已将 cache RMW 与 MMIO IER 写入放入 IRQ-off 临界区，软件 wake 放在 IRQ 恢复后执行。
- ✅ `tx_copier_active` 已升级为 Release store / Acquire load；`tx_staged_bytes` 已升级为 AcqRel RMW / Acquire load。
- ✅ QEMU rootfs benchmark 已通过：64B TX 159.25 KB/s，1B latency avg 0.177ms，FIFO boundary matrix 无 10ms 台阶，FIONBIO 双入口 PASS。
- ⚠️ 当前验证仍是 QEMU 单 hart功能/性能回归，尚未覆盖 VisionFive2 或等价多 hart stress。后续 Q20 必须复验并发 UART read/write、flush/tcdrain 与 IER enable/disable，才能关闭 O63 的跨 hart 实测风险。

#### Scenario: 真板多核下出现数据丢失或 hang

- **WHEN** VisionFive2 上跑 stress test（多核并发读写 UART / flush）
- **THEN** MUST 优先排查 O63（内存序），检查 hot path 字段是否漏升级 Relaxed
- **AND** 症状：staged_bytes 漂移、flush 过早返回、tcdrain 不返回、偶发 panic

#### Scenario: 评估 O63 实施范围

- **WHEN** 准备 O63 实施
- **THEN** MUST 全局 grep `Ordering::Relaxed`，逐个评估：是否跨 hart 访问？写端还是读端？是否 RMW？
- **AND** 优先处理 hot path（`tx_copier_loop` / `rx_copier_loop` / `flush`）字段
- **AND** 同步在 `learned/spec.md` 追加"L{编号} 内存序选型"条目，记录具体选择依据

#### Scenario: O63 实施完成后验证

- **WHEN** O63 全部升级完成
- **THEN** MUST 跑 QEMU 回归 benchmark 确认无性能退化（< 5% noise 范围）
- **AND** MUST 在真板或等价多 hart 环境上跑 SMP stress test 验证无数据丢失
- **AND** 失败时定位到具体字段的内存序选择，逐个调试

#### Scenario: Q17 QEMU 修复完成但多 hart 未实测

- **WHEN** Q17 在 QEMU 单 hart 上通过 cargo check、Shell 和 `/bin/benchmark`
- **THEN** MAY 将 Q17 标记为 QEMU gate complete
- **AND** MUST NOT 宣称 O63 跨 hart 风险已完全关闭
- **AND** MUST 保留 Q20/VisionFive2 或等价多 hart stress 复验项

### Requirement: ArceOS 借鉴清单（从明扬 arceos 异步化工作获取经验）

从 arceos（`/home/daivy/projects/serial/others/arceos/`，明扬异步化工作）已识别可借鉴的设计模式、踩坑教训、抽象机制。本节 MUST 集中登记真正需要新增工作的项；已等价实现的项标注 "✅ 已采纳"。

> 完整分析见 `.claude/analysis/arceos-borrowable-experience.md`。本节是该分析的优化待办部分。
>
> **背景**：StarryOS 脱胎于 arceos，明扬在 arceos 上做 DWMAC/网络/启动等模块的异步化推进，我们从其工作获取经验后应用到 StarryOS 异步串口后续阶段开发。

| ID | 来源 | 描述 | 优先级 | 触发条件 |
|----|------|------|--------|---------|
| **O64** | arceos ADR-004（PIT-007 / TIP-004） | **trust u-boot 模式（仅 PLIC + Clock）**：VisionFive2 启动时 U-Boot 已配置 PLIC 全局状态和 SoC 时钟树，OS 不应"重新初始化一切"。**不含 UART**：arceos starfive 的 UART 走 SBI console，无 MMIO init 先例；NS16550 寄存器（FCR/IER/波特率）重复设置无害（ADR-040 Revised）。arceos 反复失败 7+ 次后 commit `4334e41` "revolution" 锁定此决策。**Q20 真板观测必备**。 | 🔴 P0 | VisionFive2 硬件到位 |
| **O65** | arceos ADR-002（PIT-003） | **PLIC init_primary / init_percpu 防御性分离**：当前 StarryOS 使用的 axplat crates（0.3.1-pre.6 / 0.1.0-pre.2）已采用 `static PLIC: SpinNoIrq<Plic>`（编译时初始化）+ 幂等 `init_by_context()`，**当前代码安全**。旧 arceos 的 `LazyInit<Plic>` + `init_percpu()` 内调 `init_plic()` 反模式**不存在于当前代码中**，但作为防御性设计模式保留（ADR-041 Revised）。降至 P1。 | 🟡 P1（防御性） | Q20 平台切换时验证 |
| **O66** | arceos TIP-004 | **`print_preserved_status()` 验证函数**：UART / PLIC / Clock init 前后 dump 当前寄存器状态，与 U-Boot/Linux 预期对比。arceos `DwmacHalImpl::configure_platform` 实现此模式。**Q20 真板观测必备**（O64 的前置依赖）。 | 🔴 P0 | VisionFive2 硬件到位 |
| **O69** | arceos axdma + DwmacHal | **DMA 一致性内存抽象**：`DMAInfo { cpu_addr, bus_addr }` 二元组 + UNCACHED 映射 + cache_flush_range。**⏳ 与 O3/O40 合并**：JH7110 是否有外部 DMA 控制器未知，Q21 按 O3/O40 决策树走。如引入，**借鉴** axdma + DwmacHal cache_flush_range 模式。 | ⏳ Q21 决策 | Q20 真板数据 + O3 评估 |
| **O71** | arceos TIP-005 | **PAC 类型安全寄存器访问**：用 `jh7110_vf2_13b_pac` 而非 `write_volatile(magic_offset)`。编译期类型检查 + IDE 自动补全。**⏳ 待评估**：Q20 真板驱动开发时考虑引入，避免 magic offset。 | 🟡 P1 | Q20 真板驱动开发 |
<!-- tombstone: O67/O68/O70/O72/O73 --> Archived 2026-07-02 in ARC-202607021648 — 已采纳/已蕴含/已领先项从 active optimization 清单移除。

#### Scenario: Q17-Q20 真板启动顺序（O63 + O74/O75/O76 + O64/O66 协同，Revised 2026-06-28）

- **WHEN** VisionFive2 硬件到位启动真板验证
- **THEN** MUST 按顺序实施：(1) Q17 / O63 内存序修复（P0 — 先修 `ier_cache` RMW 竞争，再修 `tx_copier_active`/`tx_staged_bytes`）→ (2) Q18 / O74-O75 平台参数解耦与 early console 基础 → (3) Q19 / O76 Lichee RV Dock 单核 smoke test 演练启动链 → (4) Q20 / O66 `print_preserved_status()` 验证 U-Boot 已配置 PLIC/Clock 状态 → (5) Q20 / O64 PLIC+Clock trust-u-boot 模式（**不限制 UART 初始化**）→ (6) Q20 / O65 验证 axplat crate PLIC 初始化路径 → (7) Q20 跑通 Q15 Manual QA 全部 12 项
- **AND** MUST 失败时优先排查 O63（内存序），其症状（staged_bytes 漂移 / flush hang / RX 停滞）最难定位
- **AND** UART 可正常重新初始化 FCR/IER/波特率，无需 trust-u-boot

#### Scenario: Q21 评估 O69（DMA 决策树）

- **WHEN** Q20 真板验证完成后需要重新评估 DMA
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

- **WHEN** StarryOS 演进到需要减少 RX 路径内存拷贝（如 Q20/Q21 真板高速场景）
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

**O45 — tcdrain 真异步化 详细方案**：

当前 TCSBRK 实现（`ctl.rs:43-57`）：

```rust
block_on(poll_fn(|cx| {
    if DRIVER.tx.lock().is_empty()
        && uart.lsr().contains(LSR::TRANSMITTER_EMPTY) {
        return Ready(Ok(0));
    }
    cx.waker().wake_by_ref();  // ← 协作自旋，每次失败立即重调度
    Pending
}))
```

**问题**：`wake_by_ref()` + `Pending` 产生协作式自旋。64 字节数据需要 TX copier 发送 4 批（每批 16 字节 FIFO），tcdrain 每次检查 ring buffer 非空 → 重调度 → copier 发一批 → 重调度 → ... 共 9 次任务切换（~270 µs QEMU）。

**优化方案**：用 PollSet 注册替代自旋。

```rust
block_on(poll_fn(|cx| {
    let mut tx = DRIVER.tx.lock();
    if tx.is_empty() {
        drop(tx);
        if uart.lsr().contains(LSR::TRANSMITTER_EMPTY) { return Ready(Ok(0)); }
        TX_WAKER.register(cx.waker());  // UART 还在发 → 等 TX ISR 唤醒
    } else {
        tx.poll.register(cx.waker());   // ring buf 有数据 → 等 copier pop 唤醒
    }
    Pending
}))
```

**关键**：`RingBufTx::pop()` 已调用 `self.poll.wake()`（`ring_buffer.rs:48`）。只需在 TCSBRK 中注册到 `tx.poll`，copier 每清空一批数据就会唤醒 tcdrain。

**预期效果**：

- QEMU：切换次数从 9 降至 ~4，延迟从 ~300 µs 降至 ~130 µs
- 真板：9 µs → 4 µs（可忽略，但更优雅）

**注意**：TX_WAKER 是 AtomicWaker（单槽），TX copier 也注册在上面。tcdrain 注册会覆盖 copier。需添加独立的 drain PollSet 或改用定时器补偿。

**O46 — AtomicWaker 模式推广 详细方案**：

**现状**（2026-06-05 评估）：

| 驱动 | 当前唤醒机制 | ISR 复杂度 | 唤醒延迟 |
|------|------------|-----------|----------|
| `kernel/src/drivers/isr.rs` (UART) | `static AtomicWaker` × 3（RX/TX/DRAIN） | O(1)，~1.5 µs | ~50 ns |
| `kernel/src/file/pipe.rs:34-56` | `Arc<PollSet>` × 3（rx/tx/close） | 通用 API | ~200 ns |
| `kernel/src/file/signalfd.rs:85-93` | `Arc<PollSet>` | 通用 API | ~200 ns |
| `kernel/src/file/pidfd.rs:20` | `Arc<PollSet>` | 通用 API | ~200 ns |

**优化方案**：将 pipe / signalfd / pidfd 改造成与 UART 一致的 AtomicWaker 静态分发模式。

- `pipe.rs`：在 ISR 端（写者唤醒 rx、读者唤醒 tx、close 唤醒 close）增加 `static ATOMIC_WAKER_PIPE_{RX,TX,CLOSE}`，删除 `PollSet` 字段
- `signalfd.rs`：增加 `static SIGNAL_WAKER`，信号到达时 `wake()`
- `pidfd.rs`：增加 `static EXIT_WAKER`，进程退出时 `wake()`

**预期收益**：

- 唤醒延迟：~200 ns → ~50 ns（×3 文件 = 6 个唤醒点）
- 内存：~1 KB PollSet → 24 B × N（按 waker 数）
- 代码量：减少 ~30 行（PollSet 注册样板）
- 一致性：所有驱动统一 ISR 唤醒模式，code review 更简单

**风险评估**：

- ⚠️ 唤醒方变静态，需在 spawn 时绑定（pipe.rs 已是 spawn 模型，零影响）
- ⚠️ pipe 的 close 路径需要信号源在 file drop 时唤醒（无 ISR），但可用 `static` 即可

**优先级**：🟡 中，量化收益明确（~150ns × 6 唤醒点 + 一致性提升），但需逐文件验证

**O47 — 超时机制 详细方案**：

> ⚠️ **2026-06-11 更新**：以下方案描述的是最初计划的 embassy-time 路径，但 Q9 实际采用了更简单的方案——复用 `axtask::future::timeout()`（无需新依赖）。以下原方案归档保留供参考。

<details>
<summary>原 embassy-time 方案（未实施）</summary>

**现状问题**：

`axtask::future::block_on(poll_io(...))` 是**永久阻塞**的，调用者无 timeout 能力：

```rust
// kernel/src/file/pipe.rs:123
block_on(poll_io(self, IoEvents::IN, self.nonblocking(), || {
    self.poll_rx.poll_io(cx, ...);
    // 无 timeout 选项，无数据时永远 Pending
}))
```

**潜在影响**：

- 用户态 read() 卡死后只能 SIGKILL，无法 SIGALRM 解除（无 setitimer 集成）
- DMA 失败时硬件可能永不唤醒（Q21 真板 O3/O40 风险）
- 用户态 poll() + SO_RCVTIMEO 需要内核支持 time 抽象

**优化方案**：

1. 引入 `embassy-time = "0.3"`（仅 Timer，不引入 Executor）
2. 在 axhal 实现 time driver 桩（依赖 axhal::time::current_ticks）
3. 改造 `poll_io` 接受 `Option<Duration>` 超时参数
4. 用 `embassy_futures::select!` 组合 poll_io + Timer

**实施示例**：

```rust
use embassy_time::{Timer, Duration};

block_on(async {
    let res = embassy_futures::select::select(
        poll_io_future,
        Timer::after(Duration::from_millis(100)),
    ).await;
    match res {
        embassy_futures::select::Either::First(r) => r,
        embassy_futures::select::Either::Second(_) => Err(EAGAIN),
    }
})
```

**风险评估**：

- 🔴 高：embassy-time 需要 time driver，必须在 axhal 适配 axtask 时钟
- 🟡 中：与现有 `axtask::future::block_on` 并存，引入两套 future 抽象
- 🟢 低：仅在用户态显式传递 timeout 时启用，向后兼容

**前置依赖**：

- Q21 DMA 决策完成（确认 DMA 失败路径是否真需要 timeout）
- axhal time driver 评估

**优先级**：🟡 中，Q20 触发条件性实现

</details>

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
