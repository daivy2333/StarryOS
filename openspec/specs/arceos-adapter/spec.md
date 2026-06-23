# arceos-adapter Specification

## Purpose
TBD - created by archiving change q13-async-uart-extraction. Update Purpose after archive.
## Requirements
### Requirement: ArceOS adapter implementation
The system SHALL implement the OS abstraction traits for ArceOS, including `UartPort::update_ier()` with internal IER caching. StarryOS SHALL delete its external `CACHED_IER`, `write_ier`, `enable_rx_intr`, and `enable_tx_intr` functions.

#### Scenario: ArceOsUartPort update_ier implementation
- **WHEN** `ArceOsUartPort::update_ier(set, clear)` is called
- **THEN** it SHALL atomically read its internal `AtomicU8` cache, apply set/clear bits, store back, and write the new value to UART MMIO via `self.uart.lock().set_ier()`

#### Scenario: ISR wrapper adapts to new handler signature
- **WHEN** the StarryOS ISR wrapper calls `uart_isr_handler`
- **THEN** it SHALL pass function pointers `fn_disable_rx` and `fn_disable_tx` that each call `port.update_ier()` on the shared `ArceOsUartPort` reference

### Requirement: StarryOS integration
The system SHALL use uart_16550's async implementation including the new completion API for tcdrain.

#### Scenario: Cargo.toml update
- **WHEN** StarryOS kernel/Cargo.toml is updated
- **THEN** it SHALL enable uart_16550's `async` feature

#### Scenario: tcdrain uses driver completion
- **WHEN** `TCSBRK` ioctl is invoked
- **THEN** the implementation SHALL poll `driver().tx_completion()` instead of directly accessing UART MMIO registers

### Requirement: Performance regression prevention
The system SHALL verify that the migration does not cause performance regression.

#### Scenario: Benchmark comparison
- **WHEN** benchmark is run after migration
- **THEN** performance SHALL be at least as good as Q12 baseline (1B avg latency ≤ 124µs)

#### Scenario: QEMU verification
- **WHEN** QEMU is run with migrated code
- **THEN** kernel SHALL boot normally and Shell interaction SHALL work

### Requirement: Initialization sequence
The system SHALL maintain the existing initialization sequence after migration.

#### Scenario: Hardware initialization
- **WHEN** `init_uart_hardware()` is called
- **THEN** it SHALL initialize UART hardware using uart_16550's sync API

#### Scenario: Driver initialization
- **WHEN** `AsyncUartDriver::init()` is called
- **THEN** it SHALL create ring buffers and spawn copier tasks using OsRuntime trait

#### Scenario: TTY binding
- **WHEN** `AsyncTty` is created
- **THEN** it SHALL use `AsyncUartReader` and `AsyncUartWriter` from uart_16550

