# Q19B Tasks: Lichee RV Dock Async UART Benchmark

## Phase 0: Pre-Implementation Witnesses

- [x] Q19B.1 Record current Q19A smoke artifact behavior and serial success string as the regression baseline.
- [x] Q19B.2 Record current `make lichee` boot image inspect output and size.
- [x] Q19B.3 Record current QEMU benchmark build/run baseline or cite the latest accepted Q15/QEMU benchmark document.
- [x] Q19B.4 Confirm no source implementation files are changed before witnesses are captured.

## Phase 1: Lichee Mode Split

- [x] Q19B.5 Add explicit Lichee runtime mode selection for smoke, kernel benchmark, and user benchmark.
- [x] Q19B.6 Keep Q19A smoke mode behavior byte-for-byte equivalent at the observable serial-marker level.
- [x] Q19B.7 Ensure the active Lichee mode is printed in serial logs.
- [x] Q19B.8 Ensure QEMU default build remains unchanged.

## Phase 2: D1-Safe Async UART Port

- [x] Q19B.9 Remove or gate D1-unsafe byte raw probe `base+5` from the D1 path in `uart_init`.
- [x] Q19B.10 Add D1/DW APB UART access path using stride 4 and 32-bit volatile MMIO.
- [x] Q19B.11 Implement `receive_bytes`, `send_bytes`, `transmitter_empty`, and `update_ier` for the D1 UART path.
- [x] Q19B.12 Preserve existing QEMU NS16550 U8 behavior.
- [x] Q19B.13 Gate: D1 image creates `AsyncUartDriver` and starts RX/TX copier tasks without fault.

## Phase 3: Real D1 PLIC / UART IRQ

- [x] Q19B.14 Enable `axplat-riscv64-lichee-d1/irq` for the IRQ benchmark mode instead of only `irq-if` stub.
- [x] Q19B.15 Expose UART IRQ 18 to the async UART init path.
- [x] Q19B.16 Add temporary or permanent IRQ witness logging/counters for PLIC source 18.
- [x] Q19B.17 Gate: PLIC claims and completes UART IRQ 18.
- [x] Q19B.18 Gate: UART ISR wakes TX/RX paths through the interrupt path.

## Phase 4: Kernel Benchmark Gate

- [x] Q19B.19 Run `drivers::bench::run_startup_benchmark()` in `lichee-kbench` mode after async UART init.
- [x] Q19B.20 Ensure kernel benchmark output is visible on serial.
- [x] Q19B.21 Record D1 kernel ring-buffer metrics separately from QEMU data.

## Phase 5: `/dev/console` TTY Gate

- [x] Q19B.22 Re-enable the minimal modules required for `pseudofs::mount_all()` and `/dev/console` on D1.
- [x] Q19B.23 Bind `ASYNC_TTY` to the process/stdout path needed by the benchmark mode.
- [x] Q19B.24 Verify `/dev/console` write reaches async UART.
- [x] Q19B.25 Verify `tcdrain` / transmitter-empty behavior is meaningful on D1.

## Phase 6: Embedded User Benchmark Payload

- [x] Q19B.26 Add a build path that compiles `tests/benchmark.c` as static RISC-V musl ELF.
- [x] Q19B.27 Embed the benchmark ELF or include it in a minimal initramfs-like blob for `lichee-userbench`.
- [x] Q19B.28 Reuse existing user ELF loader logic where practical.
- [x] Q19B.29 Gate: user process starts and prints `UART Async Benchmark`.
- [x] Q19B.30 Gate: benchmark prints TX throughput, TX latency, FIFO boundary matrix, and nonblocking read sections.

## Phase 7: Result Capture and Documentation

- [x] Q19B.31 Save raw board serial output under `.claude/analysis/lichee/q19b-YYYYMMDD-{mode}.txt`.
- [x] Q19B.32 Update `docs/benchmark-report-async.md` with a separate Lichee D1 result section.
- [x] Q19B.33 Update `.claude/docs/tasks.md`, `.claude/docs/SNAPSHOT.md`, `learned/spec.md`, and `optimization/spec.md` with the final Q19B result.

