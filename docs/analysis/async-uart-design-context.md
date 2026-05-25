# Async UART Design Context

> Part of StarryOS codebase analysis (branch: `feat/uart-async`)
> See also: [async-io-framework.md](async-io-framework.md) | [device-registration.md](device-registration.md) | [tty-console-stack.md](tty-console-stack.md) | [syscall-interface.md](syscall-interface.md)

---

## 1. Existing Async Patterns (What's Available)

The codebase already has a mature async I/O infrastructure:

### Core APIs (verified working)

| Pattern | Used By | File |
|---------|---------|------|
| `block_on` + `poll_io` + `Pollable` | Pipe, EventFd, File (VFS), TTY | Multiple |
| `PollSet` (waker storage + wake) | Pipe, EventFd, ldisc | `axpoll` crate |
| `register_irq_waker` (IRQ → task wake) | N_TTY console | `axtask::future` |
| `poll_fn` loop (background async task) | tty-reader in ldisc | `core::future` |
| `block_on` (sync ↔ async bridge) | All FileLike::read/write | `axtask::future` |
| `future::timeout` (timed async wait) | sys_poll/ppoll | `axtask::future` |

### Async → VFS Integration Chain (verified working)

```
DeviceOps (as_pollable → Some(self))
  → Device (impl Pollable → delegates to ops)
    → FileNodeOps (read_at/write_at)
      → Location → FileNode
        → axfs::File
          → File (kernel FileLike wrapper)
            → FD_TABLE
              → poll/select/epoll
```

This chain is currently used by the TTY subsystem and is the correct integration point for async UART devices.

## 2. Current Console Limitations

The existing N_TTY console (`/dev/console`) has several limitations that motivate the async UART work:

| Limitation | Impact |
|------------|--------|
| **Shared with kernel messages** | Console used for both kernel logging and user TTY — mixing concerns |
| **Line discipline overhead** | Even in raw mode, data passes through ldisc processing |
| **Dedicated tty-reader task** | One always-running async task per console — constant context switching |
| **Single instance** | Only one console device, no multi-port support |
| **axhal::console coupling** | Tied to platform-specific axhal implementation — hard to replace |
| **Polling-based I/O** | read_bytes is non-blocking but still polls; no batch/dma support |

## 3. Target Architecture (from ADR-001 to ADR-006)

The asynchronous UART driver (`UartAsyncDriver`) is designed as a new independent device `/dev/ttyS0`:

```
┌─────────────────────────────────────────────────┐
│                  UartAsyncDriver                 │
│                                                   │
│  ┌──────────────┐   ┌──────────────┐              │
│  │   rx_buf     │   │   tx_buf     │              │
│  │  HeapRb<u8>  │   │  HeapRb<u8>  │              │
│  └──────┬───────┘   └──────┬───────┘              │
│         │                  │                       │
│  ┌──────┴───────┐   ┌──────┴───────┐              │
│  │  rx_wakers   │   │  tx_wakers   │              │
│  │   PollSet    │   │   PollSet    │              │
│  └──────────────┘   └──────────────┘              │
│                                                   │
│  ┌─────────────────────────────────────────┐      │
│  │          MMIO Register Access            │      │
│  │   (NS16550: RBR/THR, IER, IIR, LSR,..)  │      │
│  └─────────────────────────────────────────┘      │
└─────────────────────────────────────────────────┘
         │                    │
         ▼                    ▼
    DeviceOps trait     AsyncUart trait
         │                    │
         ▼                    │
    /dev/ttyS0                │
         │                    │
         ▼                    ▼
    Userspace read/write   Future extensions
    poll/select/epoll      (DMA, multi-port)
```

### Data Flow

```
RX Path:
  UART IRQ → PLIC → ISR
    → AtomicWaker::wake()
    → Background copier task wakes up
    → Reads bytes from UART FIFO (RBR)
    → Writes to rx_buf (HeapRb)
    → rx_wakers.wake()
    → Userspace reader woken → reads from rx_buf

TX Path:
  Userspace write → DeviceOps::write_at
    → Writes to tx_buf (HeapRb)
    → tx_wakers.wake() (if copier was waiting)
    → Background copier writes bytes to UART FIFO (THR)
    → Enables TX interrupt if FIFO was full
```

### ISR Minimal Principle

```
ISR (UART interrupt):
  1. Read IIR to determine interrupt source
  2. Clear interrupt flag
  3. Wake the appropriate copier task
  4. Return

All data movement happens in the copier task context.
```

## 4. AsyncUart Trait (Extensibility)

```rust
/// Hardware abstraction for async UART devices.
/// Enables support for different UART hardware (NS16550, DwApbUart, etc.)
trait AsyncUart: Send + Sync {
    /// Initialize the UART hardware
    fn init(&self, params: &UartParams) -> AxResult;

    /// Read bytes from hardware FIFO (non-blocking)
    fn try_read(&self, buf: &mut [u8]) -> usize;

    /// Write bytes to hardware FIFO (non-blocking, returns count written)
    fn try_write(&self, buf: &[u8]) -> usize;

    /// Enable/disable interrupts
    fn enable_rx_intr(&self);
    fn disable_rx_intr(&self);
    fn enable_tx_intr(&self);
    fn disable_tx_intr(&self);
}
```

