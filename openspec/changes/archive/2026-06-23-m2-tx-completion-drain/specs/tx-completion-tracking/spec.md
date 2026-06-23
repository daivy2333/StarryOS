## ADDED Requirements

### Requirement: TX copier active state tracking
The system SHALL track whether the TX copier is currently processing data via an atomic `tx_copier_active` flag, settable and clearable from within the TX copier's poll cycle.

#### Scenario: Copier active flag set on poll entry
- **WHEN** the TX copier enters its `poll_fn` closure
- **THEN** `tx_copier_active` SHALL be set to `true`

#### Scenario: Copier active flag cleared on yield
- **WHEN** the TX copier returns `Poll::Pending` (either due to empty ring buffer or exhausted retry budget)
- **THEN** `tx_copier_active` SHALL be cleared to `false`

#### Scenario: Copier active flag queried by flush
- **WHEN** `tx_completion()` is called from a flush context
- **THEN** the returned `TxCompletion.copier_active` SHALL reflect the current value of the atomic flag

### Requirement: TX staged bytes tracking
The system SHALL track bytes that have been popped from the TX ring buffer but not yet confirmed sent to the UART FIFO, via an atomic `tx_staged_bytes` counter.

#### Scenario: Staged bytes incremented on ring pop
- **WHEN** `pop_batch()` returns N bytes (N > 0) from the TX ring buffer
- **THEN** `tx_staged_bytes` SHALL be atomically incremented by N

#### Scenario: Staged bytes decremented on successful send
- **WHEN** `send_bytes()` returns S bytes (S > 0)
- **THEN** `tx_staged_bytes` SHALL be atomically decremented by S

#### Scenario: Staged bytes not modified on zero send
- **WHEN** `send_bytes()` returns 0 (FIFO full)
- **THEN** `tx_staged_bytes` SHALL remain unchanged

### Requirement: TxCompletion snapshot API
The system SHALL provide a `tx_completion()` method on `AsyncUartDriver` that returns a `TxCompletion` struct with four fields: `ring_empty`, `copier_active`, `staged_bytes`, and `transmitter_empty`.

#### Scenario: All conditions satisfied
- **WHEN** ring buffer is empty, copier is inactive, staged bytes is 0, and UART TEMT is true
- **THEN** `TxCompletion` SHALL have all four fields reflecting the satisfied conditions

#### Scenario: Copier still active
- **WHEN** the TX copier is in the middle of a poll cycle
- **THEN** `TxCompletion.copier_active` SHALL be `true`

#### Scenario: Staged bytes still pending
- **WHEN** data has been popped from the ring but not yet fully sent to UART
- **THEN** `TxCompletion.staged_bytes` SHALL be greater than 0
