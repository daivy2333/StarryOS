//! MS14 Iteration 003 (T11) host contract: a cloned user task inherits its
//! parent's cpumask before its FIRST enqueue.
//!
//! The REAL production `kernel/src/syscall/task/clone.rs` is included via
//! `#[path]` and executed against a minimal OS stand-in that replaces only
//! the boundaries `do_clone` touches (task/current/spawn, process data, FD/FS
//! tables, user context, VM writes). No production logic is copied. The fake
//! parent carries a singleton hart-8 mask while a fresh task defaults to the
//! full 16-hart mask, so a missing inheritance is a BEHAVIOR RED (the
//! captured first-enqueue mask is full16 instead of singleton8), never a
//! compile failure.
//!
//! Command (from the repo root):
//!   env RUSTFLAGS='-C linker=<repo>/scripts/cc-nopie.sh' cargo test \
//!     --manifest-path tests/k3-embedded-user-host/Cargo.toml --offline \
//!     --test clone_affinity -- --test-threads=1

// ── log macro stand-ins (the kernel gets them from `#[macro_use] axlog`) ──
macro_rules! debug {
    ($($t:tt)*) => {{
        let _ = format_args!($($t)*);
    }};
}
macro_rules! warn {
    ($($t:tt)*) => {{
        let _ = format_args!($($t)*);
    }};
}

extern crate alloc;
extern crate self as axfs;
extern crate self as axhal;
extern crate self as axtask;
extern crate self as starry_process;
extern crate self as starry_signal;
extern crate self as starry_vm;

use std::cell::Cell;
use std::sync::Arc;
use std::sync::Mutex;

use axerrno::AxError;

// ── shared fake vocabulary (crate root = every extern-crate-self alias) ──

/// `starry_process::Pid`.
pub type Pid = u64;

/// `starry_signal::Signo` stand-in: only construction is needed.
#[derive(Debug, Clone, Copy)]
pub struct Signo(pub u8);

impl Signo {
    pub fn from_repr(v: u8) -> Option<Self> {
        Some(Signo(v))
    }
}

/// `starry_vm::VmMutPtr` stand-in: user-pointer writes are host no-ops.
pub struct VmWriteError;

impl From<VmWriteError> for AxError {
    fn from(_: VmWriteError) -> AxError {
        AxError::InvalidInput
    }
}

pub trait VmMutPtr {
    fn vm_write<T>(self, _value: T) -> Result<(), VmWriteError>;
}

impl<T> VmMutPtr for *mut T {
    fn vm_write<V>(self, _value: V) -> Result<(), VmWriteError> {
        Ok(())
    }
}

/// `axhal::uspace::UserContext` stand-in.
pub mod uspace {
    #[derive(Debug, Clone, Copy)]
    pub struct UserContext {
        pub entry: u64,
        pub sp: u64,
        pub tls: u64,
        pub retval: isize,
        pub page_table_root: u64,
    }

    impl UserContext {
        pub fn new(entry: u64, sp: u64, _arg: u64) -> Self {
            UserContext {
                entry,
                sp,
                tls: 0,
                retval: 0,
                page_table_root: 0,
            }
        }

        pub fn set_sp(&mut self, sp: usize) {
            self.sp = sp as u64;
        }

        pub fn set_tls(&mut self, tls: usize) {
            self.tls = tls as u64;
        }

        pub fn set_retval(&mut self, retval: isize) {
            self.retval = retval;
        }

        pub fn set_page_table_root(&mut self, root: u64) {
            self.page_table_root = root;
        }
    }
}

/// `axtask::AxCpuMask` stand-in: 16-hart capacity, the qualification shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxCpuMask {
    bits: u16,
}

pub const MASK_CAPACITY: usize = 16;

impl AxCpuMask {
    pub fn new() -> Self {
        AxCpuMask { bits: 0 }
    }

