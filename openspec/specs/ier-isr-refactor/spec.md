# ier-isr-refactor Specification

## Purpose
TBD - created by archiving change m4-ier-single-owner. Update Purpose after archive.
## Requirements
### Requirement: ISR handler uses port IER interface
The ISR handler SHALL use function pointers (`fn_disable_rx`, `fn_disable_tx`) instead of direct MMIO register writes and external `cached_ier` state to disable interrupts after handling.

#### Scenario: RX interrupt handling via port
- **WHEN** a RX interrupt fires
- **THEN** the ISR handler SHALL call `fn_disable_rx()` and wake `RX_WAKER`

#### Scenario: TX interrupt handling via port
- **WHEN** a TX interrupt fires
- **THEN** the ISR handler SHALL call `fn_disable_tx()` and wake `TX_WAKER`

#### Scenario: ISR no longer depends on CACHED_IER
- **WHEN** the ISR handler is invoked
- **THEN** it SHALL NOT read or write any external `AtomicU8` IER cache; all IER manipulation is delegated to the function pointers

