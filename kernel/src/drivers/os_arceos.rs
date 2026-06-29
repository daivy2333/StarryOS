//! ArceOS adapter for uart_16550 OS abstraction traits.
//!
//! Implements the 2 minimum-viable traits defined in `uart_16550::os`
//! using ArceOS kernel services (axtask, axpoll). IRQ registration,
//! MMIO mapping, and lock acquisition are handled outside the driver
//! (see ADR-036).

use alloc::string::ToString;
use core::{future::Future, task::Waker};

use uart_16550::os::{OsRuntime, OsWakerSet};

// ── OsRuntime: task spawning and blocking ────────────────────────────

/// ArceOS runtime adapter using `axtask`.
pub struct ArceOsRuntime;

impl OsRuntime for ArceOsRuntime {
    fn spawn<F>(future: F, name: &str)
    where
        F: Future + Send + 'static,
        F::Output: Send,
    {
        let name = name.to_string();
        axtask::spawn_with_name(
            move || {
                axtask::future::block_on(future);
            },
            name,
        );
    }

    fn block_on<F>(future: F) -> F::Output
    where
        F: Future,
    {
        axtask::future::block_on(future)
    }
}

// ── OsWakerSet: waker registration and notification ──────────────────

/// ArceOS waker set adapter using `axpoll::PollSet`.
pub struct ArceOsWakerSet {
    inner: axpoll::PollSet,
}

impl OsWakerSet for ArceOsWakerSet {
    fn new() -> Self {
        Self {
            inner: axpoll::PollSet::new(),
        }
    }

    fn register(&self, waker: &Waker) {
        self.inner.register(waker);
    }

    fn wake(&self) -> u32 {
        self.inner.wake() as u32
    }
}
