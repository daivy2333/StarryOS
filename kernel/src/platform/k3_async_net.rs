//! K3 async GMAC raw data plane (MS12 Iteration 000, task T2): the
//! source-133 minimal ISR core and the first deferred-owner raw session.
//!
//! Layering (M36/D20, single hart): the ISR never borrows the driver, the
//! descriptors, the MDIO or the console — it runs the fixed order read
//! cause → save the complete pre-ack raw → latch the fatal → suppress on
//! fatal/unknown → W1C the observed known bits/summaries → publish one work
//! hint, and the axhal after-dispatch hook wakes the registered waker. The
//! native `axtask` task is the ONE descriptor owner: it registers the waker,
//! arms the device completion notify (IRQ_EN `0xD041`), rechecks the
//! descriptor heads and the event generation, and only then pends — the
//! arm/recheck window protocol closes event-before/during/after-arm without
//! polling. The raw payload sends the MS11 60-byte ARP request 20 times
//! (each with an independent 1 s terminal deadline); between the first 19
//! terminals and the last submit it awaits a native `sleep_until(100 ms)`
//! during which no descriptor poll runs — the wake is the timer's own. A
//! session cap of 30 s bounds everything; every failure prints a bounded
//! snapshot immediately (pre-ack cause, handler counts, IRQ_EN, the
//! synchronized head OWN words, the owner stage) and keeps the published
//! backing until external reset.
//!
//! The ISR core and the wait decision are host-testable through the
//! [`IrqAccess`] seam (board: raw MMIO + `axnet::publish_queue_event`;
//! host: fake register file + publish counter). No software IRQ, host
//! simulation or completion polling ever substitutes the real RI/TI.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};

#[cfg(feature = "k3")]
use crate::platform::k3_gmac as gmac;
#[cfg(not(feature = "k3"))]
use crate::k3_gmac as gmac;

/// The K3 GMAC device interrupt source (MS10 fixed route).
pub const GMAC_IRQ_SOURCE: usize = 133;

/// Status bits the ISR understands: the two completion causes, the two
/// capacity causes, the fatal bus error, the NIS/AIS summaries and the
/// board-adjudicated benign bit 10 (routine TX-completion companion on this
/// DWMAC 5.10a; R69 reference clears it as benign). Anything else in the raw
/// word is an unknown effective channel cause.
pub const KNOWN_STATUS_MASK: u32 = gmac::DMA_STATUS_RI
    | gmac::DMA_STATUS_TI
    | gmac::DMA_STATUS_RBU
    | gmac::DMA_STATUS_TBU
    | gmac::DMA_STATUS_FBE
    | gmac::DMA_STATUS_NIS
    | gmac::DMA_STATUS_AIS
    | gmac::DMA_STATUS_BIT10;

// ---------------------------------------------------------------------------
// IRQ-context access seam (the stage-10 `HandlerAccess` convention)
// ---------------------------------------------------------------------------

/// The one IRQ-context surface: GMAC register reads/writes and the ISR-safe
/// work-hint publish. The bare `fn()` handler receives no arguments, so the
/// board arms this table before registering the handler. It intentionally
/// exposes no descriptor, cache, lock or console capability.
pub struct IrqAccess {
    /// Reads one GMAC register at `offset` from `MAC_BASE_PADDR`.
    pub reg_read: fn(u32) -> u32,
    /// Writes one GMAC register at `offset` from `MAC_BASE_PADDR`.
    pub reg_write: fn(u32, u32),
    /// Publishes one queue work hint (production: the axnet queue event).
    pub publish_work: fn(),
}

static IRQ_ACCESS_PTR: AtomicUsize = AtomicUsize::new(0);

/// Arms the IRQ-context access table. Must run before the source-133
/// handler is registered; the pointer is never freed.
pub fn arm_irq_access(access: &'static IrqAccess) {
    IRQ_ACCESS_PTR.store(access as *const _ as usize, Ordering::Release);
}

fn armed_access() -> Option<&'static IrqAccess> {
    let ptr = IRQ_ACCESS_PTR.load(Ordering::Acquire) as *const IrqAccess;
    if ptr.is_null() {
        return None;
    }
    // SAFETY: armed once per boot from a `&'static` that lives for the
    // whole session; never freed.
    Some(unsafe { &*ptr })
}

// ---------------------------------------------------------------------------
// IRQ-context state (the waiter-visible observations)
// ---------------------------------------------------------------------------

struct GmacIrqState {
    /// Complete pre-ack `STATUS` raw of the latest non-zero handler run.
    last_raw: AtomicU32,
    /// Non-zero handler invocations (one per dispatched source-133 claim).
    handler_runs: AtomicU64,
    /// Fatal bus error latched before the ack.
    fatal: AtomicBool,
    /// Unknown effective channel cause latched (stable fault).
    unknown: AtomicBool,
    /// Handler runs whose raw carried each cause class (combinations count
    /// in every class they hit).
    ri: AtomicU64,
    ti: AtomicU64,
    abnormal: AtomicU64,
    /// Zero-cause dispatches (no device work, no publish).
    spurious: AtomicU64,
    /// Work-hint generation: +1 per publish. The waiter's recheck compares
    /// its snapshot against this to close the pre-Pending window.
    work_gen: AtomicU64,
}

impl GmacIrqState {
    const fn new() -> Self {
        GmacIrqState {
            last_raw: AtomicU32::new(0),
            handler_runs: AtomicU64::new(0),
            fatal: AtomicBool::new(false),
            unknown: AtomicBool::new(false),
            ri: AtomicU64::new(0),
            ti: AtomicU64::new(0),
            abnormal: AtomicU64::new(0),
            spurious: AtomicU64::new(0),
            work_gen: AtomicU64::new(0),
        }
    }
}

static IRQ_STATE: GmacIrqState = GmacIrqState::new();

/// Resets every IRQ-context observation (boot hygiene and host tests).
pub fn reset_irq_state() {
    IRQ_STATE.last_raw.store(0, Ordering::Relaxed);
    IRQ_STATE.handler_runs.store(0, Ordering::Relaxed);
    IRQ_STATE.fatal.store(false, Ordering::Relaxed);
    IRQ_STATE.unknown.store(false, Ordering::Relaxed);
    IRQ_STATE.ri.store(0, Ordering::Relaxed);
    IRQ_STATE.ti.store(0, Ordering::Relaxed);
    IRQ_STATE.abnormal.store(0, Ordering::Relaxed);
    IRQ_STATE.spurious.store(0, Ordering::Relaxed);
    IRQ_STATE.work_gen.store(0, Ordering::Relaxed);
}

/// The source-133 GMAC ISR (fixed order): read the cause → save the
/// complete pre-ack raw → latch the fatal → suppress on fatal/unknown →
/// W1C the observed known bits/summaries → publish one work hint. IRQ
/// context: no owner lock, no descriptor access, no allocation, no console.
pub fn gmac133_handler() {
    let Some(access) = armed_access() else {
        return; // not armed: nothing to service
    };
    let raw = (access.reg_read)(gmac::DMA_CHAN_STATUS);
    if raw == 0 {
        // Zero cause: counted as spurious only — no W1C, no mask, no work.
        IRQ_STATE.spurious.fetch_add(1, Ordering::Relaxed);
        return;
    }
    IRQ_STATE.last_raw.store(raw, Ordering::Release);
    IRQ_STATE.handler_runs.fetch_add(1, Ordering::Relaxed);
    // The fatal latches BEFORE the ack (the W1C below may clear FBE).
    if raw & gmac::DMA_STATUS_FBE != 0 {
        IRQ_STATE.fatal.store(true, Ordering::Release);
    }
    if raw & !KNOWN_STATUS_MASK != 0 {
        // Unknown effective channel cause: stable fault and a storm-stop
        // device mask (masks are stops only — MS10 adjudication).
        IRQ_STATE.unknown.store(true, Ordering::Release);
        (access.reg_write)(gmac::DMA_CHAN_IRQ_EN, 0);
    } else if raw & gmac::DMA_STATUS_FBE != 0 {
        // Fatal: suppress the device notify as well.
        (access.reg_write)(gmac::DMA_CHAN_IRQ_EN, 0);
    }
    // Cause counts read from the whole raw; combinations stay preserved in
    // `last_raw`.
    if raw & gmac::DMA_STATUS_RI != 0 {
        IRQ_STATE.ri.fetch_add(1, Ordering::Relaxed);
    }
    if raw & gmac::DMA_STATUS_TI != 0 {
        IRQ_STATE.ti.fetch_add(1, Ordering::Relaxed);
    }
    if raw & (gmac::DMA_STATUS_FBE | gmac::DMA_STATUS_RBU | gmac::DMA_STATUS_TBU) != 0 {
        IRQ_STATE.abnormal.fetch_add(1, Ordering::Relaxed);
    }
    // W1C exactly the observed known bits/summaries — never an unobserved
    // whole-word write; unknown bits stay observable in the register.
    let known = raw & KNOWN_STATUS_MASK;
    if known != 0 {
        (access.reg_write)(gmac::DMA_CHAN_STATUS, known);
    }
    // Work hint: RI/TI and stable fatal/valid-unknown each publish once so
    // errors wake the waiter too. Only a zero cause never publishes.
    (access.publish_work)();
    IRQ_STATE.work_gen.fetch_add(1, Ordering::Release);
}

/// Complete pre-ack `STATUS` raw of the latest non-zero handler run.
pub fn irq_last_raw() -> u32 {
    IRQ_STATE.last_raw.load(Ordering::Acquire)
}

/// Fatal bus error latched (before the ack).
pub fn irq_fatal() -> bool {
    IRQ_STATE.fatal.load(Ordering::Acquire)
}

/// Unknown effective channel cause latched.
pub fn irq_unknown() -> bool {
    IRQ_STATE.unknown.load(Ordering::Acquire)
}

/// Stable fault: a latched fatal or unknown cause.
pub fn irq_faulted() -> bool {
    irq_fatal() || irq_unknown()
}

/// `(ri, ti, abnormal, spurious)` cause counts.
pub fn irq_counts() -> (u64, u64, u64, u64) {
    (
        IRQ_STATE.ri.load(Ordering::Relaxed),
        IRQ_STATE.ti.load(Ordering::Relaxed),
        IRQ_STATE.abnormal.load(Ordering::Relaxed),
        IRQ_STATE.spurious.load(Ordering::Relaxed),
    )
}

/// Non-zero handler invocations (dispatched source-133 claims).
pub fn irq_handler_runs() -> u64 {
    IRQ_STATE.handler_runs.load(Ordering::Relaxed)
}

/// Work-hint generation (one publish per non-zero handler run).
pub fn irq_work_gen() -> u64 {
    IRQ_STATE.work_gen.load(Ordering::Acquire)
}

// ---------------------------------------------------------------------------
// The owner's wait decision (host-testable core)
// ---------------------------------------------------------------------------

/// Outcome of the arm/recheck wait decision for the deferred owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawWaitDecision {
    /// A stable fault is latched: propagate, never sleep.
    Fault,
    /// Work is visible (descriptor head or a published event): service.
    Work,
    /// Quiescent: the owner may pend on the registered waker.
    Sleep,
}

/// The pure decision: fault first, then descriptor work, then a work-hint
/// generation newer than the caller's snapshot. The snapshot must be taken
/// BEFORE the waker registration and the arm, so the recheck closes the
/// event-before/during/after-arm window and the pre-Pending publish.
pub fn raw_wait_decision(
    faulted: bool,
    gen_before: u64,
    rx_ready: bool,
    tx_ready: bool,
) -> RawWaitDecision {
    if faulted {
        return RawWaitDecision::Fault;
    }
    if rx_ready || tx_ready || irq_work_gen() != gen_before {
        RawWaitDecision::Work
    } else {
        RawWaitDecision::Sleep
    }
}

// ---------------------------------------------------------------------------
// Board session (riscv64 K3 async-net builds only)
// ---------------------------------------------------------------------------

#[cfg(all(feature = "k3", target_arch = "riscv64"))]
mod board {
    use super::*;
    use alloc::boxed::Box;
    use alloc::format;
    use core::future::poll_fn;
    use core::task::Poll;
    use core::time::Duration;

    use axtask::future::{block_on, sleep_until, timeout};
    use axalloc::GlobalPage;
    use axhal::time::wall_time;

    use crate::platform::k3_device_irq_probe::Stage10Outcome;
    use crate::platform::k3_network_probe::{stage12_arp_request, BoardBacking};
    use crate::platform::mac_probe::MAC_BASE_PADDR;
    use axplat_riscv64_k3::dma;

    use gmac::{
        adapter::K3NetAdapter, ArenaGuard, Gmac, GmacError, RxPoll, TxPoll, TxSubmit,
        TxSubmitError,
    };
    /// Fixed session contract (001-replan T2): 20 raw ARP frames, one
    /// independent 1 s terminal deadline per frame, a 25 s session cap and
    /// a 2048-byte RX scratch. (2026-10-14 user direction: every test
    /// observation window is 25 s.)
    pub const RAW_TX_FRAMES: usize = 20;
    pub const RAW_FRAME_DEADLINE_NANOS: u64 = 1_000_000_000;
    pub const RAW_INTERFRAME_NANOS: u64 = 100_000_000;
    pub const RAW_SESSION_NANOS: u64 = 25_000_000_000;
    pub const RAW_RX_SCRATCH: usize = 2048;
    /// Per service round: at most 16 poll steps per direction.
    const ROUND_BUDGET: usize = 16;
    /// Maximum per-frame sample lines.
    const SAMPLE_CAP: usize = 4;

    fn irq_reg_read(offset: u32) -> u32 {
        // MS14 Iteration 003 (T10): the GMAC window is reachable only through
        // the platform phys_to_virt mapping under the full paging stage;
        // IRQ context reads the STATUS/IRQ_EN words only.
        super::super::k3_mmio::read32(MAC_BASE_PADDR + offset as u64)
    }

    fn irq_reg_write(offset: u32, value: u32) {
        // Same window contract; the ISR writes only IRQ_EN/STATUS.
        super::super::k3_mmio::write32(MAC_BASE_PADDR + offset as u64, value)
    }

    /// The production work-hint publish: the axnet ISR-safe queue event
    /// (generation + waker, no Service/descriptor touch).
    fn irq_publish_work() {
        axnet::publish_queue_event();
    }

    // -----------------------------------------------------------------
    // MS13 T5: the runtime PHY link-event source (diagnostics builds)
    // -----------------------------------------------------------------

    /// The PHY INTN route pin (MS10 fixed route; armed per the board DTB).
    const PHY_IRQ_SOURCE: usize = 60;

