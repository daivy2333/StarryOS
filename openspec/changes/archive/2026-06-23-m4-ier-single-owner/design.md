## Context

M4 是 Q15 增量重融的架构清理步骤：将 IER 从 StarryOS/uart_16550 双 Owner 变为 uart_16550 UartPort 单 Owner。这使 uart_16550 成为真正独立的可复用异步 UART crate。

## Goals / Non-Goals

**Goals:**
- `UartPort::update_ier(set, clear)` 成为 IER 唯一修改入口
- copier 不再需要 `enable_rx_intr`/`enable_tx_intr` 回调参数
- ISR 不再依赖外部 `cached_ier`，通过函数指针委托给 port
- StarryOS 删除 CACHED_IER/write_ier/enable_* 函数

**Non-Goals:**
- 不改 M1-M3 的性能或正确性行为
- 不改变 ISR 极简原则
- 不移除 `IsrRegisters`（保留 ISR 读寄存器功能，只删除 IER 写）

## Decisions

### D1: IER 缓存放在 ArceOsUartPort 内部

**选择**: `ArceOsUartPort` 内部持有 `ier_cache: AtomicU8`，`update_ier` 读-改-写缓存 + MMIO 写

**理由**: IER 是 port 的内部状态，不应暴露给驱动层。`AtomicU8` 保证 ISR 和 copier 间无竞态。

### D2: ISR 使用函数指针而非泛型

**选择**: ISR handler 签名 `pub fn uart_isr_handler(_irq: usize, base: NonNull<u8>, fn_disable_rx: fn(), fn_disable_tx: fn())`

**理由**: IRQ hook 是 `fn(usize)` 函数指针，不支持泛型。函数指针方案与现有回调模式一致，零开销（编译期单态化）。StarryOS wrapper 通过闭包捕获 port 引用。

### D3: copier 移除回调，改用 UartPort 方法

**选择**: `start_tx_copier(&'static self)` 不再接收 `enable_tx_intr: fn()` 参数。`tx_copier_loop` 直接调用 `self.uart.update_ier(IER::THR_EMPTY, IER::empty())`。

**理由**: port 已是 `&'static`，copier 可直接访问。减少 API 参数，降低新 OS 移植认知负担。

## Risks / Trade-offs

| Risk | Mitigation |
|------|------------|
| ISR 函数指针调用开销 | 零开销（编译期静态分发到具体 port impl）|
| update_ier 内锁与 ISR 死锁 | ArceOsUartPort 内 `SpinNoIrq` 已禁中断，ISR 不会重入锁 |
| 移除 IsrRegisters::disable_* 影响其他调用者 | 当前仅 ISR 调用，无其他调用者 |
