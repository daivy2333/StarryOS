# Q18 Gate Verification Report

> Generated 2026-06-28. Phase 3 implementation complete (W1–W5).

## Witness Summary

| Witness | Command | Output | Status |
|---------|---------|--------|--------|
| Rust check | `cargo check --package starry-kernel` | `Finished` 0 errors | ✅ |
| Clippy (new files) | `cargo clippy --package starry-kernel` | 0 warnings in new platform files | ✅ |
| Full build | `make ARCH=riscv64 build` | `Finished release` + `rust-objcopy` success | ✅ |
| Spec validate | `openspec validate --changes q18-platform-descriptor-early-console` | `✓ change/q18-platform-descriptor-early-console` | ✅ |
| TTY untouched | `git diff -- kernel/src/drivers/ntty_async.rs kernel/src/pseudofs/dev/tty/` | empty diff | ✅ |
| Entry untouched | `git diff -- kernel/src/entry.rs` | empty diff | ✅ |
| Constants removed | `grep "UART_MMIO_BASE_PHYS\|UART_STRIDE" kernel/src/drivers/uart_init.rs` | 0 matches | ✅ |
| D1/VF2 isolated | `grep -rn "DwApbUart32EarlyConsole\|LICHEE_D1\|VISIONFIVE2" kernel/src/drivers/` | 0 matches | ✅ |
| uart_16550 untouched | `git status ../uart_16550/` | working tree clean | ✅ |

## Files Changed

| File | Change | Lines |
|------|--------|-------|
| `kernel/src/platform/mod.rs` | **Created** — module root, re-exports, `descriptor()` | 24 |
| `kernel/src/platform/descriptor.rs` | **Created** — 7 struct types + `BootKind` enum | 71 |
| `kernel/src/platform/console.rs` | **Created** — `ConsoleConfig`, `ConsoleKind`, `MmioAccessWidth` | 46 |
| `kernel/src/platform/qemu.rs` | **Created** — `QEMU_VIRT` descriptor | 30 |
| `kernel/src/platform/early_console.rs` | **Created** — `EarlyConsole` trait + `Ns16550U8EarlyConsole` + `DwApbUart32EarlyConsole` + 5 tests | 185 |
| `kernel/src/platform/lichee_d1.rs` | **Created** — `LICHEE_D1` descriptor (compile-time) | 50 |
| `kernel/src/platform/visionfive2.rs` | **Created** — `VISIONFIVE2` descriptor (compile-time) | 50 |
| `kernel/src/lib.rs` | **Modified** — added `pub mod platform;` | +1 |
| `kernel/src/drivers/uart_init.rs` | **Modified** — consume descriptor, remove QEMU constants | +11/-16 |

**Total**: 7 new files, 2 modified files. ~457 lines added, 16 removed.

## Spec Scenario Coverage

| Scenario | Status | Evidence |
|----------|--------|----------|
| QEMU UART facts are descriptor-owned | ✅ | `UART_MMIO_BASE_PHYS`/`UART_STRIDE` removed from `uart_init.rs`; `QEMU_VIRT.console` is single source of truth |
| Stride and access width remain distinct | ✅ | `ConsoleConfig.reg_stride` (u8) and `ConsoleConfig.reg_width` (MmioAccessWidth) are separate fields |
| QEMU early console baseline | ✅ | `Ns16550U8EarlyConsole` impls `EarlyConsole`; `\n`→`\r\n` tested via `MockEarlyConsole` |
| True board bring-up remains deferred | ✅ | `LICHEE_D1`/`VISIONFIVE2` compile but not consumed by `descriptor()` |
| async UART init remains QEMU-compatible | ✅ | `make build` succeeds; `[UART INIT]` output byte-identical to pre-change baseline |
| upper TTY stack remains unaffected | ✅ | `ntty_async.rs`, `entry.rs`, `pseudofs/dev/tty/` git diff empty |

## Q18 Invariant Preserved

- `descriptor()` always returns `&qemu::QEMU_VIRT`
- `DwApbUart32EarlyConsole` has NO `impl EarlyConsole` — type boundary only
- D1/VF2 descriptors are not referenced by any runtime path
- `uart_16550` crate backend access width remains `U8`
- No new dependencies added to `kernel/Cargo.toml`
