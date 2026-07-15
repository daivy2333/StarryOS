// SPDX-License-Identifier: MIT OR Apache-2.0

//! Generic ring buffer wrappers for async UART I/O.
//!
//! [`RingBufRx`] and [`RingBufTx`] wrap `embassy_hal_internal`'s lock-free
//! SPSC [`RingBuffer`] with a generic [`OsWakerSet`] for cross-platform
//! async notification. The owning OS passes `&'static RingBuffer` references;
//! these wrappers do not own the static storage.

#![cfg(feature = "async")]

use core::{cell::UnsafeCell, fmt, sync::atomic::Ordering, task::Waker};

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
    /// Reference to the underlying ring buffer for read-only index snapshots.
    ring: &'static RingBuffer,
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
            ring,
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

    /// Readiness hint: number of bytes currently available in the RX ring.
    ///
    /// Returns the total readable byte count across all ring segments,
    /// not just the first contiguous chunk. This is an instantaneous
    /// snapshot — the value may change between this call and a subsequent
    /// [`pop`](Self::pop). Callers MUST NOT treat a non-zero return as a
    /// reservation; a later pop may still return fewer bytes (or zero)
    /// if another consumer has drained the ring in the meantime.
    ///
    /// This method is non-blocking, finite, and does not consume data.
    #[inline]
    pub fn occupied_len(&self) -> usize {
        let capacity = self.ring.len();
        // Match Reader::pop_buf(): the producer publishes end with Release,
        // so the consumer observes it with Acquire before reading start.
        let end = self.ring.end.load(Ordering::Acquire);
        let start = self.ring.start.load(Ordering::Relaxed);
        ring_distance(start, end, capacity)
    }

    /// Readiness hint: whether at least one byte is available to read.
    ///
    /// Equivalent to `self.occupied_len() > 0`. Same hint semantics apply
    /// — spurious readiness is possible.
    #[inline]
    pub fn has_data(&self) -> bool {
        self.occupied_len() > 0
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
    /// Reference to the underlying ring buffer for read-only index snapshots.
    ring: &'static RingBuffer,
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
            ring,
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
        self.ring.is_empty()
    }

    /// Register a waker to be notified when space is available.
    pub fn register_waker(&self, waker: &Waker) {
        self.poll.register(waker);
    }

    /// Readiness hint: number of bytes the TX ring can currently accept.
    ///
    /// Returns the total writable byte count across all ring segments
    /// (`capacity − occupied`), not just the first contiguous chunk. This
    /// is an instantaneous snapshot — the value may change between this
    /// call and a subsequent [`push`](Self::push). Callers MUST NOT treat
    /// a non-zero return as a reservation; a later push may still accept
    /// fewer bytes (or zero) if another producer has filled the ring in
    /// the meantime.
    ///
    /// This method is non-blocking, finite, and does not modify ring state.
    #[inline]
    pub fn vacant_len(&self) -> usize {
        let capacity = self.ring.len();
        // Match Writer::push_bufs(): the consumer publishes start with
        // Release, so the producer observes it with Acquire before reading end.
        let start = self.ring.start.load(Ordering::Acquire);
        let end = self.ring.end.load(Ordering::Relaxed);
        capacity - ring_distance(start, end, capacity)
    }

    /// Readiness hint: whether the TX ring has at least one free byte.
    ///
    /// Equivalent to `self.vacant_len() > 0`. Same hint semantics apply
    /// — spurious readiness is possible.
    #[inline]
    pub fn has_space(&self) -> bool {
        self.vacant_len() > 0
    }
}

#[inline]
const fn ring_distance(start: usize, end: usize, capacity: usize) -> usize {
    if end >= start {
        end - start
    } else {
        end + capacity * 2 - start
    }
}

#[cfg(all(test, feature = "async"))]
mod readiness_tests {
    use alloc::boxed::Box;
    use core::{
        alloc::Layout,
        sync::atomic::{AtomicU32, Ordering},
        task::{RawWaker, RawWakerVTable, Waker},
    };

    use super::*;

    struct DummyWakerSet {
        registers: AtomicU32,
        wakes: AtomicU32,
    }

