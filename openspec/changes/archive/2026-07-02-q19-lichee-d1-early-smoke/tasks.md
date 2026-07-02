# Q19a Tasks: Lichee RV Dock D1 AxPlat Bring-up Skeleton

## Phase 0: Pre-Implementation Witnesses

- [x] Q19.1 Record current QEMU build witness before implementation.
- [x] Q19.2 Record current D1/Lichee artifact witness showing why the existing image is still QEMU axplat, if current artifacts are present.
- [x] Q19.3 Confirm no implementation files are changed before witnesses are captured.

## Phase 1: D1 AxPlat Package Skeleton

- [x] Q19.4 Add local package `crates/axplat-riscv64-lichee-d1`.
- [x] Q19.5 Add D1 `axconfig.toml` with RAM `0x40000000+0x20000000`, kernel paddr `0x40200000`, kernel vaddr `0xffffffc040200000`, UART0 `0x02500000`, PLIC `0x10000000`, CPU count `1`.
- [x] Q19.6 Add minimal D1 axplat modules: `boot.rs`, `console.rs`, `init.rs`, `irq.rs`, `mem.rs`, `power.rs`, `time.rs`, `lib.rs`, `build.rs`.
- [x] Q19.7 Ensure D1 boot code does not apply VisionFive2 hart-id adjustment and does not call QEMU boot symbols.

## Phase 2: Cargo and Build Selection

- [x] Q19.8 Add optional top-level dependency on `axplat-riscv64-lichee-d1`.
- [x] Q19.9 Make top-level `lichee-d1` feature depend on the D1 axplat package and `starry-kernel/lichee-d1`.
- [x] Q19.10 Add top-level binary linkage for the D1 axplat crate if required by the existing axplat pattern.
- [x] Q19.11 Update the `lichee` make alias to use `MYPLAT=axplat-riscv64-lichee-d1`, explicit `PLAT_CONFIG=crates/axplat-riscv64-lichee-d1/axconfig.toml`, `MEM=512M`, and `DWARF=n`.
- [x] Q19.12 Remove the current Lichee alias behavior that renames `StarryOS_riscv64-qemu-virt.*` artifacts as D1 artifacts.

## Phase 3: D1 Platform Contract Alignment

- [x] Q19.13 Align `kernel/src/platform/lichee_d1.rs` `link_vaddr` with the D1 axplat high-half base.
- [x] Q19.14 Keep QEMU descriptor and QEMU default build path unchanged.
- [x] Q19.15 Keep `qemu + lichee-d1` incompatible at compile time.

## Phase 4: AxPlat Polling Console

- [x] Q19.16 Implement D1 axplat polling console using 32-bit volatile MMIO.
- [x] Q19.17 Poll LSR register index `5` with stride `4` and THRE bit `1 << 5`.
- [x] Q19.18 Write TX register index `0` with the byte in the low 8 bits of a `u32`.
- [x] Q19.19 Ensure first-byte console does not depend on IRQ, PLIC, async tasks, rootfs, USB, SDMMC, or async UART.

## Phase 5: Android Boot Image Packaging

- [x] Q19.20 Keep Android boot image output using `page_size=2048`, `kernel_addr=0x40200000`, and name `d1-nezha`.
- [x] Q19.21 Ensure the generated raw payload and boot image stay below the current boot partition capacity. Final smoke image `kernel_size=118976` bytes.
- [x] Q19.22 Keep build tooling from automatically writing board storage.

## Phase 6: Verification Gates

- [x] Q19.23 Run QEMU default build and record output.
- [x] Q19.24 Run D1 build and record output.
- [x] Q19.25 Inspect ELF entry and generated linker script; both must show `0xffffffc040200000`.
- [x] Q19.26 Inspect disassembly/symbols; D1 artifact must reference `axplat_riscv64_lichee_d1::boot` and must not reference `axplat_riscv64_qemu_virt::boot`.
- [x] Q19.27 Inspect Android boot image; it must show `kernel_addr=0x40200000`, `page_size=2048`, and size below the boot partition limit.
- [x] Q19.28 Run boot image tool tests.
- [x] Q19.29 Run `openspec validate --changes q19-lichee-d1-early-smoke`.
- [x] Q19.36 Fix linker undefined `IrqIf` symbols with `irq-if` no-op interface while keeping full PLIC bring-up out of Q19a.
- [x] Q19.37 Localize board `Store/AMO access fault` to `percpu::imp::init` AMO on `.bss` and patch early DDR PTE with T-Head C9xx `SH|B|C`.
- [x] Q19.38 Rebuild and flash the post-PTE-fix boot image; final page table `xuantie-c9xx` attributes were fixed and board smoke completed.

## Phase 7: Manual Board Gate

- [x] Q19.30 Stop after host verification and provide manual backup/flash/restore commands.
- [x] Q19.31 User flashes manually and captures serial output.
- [x] Q19.32 Q19a board success: serial shows D1 axplat early output and `[starry-d1] early boot`.
- [x] Q19.33 If no output appears, constrain diagnosis to U-Boot jump, pre-MMU UART, post-MMU mapping, D1 link/load address, and UART 32-bit MMIO access. Completed as diagnostic path; final board output reached smoke success.

## Execution Hold

- [x] Q19.34 Planning updated for D1 axplat正路径.
- [x] Q19.35 Implementation complete. D1 axplat crate created, build system wired, normal-environment D1 build and boot image size gate verified by 2026-06-29 board smoke.

## Implementation Summary (2026-06-29, final)

- Lichee RV Dock booted the StarryOS D1 Android boot image through the official U-Boot path.
- Serial output reached `platform = riscv64-lichee-d1`, `sbi_version: 0.2`, `[starry-d1] early boot`, and `[starry-d1] smoke complete, halting.`
- Final fixes included D1/C906 early DDR PTE `SH|B|C`, final page table `page_table_entry/xuantie-c9xx`, empty D1 virtio MMIO ranges, and Lichee smoke feature gating.
- Follow-up async UART benchmark work continued in `q19b-lichee-d1-benchmark`.
