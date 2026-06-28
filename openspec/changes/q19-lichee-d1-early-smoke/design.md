# Q19a Design: D1 AxPlat Bring-up Skeleton

## Context

Q18 introduced StarryOS-level platform descriptors and early console abstractions. The first Q19 implementation proved useful but incomplete: board flashing now reaches U-Boot `Starting kernel ...`, yet no StarryOS output appears.

The current artifact is still below the wrong layer:

- `Makefile` `lichee` target builds with `APP_FEATURES=lichee-d1` but still copies `StarryOS_riscv64-qemu-virt.*`.
- Top-level `lichee-d1` feature only enables `starry-kernel/lichee-d1`.
- The generated ELF still uses QEMU high-half entry and QEMU axplat boot symbols.

The design correction is to add the missing D1 axplat layer. StarryOS `entry.rs` smoke remains useful only after axplat `_start`, early page tables, MMU transition, and platform console are correct.

Reference analysis: `.claude/analysis/d1-axplat-bringup-plan.md`.

## Goals / Non-Goals

**Goals:**

- Add a local `axplat-riscv64-lichee-d1` package with D1 memory, boot, console, and minimal platform service definitions.
- Build Lichee with `MYPLAT` / `PLAT_CONFIG` so generated artifacts are D1 artifacts, not renamed QEMU artifacts.
- Implement or scaffold a D1 axplat polling console with 32-bit MMIO UART0 access.
- Produce an Android boot image whose header still matches the known D1 boot contract.
- Establish host-side verification gates before the next manual board flash.

**Non-Goals:**

- No D1 rootfs, shell, USB, SDMMC, async UART, PLIC IRQ, benchmark, or SMP enablement.
- No automatic flashing.
- No change to QEMU default feature behavior beyond regression-safe wiring.
- No attempt to make VisionFive2 share this crate.

## Decisions

### D1: Add a Local D1 AxPlat Crate

**Choice:** create `crates/axplat-riscv64-lichee-d1` and wire it as an optional top-level dependency.

**Reasoning:**

- `_start`, boot page tables, MMU enable, early platform console, and memory regions are axplat responsibilities.
- QEMU axplat cannot be made correct for D1 by only overriding `KERNEL_BASE_PADDR`.
- VisionFive2 axplat is a useful template for RAM/load address shape, but its hart-id adjustment and device facts are board-specific.

**Expected structure:**

```text
crates/axplat-riscv64-lichee-d1/
  Cargo.toml
  axconfig.toml
  build.rs
  src/
    lib.rs
    boot.rs
    console.rs
    init.rs
    irq.rs
    mem.rs
    power.rs
    time.rs
```

### D2: Use D1 AxConfig as the Build Contract

The D1 axconfig must express these facts:

| Field | Value |
|-------|-------|
| platform | `lichee-d1` or `lichee-rv-dock` |
| package | `axplat-riscv64-lichee-d1` |
| max CPU | `1` |
| RAM base | `0x40000000` |
| RAM size | `0x20000000` |
| kernel physical base | `0x40200000` |
| kernel high-half base | `0xffffffc040200000` |
| phys/virt offset | `0xffffffc000000000` |
| UART0 base | `0x02500000` |
| UART0 IRQ | `18` |
| PLIC base | `0x10000000` |

The exact TOML keys must follow the upstream axplat examples. The values above are the board contract.

### D3: D1 Boot Code Is Based on D1 Facts, Not QEMU

`boot.rs` should follow the same shape as upstream RISC-V axplat crates, but with D1-specific facts:

- Physical entry expectation: `0x40200000`.
- RAM mapping: `0x40000000..0x80000000`.
- High-half mapping: `0xffffffc040000000` family, with kernel entry at `0xffffffc040200000`.
- Hart id remains `0`; do not apply VisionFive2's `hartid - 1` adjustment.
- Preserve the bootloader-provided FDT pointer (`a1`) for future use.

### D4: D1 Polling Console Lives in AxPlat

The first reliable byte must be available from axplat because `axruntime` can print before StarryOS `entry::init`.

D1 UART0 polling requirements:

- base `0x02500000`
- register stride `4`
- access width `u32`
- THR write at register `0`
- LSR read at register `5`
- THRE bit `1 << 5`
- newline conversion can be implemented in the console wrapper

This console must not depend on PLIC, IRQs, async tasks, rootfs, or the `uart_16550` async backend.

