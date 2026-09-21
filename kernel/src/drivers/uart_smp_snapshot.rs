// kernel/src/drivers/uart_smp_snapshot.rs

//! QEMU-only UART SMP placement/progress 快照（design D6）。
//!
//! 提供两块内容：
//! - 始终编译的 telemetry 与 copier 记录：UART ISR 实际执行 hart、copier 每次
//!   poll 的实际执行 hart（通过一个简单的 future 包装器在每次 poll 时记录
//!   `this_cpu_id()`），以及 copier 被固定的 singleton 目标 hart。各来源由
//!   [`hart_counter::HartCounter`] 维护，保证 `read` 返回自洽的 (last, mask, events)
//!   视图；纯 Relaxed telemetry，不参与任何同步决定。
//! - `#[cfg(feature = "qemu")]` 的独立 `repr(C)` 快照 wire type 与组装函数：
//!   报告 configured/schedulable mask、RX/TX affinity、实际 IRQ/copier hart、
//!   ring occupancy/vacancy、四阶段 completion 与 remote-wake/IPI/拒绝计数。
//!   wire 布局定义在 [`uart_snapshot_types`]，与宿主工具共享同一份源码。
//!
//! 该快照是 QEMU 资格观测接口；D1 与普通构建不暴露它。旧 `UART_TXDBG_*` 命令
//! 的布局与语义不受影响。

#[cfg(feature = "qemu")]
use alloc::vec::Vec;
use core::{
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll},
};

#[cfg(feature = "qemu")]
use super::uart_migration_logic::{
    self, AUTO_HART, MigrateReject, MigrationPhase, MigrationView,
};

pub use super::uart_snapshot_types::UartSmpSnapshot;
use super::{
    hart_counter::{HartCounter, UNKNOWN_HART},
    uart_snapshot_types::UART_SMP_SNAPSHOT_MAGIC,
};

/// UART copier 方向，用于区分 RX/TX 的 hart 记录。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Copier {
    Rx,
    Tx,
}

// ── telemetry（Relaxed；纯观测，不驱动同步）────────────────────────────

static UART_IRQ: HartCounter = HartCounter::new();
static RX_COUNTER: HartCounter = HartCounter::new();
static TX_COUNTER: HartCounter = HartCounter::new();
static RX_PINNED: AtomicUsize = AtomicUsize::new(UNKNOWN_HART);
static TX_PINNED: AtomicUsize = AtomicUsize::new(UNKNOWN_HART);

/// Records that the UART device ISR ran on this hart (called from the kernel's
/// `uart_isr_wrapper`). Tracks a monotonic event counter, the last hart and the
/// cumulative hart mask via a self-consistent [`HartCounter`].
pub fn record_irq_hart() {
    let id = axhal::percpu::this_cpu_id();
    UART_IRQ.record(id);
}

/// Records the singleton hart a copier was pinned to at startup. Must be called
/// *before* the copier's first enqueue so a reader never sees a copier that
/// already polled without a known affinity.
pub fn record_pinned(dir: Copier, hart: usize) {
    match dir {
        Copier::Rx => RX_PINNED.store(hart, Ordering::Relaxed),
        Copier::Tx => TX_PINNED.store(hart, Ordering::Relaxed),
    }
}

/// Records that a copier polled (i.e. actually ran a poll cycle) on `this hart`,
/// tracking monotonic progress and the cumulative hart mask. Called from the
/// [`record_harts`] future wrapper on every poll.
fn record_copier_hart(dir: Copier) {
    let id = axhal::percpu::this_cpu_id();
    match dir {
        Copier::Rx => RX_COUNTER.record(id),
        Copier::Tx => TX_COUNTER.record(id),
    }
}

/// Wraps a copier future so that every poll observes the actual executing hart.
///
/// The driver stays platform-agnostic (its `os` trait is still the 2-trait
/// minimum); the observation is performed in the kernel layer that owns the
/// copier's enqueue. A pinned copier always polls on its singleton hart; a
/// later controlled migration may poll on either allowed hart, which this
/// records without inferring from affinity.
pub struct HartRecording<F> {
    dir: Copier,
    inner: F,
}

