## MODIFIED Requirements

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

## APPENDIX: Final Evidence Tables

### Appendix A: Q19B Embedded Userbench Result

| Field | Value |
|-------|-------|
| Image | `starry-lichee-userbench-boot.img` (Android boot image, D1 payload) |
| Mode | `lichee-d1-userbench` (embedded ELF via `include_bytes!`) |
| Startup chain | U-Boot → OpenSBI → StarryOS D1 payload → embedded benchmark ELF |
| UART init | D1 32-bit MMIO stride-4, LSR readable ✅, ring buffer 64 KB × 2, IRQ 18 registered ✅ |
| Kernel benchmark | Ring buffer write: 1163 MB/s; RX read: 8392 MB/s; FIFO depth: 16 B; RX latency P99: 246 ns |
| Userbench syscalls | `set_tid_address`, `ioctl(TIOCGWINSZ)`, `writev`, `openat("/dev/console")`, `brk×2`, `mmap×2`, `clock_gettime`, `write+ioctl(TCDRAIN)`, `FIONBIO` — all passed |
| TX throughput (115200 bps) | 64 B: 7.9% line rate; 256 B: 98.9% line rate; 1024 B: 99.3% line rate |
| Exit code | `benchmark exited with code: 0` ✅ |
| tcdrain | Completed per-section with staged drain, no data loss |
| FIONBIO | `ioctl(0x3, 0x5421, &1)` → OK; non-blocking mode verified |
| Raw log | `.claude/analysis/_archive/2026-07-04-q19-lichee-analysis/lichee/userbench` |
| Archived change | `openspec/changes/archive/2026-07-02-q19b-lichee-d1-benchmark/` |

### Appendix B: Q19C-M0 Benchmark Evidence

| Field | Value |
|-------|-------|
| Manifest | `tests/benchmark.c` — unified QEMU/D1 manifest |
| Removed | 4096 B long-timeout item (S0), S10 recheck (second drain), S11 second-drain diagnostic |
| D1-specific fixes | FIFO 16 B burst accounting (S6 `hw_send_max_chunk=16`), TTY short-write fix (S12/S13), 64 B pre-section drain |
| Q19C.8e slow-pool | `TX_SLOW_POLL_LIMIT=4096` + `TX_YIELD_RETRIES=4`; `slow_poll_exh=0` on D1 board (100% slow-pool success rate) |
| 64 B small-packet result | 93%–97% line rate after pre-section drain (was ~7.9% with measurement contamination) |
| P99 TX latency | 50.86 ms long-tail (root cause not yet identified; throughput impact <2%; deferred to Q20 revalidation) |
| Q19B regression | Exit code 0 ✅; `hw_send_max_chunk=16` ✅; FIONBIO PASS ✅ |
| QEMU parameter diff | QEMU uses stride-1 NS16550; D1 uses stride-4 DW APB UART with 32-bit MMIO |
| Analysis doc | `.claude/analysis/q19c-d1-tx-optimization.md` (Q19C.8e D1 TX zero-send / P99 long-tail) |

### Appendix C: Q19C-M2 Command-Entry Board Evidence (Final)

| Field | Value |
|-------|-------|
| Image | `starry-lichee-fullbench-command-boot.img` |
| Mode | `lichee-d1-fullbench-command` (memory-root command-entry) |
| Startup chain | Android boot image → memory-root `/bin/benchmark` → eager ELF mapping (equivalent command entry) |
| Shell | `shell_status=SKIPPED` (no known-good static `/bin/sh` for D1) |
| Board evidence | `docs/M2.md` — full benchmark sections, `Done.`, `benchmark exited with code: 0`, `halting.` |
| Comment | This is the final Q19C command-entry proof. True `/bin/sh` shell not required for async UART completion. |
