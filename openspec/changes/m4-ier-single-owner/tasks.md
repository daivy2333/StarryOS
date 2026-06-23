## 1. uart_16550 UartPort trait 扩展

- [ ] 1.1 在 `UartPort` trait 新增 `fn update_ier(&self, set: IER, clear: IER)` 方法

## 2. uart_16550 copier 去回调化

- [ ] 2.1 `start_rx_copier` / `rx_copier_loop` 移除 `enable_rx_intr: fn()` 参数，改用 `self.uart.update_ier()`
- [ ] 2.2 `start_tx_copier` / `tx_copier_loop` 移除 `enable_tx_intr: fn()` 参数，改用 `self.uart.update_ier()`

## 3. uart_16550 ISR 重构

- [ ] 3.1 `uart_isr_handler` 签名改为 `(_irq, base, fn_disable_rx: fn(), fn_disable_tx: fn())`
- [ ] 3.2 移除 `IsrRegisters::disable_rx_intr` 和 `disable_tx_intr` 方法
- [ ] 3.3 ISR 中用调用函数指针替代 `regs.disable_*_intr(cached_ier)`

## 4. StarryOS 适配

- [ ] 4.1 `ArceOsUartPort` 新增 `ier_cache: AtomicU8` 字段 + `update_ier()` 实现
- [ ] 4.2 删除 `CACHED_IER`、`write_ier()`、`enable_rx_intr()`、`enable_tx_intr()`
- [ ] 4.3 ISR wrapper 适配新签名：传入 `|| port.update_ier(...)` 闭包
- [ ] 4.4 `init_uart` 中 copier 启动调用移除回调参数

## 5. 验证

- [ ] 5.1 `cargo check --features async` on uart_16550
- [ ] 5.2 `cargo test --features async` on uart_16550
- [ ] 5.3 `cargo check -p starry-kernel` on StarryOS
- [ ] 5.4 QEMU `make build ARCH=riscv64` + `make run` 启动验证
