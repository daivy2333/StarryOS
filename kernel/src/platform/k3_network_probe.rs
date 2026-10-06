//! K3 stage 11: raw GMAC RX observation window (MS11 Iteration 000, task
//! 1.3) and stage 12: raw bidirectional frames with peer delivery (MS11
//! Iteration 001, task 2.2).
//!
//! The foreground sequence on the RAM boot path: after stage 9 passed and
//! stage 10 ended with a normal [`Stage10Outcome::WindowEnded`], the three
//! device routes (60/133/277) are masked and the polling GMAC driver becomes
//! the single foreground MDIO/DMA owner. Stage 11 runs one bounded 30 s RX
//! window (the MS10 operator convention): it prints initialization progress,
//! raw descriptor/status/length evidence and the first matching ARP request
//! sample, then keeps the DMA backing. Stage 12 — only after a normally
//! ended stage 11 — re-initializes on a fresh second arena, enables TX and
//! sends 20 software-built ARP who-has requests (100 ms apart, independent
//! ≤1 s terminals) while continuing to poll RX, then runs a bounded 10 s RX
//! window for peer replies. Local terminals and peer-reply evidence are
//! reported separately; the kernel never judges PASS — the operator
//! adjudicates the wire against the peer capture. Both stages keep their DMA
//! backing and park afterwards.
//!
//! Board access goes through [`Stage11Env`] (the driver's [`GmacEnv`] plus
//! the route mask) so host tests drive the real gate/window logic against a
//! fake board.

#[cfg(feature = "k3")]
use axplat_riscv64_k3::aia as aia;
#[cfg(not(feature = "k3"))]
use super::k3_aia as aia;

#[cfg(feature = "k3")]
use crate::platform::k3_gmac as gmac;
#[cfg(not(feature = "k3"))]
use crate::k3_gmac as gmac;

#[cfg(feature = "k3-polling-net")]
use gmac::polling_net;

#[cfg(feature = "k3")]
use crate::platform::k3_device_irq_probe as irq_probe;
#[cfg(not(feature = "k3"))]
use super::k3_device_irq_probe as irq_probe;

use core::fmt::Write as _;

use gmac::{
    ArenaBacking, ArenaGuard, Gmac, GmacEnv, GmacError, RxPoll, TxPoll, TxSubmit, TxSubmitError,
    TX_TERMINAL_NANOS,
};

// ---------------------------------------------------------------------------
// Stage 11 parameters (task 1.3 contract)
// ---------------------------------------------------------------------------

/// Fixed RX observation window (30 s: the MS10 operator convention for the
/// stage 10/stage 8 windows, applied to this stage's wire).
pub const STAGE11_WINDOW_NANOS: u64 = 30_000_000_000;
/// Idle poll wait between RX sweeps.
const STAGE11_POLL_STEP_NANOS: u64 = 1_000_000;
/// Maximum printed per-frame sample lines (frames and drops each); the
/// summary line is always printed.
pub const STAGE11_SAMPLE_CAP: usize = 8;
/// Caller scratch for one FCS-stripped frame.
pub const STAGE11_SCRATCH_LEN: usize = 1514;

// ---------------------------------------------------------------------------
// Outcome and environment
// ---------------------------------------------------------------------------

/// Counters of one completed stage 11 window (factual observations only).
#[derive(Clone, Copy, Debug, Default)]
pub struct Stage11Summary {
    /// Completed frames copied by the driver.
    pub frames: usize,
    /// Classified non-deliverable descriptors.
    pub drops: usize,
    /// Bounded-stop revocations (deadline/cancel) observed.
    pub revocations: usize,
    /// Sample lines actually printed.
    pub printed: usize,
    /// Whether at least one ARP request sample was printed.
    pub arp_sample_printed: bool,
    pub window_elapsed_nanos: u64,
}

