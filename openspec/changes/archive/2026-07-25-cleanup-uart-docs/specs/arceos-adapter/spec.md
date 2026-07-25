# arceos-adapter Specification

## Purpose
Archived capability spec from Q13 async-uart extraction and Q15 follow-up integration. Current StarryOS uses a 2-trait OS adapter (`OsRuntime` + `OsWakerSet`) plus `UartPort`; Q17 will further harden `ArceOsUartPort::update_ier()` for SMP-safe memory ordering.
## Requirements
### Requirement: ArceOS adapter implementation
The system SHALL implement the current OS abstraction traits for ArceOS (`OsRuntime` + `OsWakerSet`) and SHALL expose UART hardware operations through `UartPort::update_ier()` with internal IER ownership. StarryOS SHALL delete its external `CACHED_IER`, `write_ier`, `enable_rx_intr`, and `enable_tx_intr` functions.

#### Scenario: ArceOsUartPort update_ier implementation
- **WHEN** `ArceOsUartPort::update_ier(set, clear)` is called
- **THEN** it SHALL manage IER state internally and write the new value to UART MMIO via `self.uart.lock().set_ier()`
- **AND** Q17 SHALL harden the cache update against SMP RMW races identified by O63

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
- **THEN** performance SHALL satisfy the Q15 Manual QA baseline: no 64B write+tcdrain backpressure regression, FIONBIO PASS, and 1B e2e latency within the documented QEMU noise range

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
