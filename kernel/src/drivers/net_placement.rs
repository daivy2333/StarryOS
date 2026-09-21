// kernel/src/drivers/net_placement.rs

//! Network background-role placement adapter（design D4/D5）。
//!
//! 内核适配层把共享 placement policy（`placement::place_roles`）产出的 network
//! `owner`/`runner` singleton hart 落到实处：
//! - 在黄本任务首次入队前记录被固定的 singleton hart（`record_*_pinned`）；
//! - 保存 owner/runner 的 task handle，供 V5 观测与后续 Iteration 003 迁移使用；
//! - 记录网络设备 ISR 实际执行 hart（`record_irq_hart`）。
//!
//! placement 只从 `axtask::schedulable_cpu_mask()` 导出的已发布 schedulable 集合
//! 推导，不虚构拓扑、不硬编码 hart ID（design D4）。空/非法集合 fail closed。

use core::sync::atomic::{AtomicUsize, Ordering};

use lazy_static::lazy_static;

/// 未放置/未知 hart 的哨兵值。
pub use super::hart_counter::UNKNOWN_HART;
use super::{
    hart_counter::HartCounter,
    placement::{self, RolePlacement},
};

/// 网络 owner 与 runner 被固定的 singleton hart（在首次入队前记录）。
static OWNER_PINNED: AtomicUsize = AtomicUsize::new(UNKNOWN_HART);
static RUNNER_PINNED: AtomicUsize = AtomicUsize::new(UNKNOWN_HART);

/// 网络设备 ISR 实际执行 hart（由 `virtio_net_irq` 的 handler 包装器调用）。
static NET_IRQ: HartCounter = HartCounter::new();

/// owner/runner 的 task handle；保存供 V5 观测与 Iteration 003 迁移使用。
lazy_static! {
    static ref OWNER_TASK: kspin::SpinNoIrq<Option<axtask::AxTaskRef>> =
        kspin::SpinNoIrq::new(None);
    static ref RUNNER_TASK: kspin::SpinNoIrq<Option<axtask::AxTaskRef>> =
        kspin::SpinNoIrq::new(None);
}

/// Derives the network owner/runner placement over the published schedulable set.
///
/// `Some` carries the full [`RolePlacement`] (so UART, owner and runner occupy
/// deterministic, policy-chosen harts); `None` means no schedulable hart exists
/// and every network role must fail closed.
pub fn place() -> Option<RolePlacement> {
    let sched = axtask::schedulable_cpu_mask();
    let cpus = sched_hart_ids();
    let anchor = axhal::percpu::this_cpu_id();
    placement::place_roles(&cpus, anchor)
}

/// Ordered, ascending, de-duplicated published-schedulable hart ids.
pub fn sched_hart_ids() -> alloc::vec::Vec<usize> {
    let sched = axtask::schedulable_cpu_mask();
    let mut cpus = alloc::vec::Vec::new();
    for id in 0..placement::MAX_CPU_NUM {
        if sched.get(id) {
            cpus.push(id);
        }
    }
    cpus
}

/// Records the singleton hart a network role was pinned to before first enqueue.
pub fn record_owner_pinned(hart: usize) {
    OWNER_PINNED.store(hart, Ordering::Relaxed);
}

/// Records the singleton hart the network runner was pinned to before enqueue.
pub fn record_runner_pinned(hart: usize) {
    RUNNER_PINNED.store(hart, Ordering::Relaxed);
}

/// Saves the network owner's task handle.
pub fn set_owner_task(task: axtask::AxTaskRef) {
    *OWNER_TASK.lock() = Some(task);
}

/// Saves the network runner's task handle.
pub fn set_runner_task(task: axtask::AxTaskRef) {
    *RUNNER_TASK.lock() = Some(task);
}

/// Records that the VirtIO-net ISR ran on the current hart.
pub fn record_irq_hart() {
    let id = axhal::percpu::this_cpu_id();
    NET_IRQ.record(id);
}

/// Pinned network owner hart (or `UNKNOWN_HART` before placement).
pub fn owner_pinned_hart() -> usize {
    OWNER_PINNED.load(Ordering::Relaxed)
}

/// Pinned network runner hart (or `UNKNOWN_HART` before placement).
pub fn runner_pinned_hart() -> usize {
    RUNNER_PINNED.load(Ordering::Relaxed)
}

