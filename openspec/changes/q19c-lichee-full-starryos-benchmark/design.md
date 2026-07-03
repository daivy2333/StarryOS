## Context

QEMU 的 StarryOS 启动模型是 shell/script first：

```text
src/main.rs
  -> args = ["/bin/sh", "-c", include_str!("init.sh")]
  -> starry_kernel::entry::init(args, envs)
  -> pseudofs::mount_all()
  -> FS_CONTEXT.resolve("/bin/sh")
  -> load_user_app()
  -> Process::new_init()
  -> ASYNC_TTY.bind_to()
  -> add_stdio() opens /dev/console
  -> spawn and join init process
```

Q19B 的 D1 userbench 模型是 embedded payload first：

```text
lichee_d1_init()
  -> init async UART
  -> run kernel benchmark
  -> init_memory_root()
  -> mount_all()
  -> include_bytes!("../resources/benchmark.elf")
  -> load_embedded_user_app()
  -> Process::new_init()
  -> ASYNC_TTY.bind_to()
  -> add_stdio()
  -> spawn and join benchmark
```

Q19C 要把 D1 路径从 embedded payload first 推进到 shell/script capable。实施上先做 M0 benchmark evidence cleanup，让后续数据可比较；工程上仍分成两段：Part A 解决 StarryOS 内部启动链路，Part B 解决真实块设备/rootfs。

## Goals / Non-Goals

**Goals:**

- Lichee 能通过 VFS path 解析运行 `/bin/benchmark`。
- Lichee 能通过 `/bin/sh -c /init.sh` 或等价脚本入口运行 benchmark。
- Q19B embedded benchmark 继续作为 regression baseline。
- Q19C-M0 先让 benchmark 输出包含参数 manifest，并规划真板 RX witness 与 64B 小包优化实验。
- 真板 rootfs 探索有明确采集项、接入点、失败输出和验收标准。
- benchmark 证据能区分启动链路：QEMU、Q19B embedded、Q19C memory-root path、Q19C shell/script、Q19C rootfs path。

**Non-Goals:**

- 不把 `qemu` feature 复用到 D1。
- 不把 memory-root 称为真实 rootfs。
- 不把 SDMMC/rootfs 探索和 memory-root shell 闭环绑成一个不可拆任务。
- 不删除或弱化 Q19B 的真板回归路径。

## Architecture

### Runtime Modes

| Mode | Root provider | User entry | Loader | Purpose |
|------|---------------|------------|--------|---------|
| `lichee-d1` | none | smoke marker | none | D1 boot smoke |
| `lichee-d1-kbench` | none | kernel benchmark | none | async UART kernel benchmark |
| `lichee-d1-userbench` | memory root for devfs only | embedded benchmark bytes | `load_embedded_user_app()` | Q19B regression |
| `lichee-d1-fullbench-mem` | populated memory root | `/bin/benchmark` | `load_user_app()` | path loader proof |
| `lichee-d1-fullbench-shell` | populated memory root | `/bin/sh -c /init.sh` or equivalent | `load_user_app()` | shell/script proof |
| `lichee-d1-fullbench-rootfs` | SDMMC/block rootfs | rootfs `/bin/sh` / `/bin/benchmark` | `load_user_app()` | full board parity |

Implementation can expose these as separate features, make targets, or one feature with compile-time mode selection. The observable contract is the important part: each image logs its mode and does not silently fall back to a weaker mode.

### M0: Benchmark Evidence Cleanup

M0 does not change the loader/rootfs architecture. It prepares the benchmark payload and evidence format so QEMU, Q19B embedded and later Q19C modes can be compared without guessing hidden parameters.

Current `tests/benchmark.c` facts from CodeGraph:

| Area | Current state |
|------|---------------|
| TX throughput | sizes `{64, 256, 1024, 4096}`, `iterations = 100`, `write()` loop, `tcdrain()` after every iteration |
| TX latency | single byte, `LAT_N = 100`, reports avg/P50/P95/P99 |
| FIFO matrix | sizes `{1, 15, 16, 17, 31, 32, 33, 48, 49}`, `MAT_N = 100`, per-write `tcdrain()` |
| RX | no-input nonblocking read only: `open(O_NONBLOCK)` and `ioctl(FIONBIO)` |
| Missing evidence | no benchmark version, no startup-chain label, no root-provider label, no explicit timer/source manifest, no fixed-payload RX measurement |

Planned M0 benchmark manifest:

```text
benchmark_version=q19c-m0
target_mode=<qemu-rootfs-shell|lichee-embedded-userbench|...>
startup_chain=<embedded|memory-root-path|memory-root-shell|rootfs-shell>
root_provider=<qemu-rootfs|memory-root|sdmmc-rootfs|none>
timer_source=CLOCK_MONOTONIC
tx_sizes=64,256,1024,4096
tx_iters=100
tx_drain_policy=tcdrain-per-iteration
latency_iters=100
fifo_matrix_sizes=1,15,16,17,31,32,33,48,49
rx_mode=<no-input-eagain|manual-fixed-payload|loopback-fixed-payload>
```

M0 RX work is an evidence plan, not a claim that TX proves RX performance. The minimum board witness remains no-input `EAGAIN`; the next useful witness is a fixed-payload read mode using manual serial injection or loopback if available.

M0 also keeps the 64B D1 result visible: `size=64 / iters=100 / 1.01 KB/s / 8.8% line rate`. Small-packet optimization experiments should be reported separately from the baseline:

| Experiment | Purpose | Must label |
|------------|---------|------------|
| baseline drain-per-iteration | Preserve existing semantics | `tx_drain_policy=tcdrain-per-iteration` |
| no-drain enqueue throughput | Separate enqueue cost from physical drain | `tx_drain_policy=no-drain` |
| batch-N then drain | Amortize drain/scheduler overhead | batch size and drain count |
| `writev` fragments | Test syscall-side aggregation | fragment count and total payload |
| 64/128/256 break-even | Find size where line-rate behavior begins | same iters and drain policy |

### Part A: Memory-root Fullbench

Part A extends the existing D1 memory-root escape hatch into a populated in-memory root filesystem.

Required layout:

```text
/
├── bin/
│   ├── benchmark
│   └── sh                 optional for M2 if static shell is available
├── init.sh                optional for M2
├── dev/                  mounted by mount_all()
├── dev/shm/              mounted by mount_all()
├── proc/                 mounted by mount_all()
├── sys/                  mounted by mount_all()
└── tmp/                  mounted by mount_all()
```

M1 uses `/bin/benchmark` directly. M2 uses `/bin/sh -c /init.sh` or a documented equivalent script command. Direct benchmark launch is not enough for M2 unless the equivalent command entry exercises the same argv/envp/stdio/exit path that shell/script would exercise.

### Part B: Real Rootfs Fullbench

Part B replaces the memory-root provider with a real block-backed rootfs:

```text
Allwinner D1 SDMMC
  -> D1 block driver or reused initialized block device
  -> AxBlockDevice
  -> AxDeviceContainer<AxBlockDevice>
  -> axfs_ng::init_filesystems(block_devs)
  -> FS_CONTEXT root
  -> mount_all()
  -> load_user_app("/bin/sh" or "/bin/benchmark")
```

Rootfs mode must not call `init_filesystems()` with an empty block device list. A missing block device is a hardware bring-up blocker, not a filesystem success or benchmark failure.

## Design Decisions

### D1: Add a new fullbench mode instead of expanding userbench

Q19B `lichee-d1-userbench` remains embedded-loader based. Q19C adds fullbench modes with explicit log labels.

Reason:

- Embedded userbench is the known-good board regression.
- Path loader and shell/rootfs failures need isolation from UART/syscall regressions.
- It prevents accidental success where fullbench silently falls back to embedded bytes.

### D2: Path loader is the first required proof

The first new proof is `/bin/benchmark` via `FS_CONTEXT.resolve()` and `load_user_app()`.

Reason:

- This validates the biggest semantic gap between Q19B and QEMU.
- It avoids SDMMC while still exercising VFS path lookup, file-backed ELF loading, process setup, stdio, and exit/join.

### D3: Shell/script proof is separate from direct benchmark proof

Shell/script mode is required after direct path loading works.

Reason:

- QEMU starts with `/bin/sh -c init.sh`, not direct embedded benchmark bytes.
- Shell adds separate failure modes: interpreter path, argv/envp, script content, child process behavior, stdio propagation.

### D4: Real rootfs requires hardware witness first

Rootfs mode starts only after SDMMC/block device facts are collected and at least one real block device is available.

Reason:

- Current `axfs-ng::init_filesystems()` panics on empty block devices.
- SDMMC failures are likely clock/reset/pinmux/IRQ/DMA/cache issues, distinct from StarryOS userland startup.

### D5: Use chain-specific evidence labels

Every benchmark run must label startup chain.

Required labels:

- `qemu-rootfs-shell`
- `lichee-embedded-userbench`
- `lichee-memory-root-path`
- `lichee-memory-root-shell`
- `lichee-rootfs-path`

Reason:

- QEMU does not model physical UART line delay.
- Embedded, memory-root and real rootfs results prove different things.

### D6: M0 benchmark evidence is a pre-implementation gate

