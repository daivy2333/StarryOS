## ADDED Requirements

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
