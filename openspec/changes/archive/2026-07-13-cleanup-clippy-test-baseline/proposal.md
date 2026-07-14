## Why

StarryOS 当前的 warning、clippy 与测试失败混合了真实源码 lint、内嵌 `uart_16550` manifest 漂移、Cargo workspace 作用域泄漏、无效 feature 组合和外部构建环境问题，导致质量 Gate 不能稳定区分产品回归与配置噪音。现在需要先恢复可重复的分层基线，才能让后续异步串口和 kernel 修改获得可信的自动化见证。

## What Changes

- 恢复 `crates/uart_16550/Cargo.toml` 与相邻 canonical crate 的必要 manifest parity：resolver、`embedded-io` optional dependency wiring 和 `assert2` dev-dependency。
- 将 `vec`、`ROOT_FS_CONTEXT`、`Mountpoint` import 限定到实际使用它们的 D1 user/fullbench features，消除 QEMU、D1 smoke 和 kbench 的 unused-import warning，不删除 feature-only 功能。
- 清理 telemetry 关闭路径的真实 clippy lint，同时保持 telemetry 开关两侧 API 一致。
- 将 `crates/uart_16550` 从 StarryOS workspace lint 边界隔离；StarryOS 继续通过 path dependency 使用该 crate，并显式运行独立的 driver check/test/clippy Gate。
- 定义 reusable driver、kernel target build、kernel pure-logic test 和系统运行验证四层质量 Gate；裸 host `cargo test -p starry-kernel` 在建立最小 host-test 边界前不作为全内核 Gate。
- 将 musl 工具链缺失、受限环境 `Bad system call` 和用户级 Cargo config deprecation 与源码失败分开报告。

## BDD Scenario Sketch

用户于 2026-07-13 确认采用默认假设，并批准隔离 `uart_16550` workspace lint 边界。

### Happy Path

- 独立 `uart_16550` default/async/embedded-io/all-features 能编译，host tests 和 clippy 通过。
- StarryOS 支持的 QEMU/D1 构建入口不再报告三个 feature-scoped unused import。
- workspace clippy 不再把 sibling package metadata 归因于 `uart_16550`。

### Sad Path

- manifest 再次漏掉 feature dependency 或 dev-dependency 时，对应 compile/test Gate 必须失败并指出缺失依赖。
- 缺少 musl 工具链、平台配置或执行环境禁止 C 编译时，Gate 必须报告 ENV BLOCK，不得标记源码回归或伪称通过。
- 裸 host kernel test 仍编译整套平台/VFS/syscall 模块时，必须明确记录为 unsupported Gate，不能靠 blanket `allow` 或无条件启用所有设备依赖绕过。

### Edge

- telemetry 关闭时 no-op API 必须保持可调用且无 clippy error；telemetry 开启时计数行为不变。
- workspace 隔离后，根级 `--workspace` 不再自动覆盖 `uart_16550`，验证矩阵必须显式运行其 manifest 命令。
- D1 user/fullbench 仍必须能访问 `vec!`、`ROOT_FS_CONTEXT` 和 `Mountpoint`；消除 warning 不得删除这些 feature-only 路径。

## Capabilities

### New Capabilities

- `quality-gate-baseline`: 定义 StarryOS 与内嵌 `uart_16550` 的 lint/test 分层、受支持 feature 覆盖、workspace 边界和环境阻塞分类。

### Modified Capabilities

<!-- 无现有运行时 capability 行为变化。 -->

## Impact

- `Cargo.toml`：明确 workspace exclude 与独立 crate Gate。
- `crates/uart_16550/Cargo.toml`：恢复 manifest parity。
- `crates/uart_16550/src/async_/driver.rs`：清理 telemetry-off no-op lint，不改变数据路径。
- `kernel/src/entry.rs`、`kernel/src/pseudofs/mod.rs`：精确限定 feature-only imports。
- 构建/验证流程：新增分层命令矩阵；QEMU/D1 仍使用对应 target、feature、`PLAT_CONFIG` 和 musl 前置条件。
- 不修改 UART ISR、ring、copier、IER、TTY、`tcdrain` 或 benchmark 运行时语义。
