use alloc::{boxed::Box, sync::Arc};

use lazy_static::lazy_static;
use uart_16550::{
    async_::device_ops::{AsyncUartReader, AsyncUartWriter},
    os::OsWakerSet,
};

use crate::{
    drivers::uart_init::{self, ArceOsReader, ArceOsWriter},
    pseudofs::dev::tty::{
        Tty,
        terminal::ldisc::{ProcessMode, TtyConfig},
    },
};

pub type AsyncTty = Tty<ArceOsReader, ArceOsWriter>;

lazy_static! {
    pub static ref ASYNC_TTY: Arc<AsyncTty> = Tty::new(
        Arc::default(),
        TtyConfig {
            reader: AsyncUartReader::new(uart_init::driver()),
            writer: AsyncUartWriter::new(uart_init::driver()),
            process_mode: ProcessMode::External(Box::new(move |waker| {
                // register the tty-reader's waker on the RX ring buffer's PollSet.
                // when RX copier pushes data → ring buffer wakes PollSet → tty-reader
                // runs → InputReader.poll() reads → ldisc notifies user read().
                uart_init::driver().rx.poll.register(&waker);
            })),
        },
    );
}
