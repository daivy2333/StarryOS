//! K3 stage 10 PHY device IRQ/status probe (MS10 Iteration 001, tasks 2.1–2.2).
//!
//! Task 2.1 delivers the host-testable device cause/ack core: the fixed D5
//! order in [`service_one_device_event`] snapshots every destructive cause
//! (the four read-only `syscon-gpio-edge` words, the pad MFPR35, the
//! clear-on-read INSR, the double-read BMSR link) *before* the pad ack sets
//! and re-arms MFPR35 CLEAR. The primitives live in [`super::phy_probe`]
//! (paged MDIO with page save/restore, raw edge snapshot, pad ack) and are
//! driven through [`HandlerAccess`] — a table of plain function pointers so
//! the `fn()` interrupt handler can reach the board without arguments (the
//! same static-arming convention as the stage 9 GENMSI address).
//!
//! Task 2.2 adds the stage 10 device window on the RAM boot path:
//!
//! - Foreground setup (window not open yet): enable the PHY link-change
//!   interrupt (INER a42:12 bit4), drain any stale INSR cause (INTB is
//!   level-held until the COR read), arm the pin 35 interrupt path (GPIO
//!   mux + falling edge per the board DTB phy node), read the initial
//!   link, register the source 60 handler plus the two MAC observation
//!   routes (133/277), and print the read-only route state — APLIC
//!   pending/enable/target for 60/133/277, GMAC `0x1160` and the APMU
//!   wake-enable bit12 (handoff value; the board DTB has no
//!   `spacemit,wake-irq-enable`).
//! - A fixed 120 s window: the only foreground work is pumping (host),
//!   printing newly arrived snapshots (`source`/raw status/link/aligned)
//!   and watching the budget. All device reads happen inside the source 60
//!   handler — snapshot first, ack last; the handler never prints and never
//!   shares the MDIO with the foreground (the foreground does no MDIO while
//!   the window is open).
//! - Bounded loss: at most [`STAGE10_SNAPSHOT_CAP`] snapshots. A full ring
//!   (a persistent level keeps the LevelHigh source asserting) is a defined
//!   run failure: the handler masks source 60 (APLIC CLRIE + local EIE via
//!   the armed access) without ever overwriting a not-yet-printed cause, and
//!   the foreground records the stop.
//! - After the window: a 1 s quiet check (zero new claims) and a factual
//!   result line. The kernel never judges PASS — the plug/unplug evidence is
//!   adjudicated from the serial wire (task 2.3).
//!
//! Board facts: source 60 is the `pinctrl@d401e000` interrupt (board DTB
//! `interrupts = <0x3c 0x04>` = 60, level-high); the PHY INTN pad is MFPR35.
//! Iteration 000 board evidence applies: EIE-masked MSIs are dropped, not
//! retained, so the mask layers are only used as stops, never as a pending
//! parking mechanism.

// Kernel builds bind the axplat platform crate's AIA core (single
// compilation of the production module); host harnesses bind the same
// source file as a root-module sibling named `k3_aia`.
#[cfg(feature = "k3")]
use axplat_riscv64_k3::aia as aia;
#[cfg(not(feature = "k3"))]
use super::k3_aia as aia;

use core::cell::UnsafeCell;
use core::fmt::Write as _;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use super::aia_probe::Observed;
use super::mac_probe::{APMU_BASE_PADDR, APMU_CTRL_OFFSET, MAC_BASE_PADDR};
use super::phy_probe::{self, BMSR_LINK, GPIO_EDGE_WORDS, PHY_INSR_LINK_CHANGE, ProbeEnv};

// ---------------------------------------------------------------------------
// Stage 10 parameters (task 2.2 contract)
// ---------------------------------------------------------------------------

