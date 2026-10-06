//! K3 stage 9 AIA software-injection probe pure-logic host test harness
//! (MS10 Iteration 000, task 1.3).
//!
//! Uses `#[path]` to include the real `kernel/src/platform/aia_probe.rs` and
//! the production AIA core. `FakeEnv` below is a deterministic fake board
//! (APLIC register bank with MSI delivery, IMSIC selector/data file, event
//! queue, modeled SEXT/SIE), so every test drives the actual probe stage
//! logic against the actual chip core — never a copy. Runs via
//! `rustc --edition=2024 --test` → /tmp binary.
//!
//! Fake hardware semantics (spec-conservative, R69-corrected):
//! - SETIPNUM_LE delivers an MSI only when the source is enabled and
//!   domaincfg.IE is set; GENMSI delivers regardless of the source gate (as
//!   on hardware), so it must never be used to prove a source mask.
//! - `stopei` claims return the lowest pending AND enabled EID; masked
//!   pendings are retained unclaimed until the EIE bit returns.
//! - Traps are taken only while SEXT is set; `pump()` services at most one
//!   interrupt per call (one trap).
#![allow(dead_code)]

#[path = "../crates/axplat-riscv64-k3/src/aia.rs"]
mod k3_aia;

#[path = "../kernel/src/platform/aia_probe.rs"]
mod aia_probe;

use aia_probe::{
    handler_event_count, handler_retrap_value, run_stage9, take_handler_retrap_request,
    Observed, Stage9Env, Stage9Outcome, STAGE9_CLAIM_BUDGET,
};
use k3_aia::{
    eie_bit, eie_word, AiaBackend, AiaChip, HandlerStore, ImsicAccess, APLIC_DOMAINCFG,
    APLIC_DOMAINCFG_DM, APLIC_DOMAINCFG_IE, APLIC_GENMSI, APLIC_SETIPNUM_LE, APLIC_TARGET_BASE,
};
use k3_aia as aia;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

const IMSIC_WORDS: usize = 8;

static EIP: [AtomicU64; IMSIC_WORDS] = [const { AtomicU64::new(0) }; IMSIC_WORDS];
static EIE: [AtomicU64; IMSIC_WORDS] = [const { AtomicU64::new(0) }; IMSIC_WORDS];
static EIDELIVERY: AtomicU64 = AtomicU64::new(1);
static EITHRESHOLD: AtomicU64 = AtomicU64::new(0);

fn reset_fake_imsic() {
    for w in 0..IMSIC_WORDS {
        EIP[w].store(0, Ordering::SeqCst);
        EIE[w].store(0, Ordering::SeqCst);
    }
    EIDELIVERY.store(1, Ordering::SeqCst);
    EITHRESHOLD.store(0, Ordering::SeqCst);
}

fn pending_set(eid: usize) -> bool {
    EIP[eie_word(eid)].load(Ordering::SeqCst) & eie_bit(eid) != 0
}

fn set_pending(eid: usize) {
    EIP[eie_word(eid)].fetch_or(eie_bit(eid), Ordering::SeqCst);
}

fn enabled_bit(eid: usize) -> bool {
    EIE[eie_word(eid)].load(Ordering::SeqCst) & eie_bit(eid) != 0
}

