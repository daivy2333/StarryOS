## Why

QEMU UART 参数仍直接硬编码在 `kernel/src/drivers/uart_init.rs`：UART base `0x10000000`、stride `1`、raw LSR probe `base+5`、`iomap(..., 0x1000)`。这在 QEMU benchmark 阶段可接受，但会阻塞 Lichee RV Dock 和 VisionFive2 的后续适配，因为两块真板都需要把 UART kind、base、IRQ、register stride、MMIO access width 与 boot strategy 作为平台事实集中表达。

Q18 的目标是先做可在 QEMU 上验证的结构性前置：平台参数解耦和 early console 基础。它不启动 Lichee 真板，不生成 Android boot image，不接 PLIC/rootfs/async benchmark。

## What Changes

- 新增 StarryOS platform descriptor 或等价集中配置，表达 memory/kernel/console/interrupt/timer/boot 的平台事实。
- 将 QEMU UART facts 从 `uart_init.rs` 抽出到 QEMU descriptor，并保持现有 QEMU 行为。
- 新增 early console 抽象，作为真板 bring-up 的最小可观测输出层。
- 实现 QEMU `Ns16550U8EarlyConsole` baseline。
- 预留 `DwApbUart32EarlyConsole` 接口边界，但 Q18 不要求真板启动。
- 明确 async UART 初始化只消费 descriptor，不再新增板级硬编码。

## BDD Scenario Sketch

用户明确要求：Q18 现在可以做，但执行前停下等待审计。因此本 change 只推进到计划和规格，不进入 Phase 3 实现。

### Happy Path

- QEMU descriptor 复刻当前 QEMU UART facts。
- `uart_init.rs` 从 descriptor 读取 UART base/stride/IRQ，而不是自己声明板级常量。
- QEMU build/run 行为保持不变。
- early console 可在不依赖 async task、IRQ、PLIC、rootfs 的情况下输出。

### Sad Path

- 如果 descriptor 选择错误平台，构建期或启动早期应明确暴露平台名/console kind，而不是静默访问错误 MMIO。
- 如果 early console 不支持当前 platform，应显式返回/编译失败，不 fallback 到 QEMU 常量。

### Edge Cases

- Q18 只实现 QEMU U8 early console baseline；D1/VisionFive2 U32 访问模型只定义接口边界，真板验证留到 Q19/Q20。
- Q18 不改变 `/dev/console` 上层 TTY 行为。
- Q18 不修改 `uart_16550` backend 的 32-bit MMIO access width；该工作必须等 Q19 smoke test 或后续 async UART 平台化再决策。

## Non-Goals

- 不实现 Lichee RV Dock Android boot image 工具链。
- 不烧录、不运行真板 smoke test。
- 不接入 PLIC/timer/rootfs/USB/SDMMC/benchmark。
- 不修改 `uart_16550` 的 MMIO backend access width。
- 不调整 Q17 内存序修复范围。

## Capabilities

### New Capabilities

- `platform-descriptor-early-console`: 定义平台事实集中化、early console 抽象、QEMU 行为保持、真板前置边界。

### Modified Capabilities

- `optimization`: Q18/O74/O75 milestone 的实施载体。

## Impact

- `kernel/src/drivers/uart_init.rs` — 消费 descriptor，移除新增板级硬编码入口。
- `kernel/src/platform/` 或等价模块 — 新增 platform descriptor 与 early console 抽象。
- `kernel/src/drivers/ntty_async.rs` — 预期不改；上层 TTY 继续通过 `uart_init::driver()` 工作。
- `Makefile` / `Cargo.toml` — 仅在需要为 feature/platform 选择提供最小 glue 时改动。
- Tests / verification — QEMU build/run 行为保持；Q18 不要求真板验证。