/// Fixed observation window length (30 s: 2026-10-03 operator instruction
/// after the first board run — "稍微修改一下窗口上限，设置为30秒就行").
pub const STAGE10_WINDOW_NANOS: u64 = 30_000_000_000;
/// Quiet-window length after the stage window (zero new claims).
pub const STAGE10_QUIET_NANOS: u64 = 1_000_000_000;
/// Foreground poll slice inside the window and quiet checks.
const SLICE_NANOS: u64 = 10_000_000;
/// Maximum stored (and printed) snapshots; a further claim is overflow.
pub const STAGE10_SNAPSHOT_CAP: usize = 32;
/// The pinctrl interrupt source (board DTB `interrupts = <0x3c 4>`).
pub const STAGE10_SOURCE: usize = 60;
/// MAC observation routes (registered, displayed read-only, masked if firing).
pub const STAGE10_MAC_SOURCES: [usize; 2] = [133, 277];
/// GMAC read-only observation register (D5 fixed display contract).
pub const MAC_REG_OBS: u64 = 0x1160;
/// APMU EMAC ctrl WOL wake-enable bit (R69 `WOL_WAKE_IRQ_EN`; read-only
/// display of the handoff value).
pub const APMU_WAKE_ENABLE_BIT: u32 = 1 << 12;

// ---------------------------------------------------------------------------
// Handler access + device cause/ack core (task 2.1)
// ---------------------------------------------------------------------------

/// The one device read/ack surface the IRQ-context handler needs. Armed once
/// by the environment before the stage window opens; `mask_source` is the
/// dispatch-path emergency mask (APLIC CLRIE + local EIE clear).
pub struct HandlerAccess {
    pub read_edge_words: fn() -> [u32; GPIO_EDGE_WORDS],
    pub read_mfpr35: fn() -> u32,
    /// Writes MFPR35 and returns the readback.
    pub write_mfpr35: fn(value: u32) -> u32,
    /// One paged INSR read (clear-on-read on real silicon).
    pub read_insr_snapshot: fn() -> Result<u16, phy_probe::PagedError>,
    /// Two BMSR reads (latch-low: the second is the current level).
    pub read_bmsr_twice: fn() -> Result<(u16, u16), phy_probe::MdioError>,
    /// Emergency route mask used on snapshot-capacity exhaustion.
    pub mask_source: fn(source: usize),
}

/// One serviced device event. Every destructive cause is snapshotted before
/// the ack; raw values are kept whole (undecoded bits included).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceCauseSnapshot {
    pub edge: [u32; GPIO_EDGE_WORDS],
    pub mfpr35_before: u32,
    /// Raw INSR value; 0 with `insr_error` set when the MDIO read failed.
    pub insr: u16,
    pub insr_error: bool,
    pub bmsr1: u16,
    pub bmsr2: u16,
    pub bmsr_error: bool,
    pub mfpr35_set_readback: u32,
    pub mfpr35_rearm_readback: u32,
}

impl DeviceCauseSnapshot {
    /// All-zero placeholder for the static slot storage.
    pub const ZERO: Self = DeviceCauseSnapshot {
        edge: [0; GPIO_EDGE_WORDS],
        mfpr35_before: 0,
        insr: 0,
        insr_error: false,
        bmsr1: 0,
        bmsr2: 0,
        bmsr_error: false,
        mfpr35_set_readback: 0,
        mfpr35_rearm_readback: 0,
    };

    /// INSR bit4 (link-change cause) is set and the read did not fail.
    pub const fn link_change_cause(&self) -> bool {
        !self.insr_error && self.insr & PHY_INSR_LINK_CHANGE != 0
    }

    /// Current link level (second BMSR read; latch-low resolved).
    pub const fn current_link_up(&self) -> bool {
        !self.bmsr_error && self.bmsr2 & BMSR_LINK != 0
    }
}

