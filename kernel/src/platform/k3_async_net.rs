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
    /// independent 1 s terminal deadline per frame, a 30 s session cap and
    /// a 2048-byte RX scratch.
    pub const RAW_TX_FRAMES: usize = 20;
    pub const RAW_FRAME_DEADLINE_NANOS: u64 = 1_000_000_000;
    pub const RAW_INTERFRAME_NANOS: u64 = 100_000_000;
    pub const RAW_SESSION_NANOS: u64 = 30_000_000_000;
    pub const RAW_RX_SCRATCH: usize = 2048;
    /// Per service round: at most 16 poll steps per direction.
    const ROUND_BUDGET: usize = 16;
    /// Maximum per-frame sample lines.
    const SAMPLE_CAP: usize = 4;

    fn irq_reg_read(offset: u32) -> u32 {
        // SAFETY: the GMAC window is identity-mapped via devices.mmio-ranges;
        // IRQ context reads the STATUS/IRQ_EN words only.
        unsafe {
            core::ptr::read_volatile((MAC_BASE_PADDR + offset as u64) as usize as *const u32)
        }
    }

    fn irq_reg_write(offset: u32, value: u32) {
        // SAFETY: same window contract; the ISR writes only IRQ_EN/STATUS.
        unsafe {
            core::ptr::write_volatile(
                (MAC_BASE_PADDR + offset as u64) as usize as *mut u32,
                value,
            )
        }
    }

    /// The production work-hint publish: the axnet ISR-safe queue event
    /// (generation + waker, no Service/descriptor touch).
    fn irq_publish_work() {
        axnet::publish_queue_event();
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
        // handler; 60/277 stay registered but masked.
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
        let adapter = match K3NetAdapter::new(drv, env) {
            Ok(a) => a,
            Err(e) => {
                line(&mut out, format_args!("[ms12] net err=adapter {e:?} stop"));
                park();
            }
        };

        // Arm the IRQ-context access BEFORE the handler registration (the
        // ISR may fire immediately after the route opens).
        arm_irq_access(&BOARD_IRQ_ACCESS);
        if !axhal::irq::register(GMAC_IRQ_SOURCE, gmac133_handler) {
            out("[ms12] net err=register-133 stop\n");
            park();
        }

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
            super::k3_net_diagnostics::run_diagnostics(&mut out);
            park()
        }
        #[cfg(not(feature = "k3-net-diagnostics"))]
        park()
    }
}

