//! K3 stage 9 software-injection probe for the AIA delivery base (MS10
//! Iteration 000, task 1.3).
//!
//! Runs after MS09 stage 8 on the RAM boot path and proves, with software
//! events only, that the APLIC MSI + IMSIC S-mode base brought up in
//! `init_later` actually delivers on this board:
//!
//! - SETIPNUM_LE and GENMSI each deliver three times; every event is one
//!   claim plus one handler call inside a 1 s deadline, and each phase ends
//!   with a 1 s quiet window (zero new claims).
//! - The APLIC source mask gates SETIPNUM_LE; the local EIE mask gates
//!   GENMSI (which bypasses the source enable, so it must never be used to
//!   prove a source mask). Re-enable requires a fresh trigger; an EIE-masked
//!   pending stays retained and is reported when serviced.
//! - A handler-raised GENMSI produces two independent claim/handler pairs
//!   (claim+EOI strictly before the handler; the new pending survives).
//! - An unknown EID (2, no handler) is claimed and diagnosed without panic.
//! - A handler-generated claim storm is stopped by the stage budget: after
//!   [`STAGE9_CLAIM_BUDGET`] events the foreground masks SEXT and the stage
//!   ends parked (diagnostic stop, not a delivery failure).
//!
//! All hardware access goes through [`Stage9Env`] so host tests drive the
//! real probe logic against a fake board (same convention as mac_probe/
//! phy_probe). The foreground prints only through the caller-provided sink
//! after hardware access; the IRQ handler never prints and never claims.

// Kernel builds bind the axplat platform crate's AIA core (single
// compilation of the production module); host harnesses bind the same
// source file as a root-module sibling named `k3_aia`.
#[cfg(feature = "k3")]
use axplat_riscv64_k3::aia as aia;
#[cfg(not(feature = "k3"))]
use super::k3_aia as aia;

use core::fmt::Write as _;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

/// Per-step deadline (trigger → expected claim) in nanoseconds.
pub const STAGE9_STEP_DEADLINE_NS: u64 = 1_000_000_000;
/// Quiet-window length (zero new claims) in nanoseconds.
pub const STAGE9_QUIET_NS: u64 = 1_000_000_000;
/// Non-zero-claim budget of the whole stage; exceeding it masks SEXT.
pub const STAGE9_CLAIM_BUDGET: usize = 32;
/// Foreground poll slice inside wait/quiet windows.
const SLICE_NS: u64 = 10_000_000;

// ---------------------------------------------------------------------------
// Observation + environment seam
// ---------------------------------------------------------------------------

/// Chip dispatch diagnostics observed from the foreground.
#[derive(Clone, Copy)]
pub struct Observed {
    pub last_raw_claim: u64,
    pub spurious: usize,
    pub unknown: usize,
}

/// Board access, dispatch service and route control for stage 9. `AiaBackend
///` supplies raw APLIC/CSR access; the extra methods cover what only the
/// production chip owns (route registration and the axhal-visible enable).
pub trait Stage9Env: aia::AiaBackend {
    /// Monotonic nanoseconds since boot (platform timebase).
    fn now_nanos(&self) -> u64;
    /// Busy-waits the given nanoseconds on the same timebase.
    fn wait_nanos(&mut self, ns: u64);
    /// Services at most one arrived interrupt. Board: a no-op — the trap
    /// path claims asynchronously; host fakes pump their pending trap into
    /// the dispatcher here.
    fn pump(&mut self);
    /// Full route registration (handler store + hardware enable).
    fn register_handler(&mut self, source: usize, handler: fn()) -> bool;
    /// Mask/unmask a registered route without removing its handler.
    fn set_route_enabled(&mut self, source: usize, enabled: bool);
    /// Chip dispatch diagnostics (read-only).
    fn observed(&self) -> Observed;
}

// ---------------------------------------------------------------------------
// Handler-side machinery (fn() statics; IRQ context never prints)
// ---------------------------------------------------------------------------