/// The fixed D5 order for one device event: snapshot first — edge words, pad
/// MFPR, INSR (one COR read), BMSR ×2 — then ack the pad (set CLEAR, read
/// back, clear CLEAR). Nothing is printed and no console is touched; the
/// caller (the stage 10 handler) stores the snapshot for the foreground.
pub fn service_one_device_event(a: &HandlerAccess) -> DeviceCauseSnapshot {
    let edge = (a.read_edge_words)();
    let mfpr35_before = (a.read_mfpr35)();
    let (insr, insr_error) = match (a.read_insr_snapshot)() {
        Ok(v) => (v, false),
        Err(_) => (0, true),
    };
    let (bmsr1, bmsr2, bmsr_error) = match (a.read_bmsr_twice)() {
        Ok((v1, v2)) => (v1, v2, false),
        Err(_) => (0, 0, true),
    };
    let set_readback = (a.write_mfpr35)(phy_probe::pad_edge_ack_set(mfpr35_before));
    let rearm_readback = (a.write_mfpr35)(phy_probe::pad_edge_rearm(mfpr35_before));
    DeviceCauseSnapshot {
        edge,
        mfpr35_before,
        insr,
        insr_error,
        bmsr1,
        bmsr2,
        bmsr_error,
        mfpr35_set_readback: set_readback,
        mfpr35_rearm_readback: rearm_readback,
    }
}

// ---------------------------------------------------------------------------
// Environment seam (task 2.2)
// ---------------------------------------------------------------------------

/// Board access, route control and dispatch service for stage 10.
/// `ProbeEnv` covers the foreground MDIO/pad/edge/clock surface (setup only);
/// `AiaBackend` covers the APLIC/IMSIC surface shared with the chip.
pub trait Stage10Env: ProbeEnv + aia::AiaBackend {
    /// Host fakes pump their pending trap into the dispatcher; on the board
    /// the trap path claims asynchronously and this is a no-op.
    fn pump(&mut self);
    /// Full route registration (handler store + hardware enable).
    fn register_handler(&mut self, source: usize, handler: fn()) -> bool;
    /// Mask/unmask a registered route without removing its handler.
    fn set_route_enabled(&mut self, source: usize, enabled: bool);
    /// Chip dispatch diagnostics (read-only).
    fn observed(&self) -> Observed;
    /// The handler access table this environment arms for the window.
    fn handler_access(&self) -> &'static HandlerAccess;
}

// ---------------------------------------------------------------------------
// Handler-side storage (append-only; IRQ context never prints)
// ---------------------------------------------------------------------------

/// One write-once snapshot slot: data published under `written` (Release in
/// the handler, Acquire in the foreground). Slots below the printed
/// watermark are immutable, so append and drain never overlap.
struct SnapshotSlot {
    data: UnsafeCell<DeviceCauseSnapshot>,
    written: AtomicBool,
}

// SAFETY: the slot is written exactly once by the handler before the flag is
// published, and read by the foreground only after the flag; single hart.
unsafe impl Sync for SnapshotSlot {}

impl SnapshotSlot {
    const fn new() -> Self {
        SnapshotSlot {
            data: UnsafeCell::new(DeviceCauseSnapshot::ZERO),
            written: AtomicBool::new(false),
        }
    }
}

static SLOTS: [SnapshotSlot; STAGE10_SNAPSHOT_CAP] =
    [const { SnapshotSlot::new() }; STAGE10_SNAPSHOT_CAP];
static SNAP_COUNT: AtomicUsize = AtomicUsize::new(0);
static SNAP_OVERFLOW: AtomicBool = AtomicBool::new(false);
/// MAC observation events per source (same order as [`STAGE10_MAC_SOURCES`]).
static MAC_EVENTS: [AtomicUsize; 2] = [const { AtomicUsize::new(0) }; 2];
static HANDLER_ACCESS_PTR: AtomicUsize = AtomicUsize::new(0);

fn arm_handler_access(access: &'static HandlerAccess) {
    HANDLER_ACCESS_PTR.store(access as *const _ as usize, Ordering::Release);
}

fn armed_access() -> Option<&'static HandlerAccess> {
    let ptr = HANDLER_ACCESS_PTR.load(Ordering::Acquire) as *const HandlerAccess;
    if ptr.is_null() {
        None
    } else {
        // SAFETY: armed once per stage run from a &'static the environment
        // owns for the whole window; never freed.
        Some(unsafe { &*ptr })
    }
}

/// Read-only snapshot diagnostics for tests and the foreground summary.
pub fn probe_snapshot_count() -> usize {
    SNAP_COUNT.load(Ordering::Acquire)
}

