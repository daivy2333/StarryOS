## ADDED Requirements

### Requirement: Console-only branch lifecycle

`console-lichee` MUST use polling Console as the only user-facing UART backend. The local async UART crate, kernel integration, copier tasks, UART IRQ ownership, telemetry and async feature wiring MUST be removed from this branch.

#### Scenario: Boot without async UART

- **WHEN** QEMU or D1 Console mode boots
- **THEN** `/dev/console` and process stdio MUST bind to the polling Console TTY
- **AND** no async UART driver, copier, ring benchmark or UART IRQ handler MUST start

#### Scenario: Repository cleanup

- **WHEN** the replacement is complete
- **THEN** the repository MUST contain no `crates/uart_16550` directory
- **AND** kernel and workspace manifests MUST contain no dependency or feature reference to the removed local crate

### Requirement: Platform-correct polling access

The polling Console MUST use each platform's verified UART access model. QEMU MUST use NS16550 stride 1 byte-MMIO; D1 MUST use UART0 stride 4 with 32-bit MMIO.

#### Scenario: QEMU raw output

- **WHEN** QEMU writes a raw byte
- **THEN** it MUST poll LSR THRE through byte-MMIO before writing THR

#### Scenario: D1 raw output

- **WHEN** D1 writes a raw byte
- **THEN** it MUST poll LSR THRE through 32-bit MMIO at stride 4 before writing THR

### Requirement: TTY-compatible Console behavior

The Console TTY MUST preserve current termios, controlling-terminal, FIONBIO and VFS write contracts. TTY output processing MUST be the only owner of ONLCR conversion.

#### Scenario: Complete synchronous write

- **WHEN** a blocking caller writes a non-empty buffer to `/dev/console`
- **THEN** the polling writer MUST return the complete accepted length after submitting every byte
- **AND** writable readiness MUST remain available

#### Scenario: Exactly-once ONLCR

- **WHEN** default termios maps an input LF
- **THEN** the physical output MUST contain one CR followed by one LF
- **AND** the raw polling layer MUST NOT add another CR

#### Scenario: Empty nonblocking read

- **WHEN** a supported polling reader has no byte available and FIONBIO is enabled
- **THEN** the read MUST return the existing nonblocking empty result without hanging

### Requirement: Physical Console drain

Console `tcdrain` MUST wait for LSR TEMT, not only THRE. It MUST NOT access removed async completion state.

#### Scenario: THRE precedes TEMT

- **WHEN** THRE is set while TEMT remains clear
- **THEN** `tcdrain` MUST continue waiting
- **AND** it MUST return only after TEMT is observed

#### Scenario: Drain on removed async path

- **WHEN** `TCSBRK` is invoked in the Console-only branch
- **THEN** the syscall MUST use the polling Console drain path
- **AND** no async driver symbol or waker MUST be referenced

### Requirement: Benchmark method parity

The Console baseline MUST run the existing benchmark with the same section order, payload sizes, iteration counts, timer and drain policy as the frozen async baseline. Backend-specific gaps MUST be explicit.

#### Scenario: Comparable TX sections

- **WHEN** S10-S14 or S20-S21 runs on polling Console
- **THEN** the workload and measurement boundaries MUST match the async baseline
- **AND** the manifest MUST identify `backend=polling-console`

#### Scenario: Unsupported capability

- **WHEN** S30, S31, S40 or startup ring testing requests a capability absent from a platform Console
- **THEN** the section MUST emit `UNSUPPORTED` or `SKIPPED` with a reason
- **AND** it MUST NOT report PASS or access deleted async state

### Requirement: Evidence-class separation

QEMU and D1 results MUST be recorded as separate evidence classes. A comparison MUST use the same platform on both sides and cite the frozen async commit and the Console commit.

#### Scenario: QEMU result

- **WHEN** the Console benchmark completes under QEMU
- **THEN** the result MAY support functional and same-emulator relative comparisons
- **AND** it MUST NOT support a physical line-rate claim

#### Scenario: D1 result

- **WHEN** a physical throughput or line-rate conclusion is reported
- **THEN** it MUST cite a fresh D1 Console raw log, image identity, build command and board environment

#### Scenario: Unavailable board environment

- **WHEN** D1 hardware evidence cannot be collected
- **THEN** the D1 Gate MUST be marked `ENV BLOCK`
- **AND** the four-cell async/Console comparison MUST remain incomplete

### Requirement: Test-first replacement

Every behavior change MUST have a current-state or RED test witness before its product edit, followed by GREEN and regression evidence.

#### Scenario: Console behavior implementation

- **WHEN** an implementer changes raw output, TTY integration, drain or benchmark behavior
- **THEN** the corresponding test or current-state witness MUST be recorded before the edit
- **AND** the same witness MUST pass after the edit
