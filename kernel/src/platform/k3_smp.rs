//! MS14 Iteration 000 native K3 multi-hart platform entry (`k3-smp` mode).
//!
//! One-shot, what-it-prints-is-the-verdict: the payload drives the three
//! platform-level witnesses the Iteration 000 acceptance needs and parks
//! (external RESET is the recovery path). The kernel never judges PASS —
//! the operator adjudicates from the serial wire. No GMAC/tty/userland:
//! the polling `k3` tree stays compiled but is never entered here.
//!
//! Stages (lines prefixed `[ms14]`):
//! - A (A1): one singleton task per hart 0..=15 performs a real timer wait
//!   on its own hart's wheel and reports the actual hart; the boot task
//!   supervises with a 5 s deadline and prints the completion mask.
//! - B (A2): two independent software wakes (WaitQueue notify) of a
//!   genuinely `Blocked` singleton target on hart 8; the target reports
//!   its per-hart reschedule-IPI count delta (the S_SOFT delivery
//!   witness), and both ack-then-deliver cycles are separate events.
//! - C (A3): the target masks its own `STIE` before blocking; the wake is
//!   a real source-1 injection (APLIC SETIPNUM → claim/EOI → handler →
//!   remote-ready IPI) with the target's timer dead. The target restores
//!   `STIE` on every executable branch and proves a fresh timer wait.
//! - D (A3): a remote caller (singleton on hart 1) masks/unmasks the
//!   source-1 route; only APLIC SETIE/CLRIE move (the boot-local EIE bit
//!   reads back unchanged), and delivery is proven by a NEW SETIPNUM after
//!   the re-open — never by a masked-window write.
//! - Final: boot timer sleep + summary line, then park (reset to recover).

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use core::time::Duration;

use axtask::TaskState;
use axtask::future::{block_on, sleep, timeout};

/// The cross-cluster witness target (covers the second CPU cluster; the
/// board's clusters are harts 0–7 and 8–15 with separate L2s).
const TARGET_HART: usize = 8;
/// The remote caller for the global-gate stage (first hart of cluster 0).
const REMOTE_HART: usize = 1;
/// Absolute supervision deadline for every stage (boot timer is alive).
const STAGE_DEADLINE: Duration = Duration::from_secs(5);
/// The timer vector (per-hart CSR control goes through `set_enable`).
const TIMER_VECTOR: usize = axplat_riscv64_k3::timer_irq::TIMER_IRQ_VECTOR;

static TARGET_WQ: axtask::WaitQueue = axtask::WaitQueue::new();
static SOURCE1_WQ: axtask::WaitQueue = axtask::WaitQueue::new();
static GATE_WQ: axtask::WaitQueue = axtask::WaitQueue::new();

static TASK_MASK: AtomicUsize = AtomicUsize::new(0);
static SOURCE1_DELIVERED: AtomicUsize = AtomicUsize::new(0);
static SOURCE1_HART: AtomicUsize = AtomicUsize::new(usize::MAX);
static GATE_MASKED: AtomicBool = AtomicBool::new(false);
static GATE_DONE: AtomicBool = AtomicBool::new(false);
static TARGET_TIMER_MASKED: AtomicBool = AtomicBool::new(false);
static TARGET_RESUMED: AtomicBool = AtomicBool::new(false);

fn report(line: &str) {
    ax_println!("{}", line);
}

/// Fails the stage, reports, and parks. The recovery path is external
/// RESET; no retry, no fallback.
fn stage_fault(stage: &str, detail: &str) -> ! {
    ax_println!("{}", &alloc::format!("[ms14] fault stage={stage} detail={detail}"));
    loop {
        riscv::asm::wfi();
    }
}

