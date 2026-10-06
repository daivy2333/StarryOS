//! Platform descriptor and early console abstraction.
//!
//! Centralizes board-specific facts (memory layout, console UART config,
//! interrupt routing, boot strategy) behind a build-time descriptor,
//! decoupling driver code from platform constants.

pub mod console;
pub mod descriptor;
pub mod early_console;

pub use console::{ConsoleConfig, ConsoleKind, MmioAccessWidth};
pub use descriptor::{
    BootImageConfig, BootKind, InterruptConfig, KernelImageLayout, MemoryLayout,
    PlatformDescriptor, TimerConfig, VirtioMmioNetConfig,
};
pub use early_console::{DwApbUart32EarlyConsole, EarlyConsole, Ns16550U8EarlyConsole};

pub mod k3;
pub mod lichee_d1;
pub mod mac_probe;
pub mod phy_probe;
pub mod qemu;
#[cfg(all(target_arch = "riscv64", feature = "lichee-d1"))]
pub mod smoke;
pub mod visionfive2;

/// K3 stage 9 AIA software-injection probe (MS10 Iteration 000); the module
/// binds the kernel's optional `axplat-riscv64-k3` dependency for the
/// production backend and chip observation seam.
#[cfg(feature = "k3")]
pub mod aia_probe;

/// K3 stage 10 PHY device IRQ/status probe (MS10 Iteration 001): the D5
/// cause-snapshot-before-ack core and the device window on the RAM boot path.
#[cfg(feature = "k3")]
pub mod k3_device_irq_probe;

/// K3 GMAC polling driver (MS11 Iteration 000): exclusive-arena DMA
/// foundation, bounded RX and the stage 11 foreground integration. Lives in
/// `platform` because the k3 build shape excludes the `drivers` tree (the
/// k3 image is the minimal park-path kernel).
#[cfg(feature = "k3")]
pub mod k3_gmac;

/// K3 stage 11 raw GMAC RX observation window (MS11 Iteration 000, task
/// 1.3): the stage 10 gate, route masks and the polling driver foreground.
#[cfg(feature = "k3")]
pub mod k3_network_probe;

#[cfg(all(
    feature = "qemu",
    any(feature = "lichee-d1", feature = "lichee-d1-async-uart")
))]
compile_error!("features `qemu` and lichee-d1 variants cannot be enabled together");

#[cfg(all(feature = "k3", feature = "qemu"))]
compile_error!("features `k3` and `qemu` cannot be enabled together");

#[cfg(all(
    feature = "k3",
    any(feature = "lichee-d1", feature = "lichee-d1-async-uart")
))]
compile_error!("features `k3` and lichee-d1 variants cannot be enabled together");

/// Returns the build-time platform descriptor for the active target.
pub fn descriptor() -> &'static PlatformDescriptor {
    #[cfg(feature = "k3")]
    {
        &k3::K3
    }
    #[cfg(all(not(feature = "k3"), feature = "lichee-d1"))]
    {
        &lichee_d1::LICHEE_D1
    }
    #[cfg(all(not(feature = "k3"), not(feature = "lichee-d1")))]
    {
        &qemu::QEMU_VIRT
    }
}
