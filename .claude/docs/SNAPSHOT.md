# SNAPSHOT.md — 当前项目描述

> Sync status: current
> Updated: 2026-08-24
> Revision: `0acc08137a5df9d3e1ebce709f3760e6d4471d2d`
> Branch: `net-k3`
> Worktree: modified (3 local commits ahead of `origin/net-k3`, working tree clean)

## 项目身份

StarryOS 是使用 Rust 编写、基于 ArceOS 组件化架构的宏内核操作系统。本仓库同时承载内核入口、内核子系统、本地组件、平台适配、用户态测试程序和 OpenSpec 工程文档。

## 技术栈

- Rust edition 2024，工具链 `nightly-2026-02-25`。
- ArceOS `0.3.0-preview.2` 组件族。
- 目标架构包括 RISC-V 64、LoongArch 64、AArch64 与 x86_64；当前仓库包含以 RISC-V 平台为主的板级适配。
- 异步执行与唤醒能力由本地内核组件和 `axtask::future` 等依赖提供。
- 构建入口由 Cargo workspace 与 Makefile 共同组成。

## 组成与职责

| 组成 | 职责 |
|---|---|
| `src/` | 顶层内核入口与产品组装 |
| `kernel/` | 内核主体、系统调用、VFS、设备、平台和测试支持 |
| `crates/axnet/`、`crates/smoltcp/` | 本地网络接口与协议栈实现 |
| `crates/axdriver_net/`、`crates/axdriver_virtio/`、`crates/virtio-drivers/` | 本地化网络驱动依赖（workspace patch，含 EVENT_IDX 通知控制与 transport-neutral queue contract） |
| `crates/uart_16550/` | 本地 UART 驱动实现 |
| `crates/axfs-ng/` | 本地文件系统组件 |
| `crates/axplat-riscv64-lichee-d1/` | Lichee RV Dock D1 平台组件 |
| `tests/`、`kernel/tests/` | 用户态与内核侧测试载荷 |
| `openspec/`、`.claude/` | 规范、变更、项目记忆、分析和操作文档 |

## 支持范围与交付形态

- QEMU virt 是仓库内可配置的虚拟平台交付形态。
- QEMU virt 的单 hart VirtIO-MMIO 已具备 IRQ 唤醒、唯一双向 queue service、EVENT_IDX
  通知控制、固定容量 RX/TX packet slots、typed backpressure、TX completion/reclaim 和
  ticketed C4 flush；独立 stack runner、准确 socket readiness、reset、SMP、真板与性能资格
  不在该结论内。
- Lichee RV Dock D1 与 VisionFive 2 是仓库覆盖的 RISC-V 真实平台形态。
- 当前异步 NIC 的最终目标板尚未在仓库中登记；VisionFive 2 支持和 ArceOS DWMAC 经验不构成目标板选择。
- 根 Cargo features 提供 `qemu`、`lichee-d1`、`lichee-d1-async`、`vf2` 与 `smp` 等产品组装入口。
- 交付物包括可启动内核镜像、平台构建产物，以及配套的内核态和用户态测试载荷。

## 仓库现场

- 当前 Git 分支为 `net-k3`。
- 当前 revision 为 `0acc08137a5df9d3e1ebce709f3760e6d4471d2d`。
- 工作树领先 `origin/net-k3` 三个未推送 commit（`MS06:第一次提交`、`MS06:第二次提交`、`MS06:第三次提交`），工作树当前 clean。

## 权威入口

- 公共流程和编辑规则：[`CLAUDE.md`](../../CLAUDE.md)
- Milestone 与任务状态：[`tasks.md`](tasks.md)
- 活跃及归档变更：[`openspec/changes/`](../../openspec/changes/)
- 项目模型、决策、知识、参考与改进：[`openspec/specs/`](../../openspec/specs/)
