# ARC-202607251326 — UART 生命周期归档

## 目的

分支 `net-k3`（从 `uart-lichee` 分出）将开展异步 NIC 开发。本次归档清理 UART 专属文档条目，保留跨模块 async 架构模式、NIC 直接适用条目和通用 OS 约束。

## 判断原则

| 保留条件 | 归档条件 |
|----------|----------|
| 跨模块 async 模式（ISR→wake→task、AtomicWaker、register-recheck） | 纯 UART 实现细节（16550 寄存器、TTY/ldisc、termios） |
| NIC 直接适用（M36/D20/K26 分层架构/决策/迁移矩阵） | D1/Lichee 平台专属事实（M19-M29, M40） |
| VF2 bring-up（M37/M38 PLIC、D21 trust-u-boot） | UART benchmark 证据与方法（M11, M30, K11） |
| 通用 OS 约束（M07/M13/M14） | 已完成 UART 阶段决策（Q15-Q29 实施细节） |
| 已验证方法论（M32/K03/K04 等） | UART 专用 Runbook |

## 完整映射表

### project-model (M01-M40) — 归档 27 / 保留 13

| ID | 动作 | 理由 | 恢复条件 |
|----|------|------|----------|
| M01 | **KEEP** | Async runtime: axtask+AtomicWaker — NIC 直接适用 | — |
| M02 | **Archive** | VFS DeviceOps for UART — UART 专属 | NIC 设备注册需要时参考 |
| M03 | **KEEP** | Ring buffer + copier — ISR→task 模式跨模块适用 | — |
| M04 | **Archive** | termios — UART 专属 | 需要终端行规则时恢复 |
| M05 | **Archive** | UART HAL (UartPort trait) — UART 专属 | 新增 UART 硬件时恢复 |
| M06 | **Archive** | UART DMA 策略 — UART 专属；NIC DMA 另有路线 | NIC DMA 决策时参考 |
| M07 | **KEEP** | 内核日志同步约束 — OS 级 | — |
| M08 | **Archive** | UART MMIO 0x10000000 — UART 专属 | — |
| M09 | **Archive** | NS16550 stride — UART 专属 | — |
| M10 | **Archive** | Console/Async 共存 — UART 专属 | — |
| M11 | **Archive** | RX 性能测试方法 — UART 专属 | — |
| M12 | **Archive** | uart_16550 crate — UART 专属 | — |
| M13 | **KEEP** | LTO 延期 — 构建级 | — |
| M14 | **KEEP** | OS 抽象最小接口 — 跨模块 | — |
| M15 | **Archive** | TxCompletion 四阶段 drain — UART 专属 | NIC completion 设计时参考模式 |
| M16 | **Archive** | TtyWrite 短写契约 — UART 专属 | — |
| M17 | **Archive** | 增量融合策略 — UART 专属方法论 | — |
| M18 | **Archive** | 平台描述符 — UART 板级事实 | NIC 平台描述符设计时参考 |
| M19 | **Archive** | D1 axplat 启动 — D1 专属 | — |
| M20 | **Archive** | D1/C906 PTE flags — D1 专属 | — |
| M21 | **Archive** | Q19B embedded benchmark — D1/UART 专属 | — |
| M22 | **Archive** | D1 UartPort — D1 专属 | — |
| M23 | **Archive** | D1 userbench runtime — D1 专属 | — |
| M24 | **Archive** | D1 feature 分离 — D1 专属 | — |
| M25 | **Archive** | D1 THRE 边沿丢失 — D1 专属 | — |
| M26 | **Archive** | Q19C memory-root — D1/UART 专属 | — |
| M27 | **Archive** | D1 P99 长尾 — D1 专属 | — |
| M28 | **Archive** | Q19C-M1 FS API — D1/UART 专属 | — |
| M29 | **Archive** | Q19C-M2 command-entry — D1/UART 专属 | — |
| M30 | **Archive** | Q20 benchmark evidence — UART 专属 | — |
| M31 | **Archive** | Q21/Q22 取消 — UART 专属 | — |
| M32 | **KEEP** | Lint/test Gate 分层 — 方法论 | — |
| M33 | **KEEP** | io_uring 映射 — 跨模块 | — |
| M34 | **Archive** | UART backpressure 分阶段 — UART 专属 | NIC backpressure 设计时参考 |
| M35 | **KEEP** | 并发契约分流 — 方法论跨模块 | — |
| M36 | **KEEP** | NIC 分层架构 — NIC 直接适用 | — |
| M37 | **KEEP** | PLIC/Clock trust-u-boot — VF2 bring-up | — |
| M38 | **KEEP** | PLIC 防御性设计 — VF2 bring-up | — |
| M39 | **KEEP** | SMP 原子内存序 — 跨模块 | — |
| M40 | **Archive** | Lichee RV Dock 启动链 — D1 专属 | — |

