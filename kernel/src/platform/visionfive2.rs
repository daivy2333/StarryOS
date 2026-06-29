//! VisionFive2 / StarFive JH7110 platform descriptor (compile-time only).
//!
//! Q18 does NOT execute on this hardware. This descriptor is a placeholder
//! for Q20 VisionFive2 UART verification.
//!
//! Facts verified against `axplat-riscv64-visionfive2/axconfig.toml` and
//! StarFive JH7110 datasheet:
//! - RAM: 2 GiB at 0x40000000
//! - Kernel load: 0x40200000
//! - Console: DW APB UART 0 at 0x10000000, IRQ 32, stride 4, 32-bit MMIO

use super::{
    console::{ConsoleConfig, ConsoleKind, MmioAccessWidth},
    descriptor::{
        BootImageConfig, BootKind, InterruptConfig, KernelImageLayout, MemoryLayout,
        PlatformDescriptor, TimerConfig,
    },
};

/// VisionFive2 / StarFive JH7110 platform descriptor.
///
/// Used in Q20 for UART verification. Q18 only proves this compiles.
pub const VISIONFIVE2: PlatformDescriptor = PlatformDescriptor {
    name: "visionfive2",
    memory: MemoryLayout {
        base_paddr: 0x40000000,
        size: 0x80000000, // 2 GiB
    },
    kernel: KernelImageLayout {
        load_paddr: 0x40200000,
        link_vaddr: 0xffffffff40200000,
    },
    console: ConsoleConfig {
        kind: ConsoleKind::DwApbUart,
        base_paddr: 0x10000000,
        irq: Some(32),
        reg_stride: 4,
        reg_width: MmioAccessWidth::U32,
        baud: 115200,
    },
    interrupt: InterruptConfig {
        plic_base_paddr: 0x0c000000,
    },
    timer: TimerConfig { kind: "platform" },
    boot: BootImageConfig {
        kind: BootKind::UBootImage,
    },
};