    /// The fresh-task default: every hart of the qualification shape.
    pub fn full() -> Self {
        AxCpuMask {
            bits: u16::MAX >> (16 - MASK_CAPACITY),
        }
    }

    pub fn set(&mut self, cpu: usize, on: bool) -> Option<()> {
        if cpu >= MASK_CAPACITY {
            return None;
        }
        if on {
            self.bits |= 1 << cpu;
        } else {
            self.bits &= !(1 << cpu);
        }
        Some(())
    }

    pub fn get(&self, cpu: usize) -> bool {
        cpu < MASK_CAPACITY && self.bits & (1 << cpu) != 0
    }
}

/// `axtask::TaskId`.
#[derive(Debug, Clone, Copy)]
pub struct TaskId(u64);

impl TaskId {
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

/// `axtask::AxTaskExt` stand-in (any thread impl wraps into it).
pub struct AxTaskExt;

impl AxTaskExt {
    pub fn from_impl<T>(_: T) -> Self {
        AxTaskExt
    }
}

/// The fake task: a fresh user task carries the FULL mask (the behavior the
/// production fix must overwrite with the parent's mask before enqueue).
pub struct TaskInner {
    pub id: TaskId,
    pub name: String,
    pub uctx: uspace::UserContext,
    pub mask: Cell<AxCpuMask>,
    ext: Option<AxTaskExt>,
}

impl TaskInner {
    pub fn new_user(name: &str, uctx: uspace::UserContext, _set_child_tid: usize) -> Self {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(100);
        TaskInner {
            id: TaskId(NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)),
            name: String::from(name),
            uctx,
            mask: Cell::new(AxCpuMask::full()),
            ext: None,
        }
    }

    pub fn id(&self) -> TaskId {
        self.id
    }

    pub fn name(&self) -> String {
        self.name.clone()
    }

    pub fn ctx_mut(&mut self) -> &mut uspace::UserContext {
        &mut self.uctx
    }

    pub fn cpumask(&self) -> AxCpuMask {
        self.mask.get()
    }

    pub fn set_cpumask(&self, mask: AxCpuMask) {
        self.mask.set(mask);
    }

    pub fn task_ext_mut(&mut self) -> &mut Option<AxTaskExt> {
        &mut self.ext
    }
}

/// The captured first-enqueue record (name + committed mask).
#[derive(Debug, Clone)]
pub struct AxTaskRef {
    pub name: String,
    pub mask: AxCpuMask,
}

/// `axtask::spawn_task` — the FIRST-ENQUEUE observation point: records the
/// mask the task carries when the generic run queue first sees it.
pub fn spawn_task(task: TaskInner) -> AxTaskRef {
    let record = AxTaskRef {
        name: task.name(),
        mask: task.mask.get(),
    };
    SPAWNED
        .lock()
        .unwrap()
        .push((record.name.clone(), record.mask));
    record
}

static SPAWNED: Mutex<Vec<(String, AxCpuMask)>> = Mutex::new(Vec::new());

/// The fake current task: the parent the production `do_clone` reads.
pub struct CurrentTask {
    pub name: String,
    pub mask: AxCpuMask,
    pub handle: crate::task::ThreadHandle,
}

impl CurrentTask {
    pub fn name(&self) -> String {
        self.name.clone()
    }

    pub fn cpumask(&self) -> AxCpuMask {
        self.mask
    }
}

/// `axtask::current`.
pub fn current() -> CurrentTask {
    let parent = CURRENT.lock().unwrap().clone().expect("fake parent set");
    CurrentTask {
        name: parent.name,
        mask: parent.mask,
        handle: crate::task::ThreadHandle {
            proc_data: parent.proc_data,
        },
    }
}

#[derive(Clone)]
pub struct ParentState {
    pub name: String,
    pub mask: AxCpuMask,
    pub proc_data: Arc<ProcessData>,
}

static CURRENT: Mutex<Option<ParentState>> = Mutex::new(None);