#[derive(Clone, Copy, Debug)]
pub enum Stage11Outcome {
    /// The fixed window ran to its end; the summary carries the counts.
    WindowEnded(Stage11Summary),
    /// A bounded stop (deadline/cancel revocation) closed the driver early.
    Stopped { reason: &'static str },
    /// Setup failed before or during initialization (gate/arena/init).
    SetupFailed { phase: &'static str },
    /// Fatal DMA bus error; the driver faulted and the stop ran.
    Fault { raw_status: u32 },
}

/// Stage 11 environment: the driver seam plus the foreground route mask.
/// The board implementation clears the local EIE + APLIC route via
/// `axhal::irq::set_enable`; host fakes record the call.
pub trait Stage11Env: GmacEnv {
    /// Masks one registered device route before the foreground owner starts.
    fn mask_source(&mut self, source: usize);
}

// ---------------------------------------------------------------------------
// Core gate + window (host-testable over a fake board)
// ---------------------------------------------------------------------------

/// Ethernet/ARP request shape check for sample selection only (not a
/// verdict): IPv4/ARP ethertype and ARP operation 1 (who-has request).
fn is_arp_request(frame: &[u8]) -> bool {
    frame.len() >= 42 && frame[12] == 0x08 && frame[13] == 0x06 && frame[20] == 0 && frame[21] == 1
}

macro_rules! stage11_line {
    ($out:expr, $($arg:tt)*) => {{
        let mut lb = aia::LineBuf::new();
        let _ = core::write!(&mut lb, $($arg)*);
        // 002 Cycle newline fix: stage 9+ wire lines carry a trailing \n
        // (stage 1-8 convention).
        let _ = lb.write_str("\n");
        $out(lb.as_str());
    }};
}

macro_rules! stage12_line {
    ($out:expr, $($arg:tt)*) => {{
        let mut lb = aia::LineBuf::new();
        let _ = core::write!(&mut lb, $($arg)*);
        // 002 Cycle newline fix: stage 9+ wire lines carry a trailing \n
        // (stage 1-8 convention).
        let _ = lb.write_str("\n");
        $out(lb.as_str());
    }};
}

#[cfg(feature = "k3-polling-net")]
macro_rules! stage13_line {
    ($out:expr, $($arg:tt)*) => {{
        let mut lb = aia::LineBuf::new();
        let _ = core::write!(&mut lb, $($arg)*);
        // Born with the 002 Cycle trailing newline convention.
        let _ = lb.write_str("\n");
        $out(lb.as_str());
    }};
}

/// Runs the whole stage 11 sequence on `env`: the stage 10 gate, the route
/// masks, driver initialization and the fixed RX window. Every report goes
/// through `out`; failures name the failing layer and never disturb
/// device-owned state. The arena backing is consumed and retained on every
/// path after the publish point (the caller parks afterwards).
pub fn run_stage11<E: Stage11Env, B: ArenaBacking>(
    env: &mut E,
    st10: irq_probe::Stage10Outcome,
    arena: ArenaGuard<B>,
    out: &mut impl FnMut(&str),
) -> Stage11Outcome {
    // Gate: only a normally ended stage 10 window opens the GMAC stage.
    if !matches!(st10, irq_probe::Stage10Outcome::WindowEnded(_)) {
        stage11_line!(out, "[starry-k3] stage=11 err=gate stage10-not-window-ended stop");
        return Stage11Outcome::SetupFailed { phase: "gate" };
    }
    // Mask the three device routes BEFORE the foreground MDIO/DMA owner
    // starts (the stage 10 handlers still read MDIO when unmasked).
    for source in irq_probe::STAGE10_MAC_SOURCES {
        env.mask_source(source);
    }
    env.mask_source(irq_probe::STAGE10_SOURCE);
    stage11_line!(
        out,
        "[starry-k3] stage=11 begin window-ms={} scratch={} masks=60/133/277",
        STAGE11_WINDOW_NANOS / 1_000_000,
        STAGE11_SCRATCH_LEN
    );

    let mut driver = match Gmac::new(arena) {
        Ok(d) => d,
        Err(e) => {
            stage11_line!(out, "[starry-k3] stage=11 err=arena {e:?} stop");
            return Stage11Outcome::SetupFailed { phase: "arena" };
        }
    };
    match driver.init_rx(env) {
        Ok(link) => {
            stage11_line!(out, "[starry-k3] stage=11 init {link:?} rx-running");
        }
        Err(e) => {
            stage11_line!(out, "[starry-k3] stage=11 err=init {e:?} stop");
            return Stage11Outcome::SetupFailed { phase: "init" };
        }
    }

    // Bounded RX window: fixed poll budget, raw evidence samples, summary.
    let start = env.now_nanos();
    let deadline = start + STAGE11_WINDOW_NANOS;
    let mut summary = Stage11Summary::default();
    let mut arp_sample_done = false;
    let mut scratch = [0u8; STAGE11_SCRATCH_LEN];
    while env.now_nanos() < deadline {
        match driver.poll_rx(env, &mut scratch) {
            Ok(RxPoll::Idle) => {
                env.wait_nanos(STAGE11_POLL_STEP_NANOS);
            }
            Ok(RxPoll::Frame { len, slot, raw_w3 }) => {
                summary.frames += 1;
                let arp = is_arp_request(&scratch[..len]);
                if arp && !arp_sample_done {
                    arp_sample_done = true;
                    summary.arp_sample_printed = true;
                    print_frame(out, 11, "arp-req", slot, len, raw_w3, &scratch[..len]);
                    summary.printed += 1;
                } else if summary.printed < STAGE11_SAMPLE_CAP {
                    print_frame(out, 11, "frame", slot, len, raw_w3, &[]);
                    summary.printed += 1;
                }
            }
            Ok(RxPoll::Dropped { slot, reason, raw_w3 }) => {
                summary.drops += 1;
                if summary.printed < STAGE11_SAMPLE_CAP {
                    stage11_line!(
                        out,
                        "[starry-k3] stage=11 drop slot={} reason={reason:?} raw={raw_w3:#010x}",
                        slot
                    );
                    summary.printed += 1;
                }
            }
            Err(GmacError::Fault { raw_status }) => {
                stage11_line!(out, "[starry-k3] stage=11 err=fbe raw={raw_status:#010x} stop");
                return Stage11Outcome::Fault { raw_status };
            }
            Err(GmacError::PhaseTimeout { phase }) => {
                stage11_line!(out, "[starry-k3] stage=11 err=deadline phase={phase:?} stop");
                return Stage11Outcome::Stopped { reason: "deadline" };
            }
            Err(GmacError::Cancelled { phase, after_publish }) => {
                stage11_line!(
                    out,
                    "[starry-k3] stage=11 err=cancelled phase={phase:?} after-publish={after_publish} stop"
                );
                return Stage11Outcome::Stopped { reason: "cancelled" };
            }
            Err(e) => {
                stage11_line!(out, "[starry-k3] stage=11 err={e:?} stop");
                return Stage11Outcome::Stopped { reason: "error" };
            }
        }
    }
    summary.window_elapsed_nanos = env.now_nanos() - start;
    stage11_line!(
        out,
        "[starry-k3] stage=11 result frames={} drops={} revocations={} printed={} arp-sample={} window-elapsed-ms={}",
        summary.frames,
        summary.drops,
        summary.revocations,
        summary.printed,
        if summary.arp_sample_printed { 1 } else { 0 },
        summary.window_elapsed_nanos / 1_000_000
    );
    Stage11Outcome::WindowEnded(summary)
}

/// Prints one raw frame sample: descriptor status word, length and (for the
/// selected ARP samples) the first bytes as hex. Raw content only — the
/// operator compares against the peer capture.
fn print_frame(
    out: &mut impl FnMut(&str),
    stage: u32,
    tag: &str,
    slot: usize,
    len: usize,
    raw_w3: u32,
    frame: &[u8],
) {
    let mut lb = aia::LineBuf::new();
    let _ = core::write!(
        lb,
        "[starry-k3] stage={stage} {tag} slot={} len={} raw={raw_w3:#010x} data=",
        slot,
        len
    );
    let cap = frame.len().min(60);
    for b in &frame[..cap] {
        let _ = core::write!(lb, "{b:02x}");
    }
    // 002 Cycle newline fix: sample lines carry the trailing \n too.
    let _ = lb.write_str("\n");
    out(lb.as_str());
}

// ---------------------------------------------------------------------------
// Stage 12: raw bidirectional frames and peer delivery (task 2.2)
// ---------------------------------------------------------------------------

/// Fixed ARP who-has submit count (task 2.2 contract).
pub const STAGE12_TX_FRAMES: usize = 20;
/// Interval between two submits.
pub const STAGE12_TX_INTERVAL_NANOS: u64 = 100_000_000;
/// Bounded RX window after the send phase.
pub const STAGE12_RX_WINDOW_NANOS: u64 = 10_000_000_000;
/// Idle poll wait between sweeps.
const STAGE12_POLL_STEP_NANOS: u64 = 1_000_000;

/// Counters of one completed stage 12 run (factual observations only). A
/// local terminal is never peer delivery: the reply sample and the
/// operator's capture are the peer evidence.
#[derive(Clone, Copy, Debug, Default)]
pub struct Stage12Summary {
    /// Submits actually published (OWN handed to the device).
    pub sent: usize,
    /// Local completions (OWN cleared, ES=0) reclaimed once.
    pub terminals: usize,
    /// Device-reported ES=1 local errors.
    pub tx_errors: usize,
    /// 1 s terminal windows that expired with OWN still set.
    pub tx_timeouts: usize,
    /// Submits that never published (Full for a whole packet window).
    pub not_sent: usize,
    /// RX frames copied during the stage (send phase + window).
    pub rx_frames: usize,
    /// Classified RX drops.
    pub rx_drops: usize,
    /// Whether an ARP reply (oper=2) sample was printed.
    pub arp_reply_printed: bool,
    /// Sample lines actually printed (frame/drop content samples).
    pub printed: usize,
    pub window_elapsed_nanos: u64,
}

#[derive(Clone, Copy, Debug)]
pub enum Stage12Outcome {
    /// Send phase and RX window ran to their bounds; the summary carries the
    /// counts.
    WindowEnded(Stage12Summary),
    /// A bounded stop (deadline/cancel) closed the stage early.
    Stopped { reason: &'static str },
    /// Gate refused or setup failed before/during initialization.
    SetupFailed { phase: &'static str },
    /// Fatal DMA bus error; the driver faulted and the stop ran.
    Fault { raw_status: u32 },
}

/// The standard stage 12 ARP request frame: who-has 192.168.137.1 tell
/// 192.168.137.2, software-built, 42 bytes zero-padded to 60 (the MAC
/// generates the FCS on transmit).
fn stage12_arp_request() -> [u8; 60] {
    let mut f = [0u8; 60];
    f[0..6].copy_from_slice(&[0xff; 6]); // dst: broadcast
    f[6..12].copy_from_slice(&[0x02, 0x00, 0x00, 0x00, 0x26, 0x01]); // src
    f[12] = 0x08;
    f[13] = 0x06; // ethertype ARP
    f[14] = 0x00;
    f[15] = 0x01; // htype: ethernet
    f[16] = 0x08;
    f[17] = 0x00; // ptype: IPv4
    f[18] = 6;
    f[19] = 4; // hlen/plen
    f[20] = 0;
    f[21] = 1; // oper: request (who-has)
    f[22..28].copy_from_slice(&[0x02, 0x00, 0x00, 0x00, 0x26, 0x01]); // sha = src
    f[28..32].copy_from_slice(&[192, 168, 137, 2]); // spa: tell
    // tha stays zero (unknown target)
    f[38..42].copy_from_slice(&[192, 168, 137, 1]); // tpa: who-has
    f
}

/// Ethernet/ARP reply shape check for sample selection only (not a
/// verdict): ARP ethertype and operation 2 (is-at reply).
fn is_arp_reply(frame: &[u8]) -> bool {
    frame.len() >= 42 && frame[12] == 0x08 && frame[13] == 0x06 && frame[20] == 0 && frame[21] == 2
}

/// RX counters shared by the send phase and the closing window.
struct RxCounters {
    frames: usize,
    drops: usize,
    printed: usize,
    reply_printed: bool,
}

/// One bounded RX poll step with sample printing (the send phase keeps
/// polling RX while waiting for TX terminals).
fn stage12_service_rx<E: GmacEnv, B: ArenaBacking>(
    driver: &mut Gmac<B>,
    env: &mut E,
    scratch: &mut [u8; STAGE11_SCRATCH_LEN],
    rx: &mut RxCounters,
    out: &mut impl FnMut(&str),
) -> Result<(), GmacError> {
    match driver.poll_rx(env, scratch) {
        Ok(RxPoll::Idle) => Ok(()),
        Ok(RxPoll::Frame { len, slot, raw_w3 }) => {
            rx.frames += 1;
            if is_arp_reply(&scratch[..len]) && !rx.reply_printed {
                rx.reply_printed = true;
                print_frame(out, 12, "arp-reply", slot, len, raw_w3, &scratch[..len]);
                rx.printed += 1;
            } else if rx.printed < STAGE11_SAMPLE_CAP {
                print_frame(out, 12, "frame", slot, len, raw_w3, &[]);
                rx.printed += 1;
            }
            Ok(())
        }
        Ok(RxPoll::Dropped { slot, reason, raw_w3 }) => {
            rx.drops += 1;
            if rx.printed < STAGE11_SAMPLE_CAP {
                stage12_line!(
                    out,
                    "[starry-k3] stage=12 drop slot={} reason={reason:?} raw={raw_w3:#010x}",
                    slot
                );
                rx.printed += 1;
            }
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Maps a driver error to the stage 12 outcome with its report line.
fn stage12_stop(out: &mut impl FnMut(&str), e: GmacError) -> Stage12Outcome {
    match e {
        GmacError::Fault { raw_status } => {
            stage12_line!(out, "[starry-k3] stage=12 err=fbe raw={raw_status:#010x} stop");
            Stage12Outcome::Fault { raw_status }
        }
        GmacError::PhaseTimeout { phase } => {
            stage12_line!(out, "[starry-k3] stage=12 err=deadline phase={phase:?} stop");
            Stage12Outcome::Stopped { reason: "deadline" }
        }
        GmacError::Cancelled { phase, after_publish } => {
            stage12_line!(
                out,
                "[starry-k3] stage=12 err=cancelled phase={phase:?} after-publish={after_publish} stop"
            );
            Stage12Outcome::Stopped { reason: "cancelled" }
        }
        e => {
            stage12_line!(out, "[starry-k3] stage=12 err={e:?} stop");
            Stage12Outcome::Stopped { reason: "error" }
        }
    }
}

/// Runs the whole stage 12 sequence: the stage 11 gate, a full bounded init
/// on a fresh second arena, TX enable, 20 software-built ARP who-has submits
/// (100 ms apart, each with an independent ≤1 s terminal, RX polled
/// throughout) and a bounded 10 s RX window for peer replies. The summary
/// reports local terminals and peer-reply evidence separately — a local
/// terminal never claims peer delivery. Failures end bounded and never
/// disturb device-owned state; the backing is consumed and retained on
/// every path after the publish point (the caller parks afterwards).
pub fn run_stage12<E: GmacEnv, B: ArenaBacking>(
    env: &mut E,
    st11: Stage11Outcome,
    arena: ArenaGuard<B>,
    out: &mut impl FnMut(&str),
) -> Stage12Outcome {
    // Gate: only a normally ended stage 11 window opens the TX stage; any
    // other outcome keeps the kernel parked with zero GMAC access.
    if !matches!(st11, Stage11Outcome::WindowEnded(_)) {
        stage12_line!(out, "[starry-k3] stage=12 err=gate stage11-not-window-ended stop");
        return Stage12Outcome::SetupFailed { phase: "gate" };
    }
    stage12_line!(
        out,
        "[starry-k3] stage=12 begin tx={} interval-ms={} terminal-ms={} rx-window-ms={}",
        STAGE12_TX_FRAMES,
        STAGE12_TX_INTERVAL_NANOS / 1_000_000,
        TX_TERMINAL_NANOS / 1_000_000,
        STAGE12_RX_WINDOW_NANOS / 1_000_000
    );

    // Fresh second arena, full bounded init (APMU RMW/SWR/MDIO are
    // idempotent; the stage 11 arena stays retained by its own guard).
    let mut driver = match Gmac::new(arena) {
        Ok(d) => d,
        Err(e) => {
            stage12_line!(out, "[starry-k3] stage=12 err=arena {e:?} stop");
            return Stage12Outcome::SetupFailed { phase: "arena" };
        }
    };
    match driver.init_rx(env) {
        Ok(link) => stage12_line!(out, "[starry-k3] stage=12 init {link:?} rx-running"),
        Err(e) => {
            stage12_line!(out, "[starry-k3] stage=12 err=init {e:?} stop");
            return Stage12Outcome::SetupFailed { phase: "init" };
        }
    }
    driver.enable_tx(env);
    stage12_line!(out, "[starry-k3] stage=12 tx-enabled");

    let frame = stage12_arp_request();
    let mut summary = Stage12Summary::default();
    let mut rx = RxCounters { frames: 0, drops: 0, printed: 0, reply_printed: false };
    let mut scratch = [0u8; STAGE11_SCRATCH_LEN];

    // Send phase: 20 submits, 100 ms apart, each with an independent ≤1 s
    // terminal wait (the driver reports the terminal outcome at its own
    // deadline); RX keeps being polled throughout.
    'packets: for i in 0..STAGE12_TX_FRAMES {
        let packet_deadline = env.now_nanos() + TX_TERMINAL_NANOS;
        let slot = loop {
            match driver.try_submit(env, &frame) {
                Ok(TxSubmit::Submitted { slot }) => break slot,
                Ok(TxSubmit::Full) => {
                    if env.now_nanos() >= packet_deadline {
                        summary.not_sent += 1;
                        stage12_line!(out, "[starry-k3] stage=12 tx i={i} full-window-expired");
                        continue 'packets;
                    }
                    let _ = driver.poll_tx(env);
                    if let Err(e) =
                        stage12_service_rx(&mut driver, env, &mut scratch, &mut rx, out)
                    {
                        return stage12_stop(out, e);
                    }
                    env.wait_nanos(STAGE12_POLL_STEP_NANOS);
                }
                Err(TxSubmitError::LengthInvalid { len }) => {
                    // The fixed ARP frame is always 60 bytes; a length
                    // refusal is a broken contract, never a truncated send.
                    stage12_line!(out, "[starry-k3] stage=12 err=tx-length len={len} stop");
                    return Stage12Outcome::Stopped { reason: "tx-length" };
                }
                Err(TxSubmitError::Driver(e)) => return stage12_stop(out, e),
            }
        };
        summary.sent += 1;
        stage12_line!(out, "[starry-k3] stage=12 tx i={i} slot={slot}");
        loop {
            match driver.poll_tx(env) {
                Ok(TxPoll::Completed { .. }) => {
                    summary.terminals += 1;
                    break;
                }
                Ok(TxPoll::LocalError { slot, raw_w3 }) => {
                    summary.tx_errors += 1;
                    stage12_line!(
                        out,
                        "[starry-k3] stage=12 tx i={i} err slot={slot} raw={raw_w3:#010x}"
                    );
                    break;
                }
                Ok(TxPoll::TerminalTimeout { slot, raw_w3 }) => {
                    summary.tx_timeouts += 1;
                    stage12_line!(
                        out,
                        "[starry-k3] stage=12 tx i={i} timeout slot={slot} raw={raw_w3:#010x}"
                    );
                    break;
                }
                Ok(TxPoll::Pending { .. }) | Ok(TxPoll::Idle) => {
                    if let Err(e) =
                        stage12_service_rx(&mut driver, env, &mut scratch, &mut rx, out)
                    {
                        return stage12_stop(out, e);
                    }
                    env.wait_nanos(STAGE12_POLL_STEP_NANOS);
                }
                Err(e) => return stage12_stop(out, e),
            }
        }
        env.wait_nanos(STAGE12_TX_INTERVAL_NANOS);
    }

    // Bounded RX window: keep polling for peer replies.
    let window_start = env.now_nanos();
    let window_deadline = window_start + STAGE12_RX_WINDOW_NANOS;
    while env.now_nanos() < window_deadline {
        if let Err(e) = stage12_service_rx(&mut driver, env, &mut scratch, &mut rx, out) {
            return stage12_stop(out, e);
        }
        env.wait_nanos(STAGE12_POLL_STEP_NANOS);
    }
    summary.rx_frames = rx.frames;
    summary.rx_drops = rx.drops;
    summary.arp_reply_printed = rx.reply_printed;
    summary.printed = rx.printed;
    summary.window_elapsed_nanos = env.now_nanos() - window_start;
    stage12_line!(
        out,
        "[starry-k3] stage=12 result sent={} terminals={} tx-errors={} tx-timeouts={} not-sent={} rx-frames={} rx-drops={} arp-reply={} window-elapsed-ms={}",
        summary.sent,
        summary.terminals,
        summary.tx_errors,
        summary.tx_timeouts,
        summary.not_sent,
        summary.rx_frames,
        summary.rx_drops,
        if summary.arp_reply_printed { 1 } else { 0 },
        summary.window_elapsed_nanos / 1_000_000
    );
    Stage12Outcome::WindowEnded(summary)
}

// ---------------------------------------------------------------------------
// Stage 13: ARP/ICMP/UDP/TCP polling exchange (task 3.2, MS11 Iteration 002)
// ---------------------------------------------------------------------------

/// Fixed session window (task 3.2 contract: 60 s on the board).
#[cfg(feature = "k3-polling-net")]
pub const STAGE13_WINDOW_NANOS: u64 = 60_000_000_000;
/// Idle poll wait between session services.
#[cfg(feature = "k3-polling-net")]
const STAGE13_POLL_STEP_NANOS: u64 = 1_000_000;

/// Counters of one completed stage 13 session window (factual observations
/// only — the operator adjudicates the PC-side native tool output).
#[cfg(feature = "k3-polling-net")]
#[derive(Clone, Copy, Debug)]
pub struct Stage13Summary {
    pub counters: polling_net::SessionCounters,
    pub window_elapsed_nanos: u64,
}

#[cfg(feature = "k3-polling-net")]
#[derive(Clone, Copy, Debug)]
pub enum Stage13Outcome {
    /// The fixed window ran to its end; the summary carries the counters.
    WindowEnded(Stage13Summary),
    /// A bounded stop (op timeout, cancel or session fault) closed the
    /// stage early.
    Stopped { reason: &'static str },
    /// Gate refused or setup failed before/during initialization.
    SetupFailed { phase: &'static str },
    /// Fatal DMA bus error; the driver faulted and the stop ran.
    Fault { raw_status: u32 },
}

/// Maps a recorded session fault to the stage 13 outcome; the fault line
/// always names the failing stage and its owner.
#[cfg(feature = "k3-polling-net")]
fn stage13_fault_outcome(
    out: &mut impl FnMut(&str),
    fault: polling_net::SessionFault,
) -> Stage13Outcome {
    use polling_net::{SessionFaultDetail, SessionFaultStage};
    let owner = match fault.stage {
        SessionFaultStage::DeviceRx => "device-rx",
        SessionFaultStage::DeviceTxReclaim => "device-tx-reclaim",
        SessionFaultStage::DeviceTxSubmit => "device-tx-submit",
    };
    match fault.detail {
        SessionFaultDetail::Driver(GmacError::Fault { raw_status }) => {
            stage13_line!(
                out,
                "[starry-k3] stage=13 err=fbe raw={raw_status:#010x} owner={owner} stop"
            );
            Stage13Outcome::Fault { raw_status }
        }
        detail => {
            stage13_line!(
                out,
                "[starry-k3] stage=13 err=fault owner={owner} detail={detail:?} stop"
            );
            Stage13Outcome::Stopped { reason: "session-fault" }
        }
    }
}

/// Runs the whole stage 13 sequence on the third arena: the stage 12 gate,
/// a full bounded init, TX enable, then a fixed 60 s polling session window
/// (smoltcp Interface + UDP/TCP echo sockets over static storage; ICMP echo
/// replies are generated by the stack). Every report goes through `out`;
/// the kernel never judges PASS — the operator adjudicates the PC-side
/// native tool output against this wire. Failures end bounded and name the
/// failing stage and owner; the backing is consumed and retained on every
/// path after the publish point (the caller parks afterwards).
#[cfg(feature = "k3-polling-net")]
pub fn run_stage13<'s, E: Stage11Env, B: ArenaBacking>(
    env: E,
    st12: Stage12Outcome,
    arena: ArenaGuard<B>,
    storage: &'s mut polling_net::SessionStorage<'s>,
    out: &mut impl FnMut(&str),
) -> Stage13Outcome {
    use polling_net::{K3PollDevice, K3Session, SessionError};

    // Gate: only a normally ended stage 12 window opens the protocol stage;
    // every other outcome keeps the kernel parked with zero GMAC access.
    if !matches!(st12, Stage12Outcome::WindowEnded(_)) {
        stage13_line!(out, "[starry-k3] stage=13 err=gate stage12-not-window-ended stop");
        return Stage13Outcome::SetupFailed { phase: "gate" };
    }
    stage13_line!(
        out,
        "[starry-k3] stage=13 begin window-ms={} udp-port={} tcp-port={}",
        STAGE13_WINDOW_NANOS / 1_000_000,
        polling_net::UDP_ECHO_PORT,
        polling_net::TCP_ECHO_PORT
    );

    // Fresh third arena, full bounded init (the stage 11/12 arenas stay
    // retained by their own published guards).
    let mut env = env;
    let mut driver = match Gmac::new(arena) {
        Ok(d) => d,
        Err(e) => {
            stage13_line!(out, "[starry-k3] stage=13 err=arena {e:?} stop");
            return Stage13Outcome::SetupFailed { phase: "arena" };
        }
    };
    match driver.init_rx(&mut env) {
        Ok(link) => stage13_line!(out, "[starry-k3] stage=13 init {link:?} rx-running"),
        Err(e) => {
            stage13_line!(out, "[starry-k3] stage=13 err=init {e:?} stop");
            return Stage13Outcome::SetupFailed { phase: "init" };
        }
    }
    driver.enable_tx(&mut env);
    stage13_line!(out, "[starry-k3] stage=13 tx-enabled");

    let device = K3PollDevice::new(driver, env);
    let mut session = K3Session::new(device, storage);

    // Fixed session window: continuous polling (RX ingress, stack egress,
    // TX submit and terminal reclaim) with bounded echo services.
    let start = session.now_nanos();
    let deadline = start + STAGE13_WINDOW_NANOS;
    while session.now_nanos() < deadline {
        session.poll_stack();
        session.service_udp_echo();
        match session.service_tcp_echo_and_close() {
            Ok(()) => {}
            Err(SessionError::Fault(fault)) => return stage13_fault_outcome(out, fault),
            Err(SessionError::TcpTimeout { stage }) => {
                stage13_line!(
                    out,
                    "[starry-k3] stage=13 err=op-timeout phase={stage} owner=session stop"
                );
                return Stage13Outcome::Stopped { reason: "tcp-timeout" };
            }
        }
        if let Some(fault) = session.take_fault() {
            return stage13_fault_outcome(out, fault);
        }
        session.wait_nanos(STAGE13_POLL_STEP_NANOS);
    }
    let counters = session.counters();
    let window_elapsed_nanos = session.now_nanos() - start;
    stage13_line!(
        out,
        "[starry-k3] stage=13 result icmp-replies={} udp-echo={} udp-over-limit={} tcp-echo={} tcp-close={:?} rx-frames={} rx-drops={} tx-terminals={} tx-errors={} tx-timeouts={} window-elapsed-ms={}",
        counters.icmp_echo_replies,
        counters.udp_echoed,
        counters.udp_over_limit,
        counters.tcp_echoed,
        counters.tcp_state,
        counters.rx_frames,
        counters.rx_drops,
        counters.tx_completed,
        counters.tx_errors,
        counters.tx_timeouts,
        window_elapsed_nanos / 1_000_000
    );
    Stage13Outcome::WindowEnded(Stage13Summary { counters, window_elapsed_nanos })
}

// ---------------------------------------------------------------------------
// Board glue (riscv64 K3 builds only)
// ---------------------------------------------------------------------------

#[cfg(all(feature = "k3", target_arch = "riscv64"))]
mod board {
    use super::*;
    use crate::platform::k3::K3Env;
    use crate::platform::phy_probe::ProbeEnv as _;
    use axalloc::GlobalPage;
    use axhal::mem::virt_to_phys;
    use axplat_riscv64_k3::dma;

    /// Kernel-side arena backing: one contiguous zeroed `GlobalPage` span.
    /// `Drop` would return the pages to the allocator, but the guard retains
    /// them after the publish point — device-owned storage is never
    /// released until external reset.
    pub struct BoardBacking(GlobalPage);

    impl ArenaBacking for BoardBacking {
        fn base_va(&self) -> usize {
            self.0.start_vaddr().as_usize()
        }

        fn base_pa(&self) -> u64 {
            self.0.start_paddr(virt_to_phys).as_usize() as u64
        }
    }

    impl GmacEnv for K3Env {
        fn reg_read(&mut self, offset: u32) -> u32 {
            self.mmio_read(crate::platform::mac_probe::MAC_BASE_PADDR + offset as u64)
        }

        fn reg_write(&mut self, offset: u32, value: u32) {
            self.mmio_write(crate::platform::mac_probe::MAC_BASE_PADDR + offset as u64, value)
        }

        fn cache_flush(&mut self, va: usize, len: usize) {
            dma::cbo::flush_range(va, len)
        }

        fn cache_clean(&mut self, va: usize, len: usize) {
            dma::cbo::clean_range(va, len)
        }

        fn cache_invalidate(&mut self, va: usize, len: usize) {
            dma::cbo::inval_range(va, len)
        }

        fn fence_rw(&mut self) {
            dma::cbo::fence_rw()
        }

        fn fence_iorw(&mut self) {
            dma::cbo::fence_iorw()
        }
    }

    impl Stage11Env for K3Env {
        fn mask_source(&mut self, source: usize) {
            axhal::irq::set_enable(source, false);
        }
    }

    /// K3 board entry: allocates the exclusive arena and runs stage 11 on
    /// the RAM boot path. The caller parks afterwards regardless of the
    /// outcome; the backing is held until external reset.
    pub fn run_k3_stage11(st10: irq_probe::Stage10Outcome, mut out: impl FnMut(&str)) -> Stage11Outcome {
        let mut page = match GlobalPage::alloc_contiguous(dma::ARENA_PAGES, dma::PAGE_SIZE_BYTES)
        {
            Ok(p) => p,
            Err(e) => {
                out("[starry-k3] stage=11 err=alloc stop");
                let _ = e;
                return Stage11Outcome::SetupFailed { phase: "alloc" };
            }
        };
        page.zero();
        let mut env = K3Env;
        run_stage11(&mut env, st10, ArenaGuard::new(BoardBacking(page)), &mut |line| out(line))
    }

    /// K3 board entry for stage 12: allocates the SECOND exclusive arena and
    /// runs the raw TX sequence after a normally ended stage 11. The stage 11
    /// arena stays retained by its own published guard; the caller parks
    /// afterwards regardless of the outcome.
    pub fn run_k3_stage12(st11: Stage11Outcome, mut out: impl FnMut(&str)) -> Stage12Outcome {
        let mut page = match GlobalPage::alloc_contiguous(dma::ARENA_PAGES, dma::PAGE_SIZE_BYTES)
        {
            Ok(p) => p,
            Err(e) => {
                out("[starry-k3] stage=12 err=alloc stop");
                let _ = e;
                return Stage12Outcome::SetupFailed { phase: "alloc" };
            }
        };
        page.zero();
        let mut env = K3Env;
        run_stage12(&mut env, st11, ArenaGuard::new(BoardBacking(page)), &mut |line| out(line))
    }

    /// K3 board entry for stage 13: allocates the THIRD exclusive arena and
    /// runs the protocol session after a normally ended stage 12. The stage
    /// 11/12 arenas stay retained by their own published guards; the static
    /// session storage lives on the boot stack (256 KiB); the caller parks
    /// afterwards regardless of the outcome.
    pub fn run_k3_stage13(st12: Stage12Outcome, mut out: impl FnMut(&str)) -> Stage13Outcome {
        let mut page = match GlobalPage::alloc_contiguous(dma::ARENA_PAGES, dma::PAGE_SIZE_BYTES)
        {
            Ok(p) => p,
            Err(e) => {
                out("[starry-k3] stage=13 err=alloc stop");
                let _ = e;
                return Stage13Outcome::SetupFailed { phase: "alloc" };
            }
        };
        page.zero();
        let mut storage = polling_net::SessionStorage::new();
        let env = K3Env;
        run_stage13(env, st12, ArenaGuard::new(BoardBacking(page)), &mut storage, &mut |line| {
            out(line)
        })
    }
}

#[cfg(all(feature = "k3", target_arch = "riscv64"))]
pub use board::run_k3_stage11;
#[cfg(all(feature = "k3", target_arch = "riscv64"))]
pub use board::run_k3_stage12;
#[cfg(all(feature = "k3", target_arch = "riscv64"))]
pub use board::run_k3_stage13;