fn any_pending_enabled() -> bool {
    for eid in 1..512 {
        if pending_set(eid) && enabled_bit(eid) {
            return true;
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Handler store (models the production lock-free table semantics)
// ---------------------------------------------------------------------------

struct FakeStore {
    entries: [AtomicUsize; 512],
}

impl FakeStore {
    const fn new() -> Self {
        FakeStore {
            entries: [const { AtomicUsize::new(0) }; 512],
        }
    }
}

static STORE: FakeStore = FakeStore::new();

impl HandlerStore for FakeStore {
    fn register(&self, source: usize, handler: fn()) -> bool {
        if source == 0 || source >= 512 {
            return false;
        }
        self.entries[source]
            .compare_exchange(0, handler as usize, Ordering::AcqRel, Ordering::Relaxed)
            .is_ok()
    }

    fn unregister(&self, source: usize) -> Option<fn()> {
        if source == 0 || source >= 512 {
            return None;
        }
        let h = self.entries[source].swap(0, Ordering::AcqRel);
        if h != 0 {
            Some(unsafe { core::mem::transmute::<usize, fn()>(h) })
        } else {
            None
        }
    }

    fn call(&self, eid: usize) -> bool {
        if eid == 0 || eid >= 512 {
            return false;
        }
        let h = self.entries[eid].load(Ordering::Acquire);
        if h != 0 {
            let f: fn() = unsafe { core::mem::transmute(h) };
            f();
            true
        } else {
            false
        }
    }
}

static CHIP: AiaChip<FakeStore> = AiaChip::new(&STORE);

// ---------------------------------------------------------------------------
// Fake stage 9 environment
// ---------------------------------------------------------------------------

struct FakeEnv {
    aplic: std::collections::BTreeMap<u32, u32>,
    now_ns: u64,
    /// Delivery delay after a SETIPNUM_LE / GENMSI write (default 0).
    setip_delay_ns: u64,
    genmsi_delay_ns: u64,
    /// (trigger index from 1, delay from that trigger) of an intruder
    /// delivery queued regardless of the mask state.
    extra_pending_at: Option<(usize, u64)>,
    triggers: usize,
    /// (deliver_at, eid)
    queue: Vec<(u64, usize)>,
    busy_until_ns: u64,
    sie: bool,
    sext: bool,
    in_critical: bool,
}

impl FakeEnv {
    /// Post-init state: SEXT open, delivery on, threshold 0, EIE cleared,
    /// domaincfg IE|DM (the state `init_global`/`init_percpu` leave behind).
    fn new() -> Self {
        reset_fake_imsic();
        for entry in STORE.entries.iter() {
            entry.store(0, Ordering::SeqCst);
        }
        let mut env = FakeEnv {
            aplic: std::collections::BTreeMap::new(),
            now_ns: 0,
            setip_delay_ns: 0,
            genmsi_delay_ns: 0,
            extra_pending_at: None,
            triggers: 0,
            queue: Vec::new(),
            busy_until_ns: 0,
            sie: true,
            sext: true,
            in_critical: false,
        };
        // Post-init domain state: MSI mode, domain IE on (init_global).
        env.aplic.insert(APLIC_DOMAINCFG, APLIC_DOMAINCFG_IE | APLIC_DOMAINCFG_DM);
        env
    }

    fn domain_ie(&self) -> bool {
        self.aplic.get(&APLIC_DOMAINCFG).copied().unwrap_or(0) & APLIC_DOMAINCFG_IE != 0
    }

    fn source_enabled(&self, source: usize) -> bool {
        let setie_word = self
            .aplic
            .get(&(aia::setie_word_off(source / 32)))
            .copied()
            .unwrap_or(0);
        setie_word & (1 << (source % 32)) != 0
    }

    fn target_eid(&self, source: usize) -> usize {
        let t = self
            .aplic
            .get(&(APLIC_TARGET_BASE + 4 * (source as u32 - 1)))
            .copied()
            .unwrap_or(0);
        (t & 0x7ff) as usize
    }

    fn queue_msi(&mut self, eid: usize, delay_ns: u64) {
        self.queue.push((self.now_ns + delay_ns, eid));
    }

    fn genmsi_write(&mut self, value: u32) {
        let eid = (value & 0x7ff) as usize;
        self.triggers += 1;
        let extra = match self.extra_pending_at {
            Some((n, delay)) if self.triggers == n => Some(delay),
            _ => None,
        };
        if extra.is_some() {
            // Intruder event: lands regardless of the mask state.
            self.queue_msi(eid, extra.unwrap());
        }
        self.busy_until_ns = self.now_ns + 0;
        self.queue_msi(eid, self.genmsi_delay_ns);
    }

    fn deliver_due(&mut self) {
        let now = self.now_ns;
        let due: Vec<usize> = self
            .queue
            .iter()
            .filter(|(at, _)| *at <= now)
            .map(|(_, eid)| *eid)
            .collect();
        self.queue.retain(|(at, _)| *at > now);
        for eid in due {
            set_pending(eid);
        }
    }
}

struct ImsicFile;

impl ImsicAccess for ImsicFile {
    fn read(&mut self, selector: u16) -> u64 {
        match selector {
            0x70 => EIDELIVERY.load(Ordering::SeqCst),
            0x72 => EITHRESHOLD.load(Ordering::SeqCst),
            s if (0xc0..0xc0 + 2 * IMSIC_WORDS as u16).contains(&s) && s % 2 == 0 => {
                EIE[((s - 0xc0) / 2) as usize].load(Ordering::SeqCst)
            }
            s if (0x80..0x80 + 2 * IMSIC_WORDS as u16).contains(&s) && s % 2 == 0 => {
                EIP[((s - 0x80) / 2) as usize].load(Ordering::SeqCst)
            }
            _ => 0,
        }
    }

    fn write(&mut self, selector: u16, value: u64) {
        match selector {
            0x70 => EIDELIVERY.store(value, Ordering::SeqCst),
            0x72 => EITHRESHOLD.store(value, Ordering::SeqCst),
            s if (0xc0..0xc0 + 2 * IMSIC_WORDS as u16).contains(&s) && s % 2 == 0 => {
                EIE[((s - 0xc0) / 2) as usize].store(value, Ordering::SeqCst)
            }
            // EIP registers are read-only (WARL model): writes ignored.
            _ => {}
        }
    }

    fn claim(&mut self) -> u64 {
        for eid in 1..512 {
            if pending_set(eid) && enabled_bit(eid) {
                EIP[eie_word(eid)].fetch_and(!eie_bit(eid), Ordering::SeqCst);
                return ((eid as u64) << 16) | eid as u64;
            }
        }
        0
    }
}

impl AiaBackend for FakeEnv {
    fn aplic_read(&mut self, offset: u32) -> u32 {
        if offset == APLIC_GENMSI && self.now_ns < self.busy_until_ns {
            return aia::APLIC_GENMSI_BUSY;
        }
        self.aplic.get(&offset).copied().unwrap_or(0)
    }

    fn aplic_write(&mut self, offset: u32, value: u32) {
        if offset == APLIC_SETIPNUM_LE {
            let source = value as usize & 0x3ff;
            self.triggers += 1;
            let extra = match self.extra_pending_at {
                Some((n, delay)) if self.triggers == n => Some(delay),
                _ => None,
            };
            if extra.is_some() {
                self.queue_msi(self.target_eid(source).max(1), extra.unwrap());
            }
            if self.source_enabled(source) && self.domain_ie() {
                let eid = self.target_eid(source);
                self.queue_msi(if eid == 0 { source } else { eid }, self.setip_delay_ns);
            }
            self.aplic.insert(offset, value);
            return;
        }
        if offset == APLIC_GENMSI {
            self.genmsi_write(value);
            return;
        }
        self.aplic.insert(offset, value);
    }

    fn with_imsic<R>(&mut self, f: impl FnOnce(&mut dyn ImsicAccess) -> R) -> R {
        assert!(!self.in_critical, "nested IMSIC critical section");
        self.in_critical = true;
        let saved = self.sie;
        self.sie = false;
        let r = f(&mut ImsicFile);
        if saved {
            self.sie = true;
        }
        self.in_critical = false;
        r
    }

    fn set_sext(&mut self, on: bool) {
        self.sext = on;
    }

    fn hart(&self) -> usize {
        3
    }
}

impl Stage9Env for FakeEnv {
    fn now_nanos(&self) -> u64 {
        self.now_ns
    }

    fn wait_nanos(&mut self, ns: u64) {
        self.now_ns += ns;
    }

    fn pump(&mut self) {
        self.deliver_due();
        if take_handler_retrap_request() {
            self.genmsi_write(handler_retrap_value());
            self.deliver_due();
        }
        if self.sext && any_pending_enabled() {
            CHIP.handle_external(self);
        }
    }

    fn register_handler(&mut self, source: usize, handler: fn()) -> bool {
        // Full production route registration: handler store + hardware
        // enable (sourcecfg/target/pending-clear/EIE/SETIE).
        CHIP.register(self, source, handler)
    }

    fn set_route_enabled(&mut self, source: usize, enabled: bool) {
        CHIP.set_enable(self, source, enabled);
    }

    fn observed(&self) -> Observed {
        Observed {
            last_raw_claim: CHIP.last_raw_claim(),
            spurious: CHIP.spurious_count(),
            unknown: CHIP.unknown_count(),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests (task 1.3 witness list)
// ---------------------------------------------------------------------------

/// Serializes tests: they share the fake IMSIC/handler statics and libtest
/// runs them on parallel threads by default.
fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn run_and_pass(lines: &mut Vec<String>) -> aia_probe::Stage9Summary {
    let mut env = FakeEnv::new();
    let outcome = run_stage9(&mut env, |line| lines.push(line.to_string()));
    assert_pass(outcome)
}

fn assert_pass(outcome: Stage9Outcome) -> aia_probe::Stage9Summary {
    match outcome {
        Stage9Outcome::Passed(s) => s,
        Stage9Outcome::Failed { phase, reason } => {
            panic!("stage9 failed at {phase}: {reason}")
        }
    }
}

#[test]
fn setip_and_genmsi_each_deliver_three() {
    let _guard = test_guard();
    let mut lines = Vec::new();
    let s = run_and_pass(&mut lines);
    assert_eq!(s.setip_events, 3, "lines: {lines:?}");
    assert_eq!(s.genmsi_events, 3);
    // Each entry point delivered through its own claims with raw evidence.
    assert_ne!(s.last_setip_raw, 0);
    assert_ne!(s.last_genmsi_raw, 0);
}

#[test]
fn aplic_mask_blocks_setip() {
    let _guard = test_guard();
    let mut lines = Vec::new();
    let s = run_and_pass(&mut lines);
    assert_eq!(s.aplic_mask_events, 0, "APLIC mask leaked an event");
    assert!(s.aplic_fresh, "fresh trigger after re-enable did not arrive");
    assert_eq!(s.aplic_retained, 0, "APLIC pending must be clearable before re-enable");
}

#[test]
fn eie_mask_blocks_genmsi() {
    let _guard = test_guard();
    let mut lines = Vec::new();
    let s = run_and_pass(&mut lines);
    assert_eq!(s.eie_mask_events, 0, "EIE mask leaked an event");
    assert!(s.eie_fresh, "fresh trigger after re-enable did not arrive");
}

#[test]
fn reenable_requires_fresh_trigger() {
    let _guard = test_guard();
    let mut lines = Vec::new();
    let s = run_and_pass(&mut lines);
    // The masked GENMSI pending stays retained (claim is enable-gated) and is
    // serviced right after re-enable; the fresh trigger is a further event.
    assert_eq!(s.eie_retained, 1, "retained pending not serviced on re-enable");
    assert!(s.eie_fresh, "fresh trigger did not arrive after the retained one");
}

#[test]
fn handler_generated_msi_retraps() {
    let _guard = test_guard();
    let mut lines = Vec::new();
    let s = run_and_pass(&mut lines);
    assert_eq!(s.retrap_events, 2, "handler-generated MSI must yield two independent claims");
}

#[test]
fn unknown_eid_is_reported_without_handler() {
    let _guard = test_guard();
    let mut lines = Vec::new();
    let s = run_and_pass(&mut lines);
    assert_eq!(s.unknown_delta, 1, "EID 2 must be claimed and diagnosed once");
}

#[test]
fn timeout_stops_next_stage() {
    let _guard = test_guard();
    let mut env = FakeEnv::new();
    env.genmsi_delay_ns = 2_000_000_000; // GENMSI lands after the 1 s deadline
    let outcome = run_stage9(&mut env, |_| {});
    match outcome {
        Stage9Outcome::Failed { phase, reason } => {
            assert_eq!(phase, "genmsi-x3");
            assert_eq!(reason, "deadline");
        }
        Stage9Outcome::Passed(_) => panic!("timeout must not pass"),
    }
}

#[test]
fn quiet_claim_is_failure() {
    let _guard = test_guard();
    let mut env = FakeEnv::new();
    // A third-trigger intruder lands 500 ms later, inside the post-phase
    // quiet window (the three setip events themselves stay on time).
    env.extra_pending_at = Some((3, 500_000_000));
    let outcome = run_stage9(&mut env, |_| {});
    match outcome {
        Stage9Outcome::Failed { phase, reason } => {
            assert_eq!(phase, "setip-x3");
            assert_eq!(reason, "quiet-claim");
        }
        Stage9Outcome::Passed(_) => panic!("a claim inside the quiet window must fail"),
    }
}

#[test]
fn storm_budget_masks_sext() {
    let _guard = test_guard();
    let mut lines = Vec::new();
    let mut env = FakeEnv::new();
    let outcome = run_stage9(&mut env, |line| lines.push(line.to_string()));
    let s = assert_pass(outcome);
    assert_eq!(s.storm_events, STAGE9_CLAIM_BUDGET, "budget not enforced");
    assert!(!env.sext, "storm must end with SEXT masked");
    assert!(s.storm_quiet, "masked storm window was not quiet");
}

#[test]
fn genmsi_bypass_does_not_break_the_aplic_mask_evidence() {
    let _guard = test_guard();
    // Sanity on the fake itself: a GENMSI write must deliver even while the
    // source is masked (why the EIE phase uses GENMSI and the APLIC phase
    // uses SETIPNUM), and the production constants must agree.
    let mut env = FakeEnv::new();
    env.aplic.insert(APLIC_DOMAINCFG, APLIC_DOMAINCFG_IE | APLIC_DOMAINCFG_DM);
    env.set_route_enabled(1, false);
    let base = handler_event_count();
    env.aplic_write(APLIC_GENMSI, (3 << 18) | 1);
    for _ in 0..8 {
        env.pump();
    }
    assert_eq!(handler_event_count(), base, "no handler registered yet");
    assert!(pending_set(1), "GENMSI must bypass the APLIC source gate");
}

// ---------------------------------------------------------------------------
// 002 Cycle newline fix witness (MS11 Iteration 002, task 3.2)
// ---------------------------------------------------------------------------

#[test]
fn stage9_wire_lines_carry_trailing_newline() {
    let _guard = test_guard();
    let mut lines = Vec::new();
    let _summary = run_and_pass(&mut lines);
    assert!(!lines.is_empty(), "the stage 9 run prints wire lines");
    for l in &lines {
        assert!(l.ends_with('\n'), "wire line missing trailing newline: {l:?}");
    }
}
