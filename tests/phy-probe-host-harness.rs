//! PHY/MDIO probe pure-logic host test harness (MS09 Iteration 004, tasks 5.1–5.2).
//!
//! Uses `#[path]` to reference the real `kernel/src/platform/{mac_probe,phy_probe}.rs`
//! without compiling the full kernel. All hardware access goes through the
//! probe's `ProbeEnv` seam; `FakeBoard` below is a deterministic fake board
//! (register map + access-cost clock), so every test drives the actual probe
//! logic — never a copy of it. Runs via `rustc --edition=2024 --test` → /tmp
//! binary (same convention as `early-console-host-harness.rs`).
//!
//! Fake timing rules (no real sleeping, per the task contract):
//! - `wait_nanos` advances the fake clock by exactly the requested nanos.
//! - Each MMIO access advances the clock by `access_cost_ns` (default 1 ms,
//!   so the 10 ms MDIO deadline expires after ~10 polls; set to 0 for
//!   window-count tests).
#![allow(dead_code)]

extern crate core;

#[path = "../kernel/src/platform/mac_probe.rs"]
mod mac_probe;
#[path = "../kernel/src/platform/phy_probe.rs"]
mod phy_probe;

use phy_probe::{
    GOC_READ, GOC_WRITE, MDIO_ADDR_OFFSET, MDIO_CR, MDIO_DATA_OFFSET, PHY_ADDR, ProbeEnv,
};

const MAC_MDIO_ADDR: u64 = mac_probe::MAC_BASE_PADDR + MDIO_ADDR_OFFSET;
const MAC_MDIO_DATA: u64 = mac_probe::MAC_BASE_PADDR + MDIO_DATA_OFFSET;
const GPIO1_GSDR: u64 = phy_probe::GPIO_BASE_PADDR + 0x40 + phy_probe::GPIO_GSDR_OFF;
const GPIO1_GPSR: u64 = phy_probe::GPIO_BASE_PADDR + 0x40 + phy_probe::GPIO_GPSR_OFF;
const GPIO1_GPCR: u64 = phy_probe::GPIO_BASE_PADDR + 0x40 + phy_probe::GPIO_GPCR_OFF;
const IOPD_GMAC1: u64 = phy_probe::IOPD_BASE_PADDR + phy_probe::IOPD_GMAC_OFFSET;
const APBC_ASFAR: u64 = phy_probe::APBC_BASE_PADDR
    + phy_probe::APBC_ASAR_OFFSET
    + phy_probe::APBC_ASFAR_OFFSET;
const APBC_ASSAR: u64 = phy_probe::APBC_BASE_PADDR
    + phy_probe::APBC_ASAR_OFFSET
    + phy_probe::APBC_ASSAR_OFFSET;

struct FakePhy {
    /// Clause 22 registers; DATA reads are served from here.
    regs: [u16; 32],
    /// Physical peer present (link up after AN completes).
    peer_up: bool,
    /// Instant at which an AN restart resolves (restart self-clears and AN
    /// completes); `None` = no restart in flight.
    an_resolves_at: Option<u64>,
    /// Overrides the served BMSR value entirely (invalid-value scenarios).
    force_bmsr: Option<u16>,
    /// Schedule of (bmsr_read_index, peer_up) plug flips, applied in order.
    peer_schedule: Vec<(usize, bool)>,
}

struct FakeBoard {
    regs: std::collections::BTreeMap<u64, u32>,
    now_ns: u64,
    access_cost_ns: u64,
    /// MDIO ADDR busy bit stuck at 1 forever (idle wait can never clear).
    mdio_busy_stuck: bool,
    /// Nanoseconds from a trigger write until the busy bit clears
    /// (0 = immediate; huge = completion timeout).
    completion_delay_ns: u64,
    /// (reg, goc, ready_at) of the transaction currently in flight.
    mdio_pending: Option<(u32, u32, u64)>,
    /// Register number whose result was materialized into `data_reg`.
    mdio_last_reg: u32,
    data_reg: u32,
    data_reads: usize,
    write_log: Vec<(u64, u32)>,
    read_addr_log: Vec<u64>,
    /// `read_addr_log` length captured when the io_pd word was written
    /// (witness that no MMIO read precedes the voltage-domain write).
    reads_before_iopd_write: Option<usize>,
    read_reg_log: Vec<u32>,
    wait_log: Vec<u64>,
    phy: FakePhy,
    /// Armed by a pad write; the next pad readback loses its mux bits
    /// (one-shot, drives the writeback-check failure scenario).
    corrupt_next_pad_readback: bool,
    /// Test switch: pad writes arm `corrupt_next_pad_readback`.
    corrupt_on_pad_write: bool,
    /// Number of BMSR DATA reads so far (the sampling counter).
    bmsr_reads: usize,
    /// Latch-low state: set on a peer down→up transition, consumed by the
    /// next BMSR read (which then reports link=0).
    bmsr_latch_pending: bool,
    /// Mid-window MDIO failure: at this BMSR read the bus sticks busy.
    stuck_after_bmsr_reads: Option<usize>,
    /// At this BMSR read, start forcing the served BMSR value.
    force_bmsr_at: Option<(usize, u16)>,
    /// From this BMSR read on, BMCR DATA reads serve this forced value
    /// (mid-window control-state anomaly).
    bmcr_override_from: Option<(usize, u16)>,
    /// Delay from an AN-restart write until the fake AN resolves.
    negotiate_delay_ns: u64,
}

