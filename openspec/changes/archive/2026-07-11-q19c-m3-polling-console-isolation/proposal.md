## Why

`docs/M3.md` shows that M3 now reaches the rootfs-probe entry and async UART init, then stops after the first probe facts line. The next code path is another static `ax_println!`, so the failure is no longer the startup benchmark.

Current M3 still enables `lichee-d1-async-uart` through `lichee-d1-rootfs-probe`. It initializes async UART before printing the full probe table. The platform console also writes UART0 with polling MMIO. This mixes two UART owners in the same proof path.

M3 should first prove the rootfs-probe control path with one UART owner. Async UART can be reintroduced after that path is visible on board output.

## What Changes

- Make `lichee-d1-rootfs-probe` a polling-console probe mode.
- Remove the implicit `lichee-d1-async-uart` dependency from M3.
- Add a clear build guard for `lichee-d1-rootfs-probe + lichee-d1-async-uart`.
- Add a M3 entry path that prints the complete probe table before halting.
- Keep M2, M1, userbench, and kbench async UART paths unchanged.
- Update M3 board expectations to require the full probe table and no rootfs panic.

## Capabilities

### Modified Capabilities

- `lichee-d1-fullbench`: refine M3 `lichee-d1-rootfs-probe` so the first board proof uses polling console only.

### New Capabilities

None.

## Scope

### In Scope

- `kernel/Cargo.toml` feature dependency for `lichee-d1-rootfs-probe`.
- `kernel/src/lib.rs` feature guard for M3 plus async UART.
- `kernel/src/entry.rs` D1 M3 entry routing.
- M3 board log criteria.
- Host cargo check and image build for M3.
- M2/fullbench regression checks.

### Out of Scope

- Implementing D1 SDMMC/block driver.
- Calling `axfs-ng::init_filesystems()` on D1 without a block device.
- Proving rootfs benchmark success.
- Fixing async UART TX copier or console coexistence.
- Reintroducing async UART into M3.

## BDD Gap Scan

2026-07-11: user selected default BDD assumptions.

### Happy Path

- M3 image builds as `lichee-d1-rootfs-probe` without async UART.
- D1 board log prints `lichee-rootfs-probe`.
- D1 board log prints all static SDMMC/rootfs facts.
- D1 board log prints `block_status=SKIPPED: missing D1 SDMMC/block driver`.
- D1 board log prints `rootfs_init=NOT called`.
- D1 board log prints `probe complete, halting. No panic.`

### Sad Path

- M3 must not initialize async UART before or during the polling-console probe.
- M3 must not call `init_filesystems()` while no D1 block device is registered.
- M3 must not claim SDMMC MMIO/register read success.
- M3 must not claim rootfs benchmark success.

### Edge

- `lichee-d1-rootfs-probe + lichee-d1-async-uart` must fail with an explicit mode-selection error.
- M2 and other async UART benchmark modes must still compile and keep their current behavior.
- A later async UART M3 path must be planned as a separate follow-up after polling-console evidence exists.

## Scenario Sketch

- Given the D1 platform console is available through polling MMIO, when M3 starts, then it prints the whole probe table without async UART init.
- Given no D1 block provider is registered, when M3 reaches rootfs readiness checks, then it reports SKIPPED and does not call rootfs init.
- Given async UART is selected with M3, when cargo checks the kernel, then the build fails before runtime with a clear feature error.
- Given M2 command-entry uses async UART, when the M3 change is applied, then M2 cargo/image gates still pass.

## Impact

- `kernel/Cargo.toml`: M3 feature dependency changes.
- `kernel/src/lib.rs`: feature guard changes.
- `kernel/src/entry.rs`: M3 entry isolation changes.
- `docs/M3.md`: expected board output changes after implementation.
- `.claude/docs/tasks.md`: Q19C.11c should point to this follow-up until board evidence passes.
