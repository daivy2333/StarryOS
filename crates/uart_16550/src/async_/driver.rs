// SPDX-License-Identifier: MIT OR Apache-2.0

//! Async UART driver with RX/TX copier tasks.
//!
//! Provides [`AsyncUartDriver`] which manages background RX and TX copier
//! tasks with NAPI-style interrupt coalescing for high throughput.
//!
//! The driver is generic over:
//! - `R: OsRuntime` — task spawning abstraction
//! - `W: OsWakerSet` — waker notification abstraction for ring buffers
//! - `U: UartPort` — interior-mutability-safe UART hardware access

#[cfg(feature = "telemetry")]
use core::sync::atomic::AtomicU64;
use core::{fmt, future::poll_fn, marker::PhantomData, sync::atomic::Ordering, task::Poll};

use super::{
    isr::{DRAIN_WAKER, RX_WAKER, TX_WAKER},
    ring_buffer::{RingBufRx, RingBufTx},
};
use crate::{
    os::{OsRuntime, OsWakerSet},
    spec::registers::IER,
};

/// NAPI: consecutive successful reads before entering polling mode.
pub const NAPI_THRESHOLD: u32 = 16;
/// NAPI: batch size in polling mode.
pub const NAPI_BATCH_SIZE: usize = 64;
/// Copier buffer size for bulk operations.
pub const COPIER_BUF_SIZE: usize = 1024;
/// Maximum number of fast retries within a single poll when the UART FIFO is full.
const TX_FAST_RETRY_LIMIT: usize = 32;
/// Maximum spin iterations waiting for UART TEMT after last byte sent.
const TX_TEMT_POLL_LIMIT: u32 = 256;
/// Q19C.8e: slow-poll spin interval between `send_bytes` calls during budget-exhausted fallback.
///
/// D1 C906 at fixed frequency: 256 spins ≈ sub-microsecond. Tuned so that
/// `TX_SLOW_POLL_LIMIT × TX_SLOW_POLL_SPINS` covers ~1-2 ms, enough for a 16B FIFO
/// drain at 115200 bps (~1.11 ms). QEMU does not trigger this path.
const TX_SLOW_POLL_SPINS: u32 = 256;
/// Q19C.8e: max slow-poll iterations before falling back to ISR wait.
///
/// After `TX_FAST_RETRY_LIMIT` (32) fast spins fail and the final recheck returns
/// zero, the copier enters this bounded slow-poll instead of immediately pending.
/// This works around D1 THRE IRQ edge loss (learned L255): the FIFO drains during
/// slow-poll, `send_bytes` eventually succeeds, and the copier resumes without
/// waiting for a possibly-lost IRQ. Must NOT be replaced by
/// `TX_FAST_RETRY_LIMIT=0` + drain-side `TX_WAKER` (disproven, see learned L266).
const TX_SLOW_POLL_LIMIT: u32 = 4096;
/// Q19C.8e: max self-wake yield retries before falling back to pure ISR wait.
///
/// When slow-poll exhausts `TX_SLOW_POLL_LIMIT` without progress, the copier
/// self-wakes (`cx.waker().wake_by_ref()`) to let the scheduler run other tasks
/// and retry slow-poll. This covers the 1% case where slow-poll alone is
/// insufficient (scheduler starvation or transient THRE glitch). After this many
/// yield retries, the copier falls back to pure `TX_WAKER` ISR wait.
const TX_YIELD_RETRIES: u32 = 4;

/// UART hardware access abstraction for copier tasks.
///
/// Provides interior-mutability-safe access to UART receive/transmit
/// operations. The OS layer implements this by wrapping `Uart16550` in
/// a suitable lock (e.g., `SpinNoIrq<Uart16550<MmioBackend>>`).
///
/// # Implementor contract
///
/// - `receive_bytes` must read from the UART RBR/THR register
/// - `send_bytes` must write to the UART THR register
/// - Interior mutability must ensure no data races between RX and TX copier
pub trait UartPort: Send + Sync + 'static {
    /// Read available bytes from the UART receive buffer.
    ///
    /// Returns the number of bytes actually read (may be 0 if no data
    /// is available).
    fn receive_bytes(&self, buf: &mut [u8]) -> usize;

    /// Write bytes to the UART transmit buffer.
    ///
    /// Returns the number of bytes actually written (may be 0 if the
    /// transmit buffer is full).
    fn send_bytes(&self, buf: &[u8]) -> usize;

    /// Check if the UART transmitter is fully empty.
    ///
    /// Returns `true` when both the FIFO and shift register are drained
    /// (LSR TRANSMITTER_EMPTY bit is set), indicating all data has been
    /// sent over the wire.
    fn transmitter_empty(&self) -> bool;

    /// Atomically update the IER register.
    ///
    /// Sets bits in `set` and clears bits in `clear`, using an internal
    /// cache for read-modify-write.  The OS layer owns the cache and
    /// the `set_ier` call that writes to hardware.
    fn update_ier(&self, set: IER, clear: IER);
}

/// Snapshot of TX drain progress for flush/tcdrain polling.
///
/// All four conditions must be satisfied for a complete drain:
/// `ring_empty && !copier_active && staged_bytes == 0 && transmitter_empty`
#[derive(Debug, Clone, Copy)]
pub struct TxCompletion {
    /// Whether the TX ring buffer is empty.
    pub ring_empty: bool,
    /// Whether the TX copier is currently inside a poll cycle.
    pub copier_active: bool,
    /// Bytes popped from TX ring but not yet confirmed sent to UART FIFO.
    pub staged_bytes: usize,
    /// Whether the UART shift register is empty (LSR TRANSMITTER_EMPTY).
    pub transmitter_empty: bool,
}

/// Snapshot of TX path counters for board-side diagnostics.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct TxDebugSnapshot {
    /// Calls to the user-facing TX push path.
    pub user_push_calls: u64,
    /// Bytes requested by user-facing TX push calls.
    pub user_push_requested_bytes: u64,
    /// Bytes accepted into the TX ring by user-facing push calls.
    pub user_push_accepted_bytes: u64,
    /// Non-empty TX ring pop batches performed by the TX copier.
    pub ring_pop_calls: u64,
    /// Bytes popped from the TX ring by the TX copier.
    pub ring_pop_bytes: u64,
    /// Calls from the TX copier into `UartPort::send_bytes`.
    pub hw_send_calls: u64,
    /// Bytes reported as accepted by `UartPort::send_bytes`.
    pub hw_send_bytes: u64,
    /// `send_bytes` calls that returned zero.
    pub hw_send_zero: u64,
    /// Largest single positive `send_bytes` return value.
    pub hw_send_max_chunk: u64,
    /// Times the TX copier exhausted its no-progress retry budget.
    pub no_progress_budget_exhausted: u64,
    /// Q19C.8e: times slow-poll ran the full `TX_SLOW_POLL_LIMIT` without progress.
    pub slow_poll_exhausted: u64,
    /// Q19C.8e: times yield-retries exhausted and copier fell back to pure ISR wait.
    pub yield_retries_exhausted: u64,
    /// Task 3.6 replan: times the copier found new TX data after registering the
    /// ring waker and self-woke/retried instead of parking (planted lost-wakeup).
    pub park_after_register_retry: u64,
    /// Whether the TX ring was empty in the snapshot.
    pub ring_empty: u64,
    /// Whether the TX copier was active in the snapshot.
    pub copier_active: u64,
    /// Bytes staged by the TX copier but not yet reported sent.
    pub staged_bytes: u64,
    /// Whether the UART transmitter was empty in the snapshot.
    pub transmitter_empty: u64,
}

#[derive(Debug)]
#[cfg(feature = "telemetry")]
struct TxDebugCounters {
    user_push_calls: AtomicU64,
    user_push_requested_bytes: AtomicU64,
    user_push_accepted_bytes: AtomicU64,
    ring_pop_calls: AtomicU64,
    ring_pop_bytes: AtomicU64,
    hw_send_calls: AtomicU64,
    hw_send_bytes: AtomicU64,
    hw_send_zero: AtomicU64,
    hw_send_max_chunk: AtomicU64,
    no_progress_budget_exhausted: AtomicU64,
    slow_poll_exhausted: AtomicU64,
    yield_retries_exhausted: AtomicU64,
    /// Task 3.6 replan: times the copier found new TX ring data right after
    /// registering the ring waker and self-woke/retried instead of parking.
    park_after_register_retry: AtomicU64,
}

