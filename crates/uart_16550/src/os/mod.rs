// SPDX-License-Identifier: MIT OR Apache-2.0

//! OS abstraction traits for cross-platform async UART support.
//!
//! This module defines the **minimum viable interface** that the async UART
//! driver requires from a target OS: task spawning (`OsRuntime`) and waker
//! management (`OsWakerSet`). These are the only OS capabilities the driver
//! actually calls — IRQ registration, MMIO mapping, and lock acquisition
//! are handled externally by the OS adapter layer, keeping the driver
//! logic platform-independent without unnecessary abstraction.
//!
//! See ADR-036 for the design rationale behind the 2-trait minimum.

use core::{future::Future, task::Waker};

/// Task spawning and blocking execution abstraction.
///
/// This trait provides the async runtime primitives needed by the UART driver
/// to spawn background tasks (e.g., copier tasks) and block on futures when
/// synchronous operation is required.
pub trait OsRuntime {
    /// Spawn an async task with a name for debugging.
    ///
    /// The task runs concurrently with the caller. The name is used for
    /// debugging and diagnostics only.
    fn spawn<F>(future: F, name: &str)
    where
        F: Future + Send + 'static,
        F::Output: Send;

    /// Block current thread until future completes.
    ///
    /// This is used in contexts where async/await is not available (e.g.,
    /// during initialization) to synchronously wait for a future to complete.
    fn block_on<F>(future: F) -> F::Output
    where
        F: Future;
}

/// Waker registration and notification abstraction.
///
/// This trait provides a set of wakers that can be notified together, which
/// is used to implement async notification for UART events (e.g., data
/// received, transmit buffer empty). Multiple async tasks can register their
/// wakers to be notified on the same event.
///
/// Implementations must be `Send + Sync` to allow safe sharing across
/// interrupt and task contexts.
pub trait OsWakerSet: Send + Sync {
    /// Create a new empty waker set.
    fn new() -> Self;

    /// Register a waker to be notified on wake events.
    ///
    /// If the waker is already registered, it should be updated (not duplicated).
    fn register(&self, waker: &Waker);

    /// Wake all registered wakers, return count of wakers notified.
    fn wake(&self) -> u32;
}
