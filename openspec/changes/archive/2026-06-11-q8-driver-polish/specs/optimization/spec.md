# Delta Spec — Q8 驱动引擎打磨

## MODIFIED Requirements

### Requirement: NAPI 中断合并 — 添加退出条件

Q5 阶段实现的 NAPI 中断合并（O2/O34）MUST 在零字节读取时退出轮询模式，恢复中断驱动。

**Previously**: NAPI 模式下 `consecutive` 只增不减，中断永久禁用，零字节时 CPU 空转轮询空 FIFO。

**Now**: 零字节读取时重置 `consecutive = 0` 并调用 `enable_rx_intr()` 恢复 RX 中断。

#### Scenario: NAPI 模式下数据流停止

- **WHEN** RX copier 在 NAPI 模式（consecutive ≥ NAPI_THRESHOLD）且 `receive_bytes()` 返回 0
- **THEN** consecutive 重置为 0，enable_rx_intr() 被调用
- **AND** 下次 ISR 正常触发

#### Scenario: NAPI 模式下数据持续

- **WHEN** RX copier 在 NAPI 模式且 `receive_bytes()` 返回 > 0
- **THEN** consecutive 继续递增
- **AND** RX 中断保持禁用（轮询模式）

### Requirement: ISR 极简原则 — 消除锁操作

ISR handler MUST 不获取任何锁。当前 `uart_instance().lock()` 违规，MUST 改为无锁读取。

**Previously**: `isr.rs:10` 获取 `SpinNoIrq` 锁来调用 `uart.isr()`。

**Now**: 使用无锁方式读取 ISR 寄存器（单 ISR 上下文安全）。

#### Scenario: RX 中断触发

- **WHEN** UART 产生 RX 中断
- **THEN** ISR 无锁读取 ISR 寄存器
- **AND** 禁用 RX 中断
- **AND** 唤醒 RX_WAKER
- **AND** ISR 在 2 µs 内返回

### Requirement: MMIO 封装 — IER 路径规范化

所有 MMIO 寄存器写操作 MUST 通过 `uart_16550` crate 安全 API。当前 `write_ier()` 的裸 `write_volatile` 违规，MUST 替换。

**Previously**: `uart_init.rs:72` 使用 `core::ptr::write_volatile()` 裸写 IER 寄存器。

**Now**: 通过 `uart_16550::Uart16550::set_ier()` 方法写入。

#### Scenario: 使能 RX 中断

- **WHEN** copier 调用 `enable_rx_intr()`
- **THEN** IER 通过 uart_16550 API 写入（非裸 write_volatile）
- **AND** CACHED_IER 与硬件 IER 一致

---

## ADDED Requirements

### Requirement: copier waker 去重优化

copier 的 waker 去重逻辑 SHALL 减少不必要的 `Waker::clone()` 调用。仅在 waker 变化时才 clone + register。

#### Scenario: waker 未变化

- **WHEN** `poll_fn` 被同一个 task 重复轮询
- **THEN** 仅执行 `will_wake()` 检查
- **AND** 不调用 `Waker::clone()` 和 `AtomicWaker::register()`

### Requirement: DRAIN_WAKER 条件唤醒

DRAIN_WAKER SHALL 仅在 tcdrain 活跃时触发，减少不必要的原子操作。

#### Scenario: tcdrain 未等待

- **WHEN** TX ISR 触发但无进程在等待 tcdrain
- **THEN** DRAIN_WAKER.wake() 不被调用

#### Scenario: tcdrain 正在等待

- **WHEN** TX ISR 触发且有进程在等待 tcdrain
- **THEN** DRAIN_WAKER.wake() 被调用

### Requirement: PollSet→AtomicWaker 迁移

pipe / signalfd / pidfd / event 的 PollSet 唤醒机制 SHALL 替换为 AtomicWaker 静态分发模式，以降低唤醒延迟和维护复杂度。

#### Scenario: pipe 读端等待数据

- **WHEN** 进程在空 pipe 上调用 `read()`
- **THEN** waker 注册到 AtomicWaker（非 PollSet）
- **AND** 写端写入后 AtomicWaker::wake() 唤醒读端
- **AND** 唤醒延迟 ~50ns（PollSet ~200ns）

#### Scenario: pidfd 等待进程退出

- **WHEN** 进程在 pidfd 上调用 `poll()`
- **THEN** waker 注册到 AtomicWaker（非 PollSet）
- **AND** 目标进程退出时 AtomicWaker::wake() 唤醒
- **AND** async 模型保证单 waiter（默认假设）
