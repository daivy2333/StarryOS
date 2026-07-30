# SNAPSHOT.md — 项目快照

> Last updated: 2026-07-30
> Revision: `uart-lichee` @ `79a31dd`

## 项目概览

- StarryOS 是 `no_std` Rust OS kernel；workspace `starryos` / `starry-kernel` `0.2.0-preview.2`。
- 技术基线：edition 2024、`nightly-2026-02-25`、ArceOS `0.3.0-preview.2`。
- 目标：RISC-V 64-bit QEMU `riscv64-virt` 与 Lichee RV Dock D1。
- 范围：启动、进程/ELF、VFS/TTY、异步 UART、中断和平台适配。

## 代码与文档入口

- 代码与构建：`kernel/`、`crates/`、`Makefile`、`make/`。
- OpenSpec：`openspec/config.yaml`、`openspec/specs/`、`openspec/changes/`；roadmap 见 `.claude/docs/tasks.md`。
- 长文档：`.claude/analysis/`、`.claude/runbooks/`。

## 常用验证入口

- 构建/启动：`make build`、`make run`。
- 测试：`make ci-test`、`make host-test`。
- D1 benchmark：`make lichee-userbench`、`make lichee-fullbench-command`。
- 文档 Gate：`openspec validate --specs --strict --no-interactive`、`openspec validate --changes --strict --no-interactive`、`git diff --check`。

## 当前状态

- `q17-smp-memory-ordering`：18/19 tasks；QEMU 与单 hart 修复已完成，最终 multi-hart stress 仍待等价 SMP 真板。
- Roadmap：MS01 已完成；MS02 受硬件能力边界阻塞；MS03、MS04 仅在证据触发后进入 Plan。
- 当前规格：M01-M40、D01-D21、K01-K30、R 索引、I01-I10/I12，以及各 capability specs。

## 已验证基线

- 2026-07-30：`openspec validate --specs --strict --no-interactive`，24 passed / 0 failed。
- D1 async UART：既有证据显示 fullbench command-entry 正常结束，TX 吞吐接近 115200 bps 物理线速；单 hart 结果不构成 SMP 正确性证明。
- QEMU：用于功能、回归和单 hart 行为见证，不作为真板时序、线速或 multi-hart 证据。
- Writer/Reader：raw UART producer/consumer 保持唯一所有权；OS adapter 负责串行化，ring 仍按 SPSC 边界使用。

## 外部边界与工作区保护

- MS02 需要 VisionFive2 或等价 multi-hart 环境；恢复条件和验收见 `.claude/docs/tasks.md`。
- `crates/smoltcp/` 是本轮初始化前已存在的未跟踪目录，不属于 OpenSpec 升级范围，必须保留且不得改写。
- Storage/rootfs、user completion queue、mmap user ring 当前未获实施授权；重新进入时必须先 Plan。

## 迁移记录

- 2026-07-20：旧 `architecture` / `learned` / `optimization` specs 迁入 M/D/K/R/I；载体为 `openspec/changes/archive/mig-20260720-legacy-specs/`。
- 2026-07-30：规则、状态、roadmap、iteration 与 skill 入口升级；载体 `openspec/changes/archive/2026-07-30-mig-202607301654/`。
- 审计：2,743/2,743 单元通过；migration capability 未合并。
