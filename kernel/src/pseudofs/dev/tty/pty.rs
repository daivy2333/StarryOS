use alloc::{boxed::Box, sync::Arc};
use core::task::Waker;

use axpoll::PollSet;
use kspin::SpinNoPreempt;
use ringbuf::{
    Cons, HeapRb, Prod,
    traits::{Consumer, Observer, Producer},
};

use super::{
    Tty,
    terminal::{
        Terminal,
        ldisc::{ProcessMode, TtyConfig, TtyRead, TtyWrite, TtyWriteReady},
    },
};

const PTY_BUF_SIZE: usize = 4096;

pub type PtyDriver = Tty<PtyReader, PtyWriter>;

type Buffer = Arc<HeapRb<u8>>;

pub struct PtyReader(Cons<Buffer>);

impl PtyReader {
    pub fn new(buffer: Buffer) -> Self {
        Self(Cons::new(buffer))
    }
}

impl TtyRead for PtyReader {
    fn read(&mut self, buf: &mut [u8]) -> usize {
        self.0.pop_slice(buf)
    }
}

#[derive(Clone)]
pub struct PtyWriter(Arc<SpinNoPreempt<Prod<Buffer>>>, Arc<PollSet>);

impl PtyWriter {
    pub fn new(buffer: Buffer, poll_rx: Arc<PollSet>) -> Self {
        Self(Arc::new(SpinNoPreempt::new(Prod::new(buffer))), poll_rx)
    }
}

impl TtyWrite for PtyWriter {
    fn write(&self, buf: &[u8]) -> usize {
        let written = self.0.lock().push_slice(buf);
        if written > 0 {
            self.1.wake();
        }
        if written < buf.len() {
            warn!("Discarding {} bytes written to pty", buf.len() - written);
        }
        written
    }
}

impl TtyWriteReady for PtyWriter {
    fn waits_for_write_completion(&self) -> bool {
        false
    }

    fn can_write(&self) -> bool {
        // PTY always accepts writes (short-write on full ring).
        // This preserves the current always-OUT poll behavior.
        true
    }

    fn writable_len(&self) -> usize {
        self.0.lock().vacant_len()
    }

    fn register_writable_waker(&self, _waker: &Waker) {
        // PTYs always report OUT and never park on writable readiness.
    }
}

pub(crate) fn create_pty_pair() -> (Arc<PtyDriver>, Arc<PtyDriver>) {
    let master_to_slave = Arc::new(HeapRb::new(PTY_BUF_SIZE));
    let slave_to_master = Arc::new(HeapRb::new(PTY_BUF_SIZE));
    let poll_rx_slave = Arc::new(PollSet::new());
    let poll_rx_master = Arc::new(PollSet::new());

    let terminal = Arc::new(Terminal::default());

    let master = Tty::new(
        terminal.clone(),
        TtyConfig {
            reader: PtyReader::new(slave_to_master.clone()),
            writer: PtyWriter::new(master_to_slave.clone(), poll_rx_slave.clone()),
            process_mode: ProcessMode::None(poll_rx_master.clone()),
        },
    );

    let slave = Tty::new(
        terminal,
        TtyConfig {
            reader: PtyReader::new(master_to_slave),
            writer: PtyWriter::new(slave_to_master, poll_rx_master),
            process_mode: ProcessMode::External(Box::new(move |waker| {
                poll_rx_slave.register(&waker)
            })),
        },
    );

    (master, slave)
}
