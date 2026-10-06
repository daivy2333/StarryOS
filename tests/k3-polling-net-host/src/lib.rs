//! K3 polling network host test support (MS11 Iteration 002, tasks 3.1/3.2).
//!
//! Binds the real production modules with `#[path]` — the same root-module
//! sibling pattern as the rustc-direct harnesses (`k3-gmac-host-harness.rs`
//! and friends) — plus the fake board / memory peer used by the integration
//! tests. The `k3-polling-net` feature mirrors the kernel gate so
//! `k3_gmac::polling_net` is compiled as a child of the included
//! `k3_gmac` module and `k3_network_probe::run_stage13` is compiled with it.

#![allow(dead_code, unused_imports)]

#[path = "../../../crates/axplat-riscv64-k3/src/dma.rs"]
pub mod k3_dma;

#[path = "../../../kernel/src/platform/mac_probe.rs"]
pub mod mac_probe;

#[path = "../../../kernel/src/platform/phy_probe.rs"]
pub mod phy_probe;

#[path = "../../../crates/axplat-riscv64-k3/src/aia.rs"]
pub mod k3_aia;

#[path = "../../../kernel/src/platform/aia_probe.rs"]
pub mod aia_probe;

#[path = "../../../kernel/src/platform/k3_device_irq_probe.rs"]
pub mod k3_device_irq_probe;

#[path = "../../../kernel/src/platform/k3_network_probe.rs"]
pub mod k3_network_probe;

#[path = "../../../kernel/src/platform/k3_gmac/mod.rs"]
pub mod k3_gmac;

pub mod fake;