// ── the process/fake-process vocabulary `clone.rs` consumes ─────────────

#[derive(Default, Clone, Copy)]
pub struct FakeScope;

pub struct FakeRw<T>(std::sync::RwLock<T>);

impl<T: Clone> FakeRw<T> {
    pub fn new(value: T) -> Self {
        FakeRw(std::sync::RwLock::new(value))
    }

    pub fn read(&self) -> T {
        self.0.read().unwrap().clone()
    }

    pub fn write(&self) -> T {
        self.0.write().unwrap().clone()
    }
}

pub struct FakeAspace {
    pub root: u64,
}

impl FakeAspace {
    pub fn try_clone(&mut self) -> axerrno::AxResult<Arc<kspin::SpinNoIrq<FakeAspace>>> {
        Ok(Arc::new(kspin::SpinNoIrq::new(FakeAspace {
            root: self.root,
        })))
    }

    pub fn page_table_root(&self) -> u64 {
        self.root
    }
}

#[derive(Default, Clone)]
pub struct FakeSignalActions;

pub struct FakeSignal {
    pub actions: Arc<kspin::SpinNoIrq<FakeSignalActions>>,
}

pub struct FakeProc;

impl FakeProc {
    pub fn fork(&self, _tid: u64) -> FakeProc {
        FakeProc
    }

    pub fn parent(&self) -> Option<Arc<FakeProc>> {
        None
    }

    pub fn add_thread(&self, _tid: u64) {}
}

pub struct ProcessData {
    pub proc: Arc<FakeProc>,
    pub aspace: Arc<kspin::SpinNoIrq<FakeAspace>>,
    pub signal: FakeSignal,
    pub exe_path: FakeRw<String>,
    pub cmdline: FakeRw<Arc<Vec<String>>>,
    pub scope: FakeRw<FakeScope>,
}

impl ProcessData {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        proc: FakeProc,
        exe_path: String,
        cmdline: Arc<Vec<String>>,
        aspace: Arc<kspin::SpinNoIrq<FakeAspace>>,
        _signal_actions: Arc<kspin::SpinNoIrq<FakeSignalActions>>,
        _exit_signal: Option<Signo>,
    ) -> Arc<Self> {
        Arc::new(ProcessData {
            proc: Arc::new(proc),
            aspace,
            signal: FakeSignal {
                actions: Arc::new(kspin::SpinNoIrq::new(FakeSignalActions)),
            },
            exe_path: FakeRw::new(exe_path),
            cmdline: FakeRw::new(cmdline),
            scope: FakeRw::new(FakeScope),
        })
    }

    pub fn set_umask(&self, _mask: u32) {}

    pub fn umask(&self) -> u32 {
        0o022
    }

    pub fn set_heap_top(&self, _top: usize) {}

    pub fn get_heap_top(&self) -> usize {
        0
    }
}

/// The FD-table stand-in: `scope_mut` returns a view carrying the same
/// `clone_from`/`write` shape the production table exposes.
pub struct FakeFdTable;

pub static FD_TABLE: FakeFdTable = FakeFdTable;

pub struct TableScope<'a>(&'a FakeFdTable);

impl<'a> TableScope<'a> {
    pub fn clone_from(&mut self, _src: &FakeFdTable) {}

    pub fn write(&mut self) -> TableWrite<'a> {
        TableWrite(self.0)
    }
}

pub struct TableWrite<'a>(&'a FakeFdTable);

impl<'a> TableWrite<'a> {
    pub fn clone_from(&mut self, _src: &TableRead<'a>) {}
}

pub struct TableRead<'a>(&'a FakeFdTable);

impl FakeFdTable {
    pub fn scope_mut<'a>(&'a self, _scope: &'a mut FakeScope) -> TableScope<'a> {
        TableScope(self)
    }

    pub fn read(&self) -> TableRead<'_> {
        TableRead(self)
    }
}