impl FakeBoard {
    fn board() -> Self {
        FakeBoard {
            regs: std::collections::BTreeMap::new(),
            now_ns: 0,
            access_cost_ns: 1_000_000,
            mdio_busy_stuck: false,
            completion_delay_ns: 0,
            mdio_pending: None,
            mdio_last_reg: 0xffff,
            data_reg: 0,
            data_reads: 0,
            write_log: Vec::new(),
            read_addr_log: Vec::new(),
            reads_before_iopd_write: None,
            read_reg_log: Vec::new(),
            wait_log: Vec::new(),
            phy: FakePhy {
                regs: [0; 32],
                peer_up: false,
                an_resolves_at: None,
                force_bmsr: None,
                peer_schedule: Vec::new(),
            },
            corrupt_next_pad_readback: false,
            corrupt_on_pad_write: false,
            bmsr_reads: 0,
            bmsr_latch_pending: false,
            stuck_after_bmsr_reads: None,
            force_bmsr_at: None,
            bmcr_override_from: None,
            negotiate_delay_ns: 250_000_000,
        }
    }

    /// Serves a BMSR DATA read: applies the plug schedule, the mid-window
    /// failure hooks and the latch-low semantics (first read after a
    /// down→up transition reports link=0 and clears the latch).
    fn serve_bmsr(&mut self) -> u16 {
        self.bmsr_reads += 1;
        while let Some(&(idx, up)) = self.phy.peer_schedule.first() {
            if idx != self.bmsr_reads {
                break;
            }
            let was_up = self.phy.peer_up;
            self.phy.peer_up = up;
            if !was_up && up {
                self.bmsr_latch_pending = true;
            }
            self.phy.peer_schedule.remove(0);
        }
        if self.stuck_after_bmsr_reads == Some(self.bmsr_reads) {
            self.mdio_busy_stuck = true;
        }
        if let Some((at, value)) = self.force_bmsr_at {
            if self.bmsr_reads >= at {
                self.phy.force_bmsr = Some(value);
            }
        }
        if let Some(forced) = self.phy.force_bmsr {
            return forced;
        }
        let an_complete = match self.phy.an_resolves_at {
            Some(t) => self.now_ns >= t,
            None => self.phy.regs[1] & phy_probe::BMSR_AN_COMPLETE != 0,
        };
        let mut v = self.phy.regs[1];
        v = (v & !phy_probe::BMSR_LINK)
            | if self.phy.peer_up && an_complete {
                phy_probe::BMSR_LINK
            } else {
                0
            };
        v = (v & !phy_probe::BMSR_AN_COMPLETE)
            | if an_complete {
                phy_probe::BMSR_AN_COMPLETE
            } else {
                0
            };
        if self.bmsr_latch_pending {
            self.bmsr_latch_pending = false;
            return v & !phy_probe::BMSR_LINK;
        }
        v
    }

    /// Serves a BMCR DATA read: the AN restart self-clears at the resolution
    /// instant.
    fn serve_bmcr(&mut self) -> u16 {
        if let Some((at, value)) = self.bmcr_override_from {
            if self.bmsr_reads >= at {
                return value;
            }
        }
        if let Some(t) = self.phy.an_resolves_at {
            if self.now_ns >= t {
                self.phy.regs[0] &= !phy_probe::BMCR_AN_RESTART;
                self.phy.regs[1] |= phy_probe::BMSR_AN_COMPLETE;
            }
        }
        self.phy.regs[0]
    }
}

impl ProbeEnv for FakeBoard {
    fn mmio_read(&mut self, paddr: u64) -> u32 {
        self.now_ns += self.access_cost_ns;
        self.read_addr_log.push(paddr);
        match paddr {
            MAC_MDIO_ADDR => {
                if self.mdio_busy_stuck {
                    return 1;
                }
                if let Some((reg, goc, ready)) = self.mdio_pending {
                    if self.now_ns >= ready {
                        // Transaction completes: materialize the result.
                        if goc == GOC_READ >> 2 {
                            self.data_reg = self.phy.regs[reg as usize] as u32;
                        } else {
                            self.phy.regs[reg as usize] = self.data_reg as u16;
                        }
                        self.mdio_last_reg = reg;
                        self.mdio_pending = None;
                    }
                    return if self.mdio_pending.is_some() { 1 } else { 0 };
                }
                0
            }
            MAC_MDIO_DATA => {
                self.data_reads += 1;
                self.read_reg_log.push(self.mdio_last_reg);
                if self.mdio_last_reg == phy_probe::BMSR_REG {
                    self.data_reg = self.serve_bmsr() as u32;
                } else if self.mdio_last_reg == phy_probe::BMCR_REG {
                    self.data_reg = self.serve_bmcr() as u32;
                }
                self.data_reg
            }
            _ => {
                let v = *self.regs.get(&paddr).unwrap_or(&0);
                if self.corrupt_next_pad_readback
                    && phy_probe::PAD_MFPR_BASE_PADDR <= paddr
                    && paddr < phy_probe::IOPD_BASE_PADDR
                {
                    self.corrupt_next_pad_readback = false;
                    return v & !phy_probe::PAD_MUX_MASK;
                }
                v
            }
        }
    }

