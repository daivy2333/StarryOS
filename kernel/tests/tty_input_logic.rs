//! MS17 Iteration 002 (T5) native tty canonical-input contract harness.
//!
//! Drives the production canonical module directly; the fakes replace only
//! the byte source and the delivery queue boundary (Task Contract T5):
//! space/tab retention, ERASE/KILL editing, the 1024 B line bound with a
//! whole-line discard plus one `InvalidInput`, the `VEOF` 0-read event,
//! segmented delivery across a wrapping bounded queue (backpressure and
//! wrap), and reader-event persistency across waiter cancellation.
//!
//! RED = missing module or failing assertions. Command:
//! `rustc --edition=2024 --test kernel/tests/tty_input_logic.rs -o
//! /tmp/ms17-tty-input-test && /tmp/ms17-tty-input-test`

extern crate alloc;

#[path = "../src/pseudofs/dev/tty/terminal/canonical.rs"]
mod canonical;

use alloc::vec::Vec;

use canonical::{
    CanonicalEvents, CanonicalInput, CanonicalSpec, LineSink, MAX_LINE_LEN, POLL_INPUT_BUDGET,
    ReadOutcome, readable, resolve_read,
};

/// Linux defaults: ERASE=DEL, KILL=^U, VEOF=^D, VEOL/VEOL2 disabled (NUL
/// with IEXTEN set — the same NUL-matches-EOL behavior the ldisc had).
fn spec() -> CanonicalSpec {
    CanonicalSpec {
        erase: 0x7f,
        kill: 0x15,
        eof: 0x04,
        eol: 0,
        eol2: 0,
        iexten: true,
        echok: true,
    }
}

/// Fixed-capacity wrapping byte ring standing in for the ldisc's SPSC
/// queue: `push` accepts only up to the free capacity, so a line longer
/// than the capacity is delivered in segments across the wrap boundary.
struct FakeRing {
    buf: Vec<u8>,
    head: usize,
    len: usize,
}

impl FakeRing {
    fn new(cap: usize) -> Self {
        Self {
            buf: alloc::vec![0u8; cap],
            head: 0,
            len: 0,
        }
    }

    fn pop(&mut self, out: &mut [u8]) -> usize {
        let n = out.len().min(self.len);
        for (i, b) in out.iter_mut().take(n).enumerate() {
            *b = self.buf[(self.head + i) % self.buf.len()];
        }
        self.head = (self.head + n) % self.buf.len();
        self.len -= n;
        n
    }

    fn drain_all(&mut self) -> Vec<u8> {
        let mut out = alloc::vec![0u8; self.len];
        self.pop(&mut out);
        out
    }
}

impl LineSink for FakeRing {
    fn push(&mut self, data: &[u8]) -> usize {
        let ring_len = self.buf.len();
        let free = ring_len - self.len;
        let n = free.min(data.len());
        for (i, &b) in data.iter().take(n).enumerate() {
            self.buf[(self.head + self.len + i) % ring_len] = b;
        }
        self.len += n;
        n
    }
}

/// Unbounded sink: everything offered is accepted in one call.
struct OpenSink(Vec<u8>);

impl LineSink for OpenSink {
    fn push(&mut self, data: &[u8]) -> usize {
        self.0.extend_from_slice(data);
        data.len()
    }
}

fn feed(canon: &mut CanonicalInput, events: &CanonicalEvents, bytes: &[u8]) {
    // Production poll-loop discipline: new input is only classified after
    // the previous line finished delivering (single line in flight).
    assert!(
        !canon.has_pending_delivery(),
        "feed() while a line is still delivering violates the poll discipline"
    );
    let s = spec();
    for &b in bytes {
        canon.on_char(&s, b, events);
    }
}

#[test]
fn space_and_tab_are_kept_in_canonical_lines() {
    let mut canon = CanonicalInput::new();
    let events = CanonicalEvents::new();
    feed(&mut canon, &events, b"ab c\td\nef\n");

    let mut sink = OpenSink(Vec::new());
    assert!(canon.flush_pending(&mut sink));
    assert_eq!(sink.0, b"ab c\td\nef\n");
    assert!(!events.any_pending());
}

