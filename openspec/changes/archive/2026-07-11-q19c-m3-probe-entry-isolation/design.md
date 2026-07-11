## Design

M3 should be treated as a short probe report. It does not need the UART startup benchmark before printing SDMMC/rootfs blocker evidence.

Current entry order:

1. print M3 mode label
2. initialize D1 async UART
3. run `bench::run_startup_benchmark()`
4. print rootfs-probe evidence

Observed board output stops at step 3. The implementation should change the entry order or cfg so M3 reaches the probe table before any startup benchmark work.

## Planned Implementation

Preferred option:

- Gate `bench::run_startup_benchmark()` with `#[cfg(not(feature = "lichee-d1-rootfs-probe"))]`.
- Keep M2/fullbench behavior unchanged.
- Let M3 print probe evidence immediately after UART init.
- Keep the final `wfi` halt after `probe complete, halting. No panic.`

Alternative option:

- Move the M3 probe block before startup benchmark.
- Do not run startup benchmark in M3 unless a follow-up diagnostic flag is added.

The preferred option has fewer ordering surprises and makes the mode boundary explicit.

## Requirements Traceability

| Requirement | Task(s) | Coverage | Simplification | Status |
|-------------|---------|----------|----------------|--------|
| R1 M3 probe evidence appears before benchmark risk | 1.1, 2.1, 4.3 | 100% | None | Covered |
| R2 M2/fullbench startup benchmark remains covered | 1.2, 3.1 | 100% | None | Covered |
| R3 No false SDMMC/rootfs success | 2.2, 2.3 | 100% | None | Covered |
| R4 Startup benchmark issue is preserved | 5.1, 5.2 | 100% | Deferred fix as O81 | Covered |

## Verification Plan

Host gates:

- `AX_CONFIG_PATH=$PWD/.axconfig.toml cargo check --target riscv64gc-unknown-none-elf --features "axfeat/myplat axfeat/bus-mmio lichee-d1-rootfs-probe"`
- `make lichee-rootfs-probe`
- `python3 tools/android_boot_image.py inspect starry-lichee-rootfs-probe-boot.img`

Regression gates:

- M2 command mode cargo check still passes.
- M2 image build still passes if touched by cfg changes.
- Incompatible feature combinations still fail with explicit mode-selection errors.

Board gate:

- D1 serial log includes `log_label=lichee-rootfs-probe`.
- D1 serial log includes `block_status=SKIPPED: missing D1 SDMMC/block driver`.
- D1 serial log includes `rootfs_init=NOT called`.
- D1 serial log includes `probe complete, halting. No panic.`
- D1 serial log does not include `No block device found!`.

## Open Questions

- O81 still needs a separate reproducer to decide whether the startup benchmark stopped, trapped, or only failed to flush output.
- M3 does not need that answer before probe-only board evidence can proceed.