/// Returns the stored snapshot `i`, if it was written.
pub fn probe_snapshot(i: usize) -> Option<DeviceCauseSnapshot> {
    if i >= SNAP_COUNT.load(Ordering::Acquire) || i >= STAGE10_SNAPSHOT_CAP {
        return None;
    }
    if !SLOTS[i].written.load(Ordering::Acquire) {
        return None;
    }
    // SAFETY: write-once slot published by the `written` flag.
    Some(unsafe { *SLOTS[i].data.get() })
}

/// The source 60 handler: snapshot, then ack, then publish. At capacity it
/// masks the route and flags overflow — it never overwrites a stored cause.
pub fn source60_handler() {
    let Some(access) = armed_access() else {
        return; // window not armed: nothing to service
    };
    let idx = SNAP_COUNT.load(Ordering::Relaxed);
    if idx >= STAGE10_SNAPSHOT_CAP {
        SNAP_OVERFLOW.store(true, Ordering::Relaxed);
        (access.mask_source)(STAGE10_SOURCE);
        return;
    }
    let snap = service_one_device_event(access);
    // SAFETY: slot `idx` is below the published count and written once here.
    unsafe {
        *SLOTS[idx].data.get() = snap;
    }
    SLOTS[idx].written.store(true, Ordering::Release);
    SNAP_COUNT.store(idx + 1, Ordering::Release);
}

/// MAC 133 observation handler: count, mask, never dispatch further.
pub fn mac133_handler() {
    mac_event(0, STAGE10_MAC_SOURCES[0]);
}

/// MAC 277 observation handler: count, mask, never dispatch further.
pub fn mac277_handler() {
    mac_event(1, STAGE10_MAC_SOURCES[1]);
}

fn mac_event(index: usize, source: usize) {
    MAC_EVENTS[index].fetch_add(1, Ordering::Relaxed);
    if let Some(access) = armed_access() {
        (access.mask_source)(source);
    }
}

// ---------------------------------------------------------------------------
// Output helper (foreground only)
// ---------------------------------------------------------------------------

