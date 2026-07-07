## ADDED Requirements

### Requirement: Fullbench modes are explicit

Q19C MUST provide explicit Lichee fullbench runtime modes that are distinguishable from Q19B embedded userbench and from QEMU.

#### Scenario: Mode label is printed

- **WHEN** any Q19C fullbench image boots
- **THEN** the serial log MUST print the active startup chain label before user application launch
- **AND** the label MUST be one of `lichee-memory-root-path`, `lichee-memory-root-shell`, `lichee-rootfs-probe`, or `lichee-rootfs-path`.

#### Scenario: No silent fallback to embedded loader

- **WHEN** a Q19C fullbench mode cannot launch its configured path
- **THEN** it MUST report the failing path and stage
- **AND** it MUST NOT silently fall back to `load_embedded_user_app()` and report fullbench success.

### Requirement: Preserve embedded userbench regression path

Q19C MUST preserve the Q19B embedded benchmark path as an explicit regression baseline, so that Lichee async UART, PLIC IRQ 18, TTY, syscall, `tcdrain`, and FIONBIO evidence remains independently runnable.

#### Scenario: Embedded userbench still runs after Q19C

- **WHEN** the existing Lichee userbench target is built and booted
- **THEN** it MUST run the embedded benchmark ELF path without requiring a rootfs or block device
- **AND** the embedded benchmark MUST exit with code 0 before it is accepted as a regression pass
- **AND** its output MUST remain distinguishable from Q19C fullbench output.

#### Scenario: Regression failure is isolated

- **WHEN** Q19B embedded userbench fails after Q19C changes
- **THEN** Q19C validation MUST treat it as a regression in the baseline path
- **AND** it MUST NOT attribute the failure to SDMMC/rootfs work without evidence.

### Requirement: Memory-root path loader fullbench

Lichee fullbench MUST provide a memory-root path mode in which `/bin/benchmark` is reachable through the VFS namespace and started through `load_user_app()`.

#### Scenario: Benchmark starts through VFS resolve

- **WHEN** Lichee fullbench boots in memory-root path mode
- **THEN** `FS_CONTEXT.resolve("/bin/benchmark")` MUST resolve the benchmark file
- **AND** the process MUST be loaded through the path-based `load_user_app()` flow
- **AND** benchmark output MUST be labeled as `lichee-memory-root-path`.

#### Scenario: Benchmark path is missing

- **WHEN** `/bin/benchmark` cannot be resolved in memory-root path mode
- **THEN** the system MUST report root provider, requested path, and resolve error
- **AND** it MUST NOT report benchmark success.

#### Scenario: Loaded process fails before printing

- **WHEN** `load_user_app()` returns successfully but the spawned init process exits or aborts before printing any benchmark section
- **THEN** the system MUST report spawn exit status and stage reached
- **AND** it MUST NOT classify the run as a successful path-loader proof.

#### Scenario: Memory-root still mounts pseudo filesystems

- **WHEN** memory-root path mode initializes its root
- **THEN** `pseudofs::mount_all()` MUST still provide `/dev`, `/dev/shm`, `/tmp`, `/proc`, and `/sys`
- **AND** `/dev/console` MUST be available for stdin, stdout, and stderr.

### Requirement: Shell or script benchmark parity

Q19C MUST provide a shell/script-triggered benchmark path after memory-root path loading works, so the Lichee workflow exercises the same user-entry class as QEMU.

#### Scenario: Shell starts benchmark

- **WHEN** `lichee-memory-root-shell` mode is selected and `/bin/sh` is available
- **THEN** the init process SHOULD use arguments equivalent to `["/bin/sh", "-c", "/init.sh"]`
- **AND** the benchmark MUST be launched by the shell/script path rather than by kernel direct dispatch.

#### Scenario: Equivalent script entry is used

- **WHEN** a static shell is unavailable
- **THEN** Q19C MUST provide a documented equivalent command entry
- **AND** that entry MUST verify argv/envp, stdio, process exit, and join behavior on board.

#### Scenario: Shell dependency missing

- **WHEN** `/bin/sh`, `/init.sh`, the interpreter, or a required library is missing
- **THEN** the system MUST report the missing path and loader stage
- **AND** it MUST NOT record shell/script benchmark success.

### Requirement: Rootfs mode requires real block device witness

Q19C rootfs path mode MUST only call `axfs-ng::init_filesystems()` after a real Lichee block device is available to the filesystem layer. Q19C MAY stop at `lichee-rootfs-probe` evidence when SDMMC/block support is not yet implemented.

#### Scenario: No block device is present

- **WHEN** rootfs mode is requested but no block device is registered
- **THEN** the system MUST avoid the `No block device found!` panic path
- **AND** it MUST report that rootfs benchmark is blocked by missing block device support
- **AND** it MUST include SDMMC/block probe summary in the serial log or captured evidence.

#### Scenario: Rootfs proof is skipped after probe

