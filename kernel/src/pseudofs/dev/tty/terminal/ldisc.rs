use alloc::{boxed::Box, sync::Arc};
use core::{
    cell::UnsafeCell,
    future::poll_fn,
    marker::PhantomData,
    ops::Range,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll, Waker},
};

use axerrno::{AxError, AxResult};
use axpoll::{IoEvents, PollSet, Pollable};
use axtask::future::{block_on, poll_io};
use linux_raw_sys::general::{
    ECHOCTL, ECHOK, ICRNL, IGNCR, ISIG, VEOF, VEOL, VEOL2, VERASE, VKILL, VMIN, VTIME,
};
use ringbuf::{
    CachingCons, CachingProd,
    traits::{Consumer, Observer, Producer, Split},
};
use starry_signal::SignalInfo;

use super::{
    Terminal,
    canonical::{
        CanonicalEvents, CanonicalInput, CanonicalSpec, LineSink, POLL_INPUT_BUDGET, ReadOutcome,
        readable, resolve_read,
    },
    termios::Termios2,
};
use crate::task::send_signal_to_process_group;

const BUF_SIZE: usize = 256;

type ReadBuf = Arc<ringbuf::StaticRb<u8, BUF_SIZE>>;

/// How should we process inputs?
pub enum ProcessMode {
    /// Spawns task for processing inputs, relying on external events to wake
    /// up.
    ///
    /// In this mode a dedicated task is spawned to handle inputs. When there's
    /// nothing to read the argument is invoked to register rx waker.
    External(Box<dyn Fn(Waker) + Send + Sync>),
    /// Do not process inputs.
    ///
    /// This is only used by the master side of pseudo tty. The argument is the
    /// [`PollSet`] for incoming data.
    None(Arc<PollSet>),
}

pub struct TtyConfig<R, W> {
    pub reader: R,
    pub writer: W,
    pub process_mode: ProcessMode,
}

pub use uart_16550::{TtyRead, TtyWrite};

/// Kernel-local writer readiness contract.
///
/// Extends [`TtyWrite`] with writable readiness hints and waker
/// registration, binding TX ring space (UART) or ring buffer vacancy
/// (PTY) to [`IoEvents::OUT`] in the VFS poll layer.
///
/// # Register-Recheck Protocol
///
/// OS adapters MUST use the check → register → recheck protocol
/// before parking a task on writable readiness:
///
/// 1. Call [`can_write`](TtyWriteReady::can_write).
/// 2. If not ready, call [`register_writable_waker`](TtyWriteReady::register_writable_waker).
/// 3. Recheck [`can_write`](TtyWriteReady::can_write) before parking.
///
/// Spurious wakeups are allowed.
pub trait TtyWriteReady: TtyWrite {
    /// Whether blocking writes wait until the complete request is accepted.
    #[must_use]
    fn waits_for_write_completion(&self) -> bool;

    /// Whether the writer has space to accept at least one byte.
    #[must_use]
    fn can_write(&self) -> bool;

    /// Number of bytes the writer can currently accept (hint, not
    /// a reservation — the value may change between this call and
    /// a subsequent [`TtyWrite::write`]).
    #[must_use]
    fn writable_len(&self) -> usize;

    /// Register a waker to be notified when writable readiness
    /// transitions from false to true.
    fn register_writable_waker(&self, waker: &Waker);
}

struct InputReader<R, W> {
    terminal: Arc<Terminal>,

    reader: R,
    writer: W,

    buf_tx: CachingProd<ReadBuf>,
    read_buf: [u8; BUF_SIZE],
    read_range: Range<usize>,

