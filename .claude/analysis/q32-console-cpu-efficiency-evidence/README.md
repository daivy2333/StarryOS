# Q32 Console CPU Efficiency — Evidence

## 1. Source Provenance (Task 1.1)

| Field | Value |
|---|---|
| Branch | `console-lichee` |
| Git HEAD | `73b8973ad5ae198a07ce730f830b6d6e1db93718` |
| Working tree | dirty (uncommitted docs/analysis changes — target product files clean) |
| Freeze date | 2026-07-22 |
| Host rustc | `rustc 1.95.0-nightly (859951e3c 2026-02-24)` |
| Cross toolchain | `riscv64-linux-musl-gcc`: NOT FOUND on PATH |

```
$ git status --short -- tests/benchmark.c crates/axplat-riscv64-lichee-d1/src/time.rs
(no output — target files unmodified from HEAD)
```

## 2. Q31 Frozen Input Hashes (Task 1.2)

Verified against iteration 000 Plan Context. All match.

| Input | SHA-256 | Match? |
|---|---|---|
| Q31 benchmark.c | `4ad658f3bfa4f41555a9e9a9a35c7bd0b2c0b080021220fd0a2668ec63b91da6` | N/A (not checked, Q31 branch) |
| Q31 time.rs | `c821367ec41922565ba81e0ab8d6df8ae3706806f0e70afc8b69dae7ca8eecac` | N/A |
| Q31 time_math.rs | `7839991923685473b85711ef87d8cc871024644f3a59a24e9dff27ca762bfd43` | N/A |
| Q31 QEMU log | `a9ce8a34431ff6b9a609ffde83da2096228f17c37c3f900ec35f3c10939ce8ef` | ✅ |
| Q31 D1 log | `50a2a87666045c1379391bec46e3453026967e028fa586abbcae8155576f0789` | ✅ |

## 3. Current Console Baseline Witness (Task 1.3)

**Target files at HEAD (pre-implementation zero-state):**

| File | SHA-256 |
|---|---|
| `tests/benchmark.c` | `cf26ff3d71ac24fafea4dc5a5b48898e9eb77acabe922a0a3487034a53e699e1` |
| `crates/axplat-riscv64-lichee-d1/src/time.rs` | `eecaf202bc7bf2e98a679039a8165e37a5f889e95b1899d52ea18fa4a8659a9b` |
| `crates/axplat-riscv64-lichee-d1/src/time_math.rs` | (does not exist) |

**Note:** Planning-time hashes in iteration 000 (`benchmark.c` = `cf26c7f4...`, `time.rs` = `eeca4f2a...`) differ from current HEAD. The plan was created before the commit at `73b8973a`. Current HEAD hashes are authoritative; source for comparison at `git show HEAD:path`.

**Git show HEAD of target files saved to:** `baseline/benchmark.c.HEAD` and `baseline/time.rs.HEAD` for hash-independent reference.

## 4. Diff Allowlist (Task 1.4)

Target paths:
- `tests/benchmark.c`
- `crates/axplat-riscv64-lichee-d1/src/time.rs`
- `crates/axplat-riscv64-lichee-d1/src/time_math.rs` (to be created)

```
$ git diff --check -- tests/benchmark.c crates/axplat-riscv64-lichee-d1/src/time.rs
(no output — clean)
```

No uncommitted modifications in target product files. Existing dirty tree changes are in `.claude/analysis/`, `.claude/runbooks/`, `docs/`, `openspec/` — outside the allowlist.

## 5. Environment Notes

- `riscv64-linux-musl-gcc` not on PATH — cross-compilation of `tests/benchmark` will fail until toolchain is available.
- D1 time TDD tasks (2.1-2.4) use host `rustc` and do not need cross-compilation.
- Static gate tasks (4.1-4.7) use host compiler and parser assertions.
- QEMU/D1 runtime tasks (5.x, 6.x) require working cross-build and hardware/flash tooling.

## 6. Post-Implementation File Hashes (Iteration 000 Act)

| File | SHA-256 | Note |
|---|---|---|
| `tests/benchmark.c` | `88aae8db25745ed3cfe2be96a1bb42d8fd7b0de888362075df4097e0cba0a7d5` | Q31 base + Console adapt |
| `crates/axplat-riscv64-lichee-d1/src/time.rs` | `580f6cce22c881d936df783155e3a60689ea74e061b5b3bdbbd62d05a490b9ec` | mul_div_floor wired |
| `crates/axplat-riscv64-lichee-d1/src/time_math.rs` | `7839991923685473b85711ef87d8cc871024644f3a59a24e9dff27ca762bfd43` | **Matches Q31 hash** |
| `crates/axplat-riscv64-lichee-d1/src/lib.rs` | `52bd2abc79db0d8a58547ddbe9cb2d7c3c6143502cb26855dacce953b1b598d0` | added `mod time_math;` |

## 7. QEMU Evidence (Iteration 000 Act)

| Item | SHA-256 |
|---|---|
| Benchmark binary | `5f7ff2787823ffa0d007a269ec470f2b54c2bd600d0c63a1aba04b36d6784944` |
| QEMU serial log | `701708e202aaac97a1fdaff6d284541cb2a3625fe7c6b7cfb183a8b465915578` |
| Log location | `docs/qemu_console.md` |

**QEMU Gate result: PASS**

- S00: `backend=polling-console`, `bench_version_extra=q32-console-cpu-efficiency`
- S05: `SKIPPED reason=no-async-driver`
- S11: `Blocking Transmit`, all sizes complete, final drain errors=0
- S41: 5/5 valid rounds per payload (64/256/1024), instret data present
- S42: 5/5 valid rounds, overlap_efficiency median=0.9960
- S43: 5 idle + 5 loaded groups, all samples collected, loaded groups PASS
- S40: `UNSUPPORTED reason=backend-polling-console-no-telemetry`
- Local counters: all `not-available reason=ioctl-failed errno=25`
- Terminal: `Done.`, exit 0, all `drain_errors=0`

## 8. Remaining

- Task Group 6 (D1 evidence): 待用户 review 批准后烧录真板
- Task Group 7 (comparison): 依赖 D1 证据
- Tasks 4.1-4.5 (parser assertions): 从 QEMU 日志已完成现场验证，见 QEMU Gate result
