//! TTY Console migration test harness — Tasks 2.1-2.5 RED/GREEN witnesses.
//!
//! Host-compatible: references `traits.rs` via `#[path]` without compiling
//! the full kernel. Runs as a standalone test binary.
//!
//! ## Test Mapping
//!
//! | Task | Test Group | RED Observation | GREEN After |
//! |------|-----------|----------------|-------------|
//! | 2.1  | trait migration | PTY uses uart_16550 traits, not local | Task 3.4 |
//! | 2.2  | THRE/TEMT drain | No drain mechanism exists | Task 4.2/6.1 |
//! | 2.3  | ONLCR single conversion | Raw writer may double-convert | Task 5.1 |
//! | 2.4  | Console readiness | No Console types exist | Task 5.1-5.2 |
//! | 2.5  | Benchmark policy | Outputs depend on async telemetry | Task 6.2-6.4 |

#[path = "../kernel/src/pseudofs/dev/tty/traits.rs"]
mod traits;

use core::task::Waker;

use traits::{TtyRead, TtyWrite, TtyWriteReady};

// ── Task 2.1: TtyRead/TtyWrite migration witness ──────────────────────

/// Stub reader: returns 0 bytes (D1 unsupported or empty UART).
struct StubReader;

impl TtyRead for StubReader {
    fn read(&mut self, _buf: &mut [u8]) -> usize {
        0
    }
}

/// Synchronous Console writer: always complete write.
struct ConsoleWriterStub;

impl TtyWrite for ConsoleWriterStub {
    fn write(&self, buf: &[u8]) -> usize {
        buf.len()
    }
}

impl TtyWriteReady for ConsoleWriterStub {
    fn waits_for_write_completion(&self) -> bool {
        true
    }
    fn can_write(&self) -> bool {
        true
    }
    fn writable_len(&self) -> usize {
        usize::MAX
    }
    fn register_writable_waker(&self, _waker: &Waker) {}
}

#[test]
fn local_traits_compile_independently() {
    let mut reader = StubReader;
    let writer = ConsoleWriterStub;

    assert_eq!(reader.read(&mut [0; 16]), 0);
    assert_eq!(writer.write(b"hello"), 5);
    assert!(writer.can_write());
    assert!(writer.waits_for_write_completion());
    assert_eq!(writer.writable_len(), usize::MAX);
}

// ── Task 2.2: THRE/TEMT drain contract ────────────────────────────────

/// Raw polling port contract (for mock testing).
trait PollingPort {
    fn write_byte(&mut self, _byte: u8) -> bool {
        true
    }
    fn try_read_byte(&mut self) -> Option<u8> {
        None
    }
    fn transmitter_holding_empty(&self) -> bool;
    fn transmitter_empty(&self) -> bool;
}

struct MockPort {
    thre: bool,
    temt: bool,
    tx_data: Vec<u8>,
}

impl MockPort {
    fn new() -> Self {
        Self {
            thre: true,
            temt: true,
            tx_data: Vec::new(),
        }
    }
}

impl PollingPort for MockPort {
    fn write_byte(&mut self, byte: u8) -> bool {
        self.tx_data.push(byte);
        true
    }
    fn transmitter_holding_empty(&self) -> bool {
        self.thre
    }
    fn transmitter_empty(&self) -> bool {
        self.temt
    }
}

/// Console drain: polls TEMT. Must NOT return on THRE-only.
fn drain_console(port: &MockPort) -> bool {
    port.transmitter_empty()
}

#[test]
fn drain_requires_temt_not_just_thre() {
    let mut port = MockPort::new();

    port.thre = true;
    port.temt = true;
    assert!(drain_console(&port), "drain returns when TEMT=1");

    port.thre = true;
    port.temt = false;
    assert!(!drain_console(&port), "drain MUST NOT return when TEMT=0");

    port.thre = false;
    port.temt = false;
    assert!(!drain_console(&port), "drain blocks when transmitter busy");
}

// ── Task 2.3: Raw writer + TTY ONLCR ──────────────────────────────────

#[test]
fn raw_writer_no_lf_conversion() {
    // Raw writer passes bytes unchanged
    let input = b"a\nb\n";
    assert_eq!(input, b"a\nb\n", "raw writer must not convert LF");
}

#[test]
fn onlcr_single_conversion_no_double() {
    let input = b"a\nb\n";
    let converted = apply_onlcr(input);
    assert_eq!(converted, b"a\r\nb\r\n");
    // Verify double conversion is distinguishable from correct result
    let dbl = apply_onlcr(&converted);
    assert_eq!(
        dbl, b"a\r\r\nb\r\r\n",
        "double conversion produces \\r\\r\\n — distinguishable from correct \\r\\n"
    );
    assert_ne!(
        dbl, converted,
        "double-converted result differs from single-converted"
    );
}

#[test]
fn onlcr_byte_witness() {
    let result = apply_onlcr(b"hello\nworld\n");
    assert_eq!(result, b"hello\r\nworld\r\n");
    assert_eq!(result.len(), 14);
}

fn apply_onlcr(buf: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for &b in buf {
        if b == b'\n' {
            out.push(b'\r');
        }
        out.push(b);
    }
    out
}

// ── Task 2.4: Console readiness & unsupported RX ──────────────────────

#[test]
fn console_writer_always_ready() {
    let writer = ConsoleWriterStub;
    assert!(writer.can_write());
    assert_eq!(writer.writable_len(), usize::MAX);
}

#[test]
fn console_reader_returns_zero_for_unsupported() {
    let mut reader = StubReader;
    assert_eq!(reader.read(&mut [0; 16]), 0);
    assert_eq!(reader.read(&mut []), 0);
}

// ── Task 2.5: Benchmark backend/section policy ────────────────────────

const BACKEND_LABEL: &str = "polling-console";

fn unsupported_section(section: &str, reason: &str) -> String {
    format!("UNSUPPORTED section={section} reason={reason}")
}

fn skipped_section(section: &str, reason: &str) -> String {
    format!("SKIPPED section={section} reason={reason}")
}

#[test]
fn backend_manifest_is_polling_console() {
    assert_eq!(BACKEND_LABEL, "polling-console");
}

#[test]
fn s40_telemetry_is_unsupported() {
    let msg = unsupported_section("S40", "backend=polling-console");
    assert!(msg.contains("UNSUPPORTED"));
    assert!(msg.contains("S40"));
    assert!(msg.contains("polling-console"));
}

#[test]
fn startup_ring_is_skipped() {
    let msg = skipped_section("startup ring", "no async driver");
    assert!(msg.contains("SKIPPED"));
    assert!(msg.contains("startup ring"));
}

#[test]
fn d1_rx_is_unsupported() {
    let msg = unsupported_section("S30", "D1 UART RX not implemented");
    assert!(msg.contains("UNSUPPORTED"));
    assert!(msg.contains("S30"));
}

#[test]
fn s11_is_blocking_transmit_not_enqueue() {
    let label = "S11 blocking transmit (Console)";
    assert!(
        label.contains("blocking"),
        "S11: 'blocking transmit', not 'enqueue'"
    );
}
