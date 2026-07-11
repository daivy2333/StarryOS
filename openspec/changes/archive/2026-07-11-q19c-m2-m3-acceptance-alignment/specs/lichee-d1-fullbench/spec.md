## ADDED Requirements

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

Q19C MUST record benchmark evidence with enough context to compare QEMU, Q19B embedded, Q19C memory-root path, and Q19C memory-root command results without conflating their startup chains.

#### Scenario: Recording M2 argv evidence

- **WHEN** M2 command-entry records argv/envp evidence
- **THEN** the evidence MUST distinguish kernel-side argv/envp construction from user-observed argv/envp output
- **AND** user-observed argv/envp MUST NOT be claimed unless the user payload prints argc/argv/envp or an equivalent marker.

#### Scenario: Recording board status

- **WHEN** M2 has passed D1 board benchmark
- **THEN** project task status MUST record M2 board gate as complete
- **AND** the evidence MUST reference the serial log or document that contains benchmark sections and exit code.

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
