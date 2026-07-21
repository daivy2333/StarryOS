// SPDX-License-Identifier: MIT OR Apache-2.0

//! Kernel-local TTY integration traits.
//!
//! These traits are the kernel's own definitions, replacing the re-export
//! from `uart_16550`. After async UART removal, these are the canonical
//! contracts for TTY I/O in StarryOS.
//!
//! ## Migration from `uart_16550` crate
//!
//! Before: `pub use uart_16550::{TtyRead, TtyWrite}` in `ldisc.rs`.
//! After:  These local definitions.  PTY types implement local traits.
//!         Console types implement local traits.

use core::task::Waker;

/// Trait for reading bytes from a TTY input source.
///
/// Implementors pull bytes from a hardware FIFO, ring buffer, or other
/// backend. The [`read`](TtyRead::read) method is non-blocking: it fills
/// as much of `buf` as immediately available and returns the count.
pub trait TtyRead: Send + Sync + 'static {
    /// Read available bytes into `buf`, returning the number actually read.
    ///
    /// Returns `0` if no data is immediately available.
    fn read(&mut self, buf: &mut [u8]) -> usize;
}

/// Trait for writing bytes to a TTY output sink.
///
/// Implementors push bytes to a hardware FIFO, ring buffer, or other
/// backend. The [`write`](TtyWrite::write) method is non-blocking: it
/// pushes as much of `buf` as capacity allows.
///
/// Returns the number of bytes actually accepted. Callers must handle
/// short writes — the returned count may be less than `buf.len()` if
/// the output sink is full.
pub trait TtyWrite: Send + Sync + 'static {
    /// Write bytes from `buf` to the output sink.
    ///
    /// Returns the number of bytes accepted. A return value of `0`
    /// means no bytes could be accepted (sink is full). Callers should
    /// loop on short writes until all bytes are accepted, or explicitly
    /// ignore the return value for best-effort paths such as echo.
    fn write(&self, buf: &[u8]) -> usize;
}

/// Kernel-local writer readiness contract.
///
/// Extends [`TtyWrite`] with writable readiness hints and waker
/// registration, binding TX ring space (UART) or ring buffer vacancy
/// (PTY) to [`IoEvents::OUT`](axpoll::IoEvents::OUT) in the VFS poll layer.
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
