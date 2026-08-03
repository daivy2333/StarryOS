//! Lichee RV Dock / Allwinner D1 platform descriptor (compile-time only).
//!
//! Q18 does NOT execute on this hardware. This descriptor is a placeholder
//! for Q19 Lichee RV Dock early smoke test.
//!
//! Facts verified against public platform notes and Allwinner D1 datasheet:
//! - RAM: 512 MiB at 0x40000000
//! - Kernel load: 0x40200000 (Android boot image convention)
//! - Console: DW APB UART 0 at 0x02500000, IRQ 18, stride 4, 32-bit MMIO

use super::{
    console::{ConsoleConfig, ConsoleKind, MmioAccessWidth},
    descriptor::{
        BootImageConfig, BootKind, InterruptConfig, KernelImageLayout, MemoryLayout,
        PlatformDescriptor, TimerConfig,
    },
};

/// Lichee RV Dock / Allwinner D1 platform descriptor.
///
/// Used in Q19 for early smoke test. Q18 only proves this compiles.
pub const LICHEE_D1: PlatformDescriptor = PlatformDescriptor {
    name: "lichee-rv-dock",
    memory: MemoryLayout {
        base_paddr: 0x40000000,
        size: 0x20000000, // 512 MiB
    },
    kernel: KernelImageLayout {
        load_paddr: 0x4020_0000,
        link_vaddr: 0xffffffc0_4020_0000,
    },
    console: ConsoleConfig {
        kind: ConsoleKind::DwApbUart,
        base_paddr: 0x0250_0000,
        irq: None, // early console is polling-only; IRQ deferred
        reg_stride: 4,
        reg_width: MmioAccessWidth::U32,
        baud: 115200,
    },
    interrupt: InterruptConfig {
        plic_base_paddr: 0x10000000,
    },
    timer: TimerConfig { kind: "sbi" },
    boot: BootImageConfig {
        kind: BootKind::AndroidImage,
    },
    virtio_net: None,
};
