# async-uart-core Specification

## Purpose
TBD - created by archiving change q13-async-uart-extraction. Update Purpose after archive.
## Requirements
### Requirement: ISR handler migration
The system SHALL migrate the ISR handler from StarryOS to uart_16550, using AtomicWaker for ISR-safe wake-up.

#### Scenario: RX interrupt handling
- **WHEN** a RX interrupt fires
- **THEN** the ISR handler SHALL disable RX interrupt and wake RX_WAKER

#### Scenario: TX interrupt handling
- **WHEN** a TX interrupt fires
- **THEN** the ISR handler SHALL disable TX interrupt and wake TX_WAKER and DRAIN_WAKER

### Requirement: Ring buffer migration
The system SHALL migrate the ring buffer implementation from StarryOS to uart_16550, using embassy SPSC and OsWakerSet trait.

#### Scenario: RX ring buffer push
- **WHEN** `RingBufRx::push()` is called with data
- **THEN** the ring buffer SHALL store the data and wake registered wakers via OsWakerSet

#### Scenario: TX ring buffer pop
- **WHEN** `RingBufTx::pop()` is called with a buffer
- **THEN** the ring buffer SHALL remove data from the buffer and return the number of bytes removed

### Requirement: Copier driver migration
The system SHALL migrate the copier driver from StarryOS to uart_16550, using OsRuntime trait for task spawning. The copier SHALL use `UartPort::update_ier()` for all IER manipulation instead of receiving external callback functions.

#### Scenario: RX copier loop
- **WHEN** `AsyncUartDriver::start_rx_copier()` is called
- **THEN** the system SHALL spawn a new task that continuously reads from UART and pushes to RX ring buffer. When re-enabling RX interrupts, the copier SHALL call `self.uart.update_ier(IER::DATA_READY, IER::empty())`.

#### Scenario: TX copier loop
- **WHEN** `AsyncUartDriver::start_tx_copier()` is called
- **THEN** the system SHALL spawn a new task that continuously pops from TX ring buffer and writes to UART. When re-enabling TX interrupts, the copier SHALL call `self.uart.update_ier(IER::THR_EMPTY, IER::empty())`.

### Requirement: Device ops migration
The system SHALL migrate the device ops from StarryOS to uart_16550, implementing embedded_io_async traits.

#### Scenario: AsyncUartReader read
- **WHEN** `AsyncUartReader::read()` is called with a buffer
- **THEN** the reader SHALL pop data from RX ring buffer and return the number of bytes read

#### Scenario: AsyncUartWriter write
- **WHEN** `AsyncUartWriter::write()` is called with data
- **THEN** the writer SHALL push data to TX ring buffer and return the number of bytes written

#### Scenario: embedded_io_async Read impl
- **WHEN** `AsyncUartReader` is used as `embedded_io_async::Read`
- **THEN** it SHALL read data from the RX ring buffer

#### Scenario: embedded_io_async Write impl
- **WHEN** `AsyncUartWriter` is used as `embedded_io_async::Write`
- **THEN** it SHALL write data to the TX ring buffer

#### Scenario: embedded_io_async Write flush
- **WHEN** `AsyncUartWriter::flush()` is called
- **THEN** it SHALL poll `tx_completion()` until all four conditions are satisfied (ring_empty, copier_inactive, staged_bytes zero, transmitter_empty), using DRAIN_WAKER for notification and returning only after the UART has fully drained

### Requirement: Feature gate control
The system SHALL provide an `async` feature gate to control compilation of async modules.

#### Scenario: async feature disabled
- **WHEN** the `async` feature is not enabled
- **THEN** the async modules SHALL NOT be compiled

#### Scenario: async feature enabled
- **WHEN** the `async` feature is enabled
- **THEN** the async modules SHALL be compiled and available

### Requirement: RX ring readiness hint

`RingBufRx` MUST expose a non-blocking readiness hint that reports whether received bytes are currently available without consuming data from the ring.

#### Scenario: RX occupied length is observable

- **WHEN** bytes have been pushed into `RingBufRx`
- **THEN** `RingBufRx::occupied_len()` MUST report the current readable byte count
- **AND** `RingBufRx::has_data()` MUST return true when that count is greater than zero

#### Scenario: RX readiness query does not consume data

