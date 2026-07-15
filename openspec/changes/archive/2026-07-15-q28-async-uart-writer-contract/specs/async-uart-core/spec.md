## ADDED Requirements

### Requirement: TX producer capability uniqueness

The async UART core MUST represent the TX ring producer as one unique raw `AsyncUartWriter` capability per `AsyncUartDriver`. The raw writer MUST NOT implement `Clone`, and mutating TX submission MUST require exclusive mutable access.

#### Scenario: Constructing the unique raw writer

- **WHEN** an OS adapter constructs an `AsyncUartWriter` for a driver
- **THEN** construction MUST require an explicit unsafe uniqueness contract or an equivalent one-time acquisition mechanism
- **AND** the adapter MUST establish that no second raw writer exists for the same driver

#### Scenario: Raw writer is not cloneable

- **WHEN** code owns an `AsyncUartWriter`
- **THEN** the type MUST NOT provide a `Clone` implementation
- **AND** its synchronous TX submission entry MUST require `&mut self`

#### Scenario: Ring producer cannot be bypassed

- **WHEN** crate-external safe code obtains an `AsyncUartDriver` reference
- **THEN** it MUST NOT be able to call the mutating `RingBufTx::push()` producer operation directly

### Requirement: StarryOS shared writer serialization

The StarryOS UART adapter MUST expose a cloneable `TtyWrite` writer whose clones share one task-context lock and one unique raw `AsyncUartWriter`.

#### Scenario: TTY clones direct and echo writers

- **WHEN** `Tty::new()` clones the UART writer for direct output and line-discipline echo
- **THEN** both handles MUST share the same producer lock
- **AND** at most one handle MUST enter the raw writer submission operation at a time

#### Scenario: Shared file writers submit concurrently

- **WHEN** multiple tasks call the shared `/dev/console` writer concurrently
- **THEN** all TX producer calls MUST be serialized before accessing the SPSC writer handle

#### Scenario: Producer code remains outside interrupt context

- **WHEN** the StarryOS wrapper acquires its producer lock
- **THEN** the call MUST execute in task context
- **AND** ISR paths MUST NOT acquire the producer lock or submit TX ring bytes

### Requirement: Producer lock lifetime

The StarryOS producer lock MUST protect exactly one non-blocking raw writer submission and MUST NOT span a wait or scheduling point.

#### Scenario: Blocking write encounters a full ring

- **WHEN** a blocking UART write cannot submit its complete buffer because the TX ring is full
- **THEN** it MUST release the producer lock before waiting for writable readiness
- **AND** each retry MUST reacquire the lock only for the next submission attempt

#### Scenario: Async and poll boundaries

- **WHEN** UART output uses `poll_io`, await, waker registration, or task scheduling
- **THEN** the producer lock MUST NOT remain held across the blocking or scheduling boundary

### Requirement: Concurrent accepted-prefix integrity

Every serialized raw writer submission MUST preserve its accepted prefix as a contiguous logical segment of the TX byte stream without duplication, loss, or byte-level interleaving.

#### Scenario: Two producers submit different payloads

- **WHEN** two cloned StarryOS writer handles concurrently submit distinct buffers
- **THEN** each returned byte count MUST identify the complete accepted prefix of that call
- **AND** bytes from the other producer MUST NOT appear inside that accepted prefix

#### Scenario: Blocking write retries

- **WHEN** a blocking write is split into multiple submissions by backpressure
- **THEN** another producer is allowed to submit between retries
- **AND** the system MUST NOT claim syscall-level atomicity or producer fairness

### Requirement: Q27 behavior and performance preservation

Q28 MUST preserve the Q27 blocking, nonblocking, ONLCR, readiness, poll/epoll, and drain contracts while adding producer serialization.

#### Scenario: Existing write semantics remain valid

- **WHEN** a UART write runs after producer serialization is added
- **THEN** blocking writes MUST continue until the full source request is accepted
- **AND** nonblocking writes MUST continue to return a partial count or `WouldBlock`
- **AND** empty writes MUST continue to return zero immediately

#### Scenario: ONLCR and echo behavior remain valid

- **WHEN** output processing expands newline bytes or line discipline emits echo
- **THEN** ONLCR retries MUST NOT duplicate or lose source characters
- **AND** echo MUST remain best-effort without corrupting another producer submission

#### Scenario: Performance regression gate

- **WHEN** the user manually runs one QEMU candidate measurement and one D1 candidate measurement against the same-environment Q27 baselines
- **THEN** the QEMU ring/latency and D1 throughput/p50 metrics MUST NOT regress by more than 3 percent
- **AND** a regression beyond that threshold MUST block completion
- **AND** the result MUST be recorded as a single-sample Gate without a statistical-significance claim

