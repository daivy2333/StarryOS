//! Early-console pure-logic host test harness.
//!
//! Uses `#[path]` to reference real `kernel/src/platform/console.rs` and
//! `early_console.rs` without compiling the full kernel. Runs via
//! `rustc --test` → `/tmp` binary.
#![allow(dead_code)]

extern crate alloc;
extern crate core;

// console.rs is self-contained (no parent deps)
#[path = "../kernel/src/platform/console.rs"]
mod console;

// early_console.rs uses `use super::console::*` → resolves to `console` mod above
#[path = "../kernel/src/platform/early_console.rs"]
mod early_console;
