# Boot & Initialization Flow

> Part of StarryOS codebase analysis (branch: `feat/uart-async`)
> See also: [project-overview.md](project-overview.md) | [device-registration.md](device-registration.md) | [tty-console-stack.md](tty-console-stack.md)

---

## 1. Boot Chain

```
axruntime (crates.io — external)
  │
  └── calls main()                            [src/main.rs:12]
      │
      └── starry_kernel::entry::init(&args, &envs)  [kernel/src/entry.rs:20]
          │
          ├── 1. pseudofs::mount_all()         [kernel/src/pseudofs/mod.rs:61]
          │    ├── mount_at("/dev",       dev::new_devfs())     — character/block devices
          │    ├── mount_at("/dev/shm",   MemoryFs::new())      — tmpfs
          │    ├── mount_at("/tmp",       MemoryFs::new())      — tmpfs
          │    ├── mount_at("/proc",      proc::new_procfs())   — process info
          │    └── mount_at("/sys",       MemoryFs::new())      — sysfs stub
          │
          ├── 2. spawn_alarm_task()           — periodic alarm timer
          │
          ├── 3. Resolve init executable path
          │    └── FS_CONTEXT.lock().resolve(&args[0])
          │
          ├── 4. Create user address space
          │    ├── new_user_aspace_empty()
          │    ├── copy_from_kernel(&mut uspace)     — copy kernel mappings
          │    └── load_user_app(&mut uspace, None, args, envs)
          │
          ├── 5. Create init process & task
          │    ├── UserContext::new(entry_vaddr, ustack_top, 0)
          │    ├── new_user_task(name, uctx, 0)
          │    ├── set page table root
          │    ├── Process::new_init(pid) + add_thread(pid)
          │    ├── N_TTY.bind_to(&proc)              — bind controlling terminal
          │    ├── ProcessData::new(...)
          │    └── add_stdio → /dev/console opened 3 times (stdin/out/err)
          │
          ├── 6. Spawn init task
          │    ├── Thread::new(pid, proc_data)
          │    ├── TaskExt attach
          │    └── spawn_task(task)
          │
          └── 7. Wait for init to finish
               └── task.join() → unmount + flush
```

## 2. Entry Point (`src/main.rs`)

```rust
#![no_std]
#![no_main]

pub const CMDLINE: &[&str] = &["/bin/sh", "-c", include_str!("init.sh")];

#[unsafe(no_mangle)]
fn main() {
    let args = CMDLINE.iter().copied().map(str::to_owned).collect::<Vec<_>>();
    let envs = [];
    starry_kernel::entry::init(&args, &envs);
}
```

The init process runs `/bin/sh -c <init.sh>` where `init.sh` is embedded at compile time via `include_str!()`.

## 3. Kernel Init (`kernel/src/entry.rs`)

### mount_all

```rust
pub fn mount_all() -> LinuxResult<()> {
    let fs = FS_CONTEXT.lock();
    mount_at(&fs, "/dev",     dev::new_devfs())?;
    mount_at(&fs, "/dev/shm", tmp::MemoryFs::new())?;
    mount_at(&fs, "/tmp",     tmp::MemoryFs::new())?;
    mount_at(&fs, "/proc",    proc::new_procfs())?;
    mount_at(&fs, "/sys",     tmp::MemoryFs::new())?;
    // ... /sys/class/graphics/fb0/device symlink ...
}
```

`mount_at` resolves or creates the target path, then calls `resolve(path)?.mount(&mount_fs)`.

### Init Process Setup

The init process gets:
- **stdin/stdout/stderr** → `/dev/console` (N_TTY)
- **Controlling terminal** → bound via `N_TTY.bind_to(&proc)`
- **PID** → `task.id().as_u64()`
- **Address space** → user space + kernel mapping + app binary loaded

### add_stdio

```rust
pub fn add_stdio(fd_table: &mut FlattenObjects<...>) -> AxResult<()> {
    let cx = FS_CONTEXT.lock();
    let tty_in  = open("/dev/console", read(true).write(false));
    let tty_out = open("/dev/console", read(false).write(true));
    fd_table.add(tty_in);   // fd 0 — stdin
    fd_table.add(tty_out);  // fd 1 — stdout
    fd_table.add(tty_out);  // fd 2 — stderr
}
```

Note: fd 1 and fd 2 share the same `Arc<File>` instance.

## 4. Boot Flow Diagram

```
Power On
  │
  ▼
axruntime::init()
  │
  ├── Arch setup (trap vectors, PLIC, timer)
  ├── Heap init (axalloc)
  ├── Scheduler init (axtask)
  ├── Device init (axdriver)
  ├── Filesystem init (axfs)
  │
  ▼
main()                                    [src/main.rs]
  │
  ▼
starry_kernel::entry::init()              [kernel/src/entry.rs]
  │
  ├── Mount pseudofs
  ├── Spawn alarm task
  ├── Load init binary
  ├── Create address space
  ├── Bind N_TTY to init process
  ├── Open stdio (/dev/console)
  ├── Spawn init task
  │
  ▼
Init shell running (interactive)
```

## 5. Key Files

| File | Purpose |
|------|---------|
| `src/main.rs` | Binary entry point, defines CMDLINE |
| `kernel/src/entry.rs` | Kernel init: mount, spawn, terminal binding |
| `kernel/src/lib.rs` | Module declarations |
| `kernel/src/pseudofs/mod.rs` | `mount_all()`, filesystem orchestrator |
| `kernel/src/file/mod.rs` | `add_stdio()`, FD_TABLE |