static HANDLER_EVENTS: AtomicUsize = AtomicUsize::new(0);
/// Handler re-trigger mode for the current event.
const MODE_NORMAL: usize = 0;
const MODE_RETRAP_ONCE: usize = 1;
const MODE_STORM: usize = 2;
static EVENT_MODE: AtomicUsize = AtomicUsize::new(MODE_NORMAL);
/// Board: GENMSI register vaddr the handler writes to re-trigger. 0 = host
/// mode (the flag below is consumed by the fake environment's pump).
static GENMSI_RAW_ADDR: AtomicUsize = AtomicUsize::new(0);
/// GENMSI payload for handler re-triggers (`hart<<18 | eid`).
static GENMSI_VALUE: AtomicU32 = AtomicU32::new(0);
static RETRAP_REQUESTED: AtomicBool = AtomicBool::new(false);
/// Storm phase event base (foreground sets it when arming MODE_STORM).
static STORM_BASE: AtomicUsize = AtomicUsize::new(0);
/// Set by the handler when the claim budget is reached; on riscv64 the
/// handler also closes SEXT directly (dispatch-path budget stop, so a dense
/// storm can never starve the foreground before its deadline check).
static SEXT_MASK_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Total handler invocations since boot (one per successful claim).
pub fn handler_event_count() -> usize {
    HANDLER_EVENTS.load(Ordering::Relaxed)
}

/// Sets the GENMSI address the handler uses for re-triggers (board only).
pub fn set_genmsi_raw_addr(addr: usize) {
    GENMSI_RAW_ADDR.store(addr, Ordering::Relaxed);
}

/// Consumes a handler re-trigger request (host fake environment).
pub fn take_handler_retrap_request() -> bool {
    RETRAP_REQUESTED.swap(false, Ordering::Relaxed)
}

/// GENMSI payload for handler re-triggers (host fake environment).
pub fn handler_retrap_value() -> u32 {
    GENMSI_VALUE.load(Ordering::Relaxed)
}

fn stage9_handler() {
    let events = HANDLER_EVENTS.fetch_add(1, Ordering::Relaxed) + 1;
    match EVENT_MODE.load(Ordering::Relaxed) {
        MODE_RETRAP_ONCE => {
            EVENT_MODE.store(MODE_NORMAL, Ordering::Relaxed);
            request_genmsi();
        }
        MODE_STORM => {
            if events >= STORM_BASE.load(Ordering::Relaxed) + STAGE9_CLAIM_BUDGET {
                storm_budget_stop();
            } else {
                request_genmsi();
            }
        }
        _ => {}
    }
}

/// Budget stop: close SEXT here (dispatch path) and flag the foreground.
fn storm_budget_stop() {
    SEXT_MASK_REQUESTED.store(true, Ordering::Relaxed);
    #[cfg(target_arch = "riscv64")]
    // SAFETY: clears sie.SEXT on the local hart inside the interrupt
    // handler; the stage ends parked right after this phase.
    unsafe {
        riscv::register::sie::clear_sext()
    }
}

/// Consumes the handler-raised budget-stop flag (host fake environment; on
/// riscv64 the handler already closed SEXT and this is a mirror).
pub fn take_sext_mask_request() -> bool {
    SEXT_MASK_REQUESTED.swap(false, Ordering::Relaxed)
}


