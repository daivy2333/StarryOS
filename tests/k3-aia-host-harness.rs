//! K3 AIA core pure-logic host test harness (MS10 Iteration 000, tasks 1.1–1.2).
//!
//! Uses `#[path]` to include the real production module
//! `crates/axplat-riscv64-k3/src/aia.rs` without compiling the platform
//! crate. All register/CSR access goes through the core's `AiaBackend` seam;
//! `FakeBackend` below is a deterministic fake (APLIC register bank, IMSIC
//! selector/data file, modeled `sstatus.SIE`), so every test drives the actual
//! production core — never a copy of it. Runs via `rustc --edition=2024
//! --test` → /tmp binary (same convention as the MS09 probe harnesses).
//!
//! Fake hardware semantics (from the R69-corrected contract):
//! - `stopei` claim: returns the highest-priority (lowest EID) pending AND
//!   enabled interrupt; the atomic swap clears exactly that pending.
//!   Raw 0 = none. Identity serves as priority (AIA interrupt files).
//! - EIP registers are read-only (pending clears only via claim).
//! - The modeled SIE is saved on critical-section entry and restored exactly
//!   as found (never unconditionally re-enabled).
#![allow(dead_code)]

#[path = "../crates/axplat-riscv64-k3/src/aia.rs"]
mod aia;

use aia::{
    eie_bit, eie_selector, eie_word, eip_selector, init_global, init_percpu, parse_stopei,
    route_for, set_eid_enabled, set_source_gate, AiaBackend, AiaChip, HandlerStore, ImsicAccess,
    APLIC_CLRIE_BASE, APLIC_CLRIP_BASE, APLIC_DOMAINCFG, APLIC_DOMAINCFG_IE, APLIC_SETIE_BASE,
    APLIC_SMSICFGADDR, APLIC_SMSICFGADDRH, APLIC_SOURCECFG_BASE, APLIC_TARGET_BASE, SM_DETACHED,
    SM_LEVEL_HIGH,
};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

// ---------------------------------------------------------------------------
// Shared fake IMSIC state (statics so `fn()` handlers can reach it)
// ---------------------------------------------------------------------------

const IMSIC_WORDS: usize = 8;

