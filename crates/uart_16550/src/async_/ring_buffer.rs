// SPDX-License-Identifier: MIT OR Apache-2.0

//! Generic ring buffer wrappers for async UART I/O.
//!
//! [`RingBufRx`] and [`RingBufTx`] wrap `embassy_hal_internal`'s lock-free
//! SPSC [`RingBuffer`] with a generic [`OsWakerSet`] for cross-platform
//! async notification. The owning OS passes `&'static RingBuffer` references;
//! these wrappers do not own the static storage.

#![cfg(feature = "async")]

use core::{cell::UnsafeCell, fmt, task::Waker};

use embassy_hal_internal::atomic_ring_buffer::{Reader, RingBuffer, Writer};

use crate::os::OsWakerSet;

// ── RingBufRx (receive buffer) ──────────────────────────────────────
//
// The RX copier task *writes* into this buffer; user-side consumers
// (TtyRead) *read* from it.  Both `Writer` and `Reader` are stored
// inside `UnsafeCell` because their methods require `&mut self` while
// the public API is called through `&self` (shared reference).

/// RX ring buffer — receives data from UART ISR.
///
/// Wraps an `embassy_hal_internal` lock-free SPSC ring buffer with an
/// OS-generic waker set. The ring buffer storage is owned externally
/// (typically as a `static` in the OS layer) and passed as `&'static`.
pub struct RingBufRx<W: OsWakerSet> {
    // SAFETY: SPSC — only the RX copier task ever calls the writer.
    writer: UnsafeCell<Writer<'static>>,
    // SAFETY: SPSC — only one consumer reads at a time (TtyRead).
    reader: UnsafeCell<Reader<'static>>,
    /// Waker set for notifying consumers when data is available.
    pub poll: W,
}

// SAFETY: RingBuffer uses atomics for Acquire/Release synchronisation.
// The SPSC guarantee eliminates data races on Writer / Reader despite
// interior mutability via UnsafeCell.
unsafe impl<W: OsWakerSet> Send for RingBufRx<W> {}
// SAFETY: Same reasoning as Send — SPSC atomics prevent data races.
unsafe impl<W: OsWakerSet> Sync for RingBufRx<W> {}

impl<W: OsWakerSet> fmt::Debug for RingBufRx<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RingBufRx").finish_non_exhaustive()
    }
}

impl<W: OsWakerSet> RingBufRx<W> {
    /// Create a new RX ring buffer wrapper.
    ///
    /// # Safety
    ///
    /// - `ring` must be a properly initialized `static RingBuffer`.
    /// - The caller must ensure exactly one `RingBufRx` is created per ring.
    pub unsafe fn new(ring: &'static RingBuffer) -> Self {
        Self {
            // SAFETY: Caller guarantees ring is initialized and only one
            // Writer/Reader pair is created per ring.
            writer: UnsafeCell::new(unsafe { ring.writer() }),
            // SAFETY: Same as writer — caller guarantees single Reader per ring.
            reader: UnsafeCell::new(unsafe { ring.reader() }),
            poll: W::new(),
        }
    }

    /// Push received data into the ring buffer (called by RX copier).
    ///
    /// Returns the number of bytes pushed. Wakes all registered wakers
    /// if at least one byte was pushed.
    #[inline(always)]
    pub fn push(&self, data: &[u8]) -> usize {
        // SAFETY: SPSC — only the RX copier task calls push().
        let n = unsafe { &mut *self.writer.get() }.push(|buf| {
            let len = data.len().min(buf.len());
            buf[..len].copy_from_slice(&data[..len]);
            len
        });
        if n > 0 {
            self.poll.wake();
        }
        n
    }

    /// Push multiple bytes into the ring buffer (called by RX copier).
    ///
    /// Returns the number of bytes pushed. Wakes all registered wakers
    /// if at least one byte was pushed.
    #[inline(always)]
    pub fn push_batch(&self, data: &[u8]) -> usize {
        // SAFETY: SPSC — only the RX copier task calls push().
        let n = unsafe { &mut *self.writer.get() }.push(|buf| {
            let len = data.len().min(buf.len());
            buf[..len].copy_from_slice(&data[..len]);
            len
        });
        if n > 0 {
            self.poll.wake();
        }
        n
    }

    /// Pop data from the ring buffer (called by consumers like TtyRead).
    ///
    /// Returns the number of bytes popped.
    pub fn pop(&self, buf: &mut [u8]) -> usize {
        // SAFETY: SPSC — only one consumer reads at a time.
        unsafe { &mut *self.reader.get() }.pop(|data| {
            let len = data.len().min(buf.len());
            buf[..len].copy_from_slice(&data[..len]);
            len
        })
    }