macro_rules! stage10_line {
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

/// Counters of one completed stage 10 window (factual observations only;
/// PASS is adjudicated from the serial evidence, never by the kernel).
#[derive(Clone, Copy, Debug)]
pub struct Stage10Summary {
    pub claims: usize,
    pub aligned_down: usize,
    pub aligned_up: usize,
    pub unaligned: usize,
    pub mdio_errors: usize,
    pub printed: usize,
    pub quiet_clean: bool,
    pub quiet_claims: usize,
    pub mac_events: [usize; 2],
    pub window_elapsed_nanos: u64,
}

#[derive(Clone, Copy, Debug)]
pub enum Stage10Outcome {
    /// The fixed window ran to its end; the summary carries the observations.
    WindowEnded(Stage10Summary),
    /// Bounded stop: snapshot capacity exceeded (persistent level included).
    Stopped { reason: &'static str },
    /// Setup failed before the window opened (MDIO or registration).
    SetupFailed { phase: &'static str },
}

// ---------------------------------------------------------------------------
// Stage 10 flow
// ---------------------------------------------------------------------------

fn drain_snapshots<E: Stage10Env>(
    env: &mut E,
    out: &mut impl FnMut(&str),
    s: &mut Stage10Summary,
) {
    let cur = SNAP_COUNT.load(Ordering::Acquire).min(STAGE10_SNAPSHOT_CAP);
    while s.printed < cur && SLOTS[s.printed].written.load(Ordering::Acquire) {
        let idx = s.printed;
        // SAFETY: published write-once slot below the printed watermark.
        let snap = unsafe { *SLOTS[idx].data.get() };
        if snap.insr_error || snap.bmsr_error {
            s.mdio_errors += 1;
        } else if snap.link_change_cause() {
            if snap.current_link_up() {
                s.aligned_up += 1;
            } else {
                s.aligned_down += 1;
            }
        } else {
            s.unaligned += 1;
        }
        let raw = env.observed().last_raw_claim;
        stage10_line!(
            out,
            "[starry-k3] stage=10 claim idx={} source={} edge={:#010x}/{:#010x}/{:#010x}/{:#010x} mfpr={:#010x} insr={:#06x} bmsr={:#06x}/{:#06x} link={} aligned={} raw={:#x}",
            idx,
            STAGE10_SOURCE,
            snap.edge[0], snap.edge[1], snap.edge[2], snap.edge[3],
            snap.mfpr35_before,
            snap.insr,
            snap.bmsr1, snap.bmsr2,
            if snap.current_link_up() { "up" } else { "down" },
            if snap.link_change_cause() { 1 } else { 0 },
            raw
        );
        s.printed += 1;
        s.claims += 1;
    }
}

fn overflow_stop<E: Stage10Env>(env: &mut E, out: &mut impl FnMut(&str)) -> Stage10Outcome {
    env.set_route_enabled(STAGE10_SOURCE, false);
    stage10_line!(
        out,
        "[starry-k3] stage=10 err=snapshot-capacity-exceeded claims={} cap={} mask60=1 eie60=1 stop",
        SNAP_COUNT.load(Ordering::Acquire),
        STAGE10_SNAPSHOT_CAP
    );
    Stage10Outcome::Stopped {
        reason: "snapshot-capacity-exceeded",
    }
}

/// Runs the whole stage 10 sequence on `env`, reporting every phase through
/// `out`. The fixed window is never extended; failures stop before later
/// phases and name the failing layer.
pub fn run_stage10<E: Stage10Env>(env: &mut E, mut out: impl FnMut(&str)) -> Stage10Outcome {
    // Defensive: a previously failed run must never leak state into the next
    // (the statics are process-global in host harnesses).
    for slot in &SLOTS {
        slot.written.store(false, Ordering::Relaxed);
    }
    SNAP_COUNT.store(0, Ordering::Relaxed);
    SNAP_OVERFLOW.store(false, Ordering::Relaxed);
    for counter in &MAC_EVENTS {
        counter.store(0, Ordering::Relaxed);
    }
    arm_handler_access(env.handler_access());

    stage10_line!(
        out,
        "[starry-k3] stage=10 begin device-probe window-ms={} quiet-ms={} cap={} source={} mac={}/{}",
        STAGE10_WINDOW_NANOS / 1_000_000,
        STAGE10_QUIET_NANOS / 1_000_000,
        STAGE10_SNAPSHOT_CAP,
        STAGE10_SOURCE,
        STAGE10_MAC_SOURCES[0],
        STAGE10_MAC_SOURCES[1]
    );

    // --- Foreground setup: the window is not open yet (foreground MDIO). ---
    let iner = match phy_probe::iner_enable_link_change(env) {
        Ok(v) => v,
        Err(_) => {
            stage10_line!(out, "[starry-k3] stage=10 err=iner-enable phase=mdio stop");
            return Stage10Outcome::SetupFailed { phase: "iner-enable" };
        }
    };
    if !iner.stuck() {
        stage10_line!(
            out,
            "[starry-k3] stage=10 err=iner-stuck written={:#06x} rb={:#06x} stop",
            iner.written,
            iner.readback
        );
        return Stage10Outcome::SetupFailed { phase: "iner-enable" };
    }
    let link = match (
        phy_probe::mdio_read(env, phy_probe::PHY_ADDR, phy_probe::BMSR_REG),
        phy_probe::mdio_read(env, phy_probe::PHY_ADDR, phy_probe::BMSR_REG),
    ) {
        (Ok(v1), Ok(v2)) => (v1, v2),
        _ => {
            stage10_line!(out, "[starry-k3] stage=10 err=initial-link phase=mdio stop");
            return Stage10Outcome::SetupFailed { phase: "initial-link" };
        }
    };
    stage10_line!(
        out,
        "[starry-k3] stage=10 iner enable written={:#06x} rb={:#06x} initial bmsr={:#06x}/{:#06x} link={}",
        iner.written,
        iner.readback,
        link.0,
        link.1,
        if link.1 & BMSR_LINK != 0 { "up" } else { "down" }
    );

    // Drain any stale INSR cause (one COR read): RTL8211F INTB holds low
    // until the status register is read, so a pre-window cause must release
    // the line before the falling edge is armed — otherwise the first real
    // link change would never produce an edge.
    let drain = match phy_probe::paged_read(env, phy_probe::PHY_PAGE_INSR, phy_probe::PHY_INSR_REG)
    {
        Ok(v) => v,
        Err(_) => {
            stage10_line!(out, "[starry-k3] stage=10 err=insr-drain phase=mdio stop");
            return Stage10Outcome::SetupFailed { phase: "insr-drain" };
        }
    };
    stage10_line!(out, "[starry-k3] stage=10 insr-drain={:#06x}", drain);

    // Arm the pin 35 interrupt path (board DTB `ethernet-phy@1 interrupts
    // = <35 2>` falling edge; board run 6 falsified the armed-at-handoff
    // assumption: MFPR35 read 0xa440/0xa441 with edge enables clear and,
    // after stage 6, the GMAC mux). GPIO mux + FALL set, other bits kept.
    let pad = phy_probe::pad35_edge_setup(env);
    stage10_line!(
        out,
        "[starry-k3] stage=10 pad35 setup before={:#010x} written={:#010x} readback={:#010x} ok={}",
        pad.before,
        pad.written,
        pad.readback,
        pad.ok() as u8
    );
    if !pad.ok() {
        stage10_line!(out, "[starry-k3] stage=10 err=pad35-setup stop");
        return Stage10Outcome::SetupFailed { phase: "pad35-setup" };
    }

    if !env.register_handler(STAGE10_SOURCE, source60_handler) {
        stage10_line!(out, "[starry-k3] stage=10 err=register phase=source-60 stop");
        return Stage10Outcome::SetupFailed { phase: "register-60" };
    }
    if !env.register_handler(STAGE10_MAC_SOURCES[0], mac133_handler)
        || !env.register_handler(STAGE10_MAC_SOURCES[1], mac277_handler)
    {
        stage10_line!(out, "[starry-k3] stage=10 err=register phase=mac-routes stop");
        return Stage10Outcome::SetupFailed { phase: "register-mac" };
    }

    // --- Read-only route and MAC observation display (no triggering). ---
    for source in [STAGE10_SOURCE, STAGE10_MAC_SOURCES[0], STAGE10_MAC_SOURCES[1]] {
        let word = aia::APLIC_SETIP_BASE + 4 * (source as u32 / 32);
        let pending = env.aplic_read(word) & (1 << (source % 32)) != 0;
        let enable = env.aplic_read(aia::setie_word_off(source / 32)) & (1 << (source % 32)) != 0;
        let target = env.aplic_read(aia::target_off(source));
        stage10_line!(
            out,
            "[starry-k3] stage=10 route source={} pending={} enable={} target={:#x}",
            source,
            pending as u8,
            enable as u8,
            target
        );
    }
    let gmac_obs = env.mmio_read(MAC_BASE_PADDR + MAC_REG_OBS);
    let apmu_ctrl = env.mmio_read(APMU_BASE_PADDR + APMU_CTRL_OFFSET);
    stage10_line!(
        out,
        "[starry-k3] stage=10 mac-obs gmac1160={:#010x} apmu-ctrl={:#010x} apmu-wake-bit12={}",
        gmac_obs,
        apmu_ctrl,
        (apmu_ctrl & APMU_WAKE_ENABLE_BIT != 0) as u8
    );

    // --- Fixed observation window. ---
    stage10_line!(out, "[starry-k3] stage=10 window open");
    let t0 = env.now_nanos();
    let deadline = t0 + STAGE10_WINDOW_NANOS;
    let mut s = Stage10Summary {
        claims: 0,
        aligned_down: 0,
        aligned_up: 0,
        unaligned: 0,
        mdio_errors: 0,
        printed: 0,
        quiet_clean: true,
        quiet_claims: 0,
        mac_events: [0; 2],
        window_elapsed_nanos: 0,
    };
    loop {
        env.pump();
        drain_snapshots(env, &mut out, &mut s);
        if SNAP_OVERFLOW.load(Ordering::Relaxed) {
            return overflow_stop(env, &mut out);
        }
        if env.now_nanos() >= deadline {
            break;
        }
        env.wait_nanos(SLICE_NANOS);
    }
    s.window_elapsed_nanos = env.now_nanos() - t0;

    // Read-only close diagnostics: if the window ends without claims, these
    // raw values decide which layer to blame — edge words changed means the
    // pad latched but source 60 never reached the IMSIC; edge words clear
    // with INER stuck means no falling edge was ever seen (INTB or pad-arm
    // layer); setip60 set means the line asserted but the claim path lost it.
    let close_edge = phy_probe::read_edge_words(env);
    let close_mfpr35 = env.mmio_read(phy_probe::PAD35_MFPR_PADDR);
    let close_setip60 = env.aplic_read(aia::APLIC_SETIP_BASE + 4 * (STAGE10_SOURCE as u32 / 32))
        & (1 << (STAGE10_SOURCE % 32));
    stage10_line!(
        out,
        "[starry-k3] stage=10 window-close edge={:#010x}/{:#010x}/{:#010x}/{:#010x} mfpr35={:#010x} setip60={}",
        close_edge[0],
        close_edge[1],
        close_edge[2],
        close_edge[3],
        close_mfpr35,
        (close_setip60 != 0) as u8
    );

    // --- Quiet window: any new claim is recorded, never hidden. ---
    let quiet_base = SNAP_COUNT.load(Ordering::Acquire);
    let quiet_deadline = env.now_nanos() + STAGE10_QUIET_NANOS;
    while env.now_nanos() < quiet_deadline {
        env.pump();
        drain_snapshots(env, &mut out, &mut s);
        if SNAP_OVERFLOW.load(Ordering::Relaxed) {
            return overflow_stop(env, &mut out);
        }
        env.wait_nanos(SLICE_NANOS);
    }
    let quiet_claims = SNAP_COUNT.load(Ordering::Acquire).saturating_sub(quiet_base);
    s.quiet_claims = quiet_claims;
    s.quiet_clean = quiet_claims == 0;
    if SNAP_OVERFLOW.load(Ordering::Relaxed) {
        return overflow_stop(env, &mut out);
    }

    // --- MAC event report (only what actually fired). ---
    for (i, source) in STAGE10_MAC_SOURCES.iter().enumerate() {
        s.mac_events[i] = MAC_EVENTS[i].load(Ordering::Relaxed);
        if s.mac_events[i] > 0 {
            stage10_line!(
                out,
                "[starry-k3] stage=10 mac-event source={} count={} masked=1 raw={:#x}",
                source,
                s.mac_events[i],
                env.observed().last_raw_claim
            );
        }
    }

    let result = if s.claims == 0 {
        "no-device-event"
    } else if s.aligned_down > 0 && s.aligned_up > 0 {
        "link-down-and-up-observed"
    } else if s.aligned_down > 0 || s.aligned_up > 0 {
        "one-direction-link-change"
    } else {
        "no-aligned-claim"
    };
    stage10_line!(
        out,
        "[starry-k3] stage=10 result={} claims={} down={} up={} unaligned={} mdio-err={} quiet={} quiet-claims={} window-elapsed-ms={}",
        result,
        s.claims,
        s.aligned_down,
        s.aligned_up,
        s.unaligned,
        s.mdio_errors,
        s.quiet_clean as u8,
        s.quiet_claims,
        s.window_elapsed_nanos / 1_000_000
    );
    Stage10Outcome::WindowEnded(s)
}

// ---------------------------------------------------------------------------
// Board environment (K3 kernel builds only)
// ---------------------------------------------------------------------------

#[cfg(all(feature = "k3", target_arch = "riscv64"))]
mod board {
    use super::aia;
    use super::aia::AiaBackend as _;
    use super::super::aia_probe::Observed;
    use super::super::phy_probe;
    use super::super::phy_probe::ProbeEnv;
    use super::{HandlerAccess, Stage10Env, Stage10Outcome};

    /// Raw-MMIO + axhal-clock environment for the board: one instance per
    /// call (the armed handler fns build their own on the IRQ stack).
    pub struct K3Stage10Env;

    impl ProbeEnv for K3Stage10Env {
        fn mmio_read(&mut self, paddr: u64) -> u32 {
            // SAFETY: paddr is a probe constant inside an identity-mapped
            // DT-declared window (MAC/APMU/GPIO 0xd4019000/0x1000 incl. the
            // edge sub-window, and the pad page).
            unsafe { core::ptr::read_volatile(paddr as usize as *const u32) }
        }

        fn mmio_write(&mut self, paddr: u64, value: u32) {
            // SAFETY: same window contract; writes go only to the MDIO
            // engine and the MFPR35 pad register.
            unsafe { core::ptr::write_volatile(paddr as usize as *mut u32, value) }
        }

        fn now_nanos(&self) -> u64 {
            axhal::time::monotonic_time_nanos()
        }

        fn wait_nanos(&mut self, nanos: u64) {
            axhal::time::busy_wait(core::time::Duration::from_nanos(nanos))
        }
    }

    impl aia::AiaBackend for K3Stage10Env {
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

    impl Stage10Env for K3Stage10Env {
        fn pump(&mut self) {
            // Board: the trap path claims asynchronously; foreground polls.
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

        fn handler_access(&self) -> &'static HandlerAccess {
            &BOARD_ACCESS
        }
    }

    // IRQ-context raw access fns (local env per call; no console, no state).
    fn raw_read_edge_words() -> [u32; phy_probe::GPIO_EDGE_WORDS] {
        let mut env = K3Stage10Env;
        phy_probe::read_edge_words(&mut env)
    }

    fn raw_read_mfpr35() -> u32 {
        let mut env = K3Stage10Env;
        env.mmio_read(phy_probe::PAD35_MFPR_PADDR)
    }

    fn raw_write_mfpr35(value: u32) -> u32 {
        let mut env = K3Stage10Env;
        env.mmio_write(phy_probe::PAD35_MFPR_PADDR, value);
        env.mmio_read(phy_probe::PAD35_MFPR_PADDR)
    }

    fn raw_read_insr_snapshot() -> Result<u16, phy_probe::PagedError> {
        let mut env = K3Stage10Env;
        phy_probe::paged_read(&mut env, phy_probe::PHY_PAGE_INSR, phy_probe::PHY_INSR_REG)
    }

    fn raw_read_bmsr_twice() -> Result<(u16, u16), phy_probe::MdioError> {
        let mut env = K3Stage10Env;
        let v1 = phy_probe::mdio_read(&mut env, phy_probe::PHY_ADDR, phy_probe::BMSR_REG)?;
        let v2 = phy_probe::mdio_read(&mut env, phy_probe::PHY_ADDR, phy_probe::BMSR_REG)?;
        Ok((v1, v2))
    }

    /// Dispatch-path emergency mask: local EIE clear, then APLIC CLRIE (the
    /// Iteration 000 evidence says EIE-masked MSIs are dropped, which is the
    /// intended bounded-stop semantics here).
    fn raw_mask_source(source: usize) {
        let mut backend = aia::HardwareBackend;
        aia::set_eid_enabled(&mut backend, source, false);
        backend.aplic_write(
            aia::clrie_word_off(aia::bitmap_word(source)),
            aia::bitmap_bit(source),
        );
    }

    static BOARD_ACCESS: HandlerAccess = HandlerAccess {
        read_edge_words: raw_read_edge_words,
        read_mfpr35: raw_read_mfpr35,
        write_mfpr35: raw_write_mfpr35,
        read_insr_snapshot: raw_read_insr_snapshot,
        read_bmsr_twice: raw_read_bmsr_twice,
        mask_source: raw_mask_source,
    };

    /// K3 board entry: runs stage 10 on the RAM boot path. The caller parks
    /// afterwards regardless of the outcome.
    pub fn run_k3_stage10(mut out: impl FnMut(&str)) -> Stage10Outcome {
        // Stage 9's storm budget stop leaves sie.SEXT closed and route
        // registration never touches the hart-level enable — re-open it for
        // the device window.
        aia::HardwareBackend.set_sext(true);
        let mut env = K3Stage10Env;
        super::run_stage10(&mut env, |line| out(line))
    }
}

#[cfg(all(feature = "k3", target_arch = "riscv64"))]
pub use board::run_k3_stage10;