#[cfg(feature = "telemetry")]
impl TxDebugCounters {
    const fn new() -> Self {
        Self {
            user_push_calls: AtomicU64::new(0),
            user_push_requested_bytes: AtomicU64::new(0),
            user_push_accepted_bytes: AtomicU64::new(0),
            ring_pop_calls: AtomicU64::new(0),
            ring_pop_bytes: AtomicU64::new(0),
            hw_send_calls: AtomicU64::new(0),
            hw_send_bytes: AtomicU64::new(0),
            hw_send_zero: AtomicU64::new(0),
            hw_send_max_chunk: AtomicU64::new(0),
            no_progress_budget_exhausted: AtomicU64::new(0),
            slow_poll_exhausted: AtomicU64::new(0),
            yield_retries_exhausted: AtomicU64::new(0),
            park_after_register_retry: AtomicU64::new(0),
        }
    }

    fn reset(&self) {
        self.user_push_calls.store(0, Ordering::Relaxed);
        self.user_push_requested_bytes.store(0, Ordering::Relaxed);
        self.user_push_accepted_bytes.store(0, Ordering::Relaxed);
        self.ring_pop_calls.store(0, Ordering::Relaxed);
        self.ring_pop_bytes.store(0, Ordering::Relaxed);
        self.hw_send_calls.store(0, Ordering::Relaxed);
        self.hw_send_bytes.store(0, Ordering::Relaxed);
        self.hw_send_zero.store(0, Ordering::Relaxed);
        self.hw_send_max_chunk.store(0, Ordering::Relaxed);
        self.no_progress_budget_exhausted
            .store(0, Ordering::Relaxed);
        self.slow_poll_exhausted.store(0, Ordering::Relaxed);
        self.yield_retries_exhausted.store(0, Ordering::Relaxed);
        self.park_after_register_retry
            .store(0, Ordering::Relaxed);
    }
}

impl TxCompletion {
    /// Returns `true` when all four drain conditions are satisfied.
    #[must_use]
    pub const fn is_drained(&self) -> bool {
        self.ring_empty && !self.copier_active && self.staged_bytes == 0 && self.transmitter_empty
    }
}

/// Async UART driver with RX/TX copier tasks.
///
/// Manages two background tasks:
/// - **RX copier**: reads from UART hardware and pushes to the RX ring buffer
/// - **TX copier**: pops from the TX ring buffer and writes to UART hardware
///
/// The RX copier uses NAPI-style interrupt coalescing: after
/// [`NAPI_THRESHOLD`] consecutive successful reads, it switches to
/// polling mode with [`NAPI_BATCH_SIZE`] batch reads per iteration.
///
/// # Usage
///
/// The driver is created as a `&'static` reference (typically via `static`)
/// and passed to `start_rx_copier` / `start_tx_copier` to spawn the
/// background tasks.
///
/// Copier startup requires an explicit uniqueness proof:
///
/// ```compile_fail
/// use uart_16550::{
///     async_::driver::{AsyncUartDriver, UartPort},
///     os::{OsRuntime, OsWakerSet},
/// };
///
/// fn safe_rx_copier_start_is_forbidden<
///     R: OsRuntime,
///     W: OsWakerSet,
///     U: UartPort,
/// >(
///     driver: &'static AsyncUartDriver<R, W, U>,
/// ) {
///     driver.start_rx_copier();
/// }
/// ```
///
/// ```compile_fail
/// use uart_16550::{
///     async_::driver::{AsyncUartDriver, UartPort},
///     os::{OsRuntime, OsWakerSet},
/// };
///
/// fn safe_tx_copier_start_is_forbidden<
///     R: OsRuntime,
///     W: OsWakerSet,
///     U: UartPort,
/// >(
///     driver: &'static AsyncUartDriver<R, W, U>,
/// ) {
///     driver.start_tx_copier();
/// }
/// ```
pub struct AsyncUartDriver<R: OsRuntime, W: OsWakerSet, U: UartPort> {
    /// RX ring buffer — data flows from UART to consumers.
    pub rx: RingBufRx<W>,
    /// TX ring buffer — data flows from producers to UART.
    pub tx: RingBufTx<W>,
    /// Whether the TX copier is currently inside a poll cycle (set on entry, cleared before Pending).
    pub tx_copier_active: core::sync::atomic::AtomicBool,
    /// Bytes popped from TX ring but not yet confirmed sent to UART FIFO.
    pub tx_staged_bytes: core::sync::atomic::AtomicUsize,
    #[cfg(feature = "telemetry")]
    tx_debug: TxDebugCounters,
    uart: &'static U,
    #[cfg(feature = "telemetry")]
    /// Diagnostic counters for TX copier behavior (only available
    /// with the `telemetry` feature).
    pub telemetry: crate::async_::telemetry::Telemetry,
    _runtime: PhantomData<R>,
}

// SAFETY: All fields are Send+Sync:
// - RingBufRx<W>/RingBufTx<W> have explicit unsafe Send+Sync impls
// - &'static U is Send+Sync when U: Send+Sync (guaranteed by UartPort)
// - PhantomData<R> is Send+Sync unconditionally
unsafe impl<R: OsRuntime, W: OsWakerSet, U: UartPort> Send for AsyncUartDriver<R, W, U> {}
// SAFETY: Same reasoning as Send — all fields are Sync-safe.
unsafe impl<R: OsRuntime, W: OsWakerSet, U: UartPort> Sync for AsyncUartDriver<R, W, U> {}

impl<R: OsRuntime, W: OsWakerSet, U: UartPort> fmt::Debug for AsyncUartDriver<R, W, U> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsyncUartDriver").finish_non_exhaustive()
    }
}

impl<R: OsRuntime, W: OsWakerSet, U: UartPort> AsyncUartDriver<R, W, U> {
    /// Create a new driver instance.
    ///
    /// The `uart` reference must be `'static` as it will be shared with
    /// spawned copier tasks that outlive the creating scope.
    pub const fn new(rx: RingBufRx<W>, tx: RingBufTx<W>, uart: &'static U) -> Self {
        Self {
            rx,
            tx,
            tx_copier_active: core::sync::atomic::AtomicBool::new(false),
            tx_staged_bytes: core::sync::atomic::AtomicUsize::new(0),
            #[cfg(feature = "telemetry")]
            tx_debug: TxDebugCounters::new(),
            uart,
            #[cfg(feature = "telemetry")]
            telemetry: crate::async_::telemetry::Telemetry::new(),
            _runtime: PhantomData,
        }
    }

    /// Return a snapshot of the TX drain state.
    ///
    /// Each field is read independently.
    /// Polling callers (flush/tcdrain) repeatedly call this until
    /// `is_drained()` returns true.
    pub fn tx_completion(&self) -> TxCompletion {
        TxCompletion {
            ring_empty: self.tx.is_empty(),
            copier_active: self.tx_copier_active.load(Ordering::Acquire),
            staged_bytes: self.tx_staged_bytes.load(Ordering::Acquire),
            transmitter_empty: self.uart.transmitter_empty(),
        }
    }

    /// Record a user-facing TX push operation.
    #[cfg(feature = "telemetry")]
    pub fn record_tx_push(&self, requested: usize, accepted: usize) {
        self.tx_debug
            .user_push_calls
            .fetch_add(1, Ordering::Relaxed);
        self.tx_debug
            .user_push_requested_bytes
            .fetch_add(requested as u64, Ordering::Relaxed);
        self.tx_debug
            .user_push_accepted_bytes
            .fetch_add(accepted as u64, Ordering::Relaxed);
    }

    /// Record a user-facing TX push operation.
    #[cfg(not(feature = "telemetry"))]
    #[inline(always)]
    pub const fn record_tx_push(&self, _requested: usize, _accepted: usize) {}

    /// Reset diagnostic TX counters without changing TX data-path state.
    #[cfg(feature = "telemetry")]
    pub fn reset_tx_debug(&self) {
        self.tx_debug.reset();
    }

    /// Reset diagnostic TX counters without changing TX data-path state.
    #[cfg(not(feature = "telemetry"))]
    pub const fn reset_tx_debug(&self) {}

