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

Q19C 原计划把 D1 路径从 embedded payload first 推进到 shell/script capable。2026-07-11 方向更新后，Q19C 收敛为 D1 真板内核态 + 用户态异步 UART 性能参数验证。M0/M1/M2 已覆盖当前目标；Part B 真实块设备/rootfs 不再属于当前规划。

2026-07-11 目标对齐：最终目标是在 D1 真板上测得 async UART 的内核态和用户态性能表现。Shell、SDMMC、block、真实 rootfs 会把目标扩展到 packaging/storage bring-up，不再作为 Q19C gate。若后续需要 storage/rootfs，应重新 propose 独立 change。

## Goals / Non-Goals

**Goals:**

- Lichee 能通过 VFS path 解析运行 `/bin/benchmark`。
- Lichee 能通过 documented equivalent command entry 运行 `/bin/benchmark`（M2 必达目标为 `lichee-memory-root-command`）。true shell path（`/bin/sh -c /init.sh`）仅作为 future optional。
- Q19B embedded benchmark 继续作为 regression baseline。
- Q19C-M0 先让 benchmark 输出包含参数 manifest，并规划真板 RX witness 与 64B 小包优化实验。
- benchmark 证据能区分启动链路：QEMU、Q19B embedded、Q19C memory-root path、Q19C memory-root command。

**Non-Goals:**

- 不把 `qemu` feature 复用到 D1。
- 不把 memory-root 称为真实 rootfs。
- 不把 SDMMC/rootfs 探针和 memory-root command 闭环绑成一个不可拆任务。
- 不在 Q19C 内承诺完成 D1 SDMMC 完整驱动移植。
- 不把 M3/rootfs-probe 或 Q19D SDMMC/rootfs 作为当前规划。
- 不删除或弱化 Q19B 的真板回归路径。

## Architecture

### Runtime Modes

| Mode | Feature name | Log label | Root provider | User entry | Loader | Purpose |
|------|--------------|-----------|---------------|------------|--------|---------|
| smoke | `lichee-d1` | `lichee-smoke` | none | smoke marker | none | D1 boot smoke |
| kbench | `lichee-d1-kbench` | `lichee-kbench` | none | kernel benchmark | none | async UART kernel benchmark |
| embedded | `lichee-d1-userbench` | `lichee-embedded-userbench` | memory root for devfs only | embedded benchmark bytes | `load_embedded_user_app()` | Q19B regression |
| M1 | `lichee-d1-fullbench` | `lichee-memory-root-path` | populated memory root | `/bin/benchmark` | `FS_CONTEXT.resolve()/read()` + eager ELF segment mapping | path-visible benchmark proof |
| M2-command | `lichee-d1-fullbench-command` | `lichee-memory-root-command` | populated memory root | documented equivalent command entry for `/bin/benchmark` | eager path loader | argv/envp/stdio/exit/join proof（true shell deferred as future optional） |
| M3 probe | `lichee-d1-rootfs-probe` | `lichee-rootfs-probe` | SDMMC/block probe only | none | none | historical canceled scope |
| future rootfs | future change | `lichee-rootfs-path` | SDMMC/block rootfs | rootfs `/bin/sh` / `/bin/benchmark` | `load_user_app()` | not in current roadmap |

Implementation for M1 SHOULD use one explicit `lichee-d1-fullbench` feature and one `lichee-fullbench-mem` Makefile target. M2 uses an explicit command feature. M3/rootfs-probe exists as historical work but is no longer an accepted Q19C gate.

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
target_mode=<qemu-rootfs-shell|lichee-embedded-userbench|lichee-d1-fullbench|lichee-d1-fullbench-command>
startup_chain=<embedded|memory-root-path|memory-root-command>
root_provider=<qemu-rootfs|memory-root|none>
timer_source=CLOCK_MONOTONIC
tx_sizes=64,256,1024,4096
tx_iters=100
tx_drain_policy=tcdrain-per-iteration
latency_iters=100
fifo_matrix_sizes=1,15,16,17,31,32,33,48,49
rx_mode=<no-input-eagain|manual-fixed-payload|loopback-fixed-payload>
```

M0 RX work is an evidence plan, not a claim that TX proves RX performance. The minimum board witness remains no-input `EAGAIN`; the next useful witness is a fixed-payload read mode using manual serial injection or loopback if available.

M0 also keeps the 64B D1 result visible: `size=64 / iters=100 / 1.01 KB/s / 8.8% line rate`. Small-packet optimization experiments MUST be reported separately from the baseline:

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
│   └── benchmark
├── init.sh                optional evidence text for M2
├── dev/                  mounted by mount_all()
├── dev/shm/              mounted by mount_all()
├── proc/                 mounted by mount_all()
├── sys/                  mounted by mount_all()
└── tmp/                  mounted by mount_all()
```

M1 uses `/bin/benchmark` directly. M2 uses a documented equivalent command entry. True shell execution is future optional and not required for Q19C.

The memory-root injection path MUST use filesystem-level APIs rather than direct `MemoryNode` writes. Current source already provides `FsContext::write()` in `crates/axfs-ng/src/highlevel/fs.rs`; it creates or truncates the file through `File::create()`. The intended sequence is:

