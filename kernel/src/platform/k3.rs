//! SpacemiT K3 Com260 IFX platform descriptor (compile-time only).
//!
//! Facts come from the MS09 Iteration 001 board adjudication (change
//! `ms09-com260-kit-observable-link-baseline`, Iteration 001 Act Response,
//! tasks 2.1–2.3, `evidence/001-board-ram-boundary/`) cross-checked with the
//! shipped `k3_com260_ifx.dtb`:
//! - UART0: 0xd4017000, reg-shift=2 (stride 4), reg-io-width=4 (U32 access);
//!   clock/reset/pinctrl stay in the bootloader handoff state.
//! - Safe RAM window [0x160000000, 0x2f0000000): above the CMA pool
//!   (0x140000000 + 512 MiB), below SWIOTLB (0x2f0e00000), the framebuffer
//!   reservation (0x2fe000000) and the observed U-Boot working area
//!   (>= 0x2f9700000). Kernel payload >= 0x180000000.
//! - Boot: U-Boot FIT from RAM; recovery path = normal reset back to the
//!   stock system.
//! - No PLIC/AIA base is asserted: MS09 keeps IRQ delivery fail-closed
//!   (`irq-if` stub only); APLIC/IMSIC addresses are recorded facts, not
//!   wired.

use super::{
    console::{ConsoleConfig, ConsoleKind, MmioAccessWidth},
    descriptor::{
        BootImageConfig, BootKind, InterruptConfig, KernelImageLayout, MemoryLayout,
        PlatformDescriptor, TimerConfig,
    },
    early_console::{DwApbUart32EarlyConsole, EarlyConsole},
};

/// SpacemiT K3 Com260 IFX platform descriptor.
pub const K3: PlatformDescriptor = PlatformDescriptor {
    name: "spacemit-k3-com260-ifx",
    memory: MemoryLayout {
        base_paddr: 0x1_6000_0000,
        size: 0x1_9000_0000, // adjudicated safe window, not total DRAM
    },
    kernel: KernelImageLayout {
        load_paddr: 0x1_8000_0000,
        link_vaddr: 0xffff_ffc1_8000_0000,
    },
    console: ConsoleConfig {
        kind: ConsoleKind::DwApbUart,
        base_paddr: 0xd401_7000,
        irq: None, // polling-only first-byte path; IRQ delivery stays fail-closed
        reg_stride: 4,
        reg_width: MmioAccessWidth::U32,
        baud: 115200,
    },
    interrupt: InterruptConfig {
        // 0 is the explicit "not brought up" sentinel: no PLIC/AIA base is
        // asserted for MS09 (fail-closed irq-if stub only).
        plic_base_paddr: 0,
    },
    timer: TimerConfig { kind: "sbi" },
    boot: BootImageConfig {
        kind: BootKind::UBootImage,
    },
    virtio_net: None,
};

/// K3 first-byte parking path: staged polling markers, then wfi.
///
/// Runs after axruntime early init. Every marker goes through the polling
/// early console so it does not depend on the log level or the async stack.
/// Combined with the `[k3:pt-mmu]` marker emitted by the axplat boot code,
/// the serial wire shows (3.3 staged observation):
/// - nothing at all: boot did not jump, the boot page table faulted, or
///   UART0 clock/pinmux is dead;
/// - `[k3:pt-mmu]` only: page table + MMU + UART MMIO alive, but axruntime
///   or kernel entry init failed;
/// - stages 1..3: kernel entry reached, K3 descriptor dispatch confirmed
///   (printed facts), controlled park entered;
/// - stage 4 (MS09 Iteration 003 rework): APMU GMAC bus clock-enable
///   sequence lines (begin / raw readbacks / done). `apmu begin` without
///   following lines means the APMU access faulted (mapping layer);
///   readbacks whose bit0/bit1 stay clear after step 2 mean the APMU write
///   was not effective (syscon write layer);
/// - stage 5 (MS09 Iteration 003): read-only target-MAC identity/status
///   probe lines; `probe begin` without register lines after it falsifies
///   the clock-gating assumption, all-zero/all-one reads point at a deeper
///   clock/reset layer.
///
/// # Safety
///
/// The caller MUST ensure the UART0 MMIO window is mapped (identity mapping
/// of the `devices.mmio-ranges` window by axruntime) and UART0 clock/pins
/// are in the bootloader handoff state (MS09 Iteration 001 adjudication).
pub unsafe fn run_first_byte_park() -> ! {
    let desc = super::descriptor();
    // SAFETY: K3 UART0 is in the bootloader handoff state (see contract above).
    let console = unsafe { DwApbUart32EarlyConsole::from_config(&desc.console) };

    console.write_str("[starry-k3] stage=1 entry-reached\n");
    console.write_str(
        "[starry-k3] stage=2 console uart=0xd4017000 stride=4 width=u32 baud=115200\n",
    );
    console.write_str("[starry-k3] stage=3 parked, halting (reset to recover)\n");
    // SAFETY: APMU and MAC windows are identity-mapped via devices.mmio-ranges;
    // the APMU sequence writes only CTRL 0x3ec, the MAC probe reads only
    // (see mac_probe).
    unsafe {
        super::mac_probe::apmu_clock_enable(|line| console.write_str(line));
        super::mac_probe::probe_and_report(|line| console.write_str(line));
    }
    loop {
        riscv::asm::wfi();
    }
}
