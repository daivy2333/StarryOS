extern crate self as axhal;

pub mod mem {
    use memory_addr::{PhysAddr, VirtAddr};
    use std::cell::UnsafeCell;
    struct Register(UnsafeCell<u32>);
    unsafe impl Sync for Register {}
    static REGISTER: Register = Register(UnsafeCell::new(0x1234_5678));
    pub fn phys_to_virt(pa: PhysAddr) -> VirtAddr {
        assert_eq!(pa.as_usize(), 0xcac8_2000);
        VirtAddr::from(REGISTER.0.get() as usize)
    }
    pub fn backing() -> *mut u32 { REGISTER.0.get() }
}

#[path = "../../../kernel/src/platform/k3_mmio.rs"]
mod k3_mmio;

#[test]
fn native_access_translates_physical_address_before_volatile_io() {
    let pa = 0xcac8_2000;
    assert_ne!(pa as usize, mem::backing() as usize);
    unsafe {
        assert_eq!(k3_mmio::read32(pa), 0x1234_5678);
        k3_mmio::write32(pa, 0xabcd_9876);
        assert_eq!(mem::backing().read_volatile(), 0xabcd_9876);
    }
}