    /// Register a waker to be notified when data is available.
    pub fn register_waker(&self, waker: &Waker) {
        self.poll.register(waker);
    }
}

// ── RingBufTx (transmit buffer) ─────────────────────────────────────
//
// User-side producers (TtyWrite) *write* into this buffer; the TX copier
// task *reads* from it.

/// TX ring buffer — sends data to UART.
///
/// Wraps an `embassy_hal_internal` lock-free SPSC ring buffer with an
/// OS-generic waker set. The ring buffer storage is owned externally
/// (typically as a `static` in the OS layer) and passed as `&'static`.
pub struct RingBufTx<W: OsWakerSet> {
    // SAFETY: SPSC — only one producer (TtyWrite) writes to this buffer.
    writer: UnsafeCell<Writer<'static>>,
    // SAFETY: SPSC — only the TX copier task reads from this buffer.
    reader: UnsafeCell<Reader<'static>>,
    /// Waker set for notifying when space is available or data is consumed.
    pub poll: W,
}

// SAFETY: same reasoning as RingBufRx.
unsafe impl<W: OsWakerSet> Send for RingBufTx<W> {}
// SAFETY: Same reasoning as Send — SPSC atomics prevent data races.
unsafe impl<W: OsWakerSet> Sync for RingBufTx<W> {}

impl<W: OsWakerSet> fmt::Debug for RingBufTx<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RingBufTx").finish_non_exhaustive()
    }
}

impl<W: OsWakerSet> RingBufTx<W> {
    /// Create a new TX ring buffer wrapper.
    ///
    /// # Safety
    ///
    /// - `ring` must be a properly initialized `static RingBuffer`.
    /// - The caller must ensure exactly one `RingBufTx` is created per ring.
    pub unsafe fn new(ring: &'static RingBuffer) -> Self {
        Self {
            // SAFETY: Caller guarantees ring is initialized and only one
            // Writer/Reader pair is created per ring.
            writer: UnsafeCell::new(unsafe { ring.writer() }),
            // SAFETY: Same as writer — caller guarantees single Reader per ring.
            reader: UnsafeCell::new(unsafe { ring.reader() }),
            poll: W::new(),
        }
    }

    /// Push data into the ring buffer (called by producers like TtyWrite).
    ///
    /// Returns the number of bytes pushed. Wakes all registered wakers
    /// if at least one byte was pushed (notifying the TX copier).
    #[inline(always)]
    pub fn push(&self, data: &[u8]) -> usize {
        // SAFETY: SPSC — only one producer writes to the TX buffer.
        let n = unsafe { &mut *self.writer.get() }.push(|buf| {
            let len = data.len().min(buf.len());
            buf[..len].copy_from_slice(&data[..len]);
            len
        });
        if n > 0 {
            self.poll.wake();
        }
        n
    }

    /// Pop data from the ring buffer (called by TX copier).
    ///
    /// Returns the number of bytes popped. Wakes all registered wakers
    /// if at least one byte was popped (space freed for producers).
    #[inline(always)]
    pub fn pop(&self, buf: &mut [u8]) -> usize {
        // SAFETY: SPSC — only the TX copier reads from this buffer.
        let n = unsafe { &mut *self.reader.get() }.pop(|data| {
            let len = data.len().min(buf.len());
            buf[..len].copy_from_slice(&data[..len]);
            len
        });
        if n > 0 {
            self.poll.wake();
        }
        n
    }

    /// Pop multiple bytes from the ring buffer (called by TX copier).
    ///
    /// Returns the number of bytes popped. Wakes all registered wakers
    /// if at least one byte was popped (space freed for producers).
    #[inline(always)]
    pub fn pop_batch(&self, buf: &mut [u8]) -> usize {
        // SAFETY: SPSC — only the TX copier reads from this buffer.
        let n = unsafe { &mut *self.reader.get() }.pop(|data| {
            let len = data.len().min(buf.len());
            buf[..len].copy_from_slice(&data[..len]);
            len
        });
        if n > 0 {
            self.poll.wake();
        }
        n
    }

    /// Check if the ring buffer is empty (used by flush/tcdrain via tx_completion).
    pub fn is_empty(&self) -> bool {
        // SAFETY: pop_buf is a read-only query that returns (ptr, len).
        // We do not call pop_done, so no data is consumed. The &mut self
        // requirement is satisfied via UnsafeCell interior mutability.
        unsafe { (&mut *self.reader.get()).pop_buf().1 == 0 }
    }

    /// Register a waker to be notified when space is available.
    pub fn register_waker(&self, waker: &Waker) {
        self.poll.register(waker);
    }
}