    /// Source-60 IRQ-context observations (the handler's snapshot/hold/config
    /// record for the board A4 line). All `Relaxed`, observation-only.
    static PHY60_EVENTS: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
    static PHY60_MFPR_BEFORE: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    static PHY60_MFPR_SET: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);
    static PHY60_MFPR_REARM: core::sync::atomic::AtomicU32 = core::sync::atomic::AtomicU32::new(0);

    /// The latest source-60 handler evidence (events, MFPR35 before/set/rearm).
    pub fn phy60_counts() -> (u64, u32, u32, u32) {
        use core::sync::atomic::Ordering;
        (
            PHY60_EVENTS.load(Ordering::Relaxed),
            PHY60_MFPR_BEFORE.load(Ordering::Relaxed),
            PHY60_MFPR_SET.load(Ordering::Relaxed),
            PHY60_MFPR_REARM.load(Ordering::Relaxed),
        )
    }

    /// MS13 A4 diagnostics: the live device notify word (`DMA_CHAN_IRQ_EN`),
    /// read from task context through the same IRQ-access surface the ISR
    /// uses. `0x0` = the owner never armed; `0xd041` = armed.
    pub fn gmac_irq_en_snapshot() -> u32 {
        (BOARD_IRQ_ACCESS.reg_read)(gmac::DMA_CHAN_IRQ_EN)
    }

    /// MS13 A4 diagnostics: the live DMA channel status (a W1C register —
    /// the read is non-destructive). Latched RI/TI here while the handler
    /// stays silent splits the failure to the delivery layer.
    pub fn gmac_status_snapshot() -> u32 {
        (BOARD_IRQ_ACCESS.reg_read)(gmac::DMA_CHAN_STATUS)
    }

    /// MS13 001-rework T5-R1: one time-ordered discriminator line for the
    /// handover-window delivery fault. Pure observation — timer tick count,
    /// raw trap-state CSRs, the AIA claim counters and the live GMAC
    /// notify/status words — printed at the fixed probe/payload milestones
    /// without any sleep dependency.
    pub(super) fn discriminator_line(tag: &str, out: &mut dyn FnMut(&str)) {
        use axplat_riscv64_k3::aia::{AIA_DEBUG_LAST_CLAIM, AIA_DEBUG_TRAPS};
        use axplat_riscv64_k3::timer_irq::{TIMER_DEBUG_TICKS, debug_trap_csr_snapshot};
        let ticks = TIMER_DEBUG_TICKS.load(Ordering::Relaxed);
        let csr = debug_trap_csr_snapshot();
        let aia_traps = AIA_DEBUG_TRAPS.load(Ordering::Relaxed);
        let aia_claim = AIA_DEBUG_LAST_CLAIM.load(Ordering::Relaxed);
        line(
            out,
            format_args!(
                "[ms13] disc {tag} ticks={} sie={:#x} sip={:#x} sstatus={:#x} stie={} stip={} sext={} aia-traps={} aia-claim={:#x} irq-en={:#x} status={:#x}",
                ticks,
                csr.sie,
                csr.sip,
                csr.sstatus,
                (csr.sie >> 5) & 1,
                (csr.sip >> 5) & 1,
                (csr.sie >> 9) & 1,
                aia_traps,
                aia_claim,
                gmac_irq_en_snapshot(),
                gmac_status_snapshot(),
            ),
        );
    }

    /// The minimal source-60 PHY INTN handler (MS13 T5). IRQ context: the
    /// MFPR35 pad edge ack (set CLEAR → read back → clear CLEAR with every
    /// non-CLEAR bit preserved — the MS10 hold-in-clear adjudication), the
    /// edge evidence snapshot and one axnet CONFIG cause. No MDIO access,
    /// no locks, no console: the link STATE is the owner's BMSR read.
    pub fn phy_gpio60_handler() {
        use crate::platform::phy_probe;
        let mut env = super::super::k3::K3Env;
        let ev = phy_probe::pad35_edge_ack(&mut env);
        use core::sync::atomic::Ordering;
        PHY60_MFPR_BEFORE.store(ev.before, Ordering::Relaxed);
        PHY60_MFPR_SET.store(ev.set_readback, Ordering::Relaxed);
        PHY60_MFPR_REARM.store(ev.rearm_readback, Ordering::Relaxed);
        PHY60_EVENTS.fetch_add(1, Ordering::Relaxed);
        axnet::publish_config_event();
    }

    /// Arms the runtime PHY link-event source (MS13 T5, diagnostics builds
    /// only). The pad edge arm releases the MFPR hold-in-clear and enables
    /// the falling edge, the INER link-change enable rides the paged MDIO
    /// in owner-capable boot context, and the minimal handler is registered
    /// with its route opened. A failed arm degrades only the EVENT path —
    /// the evidence line makes it observable; the BMSR state reads and the
    /// data plane are independent of it.
    fn arm_phy_source60(env: &mut super::super::k3::K3Env, out: &mut dyn FnMut(&str)) -> bool {
        use crate::platform::phy_probe;
        let setup = phy_probe::pad35_edge_setup(env);
        let iner = phy_probe::iner_enable_link_change(env);
        let iner_ok = iner.is_ok();
        // MS13 rework (INTS/INTN ack): clear any cause latched before the
        // arm so INTN sits high and the NEXT link transition produces a
        // fresh pad edge; a value latched here predates this owner and is
        // evidence only (the handler re-reads link state, not causes).
        let insr = phy_probe::paged_read(env, phy_probe::PHY_PAGE_INSR, phy_probe::PHY_INSR_REG);
        // Stage10's observation handler is still registered on 60 (the boot
        // entry only retires 133). Retire it first — the route is closed
        // here, so no IRQ can fire between unregister and register.
        axhal::irq::unregister(PHY_IRQ_SOURCE);
        let registered = axhal::irq::register(PHY_IRQ_SOURCE, phy_gpio60_handler);
        if registered {
            axhal::irq::set_enable(PHY_IRQ_SOURCE, true);
        }
        let insr_desc = match insr.as_ref() {
            Ok(v) => alloc::format!("{v:#x}"),
            Err(e) => alloc::format!("err={e:?}"),
        };
        line(
            out,
            format_args!(
                "[ms13] phy60 arm mfpr={:#x}->{:#x} iner={} registered={} insr={}",
                setup.before, setup.readback, iner_ok, registered, insr_desc
            ),
        );
        iner_ok && registered
    }

    static BOARD_IRQ_ACCESS: IrqAccess = IrqAccess {
        reg_read: irq_reg_read,
        reg_write: irq_reg_write,
        publish_work: irq_publish_work,
    };

    /// One formatted report line through the caller's console sink.
    pub(super) fn line(out: &mut dyn FnMut(&str), args: core::fmt::Arguments<'_>) {
        out(&alloc::format!("{args}\n"));
    }

    /// Task-held session ledger: it survives future cancellation (the 1 s
    /// per-frame timeout drops futures, never state) and feeds the summary
    /// and the failure snapshots.
    struct SessionLedger {
        sent: usize,
        terminals: usize,
        tx_errors: usize,
        tx_timeouts: usize,
        rx_frames: usize,
        rx_drops: usize,
        deferred_polls: u64,
        timer_wakes: u64,
        armed: bool,
    }

    impl SessionLedger {
        fn new() -> Self {
            SessionLedger {
                sent: 0,
                terminals: 0,
                tx_errors: 0,
                tx_timeouts: 0,
                rx_frames: 0,
                rx_drops: 0,
                deferred_polls: 0,
                timer_wakes: 0,
                armed: false,
            }
        }
    }

    /// Bounded frame sample line (first few frames, raw facts only).
    fn sample_frame(
        out: &mut dyn FnMut(&str),
        rx_printed: &mut usize,
        slot: usize,
        len: usize,
        raw_w3: u32,
    ) {
        if *rx_printed < SAMPLE_CAP {
            line(
                out,
                format_args!(
                    "[ms12] net rx slot={} len={} raw={raw_w3:#010x}",
                    slot, len
                ),
            );
            *rx_printed += 1;
        }
    }

    /// One RX service round: at most 16 visible steps, Idle ends it.
    fn service_rx<B: gmac::ArenaBacking>(
        drv: &mut Gmac<B>,
        env: &mut super::super::k3::K3Env,
        scratch: &mut [u8; RAW_RX_SCRATCH],
        ledger: &mut SessionLedger,
        rx_printed: &mut usize,
        out: &mut dyn FnMut(&str),
    ) -> Result<(), GmacError> {
        for _ in 0..ROUND_BUDGET {
            match drv.poll_rx(env, scratch)? {
                RxPoll::Idle => break,
                RxPoll::Frame { len, slot, raw_w3 } => {
                    ledger.rx_frames += 1;
                    sample_frame(out, rx_printed, slot, len, raw_w3);
                }
                RxPoll::Dropped { slot, reason, raw_w3 } => {
                    ledger.rx_drops += 1;
                    if *rx_printed < SAMPLE_CAP {
                        line(
                            out,
                            format_args!(
                                "[ms12] net drop slot={slot} reason={reason:?} raw={raw_w3:#010x}"
                            ),
                        );
                        *rx_printed += 1;
                    }
                }
            }
        }
        Ok(())
    }

    /// One arm→register→recheck→pend cycle: the waker registers first
    /// (first registration enables the source-133 route and the
    /// after-dispatch hook), the device notify arms once, then the
    /// descriptor/generation recheck decides sleep vs service.
    async fn wait_for_wake<B: gmac::ArenaBacking>(
        drv: &mut Gmac<B>,
        env: &mut super::super::k3::K3Env,
        ledger: &mut SessionLedger,
    ) {
        let gen_before = irq_work_gen();
        ledger.deferred_polls += 1;
        poll_fn(|cx| {
            axtask::future::register_irq_waker(GMAC_IRQ_SOURCE, cx.waker());
            if !ledger.armed {
                drv.arm_notify(env, true);
                ledger.armed = true;
            }
            match drv.check_completions(env) {
                Ok(hint) => {
                    match raw_wait_decision(
                        irq_faulted(),
                        gen_before,
                        hint.rx_ready,
                        hint.tx_ready,
                    ) {
                        RawWaitDecision::Sleep => Poll::Pending,
                        RawWaitDecision::Work | RawWaitDecision::Fault => Poll::Ready(()),
                    }
                }
                // The fault surfaces through the caller's next poll/query.
                Err(_) => Poll::Ready(()),
            }
        })
        .await;
    }

    /// Terminal outcome of one raw frame.
    enum Terminal {
        Completed,
        LocalError { slot: usize, raw_w3: u32 },
        Timeout { slot: usize, raw_w3: u32 },
        Stopped,
    }

    /// Folds the ISR latch into the driver and prints the immediate
    /// snapshot; the session stops (faulted owner, backing retained).
    fn fold_and_snapshot<B: gmac::ArenaBacking>(
        drv: &mut Gmac<B>,
        env: &mut super::super::k3::K3Env,
        ledger: &SessionLedger,
        stage: &str,
        deadline: &str,
        out: &mut dyn FnMut(&str),
    ) {
        drv.latch_fault(env, irq_last_raw());
        snapshot(drv, env, ledger, stage, deadline, out);
    }

    /// Bounded failure snapshot (printed immediately, before any park):
    /// pre-ack cause, source-133 handler counts, IRQ_EN (diagnostic only),
    /// the synchronized head OWN words, tx_inflight and the ledger.
    fn snapshot<B: gmac::ArenaBacking>(
        drv: &mut Gmac<B>,
        env: &mut super::super::k3::K3Env,
        ledger: &SessionLedger,
        stage: &str,
        deadline: &str,
        out: &mut dyn FnMut(&str),
    ) {
        let (rx_head, tx_head) = drv.diag_head_words(env);
        let (ri, ti, abnormal, spurious) = irq_counts();
        line(
            out,
            format_args!(
                "[ms12] net fault stage={stage} deadline={deadline} pre-ack-raw={:#010x} fatal={} unknown={} irq-runs={} ri={} ti={} abnormal={} spurious={} irq-en={:#010x} rx-head={:#010x} tx-head={:#010x} tx-inflight={} sent={} terminals={} tx-errors={} tx-timeouts={} rx-frames={} rx-drops={} deferred-polls={} timer-wakes={} backing=retained",
                irq_last_raw(),
                irq_fatal() as u8,
                irq_unknown() as u8,
                irq_handler_runs(),
                ri,
                ti,
                abnormal,
                spurious,
                irq_reg_read(gmac::DMA_CHAN_IRQ_EN),
                rx_head,
                tx_head,
                drv.tx_inflight(),
                ledger.sent,
                ledger.terminals,
                ledger.tx_errors,
                ledger.tx_timeouts,
                ledger.rx_frames,
                ledger.rx_drops,
                ledger.deferred_polls,
                ledger.timer_wakes,
            ),
        );
    }

    /// The raw session (the ONE descriptor owner): 20 ARP submits, each with
    /// an independent 1 s terminal wait driven by the armed IRQ; between the
    /// first 19 terminals and the next submit a pure `sleep_until(100 ms)`
    /// runs with no descriptor poll. Returns `true` when all 20 frames
    /// reached their terminal; any bounded stop returns `false` after
    /// printing its snapshot.
    async fn raw_session<B: gmac::ArenaBacking>(
        drv: &mut Gmac<B>,
        env: &mut super::super::k3::K3Env,
        ledger: &mut SessionLedger,
        scratch: &mut [u8; RAW_RX_SCRATCH],
        out: &mut dyn FnMut(&str),
    ) -> bool {
        let frame = stage12_arp_request();
        let mut rx_printed = 0usize;
        for i in 0..RAW_TX_FRAMES {
            // Submit (Full → service RX and yield; the session cap bounds it).
            let slot = loop {
                match drv.try_submit(env, &frame) {
                    Ok(TxSubmit::Submitted { slot }) => break slot,
                    Ok(TxSubmit::Full) => {
                        if service_rx(
                            drv,
                            env,
                            scratch,
                            ledger,
                            &mut rx_printed,
                            out,
                        )
                        .is_err()
                        {
                            fold_and_snapshot(drv, env, ledger, "tx-full-service", "session", out);
                            return false;
                        }
                        axtask::yield_now();
                    }
                    Err(TxSubmitError::LengthInvalid { len }) => {
                        line(
                            out,
                            format_args!("[ms12] net fault stage=tx-length len={len} stop"),
                        );
                        return false;
                    }
                    Err(TxSubmitError::Driver(e)) => {
                        line(
                            out,
                            format_args!("[ms12] net fault stage=tx-submit err={e:?} stop"),
                        );
                        snapshot(drv, env, ledger, "tx-submit", "1s", out);
                        return false;
                    }
                }
            };
            ledger.sent += 1;
            line(out, format_args!("[ms12] net tx i={i} slot={slot}"));

            // Terminal wait: poll once, then pend on the armed IRQ with the
            // arm→register→recheck protocol; the enclosing 1 s timeout is
            // this frame's independent deadline.
            // Terminal wait (contract order): the wait protocol — register →
            // arm/recheck → pend when quiet — runs BEFORE the owner poll, so
            // a completion that lands after the check arrives as a real
            // source-133 RI/TI plus a deferred wake instead of a first-poll
            // win. A completion already visible at the check is reclaimed
            // without waiting (check-first discipline, no polling loop).
            let terminal = async {
                loop {
                    if irq_faulted() {
                        return Terminal::Stopped;
                    }
                    wait_for_wake(drv, env, ledger).await;
                    match drv.poll_tx(env) {
                        Ok(TxPoll::Completed { .. }) => return Terminal::Completed,
                        Ok(TxPoll::LocalError { slot, raw_w3 }) => {
                            return Terminal::LocalError { slot, raw_w3 }
                        }
                        Ok(TxPoll::TerminalTimeout { slot, raw_w3 }) => {
                            return Terminal::Timeout { slot, raw_w3 }
                        }
                        Ok(TxPoll::Pending { .. }) | Ok(TxPoll::Idle) => {
                            if let Err(e) = service_rx(
                                drv,
                                env,
                                scratch,
                                ledger,
                                &mut rx_printed,
                                out,
                            ) {
                                let _ = e;
                                return Terminal::Stopped;
                            }
                        }
                        Err(_) => return Terminal::Stopped,
                    }
                }
            };
            match timeout(Some(Duration::from_nanos(RAW_FRAME_DEADLINE_NANOS)), terminal).await {
                Ok(Terminal::Completed) => {
                    ledger.terminals += 1;
                }
                Ok(Terminal::LocalError { slot, raw_w3 }) => {
                    ledger.tx_errors += 1;
                    line(
                        out,
                        format_args!(
                            "[ms12] net tx i={i} err slot={slot} raw={raw_w3:#010x}"
                        ),
                    );
                }
                Ok(Terminal::Timeout { slot, raw_w3 }) => {
                    ledger.tx_timeouts += 1;
                    line(
                        out,
                        format_args!(
                            "[ms12] net tx i={i} timeout slot={slot} raw={raw_w3:#010x}"
                        ),
                    );
                }
                Ok(Terminal::Stopped) => {
                    fold_and_snapshot(drv, env, ledger, "tx-terminal", "1s", out);
                    return false;
                }
                Err(_elapsed) => {
                    // The 1 s future is cancelled; the ledger, the driver
                    // and the published backing survive it. Snapshot now.
                    snapshot(drv, env, ledger, "tx-terminal", "1s", out);
                    return false;
                }
            }

            if i + 1 < RAW_TX_FRAMES {
                // The one-shot inter-frame gap: a pure timer await with no
                // descriptor poll inside — the wake is the timer's own. The
                // peer's replies land in this window; servicing them after
                // the wake keeps the RX ledger real (and walks the wrap)
                // without any polling loop.
                ledger.timer_wakes += 1;
                sleep_until(wall_time() + Duration::from_nanos(RAW_INTERFRAME_NANOS)).await;
                if service_rx(drv, env, scratch, ledger, &mut rx_printed, out).is_err() {
                    fold_and_snapshot(drv, env, ledger, "interframe-service", "session", out);
                    return false;
                }
            }
        }
        true
    }

    fn park() -> ! {
        loop {
            riscv::asm::wfi();
        }
    }

    /// The retained-arena observation window (A3): the confirmed stop is
    /// observed for 25 s of real timer time with real peer input still
    /// arriving — new DMA write-back bytes must be zero. (2026-10-14 user
    /// direction: every test observation window is 25 s.)
    const STOPPED_WINDOW_NANOS: u64 = 25_000_000_000;

    /// One synchronized read of the whole arena through the device view:
    /// block-granular invalidate (every CPU line was cleaned at publish) →
    /// fence → volatile byte reads. Never writes.
    #[cfg(feature = "k3-net-diagnostics")]
    fn sync_arena_read(
        env: &mut super::super::k3::K3Env,
        base_va: usize,
        len: usize,
    ) -> alloc::vec::Vec<u8> {
        use gmac::GmacEnv;
        let mut off = 0usize;
        while off < len {
            env.cache_invalidate(base_va + off, dma::CACHE_BLOCK_BYTES);
            off += dma::CACHE_BLOCK_BYTES;
        }
        env.fence_rw();
        let mut v = alloc::vec::Vec::with_capacity(len);
        for i in 0..len {
            // SAFETY: the checked arena VA from the construction layout;
            // read-only observation of the retained backing.
            v.push(unsafe { ((base_va + i) as *const u8).read_volatile() });
        }
        v
    }

    /// MS13 Iteration 000 (T3): the native stop/rebuild witness. Runs in the
    /// probe build on the SAME Gmac/arena before the adapter/Service
    /// handover; the Service never runs alongside it. Baseline real ARP
    /// session (real RI/TI through the source-133 route, real RX frames
    /// from the pinging peer) → bounded stop/SWR → confirmed Stopped
    /// boundary with a 2 s retained-arena observation → in-place rebuild →
    /// the real ARP session again → notify/route closed for the handover.
    /// Every failure prints its stage and parks with the backing retained;
    /// a nonzero write-back count is a counter-indication and never hands
    /// the driver to the Service.
    #[cfg(feature = "k3-net-diagnostics")]
    fn native_recovery_probe<B: gmac::ArenaBacking>(
        drv: &mut Gmac<B>,
        env: &mut super::super::k3::K3Env,
        arena_va: usize,
        out: &mut dyn FnMut(&str),
    ) {
        use gmac::GmacEnv;
        // 1. Baseline: the existing real ARP session — real terminals and
        //    real RX frames are the peer-input premise for the observation.
        let mut ledger = SessionLedger::new();
        let mut scratch = [0u8; RAW_RX_SCRATCH];
        let baseline = raw_session(drv, env, &mut ledger, &mut scratch, out);
        match block_on(timeout(Some(Duration::from_nanos(RAW_SESSION_NANOS)), baseline)) {
            Ok(true) if ledger.rx_frames >= 1 => {}
            Ok(true) => {
                line(out, format_args!("[ms13] probe err=baseline-no-rx stop"));
                park();
            }
            Ok(false) => {
                line(out, format_args!("[ms13] probe err=baseline-session stop"));
                park();
            }
            Err(_elapsed) => {
                line(out, format_args!("[ms13] probe err=baseline-cap stop"));
                park();
            }
        }
        let (ri0, ti0, _, _) = irq_counts();
        line(
            out,
            format_args!(
                "[ms13] probe baseline sent={} terminals={} rx-frames={} irq-runs={} ri={} ti={}",
                ledger.sent,
                ledger.terminals,
                ledger.rx_frames,
                irq_handler_runs(),
                ri0,
                ti0,
            ),
        );
        discriminator_line("probe-baseline", out);

        // 2. Quiesce: close the notify and the source-133 route — the stop
        //    machine owns the device from here.
        drv.mask_notify(env);
        axhal::irq::set_enable(GMAC_IRQ_SOURCE, false);

        // 3. Bounded stop: arm the machine and step to the confirmed
        //    boundary; Pending polls are paced by the real timer inside the
        //    absolute 1 s window the machine enforces itself.
        match drv.runtime_begin_reset() {
            Ok(gmac::RuntimeRecovery::Resetting) => {}
            Ok(gmac::RuntimeRecovery::Rebuilding)
            | Ok(gmac::RuntimeRecovery::Failed) => {
                unreachable!("begin reports Resetting from the healthy Running state")
            }
            Err(e) => {
                line(out, format_args!("[ms13] probe err=begin {e:?} stop"));
                park();
            }
        }
        loop {
            match drv.runtime_poll_step(env) {
                Ok(gmac::RecoveryStep::Pending) => {
                    block_on(sleep_until(wall_time() + Duration::from_millis(10)));
                }
                Ok(gmac::RecoveryStep::Stepped { stage: gmac::RuntimeRecovery::Resetting }) => {}
                Ok(gmac::RecoveryStep::Stepped { stage: gmac::RuntimeRecovery::Rebuilding }) => {
                    break;
                }
                Ok(gmac::RecoveryStep::Stepped { stage: gmac::RuntimeRecovery::Failed }) => {
                    line(out, format_args!("[ms13] probe err=stop-failed stop backing=retained"));
                    park();
                }
                Ok(gmac::RecoveryStep::Recovered) => {
                    unreachable!("the rebuild cannot finish before the confirmation")
                }
                Err(e) => {
                    line(
                        out,
                        format_args!("[ms13] probe err=stop-machine {e:?} stop backing=retained"),
                    );
                    park();
                }
            }
        }

        // 4. The confirmed Stopped boundary: raw register facts, the
        //    synchronized old descriptor/owner summary, and the arena
        //    snapshot the write-back observation compares against.
        let mode = env.reg_read(gmac::regs::DMA_MODE);
        let rxctl = env.reg_read(gmac::DMA_RX_CTL);
        let txctl = env.reg_read(gmac::DMA_TX_CTL);
        let mac = env.reg_read(gmac::MAC_CONFIG);
        let status = env.reg_read(gmac::DMA_CHAN_STATUS);
        let (rx_head, tx_head) = drv.diag_head_words(env);
        line(
            out,
            format_args!(
                "[ms13] probe stopped mode={mode:#010x} rxctl={rxctl:#010x} txctl={txctl:#010x} mac={mac:#010x} status={status:#010x} sr={} st={} te={} re={} tx-inflight={} rx-head={rx_head:#010x} tx-head={tx_head:#010x}",
                rxctl & 1,
                txctl & 1,
                (mac >> 1) & 1,
                mac & 1,
                drv.tx_inflight(),
            ),
        );
        let before = sync_arena_read(env, arena_va, dma::ARENA_BYTES);

        // 5. The 2 s retained-arena observation: real input still arrives
        //    (the peer keeps pinging) while the driver touches nothing —
        //    new DMA write-back bytes must be zero.
        block_on(sleep_until(wall_time() + Duration::from_nanos(STOPPED_WINDOW_NANOS)));
        let after = sync_arena_read(env, arena_va, dma::ARENA_BYTES);
        let writeback = before.iter().zip(after.iter()).filter(|(a, b)| a != b).count();
        line(
            out,
            format_args!(
                "[ms13] probe stopped-window window-ms={} writeback-bytes={writeback}",
                STOPPED_WINDOW_NANOS / 1_000_000,
            ),
        );
        discriminator_line("stopped-window", out);
        if let Some(off) = before.iter().zip(after.iter()).position(|(a, b)| a != b) {
            line(
                out,
                format_args!(
                    "[ms13] probe writeback-at off={off} old={:#02x} new={:#02x} stop backing=retained",
                    before[off], after[off],
                ),
            );
            park();
        }

        // 6. In-place rebuild on the same arena: step to Running, then
        //    report the fixed capacity facts.
        let mut rebuilt_steps = 0usize;
        loop {
            match drv.runtime_poll_step(env) {
                Ok(gmac::RecoveryStep::Stepped { stage: gmac::RuntimeRecovery::Rebuilding }) => {
                    rebuilt_steps += 1;
                    if rebuilt_steps > 200 {
                        line(out, format_args!("[ms13] probe err=rebuild-unbounded stop"));
                        park();
                    }
                }
                Ok(gmac::RecoveryStep::Recovered) => break,
                Ok(gmac::RecoveryStep::Stepped { stage: gmac::RuntimeRecovery::Failed }) => {
                    line(
                        out,
                        format_args!("[ms13] probe err=rebuild-failed stop backing=retained"),
                    );
                    park();
                }
                Ok(gmac::RecoveryStep::Pending)
                | Ok(gmac::RecoveryStep::Stepped { stage: gmac::RuntimeRecovery::Resetting }) => {
                    unreachable!("the rebuild phase holds no SWR window")
                }
                Err(e) => {
                    line(
                        out,
                        format_args!("[ms13] probe err=rebuild-machine {e:?} stop backing=retained"),
                    );
                    park();
                }
            }
        }
        let mut rx_published = 0usize;
        for slot in 0..dma::NUM_SLOTS {
            let va = arena_va + dma::RX_RING_OFF + slot * dma::DESC_BYTES + 12;
            // SAFETY: checked arena VA, the word3 of RX descriptor `slot`.
            let w3 = unsafe { (va as *const u32).read_volatile() };
            if w3 & gmac::RDES3_OWN != 0 {
                rx_published += 1;
            }
        }
        line(
            out,
            format_args!(
                "[ms13] probe rebuilt state=running rx-published={rx_published} tx-capacity={} irq-en={:#010x}",
                drv.tx_free_capacity(),
                env.reg_read(gmac::DMA_CHAN_IRQ_EN),
            ),
        );
        discriminator_line("rebuilt", out);

        // 7. The rebuilt plane serves real traffic again (the same
        //    20-frame ARP session: at least one full bidirectional exchange
        //    with real RI/TI and a full TX terminal).
        let mut ledger2 = SessionLedger::new();
        let session2 = raw_session(drv, env, &mut ledger2, &mut scratch, out);
        match block_on(timeout(Some(Duration::from_nanos(RAW_SESSION_NANOS)), session2)) {
            Ok(true) if ledger2.rx_frames >= 1 => {}
            Ok(true) => {
                line(out, format_args!("[ms13] probe err=rebuilt-no-rx stop"));
                park();
            }
            Ok(false) => {
                line(out, format_args!("[ms13] probe err=rebuilt-session stop"));
                park();
            }
            Err(_elapsed) => {
                line(out, format_args!("[ms13] probe err=rebuilt-cap stop"));
                park();
            }
        }
        let (ri1, ti1, _, _) = irq_counts();
        line(
            out,
            format_args!(
                "[ms13] probe rebuilt-session sent={} terminals={} rx-frames={} irq-runs={} ri={} ti={}",
                ledger2.sent,
                ledger2.terminals,
                ledger2.rx_frames,
                irq_handler_runs(),
                ri1,
                ti1,
            ),
        );

        // 8. Close for the handover: notify masked, route re-opened — the
        //    Service owner arms through its own protocol; the handler stays
        //    registered (never re-registered). The stop step (2) CLOSED the
        //    source-133 AIA route, and nothing downstream re-opens it: the
        //    owner's arm/recheck protocol only opens the device IRQ_EN word,
        //    and the axhal first-registration enable already fired in the
        //    baseline session (the waker registry entry exists, so
        //    re-registration is a no-op) — the 001-rework discriminator run
        //    watched zero claims past this window with the route closed.
        //    Re-open the route here; the owner's armed notify takes over.
        drv.mask_notify(env);
        axhal::irq::set_enable(GMAC_IRQ_SOURCE, true);
        line(out, format_args!("[ms13] probe ok handover-next backing=retained"));
        discriminator_line("handover", out);
    }

    /// K3 board entry (the async-net boot branch): after a normally ended
    /// stage 10, mask the three device routes, retire the observation
    /// handler, build the IOC-mode driver with the notification closed,
    /// register the dedicated source-133 handler and run the raw session as
    /// the single deferred owner. Parks in every path; the published backing
    /// is retained until external reset. A failed gate/init/registration
    /// never opens the device notify.
    pub fn run_k3_async_net_session(st10: Stage10Outcome, mut out: impl FnMut(&str)) -> ! {
        if !matches!(st10, Stage10Outcome::WindowEnded(_)) {
            out("[ms12] net err=gate stage10-not-window-ended stop\n");
            park();
        }
        // Mask the three device routes and retire the stage-10 observation
        // handler; 60/277 stay registered but masked.
        for source in [60, GMAC_IRQ_SOURCE, 277] {
            axhal::irq::set_enable(source, false);
        }
        axhal::irq::unregister(GMAC_IRQ_SOURCE);
        // Arm the IRQ-context access BEFORE the handler registration: the
        // ISR may fire immediately after the route opens.
        arm_irq_access(&BOARD_IRQ_ACCESS);

        let mut page = match GlobalPage::alloc_contiguous(dma::ARENA_PAGES, dma::PAGE_SIZE_BYTES)
        {
            Ok(p) => p,
            Err(_) => {
                out("[ms12] net err=alloc stop\n");
                park();
            }
        };
        page.zero();
        let mut env = super::super::k3::K3Env;
        let mut drv = match Gmac::new_async(ArenaGuard::new(BoardBacking::new(page))) {
            Ok(d) => d,
            Err(e) => {
                line(&mut out, format_args!("[ms12] net err=arena {e:?} stop"));
                park();
            }
        };
        // IOC mode is established with the notification closed (init writes
        // IRQ_EN = 0); a failed init never opens the data plane.
        if let Err(e) = drv.init_rx(&mut env) {
            line(&mut out, format_args!("[ms12] net err=init {e:?} stop"));
            park();
        }
        drv.enable_tx(&mut env);
        let mut ledger = SessionLedger::new();
        if !axhal::irq::register(GMAC_IRQ_SOURCE, gmac133_handler) {
            out("[ms12] net err=register-133 stop\n");
            park();
        }
        // Open the device notify with the route already enabled (no mask
        // window): every completion raises RI/TI through the handler from the
        // first frame on, so the A3 cause counts do not depend on whether the
        // owner's first check beats the device.
        drv.arm_notify(&mut env, true);
        ledger.armed = true;
        line(
            &mut out,
            format_args!(
                "[ms12] net begin frames={} frame-deadline-ms={} interframe-ms={} session-cap-ms={} scratch={} irq-source={} arm=0xD041",
                RAW_TX_FRAMES,
                RAW_FRAME_DEADLINE_NANOS / 1_000_000,
                RAW_INTERFRAME_NANOS / 1_000_000,
                RAW_SESSION_NANOS / 1_000_000,
                RAW_RX_SCRATCH,
                GMAC_IRQ_SOURCE,
            ),
        );

        let mut scratch = [0u8; RAW_RX_SCRATCH];
        let session = raw_session(&mut drv, &mut env, &mut ledger, &mut scratch, &mut out);
        match block_on(timeout(Some(Duration::from_nanos(RAW_SESSION_NANOS)), session)) {
            Ok(true) => {
                // Session end: mask the device notify, stop new submits and
                // report. The published backing stays retained.
                drv.mask_notify(&mut env);
                axhal::irq::set_enable(GMAC_IRQ_SOURCE, false);
                let (ri, ti, abnormal, spurious) = irq_counts();
                line(
                    &mut out,
                    format_args!(
                        "[ms12] net result sent={} terminals={} tx-errors={} tx-timeouts={} rx-frames={} rx-drops={} timer-wakes={} irq-runs={} ri={} ti={} abnormal={} spurious={} deferred-polls={} backing=retained",
                        ledger.sent,
                        ledger.terminals,
                        ledger.tx_errors,
                        ledger.tx_timeouts,
                        ledger.rx_frames,
                        ledger.rx_drops,
                        ledger.timer_wakes,
                        irq_handler_runs(),
                        ri,
                        ti,
                        abnormal,
                        spurious,
                        ledger.deferred_polls,
                    ),
                );
            }
            Ok(false) => {
                // A bounded stop inside the session already printed its
                // snapshot; close the device notify and park.
                drv.mask_notify(&mut env);
            }
            Err(_elapsed) => {
                // The 30 s session cap elapsed: bounded stop with the
                // snapshot; the state ledger and backing survive the cancel.
                snapshot(&mut drv, &mut env, &ledger, "session-cap", "30s", &mut out);
                drv.mask_notify(&mut env);
            }
        }
        park()
    }

    /// The Service startup bound (design D4): the foreground boot task waits
    /// for the queue owner to reach `Active` with `yield_now` + `rx_snapshot`
    /// only — no nudge, no `poll_interfaces` — under a 5 s absolute cap.
    pub const SERVICE_START_NANOS: u64 = 5_000_000_000;

    /// One bounded stop line for the Service startup path (the owner never
    /// came up or died before Active); the published backing stays retained.
    fn service_start_stop(out: &mut dyn FnMut(&str), stage: &str, snap: axnet::RxSnapshot) {
        line(
            out,
            format_args!(
                "[ms12] net service stage={stage} stop lifecycle={} owner={} last-error-stage={} last-error-code={} backing=retained",
                snap.lifecycle, snap.owner, snap.last_error_stage, snap.last_error_code,
            ),
        );
    }

    /// K3 board entry for the Service data plane (MS12 Iteration 001, task
    /// T4; design D4 order): after a normally ended stage 10, mask the three
    /// device routes, retire the observation handler, build the IOC-mode
    /// driver with the notification CLOSED, register the dedicated
    /// source-133 handler, install the adapter as the only NIC through
    /// `init_network` and start exactly one pinned queue owner. The notify
    /// is never pre-armed: the owner's own `arm_notify_and_check` protocol
    /// opens it. The raw session below stays the diagnostic-only path and
    /// never runs alongside the owner. Parks in every path; the published
    /// backing is retained until external reset.
    pub fn run_k3_async_net_service(st10: Stage10Outcome, mut out: impl FnMut(&str)) -> ! {
        if !matches!(st10, Stage10Outcome::WindowEnded(_)) {
            out("[ms12] net err=gate stage10-not-window-ended stop\n");
            park();
        }
        // Mask the three device routes and retire the stage-10 observation
        // handler; 60/277 stay registered but masked (probe builds re-arm
        // 60 below).
        for source in [60, GMAC_IRQ_SOURCE, 277] {
            axhal::irq::set_enable(source, false);
        }
        axhal::irq::unregister(GMAC_IRQ_SOURCE);

        // Build the driver with the notification closed (init writes
        // IRQ_EN = 0); a failed gate/init never opens the data plane.
        let mut page = match GlobalPage::alloc_contiguous(dma::ARENA_PAGES, dma::PAGE_SIZE_BYTES)
        {
            Ok(p) => p,
            Err(_) => {
                out("[ms12] net err=alloc stop\n");
                park();
            }
        };
        page.zero();
        let mut env = super::super::k3::K3Env;
        // The probe's arena observation reads the retained backing through
        // this VA (the guard retains it until external reset).
        #[cfg(feature = "k3-net-diagnostics")]
        let arena_va = page.start_vaddr().as_usize();
        let mut drv = match Gmac::new_async(ArenaGuard::new(BoardBacking::new(page))) {
            Ok(d) => d,
            Err(e) => {
                line(&mut out, format_args!("[ms12] net err=arena {e:?} stop"));
                park();
            }
        };
        if let Err(e) = drv.init_rx(&mut env) {
            line(&mut out, format_args!("[ms12] net err=init {e:?} stop"));
            park();
        }
        drv.enable_tx(&mut env);

        // MS13 T3: the IRQ-context access and the one source-133 handler
        // are armed/registered BEFORE the native probe and reused by the
        // Service handover — never re-registered, never parallel with the
        // owner. The route stays closed until a waker registration opens it.
        arm_irq_access(&BOARD_IRQ_ACCESS);
        if !axhal::irq::register(GMAC_IRQ_SOURCE, gmac133_handler) {
            out("[ms12] net err=register-133 stop\n");
            park();
        }

        // MS13 T5 (probe builds only): the runtime PHY link-event source is
        // armed before the probe and the Service handover — MFPR35 falling
        // edge, INER link-change enable and the minimal source-60 handler.
        // The plain image keeps no PHY event behavior; its link snapshot is
        // still available through the adapter's BMSR read.
        #[cfg(feature = "k3-net-diagnostics")]
        {
            let mut env = super::super::k3::K3Env;
            arm_phy_source60(&mut env, &mut out);
        }

        // MS13 T3 probe build only: the native stop/rebuild witness runs on
        // the same Gmac/arena before the adapter exists; the plain async
        // build never runs it.
        #[cfg(feature = "k3-net-diagnostics")]
        native_recovery_probe(&mut drv, &mut env, arena_va, &mut out);

        let adapter = match K3NetAdapter::new(drv, env) {
            Ok(a) => a,
            Err(e) => {
                line(&mut out, format_args!("[ms12] net err=adapter {e:?} stop"));
                park();
            }
        };

        // Install the adapter as the ONLY NIC and start exactly one pinned
        // queue owner (spawn-free installation, D5; the owner activates on
        // its first poll by suppressing both directions and switching the
        // device to the slot data path).
        let nic: Box<dyn axdriver_net::NetDriverOps> = Box::new(adapter);
        axnet::init_network(axdriver::AxDeviceContainer::from_one(nic));
        match axnet::start_rx_task_affinity(axtask::schedulable_cpu_mask()) {
            Ok(_handle) => {}
            Err(e) => {
                line(
                    &mut out,
                    format_args!("[ms12] net err=start-owner {e:?} stop"),
                );
                park();
            }
        }

        // Foreground wait: yield + snapshot only, 5 s absolute cap. The
        // route opens through the owner's first waker registration.
        let deadline = wall_time() + Duration::from_nanos(SERVICE_START_NANOS);
        loop {
            axtask::yield_now();
            let snap = axnet::rx_snapshot();
            match snap.lifecycle {
                // 2 = Active: the data plane is owned, notify armed by the
                // owner's arm/recheck protocol.
                2 => break,
                // 3 = Faulted, 4 = Unavailable: a bounded stop, never a
                // retry, never a second owner.
                3 | 4 => {
                    service_start_stop(&mut out, "owner-not-active", snap);
                    park();
                }
                _ => {}
            }
            if wall_time() >= deadline {
                service_start_stop(&mut out, "start-deadline", axnet::rx_snapshot());
                park();
            }
        }
        let (hart, mask, events) = axnet::owner_hart_tuple();
        line(
            &mut out,
            format_args!(
                "[ms12] net service owner-active hart={hart} mask={mask:#x} events={events} irq-source={} arm=owner-protocol",
                GMAC_IRQ_SOURCE,
            ),
        );
        #[cfg(feature = "k3-net-diagnostics")]
        discriminator_line("owner-active", &mut out);

        // The T6 stack runner: the ONE resident protocol promoter (the MS06
        // contract). Failure stops — the data plane parks without a stack
        // and never retries the once-only start.
        if let Err(e) = axnet::start_stack_runner_affinity(axtask::schedulable_cpu_mask()) {
            line(
                &mut out,
                format_args!("[ms12] net err=start-stack {e:?} stop"),
            );
            park();
        }
        let stack = axnet::stack_snapshot();
        line(
            &mut out,
            format_args!(
                "[ms12] net service stack-active started={} task-poll={} rounds={} fallback={}",
                stack.started, stack.task_poll, stack.rounds, stack.fallback_poll,
            ),
        );

        // Active data plane: the diagnostics payload joins only in the
        // probe build (task T5); the plain async build parks here. Both
        // branches diverge.
        #[cfg(feature = "k3-net-diagnostics")]
        {
            super::k3_net_diagnostics::run_diagnostics(arena_va, &mut out);
            park()
        }
        #[cfg(not(feature = "k3-net-diagnostics"))]
        park()
    }

    // -----------------------------------------------------------------
    // MS14 Iteration 001 (T5): the k3-smp-net multi-hart mode — boot
    // entry, install and the placed owner/runner start. One-shot,
    // what-it-prints-is-the-verdict; parks in every path and the published
    // backing is retained until external reset.
    // -----------------------------------------------------------------

    /// Saved `AxTaskRef`s of the started owner/runner (design D5: the
    /// handles are retained; affinity commits before the first enqueue and
    /// nothing migrates afterwards). Never drained — the mode is one-shot.
    // MS14 Iteration 003 (T9): the install path is shared with the
    // k3-smp-combined rootfs entry; the k3-smp-net wrapper keeps consuming it
    // with its own qualification payload on top.
    #[cfg(any(feature = "k3-smp-net", feature = "k3-smp-combined"))]
    static SMP_NET_HANDLES: spin::Mutex<alloc::vec::Vec<axtask::AxTaskRef>> =
        spin::Mutex::new(alloc::vec::Vec::new());

    /// One bounded stop line + park for the SMP net boot (the recovery path
    /// is the external RESET; no retry, no fallback).
    #[cfg(any(feature = "k3-smp-net", feature = "k3-smp-combined"))]
    fn smp_fault(out: &mut dyn FnMut(&str), stage: &str, detail: &str) -> ! {
        line(
            out,
            format_args!("[ms14] net fault stage={stage} detail={detail} stop backing=retained"),
        );
        park()
    }

    /// A singleton CPU mask for one hart (fail at compile capacity only;
    /// schedulable membership is validated by the spawn/start seams).
    #[cfg(any(feature = "k3-smp-net", feature = "k3-smp-combined"))]
    fn smp_singleton(hart: usize) -> axtask::AxCpuMask {
        let mut mask = axtask::AxCpuMask::new();
        mask.set(hart, true).expect("hart index within mask capacity");
        mask
    }

    /// Installs the one NIC and starts its pinned owner/runner. Returns
    /// the retained arena VA after both roles are ready. The caller invokes
    /// this once on the boot hart; failed installation parks with backing
    /// retained, and the caller owns any later application assembly.
    ///
    /// MS14 Iteration 003 (T9): consumed by the `k3-smp-net` wrapper (which
    /// adds its own duplicate-start witness and qualification payload) and by
    /// the `k3-smp-combined` rootfs entry (which adds no qualification load).
    /// The `owner_hart`/`runner_hart` come from the caller's placement over
    /// the published schedulable set; the 5 s owner/runner startup bounds,
    /// the fail-closed parking and the retained backing semantics are this
    /// function's own.
    #[cfg(any(feature = "k3-smp-net", feature = "k3-smp-combined"))]
    pub fn install_smp_net(
        owner_hart: usize,
        runner_hart: usize,
        mut out: impl FnMut(&str),
    ) -> usize {
        // Stage 4–5: APMU GMAC bus clock enable + read-only MAC identity.
        let mut env = super::super::k3::K3Env;
        let apmu =
            super::super::mac_probe::apmu_clock_enable(&mut env, |l| out(l));
        let mac = super::super::mac_probe::probe_and_report(&mut env, |l| out(l));

        // Stage 6 gate: identity all-zero/all-one or ineffective APMU bits
        // stop everything (the same gate the polling path enforces).
        let (clk_ok, rst_ok) =
            super::super::mac_probe::apmu_bits_effective(apmu.after1, apmu.after2);
        let version_ok = mac.version_identity_ok();
        let version_label = if version_ok {
            "ok"
        } else {
            super::super::mac_probe::class_label(mac.version_classes[0])
        };
        let mut buf = [0u8; super::super::mac_probe::READ_LINE_BUF_LEN];
        out(super::super::mac_probe::format_gate_line(
            &mut buf,
            clk_ok,
            rst_ok,
            version_label,
            clk_ok && rst_ok && version_ok,
        ));
        if !(clk_ok && rst_ok && version_ok) {
            smp_fault(&mut out, "hw-gate", "apmu/mac identity gate failed");
        }

        // Stage 6–7: pad/GPIO and PHY identity, fail closed.
        match super::super::phy_probe::run_pad_gpio(&mut env, |l| out(l)) {
            super::super::phy_probe::PadGpioOutcome::Done => {}
            super::super::phy_probe::PadGpioOutcome::Stopped(_) => {
                smp_fault(&mut out, "hw-gate", "pad/gpio stage stopped");
            }
        }
        match super::super::phy_probe::run_phy_identity(&mut env, |l| out(l)) {
            super::super::phy_probe::PhyStatus::Ok => {}
            super::super::phy_probe::PhyStatus::Stopped(_) => {
                smp_fault(&mut out, "hw-gate", "phy identity stage stopped");
            }
        }

        // ── Install (the MS12/MS13 order, no stage 8–10 outcomes) ──
        // Mask the three device routes and retire the stage-10 observation
        // handler registration slot; 60/277 stay registered but masked
        // until the source60 arm below and the owner's own protocol.
        for source in [60, GMAC_IRQ_SOURCE, 277] {
            axhal::irq::set_enable(source, false);
        }
        axhal::irq::unregister(GMAC_IRQ_SOURCE);
        // Arm the IRQ-context access BEFORE the handler registration: the
        // ISR may fire immediately after the route opens.
        arm_irq_access(&BOARD_IRQ_ACCESS);

        let mut page = match GlobalPage::alloc_contiguous(dma::ARENA_PAGES, dma::PAGE_SIZE_BYTES)
        {
            Ok(p) => p,
            Err(_) => smp_fault(&mut out, "alloc", "arena allocation failed"),
        };
        page.zero();
        // The retained arena VA for the payload's descriptor observations
        // (the guard retains the backing until external reset).
        let arena_va = page.start_vaddr().as_usize();
        let mut env = super::super::k3::K3Env;
        let mut drv = match Gmac::new_async(ArenaGuard::new(BoardBacking::new(page))) {
            Ok(d) => d,
            Err(e) => smp_fault(&mut out, "arena", &alloc::format!("{e:?}")),
        };
        // IOC mode is established with the notification closed (init writes
        // IRQ_EN = 0); a failed init never opens the data plane.
        if let Err(e) = drv.init_rx(&mut env) {
            smp_fault(&mut out, "init", &alloc::format!("{e:?}"));
        }
        drv.enable_tx(&mut env);
        if !axhal::irq::register(GMAC_IRQ_SOURCE, gmac133_handler) {
            smp_fault(&mut out, "register-133", "route registration rejected");
        }
        // Design D6: the PHY link-event source is armed in this production
        // mode too — the same MS13 arm path (MFPR35 falling edge, INER
        // link-change enable, minimal source-60 handler). A failed arm
        // degrades only the event path and stays observable.
        {
            let mut env = super::super::k3::K3Env;
            if !arm_phy_source60(&mut env, &mut out) {
                line(
                    &mut out,
                    format_args!("[ms14] net note=source60-arm-degraded event-path-only"),
                );
            }
        }

        let adapter = match K3NetAdapter::new(drv, env) {
            Ok(a) => a,
            Err(e) => smp_fault(&mut out, "adapter", &alloc::format!("{e:?}")),
        };
        // Install the adapter as the ONLY NIC (spawn-free installation; the
        // owner activates on its first poll).
        let nic: Box<dyn axdriver_net::NetDriverOps> = Box::new(adapter);
        axnet::init_network(axdriver::AxDeviceContainer::from_one(nic));

        // Exactly one pinned owner start; the handle is retained (D5).
        let owner_handle = match axnet::start_rx_task_affinity(smp_singleton(owner_hart)) {
            Ok(h) => h,
            Err(e) => smp_fault(&mut out, "start-owner", &alloc::format!("{e:?}")),
        };
        SMP_NET_HANDLES.lock().push(owner_handle);

        // Foreground wait: yield + snapshot only, 5 s absolute cap. The
        // route opens through the owner's first waker registration.
        let deadline = wall_time() + Duration::from_nanos(SERVICE_START_NANOS);
        loop {
            axtask::yield_now();
            let snap = axnet::rx_snapshot();
            match snap.lifecycle {
                // 2 = Active: the data plane is owned, notify armed by the
                // owner's arm/recheck protocol.
                2 => break,
                // 3 = Faulted, 4 = Unavailable: a bounded stop, never a
                // retry, never a second owner.
                3 | 4 => {
                    line(
                        &mut out,
                        format_args!(
                            "[ms14] net service stage=owner-not-active stop lifecycle={} owner={} last-error-stage={} last-error-code={} backing=retained",
                            snap.lifecycle, snap.owner, snap.last_error_stage, snap.last_error_code,
                        ),
                    );
                    park()
                }
                _ => {}
            }
            if wall_time() >= deadline {
                let snap = axnet::rx_snapshot();
                line(
                    &mut out,
                    format_args!(
                        "[ms14] net service stage=start-deadline stop lifecycle={} owner={} backing=retained",
                        snap.lifecycle, snap.owner,
                    ),
                );
                park()
            }
        }
        let (owner_hart, owner_mask, owner_events) = axnet::owner_hart_tuple();
        line(
            &mut out,
            format_args!(
                "[ms14] net owner-active hart={owner_hart} mask={owner_mask:#x} events={owner_events} placement_owner={} irq-source={} arm=owner-protocol handles=retained",
                owner_hart, GMAC_IRQ_SOURCE,
            ),
        );

        // Exactly one pinned runner start; the handle is retained.
        let runner_handle =
            match axnet::start_stack_runner_affinity(smp_singleton(runner_hart)) {
                Ok(h) => h,
                Err(e) => smp_fault(&mut out, "start-stack", &alloc::format!("{e:?}")),
            };
        SMP_NET_HANDLES.lock().push(runner_handle);
        // Bounded wait for the runner's first real poll: the actual-hart
        // observation (UNKNOWN_HART = usize::MAX until the first poll).
        let deadline = wall_time() + Duration::from_nanos(SERVICE_START_NANOS);
        let runner_hart = loop {
            let (hart, _, _) = axnet::runner_hart_tuple();
            if hart != usize::MAX {
                break hart;
            }
            if wall_time() >= deadline {
                smp_fault(
                    &mut out,
                    "runner-first-poll",
                    "runner never polled within the startup bound",
                );
            }
            axtask::yield_now();
        };
        let stack = axnet::stack_snapshot();
        line(
            &mut out,
            format_args!(
                "[ms14] net stack-active hart={runner_hart} placement_runner={} started={} task-poll={} rounds={} fallback={} handles=retained",
                runner_hart, stack.started, stack.task_poll, stack.rounds, stack.fallback_poll,
            ),
        );

        arena_va
    }

    /// Existing NIC qualification entry over the shared installation path.
    #[cfg(feature = "k3-smp-net")]
    pub fn run_k3_smp_net_boot() -> ! {
        use super::super::early_console::{DwApbUart32EarlyConsole, EarlyConsole};
        use super::super::k3_smp_net::{place_smp_net_roles, placement};

        let desc = super::super::descriptor();
        // SAFETY: K3 UART0 MMIO is identity-mapped via the platform
        // mmio-ranges and left in the bootloader handoff state (MS09
        // Iteration 001 adjudication); this path runs once at boot.
        let console = unsafe { DwApbUart32EarlyConsole::from_config(&desc.console) };
        let mut out = |l: &str| console.write_str(l);

        let boot = axhal::percpu::this_cpu_id();
        line(
            &mut out,
            format_args!(
                "[ms14] net mode=k3-smp-net begin harts={} boot_hart={boot}",
                axhal::cpu_num(),
            ),
        );

        // Stages 1–3: the same console facts as the polling path; stage 3
        // names this mode instead of the polling park.
        console.write_str("[starry-k3] stage=1 entry-reached\n");
        console.write_str(
            "[starry-k3] stage=2 console uart=0xd4017000 stride=4 width=u32 baud=115200\n",
        );
        console.write_str("[starry-k3] stage=3 mode=k3-smp-net (stage 8-10 observation windows skipped)\n");

        // ── Placement: the shared policy decides, fail closed ──
        let sched_mask = axtask::schedulable_cpu_mask();
        let mut schedulable = alloc::vec::Vec::new();
        for hart in 0..placement::MAX_CPU_NUM {
            if sched_mask.get(hart) {
                schedulable.push(hart);
            }
        }
        let roles = match place_smp_net_roles(&schedulable, boot) {
            Ok(r) => r,
            Err(e) => smp_fault(&mut out, "placement", &alloc::format!("{e:?}")),
        };
        line(
            &mut out,
            format_args!(
                "[ms14] net placement owner={} runner={} app={} harts={} anchor={boot}",
                roles.net_owner,
                roles.net_runner,
                roles.app_hart(),
                schedulable.len(),
            ),
        );

        let arena_va = install_smp_net(roles.net_owner, roles.net_runner, &mut out);

        // A5: the once-only seams must refuse a second start — no second
        // role exists after either rejection.
        let dup_owner = axnet::start_rx_task_affinity(smp_singleton(roles.net_owner));
        let dup_runner = axnet::start_stack_runner_affinity(smp_singleton(roles.net_runner));
        let owner_rejected = dup_owner.is_err();
        let runner_rejected = dup_runner.is_err();
        let owner_err = dup_owner.err();
        let runner_err = dup_runner.err();
        line(
            &mut out,
            format_args!(
                "[ms14] net duplicate-start owner-rejected={owner_rejected} runner-rejected={runner_rejected} owner-err={owner_err:?} runner-err={runner_err:?}",
            ),
        );
        if !(owner_rejected && runner_rejected) {
            smp_fault(
                &mut out,
                "duplicate-start",
                "a once-only seam allowed a second role",
            );
        }

        // ── T6: the fixed-hart payload ──
        let app_hart = roles.app_hart();
        let spawned = axtask::spawn_with_name_affinity(
            move || {
                let mut out = |l: &str| ax_print!("{}", l);
                super::k3_net_diagnostics::run_smp_net_payload(arena_va, app_hart, &mut out);
            },
            alloc::format!("ms14-net-app"),
            smp_singleton(app_hart),
        );
        if spawned.is_none() {
            smp_fault(
                &mut out,
                "app-spawn",
                "affinity-committed app spawn rejected",
            );
        }
        line(
            &mut out,
            format_args!("[ms14] net app spawned hart={app_hart}"),
        );
        // The payload is internally bounded in every phase; the boot task
        // supervises with yields and never prints concurrently.
        while !super::k3_net_diagnostics::SMP_PAYLOAD_DONE.load(core::sync::atomic::Ordering::Acquire)
        {
            axtask::yield_now();
        }
        line(
            &mut out,
            format_args!("[ms14] net summary stage=payload-done park (reset to recover)"),
        );
        park()
    }
}

