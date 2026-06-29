// SPDX-License-Identifier: MIT OR Apache-2.0

//! Diagnostic telemetry counters for the async UART driver.
//!
//! Gated behind `#[cfg(feature = "telemetry")]` — when disabled,
//! all counter operations are compiled out with zero runtime overhead.
//!
//! Counters track TX copier behavior:
//! - `tx_poll`: poll_fn invocations per cycle
//! - `tx_no_progress`: cycles where `send_bytes()` returned 0
//! - `tx_hw_bytes`: bytes successfully written to UART FIFO

use core::{
    fmt,
    sync::atomic::{AtomicU64, Ordering},
};

/// Telemetry counters for async UART driver diagnostics.
///
/// All counters use `Ordering::Relaxed` — they are informational only
/// and do not synchronize with other memory operations.
pub struct Telemetry {
    /// Number of poll_fn invocations in the TX copier loop.
    pub tx_poll: AtomicU64,
    /// Number of poll cycles where `send_bytes()` returned 0
    /// (UART THR was full).
    pub tx_no_progress: AtomicU64,
    /// Total bytes successfully written to the UART FIFO by the TX copier.
    pub tx_hw_bytes: AtomicU64,
}

impl fmt::Debug for Telemetry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Telemetry")
            .field("tx_poll", &self.tx_poll.load(Ordering::Relaxed))
            .field(
                "tx_no_progress",
                &self.tx_no_progress.load(Ordering::Relaxed),
            )
            .field("tx_hw_bytes", &self.tx_hw_bytes.load(Ordering::Relaxed))
            .finish()
    }
}

impl Telemetry {
    /// Create a new set of telemetry counters, all initialized to zero.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            tx_poll: AtomicU64::new(0),
            tx_no_progress: AtomicU64::new(0),
            tx_hw_bytes: AtomicU64::new(0),
        }
    }

    /// Reset all counters to zero.
    ///
    /// Useful for snapshotting per-test statistics in benchmark harnesses.
    pub fn reset(&self) {
        self.tx_poll.store(0, Ordering::Relaxed);
        self.tx_no_progress.store(0, Ordering::Relaxed);
        self.tx_hw_bytes.store(0, Ordering::Relaxed);
    }
}

impl Default for Telemetry {
    fn default() -> Self {
        Self::new()
    }
}

// SAFETY: Telemetry only contains AtomicU64 fields, which are Send + Sync.
unsafe impl Send for Telemetry {}
// SAFETY: Same reasoning as Send.
unsafe impl Sync for Telemetry {}