    canon: CanonicalInput,
    events: Arc<CanonicalEvents>,
    clear_line_buf: Arc<AtomicBool>,
}
impl<R: TtyRead, W: TtyWrite> InputReader<R, W> {
    /// Process one bounded input batch. `budget` counts the remaining
    /// input bytes this task wake may consume (T5 ⑤); delivery of an
    /// already-completed line does not consume budget. Returns whether
    /// anything progressed (input consumed or line bytes delivered).
    pub fn poll(&mut self, budget: &mut usize) -> bool {
        if self.clear_line_buf.swap(false, Ordering::Relaxed) {
            self.canon.clear();
            self.events.clear();
        }
        let mut progress = self.canon.flush_pending(&mut RingSink(&mut self.buf_tx));
        if self.read_range.is_empty() && *budget > 0 {
            let read = self.reader.read(&mut self.read_buf);
            self.read_range = 0..read;
        }
        let term = self.terminal.load_termios();
        let spec = canonical_spec(&term);
        loop {
            if self.canon.has_pending_delivery() {
                if self.canon.flush_pending(&mut RingSink(&mut self.buf_tx)) {
                    progress = true;
                }
                if self.canon.has_pending_delivery() {
                    // Still backpressured: new input waits unread in
                    // read_range — only one line is in flight, exactly like
                    // the pre-extraction discipline (no on_char while a
                    // line is delivering).
                    break;
                }
            }
            if self.buf_tx.is_full() || self.read_range.is_empty() || *budget == 0 {
                break;
            }
            let mut ch = self.read_buf[self.read_range.start];
            self.read_range.start += 1;
            *budget -= 1;

            if ch == b'\r' {
                if term.has_iflag(IGNCR) {
                    continue;
                }
                if term.has_iflag(ICRNL) {
                    ch = b'\n';
                }
            }

            self.check_send_signal(&term, ch);

            if term.echo() {
                self.output_char(&term, ch);
            }
            if !term.canonical() {
                self.buf_tx.try_push(ch).unwrap();
                progress = true;
                continue;
            }

            self.canon.on_char(&spec, ch, &self.events);
        }

        progress
    }

    fn check_send_signal(&self, term: &Termios2, ch: u8) {
        if !term.canonical() || !term.has_lflag(ISIG) {
            return;
        }
        if let Some(signo) = term.signo_for(ch)
            && let Some(pg) = self.terminal.job_control.foreground()
        {
            let sig = SignalInfo::new_kernel(signo);
            if let Err(err) = send_signal_to_process_group(pg.pgid(), Some(sig)) {
                warn!("Failed to send signal: {err:?}");
            }
        }
    }

    fn output_char(&self, term: &Termios2, ch: u8) {
        // Echo is best-effort: output may be dropped if the output buffer
        // is full. We explicitly ignore short-write counts.
        match ch {
            b'\n' => {
                let _ = self.writer.write(b"\n");
            }
            b'\r' => {
                let _ = self.writer.write(b"\r\n");
            }
            ch if ch == term.special_char(VERASE) => {
                let _ = self.writer.write(b"\x08 \x08");
            }
            ch if ch == b' ' || ch.is_ascii_graphic() => {
                let _ = self.writer.write(&[ch]);
            }
            ch if ch.is_ascii_control() && term.has_lflag(ECHOCTL) => {
                let _ = self.writer.write(&[b'^', (ch + 0x40)]);
            }
            other => {
                warn!("Ignored echo char: {other:#x}");
            }
        }
    }
}

/// [`LineSink`] adapter over the SPSC input queue producer.
struct RingSink<'a>(&'a mut CachingProd<ReadBuf>);

impl LineSink for RingSink<'_> {
    fn push(&mut self, data: &[u8]) -> usize {
        self.0.push_slice(data)
    }
}

/// Map the live termios onto the canonical classifier inputs.
fn canonical_spec(term: &Termios2) -> CanonicalSpec {
    CanonicalSpec {
        erase: term.special_char(VERASE),
        kill: term.special_char(VKILL),
        eof: term.special_char(VEOF),
        eol: term.special_char(VEOL),
        eol2: term.special_char(VEOL2),
        iexten: term.contains_iexten(),
        echok: term.has_lflag(ECHOK),
    }
}