    /// Return a snapshot of diagnostic TX counters and drain state.
    #[cfg(feature = "telemetry")]
    pub fn tx_debug_snapshot(&self) -> TxDebugSnapshot {
        let c = self.tx_completion();
        TxDebugSnapshot {
            user_push_calls: self.tx_debug.user_push_calls.load(Ordering::Relaxed),
            user_push_requested_bytes: self
                .tx_debug
                .user_push_requested_bytes
                .load(Ordering::Relaxed),
            user_push_accepted_bytes: self
                .tx_debug
                .user_push_accepted_bytes
                .load(Ordering::Relaxed),
            ring_pop_calls: self.tx_debug.ring_pop_calls.load(Ordering::Relaxed),
            ring_pop_bytes: self.tx_debug.ring_pop_bytes.load(Ordering::Relaxed),
            hw_send_calls: self.tx_debug.hw_send_calls.load(Ordering::Relaxed),
            hw_send_bytes: self.tx_debug.hw_send_bytes.load(Ordering::Relaxed),
            hw_send_zero: self.tx_debug.hw_send_zero.load(Ordering::Relaxed),
            hw_send_max_chunk: self.tx_debug.hw_send_max_chunk.load(Ordering::Relaxed),
            no_progress_budget_exhausted: self
                .tx_debug
                .no_progress_budget_exhausted
                .load(Ordering::Relaxed),
            slow_poll_exhausted: self.tx_debug.slow_poll_exhausted.load(Ordering::Relaxed),
            yield_retries_exhausted: self
                .tx_debug
                .yield_retries_exhausted
                .load(Ordering::Relaxed),
            park_after_register_retry: self
                .tx_debug
                .park_after_register_retry
                .load(Ordering::Relaxed),
            ring_empty: c.ring_empty as u64,
            copier_active: c.copier_active as u64,
            staged_bytes: c.staged_bytes as u64,
            transmitter_empty: c.transmitter_empty as u64,
        }
    }

    /// Return a snapshot of diagnostic TX counters and drain state.
    #[cfg(not(feature = "telemetry"))]
    pub fn tx_debug_snapshot(&self) -> TxDebugSnapshot {
        let c = self.tx_completion();
        TxDebugSnapshot {
            ring_empty: c.ring_empty as u64,
            copier_active: c.copier_active as u64,
            staged_bytes: c.staged_bytes as u64,
            transmitter_empty: c.transmitter_empty as u64,
            ..TxDebugSnapshot::default()
        }
    }

    /// Push bytes directly for the pre-TTY startup benchmark.
    ///
    /// # Safety
    ///
    /// The caller must ensure that no [`AsyncUartWriter`](super::device_ops::AsyncUartWriter)
    /// exists for this driver and that no other TX producer call overlaps this call.
    #[doc(hidden)]
    pub unsafe fn bench_tx_push(&self, data: &[u8]) -> usize {
        self.tx.push(data)
    }

    /// Push test data directly into the RX ring buffer (benchmark-only).
    ///
    /// # Safety
    ///
    /// The caller must ensure that no [`AsyncUartReader`](super::device_ops::AsyncUartReader)
    /// exists for this driver and that the RX copier task is not running.
    #[doc(hidden)]
    pub unsafe fn bench_rx_push(&self, data: &[u8]) -> usize {
        self.rx.push(data)
    }

    /// Pop test data directly from the RX ring buffer (benchmark-only).
    ///
    /// # Safety
    ///
    /// The caller must ensure that no [`AsyncUartReader`](super::device_ops::AsyncUartReader)
    /// exists for this driver and that the RX copier task is not running.
    #[doc(hidden)]
    pub unsafe fn bench_rx_pop(&self, buf: &mut [u8]) -> usize {
        self.rx.pop(buf)
    }

    /// Wakes the RX copier through the driver's own `RX_WAKER` seam.
    ///
    /// QEMU-only controlled-migration stimulus (Task 4.2 / MS08): the wake is
    /// issued by a task pinned to the second allowed hart so the resumed
    /// copier polls there, which the kernel-side hart counter observes
    /// directly. Spurious-safe: a parked copier re-polls and re-parks; a
    /// still-running copier may not have registered its waker yet, so the
    /// caller retries in a bounded loop rather than assuming delivery.
    #[doc(hidden)]
    pub fn wake_rx_copier(&self) {
        RX_WAKER.wake();
    }

    /// Wakes the TX copier through the driver's own `TX_WAKER` seam; see
    /// [`Self::wake_rx_copier`] for the contract.
    #[doc(hidden)]
    pub fn wake_tx_copier(&self) {
        TX_WAKER.wake();
    }

    /// Get a reference to the telemetry counters (only available with `telemetry` feature).
    #[cfg(feature = "telemetry")]
    pub const fn telemetry(&self) -> &crate::async_::telemetry::Telemetry {
        &self.telemetry
    }

    /// Start the RX copier task.
    ///
    /// Spawns an async task that continuously reads from the UART and
    /// pushes data into the RX ring buffer. Uses NAPI-style interrupt
    /// coalescing for high throughput.
    ///
    /// # Safety
    ///
    /// The caller must invoke this method exactly once per driver and must
    /// not start any other task that produces bytes into the same RX ring.
    pub unsafe fn start_rx_copier(&'static self) {
        R::spawn(self.rx_copier(), "uart-rx-copier");
    }

    /// Start the TX copier task.
    ///
    /// Spawns an async task that continuously pops from the TX ring
    /// buffer and writes data to the UART.
    ///
    /// # Safety
    ///
    /// The caller must invoke this method exactly once per driver and must
    /// not start any other task that consumes bytes from the same TX ring.
    pub unsafe fn start_tx_copier(&'static self) {
        R::spawn(self.tx_copier(), "uart-tx-copier");
    }

    /// Explicit RX copier future (design D5).
    ///
    /// Does **not** spawn: it exposes the RX copier loop so an adapter can
    /// enqueue it exactly once, ideally with a pre-enqueue singleton affinity
    /// (e.g. `axtask::spawn_with_name_affinity`) so the copier is pinned from
    /// its very first scheduling step. The caller keeps the sole-producer
    /// contract: only one RX copier may ever run per driver.
    pub fn rx_copier(&'static self) -> impl core::future::Future<Output = ()> + Send {
        async move {
            self.rx_copier_loop().await;
        }
    }

    /// Explicit TX copier future (design D5).
    ///
    /// Does **not** spawn: it exposes the TX copier loop so an adapter can
    /// enqueue it exactly once, ideally with a pre-enqueue singleton affinity.
    /// The caller keeps the sole-consumer contract: only one TX copier may
    /// ever run per driver.
    pub fn tx_copier(&'static self) -> impl core::future::Future<Output = ()> + Send {
        async move {
            self.tx_copier_loop().await;
        }
    }

    /// RX copier loop with NAPI interrupt coalescing.
    async fn rx_copier_loop(&self) {
        let mut read_buf = [0u8; COPIER_BUF_SIZE];
        let mut consecutive = 0u32;

        loop {
            poll_fn(|cx| {
                let batch = if consecutive >= NAPI_THRESHOLD {
                    NAPI_BATCH_SIZE
                } else {
                    COPIER_BUF_SIZE
                };

                let total = self.uart.receive_bytes(&mut read_buf[..batch]);

                if total > 0 {
                    self.rx.push_batch(&read_buf[..total]);
                }

                // NAPI logic: track consecutive successful reads
                if consecutive >= NAPI_THRESHOLD {
                    if total > 0 {
                        consecutive += 1;
                    } else {
                        consecutive = 0;
                        self.uart.update_ier(IER::DATA_READY, IER::empty());
                    }
                } else {
                    consecutive = if total > 0 { consecutive + 1 } else { 0 };
                }

                if consecutive < NAPI_THRESHOLD {
                    self.uart.update_ier(IER::DATA_READY, IER::empty());
                }

                // Register waker for next interrupt
                RX_WAKER.register(cx.waker());

                if total > 0 {
                    Poll::Ready(total)
                } else {
                    Poll::Pending
                }
            })
            .await;
        }
    }