Before changing Q19C loader/rootfs code, M0 should establish the benchmark output contract and current-state witness commands.

Reason:

- `benchmark.c` is the payload used by both QEMU-style and Lichee-style runs, so hidden parameter drift makes performance comparisons misleading.
- The current C payload already prints `size` and `iters` for TX throughput, but it does not print enough context to compare QEMU and board runs after Q19C adds new startup chains.
- RX currently has correctness evidence (`EAGAIN`) but not fixed-payload board measurement, so RX performance claims must wait for a dedicated witness.

## Requirements Traceability Matrix

| Requirement | Tasks | Coverage | Simplification | Status |
|-------------|-------|----------|----------------|--------|
| R0 Benchmark evidence cleanup | 1.1-1.14, 6.1-6.7 | 100% | RX fixed-payload may start as manual-input before loopback | Covered |
| R1 Preserve embedded regression | 1.1-1.5, 6.1 | 100% | None | Covered |
| R2 Memory-root path loader | 2.1-2.8, 6.2 | 100% | Uses memory-root, not physical rootfs | Covered |
| R3 Shell/script benchmark parity | 3.1-3.7, 6.3 | 100% | Static shell or documented equivalent allowed | Covered |
| R4 SDMMC/block witness | 4.1-4.9, 6.4 | 100% | PIO-first allowed before DMA | Covered |
| R5 Real rootfs benchmark | 5.1-5.8, 6.5 | 100% | ext4 or FAT accepted, selected by feature | Covered |
| R6 Evidence separation | 6.1-6.7 | 100% | None | Covered |

## Verification Plan

### Host Verification

- `openspec validate --changes`
- `openspec validate --specs`
- `git diff -- tests/benchmark.c kernel/resources/benchmark.elf` before Phase 3, proving no implementation changed during planning
- `cargo check --target riscv64gc-unknown-none-elf --features lichee-d1`
- `cargo check --target riscv64gc-unknown-none-elf --features lichee-d1-userbench`
- fullbench feature cargo check for each implemented mode
- QEMU cargo check remains valid with `--features qemu`
- Android boot image inspect records `kernel_size`, `kernel_addr`, `name`, and `DWARF=n`

### Board Verification

Each board run must capture:

- image filename and git revision,
- active mode label,
- boot image size,
- UART/PLIC initialization markers,
- root provider marker,
- user entry path and argv,
- loader used (`load_user_app` vs embedded loader marker),
- benchmark sections and exit code,
- raw serial log location.

### SDMMC Discovery Verification

Part B board probes must capture:

- controller base address and MMIO accessibility,
- clock/reset gate state before and after StarryOS init,
- pinmux state or U-Boot inheritance assumption,
- card detect / command response stage,
- IRQ source and claim/complete behavior if IRQ is used,
- PIO read block result before filesystem mount,
- cache maintenance requirement if DMA is used.

## Failure Handling

| Failure | Required output | Interpretation |
|---------|-----------------|----------------|
| `/bin/benchmark` missing | path, root provider, resolve error | memory-root packaging bug |
| ELF load fails | path, ELF type, interpreter path if present | loader or binary format issue |
| shell missing | `/bin/sh` path and root listing hint | shell packaging issue |
| script fails | script path, argv, exit code | shell/script behavior issue |
| no block device | device list count and SDMMC probe summary | Part B hardware blocker |
| rootfs mount fails | fs type, first block read status, mount error | block/fs integration issue |
| benchmark exits nonzero | exit code and benchmark section reached | userland/app issue |

## Risks / Trade-offs

- **Memory-root can mask rootfs bugs**: mitigated by chain-specific labels and separate rootfs mode.
- **Static shell availability**: if no static shell is available, M2 may use a documented equivalent command runner, but the spec still requires argv/envp/stdio/exit coverage.
- **SDMMC scope**: D1 SDMMC may require clock/reset/pinmux work outside current UART scope. This is isolated in Part B.
- **Boot image growth**: embedding benchmark, shell and scripts may increase image size. Every image build records size.
- **Dynamic ELF dependencies**: dynamic shell or benchmark requires interpreter and libraries in the same root provider; static binaries reduce early risk.

## Completion Criteria

Q19C is complete when:

- Q19B embedded userbench remains runnable.
- Lichee memory-root path mode runs `/bin/benchmark` through `load_user_app()`.
- Lichee shell/script mode runs benchmark from shell or documented equivalent script entry.
- SDMMC/block exploration either produces a working rootfs path or documents a concrete hardware blocker with captured probe data.
- If block rootfs works, benchmark runs from real rootfs and is recorded separately from memory-root data.
- OpenSpec specs, analysis, learned notes and task status contain the final evidence.
