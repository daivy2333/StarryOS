## Why

Q31 已为 Async UART 建立可审计的 CPU-efficiency 测量口径，但当前
`console-lichee` 的 benchmark 与 D1 时间换算仍不能产生同口径数据。需要在不改变
Console 运行时语义的前提下移植测量能力，才能对 Console 与 Async 结果做有效横向比较。

## What Changes

- 修正 D1 benchmark 的 cycle-to-time 换算，并用纯函数边界测试固定 24 MHz 语义。
- 修复 D1 Console runtime 的 IRQ feature wiring，使 supervisor timer 使用真实 handler；smoke
  模式继续使用 IRQ stub，polling Console 不启用 UART IRQ 数据路径。
- 将 Q31 的 S11、S41、S42、S43 workload、完成点、raw 字段和输出完整性规则移植到
  Console benchmark。
- 为 Console 后端显式适配能力差异：同步 write、零 overlap、无 UART TX 诊断计数器等，
  不可用数据输出 `not-available` 或 `not-applicable`，不伪造零值。
- 分别采集 QEMU 与 D1 Console 证据，并记录源码、binary、image、命令和日志 provenance。
- 以 Q31 冻结的 Async 日志与源码 hash 为比较基线，生成同 workload、同完成点的横向报告。
- 不修改 Console writer、TTY、polling、锁、flush/drain 或调度语义，也不设定性能胜负阈值。

### BDD Scenario Sketches

- **Happy Path**：给定 Q31 口径和可用的 Console `/proc/instret`，执行 S41/S42/S43 后，
  输出完整 raw 字段、汇总与终止标记，并能与冻结的 Async 样本逐项比较。
- **Sad Path**：给定 `instret`、TX 诊断或 overlap 能力不可用，执行对应 section 后，输出
  明确的能力状态并排除不成立的派生值，不把缺失数据写成零。
- **Edge Case**：给定同步 Console write 已覆盖整个发送窗口，S42 接受零 overlap；S43 loaded
  timer 无真实 overlap 时标为 `not-applicable`，不计算虚假的 loaded aggregate。
- **Error/Timeout/Cancel**：给定短写、drain 失败、deadline 超时或样本未完成，当前样本不得
  进入汇总，日志必须留下失败边界，后续 section 或整次运行按既定策略终止。
- **Compatibility**：给定原有 Console benchmark section，移植后已有 workload 仍可运行，
  且产品代码中的 Console 数据路径与同步语义保持不变。

## Capabilities

### New Capabilities

- `console-cpu-efficiency-benchmark`：定义 Console 侧同口径 CPU-work、overlap、timer overshoot、
  provenance 和跨 Q31 比较能力。

### Modified Capabilities

无。

## Impact

- 测试代码：`tests/benchmark.c`。
- D1 平台时间代码：`crates/axplat-riscv64-lichee-d1/src/time.rs` 与新增/移植的
  `time_math.rs`。
- D1 runtime feature：根 `Cargo.toml` 的 Lichee runtime 组合；复用现有平台 `irq.rs`，
  不修改 timer syscall 或 Console writer。
- 构建产物与证据：`tests/benchmark`、`kernel/resources/benchmark.elf`、
  `starry-lichee-fullbench-command-boot.img`、`.claude/analysis/q32-console-cpu-efficiency-evidence/`。
- 上游只读依赖：Q31 Async 源码 hash 和 `.claude/analysis/q31-cpu-efficiency-evidence/async/`。
- 不新增外部依赖，不改变 syscall、Console API、内核调度或 UART 产品路径。