/// Network placement snapshot consumed by the QEMU-only V5 wire type.
///
/// `owner_*`/`runner_*` report both the pinned singleton affinity AND the actual
/// execution hart observed at the real poll sites (via `axnet::*_hart_tuple`).
/// The affinity is scheduler-enforced for a pinned task; the execution tuple is
/// recorded separately at each poll and must never be inferred from affinity. The
/// IRQ hart and poll counts are direct observations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NetPlacementSnapshot {
    pub owner_affinity: usize,
    pub runner_affinity: usize,
    pub irq_last_hart: usize,
    pub irq_hart_mask: u64,
    pub irq_events: u64,
    pub owner_last_hart: usize,
    pub owner_hart_mask: u64,
    pub owner_events: u64,
    pub runner_last_hart: usize,
    pub runner_hart_mask: u64,
    pub runner_events: u64,
    pub ipi_sent: u64,
    pub ipi_received: u64,
    pub affinity_rejects: u64,
}

/// Reads a coherent network placement snapshot for V5 assembly.
#[cfg(feature = "qemu")]
pub fn snapshot() -> NetPlacementSnapshot {
    let (irq_last, irq_mask, irq_events) = NET_IRQ.read();
    // Task 3.3: real execution harts recorded at the owner/runner poll sites
    // (never inferred from affinity).
    let (owner_last, owner_mask, owner_events) = axnet::owner_hart_tuple();
    let (runner_last, runner_mask, runner_events) = axnet::runner_hart_tuple();
    #[cfg(feature = "smp")]
    let (ipi_sent, ipi_received) = (
        axtask::ipi_sent_count() as u64,
        axtask::ipi_received_count() as u64,
    );
    #[cfg(not(feature = "smp"))]
    let (ipi_sent, ipi_received) = (0u64, 0u64);
    NetPlacementSnapshot {
        owner_affinity: owner_pinned_hart(),
        runner_affinity: runner_pinned_hart(),
        irq_last_hart: irq_last,
        irq_hart_mask: irq_mask,
        irq_events,
        owner_last_hart: owner_last,
        owner_hart_mask: owner_mask,
        owner_events,
        runner_last_hart: runner_last,
        runner_hart_mask: runner_mask,
        runner_events,
        ipi_sent,
        ipi_received,
        affinity_rejects: axtask::affinity_reject_count() as u64,
    }
}

