// SPDX-License-Identifier: MIT OR Apache-2.0

//! Device operations for async UART.
//!
//! Provides [`AsyncUartReader`] and [`AsyncUartWriter`] that bridge
//! the async UART driver to OS-level traits ([`TtyRead`])
//! and the [`embedded_io_async`] standard async I/O interface.

use alloc::sync::Arc;
use core::{
    fmt,
    future::poll_fn,
    task::{Poll, Waker},
};

use super::{
    driver::{AsyncUartDriver, UartPort},
    isr::DRAIN_WAKER,
};
use crate::{
    os::{OsRuntime, OsWakerSet},
    tty::TtyRead,
};

/// Async UART reader backed by the RX ring buffer.
///
/// Implements [`TtyRead`] and [`embedded_io_async::Read`] for consuming
/// bytes received by the UART. Data is pulled from the RX ring buffer
/// that is filled by the driver's RX copier task.
pub struct AsyncUartReader<R: OsRuntime, W: OsWakerSet, U: UartPort> {
    driver: Arc<AsyncUartDriver<R, W, U>>,
}

impl<R: OsRuntime, W: OsWakerSet, U: UartPort> AsyncUartReader<R, W, U> {
    /// Create a new reader from a shared driver reference.
    #[must_use]
    pub const fn new(driver: Arc<AsyncUartDriver<R, W, U>>) -> Self {
        Self { driver }
    }

    /// Readiness hint: whether the RX ring has data available to read.
    ///
    /// This is a snapshot — the ring state may change between this
    /// call and a subsequent read. See
    /// [`RingBufRx::has_data`](crate::async_::ring_buffer::RingBufRx::has_data)
    /// for the full readiness-hint contract.
    #[must_use]
    #[inline]
    pub fn can_read(&self) -> bool {
        self.driver.rx.has_data()
    }

    /// Register a waker to be notified when RX data arrives.
    ///
    /// OS adapters MUST use the check → register → recheck protocol:
    /// 1. Call [`can_read`](Self::can_read) first.
    /// 2. If not ready, call this method.
    /// 3. Recheck [`can_read`](Self::can_read) before parking the task.
    ///
    /// Spurious wakeups are allowed; the caller must always recheck
    /// readiness after waking.
    #[inline]
    pub fn register_readable_waker(&self, waker: &Waker) {
        self.driver.rx.register_waker(waker);
    }
}

impl<R: OsRuntime, W: OsWakerSet, U: UartPort> fmt::Debug for AsyncUartReader<R, W, U> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsyncUartReader").finish_non_exhaustive()
    }
}

impl<R: OsRuntime + 'static, W: OsWakerSet + 'static, U: UartPort> TtyRead
    for AsyncUartReader<R, W, U>
{
    fn read(&mut self, buf: &mut [u8]) -> usize {
        self.driver.rx.pop(buf)
    }
}

impl<R: OsRuntime, W: OsWakerSet, U: UartPort> embedded_io_async::ErrorType
    for AsyncUartReader<R, W, U>
{
    type Error = core::convert::Infallible;
}

impl<R: OsRuntime, W: OsWakerSet, U: UartPort> embedded_io_async::Read
    for AsyncUartReader<R, W, U>
{
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        Ok(self.driver.rx.pop(buf))
    }
}

/// Async UART writer backed by the TX ring buffer.
///
/// Implements [`embedded_io_async::Write`] for sending bytes through the
/// UART. Data is pushed into the TX ring buffer and transmitted by the
/// driver's TX copier task.
///
/// This type represents a *unique* TX producer capability — it does not
/// implement [`Clone`]. Only one writer may be constructed per driver to
/// uphold the SPSC (single-producer) safety contract of
/// [`RingBufTx`](crate::async_::ring_buffer::RingBufTx).
///
/// Safe crate-external code cannot submit TX bytes through the driver directly:
///
/// ```compile_fail
/// use uart_16550::{
///     async_::driver::{AsyncUartDriver, UartPort},
///     os::{OsRuntime, OsWakerSet},
/// };
///
/// fn bypass_writer<R: OsRuntime, W: OsWakerSet, U: UartPort>(
///     driver: &AsyncUartDriver<R, W, U>,
/// ) {
///     let _ = driver.bench_tx_push(b"bypass");
/// }
/// ```
///
/// The raw writer itself is not cloneable:
///
/// ```compile_fail
/// use uart_16550::{
///     async_::{device_ops::AsyncUartWriter, driver::UartPort},
///     os::{OsRuntime, OsWakerSet},
/// };
///
/// fn require_clone<T: Clone>() {}
/// fn raw_writer_is_unique<R: OsRuntime, W: OsWakerSet, U: UartPort>() {
///     require_clone::<AsyncUartWriter<R, W, U>>();
/// }
/// ```
///
/// Construction requires an explicit unsafe uniqueness proof:
///
/// ```compile_fail
/// use alloc::sync::Arc;
/// use uart_16550::{
///     async_::{device_ops::AsyncUartWriter, driver::{AsyncUartDriver, UartPort}},
///     os::{OsRuntime, OsWakerSet},
/// };
///
/// fn safe_construction_is_forbidden<R: OsRuntime, W: OsWakerSet, U: UartPort>(
///     driver: Arc<AsyncUartDriver<R, W, U>>,
/// ) {
///     let _ = AsyncUartWriter::new(driver);
/// }
/// ```
///
/// The TX ring producer operation is not crate-external API:
///
/// ```compile_fail
/// use uart_16550::{async_::ring_buffer::RingBufTx, os::OsWakerSet};
///
/// fn direct_push_is_forbidden<W: OsWakerSet>(tx: &RingBufTx<W>) {
///     let _ = tx.push(b"bypass");
/// }
/// ```
pub struct AsyncUartWriter<R: OsRuntime, W: OsWakerSet, U: UartPort> {
    driver: Arc<AsyncUartDriver<R, W, U>>,
}