/// Supervised wait: polls `cond` with a real timer sleep so the boot task
/// stays schedulable and the stage deadline bounds every wait.
fn wait_until(cond: impl Fn() -> bool) -> bool {
    block_on(timeout(Some(STAGE_DEADLINE), async {
        loop {
            if cond() {
                return;
            }
            sleep(Duration::from_millis(10)).await;
        }
    }))
    .is_ok()
}

fn singleton(hart: usize) -> axtask::AxCpuMask {
    let mut mask = axtask::AxCpuMask::new();
    mask.set(hart, true).expect("hart index within mask capacity");
    mask
}

fn handler_target() {
    let hart = axhal::percpu::this_cpu_id();
    let before = axtask::ipi_received_count_by_hart(hart);
    TARGET_WQ.wait();
    // Resumed: report first, then restore the timer and prove a real wait.
    let after = axtask::ipi_received_count_by_hart(hart);
    report(&alloc::format!(
        "[ms14] target stage=b resumed hart_actual={hart} ipi_before={before} ipi_after={after}"
    ));
}

fn handler_target_timer_masked() {
    let hart = axhal::percpu::this_cpu_id();
    // Dead-timer discipline: mask the LOCAL STIE of this hart before any
    // blocking wait. Every executable branch after a resume restores it
    // first — a missing restore would leave the hart timer-dead forever.
    axhal::irq::set_enable(TIMER_VECTOR, false);
    TARGET_TIMER_MASKED.store(true, Ordering::Release);
    SOURCE1_WQ.wait();
    let after = axtask::ipi_received_count_by_hart(hart);
    // Restore FIRST (executable-branch contract), then prove the timer.
    axhal::irq::set_enable(TIMER_VECTOR, true);
    let slept = block_on(timeout(Some(Duration::from_secs(1)), sleep(Duration::from_millis(30))))
        .is_ok();
    report(&alloc::format!(
        "[ms14] target stage=c resumed hart_actual={hart} timer=masked wake=source1 ipi_after={after} timer_restored=1 fresh_sleep_ok={slept}"
    ));
    TARGET_RESUMED.store(true, Ordering::Release);
    if !slept {
        stage_fault("c", "post-restore timer sleep failed (STIE restore ineffective)");
    }
}

fn handler_gate_remote() {
    axhal::irq::set_enable(1, false);
    GATE_MASKED.store(true, Ordering::Release);
    GATE_WQ.wait();
    axhal::irq::set_enable(1, true);
    GATE_DONE.store(true, Ordering::Release);
}

fn source1_handler() {
    SOURCE1_DELIVERED.fetch_add(1, Ordering::AcqRel);
    SOURCE1_HART.store(axhal::percpu::this_cpu_id(), Ordering::Release);
    SOURCE1_WQ.notify_one(false);
}

fn spawn_singleton(f: fn(), name: &str, hart: usize) -> axtask::AxTaskRef {
    axtask::spawn_with_name_affinity(f, alloc::format!("{name}"), singleton(hart))
        .unwrap_or_else(|| stage_fault(name, "singleton spawn rejected (mask/schedulable)"))
}

/// Confirms the target really reached `Blocked` before the wake is sent.
fn confirm_blocked(task: &axtask::AxTaskRef, stage: &str) {
    if !wait_until(|| task.state() == TaskState::Blocked) {
        stage_fault(stage, "target never reached TaskState::Blocked");
    }
}

