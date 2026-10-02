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
pub mod qemu;
#[cfg(all(target_arch = "riscv64", feature = "lichee-d1"))]
pub mod smoke;
pub mod visionfive2;

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
