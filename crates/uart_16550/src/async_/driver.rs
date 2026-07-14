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
    pub fn start_rx_copier(&'static self) {
        R::spawn(
            async move {
                self.rx_copier_loop().await;
            },
            "uart-rx-copier",
        );
    }

    /// Start the TX copier task.
    ///
    /// Spawns an async task that continuously pops from the TX ring
    /// buffer and writes data to the UART.
    pub fn start_tx_copier(&'static self) {
        R::spawn(
            async move {
                self.tx_copier_loop().await;
            },
            "uart-tx-copier",
        );
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
                        if self.uart.transmitter_empty() {
                            DRAIN_WAKER.wake();
                        } else {
                            TX_WAKER.register(cx.waker());
                            self.uart.update_ier(IER::THR_EMPTY, IER::empty());
                            if self.uart.transmitter_empty() {
                                DRAIN_WAKER.wake();
                            }
                        }
                        self.tx.register_waker(cx.waker());
                        self.tx_copier_active.store(false, Ordering::Release);
                        return Poll::Pending;
                    }
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
