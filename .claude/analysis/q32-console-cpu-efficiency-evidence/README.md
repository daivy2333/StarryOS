# Q32 Console CPU Efficiency — Evidence

> Final state: 2026-07-22, commit `f61def3`

## Source Provenance

| Field | Value |
|---|---|
| Branch | `console-lichee` |
| Git HEAD | `f61def3f325694cc98d20b445b48636280d68abf` |
| Host rustc | `rustc 1.95.0-nightly (859951e3c 2026-02-24)` |
| Cross toolchain | `riscv64-linux-musl-gcc (GCC) 11.2.1` at `/opt/musl/riscv64-linux-musl-cross/bin` |

## Q31 Frozen Input Hashes

All verified against iteration 000 Plan Context.

| Input | SHA-256 |
|---|---|
| Q31 Async QEMU log | `a9ce8a34431ff6b9a609ffde83da2096228f17c37c3f900ec35f3c10939ce8ef` |
| Q31 Async D1 log | `50a2a87666045c1379391bec46e3453026967e028fa586abbcae8155576f0789` |
| Q31 time_math.rs | `7839991923685473b85711ef87d8cc871024644f3a59a24e9dff27ca762bfd43` |

## Console Source Files (HEAD `f61def3`)

| File | SHA-256 | Note |
|---|---|---|
| `tests/benchmark.c` | `32656017a293fcf3607de520632a53c3500b8b0dc3d9db8a204a7b0a8343e377` | Q31 base + Console adapt |
| `tests/benchmark_classify.h` | (new) | 5 pure classification helpers |
| `tests/benchmark_classify_test.c` | (new) | 26 host boundary tests, 26/26 GREEN |
| `crates/axplat-riscv64-lichee-d1/src/time.rs` | `580f6cce22c881d936df783155e3a60689ea74e061b5b3bdbbd62d05a490b9ec` | mul_div_floor wired |
| `crates/axplat-riscv64-lichee-d1/src/time_math.rs` | `7839991923685473b85711ef87d8cc871024644f3a59a24e9dff27ca762bfd43` | Matches Q31 hash |
| `crates/axplat-riscv64-lichee-d1/src/lib.rs` | `52bd2abc79db0d8a58547ddbe9cb2d7c3c6143502cb26855dacce953b1b598d0` | added `mod time_math;` |
| `Cargo.toml` | modified | fullbench-command/userbench/fullbench +`irq` feature |

## Console Artifacts

| Artifact | SHA-256 |
|---|---|
| QEMU benchmark binary | `2ce5c072870d1fab7b4f47c742d0408834a2a0a7607296246c06c3a94c6894a2` |
| D1 benchmark ELF | `2f0d869a0c558d02031630de7668f7119a7be11c42e10bb895f0a10e72d5387b` |
| D1 boot image | `1e85b6127a1e75306d5969d4eedb7cab50a795d55f20450f932820585a309bad` |

## Console Frozen Logs

| Log | SHA-256 | Location |
|---|---|---|
| QEMU Console | `67b7bb0260b717ad91adee3112c65bbc308f44a2d2a681dcc05ffad0094e227c` | `console/qemu-rootfs.log` |
| D1 Console | `b3f11fce62696e92077cd3f9693520708df739f42ed755f1eab8ffb513555aaf` | `console/d1-fullbench-command.log` |
| Iteration 000 QEMU (frozen) | `701708e202aaac97a1fdaff6d284541cb2a3625fe7c6b7cfb183a8b465915578` | `iteration-000/qemu_console.md` |

## QEMU Gate Result

| Check | Result |
|---|---|
| Title `Console Benchmark` | PASS |
| S00 `backend=polling-console` | PASS |
| S05 `SKIPPED reason=no-async-driver` | PASS |
| S11 `write_semantics=synchronous-blocking` | PASS |
| S41 15/15 valid rounds | PASS |
| S42 5/5 valid, ovlp ~1.05 | PASS |
| S43 5 idle + 5 loaded, all PASS | PASS |
| S40 UNSUPPORTED | PASS |
| Done, drain_errors=0 | PASS |

## D1 Gate Result

| Check | Result |
|---|---|
| Title `Console Benchmark` | PASS |
| S41 15/15 valid, inst/byte: 1194 / 1105 / 1105 | PASS |
| S42 5/5 valid, overlap=0.0000 | PASS |
| S43 idle 5/5 PASS (~8.4-8.8ms overshoot) | PASS |
| S43 loaded 5/5 not-applicable (write_dur ~355ms > 347ms) | PASS |
| S40 UNSUPPORTED | PASS |
| Done + exit 0, drain_errors=0 | PASS |

## Key Fixes

1. **D1 time conversion**: `mul_div_floor` replaces truncated `NANOS_PER_TICK=41`. 12/12 host tests. Q31 hash match.
2. **D1 S43 hang**: Root cause was IRQ stub (no timer handler). Fixed by adding `axplat-riscv64-lichee-d1/irq` to fullbench-command/userbench/fullbench features in `Cargo.toml`.
3. **Classification helpers**: `benchmark_classify.h` with 26 host boundary tests, integrated into benchmark.c for S41/S42/S43.

## Known Deviations

- No standalone 5ms timer smoke (full S43 suffices for readiness proof). Iteration 002 Plan Review accepted.
- No `lichee-d1-runtime-irq` composite feature (three runtime features directly enable `/irq`). Accepted.
- Implementation details in iteration 001 Act Response, not 002.
