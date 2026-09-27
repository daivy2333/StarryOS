# SNAPSHOT.md — 当前项目描述

> Sync status: current
> Updated: 2026-09-26
> Revision: `9efb252963c791767ed14e106ef072e59c7d0983`
> Branch: `mul-hart-k3`
> Worktree: dirty（未提交）

> 本文件描述工作树当前状态，含 HEAD `9efb2529` 之后尚未提交的 MS08 实现与收尾改动；下方「仓库现场」逐项列出未提交内容。MS08 实现与本次收尾尚未提交，提交后需再次刷新本文件。

## 项目身份

StarryOS 是使用 Rust 编写、基于 ArceOS 组件化架构的宏内核操作系统。本仓库同时承载内核入口、内核子系统、本地组件、平台适配、用户态测试程序和 OpenSpec 工程文档。

## 技术栈

- Rust edition 2024，工具链 `nightly-2026-02-25`。
- ArceOS `0.3.0-preview.2` 组件族；其中 `axtask` 以工作区自有副本（`crates/axtask`）参与构建。
- 目标架构包括 RISC-V 64、LoongArch 64、AArch64 与 x86_64；当前仓库包含以 RISC-V 平台为主的板级适配。
- 异步执行与唤醒能力由本地内核组件、工作区 `axtask` 的 schedulable run queue 与 remote-ready IPI，以及 `axtask::future` 等依赖提供。
- CPU mask 由工作区 `axtask::AxCpuMask` 封装私有 `cpumask 0.1.0`，debug 与 release 构建的越界访问均 fail closed。
- 构建入口由 Cargo workspace 与 Makefile 共同组成。

## 组成与职责

| 组成 | 职责 |
|---|---|
| `src/` | 顶层内核入口与产品组装 |
| `kernel/` | 内核主体、系统调用、VFS、设备、平台和测试支持 |
| `crates/axtask/` | 工作区自有 ArceOS 任务组件副本：schedulable run queue 集合、入队前 affinity、remote-ready IPI、`AxCpuMask` 安全封装 |
| `crates/axnet/`、`crates/smoltcp/` | 本地网络接口与协议栈实现 |
| `crates/axdriver_net/`、`crates/axdriver_virtio/`、`crates/virtio-drivers/` | 本地化网络驱动依赖（workspace patch，含 EVENT_IDX 通知控制与 transport-neutral queue contract） |
| `crates/uart_16550/` | 本地 UART 驱动实现 |
| `crates/axfs-ng/` | 本地文件系统组件 |
| `crates/axplat-riscv64-lichee-d1/` | Lichee RV Dock D1 平台组件 |
| `tests/`、`kernel/tests/`、`scripts/` | 用户态与内核侧测试载荷、MS08 SMP guest probe、host serial/peer harness 与纯输出 validator |
| `openspec/`、`.claude/` | 规范、变更、项目记忆、分析和操作文档 |

## 支持范围与交付形态

- QEMU virt 是仓库内可配置的虚拟平台交付形态。
- QEMU virt 的单 hart VirtIO-MMIO 已具备 IRQ 唤醒、唯一双向 queue service、EVENT_IDX
  通知控制、固定容量 RX/TX packet slots、typed backpressure、TX completion/reclaim、
  ticketed C4 flush、常驻 stack runner 与 per-socket readiness bridge（多 waiter、
  listener accept bridge、terminal fault 发布），并已实现单 hart 恢复语义：epoch-scoped
  socket terminal、resident recovery owner、reset/link 下的 queue 重建（`Dma::new` 零化）、
  QueueEpoch/LinkGeneration/SocketEpoch 分层推进。MS06 单 hart 手工验收已接收；MS07 恢复语义
  Iteration 000–007 已 accepted。reset 的 SMP 放大、真板与性能资格不在该结论内。
- QEMU virt 的 `SMP=16` 多 hart 网络基线已 accepted：16 个 schedulable hart 上，网络 queue
  owner 与 stack runner 各自固定 singleton affinity，跨 hart 唤醒由 remote-ready IPI 直接驱动
  （含关闭目标 hart timer 的 witness 与 timer 恢复检查），TCP/UDP 双向帧交换、queue Full→
  恢复、poll/select/epoll readiness 与 quiet 窗口均有直接观测，descriptor/slot/ticket 账本
  闭合。结论限定 QEMU virt 的 VirtIO-MMIO 模型与 16 个同构虚拟 hart。
- UART 多 hart 实现、copier 固定 placement、snapshot 观测与 D1 有界 TX slow-poll workaround
  保留；UART 专项 runtime 资格未取得（console 输出只作为测试基础设施，不构成 UART 异步
  语义结论）。受控迁移稳定性、reset/link 交错、组合压力和真板性能均无结论。
- Lichee RV Dock D1 与 VisionFive 2 是仓库覆盖的 RISC-V 真实平台形态。
- 当前异步 NIC 的最终目标板尚未在仓库中登记；VisionFive 2 支持和 ArceOS DWMAC 经验不构成目标板选择。
- 根 Cargo features 提供 `qemu`、`lichee-d1`、`lichee-d1-async`、`vf2` 与 `smp` 等产品组装入口。
- 交付物包括可启动内核镜像、平台构建产物，以及配套的内核态和用户态测试载荷。

## 仓库现场

- 当前 Git 分支为 `mul-hart-k3`；`net-k3`、`k3` 的历史结论已并入该分支。
- HEAD 为 `9efb2529`（"MS08: WIP Iteration 001-003 progress"）。MS08 Iteration 000–004 的实现、
  MS08 收尾归档与本次文档同步尚未提交：`openspec/changes/ms08-qemu-multi-hart-correctness-baseline/`
  已移动到 `openspec/changes/archive/2026-09-26-ms08-qemu-multi-hart-correctness-baseline/`，
  增量规格已合并为 `openspec/specs/qemu-multi-hart-async-io-correctness/spec.md`。
- 已无活跃 change（`openspec list` 为空）。MS08 change 于 2026-09-26 正常完成并归档
  （Iteration 000–005 共 18 个 Cycle，最终 Cycle `005/003-rework` Review accepted；Tasks 1.1–7.3
  全部闭合，6.1 与 7.1–7.3 为用户明确豁免的 SKIPPED）。
- 已知遗留：`make host-test` 中 `tests/ms04-async-rx-host-harness.rs::net_migration_stimulus_is_blocked_gated_in_source`
  失败（已验证为 pre-existing，源自 MS08 Iteration 001–003 的 placement 改动），待后续处理。
- 详细里程碑状态见 [`tasks.md`](tasks.md)。

## 权威入口

- 公共流程和编辑规则：[`AGENTS.md`](../../AGENTS.md)
- Milestone 与任务状态：[`tasks.md`](tasks.md)
- 活跃及归档变更：[`openspec/changes/`](../../openspec/changes/)
- 项目模型、决策、知识、参考与改进：[`openspec/specs/`](../../openspec/specs/)
