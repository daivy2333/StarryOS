use axplat::console::ConsoleIf;

use crate::config::{devices::UART_PADDR, plat::PHYS_VIRT_OFFSET};

pub(crate) fn init_early() {}

struct ConsoleIfImpl;

#[impl_plat_interface]
impl ConsoleIf for ConsoleIfImpl {
    fn write_bytes(bytes: &[u8]) {
        let uart_base = (PHYS_VIRT_OFFSET + UART_PADDR) as *mut u32;
        for &c in bytes {
            match c {
                b'\n' => {
                    // SAFETY: D1 UART0 MMIO is identity-mapped by OpenSBI; configured by U-Boot.
                    unsafe {
                        while uart_base.add(5).read_volatile() & (1 << 5) == 0 {
                            core::hint::spin_loop();
                        }
                        uart_base.write_volatile(b'\r' as u32);
                        while uart_base.add(5).read_volatile() & (1 << 5) == 0 {
                            core::hint::spin_loop();
                        }
                        uart_base.write_volatile(b'\n' as u32);
                    }
                }
                c => unsafe {
                    while uart_base.add(5).read_volatile() & (1 << 5) == 0 {
                        core::hint::spin_loop();
                    }
                    uart_base.write_volatile(c as u32);
                },
            }
        }
    }

    fn read_bytes(_bytes: &mut [u8]) -> usize {
        0
    }

    #[cfg(feature = "irq-if")]
    fn irq_num() -> Option<usize> {
        Some(crate::config::devices::UART_IRQ)
    }
}
