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

### Requirement: Shell or script benchmark parity

Q19C M2 MUST accept `lichee-memory-root-command` as the required Lichee memory-root command-entry proof. A true shell path MAY be implemented later only when a known-good static `/bin/sh` and its dependencies are available; Q19C MUST NOT require implementing or importing a shell.

#### Scenario: Command-entry is the accepted M2 path

- **WHEN** Q19C M2 runs without a known-good static `/bin/sh`
- **THEN** the serial log MUST label the mode as `lichee-memory-root-command`
- **AND** it MUST record `shell_status=SKIPPED` with a concrete blocker summary
- **AND** it MUST launch the documented equivalent entry for `/bin/benchmark`
- **AND** it MUST verify stdio, process exit, and join behavior before M2 is accepted.

#### Scenario: Command-entry is not shell success

- **WHEN** Q19C M2 uses the documented equivalent command entry
- **THEN** the evidence MUST NOT record `lichee-memory-root-shell` success
- **AND** it MUST NOT claim `/bin/sh -c /init.sh` ran unless that exact path was executed.

#### Scenario: True shell is future optional

- **WHEN** a known-good static shell is later provided
- **THEN** shell execution SHOULD be planned in a follow-up change
- **AND** that change MUST document shell payload source, interpreter/library needs, and `/proc/self/exe` behavior before using shell success as evidence.

### Requirement: Benchmark evidence is chain-specific

Q19C closeout MUST record the final D1 async UART evidence without conflating Q19B embedded, Q19C memory-root path, and Q19C memory-root command startup chains.

#### Scenario: Final evidence tables are added

- **WHEN** Q19C closeout documentation is updated
- **THEN** it MUST include a Q19B embedded result table
- **AND** it MUST include a Q19C-M0 benchmark evidence table
- **AND** it MUST point to Q19C-M2 command-entry board evidence as the final D1 command proof.

#### Scenario: Recording M2 argv evidence

- **WHEN** M2 command-entry records argv/envp evidence
- **THEN** the evidence MUST distinguish kernel-side argv/envp construction from user-observed argv/envp output
- **AND** user-observed argv/envp MUST NOT be claimed unless the user payload prints argc/argv/envp or an equivalent marker.

#### Scenario: Recording board status

- **WHEN** M2 has passed D1 board benchmark
- **THEN** project task status MUST record M2 board gate as complete
- **AND** the evidence MUST reference the serial log or document that contains benchmark sections and exit code.

#### Scenario: Storage work is reopened later

- **WHEN** D1 SDMMC/block/rootfs work is requested after closeout
- **THEN** it MUST be proposed as a new change
- **AND** it MUST define separate storage/rootfs acceptance gates.

---

### Requirement: M3 rootfs-probe is canceled as a current gate

Q19C MUST NOT require M3 rootfs-probe, SDMMC, block, shell, or real rootfs evidence for async UART performance completion. Historical M3 output MAY be kept as canceled-scope evidence, but it MUST NOT block Q19C.

#### Scenario: M3 output remains incomplete

- **WHEN** `lichee-rootfs-probe` output is incomplete or skipped
- **THEN** Q19C MUST still be evaluated from M0/M1/M2 async UART evidence
- **AND** the incomplete M3 output MUST NOT be recorded as UART benchmark failure.

#### Scenario: Storage/rootfs is reopened later

- **WHEN** D1 SDMMC/block/rootfs work is requested later
- **THEN** it MUST be planned in a follow-up change
- **AND** that change MUST define its own block/rootfs acceptance gates.

### Requirement: Fullbench mode features are mutually exclusive

Lichee fullbench mode features MUST be selected as one runtime mode at a time, or the build MUST fail with a clear error before compiling unreachable or partially gated paths.

#### Scenario: Incompatible mode features are selected

- **WHEN** two incompatible Lichee fullbench mode features are enabled together
- **THEN** the build MUST fail with an explicit mode-selection error
- **AND** it MUST NOT rely on branch ordering, unreachable code, or missing module imports as the failure mechanism.

#### Scenario: Single mode feature builds

- **WHEN** one Lichee fullbench mode feature is selected with the required platform features
- **THEN** the mode MUST compile independently
- **AND** its image target MUST preserve the expected mode label.

### Requirement: Fullbench modes are explicit

Q19C MUST provide explicit Lichee fullbench runtime modes that are distinguishable from Q19B embedded userbench and from QEMU.

#### Scenario: Mode label is printed

- **WHEN** any Q19C fullbench image boots
- **THEN** the serial log MUST print the active startup chain label before user application launch
- **AND** the label MUST be one of `lichee-memory-root-path` or `lichee-memory-root-command` for accepted Q19C results.

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

