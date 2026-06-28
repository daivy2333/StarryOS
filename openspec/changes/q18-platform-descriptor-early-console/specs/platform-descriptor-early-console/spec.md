## ADDED Requirements

### Requirement: Platform descriptor centralizes board facts

StarryOS MUST centralize board-specific facts behind a build-time platform descriptor or equivalent platform module before adding additional board support.

The descriptor MUST distinguish at least:

- platform name
- memory layout
- kernel image layout
- console UART kind
- console UART base physical address
- console UART IRQ
- console UART register stride
- console UART MMIO access width
- timer strategy
- boot image strategy

#### Scenario: QEMU UART facts are descriptor-owned

- **WHEN** StarryOS builds for QEMU virt
- **THEN** UART base `0x10000000`, IRQ `10`, stride `1`, and access width `U8` MUST come from the QEMU platform descriptor
- **AND** `kernel/src/drivers/uart_init.rs` MUST NOT define those QEMU board facts as local driver constants

#### Scenario: Stride and access width remain distinct

- **WHEN** a platform uses stride `4`
- **THEN** the platform descriptor MUST still separately specify MMIO access width
- **AND** code MUST NOT infer 32-bit access solely from stride

### Requirement: Early console is independent from async UART

StarryOS MUST provide an early console abstraction that can emit characters without depending on async UART runtime state.

The early console MUST NOT depend on:

- ring buffers
- async tasks
- IRQ delivery
- PLIC initialization
- rootfs
- `/dev/console`

#### Scenario: QEMU early console baseline

- **WHEN** Q18 runs on QEMU virt
- **THEN** `Ns16550U8EarlyConsole` MUST be able to write bytes using the QEMU descriptor's console configuration
- **AND** newline output MUST be terminal-compatible (`\n` emitted as `\r\n`)

#### Scenario: True board bring-up remains deferred

- **WHEN** Q18 defines `DwApbUart32EarlyConsole` or D1/VisionFive2 descriptor placeholders
- **THEN** it MUST NOT claim Lichee RV Dock or VisionFive2 hardware success
- **AND** hardware smoke tests MUST remain in Q19/Q20

### Requirement: Async UART initialization consumes platform descriptor

Async UART initialization MUST consume platform descriptor values rather than owning platform-specific constants.

#### Scenario: async UART init remains QEMU-compatible

- **WHEN** `init_uart_hardware()` initializes the QEMU async UART path
- **THEN** it MUST use the descriptor-provided base address and stride
- **AND** existing QEMU async UART behavior MUST remain unchanged

#### Scenario: upper TTY stack remains unaffected

- **WHEN** Q18 changes platform descriptor and early console plumbing
- **THEN** `ntty_async.rs`, line discipline, and `/dev/console` behavior MUST remain unchanged unless a task explicitly updates the change design and receives approval