    fn mmio_write(&mut self, paddr: u64, value: u32) {
        self.now_ns += self.access_cost_ns;
        self.write_log.push((paddr, value));
        if paddr == IOPD_GMAC1 {
            self.reads_before_iopd_write = Some(self.read_addr_log.len());
        }
        match paddr {
            MAC_MDIO_DATA => self.data_reg = value,
            MAC_MDIO_ADDR => {
                if value & 1 == 0 {
                    return; // not a trigger
                }
                let goc = (value >> 2) & 3;
                let reg = (value >> 16) & 0x1f;
                let phy = (value >> 21) & 0x1f;
                assert_eq!(phy, PHY_ADDR, "probe must address PHY {PHY_ADDR} only");
                assert_eq!((value >> 8) & 0xf, MDIO_CR, "probe must use CR=5");
                assert_eq!(value & 0b10, 0, "C45E must stay clear");
                assert!(goc == GOC_READ >> 2 || goc == GOC_WRITE >> 2, "bad GOC");
                let ready = self.now_ns + self.completion_delay_ns;
                self.mdio_pending = Some((reg, goc, ready));
                if goc == GOC_WRITE >> 2
                    && reg == phy_probe::BMCR_REG
                    && (self.data_reg as u16) & phy_probe::BMCR_AN_RESTART != 0
                {
                    self.phy.an_resolves_at = Some(self.now_ns + self.negotiate_delay_ns);
                }
            }
            _ => {
                self.regs.insert(paddr, value);
                if self.corrupt_on_pad_write
                    && phy_probe::PAD_MFPR_BASE_PADDR <= paddr
                    && paddr < phy_probe::IOPD_BASE_PADDR
                {
                    self.corrupt_next_pad_readback = true;
                }
            }
        }
    }

    fn now_nanos(&self) -> u64 {
        self.now_ns
    }

    fn wait_nanos(&mut self, nanos: u64) {
        self.wait_log.push(nanos);
        self.now_ns += nanos;
    }
}

// ---------------------------------------------------------------------------
// MDIO transaction rules (5.1 contract item 5)
// ---------------------------------------------------------------------------

#[test]
fn mdio_addr_value_layout_and_rejects() {
    // read command for PHY 1 reg 2: GB|GOC|CR|reg|phy
    let v = phy_probe::mii_addr_value(PHY_ADDR, 2, GOC_READ).unwrap();
    assert_eq!(v & 1, 1, "busy bit set to trigger");
    assert_eq!(v & 0b10, 0, "C45E clear");
    assert_eq!((v >> 2) & 3, GOC_READ >> 2);
    assert_eq!((v >> 8) & 0xf, MDIO_CR);
    assert_eq!((v >> 16) & 0x1f, 2);
    assert_eq!((v >> 21) & 0x1f, 1);
    assert_eq!(v, 0x0022_050d);
    // write command opcode
    let w = phy_probe::mii_addr_value(PHY_ADDR, 0, GOC_WRITE).unwrap();
    assert_eq!((w >> 2) & 3, GOC_WRITE >> 2);
    // illegal register and PHY rejected
    assert!(phy_probe::mii_addr_value(PHY_ADDR, 32, GOC_READ).is_none());
    assert!(phy_probe::mii_addr_value(32, 0, GOC_WRITE).is_none());
}

#[test]
fn mdio_read_happy_returns_low16() {
    let mut b = FakeBoard::board();
    b.phy.regs[2] = 0x001c;
    b.phy.regs[3] = 0xc916;
    assert_eq!(phy_probe::mdio_read(&mut b, PHY_ADDR, 2), Ok(0x001c));
    assert_eq!(phy_probe::mdio_read(&mut b, PHY_ADDR, 3), Ok(0xc916));
    // DATA sampled after completion, one trigger per transaction
    assert_eq!(b.data_reads, 2);
    assert_eq!(b.read_reg_log, vec![2, 3]);
}

#[test]
fn mdio_read_idle_timeout_reports_and_reads_no_data() {
    let mut b = FakeBoard::board();
    b.mdio_busy_stuck = true;
    let err = phy_probe::mdio_read(&mut b, PHY_ADDR, 2).unwrap_err();
    match err {
        phy_probe::MdioError::IdleTimeout { reg, raw_addr } => {
            assert_eq!(reg, 2);
            assert_eq!(raw_addr, 0x0022_050d);
        }
        other => panic!("expected idle timeout, got {other:?}"),
    }
    assert_eq!(b.data_reads, 0, "no DATA may be read on idle timeout");
}

#[test]
fn mdio_read_completion_timeout_reads_no_data() {
    let mut b = FakeBoard::board();
    b.completion_delay_ns = u64::MAX / 4; // never resolves within 10 ms
    let err = phy_probe::mdio_read(&mut b, PHY_ADDR, 3).unwrap_err();
    assert!(matches!(err, phy_probe::MdioError::CompletionTimeout { reg: 3, .. }));
    assert_eq!(b.data_reads, 0, "incomplete DATA must not be read");
}

