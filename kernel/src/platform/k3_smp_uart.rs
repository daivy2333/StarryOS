//! MS14 Iteration 002 (T7): the `k3-smp-uart` role-composition seam and the
//! two SMP boot entries.
//!
//! Pure, host-testable composition over the shared background-role placement
//! policy (`drivers/placement.rs`, design D4/D5): the RX/TX copier singletons
//! follow the shared policy's actual return, the alarm deadline task rides
//! the boot-hart anchor (the main task's hart — no third fixed hart), and
//! the embedded shell user task is the fixed A100-cluster hart 8. Fail-closed
//! contract (A9): an invalid schedulable set, colliding copier singletons,
//! or a missing/misplaced shell hart is an error the caller parks on — no
//! fallback, no invented topology. Every hart in a successful result comes
//! from the shared policy's actual return or a named constant, never a
//! hardcode.
//!
//! The kernel-only submodules hold the two boot entries: the shell sub-form
//! mounts the memory root and spawns the embedded `k3-shell` ELF on the
//! shell singleton (the tty reader is the sole RX consumer); the raw
//! sub-form reuses the MS17 data-plane payload verbatim as the sole RX
//! consumer. Both pin the main task to the boot singleton before its first
//! TimerFuture and observe the copiers' actual poll harts after
//! `start_copiers`.

// Single compiled instance of the shared policy: in kernel builds the
// `drivers` tree is present (the k3-smp-uart closure implies `k3-uart`), so
// the policy is re-exported from there; the host harness (no features)
// includes the same file verbatim as a child module.
#[cfg(feature = "k3-smp-uart")]
pub use crate::drivers::placement;
#[cfg(not(feature = "k3-smp-uart"))]
#[path = "../drivers/placement.rs"]
pub mod placement;

/// The fixed shell user-task hart (design D5: the A100-cluster application
/// hart; the qualification shape is SMP=16, so the shell sub-form validates
/// membership in the published schedulable set and parks otherwise).
pub const SHELL_HART: usize = 8;

/// The composition result: the two UART copier singletons, in shared policy
/// role order (RX = role index 0, TX = role index 1), plus the alarm
/// deadline task's singleton.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SmpUartRoles {
    /// The RX copier singleton hart (`place_roles().uart_rx`).
    pub uart_rx: usize,
    /// The TX copier singleton hart (`place_roles().uart_tx`).
    pub uart_tx: usize,
    /// The alarm deadline task's singleton hart (the boot-hart anchor).
    pub alarm_hart: usize,
}

/// Fail-closed composition errors (A9); the caller parks on every variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmpUartRoleError {
    /// The shared policy rejected the schedulable set (empty, unsorted,
    /// duplicated, or a hart id `>= MAX_CPU_NUM`).
    InvalidSchedulable,
    /// The policy placed both copier roles on one hart (only possible below
    /// two schedulable harts; this mode requires distinct singletons).
    CopierCollide,
    /// [`SHELL_HART`] is not in the schedulable set (fewer than nine harts).
    ShellHartMissing,
    /// [`SHELL_HART`] collided with a copier singleton or the alarm hart.
    ShellHartCollide,
}

/// Composes the `k3-smp-uart` singletons from the published schedulable list
/// and the boot-hart anchor. The copier roles follow the shared policy
/// (`place_roles`); the alarm deadline task rides the boot-hart anchor (the
/// same hart runs the boot's own TimerFutures, so nothing new is fixed).
///
/// The anchor is consumed exactly as `place_roles` defines it: an anchor
/// outside the set is equivalent to index 0 for the copier roles (D4, no
/// topology invention) and stays literal for [`SmpUartRoles::alarm_hart`].
/// The boot always passes the running hart, so the alarm spawn's own
/// schedulable validation never sees an out-of-set anchor in production.
pub fn place_smp_uart_roles(
    schedulable: &[usize],
    anchor: usize,
) -> Result<SmpUartRoles, SmpUartRoleError> {
    let p =
        placement::place_roles(schedulable, anchor).ok_or(SmpUartRoleError::InvalidSchedulable)?;
    if p.uart_rx == p.uart_tx {
        return Err(SmpUartRoleError::CopierCollide);
    }
    Ok(SmpUartRoles {
        uart_rx: p.uart_rx,
        uart_tx: p.uart_tx,
        alarm_hart: anchor,
    })
}

