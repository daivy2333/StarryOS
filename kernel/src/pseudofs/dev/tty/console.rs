// SPDX-License-Identifier: MIT OR Apache-2.0

//! Console TTY — polling-mode reader and synchronous full-write writer
//! that delegates to the platform polling port.
//!
//! ## Output (ConsoleWriter)
//!
//! Writes each byte via [`PollingPort::putchar`] (raw MMIO, no buffering,
//! no LF→CRLF conversion — that is the TTY layer's job).
//!
//! ## Input (ConsoleReader)
//!
//! Reads available bytes via [`PollingPort::try_getchar`] — non-blocking,
//! returns whatever is in the UART receive buffer at poll time.

use alloc::sync::Arc;
use core::task::Waker;

use lazy_static::lazy_static;

use super::{
    Tty,
    terminal::{
        Terminal,
        ldisc::{ProcessMode, TtyConfig, TtyRead, TtyWrite, TtyWriteReady},
    },
};
use crate::platform::polling::{with_console_port, with_console_port_tx};

// ---------------------------------------------------------------------------
// ConsoleWriter — synchronous output via platform console
// ---------------------------------------------------------------------------

/// Console output writer using raw [`PollingPort`] MMIO.
///
/// Stateless: can be `Clone`, `Send`, `Sync`, `'static`.
#[derive(Clone)]
pub struct ConsoleWriter;

impl TtyWrite for ConsoleWriter {
    fn write(&self, buf: &[u8]) -> usize {
        with_console_port_tx(|port| {
            for &b in buf {
                port.putchar(b);
            }
        });
        buf.len()
    }
}

impl TtyWriteReady for ConsoleWriter {
    fn waits_for_write_completion(&self) -> bool {
        true
    }

    fn can_write(&self) -> bool {
        true
    }

    fn writable_len(&self) -> usize {
        usize::MAX
    }

    fn register_writable_waker(&self, _waker: &Waker) {
        // Always ready — caller will immediately proceed.
    }
}

// ---------------------------------------------------------------------------
// ConsoleReader — no polling input available
// ---------------------------------------------------------------------------

/// Console input reader — polls UART receive buffer via [`PollingPort::try_getchar`].
///
/// Non-blocking: returns only the bytes available at poll time.
pub struct ConsoleReader;

impl TtyRead for ConsoleReader {
    fn read(&mut self, buf: &mut [u8]) -> usize {
        with_console_port(|port| {
            let mut n = 0;
            while n < buf.len() {
                match port.try_getchar() {
                    Some(b) => {
                        buf[n] = b;
                        n += 1;
                    }
                    None => break,
                }
            }
            n
        })
    }
}

// ---------------------------------------------------------------------------
// CONSOLE_TTY — lazily-constructed global console TTY
// ---------------------------------------------------------------------------

lazy_static! {
    /// The global console TTY.
    ///
    /// Uses `ProcessMode::None` so that no background reader task is spawned.
    pub static ref CONSOLE_TTY: Arc<Tty<ConsoleReader, ConsoleWriter>> = Tty::new(
        Arc::new(Terminal::default()),
        TtyConfig {
            reader: ConsoleReader,
            writer: ConsoleWriter,
            process_mode: ProcessMode::Polling,
        },
    );
}
