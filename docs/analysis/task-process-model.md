# Task & Process Model

> Part of StarryOS codebase analysis (branch: `feat/uart-async`)
> See also: [boot-init.md](boot-init.md) | [syscall-interface.md](syscall-interface.md)

---

## 1. Key Types

| Type | File | Purpose |
|------|------|---------|
| `TaskInner` | `axtask` crate (external) | The actual schedulable entity — has its own stack, context, and scheduler state |
| `Thread` | `kernel/src/task/mod.rs` | Per-thread kernel data: signals, time tracking, exit notification |
| `ProcessData` | `kernel/src/task/mod.rs` | Per-process shared data: address space, FD table (scope-local), signal actions, futex table |
| `Process` | `starry-process` crate | Process identity (PID, PGID, SID) |
| `AxTaskExt` | `axtask` crate | Extension trait for attaching kernel-specific data to tasks |

## 2. Thread Structure

```rust
pub struct Thread {
    pub proc_data: Arc<ProcessData>,        // Shared process state
    clear_child_tid: AtomicUsize,            // set_tid_address
    robust_list_head: AtomicUsize,           // robust futex list
    pub signal: Arc<ThreadSignalManager>,    // Per-thread signal state
    pub time: AssumeSync<RefCell<TimeManager>>,  // Per-thread timing
    oom_score_adj: AtomicI32,
    pub exit: Arc<AtomicBool>,               // Exit flag
    pub exit_event: Arc<PollSet>,            // Wake waiters on exit
}
```

## 3. Thread ↔ Task Bridge

The connection between `axtask::TaskInner` and kernel `Thread` is via the `TaskExt` trait:

```rust
impl TaskExt for Box<Thread> {
    fn on_enter(&self) {
        // Set per-process scope-local variables
        let scope = self.proc_data.scope.read();
        unsafe { ActiveScope::set(&scope) };
        core::mem::forget(scope);  // prevent scope drop
    }

    fn on_leave(&self) {
        // Restore to global scope
        ActiveScope::set_global();
        unsafe { self.proc_data.scope.force_read_decrement() };
    }
}

impl AsThread for TaskInner {
    fn try_as_thread(&self) -> Option<&Thread> {
        self.task_ext()
            .map(|ext| ext.downcast_ref::<Box<Thread>>().as_ref())
    }
}
```

Every context switch triggers `on_enter`/`on_leave`, which sets/restores the scope-local storage. This is how per-process FD_TABLE switching works.

## 4. ProcessData

```rust
pub struct ProcessData {
    pub proc: Arc<Process>,                      // PID, PGID, SID
    pub exe_path: RwLock<String>,                // Executable path
    pub cmdline: RwLock<Arc<Vec<String>>>,       // Command line arguments
    pub aspace: Arc<Mutex<AddrSpace>>,           // Virtual memory address space
    pub scope: RwLock<Scope>,                    // Resource scope (FD_TABLE, etc.)
    heap_top: AtomicUsize,                       // brk heap position
    pub rlim: RwLock<Rlimits>,                   // Resource limits (RLIMIT_NOFILE, etc.)
    pub child_exit_event: Arc<PollSet>,          // SIGCHLD notification
    pub exit_event: Arc<PollSet>,                // Process exit notification
    pub exit_signal: Option<Signo>,              // Signal on termination
    pub signal: Arc<ProcessSignalManager>,       // Signal actions
    futex_table: Arc<FutexTable>,                // Futex state
    umask: AtomicU32,                            // File creation mask
}
```

## 5. Scope-Local FD Table

The FD table uses `scope_local` crate for automatic per-process isolation:

```rust
scope_local::scope_local! {
    pub static FD_TABLE: Arc<RwLock<FlattenObjects<FileDescriptor, AX_FILE_LIMIT>>> = Arc::default();
}
```

Each process has its own scope, and the `on_enter`/`on_leave` switching ensures:
- Thread A (process 1) runs → FD_TABLE routes to process 1's table
- Thread B (process 2) runs → FD_TABLE routes to process 2's table
- No manual table switching needed in syscall code

## 6. Async Task Spawning

The kernel spawns a `tty-reader` async task during N_TTY initialization:

```rust
// In ldisc.rs (External process mode):
axtask::spawn_with_name("tty-reader", move || {
    block_on(poll_fn(|cx| {
        // Process input data
        while reader.poll() { poll_rx.wake(); }
        // Register for wake events
        poll_tx.register(cx.waker());
        register(cx.waker().clone());  // register_irq_waker
        // Drain again
        while reader.poll() { poll_rx.wake(); }
        Poll::Pending  // suspend until next IRQ
    }))
});
```

## 7. Process Creation (Init)

```rust
// In kernel/src/entry.rs:
let uctx = UserContext::new(entry_vaddr.into(), ustack_top, 0);
let mut task = new_user_task(name, uctx, 0);
task.ctx_mut().set_page_table_root(uspace.page_table_root());

let pid = task.id().as_u64() as Pid;
let proc = Process::new_init(pid);
proc.add_thread(pid);
N_TTY.bind_to(&proc).expect("Failed to bind ntty");

let proc = ProcessData::new(proc, path, ...);
let thr = Thread::new(pid, proc);
*task.task_ext_mut() = Some(AxTaskExt::from_impl(thr));
let task = spawn_task(task);
```

## 8. Exit Events

Both `Thread` and `ProcessData` have `Arc<PollSet>` exit events:
- `Thread::exit_event` — woken when a specific thread exits
- `ProcessData::exit_event` — woken when the process exits
- `ProcessData::child_exit_event` — woken when a child process exits (SIGCHLD)

These are used by `wait4`/`waitpid` syscalls and process reaping.

## 9. Key Files

| File | Role |
|------|------|
| `kernel/src/task/mod.rs` | `Thread`, `ProcessData`, `AsThread` trait |
| `kernel/src/task/ops.rs` | `new_user_task`, `spawn_alarm_task`, `add_task_to_table` |
| `kernel/src/task/user.rs` | User task creation and management |
| `kernel/src/task/timer.rs` | Alarm timer task |
| `kernel/src/task/signal.rs` | Signal delivery |
| `kernel/src/task/futex.rs` | Futex implementation |
| `kernel/src/task/resources.rs` | Rlimits |
| `kernel/src/task/stat.rs` | Task statistics |
| `kernel/src/entry.rs` | Init process creation |