#[test]
fn erase_and_kill_edit_the_line_before_the_terminator() {
    let mut canon = CanonicalInput::new();
    let events = CanonicalEvents::new();
    let s = spec();

    feed(&mut canon, &events, b"abc");
    canon.on_char(&s, 0x7f, &events); // ERASE drops 'c'
    feed(&mut canon, &events, b"d\n");
    let mut sink = OpenSink(Vec::new());
    canon.flush_pending(&mut sink);
    assert_eq!(sink.0, b"abd\n");

    feed(&mut canon, &events, b"xyz");
    canon.on_char(&s, 0x15, &events); // KILL clears the line
    feed(&mut canon, &events, b"q\n");
    let mut sink = OpenSink(Vec::new());
    canon.flush_pending(&mut sink);
    assert_eq!(sink.0, b"q\n");
}

#[test]
fn veof_on_empty_line_yields_single_zero_read_event() {
    let mut canon = CanonicalInput::new();
    let events = CanonicalEvents::new();
    feed(&mut canon, &events, &[0x04]);

    // The event is readable state, not queued data: an empty queue is
    // still readable until the event is consumed once.
    assert!(readable(true, &events));
    assert_eq!(resolve_read(0, &events), ReadOutcome::Eof);
    assert_eq!(resolve_read(0, &events), ReadOutcome::WouldBlock);
    assert!(!readable(true, &events));

    // The EOF byte itself never reaches the queue.
    let mut sink = OpenSink(Vec::new());
    canon.flush_pending(&mut sink);
    assert!(sink.0.is_empty());
}

#[test]
fn queued_data_is_read_before_the_eof_event() {
    let mut canon = CanonicalInput::new();
    let events = CanonicalEvents::new();
    feed(&mut canon, &events, b"hi\n");
    let mut ring = FakeRing::new(16);
    canon.flush_pending(&mut ring);
    // The VEOF is classified only after the previous line delivered, so
    // it latches on an empty accumulation line while the queue still
    // holds the older data.
    feed(&mut canon, &events, &[0x04]);

    let mut buf = [0u8; 16];
    let n = ring.pop(&mut buf);
    assert_eq!(resolve_read(n, &events), ReadOutcome::Data);
    assert_eq!(&buf[..n], b"hi\n");

    // Queue drained now: the latched EOF resolves as the 0-read event.
    assert!(readable(true, &events));
    assert_eq!(resolve_read(0, &events), ReadOutcome::Eof);
    assert_eq!(resolve_read(0, &events), ReadOutcome::WouldBlock);
}

#[test]
fn max_length_line_is_delivered_and_one_extra_byte_discards_whole() {
    let mut canon = CanonicalInput::new();
    let events = CanonicalEvents::new();

    // Exactly MAX_LINE_LEN content bytes plus the EOL terminator are a
    // legal line.
    feed(&mut canon, &events, &[b'x'; MAX_LINE_LEN]);
    feed(&mut canon, &events, b"\n");
    let mut sink = OpenSink(Vec::new());
    canon.flush_pending(&mut sink);
    assert_eq!(sink.0.len(), MAX_LINE_LEN + 1);
    assert!(!events.any_pending());

    // One more content byte turns the line overlong: the whole line is
    // discarded, exactly one InvalidInput is latched, nothing is queued.
    feed(&mut canon, &events, &[b'y'; MAX_LINE_LEN + 1]);
    feed(&mut canon, &events, b"\n");
    let mut sink = OpenSink(Vec::new());
    canon.flush_pending(&mut sink);
    assert!(sink.0.is_empty());
    assert!(readable(true, &events));
    assert_eq!(resolve_read(0, &events), ReadOutcome::InvalidInput);
    assert_eq!(resolve_read(0, &events), ReadOutcome::WouldBlock);
}

#[test]
fn overlong_line_recovers_for_the_next_line() {
    let mut canon = CanonicalInput::new();
    let events = CanonicalEvents::new();

    feed(&mut canon, &events, &[b'a'; 1100]);
    feed(&mut canon, &events, b"\n");
    feed(&mut canon, &events, b"ok\n");

    // The overlong line produced one InvalidInput; the following line is
    // delivered normally afterwards.
    assert_eq!(resolve_read(0, &events), ReadOutcome::InvalidInput);
    let mut sink = OpenSink(Vec::new());
    assert!(canon.flush_pending(&mut sink));
    assert_eq!(sink.0, b"ok\n");
}