### decisions (D01-D21) — 归档 14 / 保留 7

| ID | 动作 | 理由 |
|----|------|------|
| D01 | **KEEP** | 异步运行时选型 — NIC 直接适用 |
| D02 | **Archive** | VFS DeviceOps for UART |
| D03 | **KEEP** | 缓冲策略演进 — ring buffer 模式跨模块 |
| D04 | **Archive** | termios 策略 |
| D05 | **Archive** | UART HAL 演进 |
| D06 | **Archive** | UART DMA 策略 |
| D07 | **Archive** | 内核日志约束（M07 KEEP 保留当前约束；D07 决策历史归档） |
| D08 | **Archive** | MMIO 权限误判 — UART stride 教训 |
| D09 | **Archive** | Console/Async 共存 |
| D10 | **Archive** | uart_16550 crate 提取 |
| D11 | **KEEP** | LTO 延期 — 构建决策 |
| D12 | **Archive** | TxCompletion drain |
| D13 | **Archive** | TtyWrite 短写 |
| D14 | **Archive** | Q15 增量融合 |
| D15 | **Archive** | 平台解耦（M18 已归档） |
| D16 | **Archive** | D1 async UART 实施路线 |
| D17 | **Archive** | Q21/Q22 取消 |
| D18 | **Archive** | io_uring 借鉴边界（M33 KEEP 保留映射） |
| D19 | **Archive** | UART backpressure 演进（M34 已归档） |
| D20 | **KEEP** | NIC 架构分层 — NIC 直接适用 |
| D21 | **KEEP** | PLIC/Clock — VF2 bring-up |

### knowledge (K01-K30) — 归档 16 / 保留 14

| ID | 动作 | 理由 |
|----|------|------|
| K01 | **KEEP** | ISR 极简原则 — NIC 核心模式 |
| K02 | **Archive** | 双缓冲 Ring Buffer (UART 专属 copier) |
| K03 | **KEEP** | poll_io 标准模式 — 跨模块 |
| K04 | **KEEP** | AtomicWaker 使用模式 — 跨模块 |
| K05 | **Archive** | UART 硬件集成 (IER/IIR/LSR/MCR) |
| K06 | **Archive** | THR_EMPTY vs TEMT |
| K07 | **Archive** | QEMU 时序欺骗 — UART 线速 |
| K08 | **Archive** | 跨层状态传播 (FIONBIO/O_NONBLOCK) |
| K09 | **KEEP** | Embassy 选型边界 — NIC 直接适用 |
| K10 | **Archive** | MMIO 权限诊断 (stride bug) |
| K11 | **Archive** | benchmark 公平性 (UART Async vs Console) |
| K12 | **Archive** | 性能优化四方向 (IER/ISR/batch/waker) |
| K13 | **Archive** | UART 编程模式模板 |
| K14 | **Archive** | musl/rootfs 构建踩坑 |
| K15 | **KEEP** | OpenSpec tasks.md 漂移 — 方法论 |
| K16 | **KEEP** | SMP 内存序规则 — NIC 直接适用 |
| K17 | **Archive** | Q15 增量融合铁律 |
| K18 | **Archive** | TEMT corner-case |
| K19 | **Archive** | D1 平台关键事实 |
| K20 | **Archive** | D1 THRE/no-pending |
| K21 | **KEEP** | 真板验证分层 — NIC bring-up 直接适用 |
| K22 | **Archive** | memory-root path API (D1) |
| K23 | **KEEP** | io_uring 设计映射 — 跨模块 |
| K24 | **KEEP** | 并发边界矩阵 — NIC 并发设计参考 |
| K25 | **KEEP** | SPSC capability 边界 — NIC queue owner 模式 |
| K26 | **KEEP** | UART→NIC 迁移矩阵 — NIC 直接适用 |
| K27 | **KEEP** | ProcessMode 删除教训 — 方法论 |
| K28 | **Archive** | D1 构建踩坑集 |
| K29 | **Archive** | D1 feature 继承陷阱 |
| K30 | **Archive** | 用户态 async read 调用链 (UART TTY/ldisc) |

### references (R01-R46) — 归档 10 / 保留 13 / 已归档 23

**已归档（无需操作）**: R01-R13, R15, R17-R19, R21-R22, R27, R33, R42-R44, R46

