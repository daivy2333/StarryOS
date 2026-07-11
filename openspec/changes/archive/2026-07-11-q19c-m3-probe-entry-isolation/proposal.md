## Why

`docs/M3.md` shows that the current M3 image starts, initializes D1 async UART, and then stops or truncates during `bench::run_startup_benchmark()`.

The log does not reach the `lichee-d1-rootfs-probe` evidence block in `kernel/src/entry.rs`. This means M3 board evidence is currently blocked by a UART startup benchmark path, not by SDMMC/rootfs probe logic.

Q19C M3 is a probe-only/blocker report. It should print D1 SDMMC known facts, block provider status, `rootfs_init=NOT called`, and halt without requiring the startup benchmark to complete first.

## What Changes

- Isolate the M3 rootfs-probe entry from `bench::run_startup_benchmark()`.
- Keep startup benchmark coverage in M2/fullbench paths.
- Preserve M3 probe-only evidence wording: TBD/SKIPPED, no register probe success, no rootfs benchmark success.
- Track the startup benchmark truncation as O81 for later UART benchmark investigation.
- Update board gate expectations so M3 passes only when the probe table appears on D1 serial output with no panic.

## Capabilities

### Modified Capabilities

- `lichee-d1-fullbench`: refine M3 `lichee-d1-rootfs-probe` entry behavior.

### New Capabilities

None.

## Scope

### In Scope

- `kernel/src/entry.rs` entry ordering or cfg guards for `lichee-d1-rootfs-probe`.
- M3 board log criteria.
- Host cargo check and image build for `lichee-d1-rootfs-probe`.
- Documentation of the startup benchmark risk as a separate UART issue.

### Out of Scope

- Implementing D1 SDMMC/block driver.
- Calling `axfs-ng::init_filesystems()` on D1 without a block device.
- Fixing startup benchmark TX ring/flush behavior.
- Changing M2 command-entry behavior.

## BDD Gap Scan

2026-07-11: user requested a plan-only OpenSpec change and will implement later. Interactive BDD questioning is not used in this turn.

### Happy Path

- M3 image boots on D1 and prints `lichee-d1-rootfs-probe`.
- M3 prints the SDMMC known facts and partition facts.
- M3 prints `block_status=SKIPPED: missing D1 SDMMC/block driver`.
- M3 prints `rootfs_init=NOT called`.
- M3 halts with `probe complete, halting. No panic.`

### Sad Path

- Startup benchmark still stalls or truncates when run in a dedicated benchmark mode; this is O81, not an M3 probe failure.
- M3 must not call `init_filesystems()` while no D1 block device is registered.
- M3 must not claim SDMMC MMIO or first block read success.

### Edge

- If later M3 needs startup benchmark output for diagnostics, it must run after the probe table or behind a separate feature flag.
- If a real D1 block device is added, rootfs mounting must move to a separate Q19D/O79 change.

## Impact

- `kernel/src/entry.rs`: M3 entry ordering or benchmark cfg.
- `docs/M3.md`: expected board log should include probe evidence.
- `.claude/docs/tasks.md`: M3 board gate remains pending until true D1 probe log exists.
- `openspec/specs/learned/spec.md`: L280 records the current failure boundary.
- `openspec/specs/optimization/spec.md`: O81 tracks the startup benchmark follow-up.
