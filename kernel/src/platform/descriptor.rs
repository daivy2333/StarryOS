//! Platform descriptor — centralized board facts.
//!
//! Each supported platform provides a `pub const` instance of
//! [`PlatformDescriptor`] chosen at build time. Fields express
//! hardware layout, not driver capabilities or boot strategy.

use super::console::ConsoleConfig;

/// Optional VirtIO-MMIO network device fact.
///
/// When `Some`, the platform descriptor asserts a known VirtIO-MMIO
/// network device at the given address and interrupt line.  This is a
/// platform-level hardware fact, *not* a driver constant — the driver
/// still validates magic, version and device-id at runtime.
pub struct VirtioMmioNetConfig {
    /// MMIO base physical address.
    pub base_paddr: usize,
    /// MMIO region size in bytes.
    pub size: usize,
    /// Device ID reported by VirtIO header (1 for network card).
    pub device_id: u32,
    /// PLIC interrupt number.
    pub irq: usize,
}

/// Build-time platform descriptor chosen per target board.
pub struct PlatformDescriptor {
    /// Human-readable platform name (e.g. "qemu-virt", "lichee-rv-dock").
    pub name: &'static str,
    /// Physical memory layout (base address, size).
    pub memory: MemoryLayout,
    /// Kernel image load/link addresses.
    pub kernel: KernelImageLayout,
    /// Console UART configuration (kind, base, IRQ, stride, width).
    pub console: ConsoleConfig,
    /// Interrupt controller layout (PLIC base, etc.).
    pub interrupt: InterruptConfig,
    /// Timer hardware strategy (SBI, platform timer, etc.).
    pub timer: TimerConfig,
    /// How the kernel image is loaded (direct QEMU, Android boot image, U-Boot, etc.).
    pub boot: BootImageConfig,
    /// Optional VirtIO-MMIO network device fact.
    /// `None` means no known MMIO net device on this platform.
    pub virtio_net: Option<VirtioMmioNetConfig>,
}

/// Physical memory layout.
pub struct MemoryLayout {
    /// RAM base physical address.
    pub base_paddr: usize,
    /// Total RAM size in bytes.
    pub size: usize,
}

/// Kernel image load and link addresses.
pub struct KernelImageLayout {
    /// Physical address where the kernel image is loaded.
    pub load_paddr: usize,
    /// Virtual address where the kernel expects to run (link address).
    pub link_vaddr: usize,
}

/// Interrupt controller layout.
pub struct InterruptConfig {
    /// PLIC base physical address.
    pub plic_base_paddr: usize,
}

/// Timer hardware strategy.
///
/// Q18 uses minimal placeholder — full timer selection is deferred.
pub struct TimerConfig {
    /// Timer strategy hint: `"sbi"`, `"platform"`, etc.
    pub kind: &'static str,
}

/// How the kernel image is loaded onto the target.
pub struct BootImageConfig {
    /// Boot strategy kind.
    pub kind: BootKind,
}

/// Boot image strategy.
pub enum BootKind {
    /// Kernel loaded directly by QEMU (ELF or flat binary at load_paddr).
    DirectQemu,
    /// Android boot image format (kernel + ramdisk + cmdline in one image).
    AndroidImage,
    /// U-Boot FIT or legacy image format.
    UBootImage,
}