    /// TX copier loop.
    async fn tx_copier_loop(&self) {
        let mut write_buf = [0u8; COPIER_BUF_SIZE];
        let mut pending = 0usize;
        let mut cursor = 0usize;
        let mut yield_retries = 0u32;
        // Plan Review finding 3: the empty-transmitter THRE fallback is armed at
        // most ONCE per idle episode. A 16550 whose transmitter is already empty
        // re-asserts THRE for as long as it stays enabled, so re-enabling it on
        // every empty poll would form an IRQ -> wake -> re-enable -> IRQ loop.
        // The first arm covers the park race; after the ISR consumes the edge
        // (disable + wake), later idle polls rely on the registered ring/TX
        // wakers (a producer push wakes them). Reset when real data moves so the
        // next idle episode gets a fresh one-shot arm.
        let mut thre_idle_armed = false;

        loop {
            poll_fn(|cx| {
                #[cfg(feature = "telemetry")]
                self.telemetry.tx_poll.fetch_add(1, Ordering::Relaxed);

                self.tx_copier_active.store(true, Ordering::Release);

                // If we've sent all pending data, get more from ring buffer
                if cursor >= pending {
                    pending = self.tx.pop_batch(&mut write_buf);
                    cursor = 0;
                    if pending > 0 {
                        #[cfg(feature = "telemetry")]
                        {
                            self.tx_debug.ring_pop_calls.fetch_add(1, Ordering::Relaxed);
                            self.tx_debug
                                .ring_pop_bytes
                                .fetch_add(pending as u64, Ordering::Relaxed);
                        }
                        self.tx_staged_bytes.fetch_add(pending, Ordering::AcqRel);
                    }
                    if pending == 0 {
                        // Task 3.6 replan: close the park lost-wakeup with the
                        // event/register/recheck pattern. We register the TX ring
                        // waker FIRST, then recheck both the ring and the
                        // transmitter before committing to `Pending`. A producer
                        // that pushed between our ring-pop (above) and this
                        // registration would otherwise have its wake consumed
                        // while this task is still Running (no `Blocked -> Ready`,
                        // no remote-ready IPI) and then park with bytes stranded.
                        // Rechecking after registration turns any such new data
                        // into an immediate self-wake/retry.
                        // Task 3.6 replan (finding 5): register BOTH the TX ring
                        // waker AND the ISR's own `TX_WAKER` BEFORE enabling THRE,
                        // so the THRE interrupt fallback always has a waiter to
                        // resume. A pushing producer between ring-pop and here is
                        // then closed by the ring recheck (register→recheck) as
                        // well as by a real THRE ISR.
                        self.tx.register_waker(cx.waker());
                        TX_WAKER.register(cx.waker());

                        // Enable THRE as the hardware wake fallback on the
                        // empty-transmitter park edge. Unlike the busy-transmitter
                        // branch, the already-empty transmitter would otherwise
                        // leave no IRQ source to resume a real `Pending`. The ISR
                        // (`uart_isr_handler`) disables THRE on each THRE edge.
                        // One-shot per idle episode (finding 3): the empty
                        // transmitter re-asserts THRE for as long as it stays
                        // enabled, so re-arming on every idle poll would loop
                        // IRQ -> wake -> re-enable -> IRQ.
                        if !thre_idle_armed {
                            self.uart.update_ier(IER::THR_EMPTY, IER::empty());
                            thre_idle_armed = true;
                        }

                        // Data appeared after registration (lost-edge closed):
                        // retry instead of parking.
                        if !self.tx.is_empty() {
                            #[cfg(feature = "telemetry")]
                            self.tx_debug
                                .park_after_register_retry
                                .fetch_add(1, Ordering::Relaxed);
                            thre_idle_armed = false; // fresh one-shot for the next idle episode
                            self.tx_copier_active.store(false, Ordering::Release);
                            cx.waker().wake_by_ref();
                            return Poll::Pending;
                        }

                        if self.uart.transmitter_empty() {
                            DRAIN_WAKER.wake();
                        }
                        self.tx_copier_active.store(false, Ordering::Release);
                        return Poll::Pending;
                    }
                    // Data is available: the next empty episode re-arms THRE once.
                    thre_idle_armed = false;
                }

                // Bounded retry inner loop
                let mut retries = 0usize;
                loop {
                    let sent = self.uart.send_bytes(&write_buf[cursor..pending]);
                    #[cfg(feature = "telemetry")]
                    {
                        self.tx_debug.hw_send_calls.fetch_add(1, Ordering::Relaxed);
                        if sent > 0 {
                            self.tx_debug
                                .hw_send_bytes
                                .fetch_add(sent as u64, Ordering::Relaxed);
                            self.tx_debug
                                .hw_send_max_chunk
                                .fetch_max(sent as u64, Ordering::Relaxed);
                        } else {
                            self.tx_debug.hw_send_zero.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    cursor += sent;
                    if sent > 0 {
                        self.tx_staged_bytes.fetch_sub(sent, Ordering::AcqRel);
                    }

                    #[cfg(feature = "telemetry")]
                    if sent > 0 {
                        self.telemetry
                            .tx_hw_bytes
                            .fetch_add(sent as u64, Ordering::Relaxed);
                    } else {
                        self.telemetry
                            .tx_no_progress
                            .fetch_add(1, Ordering::Relaxed);
                    }

                    // All data sent — exit inner loop to get more from ring
                    if cursor >= pending {
                        break;
                    }

                    // Made progress — reset retry counter and continue
                    if sent > 0 {
                        retries = 0;
                        continue;
                    }

                    // No progress — increment retry counter
                    retries += 1;
                    if retries <= TX_FAST_RETRY_LIMIT {
                        continue;
                    }

                    // Budget exhausted — register waker, enable THRE, final recheck
                    #[cfg(feature = "telemetry")]
                    self.tx_debug
                        .no_progress_budget_exhausted
                        .fetch_add(1, Ordering::Relaxed);
                    TX_WAKER.register(cx.waker());
                    self.uart.update_ier(IER::THR_EMPTY, IER::empty());

                    let sent = self.uart.send_bytes(&write_buf[cursor..pending]);
                    #[cfg(feature = "telemetry")]
                    {
                        self.tx_debug.hw_send_calls.fetch_add(1, Ordering::Relaxed);
                        if sent > 0 {
                            self.tx_debug
                                .hw_send_bytes
                                .fetch_add(sent as u64, Ordering::Relaxed);
                            self.tx_debug
                                .hw_send_max_chunk
                                .fetch_max(sent as u64, Ordering::Relaxed);
                        } else {
                            self.tx_debug.hw_send_zero.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    cursor += sent;
                    if sent > 0 {
                        self.tx_staged_bytes.fetch_sub(sent, Ordering::AcqRel);
                    }

                    #[cfg(feature = "telemetry")]
                    if sent > 0 {
                        self.telemetry
                            .tx_hw_bytes
                            .fetch_add(sent as u64, Ordering::Relaxed);
                    } else {
                        self.telemetry
                            .tx_no_progress
                            .fetch_add(1, Ordering::Relaxed);
                    }

                    if cursor >= pending {
                        break;
                    }

                    // ── Q19C.8e: bounded slow-poll fallback ─────────────────────
                    // After fast retry budget (32) and final recheck both return zero,
                    // do NOT immediately pending. Instead, do a bounded slow-poll with
                    // spin intervals to give the FIFO time to drain, then retry
                    // `send_bytes`. This works around D1 THRE IRQ edge loss (L255):
                    // software detects THRE re-assertion without depending on the IRQ.
                    // QEMU does not reach this path (THRE responds immediately).
                    // Must NOT be replaced by `TX_FAST_RETRY_LIMIT=0` + drain-side
                    // `TX_WAKER` (disproven, see learned L266).
                    let mut slow_polls = 0u32;
                    let mut made_progress = false;
                    loop {
                        slow_polls += 1;
                        if slow_polls > TX_SLOW_POLL_LIMIT {
                            break;
                        }
                        for _ in 0..TX_SLOW_POLL_SPINS {
                            core::hint::spin_loop();
                        }
                        let sent = self.uart.send_bytes(&write_buf[cursor..pending]);
                        #[cfg(feature = "telemetry")]
                        {
                            self.tx_debug.hw_send_calls.fetch_add(1, Ordering::Relaxed);
                            if sent > 0 {
                                self.tx_debug
                                    .hw_send_bytes
                                    .fetch_add(sent as u64, Ordering::Relaxed);
                                self.tx_debug
                                    .hw_send_max_chunk
                                    .fetch_max(sent as u64, Ordering::Relaxed);
                            } else {
                                self.tx_debug.hw_send_zero.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                        cursor += sent;
                        if sent > 0 {
                            self.tx_staged_bytes.fetch_sub(sent, Ordering::AcqRel);
                            #[cfg(feature = "telemetry")]
                            self.telemetry
                                .tx_hw_bytes
                                .fetch_add(sent as u64, Ordering::Relaxed);
                            made_progress = true;
                            break;
                        }
                        #[cfg(feature = "telemetry")]
                        self.telemetry
                            .tx_no_progress
                            .fetch_add(1, Ordering::Relaxed);
                    }

                    if made_progress {
                        retries = 0;
                        yield_retries = 0;
                        continue;
                    }

                    // Q19C.8e: slow-poll exhausted — self-wake yield retry
                    #[cfg(feature = "telemetry")]
                    self.tx_debug
                        .slow_poll_exhausted
                        .fetch_add(1, Ordering::Relaxed);

                    yield_retries += 1;
                    if yield_retries <= TX_YIELD_RETRIES {
                        cx.waker().wake_by_ref();
                        self.tx_copier_active.store(false, Ordering::Release);
                        return Poll::Pending;
                    }

                    // Yield retries exhausted — pure ISR wait
                    #[cfg(feature = "telemetry")]
                    self.tx_debug
                        .yield_retries_exhausted
                        .fetch_add(1, Ordering::Relaxed);
                    self.tx_copier_active.store(false, Ordering::Release);
                    return Poll::Pending;
                }

                // TEMT corner-case: wait for shift register to drain.
                if !self.uart.transmitter_empty() {
                    for _ in 0..TX_TEMT_POLL_LIMIT {
                        if self.uart.transmitter_empty() {
                            break;
                        }
                        core::hint::spin_loop();
                    }
                }

                // Register waker for next interrupt
                TX_WAKER.register(cx.waker());
                if self.uart.transmitter_empty() {
                    DRAIN_WAKER.wake();
                } else {
                    self.uart.update_ier(IER::THR_EMPTY, IER::empty());
                }

                Poll::Ready(())
            })
            .await;
        }
    }
}

/// MS08 Iteration 001 / Task 2.4: causal witnesses over the explicit copier
/// futures.
///
/// These drive the *real* `rx_copier`/`tx_copier` future with a fake
/// [`UartPort`] and a hand-rolled poll loop, so the ISR→AtomicWaker→copier and
/// ring→copier→hardware→drain causality is witnessed on the actual driver code
/// without spawning a kernel task. `test` harness only; no std beyond the test
/// runner.
///
/// Unlike the Cycle 000 nominal tests (which woke before any waker was
/// registered and polled pre-seeded data), the witnesses here first park the
/// copier against the *shared* destination wakers and then drive the real
/// [`uart_isr_handler`] entry (or a capacity/readiness wake) so the resume is
/// observed through the same AtomicWaker path the product uses.
#[cfg(test)]
mod smp_witness_tests {
    use alloc::boxed::Box;
    use alloc::vec::Vec;
    use core::future::Future;
    use core::pin::Pin;
    use core::ptr::NonNull;
    use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering as AtomicOrdering};
    use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
    use std::alloc::{Layout, alloc};
    use std::sync::Mutex;

    use super::*;
    use crate::{
        async_::isr::uart_isr_handler,
        os::{OsRuntime, OsWakerSet},
        spec::registers::{IER, offsets},
    };
    use embassy_hal_internal::atomic_ring_buffer::RingBuffer;

    /// `RX_WAKER`/`TX_WAKER`/`DRAIN_WAKER` are process-global and each holds a
    /// single waker, so the copier witnesses that park against them are
    /// serialized to avoid one test clearing another's registration mid-flight.
    static WITNESS_LOCK: Mutex<()> = Mutex::new(());

    /// Waker set that counts `register`/`wake` *and* forwards a `wake` to the last
    /// registered waker (mirroring the ArceOS ring waker), so readiness (memory
    /// freed for a TX producer, data made available to an RX consumer) reaches
    /// a real waiting consumer and is observable.
    struct CountingWakerSet {
        registers: AtomicU32,
        wakes: AtomicU32,
        current: Mutex<Option<Waker>>,
    }
    impl Default for CountingWakerSet {
        fn default() -> Self {
            Self {
                registers: AtomicU32::new(0),
                wakes: AtomicU32::new(0),
                current: Mutex::new(None),
            }
        }
    }
    impl OsWakerSet for CountingWakerSet {
        fn new() -> Self {
            Self::default()
        }
        fn register(&self, w: &Waker) {
            self.registers.fetch_add(1, AtomicOrdering::Relaxed);
            *self.current.lock().unwrap() = Some(w.clone());
        }
        fn wake(&self) -> u32 {
            self.wakes.fetch_add(1, AtomicOrdering::Relaxed);
            if let Some(w) = self.current.lock().unwrap().as_ref() {
                w.wake_by_ref();
            }
            self.wakes.load(AtomicOrdering::Relaxed)
        }
    }

    struct TestRuntime;
    impl OsRuntime for TestRuntime {
        fn spawn<F>(_f: F, _n: &str)
        where
            F: Future + Send + 'static,
            F::Output: Send,
        {
        }
        fn block_on<F>(_f: F) -> F::Output
        where
            F: Future,
        {
            unreachable!("witness tests drive copiers directly, never block_on")
        }
    }

    struct FakeUartPort {
        rx: Mutex<Vec<u8>>,
        rx_pos: AtomicUsize,
        tx_sent: Mutex<Vec<u8>>,
        transmitter_empty: AtomicBool,
        /// Number of times the driver enabled the THRE interrupt (IER::THR_EMPTY).
        /// Finding-3 witnesses model a 16550 whose empty transmitter re-asserts
        /// THRE for as long as it stays enabled, so re-arming on every idle poll
        /// would form an IRQ -> wake -> re-enable loop; this counter must stay
        /// one-shot per idle episode.
        thre_enables: AtomicU32,
    }
    impl UartPort for FakeUartPort {
        fn receive_bytes(&self, buf: &mut [u8]) -> usize {
            let src = self.rx.lock().unwrap();
            let pos = self.rx_pos.load(AtomicOrdering::Relaxed);
            let n = buf.len().min(src.len().saturating_sub(pos));
            buf[..n].copy_from_slice(&src[pos..pos + n]);
            self.rx_pos.store(pos + n, AtomicOrdering::Relaxed);
            n
        }
        fn send_bytes(&self, buf: &[u8]) -> usize {
            self.tx_sent.lock().unwrap().extend_from_slice(buf);
            buf.len()
        }
        fn transmitter_empty(&self) -> bool {
            self.transmitter_empty.load(AtomicOrdering::Relaxed)
        }
        fn update_ier(&self, set: IER, _clear: IER) {
            if set.contains(IER::THR_EMPTY) {
                self.thre_enables.fetch_add(1, AtomicOrdering::Relaxed);
            }
        }
    }
    impl FakeUartPort {
        fn inject_rx(&self, bytes: &[u8]) {
            self.rx.lock().unwrap().extend_from_slice(bytes);
        }
    }

    /// A fake UART whose `update_ier(THR_EMPTY)` injects a TX payload straight
    /// into the driver's raw TX ring. In the product the empty-ring park path
    /// runs `register_waker` → `update_ier(THR_EMPTY)` → recheck `is_empty`, so
    /// this port models the *lost-edge producer push* landing exactly at the
    /// register→recheck seam. Critically the raw write does NOT wake the waker
    /// set (unlike `RingBufTx::push`), so the ONLY wake that can be observed is
    /// the copier's own register→recheck self-wake — a genuine witness that the
    /// lost edge is closed, not an artifact of the injection itself.
    struct ThreParkInjectPort {
        tx_sent: Mutex<Vec<u8>>,
        transmitter_empty: AtomicBool,
        ier_thre_set: AtomicU32,
        inject: AtomicU32,
        tx_ring: &'static RingBuffer,
    }
    impl UartPort for ThreParkInjectPort {
        fn receive_bytes(&self, _buf: &mut [u8]) -> usize {
            0
        }
        fn send_bytes(&self, buf: &[u8]) -> usize {
            self.tx_sent.lock().unwrap().extend_from_slice(buf);
            buf.len()
        }
        fn transmitter_empty(&self) -> bool {
            self.transmitter_empty.load(AtomicOrdering::Relaxed)
        }
        fn update_ier(&self, set: IER, _clear: IER) {
            if set.contains(IER::THR_EMPTY) {
                self.ier_thre_set.fetch_add(1, AtomicOrdering::Relaxed);
                // Inject once at the register->recheck seam: update_ier sits
                // textually between `tx.register_waker` and the ring recheck.
                // The raw write performs no waker-set wake.
                if self.inject.fetch_sub(1, AtomicOrdering::Relaxed) > 0 {
                    let mut w = unsafe { self.tx_ring.writer() };
                    let payload = [0x11u8, 0x22, 0x33];
                    w.push(|buf| {
                        let len = payload.len().min(buf.len());
                        buf[..len].copy_from_slice(&payload[..len]);
                        len
                    });
                }
            }
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
        unsafe { Waker::from_raw(raw_waker()) }
    }

    /// A waker that increments `count` on every `wake`, so an `AtomicWaker`
    /// resume is observable rather than a silent no-op.
    fn counting_waker(count: &'static AtomicU32) -> Waker {
        // SAFETY: `clone` returns a waker sharing the same owned counter pointer,
        // which the `drop` vtable explicitly does not free.
        unsafe fn clone(ptr: *const ()) -> RawWaker {
            // SAFETY: `RawWaker::new` is not unsafe; `ptr` is the counter pointer.
            RawWaker::new(ptr, &VTABLE)
        }
        unsafe fn wake(ptr: *const ()) {
            // SAFETY: `ptr` validly points to the `*const AtomicU32` counter
            // `count` is derived from in `build`, which outlives the waker.
            let count = unsafe { &*(ptr as *const AtomicU32) };
            count.fetch_add(1, AtomicOrdering::Relaxed);
        }
        unsafe fn wake_by_ref(ptr: *const ()) {
            // SAFETY: `wake` is the sibling unsafe fn; ptr is the same counted
            // AtomicU32 handed to `build` (see below).
            unsafe { wake(ptr) };
        }
        unsafe fn drop(_: *const ()) {}
        // SAFETY: the vtable calls are self-consistent and the stored pointer is
        // a `*const AtomicU32` valid for the waker's lifetime.
        static VTABLE: RawWakerVTable =
            RawWakerVTable::new(clone, wake, wake_by_ref, drop);
        let ptr = count as *const AtomicU32 as *const ();
        // SAFETY: `ptr` is a non-null dangling-safe pointer to `count`, used only
        // by the vtable above; the waker is used while `count` is live.
        unsafe { Waker::from_raw(RawWaker::new(ptr, &VTABLE)) }
    }

    fn make_ring(capacity: usize) -> (*mut u8, &'static RingBuffer) {
        let layout = Layout::array::<u8>(capacity).unwrap();
        let buf = unsafe { alloc(layout) };
        assert!(!buf.is_null());
        let ring = Box::leak(Box::new(RingBuffer::new()));
        unsafe { ring.init(buf, capacity) };
        (buf, ring)
    }

    fn make_driver<U: UartPort>(
        port: &'static U,
    ) -> &'static AsyncUartDriver<TestRuntime, CountingWakerSet, U> {
        let (_b1, rx_ring) = make_ring(256);
        let (_b2, tx_ring) = make_ring(256);
        let rx = unsafe { RingBufRx::<CountingWakerSet>::new(rx_ring) };
        let tx = unsafe { RingBufTx::<CountingWakerSet>::new(tx_ring) };
        Box::leak(Box::new(AsyncUartDriver::new(rx, tx, port)))
    }

    /// A fake UART MMIO region large enough for the ISR's fixed offsets. The
    /// ISR register lives at byte `offset 2` (offsets::ISR); LSR at byte 5.
    fn make_isr_base() -> NonNull<u8> {
        let buf: &'static mut [u8; 8] = Box::leak(Box::new([0u8; 8]));
        NonNull::new(buf.as_mut_ptr()).unwrap()
    }

    /// Drives a copier future until it registers the given waker and parks
    /// (`Pending`), bounding the loop so a stuck copier fails instead of hanging.
    fn park_with_waker<F: Future>(fut: &mut Pin<&mut F>, count: &'static AtomicU32) {
        let waker = counting_waker(count);
        let mut cx = Context::from_waker(&waker);
        for _ in 0..128 {
            if matches!(fut.as_mut().poll(&mut cx), Poll::Pending) {
                return;
            }
        }
        panic!("copier future never parked");
    }

    /// RED witness for the Cycle 000 nominal test: waking `RX_WAKER` *before* any
    /// waker is registered is a dropped no-op, so a "wake then poll pre-seeded
    /// data" test proves nothing about ISR→copier causality.
    #[test]
    fn prepoll_wake_without_registered_waker_is_noop() {
        let _guard = WITNESS_LOCK.lock().unwrap();
        let count: &'static AtomicU32 = Box::leak(Box::new(AtomicU32::new(0)));

        // No waker registered yet (a fresh copier registers RX_WAKER during its
        // first poll). Waking now must not reach any waker.
        RX_WAKER.wake();
        assert_eq!(
            count.load(AtomicOrdering::Relaxed),
            0,
            "wake before registration must be a dropped no-op"
        );

        // Register a counting waker, then wake: AtomicWaker forwards precisely to
        // the registered waker (the mechanism the real ISR relies on).
        let waker = counting_waker(count);
        RX_WAKER.register(&waker);
        RX_WAKER.wake();
        assert_eq!(
            count.load(AtomicOrdering::Relaxed),
            1,
            "AtomicWaker must forward the wake to a registered waker"
        );

        RX_WAKER.register(&noop_waker());
    }

    /// ISR→AtomicWaker→copier→ring→readiness: with an empty UART the copier
    /// parks (registering a waker), then a real [`uart_isr_handler`] entry (as the
    /// device would drive) wakes it, and a subsequent poll moves the bytes into
    /// the RX ring in order and makes them ready to a waiting consumer.
    #[test]
    fn isr_park_wake_resume_moves_bytes_with_order() {
        let _guard = WITNESS_LOCK.lock().unwrap();
        let port: &'static FakeUartPort = Box::leak(Box::new(FakeUartPort {
            rx: Mutex::new(Vec::new()),
            rx_pos: AtomicUsize::new(0),
            tx_sent: Mutex::new(Vec::new()),
            transmitter_empty: AtomicBool::new(true),
            thre_enables: AtomicU32::new(0),
        }));
        let drv = make_driver(port);
        let wake_count: &'static AtomicU32 = Box::leak(Box::new(AtomicU32::new(0)));
        let rx_ready: &'static AtomicU32 = Box::leak(Box::new(AtomicU32::new(0)));
        let base = make_isr_base();
        // NS16550 ISR bits[3:0]=0b0100 → Received Data Ready.
        unsafe { *base.as_ptr().add(offsets::ISR) = 0b0100_u8 };

        // A consumer parks for RX readiness, then the copier is parked on the
        // (empty) UART, registering a waker into RX_WAKER.
        drv.rx.register_waker(&counting_waker(rx_ready));
        let mut fut = core::pin::pin!(drv.rx_copier());
        park_with_waker(&mut fut, wake_count);

        // Register→recheck readiness: while parked the copier registered RX_WAKER,
        // so a direct wake propagates to our counting waker (closure of the
        // register-path, PD demand: ISR only wakes what is registered).
        let before = wake_count.load(AtomicOrdering::Relaxed);
        RX_WAKER.wake();
        assert!(
            wake_count.load(AtomicOrdering::Relaxed) > before,
            "parked RX copier must have registered a waker (register→recheck)"
        );

        // Real ISR entry: bytes arrive, ISR reads ReceivedDataReady and wakes the
        // parked copier via RX_WAKER.
        port.inject_rx(&[1, 2, 3, 4, 5]);
        let irq_before = wake_count.load(AtomicOrdering::Relaxed);
        uart_isr_handler(0, base, || {}, || {});
        assert!(
            wake_count.load(AtomicOrdering::Relaxed) > irq_before,
            "ISR must wake the parked RX copier via RX_WAKER"
        );

        // Resume: the next poll reads the injected bytes into the RX ring.
        let waker = counting_waker(wake_count);
        let mut cx = Context::from_waker(&waker);
        for _ in 0..128 {
            if drv.rx.occupied_len() == 5 {
                break;
            }
            let _ = fut.as_mut().poll(&mut cx);
        }
        assert_eq!(drv.rx.occupied_len(), 5, "copier must move all arrived bytes");

        // RX ring → readiness: the copier's push wakes the waiting consumer.
        assert!(
            rx_ready.load(AtomicOrdering::Relaxed) >= 1,
            "RX copier push must wake the consumer (ring→readiness)"
        );

        // Byte order is preserved across the ISR→wake→resume path.
        let mut out = [0u8; 5];
        assert_eq!(drv.rx.pop(&mut out), 5);
        assert_eq!(out, [1, 2, 3, 4, 5]);
    }

    /// Ring→copier→hardware→capacity/readiness→four-stage drain boundary: a Full
    /// TX ring is drained by the copier (freeing capacity and waking the
    /// producer), and `is_drained` stays Pending while TEMT=false, completing
    /// once TEMT=true.
    #[test]
    fn tx_full_capacity_recovery_and_temt_drain_boundary() {
        let _guard = WITNESS_LOCK.lock().unwrap();
        let port: &'static FakeUartPort = Box::leak(Box::new(FakeUartPort {
            rx: Mutex::new(Vec::new()),
            rx_pos: AtomicUsize::new(0),
            tx_sent: Mutex::new(Vec::new()),
            transmitter_empty: AtomicBool::new(false),
            thre_enables: AtomicU32::new(0),
        }));
        let drv = make_driver(port);

