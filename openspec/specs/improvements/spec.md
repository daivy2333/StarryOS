# Spec: improvements — 改进记录

## Purpose

记录尚未承诺实施的改进机会。条目使用 `Ixx` 编号。已完成的条目保留 tombstones。Legacy 原文：`openspec/changes/archive/mig-20260720-legacy-specs/optimization-original.md`（hash: `2ffa3af2`）。

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

### Requirement: I16 — 网卡性能矩阵基础设施补全

声明完整 N00-N46 基线前 MUST 补全当前 portable workload 和采集器不能表达的测试能力。该工作尚未承诺，不阻止复用 R47 的矩阵设计和 R49 的现有 qualification procedure。

**证据**: R47、R49；`tests/network_benchmark.c` CLI 与 Schema v1 输出审计
**状态**: 待评估，未承诺

- **负载**: TCP/UDP RTT、UDP 间隔误差、exact burst、负载下延迟、动态 overload/recovery、connect churn。
- **边界**: 4096-65536 B TCP records、packet count、socket buffer、ARP、UDP metadata 和 fill-to-EAGAIN 控制。
- **指标**: 单流公平性、EAGAIN 等待与恢复、round instret delta、benchmark IRQ snapshot、timer wake overshoot、copy/allocation/wake/descriptor telemetry。
- **速率语义**: `--offered-load` 需要从固定 1 Gbit/s 名义值改为同环境 zero-loss pilot 的比例。
- **不包含**: TAP 六方向、多流、当前 payload 上限内阶梯、quick/standard、host CPU/RSS 和 pcap。这些已有入口，只是 MS16 未运行。

#### Scenario: 准备完整 polling B0 或 async A/B

- **WHEN** 项目需要声明 R47 N00-N46 必测项完整或生成对应性能比较
- **THEN** SHOULD 将 I16 中所需能力提升为独立 change
- **AND** MUST 先区分新增测试设施和修复被测网卡

### Requirement: I17 — 异步网卡从 axnet 抽离为独立库

I17 的任何实施 MUST 通过独立 change 完成，并遵守下述触发条件、前置依赖与范围边界。

将当前 `crates/axnet` 中的异步数据面（queue task、stack runner、TX/RX slot、generation/owner ledger）从 StarryOS 内核工作区抽离为独立 crate。当前 `axnet` 已从 root workspace 排除（`crates/axnet/Cargo.toml:155-158`）但仍直接依赖 `axdriver` / `axhal` / `axtask` / `axsync` / `axpoll` 六个 ArceOS 内部 crate；本 I 的目标是把这些依赖收拢到 OS 适配层，库本体只保留协议栈 + 异步契约 + 数据面。

**状态**: 待评估，未承诺
**触发条件**: MS08（QEMU 多 hart 正确性基线）accepted
**前置依赖**: MS06（resident stack runner + socket readiness）accepted 且唯一 spawn seam 稳定；MS07（单 hart 恢复语义）accepted
**关联约束**: M36（异步 NIC 分层架构）、M41（NIC transport 与证据边界）

#### Scope

抽离工作的 6 个 OS 依赖归属：

| 当前依赖 | 抽离后归属 | trait 候选 |
|---|---|---|
| `axdriver::AxDeviceContainer<AxNetDevice>` | device container adapter | 保留 `axdriver` 依赖作为 adapter，库内部转 `axdriver_net::NetDriverOps` |
| `axhal::time::wall_time_nanos` | OS 适配层 | `OsTime::now_nanos()` |
| `axtask::future::sleep_until` | OS 适配层 | `OsSleep::sleep_until(deadline)` |
| `axtask::spawn`（隐式） | OS 适配层 | `OsRuntime::spawn`（参考 `uart_16550::os::OsRuntime`） |
| `axsync::Mutex` | OS 适配层 | 库不暴露，作为 ArceOS 默认实现 |
| `axpoll::PollSet` | OS 适配层 | `OsWakerSet`（参考 `uart_16550::os::OsWakerSet`） |

**保留在库内**：smoltcp 协议栈（已确认不抽象协议栈）。`STACK_STAGE_BUDGET=32` 等调参常量随库迁移，使用方可覆盖。

**不包含**（本 I 范围外）：
- 协议栈 trait 抽象（`NetworkStack` / `SocketSet` / `SocketHandle` 等）
- 第二个 OS 适配（RTIC、Zephyr、Linux userspace 等）
- 真板 DMA/cache 抽象（依赖 MS11 T17 设计）
- 性能优化（`STACK_STAGE_BUDGET` 调参除外）
- `axnet` API 改造（`SERVICE` / `SOCKET_SET` / `LISTEN_TABLE` 仍为 static 单例，OS adapter 通过 `Once` 注入）

#### 已知摩擦点

1. `crates/axnet/src/lib.rs:69-72` 的 `SERVICE` / `SOCKET_SET` / `LISTEN_TABLE` 是 `static Lazy` / `Once`。抽离后保持 static 单例，不重写为实例化（实例化是 API 改造，超出本 I 范围）。
2. 入口 `init_network(net_devs: AxDeviceContainer<AxNetDevice>)` 是 axdriver 具体 enum。短期保留 `axdriver` 依赖；MS09 真板需要第二 device 容器类型时再切到 `&[Box<dyn NetDriverOps>]`。
3. `crates/axnet/src/service.rs` 的 `axhal::time::wall_time_nanos` 和 `axtask::future::sleep_until` 是 `axhal` / `axtask` 仅有的直接调用，必须抽 trait。

#### Scenario: MS08 accepted 后启动抽离评估

