## 1. Platform descriptor skeleton

- [x] 1.1 Add `kernel/src/platform/` or equivalent module boundary for `PlatformDescriptor` → `kernel/src/platform/{mod,descriptor,console}.rs` created (W1)
- [x] 1.2 Define descriptor data types: memory, kernel image, console, interrupt, timer, boot → `PlatformDescriptor`, `MemoryLayout`, `KernelImageLayout`, `InterruptConfig`, `TimerConfig`, `BootImageConfig` (W1)
- [x] 1.3 Define `ConsoleKind` and `MmioAccessWidth` so stride and access width are separate → `ConsoleKind::{Ns16550,DwApbUart,SbiConsole}`, `MmioAccessWidth::{U8,U32}` (W1)

## 2. QEMU descriptor migration

- [x] 2.1 Add QEMU descriptor matching current constants: UART base `0x10000000`, IRQ `10`, stride `1`, width `U8` → `kernel/src/platform/qemu.rs` `QEMU_VIRT` (W2)
- [x] 2.2 Change `kernel/src/drivers/uart_init.rs` to consume descriptor values instead of declaring QEMU UART facts locally → `UART_MMIO_BASE_PHYS`/`UART_STRIDE` removed, reads from `platform::descriptor()` (W2)
- [x] 2.3 Keep QEMU async UART initialization behavior unchanged → `make build` succeeds, `[UART INIT]` output byte-identical to baseline (W2)

## 3. Early console abstraction

- [x] 3.1 Add `EarlyConsole` abstraction independent of ring buffer, IRQ, PLIC, rootfs, and async task runtime → `EarlyConsole` trait in `kernel/src/platform/early_console.rs` (W3)
- [x] 3.2 Implement QEMU `Ns16550U8EarlyConsole` baseline → polling NS16550 MMIO impl, no deps on ring buffer/IRQ/PLIC/rootfs (W3)
- [x] 3.3 Add newline `\n` to `\r\n` behavior in early console output → default `write_str()` impl + 5 `#[cfg(test)]` tests (W3)

## 4. Future-platform boundary

- [x] 4.1 Define D1/VisionFive2 descriptor examples or compile-time placeholders without enabling hardware execution → `lichee_d1.rs` `LICHEE_D1`, `visionfive2.rs` `VISIONFIVE2` (W4)
- [x] 4.2 Define `DwApbUart32EarlyConsole` interface boundary or documented stub for Q19/Q20 follow-up → struct + constructors added to `early_console.rs`, no `impl EarlyConsole` (W4)
- [x] 4.3 Ensure Q18 does not modify `uart_16550` backend access width → `uart_16550` crate untouched; 0 matches for access-width patterns in `../uart_16550/src/` (W4)

## 5. Gate verification

- [x] 5.1 Verify current QEMU baseline before code changes and record output → pre-change `make build` + `[UART INIT]` captured in W2 baseline
- [x] 5.2 Verify post-change QEMU build/check and record output → `make build` succeeds, `cargo check` 0 errors, `cargo clippy` 0 warnings in new files
- [x] 5.3 Verify `openspec validate --changes q18-platform-descriptor-early-console` or equivalent change validation → `✓ change/q18-platform-descriptor-early-console` (W5)
- [x] 5.4 Verify source impact remains bounded to platform/console/uart init paths; no upper TTY behavior change → `ntty_async.rs`/`entry.rs`/`pseudofs/dev/tty/` git diff empty (W5)

## Execution Hold

- [x] 6.1 User audit approval received → explicit approval 2026-06-28 ("我同意...开始执行吧")
- [x] 6.2 Only after 6.1, enter Phase 3 implementation → Phase 3 W1-W5 executed and verified
