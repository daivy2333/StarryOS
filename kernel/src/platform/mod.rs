//! Platform descriptor and early console abstraction.
//!
//! Centralizes board-specific facts (memory layout, console UART config,
//! interrupt routing, boot strategy) behind a build-time descriptor,
//! decoupling driver code from platform constants.

pub mod console;
pub mod descriptor;
pub mod early_console;

pub use console::{ConsoleConfig, ConsoleKind, MmioAccessWidth};
pub use early_console::{DwApbUart32EarlyConsole, EarlyConsole, Ns16550U8EarlyConsole};
pub use descriptor::{
    BootImageConfig, BootKind, InterruptConfig, KernelImageLayout, MemoryLayout,
    PlatformDescriptor, TimerConfig,
};

pub mod lichee_d1;
pub mod qemu;
pub mod visionfive2;

/// Returns the build-time platform descriptor for the active target.
///
/// In Q18 this always returns the QEMU descriptor. Later milestones
/// (Q19, Q20) will select the appropriate descriptor via Cargo features.
pub fn descriptor() -> &'static PlatformDescriptor {
    &qemu::QEMU_VIRT
}
