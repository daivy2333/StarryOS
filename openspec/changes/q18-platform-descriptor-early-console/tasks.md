## 1. Platform descriptor skeleton

- [ ] 1.1 Add `kernel/src/platform/` or equivalent module boundary for `PlatformDescriptor`
- [ ] 1.2 Define descriptor data types: memory, kernel image, console, interrupt, timer, boot
- [ ] 1.3 Define `ConsoleKind` and `MmioAccessWidth` so stride and access width are separate

## 2. QEMU descriptor migration

- [ ] 2.1 Add QEMU descriptor matching current constants: UART base `0x10000000`, IRQ `10`, stride `1`, width `U8`
- [ ] 2.2 Change `kernel/src/drivers/uart_init.rs` to consume descriptor values instead of declaring QEMU UART facts locally
- [ ] 2.3 Keep QEMU async UART initialization behavior unchanged

## 3. Early console abstraction

- [ ] 3.1 Add `EarlyConsole` abstraction independent of ring buffer, IRQ, PLIC, rootfs, and async task runtime
- [ ] 3.2 Implement QEMU `Ns16550U8EarlyConsole` baseline
- [ ] 3.3 Add newline `\n` to `\r\n` behavior in early console output

## 4. Future-platform boundary

- [ ] 4.1 Define D1/VisionFive2 descriptor examples or compile-time placeholders without enabling hardware execution
- [ ] 4.2 Define `DwApbUart32EarlyConsole` interface boundary or documented stub for Q19/Q20 follow-up
- [ ] 4.3 Ensure Q18 does not modify `uart_16550` backend access width

## 5. Gate verification

- [ ] 5.1 Verify current QEMU baseline before code changes and record output
- [ ] 5.2 Verify post-change QEMU build/check and record output
- [ ] 5.3 Verify `openspec validate --changes q18-platform-descriptor-early-console` or equivalent change validation
- [ ] 5.4 Verify source impact remains bounded to platform/console/uart init paths; no upper TTY behavior change

## Execution Hold

- [ ] 6.1 User audit approval received
- [ ] 6.2 Only after 6.1, enter Phase 3 implementation
