# Spec: improvements — 改进记录

## Purpose

记录尚未承诺实施的改进机会。条目使用 `Ixx` 编号。已完成的条目保留 tombstones。对应 Legacy: `openspec/specs/optimization/spec.md` (hash: `2ffa3af2`)。

## Requirements

### Requirement: I01 — D1 TX 效率优化

D1 THRE IRQ/IIR no-pending 路径 MUST NOT 作为唯一进展来源；当前使用 bounded slow-poll fallback 保证 forward progress，效率优化 MUST 在 CPU/功耗需求明确时评估（`TX_FAST_RETRY_LIMIT=32` + `TX_SLOW_POLL_LIMIT=4096` × `TX_SLOW_POLL_SPINS=256` + bounded yield fallback）。功能与线速已达标，但以大量 CPU/MMIO polling 换取 forward progress（99.84% hw send 返回 0）。

**Legacy**: O77 | **状态**: 🟡 功能保留 / 效率待优化 | **触发**: 优先调查 D1 THRE/PLIC 时序；或出现 CPU/功耗需求

- **当前数据**: D1 fullbench 达物理线速约 95.3%-99.1%，退出码 0，`slow_poll_exh=0`/`yield_exh=0`。
- **优化方向**: IRQ-first + timer watchdog、D1 platform-gated fallback。
- **约束**: 不得回退为无软件 fallback 的纯 IRQ。

#### Scenario: 优化 D1 TX wake/retry

- **WHEN** 开发者继续优化 D1 TX copier、THRE wake 或 retry policy
- **THEN** MUST 先证明改动不丢失 forward progress
- **AND** SHOULD 优先比较 IRQ-first + timer watchdog 与现有 continuous slow-poll

### Requirement: I02 — user ring/completion 远期候选

现有路径已满足当前需求；user ring/completion 改进 MUST 在证明当前路径不足且有量化收益时才评估。

**Legacy**: O82 | **状态**: 🧊 远期候选

| 可借鉴项 | 当前处理 |
|---|---|
| completion 观测增强 | 保留 TxCompletion 全局 drain |
| backpressure 可观测性 | Q27a/Q27 已完成 |
| counter 分阶段细化 | 继续使用 S40 |
| 多 writer 公平性 | Q28 已收敛 API；MPSC ring 为 O85 远期候选 |

#### Scenario: 评估 O82 user ring/completion

- **WHEN** 开发者重新提出 UART completion queue、mmap user ring 或 zero-copy
- **THEN** MUST 先证明当前路径在目标硬件上不是线速瓶颈
- **AND** MUST 保留 /dev/console read/write fallback

### Requirement: I03 — MPSC ring / 多逻辑 writer 公平性

TX ring MUST 保持 SPSC；MPSC ring 改进 MUST 仅在 Q24 或新 workload 证明 producer serialization 不足时评估。

**Legacy**: O85 | **状态**: 🧊 工业化远期候选 | **触发**: Q24 或新 workload 证明现有串行化不足

#### Scenario: TX workload requires stronger multi-producer semantics

- **WHEN** Q24 或新 workload 观测到 syscall/message 边界破坏、producer 饥饿或吞吐不足
- **THEN** Q30 MUST 先声明目标是 atomicity、fairness、latency 还是 throughput
- **AND** MUST 比较 SPSC serialization、submission granularity、explicit scheduling 和 MPSC

### Requirement: I04 — syscall 原子性与跨 write 不交错

当前 accepted-prefix 契约 MUST 保持；syscall 原子性改进 MUST 仅在真实应用要求消息边界时评估。

**Legacy**: O86 | **状态**: 🧊 证据触发 | **触发**: 真实应用要求消息边界，或观测到饥饿、交互延迟

#### Scenario: 评估 producer 语义强化

- **WHEN** workload 要求消息边界或公平性
- **THEN** MUST 先证明当前 accepted-prefix 契约不足

### Requirement: I05 — O63 multi-hart stress

Q17 multi-hart stress MUST 在 Q24 完成：至少两个 hart 上复验跨 hart write/flush/tcdrain，覆盖 read 与 IER enable/disable。

**Legacy**: O63 | **状态**: ⚠️ QEMU gate 完成 / 多 hart 待验证 | **关联**: Q24

#### Scenario: Q17 QEMU 修复完成但多 hart 未实测

