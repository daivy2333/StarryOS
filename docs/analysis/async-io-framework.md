# Async I/O Framework

> Part of StarryOS codebase analysis (branch: `feat/uart-async`)
> See also: [tty-console-stack.md](tty-console-stack.md) | [syscall-interface.md](syscall-interface.md) | [device-registration.md](device-registration.md)

---

## 1. Core Primitives

The kernel uses `axtask`'s async infrastructure. All ax* crates are v0.3.0-preview.2 from crates.io.

| API | Path | Purpose |
|-----|------|---------|
| `block_on(future)` | `axtask::future` | Synchronously block on an async future (blocks the *current task*, not CPU) |
| `poll_io(pollable, events, nonblocking, op)` | `axtask::future` | Standard async I/O pattern: try op → WouldBlock → register waker → Pending → resume → retry |
| `register_irq_waker(irq, waker)` | `axtask::future` | Connect a hardware interrupt to wake an async task |
| `poll_fn(factory)` | `core::future` | Create a Future from a closure returning `Poll<T>` |
| `future::timeout(timeout, future)` | `axtask::future` | Run a future with a timeout |

## 2. The `poll_io` Pattern

The `poll_io` function is the standard async I/O pattern used **everywhere** in the kernel:

```rust
// Block until data is available
block_on(poll_io(self, IoEvents::IN, false, || {
    let result = try_read_from_hardware();
    if result.is_ok() {
        return Ok(result.unwrap());
    }
    Err(AxError::WouldBlock)  // signal: need to wait
}))
```

### Execution Flow

1. **Call the closure** — try the I/O operation
2. **If Ok(value)** → return immediately
3. **If Err(WouldBlock)**:
   - If `nonblocking=true` → return `Err(WouldBlock)` to caller immediately
   - Otherwise → call `self.register(cx, IoEvents::IN)` (as Pollable) to save the waker
   - Return `Poll::Pending` → task is suspended by scheduler
4. **When woken** (by interrupt or another task) → scheduler polls the future again → retry closure

## 3. Pollable Trait

```rust
pub trait Pollable {
    /// Non-blocking query: what events are ready now?
    fn poll(&self) -> IoEvents;

    /// Register a waker for future event notification.
    fn register(&self, context: &mut Context<'_>, events: IoEvents);
}
```

### IoEvents

```rust
pub struct IoEvents(u32);
// Flags: IN, OUT, ERR, HUP, RDNORM, WRNORM, ALWAYS_POLL, ...
```

## 4. PollSet — Waker Container

`axpoll::PollSet` is a container that stores wakers:

| Method | Behavior |
|--------|----------|
| `new()` | Create empty PollSet (capacity: 64 wakers) |
| `register(waker)` | Store a waker for later wake |
| `wake()` | Wake ALL registered wakers immediately |

### Usage Pattern

```rust
struct MyAsyncResource {
    poll_rx: PollSet,   // wakers waiting for "readable"
    poll_tx: PollSet,   // wakers waiting for "writable"
}

impl Pollable for MyAsyncResource {
    fn poll(&self) -> IoEvents { /* check state */ }

    fn register(&self, context: &mut Context<'_>, events: IoEvents) {
        if events.contains(IoEvents::IN) {
            self.poll_rx.register(context.waker());  // save reader's waker
        }
        if events.contains(IoEvents::OUT) {
            self.poll_tx.register(context.waker());  // save writer's waker
        }
    }
}

// When data becomes available (e.g., from interrupt):
self.poll_rx.wake();  // wake all waiting readers
```

**Important**: `PollSet` stores wakers by reference. Wakers are ephemeral — each poll call may provide a different waker. `register()` replaces old wakers.

## 5. Pipe — Reference Async Implementation

`kernel/src/file/pipe.rs` is the canonical reference for async I/O patterns.

### Structure

