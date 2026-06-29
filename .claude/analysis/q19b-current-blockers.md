# Q19B Current Blockers: Lichee D1 Async UART Benchmark

> Date: 2026-06-29
> Scope: inspect current user work after Q19B Phases 0-4 implementation, identify remaining blockers before full Lichee user benchmark.
> Related: `.claude/analysis/q19b-lichee-benchmark-plan.md`, `openspec/changes/q19b-lichee-d1-benchmark/`

## Executive Summary

The current branch has crossed the Q19A smoke boundary and has meaningful Q19B progress:

- `lichee-d1` remains the smoke/regression mode.
- `lichee-d1-kbench` builds and type-checks.
- D1 UART async access is no longer using QEMU byte MMIO. The new `kernel/src/drivers/d1_uart.rs` implements a D1/DW APB UART `UartPort` using stride-aware 32-bit volatile MMIO.
- D1 kbench enables the real `axplat-riscv64-lichee-d1/irq` feature instead of the Q19A `irq-if` stub.
- `entry.rs` has a D1 kbench path that initializes async UART and calls `drivers::bench::run_startup_benchmark()`.

The remaining blocker is not early boot and not the basic D1 UART access model. The blocker is the transition from kernel benchmark to user benchmark: `/dev/console`, user task creation, user address space, and benchmark ELF loading are still coupled to QEMU-oriented filesystem/process features.

## Evidence

### Passing Host Gates

These checks pass on the current tree:

```sh
cargo check --target riscv64gc-unknown-none-elf --features lichee-d1
cargo check --target riscv64gc-unknown-none-elf --features lichee-d1-kbench
```

This supports the current split:

- smoke mode can still compile,
- kbench mode can compile with D1 async UART and real IRQ feature wiring.

### Failing Host Gate

This check fails on the current tree:

```sh
cargo check --target riscv64gc-unknown-none-elf --features lichee-d1-userbench
```

Observed failure:

```text
unresolved imports `crate::drivers::ASYNC_TTY`, `crate::file`, `crate::mm`,
`crate::pseudofs`, `crate::task`, `axfs`, `axtask::AxTaskExt`
```

The compiler notes are important: `ASYNC_TTY`, `file`, `mm`, `pseudofs`, and `task` are configured out by `#[cfg(not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench")))]`.

Because `kernel/Cargo.toml` currently defines:

```toml
lichee-d1-userbench = ["lichee-d1-kbench"]
```

`lichee-d1-userbench` inherits `lichee-d1-kbench`, and therefore also inherits the module exclusions intended for the lightweight kernel benchmark mode.

## What Is Already Working Architecturally

### Mode Split

The root `Cargo.toml` now exposes:

- `lichee-d1`: smoke image with `irq-if` stub.
- `lichee-d1-kbench`: kernel benchmark image with real D1 PLIC IRQ.
- `lichee-d1-userbench`: intended user benchmark image.

The Makefile now has:

- `make lichee`
- `make lichee-kbench`
- `make lichee-userbench`

This is the right high-level shape because each image has a clear gate and an independently named boot image output.

### D1 UART Access

The new D1 UART port uses the correct hardware model:

- D1 UART0 is DW APB UART.
- Registers use stride 4.
- Access width is 32-bit volatile MMIO.
- `RBR/THR = 0`, `IER = 1`, `IIR = 2`, `LSR = 5`.
- LSR bits still follow 16550 meanings: DR bit 0, THRE bit 5, TEMT bit 6.

This avoids the previous D1-dangerous assumptions:

- raw `base + 5` byte read,
- `uart_16550::MmioBackend` U8 access,
- QEMU NS16550 stride 1 semantics.

### ISR Path

`d1_uart_isr_handler()` reads IIR using D1-safe 32-bit MMIO and dispatches to the same async UART wakers used by the existing driver model:

- `RX_WAKER`
- `TX_WAKER`
- `DRAIN_WAKER`

This is the correct minimal bridge: hardware-specific interrupt-source decoding stays in the D1 port, while the async driver and waker model remain shared.

## Current Blockers

### Blocker 1: userbench Feature Inheritance Is Too Coarse

`lichee-d1-userbench` currently inherits `lichee-d1-kbench`. That makes sense for "userbench includes kbench initialization", but it is too coarse for `cfg` gating because kbench deliberately excludes the modules userbench needs:

- `drivers::ASYNC_TTY`
- `file`
- `mm`
- `pseudofs`
- `syscall`
- `task`
- `time`

This is why `lichee-d1-userbench` fails before reaching the actual embedded benchmark work.

Recommended direction:

- Split the current `lichee-d1-kbench` meaning into two concepts:
  - a reusable D1 async UART capability,
  - a kbench-only lightweight runtime mode.
- Avoid using `feature = "lichee-d1-kbench"` as a proxy for "exclude user/process/filesystem modules".
- Use a narrower negative gate such as `lichee-d1-smoke` and a new kbench-only feature, or introduce positive features like `lichee-d1-async-uart`, `lichee-d1-minimal-runtime`, and `lichee-d1-user-runtime`.

### Blocker 2: `/dev/console` Depends On More Than UART

The final benchmark path needs the same observable behavior as QEMU:

```text
benchmark.c -> write/read/ioctl/tcdrain -> /dev/console -> ASYNC_TTY -> AsyncUartWriter/Reader -> D1 UART
```

That path needs:

- TTY registration,
- file descriptor table,
- minimal devfs or pseudofs mount,
- syscall write/read/ioctl plumbing,
- user task and user address space setup,
- clock/time support for benchmark timing.

The D1 kbench path intentionally excludes most of these. That is correct for keeping kbench small, but it means userbench cannot be unlocked by only embedding `tests/benchmark.c`.

Recommended direction:

- Define a "minimal userbench runtime" that includes only the modules required for benchmark execution and `/dev/console`.
- Do not immediately import the full QEMU feature set.
- Treat `/dev/console` creation as a separate gate before loading the benchmark ELF.

### Blocker 3: `axfs` Is Optional But `pseudofs::mount_all()` Assumes It

`kernel/Cargo.toml` currently enables `dep:axfs` only through the `qemu` feature. D1 userbench imports `axfs::FS_CONTEXT`, but the root `lichee-d1-userbench` feature does not enable `starry-kernel/qemu` or `dep:axfs`.

Directly enabling QEMU features is risky because the QEMU feature set also carries assumptions that previously broke D1:

- virtio/block availability,
- PCI bus constants,
- display/input feature assumptions,
- full rootfs initialization.

Recommended direction:

- Either create a D1-specific minimal devfs path that does not require full `axfs`,
- or add a carefully scoped D1 fs feature that enables only the filesystem pieces required by pseudofs/devfs, without reintroducing virtio/PCI/display assumptions.

### Blocker 4: Embedded Benchmark Payload Is Not Yet the First Thing To Write

The earlier Q19B plan correctly recommended embedding `tests/benchmark.c` before SDMMC/rootfs parity. That is still right, but the current blocker comes before payload delivery.

The order should be:

1. Make `lichee-d1-userbench` compile with the required user/runtime modules.
2. Bring up `/dev/console` backed by `ASYNC_TTY`.
3. Add a tiny builtin user payload or simplest embedded ELF proof.
4. Only then embed the full `tests/benchmark.c` static RISC-V ELF.

Writing the benchmark payload first would hide the current feature/runtime boundary problem under ELF-loader noise.

### Blocker 5: Board Evidence Is Still Missing For Q19B Final Claims

The current host checks support smoke and kbench compile gates. Final Q19B still needs true board logs for:

- PLIC claim/complete of UART IRQ 18,
- RX/TX ISR wake path,
- D1 kernel ring-buffer metrics,
- `/dev/console` write reaching async UART,
- `tcdrain`/TEMT behavior on real D1 UART,
- user benchmark output sections.

The task list marks several Phase 3 and Phase 4 gates as complete. If board logs exist outside the repo, they should be copied into `.claude/analysis/lichee/q19b-YYYYMMDD-{mode}.txt`. If they do not exist yet, these should be treated as "code path wired, board evidence pending".

## Recommended Next Plan

### Q19B-Next.1: Normalize Feature Vocabulary

Introduce clearer feature meanings before more implementation:

- `lichee-d1`: platform selector.
- `lichee-d1-smoke`: smoke-only runtime.
- `lichee-d1-async-uart`: D1 async UART hardware path.
- `lichee-d1-kbench`: runtime mode that runs only kernel benchmark and halts.
- `lichee-d1-userbench`: runtime mode that includes async UART plus minimal user/devfs/syscall path.

The exact names can differ, but the important rule is that hardware capability and runtime mode should not be collapsed into one feature.

### Q19B-Next.2: Add A Minimal Userbench Runtime Gate

Before embedding benchmark.c, add a host gate:

```sh
cargo check --target riscv64gc-unknown-none-elf --features lichee-d1-userbench
```

This must pass without enabling QEMU PCI/virtio/display assumptions.

### Q19B-Next.3: Bring Up `/dev/console` Before ELF

The first runtime gate should print through `ASYNC_TTY` from kernel-controlled code or a minimal syscall smoke, not the full benchmark.

Success criteria:

- `ASYNC_TTY` exists in userbench mode.
- `/dev/console` can be registered.
- a write path reaches `AsyncUartWriter`.
- `tcdrain` can observe D1 TEMT.

### Q19B-Next.4: Add Embedded Benchmark ELF

After `/dev/console` works, compile `tests/benchmark.c` as static RISC-V ELF and embed it.

The benchmark source should remain shared with QEMU. Platform differences should be in delivery and runtime setup, not in benchmark behavior.

### Q19B-Next.5: Capture Board Evidence

Save raw serial logs under:

```text
.claude/analysis/lichee/q19b-YYYYMMDD-kbench.txt
.claude/analysis/lichee/q19b-YYYYMMDD-userbench.txt
```

Then update:

- `docs/benchmark-report-async.md`
- `.claude/docs/SNAPSHOT.md`
- `.claude/docs/tasks.md`
- `openspec/specs/learned/spec.md`
- `openspec/specs/optimization/spec.md`

## Non-Goals For The Immediate Next Step

- Do not start SDMMC/rootfs parity yet.
- Do not enable the full QEMU feature set on D1.
- Do not modify external crates just to bypass the current compile gate.
- Do not claim final Q19B completion from kernel benchmark alone.

## Conclusion

Q19B is not blocked by the D1 UART MMIO model anymore. It is blocked by the boundary between a minimal D1 kernel benchmark image and a D1 user benchmark image. The next design task is to separate "D1 async UART hardware capability" from "kbench-only minimal runtime", then build the smallest userbench runtime that can provide `/dev/console` and run an embedded benchmark ELF without pulling in QEMU PCI/virtio/rootfs assumptions.