- **WHEN** Q17 通过 QEMU single-hart 验证
- **THEN** Q17 MAY 标记为 QEMU gate complete
- **AND** O63 cross-hart risk MUST 保持 open 直到 Q24 或等价 SMP stress 通过

### Requirement: I06 — ArceOS 借鉴清单

ArceOS 借鉴项 MUST 在对应硬件到位时按优先级评估和落地。

**Legacy**: O64-O66, O69, O71 | **状态**: 等待硬件触发

| ID | 内容 | 优先级 | 触发条件 |
|---|---|---|---|
| **O64** | trust-u-boot 仅用于 PLIC+Clock，UART 仍可重设 | 🔴 P0 | VisionFive2 硬件到位 |
| **O65** | PLIC primary/percpu 防御性分离验证 | 🟡 P1 | Q24 平台切换时 |
| **O66** | print_preserved_status() 验证函数 | 🔴 P0 | VisionFive2 硬件到位 |
| **O69** | DMA 一致性内存抽象（借鉴 axdma + DwmacHal） | ⏳ Q25 决策 | Q24 或新硬件数据 |
| **O71** | PAC 类型安全寄存器访问 | 🟡 P1 | Q24 真板驱动开发 |

#### Scenario: Q17-Q20 真板启动顺序

- **WHEN** VisionFive2 硬件到位
- **THEN** MUST 按顺序: Q17 O63 内存序 → Q18 平台解耦 → Q19 Lichee smoke → Q20 O66 状态验证 → O64 trust-u-boot → O65 PLIC 验证 → Q15 Manual QA 复跑

### Requirement: I07 — 已排除优化（不实施）

通用分发结构类优化 MUST NOT 在专用驱动场景下实施；embassy 替换现有实现 MUST 先证明三个条件全部满足。

**Legacy**: O17, OE1-OE5 | **状态**: ❌ 不实施

- **O17 中断分发效率**: ISR 使用 AtomicWaker 直接唤醒 O(1)，无需 BTreeMap。专用驱动场景禁止过度设计。
- **Embassy 误用**: Channel 替换 HeapRb（失去 lock-free SPSC）、Mutex 替换 SpinNoPreempt（与 axtask 冲突）、Watch 替换 AtomicBool（单 bool 过度设计）、Semaphore 计数 NAPI（错误工具语义）、select! 替换手动 poll（不兼容 axtask）。

#### Scenario: 评估 embassy 包装替换

- **WHEN** 开发者提议用 embassy 同步原语替换现有实现
- **THEN** MUST 先证明当前实现有可测问题，新方案更快/更简洁，且不与 axtask 冲突

### Requirement: I08 — 远期优化候选

| 编号 | 内容 | 优先级 | 状态 |
|---|---|---|---|
远期优化候选 MUST 在评估 ROI 后决定是否实现，不作为里程碑硬性要求：
| **O5** | 协程优先级调度 | 低 | 取决于 axtask 支持 |
| **O37** | kernel log TX 合并 | 低 | ax_println! 走 ring buffer |
| **O32** | poll_fn 闭包 | 低 | 编译器可能已优化 |
| **O54** | ISR 直接搬运（移除 copier 任务） | 中 | 需 O51 就位 + benchmark 验证 |
| **O55** | 半满/IDLE 唤醒策略 | 低 | 需 O51 或 O54 就位 |

**Legacy**: O1/O5/O32/O36/O37/O54/O55 | **状态**: 远期

#### Scenario: 评估远期优化 ROI

- **WHEN** 开发者考虑实现远期优化之一
- **THEN** MUST 评估实施成本 vs 性能收益

### Requirement: I09 — Q13 中长期优化待探索

| 编号 | 内容 | 预期收益 | 可移植性 | 状态 |
|---|---|---|---|---|
| **O58** | Feature gate 条件编译（ArceOS 特化） | -15~25µs | ⚠️ 降低 | 🔍 探索中 |
| **O59** | 零拷贝 ring buffer（MaybeUninit） | -5~10µs | ✅ 无影响 | 🔍 探索中 |
| **O60** | DMA 集成（VisionFive2） | ~0µs | ❌ 硬件依赖 | ⏳ 等待硬件 |

Q13 中长期优化点 MUST 在短期优化不达标时按优先级评估。

**Legacy**: O58-O60 | **状态**: 中长期待探索

#### Scenario: 评估中长期优化

