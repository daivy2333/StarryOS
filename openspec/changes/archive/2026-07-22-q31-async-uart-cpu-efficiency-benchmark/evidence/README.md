# Q31 CPU Efficiency Evidence

## Source Provenance

| Field | Value |
|---|---|
| Git branch | `uart-lichee` |
| Git HEAD | `f8819a2f0da205bacfdee80cba276cc278cc452d` |
| Working tree | dirty (uncommitted q31 changes) |
| Freeze date | 2026-07-21 |
| Toolchain | `riscv64-linux-musl-gcc (GCC) 11.2.1`, `rustc nightly-2026-02-25` |
| D1 serial | `/dev/ttyUSBx`, 115200 8N1, no flow control |
| Console device | `/dev/console`, `st_rdev` major=5 minor=1（character device） |

## Source File Hashes (Iteration 002 / Current)

| File | SHA-256 |
|---|---|
| `tests/benchmark.c` | `4ad658f3bfa4f41555a9e9a9a35c7bd0b2c0b080021220fd0a2668ec63b91da6` |
| `crates/axplat-riscv64-lichee-d1/src/time.rs` | `c821367ec41922565ba81e0ab8d6df8ae3706806f0e70afc8b69dae7ca8eecac` |
| `crates/axplat-riscv64-lichee-d1/src/time_math.rs` | `7839991923685473b85711ef87d8cc871024644f3a59a24e9dff27ca762bfd43` |
| `tests/benchmark` (QEMU binary) | `139a9012447c884789d062f64f57d75bb07b22f383aee8bd0faca204305185a0` |
| `kernel/resources/benchmark.elf` (D1 ELF) | `29b18d28caed0f09b306251289f0d0253f56022ab5510ddeea0759933614aaae` |
| `starry-lichee-fullbench-command-boot.img` | `70b251e439999d67200f5ebd6ad625f2bac2d9a7ae11fee9bfaac49654139805` |

## Time Math Tests

`rustc --test time_math.rs && ./test` → **12/12 PASS**.
Tests: one_second, nanos_to_ticks, round_trip (exact), zero, one_tick, one_ns, saturation, div_by_zero, monotonic, frequency_boundaries, general_round_trip, large_round_trip.

## RED Witness (time conversion)

Old `NANOS_PER_TICK = 41`: 24,000,000 ticks × 41 = **984,000,000 ns** (off by 16 ms/s).
New `mul_div_floor`: 24,000,000 ticks × 1e9 / 24,000,000 = **1,000,000,000 ns** (exact).

## Build & Run Commands

```bash
export PATH=/opt/musl/riscv64-linux-musl-cross/bin:$PATH
make BENCH_CC=/opt/musl/riscv64-linux-musl-cross/bin/riscv64-linux-musl-gcc tests/benchmark
make BENCH_CC=/opt/musl/riscv64-linux-musl-cross/bin/riscv64-linux-musl-gcc benchmark-fullbench-elf
make lichee-fullbench-command

# QEMU
sudo mount -o loop make/disk.img /tmp/rootfs_mnt
sudo cp tests/benchmark /tmp/rootfs_mnt/bin/benchmark
sudo umount /tmp/rootfs_mnt
make run
# inside QEMU: ./benchmark

# D1
dd if=/mnt/exUDISK/starry-lichee-fullbench-command-boot.img of=/dev/by-name/boot bs=1M conv=fsync
sync && reboot -f
```

## Async Gate Status: ✅ PASS (Async only)

Iteration 002 evidence meets all acceptance criteria:

| Check | QEMU | D1 |
|---|---|---|
| S41 5/5 valid (64/256/1024B) | ✅ | ✅ |
| S42 5/5 valid | ✅ | ✅ |
| S43 5+5 valid | ✅ | ✅ |
| byte_ok=0 | 0 | 0 |
| drain_errors > 0 | 0 | 0 |
| timeout=1 | 0 | 0 |
| Done | ✅ | ✅ |
| exit 0 | ✅ (via shell) | ✅ (via kernel) |
| D1 S10 regression <5% | — | 95.2% (baseline 96.6%, -1.4%) |

**Console 对照仍然缺失。** 最终影响判断需要同测试口径的 Console QEMU/D1 数据。

## D1 Iteration 002 Key Results

### S11 — Submission Fraction

| Size | submit_fraction | producer_available |
|---|---|---|
| 64B | 0.0030 | 0.9970 |
| 256B | 0.0010 | 0.9990 |
| 1024B | 0.3608 | 0.6392 |

producer_available = 1 - submit_fraction（浮点，非布尔）。

### S41 — TX CPU Work (instret: write start → final TEMT drain)

Completed bytes fixed: 64B=6,400, 256B=25,600, 1024B=102,400.

| Size | valid_rounds | median instructions_per_byte | median instructions_per_write |
|---|---|---|---|
| 64B | 5/5 | 32,818 | 2,100,350 |
| 256B | 5/5 | 32,792 | 8,394,568 |
| 1024B | 5/5 | 44,716 | 36,382 |

1024B 显著高于 64/256B（背景：D1 backpressure 下 `write()` 产生大量 retry syscall）。

### S42 — TX Compute Overlap

Completed bytes fixed: 6,400 (64B × 100).

