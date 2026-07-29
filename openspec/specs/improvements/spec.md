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
| **O65** | PLIC primary/percpu 防御性分离验证 | 🟡 P1 | T16 或 T23 |
| **O66** | print_preserved_status() 验证函数 | 🔴 P0 | VisionFive2 硬件到位 |
| **O69** | DMA 一致性内存抽象（借鉴 axdma + DwmacHal） | ⏳ 真板决策 | T17 有硬件数据时 |
| **O71** | PAC 类型安全寄存器访问 | 🟡 P1 | T14-T21 真板驱动开发 |

#### Scenario: 真板 bring-up 按需触发

- **WHEN** VisionFive2 硬件到位或 NIC 进入 T13-T24
- **THEN** MUST 按表中 milestone 逐项评估 O64-O66、O69 和 O71

### Requirement: I13 — PCI feature 与运行矩阵

I13 提升前 MUST 先证明纯 PCI 驱动可构建。当前两处配置会同时启用两种 bus，`axdriver` 最终选择 MMIO。

**证据**: K32 | **状态**: 待评估，未承诺

- **影响**: `BUS=pci` 可能只改变 QEMU 设备，不改变内核 probe。
- **建议**: 使 bus feature 互斥，并增加 MMIO/PCI 构建与启动矩阵。
- **通过条件**: PCI net/block 可探测，IRQ、RX 和 TX 有独立见证。

#### Scenario: 需要 PCI 兼容性

- **WHEN** 硬件或测试明确要求 VirtIO PCI
- **THEN** SHOULD 将 I13 提升为独立 change
- **AND** MUST NOT 与 MMIO 异步主线混在同一 milestone

### Requirement: I14 — QEMU 串口与网络观测分离

I14 提升时 MUST 分离串口与网络见证。当前 `-nographic` 会混合串口、monitor 和 ANSI 输出。固定 5555 也可能冲突。

**证据**: K31；`make/qemu.mk:51-55,75-80` | **状态**: 待评估，未承诺

- **影响**: 自动化日志和端口失败容易被误判。
- **建议**: 支持独立 serial log、关闭 monitor、可配置 host port。
- **通过条件**: 串口见证和网络见证可独立采集，端口冲突有明确错误。

#### Scenario: 建立自动化 QEMU 网络回归

- **WHEN** 手工终端测试转为自动化回归
- **THEN** SHOULD 评估 I14

### Requirement: I15 — QEMU 根文件系统覆盖层

I15 提升时 MUST 保证原始 rootfs 不变。当前 QEMU 挂载 `make/disk.img`，诊断过程可能修改共享镜像。

**证据**: 2026-07-27 显式 `/tmp` qcow2 backing overlay 可启动 | **状态**: 待评估，未承诺

- **影响**: 重复测试可能受上次运行残留影响。
- **建议**: 为测试模式生成临时 qcow2 覆盖层。
- **通过条件**: 原始镜像哈希不变，测试结束后覆盖层可定位或清理。

#### Scenario: 运行可重复 QEMU Gate

- **WHEN** Gate 会写入 guest 文件系统
- **THEN** SHOULD 使用一次性覆盖层隔离运行

<!-- arc: ARC-202607251326 --> 4 条目已归档 (I07-I10, 2026-07-25) -> openspec/changes/archive/2026-07-25-arc-202607251326/proposal.md

<!-- arc: cleanup-uart-documentation-system --> I12 (UART benchmark measurement) archived 2026-07-25. Universal measurement rules migrated to quality-gate-baseline/spec.md:Benchmark measurement methodology.

<!-- arc: MIG-20260720-legacy-specs --> Legacy: openspec/specs/optimization/spec.md (hash: 2ffa3af2), 439 lines. Active improvements extracted as I01-I10; I11 removed (console-specific, archived to `console-lichee` branch); I12 added from async UART benchmark measurement evidence. Completed/archived entries preserved as tombstones. Archive carriers: ARC-202607021535, ARC-202607021648, ARC-202607031929, ARC-202607111510.
