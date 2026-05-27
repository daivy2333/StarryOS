> ⚠️ 此文档为早期分析，部分内容已过时。
> 最新决策参见 architecture.md ADR-013~ADR-015。

# Syscall Interface & File I/O Dispatch

> Part of StarryOS codebase analysis (branch: `feat/uart-async`)
> See also: [async-io-framework.md](async-io-framework.md) | [device-registration.md](device-registration.md) | [tty-console-stack.md](tty-console-stack.md)

---

## 1. File I/O Syscall Dispatch

### read/write Entry Points

```
User space: read(fd, buf, len)
  │
  ▼
sys_read(fd, buf, len)                         [kernel/src/syscall/fs/io.rs:48]
  │
  └── get_file_like(fd)                         [kernel/src/file/mod.rs:194]
  │   └── FD_TABLE.read().get(fd)
  │       └── returns Arc<dyn FileLike>
  │
  └── file_like.read(&mut VmBytesMut::new(buf, len))
      │
      ├── File::read()      → block_on(poll_io(self, IN, ...))     [file/fs.rs:128]
      ├── Pipe::read()      → block_on(poll_io(self, IN, ...))     [file/pipe.rs:115]
      ├── EventFd::read()   → block_on(poll_io(self, IN, ...))     [file/event.rs:36]
      ├── Socket::read()    → (network stack)
      └── (Device through VFS) → FileNodeOps::read_at → Device::read_at → ops.read_at
```

```
User space: write(fd, buf, len)
  │
  ▼
sys_write(fd, buf, len)                        [kernel/src/syscall/fs/io.rs:63]
  │
  └── get_file_like(fd)
      └── file_like.write(&mut VmBytes::new(buf, len))
          │
          ├── File::write()     → block_on(poll_io(self, OUT, ...))
          ├── Pipe::write()     → block_on(poll_io(self, OUT, ...))
          ├── EventFd::write()  → block_on(poll_io(self, OUT, ...))
          └── ...
```

## 2. FileLike Trait (Core Abstraction)

All fd-backed objects implement `FileLike`:

```rust
pub trait FileLike: Pollable + DowncastSync {
    fn read(&self, _dst: &mut IoDst) -> AxResult<usize> { Err(AxError::InvalidInput) }
    fn write(&self, _src: &mut IoSrc) -> AxResult<usize> { Err(AxError::InvalidInput) }
    fn stat(&self) -> AxResult<Kstat> { Ok(Kstat::default()) }
    fn path(&self) -> Cow<'_, str>;
    fn ioctl(&self, _cmd: u32, _arg: usize) -> AxResult<usize> { Err(AxError::NotATty) }
    fn nonblocking(&self) -> bool { false }
    fn set_nonblocking(&self, _nonblocking: bool) -> AxResult { Ok(()) }

    // Utility methods:
    fn from_fd(fd: c_int) -> AxResult<Arc<Self>>;       // downcast from FD_TABLE
    fn add_to_fd_table(self, cloexec: bool) -> AxResult<c_int>;
}
```

**Key**: `FileLike: Pollable`, so every fd object is also pollable. This is how poll/select/epoll works — they call `fd.poll()` through the `Pollable` interface.

### Implementations

| Type | FileLike impl | File |
|------|---------------|------|
| `File` (VFS file/dir) | Reads via `axfs::File`, uses `poll_io` for non-blocking | `file/fs.rs` |
| `Pipe` | Reads from `HeapRb`, uses `poll_io` + `PollSet` | `file/pipe.rs` |
| `EventFd` | Atomic counter, `poll_io` + `PollSet` | `file/event.rs` |
| `Socket` | Network I/O | `file/net.rs` |
| `EpollFile` | epoll instance | `file/epoll.rs` |
| `SignalFd` | Signal notifications | `file/signalfd.rs` |
| `PidFd` | Process handle | `file/pidfd.rs` |

**Important**: Device nodes (`/dev/*`) are accessed via `File` — they don't implement `FileLike` directly. Instead, `File` wraps the VFS `Location` and delegates to `FileNodeOps::read_at`, which ends up in `Device::read_at` → `DeviceOps::read_at`.

## 3. File Descriptor Table

```rust
scope_local::scope_local! {
    pub static FD_TABLE: Arc<RwLock<FlattenObjects<FileDescriptor, AX_FILE_LIMIT>>>;
}

pub struct FileDescriptor {
    pub inner: Arc<dyn FileLike>,
    pub cloexec: bool,
}

pub fn get_file_like(fd: c_int) -> AxResult<Arc<dyn FileLike>> {
    FD_TABLE.read().get(fd as usize).map(|fd| fd.inner.clone())
        .ok_or(AxError::BadFileDescriptor)
}
```