/// Stage A: one singleton timer-wait task per hart, completion mask 0xffff.
fn stage_a_tasks() {
    for hart in 0..axhal::cpu_num() {
        spawn_singleton(
            move || {
                let actual = axhal::percpu::this_cpu_id();
                let slept = block_on(timeout(Some(STAGE_DEADLINE), sleep(Duration::from_millis(50))))
                    .is_ok();
                report(&alloc::format!(
                    "[ms14] task stage=a hart_actual={actual} timer_wait_ok={slept}"
                ));
                let bit = 1usize << actual;
                let mask = TASK_MASK.fetch_or(bit, Ordering::AcqRel) | bit;
                if mask == (1usize << axhal::cpu_num()) - 1 {
                    TARGET_WQ.notify_one(false);
                }
            },
            "ms14-task",
            hart,
        );
    }
    let full = (1usize << axhal::cpu_num()) - 1;
    if !wait_until(|| TASK_MASK.load(Ordering::Acquire) == full) {
        stage_fault(
            "a",
            &alloc::format!(
                "task mask incomplete {:#x} (expect {full:#x})",
                TASK_MASK.load(Ordering::Acquire)
            ),
        );
    }
    report(&alloc::format!(
        "[ms14] mask stage=a tasks={:#x} harts={}",
        TASK_MASK.load(Ordering::Acquire),
        axhal::cpu_num()
    ));
}

/// Stage B: two independent software wakes of a Blocked hart-8 target.
fn stage_b_software_wakes() {
    for round in 1..=2 {
        let target = spawn_singleton(handler_target, "ms14-target-b", TARGET_HART);
        confirm_blocked(&target, "b");
        let before = axtask::ipi_received_count_by_hart(TARGET_HART);
        TARGET_WQ.notify_one(false);
        if !wait_until(|| target.state() == TaskState::Exited) {
            stage_fault("b", "target did not resume after the software wake");
        }
        let after = axtask::ipi_received_count_by_hart(TARGET_HART);
        report(&alloc::format!(
            "[ms14] wake stage=b round={round} ipi8_before={before} ipi8_after={after} delta>0={}",
            after > before
        ));
        if after <= before {
            stage_fault("b", "per-hart reschedule IPI count did not advance");
        }
    }
}

/// Stage C: source-1 injection wakes a dead-timer Blocked target on hart 8.
fn stage_c_source1_wake() {
    // Route registration happens here (boot hart): Detached source 1.
    if !axhal::irq::register(1, source1_handler) {
        stage_fault("c", "source-1 route registration rejected");
    }
    let target = spawn_singleton(handler_target_timer_masked, "ms14-target-c", TARGET_HART);
    if !wait_until(|| TARGET_TIMER_MASKED.load(Ordering::Acquire)) {
        stage_fault("c", "target never armed its timer mask");
    }
    confirm_blocked(&target, "c");
    let delivered_before = SOURCE1_DELIVERED.load(Ordering::Acquire);
    inject_source1();
    if !wait_until(|| TARGET_RESUMED.load(Ordering::Acquire)) {
        stage_fault(
            "c",
            "dead-timer target not resumed; STIE restore may be missing (reset to recover)",
        );
    }
    let delivered_after = SOURCE1_DELIVERED.load(Ordering::Acquire);
    report(&alloc::format!(
        "[ms14] source1 stage=c delivered={} irq_hart={} target_resumed=1",
        delivered_after - delivered_before,
        SOURCE1_HART.load(Ordering::Acquire),
    ));
    if delivered_after == delivered_before {
        stage_fault("c", "source-1 injection produced no claim");
    }
}

