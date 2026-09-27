// kernel/src/drivers/net_wake_witness.rs

//! QEMU-only timer-disabled remote-wake witness binding（design D7、Task 3.4）。
//!
//! 把纯状态机（[`net_wake_witness_logic`]）绑定到 RISC-V QEMU 平台：
//! - `START` 在 spawn 前通过 `reserve` 独占运行；spawn 失败在 enqueue 前由
//!   `reserve_rollback` 回滚，不产生任务；
//! - 目标 hart 用一个 singleton-affinity 任务关闭本地 timer，注册 waker 后提交
//!   park（per-run 标志在 START 复位）；target **不自行发布 Armed**——`Armed`
//!   只能由 TRIGGER 在验证 saved task 已提交 `TaskState::Blocked` 后原子提升；
//!   被接受的 remote trigger 之后的 wake 必然命中已注册 waiter
//!   （真实 `Blocked -> Ready` + 目标 hart reschedule IPI）；
//! - 目标是 timer-disabled 的：**任何** post-disable 退出路径都必须先恢复本地
//!   timer，再由 `restore_ack` 收敛到有界 terminal（`on_armed`/`on_woken`/
//!   restore-ack 失败均不留下无界停机）；
//! - `TRIGGER` 由远程 hart 调用：先验证 saved target task 已提交
//!   `TaskState::Blocked`（拒绝 Running/Ready/缺失 handle），再在机器锁下
//!   原子提升（仅未取消的 `Starting` run 接受；取消/terminal/stale 拒绝且不
//!   wake），通过后才置触发标志并唤醒已停放任务；本地/过早/已取消触发被
//!   拒绝且不改变状态；
//! - 目标任务恢复时记录 hart 与**目标 hart 自己的 reschedule-IPI 接收增量**
//!   （per-hart 因果观测，非全局 run 身份），先恢复本地 timer 并 `restore_ack`，
//!   再发布 terminal；`CANCEL` 由有界监督通过 `request_cancel` 请求取消，target
//!   仍须恢复 timer 后才进入 `Failed`（ack-before-terminal）；
//! - 监督用**相对 deadline**（`supervisor_deadline(now, period)`）在一个
//!   timer-enabled 远端 hart 上以作用域 token（[`UNKNOWN_SCOPE`] 之外的存活持有）
//!   确认只作用于自己的运行，杜绝回滚重试后旧监督误取消新运行；旧实现的单调
//!   `run_id` 属身份型证据，已移除。
//!
//! 完整 QEMU runtime 资格仍在 Iteration 005 验证；本 Cycle（3.4）通过宿主状态机
//! 模型封闭 single-flight/reset/trigger/cancel/ack-before-terminal 契约，并由
//! target build 证明该 control 可编译。D1 与普通构建不包含此模块。

use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use embassy_sync::waitqueue::AtomicWaker;

use super::net_wake_witness_logic::WitnessMachine;
pub use super::net_wake_witness_logic::{WitnessPhase, WitnessSnapshot};

/// 监督 deadline（微秒）。目标关闭本地 timer 后若在期限内未被远端 wake 恢复，
/// 监督在此 deadline 上从另一个 timer-enabled hart 请求取消并唤醒目标。
/// 该期限是**相对**时长；实际的绝对 deadline 由
/// `supervisor_deadline(wall_time(), WITNESS_DEADLINE_US)` 计算得到。
const WITNESS_DEADLINE_US: u64 = 5_000_000;

/// 单例 witness 状态机，由 ioctl 与目标任务共享。
static MACHINE: kspin::SpinNoIrq<WitnessMachine> = kspin::SpinNoIrq::new(WitnessMachine::new());

/// 目标任务注册的 waker；`trigger()` 从其（可能不同的）hart 唤醒。
static WAKER: AtomicWaker = AtomicWaker::new();

/// 目标任务应恢复的触发标志（per-run；START 复位）。
static TRIGGERED: AtomicBool = AtomicBool::new(false);

/// 目标任务应取消/失败标志（per-run；START 复位）。
static CANCELLED: AtomicBool = AtomicBool::new(false);

/// 监督中止标志（per-run；START 复位）。当 target spawn 在 supervisor 已入队后
/// 失败时置位，令已入队的 supervisor 在 deadline 唤醒后失效退出，不干预新 run。
static SUPERVISOR_ABORT: AtomicBool = AtomicBool::new(false);