- **WHEN** MS08（QEMU 多 hart 正确性基线）`Plan Review: accepted` 且 tasks 状态完成
- **AND** MS06（resident stack runner + socket readiness）`Plan Review: accepted` 且唯一 spawn seam 已稳定
- **AND** MS07（单 hart 恢复语义）`Plan Review: accepted`
- **THEN** SHOULD 启动独立 change 评估 I17 范围
- **AND** MUST 复用 MS06 Task 1.1/1.4 已经设计的唯一 spawn seam
- **AND** MUST NOT 把协议栈抽象、第二个 OS 适配、真板 DMA 抽象混入本工作

#### Scenario: 启动 I17 实施前的边界确认

- **WHEN** 准备启动 I17 change 的 Plan 阶段
- **THEN** Plan Context MUST 明确"只抽离、不移植"的范围
- **AND** Plan Context MUST 列出本 I 中保留在库内的依赖（smoltcp、axdriver 适配、axsync::Mutex 默认实现）
- **AND** Plan Context MUST 在 Task Contract 标明"协议栈抽象 / 第二 OS 适配 / 真板 DMA"为 explicit non-goals

### Requirement: I18 — 异步设备骨架抽象可行性

I18 在触发条件满足前 MUST 只记录观察与候选方向，不得预先承诺或设计统一框架。

观察：UART 与 NIC 的异步路径在"ISR 收中断 → 识别原因 → 唤醒 waker → 调度 async task"这 4 个分段上模式高度相似（共享 `embassy_sync::AtomicWaker`、bounded budget、register-recheck、drop guard before wake、generation 等契约）。但"读/写数据结构"和"协议栈/应用层"是设备特定的——UART 是 SPSC ring buffer + tty/embedded_io，NIC 是 RX/TX slot + smoltcp。

本 I 只记录这个观察和潜在方向，不设计具体 trait / macro / framework 实现。骨架层与数据结构层的边界在哪、是否真的能抽成 framework，必须等至少 3 个设备的稳定实现并对齐契约后再判断。

**状态**: 待评估，未承诺
**触发条件**: MS08（QEMU 多 hart 正确性基线）accepted + 至少一个其他 async 设备（block / GPU / USB 等）出现且稳定
**关联条目**: I17（OS 适配层抽象——本 I 的潜在前置）；M36（异步 NIC 分层架构）

#### 想法记录

- 骨架层（ISR 契约、waker 抽象、async task 契约）跨 UART 与 NIC 高度相似，目前在 `axnet` 和 `uart_16550` 各实现一份
- 数据结构层（ring buffer vs RX/TX slot vs command queue）设备特定：work item 类型、completion 语义、backpressure 表达、fault 分类各不相同
- 协议栈/应用层完全无共性
- 抽象过早 = 把未稳定契约泄漏进 framework；至少要等 3 个 async 设备的契约对齐后才能判断"通则 vs 巧合"
- 即便抽象成立，也只承诺"可复用代码模式"，不承诺"所有设备统一 framework"

#### Scope（本 I 范围）

- 记录想法和观察
- 跟踪触发条件
- 触发后启动独立 change 评估可行性

#### 不在本 I 范围

- 任何具体 trait / macro / 代码生成器设计
- 数据结构层抽象（始终设备特定）
- 协议栈层抽象（始终设备特定）
- 现在就动手——必须等触发条件

#### Scenario: 评估是否启动"骨架抽象"change

- **WHEN** MS08 `Plan Review: accepted` 且 tasks 状态完成
- **AND** 至少一个非 NIC 设备的 async 化工作完成（如 block async、GPU async 等）
- **AND** I17（OS 适配层抽象）已 `accepted` 或与本 I 同期评估
- **THEN** SHOULD 启动独立 change 评估"骨架抽象"可行性
- **AND** MUST 基于实际代码证据（≥2 个设备的稳定实现）而非推测做判断
- **AND** MUST 明确"骨架层 vs 数据结构层"的抽象边界
- **AND** MUST NOT 承诺"所有设备统一 framework"——只承诺"可复用代码模式"

#### Scenario: 启动前的边界确认

- **WHEN** 准备启动 I18 change 的 Plan 阶段
- **THEN** Plan Context MUST 列出骨架层的具体契约（ISR 不搬数据、bounded budget、register-recheck、drop guard before wake、generation）
- **AND** Plan Context MUST 列出每个候选设备在该契约下的实际实现片段
- **AND** Plan Context MUST 评估"宏 vs 代码生成 vs trait"三种实现路径的取舍
- **AND** Plan Context MUST 包含"如果数据结构层无法对齐，本 I 失败"的退出条件

<!-- arc: ARC-202607251326 --> 4 条目已归档 (I07-I10, 2026-07-25) -> openspec/changes/archive/2026-07-25-arc-202607251326/proposal.md

<!-- arc: cleanup-uart-documentation-system --> I12 (UART benchmark measurement) archived 2026-07-25. Universal measurement rules migrated to quality-gate-baseline/spec.md:Benchmark measurement methodology.

<!-- arc: MIG-20260720-legacy-specs --> Legacy original: openspec/changes/archive/mig-20260720-legacy-specs/optimization-original.md (hash: 2ffa3af2), 439 lines. Active improvements extracted as I01-I10; I11 removed (console-specific, archived to `console-lichee` branch); I12 added from async UART benchmark measurement evidence. Completed/archived entries preserved as tombstones. Archive carriers: ARC-202607021535, ARC-202607021648, ARC-202607031929, ARC-202607111510.