The FD table is a **scope-local** (per-process) slab allocator (`FlattenObjects`). Each process gets its own `FD_TABLE` instance automatically through the scope-local mechanism.

## 4. File — VFS ↔ FileLike Bridge

The `File` struct bridges VFS file operations to the `FileLike` interface:

```rust
pub struct File {
    inner: axfs::File,
    nonblock: AtomicBool,         // O_NONBLOCK flag
}

impl FileLike for File {
    fn read(&self, dst: &mut IoDst) -> AxResult<usize> {
        let inner = self.inner();
        if likely(self.is_blocking()) {
            inner.read(dst)       // Regular files: direct read (no async)
        } else {
            block_on(poll_io(self, IoEvents::IN, self.nonblocking(), || {
                inner.read(&mut *dst)
            }))
        }
    }
}
```

**Key distinction**: "blocking" here refers to the VFS node flag `BLOCKING`:
- Regular files (ext4) → `BLOCKING` = true → direct I/O, no async
- Device nodes (`/dev/console`, pipes via `File`) → `BLOCKING` = false → goes through `poll_io`

This means all device I/O uses the async path even through the `File` wrapper.

## 5. Poll/Select/Epoll

### sys_poll / sys_ppoll

```rust
pub fn sys_ppoll(fds, nfds, timeout, sigmask, sigsetsize) -> AxResult<isize> {
    let fds = fds.get_as_mut_slice(nfds);
    do_poll(fds, timeout, sigmask)
}

fn do_poll(poll_fds, timeout, sigmask) -> AxResult<isize> {
    // Build list of (Arc<dyn FileLike>, IoEvents) pairs
    let fds = FdPollSet(pairs);

    with_blocked_signals(sigmask, || {
        block_on(future::timeout(timeout, poll_io(&fds, IoEvents::empty(), false, || {
            let mut res = 0usize;
            for ((fd, events), revents) in fds.0.iter().zip(revents.iter_mut()) {
                let result = fd.poll();                   // ← Pollable::poll()
                result &= *events;
                **revents = result.bits() as _;
                if **revents != 0 { res += 1; }
            }
            if res > 0 { Ok(res as _) }
            else { Err(AxError::WouldBlock) }
        })))
    })
}
```

### FdPollSet

```rust
struct FdPollSet(Vec<(Arc<dyn FileLike>, IoEvents)>);

impl Pollable for FdPollSet {
    fn poll(&self) -> IoEvents { IoEvents::empty() }  // unused

    fn register(&self, context: &mut Context<'_>, events: IoEvents) {
        for (fd, events) in &self.0 {
            fd.register(context, *events);  // register waker on EACH fd
        }
    }
}
```

Each fd in the poll set gets the same waker. When any fd becomes ready, it wakes the waker, and `poll_io` retries.

### sys_epoll

Epoll (`kernel/src/syscall/io_mpx/epoll.rs`) follows the same pattern but with an epoll instance (`EpollFile` in `file/epoll.rs`) that maintains its own interest list and ready list.

## 6. VFS → Device → Userspace Chain

For example, reading from `/dev/ttyS0` (once implemented):

```
User: read(fd, buf, 256)
  │
  ▼
sys_read(fd, buf, 256)
  │
  ▼
get_file_like(fd) → Arc<dyn FileLike> → downcast to File
  │
  ▼
File::read → block_on(poll_io(self, IN, false, || { inner.read(&mut dst) }))
  │
  ▼
axfs::File::read → VFS → Location → FileNode::read_at
  │
  ▼
Device::read_at → ops.read_at(buf, offset)
  │
  ▼
DeviceOps::read_at → your device's implementation
```

## 7. Key Files

| File | Role |
|------|------|
| `kernel/src/syscall/fs/io.rs` | `sys_read`, `sys_write`, `sys_readv`, `sys_writev`, `sys_sendfile`, etc. |
| `kernel/src/syscall/fs/fd_ops.rs` | `sys_close`, `sys_dup`, `sys_fcntl`, etc. |
| `kernel/src/syscall/fs/mod.rs` | FS syscall dispatch |
| `kernel/src/syscall/io_mpx/poll.rs` | `sys_poll`, `sys_ppoll` |
| `kernel/src/syscall/io_mpx/epoll.rs` | `sys_epoll_create`, `sys_epoll_ctl`, `sys_epoll_wait` |
| `kernel/src/syscall/io_mpx/select.rs` | `sys_select` |
| `kernel/src/syscall/io_mpx/mod.rs` | `FdPollSet` |
| `kernel/src/syscall/mod.rs` | Main syscall dispatch table |
| `kernel/src/file/mod.rs` | `FileLike` trait, `FD_TABLE` |
| `kernel/src/file/fs.rs` | `File` wrapper (VFS ↔ FileLike bridge) |