struct SimpleReader<R> {
    reader: R,
    read_buf: [u8; BUF_SIZE],
    buf_tx: CachingProd<ReadBuf>,
}
impl<R: TtyRead> SimpleReader<R> {
    pub fn poll(&mut self) {
        let read = self.reader.read(&mut self.read_buf);
        let data = &self.read_buf[..read];
        let mut start = 0;
        for (i, &ch) in data.iter().enumerate() {
            if ch == b'\n' {
                if i > start {
                    self.buf_tx.push_slice(&data[start..i]);
                }
                self.buf_tx.push_slice(b"\r\n");
                start = i + 1;
            }
        }
        if start < read {
            self.buf_tx.push_slice(&data[start..read]);
        }
    }
}

enum Processor<R> {
    External(Arc<PollSet>),
    None(SimpleReader<R>, Arc<PollSet>),
}

pub struct LineDiscipline<R, W> {
    terminal: Arc<Terminal>,
    buf_rx: UnsafeCell<CachingCons<ReadBuf>>,
    poll_tx: Arc<PollSet>,
    clear_line_buf: Arc<AtomicBool>,
    events: Arc<CanonicalEvents>,
    processor: Processor<R>,
    _phantom: PhantomData<W>,
}

struct WaitPollable<'a>(Option<&'a Arc<PollSet>>);
impl Pollable for WaitPollable<'_> {
    fn poll(&self) -> IoEvents {
        unreachable!()
    }

    fn register(&self, context: &mut Context<'_>, _events: IoEvents) {
        if let Some(set) = self.0 {
            set.register(context.waker());
        } else {
            context.waker().wake_by_ref();
        }
    }
}

impl<R: TtyRead, W: TtyWrite> LineDiscipline<R, W> {
    pub fn new(terminal: Arc<Terminal>, config: TtyConfig<R, W>) -> Self {
        let (buf_tx, buf_rx) = ReadBuf::default().split();

        let clear_line_buf = Arc::new(AtomicBool::new(false));
        let events = Arc::new(CanonicalEvents::new());
        let mut reader = InputReader {
            terminal: terminal.clone(),

            reader: config.reader,
            writer: config.writer,

            buf_tx,
            read_buf: [0; BUF_SIZE],
            read_range: 0..0,

            canon: CanonicalInput::new(),
            events: events.clone(),
            clear_line_buf: clear_line_buf.clone(),
        };

        let poll_tx = Arc::new(PollSet::new());
        let processor = match config.process_mode {
            ProcessMode::External(register) => {
                let poll_rx = Arc::new(PollSet::new());
                axtask::spawn_with_name(
                    {
                        let poll_rx = poll_rx.clone();
                        let poll_tx = poll_tx.clone();
                        move || {
                            block_on(poll_fn(|cx| {
                                // T5 ⑤: bounded per-wake input budget shared
                                // by both drain passes.
                                let mut budget = POLL_INPUT_BUDGET;
                                while reader.poll(&mut budget) {
                                    poll_rx.wake();
                                }
                                poll_tx.register(cx.waker());
                                register(cx.waker().clone());
                                while reader.poll(&mut budget) {
                                    poll_rx.wake();
                                }
                                if budget == 0 {
                                    // The budget ran out with input likely
                                    // still queued behind it; requeue so the
                                    // burst continues instead of waiting for
                                    // an IRQ that already fired.
                                    cx.waker().wake_by_ref();
                                }
                                Poll::Pending
                            }))
                        }
                    },
                    "tty-reader".into(),
                );
                Processor::External(poll_rx)
            }
            ProcessMode::None(poll_rx) => {
                // Destruct the reader here
                Processor::None(
                    SimpleReader {
                        reader: reader.reader,
                        read_buf: [0; BUF_SIZE],
                        buf_tx: reader.buf_tx,
                    },
                    poll_rx,
                )
            }
        };
        Self {
            terminal,
            buf_rx: UnsafeCell::new(buf_rx),
            poll_tx,
            clear_line_buf,
            events,
            processor,
            _phantom: PhantomData,
        }
    }