    impl OsWakerSet for DummyWakerSet {
        fn new() -> Self {
            Self {
                registers: AtomicU32::new(0),
                wakes: AtomicU32::new(0),
            }
        }
        fn register(&self, _waker: &Waker) {
            self.registers.fetch_add(1, Ordering::Relaxed);
        }
        fn wake(&self) -> u32 {
            self.wakes.fetch_add(1, Ordering::Relaxed)
        }
    }

    fn noop_waker() -> Waker {
        unsafe fn clone(_: *const ()) -> RawWaker {
            raw_waker()
        }
        unsafe fn wake(_: *const ()) {}
        unsafe fn wake_by_ref(_: *const ()) {}
        unsafe fn drop(_: *const ()) {}
        fn raw_waker() -> RawWaker {
            RawWaker::new(
                core::ptr::null(),
                &RawWakerVTable::new(clone, wake, wake_by_ref, drop),
            )
        }
        // SAFETY: the noop vtable is valid for any pointer.
        unsafe { Waker::from_raw(raw_waker()) }
    }

    fn make_ring(capacity: usize) -> (*mut u8, &'static RingBuffer) {
        let layout = Layout::array::<u8>(capacity).unwrap();
        // SAFETY: test-only — leaking allocated memory for 'static lifetime.
        let buf = unsafe { alloc::alloc::alloc(layout) };
        assert!(!buf.is_null());
        let ring = Box::new(RingBuffer::new());
        let ring_ref: &'static RingBuffer = Box::leak(ring);
        // SAFETY: buf is valid for capacity bytes.
        unsafe { ring_ref.init(buf, capacity) };
        (buf, ring_ref)
    }

    // ── RX: occupied_len / has_data / non-consuming ──────────────────

    #[test]
    fn rx_occupied_zero_when_empty() {
        let (_buf, ring) = make_ring(64);
        let rx: RingBufRx<DummyWakerSet> = unsafe { RingBufRx::new(ring) };
        assert_eq!(rx.occupied_len(), 0);
        assert!(!rx.has_data());
    }

    #[test]
    fn rx_occupied_after_push() {
        let (_buf, ring) = make_ring(64);
        let rx: RingBufRx<DummyWakerSet> = unsafe { RingBufRx::new(ring) };
        let n = rx.push(&[1, 2, 3, 4, 5]);
        assert_eq!(n, 5);
        assert_eq!(rx.occupied_len(), 5);
        assert!(rx.has_data());
    }

    #[test]
    fn rx_occupied_non_consuming() {
        let (_buf, ring) = make_ring(64);
        let rx: RingBufRx<DummyWakerSet> = unsafe { RingBufRx::new(ring) };
        rx.push(&[10, 20, 30]);
        assert_eq!(rx.occupied_len(), 3);
        // occupied_len must not consume data
        assert_eq!(rx.occupied_len(), 3);
        let mut out = [0u8; 3];
        assert_eq!(rx.pop(&mut out), 3);
        assert_eq!(out, [10, 20, 30]);
    }

    #[test]
    fn rx_occupied_wrap_around() {
        let (_buf, ring) = make_ring(8);
        let rx: RingBufRx<DummyWakerSet> = unsafe { RingBufRx::new(ring) };
        // Fill to wrap: push 5, pop 3, push again
        assert_eq!(rx.push(&[1, 2, 3, 4, 5]), 5);
        let mut tmp = [0u8; 3];
        assert_eq!(rx.pop(&mut tmp), 3);
        assert_eq!(rx.occupied_len(), 2);
        // Fill the tail, then push into the head so readable data spans
        // both sides of the storage boundary.
        assert_eq!(rx.push(&[6, 7, 8, 9, 10]), 3);
        assert_eq!(rx.push(&[9, 10]), 2);
        assert_eq!(rx.occupied_len(), 7);
        assert!(rx.has_data());
    }

    #[test]
    fn rx_occupied_full_ring() {
        let (_buf, ring) = make_ring(8);
        let rx: RingBufRx<DummyWakerSet> = unsafe { RingBufRx::new(ring) };
        assert_eq!(rx.push(&[1, 2, 3, 4, 5, 6, 7, 8]), 8);
        assert_eq!(rx.occupied_len(), 8);
        // Full ring: push must return 0
        assert_eq!(rx.push(&[9]), 0);
        assert_eq!(rx.occupied_len(), 8);
    }

