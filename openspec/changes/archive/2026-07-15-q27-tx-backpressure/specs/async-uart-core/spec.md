## ADDED Requirements

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

