## MODIFIED Requirements

### Requirement: Copier driver migration
The system SHALL migrate the copier driver from StarryOS to uart_16550, using OsRuntime trait for task spawning. The copier SHALL use `UartPort::update_ier()` for all IER manipulation instead of receiving external callback functions.

#### Scenario: RX copier loop
- **WHEN** `AsyncUartDriver::start_rx_copier()` is called
- **THEN** the system SHALL spawn a new task that continuously reads from UART and pushes to RX ring buffer. When re-enabling RX interrupts, the copier SHALL call `self.uart.update_ier(IER::DATA_READY, IER::empty())`.

#### Scenario: TX copier loop
- **WHEN** `AsyncUartDriver::start_tx_copier()` is called
- **THEN** the system SHALL spawn a new task that continuously pops from TX ring buffer and writes to UART. When re-enabling TX interrupts, the copier SHALL call `self.uart.update_ier(IER::THR_EMPTY, IER::empty())`.
