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
The system SHALL migrate the copier driver from StarryOS to uart_16550, using OsRuntime trait for task spawning.

#### Scenario: RX copier loop
- **WHEN** `AsyncUartDriver::start_rx_copier()` is called
- **THEN** the system SHALL spawn a new task that continuously reads from UART and pushes to RX ring buffer

#### Scenario: TX copier loop
- **WHEN** `AsyncUartDriver::start_tx_copier()` is called
- **THEN** the system SHALL spawn a new task that continuously pops from TX ring buffer and writes to UART

#### Scenario: NAPI interrupt coalescing
- **WHEN** consecutive successful reads exceed NAPI_THRESHOLD (16)
- **THEN** the RX copier SHALL enter polling mode with NAPI_BATCH_SIZE (64)

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

### Requirement: Feature gate control
The system SHALL provide an `async` feature gate to control compilation of async modules.

#### Scenario: async feature disabled
- **WHEN** the `async` feature is not enabled
- **THEN** the async modules SHALL NOT be compiled

#### Scenario: async feature enabled
- **WHEN** the `async` feature is enabled
- **THEN** the async modules SHALL be compiled and available

