use alloc::sync::Arc;
use lazy_static::lazy_static;

use crate::drivers::device_ops::{AsyncUartReader, AsyncUartWriter};
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
            process_mode: ProcessMode::Manual,
        },
    );
}