/// Stage D: the remote global gate — mask/unmask from hart 1, fresh proof.
/// The caller's-own-file immunity is host-witnessed (`k3-aia-host-harness`
/// remote tests with independent fake files); the board cannot read another
/// hart's IMSIC file (remote IMSIC MMIO stays out of scope), so the board
/// witnesses the route registers and the boot-local EIE — the actual
/// delivery gate — plus the fresh post-re-open delivery.
fn stage_d_remote_gate() {
    let remote = spawn_singleton(handler_gate_remote, "ms14-gate", REMOTE_HART);
    if !wait_until(|| GATE_MASKED.load(Ordering::Acquire)) {
        stage_fault("d", "remote mask never ran");
    }
    if boot_eie1() == 0 {
        stage_fault("d", "remote mask cleared the boot-local EIE bit");
    }
    let (scfg, tgt) = boot_route1_registers();
    GATE_WQ.notify_one(false);
    if !wait_until(|| GATE_DONE.load(Ordering::Acquire) || remote.state() == TaskState::Exited) {
        stage_fault("d", "remote unmask never ran");
    }
    if boot_eie1() == 0 {
        stage_fault("d", "remote unmask cleared the boot-local EIE bit");
    }
    let (scfg_after, tgt_after) = boot_route1_registers();
    if scfg_after != scfg || tgt_after != tgt {
        stage_fault("d", "route sourcecfg/target changed by a remote gate op");
    }
    report(&alloc::format!(
        "[ms14] gate stage=d masked_remote=done reopened boot_eie1=1 target={tgt_after:#x} unchanged=1"
    ));
    // Fresh delivery only: a NEW SETIPNUM after the re-open proves the
    // global gate; the masked window proves nothing and injects nothing.
    let delivered_before = SOURCE1_DELIVERED.load(Ordering::Acquire);
    inject_source1();
    if !wait_until(|| SOURCE1_DELIVERED.load(Ordering::Acquire) > delivered_before) {
        stage_fault("d", "no delivery after the remote re-open");
    }
    report(&alloc::format!(
        "[ms14] gate stage=d fresh_setipnum=1 delivered={} irq_hart={}",
        SOURCE1_DELIVERED.load(Ordering::Acquire) - delivered_before,
        SOURCE1_HART.load(Ordering::Acquire),
    ));
}

/// Injects one source-1 event through the APLIC SETIPNUM_LE register (the
/// stage-9-proven software trigger; never GENMSI).
fn inject_source1() {
    use axplat_riscv64_k3::aia::{self, AiaBackend};
    let mut backend = aia::HardwareBackend;
    backend.aplic_write(aia::APLIC_SETIPNUM_LE, 1);
}

/// Boot-local EIE bit for EID 1 (word 0, bit 1) via the production readback.
fn boot_eie1() -> u64 {
    use axplat_riscv64_k3::aia::{self, AiaBackend};
    let mut backend = aia::HardwareBackend;
    let rb = aia::readback(&mut backend, 1);
    rb.eie_word0 & 0b10
}

/// Boot route-1 configuration registers (sourcecfg mode + MSI target) via
/// the production readback: the fixed-route witness for the gate stage.
fn boot_route1_registers() -> (u32, u32) {
    use axplat_riscv64_k3::aia::{self, AiaBackend};
    let mut backend = aia::HardwareBackend;
    let rb = aia::readback(&mut backend, 1);
    (rb.sourcecfg, rb.target)
}

/// The native platform entry: runs on the boot hart after the runtime
/// finished init (all harts schedulable, timer registered, IRQs enabled).
pub fn run_native_platform() -> ! {
    let boot = axhal::percpu::this_cpu_id();
    report(&alloc::format!(
        "[ms14] mode=k3-smp begin harts={} boot_hart={boot}",
        axhal::cpu_num()
    ));
    // Fix the main task to the boot hart before any spawn: every test task
    // commits its singleton affinity before its first enqueue; nothing
    // migrates after placement.
    if !axtask::current().set_cpumask_checked(singleton(boot)) {
        stage_fault("init", "main task boot-hart pin rejected");
    }

    stage_a_tasks();
    stage_b_software_wakes();
    stage_c_source1_wake();
    stage_d_remote_gate();

    // Final: a fresh boot-hart timer wait proves the tick is alive after
    // every stage, then park (no soft-interrupt storm: the operator sees
    // the wire go silent).
    match block_on(timeout(Some(STAGE_DEADLINE), sleep(Duration::from_millis(100)))) {
        Ok(()) => report("[ms14] timer stage=final boot_sleep_ok=1"),
        Err(_) => stage_fault("final", "boot timer sleep failed"),
    }
    report("[ms14] summary stage=done platform=witness-complete park (reset to recover)");
    loop {
        riscv::asm::wfi();
    }
}
