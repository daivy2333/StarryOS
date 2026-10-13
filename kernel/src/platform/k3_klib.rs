//! K3-limited axklib link seam (MS12 Iteration 000, task T1).
//!
//! The `axdriver/dyn` registry compiles a PCI probe callback that references
//! the `axklib` extern `"Rust"` shim `__axklib_0_3_mem_iomap` (trait-ffi
//! `__{crate}_{version}_{fn}` naming over axklib 0.3). axruntime only
//! provides its `Klib` implementation under the `paging` feature, and the K3
//! async-net mode never enables paging (static boot mapping, no dynamic
//! iomap, no `probe_all_devices`), so the kernel must define the symbol
//! itself or the dyn edge cannot be linked at all.
//!
//! This module defines that single symbol and nothing else: any real iomap
//! request returns `AxError::Unsupported` (fail-closed — the GMAC path never
//! calls it, and no dynamic mapping can silently appear behind the seam).

use axerrno::{AxError, AxResult};
use memory_addr::{PhysAddr, VirtAddr};

/// The `axklib::Klib::mem_iomap` extern entry the dyn registry links against.
/// Signature is fixed by the trait-ffi declaration in axklib 0.3.
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __axklib_0_3_mem_iomap(
    _addr: PhysAddr,
    _size: usize,
) -> AxResult<VirtAddr> {
    Err(AxError::Unsupported)
}