impl<R: OsRuntime, W: OsWakerSet, U: UartPort> AsyncUartWriter<R, W, U> {
    /// Create a unique TX producer from a shared driver reference.
    ///
    /// # Safety
    ///
    /// The caller must guarantee that no second [`AsyncUartWriter`] is
    /// constructed for the same [`AsyncUartDriver`]. This ensures the
    /// SPSC (single-producer) contract of
    /// [`RingBufTx`](crate::async_::ring_buffer::RingBufTx) is upheld.
    /// The driver's TX field uses [`UnsafeCell`](core::cell::UnsafeCell) and is only safe with
    /// a single producer.
    #[must_use]
    pub const unsafe fn new(driver: Arc<AsyncUartDriver<R, W, U>>) -> Self {
        Self { driver }
    }

    /// Readiness hint: whether the TX ring has free space for writing.
    ///
    /// This is a snapshot — the ring state may change between this
    /// call and a subsequent write. See
    /// [`RingBufTx::has_space`](crate::async_::ring_buffer::RingBufTx::has_space)
    /// for the full readiness-hint contract.
    ///
    /// Note: writable readiness is about TX ring space, NOT about
    /// physical drain or completion — use [`flush`](embedded_io_async::Write::flush)
    /// or [`AsyncUartDriver::tx_completion`] for that.
    #[must_use]
    #[inline]
    pub fn can_write(&self) -> bool {
        self.driver.tx.has_space()
    }

    /// Readiness hint: number of bytes the TX ring can currently accept.
    ///
    /// This is a snapshot — returns the total writable byte count
    /// across all ring segments. Same hint semantics as
    /// [`can_write`](Self::can_write) apply: no reservation guarantee.
    #[must_use]
    #[inline]
    pub fn writable_len(&self) -> usize {
        self.driver.tx.vacant_len()
    }

    /// Push bytes into the TX ring buffer.
    ///
    /// Returns the number of bytes actually pushed (may be less than
    /// `buf.len()` if the ring is full). Wakes the TX copier if any
    /// bytes were pushed.
    #[inline]
    pub fn try_write(&mut self, buf: &[u8]) -> usize {
        if buf.is_empty() {
            return 0;
        }
        let n = self.driver.tx.push(buf);
        self.driver.record_tx_push(buf.len(), n);
        n
    }

    /// Register a waker to be notified when TX ring space frees up.
    ///
    /// OS adapters MUST use the check → register → recheck protocol:
    /// 1. Call [`can_write`](Self::can_write) first.
    /// 2. If not ready, call this method.
    /// 3. Recheck [`can_write`](Self::can_write) before parking the task.
    ///
    /// Spurious wakeups are allowed; the caller must always recheck
    /// readiness after waking.
    #[inline]
    pub fn register_writable_waker(&self, waker: &Waker) {
        self.driver.tx.register_waker(waker);
    }
}

impl<R: OsRuntime, W: OsWakerSet, U: UartPort> fmt::Debug for AsyncUartWriter<R, W, U> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsyncUartWriter").finish_non_exhaustive()
    }
}

impl<R: OsRuntime, W: OsWakerSet, U: UartPort> embedded_io_async::ErrorType
    for AsyncUartWriter<R, W, U>
{
    type Error = core::convert::Infallible;
}

impl<R: OsRuntime, W: OsWakerSet, U: UartPort> embedded_io_async::Write
    for AsyncUartWriter<R, W, U>
{
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        Ok(self.try_write(buf))
    }

    async fn flush(&mut self) -> Result<(), Self::Error> {
        poll_fn(|cx| {
            let c = self.driver.tx_completion();
            if c.is_drained() {
                return Poll::Ready(Ok(()));
            }

            // Register waker before recheck (M1 D3 order: register → check)
            // Wake path depends on what's still pending:
            if !c.ring_empty || c.copier_active || c.staged_bytes > 0 {
                // Software side not done — wake when ring data is processed
                self.driver.tx.register_waker(cx.waker());
            }
            DRAIN_WAKER.register(cx.waker());

            // Recheck after registering waker
            let c2 = self.driver.tx_completion();
            if c2.is_drained() {
                Poll::Ready(Ok(()))
            } else {
                Poll::Pending
            }
        })
        .await
    }
}
