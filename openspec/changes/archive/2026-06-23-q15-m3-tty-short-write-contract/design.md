# Design: Q15 M3 TtyWrite Short Write Contract

## Decision

Use `usize` as the `TtyWrite::write` return value:

```rust
fn write(&self, buf: &[u8]) -> usize;
```

The value is the number of bytes accepted into the output sink, not necessarily transmitted over the wire. Wire completion remains the responsibility of `flush()` / `tcdrain()` via M2 `TxCompletion`.

## Implementation Notes

- `AsyncUartWriter::write` returns `self.driver.tx.push(buf)`.
- `PtyWriter::write` returns `push_slice(buf)` and keeps the short-write warning.
- `Tty::write_at` returns `Ok(self.writer.write(buf))`.
- `ldisc::output_char` uses `let _ = self.writer.write(...)` to document best-effort echo.
- Benchmark full-write paths must loop on short write, then call `tcdrain`.

## Risks

- User programs and benchmarks that assume one `write()` means full acceptance must handle short writes.
- Returning `Ok(0)` can surface to blocking callers if the ring is full. This is acceptable for M3 as a minimal correctness fix; blocking exact write requires a separate OUT readiness design.