#[test]
fn mdio_completion_within_deadline_ok() {
    let mut b = FakeBoard::board();
    b.phy.regs[0] = 0x1140;
    b.completion_delay_ns = 9_000_000; // 9 ms < 10 ms deadline
    assert_eq!(phy_probe::mdio_read(&mut b, PHY_ADDR, 0), Ok(0x1140));
}

#[test]
fn mdio_completion_after_deadline_times_out() {
    let mut b = FakeBoard::board();
    b.completion_delay_ns = 11_000_000; // 11 ms > 10 ms deadline; a per-poll
    // deadline reset would never time out here.
    let err = phy_probe::mdio_read(&mut b, PHY_ADDR, 0).unwrap_err();
    assert!(matches!(err, phy_probe::MdioError::CompletionTimeout { .. }));
}

#[test]
fn mdio_write_rejects_illegal_reg_without_any_write() {
    let mut b = FakeBoard::board();
    let err = phy_probe::mdio_write(&mut b, PHY_ADDR, 32, 0x1234).unwrap_err();
    assert!(matches!(err, phy_probe::MdioError::IllegalReg { reg: 32 }));
    assert!(b.write_log.is_empty(), "illegal register must not touch MMIO");
}

#[test]
fn mdio_write_writes_data_before_addr() {
    let mut b = FakeBoard::board();
    b.phy.regs[0] = 0x1140;
    phy_probe::mdio_write(&mut b, PHY_ADDR, 0, 0x1340).unwrap();
    // Value landed in register 0 of the fake PHY.
    assert_eq!(b.phy.regs[0], 0x1340);
    // Order witness: DATA first, then the ADDR trigger (write opcode).
    assert_eq!(
        b.write_log,
        vec![
            (MAC_MDIO_DATA, 0x1340),
            (
                MAC_MDIO_ADDR,
                phy_probe::mii_addr_value(PHY_ADDR, 0, GOC_WRITE).unwrap()
            ),
        ]
    );
}

// ---------------------------------------------------------------------------
// Pure classification (5.1 contract item 6, S3 / S9 initial)
// ---------------------------------------------------------------------------

#[test]
fn classify_phy_id_cases() {
    use phy_probe::{classify_phy_id, PhyIdClass};
    assert_eq!(classify_phy_id(0x001c, 0xc916), PhyIdClass::Rtl8211f);
    assert_eq!(classify_phy_id(0, 0), PhyIdClass::Unavailable);
    assert_eq!(classify_phy_id(0xffff, 0xffff), PhyIdClass::Unavailable);
    assert_eq!(classify_phy_id(0x001c, 0xffff), PhyIdClass::Unavailable);
    assert_eq!(classify_phy_id(0xffff, 0xc916), PhyIdClass::Unavailable);
    assert_eq!(classify_phy_id(0x0022, 0x1560), PhyIdClass::Unexpected);
}

