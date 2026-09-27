//! Pure, host-testable coherent `(event, last-hart, cumulative-mask)` observation
//! register for a background task's actual execution hart (Task 3.3).
//!
//! Mirrors `kernel/src/drivers/hart_counter.rs` so the same record/read model is
//! available to the axnet owner/runner futures, which must report the hart their
//! real `poll` runs on (never an affinity inference). `record` folds a bit into
//! the cumulative mask before publishing last, then increments events, so a
//! reader that sees `last ∈ mask` gets a self-consistent view; `read` uses bound
//! retries so a torn `(last, mask)` is not returned.

use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

const RETRY_BOUND: usize = 1024;

/// "Never recorded" last-hart sentinel.
pub const UNKNOWN_HART: usize = usize::MAX;

const HART_CAPACITY: usize = 64;

/// Monotonic observation register: `(event-count, last-hart, cumulative-mask)`.
#[derive(Debug)]
pub struct HartCounter {
    last: AtomicUsize,
    mask: AtomicU64,
    events: AtomicU64,
}

impl HartCounter {
    /// Construct an empty register (never recorded anything).
    pub const fn new() -> Self {
        Self {
            last: AtomicUsize::new(UNKNOWN_HART),
            mask: AtomicU64::new(0),
            events: AtomicU64::new(0),
        }
    }

    /// Record one observation on `hart`. Order: mask `fetch_or` -> last store ->
    /// events `fetch_add`, so `read` can validate `last ∈ mask`.
    pub fn record(&self, hart: usize) {
        if hart >= HART_CAPACITY {
            // Out-of-range (e.g. the test-mode System source's UNKNOWN_HART):
            // nothing is recorded. Production this_cpu_id() is always < 64.
            return;
        }
        self.mask.fetch_or(1u64 << hart, Ordering::Relaxed);
        self.last.store(hart, Ordering::Relaxed);
        self.events.fetch_add(1, Ordering::Relaxed);
    }

    /// Read a self-consistent `(last, mask, events)` view.
    pub fn read(&self) -> (usize, u64, u64) {
        for _ in 0..RETRY_BOUND {
            let last = self.last.load(Ordering::Relaxed);
            let mask = self.mask.load(Ordering::Relaxed);
            let events = self.events.load(Ordering::Relaxed);
            if last == UNKNOWN_HART {
                if mask == 0 && events == 0 {
                    return (UNKNOWN_HART, 0, 0);
                }
                continue;
            }
            if (mask & (1u64 << last)) != 0 {
                return (last, mask, events);
            }
        }
        let mask = self.mask.load(Ordering::Relaxed);
        if mask == 0 {
            return (UNKNOWN_HART, 0, 0);
        }
        let last = mask.trailing_zeros() as usize;
        (last, mask, self.events.load(Ordering::Relaxed))
    }
}

/// Hart source abstraction for the owner/runner futures.
///
/// Production reports the system percpu id; host tests inject a fixed value so a
/// fixture records the intended hart deterministically without a real platform.
#[derive(Clone, Copy)]
pub enum HartSource {
    /// Record `axhal::percpu::this_cpu_id()` (production; #[cfg(not(test))]).
    System,
    /// Record a fixed hart (host-test seam).
    #[cfg(test)]
    Injected(usize),
}

impl HartSource {
    pub(crate) fn current(&self) -> usize {
        match self {
            Self::System => {
                #[cfg(not(test))]
                {
                    axhal::percpu::this_cpu_id()
                }
                #[cfg(test)]
                {
                    UNKNOWN_HART
                }
            }
            #[cfg(test)]
            Self::Injected(hart) => *hart,
        }
    }
}