#[cfg(all(feature = "k3", target_arch = "riscv64"))]
pub use board::{run_k3_async_net_service, run_k3_async_net_session};

/// MS14 Iteration 001 (T5): the k3-smp-net multi-hart boot entry.
#[cfg(all(feature = "k3", target_arch = "riscv64", feature = "k3-smp-net"))]
pub use board::run_k3_smp_net_boot;

/// MS14 Iteration 003 (T9): the returnable single-instance NIC install for
/// the combined rootfs entry (same production path the k3-smp-net wrapper
/// consumes; it starts the owner/runner and returns the retained arena VA).
#[cfg(all(
    feature = "k3",
    target_arch = "riscv64",
    feature = "k3-smp-combined"
))]
pub use board::install_smp_net;

/// K3 capacity/flush diagnostics payload (MS12 Iteration 001, task T5).
///
/// Runs in the boot task after the queue owner reports `Active`. It drives
/// the transport-neutral queue-diagnostics control surface only: explicit
/// 200 ms reclaim holds (never a data-deadline length, always an explicit
/// release so the auto-release counter stays 0), minimal complete-frame
/// enqueues through the device's checked-ticket path (until the software
/// slot Full is refused — the hardware ring absorbs the first 16), and
/// direct reads of the committed ledgers. Every printed line is a native
/// observation; the payload never judges PASS (the operator adjudicates
/// from this wire).
#[cfg(all(feature = "k3", feature = "k3-net-diagnostics", target_arch = "riscv64"))]
mod k3_net_diagnostics {
    use alloc::format;
    use core::net::{IpAddr, Ipv4Addr, SocketAddr};
    use core::task::{Context, Poll};
    use core::time::Duration;