```rust
init_memory_root();
FS_CONTEXT.lock().create_dir("/bin", DIR_PERMISSION)?;
FS_CONTEXT.lock().write("/bin/benchmark", include_bytes!("../resources/benchmark.elf"))?;
// Optional M2 payloads only if available:
FS_CONTEXT.lock().write("/init.sh", b"/bin/benchmark\n")?;
// mount_all() happens after injection; it mounts /dev, /proc, /sys, /tmp and does not replace /bin.
pseudofs::mount_all();
```

After injection, M1 MUST verify `FS_CONTEXT.lock().resolve("/bin/benchmark")` succeeds before loading the process. The benchmark ELF MUST be checked as static/no interpreter (`readelf -l kernel/resources/benchmark.elf | grep INTERP` returns empty), because the normal file loader follows `PT_INTERP` for dynamic ELF.

Performance baseline for M1 is `docs/benchmark-report-async.md` (Q19C-M0 + Q19C.8e). M1 validation does not need to re-prove UART throughput; it needs to prove startup-chain parity up to the current accepted boundary: VFS path resolution/read, process setup, stdio, benchmark sections, and exit code 0.

2026-07-08 真板调试结论：直接使用 `load_user_app()` 的 memory-root/tmpfs lazy file-backed COW 路径能够进入进程并触发 page fault，但在 benchmark main 前以 SIGILL 退出；故障地址 `0x151d4` 反汇编为合法 RV64C `c.ld`，更像取指/懒映射字节问题，而不是 UART、syscall 或 benchmark 编译器问题。M1 接受的实现改为从 `FS_CONTEXT.read("/bin/benchmark")` 读取 VFS 可见文件，再复用 eager ELF segment mapping。lazy file-backed COW 修复独立登记为后续 loader/mm 问题，不阻塞异步 UART 或 M1 fullbench 证据。

M2 is complete through documented equivalent command entry. Busybox may depend on `/proc/self/exe`; if shell work is reopened later, it must be planned separately and must not rewrite Q19C evidence as shell success.

### Part B: SDMMC / Rootfs Probe

Part B is canceled as current Q19C scope. The historical M3/rootfs-probe work may remain as evidence of storage/rootfs blockers, but incomplete M3 output does not block Q19C async UART completion.

```text
Q19C accepted evidence
  -> M0 benchmark manifest / board data cleanup
  -> M1 memory-root path benchmark
  -> M2 memory-root command benchmark
  -> M3/rootfs-probe: canceled current gate
```

Storage/rootfs work must be re-opened as a new change if needed.

## Design Decisions

### D1: Add a new fullbench mode instead of expanding userbench

Q19B `lichee-d1-userbench` remains embedded-loader based. Q19C adds fullbench modes with explicit log labels.

Reason:

- Embedded userbench is the known-good board regression.
- Path loader and command-entry failures need isolation from UART/syscall regressions.
- It prevents accidental success where fullbench silently falls back to embedded bytes.

### D2: Path-visible benchmark is the first required proof

The first new proof is `/bin/benchmark` via `FS_CONTEXT.resolve()/read()` and eager ELF segment mapping.

Reason:

- This validates the biggest semantic gap between Q19B and QEMU.
- It avoids SDMMC while still exercising VFS path lookup/read, process setup, stdio, and exit/join.
- The file-backed lazy COW path exposed a separate D1 memory-root/tmpfs loader bug; that bug is recorded separately and is not evidence against async UART.

### D3: Command-entry proof replaces shell requirement

Command-entry mode is required after direct path loading works. True shell is future optional.

Reason:

- QEMU starts with `/bin/sh -c init.sh`, not direct embedded benchmark bytes.
- D1 has no known-good static `/bin/sh`; adding one would test shell packaging more than async UART.
- Command-entry still covers argv/envp construction, stdio, process spawn/join, exit code, and benchmark sections.

### D4: Real rootfs is out of current scope

Rootfs benchmark is not part of Q19C. It starts only if a future storage/rootfs change is proposed.

Reason:

- Current `axfs-ng::init_filesystems()` panics on empty block devices.
- SDMMC failures are likely clock/reset/pinmux/IRQ/DMA/cache issues, distinct from StarryOS userland startup.
- Block/rootfs work does not improve the current async UART performance conclusion.

### D5: Use chain-specific evidence labels

Every benchmark run must label startup chain.

Required labels:

- `qemu-rootfs-shell`
- `lichee-embedded-userbench`
- `lichee-memory-root-path`
- `lichee-memory-root-command`

Reason:

- QEMU does not model physical UART line delay.
- Embedded, memory-root path and memory-root command results prove different things.

### D6: M0 benchmark evidence is a pre-implementation gate

Before changing Q19C loader/rootfs code, M0 MUST establish the benchmark output contract and current-state witness commands.

Reason:

- `benchmark.c` is the payload used by both QEMU-style and Lichee-style runs, so hidden parameter drift makes performance comparisons misleading.
- The current C payload already prints `size` and `iters` for TX throughput, but it does not print enough context to compare QEMU and board runs after Q19C adds new startup chains.
- RX currently has correctness evidence (`EAGAIN`) but not fixed-payload board measurement, so RX performance claims must wait for a dedicated witness.

### D7: No-shell M2 uses documented command-entry proof

When no known-good static `/bin/sh` is available, M2 uses a documented equivalent command entry instead of blocking on shell packaging.

Reason:

- The current repository has no ready static shell payload for D1 memory-root.
- `load_user_app()` redirects `.sh` paths to `/bin/sh`, and the loader source still records a `/proc/self/exe` FIXME for busybox-style retry behavior.
- Q19C still needs a board witness for argv/envp, `/dev/console` stdio, process spawn/join, and exit code after M1.
- The evidence must not claim shell-launched benchmark success unless `/bin/sh -c /init.sh` actually runs.

Required evidence for the command-entry path:

- `log_label=lichee-memory-root-command`
- `shell_status=SKIPPED: no known-good static /bin/sh` or a more specific blocker
- `equivalent_entry=/bin/benchmark`
- argv and envp summary
- stdio marker for `/dev/console`
- benchmark sections and exit code

## Requirements Traceability Matrix

| Requirement | Tasks | Coverage | Simplification | Status |
|-------------|-------|----------|----------------|--------|
| R0 Benchmark evidence cleanup | 1.1-1.14, 6.1-6.7 | 100% | RX fixed-payload may start as manual-input before loopback | Covered |
| R1 Preserve embedded regression | 1.1-1.5, 6.1 | 100% | None | Covered |
| R2 Memory-root path loader | 2.0a-2.13, 6.2 | 100% | Uses memory-root, not physical rootfs; performance baseline comes from `docs/benchmark-report-async.md` | Covered |
| R3 Command-entry benchmark parity | 3.1-3.10, 6.3 | 100% | Documented equivalent command entry; true shell success not claimed | Covered |
| R4 SDMMC/block probe witness | 4.1-4.6, 6.4 | Historical only | Canceled current gate after 2026-07-11 direction update | Canceled |
| R5 Future real rootfs benchmark | 5.1-5.5, 6.5 | Not current scope | Re-propose only if storage/rootfs becomes a goal | Canceled |
| R6 Evidence separation | 6.1-6.8 | 100% | Board-only rows may be SKIPPED with blocker | Covered |

## Verification Plan

### Host Verification

- `openspec validate --changes`
- `openspec validate --specs`
- `git diff -- tests/benchmark.c kernel/resources/benchmark.elf` before Phase 3, proving no implementation changed during planning
- `cargo check --target riscv64gc-unknown-none-elf --features lichee-d1`
- `cargo check --target riscv64gc-unknown-none-elf --features lichee-d1-userbench`
- fullbench feature cargo check for each implemented mode using the generated D1 platform config, e.g. `AX_CONFIG_PATH=$PWD/.axconfig.toml cargo check --target riscv64gc-unknown-none-elf --features "axfeat/myplat axfeat/bus-mmio lichee-d1-fullbench"` after `make lichee-fullbench-mem` or equivalent D1 `defconfig`
- command mode cargo check for M2; historical probe mode cargo check may be kept as prior evidence but is not a current gate
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

Canceled current scope. If storage/rootfs is reopened later, the new change should capture:

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
| command-entry shell skipped | shell blocker, equivalent entry, argv/envp/stdio markers | accepted M2 simplification, not shell success |
| script fails | script path, argv, exit code | future shell/script issue, not Q19C gate |
| no block device | device list count and SDMMC probe summary | future storage/rootfs blocker |
| rootfs mount fails | fs type, first block read status, mount error | future block/fs integration issue |
| process exits before first benchmark section | exit code, loader stage, first section reached = none | path-loader proof failed |
| benchmark exits nonzero | exit code and benchmark section reached | userland/app issue |

## Risks / Trade-offs

- **Memory-root can mask rootfs bugs**: accepted because rootfs is no longer a Q19C goal; chain-specific labels remain required.
- **Static shell availability**: no static shell is available; M2 uses documented equivalent command entry and does not claim shell success.
- **SDMMC scope**: D1 SDMMC requires clock/reset/pinmux work outside current UART scope. Q19C cancels this path; full driver work requires a new change.
- **Boot image growth**: embedding benchmark, shell and scripts may increase image size. Every image build records size.
- **Dynamic ELF dependencies**: dynamic shell or benchmark requires interpreter and libraries in the same root provider; shell work is future optional.

## Completion Criteria

Q19C is complete when:

- Q19B embedded userbench remains runnable.
- Lichee memory-root path mode runs `/bin/benchmark` through VFS resolve/read and eager ELF segment mapping.
- Lichee memory-root command mode runs benchmark from documented equivalent command entry and records `shell_status=SKIPPED` without claiming shell success.
- M3/rootfs-probe and Q19D SDMMC/rootfs are recorded as canceled current scope, not pending Q19C gates.
- OpenSpec specs, analysis, learned notes and task status contain the final evidence.