/// Resolves the fixed shell hart for the shell sub-form (the raw sub-form
/// never calls this): fail closed when [`SHELL_HART`] is not schedulable or
/// shares a hart with a copier singleton or the alarm task.
pub fn shell_hart_for(
    schedulable: &[usize],
    roles: &SmpUartRoles,
) -> Result<usize, SmpUartRoleError> {
    if !schedulable.contains(&SHELL_HART) {
        return Err(SmpUartRoleError::ShellHartMissing);
    }
    if SHELL_HART == roles.uart_rx
        || SHELL_HART == roles.uart_tx
        || SHELL_HART == roles.alarm_hart
    {
        return Err(SmpUartRoleError::ShellHartCollide);
    }
    Ok(SHELL_HART)
}

// ── kernel-only boot support (never compiled by the host harness) ───────

/// Absolute supervision deadline for the startup waits (the boot timer is
/// alive on every k3-smp shape).
#[cfg(all(target_arch = "riscv64", feature = "k3-smp-uart"))]
const START_DEADLINE: core::time::Duration = core::time::Duration::from_secs(5);

#[cfg(all(target_arch = "riscv64", feature = "k3-smp-uart"))]
mod boot_support {
    use alloc::vec::Vec;

    use axtask::future::{block_on, sleep, timeout};
    use axtask::AxCpuMask;

    use super::START_DEADLINE;
    use crate::drivers::uart_smp_snapshot::{self, Copier};

    /// A singleton CPU mask for one hart (fail at compile capacity only;
    /// schedulable membership is validated by the spawn/start seams).
    pub(super) fn singleton(hart: usize) -> AxCpuMask {
        let mut mask = AxCpuMask::new();
        mask.set(hart, true).expect("hart index within mask capacity");
        mask
    }

    /// The ordered published-schedulable hart ids — the same construction as
    /// `start_copiers` uses, so the boot seam and `start_copiers` consume the
    /// identical input and the deterministic policy guarantees they agree.
    pub(super) fn published_schedulable() -> Vec<usize> {
        let sched = axtask::schedulable_cpu_mask();
        let mut cpus = Vec::new();
        for hart in 0..super::placement::MAX_CPU_NUM {
            if sched.get(hart) {
                cpus.push(hart);
            }
        }
        cpus
    }

    /// Fails the boot, reports, and parks. The recovery path is external
    /// RESET; no retry, no fallback.
    pub(super) fn fault(out: &dyn Fn(&str), stage: &str, detail: &str) -> ! {
        out(&alloc::format!("[ms14] uart fault stage={stage} detail={detail}"));
        loop {
            riscv::asm::wfi();
        }
    }

    /// Bounded wait for both copiers' first real poll; `None` on deadline.
    /// The actual-hart observation (A9) rides the same per-poll recording the
    /// copier futures already perform (`record_harts`).
    pub(super) fn wait_copier_harts() -> Option<(usize, usize)> {
        block_on(timeout(Some(START_DEADLINE), async {
            loop {
                if let (Some(rx), Some(tx)) = (
                    uart_smp_snapshot::copier_last_poll(Copier::Rx),
                    uart_smp_snapshot::copier_last_poll(Copier::Tx),
                ) {
                    return Some((rx, tx));
                }
                sleep(core::time::Duration::from_millis(10)).await;
            }
        }))
        .ok()
        .flatten()
    }
}

// ── shell sub-form boot (tty + embedded k3-shell on the fixed harts) ────