    use axhal::time::wall_time;
    use axerrno::AxError;
    use axnet::{
        options::Configurable, RecvOptions, SendOptions, Shutdown, Socket, SocketAddrEx,
        SocketOps,
    };
    use axnet::tcp::TcpSocket;
    use axnet::udp::UdpSocket;
    use axpoll::IoEvents;
    use axtask::future::{block_on, poll_io, timeout};

    use super::board::line;
    use super::{irq_counts, irq_handler_runs, irq_last_raw, GMAC_IRQ_SOURCE};

    /// The fixed TX slot capacity and the first refused frame index.
    const SLOT_CAPACITY: usize = 64;
    /// The fixed hardware descriptor capacity.
    const DESC_CAPACITY: usize = 16;
    /// The reclaim hold: below every data deadline and explicitly released.
    const HOLD_LEASE_MS: u64 = 200;
    /// Per-phase absolute wall cap (the payload stops, never wedges).
    /// 2026-10-14 user direction: every test observation window is 25 s.
    const PHASE_CAP_NANOS: u64 = 25_000_000_000;

    /// `HOLD_TX_RECLAIM` and `RELEASE`, mirroring the diag constants.
    const OP_HOLD_TX_RECLAIM: u64 = 2;
    const OP_RELEASE: u64 = 3;