## Phase 8: Optional SDMMC/rootfs Parity

- [x] Q19B.34 Decide whether to start a later Lichee rootfs parity milestone after embedded benchmark succeeds.

## Execution Hold

- [x] Q19B.35 Planning document generated.
- [x] Q19B.36 OpenSpec proposal/design/tasks/spec prepared.
- [x] Q19B.37 User approves plan and requirements completeness.
- [x] Q19B.38 Implementation begins: Phases 0-4 complete; Phases 5-6 deferred (require axfs dep + embedded ELF); Phase 7 pending.

## Implementation Summary (2026-06-29, final)

**Q19B-Next.1-4 (completed)**:
- Feature vocabulary normalized: `lichee-d1-async-uart` extracted as shared hardware capability
- `cargo check` host gates: all 4 modes (smoke/kbench/userbench/qemu) pass
- `/dev/console` TTY path: `pseudofs::mount_all()` + `spawn_alarm_task()` work on D1 userbench
- Embedded benchmark ELF: `tests/benchmark.c` cross-compiled (38KB static RISC-V ELF), embedded via `include_bytes!`, loaded via new `load_embedded_user_app()` in `kernel/src/mm/loader.rs`
- User process: benchmark spawned as init process with `ASYNC_TTY` binding + `add_stdio`

**New files**:
- `kernel/src/drivers/d1_uart.rs` (162 lines) — D1 DW APB UART port + ISR
- `kernel/resources/benchmark.elf` (38KB) — static RISC-V ELF embedded in kernel

**Host gates**:
```sh
cargo check --target riscv64gc-unknown-none-elf --features lichee-d1           # ✅
cargo check --target riscv64gc-unknown-none-elf --features lichee-d1-kbench    # ✅
cargo check --target riscv64gc-unknown-none-elf --features lichee-d1-userbench # ✅
cargo check --target riscv64gc-unknown-none-elf --features qemu                # ✅
make lichee-kbench     # ✅ starry-lichee-kbench-boot.img, kernel_size=188608
make lichee-userbench  # ✅ starry-lichee-userbench-boot.img, kernel_size=876736
```

**Build blockers resolved before board test**:
- D1 axplat link: `src/main.rs` now extern-crates `axplat_riscv64_lichee_d1` for both `lichee-d1` and `lichee-d1-async-uart`.
- `axfs-ng -> axdriver`: local patch disables axdriver defaults and explicitly enables `block + bus-mmio`, avoiding `cfg(bus="pci")` and missing `PCI_*` constants.
- Embedded benchmark ELF: compiled as ET_EXEC (`-static -no-pie -fno-pie -s`) with no relocations.

**Q19B-Next.5 (completed on board)**:
- PLIC IRQ 18 reached the D1 UART path; no-pending IIR and THRE edge-loss were handled by state-driven wakeups.
- Kernel benchmark and user benchmark both ran from Android boot images on Lichee RV Dock.
- User benchmark printed TX throughput, TX latency, FIFO boundary matrix, and nonblocking read sections.
- Raw/latest board evidence is recorded in `.claude/analysis/lichee/kbench`, `.claude/analysis/lichee/userbench`, and summarized in `docs/licheerv-dock-bringup.md`.

**Final D1 userbench result**:
- `starry-lichee-userbench-boot.img` completed with `benchmark exited with code: 0` and `Done.`
- 256B / 1024B / 4096B TX throughput reached 11.25 / 11.40 / 11.41 KB/s, or 97.7% / 98.9% / 99.0% of 115200bps line rate.
- 1B `tcdrain` latency reached avg 0.270 ms, P50 0.185 ms, P95 0.187 ms, P99 8.547 ms.
- FIONBIO nonblocking checks passed through both `open(O_NONBLOCK)` and `ioctl(FIONBIO)`.
- Later SDMMC/rootfs parity is explicitly out of Q19B scope and should be started as a separate milestone if needed.
