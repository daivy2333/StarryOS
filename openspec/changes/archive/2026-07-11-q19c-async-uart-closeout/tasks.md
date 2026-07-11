## 1. Current-State Witness

- [ ] 1.1 Record current active changes with `openspec list`.
- [ ] 1.2 Record current rootfs-probe code references with `rg`.
- [ ] 1.3 Record current Q19C remaining unchecked tasks.

## 2. Remove M3/rootfs-probe Code

- [ ] 2.1 Remove top-level `lichee-d1-rootfs-probe` feature from `Cargo.toml`.
- [ ] 2.2 Remove kernel `lichee-d1-rootfs-probe` feature from `kernel/Cargo.toml`.
- [ ] 2.3 Remove `make lichee-rootfs-probe` target and `.PHONY` entry from `Makefile`.
- [ ] 2.4 Remove rootfs-probe mode label and M3 branch from `kernel/src/entry.rs`.
- [ ] 2.5 Remove rootfs-probe cfg exclusions from `kernel/src/lib.rs` and `kernel/src/drivers/mod.rs`.
- [ ] 2.6 Remove rootfs-probe feature-combination guards from `kernel/src/lib.rs`.
- [ ] 2.7 Confirm Q19B, M1, and M2 entry paths still compile by code inspection before build checks.

## 3. Complete Q19C Evidence Tables

- [ ] 3.1 Add Q19B embedded result table: image, mode, startup chain, benchmark summary, raw log reference.
- [ ] 3.2 Add Q19C-M0 evidence table: manifest fields, QEMU/Q19B parameter differences, RX witness, 64B small-packet result.
- [ ] 3.3 Ensure Q19C-M2 `docs/M2.md` is the final board evidence for command-entry.

## 4. Sync OpenSpec Semantics

- [ ] 4.1 Update `q19c-lichee-full-starryos-benchmark` tasks so no M3/rootfs-probe removal item remains pending outside this closeout change.
- [ ] 4.2 Update specs/learned/architecture/optimization only if code deletion changes the current truth.
- [ ] 4.3 Keep historical M3 facts as canceled-scope evidence, not success.
- [ ] 4.4 Do not delete `docs/M3.md` or archived M3 analysis.

## 5. Validation and Archive Prep

- [ ] 5.1 Run `cargo check` for `lichee-d1-fullbench-command`.
- [ ] 5.2 Run `cargo check` for `lichee-d1-fullbench`.
- [ ] 5.3 Run a negative feature check proving `lichee-d1-rootfs-probe` is gone.
- [ ] 5.4 Run `openspec validate q19c-async-uart-closeout --strict`.
- [ ] 5.5 Run `openspec validate --changes` and `openspec validate --specs`.
- [ ] 5.6 Archive order plan: `q19c-m2-m3-acceptance-alignment` first, then `q19c-lichee-full-starryos-benchmark`, then this closeout change after execution.
