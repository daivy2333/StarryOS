# SNAPSHOT.md — 项目快照

> Last updated: 2026-07-20
> Branch: console-lichee — Q26/Q27/Q28/Q29 已归档；Q24 等待 SMP 硬件；Q30 维持证据触发

## 项目概览

- **项目**: StarryOS — 基于 RISC-V 的宏内核 OS（Rust / ArceOS 组件化架构）
- **技术栈**: Rust nightly-2026-02-25 / RISC-V 64-bit / ArceOS 0.3.0-preview.2 / `axtask::future` + `embassy_sync::AtomicWaker`
- **构建**: Makefile (`make build`, `make run`, `make lichee-userbench`)
- **测试**: QEMU virt + Lichee RV Dock D1 真板
- **格式化/Lint**: `cargo fmt` + `cargo clippy`
- **源码目录**: `kernel/`, `crates/uart_16550/`, `crates/axplat-riscv64-lichee-d1/`

## 当前分支

`console-lichee` — Q26/Q27/Q28/Q29 已归档。历史分支 `asyncuart-dev` / `feat/uart-16550-async`（Q0~Q18）。

## 近期完成

- Q17：QEMU 修复完成；ier_cache RMW 临界区化，TX completion 原子序升级。多 hart 未实测。
- Q19~Q23：D1 smoke/kbench/userbench/memory-root、Q20 jitter/S40/raw evidence 完成。Q21/Q22 取消当前规划。
- Q27a：readiness+waker+register-recheck 完成。Q27：blocking backpressure 完成，QEMU/D1 通过。
- Q28：raw AsyncUartWriter 收敛为不可 clone、unsafe 唯一构造。QEMU/D1 通过。
- Q29：raw AsyncUartReader 收敛为 unsafe 唯一构造，RX mutation crate-private，exactly-once copier startup。62 tests + 8 doctest + 10 compile-fail 通过。
- Q26：维护性清理已归档。host/static Gate 通过，部分运行时 Gate 为 ENV BLOCK。

## 当前待推进

- Q24：VisionFive2 或等价 SMP 环境复验 O63，覆盖跨 hart write/flush/tcdrain、read 与 IER enable/disable。
- Q30：仅在 Q24 或真实 workload 提供消息原子性、公平性、锁竞争证据时规划。

## 关键事实

| 主题 | 结论 |
|---|---|
| QEMU benchmark | `make run` 进入 rootfs，`/bin/benchmark` 适合功能/回归验证，不适合声明真板线速 |
| Q17/D1 边界 | Lichee RV Dock userbench 正常，但不等于 multi-hart stress |
| D1 async UART | 96.6%-99.1% 线速；THRE/no-pending 以 slow-poll fallback 保证 forward progress |
| Q27 TX backpressure | D1 S11 short writes 36→0/102400B，无性能退化 |
| Q28 writer 契约 | raw writer unsafe unique + producer lock 串行化 |
| Q29 reader 契约 | raw reader unsafe unique + RX mutation crate-private |

## OpenSpec 体系

- `openspec/specs/project-model/` — 40 个当前有效跨模块约束（M01-M40）
- `openspec/specs/decisions/` — 21 个决策记录（D01-D21）
- `openspec/specs/knowledge/` — 27 个已验证知识条目（K01-K27）
- `openspec/specs/references/` — 34 个参考索引（R01-R34）
- `openspec/specs/improvements/` — 10 个改进机会（I01-I10）
- `openspec/changes/` — 活跃变更与归档
- `CLAUDE.md` — 公共规则单一来源
- `AGENTS.md` — 入口适配器

## 迁移记录

2026-07-20：旧体系 spec（`openspec/specs/architecture/`、`learned/`、`optimization/`）全量迁移至新 M/D/K/R/I 体系。Migration carrier: `openspec/changes/archive/mig-20260720-legacy-specs/`。来源 hash: architecture `5b054d98`, learned `f09d4cae`, optimization `2ffa3af2`。
