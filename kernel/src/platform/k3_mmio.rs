//! Minimal native MMIO access over the platform `phys_to_virt` mapping (MS14
//! Iteration 003, task T10).
//!
//! Under the full paging stage the device windows are reachable only through
//! the platform high-address translation (`axhal::mem::phys_to_virt`); the
//! former raw-physical-pointer reads in the probe env, the MAC identity probe
//! and the source-133 IRQ callbacks relied on the identity mapping that no
//! longer holds once user page tables and paging are in the boot. This module
//! is the one translation point: every caller keeps its own register
//! offsets, access widths and write order unchanged, and no low identity
//! mapping is added anywhere.
//!
//! Host-testable: `tests/k3-embedded-user-host/tests/k3_mmio.rs` replaces
//! only the `axhal::mem` translation boundary and drives these functions
//! against a real backing.

use memory_addr::PhysAddr;

/// Reads one `u32` from the physical device address `paddr`.
pub fn read32(paddr: u64) -> u32 {
    let va = axhal::mem::phys_to_virt(PhysAddr::from(paddr as usize));
    // SAFETY: `paddr` is a DT-declared device window that the full paging
    // stage mapped through the platform translation; the callers only pass
    // adjudicated probe/MMIO constants (MAC/APMU/GPIO/MDIO windows).
    unsafe { core::ptr::read_volatile(va.as_usize() as *const u32) }
}

/// Writes one `u32` to the physical device address `paddr`.
pub fn write32(paddr: u64, value: u32) {
    let va = axhal::mem::phys_to_virt(PhysAddr::from(paddr as usize));
    // SAFETY: same window contract as `read32`; callers write only the
    // adjudicated registers of their own protocols (APMU CTRL, GPIO ack,
    // MDIO, GMAC STATUS/IRQ_EN).
    unsafe { core::ptr::write_volatile(va.as_usize() as *mut u32, value) }
}