    /// Safe accessor for buf_rx using UnsafeCell.
    ///
    /// # SAFETY
    /// Ringbuf uses atomic indices internally for SPSC safety.
    /// Only the reader task calls pop_slice/clear on buf_rx.
    /// The writer (InputReader) only pushes to buf_tx, never touches buf_rx.
    #[allow(clippy::mut_from_ref)]
    fn buf_rx(&self) -> &mut CachingCons<ReadBuf> {
        unsafe { &mut *self.buf_rx.get() }
    }

    pub fn drain_input(&self) {
        self.buf_rx().clear();
        self.clear_line_buf.store(true, Ordering::Relaxed);
        self.events.clear();
    }

    pub fn poll_read(&mut self) -> bool {
        if let Processor::None(reader, _) = &mut self.processor {
            reader.poll();
        }
        readable(self.buf_rx().is_empty(), &self.events)
    }

    pub fn register_rx_waker(&self, waker: &Waker) {
        match &self.processor {
            Processor::External(set) | Processor::None(_, set) => {
                set.register(waker);
            }
        }
    }

    pub fn read(&self, buf: &mut [u8], nonblocking: bool) -> AxResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        if matches!(self.processor, Processor::None(_, _)) {
            let read = self.buf_rx().pop_slice(buf);
            return if read == 0 {
                Err(AxError::WouldBlock)
            } else {
                Ok(read)
            };
        }

        let term = self.terminal.termios.lock().clone();
        let vmin = if term.canonical() {
            1
        } else {
            let vtime = term.special_char(VTIME);
            if vtime > 0 {
                let dur = core::time::Duration::from_millis(vtime as u64 * 100);
                let vmin = term.special_char(VMIN) as usize;
                if buf.len() < vmin as usize {
                    return Err(AxError::WouldBlock);
                }
                let mut total_read = 0;
                let set = match &self.processor {
                    Processor::External(set) => Some(set),
                    _ => unreachable!(),
                };
                let pollable = WaitPollable(set);
                return match block_on(axtask::future::timeout(
                    Some(dur),
                    poll_io(&pollable, IoEvents::IN, nonblocking, || {
                        total_read += self.buf_rx().pop_slice(&mut buf[total_read..]);
                        self.poll_tx.wake();
                        if total_read > 0 {
                            Ok(total_read)
                        } else {
                            Err(AxError::WouldBlock)
                        }
                    }),
                )) {
                    Ok(Ok(n)) => Ok(n),
                    Ok(Err(e)) => Err(e),
                    Err(_elapsed) => Err(AxError::TimedOut),
                };
            }
            term.special_char(VMIN) as usize
        };

        if buf.len() < vmin as usize {
            return Err(AxError::WouldBlock);
        }

        let mut total_read = 0;
        let set = match &self.processor {
            Processor::External(set) => Some(set),
            _ => unreachable!(),
        };
        let pollable = WaitPollable(set);
        let events = self.events.clone();
        block_on(poll_io(&pollable, IoEvents::IN, nonblocking, || {
            total_read += self.buf_rx().pop_slice(&mut buf[total_read..]);
            self.poll_tx.wake();
            if total_read >= vmin {
                return Ok(total_read);
            }
            // Not enough queued data yet (canonical vmin is 1, so this is
            // popped == 0 here); a latched event resolves the read instead
            // of blocking. Raw mode never latches events.
            match resolve_read(0, &events) {
                ReadOutcome::Eof => Ok(0),
                ReadOutcome::InvalidInput => Err(AxError::InvalidInput),
                _ => Err(AxError::WouldBlock),
            }
        }))
    }
}
