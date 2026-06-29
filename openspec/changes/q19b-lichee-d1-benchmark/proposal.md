# Q19B Proposal: Lichee RV Dock Async UART Benchmark

## Why

Q19A proved that StarryOS can boot on Lichee RV Dock / Allwinner D1 through the official Android boot image path and can print the smoke marker:

```text
[starry-d1] smoke complete, halting.
```

That result is necessary but not enough for the project goal. The next useful Lichee milestone is to collect async UART performance data from real hardware in the same style as the current QEMU benchmark flow.

The existing QEMU benchmark path depends on more than early serial output:

- async UART driver initialization,
- UART interrupt delivery,
- `/dev/console` through `ASYNC_TTY`,
- `tcdrain` and nonblocking ioctl behavior,
- user process execution,
- benchmark result output through serial.

Q19B defines a staged path to reach that goal without letting SDMMC/rootfs bring-up block the first D1 async UART dataset.

## What

Q19B will plan and later implement a Lichee benchmark mode that:

1. keeps Q19A smoke mode available as a regression target,
2. adds explicit Lichee benchmark modes instead of overloading `lichee-d1`,
3. makes async UART initialization safe for D1 DW APB UART 32-bit MMIO,
4. enables real D1 PLIC UART IRQ 18 for async wakeups,
5. runs the existing kernel ring benchmark as an intermediate gate,
6. brings up `/dev/console` through the async TTY stack,
7. runs `tests/benchmark.c` or equivalent through an embedded user ELF payload before requiring SDMMC/rootfs,
8. records D1 benchmark data separately from QEMU data.

## Non-Goals

- Do not implement code during the planning phase.
- Do not require SDMMC/rootfs for the first Q19B benchmark dataset.
- Do not claim Q19B complete from kernel ring benchmark alone.
- Do not merge QEMU PCI/virtio assumptions into the D1 path.
- Do not use Lichee D1 single-core results as Q17 SMP evidence.

## BDD Scenario Sketch

### Happy Path

- Lichee builds in `smoke`, `kernel benchmark`, or `user benchmark` mode.
- The board boots through U-Boot and prints the selected mode.
- D1 async UART initializes using stride 4 / 32-bit MMIO.
- PLIC IRQ 18 reaches the UART ISR path.
- Kernel benchmark prints ring buffer metrics.
- User benchmark prints the same sections as QEMU: TX throughput, TX latency, FIFO boundary matrix, nonblocking read.

### Sad Path

- If D1 UART byte-MMIO code is accidentally used, the mode must fail before user benchmark and print enough context to localize the MMIO access-width problem.
- If PLIC IRQ 18 does not fire, the system must stop at the IRQ gate rather than hiding the issue behind TTY or user process failures.
- If user ELF loading fails, kernel benchmark and `/dev/console` gates must still remain independently diagnosable.

### Edge Cases

- Q19A smoke mode must remain available and not regress.
- Android boot image must remain below the boot partition size.
- QEMU benchmark behavior must remain unchanged.
- D1 benchmark results must be reported separately because QEMU does not model physical UART line delay.

## References

- `.claude/analysis/q19b-lichee-benchmark-plan.md`
- `docs/licheerv-dock-bringup.md`
- `tests/benchmark.c`
- `kernel/src/entry.rs`
- `kernel/src/drivers/uart_init.rs`
- `crates/axplat-riscv64-lichee-d1/src/irq.rs`