    // ── The protocol session phase (T6/T7) ────────────────────────────────
    /// The fixed echo ports (T6 contract).
    const UDP_ECHO_PORT: u16 = 26001;
    const TCP_ECHO_PORT: u16 = 26002;
    /// The recovery-matrix ports (MS13 T7; distinct from the echo ports).
    const MATRIX_UDP_PORT: u16 = 26011;
    const MATRIX_TCP_PORT: u16 = 26012;
    const MATRIX_WAITER_PORT: u16 = 26013;
    /// Per-operation bound: every session op is 5 s bounded (T6 contract).
    const SESSION_OP_NANOS: u64 = 5_000_000_000;
    /// Operator-paced session wait (2026-10-14 user direction: 25 s).
    const SESSION_WAIT_NANOS: u64 = 25_000_000_000;
    /// The quiet window: no new cause and no due protocol timer (T7;
    /// 2026-10-14 user direction: 25 s).
    const QUIET_WINDOW_NANOS: u64 = 25_000_000_000;
    /// The bounded wait for the operator's post-quiet packet (T7; 25 s).
    const RESUME_WAIT_NANOS: u64 = 25_000_000_000;
    /// Settle stillness span: the source-133 claim stream must be quiet for
    /// this long before the freeze window opens (V3 round-1: the peer's TCP
    /// close tail otherwise lands inside the quiet window).
    const SETTLE_QUIET_NANOS: u64 = 500_000_000;
    /// Absolute cap for the settle wait (the payload stops, never wedges).
    const SETTLE_CAP_NANOS: u64 = 25_000_000_000;

    fn out_line(out: &mut dyn FnMut(&str), args: core::fmt::Arguments<'_>) {
        line(out, args);
    }

    /// One manual flush poll with a no-op waker (the payload re-polls; no
    /// wake routing is needed).
    fn flush_poll<F>(fut: &mut core::pin::Pin<&mut F>) -> Poll<axdriver_net::DevResult>
    where
        F: core::future::Future<Output = axdriver_net::DevResult>,
    {
        let mut cx = Context::from_waker(core::task::Waker::noop());
        fut.as_mut().poll(&mut cx)
    }

    /// Yields until `cond` over fresh v3 snapshots or the wall cap; the
    /// final snapshot is returned either way and the caller reports it.
    fn wait_v3(
        out: &mut dyn FnMut(&str),
        stage: &str,
        mut cond: impl FnMut(&axnet::RxSnapshotV3) -> bool,
    ) -> axnet::RxSnapshotV3 {
        let deadline = wall_time() + Duration::from_nanos(PHASE_CAP_NANOS);
        loop {
            let snap = axnet::rx_snapshot_v3();
            if cond(&snap) {
                return snap;
            }
            if wall_time() >= deadline {
                out_line(
                    out,
                    format_args!(
                        "[ms12] probe stage={stage} timeout lifecycle={} tx-slot={} desc-inflight={}",
                        snap.lifecycle, snap.tx_slot_occupancy, snap.tx_descriptor_inflight,
                    ),
                );
                return axnet::rx_snapshot_v3();
            }
            axtask::yield_now();
        }
    }