#[cfg(all(
    target_arch = "riscv64",
    feature = "k3-smp-uart",
    not(feature = "k3-smp-uart-raw")
))]
mod shell_boot {
    use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use core::task::Context;
    use core::time::Duration;

    use alloc::borrow::Cow;
    use alloc::format;
    use alloc::string::String;
    use alloc::sync::Arc;
    use alloc::vec::Vec;

    use axerrno::{AxError, AxResult};
    use axhal::uspace::UserContext;
    use axpoll::{IoEvents, Pollable};
    use axsync::Mutex;
    use axtask::future::{block_on, sleep, timeout};
    use axtask::{AxTaskExt, set_current_affinity, spawn_task_with_affinity};
    use flatten_objects::FlattenObjects;
    use starry_process::{Pid, Process};

    use super::boot_support::{fault, published_schedulable, singleton, wait_copier_harts};
    use super::placement;
    use super::shell_hart_for;
    use super::{place_smp_uart_roles, SHELL_HART};
    use crate::drivers::ASYNC_TTY;
    use crate::drivers::k3_uart;
    use crate::drivers::uart_init;
    use crate::file::{
        FD_TABLE, File, FileDescriptor, FileLike, IoDst, IoSrc, Kstat, StdioTable, publish_stdio,
    };
    use crate::mm::{copy_from_kernel, load_embedded_elf_into_aspace, new_user_aspace_empty};
    use crate::pseudofs;
    use crate::task::{
        AX_FILE_LIMIT, ProcessData, Thread, add_task_to_table, new_user_task,
        spawn_alarm_task_with_affinity,
    };

    /// The shell's one-time first-write record (A9): the kernel-side record
    /// point for the actual shell hart. The published stdout/stderr endpoint
    /// is this delegate in the k3-smp-uart shell sub-form only; every call
    /// delegates verbatim to the wrapped console file, the shared write path
    /// is untouched, and the first write stores the executing hart.
    struct ShellFirstWriteFile {
        inner: Arc<File>,
        written: AtomicBool,
    }

    static SHELL_FIRST_WRITE_HART: AtomicUsize = AtomicUsize::new(usize::MAX);

    impl ShellFirstWriteFile {
        fn new(inner: Arc<File>) -> Self {
            Self {
                inner,
                written: AtomicBool::new(false),
            }
        }
    }

    impl FileLike for ShellFirstWriteFile {
        fn read(&self, dst: &mut IoDst) -> AxResult<usize> {
            self.inner.read(dst)
        }

        fn write(&self, src: &mut IoSrc) -> AxResult<usize> {
            if !self.written.swap(true, Ordering::AcqRel) {
                SHELL_FIRST_WRITE_HART.store(axhal::percpu::this_cpu_id(), Ordering::Release);
            }
            self.inner.write(src)
        }

        fn stat(&self) -> AxResult<Kstat> {
            self.inner.stat()
        }

        fn ioctl(&self, cmd: u32, arg: usize) -> AxResult<usize> {
            self.inner.ioctl(cmd, arg)
        }

        fn set_nonblocking(&self, flag: bool) -> AxResult {
            self.inner.set_nonblocking(flag)
        }

        fn nonblocking(&self) -> bool {
            self.inner.nonblocking()
        }

