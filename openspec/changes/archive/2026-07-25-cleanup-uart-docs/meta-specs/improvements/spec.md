# Spec: improvements — 改进记录

## Purpose

记录尚未承诺实施的改进机会。条目使用 `Ixx` 编号。已完成的条目保留 tombstones。对应 Legacy: `openspec/specs/optimization/spec.md` (hash: `2ffa3af2`)。

## Requirements

<!-- arc: ARC-202607251326 --> 4 条目已归档 (I01-I04, 2026-07-25) -> openspec/changes/ARC-202607251326/proposal.md

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

<!-- arc: ARC-202607251326 --> 4 条目已归档 (I07-I10, 2026-07-25) -> openspec/changes/ARC-202607251326/proposal.md

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
