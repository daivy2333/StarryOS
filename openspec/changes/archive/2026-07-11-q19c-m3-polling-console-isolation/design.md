## Context

The previous M3 entry-isolation change removed the startup benchmark from rootfs-probe. Board output then advanced to the probe entry but still stopped after:

```text
[starry-d1] d1_sdmmc_controller_base=TBD (from D1 User Manual / DTS)
```

The next planned output is another `ax_println!`. That narrows the current issue to console/UART ownership, not SDMMC or rootfs logic.

Current code gives M3 two UART paths:

- `kernel/Cargo.toml` makes `lichee-d1-rootfs-probe` enable `lichee-d1-async-uart`.
- `kernel/src/entry.rs` routes all async UART D1 modes through `lichee_d1_init`.
- `lichee_d1_init` initializes async UART before the rootfs-probe table.
- `crates/axplat-riscv64-lichee-d1/src/console.rs` still implements polling console writes to UART0.

For the next proof, M3 should use one UART owner.

## Design

M3 becomes a polling-console-only probe mode.

The implementation should:

- remove `lichee-d1-async-uart` from the `lichee-d1-rootfs-probe` feature dependency;
- route `lichee-d1-rootfs-probe` through a dedicated D1 probe entry;
- keep that entry independent of async UART init, async UART benchmark, task spawning, and pseudofs setup;
- print the existing probe facts and halt with `wfi`;
- reject explicit `lichee-d1-rootfs-probe + lichee-d1-async-uart` builds with `compile_error!`.

The M3 entry should keep current probe wording conservative:

- controller base can remain TBD;
- register probing can remain SKIPPED/TBD;
- block provider can remain SKIPPED;
- rootfs init must remain NOT called.

## Alternatives

### Keep async UART and change print ordering

This still leaves two UART owners in the same proof path. It may move the stop point, but it does not isolate the failure boundary.

### Use async UART for all M3 output now

That would turn M3 into an async UART coexistence task. It is useful later, but it blocks the current rootfs-probe path proof.

### Implement SDMMC first

This does not address the current stop before any block access. It also expands Q19C beyond probe-only evidence.

## Requirements Traceability

| Requirement | Task(s) | Coverage | Simplification | Status |
|-------------|---------|----------|----------------|--------|
| R1 M3 uses one UART owner | 1.1-1.4, 2.1 | 100% | async UART reintroduction deferred | Covered |
| R2 Probe table completes on board | 2.2, 4.4 | 100% | none | Covered |
| R3 No false rootfs success | 2.3, 4.4 | 100% | none | Covered |
| R4 M2/async modes unchanged | 3.1-3.4 | 100% | none | Covered |
| R5 Invalid feature combo is explicit | 1.2, 4.2 | 100% | none | Covered |

Gate 2 result: no uncovered requirement. The only deferred item is async UART reintroduction into M3, and it is outside this change by user-approved flow.

## Verification Plan

Host gates:

- `openspec validate q19c-m3-polling-console-isolation --strict`
- `openspec validate --changes`
- `AX_CONFIG_PATH=$PWD/.axconfig.toml cargo check --target riscv64gc-unknown-none-elf --features "axfeat/myplat axfeat/bus-mmio lichee-d1-rootfs-probe"`
- `make lichee-rootfs-probe`
- negative check: `lichee-d1-rootfs-probe + lichee-d1-async-uart` fails with explicit `compile_error!`

Regression gates:

- M2 command cargo check still passes.
- `make lichee-fullbench-command` still builds.
- Existing async UART modes still route through async UART init.

Board gate:

- D1 serial log includes `log_label=lichee-rootfs-probe`.
- D1 serial log includes `d1_sdmmc_controller_base=TBD`.
- D1 serial log includes `block_status=SKIPPED: missing D1 SDMMC/block driver`.
- D1 serial log includes `rootfs_init=NOT called`.
- D1 serial log includes `probe complete, halting. No panic.`
- D1 serial log does not include `No block device found!`.

## Open Questions

- After polling-console M3 passes, a follow-up should decide whether async UART can safely replace or coexist with platform console on D1.
