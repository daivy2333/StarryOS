# Q19a: Lichee RV Dock D1 AxPlat Bring-up Skeleton

## Why

The latest board test changed the diagnosis. The generated Android boot image is now accepted by U-Boot and the boot log reaches `Starting kernel ...`, but no StarryOS output appears.

Host artifact inspection explains the failure:

- The boot image loads the payload at the D1 physical address `0x40200000`.
- The ELF is still linked at the QEMU high-half entry `0xffffffc080200000`.
- The first boot code still calls `axplat_riscv64_qemu_virt::boot`.

Therefore Q19 cannot be solved by adding more StarryOS `entry.rs` smoke logic. The failure happens before that layer. Q19a must replace the QEMU axplat boot layer with a D1-specific axplat path, then prove the build artifact no longer contains the QEMU boot path before another board flash.

## What Changes

- Add a local D1 axplat package, tentatively `crates/axplat-riscv64-lichee-d1`.
- Make the Lichee build select this D1 axplat through `MYPLAT` / `PLAT_CONFIG`, not `axfeat/defplat`.
- Move first-byte serial responsibility into the D1 axplat polling console because `axruntime` may print before StarryOS `entry::init`.
- Align the D1 link/load contract:
  - physical RAM base `0x40000000`
  - physical kernel load address `0x40200000`
  - expected high-half link base `0xffffffc040200000`
  - Android boot image `kernel_addr = 0x40200000`
- Keep the QEMU default build and benchmark path unchanged.
- Keep board flashing manual and gated by host artifact inspection.

## Non-Goals

- No rootfs, USB, SDMMC, shell, user program, async TTY, benchmark, or PLIC IRQ bring-up.
- No VisionFive2 changes.
- No D1 SMP work.
- No automatic write to `/dev/mmcblk*`, `/dev/sd*`, or `/dev/by-name/boot`.
- No broad platform refactor beyond the D1 axplat selection and the build path needed to exercise it.

## BDD Scenario Sketch

Default assumptions used for planning:

- The user wants a real StarryOS boot path, not a standalone naked payload.
- Q19a success is host-artifact correctness plus first serial-byte readiness, not full OS userspace.
- Board flashing remains a manual step after review.

### Scenario: D1 build no longer uses QEMU axplat

Given the Lichee D1 build path is selected
When StarryOS is built
Then the generated artifact must reference `axplat_riscv64_lichee_d1::boot`
And it must not reference `axplat_riscv64_qemu_virt::boot`.

### Scenario: D1 link/load addresses are internally consistent

Given the D1 build has completed
When the ELF, linker script, and Android boot image are inspected
Then the ELF/linker base must be `0xffffffc040200000`
And the Android boot image `kernel_addr` must be `0x40200000`
And the raw image must remain below the boot partition limit.

### Scenario: First serial output is owned by axplat

Given the D1 payload starts before StarryOS `entry.rs`
When early console output is emitted
Then the D1 axplat polling console must be capable of writing UART0 through 32-bit MMIO
And this output must not depend on async tasks, rootfs, interrupts, PLIC, or the StarryOS async UART stack.

### Scenario: QEMU remains stable

Given no D1 platform selection is requested
When the normal RISC-V QEMU build is used
Then QEMU must still use the existing QEMU axplat and descriptor path
And existing QEMU build behavior must not change.

## Impact

- New local axplat crate: `crates/axplat-riscv64-lichee-d1/`.
- Top-level feature/dependency wiring: `Cargo.toml`.
- Build alias/config: `Makefile` and possibly existing make platform variables.
- Top-level binary linkage if an explicit `extern crate axplat_riscv64_lichee_d1` is needed.
- StarryOS descriptor alignment if `LICHEE_D1.kernel.link_vaddr` must match the axplat high-half scheme.
- Android boot image packaging remains in `tools/android_boot_image.py`.

## Execution Hold

This change is currently in planning only. Implementation must not start until proposal, design, tasks, and spec delta are reviewed and approved.
