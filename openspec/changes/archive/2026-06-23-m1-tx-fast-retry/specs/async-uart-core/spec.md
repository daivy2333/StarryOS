## MODIFIED Requirements

### Requirement: Copier driver migration
The system SHALL migrate the copier driver from StarryOS to uart_16550, using OsRuntime trait for task spawning.

#### Scenario: RX copier loop
- **WHEN** `AsyncUartDriver::start_rx_copier()` is called
- **THEN** the system SHALL spawn a new task that continuously reads from UART and pushes to RX ring buffer

#### Scenario: TX copier loop
- **WHEN** `AsyncUartDriver::start_tx_copier()` is called
- **THEN** the system SHALL spawn a new task that continuously pops from TX ring buffer and writes to UART. When the UART THR is full, the copier SHALL perform up to `TX_FAST_RETRY_LIMIT` (32) bounded fast retries within the same poll before falling back to interrupt-driven wakeup.

#### Scenario: NAPI interrupt coalescing
- **WHEN** consecutive successful reads exceed NAPI_THRESHOLD (16)
- **THEN** the RX copier SHALL enter polling mode with NAPI_BATCH_SIZE (64)
