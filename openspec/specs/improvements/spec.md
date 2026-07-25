# Spec: improvements — 改进记录

## Purpose

记录尚未承诺实施的改进机会。条目使用 `Ixx` 编号。已完成的条目保留 tombstones。对应 Legacy: `openspec/specs/optimization/spec.md` (hash: `2ffa3af2`)。

## Requirements

<!-- arc: ARC-202607251326 --> 4 条目已归档 (I01-I04, 2026-07-25) -> openspec/changes/archive/2026-07-25-arc-202607251326/proposal.md
<!-- arc: cleanup-uart-documentation-system --> I05, I12 archived (2026-07-25). I12 universal rules migrated to quality-gate-baseline -> openspec/changes/archive/2026-07-25-cleanup-uart-docs/

<!-- arc: cleanup-uart-documentation-system --> I05 (O63 multi-hart stress) archived 2026-07-25, deferred with q17-smp-memory-ordering. Cross-hart risk remains open until VF2 multi-hart or N3 SMP/multiqueue verification.

### Requirement: I06 — ArceOS 借鉴清单

ArceOS 借鉴项 MUST 在对应硬件到位时按优先级评估和落地。

**Legacy**: O64-O66, O69, O71 | **状态**: 等待硬件触发

| ID | 内容 | 优先级 | 触发条件 |
|---|---|---|---|
| **O64** | trust-u-boot 仅用于 PLIC+Clock，UART 仍可重设 | 🔴 P0 | VisionFive2 硬件到位 |
| **O65** | PLIC primary/percpu 防御性分离验证 | 🟡 P1 | N3 SMP/multiqueue 或 VF2 平台切换时 |
| **O66** | print_preserved_status() 验证函数 | 🔴 P0 | VisionFive2 硬件到位 |
| **O69** | DMA 一致性内存抽象（借鉴 axdma + DwmacHal） | ⏳ N4 决策 | N4 DWMAC 真板或新硬件数据 |
| **O71** | PAC 类型安全寄存器访问 | 🟡 P1 | N4 真板驱动开发 |

#### Scenario: 真板 bring-up 按需触发

- **WHEN** VisionFive2 硬件到位或 NIC N3/N4 进入 SMP/真板验证阶段
- **THEN** MUST 按触发条件逐项评估：O64/O66（PLIC+Clock trust-u-boot 验证）、O65（PLIC 防御性分离）、O69（DMA 一致性，N4 DWMAC 真板时触发）、O71（PAC 类型安全寄存器，N4 真板驱动开发时触发）

<!-- arc: ARC-202607251326 --> 4 条目已归档 (I07-I10, 2026-07-25) -> openspec/changes/archive/2026-07-25-arc-202607251326/proposal.md

<!-- arc: cleanup-uart-documentation-system --> I12 (UART benchmark measurement) archived 2026-07-25. Universal measurement rules migrated to quality-gate-baseline/spec.md:Benchmark measurement methodology.

<!-- arc: MIG-20260720-legacy-specs --> Legacy: openspec/specs/optimization/spec.md (hash: 2ffa3af2), 439 lines. Active improvements extracted as I01-I10; I11 removed (console-specific, archived to `console-lichee` branch); I12 added from async UART benchmark measurement evidence. Completed/archived entries preserved as tombstones. Archive carriers: ARC-202607021535, ARC-202607021648, ARC-202607031929, ARC-202607111510.