- **WHEN** 短期优化不达标（1B avg > 130µs）
- **THEN** MUST 优先考虑 O58（feature gate），其次 O59（零拷贝），最后 O60（DMA）

### Requirement: I10 — Q26 维护性清理已归档

Q26 维护性清理 MUST 视为已完成并归档；后续类似清理 MUST 参照 Q26 的 Gate 分层（host/static vs 运行时）和 ENV BLOCK 处理方法。

**Legacy**: O48-O50, ADR-034 | **状态**: ✅ 已归档

| 条目 | 处理 |
|---|---|
| O48 memtrack | ✅ feature/API 修复 + 三态 session + 8 host tests |
| O49 ProcessMode::Manual | ✅ 删除 Manual 变体及关联分支 |
| O50 预留接口 | ✅ 删除 create_pty_master、DeviceMmap::ReadOnly |
| ADR-034 LTO | ✅ LTO=y Makefile 入口可用，开发默认关闭 |

#### Scenario: 参照 Q26 进行维护性清理

- **WHEN** 后续进行类似维护性清理
- **THEN** MUST 参照 Q26 的 Gate 分层（host/static vs 运行时）
- **AND** MUST 将无法在当前环境验证的 Gate 标记为 ENV BLOCK

### Requirement: I12 — UART benchmark 测量优化

UART benchmark MUST 区分提交、THR 接受和 TEMT 完成。QEMU 数据只用于软件路径和回归比较；D1 数据用于物理线速。CPU、内存和完整性 MUST 使用可复核的测量方法。

**状态**: 🟡 方法待补强 | **数据源**: `docs/benchmark-report-async.md`、历史提交 `24d926d` | **触发**: 下一轮 async UART 性能比较或架构决策

- **设备见证**: 每组输出设备路径和 `fstat` 设备号。UART TX、压力和完整性测试 MUST 写 `/dev/console`，不得以 `/dev/null` 数据代表串口。
- **时间口径**: 分开报告 write-only、enqueue、drain-each、batch-drain 和 final-drain。不同完成点 MUST NOT 计算性能倍率。
- **CPU 指标**: `cycle` 差值只报告 cycles、cycles/byte 或 cycles/call。CPU 使用率 MUST 由 task runtime/idle time 与 wall time 推导；QEMU host CPU 与 guest CPU 分开报告。
- **实时性指标**: 记录最大 IRQ-off cycles、timer/IRQ latency 和并发任务 P50/P99。
- **内存指标**: 分开报告 ring capacity、静态对象大小、heap 增量和峰值；无 TX/RX ring MUST NOT 写成总内存为 0。
- **完整性指标**: 检查 write 返回值、short write 和 drain 错误，并由接收端或 QEMU chardev capture 校验长度与 hash。
- **复现信息**: 记录 commit、构建参数、QEMU 命令、串口 backend、hart 数、rootfs、benchmark 版本和原始日志 hash。
- **RX 指标**: 保留空读 EAGAIN，并增加固定 payload 注入、超时、长度与内容校验。

#### Scenario: 采集 CPU 使用率

- **WHEN** 报告 Console 或 async UART 的 CPU 使用率
- **THEN** MUST 给出 busy/idle 分子、wall-time 分母和采样范围
- **AND** MUST 同时报告 cycles/byte 或 cycles/call
- **AND** MUST NOT 把 cycles/ns 标注为百分比

#### Scenario: 比较 main、polling Console 与 async UART

- **WHEN** 三种实现进入同一性能表
- **THEN** MUST 使用相同设备、payload、迭代数和 drain policy
- **AND** MUST 分开解释 QEMU 软件路径与 D1 物理线速

#### Scenario: 声明数据完整性或压力稳定性

- **WHEN** benchmark 声明 UART 数据完整或持续写入稳定
- **THEN** MUST 提供 `/dev/console` 写入、接收或 capture 校验和完成状态
- **AND** `/dev/null` 结果 MAY 作为 syscall/VFS 对照，但 MUST NOT 作为 UART 证据

<!-- arc: MIG-20260720-legacy-specs --> Legacy: openspec/specs/optimization/spec.md (hash: 2ffa3af2), 439 lines. Active improvements extracted as I01-I10; I11 removed (console-specific, archived to `console-lichee` branch); I12 added from async UART benchmark measurement evidence. Completed/archived entries preserved as tombstones. Archive carriers: ARC-202607021535, ARC-202607021648, ARC-202607031929, ARC-202607111510.
