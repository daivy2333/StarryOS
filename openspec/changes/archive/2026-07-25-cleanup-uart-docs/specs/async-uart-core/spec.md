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

#### Scenario: Crate boundary remains OS-neutral

- **WHEN** the writable length facade is implemented
- **THEN** `uart_16550` MUST NOT add dependencies on `axpoll`, VFS, syscall, `IoEvents`, or fd blocking state
- **AND** embedded async write and OS-adapter `TtyWrite` behavior MUST remain available

### Requirement: RX raw consumer capability uniqueness

The async UART core MUST represent the RX ring consumer as one unique raw `AsyncUartReader` capability per `AsyncUartDriver`. Raw reader construction MUST require an explicit unsafe uniqueness contract or an equivalent one-time acquisition mechanism, the raw reader MUST NOT implement `Clone`, and consuming reads MUST require exclusive mutable access.

#### Scenario: Constructing the unique raw reader

- **WHEN** an OS adapter constructs an `AsyncUartReader` for a driver
- **THEN** construction MUST require an explicit unsafe uniqueness contract or an equivalent one-time acquisition mechanism
- **AND** the adapter MUST establish that no second raw reader exists for the same driver

#### Scenario: Raw reader is not cloneable

- **WHEN** code owns an `AsyncUartReader`
- **THEN** the type MUST NOT provide a `Clone` implementation
- **AND** synchronous and embedded async consuming reads MUST require `&mut self`

#### Scenario: Ring consumer cannot be bypassed

- **WHEN** crate-external safe code obtains an `AsyncUartDriver` reference
- **THEN** it MUST NOT be able to call the mutating `RingBufRx::pop()` consumer operation directly

### Requirement: RX producer role encapsulation

The async UART core MUST reserve RX ring producer operations for the unique RX copier path. Crate-external safe code MUST NOT be able to push received bytes directly into the driver's RX ring.

#### Scenario: Crate-external code attempts RX production

- **WHEN** crate-external safe code obtains an `AsyncUartDriver` reference
- **THEN** it MUST NOT be able to call RX ring `push` or `push_batch` producer operations

#### Scenario: RX copier receives hardware bytes

- **WHEN** the unique RX copier reads bytes from the UART FIFO
- **THEN** it MUST remain able to push those bytes into the RX ring
- **AND** the push MUST wake registered readable wakers when data is accepted

#### Scenario: Copier startup preserves unique ring roles

- **WHEN** an OS adapter starts the RX or TX copier for a driver
- **THEN** startup MUST require an explicit unsafe uniqueness contract or an equivalent one-time mechanism
- **AND** the adapter MUST establish that each copier direction is started exactly once for that driver

### Requirement: StarryOS single RX consumer witness

StarryOS MUST construct exactly one raw `AsyncUartReader` for the global async UART driver and MUST transfer that capability to exactly one `tty-reader` task. Shared file descriptors MUST consume the line-discipline ring rather than constructing or sharing additional raw UART readers.

#### Scenario: Async TTY initialization

- **WHEN** StarryOS initializes `ASYNC_TTY`
- **THEN** the unique raw reader construction point MUST document why no second reader exists for that driver
- **AND** the reader MUST move into the single external line-discipline reader task

#### Scenario: Multiple file descriptors read the console

- **WHEN** multiple tasks read through shared or duplicated console file descriptors
- **THEN** they MUST consume bytes through the serialized line-discipline path
- **AND** they MUST NOT call the UART RX ring consumer directly

### Requirement: RX behavior and readiness preservation

Q29 MUST preserve RX byte integrity, existing non-consuming readiness hints, readable waker registration, and register-recheck behavior while converging the producer/consumer API contract.

#### Scenario: RX data crosses ring wrap-around

- **WHEN** the RX copier produces bytes spanning the ring storage boundary and the unique reader consumes them
- **THEN** bytes MUST be returned in order without duplication or loss

#### Scenario: Empty read behavior

- **WHEN** the unique reader receives an empty destination buffer or reads an empty RX ring
- **THEN** the operation MUST return zero without consuming or inventing bytes

#### Scenario: Readable waiter registers during arrival

- **WHEN** RX data may arrive between an initial not-readable check and waker registration
- **THEN** the waiter MUST register and then recheck readiness before parking
- **AND** a spurious wake MUST require another readiness check before consuming

#### Scenario: Readiness observation is shareable

- **WHEN** multiple task-context observers query RX occupied length or register readable wakers
- **THEN** those operations MUST NOT consume bytes or acquire an additional raw consumer capability

### Requirement: Q29 concurrency scope boundary

Q29 MUST keep the RX queue SPSC and MUST NOT claim multi-hart runtime validation from API, QEMU, or single-hart evidence.

#### Scenario: Multi-consumer implementation is considered

- **WHEN** the unique raw reader contract is implemented
- **THEN** the system MUST NOT introduce an MPMC ring, cloneable raw reader, or reader serialization lock without a demonstrated multi-consumer requirement

#### Scenario: Multi-hart evidence is requested

- **WHEN** a report evaluates concurrent UART read, write, drain, or IER behavior across harts
- **THEN** that conclusion MUST remain assigned to Q24 on VisionFive2 or an equivalent SMP environment
- **AND** Q29 evidence MUST be limited to API uniqueness, current ownership topology, byte integrity, readiness, and functional regression