#[test]
fn bmcr_bmsr_anomaly_cases() {
    use phy_probe::{classify_bmsr_anomaly, BmsrAnomaly};
    let labels = |bmcr: u16| {
        let (l, c) = phy_probe::bmcr_anomaly_labels(bmcr);
        l[..c].to_vec()
    };
    assert_eq!(labels(0x0000), Vec::<&'static str>::new());
    assert_eq!(labels(0x8400), vec!["reset", "isolate"]);
    assert_eq!(labels(0x4000), vec!["loopback"]);
    assert_eq!(labels(0x0800), vec!["powerdown"]);
    assert_eq!(classify_bmsr_anomaly(0x0000), Some(BmsrAnomaly::AllZero));
    assert_eq!(classify_bmsr_anomaly(0xffff), Some(BmsrAnomaly::AllOne));
    assert_eq!(classify_bmsr_anomaly(0x0020), Some(BmsrAnomaly::NoAnCapable));
    assert_eq!(classify_bmsr_anomaly(0x0028), None);
}

// ---------------------------------------------------------------------------
// Pad mux + GPIO facts (5.1 contract items 2–4, S4)
// ---------------------------------------------------------------------------

#[test]
fn pad_mux_table_and_rmw() {
    assert_eq!(phy_probe::pad_mux_for_pin(20), None);
    for pin in 21..=35 {
        assert_eq!(phy_probe::pad_mux_for_pin(pin), Some(1), "pin {pin}");
    }
    assert_eq!(phy_probe::pad_mux_for_pin(36), None);
    assert_eq!(phy_probe::pad_mux_for_pin(37), Some(0));
    assert_eq!(phy_probe::pad_mux_for_pin(38), None);
    // RMW touches the mux field only.
    assert_eq!(phy_probe::pad_mux_rmw(0x0000_0170, 1), 0x0000_0171);
    assert_eq!(phy_probe::pad_mux_rmw(0x0000_0007, 0), 0x0000_0000);
    assert!(phy_probe::pad_mux_needed(0x0000_0170, 1));
    assert!(!phy_probe::pad_mux_needed(0x0000_0171, 1));
}

#[test]
fn pad_writeback_cases() {
    assert!(phy_probe::pad_writeback_ok(0x0170, 0x0171, 0x0171));
    // mux bits lost
    assert!(!phy_probe::pad_writeback_ok(0x0170, 0x0171, 0x0170));
    // upper bits changed
    assert!(!phy_probe::pad_writeback_ok(0x0170, 0x0171, 0x0371));
}

#[test]
fn run_pad_gpio_happy_minimal_sequence() {
    let mut b = FakeBoard::board();
    let (lines, outcome) = capture(|emit| phy_probe::run_pad_gpio(&mut b, emit));
    assert!(matches!(outcome, phy_probe::PadGpioOutcome::Done));
    assert!(lines.contains(&"[starry-k3] stage=6 pad/gpio begin\n".to_string()));
    // IO-PD voltage domain per the R69 usage: ASAR unlock keys, then exactly
    // one whole-word 1.8 V write — the stage's first three MMIO writes, no
    // access between them, and the write-only domain is never read back.
    assert_eq!(
        &b.write_log[..3],
        &[
            (APBC_ASFAR, 0xbaba),
            (APBC_ASSAR, 0xeb10),
            (IOPD_GMAC1, phy_probe::IOPD_1V8_BIT),
        ]
    );
    assert_eq!(
        b.reads_before_iopd_write,
        Some(0),
        "no MMIO read may precede the io_pd write"
    );
    assert!(
        lines.contains(&"[starry-k3] stage=6 iopd write=0x00000004 ok\n".to_string()),
        "lines: {lines:?}"
    );
    let iopd_window =
        phy_probe::IOPD_BASE_PADDR..phy_probe::IOPD_BASE_PADDR + phy_probe::IOPD_WINDOW_SIZE;
    let apbc_window =
        phy_probe::APBC_BASE_PADDR..phy_probe::APBC_BASE_PADDR + phy_probe::APBC_WINDOW_SIZE;
    assert!(
        !b.read_addr_log
            .iter()
            .any(|a| iopd_window.contains(a) || apbc_window.contains(a)),
        "the voltage domain is written, never read"
    );
    // Pins 21..=35 mux 0→1: written; pin 37 already mux 0: kept (no write).
    let pad_writes: Vec<(u64, u32)> = b
        .write_log
        .iter()
        .copied()
        .filter(|(a, _)| phy_probe::PAD_MFPR_BASE_PADDR <= *a && *a < phy_probe::IOPD_BASE_PADDR)
        .collect();
    assert_eq!(pad_writes.len(), 15, "one write per pin 21..=35 only");
    for (pin, (addr, value)) in (21..=35).zip(pad_writes.iter()) {
        assert_eq!(*addr, phy_probe::PAD_MFPR_BASE_PADDR + pin * 4);
        assert_eq!(*value, 0x1, "old 0 → only the mux field changes to 1");
    }
    // GPIO sequence: exactly one reset, only bit 5, exact order and delays.
    assert_eq!(
        &b.write_log[b.write_log.len() - 4..],
        &[
            (GPIO1_GSDR, 0x20),
            (GPIO1_GPSR, 0x20),
            (GPIO1_GPCR, 0x20),
            (GPIO1_GPSR, 0x20),
        ]
    );
    assert_eq!(b.wait_log, vec![2_000, 20_000_000, 100_000_000]);
    assert!(lines.contains(&"[starry-k3] stage=6 gpio reset begin\n".to_string()));
    assert!(lines.contains(&"[starry-k3] stage=6 gpio reset done\n".to_string()));
    assert!(lines.contains(&"[starry-k3] stage=6 pad/gpio done\n".to_string()));
}

#[test]
fn run_pad_gpio_mux_keep_skips_write() {
    let mut b = FakeBoard::board();
    // Pre-set every pad to its target mux: nothing may be written.
    for pin in 21..=35 {
        b.regs.insert(phy_probe::PAD_MFPR_BASE_PADDR + pin * 4, 0x0171);
    }
    b.regs.insert(phy_probe::PAD_MFPR_BASE_PADDR + 37 * 4, 0x0170);
    let (lines, _) = capture(|emit| phy_probe::run_pad_gpio(&mut b, emit));
    let pad_writes = b
        .write_log
        .iter()
        .filter(|(a, _)| phy_probe::PAD_MFPR_BASE_PADDR <= *a && *a < phy_probe::IOPD_BASE_PADDR)
        .count();
    assert_eq!(pad_writes, 0, "already-correct mux must not be written");
    assert!(
        lines
            .iter()
            .any(|l| l.contains("pin=21 mux=1 before=0x00000171 action=keep")),
        "lines: {lines:?}"
    );
}

#[test]
fn run_pad_gpio_mux_writeback_mismatch_stops_at_pin() {
    let mut b = FakeBoard::board();
    b.corrupt_on_pad_write = true;
    let (lines, outcome) = capture(|emit| phy_probe::run_pad_gpio(&mut b, emit));
    assert!(matches!(outcome, phy_probe::PadGpioOutcome::Stopped(_)));
    assert!(
        lines.iter().any(|l| l.contains(
            "stage=6 pin=21 mux=1 before=0x00000000 written=0x00000001 readback=0x00000000 err=mux-writeback stop"
        )),
        "lines: {lines:?}"
    );
    // Stopped before the GPIO reset: no GPIO writes at all.
    assert!(!b.write_log.iter().any(|(a, _)| *a == GPIO1_GSDR));
    assert!(!lines.iter().any(|l| l.contains("gpio reset begin")));
}

// ---------------------------------------------------------------------------
// stage 7: PHY identity and status (5.1 contract item 6, S1–S3, S9 initial)
// ---------------------------------------------------------------------------

fn good_phy() -> FakeBoard {
    let mut b = FakeBoard::board();
    b.phy.regs[0] = 0x1140; // BMCR: AN enable, 100M full duplex — no anomaly bits
    b.phy.regs[1] = 0x0028; // BMSR: AN capable, AN complete; link down (no cable)
    b.phy.regs[2] = 0x001c;
    b.phy.regs[3] = 0xc916;
    b
}

#[test]
fn run_phy_identity_happy_reports_rtl8211f_and_status() {
    let mut b = good_phy();
    let (lines, status) = capture(|emit| phy_probe::run_phy_identity(&mut b, emit));
    assert!(matches!(status, phy_probe::PhyStatus::Ok));
    assert!(lines.contains(&"[starry-k3] stage=7 mdio/phy begin\n".to_string()));
    assert!(
        lines.contains(
            &"[starry-k3] stage=7 phy id1=0x001c id2=0xc916 phyid=0x001cc916 result=rtl8211f\n"
                .to_string()
        ),
        "lines: {lines:?}"
    );
    assert!(
        lines.contains(
            &"[starry-k3] stage=7 phy bmcr=0x1140 bmsr=0x0028 link=down an-capable=1 an-complete=1\n"
                .to_string()
        ),
        "lines: {lines:?}"
    );
    assert!(lines.contains(&"[starry-k3] stage=7 mdio/phy done\n".to_string()));
    // Read order: PHYID1, PHYID2, then BMCR and BMSR.
    assert_eq!(b.read_reg_log, vec![2, 3, 0, 1]);
}

#[test]
fn run_phy_identity_unavailable_stops_before_status() {
    let mut b = good_phy();
    b.phy.regs[2] = 0xffff; // ID word 0xffff → unavailable
    let (lines, status) = capture(|emit| phy_probe::run_phy_identity(&mut b, emit));
    assert!(matches!(status, phy_probe::PhyStatus::Stopped(_)));
    assert!(
        lines.iter().any(|l| l.contains("phyid=0xffffc916 result=unavailable")),
        "lines: {lines:?}"
    );
    // No BMCR/BMSR access after a failed identity.
    assert_eq!(b.read_reg_log, vec![2, 3]);
}

#[test]
fn run_phy_identity_unexpected_stops_without_config() {
    let mut b = good_phy();
    b.phy.regs[2] = 0x0022;
    b.phy.regs[3] = 0x1560;
    let (lines, status) = capture(|emit| phy_probe::run_phy_identity(&mut b, emit));
    assert!(matches!(status, phy_probe::PhyStatus::Stopped(_)));
    assert!(
        lines.iter().any(|l| l.contains("phyid=0x00221560 result=unexpected")),
        "lines: {lines:?}"
    );
    assert_eq!(b.read_reg_log, vec![2, 3], "no address scan, no status access");
}

#[test]
fn run_phy_identity_bmcr_anomaly_stops() {
    let mut b = good_phy();
    b.phy.regs[0] = 0x0840; // powerdown set
    let (lines, status) = capture(|emit| phy_probe::run_phy_identity(&mut b, emit));
    assert!(matches!(status, phy_probe::PhyStatus::Stopped(_)));
    assert!(
        lines.iter().any(|l| l.contains("err=bmcr-anomaly(powerdown) stop")),
        "lines: {lines:?}"
    );
}

#[test]
fn run_phy_identity_bmsr_anomaly_stops() {
    let mut b = good_phy();
    b.phy.regs[1] = 0x0000; // all-zero
    let (lines, status) = capture(|emit| phy_probe::run_phy_identity(&mut b, emit));
    assert!(matches!(status, phy_probe::PhyStatus::Stopped(_)));
    assert!(
        lines.iter().any(|l| l.contains("bmsr=0x0000 err=all-zero stop")),
        "lines: {lines:?}"
    );
}

#[test]
fn run_phy_identity_mdio_timeout_stops() {
    let mut b = good_phy();
    b.mdio_busy_stuck = true;
    let (lines, status) = capture(|emit| phy_probe::run_phy_identity(&mut b, emit));
    assert!(matches!(status, phy_probe::PhyStatus::Stopped(_)));
    assert!(
        lines
            .iter()
            .any(|l| l.contains("stage=7 mdio timeout phase=idle reg=2 addr=0x0022050d stop")),
        "lines: {lines:?}"
    );
}


// ---------------------------------------------------------------------------
// stage 8: AN restart + bounded link observation (5.2, S5–S9)
// ---------------------------------------------------------------------------

/// A board past stage 7: good identity, AN-resolvable, no cable yet.
fn observation_board() -> FakeBoard {
    let mut b = good_phy();
    b.access_cost_ns = 0; // window-count tests are driven purely by waits
    b
}

fn sample_states(lines: &[String]) -> Vec<&str> {
    lines
        .iter()
        .filter(|l| l.contains("stage=8 sample"))
        .map(|l| {
            let idx = l.find("state=").unwrap();
            l[idx + "state=".len()..].trim_end().split(' ').next().unwrap()
        })
        .collect()
}

#[test]
fn sample_state_matrix() {
    use phy_probe::{LinkState, Sample};
    // restart bit still set → negotiating even with link bits up
    let s = Sample { bmcr: 0x1340, bmsr1: 0x002c, bmsr2: 0x002c };
    assert_eq!(s.state(), LinkState::Negotiating);
    // AN incomplete → negotiating
    let s = Sample { bmcr: 0x1140, bmsr1: 0x0008, bmsr2: 0x0008 };
    assert_eq!(s.state(), LinkState::Negotiating);
    // remote fault is a definite non-up observation
    let s = Sample { bmcr: 0x1140, bmsr1: 0x0030, bmsr2: 0x0030 };
    assert_eq!(s.state(), LinkState::RemoteFault);
    // link up
    let s = Sample { bmcr: 0x1140, bmsr1: 0x002c, bmsr2: 0x002c };
    assert_eq!(s.state(), LinkState::Up);
    assert!(!s.latched_down_cleared());
    // link down
    let s = Sample { bmcr: 0x1140, bmsr1: 0x0028, bmsr2: 0x0028 };
    assert_eq!(s.state(), LinkState::Down);
    // latch-low: first read down, second read up
    let s = Sample { bmcr: 0x1140, bmsr1: 0x0028, bmsr2: 0x002c };
    assert_eq!(s.state(), LinkState::Up);
    assert!(s.latched_down_cleared());
    // a first-read drop is not a persistent current down
    let s = Sample { bmcr: 0x1140, bmsr1: 0x0028, bmsr2: 0x002c };
    assert!(s.current_link());
}

#[test]
fn run_link_observation_two_plug_cycles_within_window() {
    let mut b = observation_board();
    // Plug schedule keyed on BMSR read indices (2 per sample): up at sample 4,
    // down at 7, up at 10, down at 13, up at 16 — two full up→down→up cycles.
    b.phy.peer_schedule = vec![(7, true), (13, false), (19, true), (25, false), (31, true)];
    let (lines, _) = capture(|emit| phy_probe::run_link_observation(&mut b, emit));
    assert_eq!(
        sample_states(&lines),
        vec!["negotiating", "up", "down", "up", "down", "up"],
        "lines: {lines:?}"
    );
    // Every insertion (three ups across the two cycles) clears its latched
    // drop and is reported as such, never as a persistent down.
    assert_eq!(
        lines.iter().filter(|l| l.contains(" latch-cleared")).count(),
        3
    );
    // Ended with up observed; the window was not extended by the plug cycles.
    assert!(lines
        .iter()
        .any(|l| l.contains("stage=8 observation-ended last=up up-observed=yes")));
    assert_eq!(
        b.bmsr_reads,
        2 * (phy_probe::LINK_WINDOW_NANOS / phy_probe::LINK_SAMPLE_INTERVAL_NANOS) as usize,
        "exactly one BMSR pair per sample slot (fixed window)"
    );
    // Exactly one BMCR write (the AN restart): the only MDIO DATA write in
    // the whole window, followed by the BMCR write trigger, preserving all
    // other bits (0x1140 → 0x1340). Read triggers write ADDR only.
    let data_writes: Vec<usize> = b
        .write_log
        .iter()
        .enumerate()
        .filter_map(|(i, (a, _))| (*a == MAC_MDIO_DATA).then_some(i))
        .collect();
    assert_eq!(data_writes.len(), 1, "only the AN restart writes MDIO DATA");
    let i = data_writes[0];
    assert_eq!(b.write_log[i].1, 0x1340);
    assert_eq!(
        b.write_log[i + 1],
        (
            MAC_MDIO_ADDR,
            phy_probe::mii_addr_value(PHY_ADDR, phy_probe::BMCR_REG, GOC_WRITE).unwrap()
        )
    );
    // Advertisement registers read, never written.
    assert!(b.read_reg_log.contains(&phy_probe::ANA_REG));
    assert!(b.read_reg_log.contains(&phy_probe::GBCR_REG));
}

#[test]
fn run_link_observation_no_peer_reports_no_link() {
    let mut b = observation_board();
    let (lines, _) = capture(|emit| phy_probe::run_link_observation(&mut b, emit));
    assert_eq!(sample_states(&lines), vec!["negotiating", "down"]);
    assert!(lines
        .iter()
        .any(|l| l.contains("stage=8 observation-ended last=down up-observed=no no-link-observed")),
        "lines: {lines:?}");
    assert_eq!(
        b.bmsr_reads,
        2 * (phy_probe::LINK_WINDOW_NANOS / phy_probe::LINK_SAMPLE_INTERVAL_NANOS) as usize
    );
}

#[test]
fn run_link_observation_latch_low_reported_cleared() {
    let mut b = observation_board();
    b.negotiate_delay_ns = 0; // AN resolves before the first sample
    // Peer plugged before the first sample: the first BMSR read carries the
    // latched drop (link=0), the second is current (link=1).
    b.phy.peer_schedule = vec![(1, true)];
    let (lines, _) = capture(|emit| phy_probe::run_link_observation(&mut b, emit));
    assert_eq!(sample_states(&lines), vec!["up"]);
    assert!(lines.iter().any(|l| l.contains(
        "stage=8 sample bmcr=0x1140 bmsr1=0x0028 bmsr2=0x002c state=up latch-cleared"
    )), "lines: {lines:?}");
}

#[test]
fn run_link_observation_an_restart_not_cleared_stays_negotiating() {
    let mut b = observation_board();
    b.negotiate_delay_ns = u64::MAX / 2; // AN never resolves in the window
    let (lines, _) = capture(|emit| phy_probe::run_link_observation(&mut b, emit));
    assert_eq!(sample_states(&lines), vec!["negotiating"]);
    assert!(lines
        .iter()
        .any(|l| l.contains("observation-ended last=negotiating up-observed=no no-link-observed")));
}

#[test]
fn run_link_observation_mdio_error_exits_immediately() {
    let mut b = observation_board();
    // A stuck bus only times out when the fake clock advances per access.
    b.access_cost_ns = 1_000_000;
    b.phy.peer_schedule = vec![(7, true)];
    b.stuck_after_bmsr_reads = Some(30); // bus sticks busy at BMSR read #30
    let (lines, _) = capture(|emit| phy_probe::run_link_observation(&mut b, emit));
    assert!(
        lines.iter().any(|l| l.contains("stage=8 mdio timeout phase=idle reg=0 addr=0x")),
        "lines: {lines:?}"
    );
    assert!(lines.iter().any(|l| l.contains("observation-ended error=mdio-timeout")));
    assert!(
        b.bmsr_reads < 2 * (phy_probe::LINK_WINDOW_NANOS / phy_probe::LINK_SAMPLE_INTERVAL_NANOS) as usize,
        "observation must stop at the error, not run on"
    );
}

#[test]
fn run_link_observation_invalid_bmsr_stops() {
    let mut b = observation_board();
    b.force_bmsr_at = Some((20, 0xffff));
    let (lines, _) = capture(|emit| phy_probe::run_link_observation(&mut b, emit));
    assert!(
        lines.iter().any(|l| l.contains("stage=8 phy bmsr=0xffff err=all-one stop")),
        "lines: {lines:?}"
    );
    assert!(lines.iter().any(|l| l.contains("observation-ended error=invalid-bmsr")));
    assert!(
        b.bmsr_reads < 2 * (phy_probe::LINK_WINDOW_NANOS / phy_probe::LINK_SAMPLE_INTERVAL_NANOS) as usize
    );
}

#[test]
fn run_link_observation_remote_fault_is_not_up_and_continues() {
    let mut b = observation_board();
    b.negotiate_delay_ns = 0;
    b.force_bmsr_at = Some((1, 0x0038)); // AN capable + AN complete + remote fault, no link
    let (lines, _) = capture(|emit| phy_probe::run_link_observation(&mut b, emit));
    assert_eq!(sample_states(&lines), vec!["remote-fault"]);
    // Remote fault is a status, not an access error: the window runs to the end.
    assert!(lines
        .iter()
        .any(|l| l.contains("observation-ended last=remote-fault up-observed=no no-link-observed")),
        "lines: {lines:?}");
    assert_eq!(
        b.bmsr_reads,
        2 * (phy_probe::LINK_WINDOW_NANOS / phy_probe::LINK_SAMPLE_INTERVAL_NANOS) as usize
    );
}

#[test]
fn run_link_observation_bmcr_anomaly_midwindow_stops() {
    let mut b = observation_board();
    b.phy.peer_schedule = vec![(7, true)];
    // Isolate appears mid-window (from BMSR read #20 on): the observed control
    // state is anomalous and must stop the observation.
    b.bmcr_override_from = Some((20, 0x0440));
    let (lines, _) = capture(|emit| phy_probe::run_link_observation(&mut b, emit));
    assert!(
        lines
            .iter()
            .any(|l| l.contains("stage=8 phy bmcr=0x0440 err=bmcr-anomaly(isolate) stop")),
        "lines: {lines:?}"
    );
    assert!(lines.iter().any(|l| l.contains("observation-ended error=bmcr-anomaly")));
    assert!(
        b.bmsr_reads < 2 * (phy_probe::LINK_WINDOW_NANOS / phy_probe::LINK_SAMPLE_INTERVAL_NANOS) as usize,
        "observation must stop at the anomaly"
    );
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Runs `f` against a capturing emitter, returning the emitted lines and `f`'s
/// own return value.
fn capture<R>(f: impl FnOnce(&mut dyn FnMut(&str)) -> R) -> (Vec<String>, R) {
    let mut lines = Vec::new();
    let r = f(&mut |line: &str| lines.push(line.to_string()));
    (lines, r)
}
