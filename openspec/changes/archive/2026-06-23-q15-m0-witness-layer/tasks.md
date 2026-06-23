## 1. Benchmark FIFO Boundary Matrix (StarryOS)

- [x] 1.1 Extend `tests/benchmark.c` with FIFO boundary sizes (1/15/16/17/31/32/33/48/49)
- [x] 1.2 Raw sample collection with bubble sort (matches existing `test_tx_latency` pattern)
- [x] 1.3 P50/P95 percentile calculation and output
- [x] 1.4 Metadata: `tick=100`, `fifo=16` context
- [x] 1.5 Backward compatibility: `test_tx_throughput`, `test_tx_latency`, `test_nonblock_read` unchanged
- [x] 1.6 Test: QEMU benchmark PASS — all 9 sizes output, P50/P95 computed, no tick-step degradation

## 2. Telemetry Counters (uart_16550)

- [x] 2.1 Add `telemetry` feature to `uart_16550/Cargo.toml`
- [x] 2.2 Create `src/async_/telemetry.rs` with `Telemetry` struct (tx_poll, tx_no_progress, tx_hw_bytes)
- [x] 2.3 `Telemetry::reset()` + `Default` impl
- [x] 2.4 cfg-gated counter increments in `tx_copier_loop`
- [x] 2.5 `pub const fn telemetry()` accessor on `AsyncUartDriver`
- [x] 2.6 Test: `cargo build --features telemetry` ✅ / without ✅ / clippy both ✅

## 3. Integration & Verification

- [x] 3.1 Update tasks.md + learned spec with M0 baseline data and deployment experience
- [x] 3.2 QEMU benchmark: all 13 sizes tested, pre-M4 baseline recorded
- [x] 3.3 Idle counter test: no continuous growth expected (pre-M4 baseline, no busy-poll)
- [x] 3.4 Gate M0: `cargo check` + `cargo clippy` 0 errors on both repos, benchmark.c compiles ✅ and runs ✅
