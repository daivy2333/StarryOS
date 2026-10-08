//! K3 embedded-user host test support (MS17 Iteration 002, task T6).
//!
//! Binds the real production modules with `#[path]` — the same pattern as
//! `k3-polling-net-host` — plus the fake mapping backend and fake stdio
//! table used by the contract tests. The production modules depend only on
//! `core`/`alloc` plus `axerrno`/`memory_addr`/`kernel-elf-parser`
//! /`xmas-elf`, all host-compilable.

#![allow(dead_code, unused_imports)]

extern crate alloc;

#[path = "../../../kernel/src/mm/embedded_loader.rs"]
pub mod embedded_loader;

#[path = "../../../kernel/src/file/stdio.rs"]
pub mod stdio;

pub mod fake;
