# quality-gate-baseline Specification

## Purpose
定义 StarryOS 与内嵌 `uart_16550` 的分层质量 Gate 基线，包括 manifest 完整性、warning/clippy 清理、workspace lint 边界隔离、kernel pure-logic host test 和环境阻塞分类报告规则。
## Requirements
### Requirement: 内嵌 uart_16550 manifest 完整性

内嵌 `crates/uart_16550` MUST 保留其源码和 features 所需的 dependency、dev-dependency 与 resolver 配置；default、`async`、`embedded-io` 和 all-features 组合 MUST 具备可执行的编译 Gate。

#### Scenario: uart host tests use assert2

- **WHEN** `uart_16550` host unit tests 编译使用 `assert2` 的 constructor 和 MMIO tests
- **THEN** manifest MUST 提供对应 dev-dependency
- **AND** tests MUST NOT 因 unresolved `assert2` 失败

#### Scenario: embedded-io feature is enabled

- **WHEN** `uart_16550` 以 `embedded-io` 或 all-features 编译
- **THEN** feature MUST 激活 optional `embedded-io` dependency
- **AND** `src/embedded_io.rs` MUST compile without unresolved import errors

### Requirement: Feature-scoped imports 无 warning 且功能保持

只在 D1 user/fullbench 路径使用的 imports MUST 与使用点采用一致的 feature 条件；QEMU、D1 smoke 和 D1 kbench MUST NOT 因这些 imports 产生 unused-import warning。

#### Scenario: Non-fullbench build excludes feature-only imports

- **WHEN** 构建 QEMU、D1 smoke 或 D1 kbench
- **THEN** `alloc::vec`、`ROOT_FS_CONTEXT` 和 `Mountpoint` MUST NOT 作为未使用 import 编译

#### Scenario: D1 user or fullbench retains imports

- **WHEN** 构建 D1 userbench、fullbench 或 fullbench-command
- **THEN** `vec!` 与 memory-root 初始化所需类型 MUST remain available
- **AND** warning 清理 MUST NOT 删除或禁用对应 feature-only 功能

### Requirement: uart_16550 lint 边界独立

StarryOS MUST 将内嵌 `uart_16550` 作为独立质量检查 artifact；workspace lint MUST NOT 将 sibling package metadata 报告为驱动源码错误，且 StarryOS MUST 继续通过 path dependency 使用该驱动。

#### Scenario: Running root workspace lint

- **WHEN** 对 StarryOS workspace 执行 lint 或 metadata 检查
- **THEN** `uart_16550` 的 crate-level `clippy::cargo` MUST NOT 扩散到 `starryos`、`starry-kernel` 或 D1 platform package metadata

#### Scenario: Running independent uart gate

- **WHEN** `uart_16550` 不再由根 `--workspace` 自动检查
- **THEN** 质量矩阵 MUST 显式运行其 manifest check、test 和 clippy 命令
- **AND** StarryOS path dependency MUST continue to resolve to `crates/uart_16550`

### Requirement: Telemetry 开关两侧 API 与 lint 一致

Telemetry compatibility APIs MUST 在 feature 开启和关闭时保持相同的可调用接口；telemetry-off no-op 实现 MUST satisfy the configured clippy policy without changing runtime behavior.

#### Scenario: Building async uart without telemetry

- **WHEN** `uart_16550` 使用 `async` 且未启用 `telemetry`
- **THEN** `record_tx_push()` 和 `reset_tx_debug()` MUST compile without `missing_const_for_fn`
- **AND** they MUST remain side-effect-free no-op APIs

#### Scenario: Building async uart with telemetry

- **WHEN** `telemetry` feature 已启用
- **THEN** counter recording and reset behavior MUST remain unchanged

### Requirement: Kernel 测试与环境 Gate 分层

质量矩阵 MUST 区分 reusable driver host tests、kernel target/feature builds、kernel pure-logic host tests 和 QEMU/真板系统验证；环境失败 MUST 与源码失败分开报告。

#### Scenario: Running early-console pure-logic tests

- **WHEN** 执行最小 early-console host harness
- **THEN** harness MUST 引用真实 `kernel/src/platform/console.rs` 和 `early_console.rs`
- **AND** 现有 6 个 pure-logic tests MUST pass without compiling the complete kernel

#### Scenario: Running bare kernel host tests

- **WHEN** 裸 `cargo test -p starry-kernel --lib` 仍缺少平台和 optional dependency feature 上下文
- **THEN** it MUST be classified as an unsupported Gate
- **AND** the change MUST NOT enable all device dependencies unconditionally merely to suppress the failure

#### Scenario: Target build lacks environment prerequisites

- **WHEN** QEMU/D1 target build 缺少 musl 工具链、`PLAT_CONFIG`，或受限环境禁止 C 编译
- **THEN** result MUST be reported as ENV BLOCK with the failing prerequisite
- **AND** it MUST NOT be reported as source PASS or source regression

#### Scenario: Runtime-sensitive behavior validation

- **WHEN** 验证 IRQ、TTY、rootfs、用户进程或 UART 数据路径行为
- **THEN** validation MUST use supported QEMU or real-board entry points
- **AND** host unit tests MUST NOT substitute for runtime evidence

