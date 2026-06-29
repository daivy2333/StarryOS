// SPDX-License-Identifier: MIT OR Apache-2.0

//! Benchmark and diagnostic helpers for the async UART driver.
//!
//! Re-exports internal performance-related constants and counters useful
//! for benchmarking. Not part of the stable public API.

pub use super::{
    driver::{COPIER_BUF_SIZE, NAPI_BATCH_SIZE, NAPI_THRESHOLD},
    isr::{IRQ_COUNT, irq_count},
};
