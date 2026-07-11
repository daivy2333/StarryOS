## Purpose

Define the conditions under which `lichee-d1-rootfs-probe` runs on D1 hardware, ensuring probe-only blocker evidence is not blocked by unrelated startup benchmark dependencies, and that real block device witness is required before rootfs initialization.

## Requirements

### Requirement: Rootfs mode requires real block device witness

Q19C rootfs path mode MUST only call `axfs-ng::init_filesystems()` after a real Lichee block device is available to the filesystem layer. Q19C MAY stop at `lichee-rootfs-probe` blocker evidence when SDMMC/block support is not yet implemented.

#### Scenario: Probe-only evidence is not blocked by startup benchmark

- **WHEN** `lichee-d1-rootfs-probe` runs on D1 hardware
- **THEN** it MUST print the rootfs-probe blocker table before any optional startup benchmark dependency can block output
- **AND** it MUST report `SKIPPED` with a concrete SDMMC/block blocker
- **AND** it MUST report that rootfs init was not called
- **AND** it MUST NOT record rootfs benchmark success.

#### Scenario: Startup benchmark issue is separate evidence

- **WHEN** `bench::run_startup_benchmark()` stalls, truncates, or fails in a rootfs-probe feature set
- **THEN** that result MUST be tracked as a UART startup benchmark issue
- **AND** it MUST NOT be treated as SDMMC/rootfs probe evidence
- **AND** it MUST NOT prevent M3 from printing the probe-only blocker table.
