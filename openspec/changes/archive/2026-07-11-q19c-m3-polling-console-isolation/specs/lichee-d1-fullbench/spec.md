## MODIFIED Requirements

### Requirement: Rootfs mode requires real block device witness

Q19C rootfs path mode MUST only call `axfs-ng::init_filesystems()` after a real Lichee block device is available to the filesystem layer. Q19C MAY stop at `lichee-rootfs-probe` blocker evidence when SDMMC/block support is not yet implemented.

#### Scenario: Probe-only evidence uses polling console first

- **WHEN** `lichee-d1-rootfs-probe` runs on D1 hardware before D1 SDMMC/block support exists
- **THEN** it MUST use polling console output for the probe-only board proof
- **AND** it MUST NOT initialize D1 async UART before printing the probe table
- **AND** it MUST report `SKIPPED` with a concrete SDMMC/block blocker
- **AND** it MUST report that rootfs init was not called
- **AND** it MUST NOT record rootfs benchmark success.

#### Scenario: Async UART is not a M3 probe dependency

- **WHEN** `lichee-d1-rootfs-probe` is selected as the only Lichee fullbench mode
- **THEN** it MUST compile without enabling `lichee-d1-async-uart`
- **AND** it MUST not require async UART startup benchmark output before M3 can pass its board gate.

#### Scenario: Probe table completes before rootfs work

- **WHEN** the M3 polling-console board proof runs
- **THEN** the serial log MUST include the D1 SDMMC known facts
- **AND** it MUST include `block_status=SKIPPED: missing D1 SDMMC/block driver`
- **AND** it MUST include `rootfs_init=NOT called`
- **AND** it MUST include `probe complete, halting. No panic.`
- **AND** it MUST NOT include `No block device found!`.

### Requirement: Fullbench mode features are mutually exclusive

Lichee fullbench mode features MUST be selected as one runtime mode at a time, or the build MUST fail with a clear error before compiling unreachable or partially gated paths.

#### Scenario: Rootfs probe cannot be combined with async UART

- **WHEN** `lichee-d1-rootfs-probe` and `lichee-d1-async-uart` are both enabled
- **THEN** the build MUST fail with an explicit mode-selection error
- **AND** it MUST NOT rely on UART runtime hangs, branch ordering, or missing module imports as the failure mechanism.

#### Scenario: Async UART benchmark modes keep their path

- **WHEN** M2 command-entry or other async UART benchmark modes are selected without rootfs-probe
- **THEN** they MUST keep their async UART initialization path
- **AND** their cargo/image gates MUST remain valid after the M3 polling-console change.
