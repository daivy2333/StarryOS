## ADDED Requirements

### Requirement: UartPort transmitter empty query
The `UartPort` trait SHALL expose a `transmitter_empty()` method that returns whether the UART's transmit shift register is empty (LSR TRANSMITTER_EMPTY bit is set).

#### Scenario: Transmitter empty
- **WHEN** the UART has finished transmitting all data including the last byte in the shift register
- **THEN** `transmitter_empty()` SHALL return `true`

#### Scenario: Transmitter not empty
- **WHEN** data remains in the UART FIFO or shift register
- **THEN** `transmitter_empty()` SHALL return `false`

#### Scenario: Thread-safe query
- **WHEN** `transmitter_empty()` is called concurrently from flush/tcdrain context while the TX copier is running
- **THEN** the implementation SHALL provide interior mutability safety (e.g., via lock acquisition)