    /// One product-side observation line (T7): cause, drop, occupancy,
    /// budget, flush and quiet counters read directly from the committed
    /// ledgers — raw facts only, the payload never judges PASS. MS13 T5
    /// adds the source-60 PHY event evidence (events, latest MFPR35 ack
    /// readback) so the board session observes the link-event path directly.
    fn snapshot_line(out: &mut dyn FnMut(&str), tag: &str) {
        let s = axnet::rx_snapshot_v3();
        let st = axnet::stack_snapshot();
        let (ri, ti, abnormal, spurious) = irq_counts();
        let (phy60_events, _before, phy60_set, phy60_rearm) = super::board::phy60_counts();
        let irq_en = super::board::gmac_irq_en_snapshot();
        let status = super::board::gmac_status_snapshot();
        let arena = PAYLOAD_ARENA_VA.load(core::sync::atomic::Ordering::Relaxed);
        let mut rx_done = 0usize;
        if arena != 0 {
            for slot in 0..16 {
                let va =
                    arena + axplat_riscv64_k3::dma::RX_RING_OFF + slot * axplat_riscv64_k3::dma::DESC_BYTES + 12;
                // SAFETY: checked arena VA; word3 of RX descriptor `slot`.
                let w3 = unsafe { (va as *const u32).read_volatile() };
                if w3 & super::gmac::RDES3_OWN == 0 {
                    rx_done += 1;
                }
            }
        }
        let aia_traps = axplat_riscv64_k3::aia::AIA_DEBUG_TRAPS
            .load(core::sync::atomic::Ordering::Relaxed);
        let aia_claim = axplat_riscv64_k3::aia::AIA_DEBUG_LAST_CLAIM
            .load(core::sync::atomic::Ordering::Relaxed);
        let ticks = axplat_riscv64_k3::timer_irq::TIMER_DEBUG_TICKS
            .load(core::sync::atomic::Ordering::Relaxed);
        let csr = axplat_riscv64_k3::timer_irq::debug_trap_csr_snapshot();
        line(
            out,
            format_args!(
                "[ms12] probe {tag} cause=raw={:#010x} irq-runs={} ri={} ti={} abnormal={} spurious={} drop=malformed-ip={} no-route={} src-mismatch={} rx-full={} tx-full={} occupancy=rx-slot={} tx-slot={} desc-inflight={} budget=reclaim-ex={} rx-ex={} submit-ex={} flush=target={} ok={} err={} busy={} cancel={} quiet=task-poll={} rounds={} work={} self-yield={} timer-wake={} fallback={} lifecycle={} owner={} phy60=events={} set={:#x} rearm={:#x} irq-en={:#x} status={:#x} rx-done={} aia-traps={} aia-claim={:#x} ticks={} sie={:#x} sip={:#x} stie={} stip={} sext={} sstatus={:#x}",
                irq_last_raw(),
                irq_handler_runs(),
                ri,
                ti,
                abnormal,
                spurious,
                s.drop_malformed_ip,
                s.drop_no_route,
                s.drop_route_source_mismatch,
                s.rx_slot_full,
                s.tx_slot_full,
                s.rx_slot_occupancy,
                s.tx_slot_occupancy,
                s.tx_descriptor_inflight,
                s.reclaim_exhausted,
                s.rx_exhausted,
                s.submit_exhausted,
                s.flush_target,
                s.flush_success,
                s.flush_error,
                s.flush_busy,
                s.flush_cancel,
                st.task_poll,
                st.rounds,
                st.work,
                st.self_yield,
                st.timer_wake,
                st.fallback_poll,
                s.lifecycle,
                s.owner,
                phy60_events,
                phy60_set,
                phy60_rearm,
                irq_en,
                status,
                rx_done,
                aia_traps,
                aia_claim,
                ticks,
                csr.sie,
                csr.sip,
                (csr.sie >> 5) & 1,
                (csr.sip >> 5) & 1,
                (csr.sie >> 9) & 1,
                csr.sstatus,
            ),
        );
    }

