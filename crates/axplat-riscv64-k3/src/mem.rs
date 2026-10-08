use axplat::mem::{MemIf, PhysAddr, RawRange, VirtAddr, pa, va};

use crate::config::{
    devices::MMIO_RANGES,
    plat::{KERNEL_BASE_PADDR, PHYS_MEMORY_BASE, PHYS_MEMORY_SIZE, PHYS_VIRT_OFFSET},
};

struct MemIfImpl;

#[impl_plat_interface]
impl MemIf for MemIfImpl {
    fn phys_ram_ranges() -> &'static [RawRange] {
        &[(
            KERNEL_BASE_PADDR,
            PHYS_MEMORY_BASE + PHYS_MEMORY_SIZE - KERNEL_BASE_PADDR,
        )]
    }

    fn reserved_phys_ram_ranges() -> &'static [RawRange] {
        // MS18 Iteration 000 (task 1.1): only the RAM-rootfs boot shape
        // reserves the DTB and rootfs carrying windows; every other mode
        // keeps the empty list (old modes must not gain reservations).
        #[cfg(feature = "ram-rootfs")]
        {
            &crate::ram_layout::RAM_ROOTFS_RESERVED
        }
        #[cfg(not(feature = "ram-rootfs"))]
        {
            &[]
        }
    }

    fn mmio_ranges() -> &'static [RawRange] {
        &MMIO_RANGES
    }

    fn phys_to_virt(paddr: PhysAddr) -> VirtAddr {
        va!(paddr.as_usize() + PHYS_VIRT_OFFSET)
    }

    fn virt_to_phys(vaddr: VirtAddr) -> PhysAddr {
        pa!(vaddr.as_usize() - PHYS_VIRT_OFFSET)
    }

    fn kernel_aspace() -> (VirtAddr, usize) {
        (
            va!(crate::config::plat::KERNEL_ASPACE_BASE),
            crate::config::plat::KERNEL_ASPACE_SIZE,
        )
    }
}