#[cfg(all(feature = "k3", target_arch = "riscv64"))]
pub use board::{run_k3_async_net_service, run_k3_async_net_session};

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
    const PHASE_CAP_NANOS: u64 = 10_000_000_000;

    /// `HOLD_TX_RECLAIM` and `RELEASE`, mirroring the diag constants.
    const OP_HOLD_TX_RECLAIM: u64 = 2;
    const OP_RELEASE: u64 = 3;

    // ── The protocol session phase (T6/T7) ────────────────────────────────
    /// The fixed echo ports (T6 contract).
    const UDP_ECHO_PORT: u16 = 26001;
    const TCP_ECHO_PORT: u16 = 26002;
    /// Per-operation bound: every session op is 5 s bounded.
    const SESSION_OP_NANOS: u64 = 5_000_000_000;
    /// Operator-paced session wait: 10 s-level per phase (T7 D6).
    const SESSION_WAIT_NANOS: u64 = 15_000_000_000;
    /// The quiet window: no new cause and no due protocol timer (T7).
    const QUIET_WINDOW_NANOS: u64 = 2_000_000_000;
    /// The bounded wait for the operator's post-quiet packet (T7).
    const RESUME_WAIT_NANOS: u64 = 10_000_000_000;
    /// Settle stillness span: the source-133 claim stream must be quiet for
    /// this long before the freeze window opens (V3 round-1: the peer's TCP
    /// close tail otherwise lands inside the quiet window).
    const SETTLE_QUIET_NANOS: u64 = 500_000_000;
    /// Absolute cap for the settle wait (the payload stops, never wedges).
    const SETTLE_CAP_NANOS: u64 = 10_000_000_000;

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
    /// ledgers — raw facts only, the payload never judges PASS.
    fn snapshot_line(out: &mut dyn FnMut(&str), tag: &str) {
        let s = axnet::rx_snapshot_v3();
        let st = axnet::stack_snapshot();
        let (ri, ti, abnormal, spurious) = irq_counts();
        line(
            out,
            format_args!(
                "[ms12] probe {tag} cause=raw={:#010x} irq-runs={} ri={} ti={} abnormal={} spurious={} drop=malformed-ip={} no-route={} src-mismatch={} rx-full={} tx-full={} occupancy=rx-slot={} tx-slot={} desc-inflight={} budget=reclaim-ex={} rx-ex={} submit-ex={} flush=target={} ok={} err={} busy={} cancel={} quiet=task-poll={} rounds={} work={} self-yield={} timer-wake={} fallback={} lifecycle={} owner={}",
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
            ),
        );
    }

    /// One UDP echo session (T6): bind `UDP_ECHO_PORT`, receive one
    /// datagram, echo every byte back to the sender. The session wait is
    /// operator-paced; every op is bounded and every outcome is printed.
    fn udp_echo_once(out: &mut dyn FnMut(&str)) {
        let session = async {
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

        // Resume: the operator sends one packet — a new source-133 claim
        // must wake the data plane and grow the counters again.
        let irq0 = irq_handler_runs();
        let baseline = axnet::rx_snapshot_v3();
        let deadline = wall_time() + Duration::from_nanos(RESUME_WAIT_NANOS);
        loop {
            let snap = axnet::rx_snapshot_v3();
            if irq_handler_runs() != irq0
                || snap.reaped != baseline.reaped
                || snap.isr_publish != baseline.isr_publish
            {
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
    pub(super) fn run_diagnostics(out: &mut dyn FnMut(&str)) {
        out_line(
            out,
            format_args!(
                "[ms12] probe begin slots={} descs={} hold-ms={} irq-source={}",
                SLOT_CAPACITY, DESC_CAPACITY, HOLD_LEASE_MS, GMAC_IRQ_SOURCE,
            ),
        );

        // ── Phase 1: the two-level capacity boundary under a reclaim hold ──
        if let Err(e) = axnet::diagnostic_control(OP_HOLD_TX_RECLAIM, HOLD_LEASE_MS) {
            out_line(out, format_args!("[ms12] probe err=hold {e:?} stop"));
            return;
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
                    return;
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
            return;
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
            return;
        }
        match axnet::diagnostic_enqueue_frame([0xff; 6], 0x0800, b"flush") {
            Ok(rep) if rep.accepted => {}
            Ok(_) => {
                out_line(out, format_args!("[ms12] probe err=flush-enqueue-full stop"));
                return;
            }
            Err(e) => {
                out_line(out, format_args!("[ms12] probe err=flush-enqueue {e:?} stop"));
                return;
            }
        }
        axtask::yield_now();
        let mut fut = match axnet::flush() {
            Ok(f) => core::pin::pin!(f),
            Err(e) => {
                out_line(out, format_args!("[ms12] probe err=flush {e:?} stop"));
                return;
            }
        };
        if !matches!(flush_poll(&mut fut), Poll::Pending) {
            out_line(out, format_args!("[ms12] probe err=flush-not-held stop"));
            return;
        }
        out_line(out, format_args!("[ms12] probe flush pending"));
        // THE SAME waiter observes the release: the explicit release lets the
        // owner reclaim the held ticket and C4 completes this future. (Never
        // drop and re-reserve: a dropped `Pin<&mut>` does not drop the pinned
        // future, so the sole waiter slot would stay taken.)
        if let Err(e) = axnet::diagnostic_control(OP_RELEASE, 0) {
            out_line(out, format_args!("[ms12] probe err=release-2 {e:?} stop"));
            return;
        }
        out_line(out, format_args!("[ms12] probe release-2 ok"));
        let deadline = wall_time() + Duration::from_nanos(PHASE_CAP_NANOS);
        let outcome = loop {
            match flush_poll(&mut fut) {
                Poll::Ready(result) => break result,
                Poll::Pending => {
                    if wall_time() >= deadline {
                        out_line(out, format_args!("[ms12] probe err=flush-cap stop"));
                        return;
                    }
                    axtask::yield_now();
                }
            }
        };
        match outcome {
            Ok(()) => out_line(out, format_args!("[ms12] probe flush c4 ok")),
            Err(e) => {
                out_line(out, format_args!("[ms12] probe err=flush-c4 {e:?} stop"));
                return;
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

        // ── Phase 4 (T6/T7): the protocol session, product-side snapshots
        // and the quiet/resume observation — every hold released above.
        run_protocol_phase(out);
    }
}
