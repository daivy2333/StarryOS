use core::cell::UnsafeCell;
use core::ptr::addr_of_mut;
use core::task::Context;

use axpoll::PollSet;
use embassy_hal_internal::atomic_ring_buffer::{Reader, RingBuffer, Writer};

pub const BUF_SIZE: usize = 64 * 1024;

// ── Static ring buffer storage ──────────────────────────────────────
// Backing arrays. SAFETY: initialized once during kernel init, valid forever.
pub static RX_RING: RingBuffer = RingBuffer::new();
pub static TX_RING: RingBuffer = RingBuffer::new();
static mut RX_BUF: [u8; BUF_SIZE] = [0u8; BUF_SIZE];
static mut TX_BUF: [u8; BUF_SIZE] = [0u8; BUF_SIZE];

/// One-time ring buffer initialization.  Call during kernel init before
/// any `RingBufRx` / `RingBufTx` is constructed.
pub fn init_ring_buffers() {
    // SAFETY: called exactly once before any concurrent ring-buffer access.
    // The backing `static mut` buffers live for the entire kernel lifetime.
    unsafe {
        RX_RING.init(addr_of_mut!(RX_BUF).cast::<u8>(), BUF_SIZE);
        TX_RING.init(addr_of_mut!(TX_BUF).cast::<u8>(), BUF_SIZE);
    }
}

// ── RingBufRx (receive buffer) ──────────────────────────────────────
//
// The RX copier task *writes* into this buffer; user-side consumers
// (TtyRead) *read* from it.  Both `Writer` and `Reader` are stored
// inside `UnsafeCell` because their methods require `&mut self` while
// the public API is called through `&self` (shared reference).

pub struct RingBufRx {
    // SAFETY: SPSC — only the RX copier task ever calls the writer.
    writer: UnsafeCell<Writer<'static>>,
    // SAFETY: SPSC — only one consumer reads at a time (TtyRead).
    reader: UnsafeCell<Reader<'static>>,
    pub poll: PollSet,
}

// SAFETY: RingBuffer uses atomics for Acquire/Release synchronisation.
// The SPSC guarantee eliminates data races on Writer / Reader despite
// interior mutability via UnsafeCell.
unsafe impl Send for RingBufRx {}
unsafe impl Sync for RingBufRx {}

impl RingBufRx {
    pub fn new() -> Self {
        Self {
            // SAFETY: `init_ring_buffers()` is called before this point,
            // and we create exactly one Writer and one Reader per ring.
            writer: UnsafeCell::new(unsafe { RX_RING.writer() }),
            reader: UnsafeCell::new(unsafe { RX_RING.reader() }),
            poll: PollSet::new(),
        }
    }

    /// Obtain a `&mut Writer` through interior mutability.
    ///
    /// # Safety
    /// SPSC guarantee: only the RX copier task (single writer context)
    /// calls `push()`.
    #[inline]
    #[allow(clippy::mut_from_ref)]
    unsafe fn writer_ref(&self) -> &mut Writer<'static> {
        unsafe { &mut *self.writer.get() }
    }

    /// Obtain a `&mut Reader` through interior mutability.
    ///
    /// # Safety
    /// SPSC guarantee: only one consumer reads from this buffer at a time.
    #[inline]
    #[allow(clippy::mut_from_ref)]
    unsafe fn reader_ref(&self) -> &mut Reader<'static> {
        unsafe { &mut *self.reader.get() }
    }

    /// Push received data into the ring buffer (called by RX copier).
    pub fn push(&self, data: &[u8]) -> usize {
        // SAFETY: SPSC — only the RX copier task calls push().
        let n = unsafe { self.writer_ref() }.push(|buf| {
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
    pub fn pop(&self, buf: &mut [u8]) -> usize {
        // SAFETY: SPSC — only one consumer reads at a time.
        unsafe { self.reader_ref() }.pop(|data| {
            let len = data.len().min(buf.len());
            buf[..len].copy_from_slice(&data[..len]);
            len
        })
    }
}

// ── RingBufTx (transmit buffer) ─────────────────────────────────────
//
// User-side producers (TtyWrite) *write* into this buffer; the TX copier
// task *reads* from it.

pub struct RingBufTx {
    // SAFETY: SPSC — only one producer (TtyWrite) writes to this buffer.
    writer: UnsafeCell<Writer<'static>>,
    // SAFETY: SPSC — only the TX copier task reads from this buffer.
    reader: UnsafeCell<Reader<'static>>,
    ring: &'static RingBuffer,
    pub poll: PollSet,
}

// SAFETY: same reasoning as RingBufRx.
unsafe impl Send for RingBufTx {}
unsafe impl Sync for RingBufTx {}

impl RingBufTx {
    pub fn new() -> Self {
        Self {
            // SAFETY: `init_ring_buffers()` called before this point,
            // exactly one Writer and one Reader created per ring.
            writer: UnsafeCell::new(unsafe { TX_RING.writer() }),
            reader: UnsafeCell::new(unsafe { TX_RING.reader() }),
            ring: &TX_RING,
            poll: PollSet::new(),
        }
    }

    /// # Safety
    /// SPSC: only one producer writes to this buffer.
    #[inline]
    #[allow(clippy::mut_from_ref)]
    unsafe fn writer_ref(&self) -> &mut Writer<'static> {
        unsafe { &mut *self.writer.get() }
    }

    /// # Safety
    /// SPSC: only the TX copier reads from this buffer.
    #[inline]
    #[allow(clippy::mut_from_ref)]
    unsafe fn reader_ref(&self) -> &mut Reader<'static> {
        unsafe { &mut *self.reader.get() }
    }

    /// Push data into the ring buffer (called by producers like TtyWrite).
    pub fn push(&self, data: &[u8]) -> usize {
        // SAFETY: SPSC — only one producer writes to the TX buffer.
        let n = unsafe { self.writer_ref() }.push(|buf| {
            let len = data.len().min(buf.len());
            buf[..len].copy_from_slice(&data[..len]);
            len
        });
        // Wake the TX copier if it's waiting in register_waker().
        if n > 0 {
            self.poll.wake();
        }
        n
    }

    /// Pop data from the ring buffer (called by TX copier).
    pub fn pop(&self, buf: &mut [u8]) -> usize {
        // SAFETY: SPSC — only the TX copier reads from this buffer.
        let n = unsafe { self.reader_ref() }.pop(|data| {
            let len = data.len().min(buf.len());
            buf[..len].copy_from_slice(&data[..len]);
            len
        });
        if n > 0 {
            self.poll.wake();
        }
        n
    }

    pub fn is_empty(&self) -> bool {
        self.ring.is_empty()
    }

    pub fn register_waker(&self, cx: &mut Context<'_>) {
        // Re-check for data that arrived between the failed pop and this call.
        if !self.ring.is_empty() {
            cx.waker().wake_by_ref();
        } else {
            self.poll.register(cx.waker());
        }
    }
}
