## MODIFIED Requirements

### Requirement: Copier driver migration
The system SHALL migrate the copier driver from StarryOS to uart_16550, using OsRuntime trait for task spawning.

#### Scenario: RX copier loop
- **WHEN** `AsyncUartDriver::start_rx_copier()` is called
- **THEN** the system SHALL spawn a new task that continuously reads from UART and pushes to RX ring buffer

#### Scenario: TX copier loop
- **WHEN** `AsyncUartDriver::start_tx_copier()` is called
- **THEN** the system SHALL spawn a new task that continuously pops from TX ring buffer and writes to UART. When the UART THR is full, the copier SHALL perform up to `TX_FAST_RETRY_LIMIT` (32) bounded fast retries within the same poll before falling back to interrupt-driven wakeup. The copier SHALL set `tx_copier_active` to `true` on poll entry and clear it to `false` before yielding. The copier SHALL track `tx_staged_bytes` to reflect bytes popped from ring but not yet confirmed sent.

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

#### Scenario: embedded_io_async Write flush
- **WHEN** `AsyncUartWriter::flush()` is called
- **THEN** it SHALL poll `tx_completion()` until all four conditions are satisfied (ring_empty, copier_inactive, staged_bytes zero, transmitter_empty), using DRAIN_WAKER for notification and returning only after the UART has fully drained
