## ADDED Requirements

### Requirement: Feature-gated telemetry counters

The uart_16550 crate SHALL provide diagnostic counters (`tx_poll`, `tx_no_progress`, `tx_hw_bytes`) gated behind `#[cfg(feature = "telemetry")]`. When the feature is not enabled, the counters SHALL be compiled out with zero runtime overhead.

#### Scenario: Counters absent without feature

- **WHEN** `uart_16550` is compiled without `--features telemetry`
- **THEN** the resulting binary SHALL contain no atomic counter instructions in tx_copier_loop

#### Scenario: Counters increment correctly with feature

- **WHEN** `uart_16550` is compiled with `--features telemetry` and the TX copier runs
- **THEN** `tx_poll` SHALL increment on each poll_fn invocation and `tx_hw_bytes` SHALL reflect bytes written to UART FIFO

### Requirement: Idle counter stability

When the UART is idle (no data to transmit), telemetry counters SHALL NOT grow continuously over a 10-second observation period.

#### Scenario: Idle counters stable

- **WHEN** the system is idle for 10 seconds with no TX activity
- **THEN** the delta of all telemetry counters over that period SHALL be zero

### Requirement: Counter reset API

The telemetry counters SHALL provide a public reset method to allow benchmark harnesses to snapshot per-test statistics.

#### Scenario: Counters reset between tests

- **WHEN** `reset_counters()` is called
- **THEN** all telemetry counters SHALL read as zero on the next access
