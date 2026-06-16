## Why

StarryOS 已完成 Q0~Q12 共 ~618 行异步串口实现，Q13 Phase 1 已将 TtyRead/TtyWrite trait 提取到 uart_16550 crate。现在需要完成 Phase 2-3：将核心异步逻辑（ISR、ring buffer、copier、device_ops）迁移到 uart_16550，使其成为可复用的异步 UART crate，适用于任何 Rust RISC-V OS 项目。

**为什么现在做**：
1. Q12 已完成基础设施迁移（atomic_ring_buffer + embedded_io_async + TC tcdrain）
2. Phase 1 trait 提取已验证可行性
3. 代码量可控（核心异步逻辑 ~400 行）
4. 其他 OS 项目（Linux kernel module, Tock capsule, RTIC driver）也需要异步 UART

## What Changes

- **新增**：uart_16550 crate 的 `async` feature gate
- **新增**：5 个 OS 抽象 trait（OsRuntime, OsIrq, OsMmio, OsSpinNoIrq, OsWakerSet）
- **新增**：uart_16550/src/async_/ 模块（isr.rs, ring_buffer.rs, driver.rs, device_ops.rs）
- **新增**：StarryOS 适配层（kernel/src/drivers/os_arceos.rs）
- **修改**：StarryOS kernel/Cargo.toml 启用 uart_16550 async feature
- **删除**：StarryOS 本地实现（isr.rs, ring_buffer.rs, async_driver.rs, device_ops.rs）
- **保留**：StarryOS uart_init.rs（硬件初始化）+ ntty_async.rs（TTY 框架绑定）

## Capabilities

### New Capabilities

- `async-uart-traits`: 5 个 OS 抽象 trait 定义（OsRuntime, OsIrq, OsMmio, OsSpinNoIrq, OsWakerSet）
- `async-uart-core`: 核心异步逻辑迁移（ISR handler, ring buffer, copier driver, device_ops）
- `arceos-adapter`: StarryOS 适配层实现（ArceOsRuntime, ArceOsIrq, ArceOsMmio 等）

### Modified Capabilities

- `tty-traits`: Phase 1 已完成的 TtyRead/TtyWrite trait，Phase 2-3 不再修改

## Impact

**受影响的代码**：
- `uart_16550/src/` — 新增 async_ 模块 + os trait 定义
- `uart_16550/Cargo.toml` — 新增 async feature + 依赖
- `kernel/src/drivers/` — 删除 4 个文件，新增 1 个适配层文件
- `kernel/Cargo.toml` — 修改 uart_16550 依赖启用 async feature

**受影响的 API**：
- uart_16550 公共 API 新增 `AsyncUartDriver`, `RingBufRx`, `RingBufTx`, `AsyncUartReader`, `AsyncUartWriter`
- StarryOS 内部 API 保持不变（通过适配层桥接）

**依赖变化**：
- uart_16550 新增依赖：embassy-sync, embassy-hal-internal, embedded-io-async
- StarryOS 依赖不变（uart_16550 path 依赖，启用 async feature）
