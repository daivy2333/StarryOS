## ADDED Requirements

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
