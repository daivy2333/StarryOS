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
                // No other code constructs a raw reader for this driver. The
                // startup benchmark and UART initialization complete before TTY
                // init, and no second raw reader is constructed in any feature
                // path (QEMU default / qemu,smp / lichee-d1-async-uart). The
                // reader is subsequently moved into the unique tty-reader task
                // via ProcessMode::External and is never shared or cloned.
                //
                // This upholds the SPSC single-consumer contract of RingBufRx.
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
