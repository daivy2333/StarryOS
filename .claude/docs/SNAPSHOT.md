# SNAPSHOT.md — 项目快照

> Last updated: 2026-07-25
> Branch: uart-lichee — Q31/Q32 CPU-efficiency 对照、报告已生成并归档；Q24 等待 SMP 硬件；Q30 维持证据触发

## 项目概览

- **项目**: StarryOS — 基于 RISC-V 的宏内核 OS（Rust / ArceOS 组件化架构）
- **技术栈**: Rust nightly-2026-02-25 / RISC-V 64-bit / ArceOS 0.3.0-preview.2 / `axtask::future`
- **构建**: Makefile (`make build`, `make run`, `make lichee-userbench`, `make lichee-fullbench-command`)
- **测试**: QEMU virt + Lichee RV Dock D1 真板
- **格式化/Lint**: `cargo fmt` + `cargo clippy`
- **源码目录**: `kernel/`, `crates/uart_16550/`, `crates/axplat-riscv64-lichee-d1/`

## 当前分支

`uart-lichee` — 异步 UART 开发主线。Q0~Q18（spike/驱动/VFS/Shell/性能/文档/QEMU修复/平台解耦）、Q19~Q23（D1 bring-up/benchmark/决策）、Q27a/Q27/Q28/Q29（readiness/backpressure/writer/reader 契约）已全部完成并归档。Q26 维护性清理已归档。

## 近期完成

- Q17：QEMU 修复完成；ier_cache RMW 临界区化，TX completion 原子序升级。多 hart 未实测。
- Q19~Q23：D1 smoke/kbench/userbench/memory-root、Q20 jitter/S40/raw evidence 完成。Q21/Q22 取消当前规划。
- Q27a/Q27/Q28/Q29：readiness/backpressure/writer 契约/reader 契约全部完成并归档。QEMU/D1 通过。
- Q26：维护性清理已归档。
- Q31/Q32：Async 与 Console CPU 效率同口径 D1 benchmark 对照完成并归档（Q31: `2026-07-22-q31-async-uart-cpu-efficiency-benchmark`，Q32: `2026-07-22-q32-console-cpu-efficiency-benchmark`）。交叉对比报告 `docs/benchmark-report-async.md` 已生成。Q31 Async 证据冻结（SHA-256 `a9ce8a34...`/`50a2a876...`），Q32 Console 证据同步自 `console-lichee` 分支。

## 当前待推进

- Q24：VisionFive2 或等价 SMP 环境复验 O63，覆盖跨 hart write/flush/tcdrain、read 与 IER enable/disable。
- Q30：仅在 Q24 或真实 workload 提供消息原子性、公平性、锁竞争证据时规划。

## 关键事实

| 主题 | 结论 |
|---|---|
| QEMU benchmark | `make run` 进入 rootfs，`/bin/benchmark` 适合功能/回归验证，不适合声明真板线速 |
| D1 async UART | 96.6%-99.1% 线速，D1 真板 fullbench-command Done/exit 0/drain_errors=0 |
| D1 Console polling | 99.0-99.4% 线速（`console-lichee` 分支，仅供参考对比） |
| Writer 契约 | `AsyncUartWriter` unsafe 唯一构造，`RingBufTx` crate-private，SPSC 安全 |
| Reader 契约 | `AsyncUartReader` unsafe 唯一构造，`RingBufRx` crate-private，copier 单次启动 |
| TX backpressure | 阻塞 fd writable wait + 非阻塞 partial/WouldBlock，ONLCR 完整映射 |
| Port init | attach-only + width-correct IER disable，不重写 U-Boot 配置 |
| TX lock | `axplat::console::CONSOLE_LOCK` → local `CONSOLE_PORT`，drain 单次持锁到 TEMT |
| D1 CPU-efficiency | Async S41 inst/byte: 32818/32792/44716 (64/256/1024B)；Console: 1194/1105/1106。S42 Async ovlp 0.54，Console 0.00。S43 idle: Async 9.5ms，Console 8.4ms。S43 loaded: Async 25.8ms，Console not-applicable。QEMU 不作硬件证据。 |

## OpenSpec 体系

- `openspec/specs/project-model/` — 40 个当前有效跨模块约束（M01-M40）
- `openspec/specs/decisions/` — 21 个决策记录（D01-D21）
- `openspec/specs/knowledge/` — 27 个已验证知识条目（K01-K27）
- `openspec/specs/references/` — 34 个参考索引（R01-R42）
- `openspec/specs/improvements/` — 11 个改进机会（I01-I10, I12）
- `openspec/changes/` — 活跃变更（q17-smp-memory-ordering）与归档
- `CLAUDE.md` — 公共规则单一来源
- `AGENTS.md` — 入口适配器

## 证据文件

- `docs/qemu_out.md` — 冻结 async QEMU 基线（SHA256 `d2f2486a...`）
- `docs/d1_out.md` — 冻结 async D1 基线（SHA256 `b98af673...`）
- `docs/benchmark-report-async.md` — async UART 与 polling Console 交叉对比报告

## 迁移记录

2026-07-20：旧体系 spec 迁移至 M/D/K/R/I。Migration carrier: `openspec/changes/archive/mig-20260720-legacy-specs/`。
2026-07-21：从 `console-lichee` 同步文档体系（分析归档、Runbook 通用化、改进/参考去 console 专属内容）。