/// Wraps `inner` with hart observation for the given copier direction.
pub fn record_harts<F>(dir: Copier, inner: F) -> HartRecording<F> {
    HartRecording { dir, inner }
}

impl<F> Future for HartRecording<F>
where
    F: Future,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        record_copier_hart(self.dir);
        // SAFETY: `inner` is the only `!Unpin` projection and is never moved
        // after pinning; re-pinning the project from the un-pinned accessor is
        // sound and matches `core::pin`'s contract.
        let inner = unsafe { Pin::new_unchecked(&mut self.get_unchecked_mut().inner) };
        inner.poll(cx)
    }
}

// ── QEMU-only snapshot assembly ─────────────────────────────────────

#[cfg(feature = "qemu")]
fn mask_to_u64(mask: &axtask::AxCpuMask) -> u64 {
    let mut out = 0u64;
    for i in 0..axconfig::plat::MAX_CPU_NUM {
        if mask.get(i) {
            out |= 1u64 << i;
        }
    }
    out
}

/// Diagnostic byte pushed once in the startup window as the *sole* TX producer.
///
/// It travels: TX ring push → registered copier waker → cross-hart remote-ready IPI
/// → target copier resume → drain to hardware. It is a benign single NUL so an
/// accidental transmission does not inject terminal control sequences before the
/// TTY reader exists.
const REMOTE_WAKE_BYTE: u8 = 0;

