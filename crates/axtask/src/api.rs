//! Task APIs for multi-task configuration.

use alloc::{
    string::String,
    sync::{Arc, Weak},
};

use kernel_guard::NoPreemptIrqSave;

pub(crate) use crate::run_queue::{current_run_queue, select_run_queue};
#[doc(cfg(all(feature = "multitask", feature = "task-ext")))]
#[cfg(feature = "task-ext")]
pub use crate::task::{AxTaskExt, TaskExt};
#[doc(cfg(all(feature = "multitask", feature = "irq")))]
#[cfg(feature = "irq")]
pub use crate::timers::register_timer_callback;
#[doc(cfg(feature = "multitask"))]
pub use crate::{
    task::{CurrentTask, TaskId, TaskInner, TaskState},
    wait_queue::WaitQueue,
};

/// The reference type of a task.
pub type AxTaskRef = Arc<AxTask>;

/// The weak reference type of a task.
pub type WeakAxTaskRef = Weak<AxTask>;

/// The wrapper type for [`cpumask::CpuMask`] with SMP configuration.
pub type AxCpuMask = cpumask::CpuMask<{ axconfig::plat::MAX_CPU_NUM }>;

cfg_if::cfg_if! {
    if #[cfg(feature = "sched-rr")] {
        const MAX_TIME_SLICE: usize = 5;
        pub(crate) type AxTask = axsched::RRTask<TaskInner, MAX_TIME_SLICE>;
        pub(crate) type Scheduler = axsched::RRScheduler<TaskInner, MAX_TIME_SLICE>;
    } else if #[cfg(feature = "sched-cfs")] {
        pub(crate) type AxTask = axsched::CFSTask<TaskInner>;
        pub(crate) type Scheduler = axsched::CFScheduler<TaskInner>;
    } else {
        // If no scheduler features are set, use FIFO as the default.
        pub(crate) type AxTask = axsched::FifoTask<TaskInner>;
        pub(crate) type Scheduler = axsched::FifoScheduler<TaskInner>;
    }
}

#[cfg(feature = "preempt")]
struct KernelGuardIfImpl;

#[cfg(feature = "preempt")]
#[crate_interface::impl_interface]
impl kernel_guard::KernelGuardIf for KernelGuardIfImpl {
    fn disable_preempt() {
        if let Some(curr) = current_may_uninit() {
            curr.disable_preempt();
        }
    }

    fn enable_preempt() {
        if let Some(curr) = current_may_uninit() {
            curr.enable_preempt(true);
        }
    }
}

/// Gets the current task, or returns [`None`] if the current task is not
/// initialized.
pub fn current_may_uninit() -> Option<CurrentTask> {
    CurrentTask::try_get()
}

/// Gets the current task.
///
/// # Panics
///
/// Panics if the current task is not initialized.
pub fn current() -> CurrentTask {
    CurrentTask::get()
}

/// Initializes the task scheduler (for the primary CPU).
pub fn init_scheduler() {
    info!("Initialize scheduling...");

    // Initialize the run queue.
    crate::run_queue::init();

    // Take exclusive ownership of the single S_SOFT reschedule slot. A second
    // owner (e.g. `axruntime/ipi`) would share the same software-interrupt and
    // must fail closed rather than double-register. On the host the fake
    // platform refuses registration, which is exercised by the ipi seam tests
    // rather than by the product init path.
    #[cfg(all(feature = "ipi", not(test)))]
    assert!(
        crate::ipi::try_register_ipi(),
        "reschedule IPI S_SOFT slot is already owned by another runtime feature"
    );

    info!("  use {} scheduler.", Scheduler::scheduler_name());
}

pub(crate) fn cpu_mask_full() -> AxCpuMask {
    use spin::Lazy;

    static CPU_MASK_FULL: Lazy<AxCpuMask> = Lazy::new(|| {
        let cpu_num = axhal::cpu_num();
        let mut cpumask = AxCpuMask::new();
        for cpu_id in 0..cpu_num {
            cpumask.set(cpu_id, true);
        }
        cpumask
    });

    *CPU_MASK_FULL
}

/// Initializes the task scheduler for secondary CPUs.
pub fn init_scheduler_secondary() {
    crate::run_queue::init_secondary();
}