/// QEMU+SMP bounded network placement smoke (design D9 / Task 3.5).  Validated
/// only the *fixed placement* of the two network background roles after
/// secondary-ready, without claiming the later data-plane qualification:
///
/// - both owner and runner have polled on their pinned singleton harts
///   (`task_poll >= 1` from axnet telemetry);
/// - the DIRECTLY RECORDED execution tuples match the pinned singleton affinity:
///   `owner/runner_last_hart == affinity`, the hart mask is exactly the
///   singleton bit, and `owner/runner_events >= 1` (Task 3.3 real poll-site
///   observations — never inferred from affinity; Plan Review finding 4);
/// - owner/runner affinity are distinct and each is a member of the published
///   schedulable mask;
/// - no invalid-affinity rejection occurred.
///
/// It prints `[NET-SMP-SMOKE] PASS / FAIL / SKIP` and never panics on a failed
/// check (the GREEN condition is 0 panic).
#[cfg(feature = "qemu")]
pub fn snapshot_boot_smoke() {
    const WAIT_LIMIT: u32 = 2_000_000;

    if axhal::cpu_num() != 16 {
        ax_println!(
            "[NET-SMP-SMOKE] SKIP reason=non-16-smp configured={}",
            axhal::cpu_num()
        );
        return;
    }

    // Wait until both roles have actually polled on their pinned harts.
    let mut waited = 0u32;
    loop {
        let rx = axnet::rx_snapshot();
        let stack = axnet::stack_snapshot();
        if rx.task_poll >= 1 && stack.task_poll >= 1 {
            break;
        }
        waited += 1;
        if waited > WAIT_LIMIT {
            ax_println!(
                "[NET-SMP-SMOKE] FAIL reason=roles-not-polled owner_polls={} runner_polls={}",
                rx.task_poll,
                stack.task_poll
            );
            ax_println!("[NET-SMP-SMOKE] FAIL");
            return;
        }
        axtask::yield_now();
    }

    let s = snapshot();
    let sched = axtask::schedulable_cpu_mask();

    let owner_singleton = s.owner_affinity < 64 && s.owner_hart_mask == (1u64 << s.owner_affinity);
    let runner_singleton =
        s.runner_affinity < 64 && s.runner_hart_mask == (1u64 << s.runner_affinity);

    let mut checks = alloc::vec::Vec::new();
    checks.push(("owner_pinned", s.owner_affinity != UNKNOWN_HART));
    checks.push(("runner_pinned", s.runner_affinity != UNKNOWN_HART));
    checks.push((
        "owner_in_schedulable",
        s.owner_affinity == UNKNOWN_HART || sched.get(s.owner_affinity),
    ));
    checks.push((
        "runner_in_schedulable",
        s.runner_affinity == UNKNOWN_HART || sched.get(s.runner_affinity),
    ));
    checks.push((
        "owner_runner_distinct",
        s.owner_affinity != s.runner_affinity,
    ));
    // Task 3.3 direct execution observations (Plan Review finding 4): the
    // poll-site-recorded tuples must agree with the pinned singleton placement.
    checks.push(("owner_executed_on_pinned", s.owner_last_hart == s.owner_affinity));
    checks.push((
        "runner_executed_on_pinned",
        s.runner_last_hart == s.runner_affinity,
    ));
    checks.push(("owner_mask_is_singleton", owner_singleton));
    checks.push(("runner_mask_is_singleton", runner_singleton));
    checks.push(("owner_events_observed", s.owner_events >= 1));
    checks.push(("runner_events_observed", s.runner_events >= 1));
    checks.push(("no_affinity_rejects", s.affinity_rejects == 0));

    let mut ok = true;
    for (name, pass) in &checks {
        ax_println!(
            "[NET-SMP-SMOKE] {} {}",
            if *pass { "ok  " } else { "BAD " },
            name
        );
        if !*pass {
            ok = false;
        }
    }
    ax_println!(
        "[NET-SMP-SMOKE] info owner_aff={} owner_last={} owner_mask={:#x} \
         owner_events={} runner_aff={} runner_last={} runner_mask={:#x} \
         runner_events={} irq_events={} ipi_sent={} ipi_received={} rejects={}",
        s.owner_affinity,
        s.owner_last_hart,
        s.owner_hart_mask,
        s.owner_events,
        s.runner_affinity,
        s.runner_last_hart,
        s.runner_hart_mask,
        s.runner_events,
        s.irq_events,
        s.ipi_sent,
        s.ipi_received,
        s.affinity_rejects
    );
    if ok {
        ax_println!("[NET-SMP-SMOKE] PASS");
    } else {
        ax_println!("[NET-SMP-SMOKE] FAIL");
    }
}

// ── QEMU-only controlled role migration (Task 4.3 / design D8) ───────
//
// 与 UART copier 迁移同一契约（共享 uart_migration_logic 的纯决策/状态机）：
// 已保存的 owner/runner handle（绝不新建第二实例）从 fixed singleton 加宽到
// {orig, second}，由 pinned 在 second 的一次性刺激任务经 axnet 自己的 nudge
// seam 发真实唤醒，同一 hart counter 直接观察到 second 上的 poll，随后恢复
// singleton 并再次观察。任何拒绝在执行任务状态变更前发生，且只增 rejects。

/// 网络后台角色（owner = RX queue owner；runner = stack runner）。
#[cfg(feature = "qemu")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NetRole {
    Owner,
    Runner,
}

/// QEMU-only 网络迁移控制的错误（拒绝路径全部保留旧状态）。
#[cfg(feature = "qemu")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetMigrateError {
    /// 同角色已有迁移在进行（single-flight）。
    Concurrent,
    /// 角色尚未启动（无 saved handle）。
    NoTask,
    /// 角色未固定（unknown origin hart）。
    NoOrigin,
    /// 目标选择被纯 seam 拒绝（已计入 rejects）。
    Target(super::uart_migration_logic::MigrateReject),
    /// two-hart mask 构造触及容量边界（防御路径）。
    MaskCapacity,
    /// `set_cpumask_checked` 拒绝（目标不在已发布 schedulable 集合）。
    NotSchedulable,
    /// 唤醒刺激任务 spawn 失败；singleton 已回滚。
    StimulusSpawnFailed,
    /// 有界等待内未在 second 上观察到直接 poll；singleton 已回滚。
    ObserveTimeout,
    /// 恢复 singleton 失败（防御路径，不应发生）。
    RestoreFailed,
}

#[cfg(feature = "qemu")]
use super::uart_migration_logic::{
    AUTO_HART, MigrationPhase, MigrationSlot, MigrationView, select_second,
};
#[cfg(feature = "qemu")]
static OWNER_MIGRATION: MigrationSlot = MigrationSlot::new();
#[cfg(feature = "qemu")]
static RUNNER_MIGRATION: MigrationSlot = MigrationSlot::new();

