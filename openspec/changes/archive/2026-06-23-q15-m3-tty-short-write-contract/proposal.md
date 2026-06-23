# q15-m3-tty-short-write-contract

## Why

`TtyWrite::write(&[u8])` currently has no return value. Both `AsyncUartWriter` and `PtyWriter` can accept fewer bytes than requested, but `Tty::write_at()` still reports `Ok(buf.len())`. This creates silent data loss when the TX ring or PTY buffer is full.

## What Changes

- Change `uart_16550::TtyWrite::write` to return the actual accepted byte count.
- Propagate that count through StarryOS `Tty::write_at()`.
- Update `AsyncUartWriter` and `PtyWriter` implementations.
- Make line discipline echo explicitly best-effort.
- Update benchmark/write witness code to loop on short writes where full transmission is intended.

## Non-Goals

- Do not implement blocking exact write.
- Do not change TX copier, IER ownership, or TxCompletion drain logic.
- Do not modify axtask, axpoll, or external upstream crates.

## BDD Scenario Sketch

- Happy path: writer accepts the full buffer, `sys_write` observes the full byte count.
- Edge path: writer accepts only part of a buffer, `Tty::write_at` returns the partial count.
- Full-buffer path: writer accepts zero bytes, M3 returns `Ok(0)` as the minimal contract fix.
- Echo path: input echo remains best-effort and ignores the returned count explicitly.
- Benchmark path: full-send tests loop until all bytes are accepted before `tcdrain`.

