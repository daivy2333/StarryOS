# TTY/Console Stack

> Part of StarryOS codebase analysis (branch: `feat/uart-async`)
> See also: [device-registration.md](device-registration.md) | [async-io-framework.md](async-io-framework.md)

---

## 1. Console Hardware Access

The lowest layer is provided by `axhal::console` (external crate, crates.io):

| API | Signature | Behavior |
|-----|-----------|----------|
| `read_bytes` | `fn(buf: &mut [u8]) -> usize` | Reads available bytes from UART FIFO (non-blocking, returns 0 if empty) |
| `write_bytes` | `fn(buf: &[u8])` | Writes bytes to UART FIFO (blocking) |
| `irq_num` | `fn() -> Option<u32>` | Returns the UART IRQ number (for RISC-V QEMU virt: usually UART0 IRQ 10) |

These are platform-specific implementations inside the `axhal` crate (e.g., `axhal/src/platform/riscv64_qemu_virt/uart.rs` for the NS16550-compatible UART on QEMU riscv64).

## 2. Console TTY (N_TTY)

The console is exposed to userspace as `/dev/console` via the `N_TTY` singleton:

```rust
// kernel/src/pseudofs/dev/tty/ntty.rs

pub type NTtyDriver = Tty<Console, Console>;

#[derive(Clone, Copy)]
pub struct Console;

impl TtyRead for Console {
    fn read(&mut self, buf: &mut [u8]) -> usize {
        axhal::console::read_bytes(buf)
    }
}

impl TtyWrite for Console {
    fn write(&self, buf: &[u8]) {
        axhal::console::write_bytes(buf);
    }
}

lazy_static! {
    pub static ref N_TTY: Arc<NTtyDriver> = new_n_tty();
}
```

### N_TTY Construction

```rust
fn new_n_tty() -> Arc<NTtyDriver> {
    Tty::new(Arc::default(), TtyConfig {
        reader: Console,
        writer: Console,
        process_mode: if let Some(irq) = axhal::console::irq_num() {
            ProcessMode::External(Box::new(move |waker| register_irq_waker(irq, &waker)))
        } else {
            ProcessMode::Manual  // fallback: polling on read
        },
    })
}
```

Two modes:
- **External** (with IRQ): Spawns a background `tty-reader` task that sleeps until UART IRQ fires
- **Manual** (no IRQ): Polls on every read call (limited functionality)

## 3. TTY Generic Structure

```rust
// kernel/src/pseudofs/dev/tty/mod.rs

pub struct Tty<R, W> {
    this: Weak<Self>,
    terminal: Arc<Terminal>,         // job control, termios, window size
    ldisc: Mutex<LineDiscipline<R, W>>,  // line discipline processor
    writer: W,
    is_ptm: bool,
}
```

### TTY Type Aliases

| Alias | Reader | Writer | Purpose |
|-------|--------|--------|---------|
| `NTtyDriver = Tty<Console, Console>` | `axhal::console` | `axhal::console` | `/dev/console` |
| `PtyDriver = Tty<PtyReader, PtyWriter>` | ringbuf consumer | ringbuf producer | `/dev/pts/N` |
| `CurrentTty` (separate struct) | unreachable | unreachable | `/dev/tty` (CTTY alias) |

### DeviceOps for Tty

```rust
impl<R: TtyRead, W: TtyWrite> DeviceOps for Tty<R, W> {
    fn read_at(&self, buf: &mut [u8], _offset: u64) -> AxResult<usize> {
        block_on(poll_io(&self.terminal.job_control, IoEvents::IN, false, || {
            // Check foreground process group
            if self.is_ptm || self.terminal.job_control.current_in_foreground() {
                self.ldisc.lock().read(buf)  // delegate to line discipline
            } else {
                Err(AxError::WouldBlock)  // background process: block
            }
        }))
    }

    fn write_at(&self, buf: &[u8], _offset: u64) -> AxResult<usize> {
        self.writer.write(buf);
        Ok(buf.len())
    }

    fn as_pollable(&self) -> Option<&dyn Pollable> { Some(self) }
    fn flags(&self) -> NodeFlags { NodeFlags::NON_CACHEABLE | NodeFlags::STREAM }
}
```

Key: `read_at` uses `block_on(poll_io(...))` for async wait; `write_at` writes directly (synchronous).

### ioctl Support

Full `ioctl` support for termios and terminal control:

| ioctl | Purpose |
|-------|---------|
| `TCGETS` / `TCSETS` | Get/set termios attributes |
| `TCGETS2` / `TCSETS2` | Get/set termios2 (extended) |
| `TIOCGPGRP` / `TIOCSPGRP` | Process group management |
| `TIOCGWINSZ` / `TIOCSWINSZ` | Window size |
| `TIOCSCTTY` / `TIOCNOTTY` | Controlling terminal |
| `TIOCGPTN` / `TIOCSPTLCK` | PTY number / lock |

## 4. Line Discipline (ldisc.rs)

The `LineDiscipline` is the data processing layer between hardware and userspace.

### Data Flow