/// QEMU+SMP bounded startup smoke (design D6 / Task 2.6): wait until both RX and
/// TX copiers have polled and parked, then validate two things:
///
/// 1. **Fixed-placement legality** — every affinity bit belongs to the schedulable
///    mask, RX/TX affinity are distinct, the actual copier poll harts equal the
///    pinned singleton, cumulative masks are exactly that singleton (never a second
///    instance), no invalid-affinity rejection, no panic.
/// 2. **Remote-wake causality (Cycle 002 / 2.6-R2)** — publish one bounded
///    diagnostic byte in the pre-TTY window as the sole TX producer; the parked TX
///    copier (pinned on a hart remote from this boot hart) must be resumed, which
///    requires a remote-ready IPI. The before/after snapshot must show `tx_polls`
///    increase, `ipi_sent`/`ipi_received` increase, the TX copier still on its
///    singleton hart and the ring/staged state converged (vacancy restored, staged
///    zero). A zero-IPI build (e.g. `axtask/ipi` not enabled) or no resume is a FAIL.
///
/// The smoke prints every checked field plus an unambiguous
/// `[UART-SMP-SMOKE] PASS / FAIL / SKIP`. It does not define the Iteration 004
/// guest data-plane protocol. It never panics on a failed check (a panic would be
/// indistinguishable from an independent error and would violate the "0 panic"
/// GREEN condition): a failed check prints the reason and returns.
#[cfg(feature = "qemu")]
pub fn snapshot_boot_smoke() {
    /// Bounded wait upper bound for each CPU-settling / wake-delivery phase.
    const WAIT_LIMIT: u32 = 1_000_000;

    // The smoke validates the *SMP=16* placement contract. Single-hart QEMU boots
    // necessarily co-locate the copiers (no remote wake) — skip, not FAIL. Native
    // kernels that pay no price skip too.
    if axhal::cpu_num() < 2 {
        ax_println!("[UART-SMP-SMOKE] SKIP reason=non-smp-boot");
        return;
    }
    // Other SMP counts (2/3/4/8) are legitimate placements but not the fixed
    // 16-hart qualification; they must not print a spurious qualification FAIL.
    if axhal::cpu_num() != 16 {
        ax_println!(
            "[UART-SMP-SMOKE] SKIP reason=non-16-smp configured={}",
            axhal::cpu_num()
        );
        return;
    }

    // Phase 1: wait until both copiers have polled *and* the TX ring is fully
    // quiesced (copier parked on an empty ring). Requiring the ring to be empty
    // on top of `copier_active == false` removes the race where the startup
    // benchmark's TX backlog is still draining when we sample the baseline:
    // sampling a partly-drained ring would make `vacancy == tx_vacancy_before`
    // unreachable once the copier finishes draining, spuriously FAILing the
    // smoke. A parked copier has drained the ring (it only Pends when empty),
    // but the two are read as separate atomics, so require both here.
    let mut waited = 0u32;
    loop {
        let (_, _, rx_polls) = RX_COUNTER.read();
        let (_, _, tx_polls) = TX_COUNTER.read();
        let tx_parked = !crate::drivers::uart_init::driver()
            .tx_completion()
            .copier_active;
        let tx_empty = crate::drivers::uart_init::driver().tx.is_empty();
        if rx_polls >= 1 && tx_polls >= 1 && tx_parked && tx_empty {
            break;
        }
        waited += 1;
        if waited > WAIT_LIMIT {
            ax_println!(
                "[UART-SMP-SMOKE] FAIL reason=copier-not-polled rx_polls={} tx_polls={} \
                 tx_parked={} tx_empty={}",
                rx_polls,
                tx_polls,
                tx_parked,
                tx_empty
            );
            ax_println!("[UART-SMP-SMOKE] FAIL");
            return;
        }
        axtask::yield_now();
    }

    // Phase 2: baseline snapshot *before* the publication.
    let before = snapshot();
    let tx_polls_before = before.tx_polls;
    let ipi_sent_before = before.ipi_sent;
    let ipi_received_before = before.ipi_received;
    let tx_vacancy_before = before.tx_vacancy;

    // Phase 3: publish exactly one diagnostic byte as the *sole* TX producer.
    // SAFETY: `bench_tx_push` requires no `AsyncUartWriter` to exist and no
    // other TX producer to overlap. The smoke runs in `entry::init` before the
    // TTY reader/writer is created, so the boot hart is currently the only TX
    // producer. The TX copier is pinned to a hart remote from this boot hart
    // (by placement `UartTx` is the role after `UartRx`, which anchors here),
    // so waking it is a genuine cross-hart remote-ready IPI.
    let pushed = unsafe { crate::drivers::uart_init::driver().bench_tx_push(&[REMOTE_WAKE_BYTE]) };

    // Phase 4: bounded wait for the TX copier to resume. Its poll count must
    // advance, which can only happen after the remote-ready IPI delivered.
    let mut waited2 = 0u32;
    loop {
        let (_, _, tx_polls) = TX_COUNTER.read();
        if tx_polls > tx_polls_before {
            break;
        }
        waited2 += 1;
        if waited2 > WAIT_LIMIT {
            ax_println!(
                "[UART-SMP-SMOKE] FAIL reason=tx-copier-not-resumed pushed={pushed} \
                 tx_polls_before={tx_polls_before} ipi_sent_before={ipi_sent_before} \
                 ipi_received_before={ipi_received_before}"
            );
            ax_println!("[UART-SMP-SMOKE] FAIL");
            return;
        }
        axtask::yield_now();
    }

    // Phase 5 (Cycle 003 / 2.6-R4): `tx_polls` grows at poll *entry*, before the
    // ring/staged/transmitter drain has actually completed. Reading the terminal
    // state right after resume would sample a partially-drained ring and
    // mis-report `tx_ring_converged`. Under the same single-producer pre-TTY
    // window, wait until the four-stage drain (`TxCompletion::is_drained`: ring
    // empty, copier inactive, staged zero, transmitter empty) holds and the ring
    // vacancy is restored to its pre-publication baseline.
    let mut waited3 = 0u32;
    loop {
        let c = crate::drivers::uart_init::driver().tx_completion();
        let vacancy = crate::drivers::uart_init::driver().tx.vacant_len() as u32;
        if c.is_drained() && vacancy == tx_vacancy_before {
            break;
        }
        waited3 += 1;
        if waited3 > WAIT_LIMIT {
            ax_println!(
                "[UART-SMP-SMOKE] FAIL reason=completion-not-converged pushed={pushed} \
                 ring_empty={} copier_active={} staged={} transmitter_empty={} vacancy={} \
                 vacancy_before={}",
                c.ring_empty,
                c.copier_active,
                c.staged_bytes,
                c.transmitter_empty,
                vacancy,
                tx_vacancy_before
            );
            ax_println!("[UART-SMP-SMOKE] FAIL");
            return;
        }
        axtask::yield_now();
    }

    let snap = snapshot();

    let sched = snap.schedulable_mask;
    let rx = snap.rx_affinity as u64;
    let tx = snap.tx_affinity as u64;
    let rx_last = snap.rx_last_hart as i64;
    let tx_last = snap.tx_last_hart as i64;

    let mut checks = Vec::new();
    checks.push(("is_valid_frame", snap.is_valid_frame()));
    checks.push(("configured==16", snap.configured_harts == 16));
    checks.push(("rx_aff_in_schedulable", (sched & (1u64 << rx)) != 0));
    checks.push(("tx_aff_in_schedulable", (sched & (1u64 << tx)) != 0));
    // AF_PLACED: fixed placement keeps the two copiers on distinct harts.
    checks.push(("rx_tx_affinity_distinct", rx != tx));
    // AF_ACTUAL: the copier actually polled on its pinned singleton hart and the
    // cumulative mask is exactly that single hart (never a second instance).
    checks.push((
        "rx_actual_matches_affinity",
        rx_last == rx as i64 && snap.rx_hart_mask == (1u64 << rx),
    ));
    checks.push((
        "tx_actual_matches_affinity",
        tx_last == tx as i64 && snap.tx_hart_mask == (1u64 << tx),
    ));
    checks.push(("rx_polled", snap.rx_polls >= 1));
    checks.push(("tx_polled", snap.tx_polls >= 1));
    checks.push(("no_affinity_rejects", snap.affinity_rejects == 0));

    // Cycle 002 / 2.6-R2: remote-wake causality. The pushed byte must have woken
    // the (remotely-pinned) TX copier via *both* an IPI send and receive, and the
    // copier must have resumed on its pinned hart and drained the ring back to the
    // baseline vacancy (converged staged). A zero-IPI build is a hard FAIL.
    checks.push(("remote_wake_byte_pushed", pushed == 1));
    checks.push(("tx_resumed_by_wake", snap.tx_polls > tx_polls_before));
    checks.push(("ipi_sent_causal", snap.ipi_sent > ipi_sent_before));
    checks.push((
        "ipi_received_causal",
        snap.ipi_received > ipi_received_before,
    ));
    checks.push((
        "tx_still_on_pinned_hart",
        snap.tx_last_hart as i64 == tx as i64 && snap.tx_hart_mask == (1u64 << tx),
    ));
    checks.push((
        "tx_ring_converged",
        snap.tx_vacancy == tx_vacancy_before && snap.staged_bytes == 0,
    ));

    let mut ok = true;
    for (name, pass) in &checks {
        ax_println!(
            "[UART-SMP-SMOKE] {} {}",
            if *pass { "ok  " } else { "BAD " },
            name
        );
        if !*pass {
            ok = false;
        }
    }

    ax_println!(
        "[UART-SMP-SMOKE] info configured={} sched={:#x} rx_aff={} rx_last={} rx_mask={:#x} \
         tx_aff={} tx_last={} tx_mask={:#x} irq_events={} rx_polls={} tx_polls={} ipi_sent={} \
         ipi_received={} tx_vacancy={} tx_vacancy_before={} staged={}",
        snap.configured_harts,
        sched,
        rx,
        rx_last,
        snap.rx_hart_mask,
        tx,
        tx_last,
        snap.tx_hart_mask,
        snap.irq_events,
        snap.rx_polls,
        snap.tx_polls,
        snap.ipi_sent,
        snap.ipi_received,
        snap.tx_vacancy,
        tx_vacancy_before,
        snap.staged_bytes
    );

    if ok {
        ax_println!("[UART-SMP-SMOKE] PASS");
    } else {
        ax_println!("[UART-SMP-SMOKE] FAIL");
    }
}