### Requirement: Memory-root path-visible fullbench

Lichee fullbench MUST provide a memory-root path mode in which `/bin/benchmark` is reachable through the VFS namespace and started from the VFS-provided file contents.

#### Scenario: Benchmark starts through VFS resolve

- **WHEN** Lichee fullbench boots in memory-root path mode
- **THEN** `FS_CONTEXT.resolve("/bin/benchmark")` MUST resolve the benchmark file
- **AND** the process MUST be loaded from the VFS-readable benchmark ELF rather than embedded bytes
- **AND** benchmark output MUST be labeled as `lichee-memory-root-path`.

#### Scenario: Benchmark path is missing

- **WHEN** `/bin/benchmark` cannot be resolved in memory-root path mode
- **THEN** the system MUST report root provider, requested path, and resolve error
- **AND** it MUST NOT report benchmark success.

#### Scenario: Loaded process fails before printing

- **WHEN** a path-visible benchmark process is created but the spawned init process exits or aborts before printing any benchmark section
- **THEN** the system MUST report spawn exit status and stage reached
- **AND** it MUST NOT classify the run as a successful path-loader proof.

#### Scenario: Memory-root still mounts pseudo filesystems

- **WHEN** memory-root path mode initializes its root
- **THEN** `pseudofs::mount_all()` MUST still provide `/dev`, `/dev/shm`, `/tmp`, `/proc`, and `/sys`
- **AND** `/dev/console` MUST be available for stdin, stdout, and stderr.

### Requirement: Command-entry benchmark parity

Q19C M2 MUST accept `lichee-memory-root-command` as the required Lichee memory-root command-entry proof. A true shell path MAY be implemented later only when a known-good static `/bin/sh` and its dependencies are available; Q19C MUST NOT require implementing or importing a shell.

#### Scenario: Command-entry is the accepted M2 path

- **WHEN** Q19C M2 runs without a known-good static `/bin/sh`
- **THEN** the serial log MUST label the mode as `lichee-memory-root-command`
- **AND** it MUST record `shell_status=SKIPPED` with a concrete blocker summary
- **AND** it MUST launch the documented equivalent entry for `/bin/benchmark`
- **AND** it MUST verify stdio, process exit, and join behavior before M2 is accepted.

#### Scenario: Command-entry is not shell success

- **WHEN** Q19C M2 uses the documented equivalent command entry
- **THEN** the evidence MUST NOT record `lichee-memory-root-shell` success
- **AND** it MUST NOT claim `/bin/sh -c /init.sh` ran unless that exact path was executed.

#### Scenario: Shell dependency missing

- **WHEN** `/bin/sh`, `/init.sh`, the interpreter, or a required library is missing
- **THEN** the system MUST report the missing path and loader stage
- **AND** it MUST NOT record shell/script benchmark success.

### Requirement: Rootfs and SDMMC are not Q19C gates

Q19C MUST remove the D1 rootfs-probe runtime mode from active build and runtime surfaces. Q19C MUST NOT require shell, SDMMC, block, rootfs-probe, or real rootfs benchmark evidence for async UART performance completion.

#### Scenario: Rootfs-probe feature is removed

- **WHEN** Q19C closeout code cleanup is applied
- **THEN** `lichee-d1-rootfs-probe` MUST no longer be an accepted Cargo feature
- **AND** `make lichee-rootfs-probe` MUST no longer be an accepted Makefile target
- **AND** no D1 runtime branch MUST print `lichee-rootfs-probe`.

#### Scenario: Accepted D1 UART benchmark modes remain

- **WHEN** Q19C closeout code cleanup is applied
- **THEN** Q19B embedded userbench MUST remain available
- **AND** Q19C memory-root path mode MUST remain available
- **AND** Q19C memory-root command mode MUST remain available.

#### Scenario: Historical M3 evidence is retained

- **WHEN** M3/rootfs-probe evidence appears in docs or archived changes
- **THEN** it MUST be described as canceled or historical scope
- **AND** it MUST NOT be used as Q19C completion evidence
- **AND** it MUST NOT claim SDMMC register probe or rootfs benchmark success.

#### Scenario: Rootfs work is requested after Q19C

- **WHEN** D1 storage/rootfs bring-up is requested after Q19C
- **THEN** it MUST be planned as a new change
- **AND** it MUST NOT be treated as a missing Q19C async UART benchmark gate.

#### Scenario: M3/rootfs-probe remains incomplete

- **WHEN** historical M3/rootfs-probe board evidence is incomplete
- **THEN** Q19C MUST still be allowed to complete from M0/M1/M2 evidence
- **AND** the incomplete probe MUST be recorded as canceled current scope, not as a failed UART benchmark.

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