### D5: Build Selection Must Stop Renaming QEMU Artifacts

The Lichee build must stop copying `StarryOS_riscv64-qemu-virt.*` to D1 names. The build should instead produce a D1-named artifact through `MYPLAT` / `PLAT_CONFIG`.

Planned command shape:

```bash
make ARCH=riscv64 \
  MYPLAT=axplat-riscv64-lichee-d1 \
  PLAT_CONFIG=crates/axplat-riscv64-lichee-d1/axconfig.toml \
  APP_FEATURES=lichee-d1 \
  MEM=512M \
  DWARF=n \
  build
```

The `lichee` make alias may wrap this command and then pack `starry-lichee-boot.img`.

### D6: Host Artifact Inspection Is a Required Gate

Before board flashing, implementation must prove:

| Check | Expected |
|-------|----------|
| ELF entry | `0xffffffc040200000` |
| generated linker base | `0xffffffc040200000` |
| objdump boot symbols | `axplat_riscv64_lichee_d1::boot` |
| absent boot symbols | `axplat_riscv64_qemu_virt::boot` |
| boot image `kernel_addr` | `0x40200000` |
| boot image size | below current boot partition capacity |

If any row fails, board flashing is blocked.

## Requirements Traceability Matrix

| Requirement | Task(s) | Coverage | Simplification | Status |
|-------------|---------|----------|----------------|--------|
| D1 build uses D1 axplat, not QEMU axplat | Q19.1-Q19.9, Q19.18-Q19.21 | 100% | None | Covered |
| D1 link/load contract matches board facts | Q19.4-Q19.7, Q19.19-Q19.21 | 100% | None | Covered |
| D1 first-byte console is available before StarryOS entry | Q19.8-Q19.11, Q19.22 | 100% | Stops after smoke; no userspace | Covered, intentional |
| QEMU default remains unchanged | Q19.12-Q19.15, Q19.23 | 100% | None | Covered |
| Android boot image remains compatible with D1 U-Boot | Q19.16-Q19.17, Q19.20 | 100% | Uses existing local tool | Covered |
| Flashing remains manual and recoverable | Q19.24-Q19.26 | 100% | Automatic flashing forbidden | Covered |
| Q19a excludes rootfs/USB/SDMMC/async benchmark | Q19.27 | 100% | Later milestones | Covered |

No requirement is intentionally simplified without explicit scope marking.

## Execution-Ready Plan

This plan is for the next phase after review approval. Do not execute during planning.

1. Establish current witnesses.
   - Capture current QEMU build status.
   - Capture current Lichee artifact inspection showing QEMU axplat references, if build artifacts are present.
2. Add local D1 axplat crate skeleton.
   - Start from upstream QEMU/VisionFive2 structure as a template.
   - Fill D1 axconfig and minimal modules.
3. Wire Cargo features and top-level binary linkage.
   - Add optional D1 axplat dependency.
   - Make `lichee-d1` pull the D1 axplat dependency and `starry-kernel/lichee-d1`.
4. Wire make build selection.
   - Make `make lichee` use `MYPLAT` / `PLAT_CONFIG` and `DWARF=n`.
   - Stop copying QEMU-named artifacts.
5. Align StarryOS descriptor values.
   - Ensure `LICHEE_D1.kernel.link_vaddr` matches the D1 axplat high-half address.
6. Add D1 axplat polling console.
   - Use 32-bit MMIO polling.
   - Add an ultra-early marker only if needed for failure localization.
7. Run host verification.
   - Build QEMU default.
   - Build D1.
   - Inspect readelf/linker/objdump/boot image fields.
   - Run boot image tool tests.
   - Run OpenSpec validation.
8. Stop for manual board gate.
   - User flashes only after host gates pass.
   - Board success criterion is serial output from D1 axplat or `[starry-d1] early boot`.

## Risks

- The local axplat crate may need exact upstream trait/module exports; compile errors should be handled within the crate boundary, not by weakening StarryOS layering.
- D1 UART may require a subtle 32-bit access behavior; byte-oriented console helpers should not be reused blindly.
- If no serial output appears, an ultra-early pre-MMU UART write may be needed to split pre-MMU from post-MMU failures.
- `cargo axplat info` may not discover a local path package by name in every case; `PLAT_CONFIG=.../axconfig.toml` should be kept as an explicit fallback.