        let data: Vec<u8> = (0..256).map(|i| i as u8).collect();
        assert_eq!(drv.tx.push(&data), 256, "must fill the 256-byte TX ring");
        assert_eq!(drv.tx.vacant_len(), 0, "ring is Full");
        assert!(!drv.tx.has_space());

        // A TTY producer parks on capacity (register → atomic waker).
        let cap_wakes: &'static AtomicU32 = Box::leak(Box::new(AtomicU32::new(0)));
        drv.tx.register_waker(&counting_waker(cap_wakes));

        // Copier drains: pops all bytes (space freed → producer woken), sends to
        // hardware, and parks on TX_WAKER.
        let tx_wake_count: &'static AtomicU32 = Box::leak(Box::new(AtomicU32::new(0)));
        let mut fut = core::pin::pin!(drv.tx_copier());
        park_with_waker(&mut fut, tx_wake_count);

        assert_eq!(
            *port.tx_sent.lock().unwrap(),
            data,
            "TX copier must send the full payload to hardware"
        );
        assert!(drv.tx.is_empty(), "TX ring must drain");
        assert_eq!(
            drv.tx.vacant_len(),
            256,
            "capacity must be recovered for the producer"
        );
        assert_eq!(
            cap_wakes.load(AtomicOrdering::Relaxed),
            1,
            "TX copier pop must wake the capacity waiter (Full→recovery readiness)"
        );