static EIP: [AtomicU64; IMSIC_WORDS] = [const { AtomicU64::new(0) }; IMSIC_WORDS];
static EIE: [AtomicU64; IMSIC_WORDS] = [const { AtomicU64::new(0) }; IMSIC_WORDS];
/// Second independent local file (MS14 Iteration 000): models the
/// secondary/remote hart's own IMSIC file, fully distinct from the boot
/// file statics above. `FakeBackend::remote_file` selects which file the
/// production core's `with_imsic` touches.
static EIP_REMOTE: Mutex<[u64; IMSIC_WORDS]> = Mutex::new([0; IMSIC_WORDS]);
static EIE_REMOTE: Mutex<[u64; IMSIC_WORDS]> = Mutex::new([0; IMSIC_WORDS]);
static EIDELIVERY: AtomicU64 = AtomicU64::new(0);
static EITHRESHOLD: AtomicU64 = AtomicU64::new(0);
/// Monotonic operation tick across APLIC and IMSIC accesses (ordering witness).
static TICK: AtomicUsize = AtomicUsize::new(0);
/// (op-name, tick) sequence across every fake hardware access.
static OP_SEQ: Mutex<Vec<(&'static str, usize)>> = Mutex::new(Vec::new());
/// Raw claim values returned by the fake `stopei` (most recent last).
static CLAIM_LOG: Mutex<Vec<u64>> = Mutex::new(Vec::new());
/// EID the next handler invocation injects as a fresh pending (retrap model).
static INJECT_EID: AtomicUsize = AtomicUsize::new(0);
static CALLS_1: AtomicUsize = AtomicUsize::new(0);
static CALLS_60: AtomicUsize = AtomicUsize::new(0);
static CALLS_133: AtomicUsize = AtomicUsize::new(0);
static CALLS_277: AtomicUsize = AtomicUsize::new(0);

fn reset_fake_imsic() {
    for w in 0..IMSIC_WORDS {
        EIP[w].store(0, Ordering::SeqCst);
        EIE[w].store(0, Ordering::SeqCst);
    }
    *EIP_REMOTE.lock().unwrap() = [0; IMSIC_WORDS];
    *EIE_REMOTE.lock().unwrap() = [0; IMSIC_WORDS];
    EIDELIVERY.store(0, Ordering::SeqCst);
    EITHRESHOLD.store(0, Ordering::SeqCst);
    TICK.store(0, Ordering::SeqCst);
    OP_SEQ.lock().unwrap().clear();
    CLAIM_LOG.lock().unwrap().clear();
    INJECT_EID.store(0, Ordering::SeqCst);
    CALLS_1.store(0, Ordering::SeqCst);
    CALLS_60.store(0, Ordering::SeqCst);
    CALLS_133.store(0, Ordering::SeqCst);
    CALLS_277.store(0, Ordering::SeqCst);
}

fn tick() -> usize {
    TICK.fetch_add(1, Ordering::SeqCst)
}

fn push_op(name: &'static str) {
    let t = tick();
    OP_SEQ.lock().unwrap().push((name, t));
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

/// Remote-file views (MS14): the remote hart's own EIE words.
fn enabled_bit_remote(eid: usize) -> bool {
    EIE_REMOTE.lock().unwrap()[eie_word(eid)] & eie_bit(eid) != 0
}

fn remote_eie_all_zero() -> bool {
    EIE_REMOTE.lock().unwrap().iter().all(|w| *w == 0)
}

// ---------------------------------------------------------------------------
// Fake backend
// ---------------------------------------------------------------------------

struct FakeBackend {
    aplic: std::collections::BTreeMap<u32, u32>,
    write_log: Vec<(u32, u32)>,
    hart: usize,
    sie: bool,
    sext: bool,
    /// Modeled SIE value at each critical-section entry.
    sie_at_entry: Vec<bool>,
    in_critical: bool,
    /// Board model (runs 2–3): the msiaddrcfg pair is write-only silicon —
    /// writes are accepted but reads always return 0 (sourcecfg and all
    /// other registers read back normally). Default false = readable model.
    msicfg_reads_zero: bool,
    /// MS14: which hart's local file `with_imsic` exposes. `false` (the
    /// default) selects the boot file statics; `true` selects the
    /// independent remote/secondary file.
    remote_file: bool,
}

impl FakeBackend {
    fn new(sie: bool) -> Self {
        FakeBackend {
            aplic: std::collections::BTreeMap::new(),
            write_log: Vec::new(),
            hart: 3,
            sie,
            sext: false,
            sie_at_entry: Vec::new(),
            in_critical: false,
            msicfg_reads_zero: false,
            remote_file: false,
        }
    }

    /// Switches the local file the backend exposes (per-hart model).
    fn set_remote_file(&mut self, remote: bool) {
        self.remote_file = remote;
    }

    /// Test-side reads use DT-window-relative offsets translated to the
    /// absolute register address (0xe0804000 + offset).
    fn aplic_writes(&self, offset: u32) -> Vec<u32> {
        let addr = aia::APLIC_BASE_PADDR as u32 + offset;
        self.write_log
            .iter()
            .filter(|(off, _)| *off == addr)
            .map(|(_, v)| *v)
            .collect()
    }

    fn aplic_get(&self, offset: u32) -> u32 {
        let addr = aia::APLIC_BASE_PADDR as u32 + offset;
        *self.aplic.get(&addr).unwrap_or(&0)
    }
}

struct ImsicFile {
    /// Which hart's file this access touches (MS14 per-hart model).
    remote: bool,
}

impl ImsicFile {
    fn eie(&self, word: usize) -> u64 {
        if self.remote {
            EIE_REMOTE.lock().unwrap()[word]
        } else {
            EIE[word].load(Ordering::SeqCst)
        }
    }

    fn set_eie(&self, word: usize, value: u64) {
        if self.remote {
            EIE_REMOTE.lock().unwrap()[word] = value;
        } else {
            EIE[word].store(value, Ordering::SeqCst);
        }
    }

    fn eip(&self, word: usize) -> u64 {
        if self.remote {
            EIP_REMOTE.lock().unwrap()[word]
        } else {
            EIP[word].load(Ordering::SeqCst)
        }
    }

    /// Claim-side pending clear (the only EIP mutation the model allows).
    fn clear_eip_bit(&self, eid: usize) {
        let mask = !eie_bit(eid);
        if self.remote {
            EIP_REMOTE.lock().unwrap()[eie_word(eid)] &= mask;
        } else {
            EIP[eie_word(eid)].fetch_and(mask, Ordering::SeqCst);
        }
    }

    fn set_eip_bit(&self, eid: usize) {
        if self.remote {
            EIP_REMOTE.lock().unwrap()[eie_word(eid)] |= eie_bit(eid);
        } else {
            EIP[eie_word(eid)].fetch_or(eie_bit(eid), Ordering::SeqCst);
        }
    }

    fn pending_set(&self, eid: usize) -> bool {
        self.eip(eie_word(eid)) & eie_bit(eid) != 0
    }

    fn enabled_bit(&self, eid: usize) -> bool {
        self.eie(eie_word(eid)) & eie_bit(eid) != 0
    }
}

impl ImsicAccess for ImsicFile {
    fn read(&mut self, selector: u16) -> u64 {
        push_op("imsic-read");
        read_selector(self, selector)
    }

    fn write(&mut self, selector: u16, value: u64) {
        let name = match selector {
            0x70 => "delivery-write",
            0x72 => "threshold-write",
            s if (0xc0..0xc0 + 2 * IMSIC_WORDS as u16).contains(&s) => "eie-write",
            _ => "imsic-write",
        };
        push_op(name);
        write_selector(self, selector, value);
    }

    fn claim(&mut self) -> u64 {
        // Highest priority = lowest pending AND enabled EID; the atomic swap
        // returns and clears exactly that one.
        push_op("claim");
        for eid in 1..512 {
            if self.pending_set(eid) && self.enabled_bit(eid) {
                self.clear_eip_bit(eid);
                let raw = ((eid as u64) << 16) | eid as u64;
                CLAIM_LOG.lock().unwrap().push(raw);
                return raw;
            }
        }
        CLAIM_LOG.lock().unwrap().push(0);
        0
    }
}

fn read_selector(file: &ImsicFile, selector: u16) -> u64 {
    match selector {
        0x70 => EIDELIVERY.load(Ordering::SeqCst),
        0x72 => EITHRESHOLD.load(Ordering::SeqCst),
        s if (0xc0..0xc0 + 2 * IMSIC_WORDS as u16).contains(&s) && s % 2 == 0 => {
            file.eie(((s - 0xc0) / 2) as usize)
        }
        s if (0x80..0x80 + 2 * IMSIC_WORDS as u16).contains(&s) && s % 2 == 0 => {
            file.eip(((s - 0x80) / 2) as usize)
        }
        _ => 0,
    }
}

fn write_selector(file: &ImsicFile, selector: u16, value: u64) {
    match selector {
        0x70 => EIDELIVERY.store(value, Ordering::SeqCst),
        0x72 => EITHRESHOLD.store(value, Ordering::SeqCst),
        // EIP registers are read-only: writes are ignored (WARL model).
        s if (0xc0..0xc0 + 2 * IMSIC_WORDS as u16).contains(&s) && s % 2 == 0 => {
            file.set_eie(((s - 0xc0) / 2) as usize, value)
        }
        _ => {}
    }
}

impl AiaBackend for FakeBackend {
    fn aplic_read(&mut self, offset: u32) -> u32 {
        let addr = aia::aplic_base() as u32 + offset;
        // Write-only msicfg model: the pair always reads 0 on this silicon.
        if self.msicfg_reads_zero && (offset == 0x1bc8 || offset == 0x1bcc) {
            return 0;
        }
        self.aplic.get(&addr).copied().unwrap_or(0)
    }

    fn aplic_write(&mut self, offset: u32, value: u32) {
        let addr = aia::aplic_base() as u32 + offset;
        let win_off = addr - aia::APLIC_BASE_PADDR as u32;
        let name = if (APLIC_CLRIE_BASE..APLIC_CLRIE_BASE + 0x80).contains(&win_off) {
            "aplic-clrie"
        } else if (APLIC_SETIE_BASE..APLIC_SETIE_BASE + 0x80).contains(&win_off) {
            "aplic-setie"
        } else {
            "aplic-write"
        };
        push_op(name);
        self.write_log.push((addr, value));
        self.aplic.insert(addr, value);
    }

    fn with_imsic<R>(&mut self, f: impl FnOnce(&mut dyn ImsicAccess) -> R) -> R {
        assert!(!self.in_critical, "nested IMSIC critical section");
        self.in_critical = true;
        self.sie_at_entry.push(self.sie);
        let saved = self.sie;
        self.sie = false; // modeled disable_irqs()
        let r = f(&mut ImsicFile { remote: self.remote_file });
        if saved {
            self.sie = true; // restore only as found — never unconditionally
        }
        self.in_critical = false;
        r
    }

    fn set_sext(&mut self, on: bool) {
        push_op(if on { "sext-on" } else { "sext-off" });
        self.sext = on;
    }

    fn hart(&self) -> usize {
        self.hart
    }
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

// ---------------------------------------------------------------------------
// Handlers (fn() pointers carry no state; state lives in the statics above)
// ---------------------------------------------------------------------------

fn handler_1() {
    CALLS_1.fetch_add(1, Ordering::SeqCst);
    handler_common();
}

fn handler_60() {
    CALLS_60.fetch_add(1, Ordering::SeqCst);
    handler_common();
}

fn handler_133() {
    CALLS_133.fetch_add(1, Ordering::SeqCst);
    handler_common();
}

fn handler_277() {
    CALLS_277.fetch_add(1, Ordering::SeqCst);
    handler_common();
}

fn handler_common() {
    push_op("handler");
    let eid = INJECT_EID.swap(0, Ordering::SeqCst);
    if eid != 0 {
        set_pending(eid);
    }
}

fn calls_for(source: usize) -> &'static AtomicUsize {
    match source {
        1 => &CALLS_1,
        60 => &CALLS_60,
        133 => &CALLS_133,
        277 => &CALLS_277,
        _ => unreachable!("unsupported route {source}"),
    }
}


/// Serializes tests: they share the fake IMSIC/APLIC statics and libtest
/// runs them on parallel threads by default.
fn test_guard() -> std::sync::MutexGuard<'static, ()> {
    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

// ---------------------------------------------------------------------------
// Tests (task 1.1 witness list)
// ---------------------------------------------------------------------------

#[test]
fn stopei_id_is_high_field() {
    let _guard = test_guard();
    // raw = 60<<16 | 3 → identity in the high field, priority in the low.
    assert_eq!(parse_stopei((60 << 16) | 3), Some((60, 3)));
    assert_eq!(parse_stopei((1 << 16) | 1), Some((1, 1)));
    assert_eq!(parse_stopei(0), None);
}

#[test]
fn rv64_selectors_are_even() {
    let _guard = test_guard();
    for word in 0..IMSIC_WORDS {
        assert_eq!(eie_selector(word) % 2, 0, "odd EIE selector for word {word}");
        assert_eq!(eip_selector(word) % 2, 0, "odd EIP selector for word {word}");
    }
    assert_eq!(eie_selector(0), 0xc0);
    assert_eq!(eip_selector(0), 0x80);
    assert_eq!(eie_selector(1), 0xc2);
    assert_eq!(eip_selector(1), 0x82);
}

#[test]
fn global_and_per_hart_init_are_separate() {
    let _guard = test_guard();
    reset_fake_imsic();
    let mut b = FakeBackend::new(true);
    init_global(&mut b, |_| {}).expect("global init");
    // Global init touches only APLIC: no CSR critical section was entered.
    assert!(b.sie_at_entry.is_empty(), "global init entered IMSIC CSRs");
    assert!(!b.write_log.is_empty(), "global init wrote no APLIC regs");

    reset_fake_imsic();
    let mut b = FakeBackend::new(true);
    init_percpu(&mut b).expect("percpu init");
    // Per-hart init touches only local CSRs + SEXT: no APLIC writes.
    assert!(b.write_log.is_empty(), "percpu init wrote APLIC regs");
    assert_eq!(EIDELIVERY.load(Ordering::SeqCst), 1);
    assert_eq!(EITHRESHOLD.load(Ordering::SeqCst), 0);
    for w in 0..IMSIC_WORDS {
        assert_eq!(EIE[w].load(Ordering::SeqCst), 0, "EIE word {w} not cleared");
    }
    assert!(b.sext, "percpu init must open SEXT last");
}

#[test]
fn source512_last_bitmap_word_is_cleared() {
    let _guard = test_guard();
    reset_fake_imsic();
    let mut b = FakeBackend::new(true);
    init_global(&mut b, |_| {}).expect("global init");
    // Word 16 (sources 512..543) must touch only source 512 = bit 0.
    let clrie_word16 = APLIC_CLRIE_BASE + 4 * 16;
    assert_eq!(b.aplic_writes(clrie_word16), vec![1], "CLRIE word 16");
    let clrip_word16 = APLIC_CLRIP_BASE + 4 * 16;
    assert_eq!(b.aplic_writes(clrip_word16), vec![1], "CLRIP word 16");
    // Full words stay full.
    assert_eq!(b.aplic_writes(APLIC_CLRIE_BASE), vec![0xffff_ffff]);
}

#[test]
fn unsupported_registration_has_no_side_effect() {
    let _guard = test_guard();
    for bad in [0usize, 2, 511, 512] {
        reset_fake_imsic();
        let store = FakeStore::new();
        let chip = AiaChip::new(&store);
        let mut b = FakeBackend::new(true);
        assert!(!chip.register(&mut b, bad, handler_1), "source {bad} accepted");
        assert!(b.write_log.is_empty(), "source {bad} touched hardware");
        // Unknown EIDs can still be claimed once locally enabled, and are
        // diagnosed, not dispatched.
        set_pending(bad.clamp(1, 511));
        set_eid_enabled(&mut b, bad.clamp(1, 511), true);
        assert_eq!(chip.handle_external(&mut b), None);
        assert_eq!(chip.unknown_count(), 1);
        assert_eq!(CALLS_1.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn claim_precedes_handler() {
    let _guard = test_guard();
    reset_fake_imsic();
    let store = FakeStore::new();
    let chip = AiaChip::new(&store);
    let mut b = FakeBackend::new(true);
    assert!(chip.register(&mut b, 1, handler_1));
    let seq_start = OP_SEQ.lock().unwrap().len();
    set_pending(1);
    assert_eq!(chip.handle_external(&mut b), Some(1));
    assert_eq!(CALLS_1.load(Ordering::SeqCst), 1);
    let seq = OP_SEQ.lock().unwrap()[seq_start..].to_vec();
    let claim_idx = seq.iter().position(|(n, _)| *n == "claim").expect("no claim");
    let handler_idx = seq.iter().position(|(n, _)| *n == "handler").expect("no handler");
    assert!(claim_idx < handler_idx, "handler ran before claim: {seq:?}");
    // Exactly one claim per SEI: the dispatch returned before a second.
    assert_eq!(CLAIM_LOG.lock().unwrap().len(), 1);
}

#[test]
fn new_pending_survives_handler() {
    let _guard = test_guard();
    reset_fake_imsic();
    let store = FakeStore::new();
    let chip = AiaChip::new(&store);
    let mut b = FakeBackend::new(true);
    assert!(chip.register(&mut b, 1, handler_1));
    // The handler itself raises a new pending for the same EID (retrap model).
    INJECT_EID.store(1, Ordering::SeqCst);
    set_pending(1);
    assert_eq!(chip.handle_external(&mut b), Some(1));
    // The new pending survived the claim+EOI and is serviced by the next SEI.
    assert!(pending_set(1), "new pending was lost by the first dispatch");
    assert_eq!(chip.handle_external(&mut b), Some(1));
    assert_eq!(CALLS_1.load(Ordering::SeqCst), 2);
    assert!(!pending_set(1));
}

#[test]
fn disable_orders_aplic_before_eie() {
    let _guard = test_guard();
    reset_fake_imsic();
    let store = FakeStore::new();
    let chip = AiaChip::new(&store);
    let mut b = FakeBackend::new(true);
    assert!(chip.register(&mut b, 1, handler_1));
    let seq_start = OP_SEQ.lock().unwrap().len();
    chip.set_enable(&mut b, 1, false);
    let seq = OP_SEQ.lock().unwrap()[seq_start..].to_vec();
    let clrie = seq.iter().position(|(n, _)| *n == "aplic-clrie").expect("no CLRIE write");
    let eie_clear = seq
        .iter()
        .position(|(n, _)| *n == "eie-write")
        .expect("no EIE write");
    assert!(clrie < eie_clear, "EIE touched before APLIC disable: {seq:?}");
    // Hardware ends masked in both layers (source 1 → word 0, bit 1).
    assert_eq!(b.aplic_get(APLIC_CLRIE_BASE), 2);
    assert!(!enabled_bit(1));
}

#[test]
fn selector_access_restores_original_sie() {
    let _guard = test_guard();
    // Originally enabled: restored enabled.
    reset_fake_imsic();
    let store = FakeStore::new();
    let chip = AiaChip::new(&store);
    let mut b = FakeBackend::new(true);
    assert!(chip.register(&mut b, 1, handler_1));
    set_pending(1);
    assert_eq!(chip.handle_external(&mut b), Some(1));
    assert!(b.sie, "SIE not restored after originally-enabled access");
    assert_eq!(b.sie_at_entry, vec![true, true]);

    // Originally disabled: must NOT be unconditionally re-enabled.
    reset_fake_imsic();
    let store = FakeStore::new();
    let chip = AiaChip::new(&store);
    let mut b = FakeBackend::new(false);
    assert!(chip.register(&mut b, 1, handler_1));
    assert!(!b.sie, "register re-enabled a disabled SIE");
    assert_eq!(chip.handle_external(&mut b), None);
    assert!(!b.sie, "dispatch re-enabled a disabled SIE");
}

#[test]
fn spurious_and_unknown_do_not_dispatch() {
    let _guard = test_guard();
    reset_fake_imsic();
    let store = FakeStore::new();
    let chip = AiaChip::new(&store);
    let mut b = FakeBackend::new(true);
    // Unknown EID: claimed (after local enable), diagnosed, no handler call.
    set_pending(7);
    set_eid_enabled(&mut b, 7, true);
    assert_eq!(chip.handle_external(&mut b), None);
    assert_eq!(chip.unknown_count(), 1);
    assert_eq!(CALLS_1.load(Ordering::SeqCst), 0);
    // Zero claim (spurious): counted, returns None.
    assert_eq!(chip.handle_external(&mut b), None);
    assert_eq!(chip.spurious_count(), 1);
    assert!(!pending_set(7));
}

// ---------------------------------------------------------------------------
// Tests (task 1.2 additions: registration lifecycle + route coverage)
// ---------------------------------------------------------------------------

#[test]
fn register_enables_each_supported_route() {
    let _guard = test_guard();
    let routes = [
        (1usize, SM_DETACHED),
        (60, SM_LEVEL_HIGH),
        (133, SM_LEVEL_HIGH),
        (277, SM_LEVEL_HIGH),
    ];
    for (source, sm) in routes {
        reset_fake_imsic();
        let store = FakeStore::new();
        let chip = AiaChip::new(&store);
        let mut b = FakeBackend::new(true);
        assert!(chip.register(&mut b, source, handler_1), "source {source}");
        let scfg = APLIC_SOURCECFG_BASE + 4 * (source as u32 - 1);
        assert_eq!(b.aplic_get(scfg), sm, "sourcecfg SM for source {source}");
        let tgt = APLIC_TARGET_BASE + 4 * (source as u32 - 1);
        let want = ((b.hart as u32) << 18) | source as u32;
        assert_eq!(b.aplic_get(tgt), want, "target for source {source}");
        assert_eq!(
            b.aplic_writes(APLIC_SETIE_BASE + 4 * (source as u32 / 32)),
            vec![1 << (source % 32)],
            "SETIE for source {source}"
        );
        assert!(enabled_bit(source), "local EIE for source {source}");
    }
    // Route table lookup agrees.
    for source in [1usize, 60, 133, 277] {
        assert_eq!(route_for(source).map(|r| r.source), Some(source));
    }
}

#[test]
fn duplicate_registration_rejected_without_hardware() {
    let _guard = test_guard();
    reset_fake_imsic();
    let store = FakeStore::new();
    let chip = AiaChip::new(&store);
    let mut b = FakeBackend::new(true);
    assert!(chip.register(&mut b, 1, handler_1));
    let writes_before = b.write_log.len();
    assert!(!chip.register(&mut b, 1, handler_60), "duplicate accepted");
    assert_eq!(b.write_log.len(), writes_before, "duplicate touched hardware");
    assert_eq!(CALLS_60.load(Ordering::SeqCst), 0);
}

#[test]
fn unregister_disables_before_removal() {
    let _guard = test_guard();
    reset_fake_imsic();
    let store = FakeStore::new();
    let chip = AiaChip::new(&store);
    let mut b = FakeBackend::new(true);
    assert!(chip.register(&mut b, 1, handler_1));
    let seq_start = OP_SEQ.lock().unwrap().len();
    assert!(chip.unregister(&mut b, 1).is_some(), "unregister lost an existing handler");
    let seq = OP_SEQ.lock().unwrap()[seq_start..].to_vec();
    let clrie = seq.iter().position(|(n, _)| *n == "aplic-clrie").expect("no CLRIE");
    let eie_clear = seq.iter().position(|(n, _)| *n == "eie-write").expect("no EIE write");
    assert!(clrie < eie_clear, "unregister must mask APLIC before EIE: {seq:?}");
    // After removal the route stays masked in both layers, so a fresh
    // pending is retained unclaimed — no handler call, no unknown dispatch.
    set_pending(1);
    assert_eq!(chip.handle_external(&mut b), None);
    assert_eq!(chip.spurious_count(), 1);
    assert_eq!(CALLS_1.load(Ordering::SeqCst), 0);
    // Second unregister reports the absence (handler already removed).
    assert!(chip.unregister(&mut b, 1).is_none());
}

#[test]
fn set_enable_masks_without_removing_the_handler() {
    let _guard = test_guard();
    reset_fake_imsic();
    let store = FakeStore::new();
    let chip = AiaChip::new(&store);
    let mut b = FakeBackend::new(true);
    assert!(chip.register(&mut b, 1, handler_1));
    chip.set_enable(&mut b, 1, false);
    set_pending(1);
    // EIE masked: the pending is retained (not claimable), no dispatch.
    assert_eq!(chip.handle_external(&mut b), None);
    assert_eq!(chip.spurious_count(), 1);
    assert!(pending_set(1), "masked pending was dropped");
    // Re-enable without a new trigger: the retained pending is serviced.
    chip.set_enable(&mut b, 1, true);
    assert_eq!(chip.handle_external(&mut b), Some(1));
    assert_eq!(CALLS_1.load(Ordering::SeqCst), 1);
}

#[test]
fn domain_config_uses_msi_delivery_mode() {
    let _guard = test_guard();
    reset_fake_imsic();
    let mut b = FakeBackend::new(true);
    init_global(&mut b, |_| {}).expect("global init");
    // Final domaincfg = IE | DM (bit 8 | bit 2).
    assert_eq!(b.aplic_get(APLIC_DOMAINCFG), APLIC_DOMAINCFG_IE | 0x4);
    // S-mode MSI address config landed and passed the S-mode readback criteria.
    let low = b.aplic_get(APLIC_SMSICFGADDR);
    let high = b.aplic_get(APLIC_SMSICFGADDRH);
    assert_ne!(low, 0);
    assert_eq!((high >> 20) & 0x7, 6, "LHXS field");
    assert_eq!(high & 0xfff, 0, "BAPPN field");
    assert_ne!(high & (1 << 31), 0, "lock bit must be written per R69");
}

#[test]
fn msicfg_readback_refused_warns_and_init_continues() {
    // Board runs 2–3: the msiaddrcfg pair reads 0 even though the domain is
    // alive and writable (sourcecfg sticks, DM RO=1 MSI-only). Init must
    // record the refused readback as an informational line and continue —
    // the delivery criterion is stage 9's software-MSI test, not the
    // register readback.
    let _guard = test_guard();
    reset_fake_imsic();
    let mut b = FakeBackend::new(true);
    b.msicfg_reads_zero = true;
    let mut lines = Vec::new();
    init_global(&mut b, |l| lines.push(l.to_string()))
        .expect("readback refusal must not abort init");
    assert!(
        lines.iter().any(|l| l.contains("readback-refused")),
        "missing refusal warning: {lines:?}"
    );
    assert_eq!(b.aplic_get(APLIC_DOMAINCFG), APLIC_DOMAINCFG_IE | 0x4);
    assert_eq!(b.aplic_get(APLIC_SMSICFGADDR), aia::smsicfgaddr_low());
}

// ---------------------------------------------------------------------------
// MS17 Iteration 000 task T2: UART route (source 42), cfg-split contracts
// ---------------------------------------------------------------------------

/// Legacy shape (`irq-if` only): source 42 stays rejected like any unrouted
/// line — no route, no registration, no hardware effect.
#[cfg(not(feature = "uart-irq"))]
#[test]
fn route42_rejected_without_uart_irq_optin() {
    let _guard = test_guard();
    assert!(aia::route_for(42).is_none(), "42 routed without opt-in");
    reset_fake_imsic();
    let store = FakeStore::new();
    let chip = AiaChip::new(&store);
    let mut b = FakeBackend::new(true);
    assert!(!chip.register(&mut b, 42, handler_1), "42 accepted without opt-in");
    assert!(b.write_log.is_empty(), "rejected 42 touched hardware");
}

/// Opt-in shape (`uart-irq`): source 42 LevelHigh registers, claims twice and
/// disables before removal, exactly like a wired MS10 route.
#[cfg(feature = "uart-irq")]
#[test]
fn route42_optin_registers_claims_and_disables_before_removal() {
    use aia::APLIC_SOURCECFG_BASE;

    let _guard = test_guard();
    assert_eq!(aia::route_for(42).map(|r| (r.source, r.sm)), Some((42, SM_LEVEL_HIGH)));
    reset_fake_imsic();
    let store = FakeStore::new();
    let chip = AiaChip::new(&store);
    let mut b = FakeBackend::new(true);
    assert!(chip.register(&mut b, 42, handler_1), "opt-in 42 registration failed");
    assert_eq!(b.aplic_get(APLIC_SOURCECFG_BASE + 4 * 41), SM_LEVEL_HIGH);
    assert!(enabled_bit(42));

    // Two real claims through the production dispatch: each returns+completes
    // exactly one pending and calls the handler.
    set_pending(42);
    assert_eq!(chip.handle_external(&mut b), Some(42));
    assert_eq!(CALLS_1.load(Ordering::SeqCst), 1);
    set_pending(42);
    assert_eq!(chip.handle_external(&mut b), Some(42));
    assert_eq!(CALLS_1.load(Ordering::SeqCst), 2);

    // unregister masks both layers before removing the handler.
    assert!(chip.unregister(&mut b, 42).is_some());
    set_pending(42);
    assert_eq!(chip.handle_external(&mut b), None);
    assert_eq!(chip.spurious_count(), 1, "masked pending must be retained");
    assert_eq!(CALLS_1.load(Ordering::SeqCst), 2);
}

/// Opt-in shape: 42 registration is still duplicate-safe.
#[cfg(feature = "uart-irq")]
#[test]
fn route42_optin_duplicate_rejected_without_hardware() {
    let _guard = test_guard();
    reset_fake_imsic();
    let store = FakeStore::new();
    let chip = AiaChip::new(&store);
    let mut b = FakeBackend::new(true);
    assert!(chip.register(&mut b, 42, handler_1));
    let writes_before = b.write_log.len();
    assert!(!chip.register(&mut b, 42, handler_60), "duplicate 42 accepted");
    assert_eq!(b.write_log.len(), writes_before, "duplicate 42 touched hardware");
}

// ---------------------------------------------------------------------------
// MS14 Iteration 000 task T3: SMP fixed-route control plane. The fake now
// models two independent local files (boot + remote), so the tests prove
// the global-only gate never leaks into a caller's own file.
// ---------------------------------------------------------------------------

#[test]
fn remote_source_gate_preserves_local_eie() {
    let _guard = test_guard();
    aia::record_boot_hart(3);
    reset_fake_imsic();
    let store = FakeStore::new();
    let chip = AiaChip::new(&store);
    let mut b = FakeBackend::new(true);
    // Boot registers route 1: target + boot-local EIE + global SETIE.
    assert!(chip.smp_register(&mut b, 3, 1, handler_1));
    assert!(enabled_bit(1), "boot local EIE missing after registration");
    let writes_after_register = b.write_log.len();

    // A remote caller (hart 5, its own file) masks and re-opens the route.
    b.set_remote_file(true);
    let ops_start = OP_SEQ.lock().unwrap().len();
    chip.smp_set_enable(&mut b, 1, false);
    chip.smp_set_enable(&mut b, 1, true);
    let ops = OP_SEQ.lock().unwrap()[ops_start..].to_vec();
    // Only global APLIC gate writes: no local-file read-modify-write.
    assert!(
        !ops.iter().any(|(n, _)| *n == "eie-write" || *n == "imsic-read"),
        "remote gate touched the caller's local file: {ops:?}"
    );
    assert_eq!(ops.iter().filter(|(n, _)| *n == "aplic-clrie").count(), 1);
    assert_eq!(ops.iter().filter(|(n, _)| *n == "aplic-setie").count(), 1);
    // The remote caller's own file stays cleared, and the boot file's EIE
    // bit — the actual delivery gate — is untouched.
    assert!(remote_eie_all_zero(), "remote file EIE changed by the gate");
    b.set_remote_file(false);
    assert!(enabled_bit(1), "boot local EIE changed by a remote gate op");
    assert_eq!(b.write_log.len() - writes_after_register, 2, "gate wrote more than SETIE/CLRIE");

    // The free global-gate helper behaves identically (SETIE/CLRIE only).
    b.set_remote_file(true);
    let ops_start = OP_SEQ.lock().unwrap().len();
    set_source_gate(&mut b, 1, false);
    let ops = OP_SEQ.lock().unwrap()[ops_start..].to_vec();
    assert_eq!(ops, vec![("aplic-clrie", ops[0].1)], "helper did more than CLRIE: {ops:?}");
    assert!(remote_eie_all_zero(), "helper touched the caller's file");
    // Unsupported routes stay no-ops with no hardware effect.
    let writes_before = b.write_log.len();
    chip.smp_set_enable(&mut b, 7, false);
    assert_eq!(b.write_log.len(), writes_before, "unsupported route gated");
}

#[test]
fn remote_device_registration_is_rejected_without_mmio() {
    let _guard = test_guard();
    aia::record_boot_hart(3);
    reset_fake_imsic();
    let store = FakeStore::new();
    let chip = AiaChip::new(&store);
    let mut b = FakeBackend::new(true);
    assert!(chip.smp_register(&mut b, 3, 1, handler_1), "boot registration refused");
    let writes_before = b.write_log.len();

    // Remote registration: refused with zero APLIC writes and zero local
    // file effect (the route would target the boot file anyway).
    b.set_remote_file(true);
    assert!(!chip.smp_register(&mut b, 5, 60, handler_60), "remote register accepted");
    assert_eq!(b.write_log.len(), writes_before, "remote register touched APLIC");
    assert!(remote_eie_all_zero(), "remote register touched the caller's file");
    assert_eq!(CALLS_60.load(Ordering::SeqCst), 0);
    assert!(route_for(60).is_some(), "route 60 is a valid route (refusal is caller-based)");
    // Remote unregistration is refused the same way (None, no hardware).
    assert!(chip.smp_unregister(&mut b, 5, 1).is_none(), "remote unregister accepted");
    assert_eq!(b.write_log.len(), writes_before, "remote unregister touched APLIC");

    // Boot keeps full ownership: unregister masks both layers and removes.
    b.set_remote_file(false);
    assert!(chip.smp_unregister(&mut b, 3, 1).is_some(), "boot unregister lost the handler");
    assert!(b.write_log.len() > writes_before, "boot unregister wrote nothing");
}

#[test]
fn secondary_init_preserves_boot_routes() {
    let _guard = test_guard();
    aia::record_boot_hart(3);
    reset_fake_imsic();
    let store = FakeStore::new();
    let chip = AiaChip::new(&store);
    let mut b = FakeBackend::new(true);
    init_global(&mut b, |_| {}).expect("global init");
    assert!(chip.smp_register(&mut b, 3, 1, handler_1));
    // Boot route snapshot before any secondary activity.
    let scfg = b.aplic_get(APLIC_SOURCECFG_BASE);
    let tgt = b.aplic_get(APLIC_TARGET_BASE);
    let setie = b.aplic_get(APLIC_SETIE_BASE);
    assert_eq!(tgt, (3u32 << 18) | 1, "route target");
    assert!(enabled_bit(1));

    // The secondary runs its per-hart init against its OWN file: no APLIC
    // write, no boot-file mutation, no boot-hart record overwrite.
    b.set_remote_file(true);
    let writes_before = b.write_log.len();
    init_percpu(&mut b).expect("secondary percpu init");
    assert_eq!(b.write_log.len(), writes_before, "secondary percpu init wrote APLIC");
    assert_eq!(aia::boot_hart(), 3, "secondary init overwrote the boot hart record");
    assert!(remote_eie_all_zero(), "secondary file EIE not cleared by its init");
    b.set_remote_file(false);
    assert_eq!(b.aplic_get(APLIC_SOURCECFG_BASE), scfg, "boot sourcecfg changed");
    assert_eq!(b.aplic_get(APLIC_TARGET_BASE), tgt, "boot target changed");
    assert_eq!(b.aplic_get(APLIC_SETIE_BASE), setie, "boot SETIE changed");
    assert!(enabled_bit(1), "boot local EIE changed");
}
