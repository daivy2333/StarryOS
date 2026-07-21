# SNAPSHOT.md — 项目快照

> Last updated: 2026-07-21
> Branch: console-lichee — Console polling baseline 已归档；Q24 等待 SMP 硬件；Q30 维持证据触发

## 项目概览

- **项目**: StarryOS — 基于 RISC-V 的宏内核 OS（Rust / ArceOS 组件化架构）
- **技术栈**: Rust nightly-2026-02-25 / RISC-V 64-bit / ArceOS 0.3.0-preview.2 / `axtask::future` + `axplat::console::CONSOLE_LOCK`
- **构建**: Makefile (`make build`, `make run`, `make lichee-userbench`, `make lichee-fullbench-command`)
- **测试**: QEMU virt + Lichee RV Dock D1 真板
- **格式化/Lint**: `cargo fmt` + `cargo clippy`
- **源码目录**: `kernel/`, `crates/axplat-riscv64-lichee-d1/`

## 当前分支

`console-lichee` — 异步 UART 已删除，改用 polling Console。Q26/Q27/Q28/Q29 已归档；`console-polling-baseline` 已归档（2026-07-21）。历史分支 `asyncuart-dev` / `feat/uart-16550-async`（Q0~Q18）。

## 近期完成

- Q17：QEMU 修复完成；ier_cache RMW 临界区化，TX completion 原子序升级。多 hart 未实测。
- Q19~Q23：D1 smoke/kbench/userbench/memory-root、Q20 jitter/S40/raw evidence 完成。Q21/Q22 取消当前规划。
- Q27a/Q27/Q28/Q29：readiness/backpressure/writer 契约/reader 契约全部完成并归档。QEMU/D1 通过。
- Q26：维护性清理已归档。
- **Console polling baseline**（2026-07-21 归档）：删除 async UART（crate/driver/copier/IRQ），替换为 `ProcessMode::Polling` + `InputReader` 全功能 polling Console。QEMU shell 交互正常，D1 真板 99.0-99.4% 线速，short_writes=0，drain_errors=0。

## 当前待推进

- Q24：VisionFive2 或等价 SMP 环境复验 O63，覆盖跨 hart write/flush/tcdrain、read 与 IER enable/disable。
- Q30：仅在 Q24 或真实 workload 提供消息原子性、公平性、锁竞争证据时规划。

## 关键事实

| 主题 | 结论 |
|---|---|
| QEMU benchmark | `make run` 进入 rootfs，`/bin/benchmark` 适合功能/回归验证，不适合声明真板线速 |
| D1 async UART | 96.6%-99.1% 线速 |
| D1 Console polling | 99.0-99.4% 线速，D1 真板 fullbench-command Done/exit 0/drain_errors=0 |
| Console TTY | `ProcessMode::Polling` + 完整 `InputReader`（ICRNL/canonical/echo/erase/ISIG），self-wake 按需轮询 |
| Port init | attach-only + width-correct IER disable，不重写 U-Boot 配置 |
| TX lock | `axplat::console::CONSOLE_LOCK` → local `CONSOLE_PORT`，drain 单次持锁到 TEMT |

## OpenSpec 体系

- `openspec/specs/project-model/` — 40 个当前有效跨模块约束（M01-M40）
- `openspec/specs/decisions/` — 21 个决策记录（D01-D21）
- `openspec/specs/knowledge/` — 27 个已验证知识条目（K01-K27）
- `openspec/specs/references/` — 34 个参考索引（R01-R34）
- `openspec/specs/improvements/` — 10 个改进机会（I01-I10）
- `openspec/specs/polling-console-baseline/` — Console polling 能力 spec（新建）
- `openspec/changes/` — 活跃变更与归档
- `CLAUDE.md` — 公共规则单一来源
- `AGENTS.md` — 入口适配器

## 证据文件

- `docs/qemu_out.md` — 冻结 async QEMU 基线（SHA256 `d2f2486a...`）
- `docs/d1_out.md` — 冻结 async D1 基线（SHA256 `b98af673...`）
- `docs/qemu_console.md` — Console QEMU 正式日志（`backend=polling-console`，Done/exit 0）
- `docs/d1_console.md` — Console D1 真板日志（99.0-99.4% 线速，Done/exit 0）

## 迁移记录

2026-07-20：旧体系 spec 迁移至 M/D/K/R/I。Migration carrier: `openspec/changes/archive/mig-20260720-legacy-specs/`。
2026-07-21：`console-polling-baseline` 完成并归档。Migration carrier: `openspec/changes/archive/2026-07-21-console-polling-baseline/`。