#[cfg(feature = "qemu")]
fn migration_slot(role: NetRole) -> &'static MigrationSlot {
    match role {
        NetRole::Owner => &OWNER_MIGRATION,
        NetRole::Runner => &RUNNER_MIGRATION,
    }
}

/// 该角色当前迁移视图（V5 组装与 smoke 只读）。
#[cfg(feature = "qemu")]
pub fn migration_view(role: NetRole) -> MigrationView {
    migration_slot(role).load()
}

#[cfg(feature = "qemu")]
fn pinned(role: NetRole) -> usize {
    match role {
        NetRole::Owner => owner_pinned_hart(),
        NetRole::Runner => runner_pinned_hart(),
    }
}

#[cfg(feature = "qemu")]
fn saved_task(role: NetRole) -> Option<axtask::AxTaskRef> {
    match role {
        NetRole::Owner => OWNER_TASK.lock().clone(),
        NetRole::Runner => RUNNER_TASK.lock().clone(),
    }
}

#[cfg(feature = "qemu")]
fn hart_tuple(role: NetRole) -> (usize, u64, u64) {
    match role {
        NetRole::Owner => axnet::owner_hart_tuple(),
        NetRole::Runner => axnet::runner_hart_tuple(),
    }
}

/// 经 axnet 自己的软件唤醒 seam 发真实唤醒（owner 与 runner 各自唯一）。
#[cfg(feature = "qemu")]
fn wake_role(role: NetRole) {
    match role {
        NetRole::Owner => axnet::software_nudge(),
        NetRole::Runner => axnet::runner_software_nudge(),
    }
}

#[cfg(feature = "qemu")]
fn two_hart_mask(orig: usize, second: usize) -> Result<axtask::AxCpuMask, NetMigrateError> {
    let mut mask = axtask::AxCpuMask::new();
    mask.set(orig, true).map_err(|_| NetMigrateError::MaskCapacity)?;
    mask.set(second, true)
        .map_err(|_| NetMigrateError::MaskCapacity)?;
    Ok(mask)
}

#[cfg(feature = "qemu")]
fn restore_singleton(
    task: &axtask::AxTaskRef,
    orig: usize,
) -> Result<(), NetMigrateError> {
    let mut single = axtask::AxCpuMask::new();
    single.set(orig, true).map_err(|_| NetMigrateError::MaskCapacity)?;
    if !task.set_cpumask_checked(single) {
        return Err(NetMigrateError::RestoreFailed);
    }
    Ok(())
}

#[cfg(feature = "qemu")]
fn singleton(hart: usize) -> axtask::AxCpuMask {
    let mut mask = axtask::AxCpuMask::new();
    mask.set(hart, true)
        .expect("placement hart < mask capacity by schedulable-set invariant");
    mask
}

/// QEMU-only 受控网络角色迁移（Task 4.3 / D8）。语义与
/// [`super::uart_smp_snapshot::migrate_copier`] 完全一致，仅角色与唤醒 seam
/// 不同。成功返回时角色已恢复 fixed singleton 并在原 hart 上观察到后续 poll。
#[cfg(feature = "qemu")]
pub fn migrate_role(role: NetRole, explicit: usize) -> Result<(), NetMigrateError> {
    let slot = migration_slot(role);
    if !slot.try_enter() {
        return Err(NetMigrateError::Concurrent);
    }
    let result = migrate_role_inner(role, explicit, slot);
    slot.exit();
    result
}

