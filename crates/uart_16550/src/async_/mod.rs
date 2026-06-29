// SPDX-License-Identifier: MIT OR Apache-2.0

//! Async support for UART 16550.
//!
//! This module provides interrupt-driven async I/O primitives including
//! ISR handlers and waker statics for use with `embassy` executors.

pub mod bench;
pub mod device_ops;
pub mod driver;
pub mod isr;
pub mod ring_buffer;
#[cfg(feature = "telemetry")]
pub mod telemetry;