fn request_genmsi() {
    let addr = GENMSI_RAW_ADDR.load(Ordering::Relaxed);
    if addr != 0 {
        // SAFETY (board): identity-mapped APLIC GENMSI register set by the
        // platform entry; a single volatile write, no console, no loop.
        unsafe { (addr as *mut u32).write_volatile(GENMSI_VALUE.load(Ordering::Relaxed)) }
    } else {
        RETRAP_REQUESTED.store(true, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// Output helper (foreground only)
// ---------------------------------------------------------------------------

macro_rules! stage9_line {
    ($out:expr, $($arg:tt)*) => {{
        let mut lb = aia::LineBuf::new();
        let _ = core::write!(&mut lb, $($arg)*);
        // 002 Cycle newline fix: stage 9+ wire lines carry a trailing \n
        // (stage 1-8 convention).
        let _ = lb.write_str("\n");
        $out(lb.as_str());
    }};
}

// ---------------------------------------------------------------------------
// Outcome
// ---------------------------------------------------------------------------

/// Counters of one completed stage 9 run (absolute values are deltas of the
/// run; raw values are the chip's most recent claims of each entry point).
pub struct Stage9Summary {
    pub setip_events: usize,
    pub genmsi_events: usize,
    pub last_setip_raw: u64,
    pub last_genmsi_raw: u64,
    pub aplic_mask_events: usize,
    pub aplic_retained: usize,
    pub aplic_fresh: bool,
    pub eie_mask_events: usize,
    pub eie_retained: usize,
    pub eie_fresh: bool,
    pub retrap_events: usize,
    pub unknown_delta: usize,
    pub drained: usize,
    pub storm_events: usize,
    pub storm_quiet: bool,
}

pub enum Stage9Outcome {
    Passed(Stage9Summary),
    Failed {
        phase: &'static str,
        reason: &'static str,
    },
}

// ---------------------------------------------------------------------------
// Bounded wait helpers
// ---------------------------------------------------------------------------

fn wait_for_delta(
    env: &mut impl Stage9Env,
    base: usize,
    want: usize,
    phase: &'static str,
) -> Result<usize, Stage9Outcome> {
    let start = env.now_nanos();
    loop {
        env.pump();
        let events = handler_event_count();
        if events >= base + want {
            return Ok(events - base);
        }
        if env.now_nanos() - start >= STAGE9_STEP_DEADLINE_NS {
            return Err(Stage9Outcome::Failed { phase, reason: "deadline" });
        }
        env.wait_nanos(SLICE_NS);
    }
}

/// Waits one quiet window; any new claim fails the phase (the delta is
/// returned when zero).
fn quiet_window(env: &mut impl Stage9Env, phase: &'static str) -> Result<usize, Stage9Outcome> {
    let start = env.now_nanos();
    let base = handler_event_count();
    while env.now_nanos() - start < STAGE9_QUIET_NS {
        env.pump();
        if handler_event_count() != base {
            return Err(Stage9Outcome::Failed { phase, reason: "quiet-claim" });
        }
        env.wait_nanos(SLICE_NS);
    }
    Ok(0)
}

/// Collects (and allows) events for one window; returns the delta. Used to
/// service EIE-mask-retained pendings right after a re-enable.
fn collect_window(env: &mut impl Stage9Env) -> usize {
    let start = env.now_nanos();
    let base = handler_event_count();
    while env.now_nanos() - start < STAGE9_QUIET_NS {
        env.pump();
        env.wait_nanos(SLICE_NS);
    }
    handler_event_count() - base
}

fn send_genmsi(
    env: &mut impl Stage9Env,
    hart: usize,
    eid: usize,
    phase: &'static str,
) -> Result<(), Stage9Outcome> {
    let start = env.now_nanos();
    loop {
        if env.aplic_read(aia::APLIC_GENMSI) & aia::APLIC_GENMSI_BUSY == 0 {
            break;
        }
        if env.now_nanos() - start >= STAGE9_STEP_DEADLINE_NS {
            return Err(Stage9Outcome::Failed { phase, reason: "genmsi-busy" });
        }
        env.wait_nanos(SLICE_NS);
    }
    env.aplic_write(aia::APLIC_GENMSI, aia::target_value(hart, 0, eid));
    Ok(())
}

/// Bounded drain of retained local pendings (each drain is a full atomic
/// claim+EOI of one stale event); stops at `max` or when nothing claims.
fn drain_local_pending(env: &mut impl Stage9Env, max: usize) -> usize {
    let mut drained = 0;
    while drained < max {
        let raw = env.with_imsic(|c| c.claim());
        if aia::parse_stopei(raw).is_none() {
            break;
        }
        drained += 1;
    }
    drained
}

fn failed(
    out: &mut impl FnMut(&str),
    phase: &'static str,
    reason: &'static str,
) -> Stage9Outcome {
    stage9_line!(out, "[starry-k3] stage=9 fail phase={} reason={}", phase, reason);
    Stage9Outcome::Failed { phase, reason }
}

// ---------------------------------------------------------------------------
// Stage 9 flow
// ---------------------------------------------------------------------------

/// Runs the whole stage 9 sequence on `env`, reporting every phase through
/// `out`. Never panics on IRQ-side events; failures stop before later
/// phases and report the failing layer.
pub fn run_stage9<E: Stage9Env>(env: &mut E, mut out: impl FnMut(&str)) -> Stage9Outcome {
    // Defensive: a previously failed run must never leak a handler mode or
    // a stale budget-stop flag (the statics are process-global in host
    // harnesses that run several stage9 instances).
    EVENT_MODE.store(MODE_NORMAL, Ordering::Relaxed);
    SEXT_MASK_REQUESTED.store(false, Ordering::Relaxed);
    stage9_line!(out, "[starry-k3] stage=9 begin aia-software-probe budget={}", STAGE9_CLAIM_BUDGET);
    let rb = aia::readback(env, 1);
    stage9_line!(
        out,
        "[starry-k3] stage=9 readback domaincfg={:#x} msi={:#x}/{:#x} delivery={} threshold={} srcfg={:#x} target={:#x} eie0={:#x}",
        rb.domaincfg, rb.smsicfg_low, rb.smsicfg_high, rb.delivery, rb.threshold,
        rb.sourcecfg, rb.target, rb.eie_word0
    );

    if !env.register_handler(1, stage9_handler) {
        return failed(&mut out, "register", "source-1-registration-rejected");
    }
    let hart = env.hart();
    GENMSI_VALUE.store(aia::target_value(hart, 0, 1), Ordering::Relaxed);
    let mut s = Stage9Summary {
        setip_events: 0,
        genmsi_events: 0,
        last_setip_raw: 0,
        last_genmsi_raw: 0,
        aplic_mask_events: 0,
        aplic_retained: 0,
        aplic_fresh: false,
        eie_mask_events: 0,
        eie_retained: 0,
        eie_fresh: false,
        retrap_events: 0,
        unknown_delta: 0,
        drained: 0,
        storm_events: 0,
        storm_quiet: false,
    };

    // A2a: SETIPNUM_LE ×3, one claim + one handler call each.
    for i in 1..=3 {
        let base = handler_event_count();
        env.aplic_write(aia::APLIC_SETIPNUM_LE, 1);
        if let Err(e) = wait_for_delta(env, base, 1, "setip-x3") {
            // Delivery discriminator: did the trigger land in the APLIC
            // (setip bitmap) and is the route armed? pending-set + no claim
            // ⇒ MSI address config did not land; pending clear ⇒ the
            // SETIPNUM write itself failed.
            let setip0 = env.aplic_read(aia::APLIC_SETIP_BASE);
            let setie0 = env.aplic_read(aia::APLIC_SETIE_BASE);
            let target1 = env.aplic_read(aia::target_off(1));
            let domaincfg = env.aplic_read(aia::APLIC_DOMAINCFG);
            stage9_line!(
                out,
                "[starry-k3] stage=9 setip-timeout setip0={setip0:#x} setie0={setie0:#x} target1={target1:#x} domaincfg={domaincfg:#x}"
            );
            return e;
        }
        s.last_setip_raw = env.observed().last_raw_claim;
        stage9_line!(out, "[starry-k3] stage=9 setip trigger={} raw={:#x}", i, s.last_setip_raw);
    }
    s.setip_events = 3;
    match quiet_window(env, "setip-x3") {
        Ok(0) => {}
        Ok(_) => return failed(&mut out, "setip-x3", "quiet-claim-extra"),
        Err(e) => return e,
    }

    // A2b: GENMSI ×3.
    for i in 1..=3 {
        let base = handler_event_count();
        if let Err(e) = send_genmsi(env, hart, 1, "genmsi-x3") {
            return e;
        }
        if let Err(e) = wait_for_delta(env, base, 1, "genmsi-x3") {
            return e;
        }
        s.last_genmsi_raw = env.observed().last_raw_claim;
        stage9_line!(out, "[starry-k3] stage=9 genmsi trigger={} raw={:#x}", i, s.last_genmsi_raw);
    }
    s.genmsi_events = 3;
    match quiet_window(env, "genmsi-x3") {
        Ok(_) => {}
        Err(e) => return e,
    }

    // A3a: the APLIC source mask gates SETIPNUM_LE.
    env.set_route_enabled(1, false);
    env.aplic_write(aia::APLIC_SETIPNUM_LE, 1);
    match quiet_window(env, "aplic-mask") {
        Ok(d) => s.aplic_mask_events = d,
        Err(e) => return e,
    }
    aia::clear_source_pending(env, 1);
    env.set_route_enabled(1, true);
    let base = handler_event_count();
    env.aplic_write(aia::APLIC_SETIPNUM_LE, 1);
    if let Err(e) = wait_for_delta(env, base, 1, "aplic-mask") {
        return e;
    }
    s.aplic_fresh = true;
    match quiet_window(env, "aplic-mask") {
        Ok(_) => {}
        Err(e) => return e,
    }
    stage9_line!(out, "[starry-k3] stage=9 aplic-mask blocked=1 fresh=1");

    // A3b: the local EIE mask gates GENMSI; a masked pending is retained.
    env.set_route_enabled(1, false);
    if let Err(e) = send_genmsi(env, hart, 1, "eie-mask") {
        return e;
    }
    match quiet_window(env, "eie-mask") {
        Ok(d) => s.eie_mask_events = d,
        Err(e) => return e,
    }
    let drained = drain_local_pending(env, STAGE9_CLAIM_BUDGET);
    s.drained += drained;
    env.set_route_enabled(1, true);
    s.eie_retained = collect_window(env);
    match quiet_window(env, "eie-mask") {
        Ok(_) => {}
        Err(e) => return e,
    }
    let base = handler_event_count();
    if let Err(e) = send_genmsi(env, hart, 1, "eie-mask") {
        return e;
    }
    if let Err(e) = wait_for_delta(env, base, 1, "eie-mask") {
        return e;
    }
    s.eie_fresh = true;
    match quiet_window(env, "eie-mask") {
        Ok(_) => {}
        Err(e) => return e,
    }
    stage9_line!(
        out,
        "[starry-k3] stage=9 eie-mask blocked=1 drained={} retained={} fresh=1",
        s.drained, s.eie_retained
    );

    // A4: a handler-raised GENMSI survives as an independent claim.
    EVENT_MODE.store(MODE_RETRAP_ONCE, Ordering::Relaxed);
    let base = handler_event_count();
    if let Err(e) = send_genmsi(env, hart, 1, "retrap") {
        return e;
    }
    match wait_for_delta(env, base, 2, "retrap") {
        Ok(d) => s.retrap_events = d,
        Err(e) => return e,
    }
    match quiet_window(env, "retrap") {
        Ok(_) => {}
        Err(e) => return e,
    }
    stage9_line!(out, "[starry-k3] stage=9 retrap independent-claims={}", s.retrap_events);

    // A5a: an unknown EID is claimed, diagnosed, and never dispatched.
    let unknown_base = env.observed().unknown;
    aia::set_eid_enabled(env, 2, true);
    if let Err(e) = send_genmsi(env, hart, 2, "unknown-eid") {
        return e;
    }
    let start = env.now_nanos();
    loop {
        env.pump();
        if env.observed().unknown > unknown_base {
            break;
        }
        if env.now_nanos() - start >= STAGE9_STEP_DEADLINE_NS {
            return failed(&mut out, "unknown-eid", "unknown-timeout");
        }
        env.wait_nanos(SLICE_NS);
    }
    s.unknown_delta = env.observed().unknown - unknown_base;
    aia::set_eid_enabled(env, 2, false);
    s.drained += drain_local_pending(env, STAGE9_CLAIM_BUDGET);
    match quiet_window(env, "unknown-eid") {
        Ok(_) => {}
        Err(e) => return e,
    }
    stage9_line!(out, "[starry-k3] stage=9 unknown-eid delta={} no-handler=1", s.unknown_delta);

    // A5b: claim storm stopped by the budget (handler-side mask, deadline
    // as backup); SEXT masked, then quiet.
    EVENT_MODE.store(MODE_STORM, Ordering::Relaxed);
    let base = handler_event_count();
    STORM_BASE.store(base, Ordering::Relaxed);
    if let Err(e) = send_genmsi(env, hart, 1, "storm") {
        return e;
    }
    let start = env.now_nanos();
    loop {
        env.pump();
        let delta = handler_event_count() - base;
        if delta >= STAGE9_CLAIM_BUDGET || take_sext_mask_request() {
            env.set_sext(false);
            take_sext_mask_request();
            s.storm_events = delta;
            break;
        }
        if env.now_nanos() - start >= 2 * STAGE9_STEP_DEADLINE_NS {
            return failed(&mut out, "storm", "budget-not-reached");
        }
    }
    match quiet_window(env, "storm") {
        Ok(_) => s.storm_quiet = true,
        Err(e) => return e,
    }
    EVENT_MODE.store(MODE_NORMAL, Ordering::Relaxed);
    stage9_line!(out, "[starry-k3] stage=9 storm events={} sext-masked quiet=1", s.storm_events);

    let eip = aia::read_eip_snapshot(env);
    stage9_line!(
        out,
        "[starry-k3] stage=9 pass setip=3 genmsi=3 aplic_mask={} aplic_fresh=1 eie_mask={} eie_retained={} eie_fresh=1 retrap={} unknown={} drained={} storm={} eip0={:#x}",
        s.aplic_mask_events, s.eie_mask_events, s.eie_retained,
        s.retrap_events, s.unknown_delta, s.drained, s.storm_events, eip[0]
    );
    Stage9Outcome::Passed(s)
}

// ---------------------------------------------------------------------------
// Board environment (K3 kernel builds only)
// ---------------------------------------------------------------------------

#[cfg(feature = "k3")]
pub struct K3Stage9Env;

#[cfg(feature = "k3")]
impl aia::AiaBackend for K3Stage9Env {
    fn aplic_read(&mut self, offset: u32) -> u32 {
        aia::HardwareBackend.aplic_read(offset)
    }

    fn aplic_write(&mut self, offset: u32, value: u32) {
        aia::HardwareBackend.aplic_write(offset, value)
    }

    fn with_imsic<R>(&mut self, f: impl FnOnce(&mut dyn aia::ImsicAccess) -> R) -> R {
        aia::HardwareBackend.with_imsic(f)
    }

    fn set_sext(&mut self, on: bool) {
        aia::HardwareBackend.set_sext(on)
    }

    fn hart(&self) -> usize {
        aia::HardwareBackend.hart()
    }
}

#[cfg(feature = "k3")]
impl Stage9Env for K3Stage9Env {
    fn now_nanos(&self) -> u64 {
        axhal::time::monotonic_time_nanos()
    }

    fn wait_nanos(&mut self, ns: u64) {
        axhal::time::busy_wait(core::time::Duration::from_nanos(ns));
    }

    fn pump(&mut self) {
        // Board: the trap path claims asynchronously; foreground only polls.
    }

    fn register_handler(&mut self, source: usize, handler: fn()) -> bool {
        axhal::irq::register(source, handler)
    }

    fn set_route_enabled(&mut self, source: usize, enabled: bool) {
        axhal::irq::set_enable(source, enabled)
    }

    fn observed(&self) -> Observed {
        Observed {
            last_raw_claim: axplat_riscv64_k3::irq::probe::last_raw_claim(),
            spurious: axplat_riscv64_k3::irq::probe::spurious_count(),
            unknown: axplat_riscv64_k3::irq::probe::unknown_count(),
        }
    }
}

/// K3 board entry: arms the handler's raw GENMSI re-trigger address and runs
/// the stage. The caller parks afterwards regardless of the outcome.
#[cfg(feature = "k3")]
pub fn run_k3_stage9(mut out: impl FnMut(&str)) -> Stage9Outcome {
    // The handler's re-trigger writes GENMSI through the identity-mapped
    // APLIC window that init adjudicated (DT window or +0x4000 S window).
    let genmsi_addr = aia::aplic_base() + aia::APLIC_GENMSI as usize;
    set_genmsi_raw_addr(genmsi_addr);
    let mut env = K3Stage9Env;
    run_stage9(&mut env, |line| out(line))
}
