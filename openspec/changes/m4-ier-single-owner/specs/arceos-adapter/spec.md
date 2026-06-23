## MODIFIED Requirements

### Requirement: ArceOS adapter implementation
The system SHALL implement the OS abstraction traits for ArceOS, including `UartPort::update_ier()` with internal IER caching. StarryOS SHALL delete its external `CACHED_IER`, `write_ier`, `enable_rx_intr`, and `enable_tx_intr` functions.

#### Scenario: ArceOsUartPort update_ier implementation
- **WHEN** `ArceOsUartPort::update_ier(set, clear)` is called
- **THEN** it SHALL atomically read its internal `AtomicU8` cache, apply set/clear bits, store back, and write the new value to UART MMIO via `self.uart.lock().set_ier()`

#### Scenario: ISR wrapper adapts to new handler signature
- **WHEN** the StarryOS ISR wrapper calls `uart_isr_handler`
- **THEN** it SHALL pass function pointers `fn_disable_rx` and `fn_disable_tx` that each call `port.update_ier()` on the shared `ArceOsUartPort` reference