/// 组装一帧 QEMU-only UART SMP 快照。
///
/// 各 placement/progress 来源通过 [`HartCounter::read`] 返回自洽的 (last, mask,
/// events) 元组；`last` 与 `mask` 满足 `last ∈ mask` 内部不变量，事件计数单调
/// 归因 RX/TX 的 resume/poll 进度。affinity 在 copier 首次入队前已固定（见
/// `record_pinned`），因此读到实际 hart 时其目标 affinity 一定已知。
#[cfg(feature = "qemu")]
pub fn snapshot() -> UartSmpSnapshot {
    let driver = crate::drivers::uart_init::driver();
    let c = driver.tx_completion();
    let sched = axtask::schedulable_cpu_mask();

    let (irq_last, irq_mask, irq_events) = UART_IRQ.read();
    let (rx_last, rx_mask, rx_polls) = RX_COUNTER.read();
    let (tx_last, tx_mask, tx_polls) = TX_COUNTER.read();

    #[cfg(feature = "smp")]
    let (ipi_sent, ipi_received) = (
        axtask::ipi_sent_count() as u64,
        axtask::ipi_received_count() as u64,
    );
    #[cfg(not(feature = "smp"))]
    let (ipi_sent, ipi_received) = (0u64, 0u64);

    // 从显式 zeroed 帧出发，只覆盖真实字段；reserved 字节恒为零（见
    // `uart_snapshot_types` 的 wire 安全说明）。不整对象复制含隐式 padding 的 Rust
    // 对象给 guest；guest copy 走 `UartSmpSnapshot::wire_bytes`（见 ctl.rs）。
    let mut s = UartSmpSnapshot::zeroed();
    s.magic = UART_SMP_SNAPSHOT_MAGIC;
    s.configured_harts = axhal::cpu_num() as u32;
    s.schedulable_mask = mask_to_u64(&sched);
    s.rx_affinity = RX_PINNED.load(Ordering::Relaxed) as u64;
    s.tx_affinity = TX_PINNED.load(Ordering::Relaxed) as u64;
    s.irq_last_hart = {
        #[allow(clippy::cast_possible_wrap)]
        let v = irq_last as i32;
        v
    };
    s.irq_hart_mask = irq_mask;
    s.rx_last_hart = {
        #[allow(clippy::cast_possible_wrap)]
        let v = rx_last as i32;
        v
    };
    s.rx_hart_mask = rx_mask;
    s.tx_last_hart = {
        #[allow(clippy::cast_possible_wrap)]
        let v = tx_last as i32;
        v
    };
    s.tx_hart_mask = tx_mask;
    s.rx_occupancy = driver.rx.occupied_len() as u32;
    s.tx_vacancy = driver.tx.vacant_len() as u32;
    s.ring_empty = c.ring_empty as u8;
    s.copier_active = c.copier_active as u8;
    s.staged_bytes = c.staged_bytes as u32;
    s.transmitter_empty = c.transmitter_empty as u8;
    s.irq_events = irq_events;
    s.rx_polls = rx_polls;
    s.tx_polls = tx_polls;
    s.ipi_sent = ipi_sent;
    s.ipi_received = ipi_received;
    s.affinity_rejects = axtask::affinity_reject_count() as u64;
    #[cfg(feature = "qemu")]
    {
        let rx = RX_MIGRATION.load();
        s.rx_migration_state = rx.phase;
        s.rx_migration_from = rx.from as u32;
        s.rx_migration_to = rx.to as u32;
        s.rx_migration_requested_polls = rx.requested_polls;
        s.rx_migration_observed_polls = rx.observed_polls;
        s.rx_migration_rejects = rx.rejects;
        let tx = TX_MIGRATION.load();
        s.tx_migration_state = tx.phase;
        s.tx_migration_from = tx.from as u32;
        s.tx_migration_to = tx.to as u32;
        s.tx_migration_requested_polls = tx.requested_polls;
        s.tx_migration_observed_polls = tx.observed_polls;
        s.tx_migration_rejects = tx.rejects;
    }
    s
}