| ID | 动作 | 理由 |
|----|------|------|
| R14 | **KEEP** | arceos-true-board-validation — VF2 bring-up 适用 |
| R16 | **Archive** | q20-benchmark-gap-closure — UART 专属（文档移入 _archive） |
| R20 | **Archive** | docs/d1_out.md — D1 UART evidence |
| R23 | **KEEP** | async-network-project-overview — NIC |
| R24 | **KEEP** | embassy-network-module-evaluation — NIC |
| R25 | **KEEP** | arceos-async-network-driver-analysis — NIC |
| R26 | **KEEP** | starryos-async-network-roadmap — NIC |
| R28 | **Archive** | async UART API 路径速查 |
| R29 | **Archive** | musl/rootfs 构建环境 (K14 已归档) |
| R30 | **Archive** | D1 平台事实与烧录 |
| R31 | **Archive** | D1 构建与 feature gate |
| R32 | **Archive** | D1 async UART 行为数据 |
| R34 | **Archive** | Q26 维护性清理 |
| R35 | **Archive** | qemu-build Runbook (UART 专用) |
| R36 | **Archive** | d1-build-and-flash Runbook (D1 专用) |
| R37 | **KEEP** | benchmark-guide Runbook — 方法论跨模块 |
| R38 | **KEEP** | incremental-merge Runbook — 方法论 |
| R39 | **KEEP** | regression-gate Runbook — 方法论 |
| R40 | **KEEP** | board-bringup-ladder Runbook — NIC bring-up 适用 |
| R41 | **Archive** | benchmark-qemu-d1 Runbook (UART/D1 专用) |
| R45 | **KEEP** | q31-console-cpu-efficiency-port — 跨模块分析参考 |

### improvements (I01-I12) — 归档 8 / 保留 4

| ID | 动作 | 理由 |
|----|------|------|
| I01 | **Archive** | D1 TX 效率 — D1 专属 |
| I02 | **Archive** | user ring/completion — UART 专属（Q21/Q22 已取消） |
| I03 | **Archive** | MPSC ring — UART 专属（Q30 evidence trigger） |
| I04 | **Archive** | syscall 原子性 — UART 专属 |
| I05 | **KEEP** | O63 multi-hart stress — NIC SMP 验证相关 |
| I06 | **KEEP** | ArceOS 借鉴清单 — VF2/NIC bring-up 适用 |
| I07 | **Archive** | 已排除优化 (embassy 反模式) — K09 保留判断原则 |
| I08 | **Archive** | 远期优化候选 — UART 专属 |
| I09 | **Archive** | Q13 中长期优化 — UART 专属 |
| I10 | **Archive** | Q26 维护性清理 — 已完成归档 |
| I12 | **KEEP** | UART benchmark 测量优化 — 方法论跨模块（NIC benchmark 设计参考） |

### Analysis 文档

| 文件 | 动作 |
|------|------|
| `async-network-project-overview.md` | **KEEP** — NIC |
| `arceos-async-network-driver-analysis.md` | **KEEP** — NIC |
| `embassy-network-module-evaluation.md` | **KEEP** — NIC |
| `starryos-async-network-roadmap.md` | **KEEP** — NIC |
| `arceos-true-board-validation.md` | **KEEP** — VF2 bring-up |
| `q17-smp-memory-ordering.md` | **Artifact-Archive** — UART 专属（移至 `_archive/`） |
| `q20-benchmark-gap-closure.md` | **Artifact-Archive** — UART 专属 |
| `async-uart-cpu-efficiency-metrics.md` | **Artifact-Archive** — UART 专属 |
| `q31-console-cpu-efficiency-port.md` | **KEEP** — 跨模块移植分析 |
| `lichee/` | **Artifact-Archive** — D1 专属（已为 tombstone） |

### Runbooks

| 文件 | 动作 |
|------|------|
| `qemu-build.md` | **Artifact-Archive** — UART 专用 |
| `d1-build-and-flash.md` | **Artifact-Archive** — D1 专用 |
| `benchmark-qemu-d1.md` | **Artifact-Archive** — UART/D1 专用 |
| `benchmark-guide.md` | **KEEP** — 方法论 |
| `incremental-merge.md` | **KEEP** — 方法论 |
| `regression-gate.md` | **KEEP** — 方法论 |
| `board-bringup-ladder.md` | **KEEP** — NIC bring-up 适用 |

### State Docs

| 文件 | 动作 |
|------|------|
| `SNAPSHOT.md` | **Rewrite** — 反映 net-k3 分支状态 |
| `tasks.md` | **Update** — 压缩 UART 历史，保留方法论文本，添加 net-k3 节 |

## 恢复条件

所有归档条目保留编号和路径映射。恢复时：
1. 从本 proposal 映射表找到原编号
2. 从 git history (`uart-lichee` 分支) 恢复全文
3. 从 `openspec/changes/archive/` 中对应 Q-change 获取完整上下文

## 排除项

- `CLAUDE.md` — 永不自动修改
- 活跃 change `q17-smp-memory-ordering` — 仍有 1 deferred task
- 已归档 changes (Q0-Q32) — 已在 `openspec/changes/archive/`
- Legacy migration carrier `MIG-20260720-legacy-specs` — 不可变
- `.claude/analysis/_archive/` — 已归档，不重复处理