        fn path(&self) -> Cow<'_, str> {
            self.inner.path()
        }
    }

    impl Pollable for ShellFirstWriteFile {
        fn poll(&self) -> IoEvents {
            self.inner.poll()
        }

        fn register(&self, context: &mut Context<'_>, events: IoEvents) {
            self.inner.register(context, events);
        }
    }

    /// New-form-only stdio publisher: the same publish contract over an
    /// `Arc<dyn FileLike>`-typed table view, so the stdout/stderr endpoint
    /// can be the first-write witness. The shared table impl (typed
    /// `Arc<File>`) stays untouched; the order, empty-table assert and
    /// failure mapping are `publish_stdio`'s own.
    struct ShellStdioTable<'a>(&'a mut FlattenObjects<FileDescriptor, AX_FILE_LIMIT>);

    impl StdioTable for ShellStdioTable<'_> {
        type File = Arc<dyn FileLike>;

        fn is_empty(&self) -> bool {
            self.0.count() == 0
        }

        fn add(&mut self, file: Arc<dyn FileLike>) -> AxResult<()> {
            // The inherent FlattenObjects::add takes precedence (same
            // failure mapping as the shared table impl).
            self.0
                .add(FileDescriptor {
                    inner: file,
                    cloexec: false,
                })
                .map(|_| ())
                .map_err(|_| AxError::TooManyOpenFiles)
        }
    }

    /// The k3-smp-uart shell sub-form boot. The order follows the MS17
    /// stdio branch with the SMP insertions the Plan Context mandates: the
    /// main task pins to the boot singleton before its first TimerFuture,
    /// the copier actual-hart observation waits bounded after
    /// `start_copiers`, the alarm task is the affinity variant on the boot
    /// singleton, and the shell user task spawns with the shell singleton;
    /// the first-write observation line proves the shell's actual hart
    /// before the join. Everything fallible before the process commit point
    /// (`Process::new_init`) panics at its layer; nothing is published.
    pub fn run_smp_uart_shell_boot() -> ! {
        let boot = axhal::percpu::this_cpu_id();
        // Pre-init lines ride the runtime console (the async driver is not
        // up yet); post-init lines ride the serialized async writer.
        let pre = |l: &str| ax_println!("{}", l);
        let diag = |l: &str| k3_uart::diag_line(l);

        // ── Placement: the shared policy decides, fail closed ──
        let schedulable = published_schedulable();
        let roles = match place_smp_uart_roles(&schedulable, boot) {
            Ok(r) => r,
            Err(e) => fault(&pre, "placement", &format!("{e:?}")),
        };
        let shell_hart = match shell_hart_for(&schedulable, &roles) {
            Ok(h) => h,
            Err(e) => fault(&pre, "shell-hart", &format!("{e:?}")),
        };
        pre(&format!(
            "[ms14] uart mode=k3-smp-uart begin harts={} boot_hart={boot} form=shell",
            axhal::cpu_num()
        ));
        pre(&format!(
            "[ms14] uart placement rx={} tx={} alarm={} shell={shell_hart} harts={} anchor={boot}",
            roles.uart_rx,
            roles.uart_tx,
            roles.alarm_hart,
            schedulable.len()
        ));

        uart_init::init_uart_hardware();

        // Main task singleton before any TimerFuture (Invariant 3); the pin
        // contains the current hart, so no migration happens.
        if !set_current_affinity(singleton(boot)) {
            fault(&pre, "main-pin", "main task boot-hart pin rejected");
        }

        // SAFETY: this boot path runs once; no other ring producer (no
        // benchmark in this mode) ran before the copiers start.
        unsafe { uart_init::start_copiers() };

        // Bounded copier actual-hart observation (A9).
        let Some((rx_actual, tx_actual)) = wait_copier_harts() else {
            fault(
                &diag,
                "copier-first-poll",
                "copiers never polled within the startup bound",
            );
        };
        diag(&format!(
            "[ms14] uart copiers pinned rx={} tx={} actual rx={rx_actual} tx={tx_actual}",
            roles.uart_rx, roles.uart_tx
        ));

        // Memory root + mounts: the devfs console construction is the
        // ASYNC_TTY construction point, making the tty reader the sole RX
        // consumer (Invariant 2).
        pseudofs::init_memory_root();
        pseudofs::mount_all().expect("Failed to mount pseudofs");

        // Alarm deadline task: the affinity variant on the boot singleton
        // (A9 start line).
        if !spawn_alarm_task_with_affinity(singleton(roles.alarm_hart)) {
            fault(&diag, "alarm-spawn", "affinity-committed alarm spawn rejected");
        }
        diag(&format!("[ms14] uart alarm spawned hart={}", roles.alarm_hart));

        // Recoverable phase (before the commit point): an open or load
        // failure drops its objects and nothing is published.
        let stdio_files = crate::file::open_stdio_files()
            .unwrap_or_else(|e| panic!("[ms14] uart fault stage=stdio-open err={e:?}"));

        #[repr(align(8))]
        struct AlignedBytes<const N: usize>([u8; N]);

        const PAYLOAD_ELF_LEN: usize = include_bytes!("../../resources/k3-shell.elf").len();
        static EMBEDDED_PAYLOAD: AlignedBytes<PAYLOAD_ELF_LEN> =
            AlignedBytes(*include_bytes!("../../resources/k3-shell.elf"));

        let mut uspace = new_user_aspace_empty()
            .and_then(|mut it| {
                copy_from_kernel(&mut it)?;
                Ok(it)
            })
            .expect("Failed to create user address space");

        let payload_name = "k3-shell";
        let payload_args: Vec<String> = [String::from(payload_name)].into_iter().collect();
        let (entry_vaddr, ustack_top) =
            load_embedded_elf_into_aspace(&mut uspace, &EMBEDDED_PAYLOAD.0, &payload_args, &[])
                .unwrap_or_else(|e| panic!("[ms14] uart fault stage=load err={e:?}"));

        // Commit point: from here on the process exists; invariant failures
        // terminate to reset (no rollback of new_init is attempted).
        let uctx = UserContext::new(entry_vaddr.into(), ustack_top, 0);
        let mut task = new_user_task(payload_name, uctx, 0);
        task.ctx_mut().set_page_table_root(uspace.page_table_root());

        let pid = task.id().as_u64() as Pid;
        let proc = Process::new_init(pid);
        proc.add_thread(pid);
        ASYNC_TTY.bind_to(&proc).expect("Failed to bind async tty");

        let proc = ProcessData::new(
            proc,
            String::from(payload_name),
            Arc::new(payload_args),
            Arc::new(Mutex::new(uspace)),
            Arc::default(),
            None,
        );

        {
            let mut scope = proc.scope.write();
            let table_scope = FD_TABLE.scope_mut(&mut scope);
            let mut table = table_scope.write();
            let output = Arc::new(ShellFirstWriteFile::new(stdio_files.output.clone()));
            let input = stdio_files.input;
            publish_stdio(&mut ShellStdioTable(&mut *table), &mut |read| {
                if read {
                    Ok(input.clone())
                } else {
                    Ok(output.clone())
                }
            })
            .expect("Failed to publish stdio");
        }

        let thr = Thread::new(pid, proc);
        *task.task_ext_mut() = Some(AxTaskExt::from_impl(thr));

        let shell_task = spawn_task_with_affinity(task, singleton(shell_hart));
        if shell_task.is_none() {
            fault(&diag, "shell-spawn", "affinity-committed shell spawn rejected");
        }
        let shell_task = shell_task.unwrap();
        diag(&format!("[ms14] uart shell spawned hart={shell_hart}"));

        // Bounded first-write observation: the shell banner is the first
        // write through the witness; its recorded hart is the shell task's
        // actual execution hart (A9).
        let shell_actual = block_on(timeout(Some(super::START_DEADLINE), async {
            loop {
                let hart = SHELL_FIRST_WRITE_HART.load(Ordering::Acquire);
                if hart != usize::MAX {
                    return Some(hart);
                }
                sleep(Duration::from_millis(10)).await;
            }
        }))
        .ok()
        .flatten();
        let Some(shell_actual) = shell_actual else {
            fault(
                &diag,
                "shell-first-write",
                "the shell never wrote within the startup bound",
            );
        };
        diag(&format!(
            "[ms14] uart shell first-write hart={shell_actual} expected={shell_hart}"
        ));
        if shell_actual != shell_hart {
            fault(
                &diag,
                "shell-hart",
                "the shell task executed away from its singleton",
            );
        }

        let exit_code = shell_task.join();
        diag(&format!("[ms14] uart stage=exit join={exit_code:?}"));

        // Physical drain: the four TxCompletion conditions; on K3 the final
        // TEMT wait rides the native 1 ms timer hook.
        match block_on(timeout(
            Some(Duration::from_secs(30)),
            uart_init::wait_tx_drain(&uart_init::driver()),
        )) {
            Ok(()) => diag("[ms14] uart stage=drain done (TxCompletion four conditions)"),
            Err(_) => diag("[ms14] uart fault stage=drain deadline=30s"),
        }
        loop {
            riscv::asm::wfi();
        }
    }
}