/// Handles periodic timer ticks for the task manager.
///
/// For example, advance scheduler states, checks timed events, etc.
#[cfg(feature = "irq")]
#[doc(cfg(feature = "irq"))]
pub fn on_timer_tick() {
    use kernel_guard::NoOp;
    crate::timers::check_events();
    // Since irq and preemption are both disabled here,
    // we can get current run queue with the default `kernel_guard::NoOp`.
    current_run_queue::<NoOp>().scheduler_timer_tick();
}

/// Adds the given task to the run queue, returns the task reference.
pub fn spawn_task(task: TaskInner) -> AxTaskRef {
    let task_ref = task.into_arc();
    select_run_queue::<NoPreemptIrqSave>(&task_ref).add_task(task_ref.clone());
    task_ref
}

/// Validates a CPU affinity mask against an explicitly-provided online/schedulable
/// CPU count.
///
/// A valid mask must be non-empty and every set bit must index a CPU that is
/// online and schedulable, i.e. `< cpu_num`. This rejects empty masks as well as
/// masks that reference out-of-range, offline or not-yet-initialized CPUs. It is
/// the single pure validation seam shared by the pre-enqueue spawn and the safe
/// task-mask update, and is host-testable without a platform.
pub fn validate_affinity(cpumask: AxCpuMask, cpu_num: usize) -> bool {
    if cpumask.is_empty() {
        return false;
    }
    match cpumask.last_index() {
        Some(last) => last < cpu_num,
        None => false,
    }
}

/// Returns `true` only when `cpumask` is non-empty and every set bit names a run
/// queue that has been *published* as schedulable (i.e. its slot is written and
/// its readiness bit is visible with an Acquire load). Unlike [`validate_affinity`],
/// which checks a configured count, this rejects CPUs whose run queues are
/// configured but not yet initialized. Used for explicit affinity spawn/update so
/// an uninitialized queue can never be targeted.
pub(crate) fn validate_schedulable(cpumask: AxCpuMask) -> bool {
    if cpumask.is_empty() {
        return false;
    }
    // Reject any set bit at or above the configured CPU count: such a CPU can
    // never be schedulable, and must fail closed rather than slip past the loop.
    if let Some(last) = cpumask.last_index() {
        if last >= axconfig::plat::MAX_CPU_NUM {
            return false;
        }
    }
    let sched = crate::run_queue::schedulable();
    for i in 0..axconfig::plat::MAX_CPU_NUM {
        if cpumask.get(i) && !sched.get(i) {
            return false;
        }
    }
    true
}

/// Spawns a task with a pre-validated CPU affinity committed *before* the task
/// is first placed into a run queue.
///
/// Unlike the plain [`spawn_task`], which defaults to the full online mask and
/// immediately selects a run queue, this commits `cpumask` on the task before
/// `select_run_queue`, so an affinity-aware driver can pin a copier/owner/runner
/// from its very first scheduling step. Returns `None` (and does not enqueue)
/// when the mask is invalid, so an invalid placement fails closed and never
/// falls back to a wrong first enqueue.
pub fn spawn_task_with_affinity(task: TaskInner, cpumask: AxCpuMask) -> Option<AxTaskRef> {
    if !validate_schedulable(cpumask) {
        return None;
    }
    // Commit affinity before the first run-queue selection.
    task.set_cpumask(cpumask);
    let task_ref = task.into_arc();
    select_run_queue::<NoPreemptIrqSave>(&task_ref).add_task(task_ref.clone());
    Some(task_ref)
}

/// Spawns a new task with the given parameters.
///
/// Returns the task reference.
pub fn spawn_raw<F>(f: F, name: String, stack_size: usize) -> AxTaskRef
where
    F: FnOnce() + Send + 'static,
{
    spawn_task(TaskInner::new(f, name, stack_size))
}

/// Spawns a new task with an affinity committed before first enqueue.
///
/// Returns `None` when `cpumask` is invalid (see [`validate_affinity`]).
pub fn spawn_raw_with_affinity<F>(
    f: F,
    name: String,
    stack_size: usize,
    cpumask: AxCpuMask,
) -> Option<AxTaskRef>
where
    F: FnOnce() + Send + 'static,
{
    spawn_task_with_affinity(TaskInner::new(f, name, stack_size), cpumask)
}

