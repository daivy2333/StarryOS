# async-uart-traits Specification

## Purpose
Archived capability spec from Q13 async-uart extraction. ADR-036 supersedes the original 5-trait plan: the current minimum OS abstraction is `OsRuntime` + `OsWakerSet`; `OsIrq`, `OsMmio`, and `OsSpinNoIrq` remain here only as historical requirements from the archived change.
## Requirements
### Requirement: OsRuntime trait definition
The system SHALL define an `OsRuntime` trait that provides task spawning and blocking execution capabilities.

#### Scenario: Spawn async task
- **WHEN** `OsRuntime::spawn()` is called with a future and task name
- **THEN** the system SHALL spawn a new async task that executes the future concurrently

#### Scenario: Block on future
- **WHEN** `OsRuntime::block_on()` is called with a future
- **THEN** the system SHALL block the current thread until the future completes and return its result

### Requirement: OsIrq trait definition — superseded by ADR-036

The system MUST treat the archived `OsIrq` trait requirement as superseded by ADR-036. Current async UART integration SHALL NOT require `OsIrq`; IRQ registration is handled outside the reusable driver by the OS adapter layer.

#### Scenario: Register IRQ handler
- **WHEN** `OsIrq::register_handler()` is called with an IRQ number and handler function
- **THEN** this archived behavior SHALL be interpreted as historical Q13 context, not as a current implementation requirement

#### Scenario: Handler execution context
- **WHEN** an IRQ fires after handler registration
- **THEN** current StarryOS SHALL dispatch through its OS-level IRQ hook and call the uart_16550 ISR wrapper without requiring an `OsIrq` trait

### Requirement: OsMmio trait definition — superseded by ADR-036

The system MUST treat the archived `OsMmio` trait requirement as superseded by ADR-036. Current async UART integration SHALL NOT require `OsMmio`; MMIO mapping is performed before constructing the reusable driver.

#### Scenario: Map MMIO region
- **WHEN** `OsMmio::map_mmio()` is called with physical address and size
- **THEN** this archived behavior SHALL be interpreted as historical Q13 context, not as a current implementation requirement

#### Scenario: Physical to virtual translation
- **WHEN** `OsMmio::phys_to_virt()` is called with a physical address
- **THEN** current StarryOS SHALL use OS-level MMIO setup outside the async UART trait boundary

### Requirement: OsSpinNoIrq trait definition — superseded by ADR-036

The system MUST treat the archived `OsSpinNoIrq` trait requirement as superseded by ADR-036. Current async UART integration SHALL NOT require `OsSpinNoIrq`; locking is owned by the StarryOS `UartPort` implementation.

#### Scenario: Create spinlock
- **WHEN** `OsSpinNoIrq::new()` is called with an initial value
- **THEN** this archived behavior SHALL be interpreted as historical Q13 context, not as a current implementation requirement

#### Scenario: Lock with IRQ disabled
- **WHEN** `OsSpinNoIrq::lock()` is called
- **THEN** current StarryOS SHALL use its concrete lock inside `ArceOsUartPort` rather than exposing a reusable `OsSpinNoIrq` trait

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
