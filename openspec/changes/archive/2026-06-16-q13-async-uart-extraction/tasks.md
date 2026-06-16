## 1. OS 抽象 Trait 定义（uart_16550）

- [ ] 1.1 创建 `uart_16550/src/os/mod.rs` 模块声明
- [ ] 1.2 定义 `OsRuntime` trait（spawn + block_on）
- [ ] 1.3 定义 `OsIrq` trait（register_handler）
- [ ] 1.4 定义 `OsMmio` trait（map_mmio + phys_to_virt）
- [ ] 1.5 定义 `OsSpinNoIrq` trait（new + lock）
- [ ] 1.6 定义 `OsWakerSet` trait（register + wake）
- [ ] 1.7 更新 `uart_16550/src/lib.rs` 添加 os 模块导出

## 2. ISR Handler 迁移（uart_16550）

- [ ] 2.1 创建 `uart_16550/src/async_/mod.rs` 模块声明
- [ ] 2.2 创建 `uart_16550/src/async_/isr.rs` 迁移 ISR handler
- [ ] 2.3 定义 `RX_WAKER`, `TX_WAKER`, `DRAIN_WAKER` 静态变量
- [ ] 2.4 实现 `uart_isr_handler()` 函数（禁中断 + wake）

## 3. Ring Buffer 迁移（uart_16550）

- [ ] 3.1 创建 `uart_16550/src/async_/ring_buffer.rs`
- [ ] 3.2 实现 `RingBufRx`（embassy SPSC + OsWakerSet）
- [ ] 3.3 实现 `RingBufTx`（embassy SPSC + OsWakerSet）
- [ ] 3.4 添加 `push()`, `pop()`, `register_waker()` 方法

## 4. Copier Driver 迁移（uart_16550）

- [ ] 4.1 创建 `uart_16550/src/async_/driver.rs`
- [ ] 4.2 实现 `AsyncUartDriver` 结构体（rx + tx ring buffers）
- [ ] 4.3 实现 `rx_copier_loop()`（NAPI 中断合并）
- [ ] 4.4 实现 `tx_copier_loop()`（TX interleave 修复）
- [ ] 4.5 实现 `start_rx_copier()` 和 `start_tx_copier()`（使用 OsRuntime::spawn）

## 5. Device Ops 迁移（uart_16550）

- [ ] 5.1 创建 `uart_16550/src/async_/device_ops.rs`
- [ ] 5.2 实现 `AsyncUartReader`（TtyRead + embedded_io_async::Read）
- [ ] 5.3 实现 `AsyncUartWriter`（TtyWrite + embedded_io_async::Write）
- [ ] 5.4 添加 `embedded-io-async` 依赖到 Cargo.toml

## 6. Feature Gate 和依赖管理（uart_16550）

- [ ] 6.1 更新 `uart_16550/Cargo.toml` 添加 `async` feature
- [ ] 6.2 添加 `embassy-sync` 依赖（v0.6.2）
- [ ] 6.3 添加 `embassy-hal-internal` 依赖（v0.2）
- [ ] 6.4 添加 `embedded-io-async` 依赖（v0.6.1）
- [ ] 6.5 使用 `#[cfg(feature = "async")]` 条件编译 async 模块

## 7. ArceOS 适配层实现（StarryOS）

- [ ] 7.1 创建 `kernel/src/drivers/os_arceos.rs`
- [ ] 7.2 实现 `ArceOsRuntime`（axtask::spawn_with_name + block_on）
- [ ] 7.3 实现 `ArceOsIrq`（axhal::irq::register_irq_hook）
- [ ] 7.4 实现 `ArceOsMmio`（axhal::mem::phys_to_virt + axmm::iomap）
- [ ] 7.5 实现 `ArceOsSpinNoIrq`（kspin::SpinNoIrq）
- [ ] 7.6 实现 `ArceOsWakerSet`（axpoll::PollSet）

## 8. StarryOS 集成（StarryOS）

- [ ] 8.1 更新 `kernel/Cargo.toml` 启用 uart_16550 async feature
- [ ] 8.2 修改 `kernel/src/drivers/mod.rs` 导入 uart_16550 async 模块
- [ ] 8.3 修改 `kernel/src/drivers/ntty_async.rs` 使用 AsyncUartReader/Writer
- [ ] 8.4 删除 `kernel/src/drivers/isr.rs`
- [ ] 8.5 删除 `kernel/src/drivers/ring_buffer.rs`
- [ ] 8.6 删除 `kernel/src/drivers/async_driver.rs`
- [ ] 8.7 删除 `kernel/src/drivers/device_ops.rs`

## 9. 验证和测试

- [ ] 9.1 `cargo check` 0 错误（uart_16550）
- [ ] 9.2 `cargo check` 0 错误（StarryOS）
- [ ] 9.3 `cargo clippy` 0 警告（uart_16550）
- [ ] 9.4 `cargo clippy` 0 警告（StarryOS）
- [ ] 9.5 QEMU 启动验证（内核正常启动）
- [ ] 9.6 Shell 交互验证（输入输出正常）
- [ ] 9.7 benchmark 性能回归测试（对比 Q12 基线）