#[cfg(feature = "qemu")]
fn migrate_role_inner(
    role: NetRole,
    explicit: usize,
    slot: &MigrationSlot,
) -> Result<(), NetMigrateError> {
    let task = saved_task(role).ok_or(NetMigrateError::NoTask)?;
    let orig = pinned(role);
    if orig == UNKNOWN_HART {
        return Err(NetMigrateError::NoOrigin);
    }

    let second = match select_second(orig, explicit, &sched_hart_ids(), axtask::AX_CPU_MASK_CAPACITY)
    {
        Ok(h) => h,
        Err(e) => {
            let mut v = slot.load();
            v.rejects = v.rejects.saturating_add(1);
            slot.store(v);
            return Err(NetMigrateError::Target(e));
        }
    };

    let mask = two_hart_mask(orig, second)?;
    if !task.set_cpumask_checked(mask) {
        let mut v = slot.load();
        v.rejects = v.rejects.saturating_add(1);
        slot.store(v);
        return Err(NetMigrateError::NotSchedulable);
    }

    let (_, _, events_at_request) = hart_tuple(role);
    slot.store(MigrationView {
        phase: MigrationPhase::Widened as u8,
        from: orig,
        to: second,
        requested_polls: events_at_request,
        observed_polls: 0,
        rejects: slot.load().rejects,
    });

    // 真实唤醒刺激：pinned 在 second 的一次性任务经 axnet nudge seam 唤醒，
    // 直到控制流观察到 second 上的直接 poll（有界）。
    if axtask::spawn_with_name_affinity(
        move || {
            for _ in 0..64 {
                wake_role(role);
                let v = migration_slot(role).load();
                if v.observed_polls > v.requested_polls {
                    break;
                }
                axtask::yield_now();
            }
        },
        "net-migration-stimulus".into(),
        singleton(second),
    )
    .is_none()
    {
        let _ = restore_singleton(&task, orig);
        return Err(NetMigrateError::StimulusSpawnFailed);
    }

    // 有界等待 second 上的直接 poll 观察。
    const WAIT_LIMIT: u32 = 1_000_000;
    let mut waited = 0u32;
    let observed_events = loop {
        let (last, _, events) = hart_tuple(role);
        if events > events_at_request && last == second {
            break events;
        }
        waited += 1;
        if waited > WAIT_LIMIT {
            let _ = restore_singleton(&task, orig);
            return Err(NetMigrateError::ObserveTimeout);
        }
        axtask::yield_now();
    };
    let mut v = slot.load();
    v.observed_polls = observed_events;
    slot.store(v);

    restore_singleton(&task, orig)?;
    // 恢复后再次观察原 hart 上的后续 poll（每次探测重发唤醒，覆盖 Running
    // 时唤醒被丢弃的瞬态）。
    let mut waited = 0u32;
    loop {
        wake_role(role);
        let (last, _, events) = hart_tuple(role);
        if events > observed_events && last == orig {
            break;
        }
        waited += 1;
        if waited > WAIT_LIMIT {
            return Err(NetMigrateError::ObserveTimeout);
        }
        axtask::yield_now();
    }
    let mut v = slot.load();
    v.phase = MigrationPhase::Restored as u8;
    slot.store(v);
    Ok(())
}

/// owner 连续性元组：迁移窗口内必须不变的 descriptor/ledger/lifecycle
/// 字段（progress 字段 task_poll 单独要求严格递增）。
#[cfg(feature = "qemu")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnerContinuity {
    pub lifecycle: u64,
    pub owner_view: u64,
    pub fault: u64,
    pub last_error_stage: u64,
    pub last_error_code: u64,
    pub reaped: u64,
    pub refilled: u64,
    pub delivered: u64,
    pub non_ip_consumed: u64,
    pub isr_publish: u64,
    pub isr_wake: u64,
}

#[cfg(feature = "qemu")]
pub fn owner_continuity() -> OwnerContinuity {
    let s = axnet::rx_snapshot();
    OwnerContinuity {
        lifecycle: s.lifecycle,
        owner_view: s.owner,
        fault: s.fault,
        last_error_stage: s.last_error_stage,
        last_error_code: s.last_error_code,
        reaped: s.reaped,
        refilled: s.refilled,
        delivered: s.delivered,
        non_ip_consumed: s.non_ip_consumed,
        isr_publish: s.isr_publish,
        isr_wake: s.isr_wake,
    }
}

#[cfg(feature = "qemu")]
pub fn owner_polls() -> u64 {
    axnet::rx_snapshot().task_poll
}

/// runner 连续性元组（STACK_EVENT generation 属协议代次，刺激会推进它，
/// 因此不在连续性内；started/fault 不得变化）。
#[cfg(feature = "qemu")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunnerContinuity {
    pub started: u64,
    pub fault: u64,
}

#[cfg(feature = "qemu")]
pub fn runner_continuity() -> RunnerContinuity {
    let s = axnet::stack_snapshot();
    RunnerContinuity {
        started: s.started,
        fault: s.fault,
    }
}

#[cfg(feature = "qemu")]
pub fn runner_polls() -> u64 {
    axnet::stack_snapshot().task_poll
}