/// 每次 START 分配一个作用域所有权 token（私有、单调，**只在监督死锁判断中使用**，
/// 不进入 V5 也不用于判定运行证据）。区别于被移除的 `run_id`：它不归因结果，
/// 只表达“监督只属于自己的运行”。
static SCOPE_NEXT: AtomicUsize = AtomicUsize::new(1);

/// 目标 hart 在 park（arm）前观测到的、**目标 hart 自己**的 reschedule-IPI 接收
/// 数（Relaxed telemetry；per-hart 因果观测）。非 SMP 下保持 0。
static WITNESS_IPI_BEFORE: AtomicU64 = AtomicU64::new(0);

/// 目标 hart 被 wake 恢复后观测到的、**目标 hart 自己**的 reschedule-IPI 接收数
/// （Relaxed telemetry）。`after > before` 证明本次运行目标 hart 收到了至少一次
/// remote-ready IPI（因果链的接收端），不依赖全局总计。
static WITNESS_IPI_AFTER: AtomicU64 = AtomicU64::new(0);

/// 构建 singleton affinity 掩码。
///
/// `hart` 来自已发布 schedulable 集合，其成员必然低于 mask 容量；expect 在
/// fail-closed 边界记录该不变量。
fn singleton_mask(hart: usize) -> axtask::AxCpuMask {
    let mut mask = axtask::AxCpuMask::new();
    mask.set(hart, true)
        .expect("placement hart < mask capacity by schedulable-set invariant");
    mask
}

/// 目标 hart 任务：关闭本地 timer -> 注册 waiter 并提交 park -> （由 TRIGGER
/// 原子提升后）被远端 wake 恢复 -> 恢复 timer + ack -> terminal。
///
/// 核心不变量：
/// - 本地 timer 一旦在本函数被关闭，必须在**任意**退出路径上先恢复，
///   绝不带禁用 timer 提前返回（否则丢失 wake 时目标会永久停摆，也违背
///   ack-before-terminal）。`on_woken`/`restore_ack` 的结果被检验，任何失败
///   路径都把 timer 恢复并在有界 terminal（`Failed`）收敛，不留下无界停机。
/// - **target 永不自行发布 `Armed`**（Plan Review finding 2 的最终闭合）：
///   `poll_fn` 只做 `WAKER.register` + 标志检查 + `Pending`；`Armed` 只能由
///   `TRIGGER` 在验证了 saved task 已提交 `TaskState::Blocked` 之后，通过
///   `WitnessMachine::trigger` 原子提升（`Starting -> Armed` 并记录触发方）。
///   因此被接受的 trigger 之后的 wake 必然命中已注册 waiter：真实
///   `Blocked -> Ready`、目标 hart 收到 reschedule IPI、resume 计数推进
///   （`after > before`），不存在"trigger 在 target 仍 `Running` 时被接受、
///   `wake_by_ref` 只置 `woke` 标志、无 `Blocked -> Ready`、无 IPI"的窗口。
fn witness_target(_target_hart: usize) {
    // target 在 START 中已被 reserve（phase = Starting）；本任务不触碰机器，
    // 直到 TRIGGER 提升或监督取消后由统一后置路径收敛。
    axhal::irq::set_enable(axhal::time::irq_num(), false);

    // 记录目标 hart 在 park 前的 reschedule-IPI 接收基线（per-hart），供
    // terminal 后的 per-hart 因果判定。
    #[cfg(feature = "smp")]
    WITNESS_IPI_BEFORE.store(ipi_by_current_pinned(_target_hart), Ordering::Relaxed);

    axtask::future::block_on(core::future::poll_fn(|cx| {
        // 1) 先注册 waiter：提升后的 trigger/cancel 必然 wake 到真实注册的 waker。
        WAKER.register(cx.waker());
        if TRIGGERED.load(Ordering::Acquire) || CANCELLED.load(Ordering::Acquire) {
            core::task::Poll::Ready(())
        } else {
            core::task::Poll::Pending
        }
    }));

    // park 返回：先恢复本地 timer（ack-before-terminal 契约），再记录目标 hart
    // 自己的 reschedule-IPI 接收数，随后收敛到 terminal。
    axhal::irq::set_enable(axhal::time::irq_num(), true);
    #[cfg(feature = "smp")]
    WITNESS_IPI_AFTER.store(ipi_by_current_pinned(_target_hart), Ordering::Relaxed);

    let mut m = MACHINE.lock();
    // 提升路径：Armed -> Woken。取消路径（从未被提升，phase 仍 Starting）不调用
    // on_woken，避免把预期的取消收敛计成非法转换污染 V5 观测。
    if m.snapshot().phase == WitnessPhase::Armed {
        let _ = m.on_woken();
    }
    // 确认 timer 已恢复。任何 terminal 发布都必须在此之后。
    let acked = m.restore_ack().is_ok();
    if acked {
        // 有界 terminal：被监督取消则 Failed，否则 Completed。
        if CANCELLED.load(Ordering::Acquire) {
            let _ = m.fail();
        } else {
            let _ = m.complete();
        }
    }
    // ack 失败（run 已被释放/terminal 已发布）：timer 已恢复，安全返回。
}