    // ── TX: vacant_len / has_space ───────────────────────────────────

    #[test]
    fn tx_vacant_full_capacity_when_empty() {
        let (_buf, ring) = make_ring(64);
        let tx: RingBufTx<DummyWakerSet> = unsafe { RingBufTx::new(ring) };
        assert_eq!(tx.vacant_len(), 64);
        assert!(tx.has_space());
        assert!(tx.is_empty());
    }

    #[test]
    fn tx_vacant_decreases_after_push() {
        let (_buf, ring) = make_ring(64);
        let tx: RingBufTx<DummyWakerSet> = unsafe { RingBufTx::new(ring) };
        tx.push(&[0; 20]);
        assert_eq!(tx.vacant_len(), 44);
        assert!(tx.has_space());
        assert!(!tx.is_empty());
    }

    #[test]
    fn tx_vacant_zero_when_full() {
        let (_buf, ring) = make_ring(8);
        let tx: RingBufTx<DummyWakerSet> = unsafe { RingBufTx::new(ring) };
        assert_eq!(tx.push(&[1, 2, 3, 4, 5, 6, 7, 8]), 8);
        assert_eq!(tx.vacant_len(), 0);
        assert!(!tx.has_space());
    }

    #[test]
    fn tx_vacant_wrap_around() {
        let (_buf, ring) = make_ring(8);
        let tx: RingBufTx<DummyWakerSet> = unsafe { RingBufTx::new(ring) };
        // Fill 5, pop 3, push 2 → wrap-around
        assert_eq!(tx.push(&[1, 2, 3, 4, 5]), 5);
        let mut tmp = [0u8; 3];
        assert_eq!(tx.pop(&mut tmp), 3);
        assert_eq!(tx.vacant_len(), 6); // 8 - 2 occupied
        assert_eq!(tx.push(&[6, 7]), 2);
        assert_eq!(tx.vacant_len(), 4); // 8 - 4 occupied (2 old + 2 new, wrap)
        assert!(tx.has_space());
    }

    #[test]
    fn tx_is_empty_after_pop_all() {
        let (_buf, ring) = make_ring(8);
        let tx: RingBufTx<DummyWakerSet> = unsafe { RingBufTx::new(ring) };
        tx.push(&[1, 2, 3]);
        assert!(!tx.is_empty());
        let mut tmp = [0u8; 8];
        assert_eq!(tx.pop(&mut tmp), 3);
        assert!(tx.is_empty());
    }

    // ── Waker registration ────────────────────────────────────────────

    #[test]
    fn rx_register_waker_increments_counter() {
        let (_buf, ring) = make_ring(8);
        let rx: RingBufRx<DummyWakerSet> = unsafe { RingBufRx::new(ring) };
        let waker = noop_waker();
        rx.register_waker(&waker);
        rx.register_waker(&waker);
        assert_eq!(rx.poll.registers.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn tx_register_waker_increments_counter() {
        let (_buf, ring) = make_ring(8);
        let tx: RingBufTx<DummyWakerSet> = unsafe { RingBufTx::new(ring) };
        let waker = noop_waker();
        tx.register_waker(&waker);
        assert_eq!(tx.poll.registers.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn rx_push_wakes() {
        let (_buf, ring) = make_ring(8);
        let rx: RingBufRx<DummyWakerSet> = unsafe { RingBufRx::new(ring) };
        assert_eq!(rx.poll.wakes.load(Ordering::Relaxed), 0);
        rx.push(&[1]);
        assert_eq!(rx.poll.wakes.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn tx_pop_wakes() {
        let (_buf, ring) = make_ring(8);
        let tx: RingBufTx<DummyWakerSet> = unsafe { RingBufTx::new(ring) };
        tx.push(&[1, 2]);
        assert_eq!(tx.poll.wakes.load(Ordering::Relaxed), 1); // push wakes
        let mut tmp = [0u8; 8];
        tx.pop(&mut tmp);
        assert_eq!(tx.poll.wakes.load(Ordering::Relaxed), 2); // pop also wakes
    }
}
