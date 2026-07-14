## 1. Current-State Witness

- [x] 1.1 记录工作树状态，并复现 `uart_16550` async check PASS 与 host test 因 17 个 unresolved `assert2` errors 失败
- [x] 1.2 复现 `uart_16550 --all-features --lib` 因 `embedded-io` dependency 未连接而失败
- [x] 1.3 复现 async-without-telemetry clippy 的两个 `missing_const_for_fn`，并保存 workspace sibling metadata lint 见证
- [x] 1.4 记录三个 unused import 见证，以及 QEMU target build 在当前环境的 PASS 或明确 ENV BLOCK

## 2. Restore uart_16550 Manifest Contract

- [x] 2.1 在 `crates/uart_16550/Cargo.toml` 恢复 `resolver = "3"`、optional `embedded-io = "0.7"` 和 `assert2 = "0.4.0"` dev-dependency
- [x] 2.2 将 `embedded-io` feature 连接到 `dep:embedded-io`，验证 default、`async`、`embedded-io` 和 all-features 均可编译
- [x] 2.3 运行 `uart_16550` default 与 async host tests，确认 constructor/MMIO tests 不再因缺失依赖失败

## 3. Source Warning and Clippy Cleanup

- [x] 3.1 将 `kernel/src/entry.rs` 的 `alloc::vec` import 限定到 D1 userbench/fullbench/fullbench-command features
- [x] 3.2 将 `kernel/src/pseudofs/mod.rs` 的 `ROOT_FS_CONTEXT` 与 `Mountpoint` imports 限定到相同的 memory-root features
- [x] 3.3 将 telemetry-off `record_tx_push()` 与 `reset_tx_debug()` 调整为满足 `missing_const_for_fn`，保持 telemetry-on 实现不变
- [x] 3.4 运行 rustfmt，并对 warning 修改执行 feature 使用点审查，确认没有删除 D1 memory-root 或 benchmark 路径

## 4. Isolate Driver Lint Ownership

- [x] 4.1 在根 workspace `exclude` 中加入 `crates/uart_16550`，确认 `cargo metadata` 不再将其列为根 workspace member
- [x] 4.2 确认 `starry-kernel` 的 path dependency 仍解析到仓库内 `crates/uart_16550`
- [x] 4.3 独立运行 driver async-without-telemetry 与 all-features/all-targets clippy，要求不再报告 sibling package metadata 或源码 lint

## 5. Minimal Kernel Pure-Logic Test Boundary

- [x] 5.1 新增 early-console host harness，通过 `#[path]` 引用真实 `kernel/src/platform/console.rs` 与 `early_console.rs`，只在 harness 局部允许 subset dead-code warning
- [x] 5.2 新增稳定的 Makefile host-test target，使用 `rustc --test` 构建到 `/tmp` 并执行测试
- [x] 5.3 运行 host-test target，要求现有 6 个 early-console tests 全部通过

## 6. Gate Verification

- [x] 6.1 运行 `cargo fmt --all -- --check` 和 `git diff --check`
- [x] 6.2 运行独立 `uart_16550` check/test/clippy 矩阵：default、async、embedded-io、all-features、telemetry off/on
- [x] 6.3 运行 StarryOS QEMU 与 D1 smoke/kbench 的有效 target/feature 构建入口，确认三个 unused import 不再出现；无法满足外部前置条件时逐项记录 ENV BLOCK
- [x] 6.4 对 D1 userbench/fullbench/fullbench-command 做 compile/build Gate，确认 feature-only imports 仍可用；外部编译或真板前置条件缺失时逐项记录 ENV BLOCK
- [x] 6.5 记录裸 `cargo test -p starry-kernel --lib` 为 unsupported Gate，并确认本 change 未通过 blanket allow 或无条件设备依赖掩盖它
- [x] 6.6 运行 `openspec validate cleanup-clippy-test-baseline --strict` 和 `openspec validate --specs`，汇总 PASS、SOURCE FAIL、ENV BLOCK、UNSUPPORTED 证据

## 7. Review and Closeout

- [x] 7.1 Review 确认 UART ISR、ring、copier、IER、TTY、drain 和 benchmark 运行时语义没有变化
- [x] 7.2 核对 `quality-gate-baseline` 每条 requirement 均有对应任务和验证证据，无未批准简化
- [x] 7.3 Gate 5 全部可执行检查通过且环境阻塞已明确分类后，准备归档 change
