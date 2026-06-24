## MODIFIED Requirements

### Requirement: Performance regression prevention
The system SHALL verify that StarryOS integration does not cause performance regression and MUST include FIFO-boundary, CPU-idle and blocking-vs-async evidence.

#### Scenario: Benchmark comparison
- **WHEN** benchmark is run after migration or TX backpressure changes
- **THEN** 64B–4096B Async `write+tcdrain` SHALL be within 10% of the same-environment blocking baseline
- **AND** results MUST identify commit, QEMU/board, tick frequency and FIFO configuration

#### Scenario: QEMU verification
- **WHEN** QEMU is run with the updated driver
- **THEN** kernel SHALL boot normally and Shell interaction SHALL work
- **AND** FIFO boundary tests SHALL show no 10ms refill steps

#### Scenario: Idle CPU verification
- **WHEN** UART TX remains idle for 10 seconds
- **THEN** TX copier counters MUST remain stable and no busy-poll SHALL occur

## ADDED Requirements

### Requirement: 单一 IER 状态所有权

StarryOS adapter MUST route RX/TX interrupt enable/disable through the driver-owned UartPort operation; OS-level cached IER callbacks MUST NOT coexist with driver updates.

#### Scenario: TX copier 使能 THRE
- **WHEN** bounded fast-path budget is exhausted
- **THEN** TX copier MUST enable THRE through `UartPort::update_ier`
- **AND** StarryOS MUST NOT rewrite IER from a stale external cache

#### Scenario: IRQ 清除 THRE
- **WHEN** THRE interrupt is handled
- **THEN** ISR MUST clear the corresponding IER bit through the same owner
- **AND** ISR MUST only confirm source, update interrupt state, wake, and return
