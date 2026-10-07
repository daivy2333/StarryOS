use axplat::console::ConsoleIf;

use crate::config::{devices::UART_PADDR, plat::PHYS_VIRT_OFFSET};

use core::sync::atomic::{AtomicBool, Ordering};

pub(crate) fn init_early() {}

// ---------------------------------------------------------------------------
// Shared UART hardware guard (MS17 Iteration 000, task T2)
// ---------------------------------------------------------------------------

static UART_HW_LOCKED: AtomicBool = AtomicBool::new(false);

/// Runs `f` while holding the K3 UART0 hardware lock (IRQ-save).
///
/// The async PXA port (`k3_uart` in the kernel) serializes every register
/// batch — IER RMW with the LSR error latch, ISR cause classification —
/// through this same lock, so early/fault console writes and port accesses
/// can never interleave on the one hardware instance and console `LSR` polls
/// can never consume a line status the port still has to record. IRQs stay
/// disabled for the whole section, which on the single-hart bring-up image
/// also makes the lock uncontended by construction.
pub fn with_uart_hardware<R>(f: impl FnOnce() -> R) -> R {
    let saved = axcpu::asm::irqs_enabled();
    axcpu::asm::disable_irqs();
    while UART_HW_LOCKED.swap(true, Ordering::Acquire) {
        core::hint::spin_loop();
    }
    let r = f();
    UART_HW_LOCKED.store(false, Ordering::Release);
    // Restore exactly as found; never unconditionally re-enable.
    if saved {
        axcpu::asm::enable_irqs();
    }
    r
}

struct ConsoleIfImpl;

#[impl_plat_interface]
impl ConsoleIf for ConsoleIfImpl {
    fn write_bytes(bytes: &[u8]) {
        // Every console output line shares the UART0 MMIO window with the
        // async port once it is activated; serialize through the guard.
        with_uart_hardware(|| write_bytes_polled(bytes));
    }

    fn read_bytes(_bytes: &mut [u8]) -> usize {
        0
    }

    #[cfg(feature = "irq-if")]
    fn irq_num() -> Option<usize> {
        Some(crate::config::devices::UART_IRQ)
    }
}

fn write_bytes_polled(bytes: &[u8]) {
    let uart_base = (PHYS_VIRT_OFFSET + UART_PADDR) as *mut u32;
    for &c in bytes {
        match c {
            b'\n' => {
                // SAFETY: K3 UART0 is left in the bootloader handoff state
                // (clock/pinctrl configured by U-Boot, MS09 Iteration 001).
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
