# Project Overview

> Part of StarryOS codebase analysis (branch: `feat/uart-async`)
> See also: [boot-init.md](boot-init.md) | [device-registration.md](device-registration.md) | [tty-console-stack.md](tty-console-stack.md) | [async-io-framework.md](async-io-framework.md) | [syscall-interface.md](syscall-interface.md) | [task-process-model.md](task-process-model.md)

---

## 1. Project Context

StarryOS is a Linux-compatible monolithic OS kernel built on the **ArceOS** unikernel framework. It supports RISC-V 64, LoongArch64, AArch64, and x86_64 (WIP). The kernel is written in Rust (nightly-2026-02-25) and uses a componentized architecture where core OS services are provided by `ax*` crates.

The current branch `feat/uart-async` aims to implement an async high-performance UART serial driver, replacing polling-based console with interrupt-driven async I/O.

## 2. Repository Structure

```
StarryOS/
├── src/main.rs              # Boot entry point (no_std, no_main)
│   └── calls starry_kernel::entry::init()
├── kernel/src/              # Core kernel logic
│   ├── entry.rs             # Kernel init: mount, spawn init process
│   ├── lib.rs               # Module declarations
│   ├── config/              # Architecture-specific constants
│   │   ├── riscv64.rs, aarch64.rs, loongarch64.rs, x86_64.rs
│   │   └── mod.rs
│   ├── file/                # File-like objects (FileLike trait)
│   │   ├── mod.rs           # FileLike trait, FD_TABLE, add_stdio
│   │   ├── fs.rs            # File wrapper (VFS bridge)
│   │   ├── pipe.rs          # Async pipe with PollSet
│   │   ├── event.rs         # Async eventfd
│   │   ├── epoll.rs         # epoll fd
│   │   ├── net.rs           # Network socket wrapper
│   │   ├── pidfd.rs         # pidfd
│   │   └── signalfd.rs      # signalfd
│   ├── mm/                  # Memory management
│   │   ├── mod.rs, access.rs, io.rs, loader.rs
│   │   └── aspace/          # Address space (cow, file, linear, shared)
│   ├── pseudofs/            # Pseudo filesystems
│   │   ├── mod.rs           # mount_all(), mount_at()
│   │   ├── device.rs        # DeviceOps trait, Device struct
│   │   ├── dir.rs, file.rs, fs.rs, tmp.rs, proc.rs
│   │   └── dev/             # /dev device implementations
│   │       ├── mod.rs       # new_devfs(), builder() — all devices
│   │       └── tty/         # TTY subsystem (ntty, ptm, pts, pty, terminal/)
│   ├── syscall/             # System call handlers
│   │   ├── mod.rs           # Main dispatch
│   │   ├── fs/              # File I/O syscalls (io.rs, fd_ops.rs, poll/select/epoll)
│   │   ├── io_mpx/          # poll/select/epoll
│   │   ├── task/            # clone/execve/exit/schedule
│   │   ├── mm/              # brk/mmap/mprotect
│   │   ├── net/             # socket operations
│   │   ├── ipc/             # msg/shm
│   │   ├── sync/            # futex/membarrier
│   │   └── ...              # signal, time, resources, sys
│   ├── task/                # Task/thread/process management
│   │   ├── mod.rs           # Thread, ProcessData, AsThread
│   │   ├── ops.rs           # new_user_task, spawn_alarm_task
│   │   ├── futex.rs, signal.rs, timer.rs, user.rs, stat.rs, resources.rs
│   └── time.rs              # Time-related utilities
├── Cargo.toml               # Workspace root (members: [kernel])
├── Makefile                 # Build: make build/run/debug
├── docs/                    # Design documents
│   ├── analysis/            # This analysis series
│   ├── embassy.md           # Embassy async runtime reference
│   └── x11.md               # X11 guide
└── .claude/docs/            # Development documentation system
```

## 3. Build System

### Makefile Flow

```
make run → make defconfig → make build (cargo build --features qemu) → QEMU launch
make ARCH=loongarch64 run → same flow with different arch
make debug → build + QEMU with debug symbols
```

### Key Build Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `ARCH` | riscv64 | Target architecture |
| `LOG` | warn | Log level |
| `BLK` | y | Block device support |
| `NET` | y | Network support |
| `MEM` | 1G | QEMU memory |
| `APP_FEATURES` | qemu | Platform features |

### Cargo Features

| Feature | Dependencies | Purpose |
|---------|-------------|---------|
| `qemu` | `axfeat/defplat`, `bus-pci`, `display`, `input`, `vsock`, `dev-log` | Default QEMU build |
| `vf2` | `axplat-riscv64-visionfive2`, `driver-sdmmc` | VisionFive 2 board |
| `smp` | `axfeat/smp` | SMP support |
| `dev-log` | (kernel crate) | /dev/log socket |
| `memtrack` | `axfeat/dwarf`, `axalloc/tracking`, `gimli` | Memory tracking debug |
| `input` | `axinput` | Input device support |

## 4. Dependency Architecture (ArceOS Crates)

```
starryos (binary — src/main.rs)
  └── starry-kernel (kernel/)
       ├── axtask 0.3.0-preview.2    — Task scheduler, async future support
       ├── axpoll 0.1                — IoEvents, Pollable, PollSet
       ├── axhal 0.3.0-preview.2     — Hardware abstraction (console, interrupt, PLIC)
       ├── axfs 0.3.0-preview.2      — File system (ext4, VFS)
       ├── axsync 0.3.0-preview.2    — Mutex, SpinLock
       ├── axmm 0.3.0-preview.2      — Memory management
       ├── axalloc 0.3.0-preview.2   — Memory allocator
       ├── axconfig 0.3.0-preview.2  — Config constants
       ├── axruntime 0.3.0-preview.2 — Runtime init (calls main())
       ├── axdriver 0.3.0-preview.2  — Device driver framework
       ├── axdisplay 0.3.0-preview.2 — Display/framebuffer
       ├── axlog 0.3.0-preview.2     — Logging
       ├── axnet 0.3.0-preview.2     — Network stack
       ├── axio 0.3.0-pre.1          — I/O traits
       ├── axerrno 0.2               — Error types
       └── axbacktrace 0.1           — Backtrace support
```

## 5. Key External Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| `ringbuf` | 0.4.8 | Lock-free ring buffer (HeapRb) |
| `axpoll` | 0.1 | PollSet, IoEvents, Pollable |
| `axfs-ng-vfs` | 0.1 | VFS traits (FileNodeOps, DirNodeOps) |
| `starry-process` | 0.2 | Process abstraction |
| `starry-signal` | 0.3 | Signal handling |
| `starry-vm` | 0.3 | User-space memory access |
| `linux-raw-sys` | 0.12 | Linux ABI constants |
| `flatten_objects` | 0.2.4 | Slab-based FD table |
| `scope-local` | — | Per-process scope-local storage |
| `bitflags` | 2.10 | Bitflag types |
| `spin` | 0.10 | Spinlock |
| `ouroboros` | 0.18 | Self-referential structs |

## 6. Configuration (per architecture)

`kernel/src/config/` holds architecture-specific constants:

| File | Constants |
|------|-----------|
| `riscv64.rs` | `USER_HEAP_BASE`, `PAGE_SIZE`, `SIGNAL_TRAMPOLINE`, etc. |
| `aarch64.rs` | Architecture-specific memory layout |
| `loongarch64.rs` | LoongArch memory layout |
| `x86_64.rs` | x86_64 memory layout |