// ── QEMU-only controlled copier migration (Task 4.2 / design D8) ─────
//
// Each existing copier (saved handle, never a new instance) is widened from
// its fixed singleton to {orig, second}, naturally resumes on `second` through
// a genuine wake *issued by a task pinned to `second`*, is observed there by
// the same hart counter that records every real poll, and is restored to its
// singleton. Mask capacity safety (Task 4.1) and the published-schedulable
// validation are both enforced before any task state changes; every rejection
// preserves the previous mask and only increments the migration reject counter.

#[cfg(feature = "qemu")]
use super::uart_migration_logic::MigrationSlot;

/// QEMU-only migration control 的错误（拒绝路径全部保留旧状态）。
#[cfg(feature = "qemu")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrateError {
    /// 同方向已有迁移在进行（single-flight）。
    Concurrent,
    /// copier 尚未启动（无 saved handle）。
    NoTask,
    /// 角色未固定（unknown origin hart）。
    NoOrigin,
    /// 目标选择被纯 seam 拒绝（已计入 rejects）。
    Target(MigrateReject),
    /// two-hart mask 构造触及容量边界（选择 seam 已预校验的防御路径）。
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
static RX_MIGRATION: MigrationSlot = MigrationSlot::new();
#[cfg(feature = "qemu")]
static TX_MIGRATION: MigrationSlot = MigrationSlot::new();