| valid_rounds | median_useful_iters | median_overlap_efficiency | median total_over_line_ratio |
|---|---|---|---|
| 5/5 | 145,932 | 0.5353 | 1.550 |

D1 total/line ratio ≈1.55：coper drain tail ~298ms/round，占完成时间 ~35%。

### S43 — Timer Wakeup Overshoot

Samples: 50/group, interval 5 ms, 5 idle + 5 loaded groups.

| Phase | groups | aggregate P50 | aggregate P95 | aggregate P99 | aggregate max |
|---|---|---|---|---|---|
| Idle | 5/5 | 9.53 ms | 9.82 ms | 15.85 ms | 15.85 ms |
| Loaded (4,096 B burst) | 5/5 | 25.8 ms | 47.3 ms | 49.6 ms | 49.6 ms |

Burst: completed 4,096/4,096 B per group, write duration < theoretical line time (347 ms).

### Counter Derivation

原始日志中 counter 字段为 raw counter values。以下 derived 值由 README 公式从 raw 字段推导，不改变原始日志：

```
hw_send_calls_per_kb = hw_send_calls / (completed_bytes / 1024)
ring_pop_calls_per_kb = ring_pop_calls / (completed_bytes / 1024)
bytes_per_hw_send     = hw_send_bytes / hw_send_calls  (%.3f precision)
```

### Diagnostic Limitations

以下诊断信息不在当前 success-path 日志中，已记录为此 Async evidence 的已知限制：

1. **retry decomposition**：成功样本不拆分 partial、zero-progress、timeout 和 errno 子类型（完整完成时这些计数为 0，拆分无意义）。
2. **counter regression**：instret reader 有独立的 begin/end status 和 reason code，但 counter-regression 路径未单独触发（当前 D1/QEMU 均为单 hart，instret 严格单调）。
3. **manifest provenance**：`source_revision`/`source_dirty` 为 `not-available`（构建时未传宏）。Git HEAD + 源码 hash 补足 provenance。
4. **hart_count**：为 `not-available`（未传宏）。D1 单 hart，QEMU 默认单 hart。

### CPU-Work Proxy 声明

- `/proc/instret` 报告当前 hart 的 retired-instruction counter。S41 delta 是 **同环境 CPU-work proxy**，**不是 task CPU time 或 CPU utilization**。
- 不声明百分比 CPU 使用率、系统 CPU load 或 Sleep/Wait 比率。
- 不同 hart 数量或 OS runtime 环境下不可直接比较绝对值；同环境（同 hart、同 OS build、同 workload、同完成点）的相对比较有效。

## Evidence Logs

### Iteration 002 — Current Async Evidence

| File | SHA-256 | Validation |
|---|---|---|
| `async/qemu-rootfs.log` | `a9ce8a34431ff6b9a609ffde83da2096228f17c37c3f900ec35f3c10939ce8ef` | Clean: 0 byte_ok=0, 0 drain_errors, Done |
| `async/d1-fullbench-command.log` | `50a2a87666045c1379391bec46e3453026967e028fa586abbcae8155576f0789` | Clean: 0 byte_ok=0, 0 drain_errors, 0 timeout, Done exit 0 |

### Iteration 001 — Valid History (preserved for reference)

| File | SHA-256 |
|---|---|
| `async/iteration-001-valid/qemu-rootfs.log` | `5707065d637bc19408d25f7e760a6658204e72b9579a61a1d6dd5af4c0fd6f3a` |
| `async/iteration-001-valid/d1-fullbench-command.log` | `0a411c5dc8df57d5af3e0e6cae595999e83df09b28f09eb53019553b279bf719` |

### Iteration 000 — Invalid History (S41 byte_ok=0, single rounds)

| File | SHA-256 |
|---|---|
| `async/iteration-000-invalid/qemu-rootfs.log` | `33bacbd184748304e1c8c7ae3d850d9d5b875b4c43c8e14648c13189a6298a06` |
| `async/iteration-000-invalid/d1-fullbench-command.log` | `f32c16cfa62536e9abb69468ba87cea846dd028830acf256772c66f4e51c9d09` |

### Baseline — Pre-Q31 Frozen Logs (never changed)

| File | SHA-256 |
|---|---|
| `baseline/async-d1.md` | `b98af673ca56ab983c55f3ddaf4f7f39228f7a4ec69f88b6b1f0a907731947cc` |
| `baseline/async-qemu.md` | `d2f2486aa1f4df452ae14880c22ad3d08467561ae5f7799affc768b972ae15d2` |
| `baseline/console-d1.md` | `46ac67bd52f01025b891ede2861fa646af9b9637e201a15b5fb85ed8c3b8a91f` |
| `baseline/console-qemu.md` | `748f0ad9cbd41b466e8d05e6c61fee07d10666e470ace54c2c85a3aa46737da4` |

## Directory Layout

```
q31-cpu-efficiency-evidence/
├── README.md
├── baseline/                    — frozen pre-Q31
├── async/
│   ├── qemu-rootfs.log          — iteration 002 current
│   ├── d1-fullbench-command.log — iteration 002 current
│   ├── iteration-000-invalid/   — historical failures
│   └── iteration-001-valid/     — historical valid
├── console/                     — later iteration
└── comparison/                  — later iteration
```
