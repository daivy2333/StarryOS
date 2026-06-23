## MODIFIED Requirements

### Requirement: ArceOS adapter implementation
The system SHALL implement the OS abstraction traits for ArceOS (StarryOS's underlying framework), including the `UartPort` trait with `transmitter_empty()` support.

#### Scenario: ArceOsRuntime implementation
- **WHEN** `ArceOsRuntime` is used as `OsRuntime`
- **THEN** it SHALL use `axtask::spawn_with_name` for spawning and `axtask::future::block_on` for blocking

#### Scenario: ArceOsWakerSet implementation
- **WHEN** `ArceOsWakerSet` is used as `OsWakerSet`
- **THEN** it SHALL use `axpoll::PollSet` for waker registration and notification

#### Scenario: ArceOsUartPort transmitter_empty implementation
- **WHEN** `ArceOsUartPort::transmitter_empty()` is called
- **THEN** it SHALL acquire the UART lock, read the LSR register, and return whether `TRANSMITTER_EMPTY` is set

### Requirement: StarryOS integration
The system SHALL use uart_16550's async implementation including the new completion API for tcdrain.

#### Scenario: Cargo.toml update
- **WHEN** StarryOS kernel/Cargo.toml is updated
- **THEN** it SHALL enable uart_16550's `async` feature

#### Scenario: tcdrain uses driver completion
- **WHEN** `TCSBRK` ioctl is invoked
- **THEN** the implementation SHALL poll `driver().tx_completion()` instead of directly accessing UART MMIO registers
