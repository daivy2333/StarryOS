# SNAPSHOT.md — 项目快照

> Last updated: 2026-08-06
> Branch: net-k3 — 异步 NIC 开发主线；MS16 测试矩阵与 qualification Runbook 已收口并归档，未生成 TAP standard B0

## 项目概览

- **项目**: StarryOS — 基于 RISC-V 的宏内核 OS（Rust / ArceOS 组件化架构）
- **技术栈**: Rust nightly-2026-02-25 / RISC-V 64-bit / ArceOS 0.3.0-preview.2 / `axtask::future`
- **构建**: Makefile (`make build`, `make run`)
- **测试**: QEMU virt（当前）；VisionFive2（后续真板）
- **格式化/Lint**: `cargo fmt` + `cargo clippy`
- **源码目录**: `kernel/`, `crates/smoltcp/`, `crates/uart_16550/`

## 当前分支

`net-k3`（从 `uart-lichee` 分出）— 异步 NIC 开发。MS01-MS03 已归档。MS16 已按用户确认收口测试矩阵、portable workload、user-net 六方向执行资格和 R49 Runbook；TAP standard B0 未运行。

## 当前待推进

- **MS01**: ✅ 完成 — smoltcp/axnet 同步基线（归档于 `2026-07-29-t01-smoltcp-axnet-baseline`）
- **MS02**: ✅ 完成 — VirtIO-MMIO 轮询网络基线（归档于 `2026-07-29-ms02-virtio-mmio-polling-baseline`）
- **MS03**: ✅ 完成 — VirtIO-MMIO 可诊断中断基线（归档于 `openspec/changes/archive/2026-08-03-ms03-virtio-mmio-diagnostic-irq-baseline/`；iteration 000 reported + reviewed，12/12 QEMU gates PASS）
- **MS16**: ✅ 范围收口 — 归档于 `openspec/changes/archive/2026-08-06-ms16-qemu-polling-network-performance-baseline/`；测试口径见主 spec/R47，操作与覆盖状态见 R49，基础设施缺口见 I16
- **T05-T12**: IRQ 唤醒原语、异步 RX/TX、packet slot、stack、socket、恢复和多 hart
- **T13-T17**: VF2 板级事实、启动、PHY、PLIC 和 DMA/cache
- **T18-T21**: DWMAC 轮询与异步收发
- **T22-T24**: 真板恢复、多 hart 和长稳压力
- **T25**: 由数据触发 batch、moderation、offload、zero-copy 和 multiqueue

## 关键事实

| 主题 | 结论 |
|------|------|
| Async runtime | `axtask::future` + `embassy-sync::AtomicWaker`，禁止 embassy-executor |
| Protocol baseline | 本地 smoltcp 0.13.1 是目标版本；T01 先消除 axnet 的 `RxToken::preprocess` 依赖 |
| QEMU terminal (K31) | `-nographic` 连接 MMIO UART 与宿主终端；5555 只属于网络转发 |
| QEMU device (D22/K32) | 首条异步路径使用 VirtIO-MMIO；当前 feature 合并实际选择 MMIO |
| PCI (I13) | QEMU 支持 PCI device；StarryOS 纯 PCI build/run 尚未通过 |
| ISR 原则 | 最小化：读 cause → ack/mask → wake → 返回；数据搬运在任务上下文 |
| NIC 架构 (M36) | ISR → queue task (budget) → stack runner → socket readiness，4 层分离 |
| NIC 决策 (D20) | 保留 axnet-ng、smoltcp、axpoll、axtask；不引入 Embassy executor |
| Transport 边界 (M41) | probe、IRQ、DMA 属于平台层；异步队列语义不依赖总线 |
| UART→NIC 迁移 (K26) | ISR/waker/backpressure/completion 可迁移；字节 ring→DMA descriptor |
| SMP 内存序 (M39) | 按语义选 Ordering，不按架构分叉 |
| PLIC/Clock (M37/M38) | VF2 bring-up: trust-u-boot 保留 PLIC+Clock，init_primary/percpu 分离 |
| OS 接口 (M14) | 2-trait 最小接口 (`OsRuntime` + `OsWakerSet`)，只保留实际调用代码 |
| SPSC 边界 (K25) | unsafe unique constructor + crate-private mutation + exactly-once startup |
| Device mask 测试模式 (K35) | device selection 用 `fn(mask, impl IntoIterator<Item=bool>)` 纯策略 helper，无需真实设备即可单元测试 mask×eligibility 组合 |
| MS02 空闲 CPU | 10ms polling fallback：QEMU 单核 100-111% CPU 占用（30 秒采样）；轮询 fallback 是预期行为，不是 busy loop |
| MS03 IRQ | VirtIO MMIO `InterruptStatus`/`InterruptACK` 是 32-bit 寄存器（非 u8）；未对齐读写导致计数器全零；device_id=1 是网卡（非 2）；`axhal::irq::register` 接受 `fn()` handler |
| MS03 QEMU 端到端 | 12/12 gates PASS（启动签名、idle、uart、rx2、tx2、both、repeat rx2、MS02 TCP/UDP、MS01 14/14）；IRQ 7 独立于 UART IRQ 10；无 spurious net events、无 IRQ storm |
| 网卡基准资格 (K37) | environment、treatment、test 分轴；execution、correctness、performance 分层；`not-run` 与 `infrastructure-unavailable` 分开记录 |
| MS16 运行边界 | user-net 六方向已产生 manifest/round；只证明兼容执行与失败分类。TAP、standard B0 和完整矩阵未运行 |