#[cfg(feature = "qemu")]
fn migration(dir: Copier) -> &'static MigrationSlot {
    match dir {
        Copier::Rx => &RX_MIGRATION,
        Copier::Tx => &TX_MIGRATION,
    }
}

#[cfg(feature = "qemu")]
fn pinned_hart(dir: Copier) -> usize {
    match dir {
        Copier::Rx => RX_PINNED.load(Ordering::Relaxed),
        Copier::Tx => TX_PINNED.load(Ordering::Relaxed),
    }
}

#[cfg(feature = "qemu")]
fn counter(dir: Copier) -> &'static HartCounter {
    match dir {
        Copier::Rx => &RX_COUNTER,
        Copier::Tx => &TX_COUNTER,
    }
}

#[cfg(feature = "qemu")]
fn wake_copier(dir: Copier) {
    let driver = crate::drivers::uart_init::driver();
    match dir {
        Copier::Rx => driver.wake_rx_copier(),
        Copier::Tx => driver.wake_tx_copier(),
    }
}

/// Builds the two-hart affinity `{orig, second}` through the safe wrapper
/// (Task 4.1): any capacity error is propagated, never truncated.
#[cfg(feature = "qemu")]
fn two_hart_mask(orig: usize, second: usize) -> Result<axtask::AxCpuMask, MigrateError> {
    let mut mask = axtask::AxCpuMask::new();
    mask.set(orig, true).map_err(|_| MigrateError::MaskCapacity)?;
    mask.set(second, true)
        .map_err(|_| MigrateError::MaskCapacity)?;
    Ok(mask)
}

#[cfg(feature = "qemu")]
fn restore_singleton(
    dir: Copier,
    task: &axtask::AxTaskRef,
    orig: usize,
) -> Result<(), MigrateError> {
    let mut single = axtask::AxCpuMask::new();
    single.set(orig, true).map_err(|_| MigrateError::MaskCapacity)?;
    // orig 来自已发布 schedulable 集合，恢复必然通过；防御性仍检查。
    if !task.set_cpumask_checked(single) {
        return Err(MigrateError::RestoreFailed);
    }
    let _ = dir;
    Ok(())
}

/// Bounded wait for a direct poll observation satisfying `cond`; yields between
/// probes (the established smoke pattern, never a driver busy-wait).
#[cfg(feature = "qemu")]
fn wait_observed(
    dir: Copier,
    mut cond: impl FnMut(usize, u64) -> bool,
) -> Option<(usize, u64)> {
    const WAIT_LIMIT: u32 = 1_000_000;
    let mut waited = 0u32;
    loop {
        let (last, _, polls) = counter(dir).read();
        if cond(last, polls) {
            return Some((last, polls));
        }
        waited += 1;
        if waited > WAIT_LIMIT {
            return None;
        }
        axtask::yield_now();
    }
}

/// QEMU-only controlled migration of one existing copier (Task 4.2 / D8).
///
/// `explicit == AUTO_HART` selects the first published schedulable hart
/// different from the origin; an explicit target is validated for capacity,
/// self-target and schedulability *before* any task state changes. On success
/// the copier's affinity is widened, the task pinned to `second` issues the
/// driver's genuine copier wake until the copier is directly observed polling
/// on `second`, and the fixed singleton is restored and observed again. Any
/// failure rolls the mask back to the last valid singleton.
#[cfg(feature = "qemu")]
pub fn migrate_copier(dir: Copier, explicit: usize) -> Result<(), MigrateError> {
    let state = migration(dir);
    if !state.try_enter() {
        return Err(MigrateError::Concurrent);
    }
    let result = migrate_inner(dir, explicit, state);
    state.exit();
    result
}