/// The FS-context stand-in (`scope_mut` + `lock`, as `clone.rs` consumes it).
pub struct FakeFsContext;

pub static FS_CONTEXT: FakeFsContext = FakeFsContext;

pub struct FsScope<'a>(&'a FakeFsContext);

impl<'a> FsScope<'a> {
    pub fn clone_from(&mut self, _src: &FakeFsContext) {}

    pub fn lock(&mut self) -> FsLock<'a> {
        FsLock(self.0)
    }
}

pub struct FsLock<'a>(&'a FakeFsContext);

impl<'a> FsLock<'a> {
    pub fn clone_from(&mut self, _src: &FsLock<'a>) {}
}

impl FakeFsContext {
    pub fn scope_mut<'a>(&'a self, _scope: &'a mut FakeScope) -> FsScope<'a> {
        FsScope(self)
    }

    pub fn lock(&self) -> FsLock<'_> {
        FsLock(self)
    }
}

// ── the `crate::` module surface `clone.rs` imports ─────────────────────

pub mod file {
    use axerrno::AxResult;

    pub use super::FD_TABLE;

    pub trait FileLike {}

    pub struct PidFd;

    impl PidFd {
        pub fn new_thread(_thread: &crate::task::Thread) -> Self {
            PidFd
        }

        pub fn new_process(_proc: &crate::task::ProcessData) -> Self {
            PidFd
        }

        pub fn add_to_fd_table(self, _cloexec: bool) -> AxResult<i32> {
            Ok(3)
        }
    }

    pub fn close_file_like(_fd: i32) -> AxResult<()> {
        Ok(())
    }
}

pub mod mm {
    use axerrno::AxResult;

    /// Any aspace-guard shape: the host fake performs no real copy.
    pub fn copy_from_kernel<A>(_aspace: &mut A) -> AxResult<()> {
        Ok(())
    }
}

pub mod task {
    use alloc::sync::Arc;

    pub use super::ProcessData;
    use super::{AxTaskExt, AxTaskRef, TaskInner, uspace::UserContext};

    /// `crate::task::AsThread`: exposes the current task's process data.
    pub trait AsThread {
        fn as_thread(&self) -> &ThreadHandle;
    }

    impl AsThread for super::CurrentTask {
        fn as_thread(&self) -> &ThreadHandle {
            &self.handle
        }
    }

    pub struct ThreadHandle {
        pub proc_data: Arc<ProcessData>,
    }

    pub struct Thread {
        pub tid: u64,
    }

    impl Thread {
        pub fn new(tid: u64, _proc: Arc<ProcessData>) -> Thread {
            Thread { tid }
        }

        pub fn set_clear_child_tid(&self, _tid: usize) {}
    }

    /// The fresh user task defaults to the FULL mask — the exact behavior
    /// the production fix must overwrite with the parent's mask before the
    /// first enqueue.
    pub fn new_user_task(name: &str, uctx: UserContext, set_child_tid: usize) -> TaskInner {
        TaskInner::new_user(name, uctx, set_child_tid)
    }

    pub fn add_task_to_table(_task: &AxTaskRef) {}
}

// ── the production module under test ────────────────────────────────────

#[path = "../../../kernel/src/syscall/task/clone.rs"]
mod clone;

use axhal::uspace::UserContext;
use clone::{CloneArgs, CloneFlags};

/// A singleton mask for one hart of the qualification shape.
fn singleton(hart: usize) -> AxCpuMask {
    let mut mask = AxCpuMask::new();
    mask.set(hart, true).expect("hart within 16-hart capacity");
    mask
}

fn mask_of(harts: &[usize]) -> AxCpuMask {
    let mut mask = AxCpuMask::new();
    for &h in harts {
        mask.set(h, true).expect("hart within 16-hart capacity");
    }
    mask
}

