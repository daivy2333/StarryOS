## MODIFIED Requirements

### Requirement: Copier driver migration
The system SHALL migrate the copier driver from StarryOS to uart_16550, using OsRuntime trait for task spawning. TX backpressure MUST use bounded fast-path retry followed by IRQ-driven Pending, and MUST expose explicit copier active/staged completion state.

#### Scenario: RX copier loop
- **WHEN** `AsyncUartDriver::start_rx_copier()` is called
- **THEN** the system SHALL spawn a new task that continuously reads from UART and pushes to RX ring buffer

#### Scenario: TX copier loop
- **WHEN** `AsyncUartDriver::start_tx_copier()` is called
- **THEN** the system SHALL spawn one task that pops from TX ring buffer and writes to UART
- **AND** FIFO full handling MUST satisfy the bounded fast-path requirement

#### Scenario: TX copier completion state
- **WHEN** TX copier has popped bytes that are not fully submitted to UART
- **THEN** driver MUST report copier active and the remaining staged byte count

#### Scenario: NAPI interrupt coalescing
- **WHEN** consecutive successful reads exceed NAPI_THRESHOLD (16)
- **THEN** the RX copier SHALL enter polling mode with NAPI_BATCH_SIZE (64)

### Requirement: Device ops migration
The system SHALL migrate the device ops from StarryOS to uart_16550, implementing embedded_io_async traits. Writer and flush operations MUST preserve short-write and three-stage completion semantics.

#### Scenario: AsyncUartReader read
- **WHEN** `AsyncUartReader::read()` is called with a buffer
- **THEN** the reader SHALL pop data from RX ring buffer and return the number of bytes read

#### Scenario: AsyncUartWriter write
- **WHEN** `AsyncUartWriter::write()` is called with data
- **THEN** the writer SHALL push data to TX ring buffer and return the actual number of bytes accepted

#### Scenario: embedded_io_async Read impl
- **WHEN** `AsyncUartReader` is used as `embedded_io_async::Read`
- **THEN** it SHALL wait for and read data from the RX ring buffer

#### Scenario: embedded_io_async Write impl
- **WHEN** `AsyncUartWriter` is used as `embedded_io_async::Write`
- **THEN** it SHALL wait on full-ring backpressure and MUST NOT return `Ok(0)` for nonempty input

#### Scenario: embedded_io_async flush impl
- **WHEN** `AsyncUartWriter::flush()` is awaited
- **THEN** it MUST wait for ring, copier staging and UART TEMT completion