#[cfg(feature = "qemu")]
fn migrate_inner(
    dir: Copier,
    explicit: usize,
    state: &MigrationSlot,
) -> Result<(), MigrateError> {
    let task = crate::drivers::uart_init::copier_task(dir).ok_or(MigrateError::NoTask)?;
    let orig = pinned_hart(dir);
    if orig == UNKNOWN_HART {
        return Err(MigrateError::NoOrigin);
    }

    let sched = axtask::schedulable_cpu_mask();
    let mut cpus = alloc::vec::Vec::new();
    for i in 0..axconfig::plat::MAX_CPU_NUM {
        if sched.get(i) {
            cpus.push(i);
        }
    }
    let second = match uart_migration_logic::select_second(
        orig,
        explicit,
        &cpus,
        axtask::AX_CPU_MASK_CAPACITY,
    ) {
        Ok(h) => h,
        Err(e) => {
            let mut v = state.load();
            v.rejects = v.rejects.saturating_add(1);
            state.store(v);
            return Err(MigrateError::Target(e));
        }
    };

    let mask = two_hart_mask(orig, second)?;
    if !task.set_cpumask_checked(mask) {
        let mut v = state.load();
        v.rejects = v.rejects.saturating_add(1);
        state.store(v);
        return Err(MigrateError::NotSchedulable);
    }

    let (_, _, polls_at_request) = counter(dir).read();
    state.store(MigrationView {
        phase: MigrationPhase::Widened as u8,
        from: orig,
        to: second,
        requested_polls: polls_at_request,
        observed_polls: 0,
        rejects: state.load().rejects,
    });

    // Genuine wake stimulus: a one-shot task pinned to `second` issues the
    // driver's copier wake until the control observes the poll there (bounded).
    if axtask::spawn_with_name_affinity(
        move || {
            for _ in 0..64 {
                wake_copier(dir);
                let v = migration(dir).load();
                if v.observed_polls > v.requested_polls {
                    break;
                }
                axtask::yield_now();
            }
        },
        "uart-migration-stimulus".into(),
        crate::drivers::uart_init::singleton_mask_for(second),
    )
    .is_none()
    {
        let _ = restore_singleton(dir, &task, orig);
        return Err(MigrateError::StimulusSpawnFailed);
    }

    let observed = wait_observed(dir, |last, polls| {
        polls > polls_at_request && last == second
    })
    .ok_or_else(|| {
        // Roll back to the last valid singleton on observation timeout.
        let _ = restore_singleton(dir, &task, orig);
        MigrateError::ObserveTimeout
    })?;
    let (_, observed_polls) = observed;
    let mut v = state.load();
    v.observed_polls = observed_polls;
    state.store(v);

    restore_singleton(dir, &task, orig)?;
    // Observe a later poll back on the origin hart. The wake is re-issued on
    // every probe: the copier may still be Running (e.g. finishing a poll
    // triggered by the stimulus) when the first wake fires, and a wake against
    // a Running task is dropped — the next probe's wake lands after it parks.
    // The wake from this control context is remote whenever the control runs
    // on a different hart, so a remote-ready IPI delivers it.
    let polls_at_restore = observed_polls;
    let mut waited = 0u32;
    loop {
        wake_copier(dir);
        let (last, _, polls) = counter(dir).read();
        if polls > polls_at_restore && last == orig {
            break;
        }
        waited += 1;
        if waited > 1_000_000 {
            return Err(MigrateError::ObserveTimeout);
        }
        axtask::yield_now();
    }
    let mut v = state.load();
    v.phase = MigrationPhase::Restored as u8;
    state.store(v);
    Ok(())
}

