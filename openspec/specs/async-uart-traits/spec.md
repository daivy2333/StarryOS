# async-uart-traits Specification

## Purpose
TBD - created by archiving change q13-async-uart-extraction. Update Purpose after archive.
## Requirements
### Requirement: OsRuntime trait definition
The system SHALL define an `OsRuntime` trait that provides task spawning and blocking execution capabilities.

#### Scenario: Spawn async task
- **WHEN** `OsRuntime::spawn()` is called with a future and task name
- **THEN** the system SHALL spawn a new async task that executes the future concurrently

#### Scenario: Block on future
- **WHEN** `OsRuntime::block_on()` is called with a future
- **THEN** the system SHALL block the current thread until the future completes and return its result

### Requirement: OsIrq trait definition
The system SHALL define an `OsIrq` trait that provides interrupt handler registration capabilities.

#### Scenario: Register IRQ handler
- **WHEN** `OsIrq::register_handler()` is called with an IRQ number and handler function
- **THEN** the system SHALL register the handler to be called when the specified IRQ fires

#### Scenario: Handler execution context
- **WHEN** an IRQ fires after handler registration
- **THEN** the system SHALL call the registered handler in ISR context

### Requirement: OsMmio trait definition
The system SHALL define an `OsMmio` trait that provides MMIO memory mapping capabilities.

#### Scenario: Map MMIO region
- **WHEN** `OsMmio::map_mmio()` is called with physical address and size
- **THEN** the system SHALL map the physical MMIO region to virtual memory and return the virtual address

#### Scenario: Physical to virtual translation
- **WHEN** `OsMmio::phys_to_virt()` is called with a physical address
- **THEN** the system SHALL return the corresponding virtual address

### Requirement: OsSpinNoIrq trait definition
The system SHALL define an `OsSpinNoIrq` trait that provides IRQ-safe spinlock capabilities.

#### Scenario: Create spinlock
- **WHEN** `OsSpinNoIrq::new()` is called with an initial value
- **THEN** the system SHALL create a new spinlock protecting the value

#### Scenario: Lock with IRQ disabled
- **WHEN** `OsSpinNoIrq::lock()` is called
- **THEN** the system SHALL disable IRQs, acquire the lock, and return a guard that re-enables IRQs on drop

### Requirement: OsWakerSet trait definition
The system SHALL define an `OsWakerSet` trait that provides waker registration and notification capabilities.

#### Scenario: Register waker
- **WHEN** `OsWakerSet::register()` is called with a waker
- **THEN** the system SHALL register the waker to be notified on wake events

#### Scenario: Wake registered wakers
- **WHEN** `OsWakerSet::wake()` is called
- **THEN** the system SHALL wake all registered wakers and return the number of wakers notified

### Requirement: TtyWrite reports accepted byte count

`TtyWrite::write` MUST return the number of bytes accepted into the output sink. Implementations MUST NOT report success for bytes that were not accepted into a hardware FIFO, ring buffer, PTY buffer, or equivalent backend.

#### Scenario: Output sink has enough capacity

- **WHEN** a caller writes a buffer and the output sink accepts every byte
- **THEN** `TtyWrite::write` MUST return `buf.len()`

#### Scenario: Output sink has partial capacity

- **WHEN** a caller writes a buffer and the output sink accepts only part of it
- **THEN** `TtyWrite::write` MUST return the accepted byte count
- **AND** StarryOS `Tty::write_at` MUST propagate that count to VFS callers

#### Scenario: Echo output ignores short write result

- **WHEN** line discipline emits an echo character sequence
- **THEN** it MAY ignore the returned count
- **AND** the implementation MUST make that best-effort behavior explicit in code

