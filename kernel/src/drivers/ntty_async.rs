use alloc::boxed::Box;
use alloc::sync::Arc;
use lazy_static::lazy_static;

use crate::drivers::device_ops::{AsyncUartReader, AsyncUartWriter};
use crate::drivers::async_driver::DRIVER;
use crate::pseudofs::dev::tty::{
    Tty,
    terminal::ldisc::{ProcessMode, TtyConfig},
};

pub type AsyncTty = Tty<AsyncUartReader, AsyncUartWriter>;

lazy_static! {
    pub static ref ASYNC_TTY: Arc<AsyncTty> = Tty::new(
        Arc::default(),
        TtyConfig {
            reader: AsyncUartReader,
            writer: AsyncUartWriter,
            process_mode: ProcessMode::External(Box::new(move |waker| {
                // register the tty-reader's waker on the ring buffer's PollSet.
                // when RX copier pushes data → ring buffer wakes PollSet → tty-reader
                // runs → InputReader.poll() reads → ldisc notifies user read().
                // unlike Manual mode, this does NOT immediately wake — the waker
                // stays registered until the copier produces data, eliminating the
                // yield storm.
                DRIVER.rx.lock().poll.register(&waker);
            })),
        },
    );
}