#### Scenario: MPSC remains out of scope

- **WHEN** Q28 serializes multiple writer handles
- **THEN** the underlying TX ring MUST remain SPSC
- **AND** Q28 MUST NOT introduce an MPSC ring or producer fairness scheduler

## MODIFIED Requirements

### Requirement: Device ops migration

The system SHALL keep async device operations in `uart_16550` and implement `embedded_io_async` traits. `AsyncUartWriter` MUST act as a unique raw producer capability; OS-specific shared `TtyWrite` behavior MUST be supplied by a serialized adapter wrapper rather than by cloning the raw writer.

#### Scenario: AsyncUartReader read

- **WHEN** `AsyncUartReader::read()` is called with a buffer
- **THEN** the reader SHALL pop data from RX ring buffer and return the number of bytes read

#### Scenario: AsyncUartWriter write

- **WHEN** `AsyncUartWriter` is called with data (legacy scenario name; see `AsyncUartWriter non-blocking submission` for the post-Q28 behavior)
- **THEN** the writer SHALL push data to TX ring buffer and return the number of bytes accepted

#### Scenario: AsyncUartWriter non-blocking submission

- **WHEN** the unique `AsyncUartWriter` is mutably called with data
- **THEN** the writer SHALL push data to TX ring buffer
- **AND** it MUST return the number of bytes in the accepted input prefix

#### Scenario: embedded_io_async Read impl

- **WHEN** `AsyncUartReader` is used as `embedded_io_async::Read`
- **THEN** it SHALL read data from the RX ring buffer

#### Scenario: embedded_io_async Write impl

- **WHEN** the unique `AsyncUartWriter` is used as `embedded_io_async::Write`
- **THEN** it SHALL write data to the TX ring buffer through exclusive mutable access

#### Scenario: embedded_io_async Write flush

- **WHEN** `AsyncUartWriter::flush()` is called
- **THEN** it SHALL poll `tx_completion()` until all four conditions are satisfied (ring_empty, copier_inactive, staged_bytes zero, transmitter_empty), using DRAIN_WAKER for notification and returning only after the UART has fully drained

#### Scenario: TtyWrite lives in the OS adapter

- **WHEN** StarryOS requires a cloneable `TtyWrite` implementation
- **THEN** the kernel adapter MUST wrap one raw `AsyncUartWriter` in a shared producer lock
- **AND** the raw `AsyncUartWriter` MUST NOT implement the shared `TtyWrite::write(&self)` contract directly

### Requirement: Crate boundary remains OS-neutral

The `uart_16550` readiness and raw writer interfaces MUST remain OS-neutral and MUST NOT depend on StarryOS VFS, syscall, poll event, file descriptor blocking, or kernel locking semantics.

#### Scenario: No OS-specific dependency

- **WHEN** Q28 raw writer APIs are implemented
- **THEN** `uart_16550` MUST NOT introduce dependencies on `axpoll`, VFS, syscall modules, `IoEvents`, fd nonblocking state, or `kspin`

#### Scenario: Existing I/O traits remain unchanged

- **WHEN** `TtyRead`, `TtyWrite`, or `embedded_io_async` methods are called after Q28
- **THEN** their existing read/write/flush behavior MUST remain unchanged

#### Scenario: Embedded async I/O remains available

- **WHEN** the raw `AsyncUartWriter` API is migrated
- **THEN** `embedded_io_async::Write` and flush behavior MUST remain available through exclusive mutable access

#### Scenario: Shared TTY behavior stays above the crate

- **WHEN** an OS requires cloneable or concurrently shared TTY writers
- **THEN** that OS adapter MUST provide producer serialization outside `uart_16550`

### Requirement: Async writer writable length facade

`AsyncUartWriter` and its serialized OS adapter MUST expose an OS-neutral writable length hint that reports current total TX ring space without changing write, completion, drain, or producer ownership semantics.

#### Scenario: Writer writable length delegates to TX ring

- **WHEN** raw or wrapped writer code queries writable length
- **THEN** it MUST return the current `RingBufTx::vacant_len()` hint
- **AND** `can_write()` MUST remain equivalent to writable length greater than zero

#### Scenario: Writable length remains a hint

- **WHEN** `writable_len()` reports one or more bytes
- **THEN** callers MUST NOT treat the value as a reservation under serialized concurrent producers
- **AND** callers MUST use the actual writer submission return count as the committed byte count

#### Scenario: Readiness does not widen the lock lifetime

- **WHEN** an OS adapter checks, registers, and rechecks writable readiness
- **THEN** it MUST preserve the existing register-recheck protocol
- **AND** it MUST NOT hold the producer lock while the task is parked
