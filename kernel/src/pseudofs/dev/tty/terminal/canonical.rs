//! Canonical-mode input processing for the async tty line discipline.
//!
//! Production state machine extracted from `ldisc::InputReader` (MS17
//! Iteration 002, task T5). Fixes over the previous in-ldisc logic: space
//! and tab are kept in the line, canonical lines are bounded at
//! [`MAX_LINE_LEN`] with an overlong line discarded whole for exactly one
//! `InvalidInput` read, and `VEOF` on an empty line produces a single
//! 0-read event. Readiness and read resolution share the same event
//! latches ([`CanonicalEvents`]), so poll never disagrees with the next
//! `read`, and a cancelled waiter cannot consume an event it never saw.
//!
//! The module depends only on `core`/`alloc` so the native
//! `kernel/tests/tty_input_logic.rs` harness drives it directly; that
//! harness's fakes replace only the byte source and the delivery queue
//! boundary ([`LineSink`]). CR/LF translation, ISIG and echo stay in the
//! ldisc (they need the writer and job control).

use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};

/// Maximum canonical line length in content bytes; the EOL terminator is
/// not counted. A 1025th content byte makes the line overlong.
pub const MAX_LINE_LEN: usize = 1024;

/// Per-wake input budget for the tty-reader task (T5 ⑤): one task wake
/// processes at most this many input bytes before re-queuing itself, so a
/// burst cannot monopolize the task.
pub const POLL_INPUT_BUDGET: usize = 256;

/// Termios-derived classification inputs for [`CanonicalInput::on_char`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CanonicalSpec {
    pub erase: u8,
    pub kill: u8,
    pub eof: u8,
    pub eol: u8,
    pub eol2: u8,
    pub iexten: bool,
    pub echok: bool,
}

impl CanonicalSpec {
    pub fn is_eol(&self, ch: u8) -> bool {
        ch == b'\n' || ch == self.eol || (self.iexten && ch == self.eol2)
    }

    pub fn is_eof(&self, ch: u8) -> bool {
        ch == self.eof
    }

    pub fn is_erase(&self, ch: u8) -> bool {
        ch == self.erase
    }

    pub fn is_kill(&self, ch: u8) -> bool {
        self.echok && ch == self.kill
    }
}

/// Queue boundary between input processing and the reader side. `push`
/// accepts a prefix (possibly empty) of `data` and returns its length;
/// that prefix accept is the backpressure surface.
pub trait LineSink {
    fn push(&mut self, data: &[u8]) -> usize;
}

/// Reader-visible events latched by canonical input. The tty-reader task
/// (producer) and the read/poll side (consumer) share one instance, so an
/// event survives waiter cancellation and is delivered exactly once.
#[derive(Default)]
pub struct CanonicalEvents {
    eof: AtomicBool,
    overflow: AtomicBool,
}

impl CanonicalEvents {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_eof(&self) {
        self.eof.store(true, Ordering::Release);
    }

    pub fn set_overflow(&self) {
        self.overflow.store(true, Ordering::Release);
    }

    pub fn take_eof(&self) -> bool {
        self.eof.swap(false, Ordering::AcqRel)
    }

    pub fn take_overflow(&self) -> bool {
        self.overflow.swap(false, Ordering::AcqRel)
    }

    pub fn any_pending(&self) -> bool {
        self.eof.load(Ordering::Acquire) || self.overflow.load(Ordering::Acquire)
    }

    pub fn clear(&self) {
        self.eof.store(false, Ordering::Relaxed);
        self.overflow.store(false, Ordering::Relaxed);
    }
}

/// Resolution of one read attempt against popped data and pending events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadOutcome {
    /// Queued data was popped (the caller holds the bytes).
    Data,
    /// Latched `VEOF`-on-empty-line event: the read returns `Ok(0)` once.
    Eof,
    /// Latched overlong-line discard: the read returns `InvalidInput` once.
    InvalidInput,
    /// No data and no latched event: the read stays blocked.
    WouldBlock,
}

/// Resolve one read: queued data wins over latched events, and each event
/// is consumed exactly when it resolves the read.
pub fn resolve_read(popped: usize, events: &CanonicalEvents) -> ReadOutcome {
    if popped > 0 {
        return ReadOutcome::Data;
    }
    if events.take_overflow() {
        return ReadOutcome::InvalidInput;
    }
    if events.take_eof() {
        return ReadOutcome::Eof;
    }
    ReadOutcome::WouldBlock
}

/// Readiness: data in the queue or any latched event makes the tty
/// readable, so poll agrees with what the next read resolves.
pub fn readable(ring_empty: bool, events: &CanonicalEvents) -> bool {
    !ring_empty || events.any_pending()
}

/// Canonical input state machine: line accumulation, ERASE/KILL editing,
/// EOL/VEOF termination, the line bound and pending-line delivery across
/// queue backpressure.
#[derive(Default)]
pub struct CanonicalInput {
    line: Vec<u8>,
    /// Delivery offset of the completed line (`None` = nothing pending).
    deliver: Option<usize>,
    /// An overlong line is being discarded up to its terminator.
    discarding: bool,
}

impl CanonicalInput {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop all pending input state (the TCSETSF flush path). Latched
    /// reader events are flushed separately via [`CanonicalEvents::clear`].
    pub fn clear(&mut self) {
        self.line.clear();
        self.deliver = None;
        self.discarding = false;
    }

    pub fn has_pending_delivery(&self) -> bool {
        self.deliver.is_some()
    }

    /// Process one canonical-mode input byte.
    pub fn on_char(&mut self, spec: &CanonicalSpec, ch: u8, events: &CanonicalEvents) {
        if self.discarding {
            // The overlong line is discarded whole: every byte up to and
            // including its terminator is dropped without delivery, so no
            // truncated prefix can ever be executed.
            if spec.is_eol(ch) || spec.is_eof(ch) {
                self.discarding = false;
            }
            return;
        }
        if spec.is_kill(ch) {
            self.line.clear();
            return;
        }
        if spec.is_erase(ch) {
            self.line.pop();
            return;
        }
        if spec.is_eol(ch) || spec.is_eof(ch) {
            if !spec.is_eof(ch) {
                self.line.push(ch);
            }
            if self.line.is_empty() {
                // VEOF on an empty line: one 0-read event, never queued.
                events.set_eof();
            } else {
                self.deliver = Some(0);
            }
            return;
        }
        if ch == b' ' || ch == b'\t' || ch.is_ascii_graphic() {
            if self.line.len() >= MAX_LINE_LEN {
                // Overlong: discard the whole line, latch one
                // InvalidInput, and keep dropping until the terminator.
                self.line.clear();
                self.discarding = true;
                events.set_overflow();
            } else {
                self.line.push(ch);
            }
        }
        // Other control bytes are dropped (previous behavior preserved).
    }

    /// Push the pending line into `sink`; returns whether any byte was
    /// accepted. Backpressure keeps the remaining offset pending so the
    /// next flush resumes without loss or duplication.
    pub fn flush_pending(&mut self, sink: &mut impl LineSink) -> bool {
        let Some(offset) = self.deliver else {
            return false;
        };
        let accepted = sink.push(&self.line[offset..]);
        let next = offset + accepted;
        if next == self.line.len() {
            self.deliver = None;
            self.line.clear();
        } else {
            self.deliver = Some(next);
        }
        accepted > 0
    }
}