/// 读取当前目标 hart 自己的 reschedule-IPI 接收计数。
#[cfg(feature = "smp")]
#[inline]
fn ipi_by_current_pinned(target_hart: usize) -> u64 {
    axtask::ipi_received_count_by_hart(target_hart) as u64
}

/// 有界监督任务：运行在与目标不同的 timer-enabled hart 上。使用**相对 deadline**
/// （`now + WITNESS_DEADLINE_US`）——`sleep_until` 把参数当作绝对墙钟 deadline，
/// 传相对 5s 会在启动超 5s 后立即过期。到达后，若目标仍 `Starting`/`Armed` 且
/// 仍属于本监督的作用域（远端 wake 丢失、目标 timer 已关闭，或 target 尚未被
/// 调度、尚未 arm——finding 1 的迟到目标窗口），请求取消并通过 CANCELLED 唤醒
/// 目标，让它完成 timer 恢复 + ack 后进入 `Failed`。目标已离开 `Starting`/`Armed`
/// 或 run 已不属本监督作用域时不干预。
fn witness_supervisor(my_target: usize, my_scope: usize) {
    let now_us = axhal::time::wall_time_nanos() / axhal::time::NANOS_PER_MICROS;
    let deadline_us =
        super::net_wake_witness_logic::supervisor_deadline(now_us, WITNESS_DEADLINE_US);
    axtask::future::block_on(axtask::future::sleep_until(axhal::time::TimeValue::from_micros(
        deadline_us,
    )));
    if SUPERVISOR_ABORT.load(Ordering::Acquire) {
        return;
    }
    let mut machine = MACHINE.lock();
    let s = machine.snapshot();
    if super::net_wake_witness_logic::supervisor_should_cancel(&s, my_target, my_scope) {
        let _ = machine.request_cancel();
        drop(machine);
        CANCELLED.store(true, Ordering::Release);
        WAKER.wake();
    }
}

/// ioctl `op` 命令。
pub mod op {
    pub const START: u64 = 1;
    pub const TRIGGER: u64 = 2;
    pub const CANCEL: u64 = 3;
}

/// 选择一个与 `anchor` 不同的可调度 hart；找不到返回 `UNKNOWN_HART`。
fn next_other_hart(sched: &axtask::AxCpuMask, anchor: usize) -> usize {
    for i in 0..axconfig::plat::MAX_CPU_NUM {
        if i != anchor && sched.get(i) {
            return i;
        }
    }
    super::net_placement::UNKNOWN_HART
}

