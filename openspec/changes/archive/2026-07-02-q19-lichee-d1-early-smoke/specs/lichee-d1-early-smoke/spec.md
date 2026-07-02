# lichee-d1-early-smoke Specification

## ADDED Requirements

### Requirement: D1 AxPlat Boot Selection

StarryOS SHALL provide a Lichee RV Dock / Allwinner D1 build path that selects a D1-specific axplat boot layer instead of the QEMU virt axplat boot layer.

#### Scenario: D1 target selects D1 axplat

- **Given** the Lichee D1 build path is selected
- **When** the kernel is built
- **Then** the resulting artifact SHALL reference `axplat_riscv64_lichee_d1::boot`
- **And** it SHALL NOT reference `axplat_riscv64_qemu_virt::boot`.

#### Scenario: QEMU remains default

- **Given** no D1 platform selection is requested
- **When** the default RISC-V build is used
- **Then** StarryOS SHALL continue to use the existing QEMU axplat and QEMU descriptor path.

### Requirement: D1 Link and Load Contract

The D1 build artifact SHALL align its linker base, physical load address, and Android boot image header with the collected Lichee RV Dock board facts.

#### Scenario: D1 ELF and linker script match expected high-half base

- **Given** a D1 build completes
- **When** the ELF entry and generated linker script are inspected
- **Then** both SHALL use the D1 high-half kernel base `0xffffffc040200000`.

#### Scenario: Android boot image loads at D1 kernel address

- **Given** a D1 raw kernel payload has been packed as an Android boot image
- **When** the image is inspected
- **Then** it SHALL report `page_size=2048`
- **And** it SHALL report `kernel_addr=0x40200000`
- **And** the image size SHALL be below the current boot partition capacity.

### Requirement: D1 AxPlat Polling Console

The D1 axplat SHALL provide a polling UART0 console suitable for first-byte bring-up before StarryOS `entry.rs` and before async subsystems.

#### Scenario: D1 UART0 uses 32-bit MMIO polling

- **Given** the D1 axplat console writes a byte
- **When** it checks transmitter readiness
- **Then** it SHALL read LSR register index `5` using register stride `4`
- **And** it SHALL test THRE bit `1 << 5`
- **And** it SHALL write the byte through a 32-bit volatile write to THR register index `0`.

#### Scenario: First-byte output has no higher-service dependency

- **Given** the D1 payload has entered axplat boot code
- **When** early serial output is emitted
- **Then** it SHALL NOT depend on rootfs, USB, SDMMC, async tasks, async UART, PLIC IRQ delivery, or benchmark infrastructure.

### Requirement: Manual and Recoverable Flash Workflow

StarryOS SHALL keep D1 flashing manual and gated by host-side artifact inspection.

#### Scenario: Host inspection gates flashing

- **Given** a D1 boot image has been generated
- **When** host inspection shows a wrong linker base, wrong boot symbols, wrong `kernel_addr`, wrong `page_size`, or oversized image
- **Then** the workflow SHALL block board flashing instructions until the artifact is fixed.

#### Scenario: Build scripts do not write board storage

- **Given** a D1 boot image has been generated
- **When** the build or packaging command completes
- **Then** it SHALL NOT automatically write to `/dev/mmcblk*`, `/dev/sd*`, or `/dev/by-name/boot`.

### Requirement: Q19a Scope Boundary

Q19a SHALL remain a D1 axplat bring-up milestone and SHALL NOT require D1 rootfs, USB, SDMMC, shell, async TTY, PLIC IRQ, benchmark execution, SMP, or VisionFive2 support.

#### Scenario: Q19a can pass with only early serial evidence

- **Given** the board prints D1 axplat early output or `[starry-d1] early boot`
- **When** rootfs, USB, SDMMC, shell, async TTY, benchmark, and PLIC IRQ support are absent
- **Then** Q19a MAY pass
- **And** those missing services SHALL remain later milestones.
