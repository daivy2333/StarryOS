## ADDED Requirements

### Requirement: ArceOS adapter implementation
The system SHALL implement the 5 OS abstraction traits for ArceOS (StarryOS's underlying framework).

#### Scenario: ArceOsRuntime implementation
- **WHEN** `ArceOsRuntime` is used as `OsRuntime`
- **THEN** it SHALL use `axtask::spawn_with_name` for spawning and `axtask::future::block_on` for blocking

#### Scenario: ArceOsIrq implementation
- **WHEN** `ArceOsIrq` is used as `OsIrq`
- **THEN** it SHALL use `axhal::irq::register_irq_hook` for IRQ registration

#### Scenario: ArceOsMmio implementation
- **WHEN** `ArceOsMmio` is used as `OsMmio`
- **THEN** it SHALL use `axhal::mem::phys_to_virt` and `axmm::iomap` for MMIO mapping

#### Scenario: ArceOsSpinNoIrq implementation
- **WHEN** `ArceOsSpinNoIrq` is used as `OsSpinNoIrq`
- **THEN** it SHALL use `kspin::SpinNoIrq` for IRQ-safe spinlock

#### Scenario: ArceOsWakerSet implementation
- **WHEN** `ArceOsWakerSet` is used as `OsWakerSet`
- **THEN** it SHALL use `axpoll::PollSet` for waker registration and notification

### Requirement: StarryOS integration
The system SHALL modify StarryOS to use uart_16550's async implementation instead of local code.

#### Scenario: Cargo.toml update
- **WHEN** StarryOS kernel/Cargo.toml is updated
- **THEN** it SHALL enable uart_16550's `async` feature

#### Scenario: Adapter layer creation
- **WHEN** StarryOS creates os_arceos.rs
- **THEN** it SHALL implement all 5 OS traits using ArceOS APIs

#### Scenario: Local code deletion
- **WHEN** StarryOS removes local async implementation
- **THEN** it SHALL delete isr.rs, ring_buffer.rs, async_driver.rs, device_ops.rs

### Requirement: Performance regression prevention
The system SHALL verify that the migration does not cause performance regression.

#### Scenario: Benchmark comparison
- **WHEN** benchmark is run after migration
- **THEN** performance SHALL be at least as good as Q12 baseline (1B avg latency ≤ 124µs)

#### Scenario: QEMU verification
- **WHEN** QEMU is run with migrated code
- **THEN** kernel SHALL boot normally and Shell interaction SHALL work

### Requirement: Initialization sequence
The system SHALL maintain the existing initialization sequence after migration.

#### Scenario: Hardware initialization
- **WHEN** `init_uart_hardware()` is called
- **THEN** it SHALL initialize UART hardware using uart_16550's sync API

#### Scenario: Driver initialization
- **WHEN** `AsyncUartDriver::init()` is called
- **THEN** it SHALL create ring buffers and spawn copier tasks using OsRuntime trait

#### Scenario: TTY binding
- **WHEN** `AsyncTty` is created
- **THEN** it SHALL use `AsyncUartReader` and `AsyncUartWriter` from uart_16550