/// Installs the fake parent (name, mask, minimal process data) and clears
/// the spawn record.
fn set_parent(name: &str, mask: AxCpuMask) {
    let old_aspace = Arc::new(kspin::SpinNoIrq::new(FakeAspace { root: 0xdead }));
    let proc_data = task::ProcessData::new(
        FakeProc,
        String::from("/bin/sh"),
        Arc::new(vec![String::from("/bin/sh")]),
        old_aspace,
        Arc::new(kspin::SpinNoIrq::new(FakeSignalActions)),
        None,
    );
    *CURRENT.lock().unwrap() = Some(ParentState {
        name: String::from(name),
        mask,
        proc_data,
    });
    SPAWNED.lock().unwrap().clear();
}

fn last_spawned() -> AxTaskRef {
    let spawned = SPAWNED.lock().unwrap();
    let (name, mask) = spawned
        .last()
        .expect("do_clone must enqueue the child")
        .clone();
    AxTaskRef { name, mask }
}

fn current_parent_mask() -> AxCpuMask {
    CURRENT.lock().unwrap().as_ref().expect("parent").mask
}

fn fork_uctx() -> UserContext {
    UserContext::new(0x1000, 0x0000_0000_8000_0000, 0)
}

/// S31: a plain fork from the singleton-8 shell lands its child task on the
/// SAME singleton — the first enqueue already carries the parent mask.
#[test]
fn fork_child_inherits_parent_singleton_before_first_enqueue() {
    let parent = singleton(8);
    set_parent("sh", parent);

    let args = CloneArgs {
        flags: CloneFlags::empty(),
        ..Default::default()
    };
    let tid = args.do_clone(&fork_uctx()).expect("fork clone succeeds");
    assert!(tid > 0, "the child tid is published");

    let child = last_spawned();
    assert_eq!(
        child.mask, parent,
        "the child's FIRST enqueue mask must equal the parent singleton \
         (a fresh task defaults to the full mask — inheriting nothing is \
         the bug this test exists to catch)"
    );
    assert_eq!(child.name, "sh", "the child is a distinct task object");
    assert_eq!(
        current_parent_mask(),
        parent,
        "the parent mask must stay untouched"
    );
}

/// S31: the THREAD|VM|SIGHAND path (the shell's helpers, e.g. job control)
/// shares the process data and must inherit the same way.
#[test]
fn thread_child_inherits_parent_singleton_before_first_enqueue() {
    let parent = singleton(8);
    set_parent("sh", parent);

    let args = CloneArgs {
        flags: CloneFlags::THREAD | CloneFlags::VM | CloneFlags::SIGHAND,
        ..Default::default()
    };
    let tid = args.do_clone(&fork_uctx()).expect("thread clone succeeds");
    assert!(tid > 0, "the child tid is published");

    let child = last_spawned();
    assert_eq!(
        child.mask, parent,
        "the THREAD branch publishes through the same spawn point and must \
         carry the parent singleton at first enqueue"
    );
    assert_eq!(current_parent_mask(), parent, "the parent is untouched");
}

/// S31: a non-singleton parent mask is inherited verbatim (the complete set,
/// no narrowing), and the parent keeps its own mask.
#[test]
fn non_singleton_parent_mask_is_inherited_verbatim() {
    let parent = mask_of(&[2, 3]);
    set_parent("wget", parent);

    let args = CloneArgs {
        flags: CloneFlags::empty(),
        ..Default::default()
    };
    let tid = args.do_clone(&fork_uctx()).expect("fork clone succeeds");
    assert!(tid > 0);

    let child = last_spawned();
    assert_eq!(
        child.mask, parent,
        "the full parent set must be inherited without narrowing"
    );
    for hart in 0..16 {
        assert_eq!(
            child.mask.get(hart),
            parent.get(hart),
            "hart {hart} membership must match the parent exactly"
        );
    }
    assert_eq!(current_parent_mask(), parent, "the parent is untouched");
}