        // Drain boundary: ring empty, copier inactive, staged 0 hold, but
        // TEMT=false keeps the four-stage drain incomplete (still Pending).
        let c = drv.tx_completion();
        assert!(c.ring_empty && !c.copier_active && c.staged_bytes == 0);
        assert!(
            !c.is_drained(),
            "drain must still be Pending while transmitter is not empty"
        );

        // TEMT=true → the four-stage drain completes.
        port.transmitter_empty.store(true, AtomicOrdering::Relaxed);
        assert!(
            drv.tx_completion().is_drained(),
            "drain completes once all four stages hold including TEMT"
        );
    }

    /// Task 3.6 replan: a producer that pushes AFTER the copier registers the TX
    /// ring waker (but before it fully blocks) must not be lost. The copier parks
    /// with the ring waker registered; a post-registration push wakes it and the
    /// next poll must drain the byte, instead of stranding it with no THRE source.
    #[test]
    fn tx_park_registers_waker_before_blocking_and_absorbs_post_wake() {
        let _guard = WITNESS_LOCK.lock().unwrap();
        let port: &'static FakeUartPort = Box::leak(Box::new(FakeUartPort {
            rx: Mutex::new(Vec::new()),
            rx_pos: AtomicUsize::new(0),
            tx_sent: Mutex::new(Vec::new()),
            transmitter_empty: AtomicBool::new(true), // empty-transmitter park
            thre_enables: AtomicU32::new(0),
        }));
        let drv = make_driver(port);

        // Park the copier on an empty ring (this registers the TX ring waker).
        let tx_wake_count: &'static AtomicU32 = Box::leak(Box::new(AtomicU32::new(0)));
        let mut fut = core::pin::pin!(drv.tx_copier());
        let waker = counting_waker(tx_wake_count);
        let mut cx = Context::from_waker(&waker);
        for _ in 0..128 {
            if matches!(fut.as_mut().poll(&mut cx), Poll::Pending) {
                break;
            }
        }
        assert!(drv.tx.is_empty(), "copier must park on an empty ring");

        // Producer pushes after the copier registered the ring waker: the push
        // wakes the registered waker (post-register), so the data must not be
        // stranded even though the transmitter was already empty.
        let payload: Vec<u8> = [0xAAu8, 0xBB, 0xCC].to_vec();
        assert_eq!(drv.tx.push(&payload), payload.len());
        // The wake reaches the registered ring waker (counted) in the host
        // CountingWakerSet, and a follow-up poll drains the byte to hardware.
        let mut saw_ready = false;
        for _ in 0..128 {
            let poll = fut.as_mut().poll(&mut cx);
            if poll.is_ready() {
                saw_ready = true;
            }
            if !drv.tx.is_empty() && *port.tx_sent.lock().unwrap() == payload {
                break;
            }
        }
        assert_eq!(
            *port.tx_sent.lock().unwrap(),
            payload,
            "post-register push must be drained to hardware, not stranded"
        );
        assert!(drv.tx.is_empty(), "ring must drain");
        assert!(drv.tx_completion().is_drained(), "drain must complete");
        let _ = saw_ready;
    }

    /// Task 3.6 replan (finding 4 of the review): the producer push must be
    /// injected *between ring-waker registration and the return of `Pending`*,
    /// not after parking, and THRE must be witnessed as the empty-transmitter
    /// fallback. This is the deterministic lost-edge the previous test missed:
    /// the push lands in `update_ier`, which sits between `register_waker` and
    /// the ring recheck, so the recheck self-wakes and the byte is not stranded.
    #[test]
    fn tx_park_register_recheck_injects_push_mid_poll_and_witnesses_thre() {
        let _guard = WITNESS_LOCK.lock().unwrap();
        // Share one raw TX ring between the injection port and the driver so a
        // non-waking write at the register->recheck seam lands exactly where the
        // real producer would push. Single thread => single writer (this test),
        // single reader (the TX copier), so the SPSC contract holds.
        let (_b1, rx_ring) = make_ring(256);
        let (_b2, tx_ring) = make_ring(256);
        let port: &'static ThreParkInjectPort = Box::leak(Box::new(ThreParkInjectPort {
            tx_sent: Mutex::new(Vec::new()),
            transmitter_empty: AtomicBool::new(true), // empty-transmitter park
            ier_thre_set: AtomicU32::new(0),
            inject: AtomicU32::new(1),
            tx_ring,
        }));
        let rx = unsafe { RingBufRx::<CountingWakerSet>::new(rx_ring) };
        let tx = unsafe { RingBufTx::<CountingWakerSet>::new(tx_ring) };
        let drv: &'static AsyncUartDriver<TestRuntime, CountingWakerSet, ThreParkInjectPort> =
            Box::leak(Box::new(AsyncUartDriver::new(rx, tx, port)));

        let tx_wake_count: &'static AtomicU32 = Box::leak(Box::new(AtomicU32::new(0)));
        let waker = counting_waker(tx_wake_count);
        let mut cx = Context::from_waker(&waker);
        let mut fut = core::pin::pin!(drv.tx_copier());

        // First poll: empty ring -> register ring waker -> update_ier(THR_EMPTY)
        // injects the payload at the seam -> recheck sees non-empty -> self-wake
        // (registered ring waker already consumed the push) -> returns Pending
        // WITHOUT parking with stranded bytes.
        let poll = fut.as_mut().poll(&mut cx);
        assert_eq!(poll, Poll::Pending, "copier must pend after the mid-poll injection");
        // THRE was requested as the empty-transmitter hardware fallback.
        assert!(
            port.ier_thre_set.load(AtomicOrdering::Relaxed) >= 1,
            "the empty-transmitter park must enable THRE"
        );
        // The mid-poll push landed in the ring AFTER registration (so the ring
        // waker was live) — the counting waker had to receive a wake for the
        // push and/or the register->recheck self-wake.
        assert!(
            tx_wake_count.load(AtomicOrdering::Relaxed) >= 1,
            "a wake must be recorded: the ring waker was registered before the injected push"
        );
        // The payload is present, awaiting the follow-up poll (not stranded as a
        // silent no-wake no-drain).
        assert!(!drv.tx.is_empty(), "injected payload must land in the ring");

        // Follow-up poll drains the injected payload to hardware.
        for _ in 0..128 {
            let _ = fut.as_mut().poll(&mut cx);
            if *port.tx_sent.lock().unwrap() == [0x11u8, 0x22, 0x33] {
                break;
            }
        }
        assert_eq!(
            *port.tx_sent.lock().unwrap(),
            [0x11u8, 0x22, 0x33],
            "mid-poll injected byte must be drained, not stranded"
        );
        assert!(drv.tx.is_empty(), "ring must drain");
        assert!(drv.tx_completion().is_drained(), "drain must complete");
    }

    /// Task 3.6 replan (finding 5): the empty-transmitter park must register the
    /// ISR's own `TX_WAKER` (the correct waiter) BEFORE enabling THRE, so a real
    /// THRE ISR edge has a waiter to resume instead of being a dropped no-op. We
    /// park the copier on an empty ring, then drive a genuine [`uart_isr_handler`]
    /// THRE edge; the ISR must observe a registered waiter (wake reaches the
    /// copier) and disable THRE (`fn_disable_tx`). This witnesses the THRE ISR
    /// fallback directly rather than only the ring recheck self-wake.
    #[test]
    fn tx_park_thre_isr_wakes_registered_waiter() {
        // Non-capturing free fn required by the `fn()` pointer signature of
        // `uart_isr_handler`; observe via a block-local static.
        static TX_DISABLE: AtomicU32 = AtomicU32::new(0);
        TX_DISABLE.store(0, AtomicOrdering::Relaxed);
        fn consume_thre() {
            TX_DISABLE.fetch_add(1, AtomicOrdering::Relaxed);
        }

        let _guard = WITNESS_LOCK.lock().unwrap();
        let port: &'static FakeUartPort = Box::leak(Box::new(FakeUartPort {
            rx: Mutex::new(Vec::new()),
            rx_pos: AtomicUsize::new(0),
            tx_sent: Mutex::new(Vec::new()),
            transmitter_empty: AtomicBool::new(true), // empty-transmitter park
            thre_enables: AtomicU32::new(0),
        }));
        let drv = make_driver(port);
        let base = make_isr_base();
        // THRE interrupt pending: ISR low nibble 0b0010 (bit0=0 pending, code 0b001).
        unsafe { *base.as_ptr().add(offsets::ISR) = 0b0010_u8 };

        let wake_count: &'static AtomicU32 = Box::leak(Box::new(AtomicU32::new(0)));
        let mut fut = core::pin::pin!(drv.tx_copier());
        // Seed the shared ISR waker with a no-op so a THRE edge has NO waiter unless
        // the empty-transmitter park registers one (clean RED when it does not).
        TX_WAKER.register(&noop_waker());
        // Park the copier on an empty ring: the empty-transmitter branch must have
        // registered TX_WAKER as a waiter (before enabling THRE).
        park_with_waker(&mut fut, wake_count);

        let before_wake = wake_count.load(AtomicOrdering::Relaxed);
        // Drive a real THRE ISR edge: handler reads ISR, calls fn_disable_tx (clear
        // THRE) and wakes TX_WAKER — which the copier must have registered.
        uart_isr_handler(0, base, || {}, consume_thre);
        assert!(
            TX_DISABLE.load(AtomicOrdering::Relaxed) >= 1,
            "THRE ISR must call fn_disable_tx to consume/disable the THRE edge"
        );
        assert!(
            wake_count.load(AtomicOrdering::Relaxed) > before_wake,
            "THRE ISR must wake the registered waiting copier via TX_WAKER"
        );

        // The waked copier is alive and re-parks on the still-empty ring (no busy
        // loop, no stranded waiter): a bounded number of follow-up polls all return
        // Pending without panicking, and TX_WAKER stays registered.
        let waker = counting_waker(wake_count);
        let mut cx = Context::from_waker(&waker);
        for _ in 0..128 {
            assert_eq!(
                fut.as_mut().poll(&mut cx),
                Poll::Pending,
                "empty-ring copier must stay parked after the THRE ISR wake"
            );
        }
    }

    /// Plan Review finding 3: on a 16550 whose transmitter is already empty, THRE
    /// re-asserts for as long as it stays enabled. The empty-ring park must arm
    /// the THRE hardware fallback at most ONCE per idle episode: after the ISR
    /// consumes the edge (disable + wake) and the copier re-parks on the
    /// still-empty ring, re-enabling THRE would form an IRQ -> wake -> re-enable
    /// -> IRQ loop. This witness drives repeated real THRE ISR edges and asserts
    /// the driver never re-arms across idle re-parks (unbounded implementation
    /// increments `thre_enables` every cycle — RED), then proves progress still
    /// arrives through the registered ring waker without another THRE arm.
    #[test]
    fn tx_park_thre_reassertion_is_one_shot_and_idle_bounded() {
        // Non-capturing free fn required by the `fn()` pointer signature of
        // `uart_isr_handler`; observe via a block-local static.
        static TX_DISABLE: AtomicU32 = AtomicU32::new(0);
        TX_DISABLE.store(0, AtomicOrdering::Relaxed);
        fn consume_thre() {
            TX_DISABLE.fetch_add(1, AtomicOrdering::Relaxed);
        }

        let _guard = WITNESS_LOCK.lock().unwrap();
        let port: &'static FakeUartPort = Box::leak(Box::new(FakeUartPort {
            rx: Mutex::new(Vec::new()),
            rx_pos: AtomicUsize::new(0),
            tx_sent: Mutex::new(Vec::new()),
            transmitter_empty: AtomicBool::new(true), // empty-transmitter park
            thre_enables: AtomicU32::new(0),
        }));
        let drv = make_driver(port);
        let base = make_isr_base();
        // THRE interrupt pending: ISR low nibble 0b0010 (bit0=0 pending, code 0b001).
        unsafe { *base.as_ptr().add(offsets::ISR) = 0b0010_u8 };

        let wake_count: &'static AtomicU32 = Box::leak(Box::new(AtomicU32::new(0)));
        let mut fut = core::pin::pin!(drv.tx_copier());
        // Seed the shared ISR waker with a no-op so only the copier's own park
        // registration can observe the ISR wake.
        TX_WAKER.register(&noop_waker());
        // First park on the empty ring arms the THRE fallback exactly once.
        park_with_waker(&mut fut, wake_count);
        assert_eq!(
            port.thre_enables.load(AtomicOrdering::Relaxed),
            1,
            "the first empty-ring park must arm the THRE fallback exactly once"
        );

        // Model the hardware: the empty transmitter re-asserts THRE for as long as
        // it stays enabled, so a fresh ISR edge is available after every (would-be)
        // re-enable. Repeated ISR -> wake -> re-park cycles must NOT re-arm THRE.
        let waker = counting_waker(wake_count);
        let mut cx = Context::from_waker(&waker);
        for cycle in 0..8 {
            let wakes_before = wake_count.load(AtomicOrdering::Relaxed);
            uart_isr_handler(0, base, || {}, consume_thre);
            assert!(
                wake_count.load(AtomicOrdering::Relaxed) > wakes_before,
                "THRE ISR must wake the re-parked copier via its registered TX_WAKER (cycle {cycle})"
            );
            assert_eq!(
                fut.as_mut().poll(&mut cx),
                Poll::Pending,
                "copier must re-park on the still-empty ring (cycle {cycle})"
            );
            assert_eq!(
                port.thre_enables.load(AtomicOrdering::Relaxed),
                1,
                "idle re-park must not re-enable THRE (IRQ/wake/re-enable loop, cycle {cycle})"
            );
        }

        // Progress without another THRE arm: a producer push reaches the registered
        // ring waker set; the copier drains the payload; only the NEXT idle episode
        // (after real data moved) may arm THRE once more.
        let payload = [0xAAu8, 0xBB, 0xCC];
        assert_eq!(drv.tx.push(&payload), payload.len());
        for _ in 0..128 {
            let _ = fut.as_mut().poll(&mut cx);
            if *port.tx_sent.lock().unwrap() == payload {
                break;
            }
        }
        assert_eq!(
            *port.tx_sent.lock().unwrap(),
            payload,
            "producer payload must drain through the ring waker without a THRE re-arm"
        );
        for _ in 0..128 {
            assert_eq!(
                fut.as_mut().poll(&mut cx),
                Poll::Pending,
                "copier must park again after draining"
            );
        }
        assert_eq!(
            port.thre_enables.load(AtomicOrdering::Relaxed),
            2,
            "exactly one THRE arm per idle episode (initial park + post-drain re-park)"
        );
    }
}
