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
    PlatformDescriptor, TimerConfig,
};
pub use early_console::{DwApbUart32EarlyConsole, EarlyConsole, Ns16550U8EarlyConsole};

pub mod lichee_d1;
pub mod qemu;
#[cfg(all(target_arch = "riscv64", feature = "lichee-d1"))]
pub mod smoke;
pub mod visionfive2;

#[cfg(all(
    feature = "qemu",
    any(feature = "lichee-d1", feature = "lichee-d1-async-uart")
))]
compile_error!("features `qemu` and lichee-d1 variants cannot be enabled together");

/// Returns the build-time platform descriptor for the active target.
pub fn descriptor() -> &'static PlatformDescriptor {
    #[cfg(feature = "lichee-d1")]
    {
        &lichee_d1::LICHEE_D1
    }
    #[cfg(not(feature = "lichee-d1"))]
    {
        &qemu::QEMU_VIRT
    }
}
