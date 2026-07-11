## MODIFIED Requirements

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

Q19C MUST record benchmark evidence with enough context to compare QEMU, Q19B embedded, Q19C memory-root path, Q19C memory-root command, and Q19C rootfs probe/rootfs path results without conflating their startup chains.

#### Scenario: Recording M2 argv evidence

- **WHEN** M2 command-entry records argv/envp evidence
- **THEN** the evidence MUST distinguish kernel-side argv/envp construction from user-observed argv/envp output
- **AND** user-observed argv/envp MUST NOT be claimed unless the user payload prints argc/argv/envp or an equivalent marker.

#### Scenario: Recording board pending status

- **WHEN** M2 or M3 has passed host cargo/image gates but has not run on D1 hardware
- **THEN** project task status MUST record host gate as done and board gate as pending
- **AND** the overall board-dependent item MUST NOT be marked fully complete.

### Requirement: Rootfs mode requires real block device witness

Q19C rootfs path mode MUST only call `axfs-ng::init_filesystems()` after a real Lichee block device is available to the filesystem layer. Q19C MAY stop at `lichee-rootfs-probe` blocker evidence when SDMMC/block support is not yet implemented.

#### Scenario: Probe-only evidence is used

- **WHEN** `lichee-rootfs-probe` runs without a registered D1 block device
- **THEN** it MUST report `SKIPPED` with a concrete SDMMC/block blocker
- **AND** it MUST report that rootfs init was not called
- **AND** it MUST NOT record rootfs benchmark success.

#### Scenario: Register probe is not implemented

- **WHEN** M3 does not perform SDMMC MMIO/register reads
- **THEN** the evidence MUST identify register probing as skipped or TBD
- **AND** it MUST NOT claim controller MMIO accessibility or first block read success.

## ADDED Requirements

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
