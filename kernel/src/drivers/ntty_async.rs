use alloc::{boxed::Box, sync::Arc};

use lazy_static::lazy_static;
use uart_16550::{
    async_::device_ops::{AsyncUartReader, AsyncUartWriter},
    os::OsWakerSet,
};

use crate::{
    drivers::uart_init::{self, ArceOsReader, ArceOsWriter, RawArceOsWriter},
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
            reader: {
                // SAFETY: ASYNC_TTY is constructed exactly once via lazy_static.
                // No other code constructs a raw reader for this driver on the
                // boot path that reaches this point. The reader is subsequently
                // moved into the unique tty-reader task via ProcessMode::External
                // and is never shared or cloned.
                //
                // K3 (MS17 Iteration 002): the k3-uart data-plane
                // qualification payload owns a leaked raw reader and never
                // touches ASYNC_TTY, while the k3-stdio boot path constructs
                // ASYNC_TTY after the copiers start and runs no payload
                // reader. The two boot paths are mutually exclusive
                // (`k3-stdio` selects its own entry branch), so exactly one
                // RX consumer exists per boot. This upholds the SPSC
                // single-consumer contract of RingBufRx.
                unsafe { AsyncUartReader::new(uart_init::driver()) }
            },
            writer: {
                // SAFETY: ASYNC_TTY is constructed exactly once. The startup
                // benchmark finishes before TTY initialization, and no other code
                // constructs a raw writer for this driver.
                let raw = unsafe { AsyncUartWriter::new(uart_init::driver()) };
                ArceOsWriter::new(RawArceOsWriter(raw))
            },
            process_mode: ProcessMode::External(Box::new(move |waker| {
                // register the tty-reader's waker on the RX ring buffer's PollSet.
                // when RX copier pushes data → ring buffer wakes PollSet → tty-reader
                // runs → InputReader.poll() reads → ldisc notifies user read().
                uart_init::driver().rx.poll.register(&waker);
            })),
        },
    );
}