```rust
struct Shared {
    buffer: Mutex<HeapRb<u8>>,    // Shared ring buffer
    poll_rx: PollSet,              // Wakers waiting to read
    poll_tx: PollSet,              // Wakers waiting to write
    poll_close: PollSet,           // Wakers for close notification
}

pub struct Pipe {
    read_side: bool,
    shared: Arc<Shared>,
    non_blocking: AtomicBool,
}
```

### Read Flow

```rust
fn read(&self, dst: &mut IoDst) -> AxResult<usize> {
    block_on(poll_io(self, IoEvents::IN, self.nonblocking(), || {
        let read = {
            let cons = self.shared.buffer.lock();
            // try to read from ring buffer
            let count = cons.read(dst)?;
            unsafe { cons.advance_read_index(count) };
            count
        };
        if read > 0 {
            self.shared.poll_tx.wake();  // wake writers (space freed up!)
            Ok(read)
        } else if self.closed() {
            Ok(0)                        // EOF: pipe closed
        } else {
            Err(AxError::WouldBlock)     // no data, need to wait
        }
    }))
}
```

### Write Flow

```rust
fn write(&self, src: &mut IoSrc) -> AxResult<usize> {
    block_on(poll_io(self, IoEvents::OUT, self.nonblocking(), || {
        if self.closed() { return Err(AxError::BrokenPipe); }
        let written = {
            let mut prod = self.shared.buffer.lock();
            // try to write to ring buffer
            let count = src.read(prod.vacant_slices_mut())?;
            unsafe { prod.advance_write_index(count) };
            count
        };
        if written > 0 {
            self.shared.poll_rx.wake();  // wake readers (data available!)
            Ok(written)
        } else {
            Err(AxError::WouldBlock)     // buffer full, need to wait
        }
    }))
}
```

### Pollable for Pipe

```rust
impl Pollable for Pipe {
    fn poll(&self) -> IoEvents {
        let buf = self.shared.buffer.lock();
        let mut events = IoEvents::empty();
        if self.read_side {
            events.set(IoEvents::IN, buf.occupied_len() > 0);
            events.set(IoEvents::HUP, self.closed());
        } else {
            events.set(IoEvents::OUT, buf.vacant_len() > 0);
        }
        events
    }

    fn register(&self, context: &mut Context<'_>, events: IoEvents) {
        if events.contains(IoEvents::IN)  { self.shared.poll_rx.register(context.waker()); }
        if events.contains(IoEvents::OUT) { self.shared.poll_tx.register(context.waker()); }
        self.shared.poll_close.register(context.waker());
    }
}
```

### Pipe Async Pattern Summary

```
Writer:
  1. Try to write to ring buffer
  2. If full → register waker in poll_tx → Pending
  3. When reader consumes → poll_tx.wake()
  4. Retry write

Reader:
  1. Try to read from ring buffer
  2. If empty → register waker in poll_rx → Pending
  3. When writer produces → poll_rx.wake()
  4. Retry read

After successful write → poll_rx.wake()  (notify readers)
After successful read  → poll_tx.wake()  (notify writers)
```

## 6. EventFd — Lightweight Reference

`kernel/src/file/event.rs` provides a simpler pattern:

```rust
struct EventFd {
    count: AtomicU64,
    semaphore: bool,
    poll_rx: PollSet,
    poll_tx: PollSet,
}

impl Pollable for EventFd {
    fn poll(&self) -> IoEvents {
        let count = self.count.load(Ordering::Acquire);
        IoEvents::IN  if count > 0
      | IoEvents::OUT if u64::MAX - 1 > count
    }
    fn register(&self, context: &mut Context<'_>, events: IoEvents) {
        if events.contains(IoEvents::IN)  { self.poll_rx.register(context.waker()); }
        if events.contains(IoEvents::OUT) { self.poll_tx.register(context.waker()); }
    }
}
```

## 7. Interrupt-Driven Async (IRQ → Waker)

The `register_irq_waker` function connects a hardware interrupt to the async task system:

```rust
// In N_TTY initialization:
ProcessMode::External(Box::new(move |waker| register_irq_waker(irq, &waker)))
```

### Flow

