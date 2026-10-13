//! K3 async network host test support (MS12 Iteration 001, tasks 2.1–2.3).
//!
//! Binds the real production modules with `#[path]` — the same root-module
//! sibling pattern as the rustc-direct harnesses — plus the fake board used
//! by the integration tests. The `k3-async-net` feature mirrors the kernel
//! gate so `k3_gmac::adapter` is compiled as a child of the included
//! `k3_gmac` module.

#![allow(dead_code, unused_imports)]

extern crate alloc;

// Kernel ABI symbol required by `axklib` (pulled in via `axdriver/dyn`).
// Only the lib's own `cargo test` binary links without an integration test
// to carry the stub; the integration test binaries define their own, so
// this one is `cfg(test)`-gated to that build alone.
#[cfg(test)]
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __axklib_0_3_mem_iomap(
    _addr: memory_addr::PhysAddr,
    _size: usize,
) -> axerrno::AxResult<memory_addr::VirtAddr> {
    Err(axerrno::AxError::Unsupported)
}

#[path = "../../../crates/axplat-riscv64-k3/src/dma.rs"]
pub mod k3_dma;

#[path = "../../../kernel/src/platform/mac_probe.rs"]
pub mod mac_probe;

#[path = "../../../kernel/src/platform/phy_probe.rs"]
pub mod phy_probe;

#[path = "../../../kernel/src/platform/k3_gmac/mod.rs"]
pub mod k3_gmac;

pub mod fake;

/// The T6 payload host mirror (stack runner start + UDP/TCP echo sessions).
pub mod payload;