- **WHEN** SDMMC/block probe identifies that no usable block device exists yet
- **THEN** Q19C evidence MUST record `SKIPPED` with a blocker summary for `lichee-rootfs-path`
- **AND** the skipped rootfs path MUST NOT block acceptance of memory-root path loader proof.

#### Scenario: Block device is present

- **WHEN** a Lichee block device is registered and contains a supported rootfs
- **THEN** `axfs-ng::init_filesystems()` MUST initialize the root filesystem from that block device
- **AND** `mount_all()` and path-based benchmark loading MUST operate from that rootfs namespace
- **AND** benchmark output MUST be labeled as `lichee-rootfs-path`.

### Requirement: SDMMC exploration records hardware facts

Q19C Part B MUST record enough D1 SDMMC/block facts to distinguish hardware bring-up blockers from StarryOS filesystem or loader bugs. It MUST NOT require a full D1 SDMMC driver implementation inside Q19C.

#### Scenario: SDMMC probe runs

- **WHEN** SDMMC/block exploration is executed on Lichee RV Dock
- **THEN** the evidence MUST include controller MMIO accessibility, clock/reset state, card detect or equivalent status, transfer mode, and first block read result
- **AND** if IRQ is used, the evidence MUST include IRQ claim/complete behavior.

#### Scenario: PIO-first path is used

- **WHEN** DMA/cache behavior is not yet proven
- **THEN** Q19C MAY use a PIO-first probe path if implementation effort is acceptable
- **AND** the serial log MUST identify that DMA is not part of the current rootfs proof.

### Requirement: Rootfs image content is specified

Q19C rootfs benchmark MUST use a documented rootfs image layout so that failures can be traced to image content, filesystem mounting, or ELF loading.

#### Scenario: Rootfs image is prepared

- **WHEN** a rootfs image is used for Q19C
- **THEN** the evidence MUST record filesystem type, image source, benchmark binary path, shell path if present, init script path if present, and dynamic interpreter/library requirements.

#### Scenario: Rootfs file is missing

- **WHEN** `/bin/benchmark`, `/bin/sh`, or `/init.sh` is missing from rootfs mode
- **THEN** the failure MUST identify the missing path and rootfs provider
- **AND** it MUST NOT be recorded as a UART or syscall failure.

### Requirement: Benchmark evidence is chain-specific

Q19C MUST record benchmark evidence with enough context to compare QEMU, Q19B embedded, Q19C memory-root path, Q19C memory-root shell/script, and Q19C rootfs path results without conflating their startup chains.

#### Scenario: Recording benchmark evidence

- **WHEN** a benchmark result is documented
- **THEN** the record MUST include board/target, image name, git revision, feature set, startup chain, root provider, loader, benchmark output summary, exit code, and raw serial log reference.

#### Scenario: Recording skipped board evidence

- **WHEN** a board-dependent Q19C result cannot be produced because a gate was not reached
- **THEN** the evidence record MUST contain `SKIPPED` and a concrete blocker summary
- **AND** it MUST NOT fabricate benchmark or rootfs data.

#### Scenario: Comparing results

- **WHEN** Q19C results are compared to QEMU or Q19B
- **THEN** the comparison MUST state whether the benchmark used physical UART line delay, embedded bytes, memory-root path loading, shell/script entry, or real rootfs.

### Requirement: D1 TX optimization preserves progress

Q19C D1 TX optimizations MUST preserve the known-good embedded userbench startup and drain progress before claiming throughput or latency improvements.

#### Scenario: Optimizing TX wake or retry policy

- **WHEN** Q19C changes TX copier retry limits, THRE wake registration, or `tcdrain`/`flush` wake behavior
- **THEN** the D1 userbench MUST still reach the first user benchmark section after `benchmark process spawned`
- **AND** it MUST complete the embedded benchmark with exit code 0 before the optimization is accepted.

#### Scenario: Avoiding a disproven THRE-only progress dependency

- **WHEN** an optimization depends on D1 THRE interrupts as the only forward-progress source after FIFO fill
- **THEN** the design MUST include a software fallback or bounded polling path
- **AND** it MUST NOT use the disproven `TX_FAST_RETRY_LIMIT=0` plus drain-side `TX_WAKER` registration scheme as the default behavior.

#### Scenario: Recording remaining TX inefficiency

- **WHEN** D1 TX diagnostic snapshots are captured
- **THEN** the evidence MUST record `hw_send_zero`, `no_progress_budget_exhausted`, `hw_send_max_chunk`, and P99 drain/latency behavior
- **AND** the report MUST distinguish proven fixes from open optimization work.

### Requirement: Boot image size remains visible

Q19C MUST record Android boot image size for every Lichee fullbench image because embedded benchmark, shell, script, and rootfs-related payload can increase kernel image size.

#### Scenario: Fullbench image is built

- **WHEN** a Q19C Lichee fullbench boot image is produced
- **THEN** the evidence MUST include `kernel_size`, `kernel_addr`, image name, and whether `DWARF=n` was used.
