## Context

Q19C-M0/M1/M2 have enough evidence for the current D1 async UART performance goal. The remaining work is cleanup: remove canceled M3/rootfs-probe code and close the OpenSpec trail.

The cleanup touches multiple files and build modes. It is not a lightweight change because it removes a feature, a Makefile target, cfg exclusions, and an entry branch.

## Design Decisions

### D1: Remove rootfs-probe as a product mode

Remove the feature and target instead of leaving them as deprecated.

Reasons:

- Q19C no longer uses rootfs-probe as evidence.
- Keeping a build target invites accidental retesting of a canceled gate.
- Storage/rootfs bring-up must be proposed separately.

### D2: Preserve historical evidence

Do not delete documents such as `docs/M3.md` or archived changes as part of the code cleanup.

Reasons:

- They explain why M3 was canceled.
- They are useful if storage/rootfs is reopened.
- Deleting historical evidence would make the direction change harder to audit.

### D3: Keep QEMU and future VF2 storage separate

Remove only D1 rootfs-probe symbols.

Do not remove:

- QEMU rootfs commands and `rootfs` Makefile support.
- `vf2` feature or `axfeat/driver-sdmmc`.
- General rootfs learned/reference notes.

### D4: Archive order matters

Archive `q19c-m2-m3-acceptance-alignment` first because it is already complete and feeds the revised Q19C semantics. Then finish `q19c-lichee-full-starryos-benchmark` evidence tables and archive it.

## Requirements Traceability Matrix

| Requirement | Task(s) | Coverage | Simplification | Status |
|-------------|---------|----------|----------------|--------|
| R1 remove M3/rootfs-probe code entry | 2.1-2.6 | 100% | None | Covered |
| R2 keep M0/M1/M2 benchmark modes | 2.7, 3.1-3.3 | 100% | None | Covered |
| R3 preserve history, do not claim rootfs success | 1.1, 4.1-4.4 | 100% | None | Covered |
| R4 close evidence tables | 3.1-3.3 | 100% | None | Covered |
| R5 validate and archive | 5.1-5.6 | 100% | Archive may happen after user approves execution | Covered |

No requirement is missing. No simplification needs approval beyond the user request to remove current M3 code.

## Verification Plan

### Static Checks

- `rg "lichee-d1-rootfs-probe|lichee-rootfs-probe|rootfs-probe" Cargo.toml kernel/Cargo.toml kernel/src Makefile`
- `openspec validate q19c-async-uart-closeout --strict`
- `openspec validate --changes`
- `openspec validate --specs`

### Build Checks

- `AX_CONFIG_PATH=$PWD/.axconfig.toml cargo check --target riscv64gc-unknown-none-elf --features "axfeat/myplat axfeat/bus-mmio lichee-d1-fullbench-command"`
- `AX_CONFIG_PATH=$PWD/.axconfig.toml cargo check --target riscv64gc-unknown-none-elf --features "axfeat/myplat axfeat/bus-mmio lichee-d1-fullbench"`
- Negative check: `lichee-d1-rootfs-probe` should no longer be an accepted feature.

### Documentation Checks

- `openspec list` shows the closeout change active.
- `q19c-m2-m3-acceptance-alignment` is archived or ready to archive.
- `q19c-lichee-full-starryos-benchmark` has no remaining M3/rootfs-probe task as a pending gate.

## Execution Boundary

This plan does not execute removal. Phase 3 should use `openspec-act`.