/// QEMU+SMP bounded 网络角色迁移 smoke（Task 4.3）：每个角色先跑非法目标
/// 负例（拒绝且全状态保留），再跑完整 widen/observe/restore 周期，校验
/// 迁移状态、fixed placement 恢复、进度与 lifecycle/ledger 连续性。
/// 打印 `[NET-MIG-SMOKE] PASS / FAIL / SKIP`，失败不 panic。
#[cfg(feature = "qemu")]
pub fn migration_boot_smoke() {
    use alloc::vec::Vec;

    if axhal::cpu_num() < 2 {
        ax_println!("[NET-MIG-SMOKE] SKIP reason=non-smp-boot");
        return;
    }

    let mut checks: Vec<(&str, bool)> = Vec::new();
    for role in [NetRole::Owner, NetRole::Runner] {
        let name = match role {
            NetRole::Owner => "owner",
            NetRole::Runner => "runner",
        };

        // 负例：容量边界显式目标被拒绝，mask/phase/连续性全部保留。
        let (cont_before, polls_before) = match role {
            NetRole::Owner => (
                owner_continuity().lifecycle,
                owner_polls(),
            ),
            NetRole::Runner => (runner_continuity().started, runner_polls()),
        };
        let rejects_before = migration_view(role).rejects;
        let neg = migrate_role(role, axtask::AX_CPU_MASK_CAPACITY);
        let v_neg = migration_view(role);
        checks.push((name, neg.is_err()));
        checks.push((name, v_neg.rejects == rejects_before + 1));
        checks.push((
            name,
            v_neg.phase == MigrationPhase::None as u8
                || v_neg.phase == MigrationPhase::Restored as u8,
        ));
        let (cont_after, polls_after) = match role {
            NetRole::Owner => (owner_continuity().lifecycle, owner_polls()),
            NetRole::Runner => (runner_continuity().started, runner_polls()),
        };
        checks.push((name, cont_before == cont_after && polls_after >= polls_before));

        // 有效周期。
        let cont_pre = match role {
            NetRole::Owner => Err(owner_continuity()),
            NetRole::Runner => Ok(runner_continuity()),
        };
        let polls_pre = match role {
            NetRole::Owner => owner_polls(),
            NetRole::Runner => runner_polls(),
        };
        let result = migrate_role(role, AUTO_HART);
        let v = migration_view(role);
        checks.push((name, result.is_ok()));
        checks.push((name, v.phase == MigrationPhase::Restored as u8));
        checks.push((name, v.observed_polls > v.requested_polls));
        checks.push((name, v.from != AUTO_HART && v.to != AUTO_HART && v.from != v.to));

        let (last, mask, events) = hart_tuple(role);
        checks.push((name, last == v.from));
        checks.push((
            name,
            mask & !((1u64 << v.from) | (1u64 << v.to)) == 0,
        ));
        checks.push((name, events > v.requested_polls));

        // 连续性：lifecycle/ledger 字段不变；poll 进度严格前进。
        let cont_ok = match (role, &cont_pre) {
            (NetRole::Owner, Err(c)) => owner_continuity() == *c,
            (NetRole::Runner, Ok(c)) => runner_continuity() == *c,
            _ => false,
        };
        let polls_post = match role {
            NetRole::Owner => owner_polls(),
            NetRole::Runner => runner_polls(),
        };
        checks.push((name, cont_ok));
        checks.push((name, polls_post > polls_pre));
    }

    // 两角色 fixed placement 仍互异。
    checks.push((
        "pair",
        owner_pinned_hart() != runner_pinned_hart(),
    ));

    let mut ok = true;
    for (name, pass) in &checks {
        ax_println!(
            "[NET-MIG-SMOKE] {} role={}",
            if *pass { "ok  " } else { "BAD " },
            name
        );
        if !*pass {
            ok = false;
        }
    }
    let o = migration_view(NetRole::Owner);
    let r = migration_view(NetRole::Runner);
    ax_println!(
        "[NET-MIG-SMOKE] info owner_phase={} owner_from={} owner_to={} owner_req={} \
         owner_obs={} owner_rej={} runner_phase={} runner_from={} runner_to={} \
         runner_req={} runner_obs={} runner_rej={}",
        o.phase,
        o.from,
        o.to,
        o.requested_polls,
        o.observed_polls,
        o.rejects,
        r.phase,
        r.from,
        r.to,
        r.requested_polls,
        r.observed_polls,
        r.rejects
    );
    if ok {
        ax_println!("[NET-MIG-SMOKE] PASS");
    } else {
        ax_println!("[NET-MIG-SMOKE] FAIL");
    }
}