/// Spawns a new task with the given name and the default stack size ([`axconfig::TASK_STACK_SIZE`]).
///
/// Returns the task reference.
pub fn spawn_with_name<F>(f: F, name: String) -> AxTaskRef
where
    F: FnOnce() + Send + 'static,
{
    spawn_raw(f, name, axconfig::TASK_STACK_SIZE)
}

/// Spawns a new task by name with an affinity committed before first enqueue.
///
/// Returns `None` when `cpumask` is invalid.
pub fn spawn_with_name_affinity<F>(f: F, name: String, cpumask: AxCpuMask) -> Option<AxTaskRef>
where
    F: FnOnce() + Send + 'static,
{
    spawn_raw_with_affinity(f, name, axconfig::TASK_STACK_SIZE, cpumask)
}

/// Spawns a new task with the default parameters.
///
/// The default task name is an empty string. The default task stack size is
/// [`axconfig::TASK_STACK_SIZE`].
///
/// Returns the task reference.
pub fn spawn<F>(f: F) -> AxTaskRef
where
    F: FnOnce() + Send + 'static,
{
    spawn_with_name(f, String::new())
}

/// Set the priority for current task.
///
/// The range of the priority is dependent on the underlying scheduler. For
/// example, in the [CFS] scheduler, the priority is the nice value, ranging from
/// -20 to 19.
///
/// Returns `true` if the priority is set successfully.
///
/// [CFS]: https://en.wikipedia.org/wiki/Completely_Fair_Scheduler
pub fn set_priority(prio: isize) -> bool {
    current_run_queue::<NoPreemptIrqSave>().set_current_priority(prio)
}

/// Set the affinity for the current task.
/// [`AxCpuMask`] is used to specify the CPU affinity.
/// Returns `true` if the affinity is set successfully.
///
/// TODO: support set the affinity for other tasks.
pub fn set_current_affinity(cpumask: AxCpuMask) -> bool {
    if cpumask.is_empty() {
        false
    } else {
        let curr = current().clone();

        curr.set_cpumask(cpumask);
        // After setting the affinity, we need to check if current cpu matches
        // the affinity. If not, we need to migrate the task to the correct CPU.
        #[cfg(feature = "smp")]
        if !cpumask.get(axhal::percpu::this_cpu_id()) {
            const MIGRATION_TASK_STACK_SIZE: usize = 4096;
            // Spawn a new migration task for migrating.
            let migration_task = TaskInner::new(
                move || crate::run_queue::migrate_entry(curr),
                "migration-task".into(),
                MIGRATION_TASK_STACK_SIZE,
            )
            .into_arc();

            // Migrate the current task to the correct CPU using the migration task.
            current_run_queue::<NoPreemptIrqSave>().migrate_current(migration_task);

            assert!(
                cpumask.get(axhal::percpu::this_cpu_id()),
                "Migration failed"
            );
        }
        true
    }
}

/// Current task gives up the CPU time voluntarily, and switches to another
/// ready task.
pub fn yield_now() {
    current_run_queue::<NoPreemptIrqSave>().yield_current()
}

/// Current task is going to sleep for the given duration.
///
/// If the feature `irq` is not enabled, it uses busy-wait instead.
pub fn sleep(dur: core::time::Duration) {
    sleep_until(axhal::time::wall_time() + dur);
}

/// Current task is going to sleep, it will be woken up at the given deadline.
///
/// If the feature `irq` is not enabled, it uses busy-wait instead.
pub fn sleep_until(deadline: axhal::time::TimeValue) {
    #[cfg(feature = "irq")]
    crate::future::block_on(crate::future::sleep_until(deadline));
    #[cfg(not(feature = "irq"))]
    axhal::time::busy_wait_until(deadline);
}

/// Exits the current task.
pub fn exit(exit_code: i32) -> ! {
    current_run_queue::<NoPreemptIrqSave>().exit_current(exit_code)
}

/// The idle task routine.
///
/// It runs an infinite loop that keeps calling [`yield_now()`].
pub fn run_idle() -> ! {
    loop {
        yield_now();
        trace!("idle task: waiting for IRQs...");
        #[cfg(feature = "irq")]
        axhal::asm::wait_for_irqs();
    }
}
