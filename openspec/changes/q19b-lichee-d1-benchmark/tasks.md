# Q19B Tasks: Lichee RV Dock Async UART Benchmark

## Phase 0: Pre-Implementation Witnesses

- [ ] Q19B.1 Record current Q19A smoke artifact behavior and serial success string as the regression baseline.
- [ ] Q19B.2 Record current `make lichee` boot image inspect output and size.
- [ ] Q19B.3 Record current QEMU benchmark build/run baseline or cite the latest accepted Q15/QEMU benchmark document.
- [ ] Q19B.4 Confirm no source implementation files are changed before witnesses are captured.

## Phase 1: Lichee Mode Split

- [ ] Q19B.5 Add explicit Lichee runtime mode selection for smoke, kernel benchmark, and user benchmark.
- [ ] Q19B.6 Keep Q19A smoke mode behavior byte-for-byte equivalent at the observable serial-marker level.
- [ ] Q19B.7 Ensure the active Lichee mode is printed in serial logs.
- [ ] Q19B.8 Ensure QEMU default build remains unchanged.

## Phase 2: D1-Safe Async UART Port

- [ ] Q19B.9 Remove or gate D1-unsafe byte raw probe `base+5` from the D1 path in `uart_init`.
- [ ] Q19B.10 Add D1/DW APB UART access path using stride 4 and 32-bit volatile MMIO.
- [ ] Q19B.11 Implement `receive_bytes`, `send_bytes`, `transmitter_empty`, and `update_ier` for the D1 UART path.
- [ ] Q19B.12 Preserve existing QEMU NS16550 U8 behavior.
- [ ] Q19B.13 Gate: D1 image creates `AsyncUartDriver` and starts RX/TX copier tasks without fault.

## Phase 3: Real D1 PLIC / UART IRQ

- [ ] Q19B.14 Enable `axplat-riscv64-lichee-d1/irq` for the IRQ benchmark mode instead of only `irq-if` stub.
- [ ] Q19B.15 Expose UART IRQ 18 to the async UART init path.
- [ ] Q19B.16 Add temporary or permanent IRQ witness logging/counters for PLIC source 18.
- [ ] Q19B.17 Gate: PLIC claims and completes UART IRQ 18.
- [ ] Q19B.18 Gate: UART ISR wakes TX/RX paths through the interrupt path.

## Phase 4: Kernel Benchmark Gate

- [ ] Q19B.19 Run `drivers::bench::run_startup_benchmark()` in `lichee-kbench` mode after async UART init.
- [ ] Q19B.20 Ensure kernel benchmark output is visible on serial.
- [ ] Q19B.21 Record D1 kernel ring-buffer metrics separately from QEMU data.

## Phase 5: `/dev/console` TTY Gate

- [ ] Q19B.22 Re-enable the minimal modules required for `pseudofs::mount_all()` and `/dev/console` on D1.
- [ ] Q19B.23 Bind `ASYNC_TTY` to the process/stdout path needed by the benchmark mode.
- [ ] Q19B.24 Verify `/dev/console` write reaches async UART.
- [ ] Q19B.25 Verify `tcdrain` / transmitter-empty behavior is meaningful on D1.

## Phase 6: Embedded User Benchmark Payload

- [ ] Q19B.26 Add a build path that compiles `tests/benchmark.c` as static RISC-V musl ELF.
- [ ] Q19B.27 Embed the benchmark ELF or include it in a minimal initramfs-like blob for `lichee-userbench`.
- [ ] Q19B.28 Reuse existing user ELF loader logic where practical.
- [ ] Q19B.29 Gate: user process starts and prints `UART Async Benchmark`.
- [ ] Q19B.30 Gate: benchmark prints TX throughput, TX latency, FIFO boundary matrix, and nonblocking read sections.

## Phase 7: Result Capture and Documentation

- [ ] Q19B.31 Save raw board serial output under `.claude/analysis/lichee/q19b-YYYYMMDD-{mode}.txt`.
- [ ] Q19B.32 Update `docs/benchmark-report-async.md` with a separate Lichee D1 result section.
- [ ] Q19B.33 Update `.claude/docs/tasks.md`, `.claude/docs/SNAPSHOT.md`, `learned/spec.md`, and `optimization/spec.md` with the final Q19B result.

## Phase 8: Optional SDMMC/rootfs Parity

- [ ] Q19B.34 Decide whether to start a later Lichee rootfs parity milestone after embedded benchmark succeeds.

## Execution Hold

- [x] Q19B.35 Planning document generated.
- [x] Q19B.36 OpenSpec proposal/design/tasks/spec prepared.
- [ ] Q19B.37 User approves plan and requirements completeness.
- [ ] Q19B.38 Implementation begins only after approval.
