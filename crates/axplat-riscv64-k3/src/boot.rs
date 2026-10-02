use axplat::mem::{Aligned4K, pa};

use crate::config::{
    devices::UART_PADDR,
    plat::{BOOT_STACK_SIZE, PHYS_VIRT_OFFSET},
};

#[unsafe(link_section = ".bss.stack")]
static mut BOOT_STACK: [u8; BOOT_STACK_SIZE] = [0; BOOT_STACK_SIZE];

#[unsafe(link_section = ".data")]
static mut BOOT_PT_SV39: Aligned4K<[u64; 512]> = Aligned4K::new([0; 512]);

// Standard Sv39 PTE flags: V|R|W|X|G|A|D. No T-Head attribute bits — the K3
// boot page table relies on standard Sv39 semantics (MS09 design D3).
const PTE_VRWX_GAD: u64 = 0xef;

#[allow(clippy::identity_op)] // (0x0 << 10) here makes sense because it's an address
unsafe fn init_boot_page_table() {
    unsafe {
        // Identity map, 1G blocks. The kernel payload lives at 0x1_8000_0000…
        // (1 GiB block 6) and UART0 at 0xd401_7000 (1 GiB block 3), both from
        // the MS09 Iteration 001 adjudicated safe window.
        //
        // 0x1_8000_0000..0x1_C000_0000, VRWX_GAD, 1G block (kernel + RAM window)
        BOOT_PT_SV39[6] = (0x1_8000_0000 >> 12 << 10) | PTE_VRWX_GAD;
        // 0xc000_0000..0x1_0000_0000, VRWX_GAD, 1G block (UART0 MMIO)
        BOOT_PT_SV39[3] = (0xc000_0000 >> 12 << 10) | PTE_VRWX_GAD;
        // High-half window at PHYS_VIRT_OFFSET = 0xffff_ffc0_0000_0000:
        // 0xffff_ffc1_8000_0000..0xffff_ffc1_c000_0000 (kernel link window)
        BOOT_PT_SV39[0x106] = (0x1_8000_0000 >> 12 << 10) | PTE_VRWX_GAD;
        // 0xffff_ffc0_c000_0000..0xffff_ffc1_0000_0000 (UART0 via phys_to_virt)
        BOOT_PT_SV39[0x103] = (0xc000_0000 >> 12 << 10) | PTE_VRWX_GAD;
    }
}

unsafe fn init_mmu() {
    unsafe {
        axcpu::asm::write_kernel_page_table(pa!(&raw const BOOT_PT_SV39 as usize));
        axcpu::asm::flush_tlb(None);
    }
}

/// Pre-runtime first-byte witness: a fixed marker written directly through
/// UART0 MMIO right after the boot page table and MMU are up.
///
/// Disambiguates the staged 3.3 board observation on the serial wire:
/// - marker absent entirely: boot did not jump, the boot page table faulted,
///   or UART0 clock/pinmux is dead;
/// - marker garbled: UART clock/baud mismatch;
/// - marker clean but no later output: axruntime or kernel entry failed.
unsafe fn boot_stage_marker() {
    unsafe {
        let uart = (PHYS_VIRT_OFFSET + UART_PADDR) as *mut u32;
        for byte in b"[k3:pt-mmu]\r\n" {
            while uart.add(5).read_volatile() & (1 << 5) == 0 {
                core::hint::spin_loop();
            }
            uart.write_volatile(*byte as u32);
        }
    }
}

/// The earliest entry point for the primary CPU.
#[unsafe(naked)]
#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.boot")]
unsafe extern "C" fn _start() -> ! {
    // PC = 0x1_8000_0000
    // a0 = hartid
    // a1 = dtb
    core::arch::naked_asm!("
        mv      s0, a0                  // save hartid
        mv      s1, a1                  // save DTB pointer
        la      sp, {boot_stack}
        li      t0, {boot_stack_size}
        add     sp, sp, t0              // setup boot stack

        call    {init_boot_page_table}
        call    {init_mmu}              // setup boot page table and enable MMU
        call    {boot_stage_marker}     // raw UART witness: PT + MMU + UART alive

        li      s2, {phys_virt_offset}  // fix up virtual high address
        add     sp, sp, s2

        mv      a0, s0
        mv      a1, s1
        la      a2, {entry}
        add     a2, a2, s2
        jalr    a2                      // call_main(cpu_id, dtb)
        j       .",
        phys_virt_offset = const PHYS_VIRT_OFFSET,
        boot_stack_size = const BOOT_STACK_SIZE,
        boot_stack = sym BOOT_STACK,
        init_boot_page_table = sym init_boot_page_table,
        init_mmu = sym init_mmu,
        boot_stage_marker = sym boot_stage_marker,
        entry = sym axplat::call_main,
    )
}