/// QEMU+SMP bounded controlled-migration smoke (Task 4.2): for each copier,
/// exercise an invalid target (reject-and-preserve), then a full
/// widen/observe/restore cycle, checking migration state, fixed-placement
/// restoration, progress and ring/staged continuity. Never panics on a failed
/// check; prints `[UART-MIG-SMOKE] PASS / FAIL / SKIP`.
#[cfg(feature = "qemu")]
pub fn migration_boot_smoke() {
    if axhal::cpu_num() < 2 {
        ax_println!("[UART-MIG-SMOKE] SKIP reason=non-smp-boot");
        return;
    }

    let mut checks: Vec<(&str, bool)> = Vec::new();
    for dir in [Copier::Rx, Copier::Tx] {
        let name = match dir {
            Copier::Rx => "rx",
            Copier::Tx => "tx",
        };

        // Negative case: an explicit target at the mask capacity is rejected
        // before any task state changes; mask, migration phase and ring state
        // are all preserved (rejects increments only).
        let pre = snapshot();
        let rejects_before = migration(dir).load().rejects;
        let neg = migrate_copier(dir, axtask::AX_CPU_MASK_CAPACITY);
        let post = snapshot();
        let post_neg = migration(dir).load();
        checks.push((name, neg.is_err()));
        checks.push((name, post_neg.rejects == rejects_before + 1));
        checks.push((
            name,
            post_neg.phase == MigrationPhase::None as u8
                || post_neg.phase == MigrationPhase::Restored as u8,
        ));
        let ring_same = match dir {
            Copier::Rx => {
                pre.rx_occupancy == post.rx_occupancy
                    && pre.tx_vacancy == post.tx_vacancy
                    && pre.staged_bytes == post.staged_bytes
            }
            Copier::Tx => {
                pre.tx_vacancy == post.tx_vacancy && pre.staged_bytes == post.staged_bytes
            }
        };
        checks.push((name, ring_same));
        checks.push((
            name,
            match dir {
                Copier::Rx => {
                    post.rx_last_hart as usize == post.rx_affinity as usize
                        && post.rx_hart_mask == (1u64 << post.rx_affinity)
                }
                Copier::Tx => {
                    post.tx_last_hart as usize == post.tx_affinity as usize
                        && post.tx_hart_mask == (1u64 << post.tx_affinity)
                }
            },
        ));

        // Valid cycle: widen -> observe on second -> restore -> observe on orig.
        let pre_ring = ring_state(dir);
        let result = migrate_copier(dir, AUTO_HART);
        let post_ring = ring_state(dir);
        checks.push((name, result.is_ok()));

        let v = migration(dir).load();
        checks.push((name, v.phase == MigrationPhase::Restored as u8));
        checks.push((name, v.observed_polls > v.requested_polls));
        checks.push((name, v.from != AUTO_HART && v.to != AUTO_HART && v.from != v.to));

        let (last, mask, polls) = counter(dir).read();
        checks.push((name, last == v.from));
        // 累计 poll mask 是历史量：迁移后必然包含 {from, to}；有界不变量是
        // 除迁移目标外不出现任何第三个 hart（无第二实例/漂移）。
        checks.push((name, mask & !((1u64 << v.from) | (1u64 << v.to)) == 0));
        checks.push((name, polls > v.requested_polls));
        // SPSC/ring/staged continuity across the whole migration.
        checks.push((name, pre_ring == post_ring));
    }

    let mut ok = true;
    for (name, pass) in &checks {
        ax_println!(
            "[UART-MIG-SMOKE] {} dir={}",
            if *pass { "ok  " } else { "BAD " },
            name
        );
        if !*pass {
            ok = false;
        }
    }
    let rx = RX_MIGRATION.load();
    let tx = TX_MIGRATION.load();
    ax_println!(
        "[UART-MIG-SMOKE] info rx_phase={} rx_from={} rx_to={} rx_req={} rx_obs={} rx_rej={} \
         tx_phase={} tx_from={} tx_to={} tx_req={} tx_obs={} tx_rej={}",
        rx.phase,
        rx.from,
        rx.to,
        rx.requested_polls,
        rx.observed_polls,
        rx.rejects,
        tx.phase,
        tx.from,
        tx.to,
        tx.requested_polls,
        tx.observed_polls,
        tx.rejects
    );
    if ok {
        ax_println!("[UART-MIG-SMOKE] PASS");
    } else {
        ax_println!("[UART-MIG-SMOKE] FAIL");
    }
}

/// (rx_occupancy, tx_vacancy, staged) — the continuity tuple for one direction.
#[cfg(feature = "qemu")]
fn ring_state(dir: Copier) -> (u32, u32, u32) {
    let s = snapshot();
    let _ = dir;
    (s.rx_occupancy, s.tx_vacancy, s.staged_bytes)
}