/// ioctl 入口。`op` 见 [`op`]；`arg` 对 `START` 为目标 hart（`UNKNOWN` 或等于
/// 当前 hart 时自动选择一个不同的可调度 hart，保证 wake 确实是 remote）。
pub fn control(op: u64, arg: u64) -> Result<(), ()> {
    match op {
        op::START => {
            let anchor = axhal::percpu::this_cpu_id();
            let sched = axtask::schedulable_cpu_mask();
            let mut target = arg as usize;
            if target == super::net_placement::UNKNOWN_HART || target == anchor {
                target = next_other_hart(&sched, anchor);
            }
            if target == super::net_placement::UNKNOWN_HART || !sched.get(target) {
                return Err(());
            }
            // 监督必须运行在与目标不同的 timer-enabled hart（目标会关闭本地
            // timer）。
            let supervisor = next_other_hart(&sched, target);
            if supervisor == super::net_placement::UNKNOWN_HART || !sched.get(supervisor) {
                return Err(());
            }
            // single-flight: 在 spawn 前独占保留。Busy → 拒绝并发 start。
            // 每个 START 尝试分配一个私有作用域 token（所有权表达，不属证据）。
            let scope = SCOPE_NEXT.fetch_add(1, Ordering::Relaxed);
            if MACHINE.lock().reserve(scope, target).is_err() {
                return Err(());
            }
            // per-run：复位跨运行标志。
            TRIGGERED.store(false, Ordering::Release);
            CANCELLED.store(false, Ordering::Release);
            SUPERVISOR_ABORT.store(false, Ordering::Release);
            // 先入队监督（timer-enabled 无关 hart）；失败则整体回滚，不产生任务。
            if axtask::spawn_with_name_affinity(
                move || witness_supervisor(target, scope),
                "wake-witness-supervisor".into(),
                singleton_mask(supervisor),
            )
            .is_none()
            {
                let _ = MACHINE.lock().reserve_rollback();
                return Err(());
            }
            // 再入队目标。目标 spawn 失败时监督已入队：置中止标志让它过期失效，
            // 再回滚保留（监督 deadline 唤醒后读到 SUPERVISOR_ABORT 即退出；
            // 即使它读到被复位的新标志，作用域 token 不匹配也不会取消新 run）。
            let Some(task) = axtask::spawn_with_name_affinity(
                move || witness_target(target),
                "wake-witness-target".into(),
                singleton_mask(target),
            ) else {
                SUPERVISOR_ABORT.store(true, Ordering::Release);
                let _ = MACHINE.lock().reserve_rollback();
                return Err(());
            };
            // 保存 handle；Iteration 003/005 迁移与监督使用。
            *WITNESS_TASK.lock() = Some(task);
            Ok(())
        }
        op::TRIGGER => {
            // Plan Review finding 2 的最终闭合：trigger 必须针对一个**已提交
            // Blocked** 的目标。门控顺序：
            // 1) saved target handle 存在，且其公共调度状态是 `TaskState::Blocked`
            //    （拒绝 Running/Ready/缺失 handle，不改变任何状态、不 wake）；
            // 2) saved task 的 singleton affinity 必须仍覆盖当前 run 的目标 hart
            //    （拒绝跨 run/陈旧任务，杜绝 stale 提升）；
            // 3) 在机器锁下原子提升：仅未取消的 `Starting` run 接受
            //    `Starting -> Armed` 并记录触发方；取消/terminal 拒绝且不 wake。
            // 提升成功后才置 TRIGGERED 并 wake 已停放的 task：wake 必然命中
            // 已注册 waiter（Blocked -> Ready + 目标 hart reschedule IPI）。
            let anchor = axhal::percpu::this_cpu_id();
            let task = { WITNESS_TASK.lock().clone() };
            let Some(task) = task else {
                return Err(());
            };
            if task.state() != axtask::TaskState::Blocked {
                return Err(());
            }
            let mut machine = MACHINE.lock();
            let target = machine.snapshot().target_hart;
            if target == super::net_placement::UNKNOWN_HART || !task.cpumask().get(target) {
                return Err(());
            }
            if machine.trigger(anchor).is_err() {
                return Err(());
            }
            drop(machine);
            TRIGGERED.store(true, Ordering::Release);
            WAKER.wake();
            Ok(())
        }
        op::CANCEL => {
            // 有界监督请求取消：置取消标志并唤醒 target 完成 timer 恢复后 Failed。
            if MACHINE.lock().request_cancel().is_err() {
                return Err(());
            }
            CANCELLED.store(true, Ordering::Release);
            WAKER.wake();
            Ok(())
        }
        _ => Err(()),
    }
}

lazy_static::lazy_static! {
    static ref WITNESS_TASK: kspin::SpinNoIrq<Option<axtask::AxTaskRef>> =
        kspin::SpinNoIrq::new(None);
}

/// Snapshot（`READ` 之外也提供给 V5/调试用）。
pub fn snapshot() -> WitnessSnapshot {
    MACHINE.lock().snapshot()
}

/// 目标 hart 的 reschedule-IPI 因果观测（Task 3.3/3.4）：目标在 park（arm）前与
/// 被 wake 恢复后，各自看到的**目标 hart 自己**的 reschedule-IPI 接收数。两者
/// 都是 Relaxed per-hart 因果观测（`after > before` 证明本次运行目标 hart 收到
/// 了至少一次 remote-ready IPI），不是全局总计，也不构成身份型 run 证据。
pub fn ipi_causality() -> (u64, u64) {
    (
        WITNESS_IPI_BEFORE.load(Ordering::Relaxed),
        WITNESS_IPI_AFTER.load(Ordering::Relaxed),
    )
}

/// 当前 witness 阶段（observation）。
pub fn phase() -> WitnessPhase {
    MACHINE.lock().snapshot().phase
}