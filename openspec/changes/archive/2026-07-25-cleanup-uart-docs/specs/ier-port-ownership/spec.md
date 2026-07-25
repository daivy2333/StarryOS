# ier-port-ownership Specification

## Purpose
TBD - created by archiving change m4-ier-single-owner. Update Purpose after archive.
## Requirements
### Requirement: UartPort IER single ownership
The `UartPort` trait SHALL provide an `update_ier(set: IER, clear: IER)` method that atomically modifies the UART's Interrupt Enable Register. All IER state SHALL be managed internally by the port implementation; no external `CACHED_IER` or callback functions are required.

#### Scenario: Enable TX interrupt
- **WHEN** `update_ier(IER::THR_EMPTY, IER::empty())` is called
- **THEN** the THR_EMPTY bit in IER SHALL be set, and all other bits SHALL remain unchanged

#### Scenario: Disable TX interrupt  
- **WHEN** `update_ier(IER::empty(), IER::THR_EMPTY)` is called
- **THEN** the THR_EMPTY bit in IER SHALL be cleared, and all other bits SHALL remain unchanged

#### Scenario: Atomic set and clear
- **WHEN** `update_ier(IER::DATA_READY, IER::THR_EMPTY)` is called
- **THEN** DATA_READY SHALL be set and THR_EMPTY SHALL be cleared atomically in a single IER write

#### Scenario: Thread safety
- **WHEN** `update_ier()` is called concurrently from copier task and ISR context
- **THEN** the implementation SHALL provide interior mutability safety preventing data races

