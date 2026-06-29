# lichee-d1-benchmark Specification

## ADDED Requirements

### Requirement: Lichee Benchmark Modes

StarryOS SHALL provide explicit Lichee RV Dock runtime modes for smoke regression, kernel benchmark, and user benchmark so that Q19B can advance without losing the Q19A smoke gate.

#### Scenario: Smoke mode remains available

- **Given** the Lichee smoke mode is selected
- **When** the board boots through the Android boot image path
- **Then** it SHALL print the Q19A smoke marker
- **And** it SHALL NOT require async UART, PLIC IRQ, `/dev/console`, user processes, SDMMC, or rootfs.

#### Scenario: Benchmark mode is visible in logs

- **Given** a Lichee benchmark mode is selected
- **When** the board starts StarryOS
- **Then** the serial log SHALL identify the active benchmark mode before executing benchmark-specific work.

### Requirement: D1 Async UART Access Width

The D1 async UART path SHALL access UART0 as a DesignWare APB UART with register stride 4 and 32-bit MMIO access.

#### Scenario: D1 path does not use QEMU byte probe

- **Given** a D1 benchmark mode is selected
- **When** async UART initialization reads UART registers
- **Then** it SHALL NOT use QEMU-style byte access at `base + 5`
- **And** it SHALL read LSR through the D1 stride 4 / 32-bit access model.

#### Scenario: QEMU path remains byte-addressed

- **Given** the default QEMU path is selected
- **When** async UART initialization runs
- **Then** it SHALL preserve the existing NS16550 byte-addressed behavior.

### Requirement: D1 UART IRQ Gate

The D1 benchmark path SHALL verify real PLIC delivery for UART0 IRQ 18 before treating `/dev/console` or user benchmark results as valid.

#### Scenario: UART IRQ 18 is claimed and completed

- **Given** D1 PLIC IRQ mode is enabled
- **When** UART0 raises an interrupt
- **Then** the PLIC path SHALL claim source 18
- **And** it SHALL call the UART interrupt handler
- **And** it SHALL complete source 18.

#### Scenario: IRQ failure blocks later gates

- **Given** UART IRQ 18 cannot be observed
- **When** Q19B verification reaches the IRQ gate
- **Then** Q19B SHALL stop before `/dev/console` and user benchmark validation.

### Requirement: Kernel Benchmark Gate

The D1 benchmark path SHALL run the kernel async UART benchmark as an intermediate gate after async UART initialization and before user benchmark validation.

#### Scenario: Kernel benchmark output is serial-visible

- **Given** D1 async UART initialization succeeds
- **When** `drivers::bench::run_startup_benchmark()` runs
- **Then** the serial log SHALL include `[BENCH] Running startup benchmark`
- **And** it SHALL include `[BENCH] Startup benchmark complete`.

### Requirement: User Benchmark Through `/dev/console`

Q19B SHALL run a user-space UART benchmark through `/dev/console`, preserving the QEMU benchmark behavioral surface.

#### Scenario: User benchmark prints expected sections

- **Given** the D1 user benchmark mode is selected
- **When** the embedded benchmark payload runs
- **Then** the serial log SHALL include `UART Async Benchmark`
- **And** it SHALL include TX throughput output
- **And** it SHALL include TX latency output
- **And** it SHALL include FIFO boundary matrix output
- **And** it SHALL include nonblocking read output.

#### Scenario: Embedded payload can satisfy first dataset

- **Given** SDMMC/rootfs support is not yet available
- **When** an embedded static RISC-V benchmark ELF is used
- **Then** Q19B MAY still pass
- **And** SDMMC/rootfs parity SHALL remain a later milestone.

### Requirement: Benchmark Data Separation

Lichee D1 benchmark data SHALL be recorded separately from QEMU data because QEMU does not model physical UART line delay.

#### Scenario: D1 result is recorded separately

- **Given** a D1 benchmark run completes
- **When** results are documented
- **Then** the raw serial output SHALL be preserved
- **And** benchmark summary tables SHALL label the board as Lichee RV Dock / Allwinner D1
- **And** QEMU benchmark rows SHALL NOT be overwritten by D1 data.