    /// One UDP echo session (T6): bind `UDP_ECHO_PORT`, receive one
    /// datagram, echo every byte back to the sender. The session wait is
    /// operator-paced; every op is bounded and every outcome is printed.
    fn udp_echo_once(out: &mut dyn FnMut(&str)) {
        let session = async {
            // MS13 001-rework T5-R1: the payload's first timer-dependent
            // await — proves the executor polled this far (prints once, on
            // the first poll, before any hang point).
            super::board::discriminator_line("udp-enter", out);
            let sock = UdpSocket::new();
            sock.bind(SocketAddrEx::Ip(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                UDP_ECHO_PORT,
            )))?;
            let mut buf = [0u8; 128];
            // `RecvOptions::from` needs an EXISTING address slot.
            let mut from =
                SocketAddrEx::Ip(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0));
            let n = timeout(
                Some(Duration::from_nanos(SESSION_WAIT_NANOS)),
                poll_io(&sock, IoEvents::IN, false, || {
                    let opts = RecvOptions {
                        from: Some(&mut from),
                        flags: Default::default(),
                        cmsg: None,
                    };
                    sock.recv(&mut buf[..], opts)
                }),
            )
            .await
            .map_err(|_| AxError::TimedOut)??;
            let peer = match from {
                SocketAddrEx::Ip(addr) => addr,
                _ => return Err(AxError::InvalidInput),
            };
            let sent = timeout(
                Some(Duration::from_nanos(SESSION_OP_NANOS)),
                poll_io(&sock, IoEvents::OUT, false, || {
                    sock.send(
                        &buf[..n],
                        SendOptions {
                            to: Some(SocketAddrEx::Ip(peer)),
                            flags: Default::default(),
                            cmsg: alloc::vec::Vec::new(),
                        },
                    )
                }),
            )
            .await
            .map_err(|_| AxError::TimedOut)??;
            Ok((sent, peer))
        };
        match block_on(session) {
            Ok((sent, peer)) => line(
                out,
                format_args!("[ms12] probe proto udp ok bytes={sent} from={peer}"),
            ),
            Err(e) => line(
                out,
                format_args!("[ms12] probe proto udp err={e:?}"),
            ),
        }
    }

    /// One TCP echo session (T6): listen on `TCP_ECHO_PORT`, accept one
    /// connection, echo every byte of the peer's send, then actively close
    /// (normal FIN — the peer reads the data and then EOF).
    fn tcp_echo_once_close(out: &mut dyn FnMut(&str)) {
        let session = async {
            let listener = TcpSocket::new();
            listener.bind(SocketAddrEx::Ip(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                TCP_ECHO_PORT,
            )))?;
            listener.listen()?;
            let socket = timeout(
                Some(Duration::from_nanos(SESSION_WAIT_NANOS)),
                poll_io(&listener, IoEvents::IN, false, || listener.accept()),
            )
            .await
            .map_err(|_| AxError::TimedOut)??;
            let Socket::Tcp(conn) = socket else {
                return Err(AxError::InvalidInput);
            };
            let mut buf = [0u8; 64];
            let n = timeout(
                Some(Duration::from_nanos(SESSION_OP_NANOS)),
                poll_io(&conn, IoEvents::IN, false, || {
                    conn.recv(&mut buf[..], RecvOptions::default())
                }),
            )
            .await
            .map_err(|_| AxError::TimedOut)??;
            let sent = timeout(
                Some(Duration::from_nanos(SESSION_OP_NANOS)),
                poll_io(&conn, IoEvents::OUT, false, || {
                    conn.send(&buf[..n], SendOptions::default())
                }),
            )
            .await
            .map_err(|_| AxError::TimedOut)??;
            conn.shutdown(Shutdown::Both)?;
            Ok(sent)
        };
        match block_on(session) {
            Ok(sent) => line(
                out,
                format_args!("[ms12] probe proto tcp ok bytes={sent} closed=1"),
            ),
            Err(e) => line(
                out,
                format_args!("[ms12] probe proto tcp err={e:?}"),
            ),
        }
    }

    /// The protocol session phase (T6/T7), after the capacity/flush phases
    /// with every hold released: the operator warms the neighbor and pings
    /// (the stack answers ICMP itself), then sends one UDP and one TCP
    /// echo; the payload closes with the 2 s quiet observation and the
    /// bounded resume wait. Every line is a native observation.
    fn run_protocol_phase(out: &mut dyn FnMut(&str)) {
        out_line(
            out,
            format_args!(
                "[ms12] probe proto begin udp={} tcp={} op-deadline-ms={} session-wait-ms={} quiet-ms={} resume-wait-ms={}",
                UDP_ECHO_PORT,
                TCP_ECHO_PORT,
                SESSION_OP_NANOS / 1_000_000,
                SESSION_WAIT_NANOS / 1_000_000,
                QUIET_WINDOW_NANOS / 1_000_000,
                RESUME_WAIT_NANOS / 1_000_000,
            ),
        );
        snapshot_line(out, "proto-begin");
        udp_echo_once(out);
        tcp_echo_once_close(out);

        // Settle: the peer's close tail (FIN/ACK exchange) must land BEFORE
        // the freeze observation starts, or the quiet premise is structurally
        // broken (V3 round-1 finding). Wait until the claim stream has been
        // still for `SETTLE_QUIET_NANOS`, bounded by `SETTLE_CAP_NANOS`.
        let settle_cap = wall_time() + Duration::from_nanos(SETTLE_CAP_NANOS);
        let mut last_irq = irq_handler_runs();
        let mut last_change = wall_time();
        loop {
            let now = wall_time();
            let now_irq = irq_handler_runs();
            if now_irq != last_irq {
                last_irq = now_irq;
                last_change = now;
            }
            if now.saturating_sub(last_change) >= Duration::from_nanos(SETTLE_QUIET_NANOS)
                || now >= settle_cap
            {
                break;
            }
            axtask::yield_now();
        }
        out_line(
            out,
            format_args!(
                "[ms12] probe settle irq-runs={} still-ms={} cap-ms={}",
                last_irq,
                SETTLE_QUIET_NANOS / 1_000_000,
                SETTLE_CAP_NANOS / 1_000_000,
            ),
        );

        // Quiet: the operator sends nothing; with no new cause and no due
        // protocol timer the queue/stack counters must stay frozen. The
        // sleep is the runtime timer's own.
        snapshot_line(out, "quiet-before");
        axtask::sleep(Duration::from_nanos(QUIET_WINDOW_NANOS));
        snapshot_line(out, "quiet-after");

        // Resume: the operator sends one packet — a new source-133 MAC cause
        // (RI/TI growth) must wake the data plane and grow the counters
        // again. MS13 001 Minor refinement: a bare PHY source-60 claim or any
        // non-MAC ISR publish must NOT consume the resume window, so the
        // trigger is the MAC cause counters, not every handler run.
        let (ri0, ti0, _, _) = irq_counts();
        let baseline = axnet::rx_snapshot_v3();
        let deadline = wall_time() + Duration::from_nanos(RESUME_WAIT_NANOS);
        loop {
            let snap = axnet::rx_snapshot_v3();
            let (ri, ti, _, _) = irq_counts();
            if ri != ri0 || ti != ti0 || snap.reaped != baseline.reaped {
                snapshot_line(out, "resume");
                break;
            }
            if wall_time() >= deadline {
                out_line(
                    out,
                    format_args!(
                        "[ms12] probe resume timeout irq-runs={} reaped={}",
                        irq_handler_runs(),
                        snap.reaped,
                    ),
                );
                break;
            }
            axtask::yield_now();
        }
        snapshot_line(out, "proto-end");
    }

    /// The capacity/flush payload (see the module docs). The caller parks
    /// after this returns in every path.
    /// MS13 A4 diagnostics: the retained arena VA, set once at payload
    /// start so snapshot_line can observe the RX descriptors' OWN bits
    /// non-destructively (frames arriving vs interrupts dead).
    static PAYLOAD_ARENA_VA: core::sync::atomic::AtomicUsize =
        core::sync::atomic::AtomicUsize::new(0);

    pub(super) fn run_diagnostics(arena_va: usize, out: &mut dyn FnMut(&str)) {
        PAYLOAD_ARENA_VA.store(
            arena_va,
            core::sync::atomic::Ordering::Relaxed,
        );
        out_line(
            out,
            format_args!(
                "[ms12] probe begin slots={} descs={} hold-ms={} irq-source={}",
                SLOT_CAPACITY, DESC_CAPACITY, HOLD_LEASE_MS, GMAC_IRQ_SOURCE,
            ),
        );

        // ── Phases 1–3: the two-level capacity boundary, the explicit
        //    release and the flush Pending→release→C4 ──
        if !run_capacity_flush_cycle(out) {
            return;
        }

        // ── Phase 4 (T6/T7): the protocol session, product-side snapshots
        //    and the quiet/resume observation — every hold released above.
        run_protocol_phase(out);

        // ── MS13 T7: the final on-board recovery matrix (old/new sockets,
        //    physical link flap, reset/link/flush interleave, two recoveries
        //    with capacity/C4, and the fixed-last honest fail-stop). ──
        run_recovery_matrix(out);
    }

    /// One capacity/flush cycle (the original phases 1–3): the two-level
    /// capacity boundary under a reclaim hold, the explicit release drain and
    /// the flush Pending→release→C4 close. Returns `false` when a stop
    /// condition printed; the caller parks with the backing retained.
    fn run_capacity_flush_cycle(out: &mut dyn FnMut(&str)) -> bool {
        if let Err(e) = axnet::diagnostic_control(OP_HOLD_TX_RECLAIM, HOLD_LEASE_MS) {
            out_line(out, format_args!("[ms12] probe err=hold {e:?} stop"));
            return false;
        }
        // One console line costs ~10 ms at 115200 — printing per frame would
        // outlast the 200 ms lease before the loop ends. Print every 25th
        // frame and the refused frame; the loop itself stays quiet so the
        // whole run fits inside the hold window.
        //
        // Under the hold the first DESC_CAPACITY frames drain into the
        // hardware ring, so the software 64-slot boundary sits at
        // SLOT_CAPACITY + DESC_CAPACITY submitted frames: enqueue until a
        // frame is refused (the slot Full) or the structural cap (slots +
        // descriptors + 1) is exhausted.
        let cap = SLOT_CAPACITY + DESC_CAPACITY + 1;
        let mut accepted = 0usize;
        for i in 0..cap {
            match axnet::diagnostic_enqueue_frame([0xff; 6], 0x0800, &[b'p', i as u8]) {
                Ok(rep) => {
                    if rep.accepted {
                        accepted += 1;
                    }
                    if i % 25 == 0 || !rep.accepted {
                        out_line(
                            out,
                            format_args!(
                                "[ms12] probe enqueue i={i} accepted={} tx-slot={} high-water={} tx-full={} desc-inflight={} buf-inflight={} completions={}",
                                rep.accepted,
                                rep.tx_slot_occupancy,
                                rep.tx_slot_high_water,
                                rep.tx_slot_full,
                                rep.tx_descriptor_inflight.unwrap_or(0),
                                rep.tx_buffer_inflight.unwrap_or(0),
                                rep.completions_seen.unwrap_or(0),
                            ),
                        );
                    }
                    // Boundary reached: the slot Full is the observation.
                    if !rep.accepted {
                        break;
                    }
                }
                Err(e) => {
                    out_line(out, format_args!("[ms12] probe err=enqueue {e:?} stop"));
                    return false;
                }
            }
            axtask::yield_now();
        }
        // The submit stage drains the slots into the descriptors until the
        // hardware capacity holds: 16 in flight, the backlog stays slotted.
        let snap = wait_v3(out, "capacity", |s| {
            s.tx_descriptor_inflight == DESC_CAPACITY as u64
        });
        out_line(
            out,
            format_args!(
                "[ms12] probe capacity accepted={accepted} tx-slot-full={} tx-again={} tx-submit={} desc-inflight={} slot-backlog={} lifecycle={}",
                snap.tx_slot_full,
                snap.tx_again,
                snap.tx_submit,
                snap.tx_descriptor_inflight,
                snap.tx_slot_occupancy,
                snap.lifecycle,
            ),
        );

        // ── Phase 2: the explicit release drains everything ──
        if let Err(e) = axnet::diagnostic_control(OP_RELEASE, 0) {
            out_line(out, format_args!("[ms12] probe err=release {e:?} stop"));
            return false;
        }
        out_line(out, format_args!("[ms12] probe release ok"));
        let snap = wait_v3(out, "restore", |s| {
            s.tx_slot_occupancy == 0
                && s.tx_descriptor_inflight == 0
                && s.tx_buffer_inflight == 0
        });
        out_line(
            out,
            format_args!(
                "[ms12] probe restored tx-slot={} desc-inflight={} buf-available={} desc-available={} completions={} lifecycle={}",
                snap.tx_slot_occupancy,
                snap.tx_descriptor_inflight,
                snap.tx_buffer_available,
                snap.tx_descriptor_available,
                snap.tx_completion,
                snap.lifecycle,
            ),
        );

        // ── Phase 3: flush Pending → explicit release → C4 ──
        if let Err(e) = axnet::diagnostic_control(OP_HOLD_TX_RECLAIM, HOLD_LEASE_MS) {
            out_line(out, format_args!("[ms12] probe err=hold-2 {e:?} stop"));
            return false;
        }
        match axnet::diagnostic_enqueue_frame([0xff; 6], 0x0800, b"flush") {
            Ok(rep) if rep.accepted => {}
            Ok(_) => {
                out_line(out, format_args!("[ms12] probe err=flush-enqueue-full stop"));
                return false;
            }
            Err(e) => {
                out_line(out, format_args!("[ms12] probe err=flush-enqueue {e:?} stop"));
                return false;
            }
        }
        axtask::yield_now();
        let mut fut = match axnet::flush() {
            Ok(f) => core::pin::pin!(f),
            Err(e) => {
                out_line(out, format_args!("[ms12] probe err=flush {e:?} stop"));
                return false;
            }
        };
        if !matches!(flush_poll(&mut fut), Poll::Pending) {
            out_line(out, format_args!("[ms12] probe err=flush-not-held stop"));
            return false;
        }
        out_line(out, format_args!("[ms12] probe flush pending"));
        // THE SAME waiter observes the release: the explicit release lets the
        // owner reclaim the held ticket and C4 completes this future. (Never
        // drop and re-reserve: a dropped `Pin<&mut>` does not drop the pinned
        // future, so the sole waiter slot would stay taken.)
        if let Err(e) = axnet::diagnostic_control(OP_RELEASE, 0) {
            out_line(out, format_args!("[ms12] probe err=release-2 {e:?} stop"));
            return false;
        }
        out_line(out, format_args!("[ms12] probe release-2 ok"));
        let deadline = wall_time() + Duration::from_nanos(PHASE_CAP_NANOS);
        let outcome = loop {
            match flush_poll(&mut fut) {
                Poll::Ready(result) => break result,
                Poll::Pending => {
                    if wall_time() >= deadline {
                        out_line(out, format_args!("[ms12] probe err=flush-cap stop"));
                        return false;
                    }
                    axtask::yield_now();
                }
            }
        };
        match outcome {
            Ok(()) => out_line(out, format_args!("[ms12] probe flush c4 ok")),
            Err(e) => {
                out_line(out, format_args!("[ms12] probe err=flush-c4 {e:?} stop"));
                return false;
            }
        }

        // ── Final: the auto-release counter must still be zero ──
        let snap = axnet::rx_snapshot_v3();
        out_line(
            out,
            format_args!(
                "[ms12] probe end auto-release={} hold-mode={} lease-expiry={} flush-success={} flush-busy={} flush-cancel={} lifecycle={} owner={} backing=retained",
                snap.auto_release_failure,
                snap.hold_mode,
                snap.lease_expiry,
                snap.flush_success,
                snap.flush_busy,
                snap.flush_cancel,
                snap.lifecycle,
                snap.owner,
            ),
        );
        true
    }

    // ── MS13 T7: the final on-board recovery matrix ────────────────────────

    /// Encoded waiter outcome for the matrix observers (lock-free; the exact
    /// terminal category is the operator's judgment input).
    const WAITER_PENDING: u8 = 0;
    const WAITER_OK: u8 = 1;
    const WAITER_TIMED_OUT: u8 = 2;
    const WAITER_NOT_CONNECTED: u8 = 3;
    const WAITER_CONNECTION_RESET: u8 = 4;
    const WAITER_OTHER: u8 = 5;

    fn waiter_code_name(code: u8) -> &'static str {
        match code {
            WAITER_PENDING => "pending",
            WAITER_OK => "ok",
            WAITER_TIMED_OUT => "timed-out",
            WAITER_NOT_CONNECTED => "not-connected",
            WAITER_CONNECTION_RESET => "connection-reset",
            _ => "other",
        }
    }

    fn encode_waiter(res: &axerrno::AxResult<usize>) -> u8 {
        match res {
            Ok(_) => WAITER_OK,
            Err(AxError::NotConnected) => WAITER_NOT_CONNECTED,
            Err(AxError::ConnectionReset) => WAITER_CONNECTION_RESET,
            Err(AxError::TimedOut) => WAITER_TIMED_OUT,
            Err(_) => WAITER_OTHER,
        }
    }

    /// One parked old-epoch UDP waiter (bound `port`, no data coming). The
    /// task stores the terminal category when the epoch terminal reaches it.
    fn spawn_matrix_udp_waiter(port: u16) -> alloc::sync::Arc<core::sync::atomic::AtomicU8> {
        use core::sync::atomic::{AtomicU8, Ordering};
        let code = alloc::sync::Arc::new(AtomicU8::new(WAITER_PENDING));
        let code2 = code.clone();
        axtask::spawn(move || {
            let session = async {
                let sock = UdpSocket::new();
                sock.bind(SocketAddrEx::Ip(SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                    port,
                )))?;
                let mut buf = [0u8; 64];
                poll_io(&sock, IoEvents::IN, false, || {
                    sock.recv(&mut buf[..], RecvOptions::default())
                })
                .await
            };
            let res = block_on(session);
            code2.store(encode_waiter(&res), Ordering::Release);
        });
        code
    }

    /// One parked old-epoch TCP listener waiter (bound `port`).
    fn spawn_matrix_listener_waiter(port: u16) -> alloc::sync::Arc<core::sync::atomic::AtomicU8> {
        use core::sync::atomic::{AtomicU8, Ordering};
        let code = alloc::sync::Arc::new(AtomicU8::new(WAITER_PENDING));
        let code2 = code.clone();
        axtask::spawn(move || {
            let session = async {
                let listener = TcpSocket::new();
                listener.bind(SocketAddrEx::Ip(SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                    port,
                )))?;
                listener.listen()?;
                let socket = poll_io(&listener, IoEvents::IN, false, || listener.accept()).await?;
                let _ = socket;
                Ok(0usize)
            };
            let res = block_on(session);
            code2.store(encode_waiter(&res), Ordering::Release);
        });
        code
    }

    /// One operator-paced UDP echo on the matrix port (the tag names the
    /// phase; every outcome is printed, the operator adjudicates).
    fn matrix_udp_echo_once(out: &mut dyn FnMut(&str), tag: &str) {
        let session = async {
            let sock = UdpSocket::new();
            sock.bind(SocketAddrEx::Ip(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                MATRIX_UDP_PORT,
            )))?;
            let mut buf = [0u8; 128];
            let mut from =
                SocketAddrEx::Ip(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 0));
            let n = timeout(
                Some(Duration::from_nanos(SESSION_WAIT_NANOS)),
                poll_io(&sock, IoEvents::IN, false, || {
                    let opts = RecvOptions {
                        from: Some(&mut from),
                        ..Default::default()
                    };
                    sock.recv(&mut buf[..], opts)
                }),
            )
            .await
            .map_err(|_| AxError::TimedOut)??;
            let peer = match from {
                SocketAddrEx::Ip(addr) => addr,
                _ => return Err(AxError::InvalidInput),
            };
            let sent = timeout(
                Some(Duration::from_nanos(SESSION_OP_NANOS)),
                poll_io(&sock, IoEvents::OUT, false, || {
                    sock.send(
                        &buf[..n],
                        SendOptions {
                            to: Some(SocketAddrEx::Ip(peer)),
                            ..Default::default()
                        },
                    )
                }),
            )
            .await
            .map_err(|_| AxError::TimedOut)??;
            Ok((sent, peer))
        };
        match block_on(session) {
            Ok((sent, peer)) => out_line(
                out,
                format_args!("[ms13] matrix {tag} udp ok bytes={sent} from={peer}"),
            ),
            Err(e) => out_line(out, format_args!("[ms13] matrix {tag} udp err={e:?}")),
        }
    }

    /// One operator-paced TCP echo with an active close on the matrix port.
    fn matrix_tcp_echo_once(out: &mut dyn FnMut(&str), tag: &str) {
        let session = async {
            let listener = TcpSocket::new();
            listener.bind(SocketAddrEx::Ip(SocketAddr::new(
                IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                MATRIX_TCP_PORT,
            )))?;
            listener.listen()?;
            let socket = timeout(
                Some(Duration::from_nanos(SESSION_WAIT_NANOS)),
                poll_io(&listener, IoEvents::IN, false, || listener.accept()),
            )
            .await
            .map_err(|_| AxError::TimedOut)??;
            let Socket::Tcp(conn) = socket else {
                return Err(AxError::InvalidInput);
            };
            let mut buf = [0u8; 64];
            let n = timeout(
                Some(Duration::from_nanos(SESSION_OP_NANOS)),
                poll_io(&conn, IoEvents::IN, false, || {
                    conn.recv(&mut buf[..], RecvOptions::default())
                }),
            )
            .await
            .map_err(|_| AxError::TimedOut)??;
            let sent = timeout(
                Some(Duration::from_nanos(SESSION_OP_NANOS)),
                poll_io(&conn, IoEvents::OUT, false, || {
                    conn.send(&buf[..n], SendOptions::default())
                }),
            )
            .await
            .map_err(|_| AxError::TimedOut)??;
            conn.shutdown(Shutdown::Both)?;
            Ok(sent)
        };
        match block_on(session) {
            Ok(sent) => out_line(
                out,
                format_args!("[ms13] matrix {tag} tcp ok bytes={sent} closed=1"),
            ),
            Err(e) => out_line(out, format_args!("[ms13] matrix {tag} tcp err={e:?}")),
        }
    }

    /// Bounded wait for one owner lifecycle value; the final snapshot is
    /// returned either way and the timeout prints its own line. The value
    /// must be observed to leave `want` before a matching sample is
    /// accepted: single-hart cooperative scheduling lets the first sample
    /// read back the stale pre-request Active(2) while the owner has not
    /// claimed the request yet. Fail-stop's want=3 leaves at once (the live
    /// value starts at 2).
    fn wait_lifecycle(out: &mut dyn FnMut(&str), stage: &str, want: u64) -> axnet::RxSnapshotV3 {
        let deadline = wall_time() + Duration::from_nanos(PHASE_CAP_NANOS);
        let mut left = false;
        loop {
            let snap = axnet::rx_snapshot_v3();
            if snap.lifecycle != want {
                left = true;
            } else if left {
                return snap;
            }
            if wall_time() >= deadline {
                out_line(
                    out,
                    format_args!(
                        "[ms13] matrix {stage} timeout lifecycle={} want={} owner={} fault={} last-err-stage={} last-err-code={}",
                        snap.lifecycle,
                        want,
                        snap.owner,
                        axnet::rx_snapshot().fault,
                        snap.last_error_stage,
                        snap.last_error_code,
                    ),
                );
                return snap;
            }
            axtask::yield_now();
        }
    }

    /// Bounded wait for one parked waiter's terminal category.
    fn wait_waiter(
        out: &mut dyn FnMut(&str),
        stage: &str,
        code: &alloc::sync::Arc<core::sync::atomic::AtomicU8>,
    ) -> u8 {
        use core::sync::atomic::Ordering;
        let deadline = wall_time() + Duration::from_nanos(PHASE_CAP_NANOS);
        loop {
            let seen = code.load(Ordering::Acquire);
            if seen != WAITER_PENDING {
                out_line(
                    out,
                    format_args!("[ms13] matrix {stage} waiter={}", waiter_code_name(seen)),
                );
                return seen;
            }
            if wall_time() >= deadline {
                out_line(out, format_args!("[ms13] matrix {stage} waiter=pending-timeout"));
                return WAITER_PENDING;
            }
            axtask::yield_now();
        }
    }

    /// The final on-board recovery matrix (MS13 T7). Every line is a native
    /// observation the operator adjudicates; every wait is the unified 25 s
    /// window. The fail-stop injection is the FIXED last scenario: after it
    /// the device stays faulted and the payload returns (the caller parks
    /// with the backing retained until the external reset).
    fn run_recovery_matrix(out: &mut dyn FnMut(&str)) {
        out_line(
            out,
            format_args!(
                "[ms13] matrix begin udp={} tcp={} waiter={} wait-ms={} op-ms={} cap-ms={}",
                MATRIX_UDP_PORT,
                MATRIX_TCP_PORT,
                MATRIX_WAITER_PORT,
                SESSION_WAIT_NANOS / 1_000_000,
                SESSION_OP_NANOS / 1_000_000,
                PHASE_CAP_NANOS / 1_000_000,
            ),
        );
        snapshot_line(out, "matrix-begin");

        // ── R1: the old epoch's live plane (operator-paced) ──
        out_line(
            out,
            format_args!(
                "[ms13] matrix old-session send one UDP datagram to udp={} then one TCP session to tcp={} (each echoed back)",
                MATRIX_UDP_PORT, MATRIX_TCP_PORT,
            ),
        );
        matrix_udp_echo_once(out, "old");
        matrix_tcp_echo_once(out, "old");

        // Old waiters parked: a UDP recv and a listener accept with no data
        // coming; the recovery's epoch terminal must reach both.
        let udp_waiter = spawn_matrix_udp_waiter(MATRIX_WAITER_PORT);
        let listener_waiter = spawn_matrix_listener_waiter(MATRIX_TCP_PORT);
        // A pending flush anchored pre-reset: the 2 s reclaim hold outlasts
        // the 1 s quiesce window, so the anchored ticket is still
        // device-held when the recovery closes it — the flush observes the
        // recovery's lost/abort outcome, never a forged C4 success.
        const MATRIX_HOLD_LEASE_MS: u64 = 2000;
        if let Err(e) = axnet::diagnostic_control(OP_HOLD_TX_RECLAIM, MATRIX_HOLD_LEASE_MS) {
            out_line(out, format_args!("[ms13] matrix err=hold {e:?}"));
            return;
        }
        match axnet::diagnostic_enqueue_frame([0xff; 6], 0x0800, b"pre-reset") {
            Ok(rep) if rep.accepted => {}
            Ok(_) | Err(_) => {
                out_line(out, format_args!("[ms13] matrix err=flush-enqueue"));
                return;
            }
        }
        let mut matrix_flush = match axnet::flush() {
            Ok(f) => core::pin::pin!(f),
            Err(e) => {
                out_line(out, format_args!("[ms13] matrix err=flush {e:?}"));
                return;
            }
        };
        if !matches!(flush_poll(&mut matrix_flush), Poll::Pending) {
            out_line(out, format_args!("[ms13] matrix err=flush-not-held"));
            return;
        }
        out_line(out, format_args!("[ms13] matrix flush pending"));

        // ── R2: controlled reset #1 ──
        out_line(out, format_args!("[ms13] matrix reset-1 request now"));
        if let Err(e) = axnet::recovery_reset_request() {
            out_line(out, format_args!("[ms13] matrix err=reset-1 {e:?}"));
            return;
        }
        let snap = wait_lifecycle(out, "reset-1", 2);
        out_line(
            out,
            format_args!(
                "[ms13] matrix reset-1 committed lifecycle={} owner={} queue-gen={} fault={}",
                snap.lifecycle,
                snap.owner,
                snap.queue_generation,
                axnet::rx_snapshot().fault,
            ),
        );
        // The old waiters observe the reset terminal, never silence.
        wait_waiter(out, "reset-1-udp-waiter", &udp_waiter);
        wait_waiter(out, "reset-1-listener-waiter", &listener_waiter);
        // The flush observes the aborted outcome (an error, never a C4).
        match flush_poll(&mut matrix_flush) {
            Poll::Ready(Err(e)) => out_line(
                out,
                format_args!("[ms13] matrix reset-1 flush aborted err={e:?}"),
            ),
            Poll::Ready(Ok(())) => {
                out_line(out, format_args!("[ms13] matrix reset-1 flush UNEXPECTED-C4-OK"))
            }
            Poll::Pending => {
                out_line(out, format_args!("[ms13] matrix reset-1 flush still-pending"))
            }
        }
        // Release the matrix hold explicitly (the lease never had to expire;
        // the auto-release counter must stay 0).
        if let Err(e) = axnet::diagnostic_control(OP_RELEASE, 0) {
            out_line(out, format_args!("[ms13] matrix err=release {e:?}"));
            return;
        }
        // Post-recovery capacity/C4 round one.
        if !run_capacity_flush_cycle(out) {
            return;
        }
        // New objects on the same ports (operator-paced).
        out_line(
            out,
            format_args!(
                "[ms13] matrix new-session-1 send one UDP datagram to udp={} then one TCP session to tcp={}",
                MATRIX_UDP_PORT, MATRIX_TCP_PORT,
            ),
        );
        matrix_udp_echo_once(out, "new-1");
        matrix_tcp_echo_once(out, "new-1");

        // ── R3: the physical link flap (the 001 residual) ──
        // A fresh pre-flap object: its blocked recv must complete with the
        // link-down terminal.
        let flap_waiter = spawn_matrix_udp_waiter(MATRIX_WAITER_PORT);
        out_line(out, format_args!("[ms13] matrix link-down: operator unplug the cable now"));
        wait_waiter(out, "link-down", &flap_waiter);
        snapshot_line(out, "link-down-observed");
        // Reset during link-down (the reset/link interleave, recovery two).
        out_line(out, format_args!("[ms13] matrix reset-2 (link down) request now"));
        if let Err(e) = axnet::recovery_reset_request() {
            out_line(out, format_args!("[ms13] matrix err=reset-2 {e:?}"));
            return;
        }
        let snap = wait_lifecycle(out, "reset-2", 2);
        out_line(
            out,
            format_args!(
                "[ms13] matrix reset-2 committed lifecycle={} owner={} queue-gen={} fault={}",
                snap.lifecycle,
                snap.owner,
                snap.queue_generation,
                axnet::rx_snapshot().fault,
            ),
        );
        // The rising edge: the operator plugs back; the source-60 event (and
        // the owner's reopened epoch) must arrive within the window.
        let (phy60_before, _, _, _) = super::board::phy60_counts();
        out_line(out, format_args!("[ms13] matrix link-up: operator plug the cable back now"));
        let deadline = wall_time() + Duration::from_nanos(PHASE_CAP_NANOS);
        loop {
            let (phy60_now, _, _, _) = super::board::phy60_counts();
            if phy60_now > phy60_before {
                out_line(
                    out,
                    format_args!("[ms13] matrix link-up phy60-event observed events={phy60_now}"),
                );
                break;
            }
            if wall_time() >= deadline {
                out_line(out, format_args!("[ms13] matrix link-up phy60-event timeout"));
                break;
            }
            axtask::yield_now();
        }
        // Plug-in new-epoch sessions (the 001 residual) + ICMP: the operator
        // pings the board, then sends UDP and TCP once more.
        out_line(
            out,
            format_args!(
                "[ms13] matrix new-session-2 ping the board, then send one UDP datagram to udp={} and one TCP session to tcp={}",
                MATRIX_UDP_PORT, MATRIX_TCP_PORT,
            ),
        );
        matrix_udp_echo_once(out, "new-2");
        matrix_tcp_echo_once(out, "new-2");
        snapshot_line(out, "matrix-pre-capacity-2");
        // Post-recovery capacity/C4 round two (two successful recoveries).
        if !run_capacity_flush_cycle(out) {
            return;
        }

        // ── R4: the honest fail-stop (FIXED LAST; no recovery after it) ──
        out_line(
            out,
            format_args!(
                "[ms13] matrix fail-stop: arming the confirm-withhold injection (last scenario)"
            ),
        );
        super::gmac::diagnostic_arm_reset_confirm_withhold(true);
        if let Err(e) = axnet::recovery_reset_request() {
            out_line(out, format_args!("[ms13] matrix err=fail-stop-request {e:?}"));
            return;
        }
        let snap = wait_lifecycle(out, "fail-stop", 3);
        snapshot_line(out, "fail-stop-observed");
        // The faulted plane refuses new submits and retains the backing.
        let rep = axnet::diagnostic_enqueue_frame([0xff; 6], 0x0800, b"after-fail-stop");
        out_line(
            out,
            format_args!(
                "[ms13] matrix fail-stop enqueue-after={:?} lifecycle={} want=3 owner={} backing=retained external-reset-next",
                rep.map(|r| r.accepted),
                snap.lifecycle,
                snap.owner,
            ),
        );
    }

    // ── MS14 Iteration 001 (T6): the k3-smp-net multi-hart payload ────────
    //
    // Runs inside the ONE application task (singleton affinity on the fixed
    // app hart, committed before its first enqueue): the reused MS12
    // capacity/flush cycle, the four-protocol session with the approved
    // 2 s quiet freeze, and the multi-waiter readiness witnesses — every
    // application task (payload + waiters) lands on that same hart. The
    // boot task supervises through `SMP_PAYLOAD_DONE`; every printed line
    // is a native observation and the payload never judges PASS.

    /// Set by `run_smp_net_payload` on every terminal path (the boot task
    /// waits on it with yields; no retry, no second payload).
    pub(super) static SMP_PAYLOAD_DONE: core::sync::atomic::AtomicBool =
        core::sync::atomic::AtomicBool::new(false);

    /// The approved quiet freeze window (A8): settle first, then 2 s of
    /// frozen counters, then bounded resume on the operator's new packet.
    const SMP_QUIET_NANOS: u64 = 2_000_000_000;

    /// One parked waiter task bound `port`, singleton-pinned to the app
    /// hart (the MS13 matrix waiter shape with an affinity-committed
    /// spawn). `None` = the affinity-committed spawn was rejected.
    fn spawn_smp_udp_waiter(
        port: u16,
        hart: usize,
    ) -> Option<alloc::sync::Arc<core::sync::atomic::AtomicU8>> {
        use core::sync::atomic::{AtomicU8, Ordering};
        let code = alloc::sync::Arc::new(AtomicU8::new(WAITER_PENDING));
        let code2 = code.clone();
        let mut mask = axtask::AxCpuMask::new();
        mask.set(hart, true).ok()?;
        axtask::spawn_with_name_affinity(
            move || {
                let session = async {
                    let sock = UdpSocket::new();
                    sock.bind(SocketAddrEx::Ip(SocketAddr::new(
                        IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                        port,
                    )))?;
                    let mut buf = [0u8; 64];
                    poll_io(&sock, IoEvents::IN, false, || {
                        sock.recv(&mut buf[..], RecvOptions::default())
                    })
                    .await
                };
                let res = block_on(session);
                code2.store(encode_waiter(&res), Ordering::Release);
            },
            alloc::format!("ms14-net-waiter-udp"),
            mask,
        )?;
        Some(code)
    }

    /// One parked listener waiter task bound `port`, singleton-pinned to
    /// the app hart. `None` = the affinity-committed spawn was rejected.
    fn spawn_smp_listener_waiter(
        port: u16,
        hart: usize,
    ) -> Option<alloc::sync::Arc<core::sync::atomic::AtomicU8>> {
        use core::sync::atomic::{AtomicU8, Ordering};
        let code = alloc::sync::Arc::new(AtomicU8::new(WAITER_PENDING));
        let code2 = code.clone();
        let mut mask = axtask::AxCpuMask::new();
        mask.set(hart, true).ok()?;
        axtask::spawn_with_name_affinity(
            move || {
                let session = async {
                    let listener = TcpSocket::new();
                    listener.bind(SocketAddrEx::Ip(SocketAddr::new(
                        IpAddr::V4(Ipv4Addr::UNSPECIFIED),
                        port,
                    )))?;
                    listener.listen()?;
                    let socket =
                        poll_io(&listener, IoEvents::IN, false, || listener.accept()).await?;
                    let _ = socket;
                    Ok(0usize)
                };
                let res = block_on(session);
                code2.store(encode_waiter(&res), Ordering::Release);
            },
            alloc::format!("ms14-net-waiter-listen"),
            mask,
        )?;
        Some(code)
    }

    /// Bounded wait for one SMP waiter's terminal category.
    fn wait_smp_waiter(
        out: &mut dyn FnMut(&str),
        stage: &str,
        code: &alloc::sync::Arc<core::sync::atomic::AtomicU8>,
    ) -> u8 {
        use core::sync::atomic::Ordering;
        let deadline = wall_time() + Duration::from_nanos(PHASE_CAP_NANOS);
        loop {
            let seen = code.load(Ordering::Acquire);
            if seen != WAITER_PENDING {
                out_line(
                    out,
                    format_args!("[ms14] net waiter {stage} terminal={}", waiter_code_name(seen)),
                );
                return seen;
            }
            if wall_time() >= deadline {
                out_line(out, format_args!("[ms14] net waiter {stage} terminal=pending-timeout"));
                return WAITER_PENDING;
            }
            axtask::yield_now();
        }
    }

    /// The four-protocol phase: the operator pings (the stack answers ICMP
    /// itself) and drives one UDP and one TCP echo, then the settle phase
    /// absorbs the close tail, the approved 2 s quiet window must freeze
    /// every counter, and one new packet must wake the plane again.
    fn smp_protocol_phase(out: &mut dyn FnMut(&str)) {
        out_line(
            out,
            format_args!(
                "[ms14] net proto begin udp={} tcp={} op-ms={} wait-ms={} settle-ms={} quiet-ms={}",
                UDP_ECHO_PORT,
                TCP_ECHO_PORT,
                SESSION_OP_NANOS / 1_000_000,
                SESSION_WAIT_NANOS / 1_000_000,
                SETTLE_QUIET_NANOS / 1_000_000,
                SMP_QUIET_NANOS / 1_000_000,
            ),
        );
        snapshot_line(out, "proto-begin");
        out_line(
            out,
            format_args!(
                "[ms14] net proto operator ping the board, then send one UDP datagram to udp={UDP_ECHO_PORT} and run one TCP echo session to tcp={TCP_ECHO_PORT}"
            ),
        );
        udp_echo_once(out);
        tcp_echo_once_close(out);

        // Settle: the peer's close tail (FIN/ACK exchange) must land BEFORE
        // the freeze observation starts (the MS12 V3 adjudication); wait
        // until the claim stream has been still for SETTLE_QUIET_NANOS.
        let settle_cap = wall_time() + Duration::from_nanos(SETTLE_CAP_NANOS);
        let mut last_irq = irq_handler_runs();
        let mut last_change = wall_time();
        loop {
            let now = wall_time();
            let now_irq = irq_handler_runs();
            if now_irq != last_irq {
                last_irq = now_irq;
                last_change = now;
            }
            if now.saturating_sub(last_change) >= Duration::from_nanos(SETTLE_QUIET_NANOS)
                || now >= settle_cap
            {
                break;
            }
            axtask::yield_now();
        }
        out_line(
            out,
            format_args!(
                "[ms14] net settle irq-runs={} still-ms={} cap-ms={}",
                last_irq,
                SETTLE_QUIET_NANOS / 1_000_000,
                SETTLE_CAP_NANOS / 1_000_000,
            ),
        );

        // Quiet: the approved 2 s freeze — no new cause and no due protocol
        // timer, so every queue/stack counter must stay frozen. The sleep
        // is the app hart's own runtime timer.
        snapshot_line(out, "quiet-before");
        axtask::sleep(Duration::from_nanos(SMP_QUIET_NANOS));
        snapshot_line(out, "quiet-after");

        // Resume: the operator sends one packet — new MAC cause counters
        // (RI/TI) or a new reap must wake the data plane again.
        let (ri0, ti0, _, _) = irq_counts();
        let baseline = axnet::rx_snapshot_v3();
        let deadline = wall_time() + Duration::from_nanos(PHASE_CAP_NANOS);
        loop {
            let snap = axnet::rx_snapshot_v3();
            let (ri, ti, _, _) = irq_counts();
            if ri != ri0 || ti != ti0 || snap.reaped != baseline.reaped {
                snapshot_line(out, "resume");
                break;
            }
            if wall_time() >= deadline {
                out_line(
                    out,
                    format_args!(
                        "[ms14] net resume timeout irq-runs={} reaped={}",
                        irq_handler_runs(),
                        snap.reaped,
                    ),
                );
                break;
            }
            axtask::yield_now();
        }
        snapshot_line(out, "proto-end");
    }

    /// The multi-waiter readiness phase (A7): two parked waiters on the
    /// matrix ports, both singleton-pinned to the app hart; the operator's
    /// datagram and TCP connection must complete them through real I/O.
    fn smp_waiter_phase(out: &mut dyn FnMut(&str), app_hart: usize) -> bool {
        let Some(udp_code) = spawn_smp_udp_waiter(MATRIX_WAITER_PORT, app_hart) else {
            out_line(out, format_args!("[ms14] net fault stage=waiter-spawn kind=udp stop"));
            return false;
        };
        let Some(listener_code) = spawn_smp_listener_waiter(MATRIX_TCP_PORT, app_hart) else {
            out_line(
                out,
                format_args!("[ms14] net fault stage=waiter-spawn kind=listener stop"),
            );
            return false;
        };
        out_line(
            out,
            format_args!(
                "[ms14] net waiter begin app={app_hart} udp={} tcp-listen={} wait-ms={} operator: send one UDP datagram to udp={MATRIX_WAITER_PORT}, then open and close one TCP connection to tcp={MATRIX_TCP_PORT}",
                MATRIX_WAITER_PORT,
                MATRIX_TCP_PORT,
                PHASE_CAP_NANOS / 1_000_000,
            ),
        );
        let udp = wait_smp_waiter(out, "udp", &udp_code);
        let listener = wait_smp_waiter(out, "listener", &listener_code);
        out_line(
            out,
            format_args!(
                "[ms14] net waiter done udp={} listener={}",
                waiter_code_name(udp),
                waiter_code_name(listener),
            ),
        );
        true
    }

    /// The k3-smp-net payload entry (T6), running inside the singleton app
    /// task. Every terminal path sets `SMP_PAYLOAD_DONE`; the boot task
    /// prints the summary and parks. The payload never judges PASS.
    pub(super) fn run_smp_net_payload(
        arena_va: usize,
        app_hart: usize,
        out: &mut dyn FnMut(&str),
    ) {
        use core::sync::atomic::Ordering;
        PAYLOAD_ARENA_VA.store(arena_va, Ordering::Relaxed);
        let hart = axhal::percpu::this_cpu_id();
        out_line(
            out,
            format_args!(
                "[ms14] net app begin hart_actual={hart} expect={app_hart} slots={} descs={} hold-ms={} irq-source={}",
                SLOT_CAPACITY, DESC_CAPACITY, HOLD_LEASE_MS, GMAC_IRQ_SOURCE,
            ),
        );
        if hart != app_hart {
            out_line(
                out,
                format_args!(
                    "[ms14] net fault stage=app-hart actual={hart} expect={app_hart} stop"
                ),
            );
            SMP_PAYLOAD_DONE.store(true, Ordering::Release);
            return;
        }
        // ① Two-level capacity Full→recovery, single reclaim, wrap and the
        //    ticketed C4 flush (the reused MS12 cycle; holds released).
        if !run_capacity_flush_cycle(out) {
            SMP_PAYLOAD_DONE.store(true, Ordering::Release);
            return;
        }
        // ② Four protocols + settle + the approved 2 s quiet + resume.
        smp_protocol_phase(out);
        // ③ Multi-waiter readiness on the matrix ports.
        if !smp_waiter_phase(out, app_hart) {
            SMP_PAYLOAD_DONE.store(true, Ordering::Release);
            return;
        }
        // ④ Final ledger snapshot.
        snapshot_line(out, "smp-end");
        out_line(
            out,
            format_args!(
                "[ms14] net app done hart_actual={} done=1",
                axhal::percpu::this_cpu_id(),
            ),
        );
        SMP_PAYLOAD_DONE.store(true, Ordering::Release);
    }
}
