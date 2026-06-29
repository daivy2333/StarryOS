//! QEMU virt (riscv64) platform descriptor.
//!
//! Matches the hardware facts that were previously hardcoded in
//! `kernel/src/drivers/uart_init.rs`. All values verified against
//! `axplat-riscv64-qemu-virt/axconfig.toml`.

use super::{
    console::{ConsoleConfig, ConsoleKind, MmioAccessWidth},
    descriptor::{
        BootImageConfig, BootKind, InterruptConfig, KernelImageLayout, MemoryLayout,
        PlatformDescriptor, TimerConfig,
    },
};

/// QEMU virt (riscv64) platform descriptor.
pub const QEMU_VIRT: PlatformDescriptor = PlatformDescriptor {
    name: "qemu-virt",
    memory: MemoryLayout {
        base_paddr: 0x80000000,
        size: 0x10000000, // 256 MiB
    },
    kernel: KernelImageLayout {
        load_paddr: 0x80200000,
        link_vaddr: 0xffffffff80200000,
    },
    console: ConsoleConfig {
        kind: ConsoleKind::Ns16550,
        base_paddr: 0x10000000,
        irq: Some(10),
        reg_stride: 1,
        reg_width: MmioAccessWidth::U8,
        baud: 115200,
    },
    interrupt: InterruptConfig {
        plic_base_paddr: 0x0c000000,
    },
    timer: TimerConfig { kind: "sbi" },
    boot: BootImageConfig {
        kind: BootKind::DirectQemu,
    },
};