```
UART IRQ
  │
  ▼
register_irq_waker
  │
  ▼
tty-reader task (spawned on boot)
  │
  ├── 1. InputReader::poll()
  │    ├── axhal::console::read_bytes()   → read raw bytes from UART FIFO
  │    ├── Check termios flags (IGNCR, ICRNL, ECHO, ISIG, etc.)
  │    ├── Canonical mode: line editing (VERASE, VKILL, VEOF)
  │    ├── Raw mode: push directly
  │    └── Echo output if enabled
  │
  ├── 2. Push processed bytes → ring buffer (ReadBuf)
  │
  └── 3. Wake PollSet (poll_rx)
       │
       ▼
  Userspace read() → block_on(poll_io(...)) → ldisc.read()
       │
       └── Pop bytes from ring buffer → return to user
```

### ProcessMode

```rust
pub enum ProcessMode {
    /// Process inputs only on call to `read()`.
    /// Fallback: no IRQ support, Ctrl+C won't interrupt running programs.
    Manual,

    /// Spawns dedicated task, relies on external events (IRQ) to wake.
    /// Used by N_TTY console driver.
    External(Box<dyn Fn(Waker) + Send + Sync>),

    /// No processing (PTY master side). Argument is PollSet for data notification.
    None(Arc<PollSet>),
}
```

### External Mode (used by N_TTY)

```rust
ProcessMode::External(register) => {
    let poll_rx = Arc::new(PollSet::new());
    axtask::spawn_with_name("tty-reader", move || {
        block_on(poll_fn(|cx| {
            while reader.poll() {          // drain all available data
                poll_rx.wake();            // wake waiting readers
            }
            poll_tx.register(cx.waker());  // register for next wake
            register(cx.waker().clone());  // register with IRQ waker
            while reader.poll() {
                poll_rx.wake();
            }
            Poll::Pending                  // suspend until next IRQ
        }))
    });
    Processor::External(poll_rx)
}
```

The `tty-reader` task:
1. Drains all available data from hardware
2. Registers waker for next IRQ via `register_irq_waker`
3. Registers itself in `PollSet` for potential wake from write side
4. Returns `Poll::Pending` — scheduler suspends it
5. On next UART IRQ: `register_irq_waker` wakes it → repeats

### LineDiscipline read

```rust
pub fn read(&mut self, buf: &mut [u8]) -> AxResult<usize> {
    // Canonical: need 1 byte (full line). Raw: need VMIN bytes.
    let set = match &self.processor {
        Processor::Manual(_) => None,
        Processor::External(set) => Some(set),
        _ => unreachable!(),
    };
    let pollable = WaitPollable(set);
    block_on(poll_io(&pollable, IoEvents::IN, false, || {
        total_read += self.buf_rx.pop_slice(&mut buf[total_read..]);
        self.poll_tx.wake();  // wake writer (PTY peer)
        (total_read >= vmin).then_some(total_read).ok_or(AxError::WouldBlock)
    }))
}
```

## 5. Pollable for TTY

```rust
impl<R: TtyRead, W: TtyWrite> Pollable for Tty<R, W> {
    fn poll(&self) -> IoEvents {
        let mut events = IoEvents::OUT | self.terminal.job_control.poll();
        if self.is_ptm || events.contains(IoEvents::IN) {
            events.set(IoEvents::IN, self.ldisc.lock().poll_read());
        }
        events
    }

    fn register(&self, context: &mut Context<'_>, events: IoEvents) {
        if !self.is_ptm {
            self.terminal.job_control.register(context, events);
        }
        if events.contains(IoEvents::IN) {
            self.ldisc.lock().register_rx_waker(context.waker());
        }
    }
}
```

## 6. Terminal & Job Control

```rust
pub struct Terminal {
    pub job_control: JobControl,                        // foreground/background groups
    pub window_size: SpinNoPreempt<WindowSize>,         // ws_row, ws_col
    pub termios: SpinNoPreempt<Arc<Termios2>>,          // termios attributes
    pub pty_number: AtomicU32,
}
```

### Job Control

Job control manages foreground/background process groups for signal delivery (SIGTTOU, SIGTTIN).

### Termios

Full termios2 support with:
- Input flags: IGNCR, ICRNL, etc.
- Output flags: OPOST, etc.
- Local flags: ECHO, ISIG, ICANON, ECHOCTL, ECHOK, etc.
- Special characters: VERASE, VKILL, VEOF, VMIN, VTIME, etc.

## 7. Key Files

| File | Role |
|------|------|
| `kernel/src/pseudofs/dev/tty/ntty.rs` | `N_TTY` singleton, `Console` reader/writer |
| `kernel/src/pseudofs/dev/tty/mod.rs` | `Tty<R,W>`, `DeviceOps` + `Pollable` impl |
| `kernel/src/pseudofs/dev/tty/terminal/ldisc.rs` | Line discipline, `InputReader`, `ProcessMode` |
| `kernel/src/pseudofs/dev/tty/terminal/mod.rs` | `Terminal`, `WindowSize` |
| `kernel/src/pseudofs/dev/tty/terminal/termios.rs` | Termios2 definitions |
| `kernel/src/pseudofs/dev/tty/terminal/job.rs` | Job control |