```
Hardware: UART receives byte → asserts IRQ line
  │
  ▼
PLIC (RISC-V interrupt controller): routes IRQ to CPU
  │
  ▼
CPU: trap handler → dispatches to UART ISR
  │
  ▼
ISR: 1. Clear interrupt flag
  │    2. AtomicWaker::wake()     ← wake the tty-reader task
  │    3. Return from interrupt
  │
  ▼
axtask scheduler: tty-reader task is now runnable
  │
  ▼
tty-reader task: reads bytes from UART FIFO → processes line discipline
  │              → pushes to ring buffer → wakes userspace reader
```

### ISR Minimal Principle

```
ISR should only do 3 things:
1. Clear interrupt flag
2. Wake the processing task (AtomicWaker::wake or register_irq_waker)
3. Exit immediately
```

All data processing happens in task context, not in ISR.

## 8. block_on — Synchronous ↔ Async Bridge

`block_on` blocks the **current task** (not the CPU) until a future completes:

```rust
pub fn block_on<F: Future>(future: F) -> F::Output;
```

Used in every FileLike::read/write implementation. It allows the synchronous-looking code in `FileLike` methods to use async internally:

```rust
// Synchronous interface:
fn read(&self, dst: &mut IoDst) -> AxResult<usize>

// Internally async:
fn read(&self, dst: &mut IoDst) -> AxResult<usize> {
    block_on(poll_io(self, IoEvents::IN, false, || {
        // ... try I/O ...
        Err(AxError::WouldBlock)  // → suspend via poll_io
    }))
}
```

## 9. Async Pattern Template

To implement async I/O for a new device:

```rust
struct MyDevice {
    rx_wakers: PollSet,     // wakers for readers
    tx_wakers: PollSet,     // wakers for writers
    data: Mutex<InnerState>,
}

impl Pollable for MyDevice {
    fn poll(&self) -> IoEvents {
        let state = self.data.lock();
        // Return current readiness (non-blocking)
        IoEvents::IN  if state.has_data()
      | IoEvents::OUT if state.has_space()
    }

    fn register(&self, context: &mut Context<'_>, events: IoEvents) {
        // Save wakers for later notification
        if events.contains(IoEvents::IN)  { self.rx_wakers.register(context.waker()); }
        if events.contains(IoEvents::OUT) { self.tx_wakers.register(context.waker()); }
    }
}

impl DeviceOps for MyDevice {
    fn read_at(&self, buf: &mut [u8], _offset: u64) -> AxResult<usize> {
        block_on(poll_io(self, IoEvents::IN, false, || {
            self.data.lock().read(buf).ok_or(AxError::WouldBlock)
        }))
    }
    fn write_at(&self, buf: &[u8], _offset: u64) -> AxResult<usize> {
        block_on(poll_io(self, IoEvents::OUT, false, || {
            self.data.lock().write(buf).ok_or(AxError::WouldBlock)
        }))
    }
    fn as_pollable(&self) -> Option<&dyn Pollable> { Some(self) }
    // ...
}

// When data arrives (e.g., from interrupt):
device.rx_wakers.wake();

// When space frees up:
device.tx_wakers.wake();
```

## 10. Key Files

| File | Role |
|------|------|
| `kernel/src/file/pipe.rs` | Reference async implementation (PollSet + poll_io pattern) |
| `kernel/src/file/event.rs` | Lightweight async notification (EventFd) |
| `kernel/src/file/fs.rs` | File wrapper with poll_io for VFS nodes |
| `kernel/src/file/mod.rs` | FileLike trait, Pollable bound |
| `kernel/src/pseudofs/device.rs` | Device struct implements Pollable → delegates to ops |
| `kernel/src/pseudofs/dev/tty/ntty.rs` | register_irq_waker usage in N_TTY |
| `kernel/src/pseudofs/dev/tty/terminal/ldisc.rs` | poll_fn loop for tty-reader task |
| `kernel/src/pseudofs/dev/tty/mod.rs` | Tty Pollable + DeviceOps implementation |