## OpenSpec 体系

| 域 | 条目数 | 备注 |
|----|--------|------|
| `openspec/specs/project-model/` | 10 (M01-M41) | 新增 M41；M03/M33/M35/M40 已归档 |
| `openspec/specs/decisions/` | 5 (D01-D22) | 新增 D22；D03 已归档 |
| `openspec/specs/knowledge/` | 16 | 新增 K37 网卡基准分轴与资格层级 |
| `openspec/specs/references/` | 活跃 17 | R47 分析、R49 qualification Runbook 已登记 |
| `openspec/specs/improvements/` | 5 | 新增 I16 网卡性能矩阵基础设施补全，未承诺 |
| `openspec/changes/` | 0 个活跃 | MS01+MS02+MS03+MS16 已归档 |
| `.claude/analysis/` | 11 | 含 R47 网卡基准设计 |
| `.claude/runbooks/` | 7 | 含 R49 网卡基准资格扫描 |

## 证据文件

- K31/K32 已记录 2026-07-27 QEMU 对照结果
- MS01 完成证据：三轮 evidence（000/001/002），14/14 QEMU PASS — 归档于 `openspec/changes/archive/2026-07-29-t01-smoltcp-axnet-baseline/evidence/`
- MS02 完成证据：四轮 evidence（000/001/002/003），8/8 unit + 14/14 MS01 runtime + QEMU no-hostfwd + user-net TCP/UDP + TAP ARP/ICMP — 归档于 `openspec/changes/archive/2026-07-29-ms02-virtio-mmio-polling-baseline/evidence/`
- MS03 完成证据：一轮 evidence（000-initial），12/12 QEMU gates PASS — 归档于 `openspec/changes/archive/2026-08-03-ms03-virtio-mmio-diagnostic-irq-baseline/evidence/000-initial/`
- MS16 收口证据：portable workload、工具验证、N00-N03 和 user-net 六方向记录 — 归档于 `openspec/changes/archive/2026-08-06-ms16-qemu-polling-network-performance-baseline/evidence/`；不含 TAP standard B0
- UART 阶段证据已全部归档至 `openspec/changes/archive/2026-07-25-cleanup-uart-docs/`

## 迁移记录

2026-08-06：MS16 以测试矩阵、portable workload、user-net 六方向执行资格和 R49 Runbook 收口并归档。主 `network-benchmark-baseline` spec 已同步 8 条 requirements。change 保留 6/25 tasks 完成状态；TAP/多流/payload/profile 等已有入口但未运行，基础设施缺失项登记为 I16。未声明 polling B0 性能结果。
2026-08-03：MS03 VirtIO-MMIO 可诊断中断基线完成并归档 — QEMU UART 从 global hook 迁到 IRQ 10 设备 handler；VirtIO-net IRQ 7 诊断 handler 注册；pure-logic status decoder（`classify_mmio_status`）+ telemetry（`IrqTelemetry`）+ host harness 20/20 PASS；guest C probe（5 modes）；ioctl `0x4e494431` snapshot。12/12 QEMU gates PASS。3 个 runtime 期 bug 修复。Plan Review: no-follow-up。Change 归档于 `openspec/changes/archive/2026-08-03-ms03-virtio-mmio-diagnostic-irq-baseline/`。
2026-07-29：MS02 VirtIO-MMIO 轮询网络基线完成 — `register_waker` 引入 10ms polling fallback + `requires_polling` capability + 提取 `any_masked_device_requires_polling` 纯策略 helper + 8/8 unit PASS；smoltcp 启用 `auto-icmp-echo-reply`；QEMU 端到端验证 14/14 MS01 PASS + 无 hostfwd 启动 + user-net TCP/UDP 5555 + TAP ARP/ICMP + 30 秒空闲 CPU 基线。Change 归档于 `openspec/changes/archive/2026-07-29-ms02-virtio-mmio-polling-baseline/`（4 iterations，4 轮 evidence）。新增 Runbook `ms02-virtio-mmio-evidence.md` (R45)。
2026-07-29：MS01 smoltcp/axnet 同步基线完成 — 本地 smoltcp 0.13.1 + 本地 axnet，移除 `RxToken::preprocess` 私有依赖，TCP bind sidecar + listener 512 容量 + egress-until-none + 14/14 QEMU 手测 PASS。Change 归档于 `openspec/changes/archive/2026-07-29-t01-smoltcp-axnet-baseline/`（3 iterations，3 轮 evidence）。
2026-07-27：根据 QEMU 对照验证改用 VirtIO-MMIO 主线。任务由 T01-T13 拆分为 T01-T25；PCI 转为 I13。
2026-07-25：`cleanup-uart-documentation-system` — UART 文档体系清理：主载体位于 `openspec/changes/archive/2026-07-25-cleanup-uart-docs/`；q17 与旧 ARC 分别保存在各自归档目录。活跃文档只保留 OS/NIC/VF2 和通用方法。
2026-07-25：`net-k3` 分支从 `uart-lichee` 分出。UART 专属条目标记为归档，载体收编至 `openspec/changes/archive/2026-07-25-arc-202607251326/`。
2026-07-20：旧体系 spec 迁移至 M/D/K/R/I。Migration carrier: `openspec/changes/archive/mig-20260720-legacy-specs/`。