## 5. Phased Implementation Plan

| Phase | Deliverable | Depends On |
|-------|-------------|-----------|
| P0 | Embassy runtime integration, interrupt framework | — |
| P1 | Ring buffer, UartAsyncDriver, interrupt-driven I/O | P0 |
| P2 | DMA transfer (virtio-console) | P1 |
| P3 | Kernel integration (replace console, syscall, devfs) | P1 |
| P4 | Performance optimization (batch, adaptive, benchmarks) | P2+P3 |

## 6. Design Decisions (from architecture.md)

| Decision | Choice | Rationale |
|----------|--------|-----------|
| **Async runtime** | `axtask::future` + `embassy-sync::AtomicWaker` | Minimal intrusion, reuse existing scheduler |
| **Console relationship** | Independent `/dev/ttyS0` | Isolate risk, don't destabilize console |
| **VFS interface** | `DeviceOps` trait | Consistent with all existing /dev devices |
| **Buffer** | `ringbuf::HeapRb` + `PollSet` | Proven in Pipe, zero extra dependencies |
| **Termios** | Switchable, default raw | High performance + termios on demand |
| **Hardware abstraction** | `AsyncUart` trait | Support multiple UART models |

## 7. Key File Reference

### Kernel Core (FileLike & FD system)

| File | Role |
|------|------|
| `kernel/src/file/mod.rs` | `FileLike` trait, `FD_TABLE`, `add_stdio` |
| `kernel/src/file/fs.rs` | `File` wrapper — VFS ↔ FileLike bridge |
| `kernel/src/file/pipe.rs` | Reference async implementation (PollSet + poll_io) |
| `kernel/src/file/event.rs` | Lightweight async implementation |

### Device Registration

| File | Role |
|------|------|
| `kernel/src/pseudofs/device.rs` | `DeviceOps` trait, `Device` struct |
| `kernel/src/pseudofs/dev/mod.rs` | `new_devfs()`, `builder()` — add new device here |
| `kernel/src/pseudofs/mod.rs` | `mount_all()` — FS initialization |
| `kernel/src/pseudofs/dev/tty/mod.rs` | `Tty<R,W>` — reference DeviceOps implementation |

### TTY/Console (Existing Implementation)

| File | Role |
|------|------|
| `kernel/src/pseudofs/dev/tty/ntty.rs` | `N_TTY`, `Console` — current console driver |
| `kernel/src/pseudofs/dev/tty/terminal/ldisc.rs` | Line discipline, `ProcessMode::External` IRQ pattern |
| `kernel/src/pseudofs/dev/tty/ptm.rs` | `Ptmx` — dynamic device registration pattern |
| `kernel/src/pseudofs/dev/tty/pty.rs` | `PtyDriver` — ringbuf-based Tty implementation |

### Syscall Interface

| File | Role |
|------|------|
| `kernel/src/syscall/fs/io.rs` | `sys_read`, `sys_write` dispatch |
| `kernel/src/syscall/io_mpx/poll.rs` | `sys_poll`, `sys_ppoll` |
| `kernel/src/syscall/io_mpx/epoll.rs` | `sys_epoll_*` |

### Async Infrastructure

| Module | Key APIs |
|--------|----------|
| `axtask::future` | `block_on`, `poll_io`, `register_irq_waker`, `timeout` |
| `axpoll` | `PollSet`, `IoEvents`, `Pollable` |
| `embassy-sync::AtomicWaker` | `wake()` — ISR-safe waker |

### Build System

| File | Role |
|------|------|
| `Cargo.toml` | Workspace root |
| `kernel/Cargo.toml` | Kernel dependencies |
| `Makefile` | Build targets |

### Init & Entry

| File | Role |
|------|------|
| `src/main.rs` | Binary entry point |
| `kernel/src/entry.rs` | Kernel init sequence |
| `kernel/src/lib.rs` | Module declarations |

## 8. Reference Implementations to Study

When implementing async UART, study these files in order:

1. **`kernel/src/file/pipe.rs`** — The canonical async pattern: PollSet + poll_io + block_on
2. **`kernel/src/pseudofs/dev/tty/mod.rs`** —  `DeviceOps` implementation with full ioctl support + as_pollable
3. **`kernel/src/pseudofs/dev/tty/ntty.rs`** —  `register_irq_waker` usage for interrupt-to-task wake
4. **`kernel/src/pseudofs/dev/tty/terminal/ldisc.rs`** — `poll_fn` loop for background processing task
5. **`kernel/src/pseudofs/dev/tty/ptm.rs` + `pty.rs`** — Dynamic device creation pattern with ring buffers