#[test]
fn line_delivery_segments_across_the_queue_wrap_without_loss() {
    let mut canon = CanonicalInput::new();
    let events = CanonicalEvents::new();
    // A 20-byte line into an 8-byte wrapping queue: delivery must resume
    // across the wrap boundary and the backpressure pauses, byte-exact.
    let line: Vec<u8> = (b'0'..b'0' + 20).chain([b'\n']).collect();
    feed(&mut canon, &events, &line);

    let mut ring = FakeRing::new(8);
    let mut received: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 5];
    // Simulate the poll loop: flush while it makes progress; on a full
    // queue the reader side drains a few bytes and the next flush resumes.
    while canon.has_pending_delivery() {
        if !canon.flush_pending(&mut ring) {
            let n = ring.pop(&mut chunk);
            assert!(n > 0, "stalled with pending delivery and an empty queue");
            received.extend_from_slice(&chunk[..n]);
        }
        received.extend_from_slice(&ring.drain_all());
    }
    received.extend_from_slice(&ring.drain_all());
    assert_eq!(received, line);
    assert!(!events.any_pending());
}

#[test]
fn backpressure_pauses_delivery_until_the_reader_drains() {
    let mut canon = CanonicalInput::new();
    let events = CanonicalEvents::new();
    feed(&mut canon, &events, b"abcdefg\n");

    let mut ring = FakeRing::new(4);
    assert!(canon.flush_pending(&mut ring)); // "abcd"
    assert!(!canon.flush_pending(&mut ring)); // queue full, nothing accepted
    assert!(canon.has_pending_delivery());

    let mut buf = [0u8; 2];
    assert_eq!(ring.pop(&mut buf), 2); // reader consumes "ab"
    assert!(canon.flush_pending(&mut ring)); // "ef" fits, "g\n" still pending
    assert!(canon.has_pending_delivery());
    assert_eq!(ring.drain_all(), b"cdef");
    assert!(canon.flush_pending(&mut ring)); // queue empty again: "g\n"
    assert!(!canon.has_pending_delivery());
    assert_eq!(ring.drain_all(), b"g\n");
}

#[test]
fn the_per_wake_input_budget_is_pinned_to_the_contract_value() {
    // T5 ⑤ pins the tty-reader per-wake budget at 256 input bytes.
    assert_eq!(POLL_INPUT_BUDGET, 256);
}

#[test]
fn cancellation_keeps_pending_events_for_the_next_reader() {
    let mut canon = CanonicalInput::new();
    let events = CanonicalEvents::new();

    // Reader A observes readiness and is cancelled without consuming:
    // the event must stay latched for reader B.
    feed(&mut canon, &events, &[0x04]);
    assert!(readable(true, &events)); // A would wake here, then drop
    assert!(readable(true, &events)); // B still sees the same event
    assert_eq!(resolve_read(0, &events), ReadOutcome::Eof);
    assert!(!readable(true, &events));

    // The same holds for the overflow event of a discarded line.
    feed(&mut canon, &events, &[b'z'; MAX_LINE_LEN + 2]);
    feed(&mut canon, &events, b"\n");
    assert!(readable(true, &events));
    assert!(readable(true, &events));
    assert_eq!(resolve_read(0, &events), ReadOutcome::InvalidInput);
}

#[test]
fn clear_resets_pending_delivery_and_discard_state() {
    let mut canon = CanonicalInput::new();
    let events = CanonicalEvents::new();

    feed(&mut canon, &events, b"partial");
    feed(&mut canon, &events, &[b'w'; MAX_LINE_LEN + 1]);
    assert_eq!(resolve_read(0, &events), ReadOutcome::InvalidInput);

    canon.clear();
    events.clear();

    // After the flush the next line starts fresh and delivers normally.
    feed(&mut canon, &events, b"fresh\n");
    let mut sink = OpenSink(Vec::new());
    assert!(canon.flush_pending(&mut sink));
    assert_eq!(sink.0, b"fresh\n");
    assert!(!events.any_pending());
}