// ── raw sub-form boot (the MS17 data-plane payload as the sole RX reader) ──

#[cfg(all(target_arch = "riscv64", feature = "k3-smp-uart-raw"))]
mod raw_boot {
    use alloc::format;

    use super::boot_support::{fault, published_schedulable, singleton, wait_copier_harts};
    use super::place_smp_uart_roles;
    use crate::drivers::k3_uart;
    use crate::drivers::uart_init;
    use axtask::set_current_affinity;

    /// The k3-smp-uart raw sub-form boot. The MS17 qualification payload is
    /// reused verbatim (including its duplicate-activate rejection witness)
    /// as the sole raw reader/writer: ASYNC_TTY is never constructed and the
    /// memory root is never mounted in this sub-form (Invariant 2).
    pub fn run_smp_uart_raw_boot() -> ! {
        let boot = axhal::percpu::this_cpu_id();
        let pre = |l: &str| ax_println!("{}", l);
        let diag = |l: &str| k3_uart::diag_line(l);

        // ── Placement: the shared policy decides, fail closed ──
        let schedulable = published_schedulable();
        let roles = match place_smp_uart_roles(&schedulable, boot) {
            Ok(r) => r,
            Err(e) => fault(&pre, "placement", &format!("{e:?}")),
        };
        pre(&format!(
            "[ms14] uart mode=k3-smp-uart begin harts={} boot_hart={boot} form=raw",
            axhal::cpu_num()
        ));
        pre(&format!(
            "[ms14] uart placement rx={} tx={} alarm={} harts={} anchor={boot}",
            roles.uart_rx,
            roles.uart_tx,
            roles.alarm_hart,
            schedulable.len()
        ));

        uart_init::init_uart_hardware();

        // Main task singleton before any TimerFuture (Invariant 3).
        if !set_current_affinity(singleton(boot)) {
            fault(&pre, "main-pin", "main task boot-hart pin rejected");
        }

        // SAFETY: this boot path runs once; no other ring producer (no
        // benchmark in this mode) ran before the copiers start.
        unsafe { uart_init::start_copiers() };

        // Bounded copier actual-hart observation (A9).
        let Some((rx_actual, tx_actual)) = wait_copier_harts() else {
            fault(
                &diag,
                "copier-first-poll",
                "copiers never polled within the startup bound",
            );
        };
        diag(&format!(
            "[ms14] uart copiers pinned rx={} tx={} actual rx={rx_actual} tx={tx_actual}",
            roles.uart_rx, roles.uart_tx
        ));

        // The payload owns the wire from here; it never returns.
        crate::platform::k3::run_uart_data_plane();
    }
}

#[cfg(all(
    target_arch = "riscv64",
    feature = "k3-smp-uart",
    not(feature = "k3-smp-uart-raw")
))]
pub use shell_boot::run_smp_uart_shell_boot;
#[cfg(all(target_arch = "riscv64", feature = "k3-smp-uart-raw"))]
pub use raw_boot::run_smp_uart_raw_boot;
