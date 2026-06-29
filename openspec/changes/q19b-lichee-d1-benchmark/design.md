# Q19B Design: Lichee RV Dock Async UART Benchmark

## Context

Q19A uses `feature = "lichee-d1"` to enter:

```text
starry_kernel::entry::init
  -> platform::smoke::run_lichee_d1_smoke() -> !
```

This intentionally bypasses the normal QEMU benchmark path:

```text
entry::init
  -> uart_init::init_uart_hardware()
  -> drivers::bench::run_startup_benchmark()
  -> pseudofs::mount_all()
  -> load_user_app()
  -> add_stdio("/dev/console")
  -> user benchmark
```

Q19B must restore this path incrementally for D1, not by enabling the full QEMU feature set at once.

## Design Decisions

### D1: Explicit Lichee Runtime Modes

Use explicit mode names so each board image has a clear purpose:

| Mode | Purpose |
|------|---------|
| `lichee-smoke` | current Q19A smoke marker and halt |
| `lichee-kbench` | initialize async UART and run kernel benchmark |
| `lichee-userbench` | run user benchmark payload through `/dev/console` |

The concrete implementation may use Cargo features, app features, or a small build-time mode constant. The required behavior is that serial logs print the active mode and that Q19A smoke remains available.

### D2: D1-Safe Async UART Port

`kernel/src/drivers/uart_init.rs` currently still contains QEMU-shaped assumptions:

- raw LSR probe reads byte `base + 5`,
- `uart_16550::MmioBackend` performs U8 volatile access,
- QEMU uses NS16550 stride 1.

D1 uses DW APB UART with register stride 4 and 32-bit MMIO access. Q19B must therefore add one of:

1. a D1-specific `UartPort` implementation that performs 32-bit MMIO, or
2. a width-aware `uart_16550` backend that preserves `u8` register semantics while using `u32` volatile MMIO.

For first D1 data, a D1-specific `UartPort` is acceptable if it reduces risk. Long term, width-aware backend extraction is preferable because VisionFive2 has the same class of UART access-width requirement.

### D3: Real PLIC IRQ Gate Before TTY

The local D1 axplat already has real PLIC code behind feature `irq`. The current top-level `lichee-d1` feature only enables `irq-if`, which uses the no-op IRQ stub.

Q19B must add a gate that enables real D1 PLIC IRQ delivery and validates UART IRQ 18 before `/dev/console` is trusted.

Expected evidence:

- `axhal::irq::register_irq_hook(uart_isr_wrapper)` registers,
- PLIC external interrupt path claims source 18,
- the handler completes source 18,
- UART RX/TX wakers are triggered from interrupt path.

### D4: Kernel Benchmark Is an Intermediate Gate

`drivers::bench::run_startup_benchmark()` should run after async UART initialization and before user benchmark.

It proves:

- ring buffers are initialized,
- async driver exists,
- copier tasks can run,
- driver-level metrics can be printed.

It does not prove:

- user process execution,
- `/dev/console` open/write semantics,
- `tcdrain`,
- `FIONBIO`.

So it is not the final Q19B completion gate.

### D5: Embedded Benchmark ELF Before SDMMC/rootfs

The first user benchmark should not depend on SDMMC/rootfs. Q19B should compile `tests/benchmark.c` as a static RISC-V ELF and embed it in the kernel image or a minimal initramfs-like blob.

The user benchmark source should remain compatible with QEMU. Delivery differs; benchmark behavior should not.

This keeps the first benchmark dataset focused on async UART and user syscall/TTY behavior.

### D6: Data Reporting

D1 results must be appended as D1-specific data, not used to overwrite QEMU rows.

Recommended raw log path:

```text
.claude/analysis/lichee/q19b-YYYYMMDD-{mode}.txt
```

## Requirements Traceability Matrix

| Requirement | Task(s) | Coverage | Simplification | Status |
|-------------|---------|----------|----------------|--------|
| Preserve Q19A smoke regression | Q19B.1-Q19B.4, Q19B.28 | 100% | None | Covered |
| Add explicit Q19B modes | Q19B.1-Q19B.4 | 100% | implementation mechanism open | Covered |
| D1 async UART uses stride 4 / 32-bit MMIO | Q19B.5-Q19B.10 | 100% | may start D1-specific before general backend | Covered |
| Real PLIC UART IRQ 18 works | Q19B.11-Q19B.15 | 100% | no SMP claim | Covered |
| Kernel benchmark gate runs | Q19B.16-Q19B.18 | 100% | not final completion | Covered |
| `/dev/console` async TTY works | Q19B.19-Q19B.22 | 100% | minimal devfs before full rootfs | Covered |
| User benchmark runs | Q19B.23-Q19B.27 | 100% | embedded ELF before SDMMC/rootfs | Covered with explicit scope |
| Data collection and docs | Q19B.29-Q19B.32 | 100% | None | Covered |
| SDMMC/rootfs parity | Q19B.33 | 100% | deferred optional stage | Covered with explicit scope |

No requirement is missing. The only simplifications are explicit: a D1-specific UART port is allowed before a generalized backend, and embedded ELF is allowed before SDMMC/rootfs.

## Execution Hold

This design is ready for user audit. Do not implement until the user explicitly approves the plan.