- **WHEN** `RingBufRx::occupied_len()` or `RingBufRx::has_data()` is called
- **THEN** a later `RingBufRx::pop()` MUST still be able to read the same bytes unless another consumer has popped them

### Requirement: TX ring readiness hint

`RingBufTx` MUST expose a non-blocking readiness hint that reports whether transmit bytes can currently be accepted without changing drain or completion semantics.

#### Scenario: TX vacant length is observable

- **WHEN** the TX ring has free space
- **THEN** `RingBufTx::vacant_len()` MUST report the current writable byte count
- **AND** `RingBufTx::has_space()` MUST return true when that count is greater than zero

#### Scenario: TX readiness is not completion

- **WHEN** `RingBufTx::has_space()` returns true
- **THEN** callers MUST NOT treat that as evidence that previously submitted bytes have drained from the UART hardware
- **AND** physical drain MUST remain represented by `AsyncUartWriter::flush()` and `tx_completion()`

### Requirement: Async reader and writer readiness facade

`AsyncUartReader` and `AsyncUartWriter` MUST expose thin readiness and waker registration methods that delegate to their RX/TX rings without introducing OS file descriptor semantics.

#### Scenario: Reader readable facade

- **WHEN** `AsyncUartReader::can_read()` is called
- **THEN** it MUST return the RX ring data readiness hint

#### Scenario: Reader readable waker registration

- **WHEN** `AsyncUartReader::register_readable_waker(waker)` is called
- **THEN** it MUST register the waker with the RX ring waker set used by RX data arrival

#### Scenario: Writer writable facade

- **WHEN** `AsyncUartWriter::can_write()` is called
- **THEN** it MUST return the TX ring space readiness hint

#### Scenario: Writer writable waker registration

- **WHEN** `AsyncUartWriter::register_writable_waker(waker)` is called
- **THEN** it MUST register the waker with the TX ring waker set used when TX ring space is released

### Requirement: Readiness hint register-recheck contract

Readiness APIs MUST be documented as hints only. OS adapters MUST use a check -> register -> recheck protocol before sleeping on readable or writable readiness.

#### Scenario: Register after not ready

- **WHEN** an OS adapter observes `can_write() == false`
- **AND** it calls `register_writable_waker(waker)`
- **THEN** it MUST recheck `can_write()` before parking the task

#### Scenario: Spurious wake is allowed

- **WHEN** a registered readable or writable waker is woken
- **THEN** the caller MUST recheck readiness before assuming a subsequent read or write can make progress

### Requirement: Crate boundary remains OS-neutral

The `uart_16550` readiness interface MUST remain OS-neutral and MUST NOT depend on StarryOS VFS, syscall, poll event, or file descriptor blocking semantics.

#### Scenario: No OS-specific dependency

- **WHEN** Q27a readiness APIs are implemented
- **THEN** `uart_16550` MUST NOT introduce dependencies on `axpoll`, VFS, syscall modules, `IoEvents`, or fd nonblocking state

#### Scenario: Existing I/O traits remain unchanged

- **WHEN** `TtyRead`, `TtyWrite`, or `embedded_io_async` methods are called after Q27a
- **THEN** their existing read/write/flush behavior MUST remain unchanged

### Requirement: Async writer writable length facade

`AsyncUartWriter` MUST expose an OS-neutral writable length hint that reports the current total TX ring space without changing write, completion, or drain semantics.

#### Scenario: Writer writable length delegates to TX ring

- **WHEN** `AsyncUartWriter::writable_len()` is called
- **THEN** it MUST return the current `RingBufTx::vacant_len()` hint
- **AND** `AsyncUartWriter::can_write()` MUST remain equivalent to writable length greater than zero

#### Scenario: Writable length remains a hint

- **WHEN** `writable_len()` reports one or more bytes
- **THEN** callers MUST NOT treat the value as a reservation under concurrent producers
- **AND** callers MUST use the actual `write()` return count as the committed byte count

#### Scenario: Crate boundary remains OS-neutral

- **WHEN** the writable length facade is implemented
- **THEN** `uart_16550` MUST NOT add dependencies on `axpoll`, VFS, syscall, `IoEvents`, or fd blocking state
- **AND** existing `TtyWrite` and `embedded_io_async::Write` behavior MUST remain unchanged

