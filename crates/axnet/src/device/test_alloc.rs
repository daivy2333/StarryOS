//! Host-test allocation counter shared by `fixed_queue`, `router` and
//! `device` tests (Task 2.4).
//!
//! The counting allocator is installed as the crate-wide `#[global_allocator]`
//! for test builds only. Counting is per-thread: `cargo test` runs each test
//! on its own thread, so a test can freeze [`alloc_count`] after setup and
//! assert that later data-path operations allocate zero. Deltas (not absolute
//! values) are the only safe assertion, because a thread may be reused.

#![cfg(test)]

extern crate std;

use core::{
    alloc::{GlobalAlloc, Layout},
    cell::Cell,
};
use std::{alloc::System, thread_local};

thread_local! {
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}

struct CountingAlloc;

// SAFETY: forwards to `System` and only adds a per-thread observation; the
// global allocator contract is preserved exactly.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCS.with(|c| c.set(c.get() + 1));
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
}

#[global_allocator]
static ALLOCATOR: CountingAlloc = CountingAlloc;

/// Number of allocations made on the current test thread so far.
pub(crate) fn alloc_count() -> usize {
    ALLOCS.with(Cell::get)
}
